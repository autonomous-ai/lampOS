//! Pure, opt-in touch coordination. GPIO and strip acquisitions have separate
//! freshness, sequence and reader-session fences; a blocked I2C worker cannot
//! renew or invalidate GPIO evidence. No method reads hardware or grants input.
//!
//! Authored software target: head contact qualifies after 20 ms of acquired
//! active levels, at the next GPIO acquisition (configured cadence <=50 ms).
//! Thus qualification is <=70 ms after the first active acquisition, excluding
//! physical sampling delay and output-owner work. These are policy bounds, not
//! lamp measurements.
//!
//! The head (#711, `docs/creature.md` "Touch"): a touch qualified at 20 ms
//! asks a talking lamp to stop and a quiet one nothing; a tap let go before
//! 0.5 s asks it to listen, or to wake; a hand kept 1 s is a stroke and 3 s
//! sleep. Released gaps shorter than the configured gap join one hand. A
//! contact the body felt as a push on the head asks nothing more from then on.
//! The qualification, the shortest tap and the gap are settings
//! ([`HeadTiming`]), to be sized from a recording of the lamp's own pads.
//!
//! With `LAMP_HEAD_PET` on ([`TouchEngine::set_head_pet`], #3997, Part of
//! #4133), a hand petting the head, its pads taking turns alone, is told
//! apart and said on `controls/pet` ([`HeadPet`]), never as a gesture of
//! `controls/touch`. That contact asks no sleep and holds nothing from then
//! on ([`HoldSleeps`]). Off, nothing here changes.
use crate::{
    base::strip::{StripDirection, StripEffectKind, StripError, StripRecognizer, StripTiming},
    input::{InputError, Level, check_name},
};
use lamp_msgs::{
    ClockStamp, ControlFault, HEAD_PET_MOST_PER_SECOND, HEAD_PET_SWAPS, HEAD_PET_WITHIN_US,
    HeadPet, TOUCH_INPUT_TTL_US, TouchEvent, TouchGesture, TouchIntent, TouchSource,
    TouchSourceState, TouchState,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const HEAD_SETTLE_US: u64 = 500_000;
pub const HEAD_QUALIFY_US: u64 = 20_000;
/// A tap is let go before this, from its first acquisition: up to the
/// stroke, so no length between a tap and a stroke asks nothing (#3747, the
/// lead: nobody can tell a 400 ms touch from a 600 ms one).
pub const HEAD_TAP_US: u64 = HEAD_STROKE_US;
/// The default shortest tap: just above the lone 117 ms edge lamp-4ace's
/// journal showed with nobody known at the lamp (#711, 2026-10-15 19:25).
pub const HEAD_TAP_MIN_US: u64 = 120_000;
pub const HEAD_STROKE_US: u64 = 1_000_000;
pub const HEAD_SLEEP_US: u64 = 3_000_000;
/// The default gap: released acquisitions for less than this do not end a
/// hand, because V1's TTP223 pads can pulse every 50-100 ms under a resting
/// hand. Authored, to be set from a recording of lamp-4ace's pads (#711).
pub const HEAD_GAP_US: u64 = 150_000;
/// A head pad read by its pulses (`head_pulse_pads`): two changes of its
/// line at least this far apart, and no further than [`PULSE_GAP_US`], are a
/// pulse; closer ones are a bounce. As `discover-head`'s shortest pulse.
pub const PULSE_MIN_US: u64 = 20_000;
/// A pulse pad is touched until this long after its last pulse: a hand
/// resting on a TTP223 in its pulsing mode changes the line every 50-100 ms
/// (`docs/controls.md`). A single change with none after it, as lamp-cb94's
/// and lamp-c0a4's lines made at rest (#1764), is never a touch.
pub const PULSE_GAP_US: u64 = 250_000;

/// A head pad touched alone counts towards a pet once it has been alone
/// this long: a still hand shifting shows 10-20 ms of one pad, a pet's turns
/// on lamp-06c1 were 90 ms and longer (#3997).
pub const PET_ALONE_MIN_US: u64 = 40_000;
/// A pet's hand is resting once no swap has come for this long: a sliding
/// hand on lamp-06c1 swapped every 0.1-0.3 s.
pub const PET_RESTING_US: u64 = 400_000;
/// While a pet lasts, it is said again at most this often (ten a second).
pub const PET_EVERY_US: u64 = 100_000;

/// What a head hand kept [`HEAD_SLEEP_US`] asks with `LAMP_HEAD_PET` on
/// (#3997, #4133).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoldSleeps {
    /// Sleep, as today, unless the contact was told apart as a pet.
    PetOnly,
    /// No head contact asks sleep: #4133's design, once its owners say yes.
    Never,
}

/// The head's sleep with `LAMP_HEAD_PET` on: one line to change when
/// #4133's design is agreed on a lamp.
pub const HEAD_PET_HOLD: HoldSleeps = HoldSleeps::PetOnly;

/// The switch, read by Controls when it serves the head (#3997).
pub const HEAD_PET_ENV: &str = "LAMP_HEAD_PET";

/// What `LAMP_HEAD_PET` asks: [`HEAD_PET_HOLD`] when it is exactly `on`,
/// and nothing otherwise, unset as on every lamp today.
pub fn head_pet(value: Option<&str>) -> Option<HoldSleeps> {
    (value == Some("on")).then_some(HEAD_PET_HOLD)
}

/// The head's timing, from the touch pins file (`head_qualify_ms`,
/// `head_tap_min_ms`, `head_gap_ms`), each defaulting to the constants above.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeadTiming {
    /// An unbroken active run this long qualifies a touch (Stop if speaking).
    pub qualify_us: u64,
    /// A tap is let go no sooner than this after its first acquisition.
    pub tap_min_us: u64,
    /// Released acquisitions shorter than this do not end a hand.
    pub gap_us: u64,
}

