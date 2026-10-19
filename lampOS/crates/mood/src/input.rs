//! Bounded, metadata-only bus inputs. This adapter performs no I/O and keeps
//! no transcript, face list, sensor recording, or durable user history.
use crate::engine::{Event, EventKind, FaceTarget};
use lamp_bus::{Declaration, Kind, Message};
use lamp_msgs::{
    AirState, ClockStamp, ControlGesture, ControlIntent, ControlsState, Faces, HumanFamiliar,
    NameHeard, ScheduleDue, Speech, SuddenSound, Voice, VoiceState,
};
use std::{collections::BTreeSet, fmt};

pub const MAX_PAYLOAD_BYTES: usize = 16 * 1024;
pub const MAX_FACES: usize = 16;
pub const MAX_RETIRED_PUBLISHERS: usize = 16;
pub const MAX_REMINDER_IDENTITIES: usize = 128;
const TOUCH_RELEASE_TTL_US: u64 = 500_000;
// Controls closes a head session 200 ms after release, then waits 1,200 ms
// before deciding Pet. Allow that recognition delay plus bounded delivery;
// publication still expires after 500 ms and the original release is retained.
const PET_RELEASE_TTL_US: u64 = 2_000_000;

/// The wire header has a type name but no schema version. The runtime must
/// check the subscriber's declaration before calling `ingest`; that mutable
/// declaration cannot prove a version for an individual queued frame.
#[derive(Clone, Copy, Debug)]
pub struct InputSpec {
    pub topic: &'static str,
    pub type_name: &'static str,
    pub version: u32,
    pub kind: Kind,
    pub source: &'static str,
    pub max_age_ms: u64,
}

impl InputSpec {
    pub fn accepts_declaration(&self, declaration: &Declaration) -> bool {
        declaration.type_name == self.type_name
            && declaration.version == self.version
            && declaration.kind == self.kind
    }
}

pub const INPUTS: [InputSpec; 10] = [
    InputSpec {
        topic: VoiceState::TOPIC,
        type_name: VoiceState::TYPE,
        version: VoiceState::VERSION,
        kind: Kind::Latest,
        source: "voice",
        max_age_ms: 3_000,
    },
    InputSpec {
        topic: "voice/heard",
        type_name: Speech::TYPE,
        version: Speech::VERSION,
        kind: Kind::Event,
        source: "voice",
        max_age_ms: 1_500,
    },
    InputSpec {
        topic: Faces::TOPIC,
        type_name: Faces::TYPE,
        version: Faces::VERSION,
        kind: Kind::Latest,
        source: "eyes",
        max_age_ms: 2_000,
    },
    InputSpec {
        topic: ControlsState::TOPIC,
        type_name: ControlsState::TYPE,
        version: ControlsState::VERSION,
        kind: Kind::Latest,
        source: "controls",
        max_age_ms: 300,
    },
    InputSpec {
        topic: ControlGesture::TOPIC,
        type_name: ControlGesture::TYPE,
        version: ControlGesture::VERSION,
        kind: Kind::Event,
        source: "controls",
        max_age_ms: 500,
    },
    InputSpec {
        topic: SuddenSound::TOPIC,
        type_name: SuddenSound::TYPE,
        version: SuddenSound::VERSION,
        kind: Kind::Event,
        source: "ears",
        max_age_ms: 1_000,
    },
    InputSpec {
        topic: AirState::TOPIC,
        type_name: AirState::TYPE,
        version: AirState::VERSION,
        kind: Kind::Latest,
        source: "air",
        max_age_ms: 15_000,
    },
    InputSpec {
        topic: NameHeard::TOPIC,
        type_name: NameHeard::TYPE,
        version: NameHeard::VERSION,
        kind: Kind::Event,
        source: "name",
        max_age_ms: 1_500,
    },
    InputSpec {
        topic: ScheduleDue::TOPIC,
        type_name: ScheduleDue::TYPE,
        version: ScheduleDue::VERSION,
        kind: Kind::Latest,
        source: "schedule",
        max_age_ms: 3_000,
    },
    // How familiar the remembered person at the lamp is (#1670): said with
    // each recognition, every 2 s, good for 5.
    InputSpec {
        topic: HumanFamiliar::TOPIC,
        type_name: HumanFamiliar::TYPE,
        version: HumanFamiliar::VERSION,
        kind: Kind::Latest,
        source: "eyes",
        max_age_ms: 5_000,
    },
];

