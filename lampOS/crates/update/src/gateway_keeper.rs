//! One keeper for the lamp's two agent gateways: OpenClaw's
//! ([`openclaw_install::UNIT`], with `LAMP_OPENCLAW_GATEWAY_BY_DEFAULT` on)
//! and Hermes's ([`hermes_gateway::UNIT`], with `LAMP_HERMES_GATEWAY` on).
//!
//! **The owner's rule: never both running, at any moment.** Each follows
//! the lamp's default agent as lamp-tasks says it
//! ([`openclaw_install::said_default`]), read **once** per look for both,
//! so the two never act on different answers. A look first applies what is
//! not wanted (the unwanted gateway disabled and stopped, through each
//! module's own keep), then what is: the wanted gateway is started only
//! once the other is down. A stop that failed, or a gateway still
//! `active`, `activating` or `deactivating` (or whose state systemd does
//! not say), holds the start for this look, said once; the next look tries
//! again. A round does the same ([`round_gateways`]) before its own steps
//! start a gateway.
//!
//! **Never waiting on readiness.** A look holds main's gateway lock (the
//! round's own, so the two never touch a gateway at once), and a look that
//! waited for a cold gateway to be ready would hold a stop of the other
//! engine's behind it. So OpenClaw's agent `main` allow-list entry, owed
//! once its gateway was started here with the one run path on, is tried
//! once per look ([`openclaw_install::run_entry_once`]), remembered across
//! looks while OpenClaw is still wanted, and forgotten once it is not.
//!
//! An engine whose switch is off is not looked at: Hermes's gateway is
//! then taken out by the round (`hermes_gateway::after_round_off`), and
//! OpenClaw's, with neither switch on, is always on as before.
use crate::{
    hermes_gateway,
    install::Runner,
    openclaw_install::{self, Gateway, Said},
    world::Systemd,
};
use std::path::Path;

/// Which gateways follow the default, as main reads the switches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Kept {
    /// OpenClaw's gateway follows the default (OpenClaw on, and
    /// `LAMP_OPENCLAW_GATEWAY_BY_DEFAULT` or `LAMP_HERMES_GATEWAY` on):
    /// `Some` with the one run path (#6685), else `None`.
    pub openclaw: Option<bool>,
    /// Hermes's gateway kept (Hermes on and `LAMP_HERMES_GATEWAY` on).
    pub hermes: bool,
}

/// The keeper's state across looks.
#[derive(Debug, Default)]
pub struct Keeper {
    kept: Kept,
    /// OpenClaw's agent `main` entry owed: its gateway started here with the
    /// one run path on, and the add not known yet (the gateway not ready).
    entry_owed: bool,
    /// The owed entry's wait said once.
    entry_said: bool,
    /// A start held behind the other gateway, said once.
    held: bool,
}

impl Keeper {
    pub fn new(kept: Kept) -> Self {
        Self {
            kept,
            ..Self::default()
        }
    }

    /// Whether OpenClaw's agent `main` entry is still owed.
    pub fn entry_owed(&self) -> bool {
        self.entry_owed
    }

