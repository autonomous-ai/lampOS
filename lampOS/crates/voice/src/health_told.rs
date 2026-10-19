//! Which part of the lamp is unwell, told to the voice (#1681).
//!
//! The lamp's health (`lamp-health`, `lamp/health`) says when a part has
//! failed or has stopped answering. The "Not feeling well" skill shows it
//! quietly: one droop and a dimmer light. A creature that feels unwell
//! does not announce it, but if asked how it is it can say what is wrong.
//! So the model is told, beside something a person is saying to the lamp,
//! which parts are not working right, by the names a person knows them by
//! (`lamp_health::show::known_as`, and the self-test's own names), and to
//! say so only if asked. Told once a session for each set of parts; when
//! the set empties after it was told, it is told that all is well again.
//!
//! A part counts when it has been `error` for half a minute (the skill's
//! own wait), or when it has been `stale` for five minutes and the lamp's
//! `quiet_ms` says a part that came up well has been quiet that long. A
//! health older than [`FRESH_FOR`] says nothing of now: nothing is told
//! then, and the half minute starts again.
//!
//! What the model is told comes from what was last read: the provider
//! worker reads the bus's kept value every few seconds, outside any turn,
//! and a turn never waits for it.
//!
//! Off unless the service's environment says `on`.
use lamp_msgs::{Health, HealthItem, Level};
use std::{collections::BTreeMap, time::Duration};

/// Where the coordinator reads whether the lamp's health is told. `on`
/// only.
pub const ENVIRONMENT: &str = "LAMP_HEALTH_FACTS";
/// The provider worker's flag for it.
pub const FLAG: &str = "--health-facts";

/// A part `error` for this long counts (the skill's `for = "30s"`).
const ERROR_FOR: Duration = Duration::from_secs(30);
/// A part quiet for this long counts (the skill's `quiet_ms` trigger).
const QUIET_FOR: Duration = Duration::from_secs(300);
/// A health older than this says nothing of now (`Health::declaration`'s
/// `lasting`).
pub const FRESH_FOR: Duration = Duration::from_secs(15);
/// At most this many parts are named, so the line stays short.
const MOST_NAMED: usize = 4;
/// The self-test's item, whose message names the parts it found failed.
const SELFTEST: &str = "selftest";

/// What the provider worker knows of the lamp's health, from
/// `lamp/health`, and what this session was told of it.
#[derive(Debug)]
pub struct HealthTold<S> {
    /// Each part seen `error`, since when (the worker's clock, µs).
    erring_since_us: BTreeMap<String, u64>,
    /// The parts that count now, by name; `None` when no fresh health is
    /// known.
    unwell: Option<Vec<String>>,
    /// The session told, and what it was told.
    told: Option<(S, Vec<String>)>,
}

impl<S> Default for HealthTold<S> {
    fn default() -> Self {
        Self {
            erring_since_us: BTreeMap::new(),
            unwell: None,
            told: None,
        }
    }
}

impl<S: PartialEq> HealthTold<S> {
    /// The newest health, or `None` when none is fresh.
    pub fn seen(&mut self, health: Option<&Health>, now_us: u64) {
        let Some(health) = health else {
            self.erring_since_us.clear();
            self.unwell = None;
            return;
        };
        self.erring_since_us.retain(|name, _| {
            health
                .items
                .iter()
                .any(|item| &item.name == name && item.level == Level::Error)
        });
        let quiet = health.quiet_ms >= QUIET_FOR.as_millis() as u64;
        let mut unwell = Vec::new();
        for item in &health.items {
            let counts = match item.level {
                Level::Error => {
                    let since = *self
                        .erring_since_us
                        .entry(item.name.clone())
                        .or_insert(now_us);
                    now_us.saturating_sub(since) >= ERROR_FOR.as_micros() as u64
                }
                Level::Stale => quiet && item.age_ms >= QUIET_FOR.as_millis() as u64,
                _ => false,
            };
            if counts {
                for name in names_of(item) {
                    if !unwell.contains(&name) {
                        unwell.push(name);
                    }
                }
            }
        }
        self.unwell = Some(unwell);
    }

