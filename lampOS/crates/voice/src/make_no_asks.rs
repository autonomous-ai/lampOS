//! Nothing asked of the person while a skill is made or removed (the
//! first builder, 18 Nov 18:41, from the product owner: "A made skill is
//! made and said in one line, with an undo ('Made it, say "remove it" to
//! undo'), never read back for a yes. Offers become the action plus that
//! one line. An unsure line is simply not acted on, with no 'did you
//! say…?'"; and at 18:45: a made skill's remove is not in the carve-out).
//! With [`ENVIRONMENT`] `on` exactly, in no unit; off, every make and
//! remove is byte for byte as before.
//!
//! - **Made, on, one line.** A voice make, plain or with code of its own,
//!   goes on at once and the lamp says one line through the model, in the
//!   conversation's language ([`made_line`], [`told`]): "Made NAME. Say
//!   'remove it' to undo.", with at most one short clause of what it will
//!   do, never a question. Code goes on by its yes's own path
//!   (`make_skill_local::code_on_at_once`: that package by its SHA-256, its
//!   task's yes recorded for exactly its grants), only with
//!   `LAMP_MADE_SCRIPTS` on (`made_skill::scripts_on`) and only on the
//!   owner's own ask, as its yes was held (`services::owners_yes_of`); the
//!   cap, the pack check, the grants and the run namespace are unchanged. A
//!   skill that sends (a write, or code that messages the phone) keeps its
//!   read-back and yes: that yes is what lamp-services sends by. The app's
//!   MAKE IT alike ([`put_on`], the app's ask the owner's).
//! - **A clash replaces** ([`replaced`]): the new one goes on, then each
//!   skill on the lamp it clashes with is turned off by the manage helper's
//!   own `{"off": NAME}`, as the app's switch does, never deleted: "Made
//!   NAME and replaced your old OTHER. Say 'keep the old one' to undo."
//! - **A remove happens at once** ([`remove_asked`]): only the owner's ask,
//!   its name matched to their own made skills as before
//!   (`made_remove::found`); taken off by the manage helper, then kept aside
//!   for [`KEPT_FOR`] (its index entry in [`RECORDS`], its package in
//!   [`KEPT_FOLDER`]): "Removed NAME. Say 'put it back' to undo." Several as
//!   near remove nothing and ask nothing.
//! - **The undo** ([`undo`]): `make_skill`'s [`UNDO`], the model's judgment
//!   of the person's words, never a list of them: [`LAST`] for what was
//!   just done, or a name as heard. A make within [`UNDO_WITHIN`] is
//!   removed (and kept aside); a replace within it puts the old ones back
//!   on and removes the new one; a remove within [`KEPT_FOR`] is put back
//!   and on. Only the owner's ask.
//! - **Unsure, not acted on.** A made skill judged not what was asked
//!   (`LAMP_MADE_UNRELATED_REFUSED`), or a call with no request, makes
//!   nothing and asks nothing ([`NOT_ACTED`]).
//! - **Never "on" while off.** A put-on that fails is said in one plain
//!   line with why (`made_skill::not_on_line`).
use crate::made_remove::{self, Found};
use crate::made_skill::{self, Keeping, Made};
use serde_json::{Value, json};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The switch: `on` exactly, off unless set, in no unit.
pub const ENVIRONMENT: &str = "LAMP_MAKE_NO_ASKS";

