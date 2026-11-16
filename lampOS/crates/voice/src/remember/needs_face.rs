//! Nothing personal kept while nobody is known (`LAMP_MEMORY_PERSONAL_NEEDS_FACE`,
//! the owner's question 1, option (b)). On lamp-c0a4 "the user's favourite
//! colour is blue", said with no face known, was kept as the room's, so
//! every later visitor's sessions carried it. With the switch, and nobody
//! known, a `remember` and the observation pass's proposals keep nothing
//! that `LAMP_MEMORY_ROOM_AFTER_FORGET`'s detector
//! ([`super::about_someone`]) marks as about a person. A room fact ("the
//! plant needs water on Fridays") is kept as before, and a known face's own
//! is kept to them. A conversation nobody known was seen in is not given to
//! the observation pass at all ([`observes`]): the room's would keep a
//! guest's news worded without a pronoun (reviewer-three's P2 on #3450).
//! Off, all three are what they were.
use lamp_gemini::ToolCall;
use serde_json::{Value, json};

/// Only the exact value `on`; off unless set.
pub const ENVIRONMENT: &str = "LAMP_MEMORY_PERSONAL_NEEDS_FACE";

/// Whether the switch is on.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// What the model is told for a `remember` not kept, in a refused one's
/// shape (`{"remembered": false, "note": …}`): that it wasn't kept, and how
/// to have it kept (say who they are or show their face, then say it again),
/// short and friendly. Nothing is held for later, so it promises no later keep.
pub const NOT_KEPT: &str = "This wasn't kept: nobody known is speaking. If they want it kept, they can tell you who they are, or let the lamp see their face, and say it again. Say so in a few friendly words, not as a refusal. Never say it was kept or will be kept later.";

/// The journal's line for a `remember` not kept: no words of it.
pub const SKIPPED_REMEMBER: &str =
    "lamp-voice provider: a remember about a person was not kept: speaker unknown (skipped=1)";

/// The journal's line for a `remember` of the speaker's own name sent to
/// the face save ([`withheld`]): no words of it.
pub const TO_FACE_SAVE: &str = "lamp-voice provider: a remember of the speaker's own name while nobody is known was sent to the face save (LAMP_NAME_LEARN_FIX)";

/// What the model is told instead of the memory's answer, when `on`, nobody
/// is known (`person` is `None`, the room's scope) and the `remember`'s text
/// is about a person. `None`: ask the memory as before. With `to_face`
/// (`LAMP_NAME_LEARN_FIX`, #7331), a text that gives the speaker's own name
/// ([`crate::known_faces::gives_a_name`], or `named::own_name_in` for the
/// model's "The user's name is ...", never "her/his/your name is") is told to go to the face save
/// ([`face_save_note`]) in place of [`NOT_KEPT`]: on lamp-52e6 the owner's
/// "my name is ..." came back NOT_KEPT while the eyes held their face as
/// nobody's. The face save takes a fresh look and the eyes judge the face.
pub(crate) fn withheld(
    call: &ToolCall,
    person: Option<&str>,
    on: bool,
    to_face: bool,
) -> Option<Value> {
    let text = call.args.get("text").and_then(Value::as_str);
    let personal = call.name == super::REMEMBER && text.is_some_and(super::about_someone);
    (on && person.is_none() && personal).then(|| {
        let text = text.unwrap_or_default();
        // The speaker's own name only: "Her name is Linh" is someone
        // else's, never a face save of the speaker's under it (#7343).
        let name = super::named::own_name_in(text)
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty());
        if to_face && (name.is_some() || crate::known_faces::gives_a_name(text)) {
            eprintln!("{TO_FACE_SAVE}");
            return json!({"remembered": false, "note": face_save_note(name.as_deref())});
        }
        eprintln!("{SKIPPED_REMEMBER}");
        json!({"remembered": false, "note": NOT_KEPT})
    })
}

