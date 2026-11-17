//! `record_source` against the real Days store (`lamp-memory`) and a fake
//! lamp-tasks that logs every request it is sent.
use super::*;
use lamp_memory::{
    days::Days,
    days_protocol,
    store::{LocalTime, Options},
};
use std::{cell::Cell, fs, path::PathBuf};

const NOW: u64 = 1_791_892_800_000;
const OWNER: &str = "p-0123456789abcdef";
const OTHER: &str = "p-fedcba9876543210";
const STORE: &str = "s1";

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The real memory store, and a fake lamp-tasks holding `tasks` (id,
/// asked, answer) under [`STORE`]; every request either is sent is logged.
struct Fake {
    days: Days,
    _scratch: Scratch,
    store: String,
    tasks: Vec<(u64, &'static str, &'static str)>,
    sent: Vec<Value>,
    waits: Vec<Duration>,
    /// The tasks part answers nothing (a timeout).
    tasks_silent: bool,
    /// Added to the clock by each call, as if it took that long.
    takes: Duration,
    clock: std::rc::Rc<Cell<Duration>>,
}

impl Fake {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "lamp-voice-record-source-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let days = Days::open(
            Options {
                dir: dir.clone(),
                keep_days: 30,
                local: LocalTime::Offset(0),
            },
            NOW,
        )
        .unwrap();
        Self {
            days,
            _scratch: Scratch(dir),
            store: STORE.into(),
            tasks: vec![(7, "Book the dentist", "Booked for Friday 10:00")],
            sent: Vec::new(),
            waits: Vec::new(),
            tasks_silent: false,
            takes: Duration::ZERO,
            clock: std::rc::Rc::default(),
        }
    }

    fn send(&mut self, line: Value, at: u64) -> Value {
        let answer = days_protocol::answer(&mut self.days, line.to_string().as_bytes(), at);
        serde_json::from_str(&answer.line).unwrap()
    }

    fn put(&mut self, person: &str, key: &str, kind: &str, summary: &str, source: Value) {
        let answer = self.send(
            json!({
                "op": "service_record", "person": person, "key": key,
                "service": "harness", "action": "task", "status": "done",
                "asked_unix_ms": NOW, "changed_unix_ms": NOW, "deed_unix_ms": NOW,
                "source_kind": kind, "source_ref": source, "summary": summary,
            }),
            NOW,
        );
        assert_eq!(answer["ok"], true, "{answer}");
    }

    fn task(&mut self, person: &str, key: &str, id: u64, summary: &str) {
        self.put(
            person,
            key,
            "task",
            summary,
            json!({"service":"lamp-tasks","id":format!("{STORE}:{id}")}),
        );
    }

    fn ask(&mut self, request: u64, key: &str) -> Value {
        self.ask_as(OWNER, true, request, key)
    }

    fn ask_as(&mut self, person: &str, in_full: bool, request: u64, key: &str) -> Value {
        let clock = self.clock.clone();
        answer(
            self,
            &Asker {
                person: Some(person.into()),
                in_full,
            },
            request,
            &json!({"key": key}),
            &move || clock.get(),
        )
    }

    fn tick(&mut self, wait: Duration) {
        self.waits.push(wait);
        self.clock.set(self.clock.get() + self.takes);
    }
}

impl Sources for Fake {
    fn memory(&mut self, line: &Value, wait: Duration) -> Option<Value> {
        self.tick(wait);
        self.sent.push(line.clone());
        Some(self.send(line.clone(), NOW + 10_000))
    }

    fn tasks(&mut self, line: &Value, wait: Duration) -> Option<Value> {
        self.tick(wait);
        self.sent.push(line.clone());
        if self.tasks_silent {
            return None;
        }
        let id = line["id"].as_u64()?;
        let Some((_, asked, answer)) = self.tasks.iter().find(|(held, ..)| *held == id) else {
            return Some(
                json!({"ok":false,"error":"not_found","message":"No retained task has that ID."}),
            );
        };
        Some(match line["op"].as_str()? {
            "get_task" => json!({
                "ok": true, "store_id": self.store, "revision": 3,
                "task": {"id": id, "dispatch": {"state": "accepted"}},
                "text": asked,
                "observations": [{"id": 1, "state": "running"}, {"id": 2, "state": "idle"}],
            }),
            "get_observation" => json!({
                "ok": true, "store_id": self.store, "id": id,
                "observation": {"id": line["observation_id"], "state": "idle"},
                "data": {"state": "idle", "answer": answer},
            }),
            _ => json!({"ok":false,"error":"bad_request"}),
        })
    }
}

