//! Version-two whole-voice bench. The old played rubric stays in `voice`.
//!
//! Hashes identify evidence; a caller must verify the linked prerequisite report
//! and the lead must match all provenance to the private raw archive. This
//! module neither enables a sensor nor grants release permission.
pub use crate::schema::DataOrigin;
use crate::{
    digest, quantile,
    schema::{sha, token},
    score::Check,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const RUBRIC: &str = "sensor-team-2026-11-27-v2";
pub const INTERRUPT_LIMIT_US: u64 = 400_000;
pub const MAX_SYNC_ERROR_US: u64 = 5_000;
pub const IGNORE_OBSERVATION_US: u64 = 10_000_000;
pub const MIN_SCENARIO_CASES: usize = 10;
const SENSORS: [&str; 7] = [
    "call",
    "addressed",
    "direction",
    "head",
    "speaking_face",
    "voice_match",
    "barge_in",
];

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    BaselineCharacterization,
    OneSensorAddition,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Played,
    Live,
}
impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Played => "played",
            Self::Live => "live",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Scenario {
    Alone,
    Noisy,
    TwoPeople,
    Overlap,
    #[serde(rename = "followup_20s")]
    Followup20s,
    SideTalk,
}
impl Scenario {
    pub fn name(self) -> &'static str {
        match self {
            Self::Alone => "alone",
            Self::Noisy => "noisy",
            Self::TwoPeople => "two_people",
            Self::Overlap => "overlap",
            Self::Followup20s => "followup_20s",
            Self::SideTalk => "side_talk",
        }
    }
}
const SCENARIOS: [Scenario; 6] = [
    Scenario::Alone,
    Scenario::Noisy,
    Scenario::TwoPeople,
    Scenario::Overlap,
    Scenario::Followup20s,
    Scenario::SideTalk,
];

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Call,
    Followup,
    Room,
    Interruption,
    Backchannel,
    Noise,
    Stop,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Answer,
    Ignore,
    Stop,
    Continue,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum WindowBand {
    Early,
    Middle,
    Late,
    BoundaryInside,
    BoundaryOutside,
    Outside,
    Closed,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BuildIdentity {
    pub build_sha: String,
    pub producer_binary_sha256: String,
    pub source_patch_sha256: Option<String>,
    pub config_sha256: String,
    pub model_manifest_sha256: String,
}
impl BuildIdentity {
    fn valid(&self) -> bool {
        sha(&self.build_sha, 40)
            && sha(&self.producer_binary_sha256, 64)
            && self.source_patch_sha256.as_ref().is_none_or(|s| sha(s, 64))
            && sha(&self.config_sha256, 64)
            && sha(&self.model_manifest_sha256, 64)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Prerequisite {
    pub sensor: String,
    pub report_sha256: String,
    pub model_sha256: String,
    pub config_sha256: String,
    pub rubric: String,
    pub qualified: bool,
    pub holdout_on_lamp: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub id: String,
    pub mode: Mode,
    pub scenario: Scenario,
    pub kind: Kind,
    pub expected: Action,
    pub speaker_id: Option<String>,
    pub session_id: String,
    pub people: u32,
    pub truth_known: bool,
    pub data_origin: DataOrigin,
    pub consent: bool,
    pub window_band: Option<WindowBand>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Line {
    pub id: String,
    pub clock_id: String,
    pub observed: Action,
    /// Independent capture/observer evidence, never merely a nonempty transcript.
    pub stimulus_verified: bool,
    pub outcome_verified: bool,
    pub speech_start_us: u64,
    pub speech_end_us: u64,
    pub previous_lamp_audio_end_us: Option<u64>,
    pub window_closed_us: Option<u64>,
    pub observation_end_us: Option<u64>,
    pub first_audio_us: Option<u64>,
    pub interrupt_onset_us: Option<u64>,
    pub last_audio_us: Option<u64>,
    /// End of the output scheduled before the stimulus, on this capture clock.
    pub planned_lamp_audio_end_us: Option<u64>,
    pub playback_active_at_input: bool,
    pub continued_without_restart: Option<bool>,
    pub listening_after_stop: Option<bool>,
    pub safety_violation: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Run {
    pub leg: String,
    pub mode: Mode,
    pub started_unix_s: u64,
    pub identity: BuildIdentity,
    pub unchanged_config_sha256: String,
    pub kit_sha256: String,
    pub cases_sha256: String,
    pub raw_manifest_sha256: String,
    pub evidence: String,
    pub enabled_sensors: Vec<String>,
    pub combined_judge: bool,
    pub clock_id: String,
    pub sync_error_us: Option<u64>,
    pub clock_verified: bool,
    pub safety_violation: bool,
    pub lines: Vec<Line>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Comparison {
    pub version: u32,
    pub rubric: String,
    pub purpose: Purpose,
    pub baseline: BuildIdentity,
    pub candidate: BuildIdentity,
    pub unchanged_config_sha256: String,
    pub kit_sha256: String,
    pub kit_reviewed: bool,
    pub collection_manifest_sha256: String,
    pub cases_sha256: String,
    pub scorer_revision_sha: String,
    pub scorer_binary_sha256: String,
    pub protocol_frozen_unix_s: u64,
    pub labels_adjudicated_before_predictions: bool,
    pub holdout_disjoint: bool,
    pub accepted_sensors: Vec<String>,
    pub added_sensor: Option<String>,
    pub added_component_model_sha256: Option<String>,
    pub added_component_config_sha256: Option<String>,
    pub target_scenario: Option<Scenario>,
    pub prerequisite: Option<Prerequisite>,
    pub cases: Vec<Case>,
    pub runs: Vec<Run>,
}

#[derive(Debug, Serialize)]
pub struct Group {
    pub leg: String,
    pub mode: Mode,
    pub scenario: Scenario,
    pub planned: usize,
    pub correct: usize,
    pub missing: usize,
    pub unknown: usize,
    pub timing_unknown: usize,
    pub safety_violations: usize,
    pub answer_timing_expected: bool,
    pub stop_timing_expected: bool,
    pub answer_timing_samples: usize,
    pub stop_timing_samples: usize,
    pub answer_p50_us: Option<u64>,
    pub answer_p95_us: Option<u64>,
    pub stop_p50_us: Option<u64>,
    pub stop_p95_us: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct Gain {
    pub mode: Mode,
    pub baseline_leg: String,
    pub candidate_leg: String,
    pub scenario: Scenario,
    pub planned: usize,
    pub additional_correct: i64,
    pub percentage_points: f64,
}

#[derive(Debug, Serialize)]
pub struct Row {
    pub leg: String,
    pub mode: Mode,
    pub case_id: String,
    pub correct: bool,
    pub missing: bool,
    pub unknown: bool,
    pub timing_unknown: bool,
    pub safety_violation: bool,
    pub latency_us: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub version: u32,
    pub rubric: String,
    pub purpose: Purpose,
    pub passed: bool,
    pub checks: Vec<Check>,
    pub groups: Vec<Group>,
    pub rows: Vec<Row>,
    pub target_gains: Vec<Gain>,
}

/// SHA-256 of compact serde JSON for this typed ordered case vector. Capture
/// the hash after adjudication and before examining predictions.
pub fn cases_hash(cases: &[Case]) -> Result<String, String> {
    serde_json::to_vec(cases)
        .map(|b| digest(&b))
        .map_err(|e| e.to_string())
}

fn sensors(values: &[String]) -> Option<BTreeSet<&str>> {
    let set: BTreeSet<_> = values.iter().map(String::as_str).collect();
    (set.len() == values.len() && set.iter().all(|v| SENSORS.contains(v))).then_some(set)
}

fn window_matches(band: Option<WindowBand>, line: &Line, error: Option<u64>) -> bool {
    let Some(band) = band else { return true };
    let Some(error) = error else { return false };
    if band == WindowBand::Closed {
        return line
            .window_closed_us
            .and_then(|t| t.checked_add(error))
            .is_some_and(|t| t <= line.speech_start_us);
    }
    let Some(gap) = line
        .previous_lamp_audio_end_us
        .and_then(|end| line.speech_start_us.checked_sub(end))
    else {
        return false;
    };
    let Some((lower, upper)) = gap.checked_sub(error).zip(gap.checked_add(error)) else {
        return false;
    };
    let within = |lo, hi| lower >= lo && upper <= hi;
    match band {
        WindowBand::Early => within(0, 3_000_000),
        WindowBand::Middle => within(3_000_001, 10_000_000),
        WindowBand::Late => within(10_000_001, 20_000_000),
        WindowBand::BoundaryInside => within(19_500_000, 20_000_000),
        WindowBand::BoundaryOutside => within(20_000_001, 20_500_000),
        WindowBand::Outside => lower > 20_000_000,
        WindowBand::Closed => unreachable!(),
    }
}

fn valid_case(case: &Case) -> bool {
    if !token(&case.id)
        || !token(&case.session_id)
        || case.speaker_id.as_ref().is_some_and(|s| !token(s))
        || (case.kind != Kind::Noise && (case.speaker_id.is_none() || case.people == 0))
        || (case.scenario == Scenario::TwoPeople && case.people < 2)
        || case.expected == Action::Unknown
    {
        return false;
    }
    if case.scenario == Scenario::Overlap
        && ![
            Kind::Interruption,
            Kind::Backchannel,
            Kind::Room,
            Kind::Noise,
        ]
        .contains(&case.kind)
    {
        return false;
    }
    if case.scenario == Scenario::SideTalk && case.kind != Kind::Room {
        return false;
    }
    if case.scenario == Scenario::Followup20s
        && (case.kind != Kind::Followup || case.window_band.is_none())
    {
        return false;
    }
    let expected = match case.kind {
        Kind::Call => Action::Answer,
        Kind::Followup => {
            if matches!(
                case.window_band,
                Some(WindowBand::BoundaryOutside | WindowBand::Outside | WindowBand::Closed)
            ) {
                Action::Ignore
            } else {
                Action::Answer
            }
        }
        Kind::Room | Kind::Noise => {
            if case.scenario == Scenario::Overlap {
                Action::Continue
            } else {
                Action::Ignore
            }
        }
        Kind::Interruption | Kind::Stop => Action::Stop,
        Kind::Backchannel => Action::Continue,
    };
    case.expected == expected
}

fn evaluate(case: &Case, run: &Run, line: Option<&&Line>) -> Row {
    let mut row = Row {
        leg: run.leg.clone(),
        mode: run.mode,
        case_id: case.id.clone(),
        correct: false,
        missing: line.is_none(),
        unknown: false,
        timing_unknown: false,
        safety_violation: run.safety_violation,
        latency_us: None,
    };
    let Some(line) = line else { return row };
    row.safety_violation |= line.safety_violation;
    let clocks = run.clock_verified
        && run.sync_error_us.is_some_and(|v| v <= MAX_SYNC_ERROR_US)
        && line.clock_id == run.clock_id;
    row.unknown = !case.truth_known
        || case.data_origin == DataOrigin::Invented
        || (case.data_origin == DataOrigin::RealPeople && !case.consent)
        || (case.mode == Mode::Live
            && case.kind != Kind::Noise
            && case.data_origin != DataOrigin::RealPeople)
        || !line.stimulus_verified
        || !line.outcome_verified
        || line.observed == Action::Unknown
        || !clocks;
    row.unknown |= line.observed == Action::Ignore && line.first_audio_us.is_some();
    row.timing_unknown = line.observation_end_us.is_none_or(|end| {
        line.first_audio_us.is_some_and(|first| first > end)
            || line.last_audio_us.is_some_and(|last| last > end)
    }) || (line.observed == Action::Answer && line.first_audio_us.is_none());
    if !window_matches(case.window_band, line, run.sync_error_us) {
        row.unknown = true;
    }
    let matched = line.observed == case.expected;
    if matched {
        match case.expected {
            Action::Answer => {
                row.latency_us = line
                    .first_audio_us
                    .and_then(|t| t.checked_sub(line.speech_end_us));
                row.timing_unknown |= row.latency_us.is_none();
            }
            Action::Stop => {
                let start = if case.kind == Kind::Interruption {
                    row.unknown |= line.interrupt_onset_us != Some(line.speech_start_us);
                    Some(line.speech_start_us)
                } else {
                    line.interrupt_onset_us
                };
                row.latency_us = start
                    .zip(line.last_audio_us)
                    .and_then(|(start, end)| end.checked_sub(start));
                row.timing_unknown |= row.latency_us.is_none()
                    || line
                        .last_audio_us
                        .zip(line.planned_lamp_audio_end_us)
                        .is_none_or(|(last, planned)| last >= planned);
                if !line.playback_active_at_input {
                    row.unknown = true;
                }
                if case.kind == Kind::Interruption && line.listening_after_stop != Some(true) {
                    row.unknown = true;
                }
            }
            Action::Ignore => {
                row.timing_unknown |= !line.observation_end_us.is_some_and(|end| {
                    end.saturating_sub(line.speech_end_us) >= IGNORE_OBSERVATION_US
                });
            }
            Action::Continue => {
                row.timing_unknown |= !line
                    .last_audio_us
                    .zip(line.observation_end_us)
                    .zip(line.planned_lamp_audio_end_us)
                    .is_some_and(|((last, observed), planned)| {
                        last >= planned && observed >= last.max(line.speech_end_us)
                    });
                row.unknown |=
                    !line.playback_active_at_input || line.continued_without_restart != Some(true);
            }
            Action::Unknown => unreachable!(),
        }
    }
    let deadline = case.kind != Kind::Interruption
        || row
            .latency_us
            .zip(run.sync_error_us)
            .is_some_and(|(latency, error)| latency.saturating_add(error) <= INTERRUPT_LIMIT_US);
    row.correct =
        matched && !row.unknown && !row.timing_unknown && !row.safety_violation && deadline;
    row
}

fn no_latency_drop(
    a: Option<u64>,
    b: Option<u64>,
    expected: bool,
    a_error: Option<u64>,
    b_error: Option<u64>,
) -> bool {
    match (a, b, a_error, b_error) {
        (Some(a), Some(b), Some(a_error), Some(b_error)) => b
            .checked_add(b_error)
            .zip(a.checked_sub(a_error))
            .is_some_and(|(upper, lower)| upper <= lower),
        (None, None, _, _) => !expected,
        _ => false,
    }
}

pub fn compare(input: &Comparison) -> Result<Report, String> {
    if input.version != 2
        || input.rubric != RUBRIC
        || !input.baseline.valid()
        || !input.candidate.valid()
        || !sha(&input.unchanged_config_sha256, 64)
        || !sha(&input.kit_sha256, 64)
        || !sha(&input.collection_manifest_sha256, 64)
        || !sha(&input.cases_sha256, 64)
        || !sha(&input.scorer_revision_sha, 40)
        || !sha(&input.scorer_binary_sha256, 64)
        || input.protocol_frozen_unix_s == 0
    {
        return Err("invalid whole-bench version, rubric or provenance".into());
    }
    let accepted = sensors(&input.accepted_sensors).ok_or("invalid accepted-sensor set")?;
    let mut candidate_sensors = accepted.clone();
    if let Some(added) = &input.added_sensor
        && (!SENSORS.contains(&added.as_str()) || !candidate_sensors.insert(added))
    {
        return Err("addition must be exactly one new sensor".into());
    }
    if input.purpose == Purpose::OneSensorAddition
        && (input.added_sensor.is_none()
            || input.target_scenario.is_none()
            || input.prerequisite.is_none()
            || !input
                .added_component_model_sha256
                .as_ref()
                .is_some_and(|v| sha(v, 64))
            || !input
                .added_component_config_sha256
                .as_ref()
                .is_some_and(|v| sha(v, 64)))
    {
        return Err(
            "one-sensor addition needs target, component hashes and linked prerequisite".into(),
        );
    }
    let mut ids = BTreeSet::new();
    for case in &input.cases {
        if !valid_case(case) || !ids.insert((case.mode, case.id.as_str())) {
            return Err("invalid or duplicate whole-bench case".into());
        }
    }
    let mut checks = vec![
        Check::new(
            "promotion_purpose",
            input.purpose == Purpose::OneSensorAddition,
            "C/D characterization never promotes a sensor".into(),
        ),
        Check::new(
            "case_manifest_hash",
            cases_hash(&input.cases)? == input.cases_sha256,
            "canonical typed case vector must match the frozen hash".into(),
        ),
        Check::new(
            "frozen_reviewed_holdout",
            input.kit_reviewed
                && input.labels_adjudicated_before_predictions
                && input.holdout_disjoint,
            "reviewed assets, blind labels and disjoint holdout required".into(),
        ),
    ];
    if input.purpose == Purpose::OneSensorAddition {
        let p = input.prerequisite.as_ref().expect("checked prerequisite");
        checks.push(Check::new("qualified_sensor_prerequisite", Some(&p.sensor) == input.added_sensor.as_ref()
            && sha(&p.report_sha256, 64) && sha(&p.model_sha256,64) && sha(&p.config_sha256,64)
            && input.added_component_model_sha256.as_ref() == Some(&p.model_sha256)
            && input.added_component_config_sha256.as_ref() == Some(&p.config_sha256)
            && p.rubric == RUBRIC && p.qualified && p.holdout_on_lamp,
            "the CLI must also hash and validate the separate sensor report; metadata alone is not evidence".into()));
    }
    for mode in [Mode::Played, Mode::Live] {
        for scenario in SCENARIOS {
            let n = input
                .cases
                .iter()
                .filter(|c| c.mode == mode && c.scenario == scenario)
                .count();
            checks.push(Check::new(
                &format!("{}_{}_quota", mode.name(), scenario.name()),
                n >= MIN_SCENARIO_CASES,
                format!("{n}/{MIN_SCENARIO_CASES} fixed cases"),
            ));
        }
        for kind in [
            Kind::Interruption,
            Kind::Backchannel,
            Kind::Room,
            Kind::Noise,
        ] {
            let n = input
                .cases
                .iter()
                .filter(|c| c.mode == mode && c.scenario == Scenario::Overlap && c.kind == kind)
                .count();
            checks.push(Check::new(
                &format!("{}_{kind:?}_overlap_quota", mode.name()),
                n >= 10,
                format!("{n}/10 class cases"),
            ));
        }
        for band in [
            WindowBand::Early,
            WindowBand::Middle,
            WindowBand::Late,
            WindowBand::BoundaryInside,
            WindowBand::BoundaryOutside,
            WindowBand::Outside,
            WindowBand::Closed,
        ] {
            let n = input
                .cases
                .iter()
                .filter(|c| {
                    c.mode == mode
                        && c.scenario == Scenario::Followup20s
                        && c.window_band == Some(band)
                })
                .count();
            checks.push(Check::new(
                &format!("{}_{band:?}_window_coverage", mode.name()),
                n > 0,
                format!("{n} fixed cases"),
            ));
        }
        let outside_call = input.cases.iter().any(|c| {
            c.mode == mode && c.kind == Kind::Call && c.window_band == Some(WindowBand::Outside)
        });
        let inside_side = input.cases.iter().any(|c| {
            c.mode == mode
                && c.scenario == Scenario::SideTalk
                && c.window_band == Some(WindowBand::Early)
        });
        let outside_side = input.cases.iter().any(|c| {
            c.mode == mode
                && c.scenario == Scenario::SideTalk
                && c.window_band == Some(WindowBand::Outside)
        });
        checks.push(Check::new(
            &format!("{}_call_and_side_talk_boundaries", mode.name()),
            outside_call && inside_side && outside_side,
            "calls outside the window; side talk both inside and outside".into(),
        ));
    }
    let speakers: BTreeSet<_> = input
        .cases
        .iter()
        .filter(|c| {
            c.mode == Mode::Live
                && c.kind != Kind::Noise
                && c.data_origin == DataOrigin::RealPeople
                && c.consent
        })
        .filter_map(|c| c.speaker_id.as_deref())
        .collect();
    let sessions: BTreeSet<_> = input
        .cases
        .iter()
        .filter(|c| {
            c.mode == Mode::Live
                && c.kind != Kind::Noise
                && c.data_origin == DataOrigin::RealPeople
                && c.consent
        })
        .map(|c| &c.session_id)
        .collect();
    checks.push(Check::new(
        "live_speakers_and_sessions",
        speakers.len() >= 6 && sessions.len() >= 2,
        format!("{} speakers, {} sessions", speakers.len(), sessions.len()),
    ));
    let mut run_keys = BTreeSet::new();
    let mut raw_hashes = BTreeSet::new();
    let mut rows = Vec::new();
    let mut groups = Vec::new();
    let expected_legs: &[&str] = if input.purpose == Purpose::OneSensorAddition {
        &["A1", "B1", "B2", "A2"]
    } else {
        &["C", "D"]
    };
    for run in &input.runs {
        if !expected_legs.contains(&run.leg.as_str())
            || !run_keys.insert((run.mode, run.leg.as_str()))
            || !run.identity.valid()
            || !sha(&run.raw_manifest_sha256, 64)
            || !token(&run.clock_id)
            || sensors(&run.enabled_sensors).is_none()
            || !["lamp_live", "replay", "synthetic"].contains(&run.evidence.as_str())
        {
            return Err("invalid or duplicate whole-bench run".into());
        }
        let candidate = run.leg.starts_with('B') || run.leg == "C";
        let expected_identity = if candidate {
            &input.candidate
        } else {
            &input.baseline
        };
        let enabled = sensors(&run.enabled_sensors).expect("checked sensors");
        let sensor_set_ok = input.purpose != Purpose::OneSensorAddition
            || enabled
                == if candidate {
                    candidate_sensors.clone()
                } else {
                    accepted.clone()
                };
        checks.push(Check::new(&format!("{}_{}_provenance", run.mode.name(), run.leg),
            run.identity == *expected_identity && run.unchanged_config_sha256 == input.unchanged_config_sha256
                && run.kit_sha256 == input.kit_sha256 && run.cases_sha256 == input.cases_sha256
                && raw_hashes.insert(run.raw_manifest_sha256.clone()) && run.started_unix_s >= input.protocol_frozen_unix_s
                && run.evidence == "lamp_live" && sensor_set_ok && (run.combined_judge || input.purpose == Purpose::BaselineCharacterization),
            "same frozen kit/cases/unchanged settings; distinct raw captures; exactly one addition to one judge".into()));
        checks.push(Check::new(
            &format!("{}_{}_clock", run.mode.name(), run.leg),
            run.clock_verified && run.sync_error_us.is_some_and(|v| v <= MAX_SYNC_ERROR_US),
            "measured source-clock alignment <=5000 us, separately per leg".into(),
        ));
        let mut lines = BTreeMap::new();
        let mut captures = BTreeSet::new();
        for line in &run.lines {
            if !ids.contains(&(run.mode, line.id.as_str()))
                || lines.insert(line.id.as_str(), line).is_some()
                || line.speech_end_us < line.speech_start_us
                || line.first_audio_us.is_some_and(|t| t < line.speech_end_us)
                || line
                    .interrupt_onset_us
                    .zip(line.last_audio_us)
                    .is_some_and(|(a, b)| b < a)
                || line
                    .observation_end_us
                    .is_some_and(|t| t < line.speech_end_us)
                || line
                    .interrupt_onset_us
                    .is_some_and(|t| t < line.speech_start_us || t > line.speech_end_us)
                || line
                    .planned_lamp_audio_end_us
                    .is_some_and(|t| t <= line.speech_start_us)
            {
                return Err("invalid, duplicate or backward whole-bench result".into());
            }
            if !captures.insert((
                line.clock_id.as_str(),
                line.speech_start_us,
                line.speech_end_us,
            )) {
                return Err("whole-bench cases reuse the same physical capture interval".into());
            }
        }
        // Each case is an independent episode, so one physical interval cannot
        // be relabelled or subdivided into overlapping planned observations.
        let mut previous = None;
        for (clock, start, end) in captures {
            if previous
                .is_some_and(|(last_clock, last_end)| clock == last_clock && start < last_end)
            {
                return Err("whole-bench cases have overlapping physical capture intervals".into());
            }
            previous = Some((clock, end));
        }
        for scenario in SCENARIOS {
            let mut group = Group {
                leg: run.leg.clone(),
                mode: run.mode,
                scenario,
                planned: 0,
                correct: 0,
                missing: 0,
                unknown: 0,
                timing_unknown: 0,
                safety_violations: 0,
                answer_timing_expected: false,
                stop_timing_expected: false,
                answer_timing_samples: 0,
                stop_timing_samples: 0,
                answer_p50_us: None,
                answer_p95_us: None,
                stop_p50_us: None,
                stop_p95_us: None,
            };
            let mut answers = Vec::new();
            let mut stops = Vec::new();
            for case in input
                .cases
                .iter()
                .filter(|c| c.mode == run.mode && c.scenario == scenario)
            {
                let row = evaluate(case, run, lines.get(case.id.as_str()));
                group.answer_timing_expected |= case.expected == Action::Answer;
                group.stop_timing_expected |= case.expected == Action::Stop;
                group.planned += 1;
                group.correct += usize::from(row.correct);
                group.missing += usize::from(row.missing);
                group.unknown += usize::from(row.unknown);
                group.timing_unknown += usize::from(row.timing_unknown);
                group.safety_violations += usize::from(row.safety_violation);
                if !row.unknown
                    && !row.timing_unknown
                    && let Some(latency) = row.latency_us
                {
                    if case.expected == Action::Answer {
                        answers.push(latency);
                    }
                    if case.expected == Action::Stop {
                        stops.push(latency);
                    }
                }
                rows.push(row);
            }
            group.answer_p50_us = quantile(&answers, 50);
            group.answer_p95_us = quantile(&answers, 95);
            group.stop_p50_us = quantile(&stops, 50);
            group.stop_p95_us = quantile(&stops, 95);
            group.answer_timing_samples = answers.len();
            group.stop_timing_samples = stops.len();
            checks.push(Check::new(
                &format!(
                    "{}_{}_{}_evidence",
                    run.mode.name(),
                    run.leg,
                    scenario.name()
                ),
                group.missing == 0
                    && group.unknown == 0
                    && group.timing_unknown == 0
                    && group.safety_violations == 0
                    && !run.safety_violation,
                format!(
                    "{} missing, {} unknown, {} timing unknown, {} safety violations",
                    group.missing, group.unknown, group.timing_unknown, group.safety_violations
                ),
            ));
            if candidate && scenario == Scenario::Overlap {
                checks.push(Check::new(&format!("{}_{}_interrupt_p95", run.mode.name(), run.leg), group.stop_p95_us.zip(run.sync_error_us).is_some_and(|(v,e)| v.saturating_add(e) <= INTERRUPT_LIMIT_US), "real interruption onset to last actual output <=400000 us at p95, including clock error".into()));
                for kind in [
                    Kind::Interruption,
                    Kind::Backchannel,
                    Kind::Room,
                    Kind::Noise,
                ] {
                    let selected: Vec<_> = input
                        .cases
                        .iter()
                        .filter(|c| {
                            c.mode == run.mode && c.scenario == Scenario::Overlap && c.kind == kind
                        })
                        .collect();
                    let correct = selected
                        .iter()
                        .filter(|c| {
                            rows.iter().any(|r| {
                                r.leg == run.leg
                                    && r.mode == run.mode
                                    && r.case_id == c.id
                                    && r.correct
                            })
                        })
                        .count();
                    checks.push(Check::new(&format!("{}_{}_{kind:?}_overlap_bar", run.mode.name(), run.leg), !selected.is_empty() && correct * 100 >= selected.len() * 95,
                        format!("{correct}/{}; >=95% independently in this class, with unknowns and missing included", selected.len())));
                }
            }
            groups.push(group);
        }
    }
    let mut target_gains = Vec::new();
    for mode in [Mode::Played, Mode::Live] {
        let selected: Vec<_> = input.runs.iter().filter(|r| r.mode == mode).collect();
        checks.push(Check::new(
            &format!("{}_run_order", mode.name()),
            selected
                .iter()
                .map(|r| r.leg.as_str())
                .eq(expected_legs.iter().copied())
                && selected
                    .windows(2)
                    .all(|w| w[0].started_unix_s < w[1].started_unix_s),
            "complete recorded-order A1/B1/B2/A2, or report-only C/D".into(),
        ));
        if input.purpose != Purpose::OneSensorAddition {
            continue;
        }
        for (a, b) in [("A1", "B1"), ("A2", "B2")] {
            let a_error = input
                .runs
                .iter()
                .find(|r| r.mode == mode && r.leg == a)
                .and_then(|r| r.sync_error_us);
            let b_error = input
                .runs
                .iter()
                .find(|r| r.mode == mode && r.leg == b)
                .and_then(|r| r.sync_error_us);
            for scenario in SCENARIOS {
                let baseline = groups
                    .iter()
                    .find(|g| g.mode == mode && g.leg == a && g.scenario == scenario);
                let candidate = groups
                    .iter()
                    .find(|g| g.mode == mode && g.leg == b && g.scenario == scenario);
                let complete = baseline.zip(candidate);
                let no_drop = complete.is_some_and(|(x, y)| {
                    y.planned == x.planned
                        && y.correct >= x.correct
                        && y.missing <= x.missing
                        && y.unknown <= x.unknown
                        && y.timing_unknown <= x.timing_unknown
                        && no_latency_drop(
                            x.answer_p50_us,
                            y.answer_p50_us,
                            x.answer_timing_expected,
                            a_error,
                            b_error,
                        )
                        && no_latency_drop(
                            x.answer_p95_us,
                            y.answer_p95_us,
                            x.answer_timing_expected,
                            a_error,
                            b_error,
                        )
                        && no_latency_drop(
                            x.stop_p50_us,
                            y.stop_p50_us,
                            x.stop_timing_expected,
                            a_error,
                            b_error,
                        )
                        && no_latency_drop(
                            x.stop_p95_us,
                            y.stop_p95_us,
                            x.stop_timing_expected,
                            a_error,
                            b_error,
                        )
                });
                checks.push(Check::new(&format!("{}_{a}_{b}_{}_no_drop", mode.name(), scenario.name()), no_drop, "fixed planned counts; accuracy/coverage and p50/p95 may not regress; unmatched timing populations do not prove no regression".into()));
                for expected in [Action::Answer, Action::Stop] {
                    let mut before = Vec::new();
                    let mut after = Vec::new();
                    for case in input.cases.iter().filter(|c| {
                        c.mode == mode && c.scenario == scenario && c.expected == expected
                    }) {
                        let x = rows
                            .iter()
                            .find(|r| r.mode == mode && r.leg == a && r.case_id == case.id);
                        let y = rows
                            .iter()
                            .find(|r| r.mode == mode && r.leg == b && r.case_id == case.id);
                        if let Some((x, y)) = x.zip(y)
                            && !x.unknown
                            && !y.unknown
                            && !x.timing_unknown
                            && !y.timing_unknown
                            && let Some((x, y)) = x.latency_us.zip(y.latency_us)
                        {
                            before.push(x);
                            after.push(y);
                        }
                    }
                    let needed = input.cases.iter().any(|c| {
                        c.mode == mode && c.scenario == scenario && c.expected == expected
                    });
                    let no_drop = [50, 95].into_iter().all(|p| {
                        no_latency_drop(
                            quantile(&before, p),
                            quantile(&after, p),
                            needed,
                            a_error,
                            b_error,
                        )
                    });
                    checks.push(Check::new(&format!("{}_{a}_{b}_{}_{expected:?}_paired_latency", mode.name(), scenario.name()), no_drop,
                        format!("{} common cases; candidate upper clock bound <= baseline lower bound at p50 and p95", before.len())));
                }
                if input.target_scenario == Some(scenario) {
                    let gain = complete.map(|(x, y)| y.correct as i64 - x.correct as i64);
                    checks.push(Check::new(
                        &format!("{}_{a}_{b}_target_improves", mode.name()),
                        gain.is_some_and(|v| v >= 1),
                        "at least one additional correct target case in each counterbalanced pair"
                            .into(),
                    ));
                    if let Some((x, y)) = complete {
                        target_gains.push(Gain {
                            mode,
                            baseline_leg: a.into(),
                            candidate_leg: b.into(),
                            scenario,
                            planned: x.planned,
                            additional_correct: y.correct as i64 - x.correct as i64,
                            percentage_points: if x.planned == 0 {
                                0.0
                            } else {
                                100.0 * (y.correct as f64 - x.correct as f64) / x.planned as f64
                            },
                        });
                    }
                }
            }
            let no_paired_drop = input.cases.iter().filter(|c| c.mode == mode).all(|case| {
                let x = rows
                    .iter()
                    .find(|r| r.mode == mode && r.leg == a && r.case_id == case.id);
                let y = rows
                    .iter()
                    .find(|r| r.mode == mode && r.leg == b && r.case_id == case.id);
                x.zip(y).is_some_and(|(x, y)| !x.correct || y.correct)
            });
            checks.push(Check::new(&format!("{}_{a}_{b}_paired_no_loss", mode.name()), no_paired_drop, "zero cases correct before but incorrect after; gains cannot compensate a lost case".into()));
            let mut slices: BTreeMap<String, Vec<&Case>> = BTreeMap::new();
            for case in input.cases.iter().filter(|c| c.mode == mode) {
                for key in [
                    Some(format!("kind:{:?}", case.kind)),
                    case.speaker_id.as_ref().map(|s| format!("speaker:{s}")),
                    Some(format!("session:{}", case.session_id)),
                    case.window_band.map(|w| format!("window:{w:?}")),
                ]
                .into_iter()
                .flatten()
                {
                    slices.entry(key).or_default().push(case);
                }
            }
            for (slice, cases) in slices {
                let count = |leg: &str| {
                    cases
                        .iter()
                        .filter(|c| {
                            rows.iter().any(|r| {
                                r.mode == mode && r.leg == leg && r.case_id == c.id && r.correct
                            })
                        })
                        .count()
                };
                let (before, after) = (count(a), count(b));
                checks.push(Check::new(
                    &format!("{}_{a}_{b}_{slice}_no_drop", mode.name()),
                    after >= before,
                    format!(
                        "{before}/{} before, {after}/{} after; fixed slice denominator",
                        cases.len(),
                        cases.len()
                    ),
                ));
            }
        }
    }
    Ok(Report {
        version: 2,
        rubric: RUBRIC.into(),
        purpose: input.purpose,
        passed: checks.iter().all(|c| c.passed),
        checks,
        groups,
        rows,
        target_gains,
    })
}
