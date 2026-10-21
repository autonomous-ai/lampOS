//! Report validity on the first eight-step native serve conversation
//! (Lamp-4ace: b318 lamp-voice, e1dc relay; summary in
//! `docs/verification/final-serve-pilot-20261012`). Trace rows use lamp-voice's
//! coordinator record shapes in the order it writes them: a revoke records the
//! old turn's `user_interrupted` finish, then admits and records the successor.
//! The interruption pair is the run's own: turn 3 finished at 64,624,013,887 us
//! and turn 4 was admitted 153 us later under the same boot, owner generation
//! 7 -> 9, after a ~470 ms duplex yield that began at the speech candidate.
//! Turn 5 was superseded before output by the `local_endpoint` correction.
//! Every local endpoint reports a 1,200 ms silence wait. The observer metadata
//! fragment carries the run's queue overflow. Other times are reconstructed;
//! no recording is used and nothing here is acoustic evidence.
use crate::common;

use lamp_voice_eval::{
    annotations,
    evaluate::{self, AnswerOutcome, AttemptScore, CheckStatus, FindingKind, Metric, Severity},
    events::{ClockDomain, parse_trace},
    import,
    ledger::{Ledger, create_run_directory},
    plan::{LoadedPlan, SERVE_PLAN, TriggerEvent},
    record::{
        AttemptRecord, AttemptStatus, Attribution, ClockMapping, StepRecord, StepStatus,
        StimulusSource, Stratum, TriggerObservation,
    },
    report,
    stimulus::StimulusCatalog,
};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

const BIN: &str = env!("CARGO_BIN_EXE_lamp-voice-eval");
/// Turn 3's `user_interrupted` finish and turn 4's admission in the 4ace trace.
const CANCEL_3: u64 = 64_624_013_887;
const ADMIT_4: u64 = 64_624_014_040;
/// Speech candidate -> revoke of the playing reply (retained duplex yield).
const YIELD_US: u64 = 470_000;
const BOOT: [u8; 16] = [0x4a; 16];
/// Runner time = lamp time - OFFSET: a ping/pong mapping, not one clock.
const OFFSET: u64 = 1_250_000_000;
const TIME_KEYS: [&str; 5] = [
    "at_us",
    "authority_issued_at_us",
    "prefix_first_host_read_us",
    "last_block_host_read_us",
    "first_read_at_us",
];

/// Each admission follows a revoke or completion, so owners advance by two:
/// turn 3 holds generation 7 and its successor 9.
fn owner(turn: u64) -> Value {
    json!({"boot": BOOT, "turn": turn, "generation": 1 + 2 * turn})
}
fn candidate_id(turn: u64) -> Value {
    json!({"controller": BOOT, "serial": turn})
}

struct Fixture {
    rows: Vec<Value>,
    steps: Vec<StepRecord>,
}

struct Build<'a> {
    catalog: &'a StimulusCatalog,
    plan: &'a LoadedPlan,
    rows: Vec<Value>,
    steps: Vec<StepRecord>,
}

