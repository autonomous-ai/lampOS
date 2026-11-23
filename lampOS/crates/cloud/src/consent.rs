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
    /// Posture too (`LAMP_SEEN_POSTURE=on` in `lamp-cloud`'s own unit,
    /// #6543): said in the desk noticing reply, so the app shows the
    /// disclosure that names posture.
    pub posture: bool,
}

/// lamp-seen's posture switch, read here only to say it: `on` exactly.
pub const POSTURE_ENVIRONMENT: &str = "LAMP_SEEN_POSTURE";

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
            posture: false,
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

/// The owner's desk noticing switch in the app (#6543, the owner's call
/// c6946201674, "like muted sound"): `desk_noticing.get` (`{}`) and
/// `desk_noticing.set` (`{"allow": bool, "request_id": N}`, N the app's
/// send time in Unix ms, so the helper refuses a yes resent or sent before
/// the lamp's last change). Taken with lamp-seen switched
/// on alone (`LAMP_SEEN_ACTIVITY=on` in `lamp-cloud`'s own unit, with
/// `LAMP_CLOUD=on`), not behind [`ENVIRONMENT`]: it is the same yes as
/// `consent.set`'s `seen_activity`, the same file, made by the same helper
/// with `"by":"app"`, so a yes from the app and one spoken at the lamp are
/// one yes. Off is what the voice's stop does: the file removed, and
/// lamp-seen closes its gate at its next read and forgets the sitting.
pub const DESK_KINDS: [&str; 2] = ["desk_noticing.get", "desk_noticing.set"];

/// The desk noticing switch's place, as the environment says: none unless
/// `LAMP_CLOUD=on` and [`SEEN_ENVIRONMENT`]`=on`, exactly.
pub fn desk_from_environment() -> Option<Place> {
    desk_from(|name| std::env::var(name).ok())
}

/// [`desk_from_environment`] with `var` as the environment.
pub fn desk_from(var: impl Fn(&str) -> Option<String>) -> Option<Place> {
    desk_gates(
        var("LAMP_CLOUD").as_deref(),
        var(SEEN_ENVIRONMENT).as_deref(),
    )
    .then(|| Place {
        posture: var(POSTURE_ENVIRONMENT).as_deref() == Some("on"),
        ..desk_in(
            Path::new(lamp_msgs::consent::FOLDER),
            Path::new(crate::settings::SOCKET),
        )
    })
}

/// Whether the desk noticing kinds are taken: `LAMP_CLOUD=on` and
/// `LAMP_SEEN_ACTIVITY=on`.
pub fn desk_gates(cloud: Option<&str>, seen: Option<&str>) -> bool {
    cloud == Some("on") && seen == Some("on")
}

/// The desk noticing switch's place in `folder`, the helper at `socket`.
pub fn desk_in(folder: &Path, socket: &Path) -> Place {
    place_in(folder, socket, false)
}

/// What the switch says now: `{"desk_noticing": {"on", "by", "posture"}}`,
/// `on` only while the file holds a yes, `by` `"app"` for the app's,
/// `"voice"` for any other (a file from before `by` is the voice's), null
/// when off; `posture`, whether `LAMP_SEEN_POSTURE=on`, so the app shows
/// the disclosure naming it.
pub fn desk_now(place: &Place) -> Value {
    let kept = lamp_msgs::consent::file_in(&place.folder, SEEN_ACTIVITY)
        .map(|file| lamp_msgs::consent::read(&file, SEEN_ACTIVITY))
        .unwrap_or_default();
    let by = kept.yes.then(|| {
        if kept.by_app() {
            lamp_msgs::consent::BY_APP
        } else {
            lamp_msgs::consent::BY_VOICE
        }
    });
    json!({"desk_noticing": {"on": kept.yes, "by": by, "posture": place.posture}})
}

/// A `desk_noticing.set` with no `request_id`, or one that is no positive
/// integer.
pub const DESK_REQUEST_ID: &str = "request_id must be the app's send time in Unix milliseconds";

