//! One run's record and its verdict (#1700, deliverable 4;
//! docs/production-testing.md, "The verdict").
//!
//! The rules, in order:
//! - an input that does not fit the matrix is refused and nothing is
//!   recorded: an unknown check id, a self-test part given as a station step
//!   or the other way round, a check given twice, a qualification check in a
//!   routine run or a retest, a time outside the run;
//! - any fail makes the run `fail`: a check that failed, a measurement
//!   outside its limit, or `not_fitted` on a revision that must have the part;
//! - otherwise a mandatory check that is missing, skipped, interrupted or
//!   `not_checked` makes it `incomplete`, never `pass`;
//! - otherwise it is `pass`.
use crate::{
    CLOCK_SLACK_MS, Refused, Utc, commit,
    inputs::{Answered, Answers, Measurement, Reading, Repair, Step},
    line,
    matrix::{Check, Kind, Limit, Matrix, Method},
    plain, refuse,
};
use lamp_msgs::{Occasion as SelfTestOccasion, PartResult, SelfTest, Verdict};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The record's own format. A change to what a record holds is a new one.
pub const FORMAT: u32 = 1;

/// Why the run was made.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Occasion {
    /// The routine production run, on every lamp.
    Routine,
    /// The routine run again, on a lamp whose run failed or was incomplete,
    /// after its repairs.
    Retest,
    /// A qualification run, on a few units only.
    Qualification,
}

impl Occasion {
    /// Whether a check of `kind` may be in a run of this occasion: never a
    /// qualification check in a routine run or a retest.
    pub fn admits(self, kind: Kind) -> bool {
        self == Occasion::Qualification || kind != Kind::Qualification
    }

    /// Whether this occasion's runs must hold the mandatory checks of
    /// `kind`: routine and integrated ones for a routine run or a retest,
    /// qualification ones for a qualification run.
    pub fn expects(self, kind: Kind) -> bool {
        match self {
            Occasion::Routine | Occasion::Retest => kind != Kind::Qualification,
            Occasion::Qualification => kind == Kind::Qualification,
        }
    }
}

/// One check's verdict in the record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Pass,
    Fail,
    NotFitted,
    NotChecked,
    Skipped,
    Interrupted,
    /// Neither the self-test nor the station gave it.
    Missing,
}

impl From<Verdict> for Outcome {
    fn from(verdict: Verdict) -> Self {
        match verdict {
            Verdict::Pass => Outcome::Pass,
            Verdict::Fail => Outcome::Fail,
            Verdict::NotFitted => Outcome::NotFitted,
            Verdict::NotChecked => Outcome::NotChecked,
        }
    }
}

impl From<Answered> for Outcome {
    fn from(answered: Answered) -> Self {
        match answered {
            Answered::Pass => Outcome::Pass,
            Answered::Fail => Outcome::Fail,
            Answered::NotFitted => Outcome::NotFitted,
            Answered::NotChecked => Outcome::NotChecked,
            Answered::Skipped => Outcome::Skipped,
            Answered::Interrupted => Outcome::Interrupted,
        }
    }
}

/// The run's verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunVerdict {
    Pass,
    Fail,
    Incomplete,
}

/// The lamp under test.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Device {
    pub id: String,
    pub hardware_revision: String,
    pub batch: String,
}

/// The suite that judged the run: the matrix's version, its hash, whether
/// the hardware lead approved it, and the station tool's commit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Suite {
    pub matrix_version: String,
    pub matrix_sha256: String,
    pub matrix_approved: bool,
    pub tool_commit: String,
}

/// The lamp's self-test report the run used.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelfTestUsed {
    pub occasion: SelfTestOccasion,
    /// By the lamp's clock.
    pub started: Utc,
    pub took: f64,
    pub verdict: Verdict,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
}

/// One check in the record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckResult {
    pub id: String,
    pub part: String,
    pub kind: Kind,
    pub method: Method,
    pub mandatory: bool,
    /// The verdict as judged: a pass outside its limit, or `not_fitted`
    /// where the revision must have the part, is `fail` here.
    pub verdict: Outcome,
    /// What the self-test or the station said, when the judged verdict
    /// differs from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub said: Option<Outcome>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    /// The self-test's readings for its part.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<Reading>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measurement: Option<Measurement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<Limit>,
    /// When a station or manual step was done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Utc>,
}

/// A check that failed, with what was measured against its limit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Failure {
    pub check: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measured: Option<Measurement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<Limit>,
}

/// A mandatory check that did not finish.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unfinished {
    pub check: String,
    pub verdict: Outcome,
    pub reason: String,
}

/// One run, passes included.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub format: u32,
    /// `<started>-<station>-<device>`, as `20261019T080000Z-station-01-lamp-4ace`.
    pub run_id: String,
    pub occasion: Occasion,
    pub verdict: RunVerdict,
    pub started: Utc,
    pub ended: Utc,
    pub device: Device,
    /// The lampOS commit the lamp runs.
    pub lampos_build: String,
    pub suite: Suite,
    pub station: String,
    /// Opaque, local to the station: `op-` and digits, never a name.
    pub operator: String,
    /// For a retest, the run it retests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retest_of: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repairs: Vec<Repair>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selftest: Option<SelfTestUsed>,
    pub checks: Vec<CheckResult>,
    pub failures: Vec<Failure>,
    pub incomplete: Vec<Unfinished>,
}

