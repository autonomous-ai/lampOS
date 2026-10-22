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

/// The most a short text the lamp sends unasked carries, in bytes: one line
/// a phone shows whole in its notice.
pub const MESSAGE_TEXT_BYTES: usize = 280;

/// A short text the lamp sends unasked, for the person's phone
/// (`messages/out`): a skill's news while they are away, as guard mode's
/// "Someone was at your desk at 14:31, for about 2 minutes."
/// (`docs/guard-mode.md`). With `LAMP_MESSAGES=on`, `lamp-cloud` carries it
/// up as unasked news, which the backend pushes to the owner's phones
/// (`lamp_cloud::unasked`, `docs/messages.md`, "Unasked texts").
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageOut {
    /// Who sends it: `skill:<name>`, stamped by the part that publishes it,
    /// never by the skill's own files.
    pub from: String,
    /// One line, at most [`MESSAGE_TEXT_BYTES`]: never a picture, and a
    /// name of anyone the lamp saw only in guard mode's named text, for
    /// someone who said yes to being named (`LAMP_GUARD_NAMES`, #2562).
    pub text: String,
    /// With a name in `text`: the same line as it reads with no name, the
    /// words guard mode says with names off. `lamp-cloud` sends `text` only
    /// when it sends at once and the eyes still say each of [`Self::named`]
    /// said yes; else this, so a held, retried or late text never names
    /// anyone who took their yes back (#2562). Left out when there is no
    /// name, so an older reader reads the message as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unnamed: Option<String>,
    /// With `unnamed`: who `text` names, as their
    /// [`crate::known::person_id`]s, never their names, for `lamp-cloud` to
    /// check against the eyes' `tellable` as it sends
    /// ([`crate::known::still_tellable`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub named: Vec<String>,
}

/// The most people a named text names (guard mode's visit keeps three).
pub const MOST_NAMED: usize = 3;

impl MessageOut {
    pub const TYPE: &str = "lamp_msgs/MessageOut";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "messages/out";
    /// A text not taken off the lamp within this long is old news, and is
    /// not handed to a subscriber after.
    pub const LASTS: Duration = Duration::from_secs(600);

    /// A text sent: no period, nothing kept, good for [`MessageOut::LASTS`].
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event).lasting(Self::LASTS)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        check_id("from", &self.from)?;
        if self.unnamed.is_some() == self.named.is_empty()
            || self.named.len() > MOST_NAMED
            || !self.named.iter().all(|id| crate::known::is_person_id(id))
        {
            return Err(Invalid(format!(
                "a named text has its unnamed words and 1..{MOST_NAMED} person ids, or neither"
            )));
        }
        for text in std::iter::once(&self.text).chain(&self.unnamed) {
            if text.trim().is_empty()
                || text.len() > MESSAGE_TEXT_BYTES
                || text.chars().any(char::is_control)
            {
                return Err(Invalid(format!(
                    "a text sent unasked is one line of 1..{MESSAGE_TEXT_BYTES} bytes"
                )));
            }
        }
        Ok(())
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
        (MessageOut::TOPIC, MessageOut::declaration()),
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

    /// #2562: a named text carries its unnamed words; with none the field is
    /// left out, and a message from an older writer, with no field, reads as
    /// one with no name. The unnamed line is held to the same bounds.
    #[test]
    fn a_named_text_carries_its_unnamed_words_and_older_readers_are_unaffected() {
        let id = crate::known::person_id("Minh");
        let named = MessageOut {
            from: "skill:guard-mode".into(),
            text: "Minh came in at 18:10.".into(),
            unnamed: Some("Someone you've introduced me to came in at 18:10.".into()),
            named: vec![id.clone()],
        };
        assert_eq!(named.check(), Ok(()));
        let json = serde_json::to_string(&named).unwrap();
        assert_eq!(
            json,
            format!(
                r#"{{"from":"skill:guard-mode","text":"Minh came in at 18:10.","unnamed":"Someone you've introduced me to came in at 18:10.","named":["{id}"]}}"#
            )
        );
        assert!(!json.contains("\"Minh\""), "an id, never a name: {json}");
        assert_eq!(serde_json::from_str::<MessageOut>(&json).unwrap(), named);
        let old: MessageOut =
            serde_json::from_str(r#"{"from":"skill:guard-mode","text":"Hi."}"#).unwrap();
        assert_eq!(old.unnamed, None);
        assert!(old.named.is_empty());
        // Both or neither, and ids, never names.
        for (unnamed, ids) in [
            (None, vec![id.clone()]),
            (Some("Someone came in."), vec![]),
            (Some("Someone came in."), vec!["Minh".to_owned()]),
            (Some("Someone came in."), vec![id.clone(); MOST_NAMED + 1]),
        ] {
            let mut broken = named.clone();
            broken.unnamed = unnamed.map(str::to_owned);
            broken.named = ids;
            assert!(broken.check().is_err(), "{broken:?}");
        }
        for bad in ["", "two\nlines"] {
            let mut broken = named.clone();
            broken.unnamed = Some(bad.into());
            assert!(broken.check().is_err(), "{bad:?}");
        }
    }

    /// Guard mode's text (#1685): one line from a named sender, short enough
    /// for a phone's notice, and old news after ten minutes.
    #[test]
    fn a_text_sent_unasked_is_one_short_line_from_its_sender() {
        let sent = |from: &str, text: &str| MessageOut {
            from: from.into(),
            text: text.into(),
            unnamed: None,
            named: Vec::new(),
        };
        let text = "Someone was at your desk at 14:31, for about 2 minutes.";
        assert_eq!(sent("skill:guard-mode", text).check(), Ok(()));
        assert_eq!(
            serde_json::to_string(&sent("skill:guard-mode", "Hi.")).unwrap(),
            r#"{"from":"skill:guard-mode","text":"Hi."}"#
        );
        for (from, text) in [
            ("", text),
            ("skill guard", text),
            ("skill:guard-mode", "  "),
            ("skill:guard-mode", "two\nlines"),
            ("skill:guard-mode", &"a".repeat(MESSAGE_TEXT_BYTES + 1)),
        ] {
            assert!(sent(from, text).check().is_err(), "{from:?} {text:?}");
        }
        assert!(
            sent("skill:guard-mode", &"a".repeat(MESSAGE_TEXT_BYTES))
                .check()
                .is_ok()
        );
        assert_eq!(
            MessageOut::declaration().lifespan(),
            Some(MessageOut::LASTS)
        );
        assert!(
            chat_topics()
                .iter()
                .any(|(topic, _)| *topic == "messages/out")
        );
    }
}
