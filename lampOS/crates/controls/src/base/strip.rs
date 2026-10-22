//! Opt-in sampled strip recognition. The legacy base recognizer is unchanged.
//!
//! Positions are centroids on a normalized twelve-slot strip, not millimetres
//! or inferred positions between acquisitions. A complete twelve-pad order
//! retains its configured rank/11 coordinates. A sparse working set must be
//! ascending or descending: coordinates remain E/11 or (11-E)/11, respectively,
//! so excluding faulty electrodes does not compress the strip's width. Sparse
//! topology skips only configured exclusions; an inactive working electrode
//! between active working electrodes makes that footprint ambiguous. Complete
//! orders retain their existing tolerance of one inactive internal electrode.
//! These thresholds are authored policy awaiting per-unit qualification;
//! chip debounce and physical latency
//! are separate. The adapter must report missing/configuration/over-current
//! evidence as unknown, never as a zero mask.

use super::{ALL_PADS, CONTACT_DEBOUNCE_US, MIN_PADS, PAD_DEBOUNCE_US, QUALIFY_US};
use std::collections::VecDeque;
use std::fmt;

pub const FRESH_US: u64 = 100_000;
pub const RELEASE_US: u64 = 30_000;
pub const SLEEP_US: u64 = 2_000_000;
pub const SPEED_WINDOW_US: u64 = 200_000;
pub const SPEED_SAMPLES: usize = 128;
pub const STROKE_US: u64 = 300_000;
pub const DIRECTION_RANKS: f32 = 0.5;
pub const STROKE_RANKS: f32 = 1.0;
pub const STROKE_RANKS_PER_SECOND: f32 = 2.0;
pub const MAX_DISTANCE: f32 = 1024.0;
pub const MAX_SPEED: f32 = 1000.0;
pub const MAX_EFFECTS: usize = 3;
const SPAN: f32 = 11.0;

/// The strip's timing, from the board's touch pins file (`strip_qualify_ms`,
/// `strip_release_ms`, `strip_stroke_ms`, `strip_sleep_ms`), each defaulting
/// to the constants here when the board gives none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StripTiming {
    /// A three-pad chord held this long qualifies a touch.
    pub qualify_us: u64,
    /// A contact let go this long ends.
    pub release_us: u64,
    /// Movement this long, slow enough, is a Stroke.
    pub stroke_us: u64,
    /// A stationary qualified contact held this long is Sleep.
    pub sleep_us: u64,
}

