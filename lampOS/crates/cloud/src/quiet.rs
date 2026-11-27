//! The person's quiet hours from the owner's app (#8866, `docs/cloud.md`,
//! "Quiet hours from the app"): `quiet.get` and `quiet.set {"on": bool,
//! "start": "HH:MM", "end": "HH:MM"}`, both answered `{"on", "start",
//! "end"}`, the window the lamp then keeps.
//!
//! Both go to `lamp-actions`' socket (`quiet_get`, `quiet_put`), the
//! window's only keeper, which says it on `quiet/window` for Skills: the
//! night ceilings hold in it while it is on, and the evening glow and the
//! morning hum take their hours from its start and end. Nothing is kept
//! here. A set is checked whole before Actions is asked, so a refused one
//! changes nothing anywhere.
//!
//! Off unless `LAMP_QUIET_HOURS=on` exactly, with `LAMP_CLOUD=on`: then the
//! two kinds are in `can`; without it they are "not on this lamp", and the
//! lamp keeps its fixed 21:00 to 06:00.
use lamp_msgs::{QuietWindow, quiet_minute, quiet_time};
use serde_json::{Map, Value, json};

/// The two kinds, in `can`'s order.
pub const KINDS: [&str; 2] = ["quiet.get", "quiet.set"];

/// The switch: `on` exactly.
pub const ENVIRONMENT: &str = "LAMP_QUIET_HOURS";

/// What a set must be (cloud.md).
pub const SHAPE: &str = "quiet.set takes {\"on\": bool, \"start\": \"HH:MM\", \"end\": \"HH:MM\"}";
/// Why a set with the same start and end is refused (cloud.md).
pub const SAME: &str = "quiet hours need a start and an end that differ";
/// What an Actions from before #8866 is said as: it takes no quiet op.
pub const OLD_ACTIONS: &str = "this lamp's actions can't keep quiet hours yet";
/// Actions' answer that isn't a window.
pub const NONSENSE: &str = "the lamp's actions said nonsense";

/// Whether the switch is on, as the environment says.
pub fn on_from_environment() -> bool {
    on_from_var(|name| std::env::var(name).ok())
}

/// [`on_from_environment`] with `var` as the environment.
pub fn on_from_var(var: impl Fn(&str) -> Option<String>) -> bool {
    let on = |name: &str| var(name).as_deref() == Some("on");
    on("LAMP_CLOUD") && on(ENVIRONMENT)
}

/// The window a `quiet.set`'s `data` asks for: an object of exactly `on`
/// (a JSON bool), `start` and `end` (`HH:MM`, 24-hour), the two times
/// different. Anything else is refused before Actions is asked.
pub fn asked(data: &Value) -> Result<QuietWindow, String> {
    let shape = || SHAPE.to_owned();
    let fields: &Map<String, Value> = data.as_object().ok_or_else(shape)?;
    if fields.len() != 3 {
        return Err(shape());
    }
    let on = fields
        .get("on")
        .and_then(Value::as_bool)
        .ok_or_else(shape)?;
    let time = |key: &str| {
        fields
            .get(key)
            .and_then(Value::as_str)
            .and_then(quiet_minute)
            .ok_or_else(shape)
    };
    let window = QuietWindow {
        on,
        start: time("start")?,
        end: time("end")?,
    };
    window.check().map_err(|_| SAME.to_owned())?;
    Ok(window)
}

/// The line Actions is asked for a set.
pub fn put_line(window: &QuietWindow) -> Value {
    json!({"op": "quiet_put", "on": window.on, "start": quiet_time(window.start), "end": quiet_time(window.end)})
}

/// The app's reply from Actions' answer: its window, checked, as `{"on",
/// "start", "end"}`.
pub fn reply(answer: &Value) -> Result<Value, String> {
    let nonsense = || NONSENSE.to_owned();
    let on = answer["on"].as_bool().ok_or_else(nonsense)?;
    let time = |key: &str| {
        answer[key]
            .as_str()
            .and_then(quiet_minute)
            .ok_or_else(nonsense)
    };
    let window = QuietWindow {
        on,
        start: time("start")?,
        end: time("end")?,
    };
    window.check().map_err(|_| nonsense())?;
    Ok(json!({"on": window.on, "start": quiet_time(window.start), "end": quiet_time(window.end)}))
}

