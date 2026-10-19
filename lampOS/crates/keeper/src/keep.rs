//! What the keeper decides from the lamp's health: which part has been
//! unwell long enough to be restarted, and which will not stay up.
//!
//! Pure: it is handed the health, whether the voice is in a turn, the time,
//! and a [`Systemd`] to act through, so the tests play every case with a
//! stand-in and no lamp.
use crate::units::{Systemd, unit_of};
use lamp_msgs::{Health, Level};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

/// The key of a part's named state among the `values` of its `diagnostics`
/// report. No new message type: the report every part already sends
/// carries it, and a part that does not say one is read as saying nothing.
pub const STATE: &str = "state";

/// A part's named state, said as [`STATE`] in its report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartState {
    Starting,
    Ready,
    Paused,
    Stopping,
}

impl PartState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Paused => "paused",
            Self::Stopping => "stopping",
        }
    }
}

/// The part that is never restarted while someone speaks to the lamp.
pub const VOICE: &str = "voice";
/// The self-test's newest run, as `lamp-health` carries it (#812): a
/// result, not a part that runs. A failed run is never restarted, and a
/// run under way is never cut short.
pub const SELFTEST: &str = "selftest";
/// The keeper's own name on the bus. It does not restart itself.
pub const NAME: &str = "keeper";
/// The window in which restarts are counted.
pub const WINDOW: Duration = Duration::from_secs(600);

/// How the keeper acts.
#[derive(Clone, Copy, Debug)]
pub struct Rules {
    /// Restart at all. Off, it only says what it would have done.
    pub act: bool,
    /// How long a part is in error or stale before it is restarted.
    pub unwell_for: Duration,
    /// At most this many restarts of one part in [`WINDOW`].
    pub most: usize,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            act: false,
            unwell_for: Duration::from_secs(30),
            most: 3,
        }
    }
}

/// What the keeper did, or would have done, for a person to read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Done {
    /// Watching only: this part has been unwell long enough to restart.
    WouldRestart {
        part: String,
    },
    Restarted {
        part: String,
        times: usize,
    },
    /// It has been restarted the most times in the window: left alone.
    GaveUp {
        part: String,
        times: usize,
    },
    /// The voice is unwell, and is in a turn or not known to be out of one.
    HeldForVoice,
    Failed {
        part: String,
        why: String,
    },
}

impl std::fmt::Display for Done {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WouldRestart { part } => {
                write!(f, "{part} is unwell; it would be restarted with --act")
            }
            Self::Restarted { part, times } => {
                write!(f, "{part} restarted ({times} in ten minutes)")
            }
            Self::GaveUp { part, times } => write!(
                f,
                "{part} will not stay up: restarted {times} times in ten minutes, left alone"
            ),
            Self::HeldForVoice => f.write_str("the voice is unwell and in a turn: not restarted"),
            Self::Failed { part, why } => write!(f, "{part} could not be restarted: {why}"),
        }
    }
}

#[derive(Default)]
struct Kept {
    unwell_since: Option<Instant>,
    restarts: Vec<Instant>,
    /// What is said once in an unwell spell has been said.
    told: bool,
}

#[derive(Default)]
pub struct Keeper {
    rules: Rules,
    parts: BTreeMap<String, Kept>,
}

impl Keeper {
    pub fn new(rules: Rules) -> Self {
        Self {
            rules,
            parts: BTreeMap::new(),
        }
    }

    /// The parts restarted the most times and left alone.
    pub fn given_up(&self, now: Instant) -> Vec<&str> {
        self.parts
            .iter()
            .filter(|(_, kept)| kept.told && in_window(&kept.restarts, now) >= self.rules.most)
            .map(|(part, _)| part.as_str())
            .collect()
    }

    /// All restarts within the window.
    pub fn restarts(&self, now: Instant) -> usize {
        self.parts
            .values()
            .map(|kept| in_window(&kept.restarts, now))
            .sum()
    }

