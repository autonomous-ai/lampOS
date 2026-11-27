use lamp_sensor_score::{
    digest,
    schema::*,
    score::{Report, score},
};

// Invented numeric fixtures exercise the measuring code; they are not EVAL data.
fn fixture(sensor: Sensor) -> (Manifest, Run) {
    let sessions = (0..12)
        .map(|i| Session {
            id: format!("s{i}"),
            split: Split::Holdout,
            started_unix_s: 10_000 + (i / 6) * 10_000,
            consent: Some(true),
            delete_after_unix_s: None,
            retention: Some("keep_all".into()),
            inventory_sha256: Some("1".repeat(64)),
            readme_sha256: Some("2".repeat(64)),
            data_origin: Some(DataOrigin::RealPeople),
            clock_verified: Some(true),
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
            visible_faces: vec!["track1".into(), "track2".into()],
            subject_face_id: Some("track1".into()),
            subject_visible: true,
            eyes_epoch: Some("eyes0".into()),
            overlap: false,
            first_use: i < 12 || i == 240,
            condition: match i {
                318 | 478 => "slow",
                319 | 479 => "disconnected",
                _ => "normal",
            }
            .into(),
        })
        .collect();
    let mut cases = Vec::new();
    if sensor == Sensor::VoiceMatch {
        for i in 0..240 {
            trials[i].session_id = format!("s{}", i % 6);
            let reference = trials[i].clone();
            let t = &mut trials[i + 240];
            t.session_id = format!("s{}", i % 6 + 6);
            t.speaker_id = if i < 120 {
                reference.speaker_id
            } else {
                format!("p{}", (i + 1) % 6)
            };
            t.overlap = i % 120 < 20;
            if i >= 200 {
                t.source = "tv".into();
            }
            cases.push(Case {
                id: format!("c{i}"),
                sensor,
                trial_id: t.id.clone(),
                reference_trial_id: Some(reference.id),
                expected: if i < 120 { "same" } else { "different" }.into(),
                fault: false,
                opportunity_us: None,
                head_gold_adjudicated: None,
                observation_end_us: None,
            });
        }
    } else {
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
                    t.shoulders_target = Some("lamp".into());
                    if i < 240 {
                        "facing"
                    } else {
                        if i % 2 == 0 {
                            t.head_target = "away".into();
                        } else {
                            t.shoulders_target = Some("away".into());
                        }
                        "away"
                    }
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
                    let cell = (i / 80) % 4;
                    t.distance_mm = Some([600, 1200][cell / 2]);
                    t.light = ["daylight", "lamplight"][cell % 2].into();
                    if i >= 400 {
                        t.visible_faces.truncate(1);
                        "track1"
                    } else if i >= 320 {
                        fault = true;
                        "unknown"
                    } else if i % 80 >= 64 {
                        t.subject_visible = false;
                        t.subject_face_id = None;
                        "no_visible"
                    } else if i % 2 == 0 {
                        "track1"
                    } else {
                        t.subject_face_id = Some("track2".into());
                        "track2"
                    }
                }
                Sensor::BargeIn => {
                    t.overlap = true;
                    t.lamp_audio_start_us = Some(t.start_us - 1_000_000);
                    t.planned_lamp_audio_end_us = Some(t.start_us + 3_000_000);
                    if i >= 360 {
                        t.source = "noise".into();
                    }
                    ["interrupt", "backchannel", "side_talk", "noise"][i / 120]
                }
                Sensor::VoiceMatch => unreachable!(),
            };
            cases.push(Case {
                id: format!("c{i}"),
                sensor,
                trial_id: t.id.clone(),
                reference_trial_id: None,
                expected: expected.into(),
                fault,
                opportunity_us: (sensor == Sensor::Head).then_some(t.start_us),
                head_gold_adjudicated: None,
                observation_end_us: (sensor == Sensor::BargeIn).then_some(t.start_us + 3_500_000),
            });
        }
    }
    let manifest = Manifest {
        version: 2,
        rubric_id: Some(V2_RUBRIC.into()),
        collection_id: "invented-v2".into(),
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
            let barge = sensor == Sensor::BargeIn;
            let interrupt = c.expected == "interrupt";
            let input = if mouth {
                t.end_us - 20_000
            } else if barge {
                t.start_us + 100_000
            } else {
                t.end_us
            };
            Prediction {
                case_id: c.id.clone(),
                clock_id: manifest.session(&t.session_id).clock_id.clone(),
                output: if barge {
                    if interrupt { "stop" } else { "continue" }.into()
                } else {
                    c.expected.clone()
                },
                input_end_us: input,
                ready_us: Some(if mouth {
                    t.end_us - 1000
                } else {
                    input + 10_000
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
                last_lamp_audio_us: barge
                    .then_some(t.start_us + if interrupt { 300_000 } else { 3_000_000 }),
                observed_until_us: c.observation_end_us,
                playback_stopped: barge.then_some(interrupt),
            }
        })
        .collect();
    let run = Run {
        version: 2,
        manifest_sha256: digest(&serde_json::to_vec(&manifest).unwrap()),
        sensor,
        build_sha: "b".repeat(40),
        producer_binary_sha256: Some("c".repeat(64)),
        source_patch_sha256: None,
        model_sha256: "d".repeat(64),
        config_sha256: "e".repeat(64),
        evidence: "lamp_live".into(),
        location: "local".into(),
        predictions,
    };
    (manifest, run)
}

