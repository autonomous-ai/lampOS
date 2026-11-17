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
//!   forgotten at once on `room/stood_up` and when the gate closes for the
//!   privacy slide, sleep, the opt-in off or a forget, or on a stranger
//!   ([`crate::keep::Hard`]). While the gate is paused on a misread on the
//!   same tracked face ([`crate::pause`]) it is held, unsaid, for at most
//!   5 s; on any other closing (the face lost, a reading gone stale) it is
//!   held, unsaid, for at most [`crate::keep::KEEP_US`] (120 s), and goes on
//!   if the gate opens again within it ([`Pose::hold`]). No frame and no
//!   per-person history is kept.
//! - **The journal** ([`Note`], drained by `serve` with [`Pose::notes`]):
//!   each sample sent (its bytes) and its answer (scored or no body, the
//!   time it took), at most one each 30 s. The RULA score, its risk, bad or
//!   good and the region, and the said window's bad share and region, only
//!   with the diagnostics switch [`crate::JOURNAL_LABELS`] `on`: a forget or
//!   the opt-in off does not clear the journal ([`said_line`]).
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
use std::time::{Duration, Instant};
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
    started: Instant,
    /// The frame's width as sent, for the shoulders' share of it.
    width: u32,
}

/// The most pose answers' shoulder shares kept for [`Pose::answers`]
/// between two reads: the newest.
pub const ANSWERS_MOST: usize = 8;

/// One line for the journal about a posture sample ([`Note::line`]).
#[derive(Clone, Debug, PartialEq)]
pub enum Note {
    /// A frame handed to its socket, as sent (before Base64).
    Sent { bytes: usize },
    /// Answered with a score: the RULA score and risk level, whether the
    /// sample is bad and its first high region (V1's rules,
    /// [`window::judge`]), and whether it went into the window.
    Answered {
        score: i64,
        risk_level: i64,
        /// The server scored the upper body only (the hips hidden).
        partial: bool,
        bad: bool,
        region: &'static str,
        took: Duration,
        kept: bool,
    },
    /// Answered, but no body found: no sample.
    NoBody { took: Duration },
    Busy {
        took: Duration,
        held_back_s: Option<u64>,
    },
    Failed {
        failure: Failure,
        took: Duration,
        held_back_s: Option<u64>,
    },
    /// Stopped by the gate closing; its answer dropped.
    Cancelled,
}

/// The journal's line when `humans/posture` is said, without the
/// `lamp-seen: ` prefix: the window's length and its samples, and its bad
/// share and region only with `labels` ([`crate::JOURNAL_LABELS`]).
pub fn said_line(posture: &HumanPosture, samples: usize, labels: bool) -> String {
    let said = format!(
        "posture said: window {} s, {samples} samples",
        posture.window_s
    );
    if labels {
        format!(
            "{said}, bad_share {:.2}, region {}",
            posture.bad_share,
            window::region_word(posture.region)
        )
    } else {
        said
    }
}

