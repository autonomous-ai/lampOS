//! Whether a part is well after its switch: what systemd says of its unit
//! through the minute after, and what the part says of itself in the
//! lamp's health at the end of it.
//!
//! A part is well when, for the whole minute, its unit stayed running with
//! the process it was started with, and at the end it runs; and when, if
//! it was in `lamp/health` before the switch, it is there again, not stale,
//! and not in error unless it was in error before. A part that never
//! reported is judged by systemd alone. Pure: it is told the states.
use lamp_msgs::{Health, Level};

/// One unit as `systemctl show` describes it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnitState {
    pub unit: String,
    /// `ActiveState`: active, activating, deactivating, inactive, failed.
    pub active: String,
    /// `SubState`: running, auto-restart, exited, dead...
    pub sub: String,
    /// `MainPID`, 0 when there is none.
    pub pid: u32,
    /// `NRestarts`: how often systemd has started it again by itself.
    pub restarts: u32,
    /// `Result`: how its last run ended: success, exit-code, signal, ...
    pub result: String,
    /// `ConditionResult`: whether its `Condition…=` held at its last start
    /// (`yes`, `no`), empty when not said.
    pub condition: String,
}

impl UnitState {
    pub fn running(&self) -> bool {
        self.active == "active" || self.active == "reloading"
    }

    /// Ended by itself with nothing wrong: inactive, its run a success.
    pub fn ended_cleanly(&self) -> bool {
        self.active == "inactive" && self.result == "success"
    }

    /// Not started at all, because its own `Condition…=` does not hold on
    /// this lamp: as at boot, and as the build before would be.
    pub fn skipped_by_condition(&self) -> bool {
        self.ended_cleanly() && self.condition == "no"
    }
}

/// Units made to end by themselves once their job is done. The waking face
/// (`lamp-waking.service`, `Type=simple`, no `RemainAfterExit`) ends as soon
/// as the voice takes the ring, so on a lamp that is up it ends inside a
/// round's watch. Such a unit that ended cleanly is well, not stopped: on
/// lamp-9adf on 1 Nov builds 9276 and 9424 were put back for it ("the
/// waking face has ended: Released"), and the put-back's own restart ended
/// the same way, "the previous build is not well either". One that failed,
/// crashed or is started again by systemd is still not well.
pub const ENDS_BY_DESIGN: &[&str] = &["lamp-waking.service"];

// A unit whose restart its own condition skipped ([`UnitState::skipped_by_condition`])
// is not this lamp's now, not stopped: lamp-52e6 on 6 Nov put back 10441
// for "lamp-tasks.service stopped" (its `ConditionPathExists=` Harness gone
// under it), and "the previous build is not well either" for the same
// reason, so every later build would be put back too (Fifth's P2 on #5385).
// Putting a build back can't make a condition hold that neither build owns.
// Not setup's own unit: a completed setup the round restarted anyway, then
// skipped by its marker, is a phone grace period cut short, which
// `setup_lifecycle` exists to prevent; its watch rules stand as they are.
const CONDITION_OWN_RULES: &[&str] = &["lamp-setup.service"];

/// A part's name in the lamp's health: its binary's without `lamp-`.
pub fn part_of(binary: &str) -> &str {
    binary.strip_prefix("lamp-").unwrap_or(binary)
}

/// What the part said of itself in the lamp's health just before the
/// switch: its level, or none if it was not there. `fresh` is whether
/// there was a health to read at all.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Before {
    pub fresh: bool,
    pub level: Option<Level>,
}

impl Before {
    pub fn of(part: &str, health: Option<&Health>) -> Self {
        Self {
            fresh: health.is_some(),
            level: health.and_then(|health| item(health, part)),
        }
    }
}

fn item(health: &Health, part: &str) -> Option<Level> {
    health
        .items
        .iter()
        .find(|item| item.name == part)
        .map(|item| item.level)
}

