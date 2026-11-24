//! A lamp set up with nobody known gets a way back (#7876,
//! `LAMP_FACE_LEARN_LATER`, `on` only): on lamp-e82d the owner's face was
//! never kept at setup, so every later conversation kept nothing. With the
//! eyes knowing nobody and one face in view, the walk arms the ask as a
//! person's line begins a conversation ([`Moment::opens`], told the second
//! route's worker with the line's Start, `WalkUnderWay`'s `name_ask`). If
//! that line is a greeting and nothing more, its own reply asks the name in
//! place of the reply's own question, one reply, one question
//! ([`GREETING_NOTE`], [`turn`]; rule 8, #8471: never "How are you? What
//! should I call you?"). It is getting to know someone, never a consent
//! question.
//!
//! - **The answer**: the next line from anyone, heard within
//!   [`ANSWER_WITHIN_US`], goes to the model with the ask in view
//!   ([`ANSWER_NOTE`]). The model judges what they meant, never a list of
//!   words (the owner's rule, 13 Nov): a name, and it calls
//!   `remember_my_face`, which counts as answered ([`remembered`]); a
//!   decline, and it calls [`DECLINED`], which stops the ask for good
//!   ([`declined`]). Both reach the walk by its mailbox. The speaker-bound
//!   route (`known_faces::run_speaker`, `LAMP_REMEMBER_SPEAKER`) proves the
//!   face in view said its own name, unchanged; kept (the eyes'
//!   `remembered` only), the model says "I'll know you next time. Say
//!   'forget my face' to undo." (`known_faces::KEPT_NO_ASK`).
//! - **Nothing kept**: past the window, after a line between, or with the
//!   answer's turn gone by with no `remember_my_face`, every later line of
//!   that conversation tells the model their face was not kept
//!   ([`NOT_KEPT_NOTE`]), so it never says it will know them (#8471: "I'll
//!   know you now." with nothing called, 30.7 s on).
//! - **Bounds**, kept in the walk's file ([`Kept`]), counted when the
//!   worker tells the walk it asked (`{"learn_later":"asked"}`): at most
//!   once in [`DAY_S`]; an ask left unanswered counts, and after
//!   [`MOST_UNANSWERED`] it is never asked again; the model's [`DECLINED`],
//!   or "forget my face" (the lamp's own fixed undo phrase) at any time,
//!   stops it for good.
//! - **Never**: with a second face in view as the line begins, on a first
//!   line that is more than a greeting (a task, a question), or past the
//!   conversation's first line.
//!
//! Effective only with `LAMP_FACE_NO_ASK` and `LAMP_REMEMBER_SPEAKER` on
//! too ([`switched`]); the three go on together. Off, nothing here runs.
use lamp_gemini::{Tool, ToolCall};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

/// The switch: `on` only.
pub const ENVIRONMENT: &str = "LAMP_FACE_LEARN_LATER";

/// At most one ask in this long.
pub const DAY_S: u64 = 86_400;

/// Asks left unanswered before it is never asked again.
pub const MOST_UNANSWERED: u8 = 3;

/// A line heard after this much quiet begins a new conversation.
pub const CONVERSATION_GAP_US: u64 = 60_000_000;

/// A line this soon after the asking turn, with no other line between, is
/// its answer (60 s since #8471's lamp run: a name 30.7 s on was missed).
pub const ANSWER_WITHIN_US: u64 = 60_000_000;

/// How often the eyes are asked how many people they know.
pub const KNOWN_EVERY: Duration = Duration::from_secs(30);

/// Whether the feature acts: its own switch `on`, with `LAMP_FACE_NO_ASK`
/// (`face_no_ask`, the walk's own reading) and `LAMP_REMEMBER_SPEAKER` on.
pub fn switched(
    learn_later: Option<&str>,
    face_no_ask: bool,
    remember_speaker: Option<&str>,
) -> bool {
    learn_later == Some("on")
        && face_no_ask
        && crate::remember_speaker::switched_on(remember_speaker)
}

/// The function the model calls when the person, just asked, makes clear
/// they would rather not say or not be asked.
pub const DECLINED: &str = "not_telling_their_name";

