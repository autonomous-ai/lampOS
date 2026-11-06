//! Plain facts kept quietly, and few offers to keep (`LAMP_KEEP_QUIETLY`).
//!
//! In a long talk on lamp-06c1 (30 Oct, run 3) 8 of the first 23 turns
//! asked or offered to keep something ("Want me to keep that?", "I can keep
//! that with the dinner, if you'd like.", "keep that too?"), and the same
//! guest's arrival was asked about again and again. A person does not ask
//! leave to remember what a friend tells them (CLAUDE.md rule 8). The
//! memory owner's direction (3 Nov): keep plainly stated facts quietly, ask
//! only when a value is really unclear, never ask about the same thing
//! twice, and never offer to keep things unprompted.
//!
//! All of it holds only in a known person's own scope ([`own`]: a face the
//! eyes know, not one held through a gap). The room's scope, an unknown
//! speaker's and a written turn's stay ask-first, exactly as with the
//! switch off (the Reviewer-lead's P2 on #5053: a guest's or the TV's line
//! must never go quietly into the room's memory). And the scope follows the
//! face in view, not who spoke: a partner speaking off camera while the
//! owner is seen speaks in the owner's scope (the Reviewer-lead's 18:51 P2).
//! So it all holds only for a line the eyes saw that person's own face, and
//! no other, saying ([`seen_saying`], `remember::speaker_of`'s rule over the
//! line's sight, as #5070 gates a service write). A line not seen so stays
//! ask-first: no note, no limit, and a `remember` it did not ask for, that
//! is no yes to the lamp's offer, keeps nothing and the model is told to ask
//! first ([`NOT_SEEN`]). With the switch on, in a known person's own scope,
//! on a line seen said by them:
//!
//! - **The words.** The spoken turn is told [`QUIET_NOTE`] beside the
//!   person's words: a plain, lasting fact about their own life is kept by
//!   a call, without asking and without saying so; never health, money or
//!   other people's private matters unless they clearly ask
//!   (`remember::OBSERVE_PROMPT`'s rule, memory.md). `remember`'s own
//!   description is unchanged.
//! - **A quiet keep is their words** ([`unheard`]): a `remember` the line
//!   did not ask for, and that is no answer to the lamp's offer, is kept
//!   only when every key word of its sentence was heard in the line
//!   (`remember::heard::heard_instead`, `LAMP_MEMORY_KEEP_HEARD`'s check,
//!   whether or not that switch is on). Otherwise nothing is kept: since a
//!   quiet keep is never said back, a word the model put in ("food" for
//!   "tea", lamp-06c1, 20 Oct) could never be heard or put right.
//! - **The code's limit** ([`Quiet`]): a sentence in which the lamp offers
//!   or asks to keep something ([`offers`]) is said at most once a
//!   conversation (counted per person in [`OFFERS`], a conversation ending
//!   after ten minutes with no answer in that scope; a restart resets it),
//!   never about a thing an earlier offer named, and never in an answer to
//!   a line with no fact in it ([`small_talk`]). Others are left out,
//!   logged. A clarification of a doubted word, the face's consent and a
//!   done confirmation are never offers. An offer the code itself asks for
//!   (#4508: a carried line's keep held for the person's yes) is always
//!   said ([`Quiet::held_keep`]). A reply whose every sentence was left out
//!   says the answer with the offer taken out, or [`GOT_IT`]
//!   ([`Quiet::instead`]): never silence.
//!
//! The limit reads English and Vietnamese. A Japanese, Chinese or Korean
//! line is never small talk, and an offer in those languages isn't found,
//! so it is said: there the turn's words alone hold it.
//!
//! Off, nothing changes. Normal turns do a few word searches, no I/O.
use std::{
    collections::VecDeque,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

/// Only the exact value `on`.
pub const ENVIRONMENT: &str = "LAMP_KEEP_QUIETLY";
static ON: OnceLock<bool> = OnceLock::new();

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

/// Told beside the person's words, in a known person's own scope, when the
/// switch is on.
pub const QUIET_NOTE: &str = "When the person you know at the lamp plainly tells you something lasting about their own life (a plan or a date, where they keep something), keep it quietly: call remember for it beside your answer, in their own words, without asking first, and do not say you kept it. Never keep health, money, or other people's private matters unless they clearly ask you to remember them. Never offer to keep something and never ask whether you should; if one word of it is really unclear, ask about that word once, and never again about the same thing.";

/// Said when every sentence of a reply was an offer left out and nothing
/// of it is left to say: never silence (Thirteen's P2 on #5053).
pub const GOT_IT: &str = "Got it.";

/// A known person's own scope: a face the eyes know, not one held only
/// through a gap. `None`: the room's, or no scope.
pub fn own(scope: Option<&crate::memory_scope::ScopeSnapshot>) -> Option<&str> {
    scope
        .filter(|scope| !scope.held())
        .and_then(|scope| scope.person())
}

/// [`own`] for a lease.
pub fn own_for(lease: Option<&crate::memory_scope::ScopeLease>) -> Option<&str> {
    own(lease.and_then(crate::memory_scope::ScopeLease::scope))
}

/// The most lines whose speaker is kept for their answer and calls: far
/// more than run at once.
const MOST_SEEN: usize = 64;

/// The lines a known person's own face, and no other, was seen saying:
/// each request with that person's memory id. In memory only, bounded.
static SEEN: Mutex<VecDeque<(u64, String)>> = Mutex::new(VecDeque::new());

/// Records whether `person` (a memory id) was seen saying request
/// `request`'s line, from the line's sight: the faces seen speaking through
/// it (`speakers`) and the known person with their face (`known`), read
/// when it was heard. Seen only as `remember::speaker_of` names a speaker:
/// their own face alone seen speaking it (#5070's signal). Their face
/// merely in view, another face, two faces or a stale known file is not.
pub fn saw(request: u64, person: &str, speakers: &[String], known: Option<(String, String)>) {
    let line = crate::speaking_faces::LineSeen {
        request,
        speakers: speakers.to_vec(),
        known,
        enrollment: None,
    };
    let seen = crate::remember::speaker_of(Some(&line)).as_deref() == Some(person);
    if let Ok(mut lines) = SEEN.lock() {
        lines.retain(|(of, _)| *of != request);
        if seen {
            lines.push_back((request, person.to_owned()));
            while lines.len() > MOST_SEEN {
                lines.pop_front();
            }
        }
    }
}

/// Whether `person` was seen saying request `request`'s line ([`saw`]).
/// Not read, or no eyes: not seen.
pub fn seen_saying(request: u64, person: &str) -> bool {
    SEEN.lock().is_ok_and(|lines| {
        lines
            .iter()
            .any(|(of, whose)| *of == request && whose == person)
    })
}

/// [`own_for`], only for a line its person was seen saying
/// ([`seen_saying`]): `None` keeps request `request` ask-first.
pub fn own_seen(lease: Option<&crate::memory_scope::ScopeLease>, request: u64) -> Option<&str> {
    own_for(lease).filter(|person| seen_saying(request, person))
}

/// [`QUIET_NOTE`] for request `request`'s spoken turn in `lease`, with the
/// switch on, the scope a known person's own and the line seen said by them
/// ([`own_seen`]); `None` otherwise.
pub fn note_for(lease: Option<&crate::memory_scope::ScopeLease>, request: u64) -> Option<String> {
    (on() && own_seen(lease, request).is_some()).then(|| QUIET_NOTE.to_owned())
}

/// What the model is told for a quiet keep whose sentence had words the
/// person didn't say.
pub const NOT_KEPT: &str = "Nothing was kept: your sentence had words they did not say. To keep it, call remember again with only their own words; do not mention it.";

/// The journal's line for it: no words of it.
pub const NOT_KEPT_LINE: &str = "lamp-voice second: a remember the line did not ask for had words it did not hear, and was not kept (LAMP_KEEP_QUIETLY)";

/// What the model is told for a `remember` the line did not ask for, in a
/// known person's own scope, when they were not seen saying the line: it
/// may be someone else's, so it stays ask-first.
pub const NOT_SEEN: &str = "Nothing was kept: you did not see the person you know say this, so it may be someone else's. If it should be kept, ask them first, once.";

/// The journal's line for it: no words of it.
pub const NOT_SEEN_LINE: &str = "lamp-voice second: a remember the line did not ask for, on a line its person was not seen saying, was not kept: it stays ask-first (LAMP_KEEP_QUIETLY)";

/// Whether a quiet keep of the model's `model` sentence for the heard
/// `line` is refused: the line didn't ask to keep (or took a keep back),
/// it is no answer to the lamp's offer (`offered`), and the line is
/// [`small_talk`] or a key word of the sentence was not heard in it.
pub fn refused(model: &str, line: Option<&str>, offered: bool) -> bool {
    let Some(line) = line else {
        return true;
    };
    if crate::remember::asked::withdrawn(line) {
        return true;
    }
    !offered
        && !crate::remember::asked::asks_to_keep(line)
        && (small_talk(Some(line))
            || crate::remember::heard::heard_instead(model, Some(line), true).is_some())
}

/// Whether a `remember` on a line its person was not seen saying is
/// refused: the line didn't ask to keep (or took a keep back), and it is
/// no answer to the lamp's offer (`offered`). It stays ask-first.
pub fn unseen_refused(line: Option<&str>, offered: bool) -> bool {
    let Some(line) = line else {
        return true;
    };
    crate::remember::asked::withdrawn(line)
        || (!offered && !crate::remember::asked::asks_to_keep(line))
}

/// What the model is told in place of a `remember` of the model's `model`
/// sentence for the heard `line`, in a known person's own scope: on a line
/// they were `seen` saying, [`not_kept`] when [`refused`]; on one they were
/// not, [`not_seen`] when [`unseen_refused`]. `None`: run it, as before.
pub fn refusal(
    model: &str,
    line: Option<&str>,
    offered: bool,
    seen: bool,
) -> Option<serde_json::Value> {
    if seen {
        refused(model, line, offered).then(not_kept)
    } else {
        unseen_refused(line, offered).then(not_seen)
    }
}

/// What the model is told in place of running a `remember` in heard line
/// `request` under `lease`, with the switch on and the scope a known
/// person's own ([`refusal`]): a quiet keep with words not heard, or one on
/// a line they were not seen saying. `None`: run it, as before.
pub fn unheard(
    call: &lamp_gemini::ToolCall,
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
) -> Option<serde_json::Value> {
    if !on() || !heard || call.name != crate::remember::REMEMBER {
        return None;
    }
    let person = own_for(lease)?;
    let seen = seen_saying(request, person);
    let model = call
        .args
        .get("text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let now = Instant::now();
    let told = match crate::thread::THREAD.lock() {
        Ok(thread) => refusal(
            model,
            thread.heard_for(request, now).as_deref(),
            // Their seen yes to a carried line's held keep answers the
            // lamp's offer, told as the room's (lamp-06c1, 6 Nov, C55).
            crate::remember::heard::released(lease).is_some()
                || thread
                    .offered_line(
                        request,
                        Some(person),
                        now,
                        crate::remember::heard::offers_to_keep,
                        crate::remember::heard::only_a_yes,
                    )
                    .is_some(),
            seen,
        ),
        Err(_) => Some(if seen { not_kept() } else { not_seen() }),
    };
    if told.is_some() {
        eprintln!("{}", if seen { NOT_KEPT_LINE } else { NOT_SEEN_LINE });
    }
    told
}

/// What the model is told for a refused quiet keep ([`unheard`]).
pub fn not_kept() -> serde_json::Value {
    serde_json::json!({"remembered": false, "note": NOT_KEPT})
}

/// What the model is told for a keep on a line its person was not seen
/// saying ([`unheard`]).
pub fn not_seen() -> serde_json::Value {
    serde_json::json!({"remembered": false, "note": NOT_SEEN})
}

/// The words by which the lamp keeps something, lower case: never a past
/// one ("noted", "kept", "saved"), which says a thing is done.
const KEEPS: &[&str] = &[
    "keep", "keeping", "remember", "save", "saving", "note", "jot", "store",
];

/// The words after a keep word that make it another keep ("keep the light
/// on", "keep it short", "remember to remind you"): no offer to keep.
const NOT_KEEPING: &[&str] = &[
    "on", "off", "short", "going", "up", "down", "quiet", "warm", "low", "bright", "running",
    "track", "to", "brief", "simple", "dim",
];

/// The words before a keep word by which the lamp is the keeper ("want me
/// to keep", "shall I keep", "I can keep"): "do you remember" and "where
/// do you keep" are not.
const LAMP_KEEPS: &[&str] = &["i", "i'll", "i'm", "i'd", "me"];

/// The words between the keeper and its keep word by which the lamp says
/// it does *not* keep ("I can't remember her name", "I don't remember"):
/// no offer (Thirteen's 18:43 P2 on #5053).
const NOT: &[&str] = &[
    "don't", "dont", "can't", "cant", "cannot", "didn't", "couldn't", "won't", "wouldn't", "not",
    "never", "haven't", "doesn't",
];

/// The Vietnamese words right before a keep word by which the lamp says it
/// does not keep ("Mình không nhớ tên bạn ấy."): no offer.
const NOT_VI: &[&str] = &["kh\u{f4}ng", "ch\u{1b0}a", "ch\u{1eb3}ng"];

/// What a keep is of, that is no fact to offer about: the face's own
/// consent ("May I remember your face?").
const NOT_A_FACT: &[&str] = &["face", "faces", "voice", "m\u{1eb7}t", "gi\u{1ecd}ng"];

/// What after "or" makes it no alternative of a value ("…, or leave it
/// out?", "or not?", "or was that for something else?").
const NOT_AN_ALTERNATIVE: &[&str] = &[
    "not",
    "no",
    "leave",
    "skip",
    "forget",
    "was",
    "is",
    "are",
    "were",
    "should",
    "shall",
    "would",
    "do",
    "did",
    "just",
    "maybe",
    "something",
    "anything",
    "else",
];

/// How a sentence asks the person, besides a question mark.
const ASKS: &[&str] = &[
    "if you'd like",
    "if you would like",
    "if you want",
    "if you like",
    "want me to",
    "like me to",
    "just say",
    "just let me know",
];

/// The lower-case words of `text`, `'` for `’`, a word's `'s` cut.
fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .replace('\u{2019}', "'")
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|word| !word.is_empty())
        .map(|word| word.trim_end_matches("'s").to_owned())
        .collect()
}

