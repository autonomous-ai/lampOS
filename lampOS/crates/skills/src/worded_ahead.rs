//! A core skill's line worded ahead by the lamp's agent (the first
//! builder's yes of 19 Nov 10:45 on the Queue, with its three conditions):
//! with `LAMP_CORE_VIA_AGENT` on for a [`crate::core_agent::Group`], its
//! pilots' fires never wait for an agent run. lamp-skills keeps a few lines
//! the agent worded in idle time ([`Kept`]), and a fire says the oldest
//! fresh one at once, or the skill's own line with none.
//!
//! - **What is kept**: at most [`MOST_LINES`] lines a skill, each with when
//!   it was worded, fresh for [`FRESH_MS`] (24 h) and said once
//!   ([`Kept::take`]). Kept in the program's `--state-dir` ([`FILE`]), so a
//!   restart keeps them and the day's count; without one, in this process
//!   alone.
//! - **When a refill runs** ([`Kept::next_refill`]): the lamp idle (no turn
//!   open and none within [`IDLE_AFTER`]), not in the skill's quiet hours,
//!   the board not warm ([`warm`]), the owner's allowance not spent
//!   ([`Allowance`]), at most [`MOST_REFILLS_A_DAY`] a skill in any 24 hours
//!   (a hard cap, about 30 a day a lamp), and one at a time. The run is the
//!   skill's own pre-approved `do.task` with its fixed brief
//!   (`crate::core_agent::script`): nothing private goes in, and the line
//!   handed over says `"ahead": true` ([`marked`]), so lamp-tasks cancels it
//!   for a person's ask or another skill's task at once.
//! - **Each line is checked** as a fire's would be (`crate::core_agent::heard`:
//!   one short line, never `-` or empty) and never an answer for the owner
//!   alone ([`checked`]).
//! - **The journal** says each fire's line ([`said_line`]: the agent's or
//!   its own, and why) and each refill's end ([`refill_line`]: what came of
//!   it, its run time, the CPU lamp-tasks used meanwhile and the day's
//!   count), so a row can count the agent-line rate per skill, the refills a
//!   day and their CPU seconds.
use crate::core_agent::{Heard, Pilot, Task};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

/// The most lines kept for a skill.
pub const MOST_LINES: usize = 3;
/// How long a kept line stays fresh: 24 h.
pub const FRESH_MS: u64 = 24 * 60 * 60 * 1000;
/// The most refills of a skill in any 24 hours, a hard cap (the first
/// builder's condition 2: about 30 runs a day a lamp across 10 skills).
pub const MOST_REFILLS_A_DAY: usize = 3;
/// The window the day's cap counts in.
pub const DAY_MS: u64 = 24 * 60 * 60 * 1000;
/// No refill begins within this of a turn: task news's own quiet
/// (`lamp_voice::task_news::QUIET_US`, 15 s).
pub const IDLE_AFTER: Duration = Duration::from_micros(lamp_voice::task_news::QUIET_US);
/// Until `board/heat` is on the lamp, a refill pauses while any of the
/// board's temperatures (`board/state`) is over this (condition 1).
pub const WARM_C: f32 = 84.0;
/// The board's heat level (#7879): `{"level": "ok"|"warm"|"hot", ...}`,
/// lasting [`READING_LASTS_MS`]. While it is on the lamp it alone decides.
pub const BOARD_HEAT: &str = "board/heat";
/// How long a `board/heat` or `board/state` reading means anything.
pub const READING_LASTS_MS: u64 = 15_000;
/// How often the program looks whether a refill may begin.
pub const LOOK_EVERY: Duration = Duration::from_secs(5);
/// The kept lines and refills, in the program's `--state-dir`.
pub const FILE: &str = "worded-ahead.json";
/// The most bytes of [`FILE`] read.
const MOST_FILE_BYTES: u64 = 64 * 1024;
/// lamp-tasks' cgroup CPU, read before and after each refill: the agent's
/// run is lamp-tasks' child (the lamp's Hermes), so its CPU is counted
/// there, with whatever else lamp-tasks did meanwhile (an upper bound).
/// The product unit's `Slice=lampos-parts.slice` first, then a unit with
/// no slice; the first that exists is read ([`cpu_stat`]).
pub const CPU_STATS: [&str; 2] = [
    "/sys/fs/cgroup/lampos.slice/lampos-parts.slice/lamp-tasks.service/cpu.stat",
    "/sys/fs/cgroup/system.slice/lamp-tasks.service/cpu.stat",
];

