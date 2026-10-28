//! Deterministic replay with an injected arrival clock. Nothing sleeps, opens a
//! device, or republishes the recording. The output describes requests only;
//! it is not evidence of the time a light changed or a motor moved.
use crate::{Engine, Input, LEASE_US, Output, TICK_US};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use lamp_bus::{Declaration, Header, Kind};
use lamp_msgs::{BodyOffsets, ClockStamp};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    fs::File,
    io::{self, BufRead, BufReader, Read, Write},
    path::PathBuf,
};

pub const MAX_LINE_BYTES: usize = 128 * 1024;
pub const MAX_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_LINES: usize = 100_000;
pub const MAX_RECORDS: usize = 50_000;
pub const MAX_FILES: usize = 32;
/// Ten minutes of simulated time, including the requested tail.
pub const MAX_DURATION_US: u64 = 600_000_000;

/// How much a replay or an import may read: by default the bounds above;
/// with `--long`, an hour of a lamp at rest (docs/alive.md), which carries
/// some 45,000 voice heartbeats, as many joint readings and 6,000 looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub bytes: usize,
    pub lines: usize,
    pub records: usize,
    pub duration_us: u64,
}

pub const DEFAULT_BOUNDS: Bounds = Bounds {
    bytes: MAX_BYTES,
    lines: MAX_LINES,
    records: MAX_RECORDS,
    duration_us: MAX_DURATION_US,
};

/// An hour and five minutes: room for an hour recorded and its margins.
pub const LONG_BOUNDS: Bounds = Bounds {
    bytes: 384 * 1024 * 1024,
    lines: 1_000_000,
    records: 600_000,
    duration_us: 3_900_000_000,
};

/// One explicitly authored or imported arrival. Payload stamps are independent:
/// a late event keeps its old stamp when injected later at `at_us`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub at_us: u64,
    pub input: Input,
}

