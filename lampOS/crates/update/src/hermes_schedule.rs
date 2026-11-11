//! When the lamp's Hermes may be installed, and how its install gives way
//! while it runs (#5978; docs/install.md, "Hermes on the lamp", "When the
//! install runs"). Measured on a 4 GB lamp with an SD card, the install
//! (about 2.4 GB of writes, 7 to 12 min) slipped lamp-controls' 50 ms
//! privacy reading and kept the voice's microphones closed, and its
//! `products` stage brought `MemAvailable` down to about 868 MB. The
//! product has Hermes on by default (the product owner, 10 Nov) under the
//! first builder's rules, which this module holds:
//!
//! 1. **When it may start** ([`window`]): never in the first round after
//!    lamp-update starts (a boot or an update's restart); never while the
//!    setup walk is under way; only in a quiet stretch, nobody heard and
//!    the voice resting for [`QUIET_STRETCH`]; and then either right after
//!    the setup walk ended (its first try since, within
//!    [`AFTER_WALK_LASTS`]), by day at the first quiet stretch of a lamp
//!    that never tried (it just took the update), or in the quiet hour at night
//!    ([`NIGHT_FROM_MINUTE`] to [`NIGHT_TO_MINUTE`], the lamp's own time).
//!    Harness's own install is not held by it.
//! 2. **While someone speaks** ([`Watch`]): the one-shot's cgroup is frozen
//!    as soon as the voice is not quiet, and thawed once the quiet stretch
//!    is back. Frozen [`MOST_FROZEN`] in all, the install is stopped
//!    (`interrupted`) and tried again in a later window.
//! 3. **The privacy reading lost** ([`Watch`]): the install is stopped at
//!    once (`privacy_lost`), and tried again in a later window.
//!
//! Pure, every reading and every clock handed in, but for [`Watcher`]:
//! `serve`'s thread that listens to the voice on the bus all along (the
//! quiet stretch needs [`QUIET_STRETCH`] of listening) and watches each
//! install, as the rounds go on.
use crate::{
    hermes_install::{self, Carried, HermesInstall},
    quiet::{Heard, Quiet},
    world::{BusEars, Ears, Systemctl, Systemd},
};
use std::{
    path::Path,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

/// How long nobody has been heard, and the voice has rested, before the
/// install starts, and before a frozen one goes on.
pub const QUIET_STRETCH: Duration = Duration::from_secs(10 * 60);
/// How long after the setup walk's end its first quiet stretch may start
/// the install, by day; past it, only the night.
pub const AFTER_WALK_LASTS: Duration = Duration::from_secs(6 * 60 * 60);
/// The quiet hour at night, in local minutes of the day: 01:00 up to 05:00.
pub const NIGHT_FROM_MINUTE: u16 = 60;
pub const NIGHT_TO_MINUTE: u16 = 5 * 60;
/// A wall clock before this (2026-01-01 UTC) is taken as not set yet: no
/// local time is known from it.
pub const CLOCK_SET_AFTER_UNIX_MS: u64 = 1_767_225_600_000;
/// The longest the install is kept frozen, all its freezes together. With
/// the install's own 7 to 12 min, it ends inside the one-shot's
/// `TimeoutStartSec` (an hour), which counts on while it is frozen.
pub const MOST_FROZEN: Duration = Duration::from_secs(30 * 60);
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
    /// The round just ended, counted from 1 since lamp-update started.
    pub round: u64,
    /// Whether the voice has been quiet for [`QUIET_STRETCH`], or why not
    /// ([`Quiet::for_at_least`]).
    pub quiet: Result<(), String>,
    /// The setup walk under way (`crate::walk::under_way`).
    pub walk_under_way: bool,
    /// When the setup walk handed out its done line, Unix ms (its progress
    /// file's `done_at_unix_ms`); none before it, or for a lamp whose voice
    /// does not walk.
    pub walk_ended_unix_ms: Option<u64>,
    /// When the last Hermes install began or failed ([`last_try_unix_ms`]).
    pub tried_unix_ms: Option<u64>,
    /// The wall clock, Unix ms.
    pub now_unix_ms: u64,
    /// The lamp's local minute of the day now, none when not known (the
    /// zone not read, or the clock not set).
    pub local_minute: Option<u16>,
    /// The person asked for the install from the app (#6240,
    /// [`crate::engines`]) and no round has started it yet.
    pub asked: bool,
}

