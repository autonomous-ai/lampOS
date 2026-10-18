//! A Live session's functions against what would be declared now (#1472).
//!
//! A session is told its functions once, when it connects
//! ([`crate::thread::Recalling`]). The executive's catalog says only what
//! can be done now, so it changes: the light's functions drop out while the
//! voice, whose ring the light is, is down, and come back a second or two
//! after it starts. A session that connected in that gap had no light
//! functions for its whole life, and the lamp said "I can only turn it on
//! or off" (lamp-4ace, 18 Oct 11:22).
//!
//! So what each session was given is kept here, the catalogs are looked at
//! every [`LOOK_EVERY`] (lamp-actions' own `LOOK_AGAIN`), read again only
//! when a file's time changed, and when what would be declared now differs
//! from what the session was given, the provider renews the session
//! between turns, on the person switch's path, and the next one is opened
//! fresh (the first builder on #1472: Google's resumption with other
//! functions is not tested on a person's lamp). At most once per
//! [`RENEW_AT_MOST_EVERY`], so a catalog that comes and goes cannot churn
//! sessions.
use crate::tools::{CATALOG_FILE, SKILLS_DIR, Settings, declared};
use lamp_gemini::Tool;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

/// How often the catalogs are looked at: lamp-actions rewrites its own this
/// often at most (`LOOK_AGAIN`).
pub const LOOK_EVERY: Duration = Duration::from_secs(2);
/// A session is renewed for a changed catalog at most this often.
pub const RENEW_AT_MOST_EVERY: Duration = Duration::from_secs(30);

/// What a worker's newest session was given, and whether the next one is
/// to be opened fresh: shared by its sessions' [`crate::thread::Recalling`]
/// and its provider loop, so no other worker's (or test's) sessions count.
/// With how often the catalogs are looked at, and where the loop reads
/// whether a conversation is going on.
#[derive(Debug)]
pub struct Given {
    names: Mutex<Option<BTreeSet<String>>>,
    fresh_next: AtomicBool,
    pub look_every: Duration,
    pub going_on: fn() -> bool,
}

impl Default for Given {
    fn default() -> Self {
        Self::checked(LOOK_EVERY, conversation_going_on)
    }
}

/// The voice's own thread: the lamp spoke in the last 45 s.
fn conversation_going_on() -> bool {
    crate::thread::THREAD
        .lock()
        .is_ok_and(|thread| thread.going_on(Instant::now()))
}

impl Given {
    /// Looked at every `look_every`, the conversation read from `going_on`.
    pub fn checked(look_every: Duration, going_on: fn() -> bool) -> Self {
        Self {
            names: Mutex::new(None),
            fresh_next: AtomicBool::new(false),
            look_every,
            going_on,
        }
    }

    /// Kept when a session is equipped.
    pub fn record(&self, tools: &[Tool]) {
        if let Ok(mut given) = self.names.lock() {
            *given = Some(names(tools));
        }
    }

    /// What the newest session was given, if one has been equipped.
    pub fn now(&self) -> Option<BTreeSet<String>> {
        self.names.lock().ok().and_then(|given| given.clone())
    }

    /// Ask for the next session to be opened fresh.
    pub fn open_fresh(&self) {
        self.fresh_next.store(true, Ordering::Release);
    }

    /// Whether the session being opened is to be fresh; asked once.
    pub fn take_fresh(&self) -> bool {
        self.fresh_next.swap(false, Ordering::AcqRel)
    }
}

pub fn names(tools: &[Tool]) -> BTreeSet<String> {
    tools.iter().map(|tool| tool.name.clone()).collect()
}

/// What would be declared now and a session was not given, and the other
/// way round.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Change {
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

impl Change {
    fn between(given: &BTreeSet<String>, now: &BTreeSet<String>) -> Option<Self> {
        let change = Self {
            added: now.difference(given).cloned().collect(),
            removed: given.difference(now).cloned().collect(),
        };
        (!change.added.is_empty() || !change.removed.is_empty()).then_some(change)
    }
}

