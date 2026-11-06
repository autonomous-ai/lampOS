//! The owner picked by face in the app (#5495, `docs/cloud.md`, "The owner
//! by face"): `owner.get`, `owner.set`, and the faces' ids and owner badge
//! in `face.list`. Off unless `LAMP_OWNER_FACE=on`, with the faces page
//! (`LAMP_FACES_PAGE=on`) and changes after setup (`LAMP_SETTINGS_CHANGE=on`);
//! off, the lamp answers as before.
//!
//! The owner is kept by the face's id (`lamp_msgs::owner_face`), which the
//! eyes mint at random when a face is first saved and never give again, so
//! a name, typed or said, is never enough. lamp-cloud writes and reads
//! nothing itself: the eyes say the faces and their ids over their request
//! socket (`{"list":true,"waiting":true,"ids":true}`, no print), and root's
//! helper (`lamp-setup change`) keeps the pick and reads it back
//! (`{"owner_face": id|null}`, `{"owner_read": true}`), since the file is
//! root's. Fail-closed: once a face was picked, a pick whose face the eyes
//! no longer keep is no owner, never the typed name, and lamp-cloud asks
//! the helper to clear it.
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;

/// The kinds, each listed in `can` only with the switch on.
pub const KINDS: [&str; 2] = ["owner.get", "owner.set"];

/// How long the helper is waited on for a read: it reads two small files.
pub const READ_WAIT: Duration = Duration::from_secs(2);

/// Why `owner.set` is refused for an id that isn't one.
pub const NOT_AN_ID: &str = "face_id must be a saved face's id or null";
/// Why `owner.set` is refused for a face the eyes don't keep now.
pub const NOT_KNOWN: &str = "the lamp doesn't know that face";

/// A face the eyes keep: its name, its id, and when it was first saved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    pub name: String,
    pub face: Option<String>,
    pub saved_unix_s: Option<u64>,
}

/// What the eyes said: the faces they keep, and the names that wait for a
/// yes. `ids` is false for eyes that don't say ids (an older build).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Faces {
    pub people: Vec<Saved>,
    pub waiting: Vec<String>,
    pub ids: bool,
}

impl Faces {
    /// The face kept with this id.
    pub fn with_id(&self, face: &str) -> Option<&Saved> {
        self.people
            .iter()
            .find(|saved| saved.face.as_deref() == Some(face))
    }
}

/// The eyes' answer to the ids ask; `None` for one that isn't (an older
/// build, which knows no such ask).
pub fn faces_of(answer: &Value) -> Option<Faces> {
    let people = answer["people"].as_array()?;
    let mut faces = Faces {
        ids: true,
        ..Faces::default()
    };
    for person in people {
        let name = person["name"].as_str()?.to_owned();
        let face = person["face"]
            .as_str()
            .filter(|face| lamp_msgs::owner_face::is_face_id(face))
            .map(str::to_owned);
        faces.people.push(Saved {
            name,
            saved_unix_s: face.as_ref().and(person["added_unix_s"].as_u64()),
            face,
        });
    }
    faces.waiting = answer["waiting"]
        .as_array()
        .map(|names| {
            names
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Some(faces)
}

/// The faces the eyes keep, with their ids, or why not.
pub fn faces(eyes: &crate::faces::Eyes) -> Result<Faces, String> {
    let ask = json!({"list": true, "waiting": true, "ids": true});
    match crate::photos::exchange(&eyes.socket, &ask, crate::faces::DEADLINE) {
        Some(answer) => {
            faces_of(&answer).ok_or_else(|| "the lamp's eyes could not say whom they know".into())
        }
        None => Err("the lamp's eyes did not answer".into()),
    }
}

/// The pick and the typed name as root's helper reads them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Kept {
    /// The picked face's id.
    pub face: Option<String>,
    /// Whether a face was ever picked: then the typed name decides nothing.
    pub picked: bool,
    pub typed_name: Option<String>,
}

/// The helper's `done` for `{"owner_read": true}`. Fail-closed: a pick it
/// can't read as one is a pick of nobody.
pub fn kept_of(done: &Value) -> Kept {
    let picked = done["picked"].as_bool().unwrap_or(true);
    Kept {
        face: done["face"]
            .as_str()
            .filter(|face| picked && lamp_msgs::owner_face::is_face_id(face))
            .map(str::to_owned),
        picked,
        typed_name: done["typed_name"]
            .as_str()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned),
    }
}