fn report(m: &Manifest, r: &Run) -> Report {
    // Re-hash changed labels explicitly; the run must bind the exact manifest.
    let mut r = r.clone();
    r.manifest_sha256 = digest(&serde_json::to_vec(m).unwrap());
    score(m, &r, &r.manifest_sha256).unwrap()
}

#[test]
fn all_seven_current_rubrics_have_positive_qualification_examples() {
    for sensor in [
        Sensor::Call,
        Sensor::Addressed,
        Sensor::Direction,
        Sensor::Head,
        Sensor::SpeakingFace,
        Sensor::VoiceMatch,
        Sensor::BargeIn,
    ] {
        let (m, r) = fixture(sensor);
        let result = report(&m, &r);
        assert!(
            result.qualified,
            "{sensor:?}: {:?}",
            result
                .checks
                .iter()
                .filter(|c| !c.passed)
                .collect::<Vec<_>>()
        );
        assert_eq!(result.version, 2);
        assert_eq!(result.rubric_id.as_deref(), Some(V2_RUBRIC));
    }
}

fn external_replay() -> (Manifest, Run) {
    let (mut m, mut r) = fixture(Sensor::SpeakingFace);
    for session in &mut m.sessions {
        session.split = Split::Development;
        session.data_origin = Some(DataOrigin::Public);
        session.consent = None;
        session.clock_verified = Some(false);
        session.sync_error_us = None;
    }
    for trial in &mut m.trials {
        trial.bearing_deg = None;
        trial.distance_mm = None;
        trial.light = "unknown".into();
        trial.noise = "unknown".into();
    }
    r.evidence = "replay".into();
    for prediction in &mut r.predictions {
        prediction.ready_us = None;
        prediction.associated_us = None;
        prediction.first_admission_us = None;
        prediction.face_in_frame_us = None;
        prediction.replay_elapsed_us = Some(1_000);
    }
    (m, r)
}

#[test]
fn external_development_accepts_unknown_metadata_without_invented_values() {
    for origin in [
        DataOrigin::Public,
        DataOrigin::Generated,
        DataOrigin::Invented,
    ] {
        let (mut m, r) = external_replay();
        for session in &mut m.sessions {
            session.data_origin = Some(origin);
        }
        m.validate().unwrap();
        let scored = report(&m, &r);
        assert!(!scored.qualified);
        assert_eq!(scored.counts.total, 0);
        let raw = &scored.raw_by_split.as_ref().unwrap()["development"];
        assert_eq!(raw.counts.total, 320);
        assert_eq!(raw.counts.correct, 320);
        for field in [
            "distance_mm",
            "bearing_deg",
            "light",
            "noise",
            "sync_error_us",
            "collection_consent",
        ] {
            assert_eq!(raw.metadata_unknown[field], 480);
        }
        assert!(scored.rows.iter().all(|row| row.distance_mm.is_none()
            && row.bearing_deg.is_none()
            && row.latency_upper_us.is_none()
            && row.cell
                == format!(
                    "{}face-unknown-distance-unknown",
                    row.visible_face_count.unwrap()
                )));
    }
}

