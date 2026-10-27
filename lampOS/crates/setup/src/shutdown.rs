//! The safe shutdown (#3697, `docs/shutdown.md`): the lamp is turned off
//! the way V1 parked it before the power went (HAL-4.12, HAL-5.18), so that
//! nobody has to pull the plug on a running lamp. In order: the lamp says
//! [`GOING_TO_SLEEP`] through the voice's say service, waited for at most
//! [`SAY_WITHIN`]; the lamp is paused and the body rests, still for [`STILL_FOR`]; every part is
//! stopped ([`crate::reset::stop_and_wait`]); [`LET_GO_AFTER`] later the
//! body lets go, torque off on all five and each read back
//! (`lamp-body-measure let-go`); the ring shows that it is safe to unplug
//! and is left so; the disks are synced; the machine powers off.
//!
//! The order is here, the effects behind [`Host`], so a test never stops,
//! lets go or powers anything off. Every step says one line.

use serde::Serialize;
use std::{
    io::{self, BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::Path,
    time::{Duration, Instant},
};

/// What the lamp says before anything stops (the owner at the lamp: a
/// silent shutdown reads wrong): one text with the voice's speech cache,
/// which renders it ahead (`lamp_msgs::GOING_TO_SLEEP`).
pub use lamp_msgs::GOING_TO_SLEEP;

/// The longest the shutdown waits for [`GOING_TO_SLEEP`] to be played:
/// a second of quiet at priority 9, about a second to the model's first
/// sound, about four of speech. A say service that is not there, refuses,
/// or has not said it by then is said, and the shutdown goes on.
pub const SAY_WITHIN: Duration = Duration::from_secs(10);

/// The product switch for the ways a person asks (the app, a long press,
/// a spoken yes), which later changes add. Off unless `on`. The runner
/// itself, `lamp-setup shutdown-now`, is not behind it: it is only ever
/// started by one of those, or by hand.
pub const SWITCH: &str = "LAMP_SHUTDOWN";

/// Whether [`SWITCH`] is on.
pub fn switched_on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// How the lamp's parts are stopped: as the reset's plan stops them, each
/// part named and each read back stopped ([`crate::reset::stop_and_wait`]).
/// Stopped on its own before the rest (#3697): the voice reads the privacy
/// switch from lamp-controls, which `lampos.target` stops first (controls
/// start after the voice). A voice that loses the switch while it stops
/// counts it a fault, and its restart guard keeps the lamp muted at the
/// next power-on: lamp-52e6, 27 Oct, after each shutdown.
/// The path that starts `lampos.target` once the lamp is set up stops with
/// it: left on, it started the target, and with it the voice, again while
/// the shutdown was stopping it (lamp-52e6, 28 Oct 06:28: the voice back for
/// 6 s between the two stops; the Reviewer-lead on #3713). A dry run starts
/// it again with the rest of what it stopped.
pub const VOICE_FIRST: [&str; 4] = [
    "/usr/bin/systemctl",
    "stop",
    "lampos-setup-done.path",
    "lamp-voice.service",
];

pub const STOP: [&str; 3] = ["/usr/bin/systemctl", "stop", "lampos.target"];

/// How the machine is powered off.
pub const POWER_OFF: [&str; 2] = ["/usr/bin/systemctl", "poweroff"];

/// The rest the body must show before the parts stop (HAL-4.12, "move to
/// rest over 2 s"): every joint still this long.
pub const STILL_FOR: Duration = Duration::from_secs(2);

/// The longest the body is waited for to rest: a standing lamp folds over
/// its stand time (6 s on the product) and then must be still for
/// [`STILL_FOR`]. After it the shutdown goes on and says so: the body's own
/// stop folds it too, and the let-go writes nothing to a lamp found up.
pub const REST_WITHIN: Duration = Duration::from_secs(15);

/// Still: no joint moved more than this between the first read of the rest
/// and the last, in radians (about 6 counts of 4,096 a turn).
pub const STILL_RADIANS: f64 = 0.01;

/// How often the joints are looked at while the body rests.
pub const LOOK_EVERY: Duration = Duration::from_millis(100);

/// From the body's stop to its let-go (HAL-4.12, "wait 0.4 s").
pub const LET_GO_AFTER: Duration = Duration::from_millis(400);

/// What the shutdown does outside its own order. The lamp's is in
/// `lamp-setup`'s `main`; a test's records each call.
pub trait Host {
    /// Say `line` aloud and return once it was played whole, or why not,
    /// within `within`.
    fn say_aloud(&mut self, line: &str, within: Duration) -> Result<(), String>;
    /// Time since some fixed start.
    fn now(&self) -> Duration;
    fn sleep(&mut self, pause: Duration);
    /// Say on `lamp/pause` that the lamp is paused, or no longer.
    fn pause(&mut self, paused: bool) -> io::Result<()>;
    /// Where each joint is now, in radians, from a fresh `body/joint_states`;
    /// `None` while the body says nothing.
    fn joints(&mut self) -> Option<Vec<f64>>;
    /// Stop every part, and return once each reads stopped.
    fn stop(&mut self) -> io::Result<()>;
    /// The lamp's units active now, read before a dry run's stop and after.
    fn active(&mut self) -> Vec<String>;
    /// Start `units` again, after a dry run.
    fn start(&mut self, units: &[String]) -> io::Result<()>;
    /// The body's let-go: its one report line, or why it did not let go.
    fn let_go(&mut self) -> Result<String, String>;
    /// The ring's safe-to-unplug light, left on.
    fn light(&mut self) -> io::Result<()>;
    fn sync(&mut self);
    fn power_off(&mut self) -> io::Result<()>;
}

/// Why the ring is not lit green after a let-go that did not let go.
pub const NOT_LET_GO: &str = "not let go: keep it plugged in";

/// Whether lamp-body-measure's let-go report says every joint was let go:
/// `{"port": …, "let_go": {"down": …, "let_go": true, "joints": […]}}`.
fn let_go_said(report: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(report)
        .is_ok_and(|report| report["let_go"]["let_go"] == true)
}

/// Each step as it went, in order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "step", content = "why")]
pub enum Step {
    Said,
    NotSaid(String),
    Paused,
    NotPaused(String),
    Rested,
    NotRested,
    Stopped,
    NotStopped(String),
    LetGo,
    NotLetGo(String),
    Lit,
    NotLit(String),
    Synced,
    PoweringOff,
    WouldPowerOff,
    PowerOffFailed(String),
}

/// The shutdown, once the update's lock is held. Nothing that fails stops
/// it short of the power: a lamp that asked to be turned off and is left
/// half stopped helps nobody, and the system's own poweroff stops what is
/// left in its order (the reset reboots after a failed stop the same way).
/// Only `dry_run` stops short: everything but the power, said instead.
pub fn run(host: &mut impl Host, dry_run: bool, say: &mut impl FnMut(&str)) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut step = |step: Step, line: String, say: &mut dyn FnMut(&str)| {
        say(&line);
        steps.push(step);
    };
    // Said first, while every part still runs: the voice speaks it, and
    // the green light it names comes last. A dry run says it too, so it
    // can be heard.
    match host.say_aloud(GOING_TO_SLEEP, SAY_WITHIN) {
        Ok(()) => step(Step::Said, format!("said: {GOING_TO_SLEEP}"), say),
        Err(why) => step(
            Step::NotSaid(why.clone()),
            format!("not said ({why}); going on"),
            say,
        ),
    }
    // A product lamp has no keeper (lamp-52e6 on 7071): its body rests
    // as it stops, so no keeper is the plain path, not a fault.
    let paused = match host.pause(true) {
        Ok(()) => {
            step(Step::Paused, "paused: the body goes to rest".into(), say);
            true
        }
        Err(why) if why.kind() == io::ErrorKind::NotFound => {
            step(
                Step::NotPaused("no keeper".into()),
                "no keeper on this lamp: the body rests as it stops".into(),
                say,
            );
            false
        }
        Err(why) => {
            step(
                Step::NotPaused(why.to_string()),
                format!("not paused ({why}): the body rests as it stops"),
                say,
            );
            false
        }
    };
    if rested(host) {
        step(
            Step::Rested,
            format!("the body is at rest, still {} s", STILL_FOR.as_secs()),
            say,
        );
    } else {
        step(
            Step::NotRested,
            format!(
                "the body did not show rest within {} s; stopping all the same, its stop folds it",
                REST_WITHIN.as_secs()
            ),
            say,
        );
    }
    // A dry run leaves the lamp as it found it: what its stop ends that
    // is not part of `lampos.target` (`lamp-update.service`, the reset's
    // writers and starters) is not brought back by restarting the target.
    let before = if dry_run { host.active() } else { Vec::new() };
    let stopped = host.stop();
    match &stopped {
        Ok(()) => step(Step::Stopped, "every part is stopped".into(), say),
        Err(why) => step(
            Step::NotStopped(why.to_string()),
            format!("not every part stopped ({why}); going on, the poweroff stops the rest"),
            say,
        ),
    }
    // Stopped, the parts no longer read it; a kept pause would hold the
    // lamp paused when it is next started without a power-up (a dry run).
    if paused
        && stopped.is_ok()
        && let Err(why) = host.pause(false)
    {
        say(&format!("the pause is left kept ({why})"));
    }
    host.sleep(LET_GO_AFTER);
    let let_go = match host.let_go() {
        // Its report's own word, not only its exit: a lamp found up keeps
        // its torque and says `"let_go": false`.
        Ok(report) if let_go_said(&report) => {
            step(Step::LetGo, format!("let go: {report}"), say);
            true
        }
        Ok(report) => {
            step(
                Step::NotLetGo(report.clone()),
                format!("not let go: {report}"),
                say,
            );
            false
        }
        Err(why) => {
            step(
                Step::NotLetGo(why.clone()),
                format!("not let go: {why}"),
                say,
            );
            false
        }
    };
    // Green says "safe to unplug", after "Unplug me when my light turns
    // green.": only for a lamp let go. One still held up by its torque
    // would fall when the plug is pulled (the Reviewer-lead's and
    // Fourteen's P2 on #3713).
    if !let_go {
        step(
            Step::NotLit(NOT_LET_GO.into()),
            format!("the ring is not lit: {NOT_LET_GO}"),
            say,
        );
    } else {
        match host.light() {
            Ok(()) => step(Step::Lit, "the ring says it is safe to unplug".into(), say),
            Err(why) => step(
                Step::NotLit(why.to_string()),
                format!("the ring is not lit ({why})"),
                say,
            ),
        }
    }
    host.sync();
    step(Step::Synced, "synced".into(), say);
    if dry_run {
        let now = host.active();
        let again: Vec<String> = before
            .into_iter()
            .filter(|unit| !now.contains(unit))
            .collect();
        if !again.is_empty() {
            match host.start(&again) {
                Ok(()) => say(&format!("dry run: started again {}", again.join(" "))),
                Err(why) => say(&format!(
                    "dry run: not started again ({why}): {}",
                    again.join(" ")
                )),
            }
        }
        step(
            Step::WouldPowerOff,
            format!("dry run: would run {}", POWER_OFF.join(" ")),
            say,
        );
        return steps;
    }
    step(Step::PoweringOff, "powering off".into(), say);
    if let Err(why) = host.power_off() {
        step(
            Step::PowerOffFailed(why.to_string()),
            format!("the poweroff failed ({why})"),
            say,
        );
    }
    steps
}

