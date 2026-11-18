//! The owner's yes to an app's install, update or remove, and what lamp-cloud
//! reads and writes of an installed app (#7415 piece 3a, `docs/apps.md`).
//! Behind `LAMP_STORE_APPS` ([`ENVIRONMENT`]), off unless `on`: with it off
//! every op here answers [`OFF`].
//!
//! - **The yes** is kept in memory only, never on disk: a restart of
//!   lamp-control loses it and the person says yes again. It is
//!   [`MOST`] waiting at most; each lives [`LIVES`] on the monotonic clock
//!   and is used once; a new yes for the same op and name replaces the old.
//!   It is the seven values, matched exactly (the grants sorted and
//!   de-duplicated here, whatever order they came in): `{"op", "name",
//!   "version", "sha256", "grants", "net", "logs"}`, with who gave it kept beside it
//!   (`voice` or `app`), set from the writer's cgroup, never from the
//!   request.
//! - **Written** (`app_yes_record`) by the voice (a spoken yes) and lamp-cloud
//!   (the owner's tap in the app, on the lamp's own account channel); taken
//!   (`app_yes_take`) by the apps helper alone, once. Every answer is
//!   `{"ok":true}` or `{"refused":"<why>"}`.
//! - **Read and written for lamp-cloud** (its cgroup alone): `app_state`
//!   (an app's settings values, its latest panel and whether its socket is up)
//!   and `app_settings_set` (values checked against the installed manifest's
//!   settings before any is kept).
//! - The peer is known by its cgroup (`SO_PEERCRED`'s pid, `/proc/<pid>/cgroup`),
//!   as the voice is ([`crate::task_yes::peer_is_voice`]): the voice's unit,
//!   lamp-cloud's, or the apps helper's, a unit of `lamp-apps-manage@` (an
//!   instance per CONNECTION, not per app) in `lampos-parts.slice` and
//!   nothing below it.
use lamp_skills::app_package::{AppManifest, GRANTS, Setting, SettingKind, app_name_ok};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The switch for the whole apps feature: `on` only.
pub const ENVIRONMENT: &str = "LAMP_STORE_APPS";

/// Whether apps are on in this process.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// What every op here is told with the switch off.
pub const OFF: &str = "apps are off on this lamp";

pub const RECORD: &str = "app_yes_record";
pub const TAKE: &str = "app_yes_take";
pub const STATE: &str = "app_state";
pub const SETTINGS_SET: &str = "app_settings_set";

/// The most yeses waiting, and how long one lives.
pub const MOST: usize = 8;
pub const LIVES: Duration = Duration::from_secs(600);

/// `lamp-cloud`'s unit, where the system's manager puts it.
pub const LAMP_CLOUD: &str = "/lampos.slice/lampos-parts.slice/lamp-cloud.service";
/// The slice the apps helper's per-connection unit is in.
pub const HELPER_SLICE: &str = "/lampos.slice/lampos-parts.slice/";
/// The helper's unit name begins and ends so (its instance is a connection's).
pub const HELPER_PREFIX: &str = "lamp-apps-manage@";

/// Where the installed apps are: `<dir>/<name>/current/lamp-skill.json`.
pub const APPS_DIR: &str = "/opt/lampOS/apps";
/// In `--state-dir`: the owner's settings for each app.
pub const SETTINGS_FILE: &str = "app-settings.json";

/// The one panel an app may set in a minute.
pub const PANEL_EVERY: Duration = Duration::from_secs(60);

/// Who is asking, by cgroup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Peer {
    Voice,
    Cloud,
    Helper,
    Other,
}

/// The unified hierarchy's cgroup paths in `/proc/<pid>/cgroup`'s text (and
/// systemd's own on a hybrid one).
fn paths(cgroup: &str) -> impl Iterator<Item = &str> {
    cgroup.lines().filter_map(|line| {
        let mut fields = line.splitn(3, ':');
        let (_, controllers, path) = (fields.next()?, fields.next()?, fields.next()?);
        (controllers.is_empty() || controllers == "name=systemd").then_some(path)
    })
}

