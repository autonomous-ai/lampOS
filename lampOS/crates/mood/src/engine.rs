//! A deterministic, temporary inner state. Time and observations are supplied
//! by the caller; this module performs no I/O and owns no output permission.
use lamp_msgs::{
    AttentionTarget, ClockStamp, FamiliarLevel, Familiarity, Felt, FeltKind, MOOD_BREATH_US,
    MoodBreath, MoodState, SeemsKind, mood_pace, plain_breath_phase,
};
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
const EVENT_STREAMS: usize = 15;
/// How someone near seems (#1671, `LAMP_MOOD_SEEMS`): small, like a pet's,
/// and nothing kept of anyone. A smile is two `positive` reads in a row
/// (about 4 s); someone low, three `negative` (about 6 s).
pub const SMILE_READS: u8 = 2;
pub const LOW_READS: u8 = 3;
/// Smiled at: half an accepted bang's energy, with its own cooldown, and
/// warmth as laughter's.
const SMILE_ENERGY: f64 = 0.10;
const SMILE_WARMTH: f64 = 0.03;
pub const SMILE_COOLDOWN_US: u64 = 120 * SECOND_US;
/// Someone low: the lamp keeps them company quietly for two minutes, its
/// energy a step lower, unless they smile or nobody is near.
pub const QUIET_US: u64 = 2 * MINUTE_US;
const QUIET_REDUCTION: f64 = 0.10;
/// How a line sounded, from the voice's `voice/tone` (`LAMP_MOOD_TONE`):
/// glad words count as a smile, down words as someone low, and a sure,
/// lively voice with neither as half a smile's energy and no warmth. Every
/// brightening shares the smile's cooldown, so a smile and a glad line
/// together add once.
pub const TONE_GLAD: f32 = 0.5;
pub const TONE_DOWN: f32 = -0.5;
/// A glad line in a flat, calm voice, surely read, isn't taken for glad.
pub const TONE_FLAT: f32 = -0.3;
pub const TONE_LIVELY: f32 = 0.6;
/// How sure the voice must be of its arousal for it to count.
pub const TONE_SURE: f32 = 0.6;
const LIVELY_ENERGY: f64 = SMILE_ENERGY / 2.0;
/// How long glad or lively is said on `mood/state` after what made it
/// (#3691): long enough for the feelings skill, which waits for 30 s with
/// no lamp speech and a minute after a talk, to still see a glad line the
/// lamp answered. Low is said for its two-minute spell ([`QUIET_US`]).
pub const FELT_HOLD_US: u64 = 90 * SECOND_US;

/// What a line's tone counts as, if anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToneKind {
    Glad,
    Low,
    Lively,
}

/// A line's arousal (calm -1 to lively +1), how sure of it, and its words'
/// valence (down -1 to glad +1), as `voice/tone` tells them, against the
/// bars: down words are low, glad words glad unless the voice was surely
/// flat, and with no such words a surely lively voice is lively. A value
/// that isn't a number counts for nothing.
pub fn tone_kind(arousal: f32, sure: f32, valence: Option<f32>) -> Option<ToneKind> {
    let surely = |bar: bool| sure.is_finite() && sure >= TONE_SURE && arousal.is_finite() && bar;
    match valence {
        Some(valence) if valence <= TONE_DOWN => Some(ToneKind::Low),
        Some(valence) if valence >= TONE_GLAD && !surely(arousal < TONE_FLAT) => {
            Some(ToneKind::Glad)
        }
        None if surely(arousal >= TONE_LIVELY) => Some(ToneKind::Lively),
        _ => None,
    }
}

/// An add-on's nudge moves energy or warmth this much at most (#2530, the
/// first builder's bound), inside the clamps that hold without it...
pub const NUDGE_MOST: f64 = 0.1;
/// ...and fades to nothing over its last minute.
pub const NUDGE_FADE_US: u64 = MINUTE_US;

