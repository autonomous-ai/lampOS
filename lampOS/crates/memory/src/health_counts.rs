//! Counts for opt-in health reports. Legacy files are read without opening or
//! locking their store. SQLite counts stay on the owning connection, so a
//! reporter never needs a second reader that could delay a writer.
use rusqlite::Connection;
use std::{io, path::Path};

/// Retained exchanges and explicitly kept things, across every person.
/// Observations, jobs, session notes and unreadable legacy lines are excluded.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Counts {
    pub exchanges: u64,
    pub kept: u64,
}

/// Read the legacy files without locks, writes, expiry or recovery. The caller
/// must discard a sample raced by a mutation; a multi-file read is not atomic.
pub fn legacy(dir: &Path) -> io::Result<Counts> {
    crate::store::health_counts(dir)
}

pub(crate) fn enable(db: &mut Connection) -> rusqlite::Result<()> {
    let enabled: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_temp_master WHERE type='table' AND name='memory_health_counts')",
        [],
        |row| row.get(0),
    )?;
    if enabled {
        return Ok(());
    }
    let tx = db.transaction()?;
    tx.execute_batch(
        "CREATE TEMP TABLE memory_health_counts (
            singleton INTEGER PRIMARY KEY CHECK(singleton=1),
            exchanges INTEGER NOT NULL CHECK(exchanges>=0),
            kept INTEGER NOT NULL CHECK(kept>=0)
        );
        INSERT INTO memory_health_counts
            SELECT 1, count(CASE WHEN kind='exchange' THEN 1 END),
                      count(CASE WHEN kind='kept' THEN 1 END)
            FROM main.entries;
        CREATE TEMP TRIGGER memory_health_insert AFTER INSERT ON main.entries
            WHEN NEW.kind IN ('exchange','kept') BEGIN
            UPDATE memory_health_counts SET
                exchanges=exchanges+(NEW.kind='exchange'),
                kept=kept+(NEW.kind='kept') WHERE singleton=1;
        END;
        CREATE TEMP TRIGGER memory_health_delete AFTER DELETE ON main.entries
            WHEN OLD.kind IN ('exchange','kept') BEGIN
            UPDATE memory_health_counts SET
                exchanges=exchanges-(OLD.kind='exchange'),
                kept=kept-(OLD.kind='kept') WHERE singleton=1;
        END;
        CREATE TEMP TRIGGER memory_health_kind AFTER UPDATE OF kind ON main.entries
            WHEN OLD.kind!=NEW.kind BEGIN
            UPDATE memory_health_counts SET
                exchanges=exchanges-(OLD.kind='exchange')+(NEW.kind='exchange'),
                kept=kept-(OLD.kind='kept')+(NEW.kind='kept') WHERE singleton=1;
        END;",
    )?;
    tx.commit()
}

