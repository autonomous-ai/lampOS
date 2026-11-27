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
/// short and friendly, asking nothing and never "Got it" (#7877). Nothing
/// is held for later, so it promises no later keep.
pub const NOT_KEPT: &str = "This wasn't kept: nobody known is speaking. If they want it kept, they can tell you who they are, or let the lamp see their face, and say it again. Say so in a few friendly words, not as a refusal. Never say it was kept or will be kept later, never \"Got it\" or \"Noted\", and ask nothing.";

/// What the second route says itself for a line that only asks to keep,
/// every `remember` of it refused with [`NOT_KEPT`] (#7877: on lamp-e82d,
/// 19 Nov, such keeps were left to the model's words, which may say "Got
/// it"). One true line, for a face the lamp doesn't know and for a known
/// person not seen saying it alike: it asks nothing and never says it kept.
pub const REFUSED_LINE: &str = "I'm not sure who's talking, so I can't keep that.";

/// Whether `told` is a `remember` refused with [`NOT_KEPT`]
/// ([`withheld_as`]).
pub fn refused(told: &Value) -> bool {
    told["remembered"] == false && told["note"] == NOT_KEPT
}

/// With `LAMP_FACE_NO_ASK` and the face save not speaker-bound
/// ([`NoAsk::Quiet`], the first builder 21:13): their own name given while
/// nobody is known keeps nothing, and nothing is asked or said about it.
pub const NAME_QUIET: &str = "Not kept: this is their own name, and the lamp keeps a name only with the face it sees saying it, which it could not here. Ask them nothing, and say nothing about their name or face being kept; just carry on. Only if they asked in words for their face to be remembered, say plainly in a few words that you couldn't see the face saying it, so you can't keep it now, still asking nothing.";

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
/// ([`face_save_note`]) in place of [`NOT_KEPT`]: on lamp-74ff the owner's
/// "my name is ..." came back NOT_KEPT while the eyes held their face as
/// nobody's. The face save takes a fresh look and the eyes judge the face.
#[cfg(test)]
pub(crate) fn withheld(
    call: &ToolCall,
    person: Option<&str>,
    on: bool,
    to_face: bool,
) -> Option<Value> {
    withheld_as(call, person, on, to_face, NoAsk::Off)
}

/// `LAMP_FACE_NO_ASK` for a name given while nobody is known
/// ([`face_save_note`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NoAsk {
    /// Off: the face question, as before.
    Off,
    /// On, the face save speaker-bound (`LAMP_REMEMBER_SPEAKER`, the first
    /// builder 18:50): the face save is called at once, nothing asked.
    Save,
    /// On, the face save not speaker-bound (the first builder 21:13):
    /// nothing kept, nothing asked.
    Quiet,
}

/// The [`NoAsk`] for a name given while nobody is known: off without
/// `LAMP_FACE_NO_ASK` (`face_no_ask`); with it, the face save at once where
/// the memory route's face save is speaker-bound (`remember_speaker`, its
/// [`crate::known_faces::FaceSwitches`], #7876: a worker's environment never
/// holds `LAMP_REMEMBER_SPEAKER`), nothing kept and nothing asked elsewhere.
pub(crate) fn no_ask(face_no_ask: bool, remember_speaker: bool) -> NoAsk {
    if !face_no_ask {
        NoAsk::Off
    } else if remember_speaker {
        NoAsk::Save
    } else {
        NoAsk::Quiet
    }
}

/// [`withheld`], with `no_ask` (`LAMP_FACE_NO_ASK`): nothing asked
/// ([`face_save_note`]).
pub(crate) fn withheld_as(
    call: &ToolCall,
    person: Option<&str>,
    on: bool,
    to_face: bool,
    no_ask: NoAsk,
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
            return json!({"remembered": false, "note": face_save_note(name.as_deref(), no_ask)});
        }
        eprintln!("{SKIPPED_REMEMBER}");
        json!({"remembered": false, "note": NOT_KEPT})
    })
}

