//! What the station gives the tool for one run, read strictly: any field
//! the format does not have is refused, never dropped.
//!
//! - [`selftest`]: the lamp's own report (`/var/lib/lamp-selftest/latest.json`,
//!   `lamp_msgs::SelfTest`), copied off the lamp by the station.
//! - [`Answers`]: the station's own account of the run: when it started and
//!   ended, and each station or manual step with its verdict, its
//!   measurement if any, and its UTC time; for a retest, the repairs made
//!   since the run it retests.
use crate::{Refused, Utc};
use lamp_msgs::{KeyValue, Occasion as SelfTestOccasion, PartResult, SelfTest, Verdict};
use serde::{Deserialize, Serialize};

/// The self-test's report as `lamp_msgs::SelfTest` writes it, field for
/// field, with nothing more allowed. Kept beside it by the struct literals
/// in [`selftest`]: a field added there no longer builds here.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictSelfTest {
    occasion: SelfTestOccasion,
    started: f64,
    took: f64,
    verdict: Verdict,
    parts: Vec<StrictPart>,
    #[serde(default)]
    build: Option<String>,
    /// Absent from a report made before reports named their lamp.
    #[serde(default)]
    lamp: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictPart {
    part: String,
    verdict: Verdict,
    reason: String,
    #[serde(default)]
    values: Vec<Reading>,
    took: f64,
}

/// One reading beside a part's verdict, both sides text, as the self-test
/// gives them (`lamp_msgs::KeyValue`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reading {
    pub key: String,
    pub value: String,
}

/// Reads the lamp's self-test report: refused if it has a field the report
/// does not, or if it fails the report's own check (no part, a verdict that
/// is not the worst of its parts, a fail with no reason).
pub fn selftest(text: &str) -> Result<SelfTest, Refused> {
    let strict: StrictSelfTest = serde_json::from_str(text)
        .map_err(|error| Refused(format!("the self-test report: {error}")))?;
    let test = SelfTest {
        occasion: strict.occasion,
        started: strict.started,
        took: strict.took,
        verdict: strict.verdict,
        parts: strict
            .parts
            .into_iter()
            .map(|part| PartResult {
                part: part.part,
                verdict: part.verdict,
                reason: part.reason,
                values: part
                    .values
                    .into_iter()
                    .map(|Reading { key, value }| KeyValue { key, value })
                    .collect(),
                took: part.took,
            })
            .collect(),
        build: strict.build,
        lamp: strict.lamp,
    };
    test.check()
        .map_err(|why| Refused(format!("the self-test report: {why}")))?;
    Ok(test)
}

/// A station or manual step's verdict, as the station gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Answered {
    Pass,
    Fail,
    /// This lamp has no such part.
    NotFitted,
    /// It could not be checked (a fixture missing, say).
    NotChecked,
    /// The operator left it out.
    Skipped,
    /// It began and did not finish (the operator's stop, a power cut).
    Interrupted,
}

/// A measured value and its unit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measurement {
    pub value: f64,
    pub unit: String,
}

/// One station or manual step.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// The matrix's check id.
    pub check: String,
    pub verdict: Answered,
    #[serde(default)]
    pub measurement: Option<Measurement>,
    /// Why, in one line: needed for anything but a pass. Never a person's
    /// name (docs/production-testing.md, "Never in a record").
    #[serde(default)]
    pub reason: Option<String>,
    /// When the step was done.
    pub at: Utc,
}

/// What was done to the lamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// A part taken out and a new one put in.
    Replaced,
    /// A part or cable taken out and put back.
    Reseated,
    /// Soldering or other work on a part.
    Reworked,
    /// A setting or a mechanical adjustment.
    Adjusted,
}

/// A repair made since the run a retest retests.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repair {
    pub at: Utc,
    /// The check whose failure it answers.
    pub check: String,
    pub action: Action,
    /// What was done, in one line (`the Jieli microphone replaced`). Never a
    /// person's name, a supplier or a price.
    pub detail: String,
}

/// The station's answers for one run.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answers {
    /// When the run began and ended, by the station's clock.
    pub started: Utc,
    pub ended: Utc,
    pub steps: Vec<Step>,
    #[serde(default)]
    pub repairs: Vec<Repair>,
}

