use crate::alive::{self, Life};
use crate::catalogue::{add, blend, clamp, ease};
use crate::{Catalogue, Error, LEASE_US, Trigger};
use lamp_msgs::{
    BodyOffsets, ClockStamp, Concentration, ControlGesture, ExpressionBody, ExpressionFace,
    ExpressionGaze, ExpressionLease, ExpressionLight, ExpressionState, Faces, JointState, Light,
    NameHeard, PersonActivity, Room, RoomState, ScheduleDue, Speech, SuddenSound, TouchEvent,
    TouchState, Voice, VoiceEvent, VoiceMute, VoicePlayout, VoiceProgress, VoiceRemarked,
    VoiceSound, VoiceState, VoiceTiming,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, VecDeque};

#[path = "rest.rs"]
mod rest;

pub const MAX_PLAYOUT_FRAMES: usize = 128;
/// How many retired Voice sessions are remembered, so that a stale heartbeat
/// from one cannot revive it. The oldest is forgotten when another retires.
/// A forgotten session's late message is still refused on two other grounds:
/// the voice stream's order mark refuses any message stamped before one
/// already taken ("input is duplicate or out of order"): a new session starts
/// the mark's count again but never its time. And one that arrives later than
/// `voice_stale_us` (500 ms) is stale. Only a voice process 33 sessions old
/// that is still running could come back, and `lamp-voice.service` (one unit,
/// `KillMode=mixed`) does not allow that.
pub const MAX_VOICE_SESSIONS: usize = 32;
const STREAMS: usize = 22;
const CONTEXT_BYTES: usize = 16 * 1024;

/// Arrival time is supplied separately to ingest. Stamps retain the source
/// observation time, so replay and a busy receiver cannot make old data fresh.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Input {
    Voice(VoiceTiming),
    Progress(VoiceProgress),
    Activity(PersonActivity),
    Playout(VoicePlayout),
    Sound(VoiceSound),
    Concentration(Concentration),
    VoiceState {
        stamp: ClockStamp,
        value: VoiceState,
    },
    Joints {
        stamp: ClockStamp,
        value: JointState,
    },
    Faces {
        stamp: ClockStamp,
        value: Faces,
    },
    Room {
        stamp: ClockStamp,
        value: RoomState,
    },
    Light {
        stamp: ClockStamp,
        value: Light,
    },
    Heard {
        stamp: ClockStamp,
        value: Speech,
    },
    Said {
        stamp: ClockStamp,
        value: Speech,
    },
    Name {
        stamp: ClockStamp,
        value: NameHeard,
    },
    /// A sound that rose suddenly out of the room's quiet (`lamp-ears`).
    Sudden {
        stamp: ClockStamp,
        value: SuddenSound,
    },
    /// A hand on the lamp (`controls/gesture`): a tap or a stroke wakes it.
    Touch {
        stamp: ClockStamp,
        value: ControlGesture,
    },
    /// The schedule's oldest due notice (`schedule/due`): a reminder wakes it.
    Due {
        stamp: ClockStamp,
        value: ScheduleDue,
    },
    /// The local time of day, in minutes since local midnight, at `stamp`.
    /// Not a bus topic: the live service makes it from the wall clock and
    /// its local time (`--zone`, or `--utc-offset-minutes`; #820), and an
    /// import from the recording's.
    DayTime {
        stamp: ClockStamp,
        minute: u16,
    },
    /// The head's requests from `--touch` Controls (`controls/touch`, #711):
    /// a hand kept there leans the lamp into it, a hold puts it to sleep, a
    /// tap wakes it. Only with `serve --head-touch`.
    HeadTouch {
        stamp: ClockStamp,
        value: TouchEvent,
    },
    /// The head's state (`controls/touch_state`): whether that hand is still
    /// there.
    HeadHand {
        stamp: ClockStamp,
        value: TouchState,
    },
    /// The voice's mute and why (`voice/mute`, #711): while it says the lamp
    /// was put to sleep, it sleeps, and it wakes when the voice opens again.
    /// Only with `serve --head-touch`.
    VoiceMute {
        stamp: ClockStamp,
        value: VoiceMute,
    },
    /// Someone in a busy room said something about the lamp and the voice
    /// said nothing (`voice/remarked`, #4175): one glance, with
    /// `LAMP_REMARK_GLANCE=on`.
    Remarked {
        stamp: ClockStamp,
        value: VoiceRemarked,
    },
}

/// The name `Input::DayTime` goes by where a topic is asked for.
pub const DAYTIME_TOPIC: &str = "expression/daytime";

impl Input {
    pub fn stamp(&self) -> &ClockStamp {
        match self {
            Self::Voice(v) => &v.stamp,
            Self::Progress(v) => &v.timing.stamp,
            Self::Activity(v) => &v.stamp,
            Self::Playout(v) => &v.stamp,
            Self::Sound(v) => &v.playout.stamp,
            Self::Concentration(v) => &v.stamp,
            Self::VoiceState { stamp, .. }
            | Self::Joints { stamp, .. }
            | Self::Faces { stamp, .. }
            | Self::Room { stamp, .. }
            | Self::Light { stamp, .. }
            | Self::Heard { stamp, .. }
            | Self::Said { stamp, .. }
            | Self::Name { stamp, .. }
            | Self::Sudden { stamp, .. }
            | Self::Touch { stamp, .. }
            | Self::Due { stamp, .. }
            | Self::DayTime { stamp, .. }
            | Self::HeadTouch { stamp, .. }
            | Self::HeadHand { stamp, .. }
            | Self::VoiceMute { stamp, .. }
            | Self::Remarked { stamp, .. } => stamp,
        }
    }

