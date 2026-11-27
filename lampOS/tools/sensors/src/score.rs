use crate::{quantile, schema::*};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Serialize)]
pub struct Check {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

impl Check {
    pub fn new(name: &str, passed: bool, detail: String) -> Self {
        Self {
            name: name.into(),
            passed,
            detail,
        }
    }
}

#[derive(Debug, Serialize, Default)]
pub struct Counts {
    pub correct: usize,
    pub total: usize,
    pub available: usize,
}

#[derive(Debug, Serialize)]
pub struct Row {
    pub case_id: String,
    pub trial_id: String,
    pub session_id: String,
    pub speaker_id: String,
    pub expected: String,
    pub output: String,
    pub raw_output: Option<String>,
    pub classification_correct: bool,
    pub correct: bool,
    pub available: bool,
    pub fault: bool,
    pub cell: String,
    pub latency_us: Option<u64>,
    pub replay_elapsed_us: Option<u64>,
    pub ready_before_first_admission: Option<bool>,
    pub speech_end_to_result_us: Option<u64>,
    pub asr_to_result_us: Option<u64>,
    pub frame_work_us: Option<u64>,
    pub association_us: Option<u64>,
    pub within_admission_budget: bool,
    pub bearing_error_deg: Option<f64>,
    pub face_delay_us: Option<u64>,
    pub angle_correct: bool,
    pub face_correct: bool,
    pub wrong_person: bool,
    pub first_use: bool,
    pub condition: String,
    pub light: String,
    pub noise: String,
    pub distance_mm: u32,
    pub bearing_deg: f64,
    pub overlap: bool,
    pub subject_correct: bool,
    pub subject_visible: bool,
    pub sampling_delay_us: Option<u64>,
    pub opportunity_to_result_us: Option<u64>,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub version: u32,
    pub sensor: Sensor,
    pub manifest_sha256: String,
    pub build_sha: String,
    pub producer_binary_sha256: Option<String>,
    pub source_patch_sha256: Option<String>,
    pub model_sha256: String,
    pub config_sha256: String,
    pub evidence: String,
    pub qualified: bool,
    pub checks: Vec<Check>,
    pub counts: Counts,
    pub raw_classification: Counts,
    pub confusion: BTreeMap<String, BTreeMap<String, usize>>,
    pub slices: BTreeMap<String, Counts>,
    pub timing_us: BTreeMap<String, Option<u64>>,
    pub bearing_error_deg: BTreeMap<String, Option<f64>>,
    pub direction_components: BTreeMap<String, Counts>,
    pub session_bootstrap_95_percent: Option<[f64; 2]>,
    pub false_wakes_per_hour: Option<f64>,
    pub wrong_person_picks: usize,
    pub within_admission_budget: usize,
    pub rows: Vec<Row>,
}

fn tally<'a>(rows: impl Iterator<Item = &'a Row>) -> Counts {
    let mut c = Counts::default();
    for row in rows {
        c.total += 1;
        c.correct += usize::from(row.correct);
        c.available += usize::from(row.available);
    }
    c
}

fn rate<'a>(
    checks: &mut Vec<Check>,
    name: &str,
    rows: impl Iterator<Item = &'a Row>,
    minimum: usize,
    percent: usize,
) {
    let c = tally(rows);
    checks.push(Check::new(
        name,
        c.total >= minimum && c.total > 0 && c.correct * 100 >= c.total * percent,
        format!(
            "{}/{} correct; minimum {minimum}, required {percent}%",
            c.correct, c.total
        ),
    ));
}

fn budget(checks: &mut Vec<Check>, name: &str, values: &[u64], maximum: u64) {
    let p95 = quantile(values, 95);
    checks.push(Check::new(
        name,
        p95.is_some_and(|v| v <= maximum),
        format!("p95 {p95:?} us, maximum {maximum} us, n={}", values.len()),
    ));
}

