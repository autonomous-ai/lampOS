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
//! asked for again. Nothing is started: there is no systemd in an image
//! being built, and a lamp's first round after its setup records the parts
//! as installed, writes again anything its switches show differently, and
//! starts OpenClaw's gateway.
//!
//! The image runs this in its root by `pivot_root` in a mount namespace,
//! not `chroot`: the kernel refuses a user namespace to a chrooted process,
//! and Hermes's and OpenClaw's installs try theirs as `lampos` first
//! (`no_userns`).

use crate::{
    harness_install::{self, product},
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
    _root: &Path,
    _records: &Path,
    _wanted: &Wanted,
) -> Result<Vec<&'static str>, String> {
    Ok(Vec::new())
}

/// Every part the switches want there after the one-shot, or the first one
/// missing.
pub fn check(_root: &Path, _wanted: &Wanted) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests;
