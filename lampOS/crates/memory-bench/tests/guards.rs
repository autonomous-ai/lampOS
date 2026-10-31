use lamp_memory_bench::{model::*, protocol::*, score::*};
use serde_json::json;
use std::{
    io::Cursor,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

fn question(rule: Rule) -> Question {
    Question {
        id: "q1".into(),
        category: "recall".into(),
        after_record: "d1-1".into(),
        query: Query {
            probe_id: "q1".into(),
            at_unix_ms: 1,
            person: Person::Owner,
            person_id: Some("owner-a".into()),
            presence: Presence::OwnerAlone,
            heard: "Which day?".into(),
            cues: Cues::default(),
            max_context_bytes: 16384,
        },
        rule,
        followups: vec![],
        evidence_ids: vec!["d1-1".into()],
        eligible_evidence_ids: vec!["d1-1".into()],
        human_audit: false,
    }
}
fn answer(text: &str, abstain: bool) -> Answer {
    Answer {
        answer: text.into(),
        evidence_ids: vec![],
        abstain,
    }
}
#[test]
fn a_correct_token_inside_a_contradiction_does_not_pass() {
    let q = question(Rule::Exact {
        answers: vec!["Sunday".into()],
        forbidden: vec!["Saturday".into()],
    });
    assert_eq!(
        q.score(&answer("Sunday.", false)).unwrap().fraction,
        Some(1.0)
    );
    assert_eq!(
        q.score(&answer("Sunday or Saturday", false))
            .unwrap()
            .fraction,
        Some(0.0)
    );
    assert_eq!(
        q.score(&answer("not Sunday", false)).unwrap().fraction,
        Some(0.0)
    );
}
#[test]
fn abstention_cannot_hide_an_invented_answer() {
    let q = question(Rule::Abstain {
        forms: vec!["I don't remember".into()],
        forbidden: vec![],
    });
    assert_eq!(
        q.score(&answer("I don't remember.", true))
            .unwrap()
            .fraction,
        Some(1.0)
    );
    assert_eq!(
        q.score(&answer("I don't remember, but probably Sunday", true))
            .unwrap()
            .fraction,
        Some(0.0)
    );
}
#[test]
fn date_tolerance_crosses_month_and_leap_day_without_using_today() {
    let q = question(Rule::Date {
        expected: "2032-03-01".into(),
        tolerance_days: 1,
    });
    assert_eq!(
        q.score(&answer("2032-02-29", false)).unwrap().fraction,
        Some(1.0)
    );
    assert_eq!(
        q.score(&answer("2032-02-28", false)).unwrap().fraction,
        Some(0.0)
    );
    assert_eq!(
        q.score(&answer("2031-02-29", false)).unwrap().fraction,
        Some(0.0)
    );
    assert_eq!(
        q.score(&answer("2032-03-01 or 2032-03-20", false))
            .unwrap()
            .fraction,
        Some(0.0)
    );
}
#[test]
fn unjudged_rubric_is_not_zero_or_a_pass_and_a_judge_cannot_override_a_leak() {
    let q = question(Rule::Rubric {
        criteria: vec!["Reveal nothing".into()],
        forbidden: vec!["secret".into()],
    });
    assert_eq!(
        q.score(&answer("I cannot share that", false))
            .unwrap()
            .fraction,
        None
    );
    let judge = JudgeOutput {
        awarded: vec![true],
        explanation: "looks good".into(),
        evidence_ids: vec![],
    };
    assert_eq!(
        judge_score(&q, &answer("the secret", false), &judge)
            .unwrap()
            .fraction,
        Some(0.0)
    );
}
#[test]
fn protocol_rejects_truncation_wrong_ids_and_context_hidden_in_other_fields() {
    assert!(read_frame(&mut Cursor::new(b"{\"v\":1}")).is_err());
    assert!(Response::success("old", json!({})).checked("new").is_err());
    let recall = Recall {
        context: "short".into(),
        memory_instructions: "longer than the whole budget".into(),
        ..Recall::default()
    };
    assert!(recall.check_bound(10).is_err());
    let mut reader = Cursor::new(vec![b'x'; MAX_FRAME_BYTES + 20]);
    assert!(read_frame(&mut reader).is_err());
    assert_eq!(reader.position(), (MAX_FRAME_BYTES + 1) as u64);
}
struct Counting {
    calls: Arc<AtomicUsize>,
    fail: bool,
}
impl Provider for Counting {
    fn invoke(&mut self, _: &Call, _: Instant) -> lamp_memory_bench::Result<Reply> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            return Err(lamp_memory_bench::invalid("transport broke after dispatch"));
        }
        Ok(Reply {
            content: Some("ok".into()),
            usage: Some(Usage {
                input_tokens: 1,
                output_tokens: 1,
            }),
            ..Reply::default()
        })
    }
}
fn call() -> Call {
    Call::Chat {
        purpose: Purpose::Answer,
        messages: vec![json!({"role":"user","content":"hello"})],
        max_output_tokens: 10,
        response_format: None,
        tools: vec![],
        tool_choice: "none".into(),
    }
}
fn prices() -> Prices {
    Prices {
        source: "recorded-test".into(),
        date: "2030-01-01".into(),
        input_microusd_per_million: 1_000_000,
        output_microusd_per_million: 1_000_000,
    }
}
#[test]
fn insufficient_budget_never_dispatches_and_failed_requests_keep_the_reservation() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut denied = Client::new(
        Counting {
            calls: calls.clone(),
            fail: false,
        },
        prices(),
        1,
    )
    .unwrap();
    assert!(
        denied
            .call(&call(), Instant::now() + Duration::from_secs(1))
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let mut uncertain = Client::new(
        Counting {
            calls: calls.clone(),
            fail: true,
        },
        prices(),
        10_000,
    )
    .unwrap();
    assert!(
        uncertain
            .call(&call(), Instant::now() + Duration::from_secs(1))
            .is_err()
    );
    assert!(uncertain.charged_microusd() > 0);
    assert_eq!(
        uncertain.receipts()[0].charged_microusd,
        uncertain.receipts()[0].reserved_microusd
    );
}
#[test]
fn completed_usage_reconciles_and_expired_deadline_never_dispatches() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut client = Client::new(
        Counting {
            calls: calls.clone(),
            fail: false,
        },
        prices(),
        10_000,
    )
    .unwrap();
    client
        .call(&call(), Instant::now() + Duration::from_secs(1))
        .unwrap();
    assert_eq!(client.charged_microusd(), 2);
    assert!(client.call(&call(), Instant::now()).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
#[test]
fn a_recording_miss_never_becomes_a_live_call() {
    let mut recorded = Recorded::read(Cursor::new(Vec::<u8>::new())).unwrap();
    assert!(
        recorded
            .invoke(&call(), Instant::now() + Duration::from_secs(1))
            .is_err()
    );
}

struct Overrun {
    calls: Arc<AtomicUsize>,
}
impl Provider for Overrun {
    fn invoke(&mut self, _: &Call, _: Instant) -> lamp_memory_bench::Result<Reply> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(Reply {
            content: Some("answer".into()),
            usage: Some(Usage {
                input_tokens: 10_000,
                output_tokens: 1,
            }),
            ..Reply::default()
        })
    }
}
#[test]
fn known_provider_overrun_is_charged_and_permanently_stops_dispatch() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut client = Client::new(
        Overrun {
            calls: calls.clone(),
        },
        prices(),
        2000,
    )
    .unwrap();
    assert!(
        client
            .call(&call(), Instant::now() + Duration::from_secs(1))
            .is_err()
    );
    assert_eq!(client.charged_microusd(), 10_001);
    assert_eq!(client.receipts()[0].charged_microusd, 10_001);
    assert!(
        client
            .call(&call(), Instant::now() + Duration::from_secs(1))
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
#[test]
fn a_literal_secret_is_still_disclosed_beside_punctuation() {
    let q = question(Rule::Rubric {
        criteria: vec!["No disclosure".into()],
        forbidden: vec!["silver acorn".into()],
    });
    assert_eq!(
        q.score(&answer("I cannot tell you: silver acorn, sorry.", false))
            .unwrap()
            .fraction,
        Some(0.0)
    );
}
#[test]
fn a_bad_followup_cannot_enter_a_registered_question() {
    let mut q = question(Rule::Exact {
        answers: vec!["Sunday".into()],
        forbidden: vec![],
    });
    let mut next = q.query.clone();
    next.at_unix_ms = 0;
    q.followups.push(next);
    assert!(q.validate().is_err());
    q.followups[0].at_unix_ms = 2;
    q.followups[0].max_context_bytes = usize::MAX;
    assert!(q.validate().is_err());
}
#[test]
fn a_durable_reservation_precedes_dispatch_and_failed_logging_stops_the_client() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let stages = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = stages.clone();
    let mut client = Client::new(
        Counting {
            calls: calls.clone(),
            fail: false,
        },
        prices(),
        10_000,
    )
    .unwrap()
    .with_audit(move |stage, receipt| {
        if stage == "model_reserved" {
            assert_eq!(observed.load(Ordering::SeqCst), 0);
            assert!(receipt.reserved_microusd > 0);
        } else {
            assert_eq!(observed.load(Ordering::SeqCst), 1);
            assert_eq!(receipt.usage.as_ref().unwrap().input_tokens, 1);
        }
        recorded.lock().unwrap().push(stage.to_string());
        Ok(())
    });
    client
        .call(&call(), Instant::now() + Duration::from_secs(1))
        .unwrap();
    assert_eq!(
        *stages.lock().unwrap(),
        ["model_reserved", "model_completed"]
    );
    let mut client = Client::new(
        Counting {
            calls: calls.clone(),
            fail: false,
        },
        prices(),
        10_000,
    )
    .unwrap()
    .with_audit(|_, _| Err(lamp_memory_bench::invalid("synthetic disk failure")));
    assert!(
        client
            .call(&call(), Instant::now() + Duration::from_secs(1))
            .is_err()
    );
    assert!(
        client
            .call(&call(), Instant::now() + Duration::from_secs(1))
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn evidence_only_facts_are_visible_with_zero_use_and_never_silent_unknown() {
    use lamp_memory_bench::protocol::Diagnostics;
    let mut v = serde_json::json!({"schema":"tier-usage-v1","tiers":{"evidence_only_facts":{"items":7,"text_bytes":120,"recall_calls":0,"snapshot_calls":0}}});
    let d: Diagnostics = serde_json::from_value(v.clone()).unwrap();
    d.validate().unwrap();
    assert_eq!(
        serde_json::to_value(d).unwrap()["tiers"]["evidence_only_facts"]["items"],
        7
    );
    v["tiers"]["evidence_only_facts"]["recall_calls"] = serde_json::json!(1);
    assert!(
        serde_json::from_value::<Diagnostics>(v)
            .unwrap()
            .validate()
            .is_err()
    );
}
