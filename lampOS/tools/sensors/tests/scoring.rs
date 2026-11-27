use lamp_sensor_score::{digest, schema::*, score::score, voice::*};

fn fixture(sensor: Sensor) -> (Manifest, Run) {
    let sessions = (0..12)
        .map(|i| Session {
            id: format!("s{i}"),
            split: Split::Holdout,
            started_unix_s: 10_000 + (i / 6) * 10_000,
            consent: Some(true),
            delete_after_unix_s: Some(100_000),
            retention: None,
            inventory_sha256: None,
            readme_sha256: None,
            data_origin: None,
            clock_verified: None,
            clock_id: format!("clock{i}"),
            sync_error_us: Some(1000),
            recording_sha256: "a".repeat(64),
        })
        .collect();
    let mut trials: Vec<_> = (0..480)
        .map(|i| Trial {
            id: format!("t{i}"),
            session_id: format!("s{}", i % 12),
            speaker_id: format!("p{}", i % 6),
            source: "person".into(),
            start_us: (i + 1) * 10_000_000,
            end_us: (i + 1) * 10_000_000 + 1_000_000,
            call_end_us: Some((i + 1) * 10_000_000 + 1_000_000),
            last_lamp_audio_end_us: Some((i + 1) * 10_000_000 - 1_000_000),
            recipient: if i < 240 { "lamp" } else { "person" }.into(),
            script_id: format!("script{i}"),
            look_target: "lamp".into(),
            head_target: "lamp".into(),
            shoulders_target: None,
            lamp_audio_start_us: None,
            planned_lamp_audio_end_us: None,
            bearing_deg: Some(0.0),
            distance_mm: Some(600),
            light: "daylight".into(),
            noise: "quiet".into(),
            visible_faces: vec!["track1".into()],
            subject_face_id: Some("track1".into()),
            subject_visible: true,
            eyes_epoch: Some("eyes0".into()),
            overlap: false,
            first_use: i < 12,
            condition: match i {
                478 => "slow",
                479 => "disconnected",
                _ => "normal",
            }
            .into(),
        })
        .collect();
    let mut cases = Vec::new();
    match sensor {
        Sensor::VoiceMatch => {
            for i in 0..240 {
                let reference = trials[i].clone();
                let probe = &mut trials[i + 240];
                let same = i < 120;
                probe.session_id = format!("s{}", (i % 6) + 6);
                trials[i].session_id = format!("s{}", i % 6);
                let probe = &mut trials[i + 240];
                probe.speaker_id = if same {
                    reference.speaker_id
                } else {
                    format!("p{}", (i + 1) % 6)
                };
                probe.overlap = i % 120 < 20;
                if i >= 200 {
                    probe.source = "tv".into();
                }
                cases.push(Case {
                    id: format!("c{i}"),
                    sensor,
                    trial_id: probe.id.clone(),
                    reference_trial_id: Some(reference.id),
                    expected: if same { "same" } else { "different" }.into(),
                    fault: false,
                    opportunity_us: None,
                    head_gold_adjudicated: None,
                    observation_end_us: None,
                });
            }
            trials[240].first_use = true;
        }
        _ => {
            for (i, t) in trials.iter_mut().enumerate() {
                let mut fault = false;
                let expected = match sensor {
                    Sensor::Call => {
                        if i < 240 {
                            "call"
                        } else {
                            "non_call"
                        }
                    }
                    Sensor::Addressed => {
                        if i < 240 {
                            "lamp"
                        } else {
                            "room"
                        }
                    }
                    Sensor::Head => {
                        t.head_target = ["lamp", "screen", "phone"][i % 3].into();
                        &t.head_target
                    }
                    Sensor::Direction => {
                        t.bearing_deg = Some(f64::from([-60, -30, 0, 30, 60][(i / 12) % 5]));
                        t.distance_mm = Some([600, 1200][(i / 60) % 2]);
                        t.noise = ["quiet", "background"][(i / 120) % 2].into();
                        if i < 240 {
                            "turn"
                        } else {
                            fault = true;
                            "no_turn"
                        }
                    }
                    Sensor::SpeakingFace => {
                        if i >= 320 {
                            fault = true;
                        }
                        let cell = (i / 40) % 8;
                        t.visible_faces = if cell / 4 == 0 {
                            vec!["track1".into()]
                        } else {
                            vec!["track1".into(), "track2".into()]
                        };
                        t.distance_mm = Some([600, 1200][(cell / 2) % 2]);
                        t.light = ["daylight", "lamplight"][cell % 2].into();
                        match i % 40 {
                            0..=23 if t.visible_faces.len() == 2 && i % 2 == 1 => "track2",
                            0..=23 => "track1",
                            24..=31 => "no_visible",
                            _ => "unknown",
                        }
                    }
                    _ => unreachable!(),
                }
                .to_owned();
                if sensor == Sensor::SpeakingFace {
                    if expected == "no_visible" {
                        t.subject_visible = false;
                        t.subject_face_id = None;
                    } else if t.visible_faces.contains(&expected) {
                        t.subject_face_id = Some(expected.clone());
                    }
                }
                cases.push(Case {
                    id: format!("c{i}"),
                    sensor,
                    trial_id: t.id.clone(),
                    reference_trial_id: None,
                    expected,
                    fault,
                    opportunity_us: (sensor == Sensor::Head).then_some(t.start_us),
                    head_gold_adjudicated: None,
                    observation_end_us: None,
                });
            }
        }
    }
    let manifest = Manifest {
        version: 1,
        rubric_id: None,
        collection_id: "test-only".into(),
        frozen_unix_s: 9000,
        sessions,
        trials,
        cases,
    };
    let predictions = manifest
        .cases
        .iter()
        .map(|c| {
            let t = manifest.trial(&c.trial_id);
            let mouth = sensor == Sensor::SpeakingFace;
            Prediction {
                case_id: c.id.clone(),
                clock_id: manifest.session(&t.session_id).clock_id.clone(),
                output: c.expected.clone(),
                input_end_us: if mouth { t.end_us - 20_000 } else { t.end_us },
                ready_us: Some(if mouth {
                    t.end_us - 1000
                } else {
                    t.end_us + 10_000
                }),
                replay_elapsed_us: None,
                first_admission_us: None,
                asr_final_us: (sensor == Sensor::Addressed).then_some(t.end_us),
                associated_us: mouth.then_some(t.end_us + 1000),
                frame_work_us: mouth.then_some(10_000),
                bearing_deg: t.bearing_deg,
                framed_speaker_id: Some(t.speaker_id.clone()),
                face_in_frame_us: Some(t.end_us + 900_000),
                turns: Some(if c.expected == "no_turn" { 0 } else { 1 }),
                wake_count: Some(0),
                added_wait_us: 0,
                held_line: false,
                safety_violation: false,
                subject_face_id: t.subject_face_id.clone(),
                eyes_epoch: t.eyes_epoch.clone(),
                reason: None,
                natural_motion: Some(true),
                last_lamp_audio_us: None,
                observed_until_us: None,
                playback_stopped: None,
            }
        })
        .collect();
    let hash = digest(&serde_json::to_vec(&manifest).unwrap());
    let run = Run {
        version: 1,
        manifest_sha256: hash,
        sensor,
        build_sha: "b".repeat(40),
        producer_binary_sha256: None,
        source_patch_sha256: None,
        model_sha256: "c".repeat(64),
        config_sha256: "d".repeat(64),
        evidence: "lamp_live".into(),
        location: "local".into(),
        predictions,
    };
    (manifest, run)
}

