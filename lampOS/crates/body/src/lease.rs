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
//! - `{"lease":"held","tags":true}` (`tags`: a look or pose line may carry
//!   `"tag":N`, said back in every answer to it);
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
//!
//! And where the head's roll and the arm are ([`Posing`], the protocol's
//! `joints` past yaw and pitch): `{"pose":{"roll":R,"shoulder":S,"elbow":E}}`
//! in degrees, each optional but one, as `get('body')` counts them: the roll
//! from the found home, the shoulder and the elbow from the folded arm, each
//! rising with counts. The body takes it only while it stands still at its
//! full height (a lamp given `--stand`), the arm inside its window between
//! the fold and the stand and V1's anti-tip rule cutting any step. It is
//! answered once, as a look is: `{"pose":"there","roll":…,"shoulder":…,
//! "elbow":…,"clamped":…}`, `{"pose":"cut_short","why":…}` or
//! `{"pose":"refused","why":…}`. The pose is kept while held and let go,
//! back to the stand, when the lease ends.
//!
//! And may let the base go limp ([`Limping`], for recording a move by
//! hand): `{"limp":true}` turns base yaw's torque off while held, and any
//! end of the hold, or `{"limp":false}`, takes it back where it lies.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// How far, in counts, a joint at rest may stop short of its goal and still
/// count as resting on it. Here, not in `body`, so the crate builds off
/// Linux, where `body` is not compiled.
pub const REST_SLACK: u16 = 24;

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
    /// `true` with each hold: this body takes a `tag` on a holder's look
    /// and pose lines and says it back in every answer to that line
    /// ([`with_tag`]), so lamp-control tags its lines only for a body that
    /// says so (an older body answers in order, one line at a time).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<bool>,
}

impl Said {
    const HELD: Said = Said {
        lease: State::Held,
        why: None,
        tags: Some(true),
    };
    const SUSPENDED: Said = Said {
        lease: State::Suspended,
        why: Some("conversation"),
        tags: Some(true),
    };
    const FREE: Said = Said {
        lease: State::Free,
        why: None,
        tags: None,
    };

    pub fn lost(why: &'static str) -> Said {
        // Every why the body says comes through here: a seventh fails the
        // first test that says it (Fourteen on #3904).
        debug_assert!(LOST_WHYS.contains(&why), "{why} is not in LOST_WHYS");
        Said {
            lease: State::Lost,
            why: Some(why),
            tags: None,
        }
    }