/// Why the install may start now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Window {
    /// The first quiet stretch after the setup walk ended.
    AfterWalk,
    /// The quiet hour at night.
    Night,
    /// By day, the first quiet stretch of a lamp that never tried: one
    /// that took the update with Hermes on (the first builder's day
    /// window, 11 Nov 01:15 on #6059), so it is usable the same day. A try
    /// that failed or was stopped waits for the night.
    AfterUpdate,
    /// The person asked for it from the app (#6240): past the quiet
    /// stretch and the clock windows, as a tap is a person's own choice;
    /// still never in the first round nor while the setup walk is under
    /// way, and the install still freezes while someone speaks.
    Asked,
}

impl Window {
    pub fn name(self) -> &'static str {
        match self {
            Self::AfterWalk => "the first quiet stretch after the setup walk",
            Self::Night => "the quiet hour at night",
            Self::AfterUpdate => "the first quiet stretch after the update, by day",
            Self::Asked => "asked from the app",
        }
    }
}

/// Whether a Hermes install may start now, and in which window, or why
/// not, for a person.
pub fn window(moment: &Moment) -> Result<Window, String> {
    if moment.round <= 1 {
        return Err("never in the first round after lamp-update starts".to_owned());
    }
    if moment.walk_under_way {
        return Err(crate::walk::UNDER_WAY.to_owned());
    }
    if moment.asked {
        return Ok(Window::Asked);
    }
    if let Err(why) = &moment.quiet {
        return Err(format!("not a quiet stretch: {why}"));
    }
    let clock_set = moment.now_unix_ms >= CLOCK_SET_AFTER_UNIX_MS;
    if let Some(ended) = moment.walk_ended_unix_ms
        && clock_set
        && moment.now_unix_ms >= ended
        && moment.now_unix_ms - ended < AFTER_WALK_LASTS.as_millis() as u64
        && moment.tried_unix_ms.is_none_or(|tried| tried < ended)
    {
        return Ok(Window::AfterWalk);
    }
    if moment.tried_unix_ms.is_none() {
        return Ok(Window::AfterUpdate);
    }
    match moment.local_minute.filter(|_| clock_set) {
        Some(minute) if (NIGHT_FROM_MINUTE..NIGHT_TO_MINUTE).contains(&minute) => Ok(Window::Night),
        Some(_) => Err(format!(
            "outside the quiet hour at night ({:02}:00 to {:02}:00) and not right after the setup walk",
            NIGHT_FROM_MINUTE / 60,
            NIGHT_TO_MINUTE / 60
        )),
        None => Err(
            "the lamp's local time is not known, and it is not right after the setup walk"
                .to_owned(),
        ),
    }
}