/// What the model is told for their own name given while nobody is known
/// ([`withheld`]): to ask them whether to keep their face under it, by the
/// name when it reads. A name is no yes to keeping a face, so the save runs
/// only on their yes (`known_faces::ASK_FACE_FIRST`, the first builder on
/// #7331). With `no_ask` (`LAMP_FACE_NO_ASK`) nothing is asked: where the
/// face save is speaker-bound it is called now, and its answer says whether
/// the face was kept ([`NoAsk::Save`]); elsewhere nothing is kept, and only
/// their own ask for their face is told the plain reason ([`NoAsk::Quiet`],
/// as `known_faces::CANT_KEEP_NO_ASK`).
fn face_save_note(name: Option<&str>, no_ask: NoAsk) -> String {
    if no_ask == NoAsk::Quiet {
        return NAME_QUIET.to_owned();
    }
    if no_ask == NoAsk::Save {
        let call = match name {
            Some(name) => format!("call {} with name \"{name}\"", crate::known_faces::REMEMBER),
            None => format!(
                "call {} with the name they gave",
                crate::known_faces::REMEMBER
            ),
        };
        return format!(
            "Not kept as a memory: this is their own name, and the lamp keeps a name with their face. Don't say it wasn't kept, and ask them nothing: {call} now, so the lamp looks at them and keeps their face under it, and say only what it answers."
        );
    }
    let (ask, call) = match name {
        Some(name) => (
            format!("\"Shall I remember your face as {name}?\""),
            format!("call {} with name \"{name}\"", crate::known_faces::REMEMBER),
        ),
        None => (
            "\"Shall I remember your face as\" and their name".to_owned(),
            format!(
                "call {} with the name they gave",
                crate::known_faces::REMEMBER
            ),
        ),
    };
    format!(
        "Not kept as a memory: this is their own name, and the lamp keeps a name with their face. Don't say it wasn't kept; ask them only this: {ask}. Only after their yes, {call}, so the lamp looks at them and keeps their face under it, and say what it answers. If they say no, keep nothing and say that's fine."
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
            super::super::run_for(&socket, &call, deadline, Some(MINH), false),
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

    /// #7331 (lamp-74ff, 16 Nov): the owner's "my name is ..." on a line the
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
            // A question to the person, not a save: a name is no yes to
            // keeping a face (the first builder on #7331).
            assert!(
                note.contains(&format!("\"Shall I remember your face as {name}?\"")),
                "{note}"
            );
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

    /// `LAMP_FACE_NO_ASK` (the first builder 18:50): their own name given
    /// while nobody is known goes to the face save at once, nothing asked;
    /// off, the face question as before.
    #[test]
    fn with_no_ask_a_name_goes_to_the_face_save_unasked() {
        let call = remember("my name is Ada");
        let told = withheld_as(&call, None, true, true, NoAsk::Save).unwrap();
        let note = told["note"].as_str().unwrap();
        assert!(note.contains("ask them nothing"), "{note}");
        assert!(
            note.contains(&format!(
                "call {} with name \"Ada\" now",
                crate::known_faces::REMEMBER
            )),
            "{note}"
        );
        assert!(!note.contains("Shall I"), "{note}");
        let off = withheld_as(&call, None, true, true, NoAsk::Off).unwrap();
        assert!(
            off["note"]
                .as_str()
                .unwrap()
                .contains("\"Shall I remember your face as Ada?\"")
        );
        // Not their own name: not kept, as before.
        let told = withheld_as(
            &remember("Her name is Linh."),
            None,
            true,
            true,
            NoAsk::Save,
        )
        .unwrap();
        assert_eq!(told["note"], NOT_KEPT);
    }

    /// `LAMP_FACE_NO_ASK` with the face save not speaker-bound
    /// (`LAMP_REMEMBER_SPEAKER` off, the first builder 21:13): their own
    /// name given while nobody is known keeps nothing and asks nothing, no
    /// face question and no call to the face save; someone else's name is
    /// not kept, as before.
    #[test]
    fn with_no_ask_not_speaker_bound_a_name_keeps_nothing_and_asks_nothing() {
        for text in ["my name is Ada", "I'm Ada"] {
            let told = withheld_as(&remember(text), None, true, true, NoAsk::Quiet).unwrap();
            assert_eq!(told["remembered"], false, "{text}: {told}");
            let note = told["note"].as_str().unwrap();
            assert_eq!(note, NAME_QUIET, "{text}");
            assert!(note.contains("Ask them nothing"), "{note}");
            assert!(!note.contains("Shall I"), "{note}");
            assert!(!note.contains(crate::known_faces::REMEMBER), "{note}");
        }
        let told = withheld_as(
            &remember("Her name is Linh."),
            None,
            true,
            true,
            NoAsk::Quiet,
        )
        .unwrap();
        assert_eq!(told["note"], NOT_KEPT);
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

    /// #7876: in a provider or second-route worker the speaker-bound face
    /// save arrives as `--remember-speaker` through `tools::take`, never as
    /// `LAMP_REMEMBER_SPEAKER` (the worker's environment is cleared, the
    /// switch census's `remember_speaker_arguments_survive_both_workers_
    /// cleared_environments`). With `LAMP_FACE_NO_ASK` on, the needs-face
    /// choice for a name given while nobody is known is the face save at
    /// once (`NoAsk::Save`), read from the memory route's switches, never
    /// `Quiet` for want of an environment variable the worker never has.
    #[test]
    fn a_worker_s_remember_speaker_flag_makes_the_needs_face_choice_save() {
        let flags: Vec<String> = [
            super::super::FLAG,
            "/tmp/memory.sock",
            "--who-socket",
            "/tmp/eyes.sock",
            crate::known_faces::NAMES_FLAG,
            crate::remember_speaker::FLAG,
        ]
        .map(str::to_owned)
        .into();
        let (settings, rest) = crate::tools::take(&flags).unwrap();
        assert!(rest.is_empty(), "{rest:?}");
        assert!(settings.remember_speaker);
        // The worker's environment, as the coordinator builds it.
        assert!(std::env::var_os(crate::remember_speaker::ENVIRONMENT).is_none());
        let crate::tools::Route::Memory(_, Some((_, switches)), ..) =
            crate::tools::route(&settings, super::super::REMEMBER)
        else {
            panic!("remember is the memory's, with the eyes' switches");
        };
        // `LAMP_FACE_NO_ASK=on`, as the boolean the choice takes.
        assert_eq!(no_ask(true, switches.remember_speaker), NoAsk::Save);
        assert_eq!(no_ask(true, false), NoAsk::Quiet);
        assert_eq!(no_ask(false, switches.remember_speaker), NoAsk::Off);
    }
}
