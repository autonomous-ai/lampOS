use lamp_memory_bench::{
    Result, model::*, model_worker::Worker, protocol::*, runner::Runner, session::Session,
    transport::Process,
};
use serde_json::{Value, json};
use std::{
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lamp-memory-runner-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
struct Counting {
    calls: Arc<AtomicUsize>,
    delay: Duration,
}
impl Provider for Counting {
    fn invoke(&mut self, _: &Call, deadline: Instant) -> Result<Reply> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(
            self.delay
                .min(deadline.saturating_duration_since(Instant::now())),
        );
        Ok(Reply {
            content: Some("{}".into()),
            usage: Some(Usage {
                input_tokens: 3,
                output_tokens: 1,
            }),
            ..Reply::default()
        })
    }
}
fn client(delay: Duration) -> (Worker, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    (
        Worker::start(
            Client::new(
                Counting {
                    calls: calls.clone(),
                    delay,
                },
                Prices {
                    source: "synthetic-test".into(),
                    date: "2030-01-01".into(),
                    input_microusd_per_million: 1_000_000,
                    output_microusd_per_million: 1_000_000,
                },
                10_000,
            )
            .unwrap(),
        )
        .unwrap(),
        calls,
    )
}
fn init(path: &Path) -> Request {
    Request::new(
        "r1",
        Op::Init,
        json!({"run_id":"test","arm":"none","state_dir":path,"owner_id":"owner-a","timezone":"Asia/Ho_Chi_Minh","at_unix_ms":10,"config":{}}),
    )
}
fn begin() -> Request {
    Request::new(
        "r2",
        Op::ProbeBegin,
        json!({"probe_id":"q1","at_unix_ms":20}),
    )
}
fn recall() -> Request {
    Request::new(
        "r3",
        Op::Recall,
        json!({"query":{"probe_id":"q1","at_unix_ms":30,"person":"owner","person_id":"owner-a","presence":"owner_alone","heard":"test?","cues":{"activity":null,"objects":[],"topic":null},"max_context_bytes":1000}}),
    )
}
fn end() -> Request {
    Request::new("r4", Op::ProbeEnd, json!({"probe_id":"q1"}))
}
fn output(value: Value) -> String {
    format!(
        "printf '%s\\n' '{}';\n",
        value.to_string().replace('\'', "'\\''")
    )
}
fn response(id: &str, data: Value) -> String {
    format!(
        "IFS= read -r line;\n{}",
        output(serde_json::to_value(Response::success(id, data)).unwrap())
    )
}
fn runner(script: &str, dir: &Directory) -> Runner {
    Runner::spawn(Path::new("/bin/sh"), &["-c".into(), script.into()], &dir.0).unwrap()
}

#[test]
fn clock_restore_does_not_restore_request_identity() {
    let dir = Directory::new();
    let s = Session::default().prepare(&init(&dir.0)).unwrap();
    let s = s.prepare(&begin()).unwrap();
    assert!(
        s.prepare(&Request::new(
            "nested",
            Op::ProbeBegin,
            json!({"probe_id":"q2","at_unix_ms":21})
        ))
        .is_err()
    );
    let s = s.prepare(&recall()).unwrap();
    let s = s.prepare(&end()).unwrap();
    assert!(s.prepare(&begin()).is_err());
    // Main time is 10 again, even though the discarded probe reached 30.
    let s = s
        .prepare(&Request::new(
            "r5",
            Op::Sleep,
            json!({"at_unix_ms":11,"cadence":"nightly"}),
        ))
        .unwrap();
    assert!(
        s.prepare(&Request::new(
            "r6",
            Op::Sleep,
            json!({"at_unix_ms":9,"cadence":"nightly"})
        ))
        .is_err()
    );
}

#[test]
fn writes_and_partial_frames_have_real_deadlines() {
    let dir = Directory::new();
    let mut p = Process::spawn(
        Path::new("/bin/sh"),
        &["-c".into(), "exec /bin/sleep 2".into()],
        &dir.0,
    )
    .unwrap();
    let start = Instant::now();
    assert!(
        p.send(
            &json!({"large":"x".repeat(1_000_000)}),
            start + Duration::from_millis(80)
        )
        .is_err()
    );
    assert!(start.elapsed() < Duration::from_secs(1));
    drop(p);
    let mut p = Process::spawn(
        Path::new("/bin/sh"),
        &["-c".into(), "printf '{\"v\":'; exec /bin/sleep 2".into()],
        &dir.0,
    )
    .unwrap();
    assert!(
        p.receive(Instant::now() + Duration::from_millis(80))
            .unwrap()
            .is_none()
    );
}

