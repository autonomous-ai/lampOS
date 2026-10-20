//! One opaque, bounded snapshot. Tasks policy and JSON validation belong
//! to the engine. An acknowledgement requires SQLite commit and directory
//! sync. A commit error can occur after the new value became durable: discard
//! this owner and reopen/load before deciding what happened.
//!
//! The private directory and its advisory lock exclude other cooperating
//! owners. A hostile process with the same UID can still modify private files;
//! this is not a security boundary against that process.
use rusqlite::{Connection, OpenFlags, TransactionBehavior, config::DbConfig, params};
use rustix::fs::{self as unix, Mode, OFlags, RenameFlags};
use std::{
    fs::{self, File, Metadata, TryLockError},
    io,
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
    time::Duration,
};

pub const MAX_STATE_BYTES: usize = 8 * 1_048_576;
const MAX_DATABASE_BYTES: u64 = 32 * 1_048_576;
const PAGE_SIZE: u64 = 4_096;
// Rollback journals may contain database pages plus one complete snapshot.
const MAX_JOURNAL_BYTES: u64 = MAX_DATABASE_BYTES + MAX_STATE_BYTES as u64;
const DATABASE: &str = "tasks.sqlite3";
const INITIALIZING: &str = ".tasks.sqlite3.init";
const SCHEMA: &str = "CREATE TABLE state (singleton INTEGER PRIMARY KEY CHECK (singleton = 1), snapshot BLOB NOT NULL CHECK (length(snapshot) BETWEEN 1 AND 8388608)) STRICT";

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn sql(error: rusqlite::Error) -> io::Error {
    // SQLite errors contain no snapshot values for the parameterized SQL here.
    io::Error::other(format!("tasks storage: {error}"))
}

fn private(metadata: &Metadata, directory: bool) -> io::Result<()> {
    let expected_mode = if directory { 0o700 } else { 0o600 };
    if (directory && !metadata.is_dir())
        || (!directory && (!metadata.is_file() || metadata.nlink() != 1))
        || metadata.mode() & 0o7777 != expected_mode
        || metadata.uid() != rustix::process::geteuid().as_raw()
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "tasks storage must be owned by this user, unlinked, and mode 0700/0600",
        ));
    }
    Ok(())
}

fn same_file(left: &Metadata, right: &Metadata) -> bool {
    left.dev() == right.dev() && left.ino() == right.ino()
}

fn check_file(path: &Path, bound: u64) -> io::Result<Option<Metadata>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    private(&metadata, false)?;
    if metadata.len() > bound {
        return Err(invalid("tasks storage file exceeds its byte bound"));
    }
    Ok(Some(metadata))
}

fn check_sidecars(path: &Path, name: &str) -> io::Result<()> {
    // A hot rollback journal is legitimate and SQLite must recover it. Check
    // its ownership/type/size before allowing the library to open it.
    check_file(&path.join(format!("{name}-journal")), MAX_JOURNAL_BYTES)?;
    for suffix in ["-wal", "-shm"] {
        if check_file(&path.join(format!("{name}{suffix}")), MAX_DATABASE_BYTES)?.is_some() {
            return Err(invalid("tasks storage does not accept WAL sidecars"));
        }
    }
    Ok(())
}

