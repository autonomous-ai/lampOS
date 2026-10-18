//! What an update touches besides the filesystem, each behind a trait so
//! the tests play it: systemd, the bus (what the voice says and the lamp's
//! health), the disk's free room and the clock. The real ones are here.
use crate::{judge::UnitState, quiet::Heard};
use lamp_bus::{Bus, Subscriber};
use lamp_msgs::{Health, Speech, VoiceState};
use std::{
    io,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub trait Systemd {
    /// Each unit's state, as `systemctl show` gives it.
    fn states(&mut self, units: &[String]) -> Result<Vec<UnitState>, String>;
    /// `systemctl daemon-reload`: unit files written are read again.
    fn reload(&mut self) -> Result<(), String>;
    /// `systemctl restart`: returns once systemd has done it.
    fn restart(&mut self, unit: &str) -> Result<(), String>;
    /// The last line the unit's own program wrote to the journal since this
    /// program last restarted it, cleaned ([`clean_line`]); none before a
    /// restart, or when it said nothing since (#1505). Only ever asked of a
    /// unit a round has found not well.
    fn last_line(&mut self, unit: &str) -> Option<String>;
}

pub trait Ears {
    /// What the voice said on the bus in the next `wait`, which this takes.
    fn listen(&mut self, wait: Duration) -> Vec<Heard>;
    /// The lamp's health, if it is published and has not expired.
    fn health(&mut self) -> Option<Health>;
}

pub trait Disk {
    /// The bytes an ordinary program may still write on the disk `path` is on.
    fn free(&self, path: &Path) -> io::Result<u64>;
}

pub trait Clock {
    /// Milliseconds on a clock that only goes forward.
    fn now_ms(&self) -> u64;
}

/// The real systemd.
pub struct Systemctl {
    /// The longest a restart may take: the body folds down before it stops,
    /// up to 25 s.
    pub restart_within: Duration,
    /// When each unit was last restarted here, on the wall clock the
    /// journal keeps: its lines since are its own build's.
    restarted: Vec<(String, SystemTime)>,
}

impl Default for Systemctl {
    fn default() -> Self {
        Self {
            restart_within: Duration::from_secs(120),
            restarted: Vec::new(),
        }
    }
}

impl Systemd for Systemctl {
    fn states(&mut self, units: &[String]) -> Result<Vec<UnitState>, String> {
        let output = Command::new("systemctl")
            .args([
                "show",
                "--property=Id,ActiveState,SubState,MainPID,NRestarts",
                "--",
            ])
            .args(units)
            .stdin(Stdio::null())
            .output()
            .map_err(|error| format!("systemctl: {error}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
        }
        Ok(parse_show(&String::from_utf8_lossy(&output.stdout)))
    }

    fn reload(&mut self) -> Result<(), String> {
        run(&["daemon-reload"], self.restart_within)
    }

    fn restart(&mut self, unit: &str) -> Result<(), String> {
        let at = SystemTime::now();
        self.restarted.retain(|(name, _)| name != unit);
        self.restarted.push((unit.to_owned(), at));
        run(&["restart", unit], self.restart_within)
    }

    fn last_line(&mut self, unit: &str) -> Option<String> {
        let (_, at) = self.restarted.iter().find(|(name, _)| name == unit)?;
        let since = at.duration_since(UNIX_EPOCH).ok()?.as_secs();
        let output = Command::new("journalctl")
            .args(["-u", unit, "--since"])
            .arg(format!("@{since}"))
            .args(["-o", "json", "-n", "50", "--no-pager", "-q"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        last_own_line(&String::from_utf8_lossy(&output.stdout))
    }
}

/// The longest line a round's outcome carries of a part's journal.
pub const LINE_MOST: usize = 200;

/// The last line in `journalctl -o json` output that the unit's own program
/// wrote: systemd's own lines about it ("Failed with result …", "Scheduled
/// restart job …") are skipped, as is a message that is not text.
pub fn last_own_line(json_lines: &str) -> Option<String> {
    json_lines
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|entry| entry["_PID"] != "1" && entry["SYSLOG_IDENTIFIER"] != "systemd")
        .filter_map(|entry| entry["MESSAGE"].as_str().map(clean_line))
        .rfind(|line| !line.is_empty())
}

/// A journal line as an outcome may carry it: control characters gone,
/// trimmed, and at most [`LINE_MOST`] characters, cut with "…".
pub fn clean_line(line: &str) -> String {
    let line: String = line
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let line = line.trim();
    if line.chars().count() <= LINE_MOST {
        return line.to_owned();
    }
    let mut cut: String = line.chars().take(LINE_MOST - 1).collect();
    cut.push('…');
    cut
}

/// `systemctl` with these words, waiting at most `within` for it.
fn run(words: &[&str], within: Duration) -> Result<(), String> {
    let mut child = Command::new("systemctl")
        .args(words)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("systemctl: {error}"))?;
    let until = Instant::now() + within;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                let mut why = String::new();
                if let Some(mut stderr) = child.stderr.take() {
                    let _ = io::Read::read_to_string(&mut stderr, &mut why);
                }
                return Err(format!(
                    "systemctl {}: {status}: {}",
                    words.join(" "),
                    why.trim()
                ));
            }
            Ok(None) if Instant::now() < until => thread::sleep(Duration::from_millis(100)),
            Ok(None) => {
                // The job goes on in systemd; only the waiting ends.
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "systemctl {} did not finish within {} s",
                    words.join(" "),
                    within.as_secs()
                ));
            }
            Err(error) => return Err(format!("systemctl: {error}")),
        }
    }
}

