//! The journal's line for each `remember` call, with
//! `LAMP_MEMORY_REPLACES=on` (#2510): counts and yes/no only, never a word
//! of the call, what was heard or what was kept.
//!
//! On lamp-06c1 a correction ("it's green now") was never kept as one, and
//! nothing in the journal said whether the model sent no `replaces` or a
//! sentence the store, or `LAMP_MEMORY_KEEP_HEARD`, would not take. The line
//! says, for one call: whether the model sent `replaces`; whether that id
//! was among the ids this session was shown of the call's scope; how many
//! content words of the model's text are neither the old fact's nor heard
//! ([`new_words`], [`super::content_words`]' folding); with KEEP_HEARD on,
//! whose words were sent (the model's or the heard line); and the store's
//! answer kind ([`answer_kind`]).
//!
//! The old fact's text is counted only when the session holds it: until it
//! keeps the texts it was shown by id (#2704), it holds none, the line says
//! `old=n`, and the count is of the words not heard.
use super::{DaysClient, REMEMBER, content_words, days_client, person_now};
use lamp_gemini::ToolCall;
use serde_json::Value;
use std::{cell::RefCell, path::Path, sync::atomic::Ordering};

/// The journal line's beginning.
pub const LINE: &str = "lamp-voice provider: remember:";

thread_local! {
    /// The store's answer kind for this thread's last `remember`, set as
    /// the answer comes ([`DaysClient::run_at_guarded`]); a call runs on the
    /// thread that asked it.
    static ANSWERED: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// A code the store gave, as the journal may say it: lower-case letters and
/// `_` alone, at most 32 of them, else `other`.
fn code(value: Option<&str>) -> &str {
    value
        .filter(|code| {
            !code.is_empty()
                && code.len() <= 32
                && code
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
        })
        .unwrap_or("other")
}

/// What kind of answer the store gave a `remember`: `kept`, `existing`
/// (`once` found it kept already), `replaced`, `refused:<reason code>`, or
/// `error:<code>`. No text of the answer's.
pub(super) fn answer_kind(answer: &Value) -> String {
    if answer["ok"] == true {
        return if answer["existing"] == true {
            "existing"
        } else if answer["replaced"].is_u64() {
            "replaced"
        } else {
            "kept"
        }
        .to_owned();
    }
    match answer["error"].as_str() {
        Some("refused") => format!("refused:{}", code(answer["reason"].as_str())),
        error => format!("error:{}", code(error)),
    }
}

/// Noted for this thread's `remember` now being answered.
pub(super) fn answered(answer: &Value) {
    ANSWERED.with(|kind| *kind.borrow_mut() = Some(answer_kind(answer)));
}

fn take_answered() -> Option<String> {
    ANSWERED.with(|kind| kind.borrow_mut().take())
}

/// How many content words of `text` are neither `old`'s nor `heard`'s, all
/// three folded by [`content_words`] (lower case, three letters or more,
/// small words left out, a plural's `s` taken off).
pub fn new_words(text: &str, old: Option<&str>, heard: Option<&str>) -> usize {
    let known: Vec<String> = [old, heard]
        .into_iter()
        .flatten()
        .flat_map(content_words)
        .collect();
    content_words(text)
        .iter()
        .filter(|word| !known.contains(word))
        .count()
}

/// One `remember` call, as its journal line counts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remembered {
    /// The model sent `replaces`.
    pub replaces: bool,
    /// Its id was among the ids this session was shown of the call's
    /// scope; `None` without a `replaces`.
    pub shown: Option<bool>,
    /// [`new_words`] of the model's text.
    pub new_words: usize,
    /// The old fact's text was known to count against.
    pub old: bool,
    /// With `LAMP_MEMORY_KEEP_HEARD`: whether the heard line was sent in
    /// place of the model's text. `None` with it off.
    pub heard_kept: Option<bool>,
    /// [`answer_kind`], or `none` when the store gave no answer.
    pub answer: String,
}

fn yes(on: bool) -> &'static str {
    if on { "y" } else { "n" }
}

impl Remembered {
    /// The journal's line: counts and yes/no only.
    pub fn line(&self) -> String {
        let mut line = format!(
            "{LINE} replaces={} shown={} new_words={} old={}",
            yes(self.replaces),
            self.shown.map_or("-", yes),
            self.new_words,
            yes(self.old),
        );
        if let Some(heard) = self.heard_kept {
            line.push_str(if heard {
                " keep_heard=heard"
            } else {
                " keep_heard=model"
            });
        }
        line.push_str(" answer=");
        line.push_str(&self.answer);
        line
    }
}