/// Whether `path` is the helper's unit: [`HELPER_SLICE`], then exactly one
/// more component, `lamp-apps-manage@<instance>.service`, the instance only
/// `[0-9a-z-]`, and nothing after it.
pub fn is_helper_path(path: &str) -> bool {
    let Some(unit) = path.strip_prefix(HELPER_SLICE) else {
        return false;
    };
    let Some(instance) = unit
        .strip_prefix(HELPER_PREFIX)
        .and_then(|rest| rest.strip_suffix(".service"))
    else {
        return false;
    };
    !instance.is_empty()
        && !unit.contains('/')
        && instance
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase() || b == b'-')
}

/// The peer a cgroup's text says.
pub fn peer_of_cgroup(cgroup: &str) -> Peer {
    if crate::task_yes::from_voice(cgroup) {
        Peer::Voice
    } else if paths(cgroup).any(|path| path == LAMP_CLOUD) {
        Peer::Cloud
    } else if paths(cgroup).any(is_helper_path) {
        Peer::Helper
    } else {
        Peer::Other
    }
}

/// The peer with `pid` (its `SO_PEERCRED`), by its cgroup; never on a system
/// without `/proc`.
pub fn peer_of(pid: Option<i32>) -> Peer {
    pid.filter(|pid| *pid > 0)
        .and_then(|pid| std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok())
        .map_or(Peer::Other, |cgroup| peer_of_cgroup(&cgroup))
}

/// The seven values, matched exactly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub op: String,
    pub name: String,
    pub version: String,
    pub sha256: String,
    pub grants: Vec<String>,
    /// The manifest's `net` and `logs` booleans stand outside the grants, so
    /// they are in the key like grants: an update that turns the internet on
    /// is a different key, never "no new grants" (the FB, #7415).
    pub net: bool,
    pub logs: bool,
}

/// One yes waiting.
#[derive(Clone, Debug)]
struct Waiting {
    key: Key,
    by: &'static str,
    at: Instant,
}

/// The yeses waiting: memory only.
#[derive(Debug, Default)]
pub struct Yeses {
    waiting: Vec<Waiting>,
}

/// The request's seven values, checked. `Err` is the refusal's why.
pub fn key_of(value: &Value) -> Result<Key, String> {
    let Some(object) = value.as_object() else {
        return Err(
            "bad request: the seven values {op, name, version, sha256, grants, net, logs}".into(),
        );
    };
    const FIELDS: [&str; 7] = ["op", "name", "version", "sha256", "grants", "net", "logs"];
    if object.len() != FIELDS.len() || FIELDS.iter().any(|field| !object.contains_key(*field)) {
        return Err("bad request: exactly op, name, version, sha256, grants, net and logs".into());
    }
    let text = |field: &str| object[field].as_str().map(str::to_owned);
    let (Some(op), Some(name), Some(version), Some(sha256)) =
        (text("op"), text("name"), text("version"), text("sha256"))
    else {
        return Err("bad request: op, name, version and sha256 are text".into());
    };
    if !matches!(op.as_str(), "install" | "update" | "remove") {
        return Err("bad request: op is install, update or remove".into());
    }
    if !app_name_ok(&name) {
        return Err("bad request: name is not an app's name".into());
    }
    if !lamp_skills::version_ok(&version) {
        return Err("bad request: version is not MAJOR.MINOR.PATCH".into());
    }
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("bad request: sha256 is 64 lowercase hex".into());
    }
    let (Some(net), Some(logs)) = (object["net"].as_bool(), object["logs"].as_bool()) else {
        return Err("bad request: net and logs are true or false".into());
    };
    let Some(listed) = object["grants"].as_array() else {
        return Err("bad request: grants is a list".into());
    };
    let mut grants = Vec::new();
    for grant in listed {
        let Some(grant) = grant.as_str().filter(|grant| GRANTS.contains(grant)) else {
            return Err("bad request: a grant is not one of the door's".into());
        };
        grants.push(grant.to_owned());
    }
    // Sorted and de-duplicated here, whatever order they came in.
    grants.sort();
    grants.dedup();
    if op == "remove" && (!grants.is_empty() || net || logs) {
        return Err("bad request: a remove's grants are none, net and logs false".into());
    }
    Ok(Key {
        op,
        name,
        version,
        sha256,
        grants,
        net,
        logs,
    })
}