/// Why Actions refused, said to the app: its own words when it says them
/// with a code, [`OLD_ACTIONS`] for one from before the quiet ops, which
/// answers any line it doesn't know as a call it can't do.
pub fn refusal(answer: &Value) -> String {
    match (answer["code"].as_str(), answer["error"].as_str()) {
        (Some(_), Some(error)) => error.to_owned(),
        _ => OLD_ACTIONS.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_is_on_only_with_on_exactly_and_the_cloud() {
        let with = |cloud: Option<&str>, quiet: Option<&str>| {
            on_from_var(|name| match name {
                "LAMP_CLOUD" => cloud.map(str::to_owned),
                ENVIRONMENT => quiet.map(str::to_owned),
                _ => None,
            })
        };
        assert!(with(Some("on"), Some("on")));
        for quiet in [
            None,
            Some(""),
            Some("ON"),
            Some("1"),
            Some("true"),
            Some("on "),
        ] {
            assert!(!with(Some("on"), quiet), "{quiet:?}");
        }
        assert!(!with(None, Some("on")));
    }

    #[test]
    fn a_set_is_exactly_three_keys_a_bool_and_two_times_that_differ() {
        assert_eq!(
            asked(&json!({"on": true, "start": "22:00", "end": "07:00"})),
            Ok(QuietWindow {
                on: true,
                start: 1320,
                end: 420
            })
        );
        assert_eq!(
            asked(&json!({"on": false, "start": "00:00", "end": "23:59"})).map(|w| w.on),
            Ok(false)
        );
        for refused in [
            Value::Null,
            json!([]),
            json!({}),
            json!({"on": true, "start": "22:00"}),
            json!({"on": true, "start": "22:00", "end": "07:00", "days": "all"}),
            json!({"on": "true", "start": "22:00", "end": "07:00"}),
            json!({"on": 1, "start": "22:00", "end": "07:00"}),
            json!({"on": true, "start": "10pm", "end": "07:00"}),
            json!({"on": true, "start": "24:00", "end": "07:00"}),
            json!({"on": true, "start": "7:00", "end": "07:00"}),
            json!({"on": true, "start": 1320, "end": "07:00"}),
            json!({"on": true, "begin": "22:00", "end": "07:00"}),
        ] {
            assert_eq!(asked(&refused), Err(SHAPE.to_owned()), "{refused}");
        }
        assert_eq!(
            asked(&json!({"on": true, "start": "07:00", "end": "07:00"})),
            Err(SAME.to_owned())
        );
    }

    #[test]
    fn actions_answer_is_checked_and_an_old_actions_is_named() {
        assert_eq!(
            reply(&json!({"ok": true, "on": true, "start": "21:00", "end": "06:00"})),
            Ok(json!({"on": true, "start": "21:00", "end": "06:00"}))
        );
        for nonsense in [
            json!({"ok": true}),
            json!({"ok": true, "on": true, "start": "21:00", "end": "21:00"}),
            json!({"ok": true, "on": true, "start": 1260, "end": "06:00"}),
        ] {
            assert_eq!(reply(&nonsense), Err(NONSENSE.to_owned()), "{nonsense}");
        }
        assert_eq!(
            refusal(&json!({"ok": false, "error": "I can't do that."})),
            OLD_ACTIONS
        );
        assert_eq!(
            refusal(
                &json!({"ok": false, "code": "unavailable", "error": "output caps are not enabled"})
            ),
            "output caps are not enabled"
        );
        assert_eq!(
            put_line(&QuietWindow::default()),
            json!({"op": "quiet_put", "on": true, "start": "21:00", "end": "06:00"})
        );
    }
}
