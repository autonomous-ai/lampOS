//! Additive contracts for the optional expression planner. Legacy actuator
//! settings and voice topics keep their original meanings and types.
use crate::{Invalid, Light, Voice};
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const EXPRESSION_LEASE_US: u64 = 200_000;
pub const EXPRESSION_MAX_OFFSET_RAD: f64 = 0.35;
pub const EXPRESSION_MAX_FUTURE_US: u64 = 2_000_000;
pub const EXPRESSION_REST_LEASE_US: u64 = 3_000_000;
pub const EXPRESSION_REST_FRESH_US: u64 = 500_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpressionRestOp {
    Rest,
    Wake,
    Cancel,
}

/// A skill's bounded intent, never an actuator command or microphone permit.
/// Episode numbers increase within one request publisher. Renewals keep the
/// episode and operation, with a fresh source observation and sequence.
/// Additive fields are ignored; an unknown operation is still refused.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpressionRestRequest {
    pub stamp: ClockStamp,
    pub skill: String,
    pub skill_session_id: String,
    /// The current expression/state publisher's boot, not the host boot.
    pub expression_session_id: String,
    pub episode: u64,
    pub op: ExpressionRestOp,
    pub valid_until_us: u64,
}

impl ExpressionRestRequest {
    pub const TYPE: &str = "lamp_msgs/ExpressionRestRequest";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "expression/rest_request";
    /// One Skills publisher carries every generic skill's independent episode.
    pub const SOURCE: &str = "skills";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
            .every(Duration::from_secs(1))
            .lasting(Duration::from_micros(EXPRESSION_REST_FRESH_US))
    }

    pub fn check(&self) -> Result<(), Invalid> {
        self.stamp.check()?;
        identifier(&self.skill_session_id, "skill_session_id")?;
        identifier(&self.expression_session_id, "expression_session_id")?;
        if self.skill.is_empty()
            || self.skill.len() > 64
            || !self
                .skill
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
            || self.episode == 0
            || self.stamp.sequence == 0
            || self.valid_until_us <= self.stamp.at_us
            || self.valid_until_us - self.stamp.at_us > EXPRESSION_REST_LEASE_US
        {
            return Err(Invalid("rest needs a short skill name, positive episode/sequence and a lease of at most three seconds".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpressionRestPhase {
    Idle,
    Resting,
    Waking,
    Completed,
    Cancelled,
    Refused,
}

/// Current planning receipt. It does not prove the body moved, stood up or
/// unfolded; the body's own pause/fold/range guards remain authoritative.
/// Additive fields are ignored, including in the echoed request and stamp.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpressionRestState {
    pub stamp: ClockStamp,
    pub expression_session_id: String,
    pub enabled: bool,
    pub request: Option<ExpressionRestRequest>,
    pub phase: ExpressionRestPhase,
}

impl ExpressionRestState {
    pub const TYPE: &str = "lamp_msgs/ExpressionRestState";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "expression/rest_state";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_millis(200))
            .lasting(Duration::from_millis(500))
    }

    pub fn check(&self) -> Result<(), Invalid> {
        self.stamp.check()?;
        identifier(&self.expression_session_id, "expression_session_id")?;
        if (self.phase == ExpressionRestPhase::Idle) != self.request.is_none()
            || (!self.enabled
                && matches!(
                    self.phase,
                    ExpressionRestPhase::Resting | ExpressionRestPhase::Waking
                ))
        {
            return Err(Invalid(
                "rest receipt identity and enabled phase disagree".into(),
            ));
        }
        if let Some(request) = &self.request {
            request.check()?;
            if request.expression_session_id != self.expression_session_id
                || request.stamp.clock_id != self.stamp.clock_id
            {
                return Err(Invalid(
                    "rest receipt belongs to another clock or Expression session".into(),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod rest_contract_tests {
    use super::*;

    fn request() -> ExpressionRestRequest {
        ExpressionRestRequest {
            stamp: ClockStamp {
                clock_id: "clock".into(),
                at_us: 10,
                sequence: 1,
            },
            skill: "sample-routine".into(),
            skill_session_id: "publisher".into(),
            expression_session_id: "expression".into(),
            episode: 1,
            op: ExpressionRestOp::Rest,
            valid_until_us: 10 + EXPRESSION_REST_LEASE_US,
        }
    }

    #[test]
    fn rest_request_reads_additive_fields_but_refuses_unknown_operations() {
        let request = request();
        request.check().unwrap();
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(
            serde_json::from_value::<ExpressionRestRequest>(value.clone()).unwrap(),
            request
        );
        let mut changed = value;
        changed["added_later"] = serde_json::json!({"detail": true});
        changed["stamp"]["added_later"] = serde_json::json!([1, 2]);
        let read: ExpressionRestRequest = serde_json::from_value(changed.clone()).unwrap();
        assert_eq!(read, request);
        read.check().unwrap();
        changed["op"] = serde_json::json!("unfold");
        assert!(serde_json::from_value::<ExpressionRestRequest>(changed).is_err());
    }

    #[test]
    fn rest_receipt_reads_nested_additions_without_guessing_operations_or_phases() {
        let request = request();
        let state = ExpressionRestState {
            stamp: request.stamp.clone(),
            expression_session_id: request.expression_session_id.clone(),
            enabled: true,
            request: Some(request),
            phase: ExpressionRestPhase::Resting,
        };
        let mut value = serde_json::to_value(&state).unwrap();
        value["added_later"] = serde_json::json!({"detail": true});
        value["stamp"]["added_later"] = serde_json::json!(1);
        value["request"]["added_later"] = serde_json::json!([1, 2]);
        value["request"]["stamp"]["added_later"] = serde_json::json!(true);
        let read: ExpressionRestState = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(read, state);
        read.check().unwrap();

        let mut changed = value.clone();
        changed["request"]["op"] = serde_json::json!("unfold");
        assert!(serde_json::from_value::<ExpressionRestState>(changed).is_err());
        let mut changed = value.clone();
        changed["phase"] = serde_json::json!("applied");
        assert!(serde_json::from_value::<ExpressionRestState>(changed).is_err());
        let mut changed = value;
        changed["request"]["stamp"]
            .as_object_mut()
            .unwrap()
            .remove("clock_id");
        assert!(serde_json::from_value::<ExpressionRestState>(changed).is_err());
    }

    #[test]
    fn rest_lease_and_identity_are_bounded() {
        for kind in 0..7 {
            let mut request = request();
            match kind {
                0 => request.valid_until_us += 1,
                1 => request.valid_until_us = request.stamp.at_us,
                2 => request.episode = 0,
                3 => request.stamp.sequence = 0,
                4 => request.skill = "../other".into(),
                5 => request.skill = "a".repeat(65),
                _ => request.skill_session_id.clear(),
            }
            assert!(request.check().is_err(), "case {kind}");
        }
        let declaration = ExpressionRestRequest::declaration();
        assert_eq!(declaration.kind, Kind::Event);
        assert_eq!(declaration.lifespan_ms, Some(500));
    }

    #[test]
    fn rest_receipt_binds_identity_and_cannot_claim_active_shadow() {
        let request = request();
        let mut state = ExpressionRestState {
            stamp: request.stamp.clone(),
            expression_session_id: request.expression_session_id.clone(),
            enabled: true,
            request: Some(request),
            phase: ExpressionRestPhase::Resting,
        };
        state.check().unwrap();
        state.enabled = false;
        assert!(state.check().is_err());
        state.phase = ExpressionRestPhase::Refused;
        state.check().unwrap();
        state.expression_session_id = "different".into();
        assert!(state.check().is_err());
        let mut value = serde_json::to_value(&state).unwrap();
        value["phase"] = serde_json::json!("applied");
        assert!(serde_json::from_value::<ExpressionRestState>(value).is_err());
    }
}

/// Microseconds on this machine's CLOCK_MONOTONIC, never a publisher's
/// private process clock or Unix time. `clock_id` identifies the machine boot.
/// Sequence is monotonic within the publisher session and topic.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockStamp {
    pub clock_id: String,
    pub at_us: u64,
    pub sequence: u64,
}

impl ClockStamp {
    pub fn check(&self) -> Result<(), Invalid> {
        identifier(&self.clock_id, "clock_id")
    }
}

fn identifier(value: &str, field: &str) -> Result<(), Invalid> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(Invalid(format!(
            "{field} must have 1..128 bytes without controls"
        )));
    }
    Ok(())
}

/// A state heartbeat, an actual playback schedule, or a terminal edge.
/// Interrupted retires this turn's audio permanently, while reporting whether
/// the capture path is already able to retain a new person's input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceEvent {
    State,
    Scheduled,
    Interrupted,
    Cancelled,
    Finished,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VoiceTiming {
    pub stamp: ClockStamp,
    /// A fresh unique identifier every time the voice process starts. A turn
    /// number alone cannot fence queued audio from a previous process.
    pub session_id: String,
    /// Increasing within the session; request identity cannot change in a turn.
    pub turn: u64,
    pub request: u64,
    pub state: Voice,
    /// True only after the input owner can retain speech, not after a wake word.
    pub accepting_input: bool,
    pub event: VoiceEvent,
    /// The committed first sample's playout time, required only for Scheduled.
    /// A corrected schedule is another Scheduled event with a newer sequence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_audio_us: Option<u64>,
}

impl VoiceTiming {
    pub const TYPE: &str = "lamp_msgs/VoiceTiming";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "voice/timing";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_millis(100))
            .lasting(Duration::from_millis(500))
    }

    pub fn check(&self) -> Result<(), Invalid> {
        self.stamp.check()?;
        identifier(&self.session_id, "session_id")?;
        if self.event == VoiceEvent::Scheduled {
            if self.state != Voice::Thinking
                || self.first_audio_us.is_none_or(|at| {
                    at < self.stamp.at_us || at - self.stamp.at_us > EXPRESSION_MAX_FUTURE_US
                })
            {
                return Err(Invalid(
                    "Scheduled needs Thinking and a first sample within 2 seconds".into(),
                ));
            }
        } else if self.first_audio_us.is_some() {
            return Err(Invalid(
                "first_audio_us is only carried by Scheduled".into(),
            ));
        }
        if self.event == VoiceEvent::Interrupted && self.state != Voice::Listening {
            return Err(Invalid("Interrupted must report Listening".into()));
        }
        if matches!(self.event, VoiceEvent::Cancelled | VoiceEvent::Finished)
            && !matches!(self.state, Voice::Resting | Voice::Listening)
        {
            return Err(Invalid(
                "terminal voice events cannot report Thinking or Speaking".into(),
            ));
        }
        Ok(())
    }
}

/// Actual human speech activity from the voice owner's detector, after its
/// own echo/attention policy. False is a pause edge or a pause heartbeat; the
/// planner requires a previous true for the same turn before it can nod.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonActivity {
    pub stamp: ClockStamp,
    pub session_id: String,
    pub turn: u64,
    pub request: u64,
    pub speaking: bool,
}

impl PersonActivity {
    pub const TYPE: &str = "lamp_msgs/PersonActivity";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "voice/activity";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_millis(100))
            .lasting(Duration::from_millis(300))
    }

    pub fn check(&self) -> Result<(), Invalid> {
        self.stamp.check()?;
        identifier(&self.session_id, "session_id")
    }
}