/// Whether `text` ends with a question mark, ASCII or full width.
fn ends_asking(text: &str) -> bool {
    text.trim_end_matches(|c: char| c.is_whitespace() || c == '"' || c == '\u{201d}')
        .ends_with(['?', '\u{ff1f}'])
}

/// Whether the lamp's `sentence` asks about a doubted value, never to be
/// left out: two values offered ("Lily or Lilly, which should I note?",
/// "the fourteenth or the fifteenth?"), "which", "did you say", "do you
/// mean"; in Vietnamese "hay", "nào", "đúng", "phải" ("Em có nhớ đúng là
/// thứ Bảy không?").
pub fn clarifies(sentence: &str) -> bool {
    let said = words(sentence);
    let lower = sentence.to_lowercase();
    said.iter().enumerate().any(|(at, word)| {
        word == "or"
            && at > 0
            && said
                .get(at + 1)
                .is_some_and(|next| !NOT_AN_ALTERNATIVE.contains(&next.as_str()))
    }) || ["which", "hay", "n\u{e0}o", "\u{111}\u{fa}ng", "ph\u{1ea3}i"]
        .iter()
        .any(|word| said.iter().any(|said| said == word))
        || ["did you say", "do you mean", "did you mean", "you mean"]
            .iter()
            .any(|ask| lower.contains(ask))
}

