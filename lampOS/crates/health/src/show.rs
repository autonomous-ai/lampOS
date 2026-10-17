//! How the lamp is, for a person: what `lamp-health` prints, and the
//! status it ends with.
//!
//! It asks nobody. It reads the two values the bus keeps, `lamp/health`
//! and `board/state`, and shows what is in them. A value that has outlived
//! its time (fifteen seconds) is the same as none: whoever wrote it has
//! stopped. So what is shown is a picture of a moment, a few seconds old
//! at most; a last line says how old only past [`OLD`]. It is written for
//! the lamp's owner (#1327): parts by the names a person knows, the
//! kernel's zone names said plainly; `--json` keeps the programs' words.
use lamp_bus::Bus;
use lamp_msgs::{BoardState, Health, Invalid, Level};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fmt::Write,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// What `show` ends with for each level, so that a script can ask.
pub const STATUS_OK: u8 = 0;
pub const STATUS_WARN: u8 = 1;
pub const STATUS_ERROR: u8 = 2;
/// A part is stale or missing, or nobody is reporting the lamp's health
/// at all.
pub const STATUS_STALE: u8 = 3;
/// `show` was called wrongly. Not 1 or 2, which are levels.
pub const STATUS_USAGE: u8 = 64;

/// What is said when the bus keeps no health.
pub const NOBODY: &str = "Nobody is reporting the lamp's health: lamp-health serve is not running, or the bus is somewhere else.";
/// What is said when this user may not read the bus: the health may well
/// be there (on lamp-75c9 on 17 Oct, `orangepi` was told nobody reported
/// it, while `lamp-health serve` ran).
pub const DENIED: &str = "Can't read the lamp's health as this user: run `sudo lamp-health show`.";

/// The name a person knows each part by, beside the program's (the first
/// builder on #705, 17 Oct 15:08). A part not here is shown as it names
/// itself. `--json` keeps the program's names.
pub fn known_as(name: &str) -> &str {
    match name {
        "voice" => "voice",
        "body" => "arm and head",
        "eyes" => "camera",
        "ears" => "room microphone",
        "name" => "hearing its name",
        "controls" => "button and touch",
        "expression" => "light and moods",
        "air" => "air sensor",
        "schedule" => "reminders",
        "memory" => "what it remembers",
        "actions" => "volume and light commands",
        "policy" => "when to speak",
        "board" => "computer",
        "selftest" => "self-test",
        "tasks" => "work for your computer",
        "keeper" => "parts keeper",
        other => other,
    }
}

/// A value the bus keeps, and how long ago it was written.
#[derive(Clone, Debug, PartialEq)]
pub struct Kept<T> {
    pub value: T,
    pub age: Duration,
}

/// What the bus keeps of the lamp's health and the board's numbers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Look {
    pub health: Option<Kept<Health>>,
    pub board: Option<Kept<BoardState>>,
    /// What was there and could not be made sense of, in words.
    pub troubles: Vec<String>,
    /// This user may not read the bus.
    pub denied: bool,
}

fn kept<T: DeserializeOwned>(
    bus: &Bus,
    topic: &str,
    check: impl Fn(&T) -> Result<(), Invalid>,
    troubles: &mut Vec<String>,
    denied: &mut bool,
) -> Option<Kept<T>> {
    let message = match bus.latest(topic) {
        Ok(Some(message)) => message,
        Ok(None) => return None,
        Err(error) => {
            *denied |= error.kind() == std::io::ErrorKind::PermissionDenied;
            troubles.push(format!("{topic} cannot be read: {error}"));
            return None;
        }
    };
    // Nobody has renewed it within its time: nobody is writing it.
    if message.expired {
        return None;
    }
    let value = match message.parse::<T>() {
        Ok(value) => value,
        Err(error) => {
            troubles.push(format!("{topic} holds something else: {error}"));
            return None;
        }
    };
    if let Err(error) = check(&value) {
        troubles.push(format!("{topic} holds what cannot be: {error}"));
        return None;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64);
    Some(Kept {
        value,
        age: Duration::from_millis(now.saturating_sub(message.header.at_unix_ms)),
    })
}