    pub fn refused(why: &'static str) -> Said {
        Said {
            lease: State::Refused,
            why: Some(why),
            tags: None,
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
/// A look stopped short of its goal, though inside the slack a rest is let
/// stop in ([`REST_SLACK`]), is there once still this many steps: a second,
/// as long as the body waits before it counts a rest (`REST_STEPS`).
pub const REST_STOPPED_STEPS: u32 = 50;

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
    /// lamp-control's request, echoed in every answer to this line.
    #[serde(default)]
    #[allow(dead_code)]
    tag: Option<u64>,
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
    /// The joints' positions at the last step, to tell when they stopped.
    positions: Option<[u16; 3]>,
    /// How far the head's pitch and the base's yaw were from where the look
    /// drove them at the last step, in counts: said when it is cut short.
    short: [f32; 2],
    /// The look as the body last applied it, after the joints' own bounds
    /// (counts, yaw and pitch): what `there` says, when the body said it.
    applied: Option<[f32; 2]>,
    /// The line's `tag`, echoed in its answer.
    tag: Option<u64>,
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
        let tag = tag_of(line);
        let (at, from_home) = match serde_json::from_slice::<LookLine>(line) {
            Ok(LookLine {
                look: LookValue::Angles(Angles { yaw, pitch, from }),
                ..
            }) if yaw.is_finite()
                && pitch.is_finite()
                && from.as_deref().is_none_or(|from| from == "home") =>
            {
                ([yaw, pitch], from.is_some())
            }
            Ok(LookLine {
                look: LookValue::Word(word),
                ..
            }) if word == "ahead" => ([0.0, 0.0], false),
            _ => {
                return Some(with_tag(
                    refused("a look is {\"yaw\":Y,\"pitch\":P} in degrees, or \"ahead\""),
                    tag,
                ));
            }
        };
        if !held {
            return Some(with_tag(refused("the body is not held"), tag));
        }
        let before = self
            .asked
            .take()
            .map(|owed| with_tag(cut_short("another look"), owed.tag));
        // Degrees to counts: the base turns right with more counts (every
        // measured `--gaze` yaw is positive: more counts bring a face from the
        // picture's right edge to its middle; on lamp-75c9 on 28 Oct fewer
        // turned it away from a hand on the right), and the head looks up
        // with fewer (a lift is negative, as every clip's).
        // A look from home is cut by the body, after it takes the pose's
        // offset off, by the bounds every pose keeps (measured from the
        // pose): cut here to the reach, in the home frame, a standing lamp's
        // look came out at the edge whatever was asked (Fifth on #4580).
        // Here it is only kept to angles a lamp has.
        let (yaw, pitch) = if from_home {
            (
                (at[0] * COUNTS_PER_DEGREE).clamp(-2_048.0, 2_048.0),
                (-at[1] * COUNTS_PER_DEGREE).clamp(-1_024.0, 1_024.0),
            )
        } else {
            (
                (at[0] * COUNTS_PER_DEGREE).clamp(-reach.yaw, reach.yaw),
                (-at[1] * COUNTS_PER_DEGREE).clamp(-reach.up, reach.down),
            )
        };
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
            positions: None,
            short: [0.0; 2],
            applied: None,
            tag,
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
            return self
                .asked
                .take()
                .map(|owed| with_tag(cut_short(why), owed.tag));
        }
        let asked = self.asked.as_mut()?;
        if let Some((goals, positions)) = seen {
            let still = |before: Option<[u16; 3]>, now: [u16; 3]| {
                before.is_some_and(|before| {
                    [0, 2]
                        .into_iter()
                        .all(|joint| before[joint].abs_diff(now[joint]) <= 1)
                })
            };
            let goals_still = still(asked.goals, goals);
            let stopped = still(asked.positions, positions);
            asked.goals = Some(goals);
            asked.positions = Some(positions);
            // How far each of the head's pitch and the base's yaw stopped
            // from where the look drove it, in counts.
            let short = [0, 2].map(|joint| f32::from(positions[joint]) - f32::from(goals[joint]));
            let near = short.iter().all(|short| short.abs() <= THERE_WITHIN);
            // There, or stopped where a rest is let stop (`REST_SLACK`): a
            // resting head stops short against its own friction or weight
            // (lamp-75c9's pitch, 17 counts at load 80), and is not pushed on.
            let rested = short
                .iter()
                .all(|short| short.abs() <= f32::from(REST_SLACK));
            asked.settled = if goals_still && stopped {
                asked.settled + 1
            } else {
                0
            };
            asked.short = short;
            // Near, the look is there once still for `THERE_STEPS`; stopped
            // short inside the slack, once still for as long as a rest takes
            // to count (`REST_STOPPED_STEPS`), not before its servo starts.
            let due = if near {
                THERE_STEPS
            } else {
                REST_STOPPED_STEPS
            };
            if rested && asked.settled >= due {
                let asked = self.asked.take()?;
                // Where the joints stopped, in the look's own frame: the
                // look as applied, and how short each joint stopped of it.
                let aim = self.aim.unwrap_or_default();
                let (applied, cut) = match asked.applied {
                    // As the body applied it: its own bounds may cut more
                    // than the reach did (lamp-a0ae on 29 Oct: a pitch of 5
                    // up kept 4, at its pitch window, said 5 and unclamped).
                    Some(applied) => (
                        applied,
                        (applied[0] - aim[0]).abs() > 0.5 || (applied[1] - aim[1]).abs() > 0.5,
                    ),
                    None => (
                        [
                            asked.degrees[0] * COUNTS_PER_DEGREE,
                            -asked.degrees[1] * COUNTS_PER_DEGREE,
                        ],
                        false,
                    ),
                };
                // Near, as applied (the look's own angles, to a servo's
                // few counts); stopped further, where the joints are.
                let off = if near { [0.0; 2] } else { short };
                let reached = [
                    (applied[0] + off[1]) / COUNTS_PER_DEGREE,
                    -(applied[1] + off[0]) / COUNTS_PER_DEGREE,
                ];
                let stopped_off = off
                    .iter()
                    .any(|short| short.abs() > 0.5 * COUNTS_PER_DEGREE);
                return Some(with_tag(
                    there(reached, asked.clamped || cut || stopped_off),
                    asked.tag,
                ));
            }
        }
        if now.duration_since(asked.since) >= LONGEST_LOOK {
            let short = asked.short;
            let tag = asked.tag;
            self.asked = None;
            let worst = short
                .iter()
                .fold(0.0_f32, |worst, short| worst.max(short.abs()));
            return Some(with_tag(
                cut_short(&if worst > f32::from(REST_SLACK) {
                    format!(
                        "not there in time: stopped {worst:.0} counts short of the look; something holds the head or base"
                    )
                } else {
                    "not there in time: still moving".to_owned()
                }),
                tag,
            ));
        }
        None
    }

    /// The lease ended (let go, lapsed, its holder gone): the head comes
    /// back to the pose's own direction, and a look owed is cut short.
    pub fn end(&mut self, why: &'static str) -> Option<String> {
        self.aim = None;
        self.asked
            .take()
            .map(|owed| with_tag(cut_short(why), owed.tag))
    }
}