#[test]
fn late_recall_is_empty_and_cannot_become_the_next_response() {
    let dir = Directory::new();
    let late = serde_json::to_value(Recall {
        context: "late secret".into(),
        ..Recall::default()
    })
    .unwrap();
    let script = response("r1", init_reply())
        + &response("r2", json!({"isolated":true}))
        + "IFS= read -r line; /bin/sleep 0.35;\n"
        + &output(serde_json::to_value(Response::success("r3", late)).unwrap())
        + &response("r4", json!({"restored":true}))
        + &response("r5", json!({}));
    let mut runner = runner(&script, &dir);
    let (mut client, _) = client(Duration::ZERO);
    runner.execute(init(&dir.0), &mut client).unwrap();
    runner.execute(begin(), &mut client).unwrap();
    let start = Instant::now();
    let result = runner.execute(recall(), &mut client).unwrap();
    assert!(result.missed_deadline);
    assert_eq!(result.data["context"], "");
    assert!(start.elapsed() < Duration::from_millis(320));
    runner.execute(end(), &mut client).unwrap();
    runner
        .execute(Request::new("r5", Op::Close, json!({})), &mut client)
        .unwrap();
}

fn model_frame() -> Value {
    json!({"v":VERSION,"id":"r3","model_request":{"call_id":"m1","kind":"chat","purpose":"retrieve","messages":[{"role":"user","content":"test"}],"max_output_tokens":8,"response_format":null,"tools":[],"tool_choice":"none"}})
}
#[test]
fn model_timeout_preserves_cost_and_lets_the_turn_continue_empty() {
    let dir = Directory::new();
    let script = response("r1", init_reply())
        + &response("r2", json!({"isolated":true}))
        + "IFS= read -r line;\n"
        + &output(model_frame())
        + &response("r3", serde_json::to_value(Recall::default()).unwrap())
        + &response("r4", json!({"restored":true}))
        + &response("r5", json!({}));
    let mut runner = runner(&script, &dir);
    let (mut client, calls) = client(Duration::from_secs(1));
    runner.execute(init(&dir.0), &mut client).unwrap();
    runner.execute(begin(), &mut client).unwrap();
    assert!(
        runner
            .execute(recall(), &mut client)
            .unwrap()
            .missed_deadline
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        client
            .settle(Instant::now() + Duration::from_secs(1))
            .unwrap()
            .charged_microusd,
        4
    );
    runner.execute(end(), &mut client).unwrap();
    assert_eq!(
        client
            .settle(Instant::now() + Duration::from_secs(1))
            .unwrap()
            .charged_microusd,
        4
    );
    runner
        .execute(Request::new("r5", Op::Close, json!({})), &mut client)
        .unwrap();
}

#[test]
fn a_duplicate_terminal_response_poisoned_the_run() {
    let dir = Directory::new();
    let script = response("r1", init_reply())
        + &output(serde_json::to_value(Response::success("r1", json!({}))).unwrap())
        + &response("r2", json!({"isolated":true}));
    let mut runner = runner(&script, &dir);
    let (mut client, _) = client(Duration::ZERO);
    runner.execute(init(&dir.0), &mut client).unwrap();
    assert!(runner.execute(begin(), &mut client).is_err());
    assert!(runner.execute(begin(), &mut client).is_err());
}

#[test]
fn product_tool_only_messages_are_accepted_but_nonassistant_omissions_are_not() {
    let mut call = Call::Chat {
        purpose: Purpose::Extract,
        messages: vec![
            json!({"role":"assistant","tool_calls":[{"id":"t1","type":"function","function":{"name":"remember","arguments":"{}"}}]}),
        ],
        max_output_tokens: 10,
        response_format: None,
        tools: vec![],
        tool_choice: "none".into(),
    };
    call.validate().unwrap();
    if let Call::Chat { messages, .. } = &mut call {
        messages[0]["role"] = json!("user");
    }
    assert!(call.validate().is_err());
}

fn init_reply() -> Value {
    json!({"arm":"none","implementation_revision":"a".repeat(40),"capabilities":["ingest","sleep","forget","probe_begin","snapshot","recall","tool","inspect","probe_end","close"]})
}

