//! One request line to the helper, one answer line back. The line is parsed
//! strictly (an unknown field or verb is refused), every name is checked by
//! the verbs themselves, and the only path in it is an install's staged file, which [`crate::staging`] holds to a plain name directly in the staging folder.
use crate::{
    effects::Effects,
    install::{Pushed, install, install_pushed},
    layout::Layout,
    manage::{
        Held, Listed, grants_pushed_by, list, list_pushed_by, off_pushed_by, remove_pushed,
        remove_with_yes, start, stop,
    },
    package::PackageReader,
    remove::Companions,
    staging::read_staged,
    yes::Yes,
};
use serde::Deserialize;
use serde_json::{Value, json};

/// The most bytes of a request.
pub const MOST_LINE: u64 = 4_096;
/// What a switch that is off says.
pub const OFF: &str = "apps are off on this lamp";

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    List {},
    Start(String),
    Stop(String),
    Remove(String),
    Install { path: String },
}

/// What lamp-control alone may ask, for a paired computer's own pushed apps
/// (docs/apps.md, "An app from your own computer"): it holds the pairing and
/// its grants, and names the computer. No verb here takes a Store package,
/// and none of [`Request`]'s is taken from lamp-control.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DoorRequest {
    InstallPushed {
        path: String,
        client: String,
        #[serde(default)]
        client_name: Option<String>,
    },
    ListPushedBy(String),
    RemovePushed {
        client: String,
        name: String,
    },
    /// A revoked computer's pushed apps stopped (`manage::off_pushed_by`).
    OffPushedBy(String),
    /// A computer's pushed apps held to its grants now
    /// (`manage::grants_pushed_by`).
    GrantsPushedBy {
        client: String,
        grants: Vec<String>,
    },
}

pub fn parse_door(line: &str) -> Result<DoorRequest, String> {
    serde_json::from_str(line.trim()).map_err(|_| {
        "lamp-control's request is one line: {\"install_pushed\":{\"path\",\"client\",\"client_name\"}}, \
         {\"list_pushed_by\":CLIENT}, {\"remove_pushed\":{\"client\",\"name\"}}, \
         {\"off_pushed_by\":CLIENT} or {\"grants_pushed_by\":{\"client\",\"grants\"}}"
            .to_owned()
    })
}

/// The most bytes of a pairing's id or name, as lamp-control keeps them.
const MOST_CLIENT: usize = 64;

fn client_ok(client: &str) -> Result<(), String> {
    if client.is_empty() || client.len() > MOST_CLIENT || client.chars().any(char::is_control) {
        return Err("not a computer's pairing id".to_owned());
    }
    Ok(())
}

fn listed_json(app: &Listed) -> Value {
    let mut said = json!({"name": app.name, "version": app.version, "grants": app.grants,
                          "net": app.net, "stopped": app.stopped});
    if let Some(pushed) = &app.pushed {
        said["sha256"] = json!(app.sha256);
        said["title"] = json!(app.title);
        said["logs"] = json!(app.logs);
        said["pushed"] = json!(pushed);
        said["pushed_by"] = pushed_by(pushed);
    }
    said
}

/// The computer an app was pushed from, as a pushed skill's row says it
/// (lamp-cloud's `skills::PushedBy`): `computer`, its name at pairing (at
/// most 64 bytes, no control characters), and `client`, its pairing id. From
/// the record, so the phone's row and the door's answers agree.
pub fn pushed_by(pushed: &Pushed) -> Value {
    let computer: String = pushed
        .client_name
        .as_deref()
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .scan(0, |bytes, c| {
            *bytes += c.len_utf8();
            (*bytes <= MOST_CLIENT).then_some(c)
        })
        .collect();
    json!({"computer": computer, "client": pushed.client})
}

fn held_json(held: &Held) -> Value {
    json!({"stopped": held.stopped, "not_stopped": held.not_stopped})
}

