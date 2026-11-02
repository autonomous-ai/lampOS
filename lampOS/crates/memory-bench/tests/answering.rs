use lamp_memory_bench::{
    Result,
    answering::{Answerer, MemoryMode},
    conversation::Conversation,
    model::{Call, Client, Prices, Provider, Reply, Usage},
    model_worker::Worker,
    protocol::*,
    runner::Runner,
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "bench-answer-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
        Self(p)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn event() -> Event {
    Event {
        record_id: "source-a".into(),
        at_unix_ms: 10,
        session_id: "session-a".into(),
        kind: EventKind::Utterance,
        person: Person::Owner,
        person_id: Some("owner-a".into()),
        presence: Presence::OwnerAlone,
        heard: "old private history".into(),
        said: "Okay.".into(),
        stt_confidence: None,
        cues: Cues::default(),
    }
}
fn query() -> Query {
    Query {
        probe_id: "q1".into(),
        at_unix_ms: 20,
        person: Person::Owner,
        person_id: Some("owner-a".into()),
        presence: Presence::OwnerAlone,
        heard: "What should I do?".into(),
        cues: Cues::default(),
        max_context_bytes: 4096,
    }
}
fn tool_schema(name: &str) -> Value {
    json!({"type":"function","function":{"name":name,"description":"Synthetic test tool","parameters":{"type":"object","properties":{}}}})
}
fn tool_call(id: &str, name: &str) -> Value {
    json!({"id":id,"type":"function","function":{"name":name,"arguments":"{}"},"opaque_provider_field":{"keep":true}})
}
fn reply_tools(tools: Vec<Value>) -> Reply {
    Reply {
        content: None,
        tool_calls: tools,
        reasoning_details: Some(json!([{"opaque":"old private reasoning"}])),
        usage: Some(Usage {
            input_tokens: 1,
            output_tokens: 1,
        }),
        ..Reply::default()
    }
}
fn reply_answer() -> Reply {
    Reply {
        content: Some(
            json!({"answer":"Fresh reply.","evidence_ids":[],"abstain":false}).to_string(),
        ),
        usage: Some(Usage {
            input_tokens: 1,
            output_tokens: 1,
        }),
        ..Reply::default()
    }
}
struct Fake {
    seen: Arc<Mutex<Vec<Call>>>,
    replies: VecDeque<Reply>,
}
impl Provider for Fake {
    fn invoke(&mut self, call: &Call, _deadline: Instant) -> Result<Reply> {
        self.seen.lock().unwrap().push(call.clone());
        Ok(self.replies.pop_front().expect("unexpected model request"))
    }
}
fn worker(replies: Vec<Reply>) -> (Worker, Arc<Mutex<Vec<Call>>>) {
    let seen = Arc::new(Mutex::new(vec![]));
    let client = Client::new(
        Fake {
            seen: seen.clone(),
            replies: replies.into(),
        },
        Prices {
            source: "offline".into(),
            date: "2026-10-31".into(),
            input_microusd_per_million: 1_000_000,
            output_microusd_per_million: 1_000_000,
        },
        10_000_000,
    )
    .unwrap();
    (Worker::start(client).unwrap(), seen)
}
fn respond(data: Value) -> String {
    format!(
        "IFS= read -r line || exit 1\nid=${{line#*\\\"id\\\":\\\"}}\nid=${{id%%\\\"*}}\nprintf '%s\\n' \"$line\" >> requests.jsonl\nprintf '{{\"v\":2,\"id\":\"%s\",\"ok\":true,\"data\":%s}}\\n' \"$id\" '{}'\n",
        data.to_string().replace('\'', "'\\''")
    )
}
fn snapshot(text: &str) -> Value {
    json!({"owner_model":[{"text":text,"source_ids":["source-a"]}],"source_ids":["source-a"],"memory_instructions":""})
}
fn recall(tools: Vec<Value>) -> Value {
    json!({"context":"","source_ids":[],"owner_model":[],"tools":tools,"memory_instructions":""})
}
fn mutation(ran: bool) -> Value {
    json!({"result":{"synthetic":"old private tool result"},"source_ids":[],"forget_effect":{"ran":ran,"suppress_confirmation":ran}})
}
fn setup(dir: &Directory, worker: &mut Worker, tail: String) -> Runner {
    setup_query(dir, worker, tail, &query())
}
fn setup_query(dir: &Directory, worker: &mut Worker, tail: String, query: &Query) -> Runner {
    let script = respond(
        json!({"arm":"fixture","implementation_revision":"a".repeat(40),"capabilities":["ingest","sleep","forget","probe_begin","snapshot","recall","tool","inspect","probe_end","close"]}),
    ) + &respond(
        json!({"accepted":true,"source_ids":["source-a"],"forget_effect":{"ran":false,"suppress_confirmation":false}}),
    ) + &respond(json!({"isolated":true}))
        + &tail;
    let mut runner = Runner::spawn(
        std::path::Path::new("/bin/sh"),
        &["-c".into(), script],
        &dir.0,
    )
    .unwrap();
    runner.execute(Request::new("init",Op::Init,json!({"run_id":"fixture","arm":"fixture","state_dir":dir.0,"owner_id":"owner-a","timezone":"UTC","at_unix_ms":0,"config":{}})),worker).unwrap();
    runner
        .execute(
            Request::new("ingest", Op::Ingest, json!({"event":event()})),
            worker,
        )
        .unwrap();
    runner
        .execute(
            Request::new(
                "begin",
                Op::ProbeBegin,
                json!({"probe_id":query.probe_id,"at_unix_ms":query.at_unix_ms}),
            ),
            worker,
        )
        .unwrap();
    runner
}
fn finish(worker: Worker) -> usize {
    worker
        .finish(Instant::now() + Duration::from_secs(2))
        .unwrap()
        .receipts
        .len()
}
#[test]
fn forget_discards_old_prefix_history_reasoning_and_remaining_tool_batch() {
    let dir = Directory::new();
    let tools = vec![tool_schema("remember"), tool_schema("forget")];
    let tail = respond(snapshot("old private model"))
        + &respond(recall(tools))
        + &respond(mutation(false))
        + &respond(mutation(true))
        + &respond(json!({"owner_model":[],"source_ids":[],"memory_instructions":""}))
        + &respond(recall(vec![]));
    let (mut worker, seen) = worker(vec![
        reply_tools(vec![
            tool_call("one", "remember"),
            tool_call("two", "forget"),
            tool_call("three", "remember"),
        ]),
        reply_answer(),
    ]);
    let mut runner = setup(&dir, &mut worker, tail);
    let mut recent = Conversation::default();
    recent.record(&event(), ForgetEffect::default()).unwrap();
    let turn = Answerer {
        runner: &mut runner,
        worker: &mut worker,
        recent: &mut recent,
        timezone: "UTC",
        mode: MemoryMode::Bounded,
    }
    .answer(&query(), |_, _| Ok(()))
    .unwrap();
    assert_eq!(turn.tools_executed, 2);
    assert_eq!(turn.model_rounds, 2);
    assert!(turn.forget_effect.ran && turn.forget_effect.suppress_confirmation);
    let calls = seen.lock().unwrap();
    let second = serde_json::to_string(&calls[1]).unwrap();
    assert!(!second.contains("old private"));
    assert!(!second.contains("tool_call_id"));
    assert!(second.contains("What should I do?"));
    assert!(second.contains("forget operation was attempted"));
    drop(calls);
    assert!(recent.messages(&query()).unwrap().is_empty());
    assert_eq!(finish(worker), 2);
}
#[test]
fn ordinary_tool_refreshes_memory_and_preserves_opaque_tool_envelopes() {
    let dir = Directory::new();
    let tail = respond(snapshot("stale owner model"))
        + &respond(recall(vec![tool_schema("remember")]))
        + &respond(mutation(false))
        + &respond(snapshot("fresh owner model"))
        + &respond(recall(vec![]));
    let (mut worker, seen) = worker(vec![
        reply_tools(vec![tool_call("one", "remember")]),
        reply_answer(),
    ]);
    let mut runner = setup(&dir, &mut worker, tail);
    let mut recent = Conversation::default();
    Answerer {
        runner: &mut runner,
        worker: &mut worker,
        recent: &mut recent,
        timezone: "UTC",
        mode: MemoryMode::Bounded,
    }
    .answer(&query(), |_, _| Ok(()))
    .unwrap();
    let calls = seen.lock().unwrap();
    let second = serde_json::to_string(&calls[1]).unwrap();
    assert!(!second.contains("stale owner model"));
    assert!(second.contains("fresh owner model"));
    assert!(second.contains("opaque_provider_field"));
    assert!(second.contains("old private reasoning"));
    assert!(second.contains("tool_call_id"));
    drop(calls);
    assert_eq!(finish(worker), 2);
}
#[test]
fn entire_batch_is_rejected_before_an_unadvertised_or_duplicate_call_runs() {
    for bad in [
        tool_call("two", "unadvertised"),
        tool_call("one", "remember"),
    ] {
        let dir = Directory::new();
        let tail = respond(snapshot("model")) + &respond(recall(vec![tool_schema("remember")]));
        let (mut worker, _) = worker(vec![reply_tools(vec![tool_call("one", "remember"), bad])]);
        let mut runner = setup(&dir, &mut worker, tail);
        let mut recent = Conversation::default();
        let mut operations = 0;
        assert!(
            Answerer {
                runner: &mut runner,
                worker: &mut worker,
                recent: &mut recent,
                timezone: "UTC",
                mode: MemoryMode::Bounded
            }
            .answer(&query(), |kind, _| {
                if kind == "answer_tool" {
                    operations += 1;
                }
                Ok(())
            })
            .is_err()
        );
        assert_eq!(operations, 0);
        assert_eq!(finish(worker), 1);
    }
}
#[test]
fn stale_results_are_invalidated_and_only_latest_result_uses_the_cap() {
    let dir = Directory::new();
    let mut old = mutation(false);
    old["result"] = json!({"text":"STALE_RESULT_CANARY"});
    let mut fresh = mutation(false);
    fresh["result"] = json!({"text":"LATEST_RESULT"});
    let tail = respond(snapshot("model"))
        + &respond(recall(vec![tool_schema("remember")]))
        + &respond(old)
        + &respond(fresh)
        + &respond(snapshot("fresh model"))
        + &respond(recall(vec![]));
    let (mut worker, seen) = worker(vec![
        reply_tools(vec![
            tool_call("one", "remember"),
            tool_call("two", "remember"),
        ]),
        reply_answer(),
    ]);
    let mut runner = setup(&dir, &mut worker, tail);
    let mut recent = Conversation::default();
    Answerer {
        runner: &mut runner,
        worker: &mut worker,
        recent: &mut recent,
        timezone: "UTC",
        mode: MemoryMode::Bounded,
    }
    .answer(&query(), |_, _| Ok(()))
    .unwrap();
    let calls = seen.lock().unwrap();
    let second = serde_json::to_string(&calls[1]).unwrap();
    assert!(!second.contains("STALE_RESULT_CANARY"));
    assert!(second.contains("LATEST_RESULT"));
    assert!(second.contains("invalidated by a subsequent"));
    assert!(second.contains("opaque_provider_field"));
    drop(calls);
    assert_eq!(finish(worker), 2);
}
#[test]
fn excessive_tool_text_or_combined_memory_never_reaches_the_next_model_call() {
    for combined in [false, true] {
        let dir = Directory::new();
        let mut result = mutation(false);
        result["result"] = json!({"text":"x".repeat(if combined{200}else{700})});
        let mut tail = respond(snapshot("model"))
            + &respond(recall(vec![tool_schema("remember")]))
            + &respond(result);
        if combined {
            tail += &respond(snapshot(&"m".repeat(500)));
        }
        let (mut worker, seen) = worker(vec![reply_tools(vec![tool_call("one", "remember")])]);
        let mut runner = setup(&dir, &mut worker, tail);
        let mut recent = Conversation::default();
        let mut q = query();
        q.max_context_bytes = 600;
        assert!(
            Answerer {
                runner: &mut runner,
                worker: &mut worker,
                recent: &mut recent,
                timezone: "UTC",
                mode: MemoryMode::Bounded
            }
            .answer(&q, |_, _| Ok(()))
            .is_err()
        );
        assert_eq!(seen.lock().unwrap().len(), 1);
        assert_eq!(finish(worker), 1);
    }
}
#[test]
fn maximum_length_probe_id_can_dispatch_unique_bounded_tool_requests() {
    let dir = Directory::new();
    let mut q = query();
    q.probe_id = "q".repeat(128);
    let tail = respond(snapshot("model"))
        + &respond(recall(vec![tool_schema("remember")]))
        + &respond(mutation(false))
        + &respond(mutation(false))
        + &respond(snapshot("fresh"))
        + &respond(recall(vec![]));
    let (mut worker, _) = worker(vec![
        reply_tools(vec![
            tool_call("one", "remember"),
            tool_call("two", "remember"),
        ]),
        reply_answer(),
    ]);
    let mut runner = setup_query(&dir, &mut worker, tail, &q);
    let mut recent = Conversation::default();
    let mut ids = vec![];
    let turn = Answerer {
        runner: &mut runner,
        worker: &mut worker,
        recent: &mut recent,
        timezone: "UTC",
        mode: MemoryMode::Bounded,
    }
    .answer(&q, |kind, data| {
        if kind == "answer_tool" {
            ids.push(data["request"]["id"].as_str().unwrap().to_owned());
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(turn.tools_executed, 2);
    assert_ne!(ids[0], ids[1]);
    assert!(ids.iter().all(|s| valid_id(s)));
    assert_eq!(finish(worker), 2);
}
#[test]
fn tool_round_limit_stops_without_dispatching_an_unanswerable_last_batch() {
    let dir = Directory::new();
    let mut tail = String::new();
    let mut replies = vec![];
    for i in 0..8 {
        tail += &respond(snapshot("model"));
        tail += &respond(recall(vec![tool_schema("remember")]));
        replies.push(reply_tools(vec![tool_call(
            &format!("call-{i}"),
            "remember",
        )]));
        if i < 7 {
            tail += &respond(mutation(false));
        }
    }
    let (mut worker, seen) = worker(replies);
    let mut runner = setup(&dir, &mut worker, tail);
    let mut recent = Conversation::default();
    let mut dispatched = 0;
    assert!(
        Answerer {
            runner: &mut runner,
            worker: &mut worker,
            recent: &mut recent,
            timezone: "UTC",
            mode: MemoryMode::Bounded
        }
        .answer(&query(), |kind, _| {
            if kind == "answer_tool" {
                dispatched += 1;
            }
            Ok(())
        })
        .is_err()
    );
    assert_eq!(dispatched, 7);
    assert_eq!(seen.lock().unwrap().len(), 8);
    assert_eq!(finish(worker), 8);
}
#[test]
fn immediate_citations_preserve_scope_without_leaking_another_person() {
    let mut recent = Conversation::default();
    let e = event();
    recent.record(&e, ForgetEffect::default()).unwrap();
    let rows = recent.evidence_messages(&query()).unwrap();
    let metadata: Value = serde_json::from_str(rows[0]["content"].as_str().unwrap()).unwrap();
    assert_eq!(metadata["source_id"], "source-a");
    let mut q = query();
    q.person_id = Some("owner-b".into());
    assert!(recent.evidence_messages(&q).unwrap().is_empty());
    q.person_id = None;
    q.person = Person::Unsure;
    q.presence = Presence::Unknown;
    assert!(recent.evidence_messages(&q).unwrap().is_empty());
}
#[test]
fn extra_adapter_fields_cannot_bypass_memory_text_accounting() {
    let dir = Directory::new();
    let mut result = mutation(false);
    result["result"] = json!({});
    result["extra_note"] = json!("EXTRA_MEMORY_CANARY".repeat(100));
    let tail = respond(snapshot("model"))
        + &respond(recall(vec![tool_schema("remember")]))
        + &respond(result)
        + &respond(snapshot("fresh"))
        + &respond(recall(vec![]));
    let (mut worker, seen) = worker(vec![
        reply_tools(vec![tool_call("one", "remember")]),
        reply_answer(),
    ]);
    let mut runner = setup(&dir, &mut worker, tail);
    let mut recent = Conversation::default();
    let mut q = query();
    q.max_context_bytes = 600;
    Answerer {
        runner: &mut runner,
        worker: &mut worker,
        recent: &mut recent,
        timezone: "UTC",
        mode: MemoryMode::Bounded,
    }
    .answer(&q, |_, _| Ok(()))
    .unwrap();
    let calls = seen.lock().unwrap();
    assert!(
        !serde_json::to_string(&calls[1])
            .unwrap()
            .contains("EXTRA_MEMORY_CANARY")
    );
    drop(calls);
    assert_eq!(finish(worker), 2);
}