    pub fn topic(&self) -> &'static str {
        match self {
            Self::Voice(_) => VoiceTiming::TOPIC,
            Self::Progress(_) => VoiceProgress::TOPIC,
            Self::Activity(_) => PersonActivity::TOPIC,
            Self::Playout(_) => VoicePlayout::TOPIC,
            Self::Sound(_) => VoiceSound::TOPIC,
            Self::Concentration(_) => Concentration::TOPIC,
            Self::VoiceState { .. } => VoiceState::TOPIC,
            Self::Joints { .. } => JointState::TOPIC,
            Self::Faces { .. } => Faces::TOPIC,
            Self::Room { .. } => RoomState::TOPIC,
            Self::Light { .. } => Light::TOPICS[0],
            Self::Heard { .. } => Speech::TOPICS[0],
            Self::Said { .. } => Speech::TOPICS[1],
            Self::Name { .. } => NameHeard::TOPIC,
            Self::Sudden { .. } => SuddenSound::TOPIC,
            Self::Touch { .. } => ControlGesture::TOPIC,
            Self::Due { .. } => ScheduleDue::TOPIC,
            Self::DayTime { .. } => DAYTIME_TOPIC,
            Self::HeadTouch { .. } => TouchEvent::TOPIC,
            Self::HeadHand { .. } => TouchState::TOPIC,
            Self::VoiceMute { .. } => VoiceMute::TOPIC,
            Self::Remarked { .. } => VoiceRemarked::TOPIC,
        }
    }

    fn stream(&self) -> usize {
        match self {
            Self::Voice(_) => 0,
            Self::Activity(_) => 1,
            Self::Playout(_) => 2,
            Self::Concentration(_) => 3,
            Self::VoiceState { .. } => 4,
            Self::Joints { .. } => 5,
            Self::Faces { .. } => 6,
            Self::Room { .. } => 7,
            Self::Light { .. } => 8,
            Self::Heard { .. } => 9,
            Self::Said { .. } => 10,
            Self::Name { .. } => 11,
            Self::Progress(_) => 12,
            Self::Sound(_) => 13,
            Self::Sudden { .. } => 14,
            Self::Touch { .. } => 15,
            Self::Due { .. } => 16,
            Self::DayTime { .. } => 17,
            Self::HeadTouch { .. } => 18,
            Self::HeadHand { .. } => 19,
            Self::VoiceMute { .. } => 20,
            Self::Remarked { .. } => 21,
        }
    }

    /// Structural and finite-value validation, independent of arrival order.
    pub fn check(&self) -> Result<(), Error> {
        self.stamp().check()?;
        match self {
            Self::Voice(v) => v.check()?,
            Self::Progress(v) => v.check()?,
            Self::Activity(v) => v.check()?,
            Self::Playout(v) => v.check()?,
            Self::Sound(v) => v.check()?,
            Self::Concentration(v) => v.check()?,
            Self::Joints { value, .. } => {
                value.check()?;
                let names: BTreeSet<_> = value.name.iter().collect();
                if value.name.len() > 16
                    || names.len() != value.name.len()
                    || value
                        .name
                        .iter()
                        .any(|name| name.is_empty() || name.len() > 128)
                    || value.position.len() != value.name.len()
                    || !["base_yaw", "shoulder", "elbow", "head_roll", "head_pitch"]
                        .iter()
                        .all(|name| value.name.iter().any(|n| n == name))
                    || value
                        .position
                        .iter()
                        .chain(&value.commanded)
                        .chain(&value.velocity)
                        .any(|v| !v.is_finite())
                    || value.effort.iter().any(|v| !(0.0..=1.0).contains(v))
                {
                    return Err(Error(
                        "joint state needs five named measured joints and finite SI values".into(),
                    ));
                }
            }
            Self::Faces { value, .. } => {
                if value.faces.len() > 64
                    || value.faces.iter().any(|face| {
                        face.id.is_empty()
                            || face.id.len() > 128
                            || face.id.chars().any(char::is_control)
                            || face.yaw.is_some_and(|yaw| !yaw.is_finite())
                    })
                {
                    return Err(Error(
                        "face list exceeds bounds or has invalid identity/yaw".into(),
                    ));
                }
                value.check()?;
            }
            Self::Light { value, .. } => value.check()?,
            Self::Name { value, .. } => value.check()?,
            Self::Sudden { value, .. } => value.check()?,
            Self::Touch { value, .. } => value.check()?,
            Self::HeadTouch { value, .. } => value.check()?,
            Self::HeadHand { value, .. } => value.check()?,
            Self::VoiceMute { value, .. } => value.check()?,
            Self::Due { value, .. } => value.check()?,
            Self::DayTime { minute, .. } => {
                if *minute >= 1_440 {
                    return Err(Error("a minute of the day is 0..1439".into()));
                }
            }
            Self::Heard { value, .. } | Self::Said { value, .. } => {
                if value.text.len() > CONTEXT_BYTES
                    || value.locale.as_ref().is_some_and(|v| v.len() > 64)
                {
                    return Err(Error(
                        "speech context exceeds 16 KiB / 64-byte locale".into(),
                    ));
                }
            }
            Self::VoiceState { .. } | Self::Room { .. } | Self::Remarked { .. } => {}
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Output {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<ExpressionBody>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<ExpressionLight>,
    pub state: ExpressionState,
}

#[derive(Clone, Copy, Default)]
struct ClipState {
    pause_seen: Option<u64>,
    sudden_seen: Option<u64>,
    remarked_seen: Option<u64>,
    started: Option<u64>,
    last_started: Option<u64>,
    last_completed: Option<u64>,
}

#[derive(Clone, Copy)]
struct Delivery {
    generation: u64,
    held: bool,
    // The first accepted active edge in this generation, or its resume edge.
    // Later heartbeats cannot make an old sound fresh again.
    active_since: u64,
}

#[derive(Clone, Copy)]
struct RecentLevel {
    end_us: u64,
    level: f32,
}

#[derive(Clone)]
struct Interruption {
    at_us: u64,
    from: BodyOffsets,
}

/// Bounded state with no wall clock, sleeping, I/O or randomness. All times
/// supplied by the embedding process must be on the same monotonic boot clock.
pub struct Engine {
    catalogue: Catalogue,
    enabled: bool,
    clock_id: String,
    session_id: String,
    now: Option<u64>,
    sequence: u64,
    fault: Option<&'static str>,
    marks: [Option<(u64, u64)>; STREAMS],
    /// The head with the lamp's own speech (#933).
    speech: crate::speaking::SpeechMotion,
    voice: Option<VoiceTiming>,
    delivery: Option<Delivery>,
    voice_since: u64,
    retired_turn: bool,
    cancelled: bool,
    retired_sessions: VecDeque<String>,
    activity: Option<PersonActivity>,
    has_speech: bool,
    pause_since: Option<u64>,
    /// When the newest short sudden sound was heard, as Expression received it.
    sudden_since: Option<u64>,
    /// When the newest remark about the lamp was said (#4175).
    remarked_since: Option<u64>,
    clips: Vec<ClipState>,
    playout: VecDeque<VoicePlayout>,
    recent_playout: Option<RecentLevel>,
    last_playout_end: Option<u64>,
    playout_since: Option<u64>,
    first_audio: Option<u64>,
    late_schedule: bool,
    interruption: Option<Interruption>,
    last_offsets: BodyOffsets,
    joints_at: Option<u64>,
    faces: Option<(ClockStamp, Faces)>,
    room: Option<(u64, Room)>,
    light: Option<Light>,
    concentration: Option<Concentration>,
    rest_since: Option<u64>,
    legacy_voice: Option<VoiceState>,
    rejected: u64,
    /// The idle layer, sleep and waking, when the catalogue has them.
    life: Option<Life>,
    /// The internet cannot be reached (`lamp/online` false, #830): the
    /// listening face is not drawn as ready.
    offline: bool,
    routine: Option<rest::Rest>,
    routine_paused: bool,
    privacy_muted: bool,
    /// The skill that claims the night's sleep (#940), while its claim is
    /// fresh: only with rest requests on.
    sleep_claim: Option<String>,
}

/// One glance at a moment `heard` (a sudden sound or a remark), not yet
/// `seen`: started now if inside its stroke and past its cooldown, else let
/// go. Returns the moment once it is due, so it is never taken twice.
fn glance_once(
    runtime: &mut ClipState,
    seen: Option<u64>,
    heard: Option<u64>,
    clip: &crate::Behaviour,
    now_us: u64,
) -> Option<u64> {
    let heard = heard?;
    let start = heard.saturating_add(clip.delay_us);
    if seen == Some(heard) || now_us < start {
        return None;
    }
    if runtime.started.is_none()
        && now_us <= start.saturating_add(clip.stroke_us)
        && runtime
            .last_started
            .is_none_or(|last| now_us.saturating_sub(last) >= clip.cooldown_us)
    {
        runtime.started = Some(now_us);
        runtime.last_started = Some(now_us);
    }
    Some(heard)
}

impl Engine {
    pub fn new(
        catalogue: Catalogue,
        enabled: bool,
        clock_id: impl Into<String>,
    ) -> Result<Self, Error> {
        catalogue.check()?;
        let clock_id = clock_id.into();
        ClockStamp {
            clock_id: clock_id.clone(),
            at_us: 0,
            sequence: 0,
        }
        .check()?;
        let clips = vec![ClipState::default(); catalogue.behaviours.len()];
        let life = catalogue.alive.clone().map(|alive| {
            Life::new(
                alive,
                catalogue.timing.context_stale_us,
                catalogue.timing.activity_stale_us,
            )
        });
        Ok(Self {
            catalogue,
            enabled,
            clock_id,
            session_id: "expression".into(),
            now: None,
            sequence: 0,
            fault: None,
            marks: [None; STREAMS],
            speech: Default::default(),
            voice: None,
            delivery: None,
            voice_since: 0,
            retired_turn: false,
            cancelled: false,
            retired_sessions: VecDeque::new(),
            activity: None,
            has_speech: false,
            pause_since: None,
            sudden_since: None,
            remarked_since: None,
            clips,
            playout: VecDeque::new(),
            recent_playout: None,
            last_playout_end: None,
            playout_since: None,
            first_audio: None,
            late_schedule: false,
            interruption: None,
            last_offsets: BodyOffsets::ZERO,
            joints_at: None,
            faces: None,
            room: None,
            light: None,
            concentration: None,
            rest_since: None,
            legacy_voice: None,
            rejected: 0,
            life,
            offline: false,
            routine: None,
            routine_paused: false,
            privacy_muted: false,
            sleep_claim: None,
        })
    }

    /// Live publishers must use a fresh process identifier; replay may keep
    /// the deterministic default. This is allowed only before the first tick.
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Result<Self, Error> {
        let session_id = session_id.into();
        ClockStamp {
            clock_id: session_id.clone(),
            at_us: 0,
            sequence: 0,
        }
        .check()?;
        if self.now.is_some() {
            return Err(Error("planner session must be set before clock use".into()));
        }
        self.session_id = session_id;
        Ok(self)
    }

    pub fn clock_id(&self) -> &str {
        &self.clock_id
    }

    /// `LAMP_REST_THROUGH_SOUNDS=on` (#1672): asleep for the night, the
    /// voice thinking does not wake the lamp, only its own answer does. Its
    /// counted name stirs it (the head rises a little) and it settles back
    /// if no answer comes within 5 s (#4285); a touch and a reminder wake it
    /// as before.
    pub fn with_rest_through_sounds(mut self) -> Self {
        if let Some(life) = &mut self.life {
            life.sleep_through_thinking();
        }
        self
    }

    /// Opt into leased skill scheduling; the default keeps autonomous sleep.
    pub fn with_rest_requests(mut self) -> Self {
        if self.routine.is_some() {
            return self;
        }
        if let Some(life) = &mut self.life {
            life.set_scheduling(alive::Scheduling::Skills, self.now.unwrap_or(0));
        }
        self.routine = Some(rest::Rest::default());
        self
    }

    /// The skill's claim on the night's sleep (#940), fresh, or none. Only
    /// with rest requests on: a claim gives the night to skills (`on` or
    /// not: off is a person's choice that it not sleep), and the claimed
    /// skill's rest is held as the built-in sleep is. With none, Expression
    /// sleeps by its own schedule again.
    ///
    /// FALLBACK (#940, for one release): no claim, or a stale one, is a
    /// lamp-skills that is down or older than night-sleep, so the built-in
    /// schedule runs as it did before skills held the night. The release
    /// after removes it.
    pub fn set_sleep_claim(
        &mut self,
        now_us: u64,
        claim: Option<&lamp_msgs::ExpressionSleepClaim>,
    ) {
        if self.routine.is_none() {
            return;
        }
        let skill = claim.map(|claim| claim.skill.clone());
        if skill.is_none()
            && self.sleep_claim.is_some()
            && let Some(routine) = &mut self.routine
        {
            routine.withdraw(now_us, "no skill claims the night any more", &mut self.life);
        }
        self.sleep_claim = skill;
        if let Some(life) = &mut self.life {
            life.set_scheduling(
                if claim.is_some() {
                    alive::Scheduling::Skills
                } else {
                    alive::Scheduling::Autonomous
                },
                now_us,
            );
        }
    }

    fn routine_gate(&self, now_us: u64) -> rest::Gate {
        let voice = self.voice.as_ref().filter(|voice| {
            voice.stamp.at_us <= now_us
                && now_us - voice.stamp.at_us < self.catalogue.timing.voice_stale_us
        });
        let free = !self.held()
            && self.joints_fresh(now_us)
            && !self
                .concentration
                .as_ref()
                .is_some_and(|value| value.concentrating);
        // The first gate that is shut, said in the journal when it ends a
        // rest (row 7 on lamp-06c1 had a rest end without a line, #1672).
        let why_not = if !self.enabled || self.fault.is_some() || self.cancelled {
            Some("Expression is off, faulted or cancelled")
        } else if self.routine_paused {
            Some("the lamp is paused")
        } else if self.privacy_muted {
            Some("the microphones are off")
        } else if !free {
            Some("the body is held, its joints are stale, or someone is concentrating")
        } else {
            // Idle Voice need not retain a Listening turn. Its explicit
            // manual mute and the physical privacy route are separate gates.
            match voice.map(|voice| voice.state) {
                Some(Voice::Resting) => None,
                Some(Voice::Listening) => Some("the voice is no longer resting: it is listening"),
                Some(Voice::Thinking | Voice::Speaking) => {
                    Some("the voice is no longer resting: it is in a turn")
                }
                None => Some("the voice's state is stale"),
            }
        };
        rest::Gate {
            eligible: why_not.is_none(),
            why_not,
            off: (!self.enabled || self.fault.is_some()).then_some("Expression is off or faulted"),
            sleep: self.sleep_claim.clone(),
            owner: voice.map(|voice| rest::Owner {
                session: voice.session_id.clone(),
                turn: voice.turn,
                request: voice.request,
            }),
            context: alive::Context {
                voice: voice.map(|voice| voice.state),
                free,
            },
        }
    }

    fn sync_routine(&mut self, now_us: u64) {
        if self.routine.is_none() {
            return;
        }
        let gate = self.routine_gate(now_us);
        if let Some(life) = &mut self.life {
            life.observe_routine_voice(gate.context.voice);
        }
        self.routine
            .as_mut()
            .unwrap()
            .sync(now_us, &gate, &mut self.life);
    }

    /// The keeper's current pause is checked before intake and again before
    /// planning. A resume never revives an episode cancelled by that pause.
    pub fn set_rest_paused(&mut self, now_us: u64, paused: bool) {
        self.routine_paused = paused;
        self.sync_routine(now_us);
    }

    /// Transport has checked declared type, publisher source/boot/sequence.
    /// The original monotonic stamp is retained; this method never retimes it.
    pub fn ingest_rest(
        &mut self,
        now_us: u64,
        request: lamp_msgs::ExpressionRestRequest,
    ) -> Result<(), Error> {
        self.ingest_rest_with_gap(now_us, request, false)
    }

    pub(crate) fn ingest_rest_with_gap(
        &mut self,
        now_us: u64,
        request: lamp_msgs::ExpressionRestRequest,
        lost_input: bool,
    ) -> Result<(), Error> {
        self.advance(now_us)?;
        request.check()?;
        if request.stamp.clock_id != self.clock_id
            || request.expression_session_id != self.session_id
        {
            return Err(Error(
                "rest request is for another boot or Expression session".into(),
            ));
        }
        let gate = self.routine_gate(now_us);
        let routine = self
            .routine
            .as_mut()
            .ok_or_else(|| Error("rest requests are off".into()))?;
        routine.offer(now_us, request, &gate, &mut self.life, lost_input)
    }

    /// A fresh planning receipt, never proof of applied motion. The existing
    /// body owner still enforces its folded/paused and local motor guards.
    pub fn rest_state(&mut self, now_us: u64) -> Option<lamp_msgs::ExpressionRestState> {
        let _ = self.advance(now_us);
        self.sync_routine(now_us);
        self.routine
            .as_ref()
            .map(|routine| lamp_msgs::ExpressionRestState {
                stamp: ClockStamp {
                    clock_id: self.clock_id.clone(),
                    at_us: now_us,
                    sequence: self.sequence,
                },
                expression_session_id: self.session_id.clone(),
                enabled: self.enabled && self.fault.is_none(),
                request: routine.request.clone(),
                phase: routine.phase,
                reason: routine
                    .reason
                    .filter(|_| routine.phase == lamp_msgs::ExpressionRestPhase::Refused)
                    .map(str::to_owned),
            })
    }

    /// Why the last active rest was ended by Expression itself, once: a
    /// shut gate, a stronger reflex or a lapsed lease, for the journal.
    pub fn take_rest_ended(&mut self) -> Option<&'static str> {
        self.routine
            .as_mut()
            .and_then(|routine| routine.ended.take())
    }

    pub fn rejected_inputs(&self) -> u64 {
        self.rejected
    }

    /// One-shot, bounded notices for the journal. They are not persistent
    /// planner warnings and never change the actuator or state messages.
    pub fn take_sleep_name_notices(&mut self) -> Vec<String> {
        self.life
            .as_mut()
            .map_or_else(Vec::new, Life::take_name_notices)
    }

    fn fence_sleep_name(&mut self, stamp_us: u64) {
        if let Some(life) = &mut self.life {
            life.fence_name(stamp_us);
        }
    }

    /// What has gone wrong with the planner itself, if anything has: a
    /// clock that moved backwards, a voice restarted too often. Until it is
    /// restarted it asks nothing of the body or the light.
    pub fn fault(&self) -> Option<&'static str> {
        self.fault
    }

    pub fn queued_playout_frames(&self) -> usize {
        self.playout.len()
    }

    fn advance(&mut self, now_us: u64) -> Result<(), Error> {
        if self.now.is_some_and(|last| now_us < last) {
            self.fault = Some("clock moved backwards; restart expression with the new clock");
            self.clear_transients();
        }
        if now_us > u64::MAX - LEASE_US {
            self.fault = Some("clock cannot represent another command lease");
            self.clear_transients();
        }
        if let Some(fault) = self.fault {
            self.fence_sleep_name(now_us);
            self.sync_routine(now_us);
            return Err(Error(fault.into()));
        }
        self.now = Some(now_us);
        if let Some(life) = &mut self.life {
            life.advance_name(now_us);
        }
        if let Some(expired_at) = self.voice.as_ref().and_then(|voice| {
            let expires = voice
                .stamp
                .at_us
                .saturating_add(self.catalogue.timing.voice_stale_us);
            (now_us >= expires).then_some(expires)
        }) {
            self.retired_turn = true;
            self.clear_transients();
            self.fence_sleep_name(expired_at);
        }
        while self
            .playout
            .front()
            .is_some_and(|frame| frame.end_us <= now_us)
        {
            let frame = self.playout.pop_front().unwrap();
            self.remember_played(now_us, &frame);
        }
        if self.recent_playout.is_some_and(|recent| {
            now_us.saturating_sub(recent.end_us) >= self.catalogue.timing.recent_playout_us
        }) {
            self.recent_playout = None;
        }
        if self.activity.as_ref().is_some_and(|activity| {
            now_us.saturating_sub(activity.stamp.at_us) >= self.catalogue.timing.activity_stale_us
        }) {
            self.activity = None;
            self.has_speech = false;
            self.pause_since = None;
            for (clip, runtime) in self.catalogue.behaviours.iter().zip(&mut self.clips) {
                // A finishing nod has already observed a genuine pause. Once
                // its allowed Thinking transition happens, Voice need not keep
                // publishing the Listening detector's quiet heartbeat.
                // A glance at a sudden sound or a remark is the room's and
                // finishes.
                if !clip.trigger.is_rooms()
                    && !self.voice.as_ref().is_some_and(|voice| {
                        voice.state == Voice::Thinking
                            && clip.trigger == Trigger::Pause
                            && clip.continue_in.contains(&Voice::Thinking)
                    })
                {
                    runtime.started = None;
                }
            }
        }
        if !self.joints_fresh(now_us) || self.room_at(now_us) != Room::Quiet {
            for (clip, runtime) in self.catalogue.behaviours.iter().zip(&mut self.clips) {
                if clip.trigger == Trigger::Pause {
                    runtime.started = None;
                }
            }
        }
        self.update_rest_eligibility(now_us);
        self.sync_routine(now_us);
        Ok(())
    }

    fn clear_transients(&mut self) {
        self.reset_rest(self.now.unwrap_or(0));
        self.activity = None;
        self.has_speech = false;
        self.pause_since = None;
        // A sudden sound is the room's, not the voice's: it outlives a turn,
        // and a glance already under way finishes. It still stops if the lamp
        // begins to speak, since its clip is not allowed while speaking. So
        // does a glance at a remark (#4175).
        for (behaviour, clip) in self.catalogue.behaviours.iter().zip(&mut self.clips) {
            if !behaviour.trigger.is_rooms() {
                clip.started = None;
            }
            clip.pause_seen = None;
        }
        self.playout.clear();
        self.recent_playout = None;
        self.first_audio = None;
        self.playout_since = None;
        self.interruption = None;
        self.late_schedule = false;
        self.speech.forget();
    }

    /// An answer ending as it should at `now_us` owes its settle (#933).
    fn owes_settle(&self, now_us: u64) -> bool {
        self.catalogue
            .speaking
            .as_ref()
            .is_some_and(|rules| self.speech.owes_settle(rules, now_us))
    }

    fn held(&self) -> bool {
        self.delivery.is_some_and(|delivery| delivery.held)
    }

    fn joints_fresh(&self, now_us: u64) -> bool {
        self.joints_at
            .is_some_and(|at| now_us.saturating_sub(at) < self.catalogue.timing.joints_stale_us)
    }

    fn room_at(&self, now_us: u64) -> Room {
        self.room
            .filter(|(at, _)| now_us.saturating_sub(*at) < self.catalogue.timing.context_stale_us)
            .map_or(Room::Unknown, |(_, room)| room)
    }

    fn reset_rest(&mut self, now_us: u64) {
        self.rest_since = None;
        for (clip, runtime) in self.catalogue.behaviours.iter().zip(&mut self.clips) {
            if clip.trigger == Trigger::Rest && runtime.started.take().is_some() {
                runtime.last_completed = Some(now_us);
            }
        }
    }

    fn update_rest_eligibility(&mut self, now_us: u64) {
        let eligible = self.fault.is_none()
            && !self.cancelled
            && !self.held()
            && self.voice.as_ref().is_some_and(|voice| {
                voice.state == Voice::Resting
                    && voice.accepting_input
                    && now_us.saturating_sub(voice.stamp.at_us)
                        < self.catalogue.timing.voice_stale_us
            })
            && self.joints_fresh(now_us)
            && self.room_at(now_us) == Room::Quiet
            && !self.concentration.as_ref().is_some_and(|v| v.concentrating);
        if eligible {
            self.rest_since.get_or_insert(now_us);
        } else {
            self.reset_rest(now_us);
        }
    }

    /// Whether the lamp can reach the internet: while it cannot, listening
    /// is drawn as not ready (#830). The voice's ring shows the offline
    /// breath; the work light does not say "go ahead".
    pub fn set_offline(&mut self, offline: bool) {
        self.offline = offline;
    }

    /// The lamp's one breath's phase now, from 0 to 1, where Mood keeps it
    /// (#1670), or `None` for the plain breath on the boot clock.
    pub fn set_mood_breath(&mut self, phase: Option<f64>) {
        if let Some(life) = &mut self.life {
            life.set_mood_breath(phase);
        }
    }

    /// How lively the idle layer's moves are from the lamp's inner state, from
    /// -0.5 to 0.5 ([`lamp_msgs::mood_liveliness`]), added to the catalogue's
    /// energy; `None`, the catalogue's alone (#1670).
    pub fn set_mood_liveliness(&mut self, liveliness: Option<f32>) {
        if let Some(life) = &mut self.life {
            life.set_mood_liveliness(liveliness);
        }
    }

    /// Whether the privacy switch is closed, as `voice/privacy` says at a
    /// request from the head (#711): a tap then gets no look up.
    pub fn set_privacy_muted(&mut self, muted: bool) {
        self.privacy_muted = muted;
        if let Some(life) = &mut self.life {
            life.set_privacy_muted(muted);
        }
    }

    /// Whether it is looking up at a tap on the head (#711).
    pub fn looking_up(&self) -> bool {
        self.life.as_ref().is_some_and(|life| life.looking_up())
    }

    /// Forget the time of day given so far: the clock is not set (#830).
    /// Without it the alive layer knows no night.
    pub fn forget_daytime(&mut self) {
        if let Some(life) = &mut self.life {
            life.forget_daytime();
        }
    }

    pub fn ingest(&mut self, now_us: u64, input: Input) -> Result<(), Error> {
        let result = self.ingest_checked(now_us, input);
        if result.is_err() {
            self.rejected = self.rejected.saturating_add(1);
        }
        result
    }

    fn ingest_checked(&mut self, now_us: u64, input: Input) -> Result<(), Error> {
        self.advance(now_us)?;
        input.check()?;
        let wrong_protocol = match &input {
            Input::Voice(_) | Input::Playout(_) => self.catalogue.resumable_voice,
            Input::Progress(_) | Input::Sound(_) => !self.catalogue.resumable_voice,
            _ => false,
        };
        if wrong_protocol {
            return Err(Error(
                "voice input does not match the selected catalogue protocol".into(),
            ));
        }
        let stamp = input.stamp().clone();
        if stamp.clock_id != self.clock_id {
            return Err(Error("input clock_id differs from this boot".into()));
        }
        let age_limit = match &input {
            Input::Concentration(_) | Input::Light { .. } => u64::MAX,
            Input::Voice(_) | Input::Progress(_) | Input::VoiceState { .. } => {
                self.catalogue.timing.voice_stale_us
            }
            Input::Activity(_) => self.catalogue.timing.activity_stale_us,
            Input::Playout(_) | Input::Sound(_) => LEASE_US,
            Input::Joints { .. } => self.catalogue.timing.joints_stale_us,
            Input::Faces { .. } => self.catalogue.timing.faces_stale_us,
            _ => self.catalogue.timing.context_stale_us,
        };
        if stamp.at_us > now_us || now_us - stamp.at_us >= age_limit {
            return Err(Error(
                "input observation is future or stale at arrival".into(),
            ));
        }
        let stream = input.stream();
        let changed_session = match &input {
            Input::Voice(voice) => self
                .voice
                .as_ref()
                .is_some_and(|old| voice.session_id != old.session_id),
            Input::Progress(progress) => self
                .voice
                .as_ref()
                .is_some_and(|old| progress.timing.session_id != old.session_id),
            _ => false,
        };
        if let Some((at, sequence)) = self.marks[stream] {
            // Concentration's setting publisher may restart; an actually newer
            // monotonic observation is its authority, not a reset counter.
            let can_reset = changed_session || matches!(input, Input::Concentration(_));
            if stamp.at_us < at
                || (stamp.sequence <= sequence && (!can_reset || stamp.at_us == at))
                || (changed_session && stamp.at_us == at)
            {
                return Err(Error("input is duplicate or out of order".into()));
            }
        }
        match input {
            Input::Voice(voice) => self.accept_voice(now_us, voice, None)?,
            Input::Progress(progress) => self.accept_voice(
                now_us,
                progress.timing,
                Some((progress.audio_generation, progress.held)),
            )?,
            Input::Activity(activity) => {
                self.check_turn(&activity.session_id, activity.turn, activity.request, false)?;
                let allowed = self.voice.as_ref().is_some_and(|v| {
                    let retained_listening = v.state == Voice::Listening && v.accepting_input;
                    let cancelling_carried_nod = v.state == Voice::Thinking
                        && activity.speaking
                        && self.catalogue.behaviours.iter().zip(&self.clips).any(
                            |(clip, runtime)| {
                                clip.trigger == Trigger::Pause
                                    && clip.continue_in.contains(&Voice::Thinking)
                                    && runtime.started.is_some()
                            },
                        );
                    retained_listening || cancelling_carried_nod
                });
                if !allowed {
                    return Err(Error(
                        "person activity requires retained listening input or a carried finishing nod".into(),
                    ));
                }
                if activity.stamp.at_us < self.voice_since {
                    return Err(Error(
                        "person activity predates the current voice phase".into(),
                    ));
                }
                if activity.speaking {
                    if let Some(life) = &mut self.life {
                        life.person_speech(now_us, activity.stamp.at_us);
                    }
                    self.has_speech = true;
                    self.pause_since = None;
                    // Speech stops what the lamp was doing for the voice; a
                    // glance at a sudden sound or a remark is the room's and
                    // finishes.
                    for (behaviour, clip) in self.catalogue.behaviours.iter().zip(&mut self.clips) {
                        if !behaviour.trigger.is_rooms() {
                            clip.started = None;
                        }
                    }
                } else if self.has_speech && self.activity.as_ref().is_some_and(|old| old.speaking)
                {
                    self.pause_since = Some(activity.stamp.at_us);
                }
                self.activity = Some(activity);
            }
            Input::Playout(frame) => self.accept_playout(now_us, frame)?,
            Input::Sound(sound) => {
                let delivery = self.delivery.ok_or_else(|| {
                    Error("voice/progress has not established an audio generation".into())
                })?;
                if delivery.held || sound.audio_generation != delivery.generation {
                    return Err(Error(
                        "sound is held or belongs to an inactive audio generation".into(),
                    ));
                }
                if sound.playout.stamp.at_us < delivery.active_since
                    || sound.playout.start_us < delivery.active_since
                {
                    return Err(Error(
                        "sound predates the accepted generation or resume edge".into(),
                    ));
                }
                self.accept_playout(now_us, sound.playout)?;
            }
            Input::Concentration(value) => {
                if value.concentrating {
                    self.reset_rest(now_us);
                    self.pause_since = None;
                    for clip in &mut self.clips {
                        clip.started = None;
                    }
                }
                self.concentration = Some(value);
            }
            Input::VoiceState { value, .. } => self.legacy_voice = Some(value),
            Input::Joints { stamp, value } => {
                self.joints_at = Some(stamp.at_us);
                if let Some(life) = &mut self.life {
                    life.joints(stamp.at_us, &value.commanded);
                }
            }
            Input::Faces { stamp, value } => {
                if let Some(life) = &mut self.life {
                    life.faces(stamp.at_us, &value, self.catalogue.gaze.minimum_confidence);
                }
                self.faces = Some((stamp, value));
            }
            Input::Room { stamp, value } => self.room = Some((stamp.at_us, value.state)),
            Input::Light { value, .. } => self.light = Some(value),
            // These are context only. No word is invented as a pause, readiness,
            // a timing cue or proof that anything has reached the loudspeaker.
            // A name may wake the alive layer; its optional speech check uses
            // accepted Activity, never transcript text or plain Listening.
            Input::Heard { .. } | Input::Said { .. } => {}
            Input::Name { stamp, value } => {
                if let Some(life) = &mut self.life {
                    life.name(now_us, stamp.at_us, value.ago_ms);
                }
            }
            Input::Touch { value, .. } => {
                if let Some(life) = &mut self.life {
                    life.touch(&value);
                }
            }
            Input::Due { value, .. } => {
                if let Some(life) = &mut self.life {
                    life.due(&value);
                }
            }
            Input::HeadTouch { stamp, value } => {
                if let Some(life) = &mut self.life {
                    life.head_touch(stamp.at_us, &value);
                }
            }
            Input::HeadHand { stamp, value } => {
                if let Some(life) = &mut self.life {
                    life.head_hand(stamp.at_us, &value);
                }
            }
            Input::VoiceMute { stamp, value } => {
                if let Some(life) = &mut self.life {
                    life.voice_mute(stamp.at_us, &value);
                }
            }
            Input::DayTime { stamp, minute } => {
                if let Some(life) = &mut self.life {
                    life.daytime(stamp.at_us, minute);
                }
            }
            // Only a short one (a knock, a clap, something dropped) is a
            // reason to glance; a sound that goes on is a voice or a chair.
            Input::Sudden { stamp, value } => {
                if value.short {
                    self.sudden_since = Some(stamp.at_us);
                }
            }
            // Its own moment, never the startle's: a remark and a bang each
            // keep their own once and their own cooldown (#4175).
            Input::Remarked { stamp, .. } => self.remarked_since = Some(stamp.at_us),
        }
        self.update_rest_eligibility(now_us);
        self.marks[stream] = Some((stamp.at_us, stamp.sequence));
        self.sync_routine(now_us);
        Ok(())
    }

    fn accept_playout(&mut self, now_us: u64, frame: VoicePlayout) -> Result<(), Error> {
        self.check_turn(&frame.session_id, frame.turn, frame.request, true)?;
        if self
            .voice
            .as_ref()
            .is_none_or(|v| !matches!(v.state, Voice::Thinking | Voice::Speaking))
        {
            return Err(Error(
                "playout requires the active thinking or speaking turn".into(),
            ));
        }
        if (frame.end_us <= now_us
            && now_us - frame.end_us >= self.catalogue.timing.recent_playout_us)
            || frame.start_us > now_us.saturating_add(self.catalogue.timing.max_future_audio_us)
            || self
                .last_playout_end
                .is_some_and(|end| frame.start_us < end)
        {
            return Err(Error(
                "playout is expired, too far ahead, overlapping or out of order".into(),
            ));
        }
        if self.playout.len() >= MAX_PLAYOUT_FRAMES {
            return Err(Error(
                "playout queue is full; refusing more future frames".into(),
            ));
        }
        self.last_playout_end = Some(frame.end_us);
        self.playout_since.get_or_insert(frame.start_us);
        if frame.end_us <= now_us {
            // This is evidence of a level already heard. It is never queued as
            // current/future playback and cannot establish a first-audio time.
            self.remember_played(now_us, &frame);
        } else {
            self.playout.push_back(frame);
        }
        Ok(())
    }

    fn remember_played(&mut self, now_us: u64, frame: &VoicePlayout) {
        if now_us.saturating_sub(frame.end_us) < self.catalogue.timing.recent_playout_us {
            self.recent_playout = Some(RecentLevel {
                end_us: frame.end_us,
                level: frame.level,
            });
        }
    }

    fn recent_level(&self, now_us: u64) -> Option<f32> {
        let recent = self.recent_playout?;
        let age = now_us.checked_sub(recent.end_us)?;
        let window = self.catalogue.timing.recent_playout_us;
        (age < window).then(|| recent.level * (1.0 - age as f32 / window as f32))
    }

    fn check_turn(&self, session: &str, turn: u64, request: u64, audio: bool) -> Result<(), Error> {
        let Some(voice) = &self.voice else {
            return Err(Error("voice/timing has not established a session".into()));
        };
        if session != voice.session_id || turn != voice.turn || request != voice.request {
            return Err(Error(
                "input does not belong to the active voice session/turn/request".into(),
            ));
        }
        if audio && self.retired_turn {
            return Err(Error(
                "turn audio was cancelled, finished or expired".into(),
            ));
        }
        if self.cancelled {
            return Err(Error("turn was cancelled".into()));
        }
        Ok(())
    }

    fn accept_voice(
        &mut self,
        now_us: u64,
        voice: VoiceTiming,
        progress: Option<(u64, bool)>,
    ) -> Result<(), Error> {
        let new_session = self
            .voice
            .as_ref()
            .is_some_and(|old| old.session_id != voice.session_id);
        let new_turn = self
            .voice
            .as_ref()
            .is_none_or(|old| new_session || old.turn < voice.turn);
        let was_held = !new_turn && self.held();
        let held = progress.is_some_and(|(_, held)| held);
        let new_generation = progress.is_some_and(|(generation, _)| {
            new_turn || self.delivery.is_none_or(|old| old.generation != generation)
        });
        if !new_turn
            && let (Some((generation, held)), Some(old)) = (progress, self.delivery)
            && (generation < old.generation || (generation == old.generation && held && !old.held))
        {
            return Err(Error(
                "audio generation moved backwards or a resumed generation was held again".into(),
            ));
        }
        let repeated_terminal = !new_turn
            && self.retired_turn
            && matches!(
                voice.event,
                VoiceEvent::Interrupted | VoiceEvent::Cancelled | VoiceEvent::Finished
            )
            && (voice.event != VoiceEvent::Cancelled || self.cancelled);
        if self.retired_sessions.contains(&voice.session_id) {
            return Err(Error("voice session is retired".into()));
        }
        if let Some(old) = &self.voice {
            if !new_session
                && (voice.turn < old.turn
                    || (voice.turn == old.turn && voice.request != old.request))
            {
                return Err(Error(
                    "voice turn moved backwards or changed request identity".into(),
                ));
            }
            if !new_turn
                && self.retired_turn
                && matches!(voice.state, Voice::Thinking | Voice::Speaking)
            {
                return Err(Error(
                    "a retired voice turn cannot resume audio or thinking".into(),
                ));
            }
        }
        if voice.event == VoiceEvent::Scheduled
            && voice.first_audio_us.is_none_or(|at| {
                at <= now_us || at - now_us > self.catalogue.timing.max_future_audio_us
            })
        {
            return Err(Error(
                "scheduled first audio is already due or too far ahead at arrival".into(),
            ));
        }
        // A normal resting owner may say accepting_input=false until a new
        // Listening turn. Keep a pending name through that edge. Actual loss
        // of retained input, a new owner or a terminal edge invalidates its
        // speech evidence; clear_transients alone is too broad (new turns).
        let lost_input = !voice.accepting_input
            && (voice.state == Voice::Listening
                || self.voice.as_ref().is_some_and(|old| old.accepting_input));
        if new_session
            || lost_input
            || (!repeated_terminal
                && matches!(
                    voice.event,
                    VoiceEvent::Interrupted | VoiceEvent::Cancelled | VoiceEvent::Finished
                ))
        {
            self.fence_sleep_name(voice.stamp.at_us);
        }
        if voice.accepting_input
            && self.voice.as_ref().is_none_or(|old| !old.accepting_input)
            && let Some(life) = &mut self.life
        {
            life.name_speech_since(voice.stamp.at_us);
        }
        if new_turn {
            if new_session {
                // Bounded: a lamp restarts its voice at every update and
                // after a fault, so a fence that filled up and then refused
                // every new session stopped Expression within weeks (lamp-4ace
                // passed 32 in a day, 17 Oct). The oldest is forgotten instead.
                if self.retired_sessions.len() >= MAX_VOICE_SESSIONS {
                    self.retired_sessions.pop_front();
                }
                self.retired_sessions
                    .push_back(self.voice.as_ref().unwrap().session_id.clone());
                self.marks[1] = None;
                self.marks[2] = None;
                self.marks[13] = None;
            }
            // Whoever cuts the lamp off is heard at once, as a turn of
            // their own. The lamp is still turning from what it was doing
            // to them, and that goes on into their turn: on a lamp the
            // interruption and the turn that made it arrive together.
            let turning = self.interruption.take().filter(|_| !new_session);
            self.clear_transients();
            self.interruption = turning;
            self.retired_turn = false;
            self.cancelled = false;
            self.last_playout_end = None;
            self.voice_since = voice.stamp.at_us;
        } else if new_generation {
            self.clear_transients();
            self.last_playout_end = None;
            self.voice_since = voice.stamp.at_us;
        } else if self
            .voice
            .as_ref()
            .is_some_and(|old| old.state != voice.state)
        {
            // A normal phase edge keeps committed future intervals, but an
            // observation from the previous phase is not a new speaking cue.
            self.recent_playout = None;
            self.reset_rest(now_us);
            self.voice_since = voice.stamp.at_us;
            if voice.state != Voice::Listening {
                self.pause_since = None;
                self.activity = None;
                self.has_speech = false;
                for (clip, runtime) in self.catalogue.behaviours.iter().zip(&mut self.clips) {
                    let carry = !was_held
                        && self
                            .voice
                            .as_ref()
                            .is_some_and(|old| old.state == Voice::Listening)
                        && voice.state == Voice::Thinking
                        && clip.trigger == Trigger::Pause
                        && clip.continue_in.contains(&Voice::Thinking);
                    if !carry && !clip.trigger.is_rooms() {
                        runtime.started = None;
                    }
                }
            }
        }
        if held && (new_generation || !was_held) {
            let from = self.last_offsets;
            self.clear_transients();
            self.last_playout_end = None;
            self.interruption = Some(Interruption {
                at_us: voice.stamp.at_us,
                from,
            });
        } else if was_held
            && !held
            && matches!(voice.event, VoiceEvent::State | VoiceEvent::Scheduled)
        {
            // Resume starts a new active interval in the same generation. No
            // detector history or delayed sound from the hold may enter it.
            self.voice_since = voice.stamp.at_us;
            self.clear_transients();
            self.last_playout_end = None;
        }
        match voice.event {
            VoiceEvent::Scheduled => {
                let first = voice.first_audio_us.unwrap();
                let t = self.catalogue.timing;
                self.late_schedule = first - now_us
                    < t.return_us
                        .saturating_add(t.return_before_us)
                        .saturating_add(t.body_lag_us.unwrap_or(0));
                self.first_audio = Some(first);
            }
            VoiceEvent::Interrupted if !repeated_terminal => {
                // Preserve the currently requested offset, then interpolate to
                // listening. The body's executor remains responsible for its
                // actual physical starting pose and velocity limits.
                let from = self.last_offsets;
                let held_yield = was_held.then(|| self.interruption.clone()).flatten();
                self.clear_transients();
                self.retired_turn = true;
                self.interruption = if was_held {
                    held_yield
                } else {
                    Some(Interruption {
                        at_us: voice.stamp.at_us,
                        from,
                    })
                };
            }
            VoiceEvent::Cancelled | VoiceEvent::Finished if !repeated_terminal => {
                // Only a finished answer is settled after, never a cancelled one.
                let settle = voice.event == VoiceEvent::Finished && self.owes_settle(now_us);
                self.clear_transients();
                if settle {
                    self.speech.settle_from(now_us);
                }
                self.retired_turn = true;
                self.cancelled |= voice.event == VoiceEvent::Cancelled;
            }
            VoiceEvent::State => {
                // An unfenced Listening/Resting edge retires queued audio. A
                // newer tagged generation instead proves a missed hold: its
                // queue was cleared above, and it may still resume this turn.
                if !new_turn
                    && !held
                    && !new_generation
                    && matches!(voice.state, Voice::Listening | Voice::Resting)
                    && self
                        .voice
                        .as_ref()
                        .is_some_and(|old| matches!(old.state, Voice::Thinking | Voice::Speaking))
                {
                    // The answer ended as it should: its settle is still owed.
                    let settle = self.owes_settle(now_us);
                    self.clear_transients();
                    if settle {
                        self.speech.settle_from(now_us);
                    }
                    self.retired_turn = true;
                }
            }
            VoiceEvent::Interrupted | VoiceEvent::Cancelled | VoiceEvent::Finished => {}
        }
        self.delivery = progress.map(|(generation, held)| Delivery {
            generation,
            held,
            active_since: if new_generation || (was_held && !held) {
                voice.stamp.at_us
            } else {
                self.delivery
                    .map_or(voice.stamp.at_us, |old| old.active_since)
            },
        });
        self.voice = Some(voice);
        Ok(())
    }

    /// One requested frame. Catch-up ticks are never generated internally;
    /// skipping time skips obsolete gestures rather than playing them late.
    pub fn tick(&mut self, now_us: u64) -> Output {
        let _ = self.advance(now_us);
        self.sequence = match self.sequence.checked_add(1) {
            Some(value) => value,
            None => {
                self.fault = Some("expression command sequence exhausted");
                self.sequence
            }
        };
        let mut state = ExpressionState {
            stamp: ClockStamp {
                clock_id: self.clock_id.clone(),
                at_us: self.now.unwrap_or(now_us),
                sequence: self.sequence,
            },
            enabled: self.enabled,
            turn: self.voice.as_ref().map(|v| v.turn),
            request: self.voice.as_ref().map(|v| v.request),
            behaviours: Vec::new(),
            missing: Vec::new(),
            warnings: Vec::new(),
            body: false,
            light: false,
            asleep: None,
        };
        if self.catalogue.timing.body_lag_us.is_none() {
            state.missing.push("measured body lag".into());
        }
        if self.light.is_none() {
            state.missing.push("light setting".into());
        }
        if self.rejected != 0 {
            state
                .warnings
                .push(format!("{} inputs rejected", self.rejected));
        }
        if let Some(fault) = self.fault {
            self.fence_sleep_name(now_us);
            state.warnings.push(fault.into());
            return Output {
                body: None,
                light: None,
                state,
            };
        }
        let Some(voice) = self
            .voice
            .clone()
            .filter(|v| now_us - v.stamp.at_us < self.catalogue.timing.voice_stale_us)
        else {
            state.missing.push("fresh voice timing".into());
            return Output {
                body: None,
                light: None,
                state,
            };
        };
        if self.cancelled {
            state.warnings.push("voice turn cancelled".into());
            return Output {
                body: None,
                light: None,
                state,
            };
        }
        if self
            .legacy_voice
            .is_some_and(|old| old.state != voice.state)
        {
            state
                .warnings
                .push("legacy voice state differs; timed owner wins".into());
        }
        let joints_fresh = self.joints_fresh(now_us);
        if !joints_fresh {
            state.missing.push("fresh measured joints".into());
        }
        let room = self.room_at(now_us);
        if room == Room::Unknown {
            state.missing.push("fresh room state".into());
        }
        let concentrating = self.concentration.as_ref().is_some_and(|v| v.concentrating);
        let held = self.held();
        let playout = self
            .playout
            .front()
            .filter(|f| f.start_us <= now_us && now_us < f.end_us);
        let playing = playout.is_some() && !self.retired_turn;
        let recent_level = (voice.state == Voice::Speaking && !self.retired_turn)
            .then(|| self.recent_level(now_us))
            .flatten();
        let light_level = playout.map(|f| f.level).or(recent_level);
        let actual_level = light_level.unwrap_or(0.0);
        let following_light = light_level.is_some() && !self.retired_turn;
        let body_at = now_us.saturating_add(self.catalogue.timing.body_lag_us.unwrap_or(0));
        let body_playout = self
            .playout
            .iter()
            .find(|f| f.start_us <= body_at && body_at < f.end_us);
        let (body_level, body_sample_at) = if let Some(frame) = body_playout {
            (Some(frame.level), body_at)
        } else if self.catalogue.timing.recent_playout_us != 0 {
            // Without a committed future interval, this is a reaction to a
            // current/recent level at now, not an estimate of future sound.
            (light_level, now_us)
        } else {
            (None, body_at)
        };
        let body_playing = body_level.is_some() && !self.retired_turn;
        let body_level = body_level.unwrap_or(0.0);
        // The level the speaking motion meets (#933): the body's lag, when
        // the voice has said that far; otherwise the farthest piece it has
        // said that is still to end. The voice says its sound 200 ms ahead
        // (`FORETELL_US`) and the catalogues' lag is 300 ms, so no piece was
        // ever at the lag, and the head never nodded or swayed on a lamp
        // (#1759, 20 Oct). The move then lands at most the difference early.
        let speech_level = body_playout
            .or_else(|| {
                self.playout
                    .iter()
                    .filter(|f| f.start_us <= body_at && f.end_us > now_us)
                    .max_by_key(|f| f.start_us)
            })
            .map_or(0.0, |frame| frame.level);
        let mode = if playing {
            Voice::Speaking
        } else {
            voice.state
        };
        let body_mode = if body_playing { Voice::Speaking } else { mode };
        if mode == Voice::Speaking && !following_light {
            state
                .missing
                .push(if self.catalogue.timing.recent_playout_us == 0 {
                    "current actual playout level".into()
                } else {
                    "fresh current or recent playout level".into()
                });
        }
        if mode == Voice::Thinking && self.first_audio.is_none() {
            state.missing.push("scheduled first audio".into());
        }
        if self.late_schedule {
            state
                .warnings
                .push("short audio notice; return shortened, arrival unverified".into());
        }
        let activity_fresh = self
            .activity
            .as_ref()
            .is_some_and(|a| now_us - a.stamp.at_us < self.catalogue.timing.activity_stale_us);
        if mode == Voice::Listening && !activity_fresh {
            state.missing.push("fresh person activity".into());
        }
        let can_pause = mode == Voice::Listening
            && voice.accepting_input
            && !held
            && joints_fresh
            && activity_fresh
            && self.activity.as_ref().is_some_and(|a| !a.speaking);
        let (face, mut brightness) = self.face(mode, voice.accepting_input);
        // The idle layer, sleep and waking, when the catalogue has them.
        // Asleep, no clip may begin and the voice's posture gives way.
        let planned = self.life.as_mut().map(|life| {
            life.step(
                now_us,
                &alive::Context {
                    voice: Some(voice.state),
                    free: !held && !concentrating && joints_fresh,
                },
            )
        });
        if let Some(routine) = &mut self.routine {
            routine.after_tick(now_us, &mut self.life);
        }
        let asleep = planned.as_ref().is_some_and(|planned| planned.asleep);
        state.asleep = planned.as_ref().map(|planned| planned.asleep);
        let mut offsets = self.catalogue.postures.for_voice(body_mode);
        let return_scale = self.first_audio.map_or(1.0, |first| {
            let t = self.catalogue.timing;
            let back_by = first.saturating_sub(
                t.return_before_us
                    .saturating_add(t.body_lag_us.unwrap_or(0)),
            );
            ease(back_by.saturating_sub(now_us) as f64 / t.return_us as f64)
        });
        if body_mode == Voice::Thinking && self.first_audio.is_some() {
            // The base tilt returns with the authored away layer.
            offsets = blend(self.catalogue.postures.speaking, offsets, return_scale);
        }
        for (index, clip) in self.catalogue.behaviours.iter().enumerate() {
            let clip_mode = if clip.trigger == Trigger::Playout {
                body_mode
            } else {
                mode
            };
            let continuing = clip.trigger == Trigger::Pause
                && self.clips[index].started.is_some()
                && clip.continue_in.contains(&clip_mode);
            if clip.weight == 0.0
                || asleep
                || (!clip.states.contains(&clip_mode) && !continuing)
                || (clip.quiet_only && room != Room::Quiet)
                || (clip.stop_when_concentrating && concentrating)
                || (matches!(
                    clip.trigger,
                    Trigger::Pause | Trigger::Sudden | Trigger::Remarked
                ) && (held || !joints_fresh))
            {
                self.clips[index].started = None;
                continue;
            }
            if clip.trigger == Trigger::Playout {
                // Only committed intervals may anticipate the body. Recent
                // evidence fades by its original end time on the current tick.
                let start = self
                    .playout_since
                    .map(|at| at.saturating_add(clip.delay_us));
                let mut active = false;
                if body_playing
                    && let Some(elapsed) = start.and_then(|at| body_sample_at.checked_sub(at))
                    && let Some((pose, _)) = clip.sample(elapsed)
                {
                    offsets = add(offsets, pose, clip.weight * f64::from(body_level));
                    active |= body_level > 0.0;
                }
                if following_light
                    && let Some(elapsed) = start.and_then(|at| now_us.checked_sub(at))
                    && let Some((_, light)) = clip.sample(elapsed)
                {
                    brightness += light * clip.weight as f32 * actual_level;
                    active |= actual_level > 0.0;
                }
                if active {
                    state.behaviours.push(clip.name.clone());
                }
                continue;
            }
            let elapsed = match clip.trigger {
                Trigger::Thinking if mode == Voice::Thinking && !self.retired_turn => {
                    now_us.checked_sub(self.voice_since.saturating_add(clip.delay_us))
                }
                Trigger::Pause if can_pause => {
                    let runtime = &mut self.clips[index];
                    if let Some(pause) = self.pause_since {
                        let start = pause.saturating_add(clip.delay_us);
                        if runtime.pause_seen != Some(pause) && now_us >= start {
                            runtime.pause_seen = Some(pause);
                            // Skip a stroke missed by a stalled loop, rather
                            // than starting a fresh full nod after the pause.
                            if now_us <= start.saturating_add(clip.stroke_us)
                                && runtime.last_started.is_none_or(|last| {
                                    now_us.saturating_sub(last) >= clip.cooldown_us
                                })
                            {
                                runtime.started = Some(start);
                                runtime.last_started = Some(now_us);
                            }
                        }
                    }
                    runtime.started.and_then(|at| now_us.checked_sub(at))
                }
                Trigger::Pause if continuing => self.clips[index]
                    .started
                    .and_then(|at| now_us.checked_sub(at)),
                Trigger::Rest => {
                    let runtime = &mut self.clips[index];
                    if let Some(started) = runtime.started
                        && now_us >= started.saturating_add(clip.duration_us())
                    {
                        runtime.started = None;
                        runtime.last_completed = Some(started.saturating_add(clip.duration_us()));
                    }
                    if runtime.started.is_none()
                        && self
                            .rest_since
                            .is_some_and(|since| now_us.saturating_sub(since) >= clip.delay_us)
                        && runtime.last_completed.is_none_or(|completed| {
                            now_us.saturating_sub(completed) >= clip.cooldown_us
                        })
                    {
                        // Rest has no external beat to catch up with. A late
                        // tick starts one small clip now, never a backlog.
                        runtime.started = Some(now_us);
                        runtime.last_started = Some(now_us);
                    }
                    runtime.started.and_then(|at| now_us.checked_sub(at))
                }
                Trigger::Playout if playing => self
                    .playout_since
                    .and_then(|at| now_us.checked_sub(at.saturating_add(clip.delay_us))),
                Trigger::Interrupted => self
                    .interruption
                    .as_ref()
                    .and_then(|v| now_us.checked_sub(v.at_us.saturating_add(clip.delay_us))),
                Trigger::Concentrating if concentrating => Some(0),
                // A glance answers a sound or a remark now or not at all:
                // one heard by a stalled loop is let go, as a living thing
                // lets go of a sound it missed, and a second inside the
                // cooldown gets nothing.
                Trigger::Sudden => {
                    let runtime = &mut self.clips[index];
                    let seen = runtime.sudden_seen;
                    if let Some(heard) = glance_once(runtime, seen, self.sudden_since, clip, now_us)
                    {
                        runtime.sudden_seen = Some(heard);
                    }
                    runtime.started.and_then(|at| now_us.checked_sub(at))
                }
                Trigger::Remarked => {
                    let runtime = &mut self.clips[index];
                    let seen = runtime.remarked_seen;
                    if let Some(heard) =
                        glance_once(runtime, seen, self.remarked_since, clip, now_us)
                    {
                        runtime.remarked_seen = Some(heard);
                    }
                    runtime.started.and_then(|at| now_us.checked_sub(at))
                }
                _ => {
                    self.clips[index].started = None;
                    None
                }
            };
            let Some(elapsed) = elapsed else {
                continue;
            };
            let Some((pose, light)) = clip.sample(elapsed) else {
                self.clips[index].started = None;
                continue;
            };
            let weight = clip.weight
                * match clip.trigger {
                    Trigger::Thinking => return_scale,
                    Trigger::Pause if continuing => return_scale,
                    Trigger::Playout => f64::from(actual_level),
                    _ => 1.0,
                };
            offsets = add(offsets, pose, weight);
            brightness += light * weight as f32;
            if weight > 0.0 {
                state.behaviours.push(clip.name.clone());
            }
        }
        // The lamp's own speech in its head (#933), when the catalogue says
        // how: in any room, never while held, concentrating, asleep or cut in.
        if let Some(rules) = &self.catalogue.speaking {
            let speaking = mode == Voice::Speaking && !self.retired_turn && !asleep;
            let still = held || concentrating || !joints_fresh || self.interruption.is_some();
            let mut names = Vec::new();
            let motion = self
                .speech
                .step(rules, speaking, speech_level, still, now_us, &mut names);
            offsets = add(offsets, motion, 1.0);
            state
                .behaviours
                .extend(names.iter().map(|name| (*name).to_owned()));
        }
        if let Some(planned) = planned {
            offsets = add(
                add(BodyOffsets::ZERO, offsets, planned.keep),
                planned.offsets,
                1.0,
            );
            state
                .behaviours
                .extend(planned.names.iter().map(|name| (*name).to_owned()));
        }
        if let Some(interruption) = &self.interruption {
            let elapsed = now_us.saturating_sub(interruption.at_us);
            offsets = blend(
                interruption.from,
                offsets,
                ease(elapsed as f64 / self.catalogue.timing.interrupt_us as f64),
            );
            if elapsed >= self.catalogue.timing.interrupt_us {
                self.interruption = None;
            }
        }
        offsets = clamp(offsets, self.catalogue.limits);
        if concentrating {
            offsets = BodyOffsets::ZERO;
            // Stillness is also a still light: ignore all level modulation.
            brightness = self.face(mode, voice.accepting_input).1;
        }
        self.last_offsets = offsets;
        let lease = ExpressionLease {
            clock_id: self.clock_id.clone(),
            session_id: self.session_id.clone(),
            voice_session_id: voice.session_id.clone(),
            sequence: self.sequence,
            turn: voice.turn,
            request: voice.request,
            issued_us: now_us,
            apply_us: now_us,
            valid_until_us: now_us + LEASE_US,
        };
        let gaze = if !concentrating
            && (self.interruption.is_none() || held)
            && matches!(mode, Voice::Listening | Voice::Speaking)
            && (held || !self.catalogue.gaze.quiet_only || room == Room::Quiet)
        {
            self.gaze(now_us, &lease)
        } else {
            None
        };
        let body = (self.enabled && joints_fresh).then(|| ExpressionBody {
            lease: lease.clone(),
            offsets,
            hold: concentrating,
            gaze,
        });
        let light = if self.enabled {
            self.light.clone().map(|setting| ExpressionLight {
                lease,
                brightness: if setting.on {
                    brightness.clamp(0.0, 1.0)
                } else {
                    0.0
                },
                setting,
                face,
            })
        } else {
            None
        };
        state.body = body.is_some();
        state.light = light.is_some();
        Output { body, light, state }
    }

    fn face(&self, voice: Voice, ready: bool) -> (ExpressionFace, f32) {
        let levels = self.catalogue.face_levels;
        match voice {
            Voice::Resting => (ExpressionFace::Resting, levels.resting),
            Voice::Listening if ready && !self.offline => {
                (ExpressionFace::Listening, levels.listening)
            }
            Voice::Listening => (ExpressionFace::Unavailable, levels.unavailable),
            Voice::Thinking => (ExpressionFace::Thinking, levels.thinking),
            Voice::Speaking => (ExpressionFace::Speaking, levels.speaking),
        }
    }

    fn gaze(&self, now_us: u64, lease: &ExpressionLease) -> Option<ExpressionGaze> {
        let (stamp, faces) = self.faces.as_ref()?;
        let until = stamp
            .at_us
            .saturating_add(self.catalogue.timing.faces_stale_us)
            .min(lease.valid_until_us);
        if until <= now_us {
            return None;
        }
        let face = faces
            .faces
            .iter()
            .filter(|face| face.confidence >= self.catalogue.gaze.minimum_confidence)
            .max_by(|a, b| {
                let area = |f: &lamp_msgs::Face| (f.xmax - f.xmin) * (f.ymax - f.ymin);
                area(a).total_cmp(&area(b)).then_with(|| b.id.cmp(&a.id))
            })?;
        Some(ExpressionGaze {
            face_id: face.id.clone(),
            x: face.xmin + face.xmax - 1.0,
            y: face.ymin + face.ymax - 1.0,
            observed_us: stamp.at_us,
            valid_until_us: until,
        })
    }
}
