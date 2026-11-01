//! lamp-schedule's stores as earlier builds wrote them, kept as fixtures in
//! `tests/fixtures/stores/`, one for each change of their shape: every later
//! build reads each with its reminders and timers as they were, so an update
//! never leaves a lamp's reminders behind (the build row "Updates cover the
//! whole lamp, V3 to V5"). The ignored writers make a new day's file from
//! this build's own code.
use lamp_schedule::{
    engine::{Engine, Persistence},
    protocol::{Guard, Request},
    store::Store,
    time::{ClockSample, Weekday, When},
};
use serde_json::Value;
use std::{cell::RefCell, fs, io, os::unix::fs::PermissionsExt, rc::Rc};

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
        boot_id: "schedule-kept-test".into(),
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

fn set<P: Persistence>(engine: &mut Engine<P>, text: &str, when: When, at: u64) -> Value {
    let request = Request::Set {
        person: None,
        guard: guard(engine),
        text: text.into(),
        when,
    };
    engine.answer(request, &now(at)).unwrap()
}

/// Writes this build's state, with a timer snoozed, a reminder at a time,
/// one daily and one weekly, to the path in `LAMP_SCHEDULE_STATE_FIXTURE`.
#[test]
#[ignore = "writes a state fixture: run by hand with LAMP_SCHEDULE_STATE_FIXTURE set"]
fn write_a_state_fixture() {
    let path =
        std::env::var_os("LAMP_SCHEDULE_STATE_FIXTURE").expect("LAMP_SCHEDULE_STATE_FIXTURE");
    let disk = Memory::default();
    let mut engine = Engine::open(disk.clone(), "Etc/UTC", &now(0), STORE).unwrap();
    set(
        &mut engine,
        "Take the bread out",
        When::After { seconds: 60 },
        0,
    );
    set(
        &mut engine,
        "Call the plumber",
        When::At {
            time: "18:30".into(),
            day: None,
            zone: None,
            fold: None,
        },
        0,
    );
    set(
        &mut engine,
        "Water the plants",
        When::Daily {
            time: "08:00".into(),
            zone: None,
        },
        0,
    );
    set(
        &mut engine,
        "Put the bins out",
        When::Weekly {
            weekday: Weekday::Monday,
            time: "07:00".into(),
            zone: None,
        },
        0,
    );
    engine.tick(&now(60_000)).unwrap();
    let snooze = Request::Snooze {
        asker: None,
        guard: guard(&engine),
        id: 1,
        delivery: 1,
        seconds: 300,
    };
    engine.answer(snooze, &now(60_000)).unwrap();
    drop(engine);
    fs::write(path, disk.load().unwrap().unwrap()).unwrap();
}

/// Writes this build's store, holding the state fixture's snapshot, as SQL
/// to the path in `LAMP_SCHEDULE_STORE_FIXTURE`.
#[test]
#[ignore = "writes a store fixture: run by hand with LAMP_SCHEDULE_STORE_FIXTURE set"]
fn write_a_store_fixture() {
    let path =
        std::env::var_os("LAMP_SCHEDULE_STORE_FIXTURE").expect("LAMP_SCHEDULE_STORE_FIXTURE");
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("private");
    let mut store = Store::open(&dir).unwrap();
    store
        .commit(include_bytes!("../fixtures/stores/state-2026-10-21.json"))
        .unwrap();
    drop(store);
    let db = rusqlite::Connection::open_with_flags(
        dir.canonicalize().unwrap().join("schedule.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let pragma = |name: &str| -> i64 {
        db.query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))
            .unwrap()
    };
    let mut sql = format!("PRAGMA page_size = {};\n", pragma("page_size"));
    let tables: Vec<String> = db
        .prepare("SELECT sql FROM sqlite_schema ORDER BY rowid")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    for table in tables {
        sql += &format!("{table};\n");
    }
    let snapshot: Vec<u8> = db
        .query_row(
            "SELECT snapshot FROM state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let hex: String = snapshot.iter().map(|byte| format!("{byte:02x}")).collect();
    sql += &format!("INSERT INTO state VALUES (1, X'{hex}');\n");
    sql += &format!("PRAGMA user_version = {};\n", pragma("user_version"));
    fs::write(path, sql).unwrap();
}

