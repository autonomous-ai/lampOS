//! Activity's sink (#6543 phase 2): the frames' [`Sink`] that sends them to
//! the perception service in bursts, and reads back what was seen.
//!
//! It sits inside the ring's mark ([`crate::mark::Marked`]) and behind the
//! gate ([`crate::frames::Taker`]), so a frame reaches it only while the
//! gate is clear and the ring said it drew the mark for that frame.
//!
//! - **One burst of [`FRAMES`] frames every [`EVERY_US`]**, while it may
//!   (the retreat below): it wants a frame ([`Sink::ready`]) only while a
//!   burst is due or under way with frames to take. The taker spaces the
//!   frames [`SPACING_US`] apart, and the burst sends them at least a
//!   second apart, so the server buffers each into its clip.
//! - **Each burst is its own socket** ([`crate::burst`]): opened at the
//!   burst's first frame, the key exchange and V1's config, the frames and
//!   their answers, then closed, so the server keeps nothing between bursts.
//! - **Only the camera's own JPEGs, at least 320×240, sent at 320×240**
//!   ([`crate::scale`]): a larger frame (the lamp's camera serves 1280×960
//!   with `--full-look`) is decoded, scaled down and encoded again inside
//!   its in-flight copy; 320×240 goes as it came. Anything smaller, not
//!   MJPG, a derived copy, or a JPEG that does not decode at its size is
//!   dropped (`wrong_frame`). The server takes any size: its person
//!   detector letterboxes the frame to its own input, and the action model
//!   scales the person's crop to 224×224 (`preprocess_single_frame`).
//! - **Each copy is wiped** once its message is made, or with the burst.
//! - **Busy**: the burst is dropped, with no retry inside it. After
//!   [`crate::backoff::BUSY_RUN`] busy bursts in a row, or any failure, the
//!   bursts retreat ([`crate::backoff::Retreat`]) and `warn` is said.
//! - **The gate closes mid-burst** (or the link is lost, the grant changes,
//!   the process stops: [`Sink::stopped`]): the burst is aborted at once,
//!   its socket closed, its frames wiped and its answers dropped.
//! - **The journal** ([`Note`], drained by `serve` with [`Activity::notes`]):
//!   each burst's start, its frames handed (count and bytes), and its end:
//!   answered (how many kinds were counted and the server's time per
//!   frame), busy or failed (and how long the bursts are held back), or
//!   aborted. At most one burst a minute (every 15 s at fastest, with
//!   [`FAST_ENVIRONMENT`] on and the cloud's `seen_clip`), so at most four lines a minute (twelve).
//!   No picture, no id, no name, and no label: the surest label and its
//!   confidence and each kind counted by name only with the diagnostics
//!   switch [`crate::JOURNAL_LABELS`] `on`, since a forget or the opt-in
//!   off does not clear the journal.
use crate::{
    backoff::{Ended, Retreat},
    burst::{Burst, Copied, Event, Failure, Outcome, Pace},
    endpoint::Endpoint,
    frames::Sink,
    sitting,
    wire::ActionAnswer,
};
use lamp_camera::frames::FrameMeta;
use lamp_msgs::DoingSeen;
use rand_core::RngCore;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

/// Frames in one burst: the server's clip.
pub const FRAMES: u32 = 8;
/// Between two frames asked of the camera in a burst.
pub const SPACING_US: u64 = 1_200_000;
/// From one burst's start to the next's, by default. A burst under way
/// holds the next back until it ends, so two never overlap and none is
/// queued.
pub const EVERY_US: u64 = 60_000_000;
/// The fastest a burst may go, and only with [`FAST_ENVIRONMENT`] on (the
/// owner's call of 19 Nov on #7401, for the seen reactions): the cloud's
/// `seen_clip` picks the period within it and [`EVERY_US`] ([`crate::clip`]).
pub const FAST_EVERY_US: u64 = 15_000_000;
/// The switch that lets the cloud's `seen_clip` go faster than [`EVERY_US`],
/// down to [`FAST_EVERY_US`]: `on` exactly ([`crate::switched_on`]); dark,
/// no unit sets it. Anything else and it is every [`EVERY_US`].
pub const FAST_ENVIRONMENT: &str = "LAMP_SEEN_BURST_15S";

/// The fastest burst period the switch's value allows: [`FAST_EVERY_US`]
/// for `on` exactly, else [`EVERY_US`].
pub fn fastest_us(value: Option<&str>) -> u64 {
    if crate::switched_on(value) {
        FAST_EVERY_US
    } else {
        EVERY_US
    }
}
/// The smallest frame taken, and the size every frame is sent at.
pub const LEAST_SIZE: (u32, u32) = crate::scale::SENT;

