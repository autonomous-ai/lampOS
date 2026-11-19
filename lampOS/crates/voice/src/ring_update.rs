//! The ring during an update (#7516): while lamp-update's round brings a
//! newer build the resting face breathes slower and deeper, and once the
//! round ends the ring shows one short look, a swell when the lamp kept the
//! new build and a dip when it did not.
//!
//! Only with `LAMP_RING_UPDATE=on`, handed to the ring worker alone. As the
//! night's asleep face ([`crate::ring_night`]), it shapes the resting frame
//! only, so whatever already decides the ring decides it too: a turn's
//! faces paint over it, the privacy switch and a mute blank it, and the
//! ring is never drawn by anything else. It draws in the person's own
//! colour and never brighter than their level, except the swell, which
//! rises one level and comes back; it never blinks and has no red.
//!
//! Nothing is drawn while Expression says the lamp is asleep, over the
//! person's light off (the ember), or while a light someone asked for holds
//! the ring (a skill's, a scene's, a paired computer's). A look that comes
//! due while the lamp is asleep, muted or dark waits and is drawn once it
//! may be, unless 12 hours have passed since the round ended; one that comes
//! due while a light holds the ring is dropped.
//!
//! Each round's look is drawn once: the round's end is kept in
//! [`KEEP_PATH`] before it is drawn, so a voice restarted after it (the
//! update's own restart, a crash) reads it and draws nothing. The bus and
//! the file are read and written by a thread of its own, never by the
//! ring's loop.
use crate::ring_face::Lit;
use lamp_bus::Bus;
use lamp_msgs::{ExpressionState, UpdateStatus};
use lamp_ring::{PIXEL_COUNT, Rgb};
use std::{
    f32::consts::TAU,
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// The switch: the ring shows an update only with `on`.
pub const ENVIRONMENT: &str = "LAMP_RING_UPDATE";
/// Where the last round's end whose look was drawn (or dropped) is kept.
pub const KEEP_PATH: &str = "/var/lib/lampOS/voice/update-cue";

/// Whether the switch is on in this process's environment.
pub fn enabled() -> bool {
    std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// lamp-update says its status at each phase and every minute; four looks
/// a second are enough for a breath that takes ten.
const LOOK_EVERY: Duration = Duration::from_millis(250);
/// One breath while updating: about twice the plain 5.2 s.
pub const BREATH_US: u64 = 10_000_000;
/// The breath's pace for the eye's blinks, as Mood says it: 5.2 s over 10.
const BREATH_PACE: f32 = 0.52;
/// How low the updating breath goes, as a share of the person's light.
pub const BREATH_LOW: f32 = 0.5;
/// The swell: up one level over this, held, and back.
const SWELL_UP_US: u64 = 1_000_000;
const SWELL_HOLD_US: u64 = 500_000;
/// The dip: down over this and back over as long.
const DIP_DOWN_US: u64 = 1_500_000;
/// How low the dip goes, as a share of the person's light.
pub const DIP_LOW: f32 = 1.0 / 3.0;
/// Each look lasts this long in all.
pub const CUE_US: u64 = 3_000_000;
/// A look not drawn within this of its round's end is dropped.
pub const DUE_FOR_MS: u64 = 12 * 60 * 60 * 1_000;

/// How a round ended, for the ring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ended {
    /// The lamp runs the new build (`switched`, `held`, `self-switched`).
    Done,
    /// It does not (`rolled-back`; `refused` or `failed` for a round seen
    /// bringing a build).
    Failed,
}

/// Whether the ring may draw a look now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Can {
    /// At rest on the person's light, painted the last time.
    Draw,
    /// Asleep, muted, the privacy switch, a turn, or the light off: a look
    /// waits.
    Wait,
    /// A light someone asked for holds the ring: a look is dropped.
    Held,
}

/// What the resting frame shows now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Look {
    Plain,
    /// The slower, deeper breath, at `phase` (0 out, 0.5 in).
    Updating {
        phase: f64,
    },
    /// The swell, `since_us` into it.
    Done {
        since_us: u64,
    },
    /// The dip, `since_us` into it.
    Failed {
        since_us: u64,
    },
}

/// A look owed for one round's end, by that end's time, which every
/// saying of the end carries, read back after a restart too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Owed {
    ended: Ended,
    key: u64,
}

