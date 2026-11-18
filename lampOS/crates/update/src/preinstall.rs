//! The lamp's agents put on the image before it ships (#7274,
//! `lamp-update agents-preinstall`): Harness, Hermes and OpenClaw,
//! installed in the image's root by the same one-shot a lamp runs after its
//! setup ([`crate::hermes_install::one_shot`]), so a new lamp's app shows
//! them installed from its first minute instead of after a download.
//!
//! What is installed, and how it is shown, is what a `serve` round on the
//! same image would ask for: [`wanted`] is the one place both read
//! lamp-update's switches from, and [`ask_missing`] writes each missing
//! part's ask with the installer's own writer
//! ([`crate::harness_install::ask_install`],
//! [`crate::hermes_install::ask_install`],
//! [`crate::openclaw_install::ask_install`]). A part already there is not
//! asked for again. OpenClaw's gateway unit, the round's to write, is
//! written first ([`write_units`]): its install checks it is this build's.
//! Nothing is started: there is no systemd in an image being built, so
//! every `systemctl` the installs run is skipped ([`on_image`]), and a
//! lamp's first round after its setup records the parts as installed,
//! writes again anything its switches show differently, and enables and
//! starts OpenClaw's gateway (whose allow-list edit, done through the
//! running gateway, it then asks for as it always does).
//!
//! The image runs this in its root by `pivot_root` in a mount namespace,
//! not `chroot`: the kernel refuses a user namespace to a chrooted process,
//! and Hermes's and OpenClaw's installs try theirs as `lampos` first
//! (`no_userns`).

use crate::{
    harness_install::{self, Pin, product},
    hermes_install, openclaw_install,
    switches::UpdateSwitches,
};
use std::path::Path;

/// What lamp-update's switches ask for the agents, as `serve` reads them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wanted {
    /// Harness before the sign-in: on or not, and its pin.
    pub harness: product::Wanted,
    /// Hermes's own switch, and what its namespace shows.
    pub hermes: (bool, hermes_install::Shown),
    /// OpenClaw's own switch, and what its namespace shows.
    pub openclaw: (bool, hermes_install::Shown),
}

/// The agents' switches on a lamp that is the product's (`product`) or not,
/// from lamp-update's environment (`env`) and its switches as read
/// (`switches`, crate::switches::update_switches). Hermes and OpenClaw ride
/// on Harness's one-shot: their own switch on says nothing about Harness,
/// which a caller checks.
pub fn wanted(
    product: bool,
    switches: UpdateSwitches,
    env: &dyn Fn(&str) -> Option<String>,
) -> Wanted {
    let harness = product::wanted(
        env(harness_install::SWITCH).as_deref(),
        env(harness_install::MANIFEST_URL).as_deref(),
        env(harness_install::MANIFEST_SHA256).as_deref(),
        product,
        product::PRODUCT_DEFAULT,
    );
    let hermes = {
        use hermes_install as hermes;
        let on = hermes::wanted(
            env(hermes::SWITCH).as_deref(),
            product,
            hermes::PRODUCT_DEFAULT,
        );
        let skills = hermes::wanted(
            env(hermes::SKILLS_SWITCH).as_deref(),
            product,
            hermes::SKILLS_PRODUCT_DEFAULT,
        );
        let shown = hermes::Shown {
            skills,
            tasks: switches.tasks,
            one_path: switches.one_path,
        };
        (on, shown)
    };
    let openclaw = {
        use openclaw_install as openclaw;
        let on = openclaw::wanted(
            env(openclaw::SWITCH).as_deref(),
            product,
            openclaw::PRODUCT_DEFAULT,
        );
        let skills = openclaw::wanted(
            env(openclaw::SKILLS_SWITCH).as_deref(),
            product,
            openclaw::SKILLS_PRODUCT_DEFAULT,
        );
        (on, openclaw::shown(skills, switches.one_path))
    };
    Wanted {
        harness,
        hermes,
        openclaw,
    }
}