/// Reasons contain no payload values or words and are safe to count. A failed
/// source is optional: the runtime leaves the other inputs running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputError {
    UnknownTopic,
    InvalidHeader,
    InvalidPayload,
    Future,
    Stale,
    WrongClock,
    OutOfOrder,
    RetiredPublisher,
    PublisherFenceFull,
    NoControlsState,
    ReminderHistoryFull,
    ClockReset,
}

impl fmt::Display for InputError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "{self:?}")
    }
}

impl std::error::Error for InputError {}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputCounts {
    pub accepted: u64,
    pub ignored: u64,
    pub rejected: u64,
}

#[derive(Debug, Default)]
struct PublisherFence {
    current: Option<String>,
    retired: BTreeSet<String>,
    sequence: u64,
    wall_ms: u64,
    stamp: Option<(u64, u64)>,
    exhausted: bool,
}

impl PublisherFence {
    fn accept(
        &mut self,
        message: &Message,
        stamp: Option<&ClockStamp>,
    ) -> Result<bool, InputError> {
        if self.exhausted {
            return Err(InputError::PublisherFenceFull);
        }
        let header = &message.header;
        let changed = self.current.as_deref() != Some(&header.boot);
        if changed {
            if self.retired.contains(&header.boot) {
                return Err(InputError::RetiredPublisher);
            }
            if self.current.is_some() && self.retired.len() == MAX_RETIRED_PUBLISHERS {
                self.exhausted = true;
                return Err(InputError::PublisherFenceFull);
            }
            // A different unknown publisher must be newer, not merely a late
            // packet from an instance first seen after its replacement.
            if self.current.is_some() && header.at_unix_ms <= self.wall_ms {
                return Err(InputError::OutOfOrder);
            }
        } else if header.seq <= self.sequence || header.at_unix_ms < self.wall_ms {
            return Err(InputError::OutOfOrder);
        }
        if let (Some((previous_us, previous_sequence)), Some(stamp)) = (self.stamp, stamp)
            && (stamp.at_us < previous_us || (!changed && stamp.sequence <= previous_sequence))
        {
            return Err(InputError::OutOfOrder);
        }
        if changed && let Some(old) = self.current.replace(header.boot.clone()) {
            self.retired.insert(old);
        }
        self.sequence = header.seq;
        self.wall_ms = header.at_unix_ms;
        self.stamp = stamp.map(|stamp| (stamp.at_us, stamp.sequence));
        Ok(changed)
    }
}

#[derive(Debug)]
struct ControlsOwner {
    session: String,
    valid_until_us: u64,
    event_sequence: u64,
    stop_sequence: u64,
    pet_sequence: u64,
    ready: bool,
}

#[derive(Debug)]
enum Payload {
    Voice(VoiceState),
    Heard(bool),
    Faces(Faces),
    Controls(Box<ControlsState>),
    Gesture(ControlGesture),
    Sudden(SuddenSound),
    Air(AirState),
    Name(NameHeard),
    Schedule(ScheduleDue),
    Familiar(HumanFamiliar),
}

impl Payload {
    fn stamp(&self) -> Option<&ClockStamp> {
        match self {
            Self::Controls(value) => Some(&value.stamp),
            Self::Gesture(value) => Some(&value.stamp),
            Self::Schedule(value) => Some(&value.stamp),
            _ => None,
        }
    }
}

/// All retained collections have fixed limits. On publisher or reminder
/// identity exhaustion, fail closed until this optional service restarts.
#[derive(Debug)]
pub struct InputAdapter {
    clock_id: String,
    fences: [PublisherFence; INPUTS.len()],
    last_now_us: Option<u64>,
    clock_failed: bool,
    face: Option<FaceTarget>,
    face_until_us: u64,
    controls: Option<ControlsOwner>,
    reminders: BTreeSet<(String, u64, u64)>,
    reminders_exhausted: bool,
    counts: InputCounts,
}

impl InputAdapter {
    pub fn new(clock_id: impl Into<String>) -> Result<Self, InputError> {
        let clock_id = clock_id.into();
        if !identifier(&clock_id, 128) {
            return Err(InputError::WrongClock);
        }
        Ok(Self {
            clock_id,
            fences: std::array::from_fn(|_| PublisherFence::default()),
            last_now_us: None,
            clock_failed: false,
            face: None,
            face_until_us: 0,
            controls: None,
            reminders: BTreeSet::new(),
            reminders_exhausted: false,
            counts: InputCounts::default(),
        })
    }

    pub fn counts(&self) -> InputCounts {
        self.counts
    }

