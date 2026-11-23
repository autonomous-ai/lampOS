//! The conversation's watches (#7761, slice 1): "tell me when the air gets
//! stuffy". The model takes a watch with a tool, the lamp watches one fixed
//! fact for a while, says ONE line when it holds, and the watch is over.
//! Behind [`ENVIRONMENT`] (`LAMP_WATCH`, off); in no unit until its row.
//!
//! - **One process, many cycles.** The tool is called in the provider worker
//!   (`tools.rs`'s `Route::Watch`), so the watches AND the watcher thread live
//!   there: the thread is started at the worker's start ([`resume`]) or by
//!   the first `watch` ([`ensure_started`]) and tells through the say
//!   service's socket ([`say`]) as lamp-tasks' skills do, never through the
//!   coordinator's own queue (Heron's P1 on #7782). The worker lives for one
//!   cycle: a mute or a hand's sleep ends it and the next cycle starts a new
//!   one (Heron's P2 on #7782), so the list is kept in [`FILE`] and read by
//!   each new worker.
//! - **The file:** `/run/lampos-voice/watches.json`, 0600, on tmpfs (gone at
//!   a reboot): facts only (kind, level or time, when it ends), no names, no
//!   words said, no face ids. Written whole beside itself and moved over it
//!   on every take, stop and fire. One that cannot be read is no watches and
//!   one journal line, never a crash. The voice's `forget_everything`, once
//!   done, clears it ([`forget_all`]).
//! - **The facts** (a fixed list, never a free question): someone comes into
//!   view, leaves it, or turns to the lamp (the faces in view as COUNTS only,
//!   never who: a name is never read here); the air's CO₂ above a level
//!   (`air/state`); this sitting passing a mark (`room/activity`'s
//!   `sitting_s`); a time of day (the lamp's own zone).
//! - **Bounds:** at most [`MOST`] at once, each at most [`LONGEST_US`] (a
//!   day); kept on `/run`, gone at a reboot; `stop_watching` ends them all. Nothing is asked of the person.
//! - **Told once:** when a fact holds the watch is removed and the lamp's own
//!   say service is handed an instruction for the model to word
//!   (a `note` on its socket) with the fact in it, so there is no
//!   fixed spoken line. It goes through the say service's gates (paused,
//!   nobody there, a turn owed, the speaker busy, a quiet since the last
//!   turn) and is not begun after [`EXPIRES_MS`]: a late "the air is stuffy"
//!   is dropped, never said later. Handed over within a second of the reading
//!   that made it true (air comes every 5 s from its sensor, so for air that
//!   is within a second of the reading); WHEN it is spoken is the say
//!   service's.
//! - **Nothing is said while the lamp may not speak.** `pause` and the
//!   quiet are the say service's own gates; the Policy's are not asked there
//!   (`docs/skills.md`), so the watcher asks for itself ([`may_speak`]): not
//!   in the private room (`LAMP_PRIVATE_ROOM`), not while Expression says the
//!   lamp is asleep. While it may not: a LEVEL fact (air, sitting, a time)
//!   keeps watching and is told when the lamp may; an EDGE fact (someone
//!   came, left, turned) is over and unsaid, since telling it later would be
//!   wrong.
use lamp_bus::Bus;
use lamp_msgs::{AirState, Faces, RoomActivity};
use serde_json::{Value, json};
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::{thread, time::Duration};

/// `on` for the tools to be declared and the watcher to run.
pub const ENVIRONMENT: &str = "LAMP_WATCH";
/// The provider worker's flag for it.
pub const FLAG: &str = "--watch";
/// The tools.
pub const WATCH: &str = "watch";
pub const STOP: &str = "stop_watching";
/// At most this many watches at once, each at most a day.
pub const MOST: usize = 5;
pub const LONGEST_US: u64 = 24 * 3_600 * 1_000_000;
/// Not begun in a minute: a late line is dropped.
pub const EXPIRES_MS: u64 = 60_000;
/// The person asked to be told, as a reminder is: a say needing a second of
/// quiet after the last turn.
const PRIORITY: u8 = crate::say::URGENT_FROM;
/// The say service's name for these asks.
const SKILL: &str = "watch";
/// The bus's name for this reader.
const SUBSCRIBER: &str = "voice_watch";
/// The longest a read waits before the clock is looked at.
const READ_WITHIN: Duration = Duration::from_millis(200);
/// How long a face must be gone to be "away", and in view to be "arrived":
/// as the developer door counts them (`lamp-control`'s `watch.rs`).
pub const ABSENT_FOR_US: u64 = 3_000_000;
pub const LEFT_AFTER_US: u64 = 5_000_000;
/// The CO₂ levels a watch may name, in ppm.
pub const PPM_LEAST: u32 = 400;
pub const PPM_MOST: u32 = 5_000;
/// The sitting marks a watch may name, in minutes.
pub const SITTING_LEAST_MIN: u32 = 1;
pub const SITTING_MOST_MIN: u32 = 12 * 60;
/// What the model says when told "stuffy" without a level: the sensor's own
/// bar for a stuffy room (`AirFeel::of_co2`).
pub const STUFFY_PPM: u32 = 1_200;

