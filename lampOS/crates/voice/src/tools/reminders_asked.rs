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
//! declared are asked as before. Its question may be one word short, one
//! word lost or misheard, but never "reminders" and never with a word that
//! changes the ask (#7987). Off, nothing is forced.
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

/// The words a question's tolerated word ([`one_short`]) is never: the
/// one that makes it a reminders question, and "my" ("what are
/// reminders?" asks what they are).
const KEPT: &[&str] = &["reminders", "my"];

/// Words that change the ask. A line one word short ([`one_short`]) that
/// holds any of them anywhere is asked as before: a set, a cancel, a timer,
/// a need, a "not", a second clause.
const DENY: &[&str] = &[
    "remind", "reminder", "set", "add", "make", "cancel", "delete", "remove", "clear", "stop",
    "snooze", "move", "change", "edit", "timer", "timers", "alarm", "alarms", "like", "love",
    "hate", "need", "want", "should", "would", "could", "must", "and", "then", "also", "but", "or",
    "not", "don't", "no", "never", "after", "before", "instead", "every",
];

/// Whether a line is only a plain question about the person's reminders:
/// exactly one question ([`ASKS`]) and no word but those [`AROUND`]; or,
/// with no word of [`DENY`] in it, the same with one word of its question
/// lost or misheard ([`one_short`], #7987).
pub fn asks_for_reminders(said: &str) -> bool {
    let words = lower_words(said);
    reads(&words, false, false)
        || (!words.iter().any(|word| DENY.contains(&word.as_str())) && reads(&words, false, true))
}

/// Whether `words` read as [`asks_for_reminders`] says, `asked` once a
/// question is past and `spare` while one word short may still be taken.
fn reads(words: &[String], asked: bool, spare: bool) -> bool {
    let Some(first) = words.first() else {
        return asked;
    };
    if let Some(ask) = ASKS.iter().find(|ask| starts(words, ask)) {
        return !asked && reads(&words[ask.len()..], true, spare);
    }
    if AROUND.contains(&first.as_str()) {
        return reads(&words[1..], asked, spare);
    }
    spare && !asked && one_short(words).any(|taken| reads(&words[taken..], true, false))
}

/// Whether `words` open with `ask`.
fn starts(words: &[String], ask: &[&str]) -> bool {
    words.len() >= ask.len() && ask.iter().zip(words).all(|(word, said)| said == word)
}

/// How many of `words`' first words make a question of 4 words or more
/// heard one word short: one of its words lost ("what reminders do have")
/// or heard as another ("what reminders do I halve"), the word never one
/// of [`KEPT`]. On lamp-52e6 at the gate's Mac volume 45 (20-21 Nov) the
/// 7-word "Hey Lamp, what reminders do I have?" was heard whole in about
/// 17 of 60 plays, mostly as 6 words, and a 6-word line was not forced.
fn one_short(words: &[String]) -> impl Iterator<Item = usize> + '_ {
    ASKS.iter()
        .filter(|ask| ask.len() >= 4)
        .flat_map(move |ask| {
            (0..ask.len())
                .filter(|&at| !KEPT.contains(&ask[at]))
                .flat_map(move |at| {
                    let lost = [&ask[..at], &ask[at + 1..]].concat();
                    let lost = starts(words, &lost).then_some(lost.len());
                    let misheard = (words.len() >= ask.len()
                        && ask
                            .iter()
                            .zip(words)
                            .enumerate()
                            .all(|(next, (word, said))| next == at || said == word))
                    .then_some(ask.len());
                    lost.into_iter().chain(misheard)
                })
        })
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

    /// Faq's description names how reminders work as its own ask, not only
    /// their own reminders as not its: on lamp-52e6 in kit-8206c (#8206's
    /// ROW, 22 Nov) "how do reminders work?" got `calls=none` in 2 of 4,
    /// round 1 `forced=none`, with faq's description naming reminders only
    /// in its "not for" clause.
    #[test]
    fn faq_says_how_reminders_work_is_its_own() {
        let faq = include_str!("../../../../skills/faq/SKILL.md");
        let description = faq
            .lines()
            .find_map(|line| line.strip_prefix("description: "))
            .expect("no description");
        assert!(description.contains("\"how do reminders work?\""), "{description}");
    }
}
