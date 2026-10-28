use crate::{Alive, Error};
use lamp_msgs::{BodyOffsets, EXPRESSION_MAX_FUTURE_US, EXPRESSION_MAX_OFFSET_RAD, Voice};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_CATALOGUE_BYTES: usize = 128 * 1024;
pub const MAX_BEHAVIOURS: usize = 24;
pub const MAX_KEYFRAMES: usize = 64;

/// The engine understands these evidence sources, not particular clip names.
/// More clips for an existing trigger can be added without changing Rust.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    Thinking,
    Pause,
    Playout,
    Interrupted,
    Concentrating,
    Rest,
    /// A short sound rose suddenly out of the room's quiet (`ears/sudden`).
    /// Only the startle skill's own clip ([`crate::startle`], #2822): a
    /// catalogue file's is dropped, and without the skill's none reacts.
    Sudden,
    /// Someone in a busy room said something about the lamp and the voice
    /// chose to say nothing (`voice/remarked`, #4175). Only the glance
    /// Expression derives from the startle skill's move, with
    /// `LAMP_REMARK_GLANCE=on` ([`crate::startle::remark`]): a catalogue
    /// file's is dropped, as a sudden clip is.
    Remarked,
}

impl Trigger {
    /// The room's, not the voice's: a glance at a sudden sound or a remark
    /// outlives a turn and finishes once begun (#2822, #4175).
    pub fn is_rooms(self) -> bool {
        matches!(self, Self::Sudden | Self::Remarked)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    pub at_us: u64,
    pub offsets: BodyOffsets,
    /// Additive brightness relative to the face's base, bounded to -1..1.
    pub brightness: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Behaviour {
    pub name: String,
    pub trigger: Trigger,
    pub weight: f64,
    /// Delay from an actual triggering edge; a heartbeat does not restart it.
    pub delay_us: u64,
    pub cooldown_us: u64,
    /// Preparation ends at this authored stroke; the remainder relaxes.
    pub stroke_us: u64,
    pub repeat: bool,
    pub hold: bool,
    pub quiet_only: bool,
    pub stop_when_concentrating: bool,
    pub states: Vec<Voice>,
    /// States in which a clip that already began may finish. This does not
    /// authorize starting a new clip there. Only a pause nod may continue into
    /// Thinking; new speech and retired turns still cancel it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub continue_in: Vec<Voice>,
    pub keyframes: Vec<Keyframe>,
}

impl Behaviour {
    pub fn duration_us(&self) -> u64 {
        self.keyframes.last().map_or(0, |frame| frame.at_us)
    }

    /// One clip's own bounds, whichever catalogue or skill file it came
    /// from: a startle read from its skill's file is held to the same.
    pub fn check(&self) -> Result<(), Error> {
        let clip = self;
        if !(0.0..=1.0).contains(&clip.weight) {
            return Err(Error(format!(
                "{}: weight must be finite in 0..1",
                clip.name
            )));
        }
        if clip.states.is_empty() || clip.states.len() > 4 {
            return Err(Error(format!(
                "{}: declare 1..4 allowed voice states",
                clip.name
            )));
        }
        if clip
            .states
            .iter()
            .enumerate()
            .any(|(index, value)| clip.states[..index].contains(value))
        {
            return Err(Error(format!("{}: voice states must be unique", clip.name)));
        }
        if !clip.continue_in.is_empty()
            && (clip.trigger != Trigger::Pause
                || clip.states != [Voice::Listening]
                || clip.continue_in != [Voice::Thinking])
        {
            return Err(Error(
                "only a listening pause nod may continue into Thinking".into(),
            ));
        }
        if clip.trigger != Trigger::Rest
            && (clip.delay_us > 1_000_000 || clip.cooldown_us > 60_000_000)
        {
            return Err(Error(format!("{}: unbounded delay or cooldown", clip.name)));
        }
        if clip.keyframes.len() < 2
            || clip.keyframes.len() > MAX_KEYFRAMES
            || clip.keyframes[0].at_us != 0
            || clip.keyframes.windows(2).any(|v| v[0].at_us >= v[1].at_us)
            || clip.duration_us() > 10_000_000
            || clip.stroke_us > clip.duration_us()
            || (clip.repeat && clip.hold)
        {
            return Err(Error(format!(
                "{}: keyframes must begin at zero, increase and last at most 10 seconds",
                clip.name
            )));
        }
        for frame in &clip.keyframes {
            frame.offsets.check()?;
            if !(-1.0..=1.0).contains(&frame.brightness) {
                return Err(Error(format!(
                    "{}: brightness must be finite in -1..1",
                    clip.name
                )));
            }
        }
        if clip.trigger == Trigger::Pause
            && (clip.repeat
                || clip.hold
                || clip.duration_us()
                    > if clip.continue_in.is_empty() {
                        600_000
                    } else {
                        2_000_000
                    }
                || clip.cooldown_us < 2_000_000)
        {
            return Err(Error(
                "a pause nod needs a 2 s cooldown and at most 600 ms, or 2 s with Thinking continuation".into(),
            ));
        }
        if clip.trigger == Trigger::Rest
            && (clip.states != [Voice::Resting]
                || !clip.continue_in.is_empty()
                || !clip.quiet_only
                || !clip.stop_when_concentrating
                || clip.repeat
                || clip.hold
                || !(20_000_000..=3_600_000_000).contains(&clip.delay_us)
                || !(60_000_000..=3_600_000_000).contains(&clip.cooldown_us)
                || !(2_000_000..=6_000_000).contains(&clip.duration_us())
                || clip.keyframes.first().unwrap().offsets != BodyOffsets::ZERO
                || clip.keyframes.last().unwrap().offsets != BodyOffsets::ZERO
                || clip.keyframes.iter().any(|frame| {
                    frame.offsets.base_yaw != 0.0
                        || frame.offsets.shoulder != 0.0
                        || frame.offsets.elbow != 0.0
                        || frame.offsets.head_pitch.abs() > 0.06
                        || frame.offsets.head_roll.abs() > 0.06
                        || frame.brightness != 0.0
                })
                || clip.keyframes.windows(2).any(|pair| {
                    let seconds = (pair[1].at_us - pair[0].at_us) as f64 / 1_000_000.0;
                    [
                        pair[1].offsets.head_pitch - pair[0].offsets.head_pitch,
                        pair[1].offsets.head_roll - pair[0].offsets.head_roll,
                    ]
                    .into_iter()
                    .any(|delta| {
                        // Smoothstep's maximum speed is 1.5 * displacement
                        // / duration; maximum acceleration is 6 * displacement
                        // / duration squared. A long clip cannot hide a snap.
                        1.5 * delta.abs() / seconds > 0.06
                            || 6.0 * delta.abs() / (seconds * seconds) > 0.12
                    })
                }))
        {
            return Err(Error(
                "rest needs one slow quiet-only head clip, zero endpoints, 20 s delay and 60 s cooldown or longer".into(),
            ));
        }
        if clip.trigger == Trigger::Remarked
            && (clip
                .states
                .iter()
                .any(|state| !matches!(state, Voice::Resting | Voice::Listening))
                || clip.cooldown_us < 60_000_000
                || clip.stroke_us > 1_500_000)
        {
            return Err(Error(
                "a glance at a remark is never while thinking or speaking, at most once a minute and within 1.5 s of the remark".into(),
            ));
        }
        if matches!(clip.trigger, Trigger::Sudden | Trigger::Remarked)
            && (clip.repeat
                || clip.hold
                || clip.states.iter().any(|state| {
                    !matches!(state, Voice::Resting | Voice::Listening | Voice::Thinking)
                })
                || !clip.continue_in.is_empty()
                || clip.duration_us() > 2_000_000
                || clip.cooldown_us < 5_000_000
                || clip.keyframes.first().unwrap().offsets != BodyOffsets::ZERO
                || clip.keyframes.last().unwrap().offsets != BodyOffsets::ZERO
                || clip.keyframes.iter().any(|frame| {
                    frame.offsets.base_yaw != 0.0
                        || frame.offsets.shoulder != 0.0
                        || frame.offsets.elbow != 0.0
                        || frame.brightness != 0.0
                }))
        {
            return Err(Error(
                "a glance at a sudden sound or a remark is one head clip of at most 2 s from and back to rest, never while speaking, with a 5 s cooldown or longer".into(),
            ));
        }
        if clip.repeat && clip.trigger != Trigger::Playout {
            return Err(Error("only actual playout may repeat a clip".into()));
        }
        if clip.trigger == Trigger::Concentrating
            && (clip.weight != 1.0
                || clip.delay_us != 0
                || !clip.hold
                || clip
                    .keyframes
                    .iter()
                    .any(|f| f.offsets != BodyOffsets::ZERO || f.brightness != 0.0))
        {
            return Err(Error(
                "concentration must hold still immediately with no movement or pulse".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn sample(&self, elapsed_us: u64) -> Option<(BodyOffsets, f32)> {
        let duration = self.duration_us();
        let elapsed = if self.repeat {
            elapsed_us % duration
        } else if elapsed_us > duration {
            if !self.hold {
                return None;
            }
            duration
        } else {
            elapsed_us
        };
        let after = self
            .keyframes
            .partition_point(|frame| frame.at_us < elapsed);
        if after == 0 {
            return Some((self.keyframes[0].offsets, self.keyframes[0].brightness));
        }
        if after >= self.keyframes.len() {
            let end = self.keyframes.last()?;
            return Some((end.offsets, end.brightness));
        }
        let a = self.keyframes[after - 1];
        let b = self.keyframes[after];
        let share = (elapsed - a.at_us) as f64 / (b.at_us - a.at_us) as f64;
        let share = ease(share);
        let offsets = blend(a.offsets, b.offsets, share);
        let brightness = a.brightness + (b.brightness - a.brightness) * share as f32;
        Some((offsets, brightness))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Postures {
    pub resting: BodyOffsets,
    pub listening: BodyOffsets,
    pub thinking: BodyOffsets,
    pub speaking: BodyOffsets,
}

impl Postures {
    pub fn for_voice(self, voice: Voice) -> BodyOffsets {
        match voice {
            Voice::Resting => self.resting,
            Voice::Listening => self.listening,
            Voice::Thinking => self.thinking,
            Voice::Speaking => self.speaking,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceLevels {
    pub resting: f32,
    pub listening: f32,
    pub thinking: f32,
    pub speaking: f32,
    pub unavailable: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timing {
    pub voice_stale_us: u64,
    pub joints_stale_us: u64,
    pub activity_stale_us: u64,
    pub faces_stale_us: u64,
    pub context_stale_us: u64,
    pub max_future_audio_us: u64,
    /// Optional linear fade from the immutable end of the latest heard piece.
    /// Zero keeps strict current-interval sampling. This is not a body lag.
    #[serde(default)]
    pub recent_playout_us: u64,
    /// Duration of return from thinking; short notice shrinks its remainder.
    pub return_us: u64,
    /// Command should already be facing back this long before first audio.
    pub return_before_us: u64,
    /// Measured owner lag, if available. None must remain an explicit unknown.
    pub body_lag_us: Option<u64>,
    /// Blend completion, not a delay before the body starts moving.
    pub interrupt_us: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GazeRules {
    pub minimum_confidence: f32,
    /// A room judged busy inhibits fresh unsolicited gaze/nods/sway.
    pub quiet_only: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Catalogue {
    pub version: u32,
    /// Select the complete generation-fenced VoiceProgress/VoiceSound contract.
    /// False keeps the original VoiceTiming/VoicePlayout contract unchanged.
    #[serde(default)]
    pub resumable_voice: bool,
    pub limits: BodyOffsets,
    pub postures: Postures,
    pub face_levels: FaceLevels,
    pub timing: Timing,
    pub gaze: GazeRules,
    pub behaviours: Vec<Behaviour>,
    /// The idle layer, sleep and waking (docs/alive.md). Without it the
    /// planner plans exactly as it did before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alive: Option<Alive>,
    /// The lamp's own speech in its head (#933): a nod on a stressed word,
    /// a sway between sentences, a settle as it finishes. Without it the
    /// planner plans exactly as it did before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaking: Option<crate::speaking::Speaking>,
}

/// The paths in `written` that `read` (the catalogue as this build read
/// it, written again) lacks: fields this build does not read. A field it
/// reads but writes only when set (an empty list, `false`, none) is not
/// one, so a missing key whose value is that is passed over.
fn unread_paths(
    written: &serde_json::Value,
    read: &serde_json::Value,
    at: &str,
    unread: &mut Vec<String>,
) {
    use serde_json::Value;
    match (written, read) {
        (Value::Object(written), Value::Object(read)) => {
            for (key, value) in written {
                let path = if at.is_empty() {
                    key.clone()
                } else {
                    format!("{at}.{key}")
                };
                match read.get(key) {
                    Some(known) => unread_paths(value, known, &path, unread),
                    None => {
                        let unset = matches!(value, Value::Null | Value::Bool(false))
                            || value.as_array().is_some_and(Vec::is_empty);
                        if !unset {
                            unread.push(path);
                        }
                    }
                }
            }
        }
        (Value::Array(written), Value::Array(read)) => {
            for (index, (value, known)) in written.iter().zip(read).enumerate() {
                unread_paths(value, known, &format!("{at}[{index}]"), unread);
            }
        }
        _ => {}
    }
}

impl Catalogue {
    pub fn builtin() -> Self {
        Self::from_json(include_str!("../behaviours.json"))
            .expect("the built-in expression catalogue is validated by tests")
    }

    pub fn from_json(text: &str) -> Result<Self, Error> {
        Self::from_json_naming(text).map(|(catalogue, _)| catalogue)
    }

    /// As [`Self::from_json`], with the fields this build does not read,
    /// by path (`speaking.sway_yaw`, `behaviours[2].weight`). A catalogue
    /// a newer build wrote is read past them, so a roll back never stops
    /// Expression (#1505, #1680); a known field with a bad value, an
    /// unknown value of a known field (a new trigger) or another
    /// `version` is still refused.
    pub fn from_json_naming(text: &str) -> Result<(Self, Vec<String>), Error> {
        if text.len() > MAX_CATALOGUE_BYTES {
            return Err(Error("catalogue exceeds 128 KiB".into()));
        }
        let mut catalogue: Self = serde_json::from_str(text)
            .map_err(|error| Error(format!("catalogue JSON: {error}")))?;
        let written: serde_json::Value = serde_json::from_str(text)
            .map_err(|error| Error(format!("catalogue JSON: {error}")))?;
        let read = serde_json::to_value(&catalogue)
            .map_err(|error| Error(format!("catalogue JSON: {error}")))?;
        let mut unread = Vec::new();
        unread_paths(&written, &read, "", &mut unread);
        // The startle is its skill's file's alone (#2822): a sudden clip a
        // catalogue from before it still carries is named and never played.
        // So is the glance at a remark, the startle's move (#4175).
        for (index, clip) in catalogue.behaviours.iter().enumerate() {
            match clip.trigger {
                Trigger::Sudden => {
                    unread.push(format!("behaviours[{index}] (the startle is its skill's)"))
                }
                Trigger::Remarked => unread.push(format!(
                    "behaviours[{index}] (the glance at a remark is the startle skill's)"
                )),
                _ => {}
            }
        }
        catalogue
            .behaviours
            .retain(|clip| !matches!(clip.trigger, Trigger::Sudden | Trigger::Remarked));
        catalogue.check()?;
        Ok((catalogue, unread))
    }

    pub fn check(&self) -> Result<(), Error> {
        if self.version != 1 {
            return Err(Error("catalogue version must be 1".into()));
        }
        if let Some(speaking) = &self.speaking {
            speaking.check(self.limits)?;
        }
        if self
            .limits
            .values()
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=EXPRESSION_MAX_OFFSET_RAD).contains(v))
        {
            return Err(Error(
                "limits must be finite, nonnegative and at most 0.35 radians".into(),
            ));
        }
        for posture in [
            self.postures.resting,
            self.postures.listening,
            self.postures.thinking,
            self.postures.speaking,
        ] {
            posture.check()?;
            if posture
                .values()
                .into_iter()
                .zip(self.limits.values())
                .any(|(offset, limit)| offset.abs() > limit)
            {
                return Err(Error("a posture exceeds configured limits".into()));
            }
        }
        let levels = self.face_levels;
        if [
            levels.resting,
            levels.listening,
            levels.thinking,
            levels.speaking,
            levels.unavailable,
        ]
        .iter()
        .any(|value| !(0.0..=1.0).contains(value))
        {
            return Err(Error("face levels must be finite within 0..1".into()));
        }
        let t = self.timing;
        for (name, value, low, high) in [
            ("voice_stale_us", t.voice_stale_us, 100_000, 500_000),
            ("joints_stale_us", t.joints_stale_us, 80_000, 500_000),
            ("activity_stale_us", t.activity_stale_us, 100_000, 300_000),
            ("faces_stale_us", t.faces_stale_us, 20_000, 1_000_000),
            ("context_stale_us", t.context_stale_us, 100_000, 30_000_000),
            (
                "max_future_audio_us",
                t.max_future_audio_us,
                20_000,
                EXPRESSION_MAX_FUTURE_US,
            ),
            ("return_us", t.return_us, 20_000, 1_000_000),
            ("return_before_us", t.return_before_us, 20_000, 500_000),
            ("interrupt_us", t.interrupt_us, 100_000, 600_000),
        ] {
            if !(low..=high).contains(&value) {
                return Err(Error(format!("{name} must be {low}..{high} microseconds")));
            }
        }
        if t.body_lag_us.is_some_and(|lag| lag > 500_000) {
            return Err(Error(
                "body_lag_us must be measured and at most 500000".into(),
            ));
        }
        if t.recent_playout_us != 0 && !(20_000..=200_000).contains(&t.recent_playout_us) {
            return Err(Error(
                "recent_playout_us must be 0 or 20000..200000 microseconds".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.gaze.minimum_confidence) {
            return Err(Error("gaze confidence must be finite in 0..1".into()));
        }
        if self.behaviours.is_empty() || self.behaviours.len() > MAX_BEHAVIOURS {
            return Err(Error(format!(
                "catalogue needs 1..{MAX_BEHAVIOURS} behaviours"
            )));
        }
        let mut names = BTreeSet::new();
        let mut triggers = BTreeSet::new();
        let mut rest_clips = 0;
        for clip in &self.behaviours {
            if clip.name.is_empty()
                || clip.name.len() > 64
                || !clip
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b' ' || b == b'-')
                || !names.insert(&clip.name)
            {
                return Err(Error(
                    "behaviour names must be unique, short, plain lower-case names".into(),
                ));
            }
            triggers.insert(clip.trigger);
            clip.check()?;
            if clip.trigger == Trigger::Rest {
                rest_clips += 1;
                if rest_clips > 1 {
                    return Err(Error("a catalogue has at most one rest clip".into()));
                }
            }
        }
        if let Some(alive) = &self.alive {
            alive.check(self.limits)?;
        }
        for trigger in [
            Trigger::Thinking,
            Trigger::Pause,
            Trigger::Playout,
            Trigger::Interrupted,
            Trigger::Concentrating,
        ] {
            if !triggers.contains(&trigger) {
                return Err(Error(format!("catalogue has no {trigger:?} behaviour")));
            }
        }
        Ok(())
    }
}

pub(crate) fn ease(share: f64) -> f64 {
    let share = share.clamp(0.0, 1.0);
    share * share * (3.0 - 2.0 * share)
}

pub(crate) fn blend(a: BodyOffsets, b: BodyOffsets, share: f64) -> BodyOffsets {
    let a = a.values();
    let b = b.values();
    BodyOffsets::from_values(std::array::from_fn(|i| a[i] + (b[i] - a[i]) * share))
}

pub(crate) fn add(a: BodyOffsets, b: BodyOffsets, weight: f64) -> BodyOffsets {
    let a = a.values();
    let b = b.values();
    BodyOffsets::from_values(std::array::from_fn(|i| a[i] + b[i] * weight))
}

pub(crate) fn clamp(offsets: BodyOffsets, limits: BodyOffsets) -> BodyOffsets {
    let offsets = offsets.values();
    let limits = limits.values();
    BodyOffsets::from_values(std::array::from_fn(|i| {
        offsets[i].clamp(-limits[i], limits[i])
    }))
}
