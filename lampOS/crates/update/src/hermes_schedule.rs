//! When the lamp's Hermes may be installed, and how its install gives way
//! while it runs (#5978; docs/install.md, "Hermes on the lamp", "When the
//! install runs"). The product has Hermes on by default (the product
//! owner, 10 Nov). The lead's rule (V2, 15:44 on #5978), as V1 did, which
//! this module holds:
//!
//! 1. **When it may start** ([`window`]): in the first round reached,
//!    with no quiet stretch, no night and no wait for the walk to end;
//!    only never while the setup walk itself is under way. Harness's own
//!    install is not held by it, and Hermes's rides after it in Harness's
//!    one-shot, at the build's Harness pin.
//! 2. **While someone speaks** ([`Watch`]): the one-shot's cgroup is frozen
//!    as soon as the voice is not quiet, and thawed once it has rested
//!    [`RESTED`]. Frozen [`MOST_FROZEN`] in all, the install is stopped
//!    (`interrupted`) and tried again in the next round.
//! 3. **The privacy reading lost** ([`Watch`]): the install is stopped at
//!    once (`privacy_lost`), and tried again in the next round.
//!
//! Pure, every reading and every clock handed in, but for [`Watcher`]:
//! `serve`'s thread that listens to the voice on the bus all along and
//! watches each install, as the rounds go on.
use crate::{
    hermes_install::{self, Carried},
    openclaw_install,
    quiet::{BETWEEN_TURNS, Heard, Quiet},
    world::{BusEars, Ears, Systemctl, Systemd},
};
use std::{
    path::Path,
    thread,
    time::{Duration, Instant},
};

/// How long the voice has rested, nobody heard, before a frozen install
/// goes on: a few seconds between turns, not a quiet stretch.
pub const RESTED: Duration = BETWEEN_TURNS;
/// The longest the install is kept frozen, all its freezes together:
/// raised from 30 to 40 min so a busy office still finishes (the lead, V2
/// 15:44 on #5978). With a Hermes stage's own bound
/// (`crate::install::INSTALLER_WITHIN`, 15 min) it ends inside the
/// one-shot's `TimeoutStartSec` (an hour), which counts on while it is
/// frozen.
pub const MOST_FROZEN: Duration = Duration::from_secs(40 * 60);
/// The voice's privacy lease is renewed every 10 ms and lasts 50 ms
/// (`lamp_msgs::MicrophonePrivacy`). The newest one older than this, once a
/// fresh one was seen, is the reading lost: past its life by the watch's
/// own look and a little slack, so a late look is not taken for a loss.
pub const LOST_AFTER: Duration = Duration::from_millis(250);
/// How often the watch looks while the install runs.
pub const LOOK_EVERY: Duration = Duration::from_millis(100);
/// How long into an install the watch waits for a privacy lease before it
/// says, once, that it runs with no privacy watch (the voice renews its
/// lease every 10 ms, so a working bus has one long before this).
pub const NO_LEASE_SAID_AFTER: Duration = Duration::from_secs(1);

/// What [`window`] is told.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Moment {
    /// The setup walk under way (`crate::walk::under_way`).
    pub walk_under_way: bool,
}

/// Why the install may start now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Window {
    /// A round reached the channel, the setup walk not under way.
    Reached,
}

impl Window {
    pub fn name(self) -> &'static str {
        match self {
            Self::Reached => "a round reached, the setup walk not under way",
        }
    }
}

/// Whether a Hermes install may start now, or why not, for a person: in
/// any round reached (the first one too), whatever the voice is doing,
/// but never while the setup walk is under way.
pub fn window(moment: &Moment) -> Result<Window, String> {
    if moment.walk_under_way {
        return Err(crate::walk::UNDER_WAY.to_owned());
    }
    Ok(Window::Reached)
}

/// The voice's newest privacy lease as the watch reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lease {
    /// Fresh (younger than [`LOST_AFTER`]) and valid: the voice's boot
    /// that said it, the slide's level when read (`physical_muted`), and
    /// whether it is a lamp with no slide.
    Fresh {
        boot: String,
        physical: Option<bool>,
        no_slide: bool,
    },
    /// Older than [`LOST_AFTER`], not valid, or none.
    Gone,
}

