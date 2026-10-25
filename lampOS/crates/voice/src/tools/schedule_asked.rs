//! A line that plainly asks for a timer or a reminder calls the schedule
//! (`LAMP_SCHEDULE_ASKED`). On lamp-4ace on 25 Oct (20:07, the ship build
//! 6819 on the product unit, a played line), "Lamp, remind me in three
//! minutes to call my sister." was answered "Three minutes, call your
//! sister." with no call at all, so nothing was set and nothing was said
//! three minutes later; the two asks before it had called `set_timer`.
//! With the switch, such a line's first chat round must call the function
//! that sets it (`tool_choice`): a duration from now `set_timer`, a clock
//! time `set_reminder`, as their descriptions ask. A line whose kind isn't
//! plain, a question about what is set, a cancel, and a line that takes the
//! ask back are asked as before. Off, nothing is forced.
use crate::known_faces::lower_words;

/// Only the exact value `on`; off unless set.
pub const ENVIRONMENT: &str = "LAMP_SCHEDULE_ASKED";

/// Whether the switch is on.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// The function a duration from now is set with.
pub const TIMER: &str = "set_timer";
/// The function a clock time is set with.
pub const REMINDER: &str = "set_reminder";

/// What may stand before the ask at a sentence's start, past a greeting.
const BEFORE: &[&str] = &[
    "hey", "hi", "hello", "ok", "okay", "lamp", "so", "and", "oh",
];
/// "Can you …", "Could you …": an ask, even ending in a question mark.
const ASKING: &[&str] = &["can", "could", "would", "will"];
/// The plain asks, at a sentence's start, and whether each is a timer's,
/// which may take its length with "for" ("a timer for ten minutes"); a
/// reminder's takes it only with "in", so "remind me to exercise for ten
/// minutes" says what to do, not when (reviewer-two's P2 on `50e0a193b`).
const ASKS: &[(&[&str], bool)] = &[
    (&["remind", "me"], false),
    (&["set", "a", "timer"], true),
    (&["set", "a", "reminder"], false),
    (&["set", "an", "alarm"], false),
    (&["start", "a", "timer"], true),
    (&["timer", "for"], true),
    (&["wake", "me"], false),
];
/// A timer or reminder taken back anywhere in the line, clause by clause,
/// beside [`crate::remember::asked::withdrawn`]'s ("Actually, don't remind
/// me", "do not set the timer", "cancel the timer": reviewer-two's P2 on
/// `50e0a193b`). After a pronoun the words are the line's content ("…that
/// I don't set the table").
const WITHDRAWN: &[&[&str]] = &[
    &["don't", "remind"],
    &["dont", "remind"],
    &["do", "not", "remind"],
    &["don't", "set"],
    &["dont", "set"],
    &["do", "not", "set"],
    &["don't", "wake"],
    &["do", "not", "wake"],
    &["don't", "start"],
    &["do", "not", "start"],
    &["cancel", "the"],
    &["cancel", "it"],
    &["cancel", "that"],
    &["cancel", "this"],
    &["no", "timer"],
    &["no", "reminder"],
    &["no", "alarm"],
    &["forget", "the", "timer"],
    &["forget", "the", "reminder"],
];
/// Before a withdrawal's words, what makes them the line's content.
const ABOUT_SOMEONE: &[&str] = &["i", "we", "they", "he", "she", "you", "it"];

/// Whether the line takes a timer or a reminder back anywhere
/// ([`WITHDRAWN`]), or a keep ([`crate::remember::asked::withdrawn`]).
fn withdrawn(said: &str) -> bool {
    crate::remember::asked::withdrawn(said)
        || said
            .replace('\u{2019}', "'")
            .split(['.', '!', '?', ';', ',', ':', '\u{2014}', '\u{2013}'])
            .any(|clause| {
                let words = lower_words(clause);
                (0..words.len()).any(|at| {
                    WITHDRAWN.iter().any(|phrase| {
                        words.len() >= at + phrase.len()
                            && words[at..at + phrase.len()]
                                .iter()
                                .zip(phrase.iter())
                                .all(|(word, withdrawn)| word == withdrawn)
                            && !at.checked_sub(1).is_some_and(|before| {
                                ABOUT_SOMEONE.contains(&words[before].as_str())
                            })
                    })
                })
            })
}
/// Units of a duration, whole words.
const UNITS: &[&str] = &[
    "second", "seconds", "minute", "minutes", "min", "mins", "hour", "hours", "half",
];