/// Read the two kept values from `bus`.
pub fn look(bus: &Bus) -> Look {
    let (mut troubles, mut denied) = (Vec::new(), false);
    Look {
        health: kept(
            bus,
            Health::TOPIC,
            Health::check,
            &mut troubles,
            &mut denied,
        ),
        board: kept(
            bus,
            BoardState::TOPIC,
            BoardState::check,
            &mut troubles,
            &mut denied,
        ),
        troubles,
        denied,
    }
}

/// The status `show` ends with: 0 ok, 1 warn, 2 error, and 3 when a part
/// is stale or missing or there is no health to read.
pub fn status(look: &Look) -> u8 {
    match look.health.as_ref().map(|health| health.value.level) {
        Some(Level::Ok) => STATUS_OK,
        Some(Level::Warn) => STATUS_WARN,
        Some(Level::Error) => STATUS_ERROR,
        Some(Level::Stale) | None => STATUS_STALE,
    }
}

/// The two values as one JSON object on one line, each as it was
/// published, and `null` for one that is not there.
pub fn json(look: &Look) -> String {
    #[derive(Serialize)]
    struct Both<'a> {
        health: Option<&'a Health>,
        board: Option<&'a BoardState>,
    }
    serde_json::to_string(&Both {
        health: look.health.as_ref().map(|kept| &kept.value),
        board: look.board.as_ref().map(|kept| &kept.value),
    })
    // Two structures of numbers and text: there is nothing in them that
    // cannot be written.
    .unwrap_or_else(|_| "{\"health\":null,\"board\":null}".into())
}

/// Text from the bus as it is safe to print: anyone can write to the bus,
/// and a terminal obeys what it is sent.
fn plain(text: &str) -> String {
    text.chars()
        .map(|letter| if letter.is_control() { ' ' } else { letter })
        .collect()
}

/// A length of time in its two largest units: `2 d 10 h`, `3 h 12 min`,
/// `4 min 10 s`, `12 s`.
pub fn span(seconds: u64) -> String {
    let (days, hours, minutes) = (seconds / 86_400, seconds / 3_600, seconds / 60);
    if days > 0 {
        format!("{days} d {} h", hours % 24)
    } else if hours > 0 {
        format!("{hours} h {} min", minutes % 60)
    } else if minutes > 0 {
        format!("{minutes} min {} s", seconds % 60)
    } else {
        format!("{seconds} s")
    }
}

/// How long ago: to a tenth of a second while that means something.
fn ago(age: Duration) -> String {
    if age < Duration::from_secs(100) {
        format!("{:.1} s ago", age.as_secs_f64())
    } else {
        format!("{} ago", span(age.as_secs()))
    }
}

fn percent(fraction: f32) -> String {
    format!("{:.0}%", fraction * 100.0)
}

/// The board's numbers on one line.
fn board_line(board: &BoardState) -> String {
    let mut line = String::from(known_as("board"));
    // The hottest sensor; of two as hot, the first by name.
    let hottest = board
        .temperature_c
        .iter()
        .reduce(|hottest, zone| if zone.1 > hottest.1 { zone } else { hottest });
    // The hottest without its kernel name, and no load: neither means
    // anything to the owner (El on #1327); `--json` keeps both.
    if let Some((_, celsius)) = hottest {
        let _ = write!(line, "  hottest {celsius:.0} °C");
    }
    let _ = write!(
        line,
        "  memory {} used  disk {} free  up {}",
        percent(board.memory_used),
        percent(board.disk_free),
        span(board.uptime_s)
    );
    if let Some(dbm) = board.wifi_dbm {
        let _ = write!(line, "  Wi-Fi {dbm} dBm");
    }
    match board.clock_synced {
        Some(true) => line.push_str("  clock set"),
        Some(false) => line.push_str("  clock not set"),
        None => {}
    }
    line
}

