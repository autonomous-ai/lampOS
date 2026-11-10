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
//! no question or further ask in it: no "?", no question word or shape in
//! English or Vietnamese, [`asks_a_question`]) whose round 1 called
//! `remember` alone, with no words of its own, and whose kept text holds
//! every content word of the line ([`covers`], a "not" and a single letter
//! among them, with "i" still excluded). A payload containing standalone
//! "a" also needs one kept text with the same ordered content including
//! every "a"; uncertain split or reordered paraphrases take round 2. A
//! covered keep is answered with [`KEPT`] as soon as every
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
//! kept, the turn waits for round 2 as without the switch. The ask to keep
//! is still recognised by its English words only (`asks_to_keep`): a keep
//! asked in another language goes to round 2 as without the switch;
//! recognising it there is a follow-up.
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

/// Words that ask a question wherever they stand in the line, heard with
/// or without a "?" (speech to text often drops it): English and
/// Vietnamese, accents kept as heard (Thirteen's P3 on #5565: "… in the
/// drawer where are they" is a question made only of common words).
const QUESTION_WORDS: &[&str] = &[
    "what", "what's", "whats", "where", "where's", "wheres", "when", "when's", "whens", "who",
    "who's", "whos", "whom", "whose", "why", "how", "how's", "hows", "which", "gì", "đâu", "nào",
    "sao", "ai", "mấy", "hả", "nhỉ",
];
/// A question's opening shape: one of these followed by one of
/// [`ASKED_OF`] ("… do you", "… is it", "… can you").
const QUESTION_OPENS: &[&str] = &[
    "do", "does", "did", "can", "could", "will", "would", "should", "is", "are", "was", "were",
    "am",
];
const ASKED_OF: &[&str] = &["you", "i", "we", "they", "he", "she", "it", "there"];
/// Vietnamese words that ask how long or how much after "bao" ("bao giờ",
/// "bao nhiêu", "bao lâu").
const BAO_ASKS: &[&str] = &["giờ", "nhiêu", "lâu"];
/// Vietnamese particles that ask a yes or no when they end a sentence
/// ("… phải không", "… chưa"); inside one they say "not" or "not yet".
const ASKS_AT_THE_END: &[&str] = &["không", "chưa"];

/// Words that say "not", kept as content by [`covers`] (Thirteen's P3 on
/// #5565: "I don't like tea" kept "Likes tea." must not be confirmed). An
/// "n't" ends a word too ([`not_in`]).
const NOT_WORDS: &[&str] = &[
    "not", "no", "never", "nor", "neither", "none", "nothing", "nobody", "nowhere", "cannot",
    "dont", "doesnt", "didnt", "cant", "wont", "isnt", "arent", "wasnt", "werent", "hasnt",
    "havent", "hadnt", "shouldnt", "wouldnt", "couldnt", "mustnt", "không", "chẳng", "chả", "chưa",
    "đừng",
];

/// Whether `said` asks a question anywhere in it, with or without a "?":
/// a question word ([`QUESTION_WORDS`], "bao giờ"), a question's opening
/// shape ("do you", "is it"), or a Vietnamese "không" or "chưa" ending a
/// sentence. Leans to "yes": a keep sent to round 2 only costs the round.
pub fn asks_a_question(said: &str) -> bool {
    let words = crate::known_faces::lower_words(said);
    let asks = words
        .iter()
        .any(|word| QUESTION_WORDS.contains(&word.as_str()))
        || words.windows(2).any(|pair| {
            (QUESTION_OPENS.contains(&pair[0].as_str()) && ASKED_OF.contains(&pair[1].as_str()))
                || (pair[0] == "bao" && BAO_ASKS.contains(&pair[1].as_str()))
        });
    asks || said
        .split([
            '.', '!', ';', ',', '\u{3002}', '\u{FF01}', '\u{FF1B}', '\u{FF0C}',
        ])
        .filter_map(|sentence| crate::known_faces::lower_words(sentence).pop())
        .any(|last| ASKS_AT_THE_END.contains(&last.as_str()))
}

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
    if said.contains(['?', '\u{FF1F}'])
        || asks_a_question(&said)
        || !crate::remember::asked::asks_to_keep(&said)
    {
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
    "giúp", "i", "a",
];

/// The contractions' tails taken off a word before it is compared: what
/// they add is the person or the tense ("I'm", "I'd", "we'll"), or a
/// possessive. An "n't" says "not" ([`not_in`]).
const TAILS: &[&str] = &["'s", "'m", "'d", "'ll", "'ve", "'re"];