impl Default for StripTiming {
    fn default() -> Self {
        Self {
            qualify_us: QUALIFY_US,
            release_us: RELEASE_US,
            stroke_us: STROKE_US,
            sleep_us: SLEEP_US,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StripDirection {
    TowardStart,
    TowardEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StripEffectKind {
    Touch,
    Slide,
    Stroke,
    Sleep,
}

/// A request for the current interaction owner, never evidence that Voice has
/// stopped, started listening, or applied a microphone/sleep state change.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StripEffect {
    pub kind: StripEffectKind,
    pub contact_id: u64,
    pub started_us: u64,
    pub observed_us: u64,
    pub released_us: Option<u64>,
    pub position: Option<f32>,
    pub active_mask: u16,
    pub direction: Option<StripDirection>,
    pub distance: f32,
    pub speed: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StripState {
    pub ready: bool,
    pub observed_us: Option<u64>,
    /// Filtered pads that are also present in the current raw acquisition.
    pub active_mask: u16,
    pub contact_id: Option<u64>,
    pub started_us: Option<u64>,
    pub position: Option<f32>,
    /// Last hysteresis-qualified direction; absent during a sampled pause.
    pub direction: Option<StripDirection>,
    /// Cumulative normalized centroid travel since movement was established.
    pub distance: f32,
    /// Magnitude in normalized strip lengths/second over at most 200 ms and
    /// 128 actual acquisitions. No receipt clock extrapolation is used.
    pub speed: f32,
    pub held_us: u64,
    pub moving: bool,
    pub sleep_sent: bool,
    /// A disconnected footprint is consumed until the whole contact ends.
    pub ambiguous: bool,
}

impl StripState {
    fn empty(observed_us: Option<u64>) -> Self {
        Self {
            ready: false,
            observed_us,
            active_mask: 0,
            contact_id: None,
            started_us: None,
            position: None,
            direction: None,
            distance: 0.0,
            speed: 0.0,
            held_us: 0,
            moving: false,
            sleep_sent: false,
            ambiguous: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StripUpdate {
    pub state: StripState,
    /// At most MAX_EFFECTS; there is deliberately no release/tap action.
    pub effects: Vec<StripEffect>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StripError {
    InvalidOrder,
    UnknownSample,
    InvalidMask,
    ClockBackwards,
    ObservationOutOfOrder,
    FutureSample,
    StaleSample,
    SampleGap,
    ContactExhausted,
    MotionBound,
}

impl fmt::Display for StripError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidOrder => {
                "strip order needs 3..12 unique electrodes 0..11; sparse orders must ascend or descend"
            }
            Self::UnknownSample => "strip acquisition is unknown",
            Self::InvalidMask => "strip mask contains non-electrode status bits",
            Self::ClockBackwards => "strip receipt clock ran backwards",
            Self::ObservationOutOfOrder => "strip acquisition is duplicate or out of order",
            Self::FutureSample => "strip acquisition is after receipt",
            Self::StaleSample => "strip acquisition is at least 100 ms old",
            Self::SampleGap => "strip acquisition gap is at least 100 ms",
            Self::ContactExhausted => "strip contact counter is exhausted",
            Self::MotionBound => "strip motion exceeds bounded distance or speed",
        })
    }
}

impl std::error::Error for StripError {}

#[derive(Clone, Copy, Debug, Default)]
struct Pad {
    candidate: bool,
    since_us: u64,
    stable: bool,
}

#[derive(Clone, Copy, Debug)]
struct MotionSample {
    at_us: u64,
    distance: f32,
}

#[derive(Debug)]
struct Contact {
    id: u64,
    started_us: u64,
    origin_mask: u16,
    seen_mask: u16,
    origin_position: f32,
    last_position: f32,
    active_mask: u16,
    release_us: Option<u64>,
    qualified: bool,
    touch_sent: bool,
    sleep_sent: bool,
    stroke_sent: bool,
    sleep_blocked: bool,
    travel_evidence: bool,
    ambiguous: bool,
    moving_since_us: Option<u64>,
    direction: Option<StripDirection>,
    extreme: f32,
    distance: f32,
    speed: f32,
    samples: VecDeque<MotionSample>,
}

/// At most twelve-pad filtering, one contact and at most 128 speed samples.
/// Session/sequence/boot fencing belongs to the independent reader coordinator.
/// `reset` preserves time and contact high-water marks so old queued samples
/// cannot become a new press. Replace this object only after a fenced reader
/// restart; a released baseline is still required.
#[derive(Debug)]
pub struct StripRecognizer {
    timing: StripTiming,
    selected: u16,
    /// Topological rank among working electrodes, independent of their spacing.
    rank: [u8; 12],
    /// Full-strip coordinate numerator with a fixed denominator of eleven.
    coordinate: [u8; 12],
    max_rank_gap: u8,
    pads: [Pad; 12],
    seeded: bool,
    armed: bool,
    last_now_us: Option<u64>,
    last_observed_us: Option<u64>,
    raw: u16,
    effective: u16,
    contact_since_us: u64,
    qualified_since_us: Option<u64>,
    qualified: bool,
    next_contact: u64,
    contact: Option<Contact>,
}

impl StripRecognizer {
    pub fn new(pad_order: impl AsRef<[u8]>) -> Result<Self, StripError> {
        Self::with_timing(pad_order, StripTiming::default())
    }

    /// As [`Self::new`], with the board's own timing.
    pub fn with_timing(
        pad_order: impl AsRef<[u8]>,
        timing: StripTiming,
    ) -> Result<Self, StripError> {
        let pad_order = pad_order.as_ref();
        if !(3..=12).contains(&pad_order.len()) {
            return Err(StripError::InvalidOrder);
        }
        let complete = pad_order.len() == 12;
        let ascending = pad_order.windows(2).all(|pair| pair[0] < pair[1]);
        if !complete && !ascending && !pad_order.windows(2).all(|pair| pair[0] > pair[1]) {
            return Err(StripError::InvalidOrder);
        }
        let mut rank = [0; 12];
        let mut coordinate = [0; 12];
        let mut seen = 0u16;
        for (index, &pad) in pad_order.iter().enumerate() {
            if pad >= 12 || seen & (1 << pad) != 0 {
                return Err(StripError::InvalidOrder);
            }
            seen |= 1 << pad;
            rank[pad as usize] = index as u8;
            coordinate[pad as usize] = if complete {
                index as u8
            } else if ascending {
                pad
            } else {
                11 - pad
            };
        }
        Ok(Self {
            timing,
            selected: seen,
            rank,
            coordinate,
            max_rank_gap: if complete { 2 } else { 1 },
            pads: [Pad::default(); 12],
            seeded: false,
            armed: false,
            last_now_us: None,
            last_observed_us: None,
            raw: 0,
            effective: 0,
            contact_since_us: 0,
            qualified_since_us: None,
            qualified: false,
            next_contact: 0,
            contact: None,
        })
    }

    pub fn reset(&mut self) {
        self.pads = [Pad::default(); 12];
        self.seeded = false;
        self.armed = false;
        self.raw = 0;
        self.effective = 0;
        self.contact_since_us = 0;
        self.clear_contact();
    }

    /// A passive snapshot. Stale, future, or backwards reads expose neither a
    /// ready recognizer nor a contact; a tick cannot advance held time or emit.
    pub fn state(&self, now_us: u64) -> StripState {
        let mut state = StripState::empty(self.last_observed_us);
        if !self.seeded
            || !self.armed
            || self.last_now_us.is_none_or(|last| now_us < last)
            || self
                .last_observed_us
                .is_none_or(|observed| now_us.saturating_sub(observed) >= FRESH_US)
        {
            return state;
        }
        state.ready = true;
        state.active_mask = self.effective;
        let Some(contact) = &self.contact else {
            return state;
        };
        state.contact_id = Some(contact.id);
        state.started_us = Some(contact.started_us);
        state.position =
            (!contact.ambiguous && self.effective != 0).then_some(contact.last_position);
        state.direction = (self.effective != 0 && contact.speed > 0.0)
            .then_some(contact.direction)
            .flatten();
        state.distance = contact.distance;
        state.speed = if self.effective == 0 || contact.ambiguous {
            0.0
        } else {
            contact.speed
        };
        state.held_us = contact
            .release_us
            .or(self.last_observed_us)
            .unwrap_or(contact.started_us)
            .saturating_sub(contact.started_us);
        state.moving = contact.moving_since_us.is_some();
        state.sleep_sent = contact.sleep_sent;
        state.ambiguous = contact.ambiguous;
        state
    }

    pub fn update(
        &mut self,
        now_us: u64,
        observed_us: u64,
        mask: Option<u16>,
    ) -> Result<StripUpdate, StripError> {
        match self.update_checked(now_us, observed_us, mask) {
            Ok(effects) => Ok(StripUpdate {
                state: self.state(now_us),
                effects,
            }),
            Err(error) => {
                self.reset();
                Err(error)
            }
        }
    }

    fn update_checked(
        &mut self,
        now_us: u64,
        observed_us: u64,
        mask: Option<u16>,
    ) -> Result<Vec<StripEffect>, StripError> {
        let previous_now = self.last_now_us;
        let previous_observed = self.last_observed_us;
        self.last_now_us = Some(previous_now.map_or(now_us, |old| old.max(now_us)));
        if observed_us <= now_us {
            self.last_observed_us =
                Some(previous_observed.map_or(observed_us, |old| old.max(observed_us)));
        }
        if previous_now.is_some_and(|last| now_us < last) {
            return Err(StripError::ClockBackwards);
        }
        if observed_us > now_us {
            return Err(StripError::FutureSample);
        }
        if previous_observed.is_some_and(|last| observed_us <= last) {
            return Err(StripError::ObservationOutOfOrder);
        }
        if now_us - observed_us >= FRESH_US {
            return Err(StripError::StaleSample);
        }
        if self.seeded && previous_observed.is_some_and(|last| observed_us - last >= FRESH_US) {
            return Err(StripError::SampleGap);
        }
        let mask = mask.ok_or(StripError::UnknownSample)?;
        if mask & !ALL_PADS != 0 {
            return Err(StripError::InvalidMask);
        }
        // Validate the whole sensor sample first. Excluded electrode levels
        // cannot seed, prolong, qualify or move a selected contact. If no
        // working electrode is present, this is a real logical release; the
        // existing bounded grace never fabricates a current position or Sleep.
        let mask = mask & self.selected;
        if !self.seeded {
            self.seeded = true;
            self.raw = mask;
            self.effective = mask;
            self.contact_since_us = observed_us;
            for (index, pad) in self.pads.iter_mut().enumerate() {
                *pad = Pad {
                    candidate: mask & (1 << index) != 0,
                    stable: mask & (1 << index) != 0,
                    since_us: observed_us,
                };
            }
            return Ok(Vec::new());
        }

        if let Some(released) = self.contact.as_ref().and_then(|c| c.release_us) {
            if observed_us - released >= self.timing.release_us {
                self.clear_contact();
            } else if mask != 0 {
                self.contact.as_mut().expect("contact exists").release_us = None;
            }
        }
        let previous_raw = self.raw;
        let released_press = (mask == 0 && previous_raw != 0).then_some(self.contact_since_us);
        if (mask != 0) != (self.raw != 0) {
            self.contact_since_us = observed_us;
            if mask != 0 && self.contact.is_none() {
                self.qualified_since_us = None;
                self.qualified = false;
            }
        }
        self.raw = mask;
        let mut stable = 0u16;
        for (index, pad) in self.pads.iter_mut().enumerate() {
            let touched = mask & (1 << index) != 0;
            if touched != pad.candidate {
                pad.candidate = touched;
                pad.since_us = observed_us;
            }
            if observed_us - pad.since_us >= PAD_DEBOUNCE_US {
                pad.stable = touched;
            }
            if pad.stable {
                stable |= 1 << index;
            }
        }
        self.effective = stable & mask;
        if !self.armed {
            if mask == 0
                && stable == 0
                && observed_us - self.contact_since_us >= CONTACT_DEBOUNCE_US
            {
                self.armed = true;
            }
            return Ok(Vec::new());
        }

        // Close the previous interval before its falling edge. Separate short
        // third-pad spikes never accumulate into a qualifying chord.
        if self
            .qualified_since_us
            .is_some_and(|since| observed_us - since >= self.timing.qualify_us)
        {
            self.qualified = true;
        }
        if self.effective.count_ones() >= MIN_PADS {
            self.qualified_since_us.get_or_insert(observed_us);
        } else {
            self.qualified_since_us = None;
        }

        // Geometry uses the fully debounced footprint. Removing a departing
        // pad immediately while an arriving neighbour is still filtering
        // creates an artificial centroid overshoot and reversal. Qualification
        // still uses only pads that remain raw-active, and a true raw zero has
        // no current position even while release filters settle.
        let position = (mask != 0).then(|| self.position(stable)).flatten();
        // A newly arriving neighbour is absent from `effective` until its
        // debounce completes. Do not turn that filter-created temporary hole
        // into a second-hand fault. Real disjoint raw footprints still consume
        // the contact conservatively, even before their pad filters settle.
        let disconnected = self.disconnected(mask);
        // An irregular poll can first prove both debounce intervals on the
        // actual falling edge. Retain that qualified brief contact without
        // fabricating a still-active level or extending it into Sleep.
        let brief_release = released_press
            .filter(|began| self.qualified && observed_us - began >= CONTACT_DEBOUNCE_US);
        let origin_mask = if brief_release.is_some() {
            stable & previous_raw
        } else {
            self.effective
        };
        let origin_position = if brief_release.is_some() {
            self.position(origin_mask)
        } else {
            position
        };
        let started_us = brief_release.unwrap_or(self.contact_since_us);
        if self.contact.is_none()
            && origin_mask != 0
            && let Some(position) = origin_position
            && observed_us - started_us >= CONTACT_DEBOUNCE_US
        {
            self.next_contact = self
                .next_contact
                .checked_add(1)
                .ok_or(StripError::ContactExhausted)?;
            self.contact = Some(Contact {
                id: self.next_contact,
                started_us,
                origin_mask,
                seen_mask: origin_mask,
                origin_position: position,
                last_position: position,
                active_mask: self.effective,
                release_us: None,
                qualified: false,
                touch_sent: false,
                sleep_sent: false,
                stroke_sent: false,
                sleep_blocked: false,
                travel_evidence: false,
                ambiguous: disconnected || self.disconnected(origin_mask),
                moving_since_us: None,
                direction: None,
                extreme: position,
                distance: 0.0,
                speed: 0.0,
                samples: VecDeque::from([MotionSample {
                    at_us: observed_us,
                    distance: 0.0,
                }]),
            });
        }
        let mut effects = Vec::with_capacity(MAX_EFFECTS);
        let Some(contact) = &mut self.contact else {
            return Ok(effects);
        };
        contact.qualified |= self.qualified;
        contact.ambiguous |= disconnected;
        contact.active_mask = self.effective;
        if mask == 0 {
            contact.release_us.get_or_insert(self.contact_since_us);
        }
        let departed = contact.origin_mask & !self.effective;
        let arrivals = self.effective & !contact.seen_mask;
        // Recruitment then shrinking is not a Slide. It is nevertheless too
        // ambiguous to authorize Sleep once old pads depart while new remain.
        contact.sleep_blocked |=
            mask & !contact.origin_mask != 0 && contact.origin_mask & !mask != 0;
        contact.travel_evidence |= departed != 0 && arrivals != 0;
        contact.seen_mask |= self.effective;
        contact.sleep_blocked |= contact.ambiguous || contact.travel_evidence;
        if !contact.ambiguous
            && let Some(position) = position
        {
            contact.motion(position, observed_us, self.timing.stroke_us, &mut effects)?;
        }
        if contact.qualified
            && !contact.touch_sent
            && !contact.ambiguous
            && (self.effective != 0 || contact.release_us == Some(observed_us))
        {
            contact.touch_sent = true;
            effects.insert(0, contact.effect(StripEffectKind::Touch, observed_us));
        }
        if contact.qualified
            && mask != 0
            && self.effective != 0
            && !contact.sleep_sent
            && !contact.sleep_blocked
            && observed_us - contact.started_us >= self.timing.sleep_us
        {
            contact.sleep_sent = true;
            effects.push(contact.effect(StripEffectKind::Sleep, observed_us));
        }
        debug_assert!(effects.len() <= MAX_EFFECTS);
        Ok(effects)
    }

    fn clear_contact(&mut self) {
        self.contact = None;
        self.qualified_since_us = None;
        self.qualified = false;
    }

    fn position(&self, mask: u16) -> Option<f32> {
        let count = mask.count_ones();
        if count == 0 {
            return None;
        }
        let sum: u32 = self
            .coordinate
            .iter()
            .enumerate()
            .filter(|(pad, _)| mask & (1 << pad) != 0)
            .map(|(_, rank)| u32::from(*rank))
            .sum();
        Some(sum as f32 / count as f32 / SPAN)
    }

    fn disconnected(&self, mask: u16) -> bool {
        let mut ranks = 0u16;
        for (pad, rank) in self.rank.iter().enumerate() {
            if mask & (1 << pad) != 0 {
                ranks |= 1 << rank;
            }
        }
        let mut previous = None;
        for rank in 0u8..12 {
            if ranks & (1 << rank) != 0 {
                if previous.is_some_and(|previous| rank - previous > self.max_rank_gap) {
                    return true;
                }
                previous = Some(rank);
            }
        }
        false
    }
}

impl Contact {
    fn effect(&self, kind: StripEffectKind, observed_us: u64) -> StripEffect {
        StripEffect {
            kind,
            contact_id: self.id,
            started_us: self.started_us,
            observed_us,
            released_us: self.release_us,
            position: (!self.ambiguous && self.active_mask != 0).then_some(self.last_position),
            active_mask: self.active_mask,
            direction: (self.active_mask != 0 && self.speed > 0.0)
                .then_some(self.direction)
                .flatten(),
            distance: self.distance,
            speed: if self.active_mask == 0 {
                0.0
            } else {
                self.speed
            },
        }
    }

    fn motion(
        &mut self,
        position: f32,
        observed_us: u64,
        stroke_us: u64,
        effects: &mut Vec<StripEffect>,
    ) -> Result<(), StripError> {
        let displacement = position - self.origin_position;
        let changed = position != self.last_position;
        if self.moving_since_us.is_none()
            && self.travel_evidence
            && displacement.abs() >= DIRECTION_RANKS / SPAN
        {
            self.moving_since_us = Some(observed_us);
            self.direction = Some(if displacement < 0.0 {
                StripDirection::TowardStart
            } else {
                StripDirection::TowardEnd
            });
            self.extreme = position;
            self.distance = displacement.abs();
        } else if self.moving_since_us.is_some() {
            self.distance += (position - self.last_position).abs();
            match self.direction {
                Some(StripDirection::TowardEnd) => {
                    if self.extreme - position >= DIRECTION_RANKS / SPAN {
                        self.direction = Some(StripDirection::TowardStart);
                        self.extreme = position;
                    } else {
                        self.extreme = self.extreme.max(position);
                    }
                }
                Some(StripDirection::TowardStart) => {
                    if position - self.extreme >= DIRECTION_RANKS / SPAN {
                        self.direction = Some(StripDirection::TowardEnd);
                        self.extreme = position;
                    } else {
                        self.extreme = self.extreme.min(position);
                    }
                }
                None => {}
            }
        }
        if self
            .samples
            .back()
            .is_some_and(|last| last.at_us != observed_us)
        {
            if self.samples.len() == SPEED_SAMPLES {
                self.samples.pop_front();
            }
            self.samples.push_back(MotionSample {
                at_us: observed_us,
                distance: self.distance,
            });
        }
        while self.samples.len() > 1
            && self
                .samples
                .front()
                .is_some_and(|first| observed_us - first.at_us > SPEED_WINDOW_US)
        {
            self.samples.pop_front();
        }
        if let Some(first) = self.samples.front() {
            let elapsed = observed_us - first.at_us;
            self.speed = if elapsed == 0 {
                0.0
            } else {
                (self.distance - first.distance) * 1_000_000.0 / elapsed as f32
            };
        }
        if !self.distance.is_finite()
            || !self.speed.is_finite()
            || self.distance > MAX_DISTANCE
            || self.speed > MAX_SPEED
        {
            return Err(StripError::MotionBound);
        }
        self.last_position = position;
        if self.moving_since_us.is_some() && changed {
            effects.push(self.effect(StripEffectKind::Slide, observed_us));
        }
        if self
            .moving_since_us
            .is_some_and(|since| observed_us - since >= stroke_us)
            && changed
            && !self.stroke_sent
            && self.distance >= STROKE_RANKS / SPAN
            && self.distance * 1_000_000.0
                / (observed_us - self.moving_since_us.expect("moving interval")) as f32
                <= STROKE_RANKS_PER_SECOND / SPAN
        {
            // This is a contact-pattern heuristic, not physical velocity:
            // binary centroids jump at pad boundaries. Exclude the initial
            // stationary hold and never turn a pause alone into a Stroke.
            // A pause followed by further travel can lower this estimate.
            self.stroke_sent = true;
            effects.push(self.effect(StripEffectKind::Stroke, observed_us));
        }
        Ok(())
    }
}