/// What the ring knows of updates, with no bus, file or clock of its own.
#[derive(Debug, Default)]
pub struct Cues {
    /// The last end whose look was drawn or dropped, from the file at start.
    done_with: Option<u64>,
    /// The start of the last round seen bringing a build.
    seen: Option<u64>,
    owed: Option<Owed>,
    playing: Option<(Owed, u64)>,
    /// The round whose breath the journal was told of.
    said_breath: Option<u64>,
}

/// One step's result: the look, a journal line, and an end to keep.
#[derive(Debug, PartialEq)]
pub struct Step {
    pub look: Look,
    pub line: Option<String>,
    pub keep: Option<u64>,
}

/// How a round's end reads for the ring, or `None` for no look:
/// `seen` is the start of the last round seen bringing a build.
fn ended(status: &UpdateStatus, seen: Option<u64>) -> Option<Owed> {
    if status.now != "idle" {
        return None;
    }
    let last = status.last.as_ref()?;
    let was_seen = last.started_at_unix_ms.is_some() && last.started_at_unix_ms == seen;
    let ended = match last.outcome.as_str() {
        "switched" | "held" | "self-switched" => Ended::Done,
        "rolled-back" => Ended::Failed,
        "refused" | "failed" if was_seen => Ended::Failed,
        _ => return None,
    };
    Some(Owed {
        ended,
        key: last.at_unix_ms,
    })
}

impl Cues {
    /// With the end last kept (`done_with`), as read at start.
    pub fn kept(done_with: Option<u64>) -> Self {
        Self {
            done_with,
            ..Self::default()
        }
    }

    /// One step: `status` lamp-update's fresh status, if any; `can` whether
    /// a look may be drawn; `kept` the end the file now holds (once asked
    /// to keep one, the look waits for it, so it is kept before drawn).
    pub fn step(
        &mut self,
        status: Option<&UpdateStatus>,
        can: Can,
        kept: Option<u64>,
        now_us: u64,
        now_unix_ms: u64,
    ) -> Step {
        let mut line = None;
        let mut keep = None;
        let mut updating = None;
        if let Some(status) = status {
            if status.brings_a_build() {
                let round = status.round.as_ref().map(|round| round.started_at_unix_ms);
                if round.is_some() {
                    self.seen = round;
                }
                updating = Some(round.unwrap_or(0));
            } else if let Some(owed) = ended(status, self.seen)
                && Some(owed.key) != self.done_with
                && self.owed.map(|owed| owed.key) != Some(owed.key)
                && self.playing.map(|(owed, _)| owed.key) != Some(owed.key)
            {
                self.owed = Some(owed);
            }
        }
        if let Some(owed) = self.owed
            && self.playing.is_none()
        {
            let name = name(owed.ended);
            if now_unix_ms.saturating_sub(owed.key) >= DUE_FOR_MS {
                self.drop_owed(owed, &mut keep);
                line = Some(format!("update look: {name} dropped, 12 h after the round"));
            } else if can == Can::Held {
                self.drop_owed(owed, &mut keep);
                line = Some(format!(
                    "update look: {name} dropped, a light holds the ring"
                ));
            } else if can == Can::Draw {
                if kept == Some(owed.key) {
                    self.owed = None;
                    self.done_with = Some(owed.key);
                    self.playing = Some((owed, now_us));
                    line = Some(format!("update look: {name} drawn"));
                } else {
                    keep = Some(owed.key);
                }
            }
        }
        if let Some((owed, since)) = self.playing {
            let since_us = now_us.saturating_sub(since);
            if since_us < CUE_US {
                let look = match owed.ended {
                    Ended::Done => Look::Done { since_us },
                    Ended::Failed => Look::Failed { since_us },
                };
                return Step { look, line, keep };
            }
            self.playing = None;
        }
        let look = match updating {
            Some(round) if can == Can::Draw => {
                if self.said_breath != Some(round) {
                    self.said_breath = Some(round);
                    line = line.or_else(|| Some("update look: updating breath shown".to_owned()));
                }
                Look::Updating {
                    phase: (now_us % BREATH_US) as f64 / BREATH_US as f64,
                }
            }
            _ => Look::Plain,
        };
        Step { look, line, keep }
    }

    fn drop_owed(&mut self, owed: Owed, keep: &mut Option<u64>) {
        self.owed = None;
        self.done_with = Some(owed.key);
        *keep = Some(owed.key);
    }
}

fn name(ended: Ended) -> &'static str {
    match ended {
        Ended::Done => "done",
        Ended::Failed => "failed",
    }
}