#[test]
fn external_development_unknown_outputs_still_fail_raw_accuracy() {
    let (m, mut r) = external_replay();
    r.predictions[0].output = "unknown".into();
    let scored = report(&m, &r);
    let raw = &scored.raw_by_split.unwrap()["development"];
    assert_eq!((raw.counts.correct, raw.counts.total), (319, 320));
    assert!(!scored.qualified);
}

#[test]
fn unknown_clock_error_keeps_actual_admission_comparison_unknown() {
    let (mut m, mut r) = fixture(Sensor::VoiceMatch);
    for session in &mut m.sessions {
        session.split = Split::Development;
        session.data_origin = Some(DataOrigin::Public);
        session.consent = None;
        session.clock_verified = Some(false);
        session.sync_error_us = None;
    }
    for prediction in &mut r.predictions {
        prediction.first_admission_us = prediction.ready_us.map(|at| at + 1_000_000);
    }
    let scored = report(&m, &r);
    assert!(!scored.qualified);
    assert!(
        scored
            .rows
            .iter()
            .all(|row| row.ready_before_first_admission.is_none()
                && row.latency_upper_us.is_none()
                && !row.available
                && !row.within_admission_budget)
    );
}

#[test]
fn external_unknown_metadata_cannot_be_promoted_or_applied_to_real_people() {
    let (m, _) = external_replay();
    let mut holdout = m.clone();
    for session in &mut holdout.sessions {
        session.split = Split::Holdout;
    }
    assert!(holdout.validate().is_err());
    let mut real = m.clone();
    for session in &mut real.sessions {
        session.data_origin = Some(DataOrigin::RealPeople);
    }
    assert!(real.validate().is_err());
    let mut false_clock = m.clone();
    false_clock.sessions[0].clock_verified = Some(true);
    assert!(false_clock.validate().is_err());
    let mut zero = m;
    zero.trials[0].distance_mm = Some(0);
    assert!(zero.validate().is_err());
}

#[test]
fn legacy_unknown_consent_geometry_and_clock_are_still_rejected() {
    let (mut m, _) = fixture(Sensor::Call);
    m.version = 1;
    m.rubric_id = None;
    for session in &mut m.sessions {
        session.delete_after_unix_s = Some(100_000);
        session.retention = None;
        session.inventory_sha256 = None;
        session.readme_sha256 = None;
        session.data_origin = None;
        session.clock_verified = None;
    }
    m.validate().unwrap();
    let mut no_consent = m.clone();
    no_consent.sessions[0].consent = None;
    assert!(no_consent.validate().is_err());
    let mut no_distance = m.clone();
    no_distance.trials[0].distance_mm = None;
    assert!(no_distance.validate().is_err());
    let mut no_bearing = m.clone();
    no_bearing.trials[0].bearing_deg = None;
    assert!(no_bearing.validate().is_err());
    m.sessions[0].sync_error_us = None;
    assert!(m.validate().is_err());
}

#[test]
fn version_two_requires_permanent_hashed_retention_and_explicit_rubric() {
    let (mut m, r) = fixture(Sensor::Call);
    m.sessions[0].delete_after_unix_s = Some(99_999);
    assert!(m.validate().is_err());
    m.sessions[0].delete_after_unix_s = None;
    m.sessions[0].inventory_sha256 = None;
    assert!(m.validate().is_err());
    m.sessions[0].inventory_sha256 = Some("1".repeat(64));
    m.sessions[0].readme_sha256 = Some("not-a-sha256".into());
    assert!(m.validate().is_err());
    m.sessions[0].readme_sha256 = Some("2".repeat(64));
    m.sessions[0].consent = Some(false);
    assert!(m.validate().is_err());
    m.sessions[0].consent = Some(true);
    m.rubric_id = Some("older-rules".into());
    assert!(m.validate().is_err());
    m.rubric_id = Some(V2_RUBRIC.into());
    let mut mismatched = r;
    mismatched.version = 1;
    assert!(score(&m, &mismatched, &mismatched.manifest_sha256).is_err());
}

