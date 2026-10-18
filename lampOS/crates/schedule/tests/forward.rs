//! Forward reads preserve committed work, not a future request's meaning.
use lamp_schedule::{
    delivery::{Correlation, DeliveryLedger, PendingEvidence},
    engine::{Engine, Persistence},
    protocol::{Guard, Request},
    store::{MAX_STATE_BYTES, Store},
    time::{ClockSample, Weekday, When},
};
use serde_json::{Value, json};
use std::{cell::RefCell, fs, io, os::unix::fs::PermissionsExt, process::Command, rc::Rc};

const STORE: &str = "0123456789abcdef0123456789abcdef";
const FRESH: &str = "fedcba9876543210fedcba9876543210";
const WALL: i64 = 1_791_964_800_000;

#[derive(Clone, Default)]
struct Memory(Rc<RefCell<Option<Vec<u8>>>>);

impl Persistence for Memory {
    fn load(&self) -> io::Result<Option<Vec<u8>>> {
        Ok(self.0.borrow().clone())
    }
    fn commit(&mut self, bytes: &[u8]) -> io::Result<()> {
        *self.0.borrow_mut() = Some(bytes.to_owned());
        Ok(())
    }
}

fn now(ms: u64) -> ClockSample {
    ClockSample {
        unix_ms: WALL + ms as i64,
        boot_ms: ms,
        boot_id: "schedule-forward-test".into(),
        wall_trusted: true,
    }
}

fn guard<P: Persistence>(engine: &Engine<P>) -> Guard {
    Guard {
        store_id: engine.store_id().into(),
        revision: engine.revision(),
        request_id: format!("request-{}", engine.revision()),
    }
}

fn set<P: Persistence>(engine: &Engine<P>, when: When) -> Request {
    Request::Set {
        guard: guard(engine),
        text: "Check the timer".into(),
        when,
    }
}

fn fixture(when: When) -> (Memory, Request, Value) {
    let disk = Memory::default();
    let mut engine = Engine::open(disk.clone(), "Etc/UTC", &now(0), STORE).unwrap();
    let request = set(&engine, when);
    let response = engine.answer(request.clone(), &now(0)).unwrap();
    (disk, request, response)
}

fn snapshot(disk: &Memory) -> Value {
    serde_json::from_slice(&disk.load().unwrap().unwrap()).unwrap()
}

fn replace(disk: &Memory, value: &Value) -> Vec<u8> {
    let bytes = serde_json::to_vec(value).unwrap();
    *disk.0.borrow_mut() = Some(bytes.clone());
    bytes
}

#[test]
fn today_and_additive_fields_keep_items_pending_snooze_and_exact_receipt() {
    let (disk, _, _) = fixture(When::After { seconds: 60 });
    let mut engine = Engine::open(disk.clone(), "Etc/UTC", &now(60_000), FRESH).unwrap();
    engine.tick(&now(60_000)).unwrap();
    let request = Request::Snooze {
        guard: guard(&engine),
        id: 1,
        delivery: 1,
        seconds: 30,
    };
    let response = engine.answer(request.clone(), &now(60_000)).unwrap();
    engine.tick(&now(90_000)).unwrap();
    let revision = engine.revision();
    let item = engine.answer(Request::Get { id: 1 }, &now(90_000)).unwrap();
    drop(engine);
    let original = snapshot(&disk);
    for extended in [false, true] {
        let mut value = original.clone();
        if extended {
            for pointer in [
                "",
                "/items/0",
                "/items/0/deadline",
                "/items/0/deadline/elapsed",
                "/items/0/snooze",
                "/items/0/snooze/elapsed",
                "/items/0/pending",
                "/last_receipt",
            ] {
                value.pointer_mut(pointer).unwrap()["future_metadata"] =
                    json!({"nested": [true, 1, "kept by a later build"]});
            }
        }
        let bytes = replace(&disk, &value);
        let mut engine = Engine::open(disk.clone(), "Etc/UTC", &now(90_000), FRESH).unwrap();
        assert_eq!(engine.store_id(), STORE);
        assert_eq!(engine.revision(), revision);
        assert_eq!(engine.set_aside(), None);
        assert_eq!(
            engine.answer(Request::Get { id: 1 }, &now(90_000)).unwrap(),
            item
        );
        assert_eq!(
            engine.answer(request.clone(), &now(90_000)).unwrap(),
            response
        );
        assert_eq!(engine.counts(&now(90_000)), (1, 1, 0));
        assert_eq!(
            disk.load().unwrap(),
            Some(bytes),
            "opening/retry does not rewrite"
        );
    }
}

