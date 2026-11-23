use lamp_memory_bench::{
    arms::{Basic, BasicKind, MemoryArm},
    hash,
    partial::Partial,
    protocol::*,
};
use serde_json::{Value, json};
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
    let mut a = Basic::open(
        BasicKind::FullHistory,
        "owner-a".into(),
        "Asia/Ho_Chi_Minh",
        &d.0,
    )
    .unwrap();
    a.ingest(event("one", "My tea is jasmine.")).unwrap();
    let mut visitor = event("two", "My tea is cocoa.");
    visitor.person = Person::Visitor;
    a.ingest(visitor).unwrap();
    let r = a.recall(query()).unwrap();
    assert_eq!(r.source_ids, ["one"]);
    assert!(r.context.contains("jasmine"));
    assert!(!r.context.contains("cocoa"));
    // Each event with its local day as the voice says it, never the epoch.
    assert!(r.context.contains("\"when\":\"today, "), "{}", r.context);
    assert!(!r.context.contains("at_unix_ms"));
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
    let mut a = Basic::open(
        BasicKind::FullHistory,
        "owner-a".into(),
        "Asia/Ho_Chi_Minh",
        &d.0,
    )
    .unwrap();
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
    for file in ["history.json", "history.log"] {
        let stored = fs::read_to_string(d.0.join(file)).unwrap();
        assert!(!stored.contains("amber"));
        assert!(!stored.contains("locker"));
    }
    a.ingest(event("four", "My locker code is amber comet."))
        .unwrap();
    assert!(a.recall(query()).unwrap().context.is_empty());
}
#[test]
fn full_history_appends_each_ingest_and_keeps_the_snapshot_until_a_forget() {
    let d = Dir::new();
    let mut a = Basic::open(
        BasicKind::FullHistory,
        "owner-a".into(),
        "Asia/Ho_Chi_Minh",
        &d.0,
    )
    .unwrap();
    let snapshot = fs::read(d.0.join("history.json")).unwrap();
    a.ingest(event("one", "My tea is jasmine.")).unwrap();
    a.ingest(event("two", "My bike is blue.")).unwrap();
    assert_eq!(snapshot, fs::read(d.0.join("history.json")).unwrap());
    let log = fs::read_to_string(d.0.join("history.log")).unwrap();
    assert_eq!(log.lines().count(), 2);
    assert!(log.contains("jasmine") && log.contains("blue"));
    a.ingest(event("three", "Please forget my tea.")).unwrap();
    assert!(
        fs::read_to_string(d.0.join("history.log"))
            .unwrap()
            .is_empty()
    );
    let stored = fs::read_to_string(d.0.join("history.json")).unwrap();
    assert!(!stored.contains("jasmine") && stored.contains("blue"));
}
#[test]
fn no_memory_does_not_keep_dialogue() {
    let d = Dir::new();
    let mut a = Basic::open(BasicKind::None, "owner-a".into(), "Asia/Ho_Chi_Minh", &d.0).unwrap();
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
    let context = a.recall(query()).unwrap().context;
    assert!(context.contains("jasmine"));
    // The voice's recall shape with the local day, never the store's epoch.
    let entries: Value = serde_json::from_str(&context).unwrap();
    let today = "today, ";
    assert!(
        entries[0]["when"].as_str().unwrap().starts_with(today),
        "{context}"
    );
    assert_eq!(entries[0]["heard"], "My tea is jasmine.");
    assert_eq!(entries[0]["source_record_id"], "one");
    assert!(!context.contains("at_unix_ms") && !context.contains(&NOW.to_string()));
    let mut later = query();
    later.at_unix_ms = NOW + 3 * 86_400_000;
    let context = a.recall(later).unwrap().context;
    assert!(context.contains("\"when\":\"3 days ago, "), "{context}");
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
fn partial_next_sends_the_tiers_ideas_switches_and_today_sends_none() {
    use lamp_memory_bench::partial::{search_request, session_open_request};
    let next = search_request("owner-a", "tea", 2048, true);
    assert_eq!(next["newer_first"], true);
    assert_eq!(next["strength"], true);
    assert!(next.get("profile").is_none(), "profile is session_open's");
    let next = session_open_request("owner-a", "s", 1200, true);
    assert_eq!(next["strength"], true);
    assert_eq!(next["profile"], true);
    assert!(next.get("newer_first").is_none(), "newer_first is search's");
    // Today's setup: the exact requests it sent before the next setup.
    assert_eq!(
        search_request("owner-a", "tea", 2048, false),
        json!({"op":"search","person":"owner-a","query":"tea","limit":8,"max_bytes":2048,"history":false,"recall_v2":false})
    );
    assert_eq!(
        session_open_request("owner-a", "s", 1200, false),
        json!({"op":"session_open","person":"owner-a","session":"s","max_bytes":1200})
    );
}
#[test]
fn partial_recall_tells_strength_and_the_day_as_the_voice_does_and_never_earlier() {
    use lamp_memory_bench::{partial::recall_entry, when::zone};
    // 2030-04-07 18:00 in Ho Chi Minh City; each entry was said a day before.
    let now = 1_901_790_000_000;
    let at = now - 86_400_000;
    let z = zone("Asia/Ho_Chi_Minh").unwrap();
    let past_plan = recall_entry(
        &json!({"id":1,"kind":"kept","at_unix_ms":at,"text":"dentist","on":"2030-04-05","strength":"plan","earlier":true}),
        now,
        &z,
    )
    .unwrap();
    assert_eq!(
        past_plan,
        json!({"when":"yesterday, Saturday 6 April","asked_to_remember":"dentist","event_on":"2030-04-05","event_status":"past","today":"2030-04-07","strength":"plan: said as a plan for that day; whether it happened is not known"})
    );
    let maybe = recall_entry(
        &json!({"id":2,"kind":"kept","at_unix_ms":at,"text":"pottery","on":"2030-04-10","strength":"maybe"}),
        now,
        &z,
    )
    .unwrap();
    assert_eq!(maybe["event_status"], "upcoming");
    assert_eq!(
        maybe["strength"],
        "maybe: said as a maybe; not known to be decided or booked"
    );
    let on_today = recall_entry(
        &json!({"id":3,"kind":"kept","at_unix_ms":at,"text":"call mum","on":"2030-04-07"}),
        now,
        &z,
    )
    .unwrap();
    assert_eq!(on_today["event_status"], "today");
    assert!(on_today.get("strength").is_none());
    // A label the voice doesn't tell is dropped, and `earlier` never shows.
    let exchange = recall_entry(
        &json!({"id":4,"kind":"exchange","at_unix_ms":at,"heard":"tea?","said":"Green.","strength":"said","earlier":true,"on":"2030-04-05"}),
        now,
        &z,
    )
    .unwrap();
    assert_eq!(
        exchange,
        json!({"when":"yesterday, Saturday 6 April","heard":"tea?","you_said":"Green."})
    );
    let other = recall_entry(
        &json!({"id":5,"kind":"note","at_unix_ms":at,"text":"x","strength":"firm","earlier":true}),
        now,
        &z,
    )
    .unwrap();
    assert_eq!(
        other,
        json!({"when":"yesterday, Saturday 6 April","kind":"note","text":"x"})
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

#[test]
fn full_history_diagnostic_usage_survives_probe_restoration() {
    let d = Dir::new();
    let mut a = Basic::open(
        BasicKind::FullHistory,
        "owner-a".into(),
        "Asia/Ho_Chi_Minh",
        &d.0,
    )
    .unwrap();
    a.ingest(event("a", "My notebook is blue.")).unwrap();
    a.begin_probe("q1", NOW).unwrap();
    a.recall(query()).unwrap();
    a.end_probe("q1").unwrap();
    let inspected = a.inspect("owner-a").unwrap();
    assert_eq!(
        inspected["diagnostics"]["tiers"]["full_history"]["recall_calls"],
        1
    );
    assert_eq!(
        inspected["diagnostics"]["tiers"]["full_history"]["items"],
        1
    );
}