    /// Call on every transport poll, including an idle poll. Only one current
    /// face ID is kept, and it is forgotten when its metadata lease ends.
    pub fn expire(&mut self, now_us: u64) {
        if now_us >= self.face_until_us {
            self.face = None;
            self.face_until_us = 0;
        }
    }

    /// Receive one message using an explicitly sampled clock. The output is
    /// currently at most one event, and never grows with input collection size.
    /// Legacy `Header::at_us` is a private process clock and is never used.
    pub fn ingest(
        &mut self,
        message: &Message,
        now_us: u64,
        now_unix_ms: u64,
    ) -> Result<Vec<Event>, InputError> {
        self.expire(now_us);
        let result = self.ingest_inner(message, now_us, now_unix_ms);
        match &result {
            Ok(events) if events.is_empty() => {
                self.counts.ignored = self.counts.ignored.saturating_add(1);
            }
            Ok(_) => self.counts.accepted = self.counts.accepted.saturating_add(1),
            Err(_) => {
                self.counts.rejected = self.counts.rejected.saturating_add(1);
                // Do not choose a previously seen face after an invalid/stale
                // update. The engine independently expires its current lease.
                if message.header.topic == Faces::TOPIC {
                    self.face = None;
                    self.face_until_us = 0;
                }
                if message.header.topic == ControlsState::TOPIC
                    && let Some(owner) = &mut self.controls
                {
                    owner.ready = false;
                    owner.valid_until_us = 0;
                }
            }
        }
        result
    }

