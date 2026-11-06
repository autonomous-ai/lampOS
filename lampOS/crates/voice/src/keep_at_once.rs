//! A plain keep answered at once (`LAMP_KEEP_ANSWERS_AT_ONCE`, C59).
//!
//! On lamp-06c1 on 5 Nov (the batch-100 runs, 70 keep turns) a turn that
//! kept something took 3804 ms at the median from the words heard to the
//! answer's first audio, against 1746 ms for a turn with no call. The
//! memory answered in 39 ms; the cost was a second chat round: round 1
//! streams only the `remember` call and says nothing, and the first words
//! come from round 2, about 1.4 s later.
//!
//! With the switch on, on the second route, a spoken line that only asks to
//! keep something ([`only_keeps`]: every sentence of it a plain ask to keep,
//! `remember::asked::asks_to_keep`, or a word of thanks or a greeting, and
//! no question or further ask in it) whose round 1 called `remember` alone,
//! with no words of its own, is answered with [`KEPT`] as soon as every
//! call came back kept plainly ([`kept_plainly`]), and the turn ends there:
//! no second round. Anything else goes on exactly as without the switch:
//! a keep that failed, was refused, held for a yes, doubted, or told with a
//! note for the model to say ("Already kept", a correction, a replaced
//! one's words); other calls beside it; the model's own words; a line that
//! asks more. So a failed keep is never said kept: its round 2 is told the
//! failure, as today. The privacy gates (the speaker seen, the scope) run
//! before the call as they always do: the line is said only of an answer
//! the memory gave.
//!
//! With an owner's language chosen, [`KEPT`] is said in it
//! (`language::FIXED_LINES`) only from its clip rendered ahead as the
//! worker starts, as the wordless cut-in lines are; until that clip is
//! kept, the turn waits for round 2 as without the switch.
//!
//! Off, nothing changes. On, a turn does a few word searches, no I/O.
use serde_json::Value;

/// Only the exact value `on`.
pub const ENVIRONMENT: &str = "LAMP_KEEP_ANSWERS_AT_ONCE";

/// What the lamp says for a plain keep, at once.
pub const KEPT: &str = "Okay, I'll remember that.";

/// Words that ask nothing alone: thanks, a greeting, the name.
const FILLER: &[&str] = &[
    "thanks", "thank", "you", "please", "okay", "ok", "cheers", "lamp", "hey", "hi", "hello",
    "great", "cool", "so", "oh", "much", "very", "alright", "right",
];

/// What ends a clause inside a sentence: past the keep, any words after
/// one but thanks, a greeting or the name are a second clause, whatever
/// they are ("…, and I'd love a joke", "…, so what's the weather like"),
/// and go to round 2 (the Reviewer-lead's P2 on #5594). A spaced dash
/// (" - ") counts too; a dash inside a word does not.
const CLAUSE_ENDS: &[char] = &[
    ',', ':', '\u{2013}', '\u{2014}', '\u{FF0C}', '\u{3001}', '\u{FF1A}',
];

/// After a joining word, the start of a further ask ("…, and what's the
/// weather", "…, then set a timer") or of a new clause ("… and I'd love a
/// joke", "… and let me know the weather").
const JOINING: &[&str] = &["and", "also", "then", "plus", "but", "so", "or"];
/// Words that start a new clause after a joining word with no comma.
const NEW_CLAUSE: &[&str] = &[
    "i", "i'd", "i'm", "i'll", "i've", "you", "you'd", "you're", "we", "we'd", "we're", "we'll",
    "let", "let's", "please", "it's", "there's", "that's", "is", "are", "do", "does", "don't",
    "say", "sing", "help", "send", "call", "add", "ask", "need", "want", "go", "get",
];
const ASKING: &[&str] = &[
    "what", "what's", "whats", "how", "how's", "when", "where", "who", "who's", "why", "which",
    "can", "could", "will", "would", "tell", "set", "turn", "make", "play", "remind", "show",
    "look", "check", "find", "give", "read", "start", "stop", "search", "dim", "brighten",
];

