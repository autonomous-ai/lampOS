//! Signing the lamp's Harness in by voice (#1676, `LAMP_TASKS_SIGN_IN=on`,
//! [`crate::agent_tasks::SIGN_IN_FLAG`]). A lamp has no screen for Harness's
//! QR code; Harness 0.4.23 signs a device in by a code it says aloud
//! (openharness#1410). The tasks part runs Harness's own sign-in and passes
//! the answer on (`lamp-tasks serve --sign-in-by-code`, its trusted ops
//! `sign_in_start`, `sign_in_state`, `sign_in_take_code`, `sign_in_asked`
//! and `sign_in_answer`).
//! Where the sign-in is, whether its question was heard and whether an
//! answer may be taken are kept there only: the voice's news runs in its
//! coordinator and its tools in a worker, two processes. The voice:
//!
//! - **starts it** when the person asks ([`START`]), and says the code's
//!   digits and where to type them: Harness on their computer, or its site;
//! - **asks** "Sign in as …?" unasked once they have approved in Harness,
//!   the email read back slowly ("dot", "at"), as news
//!   ([`crate::task_news`], [`news`]); a code that came after the call
//!   answered is told the same way;
//! - **says the question was heard** ([`tell_asked`]) once its news turn was
//!   heard whole, with that turn's request;
//! - **passes on their answer** ([`ANSWER`]), only one spoken to the lamp,
//!   in a later request, as the tasks part allows;
//! - **says how it went**, again as news.
//!
//! No session exists before the person's yes, so a stranger who heard the
//! code gains nothing. Only a request the lamp heard may start a sign-in or
//! answer it, never one it wrote, a reminder or a text.
use lamp_gemini::{Tool, ToolCall};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

pub const START: &str = "sign_in_computer";
pub const ANSWER: &str = "sign_in_answer";
/// How long [`START`] waits for Harness's first code within its deadline:
/// a code that comes later is told as news.
const LOOK_EVERY: Duration = Duration::from_millis(200);
/// What the voice asks the tasks part by, connecting included.
const ASK_DEADLINE: Duration = Duration::from_millis(1_000);

/// How often the news reads a sign-in while one is open: the question
/// comes soon after the person approves in Harness (the reviewer's P3).
pub const READ_OPEN_EVERY: Duration = Duration::from_secs(2);

/// A sign-in's question taken to be told but not heard whole is offered
/// again this long after it was taken, while the tasks part still says
/// `asking` (the 21 Oct lamp-75c9 rerun's (3), #2602): on lamp-75c9 talk
/// in the room cut it short a second after it began, it was kept as told,
/// and the person's yes was refused as `not_told` from then on. Past a
/// question's own turn and its tail, so one being told isn't told twice.
pub const ASK_AGAIN_AFTER: Duration = Duration::from_secs(30);

/// How many times in all a sign-in's question is taken to be told: once,
/// and twice again if it isn't heard whole. Harness gives the person a few
/// minutes to answer, and a room this busy is let be after that.
pub const ASKED_MOST: u32 = 3;

/// Each sign-in's question taken to be told, by id: when it was last taken
/// (`None` once offered again) and how many times ([`ASK_AGAIN_AFTER`]).
#[derive(Debug, Default)]
pub struct AskAgain {
    told: std::collections::BTreeMap<u64, (Option<Instant>, u32)>,
}

impl AskAgain {
    /// The news `key` was taken to be told at `now`. Whether it is a
    /// sign-in's question, followed here rather than kept as told.
    pub fn told(&mut self, key: &str, now: Instant) -> bool {
        let Some(id) = asked_of(key) else {
            return false;
        };
        let (at, times) = self.told.entry(id).or_insert((None, 0));
        *at = Some(now);
        *times += 1;
        true
    }

    /// The questions followed here that were heard whole since (`heard`,
    /// the sign-ins whose question was): let go, to be kept as told, so a
    /// restart doesn't say them again (the reviewer-lead's P3 1 on #2702).
    pub fn heard(&mut self, heard: &std::collections::BTreeSet<u64>) -> Vec<String> {
        let ids: Vec<u64> = self
            .told
            .keys()
            .filter(|id| heard.contains(id))
            .copied()
            .collect();
        ids.into_iter()
            .map(|id| {
                self.told.remove(&id);
                key(id, "asking")
            })
            .collect()
    }

    /// The questions to offer again at `now`: taken to be told `after` ago
    /// or more, not heard whole (call [`Self::heard`] first), and taken
    /// fewer than [`ASKED_MOST`] times. Each is given once a telling. One
    /// told the last time is let go once its wait has passed (the
    /// reviewer-lead's P3 2 on #2702): it is in `offered` and never
    /// offered again.
    pub fn due(&mut self, after: Duration, now: Instant) -> Vec<String> {
        let waited =
            |at: &Option<Instant>| at.is_some_and(|at| now.saturating_duration_since(at) >= after);
        self.told
            .retain(|_, (at, times)| *times < ASKED_MOST || !waited(at));
        let mut due = Vec::new();
        for (id, (at, _)) in &mut self.told {
            if waited(at) {
                *at = None;
                due.push(key(*id, "asking"));
            }
        }
        due
    }

    /// How many questions are followed: for a test.
    #[cfg(test)]
    fn followed(&self) -> usize {
        self.told.len()
    }
}

/// The newest sign-in's code and email, never kept anywhere but here: what
/// [`scrub`] takes out of what the thread keeps. Kept until `until`, then
/// forgotten (the reviewer's P3 5 on #2137).
#[derive(Default)]
struct Said {
    code: Option<String>,
    email: Option<String>,
    until: Option<Instant>,
}
static SAID: Mutex<Said> = Mutex::new(Said {
    code: None,
    email: None,
    until: None,
});

/// How long the code and the account are kept after the newest news of
/// them, at most: a sign-in lasts a few minutes.
const KEPT_OPEN: Duration = Duration::from_secs(15 * 60);
/// How long they are kept once the sign-in ended: the person may still
/// say them back.
const KEPT_AFTER: Duration = Duration::from_secs(120);

fn keep_said(code: Option<&str>, email: Option<&str>) {
    if let Ok(mut said) = SAID.lock() {
        if let Some(code) = code {
            said.code = Some(code.to_owned());
        }
        if let Some(email) = email {
            said.email = Some(email.to_owned());
        }
        if said.code.is_some() || said.email.is_some() {
            said.until = Some(Instant::now() + KEPT_OPEN);
        }
    }
}

/// The sign-in ended: its code and account are kept [`KEPT_AFTER`] more.
fn said_ended() {
    if let Ok(mut said) = SAID.lock()
        && (said.code.is_some() || said.email.is_some())
    {
        said.until = Some(Instant::now() + KEPT_AFTER);
    }
}

/// `said`, forgotten once its time is past.
fn expire(said: &mut Said) {
    if said.until.is_some_and(|until| Instant::now() >= until) {
        *said = Said::default();
    }
}

/// `text` with the newest sign-in's code and email taken out, for what the
/// thread keeps: the second route's history, the thread's file and the
/// memory's journal (the first builder at 09:46 on #1676). The words were
/// read from the tool's result or the news as they were said; a later turn
/// is told only that a code was given, so it can't say it again. The code
/// is found however its digits were spaced ("1 2 3 4, 5 6 7 8",
/// "1234-5678"); the email as written or as said.
pub fn scrub(text: &str) -> String {
    let Ok(mut said) = SAID.lock() else {
        return text.to_owned();
    };
    expire(&mut said);
    let mut text = text.to_owned();
    if let Some(code) = said.code.as_deref() {
        text = without_code(&text, code);
    }
    if let Some(email) = said.email.as_deref() {
        for form in [email.to_owned(), spoken_email(email)] {
            text = replace_ignoring_case(&text, &form, "their account");
        }
    }
    text
}

/// `text` with each run of `code`'s digits, in order, with only spaces,
/// commas, dots or dashes between them, said as "a sign-in code".
fn without_code(text: &str, code: &str) -> String {
    let digits: Vec<char> = code.chars().collect();
    let chars: Vec<char> = text.chars().collect();
    let mut kept = String::new();
    let mut at = 0;
    while at < chars.len() {
        let mut next = at;
        let mut matched = 0;
        while matched < digits.len() && next < chars.len() {
            if chars[next] == digits[matched] {
                matched += 1;
                next += 1;
            } else if matched > 0 && matches!(chars[next], ' ' | ',' | '.' | '-') {
                next += 1;
            } else {
                break;
            }
        }
        if !digits.is_empty() && matched == digits.len() {
            kept.push_str("a sign-in code");
            at = next;
        } else {
            kept.push(chars[at]);
            at += 1;
        }
    }
    kept
}

fn replace_ignoring_case(text: &str, find: &str, with: &str) -> String {
    if find.is_empty() {
        return text.to_owned();
    }
    let lower = text.to_lowercase();
    let find = find.to_lowercase();
    // Lowercasing that changes lengths (rare letters) is left as it is.
    if lower.len() != text.len() {
        return text.to_owned();
    }
    let mut kept = String::new();
    let mut from = 0;
    while let Some(found) = lower[from..].find(&find) {
        kept.push_str(&text[from..from + found]);
        kept.push_str(with);
        from += found + find.len();
    }
    kept.push_str(&text[from..]);
    kept
}

pub fn is_tool(name: &str) -> bool {
    name == START || name == ANSWER
}

/// The two functions, declared with the switch.
pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: START.into(),
            description: "Sign this lamp in to the person's Harness, so it can hand work to \
                          their own computer. Call it only when the person asks to sign the lamp \
                          in, or to set up hand-overs to their computer. It gives a code to say \
                          and where they type it. If they ask for the code again, call it again: \
                          while the sign-in is open it gives back the same code. Never say a \
                          sign-in code's digits from memory."
                .into(),
            parameters: None,
            silent: false,
        },
        Tool {
            name: ANSWER.into(),
            description: "The person's answer when you asked whether to sign the lamp in as an \
                          email address: yes only when they clearly said yes to that question, \
                          otherwise no."
                .into(),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "answer": {"type": "string", "enum": ["yes", "no"]}
                },
                "required": ["answer"]
            })),
            silent: false,
        },
    ]
}

/// The code's digits as said: one by one, in two groups ("1 2 3 4, 5 6 7
/// 8").
pub fn code_to_say(code: &str) -> String {
    let digits: Vec<String> = code.chars().map(String::from).collect();
    let half = digits.len().div_ceil(2);
    if digits.len() < 6 {
        return digits.join(" ");
    }
    format!("{}, {}", digits[..half].join(" "), digits[half..].join(" "))
}

