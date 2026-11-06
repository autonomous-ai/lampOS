//! The person's own services linked from the app (Queue #4710): Gmail, their
//! calendar and Drive, Notion, Figma, GitHub, Linear, Asana, Ahrefs and a
//! Facebook Page. Off unless `LAMP_SERVICES=on`, with `LAMP_CLOUD=on`;
//! without it each kind is answered as any this lamp lacks.
//!
//! `lamp-cloud` keeps nothing itself: it hands each request to root's
//! `lamp-services` on its admin socket ([`ADMIN_SOCKET`]), one JSON line out
//! and one back, and that process keeps the tokens. As V1's, a
//! `connector.set.<code>` carries no token: it is V1's "privacy" type, and
//! the connector is fetched from the backend's
//! `GET <base>/devices/get-message?kind=connector.set.<code>` with the
//! lamp's key (V1's `privacy_fetch.go`). Each `connector.*` is answered
//! `starting` at once and `success` or `failure` later, from a thread of its
//! own, so the broker's loop never waits on the backend or the socket.
//!
//! The fetched connector goes into the admin socket's line and nowhere
//! else: never a reply, an error, a journal line or a `Debug`.
use crate::account::Account;
use crate::settings::Outcome;
use serde_json::{Map, Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;

/// `on`, beside `LAMP_CLOUD`, takes the services' kinds.
pub const ENVIRONMENT: &str = "LAMP_SERVICES";

/// `lamp-services`' admin socket, which only `lamp-cloud.service` may ask.
pub const ADMIN_SOCKET: &str = "/run/lampos-services/admin.sock";

/// The services this lamp knows, by V1's codes. Figma is `figma-api` as
/// the backend lists it for lamps, and `figma` as V1's helper named it.
/// `gmail-app` is a Lamp 2's Google sign-in for Gmail, send only; `gmail`
/// its app password. Then five MCP services: V1's four, then Notion, whose
/// app sign-in gives a token only its MCP server takes. Last, Spotify
/// (#5460), which `lamp-services` takes only with `LAMP_SERVICES_SPOTIFY`.
pub const CODES: [&str; 17] = [
    "gmail",
    "gmail-app",
    "google_calendar",
    "google_drive",
    "figma-api",
    "figma",
    "github",
    "linear",
    "asana",
    "ahrefs",
    "facebook",
    "slack",
    "mondaydotcom",
    "hubspot",
    "amplitude",
    "notion",
    "spotify",
];

/// A service linked: `connector.set.<code>`, its connector fetched.
pub const SET_PREFIX: &str = "connector.set.";
/// A service unlinked: `connector.remove.<code>`, its token wiped.
pub const REMOVE_PREFIX: &str = "connector.remove.";
/// The services linked now, by code and account, nothing secret.
pub const LIST_KIND: &str = "services.list";
/// The word in `system.info`'s `can` that says `connector.set.<code>` and
/// `connector.remove.<code>` are taken (they are many, so not each listed).
pub const CAN: &str = "services";

/// A code that is not one of [`CODES`].
pub const NOT_KNOWN: &str = "not a service this lamp knows";
/// The admin socket isn't there or refused the connection.
pub const NOT_RUNNING: &str = "the lamp's services are not running";
/// The admin socket gave no line that can be read in time.
pub const NO_ANSWER: &str = "the lamp's services did not answer in time";
/// Too many requests already under way.
pub const BUSY: &str = "the lamp's services are busy: try again in a moment";

/// The backend's route for a privacy kind's data, above the `/v1` base.
pub const FETCH_PATH: &str = "/devices/get-message";
/// V1's `privacyFetchTimeout`.
pub const FETCH_WITHIN: Duration = Duration::from_secs(30);
/// How long a `set` or `remove` is waited on: Gmail's link is checked (an
/// IMAP login, or its profile read for a Google sign-in) before it is
/// kept, then a small write.
pub const ADMIN_WAIT: Duration = Duration::from_secs(30);
/// How long `services.list` is waited on, inline on the broker's loop: it
/// reads a few small files.
pub const LIST_WAIT: Duration = Duration::from_secs(5);
/// How often a wipe the signed-out lamp could not make is tried again.
pub const WIPE_AGAIN: Duration = Duration::from_secs(30);

/// The most requests under way at once; past it, `failure` [`BUSY`].
const MOST_AT_ONCE: usize = 4;
/// The most bytes read of the backend's answer: a connector is small.
const BODY_MOST: usize = 256 * 1024;
/// The longest failure code passed on from the admin socket.
const FAILURE_CODE_MOST: usize = 32;
/// The most bytes read of the admin socket's line.
const REPLY_MOST: u64 = 64 * 1024;
/// The longest reason passed on from the backend or the admin socket.
const REASON_MOST: usize = 300;

/// Whether the switch is on: both `LAMP_CLOUD` and `LAMP_SERVICES` are `on`.
pub fn gates(cloud: Option<&str>, services: Option<&str>) -> bool {
    cloud == Some("on") && services == Some("on")
}

/// [`gates`] as the environment says.
pub fn on_from_environment() -> bool {
    gates(
        std::env::var("LAMP_CLOUD").ok().as_deref(),
        std::env::var(ENVIRONMENT).ok().as_deref(),
    )
}

/// What a connector kind asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    Set,
    Remove,
}

