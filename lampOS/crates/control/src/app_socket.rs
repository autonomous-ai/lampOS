//! An installed app's socket (#7415 piece 3b, `docs/apps.md`, "The app
//! socket"): the door's WebSocket `/v1` on a unix socket with no TLS, for the
//! apps `lamp-apps-manage` installed. The peer is known by its cgroup
//! (`SO_PEERCRED`'s pid, `/proc/<pid>/cgroup`): the unit
//! `lampapp@<name>.service` in `lampos-apps.slice` and nothing below it, never
//! a name the app says. Its grants are its installed manifest's; the owner's
//! pairing has no part in it. Behind `LAMP_STORE_APPS` ([`crate::app_yes::on`]).
//!
//! The ops only an app has (`app.took`, `app.settings.get`, `app.panel.set`)
//! are answered here, before the door parses a frame, so no other socket can
//! send them and the door's `Op` is the same for all.
use crate::app_yes::{self, Live, Place};
use crate::protocol::{Error, ErrorCode, Grant};
use ring::digest::{SHA256, digest};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Instant;

/// Where it listens unless `--app-socket` or `LAMP_APP_SOCKET` says another place.
pub const SOCKET: &str = "/run/lampos-apps/apps.sock";
pub const ENVIRONMENT: &str = "LAMP_APP_SOCKET";
/// The slice every app's unit is in, and the unit's name begins and ends so.
pub const SLICE: &str = "/lampos.slice/lampos-apps.slice/";
pub const UNIT_PREFIX: &str = "lampapp@";
/// At most this many connections an app, and at once in all.
pub const MOST_EACH: usize = 4;
pub const MOST: usize = 32;
/// A panel: its title, its lines, and each line's, in characters.
pub const PANEL_TITLE: usize = 40;
pub const PANEL_LINES: usize = 6;
pub const PANEL_LINE: usize = 80;
/// What a connection is told when the lamp's apps are off.
pub const OFF: &str = "apps are off on this lamp";

/// The app a cgroup's text says: `lampapp@<name>.service` directly in
/// [`SLICE`], with nothing under it, and `<name>` an app's name.
pub fn app_of_cgroup(cgroup: &str) -> Option<String> {
    cgroup.lines().find_map(|line| {
        let mut fields = line.splitn(3, ':');
        let (_, controllers, path) = (fields.next()?, fields.next()?, fields.next()?);
        if !(controllers.is_empty() || controllers == "name=systemd") {
            return None;
        }
        let unit = path.strip_prefix(SLICE)?;
        let name = unit
            .strip_prefix(UNIT_PREFIX)?
            .strip_suffix(".service")?
            .to_owned();
        (!unit.contains('/') && lamp_skills::app_package::app_name_ok(&name)).then_some(name)
    })
}

/// The app with `pid`, by its cgroup.
pub fn app_of(pid: Option<i32>) -> Option<String> {
    let cgroup =
        std::fs::read_to_string(format!("/proc/{}/cgroup", pid.filter(|pid| *pid > 0)?)).ok()?;
    app_of_cgroup(&cgroup)
}

/// The client id an app is: `c_` and 16 hex of its name's digest.
pub fn app_client(name: &str) -> String {
    let sum = digest(&SHA256, format!("app:{name}").as_bytes());
    let hex: String = sum.as_ref()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("c_{hex}")
}

/// An installed app as the socket knows it.
#[derive(Clone, Debug, PartialEq)]
pub struct AppPeer {
    pub name: String,
    pub version: String,
    pub client: String,
    pub grants: BTreeSet<Grant>,
}

/// The installed app `name`: its manifest's version and grants. `Err` is why
/// the connection is refused (journaled).
pub fn peer_for(apps: &Path, name: &str) -> Result<AppPeer, String> {
    let Some(manifest) = app_yes::installed(apps, name) else {
        return Err("not an installed app".to_owned());
    };
    let mut grants = BTreeSet::new();
    for grant in &manifest.grants {
        let grant: Grant = serde_json::from_value(json!(grant))
            .map_err(|_| format!("a grant this lamp does not know: {grant}"))?;
        if grant.is_service() {
            return Err(format!("{grant:?} is a service's, never an app's"));
        }
        grants.insert(grant);
    }
    Ok(AppPeer {
        name: name.to_owned(),
        version: manifest.version,
        client: app_client(name),
        grants,
    })
}

/// Binds the socket at `path`, mode 0666 in a folder of 0755: the guard is
/// the peer's cgroup, not the file's mode. An old socket of ours is removed.
pub fn bind(path: &Path) -> std::io::Result<tokio::net::UnixListener> {
    use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
    if let Some(parent) = path.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o755)
            .create(parent)?;
    }
    let _ = std::fs::remove_file(path);
    let listener = tokio::net::UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o666))?;
    Ok(listener)
}

/// A frame's `id`, as the door reads it.
fn id_of(frame: &Map<String, Value>) -> Option<u64> {
    frame.get("id").and_then(Value::as_u64)
}

fn refuse(re: u64, code: ErrorCode, why: &str) -> Value {
    crate::protocol::refused(Some(re), &Error::new(code, why))
}

/// Whether `frame` is one of the ops only an app has.
pub fn is_app_op(frame: &Map<String, Value>) -> bool {
    matches!(
        frame.get("op").and_then(Value::as_str),
        Some("app.took" | "app.settings.get" | "app.panel.set")
    )
}