impl Answers {
    pub fn parse(text: &str) -> Result<Self, Refused> {
        serde_json::from_str(text)
            .map_err(|error| Refused(format!("the station's answers: {error}")))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// lamp-75c9's on-demand run of 2026-10-16, as the lamp wrote it.
    const LAMP_75C9: &str = include_str!(
        "../../../docs/verification/selftest-lamp-75c9-20261016/runs/on-demand/latest.json"
    );
    const LAMP_75C9_FIRST_BOOT: &str = include_str!(
        "../../../docs/verification/selftest-lamp-75c9-20261016/runs/first-boot/latest.json"
    );

    #[test]
    fn a_lamps_own_report_is_read_as_written() {
        for text in [LAMP_75C9, LAMP_75C9_FIRST_BOOT] {
            let test = selftest(text).unwrap();
            assert_eq!(test.verdict, Verdict::NotChecked);
            assert!(test.parts.iter().any(|part| part.part == "storage"));
            // Made before reports named their lamp.
            assert_eq!(test.lamp, None);
        }
        let mut value: serde_json::Value = serde_json::from_str(LAMP_75C9).unwrap();
        value["lamp"] = "lamp-75c9".into();
        let test = selftest(&value.to_string()).unwrap();
        assert_eq!(test.lamp.as_deref(), Some("lamp-75c9"));
        value["lamp"] = "lamp 75c9".into();
        assert!(selftest(&value.to_string()).is_err());
    }

    #[test]
    fn a_report_with_a_field_it_does_not_have_is_refused() {
        let mut value: serde_json::Value = serde_json::from_str(LAMP_75C9).unwrap();
        value["device"] = "lamp-75c9".into();
        let why = selftest(&value.to_string()).unwrap_err().0;
        assert!(why.contains("unknown field `device`"), "{why}");
        let mut value: serde_json::Value = serde_json::from_str(LAMP_75C9).unwrap();
        value["parts"][0]["limit"] = (-90).into();
        let why = selftest(&value.to_string()).unwrap_err().0;
        assert!(why.contains("unknown field `limit`"), "{why}");
        let mut value: serde_json::Value = serde_json::from_str(LAMP_75C9).unwrap();
        value["parts"][0]["values"] = serde_json::json!([{"key": "a", "value": "1", "unit": "x"}]);
        assert!(selftest(&value.to_string()).is_err());
    }

    #[test]
    fn a_report_that_fails_its_own_check_is_refused() {
        let mut value: serde_json::Value = serde_json::from_str(LAMP_75C9).unwrap();
        value["verdict"] = "pass".into();
        let why = selftest(&value.to_string()).unwrap_err().0;
        assert!(why.contains("worst"), "{why}");
        let mut value: serde_json::Value = serde_json::from_str(LAMP_75C9).unwrap();
        value["verdict"] = "great".into();
        assert!(selftest(&value.to_string()).is_err());
        assert!(selftest("{}").is_err());
    }

    pub(crate) const ANSWERS: &str = r#"{
        "started": "2026-10-19T08:00:00Z",
        "ended": "2026-10-19T08:30:00Z",
        "steps": [
            {"check": "privacy_switch", "verdict": "pass", "at": "2026-10-19T08:10:00Z"},
            {"check": "board_load_temperature", "verdict": "pass",
             "measurement": {"value": 71.5, "unit": "°C"}, "at": "2026-10-19T08:20:00Z"}
        ]
    }"#;

    #[test]
    fn the_stations_answers_are_read_strictly() {
        let answers = Answers::parse(ANSWERS).unwrap();
        assert_eq!(answers.steps.len(), 2);
        assert_eq!(answers.steps[1].measurement.as_ref().unwrap().value, 71.5);
        assert!(answers.repairs.is_empty());
        for (from, to) in [
            ("\"steps\"", "\"operator_name\": \"x\", \"steps\""),
            (
                "\"verdict\": \"pass\", \"at\"",
                "\"verdict\": \"pass\", \"by\": \"x\", \"at\"",
            ),
            ("\"unit\": \"°C\"}", "\"unit\": \"°C\", \"raw\": 1}"),
            (
                "\"verdict\": \"pass\", \"at\"",
                "\"verdict\": \"ok\", \"at\"",
            ),
            ("08:10:00Z", "15:10:00+07:00"),
        ] {
            assert!(ANSWERS.contains(from), "{from}");
            let text = ANSWERS.replacen(from, to, 1);
            assert!(Answers::parse(&text).is_err(), "{to}");
        }
        let repair = ANSWERS.replace(
            "\"steps\"",
            "\"repairs\": [{\"at\": \"2026-10-19T07:00:00Z\", \"check\": \"ring\", \"action\": \"reseated\", \"detail\": \"ring cable\", \"cost\": 3}], \"steps\"",
        );
        assert!(
            Answers::parse(&repair)
                .unwrap_err()
                .0
                .contains("unknown field `cost`")
        );
    }
}