/// When the last install began or failed, from Hermes's record: none for
/// none, `absent` or `installed`.
pub fn last_try_unix_ms(said: Option<&HermesInstall>) -> Option<u64> {
    said.filter(|said| matches!(said.install.as_str(), "installing" | "failed"))
        .map(|said| said.at_unix_ms)
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
    /// Thaw it: quiet again for [`QUIET_STRETCH`].
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

    /// One look at `now_ms`: `quiet` for [`QUIET_STRETCH`], and the newest
    /// privacy `lease`. A reading lost after one was seen stops the install
    /// at once, frozen or not: the lease gone, the slide no longer read, or
    /// a lease from another of the voice's boots (it ended and began
    /// again). Frozen [`MOST_FROZEN`] in all stops it too. Otherwise it is
    /// frozen while the voice is not quiet, and thawed once it is again.
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

/// [`Quiet::for_at_least`] for [`QUIET_STRETCH`]: what the window and the
/// watch both ask.
pub fn quiet_stretch(quiet: &Quiet, now_ms: u64) -> Result<(), String> {
    quiet.for_at_least(QUIET_STRETCH, now_ms)
}

/// The name the watch subscribes to the voice under, beside the rounds'
/// own (`crate::world::NAME`).
pub const LISTENER: &str = "update_hermes";

/// `serve`'s watch: a thread that hears the voice from its start, so a
/// round can ask whether it has been quiet for [`QUIET_STRETCH`]
/// ([`Watcher::quiet`]), and that watches each Hermes install while the
/// record says one is under way, freezing, thawing or stopping Harness's
/// one-shot as [`Watch::look`] says.
pub struct Watcher {
    heard: Arc<Mutex<Quiet>>,
    began: Instant,
    frozen: Arc<AtomicBool>,
}

impl Watcher {
    /// Started for a `serve` with Hermes on. First a one-shot left frozen
    /// by a `serve` that ended mid-freeze is thawed, so none is left
    /// frozen for good.
    pub fn start(bus_root: &Path, records: &Path) -> Self {
        let began = Instant::now();
        let heard = Arc::new(Mutex::new(Quiet::new(0)));
        let _ = Systemctl::default().thaw_harness_unit();
        let (bus_root, records) = (bus_root.to_owned(), records.to_owned());
        let shared = Arc::clone(&heard);
        let frozen = Arc::new(AtomicBool::new(false));
        let held = Arc::clone(&frozen);
        let started = thread::Builder::new()
            .name("hermes-watch".to_owned())
            .spawn(move || watch_installs(&bus_root, &records, &shared, &held, began));
        if let Err(error) = started {
            // Nothing hears the voice: the window never opens.
            eprintln!("lamp-update: the Hermes install's watch did not start: {error}");
        }
        Self {
            heard,
            began,
            frozen,
        }
    }

    /// Set while the watch holds the install's one-shot frozen, cleared
    /// once it is thawed or stopped, or the install ended (#6240: the
    /// app's `frozen`).
    pub fn frozen(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.frozen)
    }

    /// Whether the voice has been quiet for [`QUIET_STRETCH`], or why not.
    pub fn quiet(&self) -> Result<(), String> {
        let now = self.began.elapsed().as_millis() as u64;
        let heard = self.heard.lock().unwrap_or_else(PoisonError::into_inner);
        quiet_stretch(&heard, now)
    }
}

/// The watch's thread: listens [`LOOK_EVERY`], hands what it heard to the
/// quiet, and, while [`hermes_install::installing`] (read once a second)
/// says an install is under way, looks with a [`Watch`] of its own and
/// carries out what it says ([`hermes_install::carry_out`]).
fn watch_installs(
    bus_root: &Path,
    records: &Path,
    heard: &Mutex<Quiet>,
    frozen: &AtomicBool,
    began: Instant,
) {
    let mut say = |line: &str| eprintln!("lamp-update: {line}");
    let mut ears = BusEars::named(bus_root, LISTENER, &mut say);
    let mut systemd = Systemctl::default();
    let mut watch: Option<Watch> = None;
    for look in 0u64.. {
        let said: Vec<Heard> = ears.listen(LOOK_EVERY);
        let now = began.elapsed().as_millis() as u64;
        let quiet = {
            let mut heard = heard.lock().unwrap_or_else(PoisonError::into_inner);
            for one in said {
                heard.hear(one, now);
            }
            quiet_stretch(&heard, now)
        };
        if look % 10 == 0 {
            let under_way = hermes_install::installing(records);
            if !under_way {
                watch = None;
            } else if watch.is_none() {
                watch = Some(Watch::default());
            }
        }
        let Some(current) = &mut watch else {
            frozen.store(false, Ordering::Relaxed);
            continue;
        };
        let act = current.look(now, quiet, &ears.lease());
        if let Some(line) = current.no_lease_line(now) {
            say(&line);
        }
        let unix_ms = crate::records::unix_ms();
        match hermes_install::carry_out(&act, records, &mut systemd, unix_ms, &mut say) {
            Carried::Done => {}
            Carried::NotRunning | Carried::Stopped => watch = None,
        }
        frozen.store(watch.as_ref().is_some_and(Watch::frozen), Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests;
