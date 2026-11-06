//! A face rename the lamp asked about, held for the person's yes and run by
//! the voice itself (`LAMP_NAMES_RENAME_YES`, #5485), as [`super::HeldSave`]
//! is for the first save at setup.
//!
//! On lamp-29e7, 6 Nov, a known face asked six times to be called by a new
//! name. Each time the lamp asked the right question ("Do you want me to
//! know you as A instead?"), and each yes came to nothing: refused for a
//! word outside the yes word list, or answered with no call at all, or with
//! `remember` in place of `remember_my_face`. The lamp then said it would
//! use the new name, while the face still had the old one.
use super::*;
use crate::known_faces::Said;

/// How long a held rename waits for the yes: as long as a held forget.
const HELD_RENAME_FOR: Duration = crate::remember::HELD_FORGET_FOR;

/// How long a rename asked and not run is told to the model on each turn
/// of its session ([`asked_note`]): as long as the thread remembers the
/// conversation it was asked in.
const ASKED_TOLD_FOR: Duration = crate::thread::REMEMBERED_FOR;

/// A `remember_my_face` the lamp answered by asking whether to know the
/// face by the new name instead (`known_faces::rename_asked`), held as
/// `{name, replace: true}` for the asker's yes on the same session's next
/// spoken turn ([`take`]).
pub(super) struct HeldRename {
    call: net::ToolCall,
    request: u64,
    /// The asking line's session and known person ([`rename_asker`]).
    asker: (u64, String),
    at: Instant,
}

/// A rename asked and not run: told to the model on each turn of its
/// session, so it never says the name changed ([`asked_note`]).
struct RenameAsked {
    to: String,
    /// The name the face is kept under, when the eyes said it.
    from: Option<String>,
    session: u64,
    /// The asker's known person: only their turns are told ([`spoke_it`]).
    person: String,
    at: Instant,
}

#[derive(Default)]
pub(super) struct HeldRenames {
    held: Option<HeldRename>,
    /// Who the eyes saw saying the last spoken line read while a rename was
    /// held: its request, the faces seen speaking it and the known person
    /// with their face, as [`HeldForgets`] keeps it.
    sight: Option<SightRead>,
    asked: Option<RenameAsked>,
    /// The rename a held one ran: its request and name, so a model call
    /// after the yes never renames twice.
    ran: Option<(u64, String)>,
}

/// Whom a rename is held for, or whose yes runs one: the line's session and
/// known person, its own ([`forget_asker`]) or theirs only by their face
/// alone in view with no other voice heard
/// (`memory_scope::ScopeLease::by_one_face`). Never a line held only by a
/// face's gap, nor one carried into a conversation going on, which may be
/// someone else's (the 29e7 requests 19 to 24).
fn rename_asker(scope: Option<&crate::memory_scope::ScopeLease>) -> Option<(u64, String)> {
    let lease = scope?;
    if held_by_gap(scope) || lease.carried_for().is_some() || lease.saves_carried().is_some() {
        return None;
    }
    forget_asker(scope).or_else(|| Some((lease.key().session, lease.kept_by_one_face()?)))
}

/// The faces seen speaking a line, and the known person with their face.
pub(super) type Seen<'a> = (&'a [String], Option<(String, String)>);

/// What the rename's yes turn was, for [`verdict`].
pub(super) struct YesTurn<'a> {
    /// The held new name.
    pub name: &'a str,
    pub said: &'a Said,
    pub asker: &'a (u64, String),
    /// The turn's session, if it has a scope.
    pub session: Option<u64>,
    /// Held only by a face's gap ([`held_by_gap`]).
    pub gap: bool,
    /// Carried into a conversation going on.
    pub carried: bool,
    pub too_late: bool,
    /// The faces the eyes saw speaking the line, with the known person and
    /// their face: none when nothing was read.
    pub seen: Option<Seen<'a>>,
    /// Whose the line is by their face alone in view with no other voice
    /// heard (`ScopeLease::kept_by_one_face`).
    pub one_face: Option<&'a str>,
}