impl Default for HeadTiming {
    fn default() -> Self {
        Self {
            qualify_us: HEAD_QUALIFY_US,
            tap_min_us: HEAD_TAP_MIN_US,
            gap_us: HEAD_GAP_US,
        }
    }
}
pub const BUTTON_DEBOUNCE_US: u64 = 200_000;
pub const PRIMARY_HOLD_US: u64 = 2_000_000;
pub const RESET_HOLD_US: u64 = 5_000_000;
/// The reset button arms only after it has read released this long running:
/// a line that rests pressed (a pad driving it low) and lifts briefly never
/// arms, so two short lifts 5 s apart are never a reset (the reviewer on
/// #1167). A line read pressed through all of its first second never arms
/// in that run (the reviewer on #1176 and #1209). The primary button arms on
/// its first released read, as before.
pub const RESET_ARM_US: u64 = 1_000_000;
pub const MAX_PENDING_EVENTS: usize = 64;
pub const MAX_RETIRED_READERS: usize = 32;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TouchConfig {
    pub enabled: bool,
    pub primary: Option<String>,
    pub reset: Option<String>,
    pub head_pads: Vec<String>,
    /// The ordered 3..12 working electrodes selected for this lamp. Excluded
    /// electrodes remain in raw acquisitions but never count as contact.
    pub strip_pads: Option<Vec<u8>>,
    /// The head's timing; each absent value is its default ([`HeadTiming`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_qualify_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_tap_min_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_gap_ms: Option<u64>,
    /// The strip's timing; each absent value is its default ([`StripTiming`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strip_qualify_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strip_release_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strip_stroke_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strip_sleep_ms: Option<u64>,
    /// The head pads read by their pulses, not their level, as each lamp's
    /// factory step measured them (the pins file's `head_rest`); the others
    /// read their level, as before.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub head_pulse_pads: Vec<String>,
}

impl TouchConfig {
    pub fn check(&self) -> Result<(), InputError> {
        if self.head_pads.len() > 4 {
            return Err(error(
                ControlFault::Overflow,
                "at most four head pads are supported",
            ));
        }
        let mut names = BTreeSet::new();
        for name in self
            .primary
            .iter()
            .chain(&self.reset)
            .chain(&self.head_pads)
        {
            check_name(name)?;
            if !names.insert(name) {
                return Err(error(
                    ControlFault::Invalid,
                    "touch input names must be unique",
                ));
            }
        }
        let mut pulse_pads = BTreeSet::new();
        for name in &self.head_pulse_pads {
            if !self.head_pads.contains(name) || !pulse_pads.insert(name) {
                return Err(error(
                    ControlFault::Invalid,
                    "head pulse pads must be distinct configured head pads",
                ));
            }
        }
        if let Some(pads) = &self.strip_pads {
            StripRecognizer::new(pads).map_err(strip_error)?;
        }
        let strip = self.strip_timing();
        if !(10_000..=200_000).contains(&strip.qualify_us)
            || !(10_000..=500_000).contains(&strip.release_us)
            || !(100_000..=2_000_000).contains(&strip.stroke_us)
            || !(2_000_000..=5_000_000).contains(&strip.sleep_us)
        {
            return Err(error(
                ControlFault::Invalid,
                "strip timing: qualify 10..200 ms, release 10..500 ms, stroke 100..2000 ms, sleep 2000..5000 ms",
            ));
        }
        if names.is_empty() && self.strip_pads.is_none() {
            return Err(error(
                ControlFault::Missing,
                "at least one touch input is required",
            ));
        }
        let timing = self.head_timing();
        if !(20_000..=200_000).contains(&timing.qualify_us)
            // Its old range: the tap's window grew to the stroke (#3747), not
            // how short a tap may be set.
            || !(timing.qualify_us..500_000).contains(&timing.tap_min_us)
            || timing.gap_us > 500_000
        {
            return Err(error(
                ControlFault::Invalid,
                "head timing: qualify 20..200 ms, shortest tap from qualify to under 500 ms, gap 0..500 ms",
            ));
        }
        Ok(())
    }
    pub fn head_timing(&self) -> HeadTiming {
        let default = HeadTiming::default();
        let us = |ms: Option<u64>, or: u64| ms.map_or(or, |ms| ms.saturating_mul(1_000));
        HeadTiming {
            qualify_us: us(self.head_qualify_ms, default.qualify_us),
            tap_min_us: us(self.head_tap_min_ms, default.tap_min_us),
            gap_us: us(self.head_gap_ms, default.gap_us),
        }
    }
    pub fn strip_timing(&self) -> StripTiming {
        let default = StripTiming::default();
        let us = |ms: Option<u64>, or: u64| ms.map_or(or, |ms| ms.saturating_mul(1_000));
        StripTiming {
            qualify_us: us(self.strip_qualify_ms, default.qualify_us),
            release_us: us(self.strip_release_ms, default.release_us),
            stroke_us: us(self.strip_stroke_ms, default.stroke_us),
            sleep_us: us(self.strip_sleep_ms, default.sleep_us),
        }
    }
    fn has_gpio(&self) -> bool {
        self.primary.is_some() || self.reset.is_some() || !self.head_pads.is_empty()
    }
}

/// A complete GPIO-only acquisition. Logical polarity is configured by the
/// adapter; neither a privacy switch nor cached strip values belong here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GpioSample {
    pub stamp: ClockStamp,
    pub session_id: String,
    pub primary: Option<bool>,
    pub reset: Option<bool>,
    pub head_pads: Vec<Level>,
}

/// One successful full twelve-electrode acquisition. Failed or unconfigured
/// reads must call strip_fault, never synthesize a released zero mask.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StripSample {
    pub stamp: ClockStamp,
    pub session_id: String,
    pub mask: u16,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TouchOutput {
    pub events: Vec<TouchEvent>,
    pub state: TouchState,
    /// `controls/pet`, only with `LAMP_HEAD_PET` on: left out when empty, so
    /// a replay's frames are as before.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pets: Vec<HeadPet>,
}

fn error(fault: ControlFault, message: &'static str) -> InputError {
    InputError::new(fault, message)
}
fn strip_error(value: StripError) -> InputError {
    let fault = match value {
        StripError::InvalidOrder | StripError::InvalidMask | StripError::MotionBound => {
            ControlFault::Invalid
        }
        StripError::UnknownSample => ControlFault::Missing,
        StripError::ClockBackwards => ControlFault::ClockReset,
        StripError::ObservationOutOfOrder => ControlFault::OutOfOrder,
        StripError::FutureSample => ControlFault::Future,
        StripError::StaleSample | StripError::SampleGap => ControlFault::Stale,
        StripError::ContactExhausted => ControlFault::Overflow,
    };
    InputError::new(fault, value.to_string())
}

