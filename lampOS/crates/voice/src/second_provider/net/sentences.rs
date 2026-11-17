//! Streamed model text cut into whole sentences, so speech can begin with
//! the first one while the rest is still being written.
//!
//! The voice never says list markup (Queue #6811, lamp-9adf, 17 Nov: a web
//! answer was said "this week: - New CEO… - Analysts…"). A `- `, `* `,
//! `• `, `1. ` or `1) ` at a line's start or at the reply's start is taken
//! out and becomes a pause: the item before it ends as a sentence (a `.` is
//! added when it has no stop or comma of its own). After a sentence's break
//! inside a line (`.`, `!`, `?`, `…`, `:`, `;` and a space) only a `- `,
//! `* ` or `• ` is: a number there is said, as it may be the answer ("The
//! answer is: 2.", "Gate: 12."), and so is a `-` before a number
//! ("Temperature: - 5 degrees"). One marker a break: a second right after
//! it is a word. A hyphen inside a word or a number (`e-mail`, `25-30`,
//! `-5`) and a dash inside a sentence (`too - cold`) are words. Each word is
//! decided once, when the whitespace after it has come, so the text is the
//! same however it is cut into pieces. Always on, every voice; the lamp's
//! words get the same text.

/// Words whose `.` does not end a sentence.
// ponytail: a fixed English list and "terminator then whitespace"; quotes
// after the stop, "e.g." mid-word, ellipses and non-Latin stops (the ideographic full stop) are not
// handled. A real segmenter if speech is heard cut in the wrong place.
const ABBREVIATIONS: &[&str] = &[
    "mr", "mrs", "ms", "dr", "st", "jr", "sr", "vs", "etc", "prof",
];

/// The fewest words a first clause is said on its own with.
pub const CLAUSE_WORDS: usize = 4;

/// What a bullet inside a line may follow, with a space between.
const BREAKS: [char; 6] = ['.', '!', '?', '…', ':', ';'];

/// What a list marker taken out needs no `.` added after.
const PAUSES: [char; 7] = ['.', '!', '?', '…', ':', ';', ','];

#[derive(Debug, Default)]
pub struct Sentences {
    text: String,
    /// How much of `text` is decided: list markers out, each word passed
    /// on for good. The rest (the last word and the whitespace before it)
    /// waits for the whitespace after that word.
    decided: usize,
    /// The last character of the last word passed on, or a space after a
    /// marker taken out; `None` at the reply's start. Kept when a
    /// sentence, clause or first words are taken.
    last: Option<char>,
}

