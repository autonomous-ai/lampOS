//! A lamp that reads as alive (task 36, #714): an idle layer that breathes
//! with the ring and now and then drifts a little, none of it while someone
//! works under the light; a sleep that noise does not break; and a small
//! stretch on waking. All of it is optional: a catalogue without `alive`
//! plans exactly what it planned before.
//!
//! Everything here is a function of the injected boot clock and the inputs
//! the engine was given. Nothing is random: a drift's size and timing come
//! from a fixed hash of how many drifts came before, so a replay is the same
//! every time.

use crate::Error;
use crate::catalogue::{Keyframe, add, blend, ease};
use lamp_msgs::{
    BodyOffsets, ControlGesture, Faces, Gesture, ScheduleDue, TouchEvent, TouchGesture,
    TouchSource, TouchState, Voice, VoiceMute,
};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::f64::consts::PI;

/// The ring's own resting breath (`crates/voice/src/ring_face.rs`): its
/// phase is the boot clock's seconds within the hour over 5.2 s. The body,
/// reading the same clock, breathes with it and not against it.
pub const RING_BREATH_US: u64 = 5_200_000;
const RING_HOUR_US: u64 = 3_600_000_000;
/// The eyes report every 600 ms with nobody there and every 250 ms while
/// they follow a face (docs/eyes.md). Nothing from them for this long means
/// they are not looking, and nobody can be said to be absent.
pub const EYES_QUIET_US: u64 = 5_000_000;
/// A local time of day older than this is not trusted to say it is night.
pub const DAYTIME_STALE_US: u64 = 120_000_000;
/// The idle layer's own bounds, the same as a rest clip's: small and slow.
const IDLE_REACH: f64 = 0.06;
const IDLE_SPEED: f64 = 0.06;
const IDLE_ACCELERATION: f64 = 0.12;
/// A stretch is a deliberate move and may be quicker, never a snap.
const WAKE_SPEED: f64 = 0.35;
const WAKE_ACCELERATION: f64 = 2.0;
/// Source observations, on either side of the beginning of the name.
const NAME_SPEECH_US: u64 = 2_000_000;
const MAX_PENDING_NAMES: usize = 16;
const MAX_SPEECH_OBSERVATIONS: usize = 128;

fn is_false(value: &bool) -> bool {
    !value
}

/// More energy, the same move larger; less, smaller (creature.md: "mood
/// changes the style of the same move, never adds randomness").
pub fn size(energy: f32) -> f64 {
    0.7 + 0.6 * f64::from(energy)
}

/// More energy, the same move quicker; less, slower. A factor on durations.
pub fn pace(energy: f32) -> f64 {
    1.25 - 0.5 * f64::from(energy)
}

