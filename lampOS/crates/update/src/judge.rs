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
}

impl UnitState {
    pub fn running(&self) -> bool {
        self.active == "active" || self.active == "reloading"
    }
}

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
}

impl Watch {
    pub fn new(unit: &str, part: &str, before: Before) -> Self {
        Self {
            unit: unit.to_owned(),
            part: part.to_owned(),
            before,
            first: None,
        }
    }

    /// One look during the minute. An error at once is a part that is not
    /// well, whatever the rest of the minute would show.
    pub fn look(&mut self, state: &UnitState) -> Result<(), String> {
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
}