/// A time of day told later than this after it is dropped, unsaid: the lamp
/// that slept through "tell me at 7:00" does not say "it is that time now" at
/// 10:30 (Heron's P2 on #7782).
pub const AT_LATE_MS: u64 = 5 * 60_000;

/// What the person hears, in English words the model says in their language.
pub const FULL: &str = "I'm already watching five things. Tell me to stop watching first.";
pub const STOPPED: &str = "Okay, I've stopped watching.";
pub const NOTHING: &str = "I wasn't watching anything.";
pub const NO_WATCHER: &str = "I can't watch for that right now.";

/// A fact to watch for.
#[derive(Clone, Debug, PartialEq)]
pub enum Fact {
    Arrives,
    Leaves,
    TurnsToLamp,
    /// CO₂ at or above this many ppm.
    Co2Above(u32),
    /// This sitting at least this many seconds long.
    SittingOver(u32),
    /// The lamp's local time reaches this unix millisecond.
    At(u64),
}

#[derive(Clone, Debug)]
pub struct Watch {
    fact: Fact,
    until_us: u64,
}

/// What a bus message tells the watches.
#[derive(Clone, Debug, PartialEq)]
pub enum Reading {
    /// Faces in view now (their number, and whether any is turned to the
    /// lamp): counts, never who.
    People {
        count: usize,
        facing: bool,
    },
    Air {
        co2_ppm: Option<u32>,
    },
    Sitting {
        sitting_s: u32,
    },
    /// The wall clock, in unix milliseconds.
    Clock {
        unix_ms: u64,
    },
}

/// The watches, and the little the arrive/leave edges need to know.
#[derive(Debug, Default)]
pub struct Watches {
    list: Vec<Watch>,
    present: bool,
    seen_us: Option<u64>,
    facing: bool,
    /// Whether a reading of the faces has been taken as the baseline: the
    /// first one after a start is who is there already, never news (Sixth's
    /// P2 on #7782: a watch kept across a cycle must not fire on someone who
    /// was in the room before this worker started).
    primed: bool,
}

/// Why a watch was not taken.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    Full,
}

impl Watches {
    /// Takes a watch for `fact` for `for_us` (at most a day) from `now_us`.
    pub fn add(&mut self, fact: Fact, now_us: u64, for_us: u64) -> Result<(), Refused> {
        self.expire(now_us);
        if self.list.len() >= MOST {
            return Err(Refused::Full);
        }
        self.list.push(Watch {
            fact,
            until_us: now_us.saturating_add(for_us.min(LONGEST_US)),
        });
        Ok(())
    }

    /// How many are being watched.
    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Ends every watch: how many there were.
    pub fn stop_all(&mut self) -> usize {
        std::mem::take(&mut self.list).len()
    }

    /// The list as the file keeps it: facts only.
    pub fn to_json(&self) -> Value {
        let watches: Vec<Value> = self
            .list
            .iter()
            .map(|watch| {
                let (kind, n) = match watch.fact {
                    Fact::Arrives => ("arrives", 0),
                    Fact::Leaves => ("leaves", 0),
                    Fact::TurnsToLamp => ("turns_to_lamp", 0),
                    Fact::Co2Above(ppm) => ("co2_above", u64::from(ppm)),
                    Fact::SittingOver(secs) => ("sitting_over", u64::from(secs)),
                    Fact::At(at) => ("at_time", at),
                };
                json!({"kind": kind, "n": n, "until_us": watch.until_us})
            })
            .collect();
        json!({"watches": watches})
    }