#[test]
fn legacy_call_format_and_report_omit_v2_fields() {
    let (mut m, mut r) = fixture(Sensor::Call);
    m.version = 1;
    m.rubric_id = None;
    r.version = 1;
    for session in &mut m.sessions {
        session.delete_after_unix_s = Some(100_000);
        session.retention = None;
        session.inventory_sha256 = None;
        session.readme_sha256 = None;
        session.data_origin = None;
        session.clock_verified = None;
    }
    let result = report(&m, &r);
    assert!(result.qualified);
    let value = serde_json::to_value(result).unwrap();
    assert!(value.get("rubric_id").is_none());
    assert!(value.get("raw_by_split").is_none());
    assert!(value["rows"][0].get("split").is_none());
    assert!(value["rows"][0].get("physical_outcome_correct").is_none());
    m.sessions[0].retention = Some("keep_all".into());
    assert!(m.validate().is_err());
}

#[test]
fn head_and_shoulders_disagreement_is_away_and_unknown_needs_adjudication() {
    let (mut m, mut r) = fixture(Sensor::Head);
    let baseline = report(&m, &r);
    assert_eq!(baseline.slices["head_shoulders_disagree/true"].total, 240);
    for p in r.predictions.iter_mut().skip(240).take(13) {
        p.output = "facing".into();
    }
    assert!(!report(&m, &r).qualified);
    m.trials[0].shoulders_target = Some("unknown".into());
    assert!(m.validate().is_err());
    m.cases[0].head_gold_adjudicated = Some(true);
    assert!(m.validate().is_ok());
    assert_eq!(report(&m, &r).rows[0].head_gold_adjudicated, Some(true));
    m.cases[0].head_gold_adjudicated = None;
    m.cases[0].expected = "unknown".into();
    m.cases[0].fault = true;
    r.predictions[0].output = "unknown".into();
    let result = report(&m, &r);
    assert!(!result.rows[0].classification_correct);
    assert!(!result.rows[0].correct);
    assert_eq!(result.raw_by_split.as_ref().unwrap()["holdout"].faults, 1);
}

#[test]
fn speaking_face_excludes_one_face_and_reports_higher_counts_without_merging_them() {
    let (mut m, mut r) = fixture(Sensor::SpeakingFace);
    let baseline = report(&m, &r);
    assert_eq!(baseline.counts.total, 320);
    assert_eq!(
        baseline.raw_by_split.as_ref().unwrap()["holdout"].inapplicable,
        80
    );
    m.trials[0].visible_faces.push("track3".into());
    let result = report(&m, &r);
    assert!(result.qualified);
    assert_eq!(result.slices["visible_faces/3"].total, 1);
    for i in 0..320 {
        m.trials[i].visible_faces.truncate(1);
        if m.trials[i].subject_visible {
            m.trials[i].subject_face_id = Some("track1".into());
            m.cases[i].expected = "track1".into();
            r.predictions[i].subject_face_id = Some("track1".into());
            r.predictions[i].output = "track1".into();
        }
    }
    let result = report(&m, &r);
    assert!(!result.qualified);
    assert_eq!(result.counts.total, 0);
    assert_eq!(result.raw_classification.total, 0);
    assert_eq!(result.rows.len(), 480);
}

#[test]
fn unknown_gold_and_unknown_missing_or_late_predictions_cannot_be_primary_successes() {
    let (mut m, mut r) = fixture(Sensor::SpeakingFace);
    m.cases[0].expected = "unknown".into();
    r.predictions[0].output = "unknown".into();
    r.predictions[1].output = "unknown".into();
    r.predictions[2].ready_us = Some(m.trials[2].end_us + 600_000);
    r.predictions[2].associated_us = Some(m.trials[2].end_us + 600_001);
    r.predictions.remove(3);
    let result = report(&m, &r);
    assert_eq!(result.counts.total, 320);
    assert_eq!(result.counts.correct, 316);
    assert!(!result.qualified); // Only 319 primary lines have answerable gold.
    assert!(!result.rows[0].classification_correct);
    assert!(result.rows[2].classification_correct);
    assert!(!result.rows[2].correct);
    assert_eq!(result.rows[3].output, "missing");
}