/// The largest `size` and the quickest `pace` any energy can give, against
/// which every bound is checked, so that no energy can exceed one.
const SIZE_MOST: f64 = 1.3;
const PACE_QUICKEST: f64 = 0.75;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Breath {
    /// One breath, in and out. The ring's is [`RING_BREATH_US`].
    pub period_us: u64,
    /// The offsets at the top of a breath in; nothing at the bottom.
    pub inhale: BodyOffsets,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Drift {
    /// The shortest and the longest wait between one drift and the next.
    pub every_us: [u64; 2],
    /// How long one drift takes, eased at both ends.
    pub move_us: u64,
    /// Where a drift may end: each axis somewhere from `low` to `high`,
    /// which must hold zero between them.
    pub low: BodyOffsets,
    pub high: BodyOffsets,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkLight {
    /// A face taking at least this share of the picture is close.
    pub face_area: f32,
    /// A face whose middle stays within this share of the picture is still.
    pub still: f32,
    /// Close and still for this long is someone working under the light.
    pub still_us: u64,
    /// Still taken as working this long after the face was last close and
    /// still: a head bent over work is often no face at all.
    pub hold_us: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sleep {
    /// A sleeping name needs the voice owner's person-speech evidence.
    /// Omission preserves the original immediate name wake.
    #[serde(default, skip_serializing_if = "is_false")]
    pub name_requires_speech: bool,
    /// The local minutes of the day from which and until which it may fall
    /// asleep; `[1380, 360]` is 23:00 to 06:00, past midnight.
    pub night: [u16; 2],
    /// Nobody seen and nothing said to it for this long first.
    pub nobody_us: u64,
    /// Into the sleeping pose over this long, eased.
    pub settle_us: u64,
    /// The sleeping pose, laid where the voice's resting posture would be.
    pub pose: BodyOffsets,
    /// The only movement while asleep.
    pub breath: Breath,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wake {
    /// The stretch, from and back to nothing, played as the sleeping pose
    /// lets go. Brightness is not used.
    pub keyframes: Vec<Keyframe>,
}

impl Wake {
    fn duration_us(&self) -> u64 {
        self.keyframes.last().map_or(0, |frame| frame.at_us)
    }

    fn sample(&self, elapsed_us: u64) -> BodyOffsets {
        let after = self
            .keyframes
            .partition_point(|frame| frame.at_us < elapsed_us);
        if after == 0 {
            return self.keyframes[0].offsets;
        }
        let Some(b) = self.keyframes.get(after) else {
            return self.keyframes[self.keyframes.len() - 1].offsets;
        };
        let a = self.keyframes[after - 1];
        blend(
            a.offsets,
            b.offsets,
            ease((elapsed_us - a.at_us) as f64 / (b.at_us - a.at_us) as f64),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Alive {
    /// The voice's states in which the idle layer runs. Never speaking: the
    /// lamp keeps its head still while it talks (body.md, "A glance first").
    pub states: Vec<Voice>,
    /// How long the idle layer takes to come in and to go out.
    pub blend_us: u64,
    /// The energy the style of every move here is drawn from, 0 to 1. Where
    /// Mood runs, its liveliness moves it by up to a half either way (#1670,
    /// `Life::energy`).
    pub energy: f32,
    pub breath: Breath,
    pub drift: Drift,
    pub work: WorkLight,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sleep: Option<Sleep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wake: Option<Wake>,
}

fn wrong(why: impl Into<String>) -> Error {
    Error(format!("alive: {}", why.into()))
}

/// Only the head moves: the arm carries the light, and the body does not
/// take it from Expression.
fn head_only(offsets: BodyOffsets) -> bool {
    offsets.shoulder == 0.0 && offsets.elbow == 0.0
}

/// Smoothstep from rest to rest over `seconds`: its fastest speed is
/// 1.5 * delta / seconds and its largest acceleration 6 * delta / seconds².
fn smooth_within(delta: f64, seconds: f64, speed: f64, acceleration: f64) -> bool {
    1.5 * delta / seconds <= speed && 6.0 * delta / (seconds * seconds) <= acceleration
}

impl Breath {
    fn check(&self, name: &str) -> Result<(), Error> {
        self.inhale.check()?;
        if !(2_000_000..=20_000_000).contains(&self.period_us) {
            return Err(wrong(format!("{name} period must be 2..20 seconds")));
        }
        let seconds = self.period_us as f64 / 1e6;
        // A breath of `a` peaks at speed pi * a / T and acceleration
        // 2 * pi² * a / T²; checked at the largest size any energy gives.
        if !head_only(self.inhale)
            || self.inhale.base_yaw != 0.0
            || self.inhale.values().iter().any(|value| {
                let most = value.abs() * SIZE_MOST;
                most > IDLE_REACH
                    || PI * most / seconds > IDLE_SPEED
                    || 2.0 * PI * PI * most / (seconds * seconds) > IDLE_ACCELERATION
            })
        {
            return Err(wrong(format!(
                "{name} moves the head only, at most 0.06 radians, slowly"
            )));
        }
        Ok(())
    }

    /// How far in the breath is at `now_us`, from 0 (out) to 1 (in), as the
    /// ring's: at the lamp's one breath's phase where Mood keeps it (#1670,
    /// `mood`), else the boot clock's seconds within the hour, over the period.
    pub fn share_at(&self, now_us: u64, mood: Option<f64>) -> f64 {
        let turns = mood.unwrap_or_else(|| {
            let seconds = (now_us % RING_HOUR_US) as f64 / 1e6;
            seconds / (self.period_us as f64 / 1e6)
        });
        0.5 - 0.5 * (2.0 * PI * turns).cos()
    }

    /// Where the breath is at `now_us`, as [`Self::share_at`] says.
    fn at(&self, now_us: u64, size: f64, mood: Option<f64>) -> BodyOffsets {
        add(
            BodyOffsets::ZERO,
            self.inhale,
            self.share_at(now_us, mood) * size,
        )
    }

    /// The fastest the breath moves on any axis, radians a second.
    fn speed(&self, size: f64) -> f64 {
        let largest = self
            .inhale
            .values()
            .iter()
            .fold(0.0_f64, |most, value| most.max(value.abs()));
        PI * largest * size / (self.period_us as f64 / 1e6)
    }
}

impl Alive {
    pub fn check(&self, limits: BodyOffsets) -> Result<(), Error> {
        if self.states.is_empty()
            || self.states.len() > 3
            || self.states.contains(&Voice::Speaking)
            || self
                .states
                .iter()
                .enumerate()
                .any(|(index, state)| self.states[..index].contains(state))
        {
            return Err(wrong(
                "states are 1..3 distinct voice states, never speaking",
            ));
        }
        if !(100_000..=350_000).contains(&self.blend_us) {
            return Err(wrong("blend_us must be 100000..350000"));
        }
        if !(0.0..=1.0).contains(&self.energy) {
            return Err(wrong("energy must be finite in 0..1"));
        }
        self.breath.check("breath")?;
        let drift = self.drift;
        drift.low.check()?;
        drift.high.check()?;
        let [soonest, latest] = drift.every_us;
        if !(5_000_000..=600_000_000).contains(&soonest)
            || !(soonest..=600_000_000).contains(&latest)
            || !(1_000_000..=10_000_000).contains(&drift.move_us)
        {
            return Err(wrong(
                "a drift waits 5..600 seconds, shortest first, and takes 1..10 seconds",
            ));
        }
        let seconds = drift.move_us as f64 / 1e6 * PACE_QUICKEST;
        if !head_only(drift.low)
            || !head_only(drift.high)
            || drift.low.base_yaw != 0.0
            || drift.high.base_yaw != 0.0
            || drift
                .low
                .values()
                .iter()
                .zip(drift.high.values())
                .any(|(low, high)| {
                    let reach = (high - low) * SIZE_MOST;
                    *low > 0.0
                        || high < 0.0
                        || low.abs() * SIZE_MOST > IDLE_REACH
                        || high * SIZE_MOST > IDLE_REACH
                        || !smooth_within(reach, seconds, IDLE_SPEED, IDLE_ACCELERATION)
                })
        {
            return Err(wrong(
                "a drift moves the head only, within 0.06 radians of rest either way with zero between, slowly",
            ));
        }
        let work = self.work;
        // A share above nothing and at most `most` (never NaN).
        let share = |value: f32, most: f32| value > 0.0 && value <= most;
        let sound = share(work.face_area, 1.0)
            && share(work.still, 0.5)
            && (1_000_000..=60_000_000).contains(&work.still_us)
            && work.hold_us <= 600_000_000;
        if !sound {
            return Err(wrong(
                "work light needs a face area in 0..1, stillness in 0..0.5, 1..60 s still and at most 600 s held",
            ));
        }
        if let Some(sleep) = &self.sleep {
            sleep.pose.check()?;
            sleep.breath.check("sleep breath")?;
            let settle = sleep.settle_us as f64 / 1e6;
            if sleep.night.iter().any(|minute| *minute >= 1_440)
                || sleep.night[0] == sleep.night[1]
                || !(60_000_000..=7_200_000_000).contains(&sleep.nobody_us)
                || !(1_000_000..=10_000_000).contains(&sleep.settle_us)
                || !head_only(sleep.pose)
                || sleep.pose.base_yaw != 0.0
                || sleep.pose.values().iter().any(|value| {
                    value.abs() > IDLE_REACH
                        || !smooth_within(value.abs(), settle, IDLE_SPEED, IDLE_ACCELERATION)
                })
            {
                return Err(wrong(
                    "sleep needs a night of local minutes, 1 minute to 2 hours of nobody, 1..10 s to settle and a slow head pose within 0.06 radians",
                ));
            }
        }
        if let Some(wake) = &self.wake {
            if self.sleep.is_none() {
                return Err(wrong("a waking stretch needs a sleep to wake from"));
            }
            let frames = &wake.keyframes;
            if frames.len() < 2
                || frames.len() > crate::MAX_KEYFRAMES
                || frames[0].at_us != 0
                || frames.windows(2).any(|pair| pair[0].at_us >= pair[1].at_us)
                || !(1_000_000..=5_000_000).contains(&wake.duration_us())
                || frames[0].offsets != BodyOffsets::ZERO
                || frames[frames.len() - 1].offsets != BodyOffsets::ZERO
            {
                return Err(wrong(
                    "a stretch begins at zero, rises in time, lasts 1..5 seconds and ends where it began",
                ));
            }
            for frame in frames {
                frame.offsets.check()?;
                if !head_only(frame.offsets)
                    || frame.brightness != 0.0
                    || frame
                        .offsets
                        .values()
                        .iter()
                        .zip(limits.values())
                        .any(|(value, limit)| value.abs() * SIZE_MOST > limit)
                {
                    return Err(wrong(
                        "a stretch moves the head and the base only, without light, within the catalogue's limits",
                    ));
                }
            }
            if frames.windows(2).any(|pair| {
                let seconds = (pair[1].at_us - pair[0].at_us) as f64 / 1e6 * PACE_QUICKEST;
                pair[0]
                    .offsets
                    .values()
                    .iter()
                    .zip(pair[1].offsets.values())
                    .any(|(a, b)| {
                        !smooth_within(
                            (b - a).abs() * SIZE_MOST,
                            seconds,
                            WAKE_SPEED,
                            WAKE_ACCELERATION,
                        )
                    })
            }) {
                return Err(wrong(
                    "a stretch moves at most 0.35 radians a second and 2 a second squared",
                ));
            }
        }
        Ok(())
    }
}

/// What the rest of the engine knows on this tick.
pub(crate) struct Context {
    /// The voice's state, if it is fresh.
    pub voice: Option<Voice>,
    /// Nothing holds the body: no hold for someone, no concentration, and
    /// fresh joints.
    pub free: bool,
}

pub(crate) struct Planned {
    /// Laid over the rest of the plan once that is weighed by `keep`.
    pub offsets: BodyOffsets,
    /// How much of the rest of the plan (the voice's posture and every clip)
    /// is kept: all of it awake, none of it asleep, crossed over while it
    /// falls asleep and while it wakes, so that nothing leaps.
    pub keep: f64,
    /// Asleep or falling asleep: no clip may begin.
    pub asleep: bool,
    pub names: Vec<&'static str>,
}

impl Default for Planned {
    fn default() -> Self {
        Self {
            offsets: BodyOffsets::ZERO,
            keep: 1.0,
            asleep: false,
            names: Vec::new(),
        }
    }
}

/// Leaning into a hand kept on the head (#711, `docs/creature.md` "Touch"):
/// the head lifts a hair into the hand and tilts toward it, the opposite of
/// the sleeping bow. Authored, to be looked at on a lamp.
pub const LEAN: BodyOffsets = BodyOffsets {
    base_yaw: 0.0,
    shoulder: 0.0,
    elbow: 0.0,
    head_roll: 0.02,
    head_pitch: -0.015,
};
/// The lean comes in over this, and goes over [`LEAN_OUT_US`].
pub const LEAN_IN_US: u64 = 500_000;
pub const LEAN_OUT_US: u64 = 600_000;
/// The hand is gone if the head's state has not said it is there for this
/// long: a Controls that stopped does not leave the lamp leaning.
pub const HAND_FRESH_US: u64 = 500_000;

/// Looking up at a tap on the head (#711, First on #711 at 05:05): "are you
/// there?" answered with all of itself at once, beside the voice's "hm?" and
/// the ring's listening face. A small lift of the head, more toward a face
/// seen high in the picture and less toward one low; no face, the lift
/// alone. Only the head: never a turn of the body. Authored, to be looked
/// at on a lamp.
pub const LOOK_UP: BodyOffsets = BodyOffsets {
    base_yaw: 0.0,
    shoulder: 0.0,
    elbow: 0.0,
    head_roll: 0.0,
    head_pitch: -0.035,
};
/// A face at the top of the picture adds this much lift, one at the bottom
/// takes it away (the face's centre, -1 top to 1 bottom).
pub const LOOK_UP_TOWARD: f64 = 0.025;
/// In within the moment of the hum, held as long as a call waits for the
/// rest on lamp-4ace (#891), longer while someone speaks, out slowly.
pub const LOOK_UP_IN_US: u64 = 150_000;
pub const LOOK_UP_FOR_US: u64 = 2_000_000;
pub const LOOK_UP_OUT_US: u64 = 500_000;
/// A face counts for the look only if the eyes saw it this recently.
pub const LOOK_UP_FACE_US: u64 = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Awake,
    Asleep {
        since: u64,
    },
    Waking {
        since: u64,
        from: BodyOffsets,
        kept: f64,
    },
}

/// A small history of already validated source observations. No transcript,
/// audio, wall-clock wait or worker belongs to this layer. Counters coalesce
/// notices between service ticks rather than allowing an unbounded log queue.
struct NameWake {
    speech: VecDeque<u64>,
    pending: VecDeque<(u64, u64)>,
    notices: [u64; 4],
    name_age_us: u64,
    delivery_us: u64,
    speech_since: u64,
}

impl NameWake {
    fn new(name_age_us: u64, delivery_us: u64) -> Self {
        Self {
            speech: VecDeque::new(),
            pending: VecDeque::new(),
            notices: [0; 4],
            name_age_us,
            delivery_us,
            speech_since: 0,
        }
    }

    fn notice(&mut self, kind: usize, count: u64) {
        self.notices[kind] = self.notices[kind].saturating_add(count);
    }

    fn advance(&mut self, now_us: u64) {
        self.speech.retain(|at| {
            now_us.saturating_sub(*at) <= self.name_age_us.saturating_add(NAME_SPEECH_US)
        });
        let before = self.pending.len();
        self.pending.retain(|(_, until)| now_us < *until);
        self.notice(0, (before - self.pending.len()) as u64);
    }

    fn name(&mut self, now_us: u64, stamp_us: u64, ago_ms: u32) -> bool {
        let Some(at) = stamp_us.checked_sub(u64::from(ago_ms) * 1_000) else {
            self.notice(1, 1);
            return false;
        };
        if at > now_us || now_us - at >= self.name_age_us {
            self.notice(1, 1);
            return false;
        }
        if self
            .speech
            .iter()
            .any(|speech| speech.abs_diff(at) <= NAME_SPEECH_US)
        {
            self.pending.clear();
            return true;
        }
        // An accepted activity can arrive just under its existing age limit
        // after the source window closes. This grace never widens that window.
        let until = at
            .saturating_add(NAME_SPEECH_US)
            .saturating_add(self.delivery_us);
        if now_us >= until {
            self.notice(0, 1);
        } else if self.pending.len() == MAX_PENDING_NAMES {
            self.notice(3, 1);
        } else {
            self.pending.push_back((at, until));
        }
        false
    }

    fn speech(&mut self, stamp_us: u64) -> bool {
        if stamp_us < self.speech_since {
            return false;
        }
        if self.speech.len() == MAX_SPEECH_OBSERVATIONS {
            self.speech.pop_front();
        }
        self.speech.push_back(stamp_us);
        let matched = self
            .pending
            .iter()
            .any(|(at, _)| stamp_us.abs_diff(*at) <= NAME_SPEECH_US);
        if matched {
            self.pending.clear();
        }
        matched
    }

    fn clear(&mut self, report: bool) {
        if report {
            self.notice(2, self.pending.len() as u64);
        }
        self.pending.clear();
        self.speech.clear();
    }

    fn take_notices(&mut self) -> Vec<String> {
        let reasons = [
            "no validated person speech within +/-2 s",
            "invalid or stale name onset",
            "voice ownership, input readiness or freshness was lost",
            "pending name limit reached without person speech",
        ];
        std::mem::take(&mut self.notices)
            .into_iter()
            .zip(reasons)
            .filter(|(count, _)| *count != 0)
            .map(|(count, reason)| format!("ignored sleeping name ({count}): {reason}"))
            .collect()
    }
}

pub(crate) struct Life {
    alive: Alive,
    weight: f64,
    last_us: Option<u64>,
    breath: Option<BodyOffsets>,
    drift_from: BodyOffsets,
    drift_to: BodyOffsets,
    drift_started: Option<u64>,
    drift_next: Option<u64>,
    drifts: u64,
    anchor: Option<([f32; 2], u64)>,
    worked: Option<u64>,
    faces_at: Option<u64>,
    nobody_since: Option<u64>,
    daytime: Option<(u64, u16)>,
    phase: Phase,
    called: bool,
    name_called: bool,
    name_wake: NameWake,
    last_due: Option<(u64, u64)>,
    /// The head (#711): the contact of a hand kept there long enough to lean
    /// into, when its state last said it was there, how far the lean has
    /// come in, and a hold's request for sleep.
    hand: Option<u64>,
    hand_seen_us: Option<u64>,
    lean: f64,
    sleep_asked: bool,
    /// A tap on the head of a lamp awake (#711): until when it looks up,
    /// how far the look has come in, and where the face seen highest in the
    /// picture was last (its stamp and its centre, -1 top to 1 bottom).
    looking_until_us: Option<u64>,
    look_up: f64,
    face_y: Option<(u64, f64)>,
    /// The voice's last word on sleep (`voice/mute`, #711): when, and
    /// whether its mute says the lamp was put to sleep; and the last it said
    /// while that word was fresh.
    voice_said: Option<(u64, bool)>,
    voice_slept: bool,
    /// Muted for privacy (#711, First on #711, 17 Oct 10:18): whether the
    /// voice last said its own mute, not its sleep, and whether the privacy
    /// switch was last said closed. Each holds until a word replaces it,
    /// however late, so a sender that stops never reads as open (the
    /// reviewer on #1282). And when a tap came while the voice's own mute
    /// held: the tap that clears it (`LAMP_RESTORED_MUTE`) gets its look up
    /// once the voice says it is open (the seventh on #1282).
    voice_muted: bool,
    privacy_muted: bool,
    /// The lamp's one breath's phase now, where Mood keeps it (#1670).
    mood_breath: Option<f64>,
    /// How lively the moves are from the lamp's inner state (#1670).
    mood_liveliness: Option<f32>,
    tapped_muted_us: Option<u64>,
    /// The body seen moving (its own shift at rest, #933, a gesture, or
    /// this layer's look up at a tap falling back): the goals it last said
    /// for its joints and when, since when they have gone fast, and when it
    /// was last seen moving so.
    body_goals: Option<(u64, Vec<f64>)>,
    body_fast_since: Option<u64>,
    body_moved_us: Option<u64>,
}

/// How long the voice's word on its mute counts: it says it every second
/// and keeps it three (#711).
pub const VOICE_MUTE_KEPT_US: u64 = 3_000_000;
/// A tap whose mute the voice says open within this is the tap that cleared
/// it (the restart guard's mute, `LAMP_RESTORED_MUTE`): it says so when it
/// changes, a bus hop and a pass after the tap (the seventh on #1282).
pub const TAP_OPENS_US: u64 = 1_500_000;

/// A shift the body makes by itself at rest and a drift are one layer: no
/// drift begins this soon after the body was seen moving (the body keeps
/// the same window after a drift, #933). Any move counts, not only the
/// body's own shift: a gesture, or this layer's look up at a tap falling
/// back, holds a drift off too, as a living thing does not drift right
/// after it moved for someone.
pub const BODY_QUIET_US: u64 = 30_000_000;
/// The body is seen moving when the goal it says for a joint on
/// `body/joint_states` goes this fast, in radians a second, for
/// `BODY_MOVING_US` running. This layer's own breath and drift go under
/// 0.02; a shift at rest goes 0.06 to 0.09; a goal that jumps once (a
/// head led as it lifts) is not a move.
const BODY_FAST: f64 = 0.04;
const BODY_MOVING_US: u64 = 300_000;

/// A fraction in 0..1 from a counter and a salt: splitmix64, fixed, so that
/// a drift's timing and size are the same in every replay.
fn fraction(count: u64, salt: u64) -> f64 {
    let mut z = count
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(salt.wrapping_mul(0xBF58_476D_1CE4_E5B9));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1_u64 << 53) as f64
}

impl Life {
    pub(crate) fn new(alive: Alive, name_age_us: u64, delivery_us: u64) -> Self {
        Self {
            alive,
            weight: 0.0,
            last_us: None,
            breath: None,
            drift_from: BodyOffsets::ZERO,
            drift_to: BodyOffsets::ZERO,
            drift_started: None,
            drift_next: None,
            drifts: 0,
            anchor: None,
            worked: None,
            faces_at: None,
            nobody_since: None,
            daytime: None,
            phase: Phase::Awake,
            called: false,
            name_called: false,
            name_wake: NameWake::new(name_age_us, delivery_us),
            last_due: None,
            hand: None,
            hand_seen_us: None,
            lean: 0.0,
            sleep_asked: false,
            looking_until_us: None,
            look_up: 0.0,
            face_y: None,
            voice_said: None,
            voice_slept: false,
            voice_muted: false,
            privacy_muted: false,
            mood_breath: None,
            mood_liveliness: None,
            tapped_muted_us: None,
            body_goals: None,
            body_fast_since: None,
            body_moved_us: None,
        }
    }

    fn asleep(&self) -> bool {
        matches!(self.phase, Phase::Asleep { .. })
    }

    /// The local time of day (minutes since local midnight) at `stamp_us`.
    pub(crate) fn daytime(&mut self, stamp_us: u64, minute: u16) {
        self.daytime = Some((stamp_us, minute));
    }

    /// The time of day is no longer known: the clock was found not set
    /// (#830). Night is not worked out from the last one given.
    pub(crate) fn forget_daytime(&mut self) {
        self.daytime = None;
    }

    fn night(&self, now_us: u64) -> bool {
        let (Some(sleep), Some((at, minute))) = (&self.alive.sleep, self.daytime) else {
            return false;
        };
        let since = now_us.saturating_sub(at);
        if since >= DAYTIME_STALE_US {
            return false;
        }
        let now = (u64::from(minute) + since / 60_000_000) % 1_440;
        let [from, until] = sleep.night.map(u64::from);
        if from < until {
            (from..until).contains(&now)
        } else {
            now >= from || now < until
        }
    }

    /// The goals the body said for its joints (`body/joint_states`) at
    /// `stamp_us`: whether it is seen moving ([`BODY_FAST`]).
    pub(crate) fn joints(&mut self, stamp_us: u64, goals: &[f64]) {
        let fast = match &self.body_goals {
            Some((was_us, was)) if stamp_us > *was_us && was.len() == goals.len() => {
                let seconds = (stamp_us - was_us) as f64 / 1e6;
                was.iter()
                    .zip(goals)
                    .any(|(was, goal)| (goal - was).abs() / seconds >= BODY_FAST)
            }
            _ => false,
        };
        self.body_fast_since = if fast {
            Some(self.body_fast_since.unwrap_or(stamp_us))
        } else {
            None
        };
        if self
            .body_fast_since
            .is_some_and(|since| stamp_us - since >= BODY_MOVING_US)
        {
            self.body_moved_us = Some(stamp_us);
        }
        self.body_goals = (!goals.is_empty()).then(|| (stamp_us, goals.to_vec()));
    }

    /// The eyes' newest look: whom they see, how close and how still.
    pub(crate) fn faces(&mut self, stamp_us: u64, faces: &Faces, confidence: f32) {
        self.faces_at = Some(stamp_us);
        let seen: Vec<_> = faces
            .faces
            .iter()
            .filter(|face| face.confidence >= confidence)
            .collect();
        // The face looked up at after a tap: the largest one seen.
        self.face_y = seen
            .iter()
            .max_by(|a, b| {
                let area = |f: &&&lamp_msgs::Face| (f.xmax - f.xmin) * (f.ymax - f.ymin);
                area(a).total_cmp(&area(b))
            })
            .map(|face| {
                (
                    stamp_us,
                    f64::from(face.ymin + face.ymax - 1.0).clamp(-1.0, 1.0),
                )
            });
        if seen.is_empty() {
            self.nobody_since.get_or_insert(stamp_us);
            self.anchor = None;
            return;
        }
        self.nobody_since = None;
        // Morning: the first person seen after the night wakes it. At
        // night a face does not: only its name, a touch or a reminder.
        if self.asleep() && !self.night(stamp_us) {
            self.called = true;
        }
        let area = |face: &&lamp_msgs::Face| (face.xmax - face.xmin) * (face.ymax - face.ymin);
        let close = seen
            .iter()
            .filter(|face| area(face) >= self.alive.work.face_area)
            .max_by(|a, b| area(a).total_cmp(&area(b)));
        let Some(face) = close else {
            self.anchor = None;
            return;
        };
        let middle = [(face.xmin + face.xmax) / 2.0, (face.ymin + face.ymax) / 2.0];
        match self.anchor {
            Some((from, since))
                if (middle[0] - from[0]).abs() <= self.alive.work.still
                    && (middle[1] - from[1]).abs() <= self.alive.work.still =>
            {
                if stamp_us.saturating_sub(since) >= self.alive.work.still_us {
                    self.worked = Some(stamp_us);
                }
            }
            _ => self.anchor = Some((middle, stamp_us)),
        }
    }

    fn name_requires_speech(&self) -> bool {
        self.alive
            .sleep
            .is_some_and(|sleep| sleep.name_requires_speech)
    }

    /// Its name was heard. The source onset is not the bus receipt time.
    pub(crate) fn name(&mut self, now_us: u64, stamp_us: u64, ago_ms: u32) {
        if !self.asleep() {
            return;
        }
        if self.name_requires_speech() {
            self.name_called |= self.name_wake.name(now_us, stamp_us, ago_ms);
        } else {
            self.called = true;
        }
    }

    /// Called only after Engine accepts the voice owner's speaking evidence.
    pub(crate) fn person_speech(&mut self, stamp_us: u64) {
        if self.asleep() && self.name_requires_speech() {
            self.name_called |= self.name_wake.speech(stamp_us);
        }
    }

    /// This runs even when a planner tick cannot issue an actuator request.
    pub(crate) fn advance_name(&mut self, now_us: u64) {
        self.name_wake.advance(now_us);
    }

    pub(crate) fn fence_name(&mut self, stamp_us: u64) {
        self.name_called = false;
        self.name_wake.clear(true);
        self.name_speech_since(stamp_us);
    }

    /// A delayed detector heartbeat from before input was retained again
    /// must not corroborate a new name in the same voice phase and turn.
    pub(crate) fn name_speech_since(&mut self, stamp_us: u64) {
        self.name_wake.speech_since = self.name_wake.speech_since.max(stamp_us);
    }

    pub(crate) fn take_name_notices(&mut self) -> Vec<String> {
        self.name_wake.take_notices()
    }

    /// A touch: a tap or a stroke wakes it. A long hold of the base is the
    /// mute today (`controls`), and Sleep once #711 makes it so; it does
    /// not wake it.
    pub(crate) fn touch(&mut self, gesture: &ControlGesture) {
        if gesture.enabled && gesture.gesture != Gesture::LongPress {
            self.called |= self.asleep();
        }
    }

    /// A request from the head (#711), from `--touch` Controls. A tap wakes
    /// it; a hand kept a second is leaned into while it stays; a hold of
    /// three seconds puts it to sleep. Asleep, only the tap does anything.
    pub(crate) fn head_touch(&mut self, stamp_us: u64, event: &TouchEvent) {
        if !event.enabled || event.source != TouchSource::Head {
            return;
        }
        match event.gesture {
            // Muted for privacy, a tap gets no look up: looking up says
            // "I'm listening", which a muted lamp must not seem to do
            // (First on #711, 17 Oct 10:18). Nor does it wake it. A tap may
            // be what clears the voice's own mute: then it gets its answer
            // when the voice says it is open ([`Life::voice_mute`]).
            TouchGesture::Tap if self.muted() => {
                if !self.privacy_muted {
                    self.tapped_muted_us = Some(stamp_us);
                }
            }
            // Asleep, a tap wakes it; awake, it looks up at it.
            TouchGesture::Tap if self.asleep() => self.called = true,
            TouchGesture::Tap => self.looking_until_us = Some(stamp_us + LOOK_UP_FOR_US),
            TouchGesture::Stroke if !self.asleep() => {
                self.hand = Some(event.contact_id);
                self.hand_seen_us = Some(stamp_us);
            }
            TouchGesture::Hold if !self.asleep() => {
                self.sleep_asked = true;
                self.hand = None;
            }
            _ => {}
        }
    }

    /// The voice's mute (#711). Sleep from the head is the voice's mute with
    /// the reason `sleep`: microphones off. While the voice says so, the lamp
    /// sleeps, and nothing but the voice opening again wakes it.
    pub(crate) fn voice_mute(&mut self, stamp_us: u64, mute: &VoiceMute) {
        self.voice_said = Some((stamp_us, mute.asleep()));
        let muted = mute.muted && !mute.asleep();
        let recent = |at: u64| stamp_us.saturating_sub(at) < TAP_OPENS_US;
        if muted {
            // Still muted, said every second: the mark waits for the word
            // that opens, and only ages out (the reviewer on #1282).
            self.tapped_muted_us = self.tapped_muted_us.filter(|at| recent(*at));
        } else {
            // Opened just after a tap: that tap cleared the mute (the
            // restart guard's, `LAMP_RESTORED_MUTE`), and it is answered
            // now, as any tap on an open lamp is.
            let tapped = self.tapped_muted_us.take();
            if self.voice_muted && !self.privacy_muted && tapped.is_some_and(recent) {
                if self.asleep() {
                    self.called = true;
                } else {
                    self.looking_until_us = Some(stamp_us + LOOK_UP_FOR_US);
                }
            }
        }
        self.voice_muted = muted;
    }

    /// Whether the privacy switch was closed at the last request from the
    /// head (`voice/privacy`, read then).
    pub(crate) fn set_privacy_muted(&mut self, muted: bool) {
        self.privacy_muted = muted;
    }

    /// The lamp's one breath's phase now, where Mood keeps it (#1670).
    pub(crate) fn set_mood_breath(&mut self, phase: Option<f64>) {
        self.mood_breath = phase;
    }

    /// How lively the moves are from the lamp's inner state (#1670).
    pub(crate) fn set_mood_liveliness(&mut self, liveliness: Option<f32>) {
        self.mood_liveliness = liveliness;
    }

    /// The energy the style of every move is drawn from: the catalogue's,
    /// moved by the lamp's inner state where Mood says it, within 0 to 1, so
    /// no move passes the bounds checked at energy 1.
    fn energy(&self) -> f32 {
        (self.alive.energy + self.mood_liveliness.unwrap_or(0.0)).clamp(0.0, 1.0)
    }

    /// Muted for privacy: the switch last said closed, or the voice's own
    /// mute last said (not its sleep, which a tap wakes).
    fn muted(&self) -> bool {
        self.privacy_muted || self.voice_muted
    }

    /// Whether it is looking up at a tap: while so, the runtime reads the
    /// privacy switch at each step, so a switch closed mid-look ends it.
    pub(crate) fn looking_up(&self) -> bool {
        self.looking_until_us.is_some() || self.look_up > 0.0
    }

    /// The head's state at `stamp_us`: the hand leaned into is there only
    /// while its contact lasts and counts (`held_us`; none once a push from
    /// the body spoiled it).
    pub(crate) fn head_hand(&mut self, stamp_us: u64, state: &TouchState) {
        let head = state
            .sources
            .iter()
            .find(|source| source.source == TouchSource::Head);
        let there = head.is_some_and(|head| {
            head.held_us.is_some() && head.contact_id.is_some() && head.contact_id == self.hand
        });
        if there {
            self.hand_seen_us = Some(stamp_us);
        } else if head.is_some_and(|head| head.contact_id != self.hand || head.held_us.is_none()) {
            self.hand = None;
        }
    }

    /// A reminder that has come due wakes it, once for each delivery.
    pub(crate) fn due(&mut self, due: &ScheduleDue) {
        let Some(notice) = due.oldest.as_ref().filter(|_| due.enabled && due.ready) else {
            return;
        };
        let key = (notice.id, notice.delivery);
        if self.last_due != Some(key) {
            self.last_due = Some(key);
            self.called |= self.asleep();
        }
    }

    fn working(&self, now_us: u64) -> bool {
        self.worked
            .is_some_and(|at| now_us.saturating_sub(at) <= self.alive.work.hold_us)
    }

    fn drift_now(&self, now_us: u64, pace: f64) -> BodyOffsets {
        match self.drift_started {
            Some(started) => {
                let length = self.alive.drift.move_us as f64 * pace;
                blend(
                    self.drift_from,
                    self.drift_to,
                    ease(now_us.saturating_sub(started) as f64 / length),
                )
            }
            None => self.drift_to,
        }
    }

    fn drift_wait(&self, pace: f64) -> u64 {
        let [soonest, latest] = self.alive.drift.every_us;
        let wait = soonest as f64 + fraction(self.drifts, 1) * (latest - soonest) as f64;
        (wait * pace) as u64
    }

    fn drift_target(&self, size: f64) -> BodyOffsets {
        let low = self.alive.drift.low.values();
        let high = self.alive.drift.high.values();
        BodyOffsets::from_values(std::array::from_fn(|axis| {
            (low[axis] + fraction(self.drifts, axis as u64 + 2) * (high[axis] - low[axis])) * size
        }))
    }

    fn forget_drift(&mut self) {
        self.drift_from = BodyOffsets::ZERO;
        self.drift_to = BodyOffsets::ZERO;
        self.drift_started = None;
        self.drift_next = None;
    }

    /// The layer's offsets at `now_us`. Asleep, they stand in for everything
    /// else Expression would ask of the body; waking, they cross back over to
    /// it; awake, they are laid under it.
    pub(crate) fn step(&mut self, now_us: u64, context: &Context) -> Planned {
        let step = self
            .last_us
            .map_or(0, |last| now_us.saturating_sub(last))
            .min(200_000);
        self.last_us = Some(now_us);
        let size = size(self.energy());
        let pace = pace(self.energy());
        let mut planned = Planned::default();
        // Nobody is absent while the eyes are not looking, nor while the
        // voice has someone to listen or answer to.
        if self
            .faces_at
            .is_none_or(|at| now_us.saturating_sub(at) >= EYES_QUIET_US)
            || context.voice != Some(Voice::Resting)
        {
            self.nobody_since = None;
        }
        // Thinking or speaking, it has been spoken to: it is awake.
        if self.asleep() && matches!(context.voice, Some(Voice::Thinking | Voice::Speaking)) {
            self.called = true;
        }
        // Where the voice says its mute (#711), sleep from the head is the
        // voice's: asleep while it says so, whatever calls; awake when it
        // opens. A hold it did not take as sleep is not one here either.
        let voice_asleep = self
            .voice_said
            .filter(|(at, _)| now_us.saturating_sub(*at) < VOICE_MUTE_KEPT_US)
            .map(|(_, asleep)| asleep);
        match voice_asleep {
            Some(true) => {
                self.called = false;
                self.name_called = false;
                self.sleep_asked = false;
            }
            Some(false) => {
                self.sleep_asked = false;
                if self.voice_slept {
                    self.called |= self.asleep();
                }
            }
            None => {}
        }
        if let Some(asleep) = voice_asleep {
            self.voice_slept = asleep;
        }
        if let (Phase::Asleep { since }, true) = (self.phase, self.called || self.name_called) {
            // Woken, it sleeps again only after nobody again for as long.
            self.nobody_since = None;
            let (from, settled) = self.sleeping(now_us, since);
            self.phase = Phase::Waking {
                since: now_us,
                from,
                kept: 1.0 - settled,
            };
            self.name_wake.clear(false);
        }
        self.called = false;
        self.name_called = false;
        // A hand held three seconds on its head: it goes to sleep now,
        // whatever the hour (#711), and so it does when the voice says so,
        // from waking too.
        if ((std::mem::take(&mut self.sleep_asked) && self.phase == Phase::Awake)
            || (voice_asleep == Some(true) && !self.asleep()))
            && self.alive.sleep.is_some()
        {
            self.phase = Phase::Asleep { since: now_us };
            self.name_wake.clear(false);
        }
        if let (Phase::Awake, Some(sleep)) = (self.phase, &self.alive.sleep)
            && context.free
            && context.voice == Some(Voice::Resting)
            && self.night(now_us)
            && self
                .nobody_since
                .is_some_and(|since| now_us.saturating_sub(since) >= sleep.nobody_us)
        {
            self.phase = Phase::Asleep { since: now_us };
            self.name_wake.clear(false);
        }
        match self.phase {
            Phase::Awake => {}
            Phase::Asleep { since } => {
                let (offsets, settled) = self.sleeping(now_us, since);
                planned.offsets = offsets;
                planned.keep = 1.0 - settled;
                planned.asleep = true;
                let settle = self.alive.sleep.map_or(0, |sleep| sleep.settle_us);
                planned
                    .names
                    .push(if now_us.saturating_sub(since) < settle {
                        "falling asleep"
                    } else {
                        "asleep"
                    });
            }
            Phase::Waking { since, from, kept } => {
                let elapsed = now_us.saturating_sub(since);
                let length = match &self.alive.wake {
                    Some(wake) => wake.duration_us() as f64 * pace,
                    None => self.alive.sleep.map_or(1.0, |sleep| sleep.settle_us as f64),
                };
                let share = elapsed as f64 / length;
                let stretch = self.alive.wake.as_ref().map_or(BodyOffsets::ZERO, |wake| {
                    let at = (elapsed as f64 / pace) as u64;
                    add(BodyOffsets::ZERO, wake.sample(at), size)
                });
                planned.offsets = add(stretch, from, 1.0 - ease(share));
                planned.keep = kept + (1.0 - kept) * ease(share);
                planned.names.push("waking");
                if share >= 1.0 {
                    self.phase = Phase::Awake;
                }
            }
        }
        // The idle layer comes in where it may and goes out where it may
        // not, over `blend_us` either way.
        let allowed = self.phase == Phase::Awake
            && context.free
            && context
                .voice
                .is_some_and(|voice| self.alive.states.contains(&voice))
            && !self.working(now_us);
        // Falling asleep, it lets go as slowly as it settles: a creature
        // nodding off does not drop its head in a third of a second.
        let over = match (self.phase, self.alive.sleep) {
            (Phase::Asleep { .. }, Some(sleep)) => sleep.settle_us,
            _ => self.alive.blend_us,
        };
        let change = step as f64 / over as f64;
        self.weight = if allowed {
            (self.weight + change).min(1.0)
        } else {
            (self.weight - change).max(0.0)
        };
        if self.phase == Phase::Awake
            && context.free
            && self.working(now_us)
            && context
                .voice
                .is_some_and(|voice| self.alive.states.contains(&voice))
        {
            planned.names.push("still for work");
        }
        // Leaning into a hand kept on its head, only awake and with the body
        // free; it eases in and out, never leaps.
        if self
            .hand_seen_us
            .is_none_or(|seen| now_us.saturating_sub(seen) >= HAND_FRESH_US)
        {
            self.hand = None;
        }
        let leaning = self.hand.is_some() && self.phase == Phase::Awake && context.free;
        self.lean = if leaning {
            (self.lean + step as f64 / LEAN_IN_US as f64).min(1.0)
        } else {
            // Rounding must not leave it leaning by a hair forever.
            let left = self.lean - step as f64 / LEAN_OUT_US as f64;
            if left < 1e-9 { 0.0 } else { left }
        };
        if self.lean > 0.0 {
            planned.offsets = add(planned.offsets, LEAN, ease(self.lean));
            planned.names.push("leaning into a hand");
        }
        // Looking up at a tap on its head, awake and with the body free, as
        // long as a call waits, and on while someone is speaking; never while
        // the lamp thinks or speaks. It eases in fast and out slowly.
        if context.voice == Some(Voice::Listening)
            && let Some(until) = self.looking_until_us.as_mut()
            && now_us < *until
        {
            // A fifth of a second on from each step heard: the longest step.
            *until = (*until).max(now_us + 200_000);
        }
        let looking = self.looking_until_us.is_some_and(|until| now_us < until)
            && self.phase == Phase::Awake
            && context.free
            && !self.muted()
            && !matches!(context.voice, Some(Voice::Thinking | Voice::Speaking));
        if !looking {
            self.looking_until_us = None;
        }
        self.look_up = if looking {
            (self.look_up + step as f64 / LOOK_UP_IN_US as f64).min(1.0)
        } else {
            let left = self.look_up - step as f64 / LOOK_UP_OUT_US as f64;
            if left < 1e-9 { 0.0 } else { left }
        };
        if self.look_up > 0.0 {
            let toward = self
                .face_y
                .filter(|(at, _)| now_us.saturating_sub(*at) < LOOK_UP_FACE_US)
                .map_or(0.0, |(_, y)| -y * LOOK_UP_TOWARD);
            let look = BodyOffsets {
                head_pitch: LOOK_UP.head_pitch - toward,
                ..LOOK_UP
            };
            planned.offsets = add(planned.offsets, look, ease(self.look_up));
            planned.names.push("looking up at a tap");
        }
        if self.weight == 0.0 {
            self.forget_drift();
            self.breath = None;
            return planned;
        }
        // The breath, as the ring's. Its phase jumps once an hour where the
        // ring's does; the body's breath is let catch up at twice its own
        // fastest speed rather than leap.
        let wanted = self.alive.breath.at(now_us, size, self.mood_breath);
        let most = 2.0 * self.alive.breath.speed(size) * step as f64 / 1e6;
        let breath = match self.breath {
            Some(last) => {
                let last = last.values();
                BodyOffsets::from_values(std::array::from_fn(|axis| {
                    let wanted = wanted.values()[axis];
                    last[axis] + (wanted - last[axis]).clamp(-most, most)
                }))
            }
            None => wanted,
        };
        self.breath = Some(breath);
        // Now and then a small drift, eased, held until the next. None
        // begins while the layer is going out: a lamp nodding off or turning
        // to someone does not shift its weight on the way.
        if self.drift_next.is_none() && self.drift_started.is_none() {
            self.drift_next = Some(now_us.saturating_add(self.drift_wait(pace)));
        }
        let body_quiet = self
            .body_moved_us
            .is_none_or(|at| now_us.saturating_sub(at) >= BODY_QUIET_US);
        if allowed
            && body_quiet
            && self.drift_started.is_none()
            && self.drift_next.is_some_and(|at| now_us >= at)
        {
            self.drift_from = self.drift_to;
            self.drift_to = self.drift_target(size);
            self.drift_started = Some(now_us);
            self.drift_next = None;
            self.drifts += 1;
        }
        let drift = self.drift_now(now_us, pace);
        if let Some(started) = self.drift_started {
            planned.names.push("drifting");
            if now_us.saturating_sub(started) as f64 >= self.alive.drift.move_us as f64 * pace {
                self.drift_from = self.drift_to;
                self.drift_started = None;
                self.drift_next = Some(now_us.saturating_add(self.drift_wait(pace)));
            }
        }
        planned.names.push("breathing");
        let layer = add(breath, drift, 1.0);
        planned.offsets = add(planned.offsets, layer, ease(self.weight));
        planned
    }

    /// The sleeping pose and its breath at `now_us`, as far as it has
    /// settled into them, and how far that is.
    fn sleeping(&self, now_us: u64, since: u64) -> (BodyOffsets, f64) {
        let Some(sleep) = self.alive.sleep else {
            return (BodyOffsets::ZERO, 0.0);
        };
        let settled = ease(now_us.saturating_sub(since) as f64 / sleep.settle_us as f64);
        // The sleeping breath is its own (9 s in the catalogue), never the
        // ring's: it keeps the boot clock.
        let pose = add(sleep.pose, sleep.breath.at(now_us, 1.0, None), 1.0);
        (add(BodyOffsets::ZERO, pose, settled), settled)
    }
}
