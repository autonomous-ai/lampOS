//! A small expressive body: the head's pitch and roll and the base's yaw move
//! a few degrees around where they were found, following what the lamp is
//! doing, and stop at the first surprise.
//!
//! This is not the qualified motion worker. It exists so that a lamp folded
//! in its rest pose can tilt, look away and nod while it talks, within limits
//! narrow enough to be checked by eye. What it may do:
//!
//! - It commands three joints only: head pitch, head roll and base yaw. The
//!   shoulder and the elbow are read on every step and never written.
//! - It writes three registers only: the goal position of those joints, and
//!   once at start their goal speed and acceleration, so that the servo
//!   itself is slow whatever this process sends.
//! - It never touches torque unless told to take hold (`Limits::torque_on`),
//!   to let go under overload or, with `Limits::limp`, under a hand, or as
//!   it ends on a surprise or refuses a joint found hot or loaded: then, as
//!   V1, torque off on all five (under a hand, on the head and base only).
//!   A joint whose torque is off, or whose status byte is not clear, is a
//!   reason not to start; told to take hold, it first sets the goal of a
//!   joint found without torque to where it is and then writes torque on.
//! - Every goal is within a fixed reach of its home (a pose given at start,
//!   or else the pose found), and the head's pitch also within a fixed
//!   absolute window short of the base. A head found outside the reach of a
//!   given home is first brought into it, never moved away from it. Told
//!   to wake (`Limits::wake`), a joint found out of where it may start, up
//!   to [`WAKE_REACH`] from that home, is first brought there slowly.
//! - After every step it reads all five joints. A load over its bound, a
//!   joint that does not follow, a status byte that changes, a shoulder or
//!   elbow that moves, a joint that stays hot or a wire that is not exactly
//!   as expected ends the run: the present positions are written as the goals, so that
//!   nothing keeps pushing, then torque off on all five, and nothing after that.
use crate::{
    Joint,
    clips::{self, Clip},
    cog,
    gaze::{Gaze, Looking, Seen},
    gesture::{Gestures, Mood, Pose},
    idle,
    inspection::{Interest, PortIo, Runtime},
    protocol::{ReadOnlyRequest, ReadWindow, StatusFlags, StatusParser},
    units::ENCODER_MAX,
};
use serde::Serialize;
use std::{io, time::Duration};

pub use crate::moves::{Asker, Priority, Source};

/// One step: goals are written and all five joints read this often.
pub const TICK: Duration = Duration::from_millis(20);
/// One request and its replies, or the step fails.
const EXCHANGE: Duration = Duration::from_millis(12);
const POLL: Duration = Duration::from_millis(1);
/// A mood older than this is rest: a silent sender is not a reason to hold
/// a pose. A skill that holds `Mood::Reading` says it again inside this.
pub const MOOD_STALE: Duration = Duration::from_millis(1_500);
/// The longest the head takes to ease home when asked to stop.
const LEAVE: Duration = Duration::from_secs(3);
/// How stiffly the commanded pose follows the wanted one, in radians a
/// second: it arrives in well under a second without overshoot.
const FOLLOW: f32 = 7.0;
/// At rest for this many steps, a joint within `REST_SLACK` counts of its
/// goal is given its own position as the goal. A servo stops a few counts
/// short of a goal and goes on pushing at it: on lamp-4ace the yaw servo sat
/// 8 counts short at a load of 44 for as long as it rested.
const REST_STEPS: u16 = 50;
/// A joint that met resistance is left where it is for this many steps, two
/// seconds, before it is asked to move again.
const EASE_STEPS: u16 = 100;
/// How often that may happen before the run ends: the fourth time within
/// this many steps, a minute.
const EASES_ALLOWED: usize = 3;
const EASE_MEMORY_STEPS: u64 = 3_000;
const REST_SLACK: u16 = 24;
/// At rest on its goal: within `REST_SLACK` of it. The same slack attests a
/// fold for unplugging: lamp-52e6's elbow rests 13 counts past its folded
/// goal (2082 against 2069) every time it folds (#1699 trial, 2026-10-20).
fn rests_on(position: u16, goal: u16) -> bool {
    position.abs_diff(goal) <= REST_SLACK
}
/// The most a joint it took hold of may have moved when its torque came on:
/// a goal set where it is moves it by nothing a servo would notice.
const TAKE_HOLD_SLACK: u16 = 12;
/// A write to a joint it takes hold of is tried this many times, this far
/// apart, before the run ends. Just after the power comes on a servo may not
/// answer yet: on lamp-52e6 on 2026-10-15, 19 s after power, the head's tilt
/// did not acknowledge its goal and the run ended; 15 s later it stood.
const TAKE_HOLD_TRIES: u8 = 10;
const TAKE_HOLD_RETRY: Duration = Duration::from_millis(300);
/// A joint that has arrived is given its own position as its goal, in any
/// mood, and is left there until the pose wanted of it is this many counts
/// from where it is. Asked for less, a servo pushes and does not move: on
/// lamp-4ace the head sat 3 to 8 counts from the goals of its nods while its
/// load climbed from 48 to 178 in a second and a half, three times in a
/// minute. With 12 here it did the same for nods of 11 and 12 counts, four
/// times in ten minutes: from rest that joint does not start for so little.
const WAKE_COUNTS: f32 = 20.0;
/// Under `--give-way stay`: a joint that gave way and is then moved more
/// than `HAND_MOVED` counts from where it gave way, while it stays in a hand
/// for `HAND_STEPS` steps in a row (a second), is left where it was put.
/// V1's lesson: a bump wants the lamp to come back; a person holding or
/// adjusting the head for longer than about a second wants it to stay.
///
/// With `--limp on` as well, a hand as the old runtime (V1) met it
/// (`robots/lamp/servo_overload.json`, `hal/drivers/motors/animation_service.py`
/// on `main`), the hardware team's choice to protect the servos. A
/// commanded joint over `CONTACT_FLOOR` (head pitch, head roll, base yaw;
/// V1's `contact.load`, 0.1% of full drive) for `CONTACT_STEPS` steps, or
/// over its own load bound as before, stops where it is. Still pushed after
/// that for `HAND_STEPS` steps in a row (V1's `release_after_s`, 1.0 s):
/// its load over the lower of the bound that stopped it and its floor, or
/// held more than `LIMP_LAG` counts (V1's `CONTACT_RELEASE_LAG`, one
/// degree) from where it stopped, and the lamp lets go as V1's contact
/// stop handed over to its overload cut-off, but of the head and base
/// only: torque off on head pitch, head roll and base yaw, while the
/// shoulder and the elbow keep their torque and their last goal, so a
/// standing lamp stays standing (#3702: onboarding asks a person to push
/// the head, and a lamp whose arm collapsed under that read as broken).
/// `HAND_REST` later the head and base are held where they lie and
/// brought home slowly, as under `--overload release`. V1 rested 120 s here too (`retry_s`), but that is for a servo
/// cooling from overload; a hand is not heat, and a lamp limp for two
/// minutes after a push reads as broken. A creature pushed hard flops,
/// then gathers itself within seconds (the first builder, #780). Unlike that, a hand never ends the run: it
/// may make the lamp slump again as often as it pushes, as V1's did. On
/// lamp-52e6 on 2026-10-14 a head that only gave way by its goal, torque
/// on, could not be pushed at all: roll 2,019 to 1,958 at a load of 256,
/// yaw 2,035 to 1,986 at 188; on 2026-10-15 a port that let go of the base
/// alone did not slump, and a hand still on it after it was taken again
/// ended the run.
const CONTACT_FLOOR: [u16; 3] = [650, 750, 550];
/// With a force limit the base's floor is this share of it, in percent:
/// V1's 550 is 55% of full drive.
const CONTACT_YAW_SHARE: u16 = 55;
const CONTACT_STEPS: u8 = 3;
const HAND_STEPS: u16 = 50;
use crate::HAND_MOVED;
const LIMP_LAG: u16 = 11;
/// With `--give-way`, a joint whose goal is more than `STALL_SLACK` counts
/// from where it is, whose load is over `STALL_LOAD` and which moves no
/// more than `STALL_SLACK` counts for `STALL_STEPS` steps is told to stop
/// pushing: its goal becomes where it is, until what is wanted of it moves
/// `WAKE_COUNTS` or more. Never torque off for that: on V1 a shoulder or an
/// elbow let go dropped the arm with nobody touching it.
///
/// One and a half seconds. Longer than a joint takes to be put to rest by
/// what it already has (arrived for 0.3 s, at rest for 1 s), so that only
/// a joint those leave pushing is caught. Shorter than the servo's own
/// protection (80% load for 2 s, then 20% of its force: on the shoulder or
/// the elbow that is the arm dropping), so that this acts first. Heat comes
/// from pushing for minutes, not seconds (the elbow on lamp-4ace reached
/// 69 degrees one day with the head resting on the base), so a second and a
/// half of it costs nothing. 100 of 1,000 is over what a resting joint
/// reads at its goal on lamp-4ace (68 for the head, 44 to 72 for the base
/// resting short) and under the 100 to 150 lamp-52e6's shoulder pushes with
/// for as long as it stands, 22 to 34 counts short of its goal.
const STALL_STEPS: u16 = 75;
/// A head or base joint told to stop pushing is said as a hand
/// (`body/hand`) for this many steps (half a second): long enough for the
/// voice to read it, never so long a rest against something reads as one.
const STALL_HAND_STEPS: u16 = 25;
const STALL_LOAD: u16 = 100;
/// A standing shoulder or elbow carries the arm: it counts as pushing only
/// over the load it read once stood up plus this. On lamp-4ace the shoulder
/// stands at 112 (elbow 23), and from 2026-10-15 to 16 that weight alone made
/// 426 shoulder stalls in 3 h 50 min at rest; lamp-52e6's shoulder wanders
/// 100 to 150 while it stands. 100 is twice that wander, and far under a
/// hand stopping the arm, which reads hundreds (the arm's own bound is 950).
const ARM_STALL_MARGIN: u16 = 100;
/// The most a free-standing arm joint may carry for that load to be learned:
/// lamp-52e6's shoulder stands at 100 to 150, lamp-4ace's at 112. A joint
/// reading more when it settles is already held by something (a hand, an
/// obstacle): learning that would lift its bound out of reach, so nothing is
/// learned and it keeps `STALL_LOAD` until it stands under this.
const ARM_CARRIES_MOST: u16 = STALL_LOAD + ARM_STALL_MARGIN;
const STALL_SLACK: u16 = 6;
/// A held servo's reading jitter, two counts back and forth: a joint this
/// close to its goal stands there.
const STOOD_JITTER: u16 = 2;
/// Arrived: this close to the wanted pose and this slow, for this many steps.
const ARRIVED_COUNTS: f32 = 6.0;
const ARRIVED_SPEED: f32 = 15.0;
const ARRIVED_STEPS: u8 = 15;
/// Steps in a row on which a read, or a write, may fail before the run
/// ends: half a second. While one is failing nothing new is asked of the
/// joints. Three steps were not enough: on lamp-4ace one stall of the whole
/// machine (the ring's own write overran in the same second) left a late
/// reply on the wire, the next three exchanges tripped over it, and the lamp
/// had no body until someone started it again.
const WIRE_STEPS: u8 = 25;

/// The joints this body commands, in the order of every array of three here.
/// The glance a look away begins with, in counts of the base's turn: a
/// little over three degrees, which the base is back from in a third of a
/// second at the speed lamp-4ace runs with. It is the whole look away of a
/// lamp with the folded reach (nine tenths of 40 counts). More than
/// [`WAKE_COUNTS`], or it would not be made at all.
const GLANCE: f32 = 36.0;

/// Waking (`Limits::wake`): the farthest a commanded joint may be found
/// from its home and still be brought there, about 35 degrees. A head
/// resting on the base was found 145 counts below lamp-4ace's home (2,095
/// for 1,950); lamp-52e6's home is 1,771, so its head on the base would be
/// some 300 away. Beyond this a joint is somewhere no lamp has been seen,
/// and it is refused as before. At the wake's speed the whole of it takes
/// 31 s, a head's and then a base's about a minute, inside the 90 s
/// systemd allows a service to say it is ready.
pub const WAKE_REACH: u16 = 400;
/// The fastest a joint moves while it wakes, counts a second: two fifths of
/// the body's own 50, under two degrees a second.
const WAKE_SPEED: f32 = 20.0;
/// A wake begins with a stir of at most this many counts toward home, more
/// than [`WAKE_COUNTS`] so that a servo starts for it, and then a pause.
const WAKE_STIR: f32 = 24.0;
const WAKE_PAUSE: f32 = 1.0;
/// A head that must lift out of a droop does not move for a goal just
/// ahead of it. On lamp-52e6 after a power cut on 2026-10-14 (head pitch
/// gain P 32), a goal 40 counts up moved the drooped head 7 counts and it
/// stuck at a load of 144; on a ramp of 260 counts in 3 s it lagged its
/// goal by 40 to 80 counts at loads of 200 to 320 and stopped 37 short of
/// the end. So the stir of a head that lifts reaches this far ahead of
/// where it was found, the largest lag seen while it lifted, and the rise
/// begins from there with the goal leading it.
const WAKE_LIFT_LEAD: f32 = 80.0;
/// While its goal is still on the way, a waking joint may be this far from
/// it (the most lamp-52e6's head lagged while lifting, 80, and some), for
/// `Limits::tracking_steps` steps, before the run ends.
const WAKE_TRACKING: u16 = 100;
/// The load a head lifting out of a droop may read for
/// `Limits::load_steps` steps: lamp-52e6's read 200 to 320 doing it.
const WAKE_LIFT_LOAD: u16 = 400;
/// Once every goal is home, a joint still farther than `Limits::tracking`
/// from it that has not moved `WAKE_MOVED` counts toward it for this many
/// steps (half a second) ends the run: it has stopped short.
const WAKE_STILL_STEPS: u8 = 25;
const WAKE_MOVED: u16 = 3;
/// A joint that has stopped coming nearer within this many counts of home
/// has woken where it settled. On lamp-52e6 on 2026-10-14, woken from a
/// real droop (found yaw 1,979, roll 2,101, pitch 2,029 for a home of
/// 2,035, 2,035, 1,771), the head lifted 226 counts in 20.8 s and stayed 32
/// short of home, the yaw 11 and the roll 17: from below, these servos
/// (pitch gain P 32, roll and yaw 16) do not close the last 30 to 40
/// counts. Farther than this from home it has not woken, and the run ends.
const WAKE_SETTLE: u16 = 50;
/// How far the shoulder or the elbow may sway while the lamp wakes before
/// that is taken for a move: on lamp-4ace on 2026-10-14 the shoulder moved
/// from 2,050 to 2,057 as the base turned 189 counts of its wake, one over
/// the run's 6. Twice what was seen, about a degree and a third; an arm
/// that falls or is pushed moves far more.
const WAKE_SWAY: u16 = 15;
/// The fastest a shoulder found up by itself is lowered while the lamp
/// wakes, counts a second. On lamp-52e6 on 2026-10-14, unplugged standing,
/// the shoulder stayed up at 2,633 on its gears and the elbow fell folded;
/// eased by hand from 2,633 to 2,053 over 15 s (39 a second on average,
/// about 58 at its fastest) it reached 2,059 at a load of 172 at most. This
/// is a little slower than that at its fastest: 580 counts take about 22 s,
/// and with a head's wake after it the lamp is up well inside the 90 s
/// systemd allows a service to say it is ready. The wake's own 20 would
/// take 44 s for the shoulder alone.
const ARM_LOWER_SPEED: f32 = 40.0;

/// Under `--overload release` (`Limits::overload_release`), the old
/// runtime's cut-off (`hal/drivers/motors/overload.py`,
/// `robots/lamp/servo_overload.json`): any of the five joints that reads a
/// load of at least `OVERLOAD_LOAD` of 1,000 on `OVERLOAD_STEPS` steps in a
/// row (1.5 s) has torque switched off on all five, which are left so for
/// `OVERLOAD_REST` and then taken up again slowly. A second time within
/// `OVERLOAD_AGAIN` of the first ends the run with torque left off.
///
/// A joint held to a force limit (`--force-limit`) cannot read much more
/// than that limit, so 800 would never trip on it: V1 says so of itself
/// ("a force-limited joint can never read the 800 that trips the
/// cut-off"). For such a joint the level is `OVERLOAD_SHARE` percent of its
/// limit, the same 80% of what it may push with that 800 is of a joint
/// without one: 480 at lamp-4ace's 600.
const OVERLOAD_LOAD: u16 = 800;
const OVERLOAD_SHARE: u16 = 80;
const OVERLOAD_STEPS: u16 = 75;
const OVERLOAD_REST: Duration = Duration::from_secs(120);
/// How long a slump from a hand (`--limp on`) rests before the lamp takes
/// hold again: a little longer than V1's contact pause (`pause_s`, 3 s).
const HAND_REST: Duration = Duration::from_secs(5);
const OVERLOAD_AGAIN: Duration = Duration::from_secs(600);

pub const COMMANDED: [Joint; 3] = [Joint::WristPitch, Joint::WristRoll, Joint::BaseYaw];
/// A gesture asked for while the lamp is still rising waits this long for it
/// to be standing, and is then forgotten.
const GESTURE_WAIT: Duration = Duration::from_secs(3);
/// The most a gesture's offsets change in one step: head pitch, head roll,
/// base yaw, shoulder, elbow. A little over the fastest a clip asks, so that
/// a clip is followed exactly and one that is dropped goes back no faster.
const GESTURE_STEP: [f32; 5] = [24.0, 34.0, 26.0, 21.0, 21.0];
/// The farthest a gesture takes each of those from where the lamp has it.
const GESTURE_REACH: [f32; 5] = [220.0, 220.0, 170.0, 360.0, 360.0];
/// While it speaks, a lamp standing at its full height leans toward the
/// person: the shoulder and the elbow this many counts from the pose it
/// stands in (about four and five degrees), eased in as the answer starts
/// and out as it ends. Toward the person is the way V1's nod (`nod.csv`)
/// takes both as the head dips: down. That clip goes to 77 and 120 and
/// keeps the lamp's weight inside `motion.max_cog_offset_mm` (22 mm);
/// about half of each moves the centre of gravity a few millimetres
/// whichever way the calibration has the joints. The base turning
/// and a lean are what read from the front, where the head is a disc
/// facing the person (#933, lamp-4ace, 2026-10-16).
pub const LEAN: [f32; 2] = [-45.0, -60.0];
/// The lean takes at least this long each way, and longer if its farther
/// joint would go faster than the pose follows at (`Limits::speed`).
const LEAN_SECONDS: f32 = 1.5;
/// A gesture ended for another (`Limits::arbitration`) eases back to
/// nothing over this long: from the speed it had, slowing into the end as a
/// cubic does, never faster than `GESTURE_STEP`, so it neither snaps back
/// nor stops dead.
const EASE_OUT: Duration = Duration::from_millis(700);
/// A skill's or idle gesture paused as listening began picks up again where
/// it was if the voice goes back to resting within this, with no answer
/// (#2602: the line was music, a cough, or not said to the lamp). A longer
/// hold ends it, eased out, so the arm never takes a move back up long
/// after the moment.
const RESUME_WITHIN: Duration = Duration::from_secs(3);
/// The switch for [`Limits::gesture_pause`]; only the exact value `on`
/// turns it on.
pub const GESTURE_PAUSE: &str = "LAMP_GESTURE_PAUSE";

/// Whether a value of [`GESTURE_PAUSE`] turns the pause on.
pub fn gesture_pause_from(value: Option<&str>) -> bool {
    value == Some("on")
}

/// `LAMP_BODY_OWN_RANGES=on` (#3558): the body reads each joint's own
/// stored limits at start and bounds the lamp by them (`Limits::own_ranges`).
pub const OWN_RANGES: &str = "LAMP_BODY_OWN_RANGES";

/// Whether a value of [`OWN_RANGES`] turns the bound on.
pub fn own_ranges_from(value: Option<&str>) -> bool {
    value == Some("on")
}

/// How far inside a joint's own stored limits a head or base goal stays
/// (#3558): under two degrees, so a goal is never one the servo would
/// refuse and leave the joint short of.
const OWN_RANGE_MARGIN: u16 = 20;

/// The window over which a joint off its goal must have turned toward it to
/// be following: five steps, a tenth of a second.
const FOLLOW_STEPS: usize = 5;

/// The least net turn toward the goal over [`FOLLOW_STEPS`] that is
/// following: over a held servo's reading jitter (two counts back and forth
/// nets nothing), under a servo's slowest follow.
const FOLLOW_NET: u16 = 4;

/// A joint's positions over the last [`FOLLOW_STEPS`], and whether it turned
/// toward its goal by at least [`FOLLOW_NET`] since the oldest of them:
/// following a goal that moved faster than it turns. Not whether the gap
/// shrank: while a goal ramps away faster than the servo gathers speed, the
/// gap grows though the joint turns toward it; on lamp-52e6 (28 Oct, 8001)
/// that read as a hand 52 times in 15 minutes. Not step to step: a held
/// joint's reading jitters, and a two-count step back toward the goal every
/// other step kept it "following" for good (the Reviewer-lead's P2s on
/// #3925). On its track, and on the first step off it, the window starts
/// again from here. A hand holds it still or turns it away.
fn note_turn(
    recent: &mut [u16; FOLLOW_STEPS],
    seen: &mut usize,
    goal: u16,
    position: u16,
    off_track: u8,
) -> bool {
    if off_track <= 1 {
        *recent = [position; FOLLOW_STEPS];
        *seen = 0;
        return true;
    }
    let slot = *seen % FOLLOW_STEPS;
    let then = recent[slot];
    recent[slot] = position;
    // Until the window is full (and `pushed` waits longer: `load_steps`, six),
    // only a joint not turned away is following.
    let net = if *seen < FOLLOW_STEPS { 0 } else { FOLLOW_NET };
    *seen += 1;
    goal.abs_diff(position) + net <= goal.abs_diff(then)
}

/// The window a head or base goal must be inside for a joint whose servo
/// stores `range` (registers 9 and 11): the range less `OWN_RANGE_MARGIN`
/// either side. A servo storing the whole turn, 0..=4095, was never
/// calibrated by V1 and keeps today's 0..=4095.
fn own_window((min, max): (u16, u16)) -> (u16, u16) {
    if (min, max) == (0, 4_095) {
        (0, 4_095)
    } else {
        (
            min.saturating_add(OWN_RANGE_MARGIN),
            max.saturating_sub(OWN_RANGE_MARGIN),
        )
    }
}

/// Why the poses this body was given do not fit its joints' own stored
/// limits (#3558), or `None` when they do. `ranges` are by
/// [`Joint::index`]. What the head and the base are asked for (`home`,
/// the pitch window, the stand's pitch, roll and yaw) must be inside the
/// window their goals are held to ([`own_window`]); the arm's standing
/// counts inside the servo's own limits themselves, since its goals keep
/// their own window. A folded arm rests on its mechanical stop, which may
/// lie past the servo's stored limit (lamp-52e6's shoulder, measured
/// folded at 2038 by the factory on 27 Oct, its limit 2047; a hand takes
/// joints 31 to 66 past theirs, #3558), so its folded counts may be up to
/// `Standing::FOUND_SLACK` outside them, as a fold is found.
fn outside_own_ranges(
    ranges: &[(u16, u16); 5],
    home: Option<[u16; 3]>,
    pitch_window: (u16, u16),
    standing: Option<Standing>,
) -> Option<String> {
    let mut asked: Vec<(&str, Joint, u16)> = vec![
        ("pitch window", Joint::WristPitch, pitch_window.0),
        ("pitch window", Joint::WristPitch, pitch_window.1),
    ];
    for (index, joint) in COMMANDED.into_iter().enumerate() {
        asked.extend(home.map(|home| ("home", joint, home[index])));
    }
    if let Some(up) = standing {
        for (index, joint) in ARM.into_iter().enumerate() {
            asked.push(("folded arm", joint, up.folded[index]));
        }
        for (index, joint) in COMMANDED.into_iter().chain(ARM).enumerate() {
            asked.push(("stand", joint, up.pose[index]));
        }
    }
    asked.into_iter().find_map(|(pose, joint, count)| {
        let range = ranges[joint.index()];
        let (low, high) = if pose == "folded arm" {
            let slack = Standing::FOUND_SLACK;
            (range.0.saturating_sub(slack), range.1.saturating_add(slack))
        } else if ARM.contains(&joint) {
            range
        } else {
            own_window(range)
        };
        (!(low..=high).contains(&count)).then(|| {
            let (min, max) = range;
            format!(
                "{pose} {joint} {count} is outside {low}..={high}, its servo's own {min}..={max}"
            )
        })
    })
}

/// A gesture paused at offsets `from`, moving `speed` a step, `held` into
/// its pause (#2602): where it is, eased as the ease out does but to a
/// stop a third of the way it would carry on, so it neither snaps still
/// nor swings back; and how much of its own clip's time that carried it,
/// so that going on takes the clip up from there and not from `from`.
fn held_at(held: Duration, from: [f32; 5], speed: [f32; 5]) -> ([f32; 5], Duration) {
    let span = EASE_OUT.as_secs_f32() / TICK.as_secs_f32();
    let t = (held.as_secs_f32() / EASE_OUT.as_secs_f32()).min(1.0);
    let (carries, arrives) = (t * t * t - 2.0 * t * t + t, -2.0 * t * t * t + 3.0 * t * t);
    // `from·stays + speed·span·carries + (from + speed·span/3)·arrives`,
    // stays and arrives summing to one.
    let along = carries + arrives / 3.0;
    (
        std::array::from_fn(|index| from[index] + speed[index] * span * along),
        EASE_OUT.mul_f32(along),
    )
}
/// While a gesture plays, and for half a second after, a joint of the head
/// may be this far from its goal and read this load before the gesture is
/// dropped; the shoulder and the elbow may be this far from theirs. A quick
/// move lags and reverses hard. That is the move, not something in its way,
/// and what gives is the gesture, never the run.
const GESTURE_TRACKING: u16 = 150;
const GESTURE_LOAD: u16 = 700;
const GESTURE_ARM_TRACKING: u16 = 260;
const GESTURE_COOL_STEPS: u16 = 25;
/// Looks in a row with a face in them, about a second of them, after which
/// somebody is taken to be in front of the lamp: it stands up for them and
/// stays up while they are there.
const COMPANY_SIGHTS: u8 = 6;
/// Under `Limits::look_at_name`: a call is taken as being spoken to for
/// this long, which the voice's own listening takes over if the person
/// goes on; and a call this soon after the last one is let go.
const CALLED_FOR: Duration = Duration::from_secs(2);
const CALLED_EVERY: Duration = Duration::from_secs(10);

/// A gesture asked on the mood socket, not one the body makes of itself:
/// a shift at rest (`crate::idle`, `IDLE` among them) or the turn to a call.
fn from_outside(clip: &Clip) -> bool {
    !idle::is_move(clip) && clip.name != clips::CALLED.name
}

/// What reaches the body from outside in one step, besides the mood: what
/// its eyes saw, and a gesture it was asked to make.
#[derive(Clone, Copy, Debug, Default)]
pub struct Asked {
    pub seen: Option<Seen>,
    pub gesture: Option<&'static Clip>,
    /// What the part that plans what the lamp shows asks now
    /// (`crate::expression_in`, `docs/expression.md`): encoder counts to
    /// add to the pose wanted of the head's pitch, the head's roll and the
    /// base's turn, in that order. `None` when it asks nothing or what it
    /// asked has run out.
    pub expression: Option<[f32; 3]>,
    /// The lamp's name was heard since the last step (`name/heard`), and
    /// was fresh when it came. Taken only under `Limits::look_at_name`.
    pub called: bool,
    /// Stillness is asked: Expression holds the lamp still for someone
    /// working under it, or the lamp is paused. No shift at rest.
    pub still: bool,
    /// Paused by the keeper (`lamp/pause`): a gesture asked is dropped and
    /// said, and nothing else is taken.
    pub paused: bool,
    /// A hand held on the head put the lamp to sleep (`voice/mute` with the
    /// reason `sleep`, `crate::sleep_in`, #1362): it folds now, whoever is
    /// in front of it, and a gesture asked is dropped and said.
    pub asleep: bool,
    /// The voice said, since the last step, that it is open again after a
    /// hand's sleep (a tap on the head, #1362): it stands up at once, the
    /// lift, whatever the voice's own mood says yet.
    pub woken: bool,
    /// With the reading pose a skill holds: the place across the camera's
    /// picture (-1 its left edge, 1 its right) the base turns toward, as it
    /// would to a face seen there (#1679, "light my book"). Only with a
    /// gaze (`--gaze`), which says how far a place in the picture is.
    pub toward: Option<f32>,
    /// A latched unplug request: fold now and accept no new motion.
    pub unplug: bool,
    /// The head's pitch is held by a hand, from a test input
    /// (`lamp-body --hold-from`, off in every product unit), not from the
    /// servo: the hand rules apply as when that joint gave way to a push
    /// (a skill's or idle gesture playing ends `ended: by a hand`, one
    /// waiting or asked is dropped as held), and the joint's goal stays
    /// where it is, without the give-way path, torque or a read judged.
    pub held_by_test: bool,
    /// Held by `lamp-control`'s lease (`lamp-body --lease-socket`, #3504):
    /// a standing lamp stays up while it lasts, as with someone in front of
    /// it. It never stands a folded lamp up by itself.
    pub held: bool,
    /// Where the lease's holder asks the head to look (`lamp_body::lease`,
    /// `{"look":…}`): yaw and pitch, in counts from the pose's own
    /// direction, as the gaze's own look is, already inside the reach the
    /// holder was given. Taken only while `held`, in place of the gaze's.
    pub aim: Option<[f32; 2]>,
}

impl Asked {
    /// What a paused lamp is asked: nothing, and it sees nobody. With no
    /// look at all, a face counted before the pause kept it standing for
    /// good (lamp-52e6, 2026-10-15, still up six minutes into a pause). A
    /// gesture asked while paused is passed on only to be dropped and said.
    pub const PAUSED: Asked = Asked {
        seen: Some(Seen { face: None }),
        gesture: None,
        expression: None,
        called: false,
        still: true,
        paused: true,
        asleep: false,
        woken: false,
        toward: None,
        unplug: false,
        held_by_test: false,
        held: false,
        aim: None,
    };

    /// What a lamp a hand put to sleep is asked (#1362): as paused, it sees
    /// nobody, so a person in front of it does not keep it up, and it is
    /// held still; a gesture asked is passed on only to be dropped and
    /// said. What Expression asks of the head (its sleeping breath) is
    /// added by the caller.
    pub const ASLEEP: Asked = Asked {
        seen: Some(Seen { face: None }),
        gesture: None,
        expression: None,
        called: false,
        still: true,
        paused: false,
        asleep: true,
        woken: false,
        toward: None,
        unplug: false,
        held_by_test: false,
        held: false,
        aim: None,
    };
}

/// The most that may be added to a pose for [`Asked::expression`], in
/// counts, for the head's pitch, its roll and the base's turn: about five,
/// four and six degrees. What is asked is cut to this and then to the
/// reach of each joint like everything else that follows the mood.
pub const EXPRESSION_REACH: [f32; 3] = [60.0, 45.0, 70.0];

/// The same for a lamp at rest under [`Limits::rest_glance`]: a smaller
/// glance of the head (about four and two degrees), and the base never
/// turns.
pub const REST_GLANCE_REACH: [f32; 3] = [50.0, 25.0, 0.0];
/// A head lifted from rest by a glance has its goal this far ahead of where
/// it is asked to go, inside the same bounds: from below, lamp-52e6's head
/// did not start for a goal 30 counts ahead, and lifted for one 40 to 80
/// ahead and stopped about 30 short of it (see [`WAKE_LIFT_LEAD`]).
const GLANCE_LIFT_LEAD: f32 = 40.0;
/// A head led back up from a glance that comes no count closer to its way
/// for this many steps (1 s at `TICK`) is led no more until it is back
/// there: a hand or a stall. Not shorter: lamp-52e6's head creeps back up
/// with pauses longer than 200 ms, and cut after 10 steps it lost its lead
/// for good and rested 15 to 19 counts low (#2818).
const GLANCE_LEAD_STILL_STEPS: u8 = 50;

/// A shift at rest and a drift of Expression's rest layer are one layer:
/// no shift starts this soon after a drift (Expression keeps the same
/// window after a shift, `lamp_expression::BODY_QUIET_US`).
pub const SHIFT_QUIET: Duration = Duration::from_secs(30);
/// A drift is told from the breath under it by the rest offsets' average
/// over about one breath (Expression's is 5.2 s): the breath barely moves
/// it, a drift moves it this many counts or more.
const REST_AVERAGE: Duration = Duration::from_millis(5_200);
const REST_DRIFTED: f32 = 5.0;
/// While a shift plays over Expression's rest offsets, how much of their
/// change since it began (the breath) is kept under it.
const SHIFT_BREATH: f32 = 0.5;

/// The shoulder and the elbow, in the order of every array of two here.
/// Read on every step. Written only by a body that was given a pose to
/// stand in.
pub const ARM: [Joint; 2] = [Joint::BasePitch, Joint::ElbowPitch];

/// Standing up. With this the lamp rises out of its folded pose when someone
/// speaks to it and folds down again once it has been left alone. Without
/// it the shoulder and the elbow are never written.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Standing {
    /// Shoulder and elbow when folded.
    pub folded: [u16; 2],
    /// Head pitch, head roll, base yaw, shoulder and elbow when stood up.
    pub pose: [u16; 5],
    /// How much of the way up it goes: 1 is all of it.
    pub share: f32,
    /// Seconds the whole way takes.
    pub seconds: f32,
    /// It folds down this long after the last mood that was not rest.
    pub awake: Duration,
    /// A shoulder or an elbow reading above this for `Limits::load_steps`
    /// steps sends the lamp back down.
    pub load: [u16; 2],
    /// Standing with nothing to do, it shifts a little now and then.
    pub idle: bool,
    /// Seeds the draw of when it shifts and how (`crate::idle`).
    pub idle_seed: u64,
}

impl Standing {
    /// The farthest the shoulder or the elbow may be asked to travel
    /// between folded and stood up: about 120 degrees.
    pub const TRAVEL_CAP: u16 = 1_400;
    /// A shoulder or an elbow this far from its goal for half a second
    /// sends the lamp back down. Much wider than the head's: these two
    /// carry the lamp. On lamp-4ace the shoulder held 53 counts under its
    /// goal a third of the way up and 65 at six tenths, at loads of 224
    /// and 272, for as long as it stood; the elbow reached its goal.
    pub const TRACKING: u16 = 160;
    pub const TRACKING_STEPS: u8 = 25;
    /// Over its load or off its goal for this long, two seconds, with the
    /// lamp already told to go down: the run ends.
    pub const STUCK_STEPS: u8 = 100;
    /// As warm as this for `Limits::temperature_steps`, likewise sent down.
    pub const TEMPERATURE: u8 = 62;
    /// Sent back down, it stays down this long; after the third time inside
    /// [`Self::RETREAT_WINDOW`] it stays down [`Self::LONG_REST`], and then
    /// it may rise again. Until 24 Oct the third time was for the rest of
    /// the run: lamp-4ace sat down for three strains of the elbow in half an
    /// hour on 20 Oct and never stood again in a five-day run.
    pub const TIRED: Duration = Duration::from_secs(60);
    pub const RETREATS_ALLOWED: u8 = 3;
    pub const RETREAT_WINDOW: Duration = Duration::from_secs(30 * 60);
    pub const LONG_REST: Duration = Duration::from_secs(30 * 60);
    /// To start, each of the two must be found within this of the way
    /// between folded and stood up.
    pub const FOUND_SLACK: u16 = 80;

    /// `share` in percent, `seconds` for the whole way, `awake` in seconds.
    pub fn new(
        folded: [u16; 2],
        pose: [u16; 5],
        share: u8,
        seconds: u8,
        awake: u16,
        load: [u16; 2],
    ) -> Result<Self, &'static str> {
        let far = folded
            .iter()
            .zip(&pose[3..])
            .any(|(from, to)| from.abs_diff(*to) > Self::TRAVEL_CAP);
        if far
            || pose.iter().chain(&folded).any(|count| *count > 4_095)
            // An arm window (`arm_windows`) that would reach past 0 or
            // 4,095 would be cut at the seam: a pose so near it is refused.
            || pose[3..].iter().chain(&folded).any(|count| {
                !(ARM_WINDOW_MARGIN..=4_095 - ARM_WINDOW_MARGIN).contains(count)
            })
            || !(1..=100).contains(&share)
            || !(2..=20).contains(&seconds)
            || !(5..=3_600).contains(&awake)
            || load.iter().any(|bound| !(50..=1_000).contains(bound))
        {
            return Err("a standing pose outside what this body may be given");
        }
        Ok(Self {
            folded,
            pose,
            share: f32::from(share) / 100.0,
            seconds: f32::from(seconds),
            awake: Duration::from_secs(u64::from(awake)),
            load,
            idle: false,
            idle_seed: 0,
        })
    }

    /// The same, shifting a little now and then while it stands at rest, or
    /// not.
    pub fn idling(self, idle: bool) -> Self {
        Self { idle, ..self }
    }

    /// The same, its shifts drawn from this seed.
    pub fn seeded(self, idle_seed: u64) -> Self {
        Self { idle_seed, ..self }
    }
}

