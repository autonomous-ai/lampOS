//! Old made skills remade for the lamp's agent (the first builder's yes on
//! #6811, 18 Nov 16:40, his 19:50 ruling, and his 19 Nov 06:07 and 06:08
//! rulings on #7814): with [`ENVIRONMENT`] set (`on` in the product's
//! `lamp-voice`), a made skill
//! DeepSeek made before the route (its `made_with` missing or `deepseek`)
//! is made again as one task for the lamp's agent, its own start and
//! timing exactly as they were, and sip-of-water's guards on any trigger
//! but a clock or the person's own words (`make_skill_local::remade_as_task`,
//! `make_no_cannot::skill_text`). The lamp writes it; the agent is never
//! asked to write it and lamp-tasks' make slot is never taken.
//!
//! - **Two phases** ([`Scope`]): `on` remakes the broken ones alone
//!   ([`broken`], phase 1); `all` remakes every other one too that isn't a
//!   quick reaction or a lamp-only fire ([`lamp_only_fire`], phase 2), its
//!   old steps kept beside its task ([`with_old_steps`]) and run in the
//!   agent's place when its answer isn't ready within 10 s of the fire, or
//!   the agent is down (`lamp_skills::task::OTHERWISE_AFTER_MS`): nobody's
//!   working skill gets worse. Anything else is off.
//! - **Broken** is a check of the kept skill, never a judgement of words: a
//!   `say` whose text is DeepSeek's stand-in for data the make never
//!   fetched (`say = { text = "internet: current price of Zorbo coin" }`,
//!   [`stand_in_say`]), or a said-back `description` that itself names a
//!   part left out ("Left out: its colour…", [`left_out_of`]). A skill that
//!   runs as its owner heard it at the make is never touched in phase 1.
//! - **The task's words, never guessed** ([`task_of`]): the person's kept
//!   `request` in `made.json`; else the skill's own `asks` phrase, the
//!   words its owner says to run it; else, for a skill with no `asks`, its
//!   said-back `description`; and in any case a part the description names
//!   as left out, from the description. None of these, or more than its
//!   task holds whole, and the skill stays as it is, journaled why.
//! - **What it touches** ([`chosen`]): a kept made skill on the lamp, made
//!   here by voice or in the app (`from` is `local:` or `written:`), never
//!   one added from a file (`upload:`, `added`), a Store skill or a core
//!   skill (a folder of that name not marked `made: here`), and never one
//!   changed since it was made: its folder must hold the very package made
//!   (`made_skill::went_on_as`) and, with no code of its own, the very
//!   `skill.toml` (`toml_sha256`). Never a quick reaction
//!   (`made_skill::quick_reaction`).
//! - **Its start never picked again** ([`kept_timing`]): the old
//!   `most_a_day`, `asks`, `[trigger]` (its `cooldown` in it), `[active]`
//!   and `[quiet]` are the remake's (`made_skill::RemakeAs`); of the lamp
//!   model's one call only the skill's grants are taken. One whose start
//!   can't be kept stays as it is; a remake whose kept skill would start or
//!   be timed otherwise keeps nothing (`made_skill::REMAKE_TIMING_CHANGED`).
//! - **Remade without asking** (18 Nov 18:23): each one that qualifies is
//!   queued on its own ([`step`]). The person's own "remake my old skills"
//!   comes back as `make_skill`'s [`REMAKE_OLD`] [`ASKED`] ([`asked`]): it
//!   queues them again, one put back or out of tries too.
//! - **Nothing asked** (18 Nov 21:58): with `LAMP_MAKE_NO_ASKS` on, it goes
//!   on with its task's yes recorded for its grants
//!   (`make_skill_local::code_on_at_once`), so it runs at its first start
//!   with no question; off, its task asks when it first runs, as before. Its
//!   only code is the lamp's own agent script (`made_skill::Uses::agent_task`).
//! - **Remade one at a time** ([`step`]), only while idle (no line heard for
//!   [`IDLE_FOR`]), never in the made skills' quiet hours
//!   (`made_skill::MANNERS_QUIET`), at most [`MOST_A_DAY`] a day, and only
//!   on a lamp with an agent to do its task (none: nothing is tried, no try
//!   counted, the old ones stay). The new one replaces the old under the
//!   same name, its on/off state kept, only once it was kept (the same
//!   checks a new make passes) and put on. It records `made_with` and
//!   `remade_from` (the old package's SHA-256). A failed one leaves the old
//!   exactly as it was, journaled with a short reason and no word of the
//!   skill, and is tried at most once more, on another day.
//! - **Undo for a week** ([`put_back`]): the old entry is kept under the new
//!   one's `old` key with `replaced_at_ms`, its package with it;
//!   `make_skill`'s [`PUT_BACK`] ("put it back") puts it back on. After
//!   [`UNDO_FOR`] the old package is deleted ([`drop_old`]). A remade skill
//!   removed takes its old package with it, and the owner's "forget
//!   everything" clears this undo with `make_no_asks`' keep ([`clear`]).
//!   Every write of the index here holds its lock (`made_skill::index_locked`).
//! - **Told once after** ([`told_line`], [`note`]): on the owner's next heard
//!   line with nothing else waiting, in the lamp's or the conversation's
//!   language: "I fixed your zorbo coin price skill so it does what you
//!   asked. Say 'put my old zorbo coin price skill back' to undo." (one
//!   skill names it, so an older remove's undo never takes its place)
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

/// The switch: `on` for the broken old skills alone, `all` for every old
/// skill that isn't a quick reaction or a lamp-only fire ([`Scope`]); `on`
/// in the product's `lamp-voice`.
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
/// What `make_skill`'s undo adds while a remake is the latest thing done
/// ([`put_back_for_undo_here`]): a remade skill's old version is put back
/// by [`PUT_BACK`], never the undo (Twelve's P2 on #7765).
pub const PUT_BACK_UNDO_TOLD: &str = " Not for a fixed or moved skill's old one: put_back.";
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
/// Why a phase-2 remake kept nothing: its old steps couldn't be kept
/// beside its task to run in the agent's place ([`with_old_steps`]).
pub const OLD_STEPS_NOT_KEPT: &str = "its old steps can't be kept beside its task";

/// Which old skills are remade (the first builder, 06:08): the broken ones
/// first, the working ones after the core move's pilot rows pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// `on`: the broken ones alone ([`broken`]).
    Broken,
    /// `all`: the broken ones, and every working one that isn't a quick
    /// reaction or a lamp-only fire, its old steps run in the agent's place
    /// when its answer isn't ready in time.
    All,
}

/// Whether the switch is on in this process's environment.
pub fn on() -> bool {
    #[cfg(test)]
    if let Some(on) = ON_HERE.with(std::cell::Cell::get) {
        return on;
    }
    scope().is_some()
}