/// A pose not there by then is cut short: something holds the arm.
pub const LONGEST_POSE: Duration = Duration::from_secs(6);
/// How far short of its goal (counts) a joint that has stopped may rest and
/// still have arrived: a standing arm sags under its own load (lamp-75c9 at
/// its stand, 2 Nov: 19 and 35 counts short at loads 88 and 152). A joint
/// stopped farther short is held by something, and the pose stays owed.
pub const POSE_SAG_WITHIN: f32 = 50.0;
/// How near (degrees) the pose the body applies must be to the asked one,
/// for `there` to say it as asked rather than `clamped`.
const POSE_KEPT_WITHIN: f32 = 0.5;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PoseLine {
    pose: PoseJoints,
    /// lamp-control's request, echoed in every answer to this line.
    #[serde(default)]
    #[allow(dead_code)]
    tag: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct PoseJoints {
    roll: Option<f32>,
    shoulder: Option<f32>,
    elbow: Option<f32>,
}

/// A pose asked and not yet answered.
#[derive(Clone, Copy, Debug)]
struct PoseOwed {
    since: Instant,
    settled: u32,
    arm_goals: Option<[u16; 2]>,
    /// The roll's and the arm's positions the step before.
    positions: Option<[u16; 3]>,
    /// The line's `tag`, echoed in its answer.
    tag: Option<u64>,
}

/// The holder's pose of the head's roll and the arm ([`Posing::ask`]), in
/// degrees as `get('body')` says them, and the answer still owed for it.
#[derive(Debug, Default)]
pub struct Posing {
    pose: Option<[f32; 3]>,
    asked: Option<PoseOwed>,
}

/// What the body says of a pose each step: the roll, shoulder and elbow
/// it has now (degrees, as `get('body')` counts them), or why it takes no
/// pose now, and its arm's goals and positions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoseSeen {
    pub posed: Result<[f32; 3], &'static str>,
    /// The roll, shoulder and elbow their positions say (degrees, as
    /// `posed`): what `there` reports, sag and all.
    pub reached: [f32; 3],
    pub roll: (u16, u16),
    pub arm_goals: [u16; 2],
    pub arm_positions: [u16; 2],
}

impl Posing {
    /// Whether a holder's line is a pose, for [`Posing::ask`].
    pub fn is_pose(line: &[u8]) -> bool {
        serde_json::from_slice::<serde_json::Value>(line)
            .is_ok_and(|value| value.get("pose").is_some())
    }

    /// A pose from the holder: its answer now, if one is due now (a
    /// refusal, or the pose it replaces cut short). `now_posed` is the pose
    /// the body has now, which a joint not named keeps.
    pub fn ask(
        &mut self,
        line: &[u8],
        held: bool,
        now_posed: Result<[f32; 3], &'static str>,
        now: Instant,
    ) -> Option<String> {
        let tag = tag_of(line);
        let joints = match serde_json::from_slice::<PoseLine>(line) {
            Ok(PoseLine { pose, .. }) if pose != PoseJoints::default() => pose,
            _ => {
                return Some(with_tag(
                    pose_refused(
                        "a pose is {\"roll\":R,\"shoulder\":S,\"elbow\":E} in degrees, one at least",
                    ),
                    tag,
                ));
            }
        };
        let named = [joints.roll, joints.shoulder, joints.elbow];
        if named.iter().flatten().any(|degrees| !degrees.is_finite()) {
            return Some(with_tag(pose_refused("a pose's degrees are numbers"), tag));
        }
        if !held {
            return Some(with_tag(pose_refused("the body is not held"), tag));
        }
        let posed = match now_posed {
            Ok(posed) => posed,
            Err(why) => return Some(with_tag(pose_refused(why), tag)),
        };
        let before = self
            .asked
            .take()
            .map(|owed| with_tag(pose_cut_short("another pose"), owed.tag));
        let kept = self.pose.unwrap_or(posed);
        self.pose = Some(std::array::from_fn(|index| {
            named[index].unwrap_or(kept[index])
        }));
        self.asked = Some(PoseOwed {
            since: now,
            settled: 0,
            arm_goals: None,
            positions: None,
            tag,
        });
        before
    }

    /// The pose asked (roll, shoulder, elbow in degrees), as the body's
    /// `Asked::pose` takes it.
    pub fn pose(&self) -> Option<[f32; 3]> {
        self.pose
    }

