//! Opt-in memory across days. Raw records, their provenance, and the search
//! index commit together. The legacy JSONL path remains the default.
//!
//! Only this process opens the database. Model work uses a bounded snapshot
//! and comes back through a generation-checked apply; no model is called here.
use crate::{
    search,
    store::{
        self, Ask, Day, Entry, Exchange, Kept, Kind, LocalTime, Options, Person, Removed, Status,
    },
    tidy,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub const DATABASE: &str = "memory-days.sqlite3";
/// A follow-up's observation is at least this old: not the same conversation.
pub const FOLLOW_UP_AFTER_MS: u64 = 6 * 3_600_000;
/// And at most this old: a wedding three weeks ago isn't asked about.
pub const FOLLOW_UP_WITHIN_MS: u64 = 14 * 86_400_000;
/// At most one follow-up a day for a person, so open threads never nag.
pub const FOLLOW_UP_EVERY_MS: u64 = 86_400_000;
pub const NOTE_BYTES: usize = 1200;
pub const UNUSED_DAYS: u64 = 30;
pub const MAX_RECORDS: u64 = 50_000;
pub const MAX_OBSERVATIONS: u64 = 2048;
pub const MAX_SESSIONS: u64 = 32;
pub const MAX_PENDING: u64 = 8;
const DAY_MS: u64 = 86_400_000;
const MAX_IMPORT_FILE: u64 = 64 * 1024 * 1024;

/// Typed, content-free failures. SQL and model output never become log text.
#[derive(Debug)]
pub struct Error {
    pub code: &'static str,
    pub message: &'static str,
}
pub type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}
impl From<rusqlite::Error> for Error {
    fn from(_: rusqlite::Error) -> Self {
        Self::new("storage", "the memory transaction could not be completed")
    }
}
impl From<io::Error> for Error {
    fn from(_: io::Error) -> Self {
        Self::new(
            "storage",
            "the memory files could not be opened or made durable",
        )
    }
}
fn bad(message: &'static str) -> Error {
    Error::new("bad_request", message)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Record {
    pub id: u64,
    pub person: String,
    pub kind: String,
    pub at_unix_ms: u64,
    pub heard: String,
    pub said: String,
    pub text: String,
    pub category: Option<tidy::Category>,
    pub source_ids: Vec<u64>,
    pub supersedes: Option<u64>,
    pub active: bool,
}
impl Record {
    pub fn bytes(&self) -> usize {
        self.heard.len() + self.said.len() + self.text.len()
    }
    fn legacy(&self) -> Option<Entry> {
        match self.kind.as_str() {
            "exchange" => Some(Entry::Exchange(Exchange {
                id: self.id,
                at_unix_ms: self.at_unix_ms,
                heard: self.heard.clone(),
                said: self.said.clone(),
            })),
            "kept" => Some(Entry::Kept(Kept {
                id: self.id,
                at_unix_ms: self.at_unix_ms,
                text: self.text.clone(),
            })),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Note {
    pub session: String,
    pub opened_unix_ms: u64,
    pub generation: u64,
    pub note: String,
    pub source_ids: Vec<u64>,
    pub invalidated: bool,
}

/// No service thread, network client or model belongs to this store.
pub struct Days {
    db: Connection,
    dir: PathBuf,
    keep_days: u32,
    local: LocalTime,
    now: u64,
    _held: File,
}

fn private_file(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) if !m.is_file() || m.permissions().mode() & 0o077 != 0 => Err(Error::new(
            "storage",
            "memory files must be regular files with owner-only permissions",
        )),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)?
                .sync_all()?;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

impl Days {
    pub fn open(options: Options, now: u64) -> Result<Self> {
        let offset_out_of_range = matches!(
            options.local,
            LocalTime::Offset(minutes) if !(-720..=840).contains(&minutes)
        );
        if options.keep_days == 0 || offset_out_of_range {
            return Err(bad("invalid retention or UTC offset"));
        }
        store::private_dir(&options.dir)?;
        if fs::symlink_metadata(&options.dir)?.file_type().is_symlink() {
            return Err(bad("the memory directory cannot be a symlink"));
        }
        let held = File::open(&options.dir)?;
        held.try_lock().map_err(|_| {
            Error::new(
                "busy",
                "the memory directory could not be locked exclusively",
            )
        })?;
        let path = options.dir.join(DATABASE);
        private_file(&path)?;
        // SQLite inherits the database's private mode for its rollback journal.
        let db = Connection::open(&path)?;
        db.busy_timeout(std::time::Duration::from_millis(50))?;
        // EXTRA also syncs the rollback-journal unlink's directory entry;
        // FULL alone can lose the most recent commit on power failure here.
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=DELETE; PRAGMA synchronous=EXTRA; PRAGMA fullfsync=ON; PRAGMA secure_delete=ON; PRAGMA temp_store=MEMORY;")?;
        let version: u64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > 1 {
            return Err(Error::new(
                "version",
                "this memory was written by a newer version",
            ));
        }
        db.execute_batch(include_str!("days.sql"))?;
        let mut this = Self {
            db,
            dir: options.dir,
            keep_days: options.keep_days,
            local: options.local,
            now,
            _held: held,
        };
        this.import()?;
        this.clean_legacy()?;
        this.maintain(now)?;
        Ok(this)
    }

    /// The first import is one transaction. Cleanup is resumed after a crash,
    /// before a request can be served; legacy mode refuses this directory.
    fn import(&mut self) -> Result<()> {
        let complete: Option<u64> = self
            .db
            .query_row(
                "SELECT value FROM settings WHERE key='import_complete'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if complete == Some(1) {
            return Ok(());
        }
        let tx = self.db.transaction()?;
        for person in legacy_people(&self.dir)? {
            ensure_person(&tx, &person)?;
            for path in legacy_files(&self.dir.join(person.as_str()))? {
                if path.extension().is_some_and(|s| s == "tmp") {
                    continue;
                }
                let metadata = fs::symlink_metadata(&path)?;
                if !metadata.is_file() || metadata.len() > MAX_IMPORT_FILE {
                    return Err(bad("an import file is not regular or exceeds 64 MiB"));
                }
                let bytes = fs::read(&path)?;
                for line in bytes
                    .split(|b| *b == b'\n')
                    .filter(|line| !line.iter().all(u8::is_ascii_whitespace))
                {
                    let record = if path.file_name().is_some_and(|s| s == "kept.jsonl") {
                        serde_json::from_slice::<Kept>(line).ok().map(|e| Record {
                            id: e.id,
                            person: person.as_str().into(),
                            kind: "kept".into(),
                            at_unix_ms: e.at_unix_ms,
                            heard: String::new(),
                            said: String::new(),
                            text: store::tidy(&e.text, store::TEXT_BYTES),
                            category: None,
                            source_ids: vec![],
                            supersedes: None,
                            active: true,
                        })
                    } else {
                        serde_json::from_slice::<Exchange>(line)
                            .ok()
                            .map(|e| Record {
                                id: e.id,
                                person: person.as_str().into(),
                                kind: "exchange".into(),
                                at_unix_ms: e.at_unix_ms,
                                heard: store::tidy(&e.heard, store::HEARD_BYTES),
                                said: store::tidy(&e.said, store::SAID_BYTES),
                                text: String::new(),
                                category: None,
                                source_ids: vec![],
                                supersedes: None,
                                active: true,
                            })
                    };
                    match record {
                        Some(record)
                            if record.id > 0
                                && record.id < i64::MAX as u64
                                && Day::at(record.at_unix_ms, &self.local).is_some() =>
                        {
                            let count: u64 = tx.query_row(
                                "SELECT count(*) FROM entries WHERE person=?",
                                [person.as_str()],
                                |r| r.get(0),
                            )?;
                            if count >= MAX_RECORDS {
                                return Err(Error::new(
                                    "full",
                                    "the import exceeds the per-person record limit",
                                ));
                            }
                            insert_record(&tx, &record, &self.local, record.at_unix_ms)?;
                            tx.execute(
                                "UPDATE people SET next_id=max(next_id,?2) WHERE person=?1",
                                params![person.as_str(), record.id + 1],
                            )?;
                        }
                        _ => {
                            tx.execute(
                                "UPDATE people SET skipped=skipped+1 WHERE person=?",
                                [person.as_str()],
                            )?;
                        }
                    }
                }
            }
        }
        tx.execute(
            "INSERT INTO settings(key,value) VALUES('import_complete',1)",
            [],
        )?;
        tx.execute_batch("PRAGMA user_version=1;")?;
        tx.commit()?;
        File::open(&self.dir)?.sync_all()?;
        Ok(())
    }

    fn clean_legacy(&self) -> Result<()> {
        for person in legacy_people(&self.dir)? {
            let dir = self.dir.join(person.as_str());
            for path in legacy_files(&dir)? {
                fs::remove_file(path)?;
            }
            File::open(&dir)?.sync_all()?;
            // Leave unrelated files alone. They are not imported or searched.
            match fs::remove_dir(&dir) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::DirectoryNotEmpty => {}
                Err(e) => return Err(e.into()),
            }
        }
        File::open(&self.dir)?.sync_all()?;
        Ok(())
    }

    pub fn maintain(&mut self, now: u64) -> Result<u64> {
        // A backwards clock cannot revive old data or move last use backwards.
        self.now = self.now.max(now);
        // Today's local day, less the days kept, as the journal store
        // counts them: a day around a clock change is 23 or 25 hours.
        let day = Day::at(self.now, &self.local)
            .and_then(|today| today.before(self.keep_days))
            .map_or_else(|| "1970-01-01".into(), |d| d.to_string());
        let unused = self.now.saturating_sub(UNUSED_DAYS * DAY_MS);
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM doomed", [])?;
        tx.execute("INSERT INTO doomed SELECT rid FROM entries WHERE (kind='exchange' AND day<?1) OR (kind='observation' AND used_unix_ms<?2)",params![day,unused])?;
        descendants(&tx, false)?;
        let count = erase(&tx)?;
        tx.execute(
            "DELETE FROM sessions WHERE opened_unix_ms<?",
            [self.now.saturating_sub(DAY_MS)],
        )?;
        tx.execute(
            "DELETE FROM jobs WHERE at_unix_ms<?",
            [self.now.saturating_sub(DAY_MS)],
        )?;
        tx.commit()?;
        Ok(count)
    }

    pub fn append(
        &mut self,
        person: &Person,
        at: u64,
        heard: &str,
        said: &str,
        sources: &[u64],
    ) -> Result<u64> {
        self.append_ruled(person, at, heard, said, sources, &[])
    }
    /// [`Self::append`], linked also to rules of the person's own rules
    /// scope ([`Person::rules`]) that the answer may have followed, so that
    /// forgetting such a rule removes this exchange too (#813).
    pub fn append_ruled(
        &mut self,
        person: &Person,
        at: u64,
        heard: &str,
        said: &str,
        sources: &[u64],
        rules: &[u64],
    ) -> Result<u64> {
        let heard = store::tidy(heard, store::HEARD_BYTES);
        let said = store::tidy(said, store::SAID_BYTES);
        if heard.is_empty() && said.is_empty() {
            return Err(Error::new("empty", "there is no exchange to remember"));
        }
        self.add(person, at, &heard, &said, "", sources, rules)
    }
    pub fn remember(&mut self, person: &Person, at: u64, text: &str) -> Result<u64> {
        self.remember_from(person, at, text, &[])
    }

    /// [`Self::remember`], with the exchanges of the conversation it came up
    /// in (`from`, the person's own, at most 64): forgetting it then removes
    /// them too (#1693, `LAMP_MEMORY_FORGET_ALL`).
    pub fn remember_from(
        &mut self,
        person: &Person,
        at: u64,
        text: &str,
        from: &[u64],
    ) -> Result<u64> {
        for id in from {
            let exchange: bool = self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM entries WHERE person=? AND id=? AND kind='exchange')",
                params![person.as_str(), id],
                |r| r.get(0),
            )?;
            if !exchange {
                return Err(bad(
                    "a kept thing comes only from the person's own exchanges",
                ));
            }
        }
        let text = store::tidy(text, store::TEXT_BYTES);
        if text.is_empty() {
            return Err(Error::new("empty", "there is no text to remember"));
        }
        let count: u64 = self.db.query_row(
            "SELECT count(*) FROM entries WHERE person=? AND kind='kept'",
            [person.as_str()],
            |r| r.get(0),
        )?;
        if count >= store::KEPT_ENTRIES as u64 {
            return Err(Error::new(
                "full",
                "the kept list is full; nothing was removed",
            ));
        }
        self.add(person, at, "", "", &text, from, &[])
    }
    // The one private writer of an exchange or a kept entry: each argument
    // is a column or its links.
    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        person: &Person,
        at: u64,
        heard: &str,
        said: &str,
        text: &str,
        sources: &[u64],
        rules: &[u64],
    ) -> Result<u64> {
        if Day::at(at, &self.local).is_none() {
            return Err(Error::new(
                "bad_time",
                "the timestamp is outside 1970 through 9999",
            ));
        }
        let tx = self.db.transaction()?;
        ensure_person(&tx, person)?;
        validate_sources(&tx, person, sources, false)?;
        // Rules are the person's own, in the scope beside theirs, and only
        // kept entries: another person's are never linked.
        let ruled = match rules {
            [] => None,
            _ => {
                let scope = person
                    .rules()
                    .ok_or_else(|| bad("this person has no rules scope"))?;
                validate_sources(&tx, &scope, rules, false)?;
                for id in rules {
                    let kept: bool = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM entries WHERE person=? AND id=? AND kind='kept')",
                        params![scope.as_str(), id],
                        |r| r.get(0),
                    )?;
                    if !kept {
                        return Err(bad("a rule ID is not a kept rule"));
                    }
                }
                Some(scope)
            }
        };
        let id = next_id(&tx, person)?;
        let record = Record {
            id,
            person: person.as_str().into(),
            kind: if text.is_empty() { "exchange" } else { "kept" }.into(),
            at_unix_ms: at,
            heard: heard.into(),
            said: said.into(),
            text: text.into(),
            category: None,
            source_ids: sources.to_vec(),
            supersedes: None,
            active: true,
        };
        insert_record(&tx, &record, &self.local, self.now)?;
        if let Some(scope) = ruled {
            let rid: i64 = tx.query_row(
                "SELECT rid FROM entries WHERE person=? AND id=?",
                params![person.as_str(), id],
                |r| r.get(0),
            )?;
            for rule in rules {
                tx.execute(
                    "INSERT INTO sources(child,parent) SELECT ?1,rid FROM entries WHERE person=?2 AND id=?3",
                    params![rid, scope.as_str(), rule],
                )?;
            }
        }
        tx.commit()?;
        Ok(id)
    }

    pub fn kept(&self, person: &Person, max_bytes: usize) -> Result<Vec<Kept>> {
        let mut statement=self.db.prepare("SELECT id,at_unix_ms,text FROM entries WHERE person=? AND kind='kept' ORDER BY id DESC")?;
        let rows = statement.query_map([person.as_str()], |r| {
            Ok(Kept {
                id: r.get(0)?,
                at_unix_ms: r.get(1)?,
                text: r.get(2)?,
            })
        })?;
        let mut entries = vec![];
        let mut room = max_bytes.min(16384);
        for row in rows {
            let row = row?;
            if row.text.len() > room {
                break;
            }
            room -= row.text.len();
            entries.push(row);
        }
        entries.reverse();
        Ok(entries)
    }

    pub fn recall(&self, person: &Person, ask: &Ask<'_>) -> Result<Vec<Entry>> {
        let mut entries: Vec<Entry> = self
            .find(person, ask, false, false)?
            .into_iter()
            .filter_map(|r| r.legacy())
            .collect();
        entries.sort_by_key(|r| (r.at_unix_ms(), r.id()));
        Ok(entries)
    }

    /// Ranked data, never an answer. No overlap means no memory. A caller must
    /// still judge whether any returned record actually answers its question.
    pub fn search(&mut self, person: &Person, ask: &Ask<'_>, history: bool) -> Result<Vec<Record>> {
        if ask.query.is_none_or(|query| search::query(query).is_none()) {
            return Ok(Vec::new());
        }
        let records = self.find(person, ask, true, history)?;
        let tx = self.db.transaction()?;
        for record in &records {
            if record.kind == "observation" {
                tx.execute("UPDATE entries SET used_unix_ms=max(used_unix_ms,?3) WHERE person=?1 AND id=?2",params![person.as_str(),record.id,self.now])?;
            }
        }
        tx.commit()?;
        Ok(records)
    }
    fn find(
        &self,
        person: &Person,
        ask: &Ask<'_>,
        observations: bool,
        history: bool,
    ) -> Result<Vec<Record>> {
        let query = ask.query.filter(|q| !q.trim().is_empty());
        let expression = query.and_then(search::query);
        if query.is_some() && expression.is_none() {
            return Ok(vec![]);
        }
        let day = ask.day.map(|d| d.to_string());
        let sql = if expression.is_some() {
            "SELECT e.id FROM search JOIN entries e ON e.rid=search.rowid WHERE search MATCH ?1 AND e.person=?2 AND (?3 IS NULL OR (e.kind='exchange' AND e.day=?3)) AND (?4 OR e.kind!='observation') AND (?5 OR e.active=1) ORDER BY search.rank,e.at_unix_ms DESC,e.id DESC LIMIT ?6"
        } else {
            "SELECT e.id FROM entries e WHERE ?1 IS NULL AND e.person=?2 AND (?3 IS NULL OR e.day=?3) AND ((?4 AND e.kind!='kept') OR e.kind='exchange') AND (?5 OR e.active=1) ORDER BY e.at_unix_ms DESC,e.id DESC LIMIT ?6"
        };
        let mut statement = self.db.prepare(sql)?;
        let ids = statement
            .query_map(
                params![
                    expression,
                    person.as_str(),
                    day,
                    observations,
                    history,
                    ask.limit.min(50)
                ],
                |r| r.get::<_, u64>(0),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut result = vec![];
        let mut room = ask.max_bytes.min(16384);
        for id in ids {
            let entry = self
                .record(person, id)?
                .ok_or_else(|| bad("a search record is missing"))?;
            if entry.bytes() > room {
                break;
            }
            room -= entry.bytes();
            result.push(entry);
        }
        Ok(result)
    }

    pub fn record(&self, person: &Person, id: u64) -> Result<Option<Record>> {
        read_record(&self.db, person, id)
    }

    pub fn forget(&mut self, person: &Person, id: u64) -> Result<u64> {
        let Some(record) = self.record(person, id)? else {
            return Ok(0);
        };
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM doomed", [])?;
        tx.execute(
            "INSERT INTO doomed SELECT rid FROM entries WHERE person=? AND id=?",
            params![person.as_str(), id],
        )?;
        versions(&tx)?;
        // Forgetting an extracted fact also removes its evidence, otherwise a
        // later tidy pass could simply extract the same fact again; and a
        // kept thing goes with the conversation it came up in (#1693,
        // `remember_from`). This can remove an entire exchange containing
        // more than one subject. One step back, the person's own exchanges
        // only: an exchange's own sources are what its session was shown,
        // not where the fact came from, so climbing them removed kept things
        // that were only shown (#2003). A rule the exchange followed, in the
        // scope beside theirs, is not evidence either.
        if record.kind == "observation" || record.kind == "kept" {
            tx.execute("INSERT OR IGNORE INTO doomed SELECT s.parent FROM sources s JOIN doomed d ON s.child=d.rid JOIN entries e ON e.rid=s.parent WHERE e.person=?1 AND e.kind='exchange'",[person.as_str()])?;
        }
        descendants(&tx, true)?;
        let count = erase(&tx)?;
        tx.commit()?;
        Ok(count)
    }
    pub fn forget_last(&mut self, person: &Person, only: Option<Kind>) -> Result<Option<Removed>> {
        let kind = only.map(|k| match k {
            Kind::Kept => "kept",
            Kind::Exchange => "exchange",
        });
        let id:Option<u64>=self.db.query_row("SELECT id FROM entries WHERE person=?1 AND kind!='observation' AND (?2 IS NULL OR kind=?2) ORDER BY id DESC LIMIT 1",params![person.as_str(),kind],|r|r.get(0)).optional()?;
        let Some(id) = id else {
            return Ok(None);
        };
        let record = self
            .record(person, id)?
            .ok_or_else(|| bad("the last record is missing"))?;
        self.forget(person, id)?;
        Ok(Some(Removed {
            kind: if record.kind == "kept" {
                Kind::Kept
            } else {
                Kind::Exchange
            },
            id,
            at_unix_ms: record.at_unix_ms,
        }))
    }
    pub fn forget_all(&mut self, person: Option<&Person>) -> Result<u64> {
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM doomed", [])?;
        tx.execute(
            "INSERT INTO doomed SELECT rid FROM entries WHERE ?1 IS NULL OR person=?1",
            [person.map(Person::as_str)],
        )?;
        let count = erase(&tx)?;
        // Empty people also have job and snapshot metadata, but keep the high
        // water mark so a stale external reference can never name a new entry.
        tx.execute(
            "UPDATE sessions SET note='',ids='[]',invalidated=1 WHERE ?1 IS NULL OR person=?1",
            [person.map(Person::as_str)],
        )?;
        tx.execute(
            "DELETE FROM jobs WHERE ?1 IS NULL OR person=?1",
            [person.map(Person::as_str)],
        )?;
        tx.execute(
            "UPDATE people SET skipped=0 WHERE ?1 IS NULL OR person=?1",
            [person.map(Person::as_str)],
        )?;
        tx.commit()?;
        Ok(count)
    }

    /// Enable connection-local counters for health reports. Their table and
    /// triggers are temporary; the durable schema and records are unchanged.
    /// Repeated calls do nothing, and reopening the store disables them.
    pub fn enable_health_counts(&mut self) -> Result<()> {
        crate::health_counts::enable(&mut self.db)?;
        Ok(())
    }

    /// Constant-time counts on this connection, after `enable_health_counts`.
    /// Call between requests so the snapshot describes committed records.
    pub fn health_counts(&self) -> Result<crate::health_counts::Counts> {
        Ok(crate::health_counts::sqlite(&self.db)?)
    }

    pub fn status(&self, person: Option<&Person>) -> Result<Vec<Status>> {
        let mut statement=self.db.prepare("SELECT p.person,sum(CASE WHEN e.kind='exchange' THEN 1 ELSE 0 END),sum(CASE WHEN e.kind='kept' THEN 1 ELSE 0 END),coalesce(sum(length(CAST(e.heard AS BLOB))+length(CAST(e.said AS BLOB))+length(CAST(e.text AS BLOB))),0),min(CASE WHEN e.kind='exchange' THEN e.day END),max(CASE WHEN e.kind='exchange' THEN e.day END),p.skipped FROM people p LEFT JOIN entries e ON e.person=p.person WHERE ?1 IS NULL OR p.person=?1 GROUP BY p.person ORDER BY p.person")?;
        Ok(statement
            .query_map([person.map(Person::as_str)], |r| {
                Ok(Status {
                    person: r.get(0)?,
                    exchanges: r.get(1)?,
                    kept: r.get(2)?,
                    bytes: r.get(3)?,
                    oldest_day: r.get(4)?,
                    newest_day: r.get(5)?,
                    skipped_lines: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn session_open(
        &mut self,
        person: &Person,
        session: &str,
        max_bytes: usize,
    ) -> Result<Note> {
        identifier(session)?;
        let cap = max_bytes.min(NOTE_BYTES);
        if let Some((note, previous_cap)) = self.note(person, session)? {
            if cap != previous_cap {
                return Err(bad("a session's byte cap cannot change after it opens"));
            }
            return Ok(note);
        }
        ensure_person(&self.db, person)?;
        let count: u64 = self.db.query_row(
            "SELECT count(*) FROM sessions WHERE person=?",
            [person.as_str()],
            |r| r.get(0),
        )?;
        if count >= MAX_SESSIONS {
            return Err(Error::new(
                "full",
                "close an old memory session before opening another",
            ));
        }
        let mut ids = vec![];
        let mut facts = vec![];
        let scope = if person.as_str() == Person::EVERYONE {
            "said in this room; speaker unknown"
        } else {
            "caller-supplied person; identity not verified"
        };
        let render = |facts: &Vec<serde_json::Value>| {
            serde_json::json!({"scope":scope,"untrusted":true,"facts":facts}).to_string()
        };
        let mut text = render(&facts);
        if text.len() > cap {
            text.clear();
        }
        let candidates:Vec<u64>=self.db.prepare("SELECT id FROM entries WHERE person=? AND active=1 AND kind IN ('kept','observation') ORDER BY CASE category WHEN 'promise' THEN 0 WHEN 'thread' THEN 1 ELSE 2 END,at_unix_ms DESC,id DESC LIMIT 256")?.query_map([person.as_str()],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
        for id in candidates {
            let record = self
                .record(person, id)?
                .ok_or_else(|| bad("a note record is missing"))?;
            let category = record.category.as_ref().map_or("preference", category_name);
            facts.push(serde_json::json!({"id":id,"kind":category,"text":record.text}));
            let next = render(&facts);
            if next.len() > cap {
                facts.pop();
                continue;
            }
            text = next;
            ids.push(id);
        }
        let note = Note {
            session: session.into(),
            opened_unix_ms: self.now,
            generation: generation(&self.db, person)?,
            note: text,
            source_ids: ids,
            invalidated: false,
        };
        self.db.execute("INSERT INTO sessions(person,session,opened_unix_ms,generation,max_bytes,note,ids) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![person.as_str(),session,self.now,note.generation,cap,note.note,serde_json::to_string(&note.source_ids).map_err(|_|bad("note IDs could not be encoded"))?])?;
        Ok(note)
    }
    fn note(&self, person: &Person, session: &str) -> Result<Option<(Note, usize)>> {
        let found=self.db.query_row("SELECT opened_unix_ms,generation,note,ids,invalidated,max_bytes FROM sessions WHERE person=? AND session=?",params![person.as_str(),session],|r|Ok((r.get::<_,u64>(0)?,r.get::<_,u64>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,bool>(4)?,r.get::<_,usize>(5)?))).optional()?;
        found
            .map(
                |(opened_unix_ms, generation, note, ids, invalidated, cap)| {
                    Ok((
                        Note {
                            session: session.into(),
                            opened_unix_ms,
                            generation,
                            note,
                            source_ids: serde_json::from_str(&ids)
                                .map_err(|_| bad("stored note IDs are invalid"))?,
                            invalidated,
                        },
                        cap,
                    ))
                },
            )
            .transpose()
    }
    /// At most one open `thread` or `promise` observation of a known
    /// person's to ask about once, unprompted, at a new conversation's start
    /// (#1693 (3)): at least [`FOLLOW_UP_AFTER_MS`] old, so not the same
    /// conversation, and at most [`FOLLOW_UP_WITHIN_MS`], newest first, never
    /// one given before, and none when one was given to them within
    /// [`FOLLOW_UP_EVERY_MS`]. The one given is recorded as given in the
    /// same transaction: once, even if the moment never comes. The room's
    /// scope and a rules scope have none: a lamp that can't tell who is
    /// there never asks one person about another's week.
    pub fn follow_up(&mut self, person: &Person) -> Result<Option<serde_json::Value>> {
        if person.as_str() == Person::EVERYONE || person.as_str().starts_with("r-") {
            return Err(Error::new(
                "bad_person",
                "a follow-up is for someone known, never the room's",
            ));
        }
        let now = self.now;
        let tx = self.db.transaction()?;
        let lately: Option<u64> = tx.query_row(
            "SELECT max(asked_unix_ms) FROM follow_ups WHERE person=?",
            [person.as_str()],
            |r| r.get(0),
        )?;
        if lately.is_some_and(|at| now.saturating_sub(at) < FOLLOW_UP_EVERY_MS) {
            return Ok(None);
        }
        let found: Option<(i64, u64, String, String)> = tx
            .query_row(
                "SELECT e.rid,e.id,e.text,e.category FROM entries e WHERE e.person=?1 AND e.kind='observation' AND e.active=1 AND e.category IN ('thread','promise') AND e.at_unix_ms<=?2 AND e.at_unix_ms>=?3 AND NOT EXISTS (SELECT 1 FROM follow_ups f WHERE f.rid=e.rid) ORDER BY e.at_unix_ms DESC,e.id DESC LIMIT 1",
                params![
                    person.as_str(),
                    now.saturating_sub(FOLLOW_UP_AFTER_MS),
                    now.saturating_sub(FOLLOW_UP_WITHIN_MS)
                ],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let Some((rid, id, text, category)) = found else {
            return Ok(None);
        };
        tx.execute(
            "INSERT INTO follow_ups(rid,person,asked_unix_ms) VALUES(?1,?2,?3)",
            params![rid, person.as_str(), now],
        )?;
        tx.commit()?;
        Ok(Some(
            serde_json::json!({"id":id,"category":category,"text":text}),
        ))
    }

    pub fn session_close(&mut self, person: &Person, session: &str) -> Result<()> {
        identifier(session)?;
        self.db.execute(
            "DELETE FROM sessions WHERE person=? AND session=?",
            params![person.as_str(), session],
        )?;
        Ok(())
    }

    /// Prepare immutable data only after the caller says the conversation has
    /// ended. Inference is a separate caller's job, never a socket operation.
    pub fn conversation_end(
        &mut self,
        person: &Person,
        conversation: &str,
        source_ids: &[u64],
    ) -> Result<tidy::Input> {
        self.conversation_end_with(person, conversation, source_ids, &[])
    }

    /// Check a handoff without preparing model input. There is currently no
    /// model adapter, so the protocol reports a successful skip and retains no
    /// job. Only bounded IDs and scalar record metadata are read here; neither
    /// remembered text, related observations nor existing job payloads are read.
    pub(crate) fn validate_conversation_submission(
        &self,
        person: &Person,
        conversation: &str,
        source_ids: &[u64],
        previous_ids: &[u64],
    ) -> Result<()> {
        identifier(conversation)?;
        if previous_ids.len() > 32
            || previous_ids.iter().collect::<BTreeSet<_>>().len() != previous_ids.len()
        {
            return Err(bad(
                "at most 32 distinct previous observation IDs may be selected",
            ));
        }
        if source_ids
            .iter()
            .chain(previous_ids)
            .any(|id| *id == 0 || *id > i64::MAX as u64)
        {
            return Err(bad("record IDs must be positive SQLite integers"));
        }
        validate_sources(&self.db, person, source_ids, true)?;
        for id in source_ids {
            let valid: bool = self.db.query_row(
                "SELECT kind='exchange' AND at_unix_ms<=?3 FROM entries WHERE person=?1 AND id=?2",
                params![person.as_str(), id, self.now],
                |row| row.get(0),
            )?;
            if !valid {
                return Err(bad(
                    "tidy sources must be journal exchanges that have already happened",
                ));
            }
        }
        for id in previous_ids {
            let valid: bool = self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM entries WHERE person=?1 AND id=?2 AND kind='observation' AND active=1)",
                params![person.as_str(), id],
                |row| row.get(0),
            )?;
            if !valid {
                return Err(bad("only current observations can be selected as previous"));
            }
        }
        Ok(())
    }

    pub fn conversation_end_with(
        &mut self,
        person: &Person,
        conversation: &str,
        source_ids: &[u64],
        previous_ids: &[u64],
    ) -> Result<tidy::Input> {
        identifier(conversation)?;
        if previous_ids.len() > 32
            || previous_ids.iter().collect::<BTreeSet<_>>().len() != previous_ids.len()
        {
            return Err(bad(
                "at most 32 distinct previous observation IDs may be selected",
            ));
        }
        let existing: Option<(String, String)> = self
            .db
            .query_row(
                "SELECT state,input FROM jobs WHERE person=? AND conversation=?",
                params![person.as_str(), conversation],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((state, input)) = existing {
            if state == "done" {
                return Err(Error::new("done", "this conversation was already tidied"));
            }
            let input: tidy::Input =
                serde_json::from_str(&input).map_err(|_| bad("stored tidy input is invalid"))?;
            if input.source_ids != source_ids {
                return Err(bad(
                    "a conversation's sources cannot change after preparation",
                ));
            }
            if previous_ids
                .iter()
                .any(|id| !input.previous.iter().any(|old| old.id == *id))
            {
                return Err(bad(
                    "a prepared conversation's previous observations cannot change",
                ));
            }
            return Ok(input);
        }
        validate_sources(&self.db, person, source_ids, true)?;
        let pending: u64 = self.db.query_row(
            "SELECT count(*) FROM jobs WHERE person=? AND state='pending'",
            [person.as_str()],
            |r| r.get(0),
        )?;
        if pending >= MAX_PENDING {
            return Err(Error::new("busy", "the bounded tidy queue is full"));
        }
        let mut records = vec![];
        for id in source_ids {
            let r = self
                .record(person, *id)?
                .ok_or_else(|| bad("a tidy source is missing"))?;
            if r.kind != "exchange" || r.at_unix_ms > self.now {
                return Err(bad(
                    "tidy sources must be journal exchanges that have already happened",
                ));
            }
            records.push(tidy::Evidence {
                id: r.id,
                at_unix_ms: r.at_unix_ms,
                kind: r.kind,
                heard: r.heard,
                said: r.said,
                text: r.text,
            });
        }
        let mut ids = previous_ids.to_vec();
        for id in previous_ids {
            let prior = self
                .record(person, *id)?
                .ok_or_else(|| bad("a selected previous observation is missing"))?;
            if prior.kind != "observation" || !prior.active {
                return Err(bad("only current observations can be selected as previous"));
            }
        }
        // Prefer context relevant to the ended conversation, so an older
        // preference remains correctable after many unrelated observations.
        // Explicit IDs let a caller include a known correction even when its
        // wording shares no searchable term with the original.
        let mut relevant = std::collections::BTreeMap::<u64, f64>::new();
        for record in &records {
            if let Some(query) = search::query(&format!("{} {}", record.heard, record.said)) {
                let mut statement = self.db.prepare("SELECT e.id,search.rank FROM search JOIN entries e ON e.rid=search.rowid WHERE search MATCH ?1 AND e.person=?2 AND e.kind='observation' AND e.active=1 ORDER BY search.rank,e.id DESC LIMIT 32")?;
                for row in statement.query_map(params![query, person.as_str()], |r| {
                    Ok((r.get::<_, u64>(0)?, r.get::<_, f64>(1)?))
                })? {
                    let (id, rank) = row?;
                    relevant
                        .entry(id)
                        .and_modify(|old| *old = old.min(rank))
                        .or_insert(rank);
                }
            }
        }
        let mut relevant: Vec<_> = relevant.into_iter().collect();
        relevant.sort_by(|(a, ar), (b, br)| ar.total_cmp(br).then_with(|| b.cmp(a)));
        for (id, _) in relevant {
            if ids.len() < 32 && !ids.contains(&id) {
                ids.push(id);
            }
        }
        let recent:Vec<u64>=self.db.prepare("SELECT id FROM entries WHERE person=? AND kind='observation' AND active=1 ORDER BY id DESC LIMIT 32")?.query_map([person.as_str()],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
        for id in recent {
            if ids.len() < 32 && !ids.contains(&id) {
                ids.push(id);
            }
        }
        let mut previous = vec![];
        for id in ids {
            let r = self
                .record(person, id)?
                .ok_or_else(|| bad("a previous observation is missing"))?;
            previous.push(tidy::Previous {
                id: r.id,
                at_unix_ms: r.at_unix_ms,
                text: r.text,
                category: r
                    .category
                    .ok_or_else(|| bad("an observation has no category"))?,
            });
        }
        let tx = self.db.transaction()?;
        let generation = generation(&tx, person)?;
        tx.execute("INSERT INTO jobs(person,conversation,generation,at_unix_ms,state) VALUES(?,?,?,?,'pending')",params![person.as_str(),conversation,generation,self.now])?;
        let job = tx.last_insert_rowid();
        let input = tidy::Input {
            job: format!("j{job}"),
            person: person.as_str().into(),
            generation,
            source_ids: source_ids.to_vec(),
            records,
            previous,
        };
        tidy::validate_input(&input)
            .map_err(|_| bad("the tidy input exceeds its limits or is invalid"))?;
        let encoded =
            serde_json::to_string(&input).map_err(|_| bad("tidy input could not be encoded"))?;
        tx.execute("UPDATE jobs SET input=? WHERE job=?", params![encoded, job])?;
        tx.commit()?;
        Ok(input)
    }

    /// A generation check and source validation are repeated in the same
    /// transaction as insertion. A result computed before forgetting can never
    /// repopulate this store. Ordinary appends do not cancel useful work.
    pub fn tidy_apply(
        &mut self,
        person: &Person,
        job: &str,
        output: &tidy::Output,
    ) -> Result<Vec<u64>> {
        let job = job
            .strip_prefix('j')
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|n| *n > 0 && *n < i64::MAX as u64)
            .ok_or_else(|| bad("invalid tidy job ID"))?;
        let tx = self.db.transaction()?;
        let saved: Option<(String, String, u64, String)> = tx
            .query_row(
                "SELECT state,input,generation,result FROM jobs WHERE job=? AND person=?",
                params![job, person.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let Some((state, input, prepared, result)) = saved else {
            return Err(Error::new(
                "stale",
                "the tidy job is gone or belongs to another person",
            ));
        };
        if state == "done" {
            return serde_json::from_str(&result).map_err(|_| bad("stored tidy result is invalid"));
        }
        if prepared != generation(&tx, person)? {
            return Err(Error::new(
                "stale",
                "memory changed while the model was working",
            ));
        }
        // Nothing worth keeping, or a pass that failed (#1693 (2)): the job
        // is done with nothing kept, so it neither waits out its day nor
        // counts towards `busy`. A retry finds it done, as for any result.
        if output.observations.is_empty() {
            tx.execute(
                "UPDATE jobs SET state='done',input='',result='[]' WHERE job=?",
                [job],
            )?;
            tx.commit()?;
            return Ok(vec![]);
        }
        let input: tidy::Input =
            serde_json::from_str(&input).map_err(|_| bad("stored tidy input is invalid"))?;
        tidy::validate_output(&input, output).map_err(|_| {
            bad("the model result is empty, exceeds its limits, or has invalid provenance")
        })?;
        validate_sources(&tx, person, &input.source_ids, true)?;
        let count: u64 = tx.query_row(
            "SELECT count(*) FROM entries WHERE person=? AND kind='observation'",
            [person.as_str()],
            |r| r.get(0),
        )?;
        if count + output.observations.len() as u64 > MAX_OBSERVATIONS {
            return Err(Error::new("full", "the observation limit is reached"));
        }
        let mut ids = vec![];
        for proposal in &output.observations {
            // Date the fact by its evidence, not the time a delayed model
            // happened to finish. Old conversations cannot overrule new ones.
            let at = input
                .records
                .iter()
                .filter(|record| proposal.source_ids.contains(&record.id))
                .map(|record| record.at_unix_ms)
                .max()
                .ok_or_else(|| bad("an observation needs dated evidence"))?;
            if let Some(old) = proposal.supersedes {
                let old = read_record(&tx, person, old)?
                    .ok_or_else(|| bad("the superseded observation is missing"))?;
                if old.kind != "observation"
                    || !old.active
                    || old.at_unix_ms > at
                    || old.category.as_ref() != Some(&proposal.category)
                {
                    return Err(bad(
                        "only a current older observation of the same category can be superseded",
                    ));
                }
            }
            let id = next_id(&tx, person)?;
            insert_record(
                &tx,
                &Record {
                    id,
                    person: person.as_str().into(),
                    kind: "observation".into(),
                    at_unix_ms: at,
                    heard: String::new(),
                    said: String::new(),
                    text: proposal.text.clone(),
                    category: Some(proposal.category),
                    source_ids: proposal.source_ids.clone(),
                    supersedes: proposal.supersedes,
                    active: true,
                },
                &self.local,
                self.now,
            )?;
            ids.push(id);
        }
        tx.execute(
            "UPDATE jobs SET state='done',input='',result=? WHERE job=?",
            params![
                serde_json::to_string(&ids).map_err(|_| bad("tidy result could not be encoded"))?,
                job
            ],
        )?;
        // Retain a bounded day of completion receipts for idempotent retries.
        tx.execute("DELETE FROM jobs WHERE person=?1 AND state='done' AND job NOT IN (SELECT job FROM jobs WHERE person=?1 AND state='done' ORDER BY job DESC LIMIT 32)",[person.as_str()])?;
        tx.commit()?;
        Ok(ids)
    }
}

fn identifier(value: &str) -> Result<()> {
    if (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    {
        Ok(())
    } else {
        Err(bad("an identifier is 1 to 64 of a-z, 0-9, _ and -"))
    }
}

fn legacy_people(dir: &Path) -> Result<Vec<Person>> {
    let mut people = vec![];
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if let Some(person) = entry.file_name().to_str().and_then(Person::new) {
            let meta = entry.file_type()?;
            if meta.is_symlink() {
                return Err(bad("a person directory cannot be a symlink"));
            }
            if meta.is_dir() {
                store::private_dir(&entry.path())?;
                people.push(person);
            }
        }
    }
    people.sort();
    Ok(people)
}
fn legacy_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = vec![];
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let base = name.strip_suffix(".tmp").unwrap_or(&name);
        if base == "kept.jsonl"
            || base
                .strip_prefix("journal-")
                .and_then(|s| s.strip_suffix(".jsonl"))
                .and_then(Day::parse)
                .is_some()
        {
            if !entry.file_type()?.is_file() {
                return Err(bad("an import file cannot be a symlink or directory"));
            }
            files.push(entry.path());
        }
    }
    files.sort();
    Ok(files)
}
fn ensure_person(db: &Connection, person: &Person) -> Result<()> {
    db.execute(
        "INSERT OR IGNORE INTO people(person) VALUES(?)",
        [person.as_str()],
    )?;
    Ok(())
}
fn next_id(db: &Connection, person: &Person) -> Result<u64> {
    let count: u64 = db.query_row(
        "SELECT count(*) FROM entries WHERE person=?",
        [person.as_str()],
        |r| r.get(0),
    )?;
    if count >= MAX_RECORDS {
        return Err(Error::new("full", "the per-person record limit is reached"));
    }
    let id: u64 = db.query_row(
        "SELECT next_id FROM people WHERE person=?",
        [person.as_str()],
        |r| r.get(0),
    )?;
    if id >= i64::MAX as u64 {
        return Err(Error::new("full", "the person's record IDs are exhausted"));
    }
    db.execute(
        "UPDATE people SET next_id=next_id+1 WHERE person=?",
        [person.as_str()],
    )?;
    Ok(id)
}
fn generation(db: &Connection, person: &Person) -> Result<u64> {
    Ok(db.query_row(
        "SELECT generation FROM people WHERE person=?",
        [person.as_str()],
        |r| r.get(0),
    )?)
}
fn validate_sources(db: &Connection, person: &Person, ids: &[u64], required: bool) -> Result<()> {
    if ids.len() > 64
        || (required && ids.is_empty())
        || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
    {
        return Err(bad("source IDs must be unique, with at most 64"));
    }
    for id in ids {
        let exists: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM entries WHERE person=? AND id=?)",
            params![person.as_str(), id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(bad("a source ID does not exist for this person"));
        }
    }
    Ok(())
}
fn insert_record(db: &Connection, r: &Record, local: &LocalTime, used: u64) -> Result<()> {
    let day = Day::at(r.at_unix_ms, local)
        .ok_or_else(|| Error::new("bad_time", "the timestamp is outside 1970 through 9999"))?
        .to_string();
    let category = r.category.as_ref().map(category_name);
    db.execute("INSERT INTO entries(person,id,kind,at_unix_ms,day,heard,said,text,category,used_unix_ms,active) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![r.person,r.id,r.kind,r.at_unix_ms,day,r.heard,r.said,r.text,category,used,r.active])?;
    let rid = db.last_insert_rowid();
    db.execute(
        "INSERT INTO search(rowid,body) VALUES(?,?)",
        params![
            rid,
            search::text(&format!("{} {} {}", r.heard, r.said, r.text))
        ],
    )?;
    for id in &r.source_ids {
        db.execute("INSERT INTO sources(child,parent) SELECT ?1,rid FROM entries WHERE person=?2 AND id=?3",params![rid,r.person,id])?;
    }
    if let Some(old) = r.supersedes {
        db.execute("INSERT INTO replacements(newer,older) SELECT ?1,rid FROM entries WHERE person=?2 AND id=?3",params![rid,r.person,old])?;
        db.execute(
            "UPDATE entries SET active=0 WHERE person=? AND id=?",
            params![r.person, old],
        )?;
    }
    Ok(())
}
fn category_name(category: &tidy::Category) -> &'static str {
    match category {
        tidy::Category::Promise => "promise",
        tidy::Category::Thread => "thread",
        tidy::Category::Preference => "preference",
    }
}
fn read_record(db: &Connection, person: &Person, id: u64) -> Result<Option<Record>> {
    let mut record=db.query_row("SELECT id,kind,at_unix_ms,heard,said,text,category,active FROM entries WHERE person=? AND id=?",params![person.as_str(),id],|r|{let category:Option<String>=r.get(6)?; Ok(Record{id:r.get(0)?,person:person.as_str().into(),kind:r.get(1)?,at_unix_ms:r.get(2)?,heard:r.get(3)?,said:r.get(4)?,text:r.get(5)?,category:category.map(|c|match c.as_str(){"promise"=>tidy::Category::Promise,"preference"=>tidy::Category::Preference,_=>tidy::Category::Thread}),source_ids:vec![],supersedes:None,active:r.get(7)?})}).optional()?;
    if let Some(record) = &mut record {
        record.source_ids=db.prepare("SELECT p.id FROM sources s JOIN entries c ON c.rid=s.child JOIN entries p ON p.rid=s.parent WHERE c.person=? AND c.id=? AND p.person=c.person ORDER BY p.id")?.query_map(params![person.as_str(),id],|r|r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        record.supersedes=db.query_row("SELECT p.id FROM replacements s JOIN entries c ON c.rid=s.newer JOIN entries p ON p.rid=s.older WHERE c.person=? AND c.id=?",params![person.as_str(),id],|r|r.get(0)).optional()?;
    }
    Ok(record)
}
fn versions(db: &Connection) -> Result<()> {
    db.execute("WITH RECURSIVE chain(rid) AS (SELECT rid FROM doomed UNION SELECT r.newer FROM replacements r JOIN chain c ON r.older=c.rid UNION SELECT r.older FROM replacements r JOIN chain c ON r.newer=c.rid) INSERT OR IGNORE INTO doomed SELECT rid FROM chain",[])?;
    Ok(())
}
fn descendants(db: &Connection, with_versions: bool) -> Result<()> {
    if with_versions {
        db.execute("WITH RECURSIVE chain(rid) AS (SELECT rid FROM doomed UNION SELECT s.child FROM sources s JOIN chain c ON s.parent=c.rid UNION SELECT r.newer FROM replacements r JOIN chain c ON r.older=c.rid UNION SELECT r.older FROM replacements r JOIN chain c ON r.newer=c.rid) INSERT OR IGNORE INTO doomed SELECT rid FROM chain",[])?;
    } else {
        db.execute("WITH RECURSIVE chain(rid) AS (SELECT rid FROM doomed UNION SELECT s.child FROM sources s JOIN chain c ON s.parent=c.rid) INSERT OR IGNORE INTO doomed SELECT rid FROM chain",[])?;
    }
    Ok(())
}
fn erase(tx: &Transaction<'_>) -> Result<u64> {
    let people: Vec<String> = tx
        .prepare("SELECT DISTINCT person FROM entries WHERE rid IN (SELECT rid FROM doomed)")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for person in people {
        tx.execute(
            "UPDATE people SET generation=generation+1 WHERE person=?",
            [&person],
        )?;
        tx.execute(
            "UPDATE sessions SET note='',ids='[]',invalidated=1 WHERE person=?",
            [&person],
        )?;
        tx.execute("DELETE FROM jobs WHERE person=?", [&person])?;
    }
    tx.execute(
        "DELETE FROM search WHERE rowid IN (SELECT rid FROM doomed)",
        [],
    )?;
    let count = tx.execute(
        "DELETE FROM entries WHERE rid IN (SELECT rid FROM doomed)",
        [],
    )? as u64;
    let remain:u64=tx.query_row("SELECT (SELECT count(*) FROM entries WHERE rid IN (SELECT rid FROM doomed))+(SELECT count(*) FROM search WHERE rowid IN (SELECT rid FROM doomed))",[],|r|r.get(0))?;
    if remain != 0 {
        return Err(Error::new("storage", "the deletion could not be verified"));
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #844: retention counts New York's local days, as the journal store
    /// does, across both of its 2026 clock changes, kept a day: Oct 31's
    /// exchange stays at 23:30 on Nov 1 (a 25-hour day), and Mar 7's goes
    /// at 00:30 on Mar 9 (after a 23-hour day). The same when it is opened
    /// again then, which maintains at once.
    #[test]
    fn retention_counts_local_days_across_both_clock_changes() {
        for (name, at, now, kept) in [
            ("autumn", 1_793_462_400_000, 1_793_593_800_000, 1),
            ("spring", 1_772_902_800_000, 1_773_030_600_000, 0),
        ] {
            let dir =
                std::env::temp_dir().join(format!("lamp-days-dst-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            let options = || Options {
                dir: dir.clone(),
                keep_days: 1,
                local: LocalTime::zone("America/New_York").unwrap(),
            };
            let everyone = Person::everyone();
            let mut store = Days::open(options(), at).unwrap();
            store.append(&everyone, at, "here", "yes", &[]).unwrap();
            store.maintain(now).unwrap();
            let exchanges = |store: &Days| store.status(Some(&everyone)).unwrap()[0].exchanges;
            assert_eq!(exchanges(&store), kept, "{name}");
            drop(store);
            let reopened = Days::open(options(), now).unwrap();
            assert_eq!(exchanges(&reopened), kept, "{name}, reopened");
            drop(reopened);
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn the_live_connection_requests_durable_commits_and_private_deletion() {
        let dir = std::env::temp_dir().join(format!("lamp-days-pragmas-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = Days::open(
            Options {
                dir: dir.clone(),
                keep_days: 30,
                local: LocalTime::Offset(0),
            },
            1_791_892_800_000,
        )
        .unwrap();
        for (pragma, expected) in [
            ("synchronous", 3),
            ("fullfsync", 1),
            ("foreign_keys", 1),
            ("secure_delete", 1),
            ("temp_store", 2),
        ] {
            let actual: u64 = store
                .db
                .query_row(&format!("PRAGMA {pragma}"), [], |row| row.get(0))
                .unwrap();
            assert_eq!(actual, expected, "{pragma}");
        }
        let journal: String = store
            .db
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal, "delete");
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }
}
