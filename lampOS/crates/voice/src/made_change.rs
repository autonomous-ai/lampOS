//! A spoken change to a skill the person made (#7975, #8008; the first
//! builder's ruling, 20 Nov 01:38): "change rain check to seven thirty"
//! changes that skill, in place. On lamp-9adf, 20 Nov 01:30 and 01:33, each
//! such line was made as a new skill, `morning-umbrella-reminder-2` and
//! `-3`, so three umbrella skills fired; before #8008 one was removed.
//!
//! - **The model judges the meaning.** `make_skill`'s [`CHANGE`] is the
//!   name of the skill to change as the person said it, given with its
//!   `request`, the change in their words; no word list is read here.
//! - **Which skill** ([`changed`]): their made skill found by that name as a
//!   remove finds one (`made_remove::found`). When it can't be told which
//!   (several as near, or none by that name while they made some), the
//!   model asks which in one question naming them, and nothing is made.
//! - **Re-made in place.** The make is asked for the old skill's said-back
//!   description with the change after it, the change winning
//!   ([`merged_request`]), and kept under the old skill's own name, never
//!   `name-2` (`made_skill::ChangeAs`). It goes on as a make does; the
//!   manage helper puts it in the old one's folder, so its on/off state
//!   (lamp-skills' `off` file, by name) is kept. Never a second skill, and
//!   never a remove. Only the owner's ask, as a remove's.
//! - **Put it back, for a week** ([`put_back`]): the old entry is kept
//!   under the new one's `old`, its package with it, with `replaced_at_ms`
//!   and `was_off` (the shape of #7765's remake, so both share one undo).
//!   "Put it back" (`make_no_asks`' undo, when the change is the latest
//!   thing done: [`put_back_for_undo`]) puts the old version back on in
//!   place. After [`UNDO_FOR`] the old package is deleted ([`drop_old`]);
//!   the owner's "forget everything" clears it ([`clear`]).
//! - **Told once**: "Changed NAME. Say 'put it back' to undo." ([`changed_line`]).
use crate::made_remove::{self, Found};
use crate::made_skill::{self, Keeping};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;

/// The `make_skill` argument naming the skill a make changes, as heard.
pub const CHANGE: &str = "change";

/// How long a changed skill's old version is kept to be put back.
pub const UNDO_FOR: Duration = Duration::from_secs(7 * 86_400);

/// What a change asked by anyone but the owner is told.
pub const NOT_OWNER: &str = "only the lamp's owner can change a skill they made";

/// The most of their made skills named when none has the name asked.
const MOST_NAMED: usize = 4;

/// The request a change is made from: the old skill's said-back
/// description (`was`), then the change (`change`), which wins where they
/// differ. The making model writes the merge.
pub fn merged_request(was: &str, change: &str) -> String {
    let was = was.trim();
    let change = change.trim();
    if was.is_empty() {
        return change.to_owned();
    }
    let change = change.trim_end_matches(['.', '!', '?']);
    format!(
        "{was}\nChange to make to it now: {change}.\nWhere they differ, the change wins; keep the \
         rest as it was."
    )
}

/// The one line said of a changed skill.
pub fn changed_line(name: &str, was_off: bool) -> String {
    let off = if was_off { " It stays off." } else { "" };
    format!(
        "Changed {}.{off} Say 'put it back' to undo.",
        made_skill::spoken_name(name)
    )
}

/// What the model is told when it can't be told which skill: one short
/// question naming them, and nothing made.
fn which_told(names: &[String]) -> Value {
    json!({
        "error": "which skill to change isn't clear",
        "next": format!(
            "Nothing was made or changed. Ask them in one short question which skill they mean: \
             {}. Call nothing now.",
            made_remove::said_or(names)
        )
    })
}

/// The change `request` of the person's made skill named `asked` as
/// heard, by `make` (a make of the words it is given, on this thread):
/// re-made under its name, on, in place ([`made_skill::ChangeAs`]). Only
/// the owner's ask (`owner`). `off`, lamp-skills' `off` file as read, says
/// whether it was off. What the model is told.
pub fn changed(
    keeping: &Keeping,
    asked: &str,
    request: &str,
    owner: impl FnOnce() -> bool,
    off: Option<&BTreeSet<String>>,
    make: impl FnOnce(&str) -> Value,
    now_ms: u64,
) -> Value {
    drop_old(&keeping.directory, now_ms);
    let (name, sha256) = match made_remove::found(asked, keeping) {
        Found::Made { name, sha256, .. } => (name, sha256),
        Found::Alike(names) => return which_told(&names[..names.len().min(MOST_NAMED)]),
        Found::NotMade { name, .. } => {
            return json!({
                "error": "only a skill the person made is changed by voice",
                "next": format!(
                    "Say plainly, in a few words, that {} isn't a skill they made, so it can't be \
                     changed by voice. Nothing was made or changed. Call nothing.",
                    made_skill::spoken_name(&name)
                )
            });
        }
        Found::Nothing => {
            let theirs: Vec<String> = made_skill::own_on(&keeping.directory)
                .into_iter()
                .take(MOST_NAMED)
                .collect();
            if !theirs.is_empty() {
                return which_told(&theirs);
            }
            return json!({
                "error": made_remove::NO_MADE,
                "next": "Say plainly, in a few words, that there is no skill they made by that \
                         name to change. Nothing was made. Call nothing."
            });
        }
    };
    if !owner() {
        eprintln!("lamp-voice: a made skill wasn't changed: not the owner's ask");
        return json!({
            "error": NOT_OWNER,
            "next": "Say plainly, in a few words, that only the lamp's owner can change a skill \
                     they made, and nothing changed. Name no skill. Call nothing."
        });
    }
    let was = made_skill::entries(&keeping.directory)
        .into_iter()
        .find(|entry| entry["sha256"] == sha256.as_str())
        .and_then(|entry| entry["description"].as_str().map(str::to_owned))
        .unwrap_or_default();
    let was_off = off.is_some_and(|off| off.contains(&name));
    let told = {
        let _as = made_skill::ChangeAs::set(made_skill::ChangeOf {
            name: name.clone(),
            sha256: sha256.clone(),
            was_off,
        });
        make(&merged_request(&was, request))
    };
    finished(keeping, &name, &sha256, was_off, told)
}