/// The pick, read by root's helper on `socket`, or why not.
pub fn read(socket: &Path) -> Result<Kept, String> {
    match crate::settings::ask(socket, &json!({"owner_read": true}).to_string(), READ_WAIT)? {
        Some(done) => Ok(kept_of(&done)),
        None => Err("the lamp's settings did not answer".into()),
    }
}

/// Two names the same as the lamp matches the typed name today: trimmed,
/// in any case.
fn same_name(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// `owner.get`'s `data` from what the eyes and the helper said, and
/// whether the pick names a face the eyes no longer keep (to be cleared).
pub fn state(faces: &Faces, kept: &Kept) -> (Value, bool) {
    let can_pick = faces.people.iter().any(|saved| saved.face.is_some());
    let owner = kept.face.as_deref().and_then(|face| faces.with_id(face));
    let dangling = kept.face.is_some() && owner.is_none();
    let how = match (kept.picked, owner, &kept.typed_name) {
        (true, Some(_), _) => "face",
        (true, None, _) => "none",
        (false, _, Some(_)) => "typed_name",
        (false, _, None) => "none",
    };
    // Today's match, offered and never applied: exactly one saved face
    // under the typed name.
    let suggested = match (how, &kept.typed_name) {
        ("typed_name", Some(typed)) => {
            let matching: Vec<&Saved> = faces
                .people
                .iter()
                .filter(|saved| saved.face.is_some() && same_name(&saved.name, typed))
                .collect();
            match matching.as_slice() {
                [one] => one.face.clone(),
                _ => None,
            }
        }
        _ => None,
    };
    (
        json!({
            "how": how,
            "owner_face_id": owner.and_then(|saved| saved.face.clone()),
            "owner_face_name": owner.map(|saved| saved.name.clone()),
            "typed_name": kept.typed_name,
            "suggested_face_id": suggested,
            "can_pick": can_pick,
        }),
        dangling,
    )
}

/// Clears a pick whose face is gone, through the helper: the journal's line
/// says why, never which face.
pub fn clear(socket: &Path, why: &str) {
    match crate::settings::ask(
        socket,
        &json!({"owner_face": null}).to_string(),
        crate::settings::WAIT,
    ) {
        Ok(_) => eprintln!("lamp-cloud: the owner pick was cleared: {why}"),
        Err(refused) => {
            eprintln!("lamp-cloud: the owner pick could not be cleared ({why}): {refused}")
        }
    }
}

/// `owner.get`: the owner now, read from the eyes and the helper. A pick
/// whose face is gone is said as none, and cleared.
pub fn get(eyes: &crate::faces::Eyes, socket: &Path) -> Result<Value, String> {
    let faces = faces(eyes)?;
    let kept = read(socket)?;
    let (state, dangling) = state(&faces, &kept);
    if dangling {
        clear(socket, "the face was removed");
    }
    Ok(state)
}

/// The app's `face_id`: a face id, or `None` for `null`; or why not.
pub fn asked(data: &Value) -> Result<Option<String>, String> {
    match data.get("face_id") {
        Some(Value::Null) => Ok(None),
        Some(Value::String(face)) if lamp_msgs::owner_face::is_face_id(face) => {
            Ok(Some(face.clone()))
        }
        _ => Err(NOT_AN_ID.into()),
    }
}

/// The app's `request_id` for `owner.set`, said back with its `starting`
/// and its result: one that doesn't fit is left out, never a refusal.
pub fn request_id(data: &Value) -> Option<String> {
    data.get("request_id")
        .and_then(Value::as_str)
        .filter(|id| crate::settings::fit_request_id(id))
        .map(str::to_owned)
}

/// `owner.set`'s work, after its `starting`: the id's shape, then the face
/// kept by the eyes now, then root's helper writes the pick; changing
/// nothing on any failure. Its success says the owner as `owner.get` does.
pub fn set(eyes: &crate::faces::Eyes, socket: &Path, data: &Value) -> Result<Value, String> {
    let face = asked(data)?;
    let faces = faces(eyes)?;
    let line = match &face {
        Some(face) => {
            if faces.with_id(face).is_none() {
                return Err(NOT_KNOWN.into());
            }
            json!({"owner_face": face})
        }
        None => json!({"owner_face": null}),
    };
    crate::settings::ask(socket, &line.to_string(), crate::settings::WAIT)?;
    let kept = read(socket)?;
    let (state, _) = state(&faces, &kept);
    // The app's `request_id` is said back beside these, as for every
    // change (`settings::Outcome::reply`).
    Ok(json!({
        "how": state["how"],
        "owner_face_id": state["owner_face_id"],
        "owner_face_name": state["owner_face_name"],
    }))
}

/// `face.list`'s faces with their ids, when they were first saved and the
/// owner's badge: each confirmed face as the eyes keep it, each waiting one
/// with no id, which can't be picked.
pub fn listed(faces: &Faces, kept: Option<&Kept>) -> Value {
    let owner = kept
        .and_then(|kept| kept.face.as_deref())
        .filter(|face| faces.with_id(face).is_some());
    let mut listed: Vec<Value> = faces
        .people
        .iter()
        .map(|saved| {
            json!({
                "name": saved.name,
                "waiting": false,
                "face_id": saved.face,
                "saved_at": saved.saved_unix_s.map(|at| {
                    crate::online::rfc3339(std::time::UNIX_EPOCH + Duration::from_secs(at))
                }),
                "owner": owner.is_some() && saved.face.as_deref() == owner,
            })
        })
        .collect();
    listed.extend(faces.waiting.iter().map(|name| {
        json!({"name": name, "waiting": true, "face_id": null, "saved_at": null, "owner": false})
    }));
    json!({ "faces": listed })
}

/// `face.list` with the owner's switch on: the faces with their ids and the
/// owner's badge. Eyes that say no ids are listed as before, with no id,
/// so the app hides "pick owner"; a pick the helper can't read leaves no
/// badge.
pub fn list(eyes: &crate::faces::Eyes, socket: &Path) -> Result<Value, String> {
    match faces(eyes) {
        Ok(faces) => {
            let kept = read(socket).ok();
            Ok(listed(&faces, kept.as_ref()))
        }
        Err(_) => crate::faces::list(eyes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn faces() -> Faces {
        faces_of(&json!({
            "people": [
                {"name": "B", "face": "f-3c9a51d07be24e18", "added_unix_s": 1_762_071_243},
                {"name": "Lan", "face": "f-91e0c2aa4f7d3b65", "added_unix_s": 1_762_371_611},
            ],
            "waiting": ["C"],
        }))
        .unwrap()
    }

    #[test]
    fn the_eyes_ids_are_read_and_an_older_eyes_answer_is_none() {
        let faces = faces();
        assert_eq!(faces.people.len(), 2);
        assert_eq!(faces.with_id("f-91e0c2aa4f7d3b65").unwrap().name, "Lan");
        assert_eq!(faces.waiting, ["C"]);
        assert_eq!(faces_of(&json!({"error": "unknown"})), None);
        // A name with no id (the switch off at the eyes) can't be picked.
        let none = faces_of(&json!({"people": [{"name": "A", "face": null}]})).unwrap();
        assert_eq!(none.people[0].face, None);
        assert_eq!(none.people[0].saved_unix_s, None);
    }

    /// The app's name A and the face kept as B: picked by id, B is the
    /// owner whatever the typed name says (#5495's first done-when).
    #[test]
    fn a_picked_face_is_the_owner_whatever_the_names() {
        let kept = Kept {
            face: Some("f-3c9a51d07be24e18".into()),
            picked: true,
            typed_name: Some("A".into()),
        };
        let (told, dangling) = state(&faces(), &kept);
        assert!(!dangling);
        assert_eq!(
            told,
            json!({
                "how": "face",
                "owner_face_id": "f-3c9a51d07be24e18",
                "owner_face_name": "B",
                "typed_name": "A",
                "suggested_face_id": null,
                "can_pick": true,
            })
        );
        let listed = listed(&faces(), Some(&kept));
        assert_eq!(
            listed["faces"][0],
            json!({"name": "B", "waiting": false, "face_id": "f-3c9a51d07be24e18", "saved_at": "2025-11-02T08:14:03Z", "owner": true})
        );
        assert_eq!(listed["faces"][1]["owner"], false);
        assert_eq!(
            listed["faces"][2],
            json!({"name": "C", "waiting": true, "face_id": null, "saved_at": null, "owner": false})
        );
    }

    /// Once picked, a face gone is no owner: never the typed name, even
    /// with a face saved under it (#5495's second done-when).
    #[test]
    fn a_pick_whose_face_is_gone_is_none_never_the_typed_name() {
        let kept = Kept {
            face: Some("f-0000000000000001".into()),
            picked: true,
            typed_name: Some("Lan".into()),
        };
        let (told, dangling) = state(&faces(), &kept);
        assert!(dangling);
        assert_eq!(told["how"], "none");
        assert_eq!(told["owner_face_id"], Value::Null);
        assert_eq!(told["suggested_face_id"], Value::Null);
        let cleared = Kept { face: None, ..kept };
        let (told, dangling) = state(&faces(), &cleared);
        assert!(!dangling);
        assert_eq!(told["how"], "none");
        assert!(
            listed(&faces(), Some(&cleared))["faces"]
                .as_array()
                .unwrap()
                .iter()
                .all(|face| face["owner"] == false)
        );
    }

    /// No pick ever made: today's typed name, with its one text match
    /// suggested, never applied (#5495's third done-when).
    #[test]
    fn with_no_pick_the_typed_name_stays_and_its_match_is_only_suggested() {
        let kept = Kept {
            face: None,
            picked: false,
            typed_name: Some(" lan ".trim().into()),
        };
        let (told, _) = state(&faces(), &kept);
        assert_eq!(told["how"], "typed_name");
        assert_eq!(told["owner_face_id"], Value::Null);
        assert_eq!(told["suggested_face_id"], "f-91e0c2aa4f7d3b65");
        let nobody = Kept::default();
        assert_eq!(state_of(&nobody)["how"], "none");
        // No face with an id: nothing to pick.
        let bare = Faces {
            people: vec![Saved {
                name: "Lan".into(),
                face: None,
                saved_unix_s: None,
            }],
            ..Faces::default()
        };
        assert_eq!(super::state(&bare, &kept).0["can_pick"], false);
        assert_eq!(
            super::state(&bare, &kept).0["suggested_face_id"],
            Value::Null
        );
    }

    fn state_of(kept: &Kept) -> Value {
        state(&faces(), kept).0
    }

    #[test]
    fn the_apps_face_id_is_an_id_or_null() {
        assert_eq!(
            asked(&json!({"face_id": "f-3c9a51d07be24e18"})),
            Ok(Some("f-3c9a51d07be24e18".into()))
        );
        assert_eq!(asked(&json!({"face_id": null})), Ok(None));
        for wrong in [
            json!({}),
            json!({"face_id": "B"}),
            json!({"face_id": "p-094a367b026246fb"}),
            json!({"face_id": 7}),
        ] {
            assert_eq!(asked(&wrong), Err(NOT_AN_ID.into()), "{wrong}");
        }
    }

    /// The helper's read, fail-closed: a picked file it can't say is no
    /// owner.
    #[test]
    fn the_helpers_read_is_fail_closed() {
        assert_eq!(
            kept_of(&json!({"face": "f-3c9a51d07be24e18", "picked": true, "typed_name": "A"})),
            Kept {
                face: Some("f-3c9a51d07be24e18".into()),
                picked: true,
                typed_name: Some("A".into())
            }
        );
        assert!(kept_of(&json!({})).picked);
        assert_eq!(kept_of(&json!({"face": "Lan", "picked": true})).face, None);
        assert_eq!(
            kept_of(&json!({"face": null, "picked": false, "typed_name": ""})),
            Kept::default()
        );
    }
}
