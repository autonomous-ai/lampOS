//! A look the person asked for, told and kept with what they said
//! (`LAMP_LOOK_TOLD`, `LAMP_LOOK_KEPT_WITH_ASK`).
//!
//! On lamp-06c1 (17 Nov) the person said several times that the toy on the
//! desk was their red toy motorcycle, and the lamp had kept it; each of
//! three looks still named it a red toy camera. The look turn's words were
//! only the fixed note beside the picture, and what the person said sat far
//! up in the history, so each new picture overrode the conversation.
//!
//! - With [`TOLD_ENVIRONMENT`] the route's own look function
//!   (`take_a_look`) takes two optional arguments the model writes:
//!   `question`, what they asked, and `told`, what they already said about
//!   it. They go beside the picture, before the note, with memory's hits for
//!   the question under the asker's scope ([`turn_text`]). The model judges
//!   what fits: no word lists. With no arguments the note is as before.
//! - With [`KEPT_ENVIRONMENT`] the look's turn goes on from the turn that
//!   asked it: told and kept in the asker's scope (the room's for a held,
//!   carried or unknown asker), and kept as one exchange with the
//!   question (`thread::Thread::look_asked`). Words only: never the picture.

use serde_json::{Value, json};

/// The switch for the look's arguments and their place beside the picture:
/// `on`, and nothing else.
pub const TOLD_ENVIRONMENT: &str = "LAMP_LOOK_TOLD";
/// The switch for the look kept with its question, in the asker's scope:
/// `on`, and nothing else.
pub const KEPT_ENVIRONMENT: &str = "LAMP_LOOK_KEPT_WITH_ASK";

/// Whether [`TOLD_ENVIRONMENT`] is on in this process.
pub fn told_on() -> bool {
    switch(std::env::var(TOLD_ENVIRONMENT).ok().as_deref())
}

/// Whether [`KEPT_ENVIRONMENT`] is on in this process.
pub fn kept_on() -> bool {
    switch(std::env::var(KEPT_ENVIRONMENT).ok().as_deref())
}

fn switch(value: Option<&str>) -> bool {
    value == Some("on")
}

/// What the look's function tells the model of its arguments.
pub const QUESTION_ABOUT: &str = "What they asked you about what you will see, in their words, such as \"what movie is this toy from?\". Leave it out if they only asked you to look.";
pub const TOLD_ABOUT: &str = "What they already told you about it in this conversation, such as \"my red toy motorcycle\". Leave it out if they told you nothing about it.";

/// The look function's parameters with [`TOLD_ENVIRONMENT`]: both optional.
pub fn parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "question": {"type": "string", "description": QUESTION_ABOUT},
            "told": {"type": "string", "description": TOLD_ABOUT},
        },
    })
}

/// What the model wrote with its look call: each argument trimmed, and
/// none for a missing, empty or non-string one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LookArgs {
    pub question: Option<String>,
    pub told: Option<String>,
}

impl LookArgs {
    pub fn from_args(args: &Value) -> Self {
        let text = |name: &str| {
            args.get(name)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| text.chars().any(char::is_alphanumeric))
                .map(str::to_owned)
        };
        Self {
            question: text("question"),
            told: text("told"),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.question.is_none() && self.told.is_none()
    }

    /// The words memory's hits are read for: the question, or else the
    /// line that asked for the look (`heard`), if it has words.
    pub fn hits_words<'a>(&'a self, heard: &'a str) -> Option<&'a str> {
        self.question
            .as_deref()
            .or_else(|| heard.chars().any(char::is_alphanumeric).then_some(heard))
    }
}

/// Said beside the picture when they told the lamp what it is.
pub const TOLD_LINE: &str = "Answer from what you see. If it doesn't clearly look like what they told you, say what you can make out and that it may be what they mean; don't name it as something else.";

/// The look turn's words with [`TOLD_ENVIRONMENT`]: the question and what
/// they told the lamp, each quoted as data, then memory's `hits`, then the
/// look's own `note` (the fixed note and anything after it), all beside the
/// picture. With no arguments and no hits, `note` alone, as before.
pub fn turn_text(args: &LookArgs, hits: Option<&str>, note: &str) -> String {
    let mut parts = Vec::new();
    if let Some(question) = &args.question {
        parts.push(format!("They asked: {}", json!(question)));
    }
    if let Some(told) = &args.told {
        parts.push(format!("They told you it is: {}", json!(told)));
        parts.push(TOLD_LINE.to_owned());
    }
    let mut text = parts.join("\n");
    if let Some(hits) = hits.filter(|hits| !hits.trim().is_empty()) {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(hits);
    }
    if text.is_empty() {
        return note.to_owned();
    }
    text.push_str("\n\n");
    text.push_str(note);
    text
}

