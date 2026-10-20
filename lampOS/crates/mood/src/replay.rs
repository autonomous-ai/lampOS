//! A complete authored day, not a recorded day or a latency measurement.
//! All inputs and clock observations are synthetic metadata. No I/O is needed.
use crate::engine::{Engine, Event, EventKind, FaceTarget, Reject, SECOND_US};
use lamp_msgs::MoodState;
use serde::Serialize;
use std::{collections::BTreeMap, fmt};

pub const DAY_SECONDS: u64 = 24 * 60 * 60;
pub const SAMPLE_SECONDS: u64 = 60;
pub const SAMPLE_COUNT: usize = 1_441;
pub const MAX_EVENTS: u32 = 40_000;
pub const PROVENANCE: &str = "synthetic_authored";
const CLOCK_ID: &str = "synthetic-day-clock";
const SESSION_ID: &str = "synthetic-day-session";
const FACE_BOOT: &str = "0000000000000035";
const FACE_ID: &str = "synthetic-current-face";

const fn at(hour: u64, minute: u64) -> u64 {
    (hour * 60 + minute) * 60
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Interval {
    pub start_s: u64,
    pub end_s: u64,
}

// Half-open intervals; a Faces(None) observation is supplied at each end.
const COMPANY: &[Interval] = &[
    Interval {
        start_s: at(7, 0),
        end_s: at(10, 0),
    },
    Interval {
        start_s: at(11, 15),
        end_s: at(12, 0),
    },
    Interval {
        start_s: at(13, 30),
        end_s: at(14, 15),
    },
    Interval {
        start_s: at(16, 0),
        end_s: at(19, 30),
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct AirInterval {
    pub start_s: u64,
    pub end_s: u64,
    pub co2_ppm: u16,
}

const AIR: &[AirInterval] = &[
    AirInterval {
        start_s: at(14, 0),
        end_s: at(14, 30),
        co2_ppm: 1_400,
    },
    AirInterval {
        start_s: at(14, 30),
        end_s: at(14, 45),
        co2_ppm: 600,
    },
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ScheduledEvent {
    pub elapsed_s: u64,
    pub cue: EventKind,
    pub purpose: &'static str,
}

// Sorted, complete point-event provenance. Thanks/laughter here are authored
// engine inputs, not claims that a live semantic producer exists.
const EVENTS: &[ScheduledEvent] = &[
    ScheduledEvent {
        elapsed_s: at(1, 0),
        cue: EventKind::SuddenSound,
        purpose: "night noise cannot wake or add energy",
    },
    ScheduledEvent {
        elapsed_s: at(7, 5),
        cue: EventKind::SuddenSound,
        purpose: "dawn and company still cannot wake",
    },
    ScheduledEvent {
        elapsed_s: at(7, 10),
        cue: EventKind::Thanks,
        purpose: "replay-only feedback cannot wake or warm asleep state",
    },
    ScheduledEvent {
        elapsed_s: at(7, 11),
        cue: EventKind::Laughter,
        purpose: "replay-only laughter cannot wake or warm asleep state",
    },
    ScheduledEvent {
        elapsed_s: at(7, 12),
        cue: EventKind::Heard,
        purpose: "speech activity without words cannot wake",
    },
    ScheduledEvent {
        elapsed_s: at(7, 13),
        cue: EventKind::Voice(true),
        purpose: "active Voice lease cannot wake",
    },
    ScheduledEvent {
        elapsed_s: at(8, 0),
        cue: EventKind::Name,
        purpose: "name wakes and may attend the current synthetic face",
    },
    ScheduledEvent {
        elapsed_s: at(11, 5),
        cue: EventKind::SuddenSound,
        purpose: "noise after quiet dozing cannot wake",
    },
    ScheduledEvent {
        elapsed_s: at(11, 15),
        cue: EventKind::Touch { kind: false },
        purpose: "ordinary touch wakes without a warmth nudge",
    },
    ScheduledEvent {
        elapsed_s: at(13, 5),
        cue: EventKind::SuddenSound,
        purpose: "midday sleep ignores incidental noise",
    },
    ScheduledEvent {
        elapsed_s: at(13, 30),
        cue: EventKind::UserReminder,
        purpose: "user-set reminder wakes without a target",
    },
    ScheduledEvent {
        elapsed_s: at(13, 31),
        cue: EventKind::Touch { kind: true },
        purpose: "kindly touch nudges warmth",
    },
    ScheduledEvent {
        elapsed_s: at(13, 32),
        cue: EventKind::Thanks,
        purpose: "replay-only explicit thanks nudges warmth",
    },
    ScheduledEvent {
        elapsed_s: at(13, 33),
        cue: EventKind::Laughter,
        purpose: "replay-only explicit laughter nudges warmth",
    },
    ScheduledEvent {
        elapsed_s: at(14, 45),
        cue: EventKind::Air(None),
        purpose: "missing air is no longer a fresh measurement",
    },
    ScheduledEvent {
        elapsed_s: at(16, 0),
        cue: EventKind::Touch { kind: false },
        purpose: "touch wakes after another quiet interval",
    },
    ScheduledEvent {
        elapsed_s: at(17, 0),
        cue: EventKind::SuddenSound,
        purpose: "awake bang briefly raises energy",
    },
    ScheduledEvent {
        elapsed_s: at(17, 0) + 30,
        cue: EventKind::SuddenSound,
        purpose: "repeat bang within cooldown adds no pulse",
    },
    ScheduledEvent {
        elapsed_s: at(17, 2),
        cue: EventKind::SuddenSound,
        purpose: "bang after cooldown can add a new pulse",
    },
    ScheduledEvent {
        elapsed_s: at(22, 5),
        cue: EventKind::SuddenSound,
        purpose: "night noise leaves the sleep latch closed",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Phase {
    pub id: &'static str,
    pub start_s: u64,
    pub end_s: u64,
    pub purpose: &'static str,
}

const PHASES: &[Phase] = &[
    Phase {
        id: "midnight_rest",
        start_s: 0,
        end_s: at(7, 0),
        purpose: "night startup asleep; incidental noise ignored",
    },
    Phase {
        id: "dawn_company",
        start_s: at(7, 0),
        end_s: at(8, 0),
        purpose: "day target and company do not clear sleep",
    },
    Phase {
        id: "morning_company",
        start_s: at(8, 0),
        end_s: at(10, 0),
        purpose: "name wake followed by continuous fresh company",
    },
    Phase {
        id: "morning_quiet",
        start_s: at(10, 0),
        end_s: at(11, 15),
        purpose: "company leaves, awake falls and quiet sleep latches",
    },
    Phase {
        id: "touch_company",
        start_s: at(11, 15),
        end_s: at(12, 0),
        purpose: "ordinary touch wakes, company sustains wakefulness",
    },
    Phase {
        id: "midday_quiet",
        start_s: at(12, 0),
        end_s: at(13, 30),
        purpose: "long quiet dozes; noise cannot wake",
    },
    Phase {
        id: "reminder_and_feedback",
        start_s: at(13, 30),
        end_s: at(14, 0),
        purpose: "reminder wake and authored positive feedback",
    },
    Phase {
        id: "sustained_air",
        start_s: at(14, 0),
        end_s: at(14, 30),
        purpose: "fresh high CO2 qualifies for ten minutes, then lowers energy",
    },
    Phase {
        id: "air_clearing",
        start_s: at(14, 30),
        end_s: at(14, 45),
        purpose: "fresh clear air lets the offset decay",
    },
    Phase {
        id: "afternoon_quiet",
        start_s: at(14, 45),
        end_s: at(16, 0),
        purpose: "missing air, warmth decay and quiet sleep",
    },
    Phase {
        id: "afternoon_company",
        start_s: at(16, 0),
        end_s: at(19, 0),
        purpose: "touch wake, company and bounded awake bang pulses",
    },
    Phase {
        id: "evening_company",
        start_s: at(19, 0),
        end_s: at(19, 30),
        purpose: "evening target lowers awake despite company",
    },
    Phase {
        id: "evening_quiet",
        start_s: at(19, 30),
        end_s: at(22, 0),
        purpose: "quiet settling returns to sleep",
    },
    Phase {
        id: "night_rest",
        start_s: at(22, 0),
        end_s: DAY_SECONDS,
        purpose: "night remains asleep through noise and midnight",
    },
];

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ReplayMetadata {
    pub provenance: &'static str,
    pub scenario: &'static str,
    pub duration_s: u64,
    pub step_s: u64,
    pub sample_interval_s: u64,
    pub monotonic_origin_us: u64,
    pub clock_id: &'static str,
    pub session_id: &'static str,
    pub local_clock: &'static str,
    pub face_publisher: &'static str,
    pub face_id: &'static str,
    pub company_refresh_s: u64,
    pub company_lease_s: u64,
    pub company_windows: &'static [Interval],
    pub company_end: &'static str,
    pub air_refresh_s: u64,
    pub air_lease_s: u64,
    pub air_windows: &'static [AirInterval],
    pub scheduled_events: &'static [ScheduledEvent],
    pub simultaneous_event_order: [&'static str; 3],
    pub event_limit: u32,
    pub sample_limit: usize,
    pub limitation: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Sample {
    pub provenance: &'static str,
    pub elapsed_s: u64,
    pub local_minute: u16,
    pub phase: &'static str,
    #[serde(flatten)]
    pub state: MoodState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReplaySummary {
    pub provenance: &'static str,
    pub samples: usize,
    pub applied_events: u32,
    pub meaningful_event_updates: u32,
    pub event_counts: BTreeMap<&'static str, u32>,
    pub count_meaning: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Replay {
    pub metadata: ReplayMetadata,
    pub phases: &'static [Phase],
    pub samples: Vec<Sample>,
    pub summary: ReplaySummary,
}

/// Flat, tagged JSONL records. The example writes these to stdout only.
#[derive(Serialize)]
#[serde(tag = "record", rename_all = "snake_case")]
pub enum ReplayRecord<'a> {
    Metadata(&'a ReplayMetadata),
    Phase(&'a Phase),
    Sample(&'a Sample),
    Summary(&'a ReplaySummary),
}

#[derive(Debug)]
pub enum ReplayError {
    Engine(Reject),
    Bound(&'static str),
}

impl fmt::Display for ReplayError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Engine(error) => write!(out, "authored replay engine: {error}"),
            Self::Bound(bound) => write!(out, "authored replay exceeded {bound}"),
        }
    }
}

impl std::error::Error for ReplayError {}

impl From<Reject> for ReplayError {
    fn from(value: Reject) -> Self {
        Self::Engine(value)
    }
}

/// Run exactly one authored local day. Every elapsed second supplies a local
/// minute observation; this preserves day transitions without wall-clock I/O.
/// Sampling happens after that second's observations, including at 24:00.
pub fn run_authored_day() -> Result<Replay, ReplayError> {
    let mut engine = Engine::new(CLOCK_ID.into(), SECOND_US, Some(0))?;
    let mut samples = Vec::with_capacity(SAMPLE_COUNT);
    let mut summary = ReplaySummary {
        provenance: PROVENANCE,
        samples: 0,
        applied_events: 0,
        meaningful_event_updates: 0,
        event_counts: BTreeMap::new(),
        count_meaning: "valid engine inputs, including sleep/cooldown no-ops; not publications or measured latency",
    };
    let mut next_event = 0;

    for elapsed_s in 0..=DAY_SECONDS {
        let now_us = SECOND_US + elapsed_s * SECOND_US;
        let local_minute = ((elapsed_s / 60) % 1_440) as u16;
        engine.advance(now_us, Some(local_minute))?;

        if COMPANY
            .iter()
            .any(|window| contains(window.start_s, window.end_s, elapsed_s))
        {
            apply(
                &mut engine,
                now_us,
                2,
                EventKind::Faces(Some(FaceTarget {
                    publisher_boot: FACE_BOOT.into(),
                    id: FACE_ID.into(),
                })),
                &mut summary,
            )?;
        } else if COMPANY.iter().any(|window| elapsed_s == window.end_s) {
            apply(&mut engine, now_us, 2, EventKind::Faces(None), &mut summary)?;
        }

        if elapsed_s.is_multiple_of(5)
            && let Some(window) = AIR
                .iter()
                .find(|window| contains(window.start_s, window.end_s, elapsed_s))
        {
            apply(
                &mut engine,
                now_us,
                15,
                EventKind::Air(Some(window.co2_ppm)),
                &mut summary,
            )?;
        }

        while let Some(event) = EVENTS
            .get(next_event)
            .filter(|event| event.elapsed_s == elapsed_s)
        {
            let lease_s = if matches!(event.cue, EventKind::Voice(_)) {
                3
            } else {
                1
            };
            apply(
                &mut engine,
                now_us,
                lease_s,
                event.cue.clone(),
                &mut summary,
            )?;
            next_event += 1;
        }

        if elapsed_s.is_multiple_of(SAMPLE_SECONDS) {
            if samples.len() >= SAMPLE_COUNT {
                return Err(ReplayError::Bound("sample limit"));
            }
            let phase = PHASES
                .iter()
                .find(|phase| contains(phase.start_s, phase.end_s, elapsed_s))
                .unwrap_or(&PHASES[PHASES.len() - 1]);
            samples.push(Sample {
                provenance: PROVENANCE,
                elapsed_s,
                local_minute,
                phase: phase.id,
                state: engine.snapshot(now_us, samples.len() as u64 + 1, SESSION_ID)?,
            });
        }
    }
    summary.samples = samples.len();
    Ok(Replay {
        metadata: ReplayMetadata {
            provenance: PROVENANCE,
            scenario: "task35-authored-day-v1",
            duration_s: DAY_SECONDS,
            step_s: 1,
            sample_interval_s: SAMPLE_SECONDS,
            monotonic_origin_us: SECOND_US,
            clock_id: CLOCK_ID,
            session_id: SESSION_ID,
            local_clock: "(elapsed_s / 60) modulo 1440; fixed authored day, no timezone or wall-clock reading",
            face_publisher: FACE_BOOT,
            face_id: FACE_ID,
            company_refresh_s: 1,
            company_lease_s: 2,
            company_windows: COMPANY,
            company_end: "explicit Faces(None) at every half-open window end",
            air_refresh_s: 5,
            air_lease_s: 15,
            air_windows: AIR,
            scheduled_events: EVENTS,
            simultaneous_event_order: ["company", "air", "scheduled_events"],
            event_limit: MAX_EVENTS,
            sample_limit: SAMPLE_COUNT,
            limitation: "authored synthetic metadata only; thanks/laughter have no live producer; not a recorded day, user data, device run or latency measurement",
        },
        phases: PHASES,
        samples,
        summary,
    })
}

fn contains(start_s: u64, end_s: u64, elapsed_s: u64) -> bool {
    start_s <= elapsed_s && elapsed_s < end_s
}

fn apply(
    engine: &mut Engine,
    now_us: u64,
    lease_s: u64,
    kind: EventKind,
    summary: &mut ReplaySummary,
) -> Result<(), ReplayError> {
    if summary.applied_events >= MAX_EVENTS {
        return Err(ReplayError::Bound("event limit"));
    }
    let label = match &kind {
        EventKind::Name => "name",
        EventKind::Touch { kind: true } => "kindly_touch",
        EventKind::Touch { kind: false } => "ordinary_touch",
        EventKind::UserReminder => "user_reminder",
        EventKind::Sleep => "sleep",
        EventKind::SuddenSound => "sudden_sound",
        EventKind::Thanks => "thanks_replay_only",
        EventKind::Laughter => "laughter_replay_only",
        EventKind::Faces(_) => "faces",
        EventKind::Voice(_) => "voice",
        EventKind::Heard => "heard_activity",
        EventKind::Air(_) => "air",
        EventKind::Familiar(_) => "familiar",
        EventKind::Seems(_) => "seems",
    };
    let changed = engine.apply(
        now_us,
        Event {
            observed_us: now_us,
            valid_until_us: now_us + lease_s * SECOND_US,
            kind,
        },
    )?;
    summary.applied_events += 1;
    summary.meaningful_event_updates += u32::from(changed);
    *summary.event_counts.entry(label).or_default() += 1;
    Ok(())
}
