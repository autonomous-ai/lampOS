//! A deterministic, temporary inner state. Time and observations are supplied
//! by the caller; this module performs no I/O and owns no output permission.
use lamp_msgs::{AttentionTarget, ClockStamp, FamiliarLevel, Familiarity, MoodState};
use serde::{Deserialize, Serialize};
use std::fmt;

pub const SECOND_US: u64 = 1_000_000;
pub const MINUTE_US: u64 = 60 * SECOND_US;
pub const MAX_EVENT_LEASE_US: u64 = 15 * SECOND_US;
pub const DIRECT_ATTENTION_US: u64 = 5 * SECOND_US;
pub const FACE_ATTENTION_US: u64 = 2 * SECOND_US;
pub const SOUND_ATTENTION_US: u64 = SECOND_US;
pub const SUDDEN_COOLDOWN_US: u64 = 120 * SECOND_US;
pub const WARMTH_COOLDOWN_US: u64 = 10 * SECOND_US;
pub const AIR_QUALIFY_US: u64 = 10 * MINUTE_US;
pub const QUIET_BEGIN_US: u64 = 15 * MINUTE_US;
pub const QUIET_FULL_US: u64 = 60 * MINUTE_US;
pub const AWAKE_HALF_LIFE_US: u64 = 2 * MINUTE_US;
pub const ENERGY_HALF_LIFE_US: u64 = 15 * SECOND_US;
pub const WARMTH_HALF_LIFE_US: u64 = 20 * MINUTE_US;
pub const AIR_HALF_LIFE_US: u64 = 5 * MINUTE_US;
const BASE_ENERGY: f64 = 0.30;
const BASE_WARMTH: f64 = 0.60;
const ENERGY_CAP: f64 = 0.70;
const WARMTH_CAP: f64 = 0.85;
const AIR_REDUCTION: f64 = 0.08;
const SLEEP_THRESHOLD: f64 = 0.25;
const CHANGE_DELTA: f64 = 0.01;
const EVENT_STREAMS: usize = 12;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FaceTarget {
    pub publisher_boot: String,
    pub id: String,
}

impl FaceTarget {
    fn attention(&self) -> AttentionTarget {
        AttentionTarget::Face {
            publisher_boot: self.publisher_boot.clone(),
            id: self.id.clone(),
        }
    }

    fn check(&self) -> Result<(), Reject> {
        // Bound the copy made for the shared output validation before cloning
        // any caller-supplied identity.
        if self.publisher_boot.len() != 16 || self.id.len() > lamp_msgs::MOOD_MAX_FACE_ID_BYTES {
            return Err(Reject::InvalidFace);
        }
        self.attention().check().map_err(|_| Reject::InvalidFace)
    }
}

/// Only metadata crosses this boundary. The live adapter does not produce
/// Thanks, Laughter or Sleep until their owners provide explicit contracts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum EventKind {
    Name,
    Touch {
        kind: bool,
    },
    UserReminder,
    Sleep,
    SuddenSound,
    Thanks,
    Laughter,
    Faces(Option<FaceTarget>),
    Voice(bool),
    Heard,
    Air(Option<u16>),
    /// How familiar the remembered person at the lamp is, or nobody
    /// remembered (#1670, the eyes' `humans/familiar`).
    Familiar(Option<FamiliarLevel>),
}

impl EventKind {
    fn stream(&self) -> usize {
        match self {
            Self::Name => 0,
            Self::Touch { .. } => 1,
            Self::UserReminder => 2,
            Self::Sleep => 3,
            Self::SuddenSound => 4,
            Self::Thanks => 5,
            Self::Laughter => 6,
            Self::Faces(_) => 7,
            Self::Voice(_) => 8,
            Self::Heard => 9,
            Self::Air(_) => 10,
            Self::Familiar(_) => 11,
        }
    }
}

