//! Independent authored labels exercise fitting mechanics, not lamp accuracy.
use lamp_policy::{
    attention::{self, Answers},
    decide::Decider,
    policy::{
        baseline::{Room, Snapshot, SnapshotKind},
        fit::{self, Options, QuestionLabels, Row, Split},
        local::{Cues, Local, RoomCue, Weights},
    },
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Cursor},
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

fn rows() -> Vec<Row> {
    [Split::Train, Split::Calibration, Split::Test]
        .into_iter()
        .enumerate()
        .flat_map(|(partition, split)| {
            (0..12).map(move |i| {
                let positive = i % 2 == 0;
                Row {
                    schema: 1,
                    id: format!("authored-{partition}-{i}"),
                    session: format!("session-{partition}"),
                    split,
                    source: "authored numerical test, no recordings".into(),
                    rights: "Apache-2.0 project fixture".into(),
                    cues: Cues {
                        conversation: Some(positive),
                        text_called: Some(positive),
                        room: Some(if positive {
                            RoomCue::Quiet
                        } else {
                            RoomCue::Busy
                        }),
                        word_count: Some(if positive { 12 } else { 1 }),
                        ..Cues::default()
                    },
                    labels: QuestionLabels {
                        calls_lamp: Some(positive),
                        others_business: Some(!positive),
                        lamp_could_answer: Some(positive),
                        follows_lamp: Some(positive),
                        listening_noise: Some(!positive),
                        ..QuestionLabels::default()
                    },
                    directed: Some(positive),
                    baseline: (split == Split::Test).then(|| Snapshot {
                        kind: SnapshotKind::SameSnapshot,
                        room: Room::Busy,
                        own_turn: false,
                        going_on: false,
                        named_sure: false,
                        text: if positive {
                            "Lamp, authored call"
                        } else {
                            "authored scrap"
                        }
                        .into(),
                        facing: None,
                    }),
                }
            })
        })
        .collect()
}

fn encode(rows: &[Row]) -> Vec<u8> {
    rows.iter()
        .flat_map(|row| {
            let mut line = serde_json::to_vec(row).unwrap();
            line.push(b'\n');
            line
        })
        .collect()
}

#[test]
fn deterministic_fit_roundtrips_and_never_uses_test_labels_to_fit() {
    let data = rows();
    let first = fit::fit(&data, &Options::default()).unwrap();
    let second = fit::fit(&data, &Options::default()).unwrap();
    assert_eq!(
        serde_json::to_value(&first.weights).unwrap(),
        serde_json::to_value(second.weights).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&first.report).unwrap(),
        serde_json::to_value(second.report).unwrap()
    );
    let decoded = fit::read_rows(Cursor::new(encode(&data))).unwrap();
    assert_eq!(decoded.len(), 36);
    let mut changed = data;
    for row in changed.iter_mut().filter(|r| r.split == Split::Test) {
        row.directed = row.directed.map(|label| !label);
        row.labels.calls_lamp = row.labels.calls_lamp.map(|label| !label);
    }
    let changed = fit::fit(&changed, &Options::default()).unwrap();
    assert_eq!(
        serde_json::to_value(&first.weights).unwrap(),
        serde_json::to_value(changed.weights).unwrap()
    );
    assert_ne!(first.report.test.brier, changed.report.test.brier);
    assert_eq!(first.report.train_rows, 12);
    assert_eq!(first.report.calibration_rows, 12);
    assert_eq!(first.report.test_rows, 12);
    assert_eq!(first.report.train_sessions, 1);
    assert!(
        first.report.test.brier < 0.1,
        "authored separable mechanics fixture"
    );
    assert_eq!(first.report.local_share, 0.5);
    assert_eq!(
        first.report.baseline_source,
        lamp_policy::policy::baseline::SOURCE
    );
}

#[test]
fn unknown_and_single_class_question_labels_become_prior_only_without_feature_effects() {
    let mut data = rows();
    for row in data.iter_mut().filter(|r| r.split == Split::Train) {
        row.labels.asks_to_stop = Some(false);
    }
    let result = fit::fit(&data, &Options::default()).unwrap();
    for index in [1, 6] {
        assert_eq!(
            result.report.question_heads[index].state,
            "prior_only_insufficient_classes"
        );
        let weights = serde_json::to_value(&result.weights).unwrap();
        assert!(
            weights["questions"][index]["head"]["weights"]
                .as_array()
                .unwrap()
                .iter()
                .all(|w| w.as_f64() == Some(0.0))
        );
        assert_eq!(weights["local_choice"]["weights"][index], 0.0);
    }
    assert_eq!(result.report.question_heads[1].unknown_labels, 12);
    assert_eq!(result.report.question_heads[6].negative_labels, 12);
    assert_eq!(
        result.report.test_questions["names_someone_else"].labelled,
        0
    );
    assert_eq!(
        result.report.test_questions["names_someone_else"].brier,
        None
    );
    let mut local = Local::new(result.weights);
    let first = local
        .ask("{}", &attention::questions(), Duration::from_secs(1))
        .unwrap();
    let bias = serde_json::to_value(local.weights()).unwrap()["local_choice"]["bias"]
        .as_f64()
        .unwrap();
    assert_eq!(
        local
            .weights()
            .judge(&Answers::of(&first.answers).unwrap())
            .probability,
        (1.0 / (1.0 + (-bias).exp())) as f32
    );
}