/// The voice's say service on `socket` (`docs/speaking-first.md`, "the
/// say service"): first `line` as `text` with `"cache": true`, played from
/// the voice's speech cache (rendered ahead at its start, so no network is
/// needed), then, if it was not played, `line` as `text` for the model,
/// each from the skill `shutdown` at priority 9, both within `within`
/// together; `Ok` only on a `{"outcome":"spoken"}`, played whole. Neither:
/// why, for each. Muted, paused or busy is the say service's own to judge.
pub fn say_through(socket: &Path, line: &str, within: Duration) -> Result<(), String> {
    let until = Instant::now() + within;
    let cached = ask_once(
        socket,
        serde_json::json!({"text": line, "cache": true}),
        until,
    );
    let Err(why) = cached else {
        return Ok(());
    };
    ask_once(socket, serde_json::json!({"text": line}), until)
        .map_err(|then| format!("the cached line: {why}; the words: {then}"))
}

/// One ask of the say service, `words` with the skill, priority and expiry
/// added, waited for until `until`.
fn ask_once(socket: &Path, mut ask: serde_json::Value, until: Instant) -> Result<(), String> {
    let within = until.saturating_duration_since(Instant::now());
    if within.is_zero() {
        return Err("no time left".into());
    }
    ask["skill"] = "shutdown".into();
    ask["priority"] = 9.into();
    ask["expires_in_ms"] = (within.as_millis() as u64).into();
    let mut stream = UnixStream::connect(socket)
        .map_err(|why| format!("the voice's say service is not there: {why}"))?;
    stream
        .write_all(format!("{ask}\n").as_bytes())
        .map_err(|why| format!("the say service did not take the ask: {why}"))?;
    let mut reader = BufReader::new(stream);
    loop {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err("not played in time".into());
        }
        reader
            .get_ref()
            .set_read_timeout(Some(left))
            .map_err(|why| why.to_string())?;
        let mut answer = String::new();
        match reader.read_line(&mut answer) {
            Ok(0) => return Err("the say service closed without an outcome".into()),
            Ok(_) => {}
            Err(why)
                if matches!(
                    why.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Err("not played in time".into());
            }
            Err(why) => return Err(why.to_string()),
        }
        let answer: serde_json::Value = serde_json::from_str(answer.trim_end()).unwrap_or_default();
        match answer["outcome"].as_str() {
            Some("spoken") => return Ok(()),
            Some(outcome) => {
                return Err(match answer["why"].as_str() {
                    Some(why) => format!("{outcome}: {why}"),
                    None => outcome.to_owned(),
                });
            }
            // `begun`: the outcome follows.
            None => {}
        }
    }
}