/// Half-open observation lease. Transport delay must never renew its start
/// or expiry. Publisher identity/order validation belongs to the bus adapter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub observed_us: u64,
    pub valid_until_us: u64,
    pub kind: EventKind,
}

impl Event {
    fn check(&self, now_us: u64) -> Result<(), Reject> {
        if self.valid_until_us <= self.observed_us
            || self.valid_until_us - self.observed_us > MAX_EVENT_LEASE_US
        {
            return Err(Reject::InvalidLease);
        }
        if self.observed_us > now_us {
            return Err(Reject::FutureEvent);
        }
        if now_us >= self.valid_until_us {
            return Err(Reject::ExpiredEvent);
        }
        if let EventKind::Faces(Some(face)) = &self.kind {
            face.check()?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reject {
    ClockRegression,
    InvalidClock,
    InvalidLocalMinute,
    InvalidLease,
    InvalidFace,
    FutureEvent,
    ExpiredEvent,
    OutOfOrder,
    BeforeSleep,
    SnapshotClock,
    InvalidSnapshot,
}

impl fmt::Display for Reject {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(match self {
            Self::ClockRegression => "mood monotonic time moved backwards",
            Self::InvalidClock => "mood clock identity is invalid",
            Self::InvalidLocalMinute => "local minute must be absent or within 0..1440",
            Self::InvalidLease => "mood observation needs a positive lease of at most 15 seconds",
            Self::InvalidFace => "mood face identity is invalid",
            Self::FutureEvent => "mood observation is from the future",
            Self::ExpiredEvent => "mood observation has expired",
            Self::OutOfOrder => "mood observation did not advance its input stream",
            Self::BeforeSleep => "mood observation predates or equals the latest sleep entry",
            Self::SnapshotClock => "advance mood to the snapshot time first",
            Self::InvalidSnapshot => "mood snapshot identity or values are invalid",
        })
    }
}

impl std::error::Error for Reject {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Lease {
    until_us: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Focus {
    target: AttentionTarget,
    priority: u8,
    until_us: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Air {
    until_us: u64,
    high_since_us: Option<u64>,
}

/// Fixed storage: three company leases, one current face, one focus, eleven
/// timestamp watermarks and scalar integrators. There is no personal history.
#[derive(Clone, Debug, PartialEq)]
pub struct Engine {
    clock_id: String,
    now_us: u64,
    local_minute: Option<u16>,
    awake: f64,
    energy_pulse: f64,
    warmth_pulse: f64,
    air_reduction: f64,
    asleep: bool,
    sleep_entry_us: Option<u64>,
    last_activity_us: u64,
    company: [Option<Lease>; 3],
    face: Option<FaceTarget>,
    attention: Option<Focus>,
    air: Option<Air>,
    /// The level the eyes last said for the remembered person at the lamp,
    /// held only until its lease ends: no one's history (#1670).
    familiar: Option<(FamiliarLevel, u64)>,
    last_observed: [Option<u64>; EVENT_STREAMS],
    last_sudden_us: Option<u64>,
    last_warmth_us: Option<u64>,
}

impl Engine {
    pub fn new(clock_id: String, now_us: u64, local_minute: Option<u16>) -> Result<Self, Reject> {
        validate_minute(local_minute)?;
        if clock_id.is_empty()
            || clock_id.len() > 128
            || !clock_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
        {
            return Err(Reject::InvalidClock);
        }
        let asleep = night(local_minute);
        Ok(Self {
            clock_id,
            now_us,
            local_minute,
            awake: if asleep { 0.0 } else { baseline(local_minute) },
            energy_pulse: 0.0,
            warmth_pulse: 0.0,
            air_reduction: 0.0,
            asleep,
            sleep_entry_us: asleep.then_some(now_us),
            last_activity_us: now_us,
            company: [None; 3],
            face: None,
            attention: None,
            air: None,
            familiar: None,
            last_observed: [None; EVENT_STREAMS],
            last_sudden_us: None,
            last_warmth_us: None,
        })
    }

    pub fn now_us(&self) -> u64 {
        self.now_us
    }

    /// Reject atomically, including when advancing time would enter sleep
    /// before a queued event's old observation. True means a visible change
    /// of at least .01, an attention change, or a sleep transition.
    pub fn apply(&mut self, now_us: u64, event: Event) -> Result<bool, Reject> {
        self.check_clock(now_us)?;
        event.check(now_us)?;
        let stream = event.kind.stream();
        if self.last_observed[stream].is_some_and(|last| event.observed_us <= last) {
            return Err(Reject::OutOfOrder);
        }
        let mut next = self.clone();
        next.advance_inner(now_us);
        if next
            .sleep_entry_us
            .is_some_and(|at| event.observed_us <= at)
        {
            return Err(Reject::BeforeSleep);
        }
        next.last_observed[stream] = Some(event.observed_us);
        next.apply_inner(now_us, event);
        let changed = self.changed(&next);
        *self = next;
        Ok(changed)
    }

    /// The supplied local minute is an observation effective at now_us.
    /// Intervening evolution uses the previous clock observation; this does
    /// not invent intermediate wall-clock transitions after clock jumps.
    /// Callers replay local clock changes at their recorded observation times.
    pub fn advance(&mut self, now_us: u64, local_minute: Option<u16>) -> Result<bool, Reject> {
        self.check_clock(now_us)?;
        validate_minute(local_minute)?;
        let before = self.clone();
        self.advance_inner(now_us);
        self.local_minute = local_minute;
        self.maybe_sleep(now_us);
        Ok(before.changed(self))
    }

    /// Snapshots are pure and require the already advanced current time.
    pub fn snapshot(
        &self,
        now_us: u64,
        sequence: u64,
        session_id: &str,
    ) -> Result<MoodState, Reject> {
        if now_us != self.now_us {
            return Err(Reject::SnapshotClock);
        }
        if sequence == 0
            || session_id.is_empty()
            || session_id.len() > 128
            || session_id.chars().any(char::is_control)
        {
            return Err(Reject::InvalidSnapshot);
        }
        let state = MoodState {
            stamp: ClockStamp {
                clock_id: self.clock_id.clone(),
                at_us: now_us,
                sequence,
            },
            session_id: session_id.to_owned(),
            awake: self.awake.clamp(0.0, 1.0) as f32,
            energy: self.energy() as f32,
            warmth: self.warmth() as f32,
            asleep: self.asleep,
            attention: self.attention.as_ref().map(|focus| focus.target.clone()),
            familiarity: Familiarity::Unknown,
            familiar: self.familiar_at(now_us),
        };
        state.check().map_err(|_| Reject::InvalidSnapshot)?;
        Ok(state)
    }

    fn check_clock(&self, now_us: u64) -> Result<(), Reject> {
        if now_us < self.now_us {
            Err(Reject::ClockRegression)
        } else {
            Ok(())
        }
    }

    /// The familiar level while its lease holds.
    fn familiar_at(&self, now_us: u64) -> Option<FamiliarLevel> {
        self.familiar
            .filter(|(_, until_us)| now_us < *until_us)
            .map(|(level, _)| level)
    }

    fn energy(&self) -> f64 {
        (BASE_ENERGY - self.air_reduction + self.energy_pulse).clamp(0.0, ENERGY_CAP)
    }

    fn warmth(&self) -> f64 {
        (BASE_WARMTH + self.warmth_pulse).clamp(BASE_WARMTH, WARMTH_CAP)
    }

    fn changed(&self, next: &Self) -> bool {
        self.asleep != next.asleep
            || self.attention.as_ref().map(|focus| &focus.target)
                != next.attention.as_ref().map(|focus| &focus.target)
            || (self.awake - next.awake).abs() >= CHANGE_DELTA
            || (self.energy() - next.energy()).abs() >= CHANGE_DELTA
            || (self.warmth() - next.warmth()).abs() >= CHANGE_DELTA
            || self.familiar_at(self.now_us) != next.familiar_at(next.now_us)
    }

    fn activity(&mut self, observed_us: u64) {
        self.last_activity_us = self.last_activity_us.max(observed_us);
    }

    fn wake(&mut self, observed_us: u64) {
        self.asleep = false;
        self.awake = self.awake.max(0.85);
        self.activity(observed_us);
    }

    fn set_focus(&mut self, target: AttentionTarget, priority: u8, until_us: u64) {
        if until_us > self.now_us
            && self
                .attention
                .as_ref()
                .is_none_or(|current| priority >= current.priority)
        {
            self.attention = Some(Focus {
                target,
                priority,
                until_us,
            });
        }
    }

    fn warm(&mut self, observed_us: u64, amount: f64) {
        if self.asleep
            || self
                .last_warmth_us
                .is_some_and(|last| observed_us.saturating_sub(last) < WARMTH_COOLDOWN_US)
        {
            return;
        }
        self.last_warmth_us = Some(observed_us);
        self.warmth_pulse = (self.warmth_pulse
            + amount * decay(self.now_us - observed_us, WARMTH_HALF_LIFE_US))
        .min(WARMTH_CAP - BASE_WARMTH);
    }

    fn apply_inner(&mut self, now_us: u64, event: Event) {
        let observed = event.observed_us;
        let lease = Some(Lease {
            until_us: event.valid_until_us,
        });
        match event.kind {
            EventKind::Name => {
                self.wake(observed);
                let target = self
                    .face
                    .as_ref()
                    .map_or(AttentionTarget::Sound, FaceTarget::attention);
                self.set_focus(target, 90, observed.saturating_add(DIRECT_ATTENTION_US));
            }
            EventKind::Touch { kind } => {
                self.wake(observed);
                self.set_focus(
                    AttentionTarget::Touch,
                    90,
                    observed.saturating_add(DIRECT_ATTENTION_US),
                );
                if kind {
                    self.warm(observed, 0.05);
                }
            }
            EventKind::UserReminder => {
                self.wake(observed);
                // A timer has no location. Do not invent a person to look at.
                self.attention = None;
            }
            EventKind::Sleep => self.enter_sleep(now_us),
            EventKind::SuddenSound => {
                if !self.asleep
                    && self
                        .last_sudden_us
                        .is_none_or(|last| observed.saturating_sub(last) >= SUDDEN_COOLDOWN_US)
                {
                    self.last_sudden_us = Some(observed);
                    self.energy_pulse = (self.energy_pulse
                        + 0.25 * decay(now_us - observed, ENERGY_HALF_LIFE_US))
                    .min(ENERGY_CAP - BASE_ENERGY + self.air_reduction);
                    self.set_focus(
                        AttentionTarget::Sound,
                        10,
                        observed.saturating_add(SOUND_ATTENTION_US),
                    );
                }
            }
            EventKind::Thanks | EventKind::Laughter => {
                if !self.asleep {
                    self.activity(observed);
                    self.warm(
                        observed,
                        if matches!(event.kind, EventKind::Thanks) {
                            0.04
                        } else {
                            0.03
                        },
                    );
                }
            }
            EventKind::Faces(face) => {
                let arrival = self.face != face;
                if arrival {
                    self.clear_face_focus();
                }
                self.company[0] = face.as_ref().and(lease);
                self.face = face;
                if let Some(face) = &self.face {
                    let target = face.attention();
                    self.activity(observed);
                    if arrival && !self.asleep {
                        self.set_focus(target, 20, observed.saturating_add(FACE_ATTENTION_US));
                    }
                }
            }
            EventKind::Voice(active) => {
                self.company[1] = active.then_some(lease).flatten();
                if active {
                    self.activity(observed);
                }
            }
            EventKind::Heard => {
                self.company[2] = lease;
                self.activity(observed);
            }
            EventKind::Familiar(level) => {
                self.familiar = level.map(|level| (level, event.valid_until_us));
            }
            EventKind::Air(co2) => {
                let high_since_us = co2.filter(|ppm| *ppm >= 1_200).map(|_| {
                    self.air
                        .and_then(|air| air.high_since_us)
                        .unwrap_or(observed)
                });
                self.air = Some(Air {
                    until_us: event.valid_until_us,
                    high_since_us,
                });
            }
        }
    }

    fn company_at(&self, at_us: u64) -> bool {
        self.company
            .iter()
            .flatten()
            .any(|lease| at_us < lease.until_us)
    }

    fn target(&self, at_us: u64, company: bool) -> f64 {
        let quiet = at_us.saturating_sub(self.last_activity_us);
        let fraction =
            quiet.saturating_sub(QUIET_BEGIN_US) as f64 / (QUIET_FULL_US - QUIET_BEGIN_US) as f64;
        (baseline(self.local_minute) + if company { 0.15 } else { 0.0 }
            - 0.55 * fraction.clamp(0.0, 1.0))
        .clamp(0.0, 1.0)
    }

    fn eligible_at(&self, at_us: u64) -> bool {
        !self.company_at(at_us)
            && at_us.saturating_sub(self.last_activity_us)
                >= if night(self.local_minute) {
                    QUIET_BEGIN_US
                } else {
                    QUIET_FULL_US
                }
    }

    fn enter_sleep(&mut self, at_us: u64) {
        self.asleep = true;
        self.sleep_entry_us = Some(at_us);
        self.awake = 0.0;
        self.attention = None;
        self.company = [None; 3];
        self.face = None;
    }

    fn maybe_sleep(&mut self, at_us: u64) {
        if !self.asleep && self.awake <= SLEEP_THRESHOLD && self.eligible_at(at_us) {
            self.enter_sleep(at_us);
        }
    }

    fn clear_face_focus(&mut self) {
        if self
            .attention
            .as_ref()
            .is_some_and(|focus| matches!(focus.target, AttentionTarget::Face { .. }))
        {
            self.attention = None;
        }
    }

    fn expire(&mut self, at_us: u64) {
        if self.company[0].is_some_and(|lease| at_us >= lease.until_us) {
            self.face = None;
            self.clear_face_focus();
        }
        for company in &mut self.company {
            if company.is_some_and(|lease| at_us >= lease.until_us) {
                *company = None;
            }
        }
        if self
            .attention
            .as_ref()
            .is_some_and(|focus| at_us >= focus.until_us)
        {
            self.attention = None;
        }
        if self.air.is_some_and(|air| at_us >= air.until_us) {
            self.air = None;
        }
    }

    fn advance_inner(&mut self, now_us: u64) {
        // At most twelve boundaries, independent of elapsed time. These are
        // changes in a finite lease or in the piecewise-linear awake target,
        // never one iteration per missed tick.
        let mut edges = [now_us; 16];
        edges[0] = self.now_us;
        edges[1] = now_us;
        let mut count = 2;
        {
            let mut add = |at: u64| {
                if at > self.now_us && at < now_us {
                    edges[count] = at;
                    count += 1;
                }
            };
            for lease in self.company.iter().flatten() {
                add(lease.until_us);
            }
            if let Some(air) = self.air {
                add(air.until_us);
                if let Some(since) = air.high_since_us {
                    add(since.saturating_add(AIR_QUALIFY_US));
                }
            }
            add(self.last_activity_us.saturating_add(QUIET_BEGIN_US));
            add(self.last_activity_us.saturating_add(QUIET_FULL_US));
            add(self
                .last_activity_us
                .saturating_add(if night(self.local_minute) {
                    QUIET_BEGIN_US
                } else {
                    QUIET_FULL_US
                }));
            for company in [false, true] {
                let positive = baseline(self.local_minute) + if company { 0.15 } else { 0.0 };
                if positive < 0.55 {
                    let crossing =
                        (positive / 0.55 * (QUIET_FULL_US - QUIET_BEGIN_US) as f64).ceil() as u64;
                    add(self
                        .last_activity_us
                        .saturating_add(QUIET_BEGIN_US)
                        .saturating_add(crossing));
                }
            }
        }
        edges[..count].sort_unstable();
        let mut from = self.now_us;
        self.expire(from);
        self.maybe_sleep(from);
        for to in edges[..count].iter().copied() {
            if to > from {
                self.integrate(from, to);
                from = to;
                self.expire(to);
                self.maybe_sleep(to);
            }
        }
        self.now_us = now_us;
        self.expire(now_us);
        self.maybe_sleep(now_us);
    }

    fn integrate(&mut self, from: u64, to: u64) {
        let elapsed = to - from;
        self.energy_pulse *= decay(elapsed, ENERGY_HALF_LIFE_US);
        self.warmth_pulse *= decay(elapsed, WARMTH_HALF_LIFE_US);
        let air_target = if self.air.is_some_and(|air| {
            from < air.until_us
                && air
                    .high_since_us
                    .is_some_and(|since| from.saturating_sub(since) >= AIR_QUALIFY_US)
        }) {
            AIR_REDUCTION
        } else {
            0.0
        };
        self.air_reduction =
            air_target + (self.air_reduction - air_target) * decay(elapsed, AIR_HALF_LIFE_US);
        if self.asleep {
            return;
        }
        let company = self.company_at(from);
        let start_target = self.target(from, company);
        let end_target = self.target(to, company);
        let slope = (end_target - start_target) / elapsed as f64;
        let end = follow_linear(self.awake, start_target, slope, elapsed);
        if self.eligible_at(from) && end <= SLEEP_THRESHOLD {
            // The target is non-increasing within this segment. A fixed
            // integer bisection finds the first crossing without catch-up.
            let (mut low, mut high) = (0, elapsed);
            for _ in 0..64 {
                if low >= high {
                    break;
                }
                let middle = low + (high - low) / 2;
                if follow_linear(self.awake, start_target, slope, middle) <= SLEEP_THRESHOLD {
                    high = middle;
                } else {
                    low = middle + 1;
                }
            }
            self.enter_sleep(from.saturating_add(high));
        } else {
            self.awake = end.clamp(0.0, 1.0);
        }
    }
}

fn validate_minute(minute: Option<u16>) -> Result<(), Reject> {
    if minute.is_some_and(|minute| minute >= 1_440) {
        Err(Reject::InvalidLocalMinute)
    } else {
        Ok(())
    }
}

fn night(minute: Option<u16>) -> bool {
    minute.is_some_and(|minute| !(420..1_320).contains(&minute))
}

fn baseline(minute: Option<u16>) -> f64 {
    match minute {
        Some(420..1_140) => 0.70,
        Some(1_140..1_320) | None => 0.40,
        Some(_) => 0.15,
    }
}

fn decay(elapsed_us: u64, half_life_us: u64) -> f64 {
    // More than 64 half-lives is already below any observable f32 change.
    // Bounding the exponent also keeps extreme injected clocks harmless.
    (-(elapsed_us as f64 / half_life_us as f64).min(64.0)).exp2()
}

fn follow_linear(initial: f64, target: f64, slope_per_us: f64, elapsed_us: u64) -> f64 {
    if elapsed_us == 0 {
        return initial;
    }
    let rate = std::f64::consts::LN_2 / AWAKE_HALF_LIFE_US as f64;
    let retained = decay(elapsed_us, AWAKE_HALF_LIFE_US);
    initial * retained
        + target * (1.0 - retained)
        + slope_per_us * (elapsed_us as f64 - (1.0 - retained) / rate)
}