impl Build<'_> {
    fn push(&mut self, at: u64, mut row: Value) {
        row["at_us"] = json!(at);
        self.rows.push(row);
    }
    /// Start step `index` at `start` (lamp time); returns speech (onset, end).
    fn step(
        &mut self,
        index: usize,
        event: TriggerEvent,
        turn: Option<u64>,
        event_at: u64,
        start: u64,
    ) -> (u64, u64) {
        let planned = &self.plan.plan.scenarios[0].steps[index];
        let scene = planned.scene.as_deref().unwrap();
        let timing = self.catalog.estimate(scene).unwrap();
        let (onset, end) = timing.speech_span_ms.unwrap_or((0, timing.duration_ms));
        let mut record = StepRecord::planned(planned, StepStatus::Injected);
        record.trigger = Some(TriggerObservation {
            event,
            turn,
            event_at_us: Some(event_at),
            event_domain: ClockDomain::LampMonotonic,
            runner_us: event_at,
            runner_time_method: "cue receipt mapped by ping/pong".into(),
        });
        record.target_us = Some(event_at + u64::from(planned.trigger.delay_ms) * 1000);
        record.started_us = Some(start);
        record.bound_turn = turn;
        record.timing = Some(timing);
        self.steps.push(record);
        (
            start + u64::from(onset) * 1000,
            start + u64::from(end) * 1000,
        )
    }
    fn candidate(&mut self, turn: u64, at: u64, onset: u64) {
        self.push(
            at,
            json!({"kind":"input_candidate","candidate":{"id":candidate_id(turn),
                "trigger_sequence":1000 + turn,"first_read_at_us":onset,"displaced_owner":null}}),
        );
    }
    fn admit(&mut self, turn: u64, at: u64, onset: u64) {
        self.push(
            at,
            json!({"kind":"input_admitted","owner":owner(turn),"turn":turn,
                "authority_issued_at_us":at - 40,"prefix_first_host_read_us":onset,
                "candidate":candidate_id(turn),"admission_basis":"directed_session_duplex",
                "provider_session":1}),
        );
    }
    fn endpoint(&mut self, turn: u64, at: u64) {
        self.push(
            at,
            json!({"kind":"local_endpoint","owner":owner(turn),"turn":turn,
                "last_block_host_read_us":at - 10_000,"includes_silence_wait_ms":1200,
                "acoustic_speech_end":null}),
        );
    }
    fn first_write(&mut self, turn: u64, endpoint: u64) -> u64 {
        self.push(
            endpoint + 900_000,
            json!({"kind":"provider_first_audio","turn":turn}),
        );
        let write = endpoint + 1_250_000;
        self.push(
            write,
            json!({"kind":"speaker_first_write","owner":owner(turn),"turn":turn,
                "playback_sequence":turn,"boundary":"ALSA accepted; acoustic onset unmeasured"}),
        );
        write
    }
    fn complete(&mut self, turn: u64, write: u64, gaps: u64) -> u64 {
        let retired = write + 3_000_000;
        self.push(
            retired,
            json!({"kind":"speech_final_sample_retired","owner":owner(turn),"turn":turn,
                "playback_sequence":turn}),
        );
        let outcome = if gaps > 0 {
            "audio_written_with_playback_gaps_unscored"
        } else {
            "audio_written_unscored"
        };
        self.push(
            retired + 2_000,
            json!({"kind":"turn_finished","turn":turn,"generation":1 + 2 * turn,
                "outcome":outcome,"playback_gaps":gaps,"provider_session":1,
                "provider_outcome":{"kind":"complete"}}),
        );
        retired + 2_000
    }
    fn cancel(&mut self, turn: u64, at: u64, audio: bool, gaps: u64) {
        self.push(
            at,
            json!({"kind":"turn_finished","owner":owner(turn),"turn":turn,
                "outcome":"user_interrupted","provider_audio_seen":audio,"playback_gaps":gaps,
                "provider_session":1,"provider_outcome":null}),
        );
    }
    /// An addressed request admitted at its candidate and answered in full.
    fn answered(&mut self, turn: u64, (onset, end): (u64, u64), gaps: u64) -> u64 {
        self.candidate(turn, onset + 300_000, onset);
        self.admit(turn, onset + 301_000, onset);
        self.endpoint(turn, end + 1_200_000);
        let write = self.first_write(turn, end + 1_200_000);
        self.complete(turn, write, gaps)
    }
}