// Resample whole sessions, preserving within-session dependence. This describes
// this small collection; it is not a population-accuracy guarantee.
fn clustered_interval(rows: &[Row]) -> Option<[f64; 2]> {
    let mut by_session: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for row in rows.iter().filter(|r| !r.fault) {
        let entry = by_session.entry(&row.session_id).or_default();
        entry.0 += usize::from(row.correct);
        entry.1 += 1;
    }
    if by_session.len() < 2 {
        return None;
    }
    let sessions: Vec<_> = by_session.values().collect();
    let mut rng = 8850_u64;
    let mut estimates = Vec::with_capacity(2000);
    for _ in 0..2000 {
        let (mut correct, mut total) = (0, 0);
        for _ in 0..sessions.len() {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let (a, b) = sessions[(rng >> 32) as usize % sessions.len()];
            correct += a;
            total += b;
        }
        estimates.push(correct as f64 / total as f64 * 100.0);
    }
    estimates.sort_by(f64::total_cmp);
    Some([estimates[49], estimates[1949]])
}

pub fn score(manifest: &Manifest, run: &Run, hash: &str) -> Result<Report> {
    manifest.validate()?;
    run.validate(manifest, hash)?;
    let predictions: BTreeMap<_, _> = run.predictions.iter().map(|p| (&p.case_id, p)).collect();
    let mut rows = Vec::new();
    let mut safe = true;
    let mut speakers = BTreeSet::new();
    let mut session_times = BTreeSet::new();
    let mut exposure_us = 0_u64;
    let mut wakes = 0_u64;
    let mut wake_counts_complete = true;
    for c in &manifest.cases {
        let t = manifest.trial(&c.trial_id);
        let session = manifest.session(&t.session_id);
        if c.sensor != run.sensor || session.split != Split::Holdout {
            continue;
        }
        if t.source == "person" {
            speakers.insert(&t.speaker_id);
        }
        session_times.insert(session.started_unix_s);
        if let Some(reference) = &c.reference_trial_id {
            let reference = manifest.trial(reference);
            session_times.insert(manifest.session(&reference.session_id).started_unix_s);
            if reference.source == "person" {
                speakers.insert(&reference.speaker_id);
            }
        }
        let p = predictions.get(&c.id).copied();
        let ready = p.and_then(|p| p.ready_us);
        let latency = p.and_then(|p| p.ready_us.map(|at| at - p.input_end_us));
        let expiry = if run.sensor == Sensor::Addressed {
            100_000
        } else {
            500_000
        };
        let mut available = latency.is_some_and(|v| v <= expiry);
        available &= p.is_some_and(|p| {
            p.input_end_us >= t.start_us
                && (!matches!(
                    run.sensor,
                    Sensor::Call | Sensor::Direction | Sensor::Head | Sensor::SpeakingFace
                ) || p.input_end_us <= t.end_us)
        });
        let camera = matches!(run.sensor, Sensor::Head | Sensor::SpeakingFace);
        let claims_subject = !["unknown", "no_visible"].contains(&c.expected.as_str());
        let subject_correct = !camera
            || !claims_subject
            || p.is_some_and(|p| {
                p.eyes_epoch.is_some()
                    && p.eyes_epoch == t.eyes_epoch
                    && (run.sensor != Sensor::Head || p.subject_face_id == t.subject_face_id)
            });
        if run.sensor == Sensor::Head {
            available &= p
                .zip(c.opportunity_us)
                .is_some_and(|(p, at)| p.input_end_us >= at && p.input_end_us <= t.end_us);
        }
        let asr = p.and_then(|p| p.asr_final_us.zip(p.ready_us).map(|(at, ready)| ready - at));
        if run.sensor == Sensor::Addressed {
            available &= asr.is_some_and(|v| v <= 100_000);
        }
        let association = p.and_then(|p| p.associated_us.map(|at| at - t.end_us));
        if run.sensor == Sensor::SpeakingFace {
            available &= association.is_some_and(|v| v <= 5_000)
                && p.is_some_and(|p| {
                    p.associated_us
                        .is_some_and(|at| at - p.input_end_us <= 500_000)
                        && p.frame_work_us.is_some()
                });
        }
        let output = match p {
            None => "missing".to_owned(),
            Some(_) if !available => "late_or_incomplete".to_owned(),
            Some(p) => p.output.clone(),
        };
        let error = p.and_then(|p| p.bearing_deg).map(|v| {
            let difference = (v - t.bearing_deg).abs();
            difference.min(360.0 - difference)
        });
        let face_delay = p
            .and_then(|p| p.face_in_frame_us)
            .zip(t.call_end_us)
            .map(|(at, end)| at - end);
        let angle_correct = error.is_some_and(|v| v <= 15.0);
        let face_correct = p.is_some_and(|p| {
            p.framed_speaker_id.as_ref() == Some(&t.speaker_id)
                && face_delay.is_some_and(|v| v <= 1_000_000)
                && p.turns == Some(1)
        });
        let mut correct = available && subject_correct && output == c.expected;
        if run.sensor == Sensor::Direction {
            correct &= if c.expected == "turn" {
                angle_correct && face_correct && p.is_some_and(|p| p.natural_motion == Some(true))
            } else {
                p.is_some_and(|p| p.turns == Some(0))
            };
        }
        if run.sensor == Sensor::Call && c.expected == "non_call" {
            exposure_us = exposure_us.saturating_add(t.end_us - t.start_us);
            match p.and_then(|p| p.wake_count) {
                Some(n) => {
                    wakes += u64::from(n);
                    correct &= n == 0;
                }
                None => {
                    wake_counts_complete = false;
                }
            }
        }
        if let Some(p) = p {
            safe &= !p.safety_violation && !p.held_line && p.added_wait_us == 0;
        }
        rows.push(Row {
            case_id: c.id.clone(),
            trial_id: t.id.clone(),
            session_id: t.session_id.clone(),
            speaker_id: t.speaker_id.clone(),
            expected: c.expected.clone(),
            output,
            raw_output: p.map(|p| p.output.clone()),
            classification_correct: subject_correct && p.is_some_and(|p| p.output == c.expected),
            correct,
            available,
            fault: c.fault,
            cell: format!(
                "{}face-{}mm-{}",
                t.visible_faces.len(),
                t.distance_mm,
                t.light
            ),
            latency_us: latency,
            replay_elapsed_us: p.and_then(|p| p.replay_elapsed_us),
            ready_before_first_admission: p.and_then(|p| {
                p.ready_us
                    .zip(p.first_admission_us)
                    .map(|(ready, admission)| ready <= admission)
            }),
            speech_end_to_result_us: ready.map(|at| at.saturating_sub(t.end_us)),
            asr_to_result_us: asr,
            frame_work_us: p.and_then(|p| p.frame_work_us),
            association_us: association,
            within_admission_budget: ready.is_some_and(|at| at <= t.end_us.saturating_add(100_000)),
            bearing_error_deg: error,
            face_delay_us: face_delay,
            angle_correct,
            face_correct,
            wrong_person: run.sensor == Sensor::SpeakingFace
                && p.is_some_and(|p| {
                    !["unknown", "no_visible"].contains(&p.output.as_str())
                        && p.output != c.expected
                }),
            first_use: t.first_use,
            condition: t.condition.clone(),
            light: t.light.clone(),
            noise: t.noise.clone(),
            distance_mm: t.distance_mm,
            bearing_deg: t.bearing_deg,
            overlap: t.overlap,
            subject_correct,
            subject_visible: t.subject_visible,
            sampling_delay_us: p
                .zip(c.opportunity_us)
                .map(|(p, at)| p.input_end_us.saturating_sub(at)),
            opportunity_to_result_us: ready
                .zip(c.opportunity_us)
                .map(|(ready, at)| ready.saturating_sub(at)),
            reason: p.and_then(|p| p.reason.clone()),
        });
    }
    let mut checks = vec![
        Check::new(
            "real_on_body",
            run.evidence == "lamp_live",
            run.evidence.clone(),
        ),
        Check::new(
            "six_holdout_speakers",
            speakers.len() >= 6,
            format!("{} people", speakers.len()),
        ),
        Check::new(
            "sessions_hours_apart",
            session_times
                .first()
                .zip(session_times.last())
                .is_some_and(|(a, b)| b - a >= 7200),
            format!("{} session times", session_times.len()),
        ),
        Check::new(
            "no_holds_waits_or_safety_violations",
            safe,
            "all supplied holdout predictions".into(),
        ),
    ];
    let shared_n = manifest
        .trials
        .iter()
        .filter(|t| manifest.session(&t.session_id).split == Split::Holdout)
        .count();
    checks.push(Check::new(
        "shared_collection",
        shared_n >= 480,
        format!("{shared_n}/480 distinct episodes"),
    ));
    let primary = || rows.iter().filter(|r| !r.fault);
    match run.sensor {
        Sensor::Call | Sensor::Addressed | Sensor::VoiceMatch => {
            let classes = match run.sensor {
                Sensor::Call => [("call", 98), ("non_call", 95)],
                Sensor::Addressed => [("lamp", 95), ("room", 95)],
                _ => [("same", 95), ("different", 95)],
            };
            for (class, threshold) in classes {
                rate(
                    &mut checks,
                    class,
                    primary().filter(|r| r.expected == class),
                    120,
                    threshold,
                );
            }
            if run.sensor == Sensor::Call {
                checks.push(Check::new(
                    "room_wake_counts",
                    exposure_us > 0 && wake_counts_complete,
                    format!("{wakes} wakes in {exposure_us} us of labelled non-call exposure"),
                ));
            }
            if run.sensor == Sensor::VoiceMatch {
                for (source, minimum) in [("person", 80), ("tv", 40)] {
                    let n = primary()
                        .filter(|r| {
                            r.expected == "different"
                                && manifest.trial(&r.trial_id).source == source
                        })
                        .count();
                    checks.push(Check::new(
                        &format!("different_{source}"),
                        n >= minimum,
                        format!("{n}/{minimum} pairs"),
                    ));
                }
                for class in ["same", "different"] {
                    let n = primary()
                        .filter(|r| r.expected == class && r.overlap)
                        .count();
                    checks.push(Check::new(
                        &format!("over_playback_{class}"),
                        n >= 20,
                        format!("{n}/20 pairs"),
                    ));
                }
            }
        }
        Sensor::Head => {
            for class in ["lamp", "screen", "phone"] {
                rate(
                    &mut checks,
                    class,
                    primary().filter(|r| r.expected == class),
                    80,
                    95,
                );
            }
        }
        Sensor::Direction => {
            rate(
                &mut checks,
                "joint_angle_face_time",
                primary().filter(|r| r.expected == "turn"),
                240,
                95,
            );
            rate(
                &mut checks,
                "no_turn_faults",
                rows.iter().filter(|r| r.fault && r.expected == "no_turn"),
                80,
                95,
            );
            for bearing in [-60, -30, 0, 30, 60] {
                for distance in [600, 1200] {
                    for noise in ["quiet", "background"] {
                        let count = primary()
                            .filter(|r| {
                                r.expected == "turn"
                                    && r.bearing_deg == f64::from(bearing)
                                    && r.distance_mm == distance
                                    && r.noise == noise
                            })
                            .count();
                        checks.push(Check::new(
                            &format!("grid_{bearing}_{distance}_{noise}"),
                            count >= 12,
                            format!("{count}/12 calls"),
                        ));
                    }
                }
            }
        }
        Sensor::SpeakingFace => {
            rate(&mut checks, "exact_face_or_abstention", primary(), 320, 95);
            for faces in [1, 2] {
                for distance in [600, 1200] {
                    for light in ["daylight", "lamplight"] {
                        let cell = format!("{faces}face-{distance}mm-{light}");
                        rate(
                            &mut checks,
                            &cell,
                            primary().filter(|r| r.cell == cell),
                            40,
                            95,
                        );
                        if faces == 2 {
                            for slot in 0..2 {
                                let n = primary()
                                    .filter(|r| {
                                        r.cell == cell
                                            && manifest.trial(&r.trial_id).visible_faces.get(slot)
                                                == Some(&r.expected)
                                    })
                                    .count();
                                checks.push(Check::new(
                                    &format!("{cell}_visible_slot_{slot}"),
                                    n >= 12,
                                    format!("{n}/12 labelled speakers in this position"),
                                ));
                            }
                        }
                        for (kind, minimum) in [("visible", 24), ("no_visible", 8), ("unknown", 8)]
                        {
                            let n = primary()
                                .filter(|r| {
                                    r.cell == cell
                                        && if kind == "visible" {
                                            !["no_visible", "unknown"]
                                                .contains(&r.expected.as_str())
                                        } else {
                                            r.expected == kind
                                        }
                                })
                                .count();
                            checks.push(Check::new(
                                &format!("{cell}_{kind}"),
                                n >= minimum,
                                format!("{n}/{minimum} labels"),
                            ));
                        }
                    }
                }
            }
            rate(
                &mut checks,
                "visible_speaker_recall",
                primary().filter(|r| !["no_visible", "unknown"].contains(&r.expected.as_str())),
                192,
                95,
            );
            let picks: Vec<_> = primary()
                .filter(|r| {
                    predictions
                        .get(&r.case_id)
                        .is_some_and(|p| !["no_visible", "unknown"].contains(&p.output.as_str()))
                })
                .collect();
            rate(&mut checks, "face_pick_precision", picks.into_iter(), 1, 95);
        }
    }
    let latency: Vec<_> = primary().filter_map(|r| r.latency_us).collect();
    let limit = match run.sensor {
        Sensor::SpeakingFace => 350_000,
        Sensor::VoiceMatch => 500_000,
        _ if run.location == "server" => 300_000,
        _ => 100_000,
    };
    budget(&mut checks, "input_to_result", &latency, limit);
    if run.sensor == Sensor::Addressed {
        budget(
            &mut checks,
            "final_asr_to_result",
            &primary()
                .filter_map(|r| r.asr_to_result_us)
                .collect::<Vec<_>>(),
            50_000,
        );
    }
    if run.sensor == Sensor::SpeakingFace {
        budget(
            &mut checks,
            "mouth_work_per_frame",
            &primary()
                .filter_map(|r| r.frame_work_us)
                .collect::<Vec<_>>(),
            200_000,
        );
        budget(
            &mut checks,
            "cached_line_association",
            &primary()
                .filter_map(|r| r.association_us)
                .collect::<Vec<_>>(),
            5_000,
        );
    }
    checks.push(Check::new(
        "first_use_recorded",
        rows.iter().any(|r| r.first_use),
        "first-use trials stay in totals".into(),
    ));
    for condition in ["slow", "disconnected"] {
        checks.push(Check::new(
            &format!("{condition}_dependency_recorded"),
            rows.iter().any(|r| r.condition == condition),
            condition.into(),
        ));
    }
    let counts = tally(primary());
    let mut confusion: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut slices: BTreeMap<String, Counts> = BTreeMap::new();
    for row in &rows {
        *confusion
            .entry(row.expected.clone())
            .or_default()
            .entry(row.output.clone())
            .or_default() += 1;
        for key in [
            format!("speaker/{}", row.speaker_id),
            format!("session/{}", row.session_id),
            format!("cell/{}", row.cell),
            format!("noise/{}", row.noise),
            format!("condition/{}", row.condition),
            format!("first_use/{}", row.first_use),
            format!("overlap/{}", row.overlap),
            format!("fault/{}", row.fault),
            format!("subject_visible/{}", row.subject_visible),
            format!("subject_correct/{}", row.subject_correct),
            format!("reason/{}", row.reason.as_deref().unwrap_or("none")),
        ] {
            let c = slices.entry(key).or_default();
            c.correct += usize::from(row.correct);
            c.total += 1;
            c.available += usize::from(row.available);
        }
    }
    let mut timing = BTreeMap::new();
    for (name, values) in [
        ("input_to_result", latency),
        (
            "sampling_delay",
            primary().filter_map(|r| r.sampling_delay_us).collect(),
        ),
        (
            "opportunity_to_result",
            primary()
                .filter_map(|r| r.opportunity_to_result_us)
                .collect(),
        ),
        (
            "speech_end_to_result",
            primary()
                .filter_map(|r| r.speech_end_to_result_us)
                .collect(),
        ),
        (
            "final_asr_to_result",
            primary().filter_map(|r| r.asr_to_result_us).collect(),
        ),
        (
            "call_end_to_face",
            primary().filter_map(|r| r.face_delay_us).collect(),
        ),
    ] {
        for p in [50, 95] {
            timing.insert(format!("{name}_p{p}"), quantile(&values, p));
        }
    }
    let mut bearing_error = BTreeMap::new();
    let mut direction_components = BTreeMap::new();
    if run.sensor == Sensor::Direction {
        let calls: Vec<_> = primary().filter(|r| r.expected == "turn").collect();
        let angles: Vec<_> = calls
            .iter()
            .filter_map(|r| r.bearing_error_deg)
            .map(|v| (v * 1_000_000.0).round() as u64)
            .collect();
        for percentile in [50, 95] {
            bearing_error.insert(
                format!("p{percentile}"),
                quantile(&angles, percentile).map(|v| v as f64 / 1_000_000.0),
            );
        }
        direction_components.insert(
            "angle_within_15_degrees".into(),
            Counts {
                correct: calls.iter().filter(|r| r.angle_correct).count(),
                total: calls.len(),
                available: angles.len(),
            },
        );
        direction_components.insert(
            "correct_face_within_1s_one_turn".into(),
            Counts {
                correct: calls.iter().filter(|r| r.face_correct).count(),
                total: calls.len(),
                available: calls.iter().filter(|r| r.face_delay_us.is_some()).count(),
            },
        );
    }
    Ok(Report {
        version: 1,
        sensor: run.sensor,
        manifest_sha256: hash.into(),
        build_sha: run.build_sha.clone(),
        producer_binary_sha256: run.producer_binary_sha256.clone(),
        source_patch_sha256: run.source_patch_sha256.clone(),
        model_sha256: run.model_sha256.clone(),
        config_sha256: run.config_sha256.clone(),
        evidence: run.evidence.clone(),
        qualified: checks.iter().all(|c| c.passed),
        checks,
        counts,
        raw_classification: Counts {
            correct: rows
                .iter()
                .filter(|r| !r.fault && r.classification_correct)
                .count(),
            total: rows.iter().filter(|r| !r.fault).count(),
            available: rows
                .iter()
                .filter(|r| !r.fault && r.raw_output.is_some())
                .count(),
        },
        confusion,
        slices,
        timing_us: timing,
        bearing_error_deg: bearing_error,
        direction_components,
        session_bootstrap_95_percent: clustered_interval(&rows),
        false_wakes_per_hour: (run.sensor == Sensor::Call
            && exposure_us > 0
            && wake_counts_complete)
            .then(|| wakes as f64 * 3_600_000_000.0 / exposure_us as f64),
        wrong_person_picks: rows.iter().filter(|r| r.wrong_person).count(),
        within_admission_budget: rows
            .iter()
            .filter(|r| !r.fault && r.within_admission_budget)
            .count(),
        rows,
    })
}
