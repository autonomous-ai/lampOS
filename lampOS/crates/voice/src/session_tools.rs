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
use crate::tools::{CATALOG_FILE, Settings, declared, every_skills_folder};
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
/// Between looks, a change found at the last one is checked again this
/// often, so a gap between turns far shorter than a look is still taken
/// (Jarvis on #1588), for the cost of a few set comparisons a second while
/// a change stands.
pub const RECHECK_EVERY: Duration = Duration::from_millis(20);
/// What comes back for a session told it cannot use it renews a session
/// during a conversation at most this often, for the same kind, so a light
/// that comes and goes (#1514) does not renew it again and again (the
/// first builder on #1588).
pub const BACK_AT_MOST_EVERY: Duration = Duration::from_secs(60);

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

/// The functions' names, and for `use_skill` a name for each skill it
/// reaches, so a skill installed or removed behind it is a change too
/// (#1668).
pub fn names(tools: &[Tool]) -> BTreeSet<String> {
    tools
        .iter()
        .flat_map(|tool| std::iter::once(tool.name.clone()).chain(crate::use_skill::reached(tool)))
        .collect()
}

/// What would be declared now and a session was not given, and the other
/// way round.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Change {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    /// What it brings back: the kinds of added functions the session was
    /// given none of, which is what it was told it cannot do right now
    /// ([`not_ready`]) when its instruction had room for that line (Jarvis
    /// on #1588).
    pub came_back: Vec<Kind>,
}

impl Change {
    fn between(given: &BTreeSet<String>, now: &BTreeSet<String>) -> Option<Self> {
        let change = Self {
            added: now.difference(given).cloned().collect(),
            removed: given.difference(now).cloned().collect(),
            came_back: Vec::new(),
        };
        (!change.added.is_empty() || !change.removed.is_empty()).then_some(change)
    }
}

impl std::fmt::Display for Change {
    /// `+7 (light_brighter, light_colour, light_cooler, …) -0`: counts, and
    /// the first few names, as lamp-4ace said it on 18 Oct at 12:48.
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
    files: Vec<PathBuf>,
    times: Option<Vec<Option<SystemTime>>>,
    now: BTreeSet<String>,
    looked_at: Option<Instant>,
    renewed_at: Option<Instant>,
    /// When a renewal last brought back each kind.
    came_back_at: Vec<(Kind, Instant)>,
    /// Whether the last check found a change, and when that was.
    pending: bool,
    checked_at: Option<Instant>,
    /// Whether the last renewal brought something back.
    last_came_back: bool,
}

impl Watch {
    /// With an executive or the skills' own folder only: without either
    /// there is no catalog to change.
    pub fn new(settings: &Settings, look_every: Duration) -> Option<Self> {
        let files: Vec<PathBuf> = settings
            .exec
            .iter()
            .map(|directory| directory.join(CATALOG_FILE))
            .chain(
                every_skills_folder(settings)
                    .into_iter()
                    .map(|folder| folder.join(CATALOG_FILE)),
            )
            .collect();
        if files.is_empty() {
            return None;
        }
        Some(Self {
            look_every,
            files,
            times: None,
            now: BTreeSet::new(),
            looked_at: None,
            renewed_at: None,
            came_back_at: Vec::new(),
            last_came_back: false,
            pending: false,
            checked_at: None,
        })
    }

