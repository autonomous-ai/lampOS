//! The owner's two yeses from the app (#5500, `docs/cloud.md`, "The two
//! yeses from the app"): face expressions, and room sounds on a lamp with no
//! privacy slide. Off unless `LAMP_APP_CONSENT=on`, with `LAMP_CLOUD=on`;
//! without it both kinds are answered as any this lamp lacks.
//!
//! `consent.get` reads the files where they are kept
//! ([`lamp_msgs::consent::FOLDER`], which `lampos` may read), with each
//! key's revision (`rev`, [`lamp_msgs::consent::rev`]). `consent.set` is a
//! compare-and-set: exactly one key and `if`, the key's revision the app
//! saw last, handed as one line to root's helper, `lamp-setup change`, as
//! the names go, since `lamp-cloud` can write nothing there. The helper
//! writes or removes the file only while that is still the key's revision,
//! and answers at once; the reply is what the files then say, as
//! `consent.get` answers, with `"stale": true` when nothing changed because
//! the lamp changed the key since (a retry of an old tap, or a "no" at the
//! lamp after the app looked). With no fit `if` the set is refused before
//! the helper.
//!
//! Room sounds is said, and taken, only where it does something: a lamp
//! whose privacy route is `no-slide` (lamp-ears reads its yes only there).
//! `lamp-cloud` knows that from its own `LAMP_PRIVACY_CONTROLS`, set beside
//! Controls', Voice's and Ears' on such a lamp; anything but `no-slide`
//! exactly (the product's slide lamps leave it unset) is a slide lamp.
use lamp_msgs::consent::{FACE_EXPRESSIONS, ROOM_SOUNDS};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use lamp_msgs::consent::{ENVIRONMENT, KINDS};

/// The privacy route's switch, read here only to know whether room sounds
/// applies.
pub const PRIVACY_ENVIRONMENT: &str = "LAMP_PRIVACY_CONTROLS";

/// The privacy route on a lamp with no slide: room sounds applies.
pub const NO_SLIDE: &str = "no-slide";

/// How long the helper is waited on: it writes or removes one small file
/// and answers before any restart, and the broker's loop waits on it
/// (`crate::lamp::RESTART_WAIT`).
pub const WAIT: Duration = Duration::from_secs(2);

/// Where the yeses are read and who writes them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    /// The folder the files are read in.
    pub folder: PathBuf,
    /// Root's change helper's socket ([`crate::settings::SOCKET`]).
    pub socket: PathBuf,
    pub wait: Duration,
    /// Room sounds applies: the lamp has no slide.
    pub room_sounds: bool,
}

/// Whether both kinds are taken: `LAMP_CLOUD=on` and `LAMP_APP_CONSENT=on`.
pub fn gates(cloud: Option<&str>, consent: Option<&str>) -> bool {
    cloud == Some("on") && consent == Some("on")
}

impl Place {
    /// The product's, as the environment says: none when off.
    pub fn from_environment() -> Option<Self> {
        let var = |name: &str| std::env::var(name).ok();
        gates(var("LAMP_CLOUD").as_deref(), var(ENVIRONMENT).as_deref()).then(|| Self {
            folder: lamp_msgs::consent::FOLDER.into(),
            socket: crate::settings::SOCKET.into(),
            wait: WAIT,
            room_sounds: var(PRIVACY_ENVIRONMENT).as_deref() == Some(NO_SLIDE),
        })
    }

    /// The keys this lamp has: face expressions, and room sounds only with
    /// no slide.
    pub fn keys(&self) -> Vec<&'static str> {
        let mut keys = vec![FACE_EXPRESSIONS];
        if self.room_sounds {
            keys.push(ROOM_SOUNDS);
        }
        keys
    }
}

/// What the lamp keeps now, as `consent.get` answers it: each key this lamp
/// has, `true` only while its file holds a yes, and `rev`, each key's
/// revision ([`lamp_msgs::consent::rev`]; `unknown` while the state can't
/// be read, which the helper never matches).
pub fn now(place: &Place) -> Value {
    let mut said = Map::new();
    let mut rev = Map::new();
    for (key, yes, kept) in lamp_msgs::consent::snapshot(&place.folder, &place.keys()) {
        said.insert(key.clone(), yes.into());
        rev.insert(key, kept.into());
    }
    said.insert("rev".into(), Value::Object(rev));
    Value::Object(said)
}