/// One part watched from its restart.
#[derive(Clone, Debug)]
pub struct Watch {
    pub unit: String,
    pub part: String,
    pub before: Before,
    /// The unit as it was first seen after the restart.
    first: Option<UnitState>,
    /// A unit made to end ([`ENDS_BY_DESIGN`]) that has ended cleanly.
    ended: bool,
}

impl Watch {
    pub fn new(unit: &str, part: &str, before: Before) -> Self {
        Self {
            unit: unit.to_owned(),
            part: part.to_owned(),
            before,
            first: None,
            ended: false,
        }
    }

    /// One look during the minute. An error at once is a part that is not
    /// well, whatever the rest of the minute would show.
    pub fn look(&mut self, state: &UnitState) -> Result<(), String> {
        if state.skipped_by_condition() && !CONDITION_OWN_RULES.contains(&self.unit.as_str()) {
            self.ended = true;
            self.first = None;
            return Ok(());
        }
        if ENDS_BY_DESIGN.contains(&self.unit.as_str()) && state.ended_cleanly() {
            // Its job done: a later start of it is seen afresh.
            self.ended = true;
            self.first = None;
            return Ok(());
        }
        if state.active == "failed" {
            return Err(format!("{} failed ({})", self.unit, state.sub));
        }
        if state.active == "inactive" {
            return Err(format!("{} stopped", self.unit));
        }
        if state.sub == "auto-restart" {
            return Err(format!(
                "{} ended and systemd is starting it again",
                self.unit
            ));
        }
        let first = self.first.get_or_insert_with(|| state.clone());
        if state.restarts > first.restarts {
            return Err(format!(
                "systemd started {} again {} times",
                self.unit,
                state.restarts - first.restarts
            ));
        }
        if first.pid == 0 && state.pid != 0 {
            // Seen first while it was still starting: its process is this one.
            first.pid = state.pid;
        } else if state.pid != 0 && state.pid != first.pid {
            return Err(format!(
                "{}'s process changed from {} to {}: it was started again",
                self.unit, first.pid, state.pid
            ));
        }
        Ok(())
    }

