//! One thread per run handed over: reads the run's SSE
//! (`GET /v1/runs/{id}/events`), folds every event into the shared runs,
//! answers each `approval.request` at once by policy, and settles the run
//! when the stream breaks.
//!
//! The stream breaking is never an answer. The run's status is read
//! (`GET /v1/runs/{id}`): terminal ends the run as it says; still going
//! reopens the stream from the last `seq` (`Last-Event-ID`); the gateway
//! not answering (refused), or no longer knowing the run (404, a restart),
//! ends it at once with [`crate::DROPPED`]: the person is told to try
//! again, and nothing is run again.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::http::{self, Chunk, HttpError, Req, Sse, SseParser};
use super::runs::{Applied, Runs};
use super::{ApprovalPolicy, ApprovalRequest, Logger};
use crate::{DROPPED, RunId};

/// What the session and its run threads share.
#[derive(Default)]
pub(crate) struct Shared {
    st: Mutex<State>,
    pub cv: Condvar,
}

impl Shared {
    pub(crate) fn lock(&self) -> MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[derive(Default)]
pub(crate) struct State {
    pub runs: Runs,
    /// A run thread found the gateway gone (why): the session checks it
    /// again before the next hand-over.
    pub down: Option<String>,
}

/// What a run thread needs.
#[derive(Clone)]
pub(crate) struct Ctx {
    pub base: String,
    pub key: String,
    pub shared: Arc<Shared>,
    pub policy: ApprovalPolicy,
    pub log: Logger,
    pub stop: Arc<AtomicBool>,
    /// The bound on one status read, stop or approval answer.
    pub call_timeout: Duration,
    /// No byte on the stream this long (the server writes a keepalive every
    /// 10 s): the stream is taken as broken.
    pub stream_idle: Duration,
    /// Unanswered status reads (a reset, a timeout, a 5xx) for this long
    /// end the run as dropped.
    pub settle: Duration,
    /// The pause between status reads while settling.
    pub poll: Duration,
}

enum Stream {
    /// The run ended (an event or the status said so).
    Ended,
    /// Broke or ended without a terminal event.
    Broke(String),
    /// The server has no stream for the run (swept, or a restarted gateway).
    NotFound,
    /// The gateway did not answer at all.
    Gone(String),
    Refused(String),
}

enum Status {
    Ended,
    Live,
    /// Unanswered (reset, timeout, 5xx): tried again within the settle.
    Retry(String),
    Gone(String),
    Refused(String),
}

impl Ctx {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }

    fn open(&self, id: &RunId) -> bool {
        self.shared.lock().runs.is_open(id)
    }

    fn auth(&self) -> String {
        format!("Bearer {}", self.key)
    }

    /// Sleeps `d` in short steps, waking early on stop.
    fn pause(&self, d: Duration) {
        let until = Instant::now() + d;
        while !self.stopped() {
            let Some(rest) = until.checked_duration_since(Instant::now()) else {
                return;
            };
            std::thread::sleep(rest.min(Duration::from_millis(50)));
        }
    }

    fn end(&self, id: &RunId, why: &str, dropped: bool) {
        let mut st = self.shared.lock();
        st.runs.fail(id, if dropped { DROPPED } else { why });
        if dropped {
            st.down = Some(why.to_owned());
        }
        drop(st);
        self.shared.cv.notify_all();
        (self.log)(&format!(
            "run {} ended{}: {why}",
            id.0,
            if dropped { " by the drop" } else { "" }
        ));
    }
}

/// Follows run `id` until it ends or `ctx.stop`.
pub(crate) fn follow(ctx: Ctx, id: RunId) {
    let mut poll_only = false;
    let mut trouble: Option<Instant> = None;
    loop {
        if ctx.stopped() || !ctx.open(&id) {
            return;
        }
        if !poll_only && trouble.is_none() {
            match stream_once(&ctx, &id) {
                Stream::Ended => return,
                Stream::Broke(why) => {
                    if ctx.stopped() {
                        return;
                    }
                    (ctx.log)(&format!("run {}: the event stream broke ({why})", id.0));
                }
                Stream::NotFound => poll_only = true,
                Stream::Gone(why) => return ctx.end(&id, &why, true),
                Stream::Refused(why) => return ctx.end(&id, &why, false),
            }
        }
        if ctx.stopped() {
            return;
        }
        match status_once(&ctx, &id) {
            Status::Ended => return,
            Status::Live => {
                trouble = None;
                ctx.pause(if poll_only {
                    Duration::from_secs(1)
                } else {
                    ctx.poll
                });
            }
            Status::Retry(why) => {
                let since = *trouble.get_or_insert_with(Instant::now);
                if since.elapsed() >= ctx.settle {
                    return ctx.end(
                        &id,
                        &format!("no run status for {} s: {why}", ctx.settle.as_secs()),
                        true,
                    );
                }
                ctx.pause(ctx.poll);
            }
            Status::Gone(why) => return ctx.end(&id, &why, true),
            Status::Refused(why) => return ctx.end(&id, &why, false),
        }
    }
}