    /// The body's last step and whether the body is held now, or why not:
    /// the answer owed, once it is due.
    pub fn step(
        &mut self,
        held: Result<(), &'static str>,
        seen: Option<PoseSeen>,
        now: Instant,
    ) -> Option<String> {
        if let Err(why) = held {
            if why != "conversation" {
                self.pose = None;
            }
            return self
                .asked
                .take()
                .map(|owed| with_tag(pose_cut_short(why), owed.tag));
        }
        let asked = self.asked.as_mut()?;
        let tag = asked.tag;
        if let Some(seen) = seen {
            let posed = match seen.posed {
                Ok(posed) => posed,
                Err(why) => {
                    self.asked = None;
                    return Some(with_tag(pose_cut_short(why), tag));
                }
            };
            // Each joint has arrived near its goal, or stopped where its
            // own load holds it, within `POSE_SAG_WITHIN`: a standing arm
            // rests short of its goals (lamp-75c9, 2 Nov: goals [2639, 2570],
            // positions [2620, 2535] at its stand, loads 88 and 152). One
            // stopped farther short is held, and the pose stays owed.
            let goals = [seen.roll.0, seen.arm_goals[0], seen.arm_goals[1]];
            let positions = [seen.roll.1, seen.arm_positions[0], seen.arm_positions[1]];
            let stopped = asked.positions;
            let arrived = (0..3).all(|index| {
                let short = (f32::from(goals[index]) - f32::from(positions[index])).abs();
                short <= THERE_WITHIN
                    || (short <= POSE_SAG_WITHIN
                        && stopped
                            .is_some_and(|before| before[index].abs_diff(positions[index]) <= 1))
            });
            let still = asked.arm_goals.is_some_and(|before| {
                (0..2).all(|index| before[index].abs_diff(seen.arm_goals[index]) <= 1)
            });
            asked.arm_goals = Some(seen.arm_goals);
            asked.positions = Some(positions);
            let near = arrived;
            asked.settled = if near && still { asked.settled + 1 } else { 0 };
            if asked.settled >= THERE_STEPS {
                self.asked = None;
                let wanted = self.pose.unwrap_or(posed);
                let clamped =
                    (0..3).any(|index| (posed[index] - wanted[index]).abs() > POSE_KEPT_WITHIN);
                // Said as reached: a developer sees the sag.
                return Some(with_tag(pose_there(seen.reached, clamped), tag));
            }
        }
        if now.duration_since(asked.since) >= LONGEST_POSE {
            self.asked = None;
            return Some(with_tag(
                pose_cut_short("not there in time: something holds the arm"),
                tag,
            ));
        }
        None
    }

    /// The lease ended: the arm and the roll go back to the stand, and a
    /// pose owed is cut short.
    pub fn end(&mut self, why: &'static str) -> Option<String> {
        self.pose = None;
        self.asked
            .take()
            .map(|owed| with_tag(pose_cut_short(why), owed.tag))
    }
}

fn pose_there(reached: [f32; 3], clamped: bool) -> String {
    let round = |degrees: f32| (f64::from(degrees) * 10.0).round() / 10.0;
    line(serde_json::json!({
        "pose": "there",
        "roll": round(reached[0]),
        "shoulder": round(reached[1]),
        "elbow": round(reached[2]),
        "clamped": clamped,
    }))
}

fn pose_cut_short(why: &str) -> String {
    line(serde_json::json!({"pose": "cut_short", "why": why}))
}

fn pose_refused(why: &str) -> String {
    line(serde_json::json!({"pose": "refused", "why": why}))
}

/// The holder's limp base ([`Limping::ask`], record from pose):
/// `{"limp":true}` lets base yaw go (torque off) so a hand may turn it,
/// `{"limp":false}` takes it back where it lies. Base yaw only: it cannot
/// fall. Answered at once, `{"limp":"on"}` or `{"limp":"off"}`, or
/// `{"limp":"refused","why":…}`. Any end of the hold (let go, lapsed, lost,
/// a conversation) takes it back and says `{"limp":"off","why":…}`; after a
/// conversation the holder asks again, so torque never comes off by itself.
/// Why a look is refused while the base is limp.
pub const LIMP_REFUSES: &str = "the base is limp";

#[derive(Debug, Default)]
pub struct Limping {
    on: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LimpLine {
    limp: bool,
}

impl Limping {
    /// Whether a holder's line is a limp, for [`Limping::ask`].
    pub fn is_limp(line: &[u8]) -> bool {
        serde_json::from_slice::<serde_json::Value>(line)
            .is_ok_and(|value| value.get("limp").is_some())
    }

    /// A limp line from the holder, and its answer.
    pub fn ask(&mut self, line: &[u8], held: bool) -> String {
        let Ok(LimpLine { limp }) = serde_json::from_slice::<LimpLine>(line) else {
            return limp_said(
                "refused",
                Some("a limp is {\"limp\":true} or {\"limp\":false}"),
            );
        };
        if limp && !held {
            return limp_said("refused", Some("the body is not held"));
        }
        self.on = limp;
        limp_said(if limp { "on" } else { "off" }, None)
    }

    /// A look asked while the base is limp: refused, never a no-op on the
    /// base and then a jerk when its torque comes back.
    pub fn look_refused(line: &[u8]) -> String {
        with_tag(refused(LIMP_REFUSES), tag_of(line))
    }

    /// Whether base yaw is let go now, as the body's `Asked::limp` takes it.
    pub fn on(&self) -> bool {
        self.on
    }

