//! Invented contract fixtures only. These hashes and timings are not lamp evidence.
use lamp_sensor_score::whole::{
    Action, BuildIdentity, Case, Comparison, DataOrigin, IGNORE_OBSERVATION_US, Kind, Line, Mode,
    Prerequisite, Purpose, RUBRIC, Report, Run, Scenario, WindowBand, cases_hash, compare,
};

fn hash(n: usize) -> String {
    format!("{n:064x}")
}

fn identity(n: usize) -> BuildIdentity {
    BuildIdentity {
        build_sha: format!("{n:040x}"),
        producer_binary_sha256: hash(n + 1),
        source_patch_sha256: None,
        config_sha256: hash(n + 2),
        model_manifest_sha256: hash(n + 3),
    }
}

fn case(
    mode: Mode,
    index: usize,
    scenario: Scenario,
    kind: Kind,
    band: Option<WindowBand>,
) -> Case {
    let expected = match kind {
        Kind::Call => Action::Answer,
        Kind::Followup
            if matches!(
                band,
                Some(WindowBand::Outside | WindowBand::BoundaryOutside | WindowBand::Closed)
            ) =>
        {
            Action::Ignore
        }
        Kind::Followup => Action::Answer,
        Kind::Room | Kind::Noise if scenario == Scenario::Overlap => Action::Continue,
        Kind::Room | Kind::Noise => Action::Ignore,
        Kind::Interruption | Kind::Stop => Action::Stop,
        Kind::Backchannel => Action::Continue,
    };
    Case {
        id: format!("{}_case_{index}", mode.name()),
        mode,
        scenario,
        kind,
        expected,
        speaker_id: (kind != Kind::Noise).then(|| format!("speaker_{}", index % 6)),
        session_id: format!("session_{}", index % 2),
        people: if scenario == Scenario::TwoPeople {
            2
        } else if kind == Kind::Noise {
            0
        } else {
            1
        },
        truth_known: true,
        data_origin: if mode == Mode::Live && kind != Kind::Noise {
            DataOrigin::RealPeople
        } else {
            DataOrigin::Generated
        },
        consent: true,
        window_band: band,
    }
}

fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for mode in [Mode::Played, Mode::Live] {
        let mut index = 0;
        for scenario in [Scenario::Alone, Scenario::Noisy, Scenario::TwoPeople] {
            for _ in 0..10 {
                cases.push(case(
                    mode,
                    index,
                    scenario,
                    Kind::Call,
                    Some(WindowBand::Outside),
                ));
                index += 1;
            }
        }
        for kind in [
            Kind::Interruption,
            Kind::Backchannel,
            Kind::Room,
            Kind::Noise,
        ] {
            for _ in 0..10 {
                cases.push(case(mode, index, Scenario::Overlap, kind, None));
                index += 1;
            }
        }
        for band in [
            WindowBand::Early,
            WindowBand::Middle,
            WindowBand::Late,
            WindowBand::BoundaryInside,
            WindowBand::BoundaryOutside,
            WindowBand::Outside,
            WindowBand::Closed,
            WindowBand::Early,
            WindowBand::Middle,
            WindowBand::Late,
        ] {
            cases.push(case(
                mode,
                index,
                Scenario::Followup20s,
                Kind::Followup,
                Some(band),
            ));
            index += 1;
        }
        for i in 0..10 {
            cases.push(case(
                mode,
                index,
                Scenario::SideTalk,
                Kind::Room,
                Some(if i % 2 == 0 {
                    WindowBand::Early
                } else {
                    WindowBand::Outside
                }),
            ));
            index += 1;
        }
    }
    cases
}