impl Lease {
    /// A reading the voice can open its microphones on: the slide read, or
    /// a lamp with none.
    fn read(&self) -> Option<&str> {
        match self {
            Self::Fresh {
                boot,
                physical,
                no_slide,
            } if physical.is_some() || *no_slide => Some(boot),
            _ => None,
        }
    }
}

/// What the watch does now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    Nothing,
    /// Freeze the one-shot: someone is heard, or the voice is not seen.
    Freeze(String),
    /// Thaw it: rested again for [`RESTED`].
    Thaw,
    /// Stop it and record the install `failed` with `error`.
    Stop {
        error: &'static str,
        why: String,
    },
}

/// The watch over one Hermes install: started when the install is seen
/// running, dropped when it ends.
#[derive(Clone, Debug, Default)]
pub struct Watch {
    /// When the current freeze began, on the watch's clock.
    frozen_since_ms: Option<u64>,
    /// The freezes before the current one, in all.
    frozen_ms: u64,
    /// The voice's boot whose lease was last read with the slide known:
    /// none until one is (a lamp whose voice says none is not judged).
    read_from: Option<String>,
    /// When the watch first looked, on its clock.
    began_ms: Option<u64>,
    /// Whether "no privacy watch" was said for this install.
    said_no_lease: bool,
}

impl Watch {
    /// Whether the one-shot is frozen now, as the watch has it.
    pub fn frozen(&self) -> bool {
        self.frozen_since_ms.is_some()
    }

    /// One look at `now_ms`: `quiet`, rested for [`RESTED`], and the newest
    /// privacy `lease`. A reading lost after one was seen stops the install
    /// at once, frozen or not: the lease gone, the slide no longer read, or
    /// a lease from another of the voice's boots (it ended and began
    /// again). Frozen [`MOST_FROZEN`] in all stops it too. Otherwise it is
    /// frozen while the voice is not quiet, and thawed once it has rested.
    pub fn look(&mut self, now_ms: u64, quiet: Result<(), String>, lease: &Lease) -> Act {
        match (lease.read(), &self.read_from) {
            (Some(boot), Some(before)) if boot != before => {
                return self.stop(
                    "privacy_lost",
                    "the voice's privacy lease comes from another of its boots: the reading was lost"
                        .to_owned(),
                );
            }
            (None, Some(_)) => {
                let why = match lease {
                    Lease::Gone => format!(
                        "the voice's privacy lease is gone or older than {} ms",
                        LOST_AFTER.as_millis()
                    ),
                    Lease::Fresh { .. } => {
                        "the voice's privacy lease no longer reads the slide".to_owned()
                    }
                };
                return self.stop("privacy_lost", why);
            }
            (Some(boot), _) => self.read_from = Some(boot.to_owned()),
            (None, None) => {}
        }
        match self.frozen_since_ms {
            Some(since) => {
                let frozen = self.frozen_ms + now_ms.saturating_sub(since);
                if frozen >= MOST_FROZEN.as_millis() as u64 {
                    return self.stop(
                        "interrupted",
                        format!(
                            "frozen {} min in all while the voice was not quiet",
                            frozen / 60_000
                        ),
                    );
                }
                if quiet.is_ok() {
                    self.frozen_ms = frozen;
                    self.frozen_since_ms = None;
                    return Act::Thaw;
                }
                Act::Nothing
            }
            None => match quiet {
                Ok(()) => Act::Nothing,
                Err(why) => {
                    self.frozen_since_ms = Some(now_ms);
                    Act::Freeze(why)
                }
            },
        }
    }

    /// The line to say once when no privacy lease was read with the slide
    /// known in the install's first [`NO_LEASE_SAID_AFTER`]: such an install
    /// runs with no privacy watch (a bus permission or a renamed topic would
    /// look the same), and the journal says so. `None` otherwise, and after
    /// it was said once.
    pub fn no_lease_line(&mut self, now_ms: u64) -> Option<String> {
        let began = *self.began_ms.get_or_insert(now_ms);
        if self.read_from.is_some() || self.said_no_lease {
            return None;
        }
        if now_ms.saturating_sub(began) < NO_LEASE_SAID_AFTER.as_millis() as u64 {
            return None;
        }
        self.said_no_lease = true;
        Some(format!(
            "Hermes's install runs with no privacy watch: no privacy lease with the slide read was seen on voice/privacy in its first {} ms",
            NO_LEASE_SAID_AFTER.as_millis()
        ))
    }

