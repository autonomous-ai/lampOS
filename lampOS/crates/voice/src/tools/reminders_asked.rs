//! A line that is only a plain question about the person's reminders calls
//! `list_reminders` (`LAMP_REMINDERS_ASKED`). On lamp-52e6 on 9 Nov (build
//! 10972, the product unit, played lines), "Lamp, what reminders do I
//! have?" was answered in words with no call in 2 of 8 asks, with nothing
//! told by memory, so the lamp said what it could not know. The journal
//! tagged those lines as about the lamp's own work
//! (`attention::about_its_work`), but that is any line with "reminder" in
//! it ("set a reminder at nine", "do you like reminders?"), too wide to
//! force a call on, so the question is told apart here. With the switch,
//! such a line's first chat round must call `list_reminders`
//! (`tool_choice`). A line with any other word (a set, a cancel, a move, a
//! timer, a story), a round after a call, and `list_reminders` not
//! declared are asked as before. Off, nothing is forced.
use crate::known_faces::lower_words;
use crate::second_provider::net::{Force, Forced, Message};

/// Only the exact value `on`; off unless set.
pub const ENVIRONMENT: &str = "LAMP_REMINDERS_ASKED";

/// Whether the switch is on.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// The function that reads the reminders back.
pub const LIST: &str = "list_reminders";

/// The questions, as words in a row.
pub(crate) const ASKS: &[&[&str]] = &[
    &["what", "reminders", "do", "i", "have"],
    &["what", "reminders", "i", "have"],
    &["what", "reminders", "have", "i", "got"],
    &["do", "i", "have", "any", "reminders"],
    &["have", "i", "got", "any", "reminders"],
    &["are", "there", "any", "reminders"],
    &["what", "are", "my", "reminders"],
    &["what're", "my", "reminders"],
    &["list", "my", "reminders"],
    &["read", "my", "reminders"],
    &["tell", "me", "my", "reminders"],
];

/// Every other word the line may hold: a greeting, the lamp's name, a
/// "please", "can you", "tell me", a day. Any word not here or in the
/// question ("remind", "cancel", "delete", "move", "timer", "like") leaves
/// the line to the model: a missed force only means the model answers as
/// without the switch.
const AROUND: &[&str] = &[
    "hey", "hi", "hello", "ok", "okay", "lamp", "so", "oh", "um", "uh", "please", "now", "right",
    "can", "could", "would", "you", "tell", "me", "set", "for", "today", "tonight", "tomorrow",
];

/// Whether a line is only a plain question about the person's reminders:
/// exactly one question ([`ASKS`]) and no word but those [`AROUND`].
pub fn asks_for_reminders(said: &str) -> bool {
    let words = lower_words(said);
    let mut asked = false;
    let mut at = 0;
    while at < words.len() {
        let found = ASKS.iter().find(|ask| {
            ask.iter()
                .enumerate()
                .all(|(next, word)| words.get(at + next).is_some_and(|said| said == word))
        });
        if let Some(ask) = found {
            if asked {
                return false;
            }
            asked = true;
            at += ask.len();
        } else if AROUND.contains(&words[at].as_str()) {
            at += 1;
        } else {
            return false;
        }
    }
    asked
}

/// How a line refused by [`asks_for_reminders`] reads, read on past where
/// it stops: counts and a place, never a word.
#[derive(Debug, Default)]
struct Read {
    words: usize,
    /// The questions ([`ASKS`]) found in it.
    asks: usize,
    /// Its words in no question found and not [`AROUND`].
    outside: usize,
    /// The first of them, counted from 1.
    first_outside: Option<usize>,
}

fn read(words: &[String]) -> Read {
    let mut read = Read {
        words: words.len(),
        ..Read::default()
    };
    let mut at = 0;
    while at < words.len() {
        let found = ASKS.iter().find(|ask| {
            ask.iter()
                .enumerate()
                .all(|(next, word)| words.get(at + next).is_some_and(|said| said == word))
        });
        if let Some(ask) = found {
            read.asks += 1;
            at += ask.len();
        } else {
            if !AROUND.contains(&words[at].as_str()) {
                read.outside += 1;
                read.first_outside.get_or_insert(at + 1);
            }
            at += 1;
        }
    }
    read
}