/// Whether the held rename runs on this turn: `Ok` to run it, else why not,
/// and whether the person said yes all the same, for the model to be told
/// (left untold, it made up a reason on lamp-52e6, Fifth's run B on #5415).
/// Runs only on a yes to the new name (`known_faces::rename_agreed`) from
/// the asker, within [`HELD_RENAME_FOR`], in the same session, on a line
/// neither gap-held nor carried, and either their face the one seen saying
/// it, or no face seen saying it and theirs alone in view with no other
/// voice heard (the person's call, 6 Nov).
pub(super) fn verdict(turn: &YesTurn) -> Result<(), (&'static str, bool)> {
    if turn.too_late {
        return Err(("too late", false));
    }
    if turn.gap {
        return Err(("a line held only by a face's gap", false));
    }
    if turn.carried {
        return Err(("a carried line", false));
    }
    if turn.session != Some(turn.asker.0) {
        return Err(("not the same session", false));
    }
    if !crate::known_faces::rename_agreed(turn.name, turn.said) {
        return Err(("not a yes to the new name", false));
    }
    spoke_it(&turn.asker.1, turn.seen.as_ref(), turn.one_face).map_err(|why| (why, true))
}

/// Whether `person` said the line: their face the one seen saying it, or no
/// face seen saying it and theirs alone in view with no other voice heard
/// (the person's call, 6 Nov). That second way is a risk accepted while
/// `LAMP_NAMES_RENAME_YES` is off (docs/eyes.md): a voice off camera could
/// pass for the face alone in view. The yes and the asked note both test it.
fn spoke_it(person: &str, seen: Option<&Seen>, one_face: Option<&str>) -> Result<(), &'static str> {
    match seen {
        Some((speakers, known)) if !speakers.is_empty() => {
            if crate::speaking_faces::seen_saying(speakers, person, known.clone()) {
                Ok(())
            } else {
                Err("another face was seen saying the yes")
            }
        }
        _ if one_face == Some(person) => Ok(()),
        _ => Err("their face was not seen saying the yes, and they were not alone in view"),
    }
}

/// Holds a rename after a `remember_my_face` answer that asks about it
/// (`known_faces::rename_asked`), in place of any; any other answer to a
/// face save drops one held, and one that kept a face ends the asked rename
/// told to the model. Only with the switch on, on a spoken turn.
pub(super) fn after_face_save(
    shared: &Shared,
    request: u64,
    heard: bool,
    call: &net::ToolCall,
    told: &Value,
    scope: Option<&crate::memory_scope::ScopeLease>,
) {
    if !shared.rename_yes || call.name != crate::known_faces::REMEMBER {
        return;
    }
    let Ok(mut renames) = shared.held_rename.lock() else {
        return;
    };
    let asked = heard
        .then(|| crate::known_faces::rename_asked(told))
        .flatten();
    let name = call
        .args
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty());
    match (asked, name, rename_asker(scope)) {
        (Some(from), Some(name), Some(asker)) => {
            eprintln!(
                "lamp-voice second: request {request}: held the face rename for the yes to its question (LAMP_NAMES_RENAME_YES)"
            );
            let now = (shared.clock)();
            renames.asked = Some(RenameAsked {
                to: name.to_owned(),
                from,
                session: asker.0,
                person: asker.1.clone(),
                at: now,
            });
            renames.held = Some(HeldRename {
                call: net::ToolCall {
                    args: serde_json::json!({"name": name, "replace": true}),
                    ..call.clone()
                },
                request,
                asker,
                at: now,
            });
        }
        (Some(_), _, _) => {
            if let Some(was) = renames.held.take() {
                eprintln!(
                    "lamp-voice second: request {request}: dropped the held face rename (held in request {}): asked again with no one known",
                    was.request
                );
            }
            eprintln!(
                "lamp-voice second: request {request}: the face rename is not held: no one known whose yes would run it"
            );
        }
        (None, _, _) => {
            if let Some(was) = renames.held.take() {
                eprintln!(
                    "lamp-voice second: request {request}: dropped the held face rename (held in request {}): a new save call",
                    was.request
                );
            }
            if told["ok"] == true {
                renames.asked = None;
            }
        }
    }
}

/// Whether a rename waits for a yes, or one asked is still told: the line's
/// sight is then read ([`keep_sight`]), for the yes and for whom the note is
/// told ([`asked_note`]).
pub(super) fn waits(shared: &Shared) -> bool {
    shared.rename_yes
        && shared
            .held_rename
            .lock()
            .is_ok_and(|renames| renames.held.is_some() || renames.asked.is_some())
}

