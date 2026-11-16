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
//!
//! Off unless set, and then `use_skill` is exactly as before.
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
            tools: tools.map_or_else(|| PathBuf::from(TOOLS_DIR), Path::to_path_buf),
        }
    }
}

/// A skill's state, as the lamp's records say it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    On,
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
            Self::TurnedOff | Self::InstalledOff | Self::DoesNotLoad => "off",
            Self::Snoozed => "snoozed until tomorrow morning",
            Self::Resting(_) => "resting",
        }
    }

    /// In full, for a skill told whole.
    fn said(&self) -> String {
        match self {
            Self::On => "on".into(),
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

/// `lamp-skills`' holds file; empty when there is none (nothing held,
/// snoozed or resting since the boot), why when it can't be read.
fn holds(tools: &Path) -> Result<Holds, String> {
    match read_record(&tools.join(HOLDS_FILE))? {
        None => Ok(Holds::default()),
        Some(text) => serde_json::from_str(&text)
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
            } else {
                State::On
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
pub const NEXT: &str = "These are the lamp's own records: every skill on it with its state, and those whose words come nearest to what they asked in full. Answer what they asked from these alone, in a few words: on, off, snoozed or resting, what it does and what starts it, when it last ran, whether it waits for their yes. Never guess: what is not given here the lamp doesn't keep, so say so. If none is the skill they mean, say no skill on the lamp is that one (a reminder they set themselves is not a skill). Don't call use_skill again for it.";

/// The answer to a question `asked` about a skill on the lamp at `places`
/// at `now_us` (the monotonic clock) with `present`, in the voice's request
/// `request` (`heard`: someone spoke it); names of skills made here only
/// when `owner_hears`, asked only when there is one; `waits(name)` says
/// whether a skill's task waits for the owner's yes, asked only of the
/// nearest skill with a task.
#[allow(clippy::too_many_arguments)]
pub fn told(
    asked: &str,
    places: &Places,
    now_us: u64,
    present: bool,
    request: u64,
    heard: bool,
    owner_hears: impl FnOnce() -> bool,
    waits: impl FnOnce(&str) -> Option<bool>,
) -> Value {
    if !heard {
        eprintln!("lamp-voice: request {request}: a skill's state not told: no one asked aloud");
        return json!({ "error": NOT_ASKED_ALOUD });
    }
    let mut skills = match skills(places, now_us, present) {
        Ok(skills) => skills,
        Err(why) => {
            eprintln!("lamp-voice: request {request}: skills' state not told: {why}");
            return json!({ "error": CANT_CHECK });
        }
    };
    let made = skills.iter().filter(|skill| skill.made_here).count();
    let mut not_named = 0;
    if made > 0 && !owner_hears() {
        skills.retain(|skill| !skill.made_here);
        not_named = made;
    }
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
                facts["waits_for_yes"] = json!(if waiting {
                    "yes: the task it hands to the lamp's agent waits for the owner's yes"
                } else {
                    "no: the owner said yes to its task"
                });
            }
            facts
        })
        .collect();
    let listed: Vec<String> = skills
        .iter()
        .map(|skill| {
            format!(
                "{}: {}",
                made_skill::spoken_name(&skill.name),
                skill.state.short()
            )
        })
        .collect();
    let mut told = json!({
        "asked": asked,
        "nearest": full,
        "skills": listed,
        "next": NEXT,
    });
    if not_named > 0 {
        told["made_here_not_named"] = json!(not_named);
    }
    told
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
            crate::skill_task_yes::asks_by(
                Path::new(made_skill::MANAGE_SOCKET),
                Path::new(crate::skill_task_yes::CONTROL_SOCKET),
                name,
            )
            .ok()
            .flatten()
            .map(|asks| !asks.approved)
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
        // No records at all: every skill as its file says.
        std::fs::remove_file(places.state.join(OFF_FILE)).unwrap();
        std::fs::remove_file(places.tools.join(HOLDS_FILE)).unwrap();
        let bare = super::skills(&places, now, true).unwrap();
        assert_eq!(by_name(&bare, "stretch-break").state, State::On);
        assert_eq!(by_name(&bare, "sip-of-water").ran_us, None);
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
                Some(true)
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