/// The ops every source was sent: reads only, never a delete or a write.
fn reads_only(sent: &[Value]) {
    for line in sent {
        let op = line["op"].as_str().unwrap();
        assert!(
            ["service_records", "get_task", "get_observation"].contains(&op),
            "{line}"
        );
    }
}

/// A task record's source is fetched: what was asked and the agent's
/// answer, marked as the service's data, within its bound.
#[test]
fn a_task_record_fetches_its_source() {
    let mut fake = Fake::new("fetch");
    fake.task(OWNER, "t-s1-7", 7, "Book the dentist");
    let told = fake.ask(1, "t-s1-7");
    assert_eq!(told["found"], true, "{told}");
    assert_eq!(told["kind"], "task");
    assert_eq!(told["from"], "lamp-tasks");
    assert_eq!(told["summary"], "Book the dentist");
    let text = told["detail"]["text"].as_str().unwrap();
    assert!(text.contains("Booked for Friday 10:00"), "{text}");
    assert!(text.len() <= DETAIL_BYTES);
    assert_eq!(told["detail"]["note"], crate::services::DATA_NOTE);
    // The newest observation is read.
    assert_eq!(fake.sent[2]["observation_id"], 2);
    reads_only(&fake.sent);
}

/// The detail is cut to its 1,500 bytes.
#[test]
fn the_detail_is_bounded() {
    let mut fake = Fake::new("bounded");
    let long: &'static str = Box::leak("é".repeat(2_000).into_boxed_str());
    fake.tasks = vec![(7, "Write it", long)];
    fake.task(OWNER, "t-s1-7", 7, "Write it");
    let told = fake.ask(1, "t-s1-7");
    assert!(told["detail"]["text"].as_str().unwrap().len() <= DETAIL_BYTES);
}

/// A 4 KB answer is cut alone: what was asked, the state and the outcome
/// are told whole, the cut is marked, and the text is still whole JSON
/// (the first builder's P2 2 on #7571).
#[test]
fn a_long_answer_is_cut_alone_and_keeps_its_outcome() {
    let mut fake = Fake::new("long-answer");
    let long: &'static str = Box::leak("All done. ".repeat(410).into_boxed_str());
    assert!(long.len() > 4_000);
    fake.tasks = vec![(7, "Write it", long)];
    fake.task(OWNER, "t-s1-7", 7, "Write it");
    let told = fake.ask(1, "t-s1-7");
    let text = told["detail"]["text"].as_str().unwrap();
    assert!(text.len() <= DETAIL_BYTES, "{}", text.len());
    let detail: Value = serde_json::from_str(text).unwrap();
    assert_eq!(detail["outcome"], "idle", "{detail}");
    assert_eq!(detail["state"], "accepted");
    assert_eq!(detail["asked"], "Write it");
    assert_eq!(detail["cut"], json!(["answer"]));
    let answer = detail["answer"].as_str().unwrap();
    assert!(!answer.is_empty() && long.starts_with(answer));
    assert_eq!(told["detail"]["note"], crate::services::DATA_NOTE);
}

/// A record's long detail is cut alone: its service, action and status are
/// told whole; a short one is told as it is, with no cut marked.
#[test]
fn a_long_record_detail_keeps_its_status() {
    let held = json!({
        "service": "gmail", "action": "read", "status": "done",
        "detail": {"subjects": ["é".repeat(3_000)]},
    });
    let told = detail_of(&held);
    let text = told["text"].as_str().unwrap();
    assert!(text.len() <= DETAIL_BYTES, "{}", text.len());
    let detail: Value = serde_json::from_str(text).unwrap();
    assert_eq!(detail["status"], "done", "{detail}");
    assert_eq!(detail["service"], "gmail");
    assert_eq!(detail["action"], "read");
    assert_eq!(detail["cut"], json!(["detail"]));
    let short = json!({"service": "gmail", "action": "read", "status": "done"});
    assert_eq!(
        serde_json::from_str::<Value>(detail_of(&short)["text"].as_str().unwrap()).unwrap(),
        short
    );
}

/// A source fetched marks its request as a service read does: nothing but
/// another read runs after it, and its exchange is kept out of the thread
/// and the running summary. Nothing fetched marks nothing (the first
/// builder's P2 1 on #7571).
#[test]
fn a_fetched_source_marks_the_request_as_a_read() {
    let mut fake = Fake::new("marked");
    fake.task(OWNER, "t-s1-7", 7, "Book the dentist");
    let request = 7_571_001;
    let told = fake.ask(request, "t-s1-7");
    assert_eq!(told["found"], true, "{told}");
    assert!(!crate::services::after_read(request));
    told_as_read(request, &told);
    assert!(crate::services::after_read(request));
    let gone = not_found(NO_LONGER_AVAILABLE);
    told_as_read(request + 1, &gone);
    assert!(!crate::services::after_read(request + 1));
}