/// Level of an interval committed to the physical playout clock. This is
/// neither a received cloud chunk nor time spent waiting in a software queue.
/// Half-open intervals [start_us, end_us) are non-overlapping and published
/// in playout order; cancellation is on voice/timing, never another level.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VoicePlayout {
    pub stamp: ClockStamp,
    pub session_id: String,
    pub turn: u64,
    pub request: u64,
    pub start_us: u64,
    pub end_us: u64,
    /// RMS/envelope mapped by the voice producer to 0..1, consistently across
    /// its stream. The mapping and DAC timing still require lamp measurement.
    pub level: f32,
}

impl VoicePlayout {
    pub const TYPE: &str = "lamp_msgs/VoicePlayout";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "voice/playout";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
            .every(Duration::from_millis(20))
            .lasting(Duration::from_millis(200))
    }

    pub fn check(&self) -> Result<(), Invalid> {
        self.stamp.check()?;
        identifier(&self.session_id, "session_id")?;
        if !(0.0..=1.0).contains(&self.level) {
            return Err(Invalid("playout level must be finite and in 0..1".into()));
        }
        if self.start_us >= self.end_us || self.end_us - self.start_us > EXPRESSION_LEASE_US {
            return Err(Invalid(
                "playout interval must last 1..200000 microseconds".into(),
            ));
        }
        if self.start_us > self.stamp.at_us.saturating_add(EXPRESSION_MAX_FUTURE_US) {
            return Err(Invalid(
                "playout cannot be queued over 2 seconds ahead".into(),
            ));
        }
        Ok(())
    }
}