#[test]
fn additive_repeat_fields_preserve_both_calendar_variants() {
    for when in [
        When::Daily {
            time: "09:00".into(),
            zone: None,
        },
        When::Weekly {
            weekday: Weekday::Monday,
            time: "09:00".into(),
            zone: None,
        },
    ] {
        let (disk, request, response) = fixture(when);
        let mut value = snapshot(&disk);
        let original_repeat = value["items"][0]["deadline"]["repeat"].clone();
        value["items"][0]["deadline"]["repeat"]["future_metadata"] = json!([1, 2]);
        let bytes = replace(&disk, &value);
        let mut engine = Engine::open(disk.clone(), "Etc/UTC", &now(0), FRESH).unwrap();
        assert_eq!(engine.answer(request, &now(0)).unwrap(), response);
        assert_eq!(
            engine.answer(Request::Get { id: 1 }, &now(0)).unwrap()["item"]["repeat"],
            original_repeat
        );
        assert_eq!(disk.load().unwrap(), Some(bytes));
    }
}

#[test]
fn unreadable_or_nonmatching_receipts_are_dropped_without_reexecuting_a_mutation() {
    let (disk, request, _) = fixture(When::After { seconds: 60 });
    let original = snapshot(&disk);
    for (pointer, replacement) in [
        ("/last_receipt/request/future_metadata", json!(true)),
        ("/last_receipt/request/guard/future_metadata", json!(true)),
        ("/last_receipt/request/when/future_metadata", json!(true)),
        ("/last_receipt/request/op", json!("future_operation")),
        ("/last_receipt/request/when/kind", json!("future_time")),
        ("/last_receipt/request", json!({"op":"status"})),
        ("/last_receipt/response/future_metadata", json!(true)),
        ("/last_receipt/response/id", json!(2)),
        ("/last_receipt/response/delivery", json!(2)),
        ("/last_receipt", json!("a future receipt representation")),
    ] {
        let mut value = original.clone();
        let (parent, field) = pointer.rsplit_once('/').unwrap();
        value.pointer_mut(parent).unwrap()[field] = replacement;
        let bytes = replace(&disk, &value);
        let mut engine = Engine::open(disk.clone(), "Etc/UTC", &now(0), FRESH).unwrap();
        assert_eq!(engine.store_id(), STORE);
        assert_eq!(engine.revision(), 1);
        assert_eq!(engine.counts(&now(0)), (1, 0, 0));
        assert!(engine.answer(Request::Status {}, &now(0)).unwrap()["last_receipt"].is_null());
        assert_eq!(
            engine.answer(request.clone(), &now(0)).unwrap_err().code,
            "conflict"
        );
        assert_eq!(disk.load().unwrap(), Some(bytes));
        // A new guarded request remains usable and naturally replaces the cache.
        let next = set(&engine, When::After { seconds: 120 });
        assert_eq!(engine.answer(next, &now(0)).unwrap()["id"], 2);
    }
    // A duplicate request field must not be normalized into an exact retry.
    let bytes = serde_json::to_string(&original)
        .unwrap()
        .replacen("\"seconds\":60", "\"seconds\":30,\"seconds\":60", 1)
        .into_bytes();
    *disk.0.borrow_mut() = Some(bytes.clone());
    let mut engine = Engine::open(disk.clone(), "Etc/UTC", &now(0), FRESH).unwrap();
    assert!(engine.answer(Request::Status {}, &now(0)).unwrap()["last_receipt"].is_null());
    assert_eq!(
        engine.answer(request, &now(0)).unwrap_err().code,
        "conflict"
    );
    assert_eq!(disk.load().unwrap(), Some(bytes));
}

