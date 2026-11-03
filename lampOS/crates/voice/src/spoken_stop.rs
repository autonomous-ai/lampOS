//! A stop said over the lamp's answer ends it with one fixed line, never a
//! question to the model (#3985), off unless `LAMP_SPOKEN_STOP_ENDS=on`.
//!
//! On lamp-29e7 (stable 8001, 28 Oct 09:05) a person cut a long story with
//! "Stop" (heard as "Stop them."): the cut-in committed and the line went to
//! the model as a new request, which answered "Stop who?". Then "Stop
//! telling story." was answered "I wasn't telling a story": the model read
//! its last answer, not the story it had been stopped in. A short line that
//! asks the lamp to stop ([`is_stop_request`]), said over its own answer,
//! now has its answer here: [`LINE`], said in the route's voice and kept in
//! the thread as the lamp's, so the next turn sees the story, the stop and
//! that the lamp stopped. The second route only.
//!
//! A stop said just after the answer ended, within [`JUST_AFTER`] of its
//! last sound, gets the same line (lamp-29e7 on 30 Oct, Sixth's item 6 on
//! #4710: "Stop" said as the story's last words played out went to the
//! model, or was held as a word of the room's): a stop always gets the
//! same short line.

/// Where the coordinator reads the switch.
pub const ENVIRONMENT: &str = "LAMP_SPOKEN_STOP_ENDS";
/// The provider worker's flag for it, right after `--cut-in-told`.
pub const FLAG: &str = "--spoken-stop-ends";
/// What the lamp says to a stop said over its answer.
pub const LINE: &str = "Okay, I'll stop there.";
/// How long after the lamp's answer ended a stop said is still said to
/// it: the person heard the answer end as they spoke.
pub const JUST_AFTER: std::time::Duration = std::time::Duration::from_secs(3);
/// The most words a stop request has: "Okay, stop telling the story." is
/// six; a longer line asks for more than a stop and goes to the model.
const MOST_WORDS: usize = 6;

/// Whether `text` asks the lamp to stop, and nothing more: "stop" or
/// "enough" (never after "don't", [`crate::stop_words::stop_phrase`]) or
/// "never mind", in a line of at most [`MOST_WORDS`] words that asks
/// nothing. "Stop them." (speech to text's "Stop"), "Stop it.", "Stop
/// telling the story.", "That's enough." and "Okay, enough." are; "Wait,
/// what?", "Hold on, who?" and "Quiet, what was that?" are questions for the
/// model, as are the other stop words alone (Nine's and Eighth's P2s on
/// #4004).
pub fn is_stop_request(text: &str) -> bool {
    let words = crate::not_speech::plain_words(text);
    if words.is_empty() || words.len() > MOST_WORDS || asks(text, &words) {
        return false;
    }
    let mind = words
        .windows(2)
        .any(|pair| pair[0] == "never" && pair[1] == "mind");
    mind || (words
        .iter()
        .any(|word| matches!(word.as_str(), "stop" | "enough"))
        && crate::stop_words::stop_phrase(text))
}

/// Words that open or carry a question.
const ASKING: &[&str] = &[
    "what", "who", "whom", "whose", "why", "how", "where", "when", "which", "could", "would",
    "did", "does", "are", "was", "were", "should",
];

/// Whether the line asks something: a question mark, or a question word.
fn asks(text: &str, words: &[String]) -> bool {
    text.contains('?') || words.iter().any(|word| ASKING.contains(&word.as_str()))
}

/// Whether every word of a stop is, in order, among the lamp's own answer
/// so far: a story's "Stop it!" heard back over its voice, too short for
/// the own-echo rule (`own_echo::HELD_LEAST`), is never answered as a stop
/// (the Reviewer-lead's P2 on #4004).
pub fn could_be_echo(heard: &str, said: &str) -> bool {
    let heard = crate::not_speech::plain_words(heard);
    let said = crate::not_speech::plain_words(said);
    // One word ("Stop.", "Enough.") is the person's: the answer may hold it
    // anywhere ("a bus stop", "big enough"), and a lone word heard back is
    // what a real stop sounds like too (the Reviewer-lead on #4004).
    heard.len() >= 2 && said.windows(heard.len()).any(|run| run == heard.as_slice())
}

