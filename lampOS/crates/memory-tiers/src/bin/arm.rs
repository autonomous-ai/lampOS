//! Bench-only JSON-lines adapter. No listener, credentials or direct provider.
#[path = "arm/probe.rs"]
mod probe;
use lamp_memory_tiers::{model::PROMPT, *};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::{Duration, Instant},
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
const FRAME: usize = 4 * 1024 * 1024;
const OPS: &[&str] = &[
    "init",
    "ingest",
    "sleep",
    "forget",
    "probe_begin",
    "snapshot",
    "recall",
    "tool",
    "inspect",
    "probe_end",
    "close",
];
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    v: u8,
    id: String,
    op: String,
    data: Value,
}
struct Wire {
    input: BufReader<tokio::io::Stdin>,
    output: tokio::io::Stdout,
}
impl Wire {
    async fn read(&mut self) -> Result<Option<Value>> {
        let mut bytes = Vec::new();
        loop {
            let chunk = self.input.fill_buf().await?;
            if chunk.is_empty() {
                if bytes.is_empty() {
                    return Ok(None);
                }
                return Err(Error::Invalid("unterminated frame"));
            }
            let end = chunk.iter().position(|c| *c == b'\n').map(|i| i + 1);
            let n = end.unwrap_or(chunk.len());
            if bytes.len() + n > FRAME {
                return Err(Error::Invalid("frame exceeds limit"));
            }
            bytes.extend_from_slice(&chunk[..n]);
            self.input.consume(n);
            if end.is_some() {
                break;
            }
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| Error::Invalid("malformed frame"))
    }
    async fn write(&mut self, value: &Value) -> Result<()> {
        let mut bytes =
            serde_json::to_vec(value).map_err(|_| Error::Invalid("cannot encode response"))?;
        if bytes.len() + 1 > FRAME {
            return Err(Error::Invalid("response exceeds frame limit"));
        }
        bytes.push(b'\n');
        self.output.write_all(&bytes).await?;
        self.output.flush().await?;
        Ok(())
    }
    async fn propose(
        &mut self,
        id: &str,
        call: u64,
        input: &SleepInput,
        weekly: bool,
        config: &Config,
    ) -> Result<String> {
        let input = input.json()?;
        let call_id = format!("tiers-model-{call}");
        self.write(&json!({"v":2,"id":id,"model_request":{"call_id":call_id,"kind":"chat","purpose":if weekly{"reflect"}else{"sleep"},"messages":[{"role":"system","content":PROMPT},{"role":"user","content":input}],"max_output_tokens":config.max_output_tokens,"response_format":{"type":"json_object"},"tools":[],"tool_choice":"none"}})).await?;
        let response =
            tokio::time::timeout(Duration::from_millis(config.model_timeout_ms), self.read())
                .await
                .map_err(|_| Error::Timeout)??
                .ok_or(Error::Model("broker closed"))?;
        if response["v"] != 2
            || response["id"] != id
            || response["model_response"]["call_id"] != call_id
        {
            return Err(Error::Model("broker response identity mismatch"));
        }
        let model = &response["model_response"];
        if model["ok"] != true {
            return Err(Error::Model("runner refused or failed model call"));
        }
        if !model["tool_calls"].as_array().is_some_and(Vec::is_empty) {
            return Err(Error::Model("unexpected model tools"));
        }
        if !model["usage"].is_null()
            && (model["usage"]["input_tokens"].as_u64().is_none()
                || model["usage"]["output_tokens"].as_u64().is_none())
        {
            return Err(Error::Model("invalid broker usage"));
        }
        // Unknown usage is permitted: the runner keeps its full reservation.
        // The adapter never substitutes zero or owns a second cost ledger.
        let content = model["content"]
            .as_str()
            .ok_or(Error::Model("missing model content"))?;
        SleepProposal::parse(content)?;
        Ok(content.into())
    }
}
#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Config {
    max_output_tokens: u32,
    model_timeout_ms: u64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            max_output_tokens: 4096,
            model_timeout_ms: 60_000,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Init {
    run_id: String,
    arm: String,
    state_dir: PathBuf,
    owner_id: String,
    timezone: String,
    at_unix_ms: i64,
    config: Config,
}
#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct EventCues {
    activity: Option<String>,
    objects: Vec<String>,
    topic: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    record_id: String,
    at_unix_ms: i64,
    session_id: String,
    kind: String,
    person: String,
    person_id: Option<String>,
    presence: String,
    heard: String,
    said: String,
    stt_confidence: Option<f64>,
    cues: EventCues,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Query {
    probe_id: String,
    at_unix_ms: i64,
    person: String,
    person_id: Option<String>,
    presence: String,
    heard: String,
    cues: EventCues,
    max_context_bytes: usize,
}
#[derive(Clone, Default)]
struct PolicyState {
    clock: i64,
    records: BTreeMap<i64, String>,
    event_ids: BTreeSet<String>,
    snapshot_bytes: usize,
    snapshot_scope: Option<(String, Option<String>, String)>,
    snapshot_sources: BTreeSet<i64>,
    last_bytes: usize,
    last_timed_out: bool,
    recalls: u64,
    misses: u64,
}
struct Context {
    store: Store,
    policy: PolicyState,
    // Fields drop in order: close the store before removing its files.
    scratch: Option<tempfile::TempDir>,
}
impl Context {
    fn discard(self) -> Result<()> {
        drop(self.store);
        if let Some(scratch) = self.scratch {
            scratch.close()?;
        }
        Ok(())
    }
}
struct Adapter {
    context: Option<Context>,
    main: Option<Context>,
    probe: Option<String>,
    owner: String,
    root: PathBuf,
    arm: Arm,
    config: Config,
    requests: BTreeSet<String>,
    model_calls: u64,
    closed: bool,
}
impl Default for Adapter {
    fn default() -> Self {
        Self {
            context: None,
            main: None,
            probe: None,
            owner: String::new(),
            root: PathBuf::new(),
            arm: Arm::Tiers,
            config: Config::default(),
            requests: BTreeSet::new(),
            model_calls: 0,
            closed: false,
        }
    }
}
fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| Error::Invalid("invalid operation data"))
}
fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .ok_or(Error::Invalid("missing text field"))
}
fn timestamp(value: &Value) -> Result<i64> {
    value["at_unix_ms"]
        .as_i64()
        .ok_or(Error::Invalid("missing operation time"))
}
fn valid_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-.:".contains(&c))
    {
        return Err(Error::Invalid("invalid identifier"));
    }
    Ok(())
}
fn time_check(clock: i64, at: i64) -> Result<()> {
    if at < clock || at > 253_402_300_799_999 {
        return Err(Error::Invalid(
            "operation time moved backward or out of range",
        ));
    }
    Ok(())
}
fn scope(person: &str, presence: &str) -> Result<()> {
    if !matches!(person, "owner" | "known" | "visitor" | "unsure")
        || !matches!(presence, "owner_alone" | "others" | "unknown")
    {
        return Err(Error::Invalid("unknown observed scope"));
    }
    Ok(())
}
fn owner_access(owner: &str, person: &str, id: &Option<String>, presence: &str) -> Access {
    Access {
        owner: person == "owner" && id.as_deref() == Some(owner),
        alone: presence == "owner_alone",
    }
}
fn source_ids(policy: &PolicyState, ids: &[i64]) -> Result<Vec<String>> {
    ids.iter()
        .map(|id| {
            policy
                .records
                .get(id)
                .cloned()
                .ok_or(Error::Invalid("missing source mapping"))
        })
        .collect()
}
fn invalidate_snapshot(policy: &mut PolicyState) {
    policy.snapshot_bytes = 0;
    policy.snapshot_sources.clear();
    policy.snapshot_scope = None;
}
impl Adapter {
    async fn dispatch(&mut self, request: &Request, wire: &mut Wire) -> Result<Value> {
        if request.v != 2 || !request.data.is_object() {
            return Err(Error::Invalid("unsupported protocol version"));
        }
        valid_id(&request.id)?;
        if self.requests.len() >= 100_000 || !self.requests.insert(request.id.clone()) {
            return Err(Error::Invalid("duplicate request or request limit"));
        }
        if request.op == "init" {
            if self.context.is_some() {
                return Err(Error::Invalid("already initialized"));
            }
            let init: Init = parse(request.data.clone())?;
            valid_id(&init.run_id)?;
            valid_id(&init.owner_id)?;
            if init.owner_id.len() > 128
                || init.timezone == "system"
                || lamp_local_time::LocalTime::zone(&init.timezone).is_err()
            {
                return Err(Error::Invalid("invalid owner or explicit time zone"));
            }
            time_check(0, init.at_unix_ms)?;
            if init.config.max_output_tokens == 0
                || init.config.max_output_tokens > 8192
                || init.config.model_timeout_ms == 0
                || init.config.model_timeout_ms > 60_000
            {
                return Err(Error::Invalid("invalid model limits"));
            }
            let arm = Arm::parse(&init.arm)?;
            let metadata = std::fs::symlink_metadata(&init.state_dir)?;
            if !metadata.is_dir()
                || metadata.file_type().is_symlink()
                || std::fs::read_dir(&init.state_dir)?.next().is_some()
            {
                return Err(Error::Invalid("state directory must be a fresh directory"));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o077 != 0 {
                    return Err(Error::Invalid("state directory must be private"));
                }
            }
            let root = init.state_dir.canonicalize()?;
            let mut store = Store::open(root.join("tiers.sqlite"), &init.owner_id)?;
            store.set_time_zone(&init.timezone)?;
            self.root = root;
            self.owner = init.owner_id;
            self.arm = arm;
            self.config = init.config;
            self.context = Some(Context {
                store,
                scratch: None,
                policy: PolicyState {
                    clock: init.at_unix_ms,
                    ..Default::default()
                },
            });
            return Ok(
                json!({"arm":arm.name(),"implementation_revision":option_env!("LAMP_MEMORY_TIERS_REVISION").unwrap_or("unversioned"),"capabilities":OPS}),
            );
        }
        let context = self
            .context
            .as_mut()
            .ok_or(Error::Invalid("init is required"))?;
        match request.op.as_str() {
            "ingest" => {
                let event: Event = parse(request.data["event"].clone())?;
                validate_event(&event)?;
                time_check(context.policy.clock, event.at_unix_ms)?;
                if context.policy.event_ids.contains(&event.record_id) {
                    return Err(Error::Invalid("duplicate event id"));
                }
                let access = owner_access(
                    &self.owner,
                    &event.person,
                    &event.person_id,
                    &event.presence,
                );
                if context.policy.snapshot_scope.as_ref()
                    != Some(&(
                        event.person.clone(),
                        event.person_id.clone(),
                        event.presence.clone(),
                    ))
                {
                    invalidate_snapshot(&mut context.policy);
                }
                let mut sources = Vec::new();
                let mut handled_control = false;
                let mut forget_ran = false;
                let mut suppress_confirmation = false;
                if event.kind == "utterance" && access.owner && !event.heard.trim().is_empty() {
                    match command(&event.heard) {
                        Command::Forget(text) => {
                            handled_control = true;
                            if let Some(removed) = forget_text(context, &text)? {
                                forget_ran = true;
                                suppress_confirmation = removed > 0;
                            }
                            invalidate_snapshot(&mut context.policy);
                        }
                        Command::Pause => {
                            handled_control = true;
                            context.store.pause(true)?;
                            invalidate_snapshot(&mut context.policy);
                        }
                        Command::Resume => {
                            handled_control = true;
                            context.store.pause(false)?;
                        }
                        other if access.alone => {
                            let keep = matches!(other, Command::Remember);
                            if let Some(id) = context.store.ingest(Incoming {
                                text: event.heard,
                                speaker: Speaker::Owner,
                                at_ms: event.at_unix_ms,
                                stt_confidence: event.stt_confidence,
                                strength: Strength::Said,
                                addressed_to_lamp: true,
                                topic: event
                                    .cues
                                    .topic
                                    .filter(|s| !s.trim().is_empty())
                                    .unwrap_or_else(|| "conversation".into()),
                                remember: keep && self.arm != Arm::TiersMinusImportance,
                            })? {
                                context.policy.records.insert(id, event.record_id.clone());
                                sources.push(event.record_id.clone());
                                if keep {
                                    context.store.remember(id, FactKind::Statement, None)?;
                                }
                            }
                        }
                        _ => {}
                    }
                } else if matches!(event.kind.as_str(), "session_end" | "leave") && access.owner {
                    context.store.owner_left(event.at_unix_ms)?;
                    invalidate_snapshot(&mut context.policy);
                }
                // Said text is never owner evidence; the prototype stores only
                // observed owner speech. Visitor/unsure/mixed-room events are dropped.
                context.policy.event_ids.insert(event.record_id);
                context.policy.clock = event.at_unix_ms;
                Ok(
                    json!({"accepted":(access.owner && access.alone)||handled_control,"source_ids":sources,"forget_effect":{"ran":forget_ran,"suppress_confirmation":suppress_confirmation}}),
                )
            }
            "sleep" => {
                let at = timestamp(&request.data)?;
                time_check(context.policy.clock, at)?;
                let cadence = string(&request.data, "cadence")?;
                let kind = match cadence {
                    "nightly" => PassKind::Nightly,
                    "weekly" => PassKind::Weekly,
                    _ => return Err(Error::Invalid("unknown sleep cadence")),
                };
                context.store.maintain(at, self.arm)?;
                context.policy.clock = at;
                invalidate_snapshot(&mut context.policy);
                if self.arm == Arm::TiersMinusNightlySleep && kind == PassKind::Nightly {
                    context.policy.clock = at;
                    return Ok(
                        json!({"applied":false,"source_ids":[],"reason":"not_applicable","pending":false,"next_cursor":null}),
                    );
                }
                let Some(input) = context.store.prepare_sleep(at, kind)? else {
                    context.policy.clock = at;
                    return Ok(
                        json!({"applied":false,"source_ids":[],"reason":"no_eligible_records","pending":false,"next_cursor":null}),
                    );
                };
                self.model_calls = self.model_calls.checked_add(1).ok_or(Error::Budget)?;
                // The runner is the single cost owner. This counter and broker
                // remain outside isolated Context and are never restored.
                let proposal = wire
                    .propose(
                        &request.id,
                        self.model_calls,
                        &input,
                        kind == PassKind::Weekly,
                        &self.config,
                    )
                    .await?;
                let proposal = self.arm.filter_proposal(&proposal)?;
                match context.store.apply_sleep(&input, &proposal) {
                    Ok(report) => {
                        let sources = source_ids(
                            &context.policy,
                            &input.records().iter().map(|r| r.id).collect::<Vec<_>>(),
                        )?;
                        context.policy.clock = at;
                        invalidate_snapshot(&mut context.policy);
                        Ok(
                            json!({"applied":true,"source_ids":sources,"reason":null,"pending":report.pending,"next_cursor":if report.pending {input.records().last().map(|r|format!("after-{}",r.id))}else{None}}),
                        )
                    }
                    Err(Error::Invalid(reason)) => Ok(
                        json!({"applied":false,"source_ids":[],"reason":reason,"pending":true,"next_cursor":null}),
                    ),
                    Err(error) => Err(error),
                }
            }
            "forget" => {
                let at = timestamp(&request.data)?;
                time_check(context.policy.clock, at)?;
                if request.data["person_id"].as_str() != Some(self.owner.as_str()) {
                    return Err(Error::Invalid("forget requires this owner"));
                }
                let external: Vec<String> = parse(request.data["record_ids"].clone())?;
                let removed = if external.is_empty() {
                    forget_text(context, string(&request.data, "text")?)?
                } else {
                    let mut ids = Vec::new();
                    for external in external {
                        let id = context
                            .policy
                            .records
                            .iter()
                            .find(|(_, v)| **v == external)
                            .map(|(id, _)| *id)
                            .ok_or(Error::Invalid("unknown source id"))?;
                        ids.push(id);
                    }
                    let n = context.store.forget_records(&ids)?;
                    prune_sources(context)?;
                    Some(n)
                };
                invalidate_snapshot(&mut context.policy);
                context.policy.clock = at;
                Ok(
                    json!({"applied":removed.is_some_and(|n|n>0),"removed_count":removed.unwrap_or(0),"forget_effect":{"ran":removed.is_some(),"suppress_confirmation":removed.is_some_and(|n|n>0)}}),
                )
            }
            "probe_begin" => {
                let id = string(&request.data, "probe_id")?;
                valid_id(id)?;
                if self.probe.is_some() {
                    return Err(Error::Invalid("nested probe"));
                }
                let at = timestamp(&request.data)?;
                time_check(context.policy.clock, at)?;
                let (store, scratch) = probe::fork(&self.root, &self.owner)?;
                let mut child = Context {
                    store,
                    policy: context.policy.clone(),
                    scratch: Some(scratch),
                };
                child.policy.clock = at;
                self.main = self.context.replace(child);
                self.probe = Some(id.into());
                Ok(json!({"isolated":true}))
            }
            "probe_end" => {
                if self.probe.as_deref() != Some(string(&request.data, "probe_id")?) {
                    return Err(Error::Invalid("probe identity mismatch"));
                }
                let main = self
                    .main
                    .take()
                    .ok_or(Error::Invalid("no isolated probe"))?;
                if let Some(child) = self.context.take() {
                    child.discard()?;
                }
                self.context = Some(main);
                self.probe = None;
                Ok(json!({"restored":true}))
            }
            "recall" | "snapshot" => {
                let started = Instant::now();
                let query: Query = parse(request.data["query"].clone())?;
                if self.probe.is_some() && self.probe.as_deref() != Some(query.probe_id.as_str()) {
                    return Err(Error::Invalid("query does not match active isolated probe"));
                }
                scope(&query.person, &query.presence)?;
                time_check(context.policy.clock, query.at_unix_ms)?;
                valid_id(&query.probe_id)?;
                if let Some(id) = &query.person_id {
                    valid_id(id)?;
                }
                if matches!(query.person.as_str(), "owner" | "known") && query.person_id.is_none() {
                    return Err(Error::Invalid("identified query needs a person id"));
                }
                if !(1..=1_048_576).contains(&query.max_context_bytes) {
                    return Err(Error::Invalid("context limit exceeds bound"));
                }
                if request.op == "snapshot" && !query.heard.is_empty() {
                    return Err(Error::Invalid("snapshot heard must be empty"));
                }
                let current_scope = (
                    query.person.clone(),
                    query.person_id.clone(),
                    query.presence.clone(),
                );
                if context.policy.snapshot_scope.as_ref() != Some(&current_scope) {
                    invalidate_snapshot(&mut context.policy);
                }
                let snapshot = request.op == "snapshot";
                let limit = if snapshot {
                    SNAPSHOT_BUDGET
                } else {
                    RECALL_BUDGET
                };
                let q = RecallQuery {
                    access: owner_access(
                        &self.owner,
                        &query.person,
                        &query.person_id,
                        &query.presence,
                    ),
                    now_ms: query.at_unix_ms,
                    words: query.heard,
                    cues: Cues {
                        people: query.person_id.into_iter().collect(),
                        activity: query.cues.activity,
                        objects: query.cues.objects,
                        topic: query.cues.topic,
                    },
                    max_bytes: if snapshot {
                        query.max_context_bytes
                    } else {
                        query
                            .max_context_bytes
                            .saturating_sub(context.policy.snapshot_bytes)
                    },
                    budget: limit.saturating_sub(started.elapsed()),
                    arm: self.arm,
                };
                let result = if snapshot {
                    context.store.snapshot(&q)?
                } else {
                    context.store.recall(&q)?
                };
                let result = if started.elapsed() >= limit {
                    RecallResult {
                        timed_out: true,
                        ..Default::default()
                    }
                } else {
                    result
                };
                context.policy.clock = query.at_unix_ms;
                context.policy.last_bytes = result.bytes;
                context.policy.last_timed_out = result.timed_out;
                context.policy.recalls += 1;
                context.policy.misses += u64::from(result.timed_out);
                let sources = source_ids(&context.policy, &result.source_ids())?;
                if snapshot {
                    context.policy.snapshot_bytes = result.bytes;
                    context.policy.snapshot_sources = result.source_ids().into_iter().collect();
                    context.policy.snapshot_scope = Some(current_scope);
                    let lines=result.items.iter().map(|item|Ok(json!({"text":item.text,"source_ids":source_ids(&context.policy,&item.source_ids)?}))).collect::<Result<Vec<_>>>()?;
                    Ok(
                        json!({"owner_model":lines,"source_ids":sources,"memory_instructions":result.instructions}),
                    )
                } else {
                    Ok(
                        json!({"context":result.context(),"source_ids":sources,"owner_model":[],"tools":[],"memory_instructions":result.instructions}),
                    )
                }
            }
            "tool" => Err(Error::Invalid("no memory tools advertised by this arm")),
            "inspect" => {
                if request.data["person_id"].as_str() != Some(self.owner.as_str()) {
                    return Ok(
                        json!({"owner_model":[],"retained_record_ids":[],"stats":{"access":"denied"}}),
                    );
                }
                let q = RecallQuery {
                    access: Access::OWNER_ALONE,
                    now_ms: context.policy.clock,
                    words: String::new(),
                    cues: Cues::default(),
                    max_bytes: 16384,
                    budget: SNAPSHOT_BUDGET,
                    arm: self.arm,
                };
                let snapshot = context.store.snapshot(&q)?;
                let lines=snapshot.items.iter().map(|item|Ok(json!({"text":item.text,"source_ids":source_ids(&context.policy,&item.source_ids)?}))).collect::<Result<Vec<_>>>()?;
                let records = context.store.records(Access::OWNER_ALONE)?;
                let retained = source_ids(
                    &context.policy,
                    &records.iter().map(|r| r.id).collect::<Vec<_>>(),
                )?;
                Ok(
                    json!({"owner_model":lines,"retained_record_ids":retained,"stats":{"records":records.len(),"last_context_bytes":context.policy.last_bytes,"snapshot_bytes":context.policy.snapshot_bytes,"last_recall_timed_out":context.policy.last_timed_out,"recalls":context.policy.recalls,"recall_misses":context.policy.misses,"model_calls":self.model_calls,"arm_description":self.arm.description()}}),
                )
            }
            "close" => {
                if self.probe.is_some() {
                    return Err(Error::Invalid("close requires probe restoration"));
                }
                if request.data != json!({}) {
                    return Err(Error::Invalid("close data must be empty"));
                }
                self.closed = true;
                Ok(json!({}))
            }
            _ => Err(Error::Invalid("unsupported operation")),
        }
    }
}
fn validate_event(event: &Event) -> Result<()> {
    valid_id(&event.record_id)?;
    valid_id(&event.session_id)?;
    scope(&event.person, &event.presence)?;
    if !matches!(
        event.kind.as_str(),
        "utterance" | "session_start" | "session_end" | "arrive" | "leave"
    ) {
        return Err(Error::Invalid("unknown event kind"));
    }
    if event.kind != "utterance" && (!event.heard.is_empty() || !event.said.is_empty()) {
        return Err(Error::Invalid("text on non-utterance event"));
    }
    if event.heard.len() > MAX_RECORD_BYTES
        || event.said.len() > MAX_RECORD_BYTES
        || event
            .stt_confidence
            .is_some_and(|c| !c.is_finite() || !(0.0..=1.0).contains(&c))
    {
        return Err(Error::Invalid("invalid event speech"));
    }
    if event.cues.topic.as_ref().is_some_and(|s| s.len() > 256)
        || event.cues.activity.as_ref().is_some_and(|s| s.len() > 256)
        || event.cues.objects.len() > 32
        || event.cues.objects.iter().any(|s| s.len() > 256)
    {
        return Err(Error::Invalid("event cues exceed limits"));
    }
    if let Some(id) = &event.person_id {
        valid_id(id)?;
    }
    Ok(())
}
enum Command {
    None,
    Remember,
    Forget(String),
    Pause,
    Resume,
}
fn command(text: &str) -> Command {
    let lower = text.trim().to_lowercase();
    let lower = lower.strip_prefix("please ").unwrap_or(&lower);
    if matches!(
        lower.trim_end_matches(['.', '!']),
        "stop remembering"
            | "pause memory"
            | "don't remember this"
            | "do not remember this"
            | "don't remember this conversation"
            | "do not remember this conversation"
    ) {
        return Command::Pause;
    }
    if matches!(
        lower.trim_end_matches(['.', '!']),
        "resume remembering" | "resume memory"
    ) {
        return Command::Resume;
    }
    for prefix in [
        "forget ",
        "don't remember ",
        "do not remember ",
        "don't keep ",
        "do not keep ",
    ] {
        if let Some(target) = lower.strip_prefix(prefix) {
            return Command::Forget(target.trim().into());
        }
    }
    if lower.starts_with("remember ") || lower.starts_with("keep in mind ") {
        return Command::Remember;
    }
    Command::None
}
fn normalized(text: &str) -> Vec<String> {
    use unicode_normalization::UnicodeNormalization;
    text.nfkc()
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}
fn forget_text(context: &mut Context, text: &str) -> Result<Option<usize>> {
    if text.len() > MAX_RECORD_BYTES {
        return Ok(None);
    }
    let records = context.store.records(Access::OWNER_ALONE)?;
    let clean = normalized(text).join(" ");
    let ids = if matches!(
        clean.as_str(),
        "everything" | "everything about me" | "all my memories"
    ) {
        records.iter().map(|r| r.id).collect::<BTreeSet<_>>()
    } else if matches!(
        clean.as_str(),
        "that" | "what i just said" | "the last thing i said"
    ) {
        records
            .last()
            .map(|r| BTreeSet::from([r.id]))
            .unwrap_or_default()
    } else {
        let ignored = [
            "what", "i", "told", "you", "said", "about", "my", "the", "a", "an", "that", "please",
            "me",
        ];
        let words = normalized(text)
            .into_iter()
            .filter(|w| !ignored.contains(&w.as_str()))
            .collect::<BTreeSet<_>>();
        if words.is_empty() || words.len() > 64 {
            return Ok(None);
        }
        let matches = |text: &str| {
            let tokens: BTreeSet<_> = normalized(text).into_iter().collect();
            words.iter().all(|w| tokens.contains(w))
        };
        let mut ids: BTreeSet<_> = records
            .iter()
            .filter(|r| matches(&r.text))
            .map(|r| r.id)
            .collect();
        for fact in context.store.facts(Access::OWNER_ALONE)? {
            if matches(&fact.claim) {
                ids.extend(fact.quotes.iter().map(|q| q.record_id));
            }
        }
        ids
    };
    let count = context
        .store
        .forget_records(&ids.into_iter().collect::<Vec<_>>())?;
    prune_sources(context)?;
    Ok(Some(count))
}
fn prune_sources(context: &mut Context) -> Result<()> {
    let retained: BTreeSet<_> = context
        .store
        .records(Access::OWNER_ALONE)?
        .iter()
        .map(|r| r.id)
        .collect();
    context.policy.records.retain(|id, _| retained.contains(id));
    Ok(())
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    if run().await.is_err() {
        eprintln!("tiered memory adapter stopped after a protocol, broker or storage failure");
        std::process::exit(1);
    }
}
async fn run() -> Result<()> {
    let mut wire = Wire {
        input: BufReader::new(tokio::io::stdin()),
        output: tokio::io::stdout(),
    };
    let mut adapter = Adapter::default();
    while let Some(value) = wire.read().await? {
        let request: Request = parse(value)?;
        let response = adapter.dispatch(&request, &mut wire).await;
        let fatal = matches!(
            &response,
            Err(Error::Timeout
                | Error::Model(_)
                | Error::Database(_)
                | Error::Io(_)
                | Error::Stale)
        );
        let response = match response {
            Ok(data) => json!({"v":request.v,"id":request.id,"ok":true,"data":data}),
            Err(error) => {
                json!({"v":request.v,"id":request.id,"ok":false,"error":{"code":match error {Error::Budget=>"budget",Error::Timeout=>"timeout",Error::Invalid(_)=>"invalid",Error::Model(_)=>"model",_=>"storage"},"message":match error {Error::Invalid(s)|Error::Model(s)=>s,_=>"operation failed"}}})
            }
        };
        wire.write(&response).await?;
        if fatal {
            return Err(Error::Model("broker failure ended run"));
        }
        if adapter.closed {
            return Ok(());
        }
    }
    Err(Error::Invalid("EOF before close"))
}