/// Whether the body shows rest within [`REST_WITHIN`]: every joint still,
/// within [`STILL_RADIANS`], for [`STILL_FOR`] of fresh reads.
fn rested(host: &mut impl Host) -> bool {
    let until = host.now() + REST_WITHIN;
    let mut since: Option<(Duration, Vec<f64>)> = None;
    loop {
        let now = host.now();
        match (host.joints(), &since) {
            (Some(at), Some((from, first))) if still(first, &at) => {
                if now.saturating_sub(*from) >= STILL_FOR {
                    return true;
                }
            }
            (Some(at), _) => since = Some((now, at)),
            (None, _) => since = None,
        }
        if now >= until {
            return false;
        }
        host.sleep(LOOK_EVERY);
    }
}

fn still(first: &[f64], now: &[f64]) -> bool {
    !first.is_empty()
        && first.len() == now.len()
        && first
            .iter()
            .zip(now)
            .all(|(a, b)| (a - b).abs() <= STILL_RADIANS)
}

/// The update's lock, taken without waiting, held by the caller through
/// the shutdown so no update installs under it: the one rule every helper
/// that restarts or turns off the lamp reads (`lamp_keeper::restart`).
/// Refused while another holder has it, and when it cannot be checked.
pub use lamp_keeper::restart::update_lock;

