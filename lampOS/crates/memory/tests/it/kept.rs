//! Memory across days as earlier builds wrote it, kept as fixtures in
//! `tests/fixtures/stores/`, one for each change of the database's shape:
//! every later build opens each with what it holds, so an update never
//! leaves a person's memories behind (the build row "Updates cover the
//! whole lamp, V3 to V5"). The ignored writer makes a new day's file from
//! this build's own code.
use lamp_memory::{
    Options, Person,
    days::{DATABASE, Days},
    store::{Ask, LocalTime},
};
use rusqlite::{Connection, OpenFlags};
use std::{
    fs::{self, DirBuilder},
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
};

const NOW: u64 = 1_791_892_800_000;

/// A private folder of the test's own, removed when it ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("lamp-kept-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        Self(root)
    }

    fn options(&self) -> Options {
        Options {
            dir: self.0.clone(),
            keep_days: 30,
            local: LocalTime::Offset(0),
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn ask(query: Option<&str>) -> Ask<'_> {
    Ask {
        query,
        day: None,
        limit: 20,
        max_bytes: 64 * 1024,
    }
}

/// The database at `path` as SQL that rebuilds it: each table and index as
/// the build made it, every row, the full-text index's rows through its own
/// table (its shadow tables left out, since inserting there rebuilds them),
/// and `user_version`.
fn dump(path: &Path) -> String {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let one = |sql: &str| -> i64 { db.query_row(sql, [], |row| row.get(0)).unwrap() };
    let mut sql = format!("PRAGMA page_size = {};\n", one("PRAGMA page_size"));
    let objects: Vec<(String, String, String)> = db
        .prepare("SELECT type, name, sql FROM sqlite_schema WHERE sql IS NOT NULL ORDER BY rowid")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    let fts: Vec<String> = objects
        .iter()
        .filter(|(_, _, create)| create.starts_with("CREATE VIRTUAL TABLE"))
        .map(|(_, name, _)| name.clone())
        .collect();
    let shadow = |name: &str| {
        fts.iter()
            .any(|table| name.starts_with(&format!("{table}_")))
    };
    for (kind, name, create) in &objects {
        if shadow(name) || name == "sqlite_sequence" {
            continue;
        }
        sql += &format!("{create};\n");
        if kind != "table" {
            continue;
        }
        let mut columns: Vec<String> = db
            .prepare(&format!("SELECT name FROM pragma_table_info('{name}')"))
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        if fts.contains(name) {
            columns.insert(0, "rowid".into());
        }
        let listed = columns.join(",");
        let values = columns
            .iter()
            .map(|column| format!("quote({column})"))
            .collect::<Vec<_>>()
            .join(" || ',' || ");
        let rows: Vec<String> = db
            .prepare(&format!(
                "SELECT 'INSERT INTO {name}({listed}) VALUES(' || {values} || ');' FROM {name} ORDER BY rowid"
            ))
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        for row in rows {
            sql += &format!("{row}\n");
        }
    }
    let sequences: Vec<String> = db
        .prepare("SELECT 'INSERT INTO sqlite_sequence(name,seq) VALUES(' || quote(name) || ',' || seq || ');' FROM sqlite_sequence ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    for row in sequences {
        sql += &format!("{row}\n");
    }
    sql += &format!("PRAGMA user_version = {};\n", one("PRAGMA user_version"));
    sql
}

/// Writes this build's database, with two exchanges and a kept thing for
/// the room (the kept thing sourced from an exchange) and one kept thing
/// for a person, as SQL to the path in `LAMP_MEMORY_DAYS_FIXTURE`.
#[test]
#[ignore = "writes a store fixture: run by hand with LAMP_MEMORY_DAYS_FIXTURE set"]
fn write_a_days_fixture() {
    let path = std::env::var_os("LAMP_MEMORY_DAYS_FIXTURE").expect("LAMP_MEMORY_DAYS_FIXTURE");
    let scratch = Scratch::new("write");
    let room = Person::everyone();
    let an = Person::new("an").unwrap();
    let mut days = Days::open(scratch.options(), NOW).unwrap();
    let asked = days
        .append(
            &room,
            NOW,
            "When is the bread done?",
            "In ten minutes.",
            &[],
        )
        .unwrap();
    days.append(&room, NOW + 60_000, "Thank you.", "You're welcome.", &[])
        .unwrap();
    days.remember_from(
        &room,
        NOW + 120_000,
        "The bread takes ten minutes",
        &[asked],
    )
    .unwrap();
    days.remember(&an, NOW + 180_000, "Tea without sugar")
        .unwrap();
    drop(days);
    fs::write(path, dump(&scratch.0.join(DATABASE))).unwrap();
}

/// Each kept database, rebuilt in a private folder and opened as an
/// update's restart opens it (later), has what it held: the room's
/// exchanges and kept thing, the person's kept thing, and its words found
/// by the full-text search.
#[test]
fn each_kept_database_opens_with_what_it_held() {
    let kept: [(&str, &str); 2] = [
        (
            "2026-10-21",
            include_str!("../fixtures/stores/days-2026-10-21.sql"),
        ),
        // With #2849's `speakers` table (and #1693's side tables).
        (
            "2026-10-23",
            include_str!("../fixtures/stores/days-2026-10-23.sql"),
        ),
    ];
    for (day, sql) in kept {
        let scratch = Scratch::new(day);
        let file = scratch.0.join(DATABASE);
        let db = Connection::open(&file).unwrap();
        db.execute_batch(sql).unwrap();
        drop(db);
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        let mut days = Days::open(scratch.options(), NOW + 86_400_000)
            .unwrap_or_else(|error| panic!("{day}: {error}"));
        assert_eq!(days.set_aside(), None, "{day}: nothing set aside");
        let room = Person::everyone();
        let an = Person::new("an").unwrap();
        let texts = |person: &Person| -> Vec<String> {
            days.kept(person, usize::MAX)
                .unwrap()
                .into_iter()
                .map(|kept| kept.text)
                .collect()
        };
        assert_eq!(texts(&room), ["The bread takes ten minutes"], "{day}");
        assert_eq!(texts(&an), ["Tea without sugar"], "{day}");
        let heard: Vec<String> = days
            .search(&room, &ask(Some("bread")), true)
            .unwrap()
            .into_iter()
            .map(|record| format!("{}|{}|{}", record.kind, record.heard, record.text))
            .collect();
        assert!(
            heard
                .iter()
                .any(|found| found == "exchange|When is the bread done?|"),
            "{day}: {heard:?}"
        );
        assert!(
            heard
                .iter()
                .any(|found| found == "kept||The bread takes ten minutes"),
            "{day}: {heard:?}"
        );
        // A store from before #1693's event days: the table is made on
        // open, and a follow-up with `events` answers.
        assert_eq!(days.follow_up_with(&an, true).unwrap(), None, "{day}");
        // #2849's speakers: made on open in a store from before it, empty
        // (no line kept before had a speaker); the version still 1.
        let db = Connection::open(&file).unwrap();
        let one = |sql: &str| -> u64 { db.query_row(sql, [], |row| row.get(0)).unwrap() };
        assert_eq!(one("SELECT count(*) FROM speakers"), 0, "{day}");
        assert_eq!(one("PRAGMA user_version"), 1, "{day}");
    }
}
