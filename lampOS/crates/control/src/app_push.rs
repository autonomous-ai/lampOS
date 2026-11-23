//! An app pushed from the owner's own paired computer (docs/apps.md, "An app
//! from your own computer"; the first builder's yes on #7570, #8351): the
//! door's own check of the whole package before the apps' helper is asked.
//!
//! - **The consent is the pairing.** No yes is asked at the lamp or in the
//!   app (the owner's 18 Nov rule: installs are not asked about): only a
//!   computer this lamp's owner paired, holding `skills`, reaches `app.push`
//!   (an unpaired one never says hello), and the app may ask only what that
//!   pairing grants, as a pushed skill may.
//! - **Unsigned, on this path only.** The package is read with every check a
//!   Store package has but the signature, and it must carry none
//!   (`lamp_skills::app_package::read_pushed_app_package_bytes`); the apps'
//!   helper reads it the same way again, and takes that verb from
//!   lamp-control's cgroup alone, never from lamp-cloud.
use crate::protocol::{Error, ErrorCode, Grant};
use lamp_skills::app_package::{REMEMBER_GRANT, read_pushed_app_package_bytes};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// The door's switch for app push: the apps' own, `LAMP_STORE_APPS=on`.
pub use crate::app_yes::{OFF, on};

/// What a lamp with no apps' helper given (`--apps-manage-socket`) says.
pub const NOT_HERE: &str = "app push is not on this lamp";

/// The package `bytes`, pushed as `name` by a computer holding `grants`,
/// checked: read as a pushed app, named as the push said, asking only what
/// the pairing grants (`remember`, an app's own grant, is never a
/// computer's). What the door tells of it: its name, title, version,
/// SHA-256, grants, `net` and `logs`.
pub fn checked(
    bytes: &[u8],
    sha256: &str,
    name: &str,
    grants: &BTreeSet<Grant>,
) -> Result<Value, Error> {
    let package =
        read_pushed_app_package_bytes(bytes).map_err(|why| Error::new(ErrorCode::Refused, why))?;
    let manifest = &package.manifest;
    if manifest.name != name {
        return Err(Error::new(
            ErrorCode::BadRequest,
            format!(
                "name: the push says {name:?} and its lamp-skill.json {:?}",
                manifest.name
            ),
        ));
    }
    let beyond: Vec<&str> = manifest
        .grants
        .iter()
        .map(String::as_str)
        .filter(|asked| {
            *asked == REMEMBER_GRANT
                || Grant::from_wire(asked).is_none_or(|grant| !grants.contains(&grant))
        })
        .collect();
    if !beyond.is_empty() {
        return Err(Error::new(
            ErrorCode::NoGrant,
            format!("not granted: {}", beyond.join(", ")),
        ));
    }
    Ok(json!({
        "name": manifest.name,
        "title": manifest.title,
        "version": manifest.version,
        "sha256": sha256,
        "grants": manifest.grants,
        "net": manifest.net,
        "logs": manifest.logs,
    }))
}

/// What the lamp says once a pushed app is on: one line, with its undo.
pub fn said_on(title: &str) -> String {
    format!("{title} is on, from your computer. To undo, remove it in the app.")
}
