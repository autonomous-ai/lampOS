//! Old skills remade in the middle form (the first builder's yes on #6811,
//! 18 Nov 16:40, his 19:50 ruling, and his 19 Nov 01:22 and 01:29 rulings
//! on #7820): with `LAMP_REMAKE_OLD_SKILLS=on` ([`ENVIRONMENT`], on only
//! when exactly `on`, in no unit), a made skill DeepSeek made before the
//! route (its `made_with` missing or `deepseek`) that already needs a model
//! or the lamp's agent each time it runs ([`needs_a_model_each_run`]) is
//! made again as one task for the lamp's agent: its own start and timing
//! exactly as they were, the person's own kept request word for word as the
//! task, and sip-of-water's guards on any trigger but a clock
//! (`make_skill_local::remade_as_task`, `make_no_cannot::skill_text`). The
//! lamp writes it; the agent is never asked to write it and lamp-tasks'
//! make slot is never taken.
//!
//! - **What it touches** ([`chosen`]): a kept made skill on the lamp, made
//!   here by voice or in the app (`from` is `local:` or `written:`), never
//!   one added from a file (`upload:`, `added`), a Store skill or a core
//!   skill (a folder of that name not marked `made: here`), and never one
//!   changed since it was made: its folder must hold the very package made
//!   (`made_skill::went_on_as`) and, with no code of its own, the very
//!   `skill.toml` (`toml_sha256`). Never a quick reaction
//!   (`made_skill::quick_reaction`), and never a skill whose steps all run
//!   on the lamp (its light, a fixed say's exact words, its body, its
//!   sounds): only one with a step that says the model's own words (`say`
//!   as a note, a string or `from`), looks (`look`), reads a linked
//!   service (`read`) or hands a task to the agent (`task`).
//! - **Left as it is, and journaled why** ([`chosen_names`]): one with no
//!   request of the person's kept (`request` in `made.json`, [`kept_request`];
//!   never the said-back `description`), or one whose request its task
//!   can't hold whole; and one whose start and timing can't be kept
//!   ([`kept_timing`]): no `[trigger]` (only asked for, whose phrases would
//!   be picked again), or lines that can't be taken out as the very same
//!   parts. A remake whose kept skill would start or be timed otherwise
//!   keeps nothing (`made_skill::REMAKE_TIMING_CHANGED`, [`same_timing`]).
//! - **Its start never picked again**: the old `most_a_day`, `[trigger]`
//!   (its `cooldown` in it), `[active]` and `[quiet]`, its own lines, are
//!   the remake's (`made_skill::RemakeAs`); of the lamp model's one call
//!   only the skill's grants are taken.
//! - **Remade without asking** (the first builder, 18 Nov 18:23, in place of
//!   16:40's one ask): with the switch on, each one that qualifies is queued
//!   on its own ([`step`]). The person's own "remake my old skills" comes
//!   back as `make_skill`'s [`REMAKE_OLD`] [`ASKED`] ([`asked`]): it queues
//!   them again, one put back or out of tries too.
//! - **Nothing asked** (18 Nov 21:58, 19 Nov 01:29): with
//!   `LAMP_MAKE_NO_ASKS` on, it goes on with its task's yes recorded for its
//!   grants (`make_skill_local::code_on_at_once`), as a new make's is, so it
//!   runs at its first start with no question; off, its task asks when it
//!   first runs, as before. Its only code is the lamp's own agent script
//!   (`made_skill::Uses::agent_task`); any other code is never put on here.
//! - **Remade one at a time** ([`step`]), only while idle (no line heard for
//!   [`IDLE_FOR`]), never in the made skills' quiet hours
//!   (`made_skill::MANNERS_QUIET`), at most [`MOST_A_DAY`] a day, and only
//!   on a lamp with an agent to do its task (none: nothing is tried, no try
//!   counted, the old ones stay). The new one replaces the old under the
//!   same name, its on/off state kept, only once it was kept (the same
//!   checks a new make passes) and put on. It records `made_with` (the
//!   agent that does its task) and `remade_from` (the old package's
//!   SHA-256). A failed one leaves the old exactly as it was, is journaled
//!   with a short reason and no word of the skill, and is tried at most once
//!   more, on another day.
//! - **Undo for a week** ([`put_back`]): the old entry is kept under the new
//!   one's `old` key with `replaced_at_ms`, its package with it;
//!   `make_skill`'s [`PUT_BACK`] with a name ("put my old water skill
//!   back") puts it back on. After [`UNDO_FOR`] the old package is deleted
//!   ([`drop_old`]). A remade skill removed takes its old package with it
//!   (`made_skill::packages_named`), and the owner's "forget everything"
//!   clears this undo with `make_no_asks`' keep ([`clear`]). Every write of
//!   the index here holds its lock (`made_skill::index_locked`).
//! - **Told once after** ([`told_line`], [`note`]): on the owner's next heard
//!   line with nothing else waiting, in the lamp's or the conversation's
//!   language: "I updated 3 of your skills so your agent does them; each
//!   still starts when it did. Say 'put my old water skill back' to undo."
use crate::made_skill::{self, Keeping, Made};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::Path,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Duration, SystemTime},
};

/// The switch: `on`, and nothing else.
pub const ENVIRONMENT: &str = "LAMP_REMAKE_OLD_SKILLS";
/// `make_skill`'s argument with the person's own ask to remake their old
/// skills: [`ASKED`] alone.
pub const REMAKE_OLD: &str = "remake_old";
/// What [`REMAKE_OLD`] may say.
pub const ANSWERS: [&str; 1] = [ASKED];
/// The person's own ask to remake their old skills ("remake my old
/// skills").
pub const ASKED: &str = "asked";
/// `make_skill`'s argument that puts a remade skill's old one back: its
/// name as heard.
pub const PUT_BACK: &str = "put_back";
/// The pass's own record, in the made skills' folder.
pub const STATE_FILE: &str = "remake.json";
/// The most remakes begun in a day.
pub const MOST_A_DAY: u32 = 5;
/// How long no line is heard before a remake may begin.
pub const IDLE_FOR: Duration = Duration::from_secs(10 * 60);
/// How long a remade skill's old one is kept to be put back.
pub const UNDO_FOR: Duration = Duration::from_secs(7 * 86_400);
/// The most times one skill's remake is tried, each on its own day.
pub const MOST_TRIES: u32 = 2;
/// How often the pass looks.
const LOOK_EVERY: Duration = Duration::from_secs(60);
/// How long the manage helper is waited on to put one on.
const HELPER_WAIT: Duration = Duration::from_secs(8);

/// What the model is told of an ask nobody spoke.
pub const NOT_ASKED_ALOUD: &str =
    "old skills are remade on an ask only when the person asks out loud";
/// What the model is told of an answer that isn't the owner's.
pub const NOT_OWNER: &str = "only the lamp's owner can have their old skills remade or put back";

