//! A lamp set up with nobody known gets a way back (#7876,
//! `LAMP_FACE_LEARN_LATER`, `on` only): on lamp-e82d the owner's face was
//! never kept at setup, so every later conversation kept nothing. With the
//! eyes knowing nobody, one face in view seen saying a greeting, and the
//! lamp's answer to it over, the walk asks once, in its own words, "What
//! should I call you?" It is getting to know someone, never a consent
//! question.
//!
//! - **The answer** goes to the model with the ask in view
//!   ([`ANSWER_NOTE`], on the one turn after it, [`answering`]). The model
//!   judges what they meant, never a list of words (the owner's rule, 13
//!   Nov): a name, and it calls `remember_my_face`, which counts as answered
//!   ([`remembered`]); a decline, and it calls [`DECLINED`], which stops the
//!   ask for good ([`declined`]). Both reach the walk by its mailbox. On a
//!   name, the model calls
//!   `remember_my_face`, and the speaker-bound route
//!   (`known_faces::run_speaker`, `LAMP_REMEMBER_SPEAKER`) proves the face in
//!   view said its own name, unchanged. Kept (the eyes' `remembered` only),
//!   the model says "I'll know you next time. Say 'forget my face' to undo."
//!   (`known_faces::KEPT_NO_ASK`); the eyes refusing, nothing is said of it.
//!   The setup name answered again is the same person
//!   (`known_faces::memory_person`); another name is its own.
//! - **The ask** is the walk's own line ([`ASK`], said as written, no chat):
//!   nothing is read from memory for it, and the second route keeps its
//!   exchange out of memory ([`is_ask`]).
//! - **Bounds**, kept in the walk's file ([`Kept`]): at most once in
//!   [`DAY_S`]; an ask left unanswered counts, and after [`MOST_UNANSWERED`]
//!   it is never asked again; the model's [`DECLINED`], or "forget my face"
//!   (the lamp's own fixed undo phrase) at any time, stops it for good.
//! - **Never**: with a second face in view since the conversation began,
//!   on a first line that is more than a greeting (a task, a question), or
//!   past the conversation's first exchange.
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

/// The one line asked, as the walk's own.
pub(crate) const ASK: &str =
    "(Say only this, nothing before or after it: \"What should I call you?\")";

/// The ask's words.
pub(crate) const ASK_WORDS: &str = "What should I call you?";

/// At most one ask in this long.
pub const DAY_S: u64 = 86_400;

/// Asks left unanswered before it is never asked again.
pub const MOST_UNANSWERED: u8 = 3;

/// A line heard after this much quiet begins a new conversation.
pub const CONVERSATION_GAP_US: u64 = 60_000_000;

/// The lamp's answer to the greeting ended at least this long ago before it
/// asks: its own words are over, and the person has not gone on.
pub const PAUSE_US: u64 = 1_500_000;

/// Past this after the lamp's answer the moment has gone.
pub const MOMENT_US: u64 = 10_000_000;

/// A face read speaking this close to a line's end saw it said.
pub const LINE_SEEN_US: u64 = 6_000_000;

/// A line this soon after the ask is its answer.
pub const ANSWER_WITHIN_US: u64 = 30_000_000;

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

/// The second route's note on the turn after the ask ([`answering`]).
pub(crate) const ANSWER_NOTE: &str = "You just asked them \"What should I call you?\" Judge what they meant. If they told you their name, call remember_my_face with it. If they made clear they would rather not say, or not be asked, call not_telling_their_name and say only that that's fine, in a few words. Otherwise answer what they said as usual.";

worker_global! {
    /// What the second route was told of the ask: when its turn began,
    /// taken by the first answer to it. The process's own; a test thread's
    /// own in tests.
    fn asked_at() -> Mutex<Option<Instant>> = Mutex::new(None);
}

/// The second route took the ask's turn ([`is_ask`]): the next line is its
/// answer for [`ANSWER_WITHIN_US`].
pub(crate) fn asked_now() {
    if let Ok(mut asked) = asked_at().lock() {
        *asked = Some(Instant::now());
    }
}

/// Whether a line now answers the ask: within [`ANSWER_WITHIN_US`] of it,
/// and not yet answered.
pub(crate) fn answering() -> bool {
    asked_at().lock().is_ok_and(|asked| {
        asked.is_some_and(|at| at.elapsed() <= Duration::from_micros(ANSWER_WITHIN_US))
    })
}