/// Ease in and out: no speed at either end of the way.
fn smooth(share: f32) -> f32 {
    let share = share.clamp(0.0, 1.0);
    share * share * (3.0 - 2.0 * share)
}

/// The share that `smooth` turns into this much of the way.
fn unsmooth(lift: f32) -> f32 {
    let (mut low, mut high) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let middle = (low + high) / 2.0;
        if smooth(middle) < lift {
            low = middle;
        } else {
            high = middle;
        }
    }
    (low + high) / 2.0
}

/// How far the head is toward its standing pose with the arm this far up.
/// It waits for the first quarter of the arm's way and is back at its folded
/// pose before the arm comes down that far: the head leaves the base and
/// returns beside it with the arm already carrying it clear.
fn head_share(stand: f32) -> f32 {
    smooth((stand - 0.25) / 0.75)
}

/// Bounds in encoder counts, about 11.4 to a degree. Fixed in code: nothing
/// outside this process can widen them.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Limits {
    /// The head pitch's stall bound (`STALL_LOAD` unless a lamp's own
    /// `--head-stall-load` says more): a head that holds more than 100 to
    /// stand on a lamp of its own make is not pushed by a hand for that.
    pub head_stall_load: u16,
    /// The head's pitch must be found inside this window to start at all.
    pub pitch_home: (u16, u16),
    /// And is never commanded outside this one, whatever the reach allows.
    /// A lamp's own (`--pitch-window`) is checked against the servo's own
    /// limits before anything is written.
    pub pitch_window: (u16, u16),
    /// The head's pitch is never asked to go under this, folded or stood
    /// up, whatever a mood, the eyes and a gesture add up to: the servo's
    /// own limit is just below it and a goal past that is never reached.
    pub pitch_floor: u16,
    /// The farthest a goal may be from the found pose: up, down, roll, yaw.
    pub pitch_up: f32,
    pub pitch_down: f32,
    pub roll: f32,
    pub yaw: f32,
    /// The head's roll reach once the lamp has stood up, where it is more
    /// than `roll`: folded, lamp-4ace's head cannot tilt at all (its roll
    /// servo meets something after four counts); stood up it is free.
    pub stand_roll: f32,
    /// The fastest the commanded pose moves, and how fast that may change.
    pub speed: f32,
    pub acceleration: f32,
    /// The profile written to each commanded servo at start.
    pub servo_speed: u16,
    pub servo_acceleration: u8,
    /// The most force the joints that carry the lamp may push with, of
    /// 1,000 (the servo's register 48), written to each at start and read
    /// back: the base's turn always, the shoulder and the elbow when the
    /// lamp may stand. `None`: nothing is written and each keeps what it
    /// holds, which on lamp-4ace is all of it.
    pub force: Option<u16>,
    /// Write torque on at start to a joint found without it, which this
    /// body would otherwise refuse, its goal first set to where it is: a
    /// servo whose torque went off keeps an old goal, and on lamp-52e6 those
    /// were 18 to 39 counts from where the joints were. Off: torque is
    /// never written to such a joint.
    pub torque_on: bool,
    /// Bring a commanded joint found out of where it may start, up to
    /// [`WAKE_REACH`] from a home given at start, back to that home slowly
    /// before anything else, instead of refusing: see [`wake_pose`]. Off:
    /// such a joint is refused.
    pub wake: bool,
    /// How a joint gives way to a hand, and whether a joint that pushes
    /// without moving is told to stop (see [`GiveWay`]). `None`: as before
    /// this was chosen, exactly.
    pub give_way: Option<GiveWay>,
    /// With `give_way`, meet a hand as V1 did: one still pushed a second
    /// after it stopped makes the head and base let go, the arm still
    /// holding (see `CONTACT_FLOOR`). Off: it
    /// gives way again where it is, its torque on.
    pub limp: bool,
    /// Let go under overload as the old runtime did: any joint at 80% of
    /// its full load (or of its force limit) for 1.5 s switches torque off
    /// on all five for two minutes, after which the lamp is taken up again
    /// slowly. Off: torque is never written off.
    pub overload_release: bool,
    /// The factory's (#1682): a lamp asked to stop that got down, its head
    /// home and its arm folded, is let go as a surprise lets go (torque off
    /// on all five) once its last goals are written, so that it can be read
    /// at rest afterwards. One that did not get down in time keeps holding;
    /// a standing lamp folds first, as it always does. Off: a stop keeps
    /// the lamp held. The product's unit never passes it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub let_go_at_stop: bool,
    /// `--folded-arm` given alone with `let_go_at_stop` (#1682): the
    /// shoulder and elbow counts a person confirmed folded, never this run's
    /// own read of where the arm is. A let-go run found outside its start
    /// window is let go only when its arm is within `LET_GO_NEAR` of them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub let_go_folded_arm: Option<[u16; 2]>,
    /// #2602: a start refused only because a head or base joint is found
    /// out of where it may start (`pose_wait`), on a lamp found folded,
    /// lets go of the joints it owns (torque off, so a hand can set the
    /// lamp straight), says what it asks of a person (`Glimpse::waiting`),
    /// reads them every `WAIT_READ`, and once they have been in range for
    /// `IN_RANGE_READS` reads in a row takes hold where they are and starts
    /// as it would have. Off: refused as before, torque left as it was.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub refused_let_go: bool,
    /// Let a lamp at rest take what Expression asks of the head, within
    /// [`REST_GLANCE_REACH`], and come back to rest once it runs out: a
    /// resting lamp lifts its head at a sudden sound. The arm is not
    /// stirred. Off: a lamp at rest is not stirred by Expression.
    pub rest_glance: bool,
    /// Turn to a call: the lamp's name heard ([`Asked::called`]) while it
    /// does not speak is taken as being spoken to, so that it rises and
    /// looks up as it does to be listened to and the gaze turns it to a
    /// face at once; with no face in view it glances a little to one side
    /// and the other ([`clips::CALLED`]) and lets it go. At most once in
    /// ten seconds. Off: a call is not taken.
    pub look_at_name: bool,
    /// Who may ask for a gesture over whom (#940): a gesture asked while
    /// another plays ends it if asked at a higher [`Priority`], easing out
    /// as a dropped gesture does, and is dropped otherwise, naming who
    /// holds the body; a hand on it drops any. Off: as before, a gesture
    /// asked while another plays is dropped, whoever asks.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub arbitration: bool,
    /// `LAMP_GESTURE_PAUSE=on` (#2602): a skill's or idle gesture playing
    /// as listening begins pauses, eased to a hold, and ends only when the
    /// lamp answers or the hold reaches [`RESUME_WITHIN`]; back to resting
    /// before that, it goes on where it was. Off: it ends the moment a
    /// conversation begins (#2668).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub gesture_pause: bool,
    /// `LAMP_BODY_OWN_RANGES=on` (#3558): each joint's own stored limits
    /// are read at start; a home, folded arm, stand or pitch window outside
    /// them is refused, and a head or base goal outside them (less
    /// `OWN_RANGE_MARGIN`) ends the run on `Stop::Window`, as an arm goal
    /// outside its window does. A joint storing 0..=4095 keeps today's
    /// guard. A bound, never a remap: poses and clips stay shared. Off: the
    /// limits are not read, as before.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub own_ranges: bool,
    /// A load above this on `load_steps` steps in a row makes the joint give
    /// way, and ends the run if it stays or comes back too often.
    pub load: [u16; 3],
    pub load_steps: u8,
    /// A joint found carrying more than this is not started on: at rest it
    /// should be carrying next to nothing, and one that is not is leaning
    /// on something.
    pub start_load: [u16; 3],
    /// A position this far from its goal for `tracking_steps` steps ends it.
    pub tracking: u16,
    pub tracking_steps: u8,
    /// A shoulder or elbow this far from where it was found ends it.
    pub bystander_drift: u16,
    /// A commanded joint this warm, by its own register, on every one of
    /// `temperature_steps` steps in a row ends it. One reading is not heat:
    /// on lamp-4ace the yaw servo read 72 on one step while it moved and 44
    /// to 45 on three reads at rest straight after.
    pub temperature: u8,
    pub temperature_steps: u8,
    /// With a home given at start, the farthest a commanded joint may be
    /// found from it.
    pub start_reach: u16,
}

/// What may be chosen at start, each inside a cap fixed here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Tuning {
    /// Reach in counts: pitch up, pitch down, roll, yaw.
    pub reach: [u16; 4],
    /// Counts a second.
    pub speed: u16,
    pub servo_speed: u16,
    pub servo_acceleration: u8,
}

impl Tuning {
    /// The largest reach that may be chosen: about 16 degrees up, 2 down,
    /// 9 of roll and 22 of yaw.
    pub const REACH_CAP: [u16; 4] = [180, 25, 100, 250];
    pub const SPEED_CAP: u16 = 300;
    pub const SERVO_SPEED_CAP: u16 = 1_000;
    pub const SERVO_ACCELERATION_CAP: u8 = 150;
}

impl Limits {
    /// The folded limits with another reach and speed, each within its cap.
    /// Whatever the reach, the head's pitch keeps its absolute window.
    pub fn folded_with(tuning: Tuning) -> Result<Self, &'static str> {
        let within = tuning
            .reach
            .iter()
            .zip(Tuning::REACH_CAP)
            .all(|(reach, cap)| *reach >= 1 && *reach <= cap);
        if !within
            || !(10..=Tuning::SPEED_CAP).contains(&tuning.speed)
            || !(50..=Tuning::SERVO_SPEED_CAP).contains(&tuning.servo_speed)
            || !(10..=Tuning::SERVO_ACCELERATION_CAP).contains(&tuning.servo_acceleration)
        {
            return Err("reach or speed outside what this body may be given");
        }
        Ok(Self {
            pitch_up: f32::from(tuning.reach[0]),
            pitch_down: f32::from(tuning.reach[1]),
            roll: f32::from(tuning.reach[2]),
            yaw: f32::from(tuning.reach[3]),
            speed: f32::from(tuning.speed),
            // Reaches its speed in a little over a tenth of a second.
            acceleration: f32::from(tuning.speed) * 6.0,
            servo_speed: tuning.servo_speed,
            servo_acceleration: tuning.servo_acceleration,
            // A joint may be found anywhere inside the reach of its home.
            start_reach: tuning.reach.into_iter().max().unwrap_or(60).max(60),
            ..Self::FOLDED
        })
    }

    /// For a lamp folded in its rest pose. On lamp-4ace the head crossed
    /// 1766 to 2043 with no load and met the base near 2045 to 2056, so its
    /// goal stays at or under 2030 and it dips little. Speed is what the head
    /// was moved at when it was backed off the base on 2026-10-12: 47 counts
    /// a second at most, which it followed within 13 counts on the profile
    /// written here, reading a load of up to 116 on the way and 68 at rest.
    pub const FOLDED: Self = Self {
        pitch_home: (1_760, 2_020),
        pitch_window: (1_740, 2_030),
        // lamp-4ace's head pitch servo has its own lower limit at 1,657. A
        // stretch on top of a look away asked it for 1,568 on 2026-10-13
        // and it sat at 1,656 until the stretch was over.
        pitch_floor: 1_670,
        pitch_up: 45.0,
        pitch_down: 15.0,
        roll: 30.0,
        yaw: 40.0,
        stand_roll: 0.0,
        speed: 50.0,
        acceleration: 400.0,
        servo_speed: 150,
        servo_acceleration: 50,
        // In its first second on lamp-4ace the body read 104, 96 and 88.
        // Six steps, 120 ms: a head that reverses quickly reads over the
        // bound for a few steps (217 on the third, once in eleven minutes
        // of talking) and under it again; something in its way does not.
        // The head's bound was 170 and the base's 150 until 2026-10-13.
        // Starting to lift from rest the head read 173 to 203 for longer
        // than that, and the base 208 turning back in the middle of its look
        // away, with nothing in the way of either; a joint that pushes and
        // does not move is now put to rest within half a second whatever
        // it reads, so the bound only has to catch what is pushed hard.
        load: [240, 150, 220],
        head_stall_load: STALL_LOAD,
        load_steps: 6,
        start_load: [170, 150, 150],
        tracking: 30,
        tracking_steps: 10,
        bystander_drift: 6,
        temperature: 62,
        temperature_steps: 50,
        start_reach: 60,
        force: None,
        torque_on: false,
        wake: false,
        give_way: None,
        limp: false,
        overload_release: false,
        let_go_at_stop: false,
        let_go_folded_arm: None,
        refused_let_go: false,
        rest_glance: false,
        look_at_name: false,
        arbitration: false,
        gesture_pause: false,
        own_ranges: false,
    };
}

/// What a commanded joint does after it gave way. With either, a joint that
/// pushes at a goal without moving is told to stop pushing.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GiveWay {
    /// It comes back two seconds later, as without the option.
    Return,
    /// It comes back from a bump; let go under a hand and then left, it
    /// stays where it was put, and later moods are laid relative to that.
    Stay,
}

/// What one joint said on one step.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Sample {
    pub position: u16,
    pub load: u16,
    pub temperature: u8,
    pub status: u8,
}

/// One step as the body saw it, for whoever wants to watch: the mood it
/// acted on, the goals in force and what the three joints read. Pitch, roll,
/// yaw.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Glimpse {
    pub step: u64,
    pub mood: Mood,
    pub goals: [u16; 3],
    pub positions: [u16; 3],
    pub loads: [u16; 3],
    /// What each of the three joints' temperature register reads.
    pub temperatures: [u8; 3],
    /// Times so far that each joint met resistance and was left where it was.
    pub eased: [u16; 3],
    /// Times so far that a hand left each joint where it put it, and that
    /// each joint was told to stop pushing without moving (pitch, roll, yaw,
    /// shoulder, elbow).
    pub kept_where_put: [u16; 3],
    pub stalls: [u16; 5],
    /// Whether this body may stand at all, how far it has stood up, 0
    /// folded to 1, and the shoulder and the elbow: their goals as this
    /// body last wrote them, or else as found.
    pub stands: bool,
    pub stand: f32,
    /// Fresh physical folded evidence, only after this step passed safety checks.
    pub unplug_safe: bool,
    /// Why this step is not folded evidence: an index into
    /// `crate::unplug::UNPLUG_UNSAFE`.
    pub unplug_unsafe: Option<u8>,
    /// A read was missed since the last step: no evidence either way.
    pub unplug_gap: bool,
    pub arm_goals: [u16; 2],
    pub arm_positions: [u16; 2],
    pub arm_loads: [u16; 2],
    pub arm_temperatures: [u8; 2],
    /// Where the eyes have it look: yaw and pitch, in counts from the
    /// pose's own direction.
    pub look: [f32; 2],
    /// While held, the holder's look as the body applied it, after the
    /// joints' own bounds (the pitch window, the base's reach): yaw and
    /// pitch, in counts from the pose's own direction. None otherwise. What
    /// the door says the head reached (`lease::Looking`, `there`).
    pub look_aimed: Option<[f32; 2]>,
    /// A face the eyes reported on this step: across, down and size.
    pub face: Option<[f32; 3]>,
    /// The gesture it is making.
    pub gesture: Option<&'static str>,
    /// Under `--overload release`: times so far it let go, and whether its
    /// torque is off now.
    pub released: u16,
    pub limp: bool,
    /// A head or base joint met resistance and is held where it gave way:
    /// a hand, as far as the body can tell (`body/hand`).
    pub hand: bool,
    /// What V1's anti-tip rule has done so far.
    pub tipping: Tipping,
    /// The gestures asked on the mood socket that were taken or dropped on
    /// this step: each one's name and why, for the journal and
    /// `body/gesture`. Only those asked from outside: never the body's own
    /// shifts at rest or its glance for a call. One refused by V1's
    /// anti-tip rule is `dropped: it would tip`, and said by [`Tipping`] too.
    pub gesture_fates: Fates,
    /// Why a face the eyes report is not looked at now, for a body with
    /// eyes: paused, folded, or neither listening nor speaking. `None`
    /// while one would be.
    pub looks_dropped: Option<&'static str>,
    /// A place a skill asked the reading lamp to turn toward that is on the
    /// side a face is seen on, so not turned to (#1738).
    pub toward_refused: Option<f32>,
    /// Reads so far that each of the five joints gave and were not used
    /// (the exit report's `bad_reads`), and times so far the arm sat down
    /// for a strain (the exit report's `retreats`), for the journal while it
    /// runs.
    pub bad_reads: [u16; 5],
    pub retreats: u16,
    /// Seconds left of the long rest after three strains inside
    /// `Standing::RETREAT_WINDOW`; 0 when it is not resting so. Said on its
    /// health, so a body sat down for half an hour is seen, not only felt
    /// (lamp-4ace folded unseen from 20 to 24 Oct).
    pub resting_s: u16,
    /// Under `Limits::refused_let_go`: the lamp is let go because a joint
    /// is found out of where it may start, and this is what it asks of a
    /// person, in words the voice can say. `None` on every other step.
    pub waiting: Option<&'static str>,
}

/// The gestures asked from outside that were taken or dropped on one step,
/// in order: more than one when, say, one waiting is dropped as another
/// is asked and that one begins in the same step. At most four are kept,
/// each with who it was told of ([`Told`]), which is not serialized.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Fates(
    [Option<(&'static str, &'static str)>; 4],
    #[serde(skip)] [Told; 4],
);

/// Under `Limits::arbitration`, whose a gesture dropped or ended was and
/// who holds the body instead, as their datagrams named them (#940): what
/// `body/gesture` tells the asker that lost. Never what decides who wins.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Told {
    pub source: Option<Source>,
    /// Only a skill or the conversation that named itself: never a hand,
    /// a lamp that does not stand, or a pause (the fifth on #940).
    pub by: Option<Source>,
}

impl Fates {
    /// One more, after those already said this step.
    pub fn push(&mut self, fate: (&'static str, &'static str)) {
        self.push_told(fate, Told::default());
    }

    /// One more, with who it is told of.
    pub fn push_told(&mut self, fate: (&'static str, &'static str), told: Told) {
        if let Some(at) = self.0.iter().position(Option::is_none) {
            self.0[at] = Some(fate);
            self.1[at] = told;
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&'static str, &'static str)> + '_ {
        self.0.iter().flatten().copied()
    }

    /// Each with who it is told of.
    pub fn iter_told(&self) -> impl Iterator<Item = ((&'static str, &'static str), Told)> + '_ {
        self.0
            .iter()
            .zip(self.1)
            .filter_map(|(fate, told)| fate.map(|fate| (fate, told)))
    }
}

impl IntoIterator for Fates {
    type Item = (&'static str, &'static str);
    type IntoIter = std::iter::Flatten<std::array::IntoIter<Option<Self::Item>, 4>>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter().flatten()
    }
}

/// What V1's anti-tip rule ([`crate::cog`]) did in a run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Tipping {
    /// Times steps were cut short so the lamp would not lean back past
    /// its ceiling; a run of such steps counts once.
    pub cuts: u32,
    /// The last such: its ceiling and how far back the step would have
    /// leaned, mm.
    pub ceiling_mm: f32,
    pub would_mm: f32,
    /// Gestures refused before they played, any frame of them over 22 mm
    /// off the base's axis on the pose the lamp was in, and the last one:
    /// its name, the frame and how far, mm.
    pub refused: u32,
    pub last_refused: Option<(&'static str, u16, f32)>,
    /// Gestures played that came within 85% of that, and the last one.
    pub close: u32,
    pub last_close: Option<(&'static str, u16, f32)>,
}

/// Why the run ended. Only `Cancelled` is an ordinary end. Every other end
/// but `Refused` leaves torque off on all five, as V1 let the lamp go (a
/// `Refused` of a lamp found standing does too, once folded).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Stop {
    /// Asked to stop: the head eased back to where it was found.
    Cancelled,
    /// A reason not to start. Nothing was written, except torque off on all
    /// five when a joint was found hot or loaded: as V1, the lamp is let go.
    /// A lamp found standing whose start is refused with nothing written
    /// folds down first and is then let go on it.
    Refused {
        why: String,
    },
    Load {
        joint: u8,
        load: u16,
    },
    Tracking {
        joint: u8,
        goal: u16,
        position: u16,
    },
    Status {
        joint: u8,
        found: u8,
        now: u8,
    },
    Drift {
        joint: u8,
        found: u16,
        now: u16,
    },
    Temperature {
        joint: u8,
        temperature: u8,
    },
    /// The wire was not exactly as expected, on three steps in a row or
    /// while the servo profile was being written.
    Bus {
        detail: String,
    },
    /// An arm goal was refused before it was written (`Bus::send_goals`):
    /// outside its joint's window, or more than a step from the last.
    Window {
        joint: u8,
        goal: u16,
        window: (u16, u16),
        from: u16,
    },
    /// Three positions of one joint in a row were bad reads
    /// (`Bus::judge_positions`): the last, and the last good one.
    BadRead {
        joint: u8,
        position: u16,
        from: u16,
    },
    /// `Limits::temperature_steps` temperatures of one joint in a row were
    /// bad reads that did not settle (`Bus::judge_temperatures`): how warm
    /// it is cannot be read, so it is not known to be cool. The last read,
    /// and the last good one.
    BadTemperature {
        joint: u8,
        temperature: u8,
        from: u8,
    },
    /// Under `--overload release`: it let go a second time within ten
    /// minutes, or could not take the lamp up again. Torque is left as it
    /// was let go, off.
    Overload {
        joint: u8,
        load: u16,
        detail: String,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct BodyReport {
    pub limits: Limits,
    /// Goals were written at least once.
    pub moved: bool,
    /// Pitch, roll, yaw of the pose every gesture is an offset from: the one
    /// given at start, or else the one found.
    pub home: Option<[u16; 3]>,
    pub found: Option<[Sample; 5]>,
    pub steps: u64,
    pub goal_writes: u64,
    pub profile_writes: u8,
    pub missed_reads: u64,
    /// Positions read that were bad (`Bus::judge_positions`), per joint:
    /// yaw, shoulder, elbow, roll, pitch. None was used.
    pub bad_reads: [u16; 5],
    /// Temperatures read that were bad (`Bus::judge_temperatures`), per
    /// joint as `bad_reads`. None was used for a peak or for heat. Apart
    /// from `bad_reads`: the position in the same reply was good.
    pub bad_temperatures: [u16; 5],
    /// Of `bad_temperatures`, the reads that were a repeated flap
    /// (`TEMPERATURE_FLAP`): kept out of the unreadable stop and judged on
    /// the higher of the flap and the good one. Per joint as `bad_reads`.
    pub temperature_flaps: [u16; 5],
    /// A first read that found a joint hot is read again, up to
    /// `FIRST_HEAT_REREADS` times, until two reads in a row agree, before
    /// anything acts on it: the temperatures of each read again, per joint
    /// as `bad_reads`.
    pub hot_rereads: Vec<[u8; 5]>,
    /// Bad temperature values that settled into the good one
    /// (`Bus::judge_temperatures`), the first 16: joint, last good, settled.
    /// A steady 150 that then stops on heat says so here.
    pub temperature_settles: Vec<(u8, u8, u8)>,
    pub peak_load: [u16; 3],
    pub peak_temperature: [u8; 3],
    /// Times each commanded joint met resistance and was left where it was.
    pub eased: [u16; 3],
    /// With `--give-way stay`, times a hand left each commanded joint where
    /// it put it.
    pub kept_where_put: [u16; 3],
    /// With `--give-way`, times each joint (pitch, roll, yaw, shoulder,
    /// elbow) pushed without moving and was told to stop.
    pub stalls: [u16; 5],
    /// With `--limp on`, times each commanded joint was still pushed a
    /// second after it stopped under a hand and the lamp slumped.
    pub released_by_hand: [u16; 3],
    /// With `--wake on`, times each commanded joint stopped a little short
    /// of its goal and was let rest there.
    pub rested_short: [u16; 3],
    /// With `--wake on`, the shoulder found up by itself and lowered onto
    /// its folded count before anything else.
    pub lowered: Option<Woke>,
    /// The goals written to stop pushing, when the run ended on a surprise.
    pub held_at: Option<[u16; 3]>,
    pub last: Option<[Sample; 5]>,
    /// The pose it was given to stand in and still had when it ran.
    pub standing: Option<Standing>,
    /// Why it would not stand, though it ran as a folded lamp.
    pub standing_refused: Option<String>,
    /// With `Limits::own_ranges`, each joint's own stored limits as read at
    /// start, by `Joint::index` (#3558).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub own_ranges: Option<[(u16, u16); 5]>,
    /// Writes of torque enable: to clear a latched overload on the shoulder
    /// or the elbow before standing, and, told to take hold, to each joint
    /// found without torque.
    pub torque_writes: u8,
    /// The joints found without torque that it took hold of.
    pub torque_given: Vec<&'static str>,
    /// What waking did, if it had to.
    pub woke: Option<Woke>,
    /// Writes of the force limit, and the limit every joint it was written
    /// to read back with.
    pub force_writes: u8,
    pub force: Option<u16>,
    /// It left its folded pose at least once.
    pub stood: bool,
    /// Asked to stop, it was let go folded (`Limits::let_go_at_stop`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub let_go_at_stop: bool,
    /// A let-go run found outside its start window (#1682): why, after it
    /// was let go at once, torque off only, with no goal written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub let_go_outside_window: Option<String>,
    /// Under `Limits::refused_let_go`, what it last asked of a person while
    /// it was let go for a joint out of where it may start.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused_let_go: Option<&'static str>,
    /// The start was refused for a pose a hand can set straight: the
    /// ranges to wait for (`run_until_stopped`).
    #[serde(skip)]
    pose_wait: Option<PoseWait>,
    pub arm_peak_load: [u16; 2],
    pub arm_peak_temperature: [u8; 2],
    /// Each time the arm was sent back down, and why.
    pub retreats: Vec<String>,
    /// Gestures begun, and how many of them were let go because a joint
    /// did not keep up or strained.
    pub gestures: u32,
    pub gestures_dropped: u32,
    /// Steps in which something asked by Expression was added to the pose.
    pub expression_steps: u64,
    /// Times it shifted by itself while standing at rest.
    pub idles: u32,
    /// Under `Limits::look_at_name`, calls taken.
    pub calls: u32,
    /// Under `--overload release`, each time it let go.
    pub released: Vec<Released>,
    /// What V1's anti-tip rule did.
    pub tipping: Tipping,
    pub stop: Stop,
}

/// One time it let go under overload: the joint and the load that tripped
/// it, seconds into the run, and how it brought the head home afterwards.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Released {
    pub joint: &'static str,
    pub load: u16,
    pub seconds: f32,
    pub woke: Option<Woke>,
}

/// What the wake did: the joints it moved, where each was found and where
/// it brought it, in counts, and how long that took.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Woke {
    pub joints: Vec<&'static str>,
    pub from: Vec<u16>,
    pub to: Vec<u16>,
    /// Where each was when the wake ended: short of `to` by up to
    /// [`WAKE_SETTLE`] when it settled there.
    pub settled: Vec<u16>,
    pub seconds: f32,
}

/// Where a waking lamp has its three joints `elapsed` seconds into the
/// wake from `from` to `to`, and whether the wake is over. First a stir of
/// up to [`WAKE_STIR`] counts ([`WAKE_LIFT_LEAD`] for a head that lifts),
/// then [`WAKE_PAUSE`], then the rest of the way, each eased at both ends
/// and timed so that the joint with the farthest to go moves at
/// [`WAKE_SPEED`] at most. Every joint only ever moves from where it was
/// found toward where it goes.
fn wake_pose(from: [u16; 3], to: [u16; 3], elapsed: f32) -> ([f32; 3], bool) {
    let way: [f32; 3] = std::array::from_fn(|index| f32::from(to[index]) - f32::from(from[index]));
    let stir: [f32; 3] = std::array::from_fn(|index| {
        let most = if leads(from, to, index) {
            WAKE_LIFT_LEAD
        } else {
            WAKE_STIR
        };
        way[index].clamp(-most, most)
    });
    let rest: [f32; 3] = std::array::from_fn(|index| way[index] - stir[index]);
    // An eased move peaks at one and a half times its mean speed.
    let length =
        |moves: [f32; 3]| 1.5 * moves.iter().fold(0.0f32, |m, d| m.max(d.abs())) / WAKE_SPEED;
    let (first, last) = (length(stir), length(rest));
    let share = |time: f32, length: f32| {
        if length > 0.0 {
            smooth(time / length)
        } else {
            1.0
        }
    };
    let (stirred, risen) = (
        share(elapsed, first),
        share(elapsed - first - WAKE_PAUSE, last),
    );
    let pose = std::array::from_fn(|index| {
        f32::from(from[index]) + stir[index] * stirred + rest[index] * risen
    });
    (pose, elapsed >= first + WAKE_PAUSE + last)
}

/// The head's pitch, going to fewer counts: lifting, against its weight.
fn lifts(from: [u16; 3], to: [u16; 3], index: usize) -> bool {
    index == 0 && to[0] < from[0]
}

/// A joint whose goal must lead it to move at all while it wakes: the head
/// lifting, and the base either way. On lamp-4ace on 2026-10-14 (base gain
/// P 16), woken toward a home 286 counts round, the base turned 72 counts
/// and stuck at 2,101, 34 behind a goal moving at the wake's speed.
fn leads(from: [u16; 3], to: [u16; 3], index: usize) -> bool {
    lifts(from, to, index) || (index == 2 && to[2] != from[2])
}

fn checksum(body: &[u8]) -> u8 {
    !body.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte))
}

/// The only single-servo writes there are: acceleration (register 41, one
/// byte) and goal speed (register 46, two bytes).
fn acceleration_write(joint: Joint, value: u8) -> [u8; 8] {
    let mut packet = [0xff, 0xff, joint.id(), 4, 0x03, 41, value, 0];
    packet[7] = checksum(&packet[2..7]);
    packet
}
fn speed_write(joint: Joint, value: u16) -> [u8; 9] {
    let [low, high] = value.to_le_bytes();
    let mut packet = [0xff, 0xff, joint.id(), 5, 0x03, 46, low, high, 0];
    packet[8] = checksum(&packet[2..8]);
    packet
}
/// The goal position (register 42, two bytes) of one servo. Sent only with
/// its own position: to a shoulder or an elbow found pushing under an old
/// overload, so that it stops, and to a joint it takes hold of.
fn goal_write(joint: Joint, value: u16) -> [u8; 9] {
    let [low, high] = value.to_le_bytes();
    let mut packet = [0xff, 0xff, joint.id(), 5, 0x03, 42, low, high, 0];
    packet[8] = checksum(&packet[2..8]);
    packet
}
/// One sync write of the goal position (register 42, two bytes) of the three
/// commanded joints. A broadcast: no servo answers it.
fn goals_write(goals: [u16; 3]) -> [u8; 17] {
    let mut packet = [0u8; 17];
    packet[..7].copy_from_slice(&[0xff, 0xff, 0xfe, 13, 0x83, 42, 2]);
    for (index, (joint, goal)) in COMMANDED.iter().zip(goals).enumerate() {
        let [low, high] = goal.to_le_bytes();
        packet[7 + index * 3..10 + index * 3].copy_from_slice(&[joint.id(), low, high]);
    }
    packet[16] = checksum(&packet[2..16]);
    packet
}

/// Torque enable (register 40, one byte). Sent only to clear an overload
/// latched on the shoulder or the elbow before this run, and to take hold
/// of a joint found without torque.
fn torque_write(joint: Joint, value: u8) -> [u8; 8] {
    let mut packet = [0xff, 0xff, joint.id(), 4, 0x03, 40, value, 0];
    packet[7] = checksum(&packet[2..7]);
    packet
}
/// The force limit (register 48, two bytes): the most a joint may push
/// with, of 1,000. The servo keeps it until it loses power, and starts
/// again with the limit in its register 16.
fn force_write(joint: Joint, value: u16) -> [u8; 9] {
    let [low, high] = value.to_le_bytes();
    let mut packet = [0xff, 0xff, joint.id(), 5, 0x03, 48, low, high, 0];
    packet[8] = checksum(&packet[2..8]);
    packet
}
/// One sync write of the goal position of all five joints: the three of
/// the head and base, then the shoulder and the elbow.
fn all_goals_write(goals: [u16; 3], arm: [u16; 2]) -> [u8; 23] {
    let mut packet = [0u8; 23];
    packet[..7].copy_from_slice(&[0xff, 0xff, 0xfe, 19, 0x83, 42, 2]);
    let joints = COMMANDED.iter().chain(&ARM);
    for (index, (joint, goal)) in joints.zip(goals.into_iter().chain(arm)).enumerate() {
        let [low, high] = goal.to_le_bytes();
        packet[7 + index * 3..10 + index * 3].copy_from_slice(&[joint.id(), low, high]);
    }
    packet[22] = checksum(&packet[2..22]);
    packet
}

fn nothing_yet(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted | io::ErrorKind::TimedOut
    )
}

/// Each arm goal is kept this far either side of the joint's way between
/// folded and stood up: the most a gesture adds to it (360,
/// `GESTURE_REACH`), and 60 to spare.
const ARM_WINDOW_MARGIN: u16 = 420;
/// The most an arm goal moves from the last one written in a step: a rise
/// moves it a few counts, a gesture 21 (`GESTURE_STEP`), an arm let go by a
/// hand goes back down at this.
const ARM_GOAL_STEP: u16 = 50;
/// How near home, and the arm its folded counts, a lamp asked to stop must
/// be for `Limits::let_go_at_stop` to let it go: inside both the head's
/// tracking bound (30) and the arm's goal step (50), past which the stop is
/// already a surprise, so that an arm a little short of folded (40 counts
/// up, say) keeps holding rather than dropping (the reviewer on #1776).
const LET_GO_NEAR: u16 = 20;

/// The arm is down: shoulder and elbow each within `LET_GO_NEAR` of their
/// folded counts. The one bound every let-go uses (#1682).
fn arm_folded(samples: &[Sample; 5], folded: [u16; 2]) -> bool {
    (0..2).all(|index| samples[ARM[index].index()].position.abs_diff(folded[index]) <= LET_GO_NEAR)
}

/// The most a position may move from the last good reading of its joint,
/// per step (`TICK`) since that reading, unless it is this near the goal
/// last written to the joint. An STS3215 turns at most about 3,000 counts a
/// second unloaded; at 4,096 (a turn a second, faster than any hand or
/// slump moves it) a 20 ms step is 82 counts, and this is near four times
/// that. A reading farther, or outside 0..=4095, is not where the joint is.
const READ_JUMP: u16 = 300;
/// The farthest from the last written goal an arm goal is eased toward
/// (`Bus::eased`); one farther is refused at once. A hold let go jumps by
/// the hold's distance and what a gesture adds, at most 360
/// (`GESTURE_REACH`): over 378 simulated runs (every clip after a shoulder
/// or elbow held by a wall or a hand, lamp-4ace's and lamp-52e6's flags)
/// the largest was 318, `stretching` from a shoulder held at 2609.
const ARM_EASE_REACH: u16 = 400;
/// Bad reads of one joint in a row that end the run on `Stop::BadRead`.
const BAD_READS_IN_ROW: u8 = 3;
/// What `Bus::judge_temperatures` takes as a reading of how warm a servo
/// is. On lamp-52e6 on 2026-10-17, over 632 pairs of its arm's logged
/// standing steps 20 to 30 s apart (readings under 58), none rose more than
/// a degree (3 degrees a minute at most); yet single steps among them read
/// 60 to 150, in checksummed replies whose positions and loads were where
/// the joint was, and never over 150: the servo's own register, not the
/// wire. A reading is bad over `TEMPERATURE_MAX`, or farther from the last
/// good one than `TEMPERATURE_JUMP`: a degree or two of register noise and
/// margin. A real rise moves the good one with it a read at a time.
const TEMPERATURE_MAX: u8 = 100;
const TEMPERATURE_JUMP: u8 = 3;
/// A bad read within this of the last good one, both under
/// `TEMPERATURE_MAX`, is a flap: a joint whose register reads two plausible
/// values by turns. lamp-4ace's shoulder on 2026-10-17 read about 55 and 59
/// to 61 by turns for hours (its settles 4 to 6 apart), and ended on
/// `Stop::BadTemperature` at 15:25 though every read was under 62. A flap
/// that repeats is never counted toward that stop. Once its value repeats
/// (`TEMPERATURE_FLAP_AGREE`, within `TEMPERATURE_SETTLE` and within
/// `TEMPERATURE_FLAP_READS`), the joint is judged on the
/// higher of it and the good one (`Bus::judge_temperatures`), the cautious
/// side for heat; a lone flap is a bad read, as on lamp-voice. 10 takes in
/// twice lamp-4ace's flap and is still far from the
/// glitches of 60 to 150 from under 58 that lamp-52e6 reads.
const TEMPERATURE_FLAP: u8 = 10;
/// Reads within which a flap must repeat to be judged, and for which the
/// good reads after a repeated one are judged on it. A flap that is heat
/// comes every read or two (60 and 64 by turns), lamp-4ace's every 8 or
/// so; lamp-52e6's glitches, one read in 27 at random, seldom come twice
/// within 10 reads and alike, so they never chain into 50 hot steps.
const TEMPERATURE_FLAP_READS: u8 = 10;
/// Flaps in a row that must each agree with the last, within
/// `TEMPERATURE_SETTLE`, for a flap to be kept out of the unreadable stop:
/// three alike. A real flap reads the same value again (lamp-4ace's 59 to
/// 61); noise of 64 to 70 at random has two alike about 2 times in 5 and
/// three about 1 in 7, so it still ends the run within a couple of seconds.
const TEMPERATURE_FLAP_AGREE: u8 = 2;
/// How near the last agreeing one a bad read must be to agree with it: a
/// bad value settles once half of `Limits::temperature_steps` reads agree.
const TEMPERATURE_SETTLE: u8 = 2;
/// Outliers a settle skips without starting again: 25 of 30 reads agreeing
/// settle, so a glitch among them (lamp-52e6's elbow, one read in 27)
/// does not keep a true value from settling after a low first read.
const TEMPERATURE_SETTLE_SKIPS: u8 = 5;
/// Settles kept in the report.
const SETTLES_KEPT: usize = 16;
/// Reads of a first read that found a joint hot, before it is acted on.
const FIRST_HEAT_REREADS: usize = 3;