/// What `kind` asks and of which code, when it is a connector kind at all;
/// the code is not yet checked against [`CODES`].
pub fn connector_kind(kind: &str) -> Option<(Asked, &str)> {
    if let Some(code) = kind.strip_prefix(SET_PREFIX) {
        return Some((Asked::Set, code));
    }
    kind.strip_prefix(REMOVE_PREFIX)
        .map(|code| (Asked::Remove, code))
}

/// Whether `code` is one of [`CODES`].
pub fn known(code: &str) -> bool {
    CODES.contains(&code)
}

/// Where a privacy kind's data is fetched: the backend's.
pub trait Fetch: Send + Sync {
    /// The inner `data` the backend keeps for `kind`, or why not, said
    /// plainly and never with the backend's body.
    fn fetch(&self, kind: &str) -> Result<Value, String>;
}

/// The backend, reached with the lamp's key as the ping is.
pub struct Backend {
    /// The account's base with a trailing `/v1` cut off, as the ping's.
    pub base: String,
    pub key: String,
    pub device_id: String,
}

impl std::fmt::Debug for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Backend")
            .field("base", &self.base)
            .field("key", &"<hidden>")
            .field("device_id", &self.device_id)
            .finish()
    }
}

impl Backend {
    /// The account's base and the lamp's key; none without either.
    pub fn of(account: &Account, key: &str) -> Option<Self> {
        let base = account.llm_base_url.trim().trim_end_matches('/');
        let base = base.strip_suffix("/v1").unwrap_or(base);
        let key = key.trim();
        (!base.is_empty() && !key.is_empty()).then(|| Self {
            base: base.to_owned(),
            key: key.to_owned(),
            device_id: account.device_id.trim().to_owned(),
        })
    }

    /// V1's `fetchPrivacyData` URL for `kind`.
    pub fn url(&self, kind: &str) -> Result<reqwest::Url, String> {
        let mut url = reqwest::Url::parse(&format!("{}{FETCH_PATH}", self.base))
            .map_err(|_| "fetch privacy data: the backend's address can't be read".to_owned())?;
        url.query_pairs_mut().append_pair("kind", kind);
        Ok(url)
    }
}