/// What the model is told for their own name given while nobody is known
/// ([`withheld`]): to keep it with their face, by the name when it reads.
fn face_save_note(name: Option<&str>) -> String {
    let call = match name {
        Some(name) => format!(
            "call {} now with name \"{name}\"",
            crate::known_faces::REMEMBER
        ),
        None => format!(
            "call {} now with the name they gave",
            crate::known_faces::REMEMBER
        ),
    };
    format!(
        "Not kept as a memory: this is their own name, and the lamp keeps a name with the face. Don't say it wasn't kept; {call}, so the lamp looks at them and keeps their face under it, and say what it answers."
    )
}

/// Whether a conversation that ended is given to the observation pass: when
/// `on`, only one a known person was seen in (`person` is `Some`); a room's
/// conversation is closed with nothing kept, as an explicit `remember` of a
/// room fact still keeps it. Off, every conversation is.
pub(crate) fn observes(person: Option<&str>, on: bool) -> bool {
    !on || person.is_some()
}

/// The journal's line for a room's conversation not observed; counts only.
pub const ROOM_NOT_OBSERVED: &str = "lamp-voice provider: observation pass: a conversation nobody known was seen in is not observed, closed with nothing kept";

/// The observation pass's proposals as they go to the memory: when `on` and
/// nobody is known, without those about a person; with how many were
/// dropped.
pub(crate) fn kept_proposals(
    mut observations: Vec<Value>,
    person: Option<&str>,
    on: bool,
) -> (Vec<Value>, usize) {
    if !on || person.is_some() {
        return (observations, 0);
    }
    let before = observations.len();
    observations.retain(|observation| {
        !observation["text"]
            .as_str()
            .is_some_and(super::about_someone)
    });
    let dropped = before - observations.len();
    (observations, dropped)
}