/// Whether the switch is on in this process.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// Whether `said` asks only to keep something: at least one sentence is a
/// plain ask to keep, each other sentence holds only thanks, a greeting or
/// the name, none is a question, no clause after the keep's own holds more
/// than those ([`CLAUSE_ENDS`]), and no joining word starts a further ask
/// or a new clause. Leans to "no": a dropped answer is the bad failure; a
/// keep sent to round 2 only costs the round.
pub fn only_keeps(said: &str) -> bool {
    let said = said.replace('\u{2019}', "'").replace(" - ", ", ");
    if said.contains(['?', '\u{FF1F}']) || !crate::remember::asked::asks_to_keep(&said) {
        return false;
    }
    let filler = |words: &[String]| words.iter().all(|word| FILLER.contains(&word.as_str()));
    for sentence in said.split_inclusive(['.', '!', ';', '\u{3002}', '\u{FF01}', '\u{FF1B}']) {
        if filler(&crate::known_faces::lower_words(sentence)) {
            continue;
        }
        if !crate::remember::asked::asks_to_keep(sentence) {
            return false;
        }
        // One clause is the keep; every other is thanks, a greeting or the
        // name ("Hey lamp, remember…", "…, thanks").
        let mut kept = false;
        for clause in sentence.split(CLAUSE_ENDS) {
            let words = crate::known_faces::lower_words(clause);
            if filler(&words) {
                continue;
            }
            if kept || !crate::remember::asked::asks_to_keep(clause) {
                return false;
            }
            kept = true;
            if words.windows(2).any(|pair| {
                JOINING.contains(&pair[0].as_str())
                    && (ASKING.contains(&pair[1].as_str())
                        || NEW_CLAUSE.contains(&pair[1].as_str()))
            }) {
                return false;
            }
        }
    }
    true
}

/// The words that start an ask to keep, as `remember::asked` reads them:
/// the first run of one of these in the heard line is its lead-in, not
/// what was asked to keep.
const LEAD_INS: &[&[&str]] = &[
    &["don't", "forget"],
    &["dont", "forget"],
    &["do", "not", "forget"],
    &["keep", "in", "mind"],
    &["make", "a", "note"],
    &["remember"],
    &["note"],
    &["hãy", "nhớ"],
    &["nhớ"],
];

/// Words that keep nothing, besides the memory's common words
/// (`remember::SEARCH_COMMON`) and [`FILLER`]: a word's piece past an
/// apostrophe ("I'm", "we'll"), the person told of in the third person
/// ("The user likes tea"), and "also" or "too".
const KEEPS_NOTHING: &[&str] = &[
    "im", "ive", "ll", "ve", "re", "don", "myself", "user", "person", "also", "too", "an", "rằng",
    "giúp",
];