    fn stop(&mut self, error: &'static str, why: String) -> Act {
        self.frozen_since_ms = None;
        Act::Stop { error, why }
    }
}

/// [`Quiet::for_at_least`] for [`RESTED`]: what the watch asks before it
/// thaws, and why it freezes.
pub fn rested(quiet: &Quiet, now_ms: u64) -> Result<(), String> {
    quiet.for_at_least(RESTED, now_ms)
}

/// The name the watch subscribes to the voice under, beside the rounds'
/// own (`crate::world::NAME`).
pub const LISTENER: &str = "update_hermes";

/// `serve`'s watch: a thread that hears the voice from its start and
/// watches each Hermes install while the record says one is under way,
/// freezing, thawing or stopping Harness's one-shot as [`Watch::look`]
/// says.
pub struct Watcher;

impl Watcher {
    /// Started for a `serve` with Hermes on. First a one-shot left frozen
    /// by a `serve` that ended mid-freeze is thawed, so none is left
    /// frozen for good.
    pub fn start(bus_root: &Path, records: &Path) -> Self {
        let began = Instant::now();
        let _ = Systemctl::default().thaw_harness_unit();
        let (bus_root, records) = (bus_root.to_owned(), records.to_owned());
        let started = thread::Builder::new()
            .name("hermes-watch".to_owned())
            .spawn(move || watch_installs(&bus_root, &records, began));
        if let Err(error) = started {
            // Nothing watches the install: it runs with no freeze and no
            // privacy watch.
            eprintln!("lamp-update: the Hermes install's watch did not start: {error}");
        }
        Self
    }
}

/// The watch's thread: listens [`LOOK_EVERY`], hands what it heard to its
/// quiet, and, while [`hermes_install::installing`] (read once a second)
/// says an install is under way, looks with a [`Watch`] of its own and
/// carries out what it says ([`hermes_install::carry_out`]).
fn watch_installs(bus_root: &Path, records: &Path, began: Instant) {
    let mut say = |line: &str| eprintln!("lamp-update: {line}");
    let mut ears = BusEars::named(bus_root, LISTENER, &mut say);
    let mut systemd = Systemctl::default();
    let mut heard = Quiet::new(0);
    let mut watch: Option<Watch> = None;
    for look in 0u64.. {
        let said: Vec<Heard> = ears.listen(LOOK_EVERY);
        let now = began.elapsed().as_millis() as u64;
        for one in said {
            heard.hear(one, now);
        }
        let quiet = rested(&heard, now);
        if look % 10 == 0 {
            // Hermes's or OpenClaw's (#6226): one watch, one one-shot.
            let under_way =
                hermes_install::installing(records) || openclaw_install::installing(records);
            if !under_way {
                watch = None;
            } else if watch.is_none() {
                watch = Some(Watch::default());
            }
        }
        let Some(current) = &mut watch else {
            continue;
        };
        let act = current.look(now, quiet, &ears.lease());
        if let Some(line) = current.no_lease_line(now) {
            say(&line);
        }
        let unix_ms = crate::records::unix_ms();
        match carry_out_turns(&act, records, &mut systemd, unix_ms, &mut say) {
            Carried::Done => {}
            Carried::NotRunning | Carried::Stopped => watch = None,
        }
    }
}

/// The watch's `act` carried out on Harness's one-shot for the install
/// whose turn it is (#6226): Hermes's while its record says it is under
/// way, else OpenClaw's. One freeze or thaw serves both; a stop stops the
/// one-shot whole, so the other's install, under way in it too, is
/// recorded stopped as well.
pub fn carry_out_turns(
    act: &Act,
    records: &Path,
    systemd: &mut dyn Systemd,
    now_unix_ms: u64,
    say: &mut dyn FnMut(&str),
) -> Carried {
    if *act == Act::Nothing {
        return Carried::Done;
    }
    let (turn, other) = if hermes_install::installing(records) {
        (&hermes_install::TURN, &openclaw_install::TURN)
    } else {
        (&openclaw_install::TURN, &hermes_install::TURN)
    };
    hermes_install::carry_out_for(turn, &[other], act, records, systemd, now_unix_ms, say)
}

#[cfg(test)]
mod tests;