/// Which function a line that plainly asks for a timer or a reminder
/// ([`ASKS`]) at a sentence's start must call, from that ask's own sentence
/// and only the words after the ask (reviewer-two's P2 on `50e0a193b`: a
/// time in another sentence, "My train got in at nine.", is not the
/// reminder's): [`TIMER`] for a duration from now ("in three minutes", and
/// for a timer "for ten minutes"), [`REMINDER`] for a clock time ("at
/// nine", "at 6:30", "o'clock", "noon"). None when that part says neither
/// or both, the line asks what is set ("do I have any reminders?"), is a
/// question that isn't an ask ("Will you remind me?"), or takes the ask
/// back anywhere ([`withdrawn`]).
pub fn asks_to_set(said: &str) -> Option<&'static str> {
    if withdrawn(said) {
        return None;
    }
    let said = said.replace('\u{2019}', "'");
    let (words, timer) = said
        .split_inclusive(['.', '!', '?', ';'])
        .find_map(|sentence| {
            let words = lower_words(sentence);
            let mut at = 0;
            while words
                .get(at)
                .is_some_and(|word| BEFORE.contains(&word.as_str()))
            {
                at += 1;
            }
            if words.get(at).is_some_and(|word| word == "please") {
                at += 1;
            }
            let asking = words
                .get(at)
                .is_some_and(|word| ASKING.contains(&word.as_str()))
                && words.get(at + 1).is_some_and(|word| word == "you");
            if asking {
                at += 2;
                if words.get(at).is_some_and(|word| word == "please") {
                    at += 1;
                }
            }
            if sentence.trim_end().ends_with('?') && !asking {
                return None;
            }
            ASKS.iter().find_map(|(phrase, timer)| {
                (words.len() > at + phrase.len()
                    && words[at..at + phrase.len()]
                        .iter()
                        .zip(phrase.iter())
                        .all(|(word, ask)| word == ask))
                .then(|| {
                    // "timer for" ends with its own "for": the length follows it.
                    let from = at + phrase.len() - usize::from(phrase.last() == Some(&"for"));
                    (words[from..].to_vec(), *timer)
                })
            })
        })?;
    // A length: "in"/"for" (a timer's only), a number, and its unit within
    // the next two words ("in half an hour", "in ten minutes").
    let duration = (0..words.len()).any(|at| {
        let lead = words[at].as_str();
        (lead == "in" || (timer && lead == "for"))
            && words
                .get(at + 1)
                .is_some_and(|word| word.chars().all(|c| c.is_ascii_digit()) || is_a_number(word))
            && words[at + 2..(at + 4).min(words.len())]
                .iter()
                .any(|word| UNITS.contains(&word.as_str()))
    });
    let clock = words
        .windows(2)
        .any(|pair| pair[0] == "at" && starts_a_time(&pair[1]))
        || words
            .iter()
            .any(|word| word == "o'clock" || word == "noon" || word == "midnight");
    match (duration, clock) {
        (true, false) => Some(TIMER),
        (false, true) => Some(REMINDER),
        _ => None,
    }
}

/// A number said as a word, as speech to text often writes small ones.
fn is_a_number(word: &str) -> bool {
    matches!(
        word,
        "a" | "an"
            | "one"
            | "two"
            | "three"
            | "four"
            | "five"
            | "six"
            | "seven"
            | "eight"
            | "nine"
            | "ten"
            | "fifteen"
            | "twenty"
            | "thirty"
            | "forty"
            | "fifty"
            | "sixty"
            | "half"
    )
}

/// The first word of a clock time: "9", "6:30" (split as "6", "30"),
/// "nine", "noon".
fn starts_a_time(word: &str) -> bool {
    word.chars().next().is_some_and(|c| c.is_ascii_digit())
        || (is_a_number(word) && !matches!(word, "a" | "an" | "half"))
        || matches!(word, "eleven" | "twelve" | "noon" | "midnight")
}