/// The second route's note on the greeting the walk has armed ([`turn`]):
/// the name question takes the place of the reply's own question, in the
/// one reply (rule 8, #8471: never "How are you? What should I call you?").
pub(crate) const GREETING_NOTE: &str = "They just greeted you, and you don't know them yet. Answer their greeting in a few words and, in place of any question of your own, ask them only what you should call them, as the reply's one question, e.g. \"Hello! I don't think we've met. What should I call you?\" Ask nothing else, not how they are.";

/// The second route's note on the asking turn's answer ([`Turn::Answer`]).
pub(crate) const ANSWER_NOTE: &str = "You just asked them \"What should I call you?\" Judge what they meant. If they told you their name, call remember_my_face with it; their face is kept only if its result says so, so never say you'll know, recognize or remember them unless it does. If they made clear they would rather not say, or not be asked, call not_telling_their_name and say only that that's fine, in a few words. Otherwise answer what they said as usual.";

/// The second route's note on any later line of that conversation while no
/// `remember_my_face` was called for it ([`Turn::NotKept`]): past the
/// answer's window, after a line between, or with the answer gone by.
pub(crate) const NOT_KEPT_NOTE: &str = "Earlier in this conversation you asked what to call them, and their face was not kept: never say you'll know, recognize or remember them. If they tell you their name now, you may call remember_my_face with it, and say you'll know them only if its result says their face is kept.";

/// What a request is to the ask, in the second route ([`turn`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Turn {
    /// The armed greeting: its reply asks the name ([`GREETING_NOTE`]).
    Ask,
    /// The next line, within [`ANSWER_WITHIN_US`], nothing between
    /// ([`ANSWER_NOTE`]).
    Answer,
    /// A later line of that conversation, nothing kept ([`NOT_KEPT_NOTE`]).
    NotKept,
    /// Nothing of the ask.
    Other,
}

impl Turn {
    /// The note the model is told on this turn.
    pub(crate) fn note(self) -> Option<&'static str> {
        match self {
            Self::Ask => Some(GREETING_NOTE),
            Self::Answer => Some(ANSWER_NOTE),
            Self::NotKept => Some(NOT_KEPT_NOTE),
            Self::Other => None,
        }
    }
}

/// The ask as the second route's process holds it.
#[derive(Debug, Default)]
struct Asking {
    /// The walk's word with the latest turn: the next greeting may ask.
    armed: bool,
    /// Asked on this arm already: never twice before the walk says again.
    used: bool,
    /// The asking turn judged, its reply not yet heard ([`sounded`]): a
    /// reply gone before its first sound asked nothing (El's P2 on #8471).
    pending: Option<u64>,
    /// The asking turn heard, its answer not yet come.
    asked: Option<Instant>,
    /// The request that is the ask's answer, while it is the latest.
    answer: Option<u64>,
    /// Asked and nothing kept: when the conversation's last line was.
    unkept: Option<Instant>,
    /// The latest request judged, and what it was: one answer per request.
    last: Option<(u64, Turn)>,
}

worker_global! {
    /// The second route's [`Asking`]. The process's own; a test thread's
    /// own in tests.
    fn asking() -> Mutex<Asking> = Mutex::new(Asking {
        armed: false,
        used: false,
        pending: None,
        asked: None,
        answer: None,
        unkept: None,
        last: None,
    });
}

/// The walk's word for the turn in hand (`WalkUnderWay`'s `name_ask`), in
/// the second route's process: a greeting now may ask the name.
pub(crate) fn set_armed(on: bool) {
    if let Ok(mut asking) = asking().lock() {
        if !on {
            asking.used = false;
        }
        asking.armed = on;
    }
}