thread_local! {
    /// [`ENVIRONMENT`] as a test of this thread sets it.
    static HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether nothing is asked of the person in a make or a remove, in this
/// process.
pub fn on() -> bool {
    if let Some(on) = HERE.with(std::cell::Cell::get) {
        return on;
    }
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// [`on`] for the calling thread alone, for tests; `None` reads the
/// environment again. The product never calls it.
#[doc(hidden)]
pub fn set_here(on: Option<bool>) {
    HERE.with(|here| here.set(on));
}

/// The `make_skill` argument that undoes what was just done to a skill
/// they made: [`LAST`], or the skill's name as heard.
pub const UNDO: &str = "undo";
/// [`UNDO`]'s value for whatever was done last.
pub const LAST: &str = "last";
/// How long a make or a replace may be undone.
pub const UNDO_WITHIN: Duration = Duration::from_secs(10 * 60);
/// How long a skill removed with no yes is kept to be put back.
pub const KEPT_FOR: Duration = Duration::from_secs(7 * 86_400);
/// What was done and may be undone, in the made skills' folder.
pub const RECORDS: &str = "undo.json";
/// Where a removed skill's package is kept, in the made skills' folder.
pub const KEPT_FOLDER: &str = "undo";

/// What the model is told of a make not acted on (an unsure line):
/// nothing made, nothing asked.
pub const NOT_ACTED: &str = "nothing was made: it wasn't clear what to make";
/// What the model is told to do of a make not acted on.
pub const NOT_ACTED_NEXT: &str = "Nothing was made. Ask nothing, ask them to repeat nothing, and \
                                  never say a skill was made. Call nothing.";
/// Why a skill with code of its own isn't put on for anyone but the owner.
pub const NOT_OWNERS_CODE: &str =
    "only the lamp's owner can put on a skill that runs its own code, by asking for it";
/// Why a skill with code of its own isn't put on with its code off.
pub const CODE_OFF: &str = "this lamp doesn't run a made skill's own code";
/// What a remove or an undo not the owner's is told.
pub const NOT_OWNER: &str = "only the lamp's owner can remove a skill they made, or undo that";
/// What an undo with nothing to undo is told.
pub const NOTHING_TO_UNDO: &str = "nothing that can be undone was done to a skill they made";

/// The one line said of a made skill on: its name, the undo. `extra`, a
/// sentence of its own when not empty, goes between.
pub fn made_line(made: &Made, extra: &str) -> String {
    format!(
        "Made {}.{extra} Say 'remove it' to undo.",
        made_skill::spoken_name(&made.name)
    )
}

/// The one line said of a made skill that replaced `others`.
pub fn replaced_line(made: &Made, others: &[String]) -> String {
    format!(
        "Made {} and replaced your old {}. Say 'keep the old one' to undo.",
        made_skill::spoken_name(&made.name),
        crate::made_clash::spoken_list(others)
    )
}

/// The one line said of a skill removed.
pub fn removed_line(name: &str) -> String {
    format!(
        "Removed {}. Say 'put it back' to undo.",
        made_skill::spoken_name(name)
    )
}

/// What the model is told to say of a skill made: `line`, in one short
/// line in the conversation's language, its name as written, with at most
/// one short clause of what it will do (`does`, its description), and no
/// question. It opens on [`made_skill::ALREADY_DONE`], so it is the turn's
/// make (`made_skill::tells_made`).
pub fn told(line: &str, does: Option<&str>) -> String {
    let clause = does
        .map(|does| does.trim().trim_end_matches(['.', '!', '?']))
        .filter(|does| !does.is_empty())
        .map(|does| format!("; you may add one short clause of what it will do ({does})"))
        .unwrap_or_default();
    format!(
        "{}Tell the person this as one short line, in the language of the conversation, the \
         skill's name as written{clause}. Ask nothing and add no question: \"{line}\"",
        made_skill::ALREADY_DONE
    )
}

/// What the model is told to say of a remove or an undo done, or not:
/// `line`, plainly, with no question.
pub fn said(line: &str) -> String {
    format!(
        "It is done already: call no tool, only speak. Tell the person this as one short line, \
         in the language of the conversation, the skill's name as written. Ask nothing and add \
         no question: \"{line}\""
    )
}

/// What the model is told to say of a skill made but not on: why, plainly.
fn not_on_told(line: &str) -> String {
    format!(
        "{}Tell the person this as one short line, in the language of the conversation, the \
         skill's name as written. Never say it is on. Ask nothing: \"{line}\"",
        made_skill::ALREADY_DONE
    )
}

fn now_unix_ms() -> u64 {
    crate::app_make::now_unix_ms()
}

/// The kept skill `made`, just made, put on at once: plain, by
/// [`made_skill::put_on`]; with code of its own, only with
/// [`made_skill::scripts_on`] and on the owner's ask (`owner`, asked only
/// then), by `make_skill_local::code_on_at_once`. Whether its task was
/// kept (always for a plain one), or why it isn't on; and the one line.
/// None when this switch is off, or for a skill that sends, which keeps its
/// read-back and yes.
pub fn put_on(
    keeping: &Keeping,
    made: &Made,
    owner: impl FnOnce() -> bool,
) -> Option<(Result<bool, String>, String)> {
    if !on() || !keeps_no_yes(made) {
        return None;
    }
    let on = if made.uses.code {
        if !made_skill::scripts_on() {
            Err(CODE_OFF.to_owned())
        } else if !owner() {
            Err(NOT_OWNERS_CODE.to_owned())
        } else {
            crate::make_skill_local::code_on_at_once(keeping, made)
        }
    } else {
        made_skill::put_on(keeping, made).map(|()| true)
    };
    let line = match &on {
        Ok(true) => made_line(made, ""),
        Ok(false) => made_line(made, made_skill::CODE_RUNS_NOWHERE_YET),
        Err(why) if made.uses.agent_task => made_skill::agent_not_on_line(why),
        Err(why) => made_skill::not_on_line(why),
    };
    if on.is_ok() {
        noted(
            &keeping.directory,
            json!({ "kind": "made", "name": made.name, "sha256": made.sha256 }),
        );
    }
    Some((on, line))
}

/// Whether `made` goes on with no yes with this switch: anything but a
/// skill that sends (a write, or code that messages the phone).
pub fn keeps_no_yes(made: &Made) -> bool {
    made.uses.writes.is_empty() && !(made.uses.code && made.uses.messages)
}

/// What the model is told of `made`, put on or not (`on`, `line`, from
/// [`put_on`]), with `needs` its services not linked yet.
pub fn made_told(
    made: &Made,
    on: &Result<bool, String>,
    line: &str,
    needs: Option<&[Value]>,
) -> Value {
    let link = needs
        .and_then(made_skill::needs_link_ask)
        .map(|ask| format!(" {ask}"))
        .unwrap_or_default();
    let next = match on {
        Ok(_) => told(line, Some(&made.description)),
        Err(_) => not_on_told(line),
    };
    let mut told = json!({
        "made": made.name,
        "on": on.is_ok(),
        "description": made.description,
        "uses": made.uses.spoken(),
        "next": format!("{next}{link}"),
    });
    match on {
        Ok(task) if made.uses.code => told["task"] = json!(task),
        Ok(_) => {}
        Err(why) => told["not_on"] = json!(why),
    }
    if let Some(needs) = needs.filter(|needs| !needs.is_empty()) {
        told["needs_link"] = json!(needs);
    }
    told
}

/// `made`, kept and not on, clashes with the skills on the lamp `others`
/// (`made_clash::found`): it goes on, then each of them is turned off by
/// the manage helper (never deleted), so "keep the old one" turns them back
/// on ([`undo`]). A put-on that fails turns nothing off. What the model is
/// told, and the one line.
pub fn replaced(keeping: &Keeping, made: &Made, others: &[String]) -> (Value, String) {
    if let Err(why) = made_skill::put_on(keeping, made) {
        let line = made_skill::not_on_line(&why);
        return (
            json!({
                "made": made.name,
                "on": false,
                "not_on": why,
                "next": not_on_told(&line),
            }),
            line,
        );
    }
    let mut off = Vec::new();
    let mut still_on = Vec::new();
    for other in others {
        match crate::made_turn::turned_by_helper(&keeping.manage, other, true) {
            Ok(()) => off.push(other.clone()),
            Err(why) => still_on.push((other.clone(), why)),
        }
    }
    eprintln!(
        "lamp-voice: a made skill replaced {} on the lamp with no ask, {} left on (LAMP_MAKE_NO_ASKS)",
        off.len(),
        still_on.len()
    );
    noted(
        &keeping.directory,
        if off.is_empty() {
            json!({ "kind": "made", "name": made.name, "sha256": made.sha256 })
        } else {
            json!({
                "kind": "replaced",
                "name": made.name,
                "sha256": made.sha256,
                "others": off,
            })
        },
    );
    let line = match still_on.first() {
        None => replaced_line(made, &off),
        Some((_, why)) => {
            let names: Vec<String> = still_on.iter().map(|(name, _)| name.clone()).collect();
            let (still, are) = (
                crate::made_clash::spoken_list(&names),
                if names.len() > 1 { "are" } else { "is" },
            );
            let new = made_skill::spoken_name(&made.name);
            if off.is_empty() {
                format!("Made {new}, but {still} {are} still on: {why}. Say 'remove it' to undo.")
            } else {
                format!(
                    "Made {new} and replaced your old {}, but {still} {are} still on: {why}. Say \
                     'keep the old one' to undo.",
                    crate::made_clash::spoken_list(&off)
                )
            }
        }
    };
    let mut told = json!({
        "made": made.name,
        "on": true,
        "replaced": off,
        "next": told(&line, Some(&made.description)),
    });
    if !still_on.is_empty() {
        told["not_off"] = json!(
            still_on
                .iter()
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>()
        );
    }
    (told, line)
}

/// The records of what may be undone in `directory`, as kept, with those
/// past their time dropped (a removed one's kept package with it): a make
/// or a replace after [`UNDO_WITHIN`], a remove after [`KEPT_FOR`].
fn records(directory: &Path, now_unix_ms: u64) -> Vec<Value> {
    let all: Vec<Value> = fs::read(directory.join(RECORDS))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let fresh = |record: &Value| {
        let within = if record["kind"] == "removed" {
            KEPT_FOR
        } else {
            UNDO_WITHIN
        };
        record["at_unix_ms"]
            .as_u64()
            .is_some_and(|at| now_unix_ms.saturating_sub(at) <= within.as_millis() as u64)
    };
    let (kept, gone): (Vec<Value>, Vec<Value>) = all.into_iter().partition(fresh);
    if !gone.is_empty() {
        for record in &gone {
            if let Some(path) = kept_package(directory, record) {
                let _ = fs::remove_file(path);
            }
        }
        if let Err(error) = write_records(directory, &kept) {
            eprintln!("lamp-voice: a made skill's undo records weren't trimmed: {error}");
        }
    }
    kept
}

/// The records written whole beside them and moved over them.
fn write_records(directory: &Path, records: &[Value]) -> io::Result<()> {
    fs::create_dir_all(directory)?;
    let new = directory.join(format!(".{RECORDS}.new"));
    fs::write(&new, serde_json::to_vec(records).map_err(io::Error::other)?)?;
    fs::rename(new, directory.join(RECORDS))
}

/// `record` kept as what was done last, at the lamp's clock now; an
/// earlier make or replace of the same skill gives way to it.
fn noted(directory: &Path, mut record: Value) {
    let now = now_unix_ms();
    record["at_unix_ms"] = json!(now);
    let mut all = records(directory, now);
    all.retain(|old| old["kind"] == "removed" || old["name"] != record["name"]);
    all.push(record);
    if let Err(error) = write_records(directory, &all) {
        eprintln!("lamp-voice: a made skill's undo record wasn't kept: {error}");
    }
}

/// Where a removed skill's package is kept ([`KEPT_FOLDER`]), by its
/// SHA-256; none for a record of no removed skill.
fn kept_package(directory: &Path, record: &Value) -> Option<PathBuf> {
    let sha256 = record["entry"]["sha256"]
        .as_str()
        .filter(|sha| !sha.is_empty() && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))?;
    Some(directory.join(KEPT_FOLDER).join(format!("{sha256}.tar")))
}

/// The package file an index entry names, when it is a plain file name.
fn package_of(entry: &Value) -> Option<&str> {
    entry["package"]
        .as_str()
        .filter(|name| !name.is_empty() && !name.contains('/') && !name.starts_with('.'))
}

/// The made skill `name` (its package `sha256`) taken off the lamp by
/// `remove` (the manage helper's own remove) when it is on as that very
/// package, then kept aside for [`KEPT_FOR`]: its index entry into
/// [`RECORDS`], its package into [`KEPT_FOLDER`], so [`undo`] can put it
/// back. Why not, in plain words.
pub fn remove_at_once(
    keeping: &Keeping,
    name: &str,
    sha256: &str,
    remove: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(), String> {
    let entries = made_skill::entries(&keeping.directory);
    let Some(entry) = entries
        .iter()
        .find(|entry| entry["name"] == name && entry["sha256"] == sha256)
        .cloned()
    else {
        return Err("the lamp no longer has it".into());
    };
    if entry["on"] == true {
        match made_skill::went_on_as(keeping, name) {
            Ok(Some(on)) if on == sha256 => remove(name)?,
            Ok(None) => {}
            _ => {
                return Err(format!(
                    "the skill called {} on the lamp now isn't the one they made",
                    made_skill::spoken_name(name)
                ));
            }
        }
    }
    let rest: Vec<Value> = entries
        .iter()
        .filter(|other| !(other["name"] == name && other["sha256"] == sha256))
        .cloned()
        .collect();
    let record = json!({ "kind": "removed", "name": name, "entry": entry });
    // Its package kept aside: moved, or copied while another entry still
    // names the same file.
    if let (Some(file), Some(kept)) = (
        package_of(&entry),
        kept_package(&keeping.directory, &record),
    ) {
        let from = keeping.directory.join(file);
        let shared = rest.iter().any(|other| other["package"] == file);
        let moved =
            fs::create_dir_all(kept.parent().unwrap_or(&keeping.directory)).and_then(|()| {
                if shared {
                    fs::copy(&from, &kept).map(|_| ())
                } else {
                    fs::rename(&from, &kept)
                }
            });
        if let Err(error) = moved {
            eprintln!("lamp-voice: a removed made skill's package wasn't kept aside: {error}");
        }
    }
    if let Err(error) = made_skill::write_entries(&keeping.directory, &rest) {
        eprintln!(
            "lamp-voice: a made skill was taken off the lamp, but is still listed among the made skills: {error}"
        );
    }
    noted(&keeping.directory, record);
    eprintln!(
        "lamp-voice: a made skill was removed by voice with no yes, kept aside to put back (LAMP_MAKE_NO_ASKS)"
    );
    Ok(())
}

/// A remove asked by voice (`make_skill`'s `remove`, the name as heard,
/// through `made_remove::asked_or_removed`,
/// in the voice's request `request`, `heard`: someone spoke it): the
/// person's own made skill found by that name (`made_remove::found`) is
/// removed at once on the owner's ask (`owner`), with no question, and kept
/// aside ([`remove_at_once`]). Several as near, one they didn't make, or
/// none, remove nothing and ask nothing. What the model is told.
pub fn remove_asked(
    name: &str,
    request: u64,
    heard: bool,
    keeping: &Keeping,
    owner: impl FnOnce() -> bool,
    remove: impl FnOnce(&str) -> Result<(), String>,
) -> Value {
    if !heard {
        eprintln!("lamp-voice: request {request}: a skill wasn't removed: no one asked aloud");
        return json!({ "error": made_remove::NOT_ASKED_ALOUD });
    }
    let found = made_remove::found(name, keeping);
    let mut owner = Some(owner);
    let mut owners = None;
    let mut owner = || *owners.get_or_insert_with(|| owner.take().is_some_and(|ask| ask()));
    match found {
        Found::Made {
            name: made, sha256, ..
        } => {
            if !owner() {
                eprintln!(
                    "lamp-voice: request {request}: a skill wasn't removed: not the owner's ask"
                );
                return json!({
                    "error": NOT_OWNER,
                    "next": "Say plainly, in a few words, that only the lamp's owner can remove \
                             a skill they made, and nothing was removed. Name no skill. Ask \
                             nothing; call nothing."
                });
            }
            if crate::skill_write_yes::drop_remove_of(&made) {
                eprintln!(
                    "lamp-voice: request {request}: a remove waiting for the same made skill was dropped"
                );
            }
            match remove_at_once(keeping, &made, &sha256, remove) {
                Ok(()) => json!({
                    made_remove::REMOVE: made,
                    "removed": true,
                    "next": said(&removed_line(&made)),
                }),
                Err(why) => {
                    eprintln!("lamp-voice: request {request}: a made skill wasn't removed: {why}");
                    json!({
                        "error": format!("not removed: {why}"),
                        "next": said(&format!(
                            "I couldn't remove {}: {why}.",
                            made_skill::spoken_name(&made)
                        )),
                    })
                }
            }
        }
        Found::Alike(names) => {
            let named = if owner() {
                format!(
                    " ({})",
                    made_remove::said_or(&names[..names.len().min(3)]).replace(" or ", " and ")
                )
            } else {
                String::new()
            };
            json!({
                "error": "more than one skill they made has a name like that",
                "next": format!(
                    "Say plainly, in a few words, that more than one skill they made has a name \
                     like that{named}, so nothing was removed. Ask nothing; call nothing."
                )
            })
        }
        Found::NotMade { name: other, exact } if exact || owner() => json!({
            "error": made_remove::NOT_MADE,
            "next": format!(
                "Say plainly, in a few words, that {} isn't a skill they made, so you can't \
                 remove it by voice. Call nothing.",
                made_skill::spoken_name(&other)
            )
        }),
        _ => json!({
            "error": made_remove::NO_MADE,
            "next": "Say plainly, in a few words, that there is no skill they made by that \
                     name. Ask nothing; call nothing."
        }),
    }
}

/// The undo `asked` ([`LAST`], or a skill's name as heard) of what was
/// done to a skill they made: the newest record that is it
/// ([`records`]). A make is removed and kept aside; a replace turns the
/// old ones back on, then removes the new one; a remove is put back and
/// on. What the model is told, in one plain line.
pub fn undo(keeping: &Keeping, asked: &str, now_unix_ms: u64) -> Value {
    let all = records(&keeping.directory, now_unix_ms);
    let asked = asked.trim();
    let last = asked.is_empty() || asked.eq_ignore_ascii_case(LAST);
    let is_named = |record: &Value| {
        let near = |name: &str| made_remove::near(made_remove::apart(asked, name), name);
        record["name"].as_str().is_some_and(near)
            || record["others"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .any(near)
    };
    let Some(record) = all
        .iter()
        .filter(|record| last || is_named(record))
        .max_by_key(|record| record["at_unix_ms"].as_u64().unwrap_or_default())
        .cloned()
    else {
        return json!({
            "error": NOTHING_TO_UNDO,
            "next": "Say plainly, in a few words, that there is nothing to undo now. Ask \
                     nothing; call nothing."
        });
    };
    let name = record["name"].as_str().unwrap_or_default().to_owned();
    let spoken = made_skill::spoken_name(&name);
    let sha256 = record["sha256"].as_str().unwrap_or_default().to_owned();
    let failed = |why: String| {
        eprintln!("lamp-voice: an undo of a made skill wasn't done: {why}");
        json!({ "error": format!("not undone: {why}"), "next": said(&format!("I couldn't undo that: {why}.")) })
    };
    match record["kind"].as_str() {
        Some("made") => {
            match remove_at_once(keeping, &name, &sha256, |skill| {
                crate::made_clash::remove_by_helper(&keeping.manage, skill)
            }) {
                Ok(()) => {
                    json!({ made_remove::REMOVE: name, "removed": true, "next": said(&removed_line(&name)) })
                }
                Err(why) => failed(why),
            }
        }
        Some("replaced") => {
            let others: Vec<String> = record["others"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|other| other.as_str().map(str::to_owned))
                .collect();
            for other in &others {
                if let Err(why) = crate::made_turn::turned_by_helper(&keeping.manage, other, false)
                {
                    return failed(format!(
                        "{} didn't come back on: {why}, and {spoken} stays on",
                        made_skill::spoken_name(other)
                    ));
                }
            }
            let off = match made_skill::went_on_as(keeping, &name) {
                Ok(Some(on)) if on == sha256 => {
                    crate::made_clash::remove_by_helper(&keeping.manage, &name)
                }
                Ok(None) => Ok(()),
                _ => Err(format!(
                    "the skill called {spoken} on the lamp now isn't the one made"
                )),
            };
            drop_record(&keeping.directory, &record);
            let old = crate::made_clash::spoken_list(&others);
            match off {
                Ok(()) => {
                    made_skill::forget_named(&keeping.directory, &name);
                    eprintln!(
                        "lamp-voice: a replace was undone: the old skills back on, the new one removed (LAMP_MAKE_NO_ASKS)"
                    );
                    json!({
                        "kept_old": others,
                        made_remove::REMOVE: name,
                        "next": said(&format!("Your old {old} is back on, and I removed {spoken}.")),
                    })
                }
                Err(why) => json!({
                    "kept_old": others,
                    "error": format!("not removed: {why}"),
                    "next": said(&format!(
                        "Your old {old} is back on, but I couldn't remove {spoken}: {why}."
                    )),
                }),
            }
        }
        Some("removed") => put_back(keeping, &record),
        _ => failed("the lamp doesn't know what that was".into()),
    }
}

/// The removed skill `record` put back ([`remove_at_once`] kept it): its
/// package moved back, its entry into the index, then put on as a made
/// skill is (one with code of its own by its yes's own path). Its record
/// is dropped either way. What the model is told.
fn put_back(keeping: &Keeping, record: &Value) -> Value {
    let directory = &keeping.directory;
    let mut entry = record["entry"].clone();
    let name = entry["name"].as_str().unwrap_or_default().to_owned();
    let spoken = made_skill::spoken_name(&name);
    let sha256 = entry["sha256"].as_str().unwrap_or_default().to_owned();
    let not_back = |why: &str| {
        eprintln!("lamp-voice: a removed made skill wasn't put back: {why}");
        json!({
            "error": format!("not put back: {why}"),
            "next": said(&format!("I couldn't put {spoken} back: {why}.")),
        })
    };
    let mut entries = made_skill::entries(directory);
    if entries.iter().any(|other| other["name"] == name.as_str()) {
        return not_back("a newer skill they made has its name now");
    }
    let (Some(file), Some(kept)) = (package_of(&entry), kept_package(directory, record)) else {
        return not_back("the lamp no longer has it");
    };
    let to = directory.join(file);
    let moved = match (fs::read(&kept), fs::read(&to)) {
        (Ok(_), Ok(_)) if entries.iter().any(|other| other["package"] == file) => {
            fs::remove_file(&kept)
        }
        (Ok(_), Ok(_)) => Err(io::Error::other("another skill's package has its place")),
        (Ok(_), Err(_)) => fs::rename(&kept, &to),
        (Err(error), _) => Err(error),
    };
    if let Err(error) = moved {
        eprintln!("lamp-voice: a removed made skill's package wasn't put back: {error}");
        return not_back("the lamp no longer has it");
    }
    entry["on"] = json!(false);
    if let Some(entry) = entry.as_object_mut() {
        entry.remove("not_on");
        entry.remove("check_said");
    }
    entries.push(entry.clone());
    if let Err(error) = made_skill::write_entries(directory, &entries) {
        eprintln!("lamp-voice: a removed made skill wasn't listed again: {error}");
        return not_back("the lamp couldn't keep it");
    }
    drop_record(directory, record);
    let on = match made_skill::made_of(directory, &entry) {
        Some(made) if made.uses.code => {
            crate::make_skill_local::code_on_at_once(keeping, &made).map(|_| ())
        }
        Some(_) => made_skill::put_on_within(
            directory,
            &keeping.manage,
            &sha256,
            now_unix_ms(),
            made_skill::PUT_ON_WAIT,
        )
        .map(|_| ()),
        None => Err("the lamp no longer has it".to_owned()),
    };
    match on {
        Ok(()) => {
            eprintln!("lamp-voice: a removed made skill was put back on (LAMP_MAKE_NO_ASKS)");
            json!({ "put_back": name, "on": true, "next": said(&format!("{spoken} is back on.")) })
        }
        Err(why) => json!({
            "put_back": name,
            "on": false,
            "not_on": why,
            "next": said(&format!("I put {spoken} back, but couldn't turn it on: {why}.")),
        }),
    }
}

/// `record` dropped from the records.
fn drop_record(directory: &Path, record: &Value) {
    let mut all = records(directory, now_unix_ms());
    all.retain(|kept| kept != record);
    if let Err(error) = write_records(directory, &all) {
        eprintln!("lamp-voice: a made skill's undo record wasn't dropped: {error}");
    }
}

/// [`undo`] by `make_skill`'s call on the lamp: heard, and only the
/// owner's ask, as a remove's.
pub fn undo_here(
    asked: &str,
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
) -> Value {
    undo_asked(asked, request, heard, || {
        crate::services::owners_yes_now(request, heard, lease)
    })
}

/// [`undo_here`], whose ask is the owner's by `owner`.
pub fn undo_asked(asked: &str, request: u64, heard: bool, owner: impl FnOnce() -> bool) -> Value {
    if !heard {
        eprintln!("lamp-voice: request {request}: nothing undone: no one asked aloud");
        return json!({ "error": made_remove::NOT_ASKED_ALOUD });
    }
    let Some(keeping) = Keeping::when(true) else {
        return crate::tools::unavailable();
    };
    if !owner() {
        eprintln!("lamp-voice: request {request}: nothing undone: not the owner's ask");
        return json!({
            "error": NOT_OWNER,
            "next": "Say plainly, in a few words, that only the lamp's owner can undo that, and \
                     nothing changed. Ask nothing; call nothing."
        });
    }
    undo(&keeping, asked, now_unix_ms())
}

#[cfg(test)]
mod tests;
