//! Whether one run's record is complete against a matrix version (#2795,
//! the pilot): made with that very matrix file, approved, and every check
//! its occasion expects judged `pass` or `fail` against the matrix's
//! limits, none missing.
//!
//! "Complete" is not "passed": a known-faulty pilot lamp's run fails, and
//! its record is still complete when every check was judged. A mandatory
//! check that is missing, skipped, interrupted or `not_checked` leaves it
//! incomplete. A check that is not mandatory may be left out; it is listed
//! apart, so a person sees it, and never counts as a pass.
//!
//! The record is judged again from the matrix, not trusted: a verdict, a
//! failure or a limit that does not follow from the matrix is a gap. So is
//! a verdict the record says was changed from what the station said where
//! the matrix would not change it: only a pass outside its limit, or
//! `not_fitted` where the revision must have the part, becomes a fail.
//! `failures` and `incomplete` are lists in the matrix's order, as the
//! record tool writes them: each check once, an unfinished one with its own
//! verdict.
//!
//! The result carries the run's verdict as judged here, so a caller tells a
//! complete failed run from a complete passed one.
//!
//! It reads a record already kept and changes nothing. Where records are
//! kept and finding one lamp's records are the station tool's (#2794).
use crate::{
    matrix::{Check, Matrix},
    record::{CheckResult, Outcome, Record, RunVerdict},
};
use std::{collections::BTreeSet, fmt};

/// Why a record is not complete against the matrix.
#[derive(Clone, Debug, PartialEq)]
pub enum Gap {
    /// The record was made with another matrix file: another version, or
    /// the same version edited.
    OtherMatrix { version: String },
    /// The record's matrix was not approved by the hardware lead.
    NotApproved,
    /// The lamp's hardware revision is not one the matrix names.
    UnknownRevision(String),
    /// The record lists a check the matrix does not name.
    Unknown(String),
    /// The record lists a check twice.
    Twice(String),
    /// The record lists a check its occasion never holds: a qualification
    /// check in a routine run or a retest.
    NotAdmitted(String),
    /// The record's part, kind, method, mandatory or limit for a check is
    /// not the matrix's.
    NotAsMatrix(String),
    /// A mandatory check the run's occasion expects was not judged.
    Unfinished { check: String, verdict: Outcome },
    /// A check's verdict does not follow from the matrix's limits or its
    /// revisions.
    Misjudged { check: String, why: String },
    /// The record's `failures` or `incomplete` are not its checks'.
    Lists(String),
    /// The run's verdict is not the one its checks give.
    Verdict {
        recorded: RunVerdict,
        judged: RunVerdict,
    },
}

impl fmt::Display for Gap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Gap::OtherMatrix { version } => {
                write!(f, "made with another matrix file (version {version})")
            }
            Gap::NotApproved => {
                f.write_str("made with a matrix the hardware lead had not approved")
            }
            Gap::UnknownRevision(revision) => {
                write!(f, "hardware revision {revision} is not in the matrix")
            }
            Gap::Unknown(check) => write!(f, "{check} is not in the matrix"),
            Gap::Twice(check) => write!(f, "{check} is listed twice"),
            Gap::NotAdmitted(check) => {
                write!(f, "{check} is a qualification check, never in this run")
            }
            Gap::NotAsMatrix(check) => write!(f, "{check} is not recorded as the matrix has it"),
            Gap::Unfinished { check, verdict } => {
                write!(f, "{check} is mandatory and was not judged: {verdict:?}")
            }
            Gap::Misjudged { check, why } => write!(f, "{check} is misjudged: {why}"),
            Gap::Lists(why) => f.write_str(why),
            Gap::Verdict { recorded, judged } => {
                write!(f, "the run says {recorded:?}, its checks give {judged:?}")
            }
        }
    }
}

