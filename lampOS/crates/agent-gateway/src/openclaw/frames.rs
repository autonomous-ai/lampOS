//! Every frame lamp-tasks writes to OpenClaw's gateway, and every frame it
//! reads back, in one place: the shapes of the pinned `openclaw@2026.11.2`
//! (protocol 4, `docs/gateway/protocol/*.md`). A correction to the protocol
//! is a change here only.

use serde_json::{Map, Value, json};

use crate::PlanItem;

/// The one protocol version the pinned gateway speaks.
pub(crate) const PROTOCOL: u64 = 4;
/// Loopback token auth with no device: the id and mode the gateway's own
/// CLI uses. Any other id without a device has its scopes cleared.
pub(crate) const CLIENT_ID: &str = "gateway-client";
pub(crate) const CLIENT_MODE: &str = "backend";
pub(crate) const ROLE: &str = "operator";
pub(crate) const SCOPES: [&str; 4] = [
    "operator.read",
    "operator.write",
    "operator.admin",
    "operator.approvals",
];
/// Without these granted the session can neither hand over nor keep the
/// fence, so a hello that lacks one is a refusal.
/// Without `operator.approvals` no `exec.approval.requested` is delivered.
pub(crate) const NEEDED_SCOPES: [&str; 3] =
    ["operator.write", "operator.admin", "operator.approvals"];
/// The id of the connect request (the only request before hello).
pub(crate) const CONNECT_ID: &str = "lamp-connect";
/// The tick interval before hello says the real one.
pub(crate) const DEFAULT_TICK_MS: u64 = 30_000;
/// Close code of a gateway still starting ("try again later").
pub(crate) const CLOSE_STARTING: u16 = 1013;

pub(crate) mod method {
    pub(crate) const CONNECT: &str = "connect";
    pub(crate) const SESSIONS_LIST: &str = "sessions.list";
    pub(crate) const SESSIONS_RESET: &str = "sessions.reset";
    pub(crate) const CHAT_SEND: &str = "chat.send";
    pub(crate) const CHAT_HISTORY: &str = "chat.history";
    pub(crate) const CHAT_ABORT: &str = "chat.abort";
    pub(crate) const APPROVALS_GET: &str = "exec.approvals.get";
    pub(crate) const APPROVALS_SET: &str = "exec.approvals.set";
    pub(crate) const EXEC_APPROVAL_RESOLVE: &str = "exec.approval.resolve";
    pub(crate) const PLUGIN_APPROVAL_RESOLVE: &str = "plugin.approval.resolve";
}

pub(crate) mod event {
    pub(crate) const CHALLENGE: &str = "connect.challenge";
    pub(crate) const AGENT: &str = "agent";
    pub(crate) const CHAT: &str = "chat";
    pub(crate) const EXEC_APPROVAL: &str = "exec.approval.requested";
    pub(crate) const PLUGIN_APPROVAL: &str = "plugin.approval.requested";
}

/// The connect request. `connect` params are a closed schema on the
/// gateway: only these fields, never a `device` on this path.
pub(crate) fn connect(client_id: &str, version: &str, token: &str, approvals: bool) -> String {
    let mut caps = vec!["tool-events"];
    if approvals {
        caps.extend(["exec-approvals", "approvals", "plugin-approvals"]);
    }
    req(
        CONNECT_ID,
        method::CONNECT,
        json!({
            "minProtocol": PROTOCOL,
            "maxProtocol": PROTOCOL,
            "client": {
                "id": client_id,
                "version": version,
                "platform": "linux",
                "mode": CLIENT_MODE,
            },
            "role": ROLE,
            "scopes": SCOPES,
            "caps": caps,
            "auth": {"token": token},
        }),
    )
}

pub(crate) fn req(id: &str, method: &str, params: Value) -> String {
    json!({"type": "req", "id": id, "method": method, "params": params}).to_string()
}

pub(crate) fn sessions_list() -> Value {
    json!({})
}

pub(crate) fn chat_send(key: &str, message: &str, idem: &str, queue_mode: Option<&str>) -> Value {
    let mut p = json!({
        "sessionKey": key,
        "message": message,
        "idempotencyKey": idem,
        "deliver": false,
    });
    if let Some(mode) = queue_mode {
        p["queueMode"] = json!(mode);
    }
    p
}

