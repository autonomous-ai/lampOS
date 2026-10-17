//! Every part's newest report, and how they become the lamp's health.
//!
//! A part says how it is: ok, warn or error, and a sentence. It never says
//! of itself that it has gone quiet, because a part that has gone quiet
//! says nothing. That is said here, of a part not heard from for longer
//! than the stale time, and of a part that is expected and has never
//! reported at all.
//!
//! [`health`] is the whole of the rule and touches nothing: the reports,
//! the time and what is expected go in, and a `Health` comes out.
use lamp_msgs::{Diagnostic, Health, HealthItem, Level, Online, SelfTest, Verdict};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

/// No more parts than this are kept. A lamp has a dozen; a publisher that
/// makes up a new name with every message must not fill the memory, or
/// make a `Health` too large for the bus to carry.
pub const MOST_PARTS: usize = 64;
/// A part's name is no longer than this.
pub const MOST_NAME_BYTES: usize = 48;
/// A part's message is cut to this: with 64 parts of the longest name and
/// message, a `Health` is still well under the bus's 60,000 bytes.
pub const MOST_MESSAGE_BYTES: usize = 160;

/// One part's newest report, and when it came.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kept {
    pub level: Level,
    pub message: String,
    /// When it arrived, by this program's own clock.
    pub at: Instant,
    /// Its link to something on the internet, as its `link` fact says it
    /// (`ready`, `connecting`, `down`), with `down_for_s` (#830).
    pub link: Option<String>,
    pub down_for_s: Option<u64>,
}

/// What became of a report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Taken {
    /// It is its part's newest.
    Kept,
    /// Its name is not one a part can have.
    NotAName,
    /// It is from a part not heard before, and 64 are kept already.
    Full,
}

/// The newest report of every part that has reported.
#[derive(Debug, Default)]
pub struct Parts {
    kept: BTreeMap<String, Kept>,
}

impl Parts {
    pub fn new() -> Self {
        Self::default()
    }

    /// Keep `report` as its part's newest, come at `at`.
    pub fn take(&mut self, report: &Diagnostic, at: Instant) -> Taken {
        if !is_name(&report.name) {
            return Taken::NotAName;
        }
        if self.kept.len() >= MOST_PARTS && !self.kept.contains_key(&report.name) {
            return Taken::Full;
        }
        self.kept.insert(
            report.name.clone(),
            Kept {
                level: report.level,
                message: one_line(&report.message),
                at,
                link: fact(report, "link").map(one_line),
                down_for_s: fact(report, "down_for_s").and_then(|value| value.parse().ok()),
            },
        );
        Taken::Kept
    }

    pub fn reports(&self) -> &BTreeMap<String, Kept> {
        &self.kept
    }
}

fn fact<'a>(report: &'a Diagnostic, key: &str) -> Option<&'a str> {
    report
        .values
        .iter()
        .find(|value| value.key == key)
        .map(|value| value.value.as_str())
}

/// The parts whose `link` says whether the lamp can reach the internet,
/// in the order they are believed. Today only the voice's link to its
/// speech service.
pub const ONLINE_FROM: [&str; 1] = ["voice"];

/// Whether the lamp is online at `now` (`lamp/online`): from the first
/// part in [`ONLINE_FROM`] with a fresh report that has a link. `ready`
/// and `connecting` (down under the voice's 5 s grace) are online, `down`
/// is not; with none of them fresh, nobody knows.
pub fn online(reports: &BTreeMap<String, Kept>, now: Instant, stale_after: Duration) -> Online {
    for name in ONLINE_FROM {
        let Some(kept) = reports.get(name) else {
            continue;
        };
        if now.saturating_duration_since(kept.at) >= stale_after {
            continue;
        }
        let online = match kept.link.as_deref() {
            Some("ready" | "connecting") => true,
            Some("down") => false,
            _ => continue,
        };
        return Online {
            online: Some(online),
            from: name.into(),
            down_for_s: if online { None } else { kept.down_for_s },
        };
    }
    Online {
        online: None,
        from: String::new(),
        down_for_s: None,
    }
}

/// `online` as the probe of the internet ([`crate::reach`]) finds it, with
/// the link down: the internet answering is the link's trouble, not the
/// lamp's, so the lamp is online, from `reach`; the internet not answering
/// is offline, from `reach`, the verdict the voice waits for before it says
/// so (El on #1247). With no verdict yet, nobody knows, which Expression
/// takes for online, so its offline face never shows before the probe has
/// spoken. The link not down: as given.
pub fn with_reach(online: Online, verdict: Option<bool>) -> Online {
    match (online.online, verdict) {
        (Some(false), None) => Online {
            online: None,
            from: String::new(),
            down_for_s: None,
        },
        (Some(false), Some(true)) => Online {
            online: Some(true),
            from: "reach".into(),
            down_for_s: None,
        },
        (Some(false), Some(false)) => Online {
            from: "reach".into(),
            ..online
        },
        _ => online,
    }
}

/// Whether a part can be called this: something, on one line, of 48 bytes
/// at most, with no space before or after it.
pub fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MOST_NAME_BYTES
        && name.trim() == name
        && !name.chars().any(char::is_control)
}

/// A message as it is kept: on one line, and cut where it is too long.
fn one_line(message: &str) -> String {
    let line: String = message
        .trim()
        .chars()
        .map(|letter| if letter.is_control() { ' ' } else { letter })
        .collect();
    if line.len() <= MOST_MESSAGE_BYTES {
        return line;
    }
    // Room for the sign that something was cut, which is three bytes.
    let mut end = MOST_MESSAGE_BYTES - 3;
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &line[..end])
}

fn millis(time: Duration) -> u64 {
    u64::try_from(time.as_millis()).unwrap_or(u64::MAX)
}

/// What is said of an item that is not ok, for the lamp's one sentence.
fn said(item: &HealthItem) -> String {
    if item.message.is_empty() {
        format!(
            "{}: {}, and it does not say why",
            item.name,
            item.level.as_str()
        )
    } else {
        format!("{}: {}", item.name, item.message)
    }
}

