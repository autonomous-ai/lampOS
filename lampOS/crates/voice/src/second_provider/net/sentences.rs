//! Streamed model text cut into whole sentences, so speech can begin with
//! the first one while the rest is still being written.
//!
//! The voice never says list markup (Queue #6811, lamp-9adf, 17 Nov: a web
//! answer was said "this week: - New CEO… - Analysts…"). A `- `, `* `,
//! `• `, `1. ` or `1) ` at a line's start, at the reply's start or after a
//! sentence's break (`.`, `!`, `?`, `…`, `:`, `;` and a space) is taken out
//! and becomes a pause: the item before it ends as a sentence (a `.` is
//! added when it has no stop or comma of its own). A hyphen inside a word
//! or a number (`e-mail`, `25-30`, `-5`) and a dash inside a sentence
//! (`too - cold`) are words. Always on, every voice; the lamp's words get
//! the same text.

/// Words whose `.` does not end a sentence.
// ponytail: a fixed English list and "terminator then whitespace"; quotes
// after the stop, "e.g." mid-word, ellipses and non-Latin stops (the ideographic full stop) are not
// handled. A real segmenter if speech is heard cut in the wrong place.
const ABBREVIATIONS: &[&str] = &[
    "mr", "mrs", "ms", "dr", "st", "jr", "sr", "vs", "etc", "prof",
];

/// The fewest words a first clause is said on its own with.
pub const CLAUSE_WORDS: usize = 4;

/// What a list marker may follow, with a space between, on one line.
const BREAKS: [char; 6] = ['.', '!', '?', '…', ':', ';'];

/// What a list marker taken out needs no `.` added after.
const PAUSES: [char; 7] = ['.', '!', '?', '…', ':', ';', ','];

#[derive(Debug, Default)]
pub struct Sentences {
    text: String,
    /// The text kept begins inside a sentence (after a first clause or
    /// first words that did not end at a break), so a marker at its start
    /// is no list marker unless a new line comes first.
    mid_sentence: bool,
}

impl Sentences {
    /// Add text as it arrives; returns each sentence it completed, trimmed.
    /// A stop is a sentence's end only once the whitespace after it has
    /// arrived, so `3.5` split across two pieces is not cut.
    pub fn push(&mut self, piece: &str) -> Vec<String> {
        self.text.push_str(piece);
        self.text = unlisted(&self.text, !self.mid_sentence, false);
        let mut done = Vec::new();
        let mut start = 0;
        let mut chars = self.text.char_indices().peekable();
        while let Some((at, c)) = chars.next() {
            let Some(&(next_at, next)) = chars.peek() else {
                break;
            };
            if matches!(c, '.' | '!' | '?')
                && next.is_whitespace()
                && !(c == '.' && abbreviation(&self.text[start..at]))
            {
                let sentence = self.text[start..=at].trim();
                if !sentence.is_empty() {
                    done.push(sentence.to_owned());
                }
                start = next_at;
            }
        }
        if start > 0 {
            self.mid_sentence = false;
        }
        self.text.drain(..start);
        done
    }

    /// The words written so far up to a comma, semicolon or colon, if at
    /// least [`CLAUSE_WORDS`] words come before it: for an answer's first
    /// words, so speech starts before the first sentence is finished.
    pub fn clause(&mut self) -> Option<String> {
        let mut words = 0;
        let mut chars = self.text.char_indices().peekable();
        while let Some((at, c)) = chars.next() {
            if c.is_whitespace() {
                words += 1;
            }
            let Some(&(next_at, next)) = chars.peek() else {
                break;
            };
            if matches!(c, ',' | ';' | ':') && next.is_whitespace() && words + 1 >= CLAUSE_WORDS {
                let clause = self.text[..=at].trim().to_owned();
                self.text.drain(..next_at);
                self.mid_sentence = c == ',';
                return Some(clause);
            }
        }
        None
    }

