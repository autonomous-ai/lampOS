//! Signing Harness in by a spoken code (`serve --sign-in-by-code`, #1676).
//! Harness 0.4.23's `harness login --code --json` (openharness#1410, the
//! Harness team on 20 October) prints one JSON object a line:
//!
//! - `{"type":"code","code":"12345678","expiresIn":120}`: the digits the lamp
//!   says aloud, again with the same code each time it is kept alive (up to
//!   10 minutes in all);
//! - `{"type":"confirm","email":"…"}`: the person typed the code into Harness
//!   and approved; the lamp asks "Sign in as …?" and writes `yes` or `no` and
//!   a newline. No session exists before that answer, so a stranger who
//!   heard the code gains nothing;
//! - `{"type":"result","status":"success","email":"…"}`, or `"error"` with a
//!   `code` (`DENIED`, `EXPIRED`, `CANCELLED`, `BACKEND_ERROR`).
//!
//! The part runs Harness's own sign-in and passes the person's spoken answer
//! on; it keeps no credential. The code's digits and the email are said by
//! the voice and never journalled here.
//!
//! The owner's app may start one too (#5198, `serve --app-socket`), by the
//! same code, which the app shows and the person types into Harness. Each
//! sign-in records who started it ([`By`]) and how ([`Method`]): only the
//! app answers an app-started one, only the voice a voice-started one, and
//! the voice never sees an app-started one's code or email.
//!
//! The app may instead start Harness's QR sign-in (#5198, `"method":"qr"`):
//! `harness login --qr --json` (Harness 0.6.16, the pinned version) prints
//! `{"type":"qr","url":"https://harness.autonomous.ai/signin#k=…","expiresIn":N}`
//! in place of the code, again when it is kept alive, and then the same
//! `confirm` and `result`. The app shows the link as a QR, which the Harness
//! phone app scans. Only the app starts one: the voice has nothing to show
//! it on. The link is the secret, as the code is: held only in the session,
//! given in one app view per telling ([`Session::app_view_claiming`]), never
//! said, journalled, put on the broker or in a `Debug`, and forgotten when
//! the sign-in ends.
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// The most a sign-in is kept open: Harness keeps a code alive for 10
/// minutes in all, and the person may take a while to answer the question.
pub const MOST_LIFE: Duration = Duration::from_secs(11 * 60);
/// No line from Harness's sign-in is read past this many bytes.
pub const MOST_LINE_BYTES: usize = 4096;
const MOST_EMAIL_BYTES: usize = 254;
/// How long after the question is told an answer to it is taken: a yes
/// heard later is for something else.
pub const ANSWER_WINDOW: Duration = Duration::from_secs(120);
const MOST_EXPIRES_IN: u64 = 600;

/// How a sign-in is done (#5198): Harness's 8-digit code, or its QR (only
/// from the app). Named in the app's view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    #[default]
    Code,
    Qr,
}

impl Method {
    pub fn name(self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Qr => "qr",
        }
    }
}

/// What a QR sign-in's link must begin with: Harness's sign-in page, with
/// its key after `#k=` (Harness 0.6.16's `signin#k=`, the backend's own
/// check on #5198 refuses any other).
pub const QR_PREFIX: &str = "https://harness.autonomous.ai/signin#k=";
/// The longest QR link taken.
const MOST_QR_BYTES: usize = 512;

/// Who started a sign-in (#5198): only they answer its question.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum By {
    #[default]
    Voice,
    App,
}

impl By {
    pub fn name(self) -> &'static str {
        match self {
            Self::Voice => "voice",
            Self::App => "app",
        }
    }
}

/// One line of `harness login --code --json`. Its `Debug` hides the digits
/// and the email (Jarvis's P3 on #2131), so a later `{:?}` can't print them.
#[derive(Clone, PartialEq, Eq)]
pub enum Event {
    /// The digits to say, and how many seconds they stay good.
    Code {
        code: String,
        expires_in: u64,
    },
    /// The link to show as a QR (`--qr`), and how many seconds it stays
    /// good.
    Qr {
        url: String,
        expires_in: u64,
    },
    /// The person approved in Harness: ask them whether to sign in as this.
    Confirm {
        email: String,
    },
    Result(Done),
    /// A result with more than [`Event::Result`] holds (#5198): Harness's
    /// own `message` (only text with no `@`, bounded), or
    /// `alreadySignedIn:true` (a success with nothing to do).
    ResultWith {
        done: Done,
        message: Option<String>,
        already_signed_in: bool,
    },
}

/// The most of Harness's own message kept for the app.
const MOST_MESSAGE_BYTES: usize = 200;

/// Harness's `message` as the app may show it: plain, bounded text, with
/// no address in it and no code: no eight digits in a row once every
/// non-digit is taken out (the privacy reader's P3 on #5236, the
/// reviewer-lead's P2 on #5579), so a quoted code never reaches the broker;
/// and no link or QR key (#5198).
fn message(value: Option<&Value>) -> Option<String> {
    let message = value?.as_str()?.trim();
    (!message.is_empty()
        && message.len() <= MOST_MESSAGE_BYTES
        && !message.contains('@')
        && !has_eight_digits(message)
        // Nor a link or its key: a QR sign-in's url is as secret as the
        // code (#5198), and Harness's keys are `#k=hnq_…`.
        && !message.contains("://")
        && !message.contains("#k=")
        && !message.contains("hnq_")
        && !message.chars().any(char::is_control))
    .then(|| message.to_owned())
}

/// Whether `text` holds eight digits, a sign-in code's shape, whatever
/// the spacing or separators between them: "1234 5678", "1234-5678".
fn has_eight_digits(text: &str) -> bool {
    digits(text).len() >= 8
}

/// `text`'s digits alone, every other character taken out, so a code is
/// found whatever its spacing or separators (#5579).
fn digits(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).collect()
}

/// How a sign-in ended. Its `Debug` hides the email.
#[derive(Clone, PartialEq, Eq)]
pub enum Done {
    Success {
        email: Option<String>,
    },
    /// Harness's code (`DENIED`, `EXPIRED`, `CANCELLED`, `BACKEND_ERROR`), or
    /// the part's own: `ended` (the program stopped with no result),
    /// `harness_too_old` (it doesn't know `--code`), `unreadable` (a line
    /// that isn't one of the above), `timeout` ([`MOST_LIFE`]) or
    /// `not_started`.
    Error {
        code: String,
    },
}

impl std::fmt::Debug for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Code { expires_in, .. } => {
                write!(f, "Code {{ code: <hidden>, expires_in: {expires_in} }}")
            }
            Self::Qr { expires_in, .. } => {
                write!(f, "Qr {{ url: <hidden>, expires_in: {expires_in} }}")
            }
            Self::Confirm { .. } => write!(f, "Confirm {{ email: <hidden> }}"),
            Self::Result(done) => write!(f, "Result({done:?})"),
            Self::ResultWith {
                done,
                message,
                already_signed_in,
            } => write!(
                f,
                "ResultWith {{ done: {done:?}, message: {}, already_signed_in: {already_signed_in} }}",
                if message.is_some() { "<kept>" } else { "None" }
            ),
        }
    }
}

impl std::fmt::Debug for Done {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Success { email } => write!(
                f,
                "Success {{ email: {} }}",
                if email.is_some() { "<hidden>" } else { "None" }
            ),
            Self::Error { code } => write!(f, "Error {{ code: {code:?} }}"),
        }
    }
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Starting => write!(f, "Starting"),
            Self::Code { .. } => write!(f, "Code {{ code: <hidden> }}"),
            Self::Qr { .. } => write!(f, "Qr {{ url: <hidden> }}"),
            Self::Asking { .. } => write!(f, "Asking {{ email: <hidden> }}"),
            Self::Answered { yes } => write!(f, "Answered {{ yes: {yes} }}"),
            Self::Done(done) => write!(f, "Done({done:?})"),
        }
    }
}

fn email(value: Option<&Value>) -> Option<String> {
    let email = value?.as_str()?;
    (!email.is_empty()
        && email.len() <= MOST_EMAIL_BYTES
        && email.contains('@')
        && !email.chars().any(|c| c.is_control() || c.is_whitespace()))
    .then(|| email.to_owned())
}