/// Keeps who the eyes saw saying `request`'s line for the held rename and
/// the asked note.
pub(super) fn keep_sight(shared: &Shared, sight: SightRead) {
    if let Ok(mut renames) = shared.held_rename.lock()
        && (renames.held.is_some() || renames.asked.is_some())
    {
        renames.sight = Some(sight);
    }
}

/// What became of a held rename on a turn ([`take`]).
pub(super) enum Taken {
    Nothing,
    /// Their yes runs exactly this call.
    Runs(net::ToolCall),
    /// A yes to the new name that did not count, and why: the model is
    /// told so.
    Dropped(String, &'static str),
}

/// The held rename `request` runs, if any. Every later spoken turn takes the
/// hold ([`verdict`]); a turn the lamp wrote leaves it as it is. The journal
/// never has the words or the names.
pub(super) fn take(
    shared: &Shared,
    request: u64,
    heard: bool,
    scope: Option<&crate::memory_scope::ScopeLease>,
    now: Instant,
) -> Taken {
    if !shared.rename_yes {
        return Taken::Nothing;
    }
    let Ok(mut renames) = shared.held_rename.lock() else {
        return Taken::Nothing;
    };
    // The sight stays for the same turn's asked note ([`asked_note`]).
    let sight = renames.sight.clone();
    let Some(held) = renames.held.take() else {
        return Taken::Nothing;
    };
    if !heard || held.request >= request {
        renames.held = Some(held);
        return Taken::Nothing;
    }
    drop(renames);
    let name = held
        .call
        .args
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let said = said_window(shared);
    let seen = sight
        .as_ref()
        .filter(|(seen, _, _)| *seen == request)
        .map(|(_, speakers, known)| (speakers.as_slice(), known.clone()));
    let one_face = scope.and_then(crate::memory_scope::ScopeLease::kept_by_one_face);
    let turn = YesTurn {
        name: &name,
        said: &said,
        asker: &held.asker,
        session: scope.map(|scope| scope.key().session),
        gap: held_by_gap(scope),
        carried: scope
            .is_some_and(|scope| scope.carried_for().is_some() || scope.saves_carried().is_some()),
        too_late: now.saturating_duration_since(held.at) > HELD_RENAME_FOR,
        seen,
        one_face: one_face.as_deref(),
    };
    match verdict(&turn) {
        Ok(()) => {
            eprintln!(
                "lamp-voice second: request {request}: their yes runs the face rename held in request {}",
                held.request
            );
            Taken::Runs(held.call)
        }
        Err((why, told)) => {
            eprintln!(
                "lamp-voice second: request {request}: dropped the held face rename (held in request {}): {why}",
                held.request
            );
            if told {
                Taken::Dropped(name, why)
            } else {
                Taken::Nothing
            }
        }
    }
}

/// Marks the held rename as run for `request` under `name`, the face's new
/// name now: the asked rename is no longer told, and a model call after
/// the yes never renames twice ([`renamed_already`]).
pub(super) fn ran(shared: &Shared, request: u64, name: &str) {
    if let Ok(mut renames) = shared.held_rename.lock() {
        renames.asked = None;
        renames.ran = Some((request, name.to_owned()));
    }
}

/// The answer of a rename a held one already ran for `name` in `request`,
/// for a model call after the yes: never a second.
pub(super) fn renamed_already(
    shared: &Shared,
    request: u64,
    call: &net::ToolCall,
) -> Option<Value> {
    if !shared.rename_yes || call.name != crate::known_faces::REMEMBER {
        return None;
    }
    let name = call.args.get("name").and_then(Value::as_str)?.trim();
    let renames = shared.held_rename.lock().ok()?;
    let (at, renamed) = renames.ran.as_ref()?;
    (*at == request && renamed.eq_ignore_ascii_case(name)).then(|| {
        serde_json::json!({"ok": true, "remembered": renamed, "note": "Already renamed on their yes; nothing more to do."})
    })
}

/// Whose a turn of the asked rename's session is, for [`asked_note`].
#[derive(Debug, PartialEq)]
enum Whose {
    /// The asker's, by the yes's own test ([`spoke_it`]).
    Theirs,
    /// Not proven theirs, nor proven someone else's: no face seen speaking
    /// it and not theirs alone in view (two faces, or another voice heard),
    /// their face seen speaking it beside another, or faces seen speaking
    /// it with the eyes knowing no one in view to tell them by.
    Unproven,
    /// Proven someone else's: faces seen speaking it while the eyes know
    /// who is in view and none is the asker's, or no face seen speaking it
    /// and another known face alone in view.
    SomeoneElse,
}

/// Whose `person`'s asked rename's turn is ([`Whose`]).
fn whose(person: &str, seen: Option<&Seen>, one_face: Option<&str>) -> Whose {
    if spoke_it(person, seen, one_face).is_ok() {
        return Whose::Theirs;
    }
    match seen {
        Some((speakers, known)) if !speakers.is_empty() => {
            let theirs = known
                .as_ref()
                .filter(|(name, _)| crate::known_faces::memory_person(name) == person)
                .map(|(_, face)| face);
            match (known, theirs) {
                (_, Some(face)) if speakers.contains(face) => Whose::Unproven,
                (Some(_), _) => Whose::SomeoneElse,
                (None, _) => Whose::Unproven,
            }
        }
        _ if one_face.is_some() => Whose::SomeoneElse,
        _ => Whose::Unproven,
    }
}

/// What a turn of the asked rename's session is told while it has not run,
/// so the lamp never says the name changed when it did not (lamp-29e7, 6
/// Nov: "That's what I'll use.", the face still under the old name). Only
/// within [`ASKED_TOLD_FOR`]; a turn of another session ends it. Told in
/// full only on the asker's own turn, by the yes's test ([`spoke_it`]); a
/// turn not proven theirs is told no rename has run without the old name,
/// which can't be shown to whoever is speaking (the Reviewer-lead on
/// #5586); one proven someone else's is told nothing ([`whose`]).
pub(super) fn asked_note(
    shared: &Shared,
    request: u64,
    scope: Option<&crate::memory_scope::ScopeLease>,
    now: Instant,
) -> Option<String> {
    if !shared.rename_yes {
        return None;
    }
    let mut renames = shared.held_rename.lock().ok()?;
    let asked = renames.asked.as_ref()?;
    let session = scope.map(|scope| scope.key().session);
    if now.saturating_duration_since(asked.at) > ASKED_TOLD_FOR
        || session.is_some_and(|session| session != asked.session)
    {
        renames.asked = None;
        return None;
    }
    // A turn with no scope is told nothing, and ends nothing.
    session?;
    // The old name only on the asker's turn, by the yes's own test; a turn
    // proven someone else's is told nothing and leaves it for the asker's
    // next.
    let seen = renames
        .sight
        .as_ref()
        .filter(|(seen, _, _)| *seen == request)
        .map(|(_, speakers, known)| (speakers.as_slice(), known.clone()));
    let one_face = scope.and_then(crate::memory_scope::ScopeLease::kept_by_one_face);
    match whose(&asked.person, seen.as_ref(), one_face.as_deref()) {
        Whose::Theirs => Some(not_renamed_note(&asked.to, asked.from.as_deref())),
        Whose::Unproven => Some(unproven_note(&asked.to)),
        Whose::SomeoneElse => None,
    }
}

/// The note [`asked_note`] tells a turn not proven the asker's: no rename
/// has run, never the old name.
fn unproven_note(to: &str) -> String {
    format!(
        "(For you to know, not to answer: no rename has run, and nothing was renamed. Never say the name was changed or that you will call anyone {to}. Only remember_my_face with replace true, answered ok, renames it.)"
    )
}

/// The note [`asked_note`] tells.
fn not_renamed_note(to: &str, from: Option<&str>) -> String {
    let kept = match from {
        Some(from) => format!("the face is still saved as {from}"),
        None => "the face still has the name it had".to_owned(),
    };
    format!(
        "(For you to know, not to answer: you asked whether this face should be known as {to} instead, and no rename has run: {kept}, and nothing was renamed. Never say the name was changed or that you will call them {to}. Only remember_my_face with replace true, answered ok, renames it.)"
    )
}

/// What the model is told when the held rename ran on their yes.
pub(super) fn ran_note(name: &str, result: &Value) -> String {
    format!(
        "(For you to know: they said yes to being known as {name}, so this face is renamed now, with this result: {result}. Say in a few words that you will call them {name}, and do not call remember_my_face again for it.)"
    )
}

/// What the model is told when the held rename ran and renamed nothing.
pub(super) fn not_ran_note(name: &str, why: &str) -> String {
    format!(
        "(For you to know: they said yes to being known as {name}, but the face was not renamed: {why}. The face keeps the name it had: do not say it changed.)"
    )
}

/// What the model is told when their yes to the held rename did not count.
pub(super) fn dropped_note(name: &str, why: &str) -> String {
    format!(
        "(For you to know: their yes to being known as {name} did not count: {why}. Nothing was renamed, and the face keeps the name it had: do not say it changed; you may ask them to look at you and say it again.)"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(this: &str, lamp: &str) -> Said {
        Said {
            this: this.into(),
            before: vec![("Can you call me Marisol instead?".into(), lamp.into())],
        }
    }

    const ASKED: &str = "I know this face as Tove. Do you want me to know you as Marisol instead?";

    /// The asker: the face kept as Tove, by its memory id.
    fn asker() -> (u64, String) {
        (7, crate::known_faces::memory_person("Tove"))
    }

    /// A yes seen said by the asker's own face, in the asker's session.
    fn seen_turn<'a>(
        said: &'a Said,
        asker: &'a (u64, String),
        speakers: &'a [String],
    ) -> YesTurn<'a> {
        YesTurn {
            name: "Marisol",
            said,
            asker,
            session: Some(asker.0),
            gap: false,
            carried: false,
            too_late: false,
            seen: Some((speakers, Some(("Tove".into(), "eyes/f1".into())))),
            one_face: None,
        }
    }

