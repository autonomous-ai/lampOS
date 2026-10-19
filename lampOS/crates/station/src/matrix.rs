//! The coverage matrix (`production/matrix-v<N>.toml`, #1700 deliverable 1):
//! every check a run may hold, what kind it is, how it is done, whether it is
//! mandatory, and on which hardware revisions `not_fitted` is fine. Read
//! strictly: an unknown field, kind or method is refused, never ignored.
use crate::{Refused, line, plain, refuse};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, path::Path};

/// What kind of test a check is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Every lamp, every run.
    Routine,
    /// A few units only: it can wear or damage one. Never in a routine run.
    Qualification,
    /// All parts at once (interference), in the routine run.
    Integrated,
}

/// How a check is done, and so where its result comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// Automated, by the lamp: a part of its self-test report.
    Selftest,
    /// Automated, by the station's own fixture: a step in its answers.
    Station,
    /// By a person at the station: a step in its answers.
    Manual,
}

impl Method {
    /// Whether a person does it.
    pub fn automated(self) -> bool {
        self != Method::Manual
    }
}

/// A limit on a measured value: at least `min`, at most `max`, in `unit`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    pub unit: String,
}

impl Limit {
    /// Whether `value` is within it.
    pub fn holds(&self, value: f64) -> bool {
        self.min.is_none_or(|min| value >= min) && self.max.is_none_or(|max| value <= max)
    }

    /// `at least 1 dB`, `at most 85 °C`, `1 to 3 dB`.
    pub fn words(&self) -> String {
        match (self.min, self.max) {
            (Some(min), Some(max)) => format!("{min} to {max} {}", self.unit),
            (Some(min), None) => format!("at least {min} {}", self.unit),
            (None, Some(max)) => format!("at most {max} {}", self.unit),
            (None, None) => format!("any {}", self.unit),
        }
    }
}

/// A hardware revision the matrix knows.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    pub id: String,
    pub what: String,
}

/// One check.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    /// Its name in records; for a self-test check, the part's name in the
    /// lamp's report.
    pub id: String,
    pub part: String,
    pub kind: Kind,
    pub method: Method,
    /// Missing, skipped, interrupted or not checked makes a run of its kind
    /// incomplete.
    pub mandatory: bool,
    /// The revisions on which `not_fitted` is fine; on any other it fails.
    #[serde(default)]
    pub not_fitted_allowed: Vec<String>,
    #[serde(default)]
    pub limit: Option<Limit>,
    pub what: String,
}

/// The suite's version and whether the hardware lead approved it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Suite {
    pub version: String,
    pub approved: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    suite: Suite,
    revision: Vec<Revision>,
    check: Vec<Check>,
}

/// A matrix, checked.
#[derive(Clone, Debug, PartialEq)]
pub struct Matrix {
    pub suite: Suite,
    pub revisions: Vec<Revision>,
    pub checks: Vec<Check>,
    /// SHA-256 of the file's bytes, in hex.
    pub sha256: String,
}

impl Matrix {
    /// Reads and checks the matrix at `path`.
    pub fn read(path: &Path) -> Result<Self, Refused> {
        let text = fs::read_to_string(path)
            .map_err(|error| Refused(format!("{} cannot be read: {error}", path.display())))?;
        Self::parse(&text).map_err(|Refused(why)| Refused(format!("{}: {why}", path.display())))
    }

