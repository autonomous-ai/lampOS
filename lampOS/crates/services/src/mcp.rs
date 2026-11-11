//! V1's MCP connectors (Slack, monday.com, HubSpot, Amplitude) and Notion:
//! a small MCP client over Streamable HTTP. JSON-RPC 2.0 posted to the
//! service's one endpoint: `initialize`, `notifications/initialized`, then
//! `tools/list` and `tools/call`, the `Mcp-Session-Id` the first answer gives kept for
//! the rest of that one sequence. An answer is `application/json` or a
//! `text/event-stream` holding the reply.
//!
//! The token goes only to the service's pinned MCP host (`catalog.rs`),
//! over https, with no redirect followed: a link whose `mcp_url` names
//! another host is refused when it is set and again before each call.
//!
//! A tool reads only if its name has none of [`WRITE_WORDS`] and either it
//! says `readOnlyHint` (and not `destructiveHint`) or its name starts with
//! one of its service's read verbs ([`READ_VERBS`]). Every other tool is a
//! write, made only through `service_write`'s two phases.
use crate::catalog::{self, Method, Service};
use crate::mail;
use crate::net::{self, Call, Http, Reply};
use crate::reads;
use crate::store::Entry;
use serde_json::{Map, Value, json};
use std::{
    fmt,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

/// The MCP revision the lamp asks for.
pub const PROTOCOL_VERSION: &str = "2025-06-18";
/// How long one whole sequence (handshake, list, call) may take.
pub const WITHIN: Duration = Duration::from_secs(30);
/// The most tools read from one service.
pub const TOOLS_MOST: usize = 200;
/// The most `tools/list` pages followed.
pub const PAGES_MOST: usize = 5;
/// The longest tool name taken.
pub const NAME_MOST: usize = 128;
/// The longest arguments taken, as JSON text.
pub const ARGUMENTS_MOST: usize = 4_000;
/// The most pieces a tool's answer is cut into before it is quoted.
pub const PIECES_MOST: usize = 100;
/// The size of one piece of a tool's text answer, in characters.
pub const PIECE_MOST: usize = 500;
/// The longest session id kept.
pub const SESSION_MOST: usize = 256;

/// Words that make a tool a write wherever they stand in its name, its
/// `readOnlyHint` notwithstanding.
pub const WRITE_WORDS: &[&str] = &[
    "create",
    "update",
    "delete",
    "send",
    "post",
    "write",
    "remove",
    "add",
    "set",
    "archive",
    "invite",
    "upload",
    "edit",
    "move",
    "assign",
    "publish",
    "share",
    "reply",
    "react",
    "patch",
    "put",
    "insert",
    "replace",
    "schedule",
    "change",
    "modify",
    "duplicate",
    "mark",
    "merge",
    "close",
    "cancel",
    "rename",
    "submit",
    "approve",
    "import",
    "execute",
    "trigger",
    "join",
    "leave",
    "kick",
    "pin",
    "unpin",
];

/// The verbs a read tool's name may start with.
const READS: &[&str] = &[
    "get", "list", "search", "read", "query", "fetch", "find", "describe",
];

/// Each MCP service: the words its tool names may start with that name
/// the service itself (skipped), and its read verbs.
pub const READ_VERBS: &[(&str, &[&str], &[&str])] = &[
    ("slack", &["slack"], READS),
    ("mondaydotcom", &["monday"], READS),
    ("hubspot", &["hubspot"], READS),
    ("amplitude", &["amplitude"], READS),
    ("notion", &["notion"], READS),
];

/// MCP services whose writes are only the tools named here, any other write
/// of theirs refused before its read-back. Notion's (ruled on #4710, 4 Nov
/// 23:2x): create, append to and update the owner's pages and blocks
/// (`notion-update-page` is how its server appends); never delete, archive,
/// move, duplicate, a database's shape, a comment, or anything that shares
/// or changes who may see a page, which no read-back makes safe.
pub const WRITE_TOOLS: &[(&str, &[&str])] =
    &[("notion", &["notion-create-pages", "notion-update-page"])];

/// Argument keys that turn an allowed write into one left out: Notion's
/// trash and archive are a page's property, so an update could carry them.
pub const WRITE_KEYS_REFUSED: &[&str] = &["archived", "in_trash", "is_archived", "trashed"];

/// Whether `code` may write with `name` and `arguments`. A service with no
/// entry in `WRITE_TOOLS` writes with any tool `is_read` calls a write, as
/// Slack's do.
pub fn write_allowed(code: &str, name: &str, arguments: &Value) -> Result<(), String> {
    let Some((_, tools)) = WRITE_TOOLS.iter().find(|(service, _)| *service == code) else {
        return Ok(());
    };
    if !tools.contains(&name) {
        return Err(format!(
            "{name} is not a write the lamp makes: it only creates, adds to and updates the owner's pages, never deletes, archives, moves or shares one"
        ));
    }
    if carries_key(arguments, WRITE_KEYS_REFUSED, 0) {
        return Err(format!(
            "{name} may not archive or trash a page: the lamp only creates, adds to and updates the owner's pages"
        ));
    }
    Ok(())
}

/// Whether any object in `value`, at any depth, has one of `keys`; past 32
/// levels, as if it had.
fn carries_key(value: &Value, keys: &[&str], depth: usize) -> bool {
    if depth > 32 {
        return true;
    }
    match value {
        Value::Object(map) => map.iter().any(|(key, value)| {
            keys.contains(&key.to_ascii_lowercase().as_str()) || carries_key(value, keys, depth + 1)
        }),
        Value::Array(items) => items.iter().any(|item| carries_key(item, keys, depth + 1)),
        _ => false,
    }
}

/// The words of a tool's name, in lower case: split at anything not a
/// letter or digit and where a lower-case letter meets an upper-case one.
pub fn name_words(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut lower_before = false;
    for c in name.chars() {
        if !c.is_ascii_alphanumeric() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            lower_before = false;
            continue;
        }
        if c.is_ascii_uppercase() && lower_before && !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
        lower_before = c.is_ascii_lowercase() || c.is_ascii_digit();
        word.push(c.to_ascii_lowercase());
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// Whether `tool` (a `tools/list` entry) of `code` only reads.
pub fn is_read(code: &str, tool: &Value) -> bool {
    let Some(name) = tool.get("name").and_then(Value::as_str) else {
        return false;
    };
    let words = name_words(name);
    if words.is_empty()
        || words
            .iter()
            .any(|word| WRITE_WORDS.contains(&word.as_str()))
    {
        return false;
    }
    let hint = |key: &str| {
        tool.get("annotations")
            .and_then(|annotations| annotations.get(key))
            .and_then(Value::as_bool)
    };
    if hint("destructiveHint") == Some(true) {
        return false;
    }
    let Some((_, skipped, verbs)) = READ_VERBS.iter().find(|(service, ..)| *service == code) else {
        return false;
    };
    let verb = words
        .iter()
        .find(|word| !skipped.contains(&word.as_str()))
        .map_or("", String::as_str);
    hint("readOnlyHint") == Some(true) || verbs.contains(&verb)
}

/// The endpoint a link uses: its `mcp_url` if it names one, else the
/// service's own; either only on the service's pinned hosts, https, no
/// user, port, query or fragment.
pub fn pinned_url(service: &Service, given: Option<&str>) -> Result<String, &'static str> {
    let own = service.mcp.ok_or("that service is not an MCP one")?;
    let url = given
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .unwrap_or(own);
    let refused = "the connector's MCP address is not the service's own";
    let (host, _) = net::split(url).map_err(|_| refused)?;
    if url.contains('?') || !catalog::host_allowed(service.code, &host) {
        return Err(refused);
    }
    Ok(url.to_owned())
}

/// Headers an MCP link may not name for its token.
const RESERVED_HEADERS: &[&str] = &[
    "host",
    "content-type",
    "content-length",
    "accept",
    "connection",
    "transfer-encoding",
    "cookie",
    "mcp-session-id",
    "mcp-protocol-version",
    "user-agent",
];

/// The header the token goes in, as V1's `connectorAuthHeader` reads the
/// link's `mcp_auth_header`: `bearer_access_token` (or none) the access
/// token as a bearer, `bearer_api_key` the key as one, `header:<Name>` the
/// token as that header's value.
pub fn auth(entry: &Entry) -> Result<(String, String), &'static str> {
    let descriptor = entry
        .credentials
        .get("mcp_auth_header")
        .map_or("", |descriptor| descriptor.trim());
    let bearer = |token: &str| -> Result<(String, String), &'static str> {
        if token.is_empty() {
            return Err("the connector's data holds no credential");
        }
        Ok(("authorization".to_owned(), format!("Bearer {token}")))
    };
    if descriptor == "bearer_api_key" {
        return bearer(&entry.api_key);
    }
    if let Some(name) = descriptor.strip_prefix("header:") {
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty() || name == "authorization" {
            return bearer(entry.token());
        }
        if name.len() > 64
            || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            || RESERVED_HEADERS.contains(&name.as_str())
        {
            return Err("the connector names a header its token can't go in");
        }
        if entry.token().is_empty() {
            return Err("the connector's data holds no credential");
        }
        return Ok((name, entry.token().to_owned()));
    }
    bearer(entry.token())
}