/// One line of Harness's sign-in, or `None` for a kind this part doesn't
/// know (passed over, as Harness may add some).
pub fn parse_line(line: &str) -> Result<Option<Event>, &'static str> {
    let unreadable = "unreadable";
    if line.len() > MOST_LINE_BYTES {
        return Err(unreadable);
    }
    let value: Value = serde_json::from_str(line).map_err(|_| unreadable)?;
    let kind = value["type"].as_str().ok_or(unreadable)?;
    Ok(Some(match kind {
        "code" => {
            let code = value["code"].as_str().ok_or(unreadable)?;
            let expires_in = value["expiresIn"].as_u64().ok_or(unreadable)?;
            if !(4..=12).contains(&code.len())
                || !code.bytes().all(|b| b.is_ascii_digit())
                || expires_in == 0
                || expires_in > MOST_EXPIRES_IN
            {
                return Err(unreadable);
            }
            Event::Code {
                code: code.to_owned(),
                expires_in,
            }
        }
        "qr" => {
            let url = value["url"].as_str().ok_or(unreadable)?;
            let expires_in = value["expiresIn"].as_u64().ok_or(unreadable)?;
            if !url.starts_with(QR_PREFIX)
                || url.len() == QR_PREFIX.len()
                || url.len() > MOST_QR_BYTES
                || !url.bytes().all(|b| b.is_ascii_graphic())
                || expires_in == 0
                || expires_in > MOST_EXPIRES_IN
            {
                return Err(unreadable);
            }
            Event::Qr {
                url: url.to_owned(),
                expires_in,
            }
        }
        "confirm" => Event::Confirm {
            email: email(value.get("email")).ok_or(unreadable)?,
        },
        "result" => {
            let done = match value["status"].as_str() {
                Some("success") => Done::Success {
                    email: email(value.get("email")),
                },
                Some("error") => {
                    let code = value["code"]
                        .as_str()
                        .filter(|code| {
                            !code.is_empty()
                                && code.len() <= 32
                                && code.bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
                        })
                        .unwrap_or("BACKEND_ERROR");
                    Done::Error {
                        code: code.to_owned(),
                    }
                }
                _ => return Err(unreadable),
            };
            let message = message(value.get("message"));
            let already_signed_in = matches!(done, Done::Success { .. })
                && value["alreadySignedIn"].as_bool() == Some(true);
            if message.is_none() && !already_signed_in {
                Event::Result(done)
            } else {
                Event::ResultWith {
                    done,
                    message,
                    already_signed_in,
                }
            }
        }
        _ => return Ok(None),
    }))
}

/// What starts a sign-in: Harness's own, on a lane of its own, by the
/// method asked (`--code` or `--qr`).
pub type Starter = Box<dyn FnMut(Method) -> Result<Box<dyn Link>, String> + Send>;

/// A running `harness login --code --json`: dropping it stops the program.
pub trait Link: Send {
    /// The next line read, if one has come: `None` while there is none yet.
    /// Once the program has ended, or wrote what can't be read, it gives
    /// `Some(Err(code))`, and nothing after.
    fn poll(&mut self) -> Option<Result<Event, &'static str>>;
    /// The person's answer to the question, as Harness reads it.
    fn answer(&mut self, yes: bool) -> std::io::Result<()>;
}

/// Where a sign-in is. Its `Debug` hides the digits and the email.
#[derive(Clone, PartialEq, Eq)]
pub enum State {
    /// Started; no code yet.
    Starting,
    /// The code to say, and when it stops being good.
    Code {
        code: String,
        until: Instant,
    },
    /// The QR's link for the app to show, and when it stops being good
    /// (#5198).
    Qr {
        url: String,
        until: Instant,
    },
    /// The person approved in Harness: the lamp asks whether to sign in as
    /// this email.
    Asking {
        email: String,
    },
    /// The person answered; Harness hasn't said how it went yet.
    Answered {
        yes: bool,
    },
    Done(Done),
}

/// One sign-in, from its start until it is done; the done state is kept
/// until another starts, for the voice to read.
pub struct Session {
    id: u64,
    link: Option<Box<dyn Link>>,
    state: State,
    until: Instant,
    /// The voice's request in which the question was heard whole, and
    /// when ([`Session::asked`]); for an app-started one, request 0 and
    /// when the app was first given the question ([`Session::app_view`]).
    told: Option<(u64, Instant)>,
    by: By,
    method: Method,
    /// How many times its state changed ([`Session::revision`]); never by
    /// a read.
    changes: u64,
    /// When the code stops being good, in unix ms, as the app counts down
    /// to it: set when Harness gives or extends it, never on a read.
    expires_at_ms: Option<u64>,
    /// Harness's own message with its result, and whether it said the lamp
    /// was already signed in (#5198).
    message: Option<String>,
    already_signed_in: bool,
    /// lamp-cloud's last put of this sign-in's code or question to the
    /// backend failed (#5198, [`Session::relayed`]).
    relay_failed: bool,
    /// The code already given by a start or initial claim. A same-code
    /// keepalive does not make another initial telling available.
    code_given: Option<String>,
    /// The last code Harness gave, kept until the sign-in ends so that a
    /// result after the question can't quote it to the app (#5579). Never
    /// said, journaled or shown: only matched against Harness's message.
    last_code: Option<String>,
    /// Harness gave or kept alive the QR link since the app was last given
    /// it (#5198, [`Session::app_view_claiming`]).
    qr_untold: bool,
}

impl Session {
    /// `id` tells this sign-in from the ones before it, for the voice's
    /// news.
    pub fn new(id: u64, link: Box<dyn Link>, now: Instant) -> Self {
        Self {
            id,
            link: Some(link),
            state: State::Starting,
            until: now + MOST_LIFE,
            told: None,
            code_given: None,
            last_code: None,
            by: By::Voice,
            method: Method::Code,
            changes: 0,
            expires_at_ms: None,
            message: None,
            already_signed_in: false,
            relay_failed: false,
            qr_untold: false,
        }
    }

    /// Who started it and how (#5198): a voice-started code unless said.
    pub fn started_by(mut self, by: By, method: Method) -> Self {
        self.by = by;
        self.method = method;
        self
    }

    pub fn by(&self) -> By {
        self.by
    }

    /// Bumped by every change of state and never by a read (#5198), and
    /// higher for each later sign-in: ids go up by one a sign-in, and a
    /// sign-in changes far fewer than 1000 times (a code is kept alive at
    /// most five times).
    pub fn revision(&self) -> u64 {
        self.id
            .saturating_mul(1000)
            .saturating_add(self.changes.min(999))
    }