/// Why no refill begins now, for the journal.
pub const TALKING: &str = "a turn is open or ended under 15 s ago";
pub const WARM: &str = "the board is warm or hot";
pub const ALLOWANCE_SPENT: &str = "the owner's daily allowance is spent";
pub const QUIET: &str = "in its quiet hours";
pub const CAPPED: &str = "its refills in 24 hours are spent";
pub const FULL: &str = "every kept line is fresh";
pub const OWNER_ALONE: &str = "its agent's answer was for the owner alone";
/// A refill cancelled by lamp-tasks for a person's ask or another skill's
/// task (its `cancelled` refusal).
pub const YIELDED: &str = "it yielded to a person's ask or another skill's task";
/// A refill lamp-tasks was too busy to take: given back, not counted.
pub const BUSY: &str = "lamp-tasks was busy with another run";

/// One line the agent worded ahead.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub words: String,
    /// When it was worded, in Unix ms.
    pub worded_ms: u64,
}

/// One kind of line a pilot says: its skill and, for a skill of more than
/// one task, the task's `on`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kind {
    pub pilot: &'static Pilot,
    pub task: &'static Task,
}

impl Kind {
    /// Its key in [`Kept`]: the skill, with `:on` for a skill of more than
    /// one task.
    pub fn key(&self) -> String {
        match self.task.on {
            Some(on) if self.pilot.tasks.len() > 1 => format!("{}:{on}", self.pilot.name),
            _ => self.pilot.name.to_owned(),
        }
    }

    /// The kind of `skill`'s task whose words are `task`, if it is a pilot's.
    pub fn of(skill: &str, task: &str) -> Option<Self> {
        let pilot = crate::core_agent::pilot(skill)?;
        let task = pilot.tasks.iter().find(|own| own.task == task)?;
        Some(Self { pilot, task })
    }

    /// Every kind of line of the pilots named in `skills`, in [`crate::core_agent::PILOTS`]'
    /// order.
    pub fn all_of<'a>(skills: impl Fn(&str) -> bool + 'a) -> Vec<Self> {
        crate::core_agent::PILOTS
            .iter()
            .filter(|pilot| skills(pilot.name))
            .flat_map(|pilot| pilot.tasks.iter().map(move |task| Self { pilot, task }))
            .collect()
    }
}

/// The owner's daily allowance (#7664, pending): refills come out of it
/// first and stop once it is spent, before anything a person asked for.
/// Until it lands, [`NoAllowanceYet`]: the hard cap alone holds.
pub trait Allowance {
    /// Whether a refill may still be taken out of it today.
    fn allows(&self) -> bool;
    /// A refill ran: `run` long, `cpu_s` of lamp-tasks' CPU if read.
    fn spent(&self, run: Duration, cpu_s: Option<f32>);
}

/// No allowance on the lamp yet: every refill allowed, nothing counted.
pub struct NoAllowanceYet;

/// The allowance refills come out of: the hook #7664's daily allowance
/// takes, [`NoAllowanceYet`] until then.
pub fn allowance() -> &'static (dyn Allowance + Sync) {
    &NoAllowanceYet
}

impl Allowance for NoAllowanceYet {
    fn allows(&self) -> bool {
        true
    }
    fn spent(&self, _run: Duration, _cpu_s: Option<f32>) {}
}

