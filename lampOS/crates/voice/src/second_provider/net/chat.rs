//! A streamed OpenAI-compatible chat completion with function tools: text
//! as it is written, then the tool calls the model asked for. The caller
//! runs the tools and sends their results back as `tool` messages.
use super::{Error, Result, chunk, http_client, openai_base, post};
use crate::second_config::SecondConfig;
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::Arc;

/// One message of the conversation, in the wire's own shape.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Message {
    pub role: &'static str,
    /// Text, or for a look the text and its picture as parts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// The model's reasoning for its calls, given back as it came.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_details: Option<Value>,
}

impl Message {
    fn new(role: &'static str, content: Option<String>) -> Self {
        Self {
            role,
            content: content.map(Value::String),
            tool_calls: Vec::new(),
            tool_call_id: None,
            reasoning_details: None,
        }
    }

    /// [`Message::assistant_calls`], with the reasoning the model gave for
    /// them, if any ([`ChatEvent::Reasoning`]).
    pub fn assistant_calls_with(
        text: Option<String>,
        calls: &[ToolCall],
        reasoning: Option<Value>,
    ) -> Self {
        let mut message = Self::assistant_calls(text, calls);
        message.reasoning_details = reasoning;
        message
    }
    pub fn system(text: impl Into<String>) -> Self {
        Self::new("system", Some(text.into()))
    }
    pub fn user(text: impl Into<String>) -> Self {
        Self::new("user", Some(text.into()))
    }
    /// What the person asked, with the picture the lamp's eye took for it,
    /// as an OpenAI image part (a JPEG `data:` URL).
    pub fn user_seeing(text: impl Into<String>, jpeg: &[u8]) -> Self {
        use base64::{Engine, engine::general_purpose::STANDARD};
        let mut message = Self::new("user", None);
        message.content = Some(json!([
            {"type": "text", "text": text.into()},
            {"type": "image_url", "image_url": {"url": format!("data:image/jpeg;base64,{}", STANDARD.encode(jpeg))}},
        ]));
        message
    }
    pub fn assistant(text: impl Into<String>) -> Self {
        Self::new("assistant", Some(text.into()))
    }
    /// The model's turn that asked for these calls, with what it said first.
    pub fn assistant_calls(text: Option<String>, calls: &[ToolCall]) -> Self {
        let mut message = Self::new("assistant", text.filter(|t| !t.is_empty()));
        message.tool_calls = calls
            .iter()
            .map(|call| {
                let mut asked = json!({
                    "id": call.id,
                    "type": "function",
                    "function": {"name": call.name, "arguments": call.args.to_string()},
                });
                // Given back as it came: Gemini's thought signature rides
                // here, and a call sent back without it is refused.
                if let Some(extra) = &call.extra {
                    asked["extra_content"] = extra.clone();
                }
                asked
            })
            .collect();
        message
    }
    /// One call's result.
    pub fn tool(call_id: impl Into<String>, result: impl Into<String>) -> Self {
        let mut message = Self::new("tool", Some(result.into()));
        message.tool_call_id = Some(call_id.into());
        message
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// The arguments object; `{}` when the model sent none.
    pub args: Value,
    /// The call's `extra_content`, as the service sent it: through Gemini's
    /// OpenAI-compatible endpoint the model's thought signature, which the
    /// next round must carry back with the call ([`Message::assistant_calls`]).
    pub extra: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChatEvent {
    Text(String),
    /// Every call of the turn, once the model has finished asking.
    ToolCalls(Vec<ToolCall>),
    /// The model's `reasoning_details` for the calls of this round, given
    /// back unchanged with them in the next ([`Message::assistant_calls_with`]),
    /// as OpenRouter asks; before [`ChatEvent::ToolCalls`].
    Reasoning(Value),
    /// The service searched the web for this answer: the first chunk that
    /// carried its results (`annotations`, OpenRouter's web plugin). Once.
    Searched,
    Done,
}

/// The lamp's tools as OpenAI function declarations.
fn functions(tools: &[lamp_gemini::Tool]) -> Vec<Value> {
    tools
        .iter()
        .map(|tool| {
            let parameters = tool
                .parameters
                .clone()
                .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
            json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": parameters,
                },
            })
        })
        .collect()
}

/// The lamp's tools as they stand now: asked at each chat request, as a
/// Live session is told them when it opens (`thread::Recalling::equipped`),
/// so the executive's come and go with the executive.
pub type Declare = Arc<dyn Fn() -> Vec<lamp_gemini::Tool> + Send + Sync>;