#[test]
fn sessions_ids_labels_sources_and_optimizer_are_checked_before_fitting() {
    let original = rows();
    for mutate in 0..8 {
        let mut data = original.clone();
        match mutate {
            0 => data[12].session = data[0].session.clone(),
            1 => data[1].id = data[0].id.clone(),
            2 => data[12].directed = None,
            3 => data[0].source.clear(),
            4 => data[0].rights = "unbounded".repeat(100),
            5 => data[0].schema = 2,
            6 => data[0].cues.name_score = Some(f32::INFINITY),
            7 => {
                for row in data.iter_mut().filter(|r| r.split == Split::Calibration) {
                    row.directed = Some(true);
                }
            }
            _ => unreachable!(),
        }
        assert!(
            fit::fit(&data, &Options::default()).is_err(),
            "accepted mutation {mutate}"
        );
    }
    assert!(fit::fit(&original[..12], &Options::default()).is_err());
    for options in [
        Options {
            epochs: 0,
            ..Options::default()
        },
        Options {
            epochs: 501,
            ..Options::default()
        },
        Options {
            learning_rate: f64::NAN,
            ..Options::default()
        },
        Options {
            learning_rate: 0.0,
            ..Options::default()
        },
        Options {
            l2: -0.1,
            ..Options::default()
        },
    ] {
        assert!(fit::fit(&original, &options).is_err());
    }
}

#[test]
fn absent_baselines_are_counted_without_inventing_voice_evidence() {
    let mut data = rows();
    for row in &mut data {
        row.baseline = None;
    }
    let result = fit::fit(&data, &Options::default()).unwrap();
    let report = serde_json::to_value(result.report).unwrap();
    assert_eq!(report["baseline"]["missing_snapshot"], 12);
    assert_eq!(report["baseline"]["invalid_snapshot"], 0);
}

#[test]
fn reader_rejects_malformed_oversized_empty_and_unknown_field_rows_without_echoing_text() {
    for bytes in [
        Vec::new(),
        b"\n".to_vec(),
        b"{private-secret".to_vec(),
        vec![b'x'; fit::MOST_LINE_BYTES + 1],
    ] {
        let error = fit::read_rows(Cursor::new(bytes)).unwrap_err();
        assert!(!error.to_string().contains("private-secret"));
    }
    let data = rows();
    let mut first = serde_json::to_value(&data[0]).unwrap();
    first["unknown"] = json!(1);
    assert!(fit::read_rows(Cursor::new(serde_json::to_vec(&first).unwrap())).is_err());
    let mut many = Vec::new();
    for i in 0..=fit::MOST_ROWS {
        let mut row = data[i % 36].clone();
        row.id = format!("id-{i}");
        many.extend(encode(&[row]));
    }
    assert!(fit::read_rows(Cursor::new(many)).is_err());
}

struct Place(PathBuf);
impl Place {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "lamp-fit-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
}
impl Drop for Place {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn command(place: &Place) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_lamp-policy-fit"))
        .arg(place.0.join("rows.jsonl"))
        .arg(place.0.join("weights.json"))
        .arg(place.0.join("report.json"))
        .output()
        .unwrap()
}

#[test]
fn fitter_command_produces_private_loadable_weights_and_report_without_overwriting() {
    let place = Place::new();
    fs::write(place.0.join("rows.jsonl"), encode(&rows())).unwrap();
    let output = command(&place);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let weights = place.0.join("weights.json");
    assert!(Weights::load(&weights).is_ok());
    assert_eq!(
        fs::metadata(&weights).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(place.0.join("report.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let report: Value =
        serde_json::from_slice(&fs::read(place.0.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["test_rows"], 12);
    let before = fs::read(&weights).unwrap();
    assert!(!command(&place).status.success());
    assert_eq!(before, fs::read(weights).unwrap());
    fs::remove_file(place.0.join("weights.json")).unwrap();
    fs::remove_file(place.0.join("report.json")).unwrap();
    fs::write(place.0.join("rows.jsonl"), b"private-invalid-content").unwrap();
    let refused = command(&place);
    assert!(!refused.status.success());
    assert!(!String::from_utf8_lossy(&refused.stderr).contains("private-invalid-content"));
    assert!(!place.0.join("weights.json").exists());
    assert!(!place.0.join("report.json").exists());
}

#[test]
fn fit_export_paths_refuse_symlinks_and_input_overwrite() {
    let place = Place::new();
    let input = place.0.join("rows.jsonl");
    let original = encode(&rows());
    fs::write(&input, &original).unwrap();
    std::os::unix::fs::symlink(&input, place.0.join("weights.json")).unwrap();
    assert!(!command(&place).status.success());
    assert_eq!(fs::read(&input).unwrap(), original);
    assert!(!place.0.join("report.json").exists());
    let error = fit::read_rows(Cursor::new(b"{}".to_vec())).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}
