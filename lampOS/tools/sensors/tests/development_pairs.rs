use lamp_sensor_score::{digest, schema::*, score};
use serde_json::json;
use std::{fs, process::Command};

// Invented source segments only; this tests encoding, not a measured corpus.
fn fixture() -> (Manifest, Run) {
    let sources = [
        ("ref-a", "speaker-a"),
        ("ref-b", "speaker-b"),
        ("probe", "speaker-a"),
    ];
    let sessions: Vec<_> = sources.iter().map(|(id,_)| json!({
        "id":id,"split":"development","started_unix_s":null,"consent":null,
        "retention":"keep_all","inventory_sha256":"1".repeat(64),"readme_sha256":"2".repeat(64),
        "data_origin":"invented","clock_id":id,"clock_verified":false,"sync_error_us":null,
        "recording_sha256":digest(id.as_bytes())
    })).collect();
    let trials: Vec<_> = sources.iter().map(|(id,speaker)| json!({
        "id":id,"session_id":id,"speaker_id":speaker,"source":"person","start_us":0,"end_us":1_000_000,
        "recipient":"unknown","script_id":id,"look_target":"unknown","head_target":"unknown",
        "bearing_deg":null,"distance_mm":null,"light":"unknown","noise":"unknown",
        "visible_faces":[],"subject_visible":false,"overlap":false,"first_use":false,"condition":"normal"
    })).collect();
    let cases = json!([
        {"id":"same-pair","sensor":"voice_match","trial_id":"probe","reference_trial_id":"ref-a","expected":"same","fault":false},
        {"id":"different-pair","sensor":"voice_match","trial_id":"probe","reference_trial_id":"ref-b","expected":"different","fault":false}
    ]);
    let m: Manifest = serde_json::from_value(json!({"version":2,"rubric_id":V2_RUBRIC,
        "evaluation_mode":"raw_development_pairs","collection_id":"invented-pairs","frozen_unix_s":1000,
        "sessions":sessions,"trials":trials,"cases":cases})).unwrap();
    let predictions: Vec<_> = m
        .cases
        .iter()
        .map(|c| {
            json!({"case_id":c.id,"clock_id":"probe","output":c.expected,
        "input_end_us":1_000_000,"ready_us":null,"replay_elapsed_us":10_000,
        "added_wait_us":0,"held_line":false,"safety_violation":false})
        })
        .collect();
    let r: Run = serde_json::from_value(json!({"version":2,"manifest_sha256":digest(&serde_json::to_vec(&m).unwrap()),
        "sensor":"voice_match","build_sha":"b".repeat(40),"producer_binary_sha256":"c".repeat(64),
        "model_sha256":"d".repeat(64),"config_sha256":"e".repeat(64),"evidence":"replay","location":"local",
        "predictions":predictions})).unwrap();
    (m, r)
}
fn report(m: &Manifest, r: &Run) -> score::Report {
    let mut r = r.clone();
    r.manifest_sha256 = digest(&serde_json::to_vec(m).unwrap());
    score::score(m, &r, &r.manifest_sha256).unwrap()
}

#[test]
fn reused_probe_is_two_correlated_pairs_and_zero_holdout_credit() {
    let (m, r) = fixture();
    let scored = report(&m, &r);
    assert!(!scored.qualified);
    assert_eq!(scored.counts.total, 0);
    assert!(
        scored
            .checks
            .iter()
            .any(|c| c.name == "qualification_evaluation_mode" && !c.passed)
    );
    let raw = &scored.raw_by_split.as_ref().unwrap()["development"];
    assert_eq!((raw.counts.correct, raw.counts.total), (2, 2));
    assert_eq!(raw.metadata_unknown["started_unix_s"], 2);
    let coverage = scored.development_pairs.unwrap();
    assert!(coverage.correlated_pairs);
    assert_eq!(coverage.planned_pairs, 2);
    assert_eq!(coverage.unique_probe_segments, 1);
    assert_eq!(coverage.unique_reference_segments, 2);
    assert_eq!(coverage.unique_source_segments, 3);
    assert_eq!(coverage.unique_recordings, 3);
    assert_eq!(coverage.reused_probe_segments, 1);
    assert_eq!(coverage.additional_pairs_on_reused_probes, 1);
    assert_eq!(coverage.max_pairs_per_probe, 2);
    assert_eq!(coverage.source_session_starts_unknown, 3);
    assert!(
        scored
            .rows
            .iter()
            .all(|row| row.reference_trial_id.is_some() && row.latency_us.is_none())
    );
}

#[test]
fn missing_and_unknown_predictions_keep_the_planned_pair_denominator() {
    let (m, mut r) = fixture();
    r.predictions.pop();
    r.predictions[0].output = "unknown".into();
    let scored = report(&m, &r);
    let raw = &scored.raw_by_split.as_ref().unwrap()["development"];
    assert_eq!(
        (raw.counts.correct, raw.counts.total, raw.counts.available),
        (0, 2, 1)
    );
    assert_eq!(scored.development_pairs.unwrap().planned_pairs, 2);
}

#[test]
fn duplicate_ordered_source_pair_is_rejected_even_with_new_case_id() {
    let (mut m, _) = fixture();
    let mut case = m.cases[0].clone();
    case.id = "duplicate".into();
    m.cases.push(case);
    assert!(m.validate().is_err());
}

