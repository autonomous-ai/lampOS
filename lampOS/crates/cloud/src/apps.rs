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
use serde_json::{Map, Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

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

/// Every kind this part answers, after [`CAN`].
pub const KINDS: [&str; 6] = [
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
}

/// What the broker asks of: the backend, and nothing else.
pub struct Apps {
    backend: Box<dyn Backend>,
}

impl Apps {
    pub fn new(backend: Box<dyn Backend>) -> Self {
        Self { backend }
    }

    /// The product's, with the switch on.
    pub fn from_environment() -> Option<Self> {
        (std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on"))
            .then(|| Self::new(Box::new(Real::product())))
    }

    pub fn answer(&self, kind: &str, data: &Value) -> Result<Option<Value>, String> {
        match kind {
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
            return Err("the app is being installed or removed: try again in a moment".into());
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
        let values = match self.backend.control(&Wire::state(&name)) {
            Ok(kept) => kept.get("settings").cloned().unwrap_or(Value::Null),
            Err(_) => Value::Null,
        };
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
        })
    }

    fn helper(&self, line: &Value) -> Result<Value, String> {
        ask(&self.helper, line, HELPER_WAIT, HELPER_UNREACHED)
    }

    fn control(&self, line: &Value) -> Result<Value, String> {
        ask(&self.control, line, CONTROL_WAIT, CONTROL_UNREACHED)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A fake backend: installed apps by name, and every line asked of
    /// either socket, in order.
    #[derive(Default)]
    pub struct Fake {
        pub apps: Vec<(String, Value, Marks, Unit, Option<Package>)>,
        pub lines: Mutex<Vec<String>>,
        pub helper_answer: Option<Value>,
        pub control_answer: Option<Value>,
        pub helper_down: bool,
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
        struct Shared(std::sync::Arc<Fake>);
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
        }
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
        struct Shared(std::sync::Arc<Fake>);
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
        }
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
    /// and that a remove's key has none of grants, `net` and `logs`. On a
    /// lamp-voice without `app_yes.rs` yet (#7714) there is nothing to compare
    /// with, and it says so.
    #[test]
    fn the_yes_record_is_what_lamp_controls_key_of_takes() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../control/src/app_yes.rs");
        let Ok(source) = std::fs::read_to_string(path) else {
            eprintln!("skipped: lamp-control's app_yes.rs is not here yet (#7714)");
            return;
        };
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
}