/// Whether a link to an MCP service can be kept: its address pinned, its
/// header one the lamp sends.
pub fn check_set(service: &Service, entry: &Entry) -> Result<(), &'static str> {
    pinned_url(
        service,
        entry.credentials.get("mcp_url").map(String::as_str),
    )?;
    auth(entry).map(|_| ())
}

/// Where one service's calls go, with its token. Its `Debug` names the
/// header, never its value.
pub struct Endpoint {
    pub code: &'static str,
    pub name: &'static str,
    pub url: String,
    header: (String, String),
    secrets: Vec<String>,
    /// Set when the service refused the link itself: a 401, or its words
    /// for a dead token ([`DEAD_TOKEN`]). Never on a 403.
    refused: AtomicBool,
}

/// An MCP service's or Slack's own words for a token it no longer takes.
pub const DEAD_TOKEN: &[&str] = &["invalid_auth", "token_expired", "token_revoked"];

/// Whether one of the service's own error fields says the token is dead:
/// the field is a [`DEAD_TOKEN`] word, alone or leading its explanation
/// ("invalid_auth: ..."), after an optional "error:". The word anywhere
/// else (a message or an issue the service quotes) is not its refusal.
pub fn says_dead_token(said: &str) -> bool {
    let said = said.trim_start();
    let said = match said.get(..6) {
        Some(head) if head.eq_ignore_ascii_case("error:") => said[6..].trim_start(),
        _ => said,
    };
    DEAD_TOKEN.iter().any(|word| {
        said.strip_prefix(word)
            .is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_'))
    })
}

/// Whether an error object of the service's own says the token is dead in
/// its own fields: `error`, `code` or `message` as text, the same in its
/// `data`, or `data` itself as text. Nothing else in it is read.
fn error_says_dead_token(error: &Value) -> bool {
    let own = |value: &Value| {
        ["error", "code", "message"]
            .into_iter()
            .filter_map(|key| value.get(key).and_then(Value::as_str))
            .any(says_dead_token)
    };
    own(error)
        || error
            .get("data")
            .is_some_and(|data| data.as_str().is_some_and(says_dead_token) || own(data))
}

/// Whether a tool's error result is the service saying the token is dead:
/// by its `structuredContent`'s own fields, or by its first content block,
/// the error itself, as text or as a JSON object's own fields (Slack's
/// `{"ok":false,"error":"invalid_auth"}`). Any later block, and any text the
/// error quotes, is the service's content, not its refusal.
fn tool_error_says_dead_token(result: &Value) -> bool {
    if result
        .get("structuredContent")
        .is_some_and(error_says_dead_token)
    {
        return true;
    }
    let Some(first) = result
        .get("content")
        .and_then(Value::as_array)
        .and_then(|content| content.first())
        .and_then(|block| block.get("text"))
        .and_then(Value::as_str)
    else {
        return false;
    };
    match serde_json::from_str::<Value>(first) {
        Ok(value) if value.is_object() => error_says_dead_token(&value),
        _ => says_dead_token(first),
    }
}

impl fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Endpoint")
            .field("code", &self.code)
            .field("url", &self.url)
            .field("header", &self.header.0)
            .finish()
    }
}

