//! The owner's yes, as lamp-control keeps it. lamp-cloud writes it for a tap
//! in the app and the voice for a spoken yes; the helper only ever USES IT
//! UP, once, by asking lamp-control on its admin socket. The key is what the
//! owner said yes to: the operation, the app, the version, the SHA-256 of
//! the package's bytes, and for an update the grants it newly asks. A package
//! changed after the yes has another SHA-256 and so installs nothing.
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Install,
    Update,
    Remove,
}

impl Op {
    pub fn word(self) -> &'static str {
        match self {
            Op::Install => "install",
            Op::Update => "update",
            Op::Remove => "remove",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YesKey {
    pub op: Op,
    pub name: String,
    pub version: String,
    /// 64 lowercase hex characters: the whole package's bytes.
    pub sha256: String,
    /// An install: every grant. An update: only the grants it newly asks
    /// (none if it asks no more).
    pub grants: Vec<String>,
    /// An install: the manifest's `net`. An update: only if it newly turns
    /// the network on. A remove: false.
    pub net: bool,
    /// The same for the manifest's `logs`.
    pub logs: bool,
}

pub trait Yes {
    /// Uses the yes up (once) or says why not.
    fn take(&mut self, key: &YesKey) -> Result<(), String>;
}

/// The one line sent to lamp-control's admin socket (no newline).
pub fn request_line(key: &YesKey) -> String {
    let mut grants = key.grants.clone();
    grants.sort();
    grants.dedup();
    json!({"app_yes_take": {
        "op": key.op.word(),
        "name": key.name,
        "version": key.version,
        "sha256": key.sha256,
        "grants": grants,
        "net": key.net,
        "logs": key.logs,
    }})
    .to_string()
}

/// lamp-control's answer: exactly `{"ok":true}` is a yes; `{"refused":WHY}` is
/// a no with its reason; anything else is a no.
pub fn parse_reply(line: &str) -> Result<(), String> {
    let value: Value = serde_json::from_str(line.trim()).map_err(|_| "not an answer".to_owned())?;
    let object = value.as_object().ok_or("not an answer")?;
    if object.len() == 1 && object.get("ok") == Some(&Value::Bool(true)) {
        return Ok(());
    }
    if object.len() == 1
        && let Some(Value::String(why)) = object.get("refused")
    {
        return Err(why.clone());
    }
    Err("not a yes".into())
}
