//! A line whose words so far end unfinished waits longer before its end
//! (`LAMP_SECOND_UNFINISHED_WAIT=on`, #3026). Off unless set.
//!
//! On the second route a line's end is decided by the coordinator's
//! detector alone, by the sound: the silence window, or the end-of-turn
//! model's verdict (`activity::TurnDetector`). Neither looks at the words.
//! In the owner's test on lamp-4ace on 22 Oct, 19:04–19:08, 5 of 28 lines
//! were cut at a mid-sentence pause ("Lamp, I'm thinking about … going for
//! a run tomorrow morning, is that a good idea?") and the second half was
//! answered alone.
//!
//! With the switch, the second route's worker tells the coordinator, as
//! the words come back from speech to text, whether the words so far end
//! on one of [`UNFINISHED_WORDS`] (`ProviderOutput::WordsEnd`, only when
//! that changes). While they do, an end the detector decides is held for
//! [`WAIT_BLOCKS`] more of quiet: speech in that time goes on with the same
//! line, and a person who truly stopped after "um" is answered when it runs
//! out. Each held end is told in the journal, counts only: the trailing
//! word's class, how long it waited, and whether more speech came.
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Only the exact value `on`.
pub const ENVIRONMENT: &str = "LAMP_SECOND_UNFINISHED_WAIT";

/// Whether the switch is on for this process; read once.
pub fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var(ENVIRONMENT).ok().as_deref() == Some("on"))
}

/// The extra quiet an unfinished line waits after its end was due: 1.3 s,
/// in the detector's 10 ms blocks. The first builder's guess for a real
/// mid-sentence pause is 1.2 to 1.5 s (#3026); a pause longer than this
/// still ends the line, so a person who stopped after "um" is answered at
/// most this much later than before. To be set from lamp-4ace's journal.
pub const WAIT_BLOCKS: u16 = 130;

/// What kind of word the words so far end on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trailing {
    /// A word that needs one after it: "about", "the", "and", "to", "my",
    /// or a Vietnamese one such as "và", "của", "để".
    Joining,
    /// A filler: "um", "uh".
    Filler,
    /// A question word with nothing after it: "what", "where".
    Question,
}

impl Trailing {
    /// Its name in the journal.
    pub fn name(self) -> &'static str {
        match self {
            Self::Joining => "joining",
            Self::Filler => "filler",
            Self::Question => "question",
        }
    }
}

/// Words that can't end a whole request, lower case, with their class:
/// English, and Vietnamese for a route whose speech to text is set to it.
pub const UNFINISHED_WORDS: &[(&str, Trailing)] = &[
    ("about", Trailing::Joining),
    ("the", Trailing::Joining),
    ("a", Trailing::Joining),
    ("and", Trailing::Joining),
    ("to", Trailing::Joining),
    ("so", Trailing::Joining),
    ("like", Trailing::Joining),
    ("but", Trailing::Joining),
    ("or", Trailing::Joining),
    ("of", Trailing::Joining),
    ("for", Trailing::Joining),
    ("with", Trailing::Joining),
    ("my", Trailing::Joining),
    ("um", Trailing::Filler),
    ("uh", Trailing::Filler),
    ("what", Trailing::Question),
    ("where", Trailing::Question),
    ("when", Trailing::Question),
    ("how", Trailing::Question),
    ("why", Trailing::Question),
    ("who", Trailing::Question),
    ("which", Trailing::Question),
    ("và", Trailing::Joining),
    ("của", Trailing::Joining),
    ("là", Trailing::Joining),
    ("thì", Trailing::Joining),
    ("mà", Trailing::Joining),
    ("để", Trailing::Joining),
];

/// The class of the last word of `words`, if it is one of
/// [`UNFINISHED_WORDS`]. A word with a question or exclamation mark after
/// it was said as an end ("What?"); commas and dots are only the service's
/// formatting and are ignored.
pub fn trailing(words: &str) -> Option<Trailing> {
    let last = words.split_whitespace().last()?;
    if last.ends_with(['?', '!']) {
        return None;
    }
    let word = last
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    UNFINISHED_WORDS
        .iter()
        .find(|(unfinished, _)| *unfinished == word)
        .map(|(_, class)| *class)
}

/// One held end, for the journal: counts only, never the words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Waited {
    pub class: Trailing,
    /// Quiet waited past the end that was due, in ms.
    pub waited_ms: u64,
    /// Speech came before the wait ran out: the line went on.
    pub went_on: bool,
}

impl Waited {
    /// The journal's line for `request`.
    pub fn told(&self, request: Option<u64>) -> String {
        let request = request.map_or("none".to_owned(), |request| request.to_string());
        format!(
            "lamp-voice: request {request}: unfinished words ({}) held its end {} ms, more speech came: {}",
            self.class.name(),
            self.waited_ms,
            if self.went_on { "yes" } else { "no" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_word_is_classed() {
        assert_eq!(
            trailing("Lamp, I'm thinking about"),
            Some(Trailing::Joining)
        );
        assert_eq!(
            trailing("Lamp, I'm thinking about..."),
            Some(Trailing::Joining)
        );
        assert_eq!(trailing("Can you tell me, um,"), Some(Trailing::Filler));
        assert_eq!(trailing("Lamp, what"), Some(Trailing::Question));
        assert_eq!(
            trailing("Cho tôi biết thời tiết của"),
            Some(Trailing::Joining)
        );
    }

    #[test]
    fn a_finished_line_has_no_class() {
        assert_eq!(trailing("What's the weather in Hanoi?"), None);
        assert_eq!(trailing("Turn on the light"), None);
        assert_eq!(trailing("What?"), None);
        assert_eq!(trailing(""), None);
        // "a" only as a word of its own.
        assert_eq!(trailing("Play some salsa"), None);
    }

    #[test]
    fn the_journal_line_has_counts_only() {
        let told = Waited {
            class: Trailing::Filler,
            waited_ms: 1_300,
            went_on: false,
        }
        .told(Some(7));
        assert_eq!(
            told,
            "lamp-voice: request 7: unfinished words (filler) held its end 1300 ms, more speech came: no"
        );
    }
}
