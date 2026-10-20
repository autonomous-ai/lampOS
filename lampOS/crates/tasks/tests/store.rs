//! Filesystem and SQLite durability fixtures on this test host. Crashing a
//! child process proves rollback recovery, not survival of a physical power cut.
use lamp_tasks::store::{MAX_STATE_BYTES, Store};
use rusqlite::{Connection, params};
use std::{
    fs::{self, DirBuilder, OpenOptions},
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{self, Command},
};

const DATABASE: &str = "tasks.sqlite3";
const INITIALIZING: &str = ".tasks.sqlite3.init";

struct Scratch {
    root: tempfile::TempDir,
    dir: PathBuf,
}

impl Scratch {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("private");
        Self { root, dir }
    }

    fn open(&self) -> Store {
        Store::open(&self.dir).unwrap()
    }

    fn populated(&self) {
        self.open().commit(b"previous committed snapshot").unwrap();
    }

    fn database(&self) -> PathBuf {
        self.dir.join(DATABASE)
    }

    fn sql(&self, text: &str) {
        Connection::open(self.database())
            .unwrap()
            .execute_batch(text)
            .unwrap();
    }
}

fn private_file(path: &Path, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}

#[test]
fn empty_store_and_last_committed_opaque_bytes_survive_reopen() {
    let scratch = Scratch::new();
    let mut store = scratch.open();
    assert_eq!(store.load().unwrap(), None);
    store.commit(b"{\"pending\":[1]}").unwrap();
    let opaque = b"engine owns validation\0\xff";
    store.commit(opaque).unwrap();
    assert_eq!(store.load().unwrap().as_deref(), Some(opaque.as_slice()));
    drop(store);
    assert_eq!(
        scratch.open().load().unwrap().as_deref(),
        Some(opaque.as_slice())
    );
}

#[test]
fn exact_byte_bound_works_and_invalid_sizes_preserve_the_previous_value() {
    let scratch = Scratch::new();
    let mut store = scratch.open();
    let maximum = vec![0xff; MAX_STATE_BYTES];
    store.commit(&maximum).unwrap();
    drop(store);
    let mut store = scratch.open();
    assert_eq!(store.load().unwrap().as_deref(), Some(maximum.as_slice()));
    for bytes in [Vec::new(), vec![0; MAX_STATE_BYTES + 1]] {
        assert_eq!(
            store.commit(&bytes).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(!store.poisoned());
        assert_eq!(store.load().unwrap().as_deref(), Some(maximum.as_slice()));
    }
    store.commit(b"still usable").unwrap();
    drop(store);
    assert_eq!(
        scratch.open().load().unwrap().as_deref(),
        Some(b"still usable".as_slice())
    );
}

#[test]
fn sqlite_rejects_empty_oversized_and_non_blob_snapshots_without_replacing_state() {
    let scratch = Scratch::new();
    scratch.populated();
    let db = Connection::open(scratch.database()).unwrap();
    for change in [
        "UPDATE state SET snapshot=x''",
        "UPDATE state SET snapshot=zeroblob(8388609)",
        "UPDATE state SET snapshot='not a blob'",
    ] {
        let error = db.execute_batch(change).unwrap_err();
        assert_eq!(
            error.sqlite_error_code(),
            Some(rusqlite::ErrorCode::ConstraintViolation),
            "{change}: {error}"
        );
    }
    drop(db);
    assert_eq!(
        scratch.open().load().unwrap().as_deref(),
        Some(b"previous committed snapshot".as_slice())
    );
}

#[test]
fn the_directory_and_all_created_files_are_private() {
    let scratch = Scratch::new();
    let mut store = scratch.open();
    store.commit(b"private state").unwrap();
    assert_eq!(fs::metadata(&scratch.dir).unwrap().mode() & 0o7777, 0o700);
    for entry in fs::read_dir(&scratch.dir).unwrap() {
        let metadata = entry.unwrap().metadata().unwrap();
        assert!(metadata.is_file());
        assert_eq!(metadata.mode() & 0o7777, 0o600);
        assert_eq!(metadata.nlink(), 1);
        assert_eq!(metadata.uid(), rustix::process::geteuid().as_raw());
    }
    assert!(!scratch.dir.join(INITIALIZING).exists());
    assert!(!scratch.dir.join(format!("{DATABASE}-journal")).exists());
}

#[test]
fn exclusive_ownership_blocks_another_process_and_is_released_on_drop() {
    const CHILD: &str = "LAMP_TASKS_STORAGE_LOCK_CHILD";
    if let Some(path) = std::env::var_os(CHILD) {
        assert_eq!(
            Store::open(Path::new(&path)).err().unwrap().kind(),
            io::ErrorKind::WouldBlock
        );
        process::exit(73);
    }
    let scratch = Scratch::new();
    let mut store = scratch.open();
    store.commit(b"owned").unwrap();
    assert_eq!(
        Store::open(&scratch.dir).err().unwrap().kind(),
        io::ErrorKind::WouldBlock
    );
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "exclusive_ownership_blocks_another_process_and_is_released_on_drop",
        ])
        .env(CHILD, &scratch.dir)
        .output()
        .unwrap();
    assert_eq!(child.status.code(), Some(73), "{child:?}");
    drop(store);
    assert_eq!(
        scratch.open().load().unwrap().as_deref(),
        Some(b"owned".as_slice())
    );
}

