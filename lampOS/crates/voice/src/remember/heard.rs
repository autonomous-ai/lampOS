//! A kept fact as it was heard (#2176, row 10), with
//! `LAMP_MEMORY_KEEP_HEARD=on`.
//!
//! The model writes the sentence `remember` keeps, and nothing checked it
//! against what was said: on lamp-06c1 on 20 Oct "please remember that my
//! favourite tea is jasmine" and, later, "... my favourite fruit is mango"
//! were both kept as "My favorite food is jasmine." With the switch, a
//! `remember` for a line heard at the lamp is held against the words heard
//! in that line ([`crate::thread::Thread::heard_for`]). The model's key
//! words are its words that are not common ([`COMMON`], compared with
//! accents kept) and have three letters or more, or a digit. When one of
//! them is not among the words heard, compared lower-cased and without
//! accents, the heard line is kept instead, without its "please remember
//! that" ([`LEAD_INS`]), as "They said: …", at most the memory's 300 bytes,
//! and the model is told what was kept. With nothing heard (a text from the
//! app, a line the lamp wrote) or the switch off, the model's text is kept
//! as before. The heard words are what the person said, as far as the lamp
//! knows: speech to text may mishear them too.
use lamp_gemini::ToolCall;
use serde_json::{Value, json};
use std::{sync::OnceLock, time::Instant};
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

/// Only the exact value `on`. Off, `remember` keeps the model's text.
pub const ENVIRONMENT: &str = "LAMP_MEMORY_KEEP_HEARD";
static ON: OnceLock<bool> = OnceLock::new();

/// The memory's own cap on a kept text (`lamp_memory::store::TEXT_BYTES`).
const TEXT_BYTES: usize = 300;
/// How a kept heard line begins, so the note reads as theirs.
const SAID: &str = "They said: ";

/// Whether the switch is on in this process, read once.
pub fn on() -> bool {
    *ON.get_or_init(|| std::env::var(ENVIRONMENT).ok().as_deref() == Some("on"))
}

/// Words nearly every sentence has, in [`fold`]'s form with accents kept:
/// `lamp_memory::search::COMMON` (#2282), with the words a model uses for
/// whoever spoke.
const COMMON: &[&str] = &[
    "the", "an", "and", "or", "but", "of", "to", "in", "on", "at", "for", "with", "from", "by",
    "about", "as", "is", "are", "was", "were", "be", "been", "am", "it", "its", "my", "me", "mine",
    "you", "your", "yours", "we", "our", "us", "he", "him", "his", "she", "her", "they", "them",
    "their", "what", "which", "who", "when", "where", "why", "how", "that", "this", "these",
    "those", "do", "does", "did", "have", "has", "had", "can", "could", "would", "should", "will",
    "any", "some", "not", "no", "so", "if", "there", "tell", "told", "remember", "please", "của",
    "là", "tôi", "và", "có", "không", "này", "đó", "thì", "mà", "những", "các", "một", "cho",
    "với", "được", "gì", "nào", "user", "person",
];

/// The ways a line asks to be remembered, longest first where one begins
/// another: what is cut from the front of a kept heard line.
const LEAD_INS: &[&str] = &[
    "please remember that",
    "please remember",
    "can you remember that",
    "could you remember that",
    "keep in mind that",
    "remember that",
    "remember",
    "hãy nhớ giúp tôi là",
    "hãy nhớ giúp tôi",
    "hãy nhớ rằng",
    "hãy nhớ là",
    "hãy nhớ",
    "nhớ giúp tôi là",
    "nhớ giúp tôi",
    "nhớ rằng",
    "nhớ là",
];

/// NFC, lower case, đ as d: `lamp_memory::search::text`.
fn fold(value: &str) -> String {
    value
        .nfc()
        .flat_map(char::to_lowercase)
        .map(|character| if character == 'đ' { 'd' } else { character })
        .nfc()
        .collect()
}

/// A folded word without its accents.
fn bare(word: &str) -> String {
    word.nfd()
        .filter(|c| !is_combining_mark(*c))
        .nfc()
        .collect()
}

fn words(value: &str) -> impl Iterator<Item = String> {
    fold(value)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>()
        .into_iter()
}

/// The model's words that must have been heard.
fn key_words(model: &str) -> Vec<String> {
    words(model)
        .filter(|word| !COMMON.iter().any(|common| fold(common) == *word))
        .filter(|word| word.chars().count() >= 3 || word.chars().any(|c| c.is_ascii_digit()))
        .map(|word| bare(&word))
        .collect()
}

