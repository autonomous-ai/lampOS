use crate::{Error, Result, schema, types::*};
use hmac::{Hmac, Mac};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sha2::Sha256;
use std::io::{Read, Write};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Duration,
};
use unicode_normalization::UnicodeNormalization;

/// One private database per owner. Callers drive scheduling; no background
/// thread or model call can hold this connection while inference is running.
pub struct Store {
    pub(crate) conn: Connection,
    owner: String,
    fingerprint_key: [u8; 32],
    validity: tokio::sync::watch::Sender<u64>,
    _lock: Option<std::fs::File>,
}

impl Store {
    pub fn open(path: impl AsRef<Path>, owner: &str) -> Result<Self> {
        let path = path.as_ref();
        if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(Error::Invalid("database path is a symlink"));
        }
        let mut lock_path = path.as_os_str().to_os_string();
        lock_path.push(".lock");
        let lock = private_file(Path::new(&lock_path))?;
        lock.try_lock()
            .map_err(|_| Error::Invalid("database is already open"))?;
        let populated = std::fs::metadata(path).is_ok_and(|m| m.len() > 0);
        let fingerprint_key = load_fingerprint_key(path, populated)?;
        let database = private_file(path)?;
        drop(database);
        let mut store = Self::from_connection(Connection::open(path)?, owner, fingerprint_key)?;
        store._lock = Some(lock);
        Ok(store)
    }
    pub fn in_memory(owner: &str) -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?, owner, random_key()?)
    }
    fn from_connection(conn: Connection, owner: &str, fingerprint_key: [u8; 32]) -> Result<Self> {
        text_valid(owner, 64)?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version != 0 && version != 2 {
            return Err(Error::Invalid(
                "unsupported database schema; no automatic fingerprint reset",
            ));
        }
        conn.busy_timeout(Duration::from_millis(25))?;
        conn.execute_batch(schema::SQL)?;
        conn.execute("INSERT OR IGNORE INTO meta(one,owner) VALUES(1,?)", [owner])?;
        let saved: String =
            conn.query_row("SELECT owner FROM meta WHERE one=1", [], |r| r.get(0))?;
        if saved != owner {
            return Err(Error::Invalid("database belongs to another owner"));
        }
        let key_check = digest(&fingerprint_key, "memory-tiers fingerprint key check v1");
        conn.execute(
            "INSERT OR IGNORE INTO settings VALUES('fingerprint_key_check',?)",
            [&key_check],
        )?;
        let saved_check: String = conn.query_row(
            "SELECT value FROM settings WHERE name='fingerprint_key_check'",
            [],
            |r| r.get(0),
        )?;
        if saved_check != key_check {
            return Err(Error::Invalid("fingerprint key does not match store"));
        }
        Ok(Self {
            conn,
            fingerprint_key,
            owner: owner.into(),
            validity: tokio::sync::watch::channel(0).0,
            _lock: None,
        })
    }
    pub fn pause(&mut self, paused: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE meta SET paused=?,generation=generation+1 WHERE one=1",
            [paused],
        )?;
        self.invalidate();
        Ok(())
    }
    /// Set the fixture/owner's named zone before ingestion. Fix it for the
    /// lifetime of a populated store so changing zones cannot reset budgets.
    pub fn set_time_zone(&mut self, name: &str) -> Result<()> {
        if name == "system" {
            return Err(Error::Invalid("memory needs an explicit time zone"));
        }
        lamp_local_time::LocalTime::zone(name)?;
        let populated: bool =
            self.conn
                .query_row("SELECT EXISTS(SELECT 1 FROM records)", [], |r| r.get(0))?;
        let current: String = self.conn.query_row(
            "SELECT value FROM settings WHERE name='time_zone'",
            [],
            |r| r.get(0),
        )?;
        if populated && name != current {
            return Err(Error::Invalid("time zone is fixed after ingestion"));
        }
        self.conn
            .execute("UPDATE settings SET value=? WHERE name='time_zone'", [name])?;
        self.invalidate();
        Ok(())
    }
    pub fn ingest(&mut self, incoming: Incoming) -> Result<Option<i64>> {
        let paused: bool = self
            .conn
            .query_row("SELECT paused FROM meta WHERE one=1", [], |r| r.get(0))?;
        if paused
            || !incoming.addressed_to_lamp
            || matches!(incoming.speaker, Speaker::Visitor | Speaker::Unsure)
        {
            return Ok(None);
        }
        text_valid(&incoming.text, MAX_RECORD_BYTES)?;
        text_valid(&incoming.topic, MAX_TEXT_BYTES)?;
        valid_time(incoming.at_ms)?;
        if incoming
            .stt_confidence
            .is_some_and(|value| !(0.0..=1.0).contains(&value))
        {
            return Err(Error::Invalid("invalid STT confidence"));
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if blocked(&tx, &self.fingerprint_key, &incoming.text)? {
            return Ok(None);
        }
        let open: Option<(i64, String)> = tx
            .query_row(
                "SELECT id,topic FROM episodes WHERE ended_ms IS NULL ORDER BY id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let episode_id = match open {
            Some((id, topic)) if topic == incoming.topic => id,
            _ => {
                tx.execute(
                    "UPDATE episodes SET ended_ms=max(started_ms,?) WHERE ended_ms IS NULL",
                    [incoming.at_ms],
                )?;
                tx.execute(
                    "INSERT INTO episodes(topic,started_ms,importance) VALUES(?,?,?)",
                    params![
                        incoming.topic,
                        incoming.at_ms,
                        if incoming.remember { 1.0 } else { 0.0 }
                    ],
                )?;
                tx.last_insert_rowid()
            }
        };
        if incoming.remember {
            tx.execute("UPDATE episodes SET importance=1 WHERE id=?", [episode_id])?;
        }
        tx.execute(
            "INSERT INTO records(episode_id,at_ms,speaker,body) VALUES(?,?,?,'')",
            params![episode_id, incoming.at_ms, encode(&incoming.speaker)?],
        )?;
        let id = tx.last_insert_rowid();
        let record = Record {
            id,
            episode_id,
            strength: incoming.strength.constrained_by_words(&incoming.text),
            text: incoming.text,
            speaker: incoming.speaker,
            at_ms: incoming.at_ms,
            stt_confidence: incoming.stt_confidence,
        };
        tx.execute(
            "UPDATE records SET body=? WHERE id=?",
            params![encode(&record)?, id],
        )?;
        index(&tx, "record", id, &record.text)?;
        tx.commit()?;
        Ok(Some(id))
    }
    pub fn owner_left(&mut self, at_ms: i64) -> Result<()> {
        valid_time(at_ms)?;
        self.conn.execute(
            "UPDATE episodes SET ended_ms=max(started_ms,?) WHERE ended_ms IS NULL",
            [at_ms],
        )?;
        Ok(())
    }
    pub fn records(&self, access: Access) -> Result<Vec<Record>> {
        if !access.permits() {
            return Ok(Vec::new());
        }
        records_where(&self.conn, "SELECT body FROM records ORDER BY id", [])
    }
    pub fn facts(&self, access: Access) -> Result<Vec<Fact>> {
        if !access.permits() {
            return Ok(Vec::new());
        }
        facts_where(&self.conn, "SELECT body FROM facts ORDER BY id", [])
    }
    pub fn lines(&self, access: Access) -> Result<Vec<MemoryLine>> {
        if !access.permits() {
            return Ok(Vec::new());
        }
        lines_where(&self.conn, "SELECT body FROM lines ORDER BY id", [])
    }
    pub fn episodes(&self, access: Access) -> Result<Vec<Episode>> {
        if !access.permits() {
            return Ok(Vec::new());
        }
        episodes(&self.conn)
    }
    /// Session-start snapshot. Tentative and planned items carry their strength,
    /// and every line still carries its evidence. Visitors receive an empty view.
    pub fn owner_model(&mut self, access: Access, now_ms: i64) -> Result<Vec<MemoryLine>> {
        if !access.permits() {
            return Ok(Vec::new());
        }
        self.expire(now_ms)?;
        lines_where(
            &self.conn,
            "SELECT body FROM lines WHERE kind=? ORDER BY id",
            [encode(&LineKind::OwnerModel)?],
        )
    }
    pub fn prepare_sleep(&mut self, now_ms: i64, kind: PassKind) -> Result<Option<SleepInput>> {
        valid_time(now_ms)?;
        self.expire(now_ms)?;
        let time_zone: String = self.conn.query_row(
            "SELECT value FROM settings WHERE name='time_zone'",
            [],
            |r| r.get(0),
        )?;
        let cycle = lamp_local_time::LocalTime::zone(&time_zone)?.day(now_ms as u64);
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (generation, revision, paused): (i64, i64, bool) = tx.query_row(
            "SELECT generation,revision,paused FROM meta WHERE one=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        if paused {
            return Ok(None);
        }
        // Filter evidence before applying the row limit. Unsure and assistant
        // words are never sent as owner evidence, even in weekly reflection.
        if kind == PassKind::Weekly {
            tx.execute("INSERT OR IGNORE INTO weekly_progress(cycle,last_id) SELECT ?,coalesce(max(id),0) FROM records WHERE at_ms<=?",params![cycle,now_ms])?;
        }
        let candidates = pending_records(&tx, now_ms, kind, cycle)?;
        let mut input = SleepInput {
            validity: self.validity.subscribe(),
            epoch: *self.validity.borrow(),
            owner: self.owner.clone(),
            generation,
            revision,
            now_ms,
            cycle,
            time_zone,
            kind,
            records: Vec::new(),
            facts: Vec::new(),
            lines: Vec::new(),
            episodes: Vec::new(),
        };
        for record in candidates {
            if blocked(&tx, &self.fingerprint_key, &record.text)? {
                continue;
            }
            input.records.push(record);
            if input.json().is_err() {
                input.records.pop();
                break;
            }
        }
        if input.records.is_empty() {
            tx.commit()?;
            return Ok(None);
        }
        // The full owner model fits in 2 KiB. Keep its evidence in the request;
        // historical facts are context only, never independent source words.
        let mut previous = lines_where(
            &tx,
            "SELECT body FROM lines WHERE kind=? ORDER BY id",
            [encode(&LineKind::OwnerModel)?],
        )?;
        previous.truncate(60);
        for line in previous {
            let old_facts = input.facts.len();
            for id in &line.fact_ids {
                if !input.facts.iter().any(|f| f.id == *id) {
                    input.facts.push(fact(&tx, *id)?);
                }
            }
            input.lines.push(line);
            if input.json().is_err() {
                input.lines.pop();
                input.facts.truncate(old_facts);
                break;
            }
        }
        for value in facts_where(
            &tx,
            "SELECT body FROM facts WHERE active=1 ORDER BY id DESC LIMIT 64",
            [],
        )? {
            if input.facts.iter().any(|f| f.id == value.id)
                || blocked(&tx, &self.fingerprint_key, &value.claim)?
            {
                continue;
            }
            input.facts.push(value);
            if input.json().is_err() {
                input.facts.pop();
                break;
            }
        }
        let ids: BTreeSet<_> = input.records.iter().map(|r| r.episode_id).collect();
        for id in ids {
            let mut ep = episode_row(&tx, id)?;
            ep.record_ids = input
                .records
                .iter()
                .filter(|r| r.episode_id == id)
                .map(|r| r.id)
                .collect();
            ep.detail.clear();
            input.episodes.push(ep);
            if input.json().is_err() {
                input.episodes.pop();
                break;
            }
        }
        tx.commit()?;
        Ok(Some(input))
    }
    /// The model never mutates this store. Its bounded JSON is validated inside
    /// the same write transaction as every copied quote and changed line.
    pub fn apply_sleep(&mut self, input: &SleepInput, json: &str) -> Result<ApplyReport> {
        input.check_current()?;
        let proposal = SleepProposal::parse(json)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (generation, revision, paused): (i64, i64, bool) = tx.query_row(
            "SELECT generation,revision,paused FROM meta WHERE one=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        if input.owner != self.owner
            || input.generation != generation
            || input.revision != revision
            || paused
        {
            return Err(Error::Stale);
        }
        let mut sources = BTreeMap::new();
        for source in &input.records {
            let current: Option<String> = tx
                .query_row("SELECT body FROM records WHERE id=?", [source.id], |r| {
                    r.get(0)
                })
                .optional()?;
            if current.as_deref() != Some(encode(source)?.as_str())
                || blocked(&tx, &self.fingerprint_key, &source.text)?
            {
                return Err(Error::Stale);
            }
            sources.insert(source.id, source);
        }
        let old_lines = lines_where(
            &tx,
            "SELECT body FROM lines WHERE kind=?",
            [encode(&LineKind::OwnerModel)?],
        )?;
        let mut changed = BTreeSet::new();
        let mut retired = BTreeSet::new();
        let mut keys = BTreeMap::new();
        let mut report = ApplyReport::default();
        for p in proposal.facts {
            text_valid(&p.key, 64)?;
            text_valid(&p.claim, MAX_TEXT_BYTES)?;
            if keys.contains_key(&p.key) {
                return Err(Error::Invalid("duplicate fact key"));
            }
            if blocked(&tx, &self.fingerprint_key, &p.claim)? {
                return Err(Error::Invalid("forgotten fact"));
            }
            unique_nonempty(&p.record_ids)?;
            valid_time(p.true_since_ms)?;
            if p.true_since_ms > input.now_ms {
                return Err(Error::Invalid("future true-since"));
            }
            if let Some(until) = p.true_until_ms {
                valid_time(until)?;
                if until <= p.true_since_ms {
                    return Err(Error::Invalid("invalid validity interval"));
                }
            }
            let mut strength = p.strength.constrained_by_words(&p.claim);
            let mut quotes = Vec::new();
            for id in &p.record_ids {
                let record = sources
                    .get(id)
                    .ok_or(Error::Invalid("fact cites a record outside this batch"))?;
                if record.speaker != Speaker::Owner {
                    return Err(Error::Invalid("only owner words prove owner facts"));
                }
                strength = strength.combine(record.strength);
                quotes.push(Quote {
                    record_id: record.id,
                    text: record.text.clone(),
                    speaker: record.speaker,
                    at_ms: record.at_ms,
                    stt_confidence: record.stt_confidence,
                    strength: record.strength,
                });
            }
            if p.kind == FactKind::Plan || p.kind == FactKind::Appointment {
                strength = strength.combine(Strength::Plan);
            }
            let expires = match p.kind {
                FactKind::Plan => Some(
                    quotes
                        .iter()
                        .map(|q| q.at_ms)
                        .max()
                        .unwrap_or(input.now_ms)
                        .checked_add(14 * DAY_MS)
                        .ok_or(Error::Invalid("time overflow"))?,
                ),
                FactKind::Appointment => Some(
                    p.true_until_ms
                        .ok_or(Error::Invalid("appointment needs an absolute end time"))?,
                ),
                _ => p.true_until_ms,
            };
            if let Some(id) = p.supersedes {
                if !input.facts.iter().any(|f| f.id == id) || !retired.insert(id) {
                    return Err(Error::Invalid("unknown or repeated superseded fact"));
                }
                let mut old = fact(&tx, id)?;
                if !old.active
                    || p.true_since_ms < old.true_since_ms
                    || quotes.iter().map(|q| (q.at_ms, q.record_id)).max()
                        <= old.quotes.iter().map(|q| (q.at_ms, q.record_id)).max()
                {
                    return Err(Error::Invalid("replacement needs newer owner evidence"));
                }
                old.active = false;
                old.true_until_ms = Some(p.true_since_ms);
                for line in &old_lines {
                    if line.fact_ids.contains(&id) {
                        changed.insert(line.id);
                    }
                }
                tx.execute(
                    "UPDATE facts SET body=?,active=0 WHERE id=?",
                    params![encode(&old)?, id],
                )?;
                tx.execute("DELETE FROM lines WHERE id IN (SELECT line_id FROM line_sources WHERE fact_id=?)", [id])?;
            }
            let active = expires.is_none_or(|end| end > input.now_ms);
            tx.execute(
                "INSERT INTO facts(body,active,expires_ms,used_ms) VALUES('',?,?,?)",
                params![active, expires, input.now_ms],
            )?;
            let id = tx.last_insert_rowid();
            let fact = Fact {
                id,
                claim: p.claim,
                strength,
                kind: p.kind,
                true_since_ms: p.true_since_ms,
                true_until_ms: p.true_until_ms,
                expires_ms: expires,
                supersedes: p.supersedes,
                active,
                quotes,
            };
            tx.execute(
                "UPDATE facts SET body=? WHERE id=?",
                params![encode(&fact)?, id],
            )?;
            for source in &p.record_ids {
                tx.execute("INSERT INTO fact_sources VALUES(?,?)", params![id, source])?;
            }
            index(&tx, "fact", id, &fact.claim)?;
            keys.insert(p.key, id);
            report.fact_ids.push(id);
        }
        let mut replaced = BTreeSet::new();
        for p in proposal.lines {
            text_valid(&p.text, MAX_TEXT_BYTES)?;
            if p.fact_ids.len() + p.fact_keys.len() > MAX_CITATIONS {
                return Err(Error::Invalid("too many line citations"));
            }
            let mut ids = p.fact_ids.clone();
            for id in &p.fact_ids {
                if !input.facts.iter().any(|f| f.id == *id) {
                    return Err(Error::Invalid("line cites unoffered fact"));
                }
            }
            for key in &p.fact_keys {
                ids.push(*keys.get(key).ok_or(Error::Invalid("unknown fact key"))?);
            }
            unique_nonempty(&ids)?;
            let mut evidence = Vec::new();
            let mut strength = Strength::Said;
            for id in &ids {
                let f = fact(&tx, *id)?;
                if !f.active || f.expires_ms.is_some_and(|end| end <= input.now_ms) {
                    return Err(Error::Invalid("line cites inactive fact"));
                }
                strength = strength.combine(f.strength);
                evidence.push(f);
            }
            if p.kind == LineKind::LearnedPattern
                && evidence
                    .iter()
                    .flat_map(|f| f.quotes.iter().map(|q| q.record_id))
                    .collect::<BTreeSet<_>>()
                    .len()
                    < 2
            {
                return Err(Error::Invalid(
                    "a learned pattern needs repeated owner evidence",
                ));
            }
            let text = if p.kind == LineKind::TaughtRule {
                if evidence.len() != 1 || evidence[0].quotes.len() != 1 {
                    return Err(Error::Invalid("taught rule needs one exact source"));
                }
                evidence[0].quotes[0].text.clone()
            } else {
                p.text
            };
            if blocked(&tx, &self.fingerprint_key, &text)? {
                return Err(Error::Invalid("forgotten memory line"));
            }
            if let Some(id) = p.replaces {
                if !replaced.insert(id) {
                    return Err(Error::Invalid("line replaced twice"));
                }
                let old = input
                    .lines
                    .iter()
                    .find(|l| l.id == id)
                    .ok_or(Error::Invalid("line replacement not in input"))?;
                if old.kind != p.kind {
                    return Err(Error::Invalid("line replacement changes kind"));
                }
                if old.kind == LineKind::OwnerModel && (old.text != text || old.fact_ids != ids) {
                    changed.insert(id);
                }
                tx.execute("DELETE FROM line_sources WHERE line_id=?", [id])?;
            }
            let id = if let Some(id) = p.replaces {
                // A superseded fact may already have removed this line. Keep
                // the identity so the nightly rewrite accounting stays exact.
                tx.execute(
                    "INSERT OR IGNORE INTO lines(id,kind,body) VALUES(?,?,'')",
                    params![id, encode(&p.kind)?],
                )?;
                id
            } else {
                tx.execute(
                    "INSERT INTO lines(kind,body) VALUES(?,'')",
                    [encode(&p.kind)?],
                )?;
                tx.last_insert_rowid()
            };
            let line = MemoryLine {
                id,
                kind: p.kind,
                text,
                strength,
                fact_ids: ids,
            };
            tx.execute(
                "UPDATE lines SET body=? WHERE id=?",
                params![encode(&line)?, id],
            )?;
            for fact_id in &line.fact_ids {
                tx.execute("INSERT INTO line_sources VALUES(?,?)", params![id, fact_id])?;
            }
            index(&tx, "line", id, &line.text)?;
            report.line_ids.push(id);
        }
        check_rewrites(&tx, input.cycle, &old_lines, &changed)?;
        let model = lines_where(
            &tx,
            "SELECT body FROM lines WHERE kind=?",
            [encode(&LineKind::OwnerModel)?],
        )?;
        if model.len() > 60 || encode(&model)?.len() > MAX_OWNER_MODEL_BYTES {
            return Err(Error::Invalid("owner model exceeds 2 KiB or 60 lines"));
        }
        let mut episode_ids = BTreeSet::new();
        for p in proposal.episodes {
            if !episode_ids.insert(p.episode_id)
                || !input.episodes.iter().any(|e| e.id == p.episode_id)
            {
                return Err(Error::Invalid("unknown or repeated episode"));
            }
            text_valid(&p.gist, MAX_TEXT_BYTES)?;
            unique_nonempty(&p.record_ids)?;
            if !(0.0..=1.0).contains(&p.importance) || blocked(&tx, &self.fingerprint_key, &p.gist)?
            {
                return Err(Error::Invalid("invalid episode summary"));
            }
            let mut detail = String::new();
            for id in &p.record_ids {
                let r = sources
                    .get(id)
                    .ok_or(Error::Invalid("episode source outside batch"))?;
                if r.episode_id != p.episode_id || r.speaker != Speaker::Owner {
                    return Err(Error::Invalid("episode source mismatch"));
                }
                if !detail.is_empty() {
                    detail.push('\n');
                }
                detail.push_str(&r.text);
            }
            tx.execute(
                "UPDATE episodes SET gist=?,detail=?,importance=max(importance,?) WHERE id=?",
                params![p.gist, detail, p.importance, p.episode_id],
            )?;
            index(&tx, "episode", p.episode_id, &p.gist)?;
        }
        if input.kind == PassKind::Nightly {
            for id in sources.keys() {
                tx.execute("UPDATE records SET processed=1 WHERE id=?", [id])?;
            }
        } else if let Some(last) = sources.keys().next_back() {
            tx.execute(
                "UPDATE weekly_progress SET cursor=max(cursor,?) WHERE cycle=?",
                params![last, input.cycle],
            )?;
        }
        report.records_processed = sources.len();
        report.pending = !pending_records(&tx, input.now_ms, input.kind, input.cycle)?.is_empty();
        tx.execute("UPDATE meta SET revision=revision+1 WHERE one=1", [])?;
        tx.commit()?;
        self.invalidate();
        Ok(report)
    }

    /// Explicit keeps do not depend on model availability. The owner source is
    /// both the claim and its exact evidence; the caller can read it back.
    pub fn remember(&mut self, id: i64, kind: FactKind, until_ms: Option<i64>) -> Result<i64> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let paused: bool = tx.query_row("SELECT paused FROM meta WHERE one=1", [], |r| r.get(0))?;
        if paused {
            return Err(Error::Invalid("memory is paused"));
        }
        let body: String =
            tx.query_row("SELECT body FROM records WHERE id=?", [id], |r| r.get(0))?;
        let record: Record = decode(&body)?;
        if record.speaker != Speaker::Owner || blocked(&tx, &self.fingerprint_key, &record.text)? {
            return Err(Error::Invalid("explicit keep requires current owner words"));
        }
        if let Some(until) = until_ms {
            valid_time(until)?;
            if until <= record.at_ms {
                return Err(Error::Invalid("invalid validity interval"));
            }
        }
        let expires = match kind {
            FactKind::Appointment => {
                Some(until_ms.ok_or(Error::Invalid("appointment needs an absolute end time"))?)
            }
            FactKind::Plan => Some(
                record
                    .at_ms
                    .checked_add(14 * DAY_MS)
                    .ok_or(Error::Invalid("time overflow"))?,
            ),
            _ => until_ms,
        };
        let strength = if matches!(kind, FactKind::Plan | FactKind::Appointment) {
            record.strength.combine(Strength::Plan)
        } else {
            record.strength
        };
        tx.execute(
            "INSERT INTO facts(body,active,expires_ms,used_ms) VALUES('',1,?,?)",
            params![expires, record.at_ms],
        )?;
        let fact_id = tx.last_insert_rowid();
        let value = Fact {
            id: fact_id,
            claim: record.text.clone(),
            strength,
            kind,
            true_since_ms: record.at_ms,
            true_until_ms: until_ms,
            expires_ms: expires,
            supersedes: None,
            active: true,
            quotes: vec![Quote {
                record_id: id,
                text: record.text,
                speaker: record.speaker,
                at_ms: record.at_ms,
                stt_confidence: record.stt_confidence,
                strength: record.strength,
            }],
        };
        tx.execute(
            "UPDATE facts SET body=? WHERE id=?",
            params![encode(&value)?, fact_id],
        )?;
        tx.execute("INSERT INTO fact_sources VALUES(?,?)", params![fact_id, id])?;
        index(&tx, "fact", fact_id, &value.claim)?;
        tx.execute("UPDATE meta SET revision=revision+1 WHERE one=1", [])?;
        tx.commit()?;
        self.invalidate();
        Ok(fact_id)
    }
    pub fn expire(&mut self, now_ms: i64) -> Result<usize> {
        valid_time(now_ms)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let expired = facts_where(
            &tx,
            "SELECT body FROM facts WHERE active=1 AND expires_ms<=?",
            [now_ms],
        )?;
        for mut f in expired.clone() {
            f.active = false;
            tx.execute(
                "UPDATE facts SET body=?,active=0 WHERE id=?",
                params![encode(&f)?, f.id],
            )?;
            tx.execute(
                "DELETE FROM lines WHERE id IN (SELECT line_id FROM line_sources WHERE fact_id=?)",
                [f.id],
            )?;
        }
        if !expired.is_empty() {
            tx.execute("UPDATE meta SET revision=revision+1 WHERE one=1", [])?;
        }
        tx.commit()?;
        if !expired.is_empty() {
            self.invalidate();
        }
        Ok(expired.len())
    }
    /// Deterministic fading removes derived detail only, never the original
    /// record. A recent recall protects the episode for another 90 days.
    pub fn fade(&mut self, now_ms: i64) -> Result<usize> {
        valid_time(now_ms)?;
        Ok(self.conn.execute("UPDATE episodes SET detail='' WHERE detail<>'' AND importance<0.5 AND ended_ms<? AND last_recalled_ms<?",
            params![now_ms.saturating_sub(90*DAY_MS),now_ms.saturating_sub(90*DAY_MS)])?)
    }
    /// Forget is a code operation, unavailable in model JSON. It removes source
    /// rows, facts and every dependent view. Fingerprints contain hashes only.
    pub fn forget_records(&mut self, ids: &[i64]) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let all = records_where(&tx, "SELECT body FROM records ORDER BY id", [])?;
        let mut doomed: BTreeSet<i64> = ids.iter().copied().collect();
        for id in &doomed {
            let r = all
                .iter()
                .find(|r| r.id == *id)
                .ok_or(Error::Invalid("unknown record to forget"))?;
            fingerprint(&tx, &self.fingerprint_key, &r.text)?;
        }
        let all_facts = facts_where(&tx, "SELECT body FROM facts", [])?;
        let all_lines = lines_where(&tx, "SELECT body FROM lines", [])?;
        let all_episodes = episodes(&tx)?;
        let mut affected = BTreeSet::new();
        let mut affected_lines = BTreeSet::new();
        let mut affected_episodes = BTreeSet::new();
        loop {
            let before = (
                doomed.len(),
                affected.len(),
                affected_lines.len(),
                affected_episodes.len(),
            );
            for r in &all {
                if (doomed.contains(&r.id) || blocked(&tx, &self.fingerprint_key, &r.text)?)
                    && doomed.insert(r.id)
                {
                    fingerprint(&tx, &self.fingerprint_key, &r.text)?;
                }
            }
            for f in &all_facts {
                if f.quotes.iter().any(|q| doomed.contains(&q.record_id))
                    || affected.contains(&f.id)
                    || blocked(&tx, &self.fingerprint_key, &f.claim)?
                {
                    if affected.insert(f.id) {
                        fingerprint(&tx, &self.fingerprint_key, &f.claim)?;
                    }
                    for quote in &f.quotes {
                        if doomed.insert(quote.record_id) {
                            fingerprint(&tx, &self.fingerprint_key, &quote.text)?;
                        }
                    }
                }
            }
            for line in &all_lines {
                if (line.fact_ids.iter().any(|id| affected.contains(id))
                    || blocked(&tx, &self.fingerprint_key, &line.text)?)
                    && affected_lines.insert(line.id)
                {
                    fingerprint(&tx, &self.fingerprint_key, &line.text)?;
                }
            }
            for ep in &all_episodes {
                if (ep.record_ids.iter().any(|id| doomed.contains(id))
                    || (!ep.gist.is_empty() && blocked(&tx, &self.fingerprint_key, &ep.gist)?))
                    && affected_episodes.insert(ep.id)
                    && !ep.gist.is_empty()
                {
                    fingerprint(&tx, &self.fingerprint_key, &ep.gist)?;
                }
            }
            if before
                == (
                    doomed.len(),
                    affected.len(),
                    affected_lines.len(),
                    affected_episodes.len(),
                )
            {
                break;
            }
        }
        for id in &affected {
            tx.execute("DELETE FROM facts WHERE id=?", [id])?;
        }
        for id in &affected_lines {
            tx.execute("DELETE FROM lines WHERE id=?", [id])?;
        }
        for id in &affected_episodes {
            tx.execute(
                "UPDATE episodes SET topic='',gist='',detail='',importance=0 WHERE id=?",
                [id],
            )?;
            tx.execute(
                "DELETE FROM search WHERE kind='episode' AND item_id=?",
                [id],
            )?;
        }
        for id in &doomed {
            tx.execute("DELETE FROM records WHERE id=?", [id])?;
        }
        // Keep no tombstone or erased record's timestamps. A mixed episode
        // contains only the boundaries of its surviving records.
        for id in &affected_episodes {
            tx.execute("DELETE FROM episodes WHERE id=? AND NOT EXISTS(SELECT 1 FROM records WHERE episode_id=?)", params![id,id])?;
            tx.execute("UPDATE episodes SET started_ms=(SELECT min(at_ms) FROM records WHERE episode_id=?), ended_ms=CASE WHEN ended_ms IS NULL THEN NULL ELSE (SELECT max(at_ms) FROM records WHERE episode_id=?) END,last_recalled_ms=0 WHERE id=?", params![id,id,id])?;
        }
        tx.execute(
            "UPDATE meta SET generation=generation+1,revision=revision+1 WHERE one=1",
            [],
        )?;
        tx.commit()?;
        self.invalidate();
        Ok(doomed.len())
    }
    pub fn forget_fact(&mut self, id: i64) -> Result<usize> {
        let f = fact(&self.conn, id)?;
        self.forget_records(&f.quotes.iter().map(|q| q.record_id).collect::<Vec<_>>())
    }
    fn invalidate(&self) {
        self.validity
            .send_modify(|epoch| *epoch = epoch.wrapping_add(1));
    }
}

