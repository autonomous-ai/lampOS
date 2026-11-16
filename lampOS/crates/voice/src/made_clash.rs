//! A skill made by voice that clashes with one on the lamp (#6593): the
//! same trigger (its topic, field and condition) and both using the light,
//! or both moving the head. Two such skills outrank each other's light in
//! turn ("light is outranked by skill:X"), so on lamp-9adf six
//! `glow-*-on-return` skills piled up on one arrival and "glow green when
//! I'm present" showed only some of the time.
//!
//! A clash is found after the made skill is checked and kept, before it is
//! put on ([`found`]), with every skill on the lamp it clashes with, not
//! only the first (lamp-9adf, 13 Nov: with three on one arrival, the two
//! after the first still clashed once it was replaced): nothing is put on,
//! and the model is told the spoken name of every one ([`spoken_list`]: a
//! replace never takes off one it didn't name, Heron's P2 on #6976) and to
//! ask, word for word, whether to replace
//! them, keep them (they may take turns: nothing makes any win yet) or
//! cancel ([`asked`]). Each later heard line on the second route is told the
//! clash's id and question while it waits ([`waiting_note`]), as that
//! route's history keeps no call's answer. Their answer, in a later line
//! the owner said, comes back as
//! `make_skill` with [`CLASH_ID`] and [`CHOICE`] alone ([`chosen`]):
//! replace puts the new one on, then takes each of the others off through
//! the manage helper's own remove (as the app's remove does), all within
//! [`REPLACE_WITHIN`]; keep puts the
//! new one on beside them; cancel puts nothing on and drops the made one.
//!
//! Replace takes a skill off the lamp, so the choice is guarded as a made
//! skill's yes is (`crate::skill_write_yes`, Heron's P2 on #6614):
//! - **The echo hold.** While a clash waits ([`awaited`]), a line made only
//!   of the lamp's own words in its echo tail is held, even in the private
//!   room (`second_provider`), and its request kept here ([`echo_held`]):
//!   no choice in it is taken.
//! - **Said whole first.** Replace or keep only once one of the lamp's
//!   answers, as played and not cut ([`lamp_said`], [`lamp_cut`]), held
//!   the whole question ([`crate::services::read_back_said`]); before that
//!   the model is told to ask it again whole, and the clash waits on.
//! - **Only the route with the echo judge** (`services::sends_here`) takes
//!   a replace or a keep; a cancel changes nothing on the lamp.
use crate::made_skill::{self, Keeping, Made};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The `make_skill` argument that answers a clash: its id, as told.
pub const CLASH_ID: &str = "clash_id";
/// The `make_skill` argument with the person's choice: one of [`CHOICES`].
pub const CHOICE: &str = "choice";
/// What the person may choose.
pub const CHOICES: [&str; 3] = ["replace", "keep", "cancel"];

/// How long a clash waits for the person's choice.
pub const CLASH_WITHIN: Duration = Duration::from_secs(300);

/// How long the manage helper is waited on for a remove or a put-on.
const HELPER_WAIT: Duration = Duration::from_secs(8);

/// How long a replace's removes take, all together (Heron's P2 on #6976:
/// each waited [`HELPER_WAIT`] in turn, so a hung helper and six clashes
/// held the turn about 48 s). A remove is started only while its own
/// [`HELPER_WAIT`] still ends inside it, so the last one ends within it
/// too; one not started is not removed, and is told so ([`NO_TIME`]).
pub const REPLACE_WITHIN: Duration = Duration::from_secs(12);

/// Why a remove not started within [`REPLACE_WITHIN`] wasn't done.
const NO_TIME: &str = "there wasn't time to ask the lamp in this turn";

/// How many of the lamp's answers a clash keeps, each at most
/// [`SAID_MOST`] bytes.
const SAID_KEPT: usize = 6;
const SAID_MOST: usize = 6_000;

/// The keys of a trigger that say when it fires: its topic is matched on
/// its own. Its pace (`for`, `cooldown`, `rearm`, `gap`, `wait`, …) and its
/// window (`during`, `days`) are not: two skills on one arrival clash
/// whatever their cooldown.
const CONDITION_KEYS: [&str; 7] = [
    "field", "when", "equals", "above", "below", "present", "changes",
];

/// What a skill's `skill.toml` starts on and what it uses, as a clash is
/// judged: each trigger's topic and condition ([`CONDITION_KEYS`]), and
/// whether any `[[do]]` step uses the light or the body. Kept with a made
/// skill in its index (`made_skill`'s `"clash"`), so one made here and on
/// is judged even when its folder can't be read.
pub fn signature(toml: &str) -> Value {
    let Ok(table) = toml::from_str::<toml::Table>(toml) else {
        return json!({"triggers": [], "light": false, "body": false});
    };
    let triggers: Vec<Value> = match table.get("trigger") {
        Some(toml::Value::Table(trigger)) => vec![trigger_key(trigger)],
        Some(toml::Value::Array(list)) => list
            .iter()
            .filter_map(toml::Value::as_table)
            .map(trigger_key)
            .collect(),
        _ => Vec::new(),
    };
    let steps: Vec<&toml::Table> = table
        .get("do")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_table)
        .collect();
    json!({
        "triggers": triggers,
        "light": steps.iter().any(|step| step.contains_key("light")),
        "body": steps.iter().any(|step| step.contains_key("body")),
    })
}

fn trigger_key(trigger: &toml::Table) -> Value {
    let mut key = json!({ "topic": trigger.get("topic").and_then(toml::Value::as_str) });
    for name in CONDITION_KEYS {
        if let Some(value) = trigger.get(name) {
            key[name] = serde_json::to_value(value).unwrap_or(Value::Null);
        }
    }
    key
}

/// What two skills share, by their [`signature`]s: "the light", "the
/// head", both; none when no trigger is the same or no output is shared.
pub fn shared(one: &Value, other: &Value) -> Option<&'static str> {
    let triggers = |signature: &Value| -> Vec<Value> {
        signature["triggers"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|trigger| trigger["topic"].is_string())
            .collect()
    };
    let theirs = triggers(other);
    if !triggers(one).iter().any(|trigger| theirs.contains(trigger)) {
        return None;
    }
    let both = |key: &str| one[key] == true && other[key] == true;
    match (both("light"), both("body")) {
        (true, true) => Some("the light and the head"),
        (true, false) => Some("the light"),
        (false, true) => Some("the head"),
        (false, false) => None,
    }
}

/// A skill on the lamp the made one clashes with: its folder's name, and
/// what both use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clash {
    pub name: String,
    pub shares: &'static str,
}

/// Every skill on the lamp that `made`, just kept and not yet on, clashes
/// with ([`shared`]): first those installed (`keeping.installed`, and the
/// skills pushed beside it), each by its `skill.toml`, in folder order;
/// then the made skills kept here and on whose folder isn't there, by the
/// signature kept with them. Empty when nothing clashes, or the made one's
/// signature isn't kept.
pub fn found(keeping: &Keeping, made: &Made) -> Vec<Clash> {
    let entries = made_skill::entries(&keeping.directory);
    let Some(mine) = entries
        .iter()
        .find(|entry| entry["sha256"] == made.sha256.as_str())
        .map(|entry| entry["clash"].clone())
        .filter(Value::is_object)
    else {
        return Vec::new();
    };
    let mut seen = Vec::new();
    let mut clashes = Vec::new();
    for folder in [
        keeping.installed.clone(),
        made_skill::pushed_folder(&keeping.installed),
    ] {
        let mut names: Vec<PathBuf> = std::fs::read_dir(&folder)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        names.sort();
        for path in names {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name.starts_with('.') || name == made.name {
                continue;
            }
            seen.push(name.to_owned());
            // A name in both folders is one skill: named once.
            if clashes.iter().any(|clash: &Clash| clash.name == name) {
                continue;
            }
            let Ok(toml) = std::fs::read_to_string(path.join("skill.toml")) else {
                continue;
            };
            if let Some(shares) = shared(&mine, &signature(&toml)) {
                clashes.push(Clash {
                    name: name.to_owned(),
                    shares,
                });
            }
        }
    }
    for entry in entries
        .iter()
        .filter(|entry| entry["on"] == true && entry["sha256"] != made.sha256.as_str())
    {
        let Some(name) = entry["name"].as_str() else {
            continue;
        };
        if name == made.name
            || seen.iter().any(|seen| seen == name)
            || clashes.iter().any(|clash| clash.name == name)
        {
            continue;
        }
        if let Some(shares) = shared(&mine, &entry["clash"]) {
            clashes.push(Clash {
                name: name.to_owned(),
                shares,
            });
        }
    }
    clashes
}