/// Whether the lamp's `sentence` offers or asks to keep something: it asks
/// (a question, or "if you'd like", "want me to"), and the lamp is the
/// keeper of a keep word with what it keeps: "Want me to keep that?",
/// "Anything you'd like me to keep about it?", "I can keep that with the
/// dinner, if you'd like.", "Should I add it to what I'm keeping?"; in
/// Vietnamese the lamp ("mình", "tôi", "em") keeping ("nhớ", "lưu", "ghi"),
/// asked ("nhé", "nha", "không", "có muốn"), with or without a question
/// mark. Never: the lamp asking what the person keeps ("Do you remember
/// where you put the keys?", "Where do you keep the spare key?"), another
/// keep ("Shall I keep the light on?", "Want me to remember to remind
/// you?"), a doubted word's ask ([`clarifies`]), the face's consent ("May I
/// remember your face?"), a thing done ("I noted the timer…, just say
/// if you want it changed.", "Mình đã ghi rồi nhé."), or the lamp saying it
/// does not remember ("I can't remember her name, can you tell me?",
/// "Mình không nhớ tên bạn ấy.", "Nếu mình nhớ không nhầm…"; in Vietnamese
/// "không" asks only as the last word).
pub fn offers(sentence: &str) -> bool {
    let said = words(sentence);
    if clarifies(sentence) || said.iter().any(|word| NOT_A_FACT.contains(&word.as_str())) {
        return false;
    }
    let lower = sentence.to_lowercase().replace('\u{2019}', "'");
    let asked = ends_asking(&lower) || ASKS.iter().any(|ask| lower.contains(ask));
    let english = asked
        && said.iter().enumerate().any(|(at, word)| {
            let before = &said[at.saturating_sub(3)..at];
            KEEPS.contains(&word.as_str())
                && before
                    .iter()
                    .rposition(|before| LAMP_KEEPS.contains(&before.as_str()))
                    .is_some_and(|keeper| {
                        !before[keeper + 1..]
                            .iter()
                            .any(|between| NOT.contains(&between.as_str()))
                    })
                && !said
                    .get(at + 1..(at + 4).min(said.len()))
                    .unwrap_or_default()
                    .iter()
                    .any(|next| NOT_KEEPING.contains(&next.as_str()))
        });
    let has = |word: &str| said.iter().any(|said| said == word);
    // "không" asks only as the sentence's last word ("…, có muốn mình nhớ
    // không?"): it is also the plain "not" ("Mình không nhớ tên bạn ấy.",
    // "Nếu mình nhớ không nhầm…"), Thirteen's 18:43 P2 on #5053.
    let keeps_vi = ["nh\u{1edb}", "l\u{1b0}u", "ghi"];
    let negated = said
        .windows(2)
        .any(|pair| NOT_VI.contains(&pair[0].as_str()) && keeps_vi.contains(&pair[1].as_str()))
        || lower.contains("nh\u{1edb} kh\u{f4}ng nh\u{1ea7}m");
    let vietnamese = (has("m\u{ec}nh") || has("t\u{f4}i") || has("em"))
        && keeps_vi.iter().any(|keep| has(keep))
        && (has("nh\u{e9}")
            || has("nha")
            || said.last().is_some_and(|last| last == "kh\u{f4}ng")
            || lower.contains("c\u{f3} mu\u{1ed1}n")
            || ends_asking(&lower))
        && !negated
        && !has("\u{111}\u{e3}")
        && !has("r\u{1ed3}i");
    english || vietnamese
}

/// Words too common to name a thing ("that", "the", "you"), the keep
/// words, and the words of a time ("tomorrow", "Friday", "two"): what is
/// left of an offer names what it offered to keep (the Reviewer-lead's P3
/// on #5053: "keep Lily's arrival at two?" must not stop every later offer
/// naming "two").
const COMMON: &[&str] = &[
    "the",
    "and",
    "for",
    "you",
    "your",
    "that",
    "this",
    "with",
    "what",
    "want",
    "would",
    "like",
    "should",
    "shall",
    "can",
    "could",
    "also",
    "too",
    "about",
    "them",
    "those",
    "it",
    "its",
    "keep",
    "keeping",
    "kept",
    "remember",
    "save",
    "note",
    "noted",
    "anything",
    "something",
    "else",
    "there",
    "here",
    "then",
    "just",
    "from",
    "have",
    "are",
    "was",
    "were",
    "will",
    "into",
    "onto",
    "add",
    "list",
    "what's",
    "i'm",
    "i'll",
    "me",
    "or",
    "leave",
    "out",
    "time",
    "way",
    "one",
    "all",
    // Times and days.
    "today",
    "tonight",
    "tomorrow",
    "yesterday",
    "morning",
    "afternoon",
    "evening",
    "night",
    "week",
    "weekend",
    "month",
    "year",
    "next",
    "last",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "o'clock",
    "noon",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "half",
    "quarter",
    // Vietnamese: the lamp, the person, keeping, asking, a day.
    "m\u{ec}nh",
    "t\u{f4}i",
    "b\u{1ea1}n",
    "nh\u{1edb}",
    "l\u{1b0}u",
    "ghi",
    "nh\u{e9}",
    "nha",
    "kh\u{f4}ng",
    "mu\u{1ed1}n",
    "cho",
    "n\u{e0}y",
    "\u{111}\u{f3}",
    "l\u{1ea1}i",
    "lu\u{f4}n",
    "gi\u{fa}p",
    "\u{111}\u{1b0}\u{1ee3}c",
    "ng\u{e0}y",
    "mai",
    "gi\u{1edd}",
];

