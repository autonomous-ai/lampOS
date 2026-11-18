//! Old skills remade in the middle form (the first builder's yes on #6811,
//! 18 Nov 16:40, and his 19:50 ruling): with `LAMP_REMAKE_OLD_SKILLS=on`
//! ([`ENVIRONMENT`], on only when exactly `on`, in no unit), a made skill
//! DeepSeek made before the route (its `made_with` missing or `deepseek`)
//! that is not a quick reaction (`made_skill::quick_reaction`; a quick
//! reaction is left alone) is made again as a new make judged no quick
//! reaction is: the trigger the lamp's own model picks, one task for the
//! lamp's agent whose words are the skill's own request, and sip-of-water's
//! guards, so it holds back while the person talks
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
//!   `skill.toml` (`toml_sha256`).
//! - **Remade without asking** (the first builder, 18 Nov 18:23, in place of
//!   16:40's one ask): with the switch on, each one that qualifies is queued
//!   on its own ([`step`]). The person's own "remake my old skills" comes
//!   back as `make_skill`'s [`REMAKE_OLD`] [`ASKED`] ([`asked`]): it queues
//!   them again, one put back or out of tries too.
//! - **Remade one at a time** ([`step`]), only while idle (no line heard for
//!   [`IDLE_FOR`]), never in the made skills' quiet hours
//!   (`made_skill::MANNERS_QUIET`), at most [`MOST_A_DAY`] a day, never once
//!   the relay's day is 80% used ([`When::relay_day_mostly_used`]). Each
//!   makes the skill's own kept `description`, the lamp's code-written
//!   words, its one task; the new one replaces the old under the same name,
//!   its on/off state kept, only once it was kept (the same checks a new
//!   make passes) and put on. Its only code is the lamp's own agent script
//!   (`made_skill::Uses::agent_task`), so it goes on with no yes recorded:
//!   its task runs only once the person agrees, asked when it first fires
//!   (lamp-skills' `task_yes`, kept by lamp-control); any other code is
//!   never put on here. It records `made_with` (the agent that does its
//!   task) and `remade_from` (the old package's SHA-256). A failed one leaves the old
//!   exactly as it was, is journaled with a short reason and no word of the
//!   skill, and is tried at most once more, on another day.
//! - **Undo for a week** ([`put_back`]): the old entry is kept under the new
//!   one's `old` key with `replaced_at_ms`, its package with it;
//!   `make_skill`'s [`PUT_BACK`] with a name ("put my old water skill
//!   back") puts it back on. After [`UNDO_FOR`] the old package is deleted
//!   ([`drop_old`]).
//! - **Told once after** ([`told_line`], [`note`]): on the owner's next heard
//!   line with nothing else waiting, in the lamp's or the conversation's
//!   language: "I updated 3 of your skills so they wait while you talk;
//!   each asks you once before it first runs. Say 'put my old water skill
//!   back' to undo."
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