/// A panel's text as the owner's phone shows it: no control character.
fn text_ok(text: &str, most: usize) -> bool {
    let count = text.chars().count();
    (1..=most).contains(&count) && !text.chars().any(char::is_control)
}

/// The answer to `frame` (one of [`is_app_op`]) from app `name`. A frame that
/// is no object or names no id is the door's to refuse, so this is `None`.
pub fn answer(
    live: &mut Live,
    place: &Place<'_>,
    name: &str,
    frame: &Map<String, Value>,
    on: bool,
    now: Instant,
) -> Option<Value> {
    let re = id_of(frame)?;
    let op = frame.get("op")?.as_str()?;
    if !on {
        return Some(refuse(re, ErrorCode::Off, OFF));
    }
    let only = |allowed: &[&str]| {
        frame
            .keys()
            .all(|key| key == "id" || key == "op" || allowed.contains(&key.as_str()))
    };
    Some(match op {
        "app.took" => {
            let turn = frame.get("turn").and_then(Value::as_str);
            match turn {
                Some(turn)
                    if only(&["turn"])
                        && (1..=64).contains(&turn.len())
                        && !turn.chars().any(char::is_control) =>
                {
                    crate::protocol::ok(re, json!({}))
                }
                _ => refuse(
                    re,
                    ErrorCode::BadRequest,
                    "turn: the id of an app.trigger, 1 to 64 characters",
                ),
            }
        }
        "app.settings.get" => {
            if !only(&[]) {
                return Some(refuse(
                    re,
                    ErrorCode::BadRequest,
                    "app.settings.get takes no fields",
                ));
            }
            match app_yes::settings_of(place, name) {
                Some(settings) => crate::protocol::ok(re, json!({"settings": settings})),
                None => refuse(re, ErrorCode::Refused, "this app is not installed"),
            }
        }
        _ => {
            let title = frame.get("title").and_then(Value::as_str);
            let lines: Option<Vec<&str>> = frame
                .get("lines")
                .and_then(Value::as_array)
                .and_then(|lines| lines.iter().map(Value::as_str).collect());
            let (Some(title), Some(lines)) = (title, lines) else {
                return Some(refuse(
                    re,
                    ErrorCode::BadRequest,
                    "title is text and lines a list of text",
                ));
            };
            if !only(&["title", "lines"])
                || !text_ok(title, PANEL_TITLE)
                || lines.len() > PANEL_LINES
                || !lines.iter().all(|line| text_ok(line, PANEL_LINE))
            {
                return Some(refuse(
                    re,
                    ErrorCode::BadRequest,
                    "title 1 to 40 characters, at most 6 lines of 1 to 80, no control characters",
                ));
            }
            let lines: Vec<String> = lines.into_iter().map(str::to_owned).collect();
            match live.set_panel(name, title, &lines, now) {
                Ok(rev) => crate::protocol::ok(re, json!({"rev": rev})),
                Err(wait) => {
                    let mut error = Error::new(ErrorCode::Rate, "one panel a minute");
                    error.retry_ms = Some(wait.as_millis().max(1) as u64);
                    crate::protocol::refused(Some(re), &error)
                }
            }
        }
    })
}

/// An app's `phrases.set` (`{"id", "op": "phrases.set", "phrases": [text…]}`,
/// replace-whole) as the door's own op: a `subscribe` to `phrase.heard` with
/// the phrases, or, for none, an `unsubscribe` of it. The text to hand the
/// door and how many phrases were set; a frame that is no `phrases.set`, or a
/// malformed one, is handed on as it came (the door refuses it in its own
/// words) with `None`.
pub fn phrases_set_as_subscribe(text: &str) -> (String, Option<usize>) {
    let as_is = || (text.to_owned(), None);
    let Ok(Value::Object(frame)) = serde_json::from_str::<Value>(text) else {
        return as_is();
    };
    if frame.get("op").and_then(Value::as_str) != Some("phrases.set") {
        return as_is();
    }
    let (Some(id), Some(Value::Array(phrases))) = (frame.get("id"), frame.get("phrases")) else {
        return as_is();
    };
    if frame.len() != 3 || phrases.iter().any(|phrase| !phrase.is_string()) {
        return as_is();
    }
    let door = if phrases.is_empty() {
        json!({"id": id, "op": "unsubscribe", "events": ["phrase.heard"]})
    } else {
        json!({"id": id, "op": "subscribe", "events": ["phrase.heard"], "phrases": phrases})
    };
    (door.to_string(), Some(phrases.len()))
}

/// The door's answer to a [`phrases_set_as_subscribe`]d frame, as the app's:
/// `{"ok": true, "phrases": N}` for a success, a refusal as it is.
pub fn phrases_reply(reply: &mut Value, count: usize) {
    if reply.get("ok") == Some(&json!(true))
        && let Some(fields) = reply.as_object_mut()
    {
        fields.remove("events");
        fields.insert("phrases".to_owned(), json!(count));
    }
}

/// The event an app is told when the owner changed settings.
pub fn settings_event(changed: &[String], settings: &Map<String, Value>) -> Value {
    json!({"ev": "app.settings", "changed": changed, "settings": settings})
}

#[cfg(test)]
mod tests;
