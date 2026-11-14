//! The owner's two yeses from the app (#5500, `docs/cloud.md`, "The two
//! yeses from the app"): face expressions, and room sounds on a lamp with no
//! privacy slide. Off unless `LAMP_APP_CONSENT=on`, with `LAMP_CLOUD=on`;
//! without it both kinds are answered as any this lamp lacks.
//!
//! `consent.get` reads the files where they are kept
//! ([`lamp_msgs::consent::FOLDER`], which `lampos` may read). `consent.set`
//! takes exactly one key and hands one line to root's helper, `lamp-setup
//! change`, as the names go, since `lamp-cloud` can write nothing there; the
//! helper writes or removes the file and answers at once, and the reply is
//! what the files then say, as `consent.get` answers.
//!
//! Room sounds is said, and taken, only where it does something: a lamp
//! whose privacy route is `no-slide` (lamp-ears reads its yes only there).
//! `lamp-cloud` knows that from its own `LAMP_PRIVACY_CONTROLS`, set beside
//! Controls', Voice's and Ears' on such a lamp; anything but `no-slide`
//! exactly (the product's slide lamps leave it unset) is a slide lamp.
//!
//! Seen activity (#6543) is said, and taken, the same way: only where
//! lamp-seen looks, a lamp whose `lamp-cloud` has `LAMP_SEEN_ACTIVITY=on`
//! exactly, as lamp-seen's own unit has.
use lamp_msgs::consent::{FACE_EXPRESSIONS, ROOM_SOUNDS, SEEN_ACTIVITY};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use lamp_msgs::consent::{ENVIRONMENT, KINDS};

/// The privacy route's switch, read here only to know whether room sounds
/// applies.
pub const PRIVACY_ENVIRONMENT: &str = "LAMP_PRIVACY_CONTROLS";

/// The privacy route on a lamp with no slide: room sounds applies.
pub const NO_SLIDE: &str = "no-slide";

/// lamp-seen's switch, read here only to know whether seen activity
/// applies: `on` exactly.
pub const SEEN_ENVIRONMENT: &str = "LAMP_SEEN_ACTIVITY";

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
    /// Seen activity applies: lamp-seen is switched on.
    pub seen_activity: bool,
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
            seen_activity: var(SEEN_ENVIRONMENT).as_deref() == Some("on"),
        })
    }

    /// The keys this lamp has: face expressions, room sounds only with no
    /// slide, and seen activity only with lamp-seen on.
    pub fn keys(&self) -> Vec<&'static str> {
        let mut keys = vec![FACE_EXPRESSIONS];
        if self.room_sounds {
            keys.push(ROOM_SOUNDS);
        }
        if self.seen_activity {
            keys.push(SEEN_ACTIVITY);
        }
        keys
    }
}

/// What the lamp keeps now, as `consent.get` answers it: each key this lamp
/// has, `true` only while its file holds a yes.
pub fn now(place: &Place) -> Value {
    let mut said = Map::new();
    for key in place.keys() {
        let yes = lamp_msgs::consent::file_in(&place.folder, key)
            .is_some_and(|file| lamp_msgs::consent::read(&file, key).yes);
        said.insert(key.into(), yes.into());
    }
    Value::Object(said)
}

/// The helper's line for `consent.set`'s `data`, or why not: exactly one
/// key this lamp has, `true` or `false`. No error holds what was sent.
pub fn line(place: &Place, data: &Value) -> Result<String, String> {
    let why = || {
        format!(
            "consent.set takes exactly one of {}, true or false",
            place.keys().join(", ")
        )
    };
    let object = data
        .as_object()
        .filter(|object| object.len() == 1)
        .ok_or_else(why)?;
    let (key, given) = object.iter().next().ok_or_else(why)?;
    if key == ROOM_SOUNDS && !place.room_sounds {
        return Err("room sounds is not on this lamp: it has a privacy slide".into());
    }
    if key == SEEN_ACTIVITY && !place.seen_activity {
        return Err("seen activity is not on this lamp".into());
    }
    if !place.keys().contains(&key.as_str()) {
        return Err(why());
    }
    let yes = given.as_bool().ok_or_else(why)?;
    Ok(json!({ "consent": { key.as_str(): yes } }).to_string())
}

