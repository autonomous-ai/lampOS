//! A text from the Autonomous app and the lamp's answer, on V1's wire
//! (#1673, `docs/messages.md`): `chat.send` on `fa_channel` is acked with
//! a run and put on the bus as `chat/in`; what the voice sends back on
//! `chat/out` goes out on `fd_channel` as V1's `chat.event`, its events in
//! V1's `MonitorEvent` shape, which the app already reads. Off unless
//! `LAMP_MESSAGES=on`.
//!
//! As V1's (`docs/mqtt.md`, "`chat.send` + `chat.event`"): the reply's words
//! so far go as `assistant_delta`, gathered for 250 ms so a reply is a few
//! messages and not one per word, with an empty `id` and `coalesced`; the
//! whole reply ends the run as a `chat_response` with `state: "final"`. A
//! run nothing answers within [`ANSWER_WITHIN`] ends as a `chat_response`
//! with `state: "error"`, so the app never waits on a lamp that will not
//! answer.
use crate::online::rfc3339;
use lamp_msgs::{ChatIn, ChatOut, ChatPart};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::{Duration, UNIX_EPOCH};

/// Where `lamp-cloud` reads whether texts are taken.
pub const ENVIRONMENT: &str = "LAMP_MESSAGES";
/// How long the reply's words are gathered before they go, as V1's.
pub const GATHER: Duration = Duration::from_millis(250);
/// How long a run waits for its answer before it ends as a fault: the
/// texts' own lifespan on `chat/in`, so a text the app has given up on is
/// never answered after.
pub const ANSWER_WITHIN: Duration = lamp_msgs::CHAT_ANSWER_WITHIN;
/// How much of the reply a `chat_response`'s `summary` shows, as V1's.
pub const SUMMARY_CHARS: usize = 200;
/// What the app is told when nothing answered in time.
pub const NOT_ANSWERED: &str = "The lamp didn't answer in time. Please try again.";
/// Why a photo or a file is refused, for now.
pub const NO_ATTACHMENTS: &str = "photos and files aren't read on this lamp yet";
/// Why a text is refused when nothing on the lamp takes it.
pub const NOBODY: &str = "nothing on this lamp answers texts right now";
/// How often the session looks for pieces of an answer to send.
pub const TICK: Duration = Duration::from_millis(50);

/// The wall clock in ms, for a run's name and its events' `time`.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// Whether the environment turns texts on: `on` only.
pub fn on_from_environment() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// The text a `chat.send`'s `data` carries, as `run`. `message` is needed;
/// `session_id` and `speak` are taken as V1's; a photo or a file is
/// refused, not dropped, so the person knows it was not read.
pub fn text_in(data: &Value, run: String) -> Result<ChatIn, String> {
    let attached = data
        .get("images")
        .and_then(Value::as_array)
        .is_some_and(|images| !images.is_empty())
        || data.get("file").is_some_and(|file| !file.is_null());
    if attached {
        return Err(NO_ATTACHMENTS.to_owned());
    }
    let text = data
        .get("message")
        .and_then(Value::as_str)
        .ok_or("message must be the text to send")?;
    let session_id = match data.get("session_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(session)) => Some(session.clone()),
        Some(_) => return Err("session_id must be text".to_owned()),
    };
    let speak = match data.get("speak") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(speak)) => *speak,
        Some(_) => return Err("speak must be true or false".to_owned()),
    };
    let text = ChatIn {
        run_id: run,
        session_id,
        text: text.to_owned(),
        speak,
    };
    text.check().map_err(|why| why.0)?;
    Ok(text)
}

/// The runs acked and not yet answered, and the reply's words gathered but
/// not yet sent.
#[derive(Debug, Default)]
pub struct Stream {
    runs: BTreeMap<String, Run>,
    made: u64,
    events: u64,
}

#[derive(Debug)]
struct Run {
    session_id: Option<String>,
    acked_ms: u64,
    /// Words not yet sent, and since when.
    gathered: String,
    gathered_ms: Option<u64>,
}

