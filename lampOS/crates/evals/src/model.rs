//! Closed internal evidence, deliberately separate from any producer wire format.
//!
//! `VoiceDeliveryV1` versions this aggregation rubric. It is NOT the version of
//! `TurnRecord` (whose reviewed adapter is not part of this crate yet).
use serde::{Deserialize, Deserializer, Serialize};

/// Thirty UTC days; this is a retention bound, never a coverage claim.
pub const WINDOW_HOURS: u64 = 30 * 24;
pub const MAX_SEGMENTS: usize = 4096;
pub const MAX_EPOCHS: usize = 1024;
pub const MAX_RANGES: usize = 4096;
pub const MAX_CHECKPOINT_BYTES: usize = 8 * 1024 * 1024;
/// A separate trusted caller may advance at most one day per call.
pub const MAX_WINDOW_ADVANCE_HOURS: u64 = 24;
/// Exclusive end of 9999-12-31 in Unix hours.
pub const MAX_WINDOW_END_HOUR: u64 = 70_389_528;
/// Same edges as the cloud report histogram; the boundary is different here.
/// A measurement enters the first bin whose edge is strictly greater than it.
pub const FIRST_WRITE_EDGES_MS: [u32; 6] = [500, 1000, 1500, 2000, 3000, 5000];

/// Validated product build identity. No free-form string can enter the model.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct Build(String);

impl Build {
    pub fn parse(value: &str) -> Result<Self, crate::core::Error> {
        let invalid = crate::core::Error::InvalidBuild;
        if value.len() > 61 {
            return Err(invalid);
        }
        let (sequence, commit) = value.split_once('+').ok_or(invalid)?;
        if sequence.is_empty()
            || sequence.len() > 20
            || !sequence.bytes().all(|byte| byte.is_ascii_digit())
            || !(7..=40).contains(&commit.len())
            || !commit
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(invalid);
        }
        let parsed = sequence.parse::<u64>().map_err(|_| invalid)?;
        if parsed == 0 || parsed.to_string() != sequence {
            return Err(invalid);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Build {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(|_| serde::de::Error::custom("invalid_build"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceTime {
    /// The adapter may provide this only when the source clock is trusted.
    UtcMinute(u64),
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Microphone,
    Reminder,
    Proactive,
    Text,
    Computer,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Contract {
    /// Internal aggregation/rubric version, not the producer's wire version.
    VoiceDeliveryV1,
}

/// Producer terminal evidence; `Completed` means software delivery, not answer
/// correctness, acoustic audibility, user satisfaction, or a missed-call rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Terminal {
    Completed,
    PlaybackGap,
    NoAudio,
    ProviderError,
    Deadline,
    Fault,
    PersonInterrupt,
    PrivacyCancel,
    OtherCancel,
    Unknown,
}

impl Terminal {
    pub const ALL: [Self; 10] = [
        Self::Completed,
        Self::PlaybackGap,
        Self::NoAudio,
        Self::ProviderError,
        Self::Deadline,
        Self::Fault,
        Self::PersonInterrupt,
        Self::PrivacyCancel,
        Self::OtherCancel,
        Self::Unknown,
    ];

    pub(crate) fn index(self) -> usize {
        match self {
            Self::Completed => 0,
            Self::PlaybackGap => 1,
            Self::NoAudio => 2,
            Self::ProviderError => 3,
            Self::Deadline => 4,
            Self::Fault => 5,
            Self::PersonInterrupt => 6,
            Self::PrivacyCancel => 7,
            Self::OtherCancel => 8,
            Self::Unknown => 9,
        }
    }
}

/// One admitted internal observation. There are intentionally no words, names,
/// per-person identifiers, payload maps or external protocol serialization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observation {
    /// Caller-owned local producer lifetime, unique across producer restarts.
    pub epoch: u64,
    pub turn: u64,
    pub time: SourceTime,
    pub build: Option<Build>,
    pub origin: Origin,
    pub terminal: Terminal,
    pub audio_started: Option<bool>,
    /// Endpoint DECISION to first speaker WRITE. Not acoustic end-of-speech
    /// latency. A value requires audio_started == Some(true).
    pub endpoint_to_first_write_ms: Option<u32>,
    pub contract: Contract,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RejectionCounts {
    pub expired: u64,
    pub future: u64,
    pub invalid_time: u64,
    pub invalid_evidence: u64,
    pub segment_capacity: u64,
    pub epoch_capacity: u64,
    pub dedupe_capacity: u64,
}

impl RejectionCounts {
    pub(crate) fn total(self) -> Option<u64> {
        [
            self.expired,
            self.future,
            self.invalid_time,
            self.invalid_evidence,
            self.segment_capacity,
            self.epoch_capacity,
            self.dedupe_capacity,
        ]
        .into_iter()
        .try_fold(0_u64, u64::checked_add)
    }
}

/// Lifetime receipt attempts, including replay/rejection. These are not
/// interaction opportunities. `retired` counts previously accepted records
/// removed by the retention clock, not failures. If u64 counters exhaust,
/// subsequent attempts are refused and only `exhausted` records that fact.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptCounts {
    pub calls: u64,
    pub accepted: u64,
    pub duplicates: u64,
    pub rejected: RejectionCounts,
    pub retired: u64,
    pub exhausted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TimingCounts {
    pub boundary: &'static str,
    pub edges_ms: [u32; 6],
    pub bins: [u64; 7],
    pub samples: u64,
    /// Audio explicitly started, but the timing boundary was not recorded.
    pub unavailable: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OutcomeCounts {
    pub terminal: Terminal,
    pub total: u64,
    pub audio_started: u64,
    pub audio_not_started: u64,
    pub audio_unknown: u64,
    pub timing: TimingCounts,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AggregateCounts {
    pub total: u64,
    /// All closed terminal partitions, including zero counts and unknowns.
    pub outcomes: Vec<OutcomeCounts>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HourlyAggregate {
    pub utc_hour: u64,
    /// Closed by the trusted retention clock; does NOT mean fully observed.
    pub closed: bool,
    pub build: Option<Build>,
    pub origin: Origin,
    pub contract: Contract,
    pub counts: AggregateCounts,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DailyAggregate {
    /// A UTC calendar day strictly before the current/open hour's day.
    pub utc_day: u64,
    pub build: Option<Build>,
    pub origin: Origin,
    pub contract: Contract,
    /// Distinct nonempty closed hour buckets contributing to this completed
    /// UTC day. Rolling retention can clip its oldest hours. Neither this
    /// number nor 24 establishes complete event observation.
    pub observed_closed_hours: u8,
    pub coverage: Coverage,
    pub counts: AggregateCounts,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UnanchoredAggregate {
    pub build: Option<Build>,
    pub origin: Origin,
    pub contract: Contract,
    pub counts: AggregateCounts,
}

/// Aggregate-only view. No epoch, turn, source minute, receipt cohort, or
/// individual event time is serializable through this type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    pub schema: &'static str,
    pub window_start_hour: u64,
    /// Exclusive end of the CURRENT/open hour. Its entire UTC day is excluded
    /// from daily evidence until a separate trusted advance closes that day.
    pub window_end_hour: u64,
    pub coverage: Coverage,
    pub opportunity_coverage: Coverage,
    pub receipts: ReceiptCounts,
    pub hourly: Vec<HourlyAggregate>,
    pub daily: Vec<DailyAggregate>,
    pub unanchored: Vec<UnanchoredAggregate>,
}