    /// Takes the list `to_json` made, at `now_us`: what is over, unknown or
    /// past [`MOST`] is left out; one that is not that shape is `None`.
    pub fn from_json(value: &Value, now_us: u64) -> Option<Vec<Watch>> {
        let mut list = Vec::new();
        for entry in value.get("watches")?.as_array()? {
            let n = entry.get("n").and_then(Value::as_u64).unwrap_or(0);
            let fact = match entry.get("kind").and_then(Value::as_str) {
                Some("arrives") => Fact::Arrives,
                Some("leaves") => Fact::Leaves,
                Some("turns_to_lamp") => Fact::TurnsToLamp,
                Some("co2_above") => Fact::Co2Above(u32::try_from(n).ok()?),
                Some("sitting_over") => Fact::SittingOver(u32::try_from(n).ok()?),
                Some("at_time") => Fact::At(n),
                _ => continue,
            };
            let Some(until_us) = entry.get("until_us").and_then(Value::as_u64) else {
                continue;
            };
            if until_us <= now_us || list.len() >= MOST {
                continue;
            }
            list.push(Watch {
                fact,
                until_us: until_us.min(now_us.saturating_add(LONGEST_US)),
            });
        }
        Some(list)
    }

    /// Ends the watches whose time is past: how many.
    pub fn expire(&mut self, now_us: u64) -> usize {
        let before = self.list.len();
        self.list.retain(|watch| watch.until_us > now_us);
        before - self.list.len()
    }

    /// A reading at `now_us`: the instructions for the model of each watch
    /// it makes true. Each such watch is over.
    pub fn read(&mut self, reading: &Reading, now_us: u64, may_speak: bool) -> Vec<String> {
        self.expire(now_us);
        let mut true_now: Vec<&'static str> = Vec::new();
        let mut told: Vec<String> = Vec::new();
        if let Reading::People { count, facing } = reading {
            if !self.primed {
                // The baseline only: nothing is told, nobody "arrives".
                self.primed = true;
                self.present = *count > 0;
                self.seen_us = (*count > 0).then_some(now_us);
                self.facing = *facing;
            } else {
                if *count > 0 {
                    let away = self
                        .seen_us
                        .is_none_or(|seen| now_us.saturating_sub(seen) >= ABSENT_FOR_US);
                    if !self.present && away {
                        true_now.push("arrives");
                    }
                    self.present = true;
                    self.seen_us = Some(now_us);
                } else if self.present
                    && self
                        .seen_us
                        .is_some_and(|seen| now_us.saturating_sub(seen) >= LEFT_AFTER_US)
                {
                    self.present = false;
                    true_now.push("leaves");
                }
                if *facing != self.facing {
                    self.facing = *facing;
                    if *facing {
                        true_now.push("turns");
                    }
                }
            }
        }
        let mut kept = Vec::new();
        for watch in std::mem::take(&mut self.list) {
            // A time long past is over, said to no one.
            if let (Fact::At(at), Reading::Clock { unix_ms }) = (&watch.fact, reading)
                && unix_ms.saturating_sub(*at) > AT_LATE_MS
            {
                continue;
            }
            let said = match (&watch.fact, reading) {
                (Fact::Arrives, _) if true_now.contains(&"arrives") => Some(
                    "Someone has just come into view of the lamp. Say, in one short line, \
                     that you see someone has come in. Do not say who."
                        .to_owned(),
                ),
                (Fact::Leaves, _) if true_now.contains(&"leaves") => Some(
                    "Nobody is in view of the lamp any more. Say, in one short line, that \
                     they have gone."
                        .to_owned(),
                ),
                (Fact::TurnsToLamp, _) if true_now.contains(&"turns") => Some(
                    "Someone has just turned to the lamp. Say, in one short line, that \
                     someone is looking your way. Do not say who."
                        .to_owned(),
                ),
                (Fact::Co2Above(level), Reading::Air { co2_ppm: Some(now) }) if now >= level => {
                    Some(format!(
                        "The air's carbon dioxide is now {now} ppm, over the {level} ppm they \
                         asked you to watch. Say, in one short line, the number and that \
                         the air is stuffy, and that opening a window may help."
                    ))
                }
                (Fact::SittingOver(secs), Reading::Sitting { sitting_s }) if sitting_s >= secs => {
                    Some(format!(
                        "They have been sitting for {} minutes, past the {} minutes they \
                         asked you to watch. Say, in one short line, how long, and that a \
                         break may be good.",
                        sitting_s / 60,
                        secs / 60
                    ))
                }
                (Fact::At(at), Reading::Clock { unix_ms }) if unix_ms >= at => Some(
                    "It is the time they asked you to tell them. Say, in one short line, \
                     that it is that time now."
                        .to_owned(),
                ),
                _ => None,
            };
            let edge = matches!(watch.fact, Fact::Arrives | Fact::Leaves | Fact::TurnsToLamp);
            match said {
                // It holds but the lamp may not speak: a level waits for
                // its next reading, an edge is over (said to no one).
                Some(_) if !may_speak && !edge => kept.push(watch),
                Some(_) if !may_speak => {}
                Some(note) => told.push(note),
                None => kept.push(watch),
            }
        }
        self.list = kept;
        told
    }
}