pub(crate) fn chat_history(key: &str, limit: u32) -> Value {
    json!({"sessionKey": key, "limit": limit})
}

pub(crate) fn chat_abort(key: &str, run: Option<&str>) -> Value {
    let mut p = json!({"sessionKey": key});
    if let Some(run) = run {
        p["runId"] = json!(run);
    }
    p
}

pub(crate) fn sessions_reset(key: &str) -> Value {
    json!({"key": key, "reason": "new"})
}

pub(crate) fn approvals_get() -> Value {
    json!({})
}

pub(crate) fn approvals_set(file: &Value, base_hash: Option<&str>) -> Value {
    let mut p = json!({"file": file});
    if let Some(hash) = base_hash {
        p["baseHash"] = json!(hash);
    }
    p
}

pub(crate) fn approval_resolve(id: &str, decision: &str) -> Value {
    json!({"id": id, "decision": decision})
}

/// A frame read from the gateway.
#[derive(Debug)]
pub(crate) enum Inbound {
    Res(Res),
    Event(Event),
    Other,
}

#[derive(Debug)]
pub(crate) struct Res {
    pub id: String,
    pub payload: Value,
    pub error: Option<RpcError>,
}

/// `{code, message, details?, retryable?, retryAfterMs?}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RpcError {
    pub code: String,
    pub message: String,
    pub reason: Option<String>,
    pub retryable: bool,
    pub retry_after_ms: Option<u64>,
}

impl RpcError {
    /// `UNAVAILABLE` (startup sidecars, a handler failure): try again.
    pub(crate) fn unavailable(&self) -> bool {
        self.code == "UNAVAILABLE"
    }

    pub(crate) fn words(&self) -> String {
        match &self.reason {
            Some(reason) => format!("{} ({reason}): {}", self.code, self.message),
            None => format!("{}: {}", self.code, self.message),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Event {
    pub name: String,
    pub payload: Value,
    pub seq: Option<u64>,
}

pub(crate) fn parse(text: &str) -> Inbound {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return Inbound::Other;
    };
    match v.get("type").and_then(Value::as_str) {
        Some("res") => {
            let Some(id) = v.get("id").and_then(Value::as_str) else {
                return Inbound::Other;
            };
            let ok = v.get("ok").and_then(Value::as_bool).unwrap_or(false);
            let error = (!ok).then(|| rpc_error(v.get("error").unwrap_or(&Value::Null)));
            Inbound::Res(Res {
                id: id.to_owned(),
                payload: v.get("payload").cloned().unwrap_or(Value::Null),
                error,
            })
        }
        Some("event") => Inbound::Event(Event {
            name: str_of(&v, "event").unwrap_or_default(),
            payload: v.get("payload").cloned().unwrap_or(Value::Null),
            seq: v.get("seq").and_then(Value::as_u64),
        }),
        _ => Inbound::Other,
    }
}

fn rpc_error(e: &Value) -> RpcError {
    RpcError {
        code: str_of(e, "code").unwrap_or_else(|| "UNKNOWN".to_owned()),
        message: str_of(e, "message").unwrap_or_default(),
        reason: e.get("details").and_then(|d| str_of(d, "reason")),
        retryable: e.get("retryable").and_then(Value::as_bool).unwrap_or(false),
        retry_after_ms: e.get("retryAfterMs").and_then(Value::as_u64),
    }
}

/// What `hello-ok` says that lamp-tasks uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Hello {
    pub server_version: String,
    pub scopes: Vec<String>,
    pub tick_ms: u64,
}

pub(crate) fn hello(payload: &Value) -> Hello {
    Hello {
        server_version: payload
            .get("server")
            .and_then(|s| str_of(s, "version"))
            .unwrap_or_else(|| "unknown".to_owned()),
        scopes: payload
            .get("auth")
            .and_then(|a| a.get("scopes"))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        tick_ms: payload
            .get("policy")
            .and_then(|p| p.get("tickIntervalMs"))
            .and_then(Value::as_u64)
            .filter(|ms| *ms > 0)
            .unwrap_or(DEFAULT_TICK_MS),
    }
}

/// An `agent` event: `{runId, seq, stream, ts, data, sessionKey?}`.
#[derive(Clone, Debug)]
pub(crate) struct AgentEvt {
    pub run_id: String,
    pub stream: String,
    pub data: Value,
}

impl AgentEvt {
    pub(crate) fn phase(&self) -> &str {
        self.data.get("phase").and_then(Value::as_str).unwrap_or("")
    }

