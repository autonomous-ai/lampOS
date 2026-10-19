//! The creature's temporary inner state. These values describe manner only;
//! they never grant microphone, camera, speech, motion or listening authority.
use crate::{ClockStamp, Invalid};
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const MOOD_STATE_PERIOD_US: u64 = 1_000_000;
pub const MOOD_STATE_TTL_US: u64 = 2_500_000;
pub const MOOD_MAX_FACE_ID_BYTES: usize = 128;

/// The lamp's one resting breath at the plain pace: the ring's breath and the
/// head's (`ring_face.rs`, Expression's `alive.rs`) on the boot clock's
/// seconds within the hour, when no fresh mood says otherwise.
pub const MOOD_BREATH_US: u64 = 5_200_000;
/// The quickest and the slowest pace of that breath, as a share of the plain
/// pace (#1670): a 5.2 s breath is about 4.2 s at the quickest and 7.4 s at
/// the slowest. Authored starting values, small on purpose.
pub const MOOD_PACE_QUICKEST: f32 = 1.25;
pub const MOOD_PACE_SLOWEST: f32 = 0.7;
/// Mood's calm energy, at which energy adds no pace, and its highest, at
/// which it adds the most; under [`MOOD_PACE_DOZING`] awake the breath slows.
const CALM_ENERGY: f32 = 0.30;
const MOST_ENERGY: f32 = 0.70;
const MOOD_PACE_DOZING: f32 = 0.40;

/// How quickly the lamp breathes in this state, as a share of the plain
/// pace: 1 when calm and awake (0.40 or more), quicker with energy over the
/// calm, slower as awake falls, and the slowest asleep.
pub fn mood_pace(awake: f32, energy: f32, asleep: bool) -> f32 {
    if asleep {
        return MOOD_PACE_SLOWEST;
    }
    let (lively, dozy) = lively_and_dozy(awake, energy);
    (1.0 + (MOOD_PACE_QUICKEST - 1.0) * lively - (1.0 - MOOD_PACE_SLOWEST) * dozy)
        .clamp(MOOD_PACE_SLOWEST, MOOD_PACE_QUICKEST)
}

/// How far the lamp's moves go from their authored style in this state,
/// from -0.5 to 0.5: its [`mood_pace`] laid on that range, 0 at the plain
/// pace, 0.5 at the quickest and -0.5 at the slowest, so the moves are
/// livelier exactly where the breath is quicker and never the other way.
/// Expression adds it to its catalogue's energy (#1670).
pub fn mood_liveliness(awake: f32, energy: f32, asleep: bool) -> f32 {
    let pace = mood_pace(awake, energy, asleep);
    if pace >= 1.0 {
        0.5 * (pace - 1.0) / (MOOD_PACE_QUICKEST - 1.0)
    } else {
        -0.5 * (1.0 - pace) / (1.0 - MOOD_PACE_SLOWEST)
    }
    .clamp(-0.5, 0.5)
}

/// How lively, from energy over the calm, and how dozy, as awake falls
/// under [`MOOD_PACE_DOZING`]: each from 0 to 1.
fn lively_and_dozy(awake: f32, energy: f32) -> (f32, f32) {
    (
        ((energy - CALM_ENERGY) / (MOST_ENERGY - CALM_ENERGY)).clamp(0.0, 1.0),
        ((MOOD_PACE_DOZING - awake) / MOOD_PACE_DOZING).clamp(0.0, 1.0),
    )
}

/// The lamp's one breath as Mood keeps it (#1670): where it is at the
/// state's stamp, from 0 (out) through 0.5 (in) to 1, and its pace there.
/// Every reader takes its phase from this, so the ring and the head breathe
/// as one; Mood moves it on at the pace and never jumps it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MoodBreath {
    pub phase: f32,
    pub pace: f32,
}

/// A single, temporary focus. A face is scoped to its publisher instance,
/// never a person's identity. Sound carries no invented sound direction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttentionTarget {
    Face { publisher_boot: String, id: String },
    Sound,
    Touch,
}

impl AttentionTarget {
    pub fn check(&self) -> Result<(), Invalid> {
        if let Self::Face { publisher_boot, id } = self {
            if publisher_boot.len() != 16
                || !publisher_boot
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(Invalid(
                    "face publisher boot must be 16 lowercase hex characters".into(),
                ));
            }
            if id.is_empty()
                || id.len() > MOOD_MAX_FACE_ID_BYTES
                || id.chars().any(char::is_control)
            {
                return Err(Invalid(
                    "face id must have 1..128 bytes without controls".into(),
                ));
            }
        }
        Ok(())
    }
}