/// The process's watches: the tool takes them, the watcher thread reads them.
pub static WATCHES: Mutex<Watches> = Mutex::new(Watches {
    list: Vec::new(),
    present: false,
    seen_us: None,
    facing: false,
    primed: false,
});

/// Where the watches are kept: on the voice's own runtime folder
/// (`RuntimeDirectory=lampos-voice`, 0700), tmpfs.
pub const FILE: &str = "/run/lampos-voice/watches.json";

/// A test's own file: a test never writes under `/run`.
#[cfg(test)]
static FILE_AT: Mutex<Option<PathBuf>> = Mutex::new(None);

#[cfg(not(test))]
fn file() -> Option<PathBuf> {
    Some(PathBuf::from(FILE))
}

#[cfg(test)]
fn file() -> Option<PathBuf> {
    FILE_AT.lock().ok().and_then(|at| at.clone())
}

/// Writes happen one at a time, each from the list as it is when its turn
/// comes: the file is never older than a later write.
static SAVING: Mutex<()> = Mutex::new(());

/// Writes the watches as they are now. A failure is one journal line: the
/// watches are still held in this process.
fn save() {
    let Some(path) = file() else { return };
    let _one = SAVING.lock().unwrap_or_else(|e| e.into_inner());
    let kept = match WATCHES.lock() {
        Ok(watches) => watches.to_json(),
        Err(poisoned) => poisoned.into_inner().to_json(),
    };
    if let Err(error) = write_whole(&path, &kept) {
        eprintln!("lamp-voice: watches not kept: {error}");
    }
}

/// `value` written beside `path` (0600) and moved over it.
fn write_whole(path: &std::path::Path, value: &Value) -> std::io::Result<()> {
    let new = path.with_extension("json.new");
    let _ = std::fs::remove_file(&new);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&new)?;
    file.write_all(value.to_string().as_bytes())?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&new, path)
}

/// What a new worker takes up: the file's watches that are not over. A file
/// that is not there is none; one that cannot be read is none and a journal
/// line.
fn load(path: &std::path::Path, now_us: u64) -> Vec<Watch> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            eprintln!("lamp-voice: watches file not read, none taken up: {error}");
            return Vec::new();
        }
    };
    match serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|value| Watches::from_json(&value, now_us))
    {
        Some(list) => list,
        None => {
            eprintln!("lamp-voice: watches file is not the shape it is kept in, none taken up");
            Vec::new()
        }
    }
}

/// Takes up what the file keeps, as a new worker does: the watches of the
/// cycle before.
pub fn restore(now_us: u64) -> usize {
    let Some(path) = file() else { return 0 };
    let list = load(&path, now_us);
    let kept = list.len();
    if let Ok(mut watches) = WATCHES.lock() {
        watches.list = list;
    }
    kept
}

/// Ends every watch and removes the file.
pub fn forget_all() {
    let _one = SAVING.lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(mut watches) = WATCHES.lock() {
        watches.stop_all();
    }
    if let Some(path) = file()
        && let Err(error) = std::fs::remove_file(&path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!("lamp-voice: watches file not removed: {error}");
    }
}

/// The worker's start with `LAMP_WATCH` on: the watches of the cycle before
/// are taken up and the watcher runs, so one taken before a mute or a sleep
/// is still told after it.
pub fn resume() {
    let _ = ensure_started();
}