    pub(crate) fn str(&self, key: &str) -> Option<String> {
        str_of(&self.data, key)
    }
}

pub(crate) fn agent_event(payload: &Value) -> Option<AgentEvt> {
    Some(AgentEvt {
        run_id: str_of(payload, "runId")?,
        stream: str_of(payload, "stream")?,
        data: payload.get("data").cloned().unwrap_or(Value::Null),
    })
}

/// A `chat` event: state `delta`/`final`/`aborted`/`error`/`status`.
#[derive(Clone, Debug)]
pub(crate) struct ChatEvt {
    pub run_id: String,
    pub state: String,
    /// The `message` snapshot's text, when the frame carries one.
    pub snapshot: Option<String>,
    pub delta: Option<String>,
    pub replace: bool,
    pub error: Option<String>,
}

pub(crate) fn chat_event(payload: &Value) -> Option<ChatEvt> {
    Some(ChatEvt {
        run_id: str_of(payload, "runId")?,
        state: str_of(payload, "state")?,
        snapshot: payload.get("message").map(message_text),
        delta: str_of(payload, "deltaText"),
        replace: payload
            .get("replace")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        error: str_of(payload, "errorMessage").or_else(|| str_of(payload, "errorKind")),
    })
}

/// An exec approval prompt: `{id, request: {command, commandArgv?,
/// envKeys?, host, agentId, sessionKey, allowedDecisions, ...}}`.
pub(crate) fn approval_request(payload: &Value) -> Option<super::ApprovalRequest> {
    let id = str_of(payload, "id")?;
    let request = payload.get("request").unwrap_or(payload);
    let strings = |key: &str| {
        request.get(key).and_then(Value::as_array).map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
    };
    Some(super::ApprovalRequest {
        id,
        command: str_of(request, "command").unwrap_or_default(),
        command_argv: strings("commandArgv"),
        env_keys: strings("envKeys").unwrap_or_default(),
        host: str_of(request, "host"),
        agent_id: str_of(request, "agentId"),
        session_key: str_of(request, "sessionKey"),
        allowed_decisions: strings("allowedDecisions").unwrap_or_default(),
        raw: payload.clone(),
    })
}

/// The text of a message: a string, `{text}`, or `{content}` as a string
/// or a list of blocks (only `text` blocks count).
pub(crate) fn message_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Object(o) => {
            if let Some(t) = o.get("text").and_then(Value::as_str) {
                return t.to_owned();
            }
            match o.get("content") {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Array(blocks)) => blocks
                    .iter()
                    .filter(|b| {
                        matches!(b.get("type").and_then(Value::as_str), Some("text") | None)
                    })
                    .filter_map(|b| b.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join(""),
                _ => String::new(),
            }
        }
        _ => String::new(),
    }
}