    /// Whether the body is held now, or why not: an end of the hold takes
    /// the base back, said once.
    pub fn step(&mut self, held: Result<(), &'static str>) -> Option<String> {
        match held {
            Err(why) if self.on => {
                self.on = false;
                Some(limp_said("off", Some(why)))
            }
            _ => None,
        }
    }
}

fn limp_said(state: &str, why: Option<&str>) -> String {
    match why {
        Some(why) => line(serde_json::json!({"limp": state, "why": why})),
        None => line(serde_json::json!({"limp": state})),
    }
}

fn line(value: serde_json::Value) -> String {
    let mut line = value.to_string();
    line.push('\n');
    line
}

/// The `tag` a holder's look or pose line carries, if any: lamp-control's
/// request, echoed in every answer to that line, so it never pins one line's
/// answer on another. A line refused at once can overtake one still owed
/// (Second's P2 on #5855).
fn tag_of(line: &[u8]) -> Option<u64> {
    serde_json::from_slice::<serde_json::Value>(line)
        .ok()?
        .get("tag")?
        .as_u64()
}

/// `answer` with the `tag` of the line it answers, when it had one; as it is
/// otherwise, so a line without `tag` is answered as before.
fn with_tag(answer: String, tag: Option<u64>) -> String {
    let Some(tag) = tag else {
        return answer;
    };
    match serde_json::from_str::<serde_json::Value>(&answer) {
        Ok(mut value) => {
            value["tag"] = serde_json::json!(tag);
            line(value)
        }
        Err(_) => answer,
    }
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

    /// A look from home is not cut to the reach here, in the home frame: a
    /// standing lamp's pitch pose is about 228 counts up from home, past
    /// `REACH`'s 150 up, and a look at it must reach the body whole (Fifth's
    /// P2 on #4580). The body cuts it, from the pose, as every look.
    #[test]
    fn a_look_from_home_reaches_the_body_whole_for_the_pose_to_cut() {
        let now = Instant::now();
        let mut looking = Looking::default();
        looking.ask(
            br#"{"look":{"yaw":0,"pitch":20,"from":"home"}}"#,
            true,
            REACH,
            now,
        );
        let aim = looking.aim().unwrap();
        assert!((aim[1] + 227.6).abs() < 0.1, "{aim:?}");
        assert_eq!(looking.asked().map(|(_, clamped)| clamped), Some(false));
        // From the pose, the same look is cut to the reach, as before.
        looking.ask(br#"{"look":{"yaw":0,"pitch":20}}"#, true, REACH, now);
        assert_eq!(looking.aim().unwrap()[1], -REACH.up);
    }

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
    /// Lamp-75c9 on 7 Nov: a resting head stopped 17 counts short of a look
    /// and the look was cut short after 5 s, "something holds the head".
    /// Stopped inside the rest's slack, it is there once still a second (not
    /// before: a servo may not have started), from where it stopped, and
    /// `clamped`; still farther short at 5 s, it is cut short with how far.
    #[test]
    fn a_look_stopped_short_is_there_inside_the_slack_and_cut_short_beyond_it() {
        let now = Instant::now();
        let tick = Duration::from_millis(20);
        let mut looking = Looking::default();
        looking.ask(
            br#"{"look":{"yaw":5,"pitch":5,"from":"home"}}"#,
            true,
            REACH,
            now,
        );
        // Pitch 1943 wanted (57 up from 2000), stopped at 1960; base there.
        let seen = Some(([1943, 2060, 2072], [1960, 2060, 2072]));
        let mut at = now;
        for step in 0..REST_STOPPED_STEPS {
            at += tick;
            assert_eq!(looking.step(Ok(()), seen, at), None, "step {step}");
        }
        at += tick;
        let said = value(&looking.step(Ok(()), seen, at).expect("there"));
        assert_eq!(said["look"], "there");
        assert_eq!(said["clamped"], true);
        assert_eq!(said["pitch"], 3.5);
        assert_eq!(said["yaw"], 5.0);
        // 40 counts short, past the slack: owed until 5 s, then cut short.
        looking.ask(
            br#"{"look":{"yaw":5,"pitch":5,"from":"home"}}"#,
            true,
            REACH,
            now,
        );
        let seen = Some(([1943, 2060, 2072], [1983, 2060, 2072]));
        let mut at = now;
        while at < now + LONGEST_LOOK - tick {
            at += tick;
            assert_eq!(looking.step(Ok(()), seen, at), None);
        }
        let said = value(
            &looking
                .step(Ok(()), seen, now + LONGEST_LOOK)
                .expect("cut short"),
        );
        assert_eq!(said["look"], "cut_short");
        assert_eq!(
            said["why"],
            "not there in time: stopped 40 counts short of the look; something holds the head or base"
        );
    }

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
        assert_eq!(held.line(), said(r#"{"lease":"held","tags":true}"#));
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
            said(r#"{"lease":"suspended","why":"conversation","tags":true}"#)
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

    fn seen(posed: [f32; 3], arm_goals: [u16; 2]) -> Option<PoseSeen> {
        Some(PoseSeen {
            posed: Ok(posed),
            reached: posed,
            roll: (2_044, 2_044),
            arm_goals,
            arm_positions: arm_goals,
        })
    }

    /// A pose names one joint at least, in numbers, and only while held;
    /// a joint not named keeps where the body has it.
    #[test]
    fn a_pose_is_refused_unless_held_and_a_named_joint_keeps_the_rest() {
        let now = Instant::now();
        let mut posing = Posing::default();
        let has = Ok([0.0, 52.7, 48.5]);
        for (line, why) in [
            (&br#"{"pose":{}}"#[..], "one at least"),
            (br#"{"pose":{"wrist":3}}"#, "one at least"),
            (br#"{"pose":"up"}"#, "one at least"),
        ] {
            let answer = posing.ask(line, true, has, now).unwrap();
            assert_eq!(value(&answer)["pose"], "refused");
            assert!(
                value(&answer)["why"].as_str().unwrap().contains(why),
                "{answer}"
            );
        }
        let answer = posing
            .ask(br#"{"pose":{"shoulder":30}}"#, false, has, now)
            .unwrap();
        assert_eq!(value(&answer)["why"], "the body is not held");
        let answer = posing
            .ask(
                br#"{"pose":{"shoulder":30}}"#,
                true,
                Err("a gesture plays"),
                now,
            )
            .unwrap();
        assert_eq!(value(&answer)["why"], "a gesture plays");
        assert_eq!(posing.pose(), None);
        assert_eq!(
            posing.ask(br#"{"pose":{"shoulder":30}}"#, true, has, now),
            None
        );
        assert_eq!(posing.pose(), Some([0.0, 30.0, 48.5]));
        // Another pose cuts the first short, and keeps its shoulder.
        let answer = posing
            .ask(br#"{"pose":{"elbow":20}}"#, true, has, now)
            .unwrap();
        assert_eq!(value(&answer)["pose"], "cut_short");
        assert_eq!(posing.pose(), Some([0.0, 30.0, 20.0]));
    }

    /// lamp-75c9's stand, as its arm rests: goals [2639, 2570], positions
    /// as given, its stand [49.5, 46.1] degrees from the fold.
    fn standing_at(arm_positions: [u16; 2]) -> Option<PoseSeen> {
        let degrees = |position: u16, folded: u16| {
            (f32::from(position) - f32::from(folded)) / COUNTS_PER_DEGREE
        };
        Some(PoseSeen {
            posed: Ok([0.0, 49.5, 46.1]),
            reached: [
                0.0,
                degrees(arm_positions[0], 2_076),
                degrees(arm_positions[1], 2_045),
            ],
            roll: (2_059, 2_060),
            arm_goals: [2_639, 2_570],
            arm_positions,
        })
    }

    /// lamp-75c9 at its stand, 2 Nov: the arm sags 19 and 35 counts short of
    /// its goals under its own load. After a conversation, a pose past the
    /// window (kept at the stand) is there once the arm is still, said
    /// `clamped` and as reached, sag and all.
    #[test]
    fn a_pose_after_a_conversation_is_there_with_the_arm_sagging_short() {
        let now = Instant::now();
        let mut posing = Posing::default();
        let stand = [0.0, 49.5, 46.1];
        posing.ask(
            br#"{"pose":{"shoulder":49.5,"elbow":46.1}}"#,
            true,
            Ok(stand),
            now,
        );
        let said = posing.step(Err("conversation"), None, now).unwrap();
        assert_eq!(value(&said)["why"], "conversation");
        assert_eq!(
            posing.ask(br#"{"pose":{"shoulder":80}}"#, true, Ok(stand), now),
            None
        );
        let mut answer = None;
        for step in 0..=THERE_STEPS {
            answer = posing.step(Ok(()), standing_at([2_620, 2_535]), now);
            if answer.is_some() {
                assert_eq!(step, THERE_STEPS, "settled over THERE_STEPS, no sooner");
                break;
            }
        }
        let answer = value(&answer.expect("there, not left owed"));
        assert_eq!(answer["pose"], "there");
        assert_eq!(answer["clamped"], true);
        // As reached: 19 and 35 counts under the stand's goals.
        assert_eq!(answer["shoulder"], 47.8);
        assert_eq!(answer["elbow"], 43.1);
    }

    /// Stopped farther short than a sag (`POSE_SAG_WITHIN`): something holds
    /// the arm, and the pose is cut short in time, as before.
    #[test]
    fn a_pose_stopped_far_short_of_its_goal_is_cut_short() {
        let now = Instant::now();
        let mut posing = Posing::default();
        posing.ask(
            br#"{"pose":{"shoulder":49.5}}"#,
            true,
            Ok([0.0, 49.5, 46.1]),
            now,
        );
        let held = standing_at([2_489, 2_570]);
        for step in 0..=THERE_STEPS * 3 {
            assert_eq!(posing.step(Ok(()), held, now), None, "step {step}");
        }
        let said = posing.step(Ok(()), held, now + LONGEST_POSE).unwrap();
        assert_eq!(value(&said)["pose"], "cut_short");
        assert_eq!(
            value(&said)["why"],
            "not there in time: something holds the arm"
        );
    }

    /// A joint still moving, even inside the sag, keeps the pose owed: the
    /// elbow 48 to 8 counts short, rising 2 a step.
    #[test]
    fn a_pose_with_a_joint_still_moving_stays_owed() {
        let now = Instant::now();
        let mut posing = Posing::default();
        posing.ask(
            br#"{"pose":{"elbow":46.1}}"#,
            true,
            Ok([0.0, 49.5, 46.1]),
            now,
        );
        for step in 0..=20 {
            let moving = standing_at([2_620, 2_522 + 2 * step]);
            assert_eq!(posing.step(Ok(()), moving, now), None, "step {step}");
        }
    }

    /// There once the arm has held its goals for `THERE_STEPS`, said as the
    /// body kept it (`clamped` when its window cut it); cut short when the
    /// lease is suspended, the body stops taking a pose, or in time.
    #[test]
    fn a_pose_is_there_once_still_and_says_what_was_kept() {
        let now = Instant::now();
        let mut posing = Posing::default();
        posing.ask(
            br#"{"pose":{"shoulder":90}}"#,
            true,
            Ok([0.0, 52.7, 48.5]),
            now,
        );
        let mut answer = None;
        for step in 0..=THERE_STEPS {
            answer = posing.step(Ok(()), seen([0.0, 52.7, 48.5], [2_650, 2_600]), now);
            if step < THERE_STEPS {
                assert_eq!(answer, None);
            }
        }
        let answer = value(&answer.unwrap());
        assert_eq!(answer["pose"], "there");
        assert_eq!(answer["shoulder"], 52.7);
        assert_eq!(answer["clamped"], true);
        // Kept while held: the next pose starts from it.
        assert_eq!(posing.pose(), Some([0.0, 90.0, 48.5]));

        posing.ask(
            br#"{"pose":{"elbow":10}}"#,
            true,
            Ok([0.0, 52.7, 48.5]),
            now,
        );
        let said = posing.step(Err("conversation"), None, now).unwrap();
        assert_eq!(value(&said)["why"], "conversation");
        assert!(posing.pose().is_some(), "a conversation keeps it for later");

        posing.ask(
            br#"{"pose":{"elbow":10}}"#,
            true,
            Ok([0.0, 52.7, 48.5]),
            now,
        );
        let not = Some(PoseSeen {
            posed: Err("a gesture plays"),
            ..seen([0.0; 3], [2_650, 2_600]).unwrap()
        });
        let said = posing.step(Ok(()), not, now).unwrap();
        assert_eq!(value(&said)["why"], "a gesture plays");

        posing.ask(
            br#"{"pose":{"elbow":10}}"#,
            true,
            Ok([0.0, 52.7, 48.5]),
            now,
        );
        let moving = |goal| seen([0.0, 52.7, 48.5], [2_650, goal]);
        assert_eq!(posing.step(Ok(()), moving(2_500), now), None);
        let said = posing
            .step(Ok(()), moving(2_480), now + LONGEST_POSE)
            .unwrap();
        assert_eq!(value(&said)["pose"], "cut_short");

        let said = posing.end("let go");
        assert_eq!(said, None, "nothing owed");
        assert_eq!(posing.pose(), None, "the lease's end lets the pose go");
    }

    /// A limp is taken only while held, said at once, and any end of the
    /// hold takes it back, said once with why; it never comes back by itself.
    #[test]
    fn a_limp_base_is_only_while_held_and_any_end_takes_it_back() {
        let mut limping = Limping::default();
        assert!(Limping::is_limp(br#"{"limp":true}"#));
        assert!(!Limping::is_limp(br#"{"look":"ahead"}"#));
        let said = limping.ask(br#"{"limp":true}"#, false);
        assert_eq!(value(&said)["limp"], "refused");
        assert_eq!(value(&said)["why"], "the body is not held");
        assert!(!limping.on());
        let said = limping.ask(br#"{"limp":"yes"}"#, true);
        assert_eq!(value(&said)["limp"], "refused");
        assert_eq!(
            limping.ask(br#"{"limp":true}"#, true),
            said_line(r#"{"limp":"on"}"#)
        );
        assert!(limping.on());
        assert_eq!(limping.step(Ok(())), None);
        let said = limping.step(Err("conversation")).unwrap();
        assert_eq!(
            value(&said),
            serde_json::json!({"limp": "off", "why": "conversation"})
        );
        assert!(
            !limping.on(),
            "a conversation takes it back; the holder asks again"
        );
        assert_eq!(limping.step(Ok(())), None);
        assert_eq!(limping.step(Err("the lease ended")), None, "said once");
        limping.ask(br#"{"limp":true}"#, true);
        assert_eq!(
            limping.ask(br#"{"limp":false}"#, true),
            said_line(r#"{"limp":"off"}"#)
        );
        assert!(!limping.on());
    }

    fn said_line(text: &str) -> String {
        text.to_owned() + "\n"
    }

    /// Second's P2 on #5855: a line refused at once overtakes one still
    /// owed, so each answer says which line it answers by the `tag` it came
    /// with. A accepted, B refused while A is owed, then A cut: each answer
    /// carries its own line's `tag`, and a line without `tag` is answered as
    /// before.
    #[test]
    fn each_look_and_pose_answer_carries_its_own_lines_tag() {
        let now = Instant::now();
        let mut looking = Looking::default();
        assert_eq!(
            looking.ask(br#"{"look":{"yaw":5,"pitch":2},"tag":1}"#, true, REACH, now),
            None
        );
        // B refused at once, while A is owed: B's tag, not A's.
        let b = value(
            &looking
                .ask(br#"{"look":"sideways","tag":2}"#, true, REACH, now)
                .unwrap(),
        );
        assert_eq!(
            (b["look"].as_str(), b["tag"].as_u64()),
            (Some("refused"), Some(2))
        );
        let b = value(
            &looking
                .ask(br#"{"look":"ahead","tag":3}"#, false, REACH, now)
                .unwrap(),
        );
        assert_eq!(
            (b["look"].as_str(), b["tag"].as_u64()),
            (Some("refused"), Some(3))
        );
        // A still owed, then cut: A's tag.
        let a = value(&looking.end("let go").unwrap());
        assert_eq!(
            (a["look"].as_str(), a["tag"].as_u64()),
            (Some("cut_short"), Some(1))
        );
        // A replaced by C: A's cut_short with A's tag, at C's ask.
        looking.ask(br#"{"look":"ahead","tag":4}"#, true, REACH, now);
        let a = value(
            &looking
                .ask(br#"{"look":{"yaw":1,"pitch":1},"tag":5}"#, true, REACH, now)
                .unwrap(),
        );
        assert_eq!(
            (a["look"].as_str(), a["tag"].as_u64()),
            (Some("cut_short"), Some(4))
        );
        let c = value(&looking.step(Err("hand"), None, now).unwrap());
        assert_eq!(c["tag"].as_u64(), Some(5));
        // A line without tag: answered as before, with no tag.
        looking.ask(br#"{"look":"ahead"}"#, true, REACH, now);
        let old = looking.end("let go").unwrap();
        assert_eq!(old, said(r#"{"look":"cut_short","why":"let go"}"#));

        let mut posing = Posing::default();
        let has = Ok([0.0, 52.7, 48.5]);
        assert_eq!(
            posing.ask(br#"{"pose":{"shoulder":30},"tag":7}"#, true, has, now),
            None
        );
        let b = value(
            &posing
                .ask(br#"{"pose":{"elbow":10},"tag":8}"#, false, has, now)
                .unwrap(),
        );
        assert_eq!(
            (b["pose"].as_str(), b["tag"].as_u64()),
            (Some("refused"), Some(8))
        );
        let b = value(
            &posing
                .ask(
                    br#"{"pose":{"elbow":10},"tag":9}"#,
                    true,
                    Err("the lamp is not standing still at its full height"),
                    now,
                )
                .unwrap(),
        );
        assert_eq!(
            (b["pose"].as_str(), b["tag"].as_u64()),
            (Some("refused"), Some(9))
        );
        let a = value(
            &posing
                .ask(br#"{"pose":{"elbow":5},"tag":10}"#, true, has, now)
                .unwrap(),
        );
        assert_eq!(
            (a["pose"].as_str(), a["tag"].as_u64()),
            (Some("cut_short"), Some(7))
        );
        let a = value(&posing.end("let go").unwrap());
        assert_eq!(
            (a["pose"].as_str(), a["tag"].as_u64()),
            (Some("cut_short"), Some(10))
        );
        posing.ask(br#"{"pose":{"elbow":5}}"#, true, has, now);
        assert!(value(&posing.end("let go").unwrap()).get("tag").is_none());
        // A tag that isn't an unsigned 64-bit integer refuses the line,
        // and that refusal carries no tag (Second's P3 on #5880).
        for bad in [
            &br#"{"look":"ahead","tag":"7"}"#[..],
            br#"{"look":"ahead","tag":-1}"#,
            br#"{"look":"ahead","tag":1.5}"#,
        ] {
            let answer = value(&Looking::default().ask(bad, true, REACH, now).unwrap());
            assert_eq!(answer["look"], "refused");
            assert!(answer.get("tag").is_none());
        }
        let answer = value(
            &Posing::default()
                .ask(br#"{"pose":{"elbow":5},"tag":"7"}"#, true, has, now)
                .unwrap(),
        );
        assert_eq!(answer["pose"], "refused");
        assert!(answer.get("tag").is_none());
        // While limp, a look refused with its own tag.
        assert_eq!(
            value(&Limping::look_refused(br#"{"look":"ahead","tag":11}"#))["tag"],
            11
        );
        assert!(
            value(&Limping::look_refused(br#"{"look":"ahead"}"#))
                .get("tag")
                .is_none()
        );
    }
}