/// The 4ace conversation: sky (gap), why, sunset (yields to coffee), coffee,
/// more (superseded before output), correct, change, math.
fn conversation() -> Fixture {
    let catalog = StimulusCatalog::load().unwrap();
    let plan = LoadedPlan::parse(SERVE_PLAN, &catalog).unwrap();
    let mut b = Build {
        catalog: &catalog,
        plan: &plan,
        rows: Vec::new(),
        steps: Vec::new(),
    };
    b.push(
        0,
        json!({"kind":"run_start","provider_kind":"gemini","acoustic_score":null}),
    );
    let ready = 2_000_000;
    b.push(ready, json!({"kind":"listening_ready"}));
    let span = b.step(
        0,
        TriggerEvent::ListeningReady,
        None,
        ready,
        ready + 502_000,
    );
    let mut finished = b.answered(1, span, 1);
    for (index, turn, delay) in [(1, 2, 500_000), (2, 3, 600_000)] {
        let at = finished + 100_000;
        let span = b.step(
            index,
            TriggerEvent::NextTurnReady,
            Some(turn - 1),
            at,
            at + delay + 2_000,
        );
        if turn == 2 {
            finished = b.answered(turn, span, 0);
        } else {
            b.candidate(3, span.0 + 300_000, span.0);
            b.admit(3, span.0 + 301_000, span.0);
            b.endpoint(3, span.1 + 1_200_000);
            finished = b.first_write(3, span.1 + 1_200_000);
        }
    }
    // Coffee interrupts the playing sunset reply: candidate, duplex yield,
    // revoke of turn 3, then turn 4's admission.
    let write_3 = finished;
    let (onset, end) = b.step(
        3,
        TriggerEvent::SpeakerFirstWrite,
        Some(3),
        write_3,
        write_3 + 1_202_000,
    );
    let candidate_at = onset + 300_000;
    b.candidate(4, candidate_at, onset);
    b.cancel(3, candidate_at + YIELD_US, true, 1);
    b.admit(4, candidate_at + YIELD_US + 153, onset);
    b.endpoint(4, end + 1_200_000);
    let write = b.first_write(4, end + 1_200_000);
    finished = b.complete(4, write, 0);
    // A rapid follow-up, corrected at its local endpoint before any output.
    let at = finished + 100_000;
    let (onset, end) = b.step(4, TriggerEvent::NextTurnReady, Some(4), at, at + 152_000);
    b.candidate(5, onset + 300_000, onset);
    b.admit(5, onset + 301_000, onset);
    let endpoint_5 = end + 1_200_000;
    b.endpoint(5, endpoint_5);
    let (onset, end) = b.step(
        5,
        TriggerEvent::LocalEndpoint,
        Some(5),
        endpoint_5,
        endpoint_5 + 3_000,
    );
    b.candidate(6, onset + 300_000, onset);
    b.cancel(5, onset + 301_000, false, 0);
    b.admit(6, onset + 301_150, onset);
    b.endpoint(6, end + 1_200_000);
    let write = b.first_write(6, end + 1_200_000);
    finished = b.complete(6, write, 0);
    for (index, turn, delay) in [(6, 7, 700_000), (7, 8, 3_000_000)] {
        let at = finished + 100_000;
        let span = b.step(
            index,
            TriggerEvent::NextTurnReady,
            Some(turn - 1),
            at,
            at + delay + 2_000,
        );
        finished = b.answered(turn, span, 0);
    }
    b.push(
        finished + 1_000_000,
        json!({"kind":"run_end","status":"completed_unscored","error":null}),
    );
    // Place the reconstruction on the run's own clock: turn 3's revoke.
    let cancel_3 = b
        .rows
        .iter()
        .find(|r| r["kind"] == "turn_finished" && r["turn"] == 3)
        .and_then(|r| r["at_us"].as_u64())
        .unwrap();
    let shift = CANCEL_3 - cancel_3;
    for row in &mut b.rows {
        for key in TIME_KEYS {
            if let Some(at) = row[key].as_u64() {
                row[key] = json!(at + shift);
            }
        }
    }
    for step in &mut b.steps {
        let trigger = step.trigger.as_mut().unwrap();
        let event_at = trigger.event_at_us.unwrap() + shift;
        trigger.event_at_us = Some(event_at);
        trigger.runner_us = event_at - OFFSET;
        step.target_us = step.target_us.map(|t| t + shift - OFFSET);
        step.started_us = step.started_us.map(|t| t + shift - OFFSET);
    }
    b.rows.sort_by_key(|r| r["at_us"].as_u64());
    Fixture {
        rows: b.rows,
        steps: b.steps,
    }
}

fn record(fixture: &Fixture) -> AttemptRecord {
    let catalog = StimulusCatalog::load().unwrap();
    let plan = LoadedPlan::parse(SERVE_PLAN, &catalog).unwrap();
    let scenario = &plan.plan.scenarios[0];
    let text = fixture
        .rows
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let (events, _) = parse_trace(&text).unwrap();
    AttemptRecord {
        attempt_id: "4ace-serve-conversation".into(),
        run_id: "final-serve-4ace".into(),
        scenario: scenario.id.clone(),
        cohort: scenario.cohort,
        capability: scenario.capability,
        provider: scenario.provider,
        stratum: Stratum::PhysicalServe,
        source: StimulusSource::LoudspeakerSynthetic,
        repetition: 1,
        order: 0,
        profile: None,
        seed: Some(1),
        step_domain: ClockDomain::RunnerMonotonic,
        steps: fixture.steps.clone(),
        events,
        live_events: Vec::new(),
        clock: Some(ClockMapping {
            from: ClockDomain::RunnerMonotonic,
            to: ClockDomain::LampMonotonic,
            offset_us: OFFSET as i64,
            uncertainty_us: 2_000,
            path_allowance_us: 250_000,
            method: "ping/pong over the session transport".into(),
        }),
        attribution: Attribution::Timed,
        status: AttemptStatus::Completed,
        spoken_failure_notice: None,
        evidence: json!({"room_audio": null}),
        unmapped_trace_records: 0,
        reproduce: "report_validity fixture".into(),
    }
}

fn score(record: &AttemptRecord) -> AttemptScore {
    evaluate::score(record, 15_000, None)
}

fn step<'a>(score: &'a AttemptScore, id: &str) -> &'a evaluate::StepScore {
    score.steps.iter().find(|s| s.step == id).unwrap()
}

fn findings(score: &AttemptScore, kind: FindingKind) -> Vec<&evaluate::Finding> {
    score.findings.iter().filter(|f| f.kind == kind).collect()
}

fn unmatched(score: &AttemptScore) -> Vec<&evaluate::Finding> {
    score
        .findings
        .iter()
        .filter(|f| f.detail.contains("without a matching admission"))
        .collect()
}