/// The provider worker's flag, with the switch `on`.
pub fn arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("on") {
        vec![FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

pub fn is_tool(name: &str) -> bool {
    name == WATCH || name == STOP
}

/// The tools.
pub fn tools() -> Vec<lamp_gemini::Tool> {
    vec![
        lamp_gemini::Tool {
            name: WATCH.into(),
            description: "Call this when the person asks you to tell them when something \
                          happens: \"tell me when the air gets stuffy\", \"let me know when \
                          someone comes in\", \"tell me when I've been sitting an hour\", \
                          \"remind me at 3:30\". Pick the one fact that fits. Nothing is asked \
                          of them. Then say what it gives you as \"say\", in a few words, in \
                          their language. You will be told when it happens, once."
                .into(),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "what": {
                        "type": "string",
                        "enum": ["arrives", "leaves", "turns_to_lamp", "co2_above", "sitting_over", "at_time"],
                        "description": "arrives: someone comes into view. leaves: nobody is in view any more. turns_to_lamp: someone turns to face the lamp. co2_above: the air's carbon dioxide reaches a level (\"stuffy\"). sitting_over: this sitting reaches a length. at_time: a time of day."
                    },
                    "ppm": {
                        "type": "integer",
                        "description": "co2_above only: the level in ppm. Leave out for \"stuffy\" (1200)."
                    },
                    "minutes": {
                        "type": "integer",
                        "description": "sitting_over only: how many minutes of sitting."
                    },
                    "time": {
                        "type": "string",
                        "description": "at_time only: the time of day, 24-hour HH:MM, in the lamp's own time."
                    },
                    "for_hours": {
                        "type": "integer",
                        "description": "How long to keep watching, in hours, at most 24. Leave out for the day."
                    }
                },
                "required": ["what"]
            })),
            silent: false,
        },
        lamp_gemini::Tool {
            name: STOP.into(),
            description: "Call this when the person asks you to stop watching, or to stop \
                          telling them when things happen. It ends every watch. Then say what \
                          it gives you as \"say\", in a few words, in their language."
                .into(),
            parameters: None,
            silent: false,
        },
    ]
}

/// The fact a `watch` call names, and how long: or why not, in words for the
/// model.
pub fn parse(
    args: &Value,
    now_unix_ms: u64,
    minute_of_day: Option<u16>,
) -> Result<(Fact, u64), String> {
    let for_us = match args.get("for_hours") {
        None | Some(Value::Null) => LONGEST_US,
        Some(hours) => match hours.as_u64() {
            Some(h) if (1..=24).contains(&h) => h * 3_600 * 1_000_000,
            _ => return Err("for_hours is 1 to 24".to_owned()),
        },
    };
    let number = |key: &str| args.get(key).and_then(Value::as_u64);
    let fact = match args.get("what").and_then(Value::as_str) {
        Some("arrives") => Fact::Arrives,
        Some("leaves") => Fact::Leaves,
        Some("turns_to_lamp") => Fact::TurnsToLamp,
        Some("co2_above") => {
            let ppm = match args.get("ppm") {
                None | Some(Value::Null) => u64::from(STUFFY_PPM),
                Some(_) => number("ppm").ok_or("ppm is a whole number")?,
            };
            if !(u64::from(PPM_LEAST)..=u64::from(PPM_MOST)).contains(&ppm) {
                return Err(format!("ppm is {PPM_LEAST} to {PPM_MOST}"));
            }
            Fact::Co2Above(ppm as u32)
        }
        Some("sitting_over") => {
            let minutes = number("minutes").ok_or("minutes is a whole number")?;
            if !(u64::from(SITTING_LEAST_MIN)..=u64::from(SITTING_MOST_MIN)).contains(&minutes) {
                return Err(format!(
                    "minutes is {SITTING_LEAST_MIN} to {SITTING_MOST_MIN}"
                ));
            }
            Fact::SittingOver(minutes as u32 * 60)
        }
        Some("at_time") => {
            let text = args
                .get("time")
                .and_then(Value::as_str)
                .ok_or("time is HH:MM")?;
            let (h, m) = text.split_once(':').ok_or("time is HH:MM")?;
            let (h, m): (u16, u16) = (
                h.trim().parse().map_err(|_| "time is HH:MM")?,
                m.trim().parse().map_err(|_| "time is HH:MM")?,
            );
            if h > 23 || m > 59 {
                return Err("time is HH:MM, 00:00 to 23:59".to_owned());
            }
            let now = minute_of_day.ok_or("I don't know the time here yet")?;
            // The next time the clock says it, today or tomorrow.
            let wait_min = (u64::from(h) * 60 + u64::from(m) + 1_440 - u64::from(now)) % 1_440;
            if wait_min == 0 {
                return Err("that is this very minute: say a later time".to_owned());
            }
            // A time farther away than the watch lasts would never be told:
            // refused, not taken (Heron's P3 on #7782).
            if wait_min * 60_000_000 + 60_000_000 > for_us {
                return Err(format!(
                    "that time is {} hours away, more than the watch lasts: say for_hours up to 24",
                    wait_min / 60
                ));
            }
            // To the start of that minute.
            let into_minute_ms = now_unix_ms % 60_000;
            Fact::At(now_unix_ms - into_minute_ms + wait_min * 60_000)
        }
        _ => {
            return Err(
                "what is one of arrives, leaves, turns_to_lamp, co2_above, sitting_over, at_time"
                    .to_owned(),
            );
        }
    };
    Ok((fact, for_us))
}

