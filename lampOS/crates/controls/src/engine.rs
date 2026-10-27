use crate::{
    base::{BaseGestureKind, BaseRecognizer},
    input::{INPUT_FRESH_US, InputError, Observation, check_name},
};
use lamp_msgs::{
    ButtonControlState, CONTROL_MAX_BUTTONS, CONTROL_MAX_HEAD_PADS, ClockStamp, ControlFault,
    ControlGesture, ControlIntent, ControlSource, ControlsState, Gesture, HoldTier, hold_tier,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const BUTTON_DEBOUNCE_US: u64 = 200_000;
pub const DOUBLE_TAP_US: u64 = 400_000;
pub const MUTE_HOLD_US: u64 = 2_000_000;
pub const EXTENDED_HOLD_US: u64 = 5_000_000;
pub const HEAD_SETTLE_US: u64 = 500_000;
pub const HEAD_SESSION_US: u64 = 200_000;
pub const HEAD_DECISION_US: u64 = 1_200_000;
pub const HEAD_COOLDOWN_US: u64 = 1_500_000;
/// The longest press that is still a pat: a cycle with a longer one gives no
/// `Pet`. A press runs from the first pad touched until none is, so a real
/// pat on another pad while one line drifts high is part of that long press
/// and gives none either: the quiet side (Reviewer-lead's P3 on #2239). A line that drifts high at rest and back, as lamp-cb94's 97 did
/// for 77 s with no hand on it (#1764), is no pat, and Expression wakes a
/// sleeping lamp on a `Pet` (the reviewer's P2 on #2239, rule 8). As the
/// touch path's hold (`touch::HEAD_SLEEP_US`): a hand kept longer isn't
/// patting.
pub const HEAD_PAT_LONGEST_US: u64 = 3_000_000;
pub const MAX_PENDING_GESTURES: usize = 64;
pub const MAX_READER_SESSIONS: usize = 32;

/// `controls.json`. A field a newer build writes is read past and named once
/// ([`crate::forward`], #1680); a known field with a bad value is refused.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// Off unless explicitly enabled. Shadow reports never become an action.
    pub enabled: bool,
    pub buttons: Vec<String>,
    /// Known installed pads only. Learning or probing GPIO lines is not done.
    pub head_pads: Vec<String>,
    pub base_selected_mask: Option<u16>,
}