/// Saturating counts, for `diagnostics`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub bursts: u64,
    pub answered: u64,
    pub busy: u64,
    pub failed: u64,
    pub aborted: u64,
    /// Frames handed to a burst.
    pub frames: u64,
    /// Frames not the camera's JPEG of at least 320×240, or not one that
    /// decodes at its size: dropped.
    pub wrong_frame: u64,
}

/// A burst that ended, as [`Activity::poll`] says it.
#[derive(Clone, Debug, PartialEq)]
pub struct Finished {
    pub outcome: Outcome,
    /// What it saw (empty unless answered).
    pub seen: Vec<DoingSeen>,
    /// Its tiredness, desk work and celebrating ([`sitting::read_more`]),
    /// kept only with `LAMP_SEEN_MORE_DOINGS=on`; nothing unless answered.
    pub more: sitting::More,
}

/// One line for the journal about a burst ([`Note::line`]).
#[derive(Clone, Debug, PartialEq)]
pub enum Note {
    /// A burst began: its socket opening for its first frame.
    Started,
    /// Every frame of the burst was handed to it.
    Sent { frames: u32, bytes: u64 },
    /// The burst was stopped before its end ([`Sink::stopped`]): the gate
    /// closed, the link was lost, the grant changed, or the process stops.
    Aborted { frames: u32, bytes: u64 },
    /// The burst ended.
    Ended(BurstEnd),
}

/// How a burst ended, for [`Note::Ended`].
#[derive(Clone, Debug, PartialEq)]
pub struct BurstEnd {
    pub outcome: Outcome,
    pub frames: u32,
    pub bytes: u64,
    /// The surest label any answer gave, and its confidence.
    pub top: Option<(String, f64)>,
    /// The kinds counted from it ([`sitting::read`]).
    pub seen: Vec<DoingSeen>,
    /// The server's time per frame, sent to answered: the mean and the most.
    pub server: Option<(Duration, Duration)>,
    /// From the burst's start to its end.
    pub took: Duration,
    /// How long the next burst is held back by the retreat, if it is.
    pub held_back_s: Option<u64>,
}

fn ms(duration: Duration) -> u128 {
    duration.as_millis()
}

fn kind(what: lamp_msgs::Doing) -> &'static str {
    match what {
        lamp_msgs::Doing::Drinking => "drinking",
        lamp_msgs::Doing::Eating => "eating",
        lamp_msgs::Doing::Break => "break",
        lamp_msgs::Doing::Nothing => "none",
    }
}

impl Note {
    /// The journal's line, without the `lamp-seen: ` prefix. `stopped` says
    /// why a burst was aborted (the gate's word, or the link); `labels`
    /// ([`crate::JOURNAL_LABELS`]) names what was seen, off counts only.
    pub fn line(&self, stopped: &str, labels: bool) -> String {
        match self {
            Note::Started => "burst started".to_owned(),
            Note::Sent { frames, bytes } => {
                format!("burst sent: {frames} frames, {bytes} bytes")
            }
            Note::Aborted { frames, bytes } => format!(
                "burst aborted ({stopped}) after {frames} frames, {bytes} bytes: its answers dropped"
            ),
            Note::Ended(ended) => ended.line(labels),
        }
    }
}

