//! The lamp's self-test (`lamp-selftest`, `docs/selftest.md`): whether each
//! part works, checked at the factory, at the first boot after setup and when
//! asked, with one plain reason for each part.
use crate::{Invalid, KeyValue};
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};

/// What the check found for one part.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// The part did what it was asked.
    Pass,
    /// This lamp has no such part, and is not meant to (a lamp without an
    /// air sensor, say). Not a fault.
    NotFitted,
    /// The part could not be checked this time: its owner was not running,
    /// or the check needs a person who was not there. Not a pass.
    NotChecked,
    /// The part is there and did not do what it was asked.
    Fail,
}

impl Verdict {
    /// The verdict as a person reads it.
    pub fn words(self) -> &'static str {
        match self {
            Verdict::Pass => "pass",
            Verdict::NotFitted => "not fitted",
            Verdict::NotChecked => "not checked",
            Verdict::Fail => "fail",
        }
    }
}

/// When the self-test ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Occasion {
    /// On the bench, with a person who can touch the pads.
    Factory,
    /// The first boot after setup in a home.
    FirstBoot,
    /// Asked for, by a person or by a part.
    OnDemand,
}

/// One part's result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartResult {
    /// The part, as `diagnostics` names it where it has a name there:
    /// `voice_mic`, `room_mic`, `speaker`, `servo_1` .. `servo_5`, `ring`,
    /// `camera`, `touch`, `button`, `air`, `board`.
    pub part: String,
    pub verdict: Verdict,
    /// Why, for a person, in one line. Empty only for a pass with nothing
    /// to add.
    pub reason: String,
    /// A few readings that explain the verdict.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<KeyValue>,
    /// How long the check took, in seconds.
    pub took: f64,
}

impl PartResult {
    pub fn new(part: &str, verdict: Verdict, reason: &str) -> Self {
        Self {
            part: part.to_owned(),
            verdict,
            reason: reason.to_owned(),
            values: Vec::new(),
            took: 0.0,
        }
    }

    /// The same with one more reading beside it.
    #[must_use]
    pub fn with(mut self, key: &str, value: impl ToString) -> Self {
        self.values.push(KeyValue {
            key: key.to_owned(),
            value: value.to_string(),
        });
        self
    }

    /// The one line a person reads: `ring  fail  red did not light`.
    pub fn line(&self) -> String {
        let verdict = self.verdict.words();
        if self.reason.is_empty() {
            format!("{:<10} {verdict}", self.part)
        } else {
            format!("{:<10} {verdict:<11} {}", self.part, self.reason)
        }
    }
}

/// One whole run (`lamp/selftest`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SelfTest {
    pub occasion: Occasion,
    /// When it started, in seconds since the Unix epoch.
    pub started: f64,
    /// How long the whole run took, in seconds.
    pub took: f64,
    /// The worst verdict among the parts: `fail` if any part failed, then
    /// `not_checked`, else `pass`. A part not fitted makes no run worse.
    pub verdict: Verdict,
    /// Every part checked, in the order they ran.
    pub parts: Vec<PartResult>,
    /// The self-test build that ran it (its program's folder under
    /// `/opt/lampOS/selftest/`), so `lamp-health` can tell a fail written by
    /// an older rule from one this build would still give (#1399). Absent
    /// from runs before it, and from a program run outside an install.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
}

impl SelfTest {
    pub const TYPE: &str = "lamp_msgs/SelfTest";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "lamp/selftest";

    /// The newest run replaces the one before. A run is rare and its result
    /// holds until the next, so the topic promises no period.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
    }

    /// The verdict a run of these parts has.
    pub fn verdict_of(parts: &[PartResult]) -> Verdict {
        parts
            .iter()
            .map(|p| p.verdict)
            .filter(|v| *v != Verdict::NotFitted)
            .max()
            .unwrap_or(Verdict::Pass)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        if self.parts.is_empty() {
            return Err(Invalid("no part was checked".into()));
        }
        for (name, value) in [("started", self.started), ("took", self.took)] {
            if !(value.is_finite() && value >= 0.0) {
                return Err(Invalid(format!("{name} is {value}: 0 or more")));
            }
        }
        for p in &self.parts {
            if p.part.is_empty() {
                return Err(Invalid("a part has no name".into()));
            }
            if !(p.took.is_finite() && p.took >= 0.0) {
                return Err(Invalid(format!("{} took {}: 0 or more", p.part, p.took)));
            }
            if p.verdict != Verdict::Pass && p.reason.is_empty() {
                return Err(Invalid(format!("{} gives no reason", p.part)));
            }
        }
        if self.verdict != Self::verdict_of(&self.parts) {
            return Err(Invalid("verdict is not the worst of the parts".into()));
        }
        if let Some(build) = &self.build
            && (build.is_empty() || build.len() > 128 || build.chars().any(char::is_control))
        {
            return Err(Invalid(
                "build is not a plain name of 1 to 128 bytes".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(parts: Vec<PartResult>) -> SelfTest {
        SelfTest {
            occasion: Occasion::Factory,
            started: 1_760_520_000.0,
            took: 95.5,
            verdict: SelfTest::verdict_of(&parts),
            parts,
            build: None,
        }
    }

    #[test]
    fn a_run_is_its_parts_and_the_worst_of_them() {
        let test = run(vec![
            PartResult::new("camera", Verdict::Pass, "").with("frame", "640x480"),
            PartResult::new("air", Verdict::NotFitted, "no SEN63C on the I2C bus"),
            PartResult::new("touch", Verdict::NotChecked, "no person to swipe"),
        ]);
        assert_eq!(test.verdict, Verdict::NotChecked);
        assert_eq!(test.check(), Ok(()));
        let text = serde_json::to_string(&test).unwrap();
        assert!(text.contains("\"verdict\":\"not_fitted\""), "{text}");
        assert!(text.contains("\"occasion\":\"factory\""), "{text}");
        assert_eq!(serde_json::from_str::<SelfTest>(&text).unwrap(), test);
        assert_eq!(SelfTest::declaration().kind, Kind::Latest);
    }

    #[test]
    fn a_fault_outweighs_everything_and_not_fitted_nothing() {
        let fail = PartResult::new("ring", Verdict::Fail, "red did not light");
        let absent = PartResult::new("air", Verdict::NotFitted, "no sensor");
        assert_eq!(
            SelfTest::verdict_of(std::slice::from_ref(&absent)),
            Verdict::Pass
        );
        assert_eq!(SelfTest::verdict_of(&[absent, fail]), Verdict::Fail);
        assert_eq!(SelfTest::verdict_of(&[]), Verdict::Pass);
    }

    #[test]
    fn a_run_that_says_less_than_it_must_is_refused() {
        let mut test = run(vec![PartResult::new(
            "ring",
            Verdict::Fail,
            "red did not light",
        )]);
        test.verdict = Verdict::Pass;
        assert!(test.check().unwrap_err().0.contains("worst"));
        let silent = run(vec![PartResult::new("ring", Verdict::Fail, "")]);
        assert!(silent.check().unwrap_err().0.contains("no reason"));
        assert!(run(Vec::new()).check().is_err());
    }

    #[test]
    fn one_line_per_part() {
        assert_eq!(
            PartResult::new("ring", Verdict::Fail, "red did not light").line(),
            "ring       fail        red did not light"
        );
        assert_eq!(
            PartResult::new("camera", Verdict::Pass, "").line(),
            "camera     pass"
        );
    }
}