fn row_mut<'a>(fixture: &'a mut Fixture, kind: &str, turn: u64) -> &'a mut Value {
    fixture
        .rows
        .iter_mut()
        .find(|r| r["kind"] == kind && r["turn"] == turn)
        .unwrap()
}

#[test]
fn the_fixture_keeps_the_runs_exact_interruption_order() {
    let fixture = conversation();
    let at = |kind: &str, turn: u64| {
        fixture
            .rows
            .iter()
            .position(|r| r["kind"] == kind && r["turn"] == turn)
            .unwrap()
    };
    let cancel = &fixture.rows[at("turn_finished", 3)];
    let admit = &fixture.rows[at("input_admitted", 4)];
    assert_eq!(cancel["at_us"], CANCEL_3);
    assert_eq!(admit["at_us"], ADMIT_4);
    assert_eq!(at("input_admitted", 4), at("turn_finished", 3) + 1);
    assert_eq!(cancel["owner"]["generation"], 7);
    assert_eq!(admit["owner"]["generation"], 9);
    assert_eq!(cancel["owner"]["boot"], admit["owner"]["boot"]);
    let candidate = fixture
        .rows
        .iter()
        .find(|r| r["kind"] == "input_candidate" && r["candidate"]["id"]["serial"] == 4)
        .unwrap();
    assert_eq!(CANCEL_3 - candidate["at_us"].as_u64().unwrap(), YIELD_US);
    assert!(
        fixture
            .rows
            .iter()
            .filter(|r| r["kind"] == "local_endpoint")
            .all(|r| r["includes_silence_wait_ms"] == 1200)
    );
    // Turn 5 never had provider audio or an ALSA write.
    assert!(!fixture.rows.iter().any(|r| r["turn"] == 5
        && matches!(
            r["kind"].as_str(),
            Some("provider_first_audio" | "speaker_first_write")
        )));
}

#[test]
fn the_4ace_conversation_scores_its_planned_yield_and_correction() {
    let score = score(&record(&conversation()));
    assert!(unmatched(&score).is_empty(), "{:#?}", score.findings);
    assert!(
        findings(&score, FindingKind::FalseInterruption).is_empty(),
        "{:#?}",
        score.findings
    );
    assert!(findings(&score, FindingKind::MissedInterruption).is_empty());
    assert!(findings(&score, FindingKind::TruncatedAnswer).is_empty());
    assert_eq!(
        step(&score, "sunset").answer,
        Some(AnswerOutcome::YieldedAsPlanned)
    );
    assert_eq!(step(&score, "coffee").interruption, Some(true));
    assert_eq!(step(&score, "coffee").answer, Some(AnswerOutcome::Complete));
    assert_eq!(
        step(&score, "correct").answer,
        Some(AnswerOutcome::Complete)
    );
    assert_eq!(step(&score, "correct").status, CheckStatus::Pass);
    assert!(matches!(
        step(&score, "more").status,
        CheckStatus::Unscored { .. }
    ));
    // What remains is the run's real defect: turn 1's accepted-zero gap.
    let failing: Vec<_> = score
        .findings
        .iter()
        .filter(|f| matches!(f.severity, Severity::Failure | Severity::Defect))
        .map(|f| (f.kind, f.step.clone()))
        .collect();
    assert_eq!(
        failing,
        vec![(FindingKind::PlaybackGaps, Some("sky".into()))]
    );
    // The interruption decision is still measured from the speech candidate.
    let decision = score
        .latencies
        .iter()
        .find(|l| l.metric == Metric::AdmissionToCancel)
        .unwrap();
    assert!(
        (decision.value_ms - 470.0).abs() < 0.01,
        "{}",
        decision.value_ms
    );
}

#[test]
fn a_cancellation_with_no_successor_admission_stays_a_runtime_failure() {
    let mut fixture = conversation();
    fixture
        .rows
        .retain(|r| !(r["kind"] == "input_admitted" && r["turn"] == 4));
    let score = score(&record(&fixture));
    assert_eq!(unmatched(&score).len(), 1, "{:#?}", score.findings);
    assert_eq!(unmatched(&score)[0].turn, Some(3));
    assert_eq!(unmatched(&score)[0].severity, Severity::Failure);
}

#[test]
fn a_late_successor_admission_does_not_explain_the_cancellation() {
    let mut fixture = conversation();
    row_mut(&mut fixture, "input_admitted", 4)["at_us"] = json!(CANCEL_3 + 25_000);
    let score = score(&record(&fixture));
    assert_eq!(unmatched(&score).len(), 1, "{:#?}", score.findings);
    assert!(
        unmatched(&score)[0].detail.contains("25.0 ms after"),
        "{}",
        unmatched(&score)[0].detail
    );
    assert_ne!(step(&score, "coffee").interruption, Some(true));
}

