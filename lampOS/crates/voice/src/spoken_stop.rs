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

/// Where the coordinator reads the switch.
pub const ENVIRONMENT: &str = "LAMP_SPOKEN_STOP_ENDS";
/// The provider worker's flag for it, right after `--cut-in-told`.
pub const FLAG: &str = "--spoken-stop-ends";
/// What the lamp says to a stop said over its answer.
pub const LINE: &str = "Okay, I'll stop there.";
/// The most words a stop request has: "Okay, stop telling the story." is
/// six; a longer line asks for more than a stop and goes to the model.
const MOST_WORDS: usize = 6;

/// Whether `text` asks the lamp to stop, and nothing more: a stop word or
/// pair ([`crate::stop_words::stop_phrase`]: "stop", "enough", "hold on",
/// never after "don't"), or "never mind", in a line of at most
/// [`MOST_WORDS`] words. "Stop them." (speech to text's "Stop"), "Stop it.",
/// "Stop telling the story.", "That's enough." and "Okay, enough." all are.
pub fn is_stop_request(text: &str) -> bool {
    let words = crate::not_speech::plain_words(text);
    if words.is_empty() || words.len() > MOST_WORDS {
        return false;
    }
    crate::stop_words::stop_phrase(text)
        || words
            .windows(2)
            .any(|pair| pair[0] == "never" && pair[1] == "mind")
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
            "Lamp, hold on.",
            "Never mind.",
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
        ] {
            assert!(!is_stop_request(not), "{not}");
        }
    }
}