/// Parts off because a file each needs is not on the lamp (`--needs`): one
/// that has not reported is listed as ok, "off: no <file>". The caller
/// leaves them out of `expected` first, so none is missing and the level
/// is untouched (an ok item lowers nothing); the result still passes
/// `Health::check`. A part that reports anyway keeps its own report.
pub fn mark_off(health: &mut Health, off: &[(String, String)]) {
    for (name, path) in off {
        if health.items.iter().any(|item| &item.name == name) {
            continue;
        }
        let file = path.rsplit('/').next().unwrap_or(path);
        health.items.push(HealthItem {
            name: name.clone(),
            level: Level::Ok,
            message: one_line(&format!("off: no {file} on this lamp ({path})")),
            age_ms: 0,
        });
    }
    health.items.sort_by(|a, b| {
        b.level
            .rank()
            .cmp(&a.level.rank())
            .then_with(|| a.name.cmp(&b.name))
    });
}

/// The name the self-test's newest run has among the lamp's parts.
pub const SELFTEST: &str = "selftest";

/// The self-test's runs as they came (`lamp/selftest`), as one item of the
/// lamp's health (#812: a lamp says a part has failed before its person
/// finds out). Each part keeps its last *checked* verdict: a part that
/// failed stays failed until a later run checks it again and it passes, or
/// is not fitted. A run that could not check it (`not_checked`, or not in
/// the run at all: a passive run checks no speaker, ring colours or head
/// move) leaves its fail standing (the reviewer on #1243). Kept in memory
/// only: `lamp/selftest` is on the bus's tmpfs, so a reboot forgets both.
#[derive(Debug, Default)]
pub struct SelfTests {
    /// When the newest run began (the wall clock's seconds), and how many
    /// of its parts it could not check.
    newest: Option<(f64, usize)>,
    /// The parts whose last checked verdict was a fail, and why, in the
    /// order they first failed.
    failed: Vec<(String, String)>,
    /// The newest run's line for the lamp's USB hub dropping (its
    /// `voice_mic` part's `usb_hub_said`), until a run says it never did:
    /// a warning, since the lamp works once the hub is back, but the voice,
    /// the body and the camera all went with it (the first builder on
    /// #705, 17 Oct 17:31).
    hub: Option<String>,
    /// The same line with short names (`usb_hub_said_short`), to stand
    /// before failed parts.
    hub_short: Option<String>,
}

impl SelfTests {
    /// Take a whole run (one that passed `SelfTest::check`).
    pub fn take(&mut self, test: &SelfTest) {
        for part in &test.parts {
            match part.verdict {
                Verdict::Fail => {
                    if let Some(kept) = self.failed.iter_mut().find(|(name, _)| *name == part.part)
                    {
                        kept.1.clone_from(&part.reason);
                    } else if self.failed.len() < MOST_PARTS {
                        self.failed.push((part.part.clone(), part.reason.clone()));
                    }
                }
                Verdict::Pass | Verdict::NotFitted => {
                    self.failed.retain(|(name, _)| *name != part.part);
                }
                Verdict::NotChecked => {}
            }
            if part.part == "voice_mic" {
                let value = |key: &str| {
                    part.values
                        .iter()
                        .find(|value| value.key == key)
                        .map(|value| value.value.clone())
                };
                self.hub = value("usb_hub_said");
                self.hub_short = value("usb_hub_said_short");
            }
        }
        let unchecked = test
            .parts
            .iter()
            .filter(|part| part.verdict == Verdict::NotChecked)
            .count();
        self.newest = Some((test.started, unchecked));
    }

    /// The item, once a run has come: `error` naming each part still
    /// failed and why, after the hub's line when the lamp's USB hub
    /// dropped (with short names, when the whole line would not fit); else `warn` with the hub's line; else `ok`, saying how many
    /// parts the newest run could not check. Its age is since the newest run began
    /// (`now_unix_s`, the wall clock's seconds); it is never stale.
    pub fn item(&self, now_unix_s: f64) -> Option<HealthItem> {
        let (started, unchecked) = self.newest?;
        let (level, message) = if self.failed.is_empty() {
            match &self.hub {
                Some(hub) => (Level::Warn, hub.clone()),
                None => {
                    let message = match unchecked {
                        0 => "passed".to_owned(),
                        n => format!("passed; {n} not checked"),
                    };
                    (Level::Ok, message)
                }
            }
        } else {
            let failed: Vec<String> = self
                .failed
                .iter()
                .map(|(part, reason)| format!("{} ({reason})", part_known_as(part)))
                .collect();
            let parts = if failed.len() == 1 { "part" } else { "parts" };
            let mut message = format!("{} {parts} failing: {}", failed.len(), failed.join(", "));
            // The hub first, so the line's cap never cuts it. When the
            // whole of it and the failed parts don't fit, its short names,
            // so the part's name still fits and at worst the end of its
            // reason is cut. A run from before the short line gives the
            // hub's drops alone.
            if let Some(hub) = &self.hub {
                let whole = format!("{hub}; {message}");
                message = if whole.len() <= MOST_MESSAGE_BYTES {
                    whole
                } else {
                    let short = self.hub_short.as_deref().unwrap_or_else(|| {
                        hub.split_once("; ")
                            .map_or(hub.as_str(), |(drops, _)| drops)
                    });
                    format!("{short}; {message}")
                };
            }
            (Level::Error, message)
        };
        let age_s = (now_unix_s - started).max(0.0);
        Some(HealthItem {
            name: SELFTEST.to_owned(),
            level,
            message: one_line(&message),
            age_ms: (age_s * 1_000.0).min(u64::MAX as f64) as u64,
        })
    }
}

/// A self-test part as a person knows it, for the lamp's health line (the
/// first builder on #705, 17 Oct 15:08). A part not here keeps its name.
pub fn part_known_as(part: &str) -> &str {
    match part {
        "voice_mic" => "the voice microphone",
        "room_mic" => "the room microphone",
        "speaker" => "the speaker",
        "camera" => "the camera",
        "touch" => "the touch pads",
        "touch_by_hand" => "the touch pads, touched by hand",
        "button" => "the button",
        "ring" => "the light ring",
        "head_move" => "the head's small move",
        "base_yaw" => "the base's turn",
        "shoulder" => "the shoulder",
        "elbow" => "the elbow",
        "head_roll" => "the head's tilt",
        "head_pitch" => "the head's nod",
        "air" => "the air sensor",
        "storage" => "the SD card",
        "board" => "the computer",
        other => other,
    }
}