/// Recognition is deliberately unavailable. Passing face tracks are not
/// evidence that the lamp knows someone, even after repeated observations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Familiarity {
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MoodState {
    pub stamp: ClockStamp,
    pub session_id: String,
    pub awake: f32,
    pub energy: f32,
    pub warmth: f32,
    /// A metadata sleep latch, not proof that microphones or outputs closed.
    pub asleep: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention: Option<AttentionTarget>,
    pub familiarity: Familiarity,
    /// How familiar the remembered person at the lamp is, while the eyes
    /// say they are there (#1670, `humans/familiar`): a level only, no name.
    /// Optional, beside `familiarity: unknown`, which stays for v1 readers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub familiar: Option<crate::FamiliarLevel>,
    /// The lamp's one breath (#1670). Optional: a state without it says
    /// nothing about the breath.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breath: Option<MoodBreath>,
}

impl MoodState {
    pub const TYPE: &str = "lamp_msgs/MoodState";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "mood/state";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_micros(MOOD_STATE_PERIOD_US))
            .lasting(Duration::from_micros(MOOD_STATE_TTL_US))
    }

    /// Shape only: consumers also check current boot, publisher session,
    /// monotonic ordering, expiry and their own independent output authority.
    pub fn check(&self) -> Result<(), Invalid> {
        self.stamp.check()?;
        if self.stamp.sequence == 0
            || self.session_id.is_empty()
            || self.session_id.len() > 128
            || self.session_id.chars().any(char::is_control)
        {
            return Err(Invalid(
                "mood needs a positive sequence and a bounded session identity".into(),
            ));
        }
        for (name, value) in [
            ("awake", self.awake),
            ("energy", self.energy),
            ("warmth", self.warmth),
        ] {
            if !(0.0..=1.0).contains(&value) {
                return Err(Invalid(format!(
                    "mood {name} must be finite and from 0 to 1"
                )));
            }
        }
        if self.asleep && (self.awake > 0.25 || self.attention.is_some()) {
            return Err(Invalid(
                "sleeping mood cannot claim high awake or an attention target".into(),
            ));
        }
        if let Some(attention) = &self.attention {
            attention.check()?;
        }
        if let Some(breath) = self.breath
            && (!(0.0..1.0).contains(&breath.phase)
                || !(MOOD_PACE_SLOWEST..=MOOD_PACE_QUICKEST).contains(&breath.pace))
        {
            return Err(Invalid(
                "mood breath needs a phase from 0 to 1 and a pace in its bounds".into(),
            ));
        }
        Ok(())
    }

    /// The breath at `now_us` on the stamp's monotonic clock: its phase,
    /// moved on from the stamp at the state's pace, and that pace. None
    /// without a breath, or for a moment before the stamp or at or past the
    /// state's [`MOOD_STATE_TTL_US`].
    pub fn breath_at(&self, now_us: u64) -> Option<(f64, f32)> {
        let breath = self.breath?;
        let since = now_us.checked_sub(self.stamp.at_us)?;
        (since < MOOD_STATE_TTL_US).then_some(())?;
        let moved = since as f64 / MOOD_BREATH_US as f64 * f64::from(breath.pace);
        Some(((f64::from(breath.phase) + moved).fract(), breath.pace))
    }

    /// The breath at `now_us` in a bus message: only from source `mood`,
    /// unexpired, checked, and as [`Self::breath_at`] says. Every reader of
    /// the breath takes it here, so none reads it differently.
    pub fn breath_in(message: &lamp_bus::Message, now_us: u64) -> Option<(f64, f32)> {
        Self::fresh_in(message, now_us)?.breath_at(now_us)
    }

    /// The state in a bus message if it holds at `now_us`: from source
    /// `mood`, unexpired, checked, stamped no later than now and within its
    /// [`MOOD_STATE_TTL_US`]. Every reader takes Mood's state here.
    pub fn fresh_in(message: &lamp_bus::Message, now_us: u64) -> Option<Self> {
        if message.expired || message.header.source != "mood" {
            return None;
        }
        let state = message.parse::<Self>().ok()?;
        state.check().ok()?;
        let since = now_us.checked_sub(state.stamp.at_us)?;
        (since < MOOD_STATE_TTL_US).then_some(state)
    }

    /// [`mood_liveliness`] of this state.
    pub fn liveliness(&self) -> f32 {
        mood_liveliness(self.awake, self.energy, self.asleep)
    }
}

/// The plain breath's phase at `now_us`, from 0 to 1: the boot clock's
/// seconds within the hour over [`MOOD_BREATH_US`], as the ring and the head
/// have always breathed without a mood.
pub fn plain_breath_phase(now_us: u64) -> f64 {
    let seconds = (now_us % 3_600_000_000) as f64;
    (seconds / MOOD_BREATH_US as f64).fract()
}

/// Optional Mood contract, kept separate from the original first topics.
pub fn mood_topics() -> Vec<(&'static str, Declaration)> {
    vec![(MoodState::TOPIC, MoodState::declaration())]
}
