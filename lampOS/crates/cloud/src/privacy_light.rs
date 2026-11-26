//! The privacy light's look from the owner's app (#8696, `docs/cloud.md`,
//! "The privacy light from the app"): `led.privacy.get` and
//! `led.privacy.set {"look": "red"|"dim"|"off"}`, both answered
//! `{"look": ..}`. The look changes only what the ring shows while the
//! privacy slide is closed; the microphones and the camera close the same
//! in all three.
//!
//! Both go through root's change helper (`lamp-setup change`), the get too:
//! the look is kept in the light folder, which stays root's alone (0700), and
//! the helper takes these two lines only from this process, by its peer
//! credentials. So unlike `led.resting.*`, which goes over the bus where any
//! `lampos` part may publish, nothing but the person's app can set it: there
//! is no bus topic, no lamp-control op, and no voice or agent tool.
//!
//! Off unless `LAMP_PRIVACY_LIGHT=on` exactly, with `LAMP_CLOUD=on` and
//! `LAMP_SETTINGS_CHANGE=on`, on a lamp with a slide (`LAMP_PRIVACY_CONTROLS`
//! not `no-slide`): a lamp with no slide shows no red to change, so it never
//! lists the two kinds.
use lamp_msgs::privacy_light::Look;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::Duration;

/// The two kinds, in `can`'s order.
pub const KINDS: [&str; 2] = ["led.privacy.get", "led.privacy.set"];

/// How long the helper is waited on: it writes or reads one small file and
/// answers before anything else, as for the yeses.
pub const WAIT: Duration = crate::consent::WAIT;

/// Where the look is asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    /// Root's change helper's socket ([`crate::settings::SOCKET`]).
    pub socket: PathBuf,
    pub wait: Duration,
}

impl Place {
    /// The product's, as the environment says: none when off.
    pub fn from_environment() -> Option<Self> {
        Self::from_var(|name| std::env::var(name).ok())
    }

    /// [`Place::from_environment`] with `var` as the environment.
    pub fn from_var(var: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let on = |name: &str| var(name).as_deref() == Some("on");
        let slide =
            var(crate::consent::PRIVACY_ENVIRONMENT).as_deref() != Some(crate::consent::NO_SLIDE);
        (on("LAMP_CLOUD")
            && on(SETTINGS_ENVIRONMENT)
            && lamp_msgs::privacy_light::on(var(lamp_msgs::privacy_light::ENVIRONMENT).as_deref())
            && slide)
            .then(|| Self {
                socket: crate::settings::SOCKET.into(),
                wait: WAIT,
            })
    }
}

/// The settings change's switch on `lamp-cloud` (#1959): its helper is
/// the one that keeps the look.
pub const SETTINGS_ENVIRONMENT: &str = "LAMP_SETTINGS_CHANGE";

/// Why a set is refused before the helper.
pub const REFUSED: &str = "led.privacy.set takes exactly {\"look\": \"red\" | \"dim\" | \"off\"}";

/// Why a get is answered with an error: the helper said done but named no
/// look, so the app is not shown a look the ring may not have.
pub const NAMED_NO_LOOK: &str = "the lamp's settings named no privacy light look";

/// The helper's line for `led.privacy.set`'s `data`, or why not: exactly
/// one key, `look`, one of the three names.
pub fn line(data: &Value) -> Result<String, String> {
    let look = data
        .as_object()
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get("look"))
        .and_then(Value::as_str)
        .and_then(Look::named)
        .ok_or(REFUSED)?;
    Ok(json!({ "privacy_light": look.name() }).to_string())
}

/// The reply for what the helper said: `{"look": ..}`, the look it names
/// (the first builder's P3 on #8733). When it names none, a set it said
/// done to answers the request's own look, `asked`, and a get answers
/// [`NAMED_NO_LOOK`], so the app never shows red while the ring shows the
/// person's pick.
fn reply(done: Option<Value>, asked: Option<Look>) -> Result<Value, String> {
    let look = done
        .as_ref()
        .and_then(|data| data.get("look"))
        .and_then(Value::as_str)
        .and_then(Look::named)
        .or(asked)
        .ok_or(NAMED_NO_LOOK)?;
    Ok(json!({ "look": look.name() }))
}

/// `led.privacy.get`: read by the helper, writing nothing.
pub fn get(place: &Place) -> Result<Value, String> {
    let done = crate::settings::ask(
        &place.socket,
        &json!({"privacy_light_get": true}).to_string(),
        place.wait,
    )?;
    reply(done, None)
}

