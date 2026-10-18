//! A cut-in over the lamp's answer that was misheard gets the lamp's own
//! "hm?" (#828, the first builder at 05:07 on 19 October).
//!
//! A cut-in heard as nothing the conversation could mean (another script,
//! a number alone, no English word: `crate::not_speech`) is held, and with
//! `LAMP_OTHER_LANGUAGE=sorry` the model is asked to say "Sorry?"
//! (`crate::speak_up::ASK_AGAIN_NOTE`). Of the 59 times it was asked since
//! 16 October, the model said it 13 times and went back to its story about
//! 40: its answer to the misheard line, held, is in its context as said,
//! and it goes on with it. With `LAMP_MISHEARD_HUM=on` no model turn is
//! made for the misheard line. The lamp hums the head tap's "hm?" with the
//! listening face as soon as the turn is over, as a person or a pet does
//! when they didn't catch you, and the model is told so beside the next
//! request ([`NOTE`]), not asked to answer it. Off unless set.

/// Where the coordinator and the provider worker read the switch.
pub const ENVIRONMENT: &str = "LAMP_MISHEARD_HUM";
/// The provider worker's flag for it.
pub const FLAG: &str = "--misheard-hum";
/// Beside the first request begun after the misheard cut-in was held, in
/// the form of the lamp's other notes. #1651's cut-in note says its words
/// stopped where the person cut in; this one says too that it didn't catch
/// what was said then. It says nothing of the "hm?": a person who goes on
/// talking takes the turn first, and then no hum plays (the reviewer on
/// #1659).
pub const NOTE: &str = "(For you to know, not to answer: they spoke while you were speaking, and you stopped there, but you did not catch what they said. This is what they say now. Answer it; go back to what you were saying only if they ask you to.)";
/// A request begun later than this after the misheard cut-in was held is
/// no answer to it, and is told nothing.
pub const NOTE_WITHIN_US: u64 = 10_000_000;

/// Whether a request begun at `now_us` answers a cut-in held as misheard at
/// `held_us`, and is told so.
pub fn answers_the_hum(held_us: u64, now_us: u64) -> bool {
    now_us.saturating_sub(held_us) <= NOTE_WITHIN_US
}

/// Whether the switch is on: `on` only.
pub fn enabled_from_environment() -> bool {
    enabled(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn enabled(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The provider worker's argument for it, from the environment.
pub fn arguments_from_environment() -> Vec<String> {
    arguments(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn arguments(value: Option<&str>) -> Vec<String> {
    if enabled(value) {
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
        for value in [None, Some(""), Some("off"), Some("ON"), Some("yes")] {
            assert!(arguments(value).is_empty(), "{value:?}");
            assert!(!enabled(value), "{value:?}");
        }
        let flags: Vec<String> = [FLAG, "--next"].map(str::to_owned).into();
        let (on, rest) = take(&flags);
        assert!(on);
        assert_eq!(rest, ["--next"]);
        let (on, rest) = take(&flags[1..]);
        assert!(!on);
        assert_eq!(rest, ["--next"]);
    }

    #[test]
    fn only_a_request_begun_within_ten_seconds_answers_the_hum() {
        assert!(answers_the_hum(1_000_000, 1_000_000));
        assert!(answers_the_hum(1_000_000, 11_000_000));
        assert!(!answers_the_hum(1_000_000, 11_000_001));
        assert!(!answers_the_hum(1_000_000, 60_000_000));
    }
}