    /// How what would be declared now differs from what the newest session
    /// was `given`. The catalogs are looked at once every `look_every`, and
    /// read only when one's time changed since the last look. Between
    /// looks, a change found is checked again against `given` as it is then,
    /// once every [`RECHECK_EVERY`], so the renewal can take a gap between
    /// turns shorter than a look (Jarvis on #1588); otherwise `None`, with
    /// `given` not touched.
    pub fn changed(&mut self, settings: &Settings, given: &Given, now: Instant) -> Option<Change> {
        let since = |at: Option<Instant>| at.map(|at| now.saturating_duration_since(at));
        if since(self.looked_at).is_some_and(|gone| gone < self.look_every) {
            if !self.pending || since(self.checked_at).is_some_and(|gone| gone < RECHECK_EVERY) {
                return None;
            }
            self.checked_at = Some(now);
            let change = self.against(settings, given);
            self.pending = change.is_some();
            return change;
        }
        self.looked_at = Some(now);
        self.checked_at = Some(now);
        let times: Vec<Option<SystemTime>> = self
            .files
            .iter()
            .map(|file| {
                std::fs::metadata(file)
                    .and_then(|meta| meta.modified())
                    .ok()
            })
            .collect();
        if self.times.as_ref() != Some(&times) {
            self.times = Some(times);
            self.now = names(&declared(settings));
        }
        let change = self.against(settings, given);
        self.pending = change.is_some();
        change
    }

    /// The change from what the newest session was `given` to the catalogs
    /// as last read.
    fn against(&self, settings: &Settings, given: &Given) -> Option<Change> {
        let given = given.now()?;
        let mut change = Change::between(&given, &self.now)?;
        change.came_back = missing(&given, settings)
            .into_iter()
            .filter(|kind| change.added.iter().any(|name| kind.offered_by(name)))
            .collect();
        Some(change)
    }

    /// Whether `change` brings back something that may renew a session
    /// while a conversation goes on: a kind not renewed for in the last
    /// [`BACK_AT_MOST_EVERY`].
    pub fn ready_again(&self, change: &Change, now: Instant) -> bool {
        change.came_back.iter().any(|kind| {
            self.came_back_at.iter().all(|(renewed, at)| {
                renewed != kind || now.saturating_duration_since(*at) >= BACK_AT_MOST_EVERY
            })
        })
    }

    /// Whether a renewal for `change` may be asked now: once in
    /// [`RENEW_AT_MOST_EVERY`], but sooner for what comes back
    /// ([`Self::ready_again`]) after a renewal that brought nothing back.
    /// lamp-actions restarting in a quiet room renews for the light gone,
    /// and its return seconds later must not wait out the bound with the
    /// lamp refusing meanwhile (Jarvis on #1588). Two returns in a row
    /// still wait, so a catalog that comes and goes renews at most twice
    /// in the bound.
    pub fn may_renew(&self, change: &Change, now: Instant) -> bool {
        self.renewed_at
            .is_none_or(|at| now.saturating_duration_since(at) >= RENEW_AT_MOST_EVERY)
            || (!self.last_came_back && self.ready_again(change, now))
    }

    /// A renewal asked now for `change`.
    pub fn renewed(&mut self, change: &Change, now: Instant) {
        self.renewed_at = Some(now);
        self.last_came_back = !change.came_back.is_empty();
        self.came_back_at
            .retain(|(kind, _)| !change.came_back.contains(kind));
        self.came_back_at
            .extend(change.came_back.iter().map(|kind| (*kind, now)));
    }
}

/// The journal's one line for a renewal asked for `change` (#1472): what
/// changed, and what came back that the session was told it could not use,
/// with `going_on` that a conversation is going on (the first builder on
/// #1588).
pub fn renewal_line(change: &Change, going_on: bool) -> String {
    let mut line = format!("the executive's functions changed ({change})");
    if !change.came_back.is_empty() {
        let named: Vec<&str> = change.came_back.iter().map(|kind| kind.named()).collect();
        let (and, is) = match named.as_slice() {
            [one] => ((*one).to_owned(), "is"),
            [rest @ .., last] => (format!("{} and {last}", rest.join(", ")), "are"),
            [] => unreachable!("came back is not empty"),
        };
        line.push_str(&format!(
            ": {and} {is} back, which the session was told it could not use"
        ));
    }
    line.push_str("; a new session is opened between turns");
    if going_on && !change.came_back.is_empty() {
        line.push_str(", though a conversation is going on");
    }
    line
}

