//! The one agent namespace written by a round (#6510 Part 1, step 1;
//! docs/install.md, "One agent namespace for every engine"): after each
//! round that reached the channel, every engine the lamp's Harness last
//! said is installed on the lamp's own machine
//! ([`crate::engines::Engines::harness`], the look of #6240) gets its
//! wrapper, the namespace script and its home ([`wrapper::write_engine`]),
//! written again whenever [`wrapper::present`] says it is not as this
//! build writes it. No engine is named here but those with an installer of
//! their own ([`OWN_INSTALLERS`]), which are passed over: Hermes and
//! OpenClaw keep their own wrappers until step 2.
//!
//! Only with [`SWITCH`] (`LAMP_AGENT_NS`) `on` exactly in lamp-update's own
//! unit, which no product unit sets; and only beside the look at Harness's
//! engines (`LAMP_APP_ENGINES=on`, [`crate::engines::SWITCH`]) and
//! Harness's install, as there is no list of engines without them. Off,
//! nothing is written; what an earlier round wrote is left as it is.
use super::wrapper;
use crate::{
    engines::{HERMES, HarnessLook, OPENCLAW},
    hermes_install::Shown,
};
use std::path::Path;

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

/// After a round, with [`SWITCH`] on: each of [`engines_to_write`]`(look)`
/// whose files under `root` are not as this build writes them for `shown`
/// is written ([`wrapper::write_engine`]), `lampos`'s ids asked of `owner`
/// once, only when one is to be written. Then `finds` (the program
/// `lampos`'s login shell finds for it, [`wrapper::login_finds`] on a
/// lamp) is asked: one found before the wrapper, or none, is said, as the
/// engine is not [`wrapper::present`] and Harness would run it without the
/// namespace; it is not written again for that. One line for each
/// written, refused or found behind another; none for an engine there.
/// The engines written are returned.
pub fn after_round(
    root: &Path,
    look: &HarnessLook,
    shown: Shown,
    owner: &mut dyn FnMut() -> Result<(u32, u32), String>,
    finds: &mut dyn FnMut(&str) -> Option<String>,
    say: &mut dyn FnMut(&str),
) -> Vec<String> {
    let mut ids: Option<(u32, u32)> = None;
    let mut written = Vec::new();
    for engine in engines_to_write(look) {
        let Ok(place) = wrapper::wrapper_path(&engine) else {
            continue;
        };
        // The files alone: `present` as if the login shell found the wrapper.
        if !wrapper::present(root, &engine, shown, Some(&place)) {
            let owner = match ids {
                Some(ids) => ids,
                None => match owner() {
                    Ok(found) => *ids.insert(found),
                    Err(why) => {
                        say(&format!(
                            "the agent namespace is not written for any engine: {why}"
                        ));
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
                    say(&format!(
                        "{engine}: not written to run in the lamp's agent namespace: {why}"
                    ));
                    continue;
                }
            }
        }
        let found = finds(&engine);
        if !wrapper::present(root, &engine, shown, found.as_deref()) {
            say(&match found {
                Some(found) => format!(
                    "{engine}: lampos's login shell finds {found} before {place}, so it would run without the agent namespace"
                ),
                None => {
                    format!("{engine}: lampos's login shell finds no {engine}, not even {place}")
                }
            });
        }
    }
    written
}

#[cfg(test)]
mod tests;