impl Stream {
    /// A run's name, new each time: `run-` and the time and a count.
    pub fn new_run(&mut self, now_ms: u64) -> String {
        self.made += 1;
        format!("run-{now_ms}-{}", self.made)
    }

    /// `text` was acked at `now_ms`: its answer is waited for.
    pub fn acked(&mut self, text: &ChatIn, now_ms: u64) {
        self.runs.insert(
            text.run_id.clone(),
            Run {
                session_id: text.session_id.clone(),
                acked_ms: now_ms,
                gathered: String::new(),
                gathered_ms: None,
            },
        );
    }

    /// A piece of an answer came at `now_ms`: the `chat.event`s' `data` to
    /// send now. Words are gathered; anything else sends the words first.
    /// A piece for a run not waited for (ended, or never acked here) is
    /// dropped, as V1 mirrors only the runs it acked.
    pub fn piece(&mut self, out: &ChatOut, now_ms: u64) -> Vec<Value> {
        if out.part == ChatPart::TaskNews {
            return vec![self.event(None, out.session_id.as_deref(), news(&out.text), now_ms)];
        }
        let Some(name) = out.run_id.clone() else {
            return Vec::new();
        };
        let Some(run) = self.runs.get_mut(&name) else {
            return Vec::new();
        };
        if out.part == ChatPart::Delta {
            run.gathered.push_str(&out.text);
            run.gathered_ms.get_or_insert(now_ms);
            return Vec::new();
        }
        let mut sent = self.flush(&name, now_ms);
        let run = self.runs.remove(&name).expect("the run was just found");
        let event = match out.part {
            ChatPart::Final => answer(&name, &out.text),
            _ => fault(&name, &out.text),
        };
        sent.push(self.event(Some(&name), run.session_id.as_deref(), event, now_ms));
        sent
    }

    /// At `now_ms`: words gathered for [`GATHER`] go, and a run waited on for
    /// [`ANSWER_WITHIN`] ends as a fault.
    pub fn due(&mut self, now_ms: u64) -> Vec<Value> {
        let gather = GATHER.as_millis() as u64;
        let within = ANSWER_WITHIN.as_millis() as u64;
        let mut sent = Vec::new();
        let names: Vec<String> = self.runs.keys().cloned().collect();
        for name in names {
            let run = &self.runs[&name];
            if run
                .gathered_ms
                .is_some_and(|since| now_ms.saturating_sub(since) >= gather)
            {
                sent.extend(self.flush(&name, now_ms));
            }
            let run = &self.runs[&name];
            if now_ms.saturating_sub(run.acked_ms) >= within {
                sent.extend(self.flush(&name, now_ms));
                let run = self.runs.remove(&name).expect("the run is there");
                sent.push(self.event(
                    Some(&name),
                    run.session_id.as_deref(),
                    fault(&name, NOT_ANSWERED),
                    now_ms,
                ));
            }
        }
        sent
    }

    /// The runs waited on.
    pub fn waiting(&self) -> usize {
        self.runs.len()
    }

    fn flush(&mut self, name: &str, now_ms: u64) -> Vec<Value> {
        let Some(run) = self.runs.get_mut(name) else {
            return Vec::new();
        };
        if run.gathered.is_empty() {
            return Vec::new();
        }
        let words = std::mem::take(&mut run.gathered);
        run.gathered_ms = None;
        let session = run.session_id.clone();
        // V1's gathered delta: the words since the last, an empty `id`.
        let event = json!({
            "type": "assistant_delta",
            "summary": words,
            "runId": name,
            "detail": {"coalesced": true},
        });
        vec![self.event_with_id(Some(name), session.as_deref(), event, now_ms, false)]
    }

    fn event(
        &mut self,
        run: Option<&str>,
        session: Option<&str>,
        event: Value,
        now_ms: u64,
    ) -> Value {
        self.event_with_id(run, session, event, now_ms, true)
    }