fn line(case: &Case, index: usize, run_index: usize, clock: &str, candidate: bool) -> Line {
    // Each leg has an independent physical timeline; no common absolute origin.
    let start = 30_000_000 + index as u64 * 50_000_000 + run_index as u64 * 1_000_000;
    let end = start + 1_000_000;
    let gap = match case.window_band {
        Some(WindowBand::Early) => 1_000_000,
        Some(WindowBand::Middle) => 5_000_000,
        Some(WindowBand::Late) => 15_000_000,
        Some(WindowBand::BoundaryInside) => 19_900_000,
        Some(WindowBand::BoundaryOutside) => 20_100_000,
        _ => 25_000_000,
    };
    let answer_latency = if candidate { 400_000 } else { 500_000 };
    let stop_latency = if candidate { 200_000 } else { 300_000 };
    let baseline_miss = !candidate && case.scenario == Scenario::SideTalk && index == 80;
    Line {
        id: case.id.clone(),
        clock_id: clock.into(),
        observed: if baseline_miss {
            Action::Answer
        } else {
            case.expected
        },
        stimulus_verified: true,
        outcome_verified: true,
        speech_start_us: start,
        speech_end_us: end,
        previous_lamp_audio_end_us: case.window_band.map(|_| start - gap),
        window_closed_us: (case.window_band == Some(WindowBand::Closed))
            .then_some(start - 1_000_000),
        observation_end_us: Some(if case.expected == Action::Continue {
            end + 2_000_000
        } else {
            end + IGNORE_OBSERVATION_US
        }),
        first_audio_us: (case.expected == Action::Answer || baseline_miss)
            .then_some(end + answer_latency),
        interrupt_onset_us: (case.expected == Action::Stop).then_some(start),
        last_audio_us: match case.expected {
            Action::Stop => Some(start + stop_latency),
            Action::Continue => Some(end + 2_000_000),
            _ => None,
        },
        planned_lamp_audio_end_us: matches!(case.expected, Action::Stop | Action::Continue)
            .then_some(end + 2_000_000),
        playback_active_at_input: matches!(case.expected, Action::Stop | Action::Continue),
        continued_without_restart: (case.expected == Action::Continue).then_some(true),
        listening_after_stop: (case.expected == Action::Stop).then_some(true),
        safety_violation: false,
    }
}

fn fixture() -> Comparison {
    let cases = cases();
    let cases_sha256 = cases_hash(&cases).unwrap();
    let baseline = identity(100);
    let candidate = identity(200);
    let mut runs = Vec::new();
    for mode in [Mode::Played, Mode::Live] {
        for leg in ["A1", "B1", "B2", "A2"] {
            let index = runs.len();
            let clock = format!("clock_{}_{}", mode.name(), leg);
            let is_candidate = leg.starts_with('B');
            runs.push(Run {
                leg: leg.into(),
                mode,
                started_unix_s: 200 + index as u64,
                identity: if is_candidate {
                    candidate.clone()
                } else {
                    baseline.clone()
                },
                unchanged_config_sha256: hash(300),
                kit_sha256: hash(301),
                cases_sha256: cases_sha256.clone(),
                raw_manifest_sha256: hash(400 + index),
                evidence: "lamp_live".into(),
                enabled_sensors: if is_candidate {
                    vec!["head".into()]
                } else {
                    Vec::new()
                },
                combined_judge: true,
                clock_id: clock.clone(),
                sync_error_us: Some(1_000),
                clock_verified: true,
                safety_violation: false,
                lines: cases
                    .iter()
                    .filter(|c| c.mode == mode)
                    .enumerate()
                    .map(|(i, c)| line(c, i, index, &clock, is_candidate))
                    .collect(),
            });
        }
    }
    Comparison {
        version: 2,
        rubric: RUBRIC.into(),
        purpose: Purpose::OneSensorAddition,
        baseline,
        candidate,
        unchanged_config_sha256: hash(300),
        kit_sha256: hash(301),
        kit_reviewed: true,
        collection_manifest_sha256: hash(302),
        cases_sha256,
        scorer_revision_sha: format!("{:040x}", 303),
        scorer_binary_sha256: hash(304),
        protocol_frozen_unix_s: 100,
        labels_adjudicated_before_predictions: true,
        holdout_disjoint: true,
        accepted_sensors: Vec::new(),
        added_sensor: Some("head".into()),
        added_component_model_sha256: Some(hash(306)),
        added_component_config_sha256: Some(hash(307)),
        target_scenario: Some(Scenario::SideTalk),
        prerequisite: Some(Prerequisite {
            sensor: "head".into(),
            report_sha256: hash(305),
            model_sha256: hash(306),
            config_sha256: hash(307),
            rubric: RUBRIC.into(),
            qualified: true,
            holdout_on_lamp: true,
        }),
        cases,
        runs,
    }
}

fn rehash(input: &mut Comparison) {
    input.cases_sha256 = cases_hash(&input.cases).unwrap();
    for run in &mut input.runs {
        run.cases_sha256.clone_from(&input.cases_sha256);
    }
}