impl Gap {
    /// The gap's kind and the check or value it names, without the why: a
    /// word for each kind, and for a check not judged its verdict, as
    /// `missing privacy_switch`. For a line per lamp (`fleet`).
    pub fn short(&self) -> String {
        match self {
            Gap::OtherMatrix { version } => format!("other_matrix {version}"),
            Gap::NotApproved => "not_approved".into(),
            Gap::UnknownRevision(revision) => format!("unknown_revision {revision}"),
            Gap::Unknown(check) => format!("unknown {check}"),
            Gap::Twice(check) => format!("twice {check}"),
            Gap::NotAdmitted(check) => format!("not_admitted {check}"),
            Gap::NotAsMatrix(check) => format!("not_as_matrix {check}"),
            Gap::Unfinished { check, verdict } => {
                let verdict = serde_json::to_value(verdict)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default();
                format!("{verdict} {check}")
            }
            Gap::Misjudged { check, .. } => format!("misjudged {check}"),
            Gap::Lists(_) => "lists".into(),
            Gap::Verdict { .. } => "verdict".into(),
        }
    }
}

/// One record checked against one matrix.
#[derive(Clone, Debug, PartialEq)]
pub struct Completeness {
    pub run_id: String,
    pub device: String,
    /// The run's verdict as its checks give it, judged here, whatever the
    /// record says. A complete record can be a failed run.
    pub verdict: RunVerdict,
    /// What keeps the record from being complete; none when it is.
    pub gaps: Vec<Gap>,
    /// Checks that are not mandatory and were not judged, for a person to
    /// see. They do not make the record incomplete and never pass.
    pub not_mandatory_not_judged: Vec<String>,
}

impl Completeness {
    /// Complete against the matrix: no gap.
    pub fn complete(&self) -> bool {
        self.gaps.is_empty()
    }
}

/// Checks `record` against `matrix`.
pub fn check(matrix: &Matrix, record: &Record) -> Completeness {
    let mut gaps = Vec::new();
    let suite = &record.suite;
    if suite.matrix_version != matrix.suite.version || suite.matrix_sha256 != matrix.sha256 {
        gaps.push(Gap::OtherMatrix {
            version: suite.matrix_version.clone(),
        });
    }
    if !suite.matrix_approved || !matrix.suite.approved {
        gaps.push(Gap::NotApproved);
    }
    let revision = record.device.hardware_revision.as_str();
    if !matrix.knows_revision(revision) {
        gaps.push(Gap::UnknownRevision(revision.to_owned()));
    }

    let mut seen = BTreeSet::new();
    for entry in &record.checks {
        if !seen.insert(entry.id.as_str()) {
            gaps.push(Gap::Twice(entry.id.clone()));
            continue;
        }
        let Some(check) = matrix.check(&entry.id) else {
            gaps.push(Gap::Unknown(entry.id.clone()));
            continue;
        };
        if !record.occasion.admits(check.kind) {
            gaps.push(Gap::NotAdmitted(entry.id.clone()));
            continue;
        }
        if !as_matrix(check, entry) {
            gaps.push(Gap::NotAsMatrix(entry.id.clone()));
        }
        if let Some(why) = misjudged(check, revision, entry) {
            gaps.push(Gap::Misjudged {
                check: entry.id.clone(),
                why,
            });
        }
    }

    // Each check the occasion expects, judged or not, as the record lists
    // it or missing from it.
    let mut failed = Vec::new();
    let mut unfinished = Vec::new();
    let mut not_mandatory_not_judged = Vec::new();
    for check in &matrix.checks {
        let entry = record.checks.iter().find(|c| c.id == check.id);
        if !record.occasion.admits(check.kind)
            || (entry.is_none() && !record.occasion.expects(check.kind))
        {
            continue;
        }
        let verdict = entry.map_or(Outcome::Missing, |e| e.verdict);
        match verdict {
            Outcome::Fail => {
                failed.push(check.id.as_str());
            }
            Outcome::Pass | Outcome::NotFitted => {}
            Outcome::NotChecked | Outcome::Skipped | Outcome::Interrupted | Outcome::Missing => {
                if check.mandatory {
                    unfinished.push((check.id.as_str(), verdict));
                    gaps.push(Gap::Unfinished {
                        check: check.id.clone(),
                        verdict,
                    });
                } else {
                    not_mandatory_not_judged.push(check.id.clone());
                }
            }
        }
    }

    // Compared as lists, not sets: a check listed twice, or an unfinished
    // one listed with another verdict than its own, is a gap.
    let listed_failures: Vec<&str> = record.failures.iter().map(|f| f.check.as_str()).collect();
    if listed_failures != failed {
        gaps.push(Gap::Lists(format!(
            "failures lists {listed_failures:?}, its checks fail {failed:?}"
        )));
    }
    // A check a record leaves out is missing, so `incomplete` names it too;
    // the record tool puts every unfinished mandatory check there.
    let listed_unfinished: Vec<(&str, Outcome)> = record
        .incomplete
        .iter()
        .map(|u| (u.check.as_str(), u.verdict))
        .collect();
    if listed_unfinished != unfinished {
        gaps.push(Gap::Lists(format!(
            "incomplete lists {listed_unfinished:?}, its checks leave {unfinished:?} unfinished"
        )));
    }

    let judged = if !failed.is_empty() {
        RunVerdict::Fail
    } else if !unfinished.is_empty() {
        RunVerdict::Incomplete
    } else {
        RunVerdict::Pass
    };
    if judged != record.verdict {
        gaps.push(Gap::Verdict {
            recorded: record.verdict,
            judged,
        });
    }

    Completeness {
        run_id: record.run_id.clone(),
        device: record.device.id.clone(),
        verdict: judged,
        gaps,
        not_mandatory_not_judged,
    }
}