/// The window each arm goal must be inside, shoulder then elbow, for a body
/// given a pose to stand in: its way between folded and stood up, and
/// `ARM_WINDOW_MARGIN` either side.
fn arm_windows(standing: Option<Standing>) -> [(u16, u16); 2] {
    std::array::from_fn(|index| match standing {
        Some(up) => {
            let (folded, pose) = (up.folded[index], up.pose[3 + index]);
            (
                folded.min(pose).saturating_sub(ARM_WINDOW_MARGIN),
                folded
                    .max(pose)
                    .saturating_add(ARM_WINDOW_MARGIN)
                    .min(4_095),
            )
        }
        None => (0, 4_095),
    })
}

struct Bus<'a, P, R> {
    io: &'a mut P,
    runtime: &'a mut R,
    /// Each arm joint's window (`arm_windows`).
    arm_windows: [(u16, u16); 2],
    /// Each head and base joint's window, by `Joint::index`: 0..=4095, or
    /// with `Limits::own_ranges` its own (`own_window`).
    windows: [(u16, u16); 5],
    /// Each head and base joint's own stored range, by `Joint::index`:
    /// 0..=4095, or with `Limits::own_ranges` the range itself. A hold
    /// where a joint already is (`write_goal`) is checked against it, not
    /// the inset window: it is not motion.
    ranges: [(u16, u16); 5],
    /// An arm goal refused before it was written: the run ends on it.
    refused: Option<Stop>,
    /// When positions were last read, each joint's last good position,
    /// whether that was within `ARM_GOAL_STEP` of the good one before it,
    /// the goal last written to it, its bad reads in all and in a row
    /// (`samples`).
    read_at: Option<Duration>,
    good: [Option<u16>; 5],
    steady: [bool; 5],
    goals: [Option<u16>; 5],
    bad_reads: [u16; 5],
    bad_in_row: [u8; 5],
    /// Each joint's last good temperature and its bad temperature reads
    /// (`judge_temperatures`).
    warm: [Option<u8>; 5],
    bad_temperatures: [u16; 5],
    temperature_flaps: [u16; 5],
    /// Each joint's settling bad value: the last read that agreed, how many
    /// agreed, and how many outliers were skipped.
    settling: [Option<(u8, u8, u8)>; 5],
    /// Each bad value that settled: joint, last good, settled.
    settles: Vec<(u8, u8, u8)>,
    /// `Limits::temperature_steps`: replaced reads in a row that end the
    /// run on `Stop::BadTemperature`. Half of it settles a bad value.
    settle_steps: u8,
    /// Each joint's replaced temperature reads in a row, less one for each
    /// good read.
    bad_warm_in_row: [u8; 5],
    /// Each joint's last flap (`TEMPERATURE_FLAP`): its value, how many
    /// flaps before it in a row each agreed with the next (within
    /// `TEMPERATURE_SETTLE`), and reads since. A repeated flap's value is the floor of the good reads after
    /// it until `TEMPERATURE_FLAP_READS` pass with no flap; a lone one is
    /// forgotten then, so glitches that do not repeat are never heat.
    flap: [Option<(u8, u8, u8)>; 5],
    /// Each joint's temperature reads since its last good one: a flap holds
    /// only within `TEMPERATURE_FLAP_READS` of it. Reads farther from any
    /// good one are bad reads, toward `Stop::BadTemperature`, as on #1270.
    since_good: [u8; 5],
    /// Joints whose temperature this read was replaced: their heat streak
    /// holds for the step, neither adding nor resetting.
    pub(crate) temperature_held: [bool; 5],
    /// Arm joints whose temperature is no longer judged: unreadable, the
    /// arm is folding down to be let go (`run_on`). Each read is held.
    unjudged: [bool; 5],
    /// Joints no goal is written to while the lamp folds: flagged by their
    /// own protection, or found with torque off and a temperature that
    /// cannot be read, which may be the servo's own cut-out. A goal turns
    /// such a servo back on; it is left as it is.
    left: [bool; 5],
}

impl<'a, P: PortIo, R: Runtime> Bus<'a, P, R> {
    fn new(io: &'a mut P, runtime: &'a mut R, standing: Option<Standing>) -> Self {
        Self {
            io,
            runtime,
            arm_windows: arm_windows(standing),
            windows: [(0, 4_095); 5],
            ranges: [(0, 4_095); 5],
            refused: None,
            read_at: None,
            good: [None; 5],
            steady: [true; 5],
            goals: [None; 5],
            bad_reads: [0; 5],
            bad_in_row: [0; 5],
            warm: [None; 5],
            bad_temperatures: [0; 5],
            temperature_flaps: [0; 5],
            settling: [None; 5],
            settles: Vec::new(),
            settle_steps: Limits::FOLDED.temperature_steps,
            bad_warm_in_row: [0; 5],
            flap: [None; 5],
            since_good: [0; 5],
            temperature_held: [false; 5],
            unjudged: [false; 5],
            left: [false; 5],
        }
    }
}

impl<P: PortIo, R: Runtime> Bus<'_, P, R> {
    /// A byte that arrives before a request belongs to nothing sent here.
    fn expect_quiet(&mut self) -> Result<(), String> {
        let mut bytes = [0u8; 64];
        match self.io.read(&mut bytes) {
            Ok(0) => Err("the servo port closed".into()),
            Ok(count) => Err(format!("{count} unexpected bytes before a request")),
            Err(error) if nothing_yet(&error) => Ok(()),
            Err(error) => Err(format!("read: {error}")),
        }
    }

    fn send(&mut self, request: &[u8]) -> Result<(), String> {
        self.expect_quiet()?;
        let deadline = self.runtime.now() + EXCHANGE;
        let mut sent = 0;
        while sent < request.len() {
            match self.io.write(&request[sent..]) {
                Ok(0) => return Err("the servo port accepted nothing".into()),
                Ok(count) => sent += count.min(request.len() - sent),
                Err(error) if nothing_yet(&error) => {
                    let _ = self.io.wait(Interest::Write, POLL);
                }
                Err(error) => return Err(format!("write: {error}")),
            }
            if sent < request.len() && self.runtime.now() >= deadline {
                return Err("the request was not accepted in time".into());
            }
        }
        Ok(())
    }

    /// Send a read and collect one reply from every joint asked.
    fn read(
        &mut self,
        request: &ReadOnlyRequest,
        mut reply: impl FnMut(crate::protocol::MatchedReply) -> Result<(), String>,
    ) -> Result<(), String> {
        self.send(request.bytes())?;
        // A latched flag is information here, not the end of the read: the
        // caller compares every status with the one found at start.
        let mut tracker = request.track_replies_reporting_faults();
        let mut parser = StatusParser::default();
        let deadline = self.runtime.now() + EXCHANGE;
        let mut bytes = [0u8; 64];
        loop {
            match self.io.read(&mut bytes) {
                Ok(0) => return Err("the servo port closed".into()),
                Ok(count) => {
                    let mut failure = None;
                    let fed = parser.feed(&bytes[..count.min(bytes.len())], |event| {
                        if failure.is_some() {
                            return;
                        }
                        match tracker.accept(event) {
                            Ok(matched) => failure = reply(matched).err(),
                            Err(error) => failure = Some(error.to_string()),
                        }
                    });
                    if let Some(failure) = failure {
                        return Err(failure);
                    }
                    if fed.discarded_bytes != 0 {
                        return Err(format!("{} bytes of noise", fed.discarded_bytes));
                    }
                    if tracker.is_complete() {
                        if parser.buffered_bytes() != 0 {
                            return Err("bytes after the last reply".into());
                        }
                        return Ok(());
                    }
                }
                Err(error) if nothing_yet(&error) => {
                    let _ = self.io.wait(Interest::Read, POLL);
                }
                Err(error) => return Err(format!("read: {error}")),
            }
            if self.runtime.now() >= deadline {
                return Err("a reply did not come in time".into());
            }
        }
    }

    fn samples(&mut self) -> Result<[Sample; 5], String> {
        let request = ReadOnlyRequest::sync_read(&Joint::ALL, ReadWindow::TELEMETRY)
            .map_err(|error| error.to_string())?;
        let mut samples = [Sample::default(); 5];
        self.read(&request, |reply| {
            let telemetry = reply.telemetry().map_err(|error| error.to_string())?;
            samples[reply.joint().index()] = Sample {
                position: telemetry.position().raw(),
                load: telemetry.load().magnitude_raw(),
                temperature: telemetry.temperature_raw(),
                // Either place a flag shows: the reply's own byte or the register.
                status: reply.status_flags().bits() | telemetry.status_raw(),
            };
            Ok(())
        })?;
        self.judge_positions(&mut samples)?;
        self.judge_temperatures(&mut samples)?;
        Ok(samples)
    }

    /// Every position read passes here before anything uses it. A bad one
    /// (`READ_JUMP`) is counted and replaced by the joint's last good
    /// position, so it never becomes a goal or a hold; with no good one yet
    /// the read fails. `BAD_READS_IN_ROW` of one joint end the run.
    fn judge_positions(&mut self, samples: &mut [Sample; 5]) -> Result<(), String> {
        let now = self.runtime.now();
        // Since the last read, not the last good one: a bad value that
        // stays is not let in by the time it has stayed.
        let steps = self.read_at.map_or(1, |at| {
            now.saturating_sub(at)
                .as_micros()
                .div_ceil(TICK.as_micros())
                .max(1)
        });
        self.read_at = Some(now);
        let reach = u128::from(READ_JUMP) * steps;
        for (index, sample) in samples.iter_mut().enumerate() {
            let read = sample.position;
            let off = |from: u16| u128::from(read.abs_diff(from)) > reach;
            let bad = read > 4_095
                || self.good[index]
                    .is_some_and(|good| off(good) && self.goals[index].is_none_or(off));
            if !bad {
                self.steady[index] =
                    self.good[index].is_none_or(|good| read.abs_diff(good) <= ARM_GOAL_STEP);
                self.good[index] = Some(read);
                self.bad_in_row[index] = 0;
                continue;
            }
            self.bad_reads[index] = self.bad_reads[index].saturating_add(1);
            self.bad_in_row[index] = self.bad_in_row[index].saturating_add(1);
            let joint = Joint::ALL[index].id();
            let good = self.good[index];
            if self.bad_in_row[index] >= BAD_READS_IN_ROW {
                let stop = Stop::BadRead {
                    joint,
                    position: read,
                    from: good.unwrap_or(read),
                };
                let detail = format!("{stop:?}");
                self.refused.get_or_insert(stop);
                return Err(detail);
            }
            match good {
                Some(good) => sample.position = good,
                None => return Err(format!("joint {joint} read {read}, outside 0..=4095")),
            }
        }
        Ok(())
    }

    /// Every temperature read passes here before a peak or the heat stop
    /// uses it, as positions pass `judge_positions`. A bad one is over
    /// `TEMPERATURE_MAX`, or farther from the joint's last good one than
    /// `TEMPERATURE_JUMP`. It is counted and replaced by the last good one,
    /// so it is never a peak, and marked in `temperature_held` so the heat
    /// streak holds for that step, neither adding nor resetting. A bad value
    /// settles, and becomes the good one, once half of `settle_steps` reads
    /// each agree within `TEMPERATURE_SETTLE` with the last that agreed,
    /// skipping up to `TEMPERATURE_SETTLE_SKIPS` outliers among them: a
    /// joint really at 70 after a low read, one rising fast after it, or a
    /// steady 150, is judged on it again, and each settle is kept in
    /// `settles` for the report. Bad reads in a row, less one for each good
    /// read, end the run on `Stop::BadTemperature` at `settle_steps`, as
    /// three bad positions do: a temperature that cannot be read that long
    /// is not known to be cool, and a joint can warm behind it while its
    /// last good one is cool. With no good one yet (the first read) a
    /// reading is taken as it is (`run_on` reads a hot one again before
    /// acting on it).
    fn judge_temperatures(&mut self, samples: &mut [Sample; 5]) -> Result<(), String> {
        self.temperature_held = [false; 5];
        for (index, sample) in samples.iter_mut().enumerate() {
            if self.unjudged[index] {
                self.temperature_held[index] = true;
                sample.temperature = self.warm[index].unwrap_or(0);
                continue;
            }
            let read = sample.temperature;
            let Some(good) = self.warm[index] else {
                if read > TEMPERATURE_MAX {
                    self.bad_temperatures[index] = self.bad_temperatures[index].saturating_add(1);
                } else {
                    self.warm[index] = Some(read);
                }
                continue;
            };
            // Over the limit is bad unless the good one is too: it settled there.
            let in_range = read <= TEMPERATURE_MAX || good > TEMPERATURE_MAX;
            if in_range && read.abs_diff(good) <= TEMPERATURE_JUMP {
                self.warm[index] = Some(read);
                self.since_good[index] = 0;
                // A good read is one of the settle's skipped outliers, not
                // its end: a value read on 25 of 30 reads, the good one on
                // the rest, still settles. Half and half never does.
                self.settling[index] = match self.settling[index] {
                    Some((last, agreed, skips)) if skips < TEMPERATURE_SETTLE_SKIPS => {
                        Some((last, agreed, skips + 1))
                    }
                    _ => None,
                };
                // One off, not all: one good read in 50 must not keep a
                // joint whose reads are garbage running, while one bad read
                // in 27 still never adds up.
                self.bad_warm_in_row[index] = self.bad_warm_in_row[index].saturating_sub(1);
                self.flap[index] = match self.flap[index] {
                    Some((last, agreed, since))
                        if since.saturating_add(1) < TEMPERATURE_FLAP_READS =>
                    {
                        if agreed >= TEMPERATURE_FLAP_AGREE {
                            sample.temperature = read.max(last);
                        }
                        Some((last, agreed, since + 1))
                    }
                    _ => None,
                };
                continue;
            }
            self.since_good[index] = self.since_good[index].saturating_add(1);
            // A flap only next to its good value: with no good read in the
            // last `TEMPERATURE_FLAP_READS`, the good one is stale and the
            // read is bad, so an unreadable sensor still ends the run.
            let flap = read <= TEMPERATURE_MAX
                && good <= TEMPERATURE_MAX
                && read.abs_diff(good) <= TEMPERATURE_FLAP
                && self.since_good[index] <= TEMPERATURE_FLAP_READS;
            let settling = match self.settling[index] {
                Some((last, agreed, skips)) if read.abs_diff(last) <= TEMPERATURE_SETTLE => {
                    (read, agreed.saturating_add(1), skips)
                }
                Some((last, agreed, skips)) if skips < TEMPERATURE_SETTLE_SKIPS => {
                    (last, agreed, skips + 1)
                }
                _ => (read, 1, 0),
            };
            if settling.0 == read && settling.1 >= (self.settle_steps / 2).max(1) {
                self.warm[index] = Some(read);
                self.settling[index] = None;
                self.bad_warm_in_row[index] = 0;
                // The settled value is the reading now, not a flap.
                self.flap[index] = None;
                self.since_good[index] = 0;
                if self.settles.len() < SETTLES_KEPT {
                    self.settles.push((Joint::ALL[index].id(), good, read));
                }
                continue;
            }
            self.settling[index] = Some(settling);
            self.bad_temperatures[index] = self.bad_temperatures[index].saturating_add(1);
            if flap {
                // Only a flap that repeats the last one is kept out of the
                // unreadable stop: it holds the heat streak as a replaced
                // read does, is judged on the higher of it and the good one,
                // and the good reads after it on it too. A lone flap, noise
                // or a glitch within the window, is a bad read as on
                // lamp-voice, so noise around a good one still ends the run.
                let agreed = match self.flap[index] {
                    Some((last, agreed, _)) if read.abs_diff(last) <= TEMPERATURE_SETTLE => {
                        agreed.saturating_add(1)
                    }
                    _ => 0,
                };
                self.flap[index] = Some((read, agreed, 0));
                if agreed >= TEMPERATURE_FLAP_AGREE {
                    self.temperature_flaps[index] = self.temperature_flaps[index].saturating_add(1);
                    sample.temperature = read.max(good);
                    self.temperature_held[index] = true;
                    continue;
                }
            }
            self.bad_warm_in_row[index] = self.bad_warm_in_row[index].saturating_add(1);
            if self.bad_warm_in_row[index] >= self.settle_steps {
                let stop = Stop::BadTemperature {
                    joint: Joint::ALL[index].id(),
                    temperature: read,
                    from: good,
                };
                let detail = format!("{stop:?}");
                self.refused.get_or_insert(stop);
                return Err(detail);
            }
            self.temperature_held[index] = true;
            sample.temperature = good;
        }
        Ok(())
    }

    /// Forget every joint's good temperature, so the next read is taken as
    /// it is (`run_on`, a hot first read read again).
    fn forget_temperatures(&mut self) {
        self.warm = [None; 5];
        self.settling = [None; 5];
        self.bad_warm_in_row = [0; 5];
        self.flap = [None; 5];
        self.since_good = [0; 5];
    }

    /// Torque, acceleration and goal speed of the commanded joints.
    fn profiles(&mut self) -> Result<[(u8, u8, u16); 3], String> {
        let request = ReadOnlyRequest::sync_read(&COMMANDED, ReadWindow::TORQUE_AND_PROFILE)
            .map_err(|error| error.to_string())?;
        let mut profiles = [(0, 0, 0); 3];
        self.read(&request, |reply| {
            let profile = reply
                .torque_and_profile()
                .map_err(|error| error.to_string())?;
            let index = COMMANDED
                .iter()
                .position(|joint| *joint == reply.joint())
                .ok_or("a reply from a joint that was not asked")?;
            profiles[index] = (
                profile.torque_enable_raw(),
                profile.acceleration_raw(),
                profile.goal_speed_raw(),
            );
            Ok(())
        })?;
        Ok(profiles)
    }

    /// Torque enable of all five, by `Joint::index`.
    fn torques(&mut self) -> Result<[u8; 5], String> {
        let request = ReadOnlyRequest::sync_read(&Joint::ALL, ReadWindow::TORQUE_AND_PROFILE)
            .map_err(|error| error.to_string())?;
        let mut torques = [0; 5];
        self.read(&request, |reply| {
            let profile = reply
                .torque_and_profile()
                .map_err(|error| error.to_string())?;
            torques[reply.joint().index()] = profile.torque_enable_raw();
            Ok(())
        })?;
        Ok(torques)
    }

    /// Torque, acceleration, goal speed and goal of the shoulder and elbow.
    fn arm_profiles(&mut self) -> Result<[(u8, u8, u16, u16); 2], String> {
        let request = ReadOnlyRequest::sync_read(&ARM, ReadWindow::TORQUE_AND_PROFILE)
            .map_err(|error| error.to_string())?;
        let mut profiles = [(0, 0, 0, 0); 2];
        self.read(&request, |reply| {
            let profile = reply
                .torque_and_profile()
                .map_err(|error| error.to_string())?;
            let index = ARM
                .iter()
                .position(|joint| *joint == reply.joint())
                .ok_or("a reply from a joint that was not asked")?;
            profiles[index] = (
                profile.torque_enable_raw(),
                profile.acceleration_raw(),
                profile.goal_speed_raw(),
                profile.goal_position_raw(),
            );
            Ok(())
        })?;
        Ok(profiles)
    }

    /// The servo's own lower and upper position limits (registers 9 and 11).
    fn position_limits(&mut self, joint: Joint) -> Result<(u16, u16), String> {
        let request = ReadOnlyRequest::sync_read(&[joint], ReadWindow::POSITION_LIMITS)
            .map_err(|error| error.to_string())?;
        let mut limits = (0, 0);
        self.read(&request, |reply| {
            let read = reply.position_limits().map_err(|error| error.to_string())?;
            limits = (read.min().raw(), read.max().raw());
            Ok(())
        })?;
        Ok(limits)
    }

    /// The force limit each of `joints` holds, in their order.
    fn force_limits(&mut self, joints: &[Joint]) -> Result<Vec<u16>, String> {
        let window = ReadWindow::new(48, 2).map_err(|error| error.to_string())?;
        let request =
            ReadOnlyRequest::sync_read(joints, window).map_err(|error| error.to_string())?;
        let mut limits = vec![0u16; joints.len()];
        self.read(&request, |reply| {
            let index = joints
                .iter()
                .position(|joint| *joint == reply.joint())
                .ok_or("a reply from a joint that was not asked")?;
            let [low, high] = reply.parameters() else {
                return Err("a force limit that is not two bytes".into());
            };
            limits[index] = u16::from_le_bytes([*low, *high]);
            Ok(())
        })?;
        Ok(limits)
    }

    /// The goals of the head and base, and with them the arm's for a body
    /// that may stand. Every goal is checked first (`goal_refused`).
    fn send_goals(&mut self, goals: [u16; 3], arm: Option<[u16; 2]>) -> Result<(), String> {
        self.write_goals(goals, arm, Self::goal_refused)
    }

    /// The same for a hold: each goal where its joint is, or the one already
    /// written to it (stop pushing, the settle at rest). Nothing moves, so a
    /// head or base goal is checked against its own stored range
    /// (`hold_refused`), as `write_goal`'s is (#3558).
    fn send_holds(&mut self, goals: [u16; 3], arm: Option<[u16; 2]>) -> Result<(), String> {
        self.write_goals(goals, arm, Self::hold_refused)
    }

    fn write_goals(
        &mut self,
        goals: [u16; 3],
        arm: Option<[u16; 2]>,
        refused: fn(&Self, Joint, u16) -> Option<Stop>,
    ) -> Result<(), String> {
        if let Some(stop) = COMMANDED
            .into_iter()
            .zip(goals)
            .chain(
                arm.map(|arm| ARM.into_iter().zip(arm))
                    .into_iter()
                    .flatten(),
            )
            .filter(|(joint, _)| !self.left[joint.index()])
            .find_map(|(joint, goal)| refused(self, joint, goal))
        {
            return Err(self.refuse(stop));
        }
        if self.left.contains(&true) {
            // The same sync write, without the joints left as they are.
            let entries: Vec<(Joint, u16)> = COMMANDED
                .into_iter()
                .zip(goals)
                .chain(
                    arm.map(|arm| ARM.into_iter().zip(arm))
                        .into_iter()
                        .flatten(),
                )
                .filter(|(joint, _)| !self.left[joint.index()])
                .collect();
            let mut packet = vec![0xff, 0xff, 0xfe, (entries.len() * 3 + 4) as u8, 0x83, 42, 2];
            for (joint, goal) in &entries {
                let [low, high] = goal.to_le_bytes();
                packet.extend([joint.id(), low, high]);
            }
            packet.push(checksum(&packet[2..]));
            self.send(&packet)?;
            for (joint, goal) in entries {
                self.goals[joint.index()] = Some(goal);
            }
            return Ok(());
        }
        let Some(arm) = arm else {
            self.send(&goals_write(goals))?;
            self.wrote(goals, None);
            return Ok(());
        };
        self.send(&all_goals_write(goals, arm))?;
        self.wrote(goals, Some(arm));
        Ok(())
    }

    /// One joint's goal by itself, a hold where it already is, written and
    /// acknowledged (with `patiently`, as `write_patiently`). An arm goal is
    /// checked as `send_goals` checks; a head or base hold against its own
    /// stored range (`hold_refused`).
    fn write_goal(&mut self, joint: Joint, goal: u16, patiently: bool) -> Result<(), String> {
        if self.left[joint.index()] {
            return Ok(());
        }
        if let Some(stop) = self.hold_refused(joint, goal) {
            return Err(self.refuse(stop));
        }
        let packet = goal_write(joint, goal);
        if patiently {
            self.write_patiently(joint, &packet)?;
        } else {
            self.write_acknowledged(joint, &packet, true)?;
        }
        self.goals[joint.index()] = Some(goal);
        Ok(())
    }

    /// `goal` for `joint`, last written `from`, as one the guard takes: as
    /// it is if it may be written, else `ARM_GOAL_STEP` from `from` toward it.
    /// A goal outside the joint's window, or more than `ARM_EASE_REACH`
    /// away, is left as it is for the guard to refuse at once: walking
    /// toward it would carry the arm anywhere in its window. So is any goal
    /// of a folded arm at rest (`resting`): it has no hold to let go, and
    /// its way back from a hand is already ramped.
    fn eased(&self, joint: Joint, from: u16, goal: u16, resting: bool) -> u16 {
        let (low, high) = self.window(joint);
        if self.goal_refused(joint, goal).is_none()
            || !(low..=high).contains(&goal)
            || goal.abs_diff(from) > ARM_EASE_REACH
            || resting
        {
            goal
        } else if goal > from {
            from.saturating_add(ARM_GOAL_STEP).min(goal)
        } else {
            from.saturating_sub(ARM_GOAL_STEP).max(goal)
        }
    }

    /// The window a goal for `joint` must be inside: an arm joint's
    /// (`arm_windows`), or a head or base joint's (`windows`).
    fn window(&self, joint: Joint) -> (u16, u16) {
        ARM.iter()
            .position(|arm| *arm == joint)
            .map_or(self.windows[joint.index()], |index| self.arm_windows[index])
    }

    /// A goal refused before it was written: the run ends on the first.
    fn refuse(&mut self, stop: Stop) -> String {
        let detail = format!("{stop:?}");
        self.refused.get_or_insert(stop);
        detail
    }

    /// The goals just written, for `goal_refused` and `judge_positions`.
    fn wrote(&mut self, goals: [u16; 3], arm: Option<[u16; 2]>) {
        for (joint, goal) in COMMANDED.into_iter().zip(goals) {
            self.goals[joint.index()] = Some(goal);
        }
        for (joint, goal) in ARM.into_iter().zip(arm.into_iter().flatten()) {
            self.goals[joint.index()] = Some(goal);
        }
    }

    /// Whether `goal` may be written to `joint`. A head or base goal must be
    /// inside its joint's window (0..=4095 without `LAMP_BODY_OWN_RANGES`),
    /// or, for a joint that lies past its window (last read, else last
    /// written), between where it lies and the window and inside its own
    /// stored range: back toward the window, never farther out (#3504).
    /// Their goals are bounded where they are made, and the head is sent
    /// home in one write at the end. An arm goal must be inside
    /// its joint's window, and either
    /// within `ARM_GOAL_STEP` of the goal last written to it (before one,
    /// of where it was read), or between that goal and where it was last
    /// read, a reading within `ARM_GOAL_STEP` of the one before it: a hold
    /// where it is, as when it gives way to a hand, takes hold or stops
    /// pushing, never asks it past either. If not, the stop the run ends on.
    fn goal_refused(&self, joint: Joint, goal: u16) -> Option<Stop> {
        let arm = ARM.iter().position(|arm| *arm == joint);
        let (low, high) = self.window(joint);
        let read = self.good[joint.index()];
        let from = self.goals[joint.index()].or(read);
        // A head or base joint resting past its window (held there, or
        // brought there by a hand) may be sent back toward it: a goal
        // between where it is and the window, inside its stored range.
        let (low_in, high_in) = match (arm, read.or(self.goals[joint.index()])) {
            (None, Some(at)) => (low.min(at), high.max(at)),
            _ => (low, high),
        };
        let (min, max) = self.ranges[joint.index()];
        let inside = (low..=high).contains(&goal)
            || ((low_in..=high_in).contains(&goal) && (min..=max).contains(&goal));
        let allowed = inside
            && (arm.is_none()
                || from.is_none_or(|from| goal.abs_diff(from) <= ARM_GOAL_STEP)
                || read.is_some_and(|at| {
                    let from = from.unwrap_or(at);
                    self.steady[joint.index()] && (from.min(at)..=from.max(at)).contains(&goal)
                }));
        (!allowed).then(|| Stop::Window {
            joint: joint.id(),
            goal,
            window: (low, high),
            from: from.unwrap_or(goal),
        })
    }

    /// Whether a hold where `joint` already is may be written: an arm hold
    /// as `goal_refused`; a head or base hold if inside the joint's own
    /// stored range (`ranges`). A joint resting within `OWN_RANGE_MARGIN`
    /// of a stop still takes hold there (#3558): holding is not motion, and
    /// a clamped hold would move it.
    fn hold_refused(&self, joint: Joint, goal: u16) -> Option<Stop> {
        if ARM.contains(&joint) {
            return self.goal_refused(joint, goal);
        }
        let (low, high) = self.ranges[joint.index()];
        (!(low..=high).contains(&goal)).then(|| Stop::Window {
            joint: joint.id(),
            goal,
            window: (low, high),
            from: self.goals[joint.index()]
                .or(self.good[joint.index()])
                .unwrap_or(goal),
        })
    }

    /// One register write to one servo and its plain acknowledgement, which
    /// has the form of a ping's.
    fn write_checked(&mut self, joint: Joint, packet: &[u8]) -> Result<(), String> {
        self.write_acknowledged(joint, packet, false)
    }

    /// The same, and with `flagged` an acknowledgement that carries a
    /// status flag is still one: the write that is meant to clear a latched
    /// flag is answered with it.
    fn write_acknowledged(
        &mut self,
        joint: Joint,
        packet: &[u8],
        flagged: bool,
    ) -> Result<(), String> {
        if packet == torque_write(joint, 1) && self.runtime.recovery_inhibited() {
            return Err("unplug preparation inhibits torque-on".into());
        }
        self.send(packet)?;
        let ping = ReadOnlyRequest::ping(joint);
        let mut tracker = if flagged {
            ping.track_replies_reporting_faults()
        } else {
            ping.track_replies()
        };
        let mut parser = StatusParser::default();
        let deadline = self.runtime.now() + EXCHANGE;
        let mut bytes = [0u8; 16];
        loop {
            match self.io.read(&mut bytes) {
                Ok(0) => return Err("the servo port closed".into()),
                Ok(count) => {
                    let mut failure = None;
                    parser.feed(&bytes[..count.min(bytes.len())], |event| {
                        if let Err(error) = tracker.accept(event) {
                            failure.get_or_insert(error.to_string());
                        };
                    });
                    if let Some(failure) = failure {
                        return Err(failure);
                    }
                    if tracker.is_complete() {
                        return Ok(());
                    }
                }
                Err(error) if nothing_yet(&error) => {
                    let _ = self.io.wait(Interest::Read, POLL);
                }
                Err(error) => return Err(format!("read: {error}")),
            }
            if self.runtime.now() >= deadline {
                return Err(format!("{joint} did not acknowledge a write"));
            }
        }
    }

    /// Wait `pause`, throwing away whatever a late reply leaves on the wire.
    fn settle(&mut self, pause: Duration) {
        let due = self.runtime.now() + pause;
        let mut bytes = [0u8; 64];
        while self.runtime.now() < due {
            if !matches!(self.io.read(&mut bytes), Ok(count) if count > 0) {
                let _ = self.io.wait(Interest::Read, POLL);
            }
        }
    }

    /// A write acknowledged as in `write_acknowledged`, tried again after
    /// `TAKE_HOLD_RETRY` while it fails, `TAKE_HOLD_TRIES` times in all.
    fn write_patiently(&mut self, joint: Joint, packet: &[u8]) -> Result<(), String> {
        let mut tries = 1;
        loop {
            // Patient writes belong only to take-hold. Do not retry a goal
            // or torque-on after unplug intent arrived during an ACK wait.
            if self.runtime.recovery_inhibited() {
                return Err("unplug preparation inhibits automatic take-hold".into());
            }
            match self.write_acknowledged(joint, packet, true) {
                Err(_) if tries < TAKE_HOLD_TRIES => {
                    tries += 1;
                    self.settle(TAKE_HOLD_RETRY);
                }
                done => return done,
            }
        }
    }

    fn sleep_until(&mut self, due: Duration) {
        loop {
            let now = self.runtime.now();
            if now >= due {
                return;
            }
            let _ = self.io.wait(Interest::Read, (due - now).min(POLL * 4));
        }
    }
}

/// Who watches the steps of a wake, and the step they are shown as.
type Watching<'a> = (&'a mut dyn FnMut(&Glimpse), Glimpse);

/// Bring the three joints from `from` to `to` along [`wake_pose`], the arm
/// held at `arm` for a body that may stand: the head's pitch and roll
/// first, and the base only once they have arrived or settled. Every step it reads all five
/// joints: a status byte that changes, a shoulder or an elbow that moves, a
/// commanded joint over its load or off its goal for as many steps as in
/// the run, or a wire that fails for half a second ends it. The wake goes
/// on only on steps whose write and read both went through, so a missed
/// step never makes the next goal jump. Returns what the joints read at
/// the end, and what it did. With `watching`, each step read is a step of
/// the run and is watched, as a body that is already running must be.
fn wake<P: PortIo, R: Runtime>(
    bus: &mut Bus<'_, P, R>,
    report: &mut BodyReport,
    limits: &Limits,
    (from, to): ([u16; 3], [u16; 3]),
    arm: Option<[u16; 2]>,
    found: [Sample; 5],
    mut watching: Option<Watching<'_>>,
) -> Result<[Sample; 5], Stop> {
    let started = bus.runtime.now();
    let (mut ticks, mut done) = (0u32, 0u32);
    let mut written = from;
    // The head first, the base only once the head has come up: a head
    // slumped onto its own base is dragged across it by a base that turns
    // with it. On lamp-52e6 on 2026-10-16 the base, turning with a head
    // lifting off it, read a load of 224 and stalled.
    let head_first = [to[0], to[1], from[2]];
    let mut leg = if head_first == from {
        (from, to)
    } else {
        (from, head_first)
    };
    let (mut misses, mut over, mut off) = (0u8, [0u8; 3], [0u8; 3]);
    // How near each joint has come to its goal since the goal got home, the
    // steps since it last came nearer, and whether it has settled short.
    let (mut closest, mut still, mut settled) = ([u16::MAX; 3], [0u8; 3], [false; 3]);
    loop {
        ticks += 1;
        let due = started + TICK * ticks;
        if bus.runtime.recovery_inhibited() || !matches!(bus.runtime.cancelled(), Ok(false)) {
            report.woke = Some(woke_from(
                (from, to, written),
                report.last,
                bus.runtime.now().saturating_sub(started),
            ));
            return Err(Stop::Cancelled);
        }
        let (pose, over_now) = wake_pose(leg.0, leg.1, TICK.as_secs_f32() * done as f32);
        let goals = pose.map(|goal| goal.round() as u16);
        let outcome = if goals == written {
            Ok(())
        } else {
            bus.send_goals(goals, arm).map(|()| {
                written = goals;
                report.goal_writes += 1;
                report.moved = true;
            })
        };
        match outcome.and_then(|()| bus.samples()) {
            Err(detail) => {
                report.missed_reads += 1;
                misses += 1;
                if misses >= WIRE_STEPS || bus.refused.is_some() {
                    report.woke = Some(woke_from(
                        (from, to, written),
                        report.last,
                        bus.runtime.now().saturating_sub(started),
                    ));
                    return Err(bus.refused.take().unwrap_or(Stop::Bus {
                        detail: format!("waking: {detail}"),
                    }));
                }
            }
            Ok(now) => {
                misses = 0;
                report.last = Some(now);
                let mut surprise = None;
                if let Some((watch, glimpse)) = watching.as_mut() {
                    report.steps += 1;
                    watch(&glimpse_of(*glimpse, report.steps, &now, written));
                }
                for joint in Joint::ALL {
                    let (was, is) = (found[joint.index()], now[joint.index()]);
                    if is.status != was.status {
                        surprise.get_or_insert(Stop::Status {
                            joint: joint.id(),
                            found: was.status,
                            now: is.status,
                        });
                    }
                }
                // The wake never moves the arm, though it sways as the base
                // turns.
                for joint in ARM {
                    let (was, is) = (found[joint.index()], now[joint.index()]);
                    if was.position.abs_diff(is.position) > limits.bystander_drift.max(WAKE_SWAY) {
                        surprise.get_or_insert(Stop::Drift {
                            joint: joint.id(),
                            found: was.position,
                            now: is.position,
                        });
                    }
                }
                for (index, joint) in COMMANDED.iter().enumerate() {
                    let is = now[joint.index()];
                    report.peak_load[index] = report.peak_load[index].max(is.load);
                    let bound = if lifts(from, to, index) {
                        limits.load[index].max(WAKE_LIFT_LOAD)
                    } else {
                        limits.load[index]
                    };
                    over[index] = if is.load > bound {
                        over[index].saturating_add(1)
                    } else {
                        0
                    };
                    // A head lifting against its weight lags its goal: on
                    // the way it may lag up to WAKE_TRACKING. Once the goal
                    // is home, it must come within the run's own bound, or
                    // at least keep moving toward it.
                    let apart = written[index].abs_diff(is.position);
                    off[index] = if apart > WAKE_TRACKING {
                        off[index].saturating_add(1)
                    } else {
                        0
                    };
                    let nearer = apart + WAKE_MOVED <= closest[index];
                    if nearer || !over_now {
                        closest[index] = apart;
                        still[index] = 0;
                    } else if apart > limits.tracking {
                        still[index] = still[index].saturating_add(1);
                    }
                    // Stopped short, but near enough home: woken there.
                    if still[index] >= WAKE_STILL_STEPS && apart <= WAKE_SETTLE {
                        settled[index] = true;
                        still[index] = 0;
                    }
                    let stop = if over[index] >= limits.load_steps.max(1) {
                        Some(Stop::Load {
                            joint: joint.id(),
                            load: is.load,
                        })
                    } else if off[index] >= limits.tracking_steps
                        || still[index] >= WAKE_STILL_STEPS
                    {
                        Some(Stop::Tracking {
                            joint: joint.id(),
                            goal: written[index],
                            position: is.position,
                        })
                    } else {
                        None
                    };
                    surprise = surprise.or(stop);
                }
                let arrived = (0..3).all(|index| {
                    settled[index]
                        || written[index].abs_diff(now[COMMANDED[index].index()].position)
                            <= limits.tracking
                });
                if surprise.is_none() && over_now && arrived && leg.1 != to {
                    // The head is up: now the base.
                    leg = (head_first, to);
                    done = 0;
                } else if surprise.is_some() || (over_now && arrived) {
                    report.woke = Some(woke_from(
                        (from, to, written),
                        report.last,
                        bus.runtime.now().saturating_sub(started),
                    ));
                    return surprise.map_or(Ok(now), Err);
                }
                if !over_now {
                    done += 1;
                }
            }
        }
        bus.sleep_until(due);
    }
}