    /// Look at the lamp's health once. `voice_resting` is true only when the
    /// voice is known to be out of a turn.
    pub fn look(
        &mut self,
        health: &Health,
        voice_resting: bool,
        now: Instant,
        systemd: &mut dyn Systemd,
    ) -> Vec<Done> {
        let rules = self.rules;
        let mut done = Vec::new();
        for item in &health.items {
            // The keeper itself, and the self-test's result: neither is a
            // unit to restart (the fifth on #1243).
            if item.name == NAME || item.name == SELFTEST {
                continue;
            }
            let kept = self.parts.entry(item.name.clone()).or_default();
            if !matches!(item.level, Level::Error | Level::Stale) {
                *kept = Kept {
                    restarts: std::mem::take(&mut kept.restarts),
                    ..Kept::default()
                };
                continue;
            }
            let since = *kept.unwell_since.get_or_insert(now);
            if now.saturating_duration_since(since) < rules.unwell_for {
                continue;
            }
            kept.restarts
                .retain(|at| now.saturating_duration_since(*at) < WINDOW);
            let part = item.name.clone();
            let say_once = |kept: &mut Kept, what: Done, done: &mut Vec<Done>| {
                if !std::mem::replace(&mut kept.told, true) {
                    done.push(what);
                }
            };
            if !rules.act {
                say_once(kept, Done::WouldRestart { part }, &mut done);
                continue;
            }
            if kept.restarts.len() >= rules.most {
                let times = kept.restarts.len();
                say_once(kept, Done::GaveUp { part, times }, &mut done);
                continue;
            }
            if part == VOICE && !voice_resting {
                say_once(kept, Done::HeldForVoice, &mut done);
                continue;
            }
            // A unit that is not running was stopped by a person, or given
            // up on by systemd, or is no unit at all (the board): not ours.
            let unit = unit_of(&part);
            if !systemd.active(&unit) {
                continue;
            }
            // Another full spell of being unwell before the next try.
            kept.unwell_since = Some(now);
            kept.told = false;
            match systemd.command("restart", &unit) {
                Ok(()) => {
                    kept.restarts.push(now);
                    done.push(Done::Restarted {
                        part,
                        times: kept.restarts.len(),
                    });
                }
                Err(why) => done.push(Done::Failed { part, why }),
            }
        }
        done
    }
}

