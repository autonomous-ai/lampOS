//! A streamed OpenAI-compatible chat completion with function tools: text
//! as it is written, then the tool calls the model asked for. The caller
//! runs the tools and sends their results back as `tool` messages.
use super::{Error, Result, chat_client, chunk, http_client, openai_base, post};
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
    /// Its words: the text, or a look's text part.
    pub fn words(&self) -> Option<&str> {
        match self.content.as_ref()? {
            Value::String(text) => Some(text),
            Value::Array(parts) => parts
                .iter()
                .find_map(|part| part.get("text").and_then(Value::as_str)),
            _ => None,
        }
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

/// Of the tools declared now, those a request with these messages is given
/// ([`Chat::narrowing`]): with `LAMP_SECOND_FEW_TOOLS=on`, a spoken turn's
/// few until the model asks for the rest (#1712, the first builder at
/// 18:56).
pub type Narrow =
    Arc<dyn Fn(Vec<lamp_gemini::Tool>, &[Message]) -> Vec<lamp_gemini::Tool> + Send + Sync>;

/// The function a request with these messages must call, if any
/// ([`Chat::forcing`]): sent as the request's `tool_choice`.
pub type Force = Arc<dyn Fn(&[Message]) -> Option<Forced> + Send + Sync>;

/// A function a request must call ([`Force`]), and the one value one of
/// its string parameters may take, if any: that parameter is then told
/// only that value and is required (`LAMP_TASKS_NAMED_ASK`: `who` is the
/// agent the words name, #1722). A function without that parameter is
/// told as declared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Forced {
    pub name: &'static str,
    pub only: Option<(&'static str, &'static str)>,
}

/// `tool`'s parameters with `parameter` told only `value`, and required,
/// when it has that parameter.
fn pinned(tool: &mut lamp_gemini::Tool, (parameter, value): (&str, &str)) {
    let Some(parameters) = tool.parameters.as_mut() else {
        return;
    };
    let Some(property) = parameters
        .get_mut("properties")
        .and_then(|properties| properties.get_mut(parameter))
    else {
        return;
    };
    property["enum"] = json!([value]);
    match parameters.get_mut("required").and_then(Value::as_array_mut) {
        Some(required) => {
            if !required.iter().any(|name| name == parameter) {
                required.push(json!(parameter));
            }
        }
        None => parameters["required"] = json!([parameter]),
    }
}

/// How much Gemini thinks before it answers through its OpenAI-compatible
/// endpoint: the floor, as thinking cannot be turned off on Gemini 3 (the
/// research of 19 Oct; 0.80 to 0.83 s to the first token, published).
pub const GEMINI_REASONING: &str = "minimal";

/// A chat connection idle this long is opened ahead as a line begins
/// ([`Chat::warm`]): the client's pool keeps an idle one 90 s (25 s with
/// `LAMP_SECOND_CHAT_KEPT=on`, when a line after 25 s opens a new one
/// anyway), and the service may close it sooner.
pub const WARM_AFTER: std::time::Duration = std::time::Duration::from_secs(60);
/// [`WARM_AFTER`] for a client that keeps an idle connection only
/// `CHAT_POOL_IDLE` (`LAMP_SECOND_CHAT_KEPT=on`): a little under it, so a line
/// after its connection went still finds one opened ahead.
pub const WARM_AFTER_KEPT: std::time::Duration = std::time::Duration::from_secs(20);
/// At most this much of the warming answer is read: enough to finish a
/// models list, so its connection goes back to the pool.
const WARM_BYTES: usize = 256 << 10;

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
    /// Which of the declared tools a request is given, if not all.
    narrow: std::sync::OnceLock<Narrow>,
    /// Which function a request must call, if any.
    force: std::sync::OnceLock<Force>,
    /// When the last request went, for the journal: a connection idle past
    /// the client's pool keep (`chat_client`: 90 s, or 25 s with
    /// `LAMP_SECOND_CHAT_KEPT=on`) is made anew.
    last_sent: std::sync::Mutex<Option<std::time::Instant>>,
    /// The last request's tools and first message, hashed, for the
    /// journal: a service's prompt cache holds only a prefix that is the
    /// same as before ([`Prefix`]).
    last_prefix: std::sync::Mutex<Option<Prefix>>,
    /// Idle this long, a line's warm opens the connection ahead:
    /// [`WARM_AFTER`], or [`WARM_AFTER_KEPT`] with a kept client.
    warm_after: std::time::Duration,
}