impl Fetch for Backend {
    fn fetch(&self, kind: &str) -> Result<Value, String> {
        let url = self.url(kind)?;
        fn unreached<E>(_: E) -> String {
            "fetch privacy data: the backend can't be reached now".to_owned()
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(unreached)?;
        let (status, body) = runtime.block_on(async {
            let client = reqwest::Client::builder()
                .tls_backend_preconfigured(crate::beat::tls()?)
                .https_only(!cfg!(test))
                .redirect(reqwest::redirect::Policy::none())
                .timeout(FETCH_WITHIN)
                .build()
                .map_err(unreached)?;
            let mut request = client
                .get(url)
                .header("authorization", format!("Bearer {}", self.key))
                .header("accept", "application/json");
            if !self.device_id.is_empty() {
                request = request.header("x-device-id", &self.device_id);
            }
            let mut response = request.send().await.map_err(unreached)?;
            let status = response.status().as_u16();
            let mut body = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(unreached)? {
                body.extend_from_slice(&chunk);
                if body.len() > BODY_MOST {
                    return Err("fetch privacy data: the backend's answer is too large".to_owned());
                }
            }
            Ok::<_, String>((status, body))
        })?;
        fetched(kind, status, &body)
    }
}

/// A reason from elsewhere, on one line and bounded.
fn plain(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(REASON_MOST)
        .collect()
}

/// V1's handling of the backend's answer (`fetchPrivacyData`): the inner
/// `data` of `{status, message, data: {cmd, kind, data}}`, or why not. The
/// body is never in the reason: a refusal says only its HTTP status.
pub fn fetched(kind: &str, status: u16, body: &[u8]) -> Result<Value, String> {
    let why = |text: &str| format!("fetch privacy data: {text}");
    if status != 200 {
        return Err(why(&format!("http {status}")));
    }
    let answer: Value =
        serde_json::from_slice(body).map_err(|_| why("decode response: not JSON"))?;
    if answer.get("status").and_then(Value::as_i64) != Some(1) {
        return Err(
            match answer
                .get("message")
                .and_then(Value::as_str)
                .filter(|m| !m.is_empty())
            {
                Some(message) => why(&format!("backend rejected: {}", plain(message))),
                None => why(&format!(
                    "backend status={}",
                    answer.get("status").and_then(Value::as_i64).unwrap_or(0)
                )),
            },
        );
    }
    let envelope = answer.get("data");
    let data = envelope
        .and_then(|envelope| envelope.get("data"))
        .filter(|data| !data.is_null())
        .ok_or_else(|| why("backend response missing data block"))?;
    let echoed = envelope
        .and_then(|envelope| envelope.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !echoed.is_empty() && echoed != kind {
        return Err(why(&format!(
            "backend returned kind={} for requested kind={kind}",
            plain(echoed)
        )));
    }
    Ok(data.clone())
}

/// V1's checks of a fetched connector (`runConnectorSet`), and that it is
/// for the service the kind names. The reasons name fields, never values.
pub fn checked(kind: &str, code: &str, connector: &Value) -> Result<(), String> {
    let fields = connector
        .as_object()
        .ok_or_else(|| format!("invalid {kind} data"))?;
    let text = |field: &str| fields.get(field).and_then(Value::as_str).unwrap_or("");
    if text("connector").is_empty() {
        return Err("connector is required".into());
    }
    if text("auth_type").is_empty() {
        return Err("auth_type is required".into());
    }
    if text("connector") != code {
        return Err(format!("the connector fetched is not {code}'s"));
    }
    Ok(())
}

/// The admin socket: `line` out, one JSON line back within `wait`. An
/// `"ok": true` reply as it is; `"ok": false` its plain `error`.
pub fn ask(socket: &Path, line: &Value, wait: Duration) -> Result<Value, String> {
    ask_coded(socket, line, wait).map_err(|(why, _)| why)
}

/// A failure's code from the admin socket (`relink`, `unreached`,
/// `not_allowed`): lower-case letters and `_` only, at most
/// [`FAILURE_CODE_MOST`]; any other is dropped.
fn failure_code(reply: &Value) -> Option<String> {
    reply
        .get("code")
        .and_then(Value::as_str)
        .filter(|code| {
            !code.is_empty()
                && code.len() <= FAILURE_CODE_MOST
                && code
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
        })
        .map(str::to_owned)
}

/// [`ask`], a refusal with its code, when the socket gave one.
fn ask_coded(
    socket: &Path,
    line: &Value,
    wait: Duration,
) -> Result<Value, (String, Option<String>)> {
    let unanswered = |why: &str| (why.to_owned(), None);
    let mut stream = UnixStream::connect(socket).map_err(|_| unanswered(NOT_RUNNING))?;
    stream
        .set_read_timeout(Some(wait))
        .and_then(|()| stream.set_write_timeout(Some(wait)))
        .and_then(|()| stream.write_all(format!("{line}\n").as_bytes()))
        .map_err(|_| unanswered(NOT_RUNNING))?;
    let mut reply = String::new();
    BufReader::new(stream.take(REPLY_MOST))
        .read_line(&mut reply)
        .map_err(|_| unanswered(NO_ANSWER))?;
    let reply: Value = serde_json::from_str(reply.trim()).map_err(|_| unanswered(NO_ANSWER))?;
    match reply.get("ok").and_then(Value::as_bool) {
        Some(true) => Ok(reply),
        Some(false) => Err((
            reply
                .get("error")
                .and_then(Value::as_str)
                .map(plain)
                .filter(|why| !why.is_empty())
                .unwrap_or_else(|| "the lamp's services refused it".to_owned()),
            failure_code(&reply),
        )),
        None => Err(unanswered(NO_ANSWER)),
    }
}

/// The services' requests and their results, waiting to be said.
pub struct Services {
    pub socket: PathBuf,
    fetch: Arc<dyn Fetch>,
    /// Requests under way, at most [`MOST_AT_ONCE`].
    under_way: Arc<AtomicUsize>,
    done: Arc<Mutex<Vec<Outcome>>>,
}

impl std::fmt::Debug for Services {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Services")
            .field("socket", &self.socket)
            .finish_non_exhaustive()
    }
}

impl Services {
    pub fn new(socket: impl Into<PathBuf>, fetch: Arc<dyn Fetch>) -> Arc<Self> {
        Arc::new(Self {
            socket: socket.into(),
            fetch,
            under_way: Arc::default(),
            done: Arc::default(),
        })
    }

