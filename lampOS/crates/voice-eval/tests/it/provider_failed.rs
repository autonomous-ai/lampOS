//! MAIN-009: a terminal `provider_failed` outcome is a readable, failed answer
//! before audio or after a partial answer. It never counts as success, is never
//! confused with a silent missing terminal, keeps played-audio evidence, and
//! never triggers a new stimulus. The drain path's `input_discarded_endpoint`
//! is not a normal endpoint. (Provisional field names until the coordinator
//! publishes its frozen cue schema.)
use crate::common;
use lamp_voice_eval::{
    evaluate::{self, AnswerOutcome, FindingKind},
    events::{EventKind, PROVIDER_FAILED, parse_cue, parse_trace},
    import::{ImportOptions, import_trace},
    plan::LoadedPlan,
    record::StimulusSource,
    report,
    stimulus::StimulusCatalog,
};
use serde_json::{Value, json};
use std::fs;

fn score(trace_body: Vec<Value>) -> evaluate::AttemptScore {
    let catalog = StimulusCatalog::load().unwrap();
    let plan = LoadedPlan::default_plan(&catalog).unwrap();
    let dir = common::Private::new("pf");
    let path = dir.join("events.jsonl");
    let mut trace = vec![
        json!({"kind":"run_start","at_us":0,"provider_kind":"gemini"}),
        json!({"kind":"input_admitted","turn":1,"owner":{"generation":2},"at_us":1000000}),
    ];
    trace.extend(trace_body);
    trace.push(json!({"kind":"run_end","at_us":12000000,"status":"completed_unscored"}));
    fs::write(
        &path,
        trace
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    let run = dir.join("run");
    lamp_voice_eval::ledger::create_run_directory(&run).unwrap();
    let record = import_trace(
        &plan,
        &catalog,
        &ImportOptions {
            run_id: "pf".into(),
            events_path: &path,
            scenario: "quick-chat".into(),
            turn_map: vec![("ask".into(), Some(1))],
            room_metadata: None,
            room_receipt: None,
            room_independent: false,
            source: StimulusSource::Unknown,
        },
        &run,
    )
    .unwrap();
    evaluate::score(&record, 15000, None)
}

fn failed(at: u64, gaps: u64) -> Value {
    json!({"kind":"turn_finished","turn":1,"generation":2,"at_us":at,"outcome":PROVIDER_FAILED,"playback_gaps":gaps})
}

fn kinds(score: &evaluate::AttemptScore) -> Vec<FindingKind> {
    score.findings.iter().map(|f| f.kind).collect()
}

#[test]
fn a_failure_before_audio_is_a_failed_answer_not_a_silent_missing_terminal() {
    let result = score(vec![
        json!({"kind":"local_endpoint","turn":1,"at_us":2000000}),
        failed(5000000, 0),
    ]);
    assert_eq!(
        result.steps[0].answer,
        Some(AnswerOutcome::ProviderFailed {
            partial_audio: false
        }),
        "{:?}",
        result.findings
    );
    let found = kinds(&result);
    assert!(found.contains(&FindingKind::ProviderFailed));
    assert!(!found.contains(&FindingKind::MissingAnswer), "{found:?}");
    assert!(!found.contains(&FindingKind::UnterminatedTurn), "{found:?}");
    assert_ne!(result.outcome, evaluate::Outcome::Passed);
}

#[test]
fn a_failure_after_a_partial_answer_keeps_its_audio_evidence_and_fails() {
    let result = score(vec![
        json!({"kind":"local_endpoint","turn":1,"at_us":2000000}),
        json!({"kind":"speaker_first_write","turn":1,"playback_sequence":1,"at_us":3000000}),
        json!({"kind":"speech_final_sample_retired","turn":1,"playback_sequence":1,"at_us":4000000}),
        failed(4100000, 0),
    ]);
    assert_eq!(
        result.steps[0].answer,
        Some(AnswerOutcome::ProviderFailed {
            partial_audio: true
        }),
        "{:?}",
        result.findings
    );
    // The heard second of audio is evidence, not erased by the failure.
    assert_eq!(result.playback_ms, 1000.0);
    let found = kinds(&result);
    assert!(found.contains(&FindingKind::ProviderFailed));
    assert!(!found.contains(&FindingKind::TruncatedAnswer), "{found:?}");
    assert!(!found.contains(&FindingKind::EvidenceMissing), "{found:?}");
    // Never in the success numerator; always in the denominator.
    let summary = report::summarize(&[&result]);
    assert_eq!(summary.complete_answers.count, 0);
    assert_eq!(summary.complete_answers.of, 1);
    assert_eq!(summary.provider_failed_answers, 1);
}

#[test]
fn a_failure_while_audio_is_still_playing_is_inconsistent_evidence() {
    let result = score(vec![
        json!({"kind":"local_endpoint","turn":1,"at_us":2000000}),
        json!({"kind":"speaker_first_write","turn":1,"playback_sequence":1,"at_us":3000000}),
        failed(3500000, 0),
    ]);
    assert_ne!(
        result.steps[0].answer,
        Some(AnswerOutcome::ProviderFailed {
            partial_audio: true
        }),
        "a terminal before playback retired must not read as a clean failure"
    );
    assert_ne!(result.outcome, evaluate::Outcome::Passed);
}

#[test]
fn the_drain_endpoint_is_recorded_but_never_a_normal_endpoint() {
    let trace = [
        json!({"kind":"input_discarded_endpoint","turn":1,"at_us":2000000,"last_block_host_read_us":1990000}),
    ]
    .iter()
    .map(Value::to_string)
    .collect::<Vec<_>>()
    .join("\n");
    let (events, _) = parse_trace(&trace).unwrap();
    assert!(matches!(
        events[0].kind,
        EventKind::InputDiscardedEndpoint {
            last_block_read_us: Some(1990000),
            silence_wait_ms: None,
        }
    ));
    // A failed request that ended on the drain path scores as a provider
    // failure; no answer latency starts from the discarded endpoint.
    let result = score(vec![
        json!({"kind":"input_discarded_endpoint","turn":1,"at_us":2000000}),
        failed(2500000, 0),
    ]);
    assert_eq!(
        result.steps[0].answer,
        Some(AnswerOutcome::ProviderFailed {
            partial_audio: false
        }),
        "{:?}",
        result.findings
    );
    assert!(
        result
            .latencies
            .iter()
            .all(|l| l.metric != evaluate::Metric::EndpointToFirstWrite),
        "{:?}",
        result.latencies
    );
}

const BOOT: [u8; 16] = [7; 16];

fn cue(kind: &str, sequence: u64, extra: Value) -> Vec<u8> {
    let mut value = json!({"schema":1,"sequence":sequence,"kind":kind,"boot":BOOT,
        "turn":1,"generation":2,"capture_epoch":1,"reference_epoch_context":1,
        "event_us":1000,"sent_us":1001,"expires_us":101000});
    for (key, field) in extra.as_object().unwrap() {
        value[key] = field.clone();
    }
    serde_json::to_vec(&value).unwrap()
}

#[test]
fn the_failed_terminal_cue_is_readable_with_or_without_gaps() {
    for gaps in [0, 2] {
        let parsed = parse_cue(&cue(
            "turn_completed",
            3,
            json!({"outcome":PROVIDER_FAILED,"playback_gaps":gaps}),
        ))
        .unwrap();
        assert_eq!(parsed.outcome.as_deref(), Some(PROVIDER_FAILED));
    }
    // Unknown outcomes stay invalid.
    assert!(
        parse_cue(&cue(
            "turn_completed",
            3,
            json!({"outcome":"provider_vanished","playback_gaps":0})
        ))
        .is_err()
    );
}

#[test]
fn a_late_failed_answer_keeps_both_causes_and_the_provider_evidence() {
    let outcome =
        json!({"stage":"generation","reason":"provider_unavailable","local_not_submitted":false});
    let result = score(vec![
        json!({"kind":"local_endpoint","turn":1,"at_us":2000000}),
        // First write 16 s after the endpoint: past the 15 s answer deadline.
        json!({"kind":"speaker_first_write","turn":1,"playback_sequence":1,"at_us":18000000}),
        json!({"kind":"speech_final_sample_retired","turn":1,"playback_sequence":1,"at_us":18500000}),
        json!({"kind":"turn_finished","turn":1,"generation":2,"at_us":18600000,"outcome":PROVIDER_FAILED,
            "playback_gaps":0,"provider_session":7,"provider_outcome":outcome}),
    ]);
    assert_eq!(
        result.steps[0].answer,
        Some(AnswerOutcome::ProviderFailed {
            partial_audio: true
        }),
        "{:?}",
        result.findings
    );
    let found = kinds(&result);
    assert!(found.contains(&FindingKind::ProviderFailed));
    assert!(
        found.contains(&FindingKind::LateAnswer),
        "lateness is kept as its own cause"
    );
    let failure = result
        .findings
        .iter()
        .find(|f| f.kind == FindingKind::ProviderFailed)
        .unwrap();
    assert!(
        failure.detail.contains("provider_unavailable")
            && failure.detail.contains("\"provider_session\":7"),
        "{}",
        failure.detail
    );
    let summary = report::summarize(&[&result]);
    assert_eq!(summary.provider_failed_answers, 1);
}

#[test]
fn provider_session_and_outcome_survive_trace_parsing_bounded() {
    let record = json!({"kind":"turn_finished","turn":1,"generation":2,"at_us":5,"outcome":PROVIDER_FAILED,
        "playback_gaps":0,"provider_session":3,
        "provider_outcome":{"stage":"submission","reason":"disconnected","local_not_submitted":true},
        "unrelated":"dropped"});
    let event = lamp_voice_eval::events::from_trace(&record).unwrap();
    let detail = event.provider_detail.unwrap();
    assert_eq!(detail["provider_session"], 3);
    assert_eq!(detail["provider_outcome"]["local_not_submitted"], true);
    assert!(detail.get("unrelated").is_none());
    // Oversized provider evidence is replaced by its size, not truncated text.
    let huge = json!({"kind":"input_discarded_endpoint","turn":1,"at_us":5,
        "provider_outcome":{"reason":"x".repeat(5000)}});
    let event = lamp_voice_eval::events::from_trace(&huge).unwrap();
    assert_eq!(event.provider_detail.unwrap()["truncated"], true);
}
