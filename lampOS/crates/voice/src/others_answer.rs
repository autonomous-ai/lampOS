//! Someone else's answer, left alone on the second route (#2136, the first
//! builder at 11:29 and 11:37 on 20 October), off unless
//! `LAMP_OTHERS_ANSWER=on`.
//!
//! On lamp-4ace in the 11:10 played test, A asked for a story by name.
//! During it B said "Daniel, are you coming to the meeting at two?", over
//! the story, and was never admitted, so the rule had no words of it. A then
//! answered B, "Yes, give me five minutes.", over the story too. That was
//! admitted, let out only as the conversation going on (A had just called
//! the lamp), and answered "Five minutes, counting.", with no timer set.
//! Gemini Live, hearing both voices, leaves such a line alone.
//!
//! With the switch, the second route's rule holds a line that:
//! - begins as a yes ([`begins_as_an_answer`]: "yes", "yeah", "yep", "yup"),
//!   with no question in it. A "no" said over the lamp's answer is far more
//!   often to the lamp, a correction ("No, the fox one."), and a question
//!   ("Yes, but who was he?") follows the lamp's answer (the reviewer's P2
//!   on #2175);
//! - began over the lamp's own answer (its Start's `over_the_lamps_voice`);
//! - answers nothing the lamp asked, and is let out only as the
//!   conversation going on (no name, no call, nobody turned to the lamp);
//!   `Attention::played_on_the_conversation_alone` already means both.
//!
//! A reply to nothing the lamp asked, said over its answer, answers someone
//! else. When the rule did hear the question to the other person, it
//! already holds the reply (`Attention::began_at`'s reply to a person called
//! by name); this is for when it heard only the reply.

/// Where the coordinator reads the switch.
pub const ENVIRONMENT: &str = "LAMP_OTHERS_ANSWER";
/// The provider worker's flag for it, right after `--messages`.
pub const FLAG: &str = "--others-answer";

/// Why the rule holds such a line, as its journal says it after "held
/// back: ".
pub const HELD: &str =
    "it answers nothing the lamp asked and began over the lamp's answer: someone else's answer";

/// Words that begin a yes to someone's question.
const ANSWER_WORDS: [&str; 4] = ["yes", "yeah", "yep", "yup"];

/// Whether `heard` begins as a yes to someone's question: its first word is
/// one of [`ANSWER_WORDS`], in any case, with or without a comma after it,
/// and it asks nothing itself (no `?`).
pub fn begins_as_an_answer(heard: &str) -> bool {
    !heard.contains('?')
        && heard
            .split(|c: char| !(c.is_alphanumeric() || c == '\''))
            .find(|word| !word.is_empty())
            .is_some_and(|word| ANSWER_WORDS.contains(&word.to_lowercase().as_str()))
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
        assert_eq!(take(&flags[1..]), (false, &flags[1..]));
    }

    #[test]
    fn a_line_begins_as_an_answer_on_its_first_word_only() {
        for answer in ["Yes, give me five minutes.", "yeah sure", "  Yep.", "Yup"] {
            assert!(begins_as_an_answer(answer), "{answer}");
        }
        // A "no" over the lamp's answer is a correction to it, and a
        // question follows its answer (the reviewer's P2 on #2175).
        for not in [
            "No, the fox one.",
            "Nope, the other one",
            "No, louder",
            "Yes, but who was he?",
            "Give me five minutes, yes.",
            "Yesterday was long.",
            "Nobody told me.",
            "What time is it?",
            "",
        ] {
            assert!(!begins_as_an_answer(not), "{not}");
        }
    }
}