#[test]
fn unknown_active_tags_and_invalid_known_fields_are_refused_without_rewriting() {
    let (disk, _, _) = fixture(When::Weekly {
        weekday: Weekday::Monday,
        time: "09:00".into(),
        zone: None,
    });
    let original = snapshot(&disk);
    for (pointer, replacement) in [
        ("/items/0/deadline/repeat/kind", json!("monthly")),
        ("/items/0/deadline/repeat/weekday", json!("someday")),
        ("/items/0/deadline/unix_ms", json!("later")),
        ("/items/0/deadline/local", json!("incoherent")),
        ("/items/0/id", json!(0)),
        ("/items/0/text", json!("")),
        ("/next_id", json!(1)),
        ("/schema", json!(0)),
        ("/schema", json!("2")),
    ] {
        let mut value = original.clone();
        *value.pointer_mut(pointer).unwrap() = replacement;
        let bytes = replace(&disk, &value);
        assert!(
            Engine::open(disk.clone(), "Etc/UTC", &now(0), FRESH).is_err(),
            "{pointer}"
        );
        assert_eq!(disk.load().unwrap(), Some(bytes));
    }
}

#[test]
fn duplicate_known_fields_and_oversized_snapshots_are_not_normalized_away() {
    let (disk, _, _) = fixture(When::After { seconds: 60 });
    let original = String::from_utf8(disk.load().unwrap().unwrap()).unwrap();
    for bytes in [
        original
            .replacen("\"schema\":1", "\"schema\":1,\"schema\":2", 1)
            .into_bytes(),
        original
            .replacen("\"next_id\":2", "\"next_id\":1,\"next_id\":2", 1)
            .into_bytes(),
        original
            .replacen(
                "\"at_boot_ms\":60000",
                "\"at_boot_ms\":0,\"at_boot_ms\":60000",
                1,
            )
            .into_bytes(),
        vec![b' '; MAX_STATE_BYTES + 1],
    ] {
        assert_ne!(bytes, original.as_bytes());
        *disk.0.borrow_mut() = Some(bytes.clone());
        assert!(Engine::open(disk.clone(), "Etc/UTC", &now(0), FRESH).is_err());
        assert_eq!(disk.load().unwrap(), Some(bytes));
    }
}

#[test]
fn newer_schema_is_retained_whole_and_a_fresh_store_is_usable_after_reopen() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("store");
    let future = br#"{"schema":2,"items":[{"future_kind":"unreadable here"}]}"#;
    let mut store = Store::open(&dir).unwrap();
    store.commit(future).unwrap();
    drop(store);
    let original = fs::read(dir.join("schedule.sqlite3")).unwrap();
    let mut engine = Engine::open(Store::open(&dir).unwrap(), "Etc/UTC", &now(0), FRESH).unwrap();
    let kept = engine.set_aside().unwrap().to_owned();
    assert_eq!(kept, format!("schedule.sqlite3.newer-2-{}", WALL / 1000));
    assert_eq!(fs::read(dir.join(&kept)).unwrap(), original);
    assert_eq!(engine.store_id(), FRESH);
    assert_eq!(engine.counts(&now(0)), (0, 0, 0));
    let request = set(&engine, When::After { seconds: 60 });
    let response = engine.answer(request.clone(), &now(0)).unwrap();
    drop(engine);
    let mut engine = Engine::open(Store::open(&dir).unwrap(), "Etc/UTC", &now(0), STORE).unwrap();
    assert_eq!(engine.set_aside(), None);
    assert_eq!(engine.answer(request, &now(0)).unwrap(), response);
    assert_eq!(engine.store_id(), FRESH);
    assert_eq!(fs::read(dir.join(kept)).unwrap(), original);
}

#[test]
fn newer_schema_with_invalid_new_identity_leaves_the_database_in_place() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("store");
    let mut store = Store::open(&dir).unwrap();
    store.commit(br#"{"schema":2}"#).unwrap();
    drop(store);
    let original = fs::read(dir.join("schedule.sqlite3")).unwrap();
    assert!(Engine::open(Store::open(&dir).unwrap(), "Etc/UTC", &now(0), "bad-id").is_err());
    assert_eq!(fs::read(dir.join("schedule.sqlite3")).unwrap(), original);
    assert_eq!(fs::read_dir(dir).unwrap().count(), 1);
}

