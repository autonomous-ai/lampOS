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
    // lamp-06c1, 5 Nov (C37): "I want to be sure" after a keep, only of
    // what was heard: "I want to be sure you make it" is reassurance.
    "want to be sure i heard",
    "want to be sure i caught",
    "is that spelled",
    // Of the word: "How do you spell relief?" is a joke, no doubt.
    "how do you spell it",
    "how do you spell that",
    "how do you spell your name",
    "how do you spell the name",
    "nghe nh\u{1ea7}m",
    "nghe ch\u{1b0}a r\u{f5}",
    "nghe kh\u{f4}ng r\u{f5}",
    "ch\u{1b0}a nghe r\u{f5}",
    "c\u{f3} ph\u{1ea3}i b\u{1ea1}n n\u{f3}i",
];

/// A doubt only where its sentence or clause ends right after it, or
/// "right"/"correct" follows: "I want to be sure I have it." doubts, "I
/// want to be sure I have it saved for Monday" does not.
const DOUBTS_ENDED: &[&str] = &[
    "want to be sure i have it",
    "want to be sure i have that",
    "want to be sure i got it",
    "want to be sure i got that",
    "want to be sure i've got it",
    "want to be sure i've got that",
];

/// An ask to repeat, a doubt unless [`asks_for_their_words`] finds its
/// sentence a keep's closing line: an invitation for later ("If it
/// changes, just say it again.", "Bạn cứ nói lại nhé"), the lamp saying
/// it itself ("I'll say it again at seven."), or a new topic ("Did you say
/// you also wanted a reminder?"). "Please say it again.", "Could you just
/// say it again?" and "Bạn nói lại tên hoa nhé?" stay doubts (#5454).
const ASKED_DOUBTS: &[&str] = &[
    "say that again",
    "say it again",
    "n\u{f3}i l\u{1ea1}i",
    "did you say",
    "can you repeat",
    "could you repeat",
];

/// Of what was heard, said back: a doubt unless its sentence ends with
/// '!' ("If I heard you right, your locker is 12." doubts; "If I heard you
/// right, you're off to Hanoi, have fun!" is a send-off).
const HEARD_BACK: &[&str] = &[
    "if i heard you right",
    "if i heard that right",
    "if i heard correctly",
];

/// Words anywhere in its sentence that make an [`ASKED_DOUBTS`] an
/// invitation for later, or no need.
const INVITES: &[&str] = &[
    "anytime",
    "any time",
    "whenever",
    "if anything changes",
    "if it changes",
    "if you ever",
    "feel free",
    "you can",
    "next time",
    "never",
    "no need",
    "needn't",
    "don't need",
    "don't have to",
    "won't need",
    "won't have to",
    "do not need",
    "do not have to",
    "c\u{1ee9}",
    "n\u{1ebf}u thay \u{111}\u{1ed5}i",
    "kh\u{f4}ng c\u{1ea7}n",
    "kh\u{1ecf}i",
    "ch\u{1eb3}ng c\u{1ea7}n",
];

/// Words before an [`ASKED_DOUBTS`] in its sentence that make the lamp
/// the one saying it.
const LAMP_SAYS: &[&str] = &[
    "i'll",
    "i will",
    "me to",
    "should i",
    "shall i",
    "m\u{ec}nh s\u{1ebd}",
    "\u{111}\u{1ec3} m\u{ec}nh",
];

/// First words after "say it/that again" that make it the lamp's own
/// later saying: "… say it again at seven", "… tomorrow morning".
const LATER: &[&str] = &["at", "tomorrow", "tonight", "later", "every"];

/// First words after an [`ASKED_DOUBTS`] that open a new topic: "Did you
/// say she's visiting soon?", "Did you say anything else?".
const NEW_TOPIC: &[&str] = &[
    "i",
    "i'm",
    "you",
    "you're",
    "he",
    "he's",
    "she",
    "she's",
    "they",
    "they're",
    "we",
    "we're",
    "it's",
    "anything",
    "something",
];

/// Ends of a sentence.
const SENTENCE_ENDS: [char; 4] = ['.', '!', '?', '\n'];

