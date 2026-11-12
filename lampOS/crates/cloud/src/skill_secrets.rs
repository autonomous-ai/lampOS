//! An uploaded skill's keys, given in the owner's app
//! (`LAMP_SKILL_SECRETS`, off unless `on`, set in no unit; the first
//! builder's ruling, 12 Nov 19:44 on #6059). Three kinds, over the lamp's
//! own account channel as `skills.upload` and `skills.list`, each answered
//! inline:
//!
//! - `skills.secrets` `{"skill"}`: `{"skill","needed","set"}`, names only.
//! - `skills.secret_set` `{"skill","name","value"}`: the same, after.
//! - `skills.secret_unset` `{"skill","name"}`: the same, after.
//!
//! `lamp-cloud` keeps nothing: each goes to root's `lamp-services` on its
//! admin socket ([`lamp_services::skill_secrets`]), which keeps the values
//! beside the services' tokens. A value is in that one line and nowhere
//! else: never a reply, an error, a journal line or a `Debug`. A skill the
//! app removes (`skills.remove`) has its keys dropped there too
//! ([`dropped`]).
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// `on`, beside `LAMP_CLOUD`, takes [`KINDS`].
pub const ENVIRONMENT: &str = lamp_services::skill_secrets::ENVIRONMENT;
/// The kinds, in `can`'s order.
pub const KINDS: [&str; 3] = ["skills.secrets", "skills.secret_set", "skills.secret_unset"];
/// How long one is waited on, inline on the broker's loop: a small file
/// read or written.
pub const WAIT: Duration = Duration::from_secs(5);

/// Whether the switch is on: both `LAMP_CLOUD` and [`ENVIRONMENT`] are `on`.
pub fn gates(cloud: Option<&str>, secrets: Option<&str>) -> bool {
    cloud == Some("on") && secrets == Some("on")
}

/// `lamp-services`' admin socket, with the switch; none without it.
pub fn product() -> Option<PathBuf> {
    gates(
        std::env::var("LAMP_CLOUD").ok().as_deref(),
        std::env::var(ENVIRONMENT).ok().as_deref(),
    )
    .then(|| PathBuf::from(crate::services::ADMIN_SOCKET))
}

/// The admin socket's line for the app's `kind` and `data`: its fields
/// passed as they are, never added to, for lamp-services to check whole.
pub fn line(kind: &str, data: &Value) -> Result<Value, String> {
    use lamp_services::skill_secrets as kept;
    let op = match kind {
        "skills.secrets" => kept::LIST_OP,
        "skills.secret_set" => kept::SET_OP,
        "skills.secret_unset" => kept::UNSET_OP,
        _ => return Err(crate::wire::not_here(kind)),
    };
    let data = match data {
        Value::Object(data) => data.clone(),
        Value::Null => Map::new(),
        _ => return Err(kept::BAD_FIELDS.into()),
    };
    if data.contains_key("op") {
        return Err(kept::BAD_FIELDS.into());
    }
    let mut line = data;
    line.insert("op".into(), json!(op));
    Ok(Value::Object(line))
}

/// One of [`KINDS`] answered through `socket`: lamp-services' view, names
/// only, or its refusal's words.
pub fn answer(socket: &Path, kind: &str, data: &Value) -> Result<Value, String> {
    let line = line(kind, data)?;
    let reply = crate::services::ask(socket, &line, WAIT)?;
    let view = &reply["result"];
    let names = |key: &str| -> Vec<Value> {
        view[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|name| name.is_string())
            .cloned()
            .collect()
    };
    Ok(json!({
        "skill": view["skill"],
        "needed": names("needed"),
        "set": names("set"),
    }))
}

/// A skill the app removed: its keys dropped. Its failure is journaled,
/// never the removal's (lamp-services also drops a gone skill's keys at
/// its next ask).
pub fn dropped(socket: &Path, skill: &str) {
    let line = json!({"op": lamp_services::skill_secrets::DROP_OP, "skill": skill});
    if let Err(why) = crate::services::ask(socket, &line, WAIT) {
        eprintln!("lamp-cloud: skill {skill}'s keys not dropped: {why}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;

    /// lamp-services' stand-in: the line it was sent, and `reply`.
    fn admin(reply: Value) -> (tempfile::TempDir, PathBuf, std::thread::JoinHandle<String>) {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("admin.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let served = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            writeln!(&stream, "{reply}").unwrap();
            line
        });
        (dir, socket, served)
    }

    #[test]
    fn the_switch_needs_both() {
        assert!(gates(Some("on"), Some("on")));
        assert!(!gates(Some("on"), None));
        assert!(!gates(None, Some("on")));
        assert!(!gates(Some("on"), Some("yes")));
    }

    #[test]
    fn a_set_goes_to_lamp_services_and_only_names_come_back() {
        let (_dir, socket, served) = admin(json!({"ok": true, "result": {
            "skill": "photo-booth", "needed": ["GEMINI_API_KEY"], "set": ["GEMINI_API_KEY"],
            "value": "test-value-123"}}));
        let told = answer(
            &socket,
            "skills.secret_set",
            &json!({"skill": "photo-booth", "name": "GEMINI_API_KEY", "value": "test-value-123"}),
        )
        .unwrap();
        assert_eq!(
            told,
            json!({"skill": "photo-booth", "needed": ["GEMINI_API_KEY"], "set": ["GEMINI_API_KEY"]})
        );
        let sent: Value = serde_json::from_str(&served.join().unwrap()).unwrap();
        assert_eq!(
            sent,
            json!({"op": "skill_secret_set", "skill": "photo-booth",
                "name": "GEMINI_API_KEY", "value": "test-value-123"})
        );
    }

    #[test]
    fn a_refusal_is_lamp_services_words_and_a_bad_request_never_goes() {
        let (_dir, socket, served) =
            admin(json!({"ok": false, "error": lamp_services::skill_secrets::NOT_NEEDED}));
        assert_eq!(
            answer(
                &socket,
                "skills.secret_unset",
                &json!({"skill": "photo-booth", "name": "OTHER_KEY"})
            ),
            Err(lamp_services::skill_secrets::NOT_NEEDED.to_owned())
        );
        served.join().unwrap();
        // The app polls `skills.secrets` after an upload and reads this one
        // as not there yet: its words reach it exactly.
        let (_dir, socket, served) =
            admin(json!({"ok": false, "error": lamp_services::skill_secrets::NO_SKILL}));
        assert_eq!(
            answer(&socket, "skills.secrets", &json!({"skill": "photo-booth"})),
            Err("no skill of that name is on this lamp".to_owned())
        );
        served.join().unwrap();
        assert_eq!(
            line("skills.secrets", &json!(["photo-booth"])),
            Err(lamp_services::skill_secrets::BAD_FIELDS.to_owned())
        );
        assert_eq!(
            line("skills.secrets", &json!({"skill": "x", "op": "wipe"})),
            Err(lamp_services::skill_secrets::BAD_FIELDS.to_owned())
        );
        assert_eq!(
            answer(
                Path::new("/nonexistent/admin.sock"),
                "skills.secrets",
                &json!({"skill": "x"})
            ),
            Err(crate::services::NOT_RUNNING.to_owned())
        );
    }

    #[test]
    fn a_removed_skills_keys_are_dropped() {
        let (_dir, socket, served) = admin(json!({"ok": true, "result": {"skill": "photo-booth"}}));
        dropped(&socket, "photo-booth");
        let sent: Value = serde_json::from_str(&served.join().unwrap()).unwrap();
        assert_eq!(
            sent,
            json!({"op": "skill_secrets_drop", "skill": "photo-booth"})
        );
    }
}