fn report(m: &Manifest, r: &Run) -> lamp_sensor_score::score::Report {
    score(m, r, &r.manifest_sha256).unwrap()
}

#[test]
fn all_six_perfect_collections_qualify() {
    for sensor in [
        Sensor::Call,
        Sensor::Addressed,
        Sensor::Direction,
        Sensor::Head,
        Sensor::SpeakingFace,
        Sensor::VoiceMatch,
    ] {
        let (m, r) = fixture(sensor);
        let report = report(&m, &r);
        assert!(
            report.qualified,
            "{sensor:?}: {:?}",
            report
                .checks
                .iter()
                .filter(|c| !c.passed)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn call_recall_is_98_not_95_and_missing_stays_in_denominator() {
    let (m, mut r) = fixture(Sensor::Call);
    r.predictions.drain(0..4);
    let result = report(&m, &r);
    assert!(result.qualified);
    assert_eq!(result.counts.total, 480);
    assert_eq!(result.counts.correct, 476);
    r.predictions.remove(0);
    assert!(!report(&m, &r).qualified);
}

#[test]
fn only_synthetic_or_replay_evidence_never_qualifies() {
    let (m, mut r) = fixture(Sensor::Head);
    for evidence in ["synthetic", "replay"] {
        r.evidence = evidence.into();
        if evidence == "replay" {
            for p in &mut r.predictions {
                p.ready_us = None;
                p.associated_us = None;
                p.face_in_frame_us = None;
            }
        }
        assert!(!report(&m, &r).qualified);
    }
}

#[test]
fn replay_has_raw_classification_but_never_manufactures_capture_readiness() {
    let (m, mut r) = fixture(Sensor::Addressed);
    r.evidence = "replay".into();
    // A host duration added to a recorded input timestamp is invalid evidence.
    assert!(score(&m, &r, &r.manifest_sha256).is_err());
    for p in &mut r.predictions {
        p.ready_us = None;
        p.face_in_frame_us = None;
        p.replay_elapsed_us = Some(123_000);
    }
    r.predictions.pop();
    let result = report(&m, &r);
    assert!(!result.qualified);
    assert_eq!(
        result.raw_classification.correct + 1,
        result.raw_classification.total
    );
    assert_eq!(result.counts.correct, 0);
    assert!(result.rows.iter().all(|row| row.latency_us.is_none()
        && row.asr_to_result_us.is_none()
        && row.speech_end_to_result_us.is_none()
        && row.ready_before_first_admission.is_none()));
    assert_eq!(result.rows[0].replay_elapsed_us, Some(123_000));
    r.predictions[0].first_admission_us = Some(m.trials[0].end_us);
    assert!(score(&m, &r, &r.manifest_sha256).is_err());
}

#[test]
fn live_evidence_needs_actual_readiness_and_cannot_use_replay_durations() {
    let (m, mut r) = fixture(Sensor::Addressed);
    let ready = r.predictions[0].ready_us.take();
    assert!(score(&m, &r, &r.manifest_sha256).is_err());
    r.predictions[0].ready_us = ready;
    r.predictions[0].replay_elapsed_us = Some(1000);
    assert!(score(&m, &r, &r.manifest_sha256).is_err());
}

#[test]
fn uncommitted_producer_identity_keeps_commit_binary_and_patch_distinct() {
    let (m, mut r) = fixture(Sensor::Addressed);
    r.source_patch_sha256 = Some("e".repeat(64));
    assert!(score(&m, &r, &r.manifest_sha256).is_err());
    r.producer_binary_sha256 = Some("f".repeat(64));
    let result = report(&m, &r);
    assert_eq!(result.producer_binary_sha256, r.producer_binary_sha256);
    assert_eq!(result.source_patch_sha256, r.source_patch_sha256);
    r.build_sha = "b".repeat(64);
    assert!(score(&m, &r, &r.manifest_sha256).is_err());
    r.build_sha = "b".repeat(40);
    r.producer_binary_sha256 = Some("not-a-hash".into());
    assert!(score(&m, &r, &r.manifest_sha256).is_err());
}

#[test]
fn actual_admission_deadline_is_separate_from_the_100_ms_budget() {
    let (m, mut r) = fixture(Sensor::VoiceMatch);
    for p in &mut r.predictions {
        p.first_admission_us = Some(p.input_end_us + 5_000);
    }
    let result = report(&m, &r);
    assert!(
        result
            .rows
            .iter()
            .all(|row| row.within_admission_budget
                && row.ready_before_first_admission == Some(false))
    );
    for p in &mut r.predictions {
        p.first_admission_us = p.ready_us;
    }
    assert!(
        report(&m, &r)
            .rows
            .iter()
            .all(|row| row.ready_before_first_admission == Some(true))
    );
}

#[test]
fn duplicate_unknown_rows_and_changed_hash_are_invalid() {
    let (m, mut r) = fixture(Sensor::Call);
    r.predictions.push(r.predictions[0].clone());
    assert!(score(&m, &r, &r.manifest_sha256).is_err());
    r.predictions.pop();
    r.predictions[0].case_id = "not-in-manifest".into();
    assert!(score(&m, &r, &r.manifest_sha256).is_err());
    r.predictions[0].case_id = "c0".into();
    assert!(score(&m, &r, &"d".repeat(64)).is_err());
}

#[test]
fn consent_split_and_capture_reuse_are_rejected() {
    let (mut m, _) = fixture(Sensor::Call);
    m.sessions[0].consent = Some(false);
    assert!(m.validate().is_err());
    m.sessions[0].consent = Some(true);
    m.sessions[0].split = Split::Development;
    assert!(m.validate().is_err());
    m.sessions[0].split = Split::Holdout;
    m.trials[12].start_us = m.trials[0].start_us + 100;
    assert!(m.validate().is_err());
}

#[test]
fn direction_requires_joint_success_and_one_turn() {
    let (m, mut r) = fixture(Sensor::Direction);
    for p in r.predictions.iter_mut().take(13) {
        p.turns = Some(2);
    }
    let result = report(&m, &r);
    assert!(!result.qualified);
    assert!(result.rows[0].angle_correct);
    assert!(!result.rows[0].correct);
}

#[test]
fn face_unknown_is_right_only_when_truth_is_unknown_and_cells_gate() {
    let (m, mut r) = fixture(Sensor::SpeakingFace);
    assert!(report(&m, &r).qualified);
    for p in r.predictions.iter_mut().take(3) {
        p.output = "unknown".into();
    }
    let result = report(&m, &r);
    assert!(!result.qualified);
    assert_eq!(result.counts.correct, 317);
    assert!(
        result
            .checks
            .iter()
            .any(|c| c.name == "1face-600mm-daylight" && !c.passed)
    );
}

#[test]
fn asr_latency_is_not_acoustic_latency_and_timeout_is_a_miss() {
    let (m, mut r) = fixture(Sensor::Addressed);
    for p in &mut r.predictions {
        p.input_end_us += 2_000_000;
        p.asr_final_us = Some(p.input_end_us);
        p.ready_us = Some(p.input_end_us + 40_000);
    }
    let result = report(&m, &r);
    assert!(result.qualified);
    assert_eq!(result.timing_us["final_asr_to_result_p95"], Some(40_000));
    assert_eq!(
        result.timing_us["speech_end_to_result_p95"],
        Some(2_040_000)
    );
    r.predictions[0].ready_us = r.predictions[0].ready_us.map(|at| at + 100_000);
    assert!(!report(&m, &r).rows[0].correct);
}

#[test]
fn voice_match_reports_timely_admission_without_waiting_for_late_print() {
    let (m, mut r) = fixture(Sensor::VoiceMatch);
    for p in &mut r.predictions {
        p.ready_us = Some(p.input_end_us + 400_000);
    }
    let result = report(&m, &r);
    assert!(result.qualified);
    assert!(result.rows.iter().all(|r| !r.within_admission_budget));
    r.predictions[0].held_line = true;
    assert!(!report(&m, &r).qualified);
}

#[test]
fn missing_negative_is_not_false_wake_success_and_empty_is_not_pass() {
    let (mut m, mut r) = fixture(Sensor::Call);
    r.predictions.pop();
    let result = report(&m, &r);
    assert_eq!(result.counts.total, 480);
    assert_eq!(result.false_wakes_per_hour, None);
    assert!(!result.qualified);
    m.cases.clear();
    r.predictions.clear();
    assert!(!report(&m, &r).qualified);
}

fn comparison() -> Comparison {
    let mut cases = played_cases();
    for mode in ["live"] {
        for i in 0..40 {
            let (kind, expected) = [
                ("call", "A"),
                ("followup", "A"),
                ("room", "Q"),
                ("stop", "S"),
            ][i / 10];
            cases.push(VoiceCase {
                id: format!("line{i}"),
                group: kind.into(),
                expected: expected.into(),
                kind: kind.into(),
                mode: mode.into(),
            });
        }
    }
    let mut runs = Vec::new();
    for mode in ["played", "live"] {
        for leg in ["A1", "B1", "B2", "A2"] {
            runs.push(VoiceRun {
                leg: leg.into(),
                mode: mode.into(),
                build_sha: if leg.starts_with('A') { "a" } else { "b" }.repeat(40),
                other_config_sha256: "c".repeat(64),
                kit_sha256: "d".repeat(64),
                scene_manifest_sha256: "e".repeat(64),
                sensor_enabled: leg.starts_with('B'),
                safety_violation: false,
                lines: cases
                    .iter()
                    .filter(|c| c.mode == mode)
                    .map(|c| VoiceLine {
                        id: c.id.clone(),
                        observed: c.expected.clone(),
                        heard: true,
                        speech_end_us: 1_000_000,
                        first_audio_us: (c.expected == "A").then_some(1_500_000),
                        interrupt_onset_us: (c.expected == "S").then_some(2_000_000),
                        last_audio_us: (c.expected == "S").then_some(2_100_000),
                    })
                    .collect(),
            });
        }
    }
    Comparison {
        version: 1,
        baseline_build_sha: "a".repeat(40),
        candidate_build_sha: "b".repeat(40),
        changed_switch: "LAMP_TEST_SENSOR".into(),
        kit_sha256: "d".repeat(64),
        kit_reviewed: true,
        scene_manifest_sha256: "e".repeat(64),
        other_config_sha256: "c".repeat(64),
        cases,
        runs,
    }
}

#[test]
fn matched_abba_passes_but_no_group_drop_can_hide_in_total() {
    let mut c = comparison();
    assert!(compare(&c).unwrap().passed);
    c.runs[1].lines[0].observed = "Q".into();
    assert!(!compare(&c).unwrap().passed);
}

#[test]
fn unheard_missing_and_latency_regressions_fail_comparison() {
    let mut c = comparison();
    c.runs[1].lines[20].heard = false;
    assert!(!compare(&c).unwrap().passed);
    c = comparison();
    c.runs[6].lines.pop();
    assert!(!compare(&c).unwrap().passed);
    c = comparison();
    c.runs[1].lines[0].first_audio_us = Some(2_500_000);
    assert!(!compare(&c).unwrap().passed);
}

#[test]
fn incomplete_order_unreviewed_kit_and_mismatched_assets_fail() {
    let mut c = comparison();
    c.runs.swap(1, 2);
    assert!(!compare(&c).unwrap().passed);
    c = comparison();
    c.kit_reviewed = false;
    assert!(!compare(&c).unwrap().passed);
    c = comparison();
    c.runs[1].kit_sha256 = "f".repeat(64);
    assert!(!compare(&c).unwrap().passed);
}

#[test]
fn json_roundtrip_is_strict() {
    let (m, _) = fixture(Sensor::Call);
    let mut value = serde_json::to_value(m).unwrap();
    value["sessions"][0]["raw_audio"] = "not-allowed".into();
    assert!(serde_json::from_value::<Manifest>(value).is_err());
}

#[test]
fn wrong_subject_epoch_and_sampling_are_not_hidden_by_right_class() {
    let (m, mut r) = fixture(Sensor::Head);
    r.predictions[0].subject_face_id = Some("largest-other-face".into());
    let result = report(&m, &r);
    assert!(!result.rows[0].correct);
    assert_eq!(result.rows[0].sampling_delay_us, Some(1_000_000));
    r.predictions[0].subject_face_id = Some("track1".into());
    r.predictions[0].eyes_epoch = Some("previous-process".into());
    assert!(!report(&m, &r).rows[0].correct);
}

#[test]
fn stale_capture_is_a_counted_miss_not_a_discarded_row() {
    let (m, mut r) = fixture(Sensor::Head);
    r.predictions[0].input_end_us = m.trials[0].start_us - 10;
    r.predictions[0].ready_us = Some(m.trials[0].start_us + 10);
    let result = report(&m, &r);
    assert_eq!(result.counts.total, 480);
    assert!(!result.rows[0].correct);
    assert!(!result.rows[0].available);
}

#[test]
fn timestamps_from_another_capture_clock_are_rejected() {
    let (m, mut r) = fixture(Sensor::Head);
    r.predictions[0].clock_id = "another-boot".into();
    assert!(score(&m, &r, &r.manifest_sha256).is_err());
}

#[test]
fn non_call_rejection_boundary_is_95_percent() {
    let (m, mut r) = fixture(Sensor::Call);
    for p in r.predictions.iter_mut().skip(240).take(12) {
        p.output = "call".into();
        p.wake_count = Some(1);
    }
    assert!(report(&m, &r).qualified);
    r.predictions[252].output = "call".into();
    r.predictions[252].wake_count = Some(1);
    assert!(!report(&m, &r).qualified);
}

#[test]
fn a_smaller_played_set_cannot_pass_even_if_every_result_is_right() {
    let mut c = comparison();
    let omitted = c
        .cases
        .iter()
        .find(|c| c.mode == "played")
        .unwrap()
        .id
        .clone();
    c.cases.retain(|c| !(c.mode == "played" && c.id == omitted));
    for run in c.runs.iter_mut().filter(|r| r.mode == "played") {
        run.lines.retain(|line| line.id != omitted);
    }
    assert!(!compare(&c).unwrap().passed);
}

#[test]
fn stable_binary_off_on_test_does_not_require_an_unrelated_build_change() {
    let mut c = comparison();
    c.candidate_build_sha = c.baseline_build_sha.clone();
    for run in &mut c.runs {
        run.build_sha = c.baseline_build_sha.clone();
    }
    assert!(compare(&c).unwrap().passed);
    c.runs[1].sensor_enabled = false;
    assert!(!compare(&c).unwrap().passed);
}

#[test]
fn cli_emits_reports_and_refuses_to_overwrite_them() {
    use std::{fs, process::Command};
    let dir = std::env::temp_dir().join(format!("sensor-score-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let (m, mut r) = fixture(Sensor::Call);
    r.evidence = "synthetic".into();
    fs::write(dir.join("manifest.json"), serde_json::to_vec(&m).unwrap()).unwrap();
    fs::write(dir.join("run.json"), serde_json::to_vec(&r).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_lamp-sensor-score"))
        .arg("score")
        .arg("call")
        .arg(dir.join("manifest.json"))
        .arg(dir.join("run.json"))
        .arg(dir.join("result"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("result/report.json")).unwrap()).unwrap();
    assert_eq!(report["qualified"], false);
    assert_eq!(report["scorer_binary_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(
        fs::read_to_string(dir.join("result/trials.tsv"))
            .unwrap()
            .lines()
            .count(),
        481
    );
    let again = Command::new(env!("CARGO_BIN_EXE_lamp-sensor-score"))
        .arg("score")
        .arg("call")
        .arg(dir.join("manifest.json"))
        .arg(dir.join("run.json"))
        .arg(dir.join("result"))
        .output()
        .unwrap();
    assert_eq!(again.status.code(), Some(2));
    fs::remove_dir_all(dir).unwrap();
}