/// What a change's make came to (`told`), told as a change: on in place,
/// its old one kept under its `old`; or, made and not on, nothing of it
/// kept and the old one as it was.
fn finished(keeping: &Keeping, name: &str, old_sha: &str, was_off: bool, mut told: Value) -> Value {
    let directory = &keeping.directory;
    let entries = made_skill::entries(directory);
    let new = entries.iter().find(|entry| {
        entry["name"] == name && entry["on"] == true && entry["changed_from"] == old_sha
    });
    if let Some(new) = new {
        let new_sha = new["sha256"].as_str().unwrap_or_default();
        // Its make's own undo ("remove it") is not this one's: a change is
        // put back, never removed.
        crate::make_no_asks::forget_record_of(directory, new_sha);
        let line = changed_line(name, was_off);
        eprintln!("lamp-voice: a made skill was changed in place, its old one kept a week");
        told["changed"] = json!(name);
        told["next"] = json!(crate::make_no_asks::told(
            &line,
            new["description"].as_str()
        ));
        if let Some(told) = told.as_object_mut() {
            told.remove("replaced");
        }
        crate::make_skill_local::said_line_now(line, crate::make_skill_local::Outcome::Made);
        return told;
    }
    // Waiting for the person's yes (a skill that sends): it replaces the
    // old one in place once it goes on.
    if told.get(crate::skill_write_yes::APPROVE_ID).is_some() {
        return told;
    }
    // Made and kept, not on: nothing of it kept; the old one as it was.
    let unput: Vec<String> = entries
        .iter()
        .filter(|entry| entry["name"] == name && entry["on"] != true && entry["sha256"] != old_sha)
        .filter_map(|entry| entry["sha256"].as_str().map(str::to_owned))
        .collect();
    for sha256 in &unput {
        made_skill::forget_unput(directory, sha256);
    }
    if let Some(why) = told["not_on"].as_str().map(str::to_owned) {
        let line = format!(
            "I couldn't change {}: {why}. It stays as it was.",
            made_skill::spoken_name(name)
        );
        eprintln!("lamp-voice: a made skill wasn't changed: its change didn't go on: {why}");
        told["on"] = json!(false);
        told["next"] = json!(crate::make_no_asks::said(&line));
        crate::make_skill_local::said_line_now(line, crate::make_skill_local::Outcome::Failed);
    }
    told
}

/// The changed skills in `directory` whose old ones are kept past
/// [`UNDO_FOR`] at `now_ms`: those old ones dropped, their packages with
/// them unless another entry names one.
pub fn drop_old(directory: &Path, now_ms: u64) {
    let past = |old: &Value| {
        old["replaced_at_ms"]
            .as_u64()
            .is_some_and(|at| now_ms >= at.saturating_add(UNDO_FOR.as_millis() as u64))
    };
    match drop_olds(directory, past) {
        Ok(0) => {}
        Ok(_) => eprintln!("lamp-voice: a changed skill's old version is past its week: deleted"),
        Err(error) => {
            eprintln!("lamp-voice: a changed skill's old version wasn't dropped: {error}")
        }
    }
}

/// Every changed skill's kept old one in `directory` deleted (the owner's
/// "forget everything", with `make_no_asks`' keep): whether there was any.
pub fn clear(directory: &Path) -> io::Result<bool> {
    drop_olds(directory, |_| true).map(|dropped| dropped > 0)
}

/// The kept old ones for which `drop` holds taken out of their entries,
/// each package with them unless an entry still names it: how many.
fn drop_olds(directory: &Path, drop: impl Fn(&Value) -> bool) -> io::Result<usize> {
    made_skill::index_locked(directory, || drop_olds_locked(directory, drop))
}

/// [`drop_olds`], holding the index's lock.
fn drop_olds_locked(directory: &Path, drop: impl Fn(&Value) -> bool) -> io::Result<usize> {
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
}

/// The package of `old`, a changed skill's kept old one, deleted unless
/// one of `entries` still names it ([`made_skill::packages_named`]).
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

