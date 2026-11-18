//! The apps the owner installs from the Store (#7415, piece 4): `apps.list`,
//! `apps.start`, `apps.stop`, `apps.remove`, `apps.settings.get` and
//! `apps.settings.set`, answered here for the app's Apps page. Off unless
//! `LAMP_STORE_APPS=on`, with `LAMP_CLOUD=on`; without it each kind is
//! answered as any this lamp lacks, and `can` does not list `apps`.
//!
//! `lamp-cloud` keeps and writes nothing of an app's. It READS what root's
//! manage helper leaves (the installed manifest and the marks, world-readable),
//! and asks two others, one JSON line out and one back:
//! - the manage helper ([`HELPER_SOCKET`]), root's, which starts, stops and
//!   removes an app and owns its folders;
//! - lamp-control's admin socket ([`CONTROL_SOCKET`]), which keeps the owner's
//!   yes, an app's settings values and its panel.
//!
//! **The owner's alone, as every kind here is:** only the lamp's own account
//! channel commands it (`online::commanded`); nothing here reads a "who" from
//! the payload. A remove records the owner's yes in lamp-control first (the
//! app's tap, keyed on the installed version and package SHA-256); the helper
//! spends it. Start and stop need no yes (the owner's own app, as a skill's
//! on/off).
//!
//! The wire to the helper and to lamp-control is in [`Wire`] alone, so a name
//! the other side settles differently changes one place.
use crate::settings::Outcome;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::time::Instant;

/// `on`, beside `LAMP_CLOUD`, takes the apps' kinds.
pub const ENVIRONMENT: &str = "LAMP_STORE_APPS";

/// The word in `system.info`'s `can` that says the apps' kinds are taken.
pub const CAN: &str = "apps";

pub const LIST_KIND: &str = "apps.list";
pub const START_KIND: &str = "apps.start";
pub const STOP_KIND: &str = "apps.stop";
pub const REMOVE_KIND: &str = "apps.remove";
pub const SETTINGS_SET_KIND: &str = "apps.settings.set";
pub const SETTINGS_GET_KIND: &str = "apps.settings.get";
pub const INSTALL_KIND: &str = "apps.install";
/// A push: the app stopped after its restart limit.
pub const STOPPED_PUSH: &str = "app.stopped";
/// A push: the app's panel changed (at most once a minute for an app).
pub const PANEL_PUSH: &str = "app.panel";

/// Every kind this part answers, after [`CAN`].
pub const KINDS: [&str; 7] = [
    INSTALL_KIND,
    LIST_KIND,
    START_KIND,
    STOP_KIND,
    REMOVE_KIND,
    SETTINGS_SET_KIND,
    SETTINGS_GET_KIND,
];

/// The manage helper's socket: root's, which only `lamp-cloud.service` may ask.
pub const HELPER_SOCKET: &str = "/run/lampos-apps-manage/manage.sock";
/// lamp-control's admin socket, which only `lamp-cloud.service` may ask.
pub const CONTROL_SOCKET: &str = "/run/lampos-control/admin.sock";

/// The helper may take a while to stop an app's last process.
pub const HELPER_WAIT: Duration = Duration::from_secs(20);
/// lamp-control answers from memory.
pub const CONTROL_WAIT: Duration = Duration::from_secs(5);

/// The installed package's manifest, in the version `current` links
/// (nine's FORMAT 2), readable by `lampos`.
pub const MANIFEST_FILE: &str = "lamp-skill.json";
/// The helper's own record of what it installed: `version`, `grants`, `net`
/// and the whole package's `sha256`.
pub const RECORD_FILE: &str = "lamp-apps.json";

/// The switch is off or the app does not know this kind.
pub const OFF: &str = "apps are off on this lamp";
/// The id is not a name an app has.
pub const NOT_A_NAME: &str = "id must be the app's name";
/// Nothing installed has this name.
pub const NOT_INSTALLED: &str = "no app by that name is installed";
/// The helper's socket isn't there or did not answer.
pub const HELPER_UNREACHED: &str = "the lamp's app helper can't be reached now";
/// lamp-control's admin socket isn't there or did not answer.
pub const CONTROL_UNREACHED: &str = "the lamp's control can't be reached now";

/// What an app's state is, as the Apps page shows it.
pub mod state {
    pub const INSTALLING: &str = "installing";
    pub const RUNNING: &str = "running";
    pub const STOPPED: &str = "stopped";
    /// systemd gave up after the unit's restart limit: the owner starts it.
    pub const STOPPED_AFTER_FAILURES: &str = "stopped_after_failures";
}

/// The marks root's helper keeps for an app, one empty file each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Marks {
    pub installing: bool,
    pub removing: bool,
    pub stopped: bool,
}

/// What systemd says of the app's unit (either variant).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Unit {
    pub active: bool,
    /// The unit gave up: `Result=start-limit-hit`.
    pub gave_up: bool,
}

/// What an installed app was installed as: the version and the package's
/// SHA-256, which a remove's yes is keyed on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Package {
    pub version: String,
    pub sha256: String,
    /// What it was installed with: an update's yes is for the new ones only.
    pub grants: Vec<String>,
    /// What it was installed with, outside the grants (the manifest's
    /// top-level `net` and `logs`): an update's yes carries these like grants.
    pub net: bool,
    pub logs: bool,
}

/// What this part reads of and asks of the lamp: the files, systemd, and the
/// two sockets. Behind a trait so the answers are tested with a fake.
pub trait Backend: Send + Sync {
    /// The names with an installed folder.
    fn names(&self) -> Vec<String>;
    /// The installed manifest, as signed (`current/lamp-skill.json`).
    fn manifest(&self, name: &str) -> Option<Value>;
    fn marks(&self, name: &str) -> Marks;
    fn unit(&self, name: &str) -> Unit;
    fn package(&self, name: &str) -> Option<Package>;
    /// One line to the manage helper and its answer.
    fn helper(&self, line: &Value) -> Result<Value, String>;
    /// One line to lamp-control's admin socket and its answer.
    fn control(&self, line: &Value) -> Result<Value, String>;
    /// The package at `url`, at most `most` bytes, in memory.
    fn download(&self, url: &str, most: u64) -> Result<Vec<u8>, String>;
    /// The staging folder (lampos's, root's helper reads from it).
    fn staging(&self) -> PathBuf;
    /// `bytes` into `path`, whole or not at all (a `.part` then a rename).
    fn stage(&self, path: &Path, bytes: &[u8]) -> Result<(), String>;
    /// The staged file gone (no error if it is not there).
    fn unstage(&self, path: &Path);
}

/// What the broker asks of: the backend, and nothing else.
pub struct Apps {
    backend: Arc<dyn Backend>,
    /// Results and pushes waiting to be said, oldest first.
    done: Arc<Mutex<Vec<Outcome>>>,
    /// Installs under way, at most [`MOST_AT_ONCE`].
    under_way: Arc<AtomicUsize>,
    watching: Arc<Mutex<Watch>>,
}

/// What the watcher last saw of each app, so it pushes only a change.
#[derive(Default)]
pub struct Watch {
    at: Option<Instant>,
    running: bool,
    seen: HashMap<String, Seen>,
}

#[derive(Default)]
struct Seen {
    state: &'static str,
    rev: Option<u64>,
    /// The panel moved and its push is waiting for the minute.
    owed: bool,
    pushed: Option<Instant>,
}

impl std::fmt::Debug for Apps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Apps").finish_non_exhaustive()
    }
}

/// Installs at once: a download of up to 18 MiB each.
pub const MOST_AT_ONCE: usize = 1;
/// How often the watcher looks, at most.
pub const WATCH_EVERY: Duration = Duration::from_secs(15);
/// An app's panel is pushed at most this often.
pub const PANEL_EVERY: Duration = Duration::from_secs(60);
/// The most a manifest in a package may be.
pub const MOST_MANIFEST: u64 = 64 * 1024;
/// An install or a remove of the app is under way.
pub const UNDER_WAY: &str = "the app is being installed or removed: try again in a moment";
/// The install broke off without an answer of its own.
pub const FELL_OVER: &str = "the install stopped unexpectedly: try again";
/// Another install is under way.
pub const BUSY: &str = "another app is being installed: try again in a moment";