#[cfg(test)]
std::thread_local! {
    /// [`ENVIRONMENT`] as a test of this thread sets it.
    static ON_HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether the switch is on in this process's environment.
pub fn on() -> bool {
    #[cfg(test)]
    if let Some(on) = ON_HERE.with(std::cell::Cell::get) {
        return on;
    }
    on_with(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// [`on`] for this test's thread alone.
#[cfg(test)]
pub(crate) fn set_on_here(on: Option<bool>) {
    ON_HERE.with(|here| here.set(on));
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

/// Whether the kept made skill `entry` is one to remake.
fn picked(keeping: &Keeping, entry: &Value) -> bool {
    let Some(name) = entry["name"].as_str().filter(|name| !name.is_empty()) else {
        return false;
    };
    let from = entry["from"].as_str().unwrap_or_default();
    // Made here, by voice or in the app: never an upload or a Store skill.
    if entry["on"] != true
        || entry.get("added").is_some()
        || entry.get("old").is_some()
        || !(from.starts_with("local:") || from.starts_with("written:"))
    {
        return false;
    }
    let folder = keeping.installed.join(name);
    let Ok(card) = fs::read_to_string(folder.join("SKILL.md")) else {
        return false;
    };
    // A core skill or a Store skill of that name: never the person's.
    if !card.lines().any(|line| line.trim() == "made: here") {
        return false;
    }
    if made_with(entry, &card) != "deepseek" {
        return false;
    }
    // Changed since it was made: another package, or another `skill.toml`.
    let sha256 = entry["sha256"].as_str().unwrap_or_default();
    if made_skill::went_on_as(keeping, name) != Ok(Some(sha256.to_owned())) {
        return false;
    }
    let Ok(toml) = fs::read_to_string(folder.join("skill.toml")) else {
        return false;
    };
    if entry["uses"]["code"] != true && entry["toml_sha256"] != hex_of(toml.as_bytes()).as_str() {
        return false;
    }
    !made_skill::quick_reaction(&toml)
}

/// The kept made skills to remake, oldest first.
pub fn chosen(keeping: &Keeping) -> Vec<Value> {
    made_skill::entries(&keeping.directory)
        .into_iter()
        .filter(|entry| picked(keeping, entry))
        .collect()
}

/// The names of [`chosen`].
pub fn chosen_names(keeping: &Keeping) -> Vec<String> {
    chosen(keeping)
        .iter()
        .filter_map(|entry| entry["name"].as_str().map(str::to_owned))
        .collect()
}

/// The one line once they are remade: the real count and one real name.
pub fn told_line(names: &[String]) -> Option<String> {
    let first = made_skill::spoken_name(names.first()?);
    Some(match names.len() {
        1 => format!(
            "I updated your {first} skill so it waits while you talk; it asks you once before it first runs. Say 'put my old {first} skill back' to undo."
        ),
        count => format!(
            "I updated {count} of your skills so they wait while you talk; each asks you once before it first runs. Say 'put my old {first} skill back' to undo."
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
/// What the model is told.
pub fn asked(keeping: &Keeping, heard: bool, owner_hears: impl FnOnce() -> bool) -> Value {
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
    /// Whether the relay's day is 80% used (#7738's level).
    fn relay_day_mostly_used(&self) -> bool;
}

/// What a remake does on the lamp: the lamp's own, or a test's.
pub trait Hands {
    /// `request` made in the middle form, one task for the lamp's agent on
    /// the trigger the lamp's own model picks, with sip-of-water's guards,
    /// kept under `name` and not on; and the agent that does its task, as
    /// `made_with` says it.
    fn make(&self, keeping: &Keeping, request: &str, name: &str) -> Result<(Made, String), String>;
    /// The kept skill `sha256` put on the lamp.
    fn put_on(&self, keeping: &Keeping, sha256: &str) -> Result<(), String>;
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

/// Whether the remake running on this thread stood down for a person's
/// make: its make sends nothing more (`make_skill_agent::Agent::write`).
/// Never for a make on any other thread.
pub fn stood_down() -> bool {
    STEPPING.with(std::cell::Cell::get) && STOOD_DOWN.load(Ordering::Acquire)
}

/// lamp-tasks cancelled the remake's make running here for a person's
/// make (from this process or another of lamp-voice's): it stands down,
/// as for [`stand_down`].
pub fn cancelled_for_a_person() {
    STOOD_DOWN.store(true, Ordering::Release);
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
            STEPPING.with(|here| here.set(true));
        }
        one
    }
}

impl Drop for One {
    fn drop(&mut self) {
        STEPPING.with(|here| here.set(false));
        REMAKING.store(false, Ordering::Release);
    }
}

std::thread_local! {
    /// Whether this thread holds the one remake ([`One`]).
    static STEPPING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
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
    if when.relay_day_mostly_used() {
        return Step::Waits("the relay's day mostly used");
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
    let request = old["description"].as_str().unwrap_or_default();
    if request.is_empty() {
        return Err("it has no description to make it from");
    }
    let was_off = hands.turned_off().contains(name);
    let made = hands.make(keeping, request, name);
    // A person's make came meanwhile: nothing of this one is kept.
    let stood_down = STOOD_DOWN.load(Ordering::Acquire);
    let (made, engine) = made.map_err(|why| {
        if stood_down || why == made_skill::AGENT_BUSY_TRY {
            STOOD_DOWN_WHY
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
    // (`Uses::agent_task`): it goes on with no yes recorded, so its task
    // runs only once the person agrees, asked when it first fires
    // (lamp-skills' `task_yes`, kept by lamp-control). Any other code waits
    // for a spoken yes: never put on here.
    if made.uses.code && !made.uses.agent_task {
        return unput("it has code of its own, which waits for a yes");
    }
    // Off by name before the new one goes on (lamp-skills' `off` file,
    // which an install keeps), so one that was off is never on between.
    if was_off && hands.turn(keeping, name, true).is_err() {
        return unput("it couldn't be kept off as it was");
    }
    if hands.put_on(keeping, &made.sha256).is_err() {
        return unput("it couldn't go on");
    }
    // The old entry as it was chosen, so its undo is always kept.
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
    if let Err(error) = made_skill::write_entries(&keeping.directory, &entries) {
        eprintln!("lamp-voice: a made skill's remake: on, but its old one not marked: {error}");
    }
    Ok(())
}

/// The old ones kept past [`UNDO_FOR`] dropped, their packages with them
/// unless another entry names one.
pub fn drop_old(keeping: &Keeping, now_ms: u64) {
    let mut entries = made_skill::entries(&keeping.directory);
    let mut gone = Vec::new();
    for entry in &mut entries {
        let at = entry["old"]["replaced_at_ms"].as_u64();
        if at.is_some_and(|at| now_ms >= at + UNDO_FOR.as_millis() as u64)
            && let Some(old) = entry.as_object_mut().and_then(|entry| entry.remove("old"))
        {
            gone.push(old);
        }
    }
    if gone.is_empty() {
        return;
    }
    if let Err(error) = made_skill::write_entries(&keeping.directory, &entries) {
        eprintln!("lamp-voice: a remade skill's old one wasn't dropped: {error}");
        return;
    }
    for old in gone {
        if let Some(package) = old["package"].as_str().filter(|package| {
            !package.is_empty()
                && !package.contains('/')
                && !package.starts_with('.')
                && !entries.iter().any(|entry| entry["package"] == *package)
        }) {
            let _ = fs::remove_file(keeping.directory.join(package));
        }
    }
    eprintln!("lamp-voice: a remade skill's old one is past its week: deleted");
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
    let mut entries = made_skill::entries(&keeping.directory);
    let Some(at) = entries.iter().position(|entry| {
        entry.get("old").is_some()
            && entry["name"]
                .as_str()
                .is_some_and(|name| made_skill::name_digits(name) == asked)
    }) else {
        return json!({
            "next": "Say plainly, in a few words, that there is no older version of that skill \
                     to put back. Call nothing."
        });
    };
    let new = entries[at].clone();
    let name = new["name"].as_str().unwrap_or_default().to_owned();
    let mut old = new["old"].clone();
    let was_off = old["was_off"] == true;
    if let Some(old) = old.as_object_mut() {
        old.remove("replaced_at_ms");
        old.remove("was_off");
        old.insert("on".into(), json!(false));
    }
    entries[at] = old.clone();
    if made_skill::write_entries(&keeping.directory, &entries).is_err() {
        return crate::tools::unavailable();
    }
    let old_sha = old["sha256"].as_str().unwrap_or_default();
    if hands.put_on(keeping, old_sha).is_err() {
        // Back as it was: the new one stays on.
        entries[at] = new;
        let _ = made_skill::write_entries(&keeping.directory, &entries);
        eprintln!("lamp-voice: a remade skill's old one couldn't go back on");
        return json!({
            "next": "Say plainly, in a few words, that the old one couldn't be put back just now, \
                     and that the new one stays. Call nothing."
        });
    }
    if was_off && hands.turn(keeping, &name, true).is_err() {
        eprintln!("lamp-voice: a remade skill's old one is back on, but not turned off as it was");
    }
    if let Some(package) = new["package"]
        .as_str()
        .filter(|package| !package.is_empty() && !package.contains('/'))
        .filter(|package| entries.iter().all(|entry| entry["package"] != *package))
    {
        let _ = fs::remove_file(keeping.directory.join(package));
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

    /// The seam for #7738's level: no reader of the relay's day is here
    /// yet, so this is never known to be used up.
    ///
    /// rebase: read #7738's level here once it lands.
    fn relay_day_mostly_used(&self) -> bool {
        false
    }
}

/// The lamp's own [`Hands`].
struct Lamp;

impl Hands for Lamp {
    fn make(&self, keeping: &Keeping, request: &str, name: &str) -> Result<(Made, String), String> {
        // The middle form (19:50): written by the lamp, its trigger picked
        // by the lamp's own model; the agent only does its task when it
        // fires, so one must be on the lamp, but it is never asked to
        // write it, and lamp-tasks' make slot is never taken.
        let agent =
            crate::make_skill_agent::Agent::for_voice().ok_or("no agent for its task here")?;
        let engine = agent
            .lamp_agent()
            .0
            .and_then(|engine| agent.takes(engine))
            .ok_or("the lamp's agent can't do its task")?;
        let _as = made_skill::RemakeAs::set(name);
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
    asked(&keeping, heard, || {
        crate::services::owner_hears(request, heard, lease)
    })
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

    const PRESENCE: &str = "kind = \"installable\"\nversion = \"1.0.0\"\n\n[trigger]\n\
        topic = \"room/presence\"\nfield = \"present\"\nequals = true\n\n[[do]]\n\
        light = { colour = \"warm\" }\n";
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

    /// A made skill on the lamp, as kept before the route.
    fn old_skill(keeping: &Keeping, name: &str, toml: &str, from: &str) -> String {
        let (sha, package) = package_of(keeping, name, &format!("old {name}"));
        install(
            keeping,
            name,
            toml,
            &sha,
            &format!("---\nname: {name}\ndescription: d\nmade: here\nmade_by: voice\n---\n"),
        );
        push_entry(
            keeping,
            json!({
                "from": from, "name": name, "version": "1.0.0", "sha256": sha,
                "toml_sha256": hex_of(toml.as_bytes()),
                "description": format!("When you come back, {name} glows warm."),
                "uses": {"light": true}, "package": package, "on": true,
            }),
        );
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
    }

    impl Hands for Fake {
        fn make(
            &self,
            keeping: &Keeping,
            request: &str,
            name: &str,
        ) -> Result<(Made, String), String> {
            self.asked.borrow_mut().push(request.to_owned());
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
        relay: bool,
    }

    impl Clock {
        fn day_idle() -> Self {
            Self {
                now_ms: 100 * DAY_MS + 10 * HOUR_MS,
                quiet: false,
                idle: IDLE_FOR,
                relay: false,
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
        fn relay_day_mostly_used(&self) -> bool {
            self.relay
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
            PRESENCE.replace("warm", "blue"),
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
        assert!(asked(&keeping, true, || false)["error"].is_string());
        assert!(asked(&keeping, false, || true)["error"].is_string());
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
        clock.relay = true;
        assert_eq!(
            step(&keeping, &fake, &clock),
            Step::Waits("the relay's day mostly used")
        );
        clock.relay = false;
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
        assert_eq!(
            *fake.asked.borrow(),
            vec!["When you come back, water glows warm.".to_owned()]
        );
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
            "I updated your water skill so it waits while you talk; it asks you once before it first runs. Say 'put my old water skill back' to undo."
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
        asked(&keeping, true, || true);
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
            "I updated 3 of your skills so they wait while you talk; each asks you once before it first runs. Say 'put my old water skill back' to undo."
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

    /// A scripted lamp-tasks make socket in a folder of its own: each line
    /// it is sent kept with when it came; a make answered by `make` (its
    /// number among the makes), a cancel by `cancel`.
    struct Scripted {
        dir: PathBuf,
        sent: std::sync::Arc<Mutex<Vec<(Value, std::time::Instant)>>>,
    }

    impl Drop for Scripted {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    impl Scripted {
        fn agent(&self, within: Duration) -> crate::make_skill_agent::Agent {
            crate::make_skill_agent::Agent {
                tasks: self.dir.clone(),
                say: None,
                within,
                make_within: Duration::from_secs(10),
                guide: false,
                one_path: false,
                repairs_if_untaken: false,
            }
        }

        fn ops(&self) -> Vec<String> {
            let sent = self.sent.lock().unwrap();
            sent.iter()
                .map(|(line, _)| line["op"].as_str().unwrap_or("?").to_owned())
                .collect()
        }
    }

    fn scripted(
        name: &str,
        make: impl Fn(usize) -> Value + Send + 'static,
        cancel: Value,
    ) -> Scripted {
        use std::io::{BufRead, BufReader, Write};
        let dir = PathBuf::from(format!("/tmp/mrc-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(
            dir.join(crate::make_skill_agent::MAKE_SOCKET_NAME),
        )
        .unwrap();
        let sent = std::sync::Arc::new(Mutex::new(Vec::new()));
        let kept = sent.clone();
        std::thread::spawn(move || {
            let mut makes = 0;
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut line = String::new();
                let _ = BufReader::new(&stream).read_line(&mut line);
                let line: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
                kept.lock()
                    .unwrap()
                    .push((line.clone(), std::time::Instant::now()));
                let cancels = line["op"] == "skill_make_cancel";
                let answer = if cancels {
                    cancel.clone()
                } else {
                    makes += 1;
                    make(makes)
                };
                if answer["ok"] == true && !cancels {
                    let _ = writeln!(stream, "{}", json!({"ok": true, "started": true}));
                }
                let _ = writeln!(stream, "{answer}");
            }
        });
        Scripted { dir, sent }
    }

    fn busy() -> Value {
        json!({"ok": false, "error": "busy", "message": "busy"})
    }

    /// [`ENVIRONMENT`] as this test's thread has it, until dropped.
    struct SwitchHere;

    impl SwitchHere {
        fn set(on: bool) -> Self {
            set_on_here(Some(on));
            Self
        }
    }

    impl Drop for SwitchHere {
        fn drop(&mut self) {
            set_on_here(None);
        }
    }

    /// With the switch off a person's make refused busy is as before the
    /// remake: no cancel is sent, so no cancel line is written either.
    #[test]
    fn with_the_switch_off_a_busy_make_sends_no_cancel() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let _off = SwitchHere::set(false);
        let tasks = scripted(
            "off",
            |n| match n {
                1 => busy(),
                _ => json!({"ok": true, "answer": "made"}),
            },
            json!({"ok": true, "cancelled": true}),
        );
        let agent = tasks.agent(Duration::from_secs(5));
        let why = agent.write("S", "B", false).unwrap_err().to_string();
        assert_eq!(
            crate::make_skill_agent::not_made_try(&why),
            made_skill::AGENT_BUSY_TRY
        );
        assert_eq!(tasks.ops(), ["skill_make"]);
    }

    /// The reviewer's follow-up on #7765: a person's make refused busy
    /// while a remake runs cancels the remake's make and is sent again at
    /// once, never waiting for the remake's run to end.
    #[test]
    fn a_persons_make_during_a_remake_cancels_it_and_starts_at_once() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let _on = SwitchHere::set(true);
        let tasks = scripted(
            "cancel",
            |n| match n {
                1 => busy(),
                _ => json!({"ok": true, "answer": "made"}),
            },
            json!({"ok": true, "cancelled": true}),
        );
        let agent = tasks.agent(Duration::from_secs(5));
        let one = One::take().unwrap();
        let asked = std::time::Instant::now();
        assert_eq!(agent.write("S", "B", false).unwrap(), "made");
        drop(one);
        assert_eq!(
            tasks.ops(),
            ["skill_make", "skill_make_cancel", "skill_make"]
        );
        let sent = tasks.sent.lock().unwrap().clone();
        // A person's make is never marked a remake.
        assert!(sent.iter().all(|(line, _)| line.get("remake").is_none()));
        // Sent again at once, never after a wait of [`BUSY_AGAIN`]'s.
        let start = sent[2].1 - asked;
        eprintln!("a person's make was sent again {start:?} after it was asked");
        assert!(start < Duration::from_millis(400), "{start:?}");
    }

    /// A failed cancel: one waiting line is said first, in the turn that
    /// waits on the make, then the make waits for the slot as before,
    /// within its own deadline; the cancel is sent once.
    #[test]
    fn a_failed_cancel_says_the_waiting_line_then_waits_within_the_deadline() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let _on = SwitchHere::set(true);
        let tasks = scripted(
            "failed",
            |n| match n {
                1..=3 => busy(),
                _ => json!({"ok": true, "answer": "made"}),
            },
            json!({"ok": false, "error": "not_remake", "message": "no"}),
        );
        let agent = tasks.agent(Duration::from_secs(5));
        let request = 7_765_200;
        let mut in_turn = crate::make_skill_agent::waiting_in_turn(request);
        crate::make_skill_agent::set_made_for(request);
        let one = One::take().unwrap();
        let asked = std::time::Instant::now();
        let written = agent.write("S", "B", true);
        crate::make_skill_agent::set_made_for(0);
        drop(one);
        assert_eq!(written.unwrap(), "made");
        assert!(
            asked.elapsed() < Duration::from_secs(5),
            "{:?}",
            asked.elapsed()
        );
        assert_eq!(
            in_turn.due.try_recv(),
            Ok(Some(crate::make_skill_agent::FINISHING_LINE))
        );
        assert_eq!(
            tasks.ops(),
            [
                "skill_make",
                "skill_make_cancel",
                "skill_make",
                "skill_make",
                "skill_make"
            ]
        );
        // A fixed line: said in the conversation's language.
        assert!(crate::language::is_fixed(
            crate::make_skill_agent::FINISHING_LINE
        ));
    }

    /// The wait never runs past the person's make's own deadline: each make
    /// sent again is given only what is left of it, and at the end the
    /// person hears the busy line.
    #[test]
    fn the_retry_never_passes_the_deadline() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let _on = SwitchHere::set(true);
        let tasks = scripted(
            "deadline",
            |_| busy(),
            json!({"ok": false, "error": "nothing", "message": "no"}),
        );
        let within = Duration::from_millis(2_500);
        let agent = tasks.agent(within);
        let one = One::take().unwrap();
        let asked = std::time::Instant::now();
        let why = agent.write("S", "B", false).unwrap_err().to_string();
        let took = asked.elapsed();
        drop(one);
        assert!(took <= within, "{took:?}");
        assert_eq!(
            crate::make_skill_agent::not_made_try(&why),
            made_skill::AGENT_BUSY_TRY
        );
        let sent = tasks.sent.lock().unwrap().clone();
        let makes: Vec<_> = sent
            .iter()
            .filter(|(line, _)| line["op"] == "skill_make")
            .collect();
        assert!(makes.len() >= 2, "{}", makes.len());
        for (line, at) in makes {
            let given = Duration::from_millis(line["within_ms"].as_u64().unwrap());
            assert!(given >= Duration::from_secs(1), "{given:?}");
            // Given what was left as it was sent, a moment before it came.
            let slack = Duration::from_millis(100);
            assert!(
                *at - asked + given <= within + slack,
                "{:?} + {given:?}",
                *at - asked
            );
        }
    }

    /// End to end against lamp-tasks' own make socket: a remake's make
    /// holds the slot; a person's make cancels it and starts within two
    /// seconds of its ask (before: up to the remake's ~90 s run). The
    /// remake keeps nothing and is said as standing down for the person,
    /// so it counts no try.
    #[test]
    fn a_persons_make_starts_within_two_seconds_on_lamp_tasks_own_socket() {
        use lamp_tasks::skill_task::{Runner, SkillRun};
        use std::sync::Arc;
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let _on = SwitchHere::set(true);
        /// The remake's run holds until cancelled; the person's writes
        /// its answer at once, and when it started is kept.
        #[derive(Default)]
        struct Runs {
            remake_started: AtomicBool,
            person_started: Mutex<Option<std::time::Instant>>,
        }
        impl Runner for Runs {
            fn run(&self, run: &SkillRun, _: &dyn Fn()) -> Result<u64, String> {
                let folder = run.start_in.clone().unwrap();
                let rules = fs::read_to_string(folder.join("rules.md")).unwrap();
                if rules.contains("REMAKE") {
                    self.remake_started.store(true, Ordering::Release);
                    while !run.cancelled() {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    return Ok(0);
                }
                *self.person_started.lock().unwrap() = Some(std::time::Instant::now());
                fs::write(folder.join("answer.md"), "made").unwrap();
                Ok(0)
            }
        }
        let dir = PathBuf::from(format!("/tmp/mre-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("work")).unwrap();
        // lamp-tasks binds only in a private folder.
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let runs = Arc::new(Runs::default());
        let stop = Arc::new(AtomicBool::new(false));
        let served = lamp_tasks::skill_make::MakeHandOvers {
            socket: dir.join(crate::make_skill_agent::MAKE_SOCKET_NAME),
            work: dir.join("work"),
            runner: runs.clone(),
            one_path: None,
            caller: |_| true,
            asks: None,
        }
        .start(true, stop.clone())
        .unwrap()
        .unwrap();
        let agent = crate::make_skill_agent::Agent {
            tasks: dir.clone(),
            say: None,
            within: Duration::from_secs(60),
            make_within: Duration::from_secs(120),
            guide: false,
            one_path: false,
            repairs_if_untaken: false,
        };
        let one = One::take().unwrap();
        let remaking = {
            let agent = agent.clone();
            std::thread::spawn(move || {
                let _as = made_skill::RemakeAs::set("water");
                agent.write("S", "REMAKE", false)
            })
        };
        while !runs.remake_started.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let asked = std::time::Instant::now();
        assert_eq!(agent.write("S", "PERSON", false).unwrap(), "made");
        let start = runs.person_started.lock().unwrap().unwrap() - asked;
        eprintln!("with a remake running, a person's make started {start:?} after its ask");
        assert!(start < Duration::from_secs(2), "{start:?}");
        let why = remaking.join().unwrap().unwrap_err().to_string();
        assert_eq!(
            crate::make_skill_agent::not_made_try(&why),
            made_skill::AGENT_BUSY_TRY
        );
        assert!(STOOD_DOWN.load(Ordering::Acquire));
        drop(one);
        stop.store(true, Ordering::Release);
        served.join().unwrap();
        let _ = fs::remove_dir_all(&dir);
    }
}