#[test]
fn relative_paths_and_linked_or_public_directories_are_refused() {
    assert_eq!(
        Store::open(Path::new("relative-tasks-test"))
            .err()
            .unwrap()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    let scratch = Scratch::new();
    DirBuilder::new().mode(0o700).create(&scratch.dir).unwrap();
    let linked = scratch.root.path().join("linked");
    symlink(&scratch.dir, &linked).unwrap();
    assert!(Store::open(&linked).is_err());
    assert!(!scratch.database().exists());
    fs::set_permissions(&scratch.dir, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Store::open(&scratch.dir).is_err());
    assert_eq!(fs::metadata(&scratch.dir).unwrap().mode() & 0o7777, 0o755);
}

#[test]
fn existing_empty_or_non_database_files_are_never_reinitialized() {
    for original in [b"".as_slice(), b"not a SQLite database".as_slice()] {
        let scratch = Scratch::new();
        DirBuilder::new().mode(0o700).create(&scratch.dir).unwrap();
        private_file(&scratch.database(), original);
        assert!(Store::open(&scratch.dir).is_err());
        assert_eq!(fs::read(scratch.database()).unwrap(), original);
    }
}

#[test]
fn database_symlinks_hardlinks_directories_and_public_modes_are_refused() {
    for attack in ["symlink", "hardlink", "directory", "mode"] {
        let scratch = Scratch::new();
        scratch.populated();
        let original = fs::read(scratch.database()).unwrap();
        let outside = scratch.root.path().join("outside");
        match attack {
            "symlink" => {
                fs::rename(scratch.database(), &outside).unwrap();
                symlink(&outside, scratch.database()).unwrap();
            }
            "hardlink" => fs::hard_link(scratch.database(), &outside).unwrap(),
            "directory" => {
                fs::rename(scratch.database(), &outside).unwrap();
                DirBuilder::new()
                    .mode(0o700)
                    .create(scratch.database())
                    .unwrap();
            }
            "mode" => {
                fs::set_permissions(scratch.database(), fs::Permissions::from_mode(0o640)).unwrap()
            }
            _ => unreachable!(),
        }
        assert!(Store::open(&scratch.dir).is_err(), "{attack}");
        let retained = if attack == "mode" {
            scratch.database()
        } else {
            outside
        };
        assert_eq!(fs::read(retained).unwrap(), original, "{attack}");
    }
}

#[test]
fn unsafe_journal_wal_and_shm_paths_are_refused_before_sqlite_uses_them() {
    for suffix in ["-journal", "-wal", "-shm"] {
        for attack in ["symlink", "hardlink", "directory", "mode"] {
            let scratch = Scratch::new();
            scratch.populated();
            let original = fs::read(scratch.database()).unwrap();
            let outside = scratch.root.path().join("outside");
            private_file(&outside, b"outside evidence");
            let sidecar = scratch.dir.join(format!("{DATABASE}{suffix}"));
            match attack {
                "symlink" => symlink(&outside, &sidecar).unwrap(),
                "hardlink" => fs::hard_link(&outside, &sidecar).unwrap(),
                "directory" => DirBuilder::new().mode(0o700).create(&sidecar).unwrap(),
                "mode" => {
                    private_file(&sidecar, b"insecure sidecar");
                    fs::set_permissions(&sidecar, fs::Permissions::from_mode(0o644)).unwrap();
                }
                _ => unreachable!(),
            }
            assert!(Store::open(&scratch.dir).is_err(), "{suffix} {attack}");
            assert_eq!(fs::read(scratch.database()).unwrap(), original);
            assert_eq!(fs::read(outside).unwrap(), b"outside evidence");
        }
    }
}

#[test]
fn even_private_wal_and_shm_sidecars_are_not_silently_converted() {
    for suffix in ["-wal", "-shm"] {
        let scratch = Scratch::new();
        scratch.populated();
        let path = scratch.dir.join(format!("{DATABASE}{suffix}"));
        private_file(&path, b"unexpected WAL state");
        assert!(Store::open(&scratch.dir).is_err());
        assert_eq!(fs::read(path).unwrap(), b"unexpected WAL state");
    }
}

#[test]
fn a_wal_database_without_sidecars_is_rejected_without_converting_it() {
    let scratch = Scratch::new();
    scratch.populated();
    scratch.sql("PRAGMA journal_mode=WAL");
    assert!(!scratch.dir.join(format!("{DATABASE}-wal")).exists());
    let original = fs::read(scratch.database()).unwrap();
    assert!(Store::open(&scratch.dir).is_err());
    assert_eq!(fs::read(scratch.database()).unwrap(), original);
}

#[test]
fn unknown_versions_and_extra_schema_objects_fail_closed() {
    for change in [
        "PRAGMA user_version=0",
        "ALTER TABLE state RENAME TO other",
        "CREATE TABLE other(value BLOB)",
        "CREATE TRIGGER changed AFTER INSERT ON state BEGIN SELECT 1; END",
        "CREATE INDEX extra ON state(snapshot)",
    ] {
        let scratch = Scratch::new();
        scratch.populated();
        scratch.sql(change);
        let original = fs::read(scratch.database()).unwrap();
        assert!(Store::open(&scratch.dir).is_err(), "{change}");
        assert_eq!(fs::read(scratch.database()).unwrap(), original, "{change}");
    }
}

/// #1680: a database a newer build wrote (`user_version` above 1) doesn't
/// stop the part. It is kept whole under a new name, never deleted, said by
/// `kept_aside`, and the owner goes on with an empty store.
#[test]
fn a_newer_database_version_is_set_aside_whole_and_a_new_store_begins() {
    let scratch = Scratch::new();
    scratch.populated();
    scratch.sql("PRAGMA user_version=2; ALTER TABLE state ADD COLUMN later BLOB");
    let original = fs::read(scratch.database()).unwrap();
    let mut store = Store::open(&scratch.dir).expect("a newer database does not stop the part");
    assert_eq!(store.load().unwrap(), None, "the new store is empty");
    let kept = store.kept_aside().unwrap().expect("the kept store is said");
    assert!(kept.starts_with("tasks.sqlite3.newer-db2-"), "{kept}");
    assert_eq!(
        fs::read(scratch.dir.join(&kept)).unwrap(),
        original,
        "kept whole"
    );
    store.commit(b"after").unwrap();
    drop(store);
    assert_eq!(
        scratch.open().load().unwrap().as_deref(),
        Some(&b"after"[..])
    );
    assert_eq!(fs::read(scratch.dir.join(&kept)).unwrap(), original);
}

/// #1680, the reviewer's P3 on #2057: a newer build's database in WAL mode,
/// with its `-wal` and `-shm` beside it, is set aside with them, every file
/// renamed byte for byte, not refused by the WAL checks; `user_version` is
/// read from the file's header before any of them.
#[test]
fn a_newer_database_in_wal_mode_is_set_aside_with_its_sidecars() {
    let scratch = Scratch::new();
    scratch.populated();
    let wal = scratch.dir.join(format!("{DATABASE}-wal"));
    let shm = scratch.dir.join(format!("{DATABASE}-shm"));
    let (wal_bytes, shm_bytes) = {
        let db = Connection::open(scratch.database()).unwrap();
        db.execute_batch("PRAGMA user_version=2; PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; UPDATE state SET snapshot=x'0102'")
            .unwrap();
        (fs::read(&wal).unwrap(), fs::read(&shm).unwrap())
    };
    // The newer build's last close checkpointed; put its sidecars back as
    // a crash would have left them.
    private_file(&wal, &wal_bytes);
    private_file(&shm, &shm_bytes);
    let original = fs::read(scratch.database()).unwrap();
    assert_eq!(&original[18..20], &[2, 2], "the database is in WAL mode");
    let mut store = Store::open(&scratch.dir).expect("a newer WAL database does not stop the part");
    assert_eq!(store.load().unwrap(), None, "the new store is empty");
    let kept = store.kept_aside().unwrap().expect("the kept store is said");
    assert!(kept.starts_with("tasks.sqlite3.newer-db2-"), "{kept}");
    for (name, bytes) in [
        (kept.clone(), &original),
        (format!("{kept}-wal"), &wal_bytes),
        (format!("{kept}-shm"), &shm_bytes),
    ] {
        assert_eq!(&fs::read(scratch.dir.join(&name)).unwrap(), bytes, "{name}");
    }
    assert!(!wal.exists() && !shm.exists());
    store.commit(b"after").unwrap();
    drop(store);
    assert_eq!(
        scratch.open().load().unwrap().as_deref(),
        Some(&b"after"[..])
    );
    assert_eq!(fs::read(scratch.dir.join(&kept)).unwrap(), original);
}

#[test]
fn invalid_singleton_and_unbounded_rows_are_not_returned_as_state() {
    for change in [
        "UPDATE state SET snapshot=x''",
        "UPDATE state SET snapshot=zeroblob(8388609)",
        "UPDATE state SET singleton=2",
        "INSERT INTO state VALUES (2,x'01')",
    ] {
        let scratch = Scratch::new();
        scratch.populated();
        scratch.sql(&format!("PRAGMA ignore_check_constraints=ON; {change}"));
        let original = fs::read(scratch.database()).unwrap();
        assert!(Store::open(&scratch.dir).is_err(), "{change}");
        assert_eq!(fs::read(scratch.database()).unwrap(), original);
    }
}

#[test]
fn load_bounds_values_even_if_an_external_writer_bypasses_checks_after_open() {
    for change in [
        "UPDATE state SET snapshot=x''",
        "UPDATE state SET snapshot=zeroblob(8388609)",
        "UPDATE state SET singleton=2",
        "INSERT INTO state VALUES (2,x'01')",
    ] {
        let scratch = Scratch::new();
        let mut store = scratch.open();
        store.commit(b"previous").unwrap();
        scratch.sql(&format!("PRAGMA ignore_check_constraints=ON; {change}"));
        assert!(store.load().is_err(), "{change}");
    }
}

#[test]
fn oversized_database_and_journal_are_refused_without_truncation() {
    for (name, length) in [
        (DATABASE, 32 * 1_048_576 + 1),
        ("tasks.sqlite3-journal", 40 * 1_048_576 + 1),
    ] {
        let scratch = Scratch::new();
        scratch.populated();
        let path = scratch.dir.join(name);
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&path)
            .unwrap();
        file.set_len(length).unwrap();
        drop(file);
        assert!(Store::open(&scratch.dir).is_err());
        assert_eq!(fs::metadata(path).unwrap().len(), length);
    }
}