#[test]
fn a_foreign_boot_or_stale_generation_admission_is_not_the_cause() {
    let mut foreign = conversation();
    row_mut(&mut foreign, "input_admitted", 4)["owner"]["boot"] =
        serde_json::to_value([0x5b_u8; 16]).unwrap();
    let mut stale = conversation();
    row_mut(&mut stale, "input_admitted", 4)["owner"]["generation"] = json!(7);
    for fixture in [foreign, stale] {
        let score = score(&record(&fixture));
        assert_eq!(unmatched(&score).len(), 1, "{:#?}", score.findings);
        assert_ne!(step(&score, "coffee").interruption, Some(true));
    }
}

#[test]
fn an_earlier_turn_cannot_have_caused_a_later_turns_cancellation() {
    // Turn 4's revoke recorded inside turn 3's admission window: the older
    // turn is not a successor even at the same instant.
    let mut fixture = conversation();
    let admitted_3 = row_mut(&mut fixture, "input_admitted", 3)["at_us"]
        .as_u64()
        .unwrap();
    fixture
        .rows
        .push(json!({"kind":"turn_finished","owner":owner(4),"turn":4,
        "at_us":admitted_3 + 50,"outcome":"user_interrupted","provider_audio_seen":false,
        "playback_gaps":0}));
    let score = score(&record(&fixture));
    assert!(
        unmatched(&score).iter().any(|f| f.turn == Some(4)),
        "{:#?}",
        score.findings
    );
}

#[test]
fn only_the_bound_before_output_correction_supersedes_a_pending_reply() {
    // Bound to another turn: not this reply's correction.
    let mut fixture = conversation();
    fixture.steps[5].bound_turn = Some(4);
    fixture.steps[5].trigger.as_mut().unwrap().turn = Some(4);
    let score_other = score(&record(&fixture));
    // An ordinary next-turn request that cancels a pending reply.
    let mut fixture = conversation();
    fixture.steps[5].trigger.as_mut().unwrap().event = TriggerEvent::NextTurnReady;
    let score_request = score(&record(&fixture));
    for score in [score_other, score_request] {
        let false_interruptions = findings(&score, FindingKind::FalseInterruption);
        assert_eq!(false_interruptions.len(), 1, "{:#?}", score.findings);
        assert_eq!(false_interruptions[0].turn, Some(5));
        assert_eq!(false_interruptions[0].step.as_deref(), Some("correct"));
    }
}

#[test]
fn a_correction_admitted_after_output_started_is_not_a_tested_supersession() {
    let mut fixture = conversation();
    let cancel_5 = row_mut(&mut fixture, "turn_finished", 5)["at_us"]
        .as_u64()
        .unwrap();
    row_mut(&mut fixture, "turn_finished", 5)["provider_audio_seen"] = json!(true);
    fixture.rows.push(
        json!({"kind":"speaker_first_write","owner":owner(5),"turn":5,
        "at_us":cancel_5 - 200_000,"playback_sequence":5}),
    );
    fixture.rows.sort_by_key(|r| r["at_us"].as_u64());
    let score = score(&record(&fixture));
    assert!(findings(&score, FindingKind::FalseInterruption).is_empty());
    let lost = findings(&score, FindingKind::PreconditionLost);
    assert_eq!(lost.len(), 1, "{:#?}", score.findings);
    assert_eq!(lost[0].severity, Severity::Withheld);
    assert_eq!(lost[0].turn, Some(5));
    assert!(lost[0].detail.contains("after its output started"));
}

#[test]
fn the_endpoint_boundary_states_the_runtimes_observed_silence_wait() {
    let record = record(&conversation());
    let score = score(&record);
    let summary = report::build(
        "r",
        "p",
        "sha",
        std::slice::from_ref(&score),
        Vec::new(),
        Vec::new(),
    );
    let text = serde_json::to_string(&summary).unwrap();
    assert!(!text.contains("600 ms"), "{text}");
    let markdown = report::markdown(
        &summary,
        &LoadedPlan::parse(SERVE_PLAN, &StimulusCatalog::load().unwrap())
            .unwrap()
            .plan
            .targets,
    );
    assert!(!markdown.contains("600 ms"));
    assert!(
        markdown.contains("1200 ms silence wait reported by all 8 endpoint records"),
        "{markdown}"
    );
    assert!(
        score
            .latencies
            .iter()
            .filter(|l| l.metric == Metric::EndpointToFirstWrite)
            .count()
            == 7
    );
}

