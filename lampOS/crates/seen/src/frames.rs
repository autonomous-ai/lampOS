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
//!
//! The sink is asked before each request ([`Sink::ready`]) and told when
//! sending stops ([`Sink::stopped`]): the ring's mark
//! ([`crate::mark::Marked`]) wraps the sink there, so no frame is asked for
//! or passed, to `Discard` too, unless the ring said it drew the mark.
//!
//! The link is read by a thread of its own ([`Drained`]), never by the
//! serve loop: the camera closes a reader that leaves a frame unread for
//! 0.5 s (`STALL_DEADLINE_US`), its kernel buffer holds about 16 KiB, and a
//! frame of the eyes' full look is several times that, so a loop that
//! reads once a 100 ms tick, and longer while a frame is scaled for
//! sending, took the frame too slowly and lost the link mid-burst. The
//! thread copies a whole frame into one slot of its own, which the next
//! step takes and wipes; a loss, the gate closing or a stop wipes it too.
use crate::gate::Closed;
use lamp_camera::frames::{FrameClient, FrameMeta};
use lamp_turn_owner::CameraGrant;
use std::io;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;
use zeroize::Zeroize;

/// The newest frame asked for may be at most this old when the camera
/// copies it.
pub const MAX_AGE_US: u32 = 500_000;
/// One frame asked for at most this often, unless a taker is made with
/// its own rate ([`Taker::with_every`]: activity's bursts).
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

/// How often the reader thread reads the link while a frame is asked for:
/// a frame of a few hundred KiB, 16 KiB a read, comes well within the
/// camera's 0.5 s.
pub const DRAIN_EVERY: Duration = Duration::from_millis(2);
/// How long the reader thread waits with nothing asked for, unless a
/// request or a stop wakes it.
const IDLE_WAIT: Duration = Duration::from_millis(100);

/// What the reader thread and the serve loop share.
struct Shared<S> {
    source: S,
    /// The thread's one copy: a whole frame, until the next step takes it.
    jpeg: Vec<u8>,
    frame: Option<FrameMeta>,
    /// The link failed while the thread read it: told at the next step.
    failed: Option<io::Error>,
    stop: bool,
}

impl<S: Source> Shared<S> {
    fn wipe(&mut self) {
        self.jpeg.zeroize();
        self.frame = None;
    }
}