/// `led.privacy.set`: checked here, kept by the helper, and answered with
/// what the file then says, or the request's own look when the helper said
/// done but named none.
pub fn set(place: &Place, data: &Value) -> Result<Value, String> {
    let line = line(data)?;
    let done = crate::settings::ask(&place.socket, &line, place.wait)?;
    let asked = data["look"].as_str().and_then(Look::named);
    reply(done, asked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// The switch, the settings change and a slide, all three, or neither
    /// kind is there (#8696, the first builder's answers 5 and 6).
    #[test]
    fn led_privacy_is_there_only_with_its_switches_and_a_slide() {
        let all = HashMap::from([
            ("LAMP_CLOUD", "on"),
            ("LAMP_SETTINGS_CHANGE", "on"),
            ("LAMP_PRIVACY_LIGHT", "on"),
        ]);
        let with = |changed: &[(&'static str, Option<&'static str>)]| {
            let mut env = all.clone();
            for (name, value) in changed {
                match value {
                    Some(value) => env.insert(name, value),
                    None => env.remove(name),
                };
            }
            Place::from_var(|name| env.get(name).map(|value| value.to_string()))
        };
        assert_eq!(
            with(&[]),
            Some(Place {
                socket: crate::settings::SOCKET.into(),
                wait: WAIT
            })
        );
        assert!(with(&[("LAMP_PRIVACY_CONTROLS", Some("slide"))]).is_some());
        for changed in [
            ("LAMP_PRIVACY_LIGHT", None),
            ("LAMP_PRIVACY_LIGHT", Some("1")),
            ("LAMP_PRIVACY_LIGHT", Some("ON")),
            ("LAMP_PRIVACY_LIGHT", Some("off")),
            ("LAMP_SETTINGS_CHANGE", None),
            ("LAMP_CLOUD", None),
            ("LAMP_PRIVACY_CONTROLS", Some("no-slide")),
        ] {
            assert!(with(&[changed]).is_none(), "{changed:?}");
        }
    }

    #[test]
    fn only_one_known_look_is_handed_to_the_helper() {
        for look in ["red", "dim", "off"] {
            assert_eq!(
                line(&json!({ "look": look })).unwrap(),
                json!({ "privacy_light": look }).to_string()
            );
        }
        for data in [
            json!({"look": "blue"}),
            json!({"look": "Dim"}),
            json!({"look": 2}),
            json!({"look": null}),
            json!({"look": "dim", "level": 1}),
            json!({"colour": "dim"}),
            json!({}),
            json!("dim"),
            Value::Null,
        ] {
            assert_eq!(line(&data), Err(REFUSED.to_owned()), "{data}");
        }
        // What the helper names, else the set's own look, else an error.
        let off = Some(Look::named("off").unwrap());
        assert_eq!(
            reply(Some(json!({"look": "off"})), None),
            Ok(json!({"look": "off"}))
        );
        assert_eq!(
            reply(Some(json!({"look": "red"})), off),
            Ok(json!({"look": "red"}))
        );
        assert_eq!(
            reply(Some(json!({"look": "blue"})), off),
            Ok(json!({"look": "off"}))
        );
        assert_eq!(reply(None, off), Ok(json!({"look": "off"})));
        assert_eq!(
            reply(Some(json!({"look": "blue"})), None),
            Err(NAMED_NO_LOOK.to_owned())
        );
        assert_eq!(reply(None, None), Err(NAMED_NO_LOOK.to_owned()));
    }

    /// The first builder's P3 on #8733: a helper answer that names no look
    /// is never shown as red. A set the helper said done to answers the
    /// request's own look; a get answers an error, so the app and the ring
    /// stay in step.
    #[test]
    fn an_answer_naming_no_look_is_not_red() {
        use std::io::{BufRead, Write};
        let folder = std::path::PathBuf::from(format!("/tmp/lcpn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let socket = folder.join("change.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        std::thread::spawn(move || {
            for (n, stream) in listener.incoming().enumerate() {
                let Ok(mut stream) = stream else { return };
                let mut line = String::new();
                let _ = std::io::BufReader::new(&stream).read_line(&mut line);
                let done = if n % 2 == 0 {
                    json!({})
                } else {
                    json!({"look": "blue"})
                };
                let _ = writeln!(stream, "{}", json!({ "done": done }));
            }
        });
        let place = Place { socket, wait: WAIT };
        for _ in 0..2 {
            assert_eq!(
                set(&place, &json!({"look": "dim"})),
                Ok(json!({"look": "dim"}))
            );
            assert_eq!(
                set(&place, &json!({"look": "off"})),
                Ok(json!({"look": "off"}))
            );
        }
        for _ in 0..2 {
            assert_eq!(get(&place), Err(NAMED_NO_LOOK.to_owned()));
        }
        let _ = std::fs::remove_dir_all(&folder);
    }
}