struct Fence {
    session: Option<String>,
    retired: BTreeSet<String>,
    last: Option<(u64, u64)>,
    fault: Option<ControlFault>,
    exhausted: bool,
}
impl Default for Fence {
    fn default() -> Self {
        Self {
            session: None,
            retired: BTreeSet::new(),
            last: None,
            fault: Some(ControlFault::Missing),
            exhausted: false,
        }
    }
}
impl Fence {
    fn accept(
        &mut self,
        now: u64,
        stamp: &ClockStamp,
        session: &str,
        clock: &str,
    ) -> Result<(), InputError> {
        stamp
            .check()
            .map_err(|_| error(ControlFault::Invalid, "invalid touch acquisition clock"))?;
        check_name(session)?;
        if self.exhausted {
            return Err(error(
                ControlFault::Overflow,
                "touch reader identity fence exhausted",
            ));
        }
        if stamp.clock_id != clock {
            return Err(error(
                ControlFault::WrongClock,
                "touch acquisition uses another boot clock",
            ));
        }
        if stamp.sequence == 0 {
            return Err(error(
                ControlFault::Invalid,
                "touch acquisition sequence must be nonzero",
            ));
        }
        if stamp.at_us > now {
            return Err(error(
                ControlFault::Future,
                "touch acquisition is after receipt",
            ));
        }
        if now - stamp.at_us >= TOUCH_INPUT_TTL_US {
            return Err(error(
                ControlFault::Stale,
                "touch acquisition is at least 100 ms old",
            ));
        }
        if self.session.as_deref() != Some(session) {
            if self.retired.contains(session) {
                return Err(error(
                    ControlFault::ReaderChanged,
                    "retired touch reader cannot return",
                ));
            }
            if self.last.is_some_and(|(at, _)| stamp.at_us <= at) {
                return Err(error(
                    ControlFault::OutOfOrder,
                    "new touch reader predates accepted acquisition",
                ));
            }
            if let Some(previous) = &self.session {
                if self.retired.len() == MAX_RETIRED_READERS {
                    self.exhausted = true;
                    return Err(error(
                        ControlFault::Overflow,
                        "touch reader identity fence exhausted",
                    ));
                }
                self.retired.insert(previous.clone());
                self.session = Some(session.to_owned());
                self.last = Some((stamp.at_us, stamp.sequence));
                return Err(error(
                    ControlFault::ReaderChanged,
                    "new reader needs a new released baseline",
                ));
            }
            self.session = Some(session.to_owned());
            self.last = Some((stamp.at_us, stamp.sequence));
            self.fault = None;
            return Ok(());
        }
        if let Some((at, sequence)) = self.last {
            if stamp.at_us <= at || stamp.sequence <= sequence {
                return Err(error(
                    ControlFault::OutOfOrder,
                    "touch acquisition did not advance",
                ));
            }
            // A lost complete acquisition may conceal release/repress. Consume
            // its watermark, but require a later released sample to recover.
            self.last = Some((stamp.at_us, stamp.sequence));
            if sequence.checked_add(1) != Some(stamp.sequence) {
                return Err(error(
                    ControlFault::Overflow,
                    "touch acquisition sequence gap",
                ));
            }
            if stamp.at_us - at >= TOUCH_INPUT_TTL_US {
                return Err(error(
                    ControlFault::Stale,
                    "touch acquisition gap is at least 100 ms",
                ));
            }
        }
        self.last = Some((stamp.at_us, stamp.sequence));
        self.fault = None;
        Ok(())
    }
    fn observed(&self) -> Option<u64> {
        self.last.map(|(at, _)| at)
    }
    fn expired(&self, now: u64) -> bool {
        self.fault.is_none()
            && self
                .observed()
                .is_some_and(|at| now - at >= TOUCH_INPUT_TTL_US)
    }
}

#[derive(Clone, Copy)]
struct Contact {
    id: u64,
    began: u64,
}

#[derive(Default)]
struct Button {
    raw: Option<bool>,
    armed: bool,
    contact: Option<Contact>,
    counter: u64,
    press_edge: Option<u64>,
    release_edge: Option<u64>,
    /// How long it must read released running before it arms (0: at once).
    arm_after_us: u64,
    /// Since when it has read released, while not yet armed.
    released_since: Option<u64>,
    /// When it was first read in this run, and, once known, whether it read
    /// pressed through all of its first `arm_after_us`. Both kept through
    /// [`Button::reset`].
    first_us: Option<u64>,
    pressed_at_start: Option<bool>,
}
impl Button {
    fn arming_after(arm_after_us: u64) -> Self {
        Self {
            arm_after_us,
            ..Self::default()
        }
    }
    fn reset(&mut self) {
        *self = Self {
            counter: self.counter,
            arm_after_us: self.arm_after_us,
            first_us: self.first_us,
            pressed_at_start: self.pressed_at_start,
            ..Self::default()
        };
    }
    /// A button that arms only after a released while (the reset), read
    /// pressed through all of that first while: it never arms in this run.
    /// One released read in it is enough, so a line still charging through
    /// its pull-up, or a hand that leaves the button within that second, is
    /// not refused (the reviewer on #1209).
    fn refused(&self) -> bool {
        self.pressed_at_start == Some(true)
    }
    fn observe(&mut self, active: bool, at: u64) -> Result<Option<Contact>, InputError> {
        let previous = self.raw.replace(active);
        if self.arm_after_us > 0 && self.pressed_at_start.is_none() {
            let first = *self.first_us.get_or_insert(at);
            if !active {
                self.pressed_at_start = Some(false);
            } else if at.saturating_sub(first) >= self.arm_after_us {
                self.pressed_at_start = Some(true);
            }
        }
        if self.refused() {
            return Ok(None);
        }
        if !self.armed {
            // Released long enough running, it arms; pressed, it starts over.
            self.contact = None;
            self.released_since = if active {
                None
            } else {
                Some(self.released_since.unwrap_or(at))
            };
            self.armed = self
                .released_since
                .is_some_and(|since| at.saturating_sub(since) >= self.arm_after_us);
            return Ok(None);
        }
        let Some(previous) = previous else {
            return Ok(None);
        };
        if previous == active {
            return Ok(None);
        }
        if active {
            if self
                .press_edge
                .is_some_and(|last| at - last < BUTTON_DEBOUNCE_US)
            {
                return Ok(None);
            }
            self.press_edge = Some(at);
            self.counter = self
                .counter
                .checked_add(1)
                .ok_or_else(|| error(ControlFault::Overflow, "touch contact identity exhausted"))?;
            self.contact = Some(Contact {
                id: self.counter,
                began: at,
            });
            return Ok(None);
        }
        let contact = self.contact.take();
        if self
            .release_edge
            .is_some_and(|last| at - last < BUTTON_DEBOUNCE_US)
        {
            return Ok(None);
        }
        self.release_edge = Some(at);
        Ok(contact)
    }
}