/// Each kept state, opened as an update's restart opens it, the same boot
/// later and after a reboot (the reviewer-lead's P3 on #2422: a snoozed
/// timer's elapsed deadline is the boot's that set it), reads with its items
/// as they were, the snooze still due at its wall time.
#[test]
fn each_kept_state_reads_with_its_items() {
    let kept: [(&str, &[u8]); 1] = [(
        "2026-10-21",
        include_bytes!("../fixtures/stores/state-2026-10-21.json"),
    )];
    let rebooted = ClockSample {
        unix_ms: WALL + 120_000,
        boot_ms: 5_000,
        boot_id: "schedule-kept-reboot".into(),
        wall_trusted: true,
    };
    for (day, bytes) in kept {
        for (how, at) in [("later", now(120_000)), ("rebooted", rebooted.clone())] {
            let day = format!("{day}, {how}");
            let disk = Memory(Rc::new(RefCell::new(Some(bytes.to_vec()))));
            let mut engine = Engine::open(disk, "Etc/UTC", &at, FRESH)
                .unwrap_or_else(|error| panic!("{day}: {error}"));
            assert_eq!(engine.store_id(), STORE, "{day}: the same store");
            let list = engine
                .answer(
                    Request::List {
                        after_id: 0,
                        limit: 8,
                        pending_only: false,
                    },
                    &at,
                )
                .unwrap();
            let texts: Vec<&str> = list["items"]
                .as_array()
                .unwrap_or_else(|| panic!("{day}: {list}"))
                .iter()
                .map(|item| item["text"].as_str().unwrap())
                .collect();
            assert_eq!(
                texts,
                [
                    "Take the bread out",
                    "Call the plumber",
                    "Water the plants",
                    "Put the bins out"
                ],
                "{day}"
            );
            // Each says when it was set, from the creation time the store
            // has always kept: the fixture's, 14 Oct at 08:00 UTC.
            let set = engine
                .answer(
                    Request::ListSet {
                        asker: None,
                        after_id: 0,
                        limit: 8,
                        pending_only: false,
                    },
                    &at,
                )
                .unwrap();
            for item in set["items"].as_array().unwrap() {
                assert_eq!(
                    (item["set_local"].as_str(), item["set_day"].as_str()),
                    (Some("2026-10-14T08:00:00+00:00[Etc/UTC]"), Some("today")),
                    "{day}: {item}"
                );
            }
            let snoozed = engine.answer(Request::Get { id: 1 }, &at).unwrap();
            let item = &snoozed["item"];
            assert_eq!(
                (
                    item["snoozed"].as_bool(),
                    item["delivery"].as_u64(),
                    item["local"].as_str()
                ),
                (
                    Some(true),
                    Some(2),
                    Some("2026-10-14T08:06:00+00:00[Etc/UTC]")
                ),
                "{day}: {snoozed}"
            );
            let weekly = engine.answer(Request::Get { id: 4 }, &at).unwrap();
            assert_eq!(
                (
                    weekly["item"]["repeat"]["weekday"].as_str(),
                    weekly["item"]["local"].as_str()
                ),
                (Some("monday"), Some("2026-10-19T07:00:00+00:00[Etc/UTC]")),
                "{day}: {weekly}"
            );
        }
    }
}

/// Each kept store, rebuilt from its SQL in a folder the store made, opens
/// with the same snapshot.
#[test]
fn each_kept_store_opens_with_its_snapshot() {
    let kept: [(&str, &str, &[u8]); 1] = [(
        "2026-10-21",
        include_str!("../fixtures/stores/store-2026-10-21.sql"),
        include_bytes!("../fixtures/stores/state-2026-10-21.json"),
    )];
    for (day, sql, snapshot) in kept {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("private");
        drop(Store::open(&dir).unwrap());
        let file = dir.canonicalize().unwrap().join("schedule.sqlite3");
        fs::remove_file(&file).unwrap();
        let db = rusqlite::Connection::open(&file).unwrap();
        db.execute_batch(sql).unwrap();
        drop(db);
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        let store = Store::open(&dir).unwrap_or_else(|error| panic!("{day}: {error}"));
        assert_eq!(store.load().unwrap().as_deref(), Some(snapshot), "{day}");
    }
}