/// The newest kept nudge: what it adds, and until when.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Nudge {
    energy: f64,
    warmth: f64,
    until_us: u64,
}

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
    /// One read of how the near face seems (#1671, the eyes'
    /// `humans/seems`), `None` for `unsure`.
    Seems(Option<SeemsKind>),
    /// A kept add-on's nudge (#2530, `mood/nudge`), in thousandths: energy
    /// both ways, warmth up only, each [`NUDGE_MOST`] at most, held until
    /// `until_us`.
    Nudge {
        energy_milli: i16,
        warmth_milli: u16,
        until_us: u64,
    },
    /// How a line sounded (#1722, the voice's `voice/tone`), as it counts
    /// ([`tone_kind`]).
    Tone(ToneKind),
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
            Self::Seems(_) => 12,
            Self::Nudge { .. } => 13,
            Self::Tone(_) => 14,
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
    /// The reads in a row of one kind (#1671): only a count, never whose.
    seems_run: Option<(SeemsKind, u8)>,
    last_smile_us: Option<u64>,
    /// Until when the lamp keeps someone low company quietly.
    quiet_until_us: Option<u64>,
    /// The lamp's one breath, from 0 to 1 (#1670): begun where the plain
    /// breath is, moved on at the pace whenever time does, never jumped.
    breath_phase: f64,
    /// The newest add-on nudge (#2530); none after it ends or in sleep.
    nudge: Option<Nudge>,
    /// The newest feeling (#3691), replaced by the next; none in sleep.
    felt: Option<Felt>,
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
            seems_run: None,
            last_smile_us: None,
            quiet_until_us: None,
            breath_phase: plain_breath_phase(now_us),
            nudge: None,
            felt: None,
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
            breath: Some(MoodBreath {
                // A phase that rounds up to 1 as f32 is 0.
                phase: (self.breath_phase as f32).min(1.0 - f32::EPSILON),
                pace: self.pace(),
            }),
            felt: self.felt_at(now_us),
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

    /// The feeling while it holds: low only while its quiet spell does,
    /// which ends early when nobody is near.
    fn felt_at(&self, at_us: u64) -> Option<Felt> {
        self.felt.filter(|felt| {
            at_us < felt.until_us && (felt.kind != FeltKind::Low || self.quiet_at(at_us))
        })
    }

    /// A feeling seen at `observed`, replacing any older one.
    fn feel(&mut self, kind: FeltKind, until_us: u64) {
        self.felt = Some(Felt { kind, until_us });
    }

    fn pace(&self) -> f32 {
        mood_pace(
            self.awake.clamp(0.0, 1.0) as f32,
            self.energy() as f32,
            self.asleep,
        )
    }

    fn energy(&self) -> f64 {
        let quiet = if self.quiet_at(self.now_us) {
            QUIET_REDUCTION
        } else {
            0.0
        };
        let (nudge, _) = self.nudge_at(self.now_us);
        (BASE_ENERGY - self.air_reduction - quiet + self.energy_pulse + nudge)
            .clamp(0.0, ENERGY_CAP)
    }

    /// Keeping someone low company: while its two minutes last and a face
    /// is near.
    fn quiet_at(&self, at_us: u64) -> bool {
        self.face.is_some() && self.quiet_until_us.is_some_and(|until| at_us < until)
    }

    fn warmth(&self) -> f64 {
        let (_, nudge) = self.nudge_at(self.now_us);
        (BASE_WARMTH + self.warmth_pulse + nudge).clamp(BASE_WARMTH, WARMTH_CAP)
    }

    /// What the nudge adds to energy and warmth at `at_us`: all of it until
    /// its last minute, then less, to nothing at `until_us`.
    fn nudge_at(&self, at_us: u64) -> (f64, f64) {
        let Some(nudge) = self.nudge.filter(|nudge| at_us < nudge.until_us) else {
            return (0.0, 0.0);
        };
        let share = ((nudge.until_us - at_us) as f64 / NUDGE_FADE_US as f64).min(1.0);
        (nudge.energy * share, nudge.warmth * share)
    }

    fn changed(&self, next: &Self) -> bool {
        self.asleep != next.asleep
            || self.attention.as_ref().map(|focus| &focus.target)
                != next.attention.as_ref().map(|focus| &focus.target)
            || (self.awake - next.awake).abs() >= CHANGE_DELTA
            || (self.energy() - next.energy()).abs() >= CHANGE_DELTA
            || (self.warmth() - next.warmth()).abs() >= CHANGE_DELTA
            || self.familiar_at(self.now_us) != next.familiar_at(next.now_us)
            || self.felt_at(self.now_us) != next.felt_at(next.now_us)
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
                    // A hand on the lamp while someone is low is comfort,
                    // not glad: only a feeling that ends the spell replaces low.
                    if !self.quiet_at(observed) {
                        self.feel(FeltKind::Glad, observed.saturating_add(FELT_HOLD_US));
                    }
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
                // Nobody near: how they seemed goes with them.
                if face.is_none() {
                    self.seems_run = None;
                    self.quiet_until_us = None;
                }
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
            EventKind::Seems(kind) => self.seems(now_us, observed, kind),
            // Asleep, the lamp takes no nudge; the judge keeps none then.
            EventKind::Nudge {
                energy_milli,
                warmth_milli,
                until_us,
            } if !self.asleep => {
                let most = |value: f64| value.clamp(-NUDGE_MOST, NUDGE_MOST);
                self.nudge = Some(Nudge {
                    energy: most(f64::from(energy_milli) / 1_000.0),
                    warmth: most(f64::from(warmth_milli) / 1_000.0),
                    until_us,
                });
            }
            EventKind::Nudge { .. } => {}
            EventKind::Tone(kind) => self.tone(now_us, observed, kind),
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

    /// One read of how the near face seems (#1671). A smile brightens the
    /// lamp a little for some seconds; someone low quiets it for two
    /// minutes. Only the count of reads in a row is kept, never a read.
    fn seems(&mut self, now_us: u64, observed: u64, kind: Option<SeemsKind>) {
        if self.asleep {
            self.seems_run = None;
            return;
        }
        let kind = kind.filter(|kind| *kind != SeemsKind::Unsure);
        self.seems_run = kind.map(|kind| match self.seems_run {
            Some((previous, reads)) if previous == kind => (kind, reads.saturating_add(1)),
            _ => (kind, 1),
        });
        match self.seems_run {
            Some((SeemsKind::Positive, SMILE_READS)) => {
                self.quiet_until_us = None;
                self.feel(FeltKind::Glad, observed.saturating_add(FELT_HOLD_US));
                self.brighten(now_us, observed, SMILE_ENERGY, SMILE_WARMTH);
            }
            Some((SeemsKind::Negative, LOW_READS)) => self.keep_quiet(observed),
            _ => {}
        }
    }

    /// How a line sounded (#1722): a glad line is a smile, a low one
    /// someone low, and a lively one half a smile's energy, as the face's
    /// are; a lively line in a low spell isn't felt over low. A low line with no face near is nothing, and isn't kept for a
    /// face that comes after (the reviewer-lead's P3 on #2570). Nothing of
    /// the line is kept.
    fn tone(&mut self, now_us: u64, observed: u64, kind: ToneKind) {
        if self.asleep {
            return;
        }
        match kind {
            ToneKind::Glad => {
                self.quiet_until_us = None;
                self.feel(FeltKind::Glad, observed.saturating_add(FELT_HOLD_US));
                self.brighten(now_us, observed, SMILE_ENERGY, SMILE_WARMTH);
            }
            ToneKind::Lively => {
                // Keeping someone low company wins over a lively voice,
                // which may not be theirs: only a feeling that ends the
                // spell replaces low.
                if !self.quiet_at(observed) {
                    self.feel(FeltKind::Lively, observed.saturating_add(FELT_HOLD_US));
                }
                self.brighten(now_us, observed, LIVELY_ENERGY, 0.0);
            }
            ToneKind::Low if self.face.is_some() => self.keep_quiet(observed),
            ToneKind::Low => {}
        }
    }

    /// A smile, or a glad or lively line: `energy` and `warmth`, at most once
    /// in [`SMILE_COOLDOWN_US`] between them all.
    fn brighten(&mut self, now_us: u64, observed: u64, energy: f64, warmth: f64) {
        if self
            .last_smile_us
            .is_some_and(|last| observed.saturating_sub(last) < SMILE_COOLDOWN_US)
        {
            return;
        }
        self.last_smile_us = Some(observed);
        self.energy_pulse = (self.energy_pulse
            + energy * decay(now_us - observed, ENERGY_HALF_LIFE_US))
        .min(ENERGY_CAP - BASE_ENERGY + self.air_reduction);
        if warmth > 0.0 {
            self.warm(observed, warmth);
        }
    }

    /// Someone low, by their face or their words: two minutes' quiet while a
    /// face is near, felt as low, and a startle's energy let go.
    fn keep_quiet(&mut self, observed: u64) {
        let until_us = observed.saturating_add(QUIET_US);
        self.quiet_until_us = Some(until_us);
        self.feel(FeltKind::Low, until_us);
        self.energy_pulse = 0.0;
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
        self.seems_run = None;
        self.quiet_until_us = None;
        self.nudge = None;
        self.felt = None;
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
            // Nobody near: how they seemed goes with them (#1671).
            self.seems_run = None;
            self.quiet_until_us = None;
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
        // The breath goes on at the pace the lamp had over this interval:
        // its state at the interval's start, as every reader extrapolates
        // from the last state said.
        let elapsed = now_us.saturating_sub(self.now_us) as f64;
        self.breath_phase =
            (self.breath_phase + elapsed / MOOD_BREATH_US as f64 * f64::from(self.pace())).fract();
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
