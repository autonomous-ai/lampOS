//! Signing the lamp's Harness in from the owner's app (#5198):
//! `harness.sign_in.start` (no method, or `{"method":"code"}`: Harness's
//! 8-digit code; `{"method":"qr"}`: Harness's QR link, which the Harness
//! phone app scans; any other is `method_not_supported`, as is `qr` on a
//! lamp-tasks that doesn't take it),
//! `harness.sign_in.state` and `harness.sign_in.answer`
//! (`{"id":N,"answer":"yes"}` or `"no"`), each handed to lamp-tasks' own
//! route for the app (`serve --app-socket`, [`SOCKET`]) as one JSON line
//! (`app_sign_in_start`, `app_sign_in_state`, `app_sign_in_answer`), as
//! `guard.on` is handed to lamp-skills. Off unless
//! `LAMP_TASKS_SIGN_IN_APP=on`; lamp-tasks needs `--sign-in-by-code` and
//! `--app-socket` too, so this is not the only gate.
//!
//! While the switch is on, lamp-cloud also follows the sign-in ([`Follow`])
//! and pushes its view unasked on `fd_channel`, as `harness.sign_in.state`,
//! whenever its `revision` (or lamp-tasks' `instance_id`, or `signed_in`)
//! moves: every second while one is open, every [`IDLE_EVERY`] otherwise.
//!
//! The state's `signed_in` (`yes`, `offline` or `no`, #5524) is whether
//! Harness is signed in, as lamp-tasks last read it from Harness itself
//! (`serve --set-up-later`), so the app's Harness row is right after a
//! restart too, when no sign-in is open and the view says `none`. Before
//! lamp-tasks' first read it is absent.
//!
//! **The code, the QR link and the email never go on the broker** (the
//! PM's privacy rule on #5198; [`SECRET_FIELDS`]). A view replied or pushed
//! on `fd_channel` holds the state fields only ([`BROKER_FIELDS`]). The code
//! or the QR link (each with its `expires_at`) and an app-started question's
//! email are handed to the backend over HTTPS ([`HandOffs`], [`Secrets`]),
//! and taken back there when the sign-in ends. No journal line, error or
//! `Debug` here holds any of them. The QR rides the same switch, with no
//! switch of its own.
//!
//! lamp-tasks decides everything: who started a sign-in, which surface may
//! answer it, and the answer's window.
use serde_json::{Map, Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

/// The switch, in lamp-cloud's unit (and lamp-voice's): `on` exactly.
pub const ENVIRONMENT: &str = "LAMP_TASKS_SIGN_IN_APP";

/// The backend's route for the hand-off (Backend2 on #5198), above the
/// same base as `services`' `/devices/get-message` (the account's, `/v1`
/// cut off), with the same lamp key as the bearer. Each put replaces the
/// last; a delete is `?id=N`, answered 200 also when already gone.
pub const ROUTE: &str = "/devices/harness/sign-in";

/// lamp-tasks' socket for the app's route, in its runtime folder.
pub const SOCKET: &str = "/run/lampos-tasks/app.sock";

/// The feature's word in `system.info`'s `can`, beside its kinds: an app
/// shows the sign-in screen only on a lamp that names it.
pub const CAN: &str = "harness.sign_in";

/// The kind pushed unasked when the view changes, as the state query's.
pub const PUSHED: &str = "harness.sign_in.state";

/// How often an open sign-in is looked at for a change to push.
pub const OPEN_EVERY: Duration = Duration::from_secs(1);
/// How often, with none open: a sign-in started by voice is shown soon.
pub const IDLE_EVERY: Duration = Duration::from_secs(3);

/// The three kinds, in `system.info`'s `can` order.
pub const KINDS: [&str; 3] = [
    "harness.sign_in.start",
    "harness.sign_in.state",
    "harness.sign_in.answer",
];

/// lamp-tasks answers within its own 1.2 s; the broker's loop waits on
/// it, so it is bounded short.
pub const WAIT: Duration = Duration::from_secs(2);

/// How long one HTTPS hand-off may take, on its own thread.
pub const HAND_OFF_WITHIN: Duration = Duration::from_secs(10);

/// Hand-offs waiting for that thread at most; more are dropped and said.
const HAND_OFFS_QUEUED: usize = 8;

/// A method the lamp does not take: neither `code` nor `qr`, or `qr` on a
/// lamp-tasks from before it (#5198).
pub const METHOD_NOT_SUPPORTED: &str = "method_not_supported";

/// The backend's name for the QR link in a hand-off's body, beside `id` and
/// `expires_at` as the code's: `{id, qr_url, expires_at}` (Backend2 on
/// #5198, live 11 Nov).
pub const QR_URL_FIELD: &str = "qr_url";

/// The Harness sign-in page: the only address a QR link is handed for.
/// The backend refuses any other with a 400 (Backend2 on #5198), so the
/// lamp checks it first and never sends one.
pub const QR_URL_PREFIX: &str = "https://harness.autonomous.ai/signin#k=";

/// The most of a QR link's key taken.
const MOST_QR_KEY_BYTES: usize = 512;

/// Whether `url` is the Harness sign-in page with a key: [`QR_URL_PREFIX`]
/// and 1 to 512 bytes of letters, digits, `-`, `_`, `.` or `~` (#5198).
pub fn qr_url_ok(url: &str) -> bool {
    url.strip_prefix(QR_URL_PREFIX).is_some_and(|key| {
        !key.is_empty()
            && key.len() <= MOST_QR_KEY_BYTES
            && key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~'))
    })
}

/// lamp-tasks' view's secret fields: never on the broker, never in a
/// journal line, handed to the backend over HTTPS only.
pub const SECRET_FIELDS: [&str; 3] = ["code", "qr_url", "email"];

/// What the app is told when lamp-tasks can't be reached or answers what
/// can't be read: nothing was started or answered.
pub const UNREACHABLE: &str = "unreachable";

/// The most of lamp-tasks' answer read.
const MOST_REPLY_BYTES: u64 = 16 * 1024;

/// The view's fields the app is given on the broker; any other (the code,
/// the email, lamp-tasks' store id and revision) stays off it.
pub const BROKER_FIELDS: [&str; 16] = [
    "sign_in",
    "signed_in",
    "id",
    "revision",
    "instance_id",
    "expires_at",
    "expires_in",
    "message",
    "already_signed_in",
    "by",
    "method",
    "answer_here",
    "answer_within",
    "answer",
    "result",
    "error",
];

/// Whether the switch's value turns it on: `on` exactly.
pub fn switched_on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The secret parts' way to the backend, over HTTPS (#5198).
pub trait Secrets: Send + Sync {
    /// `{"id","code","expires_at"}`, `{"id",`[`QR_URL_FIELD`]`,"expires_at"}`
    /// or `{"id","email"}`. The reason for a failure never holds the body.
    fn put(&self, body: &Value) -> Result<(), String>;
    /// The sign-in `id`'s parts taken back, best effort.
    fn delete(&self, id: u64) -> Result<(), String>;
}

/// The backend, with the lamp's key, as `services`' fetch.
pub struct Https {
    pub url: String,
    pub key: String,
    pub device_id: String,
}

impl std::fmt::Debug for Https {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Https")
            .field("url", &self.url)
            .field("key", &"<hidden>")
            .field("device_id", &self.device_id)
            .finish()
    }
}

