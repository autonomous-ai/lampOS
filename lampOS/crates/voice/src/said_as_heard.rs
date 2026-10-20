//! What the lamp said, kept as what was heard of it (#1712; the first
//! builder's yes on #1722, 20 Oct 09:38), off unless `LAMP_SAID_AS_HEARD=on`.
//!
//! The second route sends each piece of an answer, its words and then its
//! sound, as fast as speech is made, and a reply holds up to 30 s of audio
//! (`provider_flow::MAX_REPLY_SAMPLES`). Its worker keeps each piece's words
//! in the thread as the lamp's as they are sent. So a line said over the
//! lamp 5 s into a 15 s story was asked with the whole story as the lamp's
//! words: "go on" skipped what was never heard, and the cut-in note's "you
//! stopped there" (`crate::cut_in_told::NOTE`) meant the story's end.
//!
//! With the switch, the coordinator tells the line that cut a reply, or the
//! first line after a Stop that cut one, how much of it was played: a
//! [`HeardOfCut`] on `ProviderInput::Start`, the samples it had handed to the
//! speaker. The second route keeps only the words of those samples as the
//! lamp's ([`heard`]): each piece played whole, then the share of the words
//! of the piece playing at the cut with [`CUT`] after them, and nothing after
//! that. The Live route is told nothing new: Gemini's server keeps what it
//! made, so its history can't be put right this way.
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Where the coordinator reads the switch.
pub const ENVIRONMENT: &str = "LAMP_SAID_AS_HEARD";
/// The provider worker's flag for it, right after `--messages`: the second
/// route then holds an answer's hand-on to memory until it is known how
/// much of it was heard ([`HeldHandOn`]).
pub const FLAG: &str = "--said-as-heard";
/// The longest an answer's hand-on to memory is held after its turn ends:
/// the most audio a reply holds (30 s, `MAX_REPLY_SAMPLES`), and well before
/// memory's Days writer ends a conversation idle for 45 s (Twelve, 10:40 on
/// #1722).
pub const HAND_ON_WITHIN: Duration = Duration::from_secs(30);

/// After the words of a piece cut while it played.
pub const CUT: &str = "—";