fn event(id: &str, text: &str, at: u64) -> Event {
    Event {
        record_id: id.into(),
        at_unix_ms: at,
        session_id: "s1".into(),
        kind: EventKind::Utterance,
        person: Person::Owner,
        person_id: Some("owner-a".into()),
        presence: Presence::OwnerAlone,
        heard: text.into(),
        said: String::new(),
        stt_confidence: None,
        cues: Cues::default(),
    }
}
#[test]
fn plain_language_forget_invalidates_the_snapshot_and_refreshes_before_answer() {
    let dir = Directory::new();
    let snapshot = json!({"owner_model":[],"source_ids":[],"memory_instructions":"old memory"});
    let empty_snapshot = serde_json::to_value(Snapshot::default()).unwrap();
    let empty_recall = serde_json::to_value(Recall::default()).unwrap();
    let script = response("r1", init_reply())
        + &response("r2", json!({"isolated":true}))
        + &response("snapshot-3", snapshot)
        + &response("recall-3", empty_recall.clone())
        + &response(
            "forget-line",
            json!({"accepted":true,"source_ids":[],"forget_effect":{"ran":true,"suppress_confirmation":true}}),
        )
        + &response("snapshot-5", empty_snapshot)
        + &response("recall-5", empty_recall)
        + &response("r4", json!({"restored":true}))
        + &response("r5", json!({}));
    let mut runner = runner(&script, &dir);
    let (mut client, _) = client(Duration::ZERO);
    runner.execute(init(&dir.0), &mut client).unwrap();
    runner.execute(begin(), &mut client).unwrap();
    let query: QueryData = recall().decode().unwrap();
    let old = runner.memory(&query.query, &mut client).unwrap();
    assert!(old.text.contains("old memory"));
    runner
        .execute(
            Request::new(
                "forget-line",
                Op::Ingest,
                json!({"event":event("private-1","Please forget that private fact.",30)}),
            ),
            &mut client,
        )
        .unwrap();
    assert!(old.require_current(&runner).is_err());
    let fresh = runner.memory(&query.query, &mut client).unwrap();
    assert!(fresh.text.is_empty());
    fresh.require_current(&runner).unwrap();
    runner.execute(end(), &mut client).unwrap();
    runner
        .execute(Request::new("r5", Op::Close, json!({})), &mut client)
        .unwrap();
}

#[test]
fn recent_history_clears_all_scopes_and_drops_the_forget_confirmation() {
    use lamp_memory_bench::conversation::Conversation;
    let mut history = Conversation::default();
    history
        .record(&event("e1", "private value", 10), ForgetEffect::default())
        .unwrap();
    let mut other = event("e2", "other private value", 11);
    other.person = Person::Known;
    other.person_id = Some("person-b".into());
    history.record(&other, ForgetEffect::default()).unwrap();
    let query: QueryData = recall().decode().unwrap();
    assert_eq!(history.messages(&query.query).unwrap().len(), 1);
    let mut forgotten = event("e3", "Forget private value", 20);
    forgotten.said = "I forgot private value".into();
    history
        .record(
            &forgotten,
            ForgetEffect {
                ran: true,
                suppress_confirmation: true,
            },
        )
        .unwrap();
    assert!(history.messages(&query.query).unwrap().is_empty());
    let mut q = query.query.clone();
    q.person = Person::Known;
    q.person_id = other.person_id;
    assert!(history.messages(&q).unwrap().is_empty());
    history
        .record(
            &event("e4", "new conversation", 25),
            ForgetEffect::default(),
        )
        .unwrap();
    assert_eq!(history.messages(&query.query).unwrap().len(), 1);
}

#[test]
fn v1_records_stay_readable_without_inventing_forgetting_effects() {
    let value = json!({"v":1,"id":"old","ok":true,"data":{"accepted":true,"source_ids":[]}});
    let response: Response = serde_json::from_value(value).unwrap();
    let data = response.checked("old").unwrap();
    assert!(data.get("forget_effect").is_none());
    assert!(
        ForgetEffect {
            ran: false,
            suppress_confirmation: true
        }
        .validate()
        .is_err()
    );
}

#[test]
fn slow_reservation_cannot_dispatch_after_deadline_or_hold_the_caller() {
    let calls = Arc::new(AtomicUsize::new(0));
    let client = Client::new(
        Counting {
            calls: calls.clone(),
            delay: Duration::ZERO,
        },
        Prices {
            source: "synthetic".into(),
            date: "2030-01-01".into(),
            input_microusd_per_million: 1_000_000,
            output_microusd_per_million: 1_000_000,
        },
        10_000,
    )
    .unwrap()
    .with_audit(|stage, _| {
        if stage == "model_reserved" {
            std::thread::sleep(Duration::from_millis(180));
        }
        Ok(())
    });
    let mut worker = Worker::start(client).unwrap();
    let call = Call::Chat {
        purpose: Purpose::Retrieve,
        messages: vec![json!({"role":"user","content":"test"})],
        max_output_tokens: 8,
        response_format: None,
        tools: vec![],
        tool_choice: "none".into(),
    };
    let started = Instant::now();
    assert!(
        worker
            .call(&call, started + Duration::from_millis(30))
            .is_err()
    );
    assert!(started.elapsed() < Duration::from_millis(140));
    let account = worker
        .finish(Instant::now() + Duration::from_secs(1))
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(account.charged_microusd, 0);
    assert_eq!(account.receipts[0].outcome, "expired_before_dispatch");
}