/// How much Gemini thinks before it answers through its OpenAI-compatible
/// endpoint: the floor, as thinking cannot be turned off on Gemini 3 (the
/// research of 19 Oct; 0.80 to 0.83 s to the first token, published).
pub const GEMINI_REASONING: &str = "minimal";

pub struct Chat {
    client: reqwest::Client,
    url: String,
    auth: String,
    model: String,
    /// Fields the request also carries (`llm_extra`, or Gemini's reasoning).
    extra: serde_json::Map<String, Value>,
    /// The model takes a look's picture.
    sees: bool,
    declare: Declare,
    /// When the last request went, for the journal: a connection idle past
    /// the client's pool keep (`http_client`, 90 s) is made anew.
    last_sent: std::sync::Mutex<Option<std::time::Instant>>,
}

impl Chat {
    /// The configured OpenAI-compatible chat (`llm_*`, the relay on the
    /// lamps), with `llm_extra`.
    pub fn new(config: &SecondConfig, declare: Declare) -> Result<Self> {
        let mut chat = Self::with(
            &config.llm_base_url,
            &config.llm_model,
            config.llm_key.expose(),
            declare,
        )?;
        chat.extra = config.llm_extra.clone();
        Ok(chat)
    }

    /// Gemini called directly, through its OpenAI-compatible endpoint
    /// (`<base>/openai/chat/completions`, the key as a bearer token), with
    /// thinking at [`GEMINI_REASONING`]. The lamp's tools and a look's
    /// picture go as they go to the configured chat.
    pub fn gemini(base_url: &str, model: &str, key: &str, declare: Declare) -> Result<Self> {
        let mut chat = Self::with(
            &format!("{}/openai", base_url.trim_end_matches('/')),
            model,
            key,
            declare,
        )?;
        chat.extra
            .insert("reasoning_effort".into(), json!(GEMINI_REASONING));
        Ok(chat)
    }

    /// OpenRouter's OpenAI-compatible chat (`LAMP_SECOND_CHAT=openrouter`):
    /// `model` a slug of its public list, its providers tried fastest first
    /// (`provider.sort: "latency"`), thinking at `effort`
    /// (`reasoning.effort`), and with `search` (`LAMP_SECOND_SEARCH=openrouter`)
    /// its web plugin on Exa (`plugins`, the `instant` mode, three results),
    /// which searches before every answer. Read from OpenRouter's docs on 19
    /// Oct. The lamp's tools and a look's picture go as for the relay.
    pub fn openrouter(
        model: &str,
        effort: &str,
        key: &str,
        declare: Declare,
        search: bool,
    ) -> Result<Self> {
        Self::openrouter_at(
            crate::second_config::OPENROUTER_BASE_URL,
            model,
            effort,
            key,
            declare,
            search,
        )
    }

    pub(crate) fn openrouter_at(
        base_url: &str,
        model: &str,
        effort: &str,
        key: &str,
        declare: Declare,
        search: bool,
    ) -> Result<Self> {
        let mut chat = Self::with(base_url, model, key, declare)?;
        chat.extra
            .insert("provider".into(), json!({"sort": "latency"}));
        chat.extra
            .insert("reasoning".into(), json!({"effort": effort}));
        if search {
            chat.extra.insert(
                "plugins".into(),
                json!([{"id": "web", "engine": "exa", "mode": "instant", "max_results": 3}]),
            );
        }
        Ok(chat)
    }

    /// DeepSeek's own OpenAI-compatible chat (`LAMP_SECOND_CHAT=deepseek`),
    /// with thinking set, never left to its default (on): off for the lamp,
    /// on only for the probe. A model that takes no picture
    /// ([`crate::second_config::DEEPSEEK_TEXT_ONLY`]) is never sent one.
    pub fn deepseek(model: &str, key: &str, declare: Declare, thinking: bool) -> Result<Self> {
        Self::deepseek_at(
            crate::second_config::DEEPSEEK_BASE_URL,
            model,
            key,
            declare,
            thinking,
        )
    }

