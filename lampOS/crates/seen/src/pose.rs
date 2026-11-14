//! Posture (#6543 phase 3): one desk frame every 30 s to the perception
//! service's pose model, its RULA score kept as a rolling window of this
//! sitting, said on `humans/posture`.
//!
//! - **Only through the frames' rules**: [`Pose`] is a [`Sink`] behind the
//!   ring's mark ([`crate::mark::Marked`]), so a frame reaches it only while
//!   the gate is open and the ring drew the mark for it. It is ready for one
//!   only every [`EVERY_US`] (V1's `POSE_SAMPLE_INTERVAL_S`), with no
//!   request running and the retreat not holding it back; not ready,
//!   no mark is asked and no frame is taken.
//! - **Only once posture was said**: its gate ([`gate`]) is the frames'
//!   gate, closed `not_said_yet` too until an on line naming posture
//!   ([`crate::said_back::POSTURE_ON_LINE`]) has begun for the kept yes.
//!   Activity's line alone opens activity's gate, not posture's.
//! - **Only the camera's own JPEG** ([`crate::activity`]'s rule: MJPG, at
//!   least 320×240, not derived), **sent at 320×240** ([`crate::scale`]:
//!   a larger frame is decoded, scaled down and encoded again, every
//!   buffer wiped). Another frame, or one that does not decode at its
//!   size, is skipped, counted `wrong_frame`, with no request and no
//!   backoff.
//! - **One socket per frame** ([`client`]): opened, the key exchange, V1's
//!   config, the frame, its answer, closed, on a thread of its own so the
//!   gate and the ring never wait. The frame's Base64 is wiped when the
//!   request ends.
//! - **The gate closing stops it** ([`Sink::stopped`]): the request is
//!   cancelled, its socket closed within [`client::POLL`] of a read (a
//!   connect or write already begun runs to its 15 s bound; none begins
//!   after the cancel), a frame not yet sent never sent, and whatever comes
//!   back is dropped.
//! - **Busy and failures**, as activity's bursts take them
//!   ([`crate::backoff::Retreat`]): busy drops the frame; three busy in a
//!   row or any failure pause 60 s doubling to 10 min with jitter, and
//!   `diagnostics` says `warn` until an answer.
//! - **The window** ([`window`]): the share of bad samples and the region
//!   most often bad over the last ten minutes, by V1's thresholds. It is
//!   forgotten whenever the gate closes, for any reason (the privacy, sleep
//!   and not known included), and on `room/stood_up`. No frame and no
//!   per-person history is kept.
//! - **Said** on `humans/posture` ([`HumanPosture`]) every 30 s while the
//!   gate is open and the window holds at least [`window::MIN_SAMPLES`]
//!   samples: from the third sample of a sitting. The gate closed, nothing
//!   is said, so no old sample is said under the privacy. Under three
//!   samples (frames not going out, the server busy, no body found, a
//!   wipe, or the sitting just begun), nothing is said either: the topic
//!   runs out, and a skill holding on it runs as it did.
//!
//! Off unless `LAMP_SEEN_POSTURE=on` beside `LAMP_SEEN_ACTIVITY=on`.
pub mod client;
pub mod window;

use crate::backoff::{Ended, Retreat};
use crate::frames::Sink;
use crate::gate::Closed;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use client::{Client, Failure, Outcome};
use lamp_camera::frames::FrameMeta;
use lamp_msgs::HumanPosture;
use rand_core::RngCore;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;
use window::Window;
use zeroize::Zeroizing;

/// The sub-switch: posture runs only with `on` exactly, and only beside
/// [`crate::ENVIRONMENT`].
pub const ENVIRONMENT: &str = "LAMP_SEEN_POSTURE";

/// One frame at most this often (V1's `POSE_SAMPLE_INTERVAL_S`).
pub const EVERY_US: u64 = 30_000_000;

/// Posture's gate: the frames' gate, and closed `not_said_yet` until an on
/// line naming posture has begun for the kept yes
/// ([`crate::said_back::SaidBack::said_posture`]).
pub fn gate(frames: Result<(), Closed>, said_posture: bool) -> Result<(), Closed> {
    frames.and(if said_posture {
        Ok(())
    } else {
        Err(Closed::NotSaidYet)
    })
}

