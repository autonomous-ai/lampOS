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
//!
//! With [`UNDOES_ENVIRONMENT`] (C37, lamp-06c1, 5 Nov) a doubt in the round
//! right after a keep counts too: round 1 was a bare `remember` with no
//! words, and round 2 said "Was that 'rhinodulus' right — I want to be
//! sure", with the word already kept. The second route forgets that new
//! keep by its id, in the same scope, and holds the call for the same
//! person's yes, which keeps it again (`second_provider::undo_doubted_keep`).
use serde_json::Value;
use std::time::Duration;
use unicode_normalization::UnicodeNormalization;

/// The switch, in the voice service's environment: `on` only.
pub const UNDOES_ENVIRONMENT: &str = "LAMP_MEMORY_DOUBT_UNDOES";

/// Whether [`UNDOES_ENVIRONMENT`] is `on`.
pub fn undoes_on() -> bool {
    std::env::var(UNDOES_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// A doubt takes back only a keep at most this old: rounds of one answer
/// are seconds apart.
pub const UNDO_WITHIN: Duration = Duration::from_secs(30);

/// What the model is told before its next round when a doubt took back
/// the keep just before ([`UNDOES_ENVIRONMENT`]).
pub const UNDONE_NOTE: &str = "(For you to know: what you kept just before is taken back, since your words doubt what you heard. It is kept again only when they say yes to your question; if they say it differently, keep what they say then. Never say it is kept.)";

/// The id of a `remember`'s answer as the model was told it, when it kept
/// something new: `remembered`, with an `id` (`LAMP_MEMORY_REPLACES`), and
/// no note (kept already), `replaced`, `not_replaced` or `held`. Only such
/// a keep is taken back: forgetting one kept before, or one that replaced
/// another, would lose what the person said earlier.
pub fn new_keep_id(told: &Value) -> Option<u64> {
    if told["remembered"] != true
        || ["note", "replaced", "not_replaced", "held", "error"]
            .iter()
            .any(|key| told.get(key).is_some())
    {
        return None;
    }
    told["id"].as_u64()
}

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
    // lamp-06c1, 5 Nov (C37): "I want to be sure" after a keep.
    "want to be sure",
    "is that spelled",
    "how do you spell",
    "nghe nh\u{1ea7}m",
    "nghe ch\u{1b0}a r\u{f5}",
    "nghe kh\u{f4}ng r\u{f5}",
    "ch\u{1b0}a nghe r\u{f5}",
    "c\u{f3} ph\u{1ea3}i b\u{1ea1}n n\u{f3}i",
    "n\u{f3}i l\u{1ea1}i",
];

/// A doubt in two parts with the doubted word between them, lower case:
/// "Was that 'rhinodulus' right?" (lamp-06c1, 5 Nov, C37), "Did I hear
/// the name right?". The first part opens a sentence or a clause (the
/// reply's start, or after '.', '!', '?', ',', ';', ':' or a dash), so
/// "That was that, right on time" is none; the second is the word "right"
/// within [`GAP_MOST`] bytes after it and before the sentence ends ('?',
/// '!' or a line end), so "was that alright" is none either.
const DOUBTS_AROUND: &[(&str, &str)] = &[
    ("was that", "right"),
    ("did i hear", "right"),
    ("did i get", "right"),
    ("have i got", "right"),
];

/// How far after a two-part doubt's first part its "right" may come.
const GAP_MOST: usize = 60;

/// Whether `said` (lower case) has one of [`DOUBTS_AROUND`].
fn doubts_around(said: &str) -> bool {
    DOUBTS_AROUND.iter().any(|(head, tail)| {
        said.match_indices(head).any(|(at, _)| {
            let opens = said[..at]
                .trim_end_matches([' ', '"', '\'', '\u{201c}'])
                .chars()
                .next_back()
                .is_none_or(|c| ".!?,;:\u{2014}\u{2013}-".contains(c));
            let after = &said[at + head.len()..];
            if !opens || !after.starts_with(' ') {
                return false;
            }
            let mut end = after.len().min(GAP_MOST);
            while !after.is_char_boundary(end) {
                end -= 1;
            }
            let window = &after[..end];
            let window = &window[..window.find(['?', '!', '\n']).unwrap_or(window.len())];
            window.match_indices(tail).any(|(found, _)| {
                let before = window[..found].chars().next_back();
                let next = window[found + tail.len()..].chars().next();
                before.is_some_and(|c| !c.is_alphanumeric())
                    && next.is_none_or(|c| !c.is_alphanumeric())
            })
        })
    })
}

/// Whether a round's own words doubt what was heard ([`DOUBTS`], or
/// [`DOUBTS_AROUND`] with the word between).
pub fn doubts_what_was_heard(said: &str) -> bool {
    let said: String = said
        .nfc()
        .flat_map(char::to_lowercase)
        .map(|c| if c == '\u{2019}' { '\'' } else { c })
        .collect();
    DOUBTS.iter().any(|doubt| said.contains(doubt)) || doubts_around(&said)
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

    /// lamp-06c1, 5 Nov (C37): the doubt came in the round after the keep,
    /// in words the list did not have.
    #[test]
    fn the_doubts_seen_after_a_keep_are_seen() {
        for said in [
            "Was that \u{2018}rhinodulus\u{2019} right \u{2014} I want to be sure.",
            "Kept. Was that 'rhinodulus' right?",
            "Was that right?",
            "Got it \u{2014} was that \"julie\" right?",
            "Okay, was that ranunculus right?",
            "Did I hear that right?",
            "Did I hear the name right? Rhinodulus?",
            "Did I get that right?",
            "Have I got the spelling right?",
            "I want to be sure I have it.",
            "Is that spelled R-H-I-N-O?",
            "How is that spelled?",
            "How do you spell it?",
            "\"Was that right?\"",
        ] {
            assert!(doubts_what_was_heard(said), "{said}");
        }
    }

    /// The same words in a plain reply are no doubt: "right" far away, in
    /// another sentence, inside a longer word, or "was that" mid-sentence.
    #[test]
    fn near_misses_of_the_new_doubts_are_none() {
        for said in [
            "That was that, right on time.",
            "It was that alright.",
            "Was that the day you went to Hanoi? Right, the trip.",
            "Was that before or after lunch? You're right about tea.",
            "Was that fun? Rightly so.",
            "Was that a long trip for you? I hope the drive was nice and the weather stayed calm all the way, right?",
            "Turn right at the corner, that was that.",
            "Kept: your favorite flower is the ranunculus.",
            "Your bike lock code is three one five, right.",
            "I'm sure you'll love it.",
            "Make sure you have your keys.",
            // A keep's own round may say this: no doubt of a word.
            "Let me make sure I have that saved.",
            "I'll make sure I remember it.",
            "Did I tell you about the weather?",
            "The heard word was that one.",
            "Did I hearten you? Right.",
            "Was thatch right for the roof?",
        ] {
            assert!(!doubts_what_was_heard(said), "{said}");
        }
    }

    /// Only a new keep is taken back: never one kept already, one that
    /// replaced another, one held, or one with no id.
    #[test]
    fn only_a_new_keep_has_an_id_to_take_back() {
        use serde_json::json;
        assert_eq!(
            new_keep_id(&json!({"remembered": true, "id": 391})),
            Some(391)
        );
        assert_eq!(
            new_keep_id(&json!({"remembered": true, "id": 391, "kept_for": "x"})),
            Some(391)
        );
        for told in [
            json!({"remembered": true}),
            json!({"remembered": true, "id": 4, "note": "Already kept: tell them you already knew it."}),
            json!({"remembered": true, "id": 5, "replaced": 4}),
            json!({"remembered": true, "id": 5, "not_replaced": 4}),
            json!({"remembered": false, "id": 5}),
            json!({"remembered": false, "held": true, "id": 5}),
            json!({"remembered": true, "id": 5, "error": "x"}),
            json!({"remembered": true, "id": "5"}),
            json!({"error": "x"}),
        ] {
            assert_eq!(new_keep_id(&told), None, "{told}");
        }
    }

    #[test]
    fn the_undone_note_never_says_kept() {
        assert!(UNDONE_NOTE.contains("Never say it is kept"));
        assert_eq!(UNDOES_ENVIRONMENT, "LAMP_MEMORY_DOUBT_UNDOES");
    }

    #[test]
    fn the_note_never_says_kept() {
        assert!(NOT_KEPT.contains("Never say it was kept"));
        assert!(!SKIPPED.contains("tulip"));
    }
}