    pub(crate) fn deepseek_at(
        base_url: &str,
        model: &str,
        key: &str,
        declare: Declare,
        thinking: bool,
    ) -> Result<Self> {
        let mut chat = Self::with(base_url, model, key, declare)?;
        chat.extra.insert(
            "thinking".into(),
            json!({"type": if thinking { "enabled" } else { "disabled" }}),
        );
        chat.sees = !crate::second_config::DEEPSEEK_TEXT_ONLY.contains(&model);
        Ok(chat)
    }

    /// Whether the model is sent a look's picture.
    pub fn sees(&self) -> bool {
        self.sees
    }

    fn with(base_url: &str, model: &str, key: &str, declare: Declare) -> Result<Self> {
        Ok(Self {
            client: http_client()?,
            url: format!("{}/chat/completions", openai_base(base_url)),
            auth: format!("Bearer {key}"),
            model: model.to_owned(),
            extra: serde_json::Map::new(),
            sees: true,
            declare,
            last_sent: std::sync::Mutex::new(None),
        })
    }

    /// The request's body, with the tools declared now.
    fn body(&self, messages: &[Message]) -> Value {
        let mut body = json!({"model": self.model, "stream": true, "messages": messages});
        let tools = functions(&(self.declare)());
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools);
        }
        // The configuration refuses any of the request's own fields here.
        for (field, value) in &self.extra {
            if body.get(field).is_none() {
                body[field] = value.clone();
            }
        }
        body
    }

    pub async fn send(&self, messages: &[Message]) -> Result<ChatStream> {
        let body = self.body(messages);
        let asked = std::time::Instant::now();
        let before = self
            .last_sent
            .lock()
            .ok()
            .and_then(|mut last| last.replace(asked));
        let response = post(
            &self.client,
            &self.url,
            ("authorization", &self.auth),
            &body,
        )
        .await?;
        // What was asked and how soon the service began to answer, without
        // words (#1712, the first builder at 18:26: the chat's first event
        // came in 0.9 to 1.9 s on lamp-4ace, 0.7 s from the probe).
        eprintln!(
            "lamp-voice second: {}",
            asked_told(
                &body,
                asked.elapsed(),
                before.map(|at| asked.duration_since(at))
            )
        );
        Ok(ChatStream {
            response,
            lines: Lines::default(),
            deltas: Deltas::default(),
            ready: Vec::new(),
            done: false,
        })
    }
}

/// A chat request in the journal's words: its size, its messages and tools,
/// how soon its answer began (the response's headers), and how long before
/// it the last request went, which says whether its connection was kept.
fn asked_told(
    body: &Value,
    began: std::time::Duration,
    since_last: Option<std::time::Duration>,
) -> String {
    let count = |field: &str| body[field].as_array().map_or(0, Vec::len);
    format!(
        "chat request {} bytes, {} messages, {} tools; its answer began in {} ms; {}",
        body.to_string().len(),
        count("messages"),
        count("tools"),
        began.as_millis(),
        since_last.map_or(
            "the first request of this client".to_owned(),
            |since| format!("the last request {} s before", since.as_secs())
        )
    )
}

pub struct ChatStream {
    response: reqwest::Response,
    lines: Lines,
    deltas: Deltas,
    /// Decoded events not yet handed out, newest last.
    ready: Vec<ChatEvent>,
    done: bool,
}

impl ChatStream {
    /// The next event; `None` after `Done`. Each wait for the service is
    /// bounded by the client's stall limit.
    pub async fn next(&mut self) -> Result<Option<ChatEvent>> {
        loop {
            if !self.ready.is_empty() {
                return Ok(Some(self.ready.remove(0)));
            }
            if self.done {
                return Ok(None);
            }
            match chunk(&mut self.response).await? {
                Some(bytes) => {
                    for data in self.lines.push(&bytes) {
                        self.ready.extend(self.deltas.data(&data)?);
                    }
                    if self.lines.partial.len() > MAX_LINE {
                        return Err(Error::Protocol("chat stream line length"));
                    }
                }
                None => self.ready.extend(self.deltas.end()?),
            }
            self.done |= self.ready.last() == Some(&ChatEvent::Done);
        }
    }
}

/// No completion chunk comes near this; a stream without line ends does.
const MAX_LINE: usize = 1 << 20;

/// Server-sent events' `data:` payloads from bytes cut anywhere.
#[derive(Default)]
pub(crate) struct Lines {
    partial: Vec<u8>,
}

