//! Bounded, pure-memory recorder. No runtime producer or wire adapter is enabled.
//!
//! Dedupe and attribution share one versioned checkpoint. A range is merged
//! only when neighboring turn IDs have the same aggregate segment. Ranges are
//! never evicted to make room while their records remain retained. Replays use
//! local (epoch, turn) identity; the first accepted observation owns that ID.
//! Unknown-time receipts use a private retention cohort, never an event hour.
//! They cannot establish a unique all-time count after that cohort expires.
//!
//! `Contract::VoiceDeliveryV1` is this internal rubric, NOT TurnRecord's wire
//! version. Mapping the reviewed producer contract remains separate work.
use crate::model::*;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const VERSION: u16 = 1;
const SNAPSHOT_SCHEMA: &str = "lamp-eval-aggregates/1";
const TERMINALS: usize = Terminal::ALL.len();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidWindow,
    BackwardWindow,
    WindowJump,
    InvalidBuild,
    CheckpointTooLarge,
    InvalidCheckpoint,
    ArithmeticOverflow,
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidWindow => "invalid_window",
            Self::BackwardWindow => "backward_window",
            Self::WindowJump => "window_jump",
            Self::InvalidBuild => "invalid_build",
            Self::CheckpointTooLarge => "checkpoint_too_large",
            Self::InvalidCheckpoint => "invalid_checkpoint",
            Self::ArithmeticOverflow => "arithmetic_overflow",
        })
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    Expired,
    Future,
    InvalidTime,
    InvalidEvidence,
    SegmentCapacity,
    EpochCapacity,
    DedupeCapacity,
    CounterOverflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ingest {
    Accepted,
    Duplicate,
    Rejected(Rejection),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TimeKind {
    SourceHour,
    ReceiptCohort,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SegmentKey {
    time_kind: TimeKind,
    /// Source hour, or INTERNAL receipt cohort when source time is unknown.
    hour: u64,
    build: Option<Build>,
    origin: Origin,
    contract: Contract,
}

/// Per terminal: audio-started, audio-not-started, audio-unknown, then seven
/// timing bins. Redundant totals are derived, never trusted on restoration.
/// Compact arrays keep every legal capped state below the checkpoint bound.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
struct Counts([[u64; 10]; TERMINALS]);

impl Counts {
    fn total(&self) -> Option<u64> {
        self.0
            .iter()
            .flat_map(|row| row[..3].iter().copied())
            .try_fold(0_u64, u64::checked_add)
    }

    fn valid(&self) -> bool {
        self.total().is_some()
            && self.0.iter().all(|row| {
                row[3..]
                    .iter()
                    .copied()
                    .try_fold(0_u64, u64::checked_add)
                    .is_some_and(|timed| timed <= row[0])
            })
    }

    fn observe(&mut self, observation: &Observation) -> Result<(), Error> {
        let row = &mut self.0[observation.terminal.index()];
        let audio = match observation.audio_started {
            Some(true) => 0,
            Some(false) => 1,
            None => 2,
        };
        row[audio] = row[audio].checked_add(1).ok_or(Error::ArithmeticOverflow)?;
        if let Some(milliseconds) = observation.endpoint_to_first_write_ms {
            let bin = FIRST_WRITE_EDGES_MS
                .iter()
                .position(|edge| milliseconds < *edge)
                .unwrap_or(FIRST_WRITE_EDGES_MS.len());
            row[3 + bin] = row[3 + bin]
                .checked_add(1)
                .ok_or(Error::ArithmeticOverflow)?;
        }
        Ok(())
    }

    fn merge(&mut self, other: &Self) {
        for (left, right) in self.0.iter_mut().zip(other.0.iter()) {
            for (left, right) in left.iter_mut().zip(right.iter()) {
                *left = left.checked_add(*right).expect("validated retained counts");
            }
        }
    }

    fn snapshot(&self) -> AggregateCounts {
        AggregateCounts {
            total: self.total().expect("validated retained counts"),
            outcomes: Terminal::ALL
                .into_iter()
                .map(|terminal| {
                    let row = self.0[terminal.index()];
                    let bins: [u64; 7] = row[3..].try_into().expect("fixed histogram width");
                    let samples = bins.iter().sum();
                    OutcomeCounts {
                        terminal,
                        total: row[..3].iter().sum(),
                        audio_started: row[0],
                        audio_not_started: row[1],
                        audio_unknown: row[2],
                        timing: TimingCounts {
                            boundary: "endpoint_decision_to_first_speaker_write",
                            edges_ms: FIRST_WRITE_EDGES_MS,
                            bins,
                            samples,
                            unavailable: row[0] - samples,
                        },
                    }
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Segment {
    key: SegmentKey,
    counts: Counts,
}

/// Inclusive first/last turn and segment index. Kept in local checkpoints only.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
struct Range(u64, u64, u16);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Epoch {
    epoch: u64,
    #[serde(deserialize_with = "ranges")]
    ranges: Vec<Range>,
}

fn limited<'de, D, T, const N: usize>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Visitor<T, const N: usize>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>, const N: usize> serde::de::Visitor<'de> for Visitor<T, N> {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("bounded_array")
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut sequence: A,
        ) -> Result<Self::Value, A::Error> {
            let mut output = Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(N));
            loop {
                if output.len() == N {
                    if sequence.next_element::<serde::de::IgnoredAny>()?.is_some() {
                        return Err(serde::de::Error::custom("capacity"));
                    }
                    break;
                }
                match sequence.next_element()? {
                    Some(value) => output.push(value),
                    None => break,
                }
            }
            Ok(output)
        }
    }
    deserializer.deserialize_seq(Visitor::<T, N>(std::marker::PhantomData))
}

fn segments<'de, D: Deserializer<'de>>(value: D) -> Result<Vec<Segment>, D::Error> {
    limited::<D, Segment, MAX_SEGMENTS>(value)
}
fn epochs<'de, D: Deserializer<'de>>(value: D) -> Result<Vec<Epoch>, D::Error> {
    limited::<D, Epoch, MAX_EPOCHS>(value)
}
fn ranges<'de, D: Deserializer<'de>>(value: D) -> Result<Vec<Range>, D::Error> {
    limited::<D, Range, MAX_RANGES>(value)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    version: u16,
    window_end_hour: u64,
    receipts: ReceiptCounts,
    #[serde(deserialize_with = "segments")]
    segments: Vec<Segment>,
    #[serde(deserialize_with = "epochs")]
    epochs: Vec<Epoch>,
}

/// Runtime state is not externally deserializable or serializable. Only the
/// validated checkpoint and aggregate-only snapshot APIs cross its boundary.
#[derive(Clone)]
pub struct Recorder {
    state: State,
}

impl Recorder {
    /// `window_end_hour - 1` is the current/open UTC hour. Only the caller's
    /// separate trusted clock can close an hour or retire retained evidence.
    pub fn new(window_end_hour: u64) -> Result<Self, Error> {
        if !(1..=MAX_WINDOW_END_HOUR).contains(&window_end_hour) {
            return Err(Error::InvalidWindow);
        }
        Ok(Self {
            state: State {
                version: VERSION,
                window_end_hour,
                receipts: ReceiptCounts::default(),
                segments: Vec::new(),
                epochs: Vec::new(),
            },
        })
    }

    fn start(&self) -> u64 {
        self.state.window_end_hour.saturating_sub(WINDOW_HOURS)
    }

    fn reject(&mut self, reason: Rejection) -> Ingest {
        let counter = match reason {
            Rejection::Expired => &mut self.state.receipts.rejected.expired,
            Rejection::Future => &mut self.state.receipts.rejected.future,
            Rejection::InvalidTime => &mut self.state.receipts.rejected.invalid_time,
            Rejection::InvalidEvidence => &mut self.state.receipts.rejected.invalid_evidence,
            Rejection::SegmentCapacity => &mut self.state.receipts.rejected.segment_capacity,
            Rejection::EpochCapacity => &mut self.state.receipts.rejected.epoch_capacity,
            Rejection::DedupeCapacity => &mut self.state.receipts.rejected.dedupe_capacity,
            Rejection::CounterOverflow => {
                self.state.receipts.exhausted = true;
                return Ingest::Rejected(reason);
            }
        };
        // observe reserves one receipt first, so a partition cannot overflow.
        *counter += 1;
        Ingest::Rejected(reason)
    }

    /// Bounded memory operations only. The first accepted local identity owns
    /// that identity; duplicates are receipts, not new outcomes. Expired/future
    /// source time is rejected before consulting the retained dedupe set.
    pub fn observe(&mut self, observation: Observation) -> Ingest {
        let Some(next_calls) = self.state.receipts.calls.checked_add(1) else {
            return self.reject(Rejection::CounterOverflow);
        };
        self.state.receipts.calls = next_calls;
        let (time_kind, hour) = match observation.time {
            SourceTime::Unknown => (TimeKind::ReceiptCohort, self.state.window_end_hour - 1),
            SourceTime::UtcMinute(minute) => {
                if minute >= MAX_WINDOW_END_HOUR * 60 {
                    return self.reject(Rejection::InvalidTime);
                }
                let hour = minute / 60;
                if hour < self.start() {
                    return self.reject(Rejection::Expired);
                }
                if hour >= self.state.window_end_hour {
                    return self.reject(Rejection::Future);
                }
                (TimeKind::SourceHour, hour)
            }
        };
        if observation.endpoint_to_first_write_ms.is_some()
            && observation.audio_started != Some(true)
        {
            return self.reject(Rejection::InvalidEvidence);
        }
        let epoch_index = self
            .state
            .epochs
            .binary_search_by_key(&observation.epoch, |epoch| epoch.epoch);
        if let Ok(index) = epoch_index {
            let ranges = &self.state.epochs[index].ranges;
            let at = ranges.partition_point(|range| range.0 <= observation.turn);
            if at > 0 && ranges[at - 1].1 >= observation.turn {
                self.state.receipts.duplicates += 1;
                return Ingest::Duplicate;
            }
        } else if self.state.epochs.len() == MAX_EPOCHS {
            return self.reject(Rejection::EpochCapacity);
        }
        let key = SegmentKey {
            time_kind,
            hour,
            build: observation.build.clone(),
            origin: observation.origin,
            contract: observation.contract,
        };
        let segment_index = self
            .state
            .segments
            .iter()
            .position(|segment| segment.key == key);
        if segment_index.is_none() && self.state.segments.len() == MAX_SEGMENTS {
            return self.reject(Rejection::SegmentCapacity);
        }
        let segment = segment_index.unwrap_or(self.state.segments.len()) as u16;
        let mut next_ranges = match epoch_index {
            Ok(index) => self.state.epochs[index].ranges.clone(),
            Err(_) => Vec::new(),
        };
        let at = next_ranges.partition_point(|range| range.0 < observation.turn);
        next_ranges.insert(at, Range(observation.turn, observation.turn, segment));
        // Normalize both neighbors, allowing an out-of-order ID to join two
        // retained ranges even when the total range capacity is already full.
        let mut at = at;
        if at > 0
            && next_ranges[at - 1].2 == segment
            && next_ranges[at - 1].1.checked_add(1) == Some(next_ranges[at].0)
        {
            next_ranges[at - 1].1 = next_ranges[at].1;
            next_ranges.remove(at);
            at -= 1;
        }
        if at + 1 < next_ranges.len()
            && next_ranges[at + 1].2 == segment
            && next_ranges[at].1.checked_add(1) == Some(next_ranges[at + 1].0)
        {
            next_ranges[at].1 = next_ranges[at + 1].1;
            next_ranges.remove(at + 1);
        }
        let previous = epoch_index.map_or(0, |index| self.state.epochs[index].ranges.len());
        let range_count: usize = self
            .state
            .epochs
            .iter()
            .map(|epoch| epoch.ranges.len())
            .sum();
        if range_count - previous + next_ranges.len() > MAX_RANGES {
            return self.reject(Rejection::DedupeCapacity);
        }
        let mut counts = segment_index
            .map(|index| self.state.segments[index].counts.clone())
            .unwrap_or_default();
        // Every accepted record is also a receipt; total/cell overflow cannot
        // precede the reserved receipt limit in a validated recorder.
        counts
            .observe(&observation)
            .expect("reserved receipt count");
        match segment_index {
            Some(index) => self.state.segments[index].counts = counts,
            None => self.state.segments.push(Segment { key, counts }),
        }
        match epoch_index {
            Ok(index) => self.state.epochs[index].ranges = next_ranges,
            Err(index) => self.state.epochs.insert(
                index,
                Epoch {
                    epoch: observation.epoch,
                    ranges: next_ranges,
                },
            ),
        }
        self.state.receipts.accepted += 1;
        Ingest::Accepted
    }

    pub fn advance_window(&mut self, new_end_hour: u64) -> Result<(), Error> {
        if !(1..=MAX_WINDOW_END_HOUR).contains(&new_end_hour) {
            return Err(Error::InvalidWindow);
        }
        if new_end_hour < self.state.window_end_hour {
            return Err(Error::BackwardWindow);
        }
        if new_end_hour - self.state.window_end_hour > MAX_WINDOW_ADVANCE_HOURS {
            return Err(Error::WindowJump);
        }
        let start = new_end_hour.saturating_sub(WINDOW_HOURS);
        let removed = self
            .state
            .segments
            .iter()
            .filter(|segment| segment.key.hour < start)
            .try_fold(0_u64, |total, segment| {
                total.checked_add(segment.counts.total()?)
            })
            .ok_or(Error::ArithmeticOverflow)?;
        let retired = self
            .state
            .receipts
            .retired
            .checked_add(removed)
            .ok_or(Error::ArithmeticOverflow)?;
        let mut remap = vec![None; self.state.segments.len()];
        let mut next_index = 0_u16;
        for (index, segment) in self.state.segments.iter().enumerate() {
            if segment.key.hour >= start {
                remap[index] = Some(next_index);
                next_index += 1;
            }
        }
        self.state
            .segments
            .retain(|segment| segment.key.hour >= start);
        for epoch in &mut self.state.epochs {
            epoch.ranges.retain_mut(|range| {
                if let Some(index) = remap[usize::from(range.2)] {
                    range.2 = index;
                    true
                } else {
                    false
                }
            });
        }
        self.state.epochs.retain(|epoch| !epoch.ranges.is_empty());
        self.state.receipts.retired = retired;
        self.state.window_end_hour = new_end_hour;
        Ok(())
    }

    pub fn checkpoint(&self) -> Result<Vec<u8>, Error> {
        let bytes = serde_json::to_vec(&self.state).map_err(|_| Error::InvalidCheckpoint)?;
        if bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err(Error::CheckpointTooLarge);
        }
        Ok(bytes)
    }

    pub fn restore(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err(Error::CheckpointTooLarge);
        }
        let state: State = serde_json::from_slice(bytes).map_err(|_| Error::InvalidCheckpoint)?;
        let recorder = Self { state };
        recorder.validate().map_err(|_| Error::InvalidCheckpoint)?;
        Ok(recorder)
    }

    fn validate(&self) -> Result<(), Error> {
        let invalid = Error::InvalidCheckpoint;
        if self.state.version != VERSION
            || !(1..=MAX_WINDOW_END_HOUR).contains(&self.state.window_end_hour)
            || self.state.segments.len() > MAX_SEGMENTS
            || self.state.epochs.len() > MAX_EPOCHS
        {
            return Err(invalid);
        }
        let mut keys = BTreeSet::new();
        for segment in &self.state.segments {
            if segment.key.hour < self.start()
                || segment.key.hour >= self.state.window_end_hour
                || !keys.insert(&segment.key)
                || !segment.counts.valid()
                || segment.counts.total() == Some(0)
            {
                return Err(invalid);
            }
        }
        let mut cardinality = vec![0_u64; self.state.segments.len()];
        let mut previous_epoch = None;
        let mut range_count = 0_usize;
        for epoch in &self.state.epochs {
            if previous_epoch.is_some_and(|previous| previous >= epoch.epoch)
                || epoch.ranges.is_empty()
            {
                return Err(invalid);
            }
            previous_epoch = Some(epoch.epoch);
            let mut previous: Option<Range> = None;
            for range in &epoch.ranges {
                range_count += 1;
                if range_count > MAX_RANGES
                    || range.0 > range.1
                    || usize::from(range.2) >= self.state.segments.len()
                {
                    return Err(invalid);
                }
                if let Some(prior) = previous
                    && (prior.1 >= range.0
                        || (prior.2 == range.2 && prior.1.checked_add(1) == Some(range.0)))
                {
                    return Err(invalid);
                }
                previous = Some(*range);
                let length = range.1.checked_sub(range.0).and_then(|n| n.checked_add(1));
                let index = usize::from(range.2);
                cardinality[index] = cardinality[index]
                    .checked_add(length.ok_or(invalid)?)
                    .ok_or(invalid)?;
            }
        }
        for (segment, identities) in self.state.segments.iter().zip(cardinality) {
            if segment.counts.total() != Some(identities) {
                return Err(invalid);
            }
        }
        let retained = self
            .state
            .segments
            .iter()
            .try_fold(0_u64, |sum, segment| {
                sum.checked_add(segment.counts.total()?)
            })
            .ok_or(invalid)?;
        let receipts = self.state.receipts;
        if retained.checked_add(receipts.retired) != Some(receipts.accepted)
            || receipts
                .accepted
                .checked_add(receipts.duplicates)
                .and_then(|sum| sum.checked_add(receipts.rejected.total()?))
                != Some(receipts.calls)
            || (receipts.exhausted && receipts.calls != u64::MAX)
        {
            return Err(invalid);
        }
        Ok(())
    }

    pub fn snapshot(&self) -> Snapshot {
        type Group = (Option<Build>, Origin, Contract);
        let mut hourly = Vec::new();
        let mut daily: BTreeMap<(u64, Group), (u8, Counts)> = BTreeMap::new();
        let mut unknown: BTreeMap<Group, Counts> = BTreeMap::new();
        for segment in &self.state.segments {
            let key = &segment.key;
            let group = (key.build.clone(), key.origin, key.contract);
            if key.time_kind == TimeKind::ReceiptCohort {
                unknown.entry(group).or_default().merge(&segment.counts);
                continue;
            }
            let closed = key.hour < self.state.window_end_hour - 1;
            hourly.push(HourlyAggregate {
                utc_hour: key.hour,
                closed,
                build: key.build.clone(),
                origin: key.origin,
                contract: key.contract,
                counts: segment.counts.snapshot(),
            });
            // A daily row is available only after the entire UTC day closes.
            // Retention and missed inputs can still make that day's evidence
            // partial; a closed calendar day never establishes coverage.
            if key.hour / 24 < (self.state.window_end_hour - 1) / 24 {
                let (hours, counts) = daily.entry((key.hour / 24, group)).or_default();
                *hours += 1;
                counts.merge(&segment.counts);
            }
        }
        hourly.sort_by(|a, b| {
            (a.utc_hour, &a.build, a.origin, a.contract)
                .cmp(&(b.utc_hour, &b.build, b.origin, b.contract))
        });
        Snapshot {
            schema: SNAPSHOT_SCHEMA,
            window_start_hour: self.start(),
            window_end_hour: self.state.window_end_hour,
            coverage: Coverage::Unknown,
            opportunity_coverage: Coverage::Unknown,
            receipts: self.state.receipts,
            hourly,
            daily: daily
                .into_iter()
                .map(
                    |((utc_day, (build, origin, contract)), (observed_closed_hours, counts))| {
                        DailyAggregate {
                            utc_day,
                            build,
                            origin,
                            contract,
                            observed_closed_hours,
                            coverage: Coverage::Unknown,
                            counts: counts.snapshot(),
                        }
                    },
                )
                .collect(),
            unanchored: unknown
                .into_iter()
                .map(|((build, origin, contract), counts)| UnanchoredAggregate {
                    build,
                    origin,
                    contract,
                    counts: counts.snapshot(),
                })
                .collect(),
        }
    }
}