impl Sentences {
    /// Add text as it arrives; returns each sentence it completed, trimmed.
    /// A stop is a sentence's end only once the whitespace after it has
    /// arrived, so `3.5` split across two pieces is not cut.
    pub fn push(&mut self, piece: &str) -> Vec<String> {
        self.text.push_str(piece);
        self.unlist(false);
        let mut done = Vec::new();
        let mut start = 0;
        for (at, c) in self.text[..self.decided].char_indices() {
            let next_at = at + c.len_utf8();
            let Some(next) = self.text[next_at..].chars().next() else {
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
        self.take(start);
        done
    }

    /// The words written so far up to a comma, semicolon or colon, if at
    /// least [`CLAUSE_WORDS`] words come before it: for an answer's first
    /// words, so speech starts before the first sentence is finished.
    pub fn clause(&mut self) -> Option<String> {
        let mut words = 0;
        for (at, c) in self.text[..self.decided].char_indices() {
            if c.is_whitespace() {
                words += 1;
            }
            let next_at = at + c.len_utf8();
            let Some(next) = self.text[next_at..].chars().next() else {
                break;
            };
            if matches!(c, ',' | ';' | ':') && next.is_whitespace() && words + 1 >= CLAUSE_WORDS {
                let clause = self.text[..=at].trim().to_owned();
                self.take(next_at);
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
                if at > self.decided {
                    // A `-` still waiting to be known a minus or a bullet.
                    return None;
                }
                words += 1;
                if words == n {
                    let piece = self.text[..at].trim().to_owned();
                    self.take(at);
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
        self.unlist(true);
        let rest = std::mem::take(&mut self.text);
        self.decided = 0;
        self.last = None;
        let rest = rest.trim();
        (!rest.is_empty()).then(|| rest.to_owned())
    }

    /// Drops the first `n` bytes of the text, all decided, as said.
    fn take(&mut self, n: usize) {
        self.text.drain(..n);
        self.decided = self.decided.saturating_sub(n);
    }

    /// Decides each word after [`Self::decided`] once ([module
    /// docs](self)): a list marker is taken out, any other word passed on.
    /// `ended`: no more text comes, so the last word is whole; until then
    /// it waits for the next push, and so does a `-` after a break with no
    /// word after it yet.
    fn unlist(&mut self, ended: bool) {
        let text = std::mem::take(&mut self.text);
        let mut out = String::with_capacity(text.len());
        out.push_str(&text[..self.decided]);
        let mut rest = &text[self.decided..];
        loop {
            let word_at = rest
                .find(|c: char| !c.is_whitespace())
                .unwrap_or(rest.len());
            let (space, after) = rest.split_at(word_at);
            let word_end = after.find(char::is_whitespace);
            if after.is_empty() || (word_end.is_none() && !ended) {
                self.decided = out.len();
                out.push_str(rest);
                break;
            }
            let word = &after[..word_end.unwrap_or(after.len())];
            let next = after[word.len()..].trim_start();
            let marker = if space.contains('\n') || self.last.is_none() {
                list_marker(word)
            } else if !space.is_empty() && self.last.is_some_and(|c| BREAKS.contains(&c)) {
                if word == "-" && next.is_empty() && !ended {
                    // A minus or a bullet: the word after it says which.
                    self.decided = out.len();
                    out.push_str(rest);
                    break;
                }
                matches!(word, "*" | "•")
                    || (word == "-" && !next.starts_with(|c: char| c.is_ascii_digit()))
            } else {
                false
            };
            if marker {
                // The marker and the whitespace after it go; the
                // whitespace before it becomes a pause.
                if !out.trim().is_empty() {
                    let closed = out.trim_end_matches(['"', '\'', '”', '’', ')', '*']);
                    if !closed.ends_with(PAUSES) {
                        out.push('.');
                    }
                    out.push(' ');
                }
                self.last = Some(' ');
                rest = next;
                continue;
            }
            out.push_str(space);
            out.push_str(word);
            self.last = word.chars().last();
            rest = &after[word.len()..];
        }
        self.text = out;
    }
}

/// Whether `word`, a whole word, is a list marker at a line's start: `-`,
/// `*`, `•`, or one or two digits and `.` or `)`.
fn list_marker(word: &str) -> bool {
    if matches!(word, "-" | "*" | "•") {
        return true;
    }
    let digits = word.len() - word.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    (1..=2).contains(&digits) && matches!(&word[digits..], "." | ")")
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

    /// `text` pushed whole, and a character a push: the same sentences.
    fn whole_and_by_char(text: &str) -> Vec<String> {
        let whole = all(&[text]);
        let chars: Vec<String> = text.chars().map(String::from).collect();
        let chars: Vec<&str> = chars.iter().map(String::as_str).collect();
        assert_eq!(all(&chars), whole, "{text:?} a character a push");
        whole
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
    /// its "- " markers. A marker at a line's start, or a bullet after a
    /// sentence's break, is a pause, never a word.
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
            whole_and_by_char("* One\n* Two.\n• Three\n1. Four\n2) Five"),
            ["One.", "Two.", "Three.", "Four.", "Five"]
        );
        assert_eq!(whole_and_by_char("1. foo\n2. bar"), ["foo.", "bar"]);
        assert_eq!(
            whole_and_by_char("Two things: - tea. * Cake. • Bread"),
            ["Two things: tea.", "Cake.", "Bread"]
        );
    }

    /// El's hold on #7564: a number after a break inside a line is said,
    /// as it may be the answer; only a line's or the reply's start makes
    /// it a list marker. A minus after a break is said too (Heron).
    #[test]
    fn a_number_after_a_break_in_a_line_is_said() {
        assert_eq!(
            whole_and_by_char("The answer is: 2."),
            ["The answer is: 2."]
        );
        assert_eq!(
            whole_and_by_char("Today's high: 25. Humidity: 60."),
            ["Today's high: 25.", "Humidity: 60."]
        );
        assert_eq!(
            whole_and_by_char("Gate: 12. Boarding at nine."),
            ["Gate: 12.", "Boarding at nine."]
        );
        assert_eq!(
            whole_and_by_char("Score: Arsenal 2, Chelsea 1. Final: 3."),
            ["Score: Arsenal 2, Chelsea 1.", "Final: 3."]
        );
        assert_eq!(
            whole_and_by_char("Steps: 1) open it."),
            ["Steps: 1) open it."]
        );
        assert_eq!(
            whole_and_by_char("I'd pick option 1."),
            ["I'd pick option 1."]
        );
        assert_eq!(
            whole_and_by_char("Temperature: - 5 degrees. Cold."),
            ["Temperature: - 5 degrees.", "Cold."]
        );
    }

    /// El's P3 on #7564: each word is decided once, so a marker's removal
    /// does not go on with each push ("Note: - - x" lost a `-` a push).
    #[test]
    fn markers_are_taken_once_however_the_text_comes() {
        assert_eq!(whole_and_by_char("Note: - - x more."), ["Note: - x more."]);
        assert_eq!(
            all(&["Note: - - x", " and", " more", "."]),
            ["Note: - x and more."]
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
        assert_eq!(all(&["1", ". Milk\n", "2", ") Eggs"]), ["Milk.", "Eggs"]);
        assert_eq!(all(&["Done.\n", "1", ".", " Milk"]), ["Done.", "Milk"]);
    }

    /// A hyphen inside a word or a number, a minus, or a dash in a
    /// sentence is no list marker.
    #[test]
    fn hyphens_in_words_and_numbers_are_kept() {
        assert_eq!(
            whole_and_by_char(
                "Send an e-mail by 25-30 May. -5 degrees, \u{2212}5 too - cold.\n-3 at night."
            ),
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
