//! The frames: a client of the camera's frame link (`lamp_camera::frames`,
//! the `perception` role), kept to that module's rules (#6543):
//!
//! - **One request at a time**, and only while the gate ([`crate::gate`])
//!   is open in this same step; a frame that arrives is used only under it.
//! - **Copy first**: the client copies the frame into this part's one
//!   buffer before anything else happens to it, and nothing here borrows
//!   the camera's bytes.
//! - **The grant kept and checked**: the first frame's `CameraGrant` is
//!   kept; a frame under another grant means the camera's privacy lineage
//!   changed, and it is dropped with everything else.
//! - **On a privacy close, drop everything**: the gate closed for any
//!   reason, the link closed by the camera (its privacy close closes every
//!   consumer), or an error: the connection goes (its partial frame with
//!   it), the request in flight is forgotten, the copy is wiped and the
//!   grant let go.
//!
//! Each copy is handed to a [`Sink`]: the one place a frame would go to
//! the server. In phase 1 that is [`Discard`], and the copy is wiped right
//! after. Nothing is kept, on disk or in memory, past the step it came in.
use crate::gate::Closed;
use lamp_camera::frames::{FrameClient, FrameMeta};
use lamp_turn_owner::CameraGrant;
use std::io;
use zeroize::Zeroize;

/// The newest frame asked for may be at most this old when the camera
/// copies it.
pub const MAX_AGE_US: u32 = 500_000;
/// One frame asked for at most this often (phase 2 sets each sense's own
/// rate: activity's clips, posture's one in 30 s).
pub const EVERY_US: u64 = 5_000_000;

/// The camera's end, as this part uses it: [`FrameClient`] in the product,
/// a stand-in in tests.
pub trait Source {
    fn connected(&self) -> bool;
    /// One nonblocking step of connecting: `Ok(true)` once connected.
    fn connect(&mut self) -> io::Result<bool>;
    fn outstanding(&self) -> Option<u64>;
    fn request(&mut self, max_age_us: u32) -> io::Result<u64>;
    /// A complete frame copied into `jpeg`, if one has come.
    fn poll(&mut self, jpeg: &mut Vec<u8>) -> io::Result<Option<FrameMeta>>;
    /// Drop the connection, its partial frame and the request in flight.
    fn disconnect(&mut self);
}

impl Source for FrameClient {
    fn connected(&self) -> bool {
        self.is_connected()
    }
    fn connect(&mut self) -> io::Result<bool> {
        FrameClient::connect(self)
    }
    fn outstanding(&self) -> Option<u64> {
        FrameClient::outstanding(self)
    }
    fn request(&mut self, max_age_us: u32) -> io::Result<u64> {
        FrameClient::request(self, max_age_us)
    }
    fn poll(&mut self, jpeg: &mut Vec<u8>) -> io::Result<Option<FrameMeta>> {
        FrameClient::poll(self, jpeg)
    }
    fn disconnect(&mut self) {
        FrameClient::disconnect(self)
    }
}

/// Where a copied frame goes. Its bytes are lent for the call only and
/// wiped after it.
pub trait Sink {
    fn frame(&mut self, meta: &FrameMeta, jpeg: &[u8]);
}

/// Phase 1's sink: the frame goes nowhere. Phase 2's sender (encrypted,
/// [`crate::crypto`], under [`crate::backoff`]) takes its place here.
#[derive(Clone, Copy, Debug, Default)]
pub struct Discard;

impl Sink for Discard {
    fn frame(&mut self, _: &FrameMeta, _: &[u8]) {}
}

/// What one step did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Nothing to do: the gate closed with nothing held, connecting, or
    /// waiting for a frame or the next request's time.
    Idle,
    /// The gate closed: everything held was dropped.
    Dropped(Closed),
    /// A frame was asked for.
    Requested,
    /// A frame was copied and handed to the sink, then wiped.
    Handed,
    /// A frame came under another grant: dropped, with everything else.
    GrantChanged,
    /// The link closed or failed (the camera's privacy close among the
    /// causes): everything dropped.
    Lost,
}

/// Saturating counts, for `diagnostics`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub requested: u64,
    pub handed: u64,
    pub grant_changed: u64,
    pub lost: u64,
    pub dropped: u64,
}

/// The frame client and the one buffer a copy lives in.
pub struct Taker<S> {
    source: S,
    jpeg: Vec<u8>,
    grant: Option<CameraGrant>,
    next_request_us: u64,
    counts: Counts,
}

impl<S: Source> Taker<S> {
    pub fn new(source: S) -> Self {
        Self {
            source,
            jpeg: Vec::new(),
            grant: None,
            next_request_us: 0,
            counts: Counts::default(),
        }
    }

    pub fn counts(&self) -> Counts {
        self.counts
    }

    /// Whether anything is held: a connection, a request in flight, a grant
    /// or bytes.
    pub fn holds_anything(&self) -> bool {
        self.source.connected()
            || self.source.outstanding().is_some()
            || self.grant.is_some()
            || !self.jpeg.is_empty()
    }

    /// Drop everything: the connection with its partial frame, the request
    /// in flight, the grant and the copy, wiped.
    pub fn drop_everything(&mut self) {
        self.source.disconnect();
        self.jpeg.zeroize();
        self.grant = None;
    }

    /// One step at `now_us` under the gate's word for this step.
    pub fn step(&mut self, gate: Result<(), Closed>, now_us: u64, sink: &mut impl Sink) -> Step {
        if let Err(closed) = gate {
            if !self.holds_anything() {
                return Step::Idle;
            }
            self.drop_everything();
            self.counts.dropped = self.counts.dropped.saturating_add(1);
            return Step::Dropped(closed);
        }
        match self.open_step(now_us, sink) {
            Ok(step) => step,
            Err(_) => {
                self.drop_everything();
                self.counts.lost = self.counts.lost.saturating_add(1);
                Step::Lost
            }
        }
    }

    fn open_step(&mut self, now_us: u64, sink: &mut impl Sink) -> io::Result<Step> {
        if !self.source.connected() && !self.source.connect()? {
            return Ok(Step::Idle);
        }
        // A frame is copied into this part's buffer by the client itself.
        if let Some(meta) = self.source.poll(&mut self.jpeg)? {
            if self.grant.is_some_and(|kept| kept != meta.grant) {
                self.drop_everything();
                self.counts.grant_changed = self.counts.grant_changed.saturating_add(1);
                return Ok(Step::GrantChanged);
            }
            self.grant = Some(meta.grant);
            sink.frame(&meta, &self.jpeg);
            self.jpeg.zeroize();
            self.next_request_us = now_us.saturating_add(EVERY_US);
            self.counts.handed = self.counts.handed.saturating_add(1);
            return Ok(Step::Handed);
        }
        if self.source.outstanding().is_none() && now_us >= self.next_request_us {
            self.source.request(MAX_AGE_US)?;
            self.counts.requested = self.counts.requested.saturating_add(1);
            return Ok(Step::Requested);
        }
        Ok(Step::Idle)
    }
}

#[cfg(test)]
mod tests;