fn us(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

/// Saturating counts, for `diagnostics`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Requests begun (a frame handed to a socket's thread).
    pub sent: u64,
    pub answered: u64,
    /// Answered with no body found: no sample.
    pub no_body: u64,
    pub busy: u64,
    pub failed: u64,
    /// Stopped by the gate closing; their answers dropped.
    pub cancelled: u64,
    /// Frames that came while not ready: dropped unsent.
    pub dropped: u64,
    /// Windows forgotten.
    pub resets: u64,
    /// Frames not the camera's own JPEG: skipped, no request, no backoff.
    pub wrong_frame: u64,
}

struct Job {
    cancel: Arc<AtomicBool>,
    done: mpsc::Receiver<Outcome>,
    started_us: u64,
}

/// A jitter draw from 0 to 1.
pub type Jitter = Box<dyn FnMut() -> f64 + Send>;

fn system_jitter() -> f64 {
    f64::from(rand_core::OsRng.next_u32()) / f64::from(u32::MAX)
}

/// Posture's sender, window and backoff.
pub struct Pose {
    client: Arc<Client>,
    window: Window,
    retreat: Retreat,
    job: Option<Job>,
    next_due_us: u64,
    next_said_us: u64,
    /// When the gate was last open in this sitting.
    open_at_us: Option<u64>,
    /// Posture's gate was open at the last step: no frame is taken
    /// otherwise.
    open: bool,
    last_failure: Option<Failure>,
    counts: Counts,
    jitter: Jitter,
}

impl Pose {
    pub fn new(client: Client) -> Self {
        Self::with_jitter(client, Box::new(system_jitter))
    }

    pub fn with_jitter(client: Client, jitter: Jitter) -> Self {
        Self {
            client: Arc::new(client),
            window: Window::default(),
            retreat: Retreat::default(),
            job: None,
            next_due_us: 0,
            next_said_us: 0,
            open_at_us: None,
            open: false,
            last_failure: None,
            counts: Counts::default(),
            jitter,
        }
    }

    pub fn counts(&self) -> Counts {
        self.counts
    }

    /// Whether `diagnostics` says `warn`: held back, until an answer.
    pub fn warn(&self) -> bool {
        self.retreat.warn()
    }

    /// Held back until when, while held back at `now_us`.
    pub fn paused_until(&self, now_us: u64) -> Option<u64> {
        self.retreat.until_us().filter(|until| now_us < *until)
    }

    /// Whether a request is running (or cancelled and not yet ended).
    pub fn in_flight(&self) -> bool {
        self.job.is_some()
    }

    pub fn last_failure(&self) -> Option<Failure> {
        self.last_failure
    }

    /// The samples in the window now.
    pub fn samples(&self) -> usize {
        self.window.len()
    }

