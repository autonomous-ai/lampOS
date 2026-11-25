//! What the helper needs to know of an app's manifest, behind a small trait
//! so this crate parses nothing: nine's `AppManifest` (piece 1) implements
//! [`AppFacts`] when it lands, and until then the tests use a stub.
use crate::{entry::entry_ok, name};

/// The door's plain grants (`crates/control/src/protocol.rs`, `PLAIN_WIRE`),
/// copied because lamp-control is a server, not a dependency; a test reads
/// that file and fails on a drift. `net` and `logs` are top-level booleans of
/// the manifest, not grants, and a service grant is not allowed in v1.
pub const DOOR_GRANTS: &[&str] = &[
    "body",
    "light",
    "speak",
    "look",
    "hear",
    "sound",
    "skills",
    "air",
    "device",
    "camera",
    "microphone",
    "speaker",
    "notes",
    "speech",
    "volume",
];

/// An app's own grants beyond the door's, which no computer is paired with:
/// `remember` (`apps.remember`, `lamp_skills::app_package::REMEMBER_GRANT`).
pub const APP_GRANTS: &[&str] = &["remember"];

/// The most characters of an app's title (it is spoken: "stop the <title> app").
pub const MOST_TITLE: usize = 40;

pub trait AppFacts {
    fn name(&self) -> &str;
    fn title(&self) -> &str;
    fn version(&self) -> &str;
    /// The lamp build sequence the app needs.
    fn min_build(&self) -> u64;
    /// The manifest's top-level `net`.
    fn net(&self) -> bool;
    fn grants(&self) -> &[String];
    fn entry(&self) -> &str;
}

/// `MAJOR.MINOR.PATCH`, each a number of at most nine digits.
fn version_ok(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 9 && p.bytes().all(|b| b.is_ascii_digit()))
}

/// The checks the helper makes of what a manifest says, before it writes
/// anything. `build` is this lamp's build sequence; with none (`lamp.build`,
/// #7528, not in yet) the check refuses, it never passes.
pub fn check(app: &dyn AppFacts, build: Option<u64>) -> Result<(), String> {
    if !name::app_name_ok(app.name()) {
        return Err(format!("not an app name: {:?}", app.name()));
    }
    if !entry_ok(app.entry()) {
        return Err(format!("not an entry: {:?}", app.entry()));
    }
    if !version_ok(app.version()) {
        return Err(format!("not a version: {:?}", app.version()));
    }
    let title = app.title();
    if title.is_empty() || title.chars().count() > MOST_TITLE || title.chars().any(char::is_control)
    {
        return Err("the title is empty, too long or not plain text".into());
    }
    let mut seen: Vec<&str> = Vec::new();
    for grant in app.grants() {
        if !DOOR_GRANTS.contains(&grant.as_str()) && !APP_GRANTS.contains(&grant.as_str()) {
            return Err(format!("not a grant an app may ask: {grant:?}"));
        }
        if seen.contains(&grant.as_str()) {
            return Err(format!("grant {grant:?} twice"));
        }
        seen.push(grant);
    }
    match build {
        None => Err("this lamp's build is not known: no app is installed".into()),
        Some(have) if have < app.min_build() => Err(format!(
            "needs build {} and this lamp has {have}",
            app.min_build()
        )),
        Some(_) => Ok(()),
    }
}

/// The unit instance that runs the app: the one with a way out only for a
/// manifest with `net`.
pub fn unit_of(app: &dyn AppFacts) -> String {
    if app.net() {
        name::net_unit(app.name())
    } else {
        name::plain_unit(app.name())
    }
}