/// The records of `systemctl show`: `Key=value` lines, a blank line between
/// two units.
pub fn parse_show(text: &str) -> Vec<UnitState> {
    let mut units = Vec::new();
    for record in text.split("\n\n") {
        let mut unit = UnitState::default();
        for line in record.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key {
                "Id" => unit.unit = value.to_owned(),
                "ActiveState" => unit.active = value.to_owned(),
                "SubState" => unit.sub = value.to_owned(),
                "MainPID" => unit.pid = value.parse().unwrap_or(0),
                "NRestarts" => unit.restarts = value.parse().unwrap_or(0),
                _ => {}
            }
        }
        if !unit.unit.is_empty() {
            units.push(unit);
        }
    }
    units
}

/// The real bus: what the voice says, and the lamp's health.
pub struct BusEars {
    bus: Option<Bus>,
    state: Option<Subscriber>,
    speech: Vec<Subscriber>,
}

/// The name this program subscribes under.
const NAME: &str = "update";

impl BusEars {
    /// Subscribed to `voice/state`, `voice/heard` and `voice/said`. A bus
    /// that cannot be used is said once, and then nothing is heard: the
    /// voice can then not be judged quiet.
    pub fn new(root: &Path, say: &mut dyn FnMut(&str)) -> Self {
        let bus = match Bus::new(root) {
            Ok(bus) => bus,
            Err(error) => {
                say(&format!(
                    "the bus at {} cannot be used: {error}",
                    root.display()
                ));
                return Self {
                    bus: None,
                    state: None,
                    speech: Vec::new(),
                };
            }
        };
        let mut subscribe =
            |topic: &str, type_name: &str| match bus.subscribe_typed(topic, NAME, type_name) {
                Ok(subscriber) => Some(subscriber),
                Err(error) => {
                    say(&format!("{topic} cannot be subscribed to: {error}"));
                    None
                }
            };
        let state = subscribe(VoiceState::TOPIC, VoiceState::TYPE);
        let speech = Speech::TOPICS
            .iter()
            .filter_map(|topic| subscribe(topic, Speech::TYPE))
            .collect();
        Self {
            bus: Some(bus),
            state,
            speech,
        }
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

impl Ears for BusEars {
    fn listen(&mut self, wait: Duration) -> Vec<Heard> {
        let until = Instant::now() + wait;
        let mut heard = Vec::new();
        loop {
            let left = until.saturating_duration_since(Instant::now());
            match &mut self.state {
                // The state comes once a second: waiting on it wakes often.
                Some(state) => {
                    if let Ok(Some(message)) = state.recv(left.min(Duration::from_millis(250))) {
                        // The kept value of a voice that stopped long ago
                        // says nothing of now.
                        let age = unix_ms().saturating_sub(message.header.at_unix_ms);
                        if age <= crate::quiet::STATE_LASTS.as_millis() as u64
                            && let Ok(said) = message.parse::<VoiceState>()
                        {
                            heard.push(Heard::State(said.state));
                        }
                    }
                }
                None => thread::sleep(left.min(Duration::from_millis(250))),
            }
            for speech in &mut self.speech {
                while let Ok(Some(_)) = speech.recv(Duration::ZERO) {
                    heard.push(Heard::Speech);
                }
            }
            if Instant::now() >= until {
                return heard;
            }
        }
    }

    fn health(&mut self) -> Option<Health> {
        let message = self.bus.as_ref()?.latest(Health::TOPIC).ok()??;
        if message.expired {
            return None;
        }
        let health: Health = message.parse().ok()?;
        health.check().ok()?;
        Some(health)
    }
}

/// No bus at all: hears nothing, so the voice is never judged quiet, and
/// knows no health. What a look that changes nothing uses.
pub struct Deaf;

impl Ears for Deaf {
    fn listen(&mut self, wait: Duration) -> Vec<Heard> {
        thread::sleep(wait);
        Vec::new()
    }

    fn health(&mut self) -> Option<Health> {
        None
    }
}

/// The real disk.
pub struct Statvfs;

impl Disk for Statvfs {
    fn free(&self, path: &Path) -> io::Result<u64> {
        let found = rustix::fs::statvfs(path)?;
        Ok(found.f_bavail.saturating_mul(found.f_frsize))
    }
}

/// The real clock.
pub struct Monotonic(Instant);

impl Default for Monotonic {
    fn default() -> Self {
        Self(Instant::now())
    }
}

impl Clock for Monotonic {
    fn now_ms(&self) -> u64 {
        self.0.elapsed().as_millis() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systemctl_show_is_read_unit_by_unit() {
        let text = "Id=lamp-voice.service\nActiveState=active\nSubState=running\nMainPID=812\nNRestarts=2\n\nId=lamp-body.service\nActiveState=activating\nSubState=auto-restart\nMainPID=0\nNRestarts=1\n";
        let units = parse_show(text);
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].unit, "lamp-voice.service");
        assert_eq!((units[0].pid, units[0].restarts), (812, 2));
        assert!(units[0].running());
        assert_eq!(units[1].sub, "auto-restart");
        assert!(!units[1].running());
    }

    #[test]
    fn the_real_disk_has_room() {
        assert!(Statvfs.free(Path::new("/")).unwrap() > 0);
    }

    #[test]
    fn the_bus_hears_the_voice_and_not_an_old_state() {
        // Short: a socket's path holds 103 bytes, and the Mac's own temporary
        // folder is long.
        let root = tempfile::Builder::new()
            .prefix("lu-")
            .tempdir_in("/tmp")
            .unwrap();
        let bus = Bus::new(root.path()).unwrap();
        // A state kept long ago: a voice that has stopped.
        let mut state = bus
            .publisher_declared(VoiceState::TOPIC, &VoiceState::declaration(), "voice")
            .unwrap();
        let mut said = |_: &str| {};
        let mut ears = BusEars::new(root.path(), &mut said);
        let mut heard_pub = bus
            .publisher_declared("voice/heard", &Speech::declaration(), "voice")
            .unwrap();
        // Subscribed now; a publisher finds a new subscriber within 200 ms.
        thread::sleep(Duration::from_millis(300));
        heard_pub
            .publish_json(
                2,
                &Speech {
                    text: "hello".into(),
                    finished: true,
                    locale: None,
                    request: None,
                },
            )
            .unwrap();
        // The state last, just before listening: one older than
        // `quiet::STATE_LASTS` (3 s) is not heard, so a stalled test thread
        // between its publish and the listening must not age it.
        state
            .publish_json(
                1,
                &VoiceState {
                    state: lamp_msgs::Voice::Speaking,
                },
            )
            .unwrap();
        let heard = ears.listen(Duration::from_millis(500));
        assert!(
            heard.contains(&Heard::State(lamp_msgs::Voice::Speaking)),
            "{heard:?}"
        );
        assert!(heard.contains(&Heard::Speech), "{heard:?}");
        assert_eq!(ears.health(), None);
    }
}