impl Note {
    /// The journal's line, without the `lamp-seen: ` prefix; `labels`
    /// ([`crate::JOURNAL_LABELS`]) names the score, off says only scored.
    pub fn line(&self, labels: bool) -> String {
        let held = |held_back_s: &Option<u64>| {
            held_back_s
                .map(|s| format!("; posture held back {s} s"))
                .unwrap_or_default()
        };
        match self {
            Note::Sent { bytes } => format!("posture sample sent: {bytes} bytes"),
            Note::Answered { took, kept, .. } if !labels => format!(
                "posture sample answered: scored, {} ms{}",
                took.as_millis(),
                if *kept { "" } else { "; not kept (no sitting)" }
            ),
            Note::Answered {
                score,
                risk_level,
                partial,
                bad,
                region,
                took,
                kept,
            } => format!(
                "posture sample answered: score {score}, risk {risk_level}, {}{}, region {region}, {} ms{}",
                if *partial { "partial, " } else { "" },
                if *bad { "bad" } else { "good" },
                took.as_millis(),
                if *kept { "" } else { "; not kept (no sitting)" }
            ),
            Note::NoBody { took } => {
                format!(
                    "posture sample answered: no body found, {} ms",
                    took.as_millis()
                )
            }
            Note::Busy { took, held_back_s } => format!(
                "posture sample busy: the server is busy, the frame dropped, {} ms{}",
                took.as_millis(),
                held(held_back_s)
            ),
            Note::Failed {
                failure,
                took,
                held_back_s,
            } => format!(
                "posture sample failed ({}), {} ms{}",
                failure.as_str(),
                took.as_millis(),
                held(held_back_s)
            ),
            Note::Cancelled => {
                "posture sample cancelled: the gate closed, its answer dropped".to_owned()
            }
        }
    }
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
    notes: Vec<Note>,
    /// Each answer's shoulders as a share of the width
    /// ([`crate::hold::share`]), until [`Pose::answers`] takes them.
    answers: Vec<Option<f32>>,
    /// The newest answer's share and when its frame went, for
    /// [`Pose::recent_answer`].
    recent: Option<(u64, Option<f32>)>,
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
            notes: Vec::new(),
            answers: Vec::new(),
            recent: None,
        }
    }

    /// The journal's notes since the last call, oldest first.
    pub fn notes(&mut self) -> Vec<Note> {
        std::mem::take(&mut self.notes)
    }

    /// Each answer's shoulders as a share of the frame's width since the
    /// last call, oldest first, at most [`ANSWERS_MOST`]: none for an
    /// answer without both shoulders confident. Read by the held sitting
    /// ([`crate::hold`]); nothing else of the answer is kept.
    pub fn answers(&mut self) -> Vec<Option<f32>> {
        std::mem::take(&mut self.answers)
    }

    /// The newest answer's shoulders as a share of the width, when its
    /// frame went under [`EVERY_US`] ago: what a held sitting beginning
    /// now begins with ([`crate::hold`]), as the 30 s rate lets no new
    /// frame go yet. Forgotten with the window.
    pub fn recent_answer(&self, now_us: u64) -> Option<Option<f32>> {
        self.recent
            .filter(|(sent_us, _)| now_us.saturating_sub(*sent_us) < EVERY_US)
            .map(|(_, share)| share)
    }

    /// Whether a sitting's window is being watched (begun and not
    /// forgotten): something a closing gate may keep ([`crate::keep`]).
    pub fn watching(&self) -> bool {
        self.window.begun()
    }

    fn held_back_s(&self, now_us: u64) -> Option<u64> {
        self.paused_until(now_us)
            .map(|until| (until - now_us).div_ceil(1_000_000))
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
        let took = job.started.elapsed();
        let width = job.width;
        self.job = None;
        if cancelled || matches!(outcome, Outcome::Cancelled { .. }) {
            self.counts.cancelled = self.counts.cancelled.saturating_add(1);
            self.notes.push(Note::Cancelled);
            return;
        }
        let jitter = (self.jitter)();
        match outcome {
            Outcome::Answered { ergo, shoulders } => {
                self.counts.answered = self.counts.answered.saturating_add(1);
                if self.answers.len() >= ANSWERS_MOST {
                    self.answers.remove(0);
                }
                let share = crate::hold::share(shoulders.as_ref(), width);
                self.answers.push(share);
                self.recent = Some((started_us, share));
                self.retreat.take(Ended::Answered, now_us, jitter);
                self.last_failure = None;
                match ergo {
                    Some(ergo) => {
                        let (bad, high) = window::judge(&ergo);
                        let kept = self.window.begun();
                        if kept {
                            self.window.add(started_us, &ergo);
                        }
                        self.notes.push(Note::Answered {
                            score: ergo.score,
                            risk_level: ergo.risk_level,
                            partial: ergo.partial,
                            bad,
                            region: window::region_name(high),
                            took,
                            kept,
                        });
                    }
                    None => {
                        self.counts.no_body = self.counts.no_body.saturating_add(1);
                        self.notes.push(Note::NoBody { took });
                    }
                }
            }
            Outcome::Busy => {
                self.counts.busy = self.counts.busy.saturating_add(1);
                self.retreat.take(Ended::Busy, now_us, jitter);
                self.notes.push(Note::Busy {
                    took,
                    held_back_s: self.held_back_s(now_us),
                });
            }
            Outcome::Failed(failure) => {
                self.counts.failed = self.counts.failed.saturating_add(1);
                self.last_failure = Some(failure);
                self.retreat.take(Ended::Failed, now_us, jitter);
                self.notes.push(Note::Failed {
                    failure,
                    took,
                    held_back_s: self.held_back_s(now_us),
                });
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
        // Any closing that reaches here forgets the window: nothing old is
        // said under the privacy or while asleep. A pause on a misread, or
        // a short closing `serve` keeps ([`crate::keep`]), holds it instead
        // ([`Self::hold`]).
        if stood_up || gate.is_err() {
            self.forget();
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

    /// Forget the window and stop the running request: its answer is
    /// dropped.
    pub fn forget(&mut self) {
        self.cancel();
        if self.window.begun() {
            self.counts.resets = self.counts.resets.saturating_add(1);
        }
        self.window.reset();
        self.open_at_us = None;
        self.recent = None;
    }

    /// One step at `now_us` while the gate is paused on a misread
    /// ([`crate::pause`]) or closed for a while `serve` keeps
    /// ([`crate::keep`]): the window kept, in memory only (its samples
    /// still age out of its ten minutes when it is next read); the running
    /// request stopped and its answer dropped; no frame taken and nothing
    /// said.
    pub fn hold(&mut self, now_us: u64) {
        self.cancel();
        self.collect(now_us);
        self.open = false;
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
        let bytes = sent.len();
        let width = crate::scale::sent_size(meta.width, meta.height).0;
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
                self.notes.push(Note::Sent { bytes });
                self.job = Some(Job {
                    cancel,
                    done,
                    started_us: now_us,
                    started: Instant::now(),
                    width,
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