/// Whether to renew the session for a changed catalog now: only between
/// turns (no turn owned, so nobody's speech taken and the lamp not
/// speaking; none open; no output owed; no conversation going on; no look
/// owed), once for a live session, and within the bound. The session then
/// ends only at a moment nothing is owed on its connection, so never with
/// a function call open, one the Policy holds included
/// (`lamp_gemini::InputSender::renew_when_idle`).
///
/// A conversation going on does not hold back a change that brings back
/// what the session was told it cannot do right now (`ready_again`): the
/// lamp's own "I can't right now" keeps the conversation going, so while
/// the person kept asking, the light would work and the lamp would keep
/// refusing (Jarvis on #1513). That renews at the first gap between turns
/// instead, with all the rest as it is, and for one kind at most once in
/// [`BACK_AT_MOST_EVERY`] (the first builder on #1588).
pub struct Moment {
    pub turn_owned: bool,
    pub turn_open: bool,
    pub output_owed: bool,
    pub going_on: bool,
    pub ready_again: bool,
    pub look_owed: bool,
    pub live_already_asked: bool,
    pub may_renew: bool,
}

impl Moment {
    pub fn due(&self) -> bool {
        !self.turn_owned
            && !self.turn_open
            && !self.output_owed
            && (!self.going_on || self.ready_again)
            && !self.look_owed
            && !self.live_already_asked
            && self.may_renew
    }
}

/// What a session given none of the light's functions is told, so that it
/// never says it changed the light: a session opened while the ring was
/// down said "your light is all set to orange now" five times and made no
/// call (the first builder's lamp run on #1495, lamp-4ace, 18 Oct 13:57).
pub const NO_LIGHT: &str = "\n\nYour light\nYou cannot change your light right now: it is not ready. If someone asks you to change it, say so in a few words of your own, not the same way twice. Never say you changed it.";

/// What a session given `tools` is told it can't do right now, while the
/// voice takes an executive's: the things the character says it can do for
/// people that it has no function for, the light, the voice's volume and,
/// with the reminder tools switched on, timers and reminders. The light
/// alone is [`NO_LIGHT`]. With `lamp-actions` down, a session given none of
/// them said "I've turned my voice down just a little bit" for "turn your
/// light down a little", having no function at all (the first builder's
/// lamp run on #1513, 18 Oct 16:27–16:32). None when it has them all.
pub fn not_ready(tools: &[Tool], settings: &Settings) -> Option<String> {
    let missing: Vec<&str> = missing(&names(tools), settings)
        .into_iter()
        .map(Kind::cannot)
        .collect();
    let (are, them) = if missing.len() == 1 {
        ("it is", "it")
    } else {
        ("they are", "any of them")
    };
    match missing.as_slice() {
        [] => None,
        ["change your light"] => Some(NO_LIGHT.to_owned()),
        _ => Some(format!(
            "\n\nWhat you can't do right now\nYou cannot {} right now: {are} not ready. If someone asks for {them}, say so in a few words of your own, not the same way twice. Never say you did it.",
            listed(&missing)
        )),
    }
}

/// What a session is told after its instruction: what it can't do right
/// now ([`not_ready`]), with `use_skill` its skills' lines
/// ([`crate::use_skill::section`], #1668), and how to put it to sleep or
/// be unplugged when it has no function for it ([`crate::without_a_tool`],
/// #4595). None when there is none of them.
pub fn told_after(tools: &[Tool], settings: &Settings) -> Option<String> {
    let mut told = not_ready(tools, settings).unwrap_or_default();
    if tools.iter().any(|tool| tool.name == crate::use_skill::NAME) {
        told.push_str(&crate::use_skill::section(crate::use_skill::offered(
            &crate::tools::skill_tools(settings),
        )));
    }
    told.push_str(&crate::without_a_tool::for_declared(
        tools,
        settings.shutdown_from_app,
    ));
    (!told.is_empty()).then_some(told)
}

/// What the character says the lamp can do for people, each by functions
/// of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Light,
    Volume,
    Reminders,
}