/// Eases from 0 to 1 and never past.
fn ease(share: f32) -> f32 {
    let share = share.clamp(0.0, 1.0);
    share * share * (3.0 - 2.0 * share)
}

/// What the face takes for `look` over the person's light `lit`: the breath
/// it breathes at rest, if not its own, and how much of the frame drawn is
/// shown, 1 being as drawn. The lowest level is already only a step or two
/// of each channel, so there nothing is dimmed (it would break into the
/// ember's patchwork): its breath slows, and the dip holds it out.
pub fn shaped(look: Look, lit: Lit) -> (Option<(f64, f32)>, f32) {
    let dims = lit.level() > 1;
    match look {
        Look::Plain => (None, 1.0),
        Look::Updating { phase } => {
            let breath = 0.5 - 0.5 * (TAU * phase as f32).cos();
            let share = if dims {
                BREATH_LOW + (1.0 - BREATH_LOW) * breath
            } else {
                1.0
            };
            (Some((phase, BREATH_PACE)), share)
        }
        Look::Done { since_us } => {
            let up = lit.step_up_share() - 1.0;
            let rise = if since_us < SWELL_UP_US {
                ease(since_us as f32 / SWELL_UP_US as f32)
            } else if since_us < SWELL_UP_US + SWELL_HOLD_US {
                1.0
            } else {
                let back = CUE_US - SWELL_UP_US - SWELL_HOLD_US;
                1.0 - ease((since_us - SWELL_UP_US - SWELL_HOLD_US) as f32 / back as f32)
            };
            (None, 1.0 + up * rise)
        }
        Look::Failed { since_us } => {
            if !dims {
                return (Some((0.0, BREATH_PACE)), 1.0);
            }
            let down = if since_us < DIP_DOWN_US {
                ease(since_us as f32 / DIP_DOWN_US as f32)
            } else {
                1.0 - ease((since_us - DIP_DOWN_US) as f32 / (CUE_US - DIP_DOWN_US) as f32)
            };
            (None, 1.0 - (1.0 - DIP_LOW) * down)
        }
    }
}

/// The resting `pixels` with `share` of their light, no channel over
/// `ceiling`.
pub fn scaled(pixels: &[Rgb; PIXEL_COUNT], share: f32, ceiling: u8) -> [Rgb; PIXEL_COUNT] {
    if share == 1.0 {
        return *pixels;
    }
    let channel = |value: u8| {
        (f32::from(value) * share)
            .round()
            .clamp(0.0, f32::from(ceiling)) as u8
    };
    pixels.map(|pixel| {
        Rgb::new(
            channel(pixel.red),
            channel(pixel.green),
            channel(pixel.blue),
        )
    })
}

/// The end kept in `path`, if one can be read.
fn read_kept(path: &Path) -> Option<u64> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get("ended_at_unix_ms")?.as_u64()
}