impl BurstEnd {
    fn line(&self, labels: bool) -> String {
        let held = self
            .held_back_s
            .map(|s| format!("; bursts held back {s} s"))
            .unwrap_or_default();
        let sent = format!(
            "{} of {FRAMES} frames, {} bytes, {} ms in all",
            self.frames,
            self.bytes,
            ms(self.took)
        );
        match self.outcome {
            Outcome::Answered => {
                let server = self.server.map_or_else(
                    || "no answer timed".to_owned(),
                    |(mean, most)| format!("server {} ms a frame, at most {}", ms(mean), ms(most)),
                );
                if !labels {
                    let kinds = self.seen.len();
                    let plural = if kinds == 1 { "kind" } else { "kinds" };
                    return format!(
                        "burst answered: {kinds} {plural} counted; {server}; {sent}{held}"
                    );
                }
                let top = self.top.as_ref().map_or_else(
                    || "no label".to_owned(),
                    |(label, conf)| format!("top {label} {conf:.2}"),
                );
                let seen = if self.seen.is_empty() {
                    "none".to_owned()
                } else {
                    self.seen
                        .iter()
                        .map(|s| format!("{} {:.2}", kind(s.what), s.confidence))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                format!("burst answered: {top}; counted {seen}; {server}; {sent}{held}")
            }
            Outcome::Busy => {
                format!("burst busy: the server is busy, the burst dropped; {sent}{held}")
            }
            Outcome::Failed(failure) => {
                let failure = match failure {
                    Failure::Refused(status) => format!("refused, HTTP {status}"),
                    other => other.as_str().to_owned(),
                };
                format!("burst failed ({failure}); {sent}{held}")
            }
        }
    }
}

struct Running {
    burst: Burst,
    taken: u32,
    bytes: u64,
    started: Instant,
    answers: Vec<ActionAnswer>,
    /// Each answer's time, from its frame sent to it read.
    took: Vec<Duration>,
}

/// The surest label in `answers`, and its confidence.
fn top(answers: &[ActionAnswer]) -> Option<(String, f64)> {
    answers
        .iter()
        .flat_map(|answer| answer.detected_classes.iter())
        .filter(|class| class.conf.is_finite())
        .max_by(|a, b| a.conf.total_cmp(&b.conf))
        .map(|class| (class.class_name.clone(), class.conf))
}

/// Activity's sink.
pub struct Activity {
    endpoint: Endpoint,
    /// Whether the public key was fetched (forgotten on a refused
    /// exchange) rather than given.
    fetched_key: bool,
    pace: Pace,
    running: Option<Running>,
    next_us: u64,
    /// From one burst's start to the next's: [`EVERY_US`] (or
    /// [`FAST_EVERY_US`] with its switch, [`every_us`]), or longer on a warm
    /// or hot board ([`crate::heat::Pace`]).
    every_us: u64,
    retreat: Retreat,
    last_failure: Option<Failure>,
    wiped: Arc<AtomicU64>,
    counts: Counts,
    notes: Vec<Note>,
}

fn jitter() -> f64 {
    f64::from(rand_core::OsRng.next_u32()) / f64::from(u32::MAX)
}

impl Activity {
    pub fn new(endpoint: Endpoint, pace: Pace) -> Self {
        Self {
            fetched_key: endpoint.public_key.is_none(),
            endpoint,
            pace,
            running: None,
            next_us: 0,
            every_us: EVERY_US,
            retreat: Retreat::default(),
            last_failure: None,
            wiped: Arc::new(AtomicU64::new(0)),
            counts: Counts::default(),
            notes: Vec::new(),
        }
    }

    /// Space the bursts `every_us` apart, start to start
    /// ([`crate::heat::Pace`]): the next one too, counted from the last
    /// one's start. A burst under way finishes as it is.
    pub fn set_every_us(&mut self, every_us: u64) {
        if self.next_us > 0 {
            self.next_us = self
                .next_us
                .saturating_sub(self.every_us)
                .saturating_add(every_us);
        }
        self.every_us = every_us;
    }

    /// From one burst's start to the next's, now.
    pub fn every_us(&self) -> u64 {
        self.every_us
    }

    /// The same, a burst every `every_us` ([`every_us`]).
    pub fn every(self, every_us: u64) -> Self {
        Self { every_us, ..self }
    }

    /// The journal's notes since the last call, oldest first.
    pub fn notes(&mut self) -> Vec<Note> {
        std::mem::take(&mut self.notes)
    }

    /// How long the retreat holds the next burst back from `now_us`, in
    /// whole seconds, if it does.
    fn held_back_s(&self, now_us: u64) -> Option<u64> {
        self.retreat
            .until_us()
            .filter(|until| *until > now_us)
            .map(|until| (until - now_us).div_ceil(1_000_000))
    }

    pub fn counts(&self) -> Counts {
        self.counts
    }

    /// The bursts' retreat: `warn` on `lamp/health` while it holds.
    pub fn retreat(&self) -> Retreat {
        self.retreat
    }

    pub fn last_failure(&self) -> Option<Failure> {
        self.last_failure
    }

    /// Frame copies wiped so far.
    pub fn wiped(&self) -> u64 {
        self.wiped.load(Ordering::Relaxed)
    }

    /// Whether a burst is under way.
    pub fn sending(&self) -> bool {
        self.running.is_some()
    }

    /// When the next burst may start.
    pub fn next_us(&self) -> u64 {
        self.next_us.max(self.retreat.until_us().unwrap_or(0))
    }