fn failed(report: &Report, fragment: &str) -> bool {
    report
        .checks
        .iter()
        .any(|c| !c.passed && c.name.contains(fragment))
}

#[test]
fn complete_counterbalanced_addition_passes_and_reports_fixed_denominators() {
    let report = compare(&fixture()).unwrap();
    assert!(
        report.passed,
        "{:?}",
        report
            .checks
            .iter()
            .filter(|c| !c.passed)
            .collect::<Vec<_>>()
    );
    assert_eq!(report.groups.len(), 48);
    assert_eq!(report.rows.len(), 720);
    assert_eq!(report.target_gains.len(), 4);
    for gain in &report.target_gains {
        assert_eq!(gain.planned, 10);
        assert_eq!(gain.additional_correct, 1);
        assert_eq!(gain.percentage_points, 10.0);
    }
}

#[test]
fn cd_characterization_is_always_report_only() {
    let mut input = fixture();
    input.purpose = Purpose::BaselineCharacterization;
    input.target_scenario = None;
    input.added_sensor = None;
    input.added_component_model_sha256 = None;
    input.added_component_config_sha256 = None;
    input.prerequisite = None;
    input.runs.retain(|r| r.leg == "A1" || r.leg == "B1");
    for run in &mut input.runs {
        run.leg = if run.leg == "B1" { "C" } else { "D" }.into();
    }
    input.runs.sort_by_key(|r| (r.mode, r.leg.clone()));
    for (i, run) in input.runs.iter_mut().enumerate() {
        run.started_unix_s = 200 + i as u64;
    }
    let report = compare(&input).unwrap();
    assert!(!report.passed);
    assert!(failed(&report, "promotion_purpose"));
    assert!(report.target_gains.is_empty());
    assert!(!failed(&report, "run_order"));
}

#[test]
fn missing_cases_remain_in_the_denominator() {
    let mut input = fixture();
    input.runs[1].lines.remove(0);
    let report = compare(&input).unwrap();
    assert!(!report.passed);
    let group = report
        .groups
        .iter()
        .find(|g| g.leg == "B1" && g.mode == Mode::Played && g.scenario == Scenario::Alone)
        .unwrap();
    assert_eq!((group.planned, group.correct, group.missing), (10, 9, 1));
    assert!(failed(&report, "paired_no_loss"));
}

#[test]
fn absent_run_or_wrong_recording_order_cannot_qualify() {
    let mut input = fixture();
    input.runs.remove(2);
    assert!(failed(&compare(&input).unwrap(), "run_order"));
    let mut input = fixture();
    input.runs.swap(1, 2);
    assert!(failed(&compare(&input).unwrap(), "run_order"));
    let mut input = fixture();
    input.runs[1].started_unix_s = input.runs[0].started_unix_s;
    assert!(failed(&compare(&input).unwrap(), "run_order"));
}

#[test]
fn unverified_stimulus_unknown_truth_and_unknown_action_fail() {
    let mut input = fixture();
    input.runs[1].lines[0].stimulus_verified = false;
    assert!(failed(&compare(&input).unwrap(), "alone_evidence"));
    let mut input = fixture();
    input.cases[0].truth_known = false;
    rehash(&mut input);
    assert!(failed(&compare(&input).unwrap(), "alone_evidence"));
    let mut input = fixture();
    input.runs[1].lines[0].observed = Action::Unknown;
    assert!(failed(&compare(&input).unwrap(), "alone_evidence"));
    let mut input = fixture();
    input.runs[1].lines[0].outcome_verified = false;
    assert!(failed(&compare(&input).unwrap(), "alone_evidence"));
}

#[test]
fn unavailable_answer_or_stop_timing_never_becomes_zero() {
    let mut input = fixture();
    input.runs[1].lines[0].first_audio_us = None;
    let report = compare(&input).unwrap();
    assert!(failed(&report, "alone_evidence"));
    assert!(
        report
            .rows
            .iter()
            .any(|r| r.leg == "B1" && r.timing_unknown && r.latency_us.is_none())
    );
    let mut input = fixture();
    input.runs[1].lines[30].last_audio_us = None;
    assert!(failed(&compare(&input).unwrap(), "overlap_evidence"));
    let mut input = fixture();
    for run in &mut input.runs {
        for line in run.lines.iter_mut().take(10) {
            line.observed = Action::Ignore;
            line.first_audio_us = None;
        }
    }
    let report = compare(&input).unwrap();
    assert!(failed(&report, "alone_no_drop"));
    let group = &report.groups[0];
    assert!(group.answer_timing_expected);
    assert_eq!(group.answer_timing_samples, 0);
    assert_eq!(group.answer_p50_us, None);
}