/// Lower a shoulder found up with its elbow folded onto the folded
/// shoulder count, eased at both ends at [`ARM_LOWER_SPEED`] at most, the
/// head, the base and the elbow held at `head` and `elbow`. Every step it
/// reads all five joints: a changed status byte; the head, the base or
/// the elbow over its load bound (`Limits::load`, the elbow's standing
/// load) for `Limits::load_steps` steps, or more than `Limits::tracking`
/// from where it was found for `Limits::tracking_steps` reads in a row (a
/// single read off is taken as where it was found); the shoulder over its
/// load for `Limits::load_steps` steps or farther than
/// `Standing::TRACKING` from its goal for `Standing::TRACKING_STEPS`, or
/// a wire that fails for half a second ends it. Once its goal is folded
/// it waits for the shoulder to come within `Standing::FOUND_SLACK`; one
/// that stops farther away for half a second ends it too. With `watching`,
/// as in [`wake`], each step read is a step of the run and is watched: a
/// body already running tells systemd it is alive every step, and this
/// takes some 22 s, over `WatchdogSec=5`.
#[allow(clippy::too_many_arguments)]
fn lower_shoulder<P: PortIo, R: Runtime>(
    bus: &mut Bus<'_, P, R>,
    report: &mut BodyReport,
    limits: &Limits,
    up: Standing,
    head: [u16; 3],
    elbow: u16,
    found: [Sample; 5],
    mut watching: Option<Watching<'_>>,
) -> Result<[Sample; 5], Stop> {
    let shoulder = Joint::BasePitch;
    let (from, to) = (found[shoulder.index()].position, up.folded[0]);
    let way = f32::from(to) - f32::from(from);
    let length = 1.5 * way.abs() / ARM_LOWER_SPEED;
    let started = bus.runtime.now();
    let (mut ticks, mut done, mut misses) = (0u32, 0u32, 0u8);
    let (mut written, mut over, mut off, mut still) = (from, 0u8, 0u8, 0u8);
    // The held joints, judged as the run judges them: steps in a row over
    // their load bound, and off where they were found by more than
    // `limits.tracking`.
    let (mut held_over, mut drifted) = ([0u8; 5], [0u8; 5]);
    let mut closest = u16::MAX;
    let outcome = loop {
        ticks += 1;
        let due = started + TICK * ticks;
        if !matches!(bus.runtime.cancelled(), Ok(false)) {
            break Err(Stop::Cancelled);
        }
        let share = smooth(TICK.as_secs_f32() * done as f32 / length.max(TICK.as_secs_f32()));
        let goal = (f32::from(from) + way * share).round() as u16;
        let sent = if goal == written && done > 0 {
            Ok(())
        } else {
            bus.send_goals(head, Some([goal, elbow])).map(|()| {
                written = goal;
                report.goal_writes += 1;
                report.moved = true;
            })
        };
        let now = match sent.and_then(|()| bus.samples()) {
            Ok(now) => now,
            Err(detail) => {
                report.missed_reads += 1;
                misses += 1;
                if misses >= WIRE_STEPS || bus.refused.is_some() {
                    break Err(bus.refused.take().unwrap_or(Stop::Bus {
                        detail: format!("lowering the shoulder: {detail}"),
                    }));
                }
                bus.sleep_until(due);
                continue;
            }
        };
        misses = 0;
        let mut now = now;
        let mut surprise = None;
        for joint in Joint::ALL {
            let (was, is) = (found[joint.index()], now[joint.index()]);
            if is.status != was.status {
                surprise.get_or_insert(Stop::Status {
                    joint: joint.id(),
                    found: was.status,
                    now: is.status,
                });
            }
            if joint == shoulder {
                continue;
            }
            // Its load, kept with the run's peaks, so a run shows how near
            // the held joints came to their bounds while the shoulder came
            // down (the reviewer on #1115).
            match COMMANDED.iter().position(|held| *held == joint) {
                Some(at) => report.peak_load[at] = report.peak_load[at].max(is.load),
                None => report.arm_peak_load[1] = report.arm_peak_load[1].max(is.load),
            }
            // A hand pressing a held joint without moving it, as the run
            // takes one: over its bound for `load_steps` steps.
            let bound = COMMANDED
                .iter()
                .position(|held| *held == joint)
                .map_or(up.load[1], |index| limits.load[index]);
            let index = joint.index();
            held_over[index] = if is.load > bound {
                held_over[index].saturating_add(1)
            } else {
                0
            };
            if held_over[index] >= limits.load_steps.max(1) {
                surprise.get_or_insert(Stop::Load {
                    joint: joint.id(),
                    load: is.load,
                });
            }
            // Moved, for `tracking_steps` reads in a row: one read off is
            // not a hand, and is taken as where it was found, so a hold
            // never asks for it.
            if was.position.abs_diff(is.position) > limits.tracking {
                drifted[index] = drifted[index].saturating_add(1);
                if drifted[index] >= limits.tracking_steps.max(1) {
                    surprise.get_or_insert(Stop::Drift {
                        joint: joint.id(),
                        found: was.position,
                        now: is.position,
                    });
                } else {
                    now[index].position = was.position;
                }
            } else {
                drifted[index] = 0;
            }
        }
        report.last = Some(now);
        if let Some((watch, glimpse)) = watching.as_mut() {
            report.steps += 1;
            let template = Glimpse {
                arm_goals: [written, elbow],
                ..*glimpse
            };
            watch(&glimpse_of(template, report.steps, &now, head));
        }
        let is = now[shoulder.index()];
        report.arm_peak_load[0] = report.arm_peak_load[0].max(is.load);
        over = if is.load > up.load[0] { over + 1 } else { 0 };
        let apart = written.abs_diff(is.position);
        off = if apart > Standing::TRACKING {
            off + 1
        } else {
            0
        };
        let over_now = share >= 1.0;
        if apart + WAKE_MOVED <= closest || !over_now {
            (closest, still) = (apart, 0);
        } else {
            still += 1;
        }
        if over >= limits.load_steps.max(1) {
            surprise.get_or_insert(Stop::Load {
                joint: shoulder.id(),
                load: is.load,
            });
        } else if off >= Standing::TRACKING_STEPS
            || (still >= WAKE_STILL_STEPS && apart > Standing::FOUND_SLACK)
        {
            surprise.get_or_insert(Stop::Tracking {
                joint: shoulder.id(),
                goal: written,
                position: is.position,
            });
        }
        if let Some(surprise) = surprise {
            break Err(surprise);
        }
        if over_now && apart <= Standing::FOUND_SLACK {
            break Ok(now);
        }
        if !over_now {
            done += 1;
        }
        bus.sleep_until(due);
    };
    report.lowered = Some(Woke {
        joints: vec![shoulder.name()],
        from: vec![from],
        to: vec![written],
        settled: report
            .last
            .map_or_else(Vec::new, |last| vec![last[shoulder.index()].position]),
        seconds: bus.runtime.now().saturating_sub(started).as_secs_f32(),
    });
    outcome
}

/// With `--wake on`, one step of a commanded joint that may be held short
/// of its goal by its own weight: farther than `Limits::tracking` from
/// `goal` but within [`WAKE_SETTLE`], under its load bound (`over` is the
/// steps in a row it has read over it), the goal unchanged and the joint
/// not moving [`WAKE_MOVED`] counts. `steps` counts the steps it has been
/// so, from 1, and is 0 while it is not short in this way; after more than
/// [`WAKE_STILL_STEPS`] it rests where it is (true). On lamp-52e6 woken
/// with its shoulder up, the head held 30 to 37 counts short of goals of
/// 1,759 and 1,775 and the run ended on tracking three times in a row.
fn rests_short(
    limits: &Limits,
    (goal, position, over): (u16, u16, u8),
    at: &mut (u16, u16),
    steps: &mut u8,
) -> bool {
    let apart = goal.abs_diff(position);
    if !limits.wake || apart <= limits.tracking || apart > WAKE_SETTLE || over > 0 {
        *steps = 0;
        return false;
    }
    if *steps == 0 || at.0 != goal || at.1.abs_diff(position) >= WAKE_MOVED {
        (*at, *steps) = ((goal, position), 1);
        return false;
    }
    *steps = steps.saturating_add(1);
    if *steps > WAKE_STILL_STEPS {
        *steps = 0;
        return true;
    }
    false
}

/// Where the head and the base are at home with the arm `stand` of its way
/// up: the folded `home`, carried toward the standing pose with the head.
fn home_with(home: [u16; 3], standing: Option<Standing>, stand: f32) -> [f32; 3] {
    let folded = home.map(f32::from);
    match standing {
        Some(up) => {
            let share = head_share(stand);
            std::array::from_fn(|index| {
                folded[index] + (f32::from(up.pose[index]) - folded[index]) * share
            })
        }
        None => folded,
    }
}

/// What the wake did, as far as it got: the joints it set out to move,
/// where each was found, where its goal had got to and where it was last
/// read.
fn woke_from(
    (from, to, written): ([u16; 3], [u16; 3], [u16; 3]),
    last: Option<[Sample; 5]>,
    took: Duration,
) -> Woke {
    let moved: Vec<usize> = (0..3).filter(|index| from[*index] != to[*index]).collect();
    Woke {
        joints: moved.iter().map(|index| COMMANDED[*index].name()).collect(),
        from: moved.iter().map(|index| from[*index]).collect(),
        to: moved.iter().map(|index| written[*index]).collect(),
        settled: last.map_or_else(Vec::new, |last| {
            moved
                .iter()
                .map(|index| last[COMMANDED[*index].index()].position)
                .collect()
        }),
        seconds: took.as_secs_f32(),
    }
}

/// A step to watch that is not one of the run's own: `template` with what
/// the joints read now and the goals of the head and base in force.
fn glimpse_of(template: Glimpse, step: u64, now: &[Sample; 5], goals: [u16; 3]) -> Glimpse {
    Glimpse {
        step,
        unplug_safe: false,
        unplug_unsafe: None,
        unplug_gap: false,
        mood: Mood::Resting,
        goals,
        positions: COMMANDED.map(|joint| now[joint.index()].position),
        loads: COMMANDED.map(|joint| now[joint.index()].load),
        temperatures: COMMANDED.map(|joint| now[joint.index()].temperature),
        arm_positions: ARM.map(|joint| now[joint.index()].position),
        arm_loads: ARM.map(|joint| now[joint.index()].load),
        arm_temperatures: ARM.map(|joint| now[joint.index()].temperature),
        look: [0.0; 2],
        look_aimed: None,
        face: None,
        gesture: None,
        gesture_fates: Fates::default(),
        ..template
    }
}

/// Take hold of each of `joints`, found without torque: its goal is
/// written as where it was `found`, then torque on, each acknowledged. A
/// servo whose torque went off keeps whatever goal it had, and torque on
/// alone would send it there. About 100 ms later all five are read; a
/// joint that moved more than `TAKE_HOLD_SLACK` counts is given where it
/// went as its goal, and that is a reason not to go on (`Stop::Refused`).
fn take_hold<P: PortIo, R: Runtime>(
    bus: &mut Bus<'_, P, R>,
    report: &mut BodyReport,
    joints: &[Joint],
    found: &[Sample; 5],
) -> Result<[Sample; 5], Stop> {
    let mut outcome = Ok(());
    for joint in joints {
        if bus.runtime.recovery_inhibited() {
            return Err(Stop::Cancelled);
        }
        let at = found[joint.index()].position;
        // A flag is read and judged with the rest afterwards.
        if outcome.is_ok() {
            outcome = bus.write_goal(*joint, at, true);
        }
        if bus.runtime.recovery_inhibited() {
            return Err(Stop::Cancelled);
        }
        if outcome.is_ok() {
            report.torque_writes = report.torque_writes.saturating_add(1);
            report.torque_given.push(joint.name());
            outcome = bus.write_patiently(*joint, &torque_write(*joint, 1));
        }
    }
    if bus.runtime.recovery_inhibited() {
        return Err(Stop::Cancelled);
    }
    // A few steps for a goal that was not taken to show.
    let now = match outcome.and_then(|()| {
        let due = bus.runtime.now() + TICK * 5;
        bus.sleep_until(due);
        bus.samples()
    }) {
        Ok(now) => now,
        Err(detail) => {
            return Err(bus.refused.take().unwrap_or(Stop::Bus {
                detail: format!("taking hold: {detail}"),
            }));
        }
    };
    report.last = Some(now);
    let moved: Vec<(Joint, u16)> = joints
        .iter()
        .map(|joint| {
            let index = joint.index();
            (*joint, now[index].position.abs_diff(found[index].position))
        })
        .filter(|(_, by)| *by > TAKE_HOLD_SLACK)
        .collect();
    if let Some((joint, by)) = moved.first() {
        // Nothing keeps pushing a joint that went somewhere.
        for (joint, _) in &moved {
            let at = now[joint.index()].position;
            let _ = bus.write_goal(*joint, at, false);
        }
        return Err(Stop::Refused {
            why: format!("{joint} moved {by} counts when its torque came on"),
        });
    }
    Ok(now)
}

/// Whether a shoulder and an elbow `found` so are to be lowered before the
/// lamp may stand: off the way between folded and stood up, the shoulder
/// up and the elbow folded, as a power cut leaves a standing lamp and as
/// it slumps when let go (on lamp-52e6 on 2026-10-16 the elbow fell from
/// 2,595 to 2,080 in the rest and the shoulder stayed at 2,650), both with
/// torque on (`profiles`), a clear status and not hot.
fn shoulder_to_lower(up: Standing, found: &[Sample; 5], profiles: [(u8, u8, u16, u16); 2]) -> bool {
    let slack = Standing::FOUND_SLACK;
    let [shoulder, elbow] = ARM.map(|joint| found[joint.index()]);
    let (_, offs) = on_the_way(up, [shoulder.position, elbow.position]);
    offs.iter().any(|off| *off > f32::from(slack))
        && shoulder.position.abs_diff(up.folded[0]) > slack
        && elbow.position.abs_diff(up.folded[1]) <= slack
        && [shoulder, elbow]
            .iter()
            .zip(profiles)
            .all(|(sample, profile)| {
                profile.0 == 1 && sample.status == 0 && sample.temperature < Standing::TEMPERATURE
            })
}

/// Where a shoulder and an elbow at `arm` are on the way between folded
/// and stood up: how far up, 0 to 1, by the one that travels farther, and
/// how far each is off that point of the way, in counts.
fn on_the_way(up: Standing, arm: [u16; 2]) -> (f32, [f32; 2]) {
    let way = |index: usize| (f32::from(up.folded[index]), f32::from(up.pose[3 + index]));
    let leader = usize::from(up.folded[1].abs_diff(up.pose[4]) > up.folded[0].abs_diff(up.pose[3]));
    let (from, to) = way(leader);
    let lift = if (to - from).abs() < 1.0 {
        0.0
    } else {
        ((f32::from(arm[leader]) - from) / (to - from)).clamp(0.0, 1.0)
    };
    let off = [0, 1].map(|index| {
        let (from, to) = way(index);
        (f32::from(arm[index]) - (from + (to - from) * lift)).abs()
    });
    (lift, off)
}

/// Whether a lamp that may stand was found up on its way: a twentieth of
/// the way or more, both arm joints within `Standing::FOUND_SLACK` of it. A
/// start refused for such a lamp folds it down first, then lets it go.
fn found_up(up: Standing, found: &[Sample; 5]) -> bool {
    let (lift, offs) = on_the_way(up, ARM.map(|joint| found[joint.index()].position));
    lift >= 0.05
        && offs
            .iter()
            .all(|off| *off <= f32::from(Standing::FOUND_SLACK))
}

/// Under `--overload release`: let go. Nothing more is asked of the joints:
/// torque off (register 40, 0) is written to each of `joints` (all five
/// after an overload, the head and base only after a hand), each
/// acknowledged, and one that failed is tried again on the next step. For
/// `rest` from then on it reads all five every step and acts on nothing:
/// moods, gestures and the eyes are taken and dropped, and no goal is
/// written (a goal written to one of these servos turns its torque back
/// on; a joint not let go keeps its torque and its last goal). Returns
/// what the joints read once `joints` are let go and the rest is over.
/// With `arm_trip` (a hand's rest, the arm still held), the shoulder and
/// the elbow keep the run's overload rule: one at its `trip_at` load for
/// `OVERLOAD_STEPS` steps in a row ends the rest early with that joint and
/// its load, for the caller to let go of all five as an overload.
/// Asked to stop, or a wire that fails for half a second, ends it with
/// torque off.
fn let_go<P: PortIo, R: Runtime>(
    bus: &mut Bus<'_, P, R>,
    report: &mut BodyReport,
    joints: &[Joint],
    (rest, arm_trip): (Duration, Option<[u16; 5]>),
    (news, asked): (
        &mut impl FnMut() -> Option<(Mood, u8)>,
        &mut impl FnMut() -> (Asked, Priority, Option<Source>),
    ),
    watch: &mut impl FnMut(&Glimpse),
    template: Glimpse,
) -> Result<([Sample; 5], Option<Strained>), Stop> {
    let started = bus.runtime.now();
    let mut holding: Vec<Joint> = joints.to_vec();
    let mut strained = [0u16; 2];
    let (mut ticks, mut misses) = (0u32, 0u8);
    loop {
        ticks += 1;
        let due = started + TICK * ticks;
        if !matches!(bus.runtime.cancelled(), Ok(false)) {
            return Err(Stop::Cancelled);
        }
        let (_, (asked_now, _, asked_source)) = (news(), asked());
        let failed = torque_off(bus, report, &mut holding);
        report.steps += 1;
        let read = bus.samples();
        if let Ok(now) = &read {
            report.last = Some(*now);
            for (index, joint) in COMMANDED.iter().enumerate() {
                let is = now[joint.index()];
                report.peak_load[index] = report.peak_load[index].max(is.load);
                report.peak_temperature[index] = report.peak_temperature[index].max(is.temperature);
            }
            for (index, joint) in ARM.iter().enumerate() {
                let is = now[joint.index()];
                report.arm_peak_load[index] = report.arm_peak_load[index].max(is.load);
                report.arm_peak_temperature[index] =
                    report.arm_peak_temperature[index].max(is.temperature);
                if let Some(trip_at) = arm_trip {
                    strained[index] = if is.load >= trip_at[joint.index()] {
                        strained[index].saturating_add(1)
                    } else {
                        0
                    };
                }
            }
            let mut glimpse = glimpse_of(template, report.steps, now, template.goals);
            if let Some(clip) = asked_now.gesture {
                glimpse.gesture_fates.push_told(
                    (clip.name, "dropped: it has let go and lies limp"),
                    Told {
                        source: asked_source,
                        by: None,
                    },
                );
            }
            watch(&glimpse);
        }
        match (failed, read) {
            (None, Ok(now)) => {
                misses = 0;
                let tripped = ARM
                    .into_iter()
                    .zip(strained)
                    .filter(|(_, steps)| *steps >= OVERLOAD_STEPS)
                    .map(|(joint, _)| (joint, now[joint.index()].load))
                    .max_by_key(|(_, load)| *load);
                if tripped.is_some() {
                    return Ok((now, tripped));
                }
                if holding.is_empty() && bus.runtime.now() - started >= rest {
                    return Ok((now, None));
                }
            }
            (Some(detail), _) | (None, Err(detail)) => {
                report.missed_reads += 1;
                misses += 1;
                if misses >= WIRE_STEPS || bus.refused.is_some() {
                    return Err(bus.refused.take().unwrap_or(Stop::Bus {
                        detail: format!("letting go: {detail}"),
                    }));
                }
            }
        }
        bus.sleep_until(due);
    }
}

/// An arm joint that tripped the overload rule during a hand's rest, and
/// its load.
type Strained = (Joint, u16);

/// Torque off (register 40, 0) to each of `holding`, each acknowledged;
/// one that took it is dropped from `holding`. The first failure, if any.
fn torque_off<P: PortIo, R: Runtime>(
    bus: &mut Bus<'_, P, R>,
    report: &mut BodyReport,
    holding: &mut Vec<Joint>,
) -> Option<String> {
    let mut failed = None;
    holding.retain(
        |joint| match bus.write_acknowledged(*joint, &torque_write(*joint, 0), true) {
            Ok(()) => {
                report.torque_writes = report.torque_writes.saturating_add(1);
                false
            }
            Err(detail) => {
                failed.get_or_insert(detail);
                true
            }
        },
    );
    failed
}

/// As V1, a run that ends on a surprise lets go of the whole lamp: torque
/// off on all five, once each (a wire that has gone silent is not waited on).
fn let_go_at_end<P: PortIo, R: Runtime>(bus: &mut Bus<'_, P, R>, report: &mut BodyReport) {
    let _ = torque_off(bus, report, &mut Joint::ALL.to_vec());
}

/// What a let-go after the body stopped found and did (#3697,
/// `lamp-body-measure let-go`): whether the lamp was down, whether every
/// joint then read back let go, and each joint as it was read.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct LetGo {
    pub down: bool,
    pub let_go: bool,
    pub joints: Vec<LetGoJoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// One joint of a [`LetGo`], servo 1 to 5: where it was found, and its
/// torque enable read back afterwards (0 let go, 1 held; `None` unread).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LetGoJoint {
    pub joint: &'static str,
    pub position: u16,
    pub torque: Option<u8>,
}

/// The body's let-go before the power goes (#3697, HAL-4.12 and HAL-5.18):
/// run once the body has stopped, folded and holding. Each joint is read;
/// only a lamp that is down, as the stop's own let-go judges it (the head
/// within `LET_GO_NEAR` of `home`, the arm of `folded` when the lamp has
/// one), has torque off written to all five. Then each joint's torque is
/// read back. A lamp found up keeps its torque: let go, it would fall.
pub(crate) fn let_go_stopped<P: PortIo, R: Runtime>(
    io: &mut P,
    runtime: &mut R,
    home: [u16; 3],
    folded: Option<[u16; 2]>,
) -> LetGo {
    let mut bus = Bus::new(io, runtime, None);
    let mut out = LetGo::default();
    let found = match bus.samples() {
        Ok(found) => found,
        Err(error) => {
            out.error = Some(format!("first read: {error}"));
            return out;
        }
    };
    out.down = (0..3).all(|index| {
        found[COMMANDED[index].index()]
            .position
            .abs_diff(home[index])
            <= LET_GO_NEAR
    }) && folded.is_none_or(|folded| arm_folded(&found, folded));
    if out.down {
        for joint in Joint::ALL {
            if let Err(error) = bus.write_acknowledged(joint, &torque_write(joint, 0), true) {
                out.error.get_or_insert(format!("torque off: {error}"));
            }
        }
    }
    let torques = bus.torques();
    if let Err(error) = &torques {
        out.error.get_or_insert(format!("read back: {error}"));
    }
    out.joints = Joint::ALL
        .iter()
        .map(|joint| LetGoJoint {
            joint: joint.name(),
            position: found[joint.index()].position,
            torque: torques.as_ref().ok().map(|torques| torques[joint.index()]),
        })
        .collect();
    out.let_go = out.down && out.joints.iter().all(|joint| joint.torque == Some(0));
    out
}

/// How often the joints are read while the lamp is let go for a pose a
/// hand can set straight (#2602): ten times a second, a fifth of the run's
/// own reads, enough to start within a second or so of being set right.
pub const WAIT_READ: Duration = Duration::from_millis(100);
/// Reads in a row, `WAIT_READ` apart, every joint in range and still
/// before the lamp takes hold again: a second, so that a hand passing
/// through the range on its way is not taken as the lamp set straight.
const IN_RANGE_READS: u8 = 10;
/// Still, between two such reads: no joint moved more than this, 30 counts
/// a second. A hand still setting the head moves it faster than that.
const WAIT_STILL: u16 = 3;
/// What the lamp asks of a person while it waits, for the joint found out
/// of range first: pitch, roll, yaw.
const SET_STRAIGHT: [&str; 3] = [
    "My head is tipped too far. Please move it gently back toward the middle, and I will start by myself.",
    "My head is twisted too far. Please turn it gently back toward the middle, and I will start by myself.",
    "My base is turned too far. Please turn it gently back toward the middle, and I will start by myself.",
];
/// Said instead when the head is found tipped up out of range, against its
/// weight: its pitch keeps its torque, since let go it would drop onto the
/// base or a hand (#2938), so no hand can set it straight while it waits.
const PROPPED: &str = "My head is tipped up too far, and I am holding it so it does not drop. I need someone to help me set it straight.";
/// Said instead when the shoulder, lowered on waking, stopped short of
/// folded (#2940, El's lamp-06c1 case): the lamp lets go of its arm, holds
/// its head, and starts again once a hand has folded the arm.
const ARM_FOLD: &str = "My arm is not folded all the way down. Please fold it gently down, and I will start by myself.";
/// How long the lamp asks before it stops (#2938): after this nobody has
/// set it straight, and it stays let go, reads every `WAIT_READ_LATE` and
/// says `STOPPED_ASKING` instead. Counted from the first time it let go in
/// this run, so a take-hold that fails and lets go again does not restart it.
pub const WAIT_ASKING: Duration = Duration::from_secs(600);
/// Reads once the lamp has stopped asking: a fifth as often. Its
/// diagnostic goes once per change either way
/// (`crate::bus_out::WAITING_RESEND`).
pub const WAIT_READ_LATE: Duration = Duration::from_millis(500);
/// What it says, no longer asking, once `WAIT_ASKING` has passed.
pub use crate::ask::STOPPED_ASKING;

/// A start refused for a pose a hand can set straight (#2602): where each
/// of pitch, roll and yaw may be found and start, the joints let go while
/// it waits, and what it asks.
#[derive(Clone, Debug)]
struct PoseWait {
    ranges: [(u16, u16); 3],
    joints: Vec<Joint>,
    say: &'static str,
    /// The shoulder's folded count, when the wait is for the arm to be
    /// folded (#2940): within `Standing::FOUND_SLACK` of it, too.
    arm: Option<u16>,
}

/// The pose `found` (pitch, roll, yaw) as the start's own checks in
/// `run_on` judge it: `None` if they let it start, or if no hand can help
/// (the home given is itself outside the head's start window). The ranges
/// are those checks' bounds: inside the start window beside the base when
/// not waking, within `start_reach` (150 off the base) of `home_found`, or
/// within `WAKE_REACH` of it when waking, and anywhere for a roll or a base
/// that `--limp` takes as found. Let go are the three this body commands,
/// and the shoulder and the elbow too for a body that stands (found folded).
fn pose_wait(
    limits: &Limits,
    home: [u16; 3],
    home_found: [f32; 3],
    found: [u16; 3],
    (beside_base, may_wake): (bool, bool),
    stands: bool,
) -> Option<PoseWait> {
    let window = limits.pitch_home.0..=limits.pitch_home.1;
    if beside_base && !window.contains(&home[0]) {
        return None;
    }
    let allowed = if beside_base {
        limits.start_reach
    } else {
        limits.start_reach.max(150)
    };
    let reach = f32::from(if may_wake {
        allowed.max(WAKE_REACH)
    } else {
        allowed
    });
    let ranges: [(u16, u16); 3] = std::array::from_fn(|index| {
        if may_wake && limits.limp && index != 0 {
            return (0, 4_095);
        }
        let low = (home_found[index] - reach).ceil().clamp(0.0, 4_095.0) as u16;
        let high = (home_found[index] + reach).floor().clamp(0.0, 4_095.0) as u16;
        if index == 0 && beside_base && !may_wake {
            (low.max(*window.start()), high.min(*window.end()))
        } else {
            (low, high)
        }
    });
    let out = (0..3).find(|index| {
        let (low, high) = ranges[*index];
        !(low..=high).contains(&found[*index])
    })?;
    let mut joints = if stands {
        Joint::ALL.to_vec()
    } else {
        COMMANDED.to_vec()
    };
    // Let go only what gravity already holds (#2938): a head tipped up out
    // of range (fewer counts, see `lifts`) keeps its torque.
    let propped = found[0] < ranges[0].0;
    if propped {
        joints.retain(|joint| *joint != Joint::WristPitch);
    }
    Some(PoseWait {
        ranges,
        joints,
        say: if propped { PROPPED } else { SET_STRAIGHT[out] },
        arm: None,
    })
}

/// Let go of `wait.joints` (torque off, each acknowledged, a failed one
/// tried again) and read every `WAIT_READ`, each read a step watched with
/// what it asks (`Glimpse::waiting`), until pitch, roll and yaw have been in
/// `wait.ranges`, and all five still (`WAIT_STILL`), for `IN_RANGE_READS`
/// reads in a row; then take hold of the
/// joints let go where they are, as `--torque on` does. Only torque off is
/// written until then, and no goal. One that moves when taken hold of is
/// let go again and waited for. Asked to stop, it ends let go; a wire that
/// fails for `WIRE_STEPS` reads ends it as a run's would. From `asked_since`
/// plus `WAIT_ASKING` on it reads every `WAIT_READ_LATE` and says
/// `STOPPED_ASKING`, still let go and still starting once set straight.
fn wait_in_range<P: PortIo, R: Runtime>(
    bus: &mut Bus<'_, P, R>,
    report: &mut BodyReport,
    limits: &Limits,
    wait: &PoseWait,
    asked_since: Duration,
    (news, asked): (
        &mut impl FnMut() -> Option<(Mood, u8)>,
        &mut impl FnMut() -> (Asked, Priority, Option<Source>),
    ),
    watch: &mut impl FnMut(&Glimpse),
) -> Result<(), Stop> {
    let mut holding = wait.joints.clone();
    let (mut misses, mut in_range) = (0u8, 0u8);
    let mut before: Option<[u16; 5]> = None;
    // Steps in a row a commanded joint still held (a head tipped up,
    // #2938) has read over its load bound, and at its temperature bound.
    let (mut held_over, mut held_hot) = ([0u8; 3], [0u8; 3]);
    loop {
        let late = bus.runtime.now() >= asked_since + WAIT_ASKING;
        let (every, say) = if late {
            (WAIT_READ_LATE, STOPPED_ASKING)
        } else {
            (WAIT_READ, wait.say)
        };
        let due = bus.runtime.now() + every;
        if !matches!(bus.runtime.cancelled(), Ok(false)) {
            return Err(Stop::Cancelled);
        }
        // Taken and dropped: a lamp let go acts on nothing.
        let _ = (news(), asked());
        let failed = torque_off(bus, report, &mut holding);
        match (failed, bus.samples()) {
            (None, Ok(now)) => {
                misses = 0;
                report.steps += 1;
                report.last = Some(now);
                // A joint held while the rest wait is judged by the run's
                // own load and heat bounds, however long the wait: a pitch
                // held at a stale goal must not push or heat on unchecked
                // (El's P2 on #2940). Over either, the wait ends as the
                // run's check would.
                for (slot, joint) in COMMANDED.iter().enumerate() {
                    if wait.joints.contains(joint) {
                        continue;
                    }
                    let is = now[joint.index()];
                    held_over[slot] = if is.load > limits.load[slot] {
                        held_over[slot].saturating_add(1)
                    } else {
                        0
                    };
                    held_hot[slot] = if is.temperature >= limits.temperature {
                        held_hot[slot].saturating_add(1)
                    } else {
                        0
                    };
                    if held_over[slot] >= limits.load_steps.max(1) {
                        return Err(Stop::Load {
                            joint: joint.id(),
                            load: is.load,
                        });
                    }
                    if held_hot[slot] >= limits.temperature_steps.max(1) {
                        return Err(Stop::Temperature {
                            joint: joint.id(),
                            temperature: is.temperature,
                        });
                    }
                }
                let pose = COMMANDED.map(|joint| now[joint.index()].position);
                watch(&Glimpse {
                    waiting: Some(say),
                    ..glimpse_of(Glimpse::default(), report.steps, &now, pose)
                });
                let positions = now.map(|sample| sample.position);
                let still = before.is_some_and(|before| {
                    before
                        .iter()
                        .zip(positions)
                        .all(|(was, is)| was.abs_diff(is) <= WAIT_STILL)
                });
                before = Some(positions);
                let inside = still
                    && pose
                        .iter()
                        .zip(wait.ranges)
                        .all(|(at, (low, high))| (low..=high).contains(at))
                    && wait.arm.is_none_or(|folded| {
                        positions[Joint::BasePitch.index()].abs_diff(folded)
                            <= Standing::FOUND_SLACK
                    });
                in_range = if inside { in_range + 1 } else { 0 };
                if in_range >= IN_RANGE_READS && !bus.runtime.recovery_inhibited() {
                    match take_hold(bus, report, &wait.joints, &now) {
                        Ok(_) => return Ok(()),
                        // Let go again, and waited for again.
                        Err(_) => {
                            holding = wait.joints.clone();
                            in_range = 0;
                        }
                    }
                }
            }
            (Some(detail), _) | (None, Err(detail)) => {
                report.missed_reads += 1;
                misses += 1;
                if misses >= WIRE_STEPS || bus.refused.is_some() {
                    return Err(bus.refused.take().unwrap_or(Stop::Bus {
                        detail: format!("let go to be set straight: {detail}"),
                    }));
                }
            }
        }
        bus.sleep_until(due);
    }
}

/// One step of a joint that gave way at `from` and is still held at `goal`,
/// under `--give-way`: `steps` counts the steps in a row it has been in a
/// hand, its load over `bound` or it kept more than `HAND_MOVED` counts off
/// `goal` (a light head on a soft servo is held 30 to 40 counts away at a
/// load of only 90 to 130 on lamp-52e6). After a second of that,
/// `Some(true)` if it has been moved more than `HAND_MOVED` counts from
/// `from` (a hand moved it), `Some(false)` if not (something pushes it and
/// it does not move).
fn in_hand_after(steps: &mut u16, is: Sample, bound: u16, from: u16, goal: u16) -> Option<bool> {
    let in_hand = is.load > bound || is.position.abs_diff(goal) > HAND_MOVED;
    *steps = if in_hand { *steps + 1 } else { 0 };
    if *steps < HAND_STEPS {
        return None;
    }
    *steps = 0;
    Some(is.position.abs_diff(from) > HAND_MOVED)
}