#[cfg(test)]
mod tests {
    use super::*;

    /// A lamp in a test: its clock, its joints read by read, and every
    /// call in order with when it came.
    /// lamp-body-measure let-go's line as lamp-52e6 printed it, the port
    /// shortened: the let-go nested under the port's report.
    const LET_GO_REPORT: &str = r#"{"port":{"schema_version":2,"unit_id":"lamp-52e6"},"let_go":{"down":true,"let_go":true,"joints":[{"joint":"base_yaw","position":2020,"torque":0}]}}"#;

    #[derive(Default)]
    struct Fake {
        now: Duration,
        /// Each read of the joints in turn; the last one again after.
        joints: Vec<Option<Vec<f64>>>,
        reads: usize,
        stop_fails: bool,
        /// No keeper on this lamp, as on a product image.
        no_keeper: bool,
        /// The say service is not there: answered after the whole wait.
        say_fails: bool,
        /// Each read of the active units in turn; none after.
        active: Vec<Vec<&'static str>>,
        started: Vec<String>,
        /// What the let-go answers; a lamp let go when `None`.
        let_go: Option<Result<String, String>>,
        calls: Vec<(Duration, &'static str)>,
    }

    impl Host for Fake {
        fn say_aloud(&mut self, line: &str, within: Duration) -> Result<(), String> {
            assert_eq!(line, GOING_TO_SLEEP);
            self.calls.push((self.now, "say"));
            if self.say_fails {
                self.now += within;
                return Err("the voice's say service is not there".into());
            }
            self.now += Duration::from_secs(4);
            Ok(())
        }
        fn now(&self) -> Duration {
            self.now
        }
        fn sleep(&mut self, pause: Duration) {
            self.now += pause;
        }
        fn pause(&mut self, paused: bool) -> io::Result<()> {
            if self.no_keeper {
                return Err(io::ErrorKind::NotFound.into());
            }
            self.calls
                .push((self.now, if paused { "pause" } else { "resume" }));
            Ok(())
        }
        fn joints(&mut self) -> Option<Vec<f64>> {
            let read = self.joints.get(self.reads).or(self.joints.last()).cloned();
            self.reads += 1;
            read.flatten()
        }
        fn stop(&mut self) -> io::Result<()> {
            self.calls.push((self.now, "stop"));
            self.now += Duration::from_secs(3);
            if self.stop_fails {
                Err(io::Error::other("lamp-body.service active"))
            } else {
                Ok(())
            }
        }
        fn active(&mut self) -> Vec<String> {
            if self.active.is_empty() {
                return Vec::new();
            }
            self.active
                .remove(0)
                .into_iter()
                .map(str::to_owned)
                .collect()
        }
        fn start(&mut self, units: &[String]) -> io::Result<()> {
            self.calls.push((self.now, "start"));
            self.started = units.to_vec();
            Ok(())
        }
        fn let_go(&mut self) -> Result<String, String> {
            self.calls.push((self.now, "let go"));
            self.let_go
                .clone()
                .unwrap_or_else(|| Ok(LET_GO_REPORT.into()))
        }
        fn light(&mut self) -> io::Result<()> {
            self.calls.push((self.now, "light"));
            Ok(())
        }
        fn sync(&mut self) {
            self.calls.push((self.now, "sync"));
        }
        fn power_off(&mut self) -> io::Result<()> {
            self.calls.push((self.now, "power off"));
            Ok(())
        }
    }