    /// The person's call, 6 Nov: the one speaker seen is the asker, or
    /// with no face seen speaking, they are alone in view and no other
    /// voice was heard; a second face seen speaking the yes drops it.
    #[test]
    fn a_rename_runs_on_the_askers_seen_or_lone_yes_only() {
        let yes = said("Yes, please.", ASKED);
        let asker = asker();
        let theirs = vec!["eyes/f1".to_owned()];
        assert_eq!(verdict(&seen_turn(&yes, &asker, &theirs)), Ok(()));
        // A second face seen speaking it.
        let two = vec!["eyes/f1".to_owned(), "eyes/f2".to_owned()];
        assert_eq!(
            verdict(&seen_turn(&yes, &asker, &two)),
            Err(("another face was seen saying the yes", true))
        );
        // Someone else's face alone seen speaking it.
        let other = vec!["eyes/f2".to_owned()];
        assert!(verdict(&seen_turn(&yes, &asker, &other)).is_err());
        // No face seen speaking it: alone in view, no other voice.
        let alone = YesTurn {
            seen: None,
            one_face: Some(asker.1.as_str()),
            ..seen_turn(&yes, &asker, &theirs)
        };
        assert_eq!(verdict(&alone), Ok(()));
        let empty: Vec<String> = Vec::new();
        let alone_read = YesTurn {
            seen: Some((&empty, None)),
            one_face: Some(asker.1.as_str()),
            ..seen_turn(&yes, &asker, &theirs)
        };
        assert_eq!(verdict(&alone_read), Ok(()));
        // No face seen speaking it and not alone in view (or another voice
        // heard): the one-face rule took no one.
        let unseen = YesTurn {
            seen: None,
            one_face: None,
            ..seen_turn(&yes, &asker, &theirs)
        };
        assert_eq!(verdict(&unseen).map_err(|(_, told)| told), Err(true));
        // Alone in view, but someone else's face.
        let someone_else = YesTurn {
            seen: None,
            one_face: Some("p-other"),
            ..seen_turn(&yes, &asker, &theirs)
        };
        assert!(verdict(&someone_else).is_err());
    }