/// What a person is shown. `root` is where the bus was looked for, said
/// when nothing was found there.
pub fn text(look: &Look, root: &Path) -> String {
    let mut out = String::new();
    match &look.health {
        None if look.denied => {
            let _ = writeln!(out, "{DENIED}");
        }
        None => {
            let _ = writeln!(out, "{NOBODY}");
            let _ = writeln!(out, "Looked in {}.", root.display());
        }
        Some(Kept { value: health, .. }) => {
            // Worst first, as `lamp-health` sent them, and within a level by
            // the name shown, so the owner finds a part where it should be.
            let mut items: Vec<&lamp_msgs::HealthItem> = health.items.iter().collect();
            items.sort_by(|a, b| {
                b.level
                    .rank()
                    .cmp(&a.level.rank())
                    .then_with(|| known_as(&a.name).cmp(known_as(&b.name)))
            });
            let message = plain(&said(health, items.first().copied()));
            // A voice waiting with its microphones off says why, and a well
            // lamp says it first: muted is not quiet (#1371).
            let off = health.items.iter().find(|item| {
                item.name == "voice" && item.message.starts_with(lamp_msgs::MICROPHONES_OFF)
            });
            let _ = match health.level {
                Level::Ok => match off {
                    Some(voice) => writeln!(out, "The lamp is well; {}.", plain(&voice.message)),
                    None => writeln!(out, "The lamp is well."),
                },
                Level::Warn => writeln!(out, "The lamp has a warning: {message}"),
                Level::Error => writeln!(out, "The lamp is not well: {message}"),
                Level::Stale => writeln!(out, "Not all of the lamp is reporting: {message}"),
            };
            if !health.items.is_empty() || !health.missing.is_empty() {
                out.push('\n');
            }
            let names = items
                .iter()
                .map(|item| known_as(&item.name).chars().count())
                .max()
                .unwrap_or(0);
            for item in items {
                // A part not heard from is "quiet", and its message says for
                // how long: one time, not two (El on #1327).
                let level = match item.level {
                    Level::Stale => "quiet",
                    level => level.as_str(),
                };
                let line = format!(
                    "  {level:<5}  {:<names$}  {}",
                    plain(known_as(&item.name)),
                    plain(&zones_said(&item.message)),
                );
                let _ = writeln!(out, "{}", line.trim_end());
            }
            if !health.missing.is_empty() {
                let missing: Vec<&str> = health.missing.iter().map(|name| known_as(name)).collect();
                let _ = writeln!(out, "  not running: {}", plain(&missing.join(", ")));
            }
        }
    }
    if look.health.is_some() || look.board.is_some() {
        out.push('\n');
    }
    match &look.board {
        Some(board) => {
            let _ = writeln!(out, "{}", board_line(&board.value));
        }
        None if look.health.is_some() => {
            let _ = writeln!(
                out,
                "{}  no numbers: nobody is publishing board/state",
                known_as("board")
            );
        }
        None => {}
    }
    // How old the picture is, only when it is old enough to matter.
    let old = |kept: Duration| kept > OLD;
    match (&look.health, &look.board) {
        (Some(health), Some(board)) if old(health.age) || old(board.age) => {
            let _ = writeln!(
                out,
                "written {}; the board's numbers {}",
                ago(health.age),
                ago(board.age)
            );
        }
        (Some(kept), None) if old(kept.age) => {
            let _ = writeln!(out, "written {}", ago(kept.age));
        }
        (None, Some(kept)) if old(kept.age) => {
            let _ = writeln!(out, "the board's numbers written {}", ago(kept.age));
        }
        _ => {}
    }
    // The caller ends the last line.
    out.truncate(out.trim_end().len());
    out
}

/// A picture older than this says how old it is: twice `serve`'s pace of
/// five seconds, so only a missed publish shows it (the reviewer on #1327:
/// at 5 s a healthy lamp printed "written 5.0 s ago").
pub const OLD: Duration = Duration::from_secs(10);

/// The lamp's message with its part named as a person knows it:
/// `selftest: …` is said `self-test: …`. It names the first row shown
/// when that row is as bad as the lamp, so the two never name different
/// parts of one level (the reviewer on #1327: `serve` breaks a tie by the
/// program's name, the screen by the name shown). A part that never
/// reported is said "not running", as the list says.
fn said(health: &Health, first: Option<&lamp_msgs::HealthItem>) -> String {
    if let Some(item) = first.filter(|item| item.level == health.level && !item.message.is_empty())
    {
        return format!("{}: {}", known_as(&item.name), zones_said(&item.message));
    }
    if let Some(name) = health
        .missing
        .iter()
        .find(|name| health.message.starts_with(&format!("{name}: ")))
    {
        return format!("{}: not running", known_as(name));
    }
    health
        .items
        .iter()
        .find_map(|item| {
            health
                .message
                .strip_prefix(&format!("{}: ", item.name))
                .map(|rest| format!("{}: {}", known_as(&item.name), zones_said(rest)))
        })
        .unwrap_or_else(|| zones_said(&health.message))
}

