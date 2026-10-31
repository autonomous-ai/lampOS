use lamp_memory_bench::{
    evaluation,
    model::{Call, Client, Prices, Provider, Reply, Usage},
    model_worker::Worker,
    protocol::{Cues, Event, EventKind, Person, Presence, Query},
    score::{Answer, Question, Rule},
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
fn question(multi: bool) -> Question {
    let query = Query {
        probe_id: "q001".into(),
        at_unix_ms: 1_700_000_000_000,
        person: Person::Owner,
        person_id: Some("owner-a".into()),
        presence: Presence::OwnerAlone,
        heard: "How should I plan the visit?".into(),
        cues: Cues::default(),
        max_context_bytes: 4096,
    };
    let mut next = query.clone();
    next.at_unix_ms += 86_400_000;
    next.heard = "Anything on your mind?".into();
    let mut repeat = next.clone();
    repeat.at_unix_ms += 60_000;
    repeat.heard = "It went well. What next?".into();
    Question {
        id: "q001".into(),
        category: "ahead".into(),
        after_record: "source-a".into(),
        query,
        followups: if multi { vec![next, repeat] } else { vec![] },
        rule: Rule::Rubric {
            criteria: vec![
                "No early follow-up.".into(),
                "One timely follow-up.".into(),
                "No repeated follow-up.".into(),
            ],
            forbidden: vec!["forbidden detail".into()],
        },
        evidence_ids: vec!["source-a".into()],
        eligible_evidence_ids: vec!["source-a".into()],
        human_audit: true,
    }
}
fn source() -> Event {
    Event {
        record_id: "source-a".into(),
        at_unix_ms: 1_699_999_000_000,
        session_id: "s1".into(),
        kind: EventKind::Utterance,
        person: Person::Owner,
        person_id: Some("owner-a".into()),
        presence: Presence::OwnerAlone,
        heard: "The made-up visit is tomorrow.".into(),
        said: "Okay.".into(),
        stt_confidence: None,
        cues: Cues::default(),
    }
}
fn answer(text: &str) -> Answer {
    Answer {
        answer: text.into(),
        evidence_ids: vec!["source-a".into()],
        abstain: false,
    }
}
struct Fake {
    seen: Arc<Mutex<Vec<Call>>>,
    reply: Reply,
}
impl Provider for Fake {
    fn invoke(&mut self, call: &Call, _deadline: Instant) -> lamp_memory_bench::Result<Reply> {
        self.seen.lock().unwrap().push(call.clone());
        Ok(self.reply.clone())
    }
}
fn worker(value: Value) -> (Worker, Arc<Mutex<Vec<Call>>>) {
    let seen = Arc::new(Mutex::new(vec![]));
    let provider = Fake {
        seen: seen.clone(),
        reply: Reply {
            content: Some(value.to_string()),
            usage: Some(Usage {
                input_tokens: 1,
                output_tokens: 1,
            }),
            ..Reply::default()
        },
    };
    let client = Client::new(
        provider,
        Prices {
            source: "offline".into(),
            date: "2026-10-31".into(),
            input_microusd_per_million: 1_000_000,
            output_microusd_per_million: 1_000_000,
        },
        1_000_000,
    )
    .unwrap();
    (Worker::start(client).unwrap(), seen)
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(2)
}
fn judgment() -> Value {
    json!({"awarded":[true,true,true],"explanation":"Synthetic test judgment only.","evidence_ids":["source-a"]})
}
#[test]
fn frozen_policy_matches_prompts_and_fixed_text_probe_ruling() {
    let registered: Value = serde_json::from_str(include_str!(
        "../../../fixtures/memory-bench/live-policy/registration.json"
    ))
    .unwrap();
    assert_eq!(registered, evaluation::policy());
    assert!(
        registered["probe_input"]
            .as_str()
            .unwrap()
            .contains("fixed registered text")
    );
    assert_eq!(registered["output_tokens"], 512);
}
#[test]
fn judge_packet_is_blind_and_contains_all_three_timed_turns() {
    let q = question(true);
    let answers = vec![
        answer("Not yet."),
        answer("How did the visit go?"),
        answer("Start the next task."),
    ];
    let Call::Chat {
        messages,
        tools,
        tool_choice,
        ..
    } = evaluation::judge_call(&q, &answers, &[source()], "Asia/Ho_Chi_Minh")
        .unwrap()
        .unwrap()
    else {
        panic!("expected chat")
    };
    assert!(tools.is_empty());
    assert_eq!(tool_choice, "none");
    let packet: Value = serde_json::from_str(messages[1]["content"].as_str().unwrap()).unwrap();
    let keys: Vec<_> = packet
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        ["candidate_turns", "rubric", "source_evidence", "timezone"]
    );
    assert_eq!(packet["candidate_turns"].as_array().unwrap().len(), 3);
    assert_eq!(
        packet["candidate_turns"][2]["question"],
        "It went well. What next?"
    );
    assert!(
        packet["candidate_turns"][1]["local_time"]
            .as_str()
            .unwrap()
            .contains("Asia/Ho_Chi_Minh")
    );
    assert_eq!(packet["source_evidence"][0]["heard"], source().heard);
    assert!(packet.get("arm").is_none());
    assert!(packet.get("run_id").is_none());
    assert!(packet.get("provider").is_none());
}
#[test]
fn all_turns_are_judged_together_and_keep_human_audit_pending() {
    let q = question(true);
    let answers = vec![
        answer("Not yet."),
        answer("How did the visit go?"),
        answer("Start the next task."),
    ];
    let (mut worker, seen) = worker(judgment());
    let result =
        evaluation::evaluate(&q, &answers, &[source()], "Asia/Ho_Chi_Minh", &mut worker).unwrap();
    let account = worker.finish(deadline()).unwrap();
    assert_eq!(result.score.fraction, Some(1.0));
    assert!(result.score.needs_audit);
    assert!(result.judge_request_hash.is_some());
    assert_eq!(account.receipts.len(), 1);
    assert_eq!(seen.lock().unwrap().len(), 1);
}
#[test]
fn forbidden_claims_cannot_be_overridden_by_a_generous_judge() {
    let q = question(true);
    let answers = vec![answer("forbidden detail"), answer("Fine."), answer("Fine.")];
    let (mut worker, seen) = worker(judgment());
    let result =
        evaluation::evaluate(&q, &answers, &[source()], "Asia/Ho_Chi_Minh", &mut worker).unwrap();
    assert_eq!(result.score.fraction, Some(0.0));
    assert!(result.judge.is_none());
    assert!(worker.finish(deadline()).unwrap().receipts.is_empty());
    assert!(seen.lock().unwrap().is_empty());
}
#[test]
fn earlier_probe_sources_are_allowed_but_future_or_current_sources_fail() {
    let q = question(true);
    let mut answers = vec![
        answer("Not yet."),
        answer("How did it go?"),
        answer("Next task."),
    ];
    answers[1].evidence_ids = vec!["probe-q001-0".into()];
    answers[2].evidence_ids = vec!["probe-q001-1".into()];
    assert!(
        evaluation::judge_call(&q, &answers, &[source()], "UTC")
            .unwrap()
            .is_some()
    );
    for id in ["probe-q001-0", "probe-q001-1", "probe-q002-0"] {
        answers[0].evidence_ids = vec![id.into()];
        let (mut worker, _) = worker(judgment());
        let result = evaluation::evaluate(&q, &answers, &[source()], "UTC", &mut worker).unwrap();
        assert_eq!(result.score.fraction, Some(0.0));
        assert!(worker.finish(deadline()).unwrap().receipts.is_empty());
    }
    assert_eq!(q.eligible_evidence_ids, ["source-a"]);
}
#[test]
fn missing_turns_and_future_gold_sources_fail_before_dispatch() {
    let q = question(true);
    assert!(evaluation::judge_call(&q, &[answer("only one")], &[source()], "UTC").is_err());
    let mut future = source();
    future.at_unix_ms = q.query.at_unix_ms + 1;
    assert!(
        evaluation::judge_call(
            &q,
            &[answer("a"), answer("b"), answer("c")],
            &[future],
            "UTC"
        )
        .is_err()
    );
}
#[test]
fn malformed_or_wrong_rubric_judgments_are_incomplete_and_still_charged() {
    for bad in [
        json!({"awarded":[true],"explanation":"short","evidence_ids":["source-a"]}),
        json!({"awarded":[true,true,true],"explanation":"unsupported","evidence_ids":["future-source"]}),
        json!({"awarded":[true,true,true],"explanation":"extra","evidence_ids":[],"arm":"leak"}),
    ] {
        let (mut worker, _) = worker(bad);
        assert!(
            evaluation::evaluate(
                &question(false),
                &[answer("test")],
                &[source()],
                "UTC",
                &mut worker
            )
            .is_err()
        );
        let account = worker.finish(deadline()).unwrap();
        assert_eq!(account.receipts.len(), 1);
        assert_eq!(account.charged_microusd, 2);
    }
}
#[test]
fn deterministic_exact_scoring_never_calls_the_judge() {
    let mut q = question(false);
    q.rule = Rule::Exact {
        answers: vec!["blue".into()],
        forbidden: vec!["red".into()],
    };
    for (text, score) in [("blue", 1.0), ("blue or red", 0.0)] {
        let (mut worker, _) = worker(judgment());
        let result =
            evaluation::evaluate(&q, &[answer(text)], &[source()], "UTC", &mut worker).unwrap();
        assert_eq!(result.score.fraction, Some(score));
        assert!(worker.finish(deadline()).unwrap().receipts.is_empty());
    }
}