    fn ingest_inner(
        &mut self,
        message: &Message,
        now_us: u64,
        now_unix_ms: u64,
    ) -> Result<Vec<Event>, InputError> {
        if self.clock_failed || self.last_now_us.is_some_and(|last| now_us < last) {
            self.clock_failed = true;
            return Err(InputError::ClockReset);
        }
        self.last_now_us = Some(now_us);
        let index = INPUTS
            .iter()
            .position(|spec| spec.topic == message.header.topic)
            .ok_or(InputError::UnknownTopic)?;
        let spec = INPUTS[index];
        let (mut observed_us, mut valid_until_us) = envelope(message, spec, now_us, now_unix_ms)?;
        let payload = parse(message, index)?;
        if let Some(stamp) = payload.stamp() {
            if stamp.clock_id != self.clock_id {
                return Err(InputError::WrongClock);
            }
            if stamp.at_us == 0 || stamp.sequence == 0 {
                return Err(InputError::InvalidPayload);
            }
            if stamp.at_us > now_us {
                return Err(InputError::Future);
            }
            observed_us = stamp.at_us;
            valid_until_us = valid_until_us.min(
                stamp
                    .at_us
                    .checked_add(spec.max_age_ms * 1_000)
                    .ok_or(InputError::InvalidPayload)?,
            );
            if now_us >= valid_until_us {
                return Err(InputError::Stale);
            }
        }
        match &payload {
            Payload::Controls(value) => {
                if value.session_id != message.header.boot {
                    return Err(InputError::InvalidPayload);
                }
                if self.controls.as_ref().is_some_and(|owner| {
                    owner.session == value.session_id && value.event_sequence < owner.event_sequence
                }) {
                    return Err(InputError::OutOfOrder);
                }
            }
            Payload::Gesture(value)
                if value.enabled && !fresh_gesture(value, &self.clock_id, now_us) =>
            {
                return Err(InputError::Stale);
            }
            Payload::Name(value) if value.score >= 0.9 => {
                observed_us = onset(observed_us, value.ago_ms)?;
                valid_until_us =
                    valid_until_us.min(observed_us.saturating_add(spec.max_age_ms * 1_000));
            }
            Payload::Sudden(value) if value.short => {
                observed_us = onset(observed_us, value.ago_ms)?;
                valid_until_us =
                    valid_until_us.min(observed_us.saturating_add(spec.max_age_ms * 1_000));
            }
            _ => {}
        }
        if now_us >= valid_until_us {
            return Err(InputError::Stale);
        }
        let changed = self.fences[index].accept(message, payload.stamp())?;
        let event = match payload {
            Payload::Voice(value) => Some(EventKind::Voice(matches!(
                value.state,
                Voice::Thinking | Voice::Speaking
            ))),
            Payload::Heard(has_words) => has_words.then_some(EventKind::Heard),
            Payload::Faces(value) => {
                if changed || now_us >= self.face_until_us {
                    self.face = None;
                }
                let previous = self.face.as_ref().map(|face| face.id.as_str());
                let id = value
                    .faces
                    .iter()
                    .find(|face| Some(face.id.as_str()) == previous)
                    .or_else(|| value.faces.iter().min_by(|a, b| a.id.cmp(&b.id)))
                    .map(|face| face.id.clone());
                self.face = id.map(|id| FaceTarget {
                    publisher_boot: message.header.boot.clone(),
                    id,
                });
                self.face_until_us = valid_until_us;
                Some(EventKind::Faces(self.face.clone()))
            }
            Payload::Controls(value) => {
                let previous = self.controls.take();
                let same = previous
                    .as_ref()
                    .is_some_and(|owner| owner.session == value.session_id);
                let mut owner = if same {
                    previous.expect("same controls owner exists")
                } else {
                    ControlsOwner {
                        session: value.session_id,
                        valid_until_us,
                        event_sequence: 0,
                        stop_sequence: 0,
                        pet_sequence: 0,
                        ready: false,
                    }
                };
                owner.event_sequence = value.event_sequence;
                owner.valid_until_us = valid_until_us;
                owner.ready = value.enabled && value.ready;
                let touch = value.latest_stop.as_ref().filter(|stop| {
                    owner.ready
                        && stop.stamp.sequence > owner.stop_sequence
                        && fresh_gesture(stop, &self.clock_id, now_us)
                });
                let kind = touch.map(|stop| {
                    owner.stop_sequence = stop.stamp.sequence;
                    observed_us = stop.released_us;
                    valid_until_us =
                        valid_until_us.min(stop.released_us.saturating_add(TOUCH_RELEASE_TTL_US));
                    EventKind::Touch { kind: false }
                });
                self.controls = Some(owner);
                kind
            }
            Payload::Gesture(value) => {
                let owner = self
                    .controls
                    .as_mut()
                    .filter(|owner| {
                        owner.ready
                            && now_us < owner.valid_until_us
                            && owner.session == value.session_id
                    })
                    .ok_or(InputError::NoControlsState)?;
                // A gesture publisher has its own random boot; the payload
                // session identifies the accepted controls/state publisher.
                if !value.enabled {
                    None
                } else {
                    let (watermark, kind) = match value.intent {
                        ControlIntent::Stop => (&mut owner.stop_sequence, false),
                        ControlIntent::Pet => (&mut owner.pet_sequence, true),
                        // This request does not state whether its authoritative
                        // owner subsequently slept, woke, muted, or cleared.
                        ControlIntent::SoftwareMute => return Ok(Vec::new()),
                    };
                    if value.stamp.sequence <= *watermark {
                        None
                    } else {
                        *watermark = value.stamp.sequence;
                        observed_us = value.released_us;
                        valid_until_us = valid_until_us
                            .min(value.released_us.saturating_add(release_ttl(&value)));
                        Some(EventKind::Touch { kind })
                    }
                }
            }
            Payload::Sudden(value) => value.short.then_some(EventKind::SuddenSound),
            Payload::Air(value) => Some(EventKind::Air(value.co2_ppm)),
            Payload::Familiar(value) => Some(EventKind::Familiar(value.level)),
            Payload::Name(value) => (value.score >= 0.9).then_some(EventKind::Name),
            Payload::Schedule(value) => {
                if let Some(notice) = value.oldest {
                    // Schedule owns elapsed/trusted-wall time and its durable
                    // queue. A later wall-clock correction does not revoke an
                    // occurrence offered by its fresh, enabled, ready state.
                    let identity = (value.store_id, notice.id, notice.delivery);
                    if self.reminders.contains(&identity) {
                        None
                    } else if self.reminders_exhausted
                        || self.reminders.len() == MAX_REMINDER_IDENTITIES
                    {
                        self.reminders_exhausted = true;
                        return Err(InputError::ReminderHistoryFull);
                    } else {
                        self.reminders.insert(identity);
                        Some(EventKind::UserReminder)
                    }
                } else {
                    None
                }
            }
        };
        if now_us >= valid_until_us {
            return Err(InputError::Stale);
        }
        Ok(event
            .map(|kind| Event {
                observed_us,
                valid_until_us,
                kind,
            })
            .into_iter()
            .collect())
    }
}