fn stream_once(ctx: &Ctx, id: &RunId) -> Stream {
    let path = format!("/v1/runs/{}/events", id.0);
    let last_seq = ctx.shared.lock().runs.get(id).and_then(|r| r.last_seq);
    let mut req = Req::new("GET", &path)
        .header("Authorization", ctx.auth())
        .header("Accept", "text/event-stream");
    if let Some(seq) = last_seq {
        req = req.header("Last-Event-ID", seq.to_string());
    }
    let opened = http::stream(&ctx.base, &req, Instant::now() + ctx.call_timeout);
    let mut resp = match opened {
        Ok(r) => r,
        Err(HttpError::Connect(why)) => return Stream::Gone(why),
        Err(HttpError::Io(why)) => return Stream::Broke(why),
    };
    match resp.status {
        200 => {}
        404 => return Stream::NotFound,
        401 | 403 => return Stream::Refused(format!("the run's events: HTTP {}", resp.status)),
        other => return Stream::Broke(format!("HTTP {other}")),
    }
    let mut parser = SseParser::default();
    let mut last_byte = Instant::now();
    loop {
        if ctx.stopped() {
            return Stream::Broke("stopped".to_owned());
        }
        match resp
            .body
            .next_line(ctx.stream_idle.min(Duration::from_millis(500)))
        {
            Ok(Some(line)) => {
                last_byte = Instant::now();
                let Some(Sse::Event { data, .. }) = parser.line(&line) else {
                    continue;
                };
                let Ok(evt) = serde_json::from_str::<Value>(&data) else {
                    continue;
                };
                if on_event(ctx, id, &evt) {
                    return Stream::Ended;
                }
            }
            Ok(None) => return Stream::Broke("the stream ended".to_owned()),
            Err(Chunk::Idle) => {
                if last_byte.elapsed() >= ctx.stream_idle {
                    return Stream::Broke(format!(
                        "nothing on the stream for {} s",
                        ctx.stream_idle.as_secs()
                    ));
                }
            }
            Err(Chunk::Broken(why)) => return Stream::Broke(why),
        }
    }
}

/// One event in: whether the run ended.
fn on_event(ctx: &Ctx, id: &RunId, evt: &Value) -> bool {
    let approval = evt.get("event").and_then(Value::as_str) == Some("approval.request");
    let applied = ctx.shared.lock().runs.on_event(id, evt);
    ctx.shared.cv.notify_all();
    if approval && applied != Applied::Nothing {
        answer(ctx, id, evt);
    }
    applied == Applied::Ended
}

/// Answers one approval prompt at once: never left waiting on a person.
fn answer(ctx: &Ctx, id: &RunId, evt: &Value) {
    let ask = ApprovalRequest::from_event(evt);
    let decision = (ctx.policy)(&ask);
    let mut body = json!({"choice": decision.word()});
    if !ask.request_id.is_empty() {
        body["request_id"] = json!(ask.request_id);
    }
    let path = format!("/v1/runs/{}/approval", id.0);
    let req = Req::new("POST", &path)
        .header("Authorization", ctx.auth())
        .json(&body);
    let result = match http::call(&ctx.base, &req, Instant::now() + ctx.call_timeout) {
        Ok(r) if r.status == 200 => "sent".to_owned(),
        Ok(r) => r.error_words(),
        Err(e) => e.to_string(),
    };
    (ctx.log)(&format!(
        "run {}: approval {} for `{}`: {} ({result})",
        id.0,
        ask.request_id,
        crate::openclaw::brief(&Value::String(ask.command.clone())),
        decision.word()
    ));
}

fn status_once(ctx: &Ctx, id: &RunId) -> Status {
    let path = format!("/v1/runs/{}", id.0);
    let req = Req::new("GET", &path).header("Authorization", ctx.auth());
    match http::call(&ctx.base, &req, Instant::now() + ctx.call_timeout) {
        Err(HttpError::Connect(why)) => Status::Gone(why),
        Err(HttpError::Io(why)) => Status::Retry(why),
        Ok(r) if r.status == 200 => {
            let ended = ctx.shared.lock().runs.on_status(id, &r.json());
            ctx.shared.cv.notify_all();
            if ended { Status::Ended } else { Status::Live }
        }
        Ok(r) if r.status == 404 => {
            Status::Gone(format!("the gateway no longer knows run {}", id.0))
        }
        Ok(r) if matches!(r.status, 401 | 403) => Status::Refused(r.error_words()),
        Ok(r) => Status::Retry(r.error_words()),
    }
}