impl Kind {
    /// What a session without it is told it cannot do.
    fn cannot(self) -> &'static str {
        match self {
            Self::Light => "change your light",
            Self::Volume => "change how loud your voice is",
            Self::Reminders => "set, list or change timers and reminders",
        }
    }

    /// What the journal calls it.
    pub fn named(self) -> &'static str {
        match self {
            Self::Light => "the light",
            Self::Volume => "the voice's volume",
            Self::Reminders => "timers and reminders",
        }
    }

    fn offered_by(self, name: &str) -> bool {
        match self {
            Self::Light => name.starts_with("light_"),
            Self::Volume => name.starts_with("volume_"),
            Self::Reminders => crate::tools::is_schedule_tool(name),
        }
    }
}

/// The kinds a session `given` these functions has none of, while the
/// voice takes an executive's (none without one); the reminders only with
/// their switch.
fn missing(given: &BTreeSet<String>, settings: &Settings) -> Vec<Kind> {
    if settings.exec.is_none() {
        return Vec::new();
    }
    [Kind::Light, Kind::Volume, Kind::Reminders]
        .into_iter()
        .filter(|kind| *kind != Kind::Reminders || settings.schedule_tools)
        .filter(|kind| !given.iter().any(|name| kind.offered_by(name)))
        .collect()
}

