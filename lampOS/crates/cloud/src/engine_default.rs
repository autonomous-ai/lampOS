//! The lamp's default agent chosen in the owner's app (#6492):
//! `engines.default` (`{"engine": "hermes"}` or `"openclaw"`), with the same
//! switches as `engines.install` (`LAMP_APP_UPDATE=on` and
//! `LAMP_APP_ENGINES=on`, `crate::update`). lamp-cloud checks the name and
//! hands it to lamp-tasks' route for the app ([`crate::harness_sign_in::SOCKET`])
//! as one JSON line, `{"op":"app_engine_default","engine":…}`; lamp-tasks
//! decides and keeps the choice, and answers `{"default":…,
//! "chosen_in_app":true}` or a refusal's code (`engine_unknown`,
//! `engine_not_installed`), passed on as the call's error.
//!
//! `update.status`'s `engines` carries the default as lamp-tasks says it
//! (`app_engine_default_state`): `default` and `default_chosen_in_app`,
//! left out when lamp-tasks cannot be reached within its short wait or
//! has none, so the status never fails for it.
use serde_json::{Value, json};
use std::path::Path;

/// The call's kind.
pub const KIND: &str = "engines.default";

/// The agents the app may choose as the default.
pub const ENGINES: [&str; 2] = ["hermes", "openclaw"];

/// Refused: no such agent (here, before lamp-tasks is asked, or by it).
pub const ENGINE_UNKNOWN: &str = "engine_unknown";
/// Refused by lamp-tasks: the agent is not installed on the lamp.
pub const ENGINE_NOT_INSTALLED: &str = "engine_not_installed";

/// `engines.default` with the app's `data`, asked of lamp-tasks at
/// `socket`: `{"default":…,"chosen_in_app":…}`, or the refusal's code
/// ([`ENGINE_UNKNOWN`], [`ENGINE_NOT_INSTALLED`], `unreachable`, …).
pub fn choose(socket: &Path, data: &Value) -> Result<Value, String> {
    let engine = data["engine"]
        .as_str()
        .filter(|engine| ENGINES.contains(engine))
        .ok_or(ENGINE_UNKNOWN)?;
    let said = crate::harness_sign_in::exchange(
        socket,
        &json!({"op": "app_engine_default", "engine": engine}),
    )?;
    Ok(json!({
        "default": said.get("default").cloned().unwrap_or(Value::Null),
        "chosen_in_app": said["chosen_in_app"].as_bool().unwrap_or(false),
    }))
}

/// The default as lamp-tasks at `socket` says it, put in `status`'s
/// `engines` (`default`, `default_chosen_in_app`); nothing when there is no
/// `engines`, lamp-tasks is not reached, or it has no default.
pub fn add_to(status: &mut Value, socket: &Path) {
    let Some(engines) = status.get_mut("engines").and_then(Value::as_object_mut) else {
        return;
    };
    let Ok(said) =
        crate::harness_sign_in::exchange(socket, &json!({"op": "app_engine_default_state"}))
    else {
        return;
    };
    let Some(default) = said["default"].as_str().filter(|e| ENGINES.contains(e)) else {
        return;
    };
    engines.insert("default".into(), default.into());
    if let Some(chosen) = said["chosen_in_app"].as_bool() {
        engines.insert("default_chosen_in_app".into(), chosen.into());
    }
}

/// What the app shows for a refusal's `error`.
pub fn message_for(error: &str) -> &'static str {
    match error {
        ENGINE_UNKNOWN => "That isn't an agent this lamp knows.",
        ENGINE_NOT_INSTALLED => "That engine isn't installed on the lamp.",
        "engine_default_unsaved" => "The lamp could not keep that choice. Try again.",
        error if error.starts_with("not on this lamp: ") => {
            "Choosing the lamp's agent from the app is not turned on for this lamp."
        }
        _ => "The lamp could not be reached just now. Try again in a moment.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness_sign_in::tests::{folder, tasks};

    /// #6492: the engine goes to lamp-tasks as `app_engine_default`; its
    /// answer comes back; an unknown name is refused before lamp-tasks is
    /// asked; lamp-tasks' refusal passes through; no socket, unreachable.
    #[test]
    fn the_default_is_handed_to_lamp_tasks_and_its_refusals_passed_on() {
        let dir = folder("ed");
        let asked = tasks(
            &dir,
            vec![
                r#"{"ok":true,"default":"hermes","chosen_in_app":true}"#,
                r#"{"ok":false,"error":"engine_not_installed"}"#,
            ],
        );
        let socket = dir.join("app.sock");
        assert_eq!(
            choose(&socket, &json!({"engine": "hermes"})),
            Ok(json!({"default": "hermes", "chosen_in_app": true}))
        );
        assert_eq!(
            serde_json::from_str::<Value>(&asked.recv().unwrap()).unwrap(),
            json!({"op": "app_engine_default", "engine": "hermes"})
        );
        for data in [json!({"engine": "claude"}), json!({}), json!({"engine": 1})] {
            assert_eq!(
                choose(&socket, &data),
                Err(ENGINE_UNKNOWN.to_owned()),
                "{data}"
            );
        }
        assert_eq!(
            choose(&socket, &json!({"engine": "openclaw"})),
            Err(ENGINE_NOT_INSTALLED.to_owned())
        );
        assert_eq!(
            serde_json::from_str::<Value>(&asked.recv().unwrap()).unwrap(),
            json!({"op": "app_engine_default", "engine": "openclaw"})
        );
        assert!(asked.try_recv().is_err(), "an unknown name was forwarded");
        assert_eq!(
            message_for(ENGINE_NOT_INSTALLED),
            "That engine isn't installed on the lamp."
        );
        assert_eq!(
            choose(&dir.join("none.sock"), &json!({"engine": "hermes"})),
            Err(crate::harness_sign_in::UNREACHABLE.to_owned())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #6492: the default is put in `engines` when lamp-tasks says one,
    /// left out when it says none or cannot be reached, and never added
    /// where there is no `engines`.
    #[test]
    fn the_default_joins_the_engines_only_when_lamp_tasks_says_one() {
        let dir = folder("eds");
        let asked = tasks(
            &dir,
            vec![
                r#"{"ok":true,"default":"openclaw","chosen_in_app":false}"#,
                r#"{"ok":true,"default":null,"chosen_in_app":false}"#,
            ],
        );
        let socket = dir.join("app.sock");
        let engines = || json!({"engines": {"hermes": {"state": "installed"}}});
        let mut status = engines();
        add_to(&mut status, &socket);
        assert_eq!(status["engines"]["default"], "openclaw");
        assert_eq!(status["engines"]["default_chosen_in_app"], false);
        assert_eq!(
            serde_json::from_str::<Value>(&asked.recv().unwrap()).unwrap(),
            json!({"op": "app_engine_default_state"})
        );
        let mut none = engines();
        add_to(&mut none, &socket);
        assert_eq!(none, engines());
        let mut unreached = engines();
        add_to(&mut unreached, &dir.join("none.sock"));
        assert_eq!(unreached, engines());
        let mut without = json!({"now": "idle"});
        add_to(&mut without, &socket);
        assert_eq!(without, json!({"now": "idle"}));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