impl Apps {
    pub fn new(backend: Box<dyn Backend>) -> Self {
        Self {
            backend: Arc::from(backend),
            done: Arc::default(),
            under_way: Arc::default(),
            watching: Arc::default(),
        }
    }

    /// Results and pushes since the last call, oldest first.
    pub fn take(&self) -> Vec<Outcome> {
        self.done
            .lock()
            .map(|mut done| std::mem::take(&mut *done))
            .unwrap_or_default()
    }

    /// Said again first on the next connection: lost before their ack.
    pub fn owe(&self, again: Vec<Outcome>) {
        if again.is_empty() {
            return;
        }
        if let Ok(mut done) = self.done.lock() {
            let later = std::mem::take(&mut *done);
            done.extend(again);
            done.extend(later);
        }
    }

    /// The product's, with the switch on.
    pub fn from_environment() -> Option<Self> {
        (std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on"))
            .then(|| Self::new(Box::new(Real::product())))
    }

    pub fn answer(&self, kind: &str, data: &Value) -> Result<Option<Value>, String> {
        match kind {
            INSTALL_KIND => self.install(data).map(|()| None),
            LIST_KIND => Ok(Some(self.list())),
            START_KIND | STOP_KIND => self.turn(kind, data).map(Some),
            REMOVE_KIND => self.remove(data).map(Some),
            SETTINGS_SET_KIND => self.settings_set(data).map(Some),
            SETTINGS_GET_KIND => self.settings_get(data).map(Some),
            other => Err(crate::wire::not_here(other)),
        }
    }

    /// The installed apps, each as the Apps page draws it.
    pub fn list(&self) -> Value {
        let mut names = self.backend.names();
        names.retain(|name| lamp_apps::name::app_name_ok(name));
        names.sort();
        let apps: Vec<Value> = names.iter().filter_map(|name| self.row(name)).collect();
        json!({ "apps": apps })
    }

    /// One app's row: its manifest's fields the page draws, its state, and
    /// what lamp-control keeps (settings values, the panel).
    fn row(&self, name: &str) -> Option<Value> {
        let manifest = self.backend.manifest(name)?;
        let marks = self.backend.marks(name);
        let unit = self.backend.unit(name);
        let package = self.backend.package(name);
        let mut row = Map::new();
        row.insert("id".into(), json!(name));
        for key in [
            "title", "version", "grants", "net", "logs", "phrases", "triggers", "settings",
            "skills",
        ] {
            if let Some(value) = manifest.get(key) {
                row.insert(key.into(), value.clone());
            }
        }
        if let Some(package) = &package {
            row.insert("version".into(), json!(package.version));
        }
        row.insert("state".into(), json!(state_of(&marks, &unit)));
        // Values and panel are lamp-control's (`app_state`: `settings` are the
        // values now, `panel` the latest or null, `socket` whether the app is
        // connected); a lamp-control that is not there, or off, leaves them
        // out and the page shows the rest.
        if let Ok(kept) = self.backend.control(&Wire::state(name))
            && let Some(kept) = kept.as_object()
            && !kept.contains_key("refused")
        {
            for (from, to) in [
                ("settings", "values"),
                ("panel", "panel"),
                ("socket", "connected"),
            ] {
                if let Some(value) = kept.get(from) {
                    row.insert(to.into(), value.clone());
                }
            }
        }
        Some(Value::Object(row))
    }

    /// `apps.start` or `apps.stop` of `{"id": NAME}`.
    fn turn(&self, kind: &str, data: &Value) -> Result<Value, String> {
        let name = self.installed(data)?;
        let marks = self.backend.marks(&name);
        if marks.removing || marks.installing {
            return Err(UNDER_WAY.into());
        }
        let line = if kind == START_KIND {
            Wire::start(&name)
        } else {
            Wire::stop(&name)
        };
        let answer = self.backend.helper(&line)?;
        refused(&answer)?;
        Ok(
            json!({ "id": name, "state": if kind == START_KIND { state::RUNNING } else { state::STOPPED } }),
        )
    }

    /// `apps.remove` of `{"id": NAME}`: the owner's yes first, for the
    /// installed version and package; then the helper, which spends it.
    fn remove(&self, data: &Value) -> Result<Value, String> {
        let name = self.installed(data)?;
        let marks = self.backend.marks(&name);
        if marks.removing || marks.installing {
            return Err(UNDER_WAY.into());
        }
        let package = self
            .backend
            .package(&name)
            .ok_or("the app's installed version is not known: it can't be removed from here")?;
        // A remove's yes names no grants (eighth's key: remove, name, the
        // installed version and package SHA-256, grants []).
        let recorded =
            self.backend
                .control(&Wire::yes("remove", &name, &package, &[], false, false))?;
        refused(&recorded)?;
        let answer = self.backend.helper(&Wire::remove(&name))?;
        refused(&answer)?;
        Ok(json!({ "id": name, "removed": true }))
    }

    /// `apps.settings.set` of `{"id": NAME, "values": {...}}`: checked
    /// against the schema by lamp-control, which keeps them.
    fn settings_set(&self, data: &Value) -> Result<Value, String> {
        let name = self.installed(data)?;
        let values = data
            .get("values")
            .and_then(Value::as_object)
            .ok_or("values must be an object")?;
        let answer = self.backend.control(&Wire::settings_set(&name, values))?;
        refused(&answer)?;
        Ok(answer)
    }

    /// `apps.settings.get` of `{"id": NAME}`: the schema and the values now.
    fn settings_get(&self, data: &Value) -> Result<Value, String> {
        let name = self.installed(data)?;
        let manifest = self.backend.manifest(&name).ok_or(NOT_INSTALLED)?;
        // A lamp-control that is not there is said, not shown as "nothing set".
        let kept = self.backend.control(&Wire::state(&name))?;
        refused(&kept)?;
        let values = kept.get("settings").cloned().unwrap_or(Value::Null);
        Ok(json!({
            "id": name,
            "settings": manifest.get("settings").cloned().unwrap_or(json!([])),
            "values": values,
        }))
    }

    /// `data`'s `id` as a name of an installed app.
    fn installed(&self, data: &Value) -> Result<String, String> {
        let name = data
            .get("id")
            .and_then(Value::as_str)
            .filter(|name| lamp_apps::name::app_name_ok(name))
            .ok_or(NOT_A_NAME)?;
        if !self.backend.names().iter().any(|have| have == name) {
            return Err(NOT_INSTALLED.into());
        }
        Ok(name.to_owned())
    }
}

/// The state the page shows: a mark first, then systemd's word.
pub fn state_of(marks: &Marks, unit: &Unit) -> &'static str {
    if marks.installing || marks.removing {
        state::INSTALLING
    } else if unit.active {
        state::RUNNING
    } else if unit.gave_up && !marks.stopped {
        state::STOPPED_AFTER_FAILURES
    } else {
        state::STOPPED
    }
}

/// A `{"refused": WHY}` or `{"error": WHY}` answer as the reason it says.
fn refused(answer: &Value) -> Result<(), String> {
    for key in ["refused", "error"] {
        if let Some(why) = answer.get(key) {
            return Err(why
                .as_str()
                .map_or_else(|| "refused".to_owned(), str::to_owned));
        }
    }
    Ok(())
}

/// The values an `app_yes_record` carries, as lamp-control's `key_of` takes
/// them: exactly these seven, no more (a test reads its source for them).
pub const YES_FIELDS: [&str; 7] = ["op", "name", "version", "sha256", "grants", "net", "logs"];

/// The lines the two sockets take, in one place.
pub struct Wire;