#[test]
fn a_busy_failed_commit_poisoned_owner_cannot_continue_and_reopen_keeps_previous() {
    let scratch = Scratch::new();
    let mut store = scratch.open();
    store.commit(b"previous").unwrap();
    let blocking = Connection::open(scratch.database()).unwrap();
    blocking.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(store.commit(b"must not commit").is_err());
    assert!(store.poisoned());
    assert!(store.load().is_err());
    assert!(store.commit(b"must not retry").is_err());
    blocking.execute_batch("ROLLBACK").unwrap();
    drop(blocking);
    drop(store);
    assert_eq!(
        scratch.open().load().unwrap().as_deref(),
        Some(b"previous".as_slice())
    );
}

#[test]
fn changed_permissions_after_open_stop_writes_without_repairing_the_path() {
    let scratch = Scratch::new();
    let mut store = scratch.open();
    store.commit(b"previous").unwrap();
    fs::set_permissions(scratch.database(), fs::Permissions::from_mode(0o644)).unwrap();
    assert!(store.commit(b"must not commit").is_err());
    assert!(store.poisoned());
    assert_eq!(
        fs::metadata(scratch.database()).unwrap().mode() & 0o7777,
        0o644
    );
    drop(store);
    fs::set_permissions(scratch.database(), fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        scratch.open().load().unwrap().as_deref(),
        Some(b"previous".as_slice())
    );
}