    /// One that could not start: done, with why.
    pub fn failed(id: u64, code: &str) -> Self {
        Self {
            id,
            link: None,
            state: State::Done(Done::Error { code: code.into() }),
            until: Instant::now(),
            told: None,
            code_given: None,
            last_code: None,
            by: By::Voice,
            method: Method::Code,
            changes: 0,
            expires_at_ms: None,
            message: None,
            already_signed_in: false,
            relay_failed: false,
            qr_untold: false,
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    /// The code now, if any, was given in a start's answer
    /// (`sign_in_start`), which the voice says itself.
    pub fn code_given(&mut self) {
        if let State::Code { code, .. } = &self.state {
            self.code_given = Some(code.clone());
        }
    }

    /// Take this code's initial payload once, shared by the provider and
    /// coordinator through Tasks' serialized owner. A lost reply does not
    /// undo the claim; a new heard start can ask for the code again.
    pub(crate) fn take_code(&mut self, id: u64, now: Instant) -> Value {
        let unavailable = || json!({"claim": "not_available", "id": id});
        // An app-started sign-in's code is never the voice's to say.
        if id != self.id || now >= self.until || self.by == By::App {
            return unavailable();
        }
        match &self.state {
            State::Starting => json!({"claim": "waiting", "id": id}),
            State::Code { until, .. } if now >= *until => unavailable(),
            State::Code { code, .. } if self.code_given.as_ref() == Some(code) => {
                json!({"claim": "already_given", "id": id})
            }
            State::Code { .. } => {
                self.code_given();
                let mut view = self.view(now);
                view["claim"] = "granted".into();
                view
            }
            _ => unavailable(),
        }
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    /// Not yet done: another start gives this one back.
    pub fn open(&self) -> bool {
        !matches!(self.state, State::Done(_))
    }

    /// What has come from Harness since, and whether its time is up. Gives
    /// the journal's line for each change, with neither the digits nor the
    /// email.
    pub fn tend(&mut self, now: Instant) -> Vec<String> {
        let mut said = Vec::new();
        while let Some(read) = self.link.as_mut().and_then(|link| link.poll()) {
            match read {
                // A code after the question isn't one to say, nor one in a
                // QR sign-in.
                Ok(Event::Code { code, expires_in })
                    if self.method == Method::Code
                        && matches!(self.state, State::Starting | State::Code { .. }) =>
                {
                    // The same code kept alive is no change to say.
                    if !matches!(&self.state, State::Code { code: was, .. } if *was == code) {
                        said.push(format!("code given (good for {expires_in} s)"));
                    }
                    self.last_code = Some(code.clone());
                    self.state = State::Code {
                        code,
                        until: now + Duration::from_secs(expires_in),
                    };
                    self.changes += 1;
                    self.expires_at_ms = Some(unix_ms().saturating_add(expires_in * 1000));
                }
                Ok(Event::Code { .. }) => {}
                // The QR's link, as the code above: only in a QR sign-in,
                // before the question; the same link kept alive is no
                // change to say. Its key is never journalled. Each telling,
                // a new link or the same kept alive, is given to the app
                // once more, with its new `expires_at`.
                Ok(Event::Qr { url, expires_in })
                    if self.method == Method::Qr
                        && matches!(self.state, State::Starting | State::Qr { .. }) =>
                {
                    if !matches!(&self.state, State::Qr { url: was, .. } if *was == url) {
                        said.push(format!("QR given (good for {expires_in} s)"));
                    }
                    self.state = State::Qr {
                        url,
                        until: now + Duration::from_secs(expires_in),
                    };
                    self.qr_untold = true;
                    self.changes += 1;
                    self.expires_at_ms = Some(unix_ms().saturating_add(expires_in * 1000));
                }
                Ok(Event::Qr { .. }) => {}
                // Asked once: a question after it was answered is passed
                // over, so only one answer is ever written (Jarvis's P3).
                Ok(Event::Confirm { email })
                    if matches!(
                        self.state,
                        State::Starting | State::Code { .. } | State::Qr { .. }
                    ) =>
                {
                    said.push("approved in Harness: asking whether to sign in".into());
                    self.state = State::Asking { email };
                    self.changes += 1;
                }
                Ok(Event::Confirm { .. }) => {}
                Ok(Event::ResultWith {
                    done,
                    message,
                    already_signed_in,
                }) => {
                    // Not one that holds the sign-in's code's digits,
                    // whatever the spacing or separators (the privacy
                    // reader's P3 on #5236, the reviewer-lead's P2 on
                    // #5579), the question asked or not.
                    let code = self.last_code.as_deref().map(digits).unwrap_or_default();
                    self.message = message
                        .filter(|message| code.is_empty() || !digits(message).contains(&code));
                    self.already_signed_in = already_signed_in;
                    said.push(match &done {
                        Done::Success { .. } if already_signed_in => "already signed in".into(),
                        Done::Success { .. } => "signed in".into(),
                        Done::Error { code } => format!("not signed in: {code}"),
                    });
                    self.finish(match done {
                        Done::Success { .. } => Done::Success { email: None },
                        error => error,
                    });
                }
                Ok(Event::Result(done)) => {
                    said.push(match &done {
                        Done::Success { .. } => "signed in".into(),
                        Done::Error { code } => format!("not signed in: {code}"),
                    });
                    // The email is needed only for the question: none is kept
                    // after it (Jarvis's P3).
                    self.finish(match done {
                        Done::Success { .. } => Done::Success { email: None },
                        error => error,
                    });
                }
                Err(code) => {
                    said.push(format!("not signed in: {code}"));
                    self.finish(Done::Error { code: code.into() });
                }
            }
        }
        if self.open() && now >= self.until {
            said.push("not signed in: timeout".into());
            self.finish(Done::Error {
                code: "timeout".into(),
            });
        }
        // A question told and not answered in time can't be answered any
        // more: ended, so the person can start again (the reviewer's P3 on
        // #2174), not left open until `MOST_LIFE`.
        if matches!(self.state, State::Asking { .. })
            && self
                .told
                .is_some_and(|(_, at)| now.saturating_duration_since(at) > ANSWER_WINDOW)
        {
            said.push("not signed in: too_late".into());
            self.finish(Done::Error {
                code: "too_late".into(),
            });
        }
        // The code given is kept no longer than the code is (the
        // reviewer's P3).
        if !matches!(self.state, State::Code { .. }) {
            self.code_given = None;
        }
        // The same for the QR link (#5198): past it, nothing is left to give.
        if !matches!(self.state, State::Qr { .. }) {
            self.qr_untold = false;
        }
        said
    }

    /// Ends it: the code, the QR link and the email go with the state.
    fn finish(&mut self, done: Done) {
        self.state = State::Done(done);
        self.changes += 1;
        self.expires_at_ms = None;
        self.code_given = None;
        self.last_code = None;
        self.qr_untold = false;
        // Dropped: the program is stopped, if it hasn't ended itself.
        self.link = None;
    }

    /// The voice heard the question "Sign in as …?" said whole, in its
    /// `request`: only from then is an answer taken. The first telling
    /// counts; a question never said, or cut off, opens nothing. A telling
    /// in a lower request than the first is a restarted voice's, whose
    /// numbers began again: it replaces the first (the reviewer's P3).
    pub fn asked(&mut self, id: u64, request: u64, now: Instant) -> Result<(), &'static str> {
        if id != self.id {
            return Err("not_this_sign_in");
        }
        // Only the app answers an app-started sign-in (#5198).
        if self.by == By::App {
            return Err("started_in_app");
        }
        if !matches!(self.state, State::Asking { .. }) {
            return Err("not_asking");
        }
        match self.told {
            Some((told_in, _)) if request >= told_in => {}
            _ => self.told = Some((request, now)),
        }
        Ok(())
    }

    /// The person's answer, in the voice's `request`: taken once, only in a
    /// request after the one the question was told in, and within
    /// [`ANSWER_WINDOW`] of it. An answer too soon leaves the question open;
    /// one too late ends the sign-in, which can't be answered any more.
    pub fn answer(&mut self, yes: bool, request: u64, now: Instant) -> Result<(), &'static str> {
        if self.by == By::App {
            return Err("started_in_app");
        }
        if !matches!(self.state, State::Asking { .. }) {
            return Err("not_asking");
        }
        let (told_in, told_at) = self.told.ok_or("not_told")?;
        if request <= told_in {
            return Err("not_told");
        }
        self.pass_on(yes, told_at, now)
    }

    /// lamp-cloud's word on its last put of sign-in `id`'s code or
    /// question to the backend (#5198): a change of it is a change, with a
    /// new revision, and the sign-in itself goes on either way.
    pub fn relayed(&mut self, id: u64, delivered: bool) -> Result<(), &'static str> {
        if id != self.id {
            return Err("not_this_sign_in");
        }
        if self.relay_failed == delivered {
            self.relay_failed = !delivered;
            self.changes += 1;
        }
        Ok(())
    }