impl Wire {
    pub fn start(name: &str) -> Value {
        json!({ "start": name })
    }
    pub fn stop(name: &str) -> Value {
        json!({ "stop": name })
    }
    pub fn remove(name: &str) -> Value {
        json!({ "remove": name })
    }
    /// Install or update: the staged file by its absolute path (eighth's
    /// helper looks at what is installed).
    pub fn install(path: &Path) -> Value {
        json!({ "install": { "path": path } })
    }
    /// lamp-control's `app_yes_record` (nine, #7714's `app_yes::key_of`):
    /// exactly the seven values the helper's take is keyed on ([`YES_FIELDS`]),
    /// the grants sorted and de-duplicated. `net` and `logs` are manifest
    /// booleans outside the grants and are in the key like grants (the first
    /// builder, 18 Nov 12:10): an install says what it turns on, an update
    /// only what it newly turns on, a remove neither.
    pub fn yes(
        op: &str,
        name: &str,
        package: &Package,
        grants: &[String],
        net: bool,
        logs: bool,
    ) -> Value {
        let mut grants = grants.to_vec();
        grants.sort();
        grants.dedup();
        json!({"app_yes_record": {
            "op": op,
            "name": name,
            "version": package.version,
            "sha256": package.sha256,
            "grants": grants,
            "net": net,
            "logs": logs,
        }})
    }
    /// An app's settings values and panel, asked of lamp-control.
    pub fn state(name: &str) -> Value {
        json!({ "app_state": { "name": name } })
    }
    pub fn settings_set(name: &str, values: &Map<String, Value>) -> Value {
        json!({ "app_settings_set": { "name": name, "values": values } })
    }
}

/// An `apps.install`'s request, strictly: `{id, url, sha256, version}`, each
/// a string, and an optional `request_id` the answer says back.
struct Asked {
    id: String,
    url: String,
    sha256: String,
    version: String,
    request_id: Option<String>,
}

impl Asked {
    fn from_json(data: &Value) -> Result<Self, String> {
        let object = data.as_object().ok_or("the request must be an object")?;
        if let Some(extra) = object.keys().find(|key| {
            !matches!(
                key.as_str(),
                "id" | "url" | "sha256" | "version" | "request_id"
            )
        }) {
            return Err(format!("{extra:?} is not a field of apps.install"));
        }
        let text = |key: &str| {
            object
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("{key} must be a string"))
        };
        let id = text("id")?;
        if !lamp_apps::name::app_name_ok(&id) {
            return Err(NOT_A_NAME.into());
        }
        let url = text("url")?;
        if !lamp_skills::package::store_url_ok(&url) {
            return Err(lamp_skills::package::NOT_A_STORE_URL.into());
        }
        let sha256 = text("sha256")?.to_ascii_lowercase();
        if !lamp_skills::package::sha256_ok(&sha256) {
            return Err(lamp_skills::package::NOT_A_SHA256.into());
        }
        let version = text("version")?;
        if !version_ok(&version) {
            return Err(lamp_skills::package::NOT_A_VERSION.into());
        }
        let request_id = object
            .get("request_id")
            .and_then(Value::as_str)
            .filter(|id| {
                !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_graphic())
            })
            .map(str::to_owned);
        Ok(Self {
            id,
            url,
            sha256,
            version,
            request_id,
        })
    }
}

/// `MAJOR.MINOR.PATCH`, each a number of at most nine digits.
fn version_ok(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty() && part.len() <= 9 && part.bytes().all(|b| b.is_ascii_digit())
        })
}

/// The manifest's `lamp-skill.json` out of a package's bytes: the one file
/// the owner's yes needs (name, version, grants). Nothing is trusted from it:
/// the helper reads the package again and checks the signature and every
/// file, and a yes made for other values does not match.
fn manifest_of(bytes: &[u8]) -> Result<Value, String> {
    let mut archive = tar::Archive::new(bytes);
    let entries = archive.entries().map_err(|_| NOT_AN_APP)?;
    for entry in entries {
        let mut entry = entry.map_err(|_| NOT_AN_APP.to_owned())?;
        if entry.path_bytes().as_ref() != lamp_skills::package::MANIFEST.as_bytes() {
            continue;
        }
        if entry.size() > MOST_MANIFEST {
            return Err(NOT_AN_APP.into());
        }
        let mut text = Vec::new();
        (&mut entry)
            .take(MOST_MANIFEST)
            .read_to_end(&mut text)
            .map_err(|_| NOT_AN_APP.to_owned())?;
        return serde_json::from_slice(&text).map_err(|_| NOT_AN_APP.to_owned());
    }
    Err(NOT_AN_APP.into())
}

/// The package is not an app's.
pub const NOT_AN_APP: &str = "the package is not an app";
/// What was fetched is not what the Store listed.
pub const MISMATCH: &str = "the package is not the one the Store listed";

impl Apps {
    /// `apps.install`: taken `starting` at once (`Ok`), its result later
    /// (`take`): the package fetched into the staging folder, the owner's yes
    /// recorded for what it holds, and the helper asked to install it.
    pub fn install(&self, data: &Value) -> Result<(), String> {
        let asked = Asked::from_json(data)?;
        if self
            .under_way
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MOST_AT_ONCE).then_some(n + 1)
            })
            .is_err()
        {
            return Err(BUSY.into());
        }
        let (backend, done, under_way) = (
            self.backend.clone(),
            self.done.clone(),
            self.under_way.clone(),
        );
        std::thread::spawn(move || {
            // The slot is freed even if the install panics.
            let _slot = Slot(under_way);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                install_run(&*backend, &asked)
            }))
            .unwrap_or_else(|_| {
                // The phone is told, and the staged file (if one was made) goes.
                backend.unstage(&backend.staging().join(format!("{}.tar", asked.sha256)));
                Err(FELL_OVER.into())
            })
            .map(Some);
            if let Ok(mut done) = done.lock() {
                done.push(Outcome {
                    kind: INSTALL_KIND.to_owned(),
                    result,
                    request_id: asked.request_id.clone(),
                    code: None,
                });
            }
        });
        Ok(())
    }

    /// Looks at the apps now and then (never more than every
    /// [`WATCH_EVERY`], on a thread of its own) and keeps a push for a change:
    /// `app.stopped` when an app gave up, `app.panel` when its panel moved.
    pub fn watch(&self, now: Instant) {
        {
            // Never waits: a tick that finds the lock taken tries again at the next.
            let Ok(mut watch) = self.watching.try_lock() else {
                return;
            };
            if watch.running
                || watch
                    .at
                    .is_some_and(|at| now.duration_since(at) < WATCH_EVERY)
            {
                return;
            }
            watch.running = true;
            watch.at = Some(now);
        }
        let (backend, done, watching) = (
            self.backend.clone(),
            self.done.clone(),
            self.watching.clone(),
        );
        std::thread::spawn(move || {
            let _looking = Looking(watching.clone());
            let pushes = look(&*backend, &watching, now);
            if let Ok(mut done) = done.lock() {
                done.extend(pushes);
            }
        });
    }
}

/// One install's place among those under way, given back when it drops.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Clears `running` when a look ends, even by a panic.
struct Looking(Arc<Mutex<Watch>>);

impl Drop for Looking {
    fn drop(&mut self) {
        let mut watch = self.0.lock().unwrap_or_else(|e| e.into_inner());
        watch.running = false;
    }
}