/// A look the route's own function asked for in a spoken turn: its session
/// and request, the model's arguments, the words of the line that asked,
/// and the asker's scope as it was asked, for the look turn after it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LookAsk {
    pub session: u64,
    pub request: u64,
    pub args: LookArgs,
    pub heard: String,
    /// Taken from the asking turn's own lease as the look is asked: the
    /// look request's authority retires that turn before the look turn
    /// begins (Twelve's P1 on #8019). `None` for a voice with no memory
    /// scope.
    pub asker: Option<AskerScope>,
}

/// The asker's scope as a look was asked (`second_provider::look_scope`,
/// `look_hits_scope`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AskerScope {
    /// The scope the look turn may go on in: the asker's own, or the room's
    /// for a held, carried, revoked or unknown asker.
    pub kept: crate::memory_scope::ScopeSnapshot,
    /// The scope memory's hits for it may be read in; none for a held or
    /// revoked asker.
    pub hits: Option<crate::memory_scope::ScopeSnapshot>,
    /// The asking line was held only by a face's gap.
    pub held: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTE: &str = "(You are looking now.)";

    #[test]
    fn arguments_are_read_trimmed_and_blank_ones_are_none() {
        assert_eq!(LookArgs::from_args(&Value::Null), LookArgs::default());
        assert_eq!(LookArgs::from_args(&json!({})), LookArgs::default());
        assert_eq!(
            LookArgs::from_args(
                &json!({"question":"  what movie is this toy from? ","told":" ","x":1})
            ),
            LookArgs {
                question: Some("what movie is this toy from?".into()),
                told: None,
            }
        );
        assert_eq!(
            LookArgs::from_args(&json!({"question": 3, "told": "my red toy motorcycle"})),
            LookArgs {
                question: None,
                told: Some("my red toy motorcycle".into()),
            }
        );
    }

    #[test]
    fn no_arguments_and_no_hits_leave_the_note_as_it_was() {
        assert_eq!(turn_text(&LookArgs::default(), None, NOTE), NOTE);
        assert_eq!(turn_text(&LookArgs::default(), Some(" "), NOTE), NOTE);
    }

    #[test]
    fn the_question_and_what_they_told_go_before_the_note_quoted() {
        let args = LookArgs {
            question: Some("what movie is this toy from?".into()),
            told: Some("my red \"toy\" motorcycle".into()),
        };
        assert_eq!(
            turn_text(&args, Some("From your memory: a red toy motorcycle."), NOTE),
            format!(
                "They asked: \"what movie is this toy from?\"\nThey told you it is: \"my red \\\"toy\\\" motorcycle\"\n{TOLD_LINE}\n\nFrom your memory: a red toy motorcycle.\n\n{NOTE}"
            )
        );
        // A question alone asks nothing of what they told.
        let asked = LookArgs {
            question: Some("what colour is it?".into()),
            told: None,
        };
        let text = turn_text(&asked, None, NOTE);
        assert_eq!(
            text,
            format!("They asked: \"what colour is it?\"\n\n{NOTE}")
        );
        assert!(!text.contains(TOLD_LINE));
        // Hits alone go before the note too.
        assert_eq!(
            turn_text(&LookArgs::default(), Some("hits"), NOTE),
            format!("hits\n\n{NOTE}")
        );
    }

    #[test]
    fn hits_are_read_for_the_question_or_else_the_line() {
        let none = LookArgs::default();
        assert_eq!(
            none.hits_words("take a look at it"),
            Some("take a look at it")
        );
        assert_eq!(none.hits_words(" "), None);
        let asked = LookArgs {
            question: Some("what movie is this toy from?".into()),
            told: None,
        };
        assert_eq!(
            asked.hits_words("take a look"),
            Some("what movie is this toy from?")
        );
    }

    #[test]
    fn the_parameters_are_both_optional_strings() {
        let schema = parameters();
        assert!(schema.get("required").is_none());
        assert_eq!(schema["properties"]["question"]["type"], "string");
        assert_eq!(schema["properties"]["told"]["type"], "string");
    }

    #[test]
    fn the_switches_are_on_only_for_on() {
        assert!(switch(Some("on")));
        assert!(!switch(Some("1")));
        assert!(!switch(None));
    }
}
