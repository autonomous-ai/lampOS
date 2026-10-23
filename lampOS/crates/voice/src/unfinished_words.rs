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
//! [`WAIT_BLOCKS`] more of quiet at most, in all, a line: speech in that
//! time goes on with the same line, and a person who truly stopped after
//! "um" is answered when it runs out. An end the end-of-turn model already
//! held as incomplete (`model_incomplete`) is not held again. Each held end
//! is told in the journal, counts only: the trailing word's class, how long
//! it waited, and whether more speech came.
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;

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
/// most this much later than before. It is the line's, not a pause's: holds
/// on one line add up to no more, and no line's quiet passes the silence
/// window plus this. To be set from lamp-4ace's journal.
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
}

impl Trailing {
    /// Its name in the journal.
    pub fn name(self) -> &'static str {
        match self {
            Self::Joining => "joining",
            Self::Filler => "filler",
        }
    }
}

/// Words that can't end a whole request, lower case, with their class:
/// English, and Vietnamese for a route whose speech to text is set to it.
/// No question words: interim results rarely carry the "?", and "tell me
/// why", "show me how" or "you know what" are whole requests (El on #3045).
/// A rule by the words before one would be one more guess; none is simpler.
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
/// formatting and are ignored. Both sides are compared composed (NFC), so a
/// service's decomposed "và" (a, then the grave accent) is "và".
pub fn trailing(words: &str) -> Option<Trailing> {
    let last = words.split_whitespace().last()?;
    if last.ends_with(['?', '!']) {
        return None;
    }
    class(last)
}

/// The class of `word`, any punctuation around it aside.
fn class(word: &str) -> Option<Trailing> {
    // Composed before the trim: a decomposed accent is not alphanumeric.
    let word: String = word.nfc().collect();
    let word = word
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    UNFINISHED_WORDS
        .iter()
        .find(|(unfinished, _)| unfinished.nfc().eq(word.chars()))
        .map(|(_, class)| *class)
}

/// `words` without its fillers ([`Trailing::Filler`]), each with its
/// punctuation: the service's fillers are asked for only so that
/// [`trailing`] sees them (#3126), and nothing else reads a line with
/// them. A "." "?" or "!" that ended the words on a filler goes to the
/// word before it, for a comma there ("eggs, um." is "eggs.").
pub fn without_fillers(words: &str) -> String {
    let mut kept: Vec<String> = Vec::new();
    for word in words.split_whitespace() {
        if class(word) != Some(Trailing::Filler) {
            kept.push(word.to_owned());
            continue;
        }
        let mark = word.chars().last().filter(|c| matches!(c, '.' | '?' | '!'));
        if let (Some(mark), Some(last)) = (mark, kept.last_mut()) {
            let bare = last.trim_end_matches(',').len();
            last.truncate(bare);
            if !last.ends_with(['.', '?', '!']) {
                last.push(mark);
            }
        }
    }
    let mut words = kept.join(" ");
    if words.ends_with(',') {
        words.pop();
    }
    words
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
        assert_eq!(
            trailing("Cho tôi biết thời tiết của"),
            Some(Trailing::Joining)
        );
    }

    #[test]
    fn a_decomposed_word_is_its_composed_one() {
        // "và" as "a" and U+0300, the combining grave accent.
        assert_eq!(trailing("Bật đèn va\u{300}"), Some(Trailing::Joining));
        assert_eq!(trailing("Bật đèn v\u{e0}"), Some(Trailing::Joining));
    }

    #[test]
    fn a_question_word_last_is_a_whole_request() {
        assert_eq!(trailing("tell me why"), None);
        assert_eq!(trailing("show me how"), None);
        assert_eq!(trailing("you know what"), None);
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

    /// #3126: a filler goes, any case, with its punctuation; a mark that
    /// ended the words stays at their end; a word that only begins with
    /// one stays.
    #[test]
    fn fillers_are_taken_out_of_the_words() {
        assert_eq!(
            without_fillers("I need eggs, um, and some bread."),
            "I need eggs, and some bread."
        );
        assert_eq!(without_fillers("I need eggs, um"), "I need eggs");
        assert_eq!(without_fillers("I need eggs, Uh."), "I need eggs.");
        assert_eq!(without_fillers("Um, what time is it?"), "what time is it?");
        assert_eq!(without_fillers("uh-huh, umbrella"), "uh-huh, umbrella");
        assert_eq!(without_fillers("um"), "");
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