pub(crate) fn sqlite(db: &Connection) -> rusqlite::Result<Counts> {
    db.query_row(
        "SELECT exchanges,kept FROM temp.memory_health_counts WHERE singleton=1",
        [],
        |row| {
            Ok(Counts {
                exchanges: row.get(0)?,
                kept: row.get(1)?,
            })
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        days::{DATABASE, Days},
        store::{KEPT_ENTRIES, Kind, Options, Person, Store},
        tidy::{Category, Output, Proposal},
    };
    use std::{
        fs::{self, DirBuilder, OpenOptions},
        io::Write,
        os::unix::fs::{DirBuilderExt, symlink},
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    const NOW: u64 = 1_791_892_800_000;
    const DAY: u64 = 86_400_000;
    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!(
                "lamp-memory-health-counts-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            DirBuilder::new().mode(0o700).create(&dir).unwrap();
            Self(dir)
        }

        fn options(&self, keep_days: u32) -> Options {
            Options {
                dir: self.0.clone(),
                keep_days,
                local: crate::store::LocalTime::Offset(0),
            }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn legacy_counts_readable_records_after_capped_replacement_and_forgetting() {
        let scratch = Scratch::new();
        let mut store = Store::open(scratch.options(30), NOW).unwrap();
        let room = Person::everyone();
        let other = Person::new("other").unwrap();
        for n in 0..=KEPT_ENTRIES {
            store.remember(&room, NOW, &format!("fact {n}")).unwrap();
        }
        store.remember(&other, NOW, "another fact").unwrap();
        store.append(&room, NOW, "heard", "said", NOW).unwrap();
        store.append(&other, NOW, "heard", "said", NOW).unwrap();
        let journal = scratch.0.join("everyone/journal-2026-10-13.jsonl");
        OpenOptions::new()
            .append(true)
            .open(&journal)
            .unwrap()
            .write_all(b"not JSON\n{\"a cut-off line\"")
            .unwrap();
        let before = fs::read(&journal).unwrap();
        let unfinished = scratch.0.join("everyone/kept.jsonl.tmp");
        fs::write(&unfinished, b"left by an interrupted operation").unwrap();
        fs::create_dir(scratch.0.join("not a person")).unwrap();
        assert_eq!(
            legacy(&scratch.0).unwrap(),
            Counts {
                exchanges: 2,
                kept: KEPT_ENTRIES as u64 + 1,
            }
        );
        assert_eq!(fs::read(&journal).unwrap(), before);
        assert!(unfinished.exists());
        store.forget_last(&room, Some(Kind::Kept)).unwrap();
        store.forget_all(Some(&other)).unwrap();
        assert_eq!(
            legacy(&scratch.0).unwrap(),
            Counts {
                exchanges: 1,
                kept: KEPT_ENTRIES as u64 - 1,
            }
        );
        store.forget_all(None).unwrap();
        assert_eq!(legacy(&scratch.0).unwrap(), Counts::default());
        assert!(legacy(&scratch.0.join("missing")).is_err());
        assert!(!scratch.0.join("missing").exists());
    }

    #[test]
    fn legacy_probe_bounds_lines_and_accepts_a_valid_final_line_without_newline() {
        let scratch = Scratch::new();
        let person = scratch.0.join("everyone");
        fs::create_dir(&person).unwrap();
        let kept = person.join("kept.jsonl");
        let record = br#"{"id":1,"at_unix_ms":1791892800000,"text":"kept"}"#;
        let mut exact = vec![b' '; 64 * 1024 - record.len()];
        exact.extend_from_slice(record);
        fs::write(&kept, &exact).unwrap();
        assert_eq!(
            legacy(&scratch.0).unwrap(),
            Counts {
                exchanges: 0,
                kept: 1
            }
        );
        exact.push(b'\n');
        exact.extend_from_slice(record);
        fs::write(&kept, &exact).unwrap();
        assert_eq!(
            legacy(&scratch.0).unwrap(),
            Counts {
                exchanges: 0,
                kept: 2
            }
        );
        let mut oversized = vec![b' '; 64 * 1024 + 1];
        for newline in [false, true] {
            if newline {
                oversized.push(b'\n');
            }
            fs::write(&kept, &oversized).unwrap();
            assert_eq!(
                legacy(&scratch.0).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
        fs::remove_file(&kept).unwrap();
        fs::write(person.join("journal-2026-10-13.jsonl"), &oversized).unwrap();
        assert_eq!(
            legacy(&scratch.0).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn legacy_probe_refuses_nonregular_kept_files_and_does_not_follow_journal_links() {
        let scratch = Scratch::new();
        let person = scratch.0.join("everyone");
        fs::create_dir(&person).unwrap();
        let kept = person.join("kept.jsonl");
        fs::create_dir(&kept).unwrap();
        assert_eq!(
            legacy(&scratch.0).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        fs::remove_dir(&kept).unwrap();
        let outside = scratch.0.join("outside.jsonl");
        let record = br#"{"id":1,"at_unix_ms":1791892800000,"text":"private","heard":"heard","said":"said"}"#;
        fs::write(&outside, record).unwrap();
        symlink(&outside, &kept).unwrap();
        assert_eq!(
            legacy(&scratch.0).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        fs::remove_file(&kept).unwrap();
        fs::write(&kept, record).unwrap();
        symlink(&outside, person.join("journal-2026-10-13.jsonl")).unwrap();
        fs::create_dir(person.join("journal-2026-10-14.jsonl")).unwrap();
        symlink(&person, scratch.0.join("another_person")).unwrap();
        assert_eq!(
            legacy(&scratch.0).unwrap(),
            Counts {
                exchanges: 0,
                kept: 1,
            }
        );
        assert_eq!(
            legacy(&outside).unwrap_err().kind(),
            io::ErrorKind::NotADirectory
        );
    }

    #[test]
    fn sqlite_counts_are_temporary_idempotent_and_follow_commit_rollback_and_kind() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE entries(id INTEGER PRIMARY KEY,kind TEXT NOT NULL);
             INSERT INTO entries VALUES(1,'exchange'),(2,'kept'),(3,'observation');",
        )
        .unwrap();
        let schema = |db: &Connection| -> String {
            db.query_row(
                "SELECT group_concat(sql) FROM main.sqlite_master",
                [],
                |r| r.get(0),
            )
            .unwrap()
        };
        let before = schema(&db);
        assert!(sqlite(&db).is_err());
        enable(&mut db).unwrap();
        enable(&mut db).unwrap();
        assert_eq!(schema(&db), before);
        assert_eq!(
            sqlite(&db).unwrap(),
            Counts {
                exchanges: 1,
                kept: 1
            }
        );
        {
            let tx = db.transaction().unwrap();
            tx.execute("DELETE FROM entries WHERE id=1", []).unwrap();
            tx.execute("INSERT INTO entries VALUES(4,'kept')", [])
                .unwrap();
            assert_eq!(
                sqlite(&tx).unwrap(),
                Counts {
                    exchanges: 0,
                    kept: 2
                }
            );
            tx.rollback().unwrap();
        }
        assert_eq!(
            sqlite(&db).unwrap(),
            Counts {
                exchanges: 1,
                kept: 1
            }
        );
        {
            let tx = db.transaction().unwrap();
            tx.execute("UPDATE entries SET kind='kept' WHERE id=1", [])
                .unwrap();
            tx.execute("UPDATE entries SET kind='exchange' WHERE id=3", [])
                .unwrap();
            tx.execute("DELETE FROM entries WHERE id=2", []).unwrap();
            tx.execute("INSERT INTO entries VALUES(4,'observation')", [])
                .unwrap();
            tx.commit().unwrap();
        }
        assert_eq!(
            sqlite(&db).unwrap(),
            Counts {
                exchanges: 1,
                kept: 1
            }
        );
        db.execute("DELETE FROM entries", []).unwrap();
        assert_eq!(sqlite(&db).unwrap(), Counts::default());
    }

    #[test]
    fn days_counts_exclude_observations_and_notes_and_follow_cascading_deletion() {
        let scratch = Scratch::new();
        let mut store = Days::open(scratch.options(30), NOW).unwrap();
        let room = Person::everyone();
        let other = Person::new("other").unwrap();
        let source = store
            .append(&room, NOW, "Coffee black", "Noted", &[])
            .unwrap();
        store.remember(&room, NOW, "explicitly kept").unwrap();
        store.remember(&other, NOW, "another person").unwrap();
        assert!(store.health_counts().is_err());
        store.enable_health_counts().unwrap();
        let job = store.conversation_end(&room, "coffee", &[source]).unwrap();
        let observation = store
            .tidy_apply(
                &room,
                &job.job,
                &Output {
                    observations: vec![Proposal {
                        text: "Coffee black".into(),
                        category: Category::Preference,
                        source_ids: vec![source],
                        supersedes: None,
                        replaces: None,
                        on: None,
                    }],
                },
            )
            .unwrap()[0];
        store
            .append(&room, NOW, "Coffee again", "Black", &[observation])
            .unwrap();
        store.session_open(&room, "session", 1200).unwrap();
        assert_eq!(
            store.health_counts().unwrap(),
            Counts {
                exchanges: 2,
                kept: 2
            }
        );
        assert_eq!(store.forget(&room, observation).unwrap(), 3);
        assert_eq!(
            store.health_counts().unwrap(),
            Counts {
                exchanges: 0,
                kept: 2
            }
        );
        store.forget_all(Some(&other)).unwrap();
        assert_eq!(
            store.health_counts().unwrap(),
            Counts {
                exchanges: 0,
                kept: 1
            }
        );
        store.forget_all(None).unwrap();
        assert_eq!(store.health_counts().unwrap(), Counts::default());
        drop(store);
        let mut reopened = Days::open(scratch.options(30), NOW).unwrap();
        assert!(reopened.health_counts().is_err());
        reopened.enable_health_counts().unwrap();
        assert_eq!(reopened.health_counts().unwrap(), Counts::default());
    }

    #[test]
    fn days_counts_follow_expiry_and_a_failed_delete_keeps_the_previous_counts() {
        let scratch = Scratch::new();
        let room = Person::everyone();
        let mut store = Days::open(scratch.options(1), NOW).unwrap();
        store
            .append(&room, NOW - 2 * DAY, "old", "said", &[])
            .unwrap();
        let current = store.append(&room, NOW, "current", "said", &[]).unwrap();
        store.remember(&room, NOW, "kept").unwrap();
        store.enable_health_counts().unwrap();
        assert_eq!(
            store.health_counts().unwrap(),
            Counts {
                exchanges: 2,
                kept: 1
            }
        );
        store.maintain(NOW).unwrap();
        assert_eq!(
            store.health_counts().unwrap(),
            Counts {
                exchanges: 1,
                kept: 1
            }
        );
        drop(store);
        {
            let db = Connection::open(scratch.0.join(DATABASE)).unwrap();
            db.execute_batch("CREATE TRIGGER fail_health_delete BEFORE DELETE ON entries BEGIN SELECT RAISE(ABORT,'test delete failure'); END;").unwrap();
        }
        let mut store = Days::open(scratch.options(1), NOW).unwrap();
        store.enable_health_counts().unwrap();
        assert_eq!(store.forget(&room, current).unwrap_err().code, "storage");
        assert_eq!(
            store.health_counts().unwrap(),
            Counts {
                exchanges: 1,
                kept: 1
            }
        );
        assert!(store.record(&room, current).unwrap().is_some());
        drop(store);
        {
            let db = Connection::open(scratch.0.join(DATABASE)).unwrap();
            db.execute_batch("DROP TRIGGER fail_health_delete;")
                .unwrap();
        }
        let mut store = Days::open(scratch.options(1), NOW + 3 * DAY).unwrap();
        store.enable_health_counts().unwrap();
        assert_eq!(
            store.health_counts().unwrap(),
            Counts {
                exchanges: 0,
                kept: 1
            }
        );
    }
}