    /// A `chat.event`'s `data`: the run, the session and V1's `MonitorEvent`
    /// with its `id` and `time`.
    fn event_with_id(
        &mut self,
        run: Option<&str>,
        session: Option<&str>,
        mut event: Value,
        now_ms: u64,
        with_id: bool,
    ) -> Value {
        let id = if with_id {
            self.events += 1;
            format!("evt-{}", self.events)
        } else {
            String::new()
        };
        event["id"] = json!(id);
        event["time"] = json!(rfc3339(UNIX_EPOCH + Duration::from_millis(now_ms)));
        let mut data = json!({"event": event});
        if let Some(run) = run {
            data["run_id"] = json!(run);
        }
        if let Some(session) = session {
            data["session_id"] = json!(session);
        }
        data
    }
}

/// The whole reply, ending its run.
fn answer(run: &str, text: &str) -> Value {
    json!({
        "type": "chat_response",
        "summary": summary(text),
        "runId": run,
        "state": "final",
        "detail": {"role": "assistant", "message": text, "channel": "mobile"},
    })
}

/// A run ended without a reply, and why.
fn fault(run: &str, why: &str) -> Value {
    json!({
        "type": "chat_response",
        "summary": summary(why),
        "runId": run,
        "state": "error",
        "error": why,
        "detail": {"role": "assistant", "message": why, "channel": "mobile"},
    })
}

/// A task's news while nobody is at the lamp: no run asked for it.
fn news(text: &str) -> Value {
    json!({
        "type": "task_news",
        "summary": summary(text),
        "detail": {"role": "assistant", "message": text, "channel": "mobile"},
    })
}