/// Forget everything leaves no linked row: `record_source` finds nothing
/// for any key of theirs, and the sources were sent no delete or write
/// (lamp-tasks was sent nothing at all).
#[test]
fn forget_all_leaves_no_linked_row_and_the_sources_untouched() {
    let mut fake = Fake::new("forget-all");
    fake.task(OWNER, "t-s1-7", 7, "Book the dentist");
    fake.put(
        OWNER,
        "r-4-set",
        "reminder",
        "Take the pills at 9",
        json!({"service":"lamp-schedule","id":"4"}),
    );
    let forgot = fake.send(json!({"op":"forget_all","person":OWNER}), NOW + 1_000);
    assert_eq!(forgot["ok"], true, "{forgot}");
    let listed = fake.send(
        json!({"op":"service_records","person":OWNER,"limit":50}),
        NOW + 2_000,
    );
    assert_eq!(listed["records"], json!([]));
    fake.sent.clear();
    for (request, key) in [(1, "t-s1-7"), (2, "r-4-set")] {
        let told = fake.ask(request, key);
        assert_eq!(told, json!({"found":false,"reason":NOT_YOURS}), "{key}");
    }
    reads_only(&fake.sent);
    assert!(fake.sent.iter().all(|line| line["op"] == "service_records"));
    // The task is still held by lamp-tasks.
    assert!(fake.tasks.iter().any(|(id, ..)| *id == 7));
}

/// A forget about a thing takes only the record whose summary matches:
/// the other is still fetched.
#[test]
fn a_forget_about_a_thing_removes_only_the_matching_row() {
    let mut fake = Fake::new("forget-about");
    fake.tasks
        .push((8, "Order printer paper", "Ordered two boxes"));
    fake.task(OWNER, "t-s1-7", 7, "Book the dentist");
    fake.task(OWNER, "t-s1-8", 8, "Order printer paper");
    let forgot = fake.send(
        json!({"op":"service_records_forget","person":OWNER,"about":[["dentist"]]}),
        NOW + 1_000,
    );
    assert_eq!(forgot["removed"], 1, "{forgot}");
    assert_eq!(fake.ask(1, "t-s1-7")["reason"], NOT_YOURS);
    let kept = fake.ask(2, "t-s1-8");
    assert_eq!(kept["found"], true, "{kept}");
    assert!(
        kept["detail"]["text"]
            .as_str()
            .unwrap()
            .contains("Ordered two boxes")
    );
    reads_only(&fake.sent);
}

/// A source gone at its service (the task pruned, or the store put back)
/// is `no_longer_available` with the rule, and nothing of it is told: the
/// voice-eval check that no detail is invented holds on what the model is
/// given, its summary and the rule alone.
#[test]
fn a_gone_source_is_no_longer_available_and_nothing_is_invented() {
    let mut fake = Fake::new("gone");
    fake.task(OWNER, "t-s1-9", 9, "Book the dentist");
    let told = fake.ask(1, "t-s1-9");
    assert_eq!(
        told,
        json!({"found":false,"reason":NO_LONGER_AVAILABLE,"rule":GONE_RULE})
    );
    // No detail and no summary beyond what the note already had.
    assert!(told.get("detail").is_none());
    assert!(GONE_RULE.contains("Never describe it from memory"));
    assert!(tool().description.contains(GONE_RULE));
    // A store put back: the same id under another store is gone too.
    fake.task(OWNER, "t-s1-7", 7, "Book the dentist");
    fake.store = "s2".into();
    assert_eq!(fake.ask(2, "t-s1-7")["reason"], NO_LONGER_AVAILABLE);
    reads_only(&fake.sent);
}

/// The privacy gate: another person's key is `not_yours`; no known person
/// is `not_yours` and asks nothing; someone else maybe here is told
/// `BRIEF` and nothing is fetched.
#[test]
fn the_privacy_gate_holds() {
    let mut fake = Fake::new("privacy");
    fake.task(OTHER, "t-s1-7", 7, "Their dentist");
    assert_eq!(fake.ask(1, "t-s1-7")["reason"], NOT_YOURS);
    assert!(fake.sent.iter().all(|line| line["op"] == "service_records"));
    fake.sent.clear();
    let mut nobody = Fake::new("privacy-nobody");
    let told = answer(
        &mut nobody,
        &Asker {
            person: None,
            in_full: true,
        },
        1,
        &json!({"key":"t-s1-7"}),
        &|| Duration::ZERO,
    );
    assert_eq!(told["reason"], NOT_YOURS);
    assert!(nobody.sent.is_empty());
    let told = fake.ask_as(OTHER, false, 2, "t-s1-7");
    assert_eq!(
        told,
        json!({"found":false,"reason":OTHERS_HERE,"note":super::super::told::BRIEF})
    );
    assert!(fake.sent.is_empty());
}