    /// The first `n` words written so far, once `n` have come whole (each
    /// followed by whitespace): for a voice asked one request a piece, so
    /// its first sound waits for neither a sentence nor a clause (#1712).
    pub fn words(&mut self, n: usize) -> Option<String> {
        let mut words = 0;
        let mut in_word = false;
        for (at, c) in self.text.char_indices() {
            if !c.is_whitespace() {
                in_word = true;
            } else if std::mem::replace(&mut in_word, false) {
                words += 1;
                if words == n {
                    let piece = self.text[..at].trim().to_owned();
                    self.text.drain(..at);
                    self.mid_sentence = !piece.ends_with(BREAKS);
                    return Some(piece);
                }
            }
        }
        None
    }

    /// The text written so far that no sentence, clause or first words
    /// have taken yet.
    pub fn pending(&self) -> &str {
        &self.text
    }

    /// Whatever is left when the text ends.
    pub fn finish(&mut self) -> Option<String> {
        let rest = unlisted(&std::mem::take(&mut self.text), !self.mid_sentence, true);
        self.mid_sentence = false;
        let rest = rest.trim();
        (!rest.is_empty()).then(|| rest.to_owned())
    }
}

/// Whether `word`, a whole word, is a list marker: `-`, `*`, `•`, or one
/// or two digits and `.` or `)`.
fn list_marker(word: &str) -> bool {
    if matches!(word, "-" | "*" | "•") {
        return true;
    }
    let digits = word.len() - word.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    (1..=2).contains(&digits) && matches!(&word[digits..], "." | ")")
}

/// Whether `word`, still being written, may yet become a list marker.
fn may_become_marker(word: &str) -> bool {
    let digits = word.len() - word.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    list_marker(word) || (digits == word.len() && (1..=2).contains(&digits))
}

/// `text` with each list marker that is known whole taken out ([module
/// docs](self)). `at_break`: the text begins at a sentence's break.
/// `ended`: no more text comes, so a marker at its very end is whole; until
/// then a word at the end that may yet be one ("1", "1.", "-") is left as
/// written, and the next push decides.
fn unlisted(text: &str, at_break: bool, ended: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        let word_at = rest
            .find(|c: char| !c.is_whitespace())
            .unwrap_or(rest.len());
        let (space, after) = rest.split_at(word_at);
        if after.is_empty() {
            out.push_str(space);
            break;
        }
        let word_end = after.find(char::is_whitespace);
        let word = &after[..word_end.unwrap_or(after.len())];
        // `out` ends with the last word passed on, or is empty.
        let before = out.chars().last();
        let new_line = space.contains('\n');
        let breaks = new_line
            || match before {
                None => at_break,
                Some(c) => !space.is_empty() && BREAKS.contains(&c),
            };
        if breaks {
            match word_end {
                None if !ended && may_become_marker(word) => {
                    out.push_str(space);
                    out.push_str(after);
                    break;
                }
                _ if list_marker(word) => {
                    // The marker and the whitespace after it go; the
                    // whitespace before it becomes a pause.
                    let next = after[word.len()..].trim_start();
                    if before.is_some() {
                        let closed = out.trim_end_matches(['"', '\'', '”', '’', ')', '*']);
                        if !closed.ends_with(PAUSES) {
                            out.push('.');
                        }
                        out.push(' ');
                    }
                    rest = next;
                    continue;
                }
                _ => {}
            }
        }
        out.push_str(space);
        out.push_str(word);
        rest = &after[word.len()..];
    }
    out
}