#[test]
fn physical_interrupt_stop_is_inclusive_at_400_ms_and_not_a_classifier_timestamp() {
    let (m, mut r) = fixture(Sensor::BargeIn);
    for (p, t) in r.predictions.iter_mut().zip(&m.trials).take(120) {
        p.last_lamp_audio_us = Some(t.start_us + 399_000);
    }
    assert!(report(&m, &r).qualified);
    for p in r.predictions.iter_mut().take(6) {
        p.last_lamp_audio_us = None;
    }
    let at_boundary = report(&m, &r);
    assert!(at_boundary.qualified); // Exactly 114/120 physical successes.
    assert_eq!(at_boundary.raw_classification.correct, 480);
    r.predictions[6].last_lamp_audio_us = None;
    assert!(!report(&m, &r).qualified);
    for (p, t) in r.predictions.iter_mut().zip(&m.trials).take(120) {
        p.last_lamp_audio_us = Some(t.start_us + 400_001);
    }
    let result = report(&m, &r);
    assert!(!result.qualified);
    assert_eq!(result.counts.correct, 360);
    assert_eq!(result.raw_classification.correct, 480);
}

#[test]
fn negative_classes_need_the_full_declared_playback_observation_and_each_pass_separately() {
    let (m, mut r) = fixture(Sensor::BargeIn);
    for p in r.predictions.iter_mut().skip(120).take(7) {
        p.observed_until_us = p.last_lamp_audio_us;
    }
    let result = report(&m, &r);
    assert!(!result.qualified);
    assert!(
        result
            .checks
            .iter()
            .any(|c| c.name == "backchannel" && !c.passed)
    );
    assert!(
        result
            .checks
            .iter()
            .any(|c| c.name == "side_talk" && c.passed)
    );
    for p in r.predictions.iter_mut().skip(120).take(7) {
        p.observed_until_us = Some(p.last_lamp_audio_us.unwrap() + 500_000);
        p.playback_stopped = None;
    }
    assert!(!report(&m, &r).qualified);
}

#[test]
fn development_raw_scores_are_visible_but_do_not_supply_holdout_qualification() {
    let (mut m, mut r) = fixture(Sensor::Addressed);
    for session in &mut m.sessions {
        session.split = Split::Development;
    }
    r.evidence = "replay".into();
    for p in &mut r.predictions {
        p.ready_us = None;
        p.face_in_frame_us = None;
        p.replay_elapsed_us = Some(12_900);
    }
    r.predictions[0].output = "unknown".into();
    r.predictions.pop();
    let result = report(&m, &r);
    assert!(!result.qualified);
    assert_eq!(result.counts.total, 0);
    assert_eq!(result.rows.len(), 480);
    let raw = &result.raw_by_split.as_ref().unwrap()["development"];
    assert_eq!(raw.counts.total, 480);
    assert_eq!(raw.counts.correct, 478);
    assert_eq!(raw.by_class["lamp"].correct, 239);
    assert_eq!(raw.by_class["room"].correct, 239);
    assert_eq!(raw.confusion["lamp"]["unknown"], 1);
    assert_eq!(raw.confusion["room"]["missing"], 1);
    assert!(result.rows.iter().all(|row| row.latency_us.is_none()));
}

#[test]
fn replay_cannot_supply_physical_playback_or_other_live_readiness() {
    let (m, mut r) = fixture(Sensor::BargeIn);
    r.evidence = "replay".into();
    for p in &mut r.predictions {
        p.ready_us = None;
        p.face_in_frame_us = None;
    }
    assert!(score(&m, &r, &r.manifest_sha256).is_err());
    for p in &mut r.predictions {
        p.last_lamp_audio_us = None;
        p.observed_until_us = None;
        p.playback_stopped = None;
        p.replay_elapsed_us = Some(10_000);
    }
    let result = report(&m, &r);
    assert!(!result.qualified);
    assert_eq!(result.raw_classification.correct, 480);
    assert_eq!(result.counts.correct, 0);
    let (m, mut r) = fixture(Sensor::BargeIn);
    r.evidence = "synthetic".into();
    assert!(!report(&m, &r).qualified);
}

#[test]
fn origin_is_separate_from_execution_location_and_includes_voice_references() {
    for origin in [
        DataOrigin::Generated,
        DataOrigin::Public,
        DataOrigin::Invented,
    ] {
        let (mut m, r) = fixture(Sensor::Call);
        for session in &mut m.sessions {
            session.data_origin = Some(origin);
        }
        let result = report(&m, &r);
        assert!(!result.qualified);
        assert_eq!(result.evidence, "lamp_live");
        assert_eq!(result.raw_classification.correct, 480);
        assert_eq!(
            result.raw_by_split.as_ref().unwrap()["holdout"].by_origin[origin.as_str()].total,
            480
        );
        assert!(
            result
                .checks
                .iter()
                .any(|c| c.name == "real_people_primary_holdout" && !c.passed)
        );
    }
    let (mut m, r) = fixture(Sensor::VoiceMatch);
    m.sessions[0].data_origin = Some(DataOrigin::Generated);
    assert!(!report(&m, &r).qualified);
    m.sessions[0].data_origin = None;
    assert!(m.validate().is_err());
}