/// One joint pushing at a goal it does not reach, under `--give-way`.
#[derive(Clone, Copy, Debug, Default)]
struct Stall {
    /// Steps in a row it has pushed without moving, and where it was when
    /// they began.
    steps: u16,
    at: u16,
    /// Where it last stood unpressed at its goal (`STOOD_JITTER`), or was
    /// last held: a hand carries it from there, away from the goal, however
    /// slowly and whatever its load does on the way; a joint short of a new
    /// goal under its own weight, or against a shelf, came toward it.
    stood_at: Option<u16>,
    /// The goal on the last step.
    goal_was: Option<u16>,
    /// The stop just made was of a joint carried away from its goal while
    /// pressed, more than `STALL_SLACK`: a hand moved it.
    pub(crate) carried: bool,
    /// Told to stop pushing: the goal wanted of it then, and where it is
    /// held instead.
    hold: Option<(u16, u16)>,
    /// Steps it has been held so far.
    held: u16,
    /// The hold was made while a move played and was not carried by a
    /// hand: it is keyed on what is wanted with the move laid over, so the
    /// move's later frames (its return) still drive the joint, and it ends
    /// with the move. A head lifted and held by a gesture stalls against
    /// its own weight; latched on the mood pose alone it stayed up for
    /// minutes, and the rest then took that pose as its own (#3691).
    with_move: bool,
    /// Where it last stopped. A joint that stops there again (a standing
    /// shoulder resting short of its goal under its own weight, each time a
    /// scene or the rest moves its goal a little) is held again quietly: on
    /// lamp-4ace on 2026-10-15 that same rest was counted and logged twice
    /// a minute with nobody touching the lamp.
    last_at: Option<u16>,
    /// The hold just made was at that same place: held, not counted.
    pub(crate) again: bool,
    /// Steps in a row it has moved freely away from where it last stopped;
    /// after `HAND_STEPS` of them that place is forgotten, so a later stall
    /// there is a new one and counted.
    free: u16,
    /// A standing arm joint's load as it read once stood up: it pushes only
    /// over this plus `ARM_STALL_MARGIN`. None for the head, and for an arm
    /// rising, folding or folded.
    carries: Option<u16>,
    /// It read a load over its stall bound on the last step: something,
    /// most likely a hand, is still on it.
    pressed: bool,
    /// Its own stall bound, when not `STALL_LOAD`: the head's pitch, from
    /// `Limits::head_stall_load`.
    least: Option<u16>,
}

impl Stall {
    /// One step of a joint whose goal is `goal` and of which `wanted` is
    /// wanted; `moving`, while a move plays, is that with the move's offset
    /// laid over. True when it has just been told to stop pushing; `again`
    /// then says whether that was where it last stopped (not to be counted).
    fn step(&mut self, goal: u16, is: Sample, wanted: u16, moving: Option<u16>) -> bool {
        let bound = self
            .carries
            .map_or(self.least.unwrap_or(STALL_LOAD), |load| {
                STALL_LOAD.max(load.saturating_add(ARM_STALL_MARGIN))
            });
        self.pressed = is.load > bound;
        let pushing = goal.abs_diff(is.position) > STALL_SLACK && self.pressed;
        // At its goal, give or take a held servo's jitter, and not pressed:
        // a hand easing it off, its load dipping now and then, would carry
        // this along with it.
        // Nor a goal moved on from while unpressed: a joint resting a few
        // counts short of the last one (under its weight, inside what
        // counts as arrived) starts the next move from where it is.
        let moved_on = self.goal_was.is_some_and(|was| was != goal);
        self.goal_was = Some(goal);
        if !self.pressed && (moved_on || goal.abs_diff(is.position) <= STOOD_JITTER) {
            self.stood_at = Some(is.position);
        }
        let away = self
            .last_at
            .is_some_and(|at| at.abs_diff(is.position) > STALL_SLACK);
        self.free = if away && is.load <= bound {
            self.free.saturating_add(1)
        } else {
            0
        };
        if self.free >= HAND_STEPS {
            self.last_at = None;
        }
        if !pushing || self.steps == 0 || is.position.abs_diff(self.at) > STALL_SLACK {
            self.at = is.position;
            self.steps = u16::from(pushing);
            return false;
        }
        self.steps += 1;
        if self.steps < STALL_STEPS {
            return false;
        }
        self.steps = 0;
        self.carried = self.stood_at.is_some_and(|from| {
            is.position.abs_diff(from) > STALL_SLACK
                && goal.abs_diff(is.position) > goal.abs_diff(from)
        });
        // Held here from now on: a hand carrying it on goes from here.
        self.stood_at = Some(is.position);
        self.with_move = moving.is_some() && !self.carried;
        let wanted = moving.filter(|_| self.with_move).unwrap_or(wanted);
        self.hold = Some((wanted, is.position));
        self.held = 0;
        self.again = self
            .last_at
            .is_some_and(|at| at.abs_diff(is.position) <= STALL_SLACK);
        self.last_at = Some(is.position);
        true
    }

    /// Where it is held, while what is wanted of it has not moved
    /// `WAKE_COUNTS` from what was wanted when it stalled. A hold made in a
    /// move (`with_move`) is judged on `moving`, and ends with the move.
    fn holding(&mut self, wanted: u16, moving: Option<u16>) -> Option<u16> {
        let wanted = if self.with_move { moving } else { Some(wanted) };
        match (self.hold, wanted) {
            (Some((was, at)), Some(wanted)) if f32::from(wanted.abs_diff(was)) < WAKE_COUNTS => {
                self.held = self.held.saturating_add(1);
                Some(at)
            }
            _ => {
                self.hold = None;
                None
            }
        }
    }
}

/// Run the body until it is cancelled or something is not as expected.
/// `news` is asked once a step for the latest mood and never waits. `home`
/// is the pitch, roll and yaw every gesture is an offset from; without one
/// it is the pose found at start. With one, the head first eases there from
/// where it is found, at the same speed as everything else.
#[cfg(test)]
pub(crate) fn run(
    io: &mut impl PortIo,
    runtime: &mut impl Runtime,
    limits: Limits,
    home: Option<[u16; 3]>,
    news: impl FnMut() -> Option<(Mood, u8)>,
    watch: impl FnMut(&Glimpse),
) -> BodyReport {
    run_standing(io, runtime, limits, home, None, news, watch)
}

/// The same for a lamp that may also stand up: see [`Standing`]. It rises
/// when a mood other than rest arrives, stays up while they keep arriving
/// and for `Standing::awake` after the last, and folds down again; asked to
/// stop, it folds down first. A shoulder or an elbow that reads over its
/// load, does not follow or stays warm sends it back down, where it rests
/// for a minute. It needs a home, since where a standing lamp is found is
/// not its folded pose.
#[cfg(test)]
pub(crate) fn run_standing(
    io: &mut impl PortIo,
    runtime: &mut impl Runtime,
    limits: Limits,
    home: Option<[u16; 3]>,
    standing: Option<Standing>,
    news: impl FnMut() -> Option<(Mood, u8)>,
    watch: impl FnMut(&Glimpse),
) -> BodyReport {
    run_looking(
        io,
        runtime,
        limits,
        home,
        standing,
        None,
        news,
        || None,
        watch,
    )
}

/// The same for a lamp that also has eyes: see [`Looking`] and
/// [`crate::gaze`]. `eyes` is asked once a step for what the eyes last saw
/// and never waits. While the lamp listens or speaks, and is not folded or
/// folding, its head turns in small corrections to the face they report,
/// within the same reach as every gesture.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_looking(
    io: &mut impl PortIo,
    runtime: &mut impl Runtime,
    limits: Limits,
    home: Option<[u16; 3]>,
    standing: Option<Standing>,
    looking: Option<Looking>,
    news: impl FnMut() -> Option<(Mood, u8)>,
    mut eyes: impl FnMut() -> Option<Seen>,
    watch: impl FnMut(&Glimpse),
) -> BodyReport {
    let asked = move || Asked {
        seen: eyes(),
        gesture: None,
        expression: None,
        called: false,
        still: false,
        paused: false,
        asleep: false,
        woken: false,
        toward: None,
        unplug: false,
        held_by_test: false,
        held: false,
        aim: None,
    };
    run_alive(
        io, runtime, limits, home, standing, looking, news, asked, watch,
    )
}

/// The same for a lamp that also makes gestures: see [`crate::clips`].
/// `asked` is called once a step for what its eyes last saw and for a
/// gesture it has been asked to make, and never waits. A gesture plays only
/// on a lamp standing still at its full height; asked of one still rising
/// it waits up to three seconds, and of any other it is forgotten. Its
/// offsets are laid over whatever the lamp is doing and leave it where it
/// was. If a joint strains or falls behind while one plays, the gesture is
/// let go and the lamp carries on.
///
/// Every way a run ends passes here once: any surprise, at start-up, while
/// waking or lowering the shoulder, or while running, lets go of the whole
/// lamp, as V1 did, after the joints were held where they are (a stuck
/// joint held at its goal with torque on keeps pushing and heats). Not when
/// asked to stop, not on a refusal (one for a hot or loaded joint has let
/// go already), and not under overload, where torque is already off. A
/// standing lamp whose shoulder or elbow is too warm at start, or cannot be
/// read while it runs, folds down first (`run_on`).
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_alive(
    io: &mut impl PortIo,
    runtime: &mut impl Runtime,
    limits: Limits,
    home: Option<[u16; 3]>,
    standing: Option<Standing>,
    looking: Option<Looking>,
    news: impl FnMut() -> Option<(Mood, u8)>,
    mut asked: impl FnMut() -> Asked,
    watch: impl FnMut(&Glimpse),
) -> BodyReport {
    run_arbitrated(
        io,
        runtime,
        limits,
        home,
        standing,
        looking,
        news,
        move || (asked(), Priority::Conversation),
        watch,
    )
}

/// The same, with who asked for each gesture (`Limits::arbitration`).
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_arbitrated(
    io: &mut impl PortIo,
    runtime: &mut impl Runtime,
    limits: Limits,
    home: Option<[u16; 3]>,
    standing: Option<Standing>,
    looking: Option<Looking>,
    news: impl FnMut() -> Option<(Mood, u8)>,
    mut asked: impl FnMut() -> (Asked, Priority),
    watch: impl FnMut(&Glimpse),
) -> BodyReport {
    run_told(
        io,
        runtime,
        limits,
        home,
        standing,
        looking,
        news,
        move || {
            let (asked, priority) = asked();
            (asked, priority, None)
        },
        watch,
    )
}

/// The same, with the name each asker gave, to tell the one that lost the
/// body and to whom on `body/gesture` (#940).
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_told(
    io: &mut impl PortIo,
    runtime: &mut impl Runtime,
    limits: Limits,
    home: Option<[u16; 3]>,
    standing: Option<Standing>,
    looking: Option<Looking>,
    news: impl FnMut() -> Option<(Mood, u8)>,
    asked: impl FnMut() -> (Asked, Priority, Option<Source>),
    watch: impl FnMut(&Glimpse),
) -> BodyReport {
    let mut report = run_until_stopped(
        io, runtime, limits, home, standing, looking, news, asked, watch,
    );
    if !matches!(
        report.stop,
        Stop::Cancelled | Stop::Refused { .. } | Stop::Overload { .. }
    ) {
        let_go_at_end(&mut Bus::new(io, runtime, None), &mut report);
    }
    report
}

#[allow(clippy::too_many_arguments)]
fn run_until_stopped(
    io: &mut impl PortIo,
    runtime: &mut impl Runtime,
    limits: Limits,
    home: Option<[u16; 3]>,
    standing: Option<Standing>,
    looking: Option<Looking>,
    mut news: impl FnMut() -> Option<(Mood, u8)>,
    mut asked: impl FnMut() -> (Asked, Priority, Option<Source>),
    mut watch: impl FnMut(&Glimpse),
) -> BodyReport {
    let mut bus = Bus::new(io, runtime, standing);
    bus.settle_steps = limits.temperature_steps.max(1);
    let mut report = run_on(
        &mut bus, limits, home, standing, looking, &mut news, &mut asked, &mut watch, 0,
    );
    // #2602: refused for a pose a hand can set straight, it lets go, says
    // so and starts again once the pose is in range, until asked to stop.
    let mut said = None;
    let mut asked_since = None;
    while matches!(report.stop, Stop::Refused { .. })
        && let Some(wait) = report.pose_wait.take()
    {
        said = Some(wait.say);
        let since = *asked_since.get_or_insert_with(|| bus.runtime.now());
        if let Err(stop) = wait_in_range(
            &mut bus,
            &mut report,
            &limits,
            &wait,
            since,
            (&mut news, &mut asked),
            &mut watch,
        ) {
            report.stop = stop;
            break;
        }
        report = run_on(
            &mut bus,
            limits,
            home,
            standing,
            looking,
            &mut news,
            &mut asked,
            &mut watch,
            report.steps,
        );
    }
    report.refused_let_go = said;
    report.bad_reads = bus.bad_reads;
    report.bad_temperatures = bus.bad_temperatures;
    report.temperature_flaps = bus.temperature_flaps;
    report.temperature_settles = std::mem::take(&mut bus.settles);
    report
}