impl Lines {
    /// Bytes of a line not yet ended.
    pub fn pending(&self) -> usize {
        self.partial.len()
    }

    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.partial.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(end) = self.partial.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.partial.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line);
            if let Some(data) = line.trim_end_matches(['\r', '\n']).strip_prefix("data:") {
                out.push(data.trim_start().to_owned());
            }
        }
        out
    }
}

#[derive(Default)]
struct Pending {
    id: String,
    name: String,
    arguments: String,
    extra: Option<Value>,
}

/// Chunks of the completion to events. Tool calls arrive in fragments
/// keyed by `index` and are handed out whole at the end.
#[derive(Default)]
pub(crate) struct Deltas {
    calls: Vec<Pending>,
    /// A chunk said why the completion stopped (`finish_reason`).
    finished: bool,
    /// `reasoning_details` as they came, in order.
    reasoning: Vec<Value>,
    /// A chunk carried the web search's results.
    searched: bool,
}

impl Deltas {
    /// The stream ended. Without `[DONE]` it is whole only if a chunk said
    /// why the completion stopped; otherwise it was cut, and the words so
    /// far are not the answer.
    pub fn end(&mut self) -> Result<Vec<ChatEvent>> {
        if self.finished {
            self.data("[DONE]")
        } else {
            Err(Error::Protocol("chat stream cut"))
        }
    }

    pub fn data(&mut self, data: &str) -> Result<Vec<ChatEvent>> {
        if data == "[DONE]" {
            let mut out = Vec::new();
            if !self.calls.is_empty() {
                if !self.reasoning.is_empty() {
                    out.push(ChatEvent::Reasoning(Value::Array(std::mem::take(
                        &mut self.reasoning,
                    ))));
                }
                out.push(ChatEvent::ToolCalls(self.take_calls()?));
            }
            out.push(ChatEvent::Done);
            return Ok(out);
        }
        let chunk: Value =
            serde_json::from_str(data).map_err(|_| Error::Protocol("chat stream data"))?;
        let mut out = Vec::new();
        self.finished |= chunk
            .pointer("/choices/0/finish_reason")
            .is_some_and(Value::is_string);
        let Some(delta) = chunk.pointer("/choices/0/delta") else {
            return Ok(out);
        };
        if !self.searched
            && delta["annotations"]
                .as_array()
                .is_some_and(|annotations| !annotations.is_empty())
        {
            self.searched = true;
            out.push(ChatEvent::Searched);
        }
        if let Some(details) = delta["reasoning_details"].as_array() {
            if self.reasoning.len() + details.len() > 256 {
                return Err(Error::Protocol("reasoning details"));
            }
            self.reasoning.extend(details.iter().cloned());
        }
        if let Some(text) = delta["content"].as_str().filter(|t| !t.is_empty()) {
            out.push(ChatEvent::Text(text.to_owned()));
        }
        for call in delta["tool_calls"].as_array().into_iter().flatten() {
            let id = call["id"].as_str().filter(|id| !id.is_empty());
            let mut index = call["index"].as_u64().unwrap_or(0) as usize;
            // A whole new call at an index already taken by another (a
            // service that sends each call whole, without its own index) is
            // the next call, not more of that one.
            if let Some(id) = id
                && self
                    .calls
                    .get(index)
                    .is_some_and(|pending| !pending.id.is_empty() && pending.id != id)
            {
                index = self.calls.len();
            }
            if index >= 64 {
                return Err(Error::Protocol("tool call index"));
            }
            if self.calls.len() <= index {
                self.calls.resize_with(index + 1, Pending::default);
            }
            let pending = &mut self.calls[index];
            if let Some(id) = id {
                pending.id = id.to_owned();
            }
            if let Some(extra) = call.get("extra_content").filter(|extra| extra.is_object()) {
                pending.extra = Some(extra.clone());
            }
            if let Some(name) = call.pointer("/function/name").and_then(Value::as_str) {
                pending.name.push_str(name);
            }
            if let Some(part) = call.pointer("/function/arguments").and_then(Value::as_str) {
                pending.arguments.push_str(part);
            }
        }
        Ok(out)
    }