impl Https {
    /// The account's base and the lamp's key, as `services`' fetch; none
    /// without either.
    pub fn of(account: &crate::account::Account, key: &str) -> Option<Self> {
        let backend = crate::services::Backend::of(account, key)?;
        Some(Self {
            url: format!("{}{ROUTE}", backend.base),
            key: backend.key,
            device_id: backend.device_id,
        })
    }

    /// One call, tried again once on a 5xx or no answer (Backend2): the
    /// reason for a failure is its status's word, never the body.
    fn send(&self, method: reqwest::Method, url: &str, body: Option<&Value>) -> Result<(), String> {
        match self.send_once(method.clone(), url, body) {
            Err(Failed::Again(_)) => self.send_once(method, url, body),
            done => done,
        }
        .map_err(|failed| match failed {
            Failed::Again(why) | Failed::Final(why) => why,
        })
    }

    fn send_once(
        &self,
        method: reqwest::Method,
        url: &str,
        body: Option<&Value>,
    ) -> Result<(), Failed> {
        let status = self
            .call(method, url, body)
            .map_err(|why| Failed::Again(why.to_owned()))?;
        match status {
            200..=299 => Ok(()),
            401 => Err(Failed::Final(
                "the backend refused the lamp's key (401)".into(),
            )),
            400 => Err(Failed::Final("the backend said bad_request (400)".into())),
            // An id lower than the newest, or the sign-in's 11 minutes up.
            409 => Err(Failed::Final(
                "the backend said sign_in_old_id (409)".into(),
            )),
            503 => Err(Failed::Again("the backend said unavailable (503)".into())),
            500..=599 => Err(Failed::Again(format!("the backend answered {status}"))),
            other => Err(Failed::Final(format!("the backend answered {other}"))),
        }
    }

    fn call(
        &self,
        method: reqwest::Method,
        url: &str,
        body: Option<&Value>,
    ) -> Result<u16, &'static str> {
        fn unreached<E>(_: E) -> &'static str {
            "the backend can't be reached now"
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(unreached)?;
        let status = runtime.block_on(async {
            let client = reqwest::Client::builder()
                .tls_backend_preconfigured(crate::beat::tls().map_err(unreached)?)
                .https_only(!cfg!(test))
                .redirect(reqwest::redirect::Policy::none())
                .timeout(HAND_OFF_WITHIN)
                .build()
                .map_err(unreached)?;
            let mut request = client
                .request(method, url)
                .header("authorization", format!("Bearer {}", self.key))
                .header("accept", "application/json");
            if !self.device_id.is_empty() {
                request = request.header("x-device-id", &self.device_id);
            }
            if let Some(body) = body {
                request = request
                    .header("content-type", "application/json")
                    .body(body.to_string());
            }
            let response = request.send().await.map_err(unreached)?;
            Ok::<_, &'static str>(response.status().as_u16())
        })?;
        Ok(status)
    }
}

/// A call that failed, and whether it is worth one more try.
enum Failed {
    Again(String),
    Final(String),
}

impl Secrets for Https {
    fn put(&self, body: &Value) -> Result<(), String> {
        self.send(reqwest::Method::PUT, &self.url, Some(body))
    }
    fn delete(&self, id: u64) -> Result<(), String> {
        self.send(
            reqwest::Method::DELETE,
            &format!("{}?id={id}", self.url),
            None,
        )
    }
}

/// One hand-off, in the order the views were seen.
enum HandOff {
    Put(Value),
    Delete(u64),
    /// A QR link that is not [`qr_url_ok`]: never sent, told to lamp-tasks
    /// as a put that failed, so the app says to try again (#5198).
    Refused(u64),
}

impl HandOff {
    /// Hands it over; after a put, tells lamp-tasks on `report` whether it
    /// reached the backend, so the app's view says `relay_failed` until a
    /// later put does (#5198). The sign-in goes on either way.
    fn run(self, secrets: &dyn Secrets, report: Option<&Path>) {
        let (what, put_of, done) = match self {
            Self::Put(body) => (
                if body.get("email").is_some() {
                    "the question"
                } else if body.get(QR_URL_FIELD).is_some() {
                    "the QR link"
                } else {
                    "the code"
                },
                body["id"].as_u64(),
                secrets.put(&body),
            ),
            Self::Delete(id) => ("the end", None, secrets.delete(id)),
            Self::Refused(id) => (
                "the QR link",
                Some(id),
                Err("not Harness's sign-in page".to_owned()),
            ),
        };
        match &done {
            Ok(()) => journal(format!("harness sign-in: {what} handed to the backend")),
            Err(why) => journal(format!(
                "harness sign-in: {what} not handed to the backend: {why}"
            )),
        }
        if let (Some(socket), Some(id)) = (report, put_of) {
            let line = json!({"op": "app_sign_in_relay", "id": id, "delivered": done.is_ok()});
            if let Err(why) = exchange(socket, &line) {
                journal(format!(
                    "harness sign-in: lamp-tasks not told of the hand-off: {why}"
                ));
            }
        }
    }
}

/// What was handed for the sign-in open: its id, the code or the QR link
/// as handed (with its `expires_at`), and whether its question was.
#[derive(Default)]
struct Handed {
    open: Option<u64>,
    code: Option<(String, u64)>,
    qr_url: Option<(String, u64)>,
    asked: bool,
}

/// The hand-offs of the secret parts seen in lamp-tasks' views, each once:
/// a code (again as soon as Harness moves its `expires_at`, before the old
/// one passes), an app-started question's email, and the end. Sent on one
/// thread of their own, never on the broker's loop: one call at a time,
/// each after the last has answered, in the order seen (Backend2 on
/// #5198: overlapping calls could lose a newer code).
pub struct HandOffs {
    handed: Mutex<Handed>,
    to: HandOffTo,
}

enum HandOffTo {
    Thread(mpsc::SyncSender<HandOff>),
    /// The tests': each at once, on the caller's thread, told on the
    /// socket if any.
    Here(Arc<dyn Secrets>, Option<PathBuf>),
}

impl std::fmt::Debug for HandOffs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HandOffs { .. }")
    }
}

impl HandOffs {
    /// On a thread of their own, each put's outcome told to lamp-tasks on
    /// `report`.
    pub fn new(secrets: Arc<dyn Secrets>, report: Option<PathBuf>) -> Arc<Self> {
        let (send, take) = mpsc::sync_channel::<HandOff>(HAND_OFFS_QUEUED);
        let started = std::thread::Builder::new()
            .name("lamp-cloud-sign-in-hand-off".into())
            .spawn(move || {
                for hand_off in take {
                    hand_off.run(&*secrets, report.as_deref());
                }
            });
        if started.is_err() {
            journal("harness sign-in: no thread for the hand-offs".into());
        }
        Arc::new(Self {
            handed: Mutex::default(),
            to: HandOffTo::Thread(send),
        })
    }