    /// The note for `session`, if one is due: the parts that count, when
    /// this session has not been told them; that all is well again, when
    /// it was told of some and none count now. Nothing when no fresh
    /// health is known.
    pub fn due(&self, session: &S) -> Option<String> {
        let unwell = self.unwell.as_ref()?;
        let told = self
            .told
            .as_ref()
            .filter(|(told, _)| told == session)
            .map(|(_, parts)| parts);
        match told {
            Some(parts) if parts == unwell => None,
            Some(_) if unwell.is_empty() => Some(WELL_AGAIN.to_owned()),
            None if unwell.is_empty() => None,
            _ => Some(note(unwell)),
        }
    }

    /// The line for a turn that starts with nothing (the second route):
    /// the parts that count now, every turn while there are any; nothing
    /// when all is well or no fresh health is known.
    pub fn now(&self) -> Option<String> {
        self.unwell
            .as_ref()
            .filter(|unwell| !unwell.is_empty())
            .map(|unwell| note(unwell))
    }

    /// The note was given to `session`.
    pub fn told(&mut self, session: S) {
        if let Some(unwell) = &self.unwell {
            self.told = Some((session, unwell.clone()));
        }
    }
}

/// What the model is told when its parts are well again.
pub const WELL_AGAIN: &str = "(For you to know, not to answer: all your parts are working again.)";

/// What the model is told of the parts that are not working right.
pub fn note(parts: &[String]) -> String {
    let mut named = parts[..parts.len().min(MOST_NAMED)].join(", ");
    if parts.len() > MOST_NAMED {
        named.push_str(&format!(" and {} more", parts.len() - MOST_NAMED));
    }
    format!(
        "(For you to know, not to answer: not working right in you now: {named}. Say so only if someone asks how you are or what's wrong; never bring it up yourself.)"
    )
}

/// The names a person knows a part by. The self-test's item names the
/// parts it found failed in its message (`N parts failing: the SD card
/// (REASON), …`), by `lamp_health::parts::part_known_as`; any other part
/// is named by `lamp_health::show::known_as`.
fn names_of(item: &HealthItem) -> Vec<String> {
    if item.name == SELFTEST {
        let failed = failed_in(&item.message);
        if !failed.is_empty() {
            return failed;
        }
    }
    vec![lamp_health::show::known_as(&item.name).to_owned()]
}

/// The parts named after `failing: ` in the self-test's message, each up
/// to its ` (REASON)`, without a leading "the ".
fn failed_in(message: &str) -> Vec<String> {
    let Some((_, mut rest)) = message.split_once(" failing: ") else {
        return Vec::new();
    };
    let mut names = Vec::new();
    while let Some((name, tail)) = rest.split_once(" (") {
        names.push(name.strip_prefix("the ").unwrap_or(name).to_owned());
        match tail.split_once("), ") {
            Some((_, next)) => rest = next,
            None => break,
        }
    }
    names
}

/// The lamp's health as the bus keeps it, if it is fresh: one read of the
/// kept value, which never waits for a publisher. An old value, none, or
/// one that can't be read is `None`.
pub fn read(bus: &lamp_bus::Bus) -> Option<Health> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64);
    read_at(bus, now_ms)
}

/// [`read`] at `now_ms` by the wall clock.
fn read_at(bus: &lamp_bus::Bus, now_ms: u64) -> Option<Health> {
    bus.latest(Health::TOPIC)
        .ok()
        .flatten()
        .filter(|message| {
            !message.expired
                && now_ms.saturating_sub(message.header.at_unix_ms) < FRESH_FOR.as_millis() as u64
        })
        .and_then(|message| message.parse::<Health>().ok())
}

/// How often [`watch`] reads the bus.
pub const READ_EVERY: Duration = Duration::from_secs(5);