/// A request's tools and first message, hashed: what has to stay the same
/// from one request to the next for a service's prompt cache to hold
/// (#1712, the first builder at 18:56).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Prefix {
    tools: u64,
    first: u64,
}

impl Prefix {
    fn of(body: &Value) -> Self {
        let hash = |value: &Value| {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            value.to_string().hash(&mut hasher);
            hasher.finish()
        };
        Self {
            tools: hash(&body["tools"]),
            first: hash(&body["messages"][0]),
        }
    }
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
        // The answer's last chunk says how much of the prompt its cache held
        // (`prompt_cache_hit_tokens`, #1712, the first builder at 18:56).
        chat.extra
            .insert("stream_options".into(), json!({"include_usage": true}));
        Ok(chat)
    }

    /// Whether the model is sent a look's picture.
    pub fn sees(&self) -> bool {
        self.sees
    }

    /// The connection opened ahead, as a line begins, when no request went
    /// in the last [`WARM_AFTER`], or [`WARM_AFTER_KEPT`] with a kept client
    /// (`LAMP_SECOND_CHAT_WARM=on`, #1712): a GET
    /// of the service's `models`, read whole, so the line's chat goes on a
    /// connection already made. Its status and how long it took; `None`
    /// when not due, or when it failed (the chat then connects as before).
    pub async fn warm(&self) -> Option<(u16, std::time::Duration)> {
        let models = self.url.replacen("/chat/completions", "/models", 1);
        let (status, took, ..) = super::warm_get(
            &self.client,
            &self.last_sent,
            self.warm_after,
            &models,
            ("authorization", &self.auth),
            WARM_BYTES,
        )
        .await?;
        Some((status, took))
    }

    /// Its requests are given only the tools `narrow` keeps of those
    /// declared, for their messages. Set once: a second call changes
    /// nothing.
    pub fn narrowing(&self, narrow: Narrow) {
        let _ = self.narrow.set(narrow);
    }

    /// A request whose messages `force` names a function for, when that
    /// function is among those it is given, must call it (`tool_choice`,
    /// the OpenAI form, which DeepSeek takes; `LAMP_TASKS_NAMED_ASK`,
    /// #1722), with the one value it names for a parameter, if it names
    /// one ([`Forced::only`]). Never with `only`. Set once: a second call
    /// changes nothing.
    pub fn forcing(&self, force: Force) {
        let _ = self.force.set(force);
    }

    fn with(base_url: &str, model: &str, key: &str, declare: Declare) -> Result<Self> {
        let (client, kept) = chat_client()?;
        Ok(Self {
            client,
            warm_after: if kept { WARM_AFTER_KEPT } else { WARM_AFTER },
            url: format!("{}/chat/completions", openai_base(base_url)),
            auth: format!("Bearer {key}"),
            model: model.to_owned(),
            extra: serde_json::Map::new(),
            sees: true,
            declare,
            narrow: std::sync::OnceLock::new(),
            force: std::sync::OnceLock::new(),
            last_sent: std::sync::Mutex::new(None),
            last_prefix: std::sync::Mutex::new(None),
        })
    }

    /// The request's body, with the tools declared now, as narrowed for
    /// its messages.
    #[cfg(test)]
    fn body(&self, messages: &[Message]) -> Value {
        self.body_only(messages, None)
    }

    /// [`Self::body`], with only the tools named in `only` when given, and
    /// then never narrowed: a held line's round is told those alone.
    fn body_only(&self, messages: &[Message], only: Option<&[&str]>) -> Value {
        self.body_without(messages, only, &[])
    }

    /// [`Self::body_only`], never declaring the tools named in `without`.
    fn body_without(&self, messages: &[Message], only: Option<&[&str]>, without: &[&str]) -> Value {
        let mut body = json!({"model": self.model, "stream": true, "messages": messages});
        let mut declared = self.declared(messages, only, without);
        let forced = only
            .is_none()
            .then(|| self.force.get().and_then(|force| force(messages)))
            .flatten()
            .filter(|forced| declared.iter().any(|tool| tool.name == forced.name));
        if let Some(Forced {
            name,
            only: Some(only),
        }) = forced
            && let Some(tool) = declared.iter_mut().find(|tool| tool.name == name)
        {
            pinned(tool, only);
        }
        let tools = functions(&declared);
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools);
        }
        if let Some(Forced { name, .. }) = forced {
            body["tool_choice"] = json!({"type": "function", "function": {"name": name}});
        }
        // The configuration refuses any of the request's own fields here.
        for (field, value) in &self.extra {
            if body.get(field).is_none() {
                body[field] = value.clone();
            }
        }
        body
    }

    /// The tools a request on `messages` is given: those declared now but
    /// `without`, then only those in `only`, or else as narrowed.
    fn declared(
        &self,
        messages: &[Message],
        only: Option<&[&str]>,
        without: &[&str],
    ) -> Vec<lamp_gemini::Tool> {
        let mut declared = (self.declare)();
        declared.retain(|tool| !without.contains(&tool.name.as_str()));
        if let Some(only) = only {
            declared.retain(|tool| only.contains(&tool.name.as_str()));
        } else if let Some(narrow) = self.narrow.get() {
            declared = narrow(declared, messages);
        }
        declared
    }

    /// Whether a request on `messages`, never declaring `without`, is given
    /// the function `name` (`LAMP_MEMORY_REPLACES`'s turn line, #2510).
    pub fn declares(&self, messages: &[Message], without: &[&str], name: &str) -> bool {
        self.declared(messages, None, without)
            .iter()
            .any(|tool| tool.name == name)
    }

    /// The request as [`Self::send`] makes it, for a probe that makes it
    /// by hand (`probe-cascade --wire`): its URL, its authorization and
    /// its body.
    pub(crate) fn wire(&self, messages: &[Message]) -> (&str, &str, Value) {
        (&self.url, &self.auth, self.body_only(messages, None))
    }

    pub async fn send(&self, messages: &[Message]) -> Result<ChatStream> {
        self.send_only(messages, None).await
    }

    /// [`Self::send`], told only the tools named in `only`, when given.
    pub async fn send_only(
        &self,
        messages: &[Message],
        only: Option<&[&str]>,
    ) -> Result<ChatStream> {
        self.send_without(messages, only, &[]).await
    }

    /// [`Self::send_only`], never declaring the tools named in `without`.
    pub async fn send_without(
        &self,
        messages: &[Message],
        only: Option<&[&str]>,
        without: &[&str],
    ) -> Result<ChatStream> {
        self.send_on(&self.client, false, messages, only, without)
            .await
    }

    /// [`Self::send_without`] on a connection of its own, never one from
    /// the client's pool (`LAMP_SECOND_CHAT_AGAIN`, #2602): a round that
    /// hasn't begun may be stuck on a pooled connection the service has
    /// dropped, and asking again there would wait on it too.
    pub async fn send_fresh(
        &self,
        messages: &[Message],
        only: Option<&[&str]>,
        without: &[&str],
    ) -> Result<ChatStream> {
        let client = http_client()?;
        self.send_on(&client, true, messages, only, without).await
    }

    async fn send_on(
        &self,
        client: &reqwest::Client,
        fresh: bool,
        messages: &[Message],
        only: Option<&[&str]>,
        without: &[&str],
    ) -> Result<ChatStream> {
        let body = self.body_without(messages, only, without);
        let asked = std::time::Instant::now();
        let before = self
            .last_sent
            .lock()
            .ok()
            .and_then(|mut last| last.replace(asked));
        // As it's attempted, before any answer (#2602, Backend's 02:09: three
        // round 1s that never began never reached the relay). It proves an
        // attempt only, not that anything was sent: with no answer's line
        // after it, the attempt failed or hung somewhere (a connection, its
        // TLS, a reused connection, the service); with no line at all, the
        // request was never attempted (Second's and reviewer-two's P3s).
        eprintln!(
            "lamp-voice second: {}",
            going_out_told(fresh, before.map(|at| asked.duration_since(at)))
        );
        let prefix = Prefix::of(&body);
        let prefix_before = self
            .last_prefix
            .lock()
            .ok()
            .and_then(|mut last| last.replace(prefix));
        let response = post(client, &self.url, ("authorization", &self.auth), &body).await?;
        // What was asked and how soon the service began to answer, without
        // words (#1712, the first builder at 18:26: the chat's first event
        // came in 0.9 to 1.9 s on lamp-4ace, 0.7 s from the probe).
        eprintln!(
            "lamp-voice second: {}",
            asked_told(
                &body,
                asked.elapsed(),
                before.map(|at| asked.duration_since(at))
            ) + &prefix_told(prefix, prefix_before)
        );
        Ok(ChatStream {
            response,
            lines: Lines::default(),
            deltas: Deltas::default(),
            ready: Vec::new(),
            done: false,
            asked,
            first: None,
        })
    }
}