/// lamp-control's answer line, as [`answer`]'s: `{"ok":...}` or
/// `{"refused":WHY}`; with the switch off everything is refused.
pub fn answer_door(on: bool, line: &str, parts: &mut Parts) -> Value {
    let request = match parse_door(line) {
        Ok(request) => request,
        Err(why) => return json!({"refused": why}),
    };
    if !on {
        return json!({"refused": OFF});
    }
    let done = match request {
        DoorRequest::ListPushedBy(client) => {
            if let Err(why) = client_ok(&client) {
                return json!({"refused": why});
            }
            let apps: Vec<Value> = list_pushed_by(parts.layout, &client)
                .iter()
                .map(listed_json)
                .collect();
            return json!({"ok": {"apps": apps}});
        }
        DoorRequest::OffPushedBy(client) => {
            client_ok(&client).map(|()| held_json(&off_pushed_by(parts.layout, parts.fx, &client)))
        }
        DoorRequest::GrantsPushedBy { client, grants } => client_ok(&client)
            .map(|()| held_json(&grants_pushed_by(parts.layout, parts.fx, &client, &grants))),
        DoorRequest::RemovePushed { client, name } => client_ok(&client)
            .and_then(|()| remove_pushed(parts.layout, parts.fx, parts.skills, &client, &name))
            .map(|_| json!("removed")),
        DoorRequest::InstallPushed {
            path,
            client,
            client_name,
        } => client_ok(&client)
            .and_then(|()| match &client_name {
                Some(name) => client_ok(name),
                None => Ok(()),
            })
            .and_then(|()| read_staged(&parts.layout.staging, &path, parts.staged_owner))
            .and_then(|bytes| {
                let pushed = Pushed {
                    client,
                    client_name,
                };
                install_pushed(
                    parts.layout,
                    parts.fx,
                    parts.skills,
                    parts.reader,
                    &bytes,
                    parts.build,
                    &pushed,
                )
                .map(|done| {
                    json!({"name": done.name, "version": done.version,
                           "updated": done.updated, "started": done.started,
                           "pushed_by": pushed_by(&pushed)})
                })
            }),
    };
    match done {
        Ok(said) => json!({"ok": said}),
        Err(why) => json!({"refused": why}),
    }
}

pub fn parse(line: &str) -> Result<Request, String> {
    serde_json::from_str(line.trim()).map_err(|_| {
        "a request is one line: {\"list\":{}}, {\"start\":NAME}, {\"stop\":NAME}, \
         {\"remove\":NAME} or {\"install\":{\"path\":ABSOLUTE_PATH}}"
            .to_owned()
    })
}

/// Everything a request may use, so a test can hand it doubles.
pub struct Parts<'a> {
    pub layout: &'a Layout,
    pub fx: &'a mut dyn Effects,
    pub skills: &'a mut dyn Companions,
    pub yes: &'a mut dyn Yes,
    pub reader: &'a dyn PackageReader,
    /// The user that may own a staged package (lamp-cloud's).
    pub staged_owner: u32,
    /// This lamp's build sequence, if known.
    pub build: Option<u64>,
}

/// The answer line: `{"ok":...}` or `{"refused":WHY}`. With the switch off
/// everything but the list is refused.
pub fn answer(on: bool, line: &str, parts: &mut Parts) -> Value {
    let request = match parse(line) {
        Ok(request) => request,
        Err(why) => return json!({"refused": why}),
    };
    if !on && request != (Request::List {}) {
        return json!({"refused": OFF});
    }
    let done = match request {
        Request::List {} => {
            let apps: Vec<Value> = list(parts.layout).iter().map(listed_json).collect();
            return json!({"ok": {"apps": apps}});
        }
        Request::Start(name) => start(parts.layout, parts.fx, &name).map(|()| "started"),
        Request::Stop(name) => stop(parts.layout, parts.fx, &name).map(|()| "stopped"),
        Request::Remove(name) => {
            remove_with_yes(parts.layout, parts.fx, parts.skills, parts.yes, &name)
                .map(|_| "removed")
        }
        Request::Install { path } => read_staged(&parts.layout.staging, &path, parts.staged_owner)
            .and_then(|bytes| {
                install(
                    parts.layout,
                    parts.fx,
                    parts.skills,
                    parts.yes,
                    parts.reader,
                    &bytes,
                    parts.build,
                )
                .map(|done| if done.updated { "updated" } else { "installed" })
            }),
    };
    match done {
        Ok(word) => json!({"ok": word}),
        Err(why) => json!({"refused": why}),
    }
}

/// This lamp's build sequence from `lamp-update`'s `state.json`
/// (`{"installed":{"sequence":N,"commit":..}}`), as lamp-cloud reads it; no
/// other shape is taken.
pub fn build_sequence(text: &str) -> Option<u64> {
    serde_json::from_str::<Value>(text.trim())
        .ok()?
        .get("installed")?
        .get("sequence")?
        .as_u64()
}
