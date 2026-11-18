//! A question about a skill on the lamp, answered from the lamp's own
//! records (`LAMP_SKILL_STATE_TOLD`). The owner, 16 Nov, on a lamp: "is my
//! water reminder on or off?" was answered "I don't know if it's on or
//! off", since nothing the model had said a skill's state, and a read-back
//! looked among the skills the person made only. The owner's rule: whatever
//! a person asks about a skill on the lamp is answered from the lamp's
//! records, never guessed.
//!
//! - **One plain path, the model's to choose.** `use_skill` takes
//!   [`ABOUT`], the skill as the person said it (its name or what it does),
//!   given alone (`crate::use_skill`): whether a line asks about a skill is
//!   the model's judgment, never a list of words (the owner's ruling, 13
//!   Nov). A parameter of the function the voice already uses for its
//!   skills, never a new function (#7335): a session's functions are the
//!   same by name and count. It names any skill on the lamp, not only those
//!   `skill` can name (a skill with no function, Welcome back's, has no
//!   name there), so it is a word of the person's, not a name from a list.
//! - **Every skill on the lamp, from its own files.** Each folder of the
//!   skills folder and the pushed folder ([`Places`]): its `SKILL.md`'s
//!   `description`, its `skill.toml`'s `enabled` (its `[present]`'s with
//!   `LAMP_NUDGES_PRESENT=on`, as `lamp-skills` loads it), its name in
//!   `lamp-skills`' `off` file, and its snooze, its `[rest]` and its last
//!   run as `lamp-skills` keeps them in `holds.json` beside its tools file.
//!   The model is told each skill's state in one short line, and the few
//!   whose words come nearest to what was asked ([`nearest`]) in full:
//!   which of them the person means is the model's to judge.
//! - **Bounded.** At most [`LISTED_MOST`] skills listed and [`NEAREST_MOST`]
//!   told in full, each description cut at [`DOES_MOST`] bytes, each file
//!   read at most [`FILE_MOST`] bytes; no socket but one, lamp-control's
//!   yes to the nearest skill's task, when it has one.
//! - **Names to the owner.** A skill the person made here is named only
//!   when the owner hears (`services::owner_hears`), as a list's names
//!   are; a turn nobody spoke is told nothing.
//! - **A reminder may be a skill.** A [`LIST_REMINDERS`] answer carries
//!   every skill's line too ([`with_reminders`]): "is my water reminder on
//!   or off?" may call the schedule, not `use_skill` (#7335's row on
//!   lamp-9adf), and the model judges whether a skill fits. So does a
//!   `make_skill` answer that tells the person about the skills they made
//!   without making or asking anything ([`with_made`]): a skill they ask
//!   about may not be one they made.
//!
//! Off unless set, and then `use_skill`, `list_reminders`' answer and
//! `make_skill`'s answers are exactly as before.
use crate::made_skill;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Only the exact value `on`; off unless set.
pub const ENVIRONMENT: &str = "LAMP_SKILL_STATE_TOLD";

/// `lamp-skills`' own switch for a skill's `[present]` values, read here
/// from the voice's environment so a skill is told on or off as
/// `lamp-skills` runs it.
pub const PRESENT_ENVIRONMENT: &str = "LAMP_NUDGES_PRESENT";

#[cfg(test)]
thread_local! {
    /// The switch as a test of this thread sets it.
    static ON_HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether the switch is on in this process.
pub fn on() -> bool {
    #[cfg(test)]
    if let Some(on) = ON_HERE.with(std::cell::Cell::get) {
        return on;
    }
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// The switch for this test's thread alone.
#[cfg(test)]
pub fn set_on_here(on: Option<bool>) {
    ON_HERE.with(|here| here.set(on));
}

/// The `use_skill` argument that asks about a skill on the lamp: the
/// skill as the person said it, given alone.
pub const ABOUT: &str = "about";

/// What `use_skill`'s parameter says of [`ABOUT`].
pub const ABOUT_TOLD: &str = "Only when the person asks about a skill on the lamp, any skill, listed under \"Your skills\" or not: whether it is on, off or resting, what it does or what starts it, when it last ran, or whether it waits for their yes. The skill as they said it, its name or what it does, given alone with no skill. Never answer that from the conversation.";

/// What `use_skill`'s description adds with the switch.
pub const USE_SKILL_TOLD: &str =
    " To answer a question about any skill on the lamp, call it with about alone.";

/// Who answers a call with [`ABOUT`], for the journal.
pub const WHOM: &str = "the skills' records";

/// The skill asked about in a `use_skill` call, with the switch on.
pub fn asked(call: &lamp_gemini::ToolCall) -> Option<&str> {
    if !on() {
        return None;
    }
    call.args.get(ABOUT).and_then(Value::as_str)
}

/// What the model is told when the skills or their state can't be read.
pub const CANT_CHECK: &str =
    "the lamp can't check its skills right now: say so plainly, in a few words, and guess nothing";

/// What the model is told of a question asked in a turn nobody spoke.
pub const NOT_ASKED_ALOUD: &str = "the lamp's skills are told only when the person asks out loud";

/// `lamp-skills`' state folder (the product unit's `--state-dir`).
pub const STATE_DIR: &str = "/var/lib/lampos-skills-state";
/// The skills turned off by voice or in the app, a name a line, in it.
pub const OFF_FILE: &str = "off";
/// `lamp-skills`' tools folder (the product unit's `--tools-dir`), where
/// it keeps [`HOLDS_FILE`].
pub const TOOLS_DIR: &str = "/run/lampos-skills";
/// `lamp-skills`' holds and the nudges' limits (`lamp_skills::holds::FILE`).
pub const HOLDS_FILE: &str = "holds.json";

/// The most skills listed with their state.
pub const LISTED_MOST: usize = 80;
/// The most skills told in full.
pub const NEAREST_MOST: usize = 3;
/// The most bytes of a skill's description told.
pub const DOES_MOST: usize = 400;
/// The most bytes read of any one file.
pub const FILE_MOST: u64 = 64 * 1024;

/// Where the skills and their state are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Places {
    pub installed: PathBuf,
    pub pushed: PathBuf,
    pub state: PathBuf,
    pub tools: PathBuf,
}

impl Places {
    /// The product's, with `tools` the voice's skills folder when it has
    /// one (`LAMP_SKILLS_DIR`).
    pub fn product(tools: Option<&Path>) -> Self {
        let installed = PathBuf::from(crate::skill_words::INSTALLED);
        Self {
            pushed: made_skill::pushed_folder(&installed),
            installed,
            state: PathBuf::from(STATE_DIR),
            tools: tools
                .or_else(|| NOTED_TOOLS_DIR.get().map(PathBuf::as_path))
                .map_or_else(|| PathBuf::from(TOOLS_DIR), Path::to_path_buf),
        }
    }
}

/// The voice's skills folder (`--skills-dir`, the voice's `settings.skills`),
/// noted as the worker reads its flags: a route with no settings (the
/// schedule's) still reads the same records ([`Places::product`]).
static NOTED_TOOLS_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Note the voice's skills folder, once.
pub fn note_tools_dir(folder: &Path) {
    let _ = NOTED_TOOLS_DIR.set(folder.to_path_buf());
}

/// A skill's state, as the lamp's records say it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    On,
    /// On by its files and the `off` file, but `holds.json` isn't there:
    /// `lamp-skills` writes it at its first turn whenever it runs, and
    /// removes it when a write fails, so whether it is snoozed or resting
    /// can't be told.
    OnRestUnknown,
    /// Its name is in the `off` file: turned off by voice or in the app.
    TurnedOff,
    /// Its `skill.toml` says `enabled = false`.
    InstalledOff,
    /// Its `skill.toml` is missing or isn't TOML: `lamp-skills` loads none.
    DoesNotLoad,
    /// Snoozed by its stop function until the next local 06:00.
    Snoozed,
    /// Resting after nudges nobody noticed, this many more minutes.
    Resting(u64),
}