/// Takes the ask: whether it was being answered.
fn take_answering() -> bool {
    let answering = answering();
    if let Ok(mut asked) = asked_at().lock() {
        *asked = None;
    }
    answering
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

/// `remember_my_face` called on the ask's answer: the walk told it was
/// answered (`{"learn_later":"answered"}`), whatever the eyes then do.
pub fn remembered(onboarding: Option<&Path>) {
    if take_answering()
        && let Some(file) = onboarding
        && let Err(error) = super::post(file, &json!({"learn_later": "answered"}))
    {
        eprintln!("lamp-voice: onboarding: the name ask's answer not noted: {error}");
    }
}

/// Whether `text` is the ask as the walk hands it to the second route: its
/// own line, so the turn reads no memory and keeps none.
pub fn is_ask(text: &str) -> bool {
    crate::onboarding::own_words(text) == Some(ASK_WORDS)
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

/// The conversation as the walk hears it, for the moment to ask in.
#[derive(Clone, Debug, Default)]
pub struct Moment {
    /// The person's or the lamp's last words.
    last_us: Option<u64>,
    /// The person's lines this conversation.
    lines: u32,
    /// The first line was a greeting and nothing more.
    greeting: bool,
    /// A face was read speaking as the first line was said.
    seen_speaking: bool,
    /// A second face was in view since the conversation began.
    two_faces: bool,
    /// When the lamp's words after the first line last came.
    answered_us: Option<u64>,
    /// When it was asked in this conversation.
    asked_us: Option<u64>,
    /// The latest look with a face read speaking.
    speaking_us: Option<u64>,
}

impl Moment {
    /// A look: how many faces, and whether one read speaking.
    pub fn saw(&mut self, now_us: u64, faces: Option<usize>, speaking: bool) {
        if speaking {
            self.speaking_us = Some(now_us);
        }
        if faces.is_some_and(|faces| faces >= 2) && self.lines > 0 {
            self.two_faces = true;
        }
    }

    /// A line the person finished, at `now_us`: whether it says "forget my
    /// face", which stops the ask for good.
    pub fn heard(&mut self, text: &str, now_us: u64) -> bool {
        let words = words_of(text);
        let forget = said_forget_my_face(&words);
        if self
            .last_us
            .is_none_or(|last| now_us.saturating_sub(last) >= CONVERSATION_GAP_US)
        {
            let speaking_us = self.speaking_us;
            *self = Self {
                speaking_us,
                ..Self::default()
            };
        }
        self.lines = self.lines.saturating_add(1);
        self.last_us = Some(now_us);
        if self.lines == 1 {
            self.greeting = greeting_only(&words);
            self.seen_speaking = self
                .speaking_us
                .is_some_and(|at| now_us.saturating_sub(at) <= LINE_SEEN_US);
        }
        forget
    }

    /// The lamp's words, at `now_us`.
    pub fn lamp_said(&mut self, now_us: u64) {
        if self.lines > 0 {
            self.answered_us = Some(now_us);
        }
        self.last_us = Some(now_us);
    }

    /// Whether now is the moment: the conversation's first line only a
    /// greeting, a face seen saying it, no second face since, the lamp's
    /// answer over a moment ago, not asked yet.
    pub fn is_now(&self, now_us: u64) -> bool {
        self.lines == 1
            && self.greeting
            && self.seen_speaking
            && !self.two_faces
            && self.asked_us.is_none()
            && self.answered_us.is_some_and(|at| {
                let since = now_us.saturating_sub(at);
                (PAUSE_US..=MOMENT_US).contains(&since)
            })
    }

    /// Asked now.
    pub fn asked(&mut self, now_us: u64) {
        self.asked_us = Some(now_us);
        self.last_us = Some(now_us);
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
    fn the_ask_is_the_walks_own_line() {
        assert_eq!(crate::onboarding::fixed_words(ASK), Some(ASK_WORDS));
        assert!(is_ask(&crate::onboarding::own_line(ASK.to_owned())));
        assert!(!is_ask(ASK));
        assert!(!is_ask(crate::onboarding::skill::WHO_YOU_ARE_NO_ASK));
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
        // Asked, the model declines: posted once.
        asked_now();
        assert!(answering());
        assert_eq!(declined(&call, Some(&file)).0["ok"], true);
        assert_eq!(mailbox(), "{\"learn_later\":\"declined\"}\n");
        assert!(!answering());
        assert!(declined(&call, Some(&file)).0.get("error").is_some());
        // Asked, a name: answered, never declined.
        asked_now();
        remembered(Some(&file));
        assert!(mailbox().ends_with("{\"learn_later\":\"answered\"}\n"));
        assert!(!answering());
        assert!(!ANSWER_NOTE.is_empty() && ANSWER_NOTE.contains(DECLINED));
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
    fn the_moment_is_after_the_greeting_turn() {
        let mut moment = Moment::default();
        moment.saw(0, Some(1), true);
        assert!(!moment.heard("Hi lamp.", S));
        assert!(!moment.is_now(2 * S), "the lamp has not answered");
        moment.lamp_said(2 * S);
        assert!(!moment.is_now(2 * S + PAUSE_US - 1));
        assert!(moment.is_now(2 * S + PAUSE_US));
        assert!(!moment.is_now(2 * S + MOMENT_US + 1));
        moment.asked(4 * S);
        assert!(!moment.is_now(4 * S + PAUSE_US));
        assert!(!moment.heard("I don't mind, call me Sam.", 6 * S));
        // A second line is no longer the moment.
        assert!(!moment.is_now(8 * S));
    }
}