/// What request `request` is to the ask (heard: the person's line, its
/// words `said`), judged once per request at `now`. The greeting the walk
/// armed asks, once; the next line from anyone is its answer only if heard
/// within [`ANSWER_WITHIN_US`]; any other later line of that conversation
/// ([`CONVERSATION_GAP_US`] of quiet ends it) is told
/// nothing was kept, until `remember_my_face` is called.
pub(crate) fn turn_at(request: u64, heard: bool, said: &str, now: Instant) -> Turn {
    let Ok(mut asking) = asking().lock() else {
        return Turn::Other;
    };
    if let Some((last, turn)) = asking.last
        && last == request
    {
        return turn;
    }
    asking.answer = None;
    // An asking turn whose reply never sounded (revoked, cut, failed):
    // nothing was asked, and this arm may ask again.
    if asking.pending.take().is_some() {
        asking.used = false;
    }
    let within =
        |at: Instant, us: u64| now.saturating_duration_since(at) <= Duration::from_micros(us);
    let turn = if let Some(at) = asking.asked.take() {
        asking.unkept = Some(now);
        if heard && within(at, ANSWER_WITHIN_US) {
            asking.answer = Some(request);
            Turn::Answer
        } else if heard {
            Turn::NotKept
        } else {
            Turn::Other
        }
    } else if heard && asking.armed && !asking.used && greeting_only(&words_of(said)) {
        asking.used = true;
        asking.pending = Some(request);
        asking.unkept = None;
        Turn::Ask
    } else if heard
        && asking
            .unkept
            .is_some_and(|at| within(at, CONVERSATION_GAP_US))
    {
        asking.unkept = Some(now);
        Turn::NotKept
    } else {
        if heard {
            asking.unkept = None;
        }
        Turn::Other
    };
    asking.last = Some((request, turn));
    turn
}

/// [`turn_at`] now.
pub(crate) fn turn(request: u64, heard: bool, said: &str) -> Turn {
    let turn = turn_at(request, heard, said, Instant::now());
    if turn == Turn::Ask {
        eprintln!(
            "lamp-voice second: request {request}: the eyes know nobody and they greeted: the reply asks their name in place of its own question (LAMP_FACE_LEARN_LATER)"
        );
    }
    turn
}

/// Request `request`'s answer's first sound is out, as the air's note is
/// spent then (`Shared::air`): if it is the asking turn, the ask is heard.
/// Only now does its answer's window run and is the walk told it asked
/// (`{"learn_later":"asked"}` in its mailbox, `onboarding`), which counts
/// it in its bounds; a reply gone before it posts nothing.
pub(crate) fn sounded_at(request: u64, onboarding: Option<&Path>, now: Instant) {
    let heard = asking().lock().is_ok_and(|mut asking| {
        let heard = asking
            .pending
            .take_if(|pending| *pending == request)
            .is_some();
        if heard {
            asking.asked = Some(now);
        }
        heard
    });
    if heard {
        eprintln!(
            "lamp-voice second: request {request}: its reply asking their name is heard (LAMP_FACE_LEARN_LATER)"
        );
        if let Some(file) = onboarding
            && let Err(error) = super::post(file, &json!({"learn_later": "asked"}))
        {
            eprintln!("lamp-voice: onboarding: the name ask not noted: {error}");
        }
    }
}

/// [`sounded_at`] now, with the walk's file from the environment.
pub(crate) fn sounded(request: u64) {
    sounded_at(
        request,
        crate::onboarding::file_from_environment().as_deref(),
        Instant::now(),
    );
}

/// Whether the turn in hand is the ask's answer ([`Turn::Answer`]).
#[cfg(test)]
fn answering() -> bool {
    asking().lock().is_ok_and(|asking| asking.answer.is_some())
}

/// Takes the ask's answer: whether this turn was it.
fn take_answering() -> bool {
    asking()
        .lock()
        .is_ok_and(|mut asking| asking.answer.take().is_some())
}

/// `remember_my_face` called: its own result tells the model whether the
/// face is kept, so no later line is told nothing was. Whether the ask was
/// open in this conversation.
fn take_asked() -> bool {
    asking().lock().is_ok_and(|mut asking| {
        let answer = asking.answer.take().is_some();
        let unkept = asking.unkept.take().is_some();
        answer || unkept
    })
}

/// Whether the second route declares [`DECLINED`]: the switch on, with
/// names checked, `LAMP_FACE_NO_ASK` and `LAMP_REMEMBER_SPEAKER`.
pub fn worker_on(names_checked: bool, remember_speaker: bool) -> bool {
    std::env::var(ENVIRONMENT).as_deref() == Ok("on")
        && names_checked
        && remember_speaker
        && crate::known_faces::face_no_ask_on()
}