#[cfg(test)]
thread_local! {
    static ON_HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// [`on`] for this test's thread alone; `None` reads the environment again.
#[cfg(test)]
pub(crate) fn set_on_here(on: Option<bool>) {
    ON_HERE.with(|here| here.set(on));
}

/// Whether the switch's value `value` turns it on ([`scope_with`]).
pub fn on_with(value: Option<&str>) -> bool {
    scope_with(value).is_some()
}

/// The switch's scope in this process's environment.
pub fn scope() -> Option<Scope> {
    scope_with(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// What the switch's value `value` remakes: `on` the broken ones, `all`
/// every one; anything else none.
pub fn scope_with(value: Option<&str>) -> Option<Scope> {
    match value {
        Some("on") => Some(Scope::Broken),
        Some("all") => Some(Scope::All),
        _ => None,
    }
}

thread_local! {
    /// The scope the pass picks with, as a test of this thread sets it.
    static PICKING_HERE: std::cell::Cell<Option<Option<Scope>>> =
        const { std::cell::Cell::new(None) };
}

/// The scope the pass picks with: this thread's, set by a test, else the
/// environment's.
fn picking() -> Option<Scope> {
    PICKING_HERE
        .with(std::cell::Cell::get)
        .unwrap_or_else(scope)
}

/// [`picking`] for this test's thread alone; `None` reads the environment
/// again. The product never calls it.
#[doc(hidden)]
pub fn set_picking_here(scope: Option<Option<Scope>>) {
    PICKING_HERE.with(|here| here.set(scope));
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

/// Whether a made skill's `skill.toml` (`table`) has a `say` whose text is
/// DeepSeek's stand-in for what its make never did: one of its own markers
/// (`made_skill::SAY_MARKERS`, the one list `made_skill`'s keep refuses:
/// `internet:` and `cannot:`) with words after it, in any case (lamp-9adf,
/// 18 Nov: zorbo-coin-price's `say = { text = "internet: current price of
/// Zorbo coin" }`, read aloud as it stood; `cannot:` as broken too, the
/// first builder's 10:29 on #7814). Only a `say` table's `text` that
/// begins with a marker: "the internet: what a place" or a note that names
/// the internet is not one.
pub fn stand_in_say(table: &toml::Table) -> bool {
    table
        .get("do")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|step| step.get("say")?.get("text")?.as_str())
        .any(|text| {
            let text = text.trim_start();
            made_skill::SAY_MARKERS.iter().any(|marker| {
                text.get(..marker.len())
                    .is_some_and(|head| head.eq_ignore_ascii_case(marker))
                    && !text[marker.len()..].trim().is_empty()
            })
        })
}

/// How a said-back `description` names a part the make left out: the
/// lamp's own "Left out: …" (`made_skill::left_out_said`), and its frame of
/// the model's own left-out line ("The maker notes what it couldn't do:
/// …").
const LEFT_OUT_MARKS: [&str; 2] = ["Left out:", "The maker notes what it couldn't do:"];

/// Where in `description` it names a part left out ([`LEFT_OUT_MARKS`]):
/// the mark's start, and the part's words up to its sentence's end.
fn left_out_at(description: &str) -> Option<(usize, &str)> {
    LEFT_OUT_MARKS.iter().find_map(|mark| {
        let at = description.find(mark)?;
        let after = &description[at + mark.len()..];
        let part = after
            .split(['.', '!', '?', '\n'])
            .next()
            .unwrap_or_default()
            .trim();
        (!part.is_empty()).then_some((at, part))
    })
}

/// The part a said-back `description` itself names as left out (focus
/// mode's "Left out: its colour in a step that changes the light."): its
/// words, or none.
pub fn left_out_of(description: &str) -> Option<&str> {
    left_out_at(description).map(|(_, part)| part)
}

/// Whether the kept made skill (its index `entry`, its `skill.toml` as
/// `table`) is broken, by a check of the skill alone (the first builder,
/// 06:07): a stand-in `say` for data ([`stand_in_say`]), or a description
/// that names a part left out ([`left_out_of`]).
pub fn broken(entry: &Value, table: &toml::Table) -> bool {
    stand_in_say(table)
        || entry["description"]
            .as_str()
            .and_then(left_out_of)
            .is_some()
}

/// The step kinds of a lamp-only fire (the first builder, 01:29 and
/// 06:08): its light, its body and its sounds, with when each runs.
const LAMP_ONLY_STEPS: [&str; 6] = ["light", "body", "cue", "sound", "on", "after"];

/// Whether a made skill's `skill.toml` (`table`) is a lamp-only fire (a
/// pat, a glow, a nod, a chime): every step its light, its body or its
/// sounds, nothing said, looked at, read, written or handed over. A working
/// one is never moved; a broken one ([`broken`]) is fixed in phase 1.
pub fn lamp_only_fire(table: &toml::Table) -> bool {
    table
        .get("do")
        .and_then(toml::Value::as_array)
        .is_some_and(|steps| {
            !steps.is_empty()
                && steps.iter().all(|step| {
                    step.as_table().is_some_and(|step| {
                        step.keys()
                            .all(|key| LAMP_ONLY_STEPS.contains(&key.as_str()))
                    })
                })
        })
}

/// Whether `words` are whole as a remade skill's task: some words, and
/// none cut (`make_no_cannot::task_cut`).
fn task_holds(words: &str) -> bool {
    !crate::make_no_cannot::task_words(words).is_empty() && !crate::make_no_cannot::task_cut(words)
}

/// The person's own request a kept made skill was made from (`request` in
/// `made.json`), whole. None when none is kept, or when its task couldn't
/// hold it whole.
pub fn kept_request(entry: &Value) -> Option<&str> {
    entry["request"]
        .as_str()
        .map(str::trim)
        .filter(|request| task_holds(request))
}

/// A remade skill's task words, in the first builder's order (06:07,
/// 06:08), never guessed: the kept `request` ([`kept_request`]); else the
/// skill's own first `asks` phrase, its owner's words to run it; else, for
/// a skill with no `asks`, its said-back `description` (the part it names
/// left out taken out of it); and in any case a part the description names
/// as left out ([`left_out_of`]) added. None when none of these gives
/// one, or when its task couldn't hold them whole: then it stays as it is.
pub fn task_of(entry: &Value, table: &toml::Table) -> Option<String> {
    let description = entry["description"].as_str().unwrap_or_default().trim();
    let asks = table.get("asks").and_then(toml::Value::as_array);
    let base = kept_request(entry)
        .map(str::to_owned)
        .or_else(|| {
            asks?
                .iter()
                .find_map(toml::Value::as_str)
                .map(str::trim)
                .filter(|ask| !ask.is_empty())
                .map(str::to_owned)
        })
        .or_else(|| {
            if asks.is_some() {
                return None;
            }
            let said = match left_out_at(description) {
                Some((at, _)) => &description[..at],
                None => description,
            };
            Some(said.trim().to_owned()).filter(|said| !said.is_empty())
        })?;
    let task = match left_out_of(description) {
        Some(part) => format!(
            "{}; also {part}",
            base.trim().trim_end_matches(['.', '!', '?'])
        ),
        None => base,
    };
    task_holds(&task).then_some(task)
}

/// What a skill's start and timing are: `most_a_day`, `asks`, `[trigger]`
/// (its `cooldown` in it), `[active]` and `[quiet]`.
const TIMING: [&str; 5] = ["most_a_day", "asks", "trigger", "active", "quiet"];
/// [`TIMING`]'s keys written at the file's top, before any table.
const TOP_TIMING: [&str; 2] = ["most_a_day", "asks"];

/// The parts of `table` that are its start and timing ([`TIMING`]).
fn timing_of(table: &toml::Table) -> toml::Table {
    TIMING
        .iter()
        .filter_map(|key| Some(((*key).to_owned(), table.get(*key)?.clone())))
        .collect()
}

/// `value` written as a TOML value on one line (an inline table for a
/// table), or none for one it can't be: a date. Strings as JSON writes
/// them, which TOML reads as the same basic string.
fn inline(value: &toml::Value) -> Option<String> {
    Some(match value {
        toml::Value::String(text) => Value::from(text.as_str()).to_string(),
        toml::Value::Integer(number) => number.to_string(),
        toml::Value::Float(number) if number.is_finite() => format!("{number:?}"),
        toml::Value::Boolean(on) => on.to_string(),
        toml::Value::Array(items) => format!(
            "[{}]",
            items
                .iter()
                .map(inline)
                .collect::<Option<Vec<_>>>()?
                .join(", ")
        ),
        toml::Value::Table(table) => format!(
            "{{ {} }}",
            table
                .iter()
                .map(|(key, value)| Some(format!(
                    "{} = {}",
                    Value::from(key.as_str()),
                    inline(value)?
                )))
                .collect::<Option<Vec<_>>>()?
                .join(", ")
        ),
        _ => return None,
    })
}

/// `table`'s entries, each `key = value` on its own line ([`inline`]).
fn entries_of(table: &toml::Table) -> Option<String> {
    table
        .iter()
        .map(|(key, value)| {
            Some(format!(
                "{} = {}\n",
                Value::from(key.as_str()),
                inline(value)?
            ))
        })
        .collect()
}

/// The table `key` of a file written as TOML text: `[key]` with its
/// entries, or `[[key]]` once for each of an array of tables.
fn part_text(key: &str, value: &toml::Value) -> Option<String> {
    match value {
        toml::Value::Table(table) => Some(format!("[{key}]\n{}", entries_of(table)?)),
        toml::Value::Array(items) => items
            .iter()
            .map(|item| Some(format!("[[{key}]]\n{}", entries_of(item.as_table()?)?)))
            .collect(),
        _ => None,
    }
}

/// An old skill's start and timing ([`TIMING`]) as its own `skill.toml`
/// writes them, its lines as they are where they can be taken out whole,
/// else written again with the very same values, for its remake to start
/// exactly as it did (`made_skill::RemakeAs`). None when it has no
/// `[trigger]` and no `asks` (one only a tool starts), or a `[said]` start.
pub fn kept_timing(toml: &str) -> Option<String> {
    let table: toml::Table = toml::from_str(toml).ok()?;
    let started = table.get("trigger").is_some_and(|trigger| {
        trigger.is_table() || trigger.as_array().is_some_and(|all| !all.is_empty())
    }) || table.get("asks").is_some_and(toml::Value::is_array);
    if !started || table.contains_key("tool") || table.contains_key("said") {
        return None;
    }
    let timing = timing_of(&table);
    let (mut top, mut parts) = (Vec::new(), Vec::new());
    let mut part: Option<String> = None;
    for line in toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && !trimmed.contains('=') {
            part = Some(trimmed.trim_matches(['[', ']']).trim().to_owned());
        }
        match part.as_deref() {
            None => {
                if trimmed
                    .split_once('=')
                    .is_some_and(|(key, _)| TOP_TIMING.contains(&key.trim()))
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
    if toml::from_str::<toml::Table>(&text).is_ok_and(|kept| kept == timing) {
        return Some(text);
    }
    // Lines that can't be taken out whole (an array over many lines, a
    // note on a header's line): the same values written again.
    let mut text = String::new();
    for key in TOP_TIMING {
        if let Some(value) = timing.get(key) {
            text.push_str(&format!("{key} = {}\n", inline(value)?));
        }
    }
    for key in ["trigger", "active", "quiet"] {
        if let Some(value) = timing.get(key) {
            text.push_str(&format!("\n{}", part_text(key, value)?));
        }
    }
    let text = text.trim().to_owned();
    toml::from_str::<toml::Table>(&text)
        .is_ok_and(|kept| kept == timing)
        .then_some(text)
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

/// The step kinds an old skill's steps kept beside its task may have
/// ([`with_old_steps`]): none that writes, hands a task over or runs code
/// of its own, which the remade folder doesn't hold.
fn keeps_as_old_steps(table: &toml::Table) -> bool {
    !table.contains_key("grants")
        && table
            .get("do")
            .and_then(toml::Value::as_array)
            .is_some_and(|steps| {
                !steps.is_empty()
                    && steps.iter().all(|step| {
                        step.as_table().is_some_and(|step| {
                            !step.contains_key("task") && !step.contains_key("write")
                        })
                    })
            })
}

/// `new`, a remade skill's `skill.toml`, with the old skill's (`old`) own
/// steps kept beside its task as `[[otherwise]]` (phase 2, the first
/// builder's 06:08), run by lamp-skills in the agent's place when its
/// answer isn't ready within 10 s of the fire or the agent is down
/// (`lamp_skills::Engine::run_otherwise`), and its `[permissions]` taking
/// in the old one's, so the old steps may do what they did. None when the
/// old steps can't be kept so (one that writes, hands a task over or has
/// code), or the text written isn't the very same skill with them.
pub fn with_old_steps(new: &str, old: &str) -> Option<String> {
    let new_table: toml::Table = toml::from_str(new).ok()?;
    let old_table: toml::Table = toml::from_str(old).ok()?;
    if !keeps_as_old_steps(&old_table) || new_table.contains_key("otherwise") {
        return None;
    }
    let mut permissions = match new_table.get("permissions") {
        None => toml::Table::new(),
        Some(toml::Value::Table(permissions)) => permissions.clone(),
        Some(_) => return None,
    };
    if let Some(old) = old_table.get("permissions") {
        for (key, value) in old.as_table()? {
            match (permissions.get_mut(key), value) {
                (None, value) => {
                    permissions.insert(key.clone(), value.clone());
                }
                (Some(toml::Value::Boolean(on)), toml::Value::Boolean(old)) => *on |= *old,
                (Some(toml::Value::Array(given)), toml::Value::Array(old)) => {
                    for item in old {
                        if !given.contains(item) {
                            given.push(item.clone());
                        }
                    }
                }
                (Some(given), value) if given == value => {}
                _ => return None,
            }
        }
    }
    // The new one's own `[permissions]` taken out, written again whole
    // with the old one's in it, at the end beside the old steps.
    let lines: Vec<&str> = new.lines().collect();
    let head = lines.iter().position(|line| line.trim() == "[permissions]");
    let kept: Vec<&str> = match head {
        Some(head) => {
            let end = lines[head + 1..]
                .iter()
                .position(|line| line.trim_start().starts_with('['))
                .map_or(lines.len(), |at| head + 1 + at);
            lines[..head].iter().chain(&lines[end..]).copied().collect()
        }
        None if new_table.contains_key("permissions") => return None,
        None => lines,
    };
    let mut text = kept.join("\n");
    text.push('\n');
    if !permissions.is_empty() {
        text.push_str(&format!("\n[permissions]\n{}", entries_of(&permissions)?));
    }
    text.push_str(&format!(
        "\n{}",
        part_text("otherwise", old_table.get("do")?)?
    ));
    let mut expected = new_table;
    if !permissions.is_empty() {
        expected.insert("permissions".into(), toml::Value::Table(permissions));
    }
    expected.insert("otherwise".into(), old_table.get("do")?.clone());
    toml::from_str::<toml::Table>(&text)
        .is_ok_and(|written| written == expected)
        .then_some(text)
}

/// One old skill to remake, as [`chosen`] picked it: its task's words
/// ([`task_of`]), its start and timing ([`kept_timing`]), and, for a
/// working one (phase 2), its old `skill.toml`, whose steps are kept beside
/// its task ([`with_old_steps`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pick {
    pub task: String,
    pub timing: String,
    /// Broken ([`broken`]): nothing of the old steps is kept.
    pub broken: bool,
    pub old_steps: Option<String>,
    /// A broken quick reaction or lamp-only fire: remade by the lamp's own
    /// model as one still, its left-out part added ([`Quick`]); no task.
    pub quick: Option<Quick>,
}

/// A broken quick reaction or lamp-only fire to remake as one still (the
/// first builder's 10:29 on #7814): its old `skill.toml`, the part its
/// description names as left out, the person's own words a value of it
/// may come from (its kept request, else its first `asks` phrase, else its
/// said-back), and its said-back with the left-out part taken out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quick {
    pub old: String,
    pub part: String,
    pub words: String,
    pub said: String,
}

/// The lamp's own model's brief for a [`Quick`] remake: add the left-out
/// part's step, from the person's words alone, or answer `none`.
pub const QUICK_SYSTEM: &str = "You fix a desk lamp's skill. Its skill.toml left out a part. Add \
    only the step or steps for that part, each a [[do]] table of light, body, cue or sound, and \
    change nothing else: every line it has stays exactly as it is. Every value you add must be \
    one the person's words name, as they wrote it, from the first of their lines that names one; \
    never choose one they didn't. If their words name no value for the part, answer only: none. Otherwise answer only the whole skill.toml, \
    in one ```skill.toml fence.";

/// The step kinds a [`Quick`] remake may add: the lamp's own, never a say,
/// a read, a write or a task.
const QUICK_STEPS: [&str; 4] = ["light", "body", "cue", "sound"];

/// What the lamp's own model is asked for `quick`.
pub fn quick_brief(quick: &Quick) -> String {
    format!(
        "skill.toml:\n```skill.toml\n{}```\n\nLeft out: {}.\n\nThe person's words:\n{}\n",
        quick.old, quick.part, quick.words
    )
}

/// Whether every word of `value` is a word of `words`, in any case: a
/// value the person named, never one guessed.
fn named_in(value: &str, words: &str) -> bool {
    let split = |text: &str| -> Vec<String> {
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    let said = split(words);
    let value = split(value);
    !value.is_empty() && value.iter().all(|word| said.contains(word))
}

/// Whether every value in an added step is a string the person named
/// ([`named_in`]): no number, flag or table of the model's own.
fn values_named(value: &toml::Value, words: &str) -> bool {
    match value {
        toml::Value::String(text) => named_in(text, words),
        toml::Value::Table(table) => table.values().all(|value| values_named(value, words)),
        toml::Value::Array(values) => values.iter().all(|value| values_named(value, words)),
        _ => false,
    }
}

/// The lamp's own model's `answer` for `quick`, checked: the old skill
/// exactly, but its added steps (at least one, each of [`QUICK_STEPS`],
/// every value named in the person's words), and the grants of what they
/// use; still a quick reaction or a lamp-only fire as it was. Its
/// `skill.toml`, or why not (journaled, never told).
pub fn quick_fixed(quick: &Quick, answer: &str) -> Result<String, &'static str> {
    const NOT_FIXED: &str = "its left-out part wasn't added from the person's own words";
    let toml = answer
        .split("```skill.toml")
        .nth(1)
        .and_then(|rest| rest.split("```").next())
        .map(str::trim_start)
        .ok_or(NOT_FIXED)?;
    let new: toml::Table = toml::from_str(toml).map_err(|_| NOT_FIXED)?;
    let old: toml::Table = toml::from_str(&quick.old).map_err(|_| NOT_FIXED)?;
    for (key, value) in &old {
        if !matches!(key.as_str(), "do" | "permissions") && new.get(key) != Some(value) {
            return Err(NOT_FIXED);
        }
    }
    if new
        .keys()
        .any(|key| !matches!(key.as_str(), "do" | "permissions") && !old.contains_key(key))
    {
        return Err(NOT_FIXED);
    }
    // Its permissions: the old ones, and only the lamp's own added.
    let empty = toml::Table::new();
    let permissions = |table: &toml::Table| {
        table
            .get("permissions")
            .and_then(toml::Value::as_table)
            .cloned()
            .unwrap_or_else(|| empty.clone())
    };
    let (was, now) = (permissions(&old), permissions(&new));
    if was.iter().any(|(key, value)| now.get(key) != Some(value))
        || now.iter().any(|(key, value)| {
            let lamps_own = QUICK_STEPS.contains(&key.as_str()) && value.as_bool() == Some(true);
            !(was.contains_key(key) || lamps_own)
        })
    {
        return Err(NOT_FIXED);
    }
    // Its steps: the old ones in order, with the added ones among them.
    let steps = |table: &toml::Table| {
        table
            .get("do")
            .and_then(toml::Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let (old_steps, new_steps) = (steps(&old), steps(&new));
    let mut left = old_steps.iter().peekable();
    let mut added = 0;
    for step in &new_steps {
        if left.peek() == Some(&step) {
            left.next();
            continue;
        }
        let Some(table) = step.as_table() else {
            return Err(NOT_FIXED);
        };
        if table.is_empty()
            || !table.iter().all(|(key, value)| {
                QUICK_STEPS.contains(&key.as_str()) && values_named(value, &quick.words)
            })
        {
            return Err(NOT_FIXED);
        }
        added += 1;
    }
    if left.next().is_some() || added == 0 {
        return Err(NOT_FIXED);
    }
    let still = if made_skill::quick_reaction(&quick.old) {
        made_skill::quick_reaction(toml)
    } else {
        lamp_only_fire(&new)
    };
    if !still {
        return Err(NOT_FIXED);
    }
    Ok(toml.to_owned())
}

/// The person's own words a [`Quick`] remake's values may come from, in
/// the first builder's order (10:29), one a line: the kept request, then
/// the first `asks` phrase, then the said-back with its left-out part
/// taken out; a value is taken from the first that names one
/// ([`QUICK_SYSTEM`]). None with none of them.
fn quick_words(entry: &Value, table: &toml::Table, said: &str) -> Option<String> {
    let request = entry["request"].as_str().map(str::trim);
    let asks = table
        .get("asks")
        .and_then(toml::Value::as_array)
        .and_then(|asks| asks.iter().find_map(toml::Value::as_str))
        .map(str::trim);
    let lines: Vec<String> = [
        ("Their request", request),
        ("Their words to start it", asks),
        ("Its description", Some(said.trim())),
    ]
    .into_iter()
    .filter_map(|(head, words)| {
        let words = words.filter(|words| !words.is_empty())?;
        Some(format!("{head}: {words}"))
    })
    .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// Why the kept made skill `entry` is not one to remake: `None` when it is
/// no candidate at all (the switch off, not the person's, not DeepSeek's,
/// changed, a quick reaction, a lamp-only fire, or, with `on` alone, one
/// that isn't broken); `Some` of a reason to journal when it would be, but
/// is left as it is (no words for its task, its start can't be kept, or its
/// old steps can't be kept beside it).
fn picked(keeping: &Keeping, entry: &Value) -> Result<Pick, Option<&'static str>> {
    let Some(scope) = picking() else {
        return Err(None);
    };
    let Some(name) = entry["name"].as_str().filter(|name| !name.is_empty()) else {
        return Err(None);
    };
    let from = entry["from"].as_str().unwrap_or_default();
    // Made here, by voice or in the app: never an upload or a Store skill.
    // Never one remade already, its old one kept or past its week.
    if entry["on"] != true
        || entry.get("added").is_some()
        || entry.get("old").is_some()
        || entry.get("remade_from").is_some()
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
    let Ok(table) = toml::from_str::<toml::Table>(&toml) else {
        return Err(None);
    };
    // Broken or not, by a check of the skill alone, first (06:07; the
    // first builder's 10:29 on #7814: a broken quick reaction is fixed).
    let broken = broken(entry, &table);
    let quick = made_skill::quick_reaction(&toml);
    let lamp_only = lamp_only_fire(&table);
    // A skill that runs as its owner heard it is never touched in phase 1,
    // and a working quick reaction or lamp-only fire (a pat, a glow, a nod,
    // a chime) never at all (01:29, 06:08).
    if !broken && (scope == Scope::Broken || quick || lamp_only) {
        return Err(None);
    }
    // A broken one stays a quick reaction or a lamp-only fire: the lamp's
    // own model adds its left-out part, from the person's own words; no
    // task, no agent (10:29). A stand-in say is never one of these.
    if quick || lamp_only {
        let description = entry["description"].as_str().unwrap_or_default().trim();
        let Some((at, part)) = left_out_at(description) else {
            return Err(Some("its broken part isn't named"));
        };
        let said = description[..at].trim().to_owned();
        let Some(words) = quick_words(entry, &table, &said) else {
            return Err(Some(
                "no request, asks or description of theirs gives its left-out part",
            ));
        };
        let Some(timing) = kept_timing(&toml) else {
            return Err(Some("its trigger and timing can't be kept as they are"));
        };
        return Ok(Pick {
            task: part.to_owned(),
            timing,
            broken,
            old_steps: None,
            quick: Some(Quick {
                old: toml,
                part: part.to_owned(),
                words,
                said,
            }),
        });
    }
    let Some(task) = task_of(entry, &table) else {
        return Err(Some(
            "no request, asks or description of theirs gives its task",
        ));
    };
    let Some(timing) = kept_timing(&toml) else {
        return Err(Some("its trigger and timing can't be kept as they are"));
    };
    // A working one keeps its old steps to run in the agent's place.
    let old_steps = if broken {
        None
    } else {
        if entry["uses"]["code"] == true || !keeps_as_old_steps(&table) {
            return Err(Some(OLD_STEPS_NOT_KEPT));
        }
        Some(toml)
    };
    Ok(Pick {
        task,
        timing,
        broken,
        old_steps,
        quick: None,
    })
}

/// The kept made skills to remake, oldest first, each with what it was
/// picked as.
pub fn chosen(keeping: &Keeping) -> Vec<(Value, Pick)> {
    made_skill::entries(&keeping.directory)
        .into_iter()
        .filter_map(|entry| {
            let pick = picked(keeping, &entry).ok()?;
            Some((entry, pick))
        })
        .collect()
}

/// The names of [`chosen`], each one left as it is journaled with why (no
/// word of the skill).
pub fn chosen_names(keeping: &Keeping) -> Vec<String> {
    made_skill::entries(&keeping.directory)
        .iter()
        .filter(|entry| match picked(keeping, entry) {
            Ok(_) => true,
            Err(Some(why)) => {
                eprintln!("lamp-voice: an old made skill is left as it is, not remade: {why}");
                false
            }
            Err(None) => false,
        })
        .filter_map(|entry| entry["name"].as_str().map(str::to_owned))
        .collect()
}

/// The one line once they are remade (the first builder, 06:07: "I fixed
/// your Zorbo coin skill so it looks up the price. Say 'put it back' to
/// undo."): the real count and one real name, `moved` the ones kept with
/// their old steps (phase 2). Nothing is asked.
pub fn told_line(names: &[String], moved: &BTreeSet<String>) -> Option<String> {
    let first = made_skill::spoken_name(names.first()?);
    let fixed = names.iter().all(|name| !moved.contains(name));
    Some(match (names.len(), fixed) {
        (1, true) => format!(
            "I fixed your {first} skill so it does what you asked. Say 'put my old {first} skill back' to undo."
        ),
        (count, true) => format!(
            "I fixed {count} of your skills so they do what you asked. Say 'put my {first} skill \
             back' to undo."
        ),
        (1, false) => format!(
            "I moved your {first} skill to your agent; it starts when it did, and does what it \
             did before if your agent is slow. Say 'put my old {first} skill back' to undo."
        ),
        (count, false) => format!(
            "I moved {count} of your skills to your agent; each starts when it did, and does what \
             it did before if your agent is slow. Say 'put my {first} skill back' to undo."
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
    /// Of [`State::untold`], the working ones kept with their old steps
    /// (phase 2), told as moved rather than fixed ([`told_line`]).
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub moved: BTreeSet<String>,
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
    let line = told_line(&state.untold, &state.moved)?;
    // Marked before it is told: told at most once.
    state.untold.clear();
    state.moved.clear();
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
    /// A spoken turn is open, or ended within [`TURN_QUIET_FOR`].
    fn turn_open(&self) -> bool;
    /// The board is hot: a zone over [`HOT_MILLI_C`].
    fn hot(&self) -> bool;
}

/// How long after a spoken turn ends a remake still waits (the first
/// builder's 10:47 on #7814: a remake is an agent make, about 100 s of a
/// busy core, so it runs only in idle time).
pub const TURN_QUIET_FOR: Duration = Duration::from_secs(15);

/// Above this, in thousandths of a degree, the board is hot and no remake
/// begins (10:47; no `board/heat` in this base, so the board's own zones).
pub const HOT_MILLI_C: i64 = 84_000;

/// The spoken turns open now, and when the last one ended, by the lamp's
/// clock ([`TurnOpen`]).
static TURNS_OPEN: AtomicU64 = AtomicU64::new(0);
static TURN_ENDED_MS: AtomicU64 = AtomicU64::new(0);

/// Held while a spoken turn is open (`second_provider::spoken`): no remake
/// begins until it is dropped and [`TURN_QUIET_FOR`] has passed.
pub struct TurnOpen(());

impl TurnOpen {
    pub fn begin() -> Self {
        TURNS_OPEN.fetch_add(1, Ordering::AcqRel);
        Self(())
    }
}

impl Drop for TurnOpen {
    fn drop(&mut self) {
        TURN_ENDED_MS.store(now_unix_ms(), Ordering::Release);
        TURNS_OPEN.fetch_sub(1, Ordering::AcqRel);
    }
}

/// The hottest of the board's zones under `sys`
/// (`class/thermal/thermal_zone*/temp`), in thousandths of a degree; none
/// read, none.
fn hottest_milli_c(sys: &Path) -> Option<i64> {
    fs::read_dir(sys.join("class/thermal"))
        .ok()?
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("thermal_zone"))
        })
        .filter_map(|entry| {
            fs::read_to_string(entry.path().join("temp"))
                .ok()?
                .trim()
                .parse::<i64>()
                .ok()
                .filter(|milli_c| (-273_150..=1_000_000).contains(milli_c))
        })
        .max()
}

/// What a remake does on the lamp: the lamp's own, or a test's.
pub trait Hands {
    /// Whether the lamp has an agent to do a remade skill's task: none, no
    /// remake is tried (Twelve's P3 on #7820).
    fn agent_here(&self) -> bool;
    /// `pick`'s task made in the middle form, one task for the lamp's
    /// agent starting exactly as its timing (the old skill's own,
    /// [`kept_timing`]) says, with sip-of-water's guards but on a clock or
    /// the owner's words, and a working one's old steps beside it
    /// ([`with_old_steps`]), kept under `name` and not on; and the agent
    /// that does its task, as `made_with` says it.
    fn make(&self, keeping: &Keeping, pick: &Pick, name: &str) -> Result<(Made, String), String>;
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
    // Idle time alone, never on a warm board (the first builder's 10:47
    // on #7814): a remake is about 100 s of a busy core.
    if when.turn_open() {
        return Step::Waits("a turn is open");
    }
    if when.hot() {
        return Step::Waits("the board is hot");
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
    let Some((old, pick)) = chosen(keeping)
        .into_iter()
        .find(|(entry, _)| entry["name"] == name.as_str())
    else {
        // Changed, removed or remade since the yes: left as it is.
        state.queue.retain(|queued| *queued != name);
        let _ = save(&keeping.directory, &state);
        return Step::NotKept("no longer one to remake");
    };
    // No agent for its task on this lamp: nothing is tried, no try is
    // counted, and the old skills stay (Twelve's P3 on #7820). A quick
    // remake needs none.
    if pick.quick.is_none() && !hands.agent_here() {
        return Step::Waits("no agent for its task here");
    }
    state.begun_today += 1;
    if save(&keeping.directory, &state).is_err() {
        return Step::Waits("the record can't be kept");
    }
    let done = remake(keeping, hands, &old, &pick, now_ms);
    let mut state = load(&keeping.directory);
    let step = match done {
        Ok(()) => {
            state.queue.retain(|queued| *queued != name);
            state.failed.remove(&name);
            state.untold.push(name.clone());
            if pick.old_steps.is_some() {
                state.moved.insert(name.clone());
            }
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
    pick: &Pick,
    now_ms: u64,
) -> Result<(), &'static str> {
    let name = old["name"].as_str().unwrap_or_default();
    let old_sha = old["sha256"].as_str().unwrap_or_default();
    let was_off = hands.turned_off().contains(name);
    let made = hands.make(keeping, pick, name);
    // A person's make came meanwhile: nothing of this one is kept.
    let stood_down = STOOD_DOWN.load(Ordering::Acquire);
    let (made, engine) = made.map_err(|why| {
        if stood_down || why == made_skill::AGENT_BUSY_TRY {
            STOOD_DOWN_WHY
        } else if why == made_skill::REMAKE_TIMING_CHANGED {
            made_skill::REMAKE_TIMING_CHANGED
        } else if why == OLD_STEPS_NOT_KEPT {
            OLD_STEPS_NOT_KEPT
        } else if pick.quick.is_some() {
            "its fix wasn't kept"
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
    // A quick remake has no task, so no yes to keep (10:29).
    let on = if pick.quick.is_some() {
        if made.uses.code || made.uses.agent_task {
            return unput("its fix has code, which waits for a yes");
        }
        hands.put_on(keeping, &made.sha256).map(|()| true)
    } else if crate::make_no_asks::on() {
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

    fn turn_open(&self) -> bool {
        TURNS_OPEN.load(Ordering::Acquire) > 0
            || self
                .now_ms()
                .saturating_sub(TURN_ENDED_MS.load(Ordering::Acquire))
                < TURN_QUIET_FOR.as_millis() as u64
    }

    fn hot(&self) -> bool {
        hottest_milli_c(Path::new("/sys")).is_some_and(|milli_c| milli_c > HOT_MILLI_C)
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

    fn make(&self, keeping: &Keeping, pick: &Pick, name: &str) -> Result<(Made, String), String> {
        // The middle form (19:50): written by the lamp, its start the old
        // skill's own (01:22); the agent only does its task when it fires,
        // so one must be on the lamp, but it is never asked to write it,
        // and lamp-tasks' make slot is never taken. A working one's old
        // steps kept beside its task (06:08). A broken quick reaction or
        // lamp-only fire: the lamp's own model adds its left-out part, and
        // it stays one (10:29).
        let from = format!("local:remake:{}", now_unix_ms());
        if let Some(quick) = &pick.quick {
            let _as = made_skill::RemakeAs::set(name, &pick.timing, None);
            return crate::make_skill_local::remade_as_quick(
                quick,
                name,
                keeping,
                &from,
                crate::make_skill_local::own_chat,
            )
            .map(|made| (made, made_skill::MadeWith::DeepSeek.key().to_owned()));
        }
        let engine = lamps_engine().ok_or("no agent for its task here")?;
        let _as = made_skill::RemakeAs::set(name, &pick.timing, pick.old_steps.as_deref());
        crate::make_skill_local::remade_as_task(
            &pick.task,
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
        let put = || {
            made_skill::put_on_within(
                &keeping.directory,
                &keeping.manage,
                sha256,
                now_unix_ms(),
                HELPER_WAIT,
            )
        };
        match put() {
            // No answer within [`HELPER_WAIT`]: the helper may still be
            // putting it on, and an `unput` then would delete the entry of
            // a skill that went on over the old one (Twelve's P3 on #7765).
            // Asked once more: one on by then is said on at once, one that
            // isn't is put on or refused as any other.
            Err(why) if why == made_skill::NOT_SAID_IN_TIME => {
                eprintln!(
                    "lamp-voice: a made skill's remake: the helper didn't say in time: asked again"
                );
                put().map(|_| ())
            }
            put => put.map(|_| ()),
        }
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

/// Whether a remade skill's old one is kept in `directory` (its entry's
/// `old`): its "put it back" stays offered and it ages out with the switch
/// off too (Thirteen's P3 on #7765).
pub fn undo_kept(directory: &Path) -> bool {
    made_skill::entries(directory)
        .iter()
        .any(|entry| entry.get("old").is_some())
}

/// Whether `make_skill`'s [`PUT_BACK`] is offered and taken here: with the
/// switch on, or while a remade skill's old one is kept ([`undo_kept`]).
pub fn put_back_offered() -> bool {
    on() || Keeping::when(true).is_some_and(|keeping| undo_kept(&keeping.directory))
}

/// The remade skill in `directory` whose remake is the latest thing done
/// to the person's skills at `now_ms`: its old one still kept within
/// [`UNDO_FOR`], and replaced after every action `make_no_asks`' undo
/// keeps (`make_no_asks::latest_at`). Then "put it back" is its
/// [`PUT_BACK`], never that undo's "last" (Twelve's P2 on #7765: a remove
/// 3 days old came back while the skill remade tonight stayed).
pub fn latest_remade(directory: &Path, now_ms: u64) -> Option<String> {
    let (at, name) = made_skill::entries(directory)
        .iter()
        .filter_map(|entry| {
            let at = entry["old"]["replaced_at_ms"].as_u64()?;
            let name = entry["name"].as_str()?;
            (now_ms < at + UNDO_FOR.as_millis() as u64).then(|| (at, name.to_owned()))
        })
        .max()?;
    let done = crate::make_no_asks::latest_at(directory, now_ms);
    done.is_none_or(|done| at > done).then_some(name)
}

/// The remade skill `make_skill`'s undo `asked` ("last", or a name as
/// heard) is for here, when a remake is the latest thing done
/// ([`latest_remade`]): "last", or that skill's own name. Then the call is
/// its [`PUT_BACK`]; else none, and the undo is `make_no_asks`' own.
pub fn put_back_for_undo(directory: &Path, asked: &str, now_ms: u64) -> Option<String> {
    let name = latest_remade(directory, now_ms)?;
    let asked = asked.trim();
    let last = asked.is_empty() || asked.eq_ignore_ascii_case(crate::make_no_asks::LAST);
    let heard = made_skill::name_asked(asked);
    let heard = made_skill::name_digits(heard.strip_prefix("old-").unwrap_or(&heard));
    (last || heard == made_skill::name_digits(&name)).then_some(name)
}

/// [`put_back_for_undo`] on the lamp, while [`PUT_BACK`] is offered.
pub fn put_back_for_undo_here(asked: &str) -> Option<String> {
    if !put_back_offered() {
        return None;
    }
    let keeping = Keeping::when(true)?;
    put_back_for_undo(&keeping.directory, asked, now_unix_ms())
}

/// With the switch off: the old ones past [`UNDO_FOR`] dropped
/// ([`drop_old`]), as the pass does. Whether any is still kept, to look
/// again later.
pub fn age_out(keeping: &Keeping, now_ms: u64) -> bool {
    drop_old(keeping, now_ms);
    undo_kept(&keeping.directory)
}

/// With the switch off, while a remade skill's old one is kept: a look
/// every [`LOOK_EVERY`] that ages them out ([`age_out`]), ended once none
/// is kept. None kept, no thread.
fn start_aging() {
    let Some(keeping) = Keeping::when(true) else {
        return;
    };
    if !undo_kept(&keeping.directory) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("lamp-voice-remake-undo".into())
        .spawn(move || {
            while age_out(&keeping, now_unix_ms()) {
                std::thread::sleep(LOOK_EVERY);
            }
        });
    if let Err(error) = spawned {
        eprintln!("lamp-voice second: the remade skills' undo aging didn't start: {error}");
    }
}

/// Start the pass, once a process, with the switch on: a look every
/// [`LOOK_EVERY`], on a thread of its own. Off, only the kept old ones
/// age out ([`start_aging`]).
pub fn start() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    if !on() {
        start_aging();
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
        /// The old steps each make was given to keep beside its task.
        old_steps: RefCell<Vec<Option<String>>>,
        /// The lamp's own model's answer to a quick remake (`none` unset).
        quick_answer: Option<String>,
        /// The `skill.toml` each quick remake kept.
        quick_kept: RefCell<Vec<String>>,
        /// The helper refuses to put anything on (Twelve's P2 on #7765).
        put_fails: Cell<bool>,
    }

    impl Hands for Fake {
        fn agent_here(&self) -> bool {
            !self.no_agent
        }

        fn make(
            &self,
            keeping: &Keeping,
            pick: &Pick,
            name: &str,
        ) -> Result<(Made, String), String> {
            let request = pick.task.as_str();
            self.asked.borrow_mut().push(request.to_owned());
            self.timings.borrow_mut().push(pick.timing.clone());
            self.old_steps.borrow_mut().push(pick.old_steps.clone());
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
            // A quick remake: the lamp's own model's answer, checked as the
            // lamp checks it, kept with no code and no task.
            if let Some(quick) = &pick.quick {
                let answer = self.quick_answer.as_deref().unwrap_or("none");
                let toml = quick_fixed(quick, answer).map_err(str::to_owned)?;
                self.quick_kept.borrow_mut().push(toml.clone());
                let (sha, package) = package_of(keeping, name, &toml);
                push_entry(
                    keeping,
                    json!({
                        "from": "local:remake:1", "name": name, "version": "1.0.0",
                        "sha256": sha, "toml_sha256": hex_of(toml.as_bytes()),
                        "description": quick.said, "uses": {"light": true},
                        "package": package,
                    }),
                );
                return Ok((
                    Made {
                        name: name.to_owned(),
                        version: "1.0.0".into(),
                        sha256: sha,
                        description: quick.said.clone(),
                        uses: made_skill::Uses::default(),
                        package: keeping.directory.join(package),
                    },
                    "deepseek".into(),
                ));
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
            if self.put_fails.get() {
                return Err("the lamp's check didn't let it on".into());
            }
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
        turn_open: bool,
        hot: bool,
    }

    impl Clock {
        fn day_idle() -> Self {
            Self {
                now_ms: 100 * DAY_MS + 10 * HOUR_MS,
                quiet: false,
                idle: IDLE_FOR,
                turn_open: false,
                hot: false,
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
        fn turn_open(&self) -> bool {
            self.turn_open
        }
        fn hot(&self) -> bool {
            self.hot
        }
    }

    /// The first builder's 10:47 on #7814: a remake waits while a spoken
    /// turn is open or ended within 15 s, nothing tried and no try counted,
    /// and runs once it is idle.
    #[test]
    fn a_remake_waits_while_a_turn_is_open() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("turn-open");
        old_skill(&keeping, "water", PRESENCE, "local:1");
        let fake = Fake::default();
        let mut clock = Clock::day_idle();
        clock.turn_open = true;
        assert_eq!(step(&keeping, &fake, &clock), Step::Waits("a turn is open"));
        assert!(fake.asked.borrow().is_empty());
        assert_eq!(load(&keeping.directory).begun_today, 0);
        clock.turn_open = false;
        assert_eq!(
            step(&keeping, &fake, &clock),
            Step::Kept("water".to_owned())
        );
        // The lamp's own: open while held, and 15 s after.
        {
            let _open = TurnOpen::begin();
            assert!(Here.turn_open());
        }
        assert!(Here.turn_open());
        TURN_ENDED_MS.store(
            now_unix_ms() - TURN_QUIET_FOR.as_millis() as u64,
            Ordering::Release,
        );
        assert!(!Here.turn_open());
    }

    /// The first builder's 10:47 on #7814: a remake waits while the board
    /// is hot (a zone over 84 °C), nothing tried and no try counted.
    #[test]
    fn a_remake_waits_while_hot() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (root, keeping) = lamp("hot");
        old_skill(&keeping, "water", PRESENCE, "local:1");
        let fake = Fake::default();
        let mut clock = Clock::day_idle();
        clock.hot = true;
        assert_eq!(
            step(&keeping, &fake, &clock),
            Step::Waits("the board is hot")
        );
        assert!(fake.asked.borrow().is_empty());
        assert_eq!(load(&keeping.directory).begun_today, 0);
        clock.hot = false;
        assert_eq!(
            step(&keeping, &fake, &clock),
            Step::Kept("water".to_owned())
        );
        // The board's zones, the hottest of them.
        let sys = root.join("sys");
        for (zone, milli_c) in [("thermal_zone0", "61000\n"), ("thermal_zone1", "84500\n")] {
            let dir = sys.join("class/thermal").join(zone);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("temp"), milli_c).unwrap();
        }
        assert_eq!(hottest_milli_c(&sys), Some(84_500));
        assert_eq!(hottest_milli_c(&root.join("none")), None);
    }

    /// A lamp's made skills' folders, the pass picking with `all` on this
    /// thread (every old skill that isn't a quick reaction or a lamp-only
    /// fire), unless a test picks otherwise ([`set_picking_here`]).
    fn lamp(test: &str) -> (PathBuf, Keeping) {
        let (root, keeping) = keeping(&format!("remake-{test}"));
        fs::create_dir_all(&keeping.directory).unwrap();
        fs::create_dir_all(&keeping.installed).unwrap();
        set_picking_here(Some(Some(Scope::All)));
        (root, keeping)
    }

    /// An old skill kept with `description`, and `request` or none.
    fn old_skill_said(
        keeping: &Keeping,
        name: &str,
        toml: &str,
        request: Option<&str>,
        description: Option<&str>,
    ) {
        old_skill_asked(keeping, name, toml, "local:1", request);
        let mut entries = made_skill::entries(&keeping.directory);
        let entry = entries.last_mut().unwrap();
        match description {
            Some(said) => entry["description"] = json!(said),
            None => {
                entry.as_object_mut().unwrap().remove("description");
            }
        }
        made_skill::write_entries(&keeping.directory, &entries).unwrap();
    }

    /// lamp-9adf, 18 Nov: "tell me the price of Zorbo coin" kept as a say
    /// of DeepSeek's own stand-in, read aloud as it stood.
    const ZORBO: &str = "kind = \"installable\"\nversion = \"1.0.0\"\n\
                         permissions = { speak = true }\n\
                         asks = [\"tell me the price of Zorbo coin\"]\n\
                         [ask]\npolicy = \"nudge\"\n\
                         [[do]]\nsay = { text = \"internet: current price of Zorbo coin\" }\n";
    /// A focus mode on the clock whose make left its colour out, as its
    /// said-back says.
    const FOCUS: &str = "kind = \"installable\"\nversion = \"1.0.0\"\n\n[trigger]\n\
        topic = \"clock/local\"\nfield = \"known\"\nequals = true\nrearm = \"ends\"\n\
        during = { from = \"09:00\", to = \"09:01\" }\n\n[permissions]\nlight = true\n\n\
        [[do]]\nlight = { level = 40 }\n";
    const FOCUS_SAID: &str = "At 9 every morning, it dims the light for focus. Left out: its \
                              colour in a step that changes the light.";
    /// "Every night at 9, glow blue and say it's moon time": a fixed say on
    /// the clock, no quick reaction, that runs as its owner heard it.
    const MOON: &str = "kind = \"installable\"\nversion = \"1.0.0\"\n\n[trigger]\n\
        topic = \"clock/local\"\nfield = \"known\"\nequals = true\nrearm = \"ends\"\n\
        during = { from = \"21:00\", to = \"21:01\" }\n\n\
        [permissions]\nspeak = true\nlight = true\n\n[[do]]\nlight = { colour = \"blue\" }\n\n\
        [[do]]\nsay = { text = \"It's moon time.\" }\n";
    const MOON_SAID: &str = "Every night at 9, it glows blue and says it's moon time.";
    /// A glow at 9, a lamp-only fire.
    const GLOW: &str = "kind = \"installable\"\nversion = \"1.0.0\"\n\n[trigger]\n\
        topic = \"clock/local\"\nfield = \"known\"\nequals = true\nrearm = \"ends\"\n\
        during = { from = \"09:00\", to = \"09:01\" }\n\n[[do]]\nlight = { colour = \"warm\" }\n\n\
        [[do]]\ncue = \"chime\"\n";

    /// The first builder, 06:07: the zorbo-coin-price skill, whose say is
    /// DeepSeek's stand-in for a price it never fetched, is broken by a
    /// check of the skill alone; its task is its owner's own asks phrase,
    /// and its start is its asks, kept exactly.
    #[test]
    fn a_say_of_the_stand_in_is_broken_and_its_task_is_its_asks() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("zorbo");
        set_picking_here(Some(Some(Scope::Broken)));
        old_skill_said(
            &keeping,
            "zorbo-coin-price",
            ZORBO,
            None,
            Some("Tells you the price of Zorbo coin."),
        );
        let table: toml::Table = toml::from_str(ZORBO).unwrap();
        assert!(stand_in_say(&table));
        let chosen = chosen(&keeping);
        assert_eq!(chosen.len(), 1, "{chosen:?}");
        let pick = &chosen[0].1;
        assert!(pick.broken && pick.old_steps.is_none());
        assert_eq!(pick.task, "tell me the price of Zorbo coin");
        assert!(same_timing(&pick.timing, ZORBO), "{}", pick.timing);
        let fake = Fake::default();
        assert_eq!(
            step(&keeping, &fake, &Clock::day_idle()),
            Step::Kept("zorbo-coin-price".to_owned())
        );
        assert_eq!(
            *fake.asked.borrow(),
            vec!["tell me the price of Zorbo coin".to_owned()]
        );
        assert_eq!(*fake.old_steps.borrow(), vec![None]);
        // The check is narrow: only a say's text that begins with the
        // marker, with words after it.
        for (text, stand_in) in [
            ("internet: current price of Zorbo coin", true),
            ("INTERNET: the weather", true),
            ("internet:", false),
            ("the internet: what a place", false),
            ("Look it up on the internet: later.", false),
        ] {
            let toml = ZORBO.replace("internet: current price of Zorbo coin", text);
            let table: toml::Table = toml::from_str(&toml).unwrap();
            assert_eq!(stand_in_say(&table), stand_in, "{text}");
        }
        let note = ZORBO.replace(
            "say = { text = \"internet: current price of Zorbo coin\" }",
            "say = \"internet: current price of Zorbo coin\"",
        );
        assert!(!stand_in_say(&toml::from_str(&note).unwrap()));
        // Told once after, in one line, with its undo.
        let told = note_text(&keeping);
        assert!(
            told.contains(
                "I fixed your zorbo coin price skill so it does what you asked. Say 'put my old \
                 zorbo coin price skill back' to undo."
            ),
            "{told}"
        );
    }

    /// The first builder's 10:29 on #7814: a say of the make's `cannot:`
    /// marker is a stand-in as `internet:` is, from `made_skill`'s one list
    /// (`made_skill::SAY_MARKERS`), so it is picked under `on`.
    #[test]
    fn a_say_of_cannot_is_broken_and_picked_under_on() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("cannot");
        set_picking_here(Some(Some(Scope::Broken)));
        let cannot = ZORBO.replace(
            "internet: current price of Zorbo coin",
            "cannot: read the Zorbo coin price",
        );
        old_skill_said(&keeping, "zorbo-coin-price", &cannot, None, Some("d"));
        assert!(stand_in_say(&toml::from_str(&cannot).unwrap()));
        assert!(!stand_in_say(
            &toml::from_str(&cannot.replace("cannot: read the Zorbo coin price", "cannot:"))
                .unwrap()
        ));
        let chosen = chosen(&keeping);
        assert_eq!(chosen.len(), 1, "{chosen:?}");
        let pick = &chosen[0].1;
        assert!(pick.broken && pick.quick.is_none() && pick.old_steps.is_none());
        assert_eq!(pick.task, "tell me the price of Zorbo coin");
    }

    /// lamp-9adf's focus-mode as it is kept: started by its owner's words,
    /// a fixed say, its colour left out.
    const FOCUS_9ADF: &str = "kind = \"installable\"\nversion = \"1.0.0\"\n\
        asks = [\"focus mode\"]\n[permissions]\nlight = true\nspeak = true\n[ask]\n\
        policy = \"nudge\"\n[[do]]\nsay = { text = \"Focus mode activated.\" }\n";
    const FOCUS_9ADF_SAID: &str = "When you say focus mode, it says focus mode is on. Left out: \
        its colour in a step that changes the light.";

    /// The first builder's 10:29 on #7814: 9adf's focus-mode, a broken
    /// quick reaction, is picked under `on` and remade by the lamp's own
    /// model as a quick reaction still: its start and its line kept, a
    /// light step of the colour its kept request names added, no task and
    /// no agent; told once, put back within the week.
    #[test]
    fn a_broken_quick_reaction_is_fixed_as_one_with_the_colour_its_request_names() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("focus-9adf");
        set_picking_here(Some(Some(Scope::Broken)));
        assert!(made_skill::quick_reaction(FOCUS_9ADF));
        let request = "focus mode: say focus mode activated and turn the light blue";
        old_skill_said(
            &keeping,
            "focus-mode",
            FOCUS_9ADF,
            Some(request),
            Some(FOCUS_9ADF_SAID),
        );
        let old_sha = entry_named(&keeping, "focus-mode")[0]["sha256"]
            .as_str()
            .unwrap()
            .to_owned();
        let chosen = chosen(&keeping);
        assert_eq!(chosen.len(), 1, "{chosen:?}");
        let pick = &chosen[0].1;
        let quick = pick.quick.as_ref().expect("a quick remake");
        assert!(pick.broken && pick.old_steps.is_none());
        assert_eq!(quick.part, "its colour in a step that changes the light");
        assert_eq!(
            quick.said,
            "When you say focus mode, it says focus mode is on."
        );
        assert!(
            quick
                .words
                .starts_with(&format!("Their request: {request}"))
        );
        assert!(same_timing(&pick.timing, FOCUS_9ADF), "{}", pick.timing);
        let brief = quick_brief(quick);
        assert!(brief.contains(FOCUS_9ADF) && brief.contains("Left out: its colour"));
        let fixed = format!("{FOCUS_9ADF}\n[[do]]\nlight = {{ colour = \"blue\" }}\n");
        let fake = Fake {
            quick_answer: Some(format!("```skill.toml\n{fixed}```")),
            no_agent: true,
            ..Fake::default()
        };
        assert_eq!(
            step(&keeping, &fake, &Clock::day_idle()),
            Step::Kept("focus-mode".to_owned())
        );
        let kept = fake.quick_kept.borrow()[0].clone();
        let table: toml::Table = toml::from_str(&kept).unwrap();
        let steps = table["do"].as_array().unwrap();
        assert_eq!(steps.len(), 2, "{kept}");
        assert_eq!(
            steps[0]["say"]["text"].as_str(),
            Some("Focus mode activated.")
        );
        assert_eq!(steps[1]["light"]["colour"].as_str(), Some("blue"));
        assert!(
            steps.iter().all(|step| step.get("task").is_none()),
            "{kept}"
        );
        assert!(made_skill::quick_reaction(&kept), "{kept}");
        // On as it is, no task's yes to keep.
        assert!(fake.yes.borrow().is_empty() && fake.put.borrow().len() == 1);
        let entry = entry_named(&keeping, "focus-mode").remove(0);
        assert_eq!(entry[made_skill::MADE_WITH], "deepseek");
        assert_eq!(entry["remade_from"], old_sha.as_str());
        // Never picked again, its old one kept or gone.
        assert!(chosen_names(&keeping).is_empty());
        let told = note_text(&keeping);
        assert!(
            told.contains(
                "I fixed your focus mode skill so it does what you asked. Say 'put my old focus \
                 mode skill back' to undo."
            ),
            "{told}"
        );
        let back = put_back(
            &keeping,
            &fake,
            "focus mode",
            true,
            || true,
            Clock::day_idle().now_ms,
        );
        assert!(back["next"].as_str().unwrap().contains("is back"), "{back}");
        assert_eq!(
            entry_named(&keeping, "focus-mode")[0]["sha256"],
            old_sha.as_str()
        );
    }

    /// The first builder's 10:29 on #7814: the colour is never guessed. With
    /// nothing of theirs naming one (no request, its asks and said-back
    /// naming none), the model's `none`, or a colour it chose, keeps
    /// nothing: the skill stays exactly as it is and nothing is told. Its
    /// fix may add only the lamp's own steps, every value named, and change
    /// nothing else.
    #[test]
    fn a_broken_quick_reaction_with_no_colour_named_is_left_alone_and_untold() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("focus-none");
        set_picking_here(Some(Some(Scope::Broken)));
        old_skill_said(
            &keeping,
            "focus-mode",
            FOCUS_9ADF,
            None,
            Some(FOCUS_9ADF_SAID),
        );
        let before = fs::read(keeping.directory.join(made_skill::INDEX)).unwrap();
        let guessed = format!("{FOCUS_9ADF}\n[[do]]\nlight = {{ colour = \"blue\" }}\n");
        for (day, answer) in [None, Some(format!("```skill.toml\n{guessed}```"))]
            .into_iter()
            .enumerate()
        {
            let fake = Fake {
                quick_answer: answer,
                ..Fake::default()
            };
            let mut clock = Clock::day_idle();
            clock.now_ms += day as u64 * DAY_MS;
            assert_eq!(
                step(&keeping, &fake, &clock),
                Step::NotKept("its fix wasn't kept")
            );
            assert!(fake.put.borrow().is_empty());
            assert_eq!(
                fs::read(keeping.directory.join(made_skill::INDEX)).unwrap(),
                before
            );
            assert_eq!(note(&keeping, true, false, None), None);
        }
        // The checks, one by one, with blue named.
        let quick = Quick {
            old: FOCUS_9ADF.to_owned(),
            part: "its colour".into(),
            words: "Their request: focus mode in blue".into(),
            said: String::new(),
        };
        let fenced = |toml: &str| format!("```skill.toml\n{toml}```");
        let add = |step: &str| fenced(&format!("{FOCUS_9ADF}\n[[do]]\n{step}\n"));
        assert!(quick_fixed(&quick, &add("light = { colour = \"blue\" }")).is_ok());
        for wrong in [
            add("light = { colour = \"red\" }"),
            add("light = { colour = \"blue\", level = 40 }"),
            add("say = { text = \"blue\" }"),
            add("task = { task = \"blue\", run = \"scripts/agent.py\" }"),
            fenced(&FOCUS_9ADF.replace("Focus mode activated.", "Focus on.")),
            fenced(&FOCUS_9ADF.replace("speak = true", "speak = true\nnet = true")),
            fenced(FOCUS_9ADF),
            "none".to_owned(),
        ] {
            assert!(quick_fixed(&quick, &wrong).is_err(), "{wrong}");
        }
    }

    /// Thirteen's P3 on #7765: a remade skill's undo outlives the switch.
    /// With it off, the old one is still kept and put back on the owner's
    /// ask, and past its week it ages out, package and all.
    #[test]
    fn a_remade_skills_undo_and_its_week_hold_with_the_switch_off() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("undo-off");
        old_skill(&keeping, "water", PRESENCE, "local:1");
        let old_sha = old_skill(&keeping, "welcome", PRESENCE, "local:2");
        let fake = Fake::default();
        let mut clock = Clock::day_idle();
        step(&keeping, &fake, &clock);
        step(&keeping, &fake, &clock);
        assert!(undo_kept(&keeping.directory));
        // The switch off from here.
        set_picking_here(Some(None));
        assert_eq!(step(&keeping, &fake, &clock), Step::Nothing);
        let back = put_back(&keeping, &fake, "welcome", true, || true, clock.now_ms);
        assert!(back["next"].as_str().unwrap().contains("is back"), "{back}");
        assert_eq!(
            entry_named(&keeping, "welcome")[0]["sha256"],
            old_sha.as_str()
        );
        let water = entry_named(&keeping, "water").remove(0);
        let package = water["old"]["package"].as_str().unwrap().to_owned();
        assert!(age_out(&keeping, clock.now_ms));
        clock.now_ms += UNDO_FOR.as_millis() as u64;
        assert!(!age_out(&keeping, clock.now_ms));
        assert!(entry_named(&keeping, "water")[0].get("old").is_none());
        assert!(!keeping.directory.join(&package).exists());
        assert!(!undo_kept(&keeping.directory));
    }

    /// The owner's next line's note, as [`note`] gives it.
    fn note_text(keeping: &Keeping) -> String {
        note(keeping, true, false, None).unwrap()
    }

    /// The first builder, 06:07 and 06:08: a description that itself names
    /// a part left out is broken, and that part, from the description, is
    /// added to its task; with no request and no asks, the said-back is the
    /// task, the part left out taken out of it first.
    #[test]
    fn a_description_that_names_a_part_left_out_is_broken_and_the_part_is_added() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("focus");
        set_picking_here(Some(Some(Scope::Broken)));
        // A note said too: no lamp-only fire, so its agent's task.
        let noted = format!("{FOCUS}\n[[do]]\nsay = {{ note = \"Tell them focus starts.\" }}\n");
        old_skill_said(&keeping, "focus-mode", &noted, None, Some(FOCUS_SAID));
        assert_eq!(
            left_out_of(FOCUS_SAID),
            Some("its colour in a step that changes the light")
        );
        let chosen = chosen(&keeping);
        assert_eq!(chosen.len(), 1, "{chosen:?}");
        let pick = &chosen[0].1;
        assert!(pick.broken && pick.quick.is_none());
        assert_eq!(
            pick.task,
            "At 9 every morning, it dims the light for focus; also its colour in a step that \
             changes the light"
        );
        // With a request kept, the request and the part left out.
        let entry = json!({
            "request": "every morning at 9 dim the light blue for focus",
            "description": FOCUS_SAID,
        });
        let table: toml::Table = toml::from_str(FOCUS).unwrap();
        assert_eq!(
            task_of(&entry, &table).unwrap(),
            "every morning at 9 dim the light blue for focus; also its colour in a step that \
             changes the light"
        );
        // The lamp's frame of the model's own left-out line counts too.
        let framed = "It dims the light. The maker notes what it couldn't do: blinking.";
        assert_eq!(left_out_of(framed), Some("blinking"));
        // A description with no part left out is not broken.
        assert!(!broken(
            &json!({"description": "It dims the light."}),
            &table
        ));
    }

    /// The first builder, 06:08: a working skill (moon-time's fixed say)
    /// is never touched in phase 1; in phase 2 it is remade, its own old
    /// steps kept beside its task to run in the agent's place.
    #[test]
    fn a_working_skill_waits_for_phase_two_and_keeps_its_old_steps() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("moon");
        old_skill_said(&keeping, "moon-time", MOON, None, Some(MOON_SAID));
        set_picking_here(Some(Some(Scope::Broken)));
        assert!(chosen(&keeping).is_empty());
        let fake = Fake::default();
        assert_eq!(step(&keeping, &fake, &Clock::day_idle()), Step::Nothing);
        assert!(fake.asked.borrow().is_empty());
        set_picking_here(Some(Some(Scope::All)));
        let chosen = chosen(&keeping);
        assert_eq!(chosen.len(), 1);
        let pick = &chosen[0].1;
        assert!(!pick.broken);
        assert_eq!(pick.task, MOON_SAID);
        assert_eq!(pick.old_steps.as_deref(), Some(MOON));
        // Its index changed since the last look: scanned again, remade.
        let mut entries = made_skill::entries(&keeping.directory);
        entries[0]["seen"] = json!(true);
        made_skill::write_entries(&keeping.directory, &entries).unwrap();
        assert_eq!(
            step(&keeping, &fake, &Clock::day_idle()),
            Step::Kept("moon-time".to_owned())
        );
        assert_eq!(*fake.old_steps.borrow(), vec![Some(MOON.to_owned())]);
        let told = note_text(&keeping);
        assert!(
            told.contains("I moved your moon time skill to your agent"),
            "{told}"
        );
        // The middle form with its old steps beside its task, as kept: the
        // very same skill with `[[otherwise]]` and the old permissions.
        let block = kept_timing(MOON).unwrap();
        let middle = crate::make_no_cannot::skill_text("moon-time", &[], &block, MOON_SAID);
        let toml = middle
            .split("```skill.toml\n")
            .nth(1)
            .and_then(|rest| rest.split("```").next())
            .unwrap();
        let kept = with_old_steps(toml, MOON).unwrap();
        let table: toml::Table = toml::from_str(&kept).unwrap();
        let old: toml::Table = toml::from_str(MOON).unwrap();
        assert_eq!(table.get("otherwise"), old.get("do"));
        assert_eq!(table["permissions"]["speak"].as_bool(), Some(true));
        assert_eq!(table["trigger"], old["trigger"]);
        assert!(table["do"][0].get("task").is_some(), "{kept}");
        // Old steps that write, hand a task over or have code aren't kept.
        let write = MOON.replace(
            "say = { text = \"It's moon time.\" }",
            "write = { service = \"gmail\" }",
        );
        assert_eq!(with_old_steps(toml, &write), None);
    }

    /// Working quick reactions and lamp-only fires (a pat, a glow, a nod, a
    /// chime) are never moved, in either phase; a broken one is fixed under
    /// `on` as one still (the first builder's 10:29 on #7814).
    #[test]
    fn working_quick_reactions_and_lamp_only_fires_are_never_picked() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("never");
        old_skill(&keeping, "pat-nod", QUICK, "local:1");
        old_skill(&keeping, "nine-glow", GLOW, "local:2");
        // A pat's say of the stand-in is no quick reaction: broken.
        let marked = QUICK.replace("Hello!", "internet: the news");
        old_skill(&keeping, "pat-news", &marked, "local:3");
        // A glow whose said-back names a part left out: broken.
        old_skill_said(&keeping, "focus-glow", FOCUS, None, Some(FOCUS_SAID));
        for scope in [Scope::Broken, Scope::All] {
            set_picking_here(Some(Some(scope)));
            assert_eq!(
                chosen_names(&keeping),
                vec!["pat-news".to_owned(), "focus-glow".to_owned()],
                "{scope:?}"
            );
            let chosen = chosen(&keeping);
            assert!(chosen[0].1.quick.is_none(), "{scope:?}");
            assert!(chosen[1].1.quick.is_some(), "{scope:?}");
        }
        assert!(made_skill::quick_reaction(QUICK));
        assert!(lamp_only_fire(&toml::from_str(GLOW).unwrap()));
        assert!(!lamp_only_fire(&toml::from_str(MOON).unwrap()));
        assert!(!lamp_only_fire(&toml::from_str(LOCAL).unwrap()));
    }

    /// The first builder, 06:07: with no kept request, no asks and no
    /// description, nothing gives its task, and the skill stays exactly as
    /// it is; so does one whose start can't be kept.
    #[test]
    fn a_skill_with_nothing_to_give_its_task_or_no_start_is_untouched() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let broken_presence = PRESENCE.replace(
            "say = { note = \"Welcome them back.\" }",
            "say = { text = \"internet: the news\" }",
        );
        for scope in [Scope::Broken, Scope::All] {
            let (_root, keeping) = lamp(&format!("untouched-{scope:?}"));
            set_picking_here(Some(Some(scope)));
            old_skill_said(&keeping, "no-words", &broken_presence, None, None);
            // A blank description is none.
            old_skill_said(&keeping, "blank-said", &broken_presence, None, Some("  "));
            // Only a tool starts it: no `[trigger]` and no `asks`.
            let tool_only = broken_presence
                .split_once("[trigger]")
                .map(|(head, _)| {
                    format!(
                        "{head}tool = \"news\"\n[[do]]\nsay = {{ text = \"internet: news\" }}\n"
                    )
                })
                .unwrap();
            old_skill(&keeping, "tool-only", &tool_only, "local:2");
            let before = fs::read(keeping.directory.join(made_skill::INDEX)).unwrap();
            assert!(chosen_names(&keeping).is_empty(), "{scope:?}");
            let fake = Fake::default();
            assert_eq!(step(&keeping, &fake, &Clock::day_idle()), Step::Nothing);
            assert!(fake.asked.borrow().is_empty() && fake.put.borrow().is_empty());
            assert_eq!(
                fs::read(keeping.directory.join(made_skill::INDEX)).unwrap(),
                before
            );
            assert!(
                asked(&keeping, true, || true, || true)["next"]
                    .as_str()
                    .unwrap()
                    .contains("none of their skills")
            );
        }
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
        // Lines that can't be taken out whole are written again, the very
        // same values: a header with a note on its line, asks over lines.
        for toml in [
            PRESENCE.replace("[trigger]", "[trigger] # when they come"),
            MOON.replace("asks = [\"moon time\"]", "asks = [\n  \"moon time\",\n]"),
        ] {
            let kept = kept_timing(&toml).unwrap();
            assert!(same_timing(&kept, &toml), "{kept}");
        }
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
            let moved: BTreeSet<String> = names.iter().cloned().collect();
            for moved in [BTreeSet::new(), moved] {
                let line = told_line(&names, &moved).unwrap();
                assert!(
                    !line.contains("ask you") && !line.contains("asks you"),
                    "{line}"
                );
                assert!(line.contains("back' to undo."), "{line}");
            }
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
        assert!(
            told.contains(
                "I moved your water skill to your agent; it starts when it did, and does what it \
                 did before if your agent is slow. Say 'put my old water skill back' to undo."
            ),
            "{told}"
        );
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

    /// Twelve's P2 on #7765: a remake whose new one can't go on keeps the
    /// old entry exactly as it was and drops the new one, never the index
    /// swapped to a skill that isn't on.
    #[test]
    fn a_remake_that_cant_go_on_keeps_the_old_and_drops_the_new() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("put-on-fails");
        let old_sha = old_skill(&keeping, "water", PRESENCE, "local:1");
        let before = made_skill::entries(&keeping.directory);
        let fake = Fake::default();
        fake.put_fails.set(true);
        assert_eq!(
            step(&keeping, &fake, &Clock::day_idle()),
            Step::NotKept("it couldn't go on")
        );
        let tried = fake.put.borrow().last().cloned().unwrap();
        assert_ne!(tried, old_sha);
        assert_eq!(made_skill::entries(&keeping.directory), before);
        let water = entry_named(&keeping, "water");
        assert_eq!(water.len(), 1);
        assert_eq!(water[0]["sha256"], old_sha.as_str());
        assert!(water[0].get("old").is_none());
        assert!(
            made_skill::entries(&keeping.directory)
                .iter()
                .all(|entry| entry["sha256"] != tried.as_str())
        );
    }

    /// Twelve's P2 on #7765: a put-back whose old one can't go back on
    /// leaves the remade one as it was, its old one still kept to put back.
    #[test]
    fn a_put_back_that_cant_go_on_rolls_back_to_the_remade_one() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("put-back-fails");
        let old_sha = old_skill(&keeping, "water", PRESENCE, "local:1");
        let fake = Fake::default();
        let clock = Clock::day_idle();
        assert_eq!(
            step(&keeping, &fake, &clock),
            Step::Kept("water".to_owned())
        );
        let remade = entry_named(&keeping, "water").remove(0);
        assert_ne!(remade["sha256"], old_sha.as_str());
        fake.put_fails.set(true);
        let back = put_back(&keeping, &fake, "water", true, || true, clock.now_ms);
        assert!(
            back["next"]
                .as_str()
                .unwrap()
                .contains("couldn't be put back"),
            "{back}"
        );
        assert_eq!(entry_named(&keeping, "water"), vec![remade.clone()]);
        assert_eq!(remade["old"]["sha256"], old_sha.as_str());
        assert!(!load(&keeping.directory).put_back.contains("water"));
        // Once it can go on, the same ask puts it back.
        fake.put_fails.set(false);
        let back = put_back(&keeping, &fake, "water", true, || true, clock.now_ms);
        assert!(back["next"].as_str().unwrap().contains("is back"), "{back}");
        assert_eq!(
            entry_named(&keeping, "water")[0]["sha256"],
            old_sha.as_str()
        );
    }

    /// Twelve's P2 on #7765: a skill removed 3 days ago, then one remade
    /// tonight: "put it back", the words the person was told, puts back the
    /// remade one's old version, never the removed one (whose undo
    /// `make_no_asks`' "last" would take, its window being 7 days). With
    /// nothing remade after a later action, the undo stays that action's.
    #[test]
    fn put_it_back_after_a_remake_puts_back_the_remade_one_not_an_older_remove() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let (_root, keeping) = lamp("put-back-after-remove");
        let clock = Clock::day_idle();
        let removed_at = clock.now_ms - 3 * DAY_MS;
        let write_records = |records: Value| {
            fs::write(
                keeping.directory.join(crate::make_no_asks::RECORDS),
                records.to_string(),
            )
            .unwrap();
        };
        write_records(json!([{
            "kind": "removed", "name": "moon-fact", "at_unix_ms": removed_at,
            "entry": {"name": "moon-fact", "sha256": "m".repeat(64), "version": "1.0.0"},
        }]));
        assert_eq!(
            crate::make_no_asks::latest_at(&keeping.directory, clock.now_ms),
            Some(removed_at)
        );
        // Nothing remade: the undo is the remove's.
        assert_eq!(latest_remade(&keeping.directory, clock.now_ms), None);
        assert_eq!(
            put_back_for_undo(&keeping.directory, "last", clock.now_ms),
            None
        );
        let old_sha = old_skill(&keeping, "zorbo-coin", PRESENCE, "local:1");
        let fake = Fake::default();
        assert_eq!(
            step(&keeping, &fake, &clock),
            Step::Kept("zorbo-coin".to_owned())
        );
        // Told as the one-skill line names it.
        let told = told_line(&["zorbo-coin".to_owned()], &BTreeSet::new()).unwrap();
        assert!(
            told.ends_with("Say 'put my old zorbo coin skill back' to undo."),
            "{told}"
        );
        // "put it back" as the undo's "last", or by its name: the remake's.
        let later = clock.now_ms + HOUR_MS;
        for asked in ["last", "", "zorbo coin", "my old zorbo coin skill"] {
            assert_eq!(
                put_back_for_undo(&keeping.directory, asked, later).as_deref(),
                Some("zorbo-coin"),
                "{asked}"
            );
        }
        assert_eq!(
            put_back_for_undo(&keeping.directory, "moon fact", later),
            None
        );
        let name = put_back_for_undo(&keeping.directory, "last", later).unwrap();
        let back = put_back(&keeping, &fake, &name, true, || true, later);
        assert!(back["next"].as_str().unwrap().contains("is back"), "{back}");
        assert_eq!(
            entry_named(&keeping, "zorbo-coin")[0]["sha256"],
            old_sha.as_str()
        );
        assert!(entry_named(&keeping, "moon-fact").is_empty());
        // An action after a remake keeps the undo its own.
        let (_root2, keeping2) = lamp("put-back-then-make");
        old_skill(&keeping2, "zorbo-coin", PRESENCE, "local:1");
        assert_eq!(
            step(&keeping2, &Fake::default(), &clock),
            Step::Kept("zorbo-coin".to_owned())
        );
        fs::write(
            keeping2.directory.join(crate::make_no_asks::RECORDS),
            json!([{
                "kind": "made", "name": "evening-dim", "at_unix_ms": clock.now_ms + 1,
                "sha256": "e".repeat(64),
            }])
            .to_string(),
        )
        .unwrap();
        assert_eq!(put_back_for_undo(&keeping2.directory, "last", later), None);
    }

    /// The link takes `make_skill` whole with this switch and every other
    /// the product's `lamp-voice` unit sets on that shapes it (writes,
    /// tasks, code, the turn-off, the make at once, the core offer's undo
    /// and the repeat told on the request): its description
    /// within 1,024 bytes, and its parameters within
    /// [`lamp_gemini::MAX_TOOL_JSON_BYTES`] with the undo's word that a
    /// remake's put-back is its own ([`PUT_BACK_UNDO_TOLD`], added while one
    /// is the latest thing done) counted in. Before, they came to 4,245
    /// bytes and the product lamp's chat requests left `make_skill` out;
    /// now 4,055, the remake's two and the undo's word 318 of them.
    #[test]
    fn make_skill_is_within_the_link_with_remakes_and_the_products_switches_on() {
        made_skill::set_writes_here(Some(true));
        made_skill::set_scripts_here(Some(true));
        made_skill::set_off_said_here(Some(true));
        crate::skill_task_yes::set_on_here(Some(true));
        crate::make_no_asks::set_here(Some(true));
        crate::long_make::set_on_here(Some(false));
        crate::make_core_offer::set_on_here(Some(true));
        crate::skill_recurring::set_on_here(Some(true));
        set_on_here(Some(true));
        let all = crate::make_skill_local::tool();
        let parameters = all.parameters.as_ref().unwrap();
        for part in [
            REMAKE_OLD,
            PUT_BACK,
            crate::made_turn::TURN_OFF,
            crate::make_no_asks::UNDO,
        ] {
            assert!(parameters["properties"][part].is_object(), "{part}");
        }
        assert!(all.description.len() <= 1_024, "{}", all.description.len());
        let bytes = parameters.to_string().len();
        assert!(
            bytes <= crate::make_skill_local::PRODUCT_MOST_PARAMETERS_BYTES,
            "{bytes}"
        );
        let worst = bytes + PUT_BACK_UNDO_TOLD.len();
        assert!(worst <= lamp_gemini::MAX_TOOL_JSON_BYTES, "{worst}");
        let (kept, left_out) = crate::tools::admitted(vec![all]);
        assert_eq!((kept.len(), left_out), (1, Vec::<String>::new()));
        set_on_here(None);
        crate::skill_recurring::set_on_here(None);
        crate::make_core_offer::set_on_here(None);
        crate::long_make::set_on_here(None);
        crate::make_no_asks::set_here(None);
        crate::skill_task_yes::set_on_here(None);
        made_skill::set_off_said_here(None);
        made_skill::set_scripts_here(None);
        made_skill::set_writes_here(None);
    }

    #[test]
    fn with_the_switch_off_nothing_happens() {
        assert!(!on_with(None) && !on_with(Some("yes")) && !on_with(Some("ON")));
        assert!(!on_with(Some("broken")) && !on_with(Some("ALL")));
        assert_eq!(scope_with(Some("on")), Some(Scope::Broken));
        assert_eq!(scope_with(Some("all")), Some(Scope::All));
        // A real folder with one remade and untold: off, no note and
        // nothing marked; on, the line.
        let (_root, keeping) = lamp("off");
        old_skill(&keeping, "water", PRESENCE, "local:1");
        // Off (the environment's, unset in tests): nothing is picked, a
        // broken one either, and the pass remakes nothing.
        old_skill_said(&keeping, "zorbo", ZORBO, None, Some("Tells the price."));
        set_picking_here(None);
        assert!(chosen(&keeping).is_empty());
        let fake = Fake::default();
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        assert_eq!(step(&keeping, &fake, &Clock::day_idle()), Step::Nothing);
        assert!(fake.asked.borrow().is_empty());
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
        assert!(
            told.contains(
                "I fixed 3 of your skills so they do what you asked. Say 'put my water skill \
                 back' to undo."
            ),
            "{told}"
        );
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
            may_run_on: false,
        };
        let one = One::take().unwrap();
        assert_eq!(agent.write("S", "B", false).unwrap(), "made");
        assert!(STOOD_DOWN.load(Ordering::Acquire));
        drop(one);
        let _ = fs::remove_dir_all(&dir);
    }
}
