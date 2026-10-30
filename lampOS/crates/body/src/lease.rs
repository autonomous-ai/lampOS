//! The body's lease (phase 0 of the lamp programming protocol, #3504,
//! agreed with Nine at 13:11 on 27 Oct): one holder, `lamp-control`, may
//! hold the body for a while, so that the lamp's own life stands back while
//! a program moves it. `lamp-control` keeps the protocol's leases, clients
//! and caps; the body knows only whether it is held, and why it stopped.
//!
//! - `{"hold":{"for_ms":N}}`, N from 1 to [`MOST_MS`]: taken, or renewed.
//! - `{"let_go":true}`: given back.
//!
//! Each is answered with one [`Said`], and a change while held is said the
//! same way, unasked:
//! - `{"lease":"held"}`;
//! - `{"lease":"suspended","why":"conversation"}`, while someone talks to
//!   the lamp, and `held` again after;
//! - `{"lease":"lost","why":"hand|slumped|private|paused|asleep|lapsed"}`: a hand on
//!   the head, the person's privacy, a pause or a sleep ends it, and so does
//!   a hold not renewed within its `for_ms`, so a holder that dies gives
//!   the lamp its own life back within 30 s;
//! - `{"lease":"free"}`, after `let_go`;
//! - `{"lease":"refused","why":...}`, for anything else.
//!
//! A hold asked while a hand, privacy, a pause or a sleep is in force is
//! answered `lost` with why, and nothing is held.
//!
//! While held, the holder may also say where the head looks ([`Looking`],
//! the protocol's `look`): `{"look":{"yaw":Y,"pitch":P}}` in degrees, yaw
//! right and pitch up positive, or `{"look":"ahead"}`. It is answered once,
//! when the head is there or when it can't be:
//! - `{"look":"there","yaw":Y,"pitch":P,"clamped":false}`, the angles kept
//!   inside this body's reach (`clamped` when they were cut to it);
//! - `{"look":"cut_short","why":...}`: the lease suspended or ended, another
//!   look asked, or the head not there within [`LONGEST_LOOK`];
//! - `{"look":"refused","why":...}`: not held, or not a look.
//!
//! The head keeps looking there while the lease is held, and comes back to
//! the pose's own direction when it ends.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// The longest one hold lasts: the holder renews what it keeps.
pub const MOST_MS: u64 = 30_000;

/// Every why a lease is `lost` with: a closed set, which lamp-control's
/// `LeaseWhy` knows in full (Fourteen on #3900). A new one is added there
/// in the same change, or the door would keep a lease the body has lost.
pub const LOST_WHYS: [&str; 6] = ["hand", "slumped", "private", "paused", "asleep", "lapsed"];

/// What the lamp is doing besides, as the body sees it this step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Around {
    /// Someone is talking to the lamp: a mood other than resting.
    pub conversation: bool,
    /// A hand holds a joint (`body/hand`).
    pub hand: bool,
    /// The person's privacy mute (`voice/mute`).
    pub private: bool,
    /// Paused by the keeper (`lamp/pause`).
    pub paused: bool,
    /// A hand's sleep (`voice/mute`, #1362).
    pub asleep: bool,
    /// Let go under load or by a hand (`--overload release`, `--limp`): its
    /// torque off, the body can't be moved (the reviewer-lead on #3791).
    pub slumped: bool,
}

impl Around {
    /// What ends a lease now, if anything: the person first.
    fn ends(&self) -> Option<&'static str> {
        [
            (self.hand, "hand"),
            (self.slumped, "slumped"),
            (self.private, "private"),
            (self.paused, "paused"),
            (self.asleep, "asleep"),
        ]
        .into_iter()
        .find_map(|(now, why)| now.then_some(why))
    }
}

/// The lease's state, as said to its holder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Held,
    Suspended,
    Lost,
    Free,
    Refused,
}

/// One line to the holder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Said {
    pub lease: State,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<&'static str>,
}

impl Said {
    const HELD: Said = Said {
        lease: State::Held,
        why: None,
    };
    const SUSPENDED: Said = Said {
        lease: State::Suspended,
        why: Some("conversation"),
    };
    const FREE: Said = Said {
        lease: State::Free,
        why: None,
    };

    pub fn lost(why: &'static str) -> Said {
        // Every why the body says comes through here: a seventh fails the
        // first test that says it (Fourteen on #3904).
        debug_assert!(LOST_WHYS.contains(&why), "{why} is not in LOST_WHYS");
        Said {
            lease: State::Lost,
            why: Some(why),
        }
    }