#[derive(Default)]
struct Head {
    mask: u16,
    /// The pads read touched from the first acquisition on, never released
    /// since: left out until each reads released, so one line stuck active
    /// (a pad resting high read as active-high) no longer keeps the whole
    /// head from arming (#1798). None before the first acquisition.
    from_start: Option<u16>,
    /// Every head pad's bit: with all of them left out, the head is not
    /// armed, as nothing on it can count.
    all: u16,
    armed: bool,
    settle_start: Option<u64>,
    contact: Option<Contact>,
    counter: u64,
    /// The start of the current unbroken run of active acquisitions, which
    /// alone can qualify a touch, and the last active acquisition.
    run_began: Option<u64>,
    last_active: u64,
    /// The first released acquisition since `last_active`.
    released: Option<u64>,
    touch_sent: bool,
    stroke_sent: bool,
    hold_sent: bool,
    /// The body is feeling the head pushed by hand now ([`TouchEngine::set_head_pushed`]).
    pushed: bool,
    /// This contact overlapped a push: it asks nothing more.
    spoiled: bool,
    timing: HeadTiming,
    /// `LAMP_HEAD_PET`: none when off ([`TouchEngine::set_head_pet`]).
    pet: Option<HoldSleeps>,
    /// This contact's pet, as [`Pet`] counts it.
    petting: Pet,
    /// What the pet says from the last acquisition, for the engine to take.
    pet_due: Option<PetNote>,
}

/// One contact's pads taking turns (#3997).
#[derive(Clone, Copy, Default)]
struct Pet {
    /// The pad touched alone now, and since when.
    alone: Option<(u16, u64)>,
    /// The last pad counted alone.
    last_alone: Option<u16>,
    swaps: u32,
    /// The last [`HEAD_PET_SWAPS`] swaps' times, the oldest first.
    recent: [u64; HEAD_PET_SWAPS as usize],
    /// When it was told apart as a pet, and when last said.
    petted: Option<u64>,
    said: u64,
}

/// What the pet says of one acquisition.
#[derive(Clone, Copy)]
struct PetNote {
    contact: Contact,
    petted: u64,
    observed: u64,
    swaps: u32,
    per_second: f32,
    resting: bool,
    ended: bool,
}

/// What one head acquisition produced, and the release it confirmed, if any.
type HeadEffect = (Contact, TouchGesture, Option<u64>);

impl Head {
    fn reset(&mut self) {
        *self = Self {
            counter: self.counter,
            pushed: self.pushed,
            timing: self.timing,
            all: self.all,
            pet: self.pet,
            ..Self::default()
        };
    }
    /// Whether a hand kept [`HEAD_SLEEP_US`] asks sleep: always with the pet
    /// off, as before.
    fn hold_sleeps(&self) -> bool {
        match self.pet {
            None => true,
            Some(HoldSleeps::PetOnly) => self.petting.petted.is_none(),
            Some(HoldSleeps::Never) => false,
        }
    }
    /// Counts the pads taking turns alone in this contact, once a pad has
    /// been alone [`PET_ALONE_MIN_US`]; released and both-pad acquisitions
    /// between them do not break a turn (A, both, B is one swap). A pet is
    /// [`HEAD_PET_SWAPS`] of them within [`HEAD_PET_WITHIN_US`] of a contact
    /// whose touch counted.
    fn count_turns(&mut self, mask: u16, at: u64) {
        let pet = &mut self.petting;
        match (mask.is_power_of_two().then_some(mask), pet.alone) {
            (Some(bit), Some((alone, _))) if alone == bit => {}
            (Some(bit), _) => pet.alone = Some((bit, at)),
            (None, _) => pet.alone = None,
        }
        let Some((bit, since)) = pet.alone else {
            return;
        };
        if at - since < PET_ALONE_MIN_US || pet.last_alone == Some(bit) {
            return;
        }
        let swapped = pet.last_alone.is_some();
        pet.last_alone = Some(bit);
        if !swapped {
            return;
        }
        pet.swaps = pet.swaps.saturating_add(1);
        pet.recent.rotate_left(1);
        pet.recent[pet.recent.len() - 1] = at;
        if pet.petted.is_none()
            && self.touch_sent
            && pet.swaps >= HEAD_PET_SWAPS
            && at - pet.recent[0] <= HEAD_PET_WITHIN_US
        {
            pet.petted = Some(at);
        }
    }
    /// What the pet says at `at`: when it is told apart, at most every
    /// [`PET_EVERY_US`] while it lasts, and `ended` once let go.
    fn say_pet(&mut self, contact: Contact, at: u64, ended: bool) {
        let pet = &mut self.petting;
        let Some(petted) = pet.petted else {
            return;
        };
        if !ended && petted != at && at - pet.said < PET_EVERY_US {
            return;
        }
        pet.said = at;
        let last = pet.recent[pet.recent.len() - 1];
        let seconds = (at - contact.began) as f32 / 1e6;
        self.pet_due = Some(PetNote {
            contact,
            petted,
            observed: at,
            swaps: pet.swaps,
            per_second: (pet.swaps as f32 / seconds).min(HEAD_PET_MOST_PER_SECOND),
            resting: ended || at - last >= PET_RESTING_US,
            ended,
        });
    }
    fn end(&mut self) {
        self.contact = None;
        self.run_began = None;
        self.released = None;
    }
    fn observe(&mut self, raw: u16, at: u64) -> Result<Vec<HeadEffect>, InputError> {
        let from_start = self.from_start.get_or_insert(raw);
        *from_start &= raw;
        let left_out = *from_start;
        let mask = raw & !left_out;
        let started = *self.settle_start.get_or_insert(at);
        self.mask = mask;
        if at - started < HEAD_SETTLE_US || !self.armed {
            self.armed = at - started >= HEAD_SETTLE_US && mask == 0 && left_out != self.all;
            self.end();
            return Ok(Vec::new());
        }
        let Some(contact) = self.contact else {
            if mask == 0 {
                return Ok(Vec::new());
            }
            self.counter = self
                .counter
                .checked_add(1)
                .ok_or_else(|| error(ControlFault::Overflow, "touch contact identity exhausted"))?;
            self.contact = Some(Contact {
                id: self.counter,
                began: at,
            });
            (self.touch_sent, self.stroke_sent, self.hold_sent) = (false, false, false);
            self.petting = Pet::default();
            self.spoiled = self.pushed;
            self.run_began = Some(at);
            self.last_active = at;
            return Ok(Vec::new());
        };
        self.spoiled |= self.pushed;
        if mask == 0 {
            self.run_began = None;
            let released = *self.released.get_or_insert(at);
            if at - self.last_active < self.timing.gap_us {
                self.say_pet(contact, at, false);
                return Ok(Vec::new());
            }
            // Let go: only a short, qualified, unspoiled contact was a tap,
            // and never a pet, however quick.
            let tap = self.touch_sent
                && !self.stroke_sent
                && !self.spoiled
                && self.petting.petted.is_none()
                && (self.timing.tap_min_us..HEAD_TAP_US).contains(&(released - contact.began));
            self.say_pet(contact, at, true);
            self.end();
            return Ok(if tap {
                vec![(contact, TouchGesture::Tap, Some(released))]
            } else {
                Vec::new()
            });
        }
        let run_began = *self.run_began.get_or_insert(at);
        self.last_active = at;
        self.released = None;
        let mut effects = Vec::with_capacity(3);
        if self.spoiled {
            return Ok(effects);
        }
        if !self.touch_sent && at - run_began >= self.timing.qualify_us {
            self.touch_sent = true;
            effects.push((contact, TouchGesture::Touch, None));
        }
        if self.pet.is_some() {
            self.count_turns(mask, at);
            self.say_pet(contact, at, false);
        }
        let held = at - contact.began;
        if self.touch_sent && !self.stroke_sent && held >= HEAD_STROKE_US {
            self.stroke_sent = true;
            effects.push((contact, TouchGesture::Stroke, None));
        }
        if self.touch_sent && !self.hold_sent && held >= HEAD_SLEEP_US && self.hold_sleeps() {
            self.hold_sent = true;
            effects.push((contact, TouchGesture::Hold, None));
        }
        Ok(effects)
    }
    /// How long the hand has been there, through short gaps; none for a
    /// contact a push spoiled, which counts towards nothing, nor once it is
    /// a pet, which counts towards no sleep (#3997: no countdown, no sleep
    /// breath).
    fn held(&self, observed: u64) -> Option<u64> {
        let contact = self.contact?;
        (!self.spoiled && self.touch_sent && self.petting.petted.is_none())
            .then(|| observed - contact.began)
    }
}

