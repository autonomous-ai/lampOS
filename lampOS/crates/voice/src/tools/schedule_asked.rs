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
/// The plain asks, at a sentence's start.
const ASKS: &[&[&str]] = &[
    &["remind", "me"],
    &["set", "a", "timer"],
    &["set", "a", "reminder"],
    &["set", "an", "alarm"],
    &["start", "a", "timer"],
    &["timer", "for"],
    &["wake", "me"],
];
/// Units of a duration, whole words.
const UNITS: &[&str] = &[
    "second", "seconds", "minute", "minutes", "min", "mins", "hour", "hours", "half",
];

/// Which function a line that plainly asks for a timer or a reminder
/// ([`ASKS`]) at a sentence's start must call: [`TIMER`] for a duration
/// from now ("in three minutes", "for ten minutes", "a timer for an hour"),
/// [`REMINDER`] for a clock time ("at nine", "at 6:30", "tomorrow at …",
/// "o'clock"). None when the line says neither or both, asks what is set
/// ("do I have any reminders?"), is a question that isn't an ask ("Will you
/// remind me?"), or takes the ask back anywhere
/// ([`crate::remember::asked::withdrawn`]).
pub fn asks_to_set(said: &str) -> Option<&'static str> {
    if crate::remember::asked::withdrawn(said) {
        return None;
    }
    let said = said.replace('\u{2019}', "'");
    let asked = said.split_inclusive(['.', '!', '?', ';']).any(|sentence| {
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
        let a_question = sentence.trim_end().ends_with('?') && !asking;
        !a_question
            && ASKS.iter().any(|phrase| {
                words.len() > at + phrase.len()
                    && words[at..at + phrase.len()]
                        .iter()
                        .zip(phrase.iter())
                        .all(|(word, ask)| word == ask)
            })
    });
    if !asked {
        return None;
    }
    let words = lower_words(&said);
    let duration = words.windows(2).any(|pair| {
        matches!(pair[0].as_str(), "in" | "for")
            && (pair[1].chars().all(|c| c.is_ascii_digit()) || is_a_number(&pair[1]))
    }) && words.iter().any(|word| UNITS.contains(&word.as_str()));
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
}