    /// The product's: [`ADMIN_SOCKET`] and the backend, with the switch;
    /// none without it, or without the account's base or the lamp's key.
    pub fn product(account: &Account, key: &str) -> Option<Arc<Self>> {
        if !on_from_environment() {
            return None;
        }
        let backend = Backend::of(account, key)?;
        Some(Self::new(ADMIN_SOCKET, Arc::new(backend)))
    }

    /// Whether `lamp-services` is here to ask: `can` names [`CAN`] and
    /// [`LIST_KIND`] only then.
    pub fn here(&self) -> bool {
        self.socket.exists()
    }

    /// Takes a `connector.set.<code>` or `connector.remove.<code>`: on a
    /// thread of its own, its result kept for [`Self::take`]; or why not,
    /// at once.
    pub fn start(&self, kind: &str) -> Result<(), String> {
        let (asked, code) = connector_kind(kind).ok_or_else(|| crate::wire::not_here(kind))?;
        if !known(code) {
            return Err(NOT_KNOWN.to_owned());
        }
        if self
            .under_way
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MOST_AT_ONCE).then_some(n + 1)
            })
            .is_err()
        {
            return Err(BUSY.to_owned());
        }
        let (kind, code) = (kind.to_owned(), code.to_owned());
        let (socket, fetch) = (self.socket.clone(), self.fetch.clone());
        let (under_way, done) = (self.under_way.clone(), self.done.clone());
        std::thread::spawn(move || {
            let (result, failure) = match asked {
                Asked::Set => match set(&*fetch, &socket, &kind, &code) {
                    Ok(done) => (Ok(done), None),
                    Err((why, failure)) => (Err(why), failure),
                },
                Asked::Remove => (
                    ask(&socket, &json!({"op": "remove", "code": code}), ADMIN_WAIT)
                        .map(|_| Some(json!({"connector": code}))),
                    None,
                ),
            };
            under_way.fetch_sub(1, Ordering::AcqRel);
            if let Ok(mut done) = done.lock() {
                done.push(Outcome {
                    kind,
                    result,
                    request_id: None,
                    code: failure,
                });
            }
        });
        Ok(())
    }

    /// `services.list`: the services linked now, by code and account,
    /// asked inline. Only those fields are passed on.
    pub fn list(&self) -> Result<Value, String> {
        let reply = ask(&self.socket, &json!({"op": "list"}), LIST_WAIT)?;
        let connected: Vec<Value> = reply
            .get("connected")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                let code = entry.get("code").and_then(Value::as_str)?;
                let mut shown = Map::new();
                shown.insert("code".into(), json!(code));
                if let Some(account) = entry.get("account").and_then(Value::as_str) {
                    shown.insert("account".into(), json!(account));
                }
                if let Some(expired) = entry.get("expired").and_then(Value::as_bool) {
                    shown.insert("expired".into(), json!(expired));
                }
                Some(Value::Object(shown))
            })
            .collect();
        Ok(json!({ "connected": connected }))
    }

    /// Every token wiped: the lamp left its account.
    pub fn wipe(&self) -> Result<(), String> {
        ask(&self.socket, &json!({"op": "wipe"}), ADMIN_WAIT).map(|_| ())
    }

    /// Results handed to the broker and lost with the connection before it
    /// acknowledged them: said again, first, on the next one.
    pub fn owe(&self, again: Vec<Outcome>) {
        if again.is_empty() {
            return;
        }
        if let Ok(mut done) = self.done.lock() {
            let later = std::mem::take(&mut *done);
            done.extend(again);
            done.extend(later);
        }
    }

    /// The results come since the last call, oldest first.
    pub fn take(&self) -> Vec<Outcome> {
        self.done
            .lock()
            .map(|mut done| std::mem::take(&mut *done))
            .unwrap_or_default()
    }
}