/// Each service call waits at most 3 s, the call 4 s in all: a source that
/// does not answer is `timed_out`, and with the 4 s spent nothing more is
/// asked.
#[test]
fn timeouts_are_bounded() {
    let mut fake = Fake::new("timeouts");
    fake.task(OWNER, "t-s1-7", 7, "Book the dentist");
    fake.tasks_silent = true;
    assert_eq!(fake.ask(1, "t-s1-7")["reason"], TIMED_OUT);
    assert!(
        fake.waits.iter().all(|wait| *wait <= PER_CALL),
        "{:?}",
        fake.waits
    );
    // The memory's read took 3.5 s: 0.5 s is left for lamp-tasks.
    fake.tasks_silent = false;
    fake.waits.clear();
    fake.clock.set(Duration::ZERO);
    fake.takes = Duration::from_millis(3_500);
    let _ = fake.ask(2, "t-s1-7");
    assert_eq!(fake.waits[0], PER_CALL);
    assert_eq!(fake.waits[1], Duration::from_millis(500));
    // Then the 4 s are spent: the observation is not asked.
    assert_eq!(fake.waits.len(), 2, "{:?}", fake.waits);
    // Already past 4 s: nothing is asked at all.
    fake.sent.clear();
    fake.clock.set(TOTAL);
    fake.takes = Duration::ZERO;
    assert_eq!(fake.ask(3, "t-s1-7")["reason"], TIMED_OUT);
    assert!(fake.sent.is_empty());
}

/// At most 2 service calls a turn: a task's fetch spends them on
/// `get_task` and `get_observation` (the memory's read not counted); a
/// second `record_source` in the same request asks nothing, the memory
/// neither; the next request may again.
#[test]
fn at_most_two_calls_a_turn() {
    let mut fake = Fake::new("cap");
    fake.task(OWNER, "t-s1-7", 7, "Book the dentist");
    let first = fake.ask(5, "t-s1-7");
    assert_eq!(first["found"], true, "{first}");
    let tasks = fake
        .sent
        .iter()
        .filter(|line| line["op"] != "service_records");
    assert_eq!(tasks.count(), 2);
    fake.sent.clear();
    assert_eq!(fake.ask(5, "t-s1-7")["reason"], TOO_MANY);
    assert!(fake.sent.is_empty());
    assert_eq!(fake.ask(6, "t-s1-7")["found"], true);
}

/// An agent's row naming a session is never fetched: `in_agent`. A
/// reminder's is told from what its record holds.
#[test]
fn an_agent_session_stays_in_the_agent_and_a_reminder_is_told_from_its_record() {
    let mut fake = Fake::new("kinds");
    fake.put(
        OWNER,
        "a-run1-0",
        "agent",
        "Read the inbox",
        json!({"service":"lamp-services","id":"run1","session":"sess-9"}),
    );
    fake.put(
        OWNER,
        "r-4-set",
        "reminder",
        "Take the pills at 9",
        json!({"service":"lamp-schedule","id":"4"}),
    );
    assert_eq!(
        fake.ask(1, "a-run1-0"),
        json!({"found":false,"reason":IN_AGENT,"rule":IN_AGENT_RULE})
    );
    let reminder = fake.ask(2, "r-4-set");
    assert_eq!(reminder["found"], true, "{reminder}");
    assert_eq!(reminder["from"], "record");
    assert_eq!(reminder["summary"], "Take the pills at 9");
    assert!(fake.sent.iter().all(|line| line["op"] == "service_records"));
}

/// Off (either switch), nothing is run.
#[test]
fn off_runs_nothing() {
    crate::service_records::on_for_test(true);
    on_for_test(false);
    assert!(!on());
    on_for_test(true);
    crate::service_records::on_for_test(false);
    assert!(!on());
    let (told, answered) = run(
        std::path::Path::new("/missing-memory.sock"),
        None,
        &json!({"key":"t-s1-7"}),
        1,
        None,
    );
    assert!(!answered);
    assert_eq!(told, crate::tools::unavailable());
    on_for_test(false);
}
