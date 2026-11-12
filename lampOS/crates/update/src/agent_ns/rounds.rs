//! The one agent namespace written by a round (#6510 Part 1, step 1;
//! docs/install.md, "One agent namespace for every engine"): after each
//! round that reached the channel, every engine the lamp's Harness last
//! said is installed on the lamp's own machine
//! ([`crate::engines::Engines::harness`], the look of #6240) gets its
//! wrapper, the namespace script and its home ([`wrapper::write_engine`]),
//! written again whenever [`wrapper::present`] says it is not as this
//! build writes it. No engine is named here but those with an installer of
//! their own ([`OWN_INSTALLERS`]), which are passed over: Hermes and
//! OpenClaw keep their own wrappers until step 2. A wrapper this build
//! wrote ([`wrapper::ours`], exactly its text) is taken out when Harness no
//! longer says its engine is installed ([`engines_to_remove`]), or when no
//! program of the engine's own is left on `lampos`'s login `PATH`
//! ([`wrapper::login_programs`]), so it never keeps an uninstalled engine
//! looking installed; any other file is left.
//! What stays true round after round is said once ([`Said`]).
//!
//! Only with [`SWITCH`] (`LAMP_AGENT_NS`) `on` exactly in lamp-update's own
//! unit, which no product unit sets; and only beside the look at Harness's
//! engines (`LAMP_APP_ENGINES=on`, [`crate::engines::SWITCH`]) and
//! Harness's install, as there is no list of engines without them. Off,
//! nothing is written or taken out; what an earlier round wrote is left as
//! it is.
use super::wrapper;
use crate::{
    engines::{HERMES, HarnessLook, OPENCLAW},
    hermes_install::Shown,
};
use std::{collections::BTreeMap, path::Path};

/// The switch, in lamp-update's own unit: `on` exactly, any other value or
/// none off.
pub const SWITCH: &str = "LAMP_AGENT_NS";

/// The engines with an installer of their own in this build
/// (`hermes_install`, `openclaw_install`), whose wrappers stay theirs
/// until #6510 step 2: never written here.
pub const OWN_INSTALLERS: [&str; 2] = [HERMES, OPENCLAW];

/// [`SWITCH`]'s value: on only when `on` exactly.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The engines a round writes, from Harness's last look: each said
/// `installed`, with a plain name ([`wrapper::engine_ok`]), not one of
/// [`OWN_INSTALLERS`]. One `not_installed` is passed over: a wrapper on
/// `lampos`'s `PATH` would have Harness find it as installed.
pub fn engines_to_write(look: &HarnessLook) -> Vec<String> {
    look.engines
        .iter()
        .filter(|(name, engine)| {
            engine.state == "installed"
                && wrapper::engine_ok(name)
                && !OWN_INSTALLERS.contains(&name.as_str())
        })
        .map(|(name, _)| name.clone())
        .collect()
}

/// `lampos`'s user and group ids, as Hermes's install looks them up.
pub fn lampos_ids() -> Result<(u32, u32), String> {
    use crate::{hermes_install::gid_of, install};
    let mut run = install::output_of;
    let uid = install::uid_of(&mut run, install::USER)?;
    let gid = gid_of(&mut run, install::USER)?;
    let parse = |id: &str| {
        id.parse::<u32>()
            .map_err(|error| format!("{}'s id {id}: {error}", install::USER))
    };
    Ok((parse(&uid)?, parse(&gid)?))
}

/// What a round last said that stays true round after round, one line for
/// each key (an engine, or the round's own): a line is said only when its
/// key had no line, or another, the round before, as the look at Harness's
/// engines says a failed look once (#6240). Kept by the caller between
/// rounds; a key the round had nothing to say for is forgotten, so the
/// same line coming back later is said again.
#[derive(Debug, Default)]
pub struct Said {
    last: BTreeMap<String, String>,
}

/// The key of the round's own line (no ids for `lampos`): never an
/// engine's name, which is plain ([`wrapper::engine_ok`]).
const ROUND_KEY: &str = ":round";

/// One round's lines that stay true: said unless the round before said the
/// same for the same key.
struct Notes<'a> {
    last: &'a BTreeMap<String, String>,
    now: BTreeMap<String, String>,
}

impl Notes<'_> {
    fn note(&mut self, key: &str, line: String, say: &mut dyn FnMut(&str)) {
        if self.last.get(key) != Some(&line) {
            say(&line);
        }
        self.now.insert(key.to_owned(), line);
    }
}

/// The engines whose wrapper a round takes out: each [`wrapper::ours`]
/// under `root` (exactly this build's text), not one of
/// [`OWN_INSTALLERS`], that Harness's `look` no longer says is
/// `installed`. Only from a look that reached Harness (its machine known):
/// before the first, or after one that failed, none.
pub fn engines_to_remove(root: &Path, look: &HarnessLook) -> Vec<String> {
    if look.machine.is_none() {
        return Vec::new();
    }
    let installed = engines_to_write(look);
    wrapper::ours_written(root)
        .into_iter()
        .filter(|engine| !OWN_INSTALLERS.contains(&engine.as_str()) && !installed.contains(engine))
        .collect()
}