/// The heard line without its lead-in, when it has one with words after.
fn without_lead_in(heard: &str) -> &str {
    let boundary = |at: usize| {
        heard[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric())
    };
    // The end of `lead` read from `at`, matched a character at a time in
    // lower case, ending at the end of a word.
    let matched = |at: usize, lead: &str| {
        let mut rest = heard[at..].char_indices();
        for want in lead.chars() {
            let (_, got) = rest.next()?;
            if !got.to_lowercase().eq(std::iter::once(want)) {
                return None;
            }
        }
        match rest.next() {
            None => Some(heard.len()),
            Some((offset, next)) => (!next.is_alphanumeric()).then_some(at + offset),
        }
    };
    let end = heard
        .char_indices()
        .map(|(at, _)| at)
        .filter(|at| boundary(*at))
        .find_map(|at| LEAD_INS.iter().find_map(|lead| matched(at, lead)));
    let Some(end) = end else { return heard };
    let rest = heard[end..].trim_start_matches(|c: char| !c.is_alphanumeric());
    if rest.is_empty() { heard } else { rest }
}

/// What is kept for the model's `model` text when `heard` was heard in
/// the line that asked, with the switch `on`: the heard line ("They said:
/// …") when a key word of the model's was not heard, else `None`, the
/// model's text.
pub fn heard_instead(model: &str, heard: Option<&str>, on: bool) -> Option<String> {
    let heard: String = heard?.trim().nfc().collect();
    if !on || !heard.chars().any(char::is_alphanumeric) {
        return None;
    }
    let said: Vec<String> = words(&heard).map(|word| bare(&word)).collect();
    if key_words(model).iter().all(|word| said.contains(word)) {
        return None;
    }
    let mut kept = String::from(SAID);
    for c in without_lead_in(&heard).trim_end().chars() {
        if kept.len() + c.len_utf8() > TEXT_BYTES {
            break;
        }
        kept.push(c);
    }
    Some(kept)
}

/// For a `remember` the voice runs for heard line `request`: the call with
/// the heard words in place of the model's, and those words, when
/// [`heard_instead`] keeps them. `heard` is false for a line not heard at
/// the lamp.
pub fn call_as_heard(call: &ToolCall, request: u64, heard: bool) -> Option<(ToolCall, String)> {
    if !heard || call.name != super::REMEMBER || !on() {
        return None;
    }
    let words = crate::thread::THREAD
        .lock()
        .ok()?
        .heard_for(request, Instant::now());
    call_with(call, words.as_deref())
}

/// [`call_as_heard`] with the words heard in its line.
fn call_with(call: &ToolCall, heard: Option<&str>) -> Option<(ToolCall, String)> {
    if call.name != super::REMEMBER {
        return None;
    }
    let model = call.args.get("text")?.as_str()?;
    let kept = heard_instead(model, heard, true)?;
    let mut call = call.clone();
    call.args = json!({"text": kept});
    Some((call, kept))
}