fn directory(path: &Path) -> io::Result<(PathBuf, File)> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "tasks directory must be absolute without dot components",
        ));
    }
    let name = path
        .file_name()
        .ok_or_else(|| invalid("tasks directory needs a final component"))?;
    // Resolve ancestors, including /tmp and /var aliases on macOS. Never
    // resolve the final component: a linked store directory is refused.
    let parent_path = path
        .parent()
        .ok_or_else(|| invalid("tasks directory needs a parent"))?
        .canonicalize()?;
    let parent: File = unix::open(
        &parent_path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?
    .into();
    match unix::mkdirat(&parent, name, Mode::from_bits_retain(0o700)) {
        Ok(()) => parent.sync_all()?,
        Err(error) if error == rustix::io::Errno::EXIST => {}
        Err(error) => return Err(error.into()),
    }
    let held: File = unix::openat(
        &parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?
    .into();
    private(&held.metadata()?, true)?;
    held.try_lock().map_err(|error| match error {
        TryLockError::WouldBlock => io::Error::new(
            io::ErrorKind::WouldBlock,
            "tasks directory is already owned",
        ),
        TryLockError::Error(error) => error,
    })?;
    Ok((parent_path.join(name), held))
}

fn open_file(directory: &File, name: &str, create: bool) -> io::Result<File> {
    let mut flags = OFlags::RDWR | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
    if create {
        flags |= OFlags::CREATE | OFlags::EXCL;
    }
    let file: File = unix::openat(directory, name, flags, Mode::from_bits_retain(0o600))?.into();
    private(&file.metadata()?, false)?;
    Ok(file)
}

fn connect(path: &Path) -> io::Result<Connection> {
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(sql)?;
    db.busy_timeout(Duration::from_millis(50)).map_err(sql)?;
    db.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)
        .map_err(sql)?;
    db.set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false)
        .map_err(sql)?;
    db.execute_batch("PRAGMA synchronous=FULL; PRAGMA fullfsync=ON; PRAGMA temp_store=MEMORY; PRAGMA cache_size=-2048; PRAGMA mmap_size=0;").map_err(sql)?;
    let mode: String = db
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .map_err(sql)?;
    if mode != "delete" {
        return Err(invalid("tasks storage requires DELETE journal mode"));
    }
    Ok(db)
}

/// The database's `user_version` when it is above the one this build
/// writes: a newer build's, set aside rather than refused. Any other
/// version but 1 is refused by [`validate`].
fn newer_version(db: &Connection) -> io::Result<Option<u64>> {
    let version: u64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(sql)?;
    Ok((version > 1).then_some(version))
}

/// The `user_version` in the database file's own header (bytes 60..64) when
/// it is above 1 and no rollback journal is beside it, read without SQLite:
/// no connection, so nothing checkpoints, recovers or changes the journal
/// mode, and a WAL database or its sidecars stay byte for byte. A hot
/// journal is left to [`connect`] to recover first, then [`newer_version`].
/// A version bumped only in a `-wal` not yet checkpointed is not seen here:
/// that database is refused, as before.
fn header_newer_version(path: &Path, held: &File) -> io::Result<Option<u32>> {
    if check_file(&path.join(DATABASE), MAX_DATABASE_BYTES)?.is_none()
        || check_file(&path.join(format!("{DATABASE}-journal")), MAX_JOURNAL_BYTES)?.is_some()
    {
        return Ok(None);
    }
    let mut header = [0u8; 100];
    let file = open_file(held, DATABASE, false)?;
    if rustix::io::pread(&file, &mut header, 0)? < header.len()
        || &header[..16] != b"SQLite format 3\0"
    {
        return Ok(None);
    }
    let version = u32::from_be_bytes([header[60], header[61], header[62], header[63]]);
    Ok((version > 1 && version <= i32::MAX as u32).then_some(version))
}

/// Rename a newer build's database whole to
/// `tasks.sqlite3.newer-db<version>-<s>`, its `-wal`, `-shm` and `-journal`
/// beside it as `<kept>-wal` and so on, never deleted, with a journal line
/// naming only the file. Sidecars go first, so a crash between leaves the
/// database to be kept again on the next start, under its sidecars' stamp
/// ([`orphan_stamp`]).
fn keep_newer(path: &Path, held: &File, version: impl std::fmt::Display) -> io::Result<String> {
    let version = version.to_string();
    let since = match orphan_stamp(path, &version)? {
        Some(stamp) => stamp,
        None => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs())
            .to_string(),
    };
    let kept = format!("{DATABASE}.newer-db{version}-{since}");
    for suffix in ["-wal", "-shm", "-journal"] {
        let from = format!("{DATABASE}{suffix}");
        match unix::statat(held, from.as_str(), unix::AtFlags::SYMLINK_NOFOLLOW) {
            Ok(_) => {}
            Err(error) if error == rustix::io::Errno::NOENT => continue,
            Err(error) => return Err(error.into()),
        }
        unix::renameat_with(
            held,
            from.as_str(),
            held,
            format!("{kept}{suffix}").as_str(),
            RenameFlags::NOREPLACE,
        )?;
    }
    unix::renameat_with(held, DATABASE, held, kept.as_str(), RenameFlags::NOREPLACE)?;
    held.sync_all()?;
    eprintln!(
        "tasks: the store was written by a newer build (database version {version}, this build reads 1); it is set aside as {kept}, and this build starts with an empty store"
    );
    Ok(kept)
}

