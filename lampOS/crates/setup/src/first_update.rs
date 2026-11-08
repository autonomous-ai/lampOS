//! The first update before the walk (#5458): a lamp flashed with an older
//! image takes stable's build between the join and the marker, so its first
//! walk runs stable's code and not the image's.
//!
//! Only with `LAMP_SETUP_UPDATE_FIRST=on` and an app that asks for it
//! (`update_first: true` in the setup request): an app that doesn't know it
//! gives up on a setup that takes as long as an update, so it gets setup as
//! before. Between the join and the marker nothing of the product runs (every
//! part needs the marker), so the switch restarts no voice and no body, and
//! the walk isn't under way, so the updater doesn't hold for it.
//!
//! Setup asks `lamp-update serve` for a round now, as the app's Check for
//! update does (`update/check`, #4153), and follows that round on
//! `update/status`. A round that only switched the updater itself
//! (`self-switched`) is followed by the new updater's own round. An
//! unreachable round may have begun before the join, so it is asked once
//! more. Any other end of a round, an updater that says nothing, or
//! [`MOST`] passing ends the wait, and the marker is written on whatever
//! build the lamp has: a failed or slow update never strands setup.
//!
//! While setup waits, `/var/lib/lampos-setup/first-update` is there
//! ([`WAITING`]), so the updater leaves setup's own unit alone as it does a
//! completed setup's (`lamp_update::setup_lifecycle`): a restart would end
//! the wait mid-round.
use lamp_msgs::{UpdateCheck, UpdateResult, UpdateStatus};
use std::{
    io,
    time::{Duration, Instant},
};

/// The switch, in `lamp-setup.service`'s environment.
pub const SWITCH: &str = "LAMP_SETUP_UPDATE_FIRST";
/// The longest setup waits for the first update, from the ask to the round's
/// end: a whole build fetched on a slow home network, the updater's own
/// climb first when the image's is too old, the switch and its watch.
pub const MOST: Duration = Duration::from_secs(10 * 60);
/// How long the updater has to be heard at all. It says its status at start
/// and once a minute, and keeps the newest on the bus, so a running one is
/// heard at once.
pub const HEARD_WITHIN: Duration = Duration::from_secs(20);
/// How long after an ask a round must begin, before setup asks again. A
/// round under way when the ask came drops it (`app_check`).
pub const BEGINS_WITHIN: Duration = Duration::from_secs(20);
/// How often the wait looks at the status.
pub const LOOK: Duration = Duration::from_secs(1);
/// The file there while setup waits, under setup's state folder.
pub const WAITING: &str = "first-update";

/// Whether the switch is on: exactly `on`.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// What setup reads and says to the updater: the bus on a lamp, a script in
/// tests.
pub trait Updates {
    /// Asks for a round now (`update/check`).
    fn ask(&mut self) -> io::Result<()>;
    /// The newest status the updater keeps, if any.
    fn newest(&mut self) -> Option<UpdateStatus>;
    /// The next status said, waiting at most `within`.
    fn next(&mut self, within: Duration) -> Option<UpdateStatus>;
}

/// How the wait ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ended {
    /// A round ended, by its outcome's name (`switched`, `up-to-date`, ...).
    Round(String),
    /// No status from the updater in [`HEARD_WITHIN`].
    Unheard,
    /// The ask couldn't be said on the bus.
    NotAsked(String),
    /// [`MOST`] passed.
    TooLong,
}

impl Ended {
    /// Whether the lamp now runs a newer build than before the wait.
    pub fn switched(&self) -> bool {
        matches!(self, Self::Round(outcome) if outcome == "switched")
    }
}