#[test]
fn safe_abstention_is_a_fault_diagnostic_never_primary_accuracy() {
    let (m, mut r) = fixture(Sensor::SpeakingFace);
    let result = report(&m, &r);
    let raw = &result.raw_by_split.as_ref().unwrap()["holdout"];
    assert_eq!(raw.safe_abstentions, 80);
    assert_eq!(raw.counts.correct, 320);
    assert_eq!(raw.counts.total, 320);
    assert!(result.rows[320].safe_abstention.unwrap());
    assert!(!result.rows[320].classification_correct);
    assert!(!result.rows[320].correct);
    r.predictions[320].held_line = true;
    assert_eq!(report(&m, &r).rows[320].safe_abstention, Some(false));
}

#[test]
fn absent_live_readiness_is_a_counted_v2_failure_not_a_dropped_case() {
    let (m, mut r) = fixture(Sensor::Addressed);
    for p in &mut r.predictions {
        p.ready_us = None;
    }
    let result = report(&m, &r);
    assert!(!result.qualified);
    assert_eq!(result.counts.total, 480);
    assert_eq!(result.counts.correct, 0);
    assert_eq!(result.raw_classification.correct, 480);
    assert!(
        result
            .rows
            .iter()
            .all(|row| row.output == "late_or_incomplete")
    );
}

#[test]
fn natural_playback_completion_is_not_an_interrupt_stop() {
    let (mut m, mut r) = fixture(Sensor::BargeIn);
    for i in 0..7 {
        m.trials[i].planned_lamp_audio_end_us = r.predictions[i].last_lamp_audio_us;
        r.predictions[i].playback_stopped = Some(true);
    }
    let result = report(&m, &r);
    assert!(!result.qualified);
    assert_eq!(result.counts.correct, 473);
    assert_eq!(result.rows[0].stop_latency_us, Some(300_000));
    assert_eq!(result.rows[0].physical_outcome_correct, Some(false));
}

#[test]
fn v2_qualification_needs_binary_and_verified_bounded_source_clocks() {
    let (mut m, mut r) = fixture(Sensor::Call);
    r.producer_binary_sha256 = None;
    let result = report(&m, &r);
    assert!(!result.qualified);
    assert_eq!(result.raw_classification.correct, 480);
    assert!(
        result
            .checks
            .iter()
            .any(|c| c.name == "producing_binary_recorded" && !c.passed)
    );
    r.producer_binary_sha256 = Some("b".repeat(64));
    m.sessions[0].clock_verified = None;
    assert!(!report(&m, &r).qualified);
    m.sessions[0].clock_verified = Some(true);
    m.sessions[0].sync_error_us = Some(5_000);
    assert!(report(&m, &r).qualified);
    m.sessions[0].sync_error_us = Some(5_001);
    assert!(!report(&m, &r).qualified);
    let (mut m, r) = fixture(Sensor::VoiceMatch);
    m.sessions[0].clock_verified = Some(false); // A reference, not a probe session.
    assert!(!report(&m, &r).qualified);
}

#[test]
fn mapped_latency_uses_conservative_upper_bounds_but_reports_raw_values() {
    let (m, mut r) = fixture(Sensor::Call);
    for p in &mut r.predictions {
        p.ready_us = Some(p.input_end_us + 99_000);
    }
    let result = report(&m, &r);
    assert!(result.qualified);
    assert_eq!(result.timing_us["input_to_result_p95"], Some(99_000));
    assert_eq!(result.timing_us["input_to_result_upper_p95"], Some(100_000));
    for p in &mut r.predictions {
        p.ready_us = Some(p.input_end_us + 100_000);
    }
    assert!(!report(&m, &r).qualified);
    let (m, mut r) = fixture(Sensor::Addressed);
    for p in &mut r.predictions {
        p.ready_us = Some(p.input_end_us + 50_000);
    }
    let result = report(&m, &r);
    assert!(!result.qualified);
    assert_eq!(result.timing_us["final_asr_to_result_p95"], Some(50_000));
    assert_eq!(
        result.timing_us["final_asr_to_result_upper_p95"],
        Some(51_000)
    );
    assert!(
        result
            .checks
            .iter()
            .any(|c| c.name == "final_asr_to_result" && !c.passed)
    );
}