/// One look: the pushes it owes.
pub fn look(backend: &dyn Backend, watching: &Mutex<Watch>, now: Instant) -> Vec<Outcome> {
    let mut names = backend.names();
    names.retain(|name| lamp_apps::name::app_name_ok(name));
    let mut pushes = Vec::new();
    // Gather with no lock held: the backend's calls may take seconds.
    let gathered: Vec<(String, &'static str, Option<u64>)> = names
        .into_iter()
        .map(|name| {
            let state = state_of(&backend.marks(&name), &backend.unit(&name));
            let panel_rev = backend
                .control(&Wire::state(&name))
                .ok()
                .filter(|kept| kept.get("refused").is_none())
                .and_then(|kept| kept["panel"]["rev"].as_u64());
            (name, state, panel_rev)
        })
        .collect();
    let Ok(mut watch) = watching.lock() else {
        return pushes;
    };
    watch
        .seen
        .retain(|name, _| gathered.iter().any(|g| &g.0 == name));
    for (name, state, panel_rev) in gathered {
        let first = !watch.seen.contains_key(&name);
        let seen = watch.seen.entry(name.clone()).or_default();
        // The first look only records: nothing said of what was already so.
        if !first {
            if state == state::STOPPED_AFTER_FAILURES && seen.state != state {
                pushes.push(push(
                    STOPPED_PUSH,
                    json!({"id": name, "reason": "stopped after repeated failures"}),
                ));
            }
            if panel_rev.is_some() && panel_rev != seen.rev {
                seen.owed = true;
            }
            if seen.owed
                && seen
                    .pushed
                    .is_none_or(|at| now.duration_since(at) >= PANEL_EVERY)
            {
                pushes.push(push(PANEL_PUSH, json!({"id": name})));
                seen.owed = false;
                seen.pushed = Some(now);
            }
        }
        seen.state = state;
        seen.rev = panel_rev;
    }
    pushes
}

fn push(kind: &str, data: Value) -> Outcome {
    Outcome {
        kind: kind.to_owned(),
        result: Ok(Some(data)),
        request_id: None,
        code: None,
    }
}

/// The install, whole, on its thread: what it says at the end.
fn install_run(backend: &dyn Backend, asked: &Asked) -> Result<Value, String> {
    let bytes = backend.download(&asked.url, lamp_apps::staging::MOST_STAGED)?;
    // What was fetched is what the owner was shown.
    if lamp_skills::package::sha256_hex(&bytes) != asked.sha256 {
        return Err(MISMATCH.into());
    }
    let manifest = manifest_of(&bytes)?;
    if manifest["name"].as_str() != Some(asked.id.as_str())
        || manifest["version"].as_str() != Some(asked.version.as_str())
    {
        return Err(MISMATCH.into());
    }
    let asks: Vec<String> = manifest["grants"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|grant| grant.as_str().map(str::to_owned))
        .collect();
    let (asks_net, asks_logs) = (
        manifest["net"].as_bool().unwrap_or(false),
        manifest["logs"].as_bool().unwrap_or(false),
    );
    // An update's yes is for the grants, and `net` and `logs`, it newly asks;
    // an install's for all.
    let (op, grants, net, logs, updated) = match backend.package(&asked.id) {
        Some(have) if backend.names().contains(&asked.id) => (
            "update",
            asks.iter()
                .filter(|grant| !have.grants.contains(grant))
                .cloned()
                .collect::<Vec<_>>(),
            asks_net && !have.net,
            asks_logs && !have.logs,
            true,
        ),
        _ => ("install", asks, asks_net, asks_logs, false),
    };
    let path = backend.staging().join(format!("{}.tar", asked.sha256));
    backend.stage(&path, &bytes)?;
    let result = (|| {
        let package = Package {
            version: asked.version.clone(),
            sha256: asked.sha256.clone(),
            grants: Vec::new(),
            net: false,
            logs: false,
        };
        refused(&backend.control(&Wire::yes(op, &asked.id, &package, &grants, net, logs))?)?;
        let answer = backend.helper(&Wire::install(&path))?;
        refused(&answer)?;
        Ok(json!({"id": asked.id, "version": asked.version, "updated": updated}))
    })();
    // The helper never deletes it: lampos owns the folder.
    backend.unstage(&path);
    result
}

/// The product's files, systemd and sockets.
pub struct Real {
    pub layout: lamp_apps::layout::Layout,
    pub helper: PathBuf,
    pub control: PathBuf,
}

impl Real {
    pub fn product() -> Self {
        Self {
            layout: lamp_apps::layout::Layout::product(),
            helper: HELPER_SOCKET.into(),
            control: CONTROL_SOCKET.into(),
        }
    }
}

/// One JSON line to `socket`, one back, waiting at most `within`.
pub fn ask(
    socket: &Path,
    line: &Value,
    within: Duration,
    unreached: &str,
) -> Result<Value, String> {
    let io = |_: std::io::Error| unreached.to_owned();
    let mut stream = UnixStream::connect(socket).map_err(io)?;
    stream.set_read_timeout(Some(within)).map_err(io)?;
    stream.set_write_timeout(Some(CONTROL_WAIT)).map_err(io)?;
    let mut text = line.to_string();
    text.push('\n');
    stream.write_all(text.as_bytes()).map_err(io)?;
    let mut reply = String::new();
    BufReader::new(stream.take(64 * 1024))
        .read_line(&mut reply)
        .map_err(io)?;
    serde_json::from_str(reply.trim()).map_err(|_| unreached.to_owned())
}

impl Backend for Real {
    fn names(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(&self.layout.code) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| lamp_apps::name::app_name_ok(name))
            .collect()
    }

    fn manifest(&self, name: &str) -> Option<Value> {
        let text = std::fs::read_to_string(
            self.layout
                .code
                .join(name)
                .join("current")
                .join(MANIFEST_FILE),
        )
        .ok()?;
        serde_json::from_str(&text).ok()
    }

    fn marks(&self, name: &str) -> Marks {
        use lamp_apps::state::{Mark, has};
        Marks {
            installing: has(&self.layout.state, name, Mark::Installing),
            removing: has(&self.layout.state, name, Mark::Removing),
            stopped: has(&self.layout.state, name, Mark::Stopped),
        }
    }

    fn unit(&self, name: &str) -> Unit {
        let mut unit = Unit::default();
        for variant in [
            lamp_apps::name::plain_unit(name),
            lamp_apps::name::net_unit(name),
        ] {
            let Ok(output) = std::process::Command::new(lamp_apps::effects::SYSTEMCTL)
                .args(["show", "--property=ActiveState,Result", "--", &variant])
                .env_clear()
                .output()
            else {
                continue;
            };
            let text = String::from_utf8_lossy(&output.stdout);
            unit.active |= text.lines().any(|line| line == "ActiveState=active");
            unit.gave_up |= text.lines().any(|line| line == "Result=start-limit-hit");
        }
        unit
    }

    fn package(&self, name: &str) -> Option<Package> {
        // The helper's own record, written at install from the verified
        // package, never from a package file.
        let text = std::fs::read_to_string(
            self.layout
                .code
                .join(name)
                .join("current")
                .join(RECORD_FILE),
        )
        .ok()?;
        let record: Value = serde_json::from_str(&text).ok()?;
        let sha256 = record["sha256"].as_str()?;
        (sha256.len() == 64 && sha256.bytes().all(|b| b.is_ascii_hexdigit())).then(|| Package {
            version: record["version"].as_str().unwrap_or_default().to_owned(),
            sha256: sha256.to_ascii_lowercase(),
            grants: record["grants"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|grant| grant.as_str().map(str::to_owned))
                .collect(),
            net: record["net"].as_bool().unwrap_or(false),
            // The helper's record, as the helper reads it (an old record has
            // none: no logs), so the yes and the helper's key agree.
            logs: record["logs"].as_bool().unwrap_or(false),
        })
    }

    fn helper(&self, line: &Value) -> Result<Value, String> {
        ask(&self.helper, line, HELPER_WAIT, HELPER_UNREACHED)
    }

    fn control(&self, line: &Value) -> Result<Value, String> {
        ask(&self.control, line, CONTROL_WAIT, CONTROL_UNREACHED)
    }

    fn download(&self, url: &str, most: u64) -> Result<Vec<u8>, String> {
        let url = url.to_owned();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| NOT_FETCHED.to_owned())?;
            runtime.block_on(async move {
                let client = reqwest::Client::builder()
                    .tls_backend_preconfigured(crate::beat::tls()?)
                    .https_only(true)
                    .redirect(reqwest::redirect::Policy::none())
                    .timeout(DOWNLOAD_WITHIN)
                    .build()
                    .map_err(|_| NOT_FETCHED.to_owned())?;
                let mut response = client
                    .get(&url)
                    .send()
                    .await
                    .map_err(|_| NOT_FETCHED.to_owned())?;
                if !response.status().is_success() {
                    return Err(NOT_FETCHED.to_owned());
                }
                let mut bytes = Vec::new();
                while let Some(chunk) =
                    response.chunk().await.map_err(|_| NOT_FETCHED.to_owned())?
                {
                    bytes.extend_from_slice(&chunk);
                    if bytes.len() as u64 > most {
                        return Err(TOO_BIG.to_owned());
                    }
                }
                Ok(bytes)
            })
        })
        .join()
        .unwrap_or_else(|_| Err(NOT_FETCHED.to_owned()))
    }

    fn staging(&self) -> PathBuf {
        self.layout.staging.clone()
    }

    fn stage(&self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        use std::os::unix::fs::OpenOptionsExt;
        let part = path.with_extension("tar.part");
        let written = (|| -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
                .open(&part)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            std::fs::rename(&part, path)
        })();
        written.map_err(|error| {
            let _ = std::fs::remove_file(&part);
            format!("the package could not be kept for the helper: {error}")
        })
    }

    fn unstage(&self, path: &Path) {
        let _ = std::fs::remove_file(path);
    }
}