/// What an offer names: its words of three letters or more that are not
/// [`COMMON`].
fn named(sentence: &str) -> Vec<String> {
    words(sentence)
        .into_iter()
        .filter(|word| word.chars().count() >= 3 && !COMMON.contains(&word.as_str()))
        .collect()
}

/// Whether two offers name the same thing: a named word in both.
pub fn same_thing(offer: &str, earlier: &str) -> bool {
    let earlier = named(earlier);
    named(offer).iter().any(|word| earlier.contains(word))
}

/// How a line asking something begins: what, who, when, where, why, how,
/// which, is, are, do, does, did. "Can you…", "Could you…", "Tell me…"
/// ask the lamp to do something, and may carry a fact.
const QUESTION_WORDS: &[&str] = &[
    "what", "who", "when", "where", "why", "how", "which", "is", "are", "do", "does", "did",
];

/// Words that say nothing lasting: greetings, thanks, fillers.
const CHAT_WORDS: &[&str] = &[
    "hi", "hello", "hey", "lamp", "okay", "ok", "thanks", "thank", "you", "yes", "yeah", "no",
    "sure", "good", "great", "nice", "well", "morning", "evening", "night", "bye", "right", "cool",
    "fine", "the", "and", "it", "is", "was", "that", "this", "so", "just", "too", "really", "very",
    "today", "now", "again", "i'll", "i'm", "it's", "some", "put", "on",
];

/// The words by which a person's line asks the lamp to keep something.
const ASKS_TO_KEEP: &[&str] = &[
    "remember",
    "note",
    "forget",
    "nh\u{1edb}",
    "ghi",
    "l\u{1b0}u",
];

/// Whether `text` has a letter of a script written without spaces between
/// words, or one the word lists don't read (Japanese, Chinese, Korean).
fn unspaced(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c,
            '\u{3040}'..='\u{30ff}'
            | '\u{3400}'..='\u{4dbf}'
            | '\u{4e00}'..='\u{9fff}'
            | '\u{1100}'..='\u{11ff}'
            | '\u{3130}'..='\u{318f}'
            | '\u{ac00}'..='\u{d7af}')
    })
}

/// Whether the person's `line` is small talk, with nothing in it to keep: a
/// question (it ends asking, or begins with a question word), or fewer than
/// three words that are not [`CHAT_WORDS`]. "Okay, thanks.", "How long
/// should I marinate it?" and "Nice weather today." are; "My sister Lily is
/// coming on Saturday." and "Can you remember my dentist is on Tuesday?"
/// (it asks to keep) are not. No line heard (a note in writing), or one in
/// Japanese, Chinese or Korean: not small talk.
pub fn small_talk(line: Option<&str>) -> bool {
    let Some(line) = line else {
        return false;
    };
    let said = words(line);
    if unspaced(line)
        || said
            .iter()
            .any(|word| ASKS_TO_KEEP.contains(&word.as_str()))
    {
        return false;
    }
    if ends_asking(line)
        || said
            .first()
            .is_some_and(|first| QUESTION_WORDS.contains(&first.as_str()))
    {
        return true;
    }
    said.iter()
        .filter(|word| !CHAT_WORDS.contains(&word.as_str()))
        .count()
        < 3
}

/// The sentences of the lamp's words, at `.`, `!` and `?`, full width too.
pub fn sentences(text: &str) -> Vec<String> {
    let mut all = Vec::new();
    let mut sentence = String::new();
    for c in text.chars() {
        sentence.push(c);
        if matches!(c, '.' | '!' | '?' | '\u{3002}' | '\u{ff01}' | '\u{ff1f}') {
            if sentence.chars().any(char::is_alphanumeric) {
                all.push(sentence.trim().to_owned());
            }
            sentence.clear();
        }
    }
    if sentence.chars().any(char::is_alphanumeric) {
        all.push(sentence.trim().to_owned());
    }
    all
}

/// An offer's `sentence` with its offer taken out: its clauses (at `,`,
/// `;`, `:`, a dash) before the first that keeps or asks, when they have
/// two words or more ("Lily at two, want me to keep that?" is "Lily at
/// two."). `None` when the offer is the whole of it.
pub fn without_offer(sentence: &str) -> Option<String> {
    let mut kept = String::new();
    for clause in sentence.split([',', ';', ':', '\u{2014}', '\u{2013}', '\u{3001}']) {
        let lower = clause.to_lowercase().replace('\u{2019}', "'");
        let said = words(clause);
        if said.iter().any(|word| KEEPS.contains(&word.as_str()))
            || ASKS.iter().any(|ask| lower.contains(ask))
            || ends_asking(clause)
            || offers(clause)
        {
            break;
        }
        if !kept.is_empty() {
            kept.push(',');
        }
        kept.push_str(clause);
    }
    let kept = kept.trim().trim_end_matches([',', ' ']);
    (words(kept).len() >= 2).then(|| format!("{kept}."))
}

/// Why an offer was left out, for the journal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeftOut {
    /// One was said already in this conversation.
    Again,
    /// An earlier offer named the same thing.
    SameThing,
    /// The line it answers has nothing in it to keep.
    SmallTalk,
}

impl LeftOut {
    pub fn why(self) -> &'static str {
        match self {
            Self::Again => "one was offered already in this conversation",
            Self::SameThing => "an earlier offer named the same thing",
            Self::SmallTalk => "its line has nothing to keep",
        }
    }
}

/// The most people whose offers are counted at once, and the most offers
/// kept for each: past these the oldest go.
const PEOPLE: usize = 8;
const OFFERS_KEPT: usize = 8;

/// One person's offers.
#[derive(Debug)]
struct Talk {
    person: String,
    /// When an answer in this scope was last begun, or an offer said.
    last: Instant,
    /// The offers said in the conversation going on.
    in_talk: Vec<String>,
    /// Those said in earlier ones, newest last.
    before: VecDeque<String>,
}

/// The offers the lamp said, per known person's own scope and conversation,
/// in this process (Thirteen's P2 on #5053: the thread's twelve exchanges
/// don't reach back a whole talk). A conversation ends after `quiet` with
/// no answer begun in that scope. A restart resets it.
#[derive(Debug, Default)]
pub struct Offers {
    talks: VecDeque<Talk>,
}

impl Offers {
    pub const fn new() -> Self {
        Self {
            talks: VecDeque::new(),
        }
    }