#[test]
fn newer_schema_reports_once_and_a_second_start_reports_no_recovery() {
    // Keep Unix socket paths short on hosts whose default temp path is long.
    let base = if std::path::Path::new("/private/tmp").is_dir() {
        std::path::PathBuf::from("/private/tmp")
    } else {
        std::env::temp_dir()
    };
    let root = tempfile::Builder::new()
        .prefix("sfwd-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(base)
        .unwrap();
    let dir = root.path().join("state");
    Store::open(&dir)
        .unwrap()
        .commit(br#"{"schema":2}"#)
        .unwrap();
    for expected in [1, 0] {
        let output = Command::new(env!("CARGO_BIN_EXE_lamp-schedule"))
            .args(["serve", "--enable", "--dir"])
            .arg(&dir)
            .arg("--socket")
            .arg(root.path().join("call.sock"))
            .arg("--bus-root")
            .arg(root.path().join("bus"))
            .args([
                "--zone",
                "Etc/UTC",
                "--boot-id",
                "forward-journal-test",
                "--trust-clock",
                "--run-for-ms",
                "20",
            ])
            .output()
            .unwrap();
        let journal = String::from_utf8(output.stderr).unwrap();
        assert!(output.status.success(), "{journal}");
        assert_eq!(
            journal
                .lines()
                .filter(|line| line.contains("storage was written by a newer build"))
                .count(),
            expected,
            "{journal}"
        );
    }
}

/// The reviewer's P2 on #1562: after a newer store is set aside, a reminder
/// the lamp says is set must still be said. The voice's delivery ledger is
/// fenced to Schedule's store identity and refuses a changed one, so the
/// empty store keeps the set-aside one's identity and goes on from its
/// revision and ids: the ledger prepares the new reminder, and its id
/// can't meet one the ledger already holds.
#[test]
fn after_a_newer_store_is_set_aside_the_delivery_ledger_prepares_a_new_reminder() {
    const LEDGER: &str = "abcdef0123456789abcdef0123456789";
    let correlation = |sequence| Correlation {
        consumer_session: "consumer-one".into(),
        sequence,
    };
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("store");
    let mut engine = Engine::open(Store::open(&dir).unwrap(), "Etc/UTC", &now(0), STORE).unwrap();
    let first = engine
        .answer(set(&engine, When::After { seconds: 1 }), &now(0))
        .unwrap();
    let first = first["id"].as_u64().unwrap();
    engine.tick(&now(1_000)).unwrap();
    let get = Request::Get { id: first };
    let due =
        PendingEvidence::from_get(&get, &engine.answer(get.clone(), &now(1_000)).unwrap()).unwrap();
    let mut ledger = DeliveryLedger::open(Memory::default(), LEDGER).unwrap();
    ledger.prepare(&due, correlation(1)).unwrap();
    let before = engine.revision();
    drop(engine);
    // A later build writes schema 2 over the same store.
    let mut store = Store::open(&dir).unwrap();
    let mut later: Value = serde_json::from_slice(&store.load().unwrap().unwrap()).unwrap();
    later["schema"] = json!(2);
    later["a_field_from_a_later_build"] = json!(true);
    store.commit(&serde_json::to_vec(&later).unwrap()).unwrap();
    drop(store);
    // This build sets it aside and keeps its identity, a revision past it.
    let mut engine =
        Engine::open(Store::open(&dir).unwrap(), "Etc/UTC", &now(2_000), FRESH).unwrap();
    assert!(engine.set_aside().is_some());
    assert_eq!(engine.store_id(), STORE);
    assert!(
        engine.revision() > before,
        "{} after {before}",
        engine.revision()
    );
    assert_eq!(engine.counts(&now(2_000)), (0, 0, 0));
    // "Remind me", said now, is prepared for the voice to say.
    let second = engine
        .answer(set(&engine, When::After { seconds: 1 }), &now(2_000))
        .unwrap();
    let second = second["id"].as_u64().unwrap();
    assert!(second > first);
    engine.tick(&now(3_000)).unwrap();
    let get = Request::Get { id: second };
    let due =
        PendingEvidence::from_get(&get, &engine.answer(get.clone(), &now(3_000)).unwrap()).unwrap();
    ledger.prepare(&due, correlation(2)).unwrap();
}

/// Where the set-aside store's identity can't be read, the empty store gets a
/// fresh one, as before, and the ledger refuses it loudly (`store_changed`)
/// rather than taking a store it can't tell from another's.
#[test]
fn a_set_aside_store_without_a_readable_identity_starts_fresh() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("store");
    let mut store = Store::open(&dir).unwrap();
    store
        .commit(br#"{"schema":2,"store_id":"not an id","revision":3,"next_id":4}"#)
        .unwrap();
    drop(store);
    let engine = Engine::open(Store::open(&dir).unwrap(), "Etc/UTC", &now(0), FRESH).unwrap();
    assert!(engine.set_aside().is_some());
    assert_eq!(engine.store_id(), FRESH);
    assert_eq!(engine.revision(), 0);
}