#[test]
fn replacing_an_open_database_or_directory_never_redirects_the_writer() {
    for replace_directory in [false, true] {
        let scratch = Scratch::new();
        let mut store = scratch.open();
        store.commit(b"previous").unwrap();
        let retained = scratch.root.path().join("retained");
        if replace_directory {
            fs::rename(&scratch.dir, &retained).unwrap();
            DirBuilder::new().mode(0o700).create(&scratch.dir).unwrap();
        } else {
            fs::rename(scratch.database(), &retained).unwrap();
            private_file(&scratch.database(), b"replacement evidence");
        }
        assert!(store.load().is_err());
        assert!(store.commit(b"must not redirect").is_err());
        assert!(store.poisoned());
        drop(store);
        if replace_directory {
            assert!(!scratch.database().exists());
            assert_eq!(
                Store::open(&retained).unwrap().load().unwrap().as_deref(),
                Some(b"previous".as_slice())
            );
        } else {
            assert_eq!(
                fs::read(scratch.database()).unwrap(),
                b"replacement evidence"
            );
            fs::remove_file(scratch.database()).unwrap();
            fs::rename(retained, scratch.database()).unwrap();
            assert_eq!(
                scratch.open().load().unwrap().as_deref(),
                Some(b"previous".as_slice())
            );
        }
    }
}