/// The helper's line for `consent.set`'s `data`, or why not: exactly one
/// key this lamp has, `true` or `false`, and `if`, the revision the app
/// saw ([`lamp_msgs::consent::fit_rev`]), sent on as
/// `{"consent": {KEY: bool}, "if": REV}`. No error holds what was sent.
pub fn line(place: &Place, data: &Value) -> Result<String, String> {
    let why = || {
        format!(
            "consent.set takes exactly one of {}, true or false",
            place.keys().join(", ")
        )
    };
    let object = data.as_object().ok_or_else(why)?;
    let rev = lamp_msgs::consent::fit_rev(object.get("if"))?;
    let mut keys = object.iter().filter(|(key, _)| *key != "if");
    let (key, given) = keys.next().ok_or_else(why)?;
    if keys.next().is_some() {
        return Err(why());
    }
    if key == ROOM_SOUNDS && !place.room_sounds {
        return Err("room sounds is not on this lamp: it has a privacy slide".into());
    }
    if !place.keys().contains(&key.as_str()) {
        return Err(why());
    }
    let yes = given.as_bool().ok_or_else(why)?;
    Ok(json!({ "consent": { key.as_str(): yes }, "if": rev }).to_string())
}

/// `consent.set`: checked here, made by the helper, and answered with what
/// the files say after it, the key's revision as the helper said it, and
/// `"stale": true` when the helper changed nothing because the key's
/// revision was no longer the one the app saw.
pub fn set(place: &Place, data: &Value) -> Result<Value, String> {
    let line = line(place, data)?;
    let reply = crate::settings::ask_line(&place.socket, &line, place.wait)?;
    let reply: Value = serde_json::from_str(reply.trim())
        .map_err(|_| "the lamp's settings did not answer".to_owned())?;
    let stale = reply.get("stale").is_some();
    if !stale && reply.get("done").is_none() {
        return Err(reply
            .get("refused")
            .and_then(Value::as_str)
            .unwrap_or("the lamp's settings did not answer")
            .to_owned());
    }
    // The key set: its yes and revision as the helper said them, together,
    // read under its hold; the others as they are now.
    let mut said = now(place);
    let given = reply.get(if stale { "stale" } else { "done" });
    if let (Some(Value::Object(given)), Some(Value::Object(revs))) = (given, reply.get("rev")) {
        for (key, yes) in given {
            if let (Some(yes), Some(rev)) = (yes.as_bool(), revs.get(key).and_then(Value::as_str))
                && said.get(key.as_str()).is_some()
            {
                said[key.as_str()] = yes.into();
                said["rev"][key.as_str()] = rev.into();
            }
        }
    }
    if stale {
        said["stale"] = true.into();
    }
    Ok(said)
}

/// `consent.get`: [`now`], writing nothing.
pub fn get(place: &Place) -> Value {
    now(place)
}

