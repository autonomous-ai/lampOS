//! Hand gestures ask the existing owners to stop, close software input, or
//! acknowledge petting. No controls message is a microphone/camera permit.
use crate::{ClockStamp, Invalid};
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const CONTROL_EVENT_TTL_US: u64 = 500_000;
pub const CONTROL_MAX_BUTTONS: usize = 8;
pub const CONTROL_MAX_HEAD_PADS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlSource {
    Button,
    Headpad,
    Base,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gesture {
    Tap,
    DoubleTap,
    MultiTap,
    LongPress,
    Pet,
}

/// One-way requests only. Stop never opens input, SoftwareMute never toggles
/// or clears a mute, and Pet never interrupts a voice turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlIntent {
    Stop,
    SoftwareMute,
    Pet,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldTier {
    None,
    Mute,
    ExtendedMute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlFault {
    Missing,
    Stale,
    Future,
    OutOfOrder,
    WrongClock,
    ReaderChanged,
    Overflow,
    Invalid,
    ClockReset,
}

/// Source edge duration is separate from publication time. Sequence is a
/// stable event identity within this controls process session; latest-state
/// delivery carries the very same record, never a newly numbered command.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlGesture {
    pub stamp: ClockStamp,
    pub session_id: String,
    pub source: ControlSource,
    pub name: String,
    pub gesture: Gesture,
    pub intent: ControlIntent,
    pub duration_us: u64,
    pub released_us: u64,
    /// False in shadow. Consumers must check this before accepting any intent.
    pub enabled: bool,
}

impl ControlGesture {
    pub const TYPE: &str = "lamp_msgs/ControlGesture";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "controls/gesture";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
            .lasting(Duration::from_micros(CONTROL_EVENT_TTL_US))
    }

    pub fn check(&self) -> Result<(), Invalid> {
        self.stamp.check()?;
        control_name(&self.session_id)?;
        control_name(&self.name)?;
        if self.stamp.sequence == 0
            || self.released_us > self.stamp.at_us
            || self.duration_us > self.released_us
        {
            return Err(Invalid(
                "a gesture needs a nonzero identity and ordered edge/publication times".into(),
            ));
        }
        let valid = match (self.source, self.gesture, self.intent) {
            (ControlSource::Headpad, Gesture::Pet, ControlIntent::Pet) => true,
            (
                ControlSource::Button | ControlSource::Base,
                Gesture::Tap | Gesture::DoubleTap | Gesture::MultiTap,
                ControlIntent::Stop,
            ) => self.duration_us < 2_000_000,
            (
                ControlSource::Button | ControlSource::Base,
                Gesture::LongPress,
                ControlIntent::SoftwareMute,
            ) => self.duration_us >= 2_000_000,
            _ => false,
        };
        if !valid {
            return Err(Invalid(
                "gesture/source/intent do not form a permitted one-way action".into(),
            ));
        }
        Ok(())
    }

    /// The receiver must also fence process sessions and deduplicate sequence.
    /// A stored SoftwareMute remains a user request after the event expires;
    /// its owner must persist an accepted mute independently of this helper.
    pub fn fresh_at(&self, clock_id: &str, now_us: u64) -> bool {
        self.check().is_ok()
            && self.enabled
            && self.stamp.clock_id == clock_id
            && self.stamp.at_us <= now_us
            && now_us - self.stamp.at_us < CONTROL_EVENT_TTL_US
            && (self.intent != ControlIntent::Stop
                || now_us - self.released_us < CONTROL_EVENT_TTL_US)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ButtonControlState {
    pub name: String,
    pub pressed: bool,
    /// A released baseline was observed after initialization or the last fault.
    pub armed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held_us: Option<u64>,
    /// Feedback only; neither tier commits an action before release.
    pub tier: HoldTier,
}

impl ButtonControlState {
    pub fn check(&self) -> Result<(), Invalid> {
        control_name(&self.name)?;
        let expected = match self.held_us {
            Some(held) if self.pressed && self.armed => hold_tier(held),
            None => HoldTier::None,
            _ => {
                return Err(Invalid(
                    "held duration needs an armed, pressed button".into(),
                ));
            }
        };
        if expected != self.tier {
            return Err(Invalid(
                "button hold tier disagrees with observed duration".into(),
            ));
        }
        Ok(())
    }
}

pub fn hold_tier(held_us: u64) -> HoldTier {
    if held_us >= 5_000_000 {
        HoldTier::ExtendedMute
    } else if held_us >= 2_000_000 {
        HoldTier::Mute
    } else {
        HoldTier::None
    }
}

/// Physical switch evidence expires independently of the Controls heartbeat.
pub const PRIVACY_SWITCH_TTL_US: u64 = 50_000;

/// An electrical observation, never microphone permission or software mute.
/// Unknown evidence has no level and names its fault. Sequence zero is reserved
/// for the unknown sentinel before any acquisition has been accepted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivacySwitch {
    pub stamp: ClockStamp,
    pub reader_session_id: String,
    pub muted: Option<bool>,
    pub fault: Option<ControlFault>,
}

impl PrivacySwitch {
    pub fn check(&self) -> Result<(), Invalid> {
        self.stamp.check()?;
        control_name(&self.reader_session_id)?;
        if self.muted.is_some() == self.fault.is_some()
            || (self.muted.is_some() && self.stamp.sequence == 0)
            || (self.stamp.sequence == 0 && self.stamp.at_us != 0)
        {
            return Err(Invalid(
                "privacy needs an acquired level or an unknown fault".into(),
            ));
        }
        Ok(())
    }

    pub fn fresh_at(&self, clock: &str, now_us: u64) -> bool {
        self.check().is_ok()
            && self.muted.is_some()
            && self.stamp.clock_id == clock
            && self.stamp.at_us <= now_us
            && now_us - self.stamp.at_us < PRIVACY_SWITCH_TTL_US
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlsState {
    pub stamp: ClockStamp,
    pub session_id: String,
    pub enabled: bool,
    /// Fresh complete source observations, not microphone readiness.
    pub ready: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fault: Option<ControlFault>,
    pub buttons: Vec<ButtonControlState>,
    pub head_active: bool,
    pub base_ready: bool,
    /// Qualified stationary contact duration at the last actual acquisition.
    /// Release grace freezes it; missing input withdraws it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_held_us: Option<u64>,
    pub base_tier: HoldTier,
    /// Latched independently: later pet activity cannot hide a missed Stop or
    /// SoftwareMute. The original event time/sequence is retained on renewal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_stop: Option<ControlGesture>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_mute: Option<ControlGesture>,
    pub event_sequence: u64,
    pub rejected_inputs: u64,
    /// Independent chip-1 evidence; absent when its explicit opt-in is off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub privacy: Option<PrivacySwitch>,
    /// Controls runs the privacy route for a lamp with no slide
    /// (`LAMP_PRIVACY_CONTROLS=no-slide`): no switch is wired, so none is
    /// read. A positive statement, never inferred from `privacy` being
    /// absent, and never with it. Not a permit: Voice grants from its own
    /// controller. Left out when false, so other states are as before.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_privacy_slide: bool,
}

impl ControlsState {
    pub const TYPE: &str = "lamp_msgs/ControlsState";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "controls/state";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_millis(100))
            .lasting(Duration::from_millis(300))
    }

    pub fn check(&self) -> Result<(), Invalid> {
        self.stamp.check()?;
        control_name(&self.session_id)?;
        if self.no_privacy_slide && self.privacy.is_some() {
            return Err(Invalid(
                "a state that says no slide is wired carries no slide evidence".into(),
            ));
        }
        if let Some(privacy) = &self.privacy {
            privacy.check()?;
            if privacy.stamp.clock_id != self.stamp.clock_id
                || privacy.stamp.at_us > self.stamp.at_us
                || (privacy.muted.is_some()
                    && !privacy.fresh_at(&self.stamp.clock_id, self.stamp.at_us))
            {
                return Err(Invalid(
                    "privacy evidence has a foreign, future or stale acquisition".into(),
                ));
            }
        }
        if self.ready == self.fault.is_some() || self.buttons.len() > CONTROL_MAX_BUTTONS {
            return Err(Invalid(
                "ready controls have no input fault and at most eight buttons".into(),
            ));
        }
        let mut names = std::collections::BTreeSet::new();
        for button in &self.buttons {
            button.check()?;
            if !names.insert(&button.name) {
                return Err(Invalid("duplicate button status name".into()));
            }
            if !self.ready && (button.armed || button.held_us.is_some()) {
                return Err(Invalid(
                    "faulted controls cannot retain armed hold feedback".into(),
                ));
            }
        }
        if !self.ready && (self.head_active || self.base_ready) {
            return Err(Invalid(
                "faulted controls cannot claim active head or ready base".into(),
            ));
        }
        let expected_base_tier = match self.base_held_us {
            Some(held) if self.base_ready && held <= self.stamp.at_us => hold_tier(held),
            None => HoldTier::None,
            _ => {
                return Err(Invalid(
                    "base hold needs a ready recognizer and an observed duration".into(),
                ));
            }
        };
        if self.base_tier != expected_base_tier {
            return Err(Invalid(
                "base hold tier disagrees with observed duration".into(),
            ));
        }
        for (record, intent) in [
            (&self.latest_stop, ControlIntent::Stop),
            (&self.latest_mute, ControlIntent::SoftwareMute),
        ] {
            if let Some(record) = record {
                record.check()?;
                if record.intent != intent
                    || record.session_id != self.session_id
                    || record.stamp.clock_id != self.stamp.clock_id
                    || record.stamp.at_us > self.stamp.at_us
                    || record.stamp.sequence > self.event_sequence
                    || record.enabled != self.enabled
                {
                    return Err(Invalid(
                        "latched controls command has a mismatched owner, intent or time".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

fn control_name(name: &str) -> Result<(), Invalid> {
    if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
        return Err(Invalid(
            "controls identity must have 1..128 bytes without controls".into(),
        ));
    }
    Ok(())
}

pub fn controls_topics() -> Vec<(&'static str, Declaration)> {
    vec![
        (ControlGesture::TOPIC, ControlGesture::declaration()),
        (ControlsState::TOPIC, ControlsState::declaration()),
    ]
}

// The opt-in touch protocol has its own topics. Adding meanings to the v1
// enums would break old readers and could change the running button policy.
pub const TOUCH_EVENT_TTL_US: u64 = 500_000;
pub const TOUCH_STATE_TTL_US: u64 = 300_000;
pub const TOUCH_INPUT_TTL_US: u64 = 100_000;
pub const TOUCH_MAX_LATEST: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TouchSource {
    Primary,
    Reset,
    Head,
    Strip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TouchGesture {
    Touch,
    Tap,
    Stroke,
    Hold,
    Slide,
}

/// A request for the final owner, never a microphone/camera permit or proof of
/// applied output. Sleep and Wake are distinct one-way lifecycle requests;
/// neither changes the separate physical or software microphone policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TouchIntent {
    Stop,
    /// Felt, and nothing asked: the owner may show it, and does nothing else
    /// (a head touch while the lamp is quiet, before it is known to be a tap).
    Notice,
    Attend,
    Stroke,
    Sleep,
    Wake,
    Slide,
    FactoryReset,
    SoftwareMute,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TouchEvent {
    pub stamp: ClockStamp,
    pub session_id: String,
    pub reader_session_id: String,
    pub enabled: bool,
    pub source: TouchSource,
    pub name: String,
    pub contact_id: u64,
    pub gesture: TouchGesture,
    /// These are alternatives, selected once using the owner's current state.
    pub when_speaking: TouchIntent,
    pub when_quiet: TouchIntent,
    /// The owner binds lifecycle state at contact begin, not again after Wake.
    /// Unknown contact-begin lifecycle history must never authorize Sleep.
    /// Physical primary/reset requests keep their same one-way meaning asleep;
    /// None means no asleep action for that advisory gesture.
    pub when_asleep: Option<TouchIntent>,
    pub began_us: u64,
    pub observed_us: u64,
    pub released_us: Option<u64>,
    /// Head bits follow configured pad-name order; strip bits are electrodes.
    pub active_mask: u16,
    pub active_names: Vec<String>,
    /// Position on the full configured strip span, 0..1; sparse working sets
    /// retain missing physical slots. Not millimetres or a guessed side.
    pub position: Option<f32>,
    /// Physical electrode at the destination end, never a guessed left/right.
    pub toward_pad: Option<u8>,
    /// Cumulative normalized travel, and normalized units per second.
    pub distance: f32,
    pub speed: f32,
}

impl TouchEvent {
    pub const TYPE: &str = "lamp_msgs/TouchEvent";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "controls/touch";
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
            .lasting(Duration::from_micros(TOUCH_EVENT_TTL_US))
    }
    pub fn intent(&self, speaking_now: bool, asleep_at_contact_start: bool) -> Option<TouchIntent> {
        if asleep_at_contact_start {
            self.when_asleep
        } else if speaking_now {
            Some(self.when_speaking)
        } else {
            Some(self.when_quiet)
        }
    }
    pub fn retained(&self) -> bool {
        !matches!(self.when_speaking, TouchIntent::Stroke | TouchIntent::Slide)
    }
    pub fn check(&self) -> Result<(), Invalid> {
        self.stamp.check()?;
        for name in [&self.session_id, &self.reader_session_id, &self.name] {
            control_name(name)?;
        }
        if self.stamp.sequence == 0
            || self.contact_id == 0
            || self.began_us > self.observed_us
            || self.observed_us > self.stamp.at_us
            || self
                .released_us
                .is_some_and(|at| at < self.began_us || at > self.observed_us)
        {
            return Err(Invalid(
                "touch event identity or acquisition times are invalid".into(),
            ));
        }
        touch_geometry(
            self.source,
            self.active_mask,
            &self.active_names,
            self.position,
            self.toward_pad,
            self.distance,
            self.speed,
        )?;
        let age = self.observed_us - self.began_us;
        let pair = (self.when_speaking, self.when_quiet);
        let valid = match (self.source, self.gesture) {
            (TouchSource::Primary, TouchGesture::Tap) => {
                pair == (TouchIntent::Stop, TouchIntent::Stop)
                    && self.active_mask == 0
                    && self.released_us == Some(self.observed_us)
                    && age < 2_000_000
            }
            (TouchSource::Primary, TouchGesture::Hold) => {
                pair == (TouchIntent::SoftwareMute, TouchIntent::SoftwareMute)
                    && self.active_mask == 0
                    && self.released_us == Some(self.observed_us)
                    && age >= 2_000_000
            }
            (TouchSource::Reset, TouchGesture::Hold) => {
                pair == (TouchIntent::FactoryReset, TouchIntent::FactoryReset)
                    && self.active_mask == 0
                    && self.released_us == Some(self.observed_us)
                    && age >= 5_000_000
            }
            // The head (#711, `docs/creature.md` "Touch"): a touch stops the
            // lamp talking at once; only a tap, let go under the stroke's
            // second (#3747), asks it to listen; a hand kept there is
            // affection, and then sleep.
            (TouchSource::Head, TouchGesture::Touch) => {
                pair == (TouchIntent::Stop, TouchIntent::Notice)
                    && age >= 20_000
                    && self.released_us.is_none()
                    && self.active_mask != 0
            }
            (TouchSource::Head, TouchGesture::Tap) => {
                pair == (TouchIntent::Attend, TouchIntent::Attend)
                    && self.active_mask == 0
                    && self.released_us.is_some_and(|released| {
                        (20_000..1_000_000).contains(&(released - self.began_us))
                    })
            }
            (TouchSource::Head, TouchGesture::Stroke) => {
                pair == (TouchIntent::Stroke, TouchIntent::Stroke)
                    && age >= 1_000_000
                    && self.released_us.is_none()
                    && self.active_mask != 0
            }
            (TouchSource::Head, TouchGesture::Hold) => {
                pair == (TouchIntent::Sleep, TouchIntent::Sleep)
                    && age >= 3_000_000
                    && self.released_us.is_none()
                    && self.active_mask != 0
            }
            (TouchSource::Strip, TouchGesture::Touch) => {
                pair == (TouchIntent::Stop, TouchIntent::Attend)
                    && age >= 10_000
                    && if self.active_mask == 0 {
                        self.released_us == Some(self.observed_us)
                    } else {
                        self.released_us.is_none()
                    }
            }
            (TouchSource::Strip, TouchGesture::Hold) => {
                pair == (TouchIntent::Sleep, TouchIntent::Sleep)
                    && age >= 2_000_000
                    && self.released_us.is_none()
                    && self.active_mask != 0
            }
            (TouchSource::Strip, TouchGesture::Stroke) => {
                pair == (TouchIntent::Stroke, TouchIntent::Stroke)
                    && self.released_us.is_none()
                    && self.active_mask != 0
            }
            (TouchSource::Strip, TouchGesture::Slide) => {
                pair == (TouchIntent::Slide, TouchIntent::Slide)
                    && self.released_us.is_none()
                    && self.active_mask != 0
                    && self.position.is_some()
                    && self.toward_pad.is_some()
            }
            _ => false,
        };
        let asleep = match (self.source, self.gesture) {
            (TouchSource::Primary, TouchGesture::Tap) => Some(TouchIntent::Stop),
            (TouchSource::Primary, TouchGesture::Hold) => Some(TouchIntent::SoftwareMute),
            (TouchSource::Reset, TouchGesture::Hold) => Some(TouchIntent::FactoryReset),
            // Asleep, a tap or a hand kept a second on the head wakes it,
            // never its first touch alone: a lone edge is noise (#3747).
            (TouchSource::Head, TouchGesture::Tap | TouchGesture::Stroke)
            | (TouchSource::Strip, TouchGesture::Touch | TouchGesture::Hold) => {
                Some(TouchIntent::Wake)
            }
            _ => None,
        };
        // For one release a head stroke may also ask nothing asleep, as an
        // older Controls says it (#3747, the Reviewer-lead on #3767): the
        // parts switch together in one round, and a newer reader never
        // refuses the older's stroke.
        let older_stroke = (self.source, self.gesture) == (TouchSource::Head, TouchGesture::Stroke)
            && self.when_asleep.is_none();
        if !valid || (self.when_asleep != asleep && !older_stroke) {
            return Err(Invalid(
                "touch source, gesture and conditional intents disagree".into(),
            ));
        }
        Ok(())
    }
    pub fn fresh_at(&self, clock_id: &str, now_us: u64) -> bool {
        self.check().is_ok()
            && self.enabled
            && self.stamp.clock_id == clock_id
            && self.stamp.at_us <= now_us
            && now_us - self.stamp.at_us < TOUCH_EVENT_TTL_US
            && now_us - self.observed_us < TOUCH_INPUT_TTL_US
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TouchSourceState {
    pub source: TouchSource,
    pub name: String,
    pub reader_session_id: Option<String>,
    /// Fresh source acquisitions, never Voice readiness.
    pub ready: bool,
    pub fault: Option<ControlFault>,
    pub observed_us: Option<u64>,
    pub armed: bool,
    /// None means unknown. A missing read is not a released level.
    pub active: Option<bool>,
    pub contact_id: Option<u64>,
    pub began_us: Option<u64>,
    /// Advances only when an actual acquisition proves continued contact.
    pub held_us: Option<u64>,
    /// Current acquired logical levels; strip bits include only configured
    /// working electrodes. Acquisitions/recordings retain the full raw mask.
    /// Motion and event masks use separately filtered evidence.
    pub active_mask: Option<u16>,
    pub active_names: Vec<String>,
    pub position: Option<f32>,
    pub toward_pad: Option<u8>,
    pub distance: f32,
    pub speed: f32,
}

impl TouchSourceState {
    pub fn check(&self, at_us: u64) -> Result<(), Invalid> {
        control_name(&self.name)?;
        if let Some(session) = &self.reader_session_id {
            control_name(session)?;
        }
        if self.ready == self.fault.is_some()
            || (self.observed_us.is_some() && self.reader_session_id.is_none())
            || self.observed_us.is_some_and(|at| at > at_us)
            || self.contact_id == Some(0)
            || self.contact_id.is_some() != self.began_us.is_some()
            || self
                .began_us
                .is_some_and(|began| self.observed_us.is_none_or(|at| began > at))
            || self.held_us.is_some_and(|held| {
                // A head hand lasts through a pulse's short released gap (#711).
                let through_gap = self.source == TouchSource::Head && self.active == Some(false);
                !self.armed
                    || (self.active != Some(true) && !through_gap)
                    || self
                        .began_us
                        .zip(self.observed_us)
                        .is_none_or(|(began, at)| held > at - began)
            })
        {
            return Err(Invalid(
                "touch source freshness or contact evidence disagrees".into(),
            ));
        }
        if self.ready {
            if self.reader_session_id.is_none()
                || self
                    .observed_us
                    .is_none_or(|at| at_us - at >= TOUCH_INPUT_TTL_US)
                || self.active_mask.is_none()
                || self.active != self.active_mask.map(|mask| mask != 0)
            {
                return Err(Invalid(
                    "ready touch source needs fresh acquired levels".into(),
                ));
            }
        } else if self.armed
            || self.active.is_some()
            || self.contact_id.is_some()
            || self.held_us.is_some()
            || self.active_mask.is_some()
            || !self.active_names.is_empty()
            || self.position.is_some()
            || self.toward_pad.is_some()
            || self.distance != 0.0
            || self.speed != 0.0
        {
            return Err(Invalid(
                "unknown touch source cannot retain contact or motion".into(),
            ));
        }
        touch_geometry(
            self.source,
            self.active_mask.unwrap_or(0),
            &self.active_names,
            self.position,
            self.toward_pad,
            self.distance,
            self.speed,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TouchState {
    pub stamp: ClockStamp,
    pub session_id: String,
    pub enabled: bool,
    pub sources: Vec<TouchSourceState>,
    /// Original identities/timestamps. Stroke/Slide never overwrite requests.
    pub latest: Vec<TouchEvent>,
    pub event_sequence: u64,
    pub rejected_inputs: u64,
}
impl TouchState {
    pub const TYPE: &str = "lamp_msgs/TouchState";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "controls/touch_state";
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_millis(100))
            .lasting(Duration::from_micros(TOUCH_STATE_TTL_US))
    }
    pub fn check(&self) -> Result<(), Invalid> {
        self.stamp.check()?;
        control_name(&self.session_id)?;
        if self.sources.len() > 4 || self.sources.is_empty() || self.latest.len() > TOUCH_MAX_LATEST
        {
            return Err(Invalid(
                "touch state exceeds its bounded source/request set".into(),
            ));
        }
        let mut sources = std::collections::BTreeSet::new();
        for source in &self.sources {
            source.check(self.stamp.at_us)?;
            if !sources.insert(source.source) {
                return Err(Invalid("duplicate touch source".into()));
            }
        }
        let mut keys = std::collections::BTreeSet::new();
        let mut sequences = std::collections::BTreeSet::new();
        for event in &self.latest {
            event.check()?;
            let source = self
                .sources
                .iter()
                .find(|source| source.source == event.source);
            if !event.retained()
                || event.session_id != self.session_id
                || event.enabled != self.enabled
                || event.stamp.clock_id != self.stamp.clock_id
                || event.stamp.at_us > self.stamp.at_us
                || event.stamp.sequence > self.event_sequence
                || source.is_none_or(|source| {
                    source.name != event.name
                        || source.reader_session_id.as_ref() != Some(&event.reader_session_id)
                })
                || !keys.insert((event.source, event.when_speaking, event.when_quiet))
                || !sequences.insert(event.stamp.sequence)
            {
                return Err(Invalid(
                    "retained touch request has a different identity or source".into(),
                ));
            }
        }
        Ok(())
    }
}

fn touch_geometry(
    source: TouchSource,
    mask: u16,
    names: &[String],
    position: Option<f32>,
    toward: Option<u8>,
    distance: f32,
    speed: f32,
) -> Result<(), Invalid> {
    if names.len() > 4
        || !distance.is_finite()
        || !(0.0..=1024.0).contains(&distance)
        || !speed.is_finite()
        || !(0.0..=1000.0).contains(&speed)
        || position.is_some_and(|p| !p.is_finite() || !(0.0..=1.0).contains(&p))
        || toward.is_some_and(|pad| pad >= 12)
    {
        return Err(Invalid(
            "touch geometry must be finite bounded normalized evidence".into(),
        ));
    }
    let mut unique = std::collections::BTreeSet::new();
    for name in names {
        control_name(name)?;
        if !unique.insert(name) {
            return Err(Invalid("duplicate active head pad".into()));
        }
    }
    let valid = match source {
        TouchSource::Head => mask & !0x0f == 0 && names.len() == mask.count_ones() as usize,
        TouchSource::Strip => mask & !0x0fff == 0 && names.is_empty(),
        TouchSource::Primary | TouchSource::Reset => mask <= 1 && names.is_empty(),
    };
    if !valid
        || (mask == 0 && (position.is_some() || toward.is_some() || speed != 0.0))
        || (source != TouchSource::Strip
            && (position.is_some() || toward.is_some() || distance != 0.0 || speed != 0.0))
    {
        return Err(Invalid(
            "touch geometry does not belong to that source".into(),
        ));
    }
    Ok(())
}

/// The pads must take turns at least this often for a pet (#3997).
pub const HEAD_PET_SWAPS: u32 = 3;
/// Within this, from the first counted swap to the [`HEAD_PET_SWAPS`]th.
pub const HEAD_PET_WITHIN_US: u64 = 1_200_000;
/// The most swaps a second a pet says: a hand can't slide faster than the
/// pads' 40 ms alone phases let it count.
pub const HEAD_PET_MOST_PER_SECOND: f32 = 25.0;

/// A hand petting the head, the pads taking turns alone (#3997, Part of
/// #4133): its own type on its own topic, `controls/pet`, so no reader of
/// `controls/touch` or `controls/touch_state` sees a new meaning. Said when
/// the pet is told apart, again while it lasts (at most ten a second) with
/// its intensity, and once more, `ended`, when the contact is let go. Only
/// with `LAMP_HEAD_PET` on in Controls; off, nothing is said here.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HeadPet {
    pub stamp: ClockStamp,
    pub session_id: String,
    pub reader_session_id: String,
    pub enabled: bool,
    /// The head contact, as `controls/touch` names it.
    pub contact_id: u64,
    pub began_us: u64,
    /// When the pet was told apart: its [`HEAD_PET_SWAPS`]th swap.
    pub petted_us: u64,
    /// The acquisition this message says.
    pub observed_us: u64,
    /// The swaps counted in the contact so far.
    pub swaps: u32,
    /// Swaps a second over the contact so far: how lively the pet is, for a
    /// light or a purr to follow.
    pub swaps_per_second: f32,
    /// The hand is still between swaps: none for a while, or let go.
    pub resting: bool,
    /// The contact ended: the last message of this pet.
    pub ended: bool,
}

impl HeadPet {
    pub const TYPE: &str = "lamp_msgs/HeadPet";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "controls/pet";
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
            .lasting(Duration::from_micros(TOUCH_EVENT_TTL_US))
    }
    pub fn check(&self) -> Result<(), Invalid> {
        self.stamp.check()?;
        for name in [&self.session_id, &self.reader_session_id] {
            control_name(name)?;
        }
        if self.stamp.sequence == 0
            || self.contact_id == 0
            || self.began_us > self.petted_us
            || self.petted_us > self.observed_us
            || self.observed_us > self.stamp.at_us
            || self.swaps < HEAD_PET_SWAPS
            || !(0.0..=HEAD_PET_MOST_PER_SECOND).contains(&self.swaps_per_second)
            || (self.ended && !self.resting)
        {
            return Err(Invalid(
                "head pet identity, times, swaps or intensity are invalid".into(),
            ));
        }
        Ok(())
    }
    pub fn fresh_at(&self, clock_id: &str, now_us: u64) -> bool {
        self.check().is_ok()
            && self.enabled
            && self.stamp.clock_id == clock_id
            && self.stamp.at_us <= now_us
            && now_us - self.stamp.at_us < TOUCH_EVENT_TTL_US
    }
}

/// Explicit opt-in inventory; the shipped v1 controls topic list is unchanged.
pub fn touch_topics() -> Vec<(&'static str, Declaration)> {
    vec![
        (TouchEvent::TOPIC, TouchEvent::declaration()),
        (TouchState::TOPIC, TouchState::declaration()),
        (HeadPet::TOPIC, HeadPet::declaration()),
    ]
}
