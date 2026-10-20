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

/// Where the coordinator reads the switch.
pub const ENVIRONMENT: &str = "LAMP_SAID_AS_HEARD";

/// After the words of a piece cut while it played.
pub const CUT: &str = "—";

/// Whether the switch is on: `LAMP_SAID_AS_HEARD=on`.
pub fn from_environment() -> bool {
    on(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn on(value: Option<&str>) -> bool {
    value == Some("on")
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
        for value in [None, Some(""), Some("off"), Some("1"), Some("ON")] {
            assert!(!on(value), "{value:?}");
        }
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