impl Record {
    pub fn parse(text: &str) -> Result<Self, Refused> {
        let record: Self =
            serde_json::from_str(text).map_err(|error| Refused(format!("the record: {error}")))?;
        if record.format != FORMAT {
            refuse!("the record is format {}, not {FORMAT}", record.format);
        }
        Ok(record)
    }
}

/// Everything one run's record is made from.
pub struct Given<'a> {
    pub matrix: &'a Matrix,
    /// The lamp's self-test report, if the station has one.
    pub selftest: Option<&'a SelfTest>,
    pub answers: &'a Answers,
    pub device: Device,
    pub lampos_build: String,
    /// `<matrix version>+<tool commit>`, as `0+dab3f1242486`.
    pub suite: String,
    pub station: String,
    pub operator: String,
    pub occasion: Occasion,
    /// For a retest, the record of the run it retests.
    pub earlier: Option<&'a Record>,
    /// Now, by the station's clock.
    pub now: Utc,
}

/// Where a check's result came from.
enum Found<'a> {
    Part(&'a PartResult),
    Step(&'a Step),
}

/// Makes the run's record, or says why it cannot be made.
pub fn make(given: Given<'_>) -> Result<Record, Refused> {
    let matrix = given.matrix;
    let answers = given.answers;
    let device = &given.device;
    plain("the device id", &device.id, 64)?;
    if !matrix.knows_revision(&device.hardware_revision) {
        refuse!(
            "hardware revision {:?} is not in matrix version {}",
            device.hardware_revision,
            matrix.suite.version
        );
    }
    plain("the batch", &device.batch, 64)?;
    commit("the lampOS build", &given.lampos_build)?;
    let Some((version, tool_commit)) = given.suite.split_once('+') else {
        refuse!(
            "the suite {:?} is not <matrix version>+<tool commit>",
            given.suite
        );
    };
    if version != matrix.suite.version {
        refuse!(
            "the suite says matrix version {version}, the matrix given is version {}",
            matrix.suite.version
        );
    }
    commit("the station tool's commit", tool_commit)?;
    station(&given.station)?;
    operator(&given.operator)?;

    let (started, ended) = (answers.started, answers.ended);
    if ended < started {
        refuse!("the run ended ({ended}) before it started ({started})");
    }
    if ended.millis() > given.now.millis() + CLOCK_SLACK_MS {
        refuse!("the run ended at {ended}, after now ({})", given.now);
    }
    let within = |at: Utc| started <= at && at <= ended;

    let retest_of = earlier(&given, started)?;
    for repair in &answers.repairs {
        if matrix.check(&repair.check).is_none() {
            refuse!(
                "a repair answers check {:?}, which the matrix does not name",
                repair.check
            );
        }
        line("a repair's detail", &repair.detail, 200)?;
    }

    let mut found: BTreeMap<&str, Found<'_>> = BTreeMap::new();
    let selftest = match given.selftest {
        None => None,
        Some(test) => {
            test.check()
                .map_err(|why| Refused(format!("the self-test report: {why}")))?;
            let at = Utc::from_unix_seconds(test.started)?;
            if at.millis() < started.millis() - CLOCK_SLACK_MS
                || at.millis() > ended.millis() + CLOCK_SLACK_MS
            {
                refuse!(
                    "the self-test started at {at}, outside this run ({started} to {ended}): run it again and copy its newest report"
                );
            }
            for part in &test.parts {
                let Some(check) = matrix.check(&part.part) else {
                    refuse!(
                        "the self-test reports {:?}, a check matrix version {} does not name",
                        part.part,
                        matrix.suite.version
                    );
                };
                if check.method != Method::Selftest {
                    refuse!(
                        "the self-test reports {}, which the matrix has as a {} check",
                        check.id,
                        method_words(check.method)
                    );
                }
                admitted(given.occasion, check)?;
                if found.insert(&check.id, Found::Part(part)).is_some() {
                    refuse!("the self-test reports {} twice", check.id);
                }
            }
            Some(SelfTestUsed {
                occasion: test.occasion,
                started: at,
                took: test.took,
                verdict: test.verdict,
                build: test.build.clone(),
            })
        }
    };
    for step in &answers.steps {
        let Some(check) = matrix.check(&step.check) else {
            refuse!(
                "the station answers {:?}, a check matrix version {} does not name",
                step.check,
                matrix.suite.version
            );
        };
        if check.method == Method::Selftest {
            refuse!(
                "{} is the self-test's: it comes from the lamp's report, not the station's answers",
                check.id
            );
        }
        admitted(given.occasion, check)?;
        if !within(step.at) {
            refuse!(
                "{} was done at {}, outside the run ({started} to {ended})",
                check.id,
                step.at
            );
        }
        match &step.reason {
            Some(reason) => line(&format!("{}'s reason", check.id), reason, 200)?,
            None if step.verdict != Answered::Pass => {
                refuse!("{} is {:?} and gives no reason", check.id, step.verdict)
            }
            None => {}
        }
        if let Some(measurement) = &step.measurement {
            if !measurement.value.is_finite() {
                refuse!("{}'s measurement is not a number", check.id);
            }
            line(&format!("{}'s unit", check.id), &measurement.unit, 16)?;
            if let Some(limit) = &check.limit
                && measurement.unit != limit.unit
            {
                refuse!(
                    "{} is measured in {:?}; its limit is in {:?}",
                    check.id,
                    measurement.unit,
                    limit.unit
                );
            }
        } else if check.limit.is_some() && step.verdict == Answered::Pass {
            refuse!("{} has a limit, so a pass needs its measurement", check.id);
        }
        if found.insert(&check.id, Found::Step(step)).is_some() {
            refuse!("the station answers {} twice", check.id);
        }
    }

    let revision = device.hardware_revision.as_str();
    let mut checks = Vec::new();
    let mut failures = Vec::new();
    let mut incomplete = Vec::new();
    for check in &matrix.checks {
        let result = found.remove(check.id.as_str());
        if !given.occasion.admits(check.kind)
            || (result.is_none() && !given.occasion.expects(check.kind))
        {
            continue;
        }
        let mut entry = CheckResult {
            id: check.id.clone(),
            part: check.part.clone(),
            kind: check.kind,
            method: check.method,
            mandatory: check.mandatory,
            verdict: Outcome::Missing,
            said: None,
            reason: String::new(),
            values: Vec::new(),
            measurement: None,
            limit: check.limit.clone(),
            at: None,
        };
        match result {
            None => {
                entry.reason = match (check.method, &selftest) {
                    (Method::Selftest, None) => "no self-test report was given".into(),
                    (Method::Selftest, Some(_)) => "the self-test did not report it".into(),
                    _ => "the station gave no answer for it".into(),
                }
            }
            Some(Found::Part(part)) => {
                entry.verdict = part.verdict.into();
                entry.reason = part.reason.clone();
                entry.values = part
                    .values
                    .iter()
                    .map(|kv| Reading {
                        key: kv.key.clone(),
                        value: kv.value.clone(),
                    })
                    .collect();
            }
            Some(Found::Step(step)) => {
                entry.verdict = step.verdict.into();
                entry.reason = step.reason.clone().unwrap_or_default();
                entry.measurement = step.measurement.clone();
                entry.at = Some(step.at);
            }
        }
        judge(check, revision, &mut entry, &mut failures, &mut incomplete);
        checks.push(entry);
    }

    let verdict = if !failures.is_empty() {
        RunVerdict::Fail
    } else if !incomplete.is_empty() {
        RunVerdict::Incomplete
    } else {
        RunVerdict::Pass
    };
    if verdict == RunVerdict::Pass && !checks.iter().any(|c| c.verdict == Outcome::Pass) {
        refuse!("nothing in this run passed and nothing was expected of it: no record is made");
    }
    Ok(Record {
        format: FORMAT,
        run_id: format!("{}-{}-{}", started.compact(), given.station, device.id),
        occasion: given.occasion,
        verdict,
        started,
        ended,
        device: given.device.clone(),
        lampos_build: given.lampos_build.clone(),
        suite: Suite {
            matrix_version: matrix.suite.version.clone(),
            matrix_sha256: matrix.sha256.clone(),
            matrix_approved: matrix.suite.approved,
            tool_commit: tool_commit.to_owned(),
        },
        station: given.station.clone(),
        operator: given.operator.clone(),
        retest_of,
        repairs: answers.repairs.clone(),
        selftest,
        checks,
        failures,
        incomplete,
    })
}

/// Judges one check: its failure or why it did not finish, if either.
fn judge(
    check: &Check,
    revision: &str,
    entry: &mut CheckResult,
    failures: &mut Vec<Failure>,
    incomplete: &mut Vec<Unfinished>,
) {
    let failed = |reason: String, entry: &CheckResult| Failure {
        check: entry.id.clone(),
        reason,
        measured: entry.measurement.clone(),
        limit: entry.limit.clone(),
    };
    match entry.verdict {
        Outcome::Pass => {
            let outside = match (&check.limit, &entry.measurement) {
                (Some(limit), Some(measured)) if !limit.holds(measured.value) => Some(format!(
                    "measured {} {}, outside its limit: {}",
                    measured.value,
                    measured.unit,
                    limit.words()
                )),
                _ => None,
            };
            if let Some(reason) = outside {
                entry.said = Some(Outcome::Pass);
                entry.verdict = Outcome::Fail;
                failures.push(failed(reason, entry));
            }
        }
        Outcome::Fail => failures.push(failed(entry.reason.clone(), entry)),
        Outcome::NotFitted => {
            if !check.not_fitted_allowed.iter().any(|r| r == revision) {
                entry.said = Some(Outcome::NotFitted);
                entry.verdict = Outcome::Fail;
                let reason = format!(
                    "not fitted, but revision {revision} must have it: {}",
                    entry.reason
                );
                failures.push(failed(reason, entry));
            }
        }
        Outcome::NotChecked | Outcome::Skipped | Outcome::Interrupted | Outcome::Missing => {
            if check.mandatory {
                incomplete.push(Unfinished {
                    check: entry.id.clone(),
                    verdict: entry.verdict,
                    reason: entry.reason.clone(),
                });
            }
        }
    }
}

/// A qualification check in a routine run or a retest is refused.
fn admitted(occasion: Occasion, check: &Check) -> Result<(), Refused> {
    if !occasion.admits(check.kind) {
        refuse!(
            "{} is a qualification check: never part of a {} run",
            check.id,
            match occasion {
                Occasion::Retest => "retest",
                _ => "routine",
            }
        );
    }
    Ok(())
}

/// For a retest, the run it retests: the same lamp, not passed, a routine
/// run or a retest, ended before this one began, and every repair made
/// between the two. Only a retest names one, and only a retest has repairs.
fn earlier(given: &Given<'_>, started: Utc) -> Result<Option<String>, Refused> {
    let repairs = &given.answers.repairs;
    let Some(earlier) = given.earlier else {
        if given.occasion == Occasion::Retest {
            refuse!("a retest names the run it retests (--retest-of)");
        }
        if !repairs.is_empty() {
            refuse!("repairs are recorded with the retest that follows them");
        }
        return Ok(None);
    };
    if given.occasion != Occasion::Retest {
        refuse!("only a retest names an earlier run");
    }
    if earlier.device.id != given.device.id {
        refuse!(
            "run {} was of {}, not {}",
            earlier.run_id,
            earlier.device.id,
            given.device.id
        );
    }
    if earlier.verdict == RunVerdict::Pass {
        refuse!("run {} passed: there is nothing to retest", earlier.run_id);
    }
    if earlier.occasion == Occasion::Qualification {
        refuse!(
            "run {} was a qualification run; a retest is a routine run",
            earlier.run_id
        );
    }
    if earlier.ended > started {
        refuse!(
            "run {} ended at {}, after this one began ({started})",
            earlier.run_id,
            earlier.ended
        );
    }
    for repair in repairs {
        if repair.at < earlier.ended || repair.at > started {
            refuse!(
                "a repair at {} is not between run {} ({}) and this one ({started})",
                repair.at,
                earlier.run_id,
                earlier.ended
            );
        }
    }
    Ok(Some(earlier.run_id.clone()))
}

/// `station-` and a plain name.
fn station(id: &str) -> Result<(), Refused> {
    match id.strip_prefix("station-") {
        Some(rest) => plain("the station", rest, 32),
        None => refuse!("the station {id:?} is not station-<id>"),
    }
}

/// `op-` and 1 to 12 digits: opaque, local to the station, never a name.
fn operator(id: &str) -> Result<(), Refused> {
    match id.strip_prefix("op-") {
        Some(digits)
            if (1..=12).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit()) =>
        {
            Ok(())
        }
        _ => refuse!(
            "the operator {id:?} is not op-<digits>: an opaque id local to the station, never a name"
        ),
    }
}