/// Every one of `names`' spoken names: "a", "a and b", "a, b, c and d".
/// None is left out, however many (Heron's P2 on #6976: "and 2 more" let a
/// replace take off two skills the person never heard named). Two that
/// sound the same are told apart by their folder ([`told_names`]).
pub fn spoken_list(names: &[String]) -> String {
    joined(&told_names(names))
}

/// Each of `names` as said: its spoken name, and, when another of them has
/// the same words (Heron's P3 on #6976: "glow a and glow a"), its folder
/// too, as "glow a (folder glow-a)".
fn told_names(names: &[String]) -> Vec<String> {
    let spoken: Vec<String> = names
        .iter()
        .map(|name| made_skill::spoken_name(name))
        .collect();
    let words: Vec<Vec<String>> = spoken
        .iter()
        .map(|said| crate::services::check_words(said))
        .collect();
    names
        .iter()
        .zip(&spoken)
        .zip(&words)
        .map(|((name, said), heard)| {
            if words.iter().filter(|other| *other == heard).count() > 1 {
                format!("{said} (folder {name})")
            } else {
                said.clone()
            }
        })
        .collect()
}

/// `told` as one spoken list: "a", "a and b", "a, b and c".
fn joined(told: &[String]) -> String {
    match told.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// What the made one shares with `clashes`, all told: the light, the head
/// or both.
fn shares_of(clashes: &[Clash]) -> &'static str {
    let light = clashes.iter().any(|clash| clash.shares.contains("light"));
    let head = clashes.iter().any(|clash| clash.shares.contains("head"));
    match (light, head) {
        (true, true) => "the light and the head",
        (false, true) => "the head",
        _ => "the light",
    }
}

fn names_of(clashes: &[Clash]) -> Vec<String> {
    clashes.iter().map(|clash| clash.name.clone()).collect()
}

/// A clash waiting for the person's choice: one at a time, the newest.
struct Waiting {
    id: String,
    keeping: Keeping,
    sha256: String,
    name: String,
    /// The skills on the lamp it clashes with, by folder name.
    others: Vec<String>,
    /// What the lamp must have said whole, uncut, before a replace or a
    /// keep is taken ([`question`]).
    question: String,
    request: u64,
    at: Instant,
    /// Later requests whose line was held as the lamp's own echo.
    echoes: Vec<u64>,
    /// The lamp's words in each request from the make's on, and whether
    /// that answer was cut.
    said: VecDeque<(u64, String, bool)>,
}

#[cfg(not(test))]
mod kept {
    use super::Waiting;
    use std::sync::Mutex;
    static WAITING: Mutex<Option<Waiting>> = Mutex::new(None);
    pub fn waiting<R>(f: impl FnOnce(&mut Option<Waiting>) -> R) -> R {
        f(&mut WAITING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()))
    }
}

/// Tests: each test thread has its own.
#[cfg(test)]
mod kept {
    use super::Waiting;
    use std::cell::RefCell;
    thread_local! {
        static WAITING: RefCell<Option<Waiting>> = const { RefCell::new(None) };
    }
    pub fn waiting<R>(f: impl FnOnce(&mut Option<Waiting>) -> R) -> R {
        WAITING.with(|cell| f(&mut cell.borrow_mut()))
    }
}

static IDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// The line said for a clash when the turn that made it was cut: the model
/// never had the question to ask, so the person is told it wasn't put on.
pub fn cut_line(made: &Made, clashes: &[Clash]) -> String {
    let new = made_skill::spoken_name(&made.name);
    let others = spoken_list(&names_of(clashes));
    let shares = shares_of(clashes);
    if clashes.len() == 1 {
        format!(
            "I made {new}, but didn't put it on: {others} starts at the same moment and uses \
             {shares}. Ask me again to replace it or to keep both."
        )
    } else {
        format!(
            "I made {new}, but didn't put it on: {others} start at the same moment and use \
             {shares}. Ask me again to replace them or to keep them all."
        )
    }
}

/// The question the lamp asks of a clash, word for word: said whole before
/// a replace or a keep is taken. "Keep both" promises no winner: nothing
/// yet makes the newer one's light come last (Heron's P2 on #6614). With
/// more than one, `others` names every one of them ([`spoken_list`]).
fn question(new: &str, others: &str, many: bool, shares: &str) -> String {
    if many {
        format!(
            "{new} would start at the same moment as {others}, which are on the lamp, and they \
             all use {shares}. Shall I replace them all, keep them all (they may take turns), or \
             cancel?"
        )
    } else {
        format!(
            "{new} would start at the same moment as {others}, which is on the lamp, and both \
             use {shares}. Shall I replace {others}, keep both (they may take turns), or cancel?"
        )
    }
}

/// What the model is told to say of a clash: `question`, word for word.
fn say_whole(question: &str) -> String {
    format!(
        "Nothing is on yet. Say this word for word as written, leaving nothing out: \
         \"{question}\" If the conversation is in another language, say it in that language \
         too, after it."
    )
}

/// What the model is told to do after asking: wait for the answer.
fn then_choose(id: &str) -> String {
    format!(
        "Call nothing now. When they answer, call make_skill with {CLASH_ID} \"{id}\" and \
         {CHOICE} \"replace\", \"keep\" or \"cancel\" alone."
    )
}

/// `made`, kept and not on, clashes with `clashes` (one or more, from
/// [`found`]): it waits for the person's
/// choice, asked in a later line than `request`, at most [`CLASH_WITHIN`]
/// after `now`. A clash waiting before it is let go, its made one dropped
/// ([`made_skill::forget_unput`]). What the model is told: the other
/// skills' spoken names, and the question to ask word for word.
pub fn asked(
    keeping: &Keeping,
    made: &Made,
    clashes: &[Clash],
    request: u64,
    now: Instant,
) -> Value {
    let id = format!(
        "clash-{}",
        IDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let new = made_skill::spoken_name(&made.name);
    let others = names_of(clashes);
    let spoken = spoken_list(&others);
    let shares = shares_of(clashes);
    let question = question(&new, &spoken, clashes.len() > 1, shares);
    let before = kept::waiting(|waiting| {
        waiting.replace(Waiting {
            id: id.clone(),
            keeping: keeping.clone(),
            sha256: made.sha256.clone(),
            name: made.name.clone(),
            others: others.clone(),
            question: question.clone(),
            request,
            at: now,
            echoes: Vec::new(),
            said: VecDeque::new(),
        })
    });
    if let Some(before) = before.filter(|before| before.sha256 != made.sha256) {
        let_go(&before, "a newer clash was asked");
    }
    eprintln!(
        "lamp-voice: request {request}: a made skill {} clashes with {} ({}): not put on, the person is asked",
        made.name,
        others.join(", "),
        shares
    );
    json!({
        "made": made.name,
        "on": false,
        "clashes_with": spoken,
        "shares": shares,
        CLASH_ID: id,
        "next": format!("{} {}", say_whole(&question), then_choose(&id)),
    })
}

/// The note a later heard line `request` is told at `now` of the clash
/// waiting: its id and its fixed question, and how to answer it
/// ([`then_choose`]'s call). The second route's history keeps past
/// exchanges' words, not their calls, so the `clash_id` told only in the
/// make's own answer is gone by the person's next line (the
/// Reviewer-lead's P1 on #6614: "keep both" found no clash_id and was told
/// NO_REQUEST, and the made skill was dropped after [`CLASH_WITHIN`]), as
/// `crate::skill_write_yes::waiting_note` tells a yes's id. None in the
/// make's own request, none waiting, or past [`CLASH_WITHIN`]; the choice
/// keeps every guard of [`chosen`].
pub fn waiting_note(request: u64, now: Instant) -> Option<String> {
    kept::waiting(|waiting| {
        let waiting = waiting.as_ref().filter(|waiting| {
            request > waiting.request && now.saturating_duration_since(waiting.at) <= CLASH_WITHIN
        })?;
        Some(format!(
            "(For you to know, not to answer: the skill {} that the lamp just made is not on: it \
             clashes with {}, and the lamp asked them: \"{}\" It waits for their choice, \
             {CLASH_ID} \"{}\". If they choose, in any words, call make_skill with only that \
             {CLASH_ID} and {CHOICE} \"replace\", \"keep\" or \"cancel\", never a request. If \
             they say anything else, don't call it: nothing is put on.)",
            made_skill::spoken_name(&waiting.name),
            spoken_list(&waiting.others),
            waiting.question,
            waiting.id
        ))
    })
}

/// The made one of a clash cancelled or never chosen: dropped from the
/// index with its package, so it is never told as made and off (Heron's
/// P3 on #6614).
fn let_go(waiting: &Waiting, why: &str) {
    eprintln!(
        "lamp-voice: a made skill's clash: {why}; {} not put on, dropped",
        waiting.name
    );
    made_skill::forget_unput(&waiting.keeping.directory, &waiting.sha256);
}

/// Whether a clash waits for the person's choice at `now`: the echo hold
/// is on (`crate::second_provider`). One past [`CLASH_WITHIN`] is let go
/// here, its made one dropped.
pub fn awaited(now: Instant) -> bool {
    let (waits, expired) = kept::waiting(|waiting| {
        match waiting
            .as_ref()
            .map(|waiting| now.saturating_duration_since(waiting.at) <= CLASH_WITHIN)
        {
            Some(true) => (true, None),
            Some(false) => (false, waiting.take()),
            None => (false, None),
        }
    });
    if let Some(expired) = expired {
        let_go(&expired, "no choice came in time");
    }
    waits
}

/// The line of `request` was held as the lamp's own echo while a clash
/// waited: no choice in it.
pub fn echo_held(request: u64) {
    kept::waiting(|waiting| {
        if let Some(waiting) = waiting.as_mut()
            && request > waiting.request
            && !waiting.echoes.contains(&request)
        {
            waiting.echoes.push(request);
        }
    });
}

/// The lamp said `words` in `request` (`thread::Thread::lamp_for`, as
/// played): kept toward the question while a clash waits, for the make's
/// request and any after it.
pub fn lamp_said(request: u64, words: &str) {
    kept::waiting(|waiting| {
        let Some(waiting) = waiting
            .as_mut()
            .filter(|waiting| request >= waiting.request)
        else {
            return;
        };
        match waiting.said.iter_mut().find(|(id, ..)| *id == request) {
            Some((_, said, _)) => {
                if said.len() + words.len() <= SAID_MOST {
                    said.push_str(words);
                }
            }
            None => {
                if waiting.said.len() == SAID_KEPT {
                    waiting.said.pop_front();
                }
                let words: String = words.chars().take(SAID_MOST).collect();
                waiting.said.push_back((request, words, false));
            }
        }
    });
}

/// The lamp's answer in `request` was cut (`thread::Thread::heard_only`):
/// a question cut is no question.
pub fn lamp_cut(request: u64) {
    kept::waiting(|waiting| {
        let Some(waiting) = waiting
            .as_mut()
            .filter(|waiting| request >= waiting.request)
        else {
            return;
        };
        match waiting.said.iter_mut().find(|(id, ..)| *id == request) {
            Some((_, _, cut)) => *cut = true,
            None => {
                if waiting.said.len() == SAID_KEPT {
                    waiting.said.pop_front();
                }
                waiting.said.push_back((request, String::new(), true));
            }
        }
    });
}

/// Whether one of the lamp's answers before `request`, uncut, held the
/// whole question.
fn said_whole_before(waiting: &Waiting, request: u64) -> bool {
    waiting.said.iter().any(|(id, said, cut)| {
        *id < request && !cut && crate::services::read_back_said(&waiting.question, said)
    })
}

/// Why a remove didn't take a skill off.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotOff {
    /// It is still on: refused, or the helper couldn't be reached.
    Refused(String),
    /// The helper didn't answer in time: it may be off, or still on
    /// (Heron's P3 on #6976: never told as "still on").
    Unsure(String),
}