    /// `person`'s talk, made when new, its conversation ended when `quiet`
    /// passed since its last; now its last.
    fn talk(&mut self, person: &str, now: Instant, quiet: Duration) -> &mut Talk {
        let at = match self.talks.iter().position(|talk| talk.person == person) {
            Some(at) => at,
            None => {
                if self.talks.len() >= PEOPLE {
                    let oldest = self
                        .talks
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, talk)| talk.last)
                        .map_or(0, |(at, _)| at);
                    self.talks.remove(oldest);
                }
                self.talks.push_back(Talk {
                    person: person.to_owned(),
                    last: now,
                    in_talk: Vec::new(),
                    before: VecDeque::new(),
                });
                self.talks.len() - 1
            }
        };
        let talk = &mut self.talks[at];
        if now.saturating_duration_since(talk.last) > quiet {
            let ended = std::mem::take(&mut talk.in_talk);
            talk.before.extend(ended);
            while talk.before.len() > OFFERS_KEPT {
                talk.before.pop_front();
            }
        }
        talk.last = talk.last.max(now);
        talk
    }

    /// The offers said to `person` in the conversation going on, and in
    /// earlier ones, as an answer begins `now`.
    pub fn read(
        &mut self,
        person: &str,
        now: Instant,
        quiet: Duration,
    ) -> (Vec<String>, Vec<String>) {
        let talk = self.talk(person, now, quiet);
        (talk.in_talk.clone(), talk.before.iter().cloned().collect())
    }

    /// An offer said to `person`.
    pub fn said(&mut self, person: &str, offer: &str, now: Instant, quiet: Duration) {
        let talk = self.talk(person, now, quiet);
        if talk.in_talk.len() >= OFFERS_KEPT {
            talk.in_talk.remove(0);
        }
        talk.in_talk.push(offer.to_owned());
    }
}

/// This process's [`Offers`].
pub static OFFERS: Mutex<Offers> = Mutex::new(Offers::new());

/// One answer's limit on offers to keep, with the switch on.
#[derive(Debug, Default)]
pub struct Quiet {
    on: bool,
    /// Whose own scope it is: an offer said is counted for them in
    /// [`OFFERS`].
    person: Option<String>,
    /// The offers the lamp said in this conversation, newest last.
    in_talk: Vec<String>,
    /// The offers it said earlier, in this person's earlier conversations.
    before: Vec<String>,
    small_talk: bool,
    /// A held keep asked for the person's yes (#4508): its offer is said.
    held: bool,
    pub left_out: Vec<LeftOut>,
    /// A sentence of the answer was said.
    spoke: bool,
    /// The first offer left out, without its offer ([`without_offer`]).
    rest: Option<String>,
    cut: bool,
}

impl Quiet {
    /// Off: every sentence is said.
    pub fn off() -> Self {
        Self::default()
    }

    /// With the switch on: `in_talk` the lamp's words in this conversation,
    /// `before` its words in earlier ones, `line` the person's line this
    /// answers.
    pub fn new(in_talk: &[String], before: &[String], line: Option<&str>) -> Self {
        let offered = |texts: &[String]| {
            texts
                .iter()
                .flat_map(|text| sentences(text))
                .filter(|sentence| offers(sentence))
                .collect::<Vec<_>>()
        };
        Self {
            on: true,
            in_talk: offered(in_talk),
            before: offered(before),
            small_talk: small_talk(line),
            ..Self::default()
        }
    }

    /// The limit for `person`'s own scope: an offer said is counted for them.
    pub fn for_person(mut self, person: &str) -> Self {
        self.person = Some(person.to_owned());
        self
    }

    /// A keep of this answer is held for the person's yes (#4508): the
    /// offer it asks for is said.
    pub fn held_keep(&mut self) {
        self.held = true;
    }

    /// Whether `sentence` is said: all but an offer to keep left out
    /// ([`LeftOut`]).
    pub fn say(&mut self, sentence: String) -> Option<String> {
        if !self.on || self.held || !offers(&sentence) {
            return Some(sentence);
        }
        let why = if !self.in_talk.is_empty() {
            Some(LeftOut::Again)
        } else if self
            .before
            .iter()
            .any(|earlier| same_thing(&sentence, earlier))
        {
            Some(LeftOut::SameThing)
        } else if self.small_talk {
            Some(LeftOut::SmallTalk)
        } else {
            None
        };
        match why {
            Some(why) => {
                self.left_out.push(why);
                if !self.cut {
                    self.rest = without_offer(&sentence);
                }
                self.cut = true;
                None
            }
            None => {
                self.in_talk.push(sentence.clone());
                Some(sentence)
            }
        }
    }

    /// `sentence` went to be said: an offer in it is counted for this
    /// person's conversation.
    pub fn spoken(&mut self, sentence: &str) {
        self.spoke = true;
        if self.on
            && let Some(person) = &self.person
            && offers(sentence)
            && let Ok(mut offers) = OFFERS.lock()
        {
            offers.said(
                person,
                sentence,
                Instant::now(),
                crate::thread::REMEMBERED_FOR,
            );
        }
    }

    /// What is said instead when the answer is ending with nothing said
    /// and an offer was left out: the first one's answer without its offer,
    /// or `got_it`. `None` otherwise.
    pub fn instead(&mut self, got_it: &str) -> Option<String> {
        if !self.on || self.spoke || !self.cut {
            return None;
        }
        self.spoke = true;
        Some(self.rest.take().unwrap_or_else(|| got_it.to_owned()))
    }

