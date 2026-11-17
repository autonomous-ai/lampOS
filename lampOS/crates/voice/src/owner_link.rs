//! The owner's face linked to the name typed at setup, by their own words
//! (#7501, `docs/memory.md`, "The owner's face and typed name").
//!
//! On a lamp set up before onboarding kept an owner (`onboarding::Progress::owner`,
//! 29 Oct), or whose owner's face was saved outside "who you are", the
//! owners are the typed name alone (`person::by_default_owner`). When the
//! owner's face is kept under another name, its memory id is not one of
//! `person::owner_ids`, so the lamp holds their name back, reads none of
//! their services outside the private room, and their own "forget
//! everything" leaves the typed name's rows (`remember::owner_forgets_rows_of`).
//!
//! The link is the owner's own: their spoken "I'm <the typed name>", which
//! the face paths already send as `remember_my_face` with that name
//! (`known_faces::gives_another_name`), is taken as the link instead of a
//! rename, and the onboarding file keeps their face's name as the owner
//! (`onboarding::owner_linked`), set once while no owner is kept and never
//! over one. Held to the guards a services yes is held to and more
//! ([`judge`]): heard at the lamp, the typed name in their own words, the
//! known face seen saying the line (never by voice alone, and the private
//! room grants nothing here), the line's own scope that face's (never the
//! room's, another's or one carried), the one face in view, and in the
//! owner's enrolled voice when they have one. Nothing is ever linked
//! silently: the eyes keep no record of which face "who you are" saved, and
//! the only face they know may be a guest's.
//!
//! `LAMP_OWNER_LINK=on` only; off, the call goes on to the face paths as
//! before. The start-up line ([`startup_line`]) is said whatever the
//! switch, so the lamps in this state can be counted.
use lamp_gemini::ToolCall;
use serde_json::{Value, json};
use std::path::Path;

/// The switch: `on` for the link to be taken. Anything else: never.
pub const ENVIRONMENT: &str = "LAMP_OWNER_LINK";

/// What the model is told once the link is posted.
pub const LINKED: &str = "Done: their face is now known as the person who set the lamp up, by both names. Say so in a few words.";

