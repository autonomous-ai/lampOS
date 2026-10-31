//! Versioned adapter messages. Gold answers never belong in these types.
use crate::{Result, invalid};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, Write};

pub const VERSION: u32 = 2;
pub fn supported_version(v: u32) -> bool {
    matches!(v, 1 | 2)
}
pub const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;
pub const RECALL_MS: u64 = 200;
pub const SNAPSHOT_MS: u64 = 300;
pub const LOCAL_MS: u64 = 30_000;
pub const MODEL_MS: u64 = 120_000;
pub const MAX_MODEL_CALLS: usize = 8;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Init,
    Ingest,
    Sleep,
    Forget,
    ProbeBegin,
    Snapshot,
    Recall,
    Tool,
    Inspect,
    ProbeEnd,
    Close,
}
impl Op {
    pub fn budget_ms(self) -> u64 {
        match self {
            Self::Recall => RECALL_MS,
            Self::Snapshot => SNAPSHOT_MS,
            Self::Ingest | Self::Sleep | Self::Tool => MODEL_MS,
            _ => LOCAL_MS,
        }
    }
    pub fn permits_model(self) -> bool {
        matches!(self, Self::Ingest | Self::Sleep | Self::Recall | Self::Tool)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub v: u32,
    pub id: String,
    pub op: Op,
    pub data: Value,
}
impl Request {
    pub fn new(id: impl Into<String>, op: Op, data: Value) -> Self {
        Self {
            v: VERSION,
            id: id.into(),
            op,
            data,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if !supported_version(self.v) || !valid_id(&self.id) || !self.data.is_object() {
            return Err(invalid("invalid request version, id, or data"));
        }
        Ok(())
    }
    pub fn decode<T: serde::de::DeserializeOwned>(&self) -> Result<T> {
        self.validate()?;
        Ok(serde_json::from_value(self.data.clone())?)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Failure {
    pub code: String,
    pub message: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub v: u32,
    pub id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Failure>,
}
impl Response {
    pub fn success(id: &str, data: Value) -> Self {
        Self {
            v: VERSION,
            id: id.into(),
            ok: true,
            data: Some(data),
            error: None,
        }
    }
    pub fn failure(id: &str, code: &str, message: &str) -> Self {
        Self {
            v: VERSION,
            id: id.into(),
            ok: false,
            data: None,
            error: Some(Failure {
                code: code.into(),
                message: message.into(),
            }),
        }
    }
    pub fn checked(self, expected: &str) -> Result<Value> {
        if !supported_version(self.v)
            || self.id != expected
            || self.ok != self.data.is_some()
            || self.ok == self.error.is_some()
        {
            return Err(invalid("invalid terminal response identity or shape"));
        }
        match (self.data, self.error) {
            (Some(data), None) if data.is_object() => Ok(data),
            (None, Some(error)) => Err(invalid(format!(
                "adapter {}: {}",
                error.code, error.message
            ))),
            _ => Err(invalid("response data must be an object")),
        }
    }
}

/// Absent on old v1 records, never defaulted to a successful clear.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ForgetEffect {
    pub ran: bool,
    pub suppress_confirmation: bool,
}
impl ForgetEffect {
    pub fn validate(&self) -> Result<()> {
        if self.suppress_confirmation && !self.ran {
            return Err(invalid("suppressed confirmation without a forget"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Person {
    Owner,
    Known,
    Visitor,
    Unsure,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    OwnerAlone,
    Others,
    Unknown,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Utterance,
    SessionStart,
    SessionEnd,
    Arrive,
    Leave,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Cues {
    pub activity: Option<String>,
    pub objects: Vec<String>,
    pub topic: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub record_id: String,
    pub at_unix_ms: u64,
    pub session_id: String,
    pub kind: EventKind,
    pub person: Person,
    pub person_id: Option<String>,
    pub presence: Presence,
    pub heard: String,
    pub said: String,
    pub stt_confidence: Option<f64>,
    pub cues: Cues,
}
impl Event {
    pub fn validate(&self) -> Result<()> {
        if !valid_id(&self.record_id)
            || !valid_id(&self.session_id)
            || self.person_id.as_ref().is_some_and(|p| !valid_id(p))
        {
            return Err(invalid("invalid event identity"));
        }
        if self
            .stt_confidence
            .is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v))
        {
            return Err(invalid(
                "confidence must be unknown or between zero and one",
            ));
        }
        if self.kind != EventKind::Utterance && (!self.heard.is_empty() || !self.said.is_empty()) {
            return Err(invalid("non-utterance event contains dialogue"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub probe_id: String,
    pub at_unix_ms: u64,
    pub person: Person,
    pub person_id: Option<String>,
    pub presence: Presence,
    pub heard: String,
    pub cues: Cues,
    pub max_context_bytes: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Init {
    pub run_id: String,
    pub arm: String,
    pub state_dir: String,
    pub owner_id: String,
    pub timezone: String,
    pub at_unix_ms: u64,
    pub config: Value,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Ingest {
    pub event: Event,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Sleep {
    pub at_unix_ms: u64,
    pub cadence: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Forget {
    pub at_unix_ms: u64,
    pub person_id: String,
    pub record_ids: Vec<String>,
    pub text: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeBegin {
    pub probe_id: String,
    pub at_unix_ms: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeEnd {
    pub probe_id: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QueryData {
    pub query: Query,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    pub probe_id: String,
    pub name: String,
    pub arguments: Value,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Inspect {
    pub person_id: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub owner_model: Vec<ModelLine>,
    pub source_ids: Vec<String>,
    pub memory_instructions: String,
}
impl Snapshot {
    pub fn prompt_bytes(&self) -> usize {
        self.memory_instructions.len()
            + self.owner_model.iter().map(|l| l.text.len()).sum::<usize>()
    }
}
impl Query {
    pub fn validate(&self) -> Result<()> {
        if !valid_id(&self.probe_id)
            || self.person_id.as_ref().is_some_and(|id| !valid_id(id))
            || matches!(self.person, Person::Owner | Person::Known) && self.person_id.is_none()
            || !(1..=1_048_576).contains(&self.max_context_bytes)
            || self.heard.len() > 16_384
        {
            return Err(invalid("invalid bounded query"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelLine {
    pub text: String,
    pub source_ids: Vec<String>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Recall {
    pub context: String,
    pub source_ids: Vec<String>,
    pub owner_model: Vec<ModelLine>,
    pub tools: Vec<Value>,
    pub memory_instructions: String,
}
impl Recall {
    pub fn prompt_bytes(&self) -> Result<usize> {
        let tools = if self.tools.is_empty() {
            0
        } else {
            serde_json::to_vec(&self.tools)?.len()
        };
        self.owner_model.iter().try_fold(
            self.context
                .len()
                .checked_add(self.memory_instructions.len())
                .and_then(|n| n.checked_add(tools))
                .ok_or_else(|| invalid("memory byte overflow"))?,
            |n, line| {
                n.checked_add(line.text.len())
                    .ok_or_else(|| invalid("memory byte overflow"))
            },
        )
    }
    pub fn check_bound(&self, available: usize) -> Result<()> {
        if self.prompt_bytes()? > available {
            return Err(invalid("memory exceeds prompt byte cap"));
        }
        Ok(())
    }
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-.:".contains(&c))
}
/// Bounds allocation even if a peer never writes a newline. The process
/// transport owns the separate wall-clock watchdog.
pub fn read_frame<R: BufRead>(reader: &mut R) -> Result<Option<Value>> {
    let mut line = Vec::new();
    let n =
        std::io::Read::take(reader, (MAX_FRAME_BYTES + 1) as u64).read_until(b'\n', &mut line)?;
    if n == 0 {
        return Ok(None);
    }
    if n > MAX_FRAME_BYTES || line.last() != Some(&b'\n') {
        return Err(invalid("oversized or truncated frame"));
    }
    let value: Value = serde_json::from_slice(&line)?;
    if !value.is_object() {
        return Err(invalid("frame is not an object"));
    }
    Ok(Some(value))
}
pub fn write_frame<W: Write, T: Serialize>(writer: &mut W, value: &T) -> Result<()> {
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    if line.len() > MAX_FRAME_BYTES {
        return Err(invalid("outgoing frame exceeds bound"));
    }
    writer.write_all(&line)?;
    writer.flush()?;
    Ok(())
}
