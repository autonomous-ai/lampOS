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
//!
//! A line that is only a yes ([`only_a_yes`]: "Yes, please.", "Sure", "Vâng
//! ạ") is not what was asked to be kept (#3504, N6: on lamp-06c1 on 27 Oct
//! "Want me to keep that?" → "Yes, please." kept "They said: Yes, please."
//! twice out of two). When the lamp's answer before it offered to keep or
//! change something ([`offers_to_keep`]), the `remember` is held against
//! the person's line that answer was to ([`crate::thread::Thread::offered_line`]),
//! and a heard line kept instead keeps the model's `replaces`: it is the
//! line the offer was about. With no such offer, a yes alone keeps nothing
//! ([`yes_alone`]).
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
/// `lamp_memory::search::COMMON` (#2282, [`super::SEARCH_COMMON`]), with
/// these: "an", and the words a model uses for whoever spoke.
const COMMON: &[&str] = &["an", "user", "person"];

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

/// What a line that is only a yes begins with, without accents: English and
/// Vietnamese.
const YES: &[&str] = &[
    "yes", "yeah", "yep", "yup", "ok", "okay", "sure", "please", "alright", "right", "correct",
    "go", "do", "keep", "u", "um", "o", "vang", "co", "duoc", "da", "dung",
];
/// What may follow it, without accents ("Yes, please.", "Sure, go ahead",
/// "Vâng, được rồi ạ").
const YES_TAIL: &[&str] = &[
    "yes",
    "yeah",
    "ok",
    "okay",
    "sure",
    "please",
    "thanks",
    "thank",
    "you",
    "go",
    "ahead",
    "do",
    "it",
    "that",
    "keep",
    "of",
    "course",
    "lamp",
    "hey",
    "absolutely",
    "definitely",
    "great",
    "good",
    "fine",
    "right",
    "correct",
    "roi",
    "nhe",
    "nha",
    "a",
    "di",
    "vang",
    "duoc",
    "dung",
    "vay",
    "co",
    "giup",
    "toi",
    "minh",
    "em",
    "luon",
];
/// The most words a line that is only a yes has.
const YES_MOST: usize = 6;

/// Whether `line` says nothing but a yes: [`YES`], then at most
/// [`YES_MOST`] words in all of [`YES_TAIL`]. "Yes, my dog is Moe" is not.
pub fn only_a_yes(line: &str) -> bool {
    let said: Vec<String> = words(line).map(|word| bare(&word)).collect();
    let Some((first, rest)) = said.split_first() else {
        return false;
    };
    said.len() <= YES_MOST
        && YES.contains(&first.as_str())
        && rest.iter().all(|word| YES_TAIL.contains(&word.as_str()))
}

/// The words, without accents, by which the lamp's question offers to keep
/// or change something.
const OFFERS: &[&str] = &[
    "keep", "remember", "save", "note", "replace", "update", "change", "store", "ghi", "nho",
    "luu", "thay",
];

/// Whether the lamp's words `said` offer to keep or change something: one
/// of [`OFFERS`] in them ("Want me to keep that?", "Shall I update it?").
/// Whether they asked is [`crate::remarks::lamp_asked`]'s.
pub fn offers_to_keep(said: &str) -> bool {
    words(said).any(|word| OFFERS.contains(&bare(&word).as_str()))
}

/// The line a `remember` in a line heard as `heard` is held against.
#[derive(Debug, PartialEq)]
pub(crate) enum Held {
    /// The line heard, as ever.
    Heard(Option<String>),
    /// The person's line the lamp's offer was about, for a yes to it.
    Offered(String),
    /// A yes, with no offer before it.
    YesAlone,
}

/// [`Held`] for the words `heard` in a line, `offered` the line the lamp's
/// newest answer before it offered to keep, asked only for a yes.
pub(crate) fn held(heard: Option<String>, offered: impl FnOnce() -> Option<String>) -> Held {
    match heard {
        Some(line) if only_a_yes(&line) => offered().map_or(Held::YesAlone, Held::Offered),
        heard => Held::Heard(heard),
    }
}