/// Add the self-test's item to `health`, in its place among the items,
/// and work the lamp's level and message out again as [`health`] does: a
/// failed part makes the lamp `error`. A part that reported under the same
/// name keeps its own report. What comes out still passes `Health::check`.
pub fn mark_selftest(health: &mut Health, item: HealthItem) {
    if health.items.iter().any(|kept| kept.name == item.name) {
        return;
    }
    health.items.push(item);
    health.items.sort_by(|a, b| {
        b.level
            .rank()
            .cmp(&a.level.rank())
            .then_with(|| a.name.cmp(&b.name))
    });
    let worst = health.items.first().filter(|item| item.level != Level::Ok);
    (health.level, health.message) = match (worst, health.missing.first()) {
        (Some(item), _) => (item.level, said(item)),
        (None, Some(name)) => (Level::Stale, format!("{name}: has never reported")),
        (None, None) => (Level::Ok, String::new()),
    };
}

/// How the whole lamp is at `now`, from each part's newest report.
///
/// - A part heard from within `stale_after` has the level and the message
///   it gave. One not heard from for longer is `stale`, and its message
///   says for how many whole seconds. One that had said `warn` or `error`
///   before it went quiet keeps that level, and what it said stays in
///   front of the seconds: a body that stopped itself and said why is not
///   made milder by its silence. A part that calls itself `stale` is
///   taken at its word.
/// - `missing` is every name in `expected` that has never reported, in the
///   order given, each once.
/// - The items are worst first (`Level::rank`), then by name. The lamp's
///   level is the worst item's, and `stale` at least when a part is
///   missing.
/// - The message is empty when the level is ok. Otherwise it is the name
///   and the message of the first item that is not ok, or, when only a
///   missing part makes the lamp not ok, that part's name and `has never
///   reported`.
///
/// What comes out passes `Health::check`.
pub fn health(
    reports: &BTreeMap<String, Kept>,
    now: Instant,
    expected: &[String],
    stale_after: Duration,
) -> Health {
    let limit = millis(stale_after);
    let mut items: Vec<HealthItem> = reports
        .iter()
        .map(|(name, kept)| {
            let age_ms = millis(now.saturating_duration_since(kept.at));
            let (level, message) = if age_ms > limit {
                // Quiet. It is stale at least, and a part that said
                // something was wrong before it went quiet is no better
                // for having gone quiet: it keeps the level it gave and
                // what it last said, which is often why it went quiet.
                let quiet = format!("not heard from for {} s", age_ms / 1_000);
                if kept.level.rank() > Level::Stale.rank() && !kept.message.is_empty() {
                    (kept.level, format!("{}; {quiet}", kept.message))
                } else if kept.level.rank() > Level::Stale.rank() {
                    (kept.level, quiet)
                } else {
                    (Level::Stale, quiet)
                }
            } else {
                (kept.level, kept.message.clone())
            };
            HealthItem {
                name: name.clone(),
                level,
                message,
                age_ms,
            }
        })
        .collect();
    items.sort_by(|a, b| {
        b.level
            .rank()
            .cmp(&a.level.rank())
            .then_with(|| a.name.cmp(&b.name))
    });
    let mut missing: Vec<String> = Vec::new();
    for name in expected {
        if !reports.contains_key(name) && !missing.contains(name) {
            missing.push(name.clone());
        }
    }
    let worst = items.first().filter(|item| item.level != Level::Ok);
    let (level, message) = match (worst, missing.first()) {
        (Some(item), _) => (item.level, said(item)),
        (None, Some(name)) => (Level::Stale, format!("{name}: has never reported")),
        (None, None) => (Level::Ok, String::new()),
    };
    Health {
        level,
        message,
        items,
        missing,
    }
}

