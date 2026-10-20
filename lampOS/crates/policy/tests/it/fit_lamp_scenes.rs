//! Audit one published fit from 89 retained, independently script-labelled
//! played-scene records on lamp-4ace, 2026-10-14. This is host reproduction of
//! abstention, not a deployment, acoustic test or target for future accuracy.
//!
//! If fitting deliberately changes, preserve these recordings, labels, splits
//! and historical fit artifacts. Capture a separately identified new fit and
//! report, then update the audit to compare that version with an explanation.
//! Never alter labels/splits or select thresholds from the held-out test rows
//! merely to make this test pass. Additional report fields are allowed; all
//! recorded fields are checked, with small floating-point platform tolerance.
//!
//! Run with `--nocapture` to retain compact JSON rows, split summaries and the
//! production report. No external script, provider, device or network is used.
use lamp_policy::{
    attention::{self, Answers, Choice, Judgement},
    decide::Decider,
    policy::{
        fit::{self, Options, Row, Split},
        local::Local,
    },
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, io::Cursor, time::Duration};

const FIXTURE: &str = "docs/verification/policy-scenes-lamp-4ace-20261014";
const SCENES: &str =
    include_str!("../../../../docs/verification/policy-scenes-lamp-4ace-20261014/scenes.jsonl");
const WEIGHTS: &str =
    include_str!("../../../../docs/verification/policy-scenes-lamp-4ace-20261014/fit-weights.json");
const REPORT: &str =
    include_str!("../../../../docs/verification/policy-scenes-lamp-4ace-20261014/fit-report.json");
const FLOAT_TOLERANCE: f64 = 1e-10;

/// Counts, strings, booleans and nulls match exactly. Floating calculations
/// may differ in their last bits between supported platform math libraries.
/// Return the largest observed absolute numeric difference for the evidence.
fn matches_recorded(actual: &Value, recorded: &Value, path: &str) -> f64 {
    match (actual, recorded) {
        (Value::Object(actual), Value::Object(recorded)) => recorded
            .iter()
            .map(|(name, expected)| {
                let found = actual
                    .get(name)
                    .unwrap_or_else(|| panic!("missing recorded field {path}.{name}"));
                matches_recorded(found, expected, &format!("{path}.{name}"))
            })
            .fold(0.0, f64::max),
        (Value::Array(actual), Value::Array(recorded)) => {
            assert_eq!(actual.len(), recorded.len(), "{path}");
            actual
                .iter()
                .zip(recorded)
                .enumerate()
                .map(|(index, (found, expected))| {
                    matches_recorded(found, expected, &format!("{path}[{index}]"))
                })
                .fold(0.0, f64::max)
        }
        (Value::Number(actual), Value::Number(recorded))
            if actual.is_f64() && recorded.is_f64() =>
        {
            let (actual, recorded) = (actual.as_f64().unwrap(), recorded.as_f64().unwrap());
            let difference = (actual - recorded).abs();
            let scale = actual.abs().max(recorded.abs()).max(1.0);
            assert!(
                difference <= FLOAT_TOLERANCE * scale,
                "recorded numeric value changed at {path}: {actual} versus {recorded}"
            );
            difference
        }
        _ => {
            assert_eq!(actual, recorded, "recorded value changed at {path}");
            0.0
        }
    }
}

struct Scored<'a> {
    row: &'a Row,
    judgement: Judgement,
}