/// Waits for the first update, telling `phase` each phase a round says
/// (`update/status`'s `now`) and `""` at the end.
pub fn wait(
    updates: &mut impl Updates,
    most: Duration,
    now: impl Fn() -> Instant,
    phase: &mut dyn FnMut(&str),
) -> Ended {
    let started = now();
    let mut before = match updates.newest() {
        Some(status) => status.last,
        None => loop {
            if now().duration_since(started) >= HEARD_WITHIN {
                return Ended::Unheard;
            }
            if let Some(status) = updates.next(LOOK) {
                break status.last;
            }
        },
    };
    if let Err(error) = updates.ask() {
        return Ended::NotAsked(error.to_string());
    }
    let mut asked = now();
    // Whether a round has said a phase since the last ask.
    let mut began = false;
    let mut unreachable_again = true;
    let ended = loop {
        let at = now();
        if at.duration_since(started) >= most {
            break Ended::TooLong;
        }
        let Some(status) = updates.next(LOOK) else {
            if !began && at.duration_since(asked) >= BEGINS_WITHIN.max(UpdateCheck::AGAIN_AFTER) {
                if let Err(error) = updates.ask() {
                    break Ended::NotAsked(error.to_string());
                }
                asked = now();
            }
            continue;
        };
        if status.now != "idle" {
            began = true;
            phase(&status.now);
            continue;
        }
        if !began || status.last == before {
            continue;
        }
        let outcome = status
            .last
            .as_ref()
            .map_or_else(String::new, |last: &UpdateResult| last.outcome.clone());
        before = status.last;
        began = false;
        match outcome.as_str() {
            // The new updater does its own round at once, unasked.
            "self-switched" => asked = now(),
            // Begun before the join, maybe: asked once more, after the
            // least time between two checks.
            "unreachable" if unreachable_again => {
                unreachable_again = false;
                asked = now();
            }
            _ => break Ended::Round(outcome),
        }
    };
    phase("");
    ended
}

/// [`Updates`] on the lamp's bus.
pub struct OnTheBus {
    bus: lamp_bus::Bus,
    asks: lamp_bus::Publisher,
    statuses: lamp_bus::Subscriber,
}

impl OnTheBus {
    /// Setup's publisher of `update/check` and its subscription to
    /// `update/status`, on the lamp's bus (`LAMP_BUS_ROOT`, or
    /// `/run/lampos-bus`, which is there before setup is done).
    pub fn new() -> io::Result<Self> {
        let bus = lamp_bus::Bus::from_environment()?;
        let asks = bus.publisher_declared(
            UpdateCheck::TOPIC,
            &UpdateCheck::declaration(),
            crate::SUBSCRIBER,
        )?;
        let statuses =
            bus.subscribe_typed(UpdateStatus::TOPIC, crate::SUBSCRIBER, UpdateStatus::TYPE)?;
        Ok(Self {
            bus,
            asks,
            statuses,
        })
    }
}

impl Updates for OnTheBus {
    fn ask(&mut self) -> io::Result<()> {
        self.asks.publish_json(0, &UpdateCheck {}).map(drop)
    }

    fn newest(&mut self) -> Option<UpdateStatus> {
        let message = self.bus.latest(UpdateStatus::TOPIC).ok()??;
        if message.expired {
            return None;
        }
        serde_json::from_slice(&message.payload).ok()
    }

