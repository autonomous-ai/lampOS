//! Store-only diagnostic: no claim of voice memory parity.
use crate::{Error, Result, arms::MemoryArm, invalid, protocol::*};
use lamp_memory::{
    days::{DATABASE, Days},
    days_protocol,
    store::{LocalTime, Options, Person as StorePerson},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
};
#[derive(Clone)]
struct State {
    path: PathBuf,
    at: u64,
    sources: BTreeMap<u64, String>,
}
/// One found entry as the voice's recall tells it (`remember.rs`), read at
/// `now_unix_ms` in `zone`, before its `source_record_id` is added.
pub fn recall_entry(entry: &Value, now_unix_ms: u64, zone: &LocalTime) -> Result<Value> {
    // The voice's recall shape (`remember.rs`): an exchange is its
    // `when`, `heard` and `you_said`, a kept thing its `when` and
    // `asked_to_remember`; the store's epoch is never shown.
    let when = crate::when::dated(entry.clone(), now_unix_ms, zone)?["when"].clone();
    let mut value = match entry["kind"].as_str() {
        Some("exchange") => {
            json!({"when":when,"heard":entry["heard"],"you_said":entry["said"]})
        }
        Some("kept") => json!({"when":when,"asked_to_remember":entry["text"]}),
        _ => {
            let mut other = crate::when::dated(entry.clone(), now_unix_ms, zone)?;
            if let Some(fields) = other.as_object_mut() {
                for raw in ["id", "on", "earlier", "strength"] {
                    fields.remove(raw);
                }
            }
            other
        }
    };
    let fields = value
        .as_object_mut()
        .ok_or_else(|| invalid("invalid entry"))?;
    // A dated thing's own day, as the voice tells it (`remember.rs`):
    // `event_on`, `event_status` and `today`, read in the zone.
    let mut past = false;
    if entry["kind"] != "exchange"
        && let Some(day) = entry["on"].as_str().and_then(crate::when::day_of)
    {
        let today = zone.day(now_unix_ms);
        let status = match day.cmp(&today) {
            std::cmp::Ordering::Greater => "upcoming",
            std::cmp::Ordering::Equal => "today",
            std::cmp::Ordering::Less => "past",
        };
        past = status == "past";
        fields.insert("event_on".into(), json!(crate::when::day_name(day)));
        fields.insert("event_status".into(), json!(status));
        fields.insert("today".into(), json!(crate::when::day_name(today)));
    }
    // P2 `strength` reaches the answerer only in the voice's words
    // (`remember.rs` `strength_told`); any other label is dropped.
    // P1 `earlier` only orders the entries: the voice never tells it.
    if let Some(told) = entry["strength"]
        .as_str()
        .and_then(|label| crate::when::strength_told(label, past))
    {
        fields.insert("strength".into(), json!(told));
    }
    Ok(value)
}
pub struct Partial {
    owner: String,
    zone: String,
    state: State,
    main: Option<(String, State)>,
    sequence: u64,
    recall_calls: u64,
    snapshot_calls: u64,
    /// `lampos_today_partial_next`: the memory owner's Tiers-ideas switches
    /// (3 Nov, P1-P3) on, as the voice sends them with each env `on`.
    next: bool,
}
/// The setup that sends the Tiers-ideas switches (P4b).
pub const NEXT: &str = "lampos_today_partial_next";
/// The partial arm's `search`. With `next` it also sends `newer_first`
/// (`LAMP_MEMORY_NEWER_FIRST`) and `strength` (`LAMP_MEMORY_STRENGTH`);
/// without it, the bytes are today's. A Days without those fields refuses
/// them (`deny_unknown_fields`), so the setup never runs as today's.
pub fn search_request(owner: &str, query: &str, max_bytes: usize, next: bool) -> Value {
    let mut request = json!({"op":"search","person":owner,"query":query,"limit":8,"max_bytes":max_bytes,"history":false,"recall_v2":false});
    if next {
        request["newer_first"] = json!(true);
        request["strength"] = json!(true);
    }
    request
}
/// The partial arm's `session_open`. With `next` it also sends `strength`
/// and `profile` (`LAMP_MEMORY_PROFILE`); without it, today's bytes.
pub fn session_open_request(owner: &str, session: &str, max_bytes: usize, next: bool) -> Value {
    let mut request =
        json!({"op":"session_open","person":owner,"session":session,"max_bytes":max_bytes});
    if next {
        request["strength"] = json!(true);
        request["profile"] = json!(true);
    }
    request
}
impl Partial {
    pub fn open(init: &Init) -> Result<Self> {
        if StorePerson::new(&init.owner_id).is_none() {
            return Err(invalid("owner cannot map to product scope"));
        }
        let path = Path::new(&init.state_dir).join("main");
        fs::DirBuilder::new().mode(0o700).create(&path)?;
        let arm = Self {
            owner: init.owner_id.clone(),
            zone: init.timezone.clone(),
            state: State {
                path,
                at: init.at_unix_ms,
                sources: BTreeMap::new(),
            },
            main: None,
            sequence: 0,
            recall_calls: 0,
            snapshot_calls: 0,
            next: init.arm == NEXT,
        };
        drop(arm.store()?);
        Ok(arm)
    }
    fn store(&self) -> Result<Days> {
        Days::open(
            Options {
                dir: self.state.path.clone(),
                keep_days: 30,
                local: LocalTime::zone(&self.zone)
                    .map_err(|_| invalid("product timezone unavailable"))?,
            },
            self.state.at,
        )
        .map_err(|e| invalid(format!("partial store: {e}")))
    }
    fn send(&self, request: Value) -> Result<Value> {
        let mut store = self.store()?;
        let reply =
            days_protocol::answer(&mut store, &serde_json::to_vec(&request)?, self.state.at);
        let value: Value = serde_json::from_str(&reply.line)?;
        if value["ok"] != true {
            return Err(invalid(format!(
                "store refused operation: {}",
                value["error"]
            )));
        }
        Ok(value)
    }
    fn admitted(&self, p: Person, id: Option<&str>, presence: Presence) -> bool {
        p == Person::Owner && id == Some(self.owner.as_str()) && presence == Presence::OwnerAlone
    }
    fn sources(&self, value: &Value) -> Result<Vec<String>> {
        value
            .as_array()
            .ok_or_else(|| invalid("store source list absent"))?
            .iter()
            .map(|v| {
                let id = v.as_u64().ok_or_else(|| invalid("invalid store source"))?;
                self.state
                    .sources
                    .get(&id)
                    .cloned()
                    .ok_or_else(|| invalid("unmapped store source"))
            })
            .collect()
    }
}
impl MemoryArm for Partial {
    fn ingest(&mut self, e: Event) -> Result<Value> {
        e.validate()?;
        self.state.at = e.at_unix_ms;
        let accepted = self.admitted(e.person, e.person_id.as_deref(), e.presence);
        let mut sources = vec![];
        if accepted && e.kind == EventKind::Utterance {
            let reply=self.send(json!({"op":"append","person":self.owner,"at_unix_ms":e.at_unix_ms,"heard":e.heard,"said":e.said}))?;
            let id = reply["id"]
                .as_u64()
                .ok_or_else(|| invalid("append omitted id"))?;
            self.state.sources.insert(id, e.record_id.clone());
            sources.push(e.record_id);
        }
        Ok(
            json!({"accepted":accepted,"source_ids":sources,"forget_effect":ForgetEffect::default()}),
        )
    }
    fn sleep(&mut self, i: Sleep) -> Result<Value> {
        self.state.at = i.at_unix_ms;
        drop(self.store()?);
        Ok(
            json!({"applied":false,"source_ids":[],"reason":"partial: store aging only; voice consolidation unavailable","pending":false,"next_cursor":null}),
        )
    }
    fn forget(&mut self, i: Forget) -> Result<Value> {
        self.state.at = i.at_unix_ms;
        if i.person_id != self.owner {
            return Err(invalid("forget scope mismatch"));
        }
        if i.record_ids.is_empty() {
            return Err(Error::Unsupported(
                "partial store requires record ids; voice forget dispatch unavailable".into(),
            ));
        }
        let ids: Vec<u64> = self
            .state
            .sources
            .iter()
            .filter(|(_, s)| i.record_ids.contains(s))
            .map(|(id, _)| *id)
            .collect();
        let mut removed = 0;
        for id in ids {
            removed+=self.send(json!({"op":"forget","person":self.owner,"id":id,"selective":false}))?["removed"].as_u64().ok_or_else(||invalid("forget omitted count"))?;
        }
        // Direct single-record deletion does not clear the product voice thread.
        Ok(
            json!({"applied":removed>0,"removed_count":removed,"forget_effect":ForgetEffect::default()}),
        )
    }
    fn snapshot(&mut self, q: Query) -> Result<Snapshot> {
        self.state.at = q.at_unix_ms;
        if !self.admitted(q.person, q.person_id.as_deref(), q.presence) {
            return Ok(Snapshot::default());
        }
        self.sequence += 1;
        let session = format!("bench-snapshot-{}", self.sequence);
        let reply = self.send(session_open_request(
            &self.owner,
            &session,
            q.max_context_bytes.min(1200),
            self.next,
        ))?;
        self.send(json!({"op":"session_close","person":self.owner,"session":session}))?;
        let snapshot = &reply["snapshot"];
        let sources = self.sources(&snapshot["source_ids"])?;
        if sources.is_empty() {
            return Ok(Snapshot::default());
        }
        if snapshot["note"].as_str().is_some_and(|s| !s.is_empty()) {
            self.snapshot_calls = self.snapshot_calls.saturating_add(1);
        }
        Ok(Snapshot {
            owner_model: vec![ModelLine {
                text: snapshot["note"]
                    .as_str()
                    .ok_or_else(|| invalid("note absent"))?
                    .into(),
                source_ids: sources.clone(),
            }],
            source_ids: sources,
            memory_instructions: String::new(),
        })
    }
    fn recall(&mut self, q: Query) -> Result<Recall> {
        self.state.at = q.at_unix_ms;
        if !self.admitted(q.person, q.person_id.as_deref(), q.presence) {
            return Ok(Recall::default());
        }
        let reply = self.send(search_request(
            &self.owner,
            &q.heard,
            q.max_context_bytes.saturating_sub(2048).max(1),
            self.next,
        ))?;
        let entries = reply["entries"]
            .as_array()
            .ok_or_else(|| invalid("search omitted entries"))?;
        let zone = crate::when::zone(&self.zone)?;
        let mut output = vec![];
        let mut sources = vec![];
        for entry in entries {
            let id = entry["id"]
                .as_u64()
                .ok_or_else(|| invalid("entry omitted id"))?;
            let source = self
                .state
                .sources
                .get(&id)
                .ok_or_else(|| invalid("unmapped record"))?;
            let mut value = recall_entry(entry, q.at_unix_ms, &zone)?;
            let fields = value
                .as_object_mut()
                .ok_or_else(|| invalid("invalid entry"))?;
            fields.insert("source_record_id".into(), json!(source));
            sources.push(source.clone());
            output.push(value);
        }
        let r = Recall {
            context: if output.is_empty() {
                String::new()
            } else {
                serde_json::to_string(&output)?
            },
            source_ids: sources,
            ..Recall::default()
        };
        r.check_bound(q.max_context_bytes)?;
        if !r.context.is_empty() {
            self.recall_calls = self.recall_calls.saturating_add(1);
        }
        Ok(r)
    }
    fn inspect(&self, person: &str) -> Result<Value> {
        let mut retained = vec![];
        if person == self.owner {
            let store = self.store()?;
            let person = StorePerson::new(&self.owner).ok_or_else(|| invalid("invalid owner"))?;
            for (id, source) in &self.state.sources {
                if store
                    .record(&person, *id)
                    .map_err(|e| invalid(format!("inspection: {e}")))?
                    .is_some()
                {
                    retained.push(source);
                }
            }
        }
        let tiers = if person == self.owner {
            json!({"record":{"items":retained.len(),"text_bytes":null,"recall_calls":self.recall_calls,"snapshot_calls":self.snapshot_calls},"talk_notes":{"items":null,"text_bytes":null,"recall_calls":null,"snapshot_calls":null},"running_summary":{"items":null,"text_bytes":null,"recall_calls":null,"snapshot_calls":null}})
        } else {
            json!({})
        };
        Ok(
            json!({"owner_model":[],"retained_record_ids":retained,"diagnostics":{"schema":"tier-usage-v1","tiers":tiers},"stats":{"label":if self.next {"partial current product, Tiers-ideas switches on"} else {"partial current product"},"complete_baseline":false,"voice_observations":false,"voice_talk_notes":false,"voice_running_summary":false,"voice_forget_dispatch":false,"stored_bytes":fs::metadata(self.state.path.join(DATABASE))?.len(),"model_calls":0}}),
        )
    }
    fn begin_probe(&mut self, id: &str, at: u64) -> Result<()> {
        if self.main.is_some() {
            return Err(invalid("nested probe"));
        }
        // Every handle is closed. Refuse sidecars and symlinks: never copy a hot DB.
        for e in fs::read_dir(&self.state.path)? {
            let e = e?;
            if e.file_name() != DATABASE || !e.file_type()?.is_file() {
                return Err(invalid("unexpected store file at checkpoint"));
            }
        }
        self.sequence += 1;
        let path = self
            .state
            .path
            .parent()
            .ok_or_else(|| invalid("missing parent"))?
            .join(format!("probe-{}", self.sequence));
        fs::DirBuilder::new().mode(0o700).create(&path)?;
        fs::copy(self.state.path.join(DATABASE), path.join(DATABASE))?;
        fs::set_permissions(path.join(DATABASE), fs::Permissions::from_mode(0o600))?;
        let main = self.state.clone();
        self.state.path = path;
        self.state.at = at;
        self.main = Some((id.into(), main));
        Ok(())
    }
    fn end_probe(&mut self, id: &str) -> Result<()> {
        if self.main.as_ref().map(|p| p.0.as_str()) != Some(id) {
            return Err(invalid("probe mismatch"));
        }
        let main = self.main.take().ok_or_else(|| invalid("no probe"))?.1;
        let discarded = std::mem::replace(&mut self.state, main);
        fs::remove_dir_all(discarded.path)?;
        Ok(())
    }
}