    pub fn refused(why: &'static str) -> Said {
        Said {
            lease: State::Refused,
            why: Some(why),
        }
    }

    /// As one line of JSON.
    pub fn line(&self) -> String {
        let mut line = serde_json::to_string(self).unwrap_or_default();
        line.push('\n');
        line
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Hold {
    for_ms: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Asked {
    hold: Option<Hold>,
    let_go: Option<bool>,
}

/// The body's one lease.
#[derive(Debug, Default)]
pub struct Lease {
    until: Option<Instant>,
    suspended: bool,
}

impl Lease {
    /// One line from the holder, `around` as it is `now`: its answer.
    pub fn ask(&mut self, line: &[u8], around: Around, now: Instant) -> Said {
        let Ok(asked) = serde_json::from_slice::<Asked>(line) else {
            return Said::refused("not a lease request");
        };
        match (asked.hold, asked.let_go) {
            (None, Some(true)) => {
                self.end();
                Said::FREE
            }
            (Some(Hold { for_ms }), None) if (1..=MOST_MS).contains(&for_ms) => {
                if let Some(why) = around.ends() {
                    self.end();
                    return Said::lost(why);
                }
                self.until = Some(now + Duration::from_millis(for_ms));
                self.suspended = around.conversation;
                if self.suspended {
                    Said::SUSPENDED
                } else {
                    Said::HELD
                }
            }
            (Some(_), None) => Said::refused("for_ms is 1 to 30000"),
            _ => Said::refused("one of hold or let_go"),
        }
    }

    /// What changed this step, if anything, to be said unasked.
    pub fn step(&mut self, around: Around, now: Instant) -> Option<Said> {
        let until = self.until?;
        if now >= until {
            self.end();
            return Some(Said::lost("lapsed"));
        }
        if let Some(why) = around.ends() {
            self.end();
            return Some(Said::lost(why));
        }
        if around.conversation != self.suspended {
            self.suspended = around.conversation;
            return Some(if self.suspended {
                Said::SUSPENDED
            } else {
                Said::HELD
            });
        }
        None
    }

    /// Whether the body is held now: the lamp's own life stands back.
    pub fn holds(&self, now: Instant) -> bool {
        self.until.is_some_and(|until| now < until) && !self.suspended
    }

    /// Whether a lease is taken, held or suspended, and not yet ended or
    /// lapsed by a step: what a run's end may still tell its holder.
    pub fn taken(&self) -> bool {
        self.until.is_some()
    }

    /// Given back, or its holder gone.
    pub fn end(&mut self) {
        self.until = None;
        self.suspended = false;
    }
}

/// The encoder's counts in one degree of a joint's turn.
pub const COUNTS_PER_DEGREE: f32 = 4_096.0 / 360.0;
/// How near its goal the head and base must be, in counts, and for how many
/// steps in a row with the goal still, for a look to be there.
pub const THERE_WITHIN: f32 = 6.0;
pub const THERE_STEPS: u32 = 10;
/// A look not there by then is cut short: something holds the head.
pub const LONGEST_LOOK: Duration = Duration::from_secs(5);

/// How far a look may take the head from the pose's own direction, in
/// counts: the base either way, and the head up and down. The body's own
/// bounds for every pose still hold beyond these.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reach {
    pub yaw: f32,
    pub up: f32,
    pub down: f32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LookLine {
    look: LookValue,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum LookValue {
    Word(String),
    Angles(Angles),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Angles {
    yaw: f32,
    pitch: f32,
    /// `"home"`: the angles are from where the head and base were found
    /// at start (`get` `body`'s frame, a computer's `joints`), not from the
    /// pose's own direction (`look`'s, the gaze's). The body takes the
    /// pose's offset off (`body::aim_from_home`).
    #[serde(default)]
    from: Option<String>,
}

/// A look asked and not yet answered.
#[derive(Clone, Copy, Debug)]
struct Owed {
    since: Instant,
    degrees: [f32; 2],
    clamped: bool,
    settled: u32,
    goals: Option<[u16; 3]>,
    /// The look as the body last applied it, after the joints' own bounds
    /// (counts, yaw and pitch): what `there` says, when the body said it.
    applied: Option<[f32; 2]>,
}

/// The holder's look: where the head is asked to look, and the answer
/// still owed for it.
#[derive(Debug, Default)]
pub struct Looking {
    aim: Option<[f32; 2]>,
    /// Whether `aim` is counted from the found home, not the pose.
    from_home: bool,
    asked: Option<Owed>,
}

impl Looking {
    /// Whether a holder's line is a look, for [`Looking::ask`], or else the
    /// lease's.
    pub fn is_look(line: &[u8]) -> bool {
        serde_json::from_slice::<serde_json::Value>(line)
            .is_ok_and(|value| value.get("look").is_some())
    }

    /// A look from the holder ([`Looking::is_look`]): its answer now, if one
    /// is due now (a refusal, or the look it replaces cut short).
    pub fn ask(&mut self, line: &[u8], held: bool, reach: Reach, now: Instant) -> Option<String> {
        let (at, from_home) = match serde_json::from_slice::<LookLine>(line) {
            Ok(LookLine {
                look: LookValue::Angles(Angles { yaw, pitch, from }),
            }) if yaw.is_finite()
                && pitch.is_finite()
                && from.as_deref().is_none_or(|from| from == "home") =>
            {
                ([yaw, pitch], from.is_some())
            }
            Ok(LookLine {
                look: LookValue::Word(word),
            }) if word == "ahead" => ([0.0, 0.0], false),
            _ => {
                return Some(refused(
                    "a look is {\"yaw\":Y,\"pitch\":P} in degrees, or \"ahead\"",
                ));
            }
        };
        if !held {
            return Some(refused("the body is not held"));
        }
        let before = self.asked.take().map(|_| cut_short("another look"));
        // Degrees to counts: the base turns right with more counts (every
        // measured `--gaze` yaw is positive: more counts bring a face from the
        // picture's right edge to its middle; on lamp-75c9 on 28 Oct fewer
        // turned it away from a hand on the right), and the head looks up
        // with fewer (a lift is negative, as every clip's).
        let yaw = (at[0] * COUNTS_PER_DEGREE).clamp(-reach.yaw, reach.yaw);
        let pitch = (-at[1] * COUNTS_PER_DEGREE).clamp(-reach.up, reach.down);
        let degrees = [yaw / COUNTS_PER_DEGREE, -pitch / COUNTS_PER_DEGREE];
        let clamped = (degrees[0] - at[0]).abs() > 0.05 || (degrees[1] - at[1]).abs() > 0.05;
        self.aim = Some([yaw, pitch]);
        self.from_home = from_home;
        self.asked = Some(Owed {
            since: now,
            degrees,
            clamped,
            settled: 0,
            goals: None,
            applied: None,
        });
        before
    }

    /// Where the head is asked to look, as the body's `Asked::aim` takes it.
    pub fn aim(&self) -> Option<[f32; 2]> {
        self.aim
    }

    /// Whether [`Self::aim`] is counted from the found home (`"from":"home"`).
    pub fn from_home(&self) -> bool {
        self.from_home
    }

    /// The body's own word on the look it applies this step, after every
    /// bound of its joints (the pitch window, the base's reach): `there`
    /// then says those angles, and `clamped` if any bound cut the look.
    pub fn applied(&mut self, aimed: Option<[f32; 2]>) {
        if let (Some(owed), Some(aimed)) = (self.asked.as_mut(), aimed) {
            owed.applied = Some(aimed);
        }
    }

    /// The look owed an answer: its angles as kept (degrees, cut to the
    /// reach) and whether they were cut, for the journal.
    pub fn asked(&self) -> Option<([f32; 2], bool)> {
        self.asked.as_ref().map(|owed| (owed.degrees, owed.clamped))
    }

    /// The body's last step (its goals and positions for pitch, roll and
    /// yaw), and whether the body is held now, or why not: the answer owed,
    /// once it is due.
    pub fn step(
        &mut self,
        held: Result<(), &'static str>,
        seen: Option<([u16; 3], [u16; 3])>,
        now: Instant,
    ) -> Option<String> {
        if let Err(why) = held {
            if why != "conversation" {
                self.aim = None;
            }
            return self.asked.take().map(|_| cut_short(why));
        }
        let asked = self.asked.as_mut()?;
        if let Some((goals, positions)) = seen {
            let near = [0, 2].into_iter().all(|joint| {
                (f32::from(goals[joint]) - f32::from(positions[joint])).abs() <= THERE_WITHIN
            });
            let still = asked.goals.is_some_and(|before| {
                [0, 2]
                    .into_iter()
                    .all(|joint| before[joint].abs_diff(goals[joint]) <= 1)
            });
            asked.goals = Some(goals);
            asked.settled = if near && still { asked.settled + 1 } else { 0 };
            if asked.settled >= THERE_STEPS {
                let asked = self.asked.take()?;
                let aim = self.aim.unwrap_or_default();
                return Some(match asked.applied {
                    // As the body applied it: its own bounds may cut more
                    // than the reach did (lamp-a0ae on 29 Oct: a pitch of 5
                    // up kept 4, at its pitch window, said 5 and unclamped).
                    Some(applied) => there(
                        [
                            applied[0] / COUNTS_PER_DEGREE,
                            -applied[1] / COUNTS_PER_DEGREE,
                        ],
                        asked.clamped
                            || (applied[0] - aim[0]).abs() > 0.5
                            || (applied[1] - aim[1]).abs() > 0.5,
                    ),
                    None => there(asked.degrees, asked.clamped),
                });
            }
        }
        if now.duration_since(asked.since) >= LONGEST_LOOK {
            self.asked = None;
            return Some(cut_short("not there in time: something holds the head"));
        }
        None
    }

    /// The lease ended (let go, lapsed, its holder gone): the head comes
    /// back to the pose's own direction, and a look owed is cut short.
    pub fn end(&mut self, why: &'static str) -> Option<String> {
        self.aim = None;
        self.asked.take().map(|_| cut_short(why))
    }
}

fn line(value: serde_json::Value) -> String {
    let mut line = value.to_string();
    line.push('\n');
    line
}

fn there(degrees: [f32; 2], clamped: bool) -> String {
    let round = |degrees: f32| (f64::from(degrees) * 10.0).round() / 10.0;
    line(serde_json::json!({
        "look": "there",
        "yaw": round(degrees[0]),
        "pitch": round(degrees[1]),
        "clamped": clamped,
    }))
}

fn cut_short(why: &str) -> String {
    line(serde_json::json!({"look": "cut_short", "why": why}))
}

fn refused(why: &str) -> String {
    line(serde_json::json!({"look": "refused", "why": why}))
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_look_from_home_is_taken_and_any_other_from_is_refused() {
        let now = Instant::now();
        let mut looking = Looking::default();
        assert!(
            looking
                .ask(
                    br#"{"look":{"yaw":5,"pitch":3,"from":"home"}}"#,
                    true,
                    REACH,
                    now
                )
                .is_none()
        );
        assert!(looking.from_home());
        assert!(looking.aim().is_some());
        let refused = looking
            .ask(
                br#"{"look":{"yaw":5,"pitch":3,"from":"pose"}}"#,
                true,
                REACH,
                now,
            )
            .unwrap();
        assert!(refused.contains("refused"), "{refused}");
        // A plain look is the gaze's frame again.
        looking.ask(br#"{"look":{"yaw":5,"pitch":3}}"#, true, REACH, now);
        assert!(!looking.from_home());
    }

    use super::*;

    const QUIET: Around = Around {
        conversation: false,
        hand: false,
        private: false,
        paused: false,
        asleep: false,
        slumped: false,
    };

    fn said(text: &str) -> String {
        text.to_owned() + "\n"
    }

    const REACH: Reach = Reach {
        yaw: 480.0,
        up: 150.0,
        down: 15.0,
    };

    fn value(line: &str) -> serde_json::Value {
        serde_json::from_str(line).unwrap()
    }

    #[test]
    fn a_look_is_aimed_in_counts_inside_the_reach_and_said_once_there() {
        let mut looking = Looking::default();
        let now = Instant::now();
        assert!(
            !Looking::is_look(br#"{"hold":{"for_ms":5000}}"#),
            "the lease's"
        );
        assert!(Looking::is_look(br#"{"look":"ahead"}"#));
        assert_eq!(
            looking.ask(br#"{"look":{"yaw":10,"pitch":5}}"#, true, REACH, now),
            None
        );
        let aim = looking.aim().unwrap();
        assert!(
            (aim[0] - 113.8).abs() < 0.1 && (aim[1] + 56.9).abs() < 0.1,
            "{aim:?}"
        );
        // Moving: not there yet.
        assert_eq!(
            looking.step(Ok(()), Some(([1900, 2060, 1900], [1950, 2060, 1960])), now),
            None
        );
        let mut said = None;
        for step in 0..=THERE_STEPS {
            said = looking.step(Ok(()), Some(([1734, 2060, 1898], [1736, 2060, 1895])), now);
            if step < THERE_STEPS - 1 {
                assert_eq!(said, None, "step {step}");
            }
            if said.is_some() {
                break;
            }
        }
        assert_eq!(
            value(&said.unwrap()),
            serde_json::json!({"look": "there", "yaw": 10.0, "pitch": 5.0, "clamped": false})
        );
        assert_eq!(looking.step(Ok(()), None, now), None, "said once");
        assert!(looking.aim().is_some(), "it keeps looking there");
        // Past the reach: cut to it, and said so. Down is 15 counts here.
        looking.ask(br#"{"look":{"yaw":-90,"pitch":-20}}"#, true, REACH, now);
        assert_eq!(looking.aim(), Some([-480.0, 15.0]));
        let mut said = None;
        for _ in 0..=THERE_STEPS {
            said =
                said.or(looking.step(Ok(()), Some(([1806, 2060, 2492], [1806, 2060, 2492])), now));
        }
        let said = value(&said.unwrap());
        assert_eq!(said["clamped"], true);
        assert_eq!(said["yaw"], -42.2);
        assert_eq!(said["pitch"], -1.3);
        // Ahead: back to the pose's own direction.
        looking.ask(br#"{"look":"ahead"}"#, true, REACH, now);
        assert_eq!(looking.aim(), Some([0.0, 0.0]));
        // The lease ends: the aim goes, the head comes back.
        assert_eq!(value(&looking.end("let go").unwrap())["look"], "cut_short");
        assert_eq!(looking.aim(), None);
    }

    /// `there` says the look as the body applied it, and `clamped` when a
    /// joint's own bound cut more than the reach did.
    #[test]
    fn there_says_the_look_as_the_body_applied_it() {
        let mut looking = Looking::default();
        let now = Instant::now();
        let settle = |looking: &mut Looking| {
            let mut said = None;
            for _ in 0..=THERE_STEPS {
                said = said.or(looking.step(
                    Ok(()),
                    Some(([1734, 2060, 2015], [1734, 2060, 2015])),
                    now,
                ));
            }
            value(&said.unwrap())
        };
        // Asked 5 up and 0 across; the body kept 46 counts up (4.0 degrees).
        looking.ask(br#"{"look":{"yaw":0,"pitch":5}}"#, true, REACH, now);
        looking.applied(Some([0.0, -46.0]));
        let said = settle(&mut looking);
        assert_eq!(said["clamped"], true);
        assert_eq!(said["pitch"], 4.0);
        assert_eq!(said["yaw"], 0.0);
        // Applied whole: said as asked, not clamped.
        looking.ask(br#"{"look":{"yaw":10,"pitch":0}}"#, true, REACH, now);
        looking.applied(Some([10.0 * COUNTS_PER_DEGREE, 0.0]));
        let said = settle(&mut looking);
        assert_eq!(
            (said["yaw"].as_f64(), said["clamped"].as_bool()),
            (Some(10.0), Some(false))
        );
        // Before the body says how it applied it: as the reach kept it.
        looking.ask(br#"{"look":{"yaw":-90,"pitch":0}}"#, true, REACH, now);
        let said = settle(&mut looking);
        assert_eq!(
            (said["yaw"].as_f64(), said["clamped"].as_bool()),
            (Some(-42.2), Some(true))
        );
    }

    /// What the journal says of a look comes from `asked`: the angles kept
    /// and whether they were cut, until the look is answered.
    #[test]
    fn a_look_owed_is_told_with_its_kept_angles_until_it_is_answered() {
        let mut looking = Looking::default();
        let now = Instant::now();
        assert_eq!(looking.asked(), None);
        looking.ask(br#"{"look":{"yaw":15,"pitch":5}}"#, true, REACH, now);
        let (degrees, clamped) = looking.asked().unwrap();
        assert!((degrees[0] - 15.0).abs() < 0.05 && (degrees[1] - 5.0).abs() < 0.05 && !clamped);
        // Past the reach (yaw 480 counts, down 15): kept as cut, and said so.
        looking.ask(br#"{"look":{"yaw":-90,"pitch":-20}}"#, true, REACH, now);
        let (degrees, clamped) = looking.asked().unwrap();
        assert!((degrees[0] + 42.2).abs() < 0.1 && (degrees[1] + 1.3).abs() < 0.1 && clamped);
        // A refused look leaves the one owed as it was.
        looking.ask(br#"{"look":"sideways"}"#, true, REACH, now);
        assert!(looking.asked().unwrap().1);
        looking.end("let go");
        assert_eq!(looking.asked(), None);
    }

    #[test]
    fn a_look_is_cut_short_by_the_lease_another_look_or_a_hold_on_the_head() {
        let now = Instant::now();
        let mut looking = Looking::default();
        looking.ask(br#"{"look":{"yaw":0,"pitch":10}}"#, true, REACH, now);
        let replaced = looking
            .ask(br#"{"look":"ahead"}"#, true, REACH, now)
            .unwrap();
        assert_eq!(
            value(&replaced),
            serde_json::json!({"look": "cut_short", "why": "another look"})
        );
        // A conversation: cut short, and the aim kept for when it ends.
        assert_eq!(
            value(&looking.step(Err("conversation"), None, now).unwrap())["why"],
            "conversation"
        );
        assert_eq!(looking.aim(), Some([0.0, 0.0]));
        // Lost: the aim goes too.
        looking.ask(br#"{"look":"ahead"}"#, true, REACH, now);
        assert_eq!(
            value(&looking.step(Err("hand"), None, now).unwrap())["why"],
            "hand"
        );
        assert_eq!(looking.aim(), None);
        // A head that never gets there: cut short at the longest.
        looking.ask(br#"{"look":{"yaw":0,"pitch":30}}"#, true, REACH, now);
        let stuck = ([1641, 2060, 2012], [1790, 2060, 2012]);
        assert_eq!(
            looking.step(Ok(()), Some(stuck), now + Duration::from_secs(1)),
            None
        );
        let late = looking
            .step(Ok(()), Some(stuck), now + LONGEST_LOOK)
            .unwrap();
        assert_eq!(value(&late)["look"], "cut_short");
        // Not held, or not a look: refused, and nothing aimed.
        let mut looking = Looking::default();
        for (line, held) in [
            (&br#"{"look":"ahead"}"#[..], false),
            (br#"{"look":"face"}"#, true),
            (br#"{"look":{"yaw":1}}"#, true),
            (br#"{"look":{"yaw":1,"pitch":2,"speed":3}}"#, true),
        ] {
            let said = value(&looking.ask(line, held, REACH, now).unwrap());
            assert_eq!(said["look"], "refused", "{}", String::from_utf8_lossy(line));
            assert_eq!(looking.aim(), None);
        }
    }

    #[test]
    fn a_hold_is_held_renewed_and_let_go() {
        let mut lease = Lease::default();
        let now = Instant::now();
        assert!(!lease.holds(now));
        let held = lease.ask(br#"{"hold":{"for_ms":5000}}"#, QUIET, now);
        assert_eq!(held.line(), said(r#"{"lease":"held"}"#));
        assert!(lease.holds(now + Duration::from_millis(4999)));
        // Renewed before it lapses, it holds on.
        let later = now + Duration::from_millis(4000);
        lease.ask(br#"{"hold":{"for_ms":5000}}"#, QUIET, later);
        assert!(lease.holds(now + Duration::from_millis(8999)));
        assert_eq!(lease.step(QUIET, now + Duration::from_millis(8999)), None);
        let free = lease.ask(br#"{"let_go":true}"#, QUIET, later);
        assert_eq!(free.line(), said(r#"{"lease":"free"}"#));
        assert!(!lease.holds(later));
        assert_eq!(lease.step(QUIET, later), None, "nothing to say once free");
    }

    #[test]
    fn a_hold_not_renewed_lapses_by_itself() {
        let mut lease = Lease::default();
        let now = Instant::now();
        lease.ask(br#"{"hold":{"for_ms":30000}}"#, QUIET, now);
        let lapsed = lease.step(QUIET, now + Duration::from_secs(30)).unwrap();
        assert_eq!(lapsed.line(), said(r#"{"lease":"lost","why":"lapsed"}"#));
        assert!(!lease.holds(now + Duration::from_secs(30)));
        assert_eq!(lease.step(QUIET, now + Duration::from_secs(31)), None);
    }

    #[test]
    fn a_conversation_suspends_it_and_its_end_gives_it_back() {
        let mut lease = Lease::default();
        let now = Instant::now();
        lease.ask(br#"{"hold":{"for_ms":10000}}"#, QUIET, now);
        let talking = Around {
            conversation: true,
            ..QUIET
        };
        let suspended = lease.step(talking, now).unwrap();
        assert_eq!(
            suspended.line(),
            said(r#"{"lease":"suspended","why":"conversation"}"#)
        );
        assert!(!lease.holds(now), "the conversation has the body");
        assert_eq!(lease.step(talking, now), None, "said once");
        assert_eq!(lease.step(QUIET, now), Some(Said::HELD));
        assert!(lease.holds(now));
        // Asked during a conversation, it is taken suspended.
        let mut lease = Lease::default();
        let asked = lease.ask(br#"{"hold":{"for_ms":10000}}"#, talking, now);
        assert_eq!(asked, Said::SUSPENDED);
        assert!(!lease.holds(now));
    }

    /// The whys are a closed set (Fourteen on #3900): every one the body
    /// can say is in `LOST_WHYS`, and there are six, so a seventh can't
    /// land without that list, and lamp-control's `LeaseWhy`, changing.
    /// A why outside the six is refused where every `lost` is made.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "cold is not in LOST_WHYS")]
    fn a_seventh_why_is_refused_where_it_is_said() {
        Said::lost("cold");
    }

    #[test]
    fn every_lost_why_is_one_of_the_closed_six() {
        let flags: [fn(&mut Around); 5] = [
            |around| around.hand = true,
            |around| around.slumped = true,
            |around| around.private = true,
            |around| around.paused = true,
            |around| around.asleep = true,
        ];
        let mut said: Vec<&str> = flags
            .iter()
            .map(|set| {
                let mut around = QUIET;
                set(&mut around);
                around.ends().unwrap()
            })
            .collect();
        // A hold not renewed in time.
        said.push("lapsed");
        assert_eq!(said, LOST_WHYS);
    }

    #[test]
    fn a_hand_privacy_a_pause_or_a_sleep_ends_it() {
        let now = Instant::now();
        for (around, why) in [
            (
                Around {
                    hand: true,
                    ..QUIET
                },
                "hand",
            ),
            (
                Around {
                    private: true,
                    ..QUIET
                },
                "private",
            ),
            (
                Around {
                    paused: true,
                    ..QUIET
                },
                "paused",
            ),
            (
                Around {
                    asleep: true,
                    ..QUIET
                },
                "asleep",
            ),
            (
                Around {
                    slumped: true,
                    ..QUIET
                },
                "slumped",
            ),
            (
                Around {
                    hand: true,
                    paused: true,
                    conversation: true,
                    ..QUIET
                },
                "hand",
            ),
        ] {
            let mut lease = Lease::default();
            lease.ask(br#"{"hold":{"for_ms":10000}}"#, QUIET, now);
            assert_eq!(lease.step(around, now), Some(Said::lost(why)), "{why}");
            assert!(!lease.holds(now));
            // It is not taken back when that ends: the holder asks again.
            assert_eq!(lease.step(QUIET, now), None);
            assert!(!lease.holds(now));
            // Asked while it lasts, it is not held.
            let mut lease = Lease::default();
            let asked = lease.ask(br#"{"hold":{"for_ms":10000}}"#, around, now);
            assert_eq!(asked, Said::lost(why), "{why}");
            assert!(!lease.holds(now));
        }
    }

    #[test]
    fn anything_else_is_refused_with_why() {
        let mut lease = Lease::default();
        let now = Instant::now();
        for (line, why) in [
            (&br#"{"hold":{"for_ms":0}}"#[..], "for_ms is 1 to 30000"),
            (br#"{"hold":{"for_ms":30001}}"#, "for_ms is 1 to 30000"),
            (
                br#"{"hold":{"for_ms":5000},"let_go":true}"#,
                "one of hold or let_go",
            ),
            (br#"{"let_go":false}"#, "one of hold or let_go"),
            (br#"{}"#, "one of hold or let_go"),
            (
                br#"{"hold":{"for_ms":5000,"priority":"person"}}"#,
                "not a lease request",
            ),
            (br#"{"take":true}"#, "not a lease request"),
            (b"hold", "not a lease request"),
        ] {
            assert_eq!(lease.ask(line, QUIET, now), Said::refused(why), "{line:?}");
            assert!(!lease.holds(now));
        }
        assert_eq!(
            Said::refused("not a lease request").line(),
            said(r#"{"lease":"refused","why":"not a lease request"}"#)
        );
    }
}