/// Opt-in replacement for `voice/timing` for a planner that understands a
/// temporarily held turn. The complete heartbeat is atomic on one topic;
/// an untagged legacy state must never refresh or override this contract.
///
/// The owner discards queued sound and unpublished playout observations before
/// advancing `audio_generation` on each hold. Resume keeps that generation.
/// Heartbeats retain both the generation and held state. Generation is positive
/// and never decreases within a session/turn; terminal events retire the whole
/// turn, even if a later message claims a higher generation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VoiceProgress {
    #[serde(flatten)]
    pub timing: VoiceTiming,
    pub audio_generation: u64,
    pub held: bool,
}

impl VoiceProgress {
    pub const TYPE: &str = "lamp_msgs/VoiceProgress";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "voice/progress";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_millis(100))
            .lasting(Duration::from_millis(500))
    }

    pub fn check(&self) -> Result<(), Invalid> {
        self.timing.check()?;
        if self.audio_generation == 0 {
            return Err(Invalid("audio generation must be positive".into()));
        }
        if self.held
            && (self.timing.state != Voice::Listening || self.timing.event != VoiceEvent::State)
        {
            return Err(Invalid(
                "a held turn needs Listening and a nonterminal State heartbeat".into(),
            ));
        }
        Ok(())
    }
}