/// Whether `words` (lower case) has `marker` as whole words.
fn has_words(words: &str, marker: &str) -> bool {
    words.match_indices(marker).any(|(at, _)| {
        let before = words[..at].chars().next_back();
        let next = words[at + marker.len()..].chars().next();
        before.is_none_or(|c| !c.is_alphanumeric() && c != '\'')
            && next.is_none_or(|c| !c.is_alphanumeric() && c != '\'')
    })
}

/// Whether the [`ASKED_DOUBTS`] phrase `ask` at byte `at` of `said` (lower
/// case) asks the person for their own words: its sentence has none of
/// [`INVITES`], none of [`LAMP_SAYS`] before it, and what follows neither
/// opens with [`NEW_TOPIC`] (or [`LATER`] after "again") nor sends it on
/// ("to your sister").
fn asks_for_their_words(said: &str, at: usize, ask: &str) -> bool {
    let before = &said[..at];
    let from = before.rfind(SENTENCE_ENDS).map_or(0, |cut| cut + 1);
    let after = &said[at + ask.len()..];
    let rest = &after[..after.find(SENTENCE_ENDS).unwrap_or(after.len())];
    let sentence = &said[from..at + ask.len() + rest.len()];
    let first = rest
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .find(|word| !word.is_empty());
    let later = ask.ends_with("again") && first.is_some_and(|word| LATER.contains(&word));
    !INVITES.iter().any(|marker| has_words(sentence, marker))
        && !LAMP_SAYS
            .iter()
            .any(|marker| has_words(&said[from..at], marker))
        && !first.is_some_and(|word| NEW_TOPIC.contains(&word))
        && !later
        && ![
            "to your", "to my", "to her", "to his", "to them", "to him", "to our",
        ]
        .iter()
        .any(|sent_on| has_words(rest, sent_on))
}

/// Whether the [`HEARD_BACK`] phrase ending at byte `end` of `said` is in a
/// sentence that does not end with '!'.
fn heard_back(said: &str, end: usize) -> bool {
    let after = &said[end..];
    !after
        .find(SENTENCE_ENDS)
        .is_some_and(|stop| after[stop..].starts_with('!'))
}

/// Whether `said` (lower case) has one of [`DOUBTS_ENDED`], one of
/// [`ASKED_DOUBTS`] that [`asks_for_their_words`], a [`heard_back`], or "did you mean" that is not "did you
/// mean to" ("Did you mean to add a reminder too?") unless "to say".
fn doubts_in_place(said: &str) -> bool {
    let ended = DOUBTS_ENDED.iter().any(|doubt| {
        said.match_indices(doubt).any(|(at, _)| {
            let after = &said[at + doubt.len()..];
            after.chars().next().is_none_or(|c| ".!?,;:\n".contains(c))
                || after.starts_with(" right")
                || after.starts_with(" correct")
        })
    });
    let asked = ASKED_DOUBTS.iter().any(|ask| {
        said.match_indices(ask)
            .any(|(at, _)| asks_for_their_words(said, at, ask))
    }) || HEARD_BACK.iter().any(|back| {
        said.match_indices(back)
            .any(|(at, _)| heard_back(said, at + back.len()))
    });
    let meant = said.match_indices("did you mean").any(|(at, head)| {
        let after = &said[at + head.len()..];
        !after.starts_with(" to ") || after.starts_with(" to say")
    });
    ended || asked || meant
}

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
                    // "Was that all right?" asks if keeping it was fine.
                    && !window[..found].ends_with("all ")
            })
        })
    })
}