/// Why round 1 of `request` was not forced to [`LIST`] (#8181): for a
/// person's last line with a word of "remind" in it that [`force`]
/// refuses, as said and past the lamp's name, a journal line saying which
/// reading it reports (`past_name` when the line opens with the lamp's
/// name, the reading the force tries last, else `as_said`) and which check
/// refused it there (`no_ask`: none of [`ASKS`]; `two_asks`; `outside`: a
/// word in no question and not [`AROUND`]), with its counts and the first
/// such word's place. In gate 1126 a 2nd line's reminders question said
/// `forced=none` with 7 of 7 words heard, and the journal could not say
/// which word fell outside. Counts only, never a word. `None` for a line
/// the force forces, or one with no word of "remind" (most lines).
pub(crate) fn declined_line(request: u64, messages: &[Message]) -> Option<String> {
    let said = messages
        .last()
        .filter(|message| message.role == "user")?
        .words()?;
    if force()(messages).is_some()
        || !lower_words(said)
            .iter()
            .any(|word| word.starts_with("remind"))
    {
        return None;
    }
    let (reading, text) = match crate::second_provider::past_the_name(said) {
        Some(past) => ("past_name", past),
        None => ("as_said", said),
    };
    let read = read(&lower_words(text));
    let refused = match read.asks {
        0 => "no_ask",
        1 => "outside",
        _ => "two_asks",
    };
    Some(format!(
        "lamp-voice second: request {request}: round 1: not forced to {LIST}: read={reading} refused={refused} words={} asks={} outside={} first_outside_at={} ({ENVIRONMENT})",
        read.words,
        read.asks,
        read.outside,
        read.first_outside
            .map_or("none".to_owned(), |at| at.to_string()),
    ))
}

/// The force the second route's chat is given with the switch on
/// (`second_provider::net::Chat::forcing`): a request whose last message is
/// a person's plain question about their reminders must call
/// [`LIST`], which the chat forces only when it is declared; read past the
/// lamp's name too (`second_provider::forced_past_the_name`).
pub(crate) fn force() -> Force {
    crate::second_provider::forced_past_the_name(as_said())
}