    /// One look, every [`openclaw_install::KEEP_EVERY`] between rounds,
    /// with lamp-tasks' answer `said` read once by the caller.
    pub fn look(
        &mut self,
        root: &Path,
        systemd: &mut dyn Systemd,
        said: Option<Said>,
        run: &mut Runner,
        say: &mut dyn FnMut(&str),
    ) {
        let one_path = self.kept.openclaw.unwrap_or(false);
        let openclaw = self
            .kept
            .openclaw
            .map(|_| openclaw_install::gateway_wanted(true, said.clone()));
        let hermes = self
            .kept
            .hermes
            .then(|| hermes_gateway::gateway_wanted(said));
        // What is not wanted first: disabled and stopped.
        if openclaw == Some(Gateway::NotWanted) {
            openclaw_install::keep_between_rounds(
                root,
                systemd,
                Gateway::NotWanted,
                one_path,
                run,
                say,
            );
        }
        if hermes == Some(Gateway::NotWanted) {
            hermes_gateway::keep_between_rounds(root, systemd, Gateway::NotWanted, say);
        }
        if openclaw != Some(Gateway::Wanted) {
            self.entry_owed = false;
            self.entry_said = false;
        }
        // Then what is: started only once the other gateway is down.
        if openclaw == Some(Gateway::Wanted) {
            if self.hold(
                self.kept.hermes && !hermes_gateway::gateway_down(systemd),
                openclaw_install::UNIT,
                hermes_gateway::UNIT,
                say,
            ) {
                return;
            }
            match openclaw_install::keep_between_rounds(
                root,
                systemd,
                Gateway::Wanted,
                one_path,
                run,
                say,
            ) {
                Some(owed) => self.entry_owed = owed,
                None if self.entry_owed && !openclaw_install::one_shot_at_work(systemd) => {
                    match openclaw_install::run_entry_once(run, say) {
                        Some(true) => self.entry_owed = false,
                        Some(false) => {
                            say(&format!(
                                "agent {}'s allow-list entry not taken once OpenClaw's gateway was ready; the next round asks the one-shot",
                                openclaw_install::RUN_AGENT
                            ));
                            self.entry_owed = false;
                        }
                        None => {}
                    }
                }
                None => {}
            }
            if !self.entry_owed {
                self.entry_said = false;
            } else if !self.entry_said {
                say(&format!(
                    "agent {}'s allow-list entry waits for OpenClaw's gateway to be ready: tried again at each look",
                    openclaw_install::RUN_AGENT
                ));
                self.entry_said = true;
            }
        } else if hermes == Some(Gateway::Wanted) {
            if self.hold(
                self.kept.openclaw.is_some() && !openclaw_install::gateway_down(systemd),
                hermes_gateway::UNIT,
                openclaw_install::UNIT,
                say,
            ) {
                return;
            }
            hermes_gateway::keep_between_rounds(root, systemd, Gateway::Wanted, say);
        } else {
            self.held = false;
        }
    }

    /// Whether `wanted`'s start is held because `other` still runs
    /// (`other_up`), said once per hold.
    fn hold(
        &mut self,
        other_up: bool,
        wanted: &str,
        other: &str,
        say: &mut dyn FnMut(&str),
    ) -> bool {
        if !other_up {
            self.held = false;
            return false;
        }
        if !self.held {
            say(&format!(
                "{wanted} not started yet: {other} still runs, and one agent gateway runs at a time; tried again at the next look"
            ));
            self.held = true;
        }
        true
    }
}

/// A round's gateway step, stop before start, with lamp-tasks' answer read
/// once by main for both: `openclaw` is OpenClaw's [`Gateway`] when its
/// round step runs (always wanted with neither switch on), `hermes`
/// Hermes's when its gateway is kept. An unwanted gateway is disabled and
/// stopped here first (a round's look); a wanted one is returned as is
/// when the other is down, else as [`Gateway::Unknown`] (left as it is
/// this round, said), so each module's own round step
/// ([`openclaw_install::after_round_with`], [`hermes_gateway::after_round`])
/// keeps its other duties and never starts a gateway beside the other.
pub fn round_gateways(
    root: &Path,
    systemd: &mut dyn Systemd,
    openclaw: Option<Gateway>,
    hermes: Option<Gateway>,
    say: &mut dyn FnMut(&str),
) -> (Option<Gateway>, Option<Gateway>) {
    if openclaw == Some(Gateway::NotWanted) {
        openclaw_install::keep_gateway(root, systemd, Gateway::NotWanted, true, say);
    }
    if hermes == Some(Gateway::NotWanted) {
        hermes_gateway::keep_gateway(root, systemd, Gateway::NotWanted, true, say);
    }
    let mut held = |wanted: &str, other: &str| {
        say(&format!(
            "{wanted} not started by this round: {other} still runs, and one agent gateway runs at a time"
        ));
        Some(Gateway::Unknown)
    };
    let openclaw = match openclaw {
        Some(Gateway::Wanted) if hermes.is_some() && !hermes_gateway::gateway_down(systemd) => {
            held(openclaw_install::UNIT, hermes_gateway::UNIT)
        }
        gateway => gateway,
    };
    let hermes = match hermes {
        Some(Gateway::Wanted) if openclaw.is_some() && !openclaw_install::gateway_down(systemd) => {
            held(hermes_gateway::UNIT, openclaw_install::UNIT)
        }
        gateway => gateway,
    };
    (openclaw, hermes)
}