/// What the program knows now, as a refill may begin.
pub struct Now<'a> {
    /// Unix ms.
    pub now_ms: u64,
    /// A turn open or ended under [`IDLE_AFTER`] ago.
    pub talking: bool,
    /// The board warm or hot ([`warm`]).
    pub warm: bool,
    /// Whether `skill` is in its quiet hours now.
    pub quiet: &'a dyn Fn(&str) -> bool,
    pub allowance: &'a dyn Allowance,
}

/// The lines kept and the refills counted.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    /// By [`Kind::key`], oldest first.
    #[serde(default)]
    pub lines: BTreeMap<String, Vec<Line>>,
    /// Each refill begun: its skill and when, in Unix ms.
    #[serde(default)]
    pub refills: Vec<(String, u64)>,
    /// The owner's language file's text the lines were worded under
    /// (`LAMP_LANGUAGE_FILE`): another, and they are let go
    /// ([`Store::in_language`]).
    #[serde(default)]
    pub language: Option<String>,
}

/// What is kept, behind its lock, and its file (Heron's HOLD and P3 on
/// #7903): each write is of a copy taken under a lock of its own, so a
/// fire's take never waits on a write (the copy is taken in a moment), and
/// the newest copy is always the one written last. Nothing here writes on
/// its own: [`Self::save`] is called off the loop that fires skills
/// (Twelve's P3 3 on #7903), and [`Self::write_count`] on the refill's
/// own thread before its hand-over.
#[derive(Debug, Default)]
pub struct Store {
    kept: std::sync::Mutex<Kept>,
    writing: std::sync::Mutex<()>,
    file: Option<std::path::PathBuf>,
    /// How many times the kept lines were let go ([`Self::clear`]): a
    /// refill begun before one keeps nothing (Twelve's P3 1 on #7903).
    cleared: std::sync::atomic::AtomicU64,
}

/// Why no refill began: its count couldn't be written.
pub const NOT_WRITTEN: &str = "worded ahead: no refill begins: its count couldn't be written";
/// Why the kept lines were let go.
pub const FORGOTTEN: &str = "memory forgot everything";
pub const LANGUAGE_CHANGED: &str = "the owner's language changed";
/// Why a refill's line was not kept: the kept lines were let go during its
/// run.
pub const LET_GO: &str = "the kept lines were let go while it ran";