fn private_file(path: &Path) -> Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(0o600);
        if std::fs::symlink_metadata(path)
            .is_ok_and(|m| !m.is_file() || m.permissions().mode() & 0o077 != 0)
        {
            return Err(Error::Invalid(
                "database and lock files must be private regular files",
            ));
        }
    }
    Ok(options.open(path)?)
}

fn valid_time(time: i64) -> Result<()> {
    if !(0..=253_402_300_799_999).contains(&time) {
        return Err(Error::Invalid("invalid absolute timestamp"));
    }
    Ok(())
}
fn unique_nonempty(ids: &[i64]) -> Result<()> {
    if ids.is_empty()
        || ids.len() > MAX_CITATIONS
        || ids.iter().any(|id| *id <= 0)
        || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
    {
        return Err(Error::Invalid("missing, duplicated or excessive citations"));
    }
    Ok(())
}
fn values<T: for<'a> serde::Deserialize<'a>>(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<T>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, |r| r.get::<_, String>(0))?;
    rows.map(|r| decode(&r?)).collect()
}
fn records_where(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<Record>> {
    values(conn, sql, params)
}
fn facts_where(conn: &Connection, sql: &str, params: impl rusqlite::Params) -> Result<Vec<Fact>> {
    values(conn, sql, params)
}
fn lines_where(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<MemoryLine>> {
    values(conn, sql, params)
}
fn fact(conn: &Connection, id: i64) -> Result<Fact> {
    let body: Option<String> = conn
        .query_row("SELECT body FROM facts WHERE id=?", [id], |r| r.get(0))
        .optional()?;
    decode(&body.ok_or(Error::Invalid("unknown fact"))?)
}
fn episodes(conn: &Connection) -> Result<Vec<Episode>> {
    let mut stmt = conn.prepare(
        "SELECT id,topic,started_ms,ended_ms,importance,gist,detail FROM episodes ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Episode {
            id: r.get(0)?,
            topic: r.get(1)?,
            started_ms: r.get(2)?,
            ended_ms: r.get(3)?,
            importance: r.get(4)?,
            gist: r.get(5)?,
            detail: r.get(6)?,
            record_ids: Vec::new(),
        })
    })?;
    let mut out = Vec::new();
    for ep in rows {
        let mut ep = ep?;
        ep.record_ids = conn
            .prepare("SELECT id FROM records WHERE episode_id=? ORDER BY id")?
            .query_map([ep.id], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        out.push(ep);
    }
    Ok(out)
}
fn index(conn: &Connection, kind: &str, id: i64, text: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM search WHERE kind=? AND item_id=?",
        params![kind, id],
    )?;
    conn.execute(
        "INSERT INTO search(body,kind,item_id) VALUES(?,?,?)",
        params![text, kind, id],
    )?;
    Ok(())
}
fn hashes(key: &[u8; 32], text: &str, query: bool) -> BTreeSet<String> {
    let normalized: String = text.nfkc().flat_map(char::to_lowercase).collect();
    let words: Vec<_> = normalized
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect();
    let mut chunks = vec![words.join(" ")];
    // Five-word overlap catches copies embedded in a later record. It does not
    // claim to recognize arbitrary semantic paraphrases.
    for size in if query { 1..=5 } else { 5..=5 } {
        chunks.extend(words.windows(size).map(|w| w.join(" ")));
    }
    chunks
        .into_iter()
        .filter(|s| !s.is_empty())
        .map(|s| digest(key, &s))
        .collect()
}
fn blocked(conn: &Connection, key: &[u8; 32], text: &str) -> Result<bool> {
    let mut stmt =
        conn.prepare_cached("SELECT EXISTS(SELECT 1 FROM fingerprints WHERE digest=?)")?;
    for hash in hashes(key, text, true) {
        if stmt.query_row([hash], |r| r.get::<_, bool>(0))? {
            return Ok(true);
        }
    }
    Ok(false)
}
fn fingerprint(conn: &Connection, key: &[u8; 32], text: &str) -> Result<()> {
    for hash in hashes(key, text, false) {
        conn.execute("INSERT OR IGNORE INTO fingerprints VALUES(?)", [hash])?;
    }
    Ok(())
}