    /// Each at once, on the caller's thread: for tests.
    pub fn here(secrets: Arc<dyn Secrets>, report: Option<PathBuf>) -> Arc<Self> {
        Arc::new(Self {
            handed: Mutex::default(),
            to: HandOffTo::Here(secrets, report),
        })
    }

    fn hand(&self, hand_off: HandOff) {
        match &self.to {
            HandOffTo::Thread(send) => {
                if send.try_send(hand_off).is_err() {
                    journal("harness sign-in: a hand-off dropped, too many waiting".into());
                }
            }
            HandOffTo::Here(secrets, report) => hand_off.run(&**secrets, report.as_deref()),
        }
    }

    /// What lamp-tasks' full `view` asks to hand: the end of the sign-in
    /// handed before, once it is done or another; its code, whoever started
    /// it, again when an extension moves its `expires_at`; an app-started
    /// question's email.
    pub fn seen(&self, view: &Value) {
        let Ok(mut handed) = self.handed.lock() else {
            return;
        };
        let id = view["id"].as_u64();
        let state = view["sign_in"].as_str().unwrap_or_default();
        if let Some(open) = handed.open
            && (id != Some(open) || state == "done")
        {
            *handed = Handed::default();
            self.hand(HandOff::Delete(open));
        }
        let Some(id) = id else {
            return;
        };
        match state {
            "code" => {
                if let (Some(code), Some(expires_at)) =
                    (view["code"].as_str(), view["expires_at"].as_u64())
                    && handed.code.as_ref() != Some(&(code.to_owned(), expires_at))
                {
                    handed.open = Some(id);
                    handed.code = Some((code.to_owned(), expires_at));
                    self.hand(HandOff::Put(
                        json!({"id": id, "code": code, "expires_at": expires_at}),
                    ));
                }
            }
            // The QR's link (#5198), as the code: only an app starts one.
            // lamp-tasks gives it in one view per telling, so each one seen
            // is handed, unless the same link with the same `expires_at`
            // was already.
            "qr" => {
                if let (Some(url), Some(expires_at)) =
                    (view["qr_url"].as_str(), view["expires_at"].as_u64())
                    && handed.qr_url.as_ref() != Some(&(url.to_owned(), expires_at))
                {
                    handed.open = Some(id);
                    handed.qr_url = Some((url.to_owned(), expires_at));
                    // Checked here, so a link the backend would refuse never
                    // leaves the lamp.
                    self.hand(if qr_url_ok(url) {
                        let mut body = json!({"id": id, "expires_at": expires_at});
                        body[QR_URL_FIELD] = url.into();
                        HandOff::Put(body)
                    } else {
                        HandOff::Refused(id)
                    });
                }
            }
            // The email only for the app's own question: a voice-started
            // sign-in's is asked aloud, and the app shows only its code.
            "asking" if view["by"] == "app" => {
                if let Some(email) = view["email"].as_str()
                    && !handed.asked
                {
                    handed.open = Some(id);
                    handed.asked = true;
                    self.hand(HandOff::Put(json!({"id": id, "email": email})));
                }
            }
            _ => {}
        }
    }
}

/// Where the app's sign-in is asked, and where its secret parts go.
#[derive(Clone, Debug)]
pub struct Place {
    pub socket: PathBuf,
    pub hand_offs: Arc<HandOffs>,
}

impl Place {
    /// The product's: [`SOCKET`] and the backend over HTTPS, with the
    /// switch; none without it, or without the account's base or the
    /// lamp's key, since the code may not go on the broker.
    pub fn product(account: &crate::account::Account, key: &str) -> Option<Self> {
        if !switched_on(std::env::var(ENVIRONMENT).ok().as_deref()) {
            return None;
        }
        let Some(https) = Https::of(account, key) else {
            journal(
                "harness sign-in from the app is off: no backend address or key for the code"
                    .into(),
            );
            return None;
        };
        Some(Self {
            socket: SOCKET.into(),
            hand_offs: HandOffs::new(Arc::new(https), Some(SOCKET.into())),
        })
    }
}

/// lamp-tasks' line for `kind` with the app's `data`, or why there is none.
fn op(kind: &str, data: &Value) -> Result<Value, String> {
    let field = |name: &str| data.get(name).filter(|value| !value.is_null());
    match kind {
        "harness.sign_in.start" => {
            let method = match field("method").map(Value::as_str) {
                None | Some(Some("code")) => "code",
                Some(Some("qr")) => "qr",
                Some(_) => return Err(METHOD_NOT_SUPPORTED.into()),
            };
            Ok(json!({"op": "app_sign_in_start", "method": method}))
        }
        "harness.sign_in.state" => Ok(json!({"op": "app_sign_in_state"})),
        "harness.sign_in.answer" => {
            let id = field("id")
                .and_then(Value::as_u64)
                .ok_or("id is the sign-in's number")?;
            let answer = match field("answer").and_then(Value::as_str) {
                Some(answer @ ("yes" | "no")) => answer,
                _ => return Err("answer is yes or no".into()),
            };
            Ok(json!({"op": "app_sign_in_answer", "id": id, "answer": answer}))
        }
        _ => Err(crate::wire::not_here(kind)),
    }
}

/// What the app shows for a refusal's `error`, as is (#5198).
pub fn message_for(error: &str) -> &'static str {
    match error {
        METHOD_NOT_SUPPORTED => "This lamp can't sign in to Harness that way. Use the code.",
        "voice_sign_in_open" => {
            "A sign-in started by voice is open on the lamp. Its code is shown here."
        }
        "started_by_voice" => "This sign-in was started by voice. Answer it out loud at the lamp.",
        "not_asking" => "The sign-in is not waiting for an answer now.",
        "not_this_sign_in" => "That sign-in is no longer the current one.",
        "not_read" => "The question has not been shown here yet.",
        "too_late" => "The question went unanswered for two minutes. Start again.",
        "ended" => "The sign-in stopped before the answer reached it. Try again.",
        "busy" | "timeout" => "The lamp is busy just now. Try again in a moment.",
        "bad_request" => "The lamp could not read that request.",
        error if error == "sign_in_off" || error.starts_with("not on this lamp: ") => {
            "Signing in to Harness from the app is not turned on for this lamp."
        }
        _ => "The lamp's sign-in could not be reached just now. Try again in a moment.",
    }
}

/// The broker's view: [`BROKER_FIELDS`] only.
fn for_the_broker(view: &Value) -> Value {
    Value::Object(
        BROKER_FIELDS
            .iter()
            .filter_map(|name| Some(((*name).to_owned(), view.get(*name)?.clone())))
            .collect::<Map<String, Value>>(),
    )
}

