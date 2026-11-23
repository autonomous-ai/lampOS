//! A lamp's day lasts its owner's day (#7664, parts 2 and 3): the voice
//! reads the relay's `X-Usage-Day-Used` (the share of the lamp's day
//! allowance used, 0 to 1) on its answers and keeps the day's latest. From
//! [`NEAR`] it sheds what nobody asked for (speaking up on its own, news,
//! room lines sent only to be judged), never the owner's own talk; at
//! [`SPENT`] a refusal is said as the usage limit at once (#7346's line).
//!
//! The provider worker reads it (`second_provider::net::post`, every HTTP
//! answer) and writes it to [`FILE`]; the coordinator, which begins the
//! turns nobody asked for, reads that file at most once a second. Off
//! unless `LAMP_VOICE_DAY_SHED=on`: then the header is not even read.
//!
//! The same rule as lamp-cloud's forwarder (`lamp_skills::relay_share`):
//! only a value from 0 to 1 is kept; a 2xx answer without the header says
//! the lamp has no day limit; a refusal without it changes nothing; the
//! reading is for one UTC day, the relay's.
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

/// `on` exactly; anything else, or unset: off.
pub const ENVIRONMENT: &str = "LAMP_VOICE_DAY_SHED";
/// The provider worker's flag for it.
pub const FLAG: &str = "--day-shed";
/// The relay's header: the share of the lamp's day used.
pub const HEADER: &str = "x-usage-day-used";
/// Where the worker keeps the reading for the coordinator: the voice's own
/// runtime folder (`RuntimeDirectory=lampos-voice`), gone at a reboot.
pub const FILE: &str = "/run/lampos-voice/day-used.json";
/// From this share, what nobody asked for is shed.
pub const NEAR: f64 = 0.8;
/// At this share, the day is spent.
pub const SPENT: f64 = 1.0;
/// The coordinator reads the file at most this often.
const LOOKED_EVERY: Duration = Duration::from_secs(1);
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// Whether the switch is on in this environment.
pub fn on_from_environment() -> bool {
    on(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// Whether `value` turns it on: `on` exactly.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The provider worker's arguments for the switch in the environment.
pub fn arguments_from_environment() -> Vec<String> {
    arguments(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn arguments(value: Option<&str>) -> Vec<String> {
    if on(value) {
        vec![FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// Take a leading [`FLAG`] off a worker's flags.
pub fn take(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// The header's value as a share of the day used: a finite number from 0
/// to 1, else none.
pub fn used_from(header: &str) -> Option<f64> {
    header
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|used| used.is_finite() && (0.0..=1.0).contains(used))
}

/// The UTC day of `ms` since 1970.
pub fn day_of(ms: u64) -> u64 {
    ms / DAY_MS
}

/// What the lamp last read of its day: no numbers but the share, never a
/// word heard or said.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Reading {
    /// The UTC day, in days since 1970, it is for.
    pub day: u64,
    /// The share at the last answer that said one; `None` when a 2xx
    /// answer came without it (no day limit) or none has said today.
    pub used: Option<f64>,
}

impl Reading {
    /// An answer's header at `now_ms`; `success`: a 2xx answer. Whether
    /// the reading changed.
    pub fn heard(&mut self, now_ms: u64, header: Option<&str>, success: bool) -> bool {
        let before = *self;
        let day = day_of(now_ms);
        if self.day != day {
            *self = Reading { day, used: None };
        }
        match header.map(used_from) {
            Some(Some(used)) => self.used = Some(used),
            Some(None) => {}
            None if success => self.used = None,
            None => {}
        }
        *self != before
    }

    /// How far the day is at `now_ms`: another day's reading is no limit.
    pub fn level(&self, now_ms: u64) -> Level {
        match self.used {
            Some(used) if self.day == day_of(now_ms) => Level::of(used),
            _ => Level::Open,
        }
    }
}

/// How far the lamp's day is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// Under [`NEAR`], or not limited: as ever.
    Open,
    /// From [`NEAR`]: what nobody asked for is shed.
    Near,
    /// At [`SPENT`]: shed, and a refusal is the usage limit at once.
    Spent,
}

impl Level {
    fn of(used: f64) -> Self {
        if used >= SPENT {
            Level::Spent
        } else if used >= NEAR {
            Level::Near
        } else {
            Level::Open
        }
    }

    /// Whether what nobody asked for is shed.
    pub fn sheds(self) -> bool {
        self != Level::Open
    }
}

/// The wall clock, milliseconds since 1970.
pub fn wall_ms() -> u64 {
    crate::privacy_bus::wall_ms()
}

/// The provider worker's reading, read on every answer and kept in a
/// file for the coordinator.
pub struct Worker {
    path: PathBuf,
    reading: Mutex<Reading>,
    /// The level last journaled, so each change is said once.
    said: Mutex<Option<Level>>,
}

/// The worker's one reading, set at its start with its [`FLAG`].
static WORKER: std::sync::OnceLock<Worker> = std::sync::OnceLock::new();

/// At the worker's start, with its [`FLAG`]: the reading kept in [`FILE`].
pub fn start_worker() {
    let _ = WORKER.set(Worker::at(Path::new(FILE)));
}

/// The worker's reading, if the switch is on.
pub fn worker() -> Option<&'static Worker> {
    WORKER.get()
}

/// An answer's header in the worker, with the switch on.
pub fn heard(header: Option<&str>, success: bool) {
    if let Some(worker) = worker() {
        worker.heard(header, success, wall_ms());
    }
}

/// How far the day is, for the worker's own sheds: open with the switch
/// off.
pub fn worker_level() -> Level {
    worker().map_or(Level::Open, |worker| worker.level(wall_ms()))
}

impl Worker {
    /// A reading kept at `path`.
    pub fn at(path: &Path) -> Self {
        Self {
            path: path.to_owned(),
            reading: Mutex::new(Reading::default()),
            said: Mutex::new(None),
        }
    }

    fn reading(&self) -> std::sync::MutexGuard<'_, Reading> {
        self.reading
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// An answer's header at `now_ms`, kept in the file when it changed. A
    /// file that cannot be written leaves the coordinator not shedding,
    /// and nothing else.
    pub fn heard(&self, header: Option<&str>, success: bool, now_ms: u64) {
        let (changed, reading) = {
            let mut reading = self.reading();
            (reading.heard(now_ms, header, success), *reading)
        };
        if !changed {
            return;
        }
        let kept = serde_json::to_string(&reading)
            .map_err(std::io::Error::other)
            .and_then(|text| crate::thread::write_file(&self.path, &text));
        if let Err(error) = kept {
            eprintln!("lamp-voice second: the lamp's day is not kept for the coordinator: {error}");
        }
        self.journal(&reading, now_ms);
    }

    /// How far the day is at `now_ms`.
    pub fn level(&self, now_ms: u64) -> Level {
        self.reading().level(now_ms)
    }

    /// The journal's line when the level changes: the share only.
    fn journal(&self, reading: &Reading, now_ms: u64) {
        let level = reading.level(now_ms);
        let mut said = self
            .said
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *said == Some(level) {
            return;
        }
        *said = Some(level);
        let used = reading.used.map_or_else(
            || "not limited".to_owned(),
            |used| format!("{used:.2} used"),
        );
        let what = match level {
            Level::Open => "nothing shed",
            Level::Near => "what nobody asked for is shed",
            Level::Spent => "what nobody asked for is shed, and a refusal is the usage limit",
        };
        eprintln!("lamp-voice second: the lamp's day is {used}: {what}");
    }
}

/// The coordinator's look at the worker's file: at most once a second,
/// never waiting longer than a file read. Off, or a file missing,
/// unreadable or of another day: open.
pub struct Look {
    on: bool,
    path: PathBuf,
    looked: Option<(Instant, Level)>,
}

impl Look {
    pub fn new(on: bool, path: &Path) -> Self {
        Self {
            on,
            path: path.to_owned(),
            looked: None,
        }
    }

    /// The coordinator's, from the environment and [`FILE`].
    pub fn from_environment() -> Self {
        Self::new(on_from_environment(), Path::new(FILE))
    }

    /// How far the day is now.
    pub fn level(&mut self, now: Instant, now_ms: u64) -> Level {
        if !self.on {
            return Level::Open;
        }
        if let Some((at, level)) = self.looked
            && now.duration_since(at) < LOOKED_EVERY
        {
            return level;
        }
        let level = std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| serde_json::from_str::<Reading>(&text).ok())
            .map_or(Level::Open, |reading| reading.level(now_ms));
        self.looked = Some((now, level));
        level
    }
}

#[cfg(test)]
mod tests;
