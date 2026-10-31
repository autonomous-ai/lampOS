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
pub struct Partial {
    owner: String,
    zone: String,
    state: State,
    main: Option<(String, State)>,
    sequence: u64,
    recall_calls: u64,
    snapshot_calls: u64,
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
        let reply=self.send(json!({"op":"session_open","person":self.owner,"session":session,"max_bytes":q.max_context_bytes.min(1200)}))?;
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
        let reply=self.send(json!({"op":"search","person":self.owner,"query":q.heard,"limit":8,"max_bytes":q.max_context_bytes.saturating_sub(2048).max(1),"history":false,"recall_v2":false}))?;
        let entries = reply["entries"]
            .as_array()
            .ok_or_else(|| invalid("search omitted entries"))?;
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
            let mut value = entry.clone();
            value
                .as_object_mut()
                .ok_or_else(|| invalid("invalid entry"))?
                .insert("source_record_id".into(), json!(source));
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
            json!({"owner_model":[],"retained_record_ids":retained,"diagnostics":{"schema":"tier-usage-v1","tiers":tiers},"stats":{"label":"partial current product","complete_baseline":false,"voice_observations":false,"voice_talk_notes":false,"voice_running_summary":false,"voice_forget_dispatch":false,"stored_bytes":fs::metadata(self.state.path.join(DATABASE))?.len(),"model_calls":0}}),
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
