//! A keep the lamp itself doubts is not run (#3504 finding N5). On
//! lamp-06c1 on 27 Oct (P5) "tulip" was heard as "the two", and the lamp
//! kept it and then said "Kept: … "the two." I may have misheard that". The
//! `remember` description now says to ask first when unsure of a key word
//! ([`super::REMEMBER_DESCRIBED`]). And on the second route, a `remember`
//! called in a chat round whose own words doubt what was heard ("I may have
//! misheard", "did you say …?", "nghe nhầm") is not run that round: the
//! model is told [`NOT_KEPT`], and keeps it once they say it again. A round
//! with no words, as P5's first was, is not judged: only the description
//! reaches it. Normal turns do one search of the round's words, no I/O.
use unicode_normalization::UnicodeNormalization;

/// What the model is told of a `remember` not run, in a refused one's
/// shape: nothing kept, ask them to say it again, never say it was kept.
pub const NOT_KEPT: &str = "Not kept: in this same reply you doubted what you heard. Ask them, in a few words, to say it again, and keep it once they do. Never say it was kept.";

/// The journal's line for one not run: no words of it.
pub const SKIPPED: &str = "a remember was not run: its own round's words doubt what was heard";

/// What a reply says when it doubts what it heard, lower case, `'` for `’`.
const DOUBTS: &[&str] = &[
    "misheard",
    "mis-heard",
    "mishear",
    "didn't catch",
    "did not catch",
    "didn't quite catch",
    "not sure i heard",
    "not sure if i heard",
    "not sure i caught",
    "if i heard you right",
    "if i heard that right",
    "if i heard correctly",
    "did you say",
    "did you mean",
    "say that again",
    "say it again",
    "could you repeat",
    "can you repeat",
    "nghe nh\u{1ea7}m",
    "nghe ch\u{1b0}a r\u{f5}",
    "nghe kh\u{f4}ng r\u{f5}",
    "ch\u{1b0}a nghe r\u{f5}",
    "c\u{f3} ph\u{1ea3}i b\u{1ea1}n n\u{f3}i",
    "n\u{f3}i l\u{1ea1}i",
];

/// Whether a round's own words doubt what was heard ([`DOUBTS`]).
pub fn doubts_what_was_heard(said: &str) -> bool {
    let said: String = said
        .nfc()
        .flat_map(char::to_lowercase)
        .map(|c| if c == '\u{2019}' { '\'' } else { c })
        .collect();
    DOUBTS.iter().any(|doubt| said.contains(doubt))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// P5's words, and the other ways a reply doubts, EN and VI.
    #[test]
    fn a_reply_that_doubts_what_was_heard_is_seen() {
        for said in [
            "Kept: your favorite flower is \"the two.\" I may have misheard that — was it tulip?",
            "Did you say tulip or the two?",
            "I didn\u{2019}t catch the name of the flower.",
            "If I heard you right, your locker is 12.",
            "Could you repeat the flower's name?",
            "Mình sợ nghe nhầm, bạn nói lại tên hoa được không?",
            "Có phải bạn nói hoa tulip?",
        ] {
            assert!(doubts_what_was_heard(said), "{said}");
        }
    }

    /// A plain say-back, an empty round and a question about something
    /// else are not doubts: the keep runs.
    #[test]
    fn a_plain_reply_is_no_doubt() {
        for said in [
            "",
            "Kept: your favourite flower is the tulip.",
            "Got it, sunflower now, not the two.",
            "Đã nhớ: hoa yêu thích của bạn là hoa tulip.",
            "Noted. Anything else?",
        ] {
            assert!(!doubts_what_was_heard(said), "{said}");
        }
    }

    #[test]
    fn the_note_never_says_kept() {
        assert!(NOT_KEPT.contains("Never say it was kept"));
        assert!(!SKIPPED.contains("tulip"));
    }
}