    /// Carried lines stay refused (the 29e7 requests 19 to 24, the person's
    /// call): a yes on a line carried into a conversation going on, a
    /// gap-held line, another session or too late runs nothing, and the
    /// model is not told it was a yes.
    #[test]
    fn a_carried_gap_held_late_or_other_session_yes_runs_nothing() {
        let yes = said("Yes.", ASKED);
        let asker = asker();
        let theirs = vec!["eyes/f1".to_owned()];
        let base = || seen_turn(&yes, &asker, &theirs);
        for (turn, why) in [
            (
                YesTurn {
                    carried: true,
                    seen: None,
                    one_face: None,
                    ..base()
                },
                "a carried line",
            ),
            (
                YesTurn {
                    gap: true,
                    ..base()
                },
                "a line held only by a face's gap",
            ),
            (
                YesTurn {
                    too_late: true,
                    ..base()
                },
                "too late",
            ),
            (
                YesTurn {
                    session: Some(8),
                    ..base()
                },
                "not the same session",
            ),
            (
                YesTurn {
                    session: None,
                    ..base()
                },
                "not the same session",
            ),
        ] {
            assert_eq!(verdict(&turn), Err((why, false)));
        }
    }

    /// A no, another name or a lamp answer that said no new name drops it,
    /// untold as a yes.
    #[test]
    fn a_no_another_name_or_no_question_drops_the_rename() {
        let asker = asker();
        let theirs = vec!["eyes/f1".to_owned()];
        for turn in [
            said("No, keep Tove.", ASKED),
            said("Yes, but it's Marisa.", ASKED),
            said("Yes, my name is Marisa.", ASKED),
            said("Yes, M, A, R, I, S, A.", ASKED),
            said("Yes?", ASKED),
            said("Yes.", "Sure, what should I do?"),
        ] {
            assert_eq!(
                verdict(&seen_turn(&turn, &asker, &theirs)),
                Err(("not a yes to the new name", false)),
                "{}",
                turn.this
            );
        }
    }