#[test]
fn capture_mapping_error_affects_face_publication_and_association_not_host_frame_work() {
    let (m, mut r) = fixture(Sensor::SpeakingFace);
    for p in &mut r.predictions {
        p.frame_work_us = Some(200_000);
    }
    assert!(report(&m, &r).qualified);
    for (p, t) in r.predictions.iter_mut().zip(&m.trials) {
        p.input_end_us = t.end_us - 350_000;
        p.ready_us = Some(t.end_us);
    }
    let result = report(&m, &r);
    assert!(!result.qualified);
    assert_eq!(result.timing_us["input_to_result_p95"], Some(350_000));
    assert_eq!(result.timing_us["input_to_result_upper_p95"], Some(351_000));
    let (m, mut r) = fixture(Sensor::SpeakingFace);
    for (p, t) in r.predictions.iter_mut().zip(&m.trials) {
        p.associated_us = Some(t.end_us + 5_000);
    }
    let result = report(&m, &r);
    assert_eq!(result.counts.correct, 0);
    assert_eq!(
        result.timing_us["cached_line_association_upper_p95"],
        Some(6_000)
    );
}

#[test]
fn interruption_budget_includes_clock_error_at_the_exact_boundary() {
    let (m, mut r) = fixture(Sensor::BargeIn);
    for (p, t) in r.predictions.iter_mut().zip(&m.trials).take(120) {
        p.last_lamp_audio_us = Some(t.start_us + 399_000);
    }
    let result = report(&m, &r);
    assert!(result.qualified);
    assert_eq!(
        result.timing_us["interrupt_onset_to_last_audio_p95"],
        Some(399_000)
    );
    assert_eq!(
        result.timing_us["interrupt_onset_to_last_audio_upper_p95"],
        Some(400_000)
    );
    for (p, t) in r.predictions.iter_mut().zip(&m.trials).take(120) {
        p.last_lamp_audio_us = Some(t.start_us + 399_001);
    }
    assert!(!report(&m, &r).qualified);
}

#[test]
fn coupled_head_labels_do_not_replace_the_two_disagreement_strata() {
    let (mut m, r) = fixture(Sensor::Head);
    for t in m.trials.iter_mut().skip(240) {
        t.head_target = "away".into();
        t.shoulders_target = Some("away".into());
    }
    let result = report(&m, &r);
    assert_eq!(result.counts.correct, 480);
    assert!(!result.qualified);
    assert!(
        result
            .checks
            .iter()
            .any(|c| c.name == "head_lamp_shoulders_away" && !c.passed)
    );
    assert!(
        result
            .checks
            .iter()
            .any(|c| c.name == "head_away_shoulders_lamp" && !c.passed)
    );
}

#[test]
fn face_cells_need_both_speaker_slots_and_off_camera_negatives() {
    let (mut m, mut r) = fixture(Sensor::SpeakingFace);
    for i in (1..18).step_by(2) {
        m.trials[i].subject_face_id = Some("track1".into());
        m.cases[i].expected = "track1".into();
        r.predictions[i].subject_face_id = Some("track1".into());
        r.predictions[i].output = "track1".into();
    }
    let result = report(&m, &r);
    assert_eq!(result.counts.correct, 320);
    assert!(!result.qualified);
    assert!(
        result
            .checks
            .iter()
            .any(|c| c.name == "2plus_faces-600mm-daylight_visible_slot_1" && !c.passed)
    );
    let (mut m, mut r) = fixture(Sensor::SpeakingFace);
    m.trials[64].subject_visible = true;
    m.trials[64].subject_face_id = Some("track1".into());
    m.cases[64].expected = "track1".into();
    r.predictions[64].subject_face_id = Some("track1".into());
    r.predictions[64].output = "track1".into();
    let result = report(&m, &r);
    assert_eq!(result.counts.correct, 320);
    assert!(!result.qualified);
    assert!(
        result
            .checks
            .iter()
            .any(|c| c.name == "2plus_faces-600mm-daylight_no_visible" && !c.passed)
    );
}