impl State {
    /// In one or two words, for a skill's line.
    fn short(&self) -> &'static str {
        match self {
            Self::On => "on",
            Self::OnRestUnknown => "on, its snooze can't be checked right now",
            Self::TurnedOff | Self::InstalledOff | Self::DoesNotLoad => "off",
            Self::Snoozed => "snoozed until tomorrow morning",
            Self::Resting(_) => "resting",
        }
    }

    /// In full, for a skill told whole.
    fn said(&self) -> String {
        match self {
            Self::On => "on".into(),
            Self::OnRestUnknown => "on, but whether it is snoozed or resting, and when it last ran, can't be checked right now".into(),
            Self::TurnedOff => {
                "off: turned off by voice or in the app, until it is turned on".into()
            }
            Self::InstalledOff => {
                "off: it is on the lamp turned off, until it is turned on in the app".into()
            }
            Self::DoesNotLoad => "off: its files on the lamp don't load".into(),
            Self::Snoozed => "snoozed: it does nothing by itself until tomorrow morning".into(),
            Self::Resting(minutes) => format!(
                "resting: after nudges nobody noticed, it does nothing by itself for about {minutes} more minutes"
            ),
        }
    }
}

/// A skill on the lamp, as its files and `lamp-skills`' records say it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: Option<String>,
    pub state: State,
    pub made_here: bool,
    /// It hands a task to the lamp's agent (`do.task`), which waits for
    /// the owner's yes.
    pub task: bool,
    /// When it last ran by itself, on the monotonic clock, as `lamp-skills`
    /// keeps it.
    pub ran_us: Option<u64>,
}

/// A record of `lamp-skills`' that may be absent: none when it isn't
/// there, why when it is there and can't be read.
fn read_record(path: &Path) -> Result<Option<String>, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let mut text = String::new();
    file.take(FILE_MOST)
        .read_to_string(&mut text)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(Some(text))
}