/// A text's words as the coverage compares them ([`covers`]): lowercased
/// and folded as the memory's search folds them (`remember::about_fold`:
/// NFC, `đ` as `d`, no accents), split at anything not a letter or digit,
/// a possessive's `'s` and a plural's `s` taken off (`remember::plural_off`),
/// one letter alone dropped unless a digit.
fn compared(words: &[String]) -> Vec<String> {
    words
        .iter()
        .flat_map(|word| {
            let word = word.strip_suffix("'s").unwrap_or(word);
            crate::remember::about_fold(word)
                .split(|c: char| !c.is_alphanumeric())
                .filter(|piece| {
                    piece.chars().count() >= 2 || piece.chars().any(|c| c.is_ascii_digit())
                })
                .map(|piece| crate::remember::plural_off(piece).to_owned())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Whether the texts the `remember` call(s) kept cover everything the
/// heard line asked to keep (the Reviewer-lead's 09:56 P2 on #5594): every
/// content word of `said` past its lead-in ([`LEAD_INS`]) is among the
/// kept texts' words, compared as [`compared`] folds them. Thanks, a
/// greeting, the name ([`FILLER`]), the memory's common words and the
/// person's own words ([`KEEPS_NOTHING`]) are not content, so a keep told
/// in the third person ("I like tea" kept "Likes tea.") stays covered. A
/// word the call did not keep ("… and explain gravity to me", "… and wish
/// me luck") is a further ask, or a keep cut short: either way round 2
/// answers it. Needs no word list of asks.
pub fn covers(said: &str, kept: &[&str]) -> bool {
    let mut heard = crate::known_faces::lower_words(said);
    if let Some((at, len)) = (0..heard.len()).find_map(|at| {
        LEAD_INS.iter().find_map(|lead| {
            heard[at..]
                .starts_with(
                    &lead
                        .iter()
                        .map(|word| (*word).to_owned())
                        .collect::<Vec<_>>(),
                )
                .then_some((at, lead.len()))
        })
    }) {
        heard.drain(at..at + len);
    }
    let kept: Vec<String> = kept
        .iter()
        .flat_map(|text| compared(&crate::known_faces::lower_words(text)))
        .collect();
    let nothing: Vec<String> = crate::remember::SEARCH_COMMON
        .iter()
        .chain(FILLER)
        .chain(KEEPS_NOTHING)
        .map(|word| crate::remember::about_fold(word))
        .collect();
    let content: Vec<String> = compared(&heard)
        .into_iter()
        .filter(|word| !nothing.contains(word))
        .collect();
    !content.is_empty() && content.iter().all(|word| kept.contains(word))
}

/// Whether one `remember` answer is a plain keep the lamp may confirm
/// itself: kept, with nothing for the model to say (no note, no correction
/// or `replaced` one, whose round 2 says what changed, nothing held, no
/// error; Thirteen's P3 on #5565). A keep for them alone is plain unless the line
/// asked to keep it for everyone, which its round 2 must answer.
pub fn kept_plainly(told: &Value, said: &str) -> bool {
    let Some(fields) = told.as_object() else {
        return false;
    };
    if told["remembered"] != true {
        return false;
    }
    let for_everyone = || {
        crate::known_faces::lower_words(said).iter().any(|word| {
            matches!(
                word.as_str(),
                "everyone" | "everybody" | "room" | "all" | "family" | "us"
            )
        })
    };
    fields.keys().all(|key| match key.as_str() {
        "remembered" | "id" => true,
        "kept_for" => !for_everyone(),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_plain_ask_to_keep_is_only_a_keep() {
        for said in [
            "Remember that my dentist is on Tuesday.",
            "Lamp, remember that I take my coffee black.",
            "Hey lamp. Remember my sister's name is Mai. Thanks!",
            "Please remember that the spare key is under the blue pot.",
            "Don't forget that I have yoga at six and dinner at eight.",
            // The latency win stays for a plain keep with "and" inside it.
            "Remember that I like tea and coffee",
            "Remember my locker code is 441",
            "Hey lamp, remember that I like tea. Thanks, lamp!",
        ] {
            assert!(only_keeps(said), "{said}");
        }
    }

    #[test]
    fn a_line_with_more_in_it_is_not_only_a_keep() {
        for said in [
            "Remember that my dentist is on Tuesday. What's the weather?",
            "Remember that my dentist is on Tuesday, and set a timer for five minutes.",
            "Remember that I like tea. Tell me a joke.",
            "Can you remember what I said about the dentist?",
            "Remember that I like tea, and what time is it",
            "My dentist is on Tuesday.",
            "Don't remember that.",
            "Remember that I like tea, but never mind.",
            // The Reviewer-lead's P2 on #5594: any clause after the keep,
            // whatever its words, goes to round 2.
            "Remember that I like tea, and I'd love a joke.",
            "Remember that I like tea, so what's the weather like",
            "Remember that I like tea, and let me know the weather.",
            "Remember that I like tea and I'd love a joke.",
            "Remember that I like tea - and a joke please",
            "Remember that I like tea; also the news.",
        ] {
            assert!(!only_keeps(said), "{said}");
        }
    }

    #[test]
    fn only_a_plain_kept_answer_is_kept_plainly() {
        let said = "Remember that I like tea.";
        for told in [
            json!({"remembered": true}),
            json!({"remembered": true, "id": 4}),
            json!({"remembered": true, "kept_for": "Kept for them alone"}),
        ] {
            assert!(kept_plainly(&told, said), "{told}");
        }
        for told in [
            json!({"remembered": false, "note": "Not kept."}),
            json!({"error": "the memory is not reachable"}),
            json!({"remembered": true, "note": "Already kept: tell them you already knew it."}),
            json!({"remembered": true, "held": true}),
            json!({"remembered": true, "not_replaced": 3}),
            // A correction (Thirteen's P3 on #5565): round 2 says what changed.
            json!({"remembered": true, "replaced": 3}),
            json!({"ok": true}),
            json!("kept"),
        ] {
            assert!(!kept_plainly(&told, said), "{told}");
        }
        assert!(!kept_plainly(
            &json!({"remembered": true, "kept_for": "Kept for them alone"}),
            "Remember for everyone that dinner is at eight."
        ));
    }

    /// The heard line answered at once for round 1's kept `kept` texts.
    fn at_once(said: &str, kept: &[&str]) -> bool {
        only_keeps(said) && covers(said, kept)
    }

    #[test]
    fn a_further_ask_without_a_comma_goes_to_round_2() {
        // The Reviewer-lead's 09:56 P2 on #5594: no clause break, and no
        // word of the lists after the joining word.
        for said in [
            "Remember that I like tea and explain gravity to me",
            "Remember that I like tea and recommend a book",
            "Remember that I like tea and wish me luck",
            "Remember that I like tea then explain gravity",
            "Remember that I like tea and also a joke",
            "Remember that I like tea and a joke would be nice",
        ] {
            for kept in ["I like tea", "Likes tea.", "The user likes tea."] {
                assert!(!at_once(said, &[kept]), "{said} / {kept}");
            }
        }
    }

    #[test]
    fn a_keep_its_call_kept_whole_is_answered_at_once() {
        for (said, kept) in [
            ("Remember that I like tea. Thanks!", "Likes tea."),
            (
                "Remember that I like tea and coffee",
                "Likes tea and coffee.",
            ),
            (
                "Remember that my kids are Ann and Bob",
                "Their kids are Ann and Bob.",
            ),
            (
                "Hey lamp, remember that I like tea. Thanks, lamp!",
                "I like tea.",
            ),
            (
                "Remember that my dentist is on Tuesday.",
                "Their dentist is on Tuesday.",
            ),
            // Paraphrased: the person shifted, "like" kept as "likes", the
            // possessive dropped.
            ("Remember that I like tea", "The user likes tea."),
            (
                "Please remember that my sister's name is Mai.",
                "Sister's name is Mai.",
            ),
            (
                "Lamp, remember that I take my coffee black.",
                "Takes coffee black.",
            ),
        ] {
            assert!(at_once(said, &[kept]), "{said} / {kept}");
        }
        // Two keeps cover the line between them.
        assert!(at_once(
            "Remember that I like tea and my kids are Ann and Bob",
            &["Likes tea.", "Kids are Ann and Bob."]
        ));
    }

    #[test]
    fn a_word_the_call_did_not_keep_goes_to_round_2() {
        for (said, kept) in [
            ("Remember that I like tea and coffee", "Likes tea."),
            (
                "Remember that my kids are Ann and Bob",
                "Their kids are Ann.",
            ),
            ("Remember that my dentist is on Tuesday.", ""),
        ] {
            assert!(!at_once(said, &[kept]), "{said} / {kept}");
        }
        assert!(!at_once("Remember that I like tea", &[]));
        // The reviewer's lines that already go to round 2 still do.
        for said in [
            "Remember that I'm vegan, also I want pasta",
            "Remember that I like tea; play some jazz",
        ] {
            assert!(!at_once(said, &["Is vegan."]), "{said}");
        }
    }
}