/// The `<s>` of an earlier set-aside at this `version` whose `-wal`, `-shm`
/// or `-journal` was renamed but not its database: a crash between the
/// renames. The newest such stamp, reused so the database joins its
/// sidecars again rather than taking a new one (the reviewer's P3 on #2057).
///
/// Only a leftover this database's crash could have left: the database's
/// mtime no later than the ctime of each of the stamp's sidecars (their
/// rename set it), and none of the database's own sidecars landing on a name
/// already there. A stale leftover from an older crash, or a complete
/// earlier set-aside, takes a new stamp instead (the reviewer's P3 on #2138).
fn orphan_stamp(path: &Path, version: &str) -> io::Result<Option<String>> {
    let prefix = format!("{DATABASE}.newer-db{version}-");
    let database = fs::symlink_metadata(path.join(DATABASE))?;
    let written = (database.mtime(), database.mtime_nsec());
    let mut newest: Option<(u64, String)> = None;
    'entries: for entry in fs::read_dir(path)? {
        let name = entry?.file_name();
        let Some(rest) = name.to_str().and_then(|name| name.strip_prefix(&prefix)) else {
            continue;
        };
        let Some(stamp) = ["-wal", "-shm", "-journal"]
            .iter()
            .find_map(|suffix| rest.strip_suffix(suffix))
        else {
            continue;
        };
        let Ok(at) = stamp.parse::<u64>() else {
            continue;
        };
        match fs::symlink_metadata(path.join(format!("{prefix}{stamp}"))) {
            Ok(_) => continue,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        for suffix in ["-wal", "-shm", "-journal"] {
            let leftover = match fs::symlink_metadata(path.join(format!("{prefix}{stamp}{suffix}")))
            {
                Ok(leftover) => leftover,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            if (leftover.ctime(), leftover.ctime_nsec()) < written {
                continue 'entries;
            }
            match fs::symlink_metadata(path.join(format!("{DATABASE}{suffix}"))) {
                Ok(_) => continue 'entries,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        if newest.as_ref().is_none_or(|(newest, _)| at > *newest) {
            newest = Some((at, stamp.to_owned()));
        }
    }
    Ok(newest.map(|(_, stamp)| stamp))
}

fn validate(db: &Connection) -> io::Result<()> {
    let version: u64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(sql)?;
    if version != 1 {
        return Err(invalid("unsupported tasks storage schema version"));
    }
    let schemas: Vec<(String, String, String, String)> = db
        .prepare("SELECT type, name, tbl_name, coalesce(sql, '') FROM sqlite_schema LIMIT 2")
        .map_err(sql)?
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .map_err(sql)?
        .collect::<rusqlite::Result<_>>()
        .map_err(sql)?;
    if schemas
        != [(
            "table".into(),
            "state".into(),
            "state".into(),
            SCHEMA.into(),
        )]
    {
        return Err(invalid("unexpected tasks storage schema"));
    }
    let page_size: u64 = db
        .query_row("PRAGMA page_size", [], |row| row.get(0))
        .map_err(sql)?;
    let pages: u64 = db
        .query_row("PRAGMA page_count", [], |row| row.get(0))
        .map_err(sql)?;
    if page_size != PAGE_SIZE || pages > MAX_DATABASE_BYTES / PAGE_SIZE {
        return Err(invalid("tasks database exceeds its page bound or format"));
    }
    db.pragma_update(None, "max_page_count", MAX_DATABASE_BYTES / PAGE_SIZE)
        .map_err(sql)?;
    let check: String = db
        .query_row("PRAGMA quick_check(1)", [], |row| row.get(0))
        .map_err(sql)?;
    if check != "ok" {
        return Err(invalid("tasks database integrity check failed"));
    }
    load_snapshot(db)?;
    Ok(())
}

fn load_snapshot(db: &Connection) -> io::Result<Option<Vec<u8>>> {
    // SQL bounds the returned allocation even if another writer bypassed a
    // CHECK constraint. A second row or invalid singleton is never ignored.
    let mut statement = db.prepare("SELECT singleton, CASE WHEN typeof(snapshot) = 'blob' AND length(snapshot) BETWEEN 1 AND 8388608 THEN snapshot END FROM state ORDER BY singleton LIMIT 2").map_err(sql)?;
    let mut rows = statement.query([]).map_err(sql)?;
    let Some(row) = rows.next().map_err(sql)? else {
        return Ok(None);
    };
    let id: i64 = row.get(0).map_err(sql)?;
    let bytes: Option<Vec<u8>> = row.get(1).map_err(sql)?;
    if id != 1 || bytes.is_none() || rows.next().map_err(sql)?.is_some() {
        return Err(invalid("invalid tasks singleton snapshot"));
    }
    Ok(bytes)
}

fn initialize(path: &Path, held: &File) -> io::Result<()> {
    if check_file(&path.join(format!("{DATABASE}-journal")), MAX_JOURNAL_BYTES)?.is_some() {
        return Err(invalid(
            "rollback journal exists without the tasks database",
        ));
    }
    check_sidecars(path, INITIALIZING)?;
    let existing = check_file(&path.join(INITIALIZING), MAX_DATABASE_BYTES)?.is_some();
    if !existing
        && check_file(
            &path.join(format!("{INITIALIZING}-journal")),
            MAX_JOURNAL_BYTES,
        )?
        .is_some()
    {
        return Err(invalid(
            "initialization journal exists without its database",
        ));
    }
    let file = open_file(held, INITIALIZING, !existing)?;
    let mut db = connect(&path.join(INITIALIZING))?;
    let version: u64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(sql)?;
    let tables: u64 = db
        .query_row("SELECT count(*) FROM sqlite_schema", [], |row| row.get(0))
        .map_err(sql)?;
    if version == 0 && tables == 0 {
        // A crash before the initialization transaction committed may leave
        // this reserved file empty. Only this unpublished file can initialize.
        db.pragma_update(None, "page_size", PAGE_SIZE)
            .map_err(sql)?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        tx.execute_batch(SCHEMA).map_err(sql)?;
        tx.pragma_update(None, "user_version", 1).map_err(sql)?;
        tx.commit().map_err(sql)?;
    }
    validate(&db)?;
    if load_snapshot(&db)?.is_some() {
        return Err(invalid(
            "unpublished initialization file contains a snapshot",
        ));
    }
    db.close().map_err(|(_, error)| sql(error))?;
    file.sync_all()?;
    unix::renameat_with(held, INITIALIZING, held, DATABASE, RenameFlags::NOREPLACE)?;
    held.sync_all()
}

pub struct Store {
    // Drop the SQLite connection before closing another descriptor for the
    // same inode: closing it earlier could release SQLite's POSIX locks.
    db: Connection,
    file: File,
    path: PathBuf,
    poisoned: bool,
    held: File,
}

impl Store {
    pub fn open(dir: &Path) -> io::Result<Self> {
        let (path, held) = directory(dir)?;
        // Before the sidecar and journal-mode checks: a newer build's
        // database in WAL mode, or with `-wal`/`-shm` beside it, is set
        // aside with them, not refused (#1680).
        if let Some(version) = header_newer_version(&path, &held)? {
            keep_newer(&path, &held, version)?;
        }
        check_sidecars(&path, DATABASE)?;
        if check_file(&path.join(DATABASE), MAX_DATABASE_BYTES)?.is_none() {
            initialize(&path, &held)?;
        } else if check_file(&path.join(INITIALIZING), MAX_DATABASE_BYTES)?.is_some() {
            return Err(invalid(
                "both published and initializing tasks databases exist",
            ));
        }
        let mut file = open_file(&held, DATABASE, false)?;
        if file.metadata()?.len() == 0 {
            return Err(invalid("existing tasks database is empty"));
        }
        let mut db = connect(&path.join(DATABASE))?;
        if let Some(version) = newer_version(&db)? {
            // A newer build's container: the schema has one version and no
            // history of additive changes, so nothing promises this build
            // reads it. Kept whole under a new name, never deleted, and this
            // build starts with an empty store (#1680, as #1505 does for a
            // newer `schema` inside). The tasks in it are not run, watched,
            // told or closed here; their harnesses are left as they are.
            db.close().map_err(|(_, error)| sql(error))?;
            drop(file);
            keep_newer(&path, &held, version)?;
            initialize(&path, &held)?;
            file = open_file(&held, DATABASE, false)?;
            db = connect(&path.join(DATABASE))?;
        }
        validate(&db)?;
        let store = Self {
            db,
            file,
            path,
            poisoned: false,
            held,
        };
        store.check_files()?;
        // Also covers a recovered hot journal and a completed initialization
        // rename whose previous process died before syncing the directory.
        store.file.sync_all()?;
        store.held.sync_all()?;
        Ok(store)
    }

    pub fn load(&self) -> io::Result<Option<Vec<u8>>> {
        self.check_files()?;
        load_snapshot(&self.db)
    }

    pub fn commit(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.is_empty() || bytes.len() > MAX_STATE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "snapshot must contain 1..8388608 bytes",
            ));
        }
        let result = self.write(bytes);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }

    pub fn poisoned(&self) -> bool {
        self.poisoned
    }

    /// Keep the database as it is, renamed `tasks.sqlite3.<label>` in the
    /// same owned directory, and go on with a new, empty one: a store
    /// written by a newer build is never destroyed (#1505). `label` is
    /// letters, digits and `-` only. The name it is kept under. Any
    /// failure poisons this owner, as a failed commit does.
    pub fn set_aside(&mut self, label: &str) -> io::Result<String> {
        if label.is_empty()
            || label.len() > 64
            || !label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a set-aside label is letters, digits and '-'",
            ));
        }
        let result = self.rename_and_start_empty(&format!("{DATABASE}.{label}"));
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }

    /// The newest store kept aside in the owned folder
    /// (`tasks.sqlite3.newer-<schema>-<unix seconds>`), by its name, if any is
    /// still there: names only, of this folder's own files.
    pub fn kept_aside(&self) -> io::Result<Option<String>> {
        let prefix = format!("{DATABASE}.newer-");
        let mut newest: Option<(u64, String)> = None;
        for entry in fs::read_dir(&self.path)? {
            let name = entry?.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(rest) = name.strip_prefix(&prefix) else {
                continue;
            };
            let since = rest
                .rsplit('-')
                .next()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            if newest.as_ref().is_none_or(|(at, _)| since >= *at) {
                newest = Some((since, name.to_owned()));
            }
        }
        Ok(newest.map(|(_, name)| name))
    }

    fn rename_and_start_empty(&mut self, kept: &str) -> io::Result<String> {
        self.check_files()?;
        // The connection is closed before its file is renamed, so no write
        // of its can reach the kept file; a connection to nothing stands in
        // until the new database is open.
        let open = std::mem::replace(&mut self.db, Connection::open_in_memory().map_err(sql)?);
        open.close().map_err(|(_, error)| sql(error))?;
        unix::renameat_with(
            &self.held,
            DATABASE,
            &self.held,
            kept,
            RenameFlags::NOREPLACE,
        )?;
        self.held.sync_all()?;
        initialize(&self.path, &self.held)?;
        self.file = open_file(&self.held, DATABASE, false)?;
        self.db = connect(&self.path.join(DATABASE))?;
        validate(&self.db)?;
        self.check_files()?;
        Ok(kept.to_owned())
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.check_files()?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        tx.execute("INSERT INTO state(singleton, snapshot) VALUES (1, ?1) ON CONFLICT(singleton) DO UPDATE SET snapshot=excluded.snapshot", params![bytes]).map_err(sql)?;
        tx.commit().map_err(sql)?;
        // FULL synchronizes database/journal contents. DELETE journal mode
        // additionally needs this directory sync before a durable ack.
        self.held.sync_all()?;
        self.check_files()
    }

    fn check_files(&self) -> io::Result<()> {
        if self.poisoned {
            return Err(io::Error::other(
                "tasks store needs restart after a storage error",
            ));
        }
        let directory = fs::symlink_metadata(&self.path)?;
        private(&directory, true)?;
        if !same_file(&directory, &self.held.metadata()?) {
            return Err(invalid("tasks directory changed while owned"));
        }
        let file = check_file(&self.path.join(DATABASE), MAX_DATABASE_BYTES)?
            .ok_or_else(|| invalid("tasks database disappeared while owned"))?;
        if !same_file(&file, &self.file.metadata()?) {
            return Err(invalid("tasks database changed while owned"));
        }
        check_sidecars(&self.path, DATABASE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_connection_uses_full_sync_delete_journal_and_bounded_pages() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(&root.path().join("private")).unwrap();
        for (name, expected) in [
            ("synchronous", 2),
            ("fullfsync", 1),
            ("temp_store", 2),
            ("max_page_count", 8192),
            ("page_size", 4096),
            ("cache_size", -2048),
            ("mmap_size", 0),
            ("busy_timeout", 50),
        ] {
            let actual: i64 = store
                .db
                .query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))
                .unwrap();
            assert_eq!(actual, expected, "{name}");
        }
        let journal: String = store
            .db
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal, "delete");
    }

    /// #1505: a newer build's store is kept as it was, under its new name,
    /// and the owner goes on with a new, empty one in the same directory.
    #[test]
    fn a_store_set_aside_is_kept_whole_and_a_new_one_starts_empty() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("private");
        let mut store = Store::open(&dir).unwrap();
        store.commit(b"{\"schema\":2}").unwrap();
        let kept = store.set_aside("newer-2-1760770000").unwrap();
        assert_eq!(kept, "tasks.sqlite3.newer-2-1760770000");
        assert_eq!(store.load().unwrap(), None, "the new store is empty");
        store.commit(b"{\"schema\":1}").unwrap();
        assert_eq!(
            store.load().unwrap().as_deref(),
            Some(&b"{\"schema\":1}"[..])
        );
        drop(store);
        // The kept file is a whole database with the newer snapshot in it.
        // Through the folder's own path: on macOS the tempdir is under a
        // link (`/var`), which `SQLITE_OPEN_NOFOLLOW` refuses.
        let db = connect(&dir.canonicalize().unwrap().join(&kept)).unwrap();
        validate(&db).unwrap();
        assert_eq!(
            load_snapshot(&db).unwrap().as_deref(),
            Some(&b"{\"schema\":2}"[..])
        );
        drop(db);
        // And the owner opens again on the new one.
        let store = Store::open(&dir).unwrap();
        assert_eq!(
            store.load().unwrap().as_deref(),
            Some(&b"{\"schema\":1}"[..])
        );
        let mut store = store;
        for bad in ["", "../x", "a b", "x/y"] {
            assert!(store.set_aside(bad).is_err(), "{bad:?}");
        }
        // The kept store is said until it is gone; the newest of two.
        assert_eq!(
            store.kept_aside().unwrap().as_deref(),
            Some("tasks.sqlite3.newer-2-1760770000")
        );
        store.set_aside("newer-3-1760780000").unwrap();
        assert_eq!(
            store.kept_aside().unwrap().as_deref(),
            Some("tasks.sqlite3.newer-3-1760780000")
        );
        for kept in [
            "tasks.sqlite3.newer-2-1760770000",
            "tasks.sqlite3.newer-3-1760780000",
        ] {
            fs::remove_file(dir.join(kept)).unwrap();
        }
        assert_eq!(store.kept_aside().unwrap(), None);
    }
}