/// A chat request as it goes out, in the journal's words: on the client's
/// pool or a connection of its own, and how long before it the last request
/// went (a pooled connection idle long may be one the service has dropped).
fn going_out_told(fresh: bool, since_last: Option<std::time::Duration>) -> String {
    format!(
        "chat request going out on {}; {}",
        if fresh {
            "a connection of its own"
        } else {
            "the client's pool"
        },
        since_last.map_or("the first request of this client".to_owned(), |since| {
            format!("the last request {} s before", since.as_secs())
        })
    )
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

/// Whether a request's tools and first message are the last request's, for
/// its journal line; nothing for a client's first request.
fn prefix_told(now: Prefix, before: Option<Prefix>) -> String {
    let Some(before) = before else {
        return String::new();
    };
    match (now.tools == before.tools, now.first == before.first) {
        (true, true) => "; its tools and first message as the last request's",
        (true, false) => "; its tools as the last request's, its first message not",
        (false, true) => "; its first message as the last request's, its tools not",
        (false, false) => "; neither its tools nor its first message as the last request's",
    }
    .to_owned()
}

/// A `usage`'s prompt tokens and those the service's cache held:
/// DeepSeek's `prompt_cache_hit_tokens`, or OpenAI's form,
/// `prompt_tokens_details.cached_tokens`.
pub fn cache_counts(usage: Option<&Value>) -> (Option<u64>, Option<u64>) {
    let count = |pointer: &str| usage?.pointer(pointer)?.as_u64();
    (
        count("/prompt_tokens"),
        count("/prompt_cache_hit_tokens").or_else(|| count("/prompt_tokens_details/cached_tokens")),
    )
}

/// An answer in the journal's words, when it has ended (#1712, the first
/// builder at 18:56): when its first words or call came and when it ended,
/// after the request went, and the service's count of its tokens, with how
/// many of the prompt's its cache held. DeepSeek says that as
/// `prompt_cache_hit_tokens`, OpenAI's form as
/// `prompt_tokens_details.cached_tokens`. Never its words.
fn answer_told(
    usage: Option<&Value>,
    first: Option<std::time::Duration>,
    ended: std::time::Duration,
) -> String {
    let first = first.map_or("no words or call".to_owned(), |first| {
        format!("its first words or call {} ms", first.as_millis())
    });
    let tokens = match usage {
        None => "no usage given".to_owned(),
        Some(usage) => {
            let count = |pointer: &str| usage.pointer(pointer).and_then(Value::as_u64);
            let (prompt, cached) = cache_counts(Some(usage));
            let missed =
                count("/prompt_cache_miss_tokens").or_else(|| prompt?.checked_sub(cached?));
            let said = |count: Option<u64>| count.map_or("?".to_owned(), |n| n.to_string());
            match cached {
                Some(cached) => format!(
                    "{} prompt tokens, {cached} from the cache, {} not; {} completion tokens",
                    said(prompt),
                    said(missed),
                    said(count("/completion_tokens"))
                ),
                None => format!(
                    "{} prompt tokens, the cache not said; {} completion tokens",
                    said(prompt),
                    said(count("/completion_tokens"))
                ),
            }
        }
    };
    format!(
        "chat answer: {first} and its end {} ms after it was asked; {tokens}",
        ended.as_millis()
    )
}

pub struct ChatStream {
    response: reqwest::Response,
    lines: Lines,
    deltas: Deltas,
    /// Decoded events not yet handed out, newest last.
    ready: Vec<ChatEvent>,
    done: bool,
    /// When the request went, and how long after it the first words or
    /// call came, for [`answer_told`].
    asked: std::time::Instant,
    first: Option<std::time::Duration>,
}

impl ChatStream {
    /// The service's count of the answer's tokens, once the chunk that
    /// carries it has come (the last, with DeepSeek's `include_usage`).
    pub fn usage(&self) -> Option<&Value> {
        self.deltas.usage.as_ref()
    }

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
            if self.first.is_none()
                && self
                    .ready
                    .iter()
                    .any(|event| matches!(event, ChatEvent::Text(_) | ChatEvent::ToolCalls(_)))
            {
                self.first = Some(self.asked.elapsed());
            }
            if self.ready.last() == Some(&ChatEvent::Done) {
                self.done = true;
                eprintln!(
                    "lamp-voice second: {}",
                    answer_told(self.deltas.usage.as_ref(), self.first, self.asked.elapsed())
                );
            }
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
    /// The service's count of the answer's tokens (`usage`), from the chunk
    /// that carried it: the last, with DeepSeek's `include_usage`.
    usage: Option<Value>,
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
        if let Some(usage) = chunk.get("usage").filter(|usage| usage.is_object()) {
            self.usage = Some(usage.clone());
        }
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

    /// #1712: as a line begins, a chat idle past `WARM_AFTER` opens its
    /// connection with a GET of `models`; one used lately, or warmed just
    /// now, is not asked again.
    #[tokio::test]
    async fn an_idle_chat_opens_its_connection_ahead_and_a_busy_one_does_not() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let asked = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = asked.clone();
        let server = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut read = vec![0; 4096];
            let n = socket.read(&mut read).await.unwrap();
            let head = String::from_utf8_lossy(&read[..n]).to_string();
            seen.lock()
                .unwrap()
                .push(head.lines().next().unwrap_or_default().to_owned());
            assert!(head.contains("authorization: Bearer k"), "{head}");
            let body = r#"{"data":[]}"#;
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let chat = Chat::with(
            &format!("http://{address}/v1"),
            "m",
            "k",
            Arc::new(Vec::new),
        )
        .unwrap();
        let (status, _) = chat.warm().await.expect("warmed");
        assert_eq!(status, 200);
        assert_eq!(*asked.lock().unwrap(), ["GET /v1/models HTTP/1.1"]);
        // Just warmed: not asked again.
        assert!(chat.warm().await.is_none());
        server.await.unwrap();
    }

    /// #2602: a chat request is journaled as it goes out, before any answer,
    /// with where it goes and the gap since the last; never its words.
    #[test]
    fn a_chat_request_going_out_is_journaled() {
        assert_eq!(
            going_out_told(false, Some(std::time::Duration::from_secs(95))),
            "chat request going out on the client's pool; the last request 95 s before"
        );
        assert_eq!(
            going_out_told(true, None),
            "chat request going out on a connection of its own; the first request of this client"
        );
    }

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

    /// #1712, the first builder at 18:56: a request's line says whether its
    /// tools and first message are the last request's, which a prompt
    /// cache needs; never its words.
    #[test]
    fn a_requests_line_says_whether_its_tools_and_first_message_are_the_last_ones() {
        let body = |first: &str, tool: &str| {
            json!({
                "messages": [{"role": "system", "content": first}, {"role": "user", "content": "secret words"}],
                "tools": [{"type": "function", "function": {"name": tool}}],
            })
        };
        let before = Prefix::of(&body("You are Lamp.", "light_on"));
        assert_eq!(prefix_told(before, None), "");
        assert_eq!(
            prefix_told(Prefix::of(&body("You are Lamp.", "light_on")), Some(before)),
            "; its tools and first message as the last request's"
        );
        assert_eq!(
            prefix_told(Prefix::of(&body("It is noon.", "light_on")), Some(before)),
            "; its tools as the last request's, its first message not"
        );
        assert_eq!(
            prefix_told(
                Prefix::of(&body("You are Lamp.", "light_off")),
                Some(before)
            ),
            "; its first message as the last request's, its tools not"
        );
        assert_eq!(
            prefix_told(Prefix::of(&body("It is noon.", "light_off")), Some(before)),
            "; neither its tools nor its first message as the last request's"
        );
        // Another user message changes neither.
        let mut later = body("You are Lamp.", "light_on");
        later["messages"][1]["content"] = json!("other words");
        assert_eq!(Prefix::of(&later), before);
    }

    /// #1712, the first builder at 18:56: the answer's usage is kept from
    /// the chunk that carries it, which has no choices, and told with the
    /// cache's part, as DeepSeek or OpenAI's form says it.
    #[test]
    fn an_answers_usage_is_kept_and_told_with_the_caches_part() {
        let mut deltas = Deltas::default();
        let mut events = deltas
            .data(r#"{"choices":[{"delta":{"content":"Hi."},"finish_reason":"stop"}]}"#)
            .unwrap();
        events.extend(
            deltas
                .data(r#"{"choices":[],"usage":{"prompt_tokens":8123,"completion_tokens":42,"prompt_cache_hit_tokens":7936,"prompt_cache_miss_tokens":187}}"#)
                .unwrap(),
        );
        events.extend(deltas.data("[DONE]").unwrap());
        assert_eq!(events, [ChatEvent::Text("Hi.".into()), ChatEvent::Done]);
        let ms = std::time::Duration::from_millis;
        assert_eq!(
            answer_told(deltas.usage.as_ref(), Some(ms(812)), ms(1430)),
            "chat answer: its first words or call 812 ms and its end 1430 ms after it was asked; 8123 prompt tokens, 7936 from the cache, 187 not; 42 completion tokens"
        );
        let openai = json!({"prompt_tokens": 900, "completion_tokens": 5, "prompt_tokens_details": {"cached_tokens": 768}});
        assert!(
            answer_told(Some(&openai), Some(ms(5)), ms(9))
                .ends_with("; 900 prompt tokens, 768 from the cache, 132 not; 5 completion tokens")
        );
        assert!(
            answer_told(Some(&json!({"prompt_tokens": 900})), None, ms(9)).ends_with(
                "no words or call and its end 9 ms after it was asked; 900 prompt tokens, the cache not said; ? completion tokens"
            )
        );
        assert!(answer_told(None, None, ms(9)).ends_with("; no usage given"));
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

    /// A held line's round is told only the functions done though its
    /// answer is held, never narrowed: none, when the
    /// lamp has none of them.
    #[test]
    fn a_round_told_only_some_functions_declares_those_alone() {
        let offered: Vec<lamp_gemini::Tool> = serde_json::from_value(json!([
            {"name": "dance", "description": "Dance."},
            {"name": "reports_stop", "description": "Stop the reports."}
        ]))
        .unwrap();
        let chat = Chat::with(
            "http://127.0.0.1:9/v1/",
            "m",
            "k",
            Arc::new(move || offered.clone()),
        )
        .unwrap();
        // A narrow that keeps nothing: the held round is not narrowed.
        chat.narrowing(Arc::new(|_, _| Vec::new()));
        let asked = [Message::user("Stop sending reports.")];
        let names = |body: Value| -> Vec<String> {
            body["tools"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|tool| tool["function"]["name"].as_str().unwrap().to_owned())
                .collect()
        };
        assert!(names(chat.body(&asked)).is_empty());
        assert_eq!(
            names(chat.body_only(&asked, Some(&["reports_stop"]))),
            ["reports_stop"]
        );
        assert!(
            chat.body_only(&asked, Some(&["remember"]))
                .get("tools")
                .is_none()
        );
    }

    /// #2265's follow-up: a line held only by a known face's gap is never
    /// told memory's read, whatever else it is told.
    #[test]
    fn a_body_without_leaves_out_only_the_named_functions() {
        let offered: Vec<lamp_gemini::Tool> = serde_json::from_value(json!([
            {"name": "light_on", "description": "Light."},
            {"name": "recall", "description": "Recall."},
            {"name": "remember", "description": "Remember."}
        ]))
        .unwrap();
        let chat = Chat::with(
            "http://127.0.0.1:9/v1/",
            "m",
            "k",
            Arc::new(move || offered.clone()),
        )
        .unwrap();
        let names = |body: Value| -> Vec<String> {
            body["tools"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|tool| tool["function"]["name"].as_str().unwrap().to_owned())
                .collect()
        };
        let asked = [Message::user("And what else?")];
        assert_eq!(
            names(chat.body_only(&asked, None)),
            ["light_on", "recall", "remember"]
        );
        assert_eq!(
            names(chat.body_without(&asked, None, &["recall"])),
            ["light_on", "remember"]
        );
    }

    /// #1712, the first builder at 18:56: a narrowed chat's request is
    /// given the tools its narrow keeps for that request's messages.
    #[test]
    fn a_narrowed_chat_is_given_what_its_narrow_keeps_for_the_messages() {
        let tool = |name: &str| lamp_gemini::Tool {
            name: name.into(),
            description: "A tool.".into(),
            parameters: None,
            silent: false,
        };
        let chat = Chat::with(
            "http://127.0.0.1:9/v1/",
            "m",
            "k",
            Arc::new(move || vec![tool("light_on"), tool("remember")]),
        )
        .unwrap();
        let names = |body: Value| -> Vec<String> {
            body["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|tool| tool["function"]["name"].as_str().unwrap().to_owned())
                .collect()
        };
        let one = [Message::user("Lights on.")];
        let two = [Message::user("Lights on."), Message::assistant("On.")];
        assert_eq!(names(chat.body(&one)), ["light_on", "remember"]);
        chat.narrowing(Arc::new(|declared, messages| {
            declared.into_iter().take(messages.len()).collect()
        }));
        assert_eq!(names(chat.body(&one)), ["light_on"]);
        assert_eq!(names(chat.body(&two)), ["light_on", "remember"]);
        // What a turn's journal line says was given (#2510): as narrowed,
        // and never what is left out.
        assert!(!chat.declares(&one, &[], "remember"));
        assert!(chat.declares(&two, &[], "remember"));
        assert!(!chat.declares(&two, &["remember"], "remember"));
        // Set once.
        chat.narrowing(Arc::new(|_, _| Vec::new()));
        assert_eq!(names(chat.body(&one)), ["light_on"]);
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
        assert_eq!(body["stream_options"], json!({"include_usage": true}));
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

    /// Header-only GET fixture, using the same framing boundary as the TTS
    /// mocks. TCP reads can split one request or contain several requests.
    async fn kept_fixture_connection(
        mut socket: tokio::net::TcpStream,
        reply_after: std::time::Duration,
        processed: Option<Arc<tokio::sync::Notify>>,
    ) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut pending = Vec::new();
        let mut read = [0; 4096];
        loop {
            let n = match socket.read(&mut read).await {
                Ok(0) | Err(_) => return,
                Ok(n) => n,
            };
            pending.extend_from_slice(&read[..n]);
            while let Some(end) = pending.windows(4).position(|w| w == b"\r\n\r\n") {
                if end + 4 > 4096 {
                    return;
                }
                pending.drain(..end + 4);
                if !reply_after.is_zero() {
                    tokio::time::sleep(reply_after).await;
                }
                if socket
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\n{}")
                    .await
                    .is_err()
                {
                    return;
                }
            }
            if pending.len() > 4096 {
                return;
            }
            if let Some(processed) = processed.as_ref() {
                processed.notify_one();
            }
        }
    }

    #[tokio::test]
    async fn the_kept_fixture_answers_a_split_header_once_after_it_is_complete() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let processed = Arc::new(tokio::sync::Notify::new());
        let server_processed = processed.clone();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            kept_fixture_connection(socket, std::time::Duration::ZERO, Some(server_processed))
                .await;
        });
        let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
        client
            .write_all(b"GET /v1/models HTTP/1.1\r\nHost: fixture\r\n")
            .await
            .unwrap();
        // Observe a completed server read before sending the final separator,
        // rather than hoping that two client writes became two TCP reads.
        tokio::time::timeout(std::time::Duration::from_secs(2), processed.notified())
            .await
            .expect("the fixture did not read the incomplete header");
        client.write_all(b"\r\n").await.unwrap();
        client.shutdown().await.unwrap();
        let mut replies = Vec::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            client.read_to_end(&mut replies),
        )
        .await
        .expect("the fixture did not finish after the write half closed")
        .unwrap();
        server.await.unwrap();
        assert_eq!(replies, b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\n{}");
    }

    /// #2602 (`LAMP_SECOND_CHAT_KEPT`): a kept client lets an idle
    /// connection go after its idle limit, so the next request opens a new
    /// one; the default client reuses it.
    #[tokio::test]
    async fn a_kept_client_lets_an_idle_connection_go_and_the_default_reuses_it() {
        async fn connections(kept: Option<std::time::Duration>) -> usize {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let accepted = count.clone();
            tokio::spawn(async move {
                loop {
                    let (socket, _) = listener.accept().await.unwrap();
                    accepted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    tokio::spawn(kept_fixture_connection(
                        socket,
                        std::time::Duration::ZERO,
                        None,
                    ));
                }
            });
            let client = super::super::client_kept(kept).unwrap();
            for _ in 0..2 {
                let answer = client
                    .get(format!("http://{address}/v1/models"))
                    .send()
                    .await
                    .unwrap();
                answer.bytes().await.unwrap();
                tokio::time::sleep(std::time::Duration::from_millis(600)).await;
            }
            count.load(std::sync::atomic::Ordering::SeqCst)
        }
        assert_eq!(
            connections(Some(std::time::Duration::from_millis(200))).await,
            2
        );
        assert_eq!(connections(None).await, 1);
        // The switch reads `on` alone, and the warm comes before the pool
        // lets go (reviewer-two's P3 on #3115).
        assert!(super::super::chat_kept(Some("on")));
        for other in [None, Some("off"), Some("ON"), Some("1"), Some("")] {
            assert!(!super::super::chat_kept(other), "{other:?}");
        }
        assert!(WARM_AFTER_KEPT < crate::second_provider::net::CHAT_POOL_IDLE);
    }

    /// El's P3 on #3115: a kept client keeps at most one idle connection
    /// per host. Two requests at once open two; two more at once, inside
    /// the idle limit, reuse the one kept and open one new (3 in all), where
    /// the default keeps both and reuses them (2 in all).
    #[tokio::test]
    async fn a_kept_client_keeps_one_idle_connection_per_host() {
        async fn connections(idle: Option<std::time::Duration>) -> usize {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let accepted = count.clone();
            tokio::spawn(async move {
                loop {
                    let (socket, _) = listener.accept().await.unwrap();
                    accepted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    // Slow enough that two asks overlap, as before.
                    tokio::spawn(kept_fixture_connection(
                        socket,
                        std::time::Duration::from_millis(50),
                        None,
                    ));
                }
            });
            let client = super::super::client_kept(idle).unwrap();
            let url = format!("http://{address}/v1/models");
            for _ in 0..2 {
                let (one, two) = tokio::join!(client.get(&url).send(), client.get(&url).send());
                one.unwrap().bytes().await.unwrap();
                two.unwrap().bytes().await.unwrap();
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            count.load(std::sync::atomic::Ordering::SeqCst)
        }
        assert_eq!(
            connections(Some(std::time::Duration::from_secs(10))).await,
            3
        );
        assert_eq!(connections(None).await, 2);
    }

    /// #2602 (`LAMP_SECOND_CHAT_AGAIN`): a relay that keeps a pooled
    /// connection open but stops answering on it. `send` reuses that
    /// connection and waits; `send_fresh` opens a connection of its own,
    /// which the relay answers, so a stalled round asked again begins.
    #[tokio::test]
    async fn a_fresh_send_skips_a_pooled_connection_that_stopped_answering() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let connections = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let accepted = connections.clone();
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let number = accepted.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                tokio::spawn(async move {
                    let mut buffer = [0; 8192];
                    let mut answered = 0;
                    loop {
                        let Ok(n) = socket.read(&mut buffer).await else {
                            return;
                        };
                        if n == 0 {
                            return;
                        }
                        let head = String::from_utf8_lossy(&buffer[..n]).to_string();
                        // The first connection answers its warming GET, kept
                        // alive, and then nothing more: the stall.
                        if number == 1 && answered > 0 {
                            continue;
                        }
                        answered += 1;
                        let reply = if head.starts_with("GET") {
                            let body = r#"{"data":[]}"#;
                            format!(
                                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
                                body.len()
                            )
                        } else {
                            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n\
                             data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\ndata: [DONE]\n\n"
                                .to_owned()
                        };
                        if socket.write_all(reply.as_bytes()).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        let chat = Chat::with(
            &format!("http://{address}/v1"),
            "m",
            "k",
            Arc::new(Vec::new),
        )
        .unwrap();
        // A connection in the pool, as a warmed or recent chat leaves one.
        chat.warm().await.expect("warmed");
        assert_eq!(connections.load(std::sync::atomic::Ordering::SeqCst), 1);
        let asked = [Message::user("hello")];
        let pooled =
            tokio::time::timeout(std::time::Duration::from_millis(500), chat.send(&asked)).await;
        assert!(
            pooled.is_err(),
            "the pooled connection was reused and stalled"
        );
        assert_eq!(connections.load(std::sync::atomic::Ordering::SeqCst), 1);
        let mut fresh = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            chat.send_fresh(&asked, None, &[]),
        )
        .await
        .expect("a fresh connection answers")
        .unwrap();
        assert_eq!(connections.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(
            fresh.next().await.unwrap(),
            Some(ChatEvent::Text("Hi".into()))
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