fn abbreviation(before: &str) -> bool {
    let word = before
        .rsplit(|c: char| !c.is_alphabetic())
        .next()
        .unwrap_or("");
    ABBREVIATIONS
        .iter()
        .any(|known| word.eq_ignore_ascii_case(known))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(pieces: &[&str]) -> Vec<String> {
        let mut sentences = Sentences::default();
        let mut out: Vec<String> = pieces.iter().flat_map(|p| sentences.push(p)).collect();
        out.extend(sentences.finish());
        out
    }

    #[test]
    fn a_first_clause_of_four_words_is_cut_at_its_comma() {
        let mut sentences = Sentences::default();
        assert!(sentences.push("Well, ").is_empty());
        assert_eq!(sentences.clause(), None);
        sentences.push("it is about nine million, give or take");
        assert_eq!(
            sentences.clause().as_deref(),
            Some("Well, it is about nine million,")
        );
        assert_eq!(sentences.finish().as_deref(), Some("give or take"));
    }

    /// #1712: the first words go once that many have come whole; the rest
    /// of the sentence follows as a sentence.
    #[test]
    fn the_first_words_are_cut_once_they_have_come_whole() {
        let mut sentences = Sentences::default();
        assert!(sentences.push("Tomorrow will be ").is_empty());
        assert_eq!(sentences.words(5), None);
        assert!(sentences.push("hot, around").is_empty());
        assert_eq!(sentences.words(5), None, "\"around\" may go on");
        assert!(sentences.push(" 35 degrees.").is_empty());
        assert_eq!(
            sentences.words(5).as_deref(),
            Some("Tomorrow will be hot, around")
        );
        assert_eq!(sentences.push(" Stay cool."), ["35 degrees."]);
        assert_eq!(sentences.finish().as_deref(), Some("Stay cool."));
    }

    #[test]
    fn sentences_end_at_a_stop_and_whitespace() {
        assert_eq!(
            all(&["Hello there! How", " are you? I'm", " fine."]),
            ["Hello there!", "How are you?", "I'm fine."]
        );
    }

    #[test]
    fn a_sentence_is_held_until_its_stop_is_followed() {
        let mut sentences = Sentences::default();
        assert!(sentences.push("It is 3.").is_empty());
        assert!(sentences.push("5 degrees.").is_empty());
        assert_eq!(sentences.push(" Cold."), ["It is 3.5 degrees."]);
        assert_eq!(sentences.finish().as_deref(), Some("Cold."));
    }

    #[test]
    fn titles_do_not_end_a_sentence() {
        assert_eq!(
            all(&["Ask Mr. Smith and Dr. Who. Then go."]),
            ["Ask Mr. Smith and Dr. Who.", "Then go."]
        );
    }

    /// Queue #6811 (lamp-9adf, 17 Nov): a web answer's list was said with
    /// its "- " markers. A marker at a line's start or after a sentence's
    /// break is a pause, never a word.
    #[test]
    fn list_markers_are_pauses_not_words() {
        assert_eq!(
            all(&[
                "A few threads around Apple this week:\n- New CEO John Ternus reportedly pushed",
                " a reorg\n- Analysts are debating",
                " the iPhone.\n"
            ]),
            [
                "A few threads around Apple this week: New CEO John Ternus reportedly pushed a reorg.",
                "Analysts are debating the iPhone."
            ]
        );
        assert_eq!(
            all(&["* One\n* Two.\n• Three\n1. Four\n2) Five"]),
            ["One.", "Two.", "Three.", "Four.", "Five"]
        );
        assert_eq!(
            all(&["Two things: - tea. - Cake. 1. Bread"]),
            ["Two things: tea.", "Cake.", "Bread"]
        );
    }

    /// A marker cut across pieces is still known: "1." is never said as
    /// a sentence of its own, and a "-" alone at a line's end waits.
    #[test]
    fn a_list_marker_cut_across_pieces_is_still_a_pause() {
        assert_eq!(
            all(&["Here:\n1", ". Milk\n", "2", ".", " Eggs\n-", " Bread"]),
            ["Here: Milk.", "Eggs.", "Bread"]
        );
        let mut sentences = Sentences::default();
        assert!(sentences.push("Here:\n1.").is_empty());
        assert_eq!(sentences.push(" Milk. Then"), ["Here: Milk."]);
    }

    /// A hyphen inside a word or a number, a minus, or a dash in a
    /// sentence is no list marker.
    #[test]
    fn hyphens_in_words_and_numbers_are_kept() {
        assert_eq!(
            all(&["Send an e-mail by 25-30 May. -5 degrees, \u{2212}5 too - cold.\n-3 at night."]),
            [
                "Send an e-mail by 25-30 May.",
                "-5 degrees, \u{2212}5 too - cold.",
                "-3 at night."
            ]
        );
    }

    #[test]
    fn nothing_left_is_nothing() {
        let mut sentences = Sentences::default();
        assert_eq!(sentences.push("Done.\n"), ["Done."]);
        assert_eq!(sentences.finish(), None);
    }
}