/// After a round, with [`SWITCH`] on. First each wrapper this build wrote
/// ([`wrapper::ours_written`], not one of [`OWN_INSTALLERS`]) is taken out
/// ([`wrapper::remove_ours`]; its home and the namespace script stay), one
/// line each, when (a) it is one of [`engines_to_remove`]`(root, look)`, or
/// (b) `programs` (the engine's own programs on `lampos`'s login `PATH`,
/// [`wrapper::login_programs`] on a lamp) ran and found none, even while
/// Harness still says it is installed (the wrapper alone keeps it so). A
/// look that failed keeps it, said once. Then each of
/// [`engines_to_write`]`(look)` whose files under `root` are not as this
/// build writes them for `shown` is written ([`wrapper::write_engine`]),
/// unless `programs` found none of its own (said once; written again once
/// one is there), `lampos`'s ids asked of `owner` once, only when one is
/// to be written. Then `finds` (the program `lampos`'s login shell finds
/// for it, [`wrapper::login_finds`] on a lamp) is asked: one found before
/// the wrapper, or none, is said, as the engine is not
/// [`wrapper::present`] and Harness would run it without the namespace; it
/// is not written again for that. One line for each written or taken out;
/// a refusal, a wrapper found behind another, a failed look or no ids are
/// said once while they stay the same ([`Said`]), again when they change.
/// None for an engine there. `programs` is asked at most once an engine a
/// round. The engines written are returned.
#[allow(clippy::too_many_arguments)]
pub fn after_round(
    root: &Path,
    look: &HarnessLook,
    shown: Shown,
    said: &mut Said,
    owner: &mut dyn FnMut() -> Result<(u32, u32), String>,
    finds: &mut dyn FnMut(&str) -> Option<String>,
    programs: &mut dyn FnMut(&str) -> Result<Vec<String>, String>,
    say: &mut dyn FnMut(&str),
) -> Vec<String> {
    let mut notes = Notes {
        last: &said.last,
        now: BTreeMap::new(),
    };
    let mut asked: BTreeMap<String, Result<Vec<String>, String>> = BTreeMap::new();
    let mut programs = |engine: &str| {
        asked
            .entry(engine.to_owned())
            .or_insert_with(|| programs(engine))
            .clone()
    };
    let written = round(
        root,
        look,
        shown,
        &mut notes,
        owner,
        finds,
        &mut programs,
        say,
    );
    said.last = notes.now;
    written
}

#[allow(clippy::too_many_arguments)]
fn round(
    root: &Path,
    look: &HarnessLook,
    shown: Shown,
    notes: &mut Notes,
    owner: &mut dyn FnMut() -> Result<(u32, u32), String>,
    finds: &mut dyn FnMut(&str) -> Option<String>,
    programs: &mut dyn FnMut(&str) -> Result<Vec<String>, String>,
    say: &mut dyn FnMut(&str),
) -> Vec<String> {
    let unreported = engines_to_remove(root, look);
    for engine in wrapper::ours_written(root) {
        if OWN_INSTALLERS.contains(&engine.as_str()) {
            continue;
        }
        let place = wrapper::wrapper_path(&engine).unwrap_or_default();
        let why = if unreported.contains(&engine) {
            "Harness no longer says it is installed".to_owned()
        } else {
            match programs(&engine) {
                Ok(found) if found.is_empty() => {
                    "no program of its own is left on lampos's login PATH".to_owned()
                }
                Ok(_) => continue,
                Err(why) => {
                    notes.note(
                        &format!("{engine}:look"),
                        format!(
                            "{engine}: lamp-update's wrapper {place} kept, as its own programs could not be looked for: {why}"
                        ),
                        say,
                    );
                    continue;
                }
            }
        };
        match wrapper::remove_ours(root, &engine) {
            Ok(true) => say(&format!(
                "{engine}: {why}, so lamp-update's wrapper {place} is taken out"
            )),
            Ok(false) => {}
            Err(error) => notes.note(
                &format!("{engine}:remove"),
                format!("{engine}: lamp-update's wrapper not taken out: {error}"),
                say,
            ),
        }
    }
    let mut ids: Option<(u32, u32)> = None;
    let mut written = Vec::new();
    for engine in engines_to_write(look) {
        let Ok(place) = wrapper::wrapper_path(&engine) else {
            continue;
        };
        // The files alone: `present` as if the login shell found the wrapper.
        if !wrapper::present(root, &engine, shown, Some(&place)) {
            // Nothing of its own to run: a wrapper would only keep it
            // looking installed, and be taken out next round.
            if programs(&engine).is_ok_and(|found| found.is_empty()) {
                notes.note(
                    &format!("{engine}:none"),
                    format!(
                        "{engine}: not written, as no program of its own is on lampos's login PATH"
                    ),
                    say,
                );
                continue;
            }
            let owner = match ids {
                Some(ids) => ids,
                None => match owner() {
                    Ok(found) => *ids.insert(found),
                    Err(why) => {
                        notes.note(
                            ROUND_KEY,
                            format!("the agent namespace is not written for any engine: {why}"),
                            say,
                        );
                        // What the round before said of each engine still
                        // holds: not looked at again this round.
                        for (key, line) in notes.last {
                            notes.now.entry(key.clone()).or_insert_with(|| line.clone());
                        }
                        return written;
                    }
                },
            };
            match wrapper::write_engine(root, &engine, owner, shown) {
                Ok(()) => {
                    say(&format!(
                        "{engine}: written to run in the lamp's agent namespace ({place}, HOME {})",
                        wrapper::home_of(&engine).unwrap_or_default(),
                    ));
                    written.push(engine.clone());
                }
                Err(why) => {
                    notes.note(
                        &engine,
                        format!(
                            "{engine}: not written to run in the lamp's agent namespace: {why}"
                        ),
                        say,
                    );
                    continue;
                }
            }
        }
        let found = finds(&engine);
        if !wrapper::present(root, &engine, shown, found.as_deref()) {
            notes.note(
                &engine,
                match found {
                    Some(found) => format!(
                        "{engine}: lampos's login shell finds {found} before {place}, so it would run without the agent namespace"
                    ),
                    None => format!(
                        "{engine}: lampos's login shell finds no {engine}, not even {place}"
                    ),
                },
                say,
            );
        }
    }
    written
}

#[cfg(test)]
mod tests;