    /// The journal's line for what was left out since the last, if any: no
    /// words of it.
    pub fn take_line(&mut self, request: u64) -> Option<String> {
        let left_out = std::mem::take(&mut self.left_out);
        let first = left_out.first()?;
        Some(format!(
            "lamp-voice second: request {request}: {} offer(s) to keep not said: {} ({ENVIRONMENT})",
            left_out.len(),
            first.why()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offers_are_the_lamp_asking_to_keep_and_other_keeps_are_not() {
        for offer in [
            "Want me to keep that noted?",
            "Want me to keep that\u{ff1f}",
            "I'll keep that too, but first, should I add Lily's arrival to what I'm keeping, or leave it out?",
            "Anything you'd like me to keep about it, the time, the menu?",
            "Lemongrass chicken for the six on Friday, I can keep that with the dinner, if you'd like.",
            "Want me to keep the project with its deadline?",
            "Should I remember your birthday?",
            "Lily at two, want me to keep that?",
            "Shall I keep Lily's arrival at two?",
            // Vietnamese, with and without a question mark.
            "M\u{ec}nh ghi nh\u{1edb} nh\u{e9}.",
            "B\u{1ea1}n c\u{f3} mu\u{1ed1}n m\u{ec}nh nh\u{1edb} chuy\u{1ec7}n n\u{e0}y kh\u{f4}ng?",
            "\u{110}\u{1ec3} m\u{ec}nh l\u{1b0}u l\u{1ea1}i nh\u{e9}?",
        ] {
            assert!(offers(offer), "{offer}");
        }
        for not in [
            "Kept: Pine Hill Hotel, Room 204.",
            "Shall I keep the light on?",
            "Want me to remember to remind you?",
            "Shall I start it?",
            "Is she coming to yours first, or should I remind you nearer the time?",
            "Say it again in a few words and I'll keep it with the dinner.",
            "Keep it instrumental under the talking.",
            "How wide is the table?",
            "I'll keep it short.",
            // The HOLDs on #5053: the lamp asking what the person keeps.
            "Do you remember where you put the keys?",
            "Where do you keep the spare key?",
            // The doubted word's ask that QUIET_NOTE itself requires.
            "Lily or Lilly, which should I note?",
            "Was it the fourteenth or the fifteenth, so I can note it right?",
            "Did you say Lily, so I can note it?",
            "Em c\u{f3} nh\u{1edb} \u{111}\u{fa}ng l\u{e0} th\u{1ee9} B\u{1ea3}y kh\u{f4}ng?",
            "Lily hay Lilly, m\u{ec}nh n\u{ea}n ghi t\u{ea}n n\u{e0}o?",
            // The onboarding face consent.
            "May I remember your face, so I know you next time?",
            "M\u{ec}nh nh\u{1edb} khu\u{f4}n m\u{1eb7}t b\u{1ea1}n nh\u{e9}, \u{111}\u{1ec3} l\u{1ea7}n sau nh\u{1ead}n ra b\u{1ea1}n?",
            // A thing done.
            "I noted the timer for ten minutes, just say if you want it changed.",
            "M\u{ec}nh \u{111}\u{e3} ghi h\u{1eb9}n gi\u{1edd} m\u{1b0}\u{1edd}i ph\u{fa}t, c\u{1ea7}n \u{111}\u{1ed5}i th\u{ec} c\u{1ee9} n\u{f3}i nh\u{e9}.",
            // The lamp asking the person, in Vietnamese.
            "B\u{1ea1}n c\u{f3} nh\u{1edb} \u{111}\u{1ec3} ch\u{ec}a kh\u{f3}a \u{1edf} \u{111}\u{e2}u kh\u{f4}ng?",
            "B\u{1ea1}n c\u{1ea5}t ch\u{ec}a kh\u{f3}a d\u{1ef1} ph\u{f2}ng \u{1edf} \u{111}\u{e2}u?",
            // Not the lamp keeping: no I or me.
            "And the six million dong budget for the trip: keep that too?",
            // Japanese: not read.
            "\u{899a}\u{3048}\u{3066}\u{304a}\u{304d}\u{307e}\u{3057}\u{3087}\u{3046}\u{304b}\u{ff1f}",
            // Thirteen's 18:43 P2: the lamp saying it does not remember.
            "I can't remember her name, can you tell me?",
            "I don\u{2019}t remember that, can you say it again?",
            "Sorry, I cannot remember what you told me?",
            "M\u{ec}nh kh\u{f4}ng nh\u{1edb} t\u{ea}n b\u{1ea1}n \u{1ea5}y.",
            "N\u{1ebf}u m\u{ec}nh nh\u{1edb} kh\u{f4}ng nh\u{1ea7}m th\u{ec} Lily \u{111}\u{1ebf}n th\u{1ee9} B\u{1ea3}y.",
            "M\u{ec}nh ch\u{1b0}a nh\u{1edb} t\u{ea}n b\u{1ea1}n \u{1ea5}y, b\u{1ea1}n nh\u{1eaf}c m\u{ec}nh nh\u{e9}?",
        ] {
            assert!(!offers(not), "{not}");
        }
    }

    #[test]
    fn small_talk_is_a_question_or_a_line_with_nothing_to_keep() {
        for chat in [
            Some("Okay, thanks."),
            Some("How long should I marinate lemongrass chicken?"),
            Some("Do you think six people is too many"),
            Some("Nice weather today."),
            Some("Okay, I'll put on some jazz."),
            Some("Lily."),
        ] {
            assert!(small_talk(chat), "{chat:?}");
        }
        for fact in [
            Some("My sister Lily is coming to visit on Saturday."),
            Some("We booked the Pine Hill hotel, room two oh four."),
            Some("I keep the spare key in the blue jar by the door."),
            // The HOLD: an ask to keep, with or without a question mark.
            Some("Can you remember my dentist is on Tuesday"),
            Some("Can you remember my dentist is on Tuesday?"),
            Some("Could you book a table for six at the Pine Hill on Friday"),
            Some(
                "Nh\u{1edb} gi\u{fa}p m\u{ec}nh l\u{e0} th\u{1ee9} Ba m\u{ec}nh \u{111}i nha s\u{129} nh\u{e9}?",
            ),
            // Japanese, Chinese, Korean: never judged small talk.
            Some(
                "\u{59b9}\u{306f}\u{571f}\u{66dc}\u{65e5}\u{306b}\u{6765}\u{307e}\u{3059}\u{3002}",
            ),
            Some("\u{c548}\u{b155}"),
            None,
        ] {
            assert!(!small_talk(fact), "{fact:?}");
        }
    }

    #[test]
    fn an_offer_is_said_once_a_conversation_never_twice_about_a_thing_nor_on_small_talk() {
        let fact = Some("We booked the Pine Hill hotel, room 204.");
        // The first offer of a talk, on a fact, is said; a second in the same
        // answer is not.
        let mut quiet = Quiet::new(&[], &[], fact);
        assert_eq!(
            quiet.say("Want me to keep that?".into()).as_deref(),
            Some("Want me to keep that?")
        );
        assert_eq!(quiet.say("Shall I keep Lily's train too?".into()), None);
        assert_eq!(quiet.left_out, [LeftOut::Again]);
        assert!(
            quiet
                .take_line(7)
                .unwrap()
                .contains("1 offer(s) to keep not said: one was offered")
        );
        assert!(quiet.take_line(7).is_none());
        // One said earlier in the talk: none now; the rest is said.
        let talk = ["Got it. Want me to keep that noted?".to_owned()];
        let mut quiet = Quiet::new(&talk, &[], fact);
        assert_eq!(quiet.say("Shall I keep the hotel?".into()), None);
        assert_eq!(
            quiet.say("The hotel is near the lake.".into()).as_deref(),
            Some("The hotel is near the lake.")
        );
        // The same thing offered in an earlier talk: never again.
        let before = ["Shall I keep Lily's arrival at two?".to_owned()];
        let mut quiet = Quiet::new(&[], &before, fact);
        assert_eq!(quiet.say("Want me to keep Lily's train?".into()), None);
        assert_eq!(quiet.left_out, [LeftOut::SameThing]);
        assert_eq!(
            quiet
                .say("Want me to keep the hotel room?".into())
                .as_deref(),
            Some("Want me to keep the hotel room?")
        );
        // Small talk: none.
        let mut quiet = Quiet::new(&[], &[], Some("Okay, thanks."));
        assert_eq!(quiet.say("Want me to keep that?".into()), None);
        assert_eq!(quiet.left_out, [LeftOut::SmallTalk]);
        // A held keep's own ask (#4508) is always said.
        let mut quiet = Quiet::new(&talk, &[], Some("Okay."));
        quiet.held_keep();
        assert!(quiet.say("Want me to keep that for you?".into()).is_some());
        // Off: all said.
        let mut quiet = Quiet::off();
        assert!(quiet.say("Want me to keep that?".into()).is_some());
    }

    /// The HOLDs on #5053: after an earlier offer, and on small talk, the
    /// lines that must be said still are, in English and Vietnamese.
    #[test]
    fn needed_lines_are_said_after_an_offer_and_on_small_talk() {
        let talk = ["Want me to keep that?".to_owned()];
        for line in [
            Some("Okay, thanks."),
            Some("Lily."),
            Some("Saturday or Sunday."),
        ] {
            let mut quiet = Quiet::new(&talk, &talk, line);
            for needed in [
                "Do you remember where you put the keys?",
                "Where do you keep the spare key?",
                "Lily or Lilly, which should I note?",
                "Em c\u{f3} nh\u{1edb} \u{111}\u{fa}ng l\u{e0} th\u{1ee9} B\u{1ea3}y kh\u{f4}ng?",
                "May I remember your face, so I know you next time?",
                "I noted the timer for ten minutes, just say if you want it changed.",
                "M\u{ec}nh \u{111}\u{e3} ghi h\u{1eb9}n gi\u{1edd} m\u{1b0}\u{1edd}i ph\u{fa}t, c\u{1ea7}n \u{111}\u{1ed5}i th\u{ec} c\u{1ee9} n\u{f3}i nh\u{e9}.",
            ] {
                assert_eq!(
                    quiet.say(needed.into()).as_deref(),
                    Some(needed),
                    "{line:?}"
                );
            }
            assert!(quiet.left_out.is_empty());
            // A Vietnamese offer with no question mark is held as an English one.
            assert_eq!(quiet.say("M\u{ec}nh ghi nh\u{1edb} nh\u{e9}.".into()), None);
        }
    }

    /// Thirteen's P2: a reply whose only sentence is an offer left out
    /// says the answer without it, or a short acknowledgement; never
    /// nothing.
    #[test]
    fn a_reply_cut_to_nothing_says_its_answer_or_got_it() {
        let talk = ["Want me to keep that?".to_owned()];
        let line = Some("Lily lands at two tomorrow.");
        let mut quiet = Quiet::new(&talk, &[], line);
        assert_eq!(quiet.say("Lily at two, want me to keep that?".into()), None);
        assert_eq!(quiet.instead(GOT_IT).as_deref(), Some("Lily at two."));
        assert_eq!(quiet.instead(GOT_IT), None);
        // The offer alone: the acknowledgement.
        let mut quiet = Quiet::new(&talk, &[], line);
        assert_eq!(quiet.say("Want me to keep that?".into()), None);
        assert_eq!(quiet.instead(GOT_IT).as_deref(), Some(GOT_IT));
        // Something was said: nothing more.
        let mut quiet = Quiet::new(&talk, &[], line);
        assert_eq!(quiet.say("Want me to keep that?".into()), None);
        quiet.spoken("Lovely, two it is.");
        assert_eq!(quiet.instead(GOT_IT), None);
        // Nothing left out, or off: nothing.
        assert_eq!(Quiet::new(&talk, &[], line).instead(GOT_IT), None);
        assert_eq!(Quiet::off().instead(GOT_IT), None);
        assert_eq!(
            without_offer(
                "Lemongrass chicken for the six on Friday, I can keep that with the dinner, if you'd like."
            )
            .as_deref(),
            Some("Lemongrass chicken for the six on Friday.")
        );
        assert_eq!(without_offer("Shall I keep Lily's arrival at two?"), None);
        assert_eq!(
            without_offer("Th\u{1ee9} B\u{1ea3}y, m\u{ec}nh ghi nh\u{1edb} nh\u{e9}.").as_deref(),
            Some("Th\u{1ee9} B\u{1ea3}y.")
        );
        // Thirteen's 18:43 P2: a small-talk question answered "I don't
        // remember" is said whole, on its own and after an earlier offer:
        // never cut to "Got it.".
        for (question, answer) in [
            (
                "What's my sister's name?",
                "I can't remember her name, can you tell me?",
            ),
            (
                "Em g\u{e1}i m\u{ec}nh t\u{ea}n g\u{EC}?",
                "M\u{ec}nh kh\u{f4}ng nh\u{1edb} t\u{ea}n b\u{1ea1}n \u{1ea5}y.",
            ),
            (
                "Lily \u{111}\u{1ebf}n h\u{f4}m n\u{e0}o?",
                "N\u{1ebf}u m\u{ec}nh nh\u{1edb} kh\u{f4}ng nh\u{1ea7}m th\u{ec} Lily \u{111}\u{1ebf}n th\u{1ee9} B\u{1ea3}y.",
            ),
        ] {
            assert!(small_talk(Some(question)), "{question}");
            for earlier in [&[][..], &talk[..]] {
                let mut quiet = Quiet::new(earlier, earlier, Some(question));
                assert_eq!(quiet.say(answer.into()).as_deref(), Some(answer));
                quiet.spoken(answer);
                assert!(quiet.left_out.is_empty(), "{answer}");
                assert_eq!(quiet.instead(GOT_IT), None, "{answer}");
            }
        }
    }

    /// Thirteen's P2: "once a conversation" is counted per person's scope,
    /// however many exchanges, until ten minutes with no answer in it.
    #[test]
    fn offers_are_counted_per_person_and_conversation_past_the_window() {
        let quiet = Duration::from_secs(600);
        let mut offers = Offers::new();
        let then = Instant::now();
        assert_eq!(offers.read("p1", then, quiet), (vec![], vec![]));
        offers.said("p1", "Want me to keep Lily's train?", then, quiet);
        // Thirty answers later, a minute apart: still this conversation.
        let mut now = then;
        for _ in 0..30 {
            now += Duration::from_secs(60);
            let (in_talk, before) = offers.read("p1", now, quiet);
            assert_eq!(in_talk, ["Want me to keep Lily's train?"]);
            assert!(before.is_empty());
        }
        // Another person's own scope has none of them.
        assert_eq!(offers.read("p2", now, quiet), (vec![], vec![]));
        // After ten quiet minutes the talk is an earlier one.
        let later = now + Duration::from_secs(601);
        assert_eq!(
            offers.read("p1", later, quiet),
            (vec![], vec!["Want me to keep Lily's train?".to_owned()])
        );
        // Bounded: the oldest person goes past PEOPLE.
        for at in 0..PEOPLE + 2 {
            offers.said(&format!("q{at}"), "Want me to keep that?", later, quiet);
        }
        assert_eq!(offers.talks.len(), PEOPLE);
        for _ in 0..OFFERS_KEPT + 3 {
            offers.said("q9", "Want me to keep that?", later, quiet);
        }
        assert!(
            offers
                .talks
                .iter()
                .all(|talk| talk.in_talk.len() <= OFFERS_KEPT)
        );
    }

    #[test]
    fn same_thing_is_never_a_time_or_a_day() {
        let earlier = "Shall I keep Lily's arrival at two tomorrow?";
        assert!(same_thing("Want me to keep Lily's train?", earlier));
        for other in [
            "Want me to keep the dentist at two?",
            "Shall I keep the plumber tomorrow?",
            "Should I remember the dinner on Friday?",
            "M\u{ec}nh ghi nh\u{1edb} nh\u{e9}?",
        ] {
            assert!(!same_thing(other, earlier), "{other}");
        }
        assert!(!same_thing(
            "M\u{ec}nh ghi nh\u{1edb} kh\u{e1}ch s\u{1ea1}n nh\u{e9}?",
            "M\u{ec}nh l\u{1b0}u ng\u{e0}y sinh nh\u{1ead}t nh\u{e9}?"
        ));
    }

    #[test]
    fn sentences_split_at_their_ends() {
        assert_eq!(
            sentences("Kept: Pine Hill. Want me to keep Lily's train too? Sure"),
            [
                "Kept: Pine Hill.",
                "Want me to keep Lily's train too?",
                "Sure"
            ]
        );
        assert_eq!(
            sentences("\u{306f}\u{3044}\u{3002}\u{3044}\u{3044}\u{ff1f}").len(),
            2
        );
    }

    /// The Reviewer-lead's P1: the words never keep health, money or other
    /// people's private matters unasked, as `OBSERVE_PROMPT`, and name no
    /// such thing as an example.
    #[test]
    fn the_words_say_keep_quietly_never_private_and_never_offer() {
        assert!(QUIET_NOTE.contains("keep it quietly"));
        assert!(QUIET_NOTE.contains("in their own words"));
        assert!(QUIET_NOTE.contains("Never offer to keep"));
        assert!(QUIET_NOTE.contains(
            "Never keep health, money, or other people's private matters unless they clearly ask you to remember them."
        ));
        assert!(
            crate::remember::OBSERVE_PROMPT
                .contains("Never keep health, money, or other people's private matters")
        );
        for example in ["allerg", "someone's name", "the name of someone"] {
            assert!(!QUIET_NOTE.contains(example), "{example}");
        }
    }

    /// Only a known face's own scope: never the room's, one held through a
    /// gap, or none.
    #[test]
    fn only_a_known_persons_own_scope_keeps_quietly() {
        let person = crate::memory_scope::ScopeSnapshot::of("Lily");
        assert!(own(Some(&person)).is_some());
        assert_eq!(own(Some(&crate::memory_scope::ScopeSnapshot::room())), None);
        assert_eq!(own(None), None);
        assert_eq!(own_for(None), None);
        assert_eq!(note_for(None, 0), None);
    }

    /// The Reviewer-lead's 18:51 P2: the owner in view but not speaking, a
    /// second voice's plain fact keeps nothing quietly: their line is not
    /// seen said by them, so it stays ask-first. Seen only as
    /// `remember::speaker_of` names them (#5070's signal).
    #[test]
    fn a_line_its_person_was_not_seen_saying_keeps_nothing_quietly() {
        let mai = crate::known_faces::memory_person("Mai");
        let known = || Some(("Mai".to_owned(), "b1/f1".to_owned()));
        let request = 5_053_001;
        let fact = "I'm starting a new job at the bank on Monday.";
        // The owner's face in view, none seen speaking: a second voice.
        saw(request, &mai, &[], known());
        assert!(!seen_saying(request, &mai));
        assert_eq!(
            refusal(fact, Some(fact), false, seen_saying(request, &mai)),
            Some(not_seen())
        );
        // Another face seen speaking it, or two: not seen either.
        for speakers in [&["b1/f2".to_owned()][..], &["b1/f1".into(), "b1/f2".into()]] {
            saw(request, &mai, speakers, known());
            assert!(!seen_saying(request, &mai), "{speakers:?}");
        }
        // Their own face alone seen saying it: a quiet keep of their heard
        // words, as before.
        saw(request, &mai, &["b1/f1".to_owned()], known());
        assert!(seen_saying(request, &mai));
        assert!(!seen_saying(request + 1, &mai));
        assert!(!seen_saying(request, "someone-else"));
        assert_eq!(
            refusal(fact, Some(fact), false, seen_saying(request, &mai)),
            None
        );
        // Read again as not seen, it is forgotten.
        saw(request, &mai, &[], known());
        assert!(!seen_saying(request, &mai));
        // Not seen, still ask-first as before: an asked keep, or a yes to
        // the lamp's offer, runs; a taken-back line or none keeps nothing.
        assert_eq!(
            refusal(
                fact,
                Some("Please remember I'm starting at the bank."),
                false,
                false
            ),
            None
        );
        assert_eq!(refusal(fact, Some("Yes, please."), true, false), None);
        assert!(unseen_refused(Some("Yes, but don't save that."), true));
        assert!(unseen_refused(None, true));
        // Not seen: no note and no limit.
        assert_eq!(own_seen(None, request), None);
        assert_eq!(note_for(None, request), None);
    }

    /// Thirteen's P2: a quiet keep is kept only when every key word of its
    /// sentence was heard; an asked keep, or a yes to the lamp's offer, is
    /// as before; a line that took the keep back keeps nothing.
    #[test]
    fn a_quiet_keep_is_only_their_heard_words() {
        let line = "my favourite tea is jasmine";
        assert!(refused("My favorite food is jasmine.", Some(line), false));
        assert!(!refused("My favourite tea is jasmine.", Some(line), false));
        // Asked: as before (said back, LAMP_MEMORY_KEEP_HEARD's own).
        assert!(!refused(
            "My favorite food is jasmine.",
            Some("please remember that my favourite tea is jasmine"),
            false
        ));
        // A yes to the lamp's offer: as before.
        assert!(!refused(
            "Lily arrives on Saturday.",
            Some("Yes, please."),
            true
        ));
        // Taken back, or nothing heard: nothing kept.
        assert!(refused(
            "My favourite tea is jasmine.",
            Some("my favourite tea is jasmine, but don't save that"),
            false
        ));
        assert!(refused("My favourite tea is jasmine.", None, false));
    }

    /// Friday's 18:5x flag on #5053: a small-talk line keeps nothing
    /// quietly, even in its own words; the person's own "remember" on the
    /// same words still keeps, and a plain fact still keeps quietly.
    #[test]
    fn small_talk_keeps_nothing_quietly() {
        for (model, line) in [
            (
                "A walk after lunch is a good idea.",
                "Do you think a walk after lunch is a good idea?",
            ),
            ("Okay, thanks.", "Okay, thanks."),
            (
                "Cook for dinner tonight.",
                "What should I cook for dinner tonight?",
            ),
        ] {
            assert!(small_talk(Some(line)), "{line}");
            assert!(refused(model, Some(line), false), "{line}");
        }
        assert!(!refused(
            "A walk after lunch.",
            Some("Remember a walk after lunch."),
            false
        ));
        assert!(!refused(
            "My sister Lily is coming on Saturday.",
            Some("My sister Lily is coming on Saturday."),
            false
        ));
    }
}