/// `consent.set`: checked here, made by the helper, and answered with what
/// the files say after it.
pub fn set(place: &Place, data: &Value) -> Result<Value, String> {
    let line = line(place, data)?;
    crate::settings::ask(&place.socket, &line, place.wait)?;
    Ok(now(place))
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
        seen_activity: false,
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
            line(&slide, &json!({"face_expressions": true})).unwrap(),
            r#"{"consent":{"face_expressions":true}}"#
        );
        assert_eq!(
            line(&slide, &json!({"face_expressions": false})).unwrap(),
            r#"{"consent":{"face_expressions":false}}"#
        );
        assert!(
            line(&slide, &json!({"room_sounds": true}))
                .unwrap_err()
                .contains("privacy slide")
        );
        // Seen activity only where lamp-seen is on (#6543).
        assert_eq!(
            line(&slide, &json!({"seen_activity": true})).unwrap_err(),
            "seen activity is not on this lamp"
        );
        let seen = Place {
            seen_activity: true,
            ..place(false)
        };
        assert_eq!(seen.keys(), [FACE_EXPRESSIONS, SEEN_ACTIVITY]);
        assert_eq!(
            line(&seen, &json!({"seen_activity": true})).unwrap(),
            r#"{"consent":{"seen_activity":true}}"#
        );
        assert_eq!(
            line(&seen, &json!({"seen_activity": false})).unwrap(),
            r#"{"consent":{"seen_activity":false}}"#
        );
        let no_slide = place(true);
        assert_eq!(
            line(&no_slide, &json!({"room_sounds": true})).unwrap(),
            r#"{"consent":{"room_sounds":true}}"#
        );
        for data in [
            json!({}),
            json!(null),
            json!([true]),
            json!({"camera": true}),
            json!({"face_expressions": "yes"}),
            json!({"face_expressions": 1}),
            json!({"face_expressions": null}),
            json!({"face_expressions": true, "room_sounds": true}),
        ] {
            let why = line(&no_slide, &data).unwrap_err();
            assert_eq!(
                why,
                "consent.set takes exactly one of face_expressions, room_sounds, true or false",
                "{data}"
            );
        }
    }

    #[test]
    fn a_get_says_each_key_this_lamp_has_from_its_file() {
        let folder = std::env::temp_dir().join(format!("cloud-consent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let slide = place_in(&folder, Path::new("/nowhere.sock"), false);
        let no_slide = place_in(&folder, Path::new("/nowhere.sock"), true);
        assert_eq!(get(&slide), json!({"face_expressions": false}));
        assert_eq!(
            get(&no_slide),
            json!({"face_expressions": false, "room_sounds": false})
        );
        std::fs::write(
            folder.join("face-expressions.json"),
            r#"{"version":1,"face_expressions":"yes","at_unix_s":5,"by":"app"}"#,
        )
        .unwrap();
        std::fs::write(
            folder.join("room-sounds.json"),
            r#"{"version":1,"room_sounds":"yes","at_unix_s":5}"#,
        )
        .unwrap();
        // A slide lamp never says room sounds, even with a file there.
        assert_eq!(get(&slide), json!({"face_expressions": true}));
        // Nor a lamp with lamp-seen off seen activity (#6543).
        std::fs::write(
            folder.join("seen-activity.json"),
            lamp_msgs::consent::bytes(SEEN_ACTIVITY, 5, lamp_msgs::consent::BY_VOICE),
        )
        .unwrap();
        assert_eq!(get(&slide), json!({"face_expressions": true}));
        let seen = Place {
            seen_activity: true,
            ..slide.clone()
        };
        assert_eq!(
            get(&seen),
            json!({"face_expressions": true, "seen_activity": true})
        );
        assert_eq!(
            get(&no_slide),
            json!({"face_expressions": true, "room_sounds": true})
        );
        let _ = std::fs::remove_dir_all(folder);
    }
}