/// An actual committed playout interval belonging to exactly one generation
/// of a resumable Voice turn. The planner accepts it only after fresh matching
/// `VoiceProgress`, while not held, and never mixes it with `VoicePlayout`.
/// A generation labels newly committed sound; it must not relabel sound that
/// the owner discarded on a hold. This is evidence, not a speaker command.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VoiceSound {
    #[serde(flatten)]
    pub playout: VoicePlayout,
    pub audio_generation: u64,
}

impl VoiceSound {
    pub const TYPE: &str = "lamp_msgs/VoiceSound";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "voice/sound";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
            .every(Duration::from_millis(20))
            .lasting(Duration::from_millis(200))
    }

    pub fn check(&self) -> Result<(), Invalid> {
        self.playout.check()?;
        if self.audio_generation == 0 {
            return Err(Invalid("audio generation must be positive".into()));
        }
        Ok(())
    }
}

/// An explicit setting from a trusted attention/user input owner. Quiet room,
/// silence, a transcript and a wake word must never be substituted for it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Concentration {
    pub stamp: ClockStamp,
    pub concentrating: bool,
}

impl Concentration {
    pub const TYPE: &str = "lamp_msgs/Concentration";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "attention/concentration";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        self.stamp.check()
    }
}

/// Offsets in radians, not servo counts or absolute readback positions.
/// The body adds these to its calibrated safe posture / attended heading,
/// arbitrates deliberate Actions first, and enforces its own motion limits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BodyOffsets {
    pub base_yaw: f64,
    pub shoulder: f64,
    pub elbow: f64,
    pub head_roll: f64,
    pub head_pitch: f64,
}