fn in_window(restarts: &[Instant], now: Instant) -> usize {
    restarts
        .iter()
        .filter(|at| now.saturating_duration_since(**at) < WINDOW)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::HealthItem;

    /// systemd as the tests play it: every unit running, and each command
    /// written down.
    #[derive(Default)]
    struct Stand {
        commands: Vec<String>,
        stopped: Vec<String>,
    }

    impl Systemd for Stand {
        fn active(&mut self, unit: &str) -> bool {
            !self.stopped.iter().any(|stopped| stopped == unit)
        }
        fn command(&mut self, verb: &str, unit: &str) -> Result<(), String> {
            self.commands.push(format!("{verb} {unit}"));
            Ok(())
        }
        fn show(&mut self, _: &[&str]) -> Result<String, String> {
            Ok(String::new())
        }
    }

    fn health(parts: &[(&str, Level)]) -> Health {
        Health {
            level: Level::Ok,
            message: String::new(),
            items: parts
                .iter()
                .map(|(name, level)| HealthItem {
                    name: (*name).into(),
                    level: *level,
                    message: String::new(),
                    age_ms: 0,
                })
                .collect(),
            missing: Vec::new(),
            quiet_ms: 0,
        }
    }

    fn acting() -> Keeper {
        Keeper::new(Rules {
            act: true,
            ..Rules::default()
        })
    }

    const S: Duration = Duration::from_secs(1);

    /// #812: a self-test that failed a part is in the lamp's health as an
    /// error for as long as no run passes. It is no part the keeper can
    /// restart, so it is passed over and never said.
    #[test]
    fn a_failed_selftest_is_never_restarted() {
        let (mut keeper, mut systemd, start) = (acting(), Stand::default(), Instant::now());
        let failed = health(&[(SELFTEST, Level::Error), ("eyes", Level::Ok)]);
        for at in [0, 30, 60, 600] {
            assert!(
                keeper
                    .look(&failed, true, start + at * S, &mut systemd)
                    .is_empty()
            );
        }
        assert!(systemd.commands.is_empty());
        let mut watching = Keeper::new(Rules::default());
        assert!(
            watching
                .look(&failed, true, start + 600 * S, &mut systemd)
                .is_empty()
        );
    }

    #[test]
    fn a_part_unwell_for_the_delay_is_restarted_and_not_before() {
        let (mut keeper, mut systemd, start) = (acting(), Stand::default(), Instant::now());
        // Hung: it went quiet and health calls it stale.
        let hung = health(&[("body", Level::Stale), ("eyes", Level::Ok)]);
        assert!(keeper.look(&hung, true, start, &mut systemd).is_empty());
        assert!(
            keeper
                .look(&hung, true, start + 29 * S, &mut systemd)
                .is_empty()
        );
        assert_eq!(
            keeper.look(&hung, true, start + 30 * S, &mut systemd),
            [Done::Restarted {
                part: "body".into(),
                times: 1
            }]
        );
        assert_eq!(systemd.commands, ["restart lamp-body.service"]);
        // It gets another full spell before the next try.
        assert!(
            keeper
                .look(&hung, true, start + 59 * S, &mut systemd)
                .is_empty()
        );
        // A part well again in between starts its spell anew.
        let well = health(&[("body", Level::Ok)]);
        keeper.look(&well, true, start + 50 * S, &mut systemd);
        let erring = health(&[("body", Level::Error)]);
        assert!(
            keeper
                .look(&erring, true, start + 70 * S, &mut systemd)
                .is_empty()
        );
        assert_eq!(
            keeper
                .look(&erring, true, start + 100 * S, &mut systemd)
                .len(),
            1
        );
    }

    #[test]
    fn one_that_keeps_failing_is_left_alone_and_reported_once() {
        let (mut keeper, mut systemd, start) = (acting(), Stand::default(), Instant::now());
        let failing = health(&[("eyes", Level::Error)]);
        let mut said = Vec::new();
        for second in 0..200 {
            said.extend(keeper.look(&failing, true, start + second * S, &mut systemd));
        }
        assert_eq!(
            said,
            [
                Done::Restarted {
                    part: "eyes".into(),
                    times: 1
                },
                Done::Restarted {
                    part: "eyes".into(),
                    times: 2
                },
                Done::Restarted {
                    part: "eyes".into(),
                    times: 3
                },
                Done::GaveUp {
                    part: "eyes".into(),
                    times: 3
                },
            ]
        );
        assert_eq!(systemd.commands.len(), 3);
        assert_eq!(keeper.given_up(start + 200 * S), ["eyes"]);
        // Ten minutes after the first, it may be tried again.
        let again = keeper.look(&failing, true, start + 631 * S, &mut systemd);
        assert_eq!(
            again,
            [Done::Restarted {
                part: "eyes".into(),
                times: 3
            }]
        );
    }

    #[test]
    fn nothing_is_done_without_act() {
        let (mut keeper, mut systemd, start) = (
            Keeper::new(Rules::default()),
            Stand::default(),
            Instant::now(),
        );
        let failing = health(&[("body", Level::Error)]);
        let mut said = Vec::new();
        for second in 0..100 {
            said.extend(keeper.look(&failing, true, start + second * S, &mut systemd));
        }
        assert_eq!(
            said,
            [Done::WouldRestart {
                part: "body".into()
            }]
        );
        assert!(systemd.commands.is_empty());
    }

    #[test]
    fn the_voice_is_never_restarted_in_a_turn() {
        let (mut keeper, mut systemd, start) = (acting(), Stand::default(), Instant::now());
        let failing = health(&[("voice", Level::Error)]);
        let mut said = Vec::new();
        for second in 0..100 {
            said.extend(keeper.look(&failing, false, start + second * S, &mut systemd));
        }
        assert_eq!(said, [Done::HeldForVoice]);
        assert!(systemd.commands.is_empty());
        // Out of the turn, it is.
        assert_eq!(
            keeper.look(&failing, true, start + 100 * S, &mut systemd),
            [Done::Restarted {
                part: "voice".into(),
                times: 1
            }]
        );
    }

    #[test]
    fn a_unit_not_running_and_the_keeper_itself_are_left_alone() {
        let (mut keeper, start) = (acting(), Instant::now());
        let mut systemd = Stand {
            stopped: vec!["lamp-board.service".into()],
            ..Stand::default()
        };
        let failing = health(&[("board", Level::Error), ("keeper", Level::Error)]);
        for second in 0..100 {
            assert!(
                keeper
                    .look(&failing, true, start + second * S, &mut systemd)
                    .is_empty()
            );
        }
        assert!(systemd.commands.is_empty());
    }
}