/// A `connector.set.<code>`: the connector fetched, checked and handed to
/// the admin socket; the reply's `{connector, scopes}`, or why not with the
/// socket's failure code (said in `data.code`), when it gave one.
fn set(
    fetch: &dyn Fetch,
    socket: &Path,
    kind: &str,
    code: &str,
) -> Result<Option<Value>, (String, Option<String>)> {
    let connector = fetch.fetch(kind).map_err(|why| (why, None))?;
    checked(kind, code, &connector).map_err(|why| (why, None))?;
    let reply = ask_coded(
        socket,
        &json!({"op": "set", "code": code, "connector": connector}),
        ADMIN_WAIT,
    )?;
    let scopes: Vec<Value> = reply
        .get("scopes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|scope| scope.is_string())
        .cloned()
        .collect();
    Ok(Some(json!({"connector": code, "scopes": scopes})))
}

/// Wipes every token once the lamp is signed out (`crate::signed_out`,
/// watched whenever `LAMP_SERVICES=on`, whatever `LAMP_SIGNED_OUT` says):
/// the lamp was removed from its account. Tried again
/// every [`WIPE_AGAIN`] while it fails and the lamp stays signed out; once
/// more after each new spell. Never returns while `since` is open.
pub async fn wipe_when_signed_out(
    mut since: watch::Receiver<Option<u64>>,
    services: Arc<Services>,
    say: &mut impl FnMut(&str),
) {
    loop {
        // A spell begins (or is under way at start).
        if since.wait_for(Option::is_some).await.is_err() {
            return std::future::pending().await;
        }
        loop {
            let asked = services.clone();
            let wiped = tokio::task::spawn_blocking(move || asked.wipe())
                .await
                .unwrap_or_else(|_| Err(NO_ANSWER.to_owned()));
            match wiped {
                Ok(()) => {
                    say("signed out: the services' tokens are wiped");
                    break;
                }
                Err(why) => say(&format!(
                    "signed out: the services' tokens could not be wiped ({why}), tried again"
                )),
            }
            tokio::select! {
                () = tokio::time::sleep(WIPE_AGAIN) => {}
                ended = since.wait_for(Option::is_none) => {
                    if ended.is_err() {
                        return std::future::pending().await;
                    }
                    break;
                }
            }
            if since.borrow().is_none() {
                break;
            }
        }
        // Wait for this spell to end before the next.
        if since.wait_for(Option::is_none).await.is_err() {
            return std::future::pending().await;
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::time::Instant;

    /// A token no reply, error, journal line or `Debug` may hold.
    pub const TOKEN: &str = "ya29.SECRET-token-never-said";

    /// The backend in memory: its answer, and the kinds asked of it.
    pub struct FakeFetch {
        pub answer: Result<Value, String>,
        pub asked: Mutex<Vec<String>>,
    }

    impl Fetch for FakeFetch {
        fn fetch(&self, kind: &str) -> Result<Value, String> {
            self.asked.lock().unwrap().push(kind.to_owned());
            self.answer.clone()
        }
    }

    pub fn connector(code: &str) -> Value {
        json!({"connector": code, "auth_type": "oauth2", "access_token": TOKEN,
            "refresh_token": format!("{TOKEN}-refresh"), "scopes": ["read"],
            "user_email": "a@b.c"})
    }

    /// A fake `lamp-services`: each connection's line kept, `reply` sent.
    pub struct FakeAdmin {
        pub dir: tempfile::TempDir,
        pub lines: Arc<Mutex<Vec<String>>>,
    }

    impl FakeAdmin {
        pub fn socket(&self) -> PathBuf {
            self.dir.path().join("a.sock")
        }
    }

    pub fn admin(reply: &'static str) -> FakeAdmin {
        let dir = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(dir.path().join("a.sock")).unwrap();
        let lines = Arc::new(Mutex::new(Vec::new()));
        let kept = lines.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut line = String::new();
                let _ = BufReader::new(&stream).read_line(&mut line);
                kept.lock().unwrap().push(line);
                let _ = stream.write_all(format!("{reply}\n").as_bytes());
            }
        });
        FakeAdmin { dir, lines }
    }

    pub fn services(socket: PathBuf, answer: Result<Value, String>) -> Arc<Services> {
        Services::new(
            socket,
            Arc::new(FakeFetch {
                answer,
                asked: Mutex::default(),
            }),
        )
    }

    /// The results within a few seconds, at least `count` of them.
    pub fn results(services: &Services, count: usize) -> Vec<Outcome> {
        let by = Instant::now() + Duration::from_secs(5);
        let mut all = Vec::new();
        while all.len() < count && Instant::now() < by {
            all.extend(services.take());
            std::thread::sleep(Duration::from_millis(10));
        }
        all
    }

    #[test]
    fn the_switch_needs_both_words_on() {
        assert!(gates(Some("on"), Some("on")));
        for (cloud, services) in [
            (None, Some("on")),
            (Some("on"), None),
            (Some("on"), Some("1")),
            (Some("off"), Some("on")),
        ] {
            assert!(!gates(cloud, services), "{cloud:?} {services:?}");
        }
    }

    #[test]
    fn connector_kinds_name_their_service() {
        assert_eq!(
            connector_kind("connector.set.gmail"),
            Some((Asked::Set, "gmail"))
        );
        assert_eq!(
            connector_kind("connector.remove.github"),
            Some((Asked::Remove, "github"))
        );
        assert_eq!(
            connector_kind("connector.set.gmail-app"),
            Some((Asked::Set, "gmail-app"))
        );
        assert!(known("gmail-app"));
        assert!(known("spotify"));
        assert_eq!(connector_kind("services.list"), None);
        assert!(CODES.iter().all(|code| known(code)));
        for other in ["dropbox", "", "GMAIL", "gmail.x", "../gmail", "gmail_app"] {
            assert!(!known(other), "{other}");
        }
    }

    /// V1's answer handling: the inner `data`; a refusal by its status
    /// only, never its body; another kind echoed is refused.
    #[test]
    fn the_backends_answer_is_read_as_v1_reads_it() {
        let kind = "connector.set.gmail";
        let body = json!({"status": 1, "data": {"cmd": "data", "kind": kind,
            "data": connector("gmail")}});
        assert_eq!(
            fetched(kind, 200, body.to_string().as_bytes()),
            Ok(connector("gmail"))
        );
        let refused = fetched(kind, 401, format!("{{\"leak\":\"{TOKEN}\"}}").as_bytes());
        assert_eq!(refused, Err("fetch privacy data: http 401".to_owned()));
        assert_eq!(
            fetched(kind, 200, br#"{"status":0,"message":"no such message"}"#),
            Err("fetch privacy data: backend rejected: no such message".to_owned())
        );
        assert_eq!(
            fetched(kind, 200, br#"{"status":0}"#),
            Err("fetch privacy data: backend status=0".to_owned())
        );
        assert_eq!(
            fetched(
                kind,
                200,
                br#"{"status":1,"data":{"kind":"connector.set.gmail"}}"#
            ),
            Err("fetch privacy data: backend response missing data block".to_owned())
        );
        let other = json!({"status": 1, "data": {"kind": "connector.set.notion",
            "data": connector("notion")}});
        let other = fetched(kind, 200, other.to_string().as_bytes()).unwrap_err();
        assert!(other.contains("kind=connector.set.notion"), "{other}");
        assert!(!other.contains(TOKEN));
        assert!(
            fetched(kind, 200, TOKEN.as_bytes())
                .unwrap_err()
                .contains("not JSON")
        );
    }

    #[test]
    fn a_fetched_connector_is_checked_by_its_fields_never_said() {
        let kind = "connector.set.gmail";
        assert_eq!(checked(kind, "gmail", &connector("gmail")), Ok(()));
        for (bad, why) in [
            (json!(TOKEN), "invalid connector.set.gmail data"),
            (
                json!({"auth_type": "pat", "api_key": TOKEN}),
                "connector is required",
            ),
            (
                json!({"connector": "gmail", "api_key": TOKEN}),
                "auth_type is required",
            ),
            (connector("notion"), "the connector fetched is not gmail's"),
        ] {
            assert_eq!(checked(kind, "gmail", &bad), Err(why.to_owned()));
        }
    }

    /// The real fetch: V1's URL above the `/v1` base, the lamp's key as
    /// the bearer and its id, and a redirect not followed.
    #[test]
    fn the_backend_is_asked_as_v1_asks_it() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = seen.clone();
        std::thread::spawn(move || {
            let replies = [
                format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\ncontent-length: {}\r\n\r\n{}",
                    json!({"status": 1, "data": {"kind": "connector.set.github", "data": connector("github")}}).to_string().len(),
                    json!({"status": 1, "data": {"kind": "connector.set.github", "data": connector("github")}})
                ),
                "HTTP/1.1 302 Found\r\nlocation: http://elsewhere.test/\r\nconnection: close\r\ncontent-length: 0\r\n\r\n".to_owned(),
            ];
            for (stream, reply) in listener.incoming().zip(replies) {
                let mut stream = stream.unwrap();
                let mut head = String::new();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    head.push_str(&line);
                }
                kept.lock().unwrap().push(head.to_ascii_lowercase());
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        let account: Account = serde_json::from_value(json!({
            "device_id": "d-42", "llm_base_url": format!("http://{address}/v1/")
        }))
        .unwrap();
        let backend = Backend::of(&account, " lamp-key ").unwrap();
        assert!(!format!("{backend:?}").contains("lamp-key"));
        assert_eq!(
            backend.fetch("connector.set.github"),
            Ok(connector("github"))
        );
        let head = seen.lock().unwrap()[0].clone();
        assert!(
            head.starts_with("get /devices/get-message?kind=connector.set.github http/1.1"),
            "{head}"
        );
        assert!(head.contains("authorization: bearer lamp-key"), "{head}");
        assert!(head.contains("x-device-id: d-42"), "{head}");
        assert_eq!(
            backend.fetch("connector.set.github"),
            Err("fetch privacy data: http 302".to_owned())
        );
        assert!(Backend::of(&account, "").is_none());
    }

    /// A set: fetched, handed to the admin socket with its connector, and
    /// `{connector, scopes}` after; the token in the socket's line only.
    #[test]
    fn a_set_hands_the_fetched_connector_to_the_admin_socket() {
        let admin = admin(r#"{"ok":true,"connector":"gmail","scopes":["mail.read"]}"#);
        let services = services(admin.socket(), Ok(connector("gmail")));
        services.start("connector.set.gmail").unwrap();
        let done = results(&services, 1);
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].kind, "connector.set.gmail");
        assert_eq!(
            done[0].result,
            Ok(Some(json!({"connector": "gmail", "scopes": ["mail.read"]})))
        );
        let lines = admin.lines.lock().unwrap().clone();
        let line: Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(
            line,
            json!({"op": "set", "code": "gmail", "connector": connector("gmail")})
        );
        assert!(!format!("{services:?} {done:?}").contains(TOKEN));
    }

    #[test]
    fn a_refusal_and_a_failed_fetch_say_why_plainly() {
        let admin = admin(r#"{"ok":false,"error":"Gmail refused the app password"}"#);
        let refusing = services(admin.socket(), Ok(connector("gmail")));
        refusing.start("connector.set.gmail").unwrap();
        let done = results(&refusing, 1);
        assert_eq!(
            done[0].result,
            Err("Gmail refused the app password".to_owned())
        );
        let unfetched = services(
            admin.socket(),
            Err("fetch privacy data: http 404".to_owned()),
        );
        unfetched.start("connector.set.notion").unwrap();
        let done = results(&unfetched, 1);
        assert_eq!(
            done[0].result,
            Err("fetch privacy data: http 404".to_owned())
        );
        // Not asked of the socket after a failed fetch.
        assert_eq!(admin.lines.lock().unwrap().len(), 1);
        assert_eq!(done[0].code, None);
        let gone = services(admin.dir.path().join("none.sock"), Ok(connector("figma")));
        gone.start("connector.set.figma").unwrap();
        assert_eq!(results(&gone, 1)[0].result, Err(NOT_RUNNING.to_owned()));
    }

    /// A refusal's code from `lamp-services` (Spotify's `relink`, #5460)
    /// is said in `data.code`; one not in its shape is dropped.
    #[test]
    fn a_refusals_code_is_passed_on() {
        let admin = admin(
            r#"{"ok":false,"error":"Spotify refused the lamp's link: link it again in the app","code":"relink"}"#,
        );
        let services = services(admin.socket(), Ok(connector("spotify")));
        services.start("connector.set.spotify").unwrap();
        let done = results(&services, 1);
        assert_eq!(
            done[0].result,
            Err("Spotify refused the lamp's link: link it again in the app".to_owned())
        );
        assert_eq!(done[0].code.as_deref(), Some("relink"));
        for (reply, kept) in [
            (json!({"ok": false, "code": "Relink"}), None),
            (json!({"ok": false, "code": "x".repeat(33)}), None),
            (json!({"ok": false, "code": 5}), None),
            (
                json!({"ok": false, "code": "not_allowed"}),
                Some("not_allowed"),
            ),
        ] {
            assert_eq!(failure_code(&reply).as_deref(), kept, "{reply}");
        }
    }

    #[test]
    fn remove_list_and_wipe_ask_their_ops() {
        let admin = admin(
            r#"{"ok":true,"connected":[{"code":"gmail","account":"a@b.c","expired":false,"token":"x"}]}"#,
        );
        let services = services(admin.socket(), Err("never fetched".into()));
        services.start("connector.remove.linear").unwrap();
        assert_eq!(
            results(&services, 1)[0].result,
            Ok(Some(json!({"connector": "linear"})))
        );
        assert_eq!(
            services.list(),
            Ok(json!({"connected": [{"code": "gmail", "account": "a@b.c", "expired": false}]}))
        );
        assert_eq!(services.wipe(), Ok(()));
        let ops: Vec<Value> = admin
            .lines
            .lock()
            .unwrap()
            .iter()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(
            ops,
            [
                json!({"op": "remove", "code": "linear"}),
                json!({"op": "list"}),
                json!({"op": "wipe"})
            ]
        );
    }

    #[test]
    fn an_unknown_service_is_refused_at_once() {
        let services = services(PathBuf::from("/nowhere/a.sock"), Ok(connector("dropbox")));
        for kind in [
            "connector.set.dropbox",
            "connector.remove.",
            "connector.set.GMAIL",
        ] {
            assert_eq!(services.start(kind), Err(NOT_KNOWN.to_owned()), "{kind}");
        }
        assert!(services.take().is_empty());
    }

    #[test]
    fn signing_out_wipes_every_token_once_a_spell() {
        let admin = admin(r#"{"ok":true}"#);
        let services = services(admin.socket(), Err("never fetched".into()));
        let (tx, rx) = watch::channel(None);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let said = Arc::new(Mutex::new(Vec::<String>::new()));
        let kept = said.clone();
        runtime.block_on(async move {
            let wiping = async move {
                let mut say = |line: &str| kept.lock().unwrap().push(line.to_owned());
                wipe_when_signed_out(rx, services, &mut say).await;
            };
            let spells = async {
                let wiped = |n: usize| {
                    let lines = admin.lines.clone();
                    async move {
                        for _ in 0..500 {
                            if lines.lock().unwrap().len() >= n {
                                return;
                            }
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        }
                        panic!("no wipe");
                    }
                };
                tokio::time::sleep(Duration::from_millis(50)).await;
                assert!(admin.lines.lock().unwrap().is_empty(), "signed in");
                tx.send_replace(Some(1));
                wiped(1).await;
                tx.send_replace(Some(1));
                tokio::time::sleep(Duration::from_millis(50)).await;
                assert_eq!(admin.lines.lock().unwrap().len(), 1, "the same spell");
                tx.send_replace(None);
                tokio::time::sleep(Duration::from_millis(20)).await;
                tx.send_replace(Some(2));
                wiped(2).await;
                admin.lines.lock().unwrap().clone()
            };
            tokio::select! {
                () = wiping => unreachable!(),
                lines = spells => {
                    for line in lines {
                        assert_eq!(serde_json::from_str::<Value>(&line).unwrap(), json!({"op": "wipe"}));
                    }
                }
            }
        });
        assert!(
            said.lock()
                .unwrap()
                .iter()
                .all(|line| line.contains("wiped"))
        );
    }
}