impl BodyOffsets {
    pub const ZERO: Self = Self {
        base_yaw: 0.0,
        shoulder: 0.0,
        elbow: 0.0,
        head_roll: 0.0,
        head_pitch: 0.0,
    };

    pub fn values(self) -> [f64; 5] {
        [
            self.base_yaw,
            self.shoulder,
            self.elbow,
            self.head_roll,
            self.head_pitch,
        ]
    }

    pub fn from_values(v: [f64; 5]) -> Self {
        Self {
            base_yaw: v[0],
            shoulder: v[1],
            elbow: v[2],
            head_roll: v[3],
            head_pitch: v[4],
        }
    }

    pub fn check(&self) -> Result<(), Invalid> {
        if self
            .values()
            .iter()
            .any(|v| !v.is_finite() || v.abs() > EXPRESSION_MAX_OFFSET_RAD)
        {
            return Err(Invalid(
                "body offsets must be finite radians within +/-0.35".into(),
            ));
        }
        Ok(())
    }
}

/// Receivers compare all times to their own CLOCK_MONOTONIC for this boot,
/// reject older sequence/session/turns and expired leases at the hardware
/// boundary, and stop expression when renewal ceases. `apply_us` is the
/// command application time, not a claim that the mechanism has arrived.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpressionLease {
    pub clock_id: String,
    /// The expression process session, unique on every planner start.
    /// Supplied by Engine::with_session_id or its embedding process.
    pub session_id: String,
    /// Which voice instance owns this turn; distinct from planner restarts.
    pub voice_session_id: String,
    pub sequence: u64,
    pub turn: u64,
    pub request: u64,
    pub issued_us: u64,
    pub apply_us: u64,
    pub valid_until_us: u64,
}

impl ExpressionLease {
    pub fn check(&self) -> Result<(), Invalid> {
        identifier(&self.clock_id, "clock_id")?;
        identifier(&self.session_id, "session_id")?;
        identifier(&self.voice_session_id, "voice_session_id")?;
        if self.sequence == 0
            || self.apply_us < self.issued_us
            || self.valid_until_us <= self.apply_us
            || self.valid_until_us - self.issued_us > EXPRESSION_LEASE_US
        {
            return Err(Invalid(
                "lease needs an increasing sequence and ordered times within 200 ms".into(),
            ));
        }
        Ok(())
    }

    /// A stateless part of the receiver's final gate. The receiver must also
    /// compare session/turn/sequence to previously accepted commands.
    pub fn valid_at(&self, clock_id: &str, now_us: u64) -> bool {
        self.check().is_ok()
            && self.clock_id == clock_id
            && self.apply_us <= now_us
            && now_us < self.valid_until_us
    }
}

/// A normalized target, not a fabricated camera-to-joint calibration. The
/// body keeps its own stable-sight, camera geometry and velocity policy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExpressionGaze {
    pub face_id: String,
    pub x: f32,
    pub y: f32,
    pub observed_us: u64,
    pub valid_until_us: u64,
}