/// How a choice is carried out: the other skill taken off by name, the
/// new one put on by its SHA-256 (its name once on, or why not), and the
/// clock a replace's removes are timed by ([`REPLACE_WITHIN`]).
pub struct Hands<'a> {
    pub remove: &'a dyn Fn(&Keeping, &str) -> Result<(), NotOff>,
    pub put_on: &'a dyn Fn(&Keeping, &str) -> Result<String, String>,
    pub clock: &'a dyn Fn() -> Instant,
}

/// The product's [`Hands`]: the manage helper's own `{"remove": NAME}`, as
/// the app's remove asks it, [`made_skill::put_on_within`], and the real
/// clock.
pub fn helper_hands() -> Hands<'static> {
    Hands {
        clock: &Instant::now,
        remove: &|keeping, name| remove_said(&keeping.manage, name),
        put_on: &|keeping, sha256| {
            made_skill::put_on_within(
                &keeping.directory,
                &keeping.manage,
                sha256,
                crate::app_make::now_unix_ms(),
                HELPER_WAIT,
            )
        },
    }
}

/// [`remove_said`] in plain words alone, as `crate::made_remove` tells
/// it.
pub(crate) fn remove_by_helper(socket: &Path, name: &str) -> Result<(), String> {
    remove_said(socket, name).map_err(|(NotOff::Refused(why) | NotOff::Unsure(why))| why)
}

/// The manage helper's own `{"remove": NAME}`: off, or why not, and
/// whether it may be off all the same ([`NotOff`]).
fn remove_said(socket: &Path, name: &str) -> Result<(), NotOff> {
    match crate::tools::reached(socket, &json!({ "remove": name }), HELPER_WAIT) {
        None => Err(NotOff::Refused(
            "the lamp's skills couldn't be reached".into(),
        )),
        // Asked, with no answer in time: the helper may have taken it off.
        Some(None) => Err(NotOff::Unsure(
            "the lamp didn't say in time whether it is off".into(),
        )),
        Some(Some(answer)) if answer.get("done").is_some() => Ok(()),
        Some(Some(answer)) => {
            eprintln!(
                "lamp-voice: the skills helper didn't remove {name}: {}",
                answer["refused"].as_str().unwrap_or_default()
            );
            Err(NotOff::Refused(format!(
                "the lamp didn't let {} be removed",
                made_skill::spoken_name(name)
            )))
        }
    }
}

/// A `make_skill` call answering a clash: its `id` and `choice`, in the
/// voice's request `request` (`heard`: someone spoke it), on the route with
/// the echo judge or not (`here`, `services::sends_here`).
pub struct Answer<'a> {
    pub id: &'a str,
    pub choice: Option<&'a str>,
    pub request: u64,
    pub heard: bool,
    pub here: bool,
}

/// What the model is told when a choice isn't the person's own: nothing
/// changes, and the clash waits on.
const NOT_THEIRS: &str = "Say plainly, in a few words, that only the lamp's owner can choose \
                          that, out loud. Call nothing.";

/// What a replace says when not every other skill came off: those it
/// removed, then those not, each with its own reason (Heron's P3 on #6976:
/// not the first one's for all), together when the reason is the same;
/// one the helper didn't answer for in time "may still be on", never
/// "is still on". `others` is every one it asked of, so two that sound the
/// same are told apart as in the question ([`told_names`]).
fn not_all_removed(
    new: &str,
    others: &[String],
    removed: &[String],
    still_on: &[(String, String)],
    unsure: &[(String, String)],
    all_on: &str,
) -> String {
    let told = told_names(others);
    let say = |names: &[&String]| -> String {
        let said: Vec<String> = names
            .iter()
            .map(|name| {
                others
                    .iter()
                    .position(|other| other == *name)
                    .map_or_else(|| made_skill::spoken_name(name), |at| told[at].clone())
            })
            .collect();
        joined(&said)
    };
    fn grouped(failed: &[(String, String)]) -> Vec<(Vec<&String>, &str)> {
        let mut groups: Vec<(Vec<&String>, &str)> = Vec::new();
        for (name, why) in failed {
            match groups.iter_mut().find(|(_, same)| same == why) {
                Some((names, _)) => names.push(name),
                None => groups.push((vec![name], why)),
            }
        }
        groups
    }
    let mut parts = Vec::new();
    for (names, why) in grouped(still_on) {
        let was = if names.len() > 1 { "weren't" } else { "wasn't" };
        parts.push(format!("{} {was} removed: {why}", say(&names)));
    }
    for (names, why) in grouped(unsure) {
        parts.push(format!("{} may still be on: {why}", say(&names)));
    }
    let lead = if removed.is_empty() {
        format!("{new} is on, but")
    } else {
        let removed: Vec<&String> = removed.iter().collect();
        format!("{new} is on, and I removed {}, but", say(&removed))
    };
    let tail = if removed.is_empty() && unsure.is_empty() {
        format!(" {all_on}")
    } else if still_on.is_empty() {
        String::new()
    } else {
        let names: Vec<&String> = still_on.iter().map(|(name, _)| name).collect();
        let are = if names.len() > 1 { "are" } else { "is" };
        format!(" {} {are} still on.", say(&names))
    };
    format!("{lead} {}.{tail}", parts.join("; "))
}