fn pending_records(
    conn: &Connection,
    now_ms: i64,
    kind: PassKind,
    cycle: i64,
) -> Result<Vec<Record>> {
    if kind == PassKind::Nightly {
        records_where(
            conn,
            "SELECT body FROM records WHERE speaker=? AND at_ms<=? AND processed=0 ORDER BY id LIMIT ?",
            params![encode(&Speaker::Owner)?, now_ms, MAX_RECORDS],
        )
    } else {
        records_where(
            conn,
            "SELECT body FROM records WHERE speaker=? AND at_ms<=? AND at_ms>=? AND id>(SELECT cursor FROM weekly_progress WHERE cycle=?) AND id<=(SELECT last_id FROM weekly_progress WHERE cycle=?) ORDER BY id LIMIT ?",
            params![
                encode(&Speaker::Owner)?,
                now_ms,
                now_ms.saturating_sub(7 * DAY_MS),
                cycle,
                cycle,
                MAX_RECORDS
            ],
        )
    }
}
fn check_rewrites(
    conn: &Connection,
    cycle: i64,
    old: &[MemoryLine],
    changed: &BTreeSet<i64>,
) -> Result<()> {
    if old.is_empty() && changed.is_empty() {
        return Ok(());
    }
    conn.execute(
        "INSERT OR IGNORE INTO model_rewrites VALUES(?,?,?)",
        params![
            cycle,
            encode(&old.iter().map(|l| l.id).collect::<BTreeSet<_>>())?,
            "[]"
        ],
    )?;
    let (original, previous): (String, String) = conn.query_row(
        "SELECT original_ids,changed_ids FROM model_rewrites WHERE cycle=?",
        [cycle],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let original: BTreeSet<i64> = decode(&original)?;
    let mut total: BTreeSet<i64> = decode(&previous)?;
    total.extend(changed.intersection(&original).copied());
    if total.len() * 4 > original.len() {
        return Err(Error::Invalid("owner-model rewrite exceeds one quarter"));
    }
    conn.execute(
        "UPDATE model_rewrites SET changed_ids=? WHERE cycle=?",
        params![encode(&total)?, cycle],
    )?;
    Ok(())
}

fn episode_row(conn: &Connection, id: i64) -> Result<Episode> {
    Ok(conn.query_row(
        "SELECT id,topic,started_ms,ended_ms,importance,gist,detail FROM episodes WHERE id=?",
        [id],
        |r| {
            Ok(Episode {
                id: r.get(0)?,
                topic: r.get(1)?,
                started_ms: r.get(2)?,
                ended_ms: r.get(3)?,
                importance: r.get(4)?,
                gist: r.get(5)?,
                detail: r.get(6)?,
                record_ids: Vec::new(),
            })
        },
    )?)
}

fn random_key() -> Result<[u8; 32]> {
    let mut key = [0; 32];
    rustls::crypto::ring::default_provider()
        .secure_random
        .fill(&mut key)
        .map_err(|_| Error::Invalid("secure random unavailable"))?;
    Ok(key)
}
fn digest(key: &[u8; 32], text: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("fixed HMAC key length");
    mac.update(text.as_bytes());
    format!("{:x}", mac.finalize().into_bytes())
}
fn load_fingerprint_key(path: &Path, populated: bool) -> Result<[u8; 32]> {
    let mut key_path = path.as_os_str().to_os_string();
    key_path.push(".forget-key");
    let key_path = Path::new(&key_path);
    if populated && !key_path.try_exists()? {
        return Err(Error::Invalid(
            "existing store is missing its fingerprint key",
        ));
    }
    let mut file = private_file(key_path)?;
    let len = file.metadata()?.len();
    if len == 0 && !populated {
        let key = random_key()?;
        file.write_all(&key)?;
        file.sync_all()?;
        return Ok(key);
    }
    if len != 32 {
        return Err(Error::Invalid("invalid fingerprint key file"));
    }
    let mut key = [0; 32];
    file.read_exact(&mut key)?;
    Ok(key)
}