/// Whether a round's own words doubt what was heard ([`DOUBTS`], the ones
/// [`doubts_in_place`] reads, or [`DOUBTS_AROUND`] with the word between).
pub fn doubts_what_was_heard(said: &str) -> bool {
    let said: String = said
        .nfc()
        .flat_map(char::to_lowercase)
        .map(|c| if c == '\u{2019}' { '\'' } else { c })
        .collect();
    DOUBTS.iter().any(|doubt| said.contains(doubt))
        || doubts_in_place(&said)
        || doubts_around(&said)
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

    /// #5414 P2: reassurance after a keep, and other plain keep replies
    /// that hold a doubt's words, are no doubt: read as one, the keep
    /// would be forgotten and held for a yes that never comes.
    #[test]
    fn reassurance_after_a_keep_is_no_doubt() {
        for said in [
            "Kept your flight time. I want to be sure you make it.",
            "Got it. I want to be sure you don't miss it.",
            "How do you spell relief? A nap.",
            "Kept your locker number. I want to be sure it's there when you need it.",
            "Kept. If you want to be sure, ask me tomorrow.",
            "Saved. I want to be sure I have it ready for Monday.",
            "Kept. You never have to say it again.",
            "No need to say that again \u{2014} I've got it.",
            "Kept. Did you mean to add a reminder too?",
            "Kept your dentist time. Was that all right?",
            "\u{110}\u{e3} nh\u{1edb} r\u{1ed3}i, b\u{1ea1}n kh\u{f4}ng c\u{1ea7}n n\u{f3}i l\u{1ea1}i \u{111}\u{e2}u.",
            // #5454, the second read: a keep's closing line.
            "Kept. If it changes, just say it again.",
            "Got it. If you ever forget, just say it again and I'll tell you.",
            "Kept. You can say it again anytime to update it.",
            "Saved. Want me to say it again tomorrow morning?",
            "Kept. I'll say it again at seven.",
            "\u{110}\u{e3} nh\u{1edb} r\u{1ed3}i. N\u{1ebf}u thay \u{111}\u{1ed5}i th\u{ec} n\u{f3}i l\u{1ea1}i v\u{1edb}i m\u{ec}nh nh\u{e9}.",
            "Kept. If I heard you right, you're off to Hanoi, have fun!",
            "Kept: your sister's name is Mai. Did you say she's visiting soon?",
            "Kept. Did you say you also wanted a reminder?",
            "Got it. Can you repeat that to your sister too? She'll want to know.",
            "Kept. How do you spell your excitement? Probably with exclamation marks!",
            "Saved. Tell me again if it changes.",
            "Kept, and I'll remind you. No need to repeat it.",
            "Saved. Should I say it again before you leave?",
            "Kept. Did you say anything else you'd like me to keep?",
            "Got it. Tomorrow I'll say it again when you wake up.",
            "Kept. Next time, just say it again and I'll update it?",
            "\u{110}\u{e3} nh\u{1edb}. Mai m\u{ec}nh s\u{1ebd} n\u{f3}i l\u{1ea1}i cho b\u{1ea1}n nh\u{e9}?",
            // #5454, the Reviewer-lead's read: invitations.
            "Saved \u{2014} say it again anytime you want it changed.",
            "Kept. Just say it again if anything changes.",
            "If it changes, you can say that again.",
            "Feel free to say it again whenever.",
            "B\u{1ea1}n c\u{1ee9} n\u{f3}i l\u{1ea1}i nh\u{e9}",
        ] {
            assert!(!doubts_what_was_heard(said), "{said}");
        }
    }

    /// The narrower doubts that stay: of what was heard, the word, or an
    /// ask to say it again.
    #[test]
    fn the_narrower_doubts_are_still_seen() {
        for said in [
            "I want to be sure I heard the name.",
            "I want to be sure I caught that.",
            "I want to be sure I got that right.",
            "I want to be sure I have it?",
            "How do you spell that?",
            "How do you spell your name?",
            "Sorry, could you say that again?",
            "I'm not sure, say it again?",
            "I didn't get that, so could you say it again?",
            "Sorry, could you say it again?",
            "Did you say Mai or May?",
            "What did you say?",
            "Can you repeat the name?",
            "If I heard you right, that's Mai?",
            "B\u{1ea1}n n\u{f3}i l\u{1ea1}i \u{111}\u{1b0}\u{1ee3}c kh\u{f4}ng?",
            "Did you mean today or Tuesday?",
            "Did you mean to say tulip?",
            // #5454, the Reviewer-lead's read: real asks with no '?'.
            "M\u{00ec}nh nghe ch\u{1b0}a k\u{1ef9}, b\u{1ea1}n n\u{f3}i l\u{1ea1}i nh\u{e9}?",
            "B\u{1ea1}n n\u{f3}i l\u{1ea1}i t\u{ea}n hoa nh\u{e9}?",
            "I didn't get that, so please say it again.",
            "Please say it again.",
            "Could you just say it again?",
            "Sorry \u{2014} if you could say it again, I'll keep it.",
            "If I heard you right, your locker is 12.",
        ] {
            assert!(doubts_what_was_heard(said), "{said}");
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
