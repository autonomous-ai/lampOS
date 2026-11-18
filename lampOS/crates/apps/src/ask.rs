//! One request line to the helper, one answer line back. The line is parsed
//! strictly (an unknown field or verb is refused), every name is checked by
//! the verbs themselves, and the only path in it is an install's staged file, which [`crate::staging`] holds to a plain name directly in the staging folder.
use crate::{
    effects::Effects,
    install::install,
    layout::Layout,
    manage::{list, remove_with_yes, start, stop},
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
            let apps: Vec<Value> = list(parts.layout)
                .into_iter()
                .map(|a| {
                    json!({"name": a.name, "version": a.version, "grants": a.grants,
                           "net": a.net, "stopped": a.stopped})
                })
                .collect();
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
