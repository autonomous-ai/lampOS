//! One operation at a time, with model RPC through the run's shared client.
use crate::{
    Result, invalid, model::Call, model_worker::Worker, protocol::*, session::Session,
    transport::Process,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[derive(Debug, Serialize)]
pub struct Outcome {
    pub data: Value,
    pub elapsed_us: u64,
    pub missed_deadline: bool,
}
pub struct MemoryPrompt {
    pub text: String,
    pub tools: Vec<Value>,
    pub snapshot_missed: bool,
    pub recall_missed: bool,
    pub recall_skipped: bool,
    pub memory_text_bytes: usize,
    pub framing_bytes: usize,
    epoch: u64,
}
impl MemoryPrompt {
    pub fn require_current(&self, runner: &Runner) -> Result<()> {
        if self.epoch != runner.memory_epoch {
            return Err(invalid("stale memory prompt after mutation"));
        }
        Ok(())
    }
}
struct Pending {
    worker: JoinHandle<Result<(Process, BTreeSet<String>)>>,
    cancelled: Arc<AtomicBool>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelEnvelope {
    v: u32,
    id: String,
    model_request: Value,
}
pub struct Runner {
    process: Option<Process>,
    session: Session,
    pending: Option<Pending>,
    model_ids: BTreeSet<String>,
    state_dir: PathBuf,
    memory_epoch: u64,
}
impl Runner {
    pub fn spawn(executable: &Path, args: &[String], state_dir: &Path) -> Result<Self> {
        Ok(Self {
            process: Some(Process::spawn(executable, args, state_dir)?),
            session: Session::default(),
            pending: None,
            model_ids: BTreeSet::new(),
            state_dir: std::fs::canonicalize(state_dir)?,
            memory_epoch: 0,
        })
    }
    fn process(&mut self) -> Result<&mut Process> {
        self.process
            .as_mut()
            .ok_or_else(|| invalid("adapter is stopped"))
    }
    /// Always obtain a new snapshot and recall before an answer. Keeping no
    /// cross-turn memory cache is the simplest implementation of v2 invalidation.
    pub fn memory(&mut self, query: &Query, client: &mut Worker) -> Result<MemoryPrompt> {
        query.validate()?;
        self.memory_epoch = self
            .memory_epoch
            .checked_add(1)
            .ok_or_else(|| invalid("memory generation overflow"))?;
        let epoch = self.memory_epoch;
        let mut snapshot_query = query.clone();
        snapshot_query.heard.clear();
        let snapshot = self.execute(
            Request::new(
                format!("snapshot-{epoch}"),
                Op::Snapshot,
                json!({"query":snapshot_query}),
            ),
            client,
        )?;
        // A single child cannot accept recall while its late snapshot drains.
        // Let the answer proceed empty within the snapshot budget; the owned
        // background drain is reconciled before the next stateful operation.
        if snapshot.missed_deadline {
            return Ok(MemoryPrompt {
                text: String::new(),
                tools: Vec::new(),
                snapshot_missed: true,
                recall_missed: false,
                recall_skipped: true,
                memory_text_bytes: 0,
                framing_bytes: 0,
                epoch,
            });
        }
        let recall = self.execute(
            Request::new(
                format!("recall-{epoch}"),
                Op::Recall,
                json!({"query":query}),
            ),
            client,
        )?;
        let snapshot_value: Snapshot = serde_json::from_value(snapshot.data)?;
        let recall_value: Recall = serde_json::from_value(recall.data)?;
        let memory_text_bytes = snapshot_value
            .prompt_bytes()
            .checked_add(recall_value.prompt_bytes()?)
            .ok_or_else(|| invalid("memory text byte overflow"))?;
        if memory_text_bytes > query.max_context_bytes {
            return Err(invalid("combined memory text exceeds prompt cap"));
        }
        let empty = memory_text_bytes == 0;
        let text = if empty {
            String::new()
        } else {
            serde_json::to_string(&json!({"snapshot":snapshot_value,"recall":{
                "context":recall_value.context,"source_ids":recall_value.source_ids,
                "owner_model":recall_value.owner_model,"memory_instructions":recall_value.memory_instructions
            }}))?
        };
        let tool_bytes = if recall_value.tools.is_empty() {
            0
        } else {
            serde_json::to_vec(&recall_value.tools)?.len()
        };
        let framing_bytes = text
            .len()
            .saturating_add(tool_bytes)
            .saturating_sub(memory_text_bytes);
        Ok(MemoryPrompt {
            text,
            tools: recall_value.tools,
            snapshot_missed: snapshot.missed_deadline,
            recall_missed: recall.missed_deadline,
            recall_skipped: false,
            memory_text_bytes,
            framing_bytes,
            epoch,
        })
    }
    fn model(
        &mut self,
        frame: Value,
        request: &Request,
        calls: &mut usize,
        deadline: Instant,
        client: &mut Worker,
    ) -> Result<Option<Value>> {
        let (call_id, call) = decode_model(frame, request, calls, &mut self.model_ids)?;
        let reply = match client.call(&call, deadline) {
            Ok(reply) => {
                let mut value = serde_json::to_value(reply)?;
                let map = value
                    .as_object_mut()
                    .ok_or_else(|| invalid("invalid model reply"))?;
                // Keep the documented chat and embedding shapes disjoint.
                if matches!(call, Call::Chat { .. }) {
                    map.remove("vectors");
                } else {
                    map.remove("content");
                    map.remove("tool_calls");
                }
                map.insert("call_id".into(), json!(call_id));
                map.insert("ok".into(), json!(true));
                value
            }
            Err(error) if client.stopped() => return Err(error),
            Err(_) => {
                json!({"call_id":call_id,"ok":false,"error":{"code":"model_failed","message":"model request refused or failed; see run receipts"}})
            }
        };
        let frame = json!({"v":request.v,"id":request.id,"model_response":reply});
        if Instant::now() >= deadline {
            return Ok(Some(
                json!({"v":request.v,"id":request.id,"model_response":{
                    "call_id":call_id,"ok":false,"error":{"code":"deadline","message":"memory deadline expired"}
                }}),
            ));
        }
        self.process()?.send(&frame, deadline)?;
        Ok(None)
    }
    fn drain(&mut self) -> Result<()> {
        let Some(pending) = self.pending.take() else {
            return Ok(());
        };
        let (process, model_ids) = pending
            .worker
            .join()
            .map_err(|_| invalid("adapter drain panicked"))??;
        self.process = Some(process);
        self.model_ids = model_ids;
        Ok(())
    }
    pub fn execute(&mut self, request: Request, client: &mut Worker) -> Result<Outcome> {
        // Reject an invalid caller operation without altering the live adapter.
        let next = self.session.prepare(&request)?;
        if request.op == Op::Init {
            let init: Init = request.decode()?;
            if std::fs::canonicalize(&init.state_dir)? != self.state_dir {
                return Err(invalid(
                    "init state directory differs from spawned directory",
                ));
            }
        }
        if client.stopped() {
            return Err(invalid("model client stopped after accounting failure"));
        }
        let result = self.exchange(&request, client);
        match result {
            Ok(outcome) => {
                if let Err(error) = check_sources(&outcome.data, &next) {
                    self.process.take();
                    return Err(error);
                }
                if matches!(
                    request.op,
                    Op::Init
                        | Op::Ingest
                        | Op::Sleep
                        | Op::Forget
                        | Op::Tool
                        | Op::ProbeBegin
                        | Op::ProbeEnd
                        | Op::Close
                ) {
                    self.memory_epoch = self
                        .memory_epoch
                        .checked_add(1)
                        .ok_or_else(|| invalid("memory generation overflow"))?;
                }
                self.session = next;
                Ok(outcome)
            }
            Err(error) => {
                // An adapter failure may have partially mutated state. Never
                // continue a score from that uncertain state.
                self.process.take();
                Err(error)
            }
        }
    }
    fn exchange(&mut self, request: &Request, client: &mut Worker) -> Result<Outcome> {
        self.drain()?;
        let started = Instant::now();
        let deadline = started + Duration::from_millis(request.op.budget_ms());
        self.process()?.send(request, deadline)?;
        let mut calls = 0;
        let mut model_response = None;
        loop {
            let frame = self.process()?.receive(deadline)?;
            let Some(frame) = frame else {
                if !matches!(request.op, Op::Snapshot | Op::Recall) {
                    return Err(invalid("adapter operation deadline expired"));
                }
                let process = self
                    .process
                    .take()
                    .ok_or_else(|| invalid("adapter is stopped"))?;
                self.pending = Some(start_drain(
                    process,
                    request.clone(),
                    started + Duration::from_millis(LOCAL_MS),
                    calls,
                    std::mem::take(&mut self.model_ids),
                    model_response,
                )?);
                let data = if request.op == Op::Snapshot {
                    serde_json::to_value(Snapshot::default())?
                } else {
                    serde_json::to_value(Recall::default())?
                };
                return Ok(Outcome {
                    data,
                    elapsed_us: started.elapsed().as_micros() as u64,
                    missed_deadline: true,
                });
            };
            if frame.get("model_request").is_some() {
                model_response = self.model(frame, request, &mut calls, deadline, client)?;
                continue;
            }
            let response: Response = serde_json::from_value(frame)?;
            if response.v != request.v {
                return Err(invalid("adapter response version mismatch"));
            }
            let data = response.checked(&request.id)?;
            validate_response(request, &data)?;
            if request.op == Op::Close {
                self.process()?.finish(deadline)?;
            }
            return Ok(Outcome {
                data,
                elapsed_us: started.elapsed().as_micros() as u64,
                missed_deadline: false,
            });
        }
    }
}
impl Drop for Runner {
    fn drop(&mut self) {
        if let Some(pending) = self.pending.take() {
            pending.cancelled.store(true, Ordering::Release);
            let _ = pending.worker.join();
        }
    }
}
fn decode_model(
    frame: Value,
    request: &Request,
    calls: &mut usize,
    ids: &mut BTreeSet<String>,
) -> Result<(String, Call)> {
    let envelope: ModelEnvelope = serde_json::from_value(frame)?;
    if envelope.v != request.v || envelope.id != request.id || !request.op.permits_model() {
        return Err(invalid("model request outside a permitted operation"));
    }
    let mut body = envelope.model_request;
    let id = body
        .as_object_mut()
        .and_then(|m| m.remove("call_id"))
        .and_then(|v| v.as_str().map(str::to_owned))
        .filter(|id| valid_id(id))
        .ok_or_else(|| invalid("invalid model call id"))?;
    if !ids.insert(id.clone()) || *calls >= MAX_MODEL_CALLS || ids.len() > 100_000 {
        return Err(invalid("duplicate model id or model call limit"));
    }
    *calls += 1;
    let call: Call = serde_json::from_value(body)?;
    call.validate()?;
    if !call.arm_permitted() {
        return Err(invalid("adapter cannot request answer or judge calls"));
    }
    Ok((id, call))
}
fn start_drain(
    mut process: Process,
    request: Request,
    deadline: Instant,
    mut calls: usize,
    mut ids: BTreeSet<String>,
    response: Option<Value>,
) -> Result<Pending> {
    let cancelled = Arc::new(AtomicBool::new(false));
    let stop = cancelled.clone();
    let worker=std::thread::Builder::new().name("memory-late-drain".into()).spawn(move || {
        if let Some(response)=response {process.send(&response,deadline)?;}
        loop {
            if stop.load(Ordering::Acquire) {return Err(invalid("adapter drain cancelled"));}
            if Instant::now()>=deadline {return Err(invalid("late memory response exceeded outer watchdog"));}
            let wake=deadline.min(Instant::now()+Duration::from_millis(50));
            let Some(frame)=process.receive(wake)? else {continue;};
            if frame.get("model_request").is_some() {
                let (id,_)=decode_model(frame,&request,&mut calls,&mut ids)?;
                process.send(&json!({"v":request.v,"id":request.id,"model_response":{
                    "call_id":id,"ok":false,"error":{"code":"deadline","message":"memory deadline expired"}
                }}),deadline)?;
            } else {
                let response:Response=serde_json::from_value(frame)?;
                if response.v != request.v {return Err(invalid("late response version mismatch"));}
                let data=response.checked(&request.id)?;
                validate_response(&request,&data)?;
                return Ok((process,ids));
            }
        }
    })?;
    Ok(Pending { worker, cancelled })
}
fn validate_response(request: &Request, data: &Value) -> Result<()> {
    if matches!(request.op, Op::Ingest | Op::Forget | Op::Tool) {
        match data.get("forget_effect") {
            Some(value) => serde_json::from_value::<ForgetEffect>(value.clone())?.validate()?,
            None if request.v >= 2 => {
                return Err(invalid("version 2 mutation omitted forgetting effect"));
            }
            None => {}
        }
    }
    match request.op {
        Op::Init => {
            let init: Init = request.decode()?;
            let revision = data
                .get("implementation_revision")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("init omitted implementation revision"))?;
            if data.get("arm").and_then(Value::as_str) != Some(init.arm.as_str())
                || revision.len() != 40
                || !revision.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(invalid("wrong arm or unpinned implementation revision"));
            }
            let capabilities = data
                .get("capabilities")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid("init omitted capabilities"))?;
            for required in [
                "ingest",
                "sleep",
                "forget",
                "probe_begin",
                "snapshot",
                "recall",
                "inspect",
                "probe_end",
                "close",
            ] {
                if !capabilities.iter().any(|v| v.as_str() == Some(required)) {
                    return Err(invalid("adapter lacks a required capability"));
                }
            }
        }
        Op::Ingest => {
            boolean(data, "accepted")?;
            array(data, "source_ids")?;
        }
        Op::Forget => {
            boolean(data, "applied")?;
            if data.get("removed_count").and_then(Value::as_u64).is_none() {
                return Err(invalid("forget omitted removed count"));
            }
        }
        Op::Tool => {
            if data.get("result").is_none() {
                return Err(invalid("tool omitted result"));
            }
            array(data, "source_ids")?;
        }
        Op::Inspect => {
            let _: Vec<ModelLine> = serde_json::from_value(
                data.get("owner_model")
                    .cloned()
                    .ok_or_else(|| invalid("inspect omitted owner model"))?,
            )?;
            array(data, "retained_record_ids")?;
            if !data.get("stats").is_some_and(Value::is_object) {
                return Err(invalid("inspect omitted statistics"));
            }
        }
        Op::Recall => {
            let recall: Recall = serde_json::from_value(data.clone())?;
            let query: QueryData = request.decode()?;
            recall.check_bound(query.query.max_context_bytes)?;
        }
        Op::Snapshot => {
            let snapshot: Snapshot = serde_json::from_value(data.clone())?;
            let query: QueryData = request.decode()?;
            if snapshot.prompt_bytes() > query.query.max_context_bytes {
                return Err(invalid("snapshot exceeds memory cap"));
            }
        }
        Op::ProbeBegin if data.get("isolated") != Some(&Value::Bool(true)) => {
            return Err(invalid("adapter did not establish probe isolation"));
        }
        Op::ProbeEnd if data.get("restored") != Some(&Value::Bool(true)) => {
            return Err(invalid("adapter did not discard probe state"));
        }
        Op::Sleep => {
            boolean(data, "applied")?;
            array(data, "source_ids")?;
            if !data
                .get("reason")
                .is_some_and(|v| v.is_string() || v.is_null())
            {
                return Err(invalid("sleep omitted reason"));
            }
            let pending = data
                .get("pending")
                .and_then(Value::as_bool)
                .ok_or_else(|| invalid("sleep omitted pending work"))?;
            let cursor = data
                .get("next_cursor")
                .ok_or_else(|| invalid("sleep omitted cursor"))?;
            if pending && cursor.as_str().is_none_or(|s| s.is_empty())
                || !pending && !cursor.is_null()
            {
                return Err(invalid("sleep pending/cursor mismatch"));
            }
        }
        Op::Close if data.as_object().is_none_or(|o| !o.is_empty()) => {
            return Err(invalid("close response must be empty"));
        }
        _ => {}
    }
    Ok(())
}
fn boolean(data: &Value, key: &str) -> Result<()> {
    if !data.get(key).is_some_and(Value::is_boolean) {
        return Err(invalid(format!("response omitted boolean {key}")));
    }
    Ok(())
}
fn array(data: &Value, key: &str) -> Result<()> {
    if !data.get(key).is_some_and(Value::is_array) {
        return Err(invalid(format!("response omitted array {key}")));
    }
    Ok(())
}
fn check_sources(data: &Value, session: &Session) -> Result<()> {
    for key in ["source_ids", "retained_record_ids"] {
        if let Some(value) = data.get(key) {
            let ids = value
                .as_array()
                .ok_or_else(|| invalid("source ids must be an array"))?;
            if ids
                .iter()
                .any(|id| id.as_str().is_none_or(|id| !session.has_source(id)))
            {
                return Err(invalid(
                    "adapter cited a source outside its admitted input prefix",
                ));
            }
        }
    }
    if let Some(Value::Array(lines)) = data.get("owner_model") {
        for line in lines {
            check_sources(line, session)?;
        }
    }
    Ok(())
}
