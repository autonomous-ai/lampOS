//! The language one computer's turn is heard in (`LAMP_SECOND_TURN_LANGUAGE`,
//! mixed-language speech to text, step 2): the second route's speech to text
//! hears the lines of that turn, and only those, in the language the turn
//! asked for instead of the lamp's own.
//!
//! - **A closed set.** Only [`TurnLanguage::Multi`] (Deepgram's `multi`,
//!   which follows a speaker between languages inside a line: on 12
//!   Vietnamese lines with English words, `multi` heard 86% of the words,
//!   `vi` 61% and `en` 23%, #7814). The value is a variant, never a string
//!   that reaches the relay's URL; anything else is refused where the turn
//!   is parsed ([`crate::say`]).
//! - **Off unless `on`.** With the switch off the language a turn asks for
//!   is ignored and said once; the lamp hears as it always has. No product
//!   unit sets it.
//! - **One turn.** The coordinator gives a line its language when the line
//!   begins inside the turn ([`crate::control_turn::ControlTurns::language_at`]);
//!   the line's `Start` carries it to the second route, which opens its
//!   speech to text with it and does not use a listener kept ready in the
//!   lamp's own language (or the reverse). Nothing about it outlives the
//!   turn.
//!
//! The journal says the value, never a word heard.
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

/// Where the coordinator reads the switch.
pub const ENVIRONMENT: &str = "LAMP_SECOND_TURN_LANGUAGE";

/// A language a turn may ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnLanguage {
    /// Deepgram's `multi`.
    Multi,
}

impl TurnLanguage {
    /// The `language` the speech to text is asked for.
    pub fn code(self) -> &'static str {
        match self {
            Self::Multi => "multi",
        }
    }

    /// The language a name asks for; none for any other.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "multi" => Some(Self::Multi),
            _ => None,
        }
    }
}

/// Whether `value`, the switch's, arms it: exactly `on`.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The switch as this process's environment has it.
pub fn armed() -> bool {
    #[cfg(test)]
    match FORCED.load(Ordering::SeqCst) {
        1 => return false,
        2 => return true,
        _ => {}
    }
    on(std::env::var(ENVIRONMENT).ok().as_deref())
}

#[cfg(test)]
static FORCED: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// The loop reads the switch as `on` (`Some`) until it is called again with
/// `None`: the loop runs on a thread of its own, and the process environment
/// is shared by every test, so a test holding the one session sets it here.
#[cfg(test)]
pub(crate) fn force_for_test(on: Option<bool>) {
    FORCED.store(
        match on {
            None => 0,
            Some(false) => 1,
            Some(true) => 2,
        },
        Ordering::SeqCst,
    );
}

static SAID_OFF: AtomicBool = AtomicBool::new(false);

/// The language a turn's start asked for, if the switch lets it through:
/// with it off the ask is dropped and said once.
pub fn allowed(asked: Option<TurnLanguage>, armed: bool) -> Option<TurnLanguage> {
    let asked = asked?;
    if armed {
        return Some(asked);
    }
    if !SAID_OFF.swap(true, Ordering::Relaxed) {
        eprintln!(
            "lamp-voice: a turn asked to be heard in {}: ignored, {ENVIRONMENT} is not on",
            asked.code()
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_multi_is_a_language_and_only_by_its_exact_name() {
        assert_eq!(TurnLanguage::parse("multi"), Some(TurnLanguage::Multi));
        for other in [
            "",
            "Multi",
            "multi ",
            "en",
            "vi",
            "multi&model=x",
            "multi\n",
        ] {
            assert_eq!(TurnLanguage::parse(other), None, "{other:?}");
        }
        assert_eq!(TurnLanguage::Multi.code(), "multi");
    }

    #[test]
    fn the_wire_takes_the_closed_set_and_nothing_else() {
        assert_eq!(
            serde_json::from_str::<TurnLanguage>("\"multi\"").unwrap(),
            TurnLanguage::Multi
        );
        assert!(serde_json::from_str::<TurnLanguage>("\"vi\"").is_err());
        assert_eq!(
            serde_json::to_string(&TurnLanguage::Multi).unwrap(),
            "\"multi\""
        );
    }

    #[test]
    fn only_the_exact_on_arms_it_and_an_off_switch_lets_nothing_through() {
        assert!(on(Some("on")));
        for other in [
            None,
            Some(""),
            Some("ON"),
            Some("1"),
            Some("true"),
            Some("off"),
        ] {
            assert!(!on(other), "{other:?}");
        }
        assert_eq!(allowed(None, true), None);
        assert_eq!(allowed(None, false), None);
        assert_eq!(
            allowed(Some(TurnLanguage::Multi), true),
            Some(TurnLanguage::Multi)
        );
        assert_eq!(allowed(Some(TurnLanguage::Multi), false), None);
    }
}