/// One head pad read by its pulses: touched while its line keeps changing.
#[derive(Clone, Copy, Debug, Default)]
struct PulsePad {
    bit: u16,
    level: Option<bool>,
    last_change: Option<u64>,
    touched_until: u64,
}

impl PulsePad {
    fn read(&mut self, active: bool, at: u64) -> bool {
        if self.level.replace(active).is_some_and(|was| was != active) {
            if self
                .last_change
                .is_some_and(|last| (PULSE_MIN_US..=PULSE_GAP_US).contains(&(at - last)))
            {
                self.touched_until = at + PULSE_GAP_US;
            }
            self.last_change = Some(at);
        }
        at < self.touched_until
    }
    fn reset(&mut self) {
        *self = Self {
            bit: self.bit,
            ..Self::default()
        };
    }
}

pub struct TouchEngine {
    config: TouchConfig,
    clock_id: String,
    session_id: String,
    now: Option<u64>,
    terminal: bool,
    gpio: Fence,
    strip_fence: Fence,
    primary: Button,
    reset: Button,
    head: Head,
    pulse_pads: Vec<PulsePad>,
    strip: Option<StripRecognizer>,
    strip_selected_mask: u16,
    /// The unmasked, successful twelve-bit acquisition, not inferred levels.
    strip_mask: u16,
    pending: Vec<TouchEvent>,
    pending_pets: Vec<HeadPet>,
    latest: Vec<TouchEvent>,
    event_sequence: u64,
    pet_sequence: u64,
    state_sequence: u64,
    rejected_inputs: u64,
}
impl TouchEngine {
    pub fn new(
        config: TouchConfig,
        clock_id: impl Into<String>,
        session_id: impl Into<String>,
    ) -> Result<Self, InputError> {
        config.check()?;
        let clock_id = clock_id.into();
        let session_id = session_id.into();
        check_name(&clock_id)?;
        check_name(&session_id)?;
        let strip = config
            .strip_pads
            .as_ref()
            .map(|pads| {
                StripRecognizer::with_timing(pads, config.strip_timing()).map_err(strip_error)
            })
            .transpose()?;
        let strip_selected_mask = config
            .strip_pads
            .iter()
            .flatten()
            .fold(0, |mask, &pad| mask | (1 << pad));
        let head = Head {
            timing: config.head_timing(),
            all: (0..config.head_pads.len()).fold(0, |all, index| all | 1 << index),
            ..Head::default()
        };
        let pulse_pads = config
            .head_pads
            .iter()
            .enumerate()
            .filter(|(_, name)| config.head_pulse_pads.contains(name))
            .map(|(index, _)| PulsePad {
                bit: 1 << index,
                ..PulsePad::default()
            })
            .collect();
        Ok(Self {
            config,
            clock_id,
            session_id,
            now: None,
            terminal: false,
            gpio: Fence::default(),
            strip_fence: Fence::default(),
            primary: Button::default(),
            reset: Button::arming_after(RESET_ARM_US),
            head,
            pulse_pads,
            strip,
            strip_selected_mask,
            strip_mask: 0,
            pending: Vec::new(),
            pending_pets: Vec::new(),
            latest: Vec::new(),
            event_sequence: 0,
            pet_sequence: 0,
            state_sequence: 0,
            rejected_inputs: 0,
        })
    }
    /// Whether the body feels the head being pushed by hand now. A head
    /// contact that overlaps a push asks for nothing more (#711).
    pub fn set_head_pushed(&mut self, pushed: bool) {
        self.head.pushed = pushed;
    }
    /// `LAMP_HEAD_PET` (#3997): with `Some`, a hand petting the head is said
    /// on `controls/pet`, and a kept hand's sleep is as `hold` says. With
    /// `None`, the default, the head is exactly as before.
    pub fn set_head_pet(&mut self, hold: Option<HoldSleeps>) {
        self.head.pet = hold;
    }
    /// Whether the reset line read pressed through all of its first second,
    /// so never arms until Controls starts again: a line that rests pressed
    /// is not a reset button (the reviewer on #1176).
    pub fn reset_refused(&self) -> bool {
        self.reset.refused()
    }
    pub fn config(&self) -> &TouchConfig {
        &self.config
    }