/// [`DECLINED`], for the model.
pub fn tool() -> Tool {
    Tool {
        name: DECLINED.into(),
        description: "Call this only right after you asked \"What should I call you?\", when the person \
                      made clear they would rather not tell you their name, or not be asked. Never when \
                      they told you a name. Then say only that that's fine, in a few words."
            .into(),
        parameters: None,
        silent: false,
    }
}

/// [`DECLINED`] called: on the ask's answer, the walk told to stop it for
/// good (`{"learn_later":"declined"}` in its mailbox); any other time,
/// nothing.
pub fn declined(_call: &ToolCall, onboarding: Option<&Path>) -> (Value, bool) {
    if !take_answering() {
        return (
            json!({"error": "You did not just ask their name. Answer them as usual."}),
            true,
        );
    }
    if let Some(file) = onboarding
        && let Err(error) = super::post(file, &json!({"learn_later": "declined"}))
    {
        eprintln!("lamp-voice: onboarding: a decline of the name ask not noted: {error}");
    }
    eprintln!(
        "lamp-voice: the person declined \"What should I call you?\": never asked again (LAMP_FACE_LEARN_LATER)"
    );
    (
        json!({"ok": true, "note": "Say only that that's fine, in a few words. Ask nothing."}),
        true,
    )
}

/// `remember_my_face` called on the ask's answer, or later in that
/// conversation: the walk told it was answered
/// (`{"learn_later":"answered"}`), whatever the eyes then do.
pub fn remembered(onboarding: Option<&Path>) {
    if take_asked()
        && let Some(file) = onboarding
        && let Err(error) = super::post(file, &json!({"learn_later": "answered"}))
    {
        eprintln!("lamp-voice: onboarding: the name ask's answer not noted: {error}");
    }
}

/// What is kept in the walk's file. An older build reads past it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    /// When it was last asked, in Unix seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asked_at_unix_s: Option<u64>,
    /// Asks with no answer: counted as asked, given back on an answer.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub unanswered: u8,
    /// A no to it, or "forget my face": never asked again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stopped: bool,
}

fn is_zero(count: &u8) -> bool {
    *count == 0
}

impl Kept {
    /// Whether it is nothing: left out of the file.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Whether the bounds let it be asked at `now_s`.
    pub fn may_ask(&self, now_s: u64) -> bool {
        !self.stopped
            && self.unanswered < MOST_UNANSWERED
            && self
                .asked_at_unix_s
                .is_none_or(|at| now_s.saturating_sub(at) >= DAY_S)
    }

    /// Asked at `now_s`: unanswered until an answer gives it back.
    pub fn asked(&mut self, now_s: u64) {
        self.asked_at_unix_s = Some(now_s);
        self.unanswered = self.unanswered.saturating_add(1);
    }
}

/// The conversation as the walk hears it, for the moment to arm the ask in.
#[derive(Clone, Debug, Default)]
pub struct Moment {
    /// The person's or the lamp's last words.
    last_us: Option<u64>,
}

impl Moment {
    /// A line the person finished, at `now_us`: whether it says "forget my
    /// face", which stops the ask for good.
    pub fn heard(&mut self, text: &str, now_us: u64) -> bool {
        self.last_us = Some(now_us);
        said_forget_my_face(&words_of(text))
    }

    /// The lamp's words, at `now_us`.
    pub fn lamp_said(&mut self, now_us: u64) {
        self.last_us = Some(now_us);
    }

    /// Whether a line begun at `now_us` opens a conversation: nothing said
    /// for [`CONVERSATION_GAP_US`].
    pub fn opens(&self, now_us: u64) -> bool {
        self.last_us
            .is_none_or(|last| now_us.saturating_sub(last) >= CONVERSATION_GAP_US)
    }
}