fn alias_reference(m: &mut Manifest, conflicting_label: bool) {
    let mut session = m.sessions[0].clone();
    session.id = "alias".into();
    session.clock_id = "alias".into();
    session.recording_sha256 = session.recording_sha256.to_uppercase();
    m.sessions.push(session);
    let mut trial = m.trials[0].clone();
    trial.id = "alias".into();
    trial.session_id = "alias".into();
    if conflicting_label {
        trial.speaker_id = "speaker-c".into();
    }
    m.trials.push(trial);
    let mut case = m.cases[0].clone();
    case.id = "alias-pair".into();
    case.reference_trial_id = Some("alias".into());
    if conflicting_label {
        case.expected = "different".into();
    }
    m.cases.push(case);
}

#[test]
fn byte_hash_case_and_trial_aliases_cannot_duplicate_a_source_pair() {
    let (mut m, _) = fixture();
    alias_reference(&mut m, false);
    assert!(m.validate().is_err());
}

#[test]
fn one_source_segment_cannot_have_conflicting_speaker_labels() {
    let (mut m, _) = fixture();
    alias_reference(&mut m, true);
    assert!(m.validate().unwrap_err().contains("conflicting"));
}

#[test]
fn reference_and_probe_cannot_share_audio_samples() {
    let (mut m, _) = fixture();
    m.sessions[2].recording_sha256 = m.sessions[0].recording_sha256.to_uppercase();
    assert!(m.validate().is_err());
    m.trials[2].start_us = 500_000;
    m.trials[2].end_us = 1_500_000;
    assert!(m.validate().is_err());
    m.trials[2].start_us = 1_000_000;
    m.trials[2].end_us = 2_000_000;
    m.validate().unwrap(); // Adjacent, disjoint segments still are correlated development data.
}

#[test]
fn ordered_role_reversal_is_visible_without_becoming_an_independent_episode() {
    let (mut m, _) = fixture();
    let mut reverse = m.cases[0].clone();
    reverse.id = "reverse".into();
    reverse.trial_id = "ref-a".into();
    reverse.reference_trial_id = Some("probe".into());
    m.cases.push(reverse);
    m.validate().unwrap();
}

#[test]
fn raw_mode_cannot_include_holdout_real_people_other_sensors_or_fault_exclusions() {
    let (m, _) = fixture();
    let mut holdout = m.clone();
    holdout.sessions[0].split = Split::Holdout;
    assert!(holdout.validate().is_err());
    let mut real = m.clone();
    real.sessions[0].data_origin = Some(DataOrigin::RealPeople);
    assert!(real.validate().is_err());
    let mut other = m.clone();
    other.cases[0].sensor = Sensor::Call;
    assert!(other.validate().is_err());
    let mut fault = m.clone();
    fault.cases[0].fault = true;
    assert!(fault.validate().is_err());
    let mut legacy = m.clone();
    legacy.version = 1;
    legacy.rubric_id = None;
    assert!(legacy.validate().is_err());
    let mut typo = m;
    typo.evaluation_mode = Some("raw-pairs".into());
    assert!(typo.validate().is_err());
}

#[test]
fn raw_mode_requires_replay_and_exact_source_end_without_live_readiness() {
    let (m, r) = fixture();
    let mut live = r.clone();
    live.evidence = "lamp_live".into();
    assert!(live.validate(&m, &r.manifest_sha256).is_err());
    let mut readiness = r.clone();
    readiness.predictions[0].ready_us = Some(1_001_000);
    assert!(readiness.validate(&m, &r.manifest_sha256).is_err());
    let mut end = r.clone();
    end.predictions[0].input_end_us += 1;
    assert!(end.validate(&m, &r.manifest_sha256).is_err());
    let mut admission = r.clone();
    admission.predictions[0].first_admission_us = Some(1_001_000);
    assert!(admission.validate(&m, &r.manifest_sha256).is_err());
}

#[test]
fn normal_mode_keeps_one_probe_two_hour_and_call_reference_rules() {
    let (mut m, _) = fixture();
    m.evaluation_mode = None;
    for session in &mut m.sessions {
        session.started_unix_s = Some(if session.id == "probe" { 9000 } else { 1000 });
    }
    for trial in &mut m.trials {
        trial.recipient = "lamp".into();
        trial.call_end_us = Some(trial.end_us);
    }
    assert!(m.validate().unwrap_err().contains("sensor trial"));
    m.cases.pop();
    m.validate().unwrap();
    m.sessions[2].started_unix_s = Some(1000);
    assert!(m.validate().is_err());
    m.sessions[2].started_unix_s = Some(9000);
    m.trials[0].recipient = "unknown".into();
    assert!(m.validate().is_err());
}

#[test]
fn cli_scores_real_encoding_as_raw_only_and_preserves_reference_join() {
    let (m, r) = fixture();
    let dir = std::env::temp_dir().join(format!(
        "sensor-pairs-cli-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    let manifest = dir.join("manifest.json");
    let run = dir.join("run.json");
    fs::write(&manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    fs::write(&run, serde_json::to_vec(&r).unwrap()).unwrap();
    let out = dir.join("score");
    let result = Command::new(env!("CARGO_BIN_EXE_lamp-sensor-score"))
        .args([
            "score",
            "voice_match",
            manifest.to_str().unwrap(),
            run.to_str().unwrap(),
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(out.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["evaluation_mode"], "raw_development_pairs");
    assert_eq!(report["qualified"], false);
    assert_eq!(report["development_pairs"]["planned_pairs"], 2);
    let tsv = fs::read_to_string(out.join("trials.tsv")).unwrap();
    assert!(tsv.contains("reference_trial_id"));
}