/// Whether the record describes the check as the matrix does.
fn as_matrix(check: &Check, entry: &CheckResult) -> bool {
    entry.part == check.part
        && entry.kind == check.kind
        && entry.method == check.method
        && entry.mandatory == check.mandatory
        && entry.limit == check.limit
}

/// Why the check's verdict does not follow from the matrix, if it does not.
fn misjudged(check: &Check, revision: &str, entry: &CheckResult) -> Option<String> {
    let allowed = check.not_fitted_allowed.iter().any(|r| r == revision);
    let within = match (&check.limit, &entry.measurement) {
        (Some(limit), Some(measured)) => {
            if measured.unit != limit.unit {
                return Some(format!(
                    "measured in {:?}, its limit is in {:?}",
                    measured.unit, limit.unit
                ));
            }
            Some(limit.holds(measured.value))
        }
        _ => None,
    };
    match (entry.verdict, entry.said) {
        // The record tool sets `said` only when it turns a pass or a
        // `not_fitted` into a fail; any other is a verdict changed by hand,
        // as a station's fail recorded as a pass.
        (verdict, Some(said))
            if verdict != Outcome::Fail || !matches!(said, Outcome::Pass | Outcome::NotFitted) =>
        {
            Some(format!(
                "recorded {verdict:?} where {said:?} was said; only a pass or not_fitted is ever turned into a fail"
            ))
        }
        (Outcome::Pass, _) if check.limit.is_some() && entry.measurement.is_none() => {
            Some("a pass with a limit and no measurement".into())
        }
        (Outcome::Pass, _) if within == Some(false) => {
            Some("a pass with its measurement outside its limit".into())
        }
        (Outcome::NotFitted, _) if !allowed => {
            Some(format!("not_fitted, but revision {revision} must have it"))
        }
        (Outcome::Fail, Some(Outcome::Pass)) if within != Some(false) => {
            Some("failed for its limit, but its measurement is within it".into())
        }
        (Outcome::Fail, Some(Outcome::NotFitted)) if allowed => Some(format!(
            "failed for not_fitted, which revision {revision} may be"
        )),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{
        inputs::Measurement,
        matrix::tests::{SMALL, V0},
        record::{Device, FORMAT, Failure, Occasion, Suite, Unfinished},
    };

    /// The small matrix, approved.
    pub(crate) fn matrix() -> Matrix {
        Matrix::parse(&SMALL.replace("approved = false", "approved = true")).unwrap()
    }

    fn entry(check: &Check, verdict: Outcome) -> CheckResult {
        CheckResult {
            id: check.id.clone(),
            part: check.part.clone(),
            kind: check.kind,
            method: check.method,
            mandatory: check.mandatory,
            verdict,
            said: None,
            reason: String::new(),
            values: Vec::new(),
            measurement: check.limit.as_ref().map(|limit| Measurement {
                value: limit.min.or(limit.max).unwrap_or(0.0),
                unit: limit.unit.clone(),
            }),
            limit: check.limit.clone(),
            at: None,
        }
    }

    /// A routine run of `matrix` on `lamp-4ace` in which every routine
    /// check passed, as the station tool records it.
    pub(crate) fn passed(matrix: &Matrix) -> Record {
        Record {
            format: FORMAT,
            run_id: "20261019T080000Z-station-01-lamp-4ace".into(),
            occasion: Occasion::Routine,
            verdict: RunVerdict::Pass,
            started: "2026-10-19T08:00:00Z".parse().unwrap(),
            ended: "2026-10-19T08:30:00Z".parse().unwrap(),
            device: Device {
                id: "lamp-4ace".into(),
                hardware_revision: matrix.revisions[0].id.clone(),
                batch: "pilot-1".into(),
            },
            lampos_build: "dab3f12".into(),
            suite: Suite {
                matrix_version: matrix.suite.version.clone(),
                matrix_sha256: matrix.sha256.clone(),
                matrix_approved: matrix.suite.approved,
                tool_commit: "dab3f12".into(),
            },
            station: "station-01".into(),
            operator: "op-7".into(),
            retest_of: None,
            repairs: Vec::new(),
            selftest: None,
            checks: matrix
                .checks
                .iter()
                .filter(|c| Occasion::Routine.expects(c.kind))
                .map(|c| entry(c, Outcome::Pass))
                .collect(),
            failures: Vec::new(),
            incomplete: Vec::new(),
        }
    }

    fn get<'a>(record: &'a mut Record, id: &str) -> &'a mut CheckResult {
        record.checks.iter_mut().find(|c| c.id == id).unwrap()
    }

    #[test]
    fn a_run_with_every_check_judged_is_complete() {
        let matrix = matrix();
        let record = passed(&matrix);
        let result = check(&matrix, &record);
        assert!(result.complete(), "{:?}", result.gaps);
        assert_eq!(result.device, "lamp-4ace");
        assert_eq!(result.verdict, RunVerdict::Pass);
        assert!(result.not_mandatory_not_judged.is_empty());
    }

    /// A known-faulty pilot lamp's failed run is complete: every check was
    /// judged.
    #[test]
    fn a_failed_run_is_complete_when_every_check_was_judged() {
        let matrix = matrix();
        let mut record = passed(&matrix);
        let ring = get(&mut record, "ring");
        ring.verdict = Outcome::Fail;
        ring.reason = "pixel 12 stays dark".into();
        record.failures.push(Failure {
            check: "ring".into(),
            reason: "pixel 12 stays dark".into(),
            measured: None,
            limit: None,
        });
        record.verdict = RunVerdict::Fail;
        let result = check(&matrix, &record);
        assert!(result.complete(), "{:?}", result.gaps);
        // Complete, and the result says the run failed.
        assert_eq!(result.verdict, RunVerdict::Fail);
    }

    /// The judged verdict is the checks', not the record's.
    #[test]
    fn the_result_carries_the_judged_verdict() {
        let matrix = matrix();
        let mut record = passed(&matrix);
        get(&mut record, "privacy_switch").verdict = Outcome::Skipped;
        record.incomplete.push(Unfinished {
            check: "privacy_switch".into(),
            verdict: Outcome::Skipped,
            reason: "told to".into(),
        });
        record.verdict = RunVerdict::Incomplete;
        assert_eq!(check(&matrix, &record).verdict, RunVerdict::Incomplete);
        // A record that says pass over a failed check is judged a fail.
        let mut record = passed(&matrix);
        get(&mut record, "ring").verdict = Outcome::Fail;
        let result = check(&matrix, &record);
        assert!(!result.complete());
        assert_eq!(result.verdict, RunVerdict::Fail);
    }

    #[test]
    fn a_mandatory_check_missing_skipped_or_not_checked_is_a_gap() {
        let matrix = matrix();
        for verdict in [
            Outcome::NotChecked,
            Outcome::Skipped,
            Outcome::Interrupted,
            Outcome::Missing,
        ] {
            let mut record = passed(&matrix);
            get(&mut record, "privacy_switch").verdict = verdict;
            record.incomplete.push(Unfinished {
                check: "privacy_switch".into(),
                verdict,
                reason: "told to".into(),
            });
            record.verdict = RunVerdict::Incomplete;
            let result = check(&matrix, &record);
            assert_eq!(
                result.gaps,
                vec![Gap::Unfinished {
                    check: "privacy_switch".into(),
                    verdict
                }],
                "{verdict:?}"
            );
        }
        // Left out of the record altogether, and the record still says pass.
        let mut record = passed(&matrix);
        record.checks.retain(|c| c.id != "privacy_switch");
        let gaps = check(&matrix, &record).gaps;
        assert!(gaps.contains(&Gap::Unfinished {
            check: "privacy_switch".into(),
            verdict: Outcome::Missing
        }));
        assert!(gaps.contains(&Gap::Verdict {
            recorded: RunVerdict::Pass,
            judged: RunVerdict::Incomplete
        }));
    }

    #[test]
    fn a_check_that_is_not_mandatory_may_be_left_out_and_is_listed() {
        let matrix = matrix();
        let mut record = passed(&matrix);
        record.checks.retain(|c| c.id != "camera_face_distance");
        let result = check(&matrix, &record);
        assert!(result.complete(), "{:?}", result.gaps);
        assert_eq!(result.not_mandatory_not_judged, ["camera_face_distance"]);
    }

    #[test]
    fn a_record_of_another_or_unapproved_matrix_is_not_complete() {
        let matrix = matrix();
        let edited = Matrix::parse(&format!(
            "{}\n# edited\n",
            SMALL.replace("approved = false", "approved = true")
        ))
        .unwrap();
        let record = passed(&edited);
        assert_eq!(
            check(&matrix, &record).gaps,
            vec![Gap::OtherMatrix {
                version: "7".into()
            }]
        );
        let draft = Matrix::parse(SMALL).unwrap();
        let record = passed(&draft);
        assert_eq!(check(&draft, &record).gaps, vec![Gap::NotApproved]);
        let mut record = passed(&matrix);
        record.suite.matrix_approved = false;
        assert!(check(&matrix, &record).gaps.contains(&Gap::NotApproved));
    }

    #[test]
    fn a_verdict_against_the_limit_is_misjudged() {
        let matrix = matrix();
        // A pass measured over the 85 °C limit.
        let mut record = passed(&matrix);
        get(&mut record, "board_load_temperature")
            .measurement
            .as_mut()
            .unwrap()
            .value = 91.0;
        let gaps = check(&matrix, &record).gaps;
        assert!(
            matches!(&gaps[..], [Gap::Misjudged { check, .. }] if check == "board_load_temperature"),
            "{gaps:?}"
        );
        // A pass with a limit and no measurement.
        let mut record = passed(&matrix);
        get(&mut record, "board_load_temperature").measurement = None;
        assert_eq!(check(&matrix, &record).gaps.len(), 1);
        // A failure for the limit, measured within it.
        let mut record = passed(&matrix);
        let board = get(&mut record, "board_load_temperature");
        board.verdict = Outcome::Fail;
        board.said = Some(Outcome::Pass);
        record.failures.push(Failure {
            check: "board_load_temperature".into(),
            reason: "outside".into(),
            measured: None,
            limit: None,
        });
        record.verdict = RunVerdict::Fail;
        let gaps = check(&matrix, &record).gaps;
        assert!(
            matches!(&gaps[..], [Gap::Misjudged { check, .. }] if check == "board_load_temperature"),
            "{gaps:?}"
        );
        // A limit changed in the record is not the matrix's.
        let mut record = passed(&matrix);
        get(&mut record, "board_load_temperature")
            .limit
            .as_mut()
            .unwrap()
            .max = Some(95.0);
        assert_eq!(
            check(&matrix, &record).gaps,
            vec![Gap::NotAsMatrix("board_load_temperature".into())]
        );
    }

    /// A station's fail recorded as a pass is misjudged, even within its
    /// limit; so is any other `said` the record tool never writes.
    #[test]
    fn a_verdict_changed_from_what_was_said_is_misjudged() {
        let matrix = matrix();
        for (id, verdict, said) in [
            ("ring", Outcome::Pass, Outcome::Fail),
            ("board_load_temperature", Outcome::Pass, Outcome::Fail),
            ("air", Outcome::NotFitted, Outcome::Fail),
            ("ring", Outcome::Pass, Outcome::Pass),
        ] {
            let mut record = passed(&matrix);
            let entry = get(&mut record, id);
            entry.verdict = verdict;
            entry.said = Some(said);
            record.device.hardware_revision = "a-no-air".into();
            let gaps = check(&matrix, &record).gaps;
            assert!(
                matches!(&gaps[..], [Gap::Misjudged { check, .. }] if check == id),
                "{id} {verdict:?} said {said:?}: {gaps:?}"
            );
        }
        // A fail where a skip was said: the matrix never turns a skip into
        // a fail.
        let mut record = passed(&matrix);
        let ring = get(&mut record, "ring");
        ring.verdict = Outcome::Fail;
        ring.said = Some(Outcome::Skipped);
        record.failures.push(Failure {
            check: "ring".into(),
            reason: "dark".into(),
            measured: None,
            limit: None,
        });
        record.verdict = RunVerdict::Fail;
        let gaps = check(&matrix, &record).gaps;
        assert!(
            matches!(&gaps[..], [Gap::Misjudged { check, .. }] if check == "ring"),
            "{gaps:?}"
        );
    }

    /// A measurement in another unit, or in none, is misjudged whatever
    /// its value.
    #[test]
    fn a_measurement_in_another_or_no_unit_is_misjudged() {
        let matrix = matrix();
        for unit in ["°F", ""] {
            let mut record = passed(&matrix);
            get(&mut record, "board_load_temperature")
                .measurement
                .as_mut()
                .unwrap()
                .unit = unit.into();
            let gaps = check(&matrix, &record).gaps;
            assert!(
                matches!(&gaps[..], [Gap::Misjudged { check, why }]
                    if check == "board_load_temperature" && why.contains("measured in")),
                "{unit:?}: {gaps:?}"
            );
        }
    }

    /// At least and at most hold their own value: a pass exactly on the
    /// limit is complete, a fail there for the limit is misjudged.
    #[test]
    fn a_value_exactly_on_a_limit_is_within_it() {
        let at_most = matrix();
        let at_least = Matrix::parse(
            &SMALL
                .replace("approved = false", "approved = true")
                .replace("max = 85.0", "min = 20.0"),
        )
        .unwrap();
        for (matrix, on, past) in [(&at_most, 85.0, 85.5), (&at_least, 20.0, 19.5)] {
            let set = |record: &mut Record, value: f64| {
                get(record, "board_load_temperature")
                    .measurement
                    .as_mut()
                    .unwrap()
                    .value = value;
            };
            let mut record = passed(matrix);
            set(&mut record, on);
            let result = check(matrix, &record);
            assert!(result.complete(), "{on}: {:?}", result.gaps);
            // Just past it, a pass is misjudged.
            set(&mut record, past);
            assert_eq!(check(matrix, &record).gaps.len(), 1, "{past}");
            // On it, a fail for the limit is misjudged.
            let mut record = passed(matrix);
            set(&mut record, on);
            let board = get(&mut record, "board_load_temperature");
            board.verdict = Outcome::Fail;
            board.said = Some(Outcome::Pass);
            record.failures.push(Failure {
                check: "board_load_temperature".into(),
                reason: "outside".into(),
                measured: None,
                limit: None,
            });
            record.verdict = RunVerdict::Fail;
            let gaps = check(matrix, &record).gaps;
            assert!(
                matches!(&gaps[..], [Gap::Misjudged { check, .. }] if check == "board_load_temperature"),
                "{on}: {gaps:?}"
            );
        }
    }

    /// `failures` and `incomplete` are lists: a check twice, or an
    /// unfinished one with another verdict, is a gap.
    #[test]
    fn failures_and_incomplete_are_compared_as_lists() {
        let matrix = matrix();
        let mut record = passed(&matrix);
        get(&mut record, "ring").verdict = Outcome::Fail;
        let failure = Failure {
            check: "ring".into(),
            reason: "dark".into(),
            measured: None,
            limit: None,
        };
        record.failures = vec![failure.clone(), failure];
        record.verdict = RunVerdict::Fail;
        let gaps = check(&matrix, &record).gaps;
        assert!(matches!(&gaps[..], [Gap::Lists(_)]), "{gaps:?}");

        let mut record = passed(&matrix);
        get(&mut record, "privacy_switch").verdict = Outcome::Skipped;
        let unfinished = Unfinished {
            check: "privacy_switch".into(),
            verdict: Outcome::Skipped,
            reason: "told to".into(),
        };
        record.verdict = RunVerdict::Incomplete;
        record.incomplete = vec![unfinished.clone(), unfinished.clone()];
        let gaps = check(&matrix, &record).gaps;
        assert!(
            matches!(&gaps[..], [Gap::Unfinished { .. }, Gap::Lists(_)]),
            "{gaps:?}"
        );
        // Listed once, with another verdict than its check's.
        record.incomplete = vec![Unfinished {
            verdict: Outcome::Interrupted,
            ..unfinished
        }];
        let gaps = check(&matrix, &record).gaps;
        assert!(
            matches!(&gaps[..], [Gap::Unfinished { .. }, Gap::Lists(_)]),
            "{gaps:?}"
        );
    }

    #[test]
    fn not_fitted_follows_the_revision() {
        let matrix = matrix();
        // Revision `a` must have the air sensor.
        let mut record = passed(&matrix);
        get(&mut record, "air").verdict = Outcome::NotFitted;
        assert_eq!(check(&matrix, &record).gaps.len(), 1);
        // `a-no-air` may lack it.
        record.device.hardware_revision = "a-no-air".into();
        let result = check(&matrix, &record);
        assert!(result.complete(), "{:?}", result.gaps);
        record.device.hardware_revision = "b".into();
        assert!(
            check(&matrix, &record)
                .gaps
                .contains(&Gap::UnknownRevision("b".into()))
        );
    }

    #[test]
    fn unknown_doubled_or_unadmitted_checks_and_wrong_lists_are_gaps() {
        let matrix = matrix();
        let mut record = passed(&matrix);
        let ring = record.checks[0].clone();
        record.checks.push(ring);
        let mut unknown = record.checks[1].clone();
        unknown.id = "ring_pixels".into();
        record.checks.push(unknown);
        let endurance = matrix.check("servo_endurance").unwrap();
        record.checks.push(entry(endurance, Outcome::Pass));
        let gaps = check(&matrix, &record).gaps;
        assert!(gaps.contains(&Gap::Twice("ring".into())), "{gaps:?}");
        assert!(gaps.contains(&Gap::Unknown("ring_pixels".into())));
        assert!(gaps.contains(&Gap::NotAdmitted("servo_endurance".into())));
        // A failure listed for a check that passed.
        let mut record = passed(&matrix);
        record.failures.push(Failure {
            check: "ring".into(),
            reason: "dark".into(),
            measured: None,
            limit: None,
        });
        let gaps = check(&matrix, &record).gaps;
        assert!(matches!(&gaps[..], [Gap::Lists(_)]), "{gaps:?}");
    }

    /// A qualification run expects the qualification checks, not the
    /// routine ones.
    #[test]
    fn a_qualification_run_expects_its_own_checks() {
        let matrix = matrix();
        let mut record = passed(&matrix);
        record.occasion = Occasion::Qualification;
        record.checks.clear();
        let gaps = check(&matrix, &record).gaps;
        assert_eq!(
            gaps,
            vec![
                Gap::Unfinished {
                    check: "servo_endurance".into(),
                    verdict: Outcome::Missing
                },
                Gap::Lists(
                    "incomplete lists [], its checks leave [(\"servo_endurance\", Missing)] unfinished"
                        .into()
                ),
                Gap::Verdict {
                    recorded: RunVerdict::Pass,
                    judged: RunVerdict::Incomplete
                },
            ]
        );
        let endurance = matrix.check("servo_endurance").unwrap();
        record.checks.push(entry(endurance, Outcome::Pass));
        let result = check(&matrix, &record);
        assert!(result.complete(), "{:?}", result.gaps);
    }

    /// The repository's matrix, as a draft, is never complete; the same
    /// file approved is, for a run that judged every routine check.
    #[test]
    fn the_repositorys_matrix_is_complete_only_once_approved() {
        let draft = Matrix::parse(V0).unwrap();
        let record = passed(&draft);
        assert_eq!(check(&draft, &record).gaps, vec![Gap::NotApproved]);
        let approved = Matrix::parse(&V0.replace("approved = false", "approved = true")).unwrap();
        let record = passed(&approved);
        let result = check(&approved, &record);
        assert!(result.complete(), "{:?}", result.gaps);
    }
}