#[cfg(test)]
thread_local! {
    static ON_FOR_TEST: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Tests: [`on`] answers `on` on this thread; `None` reads the environment.
#[cfg(test)]
pub(crate) fn on_for_test(on: Option<bool>) {
    ON_FOR_TEST.with(|cell| cell.set(on));
}

/// Whether [`ENVIRONMENT`] is exactly `on`.
pub fn on() -> bool {
    #[cfg(test)]
    if let Some(on) = ON_FOR_TEST.with(std::cell::Cell::get) {
        return on;
    }
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// Whether two names are the same, as the eyes match one (any case, the
/// ends trimmed).
fn same_name(one: &str, other: &str) -> bool {
    one.trim().to_lowercase() == other.trim().to_lowercase()
}

/// What a `remember_my_face` with the typed name is judged on ([`judge`]).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Claim<'a> {
    /// The name the call gives.
    pub name: &'a str,
    /// Their words in the turn in hand.
    pub said: &'a str,
    /// The name typed at setup (`person::now`).
    pub typed: Option<&'a str>,
    /// The owner the onboarding file keeps (`onboarding::owner_of`).
    pub kept: Option<&'a str>,
    /// The known face the eyes saw saying the line, alone in it and not by
    /// voice only (`services::seen_saying_owner`).
    pub seen: Option<&'a str>,
    /// The person (a memory id) whose scope the line is, now: never the
    /// room's, a held one or one by the one-face rule.
    pub scope: Option<&'a str>,
    /// The line carried for someone as their conversation going on
    /// (`ScopeLease::carried_for`), no face seen saying it.
    pub carried: bool,
    /// The faces in view lately (`known_faces::most_faces_lately`).
    pub faces: Option<usize>,
    /// The line in the owner's enrolled voice (`services::owners_voice_now`);
    /// `None` when they have no print.
    pub owners_voice: Option<bool>,
}

/// What [`judge`] makes of a [`Claim`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Not a claim to the typed name by another known face: the face paths
    /// take the call as before.
    NoClaim,
    /// The owner's own claim: link the face of this name.
    Link(String),
    /// A claim, refused for this reason; the face paths take the call.
    Refused(&'static str),
}

/// The link's one rule: a `remember_my_face` with the typed name, from a
/// known face kept under another name, is the owner's link only while no
/// owner is kept, the typed name is in their words, that face was seen
/// saying the line, the line's scope is theirs and not carried, they are
/// the one face in view, and the line is in the owner's voice when the
/// owner has an enrolled print.
pub(crate) fn judge(claim: &Claim) -> Verdict {
    let Some(typed) = claim.typed else {
        return Verdict::NoClaim;
    };
    if !same_name(claim.name, typed) {
        return Verdict::NoClaim;
    }
    let typed_id = crate::known_faces::memory_person(typed);
    // A face kept under the typed name is the owner already.
    if claim
        .seen
        .is_some_and(|face| crate::known_faces::memory_person(face) == typed_id)
    {
        return Verdict::NoClaim;
    }
    if claim.kept.is_some() {
        return Verdict::Refused("an owner is kept already, never overwritten");
    }
    if !crate::known_faces::said_by_them(typed, claim.said) {
        return Verdict::Refused("the typed name is not in their words");
    }
    let Some(face) = claim.seen else {
        return Verdict::Refused("no known face seen saying it");
    };
    if claim.carried {
        return Verdict::Refused("the line is carried, not seen");
    }
    let face_id = crate::known_faces::memory_person(face);
    if claim.scope != Some(face_id.as_str()) {
        return Verdict::Refused("the line is not that face's own (the room's or another's)");
    }
    if claim.faces != Some(1) {
        return Verdict::Refused("not the one face in view");
    }
    if claim.owners_voice == Some(false) {
        return Verdict::Refused("not in the owner's enrolled voice");
    }
    Verdict::Link(face.to_owned())
}

/// A `remember_my_face` call for the heard line `request` (`heard`, its
/// memory scope `lease`), with the onboarding file at `file`: the owner's
/// link posted and what the model is told, when [`judge`] links; `None`
/// for the face paths to take it as before (the switch off, no claim, or
/// refused, the reason said in the journal without a name).
pub(crate) fn taken(
    call: &ToolCall,
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
    file: Option<&Path>,
) -> Option<(Value, bool)> {
    if !on()
        || !heard
        || call.name != crate::known_faces::REMEMBER
        || call.args.get("replace").and_then(Value::as_bool) == Some(true)
    {
        return None;
    }
    let file = file?;
    let name = call.args.get("name").and_then(Value::as_str)?;
    let typed = crate::person::now()?;
    if !same_name(name, &typed) {
        return None;
    }
    let kept = crate::onboarding::owner_of(file);
    let seen = crate::speaking_faces::line_seen(request)
        .and_then(|line| line.known.map(|(name, _)| name))
        .filter(|face| {
            crate::services::seen_saying_owner(request, &crate::known_faces::memory_person(face)).0
        });
    let said = crate::thread::THREAD
        .lock()
        .map(|thread| thread.names_window(0).this)
        .unwrap_or_default();
    let scope = lease
        .filter(|lease| !lease.forgets_need_mouth())
        .and_then(|lease| lease.scope())
        .filter(|scope| !scope.held())
        .and_then(|scope| scope.person())
        .map(str::to_owned);
    let carried = lease.and_then(|lease| lease.carried_for()).is_some();
    let typed_id = crate::known_faces::memory_person(&typed);
    let owners_voice = crate::services::owner_has_print(&typed_id)
        .then(|| crate::services::owners_voice_now(request));
    let verdict = judge(&Claim {
        name,
        said: &said,
        typed: Some(&typed),
        kept: kept.as_deref(),
        seen: seen.as_deref(),
        scope: scope.as_deref(),
        carried,
        faces: crate::known_faces::most_faces_lately(),
        owners_voice,
    });
    match verdict {
        Verdict::NoClaim => None,
        Verdict::Refused(why) => {
            eprintln!(
                "lamp-voice: request {request}: the owner's face not linked to the typed name: {why} ({ENVIRONMENT})"
            );
            None
        }
        Verdict::Link(face) => {
            crate::onboarding::owner_linked(file, &face);
            eprintln!(
                "lamp-voice: request {request}: the owner's face linked to the typed name by their own words, kept as the owner ({ENVIRONMENT})"
            );
            Some((json!({"ok": true, "note": LINKED}), true))
        }
    }
}

/// The start-up line that says a lamp is in the state [`taken`] mends: a
/// typed name, no owner kept, and the eyes knowing faces none of which is
/// kept under the typed name. Said once a run, names never in it, so
/// Backend can count the lamps; `None` otherwise.
pub fn startup_line(typed: Option<&str>, kept: Option<&str>, names: &[String]) -> Option<String> {
    let typed = typed?;
    if kept.is_some() || names.is_empty() || names.iter().any(|name| same_name(name, typed)) {
        return None;
    }
    Some(format!(
        "lamp-voice: owner: typed name only, no face linked ({} known faces; {ENVIRONMENT} {})",
        names.len(),
        if on() { "on" } else { "off" }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TYPED: &str = "Typed Name";
    const FACE: &str = "Face Name";

    /// The owner alone, seen saying "I'm Typed Name", their own line.
    fn owners_claim<'a>(face_id: &'a str) -> Claim<'a> {
        Claim {
            name: TYPED,
            said: "I'm Typed Name.",
            typed: Some(TYPED),
            kept: None,
            seen: Some(FACE),
            scope: Some(face_id),
            carried: false,
            faces: Some(1),
            owners_voice: None,
        }
    }

    /// #7501: the owner alone and seen saying the typed name links their
    /// face, and with an enrolled print, only in the owner's voice.
    #[test]
    fn the_owner_alone_seen_saying_the_typed_name_links_their_face() {
        let face_id = crate::known_faces::memory_person(FACE);
        let claim = owners_claim(&face_id);
        assert_eq!(judge(&claim), Verdict::Link(FACE.to_owned()));
        assert_eq!(
            judge(&Claim {
                name: "typed name ",
                ..claim
            }),
            Verdict::Link(FACE.to_owned())
        );
        assert_eq!(
            judge(&Claim {
                owners_voice: Some(true),
                ..claim
            }),
            Verdict::Link(FACE.to_owned())
        );
    }

    /// #7501's guards: another face's line, a second face in view, the
    /// room's line, unseen, carried, the typed name not said, not the
    /// owner's enrolled voice, and an owner kept already are each refused.
    #[test]
    fn the_link_is_refused_for_anything_but_the_owner_alone_and_seen() {
        let face_id = crate::known_faces::memory_person(FACE);
        let other_id = crate::known_faces::memory_person("Another Face");
        let claim = owners_claim(&face_id);
        let refused = |claim: Claim| matches!(judge(&claim), Verdict::Refused(_));
        // Another face's scope, or another voice than the owner's print.
        assert!(refused(Claim {
            scope: Some(&other_id),
            ..claim
        }));
        assert!(refused(Claim {
            owners_voice: Some(false),
            ..claim
        }));
        // A second face in view, or no count at all.
        assert!(refused(Claim {
            faces: Some(2),
            ..claim
        }));
        assert!(refused(Claim {
            faces: None,
            ..claim
        }));
        // The room's line (the private room grants nothing here).
        assert!(refused(Claim {
            scope: None,
            ..claim
        }));
        // Unseen: no known face seen saying it.
        assert!(refused(Claim {
            seen: None,
            ..claim
        }));
        // Carried.
        assert!(refused(Claim {
            carried: true,
            ..claim
        }));
        // The typed name not in their words.
        assert!(refused(Claim {
            said: "Yes.",
            ..claim
        }));
        // Set once, never overwritten.
        assert!(refused(Claim {
            kept: Some("Someone"),
            ..claim
        }));
    }

    /// Not a claim: another name than the typed one, no typed name, or a
    /// face kept under the typed name already.
    #[test]
    fn another_name_or_the_typed_face_is_no_claim() {
        let face_id = crate::known_faces::memory_person(FACE);
        let claim = owners_claim(&face_id);
        assert_eq!(
            judge(&Claim {
                name: "Robin",
                ..claim
            }),
            Verdict::NoClaim
        );
        assert_eq!(
            judge(&Claim {
                typed: None,
                ..claim
            }),
            Verdict::NoClaim
        );
        assert_eq!(
            judge(&Claim {
                seen: Some("typed name"),
                ..claim
            }),
            Verdict::NoClaim
        );
    }

    /// Off, or a call that isn't a heard `remember_my_face` with a name, is
    /// never taken: the face paths run it as before.
    #[test]
    fn off_or_not_a_heard_save_is_never_taken() {
        let call = |name: &str, args: Value| ToolCall {
            id: "1".into(),
            name: name.into(),
            args,
        };
        let file = Path::new("/nonexistent/onboarding.json");
        let save = call(crate::known_faces::REMEMBER, json!({"name": TYPED}));
        on_for_test(Some(false));
        assert_eq!(taken(&save, 7_501_001, true, None, Some(file)), None);
        on_for_test(Some(true));
        assert_eq!(taken(&save, 7_501_002, false, None, Some(file)), None);
        assert_eq!(taken(&save, 7_501_003, true, None, None), None);
        let replace = call(
            crate::known_faces::REMEMBER,
            json!({"name": TYPED, "replace": true}),
        );
        assert_eq!(taken(&replace, 7_501_004, true, None, Some(file)), None);
        let forget = call(crate::known_faces::FORGET, json!({"name": TYPED}));
        assert_eq!(taken(&forget, 7_501_005, true, None, Some(file)), None);
        on_for_test(None);
    }

    /// The start-up line: said when a typed name has no owner kept and none
    /// of the eyes' faces is under it, whatever the switch; never a name.
    #[test]
    fn the_startup_line_counts_a_lamp_with_the_typed_name_only() {
        let names = vec![FACE.to_owned(), "Guest".to_owned()];
        on_for_test(Some(false));
        let line = startup_line(Some(TYPED), None, &names).unwrap();
        assert_eq!(
            line,
            "lamp-voice: owner: typed name only, no face linked (2 known faces; LAMP_OWNER_LINK off)"
        );
        assert!(!line.contains(TYPED) && !line.contains(FACE));
        on_for_test(Some(true));
        assert!(
            startup_line(Some(TYPED), None, &names)
                .unwrap()
                .ends_with("LAMP_OWNER_LINK on)")
        );
        on_for_test(None);
        // An owner kept, the typed name among the faces, no faces, or no
        // typed name: not said.
        assert_eq!(startup_line(Some(TYPED), Some(FACE), &names), None);
        let typed_face = vec!["typed name".to_owned()];
        assert_eq!(startup_line(Some(TYPED), None, &typed_face), None);
        assert_eq!(startup_line(Some(TYPED), None, &[]), None);
        assert_eq!(startup_line(None, None, &names), None);
    }
}