impl std::fmt::Display for Change {
    /// `+8 (light_on, light_off, …) -0`: counts, and the first few names.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let some = |names: &[String]| {
            let shown: Vec<&str> = names.iter().take(3).map(String::as_str).collect();
            match names.len() {
                0 => String::new(),
                n if n > 3 => format!(" ({}, …)", shown.join(", ")),
                _ => format!(" ({})", shown.join(", ")),
            }
        };
        write!(
            f,
            "+{}{} -{}{}",
            self.added.len(),
            some(&self.added),
            self.removed.len(),
            some(&self.removed)
        )
    }
}

/// The catalogs looked at for a change.
pub struct Watch {
    look_every: Duration,
    files: [PathBuf; 2],
    times: Option<[Option<SystemTime>; 2]>,
    now: BTreeSet<String>,
    looked_at: Option<Instant>,
    renewed_at: Option<Instant>,
}

impl Watch {
    /// With an executive only: without one there is no catalog to change.
    pub fn new(settings: &Settings, look_every: Duration) -> Option<Self> {
        let directory = settings.exec.as_ref()?;
        Some(Self {
            look_every,
            files: [
                directory.join(CATALOG_FILE),
                directory.join(SKILLS_DIR).join(CATALOG_FILE),
            ],
            times: None,
            now: BTreeSet::new(),
            looked_at: None,
            renewed_at: None,
        })
    }

    /// How what would be declared now differs from what the newest session
    /// was `given`, looked at once every `look_every` (`None` between looks,
    /// with `given` not touched); the catalogs are read only when one's
    /// time changed since the last look.
    pub fn changed(&mut self, settings: &Settings, given: &Given, now: Instant) -> Option<Change> {
        if self
            .looked_at
            .is_some_and(|at| now.saturating_duration_since(at) < self.look_every)
        {
            return None;
        }
        self.looked_at = Some(now);
        let times = self.files.clone().map(|file| {
            std::fs::metadata(file)
                .and_then(|meta| meta.modified())
                .ok()
        });
        if self.times != Some(times) {
            self.times = Some(times);
            self.now = names(&declared(settings));
        }
        Change::between(&given.now()?, &self.now)
    }

    /// Whether a renewal for a changed catalog may be asked now.
    pub fn may_renew(&self, now: Instant) -> bool {
        self.renewed_at
            .is_none_or(|at| now.saturating_duration_since(at) >= RENEW_AT_MOST_EVERY)
    }

    pub fn renewed(&mut self, now: Instant) {
        self.renewed_at = Some(now);
    }
}

/// Whether to renew the session for a changed catalog now: only between
/// turns (none open, no output owed, no conversation going on, no look
/// owed), once for a live session, and within the bound.
pub struct Moment {
    pub turn_open: bool,
    pub output_owed: bool,
    pub going_on: bool,
    pub look_owed: bool,
    pub live_already_asked: bool,
    pub may_renew: bool,
}

impl Moment {
    pub fn due(&self) -> bool {
        !self.turn_open
            && !self.output_owed
            && !self.going_on
            && !self.look_owed
            && !self.live_already_asked
            && self.may_renew
    }
}

/// The journal's line for a session's setup: how many functions it was
/// given, the executive's among them by kind, and, when it has none of
/// the light's, that the ring is not up (#1472).
pub fn setup_line(tools: &[Tool], settings: &Settings) -> String {
    let mut line = format!("lamp-voice: session functions: {} declared", tools.len());
    let Some(directory) = &settings.exec else {
        return line;
    };
    if !directory.join(CATALOG_FILE).exists() {
        line.push_str("; the executive's catalog is not there");
        return line;
    }
    let executive = read_names(&directory.join(CATALOG_FILE));
    let given = names(tools);
    let mine: Vec<&String> = executive
        .iter()
        .filter(|name| given.contains(*name))
        .collect();
    let light = mine
        .iter()
        .filter(|name| name.starts_with("light_"))
        .count();
    let volume = mine
        .iter()
        .filter(|name| name.starts_with("volume_"))
        .count();
    line.push_str(&format!(
        "; the executive's {} (light {light}, volume {volume}, other {})",
        mine.len(),
        mine.len() - light - volume
    ));
    if light == 0 {
        line.push_str("; no light functions: the ring is not up");
    }
    line
}