/// A [`Source`] whose link is read by a thread of its own, so it is
/// drained however long the serve loop's step takes (the camera's
/// 0.5 s stall rule). The thread only reads while a request is out, and
/// only until a whole frame has come; that frame waits in one slot,
/// copied, for the next [`Source::poll`], which moves it into the
/// taker's buffer and wipes the slot. A failure drops the connection at
/// once and is said at the next step; a disconnect (the gate closing, a
/// loss, a stop) wipes the slot. Dropped, the thread stops and the slot
/// is wiped.
pub struct Drained<S: Source> {
    shared: Arc<(Mutex<Shared<S>>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl<S: Source + Send + 'static> Drained<S> {
    pub fn new(source: S) -> io::Result<Self> {
        let shared = Arc::new((
            Mutex::new(Shared {
                source,
                jpeg: Vec::new(),
                frame: None,
                failed: None,
                stop: false,
            }),
            Condvar::new(),
        ));
        let theirs = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("seen-frames".into())
            .spawn(move || drain(&theirs))?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }
}

impl<S: Source> Drained<S> {
    fn lock(&self) -> MutexGuard<'_, Shared<S>> {
        self.shared
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// The reader thread: reads while a request is out and no whole frame
/// waits, every [`DRAIN_EVERY`].
fn drain<S: Source>(shared: &(Mutex<Shared<S>>, Condvar)) {
    let (lock, wake) = shared;
    let mut held = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    while !held.stop {
        let reading = held.failed.is_none()
            && held.frame.is_none()
            && held.source.connected()
            && held.source.outstanding().is_some();
        if reading {
            let state = &mut *held;
            match state.source.poll(&mut state.jpeg) {
                Ok(Some(meta)) => state.frame = Some(meta),
                Ok(None) => {}
                Err(error) => {
                    state.source.disconnect();
                    state.wipe();
                    state.failed = Some(error);
                }
            }
        }
        let wait = if reading { DRAIN_EVERY } else { IDLE_WAIT };
        held = wake
            .wait_timeout(held, wait)
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .0;
    }
}

impl<S: Source> Source for Drained<S> {
    fn connected(&self) -> bool {
        let held = self.lock();
        // A failure not yet said keeps it "connected" until the step's
        // poll says it.
        held.source.connected() || held.failed.is_some() || held.frame.is_some()
    }
    fn connect(&mut self) -> io::Result<bool> {
        let mut held = self.lock();
        if let Some(error) = held.failed.take() {
            return Err(error);
        }
        held.source.connect()
    }
    fn outstanding(&self) -> Option<u64> {
        let held = self.lock();
        held.source
            .outstanding()
            .or(held.frame.as_ref().map(|meta| meta.request))
    }
    fn request(&mut self, max_age_us: u32) -> io::Result<u64> {
        let id = self.lock().source.request(max_age_us)?;
        self.shared.1.notify_all();
        Ok(id)
    }
    fn poll(&mut self, jpeg: &mut Vec<u8>) -> io::Result<Option<FrameMeta>> {
        let mut held = self.lock();
        if let Some(error) = held.failed.take() {
            return Err(error);
        }
        let Some(meta) = held.frame.take() else {
            return Ok(None);
        };
        jpeg.clear();
        jpeg.extend_from_slice(&held.jpeg);
        held.jpeg.zeroize();
        Ok(Some(meta))
    }
    fn disconnect(&mut self) {
        let mut held = self.lock();
        held.source.disconnect();
        held.wipe();
        held.failed = None;
    }
}

impl<S: Source> Drop for Drained<S> {
    fn drop(&mut self) {
        {
            let mut held = self.lock();
            held.stop = true;
            held.source.disconnect();
            held.wipe();
        }
        self.shared.1.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Where a copied frame goes. Its bytes are lent for the call only and
/// wiped after it.
pub trait Sink {
    /// Whether a frame may be asked for at `now_us`: what must come first
    /// (the ring's mark) is in place. Asked each step a request is due; no
    /// means no request this step.
    fn ready(&mut self, _now_us: u64) -> bool {
        true
    }
    /// The oldest a frame asked for at `now_us` may be when the camera
    /// copies it, once [`Sink::ready`] said yes: [`MAX_AGE_US`] at most,
    /// and at least 1.
    fn max_age_us(&mut self, _now_us: u64) -> u32 {
        MAX_AGE_US
    }
    /// A frame copied at `now_us`, lent for the call only.
    fn frame(&mut self, now_us: u64, meta: &FrameMeta, jpeg: &[u8]);
    /// Sending stopped: the gate closed, the link was lost, the grant
    /// changed or the process stops. Called again harmlessly.
    fn stopped(&mut self) {}
}

/// Phase 1's sink: the frame goes nowhere. Phase 2's sender (encrypted,
/// [`crate::crypto`], under [`crate::backoff`]) takes its place here.
#[derive(Clone, Copy, Debug, Default)]
pub struct Discard;

impl Sink for Discard {
    fn frame(&mut self, _: u64, _: &FrameMeta, _: &[u8]) {}
}

/// What one step did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Nothing to do: the gate closed with nothing held, connecting, or
    /// waiting for a frame or the next request's time.
    Idle,
    /// The gate closed: everything held was dropped.
    Dropped(Closed),
    /// A frame was due, but the sink is not ready for one (the ring's mark
    /// not drawn yet): none asked for.
    NotReady,
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
    every_us: u64,
    counts: Counts,
}

impl<S: Source> Taker<S> {
    pub fn new(source: S) -> Self {
        Self::with_every(source, EVERY_US)
    }

    /// One frame asked for at most every `every_us` (activity's burst
    /// spacing, `crate::activity::SPACING_US`); the sink's
    /// [`Sink::ready`] says when a frame is wanted at all.
    pub fn with_every(source: S, every_us: u64) -> Self {
        Self {
            source,
            jpeg: Vec::new(),
            grant: None,
            next_request_us: 0,
            every_us,
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
            sink.stopped();
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
                sink.stopped();
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
                sink.stopped();
                self.counts.grant_changed = self.counts.grant_changed.saturating_add(1);
                return Ok(Step::GrantChanged);
            }
            self.grant = Some(meta.grant);
            sink.frame(now_us, &meta, &self.jpeg);
            self.jpeg.zeroize();
            self.next_request_us = now_us.saturating_add(self.every_us);
            self.counts.handed = self.counts.handed.saturating_add(1);
            return Ok(Step::Handed);
        }
        if self.source.outstanding().is_none() && now_us >= self.next_request_us {
            if !sink.ready(now_us) {
                return Ok(Step::NotReady);
            }
            let max_age_us = sink.max_age_us(now_us).clamp(1, MAX_AGE_US);
            self.source.request(max_age_us)?;
            self.counts.requested = self.counts.requested.saturating_add(1);
            return Ok(Step::Requested);
        }
        Ok(Step::Idle)
    }
}

#[cfg(test)]
pub(crate) mod tests;
