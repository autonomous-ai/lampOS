//! What lamp-control tells the voice of the running apps' tools (#8386,
//! `docs/app-tools.md`, `LAMP_APP_TOOLS`, off): each running app's tools
//! file, the one its manifest names in `tools`, read from the app's
//! installed folder once a session, held to the Store's checks as
//! `lamp_msgs::app_tools` writes them (the one source of their bounds), and
//! said on `apps/tools` at each change and every 5 s, as `apps/triggers` is.
//!
//! A bad file (a path out of the app's folder, too big, not the format, a
//! name, description or schema the checks refuse) leaves that app with no
//! tools, with one journal line naming why, and the app installs and runs as
//! any other. Only apps with a socket up are told. The call itself
//! (`apps/call`, `app.call`) is not here. With the switch off nothing here
//! starts, and a manifest naming `tools` is read as no app's at all.
use crate::server::Shared;
use lamp_msgs::{AppTool, AppTools, AppsTools};
use lamp_skills::app_package::tools_path_ok;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The switch: `on` only, off unless set (lamp-voice reads the same name).
pub const ENVIRONMENT: &str = lamp_skills::app_package::TOOLS_ENVIRONMENT;

/// Whether [`ENVIRONMENT`] is `on` in this process.
pub fn on() -> bool {
    lamp_skills::app_package::tools_on()
}

/// The most a tools file may be, in bytes (the Store's 8 KiB).
pub const FILE_MOST_BYTES: u64 = 8 * 1024;

/// How often the list is said again when nothing changed (the topic is
/// read as old after 30 s, `AppTools::declaration`).
const SAY_EVERY: Duration = Duration::from_secs(5);
/// How often it is looked at for a change.
const LOOK_EVERY: Duration = Duration::from_millis(500);

/// The tools file: the skills' format, `{"tools": [{"name",
/// "description", "parameters"}]}`, plus the file's own `outside` (tool
/// names that act outside the lamp) and `confirm` (their read-back lines).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    tools: Vec<FileTool>,
    #[serde(default)]
    outside: Vec<String>,
    #[serde(default)]
    confirm: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileTool {
    name: String,
    description: String,
    #[serde(default)]
    parameters: Option<Value>,
}

/// `title` cut to the topic's bound on a character's edge, one line.
fn title_of(title: &str) -> String {
    let line = title.lines().next().unwrap_or_default();
    let mut end = line.len().min(lamp_msgs::TITLE_MOST_BYTES);
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    line[..end].to_owned()
}