    fn names(fake: &Fake) -> Vec<&'static str> {
        fake.calls.iter().map(|(_, name)| *name).collect()
    }

    fn at(fake: &Fake, name: &str) -> Duration {
        fake.calls
            .iter()
            .find(|(_, called)| *called == name)
            .unwrap()
            .0
    }

    #[test]
    fn it_says_rests_stops_waits_lets_go_lights_syncs_and_powers_off_in_that_order() {
        // Folding for a second, then still.
        let mut joints = vec![Some(vec![0.0, 1.0]); 1];
        for step in 0..10 {
            joints.push(Some(vec![0.1 * f64::from(step), 1.0]));
        }
        joints.push(Some(vec![1.0, 1.0]));
        let mut fake = Fake {
            joints,
            ..Fake::default()
        };
        let mut lines = Vec::new();
        let steps = run(&mut fake, false, &mut |line| lines.push(line.to_owned()));
        assert_eq!(
            names(&fake),
            [
                "say",
                "pause",
                "stop",
                "resume",
                "let go",
                "light",
                "sync",
                "power off"
            ]
        );
        assert_eq!(
            steps,
            [
                Step::Said,
                Step::Paused,
                Step::Rested,
                Step::Stopped,
                Step::LetGo,
                Step::Lit,
                Step::Synced,
                Step::PoweringOff
            ]
        );
        assert_eq!(lines.len(), steps.len(), "one line a step: {lines:?}");
        // Said first, for 4 s; still for 2 s after the fold ended at 1 s
        // later, then stopped.
        assert_eq!(at(&fake, "say"), Duration::ZERO);
        let stop = at(&fake, "stop");
        assert!(
            stop >= Duration::from_secs(7) && stop < Duration::from_secs(4) + REST_WITHIN,
            "{stop:?}"
        );
        // The let-go waits 0.4 s from the end of the stop.
        assert!(at(&fake, "let go") - (stop + Duration::from_secs(3)) >= LET_GO_AFTER);
    }

