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
/// An observation with an event day is asked about at most this many local
/// days after it, instead of [`FOLLOW_UP_WITHIN_MS`] from when it was said.
pub const FOLLOW_UP_WITHIN_DAYS: u32 = 14;
/// With `until_asked` (#1693, the first builder on #2602, 22 Oct 09:4x), a
/// follow-up, dated or not (13:1x), the lamp has not been heard asking about is offered in at most
/// this many sessions, each on its own day ([`FOLLOW_UP_EVERY_MS`]): the
/// first and at most two more, so it never nags. The last offer is recorded
/// as given, as without `until_asked`.
pub const FOLLOW_UP_OFFERS: u32 = 3;
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
    /// Why a `refused` request was refused (#2510): a code, no content.
    pub reason: Option<&'static str>,
}
pub type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub fn new(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            reason: None,
        }
    }
    /// A request refused for `reason` (#2510): nothing was written.
    pub fn refused(reason: &'static str, message: &'static str) -> Self {
        Self {
            code: "refused",
            message,
            reason: Some(reason),
        }
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
        // An event still to come, or within its follow-up window, outlives
        // both rules (#1693): an event day on or after this one is kept, so
        // a wedding 50 days out is there to ask about the day after it.
        let event = Day::at(self.now, &self.local)
            .and_then(|today| today.before(FOLLOW_UP_WITHIN_DAYS))
            .map_or_else(|| "1970-01-01".into(), |d| d.to_string());
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM doomed", [])?;
        tx.execute("INSERT INTO doomed SELECT rid FROM entries WHERE (kind='exchange' AND day<?1) OR (kind='observation' AND used_unix_ms<?2 AND NOT EXISTS (SELECT 1 FROM event_days d WHERE d.rid=entries.rid AND d.day>=?3))",params![day,unused,event])?;
        descendants(&tx, Some(&event))?;
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
        self.append_ruled_by(person, at, heard, said, sources, rules, None)
    }
    /// [`Self::append_ruled`], said by `speaker` (#2849, the voice's
    /// `LAMP_MEMORY_ROOM_SPEAKER`): the room's line, kept with the id of the
    /// one known person seen saying it ([`speaker_of`]), for their
    /// [`Self::forget_said`].
    #[allow(clippy::too_many_arguments)]
    pub fn append_ruled_by(
        &mut self,
        person: &Person,
        at: u64,
        heard: &str,
        said: &str,
        sources: &[u64],
        rules: &[u64],
        speaker: Option<&str>,
    ) -> Result<u64> {
        let speaker = speaker_of(person, speaker)?;
        let heard = store::tidy(heard, store::HEARD_BYTES);
        let said = store::tidy(said, store::SAID_BYTES);
        if heard.is_empty() && said.is_empty() {
            return Err(Error::new("empty", "there is no exchange to remember"));
        }
        self.add(
            person,
            at,
            (&heard, &said, ""),
            sources,
            rules,
            None,
            on_said(None, speaker),
        )
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
        self.remember_on(person, at, text, from, once, None)
    }

    /// [`Self::remember_once_from`], with the day it is about (#1693, the
    /// voice's `LAMP_MEMORY_EVENTS`): `on`, `YYYY-MM-DD`, kept in
    /// `event_days` for the kept entry when it is a real day from the local
    /// day of `at` to [`EVENT_WITHIN_DAYS`] after it ([`kept_day`]); any
    /// other `on` is dropped and the fact kept undated. A kept thing that
    /// is found already (`once`) gains the day when it has none, and takes
    /// it when it is later than the one it has (a weekly "dentist on
    /// Friday" said again for next Friday); then its follow-up row goes, so
    /// the new day is followed up once in turn. An earlier or the same day
    /// changes nothing. The sources and the day commit together.
    pub fn remember_on(
        &mut self,
        person: &Person,
        at: u64,
        text: &str,
        from: &[u64],
        once: bool,
        on: Option<&str>,
    ) -> Result<(u64, bool)> {
        self.remember_on_by(person, at, text, from, once, on, None)
    }

    /// [`Self::remember_on`], said by `speaker` (#2849): the room's kept
    /// thing with the id of the one known person seen saying it. A kept
    /// thing `once` finds is written to by nothing, and so gains no speaker:
    /// it may first have been someone else's words.
    #[allow(clippy::too_many_arguments)]
    pub fn remember_on_by(
        &mut self,
        person: &Person,
        at: u64,
        text: &str,
        from: &[u64],
        once: bool,
        on: Option<&str>,
        speaker: Option<&str>,
    ) -> Result<(u64, bool)> {
        let speaker = speaker_of(person, speaker)?;
        let on = on.and_then(|on| kept_day(on, at, &self.local));
        let text = self.kept_text(person, text, from)?;
        if once && let Some(id) = self.kept_already(person, &text)? {
            let tx = self.db.transaction()?;
            link_sources(&tx, person, id, from)?;
            if let Some(on) = on {
                let rid: i64 = tx.query_row(
                    "SELECT rid FROM entries WHERE person=?1 AND id=?2",
                    params![person.as_str(), id],
                    |r| r.get(0),
                )?;
                // Days are `YYYY-MM-DD`, so text order is day order.
                let moved = tx.execute(
                    "UPDATE event_days SET day=?2 WHERE rid=?1 AND day<?2",
                    params![rid, on.to_string()],
                )?;
                if moved > 0 {
                    tx.execute("DELETE FROM follow_ups WHERE rid=?", [rid])?;
                    // And its offers: the new day is offered afresh.
                    tx.execute("DELETE FROM follow_up_offers WHERE rid=?", [rid])?;
                } else {
                    tx.execute(
                        "INSERT OR IGNORE INTO event_days(rid,day) VALUES(?1,?2)",
                        params![rid, on.to_string()],
                    )?;
                }
            }
            tx.commit()?;
            return Ok((id, true));
        }
        self.kept_room(person)?;
        self.add(
            person,
            at,
            ("", "", &text),
            from,
            &[],
            None,
            on_said(on, speaker),
        )
        .map(|id| (id, false))
    }

    /// A correction (#2510, the voice's `LAMP_MEMORY_REPLACES`): `text`
    /// kept as [`Self::remember_from`] keeps it, replacing this person's
    /// kept thing `old`, which `shown` (what the asking session was shown)
    /// must hold, and which must be kept, active and not newer than `at`.
    /// The new one, the link `replacements(newer, older)` and the old one
    /// made inactive commit together; refused, nothing is written. Never
    /// `once`: a correction is always its new version. Forgetting either
    /// removes both ([`versions`]).
    pub fn remember_replacing(
        &mut self,
        person: &Person,
        at: u64,
        text: &str,
        from: &[u64],
        old: u64,
        shown: &[u64],
    ) -> Result<u64> {
        self.remember_replacing_on(person, at, text, from, old, shown, None)
    }

    /// [`Self::remember_replacing`], the correction with its own day `on`
    /// as [`Self::remember_on`] keeps one. The replaced one's day goes in
    /// the same transaction, whether or not the correction has one: what
    /// is only history is never followed up.
    #[allow(clippy::too_many_arguments)]
    pub fn remember_replacing_on(
        &mut self,
        person: &Person,
        at: u64,
        text: &str,
        from: &[u64],
        old: u64,
        shown: &[u64],
        on: Option<&str>,
    ) -> Result<u64> {
        self.remember_replacing_on_by(person, at, text, from, old, shown, on, None)
    }

    /// [`Self::remember_replacing_on`], said by `speaker` (#2849): the
    /// room's correction with the id of the one known person seen saying it.
    /// It is a version of what it replaces, so their [`Self::forget_said`]
    /// removes both, as any forget of either does.
    #[allow(clippy::too_many_arguments)]
    pub fn remember_replacing_on_by(
        &mut self,
        person: &Person,
        at: u64,
        text: &str,
        from: &[u64],
        old: u64,
        shown: &[u64],
        on: Option<&str>,
        speaker: Option<&str>,
    ) -> Result<u64> {
        let speaker = speaker_of(person, speaker)?;
        // Not shown first: an id this session never saw says nothing of
        // what it is, or whether it is.
        if !shown.contains(&old) {
            return Err(Error::refused(
                "not_shown",
                "only a kept thing this session was shown can be replaced",
            ));
        }
        let Some(record) = self.record(person, old)? else {
            return Err(Error::refused(
                "unknown",
                "the replaced ID is not this person's",
            ));
        };
        if record.kind != "kept" {
            return Err(Error::refused(
                "not_kept",
                "only a kept thing can be replaced",
            ));
        }
        if !record.active {
            return Err(Error::refused(
                "inactive",
                "the kept thing was already replaced",
            ));
        }
        if record.at_unix_ms > at {
            return Err(Error::refused(
                "not_older",
                "a correction cannot be older than what it replaces",
            ));
        }
        let on = on.and_then(|on| kept_day(on, at, &self.local));
        let text = self.kept_text(person, text, from)?;
        self.kept_room(person)?;
        self.add(
            person,
            at,
            ("", "", &text),
            from,
            &[],
            Some(old),
            on_said(on, speaker),
        )
    }

    /// A remember's `from`, the person's own exchanges, and its text tidied.
    fn kept_text(&self, person: &Person, text: &str, from: &[u64]) -> Result<String> {
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
        Ok(text)
    }

    /// Whether this person's kept list has room for one more. Only active
    /// kept things count: a replaced version (#2510) is history, not a slot.
    fn kept_room(&self, person: &Person) -> Result<()> {
        let count: u64 = self.db.query_row(
            "SELECT count(*) FROM entries WHERE person=? AND kind='kept' AND active=1",
            [person.as_str()],
            |r| r.get(0),
        )?;
        if count >= store::KEPT_ENTRIES as u64 {
            return Err(Error::new(
                "full",
                "the kept list is full; nothing was removed",
            ));
        }
        Ok(())
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

    // The one private writer of an exchange or a kept entry: each argument
    // is a column or its links; `said` is its event day and who said it,
    // each kept in its side table.
    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        person: &Person,
        at: u64,
        (heard, said, text): (&str, &str, &str),
        sources: &[u64],
        rules: &[u64],
        supersedes: Option<u64>,
        Said { on, speaker }: Said<'_>,
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
            supersedes,
            active: true,
        };
        // A replaced kept thing's day goes with its being replaced (#1693):
        // only the current version is ever followed up.
        if let Some(old) = supersedes {
            tx.execute(
                "DELETE FROM event_days WHERE rid IN (SELECT rid FROM entries WHERE person=?1 AND id=?2)",
                params![person.as_str(), old],
            )?;
        }
        insert_record(&tx, &record, &self.local, self.now)?;
        if let Some(on) = on {
            tx.execute(
                "INSERT INTO event_days(rid,day) SELECT rid,?3 FROM entries WHERE person=?1 AND id=?2",
                params![person.as_str(), id, on.to_string()],
            )?;
        }
        // Who said it (#2849): only ever the room's, checked by `speaker_of`.
        if let Some(speaker) = speaker {
            tx.execute(
                "INSERT INTO speakers(rid,speaker) SELECT rid,?3 FROM entries WHERE person=?1 AND id=?2",
                params![person.as_str(), id, speaker],
            )?;
        }
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
        self.kept_with(person, max_bytes, false)
    }

    /// [`Self::kept`], with `observations` what was noticed of the person
    /// too (#3113): their current observations, by the same ids, so a
    /// forget by name reaches "my favourite colour" kept without being
    /// asked. Newest first up to `max_bytes`, given oldest first.
    pub fn kept_with(
        &self,
        person: &Person,
        max_bytes: usize,
        observations: bool,
    ) -> Result<Vec<Kept>> {
        // A replaced kept thing (#2510) is no longer kept: only its history;
        // and a superseded observation is no longer what was noticed.
        let mut statement=self.db.prepare("SELECT id,at_unix_ms,text FROM entries WHERE person=?1 AND (kind='kept' OR (?2 AND kind='observation')) AND active=1 ORDER BY id DESC")?;
        let rows = statement.query_map(params![person.as_str(), observations], |r| {
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

    /// The event day attached to this person's record, if one was kept.
    /// Search replies may include it so Voice can distinguish a coming
    /// event from one that already happened; older readers ignore the field.
    pub fn event_day(&self, person: &Person, id: u64) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row(
                "SELECT d.day FROM event_days d JOIN entries e ON e.rid=d.rid WHERE e.person=?1 AND e.id=?2 ORDER BY d.day LIMIT 1",
                params![person.as_str(), id],
                |row| row.get(0),
            )
            .optional()?)
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

    /// When this person's record `id` was replaced (#2510): the
    /// `at_unix_ms` of the newer version that replaced it, through
    /// `replacements`; `None` for one never replaced.
    pub fn replaced_at(&self, person: &Person, id: u64) -> Result<Option<u64>> {
        Ok(self.db.query_row(
            "SELECT min(n.at_unix_ms) FROM replacements r JOIN entries o ON o.rid=r.older JOIN entries n ON n.rid=r.newer WHERE o.person=?1 AND o.id=?2 AND n.person=?1",
            params![person.as_str(), id],
            |r| r.get::<_, Option<u64>>(0),
        )?)
    }

    /// An observation of `person`'s, as the observe pass keeps one, for
    /// tests: `active` false is one superseded.
    #[cfg(test)]
    pub(crate) fn observe_for_test(
        &mut self,
        person: &Person,
        at: u64,
        text: &str,
        active: bool,
    ) -> u64 {
        ensure_person(&self.db, person).unwrap();
        let id = next_id(&self.db, person).unwrap();
        insert_record(
            &self.db,
            &Record {
                id,
                person: person.as_str().into(),
                kind: "observation".into(),
                at_unix_ms: at,
                heard: String::new(),
                said: String::new(),
                text: text.into(),
                category: Some(tidy::Category::Preference),
                source_ids: vec![],
                supersedes: None,
                active,
            },
            &self.local,
            self.now,
        )
        .unwrap();
        id
    }

    /// A targeted forget by the selective rule ([`selective_descendants`]).
    pub fn forget(&mut self, person: &Person, id: u64) -> Result<u64> {
        self.forget_linked(person, id, true)
    }

    /// [`Self::forget`], `selective` or not. Not, everything linked to a
    /// kept thing or an observation goes with it, as before this rule: what
    /// a request without `selective` gets, so an older voice, and one whose
    /// links say what was used (`LAMP_MEMORY_LINK_USED`), forget as before.
    pub fn forget_linked(&mut self, person: &Person, id: u64, selective: bool) -> Result<u64> {
        let count = self.forget_here(person, id, selective)?;
        self.forget_set_aside()?;
        Ok(count)
    }

    fn forget_here(&mut self, person: &Person, id: u64, selective: bool) -> Result<u64> {
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
        //
        // The evidence goes, but nothing goes because it derives from the
        // evidence (#3113): with `LAMP_MEMORY_FORGET_ALL` a kept thing's
        // sources are its conversation's exchanges, so one exchange can be
        // the evidence of the colour and a source of the wedding, and of
        // other things noticed in it. Forgetting the colour took the wedding;
        // forgetting the wedding took the colour. So the walk below starts
        // from what is named (with its versions) only, and the evidence joins
        // after it; a link from something kept to a removed exchange goes by
        // the foreign key, as on expiry.
        let evidence: Vec<i64> = if record.kind == "observation" || record.kind == "kept" {
            tx.prepare("SELECT DISTINCT s.parent FROM sources s JOIN doomed d ON s.child=d.rid JOIN entries e ON e.rid=s.parent WHERE e.person=?1 AND e.kind='exchange'")?
                .query_map([person.as_str()], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        } else {
            Vec::new()
        };
        // A kept thing or an observation takes what carries its words, not
        // all that was shown it ([`selective_descendants`]); an exchange,
        // all that derives from it, as before.
        if selective && (record.kind == "observation" || record.kind == "kept") {
            selective_descendants(&tx, person)?;
        } else {
            descendants(&tx, None)?;
        }
        for rid in evidence {
            tx.execute("INSERT OR IGNORE INTO doomed VALUES (?1)", [rid])?;
        }
        let count = erase(&tx)?;
        tx.commit()?;
        Ok(count)
    }
    /// The room's lines `speaker` said (#2849, the voice's
    /// `LAMP_MEMORY_ROOM_SPEAKER`, after that person's own `forget_all`):
    /// the room's (`everyone`) exchanges and kept things whose `speakers`
    /// row is theirs, with their versions but for a version someone else
    /// was seen saying (a correction of theirs by another, or theirs of
    /// another's: those words are the other's); and an observation of the
    /// room's made only from those, every one of its sources among them.
    /// A kept thing goes only when it is named, here by its own speakers
    /// row (#3113, Friday's P1 on #3213): one kept in a conversation they
    /// spoke in, said by someone unseen, stays. One also made from anyone
    /// else's line stays, and so does that line: the walk never climbs from
    /// what goes to its other sources, and takes no sibling (the #3127
    /// shape). Nothing of any person's own scope is reached. How many went.
    pub fn forget_said(&mut self, speaker: &str) -> Result<u64> {
        if !lamp_msgs::known::is_person_id(speaker) {
            return Err(bad("a speaker is the id of one person the eyes know"));
        }
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM doomed", [])?;
        tx.execute(
            "INSERT INTO doomed SELECT e.rid FROM speakers s JOIN entries e ON e.rid=s.rid WHERE s.speaker=?1 AND e.person=?2",
            params![speaker, Person::EVERYONE],
        )?;
        // Their versions, both ways, stopping at one someone else was seen
        // saying: it stays, and so does what lies past it (Friday's P2 on
        // #3213). An adopted version is now someone's own and stops this
        // room-only walk too; adoption keeps its links for ordinary forget.
        tx.execute(
            "WITH RECURSIVE chain(rid) AS (SELECT rid FROM doomed UNION SELECT r.newer FROM replacements r JOIN chain c ON r.older=c.rid JOIN entries e ON e.rid=r.newer WHERE e.person=?2 AND NOT EXISTS (SELECT 1 FROM speakers p WHERE p.rid=r.newer AND p.speaker!=?1) UNION SELECT r.older FROM replacements r JOIN chain c ON r.newer=c.rid JOIN entries e ON e.rid=r.older WHERE e.person=?2 AND NOT EXISTS (SELECT 1 FROM speakers p WHERE p.rid=r.older AND p.speaker!=?1)) INSERT OR IGNORE INTO doomed SELECT rid FROM chain",
            params![speaker, Person::EVERYONE],
        )?;
        // One step down from what is named, never up: only what has at
        // least one source and none outside it, read whole before any of
        // it joins, so nothing goes for having come from what goes this way.
        let made: Vec<i64> = tx
            .prepare("SELECT e.rid FROM entries e WHERE e.person=?2 AND e.kind='observation' AND e.rid NOT IN (SELECT rid FROM doomed) AND EXISTS (SELECT 1 FROM sources s WHERE s.child=e.rid) AND NOT EXISTS (SELECT 1 FROM sources s WHERE s.child=e.rid AND s.parent NOT IN (SELECT rid FROM doomed)) AND NOT EXISTS (SELECT 1 FROM speakers p WHERE p.rid=e.rid AND p.speaker!=?1)")?
            .query_map(params![speaker, Person::EVERYONE], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for rid in made {
            tx.execute("INSERT OR IGNORE INTO doomed VALUES (?1)", [rid])?;
        }
        let count = erase(&tx)?;
        tx.commit()?;
        self.forget_set_aside()?;
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
        let count = empty(&tx, person)?;
        tx.commit()?;
        self.forget_set_aside()?;
        Ok(count)
    }

    /// Everything kept for one person and every rule in their rules scope
    /// (`Person::rules`), in one transaction: the app's removal of the
    /// person (`docs/memory.md`, "Forgetting a person"). The same erase as
    /// [`Self::forget_all`] of each, so a crash between them never leaves
    /// the person forgotten and their rules kept. Their id comes off every
    /// room line they were seen saying in the same transaction, so no one
    /// later remembered by the same name takes those lines over
    /// (reviewer-three's P2 on #3213); the lines stay, as they did before
    /// the speakers table. The removed entries, then the removed rules.
    pub fn forget_person(&mut self, person: &Person) -> Result<(u64, u64)> {
        let rules = person
            .rules()
            .ok_or_else(|| Error::new("bad_person", "the person has no rules scope"))?;
        let tx = self.db.transaction()?;
        let removed = empty(&tx, Some(person))?;
        let ruled = empty(&tx, Some(&rules))?;
        tx.execute("DELETE FROM speakers WHERE speaker=?1", [person.as_str()])?;
        tx.commit()?;
        self.forget_set_aside()?;
        Ok((removed, ruled))
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
    /// there never asks one person about another's week. Event days only
    /// hold one back until its day is over ([`Self::follow_up_with`]
    /// without `events`): never asked about before the event.
    pub fn follow_up(&mut self, person: &Person) -> Result<Option<serde_json::Value>> {
        self.follow_up_with(person, false)
    }

    /// [`Self::follow_up`], and with `events` (the voice's
    /// `LAMP_MEMORY_EVENTS`) an observation with an event day (`event_days`)
    /// is asked about after it: from the local day after its event day to
    /// [`FOLLOW_UP_WITHIN_DAYS`] after it, never on the day itself or
    /// before, whenever it was said. An undated one keeps its rules. A
    /// dated one comes first, then the newest; once, and one a day, as
    /// ever. The answer carries a dated one's `on`. A kept thing with a
    /// day ([`Self::remember_on`]) is one of these too, with the category
    /// `"kept"`; a kept thing without one is never given, as without
    /// `events`.
    pub fn follow_up_with(
        &mut self,
        person: &Person,
        events: bool,
    ) -> Result<Option<serde_json::Value>> {
        self.follow_up_until_asked(person, events, false)
    }

    /// [`Self::follow_up_with`], and with `until_asked` (the voice's
    /// `LAMP_MEMORY_THREADS`; the first builder on #2602, 22 Oct 09:4x for
    /// a dated one, 13:1x for an undated thread or promise, with `events`
    /// or without) the one given is marked asked when the lamp asks, not
    /// when it is given: given, it is counted as offered (`follow_up_offers`) and
    /// its answer carries `"until_asked":true`, and it stays eligible
    /// until the voice says the lamp asked ([`Self::follow_up_asked`]) or
    /// it has been offered [`FOLLOW_UP_OFFERS`] times, the last of which is
    /// recorded as given in its transaction as ever. An offer counts as a
    /// give for the one a day, so it is offered again on a later day at the
    /// earliest.
    pub fn follow_up_until_asked(
        &mut self,
        person: &Person,
        events: bool,
        until_asked: bool,
    ) -> Result<Option<serde_json::Value>> {
        if person.as_str() == Person::EVERYONE || person.as_str().starts_with("r-") {
            return Err(Error::new(
                "bad_person",
                "a follow-up is for someone known, never the room's",
            ));
        }
        let now = self.now;
        let tx = self.db.transaction()?;
        // An offer not yet asked about counts as a give (#1693): one a day.
        // Without offers, the newest of `follow_ups` as before.
        let lately: Option<u64> = tx.query_row(
            "SELECT max(at) FROM (SELECT asked_unix_ms AS at FROM follow_ups WHERE person=?1 UNION ALL SELECT offered_unix_ms FROM follow_up_offers WHERE person=?1)",
            [person.as_str()],
            |r| r.get(0),
        )?;
        if lately.is_some_and(|at| now.saturating_sub(at) < FOLLOW_UP_EVERY_MS) {
            return Ok(None);
        }
        let found: Option<(i64, u64, String, String, Option<String>)> = if events {
            let today = Day::at(now, &self.local).ok_or_else(|| {
                Error::new("bad_time", "the timestamp is outside 1970 through 9999")
            })?;
            // Days are `YYYY-MM-DD`, so text order is day order.
            let earliest = today
                .before(FOLLOW_UP_WITHIN_DAYS)
                .map(|day| day.to_string())
                .unwrap_or_default();
            tx.query_row(
                "SELECT e.rid,e.id,e.text,coalesce(e.category,'kept'),d.day FROM entries e LEFT JOIN event_days d ON d.rid=e.rid WHERE e.person=?1 AND e.active=1 AND ((e.kind='observation' AND e.category IN ('thread','promise')) OR (e.kind='kept' AND d.day IS NOT NULL)) AND NOT EXISTS (SELECT 1 FROM follow_ups f WHERE f.rid=e.rid) AND ((d.day IS NULL AND e.at_unix_ms<=?2 AND e.at_unix_ms>=?3) OR (d.day<?4 AND d.day>=?5)) ORDER BY d.day IS NULL,e.at_unix_ms DESC,e.id DESC LIMIT 1",
                params![
                    person.as_str(),
                    now.saturating_sub(FOLLOW_UP_AFTER_MS),
                    now.saturating_sub(FOLLOW_UP_WITHIN_MS),
                    today.to_string(),
                    earliest
                ],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?
        } else {
            // A dated one is never asked about before its day is over, even
            // with the switch since turned off; after it, as any other.
            // Without dated rows this is the query from before event days.
            let today = Day::at(now, &self.local)
                .map(|day| day.to_string())
                .unwrap_or_default();
            tx.query_row(
                "SELECT e.rid,e.id,e.text,e.category FROM entries e WHERE e.person=?1 AND e.kind='observation' AND e.active=1 AND e.category IN ('thread','promise') AND e.at_unix_ms<=?2 AND e.at_unix_ms>=?3 AND NOT EXISTS (SELECT 1 FROM follow_ups f WHERE f.rid=e.rid) AND NOT EXISTS (SELECT 1 FROM event_days d WHERE d.rid=e.rid AND d.day>=?4) ORDER BY e.at_unix_ms DESC,e.id DESC LIMIT 1",
                params![
                    person.as_str(),
                    now.saturating_sub(FOLLOW_UP_AFTER_MS),
                    now.saturating_sub(FOLLOW_UP_WITHIN_MS),
                    today
                ],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, None)),
            )
            .optional()?
        };
        let Some((rid, id, text, category, on)) = found else {
            return Ok(None);
        };
        // With `until_asked`, whatever is given is offered, and recorded as
        // given only on its last offer: a dated one, and an undated thread
        // or promise too (the first builder on #2602, 22 Oct 13:1x, after
        // lamp-06c1 told one at "good morning" and never asked). Without
        // it, recorded now, as ever.
        let held = if until_asked {
            let offers: u32 = tx.query_row(
                "INSERT INTO follow_up_offers(rid,person,offers,offered_unix_ms) VALUES(?1,?2,1,?3) ON CONFLICT(rid) DO UPDATE SET offers=offers+1,offered_unix_ms=excluded.offered_unix_ms RETURNING offers",
                params![rid, person.as_str(), now],
                |r| r.get(0),
            )?;
            offers < FOLLOW_UP_OFFERS
        } else {
            false
        };
        if !held {
            tx.execute(
                "INSERT INTO follow_ups(rid,person,asked_unix_ms) VALUES(?1,?2,?3)",
                params![rid, person.as_str(), now],
            )?;
        }
        tx.commit()?;
        let mut given = serde_json::json!({"id":id,"category":category,"text":text});
        if let Some(on) = on {
            given["on"] = on.into();
        }
        if held {
            given["until_asked"] = true.into();
        }
        Ok(Some(given))
    }

    /// The lamp asked about follow-up `id`, given with `until_asked` (#1693,
    /// the first builder on #2602, 22 Oct 09:4x): recorded as given now, so
    /// never given again. Only one of this person's that was offered and
    /// not yet recorded; anything else (another's, unknown, forgotten,
    /// never offered, already recorded) changes nothing and is `false`.
    pub fn follow_up_asked(&mut self, person: &Person, id: u64) -> Result<bool> {
        let now = self.now;
        let marked = self.db.execute(
            "INSERT OR IGNORE INTO follow_ups(rid,person,asked_unix_ms) SELECT e.rid,?1,?3 FROM entries e JOIN follow_up_offers o ON o.rid=e.rid WHERE e.person=?1 AND e.id=?2 AND o.person=?1",
            params![person.as_str(), id, now],
        )?;
        Ok(marked > 0)
    }

    /// The session follow-up `id` was offered in had no greeting to ask it
    /// at: the person's first line asked for something (the voice's
    /// `LAMP_MEMORY_EVENTS`, the first builder on #2602, 22 Oct 22:17), so
    /// it was never put to the model to ask and waits for a later session.
    /// That offer is taken back: one fewer counted (`follow_up_offers`),
    /// and when it was its last, recorded as given in its transaction
    /// (`follow_ups` at the offer's own time), that record goes, so it is
    /// offered again. The time of the offer stays: one a day as ever, so the
    /// next offer is on a later day. Only one of this person's, offered and
    /// not taken back past none; anything else (another's, unknown, never
    /// offered, already asked) changes nothing and is `false`.
    pub fn follow_up_unoffered(&mut self, person: &Person, id: u64) -> Result<bool> {
        let tx = self.db.transaction()?;
        let offer: Option<(i64, u64)> = tx
            .query_row(
                "SELECT o.rid,o.offered_unix_ms FROM follow_up_offers o JOIN entries e ON e.rid=o.rid WHERE e.person=?1 AND e.id=?2 AND o.person=?1 AND o.offers>0",
                params![person.as_str(), id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((rid, offered)) = offer else {
            return Ok(false);
        };
        // Asked about since (`follow_up_asked`, a later time): kept.
        let asked: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM follow_ups WHERE rid=?1 AND asked_unix_ms<>?2)",
            params![rid, offered],
            |r| r.get(0),
        )?;
        if asked {
            return Ok(false);
        }
        tx.execute(
            "DELETE FROM follow_ups WHERE rid=?1 AND asked_unix_ms=?2",
            params![rid, offered],
        )?;
        tx.execute(
            "UPDATE follow_up_offers SET offers=offers-1 WHERE rid=?1",
            [rid],
        )?;
        tx.commit()?;
        Ok(true)
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

    /// A conversation end's sources as this store has them, and how many it
    /// named that it no longer has at all (#2176 row 13): a voice can name
    /// exchanges a forget has since removed, and the end goes on with the
    /// rest rather than being refused, or with none left closes with nothing
    /// to observe (Friday's N12 on #3504). An id another person's scope still has
    /// is kept, for the checks after to refuse as before (the reviewer-lead's
    /// P3 on #2226): a wrong person's ids never look like a forget. Duplicates
    /// and bad numbers are kept for those checks too.
    pub(crate) fn known_sources(&self, _person: &Person, ids: &[u64]) -> Result<(Vec<u64>, usize)> {
        let mut known = Vec::with_capacity(ids.len());
        let mut unknown = 0;
        for id in ids {
            let exists = *id == 0
                || *id > i64::MAX as u64
                || self.db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM entries WHERE id=?)",
                    params![id],
                    |r| r.get(0),
                )?;
            if exists {
                known.push(*id);
            } else {
                unknown += 1;
            }
        }
        Ok((known, unknown))
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
        // Only what rests on the person's own words is kept, never what the
        // lamp alone said (`tidy::said_by_the_person`, finding M on #3504).
        let theirs = tidy::Output {
            observations: output
                .observations
                .iter()
                .filter(|proposal| tidy::said_by_the_person(&input, proposal))
                .cloned()
                .collect(),
        };
        let left_out = output.observations.len() - theirs.observations.len();
        if left_out > 0 {
            eprintln!(
                "lamp-memory: tidy_apply left out {left_out} of {} observations resting only on the lamp's words",
                output.observations.len()
            );
        }
        if theirs.observations.is_empty() {
            tx.execute(
                "UPDATE jobs SET state='done',input='',result='[]' WHERE job=?",
                [job],
            )?;
            tx.commit()?;
            return Ok(vec![]);
        }
        let output = &theirs;
        // What they stated is kept, never a question they asked
        // (`tidy::only_asked`, finding N10 on #3504).
        let stated = tidy::Output {
            observations: output
                .observations
                .iter()
                .filter(|proposal| !tidy::only_asked(&input, proposal))
                .cloned()
                .collect(),
        };
        let asked = output.observations.len() - stated.observations.len();
        if asked > 0 {
            eprintln!(
                "lamp-memory: tidy_apply left out {asked} of {} observations keeping only a question",
                output.observations.len()
            );
        }
        if stated.observations.is_empty() {
            tx.execute(
                "UPDATE jobs SET state='done',input='',result='[]' WHERE job=?",
                [job],
            )?;
            tx.commit()?;
            return Ok(vec![]);
        }
        let output = &stated;
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
            let on = event_day(proposal, at, &self.local);
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
            if let Some(on) = on {
                tx.execute(
                    "INSERT INTO event_days(rid,day) SELECT rid,?3 FROM entries WHERE person=?1 AND id=?2",
                    params![person.as_str(), id, on.to_string()],
                )?;
            }
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
                "SELECT count(*) FROM entries WHERE kind=?2 AND (kind!='kept' OR active=1) AND (person=?1 OR rid IN (SELECT rid FROM adopting))",
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
        // now as `to`'s: their pacing counts it. Its event day, by rid, is
        // still its own.
        tx.execute(
            "UPDATE follow_ups SET person=?1 WHERE rid IN (SELECT rid FROM adopting)",
            [to.as_str()],
        )?;
        tx.execute(
            "UPDATE follow_up_offers SET person=?1 WHERE rid IN (SELECT rid FROM adopting)",
            [to.as_str()],
        )?;
        // Who said a room line (#2849) is the room's alone: a line now
        // someone's own goes with their own forget.
        tx.execute(
            "DELETE FROM speakers WHERE rid IN (SELECT rid FROM adopting)",
            [],
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

/// What an entry keeps beside it in side tables ([`Days::add`]): the day
/// it is about (`event_days`) and who said it (`speakers`, #2849).
struct Said<'a> {
    on: Option<Day>,
    speaker: Option<&'a str>,
}
fn on_said(on: Option<Day>, speaker: Option<&str>) -> Said<'_> {
    Said { on, speaker }
}

/// A write's `speaker` (#2849), checked before anything is written: only
/// on the room's lines, and only the id of one person the eyes know
/// (`lamp_msgs::known::is_person_id`); else `bad_request`.
fn speaker_of<'a>(person: &Person, speaker: Option<&'a str>) -> Result<Option<&'a str>> {
    let Some(speaker) = speaker else {
        return Ok(None);
    };
    if person.as_str() != Person::EVERYONE {
        return Err(bad("a speaker is kept only on the room's lines"));
    }
    if !lamp_msgs::known::is_person_id(speaker) {
        return Err(bad("a speaker is the id of one person the eyes know"));
    }
    Ok(Some(speaker))
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
/// Links `from`, validated as a new remember's are, to the kept thing `id`
/// that `once` found (#2176): forgetting it then removes this
/// conversation's exchanges too. Links it already has stay one. Inside the
/// caller's transaction, with the day it may take ([`Days::remember_on`]).
fn link_sources(tx: &Transaction<'_>, person: &Person, id: u64, from: &[u64]) -> Result<()> {
    validate_sources(tx, person, from, false)?;
    for source in from {
        tx.execute(
            "INSERT OR IGNORE INTO sources(child,parent) SELECT c.rid,p.rid FROM entries c JOIN entries p ON p.person=c.person WHERE c.person=?1 AND c.id=?2 AND p.id=?3",
            params![person.as_str(), id, source],
        )?;
    }
    Ok(())
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
/// At most this many days after its evidence's day, an event's day: a
/// wedding next month, not a plan for next year.
pub const EVENT_WITHIN_DAYS: u32 = 60;

/// A proposal's event day (#1693): a real `YYYY-MM-DD`, of a `thread` or
/// `promise` (a preference has no day), from the local day of its newest
/// evidence `at` to [`EVENT_WITHIN_DAYS`] after it. Any other `on` is
/// dropped and the observation kept undated: a day the model got wrong, or
/// one already past ("my exam was yesterday"), never costs the batch.
fn event_day(proposal: &tidy::Proposal, at: u64, local: &LocalTime) -> Option<Day> {
    if proposal.category == tidy::Category::Preference {
        return None;
    }
    kept_day(proposal.on.as_deref()?, at, local)
}

/// `on` as an event day said at `at`: a real `YYYY-MM-DD` from the local
/// day of `at` to [`EVENT_WITHIN_DAYS`] after it, else `None`. The one
/// check of an observation's day ([`event_day`]) and a kept thing's
/// ([`Days::remember_on`]).
fn kept_day(on: &str, at: u64, local: &LocalTime) -> Option<Day> {
    let on = Day::parse(on)?;
    let said = Day::at(at, local)?;
    let far = on
        .before(EVENT_WITHIN_DAYS)
        .is_some_and(|start| start > said);
    (said <= on && !far).then_some(on)
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

/// What goes with a targeted `selective` forget of a kept thing or an
/// observation (the doomed rows: it and its versions), by each version's
/// distinctive words ([`search::content_words`]):
/// - reached from them through old source links, through rows that stay as
///   well as rows that go, every exchange or observation, of any scope,
///   that carries one word of any version;
/// - of `person`'s own, linked or not, every one that is a copy of one
///   version: all of that version's words, or its whole text.
///
/// A row only shown them, with none of their words, stays (lamp-06c1, 26
/// Oct: a forgotten peanut allergy took 75 exchanges, 73 with no allergy
/// word, all linked because each turn's session was shown it).
///
/// The first builder's condition (#3504): a copy left behind is the
/// failure that matters, so a linked row goes on one shared word, a few
/// unrelated ones with it ("allergic to cats" with "allergic to
/// peanuts"), and a wordless row in between does not stop the walk
/// (Reviewer-two, 03:15). An unlinked row needs a version's every word, or
/// "favourite colour is violet" would take every "favourite" the person
/// said; each version on its own, so a copy of the new "violet" goes
/// without the old "teal". A mixed exchange goes whole. A paraphrase with
/// none of the words ("avoid those") stays: old links say what was shown,
/// not used, and typed provenance is what replaces this. A fact with no
/// distinctive word takes the whole walk ([`descendants`]), as before,
/// and its literal copies too. Kept things never go here unless named.
fn selective_descendants(db: &Connection, person: &Person) -> Result<()> {
    let texts: Vec<String> = db
        .prepare("SELECT text FROM entries WHERE rid IN (SELECT rid FROM doomed)")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let own: Vec<BTreeSet<String>> = texts
        .iter()
        .map(|text| {
            search::content_words(text)
                .into_iter()
                .collect::<BTreeSet<_>>()
        })
        .filter(|words| !words.is_empty())
        .collect();
    // Whole texts, short ones too: a two-letter value has no distinctive
    // word, so its exact copy is found only this way (Reviewer-two, 04:50).
    let literals: Vec<String> = texts
        .iter()
        .map(|text| literal(text))
        .filter(|text| !text.is_empty())
        .collect();
    let any: BTreeSet<String> = own.iter().flatten().cloned().collect();
    let doom = |rid: i64| db.execute("INSERT OR IGNORE INTO doomed VALUES (?1)", [rid]);
    if any.is_empty() {
        descendants(db, None)?;
    } else {
        // Every row reachable through links, those that stay included.
        let mut visited: BTreeSet<i64> = db
            .prepare("SELECT rid FROM doomed")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let mut frontier: Vec<i64> = visited.iter().copied().collect();
        let mut children = db.prepare(
            "SELECT e.rid,e.heard||' '||e.said||' '||e.text FROM sources s JOIN entries e ON e.rid=s.child WHERE s.parent=?1 AND e.kind!='kept'",
        )?;
        while let Some(parent) = frontier.pop() {
            let found: Vec<(i64, String)> = children
                .query_map([parent], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            for (rid, body) in found {
                if !visited.insert(rid) {
                    continue;
                }
                if search::content_words(&body)
                    .iter()
                    .any(|word| any.contains(word))
                {
                    doom(rid)?;
                }
                frontier.push(rid);
            }
        }
    }
    // The person's own copies of a version, linked or not.
    let rows: Vec<(i64, [String; 3])> = db
        .prepare("SELECT rid,heard,said,text FROM entries WHERE person=?1 AND kind!='kept' AND rid NOT IN (SELECT rid FROM doomed)")?
        .query_map([person.as_str()], |r| Ok((r.get(0)?, [r.get(1)?, r.get(2)?, r.get(3)?])))?
        .collect::<rusqlite::Result<_>>()?;
    for (rid, fields) in rows {
        let body = fields.join(" ");
        let words: BTreeSet<String> = search::content_words(&body).into_iter().collect();
        // Whole words only: "it is so" is not in "bit is sour". A text of
        // fewer than three characters is a copy only as a whole field
        // ("OK" is not every line with "ok" in it).
        let whole = format!(" {} ", literal(&body));
        let copied = |text: &String| {
            if text.chars().count() < 3 {
                fields.iter().any(|field| literal(field) == *text)
            } else {
                whole.contains(&format!(" {text} "))
            }
        };
        if own.iter().any(|version| version.is_subset(&words)) || literals.iter().any(copied) {
            doom(rid)?;
        }
    }
    versions(db)
}

/// A text as [`selective_descendants`] compares it whole: [`search::text`]'s
/// form, each run of anything but letters and digits one space, trimmed.
fn literal(text: &str) -> String {
    search::text(text)
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// What goes with the doomed rows: for a forget (`expiry` None), all that
/// derives from them and every version, except a kept thing that isn't
/// itself doomed already (#3113); for expiry, from the first day whose
/// events no longer hold (`expiry`), what derives from them only.
fn descendants(db: &Connection, expiry: Option<&str>) -> Result<()> {
    if let Some(event) = expiry {
        // Expiry: a kept thing never expires, even one remembered with its
        // conversation (#1693): the walk stops at it, and its link to an
        // expired exchange goes by the foreign key (#2176). So does an
        // observation whose event day is on or after `event`: it is kept to
        // its follow-up window. A forget, below, still follows those links.
        db.execute("WITH RECURSIVE chain(rid) AS (SELECT rid FROM doomed UNION SELECT s.child FROM sources s JOIN chain c ON s.parent=c.rid JOIN entries e ON e.rid=s.child WHERE e.kind!='kept' AND NOT EXISTS (SELECT 1 FROM event_days d WHERE d.rid=s.child AND d.day>=?1)) INSERT OR IGNORE INTO doomed SELECT rid FROM chain",[event])?;
    } else {
        // A forget: a kept thing goes only when it is named (the doomed rows
        // this walk starts from, with their versions), never because it
        // derives from a doomed row (#3113). With `LAMP_MEMORY_FORGET_ALL` a
        // kept thing's sources are its conversation's exchanges, so a named
        // exchange would take every kept thing said in its conversation. Its
        // link to the exchange goes by the foreign key, as on expiry.
        db.execute("WITH RECURSIVE chain(rid) AS (SELECT rid FROM doomed UNION SELECT s.child FROM sources s JOIN chain c ON s.parent=c.rid JOIN entries e ON e.rid=s.child WHERE e.kind!='kept' UNION SELECT r.newer FROM replacements r JOIN chain c ON r.older=c.rid UNION SELECT r.older FROM replacements r JOIN chain c ON r.newer=c.rid) INSERT OR IGNORE INTO doomed SELECT rid FROM chain",[])?;
    }
    Ok(())
}
/// Every entry of `person`, or of every person for `None`, erased in the
/// caller's transaction, with their session notes, jobs and skipped count.
fn empty(tx: &Transaction<'_>, person: Option<&Person>) -> Result<u64> {
    tx.execute("DELETE FROM doomed", [])?;
    tx.execute(
        "INSERT INTO doomed SELECT rid FROM entries WHERE ?1 IS NULL OR person=?1",
        [person.map(Person::as_str)],
    )?;
    let count = erase(tx)?;
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
    Ok(count)
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

    /// lamp-06c1, 26 Oct (75 exchanges went with one allergy): a targeted
    /// forget takes every exchange of theirs that carries the fact's words,
    /// linked to it or not, and none that was only shown it (the first
    /// builder's condition on #3504, both ways).
    #[test]
    fn targeted_forget_keeps_unrelated_exchanges_shown_the_fact() {
        let dir =
            std::env::temp_dir().join(format!("lamp-days-selective-forget-{}", std::process::id()));
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
        let person = Person::new("p-0123456789abcdef").unwrap();
        let evidence = store
            .append(
                &person,
                now,
                "I am allergic to peanuts",
                "I will remember",
                &[],
            )
            .unwrap();
        let allergy = store
            .remember_from(&person, now, "allergic to peanuts", &[evidence])
            .unwrap();
        let weather = store
            .append(
                &person,
                now + 1,
                "Will it rain tomorrow?",
                "Sunny skies",
                &[allergy],
            )
            .unwrap();
        let related = store
            .append(
                &person,
                now + 2,
                "Can I eat peanuts?",
                "Avoid peanuts because of your allergy",
                &[allergy],
            )
            .unwrap();
        let later_weather = store
            .append(&person, now + 3, "What about Friday?", "Cloudy", &[related])
            .unwrap();
        let allergy_reply = store
            .append(&person, now + 4, "Do I have an allergy?", "Yes", &[allergy])
            .unwrap();
        // Never linked to the fact, but a copy of it all the same; and one
        // with only some of its words, never linked, stays.
        let unlinked = store
            .append(
                &person,
                now + 5,
                "Remember I'm allergic to peanuts",
                "Noted",
                &[],
            )
            .unwrap();
        let some_words = store
            .append(
                &person,
                now + 6,
                "I bought peanuts for the party",
                "Nice",
                &[],
            )
            .unwrap();
        let unlinked_weather = store
            .append(&person, now + 7, "Is it windy?", "A little", &[])
            .unwrap();
        let unrelated = store.remember(&person, now, "likes jazz").unwrap();
        assert_eq!(store.forget(&person, allergy).unwrap(), 5);
        for id in [evidence, allergy, related, allergy_reply, unlinked] {
            assert!(store.record(&person, id).unwrap().is_none(), "{id}");
        }
        for id in [
            weather,
            later_weather,
            unlinked_weather,
            some_words,
            unrelated,
        ] {
            assert!(store.record(&person, id).unwrap().is_some(), "{id}");
        }
        // What a search for the fact still finds shares one of its words at
        // most, and is no copy of it.
        let found = store
            .search(
                &person,
                &Ask {
                    query: Some("allergic peanuts"),
                    day: None,
                    limit: 8,
                    max_bytes: 4096,
                },
                false,
            )
            .unwrap();
        assert_eq!(
            found.iter().map(|record| record.id).collect::<Vec<_>>(),
            [some_words]
        );
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    fn scratch(name: &str) -> (std::path::PathBuf, Days, Person, u64) {
        let dir = std::env::temp_dir().join(format!("lamp-days-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let now = 1_791_892_800_000;
        let store = Days::open(
            Options {
                dir: dir.clone(),
                keep_days: 30,
                local: LocalTime::Offset(0),
            },
            now,
        )
        .unwrap();
        (dir, store, Person::new("p-0123456789abcdef").unwrap(), now)
    }

    /// A forget without `selective` (an older voice, or one with
    /// `LAMP_MEMORY_LINK_USED`) is as before: a linked follow-up without the
    /// fact's words goes, an unlinked copy stays. With it, the other way.
    #[test]
    fn without_selective_a_forget_is_as_before() {
        for selective in [false, true] {
            let (dir, mut store, person, now) = scratch(&format!("selective-{selective}"));
            let teal = store
                .remember(&person, now, "My favourite colour is teal")
                .unwrap();
            let follow_up = store
                .append(
                    &person,
                    now + 1,
                    "why do I like it?",
                    "It reminds you of the sea",
                    &[teal],
                )
                .unwrap();
            let copy = store
                .append(
                    &person,
                    now + 2,
                    "my favourite colour is teal",
                    "Noted",
                    &[],
                )
                .unwrap();
            store.forget_linked(&person, teal, selective).unwrap();
            assert_eq!(store.record(&person, copy).unwrap().is_none(), selective);
            assert_eq!(
                store.record(&person, follow_up).unwrap().is_none(),
                !selective
            );
            drop(store);
            fs::remove_dir_all(dir).unwrap();
        }
    }

    /// Reviewer-two, 03:15 on #3613: (a) a wordless row that stays does not
    /// stop the walk, so "avoid peanuts" linked through it goes; (b) a copy
    /// of the newer version goes on its own words, without the older's; (c)
    /// a fact with no distinctive word still takes its literal copies.
    #[test]
    fn the_walk_goes_through_rows_that_stay_and_copies_match_one_version() {
        let (dir, mut store, person, now) = scratch("selective-walk");
        let allergy = store.remember(&person, now, "allergic to peanuts").unwrap();
        let bridge = store
            .append(&person, now + 1, "What's the weather?", "Sunny", &[allergy])
            .unwrap();
        let avoid = store
            .append(
                &person,
                now + 2,
                "Can I have that?",
                "Avoid peanuts",
                &[bridge],
            )
            .unwrap();
        store.forget(&person, allergy).unwrap();
        assert!(store.record(&person, bridge).unwrap().is_some());
        assert!(store.record(&person, avoid).unwrap().is_none());

        let teal = store
            .remember(&person, now, "My favourite colour is teal")
            .unwrap();
        let violet = store
            .remember_replacing(
                &person,
                now + 3,
                "My favourite colour is violet",
                &[],
                teal,
                &[teal],
            )
            .unwrap();
        let copy = store
            .append(&person, now + 4, "favourite colour violet", "Noted", &[])
            .unwrap();
        store.forget(&person, violet).unwrap();
        assert!(store.record(&person, copy).unwrap().is_none());

        let ok = store.remember(&person, now, "it is so").unwrap();
        let literal = store
            .append(&person, now + 5, "Remember: it is so!", "Fine", &[])
            .unwrap();
        let other = store.append(&person, now + 6, "is it?", "So", &[]).unwrap();
        store.forget(&person, ok).unwrap();
        assert!(store.record(&person, literal).unwrap().is_none());
        assert!(store.record(&person, other).unwrap().is_some());

        // A two-letter value (Reviewer-two, 04:50 on #3613): its exact
        // copy goes; a line that only has it in a longer word, or among
        // other words, stays.
        let short = store.remember(&person, now, "B2").unwrap();
        let copy = store.append(&person, now + 7, "b2", "Noted", &[]).unwrap();
        let longer = store.append(&person, now + 8, "B22", "Noted", &[]).unwrap();
        let among = store
            .append(&person, now + 9, "Seat B2 is free", "Okay", &[])
            .unwrap();
        let unrelated = store.append(&person, now + 10, "Hello", "Hi", &[]).unwrap();
        store.forget(&person, short).unwrap();
        assert!(store.record(&person, copy).unwrap().is_none());
        for id in [longer, among, unrelated] {
            assert!(store.record(&person, id).unwrap().is_some(), "{id}");
        }
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    /// #2176 (5a): the room's exchange and kept thing become A's, with the
    /// link between them, their search rows, and an exchange derived only
    /// from them; one also derived from something staying stays, its link
    /// kept. Targeted forgetting keeps the mixed line whose words do not
    /// expose the named fact. Another person's ids,
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
        // Still the room's, still linked: its words do not expose this fact.
        assert!(store.record(&room, mixed).unwrap().is_some());
        assert_eq!(
            store.adopt(&room, &a, &[derived]).unwrap_err().code,
            "bad_request"
        );
        assert_eq!(store.forget(&a, 4).unwrap(), 3);
        assert!(store.record(&room, mixed).unwrap().is_some());
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