/// The observer's terminal metadata for the 4ace room recording.
fn room_4ace() -> Value {
    json!({
        "valid": false, "status": "invalid_capture",
        "stop_reason": "target_frames_received_and_tail_checked",
        "delivered_format": {"sample_rate": 48_000, "channels": 1, "input_sample_format": "f32"},
        "requested_frames": 15_840_000_u64,
        "capture": {"faults": 4, "queue_dropped_frames": 13_824, "rejected_frames": 0,
            "written_frames": 15_826_176_u64, "received_frames": 15_840_000_u64,
            "stream_errors": 0, "backend_xruns": 0, "timestamp_discontinuities": 0},
        "writer": {"written_frames": 15_826_176_u64, "written_packets": 30_911,
            "source_sequence_gaps": 1},
        "fault_bit_definitions": {"1": "malformed_callback", "2": "oversized_callback",
            "4": "queue_full", "8": "timestamp_discontinuity", "16": "nonfinite_sample",
            "32": "digital_rail_clipping", "64": "stream_error", "128": "callback_count_limit",
            "256": "backend_xrun"},
        "room_wav_sha256": "a".repeat(64),
        "start_requested_host_ns": 9_000_000_000_u64,
        "errors": [],
    })
}

fn with_room(record: &mut AttemptRecord, dir: &Path, metadata: &Value) {
    // `open -W` exits 0 whatever the observer reports: exit is not evidence.
    record.evidence["room_audio"] = import::room_evidence(
        metadata,
        true,
        "room/metadata.json",
        &dir.join("room.wav"),
        Some(true),
    );
}

#[test]
fn the_invalid_room_terminal_report_is_retained_and_surfaced() {
    let dir = common::Private::new("rv-room");
    let mut record = record(&conversation());
    with_room(&mut record, &dir.path, &room_4ace());
    let room = &record.evidence["room_audio"];
    assert_eq!(room["valid"], false);
    assert_eq!(room["recorder_success"], true);
    let terminal = &room["observer_terminal"];
    assert_eq!(terminal["status"], "invalid_capture");
    assert_eq!(terminal["fault_bits"], 4);
    assert_eq!(terminal["faults"], json!(["queue_full"]));
    assert_eq!(terminal["requested_frames"], 15_840_000_u64);
    assert_eq!(terminal["written_frames"], 15_826_176_u64);
    assert_eq!(terminal["queue_dropped_frames"], 13_824);
    assert_eq!(terminal["source_sequence_gaps"], 1);
    assert_eq!(terminal["sample_rate"], 48_000);

    let score = score(&record);
    let room_findings: Vec<_> = score
        .findings
        .iter()
        .filter(|f| f.detail.contains("room recording"))
        .collect();
    assert_eq!(room_findings.len(), 1, "{:#?}", score.findings);
    let finding = room_findings[0];
    assert_eq!(finding.kind, FindingKind::EvidenceMissing);
    assert_eq!(finding.severity, Severity::Withheld);
    for part in [
        "invalid_capture",
        "queue_full",
        "13824",
        "15840000",
        "48000",
    ] {
        assert!(finding.detail.contains(part), "{part}: {}", finding.detail);
    }
    assert!(!score.acoustic.is_empty());
    for acoustic in &score.acoustic {
        let reason = acoustic.unscored_reason.as_deref().unwrap();
        assert!(reason.starts_with("room recording invalid"), "{reason}");
    }
    let state = serde_json::to_value(&score).unwrap()["room_recording"].clone();
    assert_eq!(state["state"], "invalid", "{state}");
    assert_eq!(state["status"], "invalid_capture");

    let markdown = report::attempt_markdown(&record, &score);
    assert!(
        markdown.contains("Independent room recording: **invalid** (invalid_capture"),
        "{}",
        &markdown[..markdown.len().min(1500)]
    );
    let summary = report::build(
        "r",
        "p",
        "sha",
        std::slice::from_ref(&score),
        Vec::new(),
        Vec::new(),
    );
    let json = serde_json::to_value(&summary).unwrap();
    assert_eq!(
        json["strata"][0]["summary"]["room_recordings"]["invalid"], 1,
        "{json}"
    );
    // No listening template is offered for an invalid recording.
    let template = annotations::template("final-serve-4ace", std::slice::from_ref(&record));
    assert!(template.attempts.is_empty());
}