impl Endpoint {
    /// `entry`'s endpoint on `service`, pinned.
    pub fn new(service: &'static Service, entry: &Entry) -> Result<Endpoint, String> {
        let url = pinned_url(
            service,
            entry.credentials.get("mcp_url").map(String::as_str),
        )?;
        Ok(Endpoint {
            code: service.code,
            name: service.name,
            url,
            header: auth(entry)?,
            secrets: [&entry.access_token, &entry.api_key, &entry.refresh_token]
                .into_iter()
                .filter(|secret| !secret.is_empty())
                .cloned()
                .collect(),
            refused: AtomicBool::new(false),
        })
    }

    /// Whether the service refused the link itself in a call through here.
    pub fn refused(&self) -> bool {
        self.refused.load(Ordering::Relaxed)
    }

    fn mark_refused(&self) {
        self.refused.store(true, Ordering::Relaxed);
    }

    /// `text` with every secret of the link taken out.
    pub fn scrub(&self, text: &str) -> String {
        self.secrets
            .iter()
            .fold(text.to_owned(), |text, secret| net::scrub(&text, secret))
    }
}

/// The JSON-RPC messages in an answer: its JSON (one or a batch), or the
/// `data` of each event of an event stream.
pub fn messages(reply: &Reply) -> Result<Vec<Value>, &'static str> {
    let kind = reply
        .header("content-type")
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let text = std::str::from_utf8(&reply.body).map_err(|_| "the service's answer is not text")?;
    match kind.as_str() {
        "application/json" => match serde_json::from_str(text) {
            Ok(Value::Array(list)) => Ok(list),
            Ok(one) => Ok(vec![one]),
            Err(_) => Err("the service's answer didn't read"),
        },
        "text/event-stream" => Ok(events(text)),
        _ => Err("the service answered in a form MCP doesn't use"),
    }
}

/// The JSON of each event's `data` lines in an event stream.
pub fn events(text: &str) -> Vec<Value> {
    text.replace("\r\n", "\n")
        .split("\n\n")
        .filter_map(|event| {
            let data: Vec<&str> = event
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(|data| data.strip_prefix(' ').unwrap_or(data))
                .collect();
            if data.is_empty() {
                return None;
            }
            serde_json::from_str(&data.join("\n")).ok()
        })
        .collect()
}

/// A session id the lamp sends back: visible ASCII, bounded.
fn session_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= SESSION_MOST
        && id.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
}

/// One sequence of calls to one service: opened with the handshake, its
/// session kept until it is dropped.
pub struct Client<'a, H> {
    http: &'a H,
    endpoint: &'a Endpoint,
    session: Option<String>,
    version: Option<String>,
    next: u64,
}

impl<H> fmt::Debug for Client<'_, H> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("endpoint", self.endpoint)
            .field("session", &self.session.is_some())
            .field("version", &self.version)
            .finish()
    }
}

impl<'a, H: Http> Client<'a, H> {
    /// The handshake: `initialize`, then `notifications/initialized`.
    pub async fn open(http: &'a H, endpoint: &'a Endpoint) -> Result<Client<'a, H>, String> {
        let mut client = Client {
            http,
            endpoint,
            session: None,
            version: None,
            next: 0,
        };
        let result = client
            .request(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {"name": "lampOS-services", "version": env!("CARGO_PKG_VERSION")},
                }),
            )
            .await?;
        let version = result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .filter(|version| {
                version.len() <= 32
                    && version
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
            })
            .unwrap_or(PROTOCOL_VERSION);
        client.version = Some(version.to_owned());
        client.notify("notifications/initialized").await?;
        Ok(client)
    }

    /// Post one message, guarded: the pinned host, https.
    async fn post(&self, message: &Value) -> Result<Reply, String> {
        let endpoint = self.endpoint;
        let (host, _) = net::split(&endpoint.url)?;
        if !catalog::host_allowed(endpoint.code, &host) {
            return Err(format!(
                "the lamp sends a {} token only to its own hosts",
                endpoint.code
            ));
        }
        let mut headers = vec![
            (
                "accept".to_owned(),
                "application/json, text/event-stream".to_owned(),
            ),
            ("content-type".to_owned(), "application/json".to_owned()),
            ("user-agent".to_owned(), "lampOS-services".to_owned()),
        ];
        if let Some(version) = &self.version {
            headers.push(("mcp-protocol-version".to_owned(), version.clone()));
        }
        if let Some(session) = &self.session {
            headers.push(("mcp-session-id".to_owned(), session.clone()));
        }
        headers.push(endpoint.header.clone());
        let reply = self
            .http
            .send(Call {
                method: Method::Post,
                url: endpoint.url.clone(),
                headers,
                body: Some(message.to_string().into_bytes()),
                most: crate::net::BODY_MOST,
            })
            .await?;
        let name = endpoint.name;
        match reply.status {
            200..=299 => Ok(reply),
            300..=399 => Err(format!(
                "{name} answered with a redirect, which the lamp doesn't follow"
            )),
            401 | 403 => {
                if reply.status == 401 {
                    endpoint.mark_refused();
                }
                Err(format!(
                    "{name} refused the lamp's link: it may need linking again in the app"
                ))
            }
            404 if self.session.is_some() => Err(format!("{name} ended the session: try again")),
            429 => Err(format!("{name} is busy: try again in a while")),
            _ => Err(format!("{name} answered with an error")),
        }
    }

    /// One request and its result.
    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next += 1;
        let id = self.next;
        let reply = self
            .post(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await?;
        if self.session.is_none()
            && let Some(session) = reply.header("mcp-session-id").filter(|id| session_ok(id))
        {
            self.session = Some(session.to_owned());
        }
        let name = self.endpoint.name;
        let message = messages(&reply)?
            .into_iter()
            .find(|message| {
                message.get("id").and_then(Value::as_u64) == Some(id)
                    && (message.get("result").is_some() || message.get("error").is_some())
            })
            .ok_or_else(|| format!("{name}'s answer had no reply to the lamp's call"))?;
        if let Some(error) = message.get("error") {
            let said = error.get("message").and_then(Value::as_str).unwrap_or("");
            if error_says_dead_token(error) {
                self.endpoint.mark_refused();
            }
            return Err(format!(
                "{name} refused the call (its words, quoted: \"{}\")",
                self.endpoint.scrub(&mail::tidy(said, 200))
            ));
        }
        message
            .get("result")
            .filter(|result| result.is_object())
            .cloned()
            .ok_or_else(|| format!("{name}'s answer didn't read"))
    }

    /// One notification; any 2xx is taken.
    async fn notify(&mut self, method: &str) -> Result<(), String> {
        self.post(&json!({"jsonrpc": "2.0", "method": method}))
            .await
            .map(|_| ())
    }

    /// The service's tools, at most [`TOOLS_MOST`] over [`PAGES_MOST`] pages.
    pub async fn tools(&mut self) -> Result<Vec<Value>, String> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..PAGES_MOST {
            let params = match &cursor {
                Some(cursor) => json!({"cursor": cursor}),
                None => json!({}),
            };
            let result = self.request("tools/list", params).await?;
            let page = result.get("tools").and_then(Value::as_array);
            tools.extend(
                page.into_iter()
                    .flatten()
                    .filter(|tool| tool.get("name").and_then(Value::as_str).is_some())
                    .take(TOOLS_MOST - tools.len())
                    .cloned(),
            );
            cursor = result
                .get("nextCursor")
                .and_then(Value::as_str)
                .filter(|cursor| !cursor.is_empty() && cursor.len() <= 1_024)
                .map(str::to_owned);
            if cursor.is_none() || tools.len() >= TOOLS_MOST {
                break;
            }
        }
        Ok(tools)
    }

    /// Call one tool; its result.
    /// A tool's error that says the token is dead marks the link refused
    /// ([`tool_error_says_dead_token`]); a result quoting the words doesn't.
    pub async fn call(&mut self, tool: &str, arguments: &Value) -> Result<Value, String> {
        let result = self
            .request("tools/call", json!({"name": tool, "arguments": arguments}))
            .await?;
        if result.get("isError").and_then(Value::as_bool) == Some(true)
            && tool_error_says_dead_token(&result)
        {
            self.endpoint.mark_refused();
        }
        Ok(result)
    }
}