    /// Parses and checks a matrix's text.
    pub fn parse(text: &str) -> Result<Self, Refused> {
        let file: File =
            toml::from_str(text).map_err(|error| Refused(format!("the matrix: {error}")))?;
        plain("the suite's version", &file.suite.version, 32)?;
        if file.revision.is_empty() {
            refuse!("the matrix names no hardware revision");
        }
        let mut revisions = BTreeSet::new();
        for revision in &file.revision {
            plain("a revision", &revision.id, 64)?;
            line(
                &format!("revision {}'s what", revision.id),
                &revision.what,
                300,
            )?;
            if !revisions.insert(revision.id.as_str()) {
                refuse!("revision {} is named twice", revision.id);
            }
        }
        if file.check.is_empty() {
            refuse!("the matrix has no check");
        }
        let mut ids = BTreeSet::new();
        for check in &file.check {
            if check.id.is_empty()
                || check.id.len() > 64
                || !check
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            {
                refuse!(
                    "check id {:?} is not 1 to 64 lower-case letters, digits or '_'",
                    check.id
                );
            }
            if !ids.insert(check.id.as_str()) {
                refuse!("check {} is listed twice", check.id);
            }
            line(&format!("check {}'s part", check.id), &check.part, 120)?;
            line(&format!("check {}'s what", check.id), &check.what, 300)?;
            for revision in &check.not_fitted_allowed {
                if !revisions.contains(revision.as_str()) {
                    refuse!(
                        "check {} allows not_fitted on revision {revision}, which the matrix does not name",
                        check.id
                    );
                }
            }
            if let Some(limit) = &check.limit {
                if check.method == Method::Selftest {
                    refuse!(
                        "check {} is the self-test's, which judges its own limits",
                        check.id
                    );
                }
                line(&format!("check {}'s unit", check.id), &limit.unit, 16)?;
                let finite = |v: Option<f64>| v.is_none_or(f64::is_finite);
                if (limit.min.is_none() && limit.max.is_none())
                    || !finite(limit.min)
                    || !finite(limit.max)
                    || limit.min.zip(limit.max).is_some_and(|(min, max)| min > max)
                {
                    refuse!(
                        "check {}'s limit needs a finite min, max or both, min not above max",
                        check.id
                    );
                }
            }
        }
        Ok(Self {
            suite: file.suite,
            revisions: file.revision,
            checks: file.check,
            sha256: hex(&Sha256::digest(text.as_bytes())),
        })
    }

    /// The check named `id`.
    pub fn check(&self, id: &str) -> Option<&Check> {
        self.checks.iter().find(|check| check.id == id)
    }

