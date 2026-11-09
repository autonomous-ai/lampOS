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
//!
//! Seen activity (#6543) is said, and taken, the same way: only where
//! lamp-seen looks, a lamp whose `lamp-cloud` has `LAMP_SEEN_ACTIVITY=on`
//! exactly, as lamp-seen's own unit has.
use lamp_msgs::consent::{FACE_EXPRESSIONS, ROOM_SOUNDS, SEEN_ACTIVITY};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use lamp_msgs::consent::{ENVIRONMENT, KINDS};

/// What `system.info`'s `can` carries beside the kinds when the app's
/// yeses are taken and `LAMP_NOTICING_BY_DEFAULT=on` (#7767, slice 6): the
/// app reads a missing answer as on only on a lamp that says it.
pub const DEFAULTS_ON: &str = "defaults_on";

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
    /// `LAMP_NOTICING_BY_DEFAULT=on` in `lamp-cloud`'s own unit (#7767,
    /// [`lamp_msgs::consent::noticing_by_default`]).
    pub by_default: bool,
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
        Self::from_var(|name| std::env::var(name).ok())
    }

    /// [`Place::from_environment`] with `var` as the environment.
    pub fn from_var(var: impl Fn(&str) -> Option<String>) -> Option<Self> {
        gates(var("LAMP_CLOUD").as_deref(), var(ENVIRONMENT).as_deref()).then(|| Self {
            folder: lamp_msgs::consent::FOLDER.into(),
            socket: crate::settings::SOCKET.into(),
            wait: WAIT,
            room_sounds: var(PRIVACY_ENVIRONMENT).as_deref() == Some(NO_SLIDE),
            seen_activity: var(SEEN_ENVIRONMENT).as_deref() == Some("on"),
            posture: false,
            by_default: lamp_msgs::consent::by_default_on(
                var(lamp_msgs::consent::NOTICING_BY_DEFAULT).as_deref(),
            ),
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
/// has, `true` only while its file holds a yes, or, with
/// [`Place::by_default`] (#7767, slice 6), while it holds no kept no, by the
/// readers' one rule ([`lamp_msgs::consent::read_or`]); and `rev`, each
/// key's revision ([`lamp_msgs::consent::rev`]; `unknown` while the state
/// can't be read, which the helper never matches).
pub fn now(place: &Place) -> Value {
    let mut said = Map::new();
    let mut rev = Map::new();
    for (key, yes, kept) in
        lamp_msgs::consent::snapshot(&place.folder, &place.keys(), place.by_default)
    {
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
    if key == SEEN_ACTIVITY && !place.seen_activity {
        return Err("seen activity is not on this lamp".into());
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

/// The owner's desk noticing switch in the app (#6543, the owner's call
/// c6946201674, "like muted sound"): `desk_noticing.get` (`{}`) and
/// `desk_noticing.set` (`{"allow": bool, "request_id": N}`, N the app's
/// send time in Unix ms, so the helper refuses a yes resent or sent before
/// the lamp's last change). Taken with lamp-seen switched
/// on alone (`LAMP_SEEN_ACTIVITY=on` in `lamp-cloud`'s own unit, with
/// `LAMP_CLOUD=on`), not behind [`ENVIRONMENT`]: it is the same yes as
/// `consent.set`'s `seen_activity`, the same file, made by the same helper
/// with `"by":"app"`, so a yes from the app and one spoken at the lamp are
/// one yes. Off is what the voice's stop does: the file removed (a no kept
/// where a missing file is on, #7767), and lamp-seen closes its gate at its
/// next read and forgets the sitting.
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
        by_default: lamp_msgs::consent::by_default_on(
            var(lamp_msgs::consent::NOTICING_BY_DEFAULT).as_deref(),
        ),
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
/// `"voice"` for any other (a file from before `by` is the voice's),
/// `"default"` for a missing file read as on ([`Place::by_default`],
/// #7767, slice 6), null when off; `posture`, whether
/// `LAMP_SEEN_POSTURE=on`, so the app shows the disclosure naming it.
pub fn desk_now(place: &Place) -> Value {
    let kept = lamp_msgs::consent::file_in(&place.folder, SEEN_ACTIVITY)
        .map(|file| lamp_msgs::consent::read_or(&file, SEEN_ACTIVITY, place.by_default))
        .unwrap_or_default();
    let by = kept.yes.then(|| {
        if kept.by_default {
            lamp_msgs::consent::BY_DEFAULT
        } else if kept.by_app() {
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
        by_default: false,
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
                lamp_msgs::consent::NOTICING_BY_DEFAULT => None,
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
        // #7767, slice 6: the default said only with its switch `on`
        // exactly, in both places.
        for (value, said) in [(Some("on"), true), (None, false), (Some("ON"), false)] {
            let var = |name: &str| match name {
                lamp_msgs::consent::NOTICING_BY_DEFAULT => value.map(str::to_owned),
                POSTURE_ENVIRONMENT => None,
                _ => Some("on".to_owned()),
            };
            assert_eq!(desk_from(var).unwrap().by_default, said, "{value:?}");
            assert_eq!(Place::from_var(var).unwrap().by_default, said, "{value:?}");
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
        // Seen activity only where lamp-seen is on (#6543).
        assert_eq!(
            line(&slide, &json!({"seen_activity": true, "if": "t"})).unwrap_err(),
            "seen activity is not on this lamp"
        );
        let seen = Place {
            seen_activity: true,
            ..place(false)
        };
        assert_eq!(seen.keys(), [FACE_EXPRESSIONS, SEEN_ACTIVITY]);
        assert_eq!(
            line(&seen, &json!({"seen_activity": true, "if": "t"})).unwrap(),
            r#"{"consent":{"seen_activity":true},"if":"t"}"#
        );
        assert_eq!(
            line(&seen, &json!({"seen_activity": false, "if": "t"})).unwrap(),
            r#"{"consent":{"seen_activity":false},"if":"t"}"#
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
        // Nor a lamp with lamp-seen off seen activity (#6543).
        std::fs::write(
            folder.join("seen-activity.json"),
            lamp_msgs::consent::bytes(SEEN_ACTIVITY, 5, lamp_msgs::consent::BY_VOICE),
        )
        .unwrap();
        assert_eq!(
            get(&slide),
            json!({"face_expressions": true, "rev": {"face_expressions": "r1"}})
        );
        let seen = Place {
            seen_activity: true,
            ..slide.clone()
        };
        assert_eq!(
            get(&seen),
            json!({"face_expressions": true, "seen_activity": true,
                   "rev": {"face_expressions": "r1", "seen_activity": "yes-5"}})
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

    /// #7767, slice 6: with `LAMP_NOTICING_BY_DEFAULT` on and the old nos
    /// kept, a missing file is on in both answers, desk noticing's `by`
    /// `"default"`; a kept no is off either way, and before the done mark,
    /// or with the switch off, a missing file is off as before.
    #[test]
    fn with_the_switch_on_a_missing_file_is_on_and_a_kept_no_is_off() {
        let folder =
            std::env::temp_dir().join(format!("cloud-consent-default-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let on = Place {
            seen_activity: true,
            by_default: true,
            ..place_in(&folder, Path::new("/nowhere.sock"), true)
        };
        let off = Place {
            by_default: false,
            ..on.clone()
        };
        let all =
            |yes: bool| json!({"face_expressions": yes, "room_sounds": yes, "seen_activity": yes});
        let desk =
            |on: bool, by: Value| json!({"desk_noticing": {"on": on, "by": by, "posture": false}});
        // The yeses alone: each key's revision is the same whatever the
        // default (`a_get_says_each_key_this_lamp_has_from_its_file_with_its_rev`).
        let get = |place: &Place| {
            let mut said = get(place);
            said.as_object_mut().unwrap().remove("rev");
            said
        };
        // Before the old nos are kept, no file is no.
        assert_eq!(get(&on), all(false));
        assert_eq!(desk_now(&on), desk(false, Value::Null));
        std::fs::write(folder.join(lamp_msgs::consent::OLD_NOS_DONE), b"{}").unwrap();
        assert_eq!(get(&on), all(true));
        assert_eq!(desk_now(&on), desk(true, json!("default")));
        // A default yes is no file's: its revision is the state's, never a
        // yes's own (no state here: `unknown`).
        assert_eq!(
            super::get(&on)["rev"],
            json!({"face_expressions": "unknown", "room_sounds": "unknown", "seen_activity": "unknown"})
        );
        assert_eq!(get(&off), all(false));
        assert_eq!(desk_now(&off), desk(false, Value::Null));
        // A kept no, the app's or the voice's: off, whatever the switch.
        for (key, by) in [
            (FACE_EXPRESSIONS, lamp_msgs::consent::BY_APP),
            (SEEN_ACTIVITY, lamp_msgs::consent::BY_VOICE),
        ] {
            std::fs::write(
                lamp_msgs::consent::file_in(&folder, key).unwrap(),
                lamp_msgs::consent::no_bytes(key, 5, by),
            )
            .unwrap();
        }
        assert_eq!(
            get(&on),
            json!({"face_expressions": false, "room_sounds": true, "seen_activity": false})
        );
        assert_eq!(desk_now(&on), desk(false, Value::Null));
        assert_eq!(get(&off), all(false));
        // A kept yes is said by who gave it, never as the default.
        std::fs::write(
            lamp_msgs::consent::file_in(&folder, SEEN_ACTIVITY).unwrap(),
            lamp_msgs::consent::bytes(SEEN_ACTIVITY, 6, lamp_msgs::consent::BY_APP),
        )
        .unwrap();
        assert_eq!(desk_now(&on), desk(true, json!("app")));
        assert_eq!(desk_now(&off), desk(true, json!("app")));
        let _ = std::fs::remove_dir_all(folder);
    }
}