/// The changed skill in `directory` whose change is the latest thing done
/// to the person's skills at `now_ms`: its old one kept within
/// [`UNDO_FOR`], and changed after every action `make_no_asks`' undo
/// keeps. Then "put it back" is its [`put_back`].
pub fn latest_changed(directory: &Path, now_ms: u64) -> Option<String> {
    let (at, name) = made_skill::entries(directory)
        .iter()
        .filter_map(|entry| {
            let at = entry["old"]["replaced_at_ms"].as_u64()?;
            let name = entry["name"].as_str()?;
            (now_ms < at.saturating_add(UNDO_FOR.as_millis() as u64)).then(|| (at, name.to_owned()))
        })
        .max()?;
    let done = crate::make_no_asks::latest_at(directory, now_ms);
    done.is_none_or(|done| at >= done).then_some(name)
}

/// The changed skill `make_skill`'s undo `asked` ("last", or a name as
/// heard) is for, when a change is the latest thing done
/// ([`latest_changed`]): "last", or that skill's own name. Then the undo
/// is its [`put_back`]; else none, and the undo is `make_no_asks`' own.
pub fn put_back_for_undo(directory: &Path, asked: &str, now_ms: u64) -> Option<String> {
    let name = latest_changed(directory, now_ms)?;
    let asked = asked.trim();
    let last = asked.is_empty() || asked.eq_ignore_ascii_case(crate::make_no_asks::LAST);
    (last || made_remove::named_alone(asked, std::slice::from_ref(&name)).is_some()).then_some(name)
}

/// [`put_back_for_undo`] on the lamp.
pub fn put_back_for_undo_here(asked: &str) -> Option<String> {
    let keeping = Keeping::when(true)?;
    put_back_for_undo(&keeping.directory, asked, now_unix_ms())
}

fn now_unix_ms() -> u64 {
    crate::app_make::now_unix_ms()
}

/// The changed skill `name`'s old version put back on, in place, within
/// [`UNDO_FOR`] of `now_ms`: its entry back in the new one's place, put on
/// by its SHA-256 (the manage helper puts it in the same folder, so its
/// on/off state stays), the new one's package then deleted. On a failed
/// put-on the new one stays, as it was. Only the owner's ask (`owner`).
/// What the model is told.
pub fn put_back(keeping: &Keeping, name: &str, owner: impl FnOnce() -> bool, now_ms: u64) -> Value {
    if !owner() {
        return json!({
            "error": NOT_OWNER,
            "next": "Say plainly, in a few words, that only the lamp's owner can put a skill \
                     back. Name no skill. Call nothing."
        });
    }
    let directory = &keeping.directory;
    drop_old(directory, now_ms);
    let spoken = made_skill::spoken_name(name);
    // The new one's entry swapped for its old one's, holding the index's
    // lock: no entry a make writes meanwhile is lost.
    let swapped = made_skill::index_locked(directory, || {
        let mut entries = made_skill::entries(directory);
        let at = entries
            .iter()
            .position(|entry| entry["name"] == name && entry.get("old").is_some())?;
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
            "error": "no older version of that skill is kept",
            "next": "Say plainly, in a few words, that there is no older version of that skill \
                     to put back. Call nothing."
        });
    };
    let Ok((new, old)) = swapped else {
        return crate::tools::unavailable();
    };
    let old_sha = old["sha256"].as_str().unwrap_or_default();
    if let Err(why) = made_skill::put_on_within(
        directory,
        &keeping.manage,
        old_sha,
        now_ms,
        made_skill::PUT_ON_WAIT,
    ) {
        // Back as it was: the new one stays on.
        let restored = made_skill::index_locked(directory, || {
            let mut entries = made_skill::entries(directory);
            for entry in &mut entries {
                if entry["sha256"] == old_sha {
                    *entry = new.clone();
                }
            }
            made_skill::write_entries(directory, &entries)
        });
        if let Err(error) = restored {
            eprintln!("lamp-voice: a changed skill's new version isn't listed again: {error}");
        }
        eprintln!("lamp-voice: a changed skill's old version couldn't go back on: {why}");
        return json!({
            "error": format!("not put back: {why}"),
            "next": crate::make_no_asks::said(&format!(
                "I couldn't put your old {spoken} back: {why}. The new one stays."
            )),
        });
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
    eprintln!("lamp-voice: a changed skill's old version is put back");
    json!({
        "made": name,
        "on": true,
        "put_back": true,
        "next": crate::make_no_asks::said(&format!("Your old {spoken} is back.")),
    })
}

/// [`put_back`] by `make_skill`'s call on the lamp: heard, and only the
/// owner's ask, as a remove's.
pub fn put_back_here(
    name: &str,
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
) -> Value {
    if !heard {
        return json!({ "error": made_remove::NOT_ASKED_ALOUD });
    }
    let Some(keeping) = Keeping::when(true) else {
        return crate::tools::unavailable();
    };
    put_back(
        &keeping,
        name,
        || crate::services::owners_yes_now(request, heard, lease),
        now_unix_ms(),
    )
}

#[cfg(test)]
mod tests;