#[test]
fn judge_citations_map_to_script_and_completed_probe_exchanges() {
    let mut q = question(true);
    q.eligible_evidence_ids.push("source-b".into());
    let mut extra = source();
    extra.record_id = "source-b".into();
    extra.heard = "The visit needs no preparation.".into();
    let sources = vec![source(), extra];
    let mut answers = vec![
        answer("Not yet."),
        answer("How did it go?"),
        answer("Next task."),
    ];
    answers[0].evidence_ids = vec!["source-b".into()];
    answers[1].evidence_ids = vec!["probe-q001-0".into()];
    let Call::Chat { messages, .. } = evaluation::judge_call(&q, &answers, &sources, "UTC")
        .unwrap()
        .unwrap()
    else {
        panic!("expected chat")
    };
    let packet: Value = serde_json::from_str(messages[1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(packet["source_evidence"][1]["source_id"], "source-b");
    assert_eq!(packet["source_evidence"][1]["origin"], "scripted_history");
    assert_eq!(packet["candidate_turns"][0]["source_id"], "probe-q001-0");
    assert_eq!(
        packet["candidate_turns"][0]["origin"],
        "candidate_probe_exchange"
    );
    let mut verdict = judgment();
    verdict["evidence_ids"] = json!(["source-b", "probe-q001-0"]);
    let (mut worker, _) = worker(verdict);
    let result = evaluation::evaluate(&q, &answers, &sources, "UTC", &mut worker).unwrap();
    assert_eq!(result.score.fraction, Some(1.0));
    assert!(result.score.needs_audit);
    assert_eq!(worker.finish(deadline()).unwrap().receipts.len(), 1);
    assert_eq!(q.evidence_ids, ["source-a"]);
}

#[test]
fn probe_source_namespace_cannot_alias_scripted_history() {
    let mut q = question(true);
    q.eligible_evidence_ids.push("probe-q001-0".into());
    assert!(
        evaluation::judge_call(
            &q,
            &[answer("a"), answer("b"), answer("c")],
            &[source()],
            "UTC"
        )
        .is_err()
    );
}
#[test]
fn omitted_forget_exchange_is_not_a_later_candidate_source() {
    let q = question(true);
    let mut answers = vec![
        answer("Not yet."),
        answer("How did it go?"),
        answer("Next task."),
    ];
    answers[1].evidence_ids = vec!["probe-q001-0".into()];
    let (mut worker, seen) = worker(judgment());
    let result = evaluation::evaluate_with_history(
        &q,
        &answers,
        &[false, true, true],
        &[source()],
        "UTC",
        &mut worker,
    )
    .unwrap();
    assert_eq!(result.score.fraction, Some(0.0));
    assert!(result.score.needs_audit);
    assert!(seen.lock().unwrap().is_empty());
    assert!(worker.finish(deadline()).unwrap().receipts.is_empty());
    assert!(
        evaluation::judge_call_with_history(&q, &answers, &[true, true, true], &[source()], "UTC")
            .unwrap()
            .is_some()
    );
    assert!(
        evaluation::judge_call_with_history(&q, &answers, &[true], &[source()], "UTC").is_err()
    );
}
#[test]
fn maximum_probe_names_keep_bounded_sources_and_temporal_citations() {
    let mut q = question(true);
    q.id = "q".repeat(128);
    q.query.probe_id = q.id.clone();
    for followup in &mut q.followups {
        followup.probe_id = q.id.clone();
    }
    let first = evaluation::probe_source_id(&q.id, 0);
    let next = evaluation::probe_source_id(&q.id, 1);
    assert!(lamp_memory_bench::protocol::valid_id(&first));
    assert_ne!(first, next);
    assert_eq!(evaluation::probe_source_id("q001", 0), "probe-q001-0");
    let mut e = source();
    e.record_id = first.clone();
    e.validate().unwrap();
    let mut answers = vec![
        answer("Not yet."),
        answer("How did it go?"),
        answer("Next task."),
    ];
    answers[1].evidence_ids = vec![first.clone()];
    assert!(
        evaluation::judge_call_with_history(&q, &answers, &[true, true, true], &[source()], "UTC")
            .unwrap()
            .is_some()
    );
    for id in [first, next] {
        answers[0].evidence_ids = vec![id];
        assert!(
            evaluation::judge_call_with_history(
                &q,
                &answers,
                &[true, true, true],
                &[source()],
                "UTC"
            )
            .unwrap()
            .is_none()
        );
    }
}