/// The force the second route's chat is given with the switch on
/// (`second_provider::net::Chat::forcing`): a request whose last message is
/// a person's line that plainly asks for a timer or a reminder must call
/// the function that sets it.
pub(crate) fn force() -> crate::second_provider::net::Force {
    std::sync::Arc::new(|messages: &[crate::second_provider::net::Message]| {
        let last = messages.last().filter(|message| message.role == "user")?;
        asks_to_set(last.words()?)
            .map(|name| crate::second_provider::net::Forced { name, only: None })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The plain asks, as speech to text writes them, and which function
    /// each must call; against the questions, cancels, unclear kinds and
    /// withdrawals that are asked as before.
    #[test]
    fn a_plain_ask_for_a_timer_or_a_reminder_is_told_apart() {
        for (said, name) in [
            ("Lamp, remind me in three minutes to call my sister.", TIMER),
            ("Lamp, remind me in one minute to check the oven.", TIMER),
            ("Lamp, set a timer for two minutes.", TIMER),
            ("Set a timer for 10 minutes.", TIMER),
            ("Can you remind me in an hour to stretch?", TIMER),
            ("Remind me at nine to take my pills.", REMINDER),
            ("Lamp, remind me tomorrow at 6:30 to leave early.", REMINDER),
            ("Wake me up at seven.", REMINDER),
            ("Set a reminder at noon to eat lunch.", REMINDER),
            // The time bound to the ask, whatever else the line says.
            (
                "Remind me in ten minutes to exercise for twenty minutes.",
                TIMER,
            ),
            (
                "My train got in at nine. Remind me in an hour to call home.",
                TIMER,
            ),
            ("Remind me in half an hour to check the oven.", TIMER),
        ] {
            assert_eq!(asks_to_set(said), Some(name), "{said}");
        }
        for said in [
            "Lamp, do I have any reminders?",
            "Will you remind me?",
            "How long is left on the timer?",
            "Cancel the timer.",
            "Remind me to buy milk.",
            "Remind me in a bit.",
            "Remind me at nine, in ten minutes, whichever comes first.",
            "Remind me in five minutes to call Mum. Actually, never mind.",
            "Don't remind me at nine.",
            "She said remind me in ten minutes.",
            // Reviewer-two's six on `50e0a193b`: a schedule withdrawal after
            // the ask, and a time that isn't the ask's.
            "Lamp, remind me in three minutes to stretch. Actually, don't remind me.",
            "Remind me in three minutes to stretch. Actually, do not set the timer.",
            "Remind me in three minutes to stretch. Actually, cancel the timer.",
            "Remind me to buy milk. My train got in at nine.",
            "Remind me to buy milk. I waited for ten minutes.",
            "Remind me to exercise for ten minutes.",
            "Remind me at nine to call home, do not set it at all.",
        ] {
            assert_eq!(asks_to_set(said), None, "{said}");
        }
    }

    /// The force names the function for an ask alone, never for a round
    /// that ends with a call's result.
    #[test]
    fn the_force_names_the_setting_function_for_an_ask_and_nothing_else() {
        use crate::second_provider::net::Message;
        let force = force();
        let ask = [Message::user(
            "Lamp, remind me in three minutes to call my sister.",
        )];
        assert_eq!(force(&ask).map(|forced| forced.name), Some(TIMER));
        assert_eq!(force(&[Message::user("What time is it?")]), None);
        let answered = [
            Message::user("Lamp, remind me in three minutes to call my sister."),
            Message::tool("c", "{\"ok\":true}".to_owned()),
        ];
        assert_eq!(force(&answered), None);
    }

    /// The same P2s on the wire: a plain ask's first round carries a
    /// `tool_choice` for the function that sets it; a withdrawal after the
    /// ask, or a time that isn't the ask's, carries none.
    #[test]
    fn a_taken_back_or_unbound_ask_sends_no_tool_choice() {
        use crate::second_provider::net::{Chat, Declare, Message};
        let tool = |name: &str| lamp_gemini::Tool {
            name: name.into(),
            description: "Set it.".into(),
            parameters: None,
            silent: false,
        };
        let declare: Declare = std::sync::Arc::new(move || vec![tool(TIMER), tool(REMINDER)]);
        let chat = Chat::deepseek("deepseek-chat", "k", declare, false).unwrap();
        chat.forcing(force());
        let choice = |said: &str| {
            chat.wire(&[Message::system("You are a lamp."), Message::user(said)])
                .2
                .get("tool_choice")
                .cloned()
        };
        let named =
            |name: &str| Some(serde_json::json!({"type": "function", "function": {"name": name}}));
        assert_eq!(
            choice("Lamp, remind me in three minutes to call my sister."),
            named(TIMER)
        );
        assert_eq!(
            choice("Remind me at nine to take my pills."),
            named(REMINDER)
        );
        for said in [
            "Lamp, remind me in three minutes to stretch. Actually, don't remind me.",
            "Remind me in three minutes to stretch. Actually, cancel the timer.",
            "Remind me to buy milk. My train got in at nine.",
            "Remind me to exercise for ten minutes.",
        ] {
            assert_eq!(choice(said), None, "{said}");
        }
    }
}