    fn fail_gpio(&mut self, fault: ControlFault) {
        self.gpio.fault = Some(fault);
        self.primary.reset();
        self.reset.reset();
        self.head.reset();
        self.pulse_pads.iter_mut().for_each(PulsePad::reset);
        self.pending_pets.clear();
        self.pending
            .retain(|event| event.source == TouchSource::Strip);
        self.latest
            .retain(|event| event.source == TouchSource::Strip);
    }
    fn fail_strip(&mut self, fault: ControlFault) {
        self.strip_fence.fault = Some(fault);
        if let Some(strip) = &mut self.strip {
            strip.reset();
        }
        self.strip_mask = 0;
        self.pending
            .retain(|event| event.source != TouchSource::Strip);
        self.latest
            .retain(|event| event.source != TouchSource::Strip);
    }
    fn advance(&mut self, now: u64) -> Result<(), InputError> {
        if self.now.is_some_and(|last| now < last) {
            self.terminal = true;
            self.fail_gpio(ControlFault::ClockReset);
            self.fail_strip(ControlFault::ClockReset);
        }
        if self.terminal {
            return Err(error(
                ControlFault::ClockReset,
                "touch engine clock or identity is exhausted; restart required",
            ));
        }
        self.now = Some(now);
        if self.gpio.expired(now) {
            self.fail_gpio(ControlFault::Stale);
        }
        if self.strip_fence.expired(now) {
            self.fail_strip(ControlFault::Stale);
        }
        Ok(())
    }
    fn rejected(&mut self) {
        self.rejected_inputs = self.rejected_inputs.saturating_add(1);
    }
    pub fn gpio_fault(&mut self, now_us: u64, fault: ControlFault) {
        if self.advance(now_us).is_ok() {
            self.fail_gpio(fault);
        }
        self.rejected();
    }
    pub fn strip_fault(&mut self, now_us: u64, fault: ControlFault) {
        if self.advance(now_us).is_ok() {
            self.fail_strip(fault);
        }
        self.rejected();
    }