/// Whether a history message asks for a tool (so it is not a final answer).
fn calls_tool(v: &Value) -> bool {
    if v.get("stopReason").and_then(Value::as_str) == Some("toolUse") {
        return true;
    }
    v.get("content")
        .and_then(Value::as_array)
        .is_some_and(|blocks| {
            blocks.iter().any(|b| {
                matches!(
                    b.get("type").and_then(Value::as_str),
                    Some("toolCall" | "tool_use" | "toolUse" | "tool_call")
                )
            })
        })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistMsg {
    pub role: String,
    /// A user message's `idempotencyKey` (the gateway stores ours as
    /// `<key>:user`), without the suffix.
    pub idem: Option<String>,
    pub text: String,
    pub calls_tool: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InFlight {
    pub run_id: String,
    pub text: String,
    pub plan: Vec<PlanItem>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct History {
    pub messages: Vec<HistMsg>,
    pub in_flight: Option<InFlight>,
    /// `sessionInfo.activeRunIds`: the exact runs going now, when given.
    pub active_ids: Option<Vec<String>>,
    /// The session's tokens: the larger of `sessionInfo.totalTokens` and
    /// `sessionInfo.contextBudgetStatus.estimatedPromptTokens`.
    pub total_tokens: Option<u64>,
    /// The session's messages: `totalMessages` (the whole transcript, not
    /// the page read).
    pub total_messages: Option<u64>,
}

/// `chat.history`'s `{messages, inFlightRun?}`.
pub(crate) fn history(payload: &Value) -> History {
    History {
        messages: payload
            .get("messages")
            .and_then(Value::as_array)
            .map(|ms| {
                ms.iter()
                    .map(|m| HistMsg {
                        role: str_of(m, "role").unwrap_or_default(),
                        idem: str_of(m, "idempotencyKey")
                            .map(|k| k.strip_suffix(":user").unwrap_or(&k).to_owned()),
                        text: message_text(m),
                        calls_tool: calls_tool(m),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        in_flight: payload.get("inFlightRun").and_then(|r| {
            Some(InFlight {
                run_id: str_of(r, "runId")?,
                text: str_of(r, "text").unwrap_or_default(),
                plan: r.get("plan").map(plan).unwrap_or_default(),
            })
        }),
        active_ids: payload
            .get("sessionInfo")
            .and_then(|i| i.get("activeRunIds"))
            .and_then(Value::as_array)
            .map(|ids| {
                ids.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            }),
        total_tokens: payload.get("sessionInfo").and_then(|i| {
            let total = i.get("totalTokens").and_then(Value::as_u64);
            let estimate = i
                .get("contextBudgetStatus")
                .and_then(|c| c.get("estimatedPromptTokens"))
                .and_then(Value::as_u64);
            // A short reply can leave a stale small `totalTokens`: the
            // larger of the two is the session's size.
            total.max(estimate)
        }),
        total_messages: payload.get("totalMessages").and_then(Value::as_u64),
    }
}

/// A plan: `{steps: [{status, title|text|step|content}]}`.
pub(crate) fn plan(data: &Value) -> Vec<PlanItem> {
    data.get("steps")
        .and_then(Value::as_array)
        .map(|steps| {
            steps
                .iter()
                .map(|s| PlanItem {
                    text: ["title", "text", "step", "content", "description"]
                        .iter()
                        .find_map(|k| str_of(s, k))
                        .unwrap_or_default(),
                    done: matches!(
                        s.get("status").and_then(Value::as_str),
                        Some("completed" | "complete" | "done")
                    ),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Whether the approvals `file` lets `agent` exec `pattern` exactly.
pub(crate) fn allowlist_has(file: &Value, agent: &str, pattern: &str) -> bool {
    file.get("agents")
        .and_then(|a| a.get(agent))
        .and_then(|a| a.get("allowlist"))
        .and_then(Value::as_array)
        .is_some_and(|entries| {
            entries
                .iter()
                .any(|e| e.get("pattern").and_then(Value::as_str).map(str::trim) == Some(pattern))
        })
}

/// `file` with `{pattern}` added to `agent`'s allow-list.
pub(crate) fn allowlist_with(mut file: Value, agent: &str, pattern: &str) -> Value {
    if !file.is_object() {
        file = json!({"version": 1});
    }
    let root = file.as_object_mut().expect("an object");
    let agents = root
        .entry("agents")
        .or_insert_with(|| Value::Object(Map::new()));
    if !agents.is_object() {
        *agents = Value::Object(Map::new());
    }
    let entry = agents
        .as_object_mut()
        .expect("an object")
        .entry(agent)
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    let list = entry
        .as_object_mut()
        .expect("an object")
        .entry("allowlist")
        .or_insert_with(|| Value::Array(Vec::new()));
    if !list.is_array() {
        *list = Value::Array(Vec::new());
    }
    list.as_array_mut()
        .expect("an array")
        .push(json!({"pattern": pattern}));
    file
}

pub(crate) fn str_of(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}