    /// The app's answer for sign-in `id` (#5198): only for one the app
    /// started, once the app was given its question ([`Session::app_view`]),
    /// within [`ANSWER_WINDOW`] of that, once.
    pub fn answer_from_app(
        &mut self,
        id: u64,
        yes: bool,
        now: Instant,
    ) -> Result<(), &'static str> {
        if id != self.id {
            return Err("not_this_sign_in");
        }
        if self.by != By::App {
            return Err("started_by_voice");
        }
        if !matches!(self.state, State::Asking { .. }) {
            return Err("not_asking");
        }
        let (_, told_at) = self.told.ok_or("not_read")?;
        self.pass_on(yes, told_at, now)
    }

    /// The answer written to Harness, unless its window has passed.
    fn pass_on(&mut self, yes: bool, told_at: Instant, now: Instant) -> Result<(), &'static str> {
        if now.saturating_duration_since(told_at) > ANSWER_WINDOW {
            self.finish(Done::Error {
                code: "too_late".into(),
            });
            return Err("too_late");
        }
        let link = self.link.as_mut().ok_or("not_asking")?;
        if link.answer(yes).is_err() {
            self.finish(Done::Error {
                code: "ended".into(),
            });
            return Err("ended");
        }
        self.state = State::Answered { yes };
        self.changes += 1;
        Ok(())
    }

    /// What the voice reads (`sign_in_state`). An app-started sign-in is
    /// only `in_app` here: never its code or email (#5198).
    pub fn view(&self, now: Instant) -> Value {
        if self.by == By::App {
            return json!({"sign_in": "in_app", "id": self.id});
        }
        let mut view = match &self.state {
            State::Starting => json!({"sign_in": "starting"}),
            State::Code { code, until } => json!({
                "sign_in": "code",
                "code": code,
                "expires_in": until.saturating_duration_since(now).as_secs(),
                "code_told": self.code_given.as_ref() == Some(code),
            }),
            // Only the app starts a QR sign-in, so the voice reads it as
            // the app's above; never its link.
            State::Qr { .. } => json!({"sign_in": "in_app"}),
            State::Asking { email } => json!({"sign_in": "asking", "email": email}),
            State::Answered { yes } => {
                json!({"sign_in": "answered", "answer": if *yes { "yes" } else { "no" }})
            }
            State::Done(Done::Success { .. }) => json!({"sign_in": "done", "result": "success"}),
            State::Done(Done::Error { code }) => {
                json!({"sign_in": "done", "result": "error", "code": code})
            }
        };
        view["id"] = self.id.into();
        view
    }

    /// What the app reads (#5198, `app_sign_in_*`): who started it and how,
    /// its code with `expires_in`, and its question. The email,
    /// and `answer_here`, only for one the app started: giving the app its
    /// question opens [`ANSWER_WINDOW`] for the app's answer, the first
    /// time. A voice-started one shows its code only, for the app to show.
    pub fn app_view(&mut self, now: Instant) -> Value {
        let mine = self.by == By::App;
        let mut view = match &self.state {
            State::Starting => json!({"sign_in": "starting"}),
            State::Code { code, until } => json!({
                "sign_in": "code",
                "code": code,
                "expires_in": until.saturating_duration_since(now).as_secs(),
                "expires_at": self.expires_at_ms,
            }),
            // The link itself only through [`Session::app_view_claiming`].
            State::Qr { until, .. } => json!({
                "sign_in": "qr",
                "expires_in": until.saturating_duration_since(now).as_secs(),
                "expires_at": self.expires_at_ms,
            }),
            State::Asking { email } if mine => {
                let told_at = *self.told.get_or_insert((0, now));
                json!({
                    "sign_in": "asking",
                    "email": email,
                    "answer_within": ANSWER_WINDOW
                        .saturating_sub(now.saturating_duration_since(told_at.1))
                        .as_secs(),
                })
            }
            State::Asking { .. } => json!({"sign_in": "asking"}),
            State::Answered { yes } => {
                json!({"sign_in": "answered", "answer": if *yes { "yes" } else { "no" }})
            }
            State::Done(Done::Success { .. }) if self.already_signed_in => json!({
                "sign_in": "done",
                "result": "success",
                "already_signed_in": true,
                "message": "This lamp is already signed in to Harness.",
            }),
            State::Done(Done::Success { .. }) => json!({"sign_in": "done", "result": "success"}),
            State::Done(Done::Error { code }) => json!({
                "sign_in": "done",
                "result": "error",
                "error": code,
                "message": self.message.as_deref().unwrap_or(match (self.method, code.as_str()) {
                    (Method::Qr, "EXPIRED") => QR_EXPIRED,
                    _ => done_message(code),
                }),
            }),
        };
        if self.relay_failed && self.open() {
            view["error"] = "relay_failed".into();
            view["message"] = RELAY_FAILED.into();
        }
        view["id"] = self.id.into();
        view["revision"] = self.revision().into();
        view["by"] = self.by.name().into();
        view["method"] = self.method.name().into();
        view["answer_here"] = (mine && matches!(self.state, State::Asking { .. })).into();
        view
    }

    /// [`Session::app_view`], with the QR link as `qr_url` once per
    /// telling (#5198): the claim of the link, as [`Session::take_code`] is
    /// the voice's of the code. Only the app's start, state and answer
    /// reads claim it, which lamp-cloud hands to the backend over HTTPS;
    /// its relay word does not, so a reply nobody hands on never spends it.
    pub fn app_view_claiming(&mut self, now: Instant) -> Value {
        let mut view = self.app_view(now);
        if let State::Qr { url, until } = &self.state
            && self.qr_untold
            && self.by == By::App
            && now < *until
            && now < self.until
        {
            view["qr_url"] = url.as_str().into();
            self.qr_untold = false;
        }
        view
    }
}

/// What the app shows for a QR sign-in that ran out of time (#5198).
pub const QR_EXPIRED: &str = "The QR code ran out of time. Start again for a new one.";

/// What the app shows when lamp-cloud's put of the code or the question
/// to the backend failed (#5198).
pub const RELAY_FAILED: &str = "The sign-in code couldn't reach the app. Try again.";