#[test]
fn no_absolute_clock_origin_is_shared_but_each_leg_must_be_verified() {
    let mut input = fixture();
    input.runs[1].sync_error_us = None;
    assert!(failed(&compare(&input).unwrap(), "B1_clock"));
    let mut input = fixture();
    input.runs[1].sync_error_us = Some(5_001);
    assert!(failed(&compare(&input).unwrap(), "B1_clock"));
    let mut input = fixture();
    input.runs[1].lines[0].clock_id = "other_capture".into();
    assert!(failed(&compare(&input).unwrap(), "alone_evidence"));
    assert_ne!(
        input.runs[0].lines[0].speech_start_us,
        input.runs[1].lines[0].speech_start_us
    );
}

#[test]
fn clock_uncertainty_cannot_be_claimed_as_latency_improvement() {
    let mut input = fixture();
    for line in input.runs[1].lines.iter_mut().take(10) {
        line.first_audio_us = Some(line.speech_end_us + 500_000);
    }
    let report = compare(&input).unwrap();
    assert!(failed(&report, "alone_no_drop"));
    assert!(failed(&report, "alone_Answer_paired_latency"));
    for run in &mut input.runs {
        run.sync_error_us = Some(0);
    }
    assert!(compare(&input).unwrap().passed);
}

#[test]
fn latency_regressions_and_safety_fail_even_with_target_gain() {
    let mut input = fixture();
    input.runs[1].lines[0].first_audio_us = Some(input.runs[1].lines[0].speech_end_us + 900_000);
    assert!(failed(&compare(&input).unwrap(), "alone_no_drop"));
    let mut input = fixture();
    input.runs[1].lines[0].safety_violation = true;
    assert!(failed(&compare(&input).unwrap(), "alone_evidence"));
    let mut input = fixture();
    input.runs[1].safety_violation = true;
    assert!(!compare(&input).unwrap().passed);
}

#[test]
fn a_gain_never_offsets_a_previously_correct_case_lost() {
    let mut input = fixture();
    for run in &mut input.runs {
        if run.leg.starts_with('A') {
            run.lines[81].observed = Action::Answer;
        } else {
            run.lines[82].observed = Action::Answer;
        }
    }
    let report = compare(&input).unwrap();
    assert!(
        report
            .target_gains
            .iter()
            .all(|g| g.additional_correct == 1)
    );
    assert!(failed(&report, "paired_no_loss"));
    assert!(!report.passed);
}

#[test]
fn each_b_leg_and_each_mode_must_improve_target() {
    let mut input = fixture();
    input.runs[2].lines[80].observed = Action::Answer;
    let report = compare(&input).unwrap();
    assert!(failed(&report, "played_A2_B2_target_improves"));
    let mut input = fixture();
    input.runs[5].lines[80].observed = Action::Answer;
    assert!(failed(
        &compare(&input).unwrap(),
        "live_A1_B1_target_improves"
    ));
}

#[test]
fn interruption_bar_uses_onset_actual_last_output_clock_bound_and_listening() {
    let mut input = fixture();
    let line = &mut input.runs[1].lines[30];
    line.last_audio_us = Some(line.speech_start_us + 400_000);
    let report = compare(&input).unwrap();
    assert!(failed(&report, "Interruption_overlap_bar"));
    assert!(failed(&report, "interrupt_p95"));
    let mut input = fixture();
    input.runs[1].lines[30].listening_after_stop = None;
    assert!(failed(&compare(&input).unwrap(), "overlap_evidence"));
    let mut input = fixture();
    input.runs[1].lines[30].playback_active_at_input = false;
    assert!(failed(&compare(&input).unwrap(), "overlap_evidence"));
}