    /// Read what the burst under way said; a burst that ended is said
    /// once, at `now_us`.
    pub fn poll(&mut self, now_us: u64) -> Option<Finished> {
        let running = self.running.as_mut()?;
        let mut done = None;
        for event in running.burst.poll() {
            match event {
                Event::PublicKey(key) => {
                    if self.endpoint.public_key.is_none() {
                        self.endpoint.public_key = Some(*key);
                    }
                }
                Event::Answer(answer, took) => {
                    running.answers.push(answer);
                    running.took.push(took);
                }
                Event::Done(outcome) => done = Some(outcome),
            }
        }
        let outcome = done?;
        let running = self.running.take()?;
        let more = match outcome {
            Outcome::Answered => sitting::read_more(&running.answers),
            _ => sitting::More::default(),
        };
        let (ended, seen) = match outcome {
            Outcome::Answered => {
                self.counts.answered = self.counts.answered.saturating_add(1);
                (Ended::Answered, sitting::read(&running.answers))
            }
            Outcome::Busy => {
                self.counts.busy = self.counts.busy.saturating_add(1);
                (Ended::Busy, Vec::new())
            }
            Outcome::Failed(failure) => {
                self.counts.failed = self.counts.failed.saturating_add(1);
                self.last_failure = Some(failure);
                if self.fetched_key && matches!(failure, Failure::KeyExchange) {
                    // The server's key may have changed: fetched again.
                    self.endpoint.public_key = None;
                }
                (Ended::Failed, Vec::new())
            }
        };
        self.retreat.take(ended, now_us, jitter());
        let server = running.took.iter().max().map(|most| {
            let sum: Duration = running.took.iter().sum();
            (sum / running.took.len() as u32, *most)
        });
        self.notes.push(Note::Ended(BurstEnd {
            outcome,
            frames: running.taken,
            bytes: running.bytes,
            top: top(&running.answers),
            seen: seen.clone(),
            server,
            took: running.started.elapsed(),
            held_back_s: self.held_back_s(now_us),
        }));
        Some(Finished {
            outcome,
            seen,
            more,
        })
    }

    fn start(&mut self, now_us: u64) -> bool {
        self.next_us = now_us.saturating_add(self.every_us);
        self.counts.bursts = self.counts.bursts.saturating_add(1);
        match Burst::start(self.endpoint.clone(), self.pace) {
            Ok(burst) => {
                self.running = Some(Running {
                    burst,
                    taken: 0,
                    bytes: 0,
                    started: Instant::now(),
                    answers: Vec::new(),
                    took: Vec::new(),
                });
                self.notes.push(Note::Started);
                true
            }
            Err(_) => {
                self.counts.failed = self.counts.failed.saturating_add(1);
                self.last_failure = Some(Failure::Connect);
                self.retreat.take(Ended::Failed, now_us, jitter());
                self.notes.push(Note::Ended(BurstEnd {
                    outcome: Outcome::Failed(Failure::Connect),
                    frames: 0,
                    bytes: 0,
                    top: None,
                    seen: Vec::new(),
                    server: None,
                    took: Duration::ZERO,
                    held_back_s: self.held_back_s(now_us),
                }));
                false
            }
        }
    }
}

/// Whether `meta` is the camera's own JPEG, of at least 320×240 (a larger
/// one is scaled down to it before it is sent, [`crate::scale`]): posture
/// takes the same frames only ([`crate::pose`]).
pub(crate) fn sendable(meta: &FrameMeta) -> bool {
    meta.width >= LEAST_SIZE.0
        && meta.height >= LEAST_SIZE.1
        && meta.width <= crate::scale::MOST_SIDE
        && meta.height <= crate::scale::MOST_SIDE
        && meta.format == *b"MJPG"
        && meta.derived.is_none()
}

impl Sink for Activity {
    fn ready(&mut self, now_us: u64) -> bool {
        match &self.running {
            Some(running) => running.taken < FRAMES && running.burst.open(),
            None => now_us >= self.next_us && self.retreat.may_start(now_us),
        }
    }

    fn frame(&mut self, now_us: u64, meta: &FrameMeta, jpeg: &[u8]) {
        if !sendable(meta) {
            self.counts.wrong_frame = self.counts.wrong_frame.saturating_add(1);
            return;
        }
        if !self.ready(now_us) {
            return;
        }
        // Scaled before a burst opens for it, so a frame that does not
        // decode opens no socket.
        let Ok(copy) = Copied::scaled(jpeg, (meta.width, meta.height), self.wiped.clone()) else {
            self.counts.wrong_frame = self.counts.wrong_frame.saturating_add(1);
            return;
        };
        if self.running.is_none() && !self.start(now_us) {
            return;
        }
        let Some(running) = self.running.as_mut() else {
            return;
        };
        let size = copy.size() as u64;
        if running.burst.send(copy) {
            running.taken += 1;
            running.bytes = running.bytes.saturating_add(size);
            self.counts.frames = self.counts.frames.saturating_add(1);
        }
        if running.taken >= FRAMES {
            running.burst.end();
            self.notes.push(Note::Sent {
                frames: running.taken,
                bytes: running.bytes,
            });
        }
    }

    fn stopped(&mut self) {
        if let Some(mut running) = self.running.take() {
            running.burst.abort();
            self.counts.aborted = self.counts.aborted.saturating_add(1);
            self.notes.push(Note::Aborted {
                frames: running.taken,
                bytes: running.bytes,
            });
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