fn text<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

/// The tool a call names: letters, digits, `_`, `-`, `.` or `/`, bounded.
pub fn tool_name(args: &Value) -> Result<&str, String> {
    let name = text(args, "tool").ok_or("the call needs its tool")?;
    if name.len() > NAME_MOST
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-./".contains(c))
    {
        return Err("tool is a tool's name, as tools listed it".into());
    }
    Ok(name)
}

/// The tool's arguments: an object, or its JSON text; none is `{}`.
pub fn arguments(args: &Value) -> Result<Value, String> {
    let arguments = match args.get("arguments") {
        None | Some(Value::Null) => json!({}),
        Some(Value::Object(object)) => Value::Object(object.clone()),
        Some(Value::String(text)) if text.trim().is_empty() => json!({}),
        Some(Value::String(text)) => serde_json::from_str::<Map<String, Value>>(text)
            .map(Value::Object)
            .map_err(|_| "arguments are a JSON object")?,
        Some(_) => return Err("arguments are a JSON object".into()),
    };
    if arguments.to_string().len() > ARGUMENTS_MOST {
        return Err(format!("the arguments are over {ARGUMENTS_MOST} bytes"));
    }
    Ok(arguments)
}

/// A tool's argument names, a required one marked `*`.
fn argument_names(tool: &Value) -> Vec<String> {
    let schema = tool.get("inputSchema");
    let required: Vec<&str> = schema
        .and_then(|schema| schema.get("required"))
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    schema
        .and_then(|schema| schema.get("properties"))
        .and_then(Value::as_object)
        .map(|properties| {
            properties
                .keys()
                .take(20)
                .map(|key| {
                    let key = mail::tidy(key, 40);
                    if required.contains(&key.as_str()) {
                        format!("{key}*")
                    } else {
                        key
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `what: "tools"`: the read tools, quoted, and the names of the others,
/// which only `service_write` makes.
pub fn tools_quoted(endpoint: &Endpoint, tools: &[Value]) -> Value {
    let (read, write): (Vec<&Value>, Vec<&Value>) =
        tools.iter().partition(|tool| is_read(endpoint.code, tool));
    let items = read
        .iter()
        .map(|tool| {
            json!({
                "tool": tool.get("name"),
                "about": endpoint.scrub(&mail::tidy(tool.get("description").and_then(Value::as_str).unwrap_or(""), 160)),
                "arguments": argument_names(tool),
            })
        })
        .collect();
    let mut result = reads::quoted(endpoint.name, items);
    result["write_tools"] = json!([]);
    for tool in write {
        let name = tool.get("name").and_then(Value::as_str).unwrap_or("");
        result["write_tools"]
            .as_array_mut()
            .expect("a list")
            .push(json!(mail::tidy(name, NAME_MOST)));
        if result.to_string().len() + 120 > reads::RESULT_MOST {
            result["write_tools"].as_array_mut().expect("a list").pop();
            break;
        }
    }
    result["write_tools_about"] =
        json!("writes: only through service_write, after a full read-back and a spoken send");
    result
}

/// A text cut into pieces of at most [`PIECE_MOST`] characters, at
/// paragraph ends where it can be.
fn pieces(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for paragraph in text.split("\n\n") {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }
        let mut rest = paragraph;
        while !rest.is_empty() {
            let cut = rest
                .char_indices()
                .nth(PIECE_MOST)
                .map_or(rest.len(), |(at, _)| at);
            let piece = &rest[..cut];
            match out.last_mut() {
                Some(last) if last.chars().count() + piece.chars().count() < PIECE_MOST => {
                    last.push_str("\n\n");
                    last.push_str(piece);
                }
                _ => out.push(piece.to_owned()),
            }
            rest = &rest[cut..];
        }
    }
    out
}

/// The items one text gives, each with its group for the security
/// check's join: a JSON answer's list, each in the channel, thread or page
/// it names (`reads::group_of`), else the text's pieces, all of one result's
/// texts one group: one answer, read as one page.
fn text_items(text: &str) -> Vec<(Option<String>, Value)> {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(_) | Value::Array(_)) => reads::items(text)
            .into_iter()
            .map(|item| {
                let group = reads::group_of(&item).or_else(|| Some(reads::ANSWER.to_owned()));
                (group, item)
            })
            .collect(),
        _ => pieces(text)
            .into_iter()
            .map(|piece| (Some("text".to_owned()), Value::String(piece)))
            .collect(),
    }
}

/// A tool's result as the model gets it: its content's items, each with
/// the link's secrets taken out and any security text (one-time codes,
/// password resets, sign-in alerts) left out whole, quoted as data from
/// the service under the voice's cap, with how many were left out.
pub fn result_quoted(endpoint: &Endpoint, result: &Value) -> Value {
    let mut items: Vec<(Option<String>, Value)> = Vec::new();
    for content in result
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match content.get("type").and_then(Value::as_str) {
            Some("text") => items.extend(text_items(
                &endpoint.scrub(content.get("text").and_then(Value::as_str).unwrap_or("")),
            )),
            Some("resource") => match content
                .get("resource")
                .and_then(|resource| resource.get("text"))
                .and_then(Value::as_str)
            {
                Some(text) => items.extend(text_items(&endpoint.scrub(text))),
                None => items.push((None, json!("a file, left out"))),
            },
            Some("resource_link") => items.push((None, json!({
                "link": mail::tidy(content.get("name").and_then(Value::as_str).unwrap_or(""), 200),
            }))),
            Some("image") => items.push((None, json!("an image, left out"))),
            Some("audio") => items.push((None, json!("a sound, left out"))),
            _ => {}
        }
    }
    if items.is_empty()
        && let Some(structured) = result.get("structuredContent")
    {
        items = text_items(&endpoint.scrub(&structured.to_string()));
    }
    // Security text is left out by `reads::quoted_in`, each piece alone and
    // with its neighbours of the same text, so a code cut across two pieces
    // goes too.
    let total = items.len();
    let mut quoted = reads::quoted_in(endpoint.name, items.into_iter().take(PIECES_MOST).collect());
    if total > PIECES_MOST {
        let left = quoted["left_out"].as_u64().unwrap_or(0) as usize + total - PIECES_MOST;
        quoted["left_out"] = json!(left);
        quoted["left_out_why"] = json!("to keep the answer short");
    }
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        quoted["tool_error"] = json!(true);
    }
    quoted
}

/// An argument's value as a read-back says it: text whole, in quotes;
/// anything else as its JSON.
fn said_value(value: &Value) -> String {
    match value {
        Value::String(text) => format!("\"{text}\""),
        other => other.to_string(),
    }
}

/// Argument names that say where a write lands.
const TARGET_WORDS: &[&str] = &[
    "channel",
    "channels",
    "board",
    "item",
    "object",
    "user",
    "users",
    "recipient",
    "recipients",
    "to",
    "email",
    "project",
    "workspace",
    "list",
    "group",
    "thread",
    "owner",
    "contact",
    "company",
    "deal",
    "ticket",
    "chart",
    "dashboard",
    "id",
    "ids",
];

/// The read-back of a write tool: the service and account, the tool, where
/// it lands as its arguments say, and every argument whole.
pub fn read_back(service: &Service, entry: &Entry, tool: &Value, arguments: &Value) -> String {
    let name = tool.get("name").and_then(Value::as_str).unwrap_or("");
    let about = tool
        .get("description")
        .and_then(Value::as_str)
        .map(|about| mail::tidy(about, 160))
        .filter(|about| !about.is_empty())
        .map_or(String::new(), |about| {
            format!(" (the service says it: \"{about}\")")
        });
    let account = entry.account();
    let account = if account.is_empty() {
        "the linked account".to_owned()
    } else {
        account
    };
    let given: Vec<(&String, &Value)> = arguments
        .as_object()
        .map(|object| object.iter().collect())
        .unwrap_or_default();
    let targets: Vec<String> = given
        .iter()
        .filter(|(key, _)| {
            name_words(key)
                .iter()
                .any(|word| TARGET_WORDS.contains(&word.as_str()))
        })
        .map(|(key, value)| format!("{key} {}", said_value(value)))
        .collect();
    let target = if targets.is_empty() {
        "none named, so the tool's own default".to_owned()
    } else {
        targets.join(", ")
    };
    let every = if given.is_empty() {
        "none".to_owned()
    } else {
        given
            .iter()
            .map(|(key, value)| format!("{key} = {}", said_value(value)))
            .collect::<Vec<_>>()
            .join("; ")
    };
    format!(
        "In {}, as {account}, run the tool \"{name}\"{about}. Where it lands: {target}. Every argument: {every}. It changes or sends something there; whoever can see that place sees it.",
        service.name
    )
}

#[cfg(test)]
pub(crate) mod fake {
    //! A fake MCP server behind the `Http` seam: it answers the handshake,
    //! a list of tools and their calls, as JSON or as an event stream, and
    //! keeps every call it was sent.
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct FakeMcp {
        pub calls: Mutex<Vec<Call>>,
        /// Answer as an event stream instead of JSON.
        pub sse: bool,
        /// The text a call's answer carries, beside its echo.
        pub said: String,
        /// An error every `tools/list` answers with, when set.
        pub refusal: Mutex<Option<Value>>,
        /// Answer each `tools/call` as a tool's error (`isError`).
        pub tool_error: bool,
    }

    impl FakeMcp {
        pub fn sent(&self) -> Vec<Value> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .map(|call| serde_json::from_slice(call.body.as_deref().unwrap_or(b"{}")).unwrap())
                .collect()
        }

        pub fn methods(&self) -> Vec<String> {
            self.sent()
                .iter()
                .map(|message| message["method"].as_str().unwrap_or("").to_owned())
                .collect()
        }
    }

    pub fn tools() -> Value {
        json!([
            {"name": "slack_search_messages", "description": "Search messages", "inputSchema": {"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}},
            {"name": "slack_read_channel", "description": "Read a channel"},
            {"name": "conversations_history", "annotations": {"readOnlyHint": true}},
            {"name": "slack_send_message", "description": "Send a message", "annotations": {"readOnlyHint": true}},
            {"name": "get_and_delete_draft", "annotations": {"readOnlyHint": true}},
            {"name": "list_wipe", "annotations": {"destructiveHint": true}},
            {"name": "mystery"},
        ])
    }

    impl Http for FakeMcp {
        async fn send(&self, call: Call) -> Result<Reply, String> {
            self.calls.lock().unwrap().push(call.clone());
            let message: Value =
                serde_json::from_slice(call.body.as_deref().unwrap_or(b"")).unwrap();
            let has = |name: &str| call.headers.iter().any(|(kept, _)| kept == name);
            let method = message["method"].as_str().unwrap_or("");
            if method != "initialize" {
                assert!(has("mcp-session-id"), "{method} without its session");
                assert!(has("mcp-protocol-version"), "{method} without its version");
            }
            let result = match method {
                "initialize" => {
                    json!({"protocolVersion": "2025-06-18", "capabilities": {"tools": {}}, "serverInfo": {"name": "fake"}})
                }
                "notifications/initialized" => {
                    return Ok(Reply {
                        status: 202,
                        ..Reply::default()
                    });
                }
                "tools/list" if self.refusal.lock().unwrap().is_some() => {
                    let error = self.refusal.lock().unwrap().clone();
                    return Ok(Reply {
                        status: 200,
                        body: json!({"jsonrpc":"2.0","id":message["id"],"error":error})
                            .to_string()
                            .into_bytes(),
                        headers: vec![("content-type".into(), "application/json".into())],
                    });
                }
                "tools/list" => json!({"tools": tools()}),
                "tools/call" => json!({"content": [
                    {"type": "text", "text": format!("called {} with {}", message["params"]["name"], message["params"]["arguments"])},
                    // Between them, so a security `said` hides this and
                    // not the echo (`reads::quoted` hides a neighbour).
                    {"type": "text", "text": "and then:"},
                    {"type": "text", "text": self.said},
                ], "isError": self.tool_error}),
                _ => {
                    return Ok(Reply {
                        status: 200,
                        body: json!({"jsonrpc":"2.0","id":message["id"],"error":{"code":-32601,"message":"no such method"}}).to_string().into_bytes(),
                        headers: vec![("content-type".into(), "application/json".into())],
                    });
                }
            };
            let answer = json!({"jsonrpc": "2.0", "id": message["id"], "result": result});
            let (kind, body) = if self.sse {
                (
                    "text/event-stream",
                    format!(
                        "event: message\r\ndata: {}\r\n\r\n",
                        json!({"jsonrpc":"2.0","method":"notifications/message","params":{}})
                    ) + &format!("id: 1\nevent: message\ndata: {answer}\n\n"),
                )
            } else {
                ("application/json; charset=utf-8", answer.to_string())
            };
            let mut headers = vec![("content-type".to_owned(), kind.to_owned())];
            if method == "initialize" {
                headers.push(("mcp-session-id".into(), "sess-4710".into()));
            }
            Ok(Reply {
                status: 200,
                body: body.into_bytes(),
                headers,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::{FakeMcp, tools};
    use super::*;

    fn run<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    fn slack(extra: Value) -> Entry {
        let mut data =
            json!({"auth_type":"oauth","access_token":"planted-mcp-4710","expires_in":3600});
        if let (Some(data), Some(extra)) = (data.as_object_mut(), extra.as_object()) {
            data.extend(extra.clone());
        }
        Entry::from_set("slack", &data, 0).unwrap()
    }

    #[test]
    fn tools_are_reads_only_by_hint_or_verb_and_never_by_a_write_word() {
        let sorted: Vec<(String, bool)> = tools()
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| {
                (
                    tool["name"].as_str().unwrap().to_owned(),
                    is_read("slack", tool),
                )
            })
            .collect();
        assert_eq!(
            sorted,
            [
                ("slack_search_messages".to_owned(), true),
                ("slack_read_channel".to_owned(), true),
                ("conversations_history".to_owned(), true),
                ("slack_send_message".to_owned(), false),
                ("get_and_delete_draft".to_owned(), false),
                ("list_wipe".to_owned(), false),
                ("mystery".to_owned(), false),
            ]
        );
        let tool = |name: &str| json!({"name": name});
        assert!(is_read("hubspot", &tool("search_crm_objects")));
        assert!(is_read("mondaydotcom", &tool("getBoardItems")));
        assert!(!is_read("mondaydotcom", &tool("getBoardItemsAndArchive")));
        assert!(!is_read("hubspot", &tool("hubspot_create_contact")));
        assert!(is_read("amplitude", &tool("query_chart")));
        assert!(!is_read("amplitude", &tool("amplitude")));
        // Notion's tools are named `notion-search`, `notion-fetch` and the like.
        assert!(is_read("notion", &tool("notion-search")));
        assert!(is_read("notion", &tool("notion-fetch")));
        assert!(is_read("notion", &tool("notion-get-comments")));
        assert!(!is_read("notion", &tool("notion-update-page")));
        assert!(!is_read("notion", &tool("notion-create-pages")));
        assert!(!is_read("notion", &tool("notion-duplicate-page")));
        assert!(!is_read("notion", &tool("notion-move-pages")));
        // A read word inside another isn't one; an unknown service reads nothing.
        assert!(!is_read("slack", &tool("listen")));
        assert!(!is_read("github", &tool("get_issue")));
        assert_eq!(name_words("getHTTPItems_v2"), ["get", "httpitems", "v2"]);
    }

    #[test]
    fn a_link_is_pinned_to_its_services_own_mcp_host() {
        let service = catalog::service("slack").unwrap();
        assert_eq!(
            pinned_url(service, None).unwrap(),
            "https://mcp.slack.com/mcp"
        );
        assert_eq!(
            pinned_url(service, Some("https://mcp.slack.com/v2/mcp")).unwrap(),
            "https://mcp.slack.com/v2/mcp"
        );
        for url in [
            "https://evil.test/mcp",
            "https://mcp.slack.com.evil.test/mcp",
            "https://x.mcp.slack.com/mcp",
            "http://mcp.slack.com/mcp",
            "https://mcp.slack.com:8443/mcp",
            "https://me@mcp.slack.com/mcp",
            "https://mcp.slack.com/mcp?to=evil.test",
            "https://mcp.monday.com/mcp",
        ] {
            assert!(pinned_url(service, Some(url)).is_err(), "{url}");
        }
        let amplitude = catalog::service("amplitude").unwrap();
        assert!(pinned_url(amplitude, Some("https://mcp.eu.amplitude.com/mcp")).is_ok());
        // Notion's fetched `mcp_url` is taken on its MCP host, never its REST one.
        let notion = catalog::service("notion").unwrap();
        assert_eq!(
            pinned_url(notion, None).unwrap(),
            "https://mcp.notion.com/mcp"
        );
        assert!(pinned_url(notion, Some("https://mcp.notion.com/mcp")).is_ok());
        for url in ["https://api.notion.com/mcp", "https://notion.so/mcp"] {
            assert!(pinned_url(notion, Some(url)).is_err(), "{url}");
        }
        // A link naming another host is refused at set, nothing kept.
        let data = json!({"auth_type":"oauth","access_token":"planted-mcp-4710","credentials":{"mcp_url":"https://evil.test/mcp"}});
        let why = Entry::from_set("slack", &data, 0).unwrap_err();
        assert!(!why.contains("planted"), "{why}");
        // V1's header styles.
        let entry = slack(json!({}));
        assert_eq!(
            auth(&entry).unwrap(),
            ("authorization".into(), "Bearer planted-mcp-4710".into())
        );
        let key = Entry::from_set(
            "amplitude",
            &json!({"auth_type":"pat","api_key":"k-1","credentials":{"mcp_auth_header":"bearer_api_key"}}),
            0,
        )
        .unwrap();
        assert_eq!(auth(&key).unwrap().1, "Bearer k-1");
        let custom = Entry::from_set(
            "hubspot",
            &json!({"auth_type":"pat","api_key":"k-2","credentials":{"mcp_auth_header":"header:X-Api-Key"}}),
            0,
        )
        .unwrap();
        assert_eq!(auth(&custom).unwrap(), ("x-api-key".into(), "k-2".into()));
        for header in ["header:Host", "header:mcp-session-id", "header:a b"] {
            let data =
                json!({"auth_type":"pat","api_key":"k","credentials":{"mcp_auth_header":header}});
            assert!(Entry::from_set("hubspot", &data, 0).is_err(), "{header}");
        }
    }

    #[test]
    fn the_handshake_keeps_its_session_and_reads_either_answer() {
        for sse in [false, true] {
            let fake = FakeMcp {
                sse,
                ..FakeMcp::default()
            };
            let entry = slack(json!({}));
            let endpoint = Endpoint::new(catalog::service("slack").unwrap(), &entry).unwrap();
            let listed = run(async {
                let mut client = Client::open(&fake, &endpoint).await?;
                client.tools().await
            })
            .unwrap();
            assert_eq!(listed.len(), 7);
            assert_eq!(
                fake.methods(),
                ["initialize", "notifications/initialized", "tools/list"]
            );
            let calls = fake.calls.lock().unwrap().clone();
            for call in &calls {
                assert_eq!(call.url, "https://mcp.slack.com/mcp");
                assert!(call.headers.contains(&(
                    "accept".into(),
                    "application/json, text/event-stream".into()
                )));
            }
            assert!(
                calls[2]
                    .headers
                    .contains(&("mcp-session-id".into(), "sess-4710".into()))
            );
            let sent = fake.sent();
            assert_eq!(sent[0]["params"]["protocolVersion"], PROTOCOL_VERSION);
            assert!(sent[1].get("id").is_none());
        }
    }

    #[test]
    fn event_streams_and_odd_answers_are_read_strictly() {
        let reply = |kind: &str, body: &str| Reply {
            status: 200,
            body: body.as_bytes().to_vec(),
            headers: vec![("content-type".into(), kind.into())],
        };
        let stream = "data: {\"jsonrpc\":\"2.0\",\ndata: \"id\":3,\"result\":{}}\n\n: comment\n\n";
        assert_eq!(
            messages(&reply("text/event-stream", stream)).unwrap(),
            vec![json!({"jsonrpc":"2.0","id":3,"result":{}})]
        );
        assert!(messages(&reply("text/html", "<p>")).is_err());
        assert!(messages(&reply("application/json", "{")).is_err());
        assert!(session_ok("sess-1") && !session_ok("a b") && !session_ok(""));
    }

    #[test]
    fn a_token_is_never_in_an_error_or_a_debug() {
        let entry = slack(json!({}));
        let endpoint = Endpoint::new(catalog::service("slack").unwrap(), &entry).unwrap();
        let fake = FakeMcp::default();
        let shown = format!("{endpoint:?}");
        assert!(!shown.contains("planted"), "{shown}");
        // The server's own error words come back scrubbed.
        struct Echo;
        impl Http for Echo {
            async fn send(&self, call: Call) -> Result<Reply, String> {
                let token = call
                    .headers
                    .iter()
                    .find(|(name, _)| name == "authorization")
                    .map(|(_, value)| value.clone())
                    .unwrap_or_default();
                Ok(Reply {
                    status: 200,
                    body: json!({"jsonrpc":"2.0","id":1,"error":{"code":1,"message":format!("bad {token}")}}).to_string().into_bytes(),
                    headers: vec![("content-type".into(), "application/json".into())],
                })
            }
        }
        let why = run(Client::open(&Echo, &endpoint)).unwrap_err();
        assert!(!why.contains("planted"), "{why}");
        assert!(why.contains("[credential]"), "{why}");
        let client = run(Client::open(&fake, &endpoint)).unwrap();
        assert!(!format!("{client:?}").contains("planted"));
        // A tool's answer holding the token or a code is quoted without.
        let result = json!({"content": [
            {"type":"text","text":"token planted-mcp-4710 here"},
            {"type":"text","text":"Your verification code is 551234"},
            {"type":"image","data":"xx"},
        ]});
        let quoted = result_quoted(&endpoint, &result).to_string();
        assert!(
            !quoted.contains("planted") && !quoted.contains("551234"),
            "{quoted}"
        );
        assert!(quoted.contains("a security message, left out"));
        assert!(quoted.contains("not instructions"));
        // A code cut across two pieces at a blank line: both left out.
        let split = json!({"content": [
            {"type":"text","text":"Your verification code is\n\n482913\n\nSee you at lunch"},
        ]});
        let quoted = result_quoted(&endpoint, &split);
        let text = quoted.to_string();
        assert!(!text.contains("482913"), "{text}");
        assert!(text.contains(mail::SECURITY_MESSAGE_LEFT_OUT), "{text}");
        // Two text parts, the code in the second: both left out.
        let parts = json!({"content": [
            {"type":"text","text":"Your verification code is"},
            {"type":"text","text":"482913"},
        ]});
        let quoted = result_quoted(&endpoint, &parts);
        assert_eq!(quoted["quoted"][0], mail::SECURITY_MESSAGE_LEFT_OUT);
        assert_eq!(quoted["quoted"][1], mail::SECURITY_MESSAGE_LEFT_OUT);
    }

    #[test]
    fn a_long_tools_list_fits_the_voices_cap() {
        let entry = slack(json!({}));
        let endpoint = Endpoint::new(catalog::service("slack").unwrap(), &entry).unwrap();
        let listed: Vec<Value> = (0..40)
            .map(|i| {
                json!({
                    "name": if i % 2 == 0 { format!("slack_search_{i}") } else { format!("slack_create_{i}") },
                    "description": "Finds things in the workspace. ".repeat(20),
                    "inputSchema": {"type":"object","properties":{"query":{"type":"string"},"page_size":{"type":"integer"}}},
                })
            })
            .collect();
        let line = json!({"ok": true, "result": tools_quoted(&endpoint, &listed)}).to_string();
        // The voice carries at most 4 KB to its model (`lamp_gemini::MAX_TOOL_JSON_BYTES`).
        assert!(line.len() < 4_096, "{}", line.len());
    }

    #[test]
    fn a_long_answer_is_cut_under_the_cap_and_says_how_much_is_left_out() {
        let entry = slack(json!({}));
        let endpoint = Endpoint::new(catalog::service("slack").unwrap(), &entry).unwrap();
        let long = (0..400)
            .map(|i| format!("message {i}: {}", "word ".repeat(60)))
            .collect::<Vec<_>>()
            .join("\n\n");
        let quoted = result_quoted(&endpoint, &json!({"content":[{"type":"text","text":long}]}));
        let line = json!({"ok": true, "result": quoted}).to_string();
        assert!(line.len() < reads::ANSWER_MOST, "{}", line.len());
        let kept = quoted["quoted"].as_array().unwrap().len();
        assert!(kept > 0);
        let total = pieces(&long).len();
        assert_eq!(quoted["left_out"], json!(total - kept));
        // A JSON answer's list is its items.
        let listed = result_quoted(
            &endpoint,
            &json!({"content":[{"type":"text","text":"{\"results\":[{\"a\":1},{\"a\":2}]}"}]}),
        );
        assert_eq!(listed["quoted"], json!([{"a":1},{"a":2}]));
    }

    #[test]
    fn a_read_back_names_the_tool_its_target_and_every_argument() {
        let entry = slack(json!({}));
        let tool = json!({"name":"slack_send_message","description":"Send a message"});
        let said = read_back(
            catalog::service("slack").unwrap(),
            &entry,
            &tool,
            &json!({"channel_id":"C42","text":"Ship it at 5.\nThanks","unfurl":false}),
        );
        for part in [
            "Slack",
            "\"slack_send_message\"",
            "channel_id \"C42\"",
            "text = \"Ship it at 5.\nThanks\"",
            "unfurl = false",
        ] {
            assert!(said.contains(part), "{part} in {said}");
        }
        assert!(arguments(&json!({"arguments":"[1]"})).is_err());
        assert!(arguments(&json!({"arguments":"x".repeat(5)})).is_err());
        assert_eq!(
            arguments(&json!({"arguments":"{\"a\":1}"})).unwrap(),
            json!({"a":1})
        );
        assert!(arguments(&json!({"arguments": {"t": "x".repeat(ARGUMENTS_MOST)}})).is_err());
        assert!(tool_name(&json!({"tool":"a b"})).is_err());
        assert!(tool_name(&json!({"tool":"slack_send_message"})).is_ok());
    }

    #[test]
    fn a_link_is_refused_by_the_services_own_error_never_by_text_it_quotes() {
        // The service's own error fields mark it.
        for error in [
            json!({"code":-32000,"message":"invalid_auth"}),
            json!({"code":-32000,"message":"Error: token_expired."}),
            json!({"code":-32000,"message":"failed","data":"token_revoked"}),
            json!({"code":-32000,"message":"failed","data":{"error":"invalid_auth"}}),
        ] {
            assert!(error_says_dead_token(&error), "{error}");
        }
        // Words it quotes, in its message or anywhere else, don't.
        for error in [
            json!({"code":-32000,"message":"no channel named \"invalid_auth\""}),
            json!({"code":-32000,"message":"ratelimited","data":{"text":"invalid_auth"}}),
            json!({"code":-32000,"message":"invalid_authority"}),
        ] {
            assert!(!error_says_dead_token(&error), "{error}");
        }
        // A tool's error: its first block or its structured error marks it.
        for result in [
            json!({"isError":true,"content":[{"type":"text","text":"{\"ok\":false,\"error\":\"invalid_auth\"}"}]}),
            json!({"isError":true,"content":[{"type":"text","text":"token_revoked: the token was revoked"}]}),
            json!({"isError":true,"structuredContent":{"error":"token_expired"},"content":[]}),
        ] {
            assert!(tool_error_says_dead_token(&result), "{result}");
        }
        // A message or an issue it quotes doesn't, in the error or after it.
        for result in [
            json!({"isError":true,"content":[{"type":"text","text":"{\"ok\":false,\"error\":\"msg_too_long\",\"message\":{\"text\":\"invalid_auth\"}}"}]}),
            json!({"isError":true,"content":[{"type":"text","text":"not posted: \"why do I get invalid_auth?\" is too long"}]}),
            json!({"isError":true,"content":[{"type":"text","text":"not found"},{"type":"text","text":"invalid_auth"}]}),
        ] {
            assert!(!tool_error_says_dead_token(&result), "{result}");
        }
        // Through a call: a good result, and a tool's error whose echo and
        // later block quote the words, leave the link as it was.
        for tool_error in [false, true] {
            let fake = FakeMcp {
                said: "invalid_auth".into(),
                tool_error,
                ..FakeMcp::default()
            };
            let entry = slack(json!({}));
            let endpoint = Endpoint::new(catalog::service("slack").unwrap(), &entry).unwrap();
            let result = run(async {
                let mut client = Client::open(&fake, &endpoint).await?;
                client
                    .call("slack_send_message", &json!({"text":"invalid_auth again"}))
                    .await
            })
            .unwrap();
            assert_eq!(result["isError"], tool_error);
            assert!(result.to_string().contains("invalid_auth"));
            assert!(!endpoint.refused(), "isError {tool_error}");
        }
    }
}