impl Config {
    pub fn check(&self) -> Result<(), InputError> {
        for (names, limit) in [
            (&self.buttons, CONTROL_MAX_BUTTONS),
            (&self.head_pads, CONTROL_MAX_HEAD_PADS),
        ] {
            if names.len() > limit {
                return Err(InputError::new(
                    ControlFault::Overflow,
                    "too many configured controls",
                ));
            }
            let mut unique = BTreeSet::new();
            for name in names {
                check_name(name)?;
                if !unique.insert(name) {
                    return Err(InputError::new(
                        ControlFault::Invalid,
                        "duplicate configured input",
                    ));
                }
            }
        }
        if let Some(mask) = self.base_selected_mask {
            BaseRecognizer::new(mask)
                .map_err(|e| InputError::new(ControlFault::Invalid, e.to_string()))?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Output {
    pub gestures: Vec<ControlGesture>,
    pub state: ControlsState,
}

#[derive(Default)]
struct Button {
    raw: Option<bool>,
    armed: bool,
    pressed_at: Option<u64>,
    last_press_edge: Option<u64>,
    last_release_edge: Option<u64>,
    last_click_release: Option<u64>,
    clicks: u8,
}

impl Button {
    fn observe(&mut self, active: bool, at_us: u64) -> Option<(Gesture, u64)> {
        let Some(previous) = self.raw.replace(active) else {
            self.armed = !active;
            return None;
        };
        if active == previous {
            return None;
        }
        if !self.armed {
            if !active {
                self.armed = true;
            }
            self.pressed_at = None;
            return None;
        }
        if active {
            if self
                .last_press_edge
                .is_some_and(|last| at_us - last < BUTTON_DEBOUNCE_US)
            {
                return None;
            }
            self.last_press_edge = Some(at_us);
            self.pressed_at = Some(at_us);
            return None;
        }
        // Even a debounced release cancels observed holding. Retaining a press
        // after a real full released sample could fabricate a later long hold.
        let pressed = self.pressed_at.take();
        if self
            .last_release_edge
            .is_some_and(|last| at_us - last < BUTTON_DEBOUNCE_US)
        {
            return None;
        }
        self.last_release_edge = Some(at_us);
        let pressed = pressed?;
        let held = at_us - pressed;
        if held >= MUTE_HOLD_US {
            self.clicks = 0;
            self.last_click_release = None;
            return Some((Gesture::LongPress, held));
        }
        self.clicks = if self
            .last_click_release
            .is_some_and(|last| at_us - last <= DOUBLE_TAP_US)
        {
            self.clicks.saturating_add(1)
        } else {
            1
        };
        self.last_click_release = Some(at_us);
        let gesture = match self.clicks {
            1 => Gesture::Tap,
            2 => Gesture::DoubleTap,
            _ => Gesture::MultiTap,
        };
        Some((gesture, held))
    }
}

#[derive(Default)]
struct Head {
    raw: u8,
    armed: bool,
    settle_until: Option<u64>,
    last_edge: Option<u64>,
    first_press: Option<u64>,
    last_release: Option<u64>,
    /// When the press now under way began, and the longest press this cycle
    /// ([`HEAD_PAT_LONGEST_US`]).
    pressed_since: Option<u64>,
    longest_press: u64,
    pending_session: bool,
    sessions: u8,
    decision_at: Option<u64>,
    cooldown_until: Option<u64>,
}

impl Head {
    fn clear_cycle(&mut self) {
        self.last_edge = None;
        self.first_press = None;
        self.last_release = None;
        self.pressed_since = None;
        self.longest_press = 0;
        self.pending_session = false;
        self.sessions = 0;
        self.decision_at = None;
    }

    fn observe(&mut self, mask: u8, at_us: u64) {
        let settle = *self.settle_until.get_or_insert(at_us + HEAD_SETTLE_US);
        if at_us < settle || !self.armed {
            self.raw = mask;
            self.armed = at_us >= settle && mask == 0;
            self.clear_cycle();
            return;
        }
        if mask == self.raw {
            return;
        }
        if mask & !self.raw != 0 {
            self.first_press.get_or_insert(at_us);
        }
        if self.raw == 0 {
            self.pressed_since = Some(at_us);
        } else if mask == 0
            && let Some(since) = self.pressed_since.take()
        {
            self.longest_press = self.longest_press.max(at_us - since);
        }
        self.raw = mask;
        self.last_edge = Some(at_us);
        self.pending_session = self.first_press.is_some();
        if mask == 0 && self.pending_session {
            self.last_release = Some(at_us);
        }
    }

    /// Known pads are pet-only. Spatial labels and early reversal classification
    /// need per-unit geometry and are deliberately not invented from pin order.
    fn due(&mut self, at_us: u64) -> Option<(u64, u64)> {
        if !self.armed || self.raw != 0 {
            return None;
        }
        if self.pending_session
            && let Some(edge) = self.last_edge
        {
            let closed = edge + HEAD_SESSION_US;
            if at_us >= closed {
                self.pending_session = false;
                if self.cooldown_until.is_some_and(|until| closed < until) {
                    self.cooldown_until = Some(closed + HEAD_COOLDOWN_US);
                    self.clear_cycle();
                    return None;
                }
                self.sessions = self.sessions.saturating_add(1);
                self.decision_at = Some(closed + HEAD_DECISION_US);
            }
        }
        if !self.pending_session && self.decision_at.is_some_and(|deadline| at_us >= deadline) {
            let result = self
                .first_press
                .zip(self.last_release)
                .filter(|_| self.longest_press <= HEAD_PAT_LONGEST_US)
                .map(|(press, release)| (release, release - press));
            self.clear_cycle();
            self.cooldown_until = Some(at_us + HEAD_COOLDOWN_US);
            return result;
        }
        None
    }
}

pub struct Engine {
    config: Config,
    clock_id: String,
    session_id: String,
    now: Option<u64>,
    last_observation: Option<(u64, u64)>,
    reader_session: Option<String>,
    retired_readers: Vec<String>,
    terminal_fault: Option<ControlFault>,
    fault: Option<ControlFault>,
    buttons: Vec<Button>,
    head: Head,
    base: Option<BaseRecognizer>,
    pending: Vec<ControlGesture>,
    latest_stop: Option<ControlGesture>,
    latest_mute: Option<ControlGesture>,
    event_sequence: u64,
    state_sequence: u64,
    rejected_inputs: u64,
    /// [`Self::set_shutdown`].
    shutdown: bool,
}

impl Engine {
    /// With the shutdown switch on (#3697), a button held 5 s or more asks
    /// for no mute here: in touch mode that hold is the touch path's
    /// shutdown ([`crate::touch::TouchEngine::set_shutdown`]).
    pub fn set_shutdown(&mut self, on: bool) {
        self.shutdown = on;
    }

    pub fn new(
        config: Config,
        clock_id: impl Into<String>,
        session_id: impl Into<String>,
    ) -> Result<Self, InputError> {
        config.check()?;
        let clock_id = clock_id.into();
        let session_id = session_id.into();
        check_name(&clock_id)?;
        check_name(&session_id)?;
        let buttons = (0..config.buttons.len())
            .map(|_| Button::default())
            .collect();
        let base = config
            .base_selected_mask
            .map(BaseRecognizer::new)
            .transpose()
            .map_err(|e| InputError::new(ControlFault::Invalid, e.to_string()))?;
        Ok(Self {
            config,
            clock_id,
            session_id,
            now: None,
            last_observation: None,
            reader_session: None,
            retired_readers: Vec::new(),
            terminal_fault: None,
            fault: Some(ControlFault::Missing),
            buttons,
            head: Head::default(),
            base,
            pending: Vec::new(),
            latest_stop: None,
            latest_mute: None,
            event_sequence: 0,
            state_sequence: 0,
            rejected_inputs: 0,
            shutdown: false,
        })
    }

    pub fn clock_id(&self) -> &str {
        &self.clock_id
    }
    pub fn config(&self) -> &Config {
        &self.config
    }

    fn clear_pending(&mut self) {
        for button in &mut self.buttons {
            *button = Button::default();
        }
        self.head = Head::default();
        if let Some(base) = &mut self.base {
            base.reset();
        }
        self.pending.clear();
    }

    fn fail(&mut self, fault: ControlFault) {
        self.fault = Some(fault);
        self.clear_pending();
    }

    fn advance(&mut self, now_us: u64) -> Result<(), InputError> {
        if self.now.is_some_and(|last| now_us < last) || now_us > u64::MAX - 3_000_000 {
            self.terminal_fault = Some(ControlFault::ClockReset);
            self.fail(ControlFault::ClockReset);
        }
        if let Some(fault) = self.terminal_fault {
            return Err(InputError::new(
                fault,
                "controls clock or identity fence cannot safely advance; create a new engine",
            ));
        }
        self.now = Some(now_us);
        if self.fault.is_none()
            && self
                .last_observation
                .is_some_and(|(at, _)| now_us - at >= INPUT_FRESH_US)
        {
            self.fail(ControlFault::Stale);
        }
        Ok(())
    }

    /// Input failure withdraws feedback and pending gestures. Already-recorded
    /// one-way Stop/Mute requests remain in status for the owner's deduplication.
    pub fn input_fault(&mut self, now_us: u64, fault: ControlFault) {
        if self.advance(now_us).is_ok() {
            self.fail(fault);
        }
        self.rejected_inputs = self.rejected_inputs.saturating_add(1);
    }

    /// Reset cannot change clock or microphone authority. The next acquisition
    /// establishes a new released baseline; an initially held input is ignored.
    pub fn reset(&mut self, now_us: u64) {
        if self.advance(now_us).is_ok() {
            self.fail(ControlFault::Missing);
        }
    }

    pub fn ingest(&mut self, now_us: u64, observation: Observation) -> Result<(), InputError> {
        let result = self.accept(now_us, observation);
        if let Err(error) = &result {
            self.rejected_inputs = self.rejected_inputs.saturating_add(1);
            self.fail(self.terminal_fault.unwrap_or(error.fault));
        }
        result
    }

    fn accept(&mut self, now_us: u64, observation: Observation) -> Result<(), InputError> {
        self.advance(now_us)?;
        observation.check()?;
        if observation.stamp.clock_id != self.clock_id {
            return Err(InputError::new(
                ControlFault::WrongClock,
                "observation comes from another boot",
            ));
        }
        let at_us = observation.stamp.at_us;
        if at_us > now_us {
            return Err(InputError::new(
                ControlFault::Future,
                "observation time is in the future",
            ));
        }
        if now_us - at_us >= INPUT_FRESH_US {
            return Err(InputError::new(
                ControlFault::Stale,
                "observation is stale on arrival",
            ));
        }
        let changed = self
            .reader_session
            .as_ref()
            .is_some_and(|old| old != &observation.session_id);
        if self.retired_readers.contains(&observation.session_id) {
            return Err(InputError::new(
                ControlFault::ReaderChanged,
                "input reader session is retired",
            ));
        }
        if let Some((last_at, last_sequence)) = self.last_observation {
            if at_us <= last_at || (!changed && observation.stamp.sequence <= last_sequence) {
                return Err(InputError::new(
                    ControlFault::OutOfOrder,
                    "source time and sequence must increase",
                ));
            }
            if !changed && last_sequence.checked_add(1) != Some(observation.stamp.sequence) {
                // A dropped full snapshot may contain release and repress.
                // Consume its watermark, but never its levels; otherwise a
                // lost short release can turn two taps into a long mute.
                self.last_observation = Some((at_us, observation.stamp.sequence));
                return Err(InputError::new(
                    ControlFault::Overflow,
                    "input acquisition sequence has a gap; establish released baseline again",
                ));
            }
            if at_us - last_at >= INPUT_FRESH_US {
                self.fail(ControlFault::Stale);
            }
        }
        let names_match = |configured: &[String], given: &[crate::input::Level]| {
            configured.len() == given.len()
                && configured
                    .iter()
                    .all(|name| given.iter().any(|level| &level.name == name))
        };
        if !names_match(&self.config.buttons, &observation.buttons)
            || !names_match(&self.config.head_pads, &observation.head_pads)
            || self.config.base_selected_mask.is_some() != observation.base_mask.is_some()
        {
            return Err(InputError::new(
                ControlFault::Missing,
                "full observation does not match configured controls",
            ));
        }
        if self.config.buttons.is_empty() && self.config.head_pads.is_empty() && self.base.is_none()
        {
            return Err(InputError::new(
                ControlFault::Missing,
                "no physical controls are configured",
            ));
        }
        if changed {
            if self.retired_readers.len() >= MAX_READER_SESSIONS {
                self.terminal_fault = Some(ControlFault::Overflow);
                return Err(InputError::new(
                    ControlFault::Overflow,
                    "input reader session fence is full; restart controls",
                ));
            }
            self.retired_readers
                .push(self.reader_session.take().unwrap());
            self.reader_session = Some(observation.session_id.clone());
            self.last_observation = Some((at_us, observation.stamp.sequence));
            return Err(InputError::new(
                ControlFault::ReaderChanged,
                "new input reader; establish released baseline again",
            ));
        }
        self.reader_session = Some(observation.session_id);
        self.last_observation = Some((at_us, observation.stamp.sequence));
        self.fault = None;
        self.head_due(at_us, now_us)?;
        for index in 0..self.buttons.len() {
            let name = &self.config.buttons[index];
            let active = observation
                .buttons
                .iter()
                .find(|level| &level.name == name)
                .unwrap()
                .active;
            if let Some((gesture, duration_us)) = self.buttons[index].observe(active, at_us) {
                // With the shutdown switch on, a hold of 5 s or more is the
                // touch path's shutdown (or its cancel), never this mute.
                if self.shutdown
                    && gesture == Gesture::LongPress
                    && duration_us >= lamp_msgs::SHUTDOWN_HOLD_US
                {
                    continue;
                }
                let name = name.clone();
                self.emit(
                    now_us,
                    ControlSource::Button,
                    name,
                    gesture,
                    at_us,
                    duration_us,
                )?;
            }
        }
        if !self.config.head_pads.is_empty() {
            let mask = self
                .config
                .head_pads
                .iter()
                .enumerate()
                .fold(0u8, |mask, (index, name)| {
                    if observation
                        .head_pads
                        .iter()
                        .any(|level| &level.name == name && level.active)
                    {
                        mask | (1 << index)
                    } else {
                        mask
                    }
                });
            self.head.observe(mask, at_us);
        }
        if let Some(base) = &mut self.base {
            let gesture = base
                .update(now_us, at_us, observation.base_mask)
                .map_err(|e| InputError::new(ControlFault::Invalid, format!("base input: {e}")))?;
            if let Some(gesture) = gesture {
                let kind = match gesture.kind {
                    BaseGestureKind::Tap => Gesture::Tap,
                    BaseGestureKind::DoubleTap => Gesture::DoubleTap,
                    BaseGestureKind::LongPress => Gesture::LongPress,
                };
                self.emit(
                    now_us,
                    ControlSource::Base,
                    "base".into(),
                    kind,
                    gesture.released_us,
                    gesture.duration_us,
                )?;
            }
        }
        Ok(())
    }

    fn emit(
        &mut self,
        now_us: u64,
        source: ControlSource,
        name: String,
        gesture: Gesture,
        released_us: u64,
        duration_us: u64,
    ) -> Result<(), InputError> {
        if self.pending.len() == MAX_PENDING_GESTURES {
            return Err(InputError::new(
                ControlFault::Overflow,
                "controls output queue is full; drain each step",
            ));
        }
        let Some(next_sequence) = self.event_sequence.checked_add(1) else {
            self.terminal_fault = Some(ControlFault::Overflow);
            return Err(InputError::new(
                ControlFault::Overflow,
                "controls event identity exhausted",
            ));
        };
        self.event_sequence = next_sequence;
        let intent = match gesture {
            Gesture::LongPress => ControlIntent::SoftwareMute,
            Gesture::Pet => ControlIntent::Pet,
            _ => ControlIntent::Stop,
        };
        let record = ControlGesture {
            stamp: ClockStamp {
                clock_id: self.clock_id.clone(),
                at_us: now_us,
                sequence: self.event_sequence,
            },
            session_id: self.session_id.clone(),
            source,
            name,
            gesture,
            intent,
            duration_us,
            released_us,
            enabled: self.config.enabled,
        };
        record
            .check()
            .map_err(|e| InputError::new(ControlFault::Invalid, e.to_string()))?;
        match intent {
            ControlIntent::Stop => self.latest_stop = Some(record.clone()),
            ControlIntent::SoftwareMute => self.latest_mute = Some(record.clone()),
            ControlIntent::Pet => {}
        }
        self.pending.push(record);
        Ok(())
    }

    fn head_due(&mut self, source_us: u64, now_us: u64) -> Result<(), InputError> {
        if let Some((released_us, duration_us)) = self.head.due(source_us) {
            self.emit(
                now_us,
                ControlSource::Headpad,
                "head".into(),
                Gesture::Pet,
                released_us,
                duration_us,
            )?;
        }
        Ok(())
    }

    pub fn tick(&mut self, now_us: u64) -> Output {
        let _ = self.advance(now_us);
        if self.fault.is_none()
            && let Err(error) = self.head_due(now_us, now_us)
        {
            self.input_fault(now_us, error.fault);
        }
        if let Some(next) = self.state_sequence.checked_add(1) {
            self.state_sequence = next;
        } else {
            self.terminal_fault = Some(ControlFault::Overflow);
            self.fail(ControlFault::Overflow);
        }
        let ready = self.fault.is_none();
        let observed_us = self.last_observation.map_or(0, |(at, _)| at);
        let buttons = self
            .config
            .buttons
            .iter()
            .zip(&self.buttons)
            .map(|(name, button)| {
                let held_us = if ready {
                    button.pressed_at.map(|at| observed_us - at)
                } else {
                    None
                };
                ButtonControlState {
                    name: name.clone(),
                    pressed: button.raw.unwrap_or(false),
                    armed: ready && button.armed,
                    tier: held_us.map_or(HoldTier::None, hold_tier),
                    held_us,
                }
            })
            .collect();
        let base_held_us = if ready {
            self.base.as_ref().and_then(|base| base.hold_us(now_us))
        } else {
            None
        };
        let state = ControlsState {
            privacy: None,
            no_privacy_slide: false,
            stamp: ClockStamp {
                clock_id: self.clock_id.clone(),
                at_us: self.now.unwrap_or(now_us),
                sequence: self.state_sequence,
            },
            session_id: self.session_id.clone(),
            enabled: self.config.enabled,
            ready,
            fault: self.fault,
            buttons,
            head_active: ready && self.head.armed && self.head.raw != 0,
            base_ready: ready && self.base.as_ref().is_some_and(BaseRecognizer::ready),
            base_held_us,
            base_tier: base_held_us.map_or(HoldTier::None, hold_tier),
            latest_stop: self.latest_stop.clone(),
            latest_mute: self.latest_mute.clone(),
            event_sequence: self.event_sequence,
            rejected_inputs: self.rejected_inputs,
        };
        Output {
            gestures: std::mem::take(&mut self.pending),
            state,
        }
    }
}