/// A word that says "not" ([`NOT_WORDS`], or one ending "n't"): its stem
/// as the word to compare ("don't" "do", "can't" and "cannot" "can",
/// "won't" "will"), or the word itself when it has none.
fn not_in(word: &str) -> Option<&str> {
    let stem = match word.strip_suffix("n't") {
        Some(stem) => stem,
        None if word == "cannot" => "can",
        None if NOT_WORDS.contains(&word) => match word.strip_suffix("nt") {
            Some(stem) if word.len() > 3 => stem,
            _ => word,
        },
        None => return None,
    };
    Some(match stem {
        "ca" => "can",
        "wo" => "will",
        "sha" => "shall",
        stem => stem,
    })
}

/// A text's words as the coverage compares them ([`covers`]): lowercased
/// and folded as the memory's search folds them (`remember::about_fold`:
/// NFC, `đ` as `d`, no accents), split at anything not a letter or digit,
/// a contraction's tail ([`TAILS`]) and a plural's `s` taken off
/// (`remember::plural_off`). A word that says "not" ([`not_in`]) gives its
/// stem and "not", so "don't", "do not" and "doesn't" compare alike. One
/// letter alone is a word ("blood type O", "row F"; Thirteen's P3 on #5565).
fn compared(words: &[String]) -> Vec<String> {
    words
        .iter()
        .flat_map(|word| {
            let not = not_in(word);
            let word = not.unwrap_or_else(|| {
                TAILS
                    .iter()
                    .find_map(|tail| word.strip_suffix(tail))
                    .unwrap_or(word)
            });
            crate::remember::about_fold(word)
                .split(|c: char| !c.is_alphanumeric())
                .filter(|piece| !piece.is_empty())
                .map(|piece| crate::remember::plural_off(piece).to_owned())
                .chain(not.map(|_| "not".to_owned()))
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
/// answers it. Needs no word list of asks. A standalone "a" also requires
/// one kept text with the same ordered content, retaining every "a": an
/// incidental article cannot cover a requested letter value. Split or
/// reordered such paraphrases defer; "i" remains an existing exclusion.
/// Adding or dropping negation also defers, including contracted spellings.
/// This is conservative lexical coverage, not semantic equivalence: it does
/// not associate a negation with individual clauses in mixed statements.
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
    // A question word, an ask's verb and a "not" are content even where
    // the memory's search counts them common: "… where are they", "… tell
    // me about it" and "I don't like tea" each need the kept text to hold
    // them (Thirteen's P3s on #5565).
    let nothing: Vec<String> = crate::remember::SEARCH_COMMON
        .iter()
        .chain(FILLER)
        .chain(KEEPS_NOTHING)
        .filter(|word| {
            !QUESTION_WORDS.contains(word) && !ASKING.contains(word) && !NOT_WORDS.contains(word)
        })
        .map(|word| crate::remember::about_fold(word))
        .collect();
    let heard = compared(&heard);
    // A standalone "a" may be a value, not an article. Set membership
    // would let an unrelated article cover "blood type A". Preserve its
    // occurrences and order in one kept text, otherwise defer to round 2.
    // This is deliberately stricter for split or reordered paraphrases.
    if heard.iter().any(|word| word == "a") {
        let with_a = |words: Vec<String>| {
            words
                .into_iter()
                .filter(|word| word == "a" || !nothing.contains(word))
                .collect::<Vec<_>>()
        };
        let wanted = with_a(heard.clone());
        if !kept
            .iter()
            .any(|text| with_a(compared(&crate::known_faces::lower_words(text))) == wanted)
        {
            return false;
        }
    }
    let kept: Vec<String> = kept
        .iter()
        .flat_map(|text| compared(&crate::known_faces::lower_words(text)))
        .collect();
    let content: Vec<String> = heard
        .into_iter()
        .filter(|word| !nothing.contains(word))
        .collect();
    // compared emits "not" for every spelling recognized by not_in. A
    // one-way subset check alone would accept a newly negated kept fact.
    if kept.iter().any(|word| word == "not") != content.iter().any(|word| word == "not") {
        return false;
    }
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
    fn a_question_made_only_of_common_words_goes_to_round_2() {
        // Thirteen's P3 on #5565 (and the Reviewer-lead's): heard with no
        // "?", no comma and no joining word, each word of the ask common.
        for said in [
            "Remember my keys are in the drawer where are they",
            "Remember that I like tea how about you",
            "Remember that I like tea what about you",
            "Remember that my dentist is on Tuesday is it",
            "Remember that I like tea do you",
            "Remember that I like tea can you",
            "Remember that I like tea tell me about it",
            "Remember that my dentist is on Tuesday who is it with",
            "Remember that my dentist is on Tuesday when is that",
            // Vietnamese: a question word, or a "không" / "chưa" that ends
            // the line.
            "Nhớ là chìa khóa ở trong ngăn kéo đâu rồi",
            "Nhớ là mình thích trà còn bạn thích gì",
            "Nhớ là mình thích trà bạn có biết không",
            "Nhớ là lịch nha sĩ vào thứ ba phải không.",
        ] {
            // The Vietnamese lines are not read as an ask to keep yet
            // (English ask words only): their question is caught on its own.
            if said.starts_with("Nhớ") {
                assert!(asks_a_question(said), "{said}");
            }
            for kept in [
                "Keys are in the drawer.",
                "Likes tea.",
                "The user likes tea.",
                "Their dentist is on Tuesday.",
                "Chìa khóa ở trong ngăn kéo.",
                "Thích trà.",
                "Lịch nha sĩ vào thứ ba.",
            ] {
                assert!(!at_once(said, &[kept]), "{said} / {kept}");
            }
        }
        // A question word anywhere asks: the line goes to round 2 even when
        // the model kept it whole, and an ask's verb is content.
        assert!(asks_a_question("Remember that I like tea how about you"));
        assert!(asks_a_question("Nhớ là mình thích trà phải không"));
        assert!(!asks_a_question("Remember that I like tea and coffee"));
        assert!(!asks_a_question("Nhớ là mình không ăn cay nhé"));
        assert!(!covers(
            "Remember that I like tea tell me about it",
            &["Likes tea."]
        ));
    }

    #[test]
    fn a_negation_added_to_the_kept_text_goes_to_round_2() {
        for kept in [
            "Doesn't like tea.",
            "Does not like tea.",
            "Doesnt like tea.",
            "Never likes tea.",
        ] {
            assert!(!covers("Remember that I like tea", &[kept]), "{kept}");
        }
        assert!(!covers("Remember that I can swim", &["Cannot swim."]));
        assert!(covers(
            "Remember that I don't like tea",
            &["Doesn't like tea."]
        ));
        assert!(covers(
            "Remember that I do not like tea",
            &["Doesn't like tea."]
        ));
    }

    #[test]
    fn a_negation_or_a_letter_the_call_did_not_keep_goes_to_round_2() {
        // Thirteen's P3 on #5565: "not" and a single letter are content.
        for (said, kept) in [
            ("Remember that I don't like tea", "Likes tea."),
            ("Remember that I do not like tea", "Likes tea."),
            ("Remember that I never drink coffee", "Drinks coffee."),
            ("Remember that I dont eat pork", "Eats pork."),
            ("Remember that I can't swim", "Can swim."),
            ("Remember my blood type is O", "Blood type is A."),
            ("Remember my blood type is O", "Blood type is B."),
            ("Remember that my flat is 4B", "Flat is 4."),
            ("Remember that my seat is row F", "Seat is row G."),
        ] {
            assert!(only_keeps(said), "{said}");
            assert!(!covers(said, &[kept]), "{said} / {kept}");
        }
        // Kept with its negation or its letter, it stays fast.
        for (said, kept) in [
            ("Remember that I don't like tea", "Doesn't like tea."),
            ("Remember that I don't like tea", "Does not like tea."),
            ("Remember that I do not like tea", "Doesn't like tea."),
            ("Remember that I can't swim", "Can't swim."),
            ("Remember that I can't swim", "Cannot swim."),
            ("Remember that I never drink coffee", "Never drinks coffee."),
            ("Remember my blood type is O", "Blood type is O."),
            ("Remember that my seat is row F", "Seat is row F."),
            ("Remember that I'm vegan", "Is vegan."),
            ("Remember that I'd like oat milk", "Would like oat milk."),
        ] {
            assert!(covers(said, &[kept]), "{said} / {kept}");
        }
    }

    #[test]
    fn a_letter_a_needs_one_ordered_coverage_with_its_occurrences() {
        for (said, kept) in [
            ("Remember my blood type is A", vec!["Blood type is B."]),
            (
                "Remember my blood type is A",
                vec!["A person has blood type B."],
            ),
            (
                "Remember my blood type is A",
                vec!["Blood type is B.", "Has a cat."],
            ),
            ("Remember my blood type is A", vec!["Blood type."]),
            (
                "Remember my grade is A and my group is A",
                vec!["Grade and group are A."],
            ),
            (
                "Remember my blood type is A and I have a cat",
                vec!["Blood type is A.", "Has a cat."],
            ),
            ("Remember my blood type is A", vec!["A is the blood type."]),
        ] {
            assert!(!covers(said, &kept), "{said} / {kept:?}");
        }
        for (said, kept) in [
            ("Remember my blood type is A", "Blood type is A."),
            ("Remember my blood type is a", "Blood type is a."),
            ("Remember that I have a cat", "Has a cat."),
            (
                "Remember my grade is A and my group is A",
                "Grade is A and group is A.",
            ),
            (
                "Make a note that my dentist is on Tuesday",
                "Their dentist is on Tuesday.",
            ),
        ] {
            assert!(covers(said, &[kept]), "{said} / {kept}");
        }
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