/// The names in a catalog file, none if it cannot be read.
fn read_names(path: &std::path::Path) -> BTreeSet<String> {
    #[derive(serde::Deserialize)]
    struct Catalog {
        tools: Vec<Named>,
    }
    #[derive(serde::Deserialize)]
    struct Named {
        name: String,
    }
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Catalog>(&text).ok())
        .map(|catalog| catalog.tools.into_iter().map(|tool| tool.name).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Folder(PathBuf);
    impl Folder {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("lv-st-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn tool(name: &str) -> serde_json::Value {
        json!({"name": name, "description": format!("Do {name}.")})
    }

    /// The executive's catalog as lamp-actions writes it, at a time of its
    /// own so a rewrite inside the same second is still seen.
    fn write(folder: &Folder, names: &[&str], seconds: u64) {
        let path = folder.0.join(CATALOG_FILE);
        let tools: Vec<_> = names.iter().map(|name| tool(name)).collect();
        std::fs::write(&path, json!({"tools": tools}).to_string()).unwrap();
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + seconds))
            .unwrap();
    }

    fn settings(folder: &Folder) -> Settings {
        Settings {
            decline: true,
            exec: Some(folder.0.clone()),
            ..Default::default()
        }
    }

    const SOUND: [&str; 3] = ["volume_up", "volume_down", "volume_mute"];
    const LIGHT: [&str; 3] = ["light_on", "light_off", "light_colour"];

    /// #1472's case: a session equipped while the light's functions were
    /// out of the catalog, then the catalog rewritten with them. The next
    /// look says what changed; nothing is said between looks, nor once the
    /// session given them is the newest.
    #[test]
    fn a_catalog_rewritten_after_a_session_was_equipped_is_seen_at_the_next_look() {
        let folder = Folder::new("rewritten");
        let settings = settings(&folder);
        write(&folder, &SOUND, 0);
        let session = declared(&settings);
        assert!(
            names(&session)
                .iter()
                .all(|name| !name.starts_with("light_"))
        );
        let given = Given::default();
        given.record(&session);
        let mut watch = Watch::new(&settings, LOOK_EVERY).unwrap();
        let start = Instant::now();
        assert_eq!(watch.changed(&settings, &given, start), None);
        let both: Vec<&str> = SOUND.iter().chain(LIGHT.iter()).copied().collect();
        write(&folder, &both, 1);
        // Not looked at again before its time.
        assert_eq!(
            watch.changed(&settings, &given, start + LOOK_EVERY / 2),
            None
        );
        let change = watch
            .changed(&settings, &given, start + LOOK_EVERY)
            .unwrap();
        let mut light: Vec<String> = LIGHT.iter().map(|name| (*name).to_owned()).collect();
        light.sort();
        assert_eq!(change.added, light);
        assert!(change.removed.is_empty());
        assert_eq!(
            change.to_string(),
            "+3 (light_colour, light_off, light_on) -0"
        );
        // The session opened next is given them: nothing more to change.
        let renewed = Given::default();
        renewed.record(&declared(&settings));
        assert!(renewed.now().unwrap().contains("light_colour"));
        assert_eq!(
            watch.changed(&settings, &renewed, start + LOOK_EVERY * 2),
            None
        );
        // The ring going down takes them out again, and that is a change too.
        write(&folder, &SOUND, 2);
        let change = watch
            .changed(&settings, &renewed, start + LOOK_EVERY * 3)
            .unwrap();
        assert_eq!((change.added.len(), change.removed.len()), (0, 3));
        // No session equipped yet: nothing to compare.
        assert_eq!(
            watch.changed(&settings, &Given::default(), start + LOOK_EVERY * 4),
            None
        );
    }

    /// The catalog is read only when its time changed: a file changed
    /// without its time (as no writer does) is not read again.
    #[test]
    fn the_catalog_is_read_again_only_when_its_time_changed() {
        let folder = Folder::new("time");
        let settings = settings(&folder);
        write(&folder, &SOUND, 0);
        let given = Given::default();
        given.record(&declared(&settings));
        let mut watch = Watch::new(&settings, LOOK_EVERY).unwrap();
        let start = Instant::now();
        assert_eq!(watch.changed(&settings, &given, start), None);
        write(&folder, &LIGHT, 0);
        assert_eq!(watch.changed(&settings, &given, start + LOOK_EVERY), None);
        write(&folder, &LIGHT, 5);
        assert!(
            watch
                .changed(&settings, &given, start + LOOK_EVERY * 2)
                .is_some()
        );
        assert!(Watch::new(&Settings::default(), LOOK_EVERY).is_none());
    }

    #[test]
    fn a_renewal_is_due_only_between_turns_once_a_session_and_within_the_bound() {
        let quiet = Moment {
            turn_open: false,
            output_owed: false,
            going_on: false,
            look_owed: false,
            live_already_asked: false,
            may_renew: true,
        };
        assert!(quiet.due());
        for moment in [
            Moment {
                turn_open: true,
                ..quiet
            },
            Moment {
                output_owed: true,
                ..quiet
            },
            Moment {
                going_on: true,
                ..quiet
            },
            Moment {
                look_owed: true,
                ..quiet
            },
            Moment {
                live_already_asked: true,
                ..quiet
            },
            Moment {
                may_renew: false,
                ..quiet
            },
        ] {
            assert!(!moment.due());
        }
        let folder = Folder::new("bound");
        let mut watch = Watch::new(&settings(&folder), LOOK_EVERY).unwrap();
        let start = Instant::now();
        assert!(watch.may_renew(start));
        watch.renewed(start);
        assert!(!watch.may_renew(start + RENEW_AT_MOST_EVERY - Duration::from_secs(1)));
        assert!(watch.may_renew(start + RENEW_AT_MOST_EVERY));
    }

    #[test]
    fn a_record_is_the_workers_own_and_fresh_is_taken_once() {
        let (one, other) = (Given::default(), Given::default());
        assert_eq!(one.now(), None);
        one.record(&[lamp_gemini::Tool {
            name: "light_on".into(),
            description: "On.".into(),
            parameters: None,
            silent: false,
        }]);
        assert_eq!(
            one.now().unwrap().into_iter().collect::<Vec<_>>(),
            ["light_on"]
        );
        assert_eq!(other.now(), None);
        one.open_fresh();
        assert!(!other.take_fresh());
        assert!(one.take_fresh());
        assert!(!one.take_fresh());
    }

    #[test]
    fn the_setup_line_counts_the_executives_functions_and_says_when_the_light_is_missing() {
        let folder = Folder::new("line");
        let settings = settings(&folder);
        assert_eq!(
            setup_line(&declared(&settings), &settings),
            "lamp-voice: session functions: 1 declared; the executive's catalog is not there"
        );
        write(
            &folder,
            &["volume_up", "volume_down", "light_on", "weather_now"],
            0,
        );
        assert_eq!(
            setup_line(&declared(&settings), &settings),
            "lamp-voice: session functions: 5 declared; the executive's 4 (light 1, volume 2, other 1)"
        );
        write(&folder, &SOUND, 1);
        assert_eq!(
            setup_line(&declared(&settings), &settings),
            "lamp-voice: session functions: 4 declared; the executive's 3 (light 0, volume 3, other 0); no light functions: the ring is not up"
        );
        let plain = Settings::default();
        assert_eq!(
            setup_line(&declared(&plain), &plain),
            "lamp-voice: session functions: 0 declared"
        );
    }
}