    /// Stop the running request, if any: its answer is dropped.
    fn cancel(&mut self) {
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Take a request's end, if it came.
    fn collect(&mut self, now_us: u64) {
        let Some(job) = &self.job else {
            return;
        };
        let outcome = match job.done.try_recv() {
            Ok(outcome) => outcome,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Outcome::Failed(Failure::Closed),
        };
        let cancelled = job.cancel.load(Ordering::Relaxed);
        let started_us = job.started_us;
        self.job = None;
        if cancelled || matches!(outcome, Outcome::Cancelled { .. }) {
            self.counts.cancelled = self.counts.cancelled.saturating_add(1);
            return;
        }
        let jitter = (self.jitter)();
        match outcome {
            Outcome::Answered(ergo) => {
                self.counts.answered = self.counts.answered.saturating_add(1);
                self.retreat.take(Ended::Answered, now_us, jitter);
                self.last_failure = None;
                match ergo {
                    Some(ergo) if self.window.begun() => self.window.add(started_us, &ergo),
                    Some(_) => {}
                    None => self.counts.no_body = self.counts.no_body.saturating_add(1),
                }
            }
            Outcome::Busy => {
                self.counts.busy = self.counts.busy.saturating_add(1);
                self.retreat.take(Ended::Busy, now_us, jitter);
            }
            Outcome::Failed(failure) => {
                self.counts.failed = self.counts.failed.saturating_add(1);
                self.last_failure = Some(failure);
                self.retreat.take(Ended::Failed, now_us, jitter);
            }
            Outcome::Cancelled { .. } => {}
        }
    }

    /// One step at `now_us` under posture's gate ([`gate`]), with whether
    /// `room/stood_up` came: the request's end taken, the window reset or
    /// begun, and the posture to say now, if it is due.
    pub fn step(
        &mut self,
        now_us: u64,
        gate: Result<(), Closed>,
        stood_up: bool,
    ) -> Option<HumanPosture> {
        self.collect(now_us);
        self.open = gate.is_ok();
        // Any closing forgets the window: nothing old is said under the
        // privacy or while asleep.
        if stood_up || gate.is_err() {
            self.cancel();
            if self.window.begun() {
                self.counts.resets = self.counts.resets.saturating_add(1);
            }
            self.window.reset();
            self.open_at_us = None;
        }
        if gate.is_ok() {
            if self.open_at_us.is_none() {
                // A new sitting: due at once, said with its third sample.
                self.next_said_us = now_us;
            }
            self.window.begin(now_us);
            self.open_at_us = Some(now_us);
        }
        let sitting = self
            .open_at_us
            .is_some_and(|at| now_us.saturating_sub(at) <= us(HumanPosture::LASTS));
        if !sitting || now_us < self.next_said_us {
            return None;
        }
        // Not said while the window holds fewer than three samples (frames
        // not going out, the server busy or failing, no body found, just
        // wiped or just begun): the topic runs out and a skill holding on it
        // runs as it did, as doing.rs's "a `bad_share` of 0 is said only
        // over samples" reads. Still due, so the third sample is said as
        // soon as it lands.
        let posture = self.window.summary(now_us)?;
        self.next_said_us = now_us.saturating_add(us(HumanPosture::EVERY));
        Some(posture).filter(|posture| posture.check().is_ok())
    }

    /// What `diagnostics` says of posture.
    pub fn state(&self, now_us: u64) -> &'static str {
        if self.in_flight() {
            "sending"
        } else if self.paused_until(now_us).is_some() {
            "backing_off"
        } else {
            "idle"
        }
    }
}

impl Sink for Pose {
    fn ready(&mut self, now_us: u64) -> bool {
        self.collect(now_us);
        self.open
            && self.job.is_none()
            && self.retreat.may_start(now_us)
            && now_us >= self.next_due_us
    }

    fn frame(&mut self, now_us: u64, meta: &FrameMeta, jpeg: &[u8]) {
        // Not the camera's own JPEG: skipped, never sent, so it backs
        // nothing off. The next frame may go.
        if !crate::activity::sendable(meta) {
            self.counts.wrong_frame = self.counts.wrong_frame.saturating_add(1);
            return;
        }
        if !self.ready(now_us) {
            self.counts.dropped = self.counts.dropped.saturating_add(1);
            return;
        }
        let Ok(sent) = crate::scale::for_sending(jpeg, meta.width, meta.height) else {
            self.counts.wrong_frame = self.counts.wrong_frame.saturating_add(1);
            return;
        };
        let frame_b64 = Zeroizing::new(STANDARD.encode(&*sent));
        drop(sent);
        let cancel = Arc::new(AtomicBool::new(false));
        let (tell, done) = mpsc::channel();
        let client = Arc::clone(&self.client);
        let stop = Arc::clone(&cancel);
        let spawned = std::thread::Builder::new()
            .name("seen-pose".into())
            .spawn(move || {
                let outcome = client.request(&frame_b64, &stop);
                drop(frame_b64);
                let _ = tell.send(outcome);
            });
        self.next_due_us = now_us.saturating_add(EVERY_US);
        match spawned {
            Ok(_) => {
                self.counts.sent = self.counts.sent.saturating_add(1);
                self.job = Some(Job {
                    cancel,
                    done,
                    started_us: now_us,
                });
            }
            Err(_) => self.counts.dropped = self.counts.dropped.saturating_add(1),
        }
    }

    fn stopped(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
pub(crate) mod mock;
#[cfg(test)]
mod tests;