impl ExpressionGaze {
    pub fn check(&self) -> Result<(), Invalid> {
        identifier(&self.face_id, "face_id")?;
        if !(-1.0..=1.0).contains(&self.x)
            || !(-1.0..=1.0).contains(&self.y)
            || self.valid_until_us <= self.observed_us
            || self.valid_until_us - self.observed_us > EXPRESSION_MAX_FUTURE_US
        {
            return Err(Invalid(
                "gaze needs normalized coordinates and a bounded fresh interval".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExpressionBody {
    pub lease: ExpressionLease,
    pub offsets: BodyOffsets,
    /// Capture/hold the body's actual safe pose once on entering this mode.
    /// Ignore offsets and gaze while held; do not return to a guessed home.
    pub hold: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gaze: Option<ExpressionGaze>,
}

impl ExpressionBody {
    pub const TYPE: &str = "lamp_msgs/ExpressionBody";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "expression/body";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_millis(20))
            .lasting(Duration::from_millis(200))
    }

    pub fn check(&self) -> Result<(), Invalid> {
        self.lease.check()?;
        self.offsets.check()?;
        if let Some(gaze) = &self.gaze {
            gaze.check()?;
            if self.hold
                || gaze.observed_us > self.lease.issued_us
                || gaze.valid_until_us <= self.lease.apply_us
                || gaze.valid_until_us > self.lease.valid_until_us
            {
                return Err(Invalid(
                    "a held body has no gaze; gaze must fit the lease".into(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpressionFace {
    Resting,
    Listening,
    Thinking,
    Speaking,
    /// Input cannot yet be retained; must not be drawn as listening readiness.
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExpressionLight {
    pub lease: ExpressionLease,
    /// User's lasting setting, using the existing light/command type.
    pub setting: Light,
    pub face: ExpressionFace,
    /// Multiplier of that setting, 0..1, not a replacement brightness level.
    pub brightness: f32,
}

impl ExpressionLight {
    pub const TYPE: &str = "lamp_msgs/ExpressionLight";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "expression/light";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_millis(20))
            .lasting(Duration::from_millis(200))
    }

    pub fn check(&self) -> Result<(), Invalid> {
        self.lease.check()?;
        self.setting.check()?;
        if !(0.0..=1.0).contains(&self.brightness) || (!self.setting.on && self.brightness != 0.0) {
            return Err(Invalid(
                "light brightness must be finite in 0..1, and zero when off".into(),
            ));
        }
        Ok(())
    }
}

/// What the planner intends, never a hardware completion report. `body` and
/// `light` say whether this step emits a leased command, so shadow is explicit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpressionState {
    pub stamp: ClockStamp,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<u64>,
    pub behaviours: Vec<String>,
    pub missing: Vec<String>,
    pub warnings: Vec<String>,
    pub body: bool,
    pub light: bool,
}

impl ExpressionState {
    pub const TYPE: &str = "lamp_msgs/ExpressionState";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "expression/state";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_millis(200))
            .lasting(Duration::from_millis(500))
    }

    pub fn check(&self) -> Result<(), Invalid> {
        self.stamp.check()?;
        if (!self.enabled && (self.body || self.light))
            || self.turn.is_some() != self.request.is_some()
        {
            return Err(Invalid(
                "shadow has no commands and turn/request are paired".into(),
            ));
        }
        for (name, values) in [
            ("behaviours", &self.behaviours),
            ("missing", &self.missing),
            ("warnings", &self.warnings),
        ] {
            if values.len() > 32
                || values
                    .iter()
                    .any(|v| v.is_empty() || v.len() > 128 || v.chars().any(char::is_control))
            {
                return Err(Invalid(format!(
                    "{name} is at most 32 short plain statements"
                )));
            }
        }
        Ok(())
    }
}

/// Optional topics kept separate from the original first-topic catalogue.
pub fn expression_topics() -> Vec<(&'static str, Declaration)> {
    let mut topics = vec![
        (VoiceTiming::TOPIC, VoiceTiming::declaration()),
        (PersonActivity::TOPIC, PersonActivity::declaration()),
        (VoicePlayout::TOPIC, VoicePlayout::declaration()),
        (VoiceProgress::TOPIC, VoiceProgress::declaration()),
        (VoiceSound::TOPIC, VoiceSound::declaration()),
        (Concentration::TOPIC, Concentration::declaration()),
        (ExpressionBody::TOPIC, ExpressionBody::declaration()),
        (ExpressionLight::TOPIC, ExpressionLight::declaration()),
        (ExpressionState::TOPIC, ExpressionState::declaration()),
        (
            ExpressionRestRequest::TOPIC,
            ExpressionRestRequest::declaration(),
        ),
        (
            ExpressionRestState::TOPIC,
            ExpressionRestState::declaration(),
        ),
    ];
    topics.sort_by_key(|(name, _)| *name);
    topics
}
