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
use unicode_normalization::UnicodeNormalization;

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

/// What a recall or search asks beside its [`Ask`] (#2176). The default
/// asks what it always did.
#[derive(Clone, Copy, Debug, Default)]
pub struct Wider {
    /// The last day of a span that begins on the ask's day, inclusive.
    pub last_day: Option<Day>,
    /// The second version (`recall_v2`): a question's common words match
    /// nothing ([`search::query_with`]), and a day or span finds what was
    /// kept on it, not only exchanges.
    pub second: bool,
}

/// No service thread, network client or model belongs to this store.
pub struct Days {
    db: Connection,
    dir: PathBuf,
    keep_days: u32,
    local: LocalTime,
    now: u64,
    set_aside: Option<String>,
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
        // Read before any setting below is applied: `journal_mode` alone
        // would rewrite a newer build's database.
        let version: u64 =
            Connection::open(&path)?.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > 1 {
            set_aside(&options.dir, version, now)?;
            private_file(&path)?;
        }
        // SQLite inherits the database's private mode for its rollback journal.
        let db = Connection::open(&path)?;
        db.busy_timeout(std::time::Duration::from_millis(50))?;
        // EXTRA also syncs the rollback-journal unlink's directory entry;
        // FULL alone can lose the most recent commit on power failure here.
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=DELETE; PRAGMA synchronous=EXTRA; PRAGMA fullfsync=ON; PRAGMA secure_delete=ON; PRAGMA temp_store=MEMORY;")?;
        db.execute_batch(include_str!("days.sql"))?;
        let set_aside = kept_aside(&options.dir)?;
        let mut this = Self {
            db,
            dir: options.dir,
            keep_days: options.keep_days,
            local: options.local,
            now,
            set_aside,
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
        // A set-aside database lasts no longer than the journal does (#2067).
        let before = self.now.saturating_sub(u64::from(self.keep_days) * DAY_MS) / 1000;
        if remove_set_aside(&self.dir, Some(before))? {
            self.set_aside = kept_aside(&self.dir)?;
        }
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
        self.remember_once_from(person, at, text, from, false)
            .map(|(id, _)| id)
    }

