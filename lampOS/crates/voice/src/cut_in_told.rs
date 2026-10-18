//! A spoken cut-in, told to the model (#828, the first builder at 03:15 on
//! 19 October), off unless `LAMP_CUT_IN_TOLD=on`.
//!
//! A touch or the stop button that stops the lamp tells the model that
//! what it was saying is over (`LAMP_STOP_TOLD`,
//! `crate::provider_worker::STOPPED_NOTE`). A person who cuts in by speaking
//! told it nothing. On lamp-4ace on 19 October at 02:59, "Okay, okay, that
//! is enough." was heard right, committed over the lamp's story, and the
//! answer was the story again from where it was cut: with the story cut
//! mid-sentence in its context, "Okay, okay" read as "go on".
//!
//! With the switch, the request that commits a spoken cut-in over the
//! lamp's own audible answer (`ProviderInput::Start`'s
//! `over_the_lamps_voice`, and not the first after a stop, which is told
//! that instead) is told [`NOTE`] beside what is heard, in the form of the
//! lamp's other notes. A request sends only its newest note, so it goes
//! with the others due on it, the name heard among them (`add_note`).
//! The Live route only.

/// Where the coordinator reads the switch.
pub const ENVIRONMENT: &str = "LAMP_CUT_IN_TOLD";
/// The provider worker's flag for it, right after `--going-on-asks`.
pub const FLAG: &str = "--cut-in-told";

/// What the model is told beside a request that cut in over its answer.
pub const NOTE: &str = "(For you to know, not to answer: they spoke while you were speaking, and you stopped there. Answer what they said; go back to what you were saying only if they ask you to.)";

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
}