/// The person's choice `answer` at `now`. Only the owner's line answers
/// (`owners`, as for a yes: `LAMP_PRIVATE_ROOM` on, any heard line), only a
/// later heard line than the one that made it, never one held as the
/// lamp's own echo, within [`CLASH_WITHIN`]; a replace or a keep only on
/// the route with the echo judge, once the question was said whole and
/// uncut. A choice refused leaves the clash waiting; one too late lets it
/// go. What the model is told.
pub fn chosen(
    answer: Answer<'_>,
    owners: impl FnOnce() -> bool,
    now: Instant,
    hands: &Hands<'_>,
) -> Value {
    let Answer {
        id,
        choice,
        request,
        heard,
        here,
    } = answer;
    enum Took {
        Nothing,
        Late(Box<Waiting>),
        Refused(Value),
        Taken(Box<Waiting>, &'static str),
    }
    let took = kept::waiting(|kept| {
        let Some(waiting) = kept.as_ref().filter(|waiting| waiting.id == id) else {
            return Took::Nothing;
        };
        if now.saturating_duration_since(waiting.at) > CLASH_WITHIN {
            return kept
                .take()
                .map_or(Took::Nothing, |waiting| Took::Late(Box::new(waiting)));
        }
        let Some(choice) = choice.and_then(|choice| CHOICES.into_iter().find(|c| *c == choice))
        else {
            // Asked again as the question was: of one, or of them all
            // (Heron's P3 on #6976).
            let again = if waiting.others.len() > 1 {
                "replace them all, keep them all, or cancel"
            } else {
                "replace it, keep both, or cancel"
            };
            return Took::Refused(json!({
                "error": "the choice is replace, keep or cancel",
                "next": format!("Ask them again, in a few words: {again}.")
            }));
        };
        let why = if !heard {
            Some("no one spoke it")
        } else if request <= waiting.request {
            Some("it came in the make's own request")
        } else if waiting.echoes.contains(&request) {
            Some("its line was the lamp's own words heard back")
        } else if choice != "cancel" && !here {
            Some("this route has no echo judge")
        } else if !owners() {
            Some("not the owner's own line")
        } else {
            None
        };
        if let Some(why) = why {
            eprintln!("lamp-voice: request {request}: a made skill's clash wasn't answered: {why}");
            return Took::Refused(json!({
                "error": format!("only the lamp's owner can choose, out loud: {why}"),
                "next": NOT_THEIRS
            }));
        }
        if choice != "cancel" && !said_whole_before(waiting, request) {
            eprintln!(
                "lamp-voice: request {request}: a made skill's clash wasn't answered: its question was not heard whole"
            );
            return Took::Refused(json!({
                "error": "not chosen yet: the question was not said in full, or it was cut",
                "next": format!("{} {}", say_whole(&waiting.question), then_choose(&waiting.id)),
            }));
        }
        kept.take().map_or(Took::Nothing, |waiting| {
            Took::Taken(Box::new(waiting), choice)
        })
    });
    let (waiting, choice) = match took {
        Took::Nothing => {
            return json!({
                "error": "no skill waits for that choice now",
                "next": "Say plainly, in a few words, that nothing is waiting for that now. Call nothing."
            });
        }
        Took::Late(waiting) => {
            let_go(&waiting, "the choice came too late");
            return json!({
                "error": "the choice came too late",
                "next": "Say plainly, in a few words, that it took too long and nothing was put on; \
                         they can ask for the skill again. Call nothing."
            });
        }
        Took::Refused(told) => return told,
        Took::Taken(waiting, choice) => (waiting, choice),
    };
    let new = made_skill::spoken_name(&waiting.name);
    let others = spoken_list(&waiting.others);
    let many = waiting.others.len() > 1;
    let (stays, all_on) = if many {
        ("stay as they were", "All are on.")
    } else {
        ("stays as it was", "Both are on.")
    };
    let said = |line: String| made_skill::in_their_language(&line);
    match choice {
        "cancel" => {
            let_go(&waiting, "cancelled");
            json!({
                "made": waiting.name,
                "on": false,
                "cancelled": true,
                "next": said(format!("OK, I didn't put {new} on. {others} {stays}."))
            })
        }
        // The new one on first, then each other off: a put-on that fails
        // takes nothing off (Heron's P3 on #6614).
        "replace" => {
            if let Err(why) = (hands.put_on)(&waiting.keeping, &waiting.sha256) {
                return json!({
                    "error": format!("not put on: {why}"),
                    "next": said(format!(
                        "{} {others} {stays}.",
                        made_skill::not_on_line(&why)
                    ))
                });
            }
            // Each is asked on its own: one refused (a core skill) leaves
            // the others' removes as they were. All of them end within
            // [`REPLACE_WITHIN`]: one whose own wait would end past it is
            // not started, and is told as not removed.
            let deadline = (hands.clock)() + REPLACE_WITHIN;
            let mut removed = Vec::new();
            let mut still_on: Vec<(String, String)> = Vec::new();
            let mut unsure: Vec<(String, String)> = Vec::new();
            for other in &waiting.others {
                let outcome = if (hands.clock)() + HELPER_WAIT > deadline {
                    Err(NotOff::Refused(NO_TIME.to_owned()))
                } else {
                    (hands.remove)(&waiting.keeping, other)
                };
                match outcome {
                    Ok(()) => {
                        made_skill::forget_named(&waiting.keeping.directory, other);
                        removed.push(other.clone());
                    }
                    Err(NotOff::Refused(why)) => {
                        eprintln!(
                            "lamp-voice: request {request}: a made skill's clash: {} on, {other} not removed: {why}",
                            waiting.name
                        );
                        still_on.push((other.clone(), why));
                    }
                    // Kept in `made.json`: it may still be on.
                    Err(NotOff::Unsure(why)) => {
                        eprintln!(
                            "lamp-voice: request {request}: a made skill's clash: {} on, {other} may still be on: {why}",
                            waiting.name
                        );
                        unsure.push((other.clone(), why));
                    }
                }
            }
            if !removed.is_empty() {
                eprintln!(
                    "lamp-voice: request {request}: a made skill's clash: {} removed for {}",
                    removed.join(", "),
                    waiting.name
                );
            }
            if still_on.is_empty() && unsure.is_empty() {
                return json!({
                    "made": waiting.name,
                    "on": true,
                    "replaced": removed,
                    "next": said(format!("Done: {new} is on, and I removed {others}."))
                });
            }
            let line = not_all_removed(&new, &waiting.others, &removed, &still_on, &unsure, all_on);
            let names = |failed: &[(String, String)]| -> Vec<String> {
                failed.iter().map(|(name, _)| name.clone()).collect()
            };
            let mut told = json!({
                "made": waiting.name,
                "on": true,
                "not_removed": names(&still_on),
                "next": said(line)
            });
            if !removed.is_empty() {
                told["replaced"] = json!(removed);
            }
            if !unsure.is_empty() {
                told["may_still_be_on"] = json!(names(&unsure));
            }
            told
        }
        _ => match (hands.put_on)(&waiting.keeping, &waiting.sha256) {
            Ok(_) => {
                eprintln!(
                    "lamp-voice: request {request}: a made skill's clash: {} kept beside {}",
                    waiting.name,
                    waiting.others.join(", ")
                );
                json!({
                    "made": waiting.name,
                    "on": true,
                    "kept_both": true,
                    "next": said(format!("Done: {new} is on beside {others}. {all_on}"))
                })
            }
            Err(why) => json!({
                "error": format!("not put on: {why}"),
                "next": said(made_skill::not_on_line(&why))
            }),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make_skill_local::tests::{chat_at, good, keeping, manage_helper, service};
    use std::sync::Mutex;

    const NOW: u64 = 1_800_000_000_000;

    fn forget_waiting() {
        kept::waiting(Option::take);
    }

    /// A skill on the lamp named `name` on `trigger`, with one `step`.
    fn installed(keeping: &Keeping, name: &str, trigger: &str, step: &str) {
        let folder = keeping.installed.join(name);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("skill.toml"),
            format!(
                "kind = \"installable\"\nversion = \"1.0.0\"\n\n[trigger]\n{trigger}\
                 cooldown = \"1h\"\n\n[[do]]\n{step}\n"
            ),
        )
        .unwrap();
    }

    /// The made test skill's trigger: the voice resting.
    const RESTING: &str = "topic = \"voice/state\"\nfield = \"state\"\nequals = \"resting\"\n";

    fn made(name: &str) -> Made {
        Made {
            name: name.to_owned(),
            version: "1.0.0".to_owned(),
            sha256: "ab".repeat(32),
            description: String::new(),
            uses: Default::default(),
            package: PathBuf::from("/nonexistent"),
        }
    }

    /// Lamp-9adf, 12 Nov: a glow made on the trigger another glow on the
    /// lamp has. The made skill is kept but not put on (no helper is
    /// there to ask), and the model is told the other's spoken name and to
    /// ask replace, keep both or cancel, with no winner promised.
    #[test]
    fn a_clash_is_found_and_nothing_is_put_on() {
        let (root, keeping) = keeping("clash-found");
        installed(
            &keeping,
            "rest-glow",
            RESTING,
            "light = { colour = \"blue\" }",
        );
        let (url, served) = service(vec![(200, good())]);
        let told = crate::make_skill_local::make_with(
            3,
            Some("make me a skill that dims the light at 10 pm"),
            "dim the light at 10 pm",
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(told["on"], false, "{told}");
        assert_eq!(told["clashes_with"], "rest glow", "{told}");
        assert_eq!(told["shares"], "the light", "{told}");
        assert!(told[CLASH_ID].is_string(), "{told}");
        let next = told["next"].as_str().unwrap();
        assert!(next.contains("replace rest glow"), "{next}");
        assert!(next.contains("keep both (they may take turns)"), "{next}");
        assert!(!next.contains("wins"), "no winner is promised: {next}");
        assert!(next.contains("or cancel"), "{next}");
        assert_eq!(served.join().unwrap(), 1);
        let entries = made_skill::entries(&keeping.directory);
        assert_eq!(entries.len(), 1);
        assert_ne!(entries[0]["on"], true, "nothing put on");
        let line = crate::make_skill_local::take_said_line().unwrap();
        assert!(
            line.contains("didn't put it on: rest glow starts at the same moment"),
            "{line}"
        );
        forget_waiting();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// No clash: another trigger, or the same trigger with no output both
    /// use. The made skill goes on as before.
    #[test]
    fn no_clash_on_another_trigger_or_no_shared_output() {
        let (root, keeping) = keeping("clash-none");
        installed(
            &keeping,
            "arrival-glow",
            "topic = \"room/presence\"\nfield = \"someone\"\nequals = true\n",
            "light = { colour = \"blue\" }",
        );
        installed(
            &keeping,
            "rest-nod",
            RESTING,
            "body = { gesture = \"nod\" }",
        );
        let (url, served) = service(vec![(200, good())]);
        let asked = manage_helper(&keeping.manage);
        let told = crate::make_skill_local::make_with(
            3,
            Some("make me a skill that dims the light at 10 pm"),
            "dim the light at 10 pm",
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        assert_eq!(told["on"], true, "{told}");
        assert!(told.get(CLASH_ID).is_none(), "{told}");
        asked.join().unwrap();
        served.join().unwrap();
        let _ = crate::make_skill_local::take_said_line();
        let _ = std::fs::remove_dir_all(&root);

        let light = signature(&format!(
            "[trigger]\n{RESTING}[[do]]\nlight = {{ dim = 1 }}\n"
        ));
        let head = signature(&format!(
            "[trigger]\n{RESTING}[[do]]\nbody = {{ gesture = \"nod\" }}\n"
        ));
        let both = signature(&format!(
            "[[trigger]]\n{RESTING}[[do]]\nlight = {{ dim = 1 }}\nbody = {{ gesture = \"nod\" }}\n"
        ));
        let speaking = signature(&format!(
            "[trigger]\n{}[[do]]\nlight = {{ dim = 1 }}\n",
            RESTING.replace("resting", "speaking")
        ));
        assert_eq!(shared(&light, &head), None);
        assert_eq!(shared(&light, &speaking), None);
        assert_eq!(shared(&light, &both), Some("the light"));
        assert_eq!(shared(&head, &both), Some("the head"));
        assert_eq!(shared(&both, &both), Some("the light and the head"));
        // Its pace alone differs: the same trigger.
        let paced = signature(&format!(
            "[trigger]\n{RESTING}cooldown = \"12h\"\nfor = \"5s\"\n[[do]]\nlight = {{ dim = 1 }}\n"
        ));
        assert_eq!(shared(&light, &paced), Some("the light"));
    }

    /// The question the lamp asked, said whole and uncut in `request`, as
    /// the thread tells it ([`lamp_said`]).
    fn said_whole(told: &Value, request: u64) {
        let next = told["next"].as_str().unwrap();
        let question = next.split('"').nth(1).unwrap();
        lamp_said(request, question);
    }

    /// A choice in a heard line `request`, on the route with the echo judge
    /// or not (`here`).
    fn pick<'a>(id: &'a str, choice: &'a str, request: u64, here: bool) -> Answer<'a> {
        Answer {
            id,
            choice: Some(choice),
            request,
            heard: true,
            here,
        }
    }

    /// The person's choice: only the owner's own later line; replace puts
    /// the new one on, then takes the other off; keep puts it on beside the
    /// other, promising no winner (Heron's P2 on #6614: nothing enforces
    /// one yet); cancel does nothing.
    #[test]
    fn replace_keep_and_cancel_each_do_what_they_say() {
        let (root, keeping) = keeping("clash-choice");
        let done: Mutex<Vec<String>> = Mutex::new(Vec::new());
        let taken =
            |done: &Mutex<Vec<String>>| -> Vec<String> { done.lock().unwrap().drain(..).collect() };
        let remove = |_: &Keeping, name: &str| {
            done.lock().unwrap().push(format!("remove {name}"));
            Ok(())
        };
        let put_on = |_: &Keeping, sha256: &str| {
            done.lock().unwrap().push(format!("on {}", &sha256[..4]));
            Ok("glow-green".to_owned())
        };
        let hands = Hands {
            remove: &remove,
            put_on: &put_on,
            clock: &Instant::now,
        };
        let clash = Clash {
            name: "glow-blue-on-return".to_owned(),
            shares: "the light",
        };
        let now = Instant::now();
        let ask = || {
            let told = asked(
                &keeping,
                &made("glow-green"),
                std::slice::from_ref(&clash),
                3,
                now,
            );
            said_whole(&told, 3);
            told
        };
        let id = |told: &Value| told[CLASH_ID].as_str().unwrap().to_owned();

        // Not the owner's, the same line, no one spoke, or no choice:
        // nothing done, and it still waits.
        let first = id(&ask());
        let refused = chosen(pick(&first, "replace", 4, true), || false, now, &hands);
        assert!(
            refused["error"].as_str().unwrap().contains("owner"),
            "{refused}"
        );
        let refused = chosen(pick(&first, "replace", 3, true), || true, now, &hands);
        assert!(refused["error"].is_string(), "{refused}");
        let unheard = Answer {
            heard: false,
            ..pick(&first, "replace", 4, true)
        };
        let refused = chosen(unheard, || true, now, &hands);
        assert!(refused["error"].is_string(), "{refused}");
        let refused = chosen(pick(&first, "maybe", 4, true), || true, now, &hands);
        assert!(refused["error"].is_string(), "{refused}");
        assert!(taken(&done).is_empty());

        let replaced = chosen(pick(&first, "replace", 4, true), || true, now, &hands);
        assert_eq!(replaced["on"], true, "{replaced}");
        assert_eq!(
            replaced["replaced"],
            json!(["glow-blue-on-return"]),
            "{replaced}"
        );
        // The new one on first, then the other off (Heron's P3 on #6614).
        assert_eq!(taken(&done), ["on abab", "remove glow-blue-on-return"]);
        // Answered once: asked again, nothing waits.
        let again = chosen(pick(&first, "keep", 5, true), || true, now, &hands);
        assert!(again["error"].is_string(), "{again}");
        assert!(taken(&done).is_empty());

        let kept = chosen(pick(&id(&ask()), "keep", 4, true), || true, now, &hands);
        assert_eq!(kept["kept_both"], true, "{kept}");
        let next = kept["next"].as_str().unwrap();
        assert!(next.contains("Both are on."), "{next}");
        assert!(!next.contains("wins"), "no winner is promised: {next}");
        assert_eq!(taken(&done), ["on abab"]);

        let cancelled = chosen(pick(&id(&ask()), "cancel", 4, true), || true, now, &hands);
        assert_eq!(cancelled["cancelled"], true, "{cancelled}");
        assert_eq!(cancelled["on"], false, "{cancelled}");
        assert!(taken(&done).is_empty());

        // Too late: nothing done.
        let late = chosen(
            pick(&id(&ask()), "keep", 4, true),
            || true,
            now + CLASH_WITHIN + Duration::from_secs(1),
            &hands,
        );
        assert!(late["error"].is_string(), "{late}");
        assert!(taken(&done).is_empty());
        forget_waiting();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Heron's P2 on #6614: the lamp's own question heard back is never the
    /// person's replace, nor is a choice before the question was said whole
    /// and uncut, nor one on the route with no echo judge. Each is refused,
    /// nothing is taken off, and the clash still waits for the person.
    #[test]
    fn an_echoed_unsaid_or_live_replace_is_refused() {
        let (root, keeping) = keeping("clash-echo");
        let done: Mutex<Vec<String>> = Mutex::new(Vec::new());
        let remove = |_: &Keeping, name: &str| {
            done.lock().unwrap().push(format!("remove {name}"));
            Ok(())
        };
        let put_on = |_: &Keeping, _: &str| {
            done.lock().unwrap().push("on".to_owned());
            Ok("glow-green".to_owned())
        };
        let hands = Hands {
            remove: &remove,
            put_on: &put_on,
            clock: &Instant::now,
        };
        let clash = Clash {
            name: "glow-blue-on-return".to_owned(),
            shares: "the light",
        };
        let now = Instant::now();
        let told = asked(
            &keeping,
            &made("glow-green"),
            std::slice::from_ref(&clash),
            3,
            now,
        );
        let id = told[CLASH_ID].as_str().unwrap().to_owned();
        assert!(awaited(now), "the echo hold is on while a clash waits");

        // The question cut: no choice changes the lamp yet, and the model
        // is told to ask it again whole.
        said_whole(&told, 3);
        lamp_cut(3);
        let unsaid = chosen(pick(&id, "replace", 4, true), || true, now, &hands);
        assert!(unsaid["error"].is_string(), "{unsaid}");
        assert!(
            unsaid["next"]
                .as_str()
                .unwrap()
                .contains("replace glow blue on return"),
            "{unsaid}"
        );

        // Said whole in a later answer; then its own words heard back are
        // held as the lamp's echo: that line's replace is refused.
        said_whole(&told, 4);
        echo_held(5);
        let echoed = chosen(pick(&id, "replace", 5, true), || true, now, &hands);
        assert!(
            echoed["error"].as_str().unwrap().contains("own words"),
            "{echoed}"
        );
        let echoed = chosen(pick(&id, "cancel", 5, true), || true, now, &hands);
        assert!(echoed["error"].is_string(), "{echoed}");

        // The Live route has no echo judge: no replace or keep there.
        let live = chosen(pick(&id, "replace", 6, false), || true, now, &hands);
        assert!(live["error"].is_string(), "{live}");
        assert!(done.lock().unwrap().is_empty(), "nothing taken off");

        // The person's own later line on this route: taken.
        let replaced = chosen(pick(&id, "replace", 6, true), || true, now, &hands);
        assert_eq!(replaced["on"], true, "{replaced}");
        assert_eq!(*done.lock().unwrap(), ["on", "remove glow-blue-on-return"]);
        assert!(!awaited(now));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The Reviewer-lead's P1 on #6614: a clash's id and question are noted
    /// on each later line while it waits, and on no line when nothing
    /// waits, in the make's own request, past [`CLASH_WITHIN`], or once
    /// it was answered.
    #[test]
    fn the_waiting_note_shows_only_while_a_clash_waits() {
        forget_waiting();
        let now = Instant::now();
        assert!(waiting_note(4, now).is_none(), "nothing waits");
        let (root, keeping) = keeping("clash-note");
        let clash = Clash {
            name: "glow-blue-on-return".to_owned(),
            shares: "the light",
        };
        let told = asked(
            &keeping,
            &made("glow-green"),
            std::slice::from_ref(&clash),
            3,
            now,
        );
        let id = told[CLASH_ID].as_str().unwrap().to_owned();
        assert!(waiting_note(3, now).is_none(), "the make's own request");
        assert!(waiting_note(2, now).is_none(), "an earlier request");
        let note = waiting_note(4, now).unwrap();
        assert!(note.contains(&format!("clash_id \"{id}\"")), "{note}");
        assert!(note.contains("glow green"), "{note}");
        assert!(
            note.contains(
                "Shall I replace glow blue on return, keep both (they may take turns), or cancel?"
            ),
            "{note}"
        );
        assert!(note.contains("choice \"replace\", \"keep\" or \"cancel\""));
        let late = now + CLASH_WITHIN + Duration::from_secs(1);
        assert!(waiting_note(4, late).is_none(), "expired");
        assert!(
            waiting_note(4, now + CLASH_WITHIN).is_some(),
            "at the bound"
        );

        // Answered: no note after it.
        let never = |_: &Keeping, _: &str| -> Result<(), NotOff> { Ok(()) };
        let off = |_: &Keeping, _: &str| -> Result<String, String> { Ok(String::new()) };
        let hands = Hands {
            remove: &never,
            put_on: &off,
            clock: &Instant::now,
        };
        let cancelled = chosen(pick(&id, "cancel", 4, true), || true, now, &hands);
        assert_eq!(cancelled["cancelled"], true, "{cancelled}");
        assert!(waiting_note(5, now).is_none(), "answered");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Heron's P3 on #6614: replace puts the new one on first, so a put-on
    /// that fails takes nothing off; a remove that fails after leaves both
    /// on, and says so.
    #[test]
    fn replace_never_leaves_both_off() {
        let (root, keeping) = keeping("clash-replace-fails");
        let removed = Mutex::new(0);
        let remove_ok = |_: &Keeping, _: &str| {
            *removed.lock().unwrap() += 1;
            Ok(())
        };
        let remove_no = |_: &Keeping, _: &str| {
            Err(NotOff::Refused(
                "the lamp didn't let it be removed".to_owned(),
            ))
        };
        let on_no = |_: &Keeping, _: &str| Err("the lamp's skills couldn't be reached".to_owned());
        let on_ok = |_: &Keeping, _: &str| Ok("glow-green".to_owned());
        let clash = Clash {
            name: "glow-blue-on-return".to_owned(),
            shares: "the light",
        };
        let now = Instant::now();
        let ask = || {
            let told = asked(
                &keeping,
                &made("glow-green"),
                std::slice::from_ref(&clash),
                3,
                now,
            );
            said_whole(&told, 3);
            told[CLASH_ID].as_str().unwrap().to_owned()
        };

        let hands = Hands {
            remove: &remove_ok,
            put_on: &on_no,
            clock: &Instant::now,
        };
        let failed = chosen(pick(&ask(), "replace", 4, true), || true, now, &hands);
        assert_eq!(*removed.lock().unwrap(), 0, "nothing taken off: {failed}");
        assert!(
            failed["next"]
                .as_str()
                .unwrap()
                .contains("glow blue on return stays as it was"),
            "{failed}"
        );

        let hands = Hands {
            remove: &remove_no,
            put_on: &on_ok,
            clock: &Instant::now,
        };
        let both = chosen(pick(&ask(), "replace", 4, true), || true, now, &hands);
        assert_eq!(both["on"], true, "{both}");
        assert!(both.get("replaced").is_none(), "{both}");
        let next = both["next"].as_str().unwrap();
        assert!(
            next.contains("glow green is on, but glow blue on return wasn't removed"),
            "{next}"
        );
        assert!(next.contains("Both are on"), "{next}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Lamp-9adf, 13 Nov (Fourteen's 15:54 on #6593): three skills on the
    /// lamp on one moment with the light. Only the first in folder order was
    /// named and replaced, so after "replace it" the other two still
    /// clashed. A make that clashes with all three names all three, and in
    /// a fresh lamp each time: replace puts the new one on, then takes each
    /// of the three off; keep both takes none off; cancel neither.
    #[test]
    fn a_make_clashing_with_three_names_and_replaces_all_three() {
        const THREE: [&str; 3] = [
            "blink-green-twice",
            "glow-green-on-return",
            "green-blink-on-return",
        ];
        let now = Instant::now();
        for choice in ["replace", "keep", "cancel"] {
            let (root, keeping) = keeping(&format!("clash-three-{choice}"));
            for name in THREE {
                installed(&keeping, name, RESTING, "light = { colour = \"green\" }");
            }
            let (url, served) = service(vec![(200, good())]);
            let told = crate::make_skill_local::make_with(
                3,
                Some("make me a skill that dims the light at 10 pm"),
                "dim the light at 10 pm",
                &keeping,
                chat_at(url),
                NOW,
                || None,
            );
            assert_eq!(served.join().unwrap(), 1);
            assert_eq!(told["on"], false, "{told}");
            let next = told["next"].as_str().unwrap();
            let question = next.split('"').nth(1).unwrap();
            for name in THREE {
                let spoken = made_skill::spoken_name(name);
                assert!(question.contains(&spoken), "{spoken} named: {question}");
                assert!(
                    told["clashes_with"].as_str().unwrap().contains(&spoken),
                    "{told}"
                );
            }
            let line = crate::make_skill_local::take_said_line().unwrap();
            for name in THREE {
                assert!(line.contains(&made_skill::spoken_name(name)), "{line}");
            }
            said_whole(&told, 3);

            let done: Mutex<Vec<String>> = Mutex::new(Vec::new());
            let remove = |_: &Keeping, name: &str| {
                done.lock().unwrap().push(format!("remove {name}"));
                Ok(())
            };
            let put_on = |_: &Keeping, _: &str| {
                done.lock().unwrap().push("on".to_owned());
                Ok("evening-dim".to_owned())
            };
            let hands = Hands {
                remove: &remove,
                put_on: &put_on,
                clock: &Instant::now,
            };
            let id = told[CLASH_ID].as_str().unwrap().to_owned();
            let answered = chosen(pick(&id, choice, 4, true), || true, now, &hands);
            let done = done.into_inner().unwrap();
            let next = answered["next"].as_str().unwrap_or_default().to_owned();
            match choice {
                "replace" => {
                    assert_eq!(answered["on"], true, "{answered}");
                    assert_eq!(
                        answered["replaced"],
                        json!(THREE),
                        "all three taken off: {answered}"
                    );
                    // The new one on first, then each of the three off.
                    assert_eq!(
                        done,
                        [
                            "on",
                            "remove blink-green-twice",
                            "remove glow-green-on-return",
                            "remove green-blink-on-return"
                        ]
                    );
                }
                "keep" => {
                    assert_eq!(answered["kept_both"], true, "{answered}");
                    assert_eq!(done, ["on"], "none taken off");
                }
                _ => {
                    assert_eq!(answered["cancelled"], true, "{answered}");
                    assert!(done.is_empty(), "nothing done");
                }
            }
            for name in THREE {
                assert!(
                    next.contains(&made_skill::spoken_name(name)),
                    "{choice}: {next}"
                );
            }
            forget_waiting();
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// Many clashes are each named, none cut to "and N more" (Heron's P2
    /// on #6976). A replace where one of them is refused (a core skill)
    /// still takes the others off, and says which stays on.
    #[test]
    fn many_clashes_are_all_named_and_each_is_removed_on_its_own() {
        let names = |n: usize| -> Vec<String> {
            ["glow-a", "glow-b", "glow-c", "glow-d", "glow-e"][..n]
                .iter()
                .map(|name| (*name).to_owned())
                .collect()
        };
        assert_eq!(spoken_list(&names(1)), "glow a");
        assert_eq!(spoken_list(&names(2)), "glow a and glow b");
        assert_eq!(spoken_list(&names(3)), "glow a, glow b and glow c");
        assert_eq!(
            spoken_list(&names(5)),
            "glow a, glow b, glow c, glow d and glow e"
        );

        let (root, keeping) = keeping("clash-many");
        let clashes: Vec<Clash> = names(5)
            .into_iter()
            .map(|name| Clash {
                name,
                shares: "the light",
            })
            .collect();
        let now = Instant::now();
        let told = asked(&keeping, &made("glow-green"), &clashes, 3, now);
        let question = told["next"].as_str().unwrap().split('"').nth(1).unwrap();
        assert!(
            question.contains("glow a, glow b, glow c, glow d and glow e, which are on the lamp"),
            "{question}"
        );
        assert!(question.contains("replace them all"), "{question}");
        said_whole(&told, 3);
        let removed: Mutex<Vec<String>> = Mutex::new(Vec::new());
        let remove = |_: &Keeping, name: &str| {
            if name == "glow-c" {
                return Err(NotOff::Refused(
                    "the lamp didn't let glow c be removed".to_owned(),
                ));
            }
            removed.lock().unwrap().push(name.to_owned());
            Ok(())
        };
        let put_on = |_: &Keeping, _: &str| Ok("glow-green".to_owned());
        let hands = Hands {
            remove: &remove,
            put_on: &put_on,
            clock: &Instant::now,
        };
        let id = told[CLASH_ID].as_str().unwrap().to_owned();
        let replaced = chosen(pick(&id, "replace", 4, true), || true, now, &hands);
        assert_eq!(
            replaced["replaced"],
            json!(["glow-a", "glow-b", "glow-d", "glow-e"]),
            "{replaced}"
        );
        assert_eq!(replaced["not_removed"], json!(["glow-c"]), "{replaced}");
        let next = replaced["next"].as_str().unwrap();
        assert!(next.contains("but glow c wasn't removed"), "{next}");
        assert!(next.contains("glow c is still on"), "{next}");
        assert_eq!(removed.into_inner().unwrap().len(), 4);
        forget_waiting();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Clashes on the light, one per folder name.
    fn clashes_of(names: &[&str]) -> Vec<Clash> {
        names
            .iter()
            .map(|name| Clash {
                name: (*name).to_owned(),
                shares: "the light",
            })
            .collect()
    }

    /// Heron's P2 on #6976: a make that clashes with five named only three
    /// and "2 more", and a replace then took all five off. Now the
    /// question, the cut line, the waiting note and the done line each name
    /// all five, and a replace takes off exactly those named, no other.
    #[test]
    fn five_clashes_are_all_named_and_a_replace_removes_exactly_those() {
        const FIVE: [&str; 5] = [
            "blink-green-twice",
            "glow-blue-on-return",
            "glow-green-on-return",
            "green-blink-on-return",
            "rest-glow",
        ];
        let (root, keeping) = keeping("clash-five");
        for name in FIVE {
            installed(&keeping, name, RESTING, "light = { colour = \"green\" }");
        }
        // Another trigger: on the lamp, never named, never taken off.
        installed(
            &keeping,
            "arrival-glow",
            "topic = \"room/presence\"\nfield = \"someone\"\nequals = true\n",
            "light = { colour = \"blue\" }",
        );
        let (url, served) = service(vec![(200, good())]);
        let told = crate::make_skill_local::make_with(
            3,
            Some("make me a skill that dims the light at 10 pm"),
            "dim the light at 10 pm",
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(told["on"], false, "{told}");
        let question = told["next"].as_str().unwrap().split('"').nth(1).unwrap();
        let cut = crate::make_skill_local::take_said_line().unwrap();
        let note = waiting_note(4, Instant::now()).unwrap();
        let clashes_with = told["clashes_with"].as_str().unwrap();
        for said in [question, cut.as_str(), note.as_str(), clashes_with] {
            assert!(!said.contains("more"), "none cut: {said}");
            for name in FIVE {
                let spoken = made_skill::spoken_name(name);
                assert!(said.contains(&spoken), "{spoken} named: {said}");
            }
            assert!(!said.contains("arrival glow"), "{said}");
        }
        said_whole(&told, 3);

        let done: Mutex<Vec<String>> = Mutex::new(Vec::new());
        let remove = |_: &Keeping, name: &str| {
            done.lock().unwrap().push(name.to_owned());
            Ok(())
        };
        let put_on = |_: &Keeping, _: &str| Ok("evening-dim".to_owned());
        let hands = Hands {
            remove: &remove,
            put_on: &put_on,
            clock: &Instant::now,
        };
        let id = told[CLASH_ID].as_str().unwrap().to_owned();
        let replaced = chosen(
            pick(&id, "replace", 4, true),
            || true,
            Instant::now(),
            &hands,
        );
        assert_eq!(*done.lock().unwrap(), FIVE, "exactly those named");
        assert_eq!(replaced["replaced"], json!(FIVE), "{replaced}");
        let next = replaced["next"].as_str().unwrap();
        for name in FIVE {
            let spoken = made_skill::spoken_name(name);
            assert!(next.contains(&spoken), "{spoken} told removed: {next}");
        }
        forget_waiting();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Heron's P2 on #6976: each remove waited its own 8 s in turn, so a
    /// hung helper held the turn 8 s per clash. Now every remove ends
    /// within [`REPLACE_WITHIN`]: one whose own wait would end past it is
    /// never started, and is told as not removed. On a clock of the test's
    /// own, a helper slow 3 s a remove takes two off; one hung each time
    /// takes one, and the turn still ends inside the deadline.
    #[test]
    fn a_replace_ends_its_removes_within_one_deadline() {
        let names = ["glow-a", "glow-b", "glow-c", "glow-d", "glow-e"];
        for (slow, started) in [(Duration::from_secs(3), 2), (HELPER_WAIT, 1)] {
            let (root, keeping) = keeping("clash-deadline");
            let start = Instant::now();
            let at = Mutex::new(start);
            let clock = || *at.lock().unwrap();
            let asked_of: Mutex<Vec<String>> = Mutex::new(Vec::new());
            let remove = |_: &Keeping, name: &str| {
                asked_of.lock().unwrap().push(name.to_owned());
                *at.lock().unwrap() += slow;
                if slow == HELPER_WAIT {
                    Err(NotOff::Unsure(
                        "the lamp didn't say in time whether it is off".to_owned(),
                    ))
                } else {
                    Ok(())
                }
            };
            let put_on = |_: &Keeping, _: &str| Ok("glow-green".to_owned());
            let hands = Hands {
                remove: &remove,
                put_on: &put_on,
                clock: &clock,
            };
            let told = asked(&keeping, &made("glow-green"), &clashes_of(&names), 3, start);
            said_whole(&told, 3);
            let id = told[CLASH_ID].as_str().unwrap().to_owned();
            let replaced = chosen(pick(&id, "replace", 4, true), || true, start, &hands);
            let asked_of = asked_of.into_inner().unwrap();
            assert_eq!(asked_of, names[..started], "{replaced}");
            assert!(
                *at.lock().unwrap() - start <= REPLACE_WITHIN,
                "the removes end within the deadline"
            );
            assert_eq!(
                replaced["not_removed"],
                json!(names[started..]),
                "not started, not removed: {replaced}"
            );
            let next = replaced["next"].as_str().unwrap();
            let late = spoken_list(
                &names[started..]
                    .iter()
                    .map(|n| (*n).to_owned())
                    .collect::<Vec<_>>(),
            );
            assert!(
                next.contains(&format!("{late} weren't removed: {NO_TIME}")),
                "{next}"
            );
            assert!(next.contains(&format!("{late} are still on.")), "{next}");
            if started == 2 {
                assert_eq!(replaced["replaced"], json!(["glow-a", "glow-b"]));
                assert!(
                    next.contains("and I removed glow a and glow b, but"),
                    "{next}"
                );
            } else {
                // The hung one may be off: never told as still on.
                assert_eq!(replaced["may_still_be_on"], json!(["glow-a"]));
                assert!(
                    next.contains("glow a may still be on: the lamp didn't say in time"),
                    "{next}"
                );
                assert!(!next.contains("glow a is still on"), "{next}");
            }
            forget_waiting();
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// Heron's P3 on #6976: every remove that failed was told with the
    /// first one's reason. Now each says its own, and two with the same
    /// reason are said together.
    #[test]
    fn each_failed_remove_says_its_own_reason() {
        let (root, keeping) = keeping("clash-reasons");
        let names = ["glow-a", "glow-b", "glow-c", "glow-d"];
        let remove = |_: &Keeping, name: &str| match name {
            "glow-a" => Ok(()),
            "glow-b" => Err(NotOff::Refused(
                "the lamp didn't let glow b be removed".to_owned(),
            )),
            _ => Err(NotOff::Refused(
                "the lamp's skills couldn't be reached".to_owned(),
            )),
        };
        let put_on = |_: &Keeping, _: &str| Ok("glow-green".to_owned());
        let hands = Hands {
            remove: &remove,
            put_on: &put_on,
            clock: &Instant::now,
        };
        let now = Instant::now();
        let told = asked(&keeping, &made("glow-green"), &clashes_of(&names), 3, now);
        said_whole(&told, 3);
        let id = told[CLASH_ID].as_str().unwrap().to_owned();
        let replaced = chosen(pick(&id, "replace", 4, true), || true, now, &hands);
        assert_eq!(
            replaced["next"].as_str().unwrap().split('"').nth(1),
            Some(
                "glow green is on, and I removed glow a, but glow b wasn't removed: the lamp \
                 didn't let glow b be removed; glow c and glow d weren't removed: the lamp's \
                 skills couldn't be reached. glow b, glow c and glow d are still on."
            ),
            "{replaced}"
        );
        assert_eq!(
            replaced["not_removed"],
            json!(["glow-b", "glow-c", "glow-d"])
        );
        forget_waiting();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Heron's P3 on #6976: a choice that is none of the three is asked
    /// again as the question was: of them all when there are several, of
    /// the one when there is one. The clash still waits.
    #[test]
    fn an_invalid_choice_is_asked_again_in_the_question_s_number() {
        let (root, keeping) = keeping("clash-again");
        let none = |_: &Keeping, _: &str| -> Result<(), NotOff> { panic!("nothing removed") };
        let never = |_: &Keeping, _: &str| -> Result<String, String> { panic!("nothing put on") };
        let hands = Hands {
            remove: &none,
            put_on: &never,
            clock: &Instant::now,
        };
        let now = Instant::now();
        for (names, again) in [
            (
                &["glow-a", "glow-b"][..],
                "replace them all, keep them all, or cancel.",
            ),
            (&["glow-a"][..], "replace it, keep both, or cancel."),
        ] {
            let told = asked(&keeping, &made("glow-green"), &clashes_of(names), 3, now);
            said_whole(&told, 3);
            let id = told[CLASH_ID].as_str().unwrap().to_owned();
            let refused = chosen(pick(&id, "maybe", 4, true), || true, now, &hands);
            let next = refused["next"].as_str().unwrap();
            assert!(next.ends_with(again), "{next}");
            assert!(awaited(now), "it still waits");
        }
        forget_waiting();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Heron's P3 on #6976: two folders with the same spoken words came
    /// out as "glow a and glow a". Now each of them is told by its folder
    /// too, in the question and in what a replace says; one that sounds
    /// like no other is said as before.
    #[test]
    fn two_skills_that_sound_the_same_are_told_apart_by_folder() {
        let names = ["glow-a", "glow_a", "glow-b"];
        let owned: Vec<String> = names.iter().map(|n| (*n).to_owned()).collect();
        assert_eq!(
            spoken_list(&owned),
            "glow a (folder glow-a), glow_a (folder glow_a) and glow b"
        );
        let (root, keeping) = keeping("clash-same-sound");
        let now = Instant::now();
        let told = asked(&keeping, &made("glow-green"), &clashes_of(&names), 3, now);
        let question = told["next"].as_str().unwrap().split('"').nth(1).unwrap();
        assert!(
            question.contains("glow a (folder glow-a), glow_a (folder glow_a) and glow b"),
            "{question}"
        );
        said_whole(&told, 3);
        let remove = |_: &Keeping, name: &str| {
            if name == "glow_a" {
                Err(NotOff::Refused(
                    "the lamp didn't let it be removed".to_owned(),
                ))
            } else {
                Ok(())
            }
        };
        let put_on = |_: &Keeping, _: &str| Ok("glow-green".to_owned());
        let hands = Hands {
            remove: &remove,
            put_on: &put_on,
            clock: &Instant::now,
        };
        let id = told[CLASH_ID].as_str().unwrap().to_owned();
        let replaced = chosen(pick(&id, "replace", 4, true), || true, now, &hands);
        let next = replaced["next"].as_str().unwrap();
        assert!(
            next.contains("I removed glow a (folder glow-a) and glow b, but glow_a (folder glow_a) wasn't removed"),
            "{next}"
        );
        assert!(
            next.contains("glow_a (folder glow_a) is still on."),
            "{next}"
        );
        forget_waiting();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Heron's P3 on #6614: a made skill cancelled, or whose clash went
    /// unanswered, is dropped from `made.json` with its package, so it is
    /// never listed as made and off.
    #[test]
    fn cancel_and_expiry_forget_the_made_one() {
        let (root, keeping) = keeping("clash-forget");
        std::fs::create_dir_all(&keeping.directory).unwrap();
        let sha256 = "ab".repeat(32);
        let package = keeping.directory.join("glow-green-1.0.0.tar");
        let keep_it = || {
            std::fs::write(&package, "tar").unwrap();
            std::fs::write(
                keeping.directory.join(made_skill::INDEX),
                serde_json::to_vec(&json!([
                    {"name": "glow-green", "sha256": sha256, "package": "glow-green-1.0.0.tar"},
                    {"name": "rest-glow", "sha256": "cd".repeat(32), "on": true,
                     "package": "rest-glow-1.0.0.tar"}
                ]))
                .unwrap(),
            )
            .unwrap();
        };
        let names = || -> Vec<String> {
            made_skill::entries(&keeping.directory)
                .iter()
                .filter_map(|entry| entry["name"].as_str().map(str::to_owned))
                .collect()
        };
        let nothing = |_: &Keeping, _: &str| -> Result<(), NotOff> { panic!("nothing removed") };
        let never = |_: &Keeping, _: &str| -> Result<String, String> { panic!("nothing put on") };
        let hands = Hands {
            remove: &nothing,
            put_on: &never,
            clock: &Instant::now,
        };
        let clash = Clash {
            name: "rest-glow".to_owned(),
            shares: "the light",
        };
        let now = Instant::now();

        keep_it();
        let told = asked(
            &keeping,
            &made("glow-green"),
            std::slice::from_ref(&clash),
            3,
            now,
        );
        said_whole(&told, 3);
        let id = told[CLASH_ID].as_str().unwrap().to_owned();
        let cancelled = chosen(pick(&id, "cancel", 4, true), || true, now, &hands);
        assert_eq!(cancelled["cancelled"], true, "{cancelled}");
        assert_eq!(names(), ["rest-glow"]);
        assert!(!package.exists());

        // Unanswered: once past its wait, the next heard line drops it.
        keep_it();
        asked(
            &keeping,
            &made("glow-green"),
            std::slice::from_ref(&clash),
            3,
            now,
        );
        assert!(awaited(now));
        assert_eq!(names(), ["glow-green", "rest-glow"]);
        assert!(!awaited(now + CLASH_WITHIN + Duration::from_secs(1)));
        assert_eq!(names(), ["rest-glow"]);
        assert!(!package.exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