/// The person a `remember` under `lease` writes for: the lease's admitted
/// person, `None` for the room's or with no lease.
fn writes_for(lease: Option<&crate::memory_scope::ScopeLease>) -> Option<&str> {
    lease
        .and_then(|lease| lease.scope())
        .and_then(|scope| scope.person())
}

/// [`held`] for `request` on this process's thread, now, for a `remember`
/// that writes under `lease`: an offer counts only when it was made in that
/// same scope.
fn held_now(request: u64, lease: Option<&crate::memory_scope::ScopeLease>) -> Option<Held> {
    let now = Instant::now();
    let whose = writes_for(lease);
    let thread = crate::thread::THREAD.lock().ok()?;
    Some(held(thread.heard_for(request, now), || {
        thread.offered_line(request, whose, now, offers_to_keep)
    }))
}

/// The words a `remember` in heard line `request` is held against, with
/// the switch on: its line's, or for a yes to the lamp's offer the line the
/// offer was about. Off, its line's.
pub fn words_for(request: u64, lease: Option<&crate::memory_scope::ScopeLease>) -> Option<String> {
    if !on() {
        let now = Instant::now();
        return crate::thread::THREAD.lock().ok()?.heard_for(request, now);
    }
    match held_now(request, lease)? {
        Held::Heard(words) => words,
        Held::Offered(line) => Some(line),
        Held::YesAlone => None,
    }
}

/// What the model is told for a `remember` whose line was only a yes, with
/// no offer to keep before it.
pub const YES_ALONE: &str = "Nothing was kept: they only said yes, and you had not offered to keep anything. Ask what they would like kept. Never say it was kept.";

/// The journal's line for it: no words of it.
pub const YES_ALONE_LINE: &str = "lamp-voice provider: a remember in a line that was only a yes, with no offer before it, was not kept (skipped=1)";

/// What the model is told instead of running a `remember` in heard line
/// `request` that was only a yes with no offer before it, with the switch
/// on ([`YES_ALONE`]): a line that is only a yes is never kept. `None`:
/// run it.
pub fn yes_alone(
    call: &ToolCall,
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
) -> Option<Value> {
    if !heard || call.name != super::REMEMBER || !on() {
        return None;
    }
    (held_now(request, lease)? == Held::YesAlone).then(|| {
        eprintln!("{YES_ALONE_LINE}");
        json!({"remembered": false, "note": YES_ALONE})
    })
}

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
        .filter(|word| {
            !super::SEARCH_COMMON
                .iter()
                .chain(COMMON)
                .any(|common| fold(common) == *word)
        })
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
pub fn call_as_heard(
    call: &ToolCall,
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
) -> Option<(ToolCall, String)> {
    if !heard || call.name != super::REMEMBER || !on() {
        return None;
    }
    // The replaced fact's words, only for a correction whose sentence
    // was not all heard: as this session was shown them, no store call.
    let replaced = || {
        let id = call.args.get("replaces")?.as_u64()?;
        super::shown_text(id, lease)
    };
    match held_now(request, lease)? {
        Held::Heard(words) => call_with(call, words.as_deref(), replaced),
        Held::Offered(line) => call_kept_with(call, Some(&line), replaced, true),
        Held::YesAlone => None,
    }
}

/// [`call_as_heard`] with the words heard in its line, and `replaced`, the
/// text of the fact a `replaces` names as this session was shown it. A
/// heard line kept instead is sent without the model's `replaces` (#2510,
/// reviewer-lead P2): it may lack the fact's subject ("They said: it's
/// green now"), so the old fact stays and the heard line is kept beside it.
/// Its `on` (#1693) stays: by now an `on` is left only when these same heard
/// words name that day ([`super::on::call_as_heard`]), so the heard line is
/// exactly what dates it (reviewer-lead P3 on #2680).
pub(crate) fn call_with(
    call: &ToolCall,
    heard: Option<&str>,
    replaced: impl FnOnce() -> Option<String>,
) -> Option<(ToolCall, String)> {
    call_kept_with(call, heard, replaced, false)
}