/// The journal's line for the proposals dropped: a count, no words.
pub fn dropped_line(dropped: usize) -> String {
    format!(
        "lamp-voice provider: observation pass: dropped {dropped} about a person: speaker unknown"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERSONAL: &str = "the user's favourite colour is blue";
    const KEYS: &str = "I left my keys on the shelf";
    const ROOM: &str = "the plant needs water on Fridays";
    const MINH: &str = "p-be9688998f085098";

    fn remember(text: &str) -> ToolCall {
        ToolCall {
            id: "c1".into(),
            name: super::super::REMEMBER.into(),
            args: json!({ "text": text }),
        }
    }

    fn proposals() -> Vec<Value> {
        vec![
            json!({"text": PERSONAL, "category": "preference", "source_ids": [1]}),
            json!({"text": ROOM, "category": "thread", "source_ids": [2]}),
            json!({"text": KEYS, "category": "thread", "source_ids": [3]}),
        ]
    }

    /// The c0a4 case: an unknown speaker's personal line is not kept, and
    /// the model is told why and how to have it kept.
    #[test]
    fn an_unknown_speaker_s_personal_remember_is_not_kept_and_says_why() {
        for text in [PERSONAL, KEYS] {
            let told = withheld(&remember(text), None, true, false).expect(text);
            assert_eq!(told["remembered"], false);
            let note = told["note"].as_str().unwrap();
            assert!(note.contains("nobody known is speaking"), "{note}");
            assert!(note.contains("say it again"), "{note}");
        }
        assert!(!SKIPPED_REMEMBER.contains("blue"));
    }

    /// Nothing is held for later, so the note promises no later keep.
    #[test]
    fn the_note_promises_no_later_keep() {
        for promise in ["I'll remember", "I will remember", "once I know"] {
            assert!(!NOT_KEPT.contains(promise), "{promise}: {NOT_KEPT}");
        }
    }

    /// Not kept from an unknown speaker; the same line said again by a
    /// known person goes to the memory, kept to them.
    #[test]
    fn the_same_line_said_again_by_a_known_person_is_kept_to_them() {
        let call = remember(PERSONAL);
        assert!(withheld(&call, None, true, false).is_some());
        assert_eq!(withheld(&call, Some(MINH), true, false), None);
        let (socket, served) =
            super::super::tests::memory("needs-face-again", 1, |_| json!({"ok":true,"id":1}));
        let deadline = std::time::Duration::from_secs(2);
        assert_eq!(
            super::super::run_for(&socket, &call, deadline, Some(MINH)),
            (
                super::super::tests::theirs(json!({"remembered":true})),
                true
            )
        );
        let read = served.join().unwrap();
        assert_eq!(read[0]["op"], "remember");
        assert_eq!(read[0]["person"], MINH);
        assert_eq!(read[0]["text"], PERSONAL);
        let _ = std::fs::remove_file(&socket);
    }

    /// #7331 (lamp-52e6, 15 Nov): the owner's "my name is ..." on a line the
    /// eyes held as nobody's came back NOT_KEPT. With `LAMP_NAME_LEARN_FIX`
    /// a name given while nobody is known goes to the face save, which takes
    /// a fresh look and judges the face; any other personal line is still
    /// not kept, and so is the name with the switch off.
    #[test]
    fn a_name_given_with_nobody_known_goes_to_the_face_save() {
        for (text, name) in [
            ("my name is Ada", "Ada"),
            ("The user's name is Ada.", "Ada"),
        ] {
            let told = withheld(&remember(text), None, true, true).expect(text);
            assert_eq!(told["remembered"], false, "{text}");
            let note = told["note"].as_str().unwrap();
            assert!(note.contains(crate::known_faces::REMEMBER), "{note}");
            assert!(note.contains(&format!("\"{name}\"")), "{note}");
            assert_ne!(note, NOT_KEPT);
        }
        let told = withheld(&remember(PERSONAL), None, true, true).unwrap();
        assert_eq!(told["note"], NOT_KEPT);
        // Someone else's name is no name of the speaker's: never a face save
        // under it (the Reviewer-lead's P2-2 on #7343).
        for text in [
            "Her name is Linh.",
            "His name is Linh.",
            "Your name is Linh.",
            "Their name is Linh.",
        ] {
            let told = withheld(&remember(text), None, true, true).expect(text);
            assert_eq!(told["note"], NOT_KEPT, "{text}");
        }
        let told = withheld(&remember("my favourite colour is blue"), None, true, true).unwrap();
        assert_eq!(told["note"], NOT_KEPT);
        // Switch off: not kept, as before.
        let told = withheld(&remember("my name is Ada"), None, true, false).unwrap();
        assert_eq!(told["note"], NOT_KEPT);
        // A known face's own name goes to the memory as before.
        assert_eq!(
            withheld(&remember("my name is Ada"), Some(MINH), true, true),
            None
        );
        assert!(!TO_FACE_SAVE.contains("Ada"));
    }

    #[test]
    fn an_unknown_speaker_s_room_fact_is_still_kept() {
        assert_eq!(withheld(&remember(ROOM), None, true, false), None);
    }

    #[test]
    fn a_known_face_s_personal_remember_is_kept_to_them() {
        assert_eq!(withheld(&remember(PERSONAL), Some(MINH), true, false), None);
        assert_eq!(withheld(&remember(KEYS), Some(MINH), true, false), None);
    }

    #[test]
    fn switch_off_a_personal_remember_goes_to_the_room_as_today() {
        assert_eq!(withheld(&remember(PERSONAL), None, false, false), None);
    }

    #[test]
    fn an_unknown_speaker_s_personal_proposals_are_dropped_room_facts_kept() {
        let (kept, dropped) = kept_proposals(proposals(), None, true);
        assert_eq!(dropped, 2);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0]["text"], ROOM);
        let line = dropped_line(dropped);
        assert!(
            line.contains("dropped 2") && !line.contains("keys"),
            "{line}"
        );
    }

    #[test]
    fn a_known_face_s_and_switch_off_proposals_are_all_kept() {
        assert_eq!(
            kept_proposals(proposals(), Some(MINH), true),
            (proposals(), 0)
        );
        assert_eq!(kept_proposals(proposals(), None, false), (proposals(), 0));
    }
}