/// One of [`KINDS`], asked of lamp-tasks at `place`: its view for the
/// broker ([`BROKER_FIELDS`], the secret parts handed off), or its
/// refusal's code (`sign_in_off`, `voice_sign_in_open`, `started_by_voice`,
/// `not_read`, `too_late`, …), or [`UNREACHABLE`]. The journal names the
/// kind and the outcome only.
pub fn ask(place: &Place, kind: &str, data: &Value) -> Result<Option<Value>, String> {
    let line = op(kind, data)?;
    let mut answered = exchange(&place.socket, &line);
    // A lamp-tasks from before the QR can't read its start (#5198).
    if line["method"] == "qr" && answered.as_ref().is_err_and(|why| why == "bad_request") {
        answered = Err(METHOD_NOT_SUPPORTED.into());
    }
    match &answered {
        Ok(view) => {
            place.hand_offs.seen(view);
            journal(format!(
                "{kind} from the app: {}",
                view["sign_in"].as_str().unwrap_or("answered")
            ));
        }
        Err(why) => journal(format!("{kind} from the app refused: {why}")),
    }
    answered.map(|view| Some(for_the_broker(&view)))
}

/// One line to lamp-tasks and its whole view, or the refusal's code (a
/// lowercase code only, never text). Nothing journalled here.
fn exchange(socket: &Path, line: &Value) -> Result<Value, String> {
    let asked = || -> std::io::Result<String> {
        let mut stream = UnixStream::connect(socket)?;
        stream.set_read_timeout(Some(WAIT))?;
        stream.set_write_timeout(Some(WAIT))?;
        stream.write_all(format!("{line}\n").as_bytes())?;
        let mut reply = String::new();
        BufReader::new(stream.take(MOST_REPLY_BYTES)).read_line(&mut reply)?;
        Ok(reply)
    };
    let reply = asked().map_err(|_| UNREACHABLE.to_owned())?;
    let reply: Value = serde_json::from_str(reply.trim()).map_err(|_| UNREACHABLE.to_owned())?;
    match reply.get("ok").and_then(Value::as_bool) {
        Some(true) => Ok(reply),
        Some(false) => Err(reply
            .get("error")
            .and_then(Value::as_str)
            .filter(|code| {
                !code.is_empty()
                    && code.len() <= 32
                    && code.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
            })
            .unwrap_or(UNREACHABLE)
            .to_owned()),
        None => Err(UNREACHABLE.to_owned()),
    }
}

/// The sign-in followed for pushes: the last view pushed, by lamp-tasks'
/// `instance_id`, its `revision` and Harness's `signed_in` (#5524).
pub struct Follow {
    place: Place,
    next: std::time::Instant,
    last: Option<(Value, Value, Value)>,
    open: bool,
}

impl Follow {
    pub fn new(place: Place) -> Self {
        Self {
            place,
            next: std::time::Instant::now(),
            last: None,
            open: false,
        }
    }

    /// The broker's view to push at `now`, if it moved since the last one:
    /// looked at every [`OPEN_EVERY`] while a sign-in is open, every
    /// [`IDLE_EVERY`] otherwise. The first look pushes only an open one.
    /// Each look hands the secret parts off as [`ask`] does.
    pub fn due(&mut self, now: std::time::Instant) -> Option<Value> {
        if now < self.next {
            return None;
        }
        let view = exchange(&self.place.socket, &json!({"op": "app_sign_in_state"})).ok();
        let open = view.as_ref().is_some_and(|view| {
            matches!(
                view["sign_in"].as_str(),
                Some("starting" | "code" | "qr" | "asking" | "answered")
            )
        });
        self.next = now + if open { OPEN_EVERY } else { IDLE_EVERY };
        let view = view?;
        self.place.hand_offs.seen(&view);
        let key = (
            view["instance_id"].clone(),
            view["revision"].clone(),
            view["signed_in"].clone(),
        );
        let first = self.last.is_none();
        self.open = open;
        if self.last.as_ref() == Some(&key) {
            return None;
        }
        self.last = Some(key);
        if first && !open {
            return None;
        }
        journal(format!(
            "{PUSHED} pushed: {}",
            view["sign_in"].as_str().unwrap_or("changed")
        ));
        Some(for_the_broker(&view))
    }

    /// Whether a sign-in was open at the last look.
    pub fn open(&self) -> bool {
        self.open
    }
}