impl Yeses {
    fn sweep(&mut self, now: Instant) {
        self.waiting
            .retain(|yes| now.saturating_duration_since(yes.at) < LIVES);
    }

    /// A yes written by `by` at `now`: the same op and name replace the old;
    /// the oldest goes past [`MOST`].
    pub fn record(&mut self, key: Key, by: &'static str, now: Instant) {
        self.sweep(now);
        self.waiting
            .retain(|yes| !(yes.key.op == key.op && yes.key.name == key.name));
        self.waiting.push(Waiting { key, by, at: now });
        while self.waiting.len() > MOST {
            self.waiting.remove(0);
        }
    }

    /// The yes for exactly `key`, used up: who gave it, or why not.
    pub fn take(&mut self, key: &Key, now: Instant) -> Result<&'static str, &'static str> {
        let had = self.waiting.len();
        self.sweep(now);
        let expired = self.waiting.len() < had;
        let Some(at) = self
            .waiting
            .iter()
            .position(|yes| yes.key.op == key.op && yes.key.name == key.name)
        else {
            return Err(if expired { "expired" } else { "no record" });
        };
        if self.waiting[at].key != *key {
            // A yes for other bytes or grants never matches, and is not spent.
            return Err("key does not match");
        }
        Ok(self.waiting.remove(at).by)
    }

    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }
}

/// The panel an app set last, and when.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Panel {
    pub title: String,
    pub lines: Vec<String>,
    pub rev: u64,
    pub at: Instant,
}

/// What lamp-control holds of the apps in memory: panels and whether their
/// sockets are up. The app socket (piece 3b) writes it.
#[derive(Debug, Default)]
pub struct Live {
    panels: BTreeMap<String, Panel>,
    sockets: BTreeMap<String, usize>,
    revs: BTreeMap<String, u64>,
}

impl Live {
    /// An app's panel set at `now`: its new revision, or how long until it
    /// may set one (one a minute).
    pub fn set_panel(
        &mut self,
        name: &str,
        title: &str,
        lines: &[String],
        now: Instant,
    ) -> Result<u64, Duration> {
        if let Some(last) = self.panels.get(name) {
            let since = now.saturating_duration_since(last.at);
            if since < PANEL_EVERY {
                return Err(PANEL_EVERY - since);
            }
        }
        let rev = self.revs.entry(name.to_owned()).or_insert(0);
        *rev += 1;
        self.panels.insert(
            name.to_owned(),
            Panel {
                title: title.to_owned(),
                lines: lines.to_vec(),
                rev: *rev,
                at: now,
            },
        );
        Ok(*rev)
    }

    /// A connection of `name` opened (`+1`) or closed (`-1`).
    pub fn connection(&mut self, name: &str, opened: bool) {
        let count = self.sockets.entry(name.to_owned()).or_insert(0);
        *count = if opened {
            *count + 1
        } else {
            count.saturating_sub(1)
        };
        // A panel is gone with the app's last connection (its run).
        if *count == 0 {
            self.sockets.remove(name);
            self.forget_run(name);
        }
    }

    /// An app's panel is gone with its run (it stopped).
    pub fn forget_run(&mut self, name: &str) {
        self.panels.remove(name);
        self.revs.remove(name);
    }

    fn socket(&self, name: &str) -> bool {
        self.sockets.get(name).is_some_and(|count| *count > 0)
    }
}

/// An installed app's manifest, from its `current` copy under `apps`.
pub fn installed(apps: &Path, name: &str) -> Option<AppManifest> {
    if !app_name_ok(name) {
        return None;
    }
    let bytes = std::fs::read(apps.join(name).join("current").join("lamp-skill.json")).ok()?;
    let manifest: AppManifest = serde_json::from_slice(&bytes).ok()?;
    (manifest.name == name).then_some(manifest)
}

/// Every installed app's name.
pub fn installed_names(apps: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(apps)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| installed(apps, name).is_some())
        .collect();
    names.sort();
    names
}