/// Each part the switches want and the image lacks, its ask written in
/// `records`, in the one-shot's order: Harness, Hermes, OpenClaw. Returns
/// the names asked for. Harness off asks for nothing (Hermes and OpenClaw
/// ride on it), and a pin that cannot be read is an error, as a round
/// records it `pin_unreadable`.
pub fn ask_missing(
    root: &Path,
    records: &Path,
    wanted: &Wanted,
) -> Result<Vec<&'static str>, String> {
    if !wanted.harness.on {
        return Err("Harness before the sign-in is off, and Hermes and OpenClaw ride on it".into());
    }
    let pin: Option<Pin> = wanted.harness.pin.clone()?;
    let mut asked = Vec::new();
    if !harness_install::present(root) {
        harness_install::ask_install(records, pin)?;
        asked.push("Harness");
    }
    let (hermes_on, hermes_shown) = wanted.hermes;
    if hermes_on && !hermes_install::present(root, hermes_shown) {
        hermes_install::ask_install(records, hermes_shown).map_err(|error| error.to_string())?;
        asked.push("Hermes");
    }
    let (openclaw_on, openclaw_shown) = wanted.openclaw;
    if openclaw_on && !openclaw_install::present(root, openclaw_shown) {
        openclaw_install::ask_install(records, openclaw_shown)
            .map_err(|error| error.to_string())?;
        asked.push("OpenClaw");
    }
    Ok(asked)
}

/// OpenClaw's gateway unit, as a round writes it for what OpenClaw's
/// namespace shows, when the switches want OpenClaw: its install fails
/// (`gateway`) on any other text. Whether it was written anew.
pub fn write_units(root: &Path, wanted: &Wanted) -> Result<bool, String> {
    let (openclaw_on, openclaw_shown) = wanted.openclaw;
    if !openclaw_on {
        return Ok(false);
    }
    crate::writer_unit::write_unit(
        root,
        openclaw_install::UNIT,
        &openclaw_install::unit_text_for(openclaw_shown),
    )
}

/// `run` for an image being built: a `systemctl` command (a gateway's
/// start, lamp-tasks' restart) is said and skipped as done, as there is no
/// systemd to run it; any other runs through `run`.
pub fn on_image<'a>(
    run: &'a mut crate::install::Runner<'a>,
    said: &'a mut Vec<String>,
) -> impl FnMut(&[String]) -> Result<String, String> + 'a {
    move |args: &[String]| {
        if args.first().map(String::as_str) == Some("systemctl") {
            said.push(args.join(" "));
            return Ok(String::new());
        }
        run(args)
    }
}

/// lamp-update's own unit's `Environment=` on the image at `root`, as
/// systemd would give it to a round there: the unit file, then its drop-ins
/// in name order, each later value of a variable winning and an empty
/// `Environment=` clearing what came before. On an image there is no
/// systemd to have read it, and a switch the product's unit sets
/// (`LAMP_SKILL_TASKS`, `LAMP_SKILL_DEFAULT_AGENT`) changes what Hermes's and
/// OpenClaw's namespaces show. `EnvironmentFile=` is not read: the product's
/// unit names none (the carried `/etc/lampOS/update.env` is read by
/// crate::switches itself).
pub fn unit_environment(root: &Path) -> Vec<(String, String)> {
    use std::fs;
    let units = root.join(crate::install::UNITS.trim_start_matches('/'));
    let unit = crate::round::OWN_UNIT;
    let mut texts: Vec<String> = fs::read_to_string(units.join(unit)).into_iter().collect();
    let mut drop_ins: Vec<_> = fs::read_dir(units.join(format!("{unit}.d")))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "conf")
        })
        .collect();
    drop_ins.sort();
    texts.extend(
        drop_ins
            .iter()
            .filter_map(|path| fs::read_to_string(path).ok()),
    );
    let mut set: Vec<(String, String)> = Vec::new();
    for text in &texts {
        let found = crate::switches::environment(text);
        if found.cleared {
            set.clear();
        }
        for (name, value) in found.set {
            set.retain(|(was, _)| *was != name);
            set.push((name, value));
        }
    }
    set
}

/// Every part the switches want there after the one-shot, or the first one
/// missing.
pub fn check(root: &Path, wanted: &Wanted) -> Result<(), String> {
    if !harness_install::present(root) {
        return Err("Harness is not there".into());
    }
    let (hermes_on, hermes_shown) = wanted.hermes;
    if hermes_on && !hermes_install::present(root, hermes_shown) {
        return Err("Hermes is not there".into());
    }
    let (openclaw_on, openclaw_shown) = wanted.openclaw;
    if openclaw_on && !openclaw_install::present(root, openclaw_shown) {
        return Err("OpenClaw is not there".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