/// The run itself, on `bus`, which keeps what is counted of every read.
#[allow(clippy::too_many_arguments)]
fn run_on<P: PortIo, R: Runtime>(
    bus: &mut Bus<'_, P, R>,
    limits: Limits,
    home: Option<[u16; 3]>,
    standing: Option<Standing>,
    looking: Option<Looking>,
    mut news: impl FnMut() -> Option<(Mood, u8)>,
    mut asked: impl FnMut() -> (Asked, Priority, Option<Source>),
    mut watch: impl FnMut(&Glimpse),
    from_step: u64,
) -> BodyReport {
    // One copy of the loop per port/runtime, not one per caller's closures.
    run_on_dyn(
        bus, limits, home, standing, looking, &mut news, &mut asked, &mut watch, from_step,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_on_dyn<P: PortIo, R: Runtime>(
    bus: &mut Bus<'_, P, R>,
    limits: Limits,
    home: Option<[u16; 3]>,
    standing: Option<Standing>,
    looking: Option<Looking>,
    mut news: &mut dyn FnMut() -> Option<(Mood, u8)>,
    asked: &mut dyn FnMut() -> (Asked, Priority, Option<Source>),
    mut watch: &mut dyn FnMut(&Glimpse),
    from_step: u64,
) -> BodyReport {
    // Steps were watched before this run began (#2602): it is a start
    // again after a wait, and the lamp has said it is ready.
    let watched = from_step > 0;
    // Runtime home can shift after contact or a short wake. Only the
    // explicitly configured folded target can attest unplug readiness.
    let unplug_home = home;
    let mut standing = standing;
    let mut report = BodyReport {
        limits,
        moved: false,
        home: None,
        found: None,
        steps: from_step,
        goal_writes: 0,
        profile_writes: 0,
        missed_reads: 0,
        bad_reads: [0; 5],
        bad_temperatures: [0; 5],
        temperature_flaps: [0; 5],
        hot_rereads: Vec::new(),
        temperature_settles: Vec::new(),
        peak_load: [0; 3],
        eased: [0; 3],
        kept_where_put: [0; 3],
        stalls: [0; 5],
        released_by_hand: [0; 3],
        rested_short: [0; 3],
        lowered: None,
        peak_temperature: [0; 3],
        held_at: None,
        last: None,
        standing: None,
        standing_refused: None,
        own_ranges: None,
        torque_writes: 0,
        torque_given: Vec::new(),
        woke: None,
        force_writes: 0,
        force: None,
        stood: false,
        let_go_at_stop: false,
        let_go_outside_window: None,
        refused_let_go: None,
        pose_wait: None,
        arm_peak_load: [0; 2],
        arm_peak_temperature: [0; 2],
        retreats: Vec::new(),
        gestures: 0,
        gestures_dropped: 0,
        expression_steps: 0,
        idles: 0,
        calls: 0,
        released: Vec::new(),
        tipping: Tipping::default(),
        stop: Stop::Cancelled,
    };
    let refused = |why: String| Stop::Refused { why };

    // What is there, before anything is written.
    let mut found = match bus.samples() {
        Ok(found) => found,
        Err(detail) => {
            report.stop = refused(format!("first read: {detail}"));
            return report;
        }
    };
    // One glitch on the first read must not let go of a lamp, which may be
    // standing: a joint found hot is read again until two reads in a row
    // agree within `TEMPERATURE_JUMP`, and heat is acted on only if both
    // are hot, or reads never agree and one was. A read over
    // `TEMPERATURE_MAX` is a glitch, never heat and never agreeing: one on
    // every read is a temperature that cannot be read, and the run is
    // refused (a lamp found up folds down first, then is let go; otherwise
    // nothing is written and nothing let go). Only the joints the
    // found-hot checks below judge are read for it: the three commanded,
    // and the arm of a lamp that may stand. Nothing has been written yet.
    let arm_judged = standing.is_some() && home.is_some();
    // A refusal of a lamp found up on its way, with nothing written: it is
    // not left up held, where nothing would ever lower it. It folds down
    // first, as when asked to stop, and is let go folded, on this refusal.
    let mut refusal: Option<String> = None;
    let judged = |read: &[Sample; 5]| {
        let commanded =
            COMMANDED.map(|joint| (read[joint.index()].temperature, limits.temperature));
        let arm = ARM.map(|joint| (read[joint.index()].temperature, Standing::TEMPERATURE));
        commanded
            .into_iter()
            .chain(arm.into_iter().filter(|_| arm_judged))
    };
    let over =
        |read: &[Sample; 5]| judged(read).any(|(temperature, _)| temperature > TEMPERATURE_MAX);
    let hot = |read: &[Sample; 5]| {
        judged(read).any(|(temperature, limit)| (limit..=TEMPERATURE_MAX).contains(&temperature))
    };
    if hot(&found) || over(&found) {
        let mut hottest = hot(&found).then_some(found);
        let mut cool = (!over(&found)).then_some(found);
        loop {
            if report.hot_rereads.len() >= FIRST_HEAT_REREADS {
                match (hottest, cool) {
                    (Some(read), _) | (None, Some(read)) => found = read,
                    // Only the arm of a lamp that may stand reads over 100:
                    // it is judged too warm below, and a standing one folds.
                    (None, None)
                        if COMMANDED
                            .iter()
                            .all(|joint| found[joint.index()].temperature <= TEMPERATURE_MAX) => {}
                    (None, None) => {
                        let (joint, temperature) = COMMANDED
                            .into_iter()
                            .map(|joint| (joint, found[joint.index()].temperature))
                            .max_by_key(|(_, temperature)| *temperature)
                            .unwrap_or((Joint::WristPitch, 0));
                        let why =
                            format!("temperature unreadable at start: {joint} reads {temperature}");
                        if !(arm_judged && standing.is_some_and(|up| found_up(up, &found))) {
                            report.stop = refused(why);
                            return report;
                        }
                        // Folding, these joints' temperatures are not judged.
                        for joint in COMMANDED {
                            if found[joint.index()].temperature > TEMPERATURE_MAX {
                                bus.unjudged[joint.index()] = true;
                            }
                        }
                        refusal = Some(why);
                    }
                }
                break;
            }
            bus.forget_temperatures();
            // A read again that fails acts on the hottest read, so a hot
            // joint is let go and not left held.
            let again = match (bus.samples(), hottest) {
                (Ok(again), _) => again,
                (Err(_), Some(read)) => {
                    found = read;
                    break;
                }
                (Err(detail), None) => {
                    report.stop = refused(format!("first read: {detail}"));
                    return report;
                }
            };
            report
                .hot_rereads
                .push(again.map(|sample| sample.temperature));
            let agree = !over(&found)
                && !over(&again)
                && judged(&found)
                    .zip(judged(&again))
                    .all(|((before, _), (now, _))| before.abs_diff(now) <= TEMPERATURE_JUMP);
            if agree {
                // Both hot: acted on. Otherwise the cool one starts.
                if hot(&found) && hot(&again) || !hot(&again) {
                    found = again;
                }
                break;
            }
            if hot(&again) {
                hottest = Some(again);
            } else if !over(&again) {
                cool = Some(again);
            }
            found = again;
        }
    }
    report.found = Some(found);
    report.last = Some(found);
    let mut profiles = match bus.profiles() {
        Ok(profiles) => profiles,
        Err(detail) => {
            report.stop = refused(format!("first read: {detail}"));
            return report;
        }
    };
    // A lamp's own window for the head is one the servo can reach.
    // Read once here and reused by the own ranges below.
    let mut pitch_limits = None;
    if limits.pitch_window != Limits::FOLDED.pitch_window {
        let (low, high) = limits.pitch_window;
        let read = bus.position_limits(Joint::WristPitch);
        pitch_limits = read.as_ref().ok().copied();
        match read {
            Ok((min, max)) if min <= low && high <= max => {}
            Ok((min, max)) => {
                report.stop = refused(format!(
                    "pitch window {low}..={high} is outside the head pitch servo's own {min}..={max}"
                ));
                return report;
            }
            Err(detail) => {
                report.stop = refused(format!("first read: {detail}"));
                return report;
            }
        }
    }
    // #3558: each joint's own stored limits, read-only, bound the poses
    // given and every head and base goal, as V1's calibration does.
    if limits.own_ranges {
        let mut ranges = [(0, 4_095); 5];
        for joint in Joint::ALL {
            let read = match pitch_limits {
                Some(range) if joint == Joint::WristPitch => Ok(range),
                _ => bus.position_limits(joint),
            };
            // A stored range past the encoder or upside down is not one to
            // bound the lamp by: refused, never clamped wider later. Only
            // here, so a lamp without the switch starts as before.
            let read = read.and_then(|(min, max)| {
                if max > ENCODER_MAX || min > max {
                    Err(format!(
                        "{} stores position limits {min}..={max}, not a range in 0..={ENCODER_MAX}",
                        joint.name()
                    ))
                } else {
                    Ok((min, max))
                }
            });
            match read {
                Ok(range) => ranges[joint.index()] = range,
                Err(detail) => {
                    report.stop = refused(format!("first read: {detail}"));
                    return report;
                }
            }
        }
        report.own_ranges = Some(ranges);
        if let Some(why) = outside_own_ranges(&ranges, home, limits.pitch_window, standing) {
            report.stop = refused(why);
            return report;
        }
        bus.windows = ranges.map(own_window);
        bus.ranges = ranges;
    }
    if standing.is_some() && home.is_none() {
        standing = None;
        report.standing_refused = Some("a lamp that stands needs a home".into());
    }

    // Told to take hold: a joint found without torque, which would be a
    // reason not to start, is given it. Its goal is set to where it is
    // first, since a servo whose torque went off keeps whatever goal it
    // had, and torque on alone would send it there.
    if limits.torque_on {
        // Not one the refusal below would leave as it is (found up, a
        // head or base joint flagged, or unreadable): taking hold of it
        // would switch back on what may be its own cut-out.
        let up = arm_judged && standing.is_some_and(|up| found_up(up, &found));
        let mut without = Vec::new();
        for (index, joint) in COMMANDED.iter().enumerate() {
            let leave = up && (found[joint.index()].status != 0 || bus.unjudged[joint.index()]);
            if profiles[index].0 != 1 && !leave {
                without.push(*joint);
            }
        }
        // The shoulder and the elbow only of a lamp that may stand: of
        // any other they are not this body's to write to.
        if standing.is_some() {
            match bus.arm_profiles() {
                Ok(arm) => {
                    for (index, joint) in ARM.iter().enumerate() {
                        if arm[index].0 != 1 {
                            without.push(*joint);
                        }
                    }
                }
                Err(detail) => {
                    report.stop = refused(format!("first read: {detail}"));
                    return report;
                }
            }
        }
        if !without.is_empty() {
            found = match take_hold(bus, &mut report, &without, &found) {
                Ok(now) => now,
                Err(stop) => {
                    report.stop = stop;
                    return report;
                }
            };
            report.found = Some(found);
            profiles = match bus.profiles() {
                Ok(profiles) => profiles,
                Err(detail) => {
                    report.stop = refused(format!("first read: {detail}"));
                    return report;
                }
            };
        }
    }
    let found_pose = COMMANDED.map(|joint| found[joint.index()].position);
    // Only a home that was given is one to wake to: without it, the home is
    // wherever the lamp was found.
    let may_wake = limits.wake && home.is_some();
    // A let-go run (#1682): --let-go-at-stop with no --home (so no stand,
    // which needs one) and no --torque on, as the factory's that lets a
    // held lamp go. All it ever writes is torque off, so the start window
    // does not hold it.
    let let_go_only = limits.let_go_at_stop && home.is_none() && !limits.torque_on;
    let mut home = home.unwrap_or(found_pose);
    report.home = Some(home);
    for (index, joint) in COMMANDED.iter().enumerate() {
        let sample = found[joint.index()];
        let hot = !bus.unjudged[joint.index()] && sample.temperature >= limits.temperature;
        let loaded = sample.load > limits.start_load[index];
        let why = if profiles[index].0 != 1 {
            Some(format!("{joint} torque is not on"))
        } else if sample.status != 0 {
            Some(format!("{joint} status is {:#04x}", sample.status))
        } else if hot {
            Some(format!("{joint} is at {} already", sample.temperature))
        } else if loaded {
            Some(format!("{joint} already carries load {}", sample.load))
        } else {
            None
        };
        if let Some(why) = why {
            // A joint found hot or loaded is pushing at something: as V1,
            // the whole lamp is let go, or every restart finds it so again.
            if hot || loaded {
                let_go_at_end(bus, &mut report);
            } else if arm_judged && standing.is_some_and(|up| found_up(up, &found)) {
                // Flagged by its own protection, or without torque and
                // unreadable (perhaps its heat cut-out): left as it is
                // while the rest folds.
                bus.left[joint.index()] =
                    sample.status != 0 || (profiles[index].0 != 1 && bus.unjudged[joint.index()]);
                refusal.get_or_insert(why);
                continue;
            }
            report.stop = refused(why);
            return report;
        }
    }
    // The shoulder and the elbow of a lamp that may stand: as they are
    // found, and how far up that is.
    let mut stand = 0.0f32;
    let mut arm_written = ARM.map(|joint| found[joint.index()].position);
    let mut arm_profiles = [(0u8, 0u8, 0u16, 0u16); 2];
    // Found standing with an arm too warm (or reading over 100): it folds
    // down first and is let go folded, on this heat stop.
    let mut fold_for: Option<Stop> = None;
    if let Some(up) = standing {
        arm_profiles = match bus.arm_profiles() {
            Ok(profiles) => profiles,
            Err(detail) => {
                report.stop = refused(format!("first read: {detail}"));
                return report;
            }
        };
        let (mut lift, mut offs) = on_the_way(up, ARM.map(|joint| found[joint.index()].position));
        // A power cut on a standing lamp leaves the shoulder up on its gears
        // and lets the elbow fall folded: woken, such a shoulder is lowered
        // onto its folded count before anything else, and the lamp may then
        // stand as it would have. Any other arm off its way stays as it is.
        if limits.wake && shoulder_to_lower(up, &found, arm_profiles) {
            let head = COMMANDED.map(|joint| found[joint.index()].position);
            let elbow = found[Joint::ElbowPitch.index()].position;
            // Before the first step: READY=1 has not gone out, so systemd's
            // watchdog is not armed yet (its start timeout is 90 s). After a
            // wait for a hand (#2602) it has, and the watchdog with it: each
            // step of the lowering is watched then, as the wake's are (the
            // Reviewer-lead's P2 on #3168).
            let watching = watched.then(|| {
                (
                    &mut watch as &mut dyn FnMut(&Glimpse),
                    Glimpse {
                        stands: true,
                        arm_goals: arm_written,
                        ..Glimpse::default()
                    },
                )
            });
            match lower_shoulder(bus, &mut report, &limits, up, head, elbow, found, watching) {
                Ok(now) => found = now,
                Err(stop) => {
                    if let Some(last) = report.last {
                        let at = |joint: Joint| last[joint.index()].position;
                        let hold = COMMANDED.map(at);
                        if bus.send_holds(hold, Some(ARM.map(at))).is_ok() {
                            report.goal_writes += 1;
                            report.held_at = Some(hold);
                        }
                    }
                    // Stopped short of folded (#2940): with
                    // `--refused-let-go` a hand can fold it, so the lamp is
                    // let go and waits for that, the head judged again when
                    // it starts. Anything else ends the run, as before.
                    if limits.refused_let_go && matches!(stop, Stop::Tracking { .. }) {
                        // Only the arm is let go: it rests folded by its
                        // own weight, and the head, held where it was, is
                        // judged by the run's bounds while it waits
                        // (#2938; the Reviewer-lead's P3 on #3168).
                        report.pose_wait = Some(PoseWait {
                            ranges: [(0, 4_095); 3],
                            joints: ARM.to_vec(),
                            say: ARM_FOLD,
                            arm: Some(up.folded[0]),
                        });
                        report.stop = refused(format!(
                            "the shoulder stopped short of folded while lowered: {stop:?}"
                        ));
                        return report;
                    }
                    // A hold refused is not written; the run ends on what
                    // ended it.
                    report.stop = stop;
                    return report;
                }
            }
            arm_profiles = match bus.arm_profiles() {
                Ok(profiles) => profiles,
                Err(detail) => {
                    report.stop = Stop::Bus {
                        detail: format!("after lowering the shoulder: {detail}"),
                    };
                    return report;
                }
            };
            (lift, offs) = on_the_way(up, ARM.map(|joint| found[joint.index()].position));
        }
        let mut why = None;
        let mut warm = None;
        let mut otherwise = false;
        for (index, joint) in ARM.iter().enumerate() {
            let sample = found[joint.index()];
            let off = offs[index];
            otherwise |= off > f32::from(Standing::FOUND_SLACK);
            if arm_profiles[index].0 != 1 {
                otherwise = true;
                why.get_or_insert(format!("{joint} torque is not on"));
            } else if sample.status & !StatusFlags::OVERLOAD != 0 {
                otherwise = true;
                why.get_or_insert(format!("{joint} status is {:#04x}", sample.status));
            } else if sample.temperature >= Standing::TEMPERATURE {
                why.get_or_insert(format!("{joint} is at {} already", sample.temperature));
                warm.get_or_insert(Stop::Temperature {
                    joint: joint.id(),
                    temperature: sample.temperature,
                });
            } else if off > f32::from(Standing::FOUND_SLACK) {
                let mut said =
                    format!("{joint} is {off:.0} counts off the way between folded and stood up");
                // Which way down from there clears the head, the base and
                // the desk is not known, and these two carry the lamp.
                if limits.wake {
                    said.push_str("; waking does not lower an arm found off its way");
                }
                why.get_or_insert(said);
            }
        }
        // Up on its way and only too warm: left up and held it may heat on,
        // let go it falls. It folds down first.
        if why.is_some() && !otherwise && lift >= 0.05 {
            fold_for = warm;
            why = None;
        }
        match why {
            Some(why) => {
                standing = None;
                report.standing_refused = Some(why);
                // An arm that cannot be folded: refused as before.
                if let Some(why) = refusal.take() {
                    report.stop = refused(why);
                    return report;
                }
            }
            None => {
                // Under a twentieth of the way it is folded.
                stand = if lift < 0.05 { 0.0 } else { unsmooth(lift) };
                // The goal a servo holds, only if it is a count near where
                // the joint is found; otherwise where it is. lamp-4ace's
                // elbow register held 37,639 on 2026-10-17, and taken raw
                // it was written as 4,095.
                arm_written = std::array::from_fn(|index| {
                    let register = arm_profiles[index].3;
                    let at = found[ARM[index].index()].position;
                    if register <= 4_095 && register.abs_diff(at) <= Standing::FOUND_SLACK {
                        register
                    } else {
                        at
                    }
                });
            }
        }
    }
    if let Some(why) = &refusal {
        fold_for = fold_for.or(Some(refused(why.clone())));
        // A head or base found without torque is taken hold of where it
        // is, goal first, to come down with the arm, unless it is left.
        let without: Vec<Joint> = COMMANDED
            .into_iter()
            .zip(profiles)
            .filter(|(joint, profile)| profile.0 != 1 && !bus.left[joint.index()])
            .map(|(joint, _)| joint)
            .collect();
        if !without.is_empty() {
            found = match take_hold(bus, &mut report, &without, &found) {
                Ok(now) => now,
                Err(stop) => {
                    report.stop = stop;
                    return report;
                }
            };
            report.found = Some(found);
        }
    }
    let home_of = |standing: Option<Standing>, stand: f32| home_with(home, standing, stand);
    // The window beside the base is for a head that is beside the base.
    let beside_base = head_share(stand) <= 0.0;
    // #2602: a joint found where a hand can set it straight, on a lamp
    // found folded and with nothing else against starting, is noted here;
    // the refusal below is the same, and `run_until_stopped` lets it go
    // and waits. Never a lamp found up: let go, it would fall.
    if limits.refused_let_go && refusal.is_none() && stand == 0.0 && !let_go_only {
        report.pose_wait = pose_wait(
            &limits,
            home,
            home_of(standing, stand),
            found_pose,
            (beside_base, may_wake),
            standing.is_some(),
        );
    }
    // Waking: a joint found where it may not start, but within WAKE_REACH
    // of its home, is brought there before anything else (below).
    let mut to_wake = false;
    for (pitch, woken) in [(home[0], false), (found_pose[0], may_wake)] {
        if beside_base && !(limits.pitch_home.0..=limits.pitch_home.1).contains(&pitch) {
            if woken {
                to_wake = true;
                continue;
            }
            let why = format!(
                "head pitch {pitch} is outside {}..={}",
                limits.pitch_home.0, limits.pitch_home.1
            );
            // Only with an arm found at the folded counts a person
            // confirmed, so that nothing standing falls; without them,
            // refused as any other run.
            if let_go_only
                && limits
                    .let_go_folded_arm
                    .is_some_and(|folded| arm_folded(&found, folded))
            {
                // Let go at once, torque off on all five and no goal, and
                // left so until asked to stop, as the run would have been.
                let_go_at_end(bus, &mut report);
                report.let_go_at_stop = true;
                report.let_go_outside_window =
                    Some(format!("{why}: a let-go run, torque off only"));
                while matches!(bus.runtime.cancelled(), Ok(false)) {
                    let due = bus.runtime.now() + TICK;
                    bus.sleep_until(due);
                }
                report.stop = Stop::Cancelled;
                return report;
            }
            report.stop = refused(why);
            return report;
        }
    }
    let mut home_found = home_of(standing, stand);
    for (index, joint) in COMMANDED.iter().enumerate() {
        let apart = (f32::from(found_pose[index]) - home_found[index]).abs();
        // Found part of the way up, the head is wherever that way put it.
        let allowed = if beside_base {
            limits.start_reach
        } else {
            limits.start_reach.max(150)
        };
        if may_wake && apart > f32::from(allowed.max(WAKE_REACH)) {
            // `--limp on`: a head roll or a base that a hand left far off,
            // as after the lamp slumped, is taken hold of where it lies, as
            // V1 did, and it runs from there: its home moved there. Never
            // the head's pitch, which carries the head.
            if limits.limp && index != 0 {
                let moved = i32::from(home[index]) + i32::from(found_pose[index])
                    - home_found[index].round() as i32;
                home[index] = moved.clamp(0, 4_095) as u16;
                home_found[index] = f32::from(found_pose[index]);
                report.home = Some(home);
                continue;
            }
            report.stop = refused(format!(
                "{joint} is {apart:.0} counts from its home, more than waking brings back ({WAKE_REACH})"
            ));
            return report;
        }
        if may_wake && apart > f32::from(allowed) {
            to_wake = true;
        } else if apart > f32::from(allowed) {
            report.stop = refused(format!("{joint} is {apart:.0} counts from its home"));
            return report;
        }
    }

    // An overload latched before this run leaves a servo pushing at its
    // goal with a fraction of its torque: on lamp-4ace the elbow read a
    // load of exactly 200 for a day, three counts short of its goal, and
    // writing torque alone did not end that. Such a joint cannot carry the
    // lamp up. So first it is told to stop pushing, its goal becoming where
    // it is; if the flag is still there after half a second torque is
    // written on, and then off and on. If it stays, the lamp stays folded.
    if standing.is_some() {
        for (index, joint) in ARM.into_iter().enumerate() {
            let flagged =
                |found: &[Sample; 5]| found[joint.index()].status & StatusFlags::OVERLOAD != 0;
            if !flagged(&found) {
                continue;
            }
            let at = found[joint.index()].position;
            // Stop pushing; then torque on; then torque off and on. After
            // each, a few steps for the flag to go.
            let attempts: [(bool, &[u8], u8); 3] =
                [(true, &[], 25), (false, &[1], 10), (false, &[0, 1], 10)];
            for (stop_pushing, torques, waits) in attempts {
                let mut outcome = Ok(());
                if stop_pushing {
                    outcome = bus.write_goal(joint, at, false);
                    arm_written[index] = at;
                }
                for value in torques {
                    if outcome.is_ok() {
                        report.torque_writes += 1;
                        outcome = bus.write_acknowledged(joint, &torque_write(joint, *value), true);
                    }
                }
                // The flag may take a moment to go once nothing is pushed.
                for _ in 0..waits {
                    if outcome.is_err() {
                        break;
                    }
                    let due = bus.runtime.now() + TICK;
                    bus.sleep_until(due);
                    match bus.samples() {
                        Ok(now) => {
                            found = now;
                            report.found = Some(found);
                            report.last = Some(found);
                            if !flagged(&found) {
                                break;
                            }
                        }
                        Err(detail) => outcome = Err(detail),
                    }
                }
                if let Err(detail) = outcome {
                    report.stop = if bus.runtime.recovery_inhibited() {
                        Stop::Cancelled
                    } else {
                        bus.refused.take().unwrap_or(Stop::Bus {
                            detail: format!("clearing the overload of {joint}: {detail}"),
                        })
                    };
                    return report;
                }
                if !flagged(&found) {
                    break;
                }
            }
            if found[joint.index()].status != 0 && standing.is_some() {
                standing = None;
                stand = 0.0;
                report.standing_refused = Some(format!(
                    "{joint} keeps status {:#04x}",
                    found[joint.index()].status
                ));
            }
        }
    }
    report.standing = standing;
    // An arm whose overload stayed is not folded: refused, left as it is.
    if standing.is_none()
        && let Some(why) = refusal.take()
    {
        report.stop = refused(why);
        return report;
    }

    // The servo's own speed bound, where it is not already the one wanted
    // (nothing to a joint left as it is).
    for (index, joint) in COMMANDED.iter().enumerate() {
        if bus.left[joint.index()] {
            continue;
        }
        let (_, acceleration, speed) = profiles[index];
        let mut outcome = Ok(());
        if speed != limits.servo_speed {
            report.profile_writes += 1;
            outcome = bus.write_checked(*joint, &speed_write(*joint, limits.servo_speed));
        }
        if outcome.is_ok() && acceleration != limits.servo_acceleration {
            report.profile_writes += 1;
            outcome = bus.write_checked(
                *joint,
                &acceleration_write(*joint, limits.servo_acceleration),
            );
        }
        if let Err(detail) = outcome {
            report.stop = Stop::Bus {
                detail: format!("profile of {joint}: {detail}"),
            };
            return report;
        }
    }
    if let Some(up) = standing {
        // Fast enough for the way at its quickest, and no more than twice that.
        let quickest = up
            .folded
            .iter()
            .zip(&up.pose[3..])
            .map(|(from, to)| f32::from(from.abs_diff(*to)) * 1.5 / up.seconds)
            .fold(0.0f32, f32::max);
        // The way up is slow by itself and a gesture is quick: the servo's
        // own bound is there for neither to be outrun.
        let arm_speed = (quickest * 2.0).clamp(800.0, 1_000.0) as u16;
        for (index, joint) in ARM.iter().enumerate() {
            let (_, acceleration, speed, _) = arm_profiles[index];
            let mut outcome = Ok(());
            if speed != arm_speed {
                report.profile_writes += 1;
                outcome = bus.write_checked(*joint, &speed_write(*joint, arm_speed));
            }
            if outcome.is_ok() && acceleration != limits.servo_acceleration {
                report.profile_writes += 1;
                outcome = bus.write_checked(
                    *joint,
                    &acceleration_write(*joint, limits.servo_acceleration),
                );
            }
            if let Err(detail) = outcome {
                report.stop = Stop::Bus {
                    detail: format!("profile of {joint}: {detail}"),
                };
                return report;
            }
        }
        match bus.arm_profiles() {
            Ok(now)
                if now.iter().all(|profile| {
                    (profile.0, profile.1, profile.2) == (1, limits.servo_acceleration, arm_speed)
                }) => {}
            Ok(now) => {
                report.stop = Stop::Bus {
                    detail: format!("arm profile read back as {now:?}"),
                };
                return report;
            }
            Err(detail) => {
                report.stop = Stop::Bus {
                    detail: format!("arm profile read back: {detail}"),
                };
                return report;
            }
        }
    }
    match bus.profiles() {
        Ok(now)
            if now.iter().zip(COMMANDED).all(|(profile, joint)| {
                bus.left[joint.index()]
                    || *profile == (1, limits.servo_acceleration, limits.servo_speed)
            }) => {}
        Ok(now) => {
            report.stop = Stop::Bus {
                detail: format!("profile read back as {now:?}"),
            };
            return report;
        }
        Err(detail) => {
            report.stop = Stop::Bus {
                detail: format!("profile read back: {detail}"),
            };
            return report;
        }
    }

    // The most the joints that carry the lamp may push with, if one was
    // chosen: a lamp that meets a hand or an edge gives way sooner, and
    // cannot push with more than this while this program is being slow to
    // notice. Written before anything moves, and read back.
    if let Some(force) = limits.force {
        let mut carrying = vec![Joint::BaseYaw];
        if standing.is_some() {
            carrying.extend(ARM);
        }
        for joint in &carrying {
            report.force_writes += 1;
            if let Err(detail) = bus.write_checked(*joint, &force_write(*joint, force)) {
                report.stop = Stop::Bus {
                    detail: format!("force limit of {joint}: {detail}"),
                };
                return report;
            }
        }
        match bus.force_limits(&carrying) {
            Ok(now) if now.iter().all(|held| *held == force) => report.force = Some(force),
            Ok(now) => {
                report.stop = Stop::Bus {
                    detail: format!("force limit read back as {now:?}"),
                };
                return report;
            }
            Err(detail) => {
                report.stop = Stop::Bus {
                    detail: format!("force limit read back: {detail}"),
                };
                return report;
            }
        }
    }

    // Woken from wherever it was found: brought home slowly, the way
    // something alive wakes, before the body takes it as found.
    let found_pose = if to_wake {
        let to = home_found.map(|count| count.round() as u16);
        match wake(
            bus,
            &mut report,
            &limits,
            (found_pose, to),
            standing.map(|_| arm_written),
            found,
            // After a wait for a hand (#2602) the lamp has said it is ready,
            // so the watchdog is armed: each step of the wake is watched.
            if watched {
                Some((
                    &mut watch,
                    Glimpse {
                        stands: standing.is_some(),
                        stand,
                        arm_goals: arm_written,
                        ..Glimpse::default()
                    },
                ))
            } else {
                None
            },
        ) {
            // The run takes over the goals the wake left, home, where the
            // servos have them: a head that still lags its goal by a few
            // counts is not asked to drop back to where it is. A joint that
            // settled farther short than the run's own bound runs from where
            // it settled: its goal becomes where it is, so that nothing keeps
            // pushing, and its home moves there by as much, so that moods
            // are laid on the pose it has and the gap is not tried again.
            Ok(now) => {
                found = now;
                let mut goals = to;
                for (index, joint) in COMMANDED.iter().enumerate() {
                    let at = found[joint.index()].position;
                    if at.abs_diff(to[index]) > limits.tracking {
                        goals[index] = at;
                        let shifted = i32::from(home[index]) + i32::from(at) - i32::from(to[index]);
                        home[index] = shifted.clamp(0, 4_095) as u16;
                    }
                }
                if goals != to {
                    if let Err(detail) = bus.send_holds(goals, standing.map(|_| arm_written)) {
                        report.stop = bus.refused.take().unwrap_or(Stop::Bus {
                            detail: format!("waking: {detail}"),
                        });
                        return report;
                    }
                    report.goal_writes += 1;
                    report.home = Some(home);
                }
                goals
            }
            Err(stop) => {
                // Stop pushing: each goal becomes where its joint is now.
                if let Some(last) = report.last {
                    let hold = COMMANDED.map(|joint| last[joint.index()].position);
                    if bus.send_holds(hold, standing.map(|_| arm_written)).is_ok() {
                        report.goal_writes += 1;
                        report.held_at = Some(hold);
                    }
                }
                report.stop = stop;
                return report;
            }
        }
    } else {
        found_pose
    };
    // Home as the wake left it; letting go under overload may move it again.

    // The reach around home, and the absolute window for the head's pitch
    // while the head is beside the base. A lamp that stands carries its
    // home with it, so these move as it rises. Sideways the eyes may take
    // the head further than any pose does: a person is where they are.
    let yaw_reach = limits
        .yaw
        .max(looking.map_or(0.0, |looking| looking.reach[0]));
    let bounds = |home: [f32; 3], stand: f32| -> ([f32; 3], [f32; 3]) {
        let (floor, ceiling) = if head_share(stand) > 0.0 {
            (f32::from(limits.pitch_floor), 4_095.0)
        } else {
            (
                f32::from(limits.pitch_window.0),
                f32::from(limits.pitch_window.1),
            )
        };
        // Folded, the head can dip only a little before it meets the base.
        // Stood up there is nothing under it, so it may look down as far as
        // it may look up; in between, in proportion to how clear it is.
        let clear = head_share(stand).powi(2);
        let down = limits.pitch_down + (limits.pitch_up - limits.pitch_down).max(0.0) * clear;
        let roll = limits.roll + (limits.stand_roll - limits.roll).max(0.0) * clear;
        (
            [
                (home[0] - limits.pitch_up).max(floor),
                home[1] - roll,
                home[2] - yaw_reach,
            ],
            [
                (home[0] + down).min(ceiling),
                home[1] + roll,
                home[2] + yaw_reach,
            ],
        )
    };
    let mut home_now = home_with(home, standing, stand);
    let (mut low, mut high) = bounds(home_now, stand);
    let mut stand_velocity = 0.0f32;
    let mut awake_until: Option<Duration> = None;
    let mut tired_until: Option<Duration> = None;
    let mut retreats = 0u8;
    // When each retreat inside the last `Standing::RETREAT_WINDOW` was.
    let mut retreated_at: std::collections::VecDeque<Duration> = std::collections::VecDeque::new();
    let mut arm_over = [0u8; 2];
    let mut arm_off = [0u8; 2];
    let mut arm_hot = [0u8; 2];
    // A folded arm is not pushed at: once it has been down for a second its
    // goals become where its joints are, until it next rises. A goal a few
    // counts past where a folded joint can go is what latched the elbow's
    // overload on lamp-4ace.
    let mut arm_rests = stand <= 0.0;
    // Each arm joint came down to where it reads without a jump from one
    // read to the next over `ARM_GOAL_STEP`, since the lamp last stood or
    // rose: a misread under `READ_JUMP` jumps, an arm stopping short does
    // not.
    let mut arm_came = [true; 2];
    let mut down_steps = 0u16;
    // Where the eyes have it look, yaw and pitch in counts, and for how
    // many steps no joint has been asked to move.
    let mut gaze = looking.map(Gaze::new);
    let mut look = [0.0f32; 2];
    // The lease holder's look as last taken, to wake the head only when it
    // changes (the Reviewer-lead's P2 on #4023).
    let mut aim_was: Option<[f32; 2]> = None;
    let mut still_steps = 0u32;
    // The gesture being made and one waiting to be, each with when; the
    // offsets it has the five joints at; and steps left of the allowance a
    // gesture is given.
    let mut playing: Option<(&'static Clip, Duration)> = None;
    let mut pending: Option<(&'static Clip, Duration)> = None;
    // Who asked for the gesture playing and the one waiting (weighed
    // against each other only under `Limits::arbitration`), each priority with the name its asker gave (the
    // body's own turn to a call is the conversation's, and names nobody),
    // and whether one ended for another still eases out.
    let (mut playing_by, mut pending_by): ((Priority, Option<Source>), (Priority, Option<Source>)) = (
        (Priority::Conversation, None),
        (Priority::Conversation, None),
    );
    let mut yielding = false;
    // Such a gesture's ease back: steps since it ended, its offsets then and
    // how far they moved in the step before, counts a step.
    let mut easing_out: Option<(u32, [f32; 5], [f32; 5])> = None;
    let mut gesture_moved = [0.0f32; 5];
    // The gesture waiting stood a folded lamp up; and what to say of a
    // gesture asked from outside, on the next glimpse.
    let mut pending_woke = false;
    // The awake window from before the gesture waiting was taken, and the
    // one it opened: a pause gives that one back, so the lamp folds.
    let mut pending_window: Option<(Option<Duration>, Option<Duration>)> = None;
    let mut gesture_fates = Fates::default();
    let mut gesture_now = [0.0f32; 5];
    let mut gesture_cooling = 0u16;
    // Steps are being cut short by the anti-tip rule.
    let mut tipping = false;
    // Looks in a row in which its eyes reported a face.
    let mut faces_running = 0u8;
    // When a lamp standing at rest next shifts by itself, what draws when
    // and how.
    let mut idle_due: Option<Duration> = None;
    let mut idler = idle::Idler::new(standing.map_or(0, |up| up.idle_seed));
    // Expression's rest offsets of the head (pitch, roll) averaged over a
    // breath, the average a drift last left, and when one last did.
    let mut rest_average: Option<([f32; 2], [f32; 2])> = None;
    let mut drifted_at: Option<Duration> = None;
    // Expression's rest offsets as a shift began, while one plays.
    let mut shift_from: Option<[f32; 3]> = None;
    let step = TICK.as_secs_f32();
    // A look away begins with a glance of GLANCE counts of the base's
    // turn: within the folded reach that is all of it, as it always was.
    let mut gestures = Gestures::new().glancing(GLANCE / limits.yaw.max(1.0));
    // It starts from where it is, not from where it should be: the way home
    // is travelled like any other move. Until it is inside the reach, the
    // only bound that applies is that it never moves away from it.
    let mut commanded = found_pose.map(f32::from);
    let mut velocity = [0.0f32; 3];
    let mut written = found_pose;
    let mut hot = [0u8; 3];
    let mut heard: Option<(Mood, u8, Duration)> = None;
    // Under `Limits::look_at_name`: when the last call was taken.
    let mut called_at: Option<Duration> = None;
    // The last step's mood was a conversation's (listening, thinking or
    // speaking): one beginning ends an unprompted gesture.
    let mut talked = false;
    // A skill's or idle gesture paused as listening began (#2602): when,
    // when the gesture playing then began (it is that one's pause only),
    // and its offsets and their speed a step then, which it eases to a hold
    // from.
    let mut paused: Option<(Duration, Duration, [f32; 5], [f32; 5])> = None;
    // One that went on again: when, and the offsets it was held at, which
    // it eases from into its clip over `EASE_OUT` so it starts from still.
    let mut resumed: Option<(Duration, [f32; 5])> = None;
    let mut over_load = [0u8; 3];
    let mut off_track = [0u8; 3];
    // With `--wake on`: the goal and the position a joint short of its goal
    // was last seen at, and the steps since, while it stays short.
    let mut short_at = [(0u16, 0u16); 3];
    let mut short_steps = [0u8; 3];
    // Steps left for which a joint that met resistance is not asked to move,
    // and the steps on which any joint last did.
    let mut easing = [0u16; 3];
    let mut eased_at = [0u64; EASES_ALLOWED];
    // Under `--give-way`: where each joint last gave way, steps in a row a
    // hand has pushed it since, where it last stood on its track and where
    // its hold began (or a hand last left it), the offset from home a hand
    // left each where it put it, and each of the five joints' pushing
    // without moving; each commanded joint's contact floor (V1's) and steps
    // in a row over it.
    let mut gave_at = [0u16; 3];
    let mut on_track_at = [0u16; 3];
    // Each joint's last positions off its track ([`note_turn`]).
    let mut recent = [[0u16; FOLLOW_STEPS]; 3];
    let mut seen = [0usize; 3];
    // Each joint's steps off its track not following it (the tracking stop).
    let mut stuck = [0u8; 3];

    let mut held_from = [0u16; 3];
    let mut in_hand = [0u16; 3];
    // Whether a joint held where it gave way is said as in a hand
    // (`body/hand`). A hand starts on a joint more than `HAND_MOVED` from
    // where it first gave, pressed past its own load bound or, limp, its
    // contact floor, or pushed as the slump judges it (`LIMP_LAG`); once
    // started it stays while the joint is that far off, or reads over half
    // its bound and a quarter of its bound over what it read before it gave,
    // and ends on the first step that is neither. A held joint reading only
    // its own weight at its rest is not a hand: lamp-06c1's head read 124 to
    // 136 there, over half its 240, and said a push 8 times a second.
    let mut felt = [false; 3];
    // Steps left that a head or base joint the stall rule just gave way
    // under is said as a hand ([`STALL_HAND_STEPS`]).
    let mut stalled_hand = [0u16; 3];
    // What each joint read on its last step neither held, a hand nor over
    // its bound: its own weight.
    let mut rest_load = [0u16; 3];
    // Where a joint first gave way, kept while it is held and across the
    // holds of one push (`last_held`): re-held where a hand has it each
    // second, `gave_at` follows the hand, and a hand that keeps a joint off
    // its rest, pressing under its bound, is still one.
    let mut gave_first = [0u16; 3];
    // The step each joint was last held on: one that gives way again within
    // a hold's length (`EASE_STEPS`) of it is the same push, and keeps where
    // it first gave. On lamp-52e6 on 2026-10-27 a hand turned the base about
    // 30 counts a hold, each a stop-pushing of its own at loads of 104 to
    // 160 under its 220, and from where each hold began it never moved
    // `HAND_MOVED`.
    let mut last_held: [Option<u64>; 3] = [None; 3];
    // Whether a joint went over its full load bound in this hold, kept while
    // it is held: a hand pressing at 300 on a head bound 240 did; lamp-06c1's
    // head, at 130 at its rest with one read of 192, did not. The hold bound
    // (below) eases only for one that did and still presses over its own
    // rest load.
    let mut over_full = [false; 3];
    let mut kept = [0.0f32; 3];
    let mut stalls = [Stall::default(); 5];
    stalls[0].least = Some(limits.head_stall_load);
    let contact_floor = [
        CONTACT_FLOOR[0],
        CONTACT_FLOOR[1],
        report
            .force
            .map_or(CONTACT_FLOOR[2], |force| force * CONTACT_YAW_SHARE / 100),
    ];
    let mut contact = [0u8; 3];
    // Under `--limp on`, each commanded joint stopped by V1's contact floor
    // (not by `--give-way`'s lower bounds) since it last gave way: only
    // such a joint may make the lamp slump, as only V1's contact stop
    // handed over to its cut-off. On lamp-52e6 on 2026-10-15 a head lifting
    // itself after a slump (37 counts short at a load of 144) gave way by
    // the lower bound, read as a degree off, and made the lamp slump again
    // with nobody touching it, over and over.
    let mut pinned = [false; 3];
    let mut misses = 0u8;
    let mut unsent = 0u8;
    let mut leaving: Option<Duration> = None;
    // What the run ends on once a lamp leaving for it is down: an arm too
    // warm, or unreadable, is folded first and let go folded.
    let mut leave_for = fold_for;
    if leave_for.is_some() {
        leaving = Some(bus.runtime.now());
        for joint in ARM {
            bus.unjudged[joint.index()] = true;
        }
    }
    let mut resting_steps = 0u16;
    let mut relaxed = false;
    // Under `--rest-glance`: where the head and the base rested when a
    // glance took the head out of rest. The glance is laid on that, and
    // the head goes back to it; the base is left as it is.
    let mut glance_from: Option<[f32; 3]> = None;
    // Whether Expression asked a glance on the last step.
    let mut glanced = false;
    // Steps the head has been led up while below its way without coming
    // closer, and the closest to its way it has been read since it fell
    // below it.
    let mut led_below = 0u8;
    let mut closest = u16::MAX;
    // Whether the head's goal was led on the last step, for the journal.
    let mut leading = false;
    // A joint that has arrived and is asked nothing until the wanted pose
    // moves far enough from it; the steps each has been arriving for; and
    // the pose last wanted of each.
    let mut asleep = [false; 3];
    let mut arriving = [0u8; 3];
    let mut wanted_of = commanded;
    // Under `--overload release`: the load at which each of the five
    // (in servo order) trips the cut-off, steps in a row each has read
    // it, when it last let go, and whether the joints are to be left as
    // they are when the run ends (torque off, or already held). Taken up
    // again, an arm that slumped stays where it is until it is next asked
    // to stand.
    let trip_at = Joint::ALL.map(|joint| match report.force {
        Some(force) if joint == Joint::BaseYaw || (standing.is_some() && ARM.contains(&joint)) => {
            force * OVERLOAD_SHARE / 100
        }
        _ => OVERLOAD_LOAD,
    });
    let mut overloaded = [0u16; 5];
    // When an overload last let the lamp go: a slump from a hand is not one.
    let mut overload_at: Option<Duration> = None;
    // Under `--limp on`: a hand that stopped the lamp coming up again lets
    // it go again on the next step.
    let mut slump_again: Option<(Joint, u16)> = None;
    let mut left_as_is = false;
    let mut slumped = false;
    // How far into its lean a speaking lamp is, 0 to 1 (`LEAN`).
    let mut lean = 0.0f32;
    // A shoulder and an elbow taken from a hand's hold into the lean: how
    // far from the pose they are, and whether the hand is above that.
    let mut lean_kept: [Option<(f32, bool)>; 2] = [None; 2];
    let mut started = bus.runtime.now();
    // The step the schedule counts from: it starts again after a rest, and
    // begins after any step already taken (a start after a wait for a hand
    // carries the wait's steps, and its wake's and lowering's, #2602): else
    // its first step would be due that many ticks later, an unwatched sleep
    // that systemd's armed watchdog kills (the Reviewer-lead's P2 on #3168,
    // lamp-52e6's `Watchdog timeout` 4.4 s after taking hold).
    let mut first_step = report.steps;
    if let Some(up) = standing
        && stand > 0.0
    {
        // Found standing: it stays up as if it had just been spoken to.
        awake_until = Some(started + up.awake);
    }

    let mut unplug_latched = false;
    let mut unplug_previous: Option<[u16; 5]> = None;
    let mut unplug_missed = report.missed_reads;
    let stop = loop {
        report.steps += 1;
        let due = started + TICK * ((report.steps - first_step).min(u64::from(u32::MAX)) as u32);
        let now = bus.runtime.now();
        match bus.runtime.cancelled() {
            Ok(false) => {}
            // Asked to stop, or unable to tell: ease home and leave.
            Ok(true) | Err(_) => {
                leaving.get_or_insert(now);
            }
        }
        if let Some((mood, level)) = news() {
            heard = Some((mood, level, now));
        }
        // What its eyes saw and a gesture asked of it, since the last step.
        let (mut asked_now, asked_by, asked_source) = asked();
        // A hand from the test input (`--hold-from`) holds through anything
        // that replaces what is asked, as a real hand would.
        let test_held = asked_now.held_by_test;
        let latched_now = asked_now.unplug || bus.runtime.recovery_inhibited();
        // A matching "never mind" cleared the latch: it stands as when spoken
        // to, through the ordinary guarded rise. A keeper pause still wins.
        if unplug_latched
            && !latched_now
            && !asked_now.paused
            && let Some(up) = standing
        {
            awake_until = Some(now + up.awake);
            unplug_previous = None;
        }
        unplug_latched = latched_now;
        if unplug_latched {
            asked_now = Asked {
                unplug: true,
                ..Asked::PAUSED
            };
            heard = None;
            awake_until = None;
            pending_window = None;
            called_at = None;
            if pending.is_some_and(|(clip, _)| !from_outside(clip)) {
                pending = None;
            }
            // The fold for unplugging is to the configured home, not to
            // where it last looked or was put: on lamp-52e6 on 2026-10-20
            // the base rested 60 counts toward the person and the proof,
            // which reads the configured home, never came (#1699).
            if let Some(configured) = unplug_home {
                if home != configured {
                    home = configured;
                    home_now = home_with(home, standing, stand);
                    (low, high) = bounds(home_now, stand);
                }
                kept = [0.0; 3];
                glance_from = None;
                // Resting goals are kept where the joints came to rest only
                // once that rest is the configured home.
                if !(0..3).all(|index| rests_on(written[index], configured[index])) {
                    relaxed = false;
                    resting_steps = 0;
                }
            }
        }
        // Somebody in front of it: a face in several looks running.
        match asked_now.seen {
            Some(Seen { face: Some(_) }) if looking.is_some() => {
                faces_running = faces_running.saturating_add(1);
            }
            Some(Seen { face: None }) => faces_running = 0,
            _ => {}
        }
        let company = faces_running >= COMPANY_SIGHTS;
        let (mood, level) = match (leaving, heard) {
            (None, Some((mood, level, at))) if now.saturating_sub(at) < MOOD_STALE => (mood, level),
            _ => (Mood::Resting, 0),
        };
        // Called by its name, and not speaking: spoken to, for a moment.
        // With nobody in view it also glances, once it stands: as long as
        // rising takes is added to the wait for it.
        if limits.look_at_name
            && asked_now.called
            && leaving.is_none()
            && mood != Mood::Speaking
            && called_at.is_none_or(|at| now.saturating_sub(at) >= CALLED_EVERY)
        {
            called_at = Some(now);
            report.calls += 1;
            // Not over a gesture under way; over shifting at rest, yes.
            if faces_running == 0
                && playing.is_none_or(|(clip, _)| idle::is_move(clip))
                && pending.is_none()
            {
                let rising = standing.map_or(0.0, |up| up.seconds);
                pending = Some((&clips::CALLED, now + Duration::from_secs_f32(rising)));
            }
        }
        let mood = match called_at {
            Some(at) if mood == Mood::Resting && now.saturating_sub(at) < CALLED_FOR => {
                Mood::Listening
            }
            _ => mood,
        };
        let Pose { pitch, roll, yaw } =
            gestures.target(mood, level, now.as_micros().try_into().unwrap_or(u64::MAX));
        let mut goals = written;
        // Rest that has lasted: nothing is asked of the joints until the
        // mood changes, so the goals stay where the joints came to rest.
        resting_steps = if mood == Mood::Resting && leaving.is_none() {
            resting_steps.saturating_add(1)
        } else {
            0
        };
        // Under `--rest-glance`, what Expression asks of a lamp at rest
        // takes its head out of rest for as long as it is asked.
        let glance = limits.rest_glance
            && mood == Mood::Resting
            && leaving.is_none()
            && asked_now.expression.is_some();
        if glance {
            // Where it rests: a rest that has lasted, or, as a glance
            // begins, a head already still at rest that has not relaxed yet
            // (just stood up, or between the self-test's moves). Without it
            // a head that lags lifting back was not led, and arrived where
            // it stopped short: lamp-52e6 rested 17 below after the
            // factory's self-test (#2818).
            let still = !glanced && velocity.iter().all(|speed| speed.abs() < 3.0);
            if relaxed || still {
                glance_from.get_or_insert(commanded);
            }
            resting_steps = 0;
            asleep = [false; 3];
        }
        glanced = glance;
        if mood != Mood::Resting || leaving.is_some() {
            glance_from = None;
        }
        // The wire is failing: nothing new is asked until it answers again.
        let blind = misses > 0 || unsent > 0;
        // How far up it should be, and this step of the way there. The
        // head's home goes with it, and the head with its home.
        let mut carried = [0.0f32; 3];
        let mut arm_goals = arm_written;
        // Taken up again after it let go, the arm stays where it slumped
        // until something asks the lamp to stand.
        if slumped && leaving.is_none() && (mood != Mood::Resting || company || asked_now.woken) {
            slumped = false;
        }
        // A hand holds a joint where it gave way: a gesture is not begun.
        // Or the test input says a hand holds the head (`--hold-from`).
        let in_hand_now = easing.iter().any(|left| *left > 0) || test_held;
        // An unprompted gesture, a skill's or idle life's, is an invitation:
        // whatever `--arbitration`, it ends the moment a conversation begins
        // or a hand comes on the lamp, and eases out as one ended for a
        // higher ask does. One of the conversation's, a person's or
        // safety's plays on; one such waiting to begin is dropped.
        let talking = matches!(mood, Mood::Listening | Mood::Thinking | Mood::Speaking);
        let spoken_to = talking && !talked;
        talked = talking;
        let unprompted = |clip: &Clip, by: Priority| from_outside(clip) && by <= Priority::Skill;
        // #2602, under `gesture_pause`: listening only pauses it, as a
        // creature stops mid-stretch to hear; the lamp answering (thinking
        // or speaking) ends it, and so does a hold of `RESUME_WITHIN` or
        // more. Back to resting before that, with no answer, it goes on
        // from where it was. Off, a conversation beginning ends it (#2668).
        paused = paused.filter(|(_, of, ..)| playing.is_some_and(|(_, since)| since == *of));
        let answering = matches!(mood, Mood::Thinking | Mood::Speaking);
        let ends = if in_hand_now {
            Some(crate::moves::ENDED_BY_HAND)
        } else if let Some((at, ..)) = paused {
            (answering || now.saturating_sub(at) >= RESUME_WITHIN)
                .then(|| crate::moves::ended(Priority::Conversation))
        } else {
            (spoken_to && (answering || !limits.gesture_pause))
                .then(|| crate::moves::ended(Priority::Conversation))
        };
        if ends.is_none()
            && !talking
            && let Some((at, _, from, speed)) = paused.take()
            && let Some((clip, since)) = playing
        {
            // Its clock goes on from where the hold carried it, and its
            // pose from where it is held.
            let hold = now.saturating_sub(at);
            let (offsets, along) = held_at(hold, from, speed);
            playing = Some((clip, since + hold.saturating_sub(along)));
            resumed = Some((now, offsets));
        }
        if ends.is_none()
            && limits.gesture_pause
            && spoken_to
            && let Some((_, since)) = playing.filter(|(old, _)| unprompted(old, playing_by.0))
        {
            paused = Some((now, since, gesture_now, gesture_moved));
        }
        // One still waiting to begin goes as a conversation begins, so it
        // never begins over the person (#2668); off, the ends below drops it.
        if limits.gesture_pause
            && spoken_to
            && !in_hand_now
            && let Some((old, _)) = pending.filter(|(old, _)| unprompted(old, pending_by.0))
        {
            gesture_fates.push_told(
                (old.name, crate::moves::busy(Priority::Conversation)),
                Told {
                    source: pending_by.1,
                    by: None,
                },
            );
            pending = None;
        }
        if let Some(why) = ends {
            paused = None;
            resumed = None;
            if let Some((old, _)) = playing.filter(|(old, _)| unprompted(old, playing_by.0)) {
                gesture_fates.push_told(
                    (old.name, why),
                    Told {
                        source: playing_by.1,
                        by: None,
                    },
                );
                playing = None;
                yielding = true;
                easing_out = Some((0, gesture_now, gesture_moved));
            }
            // One still waiting goes too, so it never begins after the ease
            // out only to end a step later under the same hand.
            if let Some((old, _)) = pending.filter(|(old, _)| unprompted(old, pending_by.0)) {
                let why = if !in_hand_now {
                    crate::moves::busy(Priority::Conversation)
                } else if limits.arbitration {
                    "dropped: held by a hand"
                } else {
                    "dropped: a hand holds it"
                };
                gesture_fates.push_told(
                    (old.name, why),
                    Told {
                        source: pending_by.1,
                        by: None,
                    },
                );
                pending = None;
            }
        }
        // A gesture asked of a folded lamp stands it up first, as any
        // mood but rest does (#1011); not a lamp that is stopping, held,
        // slumped or paused.
        let mut woke_for_gesture = false;
        let mut window_before_ask = awake_until;
        // A lamp that is folded, or folding, looks at nobody.
        let mut awake_now = leaving.is_none() && !slumped;
        if let Some(up) = standing.filter(|_| !slumped) {
            // Spoken to, or with somebody sitting in front of it: awake.
            // It does not fold away from a person who is still there.
            if asked_now.asleep {
                // Put to sleep by a hand on its head (#1362): it folds now,
                // whoever is there, once a gesture from outside that already
                // plays has ended; one still waiting is dropped and said.
                if let Some((clip, _)) = pending.filter(|(clip, _)| from_outside(clip)) {
                    pending = None;
                    gesture_fates.push_told(
                        (clip.name, "dropped: asleep"),
                        Told {
                            source: pending_by.1,
                            by: None,
                        },
                    );
                }
                pending_window = None;
                awake_until = playing
                    .filter(|(clip, _)| from_outside(clip))
                    .map(|(clip, began)| began + Duration::from_micros(clip.duration_us()));
            } else if mood != Mood::Resting || company || asked_now.woken {
                awake_until = Some(now + up.awake);
            } else if asked_now.held && awake_until.is_some_and(|until| now < until) {
                // Held by lamp-control's lease (#3504): a lamp that is up
                // stays up while a program moves it; a folded one is not
                // stood up by a hold alone.
                awake_until = Some(now + up.awake);
            }
            // Paused after a gesture was taken: one still waiting is
            // dropped, and the window it opened is given back (keeper.md: a
            // paused body goes to rest). One already playing ends, and the
            // lamp folds once it has.
            if asked_now.paused {
                if let Some((clip, _)) = pending.filter(|(clip, _)| from_outside(clip)) {
                    pending = None;
                    gesture_fates.push_told(
                        (clip.name, "dropped: paused"),
                        Told {
                            source: pending_by.1,
                            by: None,
                        },
                    );
                }
                if let Some((before, opened)) = pending_window.take()
                    && awake_until == opened
                {
                    let ends = playing
                        .filter(|(clip, _)| from_outside(clip))
                        .map(|(clip, began)| began + Duration::from_micros(clip.duration_us()));
                    awake_until = before.max(ends);
                }
            }
            window_before_ask = awake_until;
            if asked_now.gesture.is_some()
                && !asked_now.paused
                && !asked_now.asleep
                && leaving.is_none()
                && !in_hand_now
                && awake_until.is_none_or(|until| now >= until)
            {
                awake_until = Some(now + up.awake);
                woke_for_gesture = true;
            }
            let tired = tired_until.is_some_and(|until| now < until);
            let awake = awake_until.is_some_and(|until| now < until);
            let wanted = if leaving.is_none() && awake && !tired && !unplug_latched {
                up.share
            } else {
                0.0
            };
            awake_now = wanted > 0.0;
            if !blind {
                let rate = 1.0 / up.seconds;
                let pull = FOLLOW * FOLLOW * (wanted - stand) - 2.0 * FOLLOW * stand_velocity;
                stand_velocity = (stand_velocity + pull.clamp(-rate * 2.0, rate * 2.0) * step)
                    .clamp(-rate, rate);
                stand = (stand + stand_velocity * step).clamp(0.0, 1.0);
                if (wanted - stand).abs() < 0.000_5 && stand_velocity.abs() < 0.005 {
                    stand = wanted;
                    stand_velocity = 0.0;
                }
            }
            if stand > 0.05 {
                report.stood = true;
            }
            let lift = smooth(stand);
            down_steps = if stand <= 0.0 && stand_velocity == 0.0 {
                down_steps.saturating_add(1)
            } else {
                0
            };
            if down_steps == 0 {
                arm_rests = false;
            } else if down_steps >= REST_STEPS
                && !arm_rests
                && !blind
                && let Some(last) = report.last
            {
                arm_rests = true;
                // Its resting goal keeps torque on: where it reads, if that
                // is within a step of the goal it had, or farther only while
                // it pushes there (over `STALL_LOAD`) and came there without
                // a jump (`arm_came`), as an arm resting short of its fold on
                // a mat or a stiff joint does. A misread jumps, or leaves the
                // arm at its goal pushing at nothing, and keeps that goal.
                arm_goals = std::array::from_fn(|index| {
                    let is = last[ARM[index].index()];
                    let at = is.position;
                    if at.abs_diff(arm_written[index]) > ARM_GOAL_STEP
                        && (is.load <= STALL_LOAD || !arm_came[index])
                    {
                        arm_written[index]
                    } else {
                        at
                    }
                });
            }
            if !arm_rests {
                arm_goals = std::array::from_fn(|index| {
                    let (from, to) = (f32::from(up.folded[index]), f32::from(up.pose[3 + index]));
                    (from + (to - from) * lift).round() as u16
                });
            }
            let next = home_with(home, standing, stand);
            carried = std::array::from_fn(|index| next[index] - home_now[index]);
            home_now = next;
            (low, high) = bounds(home_now, stand);
            if stand_velocity != 0.0 || wanted != stand {
                // On its way up or down nothing of the head is left resting
                // where it was: it has to come along.
                resting_steps = 0;
                glance_from = None;
                asleep = [false; 3];
            }
        }
        if resting_steps == 0 {
            relaxed = false;
        }
        // A gesture is for a lamp standing still at its full height.
        let upright = leaving.is_none()
            && !blind
            && standing.is_some_and(|up| up.share >= 1.0 && stand >= 1.0 && stand_velocity == 0.0);
        // Anything alive that stands with nothing to do shifts a little
        // now and then, not on a clock: after a wait drawn at random, and each
        // time a different small move (`crate::idle`). It stops the moment there is something to do:
        // a mood, a call, another gesture, a hand on the head or the base,
        // stillness asked (Expression holding, or a pause). Each of those
        // starts the wait again, so no shift comes right after one.
        // Expression's rest stream (zeros or a breath, renewed every step
        // under `--rest-glance`) does not: the shift is laid over its
        // offsets while it plays. A drift of that stream holds off the
        // start of a shift for `SHIFT_QUIET`, without starting the wait over.
        match (asked_now.expression, rest_average.as_mut()) {
            (Some(asked), Some((average, mark))) => {
                let share = step / REST_AVERAGE.as_secs_f32();
                for axis in 0..2 {
                    if asked[axis].is_finite() {
                        average[axis] += (asked[axis] - average[axis]) * share;
                    }
                }
                if (0..2).any(|axis| (average[axis] - mark[axis]).abs() >= REST_DRIFTED) {
                    *mark = *average;
                    drifted_at = Some(now);
                }
            }
            (Some(asked), None) if asked[..2].iter().all(|value| value.is_finite()) => {
                rest_average = Some(([asked[0], asked[1]], [asked[0], asked[1]]));
            }
            (Some(_), None) => {}
            (None, _) => rest_average = None,
        }
        let settled = drifted_at.is_none_or(|at| now.saturating_sub(at) >= SHIFT_QUIET);
        let idling = playing.is_some_and(|(clip, _)| idle::is_move(clip));
        let quiet = upright
            && mood == Mood::Resting
            && pending.is_none()
            && (playing.is_none() || idling)
            && !asked_now.still
            && easing == [0; 3]
            && in_hand == [0; 3]
            && !test_held;
        if !quiet {
            idle_due = None;
            if idling {
                playing = None;
            }
        } else if playing.is_none() {
            match idle_due {
                None => idle_due = Some(now + idler.gap()),
                Some(due) if now >= due && settled && standing.is_some_and(|up| up.idle) => {
                    // An arm a hand holds is not swayed into the hand.
                    let arm_free = stalls[3..].iter().all(|stall| stall.hold.is_none());
                    let tilts = limits.stand_roll > limits.roll;
                    playing = Some((idler.pick(tilts, arm_free), now));
                    report.idles += 1;
                    idle_due = None;
                }
                Some(_) => {}
            }
        }
        if let Some(clip) = asked_now.gesture {
            // Why it is dropped, if it is, and who holds the body then: a
            // skill or the conversation that named itself, or nobody.
            let dropped = if asked_now.paused {
                Some(("dropped: paused", None))
            } else if asked_now.asleep {
                Some(("dropped: asleep", None))
            } else if leaving.is_some() {
                Some(("dropped: it is stopping", None))
            } else if slumped {
                Some(("dropped: it let go and lies where it slumped", None))
            } else if in_hand_now {
                Some((
                    if limits.arbitration {
                        "dropped: held by a hand"
                    } else {
                        "dropped: a hand holds it"
                    },
                    None,
                ))
            } else if standing.is_none() {
                Some(("dropped: it does not stand this run", None))
            } else if !(playing.is_none() || idling) {
                let by = playing
                    .filter(|(clip, _)| from_outside(clip))
                    .map_or((Priority::Conversation, None), |_| playing_by);
                if !limits.arbitration {
                    Some(("dropped: another gesture plays", None))
                } else if asked_by > by.0 {
                    None
                } else {
                    Some((crate::moves::busy(by.0), by.1))
                }
            } else if let Some((old, _)) = pending.filter(|_| limits.arbitration) {
                let by = if from_outside(old) {
                    pending_by
                } else {
                    (Priority::Conversation, None)
                };
                (asked_by < by.0).then(|| (crate::moves::busy(by.0), by.1))
            } else {
                None
            };
            match dropped {
                Some((why, by)) => gesture_fates.push_told(
                    (clip.name, why),
                    Told {
                        source: asked_source,
                        by,
                    },
                ),
                None => {
                    // Asked over a gesture playing: it ends now and eases
                    // out at the gesture's own step, and this one waits
                    // until it has.
                    let ender = Told {
                        source: None,
                        by: asked_source,
                    };
                    if let Some((old, _)) = playing.filter(|(old, _)| !idle::is_move(old)) {
                        if from_outside(old) {
                            gesture_fates.push_told(
                                (old.name, crate::moves::ended(asked_by)),
                                Told {
                                    source: playing_by.1,
                                    ..ender
                                },
                            );
                        }
                        playing = None;
                        yielding = true;
                        easing_out = Some((0, gesture_now, gesture_moved));
                    }
                    if let Some((old, _)) = pending.filter(|(old, _)| from_outside(old)) {
                        gesture_fates.push_told(
                            (old.name, "dropped: another was asked after it"),
                            Told {
                                source: pending_by.1,
                                ..ender
                            },
                        );
                    }
                    // Woken, it waits as long as rising takes; still
                    // rising, as long as the rest of the rise; and then as
                    // long as any gesture waits for a rising lamp. Never
                    // less than the gesture it replaces was given.
                    let rising = standing.map_or(0.0, |up| {
                        if woke_for_gesture {
                            up.seconds
                        } else {
                            (1.0 - stand).max(0.0) * up.seconds
                        }
                    });
                    let earlier = pending;
                    let begins = (now + Duration::from_secs_f32(rising))
                        .max(earlier.map_or(now, |(_, since)| since));
                    pending = Some((clip, begins));
                    pending_by = (asked_by, asked_source);
                    // It stays up for the whole of it: a window that ran
                    // out mid-clip, or `--awake-seconds` shorter than the
                    // rise, would fold it under the gesture.
                    let needed = begins + GESTURE_WAIT + Duration::from_micros(clip.duration_us());
                    awake_until = awake_until.max(Some(needed));
                    // One asked while an earlier one's window still stands,
                    // played or replaced, keeps the window from before both.
                    let before = match pending_window {
                        Some((b, opened)) if opened == window_before_ask => b,
                        _ => window_before_ask,
                    };
                    pending_window = Some((before, awake_until));
                    pending_woke = woke_for_gesture
                        || (earlier.is_some_and(|(old, _)| from_outside(old)) && pending_woke);
                }
            }
        }
        let mut began_woke = None;
        if let Some((clip, since)) = pending {
            if upright && !yielding {
                playing = Some((clip, now));
                playing_by = pending_by;
                pending = None;
                report.gestures += 1;
                began_woke = Some(pending_woke);
            } else if now.saturating_sub(since) >= GESTURE_WAIT {
                pending = None;
                if from_outside(clip) {
                    gesture_fates.push_told(
                        (
                            clip.name,
                            "dropped: stale, it was not standing 3 s after it could have been",
                        ),
                        Told {
                            source: pending_by.1,
                            by: None,
                        },
                    );
                }
            }
        }
        // V1's anti-tip rule (src/cog.rs, V1 `check_stable`): a gesture any
        // frame of which, laid over the pose the lamp is in, takes its
        // centre of gravity farther than 22 mm off the base's axis is
        // refused whole before it plays, and one within 85% of that is
        // said.
        if let Some((clip, since)) = playing
            && since == now
        {
            let pose = [
                written[0],
                written[1],
                written[2],
                arm_written[0],
                arm_written[1],
            ];
            let (frame, mm) = cog::worst_frame(pose, clip.frames);
            let at = Some((clip.name, frame.try_into().unwrap_or(u16::MAX), mm as f32));
            if mm > cog::CLIP_MM {
                playing = None;
                report.tipping.refused += 1;
                report.tipping.last_refused = at;
                if from_outside(clip) {
                    gesture_fates.push_told(
                        (clip.name, "dropped: it would tip"),
                        Told {
                            source: playing_by.1,
                            by: None,
                        },
                    );
                }
            } else if mm >= cog::CLIP_MM * cog::WARN_SHARE {
                report.tipping.close += 1;
                report.tipping.last_close = at;
            }
        }
        // Taken, unless the anti-tip rule refused it just now (said there).
        if let (Some(woke), Some((clip, since))) = (began_woke, playing)
            && since == now
            && from_outside(clip)
        {
            gesture_fates.push((
                clip.name,
                if woke {
                    "taken: it was folded and stood up for it"
                } else {
                    "taken: it plays"
                },
            ));
        }
        // A face found while it glances for its caller: the gaze has them.
        if faces_running > 0 {
            for clip in [&mut playing, &mut pending] {
                if clip.is_some_and(|(clip, _)| clip.name == clips::CALLED.name) {
                    *clip = None;
                }
            }
        }
        let mut gesture_wanted = [0.0f32; 5];
        if let Some((clip, since)) = playing {
            let elapsed = now.saturating_sub(since).as_micros();
            match (paused, clip.at(elapsed.try_into().unwrap_or(u64::MAX))) {
                // Paused (#2602): eased to a hold where it is; its own clock
                // stands still meanwhile.
                (Some((at, _, from, speed)), _) if upright => {
                    gesture_wanted = held_at(now.saturating_sub(at), from, speed).0;
                }
                // Gone on again: from the held pose into the clip, eased in
                // from still over `EASE_OUT` (a smoothstep), the clip
                // already taken up from where the hold carried it.
                (None, Some(offsets)) if upright => {
                    gesture_wanted = offsets;
                    if let Some((at, held)) = resumed {
                        let t = now.saturating_sub(at).as_secs_f32() / EASE_OUT.as_secs_f32();
                        if t < 1.0 {
                            let into = t * t * (3.0 - 2.0 * t);
                            gesture_wanted = std::array::from_fn(|index| {
                                held[index] + (offsets[index] - held[index]) * into
                            });
                        } else {
                            resumed = None;
                        }
                    }
                }
                _ => playing = None,
            }
        }
        let before = gesture_now;
        match easing_out.as_mut().filter(|_| playing.is_none()) {
            Some((steps, from, speed)) => {
                *steps += 1;
                let span = EASE_OUT.as_secs_f32() / TICK.as_secs_f32();
                let t = (*steps as f32 / span).min(1.0);
                // Hermite: from `from` at the speed it had to nothing at rest.
                let (stays, carries) = (
                    2.0 * t * t * t - 3.0 * t * t + 1.0,
                    t * t * t - 2.0 * t * t + t,
                );
                for (index, now_at) in gesture_now.iter_mut().enumerate() {
                    let wanted = from[index] * stays + speed[index] * span * carries;
                    *now_at += (wanted - *now_at).clamp(-GESTURE_STEP[index], GESTURE_STEP[index]);
                }
                if t >= 1.0 {
                    gesture_now = [0.0; 5];
                    easing_out = None;
                }
            }
            None => {
                easing_out = None;
                for (index, now_at) in gesture_now.iter_mut().enumerate() {
                    let wanted =
                        gesture_wanted[index].clamp(-GESTURE_REACH[index], GESTURE_REACH[index]);
                    *now_at += (wanted - *now_at).clamp(-GESTURE_STEP[index], GESTURE_STEP[index]);
                }
            }
        }
        gesture_moved = std::array::from_fn(|index| gesture_now[index] - before[index]);
        let gesturing = playing.is_some() || gesture_now != [0.0; 5];
        yielding &= gesturing;
        // A glance is a quick move of the head, and is judged as a gesture
        // is: a head that lags or strains lets the move go, not the run.
        gesture_cooling = if gesturing || glance_from.is_some() {
            GESTURE_COOL_STEPS
        } else {
            gesture_cooling.saturating_sub(1)
        };
        if gesturing {
            // Nothing of the head is left resting: it has to make the move.
            resting_steps = 0;
            relaxed = false;
            asleep = [false; 3];
            for (goal, offset) in arm_goals.iter_mut().zip(&gesture_now[3..]) {
                *goal = (f32::from(*goal) + offset).round().clamp(0.0, 4_095.0) as u16;
            }
        }
        // Speaking or reading, standing up and doing nothing else with its
        // arm, it leans forward, and back once it is not. While a hand
        // holds the arm and the lamp still wants to lean, the lean stays
        // exactly where it was, neither in nor out: the hold below lasts
        // while what is wanted of the arm does not change, and a lean
        // easing out by itself would end it with the hand still there and
        // then lean into the hand again. While the wire is failing the lean
        // is kept as it stands, a held place too: nothing new, and nothing
        // dropped to be taken again once the wire answers (#1010 review, 23:39).
        // Reading, a gesture is laid over the lean instead of taking it
        // away, and the lean is held where it is while the gesture plays: a
        // tutor nodding at the page keeps looking at it (#1260 review,
        // 09:19). Held, the pose V1's anti-tip rule checked the gesture over
        // as it began (above) is the pose it plays over. Speaking, a gesture
        // still takes the lean away: that lean is toward a person, and the
        // gesture is the move made at them instead.
        let reading_through = mood == Mood::Reading && gesturing;
        let leaning = matches!(mood, Mood::Speaking | Mood::Reading)
            && upright
            && (!gesturing || reading_through);
        let rate = step / LEAN_SECONDS.max(1.5 * LEAN[1].abs() / limits.speed);
        // A hold on the arm is taken into the lean once the lean eases out
        // (the answer over, a gesture, folding), or once the lean is wanted
        // and no hand reads on that joint any more. From then on the joint
        // goes from where it was held, kept as an offset from the pose so
        // that it moves with a fold, toward where the lean wants it, by no
        // more than the lean's own step, and not at all toward a hand that
        // still reads on it. A hold let go by itself instead jumped back
        // into the lean, or stayed with nobody holding it and blocked the
        // next lean (#1010 review, 15:49 and 16:28).
        let lean_arm = standing.is_some() && !slumped && !arm_rests;
        if lean_arm && !blind && (leaning || lean > 0.0) {
            for (index, kept) in lean_kept.iter_mut().enumerate() {
                let stall = &mut stalls[3 + index];
                if let Some((was, at)) = stall.hold
                    && (!leaning || !stall.pressed)
                {
                    stall.hold = None;
                    *kept = Some((f32::from(at) - f32::from(arm_goals[index]), was > at));
                }
            }
        }
        let arm_held = stalls[3..].iter().any(|stall| stall.hold.is_some());
        let still = blind || (leaning && (arm_held || reading_through));
        if !still {
            lean = (lean + if leaning { rate } else { -rate }).clamp(0.0, 1.0);
        }
        if lean_arm && (lean > 0.0 || lean_kept.iter().any(Option::is_some)) {
            let share = smooth(lean);
            for (index, (goal, by)) in arm_goals.iter_mut().zip(LEAN).enumerate() {
                let eased = by * share;
                let offset = match lean_kept[index] {
                    Some((from, _)) if blind => from,
                    Some((from, hand_up)) => {
                        let most = by.abs() * 1.5 * rate;
                        let toward = (eased - from).clamp(-most, most);
                        let into_hand = (toward > 0.0) == hand_up;
                        // Leaving, no stall check runs and `pressed` is stale: a hand
                        // no longer keeps the offset, which would take the fold
                        // past its pose (#1010 review, 23:39).
                        let pressed = stalls[3 + index].pressed && leaving.is_none();
                        let to = if pressed && into_hand {
                            from
                        } else {
                            from + toward
                        };
                        lean_kept[index] = ((to - eased).abs() >= 0.5).then_some((to, hand_up));
                        to
                    }
                    None => eased,
                };
                *goal = (f32::from(*goal) + offset).round().clamp(0.0, 4_095.0) as u16;
            }
        } else if !blind {
            lean_kept = [None; 2];
        }
        let mut face = None;
        let looks_dropped = if gaze.is_none() {
            None
        } else if asked_now.paused {
            Some("paused")
        } else if asked_now.asleep {
            Some("asleep")
        } else if !awake_now {
            Some("folded or folding")
        } else if !matches!(mood, Mood::Listening | Mood::Speaking) {
            Some("neither listening nor speaking")
        } else {
            None
        };
        if let Some(gaze) = gaze.as_mut() {
            let seen = asked_now.seen;
            face = seen
                .and_then(|seen| seen.face)
                .map(|sight| [sight.x, sight.y, sight.size]);
            look = gaze.step(
                seen,
                mood,
                awake_now,
                u64::from(still_steps) * TICK.as_micros() as u64,
                now.as_micros().try_into().unwrap_or(u64::MAX),
            );
        }
        // Held by lamp-control's lease, its holder's look in place of the
        // gaze's, inside the same bounds every pose keeps (#3504's `look`).
        let aim = asked_now.aim.filter(|_| asked_now.held);
        match aim {
            Some(aim) => look = aim,
            // Let go, with no gaze of its own to say otherwise: back to the
            // pose's own direction, moved to as the look was.
            None if gaze.is_none() => look = [0.0; 2],
            None => {}
        }
        // A new look, or its end, is the head's to make, as a gesture is:
        // nothing of it rests, relaxed or asleep. Once there it may arrive
        // and stop being pushed at, as any pose does, until the look changes.
        if aim != aim_was {
            aim_was = aim;
            resting_steps = 0;
            relaxed = false;
            asleep = [false; 3];
        }
        // The holder's look as it is applied: what each joint's own bounds
        // cut of it comes off below.
        let mut aimed = aim.map(|_| look);
        // Each axis of a pose is a share of its reach, and where the eyes
        // have it look is added to that, inside the same reach. The head's
        // tilt has the reach of a lamp that has stood up, as far as it has.
        let roll_reach =
            limits.roll + (limits.stand_roll - limits.roll).max(0.0) * head_share(stand).powi(2);
        // What Expression asks is added to the pose the mood wants, and so
        // goes through everything that pose goes through: the reach of
        // each joint, the speed and the acceleration the pose follows at,
        // rest, and the hold after resistance. Not while a gesture plays:
        // that is the whole lamp's move, and nothing is laid over it. A
        // shift at rest is the exception, one layer with Expression's: it
        // starts and ends where the rest offsets have the head, with no dip
        // to plain rest, and half the breath goes on under it.
        let reach = if glance {
            REST_GLANCE_REACH
        } else {
            EXPRESSION_REACH
        };
        let shifting =
            gesturing && playing.map_or(shift_from.is_some(), |(clip, _)| idle::is_move(clip));
        if !shifting {
            shift_from = None;
        }
        let extra = match asked_now.expression {
            Some(asked) if !gesturing || shifting => {
                report.expression_steps += 1;
                let full = [0, 1, 2].map(|index| {
                    let asked = if asked[index].is_finite() {
                        asked[index]
                    } else {
                        0.0
                    };
                    asked.clamp(-reach[index], reach[index])
                });
                if shifting {
                    let from = *shift_from.get_or_insert(full);
                    std::array::from_fn(|index| {
                        from[index] + (full[index] - from[index]) * SHIFT_BREATH
                    })
                } else {
                    full
                }
            }
            _ => [0.0; 3],
        };
        // Reading, the head looks down at the desk in front of the lamp as
        // far as its bounds let it (stood up, as far as it may look up),
        // and holds there: not lifted to a face, not nodded.
        let head_pitch = if mood == Mood::Reading {
            limits.pitch_up.max(limits.pitch_down)
        } else {
            pitch
                * if pitch < 0.0 {
                    limits.pitch_up
                } else {
                    limits.pitch_down
                }
                + look[1]
                + extra[0]
        };
        // Reading toward a place a skill names (#1679): the base turned as
        // it would be to a face there, within the gaze's reach. Nothing
        // without a gaze, which alone knows how far a place in the picture is.
        // Never toward the side a face is seen on (#1738): the head stays
        // over the desk, and the glimpse says which place was refused.
        let now_us = now.as_micros().try_into().unwrap_or(u64::MAX);
        let (aim, toward_refused) = match (mood, asked_now.toward, gaze.as_ref()) {
            (Mood::Reading, Some(toward), Some(gaze)) if !asked_now.paused => {
                if gaze.face_toward(toward, now_us) {
                    (0.0, Some(toward))
                } else {
                    (gaze.toward(toward), None)
                }
            }
            _ => (0.0, None),
        };
        let offsets = if unplug_latched {
            [0.0; 3]
        } else {
            [
                head_pitch,
                roll * roll_reach + extra[1],
                yaw * limits.yaw + look[0] + extra[2] + aim,
            ]
        };
        // A joint that met resistance is asked nothing for now, at rest or not.
        let held = easing.map(|left| left > 0);
        for left in &mut easing {
            *left = left.saturating_sub(1);
        }
        // Asked to stop, it goes home: not to where a hand put it.
        let stay = limits.give_way == Some(GiveWay::Stay) && leaving.is_none();
        // Either way it gives, a joint that gave way may have a hand on it.
        let in_a_hand = limits.give_way.is_some() && leaving.is_none();
        // `--limp on`: a hand held a second makes the lamp slump, as V1.
        let limping = in_a_hand && limits.limp;
        if leaving.is_some() {
            kept = [0.0; 3];
            stalls.iter_mut().for_each(|stall| stall.hold = None);
        }
        // A gesture is laid over the pose, outside what follows the mood.
        // A head that does not tilt when it stands does not tilt in a
        // gesture either: on lamp-4ace that joint turns a round head about
        // its own axis, which nobody sees.
        let over_of: [f32; 3] = std::array::from_fn(|index| {
            if index == 1 && limits.stand_roll <= limits.roll {
                0.0
            } else {
                gesture_now[index]
            }
        });
        // What is wanted of each joint with the move laid over, while one
        // plays: a stall in a move is held against this (`Stall::with_move`).
        let moving_of = |wanted_of: &[f32; 3], index: usize| {
            gesturing.then(|| {
                (wanted_of[index] + over_of[index])
                    .round()
                    .clamp(0.0, 4_095.0) as u16
            })
        };
        for (index, offset) in offsets.into_iter().enumerate() {
            if blind {
                continue;
            }
            // Held from the test input (`--hold-from`): the head's pitch
            // keeps the goal it has, as a hand's hold pins it, and is asked
            // nothing. Its command is kept under that goal, so that once
            // the input lets go it moves on from there, not from before.
            if index == 0 && test_held {
                velocity[0] = 0.0;
                commanded[0] = f32::from(goals[0]) - gesture_now[0];
                continue;
            }
            if held[index] {
                velocity[index] = 0.0;
                goals[index] = commanded[index].round() as u16;
                continue;
            }
            if relaxed {
                continue;
            }
            let wanted = match glance_from {
                // The base is not turned for a glance.
                Some(_) if index == 2 => continue,
                // From where it rested, by no more than the joint's own reach
                // (a head that does not tilt folded does not tilt for it),
                // and never farther out than its bounds or where it was.
                Some(from) => {
                    let reach = if index == 1 { roll_reach } else { f32::MAX };
                    (from[index] + extra[index].clamp(-reach, reach))
                        .clamp(low[index].min(from[index]), high[index].max(from[index]))
                }
                None => {
                    commanded[index] += carried[index];
                    let unbounded = home_now[index] + kept[index] + offset;
                    let wanted = unbounded.clamp(low[index], high[index]);
                    // The base's yaw and the head's pitch carry the holder's
                    // look: what the bounds cut is cut from the look.
                    if let Some(aimed) = aimed.as_mut() {
                        match index {
                            0 => aimed[1] -= unbounded - wanted,
                            2 => aimed[0] -= unbounded - wanted,
                            _ => {}
                        }
                    }
                    wanted
                }
            };
            wanted_of[index] = wanted;
            if asleep[index] {
                if (wanted - commanded[index]).abs() < WAKE_COUNTS && leaving.is_none() {
                    continue;
                }
                asleep[index] = false;
            }
            let pull =
                FOLLOW * FOLLOW * (wanted - commanded[index]) - 2.0 * FOLLOW * velocity[index];
            velocity[index] = (velocity[index]
                + pull.clamp(-limits.acceleration, limits.acceleration) * step)
                .clamp(-limits.speed, limits.speed);
            let (floor, ceiling) = (
                low[index].min(commanded[index]),
                high[index].max(commanded[index]),
            );
            commanded[index] = (commanded[index] + velocity[index] * step).clamp(floor, ceiling);
            let least = if index == 0 {
                f32::from(limits.pitch_floor)
            } else {
                0.0
            };
            let over = over_of[index];
            // A head lifting from rest is led, never past its bounds; and so
            // is one coming back up after a glance dipped it, while it is
            // still below its way: from below, lamp-52e6's head stopped 26
            // short of where it rested and was left there (#2818).
            let last_at = report
                .last
                .map(|last| last[COMMANDED[0].index()].position)
                .filter(|at| index == 0 && f32::from(*at) > commanded[0] + ARRIVED_COUNTS);
            let below = last_at.is_some();
            // A glance is judged with a gesture's bounds, so nothing else
            // ends a lead into a hand or a stall: led without coming a count
            // closer for `GLANCE_LEAD_STILL_STEPS`, the head is no longer led
            // until it is back at its way. A head rising slowly against its
            // weight keeps closing, and keeps its lead (#2818).
            let lead = index == 0 && glance_from.is_some() && (extra[0] < 0.0 || below);
            if index == 0 {
                match last_at {
                    Some(at) if at < closest => (closest, led_below) = (at, 0),
                    Some(_) => led_below = led_below.saturating_add(u8::from(lead)),
                    None => (closest, led_below) = (u16::MAX, 0),
                }
            }
            let leads = lead && led_below <= GLANCE_LEAD_STILL_STEPS;
            let led = if leads {
                (commanded[index] - GLANCE_LIFT_LEAD).max(low[index].min(commanded[index]))
            } else {
                commanded[index]
            };
            goals[index] = (led + over).round().clamp(least, 4_095.0) as u16;
            if index == 0 && leads != leading {
                leading = leads;
                let why = match (leads, below, lead) {
                    (true, _, _) => "starts",
                    (false, false, _) => "ends: arrived",
                    (false, true, true) => "ends: no progress for 1 s",
                    (false, true, false) => "ends: the glance is over",
                };
                eprintln!(
                    "lamp-body: step {} glance lead {why}; head {:?} way {:.0} goal {}",
                    report.steps,
                    report.last.map(|last| last[COMMANDED[0].index()].position),
                    commanded[0],
                    goals[0]
                );
            }
        }
        // A joint told to stop pushing is held where it was until something
        // else is wanted of it.
        if limits.give_way.is_some() && !blind {
            for index in 0..3 {
                let moving = moving_of(&wanted_of, index);
                if let Some(at) = stalls[index].holding(wanted_of[index].round() as u16, moving) {
                    goals[index] = at;
                    // Held in a move, the move stays laid over its way, so
                    // that once let go it goes on from here, not from here
                    // plus the move again.
                    let over = if stalls[index].with_move {
                        over_of[index]
                    } else {
                        0.0
                    };
                    commanded[index] = f32::from(at) - over;
                    velocity[index] = 0.0;
                }
            }
            if standing.is_some() {
                for index in 0..2 {
                    // A resting arm's goal is kept from step to step, not
                    // made again: held where a hand lifted it, that goal
                    // stayed there for good, and on lamp-52e6 on 2026-10-15
                    // the next start found the elbow 99 counts off its way
                    // and would not stand. So what is wanted of it is what
                    // it rested at, it is held only as long as a joint that
                    // gave way is, and then goes back down; a hand still on
                    // it is given way to again.
                    let wanted = match stalls[3 + index].hold {
                        Some((was, _)) if arm_rests => was,
                        _ => arm_goals[index],
                    };
                    if arm_rests && stalls[3 + index].held >= EASE_STEPS {
                        // Back down `ARM_GOAL_STEP` a step at most, held
                        // until it is there.
                        let from = arm_goals[index];
                        arm_goals[index] = if from < wanted {
                            from.saturating_add(ARM_GOAL_STEP).min(wanted)
                        } else {
                            from.saturating_sub(ARM_GOAL_STEP).max(wanted)
                        };
                        if arm_goals[index] == wanted {
                            stalls[3 + index].hold = None;
                        }
                    } else if let Some(at) = stalls[3 + index].holding(wanted, None) {
                        arm_goals[index] = at;
                    }
                }
            }
        }
        // V1's anti-tip rule (src/cog.rs, V1 `limit_lean_back`): no step
        // leans the lamp back farther than 16 mm (V1's bound for where the
        // eyes and the moods take it, `MAX_LEAN_BACK_MM`), or 22 mm while
        // a gesture plays or it rises or folds (V1's for a commanded move,
        // `max_cog_offset_mm`; folded, a lamp sits 18 to 20 mm back by this
        // model). The step is cut short; one already past may only come
        // forward. Every goal that moves the lamp is made here.
        if goals != written || arm_goals != arm_written {
            let arm_is = match (standing, report.last) {
                (None, Some(last)) => ARM.map(|joint| last[joint.index()].position),
                _ => arm_written,
            };
            let arm_to = if standing.is_some() {
                arm_goals
            } else {
                arm_is
            };
            let by_id = |head: [u16; 3], arm: [u16; 2]| {
                [head[2], arm[0], arm[1], head[1], head[0]].map(f32::from)
            };
            let (from, to) = (by_id(written, arm_is), by_id(goals, arm_to));
            let ceiling = if gesturing || !upright {
                cog::CLIP_MM
            } else {
                cog::LOOK_MM
            };
            let (cut, share) = cog::limit_lean_back(from, to, ceiling);
            if share < 1.0 {
                // Between two u16 poses, so never negative; not clamped to
                // the seam: a count past 4,095 stays one, and the guard in
                // `send_goals` refuses it.
                let count = |value: f32| value.round() as u16;
                let mut head = [count(cut[4]), count(cut[3]), count(cut[0])];
                let mut arm = [count(cut[1]), count(cut[2])];
                // Rounded, a step that would still lean past is not taken.
                if cog::lean_back_mm(by_id(head, arm)) > ceiling.max(cog::lean_back_mm(from)) + 1e-6
                {
                    (head, arm) = (written, arm_is);
                }
                for index in 0..3 {
                    // What follows the mood is held where the step stopped,
                    // so nothing jumps once the rule lets go.
                    commanded[index] -= f32::from(goals[index]) - f32::from(head[index]);
                    velocity[index] = 0.0;
                }
                goals = head;
                if let Some(up) = standing {
                    arm_goals = arm;
                    // The arm likewise: its way up or down is taken back to
                    // where the step stopped, so the rise or fold does not
                    // run on without it and jump once the rule lets go.
                    if !arm_rests && arm != arm_to {
                        stand = unsmooth(on_the_way(up, arm).0);
                        stand_velocity = 0.0;
                        home_now = home_with(home, standing, stand);
                        (low, high) = bounds(home_now, stand);
                    }
                }
                if !tipping {
                    report.tipping.cuts += 1;
                    report.tipping.ceiling_mm = ceiling as f32;
                    report.tipping.would_mm = cog::lean_back_mm(to) as f32;
                }
            }
            // Held at the line, the next step may creep a count inside it:
            // the run of cuts ends only once what is asked is a millimetre
            // inside, so a lamp held there is counted once.
            tipping = share < 1.0 || (tipping && cog::lean_back_mm(to) > ceiling - 1.0);
        }
        // Whatever made them (the rise and the fold, a gesture starting or
        // ending, a hold let go, the rest, a shift), the arm's goals move at
        // most `ARM_GOAL_STEP` a step from the ones last written, unless they
        // hold where the arm is: the guard at the wire only stops. On
        // lamp-52e6 on 2026-10-17 a shoulder held at 2609, short of its
        // pose, was asked 2683 at once by a nod, and the lamp was let go.
        if standing.is_some() {
            for (index, joint) in ARM.into_iter().enumerate() {
                arm_goals[index] =
                    bus.eased(joint, arm_written[index], arm_goals[index], arm_rests);
            }
        }
        still_steps = if goals != written || arm_goals != arm_written {
            0
        } else {
            still_steps.saturating_add(1)
        };
        // A step that lets go again (a hand stopped the lamp coming up)
        // asks nothing new of the joints first: from a stand older than
        // the slump, the arm would be sent up into the hand.
        if slump_again.is_none() && (goals != written || arm_goals != arm_written) {
            match bus.send_goals(goals, standing.map(|_| arm_goals)) {
                Ok(()) => {
                    unsent = 0;
                    written = goals;
                    arm_written = arm_goals;
                    report.goal_writes += 1;
                    report.moved = true;
                }
                Err(detail) => {
                    // An arm goal refused is not a wire to try again. Kept
                    // on the bus: the first refusal is what the run ends on.
                    if let Some(stop) = bus.refused.take() {
                        break stop;
                    }
                    report.missed_reads += 1;
                    unsent += 1;
                    // A step not read breaks a run of overloaded ones.
                    overloaded = [0; 5];
                    if unsent >= WIRE_STEPS {
                        break Stop::Bus { detail };
                    }
                    bus.sleep_until(due);
                    continue;
                }
            }
        }
        let bad_before = (bus.bad_reads, bus.bad_temperatures);
        let samples = match bus.samples() {
            Ok(samples) => {
                misses = 0;
                samples
            }
            Err(detail) => {
                report.missed_reads += 1;
                // A shoulder or an elbow whose temperature cannot be read,
                // on a lamp that may stand: let go up, the lamp falls, and
                // held it may be heating. It folds down as when asked to
                // stop, its temperature no longer judged, and is let go
                // once down.
                // Any joint unreadable so while a standing lamp folds down
                // (seventh, #1281 12:52): it keeps folding the same way.
                if let Some(Stop::BadTemperature { joint, .. }) = bus.refused
                    && standing.is_some()
                    && let Some(unread) = Joint::ALL.into_iter().find(|unread| {
                        unread.id() == joint
                            && (ARM.contains(unread) || leaving.is_some() && stand > 0.0)
                    })
                {
                    bus.unjudged[unread.index()] = true;
                    let stop = bus.refused.take();
                    leave_for = leave_for.or(stop);
                    leaving.get_or_insert(now);
                    bus.sleep_until(due);
                    continue;
                }
                misses += 1;
                overloaded = [0; 5];
                if misses >= WIRE_STEPS || bus.refused.is_some() {
                    break bus.refused.take().unwrap_or(Stop::Bus { detail });
                }
                bus.sleep_until(due);
                continue;
            }
        };
        // A head or base joint left as it is (`Bus::left`) is wherever it
        // is: nothing is asked of it, so nothing is judged against it.
        for (index, joint) in COMMANDED.iter().enumerate() {
            if bus.left[joint.index()] {
                let at = samples[joint.index()].position;
                (written[index], commanded[index], velocity[index]) = (at, f32::from(at), 0.0);
            }
        }
        // A write below that fails changes nothing and is tried again.
        let mut failed: Option<String> = None;
        report.last = Some(samples);
        if stand > 0.0 && stand_velocity >= 0.0 {
            arm_came = [true; 2];
        }
        for (index, joint) in ARM.into_iter().enumerate() {
            arm_came[index] &= bus.steady[joint.index()];
        }
        if resting_steps >= REST_STEPS && !relaxed {
            let positions = COMMANDED.map(|joint| samples[joint.index()].position);
            let near = (0..3).all(|index| {
                rests_on(positions[index], written[index]) && velocity[index].abs() < 3.0
            });
            if near {
                let sent = positions == written
                    || match bus.send_holds(positions, standing.map(|_| arm_written)) {
                        Ok(()) => {
                            written = positions;
                            report.goal_writes += 1;
                            true
                        }
                        Err(detail) => {
                            failed = Some(detail);
                            false
                        }
                    };
                if sent {
                    relaxed = true;
                    glance_from = None;
                    commanded = positions.map(f32::from);
                    velocity = [0.0; 3];
                }
            }
        }
        // A joint that has arrived stops being pushed at, whatever the mood.
        let mut rested = written;
        let mut arrived: [Option<f32>; 3] = [None; 3];
        for index in 0..3 {
            let there = !asleep[index]
                && !held[index]
                && !relaxed
                && gesture_cooling == 0
                && leaving.is_none()
                && (wanted_of[index] - commanded[index]).abs() < ARRIVED_COUNTS
                && velocity[index].abs() < ARRIVED_SPEED;
            arriving[index] = if there {
                arriving[index].saturating_add(1)
            } else {
                0
            };
            let position = samples[COMMANDED[index].index()].position;
            if arriving[index] >= ARRIVED_STEPS && rests_on(position, written[index]) {
                let at = f32::from(position).clamp(
                    low[index].min(commanded[index]),
                    high[index].max(commanded[index]),
                );
                arrived[index] = Some(at);
                rested[index] = at.round() as u16;
            }
        }
        let sent = failed.is_none()
            && (rested == written
                || match bus.send_holds(rested, standing.map(|_| arm_written)) {
                    Ok(()) => {
                        written = rested;
                        report.goal_writes += 1;
                        true
                    }
                    Err(detail) => {
                        failed = Some(detail);
                        false
                    }
                });
        if sent {
            for index in 0..3 {
                if let Some(at) = arrived[index] {
                    asleep[index] = true;
                    arriving[index] = 0;
                    commanded[index] = at;
                    velocity[index] = 0.0;
                }
            }
        }
        match failed {
            Some(detail) => {
                // A goal refused ends the run at once; it is not tried again.
                if let Some(stop) = bus.refused.take() {
                    break stop;
                }
                report.missed_reads += 1;
                unsent += 1;
                if unsent >= WIRE_STEPS {
                    break Stop::Bus { detail };
                }
            }
            None => unsent = 0,
        }
        let mut glimpse = Glimpse {
            unplug_safe: false,
            unplug_unsafe: None,
            unplug_gap: false,
            step: report.steps,
            mood,
            goals: written,
            positions: COMMANDED.map(|joint| samples[joint.index()].position),
            loads: COMMANDED.map(|joint| samples[joint.index()].load),
            temperatures: COMMANDED.map(|joint| samples[joint.index()].temperature),
            eased: report.eased,
            stands: standing.is_some(),
            stand,
            arm_goals: arm_written,
            arm_positions: ARM.map(|joint| samples[joint.index()].position),
            arm_loads: ARM.map(|joint| samples[joint.index()].load),
            arm_temperatures: ARM.map(|joint| samples[joint.index()].temperature),
            look,
            look_aimed: aimed,
            face,
            gesture: playing.map(|(clip, _)| clip.name),
            kept_where_put: report.kept_where_put,
            stalls: report.stalls,
            released: report.released.len().try_into().unwrap_or(u16::MAX),
            limp: false,
            // A hand from the test input is said as a push too, so what
            // ends on a push (a skill's search, the voice's) can be run
            // from it (#2904).
            hand: (0..3).any(|index| (held[index] && felt[index]) || stalled_hand[index] > 0)
                || test_held,
            tipping: report.tipping,
            gesture_fates: std::mem::take(&mut gesture_fates),
            looks_dropped,
            toward_refused,
            bad_reads: bus.bad_reads,
            retreats: retreats.into(),
            resting_s: match tired_until {
                Some(until)
                    if until > now
                        && retreated_at.len() >= usize::from(Standing::RETREATS_ALLOWED) =>
                {
                    // Rounded up, so the last part second still says it rests.
                    let left = until - now;
                    u16::try_from(left.as_secs() + u64::from(left.subsec_nanos() > 0))
                        .unwrap_or(u16::MAX)
                }
                _ => 0,
            },
            waiting: None,
        };
        if !unplug_latched {
            watch(&glimpse);
        }
        // A joint pushing at a goal it does not reach, and not moving, is
        // told to stop: its goal becomes where it is. Never torque off.
        // The joints told so on this step, in servo order.
        let mut stopped_pushing = [false; 5];
        // Counted down on every step, whatever else this one does.
        stalled_hand = stalled_hand.map(|left| left.saturating_sub(1));
        if limits.give_way.is_some() && leaving.is_none() {
            for index in 0..3 {
                // A joint that gave way and may be in a hand is not pushing
                // at anything of its own: held where it is from then on, it
                // would never come back once let go.
                if held[index] {
                    stalls[index].steps = 0;
                    continue;
                }
                let is = samples[COMMANDED[index].index()];
                let stopped = stalls[index].step(
                    written[index],
                    is,
                    wanted_of[index].round() as u16,
                    moving_of(&wanted_of, index),
                );
                // A gentle hand turning the head is followed by this rule
                // alone, under the load that eases it (lamp-29e7 on 8080:
                // the roll put 34 counts over a minute at 100 to 144, its
                // bound 150, and no hand was ever said): giving way here is
                // to a hand too, said for `STALL_HAND_STEPS` from the stop,
                // when it was carried away from its goal (`Stall::carried`).
                // Not a joint stuck short where it was, nor a rest found at
                // the same place again (`again`).
                if stopped && stalls[index].carried && !stalls[index].again {
                    stalled_hand[index] = STALL_HAND_STEPS;
                }
                if stopped {
                    if !stalls[index].again {
                        report.stalls[index] = report.stalls[index].saturating_add(1);
                    }
                    stopped_pushing[COMMANDED[index].index()] = true;
                }
            }
            if standing.is_some() {
                for (index, joint) in ARM.iter().enumerate() {
                    let is = samples[joint.index()];
                    let stall = &mut stalls[3 + index];
                    if !(stand > 0.0 && stand_velocity == 0.0) {
                        stall.carries = None;
                    } else if stall.carries.is_none() && is.load <= ARM_CARRIES_MOST {
                        stall.carries = Some(is.load);
                    }
                    if stall.step(arm_written[index], is, arm_written[index], None) {
                        if !stall.again {
                            report.stalls[3 + index] = report.stalls[3 + index].saturating_add(1);
                        }
                        stopped_pushing[joint.index()] = true;
                    }
                }
            }
        }
        let mut surprise = None;
        for joint in Joint::ALL {
            let (was, is) = (found[joint.index()], samples[joint.index()]);
            if is.status != was.status && !bus.left[joint.index()] {
                surprise.get_or_insert(Stop::Status {
                    joint: joint.id(),
                    found: was.status,
                    now: is.status,
                });
            }
        }
        for joint in ARM {
            let (was, is) = (found[joint.index()], samples[joint.index()]);
            if standing.is_none() && was.position.abs_diff(is.position) > limits.bystander_drift {
                surprise.get_or_insert(Stop::Drift {
                    joint: joint.id(),
                    found: was.position,
                    now: is.position,
                });
            }
        }
        if let Some(up) = standing {
            for (index, joint) in ARM.iter().enumerate() {
                let is = samples[joint.index()];
                report.arm_peak_load[index] = report.arm_peak_load[index].max(is.load);
                report.arm_peak_temperature[index] =
                    report.arm_peak_temperature[index].max(is.temperature);
                arm_over[index] = if is.load > up.load[index] {
                    arm_over[index].saturating_add(1)
                } else {
                    0
                };
                let astray = if gesture_cooling > 0 {
                    GESTURE_ARM_TRACKING
                } else {
                    Standing::TRACKING
                };
                arm_off[index] = if arm_written[index].abs_diff(is.position) > astray {
                    arm_off[index].saturating_add(1)
                } else {
                    0
                };
                // A replaced reading holds the streak (`Bus::judge_temperatures`).
                arm_hot[index] = if bus.temperature_held[joint.index()] {
                    arm_hot[index]
                } else if is.temperature >= Standing::TEMPERATURE {
                    arm_hot[index].saturating_add(1)
                } else {
                    0
                };
                let why = if arm_over[index] >= limits.load_steps.max(1) {
                    Some(format!("{joint} load {}", is.load))
                } else if arm_off[index] >= Standing::TRACKING_STEPS {
                    Some(format!(
                        "{joint} at {} for goal {}",
                        is.position, arm_written[index]
                    ))
                } else if arm_hot[index] >= limits.temperature_steps.max(1) {
                    Some(format!("{joint} at {} degrees", is.temperature))
                } else {
                    None
                };
                let Some(why) = why else { continue };
                playing = None;
                // A living thing that cannot hold itself up sits down. It
                // goes back the way it came and rests there. Only an arm
                // still strained or astray two seconds on ends the run.
                let tired = tired_until.is_some_and(|until| now < until);
                if !tired && stand > 0.0 {
                    retreats = retreats.saturating_add(1);
                    retreated_at.retain(|at| now.saturating_sub(*at) < Standing::RETREAT_WINDOW);
                    retreated_at.push_back(now);
                    if report.retreats.len() < 16 {
                        report
                            .retreats
                            .push(format!("step {}: {why}", report.steps));
                    }
                }
                // A third retreat inside the window rests it long, and while
                // still strained the rest is never shortened.
                let rest = if retreated_at.len() >= usize::from(Standing::RETREATS_ALLOWED) {
                    Standing::LONG_REST
                } else {
                    Standing::TIRED
                };
                tired_until = Some(tired_until.map_or(now + rest, |until| until.max(now + rest)));
                if arm_over[index] >= Standing::STUCK_STEPS {
                    surprise.get_or_insert(Stop::Load {
                        joint: joint.id(),
                        load: is.load,
                    });
                } else if arm_off[index] >= Standing::STUCK_STEPS {
                    surprise.get_or_insert(Stop::Tracking {
                        joint: joint.id(),
                        goal: arm_written[index],
                        position: is.position,
                    });
                }
            }
        }
        // A joint still pushed by a hand a second after it stopped.
        let mut hand_tripped: Option<(Joint, u16)> = None;
        for (index, joint) in COMMANDED.iter().enumerate() {
            let is = samples[joint.index()];
            report.peak_load[index] = report.peak_load[index].max(is.load);
            let (strained, astray) = if gesture_cooling > 0 {
                (
                    limits.load[index].max(GESTURE_LOAD),
                    limits.tracking.max(GESTURE_TRACKING),
                )
            } else {
                (limits.load[index], limits.tracking)
            };
            over_load[index] = if is.load > strained {
                over_load[index].saturating_add(1)
            } else {
                0
            };
            if !held[index] && !felt[index] && is.load <= strained {
                rest_load[index] = is.load;
            }
            over_full[index] = held[index] && (over_full[index] || is.load > strained);
            if held[index] {
                last_held[index] = Some(report.steps);
            }
            let moved = is.position.abs_diff(gave_first[index]) > HAND_MOVED;
            // Pressure over its own rest load: over half the bound and a
            // quarter of it over its own weight. A press at 300 easing to
            // 130 on a pitch bound 240 that read 25 is; one that read 130
            // before and reads 130 again is its weight.
            let pressing =
                is.load > strained / 2 && is.load > rest_load[index].saturating_add(strained / 4);
            felt[index] = held[index]
                && (moved
                    || is.load > strained
                    // Over V1's contact floor, which pins a joint and slumps
                    // the lamp under `--limp on` (below) though a gesture
                    // allows it more: a pitch pressed at 675 in a glance,
                    // its bound 700.
                    || (limping && is.load > contact_floor[index])
                    // Once a hand, still one while `pressing`.
                    || (felt[index] && pressing));
            off_track[index] = if written[index].abs_diff(is.position) > astray {
                off_track[index].saturating_add(1)
            } else {
                on_track_at[index] = is.position;
                0
            };
            // V1's contact floor, for three steps: a hand, whatever the
            // joint's own bound has seen.
            contact[index] = if limping && is.load > contact_floor[index] {
                contact[index].saturating_add(1)
            } else {
                0
            };
            let touched = limping && contact[index] >= CONTACT_STEPS;
            if gesture_cooling > 0
                && !touched
                && (over_load[index] >= limits.load_steps.max(1)
                    || off_track[index] >= limits.tracking_steps)
            {
                // The gesture asked more of this joint than it gave: the
                // gesture is let go, and the lamp goes on as it was.
                if let Some((clip, _)) = playing.take() {
                    report.gestures_dropped += 1;
                    if from_outside(clip) {
                        gesture_fates.push_told(
                            (clip.name, "dropped: a joint could not keep up with it"),
                            Told {
                                source: playing_by.1,
                                by: None,
                            },
                        );
                    }
                }
                over_load[index] = 0;
                off_track[index] = 0;
                continue;
            }
            report.peak_temperature[index] = report.peak_temperature[index].max(is.temperature);
            // A replaced reading holds the streak (`Bus::judge_temperatures`).
            hot[index] = if bus.temperature_held[joint.index()] {
                hot[index]
            } else if is.temperature >= limits.temperature {
                hot[index].saturating_add(1)
            } else {
                0
            };
            if limping && held[index] && pinned[index] {
                // It gave way, and a hand may be on it. As V1: still pushed
                // a second on, by load over the lower of the bound that
                // stopped it and its contact floor, or held a degree off
                // where it stopped, the lamp slumps (below). A load
                // and a tracking that would end the run are what a hand
                // looks like, so nothing else is judged meanwhile.
                over_load[index] = 0;
                off_track[index] = 0;
                let pushed = is.load > strained.min(contact_floor[index])
                    || is.position.abs_diff(gave_at[index]) > LIMP_LAG;
                in_hand[index] = if pushed { in_hand[index] + 1 } else { 0 };
                if pushed {
                    // Not sent back into a hand still on it (`HAND_STEPS`),
                    // and said as one: what slumps the lamp is a push, a
                    // pitch held 13 counts off (over `LIMP_LAG`, under
                    // `HAND_MOVED`) too.
                    easing[index] = easing[index].max(HAND_STEPS);
                    felt[index] = true;
                }
                if in_hand[index] >= HAND_STEPS {
                    in_hand[index] = 0;
                    hand_tripped.get_or_insert((*joint, is.load));
                    report.released_by_hand[index] =
                        report.released_by_hand[index].saturating_add(1);
                }
                if hot[index] >= limits.temperature_steps.max(1) {
                    surprise.get_or_insert(Stop::Temperature {
                        joint: joint.id(),
                        temperature: is.temperature,
                    });
                }
                continue;
            } else if in_a_hand && held[index] {
                // It gave way, and a hand may be on it: a load and a
                // tracking that would end the run are what a hand moving
                // or holding it looks like, so they are judged over a
                // second instead. On lamp-52e6 on 2026-10-15 a hand tilting
                // the head took it 45 counts past where it had given way and
                // the run ended on tracking.
                over_load[index] = 0;
                off_track[index] = 0;
                // Judged against the lower of its load bound and what a joint
                // that gives way to a hand under that bound reads: on
                // lamp-52e6 a hand lifting the head read 100 to 130, under
                // the head's 240, and a head told to stay came back. Still
                // where its hold began, only its full bound: lamp-06c1's
                // head pitch reads 124 to 136 at its own rest (23 Oct), and
                // judged by 100 it was held for good. One that went over the
                // full bound in this hold (`over_full`) and still presses
                // over its own rest load (`pressing`, load alone: not how
                // far it stands from where it first gave) keeps the lower
                // bound: a still press at 300 easing to 130 on that bound
                // 240, from a rest of 25, is the same hand and its hold
                // renews; 06c1's head pushed at 300 from its rest of 130
                // reads its weight again.
                let still = is.position.abs_diff(held_from[index]) <= HAND_MOVED;
                let bound = if still && !(over_full[index] && pressing) {
                    strained
                } else {
                    strained.min(STALL_LOAD)
                };
                // A hand read on this step keeps it held a second more: one
                // quiet read among them (a servo whose goal is where it is
                // reads little) is not the hand gone, and a joint sent home
                // with the hand still on it pushes into it and jerks.
                if is.load > bound || is.position.abs_diff(written[index]) > HAND_MOVED {
                    easing[index] = easing[index].max(HAND_STEPS);
                }
                if let Some(moved) = in_hand_after(
                    &mut in_hand[index],
                    is,
                    bound,
                    gave_at[index],
                    written[index],
                ) {
                    // Still in the hand a second on: it gives way again
                    // where it is, moved or held.
                    commanded[index] = f32::from(is.position).clamp(
                        low[index].min(commanded[index]),
                        high[index].max(commanded[index]),
                    );
                    velocity[index] = 0.0;
                    easing[index] = EASE_STEPS;
                    gave_at[index] = is.position;
                    if stay && moved {
                        // Left where it was put, inside the same reach and
                        // window as every goal, and later moods are laid
                        // from there.
                        kept[index] = commanded[index] - home_now[index] - offsets[index];
                        held_from[index] = is.position;
                        report.kept_where_put[index] =
                            report.kept_where_put[index].saturating_add(1);
                    }
                }
                if hot[index] >= limits.temperature_steps.max(1) {
                    surprise.get_or_insert(Stop::Temperature {
                        joint: joint.id(),
                        temperature: is.temperature,
                    });
                }
                continue;
            }
            // Under `--give-way`, a joint held off its goal by more than its
            // tracking while it pushes is a hand too, under the load that
            // ends a run: lamp-52e6's base turned 38 counts by a hand read
            // 120. One still closing on its goal is only following a goal
            // that jumped faster than it turns, and its load is the move's:
            // lamp-52e6 on 27 Oct "met resistance" 98 times in four hours
            // with nobody near, each a two-second hitch in a glance.
            let following = note_turn(
                &mut recent[index],
                &mut seen[index],
                written[index],
                is.position,
                off_track[index],
            );
            // The tracking stop is for a servo stuck, not turning toward its
            // goal: steps a joint follows a goal that outruns it (judged on a
            // full window) do not count. On lamp-52e6 (28 Oct, 09:39) a base
            // following a goal faster than it turns, no longer given way as a
            // hand, stayed over 30 off for 10 steps, ended on tracking, let go
            // and the standing lamp fell with nobody near.
            stuck[index] = if off_track[index] <= 1 {
                off_track[index]
            } else if following && seen[index] > FOLLOW_STEPS {
                0
            } else {
                stuck[index].saturating_add(1)
            };
            let stall_load = if index == 0 {
                limits.head_stall_load
            } else {
                STALL_LOAD
            };
            let pushed = in_a_hand
                && off_track[index] >= limits.load_steps.max(1)
                && is.load > stall_load
                && !following;
            if over_load[index] >= limits.load_steps.max(1) || pushed || touched {
                // Something resists. A living thing stops pushing; it does
                // not faint. The joint's goal becomes where it is and it is
                // left there for two seconds. Loaded still with nothing
                // asked of it, or resisted for the fourth time in a minute,
                // and the run ends as it always did; under `--give-way` a
                // person may move it as often as they like.
                let often = !in_a_hand
                    && eased_at
                        .iter()
                        .all(|at| *at != 0 && report.steps - *at < EASE_MEMORY_STEPS);
                if held[index] || often {
                    surprise.get_or_insert(Stop::Load {
                        joint: joint.id(),
                        load: is.load,
                    });
                } else {
                    commanded[index] = f32::from(is.position).clamp(
                        low[index].min(commanded[index]),
                        high[index].max(commanded[index]),
                    );
                    velocity[index] = 0.0;
                    easing[index] = EASE_STEPS;
                    // A joint that gave way because a hand had already moved
                    // it off its goal was moved from there, not from where it
                    // gave: a light head lifted 40 counts gives at 30.
                    gave_at[index] = if pushed { written[index] } else { is.position };
                    // Where it stood: one that has not moved since it last
                    // tracked was left there by its goal wandering off
                    // (lamp-06c1's head, 22 Oct, at 1,765 in every sample),
                    // not moved by a hand, which moved it from its goal.
                    let left_by_goal = is.position.abs_diff(on_track_at[index]) <= STALL_SLACK;
                    held_from[index] = if pushed && !left_by_goal {
                        written[index]
                    } else {
                        is.position
                    };
                    if last_held[index].is_none_or(|at| report.steps - at > u64::from(EASE_STEPS)) {
                        gave_first[index] = gave_at[index];
                    }
                    over_full[index] = is.load > strained;
                    in_hand[index] = 0;
                    over_load[index] = 0;
                    contact[index] = 0;
                    pinned[index] = touched;
                    eased_at.rotate_left(1);
                    eased_at[EASES_ALLOWED - 1] = report.steps;
                    report.eased[index] = report.eased[index].saturating_add(1);
                }
            } else if rests_short(
                &limits,
                (written[index], is.position, over_load[index]),
                &mut short_at[index],
                &mut short_steps[index],
            ) {
                // Held a little short of its goal by its own weight, still,
                // and not straining: it rests where it is, and later moods
                // are laid from there, as for a joint a hand left.
                commanded[index] = f32::from(is.position).clamp(
                    low[index].min(commanded[index]),
                    high[index].max(commanded[index]),
                );
                kept[index] = commanded[index] - home_now[index] - offsets[index];
                velocity[index] = 0.0;
                off_track[index] = 0;
                report.rested_short[index] = report.rested_short[index].saturating_add(1);
            } else if stuck[index] >= limits.tracking_steps && short_steps[index] == 0 {
                surprise.get_or_insert(Stop::Tracking {
                    joint: joint.id(),
                    goal: written[index],
                    position: is.position,
                });
            } else if hot[index] >= limits.temperature_steps.max(1) {
                surprise.get_or_insert(Stop::Temperature {
                    joint: joint.id(),
                    temperature: is.temperature,
                });
            }
        }
        // Under `--overload release`: a joint at its trip load for 1.5 s.
        // One told to stop pushing on this step is given its chance first:
        // its count starts again, and it trips only if it stays loaded.
        // A hand (`--limp on`) makes it let go too, and never ends the run.
        let by_hand = hand_tripped.is_some() || slump_again.is_some();
        let mut tripped: Option<(Joint, u16)> = hand_tripped.or(slump_again.take());
        if limits.overload_release && !by_hand {
            for joint in Joint::ALL {
                let (index, is) = (joint.index(), samples[joint.index()]);
                overloaded[index] = if is.load >= trip_at[index] && !stopped_pushing[index] {
                    overloaded[index].saturating_add(1)
                } else {
                    0
                };
                if overloaded[index] >= OVERLOAD_STEPS
                    && tripped.is_none_or(|(_, load)| is.load > load)
                {
                    tripped = Some((joint, is.load));
                }
            }
        }
        // It lets go before anything else this step would end the run with:
        // that is the old runtime's answer to a servo pushed this hard.
        if let Some((mut joint, mut load)) = tripped {
            let mut by_hand = by_hand;
            let mut during_rest = || {
                let next = asked();
                unplug_latched |= next.0.unplug;
                next
            };
            // A hand's rest that strains the held arm past its cut-off ends
            // as an overload: all five let go, and its rest and its second
            // time within ten minutes are an overload's.
            let (again, template, slumped_at) = loop {
                let now = bus.runtime.now();
                let again = !by_hand
                    && overload_at.is_some_and(|at| now.saturating_sub(at) < OVERLOAD_AGAIN);
                if !by_hand {
                    overload_at = Some(now);
                }
                report.released.push(Released {
                    joint: joint.name(),
                    load,
                    seconds: now.as_secs_f32(),
                    woke: None,
                });
                left_as_is = true;
                // An overload lets go of all five, and nothing holds the arm up.
                // A hand lets go of the head and base only (#3702): the arm
                // keeps its torque and its last goal, so a standing lamp a
                // person was asked to push does not collapse.
                let released: &[Joint] = if by_hand { &COMMANDED } else { &Joint::ALL };
                let template = Glimpse {
                    released: report.released.len().try_into().unwrap_or(u16::MAX),
                    limp: true,
                    hand: false,
                    stand: if by_hand { stand } else { 0.0 },
                    ..glimpse
                };
                let rest = match (again, by_hand) {
                    (true, _) => Duration::ZERO,
                    (false, true) => HAND_REST,
                    (false, false) => OVERLOAD_REST,
                };
                match let_go(
                    bus,
                    &mut report,
                    released,
                    (rest, by_hand.then_some(trip_at)),
                    (&mut news, &mut during_rest),
                    &mut watch,
                    template,
                ) {
                    Ok((now, None)) => break (again, template, Ok(now)),
                    Ok((_, Some(strained))) => {
                        (joint, load) = strained;
                        by_hand = false;
                    }
                    Err(stop) => break (again, template, Err(stop)),
                }
            };
            let slumped_at = match slumped_at {
                Ok(now) => now,
                Err(stop) => break stop,
            };
            if unplug_latched || bus.runtime.recovery_inhibited() {
                break Stop::Refused {
                    why: "unplug preparation cannot recover automatically from a slump".into(),
                };
            }
            // Something that keeps fighting should be looked at.
            if again {
                break Stop::Overload {
                    joint: joint.id(),
                    load,
                    detail: "let go a second time within ten minutes; torque left off".into(),
                };
            }
            // Taken up again: every joint let go held where it is, as
            // `--torque on` takes hold, its goal first and then its torque.
            let all: Vec<Joint> = COMMANDED.into_iter().chain(ARM).collect();
            let taken = if by_hand { &COMMANDED[..] } else { &all };
            let now = match take_hold(bus, &mut report, taken, &slumped_at) {
                Ok(now) => now,
                Err(Stop::Refused { why }) => {
                    break Stop::Overload {
                        joint: joint.id(),
                        load,
                        detail: format!("taking the lamp up again: {why}"),
                    };
                }
                Err(stop) => break stop,
            };
            left_as_is = false;
            commanded = COMMANDED.map(|joint| f32::from(now[joint.index()].position));
            velocity = [0.0; 3];
            if let Some(joint) = COMMANDED
                .into_iter()
                .find(|joint| now[joint.index()].status != 0)
            {
                break Stop::Status {
                    joint: joint.id(),
                    found: found[joint.index()].status,
                    now: now[joint.index()].status,
                };
            }
            // Torque off and on may change what a servo flags: what it reads
            // now is what later reads are judged against (the wake's and,
            // below, the run's), and where the arm slumped is where it is
            // now found. A shoulder left up over an elbow that fell is
            // lowered first, as after a power cut, so the lamp is folded
            // and stands again the ordinary way when next asked; without
            // that, on lamp-52e6 on 2026-10-16, it ran for good with its
            // shoulder up and its elbow down, never stood again, and its
            // head pushed at its folded home.
            // A read that fails is tried again for as long as the run rides
            // out a failing wire (half a second).
            let mut profiles = bus.arm_profiles();
            for _ in 1..WIRE_STEPS {
                if profiles.is_ok() || standing.is_none() {
                    break;
                }
                report.missed_reads += 1;
                let due = bus.runtime.now() + TICK;
                bus.sleep_until(due);
                profiles = bus.arm_profiles();
            }
            // An arm a hand did not let go has nothing to lower.
            let lowered = match standing.filter(|_| !by_hand) {
                Some(up) => match profiles {
                    Ok(profiles) if shoulder_to_lower(up, &now, profiles) => {
                        let head = COMMANDED.map(|joint| now[joint.index()].position);
                        let elbow = now[Joint::ElbowPitch.index()].position;
                        let glimpse = Glimpse {
                            limp: false,
                            ..template
                        };
                        lower_shoulder(
                            bus,
                            &mut report,
                            &limits,
                            up,
                            head,
                            elbow,
                            now,
                            Some((&mut watch, glimpse)),
                        )
                    }
                    Ok(_) => Ok(now),
                    Err(detail) => Err(Stop::Bus {
                        detail: format!("taking the lamp up again: {detail}"),
                    }),
                },
                None => Ok(now),
            };
            let now = match lowered {
                Ok(now) => now,
                Err(stop) => {
                    // Stop pushing, the arm included: each goal becomes
                    // where its joint is now.
                    if let Some(last) = report.last {
                        let at = |joint: Joint| last[joint.index()].position;
                        let (head, arm) = (COMMANDED.map(at), ARM.map(at));
                        if bus.send_holds(head, Some(arm)).is_ok() {
                            report.goal_writes += 1;
                            report.held_at = Some(head);
                        }
                    }
                    left_as_is = true;
                    break stop;
                }
            };
            // An arm a hand did not let go never moved: its goals and the
            // stand are kept, and it stands on as it did.
            if !by_hand {
                arm_written = ARM.map(|joint| now[joint.index()].position);
            }
            if let Some(up) = standing.filter(|_| !by_hand) {
                let (lift, off) = on_the_way(up, arm_written);
                match (0..2).find(|index| off[*index] > f32::from(Standing::FOUND_SLACK)) {
                    Some(index) => {
                        standing = None;
                        report.standing = None;
                        stand = 0.0;
                        report.standing_refused = Some(format!(
                            "{} is {:.0} counts off the way between folded and stood up after it let go; an arm found off its way is not lowered",
                            ARM[index], off[index]
                        ));
                    }
                    None => stand = if lift < 0.05 { 0.0 } else { unsmooth(lift) },
                }
            }
            stand_velocity = 0.0;
            slumped = true;
            home_now = home_with(home, standing, stand);
            (low, high) = bounds(home_now, stand);
            // The head, its roll and the base brought home as a lamp wakes
            // from a power cut: slowly, never away from home, a head that
            // drooped led enough to lift it, and woken where it settles.
            let from = COMMANDED.map(|joint| now[joint.index()].position);
            let to = home_now.map(|count| count.round() as u16);
            let woke_at_start = report.woke.take();
            let woken = wake(
                bus,
                &mut report,
                &limits,
                (from, to),
                standing.map(|_| arm_written),
                now,
                Some((
                    &mut watch,
                    Glimpse {
                        limp: false,
                        stands: standing.is_some(),
                        stand,
                        arm_goals: arm_written,
                        ..template
                    },
                )),
            );
            if let Some(released) = report.released.last_mut() {
                released.woke = report.woke.take();
            }
            report.woke = woke_at_start;
            let now = match woken {
                Ok(now) => now,
                Err(stop) => {
                    // Stop pushing: each goal becomes where its joint is now.
                    let mut hold = None;
                    if let Some(last) = report.last {
                        let at = COMMANDED.map(|joint| last[joint.index()].position);
                        if bus.send_holds(at, standing.map(|_| arm_written)).is_ok() {
                            report.goal_writes += 1;
                            report.held_at = Some(at);
                            hold = Some(at);
                        }
                    }
                    // This hold asks the arm's last goal and head readings,
                    // which pass today; a refusal of it must not outlive it
                    // into the run that goes on.
                    bus.refused = None;
                    // A hand on it while it came up: as V1, it lets go again.
                    if let (true, Some(at), Stop::Load { .. } | Stop::Tracking { .. }) =
                        (by_hand, hold, &stop)
                    {
                        written = at;
                        commanded = at.map(f32::from);
                        velocity = [0.0; 3];
                        easing = [EASE_STEPS; 3];
                        slump_again = Some((joint, load));
                        continue;
                    }
                    left_as_is = true;
                    break stop;
                }
            };
            // As at start: the run takes over the goals the wake left, and a
            // joint that settled farther short than the run's own bound runs
            // from where it settled, its home moved there by as much.
            found = now;
            let mut goals = to;
            for (index, joint) in COMMANDED.iter().enumerate() {
                let at = found[joint.index()].position;
                if at.abs_diff(to[index]) > limits.tracking {
                    goals[index] = at;
                    let shifted = i32::from(home[index]) + i32::from(at) - i32::from(to[index]);
                    home[index] = shifted.clamp(0, 4_095) as u16;
                }
            }
            if goals != to {
                if let Err(detail) = bus.send_holds(goals, standing.map(|_| arm_written)) {
                    break bus.refused.take().unwrap_or(Stop::Bus {
                        detail: format!("waking: {detail}"),
                    });
                }
                report.goal_writes += 1;
                report.home = Some(home);
                home_now = home_with(home, standing, stand);
                (low, high) = bounds(home_now, stand);
            }
            // Everything else starts again from where it is, as at start.
            written = goals;
            commanded = written.map(f32::from);
            wanted_of = commanded;
            (easing, over_load, off_track, hot, in_hand) = ([0; 3], [0; 3], [0; 3], [0; 3], [0; 3]);
            (kept, asleep, arriving) = ([0.0; 3], [false; 3], [0; 3]);
            (arm_over, arm_off, arm_hot) = ([0; 2], [0; 2], [0; 2]);
            stalls = [Stall::default(); 5];
            contact = [0; 3];
            overloaded = [0; 5];
            (resting_steps, relaxed, still_steps, down_steps) = (0, false, 0, 0);
            arm_rests = stand <= 0.0;
            (playing, pending, idle_due) = (None, None, None);
            (gesture_now, gesture_cooling) = ([0.0; 5], 0);
            // An ease back the slump cut short ends with it, or it would
            // replay the old gesture's offsets on the lamp just taken up.
            (easing_out, yielding, gesture_moved) = (None, false, [0.0; 5]);
            (gaze, look, faces_running) = (looking.map(Gaze::new), [0.0; 2], 0);
            (heard, awake_until) = (None, None);
            started = bus.runtime.now();
            first_step = report.steps;
            continue;
        }
        // A standing lamp folding down is not let go mid-air by another
        // surprise (seventh, #1281 12:52): it keeps folding, within the
        // time a stop allows, and ends on what it was folding for, or else
        // on this. A lost wire, an arm goal the guard refuses, a position
        // that cannot be read and a hand or an overload letting go end it
        // as before. A status flag is the servo's own protection, and a
        // goal turns a servo that cut itself out back on: a flagged head or
        // base joint is written nothing more and left as it is while the
        // rest folds; a flagged shoulder or elbow, without which the arm
        // cannot fold, lets go at once.
        if let Some(surprise) = surprise {
            let flagged = match surprise {
                Stop::Status { joint, .. } => Joint::ALL.into_iter().find(|j| j.id() == joint),
                _ => None,
            };
            // Not even the hold at the end is written to it.
            if let Some(joint) = flagged {
                bus.left[joint.index()] = true;
            }
            if leaving.is_none() || stand <= 0.0 || flagged.is_some_and(|j| ARM.contains(&j)) {
                break surprise;
            }
            leave_for.get_or_insert(surprise);
        }
        if let Some(since) = leaving {
            let settled = (0..3).all(|index| {
                bus.left[COMMANDED[index].index()]
                    || (commanded[index] - home_now[index]).abs() < 0.6
                        && velocity[index].abs() < 3.0
            }) && (slumped || stand <= 0.0 && stand_velocity == 0.0);
            // A lamp that stands folds down first, and that takes its time.
            let allowed = LEAVE
                + standing.map_or(Duration::ZERO, |up| {
                    Duration::from_secs_f32(up.seconds * 2.0)
                });
            if settled || now.saturating_sub(since) >= allowed {
                break leave_for.take().unwrap_or(Stop::Cancelled);
            }
        }
        if unplug_latched {
            let positions = samples.map(|sample| sample.position);
            let stable = unplug_previous.is_some_and(|previous| {
                positions
                    .iter()
                    .zip(previous)
                    .all(|(at, was)| at.abs_diff(was) <= 2)
            });
            unplug_previous = Some(positions);
            let arm_folded = standing.is_some_and(|up| {
                ARM.into_iter()
                    .zip(up.folded)
                    .all(|(joint, target)| rests_on(samples[joint.index()].position, target))
            });
            let home = unplug_home.is_some_and(|home| {
                COMMANDED
                    .into_iter()
                    .zip(home)
                    .all(|(joint, target)| rests_on(samples[joint.index()].position, target))
            });
            // In the order of `crate::unplug::UNPLUG_UNSAFE`: the first that fails is why.
            let checks: [bool; crate::unplug::UNPLUG_UNSAFE.len()] = [
                arm_folded,
                home,
                !tipping,
                stable,
                stand == 0.0 && stand_velocity == 0.0,
                playing.is_none() && pending.is_none(),
                !slumped,
                !held.iter().any(|held| *held),
                !easing.iter().any(|left| *left > 0)
                    && !stopped_pushing.iter().any(|stopped| *stopped),
                leaving.is_none() && leave_for.is_none(),
                bus.refused.is_none()
                    && !bus.left.iter().any(|left| *left)
                    && !bus.unjudged.iter().any(|value| *value),
                bad_before == (bus.bad_reads, bus.bad_temperatures)
                    && bus.bad_in_row == [0; 5]
                    && unsent == 0,
                samples.iter().all(|sample| {
                    sample.status == 0 && sample.temperature < 55 && sample.load < 100
                }),
            ];
            let unread = crate::unplug::UNREAD;
            glimpse.unplug_unsafe = checks
                .iter()
                .enumerate()
                .position(|(at, ok)| at != unread && !ok)
                .map(|at| at as u8);
            // A read missed or bad since the last sample, or a goal not yet
            // sent, is a gap in the evidence, not evidence of movement: the
            // step is not sampled, and the next read is judged as ever.
            // lamp-52e6 on 2026-10-20 failed its unplug on one bad read while
            // the readiness line played (#1699).
            glimpse.unplug_gap = report.missed_reads != unplug_missed || !checks[unread];
            glimpse.unplug_safe = glimpse.unplug_unsafe.is_none() && !glimpse.unplug_gap;
            unplug_missed = report.missed_reads;
            watch(&glimpse);
        }
        bus.sleep_until(due);
    };

    if left_as_is {
        // Let go under overload, or already held where it is: nothing more.
        // A goal written to a servo whose torque is off turns it back on.
    } else if matches!(stop, Stop::Refused { .. }) {
        // A lamp found up whose start was refused, folded: let go.
        let_go_at_end(bus, &mut report);
    } else if stop == Stop::Cancelled {
        // Leave the goals exactly at home: of a lamp that is down. One
        // that did not get down in time is left as it is.
        // Its folded arm is left resting where it is, not pushed at.
        let arm_at = report.last.map_or(arm_written, |last| {
            ARM.map(|joint| last[joint.index()].position)
        });
        // A lamp that did not get down in time writes nothing. This write
        // keeps torque on, so the arm may only be left a step from its last
        // goal: a reading farther off, even a steady one, is not written,
        // and the run ends on `Stop::Window` and lets go.
        let writes =
            stand <= 0.0 && (written != home || (standing.is_some() && arm_written != arm_at));
        let off = (0..2).find(|index| {
            writes
                && standing.is_some()
                && arm_at[*index].abs_diff(arm_written[*index]) > ARM_GOAL_STEP
        });
        if let Some(index) = off {
            bus.refused = Some(Stop::Window {
                joint: ARM[index].id(),
                goal: arm_at[index],
                window: bus.arm_windows[index],
                from: arm_written[index],
            });
        } else if writes && bus.send_goals(home, standing.map(|_| arm_at)).is_ok() {
            report.goal_writes += 1;
        }
        // Only a lamp that is down is let go: the head within
        // `LET_GO_NEAR` of home and the arm of its folded counts, so that
        // nothing standing falls (the first builder on #1722, 19 Oct).
        let down = report.last.is_some_and(|last| {
            (0..3).all(|index| {
                bus.left[COMMANDED[index].index()]
                    || last[COMMANDED[index].index()]
                        .position
                        .abs_diff(home[index])
                        <= LET_GO_NEAR
            }) && standing.is_none_or(|up| arm_folded(&last, up.folded))
        });
        if limits.let_go_at_stop && stand <= 0.0 && off.is_none() && bus.refused.is_none() && down {
            let_go_at_end(bus, &mut report);
            report.let_go_at_stop = true;
        }
    } else if let Some(last) = report.last {
        // Stop pushing: each goal becomes where its joint is now, kept
        // inside the same bounds as every other goal.
        let hold: [u16; 3] = std::array::from_fn(|index| {
            let position = f32::from(last[COMMANDED[index].index()].position);
            position
                .clamp(
                    low[index].min(commanded[index]),
                    high[index].max(commanded[index]),
                )
                .round() as u16
        });
        // The arm likewise where it is: the goal last written may be the
        // one that ended the run.
        let arm_at = ARM.map(|joint| last[joint.index()].position);
        if bus.send_holds(hold, standing.map(|_| arm_at)).is_ok() {
            report.goal_writes += 1;
            report.held_at = Some(hold);
        }
    }
    // Any surprise lets go of the whole lamp: see `run_alive`. Every
    // refusal in the run ended it where it was made; one here is of the
    // exit's own write, which ends a run asked to stop as a surprise, while
    // a hold refused keeps what ended the run.
    report.stop = match (stop, bus.refused.take()) {
        (Stop::Cancelled, Some(refusal)) => refusal,
        (stop, _) => stop,
    };
    report
}

#[cfg(test)]
mod tests;
