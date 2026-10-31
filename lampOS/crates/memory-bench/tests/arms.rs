use lamp_memory_bench::{
    arms::{Basic, BasicKind, MemoryArm},
    hash,
    partial::Partial,
    protocol::*,
};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "memory-arms-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
        Self(p)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
const NOW: u64 = 1_900_000_000_000;
fn event(id: &str, text: &str) -> Event {
    Event {
        record_id: id.into(),
        at_unix_ms: NOW,
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
fn query() -> Query {
    Query {
        probe_id: "q1".into(),
        at_unix_ms: NOW,
        person: Person::Owner,
        person_id: Some("owner-a".into()),
        presence: Presence::OwnerAlone,
        heard: "tea".into(),
        cues: Cues::default(),
        max_context_bytes: 16384,
    }
}
#[test]
fn basic_full_history_is_observed_only_and_fails_instead_of_truncating() {
    let d = Dir::new();
    let mut a = Basic::open(BasicKind::FullHistory, "owner-a".into(), &d.0).unwrap();
    a.ingest(event("one", "My tea is jasmine.")).unwrap();
    let mut visitor = event("two", "My tea is cocoa.");
    visitor.person = Person::Visitor;
    a.ingest(visitor).unwrap();
    let r = a.recall(query()).unwrap();
    assert_eq!(r.source_ids, ["one"]);
    assert!(r.context.contains("jasmine"));
    assert!(!r.context.contains("cocoa"));
    let mut small = query();
    small.max_context_bytes = 1;
    assert!(matches!(
        a.recall(small),
        Err(lamp_memory_bench::Error::Unsupported(_))
    ));
    let mut other = query();
    other.presence = Presence::Unknown;
    assert!(a.recall(other).unwrap().context.is_empty());
}
#[test]
fn forget_probe_restores_history_and_keyed_guards_without_plaintext_fingerprints() {
    let d = Dir::new();
    let mut a = Basic::open(BasicKind::FullHistory, "owner-a".into(), &d.0).unwrap();
    a.ingest(event("one", "My locker code is amber comet."))
        .unwrap();
    let before = fs::read(d.0.join("history.json")).unwrap();
    a.begin_probe("q1", NOW).unwrap();
    let forgotten = a
        .ingest(event("two", "Please forget my locker code."))
        .unwrap();
    assert_eq!(forgotten["forget_effect"]["ran"], true);
    assert!(a.recall(query()).unwrap().context.is_empty());
    a.end_probe("q1").unwrap();
    assert_eq!(before, fs::read(d.0.join("history.json")).unwrap());
    assert_eq!(a.recall(query()).unwrap().source_ids, ["one"]);
    a.ingest(event("three", "Please forget my locker code."))
        .unwrap();
    let stored = fs::read_to_string(d.0.join("history.json")).unwrap();
    assert!(!stored.contains("amber"));
    assert!(!stored.contains("locker"));
    a.ingest(event("four", "My locker code is amber comet."))
        .unwrap();
    assert!(a.recall(query()).unwrap().context.is_empty());
}
#[test]
fn no_memory_does_not_keep_dialogue() {
    let d = Dir::new();
    let mut a = Basic::open(BasicKind::None, "owner-a".into(), &d.0).unwrap();
    a.ingest(event("one", "My tea is jasmine.")).unwrap();
    assert!(a.recall(query()).unwrap().context.is_empty());
    assert!(
        !fs::read_to_string(d.0.join("history.json"))
            .unwrap()
            .contains("jasmine")
    );
}
#[test]
fn partial_uses_real_store_and_discards_probe_copy() {
    let d = Dir::new();
    let mut a = Partial::open(&Init {
        run_id: "test".into(),
        arm: "lampos_today_partial".into(),
        state_dir: d.0.to_string_lossy().into(),
        owner_id: "owner-a".into(),
        timezone: "Asia/Ho_Chi_Minh".into(),
        at_unix_ms: NOW,
        config: json!({}),
    })
    .unwrap();
    a.ingest(event("one", "My tea is jasmine.")).unwrap();
    assert!(a.recall(query()).unwrap().context.contains("jasmine"));
    let db = d.0.join("main/memory-days.sqlite3");
    let before = hash(&fs::read(&db).unwrap());
    a.begin_probe("q1", NOW).unwrap();
    a.snapshot(query()).unwrap();
    let r = a
        .forget(Forget {
            at_unix_ms: NOW,
            person_id: "owner-a".into(),
            record_ids: vec!["one".into()],
            text: String::new(),
        })
        .unwrap();
    assert_eq!(r["removed_count"], 1);
    assert_eq!(r["forget_effect"]["ran"], false);
    assert!(a.recall(query()).unwrap().context.is_empty());
    a.end_probe("q1").unwrap();
    assert_eq!(before, hash(&fs::read(db).unwrap()));
    assert_eq!(
        a.inspect("owner-a").unwrap()["retained_record_ids"],
        json!(["one"])
    );
    a.sleep(Sleep {
        at_unix_ms: NOW + 31 * 86400000,
        cadence: "nightly".into(),
    })
    .unwrap();
    assert_eq!(
        a.inspect("owner-a").unwrap()["retained_record_ids"],
        json!([])
    );
}
#[test]
fn lexical_forget_does_not_treat_reminders_as_deletions() {
    assert_eq!(
        lamp_memory_bench::arms::forgetting("Don't forget my appointment"),
        None
    );
    assert_eq!(
        lamp_memory_bench::arms::forgetting("Please forget my locker code."),
        Some("locker code".into())
    );
}

#[test]
fn reference_processes_pass_the_same_twelve_operation_contract() {
    for arm in ["none", "full_history", "lampos_today_partial"] {
        let root = Dir::new();
        let state = root.0.join("state");
        let revision = "0".repeat(40);
        let result = lamp_memory_bench::smoke::run_with_args(
            std::path::Path::new(env!("CARGO_BIN_EXE_lamp-memory-bench")),
            &["arm".into(), revision.clone()],
            arm,
            &revision,
            &state,
        )
        .unwrap();
        assert_eq!(result["passed"], true);
        assert_eq!(result["model_calls"], 0);
        assert_eq!(result["scores"], serde_json::Value::Null);
    }
}
