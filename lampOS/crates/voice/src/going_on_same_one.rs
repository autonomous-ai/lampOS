//! A conversation going on holds only for the one the lamp answered, on the
//! second route in a busy room (#2191, the first builder at 15:25 on #1722),
//! off unless `LAMP_GOING_ON_SAME_ONE=on`.
//!
//! On lamp-52e6 on 20 October at 15:2x, in a busy room, after the lamp
//! answered one named line, four to five lines of two others talking about
//! work were answered, each "a conversation with the lamp is going on": the
//! busy room's window lets out any line within 15 s of the lamp's words.
//!
//! With the switch, in a busy room, a line that would be let out only as the
//! conversation going on (no name, no call, nobody turned to the lamp, not
//! said close to it: `Attention::played_on_the_conversation_alone`) is held
//! unless it answers the lamp's own question, or the same-voice check
//! (`LAMP_SAME_VOICE_MODEL`) found it in the voice of the conversation the
//! lamp answered. Without that model only the reply to the lamp's question
//! goes on. A line ending in `?` is no exception: office talk asks too. A
//! face turned to the lamp still lets a line out, and a quiet room is as
//! before.

/// Where the coordinator reads the switch.
pub const ENVIRONMENT: &str = "LAMP_GOING_ON_SAME_ONE";
/// The provider worker's flag for it, right after `--others-answer`.
pub const FLAG: &str = "--going-on-same-one";

/// Why the rule holds such a line, as its journal says it after "held
/// back: ".
pub const HELD: &str = "a busy room: a conversation with the lamp is going on, but nothing shows the line is from the one it answered: not a reply to its question, not the conversation's voice";

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