/// The kernel's thermal zones by what they measure (El and the reviewer on
/// #1327 and #1337): the board's messages name them as the kernel does,
/// which `--json` keeps. A name followed by "," or ";", as the processors'
/// warning writes it, is said the same way.
fn zones_said(text: &str) -> String {
    let mut said = String::new();
    for word in text.split_inclusive(' ') {
        // A zone may come after "(" (the self-test's line wraps a part's
        // reason so, the reviewer on #1337) and before ",", ";", ":", ".", ")".
        let trimmed = word.trim_end();
        let inner = trimmed.trim_start_matches('(');
        let bare = inner.trim_end_matches([',', ';', ':', '.', ')']);
        match zone_said(bare) {
            Some(plain) => {
                said.push_str(&trimmed[..trimmed.len() - inner.len()]);
                said.push_str(plain);
                said.push_str(&inner[bare.len()..]);
                said.push_str(&word[trimmed.len()..]);
            }
            None => said.push_str(word),
        }
    }
    said
}

/// One zone's name as a person knows it; `None` for a word that is no zone.
fn zone_said(zone: &str) -> Option<&'static str> {
    Some(match zone {
        "cpul_thermal_zone" | "cpub_thermal_zone" | "cpu_thermal_zone" | "cpul_idle_zone"
        | "cpub_idle_zone" => "the processor",
        "gpu_thermal_zone" => "the graphics chip",
        "npu_thermal_zone" => "the AI chip",
        "ddr_thermal_zone" => "the memory chips",
        "skin_zone" => "the lamp's case",
        other if other.ends_with("_zone") => "the board",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::HealthItem;

    fn item(level: Level, name: &str, message: &str, age_ms: u64) -> HealthItem {
        HealthItem {
            name: name.into(),
            level,
            message: message.into(),
            age_ms,
        }
    }

    fn health(level: Level, message: &str, items: Vec<HealthItem>, missing: &[&str]) -> Health {
        let health = Health {
            level,
            message: message.into(),
            items,
            missing: missing.iter().map(|name| (*name).to_owned()).collect(),
        };
        assert_eq!(health.check(), Ok(()));
        health
    }

    fn board() -> BoardState {
        BoardState {
            temperature_c: [
                ("cpul_thermal_zone", 64.5),
                ("cpub_thermal_zone", 66.0),
                ("skin_zone", 37.0),
            ]
            .into_iter()
            .map(|(zone, celsius)| (zone.to_owned(), celsius))
            .collect(),
            cpu_load: 0.31,
            memory_used: 0.44,
            disk_free: 0.13,
            uptime_s: 208_800,
            wifi_dbm: None,
            clock_synced: None,
        }
    }

    fn seen(health: Option<Health>, board: Option<BoardState>) -> Look {
        Look {
            health: health.map(|value| Kept {
                value,
                age: Duration::from_millis(4_310),
            }),
            board: board.map(|value| Kept {
                value,
                age: Duration::from_millis(1_240),
            }),
            troubles: Vec::new(),
            denied: false,
        }
    }

    const ROOT: &str = "/run/lampos-bus";

    #[test]
    fn a_lamp_with_troubles_is_shown_worst_first_with_the_board_below() {
        let look = seen(
            Some(health(
                Level::Error,
                "voice: the link to the service is down",
                vec![
                    item(
                        Level::Error,
                        "voice",
                        "the link to the service is down",
                        300,
                    ),
                    item(Level::Warn, "body", "yaw is hot", 400),
                    item(Level::Stale, "eyes", "not heard from for 7 s", 7_300),
                    item(Level::Ok, "memory", "", 600),
                ],
                &["actions", "ring"],
            )),
            Some(board()),
        );
        assert_eq!(
            text(&look, Path::new(ROOT)),
            "The lamp is not well: voice: the link to the service is down\n\
             \n\
             \x20 error  voice              the link to the service is down\n\
             \x20 warn   arm and head       yaw is hot\n\
             \x20 quiet  camera             not heard from for 7 s\n\
             \x20 ok     what it remembers\n\
             \x20 not running: volume and light commands, ring\n\
             \n\
             computer  hottest 66 °C  memory 44% used  disk 13% free  up 2 d 10 h"
        );
        assert_eq!(status(&look), STATUS_ERROR);
    }

    #[test]
    fn each_level_has_its_first_line_and_its_status() {
        for (level, message, first, ends) in [
            (Level::Ok, "", "The lamp is well.", 0),
            (
                Level::Warn,
                "body: yaw is hot",
                "The lamp has a warning: arm and head: yaw is hot",
                1,
            ),
            (
                Level::Error,
                "body: yaw is hot",
                "The lamp is not well: arm and head: yaw is hot",
                2,
            ),
            (
                Level::Stale,
                "body: yaw is hot",
                "Not all of the lamp is reporting: arm and head: yaw is hot",
                3,
            ),
        ] {
            let look = seen(
                Some(health(
                    level,
                    message,
                    vec![item(level, "body", "yaw is hot", 100)],
                    &[],
                )),
                None,
            );
            let shown = text(&look, Path::new(ROOT));
            assert_eq!(shown.lines().next(), Some(first));
            assert_eq!(status(&look), ends, "{level:?}");
            // No numbers of the board: said, and the lamp's health stands.
            assert!(
                shown.ends_with("\n\ncomputer  no numbers: nobody is publishing board/state"),
                "{shown}"
            );
        }
        // Only a missing part.
        let look = seen(
            Some(health(
                Level::Stale,
                "actions: has never reported",
                vec![item(Level::Ok, "board", "", 100)],
                &["actions"],
            )),
            None,
        );
        assert_eq!(status(&look), STATUS_STALE);
        assert!(
            text(&look, Path::new(ROOT)).starts_with(
                "Not all of the lamp is reporting: volume and light commands: not running\n\n\
                 \x20 ok     computer\n\
                 \x20 not running: volume and light commands\n"
            ),
            "{}",
            text(&look, Path::new(ROOT))
        );
        // A lamp with no parts at all is well, and says so in one line.
        let look = seen(Some(health(Level::Ok, "", Vec::new(), &[])), None);
        assert!(
            text(&look, Path::new(ROOT)).starts_with("The lamp is well.\n\ncomputer  no numbers")
        );
    }

    #[test]
    fn with_no_health_it_says_that_nobody_is_reporting_and_ends_with_3() {
        let look = seen(None, None);
        assert_eq!(
            text(&look, Path::new("/tmp/elsewhere")),
            "Nobody is reporting the lamp's health: lamp-health serve is not running, \
             or the bus is somewhere else.\nLooked in /tmp/elsewhere."
        );
        assert_eq!(status(&look), STATUS_STALE);
        assert_eq!(json(&look), "{\"health\":null,\"board\":null}");
        // The board's numbers alone are shown, and are not the lamp's
        // health.
        let look = seen(None, Some(board()));
        let shown = text(&look, Path::new(ROOT));
        assert!(shown.starts_with(NOBODY), "{shown}");
        assert!(
            shown.ends_with(
                "Looked in /run/lampos-bus.\n\ncomputer  hottest 66 °C  \
                 memory 44% used  disk 13% free  up 2 d 10 h"
            ),
            "{shown}"
        );
        assert_eq!(status(&look), STATUS_STALE);
    }

    /// On lamp-75c9 on 17 Oct, run as `orangepi`, it said nobody reported
    /// the lamp's health while `lamp-health serve` ran: the user could not
    /// read the bus. It says so, and how to read it.
    #[test]
    fn a_user_who_may_not_read_the_bus_is_told_to_use_sudo_not_that_nobody_reports() {
        let mut look = seen(None, None);
        look.denied = true;
        assert_eq!(text(&look, Path::new(ROOT)), DENIED);
        assert_eq!(status(&look), STATUS_STALE);
        let root = std::env::temp_dir().join(format!("lamp-health-denied-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(Health::TOPIC)).unwrap();
        std::fs::write(root.join(Health::TOPIC).join("latest"), b"x").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            root.join(Health::TOPIC),
            std::fs::Permissions::from_mode(0o000),
        )
        .unwrap();
        let bus = Bus::new(&root).unwrap();
        let read = super::look(&bus);
        std::fs::set_permissions(
            root.join(Health::TOPIC),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(&root);
        // Root reads anything: only an ordinary user is denied.
        if read.troubles.iter().any(|t| t.contains("denied")) {
            assert!(read.denied, "{:?}", read.troubles);
            assert_eq!(text(&read, Path::new(ROOT)), DENIED);
        }
    }

    /// The owner reads each part by the name they know it by; a part not
    /// in the table keeps its own; the lamp's own message names its part
    /// the same way.
    #[test]
    fn each_part_is_named_as_a_person_knows_it() {
        assert_eq!(known_as("actions"), "volume and light commands");
        assert_eq!(known_as("selftest"), "self-test");
        assert_eq!(known_as("keeper"), "parts keeper");
        assert_eq!(known_as("ring"), "ring");
        let look = seen(
            Some(health(
                Level::Error,
                "selftest: 1 part failing: the air sensor (no reading)",
                vec![item(
                    Level::Error,
                    "selftest",
                    "1 part failing: the air sensor (no reading)",
                    100,
                )],
                &[],
            )),
            None,
        );
        assert!(text(&look, Path::new(ROOT)).starts_with(
            "The lamp is not well: self-test: 1 part failing: the air sensor (no reading)\n"
        ));
    }

    /// The reviewer's P3s on #1327: two parts as bad as each other, the
    /// first line names the one the list puts first; a picture over `OLD`
    /// says how old it is, and one under does not; a zone is said plainly.
    #[test]
    fn the_first_line_names_the_first_row_and_an_old_picture_says_its_age() {
        let mut look = seen(
            Some(health(
                Level::Error,
                "actions: gone",
                vec![
                    item(Level::Error, "actions", "gone", 100),
                    item(
                        Level::Error,
                        "board",
                        "cpul_thermal_zone is at 96 °C: past 90, the second point at which the board slows itself down",
                        100,
                    ),
                ],
                &[],
            )),
            Some(board()),
        );
        let shown = text(&look, Path::new(ROOT));
        assert!(
            shown.starts_with(
                "The lamp is not well: computer: the processor is at 96 °C: past 90, the second point at which the board slows itself down\n\n\
                 \x20 error  computer                   the processor is at 96 °C"
            ),
            "{shown}"
        );
        assert!(!shown.contains("written"), "{shown}");
        look.health.as_mut().unwrap().age = OLD + Duration::from_millis(500);
        let shown = text(&look, Path::new(ROOT));
        assert!(
            shown.ends_with("written 10.5 s ago; the board's numbers 1.2 s ago"),
            "{shown}"
        );
        assert_eq!(
            zones_said("gpu_thermal_zone is at 64 °C"),
            "the graphics chip is at 64 °C"
        );
        assert_eq!(
            zones_said("ddr_thermal_zone and ve_thermal_zone"),
            "the memory chips and the board"
        );
        // Every zone lamp-06c1 has (El, read on the lamp), and the
        // processors' warning as `board.rs` writes it, with a comma.
        for (zone, said) in [
            ("cpul_thermal_zone", "the processor"),
            ("cpub_thermal_zone", "the processor"),
            ("cpul_idle_zone", "the processor"),
            ("cpub_idle_zone", "the processor"),
            ("gpu_thermal_zone", "the graphics chip"),
            ("npu_thermal_zone", "the AI chip"),
            ("ddr_thermal_zone", "the memory chips"),
            ("skin_zone", "the lamp's case"),
        ] {
            assert_eq!(
                zones_said(&format!("{zone} is at 70 °C")),
                format!("{said} is at 70 °C")
            );
        }
        assert_eq!(
            zones_said(
                "the processors are hot: 96 °C at cpub_thermal_zone, 4 below the board's trip at 100"
            ),
            "the processors are hot: 96 °C at the processor, 4 below the board's trip at 100"
        );
        assert_eq!(
            zones_said("busiest: cpub_idle_zone; then"),
            "busiest: the processor; then"
        );
        assert_eq!(zones_said("no zone here, at all"), "no zone here, at all");
        // The self-test's line, as `parts.rs` wraps the board's reason.
        assert_eq!(
            zones_said("the air sensor (no reading), the computer (cpub_idle_zone is at 88 °C)"),
            "the air sensor (no reading), the computer (the processor is at 88 °C)"
        );
        assert_eq!(zones_said("(skin_zone)"), "(the lamp's case)");
    }

    #[test]
    fn the_boards_line_has_what_the_board_could_be_asked() {
        let mut state = board();
        state.wifi_dbm = Some(-56);
        state.clock_synced = Some(true);
        assert_eq!(
            board_line(&state),
            "computer  hottest 66 °C  memory 44% used  disk 13% free  \
             up 2 d 10 h  Wi-Fi -56 dBm  clock set"
        );
        state.clock_synced = Some(false);
        state.wifi_dbm = None;
        state.temperature_c.clear();
        state.uptime_s = 59;
        assert_eq!(
            board_line(&state),
            "computer  memory 44% used  disk 13% free  up 59 s  clock not set"
        );
    }

    /// #1371: a voice waiting on the closed privacy switch is well, and the
    /// first line says the microphones are off, not that the lamp is quiet.
    #[test]
    fn a_voice_muted_by_the_switch_is_said_on_the_first_line() {
        let off = "the microphones are off (the privacy switch)";
        let muted = health(
            Level::Ok,
            "",
            vec![
                item(Level::Ok, "voice", off, 300),
                item(Level::Ok, "air", "", 300),
            ],
            &[],
        );
        let shown = text(
            &seen(Some(muted), Some(board())),
            Path::new("/run/lampos-bus"),
        );
        let mut lines = shown.lines();
        assert_eq!(
            lines.next(),
            Some("The lamp is well; the microphones are off (the privacy switch).")
        );
        assert!(
            shown
                .lines()
                .any(|line| line.contains("voice") && line.ends_with(off)),
            "{shown}"
        );
        // Another part's message is not taken for it.
        let other = health(Level::Ok, "", vec![item(Level::Ok, "air", off, 300)], &[]);
        let shown = text(
            &seen(Some(other), Some(board())),
            Path::new("/run/lampos-bus"),
        );
        assert_eq!(shown.lines().next(), Some("The lamp is well."));
    }

    #[test]
    fn json_is_the_two_values_as_they_were_published_on_one_line() {
        let well = health(Level::Ok, "", vec![item(Level::Ok, "voice", "", 300)], &[]);
        let look = seen(Some(well.clone()), Some(board()));
        let printed = json(&look);
        assert!(!printed.contains('\n'));
        assert!(
            printed.starts_with("{\"health\":{\"level\":\"ok\","),
            "{printed}"
        );
        let read: serde_json::Value = serde_json::from_str(&printed).unwrap();
        assert_eq!(read.as_object().unwrap().len(), 2);
        assert_eq!(
            serde_json::from_value::<Health>(read["health"].clone()).unwrap(),
            well
        );
        assert_eq!(
            serde_json::from_value::<BoardState>(read["board"].clone()).unwrap(),
            board()
        );
        let look = seen(Some(well), None);
        assert!(json(&look).ends_with(",\"board\":null}"));
    }

    #[test]
    fn what_came_from_the_bus_is_printed_on_its_own_line_whatever_it_holds() {
        let look = seen(
            Some(health(
                Level::Warn,
                "bo\u{1b}[2Jdy: yaw\nis hot",
                vec![item(Level::Warn, "bo\u{1b}[2Jdy", "yaw\nis hot", 100)],
                &["act\rions"],
            )),
            None,
        );
        let shown = text(&look, Path::new(ROOT));
        assert!(!shown.contains(['\u{1b}', '\r']), "{shown:?}");
        assert_eq!(shown.lines().count(), 6, "{shown:?}");
        assert!(
            shown.contains("  warn   bo [2Jdy  yaw is hot\n"),
            "{shown:?}"
        );
    }

    #[test]
    fn a_time_is_said_in_its_two_largest_units() {
        for (seconds, said) in [
            (0, "0 s"),
            (59, "59 s"),
            (60, "1 min 0 s"),
            (250, "4 min 10 s"),
            (3_599, "59 min 59 s"),
            (3_600, "1 h 0 min"),
            (11_520, "3 h 12 min"),
            (86_399, "23 h 59 min"),
            (86_400, "1 d 0 h"),
            (208_800, "2 d 10 h"),
        ] {
            assert_eq!(span(seconds), said);
        }
        assert_eq!(ago(Duration::from_millis(349)), "0.3 s ago");
        assert_eq!(ago(Duration::from_millis(99_900)), "99.9 s ago");
        assert_eq!(ago(Duration::from_secs(100)), "1 min 40 s ago");
    }
}