/// A journal line of this module's: never the code, the QR link or the
/// email.
fn journal(line: String) {
    #[cfg(test)]
    tests::JOURNAL.with(|kept| kept.borrow_mut().push(line.clone()));
    eprintln!("lamp-cloud: {line}");
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    thread_local! {
        pub(crate) static JOURNAL: std::cell::RefCell<Vec<String>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    /// The secret parts handed, as a stand-in backend keeps them.
    #[derive(Default)]
    pub(crate) struct Kept {
        pub(crate) puts: Mutex<Vec<Value>>,
        pub(crate) deletes: Mutex<Vec<u64>>,
        pub(crate) fail: bool,
    }

    impl Secrets for Kept {
        fn put(&self, body: &Value) -> Result<(), String> {
            self.puts.lock().unwrap().push(body.clone());
            if self.fail {
                return Err("the backend answered 500".into());
            }
            Ok(())
        }
        fn delete(&self, id: u64) -> Result<(), String> {
            self.deletes.lock().unwrap().push(id);
            Ok(())
        }
    }

    /// A place on `dir`'s stand-in socket, its hand-offs kept at once.
    pub(crate) fn place(dir: &Path, kept: Arc<Kept>) -> Place {
        Place {
            socket: dir.join("app.sock"),
            hand_offs: HandOffs::here(kept, None),
        }
    }

    /// A short folder of its own: a socket's whole path fits in 103 bytes.
    fn folder(tag: &str) -> PathBuf {
        let dir = Path::new("/tmp").join(format!("hs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A stand-in for lamp-tasks' app socket: each line asked, and its
    /// answers in turn.
    fn tasks(dir: &Path, answers: Vec<&'static str>) -> mpsc::Receiver<String> {
        let listener = UnixListener::bind(dir.join("app.sock")).unwrap();
        let (sent, asked) = mpsc::channel();
        std::thread::spawn(move || {
            for (stream, answer) in listener.incoming().flatten().zip(answers) {
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let _ = sent.send(line);
                let _ = (&stream).write_all(format!("{answer}\n").as_bytes());
            }
        });
        asked
    }

    const CODE: &str = "12345678";
    const EMAIL: &str = "someone@example.com";
    const QR_URL: &str = "https://harness.autonomous.ai/signin#k=hnq_abcDEF123";

    /// #5198: the app's QR sign-in goes to lamp-tasks as `"method":"qr"`;
    /// its link (with `expires_at`) goes to the backend as `qr_url`, again
    /// when a keepalive moves `expires_at`, never on the broker, and is
    /// taken back when it ends. No journal line holds it.
    #[test]
    fn the_apps_qr_sign_in_hands_its_link_to_the_backend_and_never_the_broker() {
        let dir = folder("qr");
        let asked = tasks(
            &dir,
            vec![
                r#"{"ok":true,"sign_in":"qr","id":8,"revision":8001,"instance_id":"a","by":"app","method":"qr","answer_here":false,"qr_url":"https://harness.autonomous.ai/signin#k=hnq_abcDEF123","expires_in":120,"expires_at":1800000000000}"#,
                r#"{"ok":true,"sign_in":"qr","id":8,"revision":8001,"instance_id":"a","by":"app","method":"qr","answer_here":false,"qr_url":"https://harness.autonomous.ai/signin#k=hnq_abcDEF123","expires_in":119,"expires_at":1800000000000}"#,
                r#"{"ok":true,"sign_in":"qr","id":8,"revision":8002,"instance_id":"a","by":"app","method":"qr","answer_here":false,"qr_url":"https://harness.autonomous.ai/signin#k=hnq_abcDEF123","expires_in":120,"expires_at":1800000120000}"#,
                r#"{"ok":true,"sign_in":"done","id":8,"revision":8004,"instance_id":"a","by":"app","method":"qr","result":"error","error":"EXPIRED","message":"The QR code ran out of time. Start again for a new one."}"#,
            ],
        );
        let kept = Arc::new(Kept::default());
        let place = place(&dir, kept.clone());
        let started = ask(&place, "harness.sign_in.start", &json!({"method": "qr"}))
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&asked.recv().unwrap()).unwrap(),
            json!({"op": "app_sign_in_start", "method": "qr"})
        );
        assert_eq!(started["sign_in"], "qr");
        assert_eq!(started["method"], "qr");
        assert_eq!(started["expires_at"], 1_800_000_000_000_u64);
        assert!(started.get("qr_url").is_none(), "{started}");
        assert!(!started.to_string().contains("#k="), "{started}");
        // The same link read again: not handed again.
        let again = ask(&place, "harness.sign_in.state", &json!({}))
            .unwrap()
            .unwrap();
        assert!(again.get("qr_url").is_none());
        // Kept alive: handed again with the new expires_at.
        ask(&place, "harness.sign_in.state", &json!({})).unwrap();
        assert_eq!(
            *kept.puts.lock().unwrap(),
            [
                json!({"id": 8, "qr_url": QR_URL, "expires_at": 1_800_000_000_000_u64}),
                json!({"id": 8, "qr_url": QR_URL, "expires_at": 1_800_000_120_000_u64}),
            ]
        );
        let done = ask(&place, "harness.sign_in.state", &json!({}))
            .unwrap()
            .unwrap();
        assert_eq!(done["error"], "EXPIRED");
        assert_eq!(*kept.deletes.lock().unwrap(), [8]);
        JOURNAL.with(|journal| {
            for line in journal.borrow().iter() {
                assert!(!line.contains("#k=") && !line.contains("hnq_"), "{line}");
            }
        });
    }

    /// #5198: each kind goes to lamp-tasks as its own op with only what the
    /// app may say; the view comes back with neither the code nor the email
    /// nor lamp-tasks' own fields, and those two go to the backend instead;
    /// a refusal is its code; no socket, nothing done.
    #[test]
    fn the_apps_sign_in_is_handed_to_lamp_tasks_and_its_secrets_to_the_backend() {
        let dir = folder("ok");
        let asked = tasks(
            &dir,
            vec![
                r#"{"ok":true,"sign_in":"code","id":7,"revision":7001,"instance_id":"a","by":"app","method":"code","answer_here":false,"code":"12345678","expires_in":120,"expires_at":1800000000000,"store_id":"s"}"#,
                r#"{"ok":true,"sign_in":"asking","id":7,"revision":7002,"instance_id":"a","by":"app","method":"code","answer_here":true,"email":"someone@example.com","answer_within":120}"#,
                r#"{"ok":false,"error":"started_by_voice","message":"That sign-in was started by voice."}"#,
                r#"{"ok":false,"error":"Not A Code!"}"#,
                r#"{"ok":true,"sign_in":"done","id":7,"revision":7004,"instance_id":"a","by":"app","method":"code","result":"success"}"#,
            ],
        );
        let kept = Arc::new(Kept::default());
        let place = place(&dir, kept.clone());
        let started = ask(&place, "harness.sign_in.start", &json!({"method": "code"}))
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&asked.recv().unwrap()).unwrap(),
            json!({"op": "app_sign_in_start", "method": "code"})
        );
        assert_eq!(started["sign_in"], "code");
        assert_eq!(started["expires_at"], 1_800_000_000_000_u64);
        assert!(started.get("code").is_none() && started.get("store_id").is_none());
        let state = ask(&place, "harness.sign_in.state", &Value::Null)
            .unwrap()
            .unwrap();
        assert_eq!(state["answer_here"], true);
        assert!(state.get("email").is_none(), "{state}");
        assert_eq!(
            *kept.puts.lock().unwrap(),
            [
                json!({"id": 7, "code": CODE, "expires_at": 1_800_000_000_000_u64}),
                json!({"id": 7, "email": EMAIL}),
            ]
        );
        asked.recv().unwrap();
        assert_eq!(
            ask(
                &place,
                "harness.sign_in.answer",
                &json!({"id": 7, "answer": "yes"})
            ),
            Err("started_by_voice".to_owned())
        );
        assert_eq!(
            serde_json::from_str::<Value>(&asked.recv().unwrap()).unwrap(),
            json!({"op": "app_sign_in_answer", "id": 7, "answer": "yes"})
        );
        // A refusal that is no code is not passed on as said.
        assert_eq!(
            ask(&place, "harness.sign_in.state", &json!({})),
            Err(UNREACHABLE.to_owned())
        );
        // Done: taken back at the backend, once.
        let done = ask(&place, "harness.sign_in.state", &json!({}))
            .unwrap()
            .unwrap();
        assert_eq!(done["result"], "success");
        assert_eq!(*kept.deletes.lock().unwrap(), [7]);
        // A method neither the code nor the QR: refused here, never run.
        for method in ["email", "sms", "QR", ""] {
            assert_eq!(
                ask(&place, "harness.sign_in.start", &json!({"method": method})),
                Err(METHOD_NOT_SUPPORTED.to_owned()),
                "{method}"
            );
        }
        for (kind, data) in [
            ("harness.sign_in.start", json!({"method": 1})),
            (
                "harness.sign_in.answer",
                json!({"id": 7, "answer": "maybe"}),
            ),
            ("harness.sign_in.answer", json!({"answer": "yes"})),
            ("harness.sign_in.answer", json!({"id": -1, "answer": "no"})),
            ("harness.sign_in.later", json!({})),
        ] {
            assert!(ask(&place, kind, &data).is_err(), "{kind} {data}");
        }
        let unreached = Place {
            socket: dir.join("none.sock"),
            ..place.clone()
        };
        assert_eq!(
            ask(&unreached, "harness.sign_in.state", &json!({})),
            Err(UNREACHABLE.to_owned())
        );
        assert_eq!(
            op("harness.sign_in.start", &Value::Null),
            Ok(json!({"op": "app_sign_in_start", "method": "code"}))
        );
        assert!(switched_on(Some("on")));
        for value in [None, Some("ON"), Some("yes"), Some("")] {
            assert!(!switched_on(value), "{value:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #5198: a QR start goes to lamp-tasks as `"method":"qr"`; the link it
    /// gives goes to the backend over HTTPS beside its `expires_at`, as the
    /// code does, again when Harness tells it again, and never on the
    /// broker, in a push or a journal line; the end takes it back. A
    /// lamp-tasks from before the QR refuses it as `method_not_supported`.
    #[test]
    fn a_qr_sign_in_hands_its_link_to_the_backend_only() {
        let dir = folder("qrlink");
        let asked = tasks(
            &dir,
            vec![
                r#"{"ok":true,"sign_in":"qr","id":8,"revision":8001,"instance_id":"a","by":"app","method":"qr","answer_here":false,"qr_url":"https://harness.autonomous.ai/signin#k=hnq_abcDEF123","expires_in":120,"expires_at":100}"#,
                r#"{"ok":true,"sign_in":"qr","id":8,"revision":8001,"instance_id":"a","by":"app","method":"qr","expires_in":119,"expires_at":100}"#,
                r#"{"ok":true,"sign_in":"qr","id":8,"revision":8002,"instance_id":"a","by":"app","method":"qr","qr_url":"https://harness.autonomous.ai/signin#k=hnq_abcDEF123","expires_in":120,"expires_at":220}"#,
                r#"{"ok":true,"sign_in":"asking","id":8,"revision":8003,"instance_id":"a","by":"app","method":"qr","answer_here":true,"email":"someone@example.com","answer_within":120}"#,
                r#"{"ok":true,"sign_in":"done","id":8,"revision":8004,"instance_id":"a","by":"app","method":"qr","result":"success"}"#,
                r#"{"ok":false,"error":"bad_request","message":"bad request"}"#,
            ],
        );
        JOURNAL.with(|kept| kept.borrow_mut().clear());
        let kept = Arc::new(Kept::default());
        let place = place(&dir, kept.clone());
        let mut said = Vec::new();
        let started = ask(&place, "harness.sign_in.start", &json!({"method": "qr"}))
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&asked.recv().unwrap()).unwrap(),
            json!({"op": "app_sign_in_start", "method": "qr"})
        );
        assert_eq!(started["sign_in"], "qr");
        assert_eq!(started["method"], "qr");
        assert_eq!(started["expires_at"], 100);
        said.push(started.to_string());
        let mut follow = Follow::new(place.clone());
        let t = std::time::Instant::now();
        // A read with no link hands nothing; the link told again does.
        for at in [t, t + OPEN_EVERY] {
            if let Some(pushed) = follow.due(at) {
                said.push(pushed.to_string());
            }
        }
        assert!(follow.open());
        said.push(format!(
            "{:?}",
            ask(&place, "harness.sign_in.state", &json!({}))
        ));
        let done = ask(&place, "harness.sign_in.state", &json!({}))
            .unwrap()
            .unwrap();
        said.push(done.to_string());
        assert_eq!(
            *kept.puts.lock().unwrap(),
            [
                json!({"id": 8, "qr_url": QR_URL, "expires_at": 100}),
                json!({"id": 8, "qr_url": QR_URL, "expires_at": 220}),
                json!({"id": 8, "email": EMAIL}),
            ]
        );
        assert_eq!(*kept.deletes.lock().unwrap(), [8]);
        // A lamp-tasks from before the QR.
        assert_eq!(
            ask(&place, "harness.sign_in.start", &json!({"method": "qr"})),
            Err(METHOD_NOT_SUPPORTED.to_owned())
        );
        said.push(message_for(METHOD_NOT_SUPPORTED).to_owned());
        said.push(format!("{place:?}"));
        said.extend(JOURNAL.with(|kept| kept.borrow().clone()));
        assert!(
            said.iter()
                .any(|line| line.contains("the QR link handed to the backend")),
            "{said:?}"
        );
        for line in &said {
            assert!(!line.contains("hnq_") && !line.contains("://"), "{line}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Backend2 on #5198: only the Harness sign-in page with a key is
    /// handed; any other link is never sent, and lamp-tasks is told it
    /// failed, so the app says to try again.
    #[test]
    fn only_the_harness_sign_in_page_is_handed_as_a_qr_link() {
        assert!(qr_url_ok(QR_URL));
        assert!(qr_url_ok(&format!("{QR_URL_PREFIX}{}", "a".repeat(512))));
        for bad in [
            "https://harness.autonomous.ai/signin#k=",
            "https://harness.autonomous.ai/signin#k=a b",
            "https://harness.autonomous.ai/signin#k=a&b=c",
            "https://harness.autonomous.ai/signin?k=abc",
            "https://harness.autonomous.ai.evil.example/signin#k=abc",
            "https://evil.example/https://harness.autonomous.ai/signin#k=abc",
            "http://harness.autonomous.ai/signin#k=abc",
            "HTTPS://harness.autonomous.ai/signin#k=abc",
        ] {
            assert!(!qr_url_ok(bad), "{bad}");
        }
        assert!(!qr_url_ok(&format!("{QR_URL_PREFIX}{}", "a".repeat(513))));
        let dir = folder("qrbad");
        let asked = tasks(
            &dir,
            vec![r#"{"ok":true,"sign_in":"qr","id":9,"error":"relay_failed"}"#],
        );
        JOURNAL.with(|kept| kept.borrow_mut().clear());
        let kept = Arc::new(Kept::default());
        let hand_offs = HandOffs::here(kept.clone(), Some(dir.join("app.sock")));
        hand_offs.seen(&json!({"sign_in": "qr", "id": 9, "by": "app", "qr_url": "https://evil.example/#k=hnq_x", "expires_at": 100}));
        assert!(kept.puts.lock().unwrap().is_empty());
        assert_eq!(
            serde_json::from_str::<Value>(&asked.recv().unwrap()).unwrap(),
            json!({"op": "app_sign_in_relay", "id": 9, "delivered": false})
        );
        let said = JOURNAL.with(|kept| kept.borrow().clone());
        assert_eq!(
            said,
            ["harness sign-in: the QR link not handed to the backend: not Harness's sign-in page"]
        );
        // The end still takes the sign-in back at the backend.
        hand_offs.seen(&json!({"sign_in": "done", "id": 9}));
        assert_eq!(*kept.deletes.lock().unwrap(), [9]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #5198: no secret field is one the broker is given.
    #[test]
    fn no_secret_field_goes_on_the_broker() {
        for secret in SECRET_FIELDS {
            assert!(!BROKER_FIELDS.contains(&secret), "{secret}");
        }
        assert!(SECRET_FIELDS.contains(&QR_URL_FIELD));
        let view = json!({"sign_in": "qr", "id": 1, "code": CODE, "qr_url": QR_URL, "email": EMAIL, "expires_at": 5});
        let broker = for_the_broker(&view);
        assert_eq!(broker, json!({"sign_in": "qr", "id": 1, "expires_at": 5}));
    }

    /// #5198, the Mobile builder's ask: the view is pushed only when
    /// `instance_id` or `revision` moves, never for a read that changed
    /// nothing; nothing at the first look with none open; looked at again
    /// sooner while one is open. A code kept alive (its `expires_at`
    /// moved) is handed again; another sign-in takes the last one back.
    #[test]
    fn a_change_is_pushed_once_and_a_read_that_changed_nothing_is_not() {
        let dir = folder("push");
        let _asked = tasks(
            &dir,
            vec![
                r#"{"ok":true,"sign_in":"none","revision":0,"instance_id":"a"}"#,
                r#"{"ok":true,"sign_in":"code","id":5,"revision":5001,"instance_id":"a","code":"12345678","expires_at":100}"#,
                r#"{"ok":true,"sign_in":"code","id":5,"revision":5001,"instance_id":"a","code":"12345678","expires_at":100}"#,
                r#"{"ok":true,"sign_in":"code","id":5,"revision":5002,"instance_id":"a","code":"12345678","expires_at":220}"#,
                r#"{"ok":true,"sign_in":"code","id":6,"revision":6001,"instance_id":"b","code":"87654321","expires_at":300}"#,
            ],
        );
        let kept = Arc::new(Kept::default());
        let mut follow = Follow::new(place(&dir, kept.clone()));
        let t = std::time::Instant::now();
        assert_eq!(follow.due(t), None);
        assert!(!follow.open());
        // Not looked at again before its time.
        assert_eq!(follow.due(t + Duration::from_millis(10)), None);
        let pushed = follow.due(t + IDLE_EVERY).unwrap();
        assert_eq!(pushed["revision"], 5001);
        assert!(pushed.get("code").is_none());
        assert!(follow.open());
        assert_eq!(follow.due(t + IDLE_EVERY + OPEN_EVERY), None);
        let t2 = t + IDLE_EVERY + OPEN_EVERY * 2;
        assert_eq!(follow.due(t2).unwrap()["revision"], 5002);
        assert_eq!(follow.due(t2 + OPEN_EVERY).unwrap()["instance_id"], "b");
        let puts: Vec<Value> = kept.puts.lock().unwrap().clone();
        assert_eq!(
            puts,
            [
                json!({"id": 5, "code": "12345678", "expires_at": 100}),
                json!({"id": 5, "code": "12345678", "expires_at": 220}),
                json!({"id": 6, "code": "87654321", "expires_at": 300}),
            ]
        );
        assert_eq!(*kept.deletes.lock().unwrap(), [5]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #5524: whether Harness is signed in reaches the app in the state's
    /// view, and a change of it is pushed with no sign-in open (signed out
    /// from a computer, or lamp-tasks' first read after a restart), once.
    #[test]
    fn harness_signed_in_is_in_the_view_and_its_change_is_pushed() {
        let dir = folder("signed");
        let _asked = tasks(
            &dir,
            vec![
                r#"{"ok":true,"sign_in":"none","revision":0,"instance_id":"a","signed_in":"yes"}"#,
                r#"{"ok":true,"sign_in":"none","revision":0,"instance_id":"a","signed_in":"yes"}"#,
                r#"{"ok":true,"sign_in":"none","revision":0,"instance_id":"a","signed_in":"no"}"#,
                r#"{"ok":true,"sign_in":"none","revision":0,"instance_id":"b"}"#,
                r#"{"ok":true,"sign_in":"none","revision":0,"instance_id":"b","signed_in":"yes"}"#,
                r#"{"ok":true,"sign_in":"none","revision":0,"instance_id":"b","signed_in":"yes"}"#,
            ],
        );
        let kept = Arc::new(Kept::default());
        let place = place(&dir, kept);
        let read = ask(&place, "harness.sign_in.state", &json!({}))
            .unwrap()
            .unwrap();
        assert_eq!(read["signed_in"], "yes");
        assert_eq!(read["sign_in"], "none");
        let mut follow = Follow::new(place);
        let t = std::time::Instant::now();
        // The first look, none open: nothing pushed, as before.
        assert_eq!(follow.due(t), None);
        // Signed out from a computer: pushed, with no sign-in open.
        assert_eq!(follow.due(t + IDLE_EVERY).unwrap()["signed_in"], "no");
        // lamp-tasks restarted, not read yet: pushed for its new instance.
        assert!(
            follow
                .due(t + IDLE_EVERY * 2)
                .unwrap()
                .get("signed_in")
                .is_none()
        );
        // Its first read: pushed once, then nothing for the same.
        assert_eq!(follow.due(t + IDLE_EVERY * 3).unwrap()["signed_in"], "yes");
        assert_eq!(follow.due(t + IDLE_EVERY * 4), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The PM's privacy rule on #5198: no journal line, refusal, message or
    /// `Debug` of this module holds the code or the email, whether the
    /// hand-off works or fails.
    #[test]
    fn no_journal_line_or_error_holds_the_code_or_the_email() {
        let dir = folder("quiet");
        let _asked = tasks(
            &dir,
            vec![
                r#"{"ok":true,"sign_in":"code","id":3,"revision":3001,"instance_id":"a","by":"app","method":"code","code":"12345678","expires_at":100}"#,
                r#"{"ok":true,"sign_in":"asking","id":3,"revision":3002,"instance_id":"a","by":"app","method":"code","email":"someone@example.com"}"#,
                r#"{"ok":false,"error":"12345678"}"#,
                r#"{"ok":false,"error":"someone@example.com"}"#,
                r#"{"ok":true,"sign_in":"done","id":3,"revision":3003,"instance_id":"a","result":"error","error":"DENIED"}"#,
            ],
        );
        JOURNAL.with(|kept| kept.borrow_mut().clear());
        let kept = Arc::new(Kept {
            fail: true,
            ..Kept::default()
        });
        let place = place(&dir, kept.clone());
        let mut said = Vec::new();
        for _ in 0..5 {
            said.push(format!(
                "{:?}",
                ask(&place, "harness.sign_in.state", &json!({}))
            ));
        }
        for error in ["12345678", "someone@example.com", UNREACHABLE] {
            said.push(message_for(error).to_owned());
        }
        said.push(format!("{place:?}"));
        said.extend(JOURNAL.with(|kept| kept.borrow().clone()));
        assert!(
            said.iter()
                .any(|line| line.contains("not handed to the backend"))
        );
        for line in &said {
            assert!(
                !line.contains("1234") && !line.contains('@') && !line.contains("example"),
                "{line}"
            );
        }
        // Both were still handed, over HTTPS only.
        assert_eq!(kept.puts.lock().unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Mobile on #5198: a voice-started sign-in's code is handed too, and
    /// again when an extension moves its `expires_at`; its email never,
    /// even if a view held one. An app-started question's email is handed
    /// once; the end once.
    #[test]
    fn a_voice_started_code_is_handed_and_its_email_never() {
        let kept = Arc::new(Kept::default());
        let hand_offs = HandOffs::here(kept.clone(), None);
        let voice = |state: &str, at: u64| json!({"sign_in": state, "id": 9, "by": "voice", "code": CODE, "expires_at": at, "email": EMAIL});
        hand_offs.seen(&voice("code", 100));
        hand_offs.seen(&voice("code", 100));
        hand_offs.seen(&voice("code", 220));
        hand_offs.seen(&voice("asking", 220));
        hand_offs.seen(&json!({"sign_in": "done", "id": 9, "by": "voice"}));
        hand_offs.seen(&json!({"sign_in": "done", "id": 9, "by": "voice"}));
        assert_eq!(
            *kept.puts.lock().unwrap(),
            [
                json!({"id": 9, "code": CODE, "expires_at": 100}),
                json!({"id": 9, "code": CODE, "expires_at": 220}),
            ]
        );
        assert_eq!(*kept.deletes.lock().unwrap(), [9]);
        let app = |state: &str| json!({"sign_in": state, "id": 10, "by": "app", "code": CODE, "expires_at": 300, "email": EMAIL});
        hand_offs.seen(&app("code"));
        hand_offs.seen(&app("asking"));
        hand_offs.seen(&app("asking"));
        hand_offs.seen(&json!({"sign_in": "none"}));
        let puts = kept.puts.lock().unwrap().clone();
        assert_eq!(
            puts[2..],
            [
                json!({"id": 10, "code": CODE, "expires_at": 300}),
                json!({"id": 10, "email": EMAIL}),
            ]
        );
        assert_eq!(*kept.deletes.lock().unwrap(), [9, 10]);
    }

    /// The coordinator on #5198: a put that failed (5xx after its retry,
    /// 401, 400 or 409) is told to lamp-tasks so the app's view says
    /// `relay_failed`; one that worked after it is told too, to clear it;
    /// an end is not told; a lamp-tasks that cannot be told stops nothing.
    #[test]
    fn each_puts_outcome_is_told_to_lamp_tasks_for_the_app() {
        let dir = folder("relay");
        let asked = tasks(
            &dir,
            vec![
                r#"{"ok":true,"sign_in":"code","id":4,"error":"relay_failed"}"#,
                r#"{"ok":true,"sign_in":"code","id":4}"#,
            ],
        );
        let sock = Some(dir.join("app.sock"));
        let line = |at: u64| json!({"sign_in": "code", "id": 4, "by": "app", "code": CODE, "expires_at": at});
        let failing = HandOffs::here(
            Arc::new(Kept {
                fail: true,
                ..Kept::default()
            }),
            sock.clone(),
        );
        failing.seen(&line(100));
        assert_eq!(
            serde_json::from_str::<Value>(&asked.recv().unwrap()).unwrap(),
            json!({"op": "app_sign_in_relay", "id": 4, "delivered": false})
        );
        let kept = Arc::new(Kept::default());
        let working = HandOffs::here(kept.clone(), sock);
        working.seen(&line(220));
        assert_eq!(
            serde_json::from_str::<Value>(&asked.recv().unwrap()).unwrap(),
            json!({"op": "app_sign_in_relay", "id": 4, "delivered": true})
        );
        // The end: taken back, nothing told; and with lamp-tasks gone the
        // put is still made.
        working.seen(&json!({"sign_in": "done", "id": 4}));
        assert_eq!(*kept.deletes.lock().unwrap(), [4]);
        let gone = HandOffs::here(kept.clone(), Some(dir.join("none.sock")));
        gone.seen(&line(300));
        assert_eq!(kept.puts.lock().unwrap().len(), 2);
        assert!(asked.try_recv().is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Backend2 on #5198: one call at a time per lamp. Two quick changes and
    /// the end go out in the order seen, each after the last answered,
    /// never two at once.
    #[test]
    fn quick_changes_go_out_one_at_a_time_in_order() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        #[derive(Default)]
        struct Slow {
            calls: Mutex<Vec<String>>,
            in_flight: AtomicUsize,
            most: AtomicUsize,
        }
        impl Slow {
            fn call(&self, what: String) -> Result<(), String> {
                let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                self.most.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(60));
                self.calls.lock().unwrap().push(what);
                self.in_flight.fetch_sub(1, Ordering::SeqCst);
                Ok(())
            }
        }
        impl Secrets for Slow {
            fn put(&self, body: &Value) -> Result<(), String> {
                self.call(format!("put {}", body["expires_at"]))
            }
            fn delete(&self, id: u64) -> Result<(), String> {
                self.call(format!("delete {id}"))
            }
        }
        let slow = Arc::new(Slow::default());
        let hand_offs = HandOffs::new(slow.clone(), None);
        let code = |at: u64| json!({"sign_in": "code", "id": 2, "by": "app", "code": CODE, "expires_at": at});
        hand_offs.seen(&code(100));
        hand_offs.seen(&code(220));
        hand_offs.seen(&json!({"sign_in": "done", "id": 2}));
        let until = std::time::Instant::now() + Duration::from_secs(10);
        while slow.calls.lock().unwrap().len() < 3 && std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            *slow.calls.lock().unwrap(),
            ["put 100", "put 220", "delete 2"]
        );
        assert_eq!(slow.most.load(Ordering::SeqCst), 1);
    }

    /// Backend2's route on #5198: `PUT <base>/devices/harness/sign-in` with
    /// the body and `DELETE …?id=N`, the lamp's key as the bearer, the same
    /// base as `/devices/get-message`; a 5xx is tried again once, a 409
    /// (`sign_in_old_id`) and a 401 are not; no reason holds the body.
    #[test]
    fn the_backend_is_handed_the_code_as_backend2_asks() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = seen.clone();
        std::thread::spawn(move || {
            for (stream, status) in listener.incoming().zip([
                "200 OK",
                "503 Busy",
                "200 OK",
                "409 Conflict",
                "500 Oops",
                "500 Oops",
                "200 OK",
            ]) {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut head = String::new();
                let mut length = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(n) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = n.trim().parse().unwrap();
                    }
                    head.push_str(&line);
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                kept.lock()
                    .unwrap()
                    .push((head.to_ascii_lowercase(), String::from_utf8(body).unwrap()));
                let _ = stream.write_all(
                    format!("HTTP/1.1 {status}\r\nconnection: close\r\ncontent-length: 0\r\n\r\n")
                        .as_bytes(),
                );
            }
        });
        let account: crate::account::Account = serde_json::from_value(json!({
            "device_id": "d-42", "llm_base_url": format!("http://{address}/v1/")
        }))
        .unwrap();
        let https = Https::of(&account, " lamp-key ").unwrap();
        assert!(!format!("{https:?}").contains("lamp-key"));
        let body = json!({"id": 4, "code": CODE, "expires_at": 100});
        assert_eq!(https.put(&body), Ok(()));
        // A 503, then the one more try.
        assert_eq!(https.put(&json!({"id": 4, "email": EMAIL})), Ok(()));
        // An old id: not tried again.
        assert_eq!(
            https.put(&json!({"id": 3, "code": CODE, "expires_at": 1})),
            Err("the backend said sign_in_old_id (409)".to_owned())
        );
        // Two 5xx: failed, without the body.
        let failed = https.put(&body).unwrap_err();
        assert!(!failed.contains("1234"), "{failed}");
        assert_eq!(https.delete(4), Ok(()));
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 7);
        assert!(
            seen[0]
                .0
                .starts_with("put /devices/harness/sign-in http/1.1"),
            "{:?}",
            seen[0]
        );
        assert!(seen[0].0.contains("authorization: bearer lamp-key"));
        assert_eq!(serde_json::from_str::<Value>(&seen[0].1).unwrap(), body);
        assert!(seen[2].1.contains(EMAIL));
        assert!(
            seen[6]
                .0
                .starts_with("delete /devices/harness/sign-in?id=4 http/1.1"),
            "{:?}",
            seen[6]
        );
        assert!(Https::of(&account, "").is_none());
    }
}