/// Whether `value` is one `setting` takes.
pub fn value_ok(setting: &Setting, value: &Value) -> Result<(), String> {
    let at = |why: &str| format!("{}: {why}", setting.key);
    match setting.kind {
        SettingKind::Text => {
            let most = setting.max.unwrap_or(0.0);
            let text = value.as_str().ok_or_else(|| at("text is expected"))?;
            if text.chars().count() as f64 > most || text.chars().any(char::is_control) {
                return Err(at(&format!(
                    "at most {most} characters, no control character"
                )));
            }
        }
        SettingKind::Number => {
            let number = value
                .as_f64()
                .filter(|number| number.is_finite())
                .ok_or_else(|| at("a number is expected"))?;
            let (min, max) = (setting.min.unwrap_or(0.0), setting.max.unwrap_or(0.0));
            if number < min || number > max {
                return Err(at(&format!("from {min} to {max}")));
            }
        }
        SettingKind::Toggle => {
            if !value.is_boolean() {
                return Err(at("true or false is expected"));
            }
        }
        SettingKind::Choice => {
            let choice = value
                .as_str()
                .ok_or_else(|| at("one of the options is expected"))?;
            let known = setting
                .options
                .iter()
                .flatten()
                .any(|option| option.value == choice);
            if !known {
                return Err(at("not one of the options"));
            }
        }
    }
    Ok(())
}

/// The owner's kept values for `name` from `file`, and the manifest's
/// defaults beneath them: every key of the schema, a value each.
pub fn values(manifest: &AppManifest, kept: &Map<String, Value>) -> Map<String, Value> {
    let mut all = Map::new();
    for setting in &manifest.settings {
        let value = kept
            .get(&setting.key)
            .filter(|value| value_ok(setting, value).is_ok())
            .unwrap_or(&setting.default);
        all.insert(setting.key.clone(), value.clone());
    }
    all
}

/// An installed app's settings as the app reads them: every key of its
/// manifest, the owner's value or the default. `None` if it is not installed.
pub fn settings_of(place: &Place<'_>, name: &str) -> Option<Map<String, Value>> {
    let manifest = installed(place.apps, name)?;
    let kept = kept_of(place.state);
    let mine = kept
        .get(name)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    Some(values(&manifest, &mine))
}

fn kept_of(state: Option<&Path>) -> Map<String, Value> {
    state
        .and_then(|dir| std::fs::read(dir.join(SETTINGS_FILE)).ok())
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

fn keep_all(state: &Path, all: &Map<String, Value>) -> Result<(), String> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let target = state.join(SETTINGS_FILE);
    let temporary = state.join(format!(".{SETTINGS_FILE}.new"));
    let _ = std::fs::remove_file(&temporary);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|error| format!("settings could not be kept: {error}"))?;
    file.write_all(Value::Object(all.clone()).to_string().as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("settings could not be kept: {error}"))?;
    std::fs::rename(&temporary, &target)
        .map_err(|error| format!("settings could not be kept: {error}"))
}

/// Where the process keeps what these ops read and write.
pub struct Place<'a> {
    pub apps: &'a Path,
    pub state: Option<&'a Path>,
}

impl Place<'static> {
    pub fn product(state: Option<&Path>) -> Place<'_> {
        Place {
            apps: Path::new(APPS_DIR),
            state,
        }
    }
}

fn refused(why: &str) -> Value {
    json!({"refused": why})
}