/// The names in `lamp-skills`' `off` file, a name a line; none when there
/// is no file (no skill turned off), why when it can't be read.
pub fn turned_off(state: &Path) -> Result<BTreeSet<String>, String> {
    Ok(read_record(&state.join(OFF_FILE))?
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

/// What this module reads of `lamp-skills`' holds file: by skill, its
/// snooze, its rest and its last run, each on the monotonic clock.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Holds {
    holds: Vec<Hold>,
    limits: Limits,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Hold {
    skill: String,
    ran_us: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Limits {
    skills: BTreeMap<String, SkillLimits>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SkillLimits {
    ran_us: Option<u64>,
    snoozed_until_us: Option<u64>,
    rest: Option<Rest>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Rest {
    at_us: u64,
    resting_until_us: u64,
}

/// `lamp-skills`' holds file; none when it isn't there (lamp-skills not
/// yet at its first turn, or a write of it failed: snoozes and rests
/// unknown, [`State::OnRestUnknown`]), why when it can't be read.
fn holds(tools: &Path) -> Result<Option<Holds>, String> {
    match read_record(&tools.join(HOLDS_FILE))? {
        None => Ok(None),
        Some(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|error| format!("{HOLDS_FILE} can't be read: {error}")),
    }
}

/// Whether `skill.toml`'s text turns the skill on, and whether it hands
/// a task over, as `lamp-skills` loads it (none when it isn't TOML, or
/// isn't there: `lamp-skills` loads no such skill): its top-level `enabled`
/// (true unless said), and with `present` its `[present]`'s `enabled` and
/// `[[present.do]]` in their place.
fn file_says(text: &str, present: bool) -> Option<(bool, bool)> {
    let table = toml::from_str::<toml::Table>(text).ok()?;
    let present_table = present
        .then(|| table.get("present").and_then(toml::Value::as_table))
        .flatten();
    let enabled = present_table
        .and_then(|present| present.get("enabled"))
        .or_else(|| table.get("enabled"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(true);
    let steps = present_table
        .and_then(|present| present.get("do"))
        .or_else(|| table.get("do"))
        .and_then(toml::Value::as_array);
    let task = steps.is_some_and(|steps| {
        steps.iter().any(|step| {
            step.as_table()
                .is_some_and(|step| step.contains_key("task"))
        })
    });
    Some((enabled, task))
}

/// Whether a `SKILL.md`'s front matter says it was made on this lamp.
fn made_here(card: &str) -> bool {
    card.strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---"))
        .is_some_and(|(front, _)| front.lines().any(|line| line.trim() == "made: here"))
}

/// Every skill on the lamp at `places`, by name, its state as of `now_us`
/// (the monotonic clock) with `present` (`LAMP_NUDGES_PRESENT`); at most
/// [`LISTED_MOST`]. Why not, when the skills folder or a record of
/// `lamp-skills`' that is there can't be read: never a guess.
pub fn skills(places: &Places, now_us: u64, present: bool) -> Result<Vec<Skill>, String> {
    let off = turned_off(&places.state)?;
    let holds = holds(&places.tools)?;
    let rests_known = holds.is_some();
    let holds = holds.unwrap_or_default();
    let mut folders: BTreeMap<String, PathBuf> = BTreeMap::new();
    for folder in [&places.installed, &places.pushed] {
        let entries = match std::fs::read_dir(folder) {
            Ok(entries) => entries,
            // No pushed skill: no pushed folder.
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound && folder == &places.pushed =>
            {
                continue;
            }
            Err(error) => return Err(format!("{}: {error}", folder.display())),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if name.starts_with('.') || !path.join("SKILL.md").is_file() {
                continue;
            }
            folders.entry(name).or_insert(path);
        }
    }
    folders
        .into_iter()
        .take(LISTED_MOST)
        .map(|(name, folder)| -> Result<Skill, String> {
            let card = read_record(&folder.join("SKILL.md"))?.unwrap_or_default();
            let says =
                read_record(&folder.join("skill.toml"))?.and_then(|text| file_says(&text, present));
            let (enabled, task) = says.unwrap_or((false, false));
            let limits = holds.limits.skills.get(&name);
            let state = if says.is_none() {
                State::DoesNotLoad
            } else if !enabled {
                State::InstalledOff
            } else if off.contains(&name) {
                State::TurnedOff
            } else if limits
                .and_then(|limits| limits.snoozed_until_us)
                .is_some_and(|until| until > now_us)
            {
                State::Snoozed
            } else if let Some(until) = limits
                .and_then(|limits| limits.rest.as_ref())
                .map(|rest| rest.resting_until_us)
                .filter(|until| *until > now_us)
            {
                State::Resting((until - now_us).div_ceil(60_000_000))
            } else if rests_known {
                State::On
            } else {
                State::OnRestUnknown
            };
            let ran_us = holds
                .holds
                .iter()
                .filter(|hold| hold.skill == name)
                .filter_map(|hold| hold.ran_us)
                .chain(limits.and_then(|limits| limits.ran_us))
                .chain(
                    limits
                        .and_then(|limits| limits.rest.as_ref())
                        .map(|rest| rest.at_us),
                )
                .filter(|at| *at > 0 && *at <= now_us)
                .max();
            Ok(Skill {
                description: made_skill::written_description(&card),
                made_here: made_here(&card),
                name,
                state,
                task,
                ran_us,
            })
        })
        .collect()
}

/// `text`'s words, lowercase, letters and digits.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.chars().count() >= 2)
        .map(str::to_lowercase)
        .collect()
}

/// Whether two words are one: the same, or one the other's start when
/// both are four letters or more ("reminder", "reminders").
fn same(a: &str, b: &str) -> bool {
    a == b || (a.len().min(b.len()) >= 4 && (a.starts_with(b) || b.starts_with(a)))
}

/// Of `skills`, at most [`NEAREST_MOST`] whose name or description share
/// the most words with `asked`, each word weighed by how few skills have it
/// (a word every skill says counts for little), a name's twice a
/// description's. Which of them, if any, the person means is the model's.
pub fn nearest<'a>(asked: &str, skills: &'a [Skill]) -> Vec<&'a Skill> {
    let asked: BTreeSet<String> = words(asked).into_iter().collect();
    let texts: Vec<(Vec<String>, Vec<String>)> = skills
        .iter()
        .map(|skill| {
            (
                words(&skill.name),
                words(skill.description.as_deref().unwrap_or_default()),
            )
        })
        .collect();
    let has = |words: &[String], word: &str| words.iter().any(|other| same(word, other));
    let mut scored: Vec<(f64, &Skill)> = skills
        .iter()
        .zip(&texts)
        .map(|(skill, (name, does))| {
            let score = asked
                .iter()
                .map(|word| {
                    let among = texts
                        .iter()
                        .filter(|(name, does)| has(name, word) || has(does, word))
                        .count()
                        .max(1) as f64;
                    if has(name, word) {
                        2.0 / among
                    } else if has(does, word) {
                        1.0 / among
                    } else {
                        0.0
                    }
                })
                .sum::<f64>();
            (score, skill)
        })
        .filter(|(score, _)| *score > 0.0)
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    scored
        .into_iter()
        .take(NEAREST_MOST)
        .map(|(_, skill)| skill)
        .collect()
}

/// `text` cut to at most [`DOES_MOST`] bytes, at a word's end.
fn cut(text: &str) -> String {
    if text.len() <= DOES_MOST {
        return text.to_owned();
    }
    let mut end = DOES_MOST;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let head = &text[..end];
    let head = head.rsplit_once(' ').map_or(head, |(head, _)| head);
    format!("{}…", head.trim_end())
}

/// How long ago `ran_us` was, at `now_us`, in plain words.
fn ago(ran_us: u64, now_us: u64) -> String {
    let minutes = now_us.saturating_sub(ran_us) / 60_000_000;
    match minutes {
        0 => "just now".into(),
        1 => "a minute ago".into(),
        2..=89 => format!("{minutes} minutes ago"),
        _ => format!("about {} hours ago", (minutes + 30) / 60),
    }
}

/// What the model is told after the facts.
pub const NEXT: &str = "These are the lamp's own records: every skill on it with its state, and those whose words come nearest to what they asked in full. Answer what they asked from these alone, in a few words: on, off, snoozed or resting, what it does and what starts it, when it last ran, whether it waits for their yes. Never guess: what is not given here the lamp doesn't keep, so say so. If none is the skill they mean, say no skill on the lamp is that one (a reminder they set themselves is not a skill). Only answer: offer to change, turn on or turn off nothing. Don't call use_skill again for it.";

/// The schedule's function whose answer carries the skills' lines too
/// ([`with_reminders`]): the owner, 16 Nov, "is my water reminder on or
/// off?" called it on lamp-9adf (#7335's row), and with no reminder set it
/// said nothing was on, while Sip of water was.
pub const LIST_REMINDERS: &str = "list_reminders";
/// The key of the skills' lines added to its answer.
pub const SKILLS_ON_THE_LAMP: &str = "skills_on_the_lamp";
/// The key of what the model is told of them.
pub const SKILLS_NOTE: &str = "skills_note";
/// What the model is told beside the skills' lines in a reminders answer.
pub const REMINDERS_NOTE: &str = "A reminder they ask about may be one of the lamp's own skills, each listed in skills_on_the_lamp with its state from the lamp's records. When one fits what they asked, answer from its line too, in a few words; never say a skill is off or missing unless its line says so. Only answer: offer to change, turn on or turn off nothing.";
/// What the model is told in a reminders answer when the skills' records
/// can't be read.
pub const REMINDERS_CANT_CHECK: &str = "The lamp can't check its skills right now: if they may mean one of the lamp's skills, say you can't check that right now, and guess nothing.";

/// The answer to a question `asked` about a skill on the lamp at `places`
/// at `now_us` (the monotonic clock) with `present`, in the voice's request
/// `request` (`heard`: someone spoke it); names of skills made here only
/// when `owner_hears`, asked only when there is one; `waits(name)` says
/// whether a skill's task waits for the owner's yes (none: it has no task;
/// why not when it can't be told), asked only of the nearest skill with a
/// task. The answer is cut to what the link carries ([`fit`]).
#[allow(clippy::too_many_arguments)]
pub fn told(
    asked: &str,
    places: &Places,
    now_us: u64,
    present: bool,
    request: u64,
    heard: bool,
    owner_hears: impl FnOnce() -> bool,
    waits: impl FnOnce(&str) -> Option<Result<bool, String>>,
) -> Value {
    if !heard {
        eprintln!("lamp-voice: request {request}: a skill's state not told: no one asked aloud");
        return json!({ "error": NOT_ASKED_ALOUD });
    }
    let (skills, not_named) = match heard_skills(places, now_us, present, owner_hears) {
        Ok(found) => found,
        Err(why) => {
            eprintln!("lamp-voice: request {request}: skills' state not told: {why}");
            return json!({ "error": CANT_CHECK });
        }
    };
    let nearest = nearest(asked, &skills);
    // Counts only: the journal holds no skill's name and no words.
    eprintln!(
        "lamp-voice: request {request}: skills' state told: {} listed, {} in full",
        skills.len(),
        nearest.len()
    );
    let mut waits = Some(waits);
    let full: Vec<Value> = nearest
        .iter()
        .map(|skill| {
            let mut facts = json!({
                "name": made_skill::spoken_name(&skill.name),
                "state": skill.state.said(),
            });
            if let Some(description) = &skill.description {
                facts["does"] = json!(cut(description));
            }
            if let Some(ran_us) = skill.ran_us {
                facts["last_ran_by_itself"] = json!(ago(ran_us, now_us));
            }
            if skill.task
                && let Some(waits) = waits.take()
                && let Some(waiting) = waits(&skill.name)
            {
                match waiting {
                    Ok(waiting) => {
                        facts["waits_for_yes"] = json!(if waiting {
                            "yes: the task it hands to the lamp's agent waits for the owner's yes"
                        } else {
                            "no: the owner said yes to its task"
                        });
                    }
                    // Never a guess: not reached in time is not a wait.
                    Err(_) => facts[YES_UNCHECKED] = json!(YES_UNCHECKED_TOLD),
                }
            }
            facts
        })
        .collect();
    let mut told = json!({
        "asked": asked,
        "nearest": full,
        "skills": lines(&skills),
        "next": NEXT,
    });
    if not_named > 0 {
        told["made_here_not_named"] = json!(not_named);
    }
    if !fit(&mut told, &["skills", "nearest"]) {
        eprintln!("lamp-voice: request {request}: skills' state not told: too long to send");
        return json!({ "error": CANT_CHECK });
    }
    told
}

/// The key of how many lines [`fit`] left out.
pub const LEFT_OUT: &str = "skills_left_out";

/// `told` cut to what the provider link carries (`tools::sendable`): the
/// entries of `keys`' lists dropped from the end, the first key's first,
/// how many said in [`LEFT_OUT`]; false when even with them all gone it
/// doesn't fit.
fn fit(told: &mut Value, keys: &[&str]) -> bool {
    let mut left_out = 0;
    for key in keys {
        while !crate::tools::sendable(told) {
            let Some(entries) = told.get_mut(*key).and_then(Value::as_array_mut) else {
                break;
            };
            if entries.pop().is_none() {
                break;
            }
            left_out += 1;
            told[LEFT_OUT] = json!(left_out);
        }
    }
    crate::tools::sendable(told)
}

/// The key of a yes that can't be checked ([`YES_UNCHECKED_TOLD`]).
pub const YES_UNCHECKED: &str = "waits_for_yes_unchecked";
/// What the model is told when lamp-control or the manage helper can't
/// say in time whether a skill's task waits for the owner's yes.
pub const YES_UNCHECKED_TOLD: &str =
    "the lamp can't check right now whether its task waits for the owner's yes";
/// The most a question about a skill waits for whether its task waits for
/// the owner's yes, the manage helper and lamp-control together, so first
/// audio isn't held (#7345's P3); a yes itself waits as before.
pub const YES_CHECK_MOST: std::time::Duration = std::time::Duration::from_millis(1_500);

/// Whether the skill `name`'s task waits for the owner's yes, asked of the
/// manage helper at `manage` and lamp-control at `control` within `most`
/// together: none when it has no task; why not when either can't say in
/// time, never a wait guessed (#7345's P2).
pub fn task_waits(
    manage: &Path,
    control: &Path,
    name: &str,
    most: std::time::Duration,
) -> Option<Result<bool, String>> {
    let started = std::time::Instant::now();
    let asks = match crate::skill_task_yes::read_within(manage, name, most) {
        Ok(asks) => asks?,
        Err(why) => return Some(Err(why)),
    };
    let left = most.saturating_sub(started.elapsed());
    if left.is_zero() {
        return Some(Err("no time left to ask lamp-control".into()));
    }
    Some(
        crate::skill_task_yes::agreed_within(control, &asks.name, &asks.sha256, &asks.grants, left)
            .map(|agreed| agreed != crate::skill_task_yes::Agreement::Agreed),
    )
}

/// Every skill on the lamp ([`skills`]), but those made here when the
/// owner doesn't hear (`owner_hears`, asked only when there is one), and
/// how many were left out so.
fn heard_skills(
    places: &Places,
    now_us: u64,
    present: bool,
    owner_hears: impl FnOnce() -> bool,
) -> Result<(Vec<Skill>, usize), String> {
    let mut skills = skills(places, now_us, present)?;
    let made = skills.iter().filter(|skill| skill.made_here).count();
    let mut not_named = 0;
    if made > 0 && !owner_hears() {
        skills.retain(|skill| !skill.made_here);
        not_named = made;
    }
    Ok((skills, not_named))
}

/// Each skill's line: its spoken name and its state in a word.
fn lines(skills: &[Skill]) -> Vec<String> {
    skills
        .iter()
        .map(|skill| {
            format!(
                "{}: {}",
                made_skill::spoken_name(&skill.name),
                skill.state.short()
            )
        })
        .collect()
}

/// A [`LIST_REMINDERS`] answer `told`, in the voice's request `request`
/// (`heard`: someone spoke it), given the skills' lines at `places` too,
/// as a question with `about` gives them, with [`REMINDERS_NOTE`]: a
/// reminder the person asks about may be a skill, and which one, if any,
/// is the model's to judge. Names of skills made here only when
/// `owner_hears`; nothing added for a turn nobody spoke; with a record
/// there unreadable, [`REMINDERS_CANT_CHECK`] alone. An answer that is not
/// an object is kept under `result`.
pub fn with_reminders(
    told: &mut Value,
    places: &Places,
    now_us: u64,
    present: bool,
    request: u64,
    heard: bool,
    owner_hears: impl FnOnce() -> bool,
) {
    let notes = Notes {
        what: "reminders",
        told: REMINDERS_NOTE,
        cant_check: REMINDERS_CANT_CHECK,
    };
    with_lines(
        told,
        notes,
        places,
        now_us,
        present,
        request,
        heard,
        owner_hears,
    );
}

/// What an answer given the skills' lines is told of them, and of none
/// when they can't be read; `what` names the answer in the journal.
#[derive(Clone, Copy)]
struct Notes {
    what: &'static str,
    told: &'static str,
    cant_check: &'static str,
}

/// `told` given every skill's line ([`SKILLS_ON_THE_LAMP`]) and `notes`'
/// note ([`SKILLS_NOTE`]), as [`with_reminders`] says.
#[allow(clippy::too_many_arguments)]
fn with_lines(
    told: &mut Value,
    notes: Notes,
    places: &Places,
    now_us: u64,
    present: bool,
    request: u64,
    heard: bool,
    owner_hears: impl FnOnce() -> bool,
) {
    if !heard {
        return;
    }
    let before = told.clone();
    if !told.is_object() {
        *told = json!({ "result": told.take() });
    }
    match heard_skills(places, now_us, present, owner_hears) {
        Ok((skills, not_named)) => {
            eprintln!(
                "lamp-voice: request {request}: {} told with {} skills' lines",
                notes.what,
                skills.len()
            );
            told[SKILLS_ON_THE_LAMP] = json!(lines(&skills));
            told[SKILLS_NOTE] = json!(notes.told);
            if not_named > 0 {
                told["made_here_not_named"] = json!(not_named);
            }
        }
        Err(why) => {
            eprintln!(
                "lamp-voice: request {request}: {} told without skills: {why}",
                notes.what
            );
            told[SKILLS_NOTE] = json!(notes.cant_check);
        }
    }
    // Within what the link carries (#7360's P2: past it, Live gets no
    // answer at all): lines left out from the end, counted; with not even
    // the note fitting, the answer as it was.
    if !fit(told, &[SKILLS_ON_THE_LAMP]) {
        eprintln!(
            "lamp-voice: request {request}: {} told without skills: too long to send",
            notes.what
        );
        *told = before;
    }
}

/// What the model is told beside the skills' lines in a `make_skill`
/// answer about the skills the person made ([`with_made`]).
pub const MADE_NOTE: &str = "A skill they ask about may be any skill on the lamp, not only one they made: each is listed in skills_on_the_lamp with its state from the lamp's records, whether made here, added from a file, from the Store or the lamp's own. When one fits what they asked, answer from its line too, in a few words, though the words above name only those they made; never say they have none like it, or that it is off, unless its line says so. Only answer: offer to change, turn on or turn off nothing.";
/// What the model is told in such an answer when the skills' records
/// can't be read.
pub const MADE_CANT_CHECK: &str = "The lamp can't check its other skills right now: if they may mean a skill they didn't make, say you can't check that right now, and guess nothing.";

/// Whether a `make_skill` answer `told` to a call with `args` tells the
/// person about their skills without making or asking anything (#7335's
/// row on lamp-9adf: "do I have a skill that reads my calendar when I sit
/// down?" called `make_skill` for the list of those they made, and a skill
/// added from a file was said not to be there): the list of those they
/// made (`made`), whatever it says, and a run, a remove or a read-back
/// (`run`, `remove`, `read_back`), or with `LAMP_MADE_SKILL_OFF_SAID` on a
/// turn off or on (`turn_off`, `turn_on`, #7445), that refused, found none
/// or found another (`error`) with no yes awaited. Never a make, a clash's
/// choice or a yes, nor a turn that offers the person's own skills to call
/// again with (`made_turn::CANDIDATES`): told "offer to turn on or turn off
/// nothing" beside the lamp's skills, the model called an added skill the
/// lamp's own (#7562's row on lamp-9adf, 18 Nov).
pub fn tells_their_skills(args: &Value, told: &Value) -> bool {
    if *told == crate::tools::unavailable() || told.get(crate::made_turn::CANDIDATES).is_some() {
        return false;
    }
    if args[crate::made_list::LIST] == true {
        return true;
    }
    let named = [
        crate::made_run::RUN,
        crate::made_remove::REMOVE,
        crate::make_skill_local::READ_BACK,
    ]
    .iter()
    .any(|key| args[*key].is_string())
        // A turn-off or a turn back on (#7445), with its switch on alone.
        || (crate::made_skill::off_said_on()
            && [crate::made_turn::TURN_OFF, crate::made_turn::TURN_ON]
                .iter()
                .any(|key| args[*key].is_string()));
    named
        && told.get("error").is_some()
        && told.get(crate::skill_write_yes::APPROVE_ID).is_none()
        && *told != crate::tools::unavailable()
}

/// A `make_skill` answer `told` to a call with `args`, given the skills'
/// lines with [`MADE_NOTE`] when it tells the person about their skills
/// ([`tells_their_skills`]), as [`with_reminders`] gives a reminders
/// answer them; any other answer untouched.
#[allow(clippy::too_many_arguments)]
pub fn with_made(
    args: &Value,
    told: &mut Value,
    places: &Places,
    now_us: u64,
    present: bool,
    request: u64,
    heard: bool,
    owner_hears: impl FnOnce() -> bool,
) {
    if !tells_their_skills(args, told) {
        return;
    }
    let notes = Notes {
        what: "the skills they made",
        told: MADE_NOTE,
        cant_check: MADE_CANT_CHECK,
    };
    with_lines(
        told,
        notes,
        places,
        now_us,
        present,
        request,
        heard,
        owner_hears,
    );
}

/// [`with_made`] on the lamp, with the switch on, for a `make_skill`
/// `call`: the product's places with the voice's skills folder `skills`,
/// the owner's hearing as [`told_here`]'s. Off, `told` is untouched.
pub fn with_made_here(
    call: &lamp_gemini::ToolCall,
    told: &mut Value,
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
    skills: Option<&Path>,
) {
    if !on() {
        return;
    }
    let present = std::env::var(PRESENT_ENVIRONMENT).ok().as_deref() == Some("on");
    with_made(
        &call.args,
        told,
        &Places::product(skills),
        lamp_ipc::monotonic_us(),
        present,
        request,
        heard,
        || crate::services::owner_hears(request, heard, lease),
    );
}

/// [`with_reminders`] on the lamp, for the call `name` with the switch on
/// (only [`LIST_REMINDERS`]) that the schedule `answered` without an
/// `error`: the product's places with the voice's skills folder
/// ([`note_tools_dir`]), the owner's hearing as [`told_here`]'s. Off,
/// another call or a failure, `told` is untouched.
pub fn with_reminders_here(
    name: &str,
    told: &mut Value,
    answered: bool,
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
) {
    // A failure (not answered, unavailable, uncertain) is kept as it is.
    if !on() || name != LIST_REMINDERS || !answered || told.get("error").is_some() {
        return;
    }
    let present = std::env::var(PRESENT_ENVIRONMENT).ok().as_deref() == Some("on");
    with_reminders(
        told,
        &Places::product(None),
        lamp_ipc::monotonic_us(),
        present,
        request,
        heard,
        || crate::services::owner_hears(request, heard, lease),
    );
}

/// [`told`] on the lamp, by `use_skill`'s call: the product's places,
/// `skills` the voice's skills folder, the owner's hearing as a list's
/// (`services::owner_hears`), and a task's yes as lamp-control keeps it
/// (`skill_task_yes::asks_by`, with a skill's tasks on).
pub fn told_here(
    asked: &str,
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
    skills: Option<&Path>,
) -> Value {
    let present = std::env::var(PRESENT_ENVIRONMENT).ok().as_deref() == Some("on");
    told(
        asked,
        &Places::product(skills),
        lamp_ipc::monotonic_us(),
        present,
        request,
        heard,
        || crate::services::owner_hears(request, heard, lease),
        |name| {
            if !crate::skill_task_yes::on() {
                return None;
            }
            task_waits(
                Path::new(made_skill::MANAGE_SOCKET),
                Path::new(crate::skill_task_yes::CONTROL_SOCKET),
                name,
                YES_CHECK_MOST,
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: u64 = 60_000_000;

    /// A lamp with `skills`, each its folder name, its `SKILL.md`
    /// description and its `skill.toml`, under a fresh folder.
    fn lamp(test: &str, skills: &[(&str, &str, &str)]) -> (PathBuf, Places) {
        let root = std::env::temp_dir().join(format!("skill-about-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let places = Places {
            installed: root.join("skills"),
            pushed: root.join("skills-dev"),
            state: root.join("state"),
            tools: root.join("tools"),
        };
        for folder in [
            &places.installed,
            &places.pushed,
            &places.state,
            &places.tools,
        ] {
            std::fs::create_dir_all(folder).unwrap();
        }
        // lamp-skills running: its holds file written, nothing held.
        std::fs::write(places.tools.join(HOLDS_FILE), "{}").unwrap();
        for (name, description, toml) in skills {
            let folder = places.installed.join(name);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(
                folder.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n"),
            )
            .unwrap();
            std::fs::write(folder.join("skill.toml"), toml).unwrap();
        }
        (root, places)
    }

    const SIP: &str = "After someone has sat at the desk an hour and a half, the lamp turns to them and its light breathes once, a slow cool blue, a hint to drink some water.";

    fn product_lamp(test: &str) -> (PathBuf, Places) {
        lamp(
            test,
            &[
                (
                    "sip-of-water",
                    SIP,
                    "kind = \"core\"\n[[do]]\nsay = \"x\"\n",
                ),
                (
                    "comfort-break",
                    "After two hours at the desk, one short line that it's a good moment for a short break.",
                    "enabled = false\nkind = \"core\"\n[present]\nenabled = true\n",
                ),
                (
                    "stretch-break",
                    "After a long sitting, the lamp leans back as a hint to stretch.",
                    "kind = \"core\"\n",
                ),
                (
                    "reminders",
                    "Reminders the person sets by voice, said at their time.",
                    "kind = \"core\"\n",
                ),
                (
                    "morning-mail",
                    "Each morning, reads the newest mail's subjects.",
                    "[[do]]\ntask = { task = \"read\", run = \"main.py\" }\n",
                ),
                ("dance", "Dances when asked.", "enabled = false\n"),
            ],
        )
    }

    fn by_name<'a>(skills: &'a [Skill], name: &str) -> &'a Skill {
        skills.iter().find(|skill| skill.name == name).unwrap()
    }

    /// The owner, 16 Nov: each skill's state from the lamp's records:
    /// `enabled = false`, its `[present]` with the switch, a name in `off`,
    /// a snooze and a rest in `holds.json`, and its last run there.
    #[test]
    fn each_skills_state_is_read_from_the_lamps_records() {
        let (root, places) = product_lamp("state");
        std::fs::write(places.state.join(OFF_FILE), "stretch-break\nnot-a-skill\n").unwrap();
        let now = 1_000 * MINUTE;
        std::fs::write(
            places.tools.join(HOLDS_FILE),
            json!({
                "version": 2, "boot": "b", "written_us": now,
                "holds": [{"skill": "sip-of-water", "trigger": 0, "fingerprint": "f",
                           "since_us": 1, "last_held_us": 2, "ran_us": now - 42 * MINUTE}],
                "limits": {"skills": {
                    "reminders": {"snoozed_until_us": now + 60 * MINUTE},
                    "sip-of-water": {"rest": {"at_us": now - 50 * MINUTE,
                                              "resting_until_us": now - MINUTE}},
                    "morning-mail": {"rest": {"at_us": now - 3 * 60 * MINUTE,
                                              "resting_until_us": now + 10 * MINUTE}}
                }}
            })
            .to_string(),
        )
        .unwrap();
        let skills = skills(&places, now, true).unwrap();
        assert_eq!(skills.len(), 6);
        let sip = by_name(&skills, "sip-of-water");
        assert_eq!(sip.state, State::On);
        assert_eq!(sip.ran_us, Some(now - 42 * MINUTE));
        assert_eq!(sip.description.as_deref(), Some(SIP));
        assert!(!sip.task);
        assert_eq!(by_name(&skills, "comfort-break").state, State::On);
        assert_eq!(by_name(&skills, "stretch-break").state, State::TurnedOff);
        assert_eq!(by_name(&skills, "reminders").state, State::Snoozed);
        let mail = by_name(&skills, "morning-mail");
        assert_eq!(mail.state, State::Resting(10));
        assert!(mail.task);
        assert_eq!(by_name(&skills, "dance").state, State::InstalledOff);
        // Without `LAMP_NUDGES_PRESENT`, Comfort break's own file is off.
        let without = super::skills(&places, now, false).unwrap();
        assert_eq!(
            by_name(&without, "comfort-break").state,
            State::InstalledOff
        );
        // No records at all: every skill as its file says, and with no
        // holds file (lamp-skills not yet at its first turn, or a write of
        // it failed) a snooze or rest can't be told: never said "on" alone.
        std::fs::remove_file(places.state.join(OFF_FILE)).unwrap();
        std::fs::remove_file(places.tools.join(HOLDS_FILE)).unwrap();
        let bare = super::skills(&places, now, true).unwrap();
        assert_eq!(by_name(&bare, "stretch-break").state, State::OnRestUnknown);
        assert_eq!(by_name(&bare, "dance").state, State::InstalledOff);
        assert_eq!(by_name(&bare, "sip-of-water").ran_us, None);
        assert_eq!(
            State::OnRestUnknown.short(),
            "on, its snooze can't be checked right now"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// "is my water reminder on or off?": the sip of water is told in full
    /// with its state, what it does and when it last ran, beside every
    /// skill's line; one in `off` is told off.
    #[test]
    fn a_question_about_a_skill_is_told_from_the_records() {
        let (root, places) = product_lamp("told");
        std::fs::write(places.state.join(OFF_FILE), "stretch-break\n").unwrap();
        let now = 1_000 * MINUTE;
        std::fs::write(
            places.tools.join(HOLDS_FILE),
            json!({"holds": [{"skill": "sip-of-water", "ran_us": now - 42 * MINUTE}]}).to_string(),
        )
        .unwrap();
        let told = told(
            "water reminder",
            &places,
            now,
            true,
            1,
            true,
            || panic!("no skill made here: the owner is not asked"),
            |_| panic!("no task among the nearest"),
        );
        let nearest = told["nearest"].as_array().unwrap();
        assert!(nearest.len() <= NEAREST_MOST, "{told}");
        let sip = nearest
            .iter()
            .find(|facts| facts["name"] == "sip of water")
            .unwrap_or_else(|| panic!("{told}"));
        assert_eq!(sip["state"], "on");
        assert_eq!(sip["does"], SIP);
        assert_eq!(sip["last_ran_by_itself"], "42 minutes ago");
        let listed: Vec<&str> = told["skills"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| line.as_str().unwrap())
            .collect();
        assert!(listed.contains(&"sip of water: on"), "{listed:?}");
        assert!(listed.contains(&"stretch break: off"), "{listed:?}");
        assert!(listed.contains(&"dance: off"), "{listed:?}");
        assert_eq!(told["next"], NEXT);
        let stretch = super::told(
            "stretch break",
            &places,
            now,
            true,
            2,
            true,
            || true,
            |_| None,
        );
        assert_eq!(stretch["nearest"][0]["name"], "stretch break", "{stretch}");
        assert_eq!(
            stretch["nearest"][0]["state"],
            "off: turned off by voice or in the app, until it is turned on"
        );
        assert!(stretch["nearest"][0].get("last_ran_by_itself").is_none());
        // Bounded: a few KB with every skill.
        assert!(told.to_string().len() < 4_096, "{}", told.to_string().len());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A skill whose task waits for the owner's yes is told so, asked of
    /// lamp-control once; a turn nobody spoke is told nothing.
    #[test]
    fn a_tasks_yes_is_told_and_an_unheard_turn_gets_nothing() {
        let (root, places) = product_lamp("yes");
        let mut asked = Vec::new();
        let told = told(
            "morning mail",
            &places,
            MINUTE,
            false,
            1,
            true,
            || true,
            |name| {
                asked.push(name.to_owned());
                Some(Ok(true))
            },
        );
        assert_eq!(asked, ["morning-mail"]);
        assert_eq!(told["nearest"][0]["name"], "morning mail", "{told}");
        assert!(
            told["nearest"][0]["waits_for_yes"]
                .as_str()
                .unwrap()
                .starts_with("yes")
        );
        let unheard = super::told(
            "morning mail",
            &places,
            MINUTE,
            false,
            2,
            false,
            || panic!("owner asked"),
            |_| panic!("lamp-control asked"),
        );
        assert_eq!(unheard, json!({"error": NOT_ASKED_ALOUD}));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A skill made here is named only to the owner, as a list's names
    /// are; to anyone else it is left out and counted.
    #[test]
    fn a_skill_made_here_is_named_to_the_owner_alone() {
        let (root, places) = product_lamp("made");
        let folder = places.pushed.join("evening-dim");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("SKILL.md"),
            "---\nname: evening-dim\ndescription: At 22:00 dims the light.\nmade: here\n---\n",
        )
        .unwrap();
        let owner = told(
            "evening dim",
            &places,
            MINUTE,
            false,
            1,
            true,
            || true,
            |_| None,
        );
        assert_eq!(owner["nearest"][0]["name"], "evening dim", "{owner}");
        assert!(owner.get("made_here_not_named").is_none());
        let guest = told(
            "evening dim",
            &places,
            MINUTE,
            false,
            2,
            true,
            || false,
            |_| None,
        );
        assert_eq!(guest["nearest"], json!([]), "{guest}");
        assert!(!guest["skills"].to_string().contains("evening"), "{guest}");
        assert_eq!(guest["made_here_not_named"], 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn skill_tool(name: &str) -> lamp_gemini::Tool {
        lamp_gemini::Tool {
            name: name.to_owned(),
            description: format!("When someone asks for {name}."),
            parameters: None,
            silent: false,
        }
    }

    /// `use_skill` takes `about` with the switch alone; off, it is as
    /// before, byte for byte, and on, the rest is as before but `skill` is
    /// no longer required, within the link's bounds, and as many skills are
    /// offered as before.
    #[test]
    fn use_skill_takes_about_with_the_switch_alone() {
        let skills: Vec<lamp_gemini::Tool> = (0..40)
            .map(|n| skill_tool(&format!("a_product_skill_{n}")))
            .collect();
        set_on_here(Some(false));
        let before = crate::use_skill::function(&skills).unwrap();
        let offered_before = crate::use_skill::offered(&skills).len();
        set_on_here(None);
        let unset = crate::use_skill::function(&skills).unwrap();
        assert_eq!(
            serde_json::to_string(&before.parameters).unwrap(),
            serde_json::to_string(&unset.parameters).unwrap()
        );
        assert_eq!(before.description, unset.description);
        let parameters = before.parameters.clone().unwrap();
        assert!(parameters["properties"].get(ABOUT).is_none());
        assert_eq!(parameters["required"], json!(["skill"]));
        set_on_here(Some(true));
        let tool = crate::use_skill::function(&skills).unwrap();
        let offered_on = crate::use_skill::offered(&skills).len();
        set_on_here(None);
        assert_eq!(tool.name, crate::use_skill::NAME);
        assert_eq!(offered_on, offered_before);
        assert!(
            tool.description.len() <= 1_024,
            "{}",
            tool.description.len()
        );
        assert_eq!(
            tool.description,
            format!("{}{USE_SKILL_TOLD}", before.description)
        );
        let on = tool.parameters.unwrap();
        assert_eq!(on["properties"][ABOUT]["description"], ABOUT_TOLD);
        assert_eq!(on["required"], json!([]));
        assert!(on.to_string().len() <= lamp_gemini::MAX_TOOL_JSON_BYTES);
        let mut without = on.clone();
        without["properties"].as_object_mut().unwrap().remove(ABOUT);
        without["required"] = json!(["skill"]);
        assert_eq!(without, parameters);
    }

    /// A `use_skill` call with `about` is asked of the records only with
    /// the switch; off, it is a call like any other (`translate` refuses
    /// it, naming no skill run).
    #[test]
    fn a_use_skill_call_with_about_is_asked_only_with_the_switch() {
        let call = lamp_gemini::ToolCall {
            id: "c1".into(),
            name: crate::use_skill::NAME.into(),
            args: json!({"about": "water reminder"}),
        };
        set_on_here(Some(false));
        assert_eq!(asked(&call), None);
        set_on_here(Some(true));
        assert_eq!(asked(&call), Some("water reminder"));
        set_on_here(None);
        assert!(crate::use_skill::translate(&call, &[skill_tool("dance")]).is_err());
    }

    /// Named by what it does, not by its name: "the blue light that tells
    /// me to drink" comes nearest to the sip of water; something no skill
    /// on the lamp does comes near none, and the model is told every
    /// skill's line, to say it has none like that.
    #[test]
    fn named_by_what_it_does_or_not_on_the_lamp() {
        let (root, places) = product_lamp("does");
        let told = told(
            "the cool blue light that tells me to drink",
            &places,
            MINUTE,
            true,
            1,
            true,
            || true,
            |_| None,
        );
        assert_eq!(told["nearest"][0]["name"], "sip of water", "{told}");
        let none = super::told(
            "order a pizza",
            &places,
            MINUTE,
            true,
            2,
            true,
            || true,
            |_| None,
        );
        assert_eq!(none["nearest"], json!([]), "{none}");
        assert_eq!(none["skills"].as_array().unwrap().len(), 6, "{none}");
        assert!(NEXT.contains("say no skill on the lamp is that one"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Never a guess: with the skills folder or a record there unreadable,
    /// the model is told the lamp can't check right now; a skill whose
    /// `skill.toml` isn't TOML is told off, as lamp-skills loads none.
    #[test]
    fn records_that_cant_be_read_are_never_guessed() {
        let (root, places) = product_lamp("unread");
        std::fs::write(places.tools.join(HOLDS_FILE), "not json").unwrap();
        let ask = |places: &Places| {
            told(
                "sip of water",
                places,
                MINUTE,
                true,
                1,
                true,
                || true,
                |_| None,
            )
        };
        assert_eq!(ask(&places), json!({"error": CANT_CHECK}));
        std::fs::remove_file(places.tools.join(HOLDS_FILE)).unwrap();
        // The off file a folder, not a file: there, and unreadable.
        std::fs::create_dir_all(places.state.join(OFF_FILE)).unwrap();
        assert_eq!(ask(&places), json!({"error": CANT_CHECK}));
        std::fs::remove_dir_all(places.state.join(OFF_FILE)).unwrap();
        let gone = Places {
            installed: root.join("no-such-folder"),
            ..places.clone()
        };
        assert_eq!(ask(&gone), json!({"error": CANT_CHECK}));
        std::fs::write(places.installed.join("sip-of-water/skill.toml"), "[[[").unwrap();
        let told = ask(&places);
        assert_eq!(
            told["nearest"][0]["state"], "off: its files on the lamp don't load",
            "{told}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #7335's row on lamp-9adf: "is my water reminder on or off?" called
    /// `list_reminders`, and with none set the lamp said nothing was on.
    /// Its answer carries the skills' lines and the note, the schedule's
    /// own words kept; a turn nobody spoke gets nothing added; a record
    /// that can't be read gets the can't-check note alone; a made skill's
    /// name only to the owner.
    #[test]
    fn a_reminders_answer_carries_the_skills_lines() {
        let (root, places) = product_lamp("reminders");
        let folder = places.installed.join("drink-water-reminder");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("SKILL.md"),
            "---\nname: drink-water-reminder\ndescription: Every hour, a reminder to drink water.\nmade: here\n---\n",
        )
        .unwrap();
        std::fs::write(folder.join("skill.toml"), "kind = \"installable\"\n").unwrap();
        let schedule = json!({"reminders": []});
        let mut told = schedule.clone();
        with_reminders(&mut told, &places, MINUTE, true, 1, true, || true);
        assert_eq!(told["reminders"], json!([]), "{told}");
        let lines = told[SKILLS_ON_THE_LAMP].as_array().unwrap();
        assert!(lines.contains(&json!("sip of water: on")), "{told}");
        assert!(lines.contains(&json!("drink water reminder: on")), "{told}");
        assert!(lines.contains(&json!("dance: off")), "{told}");
        assert_eq!(told[SKILLS_NOTE], REMINDERS_NOTE);
        // Not the owner: a skill made here is left out and counted.
        let mut guest = schedule.clone();
        with_reminders(&mut guest, &places, MINUTE, true, 2, true, || false);
        assert!(
            !guest.to_string().contains("drink water reminder"),
            "{guest}"
        );
        assert_eq!(guest["made_here_not_named"], 1);
        // A turn nobody spoke: the schedule's answer as it was.
        let mut unheard = schedule.clone();
        with_reminders(&mut unheard, &places, MINUTE, true, 3, false, || {
            panic!("owner asked")
        });
        assert_eq!(unheard, schedule);
        // An answer that is not an object is kept whole.
        let mut text = json!("You have no reminders.");
        with_reminders(&mut text, &places, MINUTE, true, 4, true, || true);
        assert_eq!(text["result"], "You have no reminders.");
        assert!(text[SKILLS_ON_THE_LAMP].is_array());
        // A record that can't be read: never a guess.
        std::fs::write(places.tools.join(HOLDS_FILE), "not json").unwrap();
        let mut unread = schedule.clone();
        with_reminders(&mut unread, &places, MINUTE, true, 5, true, || true);
        assert_eq!(
            unread,
            json!({"reminders": [], SKILLS_NOTE: REMINDERS_CANT_CHECK})
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Off, or any call but `list_reminders`, the answer is untouched,
    /// byte for byte.
    #[test]
    fn off_a_reminders_answer_is_as_before() {
        let schedule = json!({"ok": true, "result": {"text": "No reminders."}});
        set_on_here(Some(false));
        let mut told = schedule.clone();
        with_reminders_here(LIST_REMINDERS, &mut told, true, 1, true, None);
        assert_eq!(told.to_string(), schedule.to_string());
        set_on_here(Some(true));
        let mut other = schedule.clone();
        with_reminders_here("set_reminder", &mut other, true, 2, true, None);
        set_on_here(None);
        assert_eq!(other.to_string(), schedule.to_string());
    }

    /// #7335's row on lamp-9adf: "do I have a skill that reads my calendar
    /// when I sit down?" called `make_skill` for the list of those made,
    /// and a skill added from a file was said not to be there. A list's
    /// answer, and a run's, a remove's or a read-back's that found none,
    /// carry the skills' lines; a make's, a yes awaited, a turn nobody
    /// spoke and an unreadable record as for reminders.
    #[test]
    fn a_made_skills_answer_carries_the_skills_lines() {
        let (root, places) = product_lamp("made-answer");
        let folder = places.installed.join("read-calendar-when-sitting");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("SKILL.md"),
            "---\nname: read-calendar-when-sitting\ndescription: When you sit down, reads your next event.\nmade: here\nadded: file\n---\n",
        )
        .unwrap();
        std::fs::write(folder.join("skill.toml"), "kind = \"installable\"\n").unwrap();
        let list = json!({"made_count": 0, "next": "Say plainly that they haven't made any skill yet. Call nothing."});
        let made = json!({"made": true});
        let mut told = list.clone();
        with_made(&made, &mut told, &places, MINUTE, true, 1, true, || true);
        assert_eq!(told["made_count"], 0, "{told}");
        assert_eq!(told["next"], list["next"]);
        let lines = told[SKILLS_ON_THE_LAMP].as_array().unwrap();
        assert!(
            lines.contains(&json!("read calendar when sitting: on")),
            "{told}"
        );
        assert!(lines.contains(&json!("sip of water: on")), "{told}");
        assert_eq!(told[SKILLS_NOTE], MADE_NOTE);
        // A run that found none: carried too.
        let none = json!({"error": crate::made_remove::NO_MADE, "next": "Say plainly ..."});
        let mut run = none.clone();
        with_made(
            &json!({"run": "calendar"}),
            &mut run,
            &places,
            MINUTE,
            true,
            2,
            true,
            || true,
        );
        assert!(run[SKILLS_ON_THE_LAMP].is_array(), "{run}");
        // A remove waiting for its yes, a make, and a clash's choice: untouched.
        for (args, answer) in [
            (
                json!({"remove": "moon fact"}),
                json!({"remove": "moon-fact", crate::skill_write_yes::APPROVE_ID: "x", "next": "Ask."}),
            ),
            (
                json!({"request": "dim the light at ten"}),
                json!({"made": "evening-dim", "on": true}),
            ),
            (json!({"request": ""}), json!({"error": "nothing was made"})),
            (
                json!({"clash_id": "c", "choice": "keep"}),
                json!({"error": "no clash"}),
            ),
        ] {
            let mut kept = answer.clone();
            with_made(&args, &mut kept, &places, MINUTE, true, 3, true, || {
                panic!("owner asked")
            });
            assert_eq!(kept.to_string(), answer.to_string(), "{args}");
        }
        // Not the owner: the skill made here is left out and counted.
        let mut guest = list.clone();
        with_made(&made, &mut guest, &places, MINUTE, true, 4, true, || false);
        assert!(!guest.to_string().contains("calendar"), "{guest}");
        assert_eq!(guest["made_here_not_named"], 1);
        // A turn nobody spoke: as it was.
        let mut unheard = list.clone();
        with_made(&made, &mut unheard, &places, MINUTE, true, 5, false, || {
            panic!("owner asked")
        });
        assert_eq!(unheard, list);
        // A record that can't be read: never a guess.
        std::fs::write(places.tools.join(HOLDS_FILE), "not json").unwrap();
        let mut unread = list.clone();
        with_made(&made, &mut unread, &places, MINUTE, true, 6, true, || true);
        let mut expected = list.clone();
        expected[SKILLS_NOTE] = json!(MADE_CANT_CHECK);
        assert_eq!(unread, expected);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Off, a `make_skill` answer is untouched, byte for byte.
    #[test]
    fn off_a_made_skills_answer_is_as_before() {
        let call = lamp_gemini::ToolCall {
            id: "c1".into(),
            name: crate::made_skill::MAKE.into(),
            args: json!({"made": true}),
        };
        let list = json!({"made_count": 2, "next": "Say how many."});
        set_on_here(Some(false));
        let mut told = list.clone();
        with_made_here(&call, &mut told, 1, true, None, None);
        set_on_here(None);
        assert_eq!(told.to_string(), list.to_string());
    }

    /// An answer about a skill only answers: it offers no change.
    #[test]
    fn the_answers_offer_no_change() {
        for note in [NEXT, REMINDERS_NOTE, MADE_NOTE] {
            assert!(note.contains("Only answer: offer to change, turn on or turn off nothing."));
        }
    }

    /// #7360's P2: a reminders page near the link's bound and 80 skills of
    /// long names stays sendable: lines left out from the end and counted,
    /// the schedule's own words whole; a page with no room for the note
    /// gets nothing added; the about answer fits too.
    #[test]
    fn answers_with_the_skills_stay_within_the_link() {
        let long: Vec<(String, String, String)> = (0..LISTED_MOST)
            .map(|n| {
                (
                    format!("a-skill-with-a-rather-long-name-number-{n:02}"),
                    "Does a thing.".to_owned(),
                    "kind = \"installable\"\n".to_owned(),
                )
            })
            .collect();
        let skills: Vec<(&str, &str, &str)> = long
            .iter()
            .map(|(a, b, c)| (a.as_str(), b.as_str(), c.as_str()))
            .collect();
        let (root, places) = lamp("bound", &skills);
        let page = json!({"reminders": [{"text": "x".repeat(2_400), "at": "09:00"}]});
        let mut told = page.clone();
        with_reminders(&mut told, &places, MINUTE, true, 1, true, || true);
        assert!(crate::tools::sendable(&told), "{}", told.to_string().len());
        assert_eq!(told["reminders"], page["reminders"]);
        assert_eq!(told[SKILLS_NOTE], REMINDERS_NOTE);
        let kept = told[SKILLS_ON_THE_LAMP].as_array().unwrap().len();
        assert!(kept > 0 && kept < LISTED_MOST, "{kept}");
        assert_eq!(told[LEFT_OUT], LISTED_MOST - kept);
        // No room even for the note: the page as it was.
        let full = json!({"reminders": [{"text": "x".repeat(3_900)}]});
        let mut told = full.clone();
        with_reminders(&mut told, &places, MINUTE, true, 2, true, || true);
        assert_eq!(told.to_string(), full.to_string());
        // The same for a made skills answer, and the about answer fits.
        let mut made = json!({"made_count": 0, "next": "x".repeat(2_400)});
        with_made(
            &json!({"made": true}),
            &mut made,
            &places,
            MINUTE,
            true,
            3,
            true,
            || true,
        );
        assert!(crate::tools::sendable(&made));
        let about = told_about(&places, "number 07");
        assert!(
            crate::tools::sendable(&about),
            "{}",
            about.to_string().len()
        );
        assert!(about[LEFT_OUT].as_u64().is_some_and(|n| n > 0), "{about}");
        let _ = std::fs::remove_dir_all(&root);
    }

    fn told_about(places: &Places, asked: &str) -> Value {
        told(asked, places, MINUTE, true, 1, true, || true, |_| None)
    }

    /// #7360's P3: a schedule answer that failed (not answered, or an
    /// error) is kept as it is.
    #[test]
    fn a_failed_reminders_answer_is_kept_as_it_is() {
        set_on_here(Some(true));
        let uncertain = json!({"error": "I could not confirm what happened to that reminder."});
        let mut told = uncertain.clone();
        with_reminders_here(LIST_REMINDERS, &mut told, false, 1, true, None);
        assert_eq!(told, uncertain);
        let mut told = crate::tools::unavailable();
        with_reminders_here(LIST_REMINDERS, &mut told, true, 2, true, None);
        assert_eq!(told, crate::tools::unavailable());
        set_on_here(None);
        // An unavailable make_skill is no answer about their skills.
        assert!(!tells_their_skills(
            &json!({"made": true}),
            &crate::tools::unavailable()
        ));
    }

    /// #7345's P2 and P3: whether a task waits for the owner's yes is never
    /// guessed: lamp-control or the manage helper not there, or not
    /// answering within the bound, is "can't check right now", and the ask
    /// ends within [`YES_CHECK_MOST`].
    #[test]
    fn a_tasks_yes_that_cant_be_checked_is_never_said_to_wait() {
        let root = std::env::temp_dir().join(format!("skill-about-yes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        // No helper at all.
        let none = task_waits(
            &root.join("none.sock"),
            &root.join("none2.sock"),
            "morning-mail",
            YES_CHECK_MOST,
        );
        assert!(matches!(none, Some(Err(_))), "{none:?}");
        // A helper that takes the line and never answers.
        let socket = root.join("m.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let held = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            std::thread::sleep(std::time::Duration::from_secs(3));
            drop(stream);
        });
        let started = std::time::Instant::now();
        let silent = task_waits(
            &socket,
            &root.join("none2.sock"),
            "morning-mail",
            YES_CHECK_MOST,
        );
        assert!(matches!(silent, Some(Err(_))), "{silent:?}");
        assert!(
            started.elapsed() < YES_CHECK_MOST + std::time::Duration::from_millis(500),
            "{:?}",
            started.elapsed()
        );
        held.join().unwrap();
        // Told: no wait guessed, said to be unchecked.
        let (lamp_root, places) = product_lamp("yes-unchecked");
        let told = told(
            "morning mail",
            &places,
            MINUTE,
            false,
            1,
            true,
            || true,
            |_| Some(Err("lamp-control couldn't be reached".into())),
        );
        assert!(told["nearest"][0].get("waits_for_yes").is_none(), "{told}");
        assert_eq!(told["nearest"][0][YES_UNCHECKED], YES_UNCHECKED_TOLD);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&lamp_root);
    }

    /// #7360's P3: the schedule's route has no settings, so the voice's
    /// skills folder noted from its flags gives the product's places their
    /// tools folder.
    #[test]
    fn the_product_places_use_the_voices_skills_folder() {
        assert_eq!(
            Places::product(Some(Path::new("/given"))).tools,
            PathBuf::from("/given")
        );
        note_tools_dir(Path::new("/run/lampos-skills-noted"));
        let noted = NOTED_TOOLS_DIR.get().unwrap().clone();
        assert_eq!(Places::product(None).tools, noted);
    }

    /// A long description is cut at a word's end within its bound.
    #[test]
    fn a_long_description_is_cut_at_a_word() {
        let long = "word ".repeat(200);
        let said = cut(&long);
        assert!(said.len() <= DOES_MOST + "…".len(), "{}", said.len());
        assert!(said.ends_with("word…"), "{said}");
        assert_eq!(cut("short"), "short");
    }
}
