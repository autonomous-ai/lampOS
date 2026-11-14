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
pub(crate) const SAID: &str = "They said: ";

#[cfg(test)]
thread_local! {
    /// The switch for one test's own thread ([`set_on_for_test`]): the
    /// process switch is read once, and other tests read it off.
    static ON_FOR_TEST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the switch is on in this process, read once.
pub fn on() -> bool {
    #[cfg(test)]
    if ON_FOR_TEST.with(std::cell::Cell::get) {
        return true;
    }
    *ON.get_or_init(|| std::env::var(ENVIRONMENT).ok().as_deref() == Some("on"))
}

/// The switch on or off for the calling test's thread alone.
#[cfg(test)]
pub(crate) fn set_on_for_test(on: bool) {
    ON_FOR_TEST.with(|cell| cell.set(on));
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

/// What a line that is only a no begins with, without accents: English and
/// Vietnamese ("No.", "Nope", "Không", "Thôi").
const NO: &[&str] = &["no", "nope", "nah", "khong", "thoi"];
/// What may follow it, without accents ("No, thanks", "Không cần đâu ạ").
const NO_TAIL: &[&str] = &[
    "no", "thanks", "thank", "you", "not", "now", "need", "lamp", "that", "it", "khong", "can",
    "dau", "a", "nhe", "cam", "on", "thoi",
];

/// Whether `line` says nothing but a no: [`NO`], then at most [`YES_MOST`]
/// words in all of [`NO_TAIL`]. "No, my size is 42" is not.
pub fn only_a_no(line: &str) -> bool {
    let said: Vec<String> = words(line).map(|word| bare(&word)).collect();
    let Some((first, rest)) = said.split_first() else {
        return false;
    };
    said.len() <= YES_MOST
        && NO.contains(&first.as_str())
        && rest.iter().all(|word| NO_TAIL.contains(&word.as_str()))
}

/// Whether `line` is only a yes, a no or an okay ([`only_a_yes`],
/// [`only_a_no`]): never kept as a fact, by `remember` or the observation
/// pass.
pub fn only_an_answer(line: &str) -> bool {
    only_a_yes(line) || only_a_no(line)
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
    /// A yes with no offer before it, or a no: nothing is kept.
    YesAlone,
}

/// [`Held`] for the words `heard` in a line, `offered` the line the lamp's
/// newest answer before it offered to keep, asked only for a yes.
pub(crate) fn held(heard: Option<String>, offered: impl FnOnce() -> Option<String>) -> Held {
    match heard {
        Some(line) if only_a_yes(&line) => offered()
            .filter(|offered| !only_an_answer(offered))
            .map_or(Held::YesAlone, Held::Offered),
        // A no keeps nothing, offer or not.
        Some(line) if only_a_no(&line) => Held::YesAlone,
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
    // Only a plain yes releases a held keep (`second_provider`'s
    // `take_held_forget`): it keeps the carried line, never its own words.
    if let Some(line) = released(lease) {
        return Some(Held::Offered(line.to_owned()));
    }
    let now = Instant::now();
    let whose = writes_for(lease);
    let thread = crate::thread::THREAD.lock().ok()?;
    Some(held(thread.heard_for(request, now), || {
        thread.offered_line(request, whose, now, offers_to_keep, only_a_yes)
    }))
}

/// The carried line's heard words whose held keep this request's seen yes
/// released (`ScopeLease::release_held_keep`): the line the lamp's offer was
/// about, though that offer was told as the room's, so no offer of theirs
/// is in the thread (lamp-06c1, 6 Nov, C55).
pub(crate) fn released(lease: Option<&crate::memory_scope::ScopeLease>) -> Option<&str> {
    lease.and_then(crate::memory_scope::ScopeLease::released_keep)
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
pub const YES_ALONE: &str = "Nothing was kept: they only said yes or no, and no offer of yours to keep something was waiting for their yes. Ask what they would like kept, if anything. Never say it was kept.";

/// The journal's line for it: no words of it.
pub const YES_ALONE_LINE: &str = "lamp-voice provider: a remember in a line that was only a yes, with no offer before it, was not kept (skipped=1)";

/// What the model is told for a `remember` or `forget` whose line was only
/// a yes, with no offer waiting, just after the scope's last memory write
/// was done (Bug C, lamp-06c1, 5 Nov: after "Mondays now, not Fridays.",
/// a done keep and a done forget, a lone yes was told to ask, and the lamp
/// asked again what to keep or forget): by what was done.
pub const DONE_ALREADY_KEPT: &str = "It is done already (kept): your last turn kept it. Just confirm briefly that it is kept; do not ask again or offer to keep it again.";
/// [`DONE_ALREADY_KEPT`] after a replace.
pub const DONE_ALREADY_CHANGED: &str = "It is done already (changed): your last turn changed it. Just confirm briefly that it is changed; do not ask again or offer to change it again.";
/// [`DONE_ALREADY_KEPT`] after a forget.
pub const DONE_ALREADY_FORGOTTEN: &str = "It is done already (forgotten): your last turn forgot it. Just confirm briefly that it is forgotten; do not ask again what to forget.";

/// The journal's line for it: no words of it.
pub const DONE_ALREADY_LINE: &str = "lamp-voice provider: a memory call in a line that was only a yes, after a done write: told it is done already (skipped=1)";

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
    let forget = call.name == super::FORGET;
    if !heard || !(call.name == super::REMEMBER || forget) || !on() {
        return None;
    }
    // The carried person's seen yes to a carried line's held keep: a yes to
    // the lamp's offer about that line ([`released`]).
    if !forget && released(lease).is_some() {
        return None;
    }
    // Read before the thread is locked: the two locks are never held
    // together.
    let done = super::done_last_in(lease);
    let thread = crate::thread::THREAD.lock().ok()?;
    yes_alone_in(
        &thread,
        (request, writes_for(lease), Instant::now()),
        forget,
        done,
    )
}

/// [`yes_alone`] for heard line `request` on `thread`, in scope `whose`, at
/// `now`, with `done` the scope's last memory write done
/// ([`super::done_last_in`]): `None` unless the line is only a yes or a no
/// with no offer of the lamp's waiting ([`Held::YesAlone`]), then as
/// [`yes_alone_decided`].
pub(super) fn yes_alone_in(
    thread: &crate::thread::Thread,
    (request, whose, now): (u64, Option<&str>, Instant),
    forget: bool,
    done: Option<super::DoneWrite>,
) -> Option<Value> {
    let done = done_last_turn(thread, request, done);
    let said = thread.heard_for(request, now);
    let yes = said.as_deref().is_some_and(only_a_yes);
    let held = held(said, || {
        thread.offered_line(request, whose, now, offers_to_keep, only_a_yes)
    });
    if held != Held::YesAlone {
        return None;
    }
    yes_alone_decided(forget, yes, done, || {
        thread
            .offered_line(request, whose, now, asks_to_forget, |_| false)
            .is_some()
    })
}

/// [`yes_alone`] past its line being only a yes or a no with no offer
/// waiting: `forget`, the call is a `forget`; `yes`, the line was a yes
/// (not a no); `done`, the scope's last memory write done
/// ([`super::done_last_in`]); `asked`, whether the lamp's answer before it
/// asked whether to forget. Only a yes after a done write is told it is
/// done already: a no never is (the reviewer-lead's P2 on #5436: "Kept it,
/// right?" "No." runs its forget). A `forget` is held only when the done
/// write was itself a forget and the lamp asked nothing about forgetting
/// (Heron's and Twelve's P2 on #5436: a yes to "Should I clear the locker
/// number too?" is consent, and runs); else it runs as before (`None`).
fn yes_alone_decided(
    forget: bool,
    yes: bool,
    done: Option<super::MemoryDone>,
    asked: impl FnOnce() -> bool,
) -> Option<Value> {
    let done = done.filter(|_| yes);
    if forget && (done != Some(super::MemoryDone::Forgotten) || asked()) {
        return None;
    }
    Some(yes_alone_told(done, forget))
}

/// What asks whether to forget, without accents: English and Vietnamese
/// ("Should I drop the old Friday note?", "Mình bỏ ghi chú cũ nhé?").
const FORGET_ASKS: &[&str] = &[
    "forget", "delete", "erase", "remove", "clear", "drop", "discard", "scrap", "wipe", "rid",
    "quen", "xoa", "bo",
];

/// Whether the lamp's words `said` ask whether to forget something: one of
/// [`FORGET_ASKS`] in them.
fn asks_to_forget(said: &str) -> bool {
    words(said).any(|word| FORGET_ASKS.contains(&bare(&word).as_str()))
}

/// What [`yes_alone`] tells the model, and journals: when the scope's last
/// memory write was done within the held window (`done`), that it is done
/// already, by what was done; else [`YES_ALONE`]. Nothing is written
/// either way. `forget`: the call is a `forget`'s, told as one.
fn yes_alone_told(done: Option<super::MemoryDone>, forget: bool) -> Value {
    use super::MemoryDone;
    let Some(done) = done else {
        eprintln!("{YES_ALONE_LINE}");
        return json!({"remembered": false, "note": YES_ALONE});
    };
    eprintln!("{DONE_ALREADY_LINE}");
    let note = match done {
        MemoryDone::Kept => DONE_ALREADY_KEPT,
        MemoryDone::Changed => DONE_ALREADY_CHANGED,
        MemoryDone::Forgotten => DONE_ALREADY_FORGOTTEN,
    };
    if forget {
        json!({"forgotten": done == MemoryDone::Forgotten, "already": true, "note": note})
    } else {
        json!({"remembered": done != MemoryDone::Forgotten, "already": true, "note": note})
    }
}

/// What the model is told before round 1 of heard line `request`, with the
/// switch on, when the line is only a yes, no offer of the lamp's waits,
/// the lamp's answer before it asked nothing, and the scope's last memory
/// write was done within the held window ([`super::done_last_in`]): that it
/// is done already, by what was done, and what was done. On lamp-06c1 at
/// build 10538 (6 Nov, S13 req 339) "Yes. Please forget it." after a done
/// forget was answered with a `recall`, not a `forget`, so [`yes_alone`]
/// never saw it, and the lamp asked again what to forget. `None` else.
pub(crate) fn done_already(
    request: u64,
    lease: Option<&crate::memory_scope::ScopeLease>,
) -> Option<(String, super::MemoryDone)> {
    if !on() || released(lease).is_some() {
        return None;
    }
    // Read before the thread is locked, as [`yes_alone`] does.
    let done = super::done_last_in(lease)?;
    let thread = crate::thread::THREAD.lock().ok()?;
    done_already_in(
        &thread,
        (request, writes_for(lease), Instant::now()),
        Some(done),
    )
}

/// [`done_already`] for heard line `request` on `thread`, in scope `whose`,
/// at `now`, with `done` the scope's last memory write done.
pub(super) fn done_already_in(
    thread: &crate::thread::Thread,
    (request, whose, now): (u64, Option<&str>, Instant),
    done: Option<super::DoneWrite>,
) -> Option<(String, super::MemoryDone)> {
    use super::MemoryDone;
    let done = done_last_turn(thread, request, done)?;
    // A lamp's answer that asked nothing offers nothing to keep
    // ([`crate::thread::Thread::offered_line`] needs a question), so the
    // yes is a yes alone ([`Held::YesAlone`]); after "Anything else?" or
    // "Want me to keep that?" it answers that question instead.
    let said = thread.heard_for(request, now);
    // After a done forget, "Yes. Please forget it." is that yes too (S13
    // req 339): only a yes once its forget words are left out.
    let yes = |line: &str| {
        only_a_yes(line)
            || (done == MemoryDone::Forgotten && words(line).any(|word| asks_to_forget(&word)) && {
                let rest: Vec<String> = words(line).filter(|word| !asks_to_forget(word)).collect();
                only_a_yes(&rest.join(" "))
            })
    };
    if !said.as_deref().is_some_and(yes)
        || thread.lamp_asked_by_its_end(request)
        || thread
            .offered_line(request, whose, now, offers_to_keep, only_a_yes)
            .is_some()
    {
        return None;
    }
    let note = match done {
        MemoryDone::Kept => DONE_ALREADY_KEPT,
        MemoryDone::Changed => DONE_ALREADY_CHANGED,
        MemoryDone::Forgotten => DONE_ALREADY_FORGOTTEN,
    };
    eprintln!("{DONE_ALREADY_TOLD_LINE}");
    Some((format!("{DONE_ALREADY_TOLD} {note})"), done))
}

/// What of `done`, the scope's last memory write done and the request it
/// was done for, is "your last turn"'s for heard line `request`: the write,
/// when that request is `request` itself or the one just before it, no
/// exchange of the thread's between them (the forget's own answer may be
/// kept out of it, S13 req 339). The Reviewer-lead's P2 on #5607: "My bike
/// is blue." kept, then "My goldfish is called Pip." answered with no
/// `remember`, then "Yes. Please keep that.": the bike's keep is not the
/// yes's last turn, and Pip was never kept. A write done for no known
/// request is none.
fn done_last_turn(
    thread: &crate::thread::Thread,
    request: u64,
    done: Option<super::DoneWrite>,
) -> Option<super::MemoryDone> {
    let (done, done_for) = done?;
    let done_for = done_for.filter(|&done_for| done_for <= request)?;
    (!thread.answered_between(done_for, request)).then_some(done)
}

/// How [`done_already`]'s note opens; it ends with the done write's own
/// note ([`DONE_ALREADY_KEPT`], [`DONE_ALREADY_CHANGED`] or
/// [`DONE_ALREADY_FORGOTTEN`]) and a closing parenthesis.
pub const DONE_ALREADY_TOLD: &str =
    "(For you to know: they only said yes, just after your last turn's memory write was done.";

/// The journal's line for it: no words of it.
pub const DONE_ALREADY_TOLD_LINE: &str = "lamp-voice provider: a line that was only a yes, after a done write: the model is told before it answers that it is done already";

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
    // A line that is only a yes, no or okay is never kept as what was said.
    if !on || !heard.chars().any(char::is_alphanumeric) || only_an_answer(&heard) {
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

/// The switch for [`near_heard`] on the second route, in the voice
/// service's environment: `on` only (C37).
pub const NEAR_ENVIRONMENT: &str = "LAMP_MEMORY_ASK_NEAR_HEARD";

/// Whether [`NEAR_ENVIRONMENT`] is `on`.
pub fn near_on() -> bool {
    std::env::var(NEAR_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// At most this many near words are asked about in one keep.
const NEAR_MOST: usize = 2;

/// The forms `word` may be an English inflection of, its own included:
/// without -s, -es, -ies (for -y), -ed or -ing, with an e put back
/// ("liking", "liked": "like") or a doubled last letter undone
/// ("stopped": "stop"), each at least three letters, and with a British
/// "our" after two letters or more spelled "or" ("favourite", "colour").
fn word_forms(word: &str) -> Vec<String> {
    let word = match word.char_indices().nth(2) {
        Some((at, _)) => format!("{}{}", &word[..at], word[at..].replace("our", "or")),
        None => word.to_owned(),
    };
    let mut forms = vec![word.clone()];
    for suffix in ["ies", "ing", "ed", "es", "s"] {
        let Some(stem) = word.strip_suffix(suffix) else {
            continue;
        };
        if stem.chars().count() < 3 {
            continue;
        }
        if suffix == "ies" {
            forms.push(format!("{stem}y"));
            continue;
        }
        forms.push(stem.to_owned());
        if suffix == "ing" || suffix == "ed" {
            forms.push(format!("{stem}e"));
            let mut last = stem.chars().rev();
            if let (Some(a), Some(b)) = (last.next(), last.next())
                && a == b
                && stem.chars().count() > 3
            {
                forms.push(stem[..stem.len() - a.len_utf8()].to_owned());
            }
        }
    }
    forms
}

/// Whether `heard` and `model` are one word written two ways: an English
/// inflection (-s, -es, -ed, -ing: "like" and "likes", "liking";
/// "peanut" and "peanuts") or a British and American spelling
/// ("favourite" and "favorite", "colour" and "color"). The model rewrites
/// this way all the time, so [`near_heard`] counts them as heard: its
/// question would only sound like a machine (Reviewer-lead on #6850).
fn same_word(heard: &str, model: &str) -> bool {
    let model_forms = word_forms(model);
    word_forms(heard)
        .iter()
        .any(|form| model_forms.contains(form))
}

/// The model's key words not heard in `heard` when each of them is a near
/// spelling of a word that was ([`super::doubted::misheard_as`]: the same
/// first letter, at most half the letters changed), as (heard, model)
/// pairs without accents; empty otherwise (C37, lamp-06c1, 5 Nov: heard
/// "renunculus", the model's `remember` said "ranunculus", and
/// [`heard_instead`] kept the heard line while the lamp said "Kept: …
/// ranunculus"). Empty when every key word was heard (the model's text is
/// kept), when one is near no heard word (the heard line is kept, as ever),
/// when more than [`NEAR_MOST`] are near ones, and for a line that is only
/// a yes, no or okay. A word heard inflected or spelled the British way
/// ([`same_word`]: "likes" for "like", "favorite" for "favourite") counts
/// as heard, never as near.
pub fn near_heard(model: &str, heard: &str) -> Vec<(String, String)> {
    let heard: String = heard.trim().nfc().collect();
    if !heard.chars().any(char::is_alphanumeric) || only_an_answer(&heard) {
        return Vec::new();
    }
    let said: Vec<String> = words(&heard).map(|word| bare(&word)).collect();
    // Only a heard key word may be the one misheard: never "please" for
    // "plant", nor a lead-in's "remember".
    let heard_keys = key_words(without_lead_in(&heard));
    let mut near: Vec<(String, String)> = Vec::new();
    for word in key_words(model) {
        // The same word inflected or spelled the British way is heard
        // ([`same_word`]): "Likes hiking" for "I like hiking" asks nothing.
        if said.contains(&word)
            || said.iter().any(|heard| same_word(heard, &word))
            || near.iter().any(|(_, kept)| *kept == word)
        {
            continue;
        }
        let Some(heard) = heard_keys
            .iter()
            .find(|heard| super::doubted::misheard_as(heard, &word))
        else {
            return Vec::new();
        };
        near.push((heard.clone(), word));
    }
    if near.len() > NEAR_MOST {
        return Vec::new();
    }
    near
}

/// Whether the lamp's `answer` asks about a keep held by [`near_heard`]:
/// an offer to keep ([`super::asks_about_keeping`]), or a question offering
/// one of the model's near words for the heard one
/// ([`super::doubted::offered_word`]: "Is that ranunculus?").
pub fn asks_about_near(answer: &str, model: &str, heard: &str) -> bool {
    if super::asks_about_keeping(answer) {
        return true;
    }
    let near = near_heard(model, heard);
    super::doubted::offered_word(answer, heard).is_some_and(|(was, offered)| {
        near.iter()
            .any(|(heard, kept)| *heard == bare(&was) && *kept == bare(&offered))
    })
}

/// What the model is told of a `remember` held by [`near_heard`]: nothing
/// kept yet, ask once whether the word is theirs, the longest near word
/// as the example.
pub fn near_held_told(near: &[(String, String)]) -> Value {
    let asked = near
        .iter()
        .map(|(heard, kept)| format!("\"{kept}\" where they were heard saying \"{heard}\""))
        .collect::<Vec<_>>()
        .join(", and ");
    // The longest near word for the example: "ranunculus", not the
    // spelling of "favorite".
    let example = near
        .iter()
        .map(|(_, kept)| kept.as_str())
        .max_by_key(|kept| kept.chars().count())
        .unwrap_or_default();
    json!({
        "remembered": false,
        "held": format!(
            "Not kept yet: you wrote {asked}, and the lamp may have misheard. Do not say it is kept or noted. Ask them once, in a few words, whether that is the word (for example \"Is that {example}?\"), and then stop. Their yes keeps your sentence as you wrote it; do not call remember again for it."
        ),
    })
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
                thread.offered_line(request, None, at, offers_to_keep, only_a_yes)
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
        let offered = |whose| thread.offered_line(2, whose, now, offers_to_keep, only_a_yes);
        assert_eq!(offered(Some("p-olivia")), Some(LIBRARY.to_owned()));
        assert_eq!(offered(None), None);
        assert_eq!(offered(Some("p-tom")), None);
        // And the room's offer is the room's alone.
        let mut thread = Thread::new();
        thread.person(LIBRARY, true, now);
        thread.lamp(1, "Want me to keep that?", now);
        thread.person("Yes.", true, now);
        let offered = |whose| thread.offered_line(2, whose, now, offers_to_keep, only_a_yes);
        assert_eq!(offered(None), Some(LIBRARY.to_owned()));
        assert_eq!(offered(Some("p-olivia")), None);
        let mut thread = Thread::new();
        thread.person("Yes.", true, now);
        assert_eq!(held_for(&thread, 1, now), Held::YesAlone);
    }

    /// lamp-06c1, 2026-11-04 (build 10012): the lamp offered to keep the
    /// shoe size, a carried "Yes." was met with a second offer ("Want me to
    /// keep that shoe size for you?"), and the second "Yes." ran the held
    /// `remember` held against the first yes: "They said: Yes." was kept.
    /// A yes is held against the line the first offer was about, through
    /// any yes the lamp met with another offer; a no keeps nothing; a yes
    /// with no offer keeps nothing; a bare answer is never kept as said.
    #[test]
    fn a_yes_after_a_second_offer_keeps_the_first_offers_line() {
        use crate::thread::Thread;
        const SHOE: &str = "My shoe size is nine. Not that it matches.";
        let now = Instant::now();
        let held_for = |thread: &Thread, request: u64| {
            held(thread.heard_for(request, now), || {
                thread.offered_line(request, None, now, offers_to_keep, only_a_yes)
            })
        };
        let model = ToolCall {
            id: String::new(),
            name: super::super::REMEMBER.into(),
            args: json!({"text": "Their shoe size is nine."}),
        };
        // A direct yes to the offer.
        let mut thread = Thread::new();
        thread.person(SHOE, true, now);
        thread.lamp(1, "Shoe size nine, noted; want me to keep that too?", now);
        thread.person("Yes.", true, now);
        assert_eq!(held_for(&thread, 2), Held::Offered(SHOE.to_owned()));
        // The carried yes, met with a second offer, then the yes that runs
        // the held `remember`: still the shoe size.
        thread.lamp(2, "Want me to keep that shoe size for you?", now);
        thread.person("Yes.", true, now);
        let Held::Offered(line) = held_for(&thread, 3) else {
            panic!("{:?}", held_for(&thread, 3));
        };
        assert_eq!(line, SHOE);
        // Its words are the model's own: the model's sentence is kept,
        // never "They said: Yes.".
        assert_eq!(call_kept_with(&model, Some(&line), || None, true), None);
        // A chain of yeses with no offer that has a line of its own keeps
        // nothing.
        let mut thread = Thread::new();
        thread.person("Yes.", true, now);
        thread.lamp(1, "Want me to keep that?", now);
        thread.person("Yes.", true, now);
        assert_eq!(held_for(&thread, 2), Held::YesAlone);
        // A no to the offer keeps nothing.
        let mut thread = Thread::new();
        thread.person(SHOE, true, now);
        thread.lamp(1, "Want me to keep that?", now);
        thread.person("No, thanks.", true, now);
        assert_eq!(held_for(&thread, 2), Held::YesAlone);
        // Thirteen's P2 on #5276: a yes to a second offer the lamp made
        // after a no never walks back past the no to the declined line.
        let mut thread = Thread::new();
        thread.person("My shoe size is nine.", true, now);
        thread.lamp(1, "Want me to keep that?", now);
        thread.person("No.", true, now);
        thread.lamp(2, "Okay. Want me to keep anything else?", now);
        thread.person("Yes.", true, now);
        assert_eq!(held_for(&thread, 3), Held::YesAlone);
        // A bare yes, with no offer, keeps nothing.
        let mut thread = Thread::new();
        thread.person("Yes.", true, now);
        assert_eq!(held_for(&thread, 1), Held::YesAlone);
        // And a bare answer is never what is kept as said.
        for answer in ["Yes.", "Okay", "No.", "Nope, thanks", "Không"] {
            assert!(only_an_answer(answer), "{answer}");
            assert_eq!(
                heard_instead("Their shoe size is nine.", Some(answer), true),
                None
            );
            assert_eq!(call_with(&model, Some(answer), || None), None);
        }
        assert!(!only_a_no("No, my shoe size is ten"));
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

    /// Bug C, lamp-06c1, 5 Nov: a yes alone just after a done keep, replace
    /// or forget is told it is done already, by what was done, never "Nothing
    /// was kept … ask"; with nothing done last it is told [`YES_ALONE`].
    #[test]
    fn a_yes_alone_after_a_done_keep_is_told_it_is_kept_already() {
        let told = yes_alone_told(Some(super::super::MemoryDone::Kept), false);
        assert_eq!(told["remembered"], true);
        assert_eq!(told["already"], true);
        assert_eq!(told["note"], DONE_ALREADY_KEPT);
        assert!(DONE_ALREADY_KEPT.starts_with("It is done already (kept)"));
    }

    #[test]
    fn a_yes_alone_after_a_done_replace_is_told_it_is_changed_already() {
        let told = yes_alone_told(Some(super::super::MemoryDone::Changed), false);
        assert_eq!(told["remembered"], true);
        assert_eq!(told["already"], true);
        assert_eq!(told["note"], DONE_ALREADY_CHANGED);
        assert!(DONE_ALREADY_CHANGED.starts_with("It is done already (changed)"));
    }

    #[test]
    fn a_yes_alone_after_a_done_forget_is_told_it_is_forgotten_already() {
        use super::super::MemoryDone;
        let told = yes_alone_told(Some(MemoryDone::Forgotten), true);
        assert_eq!(told["forgotten"], true);
        assert_eq!(told["already"], true);
        assert_eq!(told["note"], DONE_ALREADY_FORGOTTEN);
        assert!(DONE_ALREADY_FORGOTTEN.starts_with("It is done already (forgotten)"));
        // A remember after a forget is told the forget is done, and keeps
        // nothing.
        let told = yes_alone_told(Some(MemoryDone::Forgotten), false);
        assert_eq!(told["remembered"], false);
        assert_eq!(told["note"], DONE_ALREADY_FORGOTTEN);
    }

    #[test]
    fn a_yes_alone_with_nothing_done_is_told_nothing_was_kept() {
        assert_eq!(
            yes_alone_told(None, false),
            json!({"remembered": false, "note": YES_ALONE})
        );
        for note in [
            DONE_ALREADY_KEPT,
            DONE_ALREADY_CHANGED,
            DONE_ALREADY_FORGOTTEN,
        ] {
            assert!(!note.contains("Nothing was kept"), "{note}");
        }
    }

    /// The reviewer-lead's P2 on #5436: a lone no never takes the
    /// done-already path. "Kept it, right?" then "No.": the forget runs, and
    /// a remember is told nothing was kept.
    #[test]
    fn a_lone_no_after_a_done_keep_runs_its_forget() {
        use super::super::MemoryDone;
        assert!(only_a_no("No.") && !only_a_yes("No."));
        let no = only_a_yes("No.");
        assert_eq!(
            yes_alone_decided(true, no, Some(MemoryDone::Kept), || false),
            None
        );
        assert_eq!(
            yes_alone_decided(false, no, Some(MemoryDone::Kept), || false),
            Some(json!({"remembered": false, "note": YES_ALONE}))
        );
    }

    /// The reviewer-lead's P3 on #5436: [`yes_alone`]'s decision reads the
    /// done write. A yes after one is told it is done already; with none, a
    /// remember is told nothing was kept. A forget is held only after a done
    /// forget with no forget question asked (Heron's and Twelve's P2).
    #[test]
    fn a_yes_alone_is_decided_by_the_done_write() {
        use super::super::MemoryDone;
        for (done, note) in [
            (MemoryDone::Kept, DONE_ALREADY_KEPT),
            (MemoryDone::Changed, DONE_ALREADY_CHANGED),
            (MemoryDone::Forgotten, DONE_ALREADY_FORGOTTEN),
        ] {
            let told = yes_alone_decided(false, true, Some(done), || false).unwrap();
            assert_eq!(told["note"], note);
            assert_eq!(told["already"], true);
            assert_eq!(yes_alone_decided(true, true, Some(done), || true), None);
        }
        // A forget after a done keep or change runs.
        assert_eq!(
            yes_alone_decided(true, true, Some(MemoryDone::Kept), || false),
            None
        );
        assert_eq!(
            yes_alone_decided(true, true, Some(MemoryDone::Changed), || false),
            None
        );
        let told = yes_alone_decided(true, true, Some(MemoryDone::Forgotten), || false).unwrap();
        assert_eq!(told["note"], DONE_ALREADY_FORGOTTEN);
        assert_eq!(told["forgotten"], true);
        assert_eq!(
            yes_alone_decided(false, true, None, || false),
            Some(json!({"remembered": false, "note": YES_ALONE}))
        );
        assert_eq!(yes_alone_decided(true, true, None, || false), None);
    }

    /// Twelve's P2 on #5436, through [`yes_alone_in`] on a thread: after a
    /// done forget, a lone "Yes." to the lamp's "Should I clear the locker
    /// number too?" runs its forget; to "Anything else?" it is held and told
    /// it is forgotten already; a lone "No." runs it either way.
    #[test]
    fn a_yes_to_the_lamps_forget_question_runs_its_forget() {
        use super::super::MemoryDone;
        use crate::thread::Thread;
        let now = Instant::now();
        let after = |lamp: &str, person: &str| {
            let mut thread = Thread::new();
            thread.person("Forget my gym code.", true, now);
            thread.lamp(1, lamp, now);
            thread.person(person, true, now);
            thread
        };
        let forget = |thread: &Thread| {
            yes_alone_in(
                thread,
                (2, None, now),
                true,
                Some((MemoryDone::Forgotten, Some(1))),
            )
        };
        for lamp in [
            "Done. Should I clear the locker number too?",
            "Done. Want me to wipe the locker number too?",
            "Done. Should I get rid of the locker number too?",
            "Xong. Mình bỏ số tủ luôn nhé?",
        ] {
            assert_eq!(forget(&after(lamp, "Yes.")), None, "{lamp}");
        }
        let held = forget(&after("Done. Anything else?", "Yes.")).unwrap();
        assert_eq!(held["note"], DONE_ALREADY_FORGOTTEN);
        assert_eq!(forget(&after("Done. Anything else?", "No.")), None);
    }

    /// The reviewer-lead's P2 on #5436: "drop", "clear" and Vietnamese "bỏ"
    /// ask whether to forget, so a yes to them runs the forget.
    #[test]
    fn the_lamp_asking_to_drop_clear_or_bo_asks_to_forget() {
        for said in [
            "Kept. Should I drop the old Friday note?",
            "Want me to clear the old one?",
            "Should I forget the dentist too?",
            "Shall I delete it?",
            "Mình bỏ ghi chú cũ nhé?",
            "Should I get rid of the old one?",
            "Want me to wipe that?",
            "Mình xóa cái cũ nhé?",
        ] {
            assert!(asks_to_forget(said), "{said}");
        }
        for said in [
            "Kept it, right?",
            "Want me to keep that?",
            "Mondays now, not Fridays.",
        ] {
            assert!(!asks_to_forget(said), "{said}");
        }
    }
    /// lamp-06c1, build 10538, 6 Nov, S13 reqs 338-339: after "Lamp, forget
    /// what I said about my water bottle." was done, "Yes. Please forget it."
    /// was answered with a `recall` and "Nothing was kept from that
    /// exchange… What was it you wanted forgotten?". A line that is only a
    /// yes after a done write is told before it answers that it is done
    /// already, by what was done; never after a question of the lamp's, nor
    /// for a line that is more than a yes, nor with no done write.
    #[test]
    fn a_yes_after_a_done_write_is_told_it_is_done_before_it_answers() {
        use super::super::MemoryDone;
        use crate::thread::Thread;
        let now = Instant::now();
        let after = |lamp: &str, person: &str| {
            let mut thread = Thread::new();
            thread.person("Lamp, forget what I said about my water bottle.", true, now);
            thread.lamp(1, lamp, now);
            thread.person(person, true, now);
            thread
        };
        let forgotten = "That's forgotten \u{2014} nothing about your water bottle is kept now.";
        for (done, note, yes) in [
            (
                MemoryDone::Forgotten,
                DONE_ALREADY_FORGOTTEN,
                "Yes. Please forget it.",
            ),
            (
                MemoryDone::Changed,
                DONE_ALREADY_CHANGED,
                "Yes. Please keep that.",
            ),
            (MemoryDone::Kept, DONE_ALREADY_KEPT, "Yes."),
        ] {
            let (told, was) = done_already_in(
                &after(forgotten, yes),
                (2, None, now),
                Some((done, Some(1))),
            )
            .unwrap();
            assert_eq!(was, done);
            assert!(told.starts_with(DONE_ALREADY_TOLD), "{told}");
            assert!(told.contains(note), "{told}");
        }
        // The forget's answer left out of the thread (S13 req 339): the yes
        // still follows a lamp's line that asked nothing.
        let mut thread = Thread::new();
        thread.person("Yes. Please forget it.", true, now);
        assert!(
            done_already_in(
                &thread,
                (2, None, now),
                Some((MemoryDone::Forgotten, Some(1)))
            )
            .is_some()
        );
        for (lamp, person, done) in [
            (forgotten, "Yes. Please forget it.", None),
            ("Done. Anything else?", "Yes.", Some(MemoryDone::Forgotten)),
            (
                "Your bike is blue. Want me to keep that?",
                "Yes.",
                Some(MemoryDone::Kept),
            ),
            (forgotten, "No.", Some(MemoryDone::Forgotten)),
            (
                "Kept: your bike is blue.",
                "Yes, forget it.",
                Some(MemoryDone::Kept),
            ),
            (
                forgotten,
                "What colour is my water bottle?",
                Some(MemoryDone::Forgotten),
            ),
        ] {
            assert_eq!(
                done_already_in(
                    &after(lamp, person),
                    (2, None, now),
                    done.map(|done| (done, Some(1)))
                ),
                None,
                "{lamp} / {person}"
            );
        }
    }

    /// The Reviewer-lead's P2 on #5607: a done write is "your last turn"'s
    /// only for the yes just after the turn it was done in. "My bike is
    /// blue." was kept (request 1); "My goldfish is called Pip." was
    /// answered "Pip, lovely name." with no `remember` (request 2, as S14
    /// req 363); "Yes. Please keep that." (request 3) is not told that its
    /// last turn kept it, and a `remember` of it is told nothing was kept.
    /// The yes just after the bike's own turn still is.
    #[test]
    fn a_done_write_two_turns_back_is_not_the_yes_last_turn() {
        use super::super::MemoryDone;
        use crate::thread::Thread;
        let now = Instant::now();
        let mut thread = Thread::new();
        thread.person("My bike is blue.", true, now);
        thread.lamp(1, "Kept: your bike is blue.", now);
        thread.person("My goldfish is called Pip.", true, now);
        thread.lamp(2, "Pip, lovely name.", now);
        thread.person("Yes. Please keep that.", true, now);
        let bike = Some((MemoryDone::Kept, Some(1)));
        assert_eq!(done_already_in(&thread, (3, None, now), bike), None);
        assert_eq!(
            yes_alone_in(&thread, (3, None, now), false, bike),
            Some(json!({"remembered": false, "note": YES_ALONE}))
        );
        // Done for no known request: never "your last turn".
        let unknown = Some((MemoryDone::Kept, None));
        assert_eq!(done_already_in(&thread, (3, None, now), unknown), None);
        // Done in the turn just before the yes: told it is done already.
        let pip = Some((MemoryDone::Kept, Some(2)));
        assert!(done_already_in(&thread, (3, None, now), pip).is_some());
        let told = yes_alone_in(&thread, (3, None, now), false, pip).unwrap();
        assert_eq!(told["already"], true, "{told}");
        // The yes just after the bike's own turn, too.
        let mut thread = Thread::new();
        thread.person("My bike is blue.", true, now);
        thread.lamp(1, "Kept: your bike is blue.", now);
        thread.person("Yes. Please keep that.", true, now);
        assert!(done_already_in(&thread, (2, None, now), bike).is_some());
    }

    /// C37 (lamp-06c1, 5 Nov): heard "renunculus", the model wrote
    /// "ranunculus". Its words not heard are each a near spelling of a heard
    /// one, so the keep is asked about; with the same words heard nothing
    /// is asked, and a word near nothing heard follows the heard-line rule.
    #[test]
    fn a_near_spelling_of_a_heard_word_is_asked_about() {
        const HEARD: &str = "Please remember that my favourite flower is a renunculus.";
        let near = near_heard("Their favorite flower is a ranunculus.", HEARD);
        // "favorite" for "favourite" is heard: only the flower is asked.
        assert_eq!(near, [("renunculus".to_owned(), "ranunculus".to_owned())]);
        // Heard exactly: kept as the model wrote it, nothing asked.
        assert!(near_heard("My favourite flower is a renunculus.", HEARD).is_empty());
        assert_eq!(
            heard_instead("My favourite flower is a renunculus.", Some(HEARD), true),
            None
        );
        // A word near nothing heard: the heard line, as ever.
        let tulip = "Their favourite flower is a tulip.";
        assert!(near_heard(tulip, HEARD).is_empty());
        assert_eq!(
            heard_instead(tulip, Some(HEARD), true).as_deref(),
            Some("They said: my favourite flower is a renunculus.")
        );
        // One near word beside one near nothing: the heard line too.
        assert_eq!(
            near_heard("Their favourite plant is a ranunculus.", HEARD),
            []
        );
        // The 20 Oct tea line: "food" is near no heard word.
        assert!(near_heard(WRONG, TEA).is_empty());
        // A line that is only a yes is never held against.
        assert!(near_heard("Ranunculus.", "Yes.").is_empty());
        // More than two near words: the heard line.
        assert!(near_heard("Flawer ranunculus pitals.", "flower renunculus petals").is_empty());
    }

    /// Reviewer-lead on #6850: a word that differs from a heard one only by
    /// an English inflection or a British spelling is heard, never asked
    /// about. Each pair asks nothing alone, and beside a real near word
    /// only that word is asked: the pair counted as heard, not as near
    /// nothing. "ranunculus" for "renunculus" still asks.
    #[test]
    fn an_inflection_or_british_spelling_is_heard_not_near() {
        let flower = [("renunculus".to_owned(), "ranunculus".to_owned())];
        for (model, heard) in [
            ("Likes hiking.", "I like hiking."),
            ("Liking hiking.", "I like hiking."),
            ("Liked hiking.", "I like hiking."),
            ("Works at Google.", "I work at Google."),
            ("Allergic to peanuts.", "I'm allergic to peanut."),
            ("Grows tulips.", "I grow tulip."),
            (
                "Their favorite color is blue.",
                "My favourite colour is blue.",
            ),
            (
                "Their favourite colour is blue.",
                "My favorite color is blue.",
            ),
            ("Watches matches.", "I watch match."),
            ("Stopped smoking.", "I stop smoke."),
        ] {
            assert!(near_heard(model, heard).is_empty(), "{model} / {heard}");
            let model = format!("{} Ranunculus.", model.trim_end_matches('.'));
            let heard = format!("{} Renunculus.", heard.trim_end_matches('.'));
            assert_eq!(near_heard(&model, &heard), flower, "{model} / {heard}");
        }
        assert_eq!(
            near_heard("Grows ranunculus.", "I grow renunculus."),
            flower
        );
        // Not one word written two ways: "hose" and "house", "for" and
        // "four", "fees" and "feed" stay apart.
        assert!(!same_word("house", "hose"));
        assert!(!same_word("four", "for"));
        assert!(!same_word("renunculus", "ranunculus"));
    }

    #[test]
    fn a_question_offering_the_near_word_asks_about_the_held_keep() {
        const HEARD: &str = "Please remember that my favourite flower is a renunculus.";
        const MODEL: &str = "Their favorite flower is a ranunculus.";
        for answer in [
            "Is that ranunculus?",
            "Got it, a flower. Did you mean ranunculus?",
            "Want me to keep that for you?",
        ] {
            assert!(asks_about_near(answer, MODEL, HEARD), "{answer}");
        }
        for answer in [
            "Kept: your favorite flower is a ranunculus.",
            "Is that everything?",
            "Is that a rose?",
        ] {
            assert!(!asks_about_near(answer, MODEL, HEARD), "{answer}");
        }
        let told = near_held_told(&near_heard(MODEL, HEARD));
        assert_eq!(told["remembered"], false);
        // The example asks about the longest near word, the flower.
        assert!(
            told["held"]
                .as_str()
                .unwrap()
                .contains("\"Is that ranunculus?\""),
            "{told}"
        );
    }
}