    /// A lamp found up keeps its torque; one whose let-go failed may be up
    /// too. Neither is told "safe to unplug": the ring is never lit green,
    /// and the power still goes, as every other failed step lets it.
    #[test]
    fn a_lamp_not_let_go_never_turns_the_ring_green() {
        for answer in [
            Ok(LET_GO_REPORT.replace(
                r#""down":true,"let_go":true"#,
                r#""down":false,"let_go":false"#,
            )),
            // A flat report, as no measure prints: not read as let go.
            Ok(r#"{"down":true,"let_go":true}"#.to_owned()),
            Err("lamp-body-measure: the port is busy".to_owned()),
        ] {
            let mut fake = Fake {
                let_go: Some(answer.clone()),
                ..Fake::default()
            };
            let mut lines = Vec::new();
            let steps = run(&mut fake, false, &mut |line| lines.push(line.to_owned()));
            assert!(
                !names(&fake).contains(&"light"),
                "{answer:?}: {:?}",
                names(&fake)
            );
            assert!(matches!(steps[4], Step::NotLetGo(_)), "{steps:?}");
            assert_eq!(steps[5], Step::NotLit(NOT_LET_GO.into()));
            assert!(
                lines.iter().any(|line| line.contains(NOT_LET_GO)),
                "{lines:?}"
            );
            assert_eq!(steps.last(), Some(&Step::PoweringOff));
        }
    }

    /// The set-up path stops with the voice, before the target, so nothing
    /// starts the target again under the shutdown.
    #[test]
    fn the_set_up_path_stops_with_the_voice_before_the_target() {
        assert_eq!(VOICE_FIRST[..2], ["/usr/bin/systemctl", "stop"]);
        assert!(VOICE_FIRST.contains(&"lampos-setup-done.path"));
        assert!(VOICE_FIRST.contains(&"lamp-voice.service"));
        assert_eq!(STOP, ["/usr/bin/systemctl", "stop", "lampos.target"]);
    }

    #[test]
    fn a_body_that_never_rests_is_waited_for_a_bounded_time_then_stopped_and_let_go() {
        for joints in [
            vec![None],
            (0..400).map(|n| Some(vec![f64::from(n)])).collect(),
        ] {
            let mut fake = Fake {
                joints,
                ..Fake::default()
            };
            let steps = run(&mut fake, false, &mut |_| {});
            assert_eq!(steps[2], Step::NotRested);
            let stop = at(&fake, "stop") - Duration::from_secs(4);
            assert!(stop >= REST_WITHIN && stop < REST_WITHIN + Duration::from_secs(1));
            assert_eq!(
                names(&fake),
                [
                    "say",
                    "pause",
                    "stop",
                    "resume",
                    "let go",
                    "light",
                    "sync",
                    "power off"
                ]
            );
        }
    }

    #[test]
    fn a_part_that_will_not_stop_keeps_the_pause_and_still_powers_off() {
        let mut fake = Fake {
            joints: vec![Some(vec![0.0])],
            stop_fails: true,
            ..Fake::default()
        };
        let steps = run(&mut fake, false, &mut |_| {});
        assert!(matches!(steps[3], Step::NotStopped(_)));
        assert_eq!(
            names(&fake),
            [
                "say",
                "pause",
                "stop",
                "let go",
                "light",
                "sync",
                "power off"
            ]
        );
    }

    /// lamp-52e6 on 7071 has no keeper: that is the plain path, said as
    /// such, with no resume asked of a keeper that is not there.
    #[test]
    fn a_lamp_without_a_keeper_rests_as_it_stops_and_powers_off() {
        let mut fake = Fake {
            joints: vec![Some(vec![0.0])],
            no_keeper: true,
            ..Fake::default()
        };
        let mut said = Vec::new();
        let steps = run(&mut fake, false, &mut |line| said.push(line.to_owned()));
        assert_eq!(steps[1], Step::NotPaused("no keeper".into()));
        assert_eq!(
            said[1],
            "no keeper on this lamp: the body rests as it stops"
        );
        assert!(!said.iter().any(|line| line.contains("pause is left kept")));
        assert_eq!(
            names(&fake),
            ["say", "stop", "let go", "light", "sync", "power off"]
        );
    }

    #[test]
    fn a_dry_run_does_everything_but_power_off() {
        let mut fake = Fake {
            joints: vec![Some(vec![0.0])],
            ..Fake::default()
        };
        let steps = run(&mut fake, true, &mut |_| {});
        // Said, so that a dry run can be heard.
        assert_eq!(steps[0], Step::Said);
        assert_eq!(names(&fake)[0], "say");
        assert!(!names(&fake).contains(&"power off"));
        assert_eq!(names(&fake).last(), Some(&"sync"));
        assert_eq!(steps.last(), Some(&Step::WouldPowerOff));
    }

    #[test]
    fn a_dry_run_starts_again_what_it_stopped() {
        let mut fake = Fake {
            joints: vec![Some(vec![0.0])],
            active: vec![
                vec!["lampos.target", "lamp-voice.service", "lamp-update.service"],
                vec![],
            ],
            ..Fake::default()
        };
        run(&mut fake, true, &mut |_| {});
        assert_eq!(
            fake.started,
            ["lampos.target", "lamp-voice.service", "lamp-update.service"]
        );
        assert_eq!(names(&fake).last(), Some(&"start"));
        // A real power-off reads nothing and starts nothing.
        let mut fake = Fake {
            joints: vec![Some(vec![0.0])],
            active: vec![vec!["lamp-update.service"], vec![]],
            ..Fake::default()
        };
        run(&mut fake, false, &mut |_| {});
        assert!(fake.started.is_empty());
        assert_eq!(fake.active.len(), 2);
    }

    #[test]
    fn a_say_that_fails_is_said_and_still_powers_off_within_its_bound() {
        let mut fake = Fake {
            joints: vec![Some(vec![0.0])],
            say_fails: true,
            ..Fake::default()
        };
        let mut said = Vec::new();
        let steps = run(&mut fake, false, &mut |line| said.push(line.to_owned()));
        assert!(matches!(steps[0], Step::NotSaid(_)));
        assert!(said[0].starts_with("not said ("), "{said:?}");
        assert_eq!(at(&fake, "pause"), SAY_WITHIN);
        assert_eq!(names(&fake).last(), Some(&"power off"));
    }

    /// The say service's answers as the voice writes them: the cached line
    /// is asked first and, played, is all (a warm cache with no network);
    /// refused, the words follow; the two together are bounded by `within`,
    /// whatever the voice does.
    #[test]
    fn say_through_asks_the_cached_line_first_then_the_words_within_one_bound() {
        use std::os::unix::net::UnixListener;
        let dir = std::env::temp_dir().join(format!("lss-say-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("say.sock");
        let within = Duration::from_millis(300);
        let gone = say_through(&socket, GOING_TO_SLEEP, within).unwrap_err();
        assert!(gone.starts_with("the cached line: the voice's say service is not there"));
        assert!(gone.contains("; the words: the voice's say service is not there"));
        let listener = UnixListener::bind(&socket).unwrap();
        let spoken = "{\"event\":\"begun\",\"at_us\":1}\n{\"outcome\":\"spoken\"}\n";
        let refused = "{\"outcome\":\"refused\",\"why\":\"the speech cache is off\"}\n";
        let hangs = "{\"event\":\"begun\",\"at_us\":1}\n";
        for (answers, said) in [
            // Played from the cache: no words for the model.
            (vec![spoken], Ok(())),
            // No cache (an older voice, no speech service): the words, for
            // a voice that can still reach its model.
            (vec![refused, spoken], Ok(())),
            (
                vec![refused, refused],
                Err("the cached line: refused: the speech cache is off; \
                     the words: refused: the speech cache is off"
                    .to_owned()),
            ),
            // Begun and never ended: the bound, and no time left for words.
            (
                vec![hangs],
                Err("the cached line: not played in time; the words: no time left".to_owned()),
            ),
        ] {
            let asked = std::thread::scope(|scope| {
                let server = scope.spawn(|| {
                    let mut held = Vec::new();
                    let mut asked = Vec::new();
                    for answer in &answers {
                        let (stream, _) = listener.accept().unwrap();
                        let mut line = String::new();
                        BufReader::new(&stream).read_line(&mut line).unwrap();
                        (&stream).write_all(answer.as_bytes()).unwrap();
                        asked.push(serde_json::from_str::<serde_json::Value>(&line).unwrap());
                        held.push(stream);
                    }
                    std::thread::sleep(within * 2);
                    asked
                });
                let started = Instant::now();
                assert_eq!(say_through(&socket, GOING_TO_SLEEP, within), said);
                assert!(started.elapsed() < within + Duration::from_millis(100));
                server.join().unwrap()
            });
            assert_eq!(asked.len(), answers.len());
            assert_eq!(asked[0]["cache"], true);
            if let Some(words) = asked.get(1) {
                assert!(words.get("cache").is_none());
            }
            for ask in &asked {
                assert_eq!(ask["text"], GOING_TO_SLEEP);
                assert_eq!(ask["skill"], "shutdown");
                assert_eq!(ask["priority"], 9);
                assert!(ask["expires_in_ms"].as_u64().unwrap() <= 300);
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_shutdown_is_refused_while_an_update_holds_its_lock() {
        let folder = std::env::temp_dir().join(format!("lss-{}", std::process::id()));
        let records = lamp_update::records::Records::new(folder.clone());
        let held = records.lock().unwrap();
        assert_eq!(
            update_lock(&records).err().as_deref(),
            Some("an update is being installed: try again once it is done")
        );
        drop(held);
        // A child spawned meanwhile by another test in this process (scan's)
        // holds a copy of every descriptor from fork to exec, the lock's
        // too, for about a millisecond: retried for a short while, not
        // read as held once.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let lock = loop {
            match update_lock(&records) {
                Ok(lock) => break lock,
                Err(_) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(1))
                }
                Err(why) => panic!("{why}"),
            }
        };
        // Held by the shutdown: an update that starts meanwhile can't have it.
        assert!(records.lock().is_err());
        drop(lock);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn the_switch_is_on_only_when_said_on() {
        assert!(switched_on(Some("on")));
        for value in [None, Some(""), Some("off"), Some("1"), Some("ON")] {
            assert!(!switched_on(value));
        }
    }
}