/// "a or b", "a, b or c".
fn listed(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [only] => (*only).to_owned(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
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
        line.push_str(&skills_note(tools, settings));
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
    line.push_str(&skills_note(tools, settings));
    line
}

/// The setup line's word on the skills (#1668, the reviewer on #1696): how
/// many `use_skill` reaches, the skills a session is not given (past the 32
/// without it, or past its own bound with it), and those its lines name
/// alone or leave out.
fn skills_note(tools: &[Tool], settings: &Settings) -> String {
    let mut note = String::new();
    let given = names(tools);
    let skills = crate::tools::skill_tools(settings);
    let reach = tools
        .iter()
        .find(|tool| tool.name == crate::use_skill::NAME);
    if let Some(reach) = reach {
        note.push_str(&format!(
            "; {} skills through use_skill",
            crate::use_skill::reached(reach).len()
        ));
    }
    let not_given: Vec<&str> = skills
        .iter()
        .map(|skill| skill.name.as_str())
        .filter(|name| {
            !given.contains(*name) && !given.contains(&format!("{}:{name}", crate::use_skill::NAME))
        })
        .collect();
    if !not_given.is_empty() {
        note.push_str(&format!(
            "; skills not given: {}{}",
            not_given.join(", "),
            if reach.is_some() {
                " (past use_skill's bound)"
            } else {
                " (past 32: LAMP_USE_SKILL=on reaches them)"
            }
        ));
    }
    if reach.is_some() {
        let (named, left) = crate::use_skill::cut(crate::use_skill::offered(&skills));
        if !named.is_empty() || !left.is_empty() {
            note.push_str(&format!(
                "; use_skill's lines: {} named alone, {} left out",
                named.len(),
                left.len()
            ));
        }
    }
    note
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

    /// Jarvis on #1588: a change found at a look stands between looks,
    /// checked again every 20 ms, so the renewal can take a gap between
    /// turns shorter than a look; the catalog is still read only at looks,
    /// and once the session has what it holds, nothing is given until the
    /// next look.
    #[test]
    fn a_change_found_at_a_look_is_checked_again_between_looks() {
        let folder = Folder::new("stands");
        let settings = settings(&folder);
        write(&folder, &SOUND, 0);
        let given = Given::default();
        given.record(&declared(&settings));
        let mut watch = Watch::new(&settings, LOOK_EVERY).unwrap();
        let start = Instant::now();
        let at = |ms: u64| start + LOOK_EVERY + Duration::from_millis(ms);
        assert_eq!(watch.changed(&settings, &given, start), None);
        let both: Vec<&str> = SOUND.iter().chain(LIGHT.iter()).copied().collect();
        write(&folder, &both, 1);
        let found = watch.changed(&settings, &given, at(0)).unwrap();
        // Not again within 20 ms; then every 20 ms or more, the same.
        assert_eq!(watch.changed(&settings, &given, at(2)), None);
        for ms in [20, 300, 1_000] {
            assert_eq!(
                watch.changed(&settings, &given, at(ms)).as_ref(),
                Some(&found),
                "{ms} ms"
            );
        }
        // The catalog is read only at looks: the light gone again is not
        // seen before the next.
        write(&folder, &SOUND, 2);
        assert_eq!(
            watch.changed(&settings, &given, at(1_500)).as_ref(),
            Some(&found)
        );
        // A session given what the catalog held at the look: nothing stands,
        // and nothing is checked again before the next look.
        let renewed = Given::default();
        let holds: Vec<lamp_gemini::Tool> = given
            .now()
            .unwrap()
            .iter()
            .chain(&found.added)
            .map(|name| lamp_gemini::Tool {
                name: name.clone(),
                description: "Do it.".into(),
                parameters: None,
                silent: false,
            })
            .collect();
        renewed.record(&holds);
        assert_eq!(watch.changed(&settings, &renewed, at(1_600)), None);
        assert_eq!(watch.changed(&settings, &given, at(1_700)), None);
        // The next look reads the light gone: no change from `given`.
        assert_eq!(watch.changed(&settings, &given, at(2_000)), None);
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
            turn_owned: false,
            turn_open: false,
            output_owed: false,
            going_on: false,
            ready_again: false,
            look_owed: false,
            live_already_asked: false,
            may_renew: true,
        };
        assert!(quiet.due());
        for moment in [
            Moment {
                turn_owned: true,
                ..quiet
            },
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
        let other = Change::default();
        assert!(watch.may_renew(&other, start));
        watch.renewed(&other, start);
        assert!(!watch.may_renew(&other, start + RENEW_AT_MOST_EVERY - Duration::from_secs(1)));
        assert!(watch.may_renew(&other, start + RENEW_AT_MOST_EVERY));
    }

    /// Jarvis on #1588: lamp-actions restarting in a quiet room renews for
    /// the light gone; the light back seconds later may renew at once, not
    /// after the 30 s bound. Two returns in a row still wait for it.
    #[test]
    fn the_light_back_just_after_a_renewal_for_it_gone_may_renew_at_once() {
        let folder = Folder::new("back-soon");
        let mut watch = Watch::new(&settings(&folder), LOOK_EVERY).unwrap();
        let start = Instant::now();
        let gone = Change {
            removed: vec!["light_on".into()],
            ..Change::default()
        };
        let back = Change {
            added: vec!["light_on".into()],
            came_back: vec![Kind::Light],
            ..Change::default()
        };
        let volume_back = Change {
            added: vec!["volume_up".into()],
            came_back: vec![Kind::Volume],
            ..Change::default()
        };
        watch.renewed(&gone, start);
        let soon = start + Duration::from_secs(4);
        assert!(!watch.may_renew(&gone, soon));
        assert!(watch.may_renew(&back, soon), "the light back at once");
        watch.renewed(&back, soon);
        // Another return straight after one waits out the bound.
        let later = soon + Duration::from_secs(4);
        assert!(!watch.may_renew(&volume_back, later));
        assert!(watch.may_renew(&volume_back, soon + RENEW_AT_MOST_EVERY));
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

    /// #1513's follow-up: what a session is told it can't do follows what
    /// it lacks of the light's, the volume's and, with their switch, the
    /// reminders' functions; the light alone is #1513's own words; nothing
    /// for a session with them all, or a voice without an executive.
    #[test]
    fn a_session_is_told_each_thing_it_cannot_do_right_now() {
        let folder = Folder::new("not-ready");
        let declared = |names: &[&str]| -> Vec<Tool> {
            names
                .iter()
                .map(|name| serde_json::from_value(tool(name)).unwrap())
                .collect()
        };
        let exec = Settings {
            exec: Some(folder.0.clone()),
            ..Default::default()
        };
        let reminders = Settings {
            schedule_tools: true,
            ..exec.clone()
        };
        assert_eq!(
            not_ready(&declared(&["light_on", "volume_up"]), &exec),
            None
        );
        assert_eq!(
            not_ready(&declared(&["volume_up"]), &exec).as_deref(),
            Some(NO_LIGHT)
        );
        let volume = not_ready(&declared(&["light_on"]), &exec).unwrap();
        assert!(volume.contains("You cannot change how loud your voice is right now: it is not ready. If someone asks for it,"), "{volume}");
        let none = not_ready(&[], &reminders).unwrap();
        assert!(none.contains("You cannot change your light, change how loud your voice is or set, list or change timers and reminders right now: they are not ready."), "{none}");
        assert_eq!(
            not_ready(
                &declared(&["light_on", "volume_up", "set_timer"]),
                &reminders
            ),
            None
        );
        // Reminders switched off are not something it was told it can do.
        assert_eq!(
            not_ready(&declared(&["light_on", "volume_up"]), &exec),
            None
        );
        assert_eq!(not_ready(&[], &Settings::default()), None);
    }

    /// Jarvis's P2 on #1513: a catalog change brings something back only
    /// when it adds a function of a kind the session was given none of, and
    /// so was told it cannot use right now; more of a kind it had, another
    /// function, or one taken away do not. And for one kind, at most once a
    /// minute (the first builder on #1588).
    #[test]
    fn a_change_is_ready_again_only_when_it_brings_back_what_the_session_was_told_it_cannot_do() {
        let folder = Folder::new("ready-again");
        let settings = Settings {
            schedule_tools: true,
            ..settings(&folder)
        };
        let start = Instant::now();
        let mut at = 0;
        let mut change = |given: &[&str], now: &[&str]| -> Change {
            at += 10;
            write(&folder, given, at);
            let session = Given::default();
            session.record(&declared(&settings));
            let mut watch = Watch::new(&settings, LOOK_EVERY).unwrap();
            write(&folder, now, at + 1);
            let change = watch.changed(&settings, &session, start).unwrap();
            assert_eq!(
                watch.ready_again(&change, start),
                !change.came_back.is_empty()
            );
            change
        };
        let both: Vec<&'static str> = SOUND.iter().chain(LIGHT.iter()).copied().collect();
        let with = |more: &[&'static str]| -> Vec<&'static str> {
            both.iter().chain(more).copied().collect()
        };
        // The light, the volume or the reminders back.
        assert_eq!(change(&SOUND, &both).came_back, [Kind::Light]);
        assert_eq!(change(&LIGHT, &both).came_back, [Kind::Volume]);
        assert_eq!(
            change(&both, &with(&["set_timer"])).came_back,
            [Kind::Reminders]
        );
        // More of a kind it had, something else, or one gone.
        assert!(
            change(&["light_on", "volume_up"], &both)
                .came_back
                .is_empty()
        );
        assert!(
            change(
                &with(&["set_timer"]),
                &with(&["set_timer", "list_reminders"])
            )
            .came_back
            .is_empty()
        );
        assert!(change(&both, &with(&["weather_now"])).came_back.is_empty());
        assert!(change(&both, &SOUND).came_back.is_empty());
        // The light back, renewed for: not again for the light within the
        // minute, but for another kind, and for the light after it.
        let light = change(&SOUND, &both);
        let volume = change(&LIGHT, &both);
        let mut watch = Watch::new(&settings, LOOK_EVERY).unwrap();
        watch.renewed(&light, start);
        let soon = start + BACK_AT_MOST_EVERY - Duration::from_secs(1);
        assert!(!watch.ready_again(&light, soon));
        assert!(watch.ready_again(&volume, soon));
        assert!(watch.ready_again(&light, start + BACK_AT_MOST_EVERY));
        assert_eq!(Kind::Light.named(), "the light");
    }

    /// The first builder on #1588: one journal line per renewal, saying what
    /// came back and, then, that a conversation is going on.
    #[test]
    fn a_renewal_says_in_one_line_what_came_back() {
        let change = |came_back: Vec<Kind>| Change {
            added: vec!["light_on".into()],
            removed: Vec::new(),
            came_back,
        };
        let changed = "the executive's functions changed (+1 (light_on) -0)";
        let opened = "; a new session is opened between turns";
        assert_eq!(
            renewal_line(&change(Vec::new()), false),
            format!("{changed}{opened}")
        );
        assert_eq!(
            renewal_line(&change(Vec::new()), true),
            format!("{changed}{opened}"),
            "nothing came back: it did not renew in a conversation"
        );
        assert_eq!(
            renewal_line(&change(vec![Kind::Light]), true),
            format!(
                "{changed}: the light is back, which the session was told it could not use{opened}, though a conversation is going on"
            )
        );
        assert_eq!(
            renewal_line(&change(vec![Kind::Light, Kind::Volume]), false),
            format!(
                "{changed}: the light and the voice's volume are back, which the session was told it could not use{opened}"
            )
        );
    }

    /// And such a change renews while a conversation goes on, the lamp's
    /// own "I can't right now" among it, but still only between turns,
    /// once and within the bound; any other change waits for the
    /// conversation to end.
    #[test]
    fn what_a_session_was_told_it_cannot_do_coming_back_renews_during_a_conversation() {
        let talking = Moment {
            turn_owned: false,
            turn_open: false,
            output_owed: false,
            going_on: true,
            ready_again: true,
            look_owed: false,
            live_already_asked: false,
            may_renew: true,
        };
        assert!(talking.due());
        assert!(
            !Moment {
                ready_again: false,
                ..talking
            }
            .due()
        );
        for moment in [
            Moment {
                turn_owned: true,
                ..talking
            },
            Moment {
                turn_open: true,
                ..talking
            },
            Moment {
                output_owed: true,
                ..talking
            },
            Moment {
                look_owed: true,
                ..talking
            },
            Moment {
                live_already_asked: true,
                ..talking
            },
            Moment {
                may_renew: false,
                ..talking
            },
        ] {
            assert!(!moment.due());
        }
    }

    /// #1668: with `use_skill` the session is given one function for every
    /// skill, so a skill installed behind it changes no function's name; it
    /// is a change all the same, named `use_skill:<skill>`, and the session
    /// is renewed for it as for any change.
    #[test]
    fn a_skill_installed_behind_use_skill_is_a_change() {
        let folder = Folder::new("use-skill");
        let mut settings = settings(&folder);
        settings.use_skill = true;
        write(&folder, &SOUND, 0);
        std::fs::create_dir(folder.0.join(crate::tools::SKILLS_DIR)).unwrap();
        let skills = |names: &[&str], seconds: u64| {
            let path = folder.0.join(crate::tools::SKILLS_DIR).join(CATALOG_FILE);
            let tools: Vec<_> = names.iter().map(|name| tool(name)).collect();
            std::fs::write(&path, json!({"tools": tools}).to_string()).unwrap();
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + seconds))
                .unwrap();
        };
        skills(&["dance"], 0);
        let given = Given::default();
        given.record(&declared(&settings));
        assert!(given.now().unwrap().contains("use_skill:dance"));
        let mut watch = Watch::new(&settings, LOOK_EVERY).unwrap();
        let start = Instant::now();
        assert_eq!(watch.changed(&settings, &given, start), None);
        skills(&["dance", "check_my_work"], 1);
        let change = watch
            .changed(&settings, &given, start + LOOK_EVERY)
            .unwrap();
        assert_eq!(change.added, ["use_skill:check_my_work"]);
        assert!(change.removed.is_empty());
    }

    /// The skills' own folder (`--skills-dir`) is looked at too, with no
    /// executive at all: a skill's function written there is a change at
    /// the next look.
    #[test]
    fn the_skills_own_folder_is_looked_at_with_the_executives() {
        let folder = Folder::new("skills-own");
        let settings = Settings {
            decline: true,
            skills: Some(folder.0.clone()),
            ..Default::default()
        };
        assert!(Watch::new(&Settings::default(), LOOK_EVERY).is_none());
        let given = Given::default();
        given.record(&declared(&settings));
        let mut watch = Watch::new(&settings, LOOK_EVERY).unwrap();
        let start = Instant::now();
        assert_eq!(watch.changed(&settings, &given, start), None);
        write(&folder, &["dance"], 1);
        let change = watch
            .changed(&settings, &given, start + LOOK_EVERY)
            .unwrap();
        assert_eq!(change.added, ["dance"]);
        assert!(change.removed.is_empty());
    }

    /// El's peer read of #1588 (the first builder's 21:09 step: three
    /// stop/starts of lamp-actions in 20 s). systemd removes
    /// `/run/lampos-actions` when the unit stops (`RuntimeDirectory=`; a
    /// restart keeps it, `RuntimeDirectoryPreserve=restart`), so each stop
    /// and start is no catalog at all, read as empty, and then the whole one 2
    /// s later, the light, the volume and the reminders together, at 0, 7
    /// and 14 s. A
    /// session takes the catalog as it is when renewed. A renewal is asked
    /// at a look when `Moment::due` would let it, the rest of the moment
    /// quiet: in a conversation, only for what comes back, once a minute
    /// for a kind; in a quiet room, for any change, once in the bound but
    /// sooner for a return after a renewal that brought nothing back.
    #[test]
    fn three_restarts_in_twenty_seconds_renew_at_most_twice_and_once_in_a_conversation() {
        let folder = Folder::new("three-restarts");
        let start = Instant::now();
        let with_light = |up: bool| -> BTreeSet<String> {
            if up {
                ["light_on", "volume_up", "set_timer"]
                    .map(str::to_owned)
                    .into()
            } else {
                BTreeSet::new()
            }
        };
        // The catalog at each second: none for 2 s from each restart.
        let catalog = |second: u64| with_light(!matches!(second, 0..=1 | 7..=8 | 14..=15));
        let run = |going_on: bool, given_light: bool| -> Vec<u64> {
            let mut watch = Watch::new(&settings(&folder), LOOK_EVERY).unwrap();
            let mut given = with_light(given_light);
            let mut renewals = Vec::new();
            for second in (0..20).step_by(2) {
                let now = start + Duration::from_secs(second);
                let seen = catalog(second);
                let Some(mut change) = Change::between(&given, &seen) else {
                    continue;
                };
                for (name, kind) in [
                    ("light_on", Kind::Light),
                    ("volume_up", Kind::Volume),
                    ("set_timer", Kind::Reminders),
                ] {
                    if !given.contains(name) && seen.contains(name) {
                        change.came_back.push(kind);
                    }
                }
                let moment = Moment {
                    turn_owned: false,
                    turn_open: false,
                    output_owed: false,
                    going_on,
                    ready_again: watch.ready_again(&change, now),
                    look_owed: false,
                    live_already_asked: false,
                    may_renew: watch.may_renew(&change, now),
                };
                if moment.due() {
                    watch.renewed(&change, now);
                    given = seen;
                    renewals.push(second);
                }
            }
            renewals
        };
        // A quiet room: the light gone renews, and its return 2 s later
        // renews at once; the two later restarts wait out the bound, by
        // which time the light is back and nothing has changed.
        assert_eq!(run(false, true), [0, 2]);
        // A conversation going on, the session given the light: the light
        // going is held back, and its return changes nothing.
        assert_eq!(run(true, true), Vec::<u64>::new());
        // A conversation going on, the session opened in the gap without
        // the light: its return renews once, and not again in the minute.
        assert_eq!(run(true, false), [2]);
    }
}