#[test]
fn a_verified_or_absent_room_recording_keeps_its_own_state() {
    let dir = common::Private::new("rv-valid");
    let wav = dir.join("room.wav");
    lamp_sound_fixtures::write_wave(&wav, &vec![0.0; 16_000]).unwrap();
    let sha = lamp_sound_fixtures::hash(&fs::read(&wav).unwrap());
    let metadata = json!({"valid": true, "status": "captured_unscored", "room_wav_sha256": sha,
        "start_requested_host_ns": 1_000_000_000_u64});
    let mut verified = record(&conversation());
    with_room(&mut verified, &dir.path, &metadata);
    let score_verified = score(&verified);
    assert!(
        !score_verified
            .findings
            .iter()
            .any(|f| f.detail.contains("room recording"))
    );
    assert!(
        score_verified
            .acoustic
            .iter()
            .all(|a| a.unscored_reason.as_deref() == Some("no reviewed room-audio annotation"))
    );
    assert_eq!(
        serde_json::to_value(&score_verified).unwrap()["room_recording"]["state"],
        "verified"
    );
    let template = annotations::template("final-serve-4ace", std::slice::from_ref(&verified));
    assert_eq!(template.attempts.len(), 1);

    let absent = record(&conversation());
    let score_absent = score(&absent);
    assert!(
        score_absent
            .acoustic
            .iter()
            .all(|a| a.unscored_reason.as_deref() == Some("no continuous room recording was made"))
    );
    assert_eq!(
        serde_json::to_value(&score_absent).unwrap()["room_recording"]["state"],
        "absent"
    );
}

#[test]
fn a_valid_wav_from_a_recorder_that_ended_mid_session_certifies_nothing() {
    let dir = common::Private::new("rv-cover");
    let wav = dir.join("room.wav");
    lamp_sound_fixtures::write_wave(&wav, &vec![0.0; 16_000]).unwrap();
    let sha = lamp_sound_fixtures::hash(&fs::read(&wav).unwrap());
    let metadata = json!({"valid": true, "status": "captured_unscored", "room_wav_sha256": sha,
        "start_requested_host_ns": 1_000_000_000_u64});
    let lost = |kind: &str| {
        let mut record = record(&conversation());
        with_room(&mut record, &dir.path, &metadata);
        record.evidence["room_audio"]["coverage_lost"] =
            json!({"runner_us": 5_000_000, "kind": kind, "reason": "test"});
        record
    };
    // Exited before any declared end: a granted prompt may be unrecorded.
    let early = lost("recorder_exited_early");
    let score_early = score(&early);
    let state = serde_json::to_value(&score_early.room_recording).unwrap();
    assert_eq!(state["state"], "invalid", "{state}");
    assert_eq!(state["status"], "ended_during_session");
    assert!(
        score_early
            .acoustic
            .iter()
            .all(|a| a.unscored_reason.as_deref()
                == Some("room recording invalid (ended_during_session)"))
    );
    assert!(
        annotations::template("final-serve-4ace", std::slice::from_ref(&early))
            .attempts
            .is_empty()
    );
    // Refused before an uncovered prompt: every granted prompt was covered.
    let refused = lost("prompt_not_covered");
    let score_refused = score(&refused);
    assert_eq!(
        serde_json::to_value(&score_refused.room_recording).unwrap()["state"],
        "verified"
    );
    let page = report::attempt_markdown(&refused, &score_refused);
    assert!(
        page.contains("Room coverage ended during the session (`prompt_not_covered`)"),
        "{}",
        &page[..page.len().min(1500)]
    );
}

