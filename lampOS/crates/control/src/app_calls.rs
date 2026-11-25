//! An installed app's tool calls (#8406, `LAMP_APP_TOOLS`, off;
//! docs/apps.md, "An app's tools"). The voice publishes the model's call on
//! `apps/call`; lamp-control sends it to the app's socket as `app.call`
//! `{call, name, args}`, only for a running app whose newest `apps/tools`
//! lists that tool, one call in flight an app. The app answers with the op
//! `app.answer` `{call, result | error}`, and lamp-control tells the voice
//! on `apps/called`, once a call: the app's answer, or the lamp's word
//! (`not running`, `unknown tool`, `busy`, `too_long`, `timeout`). A call
//! unanswered after 4.5 s (under the voice's 5 s) is ended as `timeout`, and the app is told
//! `app.call.stopped` `{call, why}` so it knows a later answer is not taken.
//!
//! Nothing here starts with the switch off: no topic is read or published,
//! `hello` says no `tools`, and `app.answer` is refused `off`.
use crate::app_yes::Live;
use crate::protocol::{Error, ErrorCode};
use crate::server::Shared;
use lamp_msgs::{AppCall, AppCalled, AppTools};
use serde_json::{Map, Value, json};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// `on`, and nothing else.
pub const ENVIRONMENT: &str = "LAMP_APP_TOOLS";
/// What an app is told of `app.answer` with the switch off.
pub const OFF: &str = "app tools are off on this lamp";
/// How often the calls in flight are looked at for their wait.
const SWEEP_EVERY: Duration = Duration::from_millis(100);
/// An `apps/tools` reading older than this is a dead publisher's
/// (`AppTools::declaration`'s 30 s): no tool is known.
const TOOLS_LAST: Duration = Duration::from_secs(30);

