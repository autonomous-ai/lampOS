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
/// Units of a duration, whole words.
const UNITS: &[&str] = &[
    "second", "seconds", "minute", "minutes", "min", "mins", "hour", "hours", "half",
];

/// A word that, anywhere in the line, may take the ask back or put it in
/// doubt: "Actually no, don't.", "No wait, don't.", "don't bother", "Hold
/// on", "Can you cancel the timer?" (reviewer-three's and reviewer-two's P2s
/// on `52fbf23c4`). With one, nothing is forced: a missed force only means
/// the model answers as it would without the switch, while a wrong one sets
/// what nobody asked for. Words, not phrases, so no form of them slips past.
const DOUBT: &[&str] = &[
    "don't",
    "dont",
    "not",
    "no",
    "nope",
    "wait",
    "hold",
    "cancel",
    "stop",
    "never",
    "nevermind",
    "scratch",
    "forget",
    "bother",
    "actually",
    "instead",
    "rather",
    "unless",
    "maybe",
    "whichever",
    "either",
    "or",
];

/// Words that may stand between the ask and its time ("remind me tomorrow
/// at six", "wake me up at seven").
const BETWEEN: &[&str] = &["up", "tomorrow", "today", "tonight", "please", "again"];

/// What may follow a clock time for it to stand as one ("at nine.", "at 6:30
/// to …", "at seven tomorrow"), never a noun ("at 180 degrees", "at 3
/// pharmacies").
const AFTER_A_TIME: &[&str] = &[
    "to", "and", "am", "pm", "a", "p", "o'clock", "tonight", "tomorrow", "today", "please",
    "sharp", "in", "this", "on",
];

/// Which function a line that plainly asks for a timer or a reminder
/// ([`ASKS`]) at a sentence's start must call, from the words right after
/// the ask (reviewer-two's and reviewer-three's P2s on `50e0a193b` and
/// `52fbf23c4`): [`TIMER`] for a length from now ("remind me in three
/// minutes …", "set a timer for ten minutes"), [`REMINDER`] for a clock time
/// ("remind me at nine …", "remind me tomorrow at 6:30 …", "wake me up at
/// seven", "at noon"). A time later in the line ("remind me to tell her I got
/// home at nine", "it needs 20 minutes at 180 degrees") forces nothing, nor
/// a line with a word of [`DOUBT`] anywhere, nor one that is a question that
/// isn't an ask, nor a keep taken back
/// ([`crate::remember::asked::withdrawn`]). It errs toward not forcing.
pub fn asks_to_set(said: &str) -> Option<&'static str> {
    let said = said.replace('\u{2019}', "'");
    if crate::remember::asked::withdrawn(&said)
        || lower_words(&said)
            .iter()
            .any(|word| DOUBT.contains(&word.as_str()))
    {
        return None;
    }
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
                    let mut from = at + phrase.len() - usize::from(phrase.last() == Some(&"for"));
                    while words
                        .get(from)
                        .is_some_and(|word| BETWEEN.contains(&word.as_str()))
                    {
                        from += 1;
                    }
                    (words[from..].to_vec(), *timer)
                })
            })
        })?;
    let number = |word: &String| word.chars().all(|c| c.is_ascii_digit()) || is_a_number(word);
    // A length right after the ask: "in"/"for" (a timer's only), a number,
    // and its unit within the next two words ("in half an hour").
    let duration = words
        .first()
        .is_some_and(|lead| lead == "in" || (timer && lead == "for"))
        && words.get(1).is_some_and(number)
        && words[2..words.len().min(4)]
            .iter()
            .any(|word| UNITS.contains(&word.as_str()));
    // A clock time right after the ask that stands as one: "at" and a time
    // ("9", "6 30" as "6:30" is read, "nine"), then the line's end or a word
    // of [`AFTER_A_TIME`]; or "at noon", "at midnight".
    let clock = words.first().is_some_and(|lead| lead == "at")
        && words
            .get(1)
            .is_some_and(|word| word == "noon" || word == "midnight" || starts_a_time(word))
        && {
            let mut next = 2;
            if words
                .get(1)
                .is_some_and(|word| word.chars().all(|c| c.is_ascii_digit()))
                && words
                    .get(2)
                    .is_some_and(|word| word.len() == 2 && word.chars().all(|c| c.is_ascii_digit()))
            {
                next = 3;
            }
            words
                .get(next)
                .is_none_or(|word| AFTER_A_TIME.contains(&word.as_str()))
        };
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
            // Reviewer-three's and reviewer-two's on `52fbf23c4`.
            "Remind me in five minutes to call Mom. Actually no, don't.",
            "Remind me in five minutes to call Mom. No wait, don't.",
            "Set a timer for ten minutes. Actually, don't bother.",
            "Remind me at nine to call Mom. Hold on, don't do that.",
            "Remind me to check the oven, it needs 20 minutes at 180 degrees.",
            "Remind me to take the pills I got at 3 pharmacies.",
            "Set a timer for ten minutes. Can you cancel the timer?",
            "Remind me to tell a colleague I got home at nine.",
            "Remind me to call Mom at nine.",
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
            "Set a timer for ten minutes. Can you cancel the timer?",
            "Remind me to tell a colleague I got home at nine.",
            "Remind me in five minutes to call Mom. Actually no, don't.",
            "Remind me to check the oven, it needs 20 minutes at 180 degrees.",
        ] {
            assert_eq!(choice(said), None, "{said}");
        }
        assert_eq!(choice("Wake me up at seven."), named(REMINDER));
        assert_eq!(
            choice("Lamp, remind me tomorrow at 6:30 to leave early."),
            named(REMINDER)
        );
    }
}