/// Exactly one line is written for each 20 ms planner tick.
#[derive(Debug, Serialize)]
pub struct TickRecord {
    pub at_us: u64,
    pub output: Output,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningKind {
    RejectedInput,
    EstimatedArrival,
    LegacyWallClock,
    MissingSequence,
    DroppedSequence,
    PublisherRestart,
    IgnoredTopic,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Warning {
    pub kind: WarningKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at_us: Option<u64>,
    pub detail: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Summary {
    pub input_records: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub ticks: usize,
    pub first_tick_us: Option<u64>,
    pub last_tick_us: Option<u64>,
    pub warnings: Vec<Warning>,
    /// For each behaviour planned: in how many ticks, and how many times it
    /// began (a tick with it after a tick without).
    pub behaviours: BTreeMap<String, Planned>,
    /// The largest change from one tick to the next in what the body was
    /// asked, each axis on its own, in radians: where a twitch would show.
    pub largest_step: Option<BodyOffsets>,
    /// The farthest the body was asked to go, each axis on its own.
    pub farthest: Option<BodyOffsets>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Planned {
    pub ticks: usize,
    pub starts: usize,
}

impl Summary {
    fn note(&mut self, output: &Output, previous: &mut Option<(Vec<String>, Option<BodyOffsets>)>) {
        let (before, last) = previous.take().unwrap_or_default();
        for name in &output.state.behaviours {
            let entry = self.behaviours.entry(name.clone()).or_default();
            entry.ticks += 1;
            if !before.contains(name) {
                entry.starts += 1;
            }
        }
        let asked = output.body.as_ref().map(|body| body.offsets);
        if let Some(now) = asked {
            let largest = |a: Option<BodyOffsets>, b: [f64; 5]| {
                let a = a.map_or([0.0; 5], BodyOffsets::values);
                BodyOffsets::from_values(std::array::from_fn(|axis| a[axis].max(b[axis])))
            };
            self.farthest = Some(largest(self.farthest, now.values().map(f64::abs)));
            if let Some(last) = last {
                let step: [f64; 5] =
                    std::array::from_fn(|axis| (now.values()[axis] - last.values()[axis]).abs());
                self.largest_step = Some(largest(self.largest_step, step));
            }
        }
        *previous = Some((output.state.behaviours.clone(), asked));
    }
}

#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self(error.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self(error.to_string())
    }
}

struct Budget {
    bounds: Bounds,
    bytes: usize,
    lines: usize,
    records: usize,
}

impl Budget {
    fn new(bounds: Bounds) -> Self {
        Self {
            bounds,
            bytes: 0,
            lines: 0,
            records: 0,
        }
    }

    fn record(&mut self) -> Result<(), Error> {
        self.records += 1;
        if self.records > self.bounds.records {
            return Err(Error(format!(
                "more than {} input records",
                self.bounds.records
            )));
        }
        Ok(())
    }
}

fn read_lines(
    mut reader: impl BufRead,
    source: &str,
    budget: &mut Budget,
    mut visit: impl FnMut(usize, &[u8], &mut Budget) -> Result<(), Error>,
) -> Result<(), Error> {
    let mut line = Vec::new();
    let mut number = 0;
    loop {
        line.clear();
        // `lines` and unbounded `read_until` can allocate for an arbitrarily
        // long line before noticing the limit. Limit the read itself instead.
        let read = (&mut reader)
            .take((MAX_LINE_BYTES + 1) as u64)
            .read_until(b'\n', &mut line)
            .map_err(|error| Error(format!("{source} line {}: {error}", number + 1)))?;
        if read == 0 {
            return Ok(());
        }
        number += 1;
        budget.lines += 1;
        budget.bytes += read;
        let wrong = |why: String| Error(format!("{source} line {number}: {why}"));
        if read > MAX_LINE_BYTES {
            return Err(wrong(format!("line exceeds {MAX_LINE_BYTES} bytes")));
        }
        if budget.lines > budget.bounds.lines || budget.bytes > budget.bounds.bytes {
            return Err(wrong(format!(
                "recording exceeds {} lines or {} bytes",
                budget.bounds.lines, budget.bounds.bytes
            )));
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        visit(number, &line, budget).map_err(|error| wrong(error.0))?;
    }
}

fn ceil_tick(at_us: u64) -> Result<u64, Error> {
    let remainder = at_us % TICK_US;
    if remainder == 0 {
        return Ok(at_us);
    }
    at_us
        .checked_add(TICK_US - remainder)
        .ok_or_else(|| Error("timestamp overflows the next planner tick".into()))
}

/// Read and validate a complete, bounded scenario before changing the engine.
/// Arrivals must already be ordered; equal times keep their input line order.
/// All arrivals at a tick are applied before that tick. Events between ticks
/// are ingested at their exact injected time, before the following tick.
///
/// The first tick is the grid boundary at or before the first arrival. With no
/// explicit end, the last tick is the boundary at or after the last arrival
/// plus one command lease. An explicit end is inclusive and must include the
/// first grid tick after the final arrival. An empty file produces no output
/// unless an end is supplied, in which case ticks begin at zero.
pub fn run(
    reader: impl BufRead,
    writer: impl Write,
    engine: &mut Engine,
    until_us: Option<u64>,
) -> Result<Summary, Error> {
    run_bounded(reader, writer, engine, until_us, DEFAULT_BOUNDS)
}

/// [`run`] within other bounds: [`LONG_BOUNDS`] for an hour.
pub fn run_bounded(
    reader: impl BufRead,
    mut writer: impl Write,
    engine: &mut Engine,
    until_us: Option<u64>,
    bounds: Bounds,
) -> Result<Summary, Error> {
    let mut records = Vec::new();
    let mut budget = Budget::new(bounds);
    read_lines(reader, "scenario", &mut budget, |line, bytes, budget| {
        budget.record()?;
        let record: Record = serde_json::from_slice(bytes)?;
        record
            .input
            .check()
            .map_err(|error| Error(error.to_string()))?;
        if record.input.stamp().clock_id != engine.clock_id() {
            return Err(Error(format!(
                "clock {:?} does not match engine clock {:?}",
                record.input.stamp().clock_id,
                engine.clock_id()
            )));
        }
        if records
            .last()
            .is_some_and(|(_, previous): &(usize, Record)| previous.at_us > record.at_us)
        {
            return Err(Error("arrival timestamps run backwards".into()));
        }
        records.push((line, record));
        Ok(())
    })?;
    let mut summary = Summary {
        input_records: records.len(),
        ..Summary::default()
    };
    if records.is_empty() && until_us.is_none() {
        writer.flush()?;
        return Ok(summary);
    }
    let first = records.first().map_or(0, |(_, record)| record.at_us);
    let last = records.last().map_or(0, |(_, record)| record.at_us);
    let start = first / TICK_US * TICK_US;
    let end = match until_us {
        Some(until) => {
            if until < ceil_tick(last)? {
                return Err(Error(
                    "until_us must include the first tick at or after the last input".into(),
                ));
            }
            until / TICK_US * TICK_US
        }
        None => ceil_tick(
            last.checked_add(LEASE_US)
                .ok_or_else(|| Error("last timestamp plus lease overflows".into()))?,
        )?,
    };
    if end.saturating_sub(start) > bounds.duration_us {
        return Err(Error(format!(
            "replay horizon exceeds {} microseconds",
            bounds.duration_us
        )));
    }
    let mut pending = records.into_iter().peekable();
    let mut at_us = start;
    let mut previous = None;
    loop {
        while pending
            .peek()
            .is_some_and(|(_, record)| record.at_us <= at_us)
        {
            let (line, record) = pending.next().expect("peeked record");
            match engine.ingest(record.at_us, record.input) {
                Ok(()) => summary.accepted += 1,
                Err(error) => {
                    summary.rejected += 1;
                    summary.warnings.push(Warning {
                        kind: WarningKind::RejectedInput,
                        source: None,
                        line: Some(line),
                        at_us: Some(record.at_us),
                        detail: error.to_string(),
                    });
                }
            }
        }
        let tick = TickRecord {
            at_us,
            output: engine.tick(at_us),
        };
        summary.note(&tick.output, &mut previous);
        serde_json::to_writer(&mut writer, &tick)?;
        writer.write_all(b"\n")?;
        summary.first_tick_us.get_or_insert(at_us);
        summary.last_tick_us = Some(at_us);
        summary.ticks += 1;
        if at_us == end {
            break;
        }
        at_us = at_us
            .checked_add(TICK_US)
            .ok_or_else(|| Error("planner clock overflows".into()))?;
    }
    writer.flush()?;
    Ok(summary)
}

/// An explicitly shared monotonic timestamp paired with a publisher's wall
/// timestamp. It does not assert that either is a recorder receipt timestamp.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockAnchor {
    pub clock_id: String,
    pub at_us: u64,
    pub at_unix_ms: u64,
}

impl ClockAnchor {
    fn map_wall(&self, at_unix_ms: u64) -> Result<u64, Error> {
        let at =
            i128::from(self.at_us) + (i128::from(at_unix_ms) - i128::from(self.at_unix_ms)) * 1_000;
        u64::try_from(at).map_err(|_| {
            Error("wall-clock mapping is outside the shared monotonic clock's range".into())
        })
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Imported {
    pub anchor: ClockAnchor,
    pub records: Vec<Record>,
    pub warnings: Vec<Warning>,
}

/// A line written by `lamp-bus record`. Declarations are deliberately accepted
/// and checked alongside messages rather than silently treated as events.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BusLine {
    header: Option<Header>,
    payload_b64: Option<String>,
    topic: Option<String>,
    declaration: Option<Declaration>,
}

struct Captured {
    header: Header,
    payload: serde_json::Value,
    kind: &'static str,
    typed: bool,
    source: String,
    line: usize,
}

fn topic(topic: &str) -> Option<(&'static str, &'static str, Kind, bool)> {
    use lamp_msgs::{
        Concentration, ControlGesture, Faces, JointState, Light, NameHeard, PersonActivity,
        RoomState, ScheduleDue, Speech, SuddenSound, VoicePlayout, VoiceProgress, VoiceSound,
        VoiceState, VoiceTiming,
    };
    Some(match topic {
        VoiceTiming::TOPIC => ("voice", VoiceTiming::TYPE, Kind::Latest, true),
        VoiceProgress::TOPIC => ("progress", VoiceProgress::TYPE, Kind::Latest, true),
        PersonActivity::TOPIC => ("activity", PersonActivity::TYPE, Kind::Latest, true),
        VoicePlayout::TOPIC => ("playout", VoicePlayout::TYPE, Kind::Event, true),
        VoiceSound::TOPIC => ("sound", VoiceSound::TYPE, Kind::Event, true),
        Concentration::TOPIC => ("concentration", Concentration::TYPE, Kind::Latest, true),
        VoiceState::TOPIC => ("voice_state", VoiceState::TYPE, Kind::Latest, false),
        JointState::TOPIC => ("joints", JointState::TYPE, Kind::Latest, false),
        Faces::TOPIC => ("faces", Faces::TYPE, Kind::Latest, false),
        RoomState::TOPIC => ("room", RoomState::TYPE, Kind::Latest, false),
        "light/command" => ("light", Light::TYPE, Kind::Latest, false),
        "voice/heard" => ("heard", Speech::TYPE, Kind::Event, false),
        "voice/said" => ("said", Speech::TYPE, Kind::Event, false),
        NameHeard::TOPIC => ("name", NameHeard::TYPE, Kind::Event, false),
        SuddenSound::TOPIC => ("sudden", SuddenSound::TYPE, Kind::Event, false),
        lamp_msgs::VoiceRemarked::TOPIC => (
            "remarked",
            lamp_msgs::VoiceRemarked::TYPE,
            Kind::Event,
            false,
        ),
        ControlGesture::TOPIC => ("touch", ControlGesture::TYPE, Kind::Event, false),
        ScheduleDue::TOPIC => ("due", ScheduleDue::TYPE, Kind::Latest, false),
        lamp_msgs::TouchEvent::TOPIC => (
            "head_touch",
            lamp_msgs::TouchEvent::TYPE,
            Kind::Event,
            false,
        ),
        lamp_msgs::TouchState::TOPIC => (
            "head_hand",
            lamp_msgs::TouchState::TYPE,
            Kind::Latest,
            false,
        ),
        _ => return None,
    })
}

fn captured_warning(captured: &Captured, kind: WarningKind, detail: String) -> Warning {
    Warning {
        kind,
        source: Some(captured.source.clone()),
        line: Some(captured.line),
        at_us: None,
        detail,
    }
}

fn capture(paths: &[PathBuf], bounds: Bounds) -> Result<(Vec<Captured>, Vec<Warning>), Error> {
    if paths.is_empty() || paths.len() > MAX_FILES {
        return Err(Error(format!(
            "supply from 1 to {MAX_FILES} bus recordings"
        )));
    }
    let mut budget = Budget::new(bounds);
    let mut captured = Vec::new();
    let mut warnings = Vec::new();
    let mut declarations: BTreeMap<String, Declaration> = BTreeMap::new();
    let mut sequences: BTreeMap<(String, String, String), (u64, u64)> = BTreeMap::new();
    let mut boots: BTreeMap<(String, String), String> = BTreeMap::new();
    let mut retired = BTreeSet::new();
    for path in paths {
        let source = path.display().to_string();
        let reader =
            BufReader::new(File::open(path).map_err(|error| Error(format!("{source}: {error}")))?);
        read_lines(reader, &source, &mut budget, |line, bytes, budget| {
            let row: BusLine = serde_json::from_slice(bytes)?;
            if let (None, None, Some(topic_name), Some(declaration)) =
                (&row.header, &row.payload_b64, &row.topic, &row.declaration)
            {
                if declaration.period_ms == Some(0) {
                    return Err(Error(
                        "declaration period must be positive when present".into(),
                    ));
                }
                if let Some((_, expected, kind, _)) = topic(topic_name)
                    && (declaration.type_name != expected || declaration.kind != kind)
                {
                    return Err(Error(format!(
                        "declaration for {topic_name} disagrees with {expected} and {kind:?}"
                    )));
                }
                declarations.insert(topic_name.clone(), declaration.clone());
                return Ok(());
            }
            let (Some(header), Some(encoded), None, None) =
                (row.header, row.payload_b64, row.topic, row.declaration)
            else {
                return Err(Error(
                    "expected one declaration or one header/payload_b64 message".into(),
                ));
            };
            budget.record()?;
            let Some((kind_name, expected, kind, typed)) = topic(&header.topic) else {
                warnings.push(Warning {
                    kind: WarningKind::IgnoredTopic,
                    source: Some(source.clone()),
                    line: Some(line),
                    at_us: None,
                    detail: format!("topic {} is not an expression input", header.topic),
                });
                return Ok(());
            };
            if header.enc != lamp_bus::JSON
                || header.kind != kind
                || header
                    .type_name
                    .as_deref()
                    .is_some_and(|name| name != expected)
            {
                return Err(Error(format!(
                    "{} needs JSON {expected} with kind {kind:?}",
                    header.topic
                )));
            }
            if let Some(declared) = declarations.get(&header.topic)
                && (declared.type_name != expected || declared.kind != kind)
            {
                return Err(Error("message conflicts with the topic declaration".into()));
            }
            if header.seq == 0
                || header.boot.len() != 16
                || !header.boot.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(Error(
                    "header needs sequence from 1 and a 16-digit hex boot".into(),
                ));
            }
            let payload = STANDARD
                .decode(encoded)
                .map_err(|error| Error(format!("payload_b64: {error}")))?;
            if payload.len() > lamp_bus::MOST_BYTES {
                return Err(Error("decoded bus payload exceeds one bus datagram".into()));
            }
            let row = Captured {
                header,
                payload: serde_json::from_slice(&payload)?,
                kind: kind_name,
                typed,
                source: source.clone(),
                line,
            };
            let instance = (
                row.header.topic.clone(),
                row.header.source.clone(),
                row.header.boot.clone(),
            );
            if retired.contains(&instance) {
                warnings.push(captured_warning(
                    &row,
                    WarningKind::DroppedSequence,
                    "old publisher instance after its replacement; dropped".into(),
                ));
                return Ok(());
            }
            let owner = (row.header.topic.clone(), row.header.source.clone());
            if let Some(previous) = boots.get(&owner)
                && previous != &row.header.boot
            {
                retired.insert((owner.0.clone(), owner.1.clone(), previous.clone()));
                warnings.push(captured_warning(
                    &row,
                    WarningKind::PublisherRestart,
                    format!(
                        "{} publisher restarted; old instance is retired",
                        row.header.topic
                    ),
                ));
            }
            boots.insert(owner, row.header.boot.clone());
            if let Some((sequence, wall)) = sequences.get(&instance) {
                if row.header.seq <= *sequence {
                    warnings.push(captured_warning(
                        &row,
                        WarningKind::DroppedSequence,
                        format!(
                            "sequence {} after {}; duplicate or reordered message dropped",
                            row.header.seq, sequence
                        ),
                    ));
                    return Ok(());
                }
                if row.header.at_unix_ms < *wall {
                    return Err(Error("publisher wall timestamp runs backwards; a shared-clock ordering cannot be inferred".into()));
                }
                if row.header.seq - sequence > 1 {
                    warnings.push(captured_warning(
                        &row,
                        WarningKind::MissingSequence,
                        format!(
                            "{} messages missing before sequence {}",
                            row.header.seq - sequence - 1,
                            row.header.seq
                        ),
                    ));
                }
            }
            sequences.insert(instance, (row.header.seq, row.header.at_unix_ms));
            captured.push(row);
            Ok(())
        })?;
    }
    Ok((captured, warnings))
}

fn typed_input(row: &Captured) -> Result<Input, Error> {
    let mut value = row.payload.clone();
    let object = value
        .as_object_mut()
        .ok_or_else(|| Error("typed bus payload is not an object".into()))?;
    if object.insert("kind".into(), row.kind.into()).is_some() {
        return Err(Error(
            "typed payload unexpectedly declares an input kind".into(),
        ));
    }
    let input: Input = serde_json::from_value(value)?;
    input.check().map_err(|error| Error(error.to_string()))?;
    Ok(input)
}

/// Import actual `lamp-bus record` files using a timing payload in the named
/// clock as the required wall/shared-clock anchor. The first typed input in
/// deterministic file/line order supplies that anchor. Private header `at_us`
/// values are never used. Unknown input clocks are fatal.
pub fn import_bus(paths: &[PathBuf], clock_id: &str) -> Result<Imported, Error> {
    import_bus_bounded(paths, clock_id, DEFAULT_BOUNDS)
}

/// [`import_bus`] within other bounds: [`LONG_BOUNDS`] for an hour.
pub fn import_bus_bounded(
    paths: &[PathBuf],
    clock_id: &str,
    bounds: Bounds,
) -> Result<Imported, Error> {
    let (captured, warnings) = capture(paths, bounds)?;
    let anchor_row = captured
        .iter()
        .find(|row| row.typed)
        .ok_or_else(|| Error("no typed timing record supplies a shared-clock anchor; provide one explicitly with import_bus_with_anchor".into()))?;
    let input = typed_input(anchor_row)?;
    if input.stamp().clock_id != clock_id {
        return Err(Error(format!(
            "anchor clock {:?} is not {clock_id:?}",
            input.stamp().clock_id
        )));
    }
    let anchor = ClockAnchor {
        clock_id: clock_id.into(),
        at_us: input.stamp().at_us,
        at_unix_ms: anchor_row.header.at_unix_ms,
    };
    convert(captured, warnings, anchor, bounds)
}

/// Explicit mapping for older recordings that contain no shared-clock payload.
/// Supplying this anchor asserts a replay origin, not a measured clock sync.
pub fn import_bus_with_anchor(paths: &[PathBuf], anchor: &ClockAnchor) -> Result<Imported, Error> {
    let (captured, warnings) = capture(paths, DEFAULT_BOUNDS)?;
    convert(captured, warnings, anchor.clone(), DEFAULT_BOUNDS)
}

/// Add the local time of day to an import, as the live service tells it,
/// at its first record and every minute after: the recording's wall clock
/// through its anchor, `utc_offset_minutes` east of UTC. A recording has no
/// time of day of its own; without this a replay never sees night.
pub fn add_daytime(imported: &mut Imported, utc_offset_minutes: i32) -> Result<(), Error> {
    add_local_daytime(
        imported,
        &crate::local_time::LocalTime::Offset(utc_offset_minutes),
    )
}

/// As [`add_daytime`], in a zone whose offset is taken at each moment (#820).
pub fn add_local_daytime(
    imported: &mut Imported,
    local: &crate::local_time::LocalTime,
) -> Result<(), Error> {
    let (Some(first), Some(last)) = (imported.records.first(), imported.records.last()) else {
        return Ok(());
    };
    let (first, last) = (first.at_us, last.at_us);
    let anchor = &imported.anchor;
    let mut at_us = first;
    let mut sequence = 0;
    while at_us <= last {
        let wall =
            i128::from(anchor.at_unix_ms) + (i128::from(at_us) - i128::from(anchor.at_us)) / 1_000;
        let wall =
            u64::try_from(wall).map_err(|_| Error("the time of day falls before 1970".into()))?;
        sequence += 1;
        imported.records.push(Record {
            at_us,
            input: Input::DayTime {
                stamp: ClockStamp {
                    clock_id: anchor.clock_id.clone(),
                    at_us,
                    sequence,
                },
                minute: local.minute_of_day(wall),
            },
        });
        at_us = at_us.saturating_add(crate::runtime::DAYTIME_EVERY_US);
    }
    imported.records.sort_by_key(|record| record.at_us);
    Ok(())
}

fn convert(
    captured: Vec<Captured>,
    mut warnings: Vec<Warning>,
    anchor: ClockAnchor,
    bounds: Bounds,
) -> Result<Imported, Error> {
    ClockStamp {
        clock_id: anchor.clock_id.clone(),
        at_us: anchor.at_us,
        sequence: 1,
    }
    .check()
    .map_err(|error| Error(error.to_string()))?;
    warnings.push(Warning {
        kind: WarningKind::EstimatedArrival,
        source: None,
        line: None,
        at_us: None,
        detail: "bus recordings contain publisher send times, not recorder receipt times; typed inputs are injected at their payload stamp (zero transit delay). Equal times retain supplied file/line order; cross-topic delivery order is unknown. This cannot measure arrival latency or physical presentation".into(),
    });
    if captured.iter().any(|row| !row.typed) {
        warnings.push(Warning {
            kind: WarningKind::LegacyWallClock,
            source: None,
            line: None,
            at_us: None,
            detail: "legacy inputs use anchor.at_us + (header.at_unix_ms - anchor.at_unix_ms) * 1000; wall-pair quantization is less than 1 ms if the same stable wall clock is used. Clock jumps, cross-host skew, payload-to-send and send-to-receipt delays are unmeasured and have no inferred error bound. Header at_us is private and ignored; ties retain file/line order".into(),
        });
    }
    let mut records = Vec::with_capacity(captured.len());
    for row in captured {
        let input = if row.typed {
            typed_input(&row)
        } else {
            let at_us = anchor.map_wall(row.header.at_unix_ms)?;
            serde_json::from_value(serde_json::json!({
                "kind": row.kind,
                "stamp": {"clock_id":anchor.clock_id,"at_us":at_us,"sequence":row.header.seq},
                "value": row.payload,
            }))
            .map_err(Error::from)
        }
        .map_err(|error| Error(format!("{} line {}: {error}", row.source, row.line)))?;
        input
            .check()
            .map_err(|error| Error(format!("{} line {}: {error}", row.source, row.line)))?;
        if input.stamp().clock_id != anchor.clock_id {
            return Err(Error(format!(
                "{} line {}: input clock {:?} differs from anchor {:?}",
                row.source,
                row.line,
                input.stamp().clock_id,
                anchor.clock_id
            )));
        }
        records.push(Record {
            at_us: input.stamp().at_us,
            input,
        });
    }
    // Stable sort preserves within-file and caller-specified file order for
    // ties, including the uncertainty introduced by millisecond wall stamps.
    records.sort_by_key(|record| record.at_us);
    // Header sequence belongs to one publisher instance. The wrapper models
    // the replay subscriber's receipt sequence so a valid restarted legacy
    // publisher is not rejected merely because its count began at one again.
    for (index, record) in records.iter_mut().enumerate() {
        if let Input::VoiceState { stamp, .. }
        | Input::Joints { stamp, .. }
        | Input::Faces { stamp, .. }
        | Input::Room { stamp, .. }
        | Input::Light { stamp, .. }
        | Input::Heard { stamp, .. }
        | Input::Said { stamp, .. }
        | Input::Name { stamp, .. }
        | Input::Touch { stamp, .. }
        | Input::Due { stamp, .. }
        | Input::HeadTouch { stamp, .. }
        | Input::HeadHand { stamp, .. } = &mut record.input
        {
            stamp.sequence = index as u64 + 1;
        }
    }
    if let (Some(first), Some(last)) = (records.first(), records.last())
        && last.at_us - first.at_us > bounds.duration_us
    {
        return Err(Error(format!(
            "imported time horizon exceeds {} microseconds",
            bounds.duration_us
        )));
    }
    Ok(Imported {
        anchor,
        records,
        warnings,
    })
}
