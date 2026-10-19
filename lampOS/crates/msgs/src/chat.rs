//! A person's text to the lamp from the Autonomous app, and what the lamp
//! sends back (`chat/in`, `chat/out`, #1673, `docs/messages.md`).
//! `lamp-cloud` carries both between the company's broker, as V1's
//! `chat.send` and `chat.event`, and the bus; the voice answers them in
//! text. Off unless `LAMP_MESSAGES=on`.
use crate::Invalid;
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// How long a text waits for its answer: `lamp-cloud` then ends its run as
/// a fault for the app, and a text still on `chat/in` by then is passed
/// over unanswered, its functions never run ([`ChatIn::declaration`]'s
/// lifespan; the reviewer on #1713).
pub const CHAT_ANSWER_WITHIN: Duration = Duration::from_secs(60);

/// The most text one message carries, in bytes: well inside a bus message
/// (60,000 bytes in all, `docs/bus.md`).
pub const CHAT_TEXT_BYTES: usize = 16_000;
/// The longest run or session name, in bytes.
pub const CHAT_ID_BYTES: usize = 64;

/// A text the person sent (`chat/in`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatIn {
    /// The run `lamp-cloud` acked it with: every piece of the reply carries
    /// it, so the app can tell them apart.
    pub run_id: String,
    /// The app's own conversation, echoed back and never read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// What they wrote.
    pub text: String,
    /// V1's `speak`, carried and ignored: the lamp never says a text's reply
    /// aloud, whatever the app asks (`docs/messages.md`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub speak: bool,
}

impl ChatIn {
    pub const TYPE: &str = "lamp_msgs/ChatIn";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "chat/in";

    /// A text sent: no period, nothing kept, and meaningful for
    /// [`CHAT_ANSWER_WITHIN`]: a subscriber is not handed one older.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event).lasting(CHAT_ANSWER_WITHIN)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        check_id("run_id", &self.run_id)?;
        if let Some(session) = &self.session_id {
            check_id("session_id", session)?;
        }
        if self.text.trim().is_empty() {
            return Err(Invalid("a text has words in it".into()));
        }
        check_text(&self.text)
    }
}

/// What a piece of the lamp's reply is. The app reads them as V1's
/// `chat.event` types (`docs/messages.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatPart {
    /// The reply's words so far (V1's `assistant_delta`).
    Delta,
    /// The whole reply, which ends the run (V1's `chat_response`, state
    /// `final`).
    Final,
    /// The run ended without a reply (V1's `chat_response`, state `error`);
    /// `text` says why in words the person can read.
    Error,
    /// A task's news, sent while nobody is at the lamp. No text asked for it,
    /// so it has no run.
    TaskNews,
}

/// A piece of what the lamp sends back (`chat/out`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatOut {
    /// The run it answers; none only for task news.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// The app's conversation the text came in, echoed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub part: ChatPart,
    pub text: String,
}

impl ChatOut {
    pub const TYPE: &str = "lamp_msgs/ChatOut";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "chat/out";

    /// A piece sent: no period, nothing kept.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        match (&self.run_id, self.part) {
            (None, ChatPart::TaskNews)
            | (Some(_), ChatPart::Delta | ChatPart::Final | ChatPart::Error) => {}
            (None, _) => return Err(Invalid("a reply carries the run it answers".into())),
            (Some(_), ChatPart::TaskNews) => {
                return Err(Invalid("task news answers no run".into()));
            }
        }
        if let Some(run) = &self.run_id {
            check_id("run_id", run)?;
        }
        if let Some(session) = &self.session_id {
            check_id("session_id", session)?;
        }
        check_text(&self.text)
    }
}

fn check_id(what: &str, id: &str) -> Result<(), Invalid> {
    if id.is_empty()
        || id.len() > CHAT_ID_BYTES
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(Invalid(format!(
            "{what} is 1..{CHAT_ID_BYTES} letters, digits, '-', '_', '.' or ':'"
        )));
    }
    Ok(())
}

fn check_text(text: &str) -> Result<(), Invalid> {
    if text.len() > CHAT_TEXT_BYTES {
        return Err(Invalid(format!(
            "a text is at most {CHAT_TEXT_BYTES} bytes"
        )));
    }
    Ok(())
}

/// The texting topics (#1673), apart from the first topics.
pub fn chat_topics() -> Vec<(&'static str, Declaration)> {
    vec![
        (ChatIn::TOPIC, ChatIn::declaration()),
        (ChatOut::TOPIC, ChatOut::declaration()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_in(text: &str) -> ChatIn {
        ChatIn {
            run_id: "run-1".into(),
            session_id: Some("abc123".into()),
            text: text.into(),
            speak: false,
        }
    }

    /// The reviewer on #1713: a text the app has given up on is never
    /// answered after, so `chat/in` lasts as long as a run waits.
    #[test]
    fn a_text_lasts_as_long_as_its_run_waits() {
        assert_eq!(ChatIn::declaration().lifespan(), Some(CHAT_ANSWER_WITHIN));
        assert_eq!(ChatOut::declaration().lifespan(), None);
    }

    #[test]
    fn a_text_in_has_words_a_run_and_fits_a_message() {
        assert!(
            text_in("Can you remind me to call Mum at six?")
                .check()
                .is_ok()
        );
        assert!(text_in("   ").check().is_err());
        assert!(text_in(&"a".repeat(CHAT_TEXT_BYTES + 1)).check().is_err());
        let mut no_run = text_in("hi");
        no_run.run_id = String::new();
        assert!(no_run.check().is_err());
        let mut odd = text_in("hi");
        odd.session_id = Some("a b".into());
        assert!(odd.check().is_err());
        let text = serde_json::to_string(&text_in("hi")).unwrap();
        assert_eq!(
            text,
            r#"{"run_id":"run-1","session_id":"abc123","text":"hi"}"#
        );
        let back: ChatIn = serde_json::from_str(&text).unwrap();
        assert!(!back.speak);
    }

    #[test]
    fn a_reply_answers_its_run_and_task_news_none() {
        let reply = |run: Option<&str>, part| ChatOut {
            run_id: run.map(str::to_owned),
            session_id: None,
            part,
            text: "Sure.".into(),
        };
        for part in [ChatPart::Delta, ChatPart::Final, ChatPart::Error] {
            assert!(reply(Some("run-1"), part).check().is_ok(), "{part:?}");
            assert!(reply(None, part).check().is_err(), "{part:?}");
        }
        assert!(reply(None, ChatPart::TaskNews).check().is_ok());
        assert!(reply(Some("run-1"), ChatPart::TaskNews).check().is_err());
        assert_eq!(
            serde_json::to_string(&reply(Some("run-1"), ChatPart::Final)).unwrap(),
            r#"{"run_id":"run-1","part":"final","text":"Sure."}"#
        );
    }
}