/// Whether a line begun at `began_us` began within [`JUST_AFTER`] of the
/// lamp's answer's last sound, played by `until_us` (both on the monotonic
/// clock): any time before that end too, as it played. Never with no answer
/// played yet (`until_us` 0) or no start known.
pub fn just_after(began_us: u64, until_us: u64) -> bool {
    began_us != 0
        && until_us != 0
        && began_us <= until_us.saturating_add(JUST_AFTER.as_micros() as u64)
}

/// The worker's flags for the switch, from the coordinator's environment.
pub fn arguments_from_environment() -> Vec<String> {
    arguments(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("on") {
        vec![FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// Take a leading [`FLAG`] off a worker's flags.
pub fn take(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == FLAG => (true, rest),
        _ => (false, flags),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_is_on_only_for_on_and_its_flag_is_taken_first() {
        assert_eq!(arguments(Some("on")), [FLAG]);
        for off in [None, Some(""), Some("off"), Some("1"), Some("ON")] {
            assert!(arguments(off).is_empty(), "{off:?}");
        }
        let flags = [FLAG.to_owned(), "x".to_owned()];
        assert_eq!(take(&flags), (true, &flags[1..]));
        let other = ["x".to_owned()];
        assert_eq!(take(&other), (false, &other[..]));
    }

    #[test]
    fn a_stop_just_after_the_answer_is_within_three_seconds_of_its_end() {
        let end = 50_000_000;
        assert!(just_after(end - 2_000_000, end), "while it played");
        assert!(just_after(end + 1_000_000, end));
        assert!(just_after(end + 3_000_000, end));
        assert!(!just_after(end + 3_000_001, end));
        assert!(!just_after(end + 1_000_000, 0), "no answer played");
        assert!(!just_after(0, end), "no start known");
    }

    #[test]
    fn a_stop_with_the_lamps_name_is_one_and_a_long_line_with_stop_is_not() {
        for stop in [
            "Stop Lamp.",
            "Stop, Lamp!",
            "Lamp, stop.",
            "Lamp stop",
            "Hey Lamp, stop.",
            "Okay Lamp, stop it.",
        ] {
            assert!(is_stop_request(stop), "{stop}");
        }
        for not in [
            "Don't stop.",
            "Lamp, don't stop.",
            "I had to stop at the shop on the way home today.",
            "We stopped at the bus stop and then walked all the way home.",
        ] {
            assert!(!is_stop_request(not), "{not}");
        }
    }

    #[test]
    fn a_stop_the_lamp_itself_just_said_could_be_its_echo() {
        assert!(could_be_echo(
            "Stop it!",
            "The ogre roared, stop it! and the frogs went quiet."
        ));
        assert!(!could_be_echo(
            "Stop it.",
            "The ogre roared and the frogs went quiet."
        ));
        assert!(!could_be_echo("", "Stop it."));
        // One word is the person's, wherever the answer has it.
        assert!(!could_be_echo("Stop.", "We waited at the bus stop."));
        assert!(!could_be_echo("Enough.", "It was big enough for both."));
    }

    #[test]
    fn a_plain_ask_to_stop_is_one_in_its_own_words() {
        // #3985's five, and 29e7's two.
        for stop in [
            "Stop.",
            "Stop it.",
            "Stop telling the story.",
            "That's enough.",
            "Okay, enough.",
            "Stop them.",
            "Stop telling story.",
            "Okay, okay, stop telling the story.",
            "Never mind.",
            "That is enough.",
        ] {
            assert!(is_stop_request(stop), "{stop}");
        }
    }

    #[test]
    fn a_line_asking_more_or_not_to_stop_goes_to_the_model() {
        for not in [
            "",
            "Don't stop.",
            "No, never stop, go on.",
            "Tell me more about the ogre.",
            "Stop, and tell me what the weather is like tomorrow.",
            "Okay, what happened to the ogre in the end?",
            "Wait, what did you say?",
            "Wait, what?",
            "Hold on, who?",
            "Quiet, what was that?",
            "Wait.",
            "Pause.",
        ] {
            assert!(!is_stop_request(not), "{not}");
        }
    }
}