#[test]
fn interruption_decision_or_asr_time_cannot_replace_acoustic_onset() {
    let mut input = fixture();
    let line = &mut input.runs[1].lines[30];
    line.interrupt_onset_us = Some(line.speech_start_us + 900_000);
    line.last_audio_us = Some(line.speech_start_us + 1_100_000);
    let id = line.id.clone();
    let report = compare(&input).unwrap();
    let row = report
        .rows
        .iter()
        .find(|r| r.mode == Mode::Played && r.leg == "B1" && r.case_id == id)
        .unwrap();
    assert_eq!(row.latency_us, Some(1_100_000));
    assert!(row.unknown);
    assert!(!row.correct);
    assert!(failed(&report, "Interruption_overlap_bar"));
    assert!(!report.passed);
}

#[test]
fn each_non_interrupt_overlap_class_must_keep_talking_at_95_percent() {
    for index in [40, 50, 60] {
        let mut input = fixture();
        // Keep the baseline error too: no-regression alone must not satisfy this bar.
        for run in &mut input.runs {
            run.lines[index].observed = Action::Stop;
        }
        let report = compare(&input).unwrap();
        assert!(failed(&report, "overlap_bar"));
        assert!(!report.passed);
    }
    let mut input = fixture();
    input.runs[1].lines[40].continued_without_restart = None;
    assert!(failed(&compare(&input).unwrap(), "overlap_evidence"));
}

#[test]
fn natural_output_completion_is_not_an_interruption_or_successful_continuation() {
    let mut input = fixture();
    input.runs[1].lines[40].planned_lamp_audio_end_us = None;
    assert!(failed(&compare(&input).unwrap(), "overlap_evidence"));
    let mut input = fixture();
    let line = &mut input.runs[1].lines[40];
    line.last_audio_us = Some(line.planned_lamp_audio_end_us.unwrap() - 1);
    assert!(failed(&compare(&input).unwrap(), "overlap_evidence"));
    let mut input = fixture();
    let line = &mut input.runs[1].lines[30];
    line.planned_lamp_audio_end_us = line.last_audio_us;
    assert!(failed(&compare(&input).unwrap(), "overlap_evidence"));
    let mut input = fixture();
    input.runs[1].lines[30].planned_lamp_audio_end_us = None;
    assert!(failed(&compare(&input).unwrap(), "overlap_evidence"));
}

#[test]
fn live_speaker_ids_require_real_people_origin_and_consent() {
    let mut input = fixture();
    for case in input.cases.iter_mut().filter(|c| c.mode == Mode::Live) {
        case.data_origin = DataOrigin::Generated;
    }
    rehash(&mut input);
    let report = compare(&input).unwrap();
    assert!(failed(&report, "live_speakers_and_sessions"));
    assert!(failed(&report, "live_B1_alone_evidence"));
    let mut input = fixture();
    input.cases[90].consent = false;
    rehash(&mut input);
    assert!(failed(&compare(&input).unwrap(), "live_B1_alone_evidence"));
    let mut input = fixture();
    input.cases[0].data_origin = DataOrigin::Invented;
    rehash(&mut input);
    assert!(failed(
        &compare(&input).unwrap(),
        "played_B1_alone_evidence"
    ));
    let mut json = serde_json::to_value(fixture()).unwrap();
    json["cases"][0]
        .as_object_mut()
        .unwrap()
        .remove("data_origin");
    assert!(serde_json::from_value::<Comparison>(json).is_err());
}

#[test]
fn followup_boundaries_use_speech_start_and_previous_actual_output_end() {
    let mut input = fixture();
    let line = &mut input.runs[1].lines[73];
    assert_eq!(
        input.cases[73].window_band,
        Some(WindowBand::BoundaryInside)
    );
    line.previous_lamp_audio_end_us = Some(line.speech_start_us - 20_000_001);
    assert!(failed(&compare(&input).unwrap(), "followup_20s_evidence"));
    let mut input = fixture();
    input.runs[1].lines[70].previous_lamp_audio_end_us = None;
    assert!(failed(&compare(&input).unwrap(), "followup_20s_evidence"));
    let mut input = fixture();
    let line = &mut input.runs[1].lines[73];
    line.previous_lamp_audio_end_us = Some(line.speech_start_us - 20_000_000);
    assert!(failed(&compare(&input).unwrap(), "followup_20s_evidence"));
    input.runs[1].sync_error_us = Some(0);
    assert!(compare(&input).unwrap().passed); // Exact known start is in; speech end is after expiry.
    let mut input = fixture();
    input.runs[1].lines[76].window_closed_us = None;
    assert!(failed(&compare(&input).unwrap(), "followup_20s_evidence"));
}