    fn next(&mut self, within: Duration) -> Option<UpdateStatus> {
        let message = self.statuses.recv(within).ok()??;
        serde_json::from_slice(&message.payload).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::UpdateResult;
    use std::{cell::Cell, collections::VecDeque, rc::Rc};

    /// A scripted updater: each look takes the next of `said` (None: nothing
    /// said in that look) and moves the clock on by the look.
    struct Script {
        newest: Option<UpdateStatus>,
        said: VecDeque<Option<UpdateStatus>>,
        asks: usize,
        clock: Rc<Cell<Duration>>,
    }

    impl Updates for Script {
        fn ask(&mut self) -> io::Result<()> {
            self.asks += 1;
            Ok(())
        }
        fn newest(&mut self) -> Option<UpdateStatus> {
            self.newest.clone()
        }
        fn next(&mut self, within: Duration) -> Option<UpdateStatus> {
            self.clock.set(self.clock.get() + within);
            self.said.pop_front().flatten()
        }
    }

    fn status(now: &str, last: Option<(&str, u64)>) -> UpdateStatus {
        UpdateStatus {
            running: None,
            now: now.into(),
            last: last.map(|(outcome, at)| UpdateResult {
                outcome: outcome.into(),
                sequence: None,
                at_unix_ms: at,
                reason: None,
                started_at_unix_ms: None,
            }),
            harness: None,
            round: None,
        }
    }

    fn run(
        newest: Option<UpdateStatus>,
        said: Vec<Option<UpdateStatus>>,
    ) -> (Ended, usize, Vec<String>, Duration) {
        let clock = Rc::new(Cell::new(Duration::ZERO));
        let base = Instant::now();
        let mut script = Script {
            newest,
            said: said.into(),
            asks: 0,
            clock: Rc::clone(&clock),
        };
        let mut phases = Vec::new();
        let ended = wait(&mut script, MOST, || base + clock.get(), &mut |now| {
            phases.push(now.to_owned())
        });
        (ended, script.asks, phases, clock.get())
    }

    #[test]
    fn a_round_asked_for_is_followed_to_its_end() {
        let (ended, asks, phases, _) = run(
            Some(status("idle", Some(("unreachable", 1)))),
            vec![
                None,
                Some(status("checking", Some(("unreachable", 1)))),
                Some(status("fetching", Some(("unreachable", 1)))),
                Some(status("switching", Some(("unreachable", 1)))),
                Some(status("watching", Some(("unreachable", 1)))),
                Some(status("idle", Some(("switched", 2)))),
            ],
        );
        assert_eq!(ended, Ended::Round("switched".into()));
        assert!(ended.switched());
        assert_eq!(asks, 1);
        assert_eq!(
            phases,
            ["checking", "fetching", "switching", "watching", ""]
        );
    }

    #[test]
    fn the_updaters_own_climb_is_followed_by_its_new_round() {
        let (ended, asks, _, _) = run(
            Some(status("idle", None)),
            vec![
                Some(status("checking", None)),
                Some(status("switching", None)),
                Some(status("idle", Some(("self-switched", 2)))),
                // The new updater starts, says its records' last, and goes on.
                Some(status("idle", Some(("self-switched", 2)))),
                Some(status("checking", Some(("self-switched", 2)))),
                Some(status("idle", Some(("up-to-date", 3)))),
            ],
        );
        assert_eq!(ended, Ended::Round("up-to-date".into()));
        assert!(!ended.switched());
        assert_eq!(asks, 1, "the new updater's round is its own");
    }

    #[test]
    fn an_unreachable_round_is_asked_once_more_then_ends_it() {
        let mut said = vec![
            Some(status("checking", None)),
            Some(status("idle", Some(("unreachable", 2)))),
        ];
        // The least time between checks, then a second round.
        said.extend((0..61).map(|_| None));
        said.extend([
            Some(status("checking", Some(("unreachable", 2)))),
            Some(status("idle", Some(("unreachable", 3)))),
        ]);
        let (ended, asks, _, _) = run(Some(status("idle", None)), said);
        assert_eq!(ended, Ended::Round("unreachable".into()));
        assert_eq!(asks, 2);
    }

    #[test]
    fn an_ask_dropped_while_a_round_ran_is_said_again() {
        let mut said: Vec<Option<UpdateStatus>> = (0..61).map(|_| None).collect();
        said.extend([
            Some(status("checking", None)),
            Some(status("idle", Some(("not-quiet", 2)))),
        ]);
        let (ended, asks, _, _) = run(Some(status("idle", None)), said);
        assert_eq!(ended, Ended::Round("not-quiet".into()));
        assert_eq!(asks, 2);
    }

    #[test]
    fn an_updater_never_heard_ends_the_wait_at_once() {
        let (ended, asks, _, waited) = run(None, Vec::new());
        assert_eq!(ended, Ended::Unheard);
        assert_eq!(asks, 0);
        assert_eq!(waited, HEARD_WITHIN);
    }

    #[test]
    fn a_round_that_never_ends_ends_the_wait_at_the_bound() {
        let mut said = vec![Some(status("fetching", None))];
        said.extend((0..2_000).map(|_| None));
        let (ended, _, phases, waited) = run(Some(status("idle", None)), said);
        assert_eq!(ended, Ended::TooLong);
        assert_eq!(waited, MOST);
        assert_eq!(phases, ["fetching", ""]);
    }

    #[test]
    fn the_switch_is_only_on() {
        assert!(on(Some("on")));
        for value in [None, Some("off"), Some("ON"), Some(" on"), Some("1")] {
            assert!(!on(value), "{value:?}");
        }
    }
}