/// The answer to `asked` if it is one of these ops, none for any other. With
/// the switch `on` off, [`OFF`].
#[allow(clippy::too_many_arguments)]
pub fn answer(
    yeses: &mut Yeses,
    live: &Live,
    place: &Place<'_>,
    asked: &Map<String, Value>,
    peer: Peer,
    on: bool,
    now: Instant,
) -> Option<Value> {
    let (op, value) = [RECORD, TAKE, STATE, SETTINGS_SET]
        .into_iter()
        .find_map(|op| asked.get(op).map(|value| (op, value)))?;
    if !on {
        return Some(refused(OFF));
    }
    Some(match op {
        RECORD => {
            let by = match peer {
                Peer::Voice => "voice",
                Peer::Cloud => "app",
                _ => return Some(refused("only the voice and the app's channel record a yes")),
            };
            match key_of(value) {
                Ok(key) => {
                    eprintln!(
                        "lamp-control: the owner's yes to {} {} {} kept (by {by}, {} grant(s))",
                        key.op,
                        key.name,
                        key.version,
                        key.grants.len()
                    );
                    yeses.record(key, by, now);
                    json!({"ok": true})
                }
                Err(why) => refused(&why),
            }
        }
        TAKE => {
            if peer != Peer::Helper {
                return Some(refused("not the apps helper"));
            }
            match key_of(value) {
                Ok(key) => match yeses.take(&key, now) {
                    Ok(by) => {
                        eprintln!(
                            "lamp-control: the yes to {} {} {} taken (given by {by})",
                            key.op, key.name, key.version
                        );
                        json!({"ok": true})
                    }
                    Err(why) => {
                        eprintln!(
                            "lamp-control: a take for {} {} refused: {why}",
                            key.op, key.name
                        );
                        refused(why)
                    }
                },
                Err(why) => refused(&why),
            }
        }
        STATE | SETTINGS_SET if peer != Peer::Cloud => refused("not the cloud"),
        STATE => state(live, place, value, now),
        _ => settings_set(place, value),
    })
}

fn state(live: &Live, place: &Place<'_>, value: &Value, _now: Instant) -> Value {
    let Some(object) = value.as_object() else {
        return refused("bad request: {} or {\"name\"}");
    };
    if object.keys().any(|key| key != "name") {
        return refused("bad request: only name");
    }
    let kept = kept_of(place.state);
    let one = |name: &str| -> Option<Value> {
        let manifest = installed(place.apps, name)?;
        let mine = kept
            .get(name)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let panel = live.panels.get(name).map_or(
            Value::Null,
            |panel| json!({"title": panel.title, "lines": panel.lines, "rev": panel.rev}),
        );
        Some(json!({
            "settings": values(&manifest, &mine),
            "panel": panel,
            "socket": live.socket(name),
        }))
    };
    match object.get("name") {
        Some(name) => match name.as_str().and_then(one) {
            Some(app) => app,
            None => refused("no such app"),
        },
        None => {
            let apps: Map<String, Value> = installed_names(place.apps)
                .into_iter()
                .filter_map(|name| one(&name).map(|app| (name, app)))
                .collect();
            json!({"apps": apps})
        }
    }
}

fn settings_set(place: &Place<'_>, value: &Value) -> Value {
    let Some(object) = value.as_object() else {
        return refused("bad request: {name, values}");
    };
    if object.len() != 2 || !object.contains_key("name") || !object.contains_key("values") {
        return refused("bad request: exactly name and values");
    }
    let (Some(name), Some(values)) = (object["name"].as_str(), object["values"].as_object()) else {
        return refused("bad request: name is text and values an object");
    };
    let Some(manifest) = installed(place.apps, name) else {
        return refused("no such app");
    };
    // Every value is checked before any is kept: all or nothing.
    for (key, value) in values {
        let Some(setting) = manifest.settings.iter().find(|setting| &setting.key == key) else {
            return refused(&format!("{key}: not a setting of this app"));
        };
        if let Err(why) = value_ok(setting, value) {
            return refused(&why);
        }
    }
    let Some(state) = place.state else {
        return refused("settings cannot be kept here");
    };
    let mut all = kept_of(Some(state));
    let mine = all
        .entry(name.to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(mine) = mine.as_object_mut() else {
        return refused("settings could not be kept");
    };
    let mut changed = Vec::new();
    for (key, value) in values {
        if mine.get(key) != Some(value) {
            changed.push(key.clone());
        }
        mine.insert(key.clone(), value.clone());
    }
    if let Err(why) = keep_all(state, &all) {
        return refused(&why);
    }
    changed.sort();
    eprintln!(
        "lamp-control: {name}'s settings changed ({} key(s))",
        changed.len()
    );
    json!({"ok": true, "changed": changed})
}

/// The path the helper's tests and the product agree on for tests.
pub fn state_file(state: &Path) -> PathBuf {
    state.join(SETTINGS_FILE)
}

#[cfg(test)]
mod tests;