/// An email as it is read back, slowly and clearly, as Harness asks:
/// "someone at example dot com"; a dash, an underscore and a plus said.
pub fn spoken_email(email: &str) -> String {
    let mut said = String::new();
    for c in email.chars() {
        let word = match c {
            '@' => " at ",
            '.' => " dot ",
            '-' => " dash ",
            '_' => " underscore ",
            '+' => " plus ",
            _ => {
                said.push(c);
                continue;
            }
        };
        said.push_str(word);
    }
    said.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Where the code is typed, for the model to say.
const WHERE: &str = "they type it into Harness on their computer: in the Harness app under \
                     Machines, Approve a device, or at harness dot autonomous dot ai";

/// What the model is told of a sign-in in this state (the tasks part's
/// `sign_in_state`), as the answer to [`START`].
fn told(state: &Value) -> Value {
    match state["sign_in"].as_str() {
        Some("code") => {
            let code = state["code"].as_str().unwrap_or_default();
            json!({
                "code_to_say": code_to_say(code),
                "next": format!(
                    "Say the code digit by digit, slowly, in the two groups given, then that \
                     {WHERE}. Then say you will ask them to confirm their email once they have \
                     approved it. Never write the code down any other way."
                ),
            })
        }
        Some("starting") => json!({
            "next": "Harness is getting a code. Say briefly that you will read it out in a moment."
        }),
        Some("asking") | Some("answered") => json!({
            "next": "A sign-in is already waiting for the person's answer. Say so in a few words."
        }),
        Some("done") => done_told(state),
        _ => json!({"error": "The sign-in could not be started just now."}),
    }
}

/// How a done sign-in is told.
fn done_told(state: &Value) -> Value {
    if state["result"] == "success" {
        return json!({"result": "The lamp is signed in to Harness. Say so in a few words."});
    }
    let why = match state["code"].as_str().unwrap_or_default() {
        "DENIED" => "it was not approved",
        "EXPIRED" => "the code ran out of time; they can ask again",
        "CANCELLED" => "it was cancelled",
        "harness_too_old" => "Harness on this lamp needs updating first",
        "no_harness" => "Harness is not installed on this lamp",
        "timeout" => "it took too long; they can ask again",
        "too_late" => "the question went unanswered for two minutes; they can ask again",
        _ => "Harness could not sign it in just now",
    };
    json!({"error": format!("The lamp was not signed in: {why}.")})
}

fn ask(socket: &Path, line: &Value, deadline: Duration) -> Option<Value> {
    if deadline.is_zero() {
        return None;
    }
    crate::tools::exchange_up_to(
        socket,
        line,
        deadline.min(ASK_DEADLINE),
        crate::tools::MAX_TASKS_ANSWER_BYTES,
    )
}

fn needs_tasks_update() -> Value {
    json!({"error": "I need to finish updating before I can sign in. Please ask me again in a few minutes."})
}

fn start_refused(answer: &Value) -> Value {
    if answer["error"] == "bad_request" {
        return needs_tasks_update();
    }
    let why = match answer["error"].as_str() {
        Some("sign_in_off" | "untrusted") => "Signing in by voice is not set up on this lamp.",
        Some("timeout" | "busy") => {
            "The lamp's hand-over part is busy just now. Say to ask again in a moment."
        }
        _ => "The sign-in could not be started just now. Say to ask again later.",
    };
    json!({"error": why})
}

/// The owner's remaining lifetime starts before the RPC, so transit cannot
/// extend it. An expired or malformed grant is consumed but never spoken.
fn code_until(state: &Value, started: Instant) -> Option<Instant> {
    let code = state["code"].as_str()?;
    if state["sign_in"] != "code"
        || !(4..=12).contains(&code.len())
        || !code.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let until = started.checked_add(Duration::from_secs(state["expires_in"].as_u64()?))?;
    (until > Instant::now()).then_some(until)
}

/// One of the two functions, in the voice's `request`, within `deadline`.
/// Only a request the lamp heard may start or answer a sign-in.
pub fn run(
    directory: &Path,
    call: &ToolCall,
    request: u64,
    heard: bool,
    deadline: Duration,
) -> (Value, bool) {
    if !heard {
        return (
            json!({"error": "Only someone speaking to the lamp may sign it in."}),
            true,
        );
    }
    let started = Instant::now();
    let left = || deadline.saturating_sub(started.elapsed());
    let socket = directory.join("voice.sock");
    let refused = |answer: &Value| answer["ok"] != true;
    match call.name.as_str() {
        START => {
            let Some(capability) = ask(&socket, &json!({"op": "sign_in_state"}), left()) else {
                return (crate::tools::unavailable(), false);
            };
            if refused(&capability) {
                return (start_refused(&capability), true);
            }
            if capability["code_claims"] != 1 {
                return (needs_tasks_update(), true);
            }
            // A heard request is a new START, including an explicit repeat.
            // Its continuation is a claim for this id, never another START.
            let start_sent = Instant::now();
            let Some(mut state) = ask(&socket, &json!({"op": "sign_in_start"}), left()) else {
                return (crate::tools::unavailable(), false);
            };
            if refused(&state) {
                return (start_refused(&state), true);
            }
            if state["sign_in"] == "code" && code_until(&state, start_sent).is_none() {
                return (
                    json!({"error": "That sign-in code is no longer available. Say they can ask to sign in again."}),
                    true,
                );
            }
            let id = state["id"].as_u64();
            while state["sign_in"] == "starting" && left() > LOOK_EVERY + ASK_DEADLINE {
                let Some(id) = id else {
                    return (crate::tools::unavailable(), false);
                };
                std::thread::sleep(LOOK_EVERY);
                let claim_started = Instant::now();
                // A lost reply may hide a grant, which is never refunded, so
                // the news won't tell it: say so, and a new heard START asks
                // again (`a_lost_grant_reply_is_not_refunded_…`).
                let Some(read) = ask(
                    &socket,
                    &json!({"op": "sign_in_take_code", "id": id}),
                    left(),
                ) else {
                    return (crate::tools::unavailable(), false);
                };
                if refused(&read) {
                    // A busy or timed-out owner granted nothing, and the
                    // session stays open, so the news may still tell the
                    // code: say it is coming, never that the sign-in failed
                    // (the Reviewer-lead's P3 on #4070).
                    if matches!(read["error"].as_str(), Some("timeout" | "busy")) {
                        break;
                    }
                    return (start_refused(&read), true);
                }
                match read["claim"].as_str() {
                    Some("waiting") => {}
                    Some("granted")
                        if read["id"] == id && code_until(&read, claim_started).is_some() =>
                    {
                        state = read;
                    }
                    Some("already_given") => {
                        return (
                            json!({
                                "next": "The sign-in code is already being provided. Do not say it again."
                            }),
                            true,
                        );
                    }
                    Some("not_available" | "granted") => {
                        return (
                            json!({
                                "error": "That sign-in code is no longer available. Say they can ask to sign in again."
                            }),
                            true,
                        );
                    }
                    _ => return (needs_tasks_update(), true),
                }
            }
            if let Some(code) = state["code"].as_str() {
                keep_said(Some(code), None);
                // The lamp says it in this request's answer.
                mark(request);
            }
            (told(&state), true)
        }
        ANSWER => {
            let yes = match call.args.get("answer").and_then(Value::as_str) {
                Some("yes") => true,
                Some("no") => false,
                _ => return (json!({"error": "Give the answer as yes or no."}), true),
            };
            // The lamp's answer may read the account back.
            mark(request);
            // Taken by the tasks part only after the question was heard, in
            // a later request, within two minutes, once.
            let line = json!({
                "op": "sign_in_answer",
                "answer": if yes { "yes" } else { "no" },
                "request": request,
            });
            let Some(answer) = ask(&socket, &line, left()) else {
                return (crate::tools::unavailable(), false);
            };
            if refused(&answer) {
                let why = match answer["error"].as_str() {
                    Some("too_late") => {
                        "The sign-in question was asked too long ago, so the sign-in ended. \
                         Say they can ask to sign in again."
                    }
                    _ => "No sign-in question is waiting for an answer now.",
                };
                return (json!({"error": why}), true);
            }
            (
                json!({"result": if yes {
                    "Passed on. Say you are signing in, and that you will say when it is done."
                } else {
                    "Not signed in. Say so in a few words."
                }}),
                true,
            )
        }
        _ => (crate::tools::unavailable(), false),
    }
}

/// The note that asks the person, unasked, once they approved in Harness.
fn asking_note(email: &str) -> String {
    let said = spoken_email(email);
    format!(
        "Harness on the person's computer was approved for this lamp, and asks to sign it in as \
         {said}. Ask them once, briefly and clearly, reading the address slowly: \"Shall I sign \
         in as {said}?\" Then, on their answer, call {ANSWER}: yes only for a clear yes."
    )
}

/// What the news thread knows of the sign-ins it has seen: the first state
/// each id was seen in, and whether its answer was a no.
#[derive(Debug, Default)]
pub struct Follow {
    seen: std::collections::BTreeMap<u64, Seen>,
}

#[derive(Debug, Default)]
struct Seen {
    /// Seen first already done: the start's own answer told it.
    done_first: bool,
    said_no: bool,
}

/// The key of the sign-in `id`'s news `what`. It has no `:`, so it is never
/// taken for a task's, and neither the code nor the email.
fn key(id: u64, what: &str) -> String {
    format!("sign-in-{id}-{what}")
}

/// Whether `key` is a sign-in's news ([`key`]), not a task's.
pub fn is_news(key: &str) -> bool {
    key.starts_with("sign-in-")
}

/// The sign-in whose question `key` is, if it is one.
pub fn asked_of(key: &str) -> Option<u64> {
    key.strip_prefix("sign-in-")?
        .strip_suffix("-asking")?
        .parse()
        .ok()
}

/// A sign-in note and the code's conservative lifetime, if it is a code.
#[derive(Debug, PartialEq)]
pub struct SignInNews {
    pub key: String,
    pub note: String,
    pub until: Option<Instant>,
}

/// The news of a sign-in (read by the news thread, [`crate::task_news`]),
/// from the tasks part: a code claimed after the start's
/// answer said one, the question once approved in Harness, and how it
/// ended. Each is told once (`offered`). A sign-in first seen done gets no
/// news, since the start's answer told it; nor does one ended after the
/// person said no. Also whether one is open, to be read again soon.
pub fn news(
    directory: &Path,
    offered: &std::collections::BTreeSet<String>,
    follow: &mut Follow,
) -> (Vec<SignInNews>, bool) {
    let started = Instant::now();
    let socket = directory.join("voice.sock");
    let Some(state) = ask(&socket, &json!({"op": "sign_in_state"}), ASK_DEADLINE)
        .filter(|state| state["ok"] == true)
    else {
        return (Vec::new(), false);
    };
    let Some(id) = state["id"].as_u64() else {
        return (Vec::new(), false);
    };
    let what = state["sign_in"].as_str().unwrap_or_default();
    // Only the newest is kept: the tasks part has no other.
    follow.seen.retain(|kept, _| *kept == id);
    let seen = follow.seen.entry(id).or_insert_with(|| Seen {
        done_first: what == "done",
        said_no: false,
    });
    let mut news = Vec::new();
    match what {
        "code" if state["code_told"] != true && !offered.contains(&key(id, "code")) => {
            // State is observational. Both processes claim from the same
            // Tasks owner; neither may speak digits from this older snapshot.
            if state["code_claims"] == 1 {
                let claim_started = Instant::now();
                if let Some(claim) = ask(
                    &socket,
                    &json!({"op": "sign_in_take_code", "id": id}),
                    ASK_DEADLINE.saturating_sub(started.elapsed()),
                )
                .filter(|claim| {
                    claim["ok"] == true && claim["claim"] == "granted" && claim["id"] == id
                }) && let Some(until) = code_until(&claim, claim_started)
                    && let Some(code) = claim["code"].as_str()
                {
                    keep_said(Some(code), None);
                    news.push(SignInNews {
                        key: key(id, "code"),
                        note: format!(
                            "The code to sign this lamp in to Harness is ready: {}. Say it digit by \
                             digit, slowly, in the two groups given, then that {WHERE}.",
                            code_to_say(code)
                        ),
                        until: Some(until),
                    });
                }
            }
        }
        "asking" => {
            keep_said(None, state["email"].as_str());
            if let Some(email) = state["email"].as_str()
                && !offered.contains(&key(id, "asking"))
            {
                news.push(SignInNews {
                    key: key(id, "asking"),
                    note: asking_note(email),
                    until: None,
                });
            }
        }
        "answered" => seen.said_no = state["answer"] == "no",
        "done" if !seen.done_first && !seen.said_no && !offered.contains(&key(id, "done")) => {
            let told = done_told(&state);
            let note = told["result"]
                .as_str()
                .or_else(|| told["error"].as_str())
                .unwrap_or_default()
                .to_owned();
            news.push(SignInNews {
                key: key(id, "done"),
                note,
                until: None,
            });
        }
        _ => {}
    }
    // Tagged, so the worker that says it marks its request ([`written`]).
    let news = news
        .into_iter()
        .map(|mut news| {
            news.note = format!("{NEWS_TAG}{}", news.note);
            news
        })
        .collect();
    (
        news,
        matches!(what, "starting" | "code" | "asking" | "answered"),
    )
}

/// Tell the tasks part that sign-in `id`'s question was heard whole in the
/// voice's `request`: from then its answer is taken. Whether it was taken.
pub fn tell_asked(directory: &Path, id: u64, request: u64) -> bool {
    ask(
        &directory.join("voice.sock"),
        &json!({"op": "sign_in_asked", "id": id, "request": request}),
        ASK_DEADLINE,
    )
    .is_some_and(|answer| answer["ok"] == true)
}

/// What the lamp's words in a request that said a sign-in's code or
/// account leave the worker as, in place of them (#2137's redesign, step 3;
/// the reviewer's P2 and Jarvis's P1). The audio is untouched.
pub const SAID_LINE: &str = "(a sign-in code and account were said)";
/// How each sign-in news note begins: the worker given it marks its
/// request ([`written`]).
pub const NEWS_TAG: &str = "(Sign-in news, for you alone.) ";
/// The most requests kept marked: a sign-in has a few at most.
const MOST_MARKED: usize = 16;

/// In this worker: the tasks part's folder, when signing in by voice is on
/// ([`crate::agent_tasks::SIGN_IN_FLAG`]).
static DIRECTORY: Mutex<Option<PathBuf>> = Mutex::new(None);

/// What the lamp's words in a request of a Live session that heard a
/// sign-in's code or account leave the worker as: the session may say them
/// again in any later request (Jarvis's and the reviewer's P2 on #2137).
pub const HELD_LINE: &str = "(words kept back: this conversation heard a sign-in code)";

/// In this worker: the requests that said a code or an account, those of
/// them marked only for their Live session ([`leaving_live`]), and the ones
/// whose line has left already.
#[derive(Default)]
struct Marked {
    requests: BTreeSet<u64>,
    held: BTreeSet<u64>,
    lined: BTreeSet<u64>,
}
static MARKED: Mutex<Marked> = Mutex::new(Marked {
    requests: BTreeSet::new(),
    held: BTreeSet::new(),
    lined: BTreeSet::new(),
});

std::thread_local! {
    /// The Live session that heard a code or an account. The Live worker
    /// runs on one thread (its current-thread runtime), which sees both
    /// its connections and its words, so it is kept there: each test's
    /// worker has its own.
    static HELD_SESSION: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

impl Marked {
    fn mark(&mut self, request: u64) {
        self.requests.insert(request);
        while self.requests.len() > MOST_MARKED {
            self.requests.pop_first();
        }
        let Self {
            requests,
            held,
            lined,
            ..
        } = self;
        held.retain(|kept| requests.contains(kept));
        lined.retain(|kept| requests.contains(kept));
    }
}

/// Signing in by voice is on in this worker, with the tasks part in
/// `directory`.
pub fn serve_from(directory: &Path) {
    if let Ok(mut kept) = DIRECTORY.lock() {
        *kept = Some(directory.to_path_buf());
    }
}

/// The lamp's words in `request` say a code or an account: they leave this
/// worker as [`SAID_LINE`].
pub fn mark(request: u64) {
    if let Ok(mut marked) = MARKED.lock() {
        marked.mark(request);
    }
}

pub fn marked(request: u64) -> bool {
    MARKED
        .lock()
        .is_ok_and(|marked| marked.requests.contains(&request))
}

/// A written request this worker was given (the coordinator's news): one
/// that is a sign-in's is marked, and the code and the account are read
/// from the tasks part, so that what is kept of the note and of the
/// person's words can leave them out ([`scrub`], [`scrub_piece`]); once it
/// ended, they are kept [`KEPT_AFTER`] more. The read is off the worker's
/// loop, so a slow tasks part never holds its sound (the reviewer's P3 4
/// on #2137); its request's own words are its line whatever it finds.
pub fn written(request: u64, text: &str) -> Option<std::thread::JoinHandle<()>> {
    if !text.starts_with(NEWS_TAG) {
        return None;
    }
    mark(request);
    let directory = DIRECTORY.lock().ok().and_then(|kept| kept.clone())?;
    std::thread::Builder::new()
        .name("sign-in-said".into())
        .spawn(move || {
            let Some(state) = ask(
                &directory.join("voice.sock"),
                &json!({"op": "sign_in_state"}),
                ASK_DEADLINE,
            ) else {
                return;
            };
            keep_said(state["code"].as_str(), state["email"].as_str());
            if state["sign_in"] == "done" {
                said_ended();
            }
        })
        .ok()
}

/// What leaves this worker of `output`: the lamp's words in a marked
/// request as [`SAID_LINE`], once, its later pieces empty; the person's
/// words with the code and the account taken out, piece by piece. Called
/// again on what it gave, it gives the same.
pub fn leaving(output: &mut crate::provider_worker::ProviderOutput) {
    if let crate::provider_worker::ProviderOutput::Transcript { request, text, .. } = output {
        *text = leaving_text(*request, std::mem::take(text));
    }
}

/// [`leaving`] for words of `request` (none: the person's), as they come
/// from the service, before anything keeps them.
pub fn leaving_text(request: Option<u64>, text: String) -> String {
    let Some(request) = request else {
        return scrub_piece(&text);
    };
    let Ok(mut marked) = MARKED.lock() else {
        return text;
    };
    if text == SAID_LINE || text == HELD_LINE || text.is_empty() {
        return text;
    }
    // Not marked: the model may still say the code or the account from
    // what it guesses or recalls ("Three one five eight three" on
    // lamp-75c9, 21 Oct, asked for the code again in a request of its
    // own): taken out as from the person's words (#2602 finding 5).
    if !marked.requests.contains(&request) {
        return scrub_piece(&text);
    }
    if !marked.lined.insert(request) {
        String::new()
    } else if marked.held.contains(&request) {
        HELD_LINE.to_owned()
    } else {
        SAID_LINE.to_owned()
    }
}

/// A marked `request` was heard in Live session `session`, with words or
/// none (an answer cut before its first word, the asking note): that
/// session heard the code or the account.
pub fn in_session(request: u64, session: u64) {
    if marked(request) {
        HELD_SESSION.set(Some(session));
    }
}

/// A Live connection was made as `session`: `resumed`, it goes on with the
/// conversation before it, so what that one heard is held in it too; fresh,
/// it heard nothing, and nothing is held.
pub fn connected(session: u64, resumed: bool) {
    HELD_SESSION.set(if resumed {
        HELD_SESSION.get().map(|_| session)
    } else {
        None
    });
}

/// [`leaving_text`] for the lamp's words of `request` in Live session
/// `session`: once a marked request's words come in a session, that
/// session heard the code or the account, and may say them again in any
/// later request, so each of its requests leaves as [`HELD_LINE`], in it
/// and in any connection resumed from it ([`connected`]), until a fresh
/// one (Jarvis's and the reviewer's P2 on #2137). The audio is untouched.
pub fn leaving_live(request: u64, session: u64, text: String) -> String {
    if let Ok(mut marked) = MARKED.lock() {
        if marked.requests.contains(&request) {
            HELD_SESSION.set(Some(session));
        } else if HELD_SESSION.get() == Some(session) {
            marked.mark(request);
            marked.held.insert(request);
        }
    }
    leaving_text(Some(request), text)
}

/// In this worker: the person's words of the line being heard, held while
/// a sign-in's code or account is kept ([`person_piece`]).
static PERSON: Mutex<String> = Mutex::new(String::new());
/// The most of a line held: past it, what is held leaves.
const MOST_HELD: usize = 4_000;

/// The person's words on Live as they leave this worker. While a sign-in's
/// code or account is kept, each piece is held until its line is finished,
/// and the line leaves whole, scrubbed: a piece split inside the code or
/// the account can't be scrubbed alone, and one sent can't be taken back
/// (Jarvis's P2 on #2137). Empty for a piece held. Otherwise each piece is
/// scrubbed as it comes ([`scrub_piece`]). The second route hears each line
/// whole, so it never needs this.
pub fn person_piece(text: &str, finished: bool) -> String {
    let holding = SAID.lock().is_ok_and(|mut said| {
        expire(&mut said);
        said.code.is_some() || said.email.is_some()
    });
    let Ok(mut held) = PERSON.lock() else {
        return scrub_piece(text);
    };
    if !holding && held.is_empty() {
        return scrub_piece(text);
    }
    held.push_str(text);
    if finished || !holding || held.len() > MOST_HELD {
        let whole = std::mem::take(&mut *held);
        return scrub_piece(&whole);
    }
    String::new()
}

/// [`scrub`], and for a piece of the person's words, also any run of three
/// or more of the code's digits in its order, and the account's name or its
/// domain on its own, as written or as said (Jarvis's P2: the words come
/// in pieces split anywhere).
pub fn scrub_piece(text: &str) -> String {
    let mut text = scrub(text);
    let Ok(mut said) = SAID.lock() else {
        return text;
    };
    expire(&mut said);
    if let Some(code) = said.code.as_deref() {
        text = without_code_part(&text, code);
        text = without_code_words(&text, code);
    }
    if let Some((name, domain)) = said
        .email
        .as_deref()
        .and_then(|email| email.split_once('@'))
    {
        for part in [name, domain] {
            for form in [part.to_owned(), spoken_email(part)] {
                if form.chars().filter(char::is_ascii_alphanumeric).count() >= 3 {
                    text = replace_ignoring_case(&text, &form, "their account");
                }
            }
        }
    }
    text
}

/// The digit a word says, if it says one ("four", "oh").
fn digit_word(word: &str) -> Option<char> {
    let word: String = word
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase();
    Some(match word.as_str() {
        "zero" | "oh" => '0',
        "one" => '1',
        "two" => '2',
        "three" => '3',
        "four" => '4',
        "five" => '5',
        "six" => '6',
        "seven" => '7',
        "eight" => '8',
        "nine" => '9',
        _ => return None,
    })
}

/// `text` with each run of three or more digits said as words ("four eight
/// two"), a part of `code` in its order, said as "a sign-in code" (Jarvis's
/// test 3).
fn without_code_words(text: &str, code: &str) -> String {
    let words: Vec<&str> = text.split_inclusive(char::is_whitespace).collect();
    let mut kept = String::new();
    let mut at = 0;
    while at < words.len() {
        let mut end = at;
        let mut digits = String::new();
        while let Some(digit) = words.get(end).and_then(|word| digit_word(word)) {
            digits.push(digit);
            end += 1;
        }
        if digits.len() >= 3 && code.contains(&digits) {
            kept.push_str("a sign-in code");
            // The spacing and stop after the last word are kept.
            let last = words[end - 1];
            kept.extend(last.chars().filter(|c| !c.is_alphanumeric()));
            at = end;
        } else {
            kept.push_str(words[at]);
            at += 1;
        }
    }
    kept
}

/// `text` with each run of three or more digits that is a part of `code`,
/// in its order, spaced as [`without_code`] allows, said as "a sign-in
/// code".
fn without_code_part(text: &str, code: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut kept = String::new();
    let mut at = 0;
    while at < chars.len() {
        // The run of digits from here, with only spacing between them.
        let mut digits = String::new();
        let mut next = at;
        let mut end = at;
        while next < chars.len() {
            if chars[next].is_ascii_digit() {
                digits.push(chars[next]);
                next += 1;
                end = next;
            } else if !digits.is_empty() && matches!(chars[next], ' ' | ',' | '.' | '-') {
                next += 1;
            } else {
                break;
            }
        }
        if digits.len() >= 3 && code.contains(&digits) {
            kept.push_str("a sign-in code");
            at = end;
        } else if digits.is_empty() {
            kept.push(chars[at]);
            at += 1;
        } else {
            kept.extend(&chars[at..end]);
            at = end;
        }
    }
    kept
}

/// The code and the account, as a sign-in's would be kept, for a test of
/// a worker's path.
#[cfg(test)]
pub(crate) fn keep_said_for_test(code: &str, email: &str) {
    keep_said(Some(code), Some(email));
}

#[cfg(test)]
pub(crate) fn forget() {
    if let Ok(mut marked) = MARKED.lock() {
        *marked = Marked::default();
    }
    if let Ok(mut said) = SAID.lock() {
        *said = Said::default();
    }
    if let Ok(mut held) = PERSON.lock() {
        held.clear();
    }
    HELD_SESSION.set(None);
}

/// The Live session that heard a sign-in's code, for a test of a resumed
/// connection.
#[cfg(test)]
pub(crate) fn held_session_for_test() -> Option<u64> {
    HELD_SESSION.get()
}

/// The sign-in ended `ago` already, for a test of how long its code and
/// account are kept.
#[cfg(test)]
pub(crate) fn ended_for_test(ago: Duration) {
    said_ended();
    if let Ok(mut said) = SAID.lock() {
        said.until = said.until.and_then(|until| until.checked_sub(ago));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod code_delivery {
        use super::*;
        use crate::{
            agent_tasks::{DEADLINE, Place, run as tasks_run},
            task_news::{News, TaskNews},
        };
        use lamp_bus::Bus;
        use lamp_msgs::Diagnostic;
        use lamp_tasks::{
            harness::{Launch, LaunchOutcome, Observation},
            runtime::{
                Adapter, Config, DAEMON_RETRY_AFTER, REPORT_EVERY, SystemClock, request,
                serve_with_adapter,
            },
            sign_in::{Event, Link, Starter},
        };
        use std::{
            collections::BTreeMap,
            fs,
            io::{self, BufRead, BufReader, Write},
            os::unix::{
                fs::symlink,
                net::{UnixListener, UnixStream},
            },
            sync::{
                Arc,
                atomic::{AtomicBool, AtomicUsize, Ordering},
                mpsc::{self, Receiver, Sender},
            },
            thread::{self, JoinHandle},
        };

        const WAIT: Duration = Duration::from_secs(5);
        const CODE: &str = "12345678";
        const SPOKEN: &str = "1 2 3 4, 5 6 7 8";

        /// Harness gives a line only when the test sends it. The receipt
        /// means the real Tasks owner took that line, not that time passed.
        struct CodeLink {
            lines: Receiver<Event>,
            read: Sender<()>,
        }
        impl Link for CodeLink {
            fn poll(&mut self) -> Option<Result<Event, &'static str>> {
                let event = self.lines.try_recv().ok()?;
                let _ = self.read.send(());
                Some(Ok(event))
            }
            fn answer(&mut self, _: bool) -> io::Result<()> {
                Err(io::Error::other(
                    "this fixture has no confirmation question",
                ))
            }
        }

        struct Harness {
            link: Mutex<Option<CodeLink>>,
            starts: Arc<AtomicUsize>,
        }
        impl Adapter for Harness {
            fn launch(&self, _: &Launch) -> LaunchOutcome {
                panic!("a sign-in must not launch an agent")
            }
            fn observe(&self, _: &str, _: &str) -> Result<Observation, String> {
                panic!("a sign-in must not observe an agent")
            }
            fn sign_in_starter(&self) -> Option<Starter> {
                let mut link = self.link.lock().unwrap().take();
                let starts = self.starts.clone();
                Some(Box::new(move || {
                    starts.fetch_add(1, Ordering::SeqCst);
                    link.take()
                        .map(|link| Box::new(link) as Box<dyn Link>)
                        .ok_or_else(|| "a second Harness sign-in was started".into())
                }))
            }
        }

        struct Tasks {
            root: PathBuf,
            stop: Arc<AtomicBool>,
            serving: Option<JoinHandle<io::Result<()>>>,
            lines: Sender<Event>,
            read: Receiver<()>,
            starts: Arc<AtomicUsize>,
        }
        impl Tasks {
            fn new(name: &str) -> Self {
                let root = PathBuf::from(format!("/tmp/lv2839-{name}-{}", std::process::id()));
                let _ = fs::remove_dir_all(&root);
                fs::create_dir_all(&root).unwrap();
                let config = Config {
                    enabled: true,
                    dir: root.join("data"),
                    socket: root.join("tasks/request.sock"),
                    voice_socket: root.join("tasks/voice.sock"),
                    bus_root: root.join("b"),
                    targets: BTreeMap::new(),
                    run_for: None,
                    watch: false,
                    progress: false,
                    push_wait: false,
                    close_told: false,
                    targets_newer: None,
                    set_up_later: None,
                    sign_in_by_code: true,
                    report_every: REPORT_EVERY,
                    daemon_retry_after: DAEMON_RETRY_AFTER,
                };
                let mut ready = Bus::new(&config.bus_root)
                    .unwrap()
                    .subscribe_typed(Diagnostic::TOPIC, "code_test", Diagnostic::TYPE)
                    .unwrap();
                let (lines, received) = mpsc::channel();
                let (read, receipt) = mpsc::channel();
                let starts = Arc::new(AtomicUsize::new(0));
                let harness = Harness {
                    link: Mutex::new(Some(CodeLink {
                        lines: received,
                        read,
                    })),
                    starts: starts.clone(),
                };
                let stop = Arc::new(AtomicBool::new(false));
                let stopped = stop.clone();
                let serving = thread::spawn(move || {
                    serve_with_adapter(
                        config,
                        SystemClock::new(Some("sign-in-test")).unwrap(),
                        stopped,
                        |_| Ok(harness),
                    )
                });
                let fixture = Self {
                    root,
                    stop,
                    serving: Some(serving),
                    lines,
                    read: receipt,
                    starts,
                };
                let report: Diagnostic = ready
                    .recv(WAIT)
                    .unwrap()
                    .expect("Tasks ready")
                    .parse()
                    .unwrap();
                assert_eq!(report.name, "tasks");
                assert!(
                    report
                        .values
                        .iter()
                        .any(|v| v.key == "store_ready" && v.value == "true")
                );
                fixture
            }

            fn directory(&self) -> PathBuf {
                self.root.join("tasks")
            }

            fn code(&self) {
                self.lines
                    .send(Event::Code {
                        code: CODE.into(),
                        expires_in: 120,
                    })
                    .unwrap();
            }
        }
        impl Drop for Tasks {
            fn drop(&mut self) {
                self.stop.store(true, Ordering::Release);
                let result = self.serving.take().unwrap().join();
                let _ = fs::remove_dir_all(&self.root);
                forget();
                if !thread::panicking() {
                    result.unwrap().unwrap();
                }
            }
        }

        /// Hold only delivery of the initial `starting` reply. Its real
        /// Tasks owner stays free to receive Harness's code and answer the
        /// news reader. Later calls use the real socket directly.
        struct HeldStart {
            directory: PathBuf,
            entered: Receiver<Value>,
            release: Option<Sender<()>>,
            relay: Option<JoinHandle<()>>,
        }
        impl HeldStart {
            fn new(tasks: &Tasks) -> Self {
                let directory = tasks.root.join("held");
                fs::create_dir(&directory).unwrap();
                let socket = directory.join("voice.sock");
                let listener = UnixListener::bind(&socket).unwrap();
                let real = tasks.directory().join("voice.sock");
                let (arrived, entered) = mpsc::channel();
                let (release, released) = mpsc::channel();
                let relay = thread::spawn(move || {
                    // The capability read is observational and precedes
                    // the held START; it is never the delivery barrier.
                    let (mut capability, _) = listener.accept().unwrap();
                    capability.set_read_timeout(Some(WAIT)).unwrap();
                    capability.set_write_timeout(Some(WAIT)).unwrap();
                    let mut line = String::new();
                    if BufReader::new(&capability).read_line(&mut line).unwrap() == 0 {
                        return;
                    }
                    let line: Value = serde_json::from_str(&line).unwrap();
                    assert_eq!(line, json!({"op":"sign_in_state"}));
                    let state = request(&real, &line, ASK_DEADLINE).unwrap();
                    writeln!(capability, "{state}").unwrap();
                    let (mut stream, _) = listener.accept().unwrap();
                    stream.set_read_timeout(Some(WAIT)).unwrap();
                    stream.set_write_timeout(Some(WAIT)).unwrap();
                    let mut line = String::new();
                    if BufReader::new(&stream).read_line(&mut line).unwrap() == 0 {
                        return; // The fixture was dropped before START.
                    }
                    let line: Value = serde_json::from_str(&line).unwrap();
                    assert_eq!(line, json!({"op":"sign_in_start"}));
                    let state = request(&real, &line, ASK_DEADLINE).unwrap();
                    fs::remove_file(&socket).unwrap();
                    symlink(&real, &socket).unwrap();
                    arrived.send(state.clone()).unwrap();
                    if released.recv_timeout(WAIT).is_ok() {
                        writeln!(stream, "{state}").unwrap();
                    }
                });
                Self {
                    directory,
                    entered,
                    release: Some(release),
                    relay: Some(relay),
                }
            }
        }
        impl Drop for HeldStart {
            fn drop(&mut self) {
                drop(self.release.take());
                // Also releases accept if START failed before connecting.
                let socket = self.directory.join("voice.sock");
                if !socket.is_symlink() {
                    let _ = UnixStream::connect(socket);
                }
                let result = self.relay.take().unwrap().join();
                if !thread::panicking() {
                    result.unwrap();
                }
            }
        }

        /// Keep one actual owner reply at the socket boundary. Earlier
        /// calls pass through; later calls reach Tasks directly. Releasing
        /// with false deliberately loses this reply after the owner acted.
        struct HeldReply {
            directory: PathBuf,
            seen: Receiver<(Value, Value)>,
            entered: Receiver<Value>,
            release: Option<Sender<bool>>,
            relay: Option<JoinHandle<()>>,
        }
        impl HeldReply {
            fn new(tasks: &Tasks, name: &str, operation: &'static str) -> Self {
                let directory = tasks.root.join(name);
                fs::create_dir(&directory).unwrap();
                let socket = directory.join("voice.sock");
                let listener = UnixListener::bind(&socket).unwrap();
                let real = tasks.directory().join("voice.sock");
                let (observed, seen) = mpsc::channel();
                let (arrived, entered) = mpsc::channel();
                let (release, released) = mpsc::channel();
                let relay = thread::spawn(move || {
                    loop {
                        let (mut stream, _) = listener.accept().unwrap();
                        stream.set_read_timeout(Some(WAIT)).unwrap();
                        stream.set_write_timeout(Some(WAIT)).unwrap();
                        let mut line = String::new();
                        if BufReader::new(&stream).read_line(&mut line).unwrap() == 0 {
                            return;
                        }
                        let line: Value = serde_json::from_str(&line).unwrap();
                        let state = request(&real, &line, ASK_DEADLINE).unwrap();
                        // The test may not publish Harness's code before the
                        // provider's first continuation. Forward Waiting as
                        // usual; only a terminal claim is the held handoff.
                        let waiting =
                            line["op"] == "sign_in_take_code" && state["claim"] == "waiting";
                        if line["op"] == operation && !waiting {
                            fs::remove_file(&socket).unwrap();
                            symlink(&real, &socket).unwrap();
                            arrived.send(state.clone()).unwrap();
                            if released.recv_timeout(WAIT) == Ok(true) {
                                writeln!(stream, "{state}").unwrap();
                            }
                            return;
                        }
                        observed.send((line, state.clone())).unwrap();
                        writeln!(stream, "{state}").unwrap();
                    }
                });
                Self {
                    directory,
                    seen,
                    entered,
                    release: Some(release),
                    relay: Some(relay),
                }
            }
        }
        impl Drop for HeldReply {
            fn drop(&mut self) {
                drop(self.release.take());
                let socket = self.directory.join("voice.sock");
                if !socket.is_symlink() {
                    let _ = UnixStream::connect(socket);
                }
                let result = self.relay.take().unwrap().join();
                if !thread::panicking() {
                    result.unwrap();
                }
            }
        }

        /// A separate provider process shares only the Tasks socket with
        /// coordinator news. Process-local locks cannot make this test pass.
        struct Provider {
            child: std::process::Child,
            result: PathBuf,
            stdout: PathBuf,
            stderr: PathBuf,
        }
        impl Provider {
            fn start(directory: &Path) -> Self {
                Self::launch(directory, "provider", "start", None)
            }
            fn launch(directory: &Path, name: &str, mode: &str, id: Option<u64>) -> Self {
                let result = directory.join(format!("{name}-result.json"));
                let stdout = directory.join(format!("{name}.stdout"));
                let stderr = directory.join(format!("{name}.stderr"));
                let child = std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "sign_in::tests::code_delivery::provider_process_start",
                    ])
                    .env("LAMP_TEST_SIGN_IN_DIRECTORY", directory)
                    .env("LAMP_TEST_SIGN_IN_RESULT", &result)
                    .env("LAMP_TEST_SIGN_IN_MODE", mode)
                    .env("LAMP_TEST_SIGN_IN_ID", id.unwrap_or_default().to_string())
                    .stdin(std::process::Stdio::null())
                    .stdout(fs::File::create(&stdout).unwrap())
                    .stderr(fs::File::create(&stderr).unwrap())
                    .spawn()
                    .unwrap();
                Self {
                    child,
                    result,
                    stdout,
                    stderr,
                }
            }
            fn answer(&mut self) -> Value {
                let until = Instant::now() + WAIT;
                loop {
                    if let Some(status) = self.child.try_wait().unwrap() {
                        assert!(
                            status.success(),
                            "provider fixture: {status}\nstdout:\n{}\nstderr:\n{}",
                            fs::read_to_string(&self.stdout).unwrap_or_default(),
                            fs::read_to_string(&self.stderr).unwrap_or_default()
                        );
                        return serde_json::from_slice(&fs::read(&self.result).unwrap()).unwrap();
                    }
                    if Instant::now() >= until {
                        let killed = self.child.kill();
                        let waited = self.child.wait();
                        panic!(
                            "provider did not return its answer; kill {killed:?}, wait {waited:?}\nstdout:\n{}\nstderr:\n{}",
                            fs::read_to_string(&self.stdout).unwrap_or_default(),
                            fs::read_to_string(&self.stderr).unwrap_or_default()
                        );
                    }
                    thread::sleep(Duration::from_millis(5));
                }
            }
        }
        impl Drop for Provider {
            fn drop(&mut self) {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }

        #[test]
        fn provider_process_start() {
            let Some(directory) = std::env::var_os("LAMP_TEST_SIGN_IN_DIRECTORY") else {
                return;
            };
            let result = PathBuf::from(std::env::var_os("LAMP_TEST_SIGN_IN_RESULT").unwrap());
            let directory = PathBuf::from(directory);
            let mode = std::env::var("LAMP_TEST_SIGN_IN_MODE").unwrap();
            let answer = if mode == "claim" {
                // A replacement's internal continuation is never a new
                // heard START. Only the real shared owner is contacted.
                let id: u64 = std::env::var("LAMP_TEST_SIGN_IN_ID")
                    .unwrap()
                    .parse()
                    .unwrap();
                request(
                    &directory.join("voice.sock"),
                    &json!({"op":"sign_in_take_code","id":id}),
                    ASK_DEADLINE,
                )
                .unwrap()
            } else {
                assert!(matches!(mode.as_str(), "start" | "lost"));
                let call = ToolCall {
                    id: "start".into(),
                    name: START.into(),
                    args: json!({}),
                };
                let (answer, handled) = tasks_run(&place(directory), &call, 1, true, DEADLINE);
                assert_eq!(handled, mode == "start");
                answer
            };
            fs::write(result, serde_json::to_vec(&answer).unwrap()).unwrap();
        }

        fn place(directory: PathBuf) -> Place {
            Place {
                directory,
                target: crate::agent_tasks::DEFAULT_TARGET.into(),
                who: false,
                lead_in: false,
                asked: false,
                accounts: false,
                machine_names: false,
                needs: None,
                staff_note: false,
                make_skill: false,
                named_only: false,
                correct: false,
                questions: false,
                short_confirm: false,
                sign_in: true,
            }
        }

        fn ordinary_news(key: &str) -> News {
            News {
                key: key.into(),
                note: "ordinary task news".into(),
                until: None,
                progress: None,
                finished: None,
                also: Vec::new(),
            }
        }

        #[test]
        fn an_expired_code_leaves_other_queued_news_in_order() {
            let mut code = ordinary_news("sign-in-7-code");
            code.note = SPOKEN.into();
            code.until = Some(Instant::now());
            let (mut waiting, _heard) = TaskNews::all_waiting_for_test(
                vec![ordinary_news("before"), code, ordinary_news("after")],
                Duration::ZERO,
            );
            assert_eq!(waiting.waiting().unwrap().key, "before");
            assert!(!waiting.sign_in_first());
            assert_eq!(waiting.tell().unwrap().key, "before");
            assert_eq!(waiting.tell().unwrap().key, "after");
            assert!(waiting.waiting().is_none());
        }

        fn check_code_delivery(queued: bool) {
            let _thread = crate::provider_worker::tests::one_thread();
            let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            forget();
            let tasks = Tasks::new(if queued { "queued" } else { "direct" });
            let call = ToolCall {
                id: "start".into(),
                name: START.into(),
                args: json!({}),
            };
            let mut at = place(tasks.directory());
            at.sign_in = false;
            assert!(
                tasks_run(&at, &call, 1, true, DEADLINE)
                    .0
                    .get("error")
                    .is_some()
            );
            at.sign_in = true;
            assert!(
                tasks_run(&at, &call, 1, false, DEADLINE)
                    .0
                    .get("error")
                    .is_some()
            );
            assert_eq!(tasks.starts.load(Ordering::SeqCst), 0);

            let mut follow = Follow::default();
            let (first, mut waiting) = if queued {
                let mut held = HeldStart::new(&tasks);
                at.directory = held.directory.clone();
                {
                    let mut starting = Provider::start(&at.directory);
                    let state = held
                        .entered
                        .recv_timeout(WAIT)
                        .expect("START reached Tasks");
                    assert_eq!(state["sign_in"], "starting");
                    tasks.code();
                    tasks.read.recv_timeout(WAIT).expect("Harness code read");
                    let (offered, open) = news(&tasks.directory(), &BTreeSet::new(), &mut follow);
                    assert!(open);
                    assert_eq!(
                        offered.len(),
                        1,
                        "the code must be queued before START continues"
                    );
                    let mut notes: Vec<News> = offered
                        .into_iter()
                        .map(|news| News {
                            key: news.key,
                            note: news.note,
                            until: news.until,
                            progress: None,
                            finished: None,
                            also: Vec::new(),
                        })
                        .collect();
                    notes.insert(0, ordinary_news("before"));
                    notes.push(ordinary_news("after"));
                    let (mut waiting, _heard) =
                        TaskNews::all_waiting_for_test(notes, Duration::ZERO);
                    assert_eq!(waiting.waiting().unwrap().key, "before");
                    assert!(waiting.sign_in_first());
                    assert!(waiting.waiting().unwrap().note.contains(SPOKEN));
                    held.release.take().unwrap().send(()).unwrap();
                    (starting.answer(), waiting)
                }
            } else {
                tasks.code();
                let answer = tasks_run(&at, &call, 1, true, DEADLINE).0;
                tasks.read.recv_timeout(WAIT).expect("Harness code read");
                (
                    answer,
                    TaskNews::all_waiting_for_test(Vec::new(), Duration::ZERO).0,
                )
            };
            assert!(first.get("error").is_none(), "{first}");
            // Exercise the real queue at the same note boundary as its
            // reader, including a note already queued before START returns.
            let mut deliveries = first["code_to_say"]
                .as_str()
                .map_or(0, |s| s.matches(SPOKEN).count());
            let mut order = Vec::new();
            while waiting.waiting().is_some() {
                let note = waiting.tell().unwrap();
                deliveries += note.note.matches(SPOKEN).count();
                order.push(note.key);
            }
            if queued {
                assert_eq!(order.len(), 3);
                assert!(order[0].starts_with("sign-in-") && order[0].ends_with("-code"));
                assert_eq!(&order[1..], &["before", "after"]);
            }
            let before_repeat = deliveries;

            // Harness keeps the same code alive. Reading unchanged state
            // again must not manufacture another telling.
            tasks.code();
            tasks
                .read
                .recv_timeout(WAIT)
                .expect("Harness keepalive read");
            assert!(
                news(&tasks.directory(), &BTreeSet::new(), &mut follow)
                    .0
                    .is_empty()
            );
            // A new heard request represents "say it again": the model
            // calls START again, as its tool instruction requires.
            let repeat = tasks_run(&at, &call, 2, true, DEADLINE).0;
            assert_eq!(repeat["code_to_say"], SPOKEN, "{repeat}");
            deliveries += 1;
            assert!(
                news(&tasks.directory(), &BTreeSet::new(), &mut follow)
                    .0
                    .is_empty()
            );
            assert_eq!(
                tasks.starts.load(Ordering::SeqCst),
                1,
                "one Harness session"
            );
            assert_eq!(
                before_repeat, 1,
                "code deliveries before the requested repeat (queued={queued})"
            );
            assert_eq!(
                deliveries, 2,
                "one initial telling and one requested repeat"
            );
        }

        #[test]
        fn a_sign_in_code_is_delivered_once_and_repeated_only_on_request() {
            check_code_delivery(false);
        }

        #[test]
        fn a_queued_sign_in_code_is_not_repeated_by_the_pending_start() {
            check_code_delivery(true);
        }

        #[test]
        fn a_provider_claim_wins_over_news_holding_the_earlier_unclaimed_state() {
            let _thread = crate::provider_worker::tests::one_thread();
            let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            forget();
            let tasks = Tasks::new("provider-first");
            let mut starting = HeldStart::new(&tasks);
            let mut provider = Provider::start(&starting.directory);
            let initial = starting.entered.recv_timeout(WAIT).unwrap();
            assert_eq!(initial["sign_in"], "starting");
            tasks.code();
            tasks.read.recv_timeout(WAIT).expect("Harness code read");
            let mut stale = HeldReply::new(&tasks, "stale-news", "sign_in_state");
            let directory = stale.directory.clone();
            thread::scope(|scope| {
                let reading =
                    scope.spawn(move || news(&directory, &BTreeSet::new(), &mut Follow::default()));
                let snapshot = stale.entered.recv_timeout(WAIT).unwrap();
                assert_eq!(snapshot["sign_in"], "code");
                assert_eq!(snapshot["code_told"], false);
                assert_eq!(snapshot["id"], initial["id"]);
                // The provider cannot have reached its continuation until
                // this event releases its original Starting reply.
                starting.release.take().unwrap().send(()).unwrap();
                let answer = provider.answer();
                assert_eq!(answer["code_to_say"], SPOKEN, "{answer}");
                let current = request(
                    &tasks.directory().join("voice.sock"),
                    &json!({"op":"sign_in_state"}),
                    ASK_DEADLINE,
                )
                .unwrap();
                assert_eq!(current["code_told"], true);
                stale.release.take().unwrap().send(true).unwrap();
                let (offered, open) = reading.join().unwrap();
                assert!(open);
                assert!(
                    offered.is_empty(),
                    "stale state must not supply a second code: {offered:?}"
                );
                assert_eq!(
                    answer["code_to_say"]
                        .as_str()
                        .unwrap()
                        .matches(SPOKEN)
                        .count(),
                    1
                );
            });
            assert_eq!(tasks.starts.load(Ordering::SeqCst), 1);
        }

        fn check_consumed_grant(cancelled: bool) {
            let _thread = crate::provider_worker::tests::one_thread();
            let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            forget();
            let tasks = Tasks::new(if cancelled {
                "cancel-grant"
            } else {
                "lost-grant"
            });
            let mut lost = HeldReply::new(&tasks, "lost", "sign_in_take_code");
            let mut provider = Provider::launch(&lost.directory, "lost-provider", "lost", None);
            let (capability, state) = lost.seen.recv_timeout(WAIT).unwrap();
            assert_eq!(capability["op"], "sign_in_state");
            assert_eq!(state["code_claims"], 1);
            let (start, state) = lost.seen.recv_timeout(WAIT).unwrap();
            assert_eq!(start["op"], "sign_in_start");
            assert_eq!(state["sign_in"], "starting");
            let id = state["id"].as_u64().unwrap();
            tasks.code();
            tasks.read.recv_timeout(WAIT).expect("Harness code read");
            let granted = lost.entered.recv_timeout(WAIT).unwrap();
            assert_eq!(granted["claim"], "granted");
            assert_eq!(granted["id"], id);
            assert_eq!(granted["code"], CODE);
            if cancelled {
                provider.child.kill().unwrap();
                assert!(!provider.child.wait().unwrap().success());
            }
            // Tasks has granted; no byte of that grant reaches the provider.
            lost.release.take().unwrap().send(false).unwrap();
            if !cancelled {
                let answer = provider.answer();
                assert!(answer.get("error").is_some(), "{answer}");
                assert!(answer.get("code_to_say").is_none(), "{answer}");
                assert!(!answer.to_string().contains(CODE));
                assert!(!answer.to_string().contains(SPOKEN));
            }
            drop(provider);
            let mut replacement =
                Provider::launch(&tasks.directory(), "replacement", "claim", Some(id));
            let continued = replacement.answer();
            assert_eq!(continued["claim"], "already_given");
            assert_eq!(continued["id"], id);
            assert!(continued.get("code").is_none());
            drop(replacement);

            // Model whole-Voice local-state loss, without claiming a real
            // coordinator restart: Tasks remains the same live owner.
            forget();
            let mut fresh_follow = Follow::default();
            assert!(
                news(&tasks.directory(), &BTreeSet::new(), &mut fresh_follow)
                    .0
                    .is_empty()
            );
            tasks.code();
            tasks
                .read
                .recv_timeout(WAIT)
                .expect("Harness keepalive read");
            let mut restarted =
                Provider::launch(&tasks.directory(), "fresh-voice", "claim", Some(id));
            let continued = restarted.answer();
            assert_eq!(continued["claim"], "already_given");
            assert!(continued.get("code").is_none());
            drop(restarted);
            // Only a genuinely new heard START authorizes another payload.
            let mut repeat = Provider::start(&tasks.directory());
            assert_eq!(repeat.answer()["code_to_say"], SPOKEN);
            assert!(
                news(&tasks.directory(), &BTreeSet::new(), &mut fresh_follow)
                    .0
                    .is_empty()
            );
            assert_eq!(tasks.starts.load(Ordering::SeqCst), 1);
        }

        #[test]
        fn a_lost_grant_reply_is_not_refunded_to_a_replacement_provider() {
            check_consumed_grant(false);
        }

        #[test]
        fn a_provider_killed_after_its_grant_does_not_reopen_initial_delivery() {
            check_consumed_grant(true);
        }

        #[test]
        fn a_requested_repeat_before_queued_code_drains_keeps_the_initial_news_in_order() {
            let _thread = crate::provider_worker::tests::one_thread();
            let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            forget();
            let tasks = Tasks::new("repeat-queued");
            let started = request(
                &tasks.directory().join("voice.sock"),
                &json!({"op":"sign_in_start"}),
                ASK_DEADLINE,
            )
            .unwrap();
            assert_eq!(started["sign_in"], "starting");
            tasks.code();
            tasks.read.recv_timeout(WAIT).expect("Harness code read");
            let mut follow = Follow::default();
            let (offered, open) = news(&tasks.directory(), &BTreeSet::new(), &mut follow);
            assert!(open);
            assert_eq!(offered.len(), 1);
            let code = offered.into_iter().next().unwrap();
            let code_key = code.key.clone();
            let (mut waiting, _heard) = TaskNews::all_waiting_for_test(
                vec![
                    ordinary_news("before"),
                    News {
                        key: code.key,
                        note: code.note,
                        until: code.until,
                        progress: None,
                        finished: None,
                        also: Vec::new(),
                    },
                    ordinary_news("after"),
                ],
                Duration::ZERO,
            );
            assert_eq!(waiting.waiting().unwrap().key, "before");
            assert!(waiting.sign_in_first());
            let mut repeat = Provider::start(&tasks.directory());
            assert_eq!(repeat.answer()["code_to_say"], SPOKEN);
            // The minimal claim contract permits this requested payload
            // before the older initial note. It does not revoke that note.
            assert_eq!(waiting.waiting().unwrap().key, code_key);
            let initial = waiting.tell().unwrap();
            assert_eq!(initial.note.matches(SPOKEN).count(), 1);
            assert_eq!(waiting.tell().unwrap().key, "before");
            assert_eq!(waiting.tell().unwrap().key, "after");
            assert!(waiting.waiting().is_none());
            assert!(
                news(&tasks.directory(), &BTreeSet::new(), &mut follow)
                    .0
                    .is_empty()
            );
            assert_eq!(tasks.starts.load(Ordering::SeqCst), 1);
        }
    }

    /// Jarvis's P2 and the reviewer's P3 5 on #2137: while a sign-in's code
    /// and account are kept, the person's pieces on Live are held to their
    /// line's end and leave whole, scrubbed; two minutes after the sign-in
    /// ended both are forgotten, and pieces leave as they come again.
    #[test]
    fn the_code_is_kept_only_for_its_sign_in_and_held_pieces_leave_whole() {
        let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        forget();
        assert_eq!(person_piece("four eight", false), "four eight");
        keep_said(Some("48213907"), Some("someone@example.com"));
        assert_eq!(person_piece("It said 4 8", false), "");
        assert_eq!(person_piece(" 2 1 3 9", false), "");
        assert_eq!(
            person_piece(" 0 7, someone", true),
            "It said a sign-in code, their account"
        );
        // Ended: still kept a while, for the person may say it back.
        said_ended();
        assert_eq!(person_piece("four eight two", true), "a sign-in code");
        ended_for_test(KEPT_AFTER);
        assert_eq!(scrub_piece("48213907, someone"), "48213907, someone");
        assert_eq!(person_piece("four eight", false), "four eight");
        forget();
    }

    #[test]
    fn the_code_is_said_in_two_groups_and_the_email_slowly() {
        // Said again only through the tool, never from memory (#2602
        // finding 5).
        let start = &tools()[0].description;
        assert!(
            start.contains("call it again") && start.contains("Never say"),
            "{start}"
        );
        assert_eq!(code_to_say("12345678"), "1 2 3 4, 5 6 7 8");
        assert_eq!(code_to_say("1234567"), "1 2 3 4, 5 6 7");
        assert_eq!(code_to_say("1234"), "1 2 3 4");
        assert_eq!(
            spoken_email("j.doe-x_y+z@mail.example.com"),
            "j dot doe dash x underscore y plus z at mail dot example dot com"
        );
        let note = asking_note("someone@example.com");
        assert!(note.contains("\"Shall I sign in as someone at example dot com?\""));
        assert!(note.contains(ANSWER));
        assert!(!note.contains('@'));
    }

    #[test]
    fn each_state_is_told_as_the_model_needs_it() {
        let code = told(&json!({"sign_in": "code", "code": "12345678", "expires_in": 120}));
        assert_eq!(code["code_to_say"], "1 2 3 4, 5 6 7 8");
        assert!(code["next"].as_str().unwrap().contains("Approve a device"));
        assert!(code.get("error").is_none());
        assert!(
            told(&json!({"sign_in": "starting"}))["next"]
                .as_str()
                .unwrap()
                .contains("in a moment")
        );
        assert_eq!(
            told(&json!({"sign_in": "done", "result": "success"}))["result"],
            "The lamp is signed in to Harness. Say so in a few words."
        );
        for (code, why) in [
            ("DENIED", "not approved"),
            ("EXPIRED", "ran out of time"),
            ("harness_too_old", "needs updating"),
            ("BACKEND_ERROR", "could not sign it in"),
        ] {
            let told = told(&json!({"sign_in": "done", "result": "error", "code": code}));
            assert!(told["error"].as_str().unwrap().contains(why), "{code}");
        }
    }

    /// #1676, through a stand-in tasks part: never without the switch, nor
    /// for a request the lamp didn't hear; the code to say; the question,
    /// once approved in Harness, as news, once; its hearing told to the
    /// tasks part with its request; the yes passed on with its own; a
    /// refusal there said; how it ended as news, once. The worker's calls
    /// and the news share nothing in this process but the tasks part, as on
    /// a lamp, where they are two processes (#2137's redesign).
    #[test]
    fn a_sign_in_by_voice_goes_from_its_code_to_its_news() {
        use crate::agent_tasks::{DEADLINE, Place, run as tasks_run};
        let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        forget();
        let directory = std::env::temp_dir().join(format!("voice-sign-in-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let mut place = Place {
            directory: directory.clone(),
            target: crate::agent_tasks::DEFAULT_TARGET.into(),
            who: false,
            lead_in: false,
            asked: false,
            accounts: false,
            machine_names: false,
            needs: None,
            staff_note: false,
            make_skill: false,
            named_only: false,
            correct: false,
            questions: false,
            short_confirm: false,
            sign_in: false,
        };
        let call = |name: &str, args: Value| ToolCall {
            id: "c".into(),
            name: name.into(),
            args,
        };
        let email = "someone@example.com";
        let asking = json!({"ok":true,"sign_in":"asking","email":email,"id":1});
        let replies = CodeReplies::on_voice(
            &directory,
            vec![
                (
                    "voice.sock",
                    json!({"ok":true,"sign_in":"none","code_claims":1}),
                ),
                (
                    "voice.sock",
                    json!({"ok":true,"sign_in":"code","code":"12345678","expires_in":120,"code_told":true,"id":1}),
                ),
                (
                    "voice.sock",
                    json!({"ok":true,"sign_in":"code","code":"12345678","expires_in":110,"code_told":true,"id":1}),
                ),
                ("voice.sock", asking.clone()),
                ("voice.sock", asking.clone()),
                ("voice.sock", asking.clone()),
                ("voice.sock", json!({"ok":false,"error":"too_late"})),
                (
                    "voice.sock",
                    json!({"ok":true,"sign_in":"answered","answer":"yes","id":1}),
                ),
                (
                    "voice.sock",
                    json!({"ok":true,"sign_in":"done","result":"success","id":1}),
                ),
                (
                    "voice.sock",
                    json!({"ok":true,"sign_in":"done","result":"success","id":1}),
                ),
            ],
        );
        let seen = &replies.seen;
        // Off: not even asked of the tasks part.
        let (told, _) = tasks_run(&place, &call(START, json!({})), 3, true, DEADLINE);
        assert_eq!(told, crate::tools::unavailable());
        place.sign_in = true;
        // A request the lamp wrote, or a text: refused, nothing asked.
        let (told, _) = tasks_run(&place, &call(START, json!({})), 3, false, DEADLINE);
        assert!(
            told["error"]
                .as_str()
                .unwrap()
                .contains("speaking to the lamp")
        );
        assert!(seen.try_recv().is_err());
        let (told, answered) = tasks_run(&place, &call(START, json!({})), 3, true, DEADLINE);
        assert!(answered);
        assert_eq!(told["code_to_say"], "1 2 3 4, 5 6 7 8", "{told}");
        assert_eq!(seen.recv().unwrap(), json!({"op":"sign_in_state"}));
        assert_eq!(seen.recv().unwrap(), json!({"op":"sign_in_start"}));
        // The news, with its own follow, as the coordinator's: the code was
        // said by the start's answer, so no news of it, but read again soon.
        let mut follow = Follow::default();
        let mut offered = std::collections::BTreeSet::new();
        assert_eq!(news(&directory, &offered, &mut follow), (Vec::new(), true));
        // The question, once.
        let (told_news, open) = news(&directory, &offered, &mut follow);
        assert!(open);
        assert_eq!(told_news.len(), 1, "{told_news:?}");
        let key = told_news[0].key.clone();
        assert_eq!(key, "sign-in-1-asking");
        assert_eq!(asked_of(&key), Some(1));
        assert_eq!(asked_of("s1:2:3"), None);
        assert!(told_news[0].note.contains("someone at example dot com"));
        offered.insert(key.clone());
        assert!(news(&directory, &offered, &mut follow).0.is_empty());
        for _ in 0..3 {
            assert_eq!(seen.recv().unwrap(), json!({"op":"sign_in_state"}));
        }
        // A yes, not heard: never passed on.
        let yes = call(ANSWER, json!({"answer":"yes"}));
        let (told, _) = tasks_run(&place, &yes, 5, false, DEADLINE);
        assert!(told.get("error").is_some());
        assert!(seen.try_recv().is_err());
        // Heard whole in request 4: told to the tasks part, which decides.
        assert!(tell_asked(&directory, 1, 4));
        assert_eq!(
            seen.recv().unwrap(),
            json!({"op":"sign_in_asked","id":1,"request":4})
        );
        // Refused there: said, and why.
        let (told, _) = tasks_run(&place, &yes, 5, true, DEADLINE);
        assert!(
            told["error"].as_str().unwrap().contains("too long ago"),
            "{told}"
        );
        assert_eq!(
            seen.recv().unwrap(),
            json!({"op":"sign_in_answer","answer":"yes","request":5})
        );
        let (told, _) = tasks_run(&place, &yes, 6, true, DEADLINE);
        assert!(
            told["result"].as_str().unwrap().contains("signing in"),
            "{told}"
        );
        assert_eq!(
            seen.recv().unwrap(),
            json!({"op":"sign_in_answer","answer":"yes","request":6})
        );
        // Done: said once, and not read soon again.
        let (told_news, open) = news(&directory, &offered, &mut follow);
        assert!(!open);
        assert_eq!(told_news.len(), 1);
        assert_eq!(told_news[0].key, "sign-in-1-done");
        assert!(told_news[0].note.contains("signed in to Harness"));
        offered.insert(told_news[0].key.clone());
        assert!(news(&directory, &offered, &mut follow).0.is_empty());
        drop(replies);
        forget();
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// The reviewer's P3 on #2137: a refused start says why; only the
    /// switch being off is "not set up".
    #[test]
    fn a_refused_start_says_why() {
        use crate::agent_tasks::{DEADLINE, Place, run as tasks_run};
        let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        forget();
        let directory =
            std::env::temp_dir().join(format!("voice-sign-in-refused-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let place = Place {
            directory: directory.clone(),
            target: crate::agent_tasks::DEFAULT_TARGET.into(),
            who: false,
            lead_in: false,
            asked: false,
            accounts: false,
            machine_names: false,
            needs: None,
            staff_note: false,
            make_skill: false,
            named_only: false,
            correct: false,
            questions: false,
            short_confirm: false,
            sign_in: true,
        };
        // `bad_request`: a tasks part too old for the sign-in's ops.
        let refusals = [
            "sign_in_off",
            "timeout",
            "busy",
            "spawn_failed",
            "bad_request",
        ];
        let replies = CodeReplies::on_voice(
            &directory,
            refusals
                .iter()
                .map(|code| ("voice.sock", json!({"ok":false,"error":code})))
                .collect(),
        );
        let start = ToolCall {
            id: "c".into(),
            name: START.into(),
            args: json!({}),
        };
        let said: Vec<String> = refusals
            .iter()
            .map(|_| {
                let (told, _) = tasks_run(&place, &start, 3, true, DEADLINE);
                told["error"].as_str().unwrap().to_owned()
            })
            .collect();
        assert!(said[0].contains("not set up"), "{said:?}");
        assert!(
            said[1].contains("busy") && said[2].contains("busy"),
            "{said:?}"
        );
        assert!(said[3].contains("could not be started"), "{said:?}");
        assert!(said[1..4].iter().all(|why| !why.contains("not set up")));
        assert!(said[4].contains("finish updating"), "{said:?}");
        drop(replies);
        forget();
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// The privacy fixture owns the asynchronous state read even if an
    /// assertion before its normal join fails. Its existing RPC deadline
    /// still applies; joining is not a hard OS scheduling deadline.
    struct OwnedSignInRead(Option<std::thread::JoinHandle<()>>);

    impl OwnedSignInRead {
        fn new(read: std::thread::JoinHandle<()>) -> Self {
            Self(Some(read))
        }

        fn join(mut self) -> std::thread::Result<()> {
            self.0.take().unwrap().join()
        }
    }

    impl Drop for OwnedSignInRead {
        fn drop(&mut self) {
            if let Some(read) = self.0.take() {
                let result = read.join();
                if !std::thread::panicking() {
                    result.unwrap();
                }
            }
        }
    }

    /// The new protocol matrices own their one trusted listener. Iterate
    /// replies before accepting, so exhausting a script never waits for an
    /// extra connection. Drop releases an idle accept and joins on panic.
    struct CodeReplies {
        seen: std::sync::mpsc::Receiver<Value>,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        server: Option<std::thread::JoinHandle<()>>,
    }

    impl CodeReplies {
        /// These sign-in scripts use only the trusted socket. Preserve
        /// their reply sequence without spawning an unused public listener.
        fn on_voice(directory: &Path, answers: Vec<(&str, Value)>) -> Self {
            let answers = answers
                .into_iter()
                .map(|(socket, answer)| {
                    assert_eq!(socket, "voice.sock");
                    answer
                })
                .collect();
            Self::new(directory, answers)
        }

        fn new(directory: &Path, answers: Vec<Value>) -> Self {
            use std::{
                io::{BufRead, BufReader, Write},
                os::unix::net::UnixListener,
                sync::{
                    Arc,
                    atomic::{AtomicBool, Ordering},
                    mpsc,
                },
            };
            const WAIT: Duration = Duration::from_secs(5);
            let listener = UnixListener::bind(directory.join("voice.sock")).unwrap();
            listener.set_nonblocking(true).unwrap();
            let (sent, seen) = mpsc::channel();
            let stop = Arc::new(AtomicBool::new(false));
            let stopped = stop.clone();
            let server = std::thread::spawn(move || {
                for answer in answers {
                    let until = Instant::now() + WAIT;
                    let mut stream = loop {
                        if stopped.load(Ordering::Acquire) {
                            return;
                        }
                        match listener.accept() {
                            Ok((stream, _)) => break stream,
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                assert!(Instant::now() < until, "sign-in mock expected a request");
                                std::thread::sleep(Duration::from_millis(5));
                            }
                            Err(error) => panic!("sign-in mock accept: {error}"),
                        }
                    };
                    stream.set_nonblocking(false).unwrap();
                    stream.set_read_timeout(Some(WAIT)).unwrap();
                    stream.set_write_timeout(Some(WAIT)).unwrap();
                    let mut line = String::new();
                    assert_ne!(BufReader::new(&stream).read_line(&mut line).unwrap(), 0);
                    sent.send(serde_json::from_str(&line).unwrap()).unwrap();
                    writeln!(stream, "{answer}").unwrap();
                }
            });
            Self {
                seen,
                stop,
                server: Some(server),
            }
        }
    }

    impl Drop for CodeReplies {
        fn drop(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::Release);
            let result = self.server.take().unwrap().join();
            if !std::thread::panicking() {
                result.unwrap();
            }
        }
    }

    #[test]
    fn a_code_handoff_needs_the_claim_protocol_and_never_restarts_its_poll() {
        let _thread = crate::provider_worker::tests::one_thread();
        let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let capable = json!({"ok":true,"sign_in":"none","code_claims":1});
        let starting = json!({"ok":true,"sign_in":"starting","id":7});
        let granted = json!({"ok":true,"claim":"granted","sign_in":"code","id":7,"code":"12345678","expires_in":120});
        let cases = [
            (
                vec![json!({"ok":true,"sign_in":"none"})],
                vec!["sign_in_state"],
                "finish updating",
                false,
            ),
            (
                vec![capable.clone(), json!({"ok":false,"error":"bad_request"})],
                vec!["sign_in_state", "sign_in_start"],
                "finish updating",
                false,
            ),
            (
                vec![
                    capable.clone(),
                    starting.clone(),
                    json!({"ok":false,"error":"bad_request"}),
                ],
                vec!["sign_in_state", "sign_in_start", "sign_in_take_code"],
                "finish updating",
                false,
            ),
            (
                vec![
                    capable.clone(),
                    starting.clone(),
                    json!({"ok":true,"claim":"already_given","id":7}),
                ],
                vec!["sign_in_state", "sign_in_start", "sign_in_take_code"],
                "already being provided",
                false,
            ),
            (
                vec![capable.clone(), starting.clone(), granted.clone()],
                vec!["sign_in_state", "sign_in_start", "sign_in_take_code"],
                "",
                true,
            ),
            (
                vec![
                    capable.clone(),
                    json!({"ok":true,"sign_in":"code","id":7,"code":"12345678","expires_in":0}),
                ],
                vec!["sign_in_state", "sign_in_start"],
                "no longer available",
                false,
            ),
            (
                vec![
                    capable.clone(),
                    starting.clone(),
                    json!({"ok":true,"claim":"granted","sign_in":"code","id":7,"code":"12345678","expires_in":0}),
                ],
                vec!["sign_in_state", "sign_in_start", "sign_in_take_code"],
                "no longer available",
                false,
            ),
            // A busy or timed-out owner while Harness is still starting:
            // nothing was granted, so the code is said to be coming.
            (
                vec![
                    capable.clone(),
                    starting.clone(),
                    json!({"ok":false,"error":"busy"}),
                ],
                vec!["sign_in_state", "sign_in_start", "sign_in_take_code"],
                "read it out in a moment",
                false,
            ),
            (
                vec![
                    capable.clone(),
                    starting.clone(),
                    json!({"ok":false,"error":"timeout"}),
                ],
                vec!["sign_in_state", "sign_in_start", "sign_in_take_code"],
                "read it out in a moment",
                false,
            ),
            (
                vec![
                    capable,
                    starting,
                    json!({"ok":true,"claim":"granted","sign_in":"code","id":8,"code":"12345678","expires_in":120}),
                ],
                vec!["sign_in_state", "sign_in_start", "sign_in_take_code"],
                "no longer available",
                false,
            ),
        ];
        for (index, (answers, operations, explanation, says_code)) in cases.into_iter().enumerate()
        {
            let directory =
                std::env::temp_dir().join(format!("voice-claim-{}-{index}", std::process::id()));
            std::fs::create_dir_all(&directory).unwrap();
            let replies = CodeReplies::new(&directory, answers);
            let seen = &replies.seen;
            let call = ToolCall {
                id: "claim".into(),
                name: START.into(),
                args: json!({}),
            };
            let (answer, handled) = run(&directory, &call, 41, true, crate::agent_tasks::DEADLINE);
            assert!(handled, "case {index}: {answer}");
            assert_eq!(
                answer.get("code_to_say").is_some(),
                says_code,
                "case {index}: {answer}"
            );
            if says_code {
                assert_eq!(answer["code_to_say"], "1 2 3 4, 5 6 7 8");
            } else {
                assert!(
                    answer.to_string().contains(explanation),
                    "case {index}: {answer}"
                );
                assert!(!answer.to_string().contains("12345678"));
            }
            for operation in operations {
                let observed = seen.recv_timeout(Duration::from_secs(5)).unwrap();
                assert_eq!(observed["op"], operation, "case {index}");
                if operation == "sign_in_take_code" {
                    assert_eq!(observed["id"], 7);
                }
            }
            assert!(seen.try_recv().is_err());
            drop(replies);
            std::fs::remove_dir_all(directory).unwrap();
            forget();
        }
    }

    #[test]
    fn code_validity_cannot_be_extended_by_a_delayed_reply() {
        let sent = Instant::now().checked_sub(Duration::from_secs(2)).unwrap();
        assert!(
            code_until(
                &json!({"sign_in":"code","code":"12345678","expires_in":1}),
                sent
            )
            .is_none()
        );
        assert!(
            code_until(
                &json!({"sign_in":"code","code":"12x45678","expires_in":120}),
                Instant::now()
            )
            .is_none()
        );
        let sent = Instant::now();
        assert_eq!(
            code_until(
                &json!({"sign_in":"code","code":"12345678","expires_in":120}),
                sent
            ),
            Some(sent + Duration::from_secs(120))
        );
    }

    #[test]
    fn news_never_reads_digits_from_an_unclaimed_or_expired_snapshot() {
        let _thread = crate::provider_worker::tests::one_thread();
        let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = json!({"ok":true,"sign_in":"code","id":7,"code":"11112222","expires_in":120,"code_told":false,"code_claims":1});
        let mut legacy = state.clone();
        legacy.as_object_mut().unwrap().remove("code_claims");
        let cases = [
            (vec![legacy], None),
            (
                vec![state.clone(), json!({"ok":false,"error":"bad_request"})],
                None,
            ),
            (
                vec![
                    state.clone(),
                    json!({"ok":true,"claim":"already_given","id":7}),
                ],
                None,
            ),
            (
                vec![
                    state.clone(),
                    json!({"ok":true,"claim":"granted","sign_in":"code","id":7,"code":"33334444","expires_in":0}),
                ],
                None,
            ),
            (
                vec![
                    state,
                    json!({"ok":true,"claim":"granted","sign_in":"code","id":7,"code":"33334444","expires_in":120}),
                ],
                Some("3 3 3 3, 4 4 4 4"),
            ),
        ];
        for (index, (answers, expected)) in cases.into_iter().enumerate() {
            let directory =
                std::env::temp_dir().join(format!("news-claim-{}-{index}", std::process::id()));
            std::fs::create_dir_all(&directory).unwrap();
            let requests = answers.len();
            let replies = CodeReplies::new(&directory, answers);
            let seen = &replies.seen;
            let (notes, open) = news(&directory, &BTreeSet::new(), &mut Follow::default());
            assert!(open);
            assert_eq!(notes.len(), usize::from(expected.is_some()));
            if let Some(expected) = expected {
                assert!(notes[0].note.contains(expected));
                assert!(!notes[0].note.contains("1 1 1 1, 2 2 2 2"));
                assert!(notes[0].until.is_some_and(|until| until > Instant::now()));
            }
            assert_eq!(
                seen.recv_timeout(Duration::from_secs(5)).unwrap()["op"],
                "sign_in_state"
            );
            if requests == 2 {
                assert_eq!(
                    seen.recv_timeout(Duration::from_secs(5)).unwrap(),
                    json!({"op":"sign_in_take_code","id":7})
                );
            }
            assert!(seen.try_recv().is_err());
            drop(replies);
            std::fs::remove_dir_all(directory).unwrap();
            forget();
        }
    }

    /// The 21 Oct lamp-75c9 rerun's (3), #2602: a question taken to be told
    /// is due again once its wait has passed, once a telling, and never
    /// once heard whole; at most [`ASKED_MOST`] tellings in all. Other news
    /// isn't followed.
    #[test]
    fn a_question_not_heard_whole_is_due_again_a_few_times_at_most() {
        let after = ASK_AGAIN_AFTER;
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        let none = std::collections::BTreeSet::new();
        let mut again = AskAgain::default();
        assert!(!again.told("sign-in-1-code", at(0)));
        assert!(!again.told("s1:2:3", at(0)));
        assert!(again.told("sign-in-1-asking", at(0)));
        assert!(again.heard(&none).is_empty());
        assert!(again.due(after, at(29)).is_empty());
        assert_eq!(again.due(after, at(30)), ["sign-in-1-asking"]);
        // Once a telling.
        assert!(again.due(after, at(90)).is_empty());
        assert!(again.told("sign-in-1-asking", at(40)));
        assert_eq!(again.due(after, at(70)), ["sign-in-1-asking"]);
        // The third telling is the last, and let go once its wait passed.
        assert!(again.told("sign-in-1-asking", at(80)));
        assert!(again.due(after, at(100)).is_empty());
        assert_eq!(again.followed(), 1);
        assert!(again.due(after, at(110)).is_empty());
        assert_eq!(again.followed(), 0);
        // Heard whole: let go, to be kept as told, and never due.
        assert!(again.told("sign-in-2-asking", at(0)));
        let heard = std::collections::BTreeSet::from([2]);
        assert_eq!(again.heard(&heard), ["sign-in-2-asking"]);
        assert!(again.heard(&heard).is_empty());
        assert!(again.due(after, at(100)).is_empty());
        assert_eq!(again.followed(), 0);
    }

    /// The reviewer's P2 and P3 on #2137: a sign-in first seen done gets no
    /// news (the start's own answer told it); after a no, its end isn't
    /// told; a code the start's answer didn't give is; a new sign-in is
    /// followed afresh.
    #[test]
    fn a_sign_in_is_never_told_twice_nor_its_end_after_a_no() {
        let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        forget();
        let directory =
            std::env::temp_dir().join(format!("voice-sign-in-twice-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let state = |value: Value| ("voice.sock", value);
        let replies = CodeReplies::on_voice(
            &directory,
            vec![
                state(json!({"ok":true,"sign_in":"none"})),
                state(
                    json!({"ok":true,"sign_in":"done","result":"error","code":"no_harness","id":1}),
                ),
                state(
                    json!({"ok":true,"sign_in":"code","code":"24681357","expires_in":120,"code_told":false,"code_claims":1,"id":2}),
                ),
                state(
                    json!({"ok":true,"claim":"granted","sign_in":"code","code":"24681357","expires_in":120,"code_told":true,"id":2}),
                ),
                state(json!({"ok":true,"sign_in":"answered","answer":"no","id":2})),
                state(
                    json!({"ok":true,"sign_in":"done","result":"error","code":"CANCELLED","id":2}),
                ),
                state(json!({"ok":true,"sign_in":"done","result":"error","code":"EXPIRED","id":3})),
                state(json!({"ok":false,"error":"sign_in_off"})),
            ],
        );
        let mut follow = Follow::default();
        let offered = std::collections::BTreeSet::new();
        let mut read = || news(&directory, &offered, &mut follow);
        assert_eq!(read(), (Vec::new(), false));
        // Started and failed at once: the start's answer said so.
        assert_eq!(read(), (Vec::new(), false));
        // A code that came after the start's answer: told, with its digits
        // to say.
        let (told, open) = read();
        assert!(open);
        assert_eq!(told[0].key, "sign-in-2-code");
        assert!(told[0].note.contains("2 4 6 8, 1 3 5 7"), "{told:?}");
        // A no, and Harness's end after it: not told.
        assert_eq!(read(), (Vec::new(), true));
        assert_eq!(read(), (Vec::new(), false));
        // Another, first seen done: nothing either.
        assert_eq!(read(), (Vec::new(), false));
        // Refused, or off: nothing, and not read soon.
        assert_eq!(read(), (Vec::new(), false));
        drop(replies);
        forget();
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// Step 3 of #2137's redesign (Jarvis's tests 1–3, with his planted
    /// values): a sign-in's news marks the request it is said in, and the
    /// code and the account are read from the tasks part. The lamp's words
    /// of a marked request leave the worker as one line, however they were
    /// split; the person's, piece by piece, with the code's digits however
    /// said and the account's name and domain taken out. Other numbers and
    /// requests are left as they were.
    #[test]
    fn what_leaves_the_worker_has_neither_the_code_nor_the_account() {
        use crate::provider_worker::ProviderOutput;
        let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        forget();
        let directory =
            std::env::temp_dir().join(format!("voice-sign-in-leaving-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let replies = CodeReplies::on_voice(
            &directory,
            vec![(
                "voice.sock",
                json!({"ok":true,"sign_in":"asking","email":"planted-jarvis@example.invalid","id":1}),
            )],
        );
        let seen = &replies.seen;
        serve_from(&directory);
        // Not a sign-in's: nothing marked, nothing asked.
        assert!(written(7, "Oh, Claude Code just finished.").is_none());
        assert!(!marked(7));
        assert!(seen.try_recv().is_err());
        // Read off the worker's loop: the request is marked at once.
        let read = OwnedSignInRead::new(
            written(
                8,
                &format!(
                    "{NEWS_TAG}Shall I sign in as planted dash jarvis at example dot invalid?"
                ),
            )
            .unwrap(),
        );
        assert!(marked(8));
        assert_eq!(seen.recv().unwrap(), json!({"op":"sign_in_state"}));
        read.join().unwrap();
        // The code, as the start's answer gave it in another request.
        keep_said(Some("48213907"), None);
        mark(5);
        // The lamp's words, in pieces split mid-group: one line, once.
        let lamp = |request: u64, text: &str| {
            let mut output = ProviderOutput::Transcript {
                request: Some(request),
                text: text.into(),
                finished: false,
            };
            leaving(&mut output);
            // Again, as a send tried twice: the same.
            leaving(&mut output);
            match output {
                ProviderOutput::Transcript { text, .. } => text,
                _ => unreachable!(),
            }
        };
        assert_eq!(lamp(5, "Your code is 4 8 2"), SAID_LINE);
        assert_eq!(lamp(5, "1, 3 9 0 7. Type it"), "");
        assert_eq!(lamp(8, "Shall I sign in as planted dash"), SAID_LINE);
        assert_eq!(lamp(8, " jarvis at example dot invalid?"), "");
        assert_eq!(lamp(9, "It's 10 past 4."), "It's 10 past 4.");
        // An unmarked request that says the code anyway (asked for it again,
        // lamp-75c9, 21 Oct) has it taken out, as the person's words.
        assert_eq!(lamp(10, "Four eight two one, three nine"), "a sign-in code");
        assert_eq!(
            lamp(10, " at planted-jarvis, yes."),
            " at their account, yes."
        );
        // The person's words, in pieces, in all their forms.
        for (piece, kept) in [
            ("It said 4 8 2", "It said a sign-in code"),
            ("1, 3 9 0 7", "a sign-in code"),
            ("four eight two one, three nine", "a sign-in code"),
            ("four eight two, then five", "a sign-in code, then five"),
            ("48213907", "a sign-in code"),
            ("yes, planted-jarvis", "yes, their account"),
            ("at example.invalid", "at their account"),
            ("planted dash jarvis at", "their account at"),
            ("Example Dot Invalid please", "their account please"),
            (
                "Remind me at 2 pm in 10 minutes",
                "Remind me at 2 pm in 10 minutes",
            ),
            ("one two", "one two"),
        ] {
            assert_eq!(leaving_text(None, piece.into()), kept, "{piece}");
        }
        drop(replies);
        forget();
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// The first builder at 09:46 on #1676: the code and the email are never
    /// kept in what the thread keeps, however the code's digits were spaced
    /// and however the email was said; other numbers are left.
    #[test]
    fn the_code_and_the_email_are_taken_out_of_what_is_kept() {
        let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        forget();
        assert_eq!(scrub("Code 1 2 3 4, 5 6 7 8."), "Code 1 2 3 4, 5 6 7 8.");
        keep_said(Some("12345678"), Some("J.Doe_x@Example.com"));
        for (said, kept) in [
            (
                "The code is 1 2 3 4, 5 6 7 8. Type it.",
                "The code is a sign-in code. Type it.",
            ),
            ("12345678", "a sign-in code"),
            ("It's 1234-5678.", "It's a sign-in code."),
            (
                "Shall I sign in as j dot doe underscore x at example dot com?",
                "Shall I sign in as their account?",
            ),
            (
                "Signed in as j.doe_x@example.com.",
                "Signed in as their account.",
            ),
            (
                "Set a timer for 1 2 3 minutes, at 12:34.",
                "Set a timer for 1 2 3 minutes, at 12:34.",
            ),
        ] {
            assert_eq!(scrub(said), kept, "{said}");
        }
        // Through the thread: what its file keeps of the person and the
        // lamp.
        let now = Instant::now();
        let mut thread = crate::thread::Thread::default();
        thread.person("Sign in as j.doe_x@example.com.", true, now);
        thread.lamp(9, "The code is 1 2 3 4, 5 6 7 8.", now);
        thread.finished(9, now);
        let kept = thread.save(now, std::time::SystemTime::now()).unwrap();
        assert!(
            kept.contains("a sign-in code") && kept.contains("their account"),
            "{kept}"
        );
        assert!(
            !kept.contains("1 2 3 4") && !kept.contains("example"),
            "{kept}"
        );
        forget();
    }

    /// Jarvis's planted run on #2137 (P2s 1 and 2), in the thread: the
    /// person's words and the lamp's in 3-word pieces, none holding the
    /// whole code or address, are scrubbed joined; and a cut answer's played
    /// words (`heard_only`) are too.
    #[test]
    fn pieces_too_short_to_scrub_alone_are_scrubbed_joined_in_the_thread() {
        let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        forget();
        keep_said(Some("48213907"), Some("planted-jarvis@example.invalid"));
        let now = Instant::now();
        let mut thread = crate::thread::Thread::default();
        for piece in ["Planted dash", " jarvis at example", " dot invalid"] {
            thread.person(piece, false, now);
        }
        thread.person(".", true, now);
        for piece in ["Yes, it's 4", " 8 2 1,", " 3 9 0", " 7."] {
            thread.lamp(26, piece, now);
        }
        thread.finished(26, now);
        thread.lamp(27, "Your code is 4 8 2 1, 3 9", now);
        thread.heard_only(27, "Your code is 4 8 2 1, 3 9 0 7");
        thread.finished(27, now);
        let kept = thread.save(now, std::time::SystemTime::now()).unwrap();
        for planted in ["jarvis", "example", "invalid", "4 8 2", "3 9 0", "48213907"] {
            assert!(!kept.contains(planted), "{planted}: {kept}");
        }
        assert!(kept.contains("a sign-in code"), "{kept}");
        forget();
    }

    /// `LAMP_TASKS_SIGN_IN=on` reaches the worker as its flag, after the
    /// others, and declares the two functions; anything else declares none.
    #[test]
    fn the_switch_reaches_the_worker_and_declares_the_two_functions() {
        use crate::agent_tasks::{FLAG, MAKE_SKILL_FLAG, SIGN_IN_FLAG, take};
        let flags = crate::agent_tasks::tests::arguments_for_sign_in(Some("on"), Some("on"));
        assert_eq!(flags, [FLAG, "/run/t", MAKE_SKILL_FLAG, SIGN_IN_FLAG]);
        let (taken, rest) = take(&flags).unwrap();
        let place = taken.unwrap();
        assert!(place.sign_in && place.make_skill && rest.is_empty());
        let names: Vec<String> = crate::agent_tasks::tools(&place)
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        assert!(names.contains(&START.to_owned()) && names.contains(&ANSWER.to_owned()));
        for value in [None, Some("yes"), Some("ON")] {
            let flags = crate::agent_tasks::tests::arguments_for_sign_in(None, value);
            let place = take(&flags).unwrap().0.unwrap();
            assert!(!place.sign_in, "{value:?}");
            assert!(
                crate::agent_tasks::tools(&place)
                    .iter()
                    .all(|tool| !is_tool(&tool.name)),
                "{value:?}"
            );
        }
    }
}