/// Whether two healths say the same but for the ages: each item's
/// `age_ms`, and the count of seconds in the message of a part that is
/// stale because it has not been heard from. The lamp's own message is not
/// compared: it follows from the items and the missing.
///
/// This is what decides whether a health is news. A part that stays quiet
/// is news once, when it goes stale, and not again with every second.
pub fn same_but_for_ages(a: &Health, b: &Health, stale_after: Duration) -> bool {
    let limit = millis(stale_after);
    let quiet = |item: &HealthItem| item.age_ms > limit;
    a.level == b.level
        && a.missing == b.missing
        && a.items.len() == b.items.len()
        && a.items.iter().zip(&b.items).all(|(a, b)| {
            a.name == b.name
                && a.level == b.level
                && quiet(a) == quiet(b)
                && (quiet(a) || a.message == b.message)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const STALE: Duration = Duration::from_secs(5);

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn report(level: Level, name: &str, message: &str) -> Diagnostic {
        Diagnostic {
            level,
            name: name.into(),
            message: message.into(),
            values: Vec::new(),
        }
    }

    /// The reports of a test: each a level, a name, a message and how
    /// long before `now` it came.
    fn lamp(reports: &[(Level, &str, &str, u64)]) -> (Parts, Instant) {
        // Far enough from the clock's start for every age to be taken off.
        let now = Instant::now() + Duration::from_secs(3_600);
        let mut parts = Parts::new();
        for (level, name, message, age_ms) in reports {
            assert_eq!(
                parts.take(&report(*level, name, message), now - ms(*age_ms)),
                Taken::Kept
            );
        }
        (parts, now)
    }

    fn of(reports: &[(Level, &str, &str, u64)], expected: &[&str]) -> Health {
        let (parts, now) = lamp(reports);
        let expected: Vec<String> = expected.iter().map(|name| (*name).to_owned()).collect();
        let health = health(parts.reports(), now, &expected, STALE);
        assert_eq!(health.check(), Ok(()), "{health:?}");
        health
    }

    fn shape(health: &Health) -> Vec<(&str, Level, &str)> {
        health
            .items
            .iter()
            .map(|item| (item.name.as_str(), item.level, item.message.as_str()))
            .collect()
    }

    fn run(parts: &[(&str, Verdict, &str)], started: f64) -> SelfTest {
        let parts: Vec<lamp_msgs::PartResult> = parts
            .iter()
            .map(|(part, verdict, reason)| lamp_msgs::PartResult {
                part: (*part).into(),
                verdict: *verdict,
                reason: (*reason).into(),
                values: Vec::new(),
                took: 0.1,
            })
            .collect();
        SelfTest {
            occasion: lamp_msgs::Occasion::FirstBoot,
            started,
            took: 2.0,
            verdict: SelfTest::verdict_of(&parts),
            parts,
        }
    }

    /// #812: the item names each part that failed and why, says how many
    /// the newest run could not check when none failed, and is as old as
    /// the newest run; there is none before a run came.
    /// The first builder on #705, 17 Oct 17:31: the self-test's line for the
    /// lamp's USB hub dropping is a warning on its own, beside a failed
    /// part's error, and gone once a run says nothing of it.
    #[test]
    fn a_hub_that_dropped_is_a_warning_until_a_run_says_it_did_not() {
        // lamp-4ace's four ports, as the self-test says them.
        let said = "the lamp's USB hub dropped 2 times since boot; the voice mic, the body's servo port, \
                    the speaker with the room mic and the camera went with it";
        let short = "the lamp's USB hub dropped 2 times since boot, with the voice mic, servo port, \
                     speaker/room mic and camera";
        let with_hub = |verdict: Verdict| {
            let mut test = run(&[("voice_mic", verdict, "")], 1_000.0);
            test.parts[0] = test.parts[0]
                .clone()
                .with("usb_hub_said", said)
                .with("usb_hub_said_short", short);
            test
        };
        let mut tests = SelfTests::default();
        tests.take(&with_hub(Verdict::Pass));
        let item = tests.item(1_001.0).unwrap();
        assert_eq!((item.level, item.message.as_str()), (Level::Warn, said));
        tests.take(&run(&[("storage", Verdict::Fail, "9 re-tunes")], 1_002.0));
        let item = tests.item(1_003.0).unwrap();
        assert_eq!(item.level, Level::Error);
        // Beside a failed part, the hub's short line: what went with it
        // and the part both said.
        assert_eq!(
            item.message,
            format!("{short}; 1 part failing: the SD card (9 re-tunes)")
        );
        // Whole as published even with the log wrapped ("at least"): 156
        // bytes, inside the cap, nothing cut.
        let wrapped_short = short.replace("dropped 2", "dropped at least 2");
        let mut test = run(&[("storage", Verdict::Fail, "9 re-tunes")], 1_010.0);
        test.parts.push(
            run(&[("voice_mic", Verdict::Pass, "")], 1_010.0).parts[0]
                .clone()
                .with(
                    "usb_hub_said",
                    said.replace("dropped 2", "dropped at least 2"),
                )
                .with("usb_hub_said_short", &wrapped_short),
        );
        let mut wrapped = SelfTests::default();
        wrapped.take(&test);
        let message = wrapped.item(1_011.0).unwrap().message;
        assert_eq!(
            message,
            format!("{wrapped_short}; 1 part failing: the SD card (9 re-tunes)")
        );
        assert!(message.len() <= MOST_MESSAGE_BYTES, "{}", message.len());
        // A short hub line fits whole beside a failed part.
        let mut tests = SelfTests::default();
        let mut test = run(&[("storage", Verdict::Fail, "9 re-tunes")], 1_000.0);
        test.parts.push(
            run(&[("voice_mic", Verdict::Pass, "")], 1_000.0).parts[0]
                .clone()
                .with(
                    "usb_hub_said",
                    "the lamp's USB hub dropped 1 time since boot; the camera went with it",
                ),
        );
        tests.take(&test);
        assert_eq!(
            tests.item(1_001.0).unwrap().message,
            "the lamp's USB hub dropped 1 time since boot; the camera went with it; \
             1 part failing: the SD card (9 re-tunes)"
        );
        // Whatever fails, the hub's drops come first and whole, and once:
        // the log wrapped, and the mic gone with a long reason.
        let wrapped = said.replace("dropped 2", "dropped at least 2");
        assert!(wrapped.len() <= MOST_MESSAGE_BYTES);
        let mut tests = SelfTests::default();
        let mut test = run(
            &[(
                "voice_mic",
                Verdict::Fail,
                "not on USB now: no sound card Device, the voice mic alone dropped 1 time since boot",
            )],
            1_008.0,
        );
        let wrapped_short = short.replace("dropped 2", "dropped at least 2");
        let failed = test.parts[0].clone();
        test.parts[0] = failed
            .clone()
            .with("usb_hub_said", &wrapped)
            .with("usb_hub_said_short", &wrapped_short);
        tests.take(&test);
        let item = tests.item(1_009.0).unwrap();
        assert_eq!(item.level, Level::Error);
        assert!(
            item.message.starts_with(&format!(
                "{wrapped_short}; 1 part failing: the voice microphone ("
            )),
            "{}",
            item.message
        );
        assert_eq!(item.message.matches("USB hub").count(), 1);
        // A run from before the short line: the hub's drops alone.
        let mut tests = SelfTests::default();
        test.parts[0] = failed.with("usb_hub_said", &wrapped);
        tests.take(&test);
        assert!(tests.item(1_009.0).unwrap().message.starts_with(
            "the lamp's USB hub dropped at least 2 times since boot; 1 part failing: "
        ));
        // A run whose mic says no hub drop clears it; one without the mic
        // keeps it.
        let mut tests = SelfTests::default();
        tests.take(&with_hub(Verdict::Pass));
        tests.take(&run(&[("ring", Verdict::Pass, "")], 1_004.0));
        assert_eq!(tests.item(1_005.0).unwrap().level, Level::Warn);
        tests.take(&run(&[("voice_mic", Verdict::Pass, "")], 1_006.0));
        let item = tests.item(1_007.0).unwrap();
        assert_eq!((item.level, item.message.as_str()), (Level::Ok, "passed"));
    }

    #[test]
    fn the_selftests_item_names_what_failed_and_is_as_old_as_its_run() {
        let mut tests = SelfTests::default();
        assert_eq!(tests.item(0.0), None);
        tests.take(&run(
            &[
                ("ring", Verdict::Fail, "red did not light"),
                ("air", Verdict::NotFitted, "no sensor"),
                (
                    "storage",
                    Verdict::Fail,
                    "1 I/O error on the card since boot",
                ),
            ],
            1_000.0,
        ));
        let item = tests.item(1_090.5).unwrap();
        assert_eq!(item.name, SELFTEST);
        assert_eq!(item.level, Level::Error);
        assert_eq!(
            item.message,
            "2 parts failing: the light ring (red did not light), the SD card (1 I/O error on the card since boot)"
        );
        assert_eq!(item.age_ms, 90_500);
        let mut tests = SelfTests::default();
        tests.take(&run(
            &[
                ("ring", Verdict::Pass, ""),
                ("touch", Verdict::NotChecked, "nobody there"),
                ("air", Verdict::NotFitted, "no sensor"),
            ],
            1_000.0,
        ));
        let item = tests.item(1_000.0).unwrap();
        assert_eq!(
            (item.level, item.message.as_str()),
            (Level::Ok, "passed; 1 not checked")
        );
        let mut tests = SelfTests::default();
        tests.take(&run(&[("ring", Verdict::Pass, "")], 1_000.0));
        assert_eq!(tests.item(999.0).unwrap().message, "passed");
        // A clock behind the run's start is no negative age.
        assert_eq!(tests.item(999.0).unwrap().age_ms, 0);
    }

    /// The reviewer on #1243: a part that failed stays failed until a later
    /// run checks it. A run that could not check it (`not_checked`, or
    /// without it: a passive run checks no ring colours) leaves the fail;
    /// a run where it passes, or is not fitted, clears it. Its reason is the
    /// newest fail's.
    #[test]
    fn a_failed_part_stays_failed_until_a_later_run_checks_it() {
        let mut tests = SelfTests::default();
        tests.take(&run(
            &[
                (
                    "storage",
                    Verdict::Fail,
                    "1 I/O error on the card since boot",
                ),
                ("ring", Verdict::Fail, "red did not light"),
            ],
            0.0,
        ));
        tests.take(&run(
            &[("storage", Verdict::NotChecked, "no CAP_SYSLOG")],
            100.0,
        ));
        let item = tests.item(100.0).unwrap();
        assert_eq!(
            (item.level, item.message.as_str()),
            (
                Level::Error,
                "2 parts failing: the SD card (1 I/O error on the card since boot), the light ring (red did not light)"
            )
        );
        tests.take(&run(
            &[(
                "storage",
                Verdict::Fail,
                "2 I/O errors on the card since boot",
            )],
            200.0,
        ));
        assert_eq!(
            tests.item(200.0).unwrap().message,
            "2 parts failing: the SD card (2 I/O errors on the card since boot), the light ring (red did not light)"
        );
        tests.take(&run(
            &[
                ("storage", Verdict::Pass, ""),
                ("ring", Verdict::NotFitted, "none"),
            ],
            300.0,
        ));
        let item = tests.item(300.0).unwrap();
        assert_eq!((item.level, item.message.as_str()), (Level::Ok, "passed"));
    }

    /// A failed run makes a well lamp `error`, with the item first and the
    /// lamp's message from it; a passing run lowers nothing; a part that
    /// reported as `selftest` keeps its own report.
    #[test]
    fn a_failed_selftest_makes_the_lamp_error_and_still_passes_the_check() {
        let mut failed = SelfTests::default();
        failed.take(&run(&[("ring", Verdict::Fail, "red did not light")], 0.0));
        let mut health = of(&[(Level::Ok, "voice", "", 0)], &["voice", "body"]);
        mark_selftest(&mut health, failed.item(1.0).unwrap());
        assert_eq!(health.check(), Ok(()), "{health:?}");
        assert_eq!(health.items[0].name, SELFTEST);
        assert_eq!(
            (health.level, health.message.as_str()),
            (
                Level::Error,
                "selftest: 1 part failing: the light ring (red did not light)"
            )
        );
        assert_eq!(health.missing, ["body"]);

        let mut passed = SelfTests::default();
        passed.take(&run(&[("ring", Verdict::Pass, "")], 0.0));
        let mut health = of(&[(Level::Ok, "voice", "", 0)], &["voice"]);
        mark_selftest(&mut health, passed.item(1.0).unwrap());
        assert_eq!(health.check(), Ok(()), "{health:?}");
        assert_eq!((health.level, health.message.as_str()), (Level::Ok, ""));
        assert_eq!(health.items.len(), 2);

        let mut health = of(&[(Level::Warn, SELFTEST, "its own", 0)], &[]);
        mark_selftest(&mut health, failed.item(1.0).unwrap());
        assert_eq!(shape(&health), [(SELFTEST, Level::Warn, "its own")]);
    }

    /// #811: a part that runs only where its file is (tasks needs Harness)
    /// is listed as off while the file is missing: ok, never missing, and the
    /// lamp's health stays ok.
    #[test]
    fn a_part_off_for_want_of_its_file_is_listed_off_not_missing() {
        let parts = Parts::new();
        let now = Instant::now();
        let expected = vec!["voice".to_owned()];
        let mut health = health(parts.reports(), now, &expected, STALE);
        assert_eq!(health.missing, vec!["voice".to_owned()]);
        mark_off(
            &mut health,
            &[("tasks".into(), "/usr/local/bin/harness".into())],
        );
        assert_eq!(
            health.missing,
            vec!["voice".to_owned()],
            "only the off part is spared"
        );
        let tasks = health
            .items
            .iter()
            .find(|item| item.name == "tasks")
            .unwrap();
        assert_eq!(tasks.level, Level::Ok);
        assert_eq!(
            tasks.message,
            "off: no harness on this lamp (/usr/local/bin/harness)"
        );
        let mut alone = super::health(parts.reports(), now, &[], STALE);
        mark_off(
            &mut alone,
            &[("tasks".into(), "/usr/local/bin/harness".into())],
        );
        assert_eq!(alone.level, Level::Ok);
        assert!(alone.missing.is_empty());
    }
    #[test]
    fn a_lamp_whose_parts_are_all_well_is_ok_and_says_nothing() {
        let health = of(
            &[
                (Level::Ok, "voice", "", 300),
                (Level::Ok, "body", "", 400),
                (Level::Ok, "eyes", "", 100),
            ],
            &["voice", "body"],
        );
        assert_eq!((health.level, health.message.as_str()), (Level::Ok, ""));
        // Among parts as well as each other, by name.
        assert_eq!(
            shape(&health),
            [
                ("body", Level::Ok, ""),
                ("eyes", Level::Ok, ""),
                ("voice", Level::Ok, "")
            ]
        );
        assert_eq!(
            health
                .items
                .iter()
                .map(|item| item.age_ms)
                .collect::<Vec<_>>(),
            [400, 100, 300]
        );
        assert!(health.missing.is_empty());
        // Nothing at all has reported and nothing is expected: ok too.
        let nothing = of(&[], &[]);
        assert_eq!((nothing.level, nothing.message.as_str()), (Level::Ok, ""));
        assert!(nothing.items.is_empty() && nothing.missing.is_empty());
    }

    #[test]
    fn the_items_are_worst_first_then_by_name_and_the_lamp_is_as_bad_as_its_worst() {
        let health = of(
            &[
                (Level::Ok, "memory", "", 600),
                (Level::Warn, "body", "yaw is hot", 400),
                (Level::Ok, "actions", "", 200),
                (
                    Level::Error,
                    "voice",
                    "the link to the service is down",
                    300,
                ),
                (Level::Ok, "eyes", "", 7_300),
                (Level::Warn, "board", "the disk is filling up", 100),
                (Level::Error, "ring", "no light answers", 900),
            ],
            &[],
        );
        assert_eq!(
            shape(&health),
            [
                ("ring", Level::Error, "no light answers"),
                ("voice", Level::Error, "the link to the service is down"),
                ("board", Level::Warn, "the disk is filling up"),
                ("body", Level::Warn, "yaw is hot"),
                ("eyes", Level::Stale, "not heard from for 7 s"),
                ("actions", Level::Ok, ""),
                ("memory", Level::Ok, ""),
            ]
        );
        assert_eq!(health.level, Level::Error);
        // The first item that is not ok.
        assert_eq!(health.message, "ring: no light answers");
    }

    #[test]
    fn each_level_is_the_lamps_when_nothing_is_worse() {
        for (worst, others, message) in [
            (Level::Error, [Level::Warn, Level::Ok], "a: wrong"),
            (Level::Warn, [Level::Ok, Level::Ok], "a: wrong"),
            (Level::Stale, [Level::Ok, Level::Ok], "a: wrong"),
        ] {
            let health = of(
                &[
                    (others[0], "b", "also", 10),
                    (worst, "a", "wrong", 10),
                    (others[1], "c", "", 10),
                ],
                &[],
            );
            assert_eq!(health.level, worst);
            assert_eq!(health.message, message);
            assert_eq!(health.items[0].name, "a");
        }
        // Stale is worse than ok and not as bad as a warning.
        let health = of(
            &[
                (Level::Ok, "voice", "", 6_000),
                (Level::Warn, "body", "yaw is hot", 10),
            ],
            &[],
        );
        assert_eq!(health.level, Level::Warn);
        assert_eq!(health.message, "body: yaw is hot");
        assert_eq!(
            shape(&health)[1],
            ("voice", Level::Stale, "not heard from for 6 s")
        );
    }

    #[test]
    fn a_part_is_stale_once_it_has_been_quiet_for_longer_than_the_stale_time() {
        // To the millisecond: at the limit it is still what it said.
        let at = of(&[(Level::Warn, "body", "yaw is hot", 5_000)], &[]);
        assert_eq!(shape(&at), [("body", Level::Warn, "yaw is hot")]);
        assert_eq!(at.level, Level::Warn);
        let past = of(&[(Level::Ok, "body", "", 5_001)], &[]);
        assert_eq!(
            shape(&past),
            [("body", Level::Stale, "not heard from for 5 s")]
        );
        assert_eq!(
            (past.level, past.message.as_str()),
            (Level::Stale, "body: not heard from for 5 s")
        );
        assert_eq!(past.items[0].age_ms, 5_001);
        // A part that said something was wrong and then went quiet is no
        // better for it: it keeps its level, and what it said.
        let warned = of(&[(Level::Warn, "body", "yaw is hot", 5_001)], &[]);
        assert_eq!(
            shape(&warned),
            [("body", Level::Warn, "yaw is hot; not heard from for 5 s")]
        );
        assert_eq!(warned.message, "body: yaw is hot; not heard from for 5 s");
        let error = of(
            &[(Level::Error, "body", "stopped: elbow is too warm", 9_400)],
            &[],
        );
        assert_eq!(
            (error.level, error.message.as_str()),
            (
                Level::Error,
                "body: stopped: elbow is too warm; not heard from for 9 s"
            )
        );
        // With nothing said beside the level, the seconds alone.
        let silent = of(&[(Level::Error, "voice", "", 6_000)], &[]);
        assert_eq!(
            shape(&silent),
            [("voice", Level::Error, "not heard from for 6 s")]
        );
        // And it is still worse than a part that was well and went quiet.
        let both = of(
            &[
                (Level::Ok, "eyes", "", 8_000),
                (Level::Warn, "body", "yaw is hot", 8_000),
            ],
            &[],
        );
        assert_eq!(both.items[0].name, "body");
        assert_eq!(both.level, Level::Warn);
        // Whole seconds, not rounded up.
        let later = of(&[(Level::Ok, "eyes", "", 7_999)], &[]);
        assert_eq!(later.items[0].message, "not heard from for 7 s");
        let long = of(&[(Level::Ok, "eyes", "", 3_600_000)], &[]);
        assert_eq!(long.items[0].message, "not heard from for 3600 s");
        // Another stale time is another limit.
        let (parts, now) = lamp(&[(Level::Ok, "eyes", "", 150)]);
        let quick = health(parts.reports(), now, &[], ms(149));
        assert_eq!(quick.check(), Ok(()));
        assert_eq!(
            shape(&quick),
            [("eyes", Level::Stale, "not heard from for 0 s")]
        );
        let slow = health(parts.reports(), now, &[], ms(150));
        assert_eq!(shape(&slow), [("eyes", Level::Ok, "")]);
    }

    #[test]
    fn a_part_that_comes_back_from_stale_is_what_it_says_again() {
        let (mut parts, now) = lamp(&[
            (Level::Ok, "eyes", "", 9_000),
            (Level::Ok, "voice", "", 200),
        ]);
        let before = health(parts.reports(), now, &[], STALE);
        assert_eq!(before.check(), Ok(()));
        assert_eq!(
            (before.level, before.message.as_str()),
            (Level::Stale, "eyes: not heard from for 9 s")
        );
        // It reports again, and what is kept is the new report alone.
        let later = now + ms(250);
        assert_eq!(
            parts.take(&report(Level::Warn, "eyes", "the camera is slow"), later),
            Taken::Kept
        );
        let after = health(parts.reports(), later + ms(50), &[], STALE);
        assert_eq!(after.check(), Ok(()));
        assert_eq!(
            shape(&after),
            [
                ("eyes", Level::Warn, "the camera is slow"),
                ("voice", Level::Ok, "")
            ]
        );
        assert_eq!(after.items[0].age_ms, 50);
        assert_eq!(after.items[1].age_ms, 500);
        // And well again with its next.
        parts.take(&report(Level::Ok, "eyes", ""), later + ms(1_000));
        let well = health(parts.reports(), later + ms(1_000), &[], STALE);
        assert_eq!((well.level, well.message.as_str()), (Level::Ok, ""));
        // A report that seems to come from after `now` is of no age.
        let early = health(parts.reports(), later, &[], STALE);
        assert_eq!(early.items[0].age_ms, 0);
    }

    #[test]
    fn a_part_that_calls_itself_stale_is_taken_at_its_word() {
        let health = of(
            &[
                (Level::Stale, "voice", "the service has not answered", 300),
                (Level::Ok, "body", "", 300),
            ],
            &[],
        );
        assert_eq!(
            shape(&health),
            [
                ("voice", Level::Stale, "the service has not answered"),
                ("body", Level::Ok, "")
            ]
        );
        assert_eq!(
            (health.level, health.message.as_str()),
            (Level::Stale, "voice: the service has not answered")
        );
        // Until it goes quiet itself.
        let quiet = of(&[(Level::Stale, "voice", "its own words", 6_500)], &[]);
        assert_eq!(quiet.items[0].message, "not heard from for 6 s");
    }

    #[test]
    fn expected_parts_that_never_reported_are_missing_in_the_order_given() {
        let health = of(
            &[(Level::Ok, "voice", "", 100)],
            &["memory", "voice", "actions", "memory", "body"],
        );
        assert_eq!(health.missing, ["memory", "actions", "body"]);
        // A missing part alone makes the lamp stale, and is what is said.
        assert_eq!(
            (health.level, health.message.as_str()),
            (Level::Stale, "memory: has never reported")
        );
        // Nothing has reported at all.
        let empty = of(&[], &["voice"]);
        assert_eq!(
            (empty.level, empty.message.as_str()),
            (Level::Stale, "voice: has never reported")
        );
        assert!(empty.items.is_empty());
        // A part that is not ok is said before a missing one, whatever its
        // level: an item that went quiet, a warning, an error.
        for (level, age, message) in [
            (Level::Ok, 8_000, "voice: not heard from for 8 s"),
            (Level::Warn, 100, "voice: slow"),
            (Level::Error, 100, "voice: slow"),
        ] {
            let health = of(&[(level, "voice", "slow", age)], &["actions"]);
            assert_eq!(health.message, message);
            assert_eq!(health.missing, ["actions"]);
            assert!(health.level.rank() >= Level::Stale.rank());
        }
        // A part that reported once is never missing again, however long
        // ago: it is stale.
        let old = of(&[(Level::Ok, "actions", "", 900_000)], &["actions"]);
        assert!(old.missing.is_empty());
        assert_eq!(old.items[0].level, Level::Stale);
        // A part nobody expected is an item like any other.
        let extra = of(&[(Level::Ok, "ring", "", 100)], &[]);
        assert_eq!((extra.level, extra.items.len()), (Level::Ok, 1));
    }

    #[test]
    fn a_part_that_is_not_ok_and_says_nothing_is_still_named() {
        let health = of(
            &[(Level::Warn, "body", "", 100), (Level::Ok, "eyes", "", 100)],
            &[],
        );
        assert_eq!(health.message, "body: warn, and it does not say why");
        let health = of(&[(Level::Error, "body", "  \n", 100)], &[]);
        assert_eq!(health.message, "body: error, and it does not say why");
        // What a part that is well says stays with the part.
        let health = of(&[(Level::Ok, "body", "warming up", 100)], &[]);
        assert_eq!((health.level, health.message.as_str()), (Level::Ok, ""));
        assert_eq!(health.items[0].message, "warming up");
    }

    #[test]
    fn a_report_is_kept_by_its_name_and_the_newest_replaces_the_one_before() {
        let now = Instant::now();
        let mut parts = Parts::new();
        parts.take(&report(Level::Error, "voice", "down"), now);
        parts.take(&report(Level::Ok, "body", ""), now);
        parts.take(&report(Level::Ok, "voice", ""), now + ms(10));
        assert_eq!(parts.reports().len(), 2);
        assert_eq!(
            parts.reports()["voice"],
            Kept {
                level: Level::Ok,
                message: String::new(),
                at: now + ms(10),
                link: None,
                down_for_s: None,
            }
        );
    }

    #[test]
    fn a_link_down_with_the_internet_answering_is_online_from_reach() {
        let down = Online {
            online: Some(false),
            from: "voice".into(),
            down_for_s: Some(40),
        };
        let reached = with_reach(down.clone(), Some(true));
        assert_eq!(
            reached,
            Online {
                online: Some(true),
                from: "reach".into(),
                down_for_s: None,
            }
        );
        reached.check().unwrap();
        // The internet not answering: offline, from `reach`, as long as the
        // link has been down.
        let no_internet = with_reach(down.clone(), Some(false));
        assert_eq!(
            no_internet,
            Online {
                online: Some(false),
                from: "reach".into(),
                down_for_s: Some(40),
            }
        );
        no_internet.check().unwrap();
        // No verdict yet: nobody knows, so nothing shows offline before the
        // probe has spoken.
        let pending = with_reach(down.clone(), None);
        assert_eq!(pending.online, None);
        pending.check().unwrap();
        // A link up, or nobody knowing: as given.
        let up = Online {
            online: Some(true),
            from: "voice".into(),
            down_for_s: None,
        };
        assert_eq!(with_reach(up.clone(), Some(true)), up);
        let unknown = Online {
            online: None,
            from: String::new(),
            down_for_s: None,
        };
        assert_eq!(with_reach(unknown.clone(), Some(false)), unknown);
    }

    #[test]
    fn online_is_the_voice_link_while_its_report_is_fresh() {
        let now = Instant::now();
        let stale = Duration::from_secs(5);
        let mut parts = Parts::new();
        let with = |link: &str, down: Option<&str>| {
            let mut report = report(Level::Ok, "voice", "");
            report.values.push(lamp_msgs::KeyValue {
                key: "link".into(),
                value: link.into(),
            });
            if let Some(down) = down {
                report.values.push(lamp_msgs::KeyValue {
                    key: "down_for_s".into(),
                    value: down.into(),
                });
            }
            report
        };
        // Nobody reporting a link: unknown.
        parts.take(&report(Level::Ok, "body", ""), now);
        assert_eq!(online(parts.reports(), now, stale).online, None);
        for (link, down, said, for_s) in [
            ("ready", None, Some(true), None),
            ("connecting", None, Some(true), None),
            ("down", Some("12"), Some(false), Some(12)),
            ("down", None, Some(false), None),
            ("something else", None, None, None),
        ] {
            parts.take(&with(link, down), now);
            let found = online(parts.reports(), now, stale);
            assert_eq!((found.online, found.down_for_s), (said, for_s), "{link}");
            assert!(found.check().is_ok());
        }
        // A voice that has stopped reporting says nothing any more.
        parts.take(&with("down", Some("3")), now);
        assert_eq!(online(parts.reports(), now + stale, stale).online, None);
    }

    #[test]
    fn a_name_is_something_short_on_one_line_and_a_message_is_kept_on_one() {
        for name in ["voice", "body", "Voice 2", "mắt", "voice/link", "a"] {
            assert!(is_name(name), "{name:?}");
        }
        assert!(is_name(&"n".repeat(MOST_NAME_BYTES)));
        for name in [
            "",
            " voice",
            "voice ",
            "vo\nice",
            "vo\tice",
            "\u{1b}[31mvoice",
            &"n".repeat(MOST_NAME_BYTES + 1),
        ] {
            assert!(!is_name(name), "{name:?}");
        }
        let now = Instant::now();
        let mut parts = Parts::new();
        assert_eq!(
            parts.take(&report(Level::Ok, "vo\nice", ""), now),
            Taken::NotAName
        );
        assert!(parts.reports().is_empty());
        // Line ends and the like become spaces, and the ends are trimmed.
        parts.take(
            &report(Level::Warn, "body", "  yaw is hot\nand slow\t\n"),
            now,
        );
        assert_eq!(parts.reports()["body"].message, "yaw is hot and slow");
        // A long message is cut on a whole character, and says that it is.
        parts.take(&report(Level::Warn, "body", &"nóng ".repeat(60)), now);
        let cut = &parts.reports()["body"].message;
        assert!(cut.len() <= MOST_MESSAGE_BYTES, "{}", cut.len());
        assert!(cut.len() > MOST_MESSAGE_BYTES - 8, "{}", cut.len());
        assert!(cut.starts_with("nóng nóng ") && cut.ends_with('…'));
        // One of exactly the most is kept whole.
        let most = "x".repeat(MOST_MESSAGE_BYTES);
        parts.take(&report(Level::Warn, "body", &most), now);
        assert_eq!(parts.reports()["body"].message, most);
    }

    #[test]
    fn no_more_than_sixty_four_parts_are_kept_and_those_kept_still_report() {
        let now = Instant::now();
        let mut parts = Parts::new();
        for number in 0..MOST_PARTS {
            let name = format!("part{number}");
            assert_eq!(
                parts.take(&report(Level::Ok, &name, ""), now),
                Taken::Kept,
                "{name}"
            );
        }
        assert_eq!(
            parts.take(&report(Level::Error, "one more", "x"), now),
            Taken::Full
        );
        assert_eq!(parts.reports().len(), MOST_PARTS);
        assert_eq!(
            parts.take(&report(Level::Warn, "part7", "hot"), now),
            Taken::Kept
        );
        let health = health(parts.reports(), now, &[], STALE);
        assert_eq!(health.check(), Ok(()));
        assert_eq!(health.message, "part7: hot");
        // The largest health there can be fits in one message of the bus:
        // every name and message as long as may be, and as many expected.
        let mut parts = Parts::new();
        let long = "\"".repeat(MOST_MESSAGE_BYTES);
        let mut expected = Vec::new();
        for number in 0..MOST_PARTS {
            let name = format!("{number:02}{}", "\\".repeat(MOST_NAME_BYTES - 2));
            parts.take(&report(Level::Error, &name, &long), now);
            expected.push(format!("m{number:02}{}", "\\".repeat(MOST_NAME_BYTES - 3)));
        }
        let largest = super::health(parts.reports(), now, &expected, STALE);
        assert_eq!(largest.missing.len(), MOST_PARTS);
        let bytes = serde_json::to_vec(&largest).unwrap().len();
        assert!(bytes < 50_000, "{bytes}");
    }

    #[test]
    fn two_healths_are_the_same_news_when_only_ages_differ() {
        let same = |a: &Health, b: &Health| same_but_for_ages(a, b, STALE);
        let reports = [
            (Level::Warn, "body", "yaw is hot", 400),
            (Level::Ok, "eyes", "", 7_300),
            (Level::Ok, "voice", "", 300),
        ];
        let first = of(&reports, &["actions"]);
        assert!(same(&first, &first));
        // A second later: every age is another, and so is the count in
        // the quiet part's message.
        let mut later = reports;
        for report in &mut later {
            report.3 += 1_000;
        }
        let second = of(&later, &["actions"]);
        assert_ne!(first, second);
        assert_eq!(second.items[1].message, "not heard from for 8 s");
        assert!(same(&first, &second));
        // A level, a message, a part more, a part missing: each is news.
        let mut level = reports;
        level[2].0 = Level::Warn;
        assert!(!same(&first, &of(&level, &["actions"])));
        let mut message = reports;
        message[0].2 = "yaw is very hot";
        assert!(!same(&first, &of(&message, &["actions"])));
        assert!(!same(&first, &of(&reports[..2], &["actions"])));
        assert!(!same(&first, &of(&reports, &[])));
        assert!(!same(&first, &of(&reports, &["actions", "memory"])));
        // A part going quiet is news, and so is its coming back as it was.
        let mut quiet = reports;
        quiet[2].3 = 5_001;
        assert!(!same(&first, &of(&quiet, &["actions"])));
        let mut back = reports;
        back[1].3 = 10;
        assert!(!same(&first, &of(&back, &["actions"])));
        // A part that calls itself stale has a message of its own, and a
        // change in that is news like any other.
        let own = of(
            &[(Level::Stale, "voice", "waiting for the service", 10)],
            &[],
        );
        let other = of(&[(Level::Stale, "voice", "waiting for a key", 10)], &[]);
        assert!(!same(&own, &other));
        // Once it has gone quiet as well, only its silence is said.
        let gone = of(
            &[(Level::Stale, "voice", "waiting for the service", 6_000)],
            &[],
        );
        let longer = of(&[(Level::Stale, "voice", "waiting for a key", 9_000)], &[]);
        assert!(same(&gone, &longer));
        assert!(!same(&own, &gone));
    }
}