/// For the second route, whose turns each start with nothing (#1681):
/// what is known of the lamp's health, kept up by a thread of its own that
/// reads the bus every `every`. A turn takes the line from it
/// ([`HealthTold::now`]) and never reads the bus itself. If the thread
/// can't be started, nothing is ever told.
pub fn watch(
    bus: lamp_bus::Bus,
    every: Duration,
) -> std::sync::Arc<std::sync::Mutex<HealthTold<()>>> {
    let told = std::sync::Arc::new(std::sync::Mutex::new(HealthTold::default()));
    let kept = std::sync::Arc::downgrade(&told);
    let started = std::time::Instant::now();
    let _ = std::thread::Builder::new()
        .name("health-told".into())
        .spawn(move || {
            loop {
                let health = read(&bus);
                let now_us = started.elapsed().as_micros() as u64;
                // Gone with the worker that kept it.
                let Some(kept) = kept.upgrade() else {
                    return;
                };
                if let Ok(mut told) = kept.lock() {
                    told.seen(health.as_ref(), now_us);
                }
                drop(kept);
                std::thread::sleep(every);
            }
        });
    told
}

/// The provider worker's argument for the switch in the environment.
pub fn arguments_from_environment() -> Vec<String> {
    arguments(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("on") {
        vec![FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// Take a leading `--health-facts` off a worker's flags.
pub fn take(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == FLAG => (true, rest),
        _ => (false, flags),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEC: u64 = 1_000_000;

    fn item(name: &str, level: Level, message: &str, age_ms: u64) -> HealthItem {
        HealthItem {
            name: name.to_owned(),
            level,
            message: message.to_owned(),
            age_ms,
        }
    }

    fn health(items: Vec<HealthItem>, quiet_ms: u64) -> Health {
        let level = items
            .iter()
            .map(|item| item.level)
            .max_by_key(|level| level.rank())
            .unwrap_or(Level::Ok);
        Health {
            level,
            message: String::new(),
            items,
            missing: Vec::new(),
            quiet_ms,
        }
    }

    fn well() -> Health {
        health(
            vec![
                item("body", Level::Ok, "", 400),
                item("eyes", Level::Ok, "", 300),
                item("selftest", Level::Ok, "passed", 60_000),
            ],
            0,
        )
    }

    #[test]
    fn a_well_lamp_tells_nothing() {
        let mut told = HealthTold::<u8>::default();
        for at in 0..100 {
            told.seen(Some(&well()), at * 5 * SEC);
            assert_eq!(told.due(&1), None);
        }
        // A warning, or a part briefly stale, is not unwell.
        let warned = health(
            vec![
                item("body", Level::Warn, "elbow is warm", 0),
                item("eyes", Level::Stale, "not heard for 8 s", 8_000),
            ],
            8_000,
        );
        told.seen(Some(&warned), 600 * SEC);
        told.seen(Some(&warned), 900 * SEC);
        assert_eq!(told.due(&1), None);
    }

    #[test]
    fn an_error_for_half_a_minute_names_the_part_as_a_person_knows_it() {
        let mut told = HealthTold::default();
        let broken = health(
            vec![
                item("body", Level::Error, "elbow is too warm", 0),
                item("eyes", Level::Ok, "", 0),
            ],
            0,
        );
        told.seen(Some(&broken), 0);
        told.seen(Some(&broken), 25 * SEC);
        assert_eq!(told.due(&1), None, "not before half a minute");
        told.seen(Some(&broken), 30 * SEC);
        let note = told.due(&1).unwrap();
        assert_eq!(
            note,
            "(For you to know, not to answer: not working right in you now: arm and head. Say so only if someone asks how you are or what's wrong; never bring it up yourself.)"
        );
        // Told once a session, again in a new one.
        told.told(1);
        told.seen(Some(&broken), 35 * SEC);
        assert_eq!(told.due(&1), None);
        assert_eq!(told.due(&2).as_deref(), Some(note.as_str()));
    }

    #[test]
    fn the_self_tests_parts_are_named_by_their_own_names() {
        let mut told = HealthTold::<u8>::default();
        let failed = health(
            vec![
                item(
                    "selftest",
                    Level::Error,
                    "the lamp's USB hub dropped 2 times; 2 parts failing: the SD card (1 I/O error on the card since boot), the light ring (red did not light); a fault seen before this update, which this self-test could not check: the speaker (silent)",
                    90_000,
                ),
                item("eyes", Level::Error, "no frames", 0),
            ],
            0,
        );
        told.seen(Some(&failed), 0);
        told.seen(Some(&failed), 30 * SEC);
        let note = told.due(&1).unwrap();
        assert!(
            note.contains("in you now: SD card, light ring, camera."),
            "{note}"
        );
        assert!(!note.contains("speaker") && !note.contains("I/O"), "{note}");
    }

    #[test]
    fn a_part_quiet_for_five_minutes_counts_and_a_short_restart_does_not() {
        let mut told = HealthTold::<u8>::default();
        let restarting = health(vec![item("ears", Level::Stale, "", 6_000)], 6_000);
        told.seen(Some(&restarting), 0);
        told.seen(Some(&restarting), 60 * SEC);
        assert_eq!(told.due(&1), None);
        // Stale since boot (never heard well: `quiet_ms` 0) does not count.
        let never = health(vec![item("ears", Level::Stale, "", 400_000)], 0);
        told.seen(Some(&never), 400 * SEC);
        assert_eq!(told.due(&1), None);
        let stopped = health(vec![item("ears", Level::Stale, "", 300_000)], 300_000);
        told.seen(Some(&stopped), 700 * SEC);
        assert!(
            told.due(&1)
                .unwrap()
                .contains("in you now: room microphone."),
            "{:?}",
            told.due(&1)
        );
    }

    #[test]
    fn well_again_is_told_once_to_the_session_that_was_told() {
        let mut told = HealthTold::default();
        let broken = health(vec![item("eyes", Level::Error, "no frames", 0)], 0);
        told.seen(Some(&broken), 0);
        told.seen(Some(&broken), 30 * SEC);
        told.told(1);
        told.seen(Some(&well()), 40 * SEC);
        assert_eq!(told.due(&1).as_deref(), Some(WELL_AGAIN));
        // A session never told of it is told nothing.
        assert_eq!(told.due(&2), None);
        told.told(1);
        assert_eq!(told.due(&1), None);
        // An error that comes back waits its half minute again.
        told.seen(Some(&broken), 50 * SEC);
        assert_eq!(told.due(&1), None);
        told.seen(Some(&broken), 80 * SEC);
        assert!(told.due(&1).is_some());
    }

    #[test]
    fn an_old_health_tells_nothing_and_starts_the_wait_again() {
        let mut told = HealthTold::<u8>::default();
        let broken = health(vec![item("eyes", Level::Error, "no frames", 0)], 0);
        told.seen(Some(&broken), 0);
        told.seen(Some(&broken), 30 * SEC);
        assert!(told.due(&1).is_some());
        // No fresh health: nothing is told, not even that all is well.
        told.seen(None, 35 * SEC);
        assert_eq!(told.due(&1), None);
        told.told(1);
        assert_eq!(told.due(&1), None);
        told.seen(Some(&broken), 40 * SEC);
        assert_eq!(told.due(&1), None, "the half minute starts again");
        assert_eq!(HealthTold::<u8>::default().due(&1), None);
    }

    #[test]
    fn the_note_comes_from_what_was_last_read_and_never_from_the_bus() {
        let root = std::path::PathBuf::from(format!("/tmp/lht-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        assert_eq!(read(&bus), None, "nothing kept yet");
        let broken = health(vec![item("eyes", Level::Error, "no frames", 0)], 0);
        let mut says = bus
            .publisher_declared(Health::TOPIC, &Health::declaration(), "health")
            .unwrap();
        says.publish_json(0, &broken).unwrap();
        let mut told = HealthTold::<u8>::default();
        // Past its 15 s, the same value is not read at all.
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        assert!(read_at(&bus, now_ms + 10_000).is_some());
        assert_eq!(read_at(&bus, now_ms + 16_000), None, "older than 15 s");
        told.seen(read(&bus).as_ref(), 0);
        told.seen(read(&bus).as_ref(), 30 * SEC);
        // The bus gone: what is due is still known, with nothing to read.
        drop(says);
        std::fs::remove_dir_all(&root).unwrap();
        let started = std::time::Instant::now();
        assert!(told.due(&1).unwrap().contains("in you now: camera."));
        assert!(started.elapsed() < Duration::from_millis(5));
        assert_eq!(read(&bus), None);
    }

    #[test]
    fn told_once_for_a_set_however_many_turns_and_again_when_it_changes() {
        let mut told = HealthTold::default();
        let eyes = health(vec![item("eyes", Level::Error, "no frames", 0)], 0);
        told.seen(Some(&eyes), 0);
        told.seen(Some(&eyes), 30 * SEC);
        assert!(told.due(&7).is_some());
        told.told(7);
        for at in 7..40 {
            told.seen(Some(&eyes), at * 5 * SEC);
            assert_eq!(told.due(&7), None, "turn at {at}");
        }
        let both = health(
            vec![
                item("eyes", Level::Error, "no frames", 0),
                item("body", Level::Error, "elbow is too warm", 0),
            ],
            0,
        );
        told.seen(Some(&both), 200 * SEC);
        told.seen(Some(&both), 230 * SEC);
        assert!(
            told.due(&7)
                .unwrap()
                .contains("in you now: camera, arm and head.")
        );
    }

    #[test]
    fn the_second_routes_line_is_there_every_turn_while_unwell_and_only_then() {
        let mut told = HealthTold::<()>::default();
        assert_eq!(told.now(), None);
        told.seen(Some(&well()), 0);
        assert_eq!(told.now(), None);
        let eyes = health(vec![item("eyes", Level::Error, "no frames", 0)], 0);
        told.seen(Some(&eyes), 10 * SEC);
        assert_eq!(told.now(), None, "not before half a minute");
        told.seen(Some(&eyes), 40 * SEC);
        let line = told.now().unwrap();
        assert!(line.contains("in you now: camera."), "{line}");
        assert_eq!(told.now(), Some(line), "every turn, while it lasts");
        told.seen(None, 45 * SEC);
        assert_eq!(told.now(), None, "nothing fresh is nothing told");
    }

    #[test]
    fn the_second_routes_watch_keeps_it_from_the_bus_on_its_own_thread() {
        let root = std::path::PathBuf::from(format!("/tmp/lhw-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut says = bus
            .publisher_declared(Health::TOPIC, &Health::declaration(), "health")
            .unwrap();
        // A part that came up well and has been quiet five minutes counts
        // at once.
        let stopped = health(vec![item("ears", Level::Stale, "", 300_000)], 300_000);
        says.publish_json(0, &stopped).unwrap();
        let watched = watch(
            lamp_bus::Bus::new(&root).unwrap(),
            Duration::from_millis(10),
        );
        let started = std::time::Instant::now();
        let line = loop {
            if let Some(line) = watched.lock().unwrap().now() {
                break line;
            }
            assert!(started.elapsed() < Duration::from_secs(5), "never read");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(line.contains("in you now: room microphone."), "{line}");
        says.publish_json(0, &well()).unwrap();
        let started = std::time::Instant::now();
        while watched.lock().unwrap().now().is_some() {
            assert!(started.elapsed() < Duration::from_secs(5), "never cleared");
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(says);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_line_names_at_most_four_parts() {
        let parts: Vec<String> = ["a", "b", "c", "d", "e", "f"].map(String::from).to_vec();
        let note = note(&parts);
        assert!(
            note.contains("in you now: a, b, c, d and 2 more."),
            "{note}"
        );
        assert!(note.len() < 200, "{}", note.len());
    }

    #[test]
    fn the_switch_is_on_only_when_set_and_its_flag_is_taken_off() {
        assert_eq!(arguments(Some("on")), [FLAG]);
        assert!(arguments(None).is_empty() && arguments(Some("yes")).is_empty());
        let flags = vec![FLAG.to_owned(), "--other".to_owned()];
        assert_eq!(take(&flags), (true, &flags[1..]));
        assert_eq!(take(&flags[1..]), (false, &flags[1..]));
    }
}