/// [`call_with`], the heard line kept instead sent with the model's
/// `replaces` when `keeps_replaces`: for the line a yes to the lamp's
/// offer was about (N6), which is the whole of what the offer was for.
fn call_kept_with(
    call: &ToolCall,
    heard: Option<&str>,
    replaced: impl FnOnce() -> Option<String>,
    keeps_replaces: bool,
) -> Option<(ToolCall, String)> {
    if call.name != super::REMEMBER {
        return None;
    }
    let model = call.args.get("text")?.as_str()?;
    let kept = heard_instead(model, heard, true)?;
    // A correction (#2602) keeps the model's words when each is checkable:
    // the old fact's or heard, both without accents as for the heard words.
    if call.args.get("replaces").is_some_and(|id| !id.is_null())
        && let Some(old) = replaced()
    {
        let known: Vec<String> = words(&old)
            .chain(heard.into_iter().flat_map(words))
            .map(|word| bare(&word))
            .collect();
        if key_words(model).iter().all(|word| known.contains(word)) {
            return None;
        }
    }
    let mut call = call.clone();
    let on = call.args.get("on").cloned();
    let replaces = call.args.get("replaces").cloned();
    call.args = json!({"text": kept});
    if let Some(on) = on {
        call.args["on"] = on;
    }
    if let Some(replaces) = replaces.filter(|id| keeps_replaces && !id.is_null()) {
        call.args["replaces"] = replaces;
    }
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

/// What the model is told for its `remember` call `model`: [`told`] when
/// `as_heard`, the heard line, was sent instead. With `replaces`
/// (`LAMP_MEMORY_REPLACES`), a correction sent so went without its
/// `replaces` ([`call_with`]): the old fact stays, and the answer says
/// `not_replaced` with its id, so the model doesn't say it changed (the
/// reviewer-lead's P3 at #2590's merge).
pub fn answered(
    told: Value,
    model: &ToolCall,
    as_heard: Option<&(ToolCall, String)>,
    replaces: bool,
) -> Value {
    let Some((sent, kept)) = as_heard else {
        return told;
    };
    let mut told = self::told(told, kept);
    // Sent with its `replaces` (a yes to the lamp's offer, N6): as told.
    if sent.args.get("replaces").is_some_and(|id| !id.is_null()) {
        return told;
    }
    let old = model.args.get("replaces").and_then(Value::as_u64);
    if let Some(old) = old.filter(|_| replaces && told["remembered"] == true) {
        told["not_replaced"] = json!(old);
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
        let (sent, kept) =
            call_with(&call(super::super::REMEMBER, WRONG), Some(TEA), || None).unwrap();
        assert_eq!(kept, "They said: my favourite tea is jasmine");
        assert_eq!(sent.args, json!({"text": kept}));
        assert_eq!(sent.name, super::super::REMEMBER);
        assert_eq!(
            call_with(&call(super::super::RECALL, WRONG), Some(TEA), || None),
            None
        );
        assert_eq!(
            call_with(&call(super::super::REMEMBER, WRONG), None, || None),
            None
        );
        // A correction (#2510) whose heard line is kept drops its `replaces`.
        let mut correction = call(super::super::REMEMBER, WRONG);
        correction.args["replaces"] = json!(4);
        let (sent, kept) = call_with(&correction, Some(TEA), || None).unwrap();
        assert_eq!(sent.args, json!({"text": kept}));
        // Its day stays with the heard line that named it (#1693,
        // reviewer-lead P3 on #2680).
        let mut dated = call(super::super::REMEMBER, "Dentist appointment on Friday.");
        dated.args["on"] = json!("2026-10-23");
        let (sent, kept) =
            call_with(&dated, Some("remember my dentist is on friday"), || None).unwrap();
        assert_eq!(kept, "They said: my dentist is on friday");
        assert_eq!(sent.args, json!({"text": kept, "on": "2026-10-23"}));
    }

    /// lamp-06c1, 21 Oct (#2602): "it's green now" against kept thing 8.
    /// A correction keeps the model's words, with `replaces`, when each of
    /// them is the old fact's or was heard; else the heard line, without.
    #[test]
    fn a_correction_keeps_the_models_words_when_each_is_the_old_facts_or_heard() {
        const VIOLET: &str = "Her favourite colour is violet";
        const GREEN_NOW: &str = "it's green now";
        let correction = |text: &str, replaces: Option<u64>| {
            let mut args = json!({"text": text});
            if let Some(id) = replaces {
                args["replaces"] = json!(id);
            }
            ToolCall {
                id: String::new(),
                name: super::super::REMEMBER.into(),
                args,
            }
        };
        let old = |text: &'static str| move || Some(text.to_owned());
        // (a) Every word the old fact's or heard: sent as the model wrote it.
        let green = correction("Her favourite colour is green.", Some(8));
        assert_eq!(call_with(&green, Some(GREEN_NOW), old(VIOLET)), None);
        // (b) "dark" was neither: the heard line, without `replaces`.
        let dark = correction("Her favourite colour is dark green", Some(8));
        let (sent, kept) = call_with(&dark, Some(GREEN_NOW), old(VIOLET)).unwrap();
        assert_eq!(kept, "They said: it's green now");
        assert_eq!(sent.args, json!({"text": kept}));
        // An old fact this session was not shown checks nothing.
        let (sent, _) = call_with(&green, Some(GREEN_NOW), || None).unwrap();
        assert_eq!(sent.args, json!({"text": "They said: it's green now"}));
        // (c) Accents folded on both sides, as for the heard words.
        let xanh = correction("Màu yêu thích của chị là xanh lá", Some(3));
        assert_eq!(
            call_with(
                &xanh,
                Some("giờ là xanh lá rồi"),
                old("màu yêu thích của chị là tím")
            ),
            None
        );
        let mau = correction("Mau yeu thich cua chi la xanh la", Some(3));
        assert_eq!(
            call_with(
                &mau,
                Some("giờ là xanh lá rồi"),
                old("Màu yêu thích của chị là tím")
            ),
            None
        );
        // (d) Without `replaces`, as before: the heard line, the old fact
        // never asked for.
        let plain = correction("Her favourite colour is green.", None);
        let (sent, kept) = call_with(&plain, Some(GREEN_NOW), || -> Option<String> {
            panic!("asked for an old fact without replaces")
        })
        .unwrap();
        assert_eq!(sent.args, json!({"text": kept}));
        assert_eq!(kept, "They said: it's green now");
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
            json!({"remembered": true, "note": crate::remember::ALREADY_KEPT}),
            json!({"remembered": true, "existing": true, "note": crate::remember::ALREADY_KEPT}),
        ] {
            assert_eq!(super::told(answer.clone(), "They said: y"), answer);
        }
        // A correction (#2510) that kept the heard line says so too.
        let told = super::told(json!({"remembered": true, "replaced": 4}), "They said: y");
        assert_eq!(
            (told["replaced"].as_u64(), told["kept"].as_str()),
            (Some(4), Some("They said: y"))
        );
        assert!(told["note"].is_string());
    }

    /// lamp-06c1, 27 Oct (#3504, N6): "Want me to keep that?" → "Yes,
    /// please." kept "They said: Yes, please.", 2 of 2.
    #[test]
    fn a_line_that_is_only_a_yes_is_known_in_english_and_vietnamese() {
        for yes in [
            "Yes, please.",
            "yes",
            "Yeah, go ahead",
            "Okay, keep it.",
            "Sure, thanks!",
            "Please do",
            "Ừ",
            "Vâng ạ",
            "Có, được rồi",
            "Dạ vâng",
            "Ok nhé",
        ] {
            assert!(only_a_yes(yes), "{yes}");
        }
        for not in [
            "",
            "...",
            "Yes, my dog is called Moe",
            "please remember that my tea is green",
            "Best, replace the bakery with the library.",
            "Có, tôi làm ở thư viện",
            "yes yes yes yes yes yes yes",
            "No, thanks",
        ] {
            assert!(!only_a_yes(not), "{not}");
        }
        assert!(offers_to_keep("I've got that. Want me to keep that?"));
        assert!(offers_to_keep("Shall I update it to the library?"));
        assert!(offers_to_keep("Mình ghi nhớ nhé?"));
        assert!(!offers_to_keep("Shall I remove it?"));
        assert!(!offers_to_keep("How was your day?"));
    }

    /// A yes is held against the line the lamp's offer was about; with no
    /// offer before it, nothing is kept; any other line, as before.
    #[test]
    fn a_yes_to_the_lamps_offer_keeps_the_line_the_offer_was_about() {
        use crate::thread::Thread;
        const LIBRARY: &str = "I work at a library now, not the bakery";
        let now = Instant::now();
        let held_for = |thread: &Thread, request: u64, at: Instant| {
            held(thread.heard_for(request, at), || {
                thread.offered_line(request, None, at, offers_to_keep)
            })
        };
        let mut thread = Thread::new();
        thread.person(LIBRARY, true, now);
        thread.lamp(1, "A library, nice. Want me to keep that?", now);
        thread.person("Yes, please.", true, now);
        assert_eq!(held_for(&thread, 2, now), Held::Offered(LIBRARY.to_owned()));
        // Once the lamp has begun answering the yes, the same.
        thread.lamp(2, "Done.", now);
        assert_eq!(held_for(&thread, 2, now), Held::Offered(LIBRARY.to_owned()));
        // A line that says more than yes is its own, as ever.
        let mut thread = Thread::new();
        thread.person(LIBRARY, true, now);
        thread.lamp(1, "Want me to keep that?", now);
        thread.person("Yes, my dog is called Moe", true, now);
        assert_eq!(
            held_for(&thread, 2, now),
            Held::Heard(Some("Yes, my dog is called Moe".to_owned()))
        );
        // No offer: a question that offers nothing, none at all, or a yes
        // long after.
        for (lamp, after) in [
            ("How was your day?", 0),
            ("A library, nice.", 0),
            ("Want me to keep that?", 60),
        ] {
            let mut thread = Thread::new();
            thread.person(LIBRARY, true, now);
            thread.lamp(1, lamp, now);
            let later = now + Duration::from_secs(after);
            thread.person("Yes.", true, later);
            assert_eq!(held_for(&thread, 2, later), Held::YesAlone, "{lamp}");
        }
        // Reviewer-three's P2 on #3683: the offer counts only in the scope
        // the yes's `remember` writes to, never by the face at the lamp. A
        // known person's offer, then a yes kept as the room's (someone not
        // attributed) or as another known person's, keeps nothing, even
        // with the first person's face still named at the lamp.
        let mut thread = Thread::new();
        thread.seen_now(Some("p-olivia".to_owned()));
        thread.person(LIBRARY, true, now);
        thread.lamp(1, "Want me to keep that?", now);
        thread.person("Yes.", true, now);
        let offered = |whose| thread.offered_line(2, whose, now, offers_to_keep);
        assert_eq!(offered(Some("p-olivia")), Some(LIBRARY.to_owned()));
        assert_eq!(offered(None), None);
        assert_eq!(offered(Some("p-tom")), None);
        // And the room's offer is the room's alone.
        let mut thread = Thread::new();
        thread.person(LIBRARY, true, now);
        thread.lamp(1, "Want me to keep that?", now);
        thread.person("Yes.", true, now);
        let offered = |whose| thread.offered_line(2, whose, now, offers_to_keep);
        assert_eq!(offered(None), Some(LIBRARY.to_owned()));
        assert_eq!(offered(Some("p-olivia")), None);
        let mut thread = Thread::new();
        thread.person("Yes.", true, now);
        assert_eq!(held_for(&thread, 1, now), Held::YesAlone);
    }

    /// P9b: the model's "Works at a library." against the bakery (kept
    /// thing 5), after a yes. Each word the old fact's or the offered
    /// line's: sent as written with `replaces`; else the offered line,
    /// still with `replaces`, never "They said: Yes, please.".
    #[test]
    fn a_correction_after_a_yes_keeps_its_replaces() {
        const LIBRARY: &str = "I work at a library now, not the bakery";
        let call = |text: &str| ToolCall {
            id: String::new(),
            name: super::super::REMEMBER.into(),
            args: json!({"text": text, "replaces": 5}),
        };
        let bakery = || Some("Works at a bakery.".to_owned());
        let library = call("Works at a library.");
        assert_eq!(call_kept_with(&library, Some(LIBRARY), bakery, true), None);
        let (sent, kept) = call_kept_with(&library, Some(LIBRARY), || None, true).unwrap();
        assert_eq!(kept, "They said: I work at a library now, not the bakery");
        assert_eq!(sent.args, json!({"text": kept, "replaces": 5}));
        let told = answered(
            json!({"remembered": true, "replaced": 5}),
            &library,
            Some(&(sent, kept)),
            true,
        );
        assert!(told.get("not_replaced").is_none());
        assert_eq!(told["replaced"], 5);
        // Any other heard line kept instead still drops it (#2510).
        let (sent, _) = call_with(&library, Some(LIBRARY), || None).unwrap();
        assert!(sent.args.get("replaces").is_none());
    }

    /// The reviewer-lead's P3 at #2590's merge: a correction kept as heard
    /// went without its `replaces`, so the old fact stays; the model is
    /// told `not_replaced` with the old id, so it doesn't say it changed.
    #[test]
    fn a_correction_kept_as_heard_answers_not_replaced() {
        const GREEN_NOW: &str = "it's green now";
        let call = |text: &str, replaces: Option<u64>| {
            let mut args = json!({"text": text});
            if let Some(id) = replaces {
                args["replaces"] = json!(id);
            }
            ToolCall {
                id: String::new(),
                name: super::super::REMEMBER.into(),
                args,
            }
        };
        let dark = call("Her favourite colour is dark green", Some(8));
        let fallback = call_with(&dark, Some(GREEN_NOW), || None);
        let kept = json!({"remembered": true});
        let told = answered(kept.clone(), &dark, fallback.as_ref(), true);
        assert_eq!(told["not_replaced"], 8);
        assert_eq!(told["kept"], "They said: it's green now");
        assert!(told.get("replaced").is_none());
        // Already kept as heard (#2298): still not replaced.
        let existing = json!({"remembered": true, "existing": true});
        assert_eq!(
            answered(existing.clone(), &dark, fallback.as_ref(), true)["not_replaced"],
            8
        );
        // Checkable (#2704): sent with `replaces`, answered as before.
        let green = call("Her favourite colour is green.", Some(8));
        let violet = || Some("Her favourite colour is violet".to_owned());
        let checked = call_with(&green, Some(GREEN_NOW), violet);
        assert_eq!(checked, None);
        let replaced = json!({"remembered": true, "replaced": 8, "id": 10});
        assert_eq!(
            answered(replaced.clone(), &green, checked.as_ref(), true),
            replaced
        );
        // No `replaces`, or a failed call: no `not_replaced`.
        let plain = call("Her favourite colour is dark green", None);
        let heard = call_with(&plain, Some(GREEN_NOW), || None);
        assert!(
            answered(kept.clone(), &plain, heard.as_ref(), true)
                .get("not_replaced")
                .is_none()
        );
        let error = json!({"error": "x"});
        assert_eq!(
            answered(error.clone(), &dark, fallback.as_ref(), true),
            error
        );
        // `LAMP_MEMORY_REPLACES` off: as before, the heard line's answer.
        assert_eq!(
            answered(kept.clone(), &dark, fallback.as_ref(), false),
            super::told(kept, "They said: it's green now")
        );
    }
}