/// Whether the switch is on in this process's environment.
pub fn on() -> bool {
    on_with(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// Whether the switch's value `value` turns it on: exactly `on`.
pub fn on_with(value: Option<&str>) -> bool {
    value == Some("on")
}

/// Who made a kept skill: its index entry's `made_with`, else its card's
/// `made_with:` line; missing is DeepSeek's (made before the route).
fn made_with(entry: &Value, card: &str) -> String {
    entry["made_with"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| {
            card.lines()
                .find_map(|line| line.trim().strip_prefix("made_with:"))
                .map(|said| said.trim().to_owned())
        })
        .unwrap_or_else(|| "deepseek".to_owned())
}

fn hex_of(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Whether a made skill's `skill.toml` (`table`) already needs a model or
/// the lamp's agent each time it runs (the first builder, 19 Nov 01:29 on
/// #7820): a step that says the model's own words (`say` as a note, a
/// string, or `from`), looks with the camera (`look`), reads a linked
/// service (`read`) or hands a task to the agent (`task`). A skill whose
/// steps all run on the lamp (its light, a fixed say's exact words, its
/// body, its sounds) is not: it is never remade.
pub fn needs_a_model_each_run(table: &toml::Table) -> bool {
    table
        .get("do")
        .and_then(toml::Value::as_array)
        .is_some_and(|steps| {
            steps.iter().filter_map(toml::Value::as_table).any(|step| {
                ["look", "read", "task"]
                    .iter()
                    .any(|key| step.contains_key(*key))
                    || step.get("say").is_some_and(|say| match say {
                        toml::Value::Table(say) => !say.contains_key("text"),
                        _ => true,
                    })
            })
        })
}

/// The person's own request a kept made skill was made from (`request` in
/// `made.json`), whole: never the lamp's said-back `description` (the
/// first builder, 19 Nov 01:22 on #7820). None when none is kept, or when
/// its task couldn't hold it whole (`make_no_cannot::task_cut`).
pub fn kept_request(entry: &Value) -> Option<&str> {
    entry["request"].as_str().map(str::trim).filter(|request| {
        !request.is_empty()
            && !crate::make_no_cannot::task_words(request).is_empty()
            && !crate::make_no_cannot::task_cut(request)
    })
}

/// What a skill's start and timing are: `most_a_day`, `[trigger]` (its
/// `cooldown` in it), `[active]` and `[quiet]`.
const TIMING: [&str; 4] = ["most_a_day", "trigger", "active", "quiet"];

/// The parts of `table` that are its start and timing ([`TIMING`]).
fn timing_of(table: &toml::Table) -> toml::Table {
    TIMING
        .iter()
        .filter_map(|key| Some(((*key).to_owned(), table.get(*key)?.clone())))
        .collect()
}

/// An old skill's start and timing ([`TIMING`]) as its own `skill.toml`
/// writes them, its lines as they are, for its remake to start exactly as
/// it did (`made_skill::RemakeAs`). None when it has no `[trigger]` (one
/// only asked for, whose phrases the middle form would pick again), or when
/// its lines can't be taken out as the very same parts: then it is left as
/// it is.
pub fn kept_timing(toml: &str) -> Option<String> {
    let table: toml::Table = toml::from_str(toml).ok()?;
    table.get("trigger")?.as_table()?;
    if table.contains_key("tool") || table.contains_key("said") {
        return None;
    }
    let (mut top, mut parts) = (Vec::new(), Vec::new());
    let mut part: Option<String> = None;
    for line in toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            part = Some(trimmed.trim_matches(['[', ']']).trim().to_owned());
        }
        match part.as_deref() {
            None => {
                if trimmed
                    .split_once('=')
                    .is_some_and(|(key, _)| key.trim() == "most_a_day")
                {
                    top.push(line);
                }
            }
            Some(head) if TIMING.contains(&head.split('.').next().unwrap_or_default()) => {
                parts.push(line);
            }
            Some(_) => {}
        }
    }
    let text = format!("{}\n\n{}", top.join("\n"), parts.join("\n"))
        .trim()
        .to_owned();
    let kept: toml::Table = toml::from_str(&text).ok()?;
    (kept == timing_of(&table)).then_some(text)
}

/// Whether the made `skill.toml` `made` starts and is timed exactly as the
/// old skill's `kept` ([`kept_timing`]) says.
pub fn same_timing(kept: &str, made: &str) -> bool {
    match (
        toml::from_str::<toml::Table>(kept),
        toml::from_str::<toml::Table>(made),
    ) {
        (Ok(kept), Ok(made)) => timing_of(&kept) == timing_of(&made),
        _ => false,
    }
}

/// Why the kept made skill `entry` is not one to remake: `None` when it is
/// no candidate at all (not the person's, not DeepSeek's, changed, a quick
/// reaction, or one whose steps all run on the lamp); `Some` of a reason to
/// journal when it would be, but is left as it is (no request of theirs
/// kept, or its start and timing can't be kept).
fn picked(keeping: &Keeping, entry: &Value) -> Result<(), Option<&'static str>> {
    let Some(name) = entry["name"].as_str().filter(|name| !name.is_empty()) else {
        return Err(None);
    };
    let from = entry["from"].as_str().unwrap_or_default();
    // Made here, by voice or in the app: never an upload or a Store skill.
    if entry["on"] != true
        || entry.get("added").is_some()
        || entry.get("old").is_some()
        || !(from.starts_with("local:") || from.starts_with("written:"))
    {
        return Err(None);
    }
    let folder = keeping.installed.join(name);
    let Ok(card) = fs::read_to_string(folder.join("SKILL.md")) else {
        return Err(None);
    };
    // A core skill or a Store skill of that name: never the person's.
    if !card.lines().any(|line| line.trim() == "made: here") {
        return Err(None);
    }
    if made_with(entry, &card) != "deepseek" {
        return Err(None);
    }
    // Changed since it was made: another package, or another `skill.toml`.
    let sha256 = entry["sha256"].as_str().unwrap_or_default();
    if made_skill::went_on_as(keeping, name) != Ok(Some(sha256.to_owned())) {
        return Err(None);
    }
    let Ok(toml) = fs::read_to_string(folder.join("skill.toml")) else {
        return Err(None);
    };
    if entry["uses"]["code"] != true && entry["toml_sha256"] != hex_of(toml.as_bytes()).as_str() {
        return Err(None);
    }
    if made_skill::quick_reaction(&toml) {
        return Err(None);
    }
    // Its steps all run on the lamp: never remade (01:29).
    let Ok(table) = toml::from_str::<toml::Table>(&toml) else {
        return Err(None);
    };
    if !needs_a_model_each_run(&table) {
        return Err(None);
    }
    // Its own start and timing, and the person's own words, or it stays
    // as it is (01:22).
    if kept_request(entry).is_none() {
        return Err(Some("no request of theirs is kept"));
    }
    if kept_timing(&toml).is_none() {
        return Err(Some("its trigger and timing can't be kept as they are"));
    }
    Ok(())
}

/// The kept made skills to remake, oldest first.
pub fn chosen(keeping: &Keeping) -> Vec<Value> {
    made_skill::entries(&keeping.directory)
        .into_iter()
        .filter(|entry| picked(keeping, entry).is_ok())
        .collect()
}

/// The names of [`chosen`], each one left as it is journaled with why (no
/// word of the skill).
pub fn chosen_names(keeping: &Keeping) -> Vec<String> {
    made_skill::entries(&keeping.directory)
        .iter()
        .filter(|entry| match picked(keeping, entry) {
            Ok(()) => true,
            Err(Some(why)) => {
                eprintln!("lamp-voice: an old made skill is left as it is, not remade: {why}");
                false
            }
            Err(None) => false,
        })
        .filter_map(|entry| entry["name"].as_str().map(str::to_owned))
        .collect()
}

/// The one line once they are remade: the real count and one real name.
/// Nothing is asked: a remade skill runs at its first start with no yes
/// (the first builder, 18 Nov 21:58 and 19 Nov 01:29).
pub fn told_line(names: &[String]) -> Option<String> {
    let first = made_skill::spoken_name(names.first()?);
    Some(match names.len() {
        1 => format!(
            "I updated your {first} skill so your agent does it; it still starts when it did. Say 'put my old {first} skill back' to undo."
        ),
        count => format!(
            "I updated {count} of your skills so your agent does them; each still starts when it did. Say 'put my old {first} skill back' to undo."
        ),
    })
}

/// A skill's failed tries: the day of the last and how many.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tried {
    pub day: i64,
    pub times: u32,
}

/// The pass's record ([`STATE_FILE`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    /// The names to remake, not remade yet.
    #[serde(default)]
    pub queue: Vec<String>,
    /// The names whose old one the person put back: never queued on their
    /// own again, only on their ask ([`asked`]).
    #[serde(default)]
    pub put_back: BTreeSet<String>,
    #[serde(default)]
    pub failed: BTreeMap<String, Tried>,
    /// The day [`State::begun_today`] counts.
    #[serde(default)]
    pub day: i64,
    #[serde(default)]
    pub begun_today: u32,
    /// Remade, not told yet.
    #[serde(default)]
    pub untold: Vec<String>,
}

/// Whether no remake can begin again today: none queued, the day's
/// [`MOST_A_DAY`] begun, or each queued one tried today.
fn done_today(state: &State) -> bool {
    state.queue.is_empty()
        || state.begun_today >= MOST_A_DAY
        || state.queue.iter().all(|name| {
            state
                .failed
                .get(name)
                .is_some_and(|tried| tried.day == state.day)
        })
}