    /// The end of the minute: the last look, and the part's own word.
    pub fn finish(&mut self, state: &UnitState, health: Option<&Health>) -> Result<(), String> {
        self.look(state)?;
        if self.ended && state.ended_cleanly() {
            return Ok(());
        }
        if !state.running() {
            return Err(format!(
                "{} is not running at the end of the watch ({} {})",
                self.unit, state.active, state.sub
            ));
        }
        if self.part == "health" && self.before.fresh && health.is_none() {
            return Err("the lamp's health is not published since lamp-health was switched".into());
        }
        let (Some(health), Some(before)) = (health, self.before.level) else {
            return Ok(());
        };
        match health.items.iter().find(|item| item.name == self.part) {
            None => Err(format!("{} reported before and has not since", self.part)),
            Some(item) if item.level == Level::Stale => Err(format!(
                "{} reported before and has not since: {}",
                self.part, item.message
            )),
            Some(item) if item.level == Level::Error && before != Level::Error => Err(format!(
                "{} says it is in error: {}",
                self.part, item.message
            )),
            Some(_) => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::HealthItem;

    fn running(pid: u32, restarts: u32) -> UnitState {
        UnitState {
            unit: "lamp-voice.service".into(),
            active: "active".into(),
            sub: "running".into(),
            pid,
            restarts,
            result: "success".into(),
            condition: "yes".into(),
        }
    }

    fn health(items: &[(&str, Level)]) -> Health {
        Health {
            level: Level::Ok,
            message: String::new(),
            items: items
                .iter()
                .map(|(name, level)| HealthItem {
                    name: (*name).into(),
                    level: *level,
                    message: "said".into(),
                    age_ms: 100,
                })
                .collect(),
            missing: Vec::new(),
            quiet_ms: 0,
        }
    }

    #[test]
    fn a_part_that_stays_up_with_one_process_and_says_it_is_well_is_well() {
        let mut watch = Watch::new(
            "lamp-voice.service",
            "voice",
            Before::of("voice", Some(&health(&[("voice", Level::Ok)]))),
        );
        for _ in 0..60 {
            watch.look(&running(41, 0)).unwrap();
        }
        let now = health(&[("voice", Level::Warn)]);
        assert_eq!(watch.finish(&running(41, 0), Some(&now)), Ok(()));
    }

    #[test]
    fn restarted_failed_or_another_process_is_not_well() {
        let mut watch = Watch::new("lamp-voice.service", "voice", Before::default());
        watch.look(&running(41, 0)).unwrap();
        assert!(watch.look(&running(41, 1)).is_err());

        let mut watch = Watch::new("lamp-voice.service", "voice", Before::default());
        watch.look(&running(41, 0)).unwrap();
        assert!(watch.look(&running(42, 0)).is_err());

        let mut watch = Watch::new("lamp-voice.service", "voice", Before::default());
        let mut again = running(0, 0);
        again.active = "activating".into();
        again.sub = "auto-restart".into();
        assert!(watch.look(&again).is_err());

        let mut watch = Watch::new("lamp-voice.service", "voice", Before::default());
        let mut failed = running(0, 0);
        failed.active = "failed".into();
        assert!(watch.look(&failed).is_err());

        // Still starting when first seen: its process is the one it gets.
        let mut watch = Watch::new("lamp-voice.service", "voice", Before::default());
        let mut starting = running(0, 0);
        starting.active = "activating".into();
        starting.sub = "start".into();
        watch.look(&starting).unwrap();
        watch.look(&running(43, 0)).unwrap();
        assert!(watch.finish(&running(43, 0), None).is_ok());
        assert!(watch.finish(&starting, None).is_err());
    }

    #[test]
    fn the_part_s_own_word_counts_against_what_it_said_before() {
        let fresh = |level| Before::of("voice", Some(&health(&[("voice", level)])));
        let now = |level| health(&[("voice", level)]);

        let mut watch = Watch::new("lamp-voice.service", "voice", fresh(Level::Ok));
        let why = watch
            .finish(&running(41, 0), Some(&now(Level::Error)))
            .unwrap_err();
        assert!(why.contains("in error"), "{why}");

        let mut watch = Watch::new("lamp-voice.service", "voice", fresh(Level::Ok));
        assert!(
            watch
                .finish(&running(41, 0), Some(&now(Level::Stale)))
                .is_err()
        );

        let mut watch = Watch::new("lamp-voice.service", "voice", fresh(Level::Ok));
        assert!(watch.finish(&running(41, 0), Some(&health(&[]))).is_err());

        // In error before and after: not this build's doing.
        let mut watch = Watch::new("lamp-voice.service", "voice", fresh(Level::Error));
        assert!(
            watch
                .finish(&running(41, 0), Some(&now(Level::Error)))
                .is_ok()
        );

        // Never reported: systemd alone judges it.
        let mut watch = Watch::new("lamp-voice.service", "voice", Before::of("voice", None));
        assert!(watch.finish(&running(41, 0), Some(&health(&[]))).is_ok());

        // lamp-health itself must bring the lamp's health back.
        let mut watch = Watch::new(
            "lamp-health.service",
            "health",
            Before::of("health", Some(&health(&[]))),
        );
        assert!(watch.finish(&running(41, 0), None).is_err());
    }

    fn waking(active: &str, sub: &str, pid: u32, result: &str) -> UnitState {
        UnitState {
            unit: "lamp-waking.service".into(),
            active: active.into(),
            sub: sub.into(),
            pid,
            restarts: 0,
            result: result.into(),
            condition: "yes".into(),
        }
    }

    #[test]
    fn the_waking_face_ending_cleanly_inside_the_watch_is_well() {
        // lamp-9adf, 1 Nov: restarted by the round, then "the waking face has
        // ended: Released" once the voice took the ring, inside the minute.
        let mut watch = Watch::new("lamp-waking.service", "setup", Before::default());
        watch
            .look(&waking("active", "running", 812, "success"))
            .unwrap();
        watch
            .look(&waking("deactivating", "stop-sigterm", 812, "success"))
            .unwrap();
        for _ in 0..30 {
            watch
                .look(&waking("inactive", "dead", 0, "success"))
                .unwrap();
        }
        assert_eq!(
            watch.finish(&waking("inactive", "dead", 0, "success"), None),
            Ok(())
        );
    }

    #[test]
    fn the_waking_face_that_failed_or_is_restarted_is_still_not_well() {
        let mut watch = Watch::new("lamp-waking.service", "setup", Before::default());
        watch
            .look(&waking("active", "running", 812, "success"))
            .unwrap();
        assert!(
            watch
                .look(&waking("inactive", "dead", 0, "exit-code"))
                .is_err()
        );
        let mut watch = Watch::new("lamp-waking.service", "setup", Before::default());
        assert!(
            watch
                .look(&waking("failed", "failed", 0, "exit-code"))
                .is_err()
        );
        let mut watch = Watch::new("lamp-waking.service", "setup", Before::default());
        assert!(
            watch
                .look(&waking("activating", "auto-restart", 0, "exit-code"))
                .is_err()
        );
    }

    #[test]
    fn only_a_unit_made_to_end_may_end_cleanly() {
        let mut watch = Watch::new("lamp-voice.service", "voice", Before::default());
        watch.look(&running(41, 0)).unwrap();
        let ended = UnitState {
            active: "inactive".into(),
            sub: "dead".into(),
            result: "success".into(),
            ..running(0, 0)
        };
        assert!(watch.look(&ended).is_err());
        assert!(watch.finish(&ended, None).is_err());
    }

    /// lamp-52e6, 6 Nov (Fifth's P2 on #5385): lamp-tasks restarted with
    /// its `ConditionPathExists=` Harness gone under it was skipped by
    /// systemd, and counted as stopped, so 10441 was put back, and every
    /// later build would be. Skipped by its own condition is not this
    /// lamp's now, as at boot; ended any other way is still not well.
    #[test]
    fn a_restart_its_own_condition_skipped_is_not_a_stop() {
        let skipped = UnitState {
            unit: "lamp-tasks.service".into(),
            active: "inactive".into(),
            sub: "dead".into(),
            pid: 0,
            restarts: 0,
            result: "success".into(),
            condition: "no".into(),
        };
        let mut watch = Watch::new("lamp-tasks.service", "tasks", Before::default());
        assert!(watch.look(&skipped).is_ok());
        assert!(watch.finish(&skipped, None).is_ok());
        // A part that reported before and is skipped now is still judged
        // by systemd alone: it never started, so it can say nothing.
        let mut watch = Watch::new(
            "lamp-tasks.service",
            "tasks",
            Before::of("tasks", Some(&health(&[("tasks", Level::Ok)]))),
        );
        assert!(watch.finish(&skipped, Some(&health(&[]))).is_ok());
        // Stopped with its condition met, or failed, is not well.
        for (condition, result, active) in [
            ("yes", "success", "inactive"),
            ("", "success", "inactive"),
            ("no", "exit-code", "inactive"),
            ("no", "exit-code", "failed"),
        ] {
            let ended = UnitState {
                condition: condition.into(),
                result: result.into(),
                active: active.into(),
                ..skipped.clone()
            };
            let mut watch = Watch::new("lamp-tasks.service", "tasks", Before::default());
            assert!(
                watch.finish(&ended, None).is_err(),
                "{condition} {result} {active}"
            );
        }
        // Setup's own unit keeps setup_lifecycle's rules: skipped is stopped.
        let setup = UnitState {
            unit: "lamp-setup.service".into(),
            ..skipped.clone()
        };
        let mut watch = Watch::new("lamp-setup.service", "setup", Before::default());
        assert!(watch.finish(&setup, None).is_err());
    }
}