/// Keeps `key` in `path`, whole or not at all.
fn write_kept(path: &Path, key: u64) -> io::Result<()> {
    let beside = path.with_extension("new");
    std::fs::write(&beside, format!("{{\"ended_at_unix_ms\":{key}}}\n"))?;
    std::fs::rename(&beside, path)
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

#[derive(Default)]
struct Found {
    status: Mutex<Option<UpdateStatus>>,
    asleep: AtomicBool,
    /// The end asked to be kept, and the end the file holds (or that could
    /// not be written, which is said and drawn all the same).
    keep: Mutex<(Option<u64>, Option<u64>)>,
    stop: AtomicBool,
}

/// The ring's side: the reading thread's findings and the cues.
pub struct RingUpdate {
    found: Arc<Found>,
    cues: Cues,
}

impl RingUpdate {
    /// lamp-update's status and Expression's sleep on the bus under `root`,
    /// read by a thread of its own from now on; the kept end at [`KEEP_PATH`].
    pub fn on(root: &Path) -> io::Result<Self> {
        Self::keeping(root, PathBuf::from(KEEP_PATH))
    }

    /// As [`Self::on`], the end kept at `path`.
    pub fn keeping(root: &Path, path: PathBuf) -> io::Result<Self> {
        let bus = Bus::new(root)?;
        let kept = read_kept(&path);
        let found = Arc::new(Found {
            keep: Mutex::new((None, kept)),
            ..Found::default()
        });
        let reading = Arc::clone(&found);
        thread::Builder::new()
            .name("voice-ring-update".into())
            .spawn(move || {
                while !reading.stop.load(Ordering::Acquire) {
                    // A bus that cannot be read is no update and no sleep.
                    let status = bus
                        .latest(UpdateStatus::TOPIC)
                        .ok()
                        .flatten()
                        .filter(|message| !message.expired)
                        .and_then(|message| message.parse::<UpdateStatus>().ok());
                    if let Ok(mut slot) = reading.status.lock() {
                        *slot = status;
                    }
                    let asleep = bus
                        .latest(ExpressionState::TOPIC)
                        .ok()
                        .flatten()
                        .is_some_and(|message| {
                            crate::ring_night::asleep_in(&message, lamp_ipc::monotonic_us())
                        });
                    reading.asleep.store(asleep, Ordering::Release);
                    let asked = reading.keep.lock().ok().and_then(|keep| {
                        let (asked, kept) = *keep;
                        asked.filter(|asked| Some(*asked) != kept)
                    });
                    if let Some(asked) = asked {
                        if let Err(error) = write_kept(&path, asked) {
                            eprintln!(
                                "lamp-voice ring: update look: the round's end was not kept, drawn all the same: {error}"
                            );
                        }
                        if let Ok(mut keep) = reading.keep.lock() {
                            keep.1 = Some(asked);
                        }
                    }
                    thread::sleep(LOOK_EVERY);
                }
            })?;
        Ok(Self {
            found,
            cues: Cues::kept(kept),
        })
    }

    /// Whether Expression says the lamp is asleep, as last read.
    pub fn asleep(&self) -> bool {
        self.found.asleep.load(Ordering::Acquire)
    }

    /// Call from the ring's loop with its clock: what the resting frame
    /// shows now. It reads no bus or file and never waits.
    pub fn tick(&mut self, now_us: u64, can: Can) -> Look {
        let status = self
            .found
            .status
            .lock()
            .ok()
            .and_then(|found| found.clone());
        let kept = self.found.keep.lock().ok().and_then(|keep| keep.1);
        let step = self
            .cues
            .step(status.as_ref(), can, kept, now_us, unix_ms());
        if let Some(asked) = step.keep
            && let Ok(mut keep) = self.found.keep.lock()
        {
            keep.0 = Some(asked);
        }
        if let Some(line) = step.line {
            eprintln!("lamp-voice ring: {line}");
        }
        step.look
    }
}

impl Drop for RingUpdate {
    /// The thread ends at its next look; the ring's end never waits on it.
    fn drop(&mut self) {
        self.found.stop.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::{UpdateBuild, UpdateResult, UpdateRound};

    const HOUR_MS: u64 = 60 * 60 * 1_000;
    const ROUND: u64 = 1_800_000_000_000;
    const END: u64 = ROUND + 120_000;

    fn build(sequence: u64) -> UpdateBuild {
        UpdateBuild {
            sequence,
            commit: "4a39f04".into(),
        }
    }

    fn status(now: &str, last: Option<(&str, Option<u64>)>) -> UpdateStatus {
        UpdateStatus {
            running: Some(build(812)),
            now: now.into(),
            last: last.map(|(outcome, started)| UpdateResult {
                outcome: outcome.into(),
                sequence: Some(813),
                at_unix_ms: END,
                reason: None,
                started_at_unix_ms: started,
            }),
            harness: None,
            round: (now != "idle").then(|| UpdateRound {
                started_at_unix_ms: ROUND,
                step_started_at_unix_ms: ROUND,
                target: Some(build(813)),
                ..UpdateRound::default()
            }),
            engines: None,
            refused: None,
        }
    }

    /// The looks a run of steps draws, one step every 25 ms for `for_us`,
    /// the file keeping each end asked at once.
    struct Run {
        cues: Cues,
        kept: Option<u64>,
        now_us: u64,
        lines: Vec<String>,
    }

    impl Run {
        fn new(kept: Option<u64>) -> Self {
            Self {
                cues: Cues::kept(kept),
                kept,
                now_us: 0,
                lines: Vec::new(),
            }
        }

        fn steps(
            &mut self,
            status: &UpdateStatus,
            can: Can,
            unix_ms: u64,
            for_us: u64,
        ) -> Vec<Look> {
            let end = self.now_us + for_us;
            let mut looks = Vec::new();
            while self.now_us < end {
                self.now_us += 25_000;
                let step = self
                    .cues
                    .step(Some(status), can, self.kept, self.now_us, unix_ms);
                if let Some(keep) = step.keep {
                    self.kept = Some(keep);
                }
                self.lines.extend(step.line);
                looks.push(step.look);
            }
            looks
        }
    }

    fn swells(looks: &[Look]) -> usize {
        looks
            .iter()
            .filter(|look| matches!(look, Look::Done { since_us: 0 }))
            .count()
    }

    fn dips(looks: &[Look]) -> usize {
        looks
            .iter()
            .filter(|look| matches!(look, Look::Failed { since_us: 0 }))
            .count()
    }

    /// The first builder's ask on #7516: a look due while the lamp is asleep
    /// is drawn once on waking, and never once 12 h have passed.
    #[test]
    fn a_cue_due_asleep_is_drawn_once_on_waking_and_never_after_12_h() {
        let switched = status("idle", Some(("switched", Some(ROUND))));
        let mut run = Run::new(None);
        let asleep = run.steps(&switched, Can::Wait, END + HOUR_MS, 5_000_000);
        assert!(asleep.iter().all(|look| *look == Look::Plain));
        let woke = run.steps(&switched, Can::Draw, END + 8 * HOUR_MS, 10_000_000);
        assert_eq!(swells(&woke), 1, "{woke:?}");
        assert_eq!(run.kept, Some(END));
        // Asleep and awake again, the same end draws nothing more.
        run.steps(&switched, Can::Wait, END + 9 * HOUR_MS, 1_000_000);
        let again = run.steps(&switched, Can::Draw, END + 9 * HOUR_MS, 10_000_000);
        assert_eq!(swells(&again), 0);

        let mut late = Run::new(None);
        late.steps(&switched, Can::Wait, END + HOUR_MS, 1_000_000);
        let woke = late.steps(&switched, Can::Draw, END + DUE_FOR_MS, 10_000_000);
        assert!(woke.iter().all(|look| *look == Look::Plain), "{woke:?}");
        assert_eq!(
            late.lines,
            ["update look: done dropped, 12 h after the round"]
        );
    }

    /// The end is kept before the look is drawn, and a voice started after
    /// it (the update's own restart) reads it and draws nothing.
    #[test]
    fn a_look_is_kept_before_it_is_drawn_and_never_drawn_again_after_a_restart() {
        let switched = status("idle", Some(("switched", Some(ROUND))));
        let mut cues = Cues::kept(None);
        let first = cues.step(Some(&switched), Can::Draw, None, 1, END);
        assert_eq!((first.look, first.keep), (Look::Plain, Some(END)));
        let drawn = cues.step(Some(&switched), Can::Draw, Some(END), 2, END);
        assert_eq!(drawn.look, Look::Done { since_us: 0 });
        let mut restarted = Run::new(Some(END));
        let looks = restarted.steps(&switched, Can::Draw, END, 5_000_000);
        assert!(looks.iter().all(|look| *look == Look::Plain));
    }

    /// Updating, the slow breath while it may be drawn; then the look for
    /// how the round ended, 3 s long; a round that only checks shows none.
    #[test]
    fn the_breath_shows_while_a_build_comes_and_one_look_after() {
        let mut run = Run::new(None);
        let fetching = run.steps(&status("fetching", None), Can::Draw, ROUND, 1_000_000);
        assert!(
            fetching
                .iter()
                .all(|look| matches!(look, Look::Updating { .. }))
        );
        let talking = run.steps(&status("switching", None), Can::Wait, ROUND, 100_000);
        assert!(talking.iter().all(|look| *look == Look::Plain));
        let ended = run.steps(
            &status("idle", Some(("switched", Some(ROUND)))),
            Can::Draw,
            END,
            5_000_000,
        );
        let drawn = ended
            .iter()
            .filter(|look| matches!(look, Look::Done { .. }))
            .count();
        assert_eq!(swells(&ended), 1);
        assert_eq!(drawn as u64, CUE_US / 25_000);
        assert_eq!(
            run.lines,
            [
                "update look: updating breath shown",
                "update look: done drawn"
            ]
        );
        let mut checking = Run::new(None);
        let mut current = status("checking", None);
        current.round.as_mut().unwrap().target = None;
        let looks = checking.steps(&current, Can::Draw, ROUND, 1_000_000);
        let up = checking.steps(
            &status("idle", Some(("up-to-date", Some(ROUND)))),
            Can::Draw,
            END,
            1_000_000,
        );
        assert!(looks.iter().chain(&up).all(|look| *look == Look::Plain));
    }

    /// A put-back always dips; a refusal or a failure only for a round seen
    /// bringing a build, so a build refused before anything is fetched
    /// shows nothing every 15 minutes.
    #[test]
    fn a_failure_dips_only_for_a_round_that_brought_a_build() {
        let mut back = Run::new(None);
        let looks = back.steps(
            &status("idle", Some(("rolled-back", None))),
            Can::Draw,
            END,
            4_000_000,
        );
        assert_eq!(dips(&looks), 1);
        let mut refused = Run::new(None);
        let looks = refused.steps(
            &status("idle", Some(("refused", Some(ROUND)))),
            Can::Draw,
            END,
            4_000_000,
        );
        assert_eq!(dips(&looks), 0);
        let mut seen = Run::new(None);
        seen.steps(&status("fetching", None), Can::Draw, ROUND, 100_000);
        let looks = seen.steps(
            &status("idle", Some(("failed", Some(ROUND)))),
            Can::Draw,
            END,
            4_000_000,
        );
        assert_eq!(dips(&looks), 1);
    }

    /// A light someone asked for keeps the ring: the look is dropped, kept
    /// as drawn, and not drawn once the light is given back.
    #[test]
    fn a_look_due_while_a_light_holds_the_ring_is_dropped() {
        let switched = status("idle", Some(("switched", Some(ROUND))));
        let mut run = Run::new(None);
        run.steps(&switched, Can::Held, END, 100_000);
        let after = run.steps(&switched, Can::Draw, END, 5_000_000);
        assert!(after.iter().all(|look| *look == Look::Plain));
        assert_eq!(run.kept, Some(END));
        assert_eq!(
            run.lines,
            ["update look: done dropped, a light holds the ring"]
        );
    }

    /// The breath and the dip never go over the person's light, the swell
    /// goes up exactly one level and back, and the lowest level is never
    /// dimmed.
    #[test]
    fn its_looks_stay_under_the_light_but_the_one_level_swell() {
        let two = Lit::of(true, "blue", 2).unwrap();
        let one = Lit::of(true, "blue", 1).unwrap();
        for step in 0..=400u64 {
            let phase = step as f64 / 400.0;
            let (breath, share) = shaped(Look::Updating { phase }, two);
            assert_eq!(breath, Some((phase, BREATH_PACE)));
            assert!((BREATH_LOW - 1e-6..=1.0).contains(&share), "{share}");
            assert_eq!(shaped(Look::Updating { phase }, one).1, 1.0);
            let since_us = step * CUE_US / 400;
            let dip = shaped(Look::Failed { since_us }, two).1;
            assert!((DIP_LOW - 1e-6..=1.0).contains(&dip), "{dip}");
            assert_eq!(
                shaped(Look::Failed { since_us }, one),
                (Some((0.0, BREATH_PACE)), 1.0)
            );
            let swell = shaped(Look::Done { since_us }, two).1;
            assert!(
                (1.0..=two.step_up_share() + 1e-6).contains(&swell),
                "{swell}"
            );
        }
        let top = shaped(
            Look::Done {
                since_us: 1_200_000,
            },
            two,
        )
        .1;
        assert!((top - two.step_up_share()).abs() < 1e-6);
        assert_eq!(shaped(Look::Done { since_us: CUE_US }, two).1, 1.0);
        let five = Lit::of(true, "white", 5).unwrap();
        assert!(five.step_up_share() < 1.12);
        let pixels = [Rgb::new(20, 10, 0); PIXEL_COUNT];
        assert_eq!(scaled(&pixels, 0.5, 24)[0], Rgb::new(10, 5, 0));
        assert_eq!(scaled(&pixels, 3.0, 24)[0], Rgb::new(24, 24, 0));
        assert_eq!(scaled(&pixels, 1.0, 24), pixels);
    }

    /// The kept end is read back as written, and a file that isn't one is
    /// none.
    #[test]
    fn the_kept_end_reads_back() {
        let dir = std::env::temp_dir().join(format!("ring-update-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("update-cue");
        assert_eq!(read_kept(&path), None);
        write_kept(&path, END).unwrap();
        assert_eq!(read_kept(&path), Some(END));
        std::fs::write(&path, "nonsense").unwrap();
        assert_eq!(read_kept(&path), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