/// The tool's answer: `watch` takes a watch, `stop_watching` ends them all.
pub fn run(name: &str, args: &Value) -> (Value, bool) {
    let now_us = lamp_ipc::monotonic_us();
    if name == STOP {
        let ended = WATCHES
            .lock()
            .map(|mut watches| watches.stop_all())
            .unwrap_or(0);
        if ended > 0 {
            save();
        }
        let say = if ended == 0 { NOTHING } else { STOPPED };
        return (json!({"ok": true, "say": say}), true);
    }
    let unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or(0);
    let minute = crate::local_time::system().map(|zone| zone.minute_of_day(unix_ms));
    match parse(args, unix_ms, minute) {
        Err(why) => (json!({"error": why}), false),
        Ok(_) if !started() => (json!({"error": NO_WATCHER}), false),
        Ok((fact, for_us)) => {
            let taken = WATCHES
                .lock()
                .map(|mut watches| watches.add(fact, now_us, for_us))
                .unwrap_or(Err(Refused::Full));
            match taken {
                Ok(()) => {
                    save();
                    (
                        json!({"ok": true, "say": "Okay, I'll tell you when it happens."}),
                        true,
                    )
                }
                Err(Refused::Full) => (json!({"error": FULL}), false),
            }
        }
    }
}

/// Whether the watcher runs, started now if it is not: always so in a test
/// (the tests take watches without a bus).
fn started() -> bool {
    #[cfg(test)]
    {
        true
    }
    #[cfg(not(test))]
    ensure_started()
}

/// Starts the watcher on a thread of its own, once in this process: whether
/// it is running. Needs the bus; one that cannot be read says so to the
/// person (`watch` answers an error) rather than taking a watch nothing
/// would tell.
pub fn ensure_started() -> bool {
    static STARTED: OnceLock<bool> = OnceLock::new();
    *STARTED.get_or_init(|| {
        let started = Bus::from_environment().and_then(|bus| {
            // The cycle before's watches, before the thread reads any.
            let kept = restore(lamp_ipc::monotonic_us());
            if kept > 0 {
                eprintln!("lamp-voice: {kept} watches taken up from the cycle before");
            }
            start_on(&bus, socket())
        });
        if let Err(error) = &started {
            eprintln!("lamp-voice: watches are off: {error}");
        }
        started.is_ok()
    })
}

/// The watcher on `bus`, telling the say service at `say_socket`.
pub fn start_on(bus: &Bus, say_socket: PathBuf) -> std::io::Result<thread::JoinHandle<()>> {
    let faces = bus.subscribe_typed(Faces::TOPIC, SUBSCRIBER, Faces::TYPE)?;
    let air = bus.subscribe_typed(AirState::TOPIC, SUBSCRIBER, AirState::TYPE)?;
    let activity = bus.subscribe_typed(RoomActivity::TOPIC, SUBSCRIBER, RoomActivity::TYPE)?;
    let again = Bus::from_environment().ok();
    thread::Builder::new()
        .name("lamp-voice-watch".into())
        .spawn(move || run_watcher(faces, air, activity, again, say_socket))
}

