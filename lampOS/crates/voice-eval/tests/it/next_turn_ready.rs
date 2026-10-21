//! `next_turn_ready`: plan rules and the fake runtime end to end. The runner's
//! scripted cases (late reopen, extra input, terminal link, owner faults) are
//! in `runner::occurrence_tests`. No audio, device or cloud is involved.
use crate::common;

use lamp_voice_eval::{
    events::EventKind,
    fake::FakeBackend,
    plan::{DEFAULT_PLAN, LoadedPlan, TriggerEvent},
    record::StepStatus,
    runner::{SuiteOptions, run_suite},
    stimulus::StimulusCatalog,
};
use serde_json::{Value, json};

/// The default plan with `follow-up-chain`'s follow-ups waiting for the next
/// turn instead of a delay after completion.
fn next_turn_plan() -> Value {
    let mut source: Value = serde_json::from_str(DEFAULT_PLAN).unwrap();
    source["id"] = json!("next-turn-ready");
    for scenario in source["scenarios"].as_array_mut().unwrap() {
        if scenario["id"] == "follow-up-chain" {
            for step in scenario["steps"].as_array_mut().unwrap().iter_mut().skip(1) {
                step["trigger"]["after"] = json!("next_turn_ready");
            }
        }
    }
    source
}

#[test]
fn next_turn_ready_validates_only_after_a_step_that_expects_a_turn() {
    let catalog = StimulusCatalog::load().unwrap();
    LoadedPlan::parse(&next_turn_plan().to_string(), &catalog).unwrap();
    // After a step whose speech must not become a turn there is nothing to
    // finish first, so the wait could never mean "the next turn".
    let mut source = next_turn_plan();
    let why = source["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "follow-up-chain")
        .unwrap()["steps"][1]
        .clone();
    for scenario in source["scenarios"].as_array_mut().unwrap() {
        if scenario["id"] == "listener-acknowledgment" {
            scenario["steps"].as_array_mut().unwrap().push(why.clone());
        }
    }
    let error = LoadedPlan::parse(&source.to_string(), &catalog)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("next_turn_ready must follow"), "{error}");
}

#[test]
fn the_fake_runtime_reports_readiness_and_follow_ups_start_on_it() {
    let catalog = StimulusCatalog::load().unwrap();
    let plan = LoadedPlan::parse(&next_turn_plan().to_string(), &catalog).unwrap();
    let dir = common::Private::new("next-turn");
    let run = dir.join("run");
    lamp_voice_eval::ledger::create_run_directory(&run).unwrap();
    let profile = "v2-directed-current";
    let mut backend = FakeBackend::new(
        profile,
        plan.profile(profile).unwrap().clone(),
        plan.plan.fixed_reply.clone(),
        None,
    );
    let options = SuiteOptions {
        run_id: "t".into(),
        scenarios: vec!["follow-up-chain".into()],
        repetitions: 1,
        seed: 7,
        shuffle: false,
        attempt_seed: None,
        self_test: Value::Null,
    };
    let records = run_suite(&plan, &catalog, &mut backend, &options, &run).unwrap();
    let record = &records[0];
    let finished: Vec<(u64, u64)> = record
        .events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::TurnCompleted { .. }))
        .map(|e| (e.turn.unwrap(), e.at_us.unwrap()))
        .collect();
    assert_eq!(finished.len(), 3, "{finished:?}");
    // `vad-only` reports availability once and never withdraws it, so the
    // next turn is ready at each completion; the delay counts from there.
    for (index, step) in record.steps.iter().enumerate().skip(1) {
        assert_eq!(step.status, StepStatus::Injected, "{step:?}");
        let trigger = step.trigger.as_ref().unwrap();
        assert_eq!(trigger.event, TriggerEvent::NextTurnReady);
        let (turn, at) = finished[index - 1];
        assert_eq!(trigger.turn, Some(turn));
        assert_eq!(trigger.event_at_us, Some(at));
        let delay = plan.scenario("follow-up-chain").unwrap().steps[index]
            .trigger
            .delay_ms;
        assert_eq!(step.started_us, Some(at + u64::from(delay) * 1000));
    }
}