/// A place in `folder`, the helper at `socket`, for tests and rigs.
pub fn place_in(folder: &Path, socket: &Path, room_sounds: bool) -> Place {
    Place {
        folder: folder.into(),
        socket: socket.into(),
        wait: WAIT,
        room_sounds,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(room_sounds: bool) -> Place {
        place_in(
            Path::new("/nowhere"),
            Path::new("/nowhere.sock"),
            room_sounds,
        )
    }

    #[test]
    fn the_switch_is_on_exactly_and_needs_the_cloud() {
        assert!(gates(Some("on"), Some("on")));
        for (cloud, consent) in [
            (Some("on"), None),
            (Some("on"), Some("yes")),
            (Some("on"), Some("ON")),
            (None, Some("on")),
            (Some("off"), Some("on")),
        ] {
            assert!(!gates(cloud, consent), "{cloud:?} {consent:?}");
        }
    }

    #[test]
    fn a_set_takes_one_key_this_lamp_has_as_true_or_false() {
        let slide = place(false);
        assert_eq!(
            line(&slide, &json!({"face_expressions": true, "if": "t1"})).unwrap(),
            r#"{"consent":{"face_expressions":true},"if":"t1"}"#
        );
        assert_eq!(
            line(&slide, &json!({"face_expressions": false, "if": "none"})).unwrap(),
            r#"{"consent":{"face_expressions":false},"if":"none"}"#
        );
        assert!(
            line(&slide, &json!({"room_sounds": true, "if": "t3"}))
                .unwrap_err()
                .contains("privacy slide")
        );
        let no_slide = place(true);
        assert_eq!(
            line(&no_slide, &json!({"room_sounds": true, "if": "t4"})).unwrap(),
            r#"{"consent":{"room_sounds":true},"if":"t4"}"#
        );
        for data in [
            json!({"if": "t5"}),
            json!({"camera": true, "if": "t5"}),
            json!({"face_expressions": "yes", "if": "t5"}),
            json!({"face_expressions": 1, "if": "t5"}),
            json!({"face_expressions": null, "if": "t5"}),
            json!({"face_expressions": true, "room_sounds": true, "if": "t5"}),
        ] {
            let why = line(&no_slide, &data).unwrap_err();
            assert_eq!(
                why,
                "consent.set takes exactly one of face_expressions, room_sounds, true or false",
                "{data}"
            );
        }
        for data in [json!(null), json!([true]), json!("ask")] {
            assert!(line(&no_slide, &data).is_err(), "{data}");
        }
    }

    /// #5500: each set names `if`, the key's revision the app saw, so a
    /// retry of an old yes after a later "no" at the lamp changes nothing.
    /// Without one, or with one that is not 1 to 64 printable ASCII
    /// characters, the set is refused before the helper is asked.
    #[test]
    fn a_set_without_a_fit_if_is_refused_before_the_helper() {
        let no_slide = place(true);
        for data in [
            json!({"face_expressions": true}),
            json!({"room_sounds": false}),
            json!({"face_expressions": true, "if": ""}),
            json!({"face_expressions": true, "if": "a".repeat(65)}),
            json!({"face_expressions": true, "if": "a b"}),
            json!({"face_expressions": true, "if": "tap\u{e9}"}),
            json!({"face_expressions": true, "if": 7}),
            json!({"face_expressions": true, "if": null}),
        ] {
            assert_eq!(
                line(&no_slide, &data),
                Err(lamp_msgs::consent::REV_REFUSED.into()),
                "{data}"
            );
        }
        // Through `set`, with a helper listening: never asked.
        let folder = PathBuf::from(format!("/tmp/lcc-if-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let socket = folder.join("change.sock");
        let helper = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        helper.set_nonblocking(true).unwrap();
        let place = place_in(&folder, &socket, true);
        for data in [
            json!({"face_expressions": true}),
            json!({"room_sounds": true, "if": ""}),
            json!({"face_expressions": true, "if": "a".repeat(65)}),
            json!({"face_expressions": true, "if": "a b"}),
            json!({"face_expressions": true, "if": "tap\u{e9}"}),
        ] {
            assert_eq!(
                set(&place, &data),
                Err(lamp_msgs::consent::REV_REFUSED.into()),
                "{data}"
            );
        }
        assert_eq!(
            helper.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "the helper was asked"
        );
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_get_says_each_key_this_lamp_has_from_its_file_with_its_rev() {
        let folder = std::env::temp_dir().join(format!("cloud-consent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let slide = place_in(&folder, Path::new("/nowhere.sock"), false);
        let no_slide = place_in(&folder, Path::new("/nowhere.sock"), true);
        assert_eq!(
            get(&slide),
            json!({"face_expressions": false, "rev": {"face_expressions": "unknown"}})
        );
        assert_eq!(
            get(&no_slide),
            json!({"face_expressions": false, "room_sounds": false,
                   "rev": {"face_expressions": "unknown", "room_sounds": "unknown"}})
        );
        std::fs::write(
            folder.join("face-expressions.json"),
            r#"{"version":1,"face_expressions":"yes","at_unix_s":5,"by":"app","rev":"r1"}"#,
        )
        .unwrap();
        std::fs::write(
            folder.join("room-sounds.json"),
            r#"{"version":1,"room_sounds":"yes","at_unix_s":5}"#,
        )
        .unwrap();
        // A slide lamp never says room sounds, even with a file there.
        assert_eq!(
            get(&slide),
            json!({"face_expressions": true, "rev": {"face_expressions": "r1"}})
        );
        assert_eq!(
            get(&no_slide),
            json!({"face_expressions": true, "room_sounds": true,
                   "rev": {"face_expressions": "r1", "room_sounds": "yes-5"}})
        );
        // Taken back after the helper's r2; then a state that can't be
        // read: `unknown`, which the helper never matches.
        std::fs::remove_file(folder.join("face-expressions.json")).unwrap();
        std::fs::write(
            folder.join(lamp_msgs::consent::STATE_FILE),
            r#"{"version":1,"revs":{"face_expressions":{"wrote":"r2"}}}"#,
        )
        .unwrap();
        assert_eq!(get(&slide)["rev"], json!({"face_expressions": "gone-r2"}));
        std::fs::write(folder.join(lamp_msgs::consent::STATE_FILE), "{").unwrap();
        assert_eq!(get(&slide)["rev"], json!({"face_expressions": "unknown"}));
        assert_eq!(get(&no_slide)["rev"]["room_sounds"], "yes-5");
        let _ = std::fs::remove_dir_all(folder);
    }
}