/// Run `run`, the memory's `remember` for the model's `model` call, and
/// journal its line, when `socket` is the Days client's and
/// [`super::REPLACES_ENVIRONMENT`] is on; any other call, or the switch off,
/// is `run` alone, and nothing new is journalled. `heard` is what was heard
/// in the call's line; `heard_kept` is KEEP_HEARD's choice, `None` with it
/// off.
pub fn remember_noted(
    socket: &Path,
    model: &ToolCall,
    heard: Option<&str>,
    heard_kept: Option<bool>,
    lease: Option<&crate::memory_scope::ScopeLease>,
    run: impl FnOnce() -> (Value, bool),
) -> (Value, bool) {
    let Some(client) = days_client().filter(|client| client.socket == socket) else {
        return run();
    };
    if model.name != REMEMBER || !client.replaces.load(Ordering::Acquire) {
        return run();
    }
    let person = match lease {
        Some(lease) => lease
            .scope()
            .and_then(|scope| scope.person().map(str::to_owned)),
        None => person_now(),
    };
    let (told, line) = client.noted(person.as_deref(), model, heard, heard_kept, run);
    if let Some(line) = line {
        eprintln!("{line}");
    }
    told
}

impl DaysClient {
    /// [`remember_noted`] for this client, in `person`'s scope: what `run`
    /// gave, and the line; no line with the switch off or for another call.
    pub(super) fn noted(
        &self,
        person: Option<&str>,
        model: &ToolCall,
        heard: Option<&str>,
        heard_kept: Option<bool>,
        run: impl FnOnce() -> (Value, bool),
    ) -> ((Value, bool), Option<String>) {
        if model.name != REMEMBER || !self.replaces.load(Ordering::Acquire) {
            return (run(), None);
        }
        let replaces = model.args.get("replaces").filter(|id| !id.is_null());
        // Before the call: a kept correction adds its new id to what was
        // shown.
        let shown = replaces.map(|id| {
            id.as_u64().is_some_and(|id| {
                self.context
                    .lock()
                    .is_ok_and(|context| context.shown(person).contains(&id))
            })
        });
        let text = model.args["text"].as_str().unwrap_or_default();
        // The old fact's text, when the session holds it (#2704): none yet.
        let old: Option<&str> = None;
        take_answered();
        let told = run();
        let line = Remembered {
            replaces: replaces.is_some(),
            shown,
            new_words: new_words(text, old, heard),
            old: old.is_some(),
            heard_kept,
            answer: take_answered().unwrap_or_else(|| "none".to_owned()),
        }
        .line();
        (told, Some(line))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_stores_answer_is_told_by_its_kind_and_codes_alone() {
        for (answer, kind) in [
            (json!({"ok":true,"id":10}), "kept"),
            (json!({"ok":true,"id":8,"existing":true}), "existing"),
            (json!({"ok":true,"id":9,"replaced":8}), "replaced"),
            (
                json!({"ok":false,"error":"refused","reason":"inactive","message":"violet"}),
                "refused:inactive",
            ),
            // A reason that is no code is never said.
            (
                json!({"ok":false,"error":"refused","reason":"Her colour is violet"}),
                "refused:other",
            ),
            (json!({"ok":false,"error":"refused"}), "refused:other"),
            (
                json!({"ok":false,"error":"bad_request","message":"x"}),
                "error:bad_request",
            ),
            (json!({"ok":false}), "error:other"),
        ] {
            assert_eq!(answer_kind(&answer), kind, "{answer}");
        }
    }

    #[test]
    fn new_words_are_the_texts_content_words_neither_old_nor_heard() {
        let green = "Her favourite colour is green.";
        assert_eq!(new_words(green, None, Some("it's green now")), 2);
        assert_eq!(
            new_words(
                green,
                Some("Her favourite colour is violet."),
                Some("it's green now")
            ),
            0
        );
        // Folded alike: case, a plural's `s`, a possessive.
        assert_eq!(
            new_words(
                "Her sister's favourite COLOURS",
                None,
                Some("my sister favourite colour")
            ),
            0
        );
        assert_eq!(new_words(green, None, None), 3);
        assert_eq!(new_words("", None, None), 0);
    }

    #[test]
    fn the_line_says_each_field_in_counts_and_yes_or_no() {
        let mut remembered = Remembered {
            replaces: true,
            shown: Some(true),
            new_words: 2,
            old: false,
            heard_kept: Some(false),
            answer: "replaced".to_owned(),
        };
        assert_eq!(
            remembered.line(),
            "lamp-voice provider: remember: replaces=y shown=y new_words=2 old=n keep_heard=model answer=replaced"
        );
        remembered.replaces = false;
        remembered.shown = None;
        remembered.heard_kept = None;
        remembered.answer = "kept".to_owned();
        assert_eq!(
            remembered.line(),
            "lamp-voice provider: remember: replaces=n shown=- new_words=2 old=n answer=kept"
        );
    }
}