    fn gpio_mask(&self, sample: &GpioSample) -> Result<u16, InputError> {
        if !self.config.has_gpio()
            || sample.primary.is_some() != self.config.primary.is_some()
            || sample.reset.is_some() != self.config.reset.is_some()
            || sample.head_pads.len() != self.config.head_pads.len()
        {
            return Err(error(
                ControlFault::Missing,
                "GPIO acquisition must include exactly the configured inputs",
            ));
        }
        let mut mask = 0;
        let mut seen = BTreeSet::new();
        for pad in &sample.head_pads {
            let index = self
                .config
                .head_pads
                .iter()
                .position(|name| *name == pad.name)
                .ok_or_else(|| {
                    error(
                        ControlFault::Invalid,
                        "GPIO acquisition contains an unknown head pad",
                    )
                })?;
            if !seen.insert(index) {
                return Err(error(
                    ControlFault::Invalid,
                    "GPIO acquisition repeats a head pad",
                ));
            }
            if pad.active {
                mask |= 1 << index;
            }
        }
        Ok(mask)
    }
    pub fn ingest_gpio(&mut self, now_us: u64, sample: GpioSample) -> Result<(), InputError> {
        let result = self.ingest_gpio_checked(now_us, sample);
        if let Err(failure) = &result {
            if !self.terminal {
                self.fail_gpio(failure.fault);
            }
            self.rejected();
        }
        result
    }
    fn ingest_gpio_checked(&mut self, now: u64, sample: GpioSample) -> Result<(), InputError> {
        self.advance(now)?;
        let mut mask = self.gpio_mask(&sample)?;
        self.gpio
            .accept(now, &sample.stamp, &sample.session_id, &self.clock_id)?;
        let at = sample.stamp.at_us;
        // A pulse pad counts as touched while it pulses, not by its level.
        for pad in &mut self.pulse_pads {
            if pad.read(mask & pad.bit != 0, at) {
                mask |= pad.bit;
            } else {
                mask &= !pad.bit;
            }
        }
        for (source, level) in [
            (TouchSource::Primary, sample.primary),
            (TouchSource::Reset, sample.reset),
        ] {
            let button = if source == TouchSource::Primary {
                &mut self.primary
            } else {
                &mut self.reset
            };
            if let Some(active) = level
                && let Some(contact) = button.observe(active, at)?
            {
                let held = at - contact.began;
                if source == TouchSource::Reset && held < RESET_HOLD_US {
                    continue;
                }
                let mut event = self.event_for(source, contact, at, now);
                event.released_us = Some(at);
                event.gesture = if source == TouchSource::Reset || held >= PRIMARY_HOLD_US {
                    TouchGesture::Hold
                } else {
                    TouchGesture::Tap
                };
                let intent = if source == TouchSource::Reset {
                    TouchIntent::FactoryReset
                } else if held >= PRIMARY_HOLD_US {
                    TouchIntent::SoftwareMute
                } else {
                    TouchIntent::Stop
                };
                event.when_speaking = intent;
                event.when_quiet = intent;
                event.when_asleep = Some(intent);
                self.emit(event)?;
            }
        }
        if !self.config.head_pads.is_empty() {
            for (contact, gesture, released) in self.head.observe(mask, at)? {
                let mut event = self.event_for(TouchSource::Head, contact, at, now);
                event.gesture = gesture;
                // Without the pads left out (#1798).
                event.active_mask = self.head.mask;
                event.active_names = self.head_names(self.head.mask);
                event.released_us = released;
                // Any contact of at least the shortest tap wakes a sleeping
                // lamp (#3747, the lead's table), at its letting go under a
                // second or at the stroke if kept, so a person who holds it
                // longer to bring it back still does. The touch alone never
                // wakes it: a lone edge under the shortest tap is noise
                // (`HEAD_TAP_MIN_US`). A hand kept three seconds still asks
                // for sleep (the lead, #1711), the owner deciding whether it
                // may (not while onboarding is under way).
                let (speaking, quiet, asleep) = match gesture {
                    TouchGesture::Tap => (
                        TouchIntent::Attend,
                        TouchIntent::Attend,
                        Some(TouchIntent::Wake),
                    ),
                    // A Stroke asks nothing more of a lamp asleep, in the form
                    // every reader takes, older ones too (Third's P2 on #3767):
                    // the voice and Expression wake on a head Stroke whose
                    // contact began asleep by their own rule.
                    TouchGesture::Stroke => (TouchIntent::Stroke, TouchIntent::Stroke, None),
                    TouchGesture::Hold => (TouchIntent::Sleep, TouchIntent::Sleep, None),
                    _ => (TouchIntent::Stop, TouchIntent::Notice, None),
                };
                event.when_speaking = speaking;
                event.when_quiet = quiet;
                event.when_asleep = asleep;
                self.emit(event)?;
            }
            if let Some(note) = self.head.pet_due.take() {
                self.emit_pet(note, now)?;
            }
        }
        Ok(())
    }
    fn emit_pet(&mut self, note: PetNote, now: u64) -> Result<(), InputError> {
        if self.pending_pets.len() == MAX_PENDING_EVENTS {
            return Err(error(ControlFault::Overflow, "head pet queue is full"));
        }
        self.pet_sequence = self
            .pet_sequence
            .checked_add(1)
            .ok_or_else(|| error(ControlFault::Overflow, "head pet identity exhausted"))?;
        let pet = HeadPet {
            stamp: ClockStamp {
                clock_id: self.clock_id.clone(),
                at_us: now,
                sequence: self.pet_sequence,
            },
            session_id: self.session_id.clone(),
            reader_session_id: self.gpio.session.clone().expect("accepted reader"),
            enabled: self.config.enabled,
            contact_id: note.contact.id,
            began_us: note.contact.began,
            petted_us: note.petted,
            observed_us: note.observed,
            swaps: note.swaps,
            swaps_per_second: note.per_second,
            resting: note.resting,
            ended: note.ended,
        };
        pet.check()
            .map_err(|_| error(ControlFault::Invalid, "recognizer produced an invalid pet"))?;
        self.pending_pets.push(pet);
        Ok(())
    }
    pub fn ingest_strip(&mut self, now_us: u64, sample: StripSample) -> Result<(), InputError> {
        let result = self.ingest_strip_checked(now_us, sample);
        if let Err(failure) = &result {
            if !self.terminal {
                self.fail_strip(failure.fault);
            }
            self.rejected();
        }
        result
    }
    fn ingest_strip_checked(&mut self, now: u64, sample: StripSample) -> Result<(), InputError> {
        self.advance(now)?;
        if self.strip.is_none() || sample.mask & !0x0fff != 0 {
            return Err(error(
                ControlFault::Invalid,
                "unconfigured strip or invalid electrode mask",
            ));
        }
        let previous = self.strip_fence.session.clone();
        let accepted =
            self.strip_fence
                .accept(now, &sample.stamp, &sample.session_id, &self.clock_id);
        if previous != self.strip_fence.session && previous.is_some() {
            self.strip = Some(
                StripRecognizer::with_timing(
                    self.config.strip_pads.as_ref().expect("configured strip"),
                    self.config.strip_timing(),
                )
                .map_err(strip_error)?,
            );
        }
        accepted?;
        self.strip_mask = sample.mask;
        let update = self
            .strip
            .as_mut()
            .expect("configured strip")
            .update(now, sample.stamp.at_us, Some(sample.mask))
            .map_err(strip_error)?;
        for effect in update.effects {
            let mut event = self.event_for(
                TouchSource::Strip,
                Contact {
                    id: effect.contact_id,
                    began: effect.started_us,
                },
                effect.observed_us,
                now,
            );
            let (gesture, intent) = match effect.kind {
                StripEffectKind::Touch => (TouchGesture::Touch, None),
                StripEffectKind::Slide => (TouchGesture::Slide, Some(TouchIntent::Slide)),
                StripEffectKind::Stroke => (TouchGesture::Stroke, Some(TouchIntent::Stroke)),
                StripEffectKind::Sleep => (TouchGesture::Hold, Some(TouchIntent::Sleep)),
            };
            event.gesture = gesture;
            if let Some(intent) = intent {
                event.when_speaking = intent;
                event.when_quiet = intent;
                event.when_asleep = (intent == TouchIntent::Sleep).then_some(TouchIntent::Wake);
            }
            event.released_us = effect.released_us;
            event.active_mask = effect.active_mask;
            event.position = effect.position;
            event.toward_pad = self.toward(effect.direction);
            event.distance = effect.distance;
            event.speed = effect.speed;
            self.emit(event)?;
        }
        Ok(())
    }
    /// The head pads left out because they have read touched since the
    /// first acquisition, never released (#1798): health names them.
    pub fn head_left_out(&self) -> Vec<String> {
        self.head_names(self.head.from_start.unwrap_or(0))
    }
    fn head_names(&self, mask: u16) -> Vec<String> {
        self.config
            .head_pads
            .iter()
            .enumerate()
            .filter(|(index, _)| mask & (1 << index) != 0)
            .map(|(_, name)| name.clone())
            .collect()
    }
    fn toward(&self, direction: Option<StripDirection>) -> Option<u8> {
        let pads = self.config.strip_pads.as_ref()?;
        match direction? {
            StripDirection::TowardStart => pads.first().copied(),
            StripDirection::TowardEnd => pads.last().copied(),
        }
    }
    fn name(&self, source: TouchSource) -> &str {
        match source {
            TouchSource::Primary => self.config.primary.as_deref().unwrap_or("primary"),
            TouchSource::Reset => self.config.reset.as_deref().unwrap_or("reset"),
            TouchSource::Head => "head",
            TouchSource::Strip => "strip",
        }
    }
    fn event_for(
        &self,
        source: TouchSource,
        contact: Contact,
        observed: u64,
        now: u64,
    ) -> TouchEvent {
        let fence = if source == TouchSource::Strip {
            &self.strip_fence
        } else {
            &self.gpio
        };
        TouchEvent {
            stamp: ClockStamp {
                clock_id: self.clock_id.clone(),
                at_us: now,
                sequence: 0,
            },
            session_id: self.session_id.clone(),
            reader_session_id: fence.session.clone().expect("accepted reader"),
            enabled: self.config.enabled,
            source,
            name: self.name(source).to_owned(),
            contact_id: contact.id,
            gesture: TouchGesture::Touch,
            when_speaking: TouchIntent::Stop,
            when_quiet: TouchIntent::Attend,
            when_asleep: Some(TouchIntent::Wake),
            began_us: contact.began,
            observed_us: observed,
            released_us: None,
            active_mask: 0,
            active_names: Vec::new(),
            position: None,
            toward_pad: None,
            distance: 0.0,
            speed: 0.0,
        }
    }
    fn emit(&mut self, mut event: TouchEvent) -> Result<(), InputError> {
        if self.pending.len() == MAX_PENDING_EVENTS {
            return Err(error(ControlFault::Overflow, "touch event queue is full"));
        }
        self.event_sequence = self
            .event_sequence
            .checked_add(1)
            .ok_or_else(|| error(ControlFault::Overflow, "touch event identity exhausted"))?;
        event.stamp.sequence = self.event_sequence;
        event.check().map_err(|_| {
            error(
                ControlFault::Invalid,
                "recognizer produced invalid touch evidence",
            )
        })?;
        if event.retained() {
            self.latest.retain(|old| {
                (old.source, old.when_speaking, old.when_quiet)
                    != (event.source, event.when_speaking, event.when_quiet)
            });
            self.latest.push(event.clone());
        }
        self.pending.push(event);
        Ok(())
    }
    fn source_state(&self, source: TouchSource, now: u64) -> TouchSourceState {
        let fence = if source == TouchSource::Strip {
            &self.strip_fence
        } else {
            &self.gpio
        };
        let mut state = TouchSourceState {
            source,
            name: self.name(source).to_owned(),
            reader_session_id: fence.session.clone(),
            ready: fence.fault.is_none(),
            fault: fence.fault,
            observed_us: fence.observed(),
            armed: false,
            active: None,
            contact_id: None,
            began_us: None,
            held_us: None,
            active_mask: None,
            active_names: Vec::new(),
            position: None,
            toward_pad: None,
            distance: 0.0,
            speed: 0.0,
        };
        if !state.ready {
            return state;
        }
        let contact = match source {
            TouchSource::Primary | TouchSource::Reset => {
                let button = if source == TouchSource::Primary {
                    &self.primary
                } else {
                    &self.reset
                };
                state.armed = button.armed;
                state.active_mask = button.raw.map(u16::from);
                button.contact
            }
            TouchSource::Head => {
                state.armed = self.head.armed;
                state.active_mask = Some(self.head.mask);
                state.active_names = self.head_names(self.head.mask);
                self.head.contact
            }
            TouchSource::Strip => {
                let strip = self.strip.as_ref().expect("configured strip").state(now);
                state.armed = strip.ready;
                // State is the acquired working-electrode mask, before contact
                // filtering. Raw excluded bits remain in StripSample/recording
                // evidence but cannot prolong a contact or its held duration.
                // A filtered handoff alone still must not fabricate a release.
                state.active_mask = Some(self.strip_mask & self.strip_selected_mask);
                state.position = strip.position;
                state.toward_pad = self.toward(strip.direction);
                state.distance = strip.distance;
                state.speed = strip.speed;
                strip
                    .contact_id
                    .zip(strip.started_us)
                    .map(|(id, began)| Contact { id, began })
            }
        };
        state.active = state.active_mask.map(|mask| mask != 0);
        if let Some(contact) = contact {
            state.contact_id = Some(contact.id);
            state.began_us = Some(contact.began);
            if state.armed && state.active == Some(true) {
                state.held_us = state.observed_us.map(|at| at - contact.began);
            }
        }
        if source == TouchSource::Head {
            // Through a pulse's short gap too; nothing for a pushed head.
            state.held_us = state
                .observed_us
                .filter(|_| state.armed)
                .and_then(|at| self.head.held(at));
        }
        state
    }
    pub fn tick(&mut self, now_us: u64) -> TouchOutput {
        if self.advance(now_us).is_err() {
            self.rejected();
        }
        let now = self.now.unwrap_or(now_us);
        if let Some(sequence) = self.state_sequence.checked_add(1) {
            self.state_sequence = sequence;
        } else {
            self.terminal = true;
            self.fail_gpio(ControlFault::Overflow);
            self.fail_strip(ControlFault::Overflow);
        }
        let mut sources = Vec::with_capacity(4);
        for (present, source) in [
            (self.config.primary.is_some(), TouchSource::Primary),
            (self.config.reset.is_some(), TouchSource::Reset),
            (!self.config.head_pads.is_empty(), TouchSource::Head),
            (self.strip.is_some(), TouchSource::Strip),
        ] {
            if present {
                sources.push(self.source_state(source, now));
            }
        }
        let state = TouchState {
            stamp: ClockStamp {
                clock_id: self.clock_id.clone(),
                at_us: now,
                sequence: self.state_sequence,
            },
            session_id: self.session_id.clone(),
            enabled: self.config.enabled,
            sources,
            latest: self.latest.clone(),
            event_sequence: self.event_sequence,
            rejected_inputs: self.rejected_inputs,
        };
        debug_assert!(
            state.check().is_ok(),
            "touch state must validate: {:?}",
            state.check()
        );
        TouchOutput {
            events: std::mem::take(&mut self.pending),
            state,
            pets: std::mem::take(&mut self.pending_pets),
        }
    }
}