fn identifier(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

fn envelope(
    message: &Message,
    spec: InputSpec,
    now_us: u64,
    now_unix_ms: u64,
) -> Result<(u64, u64), InputError> {
    let header = &message.header;
    if header.kind != spec.kind
        || header.enc != lamp_bus::JSON
        || header.type_name.as_deref() != Some(spec.type_name)
        || header.source != spec.source
        || header.boot.len() != 16
        || !header
            .boot
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || header.seq == 0
        || header.at_unix_ms == 0
        || now_unix_ms == 0
        || header.expires_ms == Some(0)
        || message.payload.len() > MAX_PAYLOAD_BYTES
    {
        return Err(InputError::InvalidHeader);
    }
    if message.expired {
        return Err(InputError::Stale);
    }
    let age_ms = now_unix_ms
        .checked_sub(header.at_unix_ms)
        .ok_or(InputError::Future)?;
    let lasts_ms = header
        .expires_ms
        .unwrap_or(spec.max_age_ms)
        .min(spec.max_age_ms);
    if age_ms >= lasts_ms {
        return Err(InputError::Stale);
    }
    let observed_us = now_us
        .checked_sub(age_ms * 1_000)
        .ok_or(InputError::InvalidHeader)?;
    let valid_until_us = observed_us
        .checked_add(lasts_ms * 1_000)
        .ok_or(InputError::InvalidHeader)?;
    Ok((observed_us, valid_until_us))
}

fn onset(published_us: u64, ago_ms: u32) -> Result<u64, InputError> {
    published_us
        .checked_sub(u64::from(ago_ms) * 1_000)
        .ok_or(InputError::Stale)
}

fn fresh_gesture(value: &ControlGesture, clock_id: &str, now_us: u64) -> bool {
    value.fresh_at(clock_id, now_us)
        && now_us
            .checked_sub(value.released_us)
            .is_some_and(|age| age < release_ttl(value))
}

fn release_ttl(value: &ControlGesture) -> u64 {
    if value.intent == ControlIntent::Pet {
        PET_RELEASE_TTL_US
    } else {
        TOUCH_RELEASE_TTL_US
    }
}

fn parse(message: &Message, index: usize) -> Result<Payload, InputError> {
    let malformed = |_| InputError::InvalidPayload;
    Ok(match index {
        0 => Payload::Voice(message.parse().map_err(malformed)?),
        1 => {
            let value: Speech = message.parse().map_err(malformed)?;
            if value
                .locale
                .as_ref()
                .is_some_and(|locale| !identifier(locale, 128))
            {
                return Err(InputError::InvalidPayload);
            }
            // Drop the entire Speech immediately. Neither words, language,
            // request identity, length, nor sentiment enter retained state.
            Payload::Heard(!value.text.trim().is_empty())
        }
        2 => {
            let value: Faces = message.parse().map_err(malformed)?;
            if value.faces.len() > MAX_FACES
                || value.check().is_err()
                || value.faces.iter().any(|face| {
                    !identifier(&face.id, 64) || face.yaw.is_some_and(|yaw| !yaw.is_finite())
                })
            {
                return Err(InputError::InvalidPayload);
            }
            let ids: BTreeSet<_> = value.faces.iter().map(|face| &face.id).collect();
            if ids.len() != value.faces.len() {
                return Err(InputError::InvalidPayload);
            }
            Payload::Faces(value)
        }
        3 => {
            let value: ControlsState = message.parse().map_err(malformed)?;
            value.check().map_err(|_| InputError::InvalidPayload)?;
            Payload::Controls(Box::new(value))
        }
        4 => {
            let value: ControlGesture = message.parse().map_err(malformed)?;
            value.check().map_err(|_| InputError::InvalidPayload)?;
            Payload::Gesture(value)
        }
        5 => {
            let value: SuddenSound = message.parse().map_err(malformed)?;
            value.check().map_err(|_| InputError::InvalidPayload)?;
            if value.loud_ms > 300 || value.short != (value.loud_ms < 200) {
                return Err(InputError::InvalidPayload);
            }
            Payload::Sudden(value)
        }
        6 => {
            let value: AirState = message.parse().map_err(malformed)?;
            value.check().map_err(|_| InputError::InvalidPayload)?;
            Payload::Air(value)
        }
        7 => {
            let value: NameHeard = message.parse().map_err(malformed)?;
            value.check().map_err(|_| InputError::InvalidPayload)?;
            Payload::Name(value)
        }
        8 => {
            let value: ScheduleDue = message.parse().map_err(malformed)?;
            value.check().map_err(|_| InputError::InvalidPayload)?;
            Payload::Schedule(value)
        }
        9 => {
            let value: HumanFamiliar = message.parse().map_err(malformed)?;
            value.check().map_err(|_| InputError::InvalidPayload)?;
            Payload::Familiar(value)
        }
        _ => return Err(InputError::UnknownTopic),
    })
}