/// [`force`] on the line as said.
fn as_said() -> Force {
    std::sync::Arc::new(|messages: &[Message]| {
        let last = messages.last().filter(|message| message.role == "user")?;
        asks_for_reminders(last.words()?).then_some(Forced {
            name: LIST,
            only: None,
            fields: None,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::second_provider::net::{Chat, Declare};
    use std::sync::Arc;

    /// The played line on lamp-52e6 (9 Nov) and its plain kin are forced;
    /// a set, a change, a cancel, a timer and talk about reminders are not.
    #[test]
    fn a_plain_question_about_reminders_is_told_apart() {
        for said in [
            "Lamp, what reminders do I have?",
            "What reminders do I have",
            "Hey Lamp, do I have any reminders?",
            "What are my reminders?",
            "Lamp, list my reminders, please.",
            "Can you tell me what reminders I have?",
            "Do I have any reminders for tomorrow?",
        ] {
            assert!(asks_for_reminders(said), "{said}");
        }
        for said in [
            "Remind me to call Sam at six.",
            "What's on my timer?",
            "Do you like reminders?",
            "What reminders do I have? Cancel the first one.",
            "Delete my reminders.",
            "Move my reminders to tomorrow.",
            "Set a reminder at nine.",
            "What timers do I have?",
            "What reminders do I have, and remind me at nine.",
            "Lamp.",
            "",
        ] {
            assert!(!asks_for_reminders(said), "{said}");
        }
    }

    /// The chat sends `tool_choice` naming `list_reminders` only on the
    /// first round of a plain question, and only when it is declared.
    #[test]
    fn a_plain_question_forces_list_reminders_only_when_offered() {
        let tool = |name: &str| lamp_gemini::Tool {
            name: name.into(),
            description: String::new(),
            parameters: None,
            silent: false,
        };
        let chat = |names: &'static [&'static str]| {
            let declare: Declare = Arc::new(move || names.iter().map(|name| tool(name)).collect());
            let chat = Chat::deepseek("deepseek-chat", "k", declare, false).unwrap();
            chat.forcing(force());
            chat
        };
        let choice =
            |chat: &Chat, messages: &[Message]| chat.wire(messages).2.get("tool_choice").cloned();
        let listed = chat(&["set_timer", "list_reminders"]);
        let asked = vec![
            Message::system("You are a lamp."),
            Message::user("Lamp, what reminders do I have?"),
        ];
        assert_eq!(
            choice(&listed, &asked),
            Some(serde_json::json!({"type": "function", "function": {"name": LIST}}))
        );
        let mut after_a_call = asked.clone();
        after_a_call.push(Message::tool("1", "{\"items\": []}"));
        assert_eq!(choice(&listed, &after_a_call), None, "round 2");
        assert_eq!(
            choice(&chat(&["set_timer"]), &asked),
            None,
            "list_reminders not offered"
        );
        let set = vec![Message::user("Remind me to call Sam at six.")];
        assert_eq!(choice(&listed, &set), None);
    }

    /// A reminders line the force refuses says which check refused it and
    /// where, in counts, never a word (#8181); a forced line, a line with
    /// no word of "remind" and a round after a call say nothing.
    #[test]
    fn a_refused_reminders_line_says_why_in_counts_only() {
        let line = |said: &str| {
            declined_line(
                7,
                &[Message::system("You are a lamp."), Message::user(said)],
            )
        };
        for (said, why) in [
            (
                "Hey, Lamp. What reminders do I have, Sam?",
                "read=past_name refused=outside words=6 asks=1 outside=1 first_outside_at=6",
            ),
            (
                "Hey, Lamp. What reminder do I have?",
                "read=past_name refused=no_ask words=5 asks=0 outside=5 first_outside_at=1",
            ),
            (
                "What reminders do I have? Do I have any reminders?",
                "read=as_said refused=two_asks words=10 asks=2 outside=0 first_outside_at=none",
            ),
            (
                "Remind me to call Sam at six.",
                "read=as_said refused=no_ask words=7 asks=0 outside=6 first_outside_at=1",
            ),
        ] {
            assert_eq!(
                line(said).as_deref(),
                Some(
                    format!(
                        "lamp-voice second: request 7: round 1: not forced to list_reminders: {why} (LAMP_REMINDERS_ASKED)"
                    )
                    .as_str()
                ),
                "{said}"
            );
        }
        // Forced, as said or past the name: nothing to say.
        assert_eq!(line("Hey, Lamp. What reminders do I have?"), None);
        assert_eq!(line("Hey, Lamb. What reminders do I have?"), None);
        assert_eq!(line("Turn the light blue."), None);
        let mut after_a_call = vec![Message::user("What reminder do I have?")];
        after_a_call.push(Message::tool("1", "{}"));
        assert_eq!(declined_line(7, &after_a_call), None);
    }

    /// A question about the person's own reminders that the force does not
    /// read is the model's choice, from the descriptions alone: on
    /// lamp-52e6 in gate 1126 (#8181) such a line, the 2nd of a
    /// conversation, went to faq's `lamp_help` with `topic` `reminders`.
    /// So `list_reminders` says it is for their own reminders and timers
    /// at any point, not `lamp_help`, and faq's description says it is not
    /// for their own, which are `list_reminders`'. Each within its bound:
    /// a function's 1,024 bytes, and faq's whole line in `use_skill`.
    #[test]
    fn list_reminders_and_lamp_help_say_which_is_for_their_own_reminders() {
        let listing = lamp_actions::schedule::LIST_DESCRIBED;
        for words in [
            "their own reminders or timers at any point",
            "not recall or lamp_help",
        ] {
            assert!(listing.contains(words), "{words}: {listing}");
        }
        assert!(listing.len() <= 1_024, "{}", listing.len());
        let faq = include_str!("../../../../skills/faq/SKILL.md");
        let field = |key: &str| {
            faq.lines()
                .find_map(|line| line.strip_prefix(key))
                .unwrap_or_else(|| panic!("no {key}"))
        };
        for key in ["description: ", "when: "] {
            let said = field(key);
            assert!(
                said.contains("not for their own reminders or timers") && said.contains(LIST),
                "{key}{said}"
            );
        }
        assert!(field("when: ").len() <= 200);
    }
}