    /// Whether the matrix names the revision `id`.
    pub fn knows_revision(&self, id: &str) -> bool {
        self.revisions.iter().any(|revision| revision.id == id)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The matrix in the repository.
    pub(crate) const V0: &str = include_str!("../../../production/matrix-v0.toml");

    /// A small matrix for the verdict tests: two self-test parts, a manual
    /// step, a measured station step, a non-mandatory one, an optional
    /// part and a qualification check.
    pub(crate) const SMALL: &str = r#"
[suite]
version = "7"
approved = false

[[revision]]
id = "a"
what = "every part"

[[revision]]
id = "a-no-air"
what = "no air sensor"

[[check]]
id = "ring"
part = "ring"
kind = "routine"
method = "selftest"
mandatory = true
what = "colours seen"

[[check]]
id = "touch_by_hand"
part = "touch"
kind = "routine"
method = "selftest"
mandatory = true
what = "felt"

[[check]]
id = "air"
part = "air"
kind = "routine"
method = "selftest"
mandatory = true
not_fitted_allowed = ["a-no-air"]
what = "fresh"

[[check]]
id = "privacy_switch"
part = "privacy switch"
kind = "routine"
method = "manual"
mandatory = true
what = "closes the microphones and the camera"

[[check]]
id = "board_load_temperature"
part = "board"
kind = "routine"
method = "station"
mandatory = true
what = "under load"
limit = { max = 85.0, unit = "°C" }

[[check]]
id = "camera_face_distance"
part = "camera"
kind = "routine"
method = "manual"
mandatory = false
what = "a face at 40 to 80 cm"

[[check]]
id = "servo_endurance"
part = "servos"
kind = "qualification"
method = "station"
mandatory = true
what = "temperature after moves"
"#;

    /// Every part `lamp-selftest` reports today, in its order
    /// (`crates/selftest/src/main.rs`, `listen::checks`, `listen::JOINTS`,
    /// `head::PART`; `touch_by_hand` at the factory only).
    const SELFTEST_PARTS: [&str; 16] = [
        "voice_mic",
        "room_mic",
        "air",
        "touch",
        "board",
        "camera",
        "base_yaw",
        "shoulder",
        "elbow",
        "head_roll",
        "head_pitch",
        "ring",
        "speaker",
        "head_move",
        "storage",
        "touch_by_hand",
    ];

    #[test]
    fn the_repositorys_matrix_is_a_proposal_and_names_every_self_test_part() {
        let matrix = Matrix::parse(V0).unwrap();
        assert_eq!(matrix.suite.version, "0");
        assert!(!matrix.suite.approved, "nothing is approved yet");
        let from_selftest: Vec<&str> = matrix
            .checks
            .iter()
            .filter(|check| check.method == Method::Selftest)
            .map(|check| check.id.as_str())
            .collect();
        let mut expected = SELFTEST_PARTS.to_vec();
        expected.sort_unstable();
        let mut found = from_selftest.clone();
        found.sort_unstable();
        assert_eq!(found, expected);
        for id in &from_selftest {
            let check = matrix.check(id).unwrap();
            assert_eq!(check.kind, Kind::Routine, "{id}");
            assert!(check.mandatory, "{id}");
        }
        // Only the air sensor may be absent, and only where it is not fitted.
        let optional: Vec<(&str, &[String])> = matrix
            .checks
            .iter()
            .filter(|check| !check.not_fitted_allowed.is_empty())
            .map(|check| (check.id.as_str(), check.not_fitted_allowed.as_slice()))
            .collect();
        assert!(
            optional
                .iter()
                .all(|(id, on)| id.starts_with("air") && on == &["draft-a-no-air".to_owned()]),
            "{optional:?}"
        );
        assert!(
            matrix
                .checks
                .iter()
                .any(|check| check.kind == Kind::Qualification && check.mandatory)
        );
        assert_eq!(matrix.sha256.len(), 64);
    }

    #[test]
    fn an_unknown_field_kind_or_method_is_refused() {
        let unknown_top = format!("{SMALL}\n[station]\nid = \"x\"\n");
        assert!(Matrix::parse(&unknown_top).is_err());
        let unknown_in_check = SMALL.replace("what = \"felt\"", "what = \"felt\"\nlimits = 3");
        let why = Matrix::parse(&unknown_in_check).unwrap_err().0;
        assert!(why.contains("unknown field"), "{why}");
        let unknown_in_suite = SMALL.replace("approved = false", "approved = false\nby = \"x\"");
        assert!(Matrix::parse(&unknown_in_suite).is_err());
        let unknown_kind = SMALL.replace("kind = \"qualification\"", "kind = \"endurance\"");
        assert!(Matrix::parse(&unknown_kind).is_err());
        let unknown_method = SMALL.replace("method = \"manual\"", "method = \"by_eye\"");
        assert!(Matrix::parse(&unknown_method).is_err());
        let unknown_in_limit = SMALL.replace("unit = \"°C\" }", "unit = \"°C\", typ = 60.0 }");
        assert!(Matrix::parse(&unknown_in_limit).is_err());
        assert!(Matrix::parse(SMALL).is_ok());
    }

    #[test]
    fn a_matrix_that_contradicts_itself_is_refused() {
        let twice = SMALL.replace("id = \"touch_by_hand\"", "id = \"ring\"");
        assert!(Matrix::parse(&twice).unwrap_err().0.contains("twice"));
        let unknown_revision = SMALL.replace("[\"a-no-air\"]", "[\"b\"]");
        assert!(
            Matrix::parse(&unknown_revision)
                .unwrap_err()
                .0
                .contains("does not name")
        );
        let upside_down = SMALL.replace("max = 85.0", "min = 90.0, max = 85.0");
        assert!(Matrix::parse(&upside_down).is_err());
        let empty_limit = SMALL.replace("max = 85.0, ", "");
        assert!(Matrix::parse(&empty_limit).is_err());
        let selftest_limit = SMALL.replace(
            "what = \"colours seen\"",
            "what = \"colours seen\"\nlimit = { max = 1.0, unit = \"s\" }",
        );
        assert!(Matrix::parse(&selftest_limit).is_err());
        let bad_id = SMALL.replace("id = \"ring\"", "id = \"Ring\"");
        assert!(Matrix::parse(&bad_id).is_err());
        let no_revision = SMALL
            .replace("[[revision]]\nid = \"a\"\nwhat = \"every part\"\n", "")
            .replace(
                "[[revision]]\nid = \"a-no-air\"\nwhat = \"no air sensor\"\n",
                "",
            );
        assert!(Matrix::parse(&no_revision).is_err());
    }

    #[test]
    fn a_limit_holds_its_bounds() {
        let limit = Limit {
            min: Some(1.0),
            max: Some(3.0),
            unit: "dB".into(),
        };
        assert!(limit.holds(1.0) && limit.holds(3.0) && limit.holds(2.0));
        assert!(!limit.holds(0.9) && !limit.holds(3.1) && !limit.holds(f64::NAN));
        assert_eq!(limit.words(), "1 to 3 dB");
        let most = Limit {
            min: None,
            max: Some(85.0),
            unit: "°C".into(),
        };
        assert!(most.holds(-10.0) && !most.holds(85.5));
        assert_eq!(most.words(), "at most 85 °C");
    }

    #[test]
    fn the_hash_changes_with_the_file() {
        let a = Matrix::parse(SMALL).unwrap();
        let b = Matrix::parse(&SMALL.replace("max = 85.0", "max = 95.0")).unwrap();
        assert_eq!(a.suite.version, b.suite.version);
        assert_ne!(a.sha256, b.sha256);
    }
}