    fn take_calls(&mut self) -> Result<Vec<ToolCall>> {
        std::mem::take(&mut self.calls)
            .into_iter()
            .filter(|call| !call.name.is_empty())
            .enumerate()
            .map(|(index, call)| {
                let args = if call.arguments.trim().is_empty() {
                    json!({})
                } else {
                    serde_json::from_str(&call.arguments)
                        .map_err(|_| Error::Protocol("tool call arguments"))?
                };
                Ok(ToolCall {
                    // Its result is matched to it by id: one is made for a
                    // call that came without.
                    id: if call.id.is_empty() {
                        format!("call_{index}")
                    } else {
                        call.id
                    },
                    name: call.name,
                    args,
                    extra: call.extra,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #1712, the first builder at 18:26: a chat request is journaled by its
    /// size and counts, how soon its answer began, and the gap since the
    /// last request; never its words.
    #[test]
    fn a_chat_request_is_journaled_without_words() {
        let body = json!({
            "model": "m",
            "messages": [{"role": "user", "content": "secret words"}],
            "tools": [{"type": "function"}, {"type": "function"}],
        });
        let told = asked_told(
            &body,
            std::time::Duration::from_millis(412),
            Some(std::time::Duration::from_secs(95)),
        );
        assert_eq!(
            told,
            format!(
                "chat request {} bytes, 1 messages, 2 tools; its answer began in 412 ms; the last request 95 s before",
                body.to_string().len()
            )
        );
        assert!(!told.contains("secret"));
        assert!(
            asked_told(&json!({}), std::time::Duration::ZERO, None)
                .ends_with("the first request of this client")
        );
    }
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Feed recorded bytes cut at every `|`.
    fn events(recorded: &str) -> Vec<ChatEvent> {
        let mut lines = Lines::default();
        let mut deltas = Deltas::default();
        recorded
            .split('|')
            .flat_map(|piece| lines.push(piece.as_bytes()))
            .flat_map(|data| deltas.data(&data).unwrap())
            .collect()
    }

    #[test]
    fn text_split_across_chunks_comes_out_whole() {
        let recorded = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"\"}}]}\r\n\r\n\
            da|ta: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n\
            data: {\"choices\":[{\"del|ta\":{\"content\":\"lo.\"}}]}\n\n\
            : keep-alive comment\n\n\
            data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
            data: [DO|NE]\n\n";
        assert_eq!(
            events(recorded),
            [
                ChatEvent::Text("Hel".into()),
                ChatEvent::Text("lo.".into()),
                ChatEvent::Done
            ]
        );
    }

    #[test]
    fn tool_call_fragments_are_joined_by_index() {
        let recorded = r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"name":"the_time","arguments":""}}]}}]}
|
data: {"choices":[{"delta":{"tool_calls":[{"index":1,"id":"call_b","function":{"name":"remember","arguments":"{\"fa"}}]}}]}

data: {"choices":[{"delta":{"tool_calls":[{"index":1,"function":{"arguments":"ct\": \"likes "}}]}}]}

data: {"choices":[{"delta":{"tool_calls":[{"index":1,"function":{"arguments":"tea\"}"}}]}}]}

data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}