#[test]
fn interrupted_initialization_recovers_only_an_unpublished_empty_schema() {
    for completed_schema in [false, true] {
        let scratch = Scratch::new();
        if completed_schema {
            drop(scratch.open());
            fs::rename(scratch.database(), scratch.dir.join(INITIALIZING)).unwrap();
        } else {
            DirBuilder::new().mode(0o700).create(&scratch.dir).unwrap();
            private_file(&scratch.dir.join(INITIALIZING), b"");
        }
        assert_eq!(scratch.open().load().unwrap(), None);
        assert!(scratch.database().exists());
        assert!(!scratch.dir.join(INITIALIZING).exists());
    }
}

#[test]
fn a_nonempty_initialization_file_is_preserved_for_explicit_recovery() {
    let scratch = Scratch::new();
    scratch.populated();
    let pending = scratch.dir.join(INITIALIZING);
    fs::rename(scratch.database(), &pending).unwrap();
    let original = fs::read(&pending).unwrap();
    assert!(Store::open(&scratch.dir).is_err());
    assert!(!scratch.database().exists());
    assert_eq!(fs::read(pending).unwrap(), original);
}

#[test]
fn orphan_journals_never_authorize_a_new_empty_database() {
    for name in ["tasks.sqlite3-journal", ".tasks.sqlite3.init-journal"] {
        let scratch = Scratch::new();
        DirBuilder::new().mode(0o700).create(&scratch.dir).unwrap();
        let journal = scratch.dir.join(name);
        private_file(&journal, b"recovery evidence");
        assert!(Store::open(&scratch.dir).is_err());
        assert!(!scratch.database().exists());
        assert_eq!(fs::read(journal).unwrap(), b"recovery evidence");
    }
}

#[test]
fn interrupted_transaction_recovers_the_last_committed_snapshot() {
    const CHILD: &str = "LAMP_TASKS_STORAGE_CRASH_CHILD";
    if let Some(path) = std::env::var_os(CHILD) {
        let db = Connection::open(Path::new(&path).join(DATABASE)).unwrap();
        db.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA cache_size=2; PRAGMA cache_spill=ON; BEGIN IMMEDIATE;").unwrap();
        db.execute(
            "UPDATE state SET snapshot=?1",
            params![vec![0x55_u8; MAX_STATE_BYTES]],
        )
        .unwrap();
        // Exit without running Connection/Transaction destructors. The tiny
        // page cache makes the uncommitted update spill to the database file.
        process::exit(91);
    }
    let scratch = Scratch::new();
    // Replacing a full snapshot journals the old pages, so this covers
    // recovery of an eight-MiB value rather than only database growth.
    let previous = vec![0x11; MAX_STATE_BYTES];
    scratch.open().commit(&previous).unwrap();
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "interrupted_transaction_recovers_the_last_committed_snapshot",
        ])
        .env(CHILD, &scratch.dir)
        .output()
        .unwrap();
    assert_eq!(child.status.code(), Some(91), "{child:?}");
    let journal = scratch.dir.join(format!("{DATABASE}-journal"));
    assert!(fs::metadata(&journal).unwrap().len() >= MAX_STATE_BYTES as u64);
    assert_eq!(
        scratch.open().load().unwrap().as_deref(),
        Some(previous.as_slice())
    );
    assert!(!journal.exists());
}
