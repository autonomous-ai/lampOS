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

/// One line of `harness login --code --json`. Its `Debug` hides the digits
/// and the email (Jarvis's P3 on #2131), so a later `{:?}` can't print them.
#[derive(Clone, PartialEq, Eq)]
pub enum Event {
    /// The digits to say, and how many seconds they stay good.
    Code {
        code: String,
        expires_in: u64,
    },
    /// The person approved in Harness: ask them whether to sign in as this.
    Confirm {
        email: String,
    },
    Result(Done),
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
            Self::Confirm { .. } => write!(f, "Confirm {{ email: <hidden> }}"),
            Self::Result(done) => write!(f, "Result({done:?})"),
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
        "confirm" => Event::Confirm {
            email: email(value.get("email")).ok_or(unreadable)?,
        },
        "result" => Event::Result(match value["status"].as_str() {
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
        }),
        _ => return Ok(None),
    }))
}

/// What starts a sign-in: Harness's own, on a lane of its own.
pub type Starter = Box<dyn FnMut() -> Result<Box<dyn Link>, String> + Send>;

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
    /// when ([`Session::asked`]).
    told: Option<(u64, Instant)>,
    /// The code already given by a start or initial claim. A same-code
    /// keepalive does not make another initial telling available.
    code_given: Option<String>,
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
        }
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
        if id != self.id || now >= self.until {
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
                // A code after the question isn't one to say.
                Ok(Event::Code { code, expires_in })
                    if matches!(self.state, State::Starting | State::Code { .. }) =>
                {
                    // The same code kept alive is no change to say.
                    if !matches!(&self.state, State::Code { code: was, .. } if *was == code) {
                        said.push(format!("code given (good for {expires_in} s)"));
                    }
                    self.state = State::Code {
                        code,
                        until: now + Duration::from_secs(expires_in),
                    };
                }
                Ok(Event::Code { .. }) => {}
                // Asked once: a question after it was answered is passed
                // over, so only one answer is ever written (Jarvis's P3).
                Ok(Event::Confirm { email })
                    if matches!(self.state, State::Starting | State::Code { .. }) =>
                {
                    said.push("approved in Harness: asking whether to sign in".into());
                    self.state = State::Asking { email };
                }
                Ok(Event::Confirm { .. }) => {}
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
        said
    }

    fn finish(&mut self, done: Done) {
        self.state = State::Done(done);
        self.code_given = None;
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
        if !matches!(self.state, State::Asking { .. }) {
            return Err("not_asking");
        }
        let (told_in, told_at) = self.told.ok_or("not_told")?;
        if request <= told_in {
            return Err("not_told");
        }
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
        Ok(())
    }

    /// What the voice reads (`sign_in_state`).
    pub fn view(&self, now: Instant) -> Value {
        let mut view = match &self.state {
            State::Starting => json!({"sign_in": "starting"}),
            State::Code { code, until } => json!({
                "sign_in": "code",
                "code": code,
                "expires_in": until.saturating_duration_since(now).as_secs(),
                "code_told": self.code_given.as_ref() == Some(code),
            }),
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
}