data: [DONE]
"#;
        assert_eq!(
            events(recorded),
            [
                ChatEvent::ToolCalls(vec![
                    ToolCall {
                        id: "call_a".into(),
                        name: "the_time".into(),
                        args: json!({}),
                        extra: None,
                    },
                    ToolCall {
                        id: "call_b".into(),
                        name: "remember".into(),
                        args: json!({"fact": "likes tea"}),
                        extra: None,
                    },
                ]),
                ChatEvent::Done
            ]
        );
    }

    /// A tool the executive offers after the worker started is declared
    /// from the next request on, and one it takes away is not.
    #[test]
    fn the_tools_are_declared_as_they_stand_at_each_request() {
        let offered: Arc<std::sync::Mutex<Vec<lamp_gemini::Tool>>> = Arc::default();
        let now = offered.clone();
        let chat = Chat::with(
            "http://127.0.0.1:9/v1/",
            "m",
            "k",
            Arc::new(move || now.lock().unwrap().clone()),
        )
        .unwrap();
        let asked = [Message::user("Lights on.")];
        assert!(chat.body(&asked).get("tools").is_none());
        *offered.lock().unwrap() = serde_json::from_value(json!([
            {"name": "light_on", "description": "Light on.", "parameters": {"type": "object", "properties": {}}}
        ]))
        .unwrap();
        assert_eq!(
            chat.body(&asked)["tools"][0]["function"]["name"],
            "light_on"
        );
        offered.lock().unwrap().clear();
        assert!(chat.body(&asked).get("tools").is_none());
    }

    /// An end without `[DONE]` is whole after a `finish_reason`, and cut
    /// before one.
    #[test]
    fn a_stream_ended_without_done_is_whole_only_after_a_finish_reason() {
        let mut deltas = Deltas::default();
        deltas
            .data(r#"{"choices":[{"delta":{"content":"It is"},"finish_reason":null}]}"#)
            .unwrap();
        assert_eq!(
            deltas.end().unwrap_err(),
            Error::Protocol("chat stream cut")
        );
        let mut deltas = Deltas::default();
        deltas
            .data(r#"{"choices":[{"delta":{"content":"It is noon."},"finish_reason":null}]}"#)
            .unwrap();
        deltas
            .data(r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#)
            .unwrap();
        assert_eq!(deltas.end().unwrap(), [ChatEvent::Done]);
    }

    /// `LAMP_SECOND_CHAT=gemini`: Google's OpenAI-compatible endpoint, the
    /// key as a bearer token, thinking at its floor, the lamp's tools and a
    /// look's picture as an image part, as the configured chat sends them.
    #[test]
    fn gemini_is_asked_at_its_openai_endpoint_with_minimal_thinking() {
        let tools: Vec<lamp_gemini::Tool> = serde_json::from_value(json!([
            {"name": "light_on", "description": "Light on."}
        ]))
        .unwrap();
        let chat = Chat::gemini(
            "https://generativelanguage.googleapis.com/v1beta/",
            "gemini-3.6-flash",
            "g-key",
            Arc::new(move || tools.clone()),
        )
        .unwrap();
        assert_eq!(
            chat.url,
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
        );
        assert_eq!(chat.auth, "Bearer g-key");
        let body = chat.body(&[Message::user_seeing("What is this?", &[0xff, 0xd8])]);
        assert_eq!(body["model"], "gemini-3.6-flash");
        assert_eq!(body["reasoning_effort"], "minimal");
        assert_eq!(body["stream"], true);
        assert_eq!(body["tools"][0]["function"]["name"], "light_on");
        assert_eq!(body["messages"][0]["content"][1]["type"], "image_url");
        assert_eq!(
            body["messages"][0]["content"][1]["image_url"]["url"],
            "data:image/jpeg;base64,/9g="
        );
    }

    /// `llm_extra` rides on the configured chat's request (an endpoint's own
    /// reasoning setting), and never replaces the request's own fields.
    #[test]
    fn the_configured_chat_carries_its_extra_fields() {
        let mut chat = Chat::with(
            "https://relay.example.invalid/v1",
            "m",
            "k",
            Arc::new(Vec::new),
        )
        .unwrap();
        chat.extra =
            serde_json::from_value(json!({"thinking": {"type": "disabled"}, "model": "not this"}))
                .unwrap();
        let body = chat.body(&[Message::user("Hi.")]);
        assert_eq!(body["thinking"], json!({"type": "disabled"}));
        assert_eq!(body["model"], "m");
    }

    /// Gemini's OpenAI-compatible stream gives each call whole, its thought
    /// signature in `extra_content`; two calls may come at the same index.
    /// Each is its own call, and the signature goes back with it.
    #[test]
    fn whole_calls_keep_their_signatures_and_go_back_with_them() {
        let recorded = r#"data: {"choices":[{"delta":{"role":"assistant","tool_calls":[{"extra_content":{"google":{"thought_signature":"c2ln"}},"function":{"arguments":"{\"level\":30}","name":"set_brightness"},"id":"function-call-1","type":"function"}]},"index":0}]}

data: {"choices":[{"delta":{"tool_calls":[{"function":{"arguments":"{}","name":"the_time"},"id":"function-call-2","type":"function"}]},"index":0,"finish_reason":"tool_calls"}]}

data: [DONE]
"#;
        let got = events(recorded);
        let ChatEvent::ToolCalls(calls) = &got[0] else {
            panic!("{got:?}")
        };
        assert_eq!(calls.len(), 2, "{calls:?}");
        assert_eq!(calls[0].name, "set_brightness");
        assert_eq!(calls[0].args, json!({"level": 30}));
        assert_eq!(
            calls[0].extra,
            Some(json!({"google": {"thought_signature": "c2ln"}}))
        );
        assert_eq!(calls[1].name, "the_time");
        assert_eq!(calls[1].extra, None);
        let back = serde_json::to_value(Message::assistant_calls(None, calls)).unwrap();
        assert_eq!(
            back["tool_calls"][0]["extra_content"],
            json!({"google": {"thought_signature": "c2ln"}})
        );
        assert!(back["tool_calls"][1].get("extra_content").is_none());
    }

    /// `LAMP_SECOND_CHAT=openrouter`: OpenRouter's chat, the pinned model,
    /// its providers tried fastest first, thinking at the effort given, and
    /// its web plugin on Exa only with the search switch; the lamp's tools
    /// and a look's picture as an image part.
    #[test]
    fn openrouter_is_asked_for_the_fastest_provider_and_searches_only_when_switched() {
        let tools: Vec<lamp_gemini::Tool> = serde_json::from_value(json!([
            {"name": "light_on", "description": "Light on."}
        ]))
        .unwrap();
        let declare: Declare = Arc::new(move || tools.clone());
        let plain = Chat::openrouter(
            "deepseek/deepseek-v4.1-flash-1017",
            "none",
            "or-key",
            declare.clone(),
            false,
        )
        .unwrap();
        assert_eq!(plain.url, "https://openrouter.ai/api/v1/chat/completions");
        assert_eq!(plain.auth, "Bearer or-key");
        let body = plain.body(&[Message::user_seeing("What is this?", &[0xff, 0xd8])]);
        assert_eq!(body["model"], "deepseek/deepseek-v4.1-flash-1017");
        assert_eq!(body["provider"], json!({"sort": "latency"}));
        assert_eq!(body["reasoning"], json!({"effort": "none"}));
        assert_eq!(body["tools"][0]["function"]["name"], "light_on");
        assert_eq!(body["messages"][0]["content"][1]["type"], "image_url");
        assert!(body.get("plugins").is_none());
        let searching = Chat::openrouter(
            "google/gemini-3.6-flash",
            "minimal",
            "or-key",
            declare,
            true,
        )
        .unwrap();
        let body = searching.body(&[Message::user("Any news?")]);
        assert_eq!(
            body["plugins"],
            json!([{"id": "web", "engine": "exa", "mode": "instant", "max_results": 3}])
        );
        assert_eq!(body["reasoning"], json!({"effort": "minimal"}));
        assert!(searching.sees());
    }

    /// `LAMP_SECOND_CHAT=deepseek`: DeepSeek's own chat, thinking always set,
    /// off unless the probe asks; its text-only model is never sent a picture.
    #[test]
    fn deepseek_is_asked_with_thinking_set_and_its_text_only_model_sees_nothing() {
        let off = Chat::deepseek("deepseek-flash", "ds-key", Arc::new(Vec::new), false).unwrap();
        assert_eq!(off.url, "https://api.deepseek.com/chat/completions");
        assert_eq!(off.auth, "Bearer ds-key");
        let body = off.body(&[Message::user("Hi.")]);
        assert_eq!(body["model"], "deepseek-flash");
        assert_eq!(body["thinking"], json!({"type": "disabled"}));
        assert!(off.sees());
        let on = Chat::deepseek("deepseek-v4-pro", "ds-key", Arc::new(Vec::new), true).unwrap();
        assert_eq!(on.body(&[])["thinking"], json!({"type": "enabled"}));
        assert!(!on.sees());
    }

    /// OpenRouter's reasoning for a round's calls comes back with them, as
    /// it came; its web search's results are seen once, when they come.
    #[test]
    fn reasoning_goes_back_with_the_calls_and_a_search_is_seen_once() {
        let recorded = r#"data: {"choices":[{"delta":{"annotations":[{"type":"url_citation","url_citation":{"url":"https://a.invalid","title":"A"}}]}}]}

data: {"choices":[{"delta":{"reasoning_details":[{"type":"reasoning.encrypted","data":"c2ln","format":"google-gemini-v1","index":0}]}}]}

data: {"choices":[{"delta":{"annotations":[{"type":"url_citation","url_citation":{"url":"https://b.invalid","title":"B"}}],"tool_calls":[{"index":0,"id":"tool_1","type":"function","function":{"name":"the_time","arguments":"{}"}}]}}]}

data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}

data: [DONE]
"#;
        let got = events(recorded);
        assert_eq!(got[0], ChatEvent::Searched);
        let details = json!([{"type": "reasoning.encrypted", "data": "c2ln", "format": "google-gemini-v1", "index": 0}]);
        assert_eq!(got[1], ChatEvent::Reasoning(details.clone()));
        let ChatEvent::ToolCalls(calls) = &got[2] else {
            panic!("{got:?}")
        };
        assert_eq!(calls[0].name, "the_time");
        assert_eq!(got[3], ChatEvent::Done);
        assert_eq!(got.len(), 4);
        let back = serde_json::to_value(Message::assistant_calls_with(
            None,
            calls,
            Some(details.clone()),
        ))
        .unwrap();
        assert_eq!(back["reasoning_details"], details);
        let plain = serde_json::to_value(Message::assistant_calls(None, calls)).unwrap();
        assert!(plain.get("reasoning_details").is_none());
    }

    /// A call that came without an id is given one, so its result can be
    /// matched to it.
    #[test]
    fn a_call_without_an_id_is_given_one() {
        let mut deltas = Deltas::default();
        deltas
            .data(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"the_time","arguments":"{}"}}]}}]}"#)
            .unwrap();
        let got = deltas.data("[DONE]").unwrap();
        let ChatEvent::ToolCalls(calls) = &got[0] else {
            panic!("{got:?}")
        };
        assert_eq!(calls[0].id, "call_0");
    }

    #[test]
    fn broken_arguments_are_an_error() {
        let mut deltas = Deltas::default();
        deltas
            .data(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c","function":{"name":"x","arguments":"{"}}]}}]}"#)
            .unwrap();
        assert_eq!(
            deltas.data("[DONE]").unwrap_err(),
            Error::Protocol("tool call arguments")
        );
    }

    #[test]
    fn a_follow_up_carries_the_calls_and_their_results() {
        let call = ToolCall {
            id: "call_a".into(),
            name: "the_time".into(),
            args: json!({"zone": "local"}),
            extra: None,
        };
        let messages = [
            Message::assistant_calls(Some(String::new()), std::slice::from_ref(&call)),
            Message::tool("call_a", "10:42"),
        ];
        assert_eq!(
            serde_json::to_value(messages).unwrap(),
            json!([
                {"role": "assistant", "tool_calls": [{"id": "call_a", "type": "function",
                    "function": {"name": "the_time", "arguments": "{\"zone\":\"local\"}"}}]},
                {"role": "tool", "content": "10:42", "tool_call_id": "call_a"},
            ])
        );
    }

    #[test]
    fn tools_are_declared_as_functions() {
        let tools: Vec<lamp_gemini::Tool> = serde_json::from_value(json!([
            {"name": "the_time", "description": "Now."},
            {"name": "remember", "description": "Keep a fact.",
             "parameters": {"type": "object", "properties": {"fact": {"type": "string"}}}},
        ]))
        .unwrap();
        assert_eq!(
            functions(&tools),
            [
                json!({"type": "function", "function": {"name": "the_time", "description": "Now.",
                    "parameters": {"type": "object", "properties": {}}}}),
                json!({"type": "function", "function": {"name": "remember", "description": "Keep a fact.",
                    "parameters": {"type": "object", "properties": {"fact": {"type": "string"}}}}}),
            ]
        );
    }

    /// A local plain HTTP server sends a recorded stream in two pieces.
    #[tokio::test]
    async fn a_streamed_reply_is_read_over_http() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            while !String::from_utf8_lossy(&request).contains("\"stream\":true") {
                let n = socket.read(&mut buffer).await.unwrap();
                request.extend_from_slice(&buffer[..n]);
            }
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n\
                      data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\nda",
                )
                .await
                .unwrap();
            socket.flush().await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            socket
                .write_all(b"ta: {\"choices\":[{\"delta\":{\"content\":\" there.\"}}]}\n\ndata: [DONE]\n\n")
                .await
                .unwrap();
            String::from_utf8(request).unwrap()
        });
        let chat = Chat::with(
            &format!("http://{address}/v1/"),
            "m",
            "secret",
            Arc::new(Vec::new),
        )
        .unwrap();
        let mut stream = chat.send(&[Message::user("hello")]).await.unwrap();
        let mut got = Vec::new();
        while let Some(event) = stream.next().await.unwrap() {
            got.push(event);
        }
        assert_eq!(
            got,
            [
                ChatEvent::Text("Hi".into()),
                ChatEvent::Text(" there.".into()),
                ChatEvent::Done
            ]
        );
        let request = server.await.unwrap().to_lowercase();
        assert!(request.starts_with("post /v1/chat/completions http/1.1"));
        assert!(request.contains("authorization: bearer secret"));
        assert!(!request.contains("\"tools\""));
    }
}