/// R1 (Main review of 3c8e3d3b5): a listened annotation of a matching, valid
/// WAV must not score timing or content when its recorder exited early.
#[test]
fn a_listened_annotation_cannot_certify_a_recording_whose_recorder_exited_early() {
    use lamp_voice_eval::annotations::{AttemptAnnotation, StepAnnotation};
    use std::collections::BTreeMap;
    let dir = common::Private::new("rv-r1");
    let wav = dir.join("room.wav");
    // 30 s at 16 kHz: the annotated boundaries lie inside it.
    lamp_sound_fixtures::write_wave(&wav, &vec![0.0; 480_000]).unwrap();
    let sha = lamp_sound_fixtures::hash(&fs::read(&wav).unwrap());
    let metadata = json!({"valid": true, "status": "captured_unscored", "room_wav_sha256": sha,
        "start_requested_host_ns": 1_000_000_000_u64});
    let annotations = |record: &AttemptRecord| {
        let step = StepAnnotation {
            user_speech_end_s: Some(2.0),
            first_substantive_answer_word_s: Some(3.5),
            answer_relevant: Some(false),
            answer_complete: Some(false),
            ..StepAnnotation::default()
        };
        let mut steps = BTreeMap::new();
        steps.insert("sky".to_owned(), step);
        let mut attempts = BTreeMap::new();
        attempts.insert(
            record.attempt_id.clone(),
            AttemptAnnotation {
                room_recording_sha256: Some(sha.clone()),
                listened: true,
                steps,
                notes: None,
            },
        );
        annotations::Annotations {
            schema: 1,
            run_id: record.run_id.clone(),
            annotator: "test".into(),
            method: "test".into(),
            attempts,
        }
    };
    // Control: the same annotation of a covering recording is scored.
    let mut covered = record(&conversation());
    with_room(&mut covered, &dir.path, &metadata);
    let scored = evaluate::score(&covered, 15_000, Some(&annotations(&covered)));
    assert!(scored.acoustic.iter().any(|a| a.measurement().is_some()));
    assert!(
        scored
            .findings
            .iter()
            .any(|f| f.evidence == evaluate::Evidence::RoomAnnotation)
    );
    // The recorder exited before the session ended: nothing certifies.
    let mut early = covered.clone();
    early.evidence["room_audio"]["coverage_lost"] =
        json!({"runner_us": 5_000_000, "kind": "recorder_exited_early", "reason": "test"});
    let annotations = annotations(&early);
    let refused = evaluate::score(&early, 15_000, Some(&annotations));
    assert!(
        refused.acoustic.iter().all(|a| a.measurement().is_none()),
        "{:?}",
        refused.acoustic
    );
    assert!(
        refused
            .latencies
            .iter()
            .all(|l| l.kind != evaluate::MetricKind::Acoustic)
    );
    assert!(
        !refused
            .findings
            .iter()
            .any(|f| f.evidence == evaluate::Evidence::RoomAnnotation),
        "{:#?}",
        refused.findings
    );
    assert!(annotations.reviewed_step(&early, "sky").is_none());
    // R2: a prompt whose actual first copy passed the latest covered start.
    let mut overran = covered.clone();
    overran.evidence["room_audio"]["coverage_violations"] = json!([{"step": "sky",
        "actual_start_runner_us": 10_050_000, "latest_start_runner_us": 10_020_000,
        "overrun_us": 30_000}]);
    let refused_overrun = evaluate::score(&overran, 15_000, Some(&annotations));
    assert!(
        refused_overrun
            .acoustic
            .iter()
            .all(|a| a.measurement().is_none())
    );
    assert!(annotations.reviewed_step(&overran, "sky").is_none());
    let state = serde_json::to_value(&refused_overrun.room_recording).unwrap();
    assert_eq!(state["status"], "prompt_outside_recording", "{state}");
    assert!(
        refused.acoustic.iter().all(|a| a
            .unscored_reason
            .as_deref()
            .is_some_and(|r| r.contains("recorder ended during the session"))),
        "{:?}",
        refused.acoustic
    );
}

/// Re-scoring a retained run never touches its original evaluation, and the
/// CLI summary separates lifecycle exit from room-evidence validity.
#[test]
fn reevaluating_a_retained_run_preserves_the_original_failed_report() {
    let dir = common::Private::new("rv-cli");
    let run = dir.join("run");
    create_run_directory(&run).unwrap();
    let catalog = StimulusCatalog::load().unwrap();
    let plan = LoadedPlan::parse(SERVE_PLAN, &catalog).unwrap();
    let mut record = record(&conversation());
    with_room(&mut record, &dir.path, &room_4ace());
    let mut ledger = Ledger::create(&run).unwrap();
    ledger
        .append(
            "run_start",
            json!({"run_id": record.run_id, "plan_sha256": plan.sha256}),
            true,
        )
        .unwrap();
    ledger
        .append(
            "attempt_planned",
            json!({"attempt_id": record.attempt_id}),
            true,
        )
        .unwrap();
    ledger
        .append("attempt_finished", json!({"record": record}), true)
        .unwrap();
    ledger.append("run_end", json!({}), true).unwrap();
    // The original evaluator output stays exactly as it was written.
    let original = run.join("evaluations").join("1760000000000");
    fs::create_dir_all(&original).unwrap();
    let original_bytes = br#"{"original_outcome":"failed","note":"retained unchanged"}"#;
    fs::write(original.join("evaluation.json"), original_bytes).unwrap();

    let plan_path = dir.join("serve-plan.json");
    fs::write(&plan_path, SERVE_PLAN).unwrap();
    let output = Command::new(BIN)
        .args([
            "evaluate",
            run.to_str().unwrap(),
            "--plan",
            plan_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let summary: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["outcomes"]["failed"], 1, "{summary}");
    assert_eq!(summary["room_recordings"]["invalid"], 1, "{summary}");
    assert_eq!(summary["acoustic_measurements"], 0, "{summary}");
    assert_eq!(
        fs::read(original.join("evaluation.json")).unwrap(),
        original_bytes
    );
    let evaluations: Vec<_> = fs::read_dir(run.join("evaluations"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(evaluations.len(), 2);
    let report = fs::read_to_string(summary["report"].as_str().unwrap()).unwrap();
    assert!(!report.contains("600 ms"));
    let attempt = fs::read_to_string(
        Path::new(summary["report"].as_str().unwrap())
            .with_file_name("attempts")
            .join("4ace-serve-conversation.md"),
    )
    .unwrap();
    assert!(!attempt.contains("without a matching admission"));
    assert!(!attempt.contains("FalseInterruption"));
}