#[cfg(test)]
thread_local! {
    static ON_FOR_TEST: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether app tools are on (`LAMP_APP_TOOLS=on`).
pub fn on() -> bool {
    #[cfg(test)]
    if let Some(on) = ON_FOR_TEST.with(std::cell::Cell::get) {
        return on;
    }
    std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// The switch as a test on this thread sees it.
#[cfg(test)]
pub fn set_on_for_test(on: Option<bool>) {
    ON_FOR_TEST.with(|cell| cell.set(on));
}

/// The event an app is sent for the voice's call.
pub fn call_event(call: &AppCall) -> Value {
    json!({"ev": "app.call", "call": call.call, "name": call.name, "args": call.args})
}

/// The event an app is sent when the lamp stops waiting for its answer.
pub fn stopped_event(call: &str, why: &str) -> Value {
    json!({"ev": "app.call.stopped", "call": call, "why": why})
}

fn refuse(re: u64, code: ErrorCode, why: &str) -> Value {
    crate::protocol::refused(Some(re), &Error::new(code, why))
}

/// The op `app.answer` from app `name`: `call`, the id of its `app.call`,
/// and exactly one of `result`, a JSON object, and `error`, one line of the
/// app's own words (never one of the lamp's, which the voice says as the
/// lamp's). A result over 2,048 bytes or an error over 200 characters ends
/// the call as `too_long`. Answered `{"taken": bool}`: true once, for the
/// app's call in flight within 4.5 s.
pub fn answer_op(
    live: &mut Live,
    name: &str,
    frame: &Map<String, Value>,
    re: u64,
    on: bool,
    now: Instant,
) -> Value {
    if !on {
        return refuse(re, ErrorCode::Off, OFF);
    }
    if let Err(wait) = live.may_answer(name, now) {
        let mut error = Error::new(ErrorCode::Rate, "ten answers a second");
        error.retry_ms = Some(wait.as_millis().max(1) as u64);
        return crate::protocol::refused(Some(re), &error);
    }
    let shape = "app.answer takes call and one of result (an object) or error (one line)";
    if frame
        .keys()
        .any(|key| !matches!(key.as_str(), "id" | "op" | "call" | "result" | "error"))
    {
        return refuse(re, ErrorCode::BadRequest, shape);
    }
    let Some(call) = frame.get("call").and_then(Value::as_str) else {
        return refuse(re, ErrorCode::BadRequest, shape);
    };
    let answer = match (frame.get("result"), frame.get("error")) {
        (Some(result @ Value::Object(_)), None) => {
            if result.to_string().len() > lamp_msgs::ARGS_MOST_BYTES {
                Err("too_long".to_owned())
            } else {
                Ok(result.clone())
            }
        }
        (None, Some(Value::String(error))) => {
            if error.is_empty() || error.chars().any(char::is_control) {
                return refuse(re, ErrorCode::BadRequest, shape);
            }
            if AppCalled::LAMP_WORDS.contains(&error.as_str()) {
                return refuse(
                    re,
                    ErrorCode::BadRequest,
                    "error: the app's own words, never one of the lamp's",
                );
            }
            if error.chars().count() > lamp_msgs::ERROR_MOST {
                Err("too_long".to_owned())
            } else {
                Err(error.clone())
            }
        }
        _ => return refuse(re, ErrorCode::BadRequest, shape),
    };
    let taken = live.answer_call(name, call, answer, now);
    crate::protocol::ok(re, json!({"taken": taken}))
}

/// A call's end as its journal line says it: `answered`, the lamp's word,
/// or `the app's error`. Never the result or the app's words, which may be
/// the person's.
pub fn outcome(called: &AppCalled) -> &str {
    match called.error.as_deref() {
        None => "answered",
        Some(word) if AppCalled::LAMP_WORDS.contains(&word) => word,
        Some(_) => "the app's error",
    }
}

/// Whether `tools`, read at `read_at`, lists `name` for `app` at `now`.
pub fn lists(tools: Option<&(AppTools, Instant)>, app: &str, name: &str, now: Instant) -> bool {
    tools.is_some_and(|(tools, read_at)| {
        now.saturating_duration_since(*read_at) < TOOLS_LAST
            && tools
                .apps
                .iter()
                .any(|each| each.app == app && each.tools.iter().any(|tool| tool.name == name))
    })
}

/// Starts the bus side, only with the switch on: the newest `apps/tools`
/// kept, `apps/call` sent on to the app, the calls' wait looked at, and every
/// end told on `apps/called`. Each part that can't start is said.
pub fn start(bus: &lamp_bus::Bus, shared: Arc<Shared>) {
    if !on() {
        return;
    }
    match bus.publisher_declared(AppCalled::TOPIC, &AppCalled::declaration(), "control") {
        Ok(publisher) => {
            let publisher = Mutex::new(publisher);
            shared.set_called_sink(Arc::new(move |called: &AppCalled| {
                if called.check().is_err() {
                    eprintln!(
                        "lamp-control: app {}: an answer not told, unfit",
                        called.app
                    );
                    return;
                }
                if let Ok(mut publisher) = publisher.lock()
                    && let Err(error) = publisher.publish_json(lamp_ipc::monotonic_us(), called)
                {
                    eprintln!(
                        "lamp-control: app {}: a call's end not told to the voice: {error}",
                        called.app
                    );
                }
            }));
        }
        Err(error) => {
            eprintln!("lamp-control: no app call's end is told: {error}");
            return;
        }
    }
    let tools: Arc<Mutex<Option<(AppTools, Instant)>>> = Arc::new(Mutex::new(None));
    match bus.subscribe(AppTools::TOPIC, "control") {
        Ok(mut subscriber) => {
            let tools = tools.clone();
            let spawned = std::thread::Builder::new()
                .name("app-tools-read".into())
                .spawn(move || {
                    loop {
                        match subscriber.recv(Duration::from_secs(1)) {
                            Ok(Some(message)) => {
                                if let Ok(read) = message.parse::<AppTools>()
                                    && read.check().is_ok()
                                {
                                    *tools.lock().expect("tools") = Some((read, Instant::now()));
                                }
                            }
                            Ok(None) => {}
                            Err(error) => {
                                eprintln!("lamp-control: apps/tools can't be read: {error}");
                                std::thread::sleep(Duration::from_secs(1));
                            }
                        }
                    }
                });
            if let Err(error) = spawned {
                eprintln!("lamp-control: no app's tools are read: {error}");
            }
        }
        Err(error) => eprintln!("lamp-control: no app's tools are read: {error}"),
    }
    match bus.subscribe(AppCall::TOPIC, "control") {
        Ok(mut subscriber) => {
            let shared = shared.clone();
            let spawned = std::thread::Builder::new()
                .name("app-call".into())
                .spawn(move || {
                    let mut unread_said = false;
                    loop {
                        match subscriber.recv(Duration::from_secs(1)) {
                            Ok(Some(message)) => match message.parse::<AppCall>() {
                                Ok(call) if call.check().is_ok() => {
                                    let now = Instant::now();
                                    let listed = lists(
                                        tools.lock().expect("tools").as_ref(),
                                        &call.app,
                                        &call.name,
                                        now,
                                    );
                                    // A call ended at once has its line
                                    // where it is told (`tell_called`).
                                    let said = match shared.send_call(&call, listed, now) {
                                        Ok(()) => Some("sent"),
                                        Err(crate::app_yes::ALREADY) => {
                                            Some("already in flight, passed over")
                                        }
                                        Err(_) => None,
                                    };
                                    if let Some(said) = said {
                                        eprintln!(
                                            "lamp-control: app {}: call {}: {said}",
                                            call.app, call.call
                                        );
                                    }
                                }
                                // Unfit but naming its call and app: ended
                                // at once, not left to the voice's 5 s.
                                Ok(call)
                                    if lamp_msgs::call_id_ok(&call.call)
                                        && lamp_skills::app_package::app_name_ok(&call.app) =>
                                {
                                    shared.end_call(&call, "unknown tool");
                                }
                                Ok(_) => {}
                                Err(error) => {
                                    if !unread_said {
                                        unread_said = true;
                                        eprintln!(
                                            "lamp-control: apps/call: a message not read, passed over: {error}"
                                        );
                                    }
                                }
                            },
                            Ok(None) => {}
                            Err(error) => {
                                eprintln!("lamp-control: apps/call can't be read: {error}");
                                std::thread::sleep(Duration::from_secs(1));
                            }
                        }
                    }
                });
            if let Err(error) = spawned {
                eprintln!("lamp-control: no app call is sent: {error}");
            }
        }
        Err(error) => eprintln!("lamp-control: no app call is sent: {error}"),
    }
    let spawned = std::thread::Builder::new()
        .name("app-call-sweep".into())
        .spawn(move || {
            loop {
                shared.sweep_calls(Instant::now());
                std::thread::sleep(SWEEP_EVERY);
            }
        });
    if let Err(error) = spawned {
        eprintln!("lamp-control: no app call's wait is kept: {error}");
    }
}

#[cfg(test)]
mod tests;
