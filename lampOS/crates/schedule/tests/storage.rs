//! Filesystem and SQLite durability fixtures on this test host. Crashing a
//! child process proves rollback recovery, not survival of a physical power cut.
use lamp_schedule::store::{MAX_STATE_BYTES, Store, kept_aside};
use rusqlite::Connection;
use std::{
    fs::{self, DirBuilder, OpenOptions},
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

const DATABASE: &str = "schedule.sqlite3";
const INITIALIZING: &str = ".schedule.sqlite3.init";

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
fn set_aside_keeps_the_whole_database_and_directory_ownership() {
    let scratch = Scratch::new();
    let mut store = scratch.open();
    store.commit(b"a newer snapshot").unwrap();
    let original = fs::read(scratch.database()).unwrap();
    let kept = store.set_aside("newer-2-test").unwrap();
    assert_eq!(kept, "schedule.sqlite3.newer-2-test");
    assert_eq!(fs::read(scratch.dir.join(&kept)).unwrap(), original);
    let metadata = fs::metadata(scratch.dir.join(&kept)).unwrap();
    assert_eq!(metadata.mode() & 0o7777, 0o600);
    assert_eq!(metadata.nlink(), 1);
    assert_eq!(store.load().unwrap(), None);
    assert_eq!(
        Store::open(&scratch.dir).err().unwrap().kind(),
        io::ErrorKind::WouldBlock
    );
    store.commit(b"fresh snapshot").unwrap();
    drop(store);
    assert_eq!(scratch.open().load().unwrap().unwrap(), b"fresh snapshot");
    assert_eq!(fs::read(scratch.dir.join(kept)).unwrap(), original);
}

#[test]
fn set_aside_rejects_bad_labels_and_never_replaces_an_existing_archive() {
    let scratch = Scratch::new();
    let mut store = scratch.open();
    store.commit(b"original snapshot").unwrap();
    let original = fs::read(scratch.database()).unwrap();
    for bad in ["", "../x", "x/y", "a b", &"x".repeat(65)] {
        assert_eq!(
            store.set_aside(bad).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(!store.poisoned());
        assert_eq!(fs::read(scratch.database()).unwrap(), original);
    }
    let kept = scratch.dir.join("schedule.sqlite3.newer-2-test");
    private_file(&kept, b"an archive that must remain");
    assert!(store.set_aside("newer-2-test").is_err());
    assert!(store.poisoned());
    assert!(store.load().is_err());
    assert_eq!(fs::read(&kept).unwrap(), b"an archive that must remain");
    assert_eq!(fs::read(scratch.database()).unwrap(), original);
    drop(store);
    assert_eq!(
        scratch.open().load().unwrap().unwrap(),
        b"original snapshot"
    );
}

#[test]
fn failure_after_set_aside_keeps_the_synced_archive_and_poisons_the_owner() {
    let scratch = Scratch::new();
    let mut store = scratch.open();
    store.commit(b"newer snapshot to retain").unwrap();
    let original = fs::read(scratch.database()).unwrap();
    // Inject a replacement-initialization failure after the old DB is renamed.
    private_file(&scratch.dir.join(INITIALIZING), b"not a SQLite database");
    assert!(store.set_aside("newer-2-test").is_err());
    assert!(store.poisoned());
    assert!(store.load().is_err());
    assert!(store.commit(b"must not overwrite anything").is_err());
    assert!(!scratch.database().exists());
    assert_eq!(
        fs::read(scratch.dir.join("schedule.sqlite3.newer-2-test")).unwrap(),
        original
    );
}

#[test]
fn exact_byte_bound_works_and_invalid_sizes_preserve_the_previous_value() {
    let scratch = Scratch::new();
    let mut store = scratch.open();
    let maximum = vec![0xff; MAX_STATE_BYTES];
    store.commit(&maximum).unwrap();
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
fn relative_paths_and_linked_or_public_directories_are_refused() {
    assert_eq!(
        Store::open(Path::new("relative-schedule-test"))
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
/// stop the part. It is kept whole under a new name, never deleted, found by
/// `kept_aside` (so health says it), and the owner goes on with an empty
/// store.
#[test]
fn a_newer_database_version_is_set_aside_whole_and_a_new_store_begins() {
    let scratch = Scratch::new();
    scratch.populated();
    scratch.sql("PRAGMA user_version=2; ALTER TABLE state ADD COLUMN later BLOB");
    let original = fs::read(scratch.database()).unwrap();
    let mut store = Store::open(&scratch.dir).expect("a newer database does not stop the part");
    assert_eq!(store.load().unwrap(), None, "the new store is empty");
    let kept = kept_aside(&scratch.dir)
        .unwrap()
        .expect("the kept store is said");
    assert!(kept.starts_with("schedule.sqlite3.newer-db2-"), "{kept}");
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

/// #1680, the reviewer's P3 on #2066 (the WAL order of #2057): a newer
/// build's database in WAL mode, closed or with its `-wal`/`-shm` beside it,
/// is set aside with its sidecars, not refused by this build's journal-mode
/// and sidecar checks, and its bytes are not changed.
#[test]
fn a_newer_wal_database_is_set_aside_with_its_sidecars() {
    for with_sidecars in [false, true] {
        let newer = Scratch::new();
        newer.populated();
        newer.sql("PRAGMA user_version=2");
        let live = Connection::open(newer.database()).unwrap();
        live.execute_batch("PRAGMA journal_mode=WAL; UPDATE state SET snapshot = x'01'")
            .unwrap();
        let mut files = vec![("", fs::read(newer.database()).unwrap())];
        if with_sidecars {
            for suffix in ["-wal", "-shm"] {
                let bytes = fs::read(newer.dir.join(format!("{DATABASE}{suffix}"))).unwrap();
                files.push((suffix, bytes));
            }
        } else {
            drop(live);
            files[0].1 = fs::read(newer.database()).unwrap();
        }
        let scratch = Scratch::new();
        scratch.populated();
        fs::remove_file(scratch.database()).unwrap();
        for (suffix, bytes) in &files {
            private_file(&scratch.dir.join(format!("{DATABASE}{suffix}")), bytes);
        }
        let mut store = Store::open(&scratch.dir)
            .unwrap_or_else(|error| panic!("sidecars {with_sidecars}: {error}"));
        assert_eq!(store.load().unwrap(), None, "the new store is empty");
        let kept = kept_aside(&scratch.dir)
            .unwrap()
            .expect("the kept store is said");
        assert!(kept.starts_with("schedule.sqlite3.newer-db2-"), "{kept}");
        for (suffix, bytes) in &files {
            assert_eq!(
                &fs::read(scratch.dir.join(format!("{kept}{suffix}"))).unwrap(),
                bytes,
                "{suffix} kept whole"
            );
            if !suffix.is_empty() {
                assert!(!scratch.dir.join(format!("{DATABASE}{suffix}")).exists());
            }
        }
        store.commit(b"after").unwrap();
    }
}

/// The reviewer's P3 on #2057, here too: a crash after the sidecars'
/// renames and before the database's leaves `-wal` and `-shm` at
/// `newer-db2-<s1>`. The next start keeps the database under that same
/// `<s1>`, so the WAL is beside its database again.
#[test]
fn a_set_aside_cut_before_the_database_rename_joins_its_sidecars_again() {
    let newer = Scratch::new();
    newer.populated();
    newer.sql("PRAGMA user_version=2");
    let live = Connection::open(newer.database()).unwrap();
    live.execute_batch("PRAGMA journal_mode=WAL; UPDATE state SET snapshot = x'01'")
        .unwrap();
    let mut files = vec![("", fs::read(newer.database()).unwrap())];
    for suffix in ["-wal", "-shm"] {
        files.push((
            suffix,
            fs::read(newer.dir.join(format!("{DATABASE}{suffix}"))).unwrap(),
        ));
    }
    let scratch = Scratch::new();
    scratch.populated();
    fs::remove_file(scratch.database()).unwrap();
    // The cut: the sidecars renamed under an earlier stamp, the database not.
    let kept = format!("{DATABASE}.newer-db2-1000");
    for (suffix, bytes) in &files {
        let name = if suffix.is_empty() {
            DATABASE.to_owned()
        } else {
            format!("{kept}{suffix}")
        };
        private_file(&scratch.dir.join(name), bytes);
    }
    let store = Store::open(&scratch.dir).expect("the cut does not stop the part");
    assert_eq!(
        kept_aside(&scratch.dir).unwrap().as_deref(),
        Some(kept.as_str())
    );
    for (suffix, bytes) in &files {
        assert_eq!(
            &fs::read(scratch.dir.join(format!("{kept}{suffix}"))).unwrap(),
            bytes,
            "{suffix} beside its database"
        );
    }
    drop(store);
    drop(live);
}

/// A newer build's database at version 2 in WAL mode, with its `-wal` and
/// `-shm` as a crash leaves them, or none when it closed cleanly: each
/// file's suffix and bytes.
fn newer_wal_files(sidecars: bool) -> Vec<(&'static str, Vec<u8>)> {
    let newer = Scratch::new();
    newer.populated();
    newer.sql("PRAGMA user_version=2");
    let live = Connection::open(newer.database()).unwrap();
    live.execute_batch("PRAGMA journal_mode=WAL; UPDATE state SET snapshot = x'01'")
        .unwrap();
    let mut files = vec![("", fs::read(newer.database()).unwrap())];
    if sidecars {
        for suffix in ["-wal", "-shm"] {
            let bytes = fs::read(newer.dir.join(format!("{DATABASE}{suffix}"))).unwrap();
            files.push((suffix, bytes));
        }
    } else {
        drop(live);
        files[0].1 = fs::read(newer.database()).unwrap();
    }
    files
}

/// Puts `files` in the store's folder under `name`, the database last.
fn place(scratch: &Scratch, name: &str, files: &[(&'static str, Vec<u8>)]) {
    for (suffix, bytes) in files.iter().rev() {
        private_file(&scratch.dir.join(format!("{name}{suffix}")), bytes);
    }
}

/// Sets the database's mtime, as a write at that time would.
fn written_at(path: &Path, at: SystemTime) {
    OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(at)
        .unwrap();
}

/// The reviewer's P3 on #2138: a stale `newer-db2-1000-wal`, left by an
/// older crash, beside a version-2 database written after it, with and
/// without the database's own `-wal`. The part starts, the database takes a
/// new stamp with its own sidecars, and the stale file is left as it is.
#[test]
fn a_stale_leftover_sidecar_is_not_taken_as_the_newer_databases() {
    for sidecars in [true, false] {
        let files = newer_wal_files(sidecars);
        let scratch = Scratch::new();
        scratch.populated();
        fs::remove_file(scratch.database()).unwrap();
        let stale = scratch.dir.join(format!("{DATABASE}.newer-db2-1000-wal"));
        private_file(&stale, b"an older crash's wal");
        place(&scratch, DATABASE, &files);
        written_at(
            &scratch.database(),
            SystemTime::now() + Duration::from_secs(2),
        );
        let store = Store::open(&scratch.dir)
            .unwrap_or_else(|error| panic!("own -wal {sidecars}: {error}"));
        let kept = kept_aside(&scratch.dir)
            .unwrap()
            .expect("the kept store is said");
        assert!(kept.starts_with("schedule.sqlite3.newer-db2-"), "{kept}");
        assert_ne!(
            kept, "schedule.sqlite3.newer-db2-1000",
            "own -wal {sidecars}"
        );
        for (suffix, bytes) in &files {
            assert_eq!(
                &fs::read(scratch.dir.join(format!("{kept}{suffix}"))).unwrap(),
                bytes,
                "own -wal {sidecars}: {suffix}"
            );
        }
        assert_eq!(fs::read(&stale).unwrap(), b"an older crash's wal");
        drop(store);
    }
}

/// Two flips at the same version, each with a `-wal`: the earlier
/// set-aside is complete, so the second takes its own stamp. And with a
/// clock behind (a board with no RTC), a second database closed cleanly and
/// dated before the first set-aside still takes its own stamp.
#[test]
fn each_set_aside_at_the_same_version_keeps_its_own_stamp() {
    for clock_behind in [false, true] {
        let first = newer_wal_files(true);
        let second = newer_wal_files(!clock_behind);
        let scratch = Scratch::new();
        scratch.populated();
        fs::remove_file(scratch.database()).unwrap();
        let earlier = format!("{DATABASE}.newer-db2-1000");
        place(&scratch, &earlier, &first);
        place(&scratch, DATABASE, &second);
        written_at(
            &scratch.database(),
            if clock_behind {
                SystemTime::UNIX_EPOCH + Duration::from_secs(1000)
            } else {
                SystemTime::now() + Duration::from_secs(2)
            },
        );
        let store = Store::open(&scratch.dir)
            .unwrap_or_else(|error| panic!("clock behind {clock_behind}: {error}"));
        let kept = kept_aside(&scratch.dir)
            .unwrap()
            .expect("the kept store is said");
        assert!(kept.starts_with("schedule.sqlite3.newer-db2-"), "{kept}");
        assert_ne!(kept, earlier, "clock behind {clock_behind}");
        for (name, files) in [(&earlier, &first), (&kept, &second)] {
            for (suffix, bytes) in files {
                assert_eq!(
                    &fs::read(scratch.dir.join(format!("{name}{suffix}"))).unwrap(),
                    bytes,
                    "clock behind {clock_behind}: {name}{suffix}"
                );
            }
        }
        drop(store);
    }
}

#[test]
fn invalid_singleton_and_unbounded_rows_are_not_returned_as_state() {
    for change in [
        "UPDATE state SET snapshot=x''",
        "UPDATE state SET snapshot=zeroblob(1048577)",
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
        "UPDATE state SET snapshot=zeroblob(1048577)",
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
        (DATABASE, 16 * 1_048_576 + 1),
        ("schedule.sqlite3-journal", 17 * 1_048_576 + 1),
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
    for name in ["schedule.sqlite3-journal", ".schedule.sqlite3.init-journal"] {
        let scratch = Scratch::new();
        DirBuilder::new().mode(0o700).create(&scratch.dir).unwrap();
        let journal = scratch.dir.join(name);
        private_file(&journal, b"recovery evidence");
        assert!(Store::open(&scratch.dir).is_err());
        assert!(!scratch.database().exists());
        assert_eq!(fs::read(journal).unwrap(), b"recovery evidence");
    }
}