/// Whether the switch is on: `LAMP_SAID_AS_HEARD=on`.
pub fn from_environment() -> bool {
    on(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The worker's flags for the switch, from the coordinator's environment.
pub fn arguments_from_environment() -> Vec<String> {
    arguments(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn arguments(value: Option<&str>) -> Vec<String> {
    if on(value) {
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

/// An answer's hand-on to the lamp's memory, held from its turn's end
/// (`crate::remember::finished`). The route sends a long answer in seconds
/// and the lamp plays it for many more, so handed on then, memory took it
/// whole even when a line or a Stop cut it. Held, it goes once the next line
/// begins, after its words are kept as heard, or after [`HAND_ON_WITHIN`],
/// or as the worker stops. Each call gives the request to hand on now.
#[derive(Debug, Default)]
pub struct HeldHandOn(Option<(u64, Instant)>);

impl HeldHandOn {
    /// `request`'s turn ended at `now`: held, and an older one held before
    /// it is handed on now.
    pub fn hold(&mut self, request: u64, now: Instant) -> Option<u64> {
        self.0.replace((request, now)).map(|(older, _)| older)
    }

    /// A new line begins, or the worker stops: what is held goes now.
    pub fn release(&mut self) -> Option<u64> {
        self.0.take().map(|(request, _)| request)
    }

    /// What has been held [`HAND_ON_WITHIN`] by `now` goes now.
    pub fn due(&mut self, now: Instant) -> Option<u64> {
        self.0
            .take_if(|(_, at)| now.saturating_duration_since(*at) >= HAND_ON_WITHIN)
            .map(|(request, _)| request)
    }
}

/// How much of a reply was played before a line, or a Stop, cut it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeardOfCut {
    /// The request whose reply was cut.
    pub request: u64,
    /// Its samples the coordinator had handed to the speaker, at the
    /// reply's rate.
    pub samples: u64,
}

/// One piece of an answer as the second route sent it: its words, none for
/// a wait sound, which has sound and no words in the thread; and its samples.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Piece {
    pub words: Option<String>,
    pub samples: u64,
}

/// The words of `pieces` played in their first `samples`: those of each
/// piece played whole, then the share of the words of the one playing at the
/// cut, rounded down, with [`CUT`] after the last; none of it when its share
/// comes to no word. A piece with no sound was never heard. Empty when no
/// word was played.
pub fn heard(pieces: &[Piece], samples: u64) -> String {
    let mut said: Vec<&str> = Vec::new();
    let mut left = samples;
    let mut cut = false;
    for piece in pieces.iter().filter(|piece| piece.samples > 0) {
        let words: Vec<&str> = piece
            .words
            .as_deref()
            .unwrap_or_default()
            .split_whitespace()
            .collect();
        if left >= piece.samples {
            left -= piece.samples;
            said.extend(words);
            continue;
        }
        let share = u128::from(left) * words.len() as u128 / u128::from(piece.samples);
        let share = usize::try_from(share).unwrap_or(0);
        said.extend(&words[..share]);
        cut = share > 0;
        break;
    }
    let mut text = said.join(" ");
    if cut {
        text.push_str(CUT);
    }
    text
}

/// How many words `pieces` have in all.
pub fn words_in(pieces: &[Piece]) -> usize {
    pieces
        .iter()
        .filter_map(|piece| piece.words.as_deref())
        .map(|words| words.split_whitespace().count())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(words: Option<&str>, samples: u64) -> Piece {
        Piece {
            words: words.map(str::to_owned),
            samples,
        }
    }

    #[test]
    fn the_switch_is_on_only_when_said() {
        assert!(on(Some("on")));
        assert_eq!(arguments(Some("on")), [FLAG]);
        for value in [None, Some(""), Some("off"), Some("1"), Some("ON")] {
            assert!(!on(value), "{value:?}");
            assert!(arguments(value).is_empty(), "{value:?}");
        }
        let flags = [FLAG.to_owned(), "x".to_owned()];
        assert_eq!(take(&flags), (true, &flags[1..]));
        assert_eq!(take(&flags[1..]), (false, &flags[1..]));
    }

    /// An answer's hand-on waits for the next line, the most a reply can
    /// play, or the stop; one held goes when a newer turn ends.
    #[test]
    fn a_held_hand_on_goes_with_the_next_line_or_once_the_reply_has_played() {
        let start = Instant::now();
        let mut held = HeldHandOn::default();
        assert_eq!(held.release(), None);
        assert_eq!(held.hold(1, start), None);
        assert_eq!(
            held.due(start + HAND_ON_WITHIN - Duration::from_millis(1)),
            None
        );
        assert_eq!(held.due(start + HAND_ON_WITHIN), Some(1));
        assert_eq!(held.due(start + HAND_ON_WITHIN * 2), None, "once");
        assert_eq!(held.hold(2, start), None);
        assert_eq!(held.hold(3, start), Some(2));
        assert_eq!(held.release(), Some(3));
        assert_eq!(held.release(), None);
    }

    /// Three sentences of a story, the second cut halfway: the first whole,
    /// half of the second's words with the dash, none of the third.
    #[test]
    fn only_the_words_played_before_the_cut_are_kept() {
        let story = [
            piece(Some("Once upon a time, a keeper lived."), 2_400),
            piece(Some("Every night he climbed the stairs."), 2_400),
            piece(Some("One winter a ship came."), 2_400),
        ];
        assert_eq!(
            heard(&story, 2_400 + 1_200),
            "Once upon a time, a keeper lived. Every night he—"
        );
        // At a piece's end, nothing of the next is said and no dash goes.
        assert_eq!(heard(&story, 2_400), "Once upon a time, a keeper lived.");
        // Short of the next piece's first word: none of it.
        assert_eq!(
            heard(&story, 2_400 + 300),
            "Once upon a time, a keeper lived."
        );
        // All played: every word, as without the switch.
        assert_eq!(
            heard(&story, 7_200),
            "Once upon a time, a keeper lived. Every night he climbed the stairs. One winter a ship came."
        );
        assert_eq!(heard(&story, 10_000), heard(&story, 7_200));
        assert_eq!(words_in(&story), 18);
    }

    /// The wait sound's samples come before the answer's and say nothing:
    /// the answer's words are counted from its end. Nothing played is no
    /// words; a piece whose sound never came was never heard.
    #[test]
    fn a_wait_sound_counts_its_time_and_says_no_word() {
        let answer = [
            piece(None, 4_800),
            piece(Some("It is noon."), 1_500),
            piece(Some("Lunch soon."), 0),
            piece(Some("Enjoy it."), 1_500),
        ];
        assert_eq!(heard(&answer, 4_800), "");
        assert_eq!(heard(&answer, 4_000), "");
        assert_eq!(heard(&answer, 0), "");
        assert_eq!(heard(&answer, 4_800 + 1_000), "It is—");
        assert_eq!(heard(&answer, 4_800 + 1_500 + 750), "It is noon. Enjoy—");
        assert_eq!(words_in(&answer), 7);
        assert_eq!(heard(&[], 9_000), "");
    }
}