#[test]
fn negative_cases_need_observation_through_the_declared_boundary() {
    let mut input = fixture();
    let line = &mut input.runs[1].lines[80];
    line.observation_end_us = Some(line.speech_end_us + IGNORE_OBSERVATION_US - 1);
    assert!(failed(&compare(&input).unwrap(), "side_talk_evidence"));
    let mut input = fixture();
    input.runs[1].lines[40].observation_end_us = Some(input.runs[1].lines[40].speech_end_us);
    assert!(failed(&compare(&input).unwrap(), "overlap_evidence"));
}

#[test]
fn actual_output_cannot_be_labelled_ignore_or_escape_the_observation_horizon() {
    let mut input = fixture();
    let line = &mut input.runs[1].lines[80];
    line.first_audio_us = Some(line.speech_end_us + 100_000);
    assert_eq!(line.observed, Action::Ignore);
    let report = compare(&input).unwrap();
    assert!(failed(&report, "side_talk_evidence"));
    assert!(!report.passed);
    let mut input = fixture();
    let line = &mut input.runs[1].lines[0];
    line.observation_end_us = Some(line.first_audio_us.unwrap() - 1);
    assert!(failed(&compare(&input).unwrap(), "alone_evidence"));
    let mut input = fixture();
    input.runs[1].lines[30].observation_end_us = None;
    assert!(failed(&compare(&input).unwrap(), "overlap_evidence"));
    let mut input = fixture();
    input.runs[0].lines[80].first_audio_us = None;
    assert!(failed(&compare(&input).unwrap(), "side_talk_evidence"));
}

#[test]
fn added_component_must_match_the_qualified_sensor_model_and_config() {
    let mut input = fixture();
    input.added_component_model_sha256 = Some(hash(999));
    assert!(failed(
        &compare(&input).unwrap(),
        "qualified_sensor_prerequisite"
    ));
    let mut input = fixture();
    input.prerequisite.as_mut().unwrap().config_sha256 = hash(999);
    assert!(failed(
        &compare(&input).unwrap(),
        "qualified_sensor_prerequisite"
    ));
    let mut input = fixture();
    input.added_component_config_sha256 = None;
    assert!(compare(&input).is_err());
}

#[test]
fn hashes_builds_assets_and_freeze_are_binding() {
    let mut input = fixture();
    input.cases[0].speaker_id = Some("changed".into());
    assert!(failed(&compare(&input).unwrap(), "case_manifest_hash"));
    let mut input = fixture();
    input.runs[1].identity.model_manifest_sha256 = hash(999);
    assert!(failed(&compare(&input).unwrap(), "B1_provenance"));
    let mut input = fixture();
    input.runs[1].kit_sha256 = hash(999);
    assert!(failed(&compare(&input).unwrap(), "B1_provenance"));
    let mut input = fixture();
    input.runs[1].raw_manifest_sha256 = input.runs[0].raw_manifest_sha256.clone();
    assert!(failed(&compare(&input).unwrap(), "B1_provenance"));
    let mut input = fixture();
    input.protocol_frozen_unix_s = 201;
    assert!(failed(&compare(&input).unwrap(), "A1_provenance"));
    let mut input = fixture();
    input.baseline.source_patch_sha256 = Some("incomplete".into());
    assert!(compare(&input).is_err());
}

#[test]
fn promotion_requires_exact_one_addition_one_judge_and_linked_qualification() {
    let mut input = fixture();
    input.accepted_sensors.push("head".into());
    assert!(compare(&input).is_err());
    let mut input = fixture();
    input.runs[1].enabled_sensors.push("voice_match".into());
    assert!(failed(&compare(&input).unwrap(), "B1_provenance"));
    let mut input = fixture();
    input.runs[1].combined_judge = false;
    assert!(failed(&compare(&input).unwrap(), "B1_provenance"));
    let mut input = fixture();
    input.prerequisite.as_mut().unwrap().qualified = false;
    assert!(failed(
        &compare(&input).unwrap(),
        "qualified_sensor_prerequisite"
    ));
    let mut input = fixture();
    input.prerequisite.as_mut().unwrap().sensor = "voice_match".into();
    assert!(failed(
        &compare(&input).unwrap(),
        "qualified_sensor_prerequisite"
    ));
    let mut input = fixture();
    input.prerequisite.as_mut().unwrap().report_sha256 = "missing".into();
    assert!(failed(
        &compare(&input).unwrap(),
        "qualified_sensor_prerequisite"
    ));
    let mut input = fixture();
    input.added_sensor = None;
    assert!(compare(&input).is_err());
}