/// The tools of app `app` (titled `title`), from its tools file at `path`
/// inside its installed folder `folder`, checked as `apps/tools` checks
/// them; or why there are none.
pub fn read(folder: &Path, app: &str, title: &str, path: &str) -> Result<AppsTools, String> {
    if !tools_path_ok(path) {
        return Err(
            "its path is not tools.json or a .json file under src/ in the app's folder".into(),
        );
    }
    let at = folder.join(path);
    // Never through a link: the file the helper laid, or none.
    let kind = std::fs::symlink_metadata(&at).map_err(|error| format!("not read: {error}"))?;
    if !kind.is_file() {
        return Err("not a plain file".into());
    }
    if kind.len() > FILE_MOST_BYTES {
        return Err(format!("over {FILE_MOST_BYTES} bytes"));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&at)
        .and_then(|file| file.take(FILE_MOST_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| format!("not read: {error}"))?;
    if bytes.len() as u64 > FILE_MOST_BYTES {
        return Err(format!("over {FILE_MOST_BYTES} bytes"));
    }
    let file: File =
        serde_json::from_slice(&bytes).map_err(|error| format!("not a tools file: {error}"))?;
    let named = |name: &String| file.tools.iter().any(|tool| &tool.name == name);
    if !file.outside.iter().all(named) {
        return Err("outside names a tool the file does not have".into());
    }
    if !file.confirm.keys().all(|name| file.outside.contains(name)) {
        return Err("confirm names a tool not in outside".into());
    }
    let tools = AppsTools {
        app: app.to_owned(),
        title: title_of(title),
        since_ms: 0,
        tools: file
            .tools
            .into_iter()
            .map(|tool| AppTool {
                outside: file.outside.contains(&tool.name),
                name: tool.name,
                description: tool.description,
                parameters: tool.parameters,
            })
            .collect(),
    };
    AppTools {
        apps: vec![tools.clone()],
    }
    .check()
    .map_err(|why| why.0)?;
    Ok(tools)
}

/// The running apps' tools, each app's file read once a session (its
/// `since_ms`) and journalled once: a file is laid by an install or an
/// update, which starts the app's session again.
#[derive(Default)]
pub struct Lister {
    read: BTreeMap<String, (u64, Option<AppsTools>)>,
}

impl Lister {
    /// The tools of `running` apps (name and session start), from the
    /// manifests under `apps`; `tools` is the switch. Apps with none, or
    /// with a bad file, are left out.
    pub fn list(&mut self, apps: &Path, running: &[(String, u64)], tools: bool) -> AppTools {
        self.read.retain(|name, (since, _)| {
            running
                .iter()
                .any(|(running, at)| running == name && at == since)
        });
        let mut list = AppTools { apps: Vec::new() };
        if !tools {
            return list;
        }
        for (name, since) in running {
            let (_, kept) = self
                .read
                .entry(name.clone())
                .or_insert_with(|| (*since, read_installed(apps, name)));
            if let Some(app) = kept
                && list.apps.len() < AppTools::MAX_APPS
            {
                list.apps.push(AppsTools {
                    since_ms: *since,
                    ..app.clone()
                });
            }
        }
        list
    }
}

/// An installed app's tools, or none; a bad file is said once, here.
fn read_installed(apps: &Path, name: &str) -> Option<AppsTools> {
    let manifest = crate::app_yes::installed_switched(apps, name, true)?;
    let path = manifest.tools.as_deref()?;
    match read(
        &apps.join(name).join("current"),
        name,
        &manifest.title,
        path,
    ) {
        Ok(app) if app.tools.is_empty() => None,
        Ok(app) => {
            eprintln!(
                "lamp-control: app {name}: {} tools read from its tools file",
                app.tools.len()
            );
            Some(app)
        }
        Err(why) => {
            eprintln!("lamp-control: app {name}: no tools: its tools file {why}");
            None
        }
    }
}

/// Starts the bus side: the running apps' tools said on `apps/tools` as
/// they change and every 5 s. With no publisher, said once and nothing more.
pub fn start(bus: &lamp_bus::Bus, shared: Arc<Shared>) {
    let mut publisher =
        match bus.publisher_declared(AppTools::TOPIC, &AppTools::declaration(), "control") {
            Ok(publisher) => publisher,
            Err(error) => {
                eprintln!("lamp-control: no app tools are told: {error}");
                return;
            }
        };
    let spawned = std::thread::Builder::new()
        .name("app-tools".into())
        .spawn(move || {
            let mut lister = Lister::default();
            let mut said: Option<(AppTools, Instant)> = None;
            loop {
                let now = Instant::now();
                let (running, apps_dir) = shared.running_since();
                let list = lister.list(&apps_dir, &running, true);
                let due = match &said {
                    Some((last, at)) => *last != list || now.duration_since(*at) >= SAY_EVERY,
                    None => true,
                };
                if due && list.check().is_ok() {
                    match publisher.publish_json(lamp_ipc::monotonic_us(), &list) {
                        Ok(_) => said = Some((list, now)),
                        Err(error) => {
                            eprintln!("lamp-control: the apps' tools were not told: {error}")
                        }
                    }
                }
                std::thread::sleep(LOOK_EVERY);
            }
        });
    if let Err(error) = spawned {
        eprintln!("lamp-control: no app tools are told: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// An installed app `name` under a fresh `apps` folder, its manifest
    /// naming `tools` (or none), and its tools file holding `file`.
    fn installed(tag: &str, tools: Option<&str>, file: Option<&[u8]>) -> std::path::PathBuf {
        // Each its own folder: the tests run at once.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let apps =
            std::env::temp_dir().join(format!("lampos-app-tools-{tag}-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&apps);
        let dir = apps.join("gym-coach").join("current");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let mut manifest = json!({
            "format": 2, "kind": "app", "name": "gym-coach", "title": "Gym Coach", "version": "2.0.0",
            "min_build": 1, "runtime": {"name": "python3", "entry": "coach"},
            "grants": ["hear"], "triggers": [], "files": []
        });
        if let Some(tools) = tools {
            manifest["tools"] = json!(tools);
        }
        std::fs::write(dir.join("lamp-skill.json"), manifest.to_string()).unwrap();
        if let (Some(path), Some(file)) = (tools, file) {
            std::fs::write(dir.join(path), file).unwrap();
        }
        apps
    }

    fn coach_file() -> Value {
        json!({
            "tools": [
                {"name": "set_plan", "description": "Set the weekly training plan.",
                 "parameters": {"type": "object",
                    "properties": {"days": {"type": "integer", "minimum": 1, "maximum": 7}},
                    "required": ["days"]}},
                {"name": "book_class", "description": "Book a class at the gym."}
            ],
            "outside": ["book_class"],
            "confirm": {"book_class": "Gym Coach will book a class. Shall I?"}
        })
    }

    fn read_file(file: &Value) -> Result<AppsTools, String> {
        let apps = installed(
            "read",
            Some("tools.json"),
            Some(file.to_string().as_bytes()),
        );
        read(
            &apps.join("gym-coach/current"),
            "gym-coach",
            "Gym Coach",
            "tools.json",
        )
    }

    #[test]
    fn a_right_file_reads_as_the_voice_checks_it_outside_marked() {
        let app = read_file(&coach_file()).unwrap();
        assert_eq!(app.tools.len(), 2);
        assert_eq!(app.tools[0].name, "set_plan");
        assert!(!app.tools[0].outside && app.tools[1].outside);
        assert_eq!(AppTools { apps: vec![app] }.check(), Ok(()));
        // A file under src/ too.
        let apps = installed(
            "src",
            Some("src/tools.json"),
            Some(coach_file().to_string().as_bytes()),
        );
        let app = read(
            &apps.join("gym-coach/current"),
            "gym-coach",
            "Gym Coach",
            "src/tools.json",
        );
        assert_eq!(app.unwrap().tools.len(), 2);
    }

    #[test]
    fn a_path_out_of_the_apps_folder_or_a_link_is_never_read() {
        let apps = installed(
            "path",
            Some("tools.json"),
            Some(coach_file().to_string().as_bytes()),
        );
        let folder = apps.join("gym-coach/current");
        for path in [
            "../tools.json",
            "/etc/passwd",
            "src/../../x.json",
            "other.json",
            ".tools.json",
        ] {
            let why = read(&folder, "gym-coach", "Gym Coach", path).unwrap_err();
            assert!(why.contains("its path is not"), "{path}: {why}");
        }
        // A link in the file's place is not followed.
        std::fs::remove_file(folder.join("tools.json")).unwrap();
        std::fs::write(apps.join("elsewhere.json"), coach_file().to_string()).unwrap();
        std::os::unix::fs::symlink(apps.join("elsewhere.json"), folder.join("tools.json")).unwrap();
        let why = read(&folder, "gym-coach", "Gym Coach", "tools.json").unwrap_err();
        assert_eq!(why, "not a plain file");
        // A missing file is no tools.
        std::fs::remove_file(folder.join("tools.json")).unwrap();
        assert!(read(&folder, "gym-coach", "Gym Coach", "tools.json").is_err());
    }

    #[test]
    fn an_oversize_file_is_refused_before_it_is_parsed() {
        let mut file = coach_file();
        file["tools"][0]["description"] = json!("x".repeat(FILE_MOST_BYTES as usize));
        let why = read_file(&file).unwrap_err();
        assert_eq!(why, "over 8192 bytes");
    }

    #[test]
    fn each_of_the_stores_checks_leaves_the_app_with_no_tools_and_says_why() {
        let with = |change: &dyn Fn(&mut Value)| {
            let mut file = coach_file();
            change(&mut file);
            read_file(&file)
        };
        type Change<'a> = &'a dyn Fn(&mut Value);
        let cases: [(Change, &str); 9] = [
            (
                &|f| f["tools"][0]["name"] = json!("Set-Plan"),
                "a tool's name is not",
            ),
            (
                &|f| f["tools"][0]["name"] = json!("a".repeat(31)),
                "a tool's name is not",
            ),
            // Thirty is the most a name may be, and gym_coach_ fits it.
            (&|f| f["tools"][0]["name"] = json!("a".repeat(30)), ""),
            (
                &|f| f["tools"][0]["description"] = json!("x".repeat(151)),
                "plain words of at most 150",
            ),
            (
                &|f| f["tools"][0]["description"] = json!("Say \"yes\""),
                "plain words",
            ),
            (
                &|f| {
                    f["tools"][0]["parameters"] =
                        json!({"type": "object", "properties": {"x": {"type": "object"}}})
                },
                "a property is",
            ),
            (
                &|f| {
                    f["tools"][1]["name"] = json!("set_plan");
                    f["outside"] = json!([]);
                    f["confirm"] = json!({});
                },
                "repeats",
            ),
            (&|f| f["outside"] = json!(["nope"]), "outside names a tool"),
            (
                &|f| f["extra"] = json!(1),
                "not a tools file: unknown field",
            ),
        ];
        for (change, words) in cases {
            match with(change) {
                Ok(app) if words.is_empty() => assert_eq!(app.tools.len(), 2),
                Ok(app) => panic!("read, and should be refused ({words}): {app:?}"),
                Err(why) => assert!(why.contains(words), "{why:?} lacks {words:?}"),
            }
        }
        // App and tool together past 47 bytes: no tools.
        let apps = installed(
            "long",
            Some("tools.json"),
            Some(coach_file().to_string().as_bytes()),
        );
        let folder = apps.join("gym-coach/current");
        let long = "a".repeat(24);
        let mut file = coach_file();
        file["tools"][0]["name"] = json!("b".repeat(24));
        std::fs::write(folder.join("tools.json"), file.to_string()).unwrap();
        let why = read(&folder, &long, "Long", "tools.json").unwrap_err();
        assert!(why.contains("over 47 bytes"), "{why}");
        // Nine tools: past the eight an app.
        let mut file = coach_file();
        file["outside"] = json!([]);
        file["confirm"] = json!({});
        file["tools"] = json!(
            (0..9)
                .map(|n| json!({"name": format!("t{n}"), "description": "Do a thing."}))
                .collect::<Vec<_>>()
        );
        assert!(read_file(&file).unwrap_err().contains("more than 8"));
        // Not JSON.
        std::fs::write(folder.join("tools.json"), b"not json").unwrap();
        let why = read(&folder, "gym-coach", "Gym Coach", "tools.json").unwrap_err();
        assert!(why.starts_with("not a tools file"), "{why}");
    }

    #[test]
    fn the_lister_reads_a_file_once_a_session_and_leaves_a_bad_one_out() {
        let apps = installed(
            "lister",
            Some("tools.json"),
            Some(coach_file().to_string().as_bytes()),
        );
        let mut lister = Lister::default();
        let running = vec![("gym-coach".to_owned(), 7)];
        let list = lister.list(&apps, &running, true);
        assert_eq!(list.apps.len(), 1);
        assert_eq!(list.apps[0].since_ms, 7);
        // Broken mid-session: the session's read stands.
        let file = apps.join("gym-coach/current/tools.json");
        std::fs::write(&file, b"{}").unwrap();
        assert_eq!(lister.list(&apps, &running, true), list);
        // A new session reads again: bad, so the app has none and still runs.
        let again = vec![("gym-coach".to_owned(), 8)];
        assert!(lister.list(&apps, &again, true).apps.is_empty());
        // An app not running is never told.
        assert!(lister.list(&apps, &[], true).apps.is_empty());
        // No `tools` in the manifest: none.
        let plain = installed("plain", None, None);
        assert!(lister.list(&plain, &running, true).apps.is_empty());
    }

    #[test]
    fn with_the_switch_off_nothing_is_listed_and_a_tools_manifest_is_no_apps() {
        let apps = installed(
            "off",
            Some("tools.json"),
            Some(coach_file().to_string().as_bytes()),
        );
        let running = vec![("gym-coach".to_owned(), 7)];
        assert!(
            Lister::default()
                .list(&apps, &running, false)
                .apps
                .is_empty()
        );
        assert!(crate::app_yes::installed_switched(&apps, "gym-coach", false).is_none());
        assert!(crate::app_yes::installed_switched(&apps, "gym-coach", true).is_some());
        // A manifest without `tools` reads the same either way.
        let plain = installed("off-plain", None, None);
        assert!(crate::app_yes::installed_switched(&plain, "gym-coach", false).is_some());
    }

    #[test]
    fn a_long_title_is_cut_on_a_characters_edge() {
        let title = "é".repeat(40);
        let cut = title_of(&title);
        assert!(cut.len() <= lamp_msgs::TITLE_MOST_BYTES && title.starts_with(&cut));
        assert_eq!(title_of("Gym Coach"), "Gym Coach");
    }
}