fn run_watcher(
    mut faces: lamp_bus::Subscriber,
    mut air: lamp_bus::Subscriber,
    mut activity: lamp_bus::Subscriber,
    bus: Option<Bus>,
    say_socket: PathBuf,
) {
    loop {
        let mut readings: Vec<Reading> = Vec::new();
        match faces.recv(READ_WITHIN) {
            Ok(Some(message)) => {
                if let Ok(seen) = message.parse::<Faces>() {
                    readings.push(Reading::People {
                        count: seen.faces.len(),
                        facing: seen.faces.iter().any(|face| face.facing == Some(true)),
                    });
                }
            }
            Ok(None) => {}
            // Not a tight loop on a bus gone wrong.
            Err(_) => thread::sleep(READ_WITHIN),
        }
        while let Ok(Some(message)) = air.recv(Duration::ZERO) {
            if let Ok(state) = message.parse::<AirState>() {
                readings.push(Reading::Air {
                    co2_ppm: state.co2_ppm.map(u32::from),
                });
            }
        }
        while let Ok(Some(message)) = activity.recv(Duration::ZERO) {
            if let Ok(room) = message.parse::<RoomActivity>() {
                readings.push(Reading::Sitting {
                    sitting_s: room.sitting_s,
                });
            }
        }
        let unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_millis() as u64)
            .unwrap_or(0);
        readings.push(Reading::Clock { unix_ms });
        let now_us = lamp_ipc::monotonic_us();
        // Asked before the lock is taken: the tool's take never waits on it.
        let may = may_speak(bus.as_ref(), now_us);
        let mut told = Vec::new();
        let mut changed = false;
        // Read even with none watched: who is in view must be known when
        // "tell me when someone comes in" is asked, or a person already
        // there would be told as coming in.
        if let Ok(mut watches) = WATCHES.lock() {
            let before = watches.len();
            for reading in &readings {
                told.extend(watches.read(reading, now_us, may));
            }
            changed = watches.len() != before;
        }
        // One told or one over is not kept: written outside the lock.
        if changed {
            save();
        }
        for note in told {
            say(&say_socket, &note);
        }
    }
}

/// Whether the lamp may speak unasked now: not in the private room, and not
/// while Expression says it is asleep (its state, fresh).
pub fn may_speak(bus: Option<&Bus>, now_us: u64) -> bool {
    if crate::person::private_room() {
        return false;
    }
    let asleep = bus
        .and_then(|bus| bus.latest(lamp_msgs::ExpressionState::TOPIC).ok().flatten())
        .is_some_and(|message| crate::ring_night::asleep_in(&message, now_us));
    !asleep
}

/// The say service's socket, as the voice's own worker finds it for its
/// other asks (`agent_asks`): the default one.
fn socket() -> PathBuf {
    PathBuf::from(crate::say::DEFAULT_SOCKET)
}

/// The line the say service is asked: a `note` the model words.
pub fn note_line(note: &str) -> Value {
    json!({"skill": SKILL, "note": note, "priority": PRIORITY, "expires_in_ms": EXPIRES_MS})
}

/// Hands the instruction to the say service's socket without waiting; its
/// outcome is journalled from a thread of its own, without the words.
fn say(path: &std::path::Path, note: &str) {
    let line = note_line(note);
    let path = path.to_path_buf();
    let _ = thread::Builder::new()
        .name("lamp-voice-watch-say".into())
        .spawn(move || {
            let outcome = say_at(&path, &line);
            eprintln!("lamp-voice: a watch: {outcome}");
        });
}

/// One ask on the socket at `path`: what the say service answered.
pub fn say_at(path: &std::path::Path, line: &Value) -> String {
    let Ok(mut stream) = UnixStream::connect(path) else {
        return "the say service is not there; not told".to_owned();
    };
    let wait = Duration::from_millis(EXPIRES_MS) + Duration::from_secs(30);
    if stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .is_err()
        || writeln!(stream, "{line}").is_err()
        || stream.set_read_timeout(Some(wait)).is_err()
    {
        return "the say service did not take it".to_owned();
    }
    let mut lines = BufReader::new(stream);
    let mut text = String::new();
    loop {
        text.clear();
        match lines.read_line(&mut text) {
            Ok(0) | Err(_) => return "the say service went quiet".to_owned(),
            Ok(_) => {
                let told: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                // Begun: read on for its end.
                if let Some(outcome) = told["outcome"].as_str() {
                    return outcome.to_owned();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