fn summary(text: &str) -> String {
    text.chars().take(SUMMARY_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sent(data: Value) -> ChatIn {
        text_in(&data, "run-1".into()).unwrap()
    }

    fn piece(run: &str, part: ChatPart, text: &str) -> ChatOut {
        ChatOut {
            run_id: Some(run.into()),
            session_id: None,
            part,
            text: text.into(),
        }
    }

    #[test]
    fn a_chat_send_is_its_message_and_v1s_session_and_speak() {
        let text = sent(json!({"message": "Remind me at six", "session_id": "abc123"}));
        assert_eq!(text.text, "Remind me at six");
        assert_eq!(text.session_id.as_deref(), Some("abc123"));
        assert!(!text.speak);
        assert!(sent(json!({"message": "hi", "speak": true})).speak);
        for bad in [
            json!({}),
            json!({"message": ""}),
            json!({"message": 3}),
            json!({"message": "hi", "speak": "yes"}),
            json!({"message": "hi", "session_id": 7}),
        ] {
            assert!(text_in(&bad, "run-1".into()).is_err(), "{bad}");
        }
        assert_eq!(
            text_in(
                &json!({"message": "look", "images": ["/9j/4AAQ"]}),
                "run-1".into()
            ),
            Err(NO_ATTACHMENTS.to_owned())
        );
        assert_eq!(
            text_in(
                &json!({"message": "read this", "file": {"name": "a.pdf", "content": "JVBE"}}),
                "run-1".into()
            ),
            Err(NO_ATTACHMENTS.to_owned())
        );
        assert!(text_in(&json!({"message": "hi", "images": []}), "run-1".into()).is_ok());
    }

    #[test]
    fn words_are_gathered_for_a_quarter_second_and_the_whole_reply_ends_the_run() {
        let mut stream = Stream::default();
        let text = sent(json!({"message": "hi", "session_id": "s1"}));
        stream.acked(&text, 1_000);
        assert!(
            stream
                .piece(&piece("run-1", ChatPart::Delta, "Hello"), 1_010)
                .is_empty()
        );
        assert!(
            stream
                .piece(&piece("run-1", ChatPart::Delta, " there"), 1_100)
                .is_empty()
        );
        assert!(stream.due(1_200).is_empty(), "not yet 250 ms");
        let words = stream.due(1_260);
        assert_eq!(words.len(), 1);
        assert_eq!(words[0]["event"]["type"], "assistant_delta");
        assert_eq!(words[0]["event"]["summary"], "Hello there");
        assert_eq!(words[0]["event"]["id"], "");
        assert_eq!(words[0]["event"]["detail"]["coalesced"], true);
        assert_eq!(words[0]["run_id"], "run-1");
        assert_eq!(words[0]["session_id"], "s1");
        // Words not yet sent go before the whole reply, never after it.
        assert!(
            stream
                .piece(&piece("run-1", ChatPart::Delta, "!"), 1_300)
                .is_empty()
        );
        let end = stream.piece(&piece("run-1", ChatPart::Final, "Hello there!"), 1_320);
        assert_eq!(end.len(), 2);
        assert_eq!(end[0]["event"]["type"], "assistant_delta");
        assert_eq!(end[0]["event"]["summary"], "!");
        assert_eq!(end[1]["event"]["type"], "chat_response");
        assert_eq!(end[1]["event"]["state"], "final");
        assert_eq!(end[1]["event"]["detail"]["message"], "Hello there!");
        assert_eq!(end[1]["event"]["runId"], "run-1");
        assert!(end[1]["event"]["id"].as_str().unwrap().starts_with("evt-"));
        assert_eq!(end[1]["event"]["time"], "1970-01-01T00:00:01Z");
        assert_eq!(stream.waiting(), 0);
        // A piece for a run that has ended is dropped.
        assert!(
            stream
                .piece(&piece("run-1", ChatPart::Final, "again"), 1_400)
                .is_empty()
        );
    }

    #[test]
    fn a_run_nothing_answers_ends_as_a_fault_after_a_minute() {
        let mut stream = Stream::default();
        stream.acked(&sent(json!({"message": "hi"})), 0);
        assert!(stream.due(59_999).is_empty());
        let ended = stream.due(60_000);
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0]["event"]["type"], "chat_response");
        assert_eq!(ended[0]["event"]["state"], "error");
        assert_eq!(ended[0]["event"]["error"], NOT_ANSWERED);
        assert_eq!(stream.waiting(), 0);
        assert!(stream.due(120_000).is_empty());
    }

    #[test]
    fn a_fault_and_task_news_go_as_v1s_events_and_news_has_no_run() {
        let mut stream = Stream::default();
        stream.acked(&sent(json!({"message": "hi"})), 0);
        let fault = stream.piece(&piece("run-1", ChatPart::Error, "The link is down."), 10);
        assert_eq!(fault[0]["event"]["state"], "error");
        assert_eq!(fault[0]["event"]["error"], "The link is down.");
        let news = stream.piece(
            &ChatOut {
                run_id: None,
                session_id: None,
                part: ChatPart::TaskNews,
                text: "Claude Code finished the report.".into(),
            },
            20,
        );
        assert_eq!(news.len(), 1);
        assert_eq!(news[0]["event"]["type"], "task_news");
        assert!(news[0].get("run_id").is_none());
        assert_eq!(
            news[0]["event"]["detail"]["message"],
            "Claude Code finished the report."
        );
    }

    #[test]
    fn a_summary_is_the_first_two_hundred_characters() {
        let long = "é".repeat(300);
        assert_eq!(summary(&long).chars().count(), SUMMARY_CHARS);
        assert_eq!(summary("short"), "short");
    }

    #[test]
    fn each_run_has_its_own_name() {
        let mut stream = Stream::default();
        let one = stream.new_run(5);
        let two = stream.new_run(5);
        assert_ne!(one, two);
        assert!(
            lamp_msgs::ChatIn {
                run_id: one,
                session_id: None,
                text: "hi".into(),
                speak: false,
            }
            .check()
            .is_ok()
        );
    }
}