/// The words of a line, lower case, letters, digits and apostrophes.
fn words_of(text: &str) -> Vec<String> {
    text.to_lowercase()
        .replace('’', "'")
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Whether the words are a greeting and nothing more: "Hi", "Hey lamp,
/// good morning!", "Hello, how are you?". A question or a task is not.
fn greeting_only(words: &[String]) -> bool {
    const GREETING: &[&str] = &[
        "hi",
        "hey",
        "hello",
        "hiya",
        "howdy",
        "yo",
        "morning",
        "afternoon",
        "evening",
        "good",
        "there",
        "lamp",
        "how",
        "are",
        "you",
        "doing",
        "what's",
        "up",
        "again",
        "buddy",
        "friend",
    ];
    const OPENS: &[&str] = &[
        "hi", "hey", "hello", "hiya", "howdy", "yo", "good", "morning",
    ];
    words
        .first()
        .is_some_and(|first| OPENS.contains(&first.as_str()))
        && words.iter().all(|word| GREETING.contains(&word.as_str()))
}

/// Whether the words ask to forget their face.
fn said_forget_my_face(words: &[String]) -> bool {
    words
        .windows(3)
        .any(|three| three[0] == "forget" && three[1] == "my" && three[2] == "face")
}

/// How many people the eyes know, kept fresh by a thread of its own:
/// `None` until the eyes answer.
pub(crate) type Known = Arc<Mutex<Option<usize>>>;

/// Starts the read of how many people the eyes know (`{"list":true}` on
/// their socket, `known_faces::names_reached`), every [`KNOWN_EVERY`], off
/// every reply's path, until the walk lets it go.
pub(crate) fn start(socket: PathBuf) -> Option<Known> {
    let known: Known = Arc::new(Mutex::new(None));
    let weak: Weak<Mutex<Option<usize>>> = Arc::downgrade(&known);
    let spawned = std::thread::Builder::new()
        .name("learn-later-known".into())
        .spawn(move || {
            loop {
                let count = crate::known_faces::names_reached(&socket)
                    .flatten()
                    .map(|names| names.len());
                let Some(known) = weak.upgrade() else {
                    return;
                };
                if let Ok(mut kept) = known.lock() {
                    *kept = count;
                }
                drop(known);
                std::thread::sleep(KNOWN_EVERY);
            }
        });
    match spawned {
        Ok(_) => Some(known),
        Err(error) => {
            eprintln!(
                "lamp-voice: onboarding: how many people the eyes know not read: {error} (LAMP_FACE_LEARN_LATER)"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;

    #[test]
    fn the_three_switches_go_on_together() {
        assert!(switched(Some("on"), true, Some("on")));
        assert!(!switched(None, true, Some("on")));
        assert!(!switched(Some("yes"), true, Some("on")));
        assert!(!switched(Some("on"), false, Some("on")));
        assert!(!switched(Some("on"), true, None));
        assert!(!switched(Some("on"), true, Some("off")));
    }

    #[test]
    fn a_greeting_is_only_a_greeting() {
        for line in [
            "Hi.",
            "Hey lamp, good morning!",
            "Hello, how are you?",
            "Hi there",
        ] {
            assert!(greeting_only(&words_of(line)), "{line}");
        }
        for line in [
            "Hi, set a timer for ten minutes.",
            "What's the weather?",
            "Hey, what time is it?",
            "",
        ] {
            assert!(!greeting_only(&words_of(line)), "{line}");
        }
    }

    /// The asking turn `request`, armed by the walk, at `at`, its reply
    /// heard.
    fn asked_on(request: u64, at: Instant) {
        set_armed(true);
        assert_eq!(turn_at(request, true, "Hello, how are you?", at), Turn::Ask);
        sounded_at(request, None, at);
    }

    /// El's P2 on #8471: an asking turn whose reply is gone before its
    /// first sound (`Stop::Gone`: revoked, a barge-in) posts nothing to
    /// the walk, and the next line gets no answer note for a question never
    /// heard. Another request's sound hears nothing of it either.
    #[test]
    fn an_asking_reply_gone_before_its_sound_asked_nothing() {
        let home = crate::process::SessionDirectory::create().unwrap();
        let file = home.path.join("onboarding.json");
        let mailbox =
            || std::fs::read_to_string(crate::onboarding::mailbox_of(&file)).unwrap_or_default();
        let at = Instant::now();
        set_armed(true);
        assert_eq!(turn_at(60, true, "Hi lamp.", at), Turn::Ask);
        sounded_at(59, Some(&file), at);
        assert_eq!(mailbox(), "");
        assert_eq!(turn_at(61, true, "Juniper.", at + S_D), Turn::Other);
        assert!(!answering());
        sounded_at(60, Some(&file), at + S_D);
        assert_eq!(mailbox(), "", "a late sound of the gone reply asks nothing");
        // Heard: posted once, and the next line is its answer.
        assert_eq!(turn_at(62, true, "Hello.", at + 2 * S_D), Turn::Ask);
        sounded_at(62, Some(&file), at + 2 * S_D);
        sounded_at(62, Some(&file), at + 2 * S_D);
        assert_eq!(mailbox(), "{\"learn_later\":\"asked\"}\n");
        assert_eq!(turn_at(63, true, "Juniper.", at + 3 * S_D), Turn::Answer);
        remembered(None);
    }

    /// Rule 8 (#8471's lamp run: "...How are you?" then the name question):
    /// the armed greeting's own reply asks the name, in place of its own
    /// question, one reply, one question: the note goes on the greeting's
    /// turn, and no line of the walk's follows it. Once per arm; a task, a
    /// line the walk did not arm, or a lamp's own line asks nothing.
    #[test]
    fn the_ask_is_folded_into_the_greetings_reply() {
        let at = Instant::now();
        set_armed(false);
        assert_eq!(turn_at(1, true, "Hello, how are you?", at), Turn::Other);
        set_armed(true);
        assert_eq!(turn_at(2, false, "Hello, how are you?", at), Turn::Other);
        assert_eq!(turn_at(3, true, "Hi, set a timer.", at), Turn::Other);
        assert_eq!(turn_at(4, true, "Hello, how are you?", at), Turn::Ask);
        // The same request judged again: the same turn, one note.
        assert_eq!(turn_at(4, true, "Hello, how are you?", at), Turn::Ask);
        assert_eq!(Turn::Ask.note(), Some(GREETING_NOTE));
        assert!(GREETING_NOTE.contains("What should I call you?"));
        assert!(GREETING_NOTE.contains("in place of any question of your own"));
        // Answered; a greeting later on the same arm asks nothing more.
        sounded_at(4, None, at);
        assert_eq!(turn_at(5, true, "Sam.", at), Turn::Answer);
        remembered(None);
        assert_eq!(
            turn_at(6, true, "Hi.", at + Duration::from_secs(120)),
            Turn::Other
        );
        // The walk's word again (a new arm): asked again.
        set_armed(false);
        set_armed(true);
        assert_eq!(turn_at(7, true, "Hi.", at), Turn::Ask);
        remembered(None);
    }

    /// #8471's lamp run: the name came 30.7 s after the ask, with no line
    /// between. It is the ask's answer: the model is told so, with
    /// `remember_my_face` the only way to keep the face.
    #[test]
    fn a_name_at_30_7_s_with_no_line_between_is_the_answer() {
        let at = Instant::now();
        asked_on(10, at);
        assert_eq!(
            turn_at(11, true, "Juniper.", at + Duration::from_millis(30_700)),
            Turn::Answer
        );
        assert!(answering(), "30.7 s after the ask is its answer");
        assert!(ANSWER_NOTE.contains("remember_my_face"));
        assert!(ANSWER_NOTE.contains("never say you'll know"));
        remembered(None);
        assert!(!answering());
    }

    /// Past the window (61 s), after a line between, or with the answer
    /// turn gone by with nothing called: the model is told nothing was kept,
    /// on every later line of that conversation, until `remember_my_face`.
    #[test]
    fn past_the_window_or_a_line_between_the_model_is_told_nothing_was_kept() {
        let at = Instant::now();
        asked_on(20, at);
        assert_eq!(
            turn_at(21, true, "Juniper.", at + Duration::from_secs(61)),
            Turn::NotKept
        );
        assert!(!answering());
        assert_eq!(Turn::NotKept.note(), Some(NOT_KEPT_NOTE));
        assert!(NOT_KEPT_NOTE.contains("their face was not kept"));
        assert_eq!(
            turn_at(22, true, "Nice to meet you.", at + Duration::from_secs(70)),
            Turn::NotKept
        );
        // A conversation's gap of quiet ends it.
        assert_eq!(
            turn_at(23, true, "Hi.", at + Duration::from_secs(200)),
            Turn::Other
        );
        // A line between (a lamp's own, a reminder): not the answer.
        let at = Instant::now();
        set_armed(false);
        asked_on(30, at);
        assert_eq!(turn_at(31, false, "A reminder.", at + S_D), Turn::Other);
        assert_eq!(turn_at(32, true, "Juniper.", at + 2 * S_D), Turn::NotKept);
        // The answer turn with nothing called: the next line is told.
        let at = Instant::now();
        set_armed(false);
        asked_on(40, at);
        assert_eq!(turn_at(41, true, "Juniper.", at + S_D), Turn::Answer);
        assert_eq!(turn_at(42, true, "So?", at + 2 * S_D), Turn::NotKept);
        assert!(!answering());
        // `remember_my_face` called late: its result tells; no note after.
        remembered(None);
        assert_eq!(turn_at(43, true, "Thanks.", at + 3 * S_D), Turn::Other);
    }

    const S_D: Duration = Duration::from_secs(1);

    /// The model judges the answer: its [`DECLINED`] stops the ask only on
    /// the ask's answer, once; `remember_my_face` on it counts as answered.
    /// No word in the line decides it: "I don't mind, call me Sam" is a name.
    #[test]
    fn the_model_judges_a_decline_and_only_on_the_asks_answer() {
        let home = crate::process::SessionDirectory::create().unwrap();
        let file = home.path.join("onboarding.json");
        let call = ToolCall {
            id: "1".into(),
            name: DECLINED.into(),
            args: json!({}),
        };
        let mailbox =
            || std::fs::read_to_string(crate::onboarding::mailbox_of(&file)).unwrap_or_default();
        // Not asked: nothing posted.
        assert!(declined(&call, Some(&file)).0.get("error").is_some());
        assert_eq!(mailbox(), "");
        // Asked, the walk told so; the model declines: posted once.
        set_armed(true);
        assert_eq!(turn(50, true, "Hi lamp."), Turn::Ask);
        assert_eq!(mailbox(), "", "not before its reply is heard");
        sounded_at(50, Some(&file), Instant::now());
        assert_eq!(mailbox(), "{\"learn_later\":\"asked\"}\n");
        assert_eq!(turn(51, true, "No, thanks."), Turn::Answer);
        assert!(answering());
        assert_eq!(declined(&call, Some(&file)).0["ok"], true);
        assert!(mailbox().ends_with("{\"learn_later\":\"declined\"}\n"));
        assert!(!answering());
        assert!(declined(&call, Some(&file)).0.get("error").is_some());
        // Asked, a name: answered, never declined.
        set_armed(false);
        set_armed(true);
        assert_eq!(turn(52, true, "Hello."), Turn::Ask);
        sounded_at(52, Some(&file), Instant::now());
        assert_eq!(turn(53, true, "I don't mind, call me Sam"), Turn::Answer);
        remembered(Some(&file));
        assert!(mailbox().ends_with("{\"learn_later\":\"answered\"}\n"));
        assert!(!answering());
        assert!(ANSWER_NOTE.contains(DECLINED));
        assert!(said_forget_my_face(&words_of("Lamp, forget my face.")));
        assert!(!said_forget_my_face(&words_of("I don't mind, call me Sam")));
    }

    #[test]
    fn once_a_day_and_three_unanswered_then_never() {
        let mut kept = Kept::default();
        assert!(kept.may_ask(1_000));
        kept.asked(1_000);
        assert!(!kept.may_ask(1_000 + DAY_S - 1));
        assert!(kept.may_ask(1_000 + DAY_S));
        kept.asked(1_000 + DAY_S);
        kept.asked(1_000 + 2 * DAY_S);
        assert_eq!(kept.unanswered, MOST_UNANSWERED);
        assert!(!kept.may_ask(1_000 + 30 * DAY_S));
        let stopped = Kept {
            stopped: true,
            ..Kept::default()
        };
        assert!(!stopped.may_ask(u64::MAX));
    }

    #[test]
    fn a_line_opens_a_conversation_after_a_minute_of_quiet() {
        let mut moment = Moment::default();
        assert!(moment.opens(0));
        assert!(!moment.heard("Hi lamp.", S));
        assert!(!moment.opens(2 * S));
        moment.lamp_said(3 * S);
        assert!(!moment.opens(3 * S + CONVERSATION_GAP_US - 1));
        assert!(moment.opens(3 * S + CONVERSATION_GAP_US));
        assert!(moment.heard("Forget my face.", 4 * S));
    }
}