/// What the model is told once `kept` was kept in place of its sentence.
/// An answer that already has a `note` or says `existing` (a match found,
/// nothing new written, #2298) is left as it is.
pub fn told(mut told: Value, kept: &str) -> Value {
    let new_entry = told.get("note").is_none() && told["existing"] != true;
    if told["remembered"] == true && new_entry {
        told["kept"] = json!(kept);
        told["note"] = json!(
            "Your sentence had words they did not say, so I kept what they said. Say back what was kept."
        );
    }
    told
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const TEA: &str = "please remember that my favourite tea is jasmine";
    const MANGO: &str = "please remember that my favourite fruit is mango";
    const WRONG: &str = "My favorite food is jasmine.";

    fn kept(model: &str, heard: Option<&str>, on: bool) -> String {
        heard_instead(model, heard, on).unwrap_or_else(|| model.to_owned())
    }

    /// lamp-06c1, 20 Oct: the tea line kept as "food".
    #[test]
    fn a_model_sentence_with_words_not_said_keeps_what_was_heard() {
        assert_eq!(
            kept(WRONG, Some(TEA), true),
            "They said: my favourite tea is jasmine"
        );
        // And the mango line kept as jasmine.
        assert_eq!(
            kept(WRONG, Some(MANGO), true),
            "They said: my favourite fruit is mango"
        );
    }

    #[test]
    fn a_paraphrase_with_the_words_said_keeps_the_models_sentence() {
        for model in [
            "Their favourite tea is jasmine.",
            "The user's favourite tea is jasmine",
            "Favourite tea: jasmine",
        ] {
            assert_eq!(kept(model, Some(TEA), true), model);
        }
        // Accents and đ compared without, numbers as words.
        assert_eq!(
            kept(
                "Mã tủ đồ là 417.",
                Some("hãy nhớ là mã tủ đồ của tôi là 417"),
                true
            ),
            "Mã tủ đồ là 417."
        );
        assert_eq!(
            kept(
                "Their favourite dish is pho, from Đà Nẵng",
                Some("remember my favourite dish is phở from Da Nang"),
                true
            ),
            "Their favourite dish is pho, from Đà Nẵng"
        );
        assert_eq!(
            kept(
                "Locker code 418",
                Some("remember my locker code is 417"),
                true
            ),
            "They said: my locker code is 417"
        );
    }

    #[test]
    fn off_or_with_nothing_heard_the_models_text_is_kept_exactly() {
        assert_eq!(kept(WRONG, Some(TEA), false), WRONG);
        assert_eq!(kept(WRONG, None, true), WRONG);
        assert_eq!(kept(WRONG, Some("  "), true), WRONG);
    }

    #[test]
    fn the_lead_in_goes_wherever_it_is_and_the_cap_holds() {
        assert_eq!(
            kept(
                WRONG,
                Some("Hey Lamp, Please Remember that, my tea is green"),
                true
            ),
            "They said: my tea is green"
        );
        assert_eq!(
            kept("Bánh mì", Some("Hãy nhớ rằng tôi thích phở"), true),
            "They said: tôi thích phở"
        );
        // A line that is only the lead-in is kept whole.
        assert_eq!(kept(WRONG, Some("remember"), true), "They said: remember");
        let long = format!("remember that {}", "ạ".repeat(200));
        let long = kept(WRONG, Some(&long), true);
        assert!(
            long.len() <= TEXT_BYTES && long.starts_with(SAID),
            "{}",
            long.len()
        );
    }

    /// The words a `remember` is held against: its line's, from the thread
    /// both voice routes keep, whether or not the lamp has begun to answer.
    #[test]
    fn the_call_is_held_against_the_words_heard_in_its_own_line() {
        use crate::thread::Thread;
        let now = Instant::now();
        let mut thread = Thread::new();
        assert_eq!(thread.heard_for(7, now), None);
        thread.person("please remember that my favourite", false, now);
        thread.person(" tea is jasmine", true, now);
        assert_eq!(thread.heard_for(7, now).as_deref(), Some(TEA));
        // Once the lamp answers line 7, its words stay with it.
        thread.lamp(7, "Okay, I'll keep that.", now);
        thread.person("thanks", true, now);
        assert_eq!(thread.heard_for(7, now).as_deref(), Some(TEA));
        // Something said long before is not the line asking.
        let mut thread = Thread::new();
        thread.person(TEA, true, now);
        assert_eq!(thread.heard_for(8, now + Duration::from_secs(60)), None);

        let call = |name: &str, text: &str| ToolCall {
            id: String::new(),
            name: name.into(),
            args: json!({"text": text}),
        };
        let (sent, kept) = call_with(&call(super::super::REMEMBER, WRONG), Some(TEA)).unwrap();
        assert_eq!(kept, "They said: my favourite tea is jasmine");
        assert_eq!(sent.args, json!({"text": kept}));
        assert_eq!(sent.name, super::super::REMEMBER);
        assert_eq!(
            call_with(&call(super::super::RECALL, WRONG), Some(TEA)),
            None
        );
        assert_eq!(call_with(&call(super::super::REMEMBER, WRONG), None), None);
    }

    #[test]
    fn the_model_is_told_what_was_kept_only_when_it_was() {
        let told = told(json!({"remembered": true}), "They said: my tea is green");
        assert_eq!(told["kept"], "They said: my tea is green");
        assert!(told["note"].is_string());
        assert_eq!(
            super::told(json!({"error": "x"}), "They said: y"),
            json!({"error": "x"})
        );
        // With #2298's one-entry check: a match already kept stays as told.
        for answer in [
            json!({"remembered": true, "existing": true}),
            json!({"remembered": true, "note": "That was already kept."}),
            json!({"remembered": true, "existing": true, "note": "That was already kept."}),
        ] {
            assert_eq!(super::told(answer.clone(), "They said: y"), answer);
        }
    }
}