fn method_words(method: Method) -> &'static str {
    match method {
        Method::Selftest => "self-test",
        Method::Station => "station",
        Method::Manual => "manual",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matrix::tests::{SMALL, V0};

    const STARTED: &str = "2026-10-19T08:00:00Z";
    const ENDED: &str = "2026-10-19T08:30:00Z";
    /// The self-test's start, five minutes into the run, in Unix seconds.
    const SELFTEST_AT: f64 = 1_792_396_800.0 + 300.0;

    fn at(text: &str) -> Utc {
        text.parse().unwrap()
    }

    fn part(name: &str, verdict: Verdict) -> PartResult {
        let reason = if verdict == Verdict::Pass {
            ""
        } else {
            "told to"
        };
        PartResult::new(name, verdict, reason)
    }

    fn selftest(parts: Vec<PartResult>) -> SelfTest {
        SelfTest {
            occasion: SelfTestOccasion::Factory,
            started: SELFTEST_AT,
            took: 95.0,
            verdict: SelfTest::verdict_of(&parts),
            parts,
            build: None,
        }
    }

    /// The small matrix's three self-test parts, all passing.
    fn good_selftest() -> SelfTest {
        selftest(vec![
            part("ring", Verdict::Pass),
            part("touch_by_hand", Verdict::Pass),
            part("air", Verdict::Pass),
        ])
    }

    fn step(check: &str, verdict: Answered) -> Step {
        Step {
            check: check.into(),
            verdict,
            measurement: None,
            reason: (verdict != Answered::Pass).then(|| "told to".into()),
            at: at("2026-10-19T08:10:00Z"),
        }
    }

    fn measured(check: &str, verdict: Answered, value: f64) -> Step {
        Step {
            measurement: Some(Measurement {
                value,
                unit: "°C".into(),
            }),
            ..step(check, verdict)
        }
    }

    /// The small matrix's mandatory station and manual steps, all passing.
    fn good_steps() -> Vec<Step> {
        vec![
            step("privacy_switch", Answered::Pass),
            measured("board_load_temperature", Answered::Pass, 71.5),
        ]
    }

    fn answers(steps: Vec<Step>) -> Answers {
        Answers {
            started: at(STARTED),
            ended: at(ENDED),
            steps,
            repairs: Vec::new(),
        }
    }

    struct Run {
        matrix: Matrix,
        selftest: Option<SelfTest>,
        answers: Answers,
        revision: String,
        occasion: Occasion,
        suite: String,
        operator: String,
        earlier: Option<Record>,
    }

    impl Run {
        fn new(selftest: Option<SelfTest>, steps: Vec<Step>) -> Self {
            Self {
                matrix: Matrix::parse(SMALL).unwrap(),
                selftest,
                answers: answers(steps),
                revision: "a".into(),
                occasion: Occasion::Routine,
                suite: "7+dab3f1242486".into(),
                operator: "op-7".into(),
                earlier: None,
            }
        }

        fn good() -> Self {
            Self::new(Some(good_selftest()), good_steps())
        }

        fn make(&self) -> Result<Record, Refused> {
            make(Given {
                matrix: &self.matrix,
                selftest: self.selftest.as_ref(),
                answers: &self.answers,
                device: Device {
                    id: "lamp-4ace".into(),
                    hardware_revision: self.revision.clone(),
                    batch: "pilot-1".into(),
                },
                lampos_build: "dab3f1242486beb7797393d5006396f868f7858a".into(),
                suite: self.suite.clone(),
                station: "station-01".into(),
                operator: self.operator.clone(),
                occasion: self.occasion,
                earlier: self.earlier.as_ref(),
                now: at("2026-10-19T08:31:00Z"),
            })
        }

        fn verdict(&self) -> RunVerdict {
            self.make().unwrap().verdict
        }

        fn refused(&self) -> String {
            self.make().unwrap_err().0
        }
    }

    fn check<'a>(record: &'a Record, id: &str) -> &'a CheckResult {
        record.checks.iter().find(|c| c.id == id).unwrap()
    }

    #[test]
    fn every_check_passing_is_a_pass_and_says_what_judged_it() {
        let record = Run::good().make().unwrap();
        assert_eq!(record.verdict, RunVerdict::Pass);
        assert!(record.failures.is_empty() && record.incomplete.is_empty());
        assert_eq!(record.run_id, "20261019T080000Z-station-01-lamp-4ace");
        assert_eq!(record.suite.matrix_version, "7");
        assert!(!record.suite.matrix_approved);
        assert_eq!(record.suite.tool_commit, "dab3f1242486");
        assert_eq!(
            record.selftest.as_ref().unwrap().started,
            at("2026-10-19T08:05:00Z")
        );
        // Every routine check is in it, the one not given among them; the
        // qualification check is not.
        let ids: Vec<&str> = record.checks.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "ring",
                "touch_by_hand",
                "air",
                "privacy_switch",
                "board_load_temperature",
                "camera_face_distance"
            ]
        );
        assert_eq!(
            check(&record, "camera_face_distance").verdict,
            Outcome::Missing
        );
        assert_eq!(
            check(&record, "board_load_temperature")
                .measurement
                .as_ref()
                .unwrap()
                .value,
            71.5
        );
    }

    /// The rule the task names: a self-test that passes but for one
    /// mandatory part it could not check is `incomplete`, never `pass`.
    #[test]
    fn a_self_test_passing_but_for_one_not_checked_mandatory_part_is_incomplete() {
        let run = Run::new(
            Some(selftest(vec![
                part("ring", Verdict::Pass),
                part("touch_by_hand", Verdict::NotChecked),
                part("air", Verdict::Pass),
            ])),
            good_steps(),
        );
        let record = run.make().unwrap();
        assert_eq!(record.verdict, RunVerdict::Incomplete);
        assert_eq!(
            record.incomplete,
            [Unfinished {
                check: "touch_by_hand".into(),
                verdict: Outcome::NotChecked,
                reason: "told to".into(),
            }]
        );
        assert!(record.failures.is_empty());
    }

    #[test]
    fn a_mandatory_check_missing_skipped_or_interrupted_is_incomplete() {
        // The self-test did not report a part.
        let run = Run::new(
            Some(selftest(vec![
                part("ring", Verdict::Pass),
                part("air", Verdict::Pass),
            ])),
            good_steps(),
        );
        let record = run.make().unwrap();
        assert_eq!(record.verdict, RunVerdict::Incomplete);
        assert_eq!(record.incomplete[0].check, "touch_by_hand");
        assert_eq!(record.incomplete[0].verdict, Outcome::Missing);
        assert!(record.incomplete[0].reason.contains("did not report"));
        // No self-test at all.
        let record = Run::new(None, good_steps()).make().unwrap();
        assert_eq!(record.verdict, RunVerdict::Incomplete);
        assert_eq!(record.incomplete.len(), 3);
        assert!(record.selftest.is_none());
        // A mandatory step not answered, skipped, interrupted or not checked.
        let run = Run::new(
            Some(good_selftest()),
            vec![measured("board_load_temperature", Answered::Pass, 70.0)],
        );
        assert_eq!(run.verdict(), RunVerdict::Incomplete);
        for verdict in [
            Answered::Skipped,
            Answered::Interrupted,
            Answered::NotChecked,
        ] {
            let run = Run::new(
                Some(good_selftest()),
                vec![
                    step("privacy_switch", verdict),
                    measured("board_load_temperature", Answered::Pass, 70.0),
                ],
            );
            let record = run.make().unwrap();
            assert_eq!(record.verdict, RunVerdict::Incomplete, "{verdict:?}");
            assert_eq!(record.incomplete[0].verdict, Outcome::from(verdict));
        }
    }

    #[test]
    fn a_check_that_is_not_mandatory_may_be_left_out_but_not_failed() {
        let mut steps = good_steps();
        steps.push(step("camera_face_distance", Answered::Skipped));
        let run = Run::new(Some(good_selftest()), steps);
        assert_eq!(run.verdict(), RunVerdict::Pass);
        let mut steps = good_steps();
        steps.push(step("camera_face_distance", Answered::Fail));
        let record = Run::new(Some(good_selftest()), steps).make().unwrap();
        assert_eq!(record.verdict, RunVerdict::Fail);
        assert_eq!(record.failures[0].check, "camera_face_distance");
    }

    #[test]
    fn any_fail_makes_the_run_fail_even_beside_an_incomplete_check() {
        let run = Run::new(
            Some(selftest(vec![
                part("ring", Verdict::Fail),
                part("touch_by_hand", Verdict::NotChecked),
                part("air", Verdict::Pass),
            ])),
            good_steps(),
        );
        let record = run.make().unwrap();
        assert_eq!(record.verdict, RunVerdict::Fail);
        assert_eq!(record.failures[0].check, "ring");
        assert_eq!(record.failures[0].reason, "told to");
        // What did not finish is still said.
        assert_eq!(record.incomplete[0].check, "touch_by_hand");
        let mut steps = good_steps();
        steps[0] = step("privacy_switch", Answered::Fail);
        let record = Run::new(Some(good_selftest()), steps).make().unwrap();
        assert_eq!(record.verdict, RunVerdict::Fail);
        assert_eq!(record.failures[0].check, "privacy_switch");
    }

    #[test]
    fn a_measurement_outside_its_limit_fails_whatever_was_said() {
        let mut steps = good_steps();
        steps[1] = measured("board_load_temperature", Answered::Pass, 87.0);
        let record = Run::new(Some(good_selftest()), steps).make().unwrap();
        assert_eq!(record.verdict, RunVerdict::Fail);
        let failure = &record.failures[0];
        assert_eq!(failure.check, "board_load_temperature");
        assert_eq!(failure.measured.as_ref().unwrap().value, 87.0);
        assert_eq!(failure.limit.as_ref().unwrap().max, Some(85.0));
        assert!(
            failure.reason.contains("at most 85 °C"),
            "{}",
            failure.reason
        );
        let entry = check(&record, "board_load_temperature");
        assert_eq!(
            (entry.verdict, entry.said),
            (Outcome::Fail, Some(Outcome::Pass))
        );
        // On the limit is within it.
        let mut steps = good_steps();
        steps[1] = measured("board_load_temperature", Answered::Pass, 85.0);
        assert_eq!(
            Run::new(Some(good_selftest()), steps).verdict(),
            RunVerdict::Pass
        );
        // A fail within the limit stays a fail: the operator saw something.
        let mut steps = good_steps();
        steps[1] = measured("board_load_temperature", Answered::Fail, 60.0);
        assert_eq!(
            Run::new(Some(good_selftest()), steps).verdict(),
            RunVerdict::Fail
        );
    }

    #[test]
    fn a_limited_check_needs_its_measurement_in_its_unit() {
        let mut steps = good_steps();
        steps[1] = step("board_load_temperature", Answered::Pass);
        let why = Run::new(Some(good_selftest()), steps).refused();
        assert!(why.contains("needs its measurement"), "{why}");
        let mut steps = good_steps();
        steps[1].measurement.as_mut().unwrap().unit = "°F".into();
        let why = Run::new(Some(good_selftest()), steps).refused();
        assert!(why.contains("its limit is in"), "{why}");
        let mut steps = good_steps();
        steps[1].measurement.as_mut().unwrap().value = f64::INFINITY;
        assert!(Run::new(Some(good_selftest()), steps).make().is_err());
        // A failed part that could not be measured is still recorded.
        let mut steps = good_steps();
        steps[1] = step("board_load_temperature", Answered::Fail);
        assert_eq!(
            Run::new(Some(good_selftest()), steps).verdict(),
            RunVerdict::Fail
        );
    }

    #[test]
    fn not_fitted_is_fine_only_where_the_matrix_allows_it_for_that_revision() {
        let no_air = || {
            selftest(vec![
                part("ring", Verdict::Pass),
                part("touch_by_hand", Verdict::Pass),
                part("air", Verdict::NotFitted),
            ])
        };
        let mut run = Run::new(Some(no_air()), good_steps());
        run.revision = "a-no-air".into();
        let record = run.make().unwrap();
        assert_eq!(record.verdict, RunVerdict::Pass);
        assert_eq!(check(&record, "air").verdict, Outcome::NotFitted);
        // The same lamp's report on a revision that has the sensor.
        run.revision = "a".into();
        let record = run.make().unwrap();
        assert_eq!(record.verdict, RunVerdict::Fail);
        assert!(
            record.failures[0]
                .reason
                .contains("revision a must have it"),
            "{}",
            record.failures[0].reason
        );
        let entry = check(&record, "air");
        assert_eq!(
            (entry.verdict, entry.said),
            (Outcome::Fail, Some(Outcome::NotFitted))
        );
        // A part the matrix never allows to be absent.
        let mut steps = good_steps();
        steps[0] = step("privacy_switch", Answered::NotFitted);
        let mut run = Run::new(Some(good_selftest()), steps);
        run.revision = "a-no-air".into();
        assert_eq!(run.verdict(), RunVerdict::Fail);
    }

    #[test]
    fn an_unknown_or_misplaced_check_is_refused() {
        let mut test = good_selftest();
        test.parts.push(part("antenna", Verdict::Pass));
        test.verdict = SelfTest::verdict_of(&test.parts);
        let why = Run::new(Some(test), good_steps()).refused();
        assert!(why.contains("\"antenna\""), "{why}");
        let mut steps = good_steps();
        steps.push(step("antenna", Answered::Pass));
        let why = Run::new(Some(good_selftest()), steps).refused();
        assert!(why.contains("\"antenna\""), "{why}");
        // A self-test part given as a station step, and the other way round.
        let mut steps = good_steps();
        steps.push(step("ring", Answered::Pass));
        let why = Run::new(Some(good_selftest()), steps).refused();
        assert!(why.contains("self-test's"), "{why}");
        let mut test = good_selftest();
        test.parts.push(part("privacy_switch", Verdict::Pass));
        let why = Run::new(Some(test), good_steps()).refused();
        assert!(why.contains("manual check"), "{why}");
        // Twice.
        let mut steps = good_steps();
        steps.push(step("privacy_switch", Answered::Pass));
        assert!(
            Run::new(Some(good_selftest()), steps)
                .refused()
                .contains("twice")
        );
        let mut test = good_selftest();
        test.parts.push(part("ring", Verdict::Pass));
        assert!(
            Run::new(Some(test), good_steps())
                .refused()
                .contains("twice")
        );
    }

    /// The rule the task names: a qualification check inside a routine run
    /// is refused, and inside a retest too.
    #[test]
    fn a_qualification_check_inside_a_routine_run_is_refused() {
        let mut steps = good_steps();
        steps.push(step("servo_endurance", Answered::Pass));
        let run = Run::new(Some(good_selftest()), steps.clone());
        let why = run.refused();
        assert!(why.contains("never part of a routine run"), "{why}");
        let mut retest = Run::new(Some(good_selftest()), steps.clone());
        retest.occasion = Occasion::Retest;
        retest.earlier = Some(failed_earlier());
        retest.answers.started = at("2026-10-19T08:00:00Z");
        assert!(retest.refused().contains("never part of a retest run"));
        // In a qualification run it is the point, and it is expected there.
        let mut qualification = Run::new(Some(good_selftest()), steps);
        qualification.occasion = Occasion::Qualification;
        let record = qualification.make().unwrap();
        assert_eq!(record.verdict, RunVerdict::Pass);
        assert_eq!(check(&record, "servo_endurance").kind, Kind::Qualification);
        // Not done, a qualification run is incomplete; the routine checks
        // it did not give are not asked of it.
        let mut qualification = Run::new(None, Vec::new());
        qualification.occasion = Occasion::Qualification;
        let record = qualification.make().unwrap();
        assert_eq!(record.verdict, RunVerdict::Incomplete);
        let ids: Vec<&str> = record.checks.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["servo_endurance"]);
    }

    /// A routine run of lamp-4ace that failed, an hour before.
    fn failed_earlier() -> Record {
        let mut earlier = Run::new(
            Some(selftest(vec![
                part("ring", Verdict::Fail),
                part("touch_by_hand", Verdict::Pass),
                part("air", Verdict::Pass),
            ])),
            good_steps(),
        );
        earlier.answers.started = at("2026-10-19T06:00:00Z");
        earlier.answers.ended = at("2026-10-19T06:30:00Z");
        for step in &mut earlier.answers.steps {
            step.at = at("2026-10-19T06:10:00Z");
        }
        earlier.selftest.as_mut().unwrap().started = SELFTEST_AT - 7200.0;
        let record = earlier.make().unwrap();
        assert_eq!(record.verdict, RunVerdict::Fail);
        record
    }

    fn retest() -> Run {
        let mut run = Run::good();
        run.occasion = Occasion::Retest;
        run.earlier = Some(failed_earlier());
        run.answers.repairs = vec![Repair {
            at: at("2026-10-19T07:00:00Z"),
            check: "ring".into(),
            action: crate::inputs::Action::Reseated,
            detail: "the ring's cable".into(),
        }];
        run
    }

    #[test]
    fn a_retest_names_the_failed_run_and_its_repairs() {
        let record = retest().make().unwrap();
        assert_eq!(record.verdict, RunVerdict::Pass);
        assert_eq!(
            record.retest_of.as_deref(),
            Some("20261019T060000Z-station-01-lamp-4ace")
        );
        assert_eq!(record.repairs.len(), 1);
        assert_eq!(record.occasion, Occasion::Retest);
        // Without the run it retests.
        let mut run = retest();
        run.earlier = None;
        assert!(run.refused().contains("--retest-of"));
        // Of another lamp, a run that passed, or one after it.
        let mut run = retest();
        run.earlier.as_mut().unwrap().device.id = "lamp-52e6".into();
        assert!(run.refused().contains("lamp-52e6"));
        let mut run = retest();
        run.earlier.as_mut().unwrap().verdict = RunVerdict::Pass;
        assert!(run.refused().contains("nothing to retest"));
        let mut run = retest();
        run.earlier.as_mut().unwrap().ended = at("2026-10-19T08:01:00Z");
        assert!(run.refused().contains("after this one began"));
        let mut run = retest();
        run.earlier.as_mut().unwrap().occasion = Occasion::Qualification;
        assert!(run.refused().contains("qualification run"));
        // A repair outside the gap between the two runs.
        let mut run = retest();
        run.answers.repairs[0].at = at("2026-10-19T06:15:00Z");
        assert!(run.refused().contains("not between"));
        let mut run = retest();
        run.answers.repairs[0].check = "antenna".into();
        assert!(run.refused().contains("does not name"));
        // Repairs, or an earlier run, belong to a retest only.
        let mut run = retest();
        run.occasion = Occasion::Routine;
        assert!(run.refused().contains("only a retest"));
        let mut run = retest();
        run.occasion = Occasion::Routine;
        run.earlier = None;
        assert!(run.refused().contains("with the retest"));
    }

    #[test]
    fn a_time_outside_the_run_is_refused() {
        let mut steps = good_steps();
        steps[0].at = at("2026-10-19T09:00:00Z");
        assert!(
            Run::new(Some(good_selftest()), steps)
                .refused()
                .contains("outside the run")
        );
        // A self-test from another day: an old latest.json.
        let mut test = good_selftest();
        test.started -= 86_400.0;
        assert!(
            Run::new(Some(test), good_steps())
                .refused()
                .contains("run it again")
        );
        // Within the clocks' slack it is taken.
        let mut test = good_selftest();
        test.started = SELFTEST_AT - 300.0 - 240.0;
        assert!(Run::new(Some(test), good_steps()).make().is_ok());
        let mut run = Run::good();
        run.answers.ended = at("2026-10-19T07:59:00Z");
        assert!(run.refused().contains("before it started"));
        let mut run = Run::good();
        run.answers.ended = at("2026-10-20T08:30:00Z");
        assert!(run.refused().contains("after now"));
    }

    #[test]
    fn the_identity_is_plain_and_the_operator_opaque() {
        let mut run = Run::good();
        run.revision = "b".into();
        assert!(run.refused().contains("not in matrix version 7"));
        let mut run = Run::good();
        run.suite = "6+dab3f1242486".into();
        assert!(run.refused().contains("version 6"));
        let mut run = Run::good();
        run.suite = "7".into();
        assert!(run.make().is_err());
        for refused in ["alice", "op-alice", "op-", "op-1234567890123", "Op-7"] {
            let mut run = Run::good();
            run.operator = refused.into();
            assert!(run.refused().contains("never a name"), "{refused}");
        }
    }

    #[test]
    fn a_non_pass_step_gives_its_reason() {
        let mut steps = good_steps();
        steps[0] = step("privacy_switch", Answered::Fail);
        steps[0].reason = None;
        let why = Run::new(Some(good_selftest()), steps).refused();
        assert!(why.contains("gives no reason"), "{why}");
        let mut steps = good_steps();
        steps[0] = step("privacy_switch", Answered::Fail);
        steps[0].reason = Some("two\nlines".into());
        assert!(Run::new(Some(good_selftest()), steps).make().is_err());
    }

    #[test]
    fn a_run_with_nothing_to_judge_is_never_a_pass() {
        let matrix = SMALL.replace("mandatory = true", "mandatory = false");
        let mut run = Run::new(None, Vec::new());
        run.matrix = Matrix::parse(&matrix).unwrap();
        assert!(run.refused().contains("nothing in this run passed"));
    }

    #[test]
    fn a_record_reads_back_as_written_and_refuses_what_it_does_not_have() {
        let record = retest().make().unwrap();
        let text = serde_json::to_string_pretty(&record).unwrap();
        assert_eq!(Record::parse(&text).unwrap(), record);
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["operator_name"] = "x".into();
        assert!(Record::parse(&value.to_string()).is_err());
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["format"] = 2.into();
        assert!(Record::parse(&value.to_string()).is_err());
    }

    /// The repository's matrix with a lamp that passed every self-test part
    /// and every mandatory manual step is a pass, and its record lists
    /// each routine check once.
    #[test]
    fn the_repositorys_matrix_judges_a_whole_routine_run() {
        let matrix = Matrix::parse(V0).unwrap();
        let parts: Vec<PartResult> = matrix
            .checks
            .iter()
            .filter(|c| c.method == Method::Selftest)
            .map(|c| part(&c.id, Verdict::Pass))
            .collect();
        let steps: Vec<Step> = matrix
            .checks
            .iter()
            .filter(|c| {
                c.method != Method::Selftest && c.mandatory && c.kind != Kind::Qualification
            })
            .map(|c| step(&c.id, Answered::Pass))
            .collect();
        let mut run = Run::new(Some(selftest(parts)), steps);
        run.matrix = matrix;
        run.suite = "0+dab3f1242486".into();
        run.revision = "draft-a".into();
        let record = run.make().unwrap();
        assert_eq!(record.verdict, RunVerdict::Pass, "{:?}", record.incomplete);
        assert!(record.checks.iter().all(|c| c.kind != Kind::Qualification));
        let routine = run
            .matrix
            .checks
            .iter()
            .filter(|c| c.kind != Kind::Qualification)
            .count();
        assert_eq!(record.checks.len(), routine);
    }
}