impl Store {
    /// What `file` keeps, if there is one; in this process alone otherwise.
    pub fn at(file: Option<std::path::PathBuf>) -> Self {
        let kept = file.as_deref().map(Kept::load).unwrap_or_default();
        Self {
            kept: std::sync::Mutex::new(kept),
            writing: std::sync::Mutex::new(()),
            file,
            cleared: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// What is kept, locked.
    pub fn kept(&self) -> std::sync::MutexGuard<'_, Kept> {
        self.kept
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// What is kept now, written whole to its file, if it has one.
    pub fn save(&self) -> Result<(), String> {
        let Some(file) = &self.file else {
            return Ok(());
        };
        let _writing = self
            .writing
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let copy = self.kept().clone();
        copy.save(file)
    }

    /// The refill to begin now ([`Kept::next_refill`]) and `today`, its
    /// count of the day's cap, counted; none when none may begin. Its
    /// count is written by [`Self::write_count`] before it is handed over.
    pub fn begin_refill(&self, kinds: &[Kind], now: &Now<'_>) -> Option<(Kind, usize)> {
        let mut kept = self.kept();
        let kind = kept.next_refill(kinds, now).ok()?;
        kept.count_refill(kind.pilot.name, now.now_ms);
        let today = kept.refills_today(kind.pilot.name, now.now_ms);
        Some((kind, today))
    }

    /// A refill of `skill` counted at `at_ms` written to the file before
    /// it is handed over (Heron's HOLD on #7903), so a crash or a restart
    /// during its run keeps the count; not written, the count is given
    /// back and the refill must not begin.
    pub fn write_count(&self, skill: &str, at_ms: u64) -> Result<(), String> {
        self.save().map_err(|why| {
            self.kept().uncount_refill(skill, at_ms);
            format!("{NOT_WRITTEN}: {why}")
        })
    }

    /// How many times the kept lines were let go so far.
    pub fn cleared(&self) -> u64 {
        self.cleared.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Every kept line let go (to be written by [`Self::save`]); the day's
    /// count stays, so the cap holds. A refill under way keeps nothing
    /// ([`Self::cleared`]). Whether any was kept.
    pub fn clear(&self) -> bool {
        let mut kept = self.kept();
        self.cleared
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        let had = !kept.lines.is_empty();
        kept.lines.clear();
        had
    }

    /// The lines kept in the owner's `language` now (the language file's
    /// text): worded under another, they are let go and the new one kept.
    /// Whether anything changed, to be written.
    pub fn in_language(&self, language: Option<&str>) -> bool {
        let language = language.map(str::trim).map(str::to_owned);
        {
            let mut kept = self.kept();
            if kept.language == language {
                return false;
            }
            kept.language = language;
        }
        self.clear();
        true
    }
}

/// Whether a line worded at `worded_ms` is fresh at `now_ms`: under
/// [`FRESH_MS`] old, never from after now.
fn fresh(worded_ms: u64, now_ms: u64) -> bool {
    worded_ms <= now_ms && now_ms - worded_ms < FRESH_MS
}

impl Kept {
    /// What `path` keeps; nothing for a file missing, too large or not
    /// read, so the lamp says its own lines and words them again.
    pub fn load(path: &Path) -> Self {
        use std::io::Read;
        let Ok(file) = std::fs::File::open(path) else {
            return Self::default();
        };
        let mut bytes = Vec::new();
        if file
            .take(MOST_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .is_err()
            || bytes.len() as u64 > MOST_FILE_BYTES
        {
            return Self::default();
        }
        serde_json::from_slice(&bytes).unwrap_or_default()
    }

    /// Written whole to `path`, readable by its owner alone.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_vec(self).map_err(|error| error.to_string())?;
        lamp_update::records::write_whole(path, &json, 0o600)
            .map_err(|error| format!("worded ahead: {error}"))
    }

    /// The fresh lines of `skill`, of every kind, at `now_ms`.
    pub fn fresh_of(&self, skill: &str, now_ms: u64) -> usize {
        self.lines
            .iter()
            .filter(|(key, _)| key.as_str() == skill || key.starts_with(&format!("{skill}:")))
            .map(|(_, lines)| {
                lines
                    .iter()
                    .filter(|line| fresh(line.worded_ms, now_ms))
                    .count()
            })
            .sum()
    }

    /// The oldest fresh line of `kind`, taken so it is said once; the stale
    /// ones let go with it.
    /// [`Self::take`], only while the lines were worded in the owner's
    /// `language` now (the language file's text): another, and none is
    /// said, before the next look lets them go (Twelve's P3 2 on #7903).
    pub fn take_in(&mut self, kind: &Kind, now_ms: u64, language: Option<&str>) -> Option<Line> {
        if self.language.as_deref() != language.map(str::trim) {
            return None;
        }
        self.take(kind, now_ms)
    }

    pub fn take(&mut self, kind: &Kind, now_ms: u64) -> Option<Line> {
        let key = kind.key();
        let lines = self.lines.get_mut(&key)?;
        lines.retain(|line| fresh(line.worded_ms, now_ms));
        let line = (!lines.is_empty()).then(|| lines.remove(0));
        if lines.is_empty() {
            self.lines.remove(&key);
        }
        line
    }

    /// `words` kept for `kind`, worded at `now_ms`; past [`MOST_LINES`] for
    /// its skill, its oldest let go.
    pub fn keep(&mut self, kind: &Kind, words: String, now_ms: u64) {
        let lines = self.lines.entry(kind.key()).or_default();
        lines.retain(|line| fresh(line.worded_ms, now_ms));
        lines.push(Line {
            words,
            worded_ms: now_ms,
        });
        while self.fresh_of(kind.pilot.name, now_ms) > MOST_LINES {
            let Some(lines) = self.lines.get_mut(&kind.key()) else {
                break;
            };
            lines.remove(0);
        }
    }

    /// `skill`'s refills begun in the 24 hours to `now_ms`.
    pub fn refills_today(&self, skill: &str, now_ms: u64) -> usize {
        self.refills
            .iter()
            .filter(|(name, at)| name == skill && *at <= now_ms && now_ms - at < DAY_MS)
            .count()
    }

    /// A refill of `skill` begun at `now_ms`: counted before it is handed
    /// over, so no end, a crash or a restart lets one more through.
    pub fn count_refill(&mut self, skill: &str, now_ms: u64) {
        self.refills
            .retain(|(_, at)| *at <= now_ms && now_ms - at < DAY_MS);
        self.refills.push((skill.to_owned(), now_ms));
    }

    /// `skill`'s refill counted at `at_ms` given back: lamp-tasks never took
    /// it ([`BUSY`]), so nothing ran.
    pub fn uncount_refill(&mut self, skill: &str, at_ms: u64) {
        if let Some(at) = self
            .refills
            .iter()
            .position(|(name, at)| name == skill && *at == at_ms)
        {
            self.refills.remove(at);
        }
    }

    /// lamp-tasks' `answer` to `kind`'s refill counted at `at_ms`, come at
    /// `now_ms`: its line kept once [`checked`]; why not otherwise, and a
    /// refill lamp-tasks was too busy to take given back.
    pub fn refilled(
        &mut self,
        kind: &Kind,
        answer: &str,
        at_ms: u64,
        now_ms: u64,
    ) -> Result<(), &'static str> {
        match checked(answer) {
            Ok(line) => {
                self.keep(kind, line, now_ms);
                Ok(())
            }
            Err(BUSY) => {
                self.uncount_refill(kind.pilot.name, at_ms);
                Err(BUSY)
            }
            Err(why) => Err(why),
        }
    }

    /// The kind to refill now, of `kinds`: the one whose skill has the
    /// fewest fresh lines, under [`MOST_LINES`] and its day's cap, out of
    /// its quiet hours, with the lamp idle, the board cool and the
    /// allowance not spent; why none otherwise.
    pub fn next_refill(&self, kinds: &[Kind], now: &Now<'_>) -> Result<Kind, &'static str> {
        if now.talking {
            return Err(TALKING);
        }
        if now.warm {
            return Err(WARM);
        }
        if !now.allowance.allows() {
            return Err(ALLOWANCE_SPENT);
        }
        let mut why = FULL;
        let mut best: Option<(usize, Kind)> = None;
        for kind in kinds {
            let skill = kind.pilot.name;
            let fresh = self.fresh_of(skill, now.now_ms);
            if fresh >= MOST_LINES {
                continue;
            }
            if self.refills_today(skill, now.now_ms) >= MOST_REFILLS_A_DAY {
                why = CAPPED;
                continue;
            }
            if (now.quiet)(skill) {
                why = QUIET;
                continue;
            }
            // A kind with no line of its own first, then the skill with the
            // fewest.
            let own = self.lines.get(&kind.key()).map_or(0, |lines| {
                lines.iter().filter(|line| fresh_at(line, now)).count()
            });
            let rank = own * (MOST_LINES + 1) + fresh;
            if best.is_none_or(|(least, _)| rank < least) {
                best = Some((rank, *kind));
            }
        }
        best.map(|(_, kind)| kind).ok_or(why)
    }
}

fn fresh_at(line: &Line, now: &Now<'_>) -> bool {
    fresh(line.worded_ms, now.now_ms)
}

/// Whether the board is warm, from the newest `board/heat` (`heat`, its
/// age in ms) while there is one and it is fresh, or else from the
/// newest `board/state` (`state`): any temperature over [`WARM_C`]. With
/// neither fresh, not warm: nothing says so.
pub fn warm(
    heat: Option<(&serde_json::Value, u64)>,
    state: Option<(&serde_json::Value, u64)>,
) -> bool {
    fn lasting((value, age): (&serde_json::Value, u64)) -> Option<&serde_json::Value> {
        (age < READING_LASTS_MS).then_some(value)
    }
    if let Some(heat) = heat.and_then(lasting)
        && let Some(level) = heat["level"].as_str()
    {
        return matches!(level, "warm" | "hot");
    }
    state.and_then(lasting).is_some_and(|state| {
        state["temperature_c"].as_object().is_some_and(|zones| {
            zones
                .values()
                .filter_map(serde_json::Value::as_f64)
                .any(|celsius| celsius > f64::from(WARM_C))
        })
    })
}

/// A refill's hand-over line: `line`, lamp-skills' `do.task` line, with
/// `"ahead": true`, so lamp-tasks cancels it for a person's ask.
pub fn marked(line: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(line) {
        Ok(serde_json::Value::Object(mut fields)) => {
            fields.insert("ahead".into(), serde_json::Value::Bool(true));
            serde_json::Value::Object(fields).to_string()
        }
        _ => line.to_owned(),
    }
}

/// The line to keep from lamp-tasks' `answer` to a refill, checked as a
/// fire's line is: why not otherwise.
pub fn checked(answer: &str) -> Result<String, &'static str> {
    let read = serde_json::from_str::<serde_json::Value>(answer.trim()).unwrap_or_default();
    if read["owner_alone"] == serde_json::Value::Bool(true) {
        return Err(OWNER_ALONE);
    }
    match read["error"].as_str() {
        Some("cancelled") => return Err(YIELDED),
        Some("busy") => return Err(BUSY),
        _ => {}
    }
    match crate::core_agent::heard(crate::task::told(answer)) {
        Heard::Line(line) => Ok(line),
        Heard::Nothing(why) => Err(why),
    }
}

/// The first of [`CPU_STATS`] that exists, if any.
pub fn cpu_stat() -> Option<&'static Path> {
    CPU_STATS.iter().map(Path::new).find(|stat| stat.exists())
}

/// lamp-tasks' cgroup CPU in microseconds (`usage_usec` of a
/// [`CPU_STATS`] file's form), if it could be read.
pub fn cpu_usec(stat: &Path) -> Option<u64> {
    let text = std::fs::read_to_string(stat).ok()?;
    text.lines()
        .find_map(|line| line.strip_prefix("usage_usec "))
        .and_then(|value| value.trim().parse().ok())
}

/// The journal's words for a fire's line, never the agent's: `line` the
/// kept one said, worded `ago_ms` before, or why its own was.
pub fn said_line(line: Result<(&Line, u64), &str>) -> String {
    match line {
        Ok((line, ago_ms)) => format!(
            "worded ahead: said its agent's line ({} characters, worded {:.1} h ago)",
            line.words.chars().count(),
            ago_ms as f64 / 3_600_000.0
        ),
        Err(why) => format!("worded ahead: said its own line: {why}"),
    }
}

/// Why a fire says its own line with nothing fresh kept.
pub const NONE_FRESH: &str = "no fresh line of its agent's kept";

/// The journal's words for a refill's end: `kind`'s key, what came of it,
/// its run time and lamp-tasks' CPU meanwhile, and `today` of the day's
/// cap.
pub fn refill_line(
    kind: &Kind,
    outcome: Result<(), &str>,
    run: Duration,
    cpu_s: Option<f32>,
    today: usize,
) -> String {
    format!(
        "worded ahead: refill of {}: {}; run {:.1} s, cpu {}, refill {today} of {MOST_REFILLS_A_DAY} in 24 hours",
        kind.key(),
        match outcome {
            Ok(()) => "its agent's line kept".to_owned(),
            Err(why) => format!("nothing kept: {why}"),
        },
        run.as_secs_f32(),
        cpu_s.map_or_else(|| "not read".to_owned(), |cpu| format!("{cpu:.1} s")),
    )
}

#[cfg(test)]
mod tests;