/// `desk_noticing.set`: `allow` true or false and the app's `request_id`,
/// handed to the helper as `{"desk_noticing": {"allow", "request_id"}}`,
/// and answered with [`desk_now`]. The helper keeps a yes only with an id
/// above the mark beside it, and refuses it otherwise; a yes already kept,
/// the voice's too, is left as it is.
pub fn desk_set(place: &Place, data: &Value) -> Result<Value, String> {
    let line = desk_line(data)?;
    crate::settings::ask(&place.socket, &line, place.wait)?;
    Ok(desk_now(place))
}

/// The helper's line for `desk_noticing.set`'s `data`, or why not.
fn desk_line(data: &Value) -> Result<String, String> {
    let allow = data
        .get("allow")
        .and_then(Value::as_bool)
        .ok_or("allow must be true or false")?;
    let id = data
        .get("request_id")
        .and_then(Value::as_u64)
        .filter(|id| *id > 0)
        .ok_or(DESK_REQUEST_ID)?;
    Ok(json!({"desk_noticing": {"allow": allow, "request_id": id}}).to_string())
}

/// A place in `folder`, the helper at `socket`, for tests and rigs.
pub fn place_in(folder: &Path, socket: &Path, room_sounds: bool) -> Place {
    Place {
        folder: folder.into(),
        socket: socket.into(),
        wait: WAIT,
        room_sounds,
        seen_activity: false,
        posture: false,
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

    /// #6543 (Twelve's P2 on #7551): desk noticing is taken only with both
    /// switches `on` exactly.
    #[test]
    fn desk_noticing_needs_the_cloud_and_lamp_seen_on_exactly() {
        assert!(desk_gates(Some("on"), Some("on")));
        for (cloud, seen) in [
            (Some("on"), None),
            (Some("on"), Some("1")),
            (Some("on"), Some("ON")),
            (None, Some("on")),
            (Some("on "), Some("on")),
            (Some("off"), Some("on")),
        ] {
            assert!(!desk_gates(cloud, seen), "{cloud:?} {seen:?}");
            let var = |name: &str| match name {
                "LAMP_CLOUD" => cloud.map(str::to_owned),
                SEEN_ENVIRONMENT => seen.map(str::to_owned),
                _ => Some("on".to_owned()),
            };
            assert_eq!(desk_from(var), None, "{cloud:?} {seen:?}");
        }
        // On: the product's folder and helper, posture said only with its
        // own switch `on` exactly.
        for (posture, said) in [(Some("on"), true), (None, false), (Some("yes"), false)] {
            let var = |name: &str| match name {
                POSTURE_ENVIRONMENT => posture.map(str::to_owned),
                _ => Some("on".to_owned()),
            };
            assert_eq!(
                desk_from(var),
                Some(Place {
                    posture: said,
                    ..place_in(
                        Path::new(lamp_msgs::consent::FOLDER),
                        Path::new(crate::settings::SOCKET),
                        false
                    )
                }),
                "{posture:?}"
            );
        }
    }

    /// #6543 (Heron's P2 on #7551): the set carries the app's request_id
    /// to the helper; none, or one that is no positive integer, is refused
    /// before it.
    #[test]
    fn a_desk_noticing_set_needs_the_apps_request_id() {
        assert_eq!(
            desk_line(&json!({"allow": true, "request_id": 1_763_380_000_000_u64})).unwrap(),
            r#"{"desk_noticing":{"allow":true,"request_id":1763380000000}}"#
        );
        assert_eq!(
            desk_line(&json!({"allow": false, "request_id": 7})).unwrap(),
            r#"{"desk_noticing":{"allow":false,"request_id":7}}"#
        );
        for data in [
            json!({"allow": true}),
            json!({"allow": true, "request_id": 0}),
            json!({"allow": true, "request_id": -3}),
            json!({"allow": true, "request_id": 1.5}),
            json!({"allow": true, "request_id": "17"}),
            json!({"allow": false, "request_id": null}),
        ] {
            assert_eq!(desk_line(&data).unwrap_err(), DESK_REQUEST_ID, "{data}");
        }
        assert_eq!(
            desk_line(&json!({"request_id": 5})).unwrap_err(),
            "allow must be true or false"
        );
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