    /// Whose an asked rename's turn is: the asker's by the yes's test; not
    /// proven with two faces in view and none seen speaking, or their face
    /// seen speaking beside another, or faces seen speaking with no one
    /// known in view; someone else's when the eyes know who is in view and
    /// none speaking is the asker, or another known face is alone in view.
    #[test]
    fn whose_an_asked_renames_turn_is() {
        let person = crate::known_faces::memory_person("Tove");
        let tove = Some(("Tove".to_owned(), "eyes/f1".to_owned()));
        let kim = Some(("Kim".to_owned(), "eyes/f2".to_owned()));
        let f1 = vec!["eyes/f1".to_owned()];
        let f2 = vec!["eyes/f2".to_owned()];
        let both = vec!["eyes/f1".to_owned(), "eyes/f2".to_owned()];
        let empty: Vec<String> = Vec::new();
        for (seen, one_face, expected) in [
            (Some((f1.as_slice(), tove.clone())), None, Whose::Theirs),
            (None, Some(person.as_str()), Whose::Theirs),
            (None, None, Whose::Unproven),
            (
                Some((empty.as_slice(), tove.clone())),
                None,
                Whose::Unproven,
            ),
            (Some((both.as_slice(), tove.clone())), None, Whose::Unproven),
            (Some((f2.as_slice(), None)), None, Whose::Unproven),
            (
                Some((f2.as_slice(), tove.clone())),
                None,
                Whose::SomeoneElse,
            ),
            (Some((f2.as_slice(), kim.clone())), None, Whose::SomeoneElse),
            (None, Some("p-other"), Whose::SomeoneElse),
        ] {
            assert_eq!(
                whose(&person, seen.as_ref(), one_face),
                expected,
                "{seen:?} {one_face:?}"
            );
        }
        let note = unproven_note("Marisol");
        assert!(note.contains("no rename has run"), "{note}");
        assert!(note.contains("Never say the name was changed"), "{note}");
        assert!(
            !note.contains("Tove") && !note.contains("saved as"),
            "{note}"
        );
    }

    /// No claim the lamp can't back: the note says the old name is still
    /// saved and that nothing was renamed, and forbids saying it changed.
    #[test]
    fn an_unrun_rename_is_never_claimed() {
        let note = not_renamed_note("Marisol", Some("Tove"));
        assert!(note.contains("still saved as Tove"), "{note}");
        assert!(note.contains("nothing was renamed"), "{note}");
        assert!(note.contains("Never say the name was changed"), "{note}");
        let note = not_renamed_note("Marisol", None);
        assert!(note.contains("still has the name it had"), "{note}");
        for note in [
            not_ran_note("Marisol", "no face"),
            dropped_note("Marisol", "a carried line"),
        ] {
            assert!(note.contains("do not say it changed"), "{note}");
        }
    }
}