/// The package could not be fetched.
pub const NOT_FETCHED: &str = "the package could not be fetched";
/// The package is bigger than an app package may be.
pub const TOO_BIG: &str = "the package is bigger than an app may be";
/// How long a download may take, whole.
pub const DOWNLOAD_WITHIN: Duration = Duration::from_secs(120);

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A fake backend: installed apps by name, and every line asked of
    /// either socket, in order.
    #[derive(Default)]
    pub struct Fake {
        pub apps: Vec<(String, Value, Marks, Unit, Option<Package>)>,
        pub lines: Mutex<Vec<String>>,
        pub helper_answer: Option<Value>,
        pub control_answer: Option<Value>,
        pub helper_down: bool,
        /// What `download` gives, the whole package.
        pub package: Option<Vec<u8>>,
    }

    impl Fake {
        pub fn app(name: &str) -> (String, Value, Marks, Unit, Option<Package>) {
            (
                name.to_owned(),
                json!({
                    "title": "English lessons", "version": "1.2.0", "grants": ["speak", "hear"],
                    "net": false, "logs": true, "phrases": ["start my english lesson"],
                    "triggers": [], "settings": [{"key": "level", "type": "choice"}],
                    "skills": ["english-tutor"],
                }),
                Marks::default(),
                Unit {
                    active: true,
                    gave_up: false,
                },
                Some(Package {
                    version: "1.2.0".into(),
                    sha256: "a".repeat(64),
                    grants: vec!["speak".into(), "hear".into()],
                    net: false,
                    logs: true,
                }),
            )
        }
    }

    impl Backend for Fake {
        fn names(&self) -> Vec<String> {
            self.apps.iter().map(|app| app.0.clone()).collect()
        }
        fn manifest(&self, name: &str) -> Option<Value> {
            self.apps
                .iter()
                .find(|app| app.0 == name)
                .map(|app| app.1.clone())
        }
        fn marks(&self, name: &str) -> Marks {
            self.apps
                .iter()
                .find(|app| app.0 == name)
                .map(|app| app.2)
                .unwrap_or_default()
        }
        fn unit(&self, name: &str) -> Unit {
            self.apps
                .iter()
                .find(|app| app.0 == name)
                .map(|app| app.3)
                .unwrap_or_default()
        }
        fn package(&self, name: &str) -> Option<Package> {
            self.apps
                .iter()
                .find(|app| app.0 == name)
                .and_then(|app| app.4.clone())
        }
        fn helper(&self, line: &Value) -> Result<Value, String> {
            self.lines.lock().unwrap().push(format!("helper {line}"));
            if self.helper_down {
                return Err(HELPER_UNREACHED.into());
            }
            Ok(self.helper_answer.clone().unwrap_or(json!({"ok": true})))
        }
        fn download(&self, url: &str, _most: u64) -> Result<Vec<u8>, String> {
            self.lines.lock().unwrap().push(format!("download {url}"));
            self.package.clone().ok_or_else(|| NOT_FETCHED.to_owned())
        }
        fn staging(&self) -> PathBuf {
            "/staging".into()
        }
        fn stage(&self, path: &Path, _bytes: &[u8]) -> Result<(), String> {
            self.lines
                .lock()
                .unwrap()
                .push(format!("stage {}", path.display()));
            Ok(())
        }
        fn unstage(&self, path: &Path) {
            self.lines
                .lock()
                .unwrap()
                .push(format!("unstage {}", path.display()));
        }
        fn control(&self, line: &Value) -> Result<Value, String> {
            self.lines.lock().unwrap().push(format!("control {line}"));
            if line.get("app_state").is_some() {
                return Ok(
                    json!({"settings": {"level": "a1"}, "panel": {"title": "Level A1", "lines": [], "rev": 3}, "socket": true}),
                );
            }
            Ok(self.control_answer.clone().unwrap_or(json!({"ok": true})))
        }
    }

    fn apps(fake: Fake) -> (Apps, std::sync::Arc<Mutex<Vec<String>>>) {
        let lines = std::sync::Arc::new(Mutex::new(Vec::new()));
        // The lines are read back through the same fake: keep a handle.
        let _ = &lines;
        (Apps::new(Box::new(fake)), lines)
    }

    #[test]
    fn list_has_each_installed_app_with_its_state_values_and_panel() {
        let mut fake = Fake::default();
        fake.apps.push(Fake::app("english-lessons"));
        let (apps, _) = apps(fake);
        let list = apps.list();
        let row = &list["apps"][0];
        assert_eq!(row["id"], "english-lessons");
        assert_eq!(row["state"], "running");
        assert_eq!(row["grants"], json!(["speak", "hear"]));
        assert_eq!(row["values"]["level"], "a1");
        assert_eq!(row["panel"]["title"], "Level A1");
        assert_eq!(row["connected"], true);
        assert_eq!(row["version"], "1.2.0");
    }

    #[test]
    fn a_name_that_is_not_an_app_name_is_never_listed_or_acted_on() {
        let mut fake = Fake::default();
        fake.apps.push(Fake::app("good"));
        fake.apps.push(Fake::app("../etc"));
        fake.apps.push(Fake::app("Bad"));
        let (apps, _) = apps(fake);
        let list = apps.list();
        assert_eq!(list["apps"].as_array().unwrap().len(), 1);
        for id in ["../etc", "Bad", "", "a b", "x"] {
            for kind in [START_KIND, STOP_KIND, REMOVE_KIND, SETTINGS_GET_KIND] {
                assert!(
                    apps.answer(kind, &json!({"id": id})).is_err(),
                    "{kind} {id}"
                );
            }
        }
        assert!(apps.answer(START_KIND, &json!({})).is_err());
        assert!(apps.answer(START_KIND, &json!({"id": 5})).is_err());
    }

    #[test]
    fn the_state_comes_from_the_marks_first_then_systemd() {
        let unit = |active, gave_up| Unit { active, gave_up };
        let none = Marks::default();
        assert_eq!(state_of(&none, &unit(true, false)), "running");
        assert_eq!(state_of(&none, &unit(false, false)), "stopped");
        assert_eq!(
            state_of(&none, &unit(false, true)),
            "stopped_after_failures"
        );
        let stopped = Marks {
            stopped: true,
            ..none
        };
        assert_eq!(state_of(&stopped, &unit(false, true)), "stopped");
        let installing = Marks {
            installing: true,
            ..none
        };
        assert_eq!(state_of(&installing, &unit(true, false)), "installing");
        let removing = Marks {
            removing: true,
            ..none
        };
        assert_eq!(state_of(&removing, &unit(false, false)), "installing");
    }

    #[test]
    fn start_and_stop_ask_the_helper_by_name_and_refuse_while_busy() {
        let mut fake = Fake::default();
        fake.apps.push(Fake::app("x1"));
        let mut busy = Fake::app("x2");
        busy.2.installing = true;
        fake.apps.push(busy);
        let (apps, _) = apps(fake);
        assert_eq!(
            apps.answer(STOP_KIND, &json!({"id": "x1"}))
                .unwrap()
                .unwrap()["state"],
            "stopped"
        );
        assert_eq!(
            apps.answer(START_KIND, &json!({"id": "x1"}))
                .unwrap()
                .unwrap()["state"],
            "running"
        );
        assert!(apps.answer(START_KIND, &json!({"id": "x2"})).is_err());
    }

    #[test]
    fn a_remove_records_the_yes_for_the_installed_package_then_asks_the_helper() {
        let mut fake = Fake::default();
        fake.apps.push(Fake::app("english-lessons"));
        let fake = std::sync::Arc::new(fake);
        let apps = Apps::new(Box::new(Shared(fake.clone())));
        let done = apps
            .answer(REMOVE_KIND, &json!({"id": "english-lessons"}))
            .unwrap()
            .unwrap();
        assert_eq!(done["removed"], true);
        let lines = fake.lines.lock().unwrap().clone();
        let yes = lines
            .iter()
            .position(|l| l.starts_with("control") && l.contains("app_yes_record"))
            .unwrap();
        let helper = lines
            .iter()
            .position(|l| l.starts_with("helper") && l.contains("remove"))
            .unwrap();
        assert!(yes < helper, "the yes first: {lines:?}");
        assert!(lines[yes].contains(&"a".repeat(64)) && lines[yes].contains("1.2.0"));
        assert!(
            lines[yes].contains(r#""grants":[]"#),
            "a remove's yes names no grants: {}",
            lines[yes]
        );
        assert!(lines[yes].contains(r#""op":"remove""#));
    }

    #[test]
    fn a_remove_refused_by_lamp_control_never_reaches_the_helper() {
        let mut fake = Fake::default();
        fake.apps.push(Fake::app("x1"));
        fake.control_answer = Some(json!({"refused": "apps are off on this lamp"}));
        let fake = std::sync::Arc::new(fake);
        let apps = Apps::new(Box::new(Shared(fake.clone())));
        let error = apps.answer(REMOVE_KIND, &json!({"id": "x1"})).unwrap_err();
        assert_eq!(error, "apps are off on this lamp");
        assert!(
            !fake
                .lines
                .lock()
                .unwrap()
                .iter()
                .any(|l| l.starts_with("helper"))
        );
    }

    #[test]
    fn a_remove_with_no_known_installed_package_is_refused_before_anything_is_asked() {
        let mut fake = Fake::default();
        let mut app = Fake::app("x1");
        app.4 = None;
        fake.apps.push(app);
        let (apps, _) = apps(fake);
        assert!(apps.answer(REMOVE_KIND, &json!({"id": "x1"})).is_err());
    }

    #[test]
    fn a_helper_that_is_down_is_told_plainly() {
        let mut fake = Fake::default();
        fake.apps.push(Fake::app("x1"));
        fake.helper_down = true;
        let (apps, _) = apps(fake);
        assert_eq!(
            apps.answer(STOP_KIND, &json!({"id": "x1"})).unwrap_err(),
            HELPER_UNREACHED
        );
    }

    #[test]
    fn settings_set_passes_only_the_name_and_the_values_object() {
        let mut fake = Fake::default();
        fake.apps.push(Fake::app("x1"));
        let (apps, _) = apps(fake);
        assert!(
            apps.answer(SETTINGS_SET_KIND, &json!({"id": "x1"}))
                .is_err()
        );
        assert!(
            apps.answer(SETTINGS_SET_KIND, &json!({"id": "x1", "values": [1]}))
                .is_err()
        );
        assert!(
            apps.answer(
                SETTINGS_SET_KIND,
                &json!({"id": "x1", "values": {"level": "a1"}})
            )
            .is_ok()
        );
        let got = apps
            .answer(SETTINGS_GET_KIND, &json!({"id": "x1"}))
            .unwrap()
            .unwrap();
        assert_eq!(got["values"]["level"], "a1");
        assert_eq!(got["settings"][0]["key"], "level");
    }

    #[test]
    fn the_yes_line_has_exactly_the_seven_values_and_no_who() {
        let package = Package {
            version: "1.0.0".into(),
            sha256: "b".repeat(64),
            grants: Vec::new(),
            net: false,
            logs: false,
        };
        let line = Wire::yes(
            "install",
            "x1",
            &package,
            &["speak".into(), "body".into(), "speak".into()],
            true,
            false,
        );
        let inner = line["app_yes_record"].as_object().unwrap();
        let mut keys: Vec<&str> = inner.keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut want = YES_FIELDS.to_vec();
        want.sort_unstable();
        assert_eq!(keys, want);
        assert_eq!(inner["grants"], json!(["body", "speak"]));
        assert_eq!(inner["net"], true);
        assert_eq!(inner["logs"], false);
    }

    /// lamp-control is a server, not a dependency, so its `key_of` is read as
    /// text (as lamp-apps reads the door's grants): the seven fields it takes,
    /// and that a remove's key has none of grants, `net` and `logs`. It fails if
    /// `app_yes.rs` is not there: since #7714 the key is always in the tree.
    #[test]
    fn the_yes_record_is_what_lamp_controls_key_of_takes() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../control/src/app_yes.rs");
        let source = std::fs::read_to_string(path)
            .expect("lamp-control's app_yes.rs (#7714) must be here: the key is read from it");
        let at = source.find("const FIELDS: [&str; 7] = [").expect(
            "key_of's FIELDS: lamp-control's key changed, so this record must change with it",
        );
        let block = &source[at..];
        let block = &block[..block.find("];").unwrap()];
        let theirs: Vec<&str> = block.split('"').skip(1).step_by(2).collect();
        assert_eq!(
            theirs, YES_FIELDS,
            "lamp-control's key and this record drifted"
        );
        let package = Package {
            version: "1.0.0".into(),
            sha256: "b".repeat(64),
            grants: Vec::new(),
            net: false,
            logs: false,
        };
        let remove = Wire::yes("remove", "x1", &package, &[], false, false);
        let inner = &remove["app_yes_record"];
        assert_eq!(inner["grants"], json!([]));
        assert_eq!(
            (&inner["net"], &inner["logs"]),
            (&json!(false), &json!(false))
        );
        assert_eq!(inner.as_object().unwrap().len(), theirs.len());
    }

    /// A package's bytes: a tar with the manifest beside the skill folder.
    fn package_of(manifest: Value) -> (Vec<u8>, String) {
        let text = manifest.to_string();
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(text.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "lamp-skill.json", text.as_bytes())
            .unwrap();
        let bytes = builder.into_inner().unwrap();
        let sha256 = lamp_skills::package::sha256_hex(&bytes);
        (bytes, sha256)
    }

    fn asked_for(id: &str, version: &str, sha256: &str) -> Asked {
        Asked {
            id: id.into(),
            url: "https://store.example/x.tar".into(),
            sha256: sha256.into(),
            version: version.into(),
            request_id: None,
        }
    }

    struct Shared(Arc<Fake>);
    impl Backend for Shared {
        fn names(&self) -> Vec<String> {
            self.0.names()
        }
        fn manifest(&self, n: &str) -> Option<Value> {
            self.0.manifest(n)
        }
        fn marks(&self, n: &str) -> Marks {
            self.0.marks(n)
        }
        fn unit(&self, n: &str) -> Unit {
            self.0.unit(n)
        }
        fn package(&self, n: &str) -> Option<Package> {
            self.0.package(n)
        }
        fn helper(&self, l: &Value) -> Result<Value, String> {
            self.0.helper(l)
        }
        fn control(&self, l: &Value) -> Result<Value, String> {
            self.0.control(l)
        }
        fn download(&self, u: &str, m: u64) -> Result<Vec<u8>, String> {
            self.0.download(u, m)
        }
        fn staging(&self) -> PathBuf {
            self.0.staging()
        }
        fn stage(&self, p: &Path, b: &[u8]) -> Result<(), String> {
            self.0.stage(p, b)
        }
        fn unstage(&self, p: &Path) {
            self.0.unstage(p)
        }
    }

    #[test]
    fn an_install_fetches_checks_records_the_yes_then_asks_the_helper_and_cleans_up() {
        let (bytes, sha) =
            package_of(json!({"name": "x1", "version": "1.0.0", "grants": ["speak", "hear"]}));
        let fake = Arc::new(Fake {
            package: Some(bytes),
            ..Default::default()
        });
        let done = install_run(&Shared(fake.clone()), &asked_for("x1", "1.0.0", &sha)).unwrap();
        assert_eq!(
            done,
            json!({"id": "x1", "version": "1.0.0", "updated": false})
        );
        let lines = fake.lines.lock().unwrap().clone();
        let at = |what: &str| {
            lines
                .iter()
                .position(|l| l.contains(what))
                .unwrap_or_else(|| panic!("{what}: {lines:?}"))
        };
        assert!(at("download") < at("stage") && at("stage") < at("app_yes_record"));
        assert!(
            at("app_yes_record") < at(r#"helper {"install""#)
                && at(r#"helper {"install""#) < at("unstage")
        );
        assert!(lines[at("app_yes_record")].contains(r#""op":"install""#));
        assert!(lines[at("app_yes_record")].contains(r#""grants":["hear","speak"]"#));
        assert!(lines[at(r#"helper {"install""#)].contains(&format!("/staging/{sha}.tar")));
    }

    #[test]
    fn a_package_that_is_not_the_one_listed_is_refused_before_anything_is_kept() {
        let (bytes, sha) = package_of(json!({"name": "x1", "version": "1.0.0", "grants": []}));
        for (id, version, sha256, why) in [
            ("x1", "1.0.0", "f".repeat(64), MISMATCH),
            ("x2", "1.0.0", sha.clone(), MISMATCH),
            ("x1", "1.0.1", sha.clone(), MISMATCH),
        ] {
            let fake = Arc::new(Fake {
                package: Some(bytes.clone()),
                ..Default::default()
            });
            let error =
                install_run(&Shared(fake.clone()), &asked_for(id, version, &sha256)).unwrap_err();
            assert_eq!(error, why);
            let lines = fake.lines.lock().unwrap().clone();
            assert!(
                !lines.iter().any(|l| l.starts_with("stage")
                    || l.starts_with("control")
                    || l.starts_with("helper")),
                "{lines:?}"
            );
        }
        let (junk, sha) = {
            let bytes = b"not a tar".to_vec();
            let sha = lamp_skills::package::sha256_hex(&bytes);
            (bytes, sha)
        };
        let fake = Arc::new(Fake {
            package: Some(junk),
            ..Default::default()
        });
        assert_eq!(
            install_run(&Shared(fake), &asked_for("x1", "1.0.0", &sha)).unwrap_err(),
            NOT_AN_APP
        );
    }

    #[test]
    fn an_update_records_the_yes_for_the_new_grants_only() {
        let (bytes, sha) = package_of(
            json!({"name": "english-lessons", "version": "1.3.0", "grants": ["speak", "hear", "light"]}),
        );
        let mut fake = Fake {
            package: Some(bytes),
            ..Default::default()
        };
        fake.apps.push(Fake::app("english-lessons"));
        let fake = Arc::new(fake);
        let done = install_run(
            &Shared(fake.clone()),
            &asked_for("english-lessons", "1.3.0", &sha),
        )
        .unwrap();
        assert_eq!(done["updated"], true);
        let lines = fake.lines.lock().unwrap().clone();
        let yes = lines.iter().find(|l| l.contains("app_yes_record")).unwrap();
        assert!(
            yes.contains(r#""op":"update""#) && yes.contains(r#""grants":["light"]"#),
            "{yes}"
        );
    }

    #[test]
    fn a_yes_that_is_refused_installs_nothing_and_the_staged_file_goes() {
        let (bytes, sha) = package_of(json!({"name": "x1", "version": "1.0.0", "grants": []}));
        let fake = Arc::new(Fake {
            package: Some(bytes),
            control_answer: Some(json!({"refused": "apps are off on this lamp"})),
            ..Default::default()
        });
        let error =
            install_run(&Shared(fake.clone()), &asked_for("x1", "1.0.0", &sha)).unwrap_err();
        assert_eq!(error, "apps are off on this lamp");
        let lines = fake.lines.lock().unwrap().clone();
        assert!(!lines.iter().any(|l| l.starts_with("helper")));
        assert!(lines.iter().any(|l| l.starts_with("unstage")));
    }

    #[test]
    fn an_install_request_is_held_to_its_four_fields() {
        let good = json!({"id": "x1", "url": "https://store.example/x.tar", "sha256": "a".repeat(64), "version": "1.0.0"});
        assert!(Asked::from_json(&good).is_ok());
        let with = |key: &str, value: Value| {
            let mut bad = good.clone();
            bad[key] = value;
            Asked::from_json(&bad)
        };
        assert!(with("id", json!("../x")).is_err());
        assert!(with("url", json!("http://store.example/x.tar")).is_err());
        assert!(with("url", json!("https://u@store.example/x.tar")).is_err());
        assert!(with("sha256", json!("abc")).is_err());
        assert!(with("sha256", json!("G".repeat(64))).is_err());
        assert!(with("version", json!("1.0")).is_err());
        assert!(with("extra", json!(1)).is_err());
        assert!(with("request_id", json!("r_1")).is_ok());
        assert!(Asked::from_json(&json!([])).is_err());
        let upper = with("sha256", json!("A".repeat(64))).unwrap();
        assert_eq!(upper.sha256, "a".repeat(64));
    }

    #[test]
    fn the_watcher_says_nothing_first_then_a_stop_once_and_a_panel_at_most_a_minute() {
        let watching = Mutex::new(Watch::default());
        let start = Instant::now();
        let mut fake = Fake::default();
        fake.apps.push(Fake::app("x1"));
        let fake = Arc::new(fake);
        let backend = Shared(fake.clone());
        // First look: only recorded (panel rev 3 in the fake, the app running).
        assert!(look(&backend, &watching, start).is_empty());
        // The app gives up: one app.stopped, and not again.
        struct Gave(Shared);
        impl Backend for Gave {
            fn names(&self) -> Vec<String> {
                self.0.names()
            }
            fn manifest(&self, n: &str) -> Option<Value> {
                self.0.manifest(n)
            }
            fn marks(&self, n: &str) -> Marks {
                self.0.marks(n)
            }
            fn unit(&self, _: &str) -> Unit {
                Unit {
                    active: false,
                    gave_up: true,
                }
            }
            fn package(&self, n: &str) -> Option<Package> {
                self.0.package(n)
            }
            fn helper(&self, l: &Value) -> Result<Value, String> {
                self.0.helper(l)
            }
            fn control(&self, l: &Value) -> Result<Value, String> {
                // The panel's revision moves at every ask.
                let _ = l;
                Ok(
                    json!({"settings": {}, "panel": {"title": "t", "lines": [], "rev": REV.fetch_add(1, Ordering::SeqCst)}, "socket": true}),
                )
            }
            fn download(&self, u: &str, m: u64) -> Result<Vec<u8>, String> {
                self.0.download(u, m)
            }
            fn staging(&self) -> PathBuf {
                self.0.staging()
            }
            fn stage(&self, p: &Path, b: &[u8]) -> Result<(), String> {
                self.0.stage(p, b)
            }
            fn unstage(&self, p: &Path) {
                self.0.unstage(p)
            }
        }
        static REV: AtomicUsize = AtomicUsize::new(10);
        let gave = Gave(backend);
        let first = look(&gave, &watching, start + Duration::from_secs(15));
        let kinds: Vec<&str> = first.iter().map(|o| o.kind.as_str()).collect();
        assert_eq!(kinds, [STOPPED_PUSH, PANEL_PUSH]);
        assert_eq!(
            first[0].result.as_ref().unwrap().as_ref().unwrap()["id"],
            "x1"
        );
        // The panel moved again within the minute: owed, not said; the stop is not said again.
        assert!(look(&gave, &watching, start + Duration::from_secs(30)).is_empty());
        let later = look(&gave, &watching, start + Duration::from_secs(80));
        assert_eq!(later.len(), 1);
        assert_eq!(later[0].kind, PANEL_PUSH);
    }

    #[test]
    fn results_owed_again_come_first_and_take_empties() {
        let apps = Apps::new(Box::new(Fake::default()));
        apps.done
            .lock()
            .unwrap()
            .push(push(PANEL_PUSH, json!({"id": "b"})));
        apps.owe(vec![push(STOPPED_PUSH, json!({"id": "a"}))]);
        let taken = apps.take();
        assert_eq!(taken[0].kind, STOPPED_PUSH);
        assert_eq!(taken[1].kind, PANEL_PUSH);
        assert!(apps.take().is_empty());
    }

    #[test]
    fn net_and_logs_are_in_the_yes_like_grants_an_update_only_for_what_it_newly_turns_on() {
        let yes_of = |manifest: Value, installed: bool| {
            let (bytes, sha) = package_of(manifest);
            let mut fake = Fake {
                package: Some(bytes),
                ..Default::default()
            };
            if installed {
                // Fake::app has net false and logs true.
                fake.apps.push(Fake::app("x1"));
            }
            let fake = Arc::new(fake);
            install_run(&Shared(fake.clone()), &asked_for("x1", "2.0.0", &sha)).unwrap();
            let lines = fake.lines.lock().unwrap().clone();
            lines
                .iter()
                .find(|l| l.contains("app_yes_record"))
                .unwrap()
                .clone()
        };
        let manifest = |net: bool, logs: bool| json!({"name": "x1", "version": "2.0.0", "grants": [], "net": net, "logs": logs});
        // An install says what it turns on.
        let line = yes_of(manifest(true, true), false);
        assert!(
            line.contains(r#""net":true"#) && line.contains(r#""logs":true"#),
            "{line}"
        );
        let line = yes_of(manifest(false, false), false);
        assert!(
            line.contains(r#""net":false"#) && line.contains(r#""logs":false"#),
            "{line}"
        );
        // An update turning the internet on says so, though it asks no new grant;
        // logs were on already, so they are not said again.
        let line = yes_of(manifest(true, true), true);
        assert!(
            line.contains(r#""op":"update""#) && line.contains(r#""grants":[]"#),
            "{line}"
        );
        assert!(
            line.contains(r#""net":true"#) && line.contains(r#""logs":false"#),
            "{line}"
        );
        // An update that turns nothing new on says nothing.
        let line = yes_of(manifest(false, true), true);
        assert!(
            line.contains(r#""net":false"#) && line.contains(r#""logs":false"#),
            "{line}"
        );
    }

    /// A backend whose lamp-control call is slow, or whose download panics.
    struct Trouble {
        inner: Shared,
        slow: bool,
    }

    impl Backend for Trouble {
        fn names(&self) -> Vec<String> {
            self.inner.names()
        }
        fn manifest(&self, n: &str) -> Option<Value> {
            self.inner.manifest(n)
        }
        fn marks(&self, n: &str) -> Marks {
            self.inner.marks(n)
        }
        fn unit(&self, n: &str) -> Unit {
            self.inner.unit(n)
        }
        fn package(&self, n: &str) -> Option<Package> {
            self.inner.package(n)
        }
        fn helper(&self, l: &Value) -> Result<Value, String> {
            self.inner.helper(l)
        }
        fn control(&self, l: &Value) -> Result<Value, String> {
            if self.slow {
                std::thread::sleep(Duration::from_millis(500));
            }
            self.inner.control(l)
        }
        fn download(&self, u: &str, m: u64) -> Result<Vec<u8>, String> {
            if !self.slow {
                panic!("the download fell over");
            }
            self.inner.download(u, m)
        }
        fn staging(&self) -> PathBuf {
            self.inner.staging()
        }
        fn stage(&self, p: &Path, b: &[u8]) -> Result<(), String> {
            self.inner.stage(p, b)
        }
        fn unstage(&self, p: &Path) {
            self.inner.unstage(p)
        }
    }

    #[test]
    fn a_tick_never_waits_for_a_slow_look() {
        let mut fake = Fake::default();
        fake.apps.push(Fake::app("x1"));
        let apps = Apps::new(Box::new(Trouble {
            inner: Shared(std::sync::Arc::new(fake)),
            slow: true,
        }));
        let start = Instant::now();
        apps.watch(start);
        // The look is now inside its 500 ms control call.
        std::thread::sleep(Duration::from_millis(100));
        let asked = Instant::now();
        apps.watch(start + Duration::from_secs(20));
        assert!(
            // The look sleeps 500 ms inside control(): a waiting tick would
            // take ~400 ms, so 100 ms is far from both and safe on a busy runner.
            asked.elapsed() < Duration::from_millis(100),
            "watch waited {:?}",
            asked.elapsed()
        );
    }

    #[test]
    fn a_panicking_install_gives_its_slot_back_says_so_and_clears_the_staged_file() {
        let mut fake = Fake::default();
        fake.apps.push(Fake::app("x1"));
        let fake = std::sync::Arc::new(fake);
        let apps = Apps::new(Box::new(Trouble {
            inner: Shared(fake.clone()),
            slow: false,
        }));
        let good = json!({"id": "x1", "url": "https://store.example/x.tar", "sha256": "a".repeat(64), "version": "1.0.0", "request_id": "r1"});
        apps.install(&good).unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        let mut told = Vec::new();
        while told.is_empty() {
            assert!(Instant::now() < end, "no answer after a panic");
            std::thread::sleep(Duration::from_millis(5));
            told = apps.take();
        }
        assert_eq!(told[0].kind, INSTALL_KIND);
        assert_eq!(told[0].request_id.as_deref(), Some("r1"));
        assert_eq!(told[0].result.as_ref().unwrap_err(), FELL_OVER);
        let lines = fake.lines.lock().unwrap().clone();
        assert!(
            lines.iter().any(
                |l| l.starts_with("unstage ") && l.contains(&format!("{}.tar", "a".repeat(64)))
            ),
            "{lines:?}"
        );
        assert_eq!(apps.under_way.load(Ordering::Acquire), 0);
        assert!(apps.install(&good).is_ok(), "the next install is not BUSY");
    }

    #[test]
    fn a_remove_is_refused_while_an_install_or_remove_is_under_way() {
        for marks in [
            Marks {
                removing: true,
                ..Marks::default()
            },
            Marks {
                installing: true,
                ..Marks::default()
            },
        ] {
            let mut fake = Fake::default();
            let mut app = Fake::app("x1");
            app.2 = marks;
            fake.apps.push(app);
            let fake = std::sync::Arc::new(fake);
            let apps = Apps::new(Box::new(Shared(fake.clone())));
            let err = apps.answer(REMOVE_KIND, &json!({"id": "x1"})).unwrap_err();
            assert_eq!(err, UNDER_WAY);
            assert!(
                fake.lines.lock().unwrap().is_empty(),
                "nothing asked: {:?}",
                fake.lines.lock().unwrap()
            );
        }
    }

    #[test]
    fn settings_get_says_a_lamp_control_that_refuses_and_does_not_show_nothing_set() {
        let mut fake = Fake::default();
        fake.apps.push(Fake::app("x1"));
        fake.control_answer = Some(json!({"refused": "apps are off on this lamp"}));
        // app_state is answered by the fake's own branch: use a line it does not know.
        let fake = std::sync::Arc::new(fake);
        let apps = Apps::new(Box::new(Refusing(Shared(fake))));
        let err = apps
            .answer(SETTINGS_GET_KIND, &json!({"id": "x1"}))
            .unwrap_err();
        assert!(err.contains("off on this lamp"), "{err}");
    }

    /// A backend whose lamp-control always refuses.
    struct Refusing(Shared);

    impl Backend for Refusing {
        fn names(&self) -> Vec<String> {
            self.0.names()
        }
        fn manifest(&self, n: &str) -> Option<Value> {
            self.0.manifest(n)
        }
        fn marks(&self, n: &str) -> Marks {
            self.0.marks(n)
        }
        fn unit(&self, n: &str) -> Unit {
            self.0.unit(n)
        }
        fn package(&self, n: &str) -> Option<Package> {
            self.0.package(n)
        }
        fn helper(&self, l: &Value) -> Result<Value, String> {
            self.0.helper(l)
        }
        fn control(&self, _: &Value) -> Result<Value, String> {
            Ok(json!({"refused": "apps are off on this lamp"}))
        }
        fn download(&self, u: &str, m: u64) -> Result<Vec<u8>, String> {
            self.0.download(u, m)
        }
        fn staging(&self) -> PathBuf {
            self.0.staging()
        }
        fn stage(&self, p: &Path, b: &[u8]) -> Result<(), String> {
            self.0.stage(p, b)
        }
        fn unstage(&self, p: &Path) {
            self.0.unstage(p)
        }
    }

    #[test]
    fn the_installed_packages_logs_are_the_helpers_records_not_the_manifests() {
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("x1").join("current");
        std::fs::create_dir_all(&current).unwrap();
        // The manifest says logs; the helper's record (an earlier build's) has none.
        std::fs::write(current.join(MANIFEST_FILE), r#"{"logs": true}"#).unwrap();
        let record = |logs: &str| {
            format!(
                r#"{{"version":"1.0.0","grants":[],"net":false,{logs}"sha256":"{}"}}"#,
                "c".repeat(64)
            )
        };
        let real = Real {
            layout: lamp_apps::layout::Layout {
                code: dir.path().to_owned(),
                data: dir.path().to_owned(),
                private: dir.path().to_owned(),
                state: dir.path().to_owned(),
                staging: dir.path().to_owned(),
                lock_wait: Duration::from_secs(1),
            },
            helper: "/nowhere".into(),
            control: "/nowhere".into(),
        };
        std::fs::write(current.join(RECORD_FILE), record("")).unwrap();
        assert!(!real.package("x1").unwrap().logs);
        std::fs::write(current.join(RECORD_FILE), record(r#""logs":true,"#)).unwrap();
        assert!(real.package("x1").unwrap().logs);
    }
}