    /// [`Self::remember_from`]; with `once` (#2176,
    /// `LAMP_MEMORY_REMEMBER_ONCE`), a text this person already keeps, active
    /// and the same words ignoring case, spacing and a final punctuation mark
    /// ([`same_text`]), is that kept thing: its id, `true`, and no entry is
    /// written; it gains `from` as sources, so forgetting it removes both
    /// conversations' exchanges. Without `once`, as before.
    pub fn remember_once_from(
        &mut self,
        person: &Person,
        at: u64,
        text: &str,
        from: &[u64],
        once: bool,
    ) -> Result<(u64, bool)> {
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
        if once && let Some(id) = self.kept_already(person, &text)? {
            self.link_sources(person, id, from)?;
            return Ok((id, true));
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
            .map(|id| (id, false))
    }

    /// The id of this person's active kept thing with `text`, the same by
    /// [`same_text`].
    fn kept_already(&self, person: &Person, text: &str) -> Result<Option<u64>> {
        let wanted = same_text(text);
        let mut statement = self.db.prepare(
            "SELECT id,text FROM entries WHERE person=? AND kind='kept' AND active=1 ORDER BY id",
        )?;
        let mut rows = statement.query([person.as_str()])?;
        while let Some(row) = rows.next()? {
            let kept: String = row.get(1)?;
            if same_text(&kept) == wanted {
                return Ok(Some(row.get(0)?));
            }
        }
        Ok(None)
    }

    /// Links `from`, validated as a new remember's are, to the kept thing
    /// `id` that `once` found (#2176): forgetting it then removes this
    /// conversation's exchanges too. Links it already has stay one.
    fn link_sources(&mut self, person: &Person, id: u64, from: &[u64]) -> Result<()> {
        if from.is_empty() {
            return Ok(());
        }
        let tx = self.db.transaction()?;
        validate_sources(&tx, person, from, false)?;
        for source in from {
            tx.execute(
                "INSERT OR IGNORE INTO sources(child,parent) SELECT c.rid,p.rid FROM entries c JOIN entries p ON p.person=c.person WHERE c.person=?1 AND c.id=?2 AND p.id=?3",
                params![person.as_str(), id, source],
            )?;
        }
        tx.commit()?;
        Ok(())
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
        self.recall_wider(person, ask, Wider::default())
    }

    /// [`Self::recall`], with a span of days and the second version's
    /// rules ([`Wider`], #2176).
    pub fn recall_wider(&self, person: &Person, ask: &Ask<'_>, wider: Wider) -> Result<Vec<Entry>> {
        let mut entries: Vec<Entry> = self
            .find(person, ask, false, false, wider)?
            .into_iter()
            .filter_map(|r| r.legacy())
            .collect();
        entries.sort_by_key(|r| (r.at_unix_ms(), r.id()));
        Ok(entries)
    }

    /// Ranked data, never an answer. No overlap means no memory. A caller must
    /// still judge whether any returned record actually answers its question.
    pub fn search(&mut self, person: &Person, ask: &Ask<'_>, history: bool) -> Result<Vec<Record>> {
        self.search_wider(person, ask, history, Wider::default())
    }

    /// [`Self::search`], with a span of days and the second version's
    /// rules ([`Wider`], #2176).
    pub fn search_wider(
        &mut self,
        person: &Person,
        ask: &Ask<'_>,
        history: bool,
        wider: Wider,
    ) -> Result<Vec<Record>> {
        if ask
            .query
            .is_none_or(|query| search::query_with(query, wider.second).is_none())
        {
            return Ok(Vec::new());
        }
        let records = self.find(person, ask, true, history, wider)?;
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
        wider: Wider,
    ) -> Result<Vec<Record>> {
        let query = ask.query.filter(|q| !q.trim().is_empty());
        let expression = query.and_then(|query| search::query_with(query, wider.second));
        if query.is_some() && expression.is_none() {
            return Ok(vec![]);
        }
        let day = ask.day.map(|d| d.to_string());
        // One day is a span from it to itself; `day_to` without a first day
        // is never asked ([`crate::days_protocol`]).
        let last = wider.last_day.or(ask.day).map(|d| d.to_string());
        let sql = if expression.is_some() {
            "SELECT e.id FROM search JOIN entries e ON e.rid=search.rowid WHERE search MATCH ?1 AND e.person=?2 AND (?3 IS NULL OR ((e.kind='exchange' OR (?8 AND e.kind='kept')) AND e.day BETWEEN ?3 AND ?7)) AND (?4 OR e.kind!='observation') AND (?5 OR e.active=1) ORDER BY search.rank,e.at_unix_ms DESC,e.id DESC LIMIT ?6"
        } else {
            "SELECT e.id FROM entries e WHERE ?1 IS NULL AND e.person=?2 AND (?3 IS NULL OR e.day BETWEEN ?3 AND ?7) AND ((?4 AND e.kind!='kept') OR e.kind='exchange' OR (?8 AND ?3 IS NOT NULL AND e.kind='kept')) AND (?5 OR e.active=1) ORDER BY e.at_unix_ms DESC,e.id DESC LIMIT ?6"
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
                    ask.limit.min(50),
                    last,
                    wider.second
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
        let count = self.forget_here(person, id)?;
        self.forget_set_aside()?;
        Ok(count)
    }

    fn forget_here(&mut self, person: &Person, id: u64) -> Result<u64> {
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
            self.forget_set_aside()?;
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
        self.forget_set_aside()?;
        Ok(count)
    }

    /// A set-aside database is the person's memory too, and this build can't
    /// reach a fact or a person inside it: every forget removes it whole,
    /// with its sidecars, so nothing said forgotten stays on disk (#2067).
    fn forget_set_aside(&mut self) -> Result<()> {
        remove_set_aside(&self.dir, None)?;
        self.set_aside = None;
        Ok(())
    }

    /// The newest database a newer build wrote that this one set aside,
    /// while it is still in the directory: health says it on every start.
    pub fn set_aside(&self) -> Option<&str> {
        self.set_aside.as_deref()
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
        // A person at the limit has their oldest sessions closed, not the
        // new one refused (#1722, D): a voice that stops without closing
        // its own leaves one behind, and enough restarts filled the room's,
        // after which every session was told no note. A frozen note from a
        // process that has gone is never read again.
        if count >= MAX_SESSIONS {
            self.db.execute(
                "DELETE FROM sessions WHERE rowid IN (SELECT rowid FROM sessions WHERE person=?1 ORDER BY opened_unix_ms,rowid LIMIT ?2)",
                params![person.as_str(), count + 1 - MAX_SESSIONS],
            )?;
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
            // A kept fact, and an observation in a category a newer build
            // wrote, show as a plain fact: never as another category (#2067).
            let category = match (&record.category, record.kind.as_str()) {
                (Some(category), "observation") => category_name(category),
                _ => PLAIN_FACT,
            };
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

    /// Every journal exchange, oldest first: whose, its id and when. For
    /// the observation pass's dry run on a copy of a lamp's store
    /// (`lamp-voice-eval observe-dry`, #1693 (4)), which cuts the journal
    /// into conversations as the voice does. Ids and times only.
    pub fn exchanges(&self) -> Result<Vec<(Person, u64, u64)>> {
        let mut statement = self.db.prepare(
            "SELECT person,id,at_unix_ms FROM entries WHERE kind='exchange' ORDER BY at_unix_ms,id",
        )?;
        let rows = statement.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, u64>(1)?,
                r.get::<_, u64>(2)?,
            ))
        })?;
        let mut found = vec![];
        for row in rows {
            let (person, id, at) = row?;
            if let Some(person) = Person::new(&person) {
                found.push((person, id, at));
            }
        }
        Ok(found)
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
            if prior.kind != "observation" || !prior.active || prior.category.is_none() {
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
                let mut statement = self.db.prepare("SELECT e.id,search.rank FROM search JOIN entries e ON e.rid=search.rowid WHERE search MATCH ?1 AND e.person=?2 AND e.kind='observation' AND e.active=1 AND e.category IN ('promise','thread','preference') ORDER BY search.rank,e.id DESC LIMIT 32")?;
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
        let recent:Vec<u64>=self.db.prepare("SELECT id FROM entries WHERE person=? AND kind='observation' AND active=1 AND category IN ('promise','thread','preference') ORDER BY id DESC LIMIT 32")?.query_map([person.as_str()],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
        for id in recent {
            if ids.len() < 32 && !ids.contains(&id) {
                ids.push(id);
            }
        }
        // One in a category a newer build wrote takes no slot above: it is
        // left as it is (#2067).
        let mut previous = vec![];
        for id in ids {
            let r = self
                .record(person, id)?
                .ok_or_else(|| bad("a previous observation is missing"))?;
            // One in a category a newer build wrote is left as it is.
            let Some(category) = r.category else {
                continue;
            };
            previous.push(tidy::Previous {
                id: r.id,
                at_unix_ms: r.at_unix_ms,
                text: r.text,
                category,
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

    /// The room's exchanges and kept things `ids` (1 to 64, distinct, each
    /// the room's own and active) become `to`'s, someone known, in one
    /// transaction (#2176, 5a): what was said before a lone face was known
    /// follows that person. With them goes anything in the room's scope
    /// derived only from them. Each keeps its row, so its search row and
    /// its links go with it; each gets the next id in `to`'s numbering.
    /// The room's generation moves on and its notes and jobs are dropped,
    /// as when something of it is forgotten. Anything refused moves
    /// nothing. Gives each moved entry's room id and its new id.
    pub fn adopt(&mut self, from: &Person, to: &Person, ids: &[u64]) -> Result<Vec<(u64, u64)>> {
        if from.as_str() != Person::EVERYONE {
            return Err(Error::new(
                "bad_person",
                "only the room's entries can be adopted",
            ));
        }
        if to.as_str() == Person::EVERYONE || to.as_str().starts_with("r-") {
            return Err(Error::new(
                "bad_person",
                "entries are adopted by someone known, never the room or a rules scope",
            ));
        }
        if ids.is_empty()
            || ids.len() > 64
            || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
        {
            return Err(bad("adopted IDs must be unique, with 1 to 64"));
        }
        let tx = self.db.transaction()?;
        tx.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS adopting(rid INTEGER PRIMARY KEY); DELETE FROM adopting;",
        )?;
        for id in ids {
            let listed: Option<i64> = tx
                .query_row(
                    "SELECT rid FROM entries WHERE person=?1 AND id=?2 AND active=1 AND kind IN ('exchange','kept')",
                    params![from.as_str(), id],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(rid) = listed else {
                return Err(bad(
                    "only the room's own active exchanges and kept things can be adopted",
                ));
            };
            tx.execute("INSERT INTO adopting(rid) VALUES(?)", [rid])?;
        }
        // Derived only from what moves: every parent of it moves too. A
        // version (a replacement either way) stays where it is.
        while tx.execute(
            "INSERT OR IGNORE INTO adopting SELECT DISTINCT s.child FROM sources s JOIN entries e ON e.rid=s.child WHERE s.parent IN (SELECT rid FROM adopting) AND e.person=?1 AND e.rid NOT IN (SELECT rid FROM adopting) AND NOT EXISTS (SELECT 1 FROM replacements r WHERE r.newer=e.rid OR r.older=e.rid) AND NOT EXISTS (SELECT 1 FROM sources o WHERE o.child=e.rid AND o.parent NOT IN (SELECT rid FROM adopting))",
            [from.as_str()],
        )? > 0
        {}
        ensure_person(&tx, to)?;
        for (kind, most) in [
            ("kept", store::KEPT_ENTRIES as u64),
            ("observation", MAX_OBSERVATIONS),
        ] {
            let after: u64 = tx.query_row(
                "SELECT count(*) FROM entries WHERE kind=?2 AND (person=?1 OR rid IN (SELECT rid FROM adopting))",
                params![to.as_str(), kind],
                |r| r.get(0),
            )?;
            if after > most {
                return Err(Error::new(
                    "full",
                    "the person's memory has no room for them",
                ));
            }
        }
        let moving: Vec<(i64, u64)> = tx
            .prepare(
                "SELECT rid,id FROM entries WHERE rid IN (SELECT rid FROM adopting) ORDER BY rid",
            )?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut moved = Vec::with_capacity(moving.len());
        for (rid, old) in moving {
            let new = next_id(&tx, to)?;
            tx.execute(
                "UPDATE entries SET person=?2,id=?3 WHERE rid=?1",
                params![rid, to.as_str(), new],
            )?;
            moved.push((old, new));
        }
        // A moved observation asked about once is still asked about once,
        // now as `to`'s: their pacing counts it.
        tx.execute(
            "UPDATE follow_ups SET person=?1 WHERE rid IN (SELECT rid FROM adopting)",
            [to.as_str()],
        )?;
        tx.execute(
            "UPDATE people SET generation=generation+1 WHERE person=?",
            [from.as_str()],
        )?;
        tx.execute(
            "UPDATE sessions SET note='',ids='[]',invalidated=1 WHERE person=?",
            [from.as_str()],
        )?;
        tx.execute("DELETE FROM jobs WHERE person=?", [from.as_str()])?;
        tx.execute("DELETE FROM adopting", [])?;
        tx.commit()?;
        Ok(moved)
    }
}

/// What two kept things are compared by (#2176): NFC, lower case, white
/// space trimmed and collapsed, and punctuation at the end dropped.
fn same_text(text: &str) -> String {
    let lower: String = text.nfc().flat_map(char::to_lowercase).nfc().collect();
    let words = lower.split_whitespace().collect::<Vec<_>>().join(" ");
    words
        .trim_end_matches(|c: char| c.is_whitespace() || is_punctuation(c))
        .to_owned()
}

fn is_punctuation(c: char) -> bool {
    c.is_ascii_punctuation() || matches!(c, '…' | '。' | '！' | '？' | '，' | '、' | '；' | '：')
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

/// A newer build's database (`user_version` above 1): the person's own
/// memory, so it is kept whole under a new name until a forget removes it
/// or its stamp is older than `keep_days` (#2067), and this build starts a
/// new one (#1680, as lamp-tasks does).
/// Nothing reads it back yet. Its sidecars, if any outlived the close, keep
/// their pairing. The journal line names only files.
fn set_aside(dir: &Path, version: u64, now: u64) -> Result<()> {
    // Stamped by the store's clock, as `keep_days` counts it.
    let since = now / 1000;
    let mut kept = format!("{DATABASE}.newer-db{version}-{since}");
    // The directory is locked, so nothing else names a file here meanwhile.
    for n in 1.. {
        if fs::symlink_metadata(dir.join(&kept)).is_err() {
            break;
        }
        kept = format!("{DATABASE}.newer-db{version}-{since}.{n}");
    }
    for sidecar in ["-journal", "-wal", "-shm"] {
        let from = dir.join(format!("{DATABASE}{sidecar}"));
        if fs::symlink_metadata(&from).is_ok() {
            fs::rename(from, dir.join(format!("{kept}{sidecar}")))?;
        }
    }
    fs::rename(dir.join(DATABASE), dir.join(&kept))?;
    File::open(dir)?.sync_all()?;
    eprintln!(
        "lamp-memory: memory across days was written by a newer build (database version {version}, this build reads 1); it is set aside as {kept}, and this build starts with an empty store"
    );
    Ok(())
}

/// Remove every database [`set_aside`] kept in `dir`, with its sidecars:
/// all of them for a forget, or with `before`, those stamped at that second
/// or earlier: `keep_days` old. The journal line names only the files. True if any went.
fn remove_set_aside(dir: &Path, before: Option<u64>) -> Result<bool> {
    let prefix = format!("{DATABASE}.newer-db");
    let mut removed = vec![];
    for entry in fs::read_dir(dir)? {
        let name = entry?.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(rest) = name.strip_prefix(&prefix) else {
            continue;
        };
        if before.is_none_or(|before| stamp(rest).is_some_and(|since| since <= before)) {
            fs::remove_file(dir.join(name))?;
            removed.push(name.to_owned());
        }
    }
    if removed.is_empty() {
        return Ok(false);
    }
    File::open(dir)?.sync_all()?;
    removed.sort();
    let why = if before.is_some() {
        "memory a newer build wrote is keep_days old and was removed"
    } else {
        "a forget also removed the memory a newer build wrote"
    };
    eprintln!("lamp-memory: {why}: {}", removed.join(", "));
    Ok(true)
}

/// The `<seconds>` of `<version>-<seconds>[.n][sidecar]`.
fn stamp(rest: &str) -> Option<u64> {
    rest.split('-')
        .nth(1)
        .and_then(|s| s.split('.').next())
        .and_then(|s| s.parse::<u64>().ok())
}

/// The newest database set aside by [`set_aside`] still in `dir`.
fn kept_aside(dir: &Path) -> Result<Option<String>> {
    let prefix = format!("{DATABASE}.newer-db");
    let mut newest: Option<(u64, String)> = None;
    for entry in fs::read_dir(dir)? {
        let name = entry?.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(rest) = name.strip_prefix(&prefix) else {
            continue;
        };
        if ["-journal", "-wal", "-shm"]
            .iter()
            .any(|s| rest.ends_with(s))
        {
            continue;
        }
        let since = stamp(rest).unwrap_or(0);
        if newest.as_ref().is_none_or(|(at, _)| since >= *at) {
            newest = Some((since, name.to_owned()));
        }
    }
    Ok(newest.map(|(_, name)| name))
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
/// The label a plain fact carries in a session note, the one voice reads
/// for a kept fact (`voice/src/remember.rs`).
const PLAIN_FACT: &str = "preference";
fn category_name(category: &tidy::Category) -> &'static str {
    match category {
        tidy::Category::Promise => "promise",
        tidy::Category::Thread => "thread",
        tidy::Category::Preference => "preference",
    }
}
/// A category a newer build wrote is none here: the record is kept as
/// stored, never read as another category, tidied over or superseded
/// (#1680).
fn category_from(name: &str) -> Option<tidy::Category> {
    match name {
        "promise" => Some(tidy::Category::Promise),
        "thread" => Some(tidy::Category::Thread),
        "preference" => Some(tidy::Category::Preference),
        _ => None,
    }
}
fn read_record(db: &Connection, person: &Person, id: u64) -> Result<Option<Record>> {
    let mut record=db.query_row("SELECT id,kind,at_unix_ms,heard,said,text,category,active FROM entries WHERE person=? AND id=?",params![person.as_str(),id],|r|{let category:Option<String>=r.get(6)?; Ok(Record{id:r.get(0)?,person:person.as_str().into(),kind:r.get(1)?,at_unix_ms:r.get(2)?,heard:r.get(3)?,said:r.get(4)?,text:r.get(5)?,category:category.and_then(|c|category_from(&c)),source_ids:vec![],supersedes:None,active:r.get(7)?})}).optional()?;
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
        // Expiry: a kept thing never expires, even one remembered with its
        // conversation (#1693): the walk stops at it, and its link to an
        // expired exchange goes by the foreign key (#2176). A forget, above,
        // still follows that link to it.
        db.execute("WITH RECURSIVE chain(rid) AS (SELECT rid FROM doomed UNION SELECT s.child FROM sources s JOIN chain c ON s.parent=c.rid JOIN entries e ON e.rid=s.child WHERE e.kind!='kept') INSERT OR IGNORE INTO doomed SELECT rid FROM chain",[])?;
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

    /// #2176: a kept thing no longer active (replaced) is not what `once`
    /// finds; the same text is kept anew.
    #[test]
    fn a_remember_once_passes_over_a_kept_thing_no_longer_active() {
        let dir = std::env::temp_dir().join(format!("lamp-days-once-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let now = 1_791_892_800_000;
        let options = Options {
            dir: dir.clone(),
            keep_days: 30,
            local: LocalTime::Offset(0),
        };
        let mut store = Days::open(options, now).unwrap();
        let everyone = Person::everyone();
        let first = store.remember_once_from(&everyone, now, "coffee black", &[], true);
        assert_eq!(first.unwrap(), (1, false));
        let again = store.remember_once_from(&everyone, now, "Coffee black!", &[], true);
        assert_eq!(again.unwrap(), (1, true));
        store
            .db
            .execute("UPDATE entries SET active=0 WHERE id=1", [])
            .unwrap();
        let anew = store.remember_once_from(&everyone, now, "coffee black", &[], true);
        assert_eq!(anew.unwrap(), (2, false));
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    /// #2176: a remember `once` that finds the kept thing links the new
    /// conversation's exchanges to it, so forgetting it removes both.
    #[test]
    fn a_remember_once_links_its_sources_to_the_kept_thing_it_finds() {
        let dir = std::env::temp_dir().join(format!("lamp-days-once-src-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let now = 1_791_892_800_000;
        let options = Options {
            dir: dir.clone(),
            keep_days: 30,
            local: LocalTime::Offset(0),
        };
        let mut store = Days::open(options, now).unwrap();
        let mai = Person::new("p-0123456789abcdef").unwrap();
        let a = store
            .append(&mai, now, "I take coffee black", "noted", &[])
            .unwrap();
        let b = store
            .append(&mai, now, "coffee black, remember", "sure", &[])
            .unwrap();
        let other = store.append(&mai, now, "nice day", "it is", &[]).unwrap();
        let (k, existing) = store
            .remember_once_from(&mai, now, "coffee black", &[a], true)
            .unwrap();
        assert!(!existing);
        let again = store.remember_once_from(&mai, now, "Coffee black.", &[b], true);
        assert_eq!(again.unwrap(), (k, true));
        assert_eq!(
            store.record(&mai, k).unwrap().unwrap().source_ids,
            vec![a, b]
        );
        // A source that isn't the person's exchange is refused, nothing linked.
        assert!(
            store
                .remember_once_from(&mai, now, "coffee black", &[k], true)
                .is_err()
        );
        assert_eq!(store.kept(&mai, 16384).unwrap().len(), 1);
        store.forget(&mai, k).unwrap();
        for id in [k, a, b] {
            assert!(store.record(&mai, id).unwrap().is_none(), "{id}");
        }
        assert!(store.record(&mai, other).unwrap().is_some());
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    /// #2176 (5a): the room's exchange and kept thing become A's, with the
    /// link between them, their search rows, and an exchange derived only
    /// from them; one also derived from something staying stays, its link
    /// kept, so forgetting either still reaches it. Another person's ids,
    /// an inactive one, or any refused id move nothing.
    #[test]
    fn the_rooms_early_words_are_adopted_whole_and_nothing_else_is() {
        let dir = std::env::temp_dir().join(format!("lamp-days-adopt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let now = 1_791_892_800_000;
        let mut store = Days::open(
            Options {
                dir: dir.clone(),
                keep_days: 30,
                local: LocalTime::Offset(0),
            },
            now,
        )
        .unwrap();
        let room = Person::everyone();
        let a = Person::new("p-a").unwrap();
        let b = Person::new("p-b").unwrap();
        let older = store
            .remember(&room, now, "the room's older thing")
            .unwrap();
        assert_eq!(
            store.append(&a, now, "theirs before", "yes", &[]).unwrap(),
            1
        );
        assert_eq!(store.append(&a, now, "and again", "yes", &[]).unwrap(), 2);
        let said = store
            .append(&room, now, "my sister marries in june", "lovely", &[])
            .unwrap();
        let kept = store
            .remember_from(&room, now, "sister marries in june", &[said])
            .unwrap();
        let derived = store
            .append(&room, now, "about june", "noted", &[kept])
            .unwrap();
        let mixed = store
            .append(&room, now, "both", "ok", &[kept, older])
            .unwrap();
        let mut bs = 0;
        for _ in 0..10 {
            bs = store.append(&b, now, "b's own", "fine", &[]).unwrap();
        }
        let inactive = store.append(&room, now, "gone quiet", "ok", &[]).unwrap();
        store
            .db
            .execute(
                "UPDATE entries SET active=0 WHERE person='everyone' AND id=?",
                [inactive],
            )
            .unwrap();
        // A follow-up row on what moves goes with it; one on what stays,
        // stays the room's.
        let follow_up = |store: &Days, rid_of: u64| {
            store
                .db
                .execute(
                    "INSERT INTO follow_ups(rid,person,asked_unix_ms) SELECT rid,'everyone',?2 FROM entries WHERE person='everyone' AND id=?1",
                    params![rid_of, now],
                )
                .unwrap()
        };
        assert_eq!(follow_up(&store, derived), 1);
        assert_eq!(follow_up(&store, mixed), 1);
        let followed = |store: &Days, who: &str| -> u64 {
            store
                .db
                .query_row(
                    "SELECT count(*) FROM follow_ups WHERE person=?",
                    [who],
                    |r| r.get(0),
                )
                .unwrap()
        };
        let generation = super::generation(&store.db, &room).unwrap();
        for (ids, why) in [
            (vec![said, 999], "an id the room hasn't"),
            (vec![said, bs], "an id only another has"),
            (vec![said, inactive], "an inactive one"),
            (vec![said, said], "twice"),
            (vec![], "none"),
        ] {
            assert_eq!(
                store.adopt(&room, &a, &ids).unwrap_err().code,
                "bad_request",
                "{why}"
            );
        }
        assert_eq!(store.adopt(&b, &a, &[bs]).unwrap_err().code, "bad_person");
        assert_eq!(
            store.adopt(&room, &room, &[said]).unwrap_err().code,
            "bad_person"
        );
        assert_eq!(
            store
                .adopt(&room, &a.rules().unwrap(), &[said])
                .unwrap_err()
                .code,
            "bad_person"
        );
        assert!(
            store.record(&room, said).unwrap().is_some(),
            "nothing moved"
        );
        assert_eq!(super::generation(&store.db, &room).unwrap(), generation);

        assert_eq!(followed(&store, "everyone"), 2, "nothing moved");
        let moved = store.adopt(&room, &a, &[said, kept]).unwrap();
        assert_eq!(moved, [(said, 3), (kept, 4), (derived, 5)]);
        assert_eq!(
            (followed(&store, "everyone"), followed(&store, "p-a")),
            (1, 1)
        );
        for id in [said, kept, derived] {
            assert!(store.record(&room, id).unwrap().is_none(), "{id}");
        }
        let theirs = store.record(&a, 4).unwrap().unwrap();
        assert_eq!((theirs.kind.as_str(), theirs.source_ids), ("kept", vec![3]));
        assert_eq!(store.record(&a, 5).unwrap().unwrap().source_ids, [4]);
        let ask = Ask {
            query: Some("june sister"),
            day: None,
            limit: 8,
            max_bytes: 4096,
        };
        let found = |store: &mut Days, who: &Person| -> Vec<u64> {
            let mut ids: Vec<u64> = store
                .search(who, &ask, false)
                .unwrap()
                .iter()
                .map(|r| r.id)
                .collect();
            ids.sort_unstable();
            ids
        };
        assert_eq!(found(&mut store, &a), [3, 4, 5]);
        assert!(found(&mut store, &room).is_empty());
        assert_eq!(
            store.kept(&a, 4096).unwrap()[0].text,
            "sister marries in june"
        );
        // The room's generation moved on, as for anything of it gone.
        assert_eq!(super::generation(&store.db, &room).unwrap(), generation + 1);
        // Still the room's, still linked: forgetting A's kept thing takes it.
        assert!(store.record(&room, mixed).unwrap().is_some());
        assert_eq!(
            store.adopt(&room, &a, &[derived]).unwrap_err().code,
            "bad_request"
        );
        assert_eq!(store.forget(&a, 4).unwrap(), 4);
        assert!(store.record(&room, mixed).unwrap().is_none());
        assert!(store.record(&room, older).unwrap().is_some());
        drop(store);
        fs::remove_dir_all(dir).unwrap();
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