/// Now, in unix ms.
fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// What the app shows for a sign-in that ended with `code` and no message
/// of Harness's own (#5198).
pub fn done_message(code: &str) -> &'static str {
    match code {
        "EXPIRED" => "The code ran out of time. Start again for a new code.",
        "DENIED" => "The sign-in was not approved.",
        "CANCELLED" => "The sign-in was cancelled.",
        "BACKEND_ERROR" => "Harness could not be reached. Try again in a moment.",
        "DAEMON_BUSY" => "Harness on the lamp is busy. Try again in a moment.",
        "DAEMON_DID_NOT_STOP" => {
            "Harness on the lamp could not get ready to sign in. Try again in a moment."
        }
        "harness_too_old" => "Harness on the lamp needs updating first.",
        "no_harness" => "Harness is not installed on this lamp.",
        "timeout" => "The sign-in took too long. Start again.",
        "too_late" => "The question went unanswered for two minutes. Start again.",
        _ => "The sign-in stopped. Try again.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

    type Script = Arc<Mutex<VecDeque<Option<Result<Event, &'static str>>>>>;

    /// Harness's lines, as a script, and the answers written.
    struct Scripted {
        lines: Script,
        answers: Arc<Mutex<Vec<bool>>>,
    }
    impl Link for Scripted {
        fn poll(&mut self) -> Option<Result<Event, &'static str>> {
            self.lines.lock().unwrap().pop_front().flatten()
        }
        fn answer(&mut self, yes: bool) -> std::io::Result<()> {
            self.answers.lock().unwrap().push(yes);
            Ok(())
        }
    }

    fn scripted(now: Instant) -> (Session, Script, Arc<Mutex<Vec<bool>>>) {
        let lines = Script::default();
        let answers = Arc::new(Mutex::new(Vec::new()));
        let link = Scripted {
            lines: lines.clone(),
            answers: answers.clone(),
        };
        (Session::new(1, Box::new(link), now), lines, answers)
    }

    fn code(code: &str) -> Option<Result<Event, &'static str>> {
        Some(Ok(Event::Code {
            code: code.into(),
            expires_in: 120,
        }))
    }

    const QR_URL: &str = "https://harness.autonomous.ai/signin#k=hnq_abcDEF123";

    fn qr(url: &str) -> Option<Result<Event, &'static str>> {
        Some(Ok(Event::Qr {
            url: url.into(),
            expires_in: 120,
        }))
    }

    /// #5198: `harness login --qr --json`'s link line is read only when it
    /// is Harness's sign-in page with a key, bounded and plain, and its
    /// `Debug` hides the link.
    #[test]
    fn a_qr_line_is_read_only_as_harness_sign_in_page_and_its_debug_hides_it() {
        let line = |url: &str, expires_in: u64| {
            json!({"type": "qr", "url": url, "expiresIn": expires_in}).to_string()
        };
        let read = parse_line(&line(QR_URL, 120)).unwrap().unwrap();
        assert_eq!(
            read,
            Event::Qr {
                url: QR_URL.into(),
                expires_in: 120
            }
        );
        assert!(!format!("{read:?}").contains("hnq_"), "{read:?}");
        let long = format!("{QR_PREFIX}{}", "a".repeat(600));
        for (url, expires_in) in [
            ("https://evil.example/signin#k=hnq_abc", 120),
            ("http://harness.autonomous.ai/signin#k=hnq_abc", 120),
            (QR_PREFIX, 120),
            ("https://harness.autonomous.ai/signin#k=hnq abc", 120),
            (long.as_str(), 120),
            (QR_URL, 0),
            (QR_URL, 601),
        ] {
            assert_eq!(
                parse_line(&line(url, expires_in)),
                Err("unreadable"),
                "{url}"
            );
        }
    }

    /// #5198: an app-started QR sign-in shows its link to the app alone,
    /// as `qr_url` with `expires_at`; the voice reads it as the app's and
    /// can't claim it; a keepalive is no new change to say; then the same
    /// question and answer as a code. A code line in it, or a link in a
    /// code sign-in, is passed over. No journal line holds the link.
    #[test]
    fn an_app_qr_sign_in_shows_its_link_to_the_app_only() {
        let now = Instant::now();
        let (session, lines, answers) = scripted(now);
        let mut session = session.started_by(By::App, Method::Qr);
        lines.lock().unwrap().push_back(code("12345678"));
        lines.lock().unwrap().push_back(qr(QR_URL));
        let said = session.tend(now);
        assert_eq!(said, ["QR given (good for 120 s)"]);
        let app = session.app_view_claiming(now + Duration::from_secs(1));
        assert_eq!(app["sign_in"], "qr");
        assert_eq!(app["method"], "qr");
        assert_eq!(app["qr_url"], QR_URL);
        assert_eq!(app["expires_in"], 119);
        assert!(app["expires_at"].as_u64().is_some());
        assert!(app.get("code").is_none());
        assert_eq!(session.view(now), json!({"sign_in": "in_app", "id": 1}));
        assert_eq!(
            session.take_code(1, now),
            json!({"claim": "not_available", "id": 1})
        );
        let revision = session.revision();
        lines.lock().unwrap().push_back(qr(QR_URL));
        assert!(session.tend(now + Duration::from_secs(60)).is_empty());
        assert!(session.revision() > revision, "the extension is a change");
        lines.lock().unwrap().push_back(Some(Ok(Event::Confirm {
            email: "someone@example.com".into(),
        })));
        let said = session.tend(now + Duration::from_secs(61));
        assert_eq!(said, ["approved in Harness: asking whether to sign in"]);
        let asking = session.app_view(now + Duration::from_secs(62));
        assert_eq!(asking["sign_in"], "asking");
        assert_eq!(asking["answer_here"], true);
        assert!(asking.get("qr_url").is_none());
        session
            .answer_from_app(1, true, now + Duration::from_secs(63))
            .unwrap();
        assert_eq!(*answers.lock().unwrap(), [true]);
        // A code sign-in passes a link over.
        let (code_session, lines, _) = scripted(now);
        let mut code_session = code_session.started_by(By::App, Method::Code);
        lines.lock().unwrap().push_back(qr(QR_URL));
        assert!(code_session.tend(now).is_empty());
        assert_eq!(code_session.app_view(now)["sign_in"], "starting");
    }

    /// #5198: a QR sign-in that ran out of time says so as a QR, and a
    /// Harness message quoting a link's key is never kept for the app.
    #[test]
    fn a_qr_that_ran_out_says_so_and_a_message_with_its_key_is_dropped() {
        let now = Instant::now();
        let (session, lines, _) = scripted(now);
        let mut session = session.started_by(By::App, Method::Qr);
        lines.lock().unwrap().push_back(qr(QR_URL));
        lines
            .lock()
            .unwrap()
            .push_back(Some(Ok(Event::Result(Done::Error {
                code: "EXPIRED".into(),
            }))));
        session.tend(now);
        let done = session.app_view(now);
        assert_eq!(done["error"], "EXPIRED");
        assert_eq!(done["message"], QR_EXPIRED);
        assert!(done.get("qr_url").is_none());
        let quoted = json!({"type": "result", "status": "error", "code": "DENIED",
            "message": "Not approved: https://harness.autonomous.ai/signin#k=hnq_abcDEF123"})
        .to_string();
        assert_eq!(
            parse_line(&quoted).unwrap(),
            Some(Event::Result(Done::Error {
                code: "DENIED".into()
            }))
        );
    }

    #[test]
    fn the_initial_code_is_claimed_once_and_a_keepalive_does_not_reopen_it() {
        let now = Instant::now();
        let (mut session, lines, _) = scripted(now);
        assert_eq!(
            session.take_code(1, now),
            json!({"claim": "waiting", "id": 1})
        );
        lines.lock().unwrap().push_back(code("12345678"));
        session.tend(now);
        let snapshot = session.view(now);
        assert_eq!(snapshot["code_told"], false);
        assert_eq!(session.view(now), snapshot, "state reads do not claim");
        assert_eq!(
            session.take_code(1, now + Duration::from_secs(1)),
            json!({"claim": "granted", "sign_in": "code", "id": 1,
                "code": "12345678", "expires_in": 119, "code_told": true})
        );
        assert_eq!(
            session.take_code(1, now + Duration::from_secs(2)),
            json!({"claim": "already_given", "id": 1})
        );
        lines.lock().unwrap().push_back(code("12345678"));
        session.tend(now + Duration::from_secs(60));
        assert_eq!(
            session.take_code(1, now + Duration::from_secs(121)),
            json!({"claim": "already_given", "id": 1})
        );
        assert_eq!(
            session.view(now + Duration::from_secs(121))["expires_in"],
            59
        );
        lines.lock().unwrap().push_back(code("87654321"));
        session.tend(now + Duration::from_secs(122));
        let next = session.take_code(1, now + Duration::from_secs(123));
        assert_eq!(next["claim"], "granted");
        assert_eq!(next["code"], "87654321");
        assert_eq!(next["expires_in"], 119);
    }

    #[test]
    fn a_legacy_start_and_a_code_claim_share_the_initial_telling() {
        let now = Instant::now();
        let (mut session, lines, _) = scripted(now);
        lines.lock().unwrap().push_back(code("12345678"));
        session.tend(now);
        session.code_given();
        assert_eq!(
            session.take_code(1, now),
            json!({"claim": "already_given", "id": 1})
        );
        // An explicit later start still returns the legacy code view.
        session.code_given();
        assert_eq!(session.view(now)["code"], "12345678");
        assert_eq!(session.view(now)["code_told"], true);
    }

    #[test]
    fn a_stale_or_expired_claim_contains_no_code_and_cannot_spend_a_new_code() {
        let now = Instant::now();
        let (mut session, lines, _) = scripted(now);
        lines.lock().unwrap().push_back(code("12345678"));
        session.tend(now);
        assert_eq!(
            session.take_code(2, now),
            json!({"claim": "not_available", "id": 2})
        );
        assert_eq!(session.view(now)["code_told"], false);
        assert_eq!(
            session.take_code(1, now + Duration::from_secs(120)),
            json!({"claim": "not_available", "id": 1})
        );
        assert_eq!(session.view(now)["code_told"], false);
        lines.lock().unwrap().push_back(code("87654321"));
        session.tend(now + Duration::from_secs(121));
        let taken = session.take_code(1, now + Duration::from_secs(122));
        assert_eq!(taken["claim"], "granted");
        assert_eq!(taken["code"], "87654321");
        assert_eq!(taken["expires_in"], 119);
        assert_eq!(
            session.take_code(1, now + MOST_LIFE),
            json!({"claim": "not_available", "id": 1})
        );
    }

    #[test]
    fn a_code_claim_never_acknowledges_a_question_or_returns_its_email() {
        let now = Instant::now();
        let (mut session, lines, answers) = scripted(now);
        lines.lock().unwrap().push_back(code("12345678"));
        session.tend(now);
        assert_eq!(session.take_code(1, now)["claim"], "granted");
        lines.lock().unwrap().push_back(Some(Ok(Event::Confirm {
            email: "someone@example.com".into(),
        })));
        session.tend(now);
        assert_eq!(
            session.take_code(1, now),
            json!({"claim": "not_available", "id": 1})
        );
        assert_eq!(session.answer(true, 2, now), Err("not_told"));
        assert!(answers.lock().unwrap().is_empty());
        session.asked(1, 2, now).unwrap();
        session.answer(true, 3, now).unwrap();
        assert_eq!(*answers.lock().unwrap(), [true]);
        assert_eq!(
            session.take_code(1, now),
            json!({"claim": "not_available", "id": 1})
        );
        lines
            .lock()
            .unwrap()
            .push_back(Some(Ok(Event::Result(Done::Success {
                email: Some("someone@example.com".into()),
            }))));
        session.tend(now);
        assert_eq!(
            session.take_code(1, now),
            json!({"claim": "not_available", "id": 1})
        );
    }

    /// The lines Harness's team gave on openharness#1410 (20 October).
    #[test]
    fn harness_sign_in_lines_are_read_strictly() {
        assert_eq!(
            parse_line(r#"{"type":"code","code":"12345678","expiresIn":120}"#),
            Ok(Some(Event::Code {
                code: "12345678".into(),
                expires_in: 120
            }))
        );
        assert_eq!(
            parse_line(r#"{"type":"confirm","email":"someone@example.com"}"#),
            Ok(Some(Event::Confirm {
                email: "someone@example.com".into()
            }))
        );
        assert_eq!(
            parse_line(r#"{"type":"result","status":"success","email":"someone@example.com"}"#),
            Ok(Some(Event::Result(Done::Success {
                email: Some("someone@example.com".into())
            })))
        );
        assert_eq!(
            parse_line(r#"{"type":"result","status":"error","code":"EXPIRED"}"#),
            Ok(Some(Event::Result(Done::Error {
                code: "EXPIRED".into()
            })))
        );
        // A code Harness may add is kept; one that isn't a code is not.
        assert_eq!(
            parse_line(r#"{"type":"result","status":"error","code":"rate limited!"}"#),
            Ok(Some(Event::Result(Done::Error {
                code: "BACKEND_ERROR".into()
            })))
        );
        // A kind this part doesn't know is passed over.
        assert_eq!(parse_line(r#"{"type":"progress","step":2}"#), Ok(None));
        for bad in [
            r#"{"type":"code","code":"12a45678","expiresIn":120}"#,
            r#"{"type":"code","code":"123","expiresIn":120}"#,
            r#"{"type":"code","code":"12345678","expiresIn":0}"#,
            r#"{"type":"code","code":"12345678","expiresIn":601}"#,
            r#"{"type":"code","code":"12345678"}"#,
            r#"{"type":"confirm","email":"no at sign"}"#,
            r#"{"type":"confirm","email":"a@b\nc"}"#,
            r#"{"type":"confirm"}"#,
            r#"{"type":"result","status":"maybe"}"#,
            r#"{"code":"12345678"}"#,
            "not json",
        ] {
            assert_eq!(parse_line(bad), Err("unreadable"), "{bad}");
        }
        assert_eq!(
            parse_line(&format!(
                r#"{{"type":"confirm","email":"{}@example.com"}}"#,
                "a".repeat(MOST_LINE_BYTES)
            )),
            Err("unreadable")
        );
    }

    /// The whole sign-in: the code, kept alive with the same code, the
    /// question, the yes written, and Harness's result. The journal's lines
    /// hold neither the digits nor the email.
    #[test]
    fn a_sign_in_goes_from_its_code_to_the_question_to_the_answer() {
        let now = Instant::now();
        let (mut session, lines, answers) = scripted(now);
        assert_eq!(session.view(now), json!({"sign_in": "starting", "id": 1}));
        assert_eq!(session.answer(true, 1, now), Err("not_asking"));
        lines.lock().unwrap().extend([code("12345678"), None]);
        let said = session.tend(now);
        assert_eq!(said, ["code given (good for 120 s)"]);
        assert_eq!(
            session.view(now + Duration::from_secs(20)),
            json!({"sign_in": "code", "code": "12345678", "expires_in": 100, "code_told": false, "id": 1})
        );
        // Given in a start's answer: the voice said it.
        session.code_given();
        assert_eq!(session.view(now)["code_told"], true);
        // Kept alive: the same code, nothing new to say.
        lines.lock().unwrap().extend([code("12345678"), None]);
        assert!(session.tend(now + Duration::from_secs(60)).is_empty());
        lines.lock().unwrap().extend([
            Some(Ok(Event::Confirm {
                email: "someone@example.com".into(),
            })),
            None,
        ]);
        let said = session.tend(now + Duration::from_secs(90));
        assert_eq!(said, ["approved in Harness: asking whether to sign in"]);
        assert_eq!(
            session.view(now),
            json!({"sign_in": "asking", "email": "someone@example.com", "id": 1})
        );
        assert_eq!(session.asked(1, 4, now), Ok(()));
        assert_eq!(session.answer(true, 5, now), Ok(()));
        assert_eq!(*answers.lock().unwrap(), [true]);
        assert_eq!(session.view(now)["answer"], "yes");
        // Only one answer.
        assert_eq!(session.answer(false, 6, now), Err("not_asking"));
        lines.lock().unwrap().extend([
            Some(Ok(Event::Result(Done::Success {
                email: Some("someone@example.com".into()),
            }))),
            None,
        ]);
        let said = session.tend(now + Duration::from_secs(95));
        assert_eq!(said, ["signed in"]);
        assert!(!session.open());
        // Done: the email is kept no longer than the question.
        assert_eq!(
            session.view(now),
            json!({"sign_in": "done", "result": "success", "id": 1})
        );
        assert_eq!(session.state(), &State::Done(Done::Success { email: None }));
        for line in ["code given (good for 120 s)", "signed in"] {
            assert!(!line.contains("1234") && !line.contains('@'));
        }
    }

    /// A program that ends, or says what can't be read, ends the sign-in
    /// with why; one past its time is stopped. A no is written as asked.
    #[test]
    fn a_sign_in_ends_with_why_when_harness_stops_or_its_time_is_up() {
        let now = Instant::now();
        let (mut session, lines, _) = scripted(now);
        lines
            .lock()
            .unwrap()
            .push_back(Some(Err("harness_too_old")));
        assert_eq!(session.tend(now), ["not signed in: harness_too_old"]);
        assert_eq!(
            session.view(now),
            json!({"sign_in": "done", "result": "error", "code": "harness_too_old", "id": 1})
        );
        let (mut session, lines, answers) = scripted(now);
        lines.lock().unwrap().extend([
            Some(Ok(Event::Confirm {
                email: "someone@example.com".into(),
            })),
            None,
        ]);
        session.tend(now);
        session.asked(1, 1, now).unwrap();
        assert_eq!(session.answer(false, 2, now), Ok(()));
        assert_eq!(*answers.lock().unwrap(), [false]);
        lines.lock().unwrap().extend([
            Some(Ok(Event::Result(Done::Error {
                code: "DENIED".into(),
            }))),
            None,
        ]);
        assert_eq!(session.tend(now), ["not signed in: DENIED"]);
        let (mut session, _, _) = scripted(now);
        assert!(
            session
                .tend(now + MOST_LIFE - Duration::from_secs(1))
                .is_empty()
        );
        assert_eq!(session.tend(now + MOST_LIFE), ["not signed in: timeout"]);
        assert!(!session.open());
        assert_eq!(
            Session::failed(3, "no_harness").view(now)["code"],
            "no_harness"
        );
    }

    /// Jarvis's P3s on #2131: a question after the answer is passed over, so
    /// only one answer is ever written; and no `{:?}` prints the digits or
    /// the email.
    #[test]
    fn one_answer_only_and_no_debug_print_holds_the_code_or_the_email() {
        let now = Instant::now();
        let (mut session, lines, answers) = scripted(now);
        let confirm = || {
            Some(Ok(Event::Confirm {
                email: "someone@example.com".into(),
            }))
        };
        lines
            .lock()
            .unwrap()
            .extend([code("12345678"), confirm(), None]);
        session.tend(now);
        session.asked(1, 1, now).unwrap();
        assert_eq!(session.answer(true, 2, now), Ok(()));
        lines.lock().unwrap().extend([confirm(), None]);
        assert!(session.tend(now).is_empty());
        assert_eq!(session.state(), &State::Answered { yes: true });
        assert_eq!(session.answer(true, 3, now), Err("not_asking"));
        assert_eq!(*answers.lock().unwrap(), [true]);
        let printed = format!(
            "{:?} {:?} {:?} {:?} {:?}",
            Event::Code {
                code: "12345678".into(),
                expires_in: 120
            },
            Event::Confirm {
                email: "someone@example.com".into()
            },
            State::Code {
                code: "12345678".into(),
                until: now
            },
            State::Asking {
                email: "someone@example.com".into()
            },
            Done::Success {
                email: Some("someone@example.com".into())
            },
        );
        assert!(
            !printed.contains("1234") && !printed.contains("example") && !printed.contains('@'),
            "{printed}"
        );
    }

    /// #2137's redesign: an answer is taken only after the voice heard the
    /// question whole, in a later request, within two minutes, once. One
    /// too soon doesn't spend the question; one for another sign-in tells
    /// nothing; a restarted voice's telling replaces the first. Past the
    /// window, the sign-in ends, by an answer or by the clock.
    #[test]
    fn an_answer_is_taken_only_in_a_later_request_soon_after_the_question_was_told() {
        let now = Instant::now();
        let confirm = || {
            Some(Ok(Event::Confirm {
                email: "someone@example.com".into(),
            }))
        };
        let (mut session, lines, answers) = scripted(now);
        lines.lock().unwrap().extend([code("12345678"), None]);
        session.tend(now);
        // Told before Harness asks: nothing is opened.
        assert_eq!(session.asked(1, 3, now), Err("not_asking"));
        lines.lock().unwrap().extend([confirm(), None]);
        session.tend(now);
        // Not told yet: a yes heard is for something else.
        assert_eq!(session.answer(true, 9, now), Err("not_told"));
        assert_eq!(session.asked(2, 4, now), Err("not_this_sign_in"));
        assert_eq!(session.answer(true, 9, now), Err("not_told"));
        assert_eq!(session.asked(1, 4, now), Ok(()));
        // A later telling doesn't move the question on.
        assert_eq!(session.asked(1, 7, now + Duration::from_secs(60)), Ok(()));
        // The request the question was told in, or one before it.
        assert_eq!(session.answer(true, 4, now), Err("not_told"));
        assert_eq!(session.answer(true, 3, now), Err("not_told"));
        assert!(answers.lock().unwrap().is_empty());
        assert_eq!(
            session.state(),
            &State::Asking {
                email: "someone@example.com".into()
            }
        );
        // Refusals spent nothing: the next request in time, at the window's
        // very edge, is taken, once.
        assert!(session.tend(now + ANSWER_WINDOW).is_empty());
        assert_eq!(session.answer(false, 5, now + ANSWER_WINDOW), Ok(()));
        assert_eq!(
            session.answer(true, 6, now + ANSWER_WINDOW),
            Err("not_asking")
        );
        assert_eq!(*answers.lock().unwrap(), [false]);
        assert_eq!(session.view(now)["id"], 1);

        // A voice restarted while Harness asks: its numbers began again,
        // and its telling counts in place of the first.
        let (mut session, lines, answers) = scripted(now);
        lines.lock().unwrap().extend([confirm(), None]);
        session.tend(now);
        session.asked(1, 40, now).unwrap();
        assert_eq!(session.answer(true, 3, now), Err("not_told"));
        session.asked(1, 2, now + Duration::from_secs(5)).unwrap();
        assert_eq!(
            session.answer(true, 3, now + Duration::from_secs(6)),
            Ok(())
        );
        assert_eq!(*answers.lock().unwrap(), [true]);

        // Past the window, by an answer: ended, and said why.
        let (mut session, lines, answers) = scripted(now);
        lines.lock().unwrap().extend([confirm(), None]);
        session.tend(now);
        session.asked(1, 4, now).unwrap();
        let late = now + ANSWER_WINDOW + Duration::from_secs(1);
        assert_eq!(session.answer(true, 5, late), Err("too_late"));
        assert!(answers.lock().unwrap().is_empty());
        assert_eq!(session.view(late)["code"], "too_late");
        assert!(!session.open());
        // And by the clock, with no answer at all.
        let (mut session, lines, _) = scripted(now);
        lines.lock().unwrap().extend([confirm(), None]);
        session.tend(now);
        session.asked(1, 4, now).unwrap();
        assert_eq!(session.tend(late), ["not signed in: too_late"]);
        assert!(!session.open());
    }

    /// #5198: Harness's own message and `alreadySignedIn` are kept for the
    /// app (a message holding an address is not); a kind this part doesn't
    /// run is passed over.
    #[test]
    fn a_result_keeps_harness_message_and_already_signed_in_for_the_app() {
        assert_eq!(
            parse_line(r#"{"type":"result","status":"success","alreadySignedIn":true}"#),
            Ok(Some(Event::ResultWith {
                done: Done::Success { email: None },
                message: None,
                already_signed_in: true,
            }))
        );
        assert_eq!(
            parse_line(
                r#"{"type":"result","status":"error","code":"DENIED","message":"Too many wrong codes. Try again in an hour."}"#
            ),
            Ok(Some(Event::ResultWith {
                done: Done::Error {
                    code: "DENIED".into()
                },
                message: Some("Too many wrong codes. Try again in an hour.".into()),
                already_signed_in: false,
            }))
        );
        assert_eq!(
            parse_line(
                r#"{"type":"result","status":"error","code":"DENIED","message":"no for someone@example.com"}"#
            ),
            Ok(Some(Event::Result(Done::Error {
                code: "DENIED".into()
            })))
        );
        // A quoted code is dropped too (the privacy reader's P3 on #5236):
        // eight digits or more, but not fewer.
        for quoted in ["Code 12345678 has expired.", "12345678", "Code (12345678)."] {
            assert_eq!(
                parse_line(
                    &json!({"type":"result","status":"error","code":"EXPIRED","message":quoted})
                        .to_string()
                ),
                Ok(Some(Event::Result(Done::Error {
                    code: "EXPIRED".into()
                }))),
                "{quoted}"
            );
        }
        // Digits only (the reviewer-lead's P2 on #5579): eight digits
        // spaced, dashed or split by any separator are dropped too.
        for quoted in [
            "Code 1234 5678 has expired.",
            "Code 1234-5678.",
            "Code 12.34.56.78",
        ] {
            assert_eq!(
                parse_line(
                    &json!({"type":"result","status":"error","code":"EXPIRED","message":quoted})
                        .to_string()
                ),
                Ok(Some(Event::Result(Done::Error {
                    code: "EXPIRED".into()
                }))),
                "{quoted}"
            );
        }
        for kept in ["Try again in 60 minutes.", "Reference 1234567."] {
            assert!(
                matches!(
                    parse_line(
                        &json!({"type":"result","status":"error","code":"DENIED","message":kept})
                            .to_string()
                    ),
                    Ok(Some(Event::ResultWith {
                        message: Some(_),
                        ..
                    }))
                ),
                "{kept}"
            );
        }
        // A kind this part doesn't know is passed over.
        assert_eq!(
            parse_line(r#"{"type":"browser","url":"https://example.com/","expiresIn":120}"#),
            Ok(None)
        );
        let now = Instant::now();
        let (session, lines, _) = scripted(now);
        let mut session = session.started_by(By::App, Method::Code);
        lines.lock().unwrap().push_back(Some(Ok(Event::ResultWith {
            done: Done::Success { email: None },
            message: None,
            already_signed_in: true,
        })));
        let before = session.revision();
        assert_eq!(session.tend(now), ["already signed in"]);
        let view = session.app_view(now);
        assert_eq!(view["already_signed_in"], true);
        assert_eq!(view["result"], "success");
        assert!(view["revision"].as_u64().unwrap() > before);
        assert_eq!(session.app_view(now)["revision"], view["revision"]);
        assert_eq!(session.view(now), json!({"sign_in": "in_app", "id": 1}));

        // A message quoting the open code, whatever its shape, is not kept
        // (the privacy reader's P3 on #5236); the app shows its own words.
        let (session, lines, _) = scripted(now);
        let mut session = session.started_by(By::App, Method::Code);
        lines.lock().unwrap().extend([
            code("AB12CD"),
            Some(Ok(Event::ResultWith {
                done: Done::Error {
                    code: "EXPIRED".into(),
                },
                message: Some("Code AB12CD has expired.".into()),
                already_signed_in: false,
            })),
        ]);
        session.tend(now);
        let view = session.app_view(now);
        assert_eq!(view["result"], "error");
        assert!(!view.to_string().contains("AB12CD"), "{view}");

        // The PM's privacy rule: no journal line of an app-started sign-in,
        // nor a refusal, holds the code or the email.
        let (session, lines, _) = scripted(now);
        let mut session = session.started_by(By::App, Method::Code);
        lines.lock().unwrap().extend([
            code("12345678"),
            Some(Ok(Event::Confirm {
                email: "someone@example.com".into(),
            })),
        ]);
        let mut said = session.tend(now);
        said.push(format!("{:?}", session.answer_from_app(1, true, now)));
        session.app_view(now);
        said.push(format!("{:?}", session.answer_from_app(2, true, now)));
        said.push(format!("{:?}", session.answer(true, 9, now)));
        said.push(format!("{:?}", session.state()));
        session.answer_from_app(1, false, now).unwrap();
        lines
            .lock()
            .unwrap()
            .push_back(Some(Ok(Event::Result(Done::Error {
                code: "DENIED".into(),
            }))));
        said.extend(session.tend(now));
        assert!(said.len() >= 5, "{said:?}");
        for line in &said {
            assert!(!line.contains("1234") && !line.contains('@'), "{line}");
        }
    }

    fn app_qr(now: Instant) -> (Session, Script, Arc<Mutex<Vec<bool>>>) {
        let (session, lines, answers) = scripted(now);
        (session.started_by(By::App, Method::Qr), lines, answers)
    }

    /// #5198: `harness login --qr --json`'s link is read strictly: https
    /// only, no space or control in it, a time as the code's; a message
    /// holding a link is not kept.
    #[test]
    fn harness_qr_lines_are_read_strictly() {
        assert_eq!(
            parse_line(&format!(
                r#"{{"type":"qr","url":"{QR_URL}","expiresIn":120}}"#
            )),
            Ok(Some(Event::Qr {
                url: QR_URL.into(),
                expires_in: 120
            }))
        );
        for bad in [
            r#"{"type":"qr","url":"http://harness.autonomous.ai/signin#k=x","expiresIn":120}"#,
            r#"{"type":"qr","url":"https://","expiresIn":120}"#,
            r#"{"type":"qr","url":"https://a b","expiresIn":120}"#,
            r#"{"type":"qr","url":"https://a\nb","expiresIn":120}"#,
            r#"{"type":"qr","url":"https://a","expiresIn":0}"#,
            r#"{"type":"qr","url":"https://a","expiresIn":601}"#,
            r#"{"type":"qr","url":"https://a"}"#,
            r#"{"type":"qr","expiresIn":120}"#,
        ] {
            assert_eq!(parse_line(bad), Err("unreadable"), "{bad}");
        }
        assert_eq!(
            parse_line(
                r#"{"type":"result","status":"error","code":"EXPIRED","message":"Scan https://x#k=y"}"#
            ),
            Ok(Some(Event::Result(Done::Error {
                code: "EXPIRED".into()
            })))
        );
        for quoted in ["Key hnq_secret0123 has expired.", "Link #k=abc expired."] {
            assert_eq!(
                parse_line(
                    &json!({"type":"result","status":"error","code":"EXPIRED","message":quoted})
                        .to_string()
                ),
                Ok(Some(Event::Result(Done::Error {
                    code: "EXPIRED".into()
                }))),
                "{quoted}"
            );
        }
    }

    /// #5198: a QR sign-in started in the app goes from its link to the
    /// question to the answer, as the code's does; the link's
    /// `expires_at` counts down as the code's; the voice sees `in_app`.
    #[test]
    fn a_qr_sign_in_goes_from_its_link_to_the_question_to_the_answer() {
        let now = Instant::now();
        let (mut session, lines, answers) = app_qr(now);
        assert_eq!(session.app_view_claiming(now)["sign_in"], "starting");
        lines.lock().unwrap().extend([qr(QR_URL), None]);
        assert_eq!(session.tend(now), ["QR given (good for 120 s)"]);
        let shown = session.app_view_claiming(now + Duration::from_secs(20));
        assert_eq!(shown["sign_in"], "qr");
        assert_eq!(shown["method"], "qr");
        assert_eq!(shown["by"], "app");
        assert_eq!(shown["qr_url"], QR_URL);
        assert_eq!(shown["expires_in"], 100);
        assert!(shown["expires_at"].as_u64().unwrap() > 1_700_000_000_000);
        assert!(shown.get("code").is_none());
        assert_eq!(session.view(now), json!({"sign_in": "in_app", "id": 1}));
        assert_eq!(
            session.take_code(1, now),
            json!({"claim": "not_available", "id": 1})
        );
        lines.lock().unwrap().extend([
            Some(Ok(Event::Confirm {
                email: "someone@example.com".into(),
            })),
            None,
        ]);
        assert_eq!(
            session.tend(now),
            ["approved in Harness: asking whether to sign in"]
        );
        assert_eq!(session.answer_from_app(1, true, now), Err("not_read"));
        let asking = session.app_view_claiming(now);
        assert_eq!(asking["sign_in"], "asking");
        assert_eq!(asking["email"], "someone@example.com");
        assert!(asking.get("qr_url").is_none());
        assert_eq!(session.answer_from_app(1, true, now), Ok(()));
        assert_eq!(*answers.lock().unwrap(), [true]);
        lines
            .lock()
            .unwrap()
            .extend([Some(Ok(Event::Result(Done::Success { email: None }))), None]);
        assert_eq!(session.tend(now), ["signed in"]);
        let done = session.app_view_claiming(now);
        assert_eq!(done["result"], "success");
        assert!(done.get("qr_url").is_none());

        // Ran out of time: the app's words say the QR, not the code.
        let (mut session, lines, _) = app_qr(now);
        lines.lock().unwrap().extend([
            qr(QR_URL),
            Some(Ok(Event::Result(Done::Error {
                code: "EXPIRED".into(),
            }))),
        ]);
        session.tend(now);
        assert_eq!(session.app_view(now)["message"], QR_EXPIRED);
    }

    /// #5198, the PM's privacy rule for the QR: the link is given in one
    /// app view per telling (again only when Harness tells it again), never
    /// in a plain read, a `Debug`, a journal line, the voice's view or a
    /// refusal, and is gone once the sign-in ends.
    #[test]
    fn the_qr_link_is_claimed_once_and_never_printed_or_kept() {
        let now = Instant::now();
        let (mut session, lines, _) = app_qr(now);
        lines.lock().unwrap().extend([qr(QR_URL), None]);
        let mut said = session.tend(now);
        // A plain read, as the relay word's reply, never spends the claim.
        assert!(session.app_view(now).get("qr_url").is_none());
        assert_eq!(session.app_view_claiming(now)["qr_url"], QR_URL);
        assert!(session.app_view_claiming(now).get("qr_url").is_none());
        let revision = session.revision();
        // Kept alive: given once more, with its new countdown.
        lines.lock().unwrap().extend([qr(QR_URL), None]);
        said.extend(session.tend(now + Duration::from_secs(60)));
        assert!(session.revision() > revision);
        assert_eq!(
            session.app_view_claiming(now + Duration::from_secs(60))["qr_url"],
            QR_URL
        );
        // Untold, then past its time: not given.
        lines.lock().unwrap().extend([qr(QR_URL), None]);
        said.extend(session.tend(now + Duration::from_secs(61)));
        assert!(
            session
                .app_view_claiming(now + Duration::from_secs(200))
                .get("qr_url")
                .is_none()
        );
        said.push(format!("{:?}", session.state()));
        said.push(format!(
            "{:?}",
            Event::Qr {
                url: QR_URL.into(),
                expires_in: 120
            }
        ));
        said.push(session.view(now).to_string());
        said.push(format!("{:?}", session.answer_from_app(1, true, now)));
        said.push(format!("{:?}", session.answer(true, 9, now)));
        said.push(session.take_code(1, now).to_string());
        lines.lock().unwrap().extend([qr(QR_URL), None]);
        said.extend(session.tend(now + Duration::from_secs(62)));
        lines
            .lock()
            .unwrap()
            .push_back(Some(Ok(Event::Result(Done::Error {
                code: "CANCELLED".into(),
            }))));
        said.extend(session.tend(now + Duration::from_secs(63)));
        let ended = session.app_view_claiming(now + Duration::from_secs(63));
        said.push(ended.to_string());
        said.push(format!("{:?}", session.state()));
        assert!(said.len() >= 10, "{said:?}");
        for line in &said {
            assert!(!line.contains("hnq_") && !line.contains("://"), "{line}");
        }
        assert!(!session.qr_untold);
        assert_eq!(
            session.state(),
            &State::Done(Done::Error {
                code: "CANCELLED".into()
            })
        );
    }

    /// #5198: each method takes only its own form. A code sign-in passes
    /// over a QR line (nothing to give), and a QR one a code (the voice and
    /// the app never see digits it didn't ask for); a voice-started
    /// sign-in, always by code, never gives a link.
    #[test]
    fn each_method_takes_only_its_own_form() {
        let now = Instant::now();
        let (mut session, lines, _) = scripted(now);
        lines.lock().unwrap().extend([qr(QR_URL), None]);
        assert!(session.tend(now).is_empty());
        assert_eq!(session.state(), &State::Starting);
        assert!(session.app_view_claiming(now).get("qr_url").is_none());
        let (mut session, lines, _) = app_qr(now);
        lines.lock().unwrap().extend([code("12345678"), None]);
        assert!(session.tend(now).is_empty());
        let view = session.app_view_claiming(now);
        assert_eq!(view["sign_in"], "starting");
        assert!(view.get("code").is_none());
    }

    /// The reviewer's P3 on #2174: the code a start gave is kept only while
    /// the code is.
    #[test]
    fn the_code_given_is_forgotten_once_the_code_is_spent() {
        let now = Instant::now();
        let (mut session, lines, _) = scripted(now);
        lines.lock().unwrap().extend([code("12345678"), None]);
        session.tend(now);
        session.code_given();
        assert_eq!(session.view(now)["code_told"], true);
        lines.lock().unwrap().extend([
            Some(Ok(Event::Confirm {
                email: "someone@example.com".into(),
            })),
            None,
        ]);
        session.tend(now);
        assert_eq!(session.code_given, None);
    }
    /// The reviewer-lead's P2 on #5579: Harness's message is dropped when
    /// it holds the open code's digits, whatever the spacing or separators,
    /// for a code of any length and after the question too.
    #[test]
    fn a_sign_in_message_quoting_the_code_with_separators_is_dropped() {
        let error = |message: &str| {
            Some(Ok(Event::ResultWith {
                done: Done::Error {
                    code: "DENIED".into(),
                },
                message: Some(message.into()),
                already_signed_in: false,
            }))
        };
        let now = Instant::now();
        // A six-digit open code quoted with dashes.
        let (mut session, lines, _) = scripted(now);
        lines
            .lock()
            .unwrap()
            .extend([code("123456"), error("Code 12-34-56 was wrong."), None]);
        session.tend(now);
        assert_eq!(
            session.app_view(now)["message"],
            done_message("DENIED"),
            "a dashed six-digit code"
        );
        // A six-digit code quoted spaced, after the question was asked.
        let (mut session, lines, _) = scripted(now);
        lines.lock().unwrap().extend([
            code("123456"),
            Some(Ok(Event::Confirm {
                email: "someone@example.com".into(),
            })),
            None,
        ]);
        session.tend(now);
        lines
            .lock()
            .unwrap()
            .extend([error("Code 123 456 was already used."), None]);
        session.tend(now);
        assert_eq!(
            session.app_view(now)["message"],
            done_message("DENIED"),
            "a spaced code after the question"
        );
        // One without the code's digits is kept.
        let (mut session, lines, _) = scripted(now);
        lines
            .lock()
            .unwrap()
            .extend([code("123456"), error("Try again in 60 minutes."), None]);
        session.tend(now);
        assert_eq!(session.app_view(now)["message"], "Try again in 60 minutes.");
    }
}
