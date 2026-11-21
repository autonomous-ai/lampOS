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

    /// #7987: on lamp-52e6 at the gate's Mac volume 45, "Hey Lamp, what
    /// reminders do I have?" was heard as all 7 words in about 17 of 60
    /// plays, mostly as 6. Every 6-word hearing is forced unless the word
    /// lost is "reminders"; a 5-word hearing only when one of the two lost
    /// is "Hey" or "Lamp", or "do" (which leaves "what reminders I have").
    #[test]
    fn a_plain_question_one_word_short_is_still_forced() {
        let words = ["Hey", "Lamp,", "what", "reminders", "do", "I", "have?"];
        let without = |lost: &[usize]| {
            words
                .iter()
                .enumerate()
                .filter(|(at, _)| !lost.contains(at))
                .map(|(_, word)| *word)
                .collect::<Vec<_>>()
                .join(" ")
        };
        // Indices into `words`: 0 Hey, 1 Lamp, 2 what, 3 reminders, 4 do,
        // 5 I, 6 have.
        for lost in 0..words.len() {
            let said = without(&[lost]);
            assert_eq!(asks_for_reminders(&said), lost != 3, "{said}");
        }
        let forced_two: &[[usize; 2]] = &[
            [0, 1],
            [0, 2],
            [0, 4],
            [0, 5],
            [0, 6],
            [1, 2],
            [1, 4],
            [1, 5],
            [1, 6],
            [2, 4],
            [4, 5],
            [4, 6],
        ];
        for first in 0..words.len() {
            for second in first + 1..words.len() {
                let said = without(&[first, second]);
                assert_eq!(
                    asks_for_reminders(&said),
                    forced_two.contains(&[first, second]),
                    "{said}"
                );
            }
        }
        // A word misheard as another, the other phrases one word short.
        for said in [
            "Hey Lamp, what reminders do I halve?",
            "Hey Lamp, what reminders to I have?",
            "Hey Lamp, what reminders do eye have?",
            "Hey Lamp, do I have reminders?",
            "Hey Lamp, are there reminders?",
        ] {
            assert!(asks_for_reminders(said), "{said}");
        }
    }

    /// One word short never forces a line that asks something else: the
    /// word lost or misheard is never "reminders" or "my", never one that
    /// changes the ask (a set, a cancel, a timer, a need, a "not"), never a
    /// word outside the question (a person's name, "to set"), and never a
    /// second one.
    #[test]
    fn one_word_short_never_forces_another_ask() {
        for said in [
            "Remind me what I have.",
            "What reminders do I have to set?",
            "Cancel reminders do I have?",
            "Cancel what reminders do I have?",
            "What timers do I have?",
            "What alarms do I have?",
            "Do you like reminders?",
            "What reminders do I need?",
            "What reminders should I have?",
            "What reminders do I set?",
            "What reminders don't I have?",
            "What reminder do I have?",
            "What are reminders?",
            "What are your reminders?",
            "Delete my reminders.",
            "Hey Anna, what reminders do I have?",
            "Hey Lamp, what reminders do I have, Anna?",
            "Hey Lamp, reminders do have?",
            "What reminders do halve?",
            "What reminders do I halve, please, Sam?",
            "Reminders do I have? Reminders do I have?",
            "What reminders do I have and what reminders do have?",
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
}