fn class_summary(rows: &[&Scored<'_>], directed: bool) -> Value {
    let group: Vec<_> = rows
        .iter()
        .filter(|row| row.row.directed == Some(directed))
        .collect();
    let scores: Vec<_> = group.iter().map(|row| row.judgement.probability).collect();
    let count = |choice| {
        group
            .iter()
            .filter(|row| row.judgement.choice == choice)
            .count()
    };
    json!({
        "rows": group.len(),
        "answer": count(Choice::Answer),
        "look_up": count(Choice::LookUp),
        "stay_quiet": count(Choice::StayQuiet),
        "score_min": scores.iter().copied().reduce(f32::min),
        "score_max": scores.iter().copied().reduce(f32::max),
        "score_mean": (!scores.is_empty()).then(|| scores.iter().map(|p| f64::from(*p)).sum::<f64>() / scores.len() as f64)
    })
}

#[test]
fn recorded_played_scenes_reproduce_the_abstaining_local_fit() {
    let rows = fit::read_rows(Cursor::new(SCENES)).unwrap();
    assert_eq!(rows.len(), 89, "this audit identifies one retained dataset");
    let unchanged = serde_json::to_value(&rows).unwrap();
    let result = fit::fit(&rows, &Options::default()).unwrap();
    assert_eq!(serde_json::to_value(&rows).unwrap(), unchanged);
    let actual_weights = serde_json::to_value(&result.weights).unwrap();
    let actual_report = serde_json::to_value(&result.report).unwrap();
    let recorded_weights: Value = serde_json::from_str(WEIGHTS).unwrap();
    let recorded_report: Value = serde_json::from_str(REPORT).unwrap();
    let weights_difference = matches_recorded(&actual_weights, &recorded_weights, "weights");
    let report_difference = matches_recorded(&actual_report, &recorded_report, "report");

    println!(
        "{}",
        json!({
            "event":"fixture_audit", "schema":1, "fixture":FIXTURE,
            "interpretation":"host replay of retained played-scene observations; abstention is not safety or speech evidence",
            "source":result.report.sources, "rights":result.report.rights,
            "answer_from":attention::ANSWER_FROM, "look_up_from":attention::LOOK_UP_FROM,
            "thresholds_fitted":false, "mixture_fitted":false,
            "weights_exact_json_match":actual_weights == recorded_weights,
            "report_exact_json_match":actual_report == recorded_report,
            "float_relative_tolerance_with_unit_floor":FLOAT_TOLERANCE,
            "weights_max_absolute_difference":weights_difference,
            "report_max_absolute_difference":report_difference,
            "os":std::env::consts::OS, "arch":std::env::consts::ARCH
        })
    );

    let mut local = Local::new(result.weights);
    let questions = attention::questions();
    let mut scored = Vec::with_capacity(rows.len());
    for row in &rows {
        let asked = local
            .ask(
                &serde_json::to_string(&row.cues).unwrap(),
                &questions,
                Duration::from_secs(5),
            )
            .unwrap();
        let answers = Answers::of(&asked.answers).unwrap();
        let judgement = local.weights().judge(&answers);
        assert!((0.0..=1.0).contains(&judgement.probability));
        assert!(!judgement.from_rule);
        // These are the model's own raw decisions. No runtime fallback guard,
        // slow judge, mixture or Voice first-output boundary is introduced.
        println!(
            "{}",
            json!({
                "event":"row", "id":row.id, "session":row.session,
                "split":row.split, "source":row.source, "rights":row.rights,
                "cues":row.cues, "labels":row.labels, "directed":row.directed,
                "questions":answers.by_name(), "judgement":judgement
            })
        );
        scored.push(Scored { row, judgement });
    }

    for (split, expected_rows, expected_sessions, expected_directed) in [
        (Split::Train, 35, 8, 10),
        (Split::Calibration, 26, 4, 9),
        (Split::Test, 28, 4, 8),
    ] {
        let group: Vec<_> = scored.iter().filter(|row| row.row.split == split).collect();
        let sessions: BTreeSet<_> = group.iter().map(|row| &row.row.session).collect();
        assert_eq!(group.len(), expected_rows);
        assert_eq!(sessions.len(), expected_sessions);
        assert_eq!(
            group
                .iter()
                .filter(|row| row.row.directed == Some(true))
                .count(),
            expected_directed
        );
        assert!(group.iter().all(|row| row.row.directed.is_some()));
        assert!(
            group
                .iter()
                .all(|row| row.judgement.choice != Choice::Answer)
        );
        assert!(
            group
                .iter()
                .all(|row| row.judgement.probability < attention::ANSWER_FROM)
        );
        let directed = class_summary(&group, true);
        let not_directed = class_summary(&group, false);
        if split == Split::Test {
            assert_eq!(directed["answer"], recorded_report["test"]["answers"]);
            assert_eq!(directed["look_up"], 3);
            assert_eq!(directed["stay_quiet"], 5);
            assert_eq!(
                not_directed["answer"],
                recorded_report["test"]["false_answers"]
            );
            assert_eq!(not_directed["look_up"], 9);
            assert_eq!(not_directed["stay_quiet"], 11);
        }
        println!(
            "{}",
            json!({
                "event":"split", "split":split, "rows":group.len(), "sessions":sessions,
                "directed":directed, "not_directed":not_directed,
                "named_calls":group.iter().filter(|row| row.row.labels.calls_lamp == Some(true)).count(),
                "follow_ups":group.iter().filter(|row| row.row.labels.follows_lamp == Some(true)).count(),
                "near_names":group.iter().filter(|row| row.row.labels.names_someone_else == Some(true)).count()
            })
        );
    }
    println!("{}", json!({"event":"fit_report", "report":actual_report}));
}