/// The made skills' index as it stands (its time and size): scanned for
/// skills to queue at the same mark, none is new ([`step`]), with no scan.
fn index_mark(directory: &Path) -> Option<(SystemTime, u64)> {
    let meta = fs::metadata(directory.join(made_skill::INDEX)).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// The index mark last scanned for skills to queue, by made skills' folder.
static SCANNED_AT: std::sync::Mutex<BTreeMap<std::path::PathBuf, (SystemTime, u64)>> =
    std::sync::Mutex::new(BTreeMap::new());

/// The record in `directory`; none is a fresh one.
pub fn load(directory: &Path) -> State {
    fs::read(directory.join(STATE_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save(directory: &Path, state: &State) -> io::Result<()> {
    let new = directory.join(format!(".{STATE_FILE}.new"));
    fs::write(&new, serde_json::to_vec(state).map_err(io::Error::other)?)?;
    fs::rename(&new, directory.join(STATE_FILE))
}

/// The language the line after is told in: the lamp's own, else the
/// conversation's.
fn said_in(language: Option<crate::language::Language>) -> &'static str {
    use crate::language::Language;
    match language {
        None => "the conversation's language",
        Some(Language::Vietnamese) => "Vietnamese",
        Some(Language::Japanese) => "Japanese",
        Some(Language::Chinese) => "Mandarin Chinese",
        Some(Language::Korean) => "Korean",
    }
}

/// The note for the owner's heard line (`owner`; none for anyone else,
/// whose line reads nothing here), with nothing else waiting (`busy`): the
/// one line once no remake can begin again today ([`told_line`]), told
/// once in `language` ([`said_in`]). Never an ask.
pub fn note(
    keeping: &Keeping,
    owner: bool,
    busy: bool,
    language: Option<crate::language::Language>,
) -> Option<String> {
    if !owner || busy {
        return None;
    }
    let mut state = load(&keeping.directory);
    if state.untold.is_empty() || !done_today(&state) {
        return None;
    }
    let line = told_line(&state.untold)?;
    // Marked before it is told: told at most once.
    state.untold.clear();
    save(&keeping.directory, &state).ok()?;
    eprintln!("lamp-voice: old made skills remade: the person is told");
    Some(format!(
        "(For you to tell the person first, before anything else, once, in {}: every part of \
         it, the skills' names kept as written, nothing added: {line} Then answer them.)",
        said_in(language)
    ))
}

/// The person's own ask to remake their old skills (`heard`: spoken;
/// `owner_hears`: the owner's line): every one that qualifies queued, one
/// they put back or out of tries too, remade by the pass within its caps.
/// On a lamp with no agent for their tasks (`agent_here`), none is queued.
/// What the model is told.
pub fn asked(
    keeping: &Keeping,
    heard: bool,
    owner_hears: impl FnOnce() -> bool,
    agent_here: impl FnOnce() -> bool,
) -> Value {
    if !heard {
        return json!({ "error": NOT_ASKED_ALOUD });
    }
    if !owner_hears() {
        return json!({
            "error": NOT_OWNER,
            "next": "Say plainly, in a few words, that only the lamp's owner can have their \
                     skills remade. Call nothing."
        });
    }
    if !agent_here() {
        return json!({
            "next": "Say in a few words, in the conversation's language, that their skills can't \
                     be updated on this lamp now, and that each keeps working as it is. Call \
                     nothing."
        });
    }
    let names = chosen_names(keeping);
    if names.is_empty() {
        return json!({
            "next": "Say in a few words, in the conversation's language, that none of their \
                     skills needs remaking. Call nothing."
        });
    }
    let mut state = load(&keeping.directory);
    for name in &names {
        state.put_back.remove(name);
        state.failed.remove(name);
        if !state.queue.contains(name) {
            state.queue.push(name.clone());
        }
    }
    if save(&keeping.directory, &state).is_err() {
        return crate::tools::unavailable();
    }
    eprintln!(
        "lamp-voice: old made skills: the owner asked: {} queued",
        names.len()
    );
    json!({
        "next": format!(
            "Say in a few words, in the conversation's language, that you'll update {} of their \
             skills one at a time while it's quiet, and that each keeps working as it is until \
             then. Call nothing.",
            names.len()
        )
    })
}

/// When and how busy it is, for [`step`]: the lamp's own, or a test's.
pub trait When {
    fn now_ms(&self) -> u64;
    /// The local day, for [`MOST_A_DAY`].
    fn day(&self) -> i64;
    /// In the made skills' quiet hours (`made_skill::MANNERS_QUIET`).
    fn quiet(&self) -> bool;
    /// Since a line was last heard.
    fn idle(&self) -> Duration;
}

/// What a remake does on the lamp: the lamp's own, or a test's.
pub trait Hands {
    /// Whether the lamp has an agent to do a remade skill's task: none, no
    /// remake is tried (Twelve's P3 on #7820).
    fn agent_here(&self) -> bool;
    /// `request` made in the middle form, one task for the lamp's agent
    /// starting exactly as `timing` (the old skill's own, [`kept_timing`])
    /// says, with sip-of-water's guards but on a clock, kept under `name`
    /// and not on; and the agent that does its task, as `made_with` says
    /// it.
    fn make(
        &self,
        keeping: &Keeping,
        request: &str,
        name: &str,
        timing: &str,
    ) -> Result<(Made, String), String>;
    /// The kept skill `sha256` put on the lamp.
    fn put_on(&self, keeping: &Keeping, sha256: &str) -> Result<(), String>;
    /// The remade skill `made` put on the lamp with its task's yes recorded
    /// for its grants (`make_skill_local::code_on_at_once`), so its first
    /// run asks nothing: whether the yes was kept.
    fn put_on_with_yes(&self, keeping: &Keeping, made: &Made) -> Result<bool, String>;
    /// The skill `name` turned off or on.
    fn turn(&self, keeping: &Keeping, name: &str, off: bool) -> Result<(), String>;
    /// The skills turned off on the lamp.
    fn turned_off(&self) -> BTreeSet<String>;
}

/// What one look of the pass did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Nothing to remake.
    Nothing,
    /// A remake waits: why.
    Waits(&'static str),
    /// Remade under its own name.
    Kept(String),
    /// Not kept: why, in the lamp's own short words.
    NotKept(&'static str),
}

/// One remake at a time, in this process.
static REMAKING: AtomicBool = AtomicBool::new(false);

/// A person's make came while a remake ran ([`stand_down`]): the remake
/// keeps nothing, and is tried again another day.
static STOOD_DOWN: AtomicBool = AtomicBool::new(false);

/// Why a remake kept nothing when a person's make came: never counted as
/// one of its [`MOST_TRIES`].
pub const STOOD_DOWN_WHY: &str = "a person's make went first";

/// The person goes first (#7765): a make of the person's, refused because
/// lamp-tasks' one make slot is taken, asks a remake running here to stand
/// down. Whether one runs: the person's make then waits for the slot,
/// never told busy (`make_skill_agent::Agent::write`). Since the middle
/// form a remake never takes that slot itself; a remake running while a
/// person's make waits for another make's slot still keeps nothing, so the
/// person's make is never raced.
pub fn stand_down() -> bool {
    if made_skill::remaking_here() || !REMAKING.load(Ordering::Acquire) {
        return false;
    }
    STOOD_DOWN.store(true, Ordering::Release);
    true
}

struct One;

impl One {
    fn take() -> Option<Self> {
        let one = REMAKING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            .then_some(Self);
        if one.is_some() {
            STOOD_DOWN.store(false, Ordering::Release);
        }
        one
    }
}

impl Drop for One {
    fn drop(&mut self) {
        REMAKING.store(false, Ordering::Release);
    }
}

/// One look of the pass: old ones past [`UNDO_FOR`] dropped, then at most
/// one remake, within every cap.
pub fn step(keeping: &Keeping, hands: &impl Hands, when: &impl When) -> Step {
    let Some(_one) = One::take() else {
        return Step::Waits("another remake runs");
    };
    let now_ms = when.now_ms();
    drop_old(keeping, now_ms);
    let mut state = load(&keeping.directory);
    // Queued on their own (18:23): each that qualifies, never one put back
    // or out of tries, scanned again only once the index changed.
    if state.queue.is_empty() {
        let mark = index_mark(&keeping.directory);
        let scanned = mark.is_some()
            && SCANNED_AT
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(&keeping.directory)
                .copied()
                == mark;
        if !scanned {
            state.queue = chosen_names(keeping)
                .into_iter()
                .filter(|name| {
                    !state.put_back.contains(name)
                        && state
                            .failed
                            .get(name)
                            .is_none_or(|tried| tried.times < MOST_TRIES)
                })
                .collect();
            if let Some(mark) = mark {
                SCANNED_AT
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .insert(keeping.directory.clone(), mark);
            }
            if !state.queue.is_empty() {
                eprintln!(
                    "lamp-voice: old made skills: {} queued to remake",
                    state.queue.len()
                );
                let _ = save(&keeping.directory, &state);
            }
        }
        if state.queue.is_empty() {
            return Step::Nothing;
        }
    }
    let day = when.day();
    if state.day != day {
        state.day = day;
        state.begun_today = 0;
    }
    if state.begun_today >= MOST_A_DAY {
        return Step::Waits("five remakes today");
    }
    if when.quiet() {
        return Step::Waits("the quiet hours");
    }
    if when.idle() < IDLE_FOR {
        return Step::Waits("a conversation not long ago");
    }
    // No agent for its task on this lamp: nothing is tried, no try is
    // counted, and the old skills stay (Twelve's P3 on #7820).
    if !hands.agent_here() {
        return Step::Waits("no agent for its task here");
    }
    // One not tried today and tried fewer than [`MOST_TRIES`] times.
    let Some(name) = state
        .queue
        .iter()
        .find(|name| state.failed.get(*name).is_none_or(|tried| tried.day != day))
        .cloned()
    else {
        return Step::Waits("each tried today");
    };
    let Some(old) = chosen(keeping)
        .into_iter()
        .find(|entry| entry["name"] == name.as_str())
    else {
        // Changed, removed or remade since the yes: left as it is.
        state.queue.retain(|queued| *queued != name);
        let _ = save(&keeping.directory, &state);
        return Step::NotKept("no longer one to remake");
    };
    state.begun_today += 1;
    if save(&keeping.directory, &state).is_err() {
        return Step::Waits("the record can't be kept");
    }
    let done = remake(keeping, hands, &old, now_ms);
    let mut state = load(&keeping.directory);
    let step = match done {
        Ok(()) => {
            state.queue.retain(|queued| *queued != name);
            state.failed.remove(&name);
            state.untold.push(name.clone());
            eprintln!("lamp-voice: a made skill's remake: kept");
            Step::Kept(name)
        }
        Err(why) => {
            let tried = state.failed.entry(name.clone()).or_default();
            tried.day = day;
            // Standing down for a person is no try of its own.
            if why != STOOD_DOWN_WHY {
                tried.times += 1;
            }
            if tried.times >= MOST_TRIES {
                state.queue.retain(|queued| *queued != name);
            }
            eprintln!("lamp-voice: a made skill's remake: not kept: {why}");
            Step::NotKept(why)
        }
    };
    let _ = save(&keeping.directory, &state);
    step
}

/// The kept skill `old` remade by the agent and put on in its place, its
/// on/off state kept; the old one kept under the new one's `old`. Not
/// kept, the old stays exactly as it was.
fn remake(
    keeping: &Keeping,
    hands: &impl Hands,
    old: &Value,
    now_ms: u64,
) -> Result<(), &'static str> {
    let name = old["name"].as_str().unwrap_or_default();
    let old_sha = old["sha256"].as_str().unwrap_or_default();
    // The person's own kept words, never the said-back description, and
    // its own start and timing: else it stays as it is (01:22).
    let request = kept_request(old).ok_or("no request of theirs is kept")?;
    let timing = fs::read_to_string(keeping.installed.join(name).join("skill.toml"))
        .ok()
        .and_then(|toml| kept_timing(&toml))
        .ok_or("its trigger and timing can't be kept as they are")?;
    let was_off = hands.turned_off().contains(name);
    let made = hands.make(keeping, request, name, &timing);
    // A person's make came meanwhile: nothing of this one is kept.
    let stood_down = STOOD_DOWN.load(Ordering::Acquire);
    let (made, engine) = made.map_err(|why| {
        if stood_down || why == made_skill::AGENT_BUSY_TRY {
            STOOD_DOWN_WHY
        } else if why == made_skill::REMAKE_TIMING_CHANGED {
            made_skill::REMAKE_TIMING_CHANGED
        } else {
            "its task wasn't kept"
        }
    })?;
    let unput = |why| {
        made_skill::forget_unput(&keeping.directory, &made.sha256);
        Err(why)
    };
    if stood_down {
        return unput(STOOD_DOWN_WHY);
    }
    if made.name != name || made.sha256 == old_sha {
        return unput("it wasn't made as a new skill of its name");
    }
    // The middle form's only code is the lamp's own agent script
    // (`Uses::agent_task`); any other code waits for a spoken yes: never
    // put on here.
    if made.uses.code && !made.uses.agent_task {
        return unput("it has code of its own, which waits for a yes");
    }
    // Off by name before the new one goes on (lamp-skills' `off` file,
    // which an install keeps), so one that was off is never on between.
    if was_off && hands.turn(keeping, name, true).is_err() {
        return unput("it couldn't be kept off as it was");
    }
    // Nothing asked (the first builder, 18 Nov 21:58, and 19 Nov 01:29):
    // with `LAMP_MAKE_NO_ASKS` on, its task's yes is recorded as it goes
    // on, as a new make's is, so it runs at its first start with no
    // question; off, as before, its task asks when it first runs.
    let on = if crate::make_no_asks::on() {
        hands.put_on_with_yes(keeping, &made)
    } else {
        hands.put_on(keeping, &made.sha256).map(|()| true)
    };
    match on {
        Err(_) => return unput("it couldn't go on"),
        Ok(false) => eprintln!(
            "lamp-voice: a made skill's remake: on, but its task's yes wasn't kept: it asks when \
             it first runs"
        ),
        Ok(true) => {}
    }
    // The old entry as it was chosen, so its undo is always kept.
    let written = made_skill::index_locked(&keeping.directory, || {
        let mut entries = made_skill::entries(&keeping.directory);
        entries.retain(|entry| entry["sha256"] != old_sha);
        let mut kept_old = old.clone();
        kept_old["replaced_at_ms"] = json!(now_ms);
        kept_old["was_off"] = json!(was_off);
        for entry in &mut entries {
            if entry["sha256"] == made.sha256.as_str() {
                entry[made_skill::MADE_WITH] = json!(engine);
                entry["remade_from"] = json!(old_sha);
                entry["old"] = kept_old.clone();
            }
        }
        made_skill::write_entries(&keeping.directory, &entries)
    });
    if let Err(error) = written {
        eprintln!("lamp-voice: a made skill's remake: on, but its old one not marked: {error}");
    }
    Ok(())
}

/// The kept old ones of the remade skills in `directory` for which `drop`
/// holds taken out of their entries, each package with them unless an
/// entry still names it: how many.
fn drop_olds(directory: &Path, drop: impl Fn(&Value) -> bool) -> io::Result<usize> {
    made_skill::index_locked(directory, || {
        let mut entries = made_skill::entries(directory);
        let mut gone = Vec::new();
        for entry in &mut entries {
            if entry.get("old").is_some()
                && drop(&entry["old"])
                && let Some(old) = entry.as_object_mut().and_then(|entry| entry.remove("old"))
            {
                gone.push(old);
            }
        }
        if gone.is_empty() {
            return Ok(0);
        }
        made_skill::write_entries(directory, &entries)?;
        for old in &gone {
            drop_package_of(directory, old, &entries);
        }
        Ok(gone.len())
    })
}

/// The package of `old`, a remade skill's kept old one, deleted unless one
/// of `entries` still names it ([`made_skill::packages_named`]).
pub(crate) fn drop_package_of(directory: &Path, old: &Value, entries: &[Value]) {
    if let Some(package) = old["package"].as_str().filter(|package| {
        !package.is_empty()
            && !package.contains('/')
            && !package.starts_with('.')
            && !entries
                .iter()
                .any(|entry| made_skill::packages_named(entry).any(|named| named == *package))
    }) {
        let _ = fs::remove_file(directory.join(package));
    }
}

/// The old ones kept past [`UNDO_FOR`] dropped, their packages with them
/// unless another entry names one.
pub fn drop_old(keeping: &Keeping, now_ms: u64) {
    let past = |old: &Value| {
        old["replaced_at_ms"]
            .as_u64()
            .is_some_and(|at| now_ms >= at + UNDO_FOR.as_millis() as u64)
    };
    match drop_olds(&keeping.directory, past) {
        Ok(0) => {}
        Ok(_) => eprintln!("lamp-voice: a remade skill's old one is past its week: deleted"),
        Err(error) => eprintln!("lamp-voice: a remade skill's old one wasn't dropped: {error}"),
    }
}

/// The remake's own undo store in `directory` deleted whole (the owner's
/// "forget everything", with `crate::make_no_asks`' keep: one undo store,
/// Twelve's P2 on #7820): every remade skill's kept old one and its
/// package, and [`STATE_FILE`]. Whether there was any.
pub fn clear(directory: &Path) -> io::Result<bool> {
    let olds = drop_olds(directory, |_| true)?;
    let state = match fs::remove_file(directory.join(STATE_FILE)) {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error),
    };
    Ok(olds > 0 || state)
}

/// The remade skill named `asked` as heard ("water", "my old water skill")
/// whose old one is still kept within [`UNDO_FOR`] put back, on or off as
/// it was. What the model is told.
pub fn put_back(
    keeping: &Keeping,
    hands: &impl Hands,
    asked: &str,
    heard: bool,
    owner_hears: impl FnOnce() -> bool,
    now_ms: u64,
) -> Value {
    if !heard {
        return json!({ "error": NOT_ASKED_ALOUD });
    }
    if !owner_hears() {
        return json!({
            "error": NOT_OWNER,
            "next": "Say plainly, in a few words, that only the lamp's owner can put a skill \
                     back. Name no skill. Call nothing."
        });
    }
    drop_old(keeping, now_ms);
    let asked = made_skill::name_asked(asked);
    let asked = made_skill::name_digits(asked.strip_prefix("old-").unwrap_or(&asked));
    let directory = &keeping.directory;
    // The new one's entry swapped for its old one's, holding the index's
    // lock: none of a make's entries written meanwhile is lost.
    let swapped = made_skill::index_locked(directory, || {
        let mut entries = made_skill::entries(directory);
        let at = entries.iter().position(|entry| {
            entry.get("old").is_some()
                && entry["name"]
                    .as_str()
                    .is_some_and(|name| made_skill::name_digits(name) == asked)
        })?;
        let new = entries[at].clone();
        let mut old = new["old"].clone();
        if let Some(old) = old.as_object_mut() {
            old.remove("replaced_at_ms");
            old.remove("was_off");
            old.insert("on".into(), json!(false));
        }
        entries[at] = old.clone();
        Some(made_skill::write_entries(directory, &entries).map(|()| (new, old)))
    });
    let Some(swapped) = swapped else {
        return json!({
            "next": "Say plainly, in a few words, that there is no older version of that skill \
                     to put back. Call nothing."
        });
    };
    let Ok((new, old)) = swapped else {
        return crate::tools::unavailable();
    };
    let name = new["name"].as_str().unwrap_or_default().to_owned();
    let was_off = new["old"]["was_off"] == true;
    let old_sha = old["sha256"].as_str().unwrap_or_default();
    if hands.put_on(keeping, old_sha).is_err() {
        // Back as it was: the new one stays on.
        let _ = made_skill::index_locked(directory, || {
            let mut entries = made_skill::entries(directory);
            for entry in &mut entries {
                if entry["sha256"] == old_sha {
                    *entry = new.clone();
                }
            }
            made_skill::write_entries(directory, &entries)
        });
        eprintln!("lamp-voice: a remade skill's old one couldn't go back on");
        return json!({
            "next": "Say plainly, in a few words, that the old one couldn't be put back just now, \
                     and that the new one stays. Call nothing."
        });
    }
    if was_off && hands.turn(keeping, &name, true).is_err() {
        eprintln!("lamp-voice: a remade skill's old one is back on, but not turned off as it was");
    }
    let entries = made_skill::entries(directory);
    if let Some(package) = new["package"]
        .as_str()
        .filter(|package| !package.is_empty() && !package.contains('/'))
        .filter(|package| {
            entries
                .iter()
                .all(|entry| made_skill::packages_named(entry).all(|named| named != *package))
        })
    {
        let _ = fs::remove_file(directory.join(package));
    }
    // Never remade on its own again: only on the person's ask.
    let mut state = load(&keeping.directory);
    state.queue.retain(|queued| *queued != name);
    state.put_back.insert(name.clone());
    let _ = save(&keeping.directory, &state);
    eprintln!("lamp-voice: a remade skill's old one is put back");
    json!({
        "next": format!(
            "Say in a few words, in the conversation's language, that your old {} skill is \
             back. Call nothing.",
            made_skill::spoken_name(&name)
        )
    })
}

/// When a line was last heard here, by the lamp's clock.
static LAST_HEARD_MS: AtomicU64 = AtomicU64::new(0);

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// The lamp's own [`When`].
struct Here;

impl When for Here {
    fn now_ms(&self) -> u64 {
        now_unix_ms()
    }

    fn day(&self) -> i64 {
        let offset = crate::local_time::offset_here().unwrap_or_default();
        (self.now_ms() as i64 / 1_000 + i64::from(offset)).div_euclid(86_400)
    }

    /// A zone that can't be read is taken as quiet: never a remake then.
    fn quiet(&self) -> bool {
        let Some(zone) = crate::local_time::system() else {
            return true;
        };
        let minute = u32::from(zone.minute_of_day(self.now_ms()));
        let at = |hh_mm: &str| {
            let (hours, minutes) = hh_mm.split_once(':').unwrap_or(("0", "0"));
            hours.parse::<u32>().unwrap_or(0) * 60 + minutes.parse::<u32>().unwrap_or(0)
        };
        let (from, to) = made_skill::MANNERS_QUIET;
        minute >= at(from) || minute < at(to)
    }

    fn idle(&self) -> Duration {
        Duration::from_millis(
            self.now_ms()
                .saturating_sub(LAST_HEARD_MS.load(Ordering::Relaxed)),
        )
    }
}

/// The lamp's own [`Hands`].
struct Lamp;

/// The engine of the lamp's agent that does a remade skill's task, if one
/// is on the lamp and can.
fn lamps_engine() -> Option<&'static str> {
    let agent = crate::make_skill_agent::Agent::for_voice()?;
    agent.lamp_agent().0.and_then(|engine| agent.takes(engine))
}

impl Hands for Lamp {
    fn agent_here(&self) -> bool {
        lamps_engine().is_some()
    }

    fn make(
        &self,
        keeping: &Keeping,
        request: &str,
        name: &str,
        timing: &str,
    ) -> Result<(Made, String), String> {
        // The middle form (19:50): written by the lamp, its start the old
        // skill's own (01:22); the agent only does its task when it fires,
        // so one must be on the lamp, but it is never asked to write it,
        // and lamp-tasks' make slot is never taken.
        let engine = lamps_engine().ok_or("no agent for its task here")?;
        let _as = made_skill::RemakeAs::set(name, timing);
        let from = format!("local:remake:{}", now_unix_ms());
        crate::make_skill_local::remade_as_task(
            request,
            keeping,
            &from,
            engine,
            crate::make_skill_local::own_chat,
        )
        .map(|made| {
            (
                made,
                made_skill::MadeWith::of_engine(engine).key().to_owned(),
            )
        })
    }

    fn put_on(&self, keeping: &Keeping, sha256: &str) -> Result<(), String> {
        made_skill::put_on_within(
            &keeping.directory,
            &keeping.manage,
            sha256,
            now_unix_ms(),
            HELPER_WAIT,
        )
        .map(|_| ())
    }

    fn put_on_with_yes(&self, keeping: &Keeping, made: &Made) -> Result<bool, String> {
        crate::make_skill_local::code_on_at_once(keeping, made)
    }

    fn turn(&self, keeping: &Keeping, name: &str, off: bool) -> Result<(), String> {
        crate::made_turn::turned_by_helper(&keeping.manage, name, off)
    }

    fn turned_off(&self) -> BTreeSet<String> {
        crate::skill_about::turned_off(Path::new(crate::skill_about::STATE_DIR)).unwrap_or_default()
    }
}

/// The note for the heard line `request` on the lamp: a line heard now, and
/// the one line after ([`note`]) for the owner (`owner`). None with
/// the switch off.
pub fn note_for(request: u64, owner: bool) -> Option<String> {
    let keeping = Keeping::when(true)?;
    note_for_in(on(), &keeping, request, owner)
}

/// [`note_for`] with the switch `on`, of the made skills in `keeping`.
fn note_for_in(on: bool, keeping: &Keeping, request: u64, owner: bool) -> Option<String> {
    if !on {
        return None;
    }
    let now_ms = now_unix_ms();
    LAST_HEARD_MS.store(now_ms, Ordering::Relaxed);
    let now = std::time::Instant::now();
    let busy = owner
        && (crate::skill_write_yes::awaited(now)
            || crate::made_clash::awaited(now)
            || crate::make_skill_local::pending());
    let note = note(keeping, owner, busy, crate::language::at_start());
    if note.is_some() {
        eprintln!("lamp-voice second: request {request}: old made skills: a note");
    }
    note
}

/// `make_skill`'s [`REMAKE_OLD`] on the lamp: [`ASKED`] alone.
pub fn answered_here(
    _answer: &str,
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
) -> Value {
    let Some(keeping) = Keeping::when(true) else {
        return crate::tools::unavailable();
    };
    asked(
        &keeping,
        heard,
        || crate::services::owner_hears(request, heard, lease),
        || Lamp.agent_here(),
    )
}

/// `make_skill`'s [`PUT_BACK`] on the lamp.
pub fn put_back_here(
    asked: &str,
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
) -> Value {
    let Some(keeping) = Keeping::when(true) else {
        return crate::tools::unavailable();
    };
    put_back(
        &keeping,
        &Lamp,
        asked,
        heard,
        || crate::services::owner_hears(request, heard, lease),
        now_unix_ms(),
    )
}

/// Start the pass, once a process, with the switch on: a look every
/// [`LOOK_EVERY`], on a thread of its own.
pub fn start() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if !on() || STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    LAST_HEARD_MS.store(now_unix_ms(), Ordering::Relaxed);
    let spawned = std::thread::Builder::new()
        .name("lamp-voice-remake".into())
        .spawn(|| {
            loop {
                std::thread::sleep(LOOK_EVERY);
                if let Some(keeping) = Keeping::when(true)
                    && !crate::make_skill_local::pending()
                {
                    step(&keeping, &Lamp, &Here);
                }
            }
        });
    match spawned {
        Ok(_) => eprintln!(
            "lamp-voice second: old made skills are remade in the middle form, without asking ({ENVIRONMENT})"
        ),
        Err(error) => eprintln!("lamp-voice second: the remake pass didn't start: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make_skill_local::tests::keeping;
    use std::cell::{Cell, RefCell};
    use std::path::PathBuf;
    use std::sync::Mutex;

    /// The pass's one-at-a-time guard is the process's: its tests run one
    /// at a time.
    static SERIAL: Mutex<()> = Mutex::new(());

    /// An old skill on someone's coming back whose say is the model's own
    /// words each time: it needs a model at each run.
    const PRESENCE: &str = "kind = \"installable\"\nversion = \"1.0.0\"\nmost_a_day = 2\n\n\
        [trigger]\ntopic = \"room/presence\"\nfield = \"present\"\nequals = true\n\
        cooldown = \"1h\"\n\n[active]\nfrom = \"08:00\"\nto = \"20:00\"\n\n[quiet]\n\
        from = \"22:00\"\nto = \"08:00\"\n\n[[do]]\nsay = { note = \"Welcome them back.\" }\n";
    /// An old skill whose steps all run on the lamp: its light, a fixed
    /// say, its body and its hum.
    const LOCAL: &str = "kind = \"installable\"\nversion = \"1.0.0\"\n\n[trigger]\n\
        topic = \"room/presence\"\nfield = \"present\"\nequals = true\n\n[[do]]\n\
        light = { colour = \"warm\" }\n\n[[do]]\nsay = { text = \"Welcome back.\" }\n\n\
        [[do]]\nbody = { gesture = \"nod\" }\n\n[[do]]\ncue = \"hum\"\n";
    /// "Every morning at 8, tell me the weather", as DeepSeek kept it.
    const MORNING: &str = "kind = \"installable\"\nversion = \"1.0.0\"\n\n[trigger]\n\
        topic = \"clock/local\"\nfield = \"known\"\nequals = true\nrearm = \"ends\"\n\
        during = { from = \"08:00\", to = \"08:01\" }\n\n[[do]]\n\
        read = { service = \"weather\" }\n";
    const QUICK: &str = "kind = \"installable\"\nversion = \"1.0.0\"\n\n[trigger]\n\
        topic = \"controls/pet\"\nfield = \"ended\"\nequals = false\n\n[[do]]\n\
        body = { gesture = \"nod\" }\n\n[[do]]\nsay = { text = \"Hello!\" }\n";
    const HOUR_MS: u64 = 3_600_000;
    const DAY_MS: u64 = 86_400_000;

    fn package_of(keeping: &Keeping, name: &str, bytes: &str) -> (String, String) {
        fs::create_dir_all(&keeping.directory).unwrap();
        let sha = hex_of(bytes.as_bytes());
        let package = format!("{name}-1.0.0-{}.tar", &sha[..12]);
        fs::write(keeping.directory.join(&package), bytes).unwrap();
        (sha, package)
    }

    fn install(keeping: &Keeping, name: &str, toml: &str, sha: &str, card: &str) {
        let folder = keeping.installed.join(name);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("skill.toml"), toml).unwrap();
        fs::write(folder.join("SKILL.md"), card).unwrap();
        fs::write(
            folder.join(".package.json"),
            json!({ "sha256": sha }).to_string(),
        )
        .unwrap();
    }

    fn push_entry(keeping: &Keeping, entry: Value) {
        let mut entries = made_skill::entries(&keeping.directory);
        entries.push(entry);
        made_skill::write_entries(&keeping.directory, &entries).unwrap();
    }

    /// The person's own request kept for the old skill `name`.
    fn request_of(name: &str) -> String {
        format!("When I come back to my desk, welcome me back, {name}")
    }

    /// A made skill on the lamp, as kept before the route, its request
    /// kept ([`request_of`]).
    fn old_skill(keeping: &Keeping, name: &str, toml: &str, from: &str) -> String {
        old_skill_asked(keeping, name, toml, from, Some(&request_of(name)))
    }

    /// [`old_skill`], with `request` kept, or none.
    fn old_skill_asked(
        keeping: &Keeping,
        name: &str,
        toml: &str,
        from: &str,
        request: Option<&str>,
    ) -> String {
        let (sha, package) = package_of(keeping, name, &format!("old {name}"));
        install(
            keeping,
            name,
            toml,
            &sha,
            &format!("---\nname: {name}\ndescription: d\nmade: here\nmade_by: voice\n---\n"),
        );
        let mut entry = json!({
            "from": from, "name": name, "version": "1.0.0", "sha256": sha,
            "toml_sha256": hex_of(toml.as_bytes()),
            "description": format!("When you come back, {name} welcomes you."),
            "uses": {"speak": true}, "package": package, "on": true,
        });
        if let Some(request) = request {
            entry["request"] = json!(request);
        }
        push_entry(keeping, entry);
        sha
    }

    fn entry_named(keeping: &Keeping, name: &str) -> Vec<Value> {
        made_skill::entries(&keeping.directory)
            .into_iter()
            .filter(|entry| entry["name"] == name)
            .collect()
    }

    /// A fake middle-form make and manage helper.
    #[derive(Default)]
    struct Fake {
        fail: bool,
        /// Its make has code of the model's own, not the lamp's agent
        /// script alone.
        own_code: bool,
        asked: RefCell<Vec<String>>,
        put: RefCell<Vec<String>>,
        turned: RefCell<Vec<(String, bool)>>,
        off: BTreeSet<String>,
        /// A look of the pass taken while this make runs.
        inner: Cell<Option<bool>>,
        /// A person's make comes while this make runs ([`stand_down`]).
        person: bool,
        /// The helper's calls, in order.
        order: RefCell<Vec<&'static str>>,
        /// No agent on this lamp.
        no_agent: bool,
        /// The start and timing each make was given.
        timings: RefCell<Vec<String>>,
        /// The skills put on with their task's yes recorded.
        yes: RefCell<Vec<String>>,
    }

    impl Hands for Fake {
        fn agent_here(&self) -> bool {
            !self.no_agent
        }

        fn make(
            &self,
            keeping: &Keeping,
            request: &str,
            name: &str,
            timing: &str,
        ) -> Result<(Made, String), String> {
            self.asked.borrow_mut().push(request.to_owned());
            self.timings.borrow_mut().push(timing.to_owned());
            if self.inner.get() == Some(false) {
                let again = step(keeping, &Fake::default(), &Clock::day_idle());
                self.inner
                    .set(Some(again == Step::Waits("another remake runs")));
            }
            if self.person {
                assert!(stand_down(), "a remake runs");
            }
            if self.fail {
                return Err("no answer".into());
            }
            let (sha, package) = package_of(keeping, name, &format!("new {name}"));
            push_entry(
                keeping,
                json!({
                    "from": "local:remake:1", "name": name, "version": "1.0.0",
                    "sha256": sha, "toml_sha256": "t", "description": request,
                    "uses": {"code": true, "agent_task": !self.own_code},
                    "package": package,
                }),
            );
            // The middle form: one task for the lamp's agent, the lamp's
            // own script its only code.
            let uses = made_skill::Uses {
                code: true,
                agent_task: !self.own_code,
                ..made_skill::Uses::default()
            };
            Ok((
                Made {
                    name: name.to_owned(),
                    version: "1.0.0".into(),
                    sha256: sha,
                    description: request.to_owned(),
                    uses,
                    package: keeping.directory.join(package),
                },
                "hermes".into(),
            ))
        }

        fn put_on(&self, keeping: &Keeping, sha256: &str) -> Result<(), String> {
            self.put.borrow_mut().push(sha256.to_owned());
            self.order.borrow_mut().push("put");
            let mut entries = made_skill::entries(&keeping.directory);
            let mut name = String::new();
            for entry in &mut entries {
                if entry["sha256"] == sha256 {
                    entry["on"] = json!(true);
                    name = entry["name"].as_str().unwrap().to_owned();
                }
            }
            made_skill::write_entries(&keeping.directory, &entries).unwrap();
            fs::write(
                keeping.installed.join(&name).join(".package.json"),
                json!({ "sha256": sha256 }).to_string(),
            )
            .unwrap();
            Ok(())
        }

        fn put_on_with_yes(&self, keeping: &Keeping, made: &Made) -> Result<bool, String> {
            self.put_on(keeping, &made.sha256)?;
            self.yes.borrow_mut().push(made.name.clone());
            Ok(true)
        }

        fn turn(&self, _: &Keeping, name: &str, off: bool) -> Result<(), String> {
            self.turned.borrow_mut().push((name.to_owned(), off));
            self.order.borrow_mut().push("turn");
            Ok(())
        }

        fn turned_off(&self) -> BTreeSet<String> {
            self.off.clone()
        }
    }

    /// A fake clock and room.
    struct Clock {
        now_ms: u64,
        quiet: bool,
        idle: Duration,
    }

    impl Clock {
        fn day_idle() -> Self {
            Self {
                now_ms: 100 * DAY_MS + 10 * HOUR_MS,
                quiet: false,
                idle: IDLE_FOR,
            }
        }
    }

    impl When for Clock {
        fn now_ms(&self) -> u64 {
            self.now_ms
        }
        fn day(&self) -> i64 {
            (self.now_ms / DAY_MS) as i64
        }
        fn quiet(&self) -> bool {
            self.quiet
        }
        fn idle(&self) -> Duration {
            self.idle
        }
    }

    fn lamp(test: &str) -> (PathBuf, Keeping) {
        let (root, keeping) = keeping(&format!("remake-{test}"));
        fs::create_dir_all(&keeping.directory).unwrap();
        fs::create_dir_all(&keeping.installed).unwrap();
        (root, keeping)
    }

    #[test]
    fn only_a_deepseek_presence_skill_is_picked() {
        let (_root, keeping) = lamp("picked");
        old_skill(&keeping, "welcome-glow", PRESENCE, "local:1");
        old_skill(&keeping, "pat-nod", QUICK, "local:2");
        old_skill(&keeping, "from-file", PRESENCE, "upload:x.zip");
        // A core skill of that name: its card isn't marked made here.
        old_skill(&keeping, "stretch-break", PRESENCE, "local:3");
        fs::write(
            keeping.installed.join("stretch-break").join("SKILL.md"),
            "---\nname: stretch-break\ndescription: d\n---\n",
        )
        .unwrap();
        // Changed since made: another skill.toml in its folder.
        old_skill(&keeping, "changed", PRESENCE, "local:4");
        fs::write(
            keeping.installed.join("changed").join("skill.toml"),
            PRESENCE.replace("Welcome", "Greet"),
        )
        .unwrap();
        // Made by the agent already.
        old_skill(&keeping, "hermes-made", PRESENCE, "local:5");
        let mut entries = made_skill::entries(&keeping.directory);
        entries.last_mut().unwrap()["made_with"] = json!("hermes");
        made_skill::write_entries(&keeping.directory, &entries).unwrap();
        assert_eq!(chosen_names(&keeping), vec!["welcome-glow".to_owned()]);
        assert!(made_skill::quick_reaction(QUICK) && !made_skill::quick_reaction(PRESENCE));
    }

    /// The first builder, 19 Nov 01:29: a skill whose steps all run on the
    /// lamp (its light, a fixed say, its body, its hum) is never remade;
    /// only one that already needs a model or the agent at each run.
    #[test]
    fn a_skill_that_runs_on_the_lamp_is_never_picked() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("local");
        old_skill(&keeping, "warm-welcome", LOCAL, "local:1");
        let before = fs::read(keeping.directory.join(made_skill::INDEX)).unwrap();
        assert!(!made_skill::quick_reaction(LOCAL));
        assert!(chosen_names(&keeping).is_empty());
        let fake = Fake::default();
        assert_eq!(step(&keeping, &fake, &Clock::day_idle()), Step::Nothing);
        assert!(fake.asked.borrow().is_empty());
        assert_eq!(
            fs::read(keeping.directory.join(made_skill::INDEX)).unwrap(),
            before
        );
        // One step that needs a model each run is enough: a note, a read,
        // a look or a task; a fixed say's exact words are not.
        for (toml, needs) in [
            (LOCAL, false),
            (PRESENCE, true),
            (MORNING, true),
            (&LOCAL.replace("cue = \"hum\"", "look = {}"), true),
            (
                &LOCAL.replace("cue = \"hum\"", "say = \"a note of its own\""),
                true,
            ),
        ] {
            let table: toml::Table = toml::from_str(toml).unwrap();
            assert_eq!(needs_a_model_each_run(&table), needs, "{toml}");
        }
    }

    /// The first builder, 19 Nov 01:22: no request of theirs kept, or a
    /// start and timing the middle form can't hold, and the skill is left
    /// exactly as it is, never remade from its said-back description.
    #[test]
    fn a_skill_with_no_kept_request_or_no_kept_trigger_is_untouched() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("untouched");
        old_skill_asked(&keeping, "no-request", PRESENCE, "local:1", None);
        // Only asked for (no `[trigger]`): its phrases would be picked again.
        let asked_only = PRESENCE
            .split_once("[trigger]")
            .map(|(head, _)| format!("{head}[[do]]\nsay = {{ note = \"Hi.\" }}\n"))
            .unwrap();
        old_skill(&keeping, "asked-only", &asked_only, "local:2");
        // A trigger's lines that can't be taken out whole (a header with a
        // note on its line).
        old_skill(
            &keeping,
            "noted-header",
            &PRESENCE.replace("[trigger]", "[trigger] # when they come"),
            "local:3",
        );
        let before = fs::read(keeping.directory.join(made_skill::INDEX)).unwrap();
        assert!(chosen_names(&keeping).is_empty());
        let fake = Fake::default();
        assert_eq!(step(&keeping, &fake, &Clock::day_idle()), Step::Nothing);
        assert!(fake.asked.borrow().is_empty() && fake.put.borrow().is_empty());
        assert_eq!(
            fs::read(keeping.directory.join(made_skill::INDEX)).unwrap(),
            before
        );
        // The owner's own ask queues none of them either.
        assert!(
            asked(&keeping, true, || true, || true)["next"]
                .as_str()
                .unwrap()
                .contains("none of their skills")
        );
        // A middle form whose start or timing came out other than the
        // old one's keeps nothing.
        let kept = kept_timing(PRESENCE).unwrap();
        assert!(same_timing(&kept, PRESENCE));
        for other in [
            PRESENCE.replace("most_a_day = 2", "most_a_day = 3"),
            PRESENCE.replace("cooldown = \"1h\"", "cooldown = \"2h\""),
            PRESENCE.replace("to = \"20:00\"", "to = \"21:00\""),
            PRESENCE.replace("[quiet]\nfrom = \"22:00\"\nto = \"08:00\"\n", ""),
            PRESENCE.replace("room/presence", "humans/faces/tracked"),
        ] {
            assert!(!same_timing(&kept, &other), "{other}");
        }
    }

    /// The first builder, 19 Nov 01:22: a remade skill is made from the
    /// person's own kept request, word for word, and starts exactly as the
    /// old one did: its `[trigger]` (cooldown and all), `[active]`,
    /// `[quiet]` and `most_a_day`.
    #[test]
    fn a_remake_is_given_the_kept_request_and_the_old_trigger_and_timing() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("timing");
        old_skill(&keeping, "water", PRESENCE, "local:1");
        let fake = Fake::default();
        assert_eq!(
            step(&keeping, &fake, &Clock::day_idle()),
            Step::Kept("water".to_owned())
        );
        assert_eq!(*fake.asked.borrow(), vec![request_of("water")]);
        let timing = fake.timings.borrow()[0].clone();
        let given: toml::Table = toml::from_str(&timing).unwrap();
        let old: toml::Table = toml::from_str(PRESENCE).unwrap();
        for key in TIMING {
            assert_eq!(given.get(key), old.get(key), "{key}: {timing}");
        }
        assert!(given.get("do").is_none(), "{timing}");
    }

    /// The first builder, 18 Nov 21:58 and 19 Nov 01:29: nothing is asked.
    /// With `LAMP_MAKE_NO_ASKS` on, a remade skill goes on with its task's
    /// yes recorded, so it runs at its first start with no question; the
    /// line after never says it asks.
    #[test]
    fn a_remade_skill_asks_nothing_and_the_line_after_says_no_ask() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("no-yes");
        old_skill(&keeping, "water", PRESENCE, "local:1");
        crate::make_no_asks::set_here(Some(true));
        let fake = Fake::default();
        let kept = step(&keeping, &fake, &Clock::day_idle());
        crate::make_no_asks::set_here(None);
        assert_eq!(kept, Step::Kept("water".to_owned()));
        assert_eq!(*fake.yes.borrow(), vec!["water".to_owned()]);
        let told = note(&keeping, true, false, None).unwrap();
        assert!(!told.contains("asks you once"), "{told}");
        for count in [1, 3] {
            let names: Vec<String> = (0..count).map(|n| format!("water-{n}")).collect();
            let line = told_line(&names).unwrap();
            assert!(!line.contains("ask"), "{line}");
        }
    }

    /// A clock skill's remake is given its clock as it was, and the form
    /// puts no `[[while]]` guards on a clock (`make_no_cannot::skill_text`).
    #[test]
    fn a_clock_skill_is_remade_on_its_clock() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("clock");
        old_skill_asked(
            &keeping,
            "morning-weather",
            MORNING,
            "local:1",
            Some("every morning at 8, tell me the weather"),
        );
        let fake = Fake::default();
        assert_eq!(
            step(&keeping, &fake, &Clock::day_idle()),
            Step::Kept("morning-weather".to_owned())
        );
        let timing = fake.timings.borrow()[0].clone();
        assert!(same_timing(&timing, MORNING), "{timing}");
        assert!(crate::make_no_cannot::on_the_clock(&timing));
    }

    /// Twelve's P3 on #7820: on a lamp with no agent, no remake is tried,
    /// no try is counted and the old skills stay.
    #[test]
    fn with_no_agent_no_remake_is_tried() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("no-agent");
        old_skill(&keeping, "water", PRESENCE, "local:1");
        let fake = Fake {
            no_agent: true,
            ..Fake::default()
        };
        let mut clock = Clock::day_idle();
        for _ in 0..3 {
            assert_eq!(
                step(&keeping, &fake, &clock),
                Step::Waits("no agent for its task here")
            );
            clock.now_ms += DAY_MS;
        }
        assert!(fake.asked.borrow().is_empty());
        let state = load(&keeping.directory);
        assert!(
            state.failed.is_empty() && state.begun_today == 0,
            "{state:?}"
        );
        assert!(
            asked(&keeping, true, || true, || false)["next"]
                .as_str()
                .unwrap()
                .contains("can't be updated")
        );
        // An agent later: it is remade.
        assert_eq!(
            step(&keeping, &Fake::default(), &clock),
            Step::Kept("water".to_owned())
        );
    }

    /// Twelve's P2 on #7820, one undo store: the owner's "forget
    /// everything" clears the remake's kept old ones and `remake.json`
    /// with `make_no_asks`' keep.
    #[test]
    fn forget_everything_clears_the_remakes_undo_too() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("forget");
        old_skill(&keeping, "water", PRESENCE, "local:1");
        old_skill(&keeping, "welcome", PRESENCE, "local:2");
        let fake = Fake::default();
        step(&keeping, &fake, &Clock::day_idle());
        let water = entry_named(&keeping, "water").remove(0);
        let old_package = water["old"]["package"].as_str().unwrap().to_owned();
        assert!(keeping.directory.join(&old_package).is_file());
        assert!(keeping.directory.join(STATE_FILE).is_file());
        let owners = ["bo".to_owned()];
        // Another person's forget leaves it.
        let told = crate::make_no_asks::forget_if(
            &keeping.directory,
            (json!({"forgotten": true}), true),
            Some("al"),
            &owners,
        );
        assert!(told.0.get("removed_skills_kept").is_none());
        assert!(keeping.directory.join(STATE_FILE).is_file());
        let told = crate::make_no_asks::forget_if(
            &keeping.directory,
            (json!({"forgotten": true}), true),
            Some("bo"),
            &owners,
        );
        assert_eq!(told.0["removed_skills_kept"], "forgotten too");
        assert!(!keeping.directory.join(STATE_FILE).exists());
        assert!(!keeping.directory.join(&old_package).exists());
        assert!(entry_named(&keeping, "water")[0].get("old").is_none());
        // Its new one stays on, and nothing is left to put back.
        assert_eq!(entry_named(&keeping, "water")[0]["on"], true);
        let back = put_back(&keeping, &fake, "water", true, || true, 0);
        assert!(back["next"].as_str().unwrap().contains("no older version"));
    }

    /// Twelve's P2 on #7820: a remade skill removed takes its kept old
    /// package with it, by the made skills' remove and by the no-ask one.
    #[test]
    fn removing_a_remade_skill_drops_its_old_package() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("removed");
        old_skill(&keeping, "water", PRESENCE, "local:1");
        old_skill(&keeping, "welcome", PRESENCE, "local:2");
        let fake = Fake::default();
        let clock = Clock::day_idle();
        step(&keeping, &fake, &clock);
        step(&keeping, &fake, &clock);
        let old_of = |name: &str| {
            entry_named(&keeping, name)[0]["old"]["package"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        let (water_old, welcome_old) = (old_of("water"), old_of("welcome"));
        made_skill::forget_named_checked(&keeping.directory, "water").unwrap();
        assert!(entry_named(&keeping, "water").is_empty());
        assert!(!keeping.directory.join(&water_old).exists());
        assert!(keeping.directory.join(&welcome_old).is_file());
        let welcome = entry_named(&keeping, "welcome").remove(0);
        crate::make_no_asks::remove_at_once(
            &keeping,
            "welcome",
            welcome["sha256"].as_str().unwrap(),
            |_| Ok(()),
        )
        .unwrap();
        assert!(!keeping.directory.join(&welcome_old).exists());
    }

    /// The first builder, 18:23: no ask is ever given; each that qualifies
    /// is queued with no yes and remade.
    #[test]
    fn no_offer_is_ever_given_and_qualifying_skills_are_queued_on_their_own() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("no-ask");
        for name in ["water", "welcome", "hum", "stand-up", "clock-glow"] {
            old_skill(&keeping, name, PRESENCE, "local:1");
        }
        // No line of the owner's is ever given an ask, and nothing waits on
        // one.
        for _ in 0..5 {
            assert_eq!(note(&keeping, true, false, None), None);
        }
        assert_eq!(load(&keeping.directory), State::default());
        // Queued with no yes, and remade.
        let fake = Fake::default();
        assert_eq!(
            step(&keeping, &fake, &Clock::day_idle()),
            Step::Kept("water".to_owned())
        );
        assert_eq!(load(&keeping.directory).queue.len(), 4);
        // A guest's ask, or one nobody spoke, queues nothing more.
        assert!(asked(&keeping, true, || false, || true)["error"].is_string());
        assert!(asked(&keeping, false, || true, || true)["error"].is_string());
    }

    #[test]
    fn one_at_a_time_five_a_day_idle_only_never_at_night() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("caps");
        let names: Vec<String> = (1..=7).map(|n| format!("glow-{n}")).collect();
        for name in &names {
            old_skill(&keeping, name, PRESENCE, "local:1");
        }
        let fake = Fake::default();
        let mut clock = Clock::day_idle();
        clock.quiet = true;
        assert_eq!(
            step(&keeping, &fake, &clock),
            Step::Waits("the quiet hours")
        );
        clock.quiet = false;
        clock.idle = IDLE_FOR - Duration::from_secs(1);
        assert_eq!(
            step(&keeping, &fake, &clock),
            Step::Waits("a conversation not long ago")
        );
        clock.idle = IDLE_FOR;
        assert!(fake.asked.borrow().is_empty());
        // One at a time: a look while a remake runs starts none.
        let fake = Fake {
            inner: Cell::new(Some(false)),
            ..Fake::default()
        };
        assert_eq!(
            step(&keeping, &fake, &clock),
            Step::Kept("glow-1".to_owned())
        );
        assert_eq!(fake.inner.get(), Some(true));
        for _ in 2..=5 {
            assert!(matches!(step(&keeping, &fake, &clock), Step::Kept(_)));
        }
        assert_eq!(
            step(&keeping, &fake, &clock),
            Step::Waits("five remakes today")
        );
        clock.now_ms += DAY_MS;
        assert_eq!(
            step(&keeping, &fake, &clock),
            Step::Kept("glow-6".to_owned())
        );
    }

    #[test]
    fn a_remake_replaces_it_under_its_name_with_made_with_and_off_kept() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("kept");
        let old_sha = old_skill(&keeping, "water", PRESENCE, "local:1");
        let fake = Fake {
            off: BTreeSet::from(["water".to_owned()]),
            ..Fake::default()
        };
        let clock = Clock::day_idle();
        assert_eq!(
            step(&keeping, &fake, &clock),
            Step::Kept("water".to_owned())
        );
        // The skill's own kept words were the request.
        assert_eq!(*fake.asked.borrow(), vec![request_of("water")]);
        let kept = entry_named(&keeping, "water");
        assert_eq!(kept.len(), 1);
        let new = &kept[0];
        assert_eq!(new["made_with"], "hermes");
        assert_eq!(new["remade_from"], old_sha.as_str());
        assert_eq!(new["on"], true);
        assert_eq!(new["old"]["sha256"], old_sha.as_str());
        assert_eq!(*fake.turned.borrow(), vec![("water".to_owned(), true)]);
        // Off before the new one went on: never on between.
        assert_eq!(*fake.order.borrow(), vec!["turn", "put"]);
        // Told once, after.
        let told = note(&keeping, true, false, None).unwrap();
        assert!(told.contains(
            "I updated your water skill so your agent does it; it still starts when it did. Say 'put my old water skill back' to undo."
        ));
        assert_eq!(note(&keeping, true, false, None), None);
    }

    #[test]
    fn a_failed_remake_keeps_the_old_skill_untouched_and_tries_once_more_another_day() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("failed");
        old_skill(&keeping, "water", PRESENCE, "local:1");
        let before = fs::read(keeping.directory.join(made_skill::INDEX)).unwrap();
        let fake = Fake {
            fail: true,
            ..Fake::default()
        };
        let mut clock = Clock::day_idle();
        assert_eq!(
            step(&keeping, &fake, &clock),
            Step::NotKept("its task wasn't kept")
        );
        assert_eq!(
            fs::read(keeping.directory.join(made_skill::INDEX)).unwrap(),
            before
        );
        assert!(fake.put.borrow().is_empty());
        assert_eq!(
            step(&keeping, &fake, &clock),
            Step::Waits("each tried today")
        );
        clock.now_ms += DAY_MS;
        assert!(matches!(step(&keeping, &fake, &clock), Step::NotKept(_)));
        clock.now_ms += DAY_MS;
        assert_eq!(step(&keeping, &fake, &clock), Step::Nothing);
        assert_eq!(fake.asked.borrow().len(), 2);
    }

    /// The middle form (19:50): its only code is the lamp's own agent
    /// script, so it goes on with no yes recorded (its task asks when it
    /// first fires). A remake with any other code is never put on, and the
    /// old skill stays exactly as it was.
    #[test]
    fn only_the_lamps_own_task_script_goes_on_any_other_code_is_never_put_on() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("own-code");
        let old_sha = old_skill(&keeping, "water", PRESENCE, "local:1");
        let fake = Fake {
            own_code: true,
            ..Fake::default()
        };
        assert_eq!(
            step(&keeping, &fake, &Clock::day_idle()),
            Step::NotKept("it has code of its own, which waits for a yes")
        );
        assert!(fake.put.borrow().is_empty());
        let water = entry_named(&keeping, "water");
        assert_eq!(water.len(), 1);
        assert_eq!(water[0]["sha256"], old_sha.as_str());
        assert_eq!(water[0]["on"], true);
        // The middle form: put on, its task the agent's.
        let (_root, keeping) = lamp("task-script");
        old_skill(&keeping, "water", PRESENCE, "local:1");
        let fake = Fake::default();
        assert_eq!(
            step(&keeping, &fake, &Clock::day_idle()),
            Step::Kept("water".to_owned())
        );
        let new = entry_named(&keeping, "water").remove(0);
        assert_eq!(new["uses"]["agent_task"], true);
        assert_eq!(new["on"], true);
        assert_eq!(fake.put.borrow().len(), 1);
    }

    #[test]
    fn put_back_within_a_week_restores_it_and_after_a_week_the_old_is_deleted() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("undo");
        let old_sha = old_skill(&keeping, "water", PRESENCE, "local:1");
        old_skill(&keeping, "welcome", PRESENCE, "local:2");
        let fake = Fake::default();
        let mut clock = Clock::day_idle();
        step(&keeping, &fake, &clock);
        step(&keeping, &fake, &clock);
        // A guest's ask puts nothing back.
        let guest = put_back(&keeping, &fake, "water", true, || false, clock.now_ms);
        assert!(guest["error"].is_string());
        let back = put_back(
            &keeping,
            &fake,
            "my old water skill",
            true,
            || true,
            clock.now_ms + DAY_MS,
        );
        assert!(back["next"].as_str().unwrap().contains("is back"), "{back}");
        let water = entry_named(&keeping, "water");
        assert_eq!(water.len(), 1);
        assert_eq!(water[0]["sha256"], old_sha.as_str());
        assert!(water[0].get("old").is_none() && water[0].get("made_with").is_none());
        assert_eq!(fake.put.borrow().last().unwrap(), &old_sha);
        // After seven days the other's old one is gone, package and all.
        let welcome = entry_named(&keeping, "welcome").remove(0);
        let old_package = welcome["old"]["package"].as_str().unwrap().to_owned();
        assert!(keeping.directory.join(&old_package).is_file());
        clock.now_ms += UNDO_FOR.as_millis() as u64;
        step(&keeping, &fake, &clock);
        assert!(entry_named(&keeping, "welcome")[0].get("old").is_none());
        assert!(!keeping.directory.join(&old_package).exists());
        let late = put_back(&keeping, &fake, "welcome", true, || true, clock.now_ms);
        assert!(late["next"].as_str().unwrap().contains("no older version"));
        // One put back is never remade on its own again, only on the
        // person's ask.
        clock.now_ms += DAY_MS;
        assert_eq!(step(&keeping, &fake, &clock), Step::Nothing);
        asked(&keeping, true, || true, || true);
        assert_eq!(load(&keeping.directory).queue, vec!["water".to_owned()]);
    }

    #[test]
    fn with_the_switch_off_nothing_happens() {
        assert!(!on_with(None) && !on_with(Some("yes")) && !on_with(Some("ON")));
        assert!(on_with(Some("on")));
        // A real folder with one remade and untold: off, no note and
        // nothing marked; on, the line.
        let (_root, keeping) = lamp("off");
        old_skill(&keeping, "water", PRESENCE, "local:1");
        let untold = State {
            untold: vec!["water".to_owned()],
            ..State::default()
        };
        save(&keeping.directory, &untold).unwrap();
        assert_eq!(note_for_in(false, &keeping, 1, true), None);
        assert_eq!(load(&keeping.directory), untold);
        assert!(note_for_in(true, &keeping, 2, true).is_some());
        // Off in tests' environment: the make_skill function offers
        // neither argument.
        let tool = crate::make_skill_local::tool();
        let properties = &tool.parameters.unwrap()["properties"];
        assert!(properties.get(REMAKE_OLD).is_none() && properties.get(PUT_BACK).is_none());
    }

    /// The one line after, at the owner's next idle line, in the owner's
    /// language: never to a guest, never while something else waits.
    #[test]
    fn the_one_line_after_is_told_once_in_the_owners_language() {
        let (_root, keeping) = lamp("vi");
        for name in ["water", "welcome", "hum"] {
            old_skill(&keeping, name, PRESENCE, "local:1");
        }
        let untold = State {
            untold: vec!["water".to_owned(), "welcome".to_owned(), "hum".to_owned()],
            ..State::default()
        };
        save(&keeping.directory, &untold).unwrap();
        let vi = Some(crate::language::Language::Vietnamese);
        assert_eq!(note(&keeping, false, false, vi), None);
        assert_eq!(note(&keeping, true, true, vi), None);
        let told = note(&keeping, true, false, vi).unwrap();
        assert!(told.contains("in Vietnamese"), "{told}");
        assert!(told.contains(
            "I updated 3 of your skills so your agent does them; each still starts when it did. Say 'put my old water skill back' to undo."
        ));
        assert_eq!(note(&keeping, true, false, vi), None);
        save(&keeping.directory, &untold).unwrap();
        let en = note(&keeping, true, false, None).unwrap();
        assert!(en.contains("in the conversation's language"), "{en}");
    }

    /// #7765's HOLD, point 3, kept: with nothing to queue, the scan runs
    /// again only once the index changed. Its cost on 31 made skills,
    /// printed.
    #[test]
    fn nothing_to_queue_is_kept_until_the_index_changes() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("nothing");
        for n in 0..30 {
            old_skill(&keeping, &format!("pat-{n}"), QUICK, "local:1");
        }
        // One to remake but for its card, which isn't marked made here.
        old_skill(&keeping, "water", PRESENCE, "local:2");
        let card = keeping.installed.join("water").join("SKILL.md");
        fs::write(&card, "---\nname: water\ndescription: d\n---\n").unwrap();
        let (fake, clock) = (Fake::default(), Clock::day_idle());
        let began = std::time::Instant::now();
        assert_eq!(step(&keeping, &fake, &clock), Step::Nothing);
        let scanned = began.elapsed();
        // Its card fixed, the index the same: no scan, still nothing.
        fs::write(
            &card,
            "---\nname: water\ndescription: d\nmade: here\nmade_by: voice\n---\n",
        )
        .unwrap();
        let began = std::time::Instant::now();
        assert_eq!(step(&keeping, &fake, &clock), Step::Nothing);
        let kept = began.elapsed();
        eprintln!("REMAKE_SCAN_COST 31 skills: scanned {scanned:?}, nothing new {kept:?}");
        // The index changed: scanned again, queued and remade.
        let mut entries = made_skill::entries(&keeping.directory);
        entries[0]["seen"] = json!(true);
        made_skill::write_entries(&keeping.directory, &entries).unwrap();
        assert_eq!(
            step(&keeping, &fake, &clock),
            Step::Kept("water".to_owned())
        );
    }

    /// #7765's HOLD, point 4: a person's make during a remake: the remake
    /// keeps nothing, the old skill stays, and it is no try of its own.
    #[test]
    fn a_persons_make_during_a_remake_makes_it_stand_down() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("stand-down");
        let old_sha = old_skill(&keeping, "water", PRESENCE, "local:1");
        // No remake running: nothing stands down.
        assert!(!stand_down());
        let fake = Fake {
            person: true,
            ..Fake::default()
        };
        let mut clock = Clock::day_idle();
        assert_eq!(step(&keeping, &fake, &clock), Step::NotKept(STOOD_DOWN_WHY));
        assert!(fake.put.borrow().is_empty());
        let water = entry_named(&keeping, "water");
        assert_eq!(water.len(), 1);
        assert_eq!(water[0]["sha256"], old_sha.as_str());
        assert_eq!(load(&keeping.directory).failed["water"].times, 0);
        // Another day, with no person's make, it is remade.
        clock.now_ms += DAY_MS;
        assert_eq!(
            step(&keeping, &Fake::default(), &clock),
            Step::Kept("water".to_owned())
        );
    }

    /// #7765's HOLD, point 4: lamp-tasks' one make slot held by a remake,
    /// a person's make is never told busy: the remake stands down and the
    /// person's make waits its turn.
    #[test]
    fn a_persons_make_waits_for_the_slot_a_remake_holds() {
        use std::io::{BufRead, BufReader, Write};
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let dir = std::path::PathBuf::from(format!("/tmp/mrk-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(
            dir.join(crate::make_skill_agent::MAKE_SOCKET_NAME),
        )
        .unwrap();
        // Busy twice, as while the remake's run ends, then a make.
        std::thread::spawn(move || {
            for (n, stream) in listener.incoming().enumerate() {
                let Ok(mut stream) = stream else { return };
                let mut line = String::new();
                let _ = BufReader::new(&stream).read_line(&mut line);
                let answer = if n < 2 {
                    json!({"ok": false, "error": "busy", "message": "busy"})
                } else {
                    json!({"ok": true, "answer": "made"})
                };
                let _ = writeln!(stream, "{answer}");
            }
        });
        let agent = crate::make_skill_agent::Agent {
            tasks: dir.clone(),
            say: None,
            within: Duration::from_secs(5),
            make_within: Duration::from_secs(10),
            guide: false,
            one_path: false,
            repairs_if_untaken: false,
        };
        let one = One::take().unwrap();
        assert_eq!(agent.write("S", "B", false).unwrap(), "made");
        assert!(STOOD_DOWN.load(Ordering::Acquire));
        drop(one);
        let _ = fs::remove_dir_all(&dir);
    }
}