#[test]
fn replay_nonblind_or_training_overlap_never_promotes() {
    let mut input = fixture();
    input.runs[1].evidence = "replay".into();
    assert!(failed(&compare(&input).unwrap(), "B1_provenance"));
    let mut input = fixture();
    input.labels_adjudicated_before_predictions = false;
    assert!(failed(&compare(&input).unwrap(), "frozen_reviewed_holdout"));
    let mut input = fixture();
    input.holdout_disjoint = false;
    assert!(failed(&compare(&input).unwrap(), "frozen_reviewed_holdout"));
}

#[test]
fn required_quotas_cannot_be_replaced_by_easy_slices_or_fake_noise_speakers() {
    let mut input = fixture();
    let removed = input.cases.remove(0).id;
    for run in &mut input.runs {
        run.lines.retain(|l| l.id != removed);
    }
    rehash(&mut input);
    assert!(failed(&compare(&input).unwrap(), "played_alone_quota"));
    let mut input = fixture();
    for case in &mut input.cases {
        if case.speaker_id.as_deref() == Some("speaker_5") {
            case.speaker_id = Some("speaker_4".into());
        }
        if case.kind == Kind::Noise {
            case.speaker_id = Some("speaker_5".into());
        }
    }
    rehash(&mut input);
    assert!(failed(
        &compare(&input).unwrap(),
        "live_speakers_and_sessions"
    ));
    let mut input = fixture();
    for case in &mut input.cases {
        if case.window_band == Some(WindowBand::Middle) {
            case.window_band = Some(WindowBand::Early);
        }
    }
    rehash(&mut input);
    assert!(failed(&compare(&input).unwrap(), "Middle_window_coverage"));
}

#[test]
fn invalid_duplicates_backwards_time_unknown_fields_and_v1_are_rejected() {
    let mut input = fixture();
    input.cases.push(input.cases[0].clone());
    assert!(compare(&input).is_err());
    let mut input = fixture();
    let duplicate = input.runs[0].lines[0].clone();
    input.runs[0].lines.push(duplicate);
    assert!(compare(&input).is_err());
    let mut input = fixture();
    input.runs[0].lines[0].first_audio_us = Some(input.runs[0].lines[0].speech_end_us - 1);
    assert!(compare(&input).is_err());
    let mut value = serde_json::to_value(fixture()).unwrap();
    value["runs"][0]["lines"][0]["host_duration_us"] = 123.into();
    assert!(serde_json::from_value::<Comparison>(value).is_err());
    let mut input = fixture();
    input.version = 1;
    assert!(compare(&input).is_err());
}

#[test]
fn one_physical_capture_cannot_count_as_several_cases() {
    let mut input = fixture();
    let copied = input.runs[1].lines[0].clone();
    for line in input.runs[1].lines.iter_mut().take(10).skip(1) {
        let id = line.id.clone();
        *line = copied.clone();
        line.id = id;
    }
    assert!(compare(&input).is_err());
}

#[test]
fn independent_cases_cannot_overlap_on_the_same_capture_clock() {
    let mut input = fixture();
    let previous = input.runs[1].lines[0].clone();
    let line = &mut input.runs[1].lines[1];
    line.speech_start_us = previous.speech_start_us + 500_000;
    line.speech_end_us = previous.speech_end_us + 500_000;
    line.previous_lamp_audio_end_us = previous.previous_lamp_audio_end_us.map(|t| t + 500_000);
    line.first_audio_us = previous.first_audio_us.map(|t| t + 500_000);
    line.observation_end_us = previous.observation_end_us.map(|t| t + 500_000);
    assert!(compare(&input).is_err());
}

#[test]
fn schema_spells_out_followup_20s_and_roundtrips_hash_without_precision_loss() {
    let input = fixture();
    let value = serde_json::to_value(&input).unwrap();
    assert_eq!(value["cases"][70]["scenario"], "followup_20s");
    let decoded: Comparison = serde_json::from_value(value).unwrap();
    assert_eq!(cases_hash(&decoded.cases).unwrap(), input.cases_sha256);
    assert!(compare(&decoded).unwrap().passed);
}
