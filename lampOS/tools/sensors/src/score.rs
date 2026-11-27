use crate::{quantile, schema::*};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

const V2_MAX_SYNC_ERROR_US: u64 = 5_000;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub split: Option<Split>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applicable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_origin: Option<DataOrigin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safe_abstention: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock_verified: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_error_us: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_upper_us: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asr_to_result_upper_us: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub association_upper_us: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_latency_upper_us: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible_face_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_shoulders_disagree: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_gold_adjudicated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_latency_us: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physical_outcome_correct: Option<bool>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rubric_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_by_split: Option<BTreeMap<String, RawSummary>>,
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

#[derive(Debug, Serialize, Default)]
pub struct RawSummary {
    pub counts: Counts,
    pub by_class: BTreeMap<String, Counts>,
    pub confusion: BTreeMap<String, BTreeMap<String, usize>>,
    pub by_origin: BTreeMap<String, Counts>,
    pub inapplicable: usize,
    pub faults: usize,
    pub safe_abstentions: usize,
}

fn held_out(row: &Row) -> bool {
    row.split != Some(Split::Development)
}

fn eligible(row: &Row) -> bool {
    !row.fault && row.applicable != Some(false)
}

fn raw_summary<'a>(rows: impl Iterator<Item = &'a Row>) -> RawSummary {
    let mut result = RawSummary::default();
    for row in rows {
        result.safe_abstentions += usize::from(row.safe_abstention == Some(true));
        if row.applicable == Some(false) {
            result.inapplicable += 1;
        } else if row.fault {
            result.faults += 1;
        } else {
            for count in [
                &mut result.counts,
                result.by_class.entry(row.expected.clone()).or_default(),
                result
                    .by_origin
                    .entry(row.data_origin.expect("v2 origin").as_str().into())
                    .or_default(),
            ] {
                count.total += 1;
                count.correct += usize::from(row.classification_correct);
                count.available += usize::from(row.raw_output.is_some());
            }
            *result
                .confusion
                .entry(row.expected.clone())
                .or_default()
                .entry(row.raw_output.as_deref().unwrap_or("missing").to_owned())
                .or_default() += 1;
        }
    }
    result
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
    for row in rows.iter().filter(|r| held_out(r) && eligible(r)) {
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
    let v2 = manifest.version == 2;
    let mut rows = Vec::new();
    let mut safe = true;
    let mut speakers = BTreeSet::new();
    let mut session_times = BTreeSet::new();
    let mut exposure_us = 0_u64;
    let mut wakes = 0_u64;
    let mut wake_counts_complete = true;
    let mut primary_holdout_n = 0;
    let mut real_people = true;
    let mut used_holdout_sessions = BTreeSet::new();
    for c in &manifest.cases {
        let t = manifest.trial(&c.trial_id);
        let session = manifest.session(&t.session_id);
        if c.sensor != run.sensor || (!v2 && session.split != Split::Holdout) {
            continue;
        }
        let holdout = session.split == Split::Holdout;
        let applicable = !v2 || run.sensor != Sensor::SpeakingFace || t.visible_faces.len() >= 2;
        let qualification_case = holdout && applicable && (!v2 || !c.fault);
        if holdout && applicable {
            used_holdout_sessions.insert(session.id.as_str());
        }
        if qualification_case {
            primary_holdout_n += 1;
            real_people &= session.data_origin == Some(DataOrigin::RealPeople);
        }
        if qualification_case && t.source == "person" {
            speakers.insert(&t.speaker_id);
        }
        if qualification_case {
            session_times.insert(session.started_unix_s);
        }
        if let Some(reference) = &c.reference_trial_id
            && qualification_case
        {
            let reference = manifest.trial(reference);
            used_holdout_sessions.insert(reference.session_id.as_str());
            real_people &=
                manifest.session(&reference.session_id).data_origin == Some(DataOrigin::RealPeople);
            session_times.insert(manifest.session(&reference.session_id).started_unix_s);
            if reference.source == "person" {
                speakers.insert(&reference.speaker_id);
            }
        }
        let p = predictions.get(&c.id).copied();
        let ready = p.and_then(|p| p.ready_us);
        let latency = p.and_then(|p| p.ready_us.map(|at| at - p.input_end_us));
        let upper = |value: u64| {
            if v2 {
                value.saturating_add(session.sync_error_us)
            } else {
                value
            }
        };
        let expiry = if run.sensor == Sensor::Addressed {
            100_000
        } else {
            500_000
        };
        let mut available = latency.is_some_and(|v| upper(v) <= expiry)
            && (!v2
                || (session.clock_verified == Some(true)
                    && session.sync_error_us <= V2_MAX_SYNC_ERROR_US));
        available &= p.is_some_and(|p| {
            p.input_end_us >= t.start_us
                && (!matches!(
                    run.sensor,
                    Sensor::Call
                        | Sensor::Direction
                        | Sensor::Head
                        | Sensor::SpeakingFace
                        | Sensor::BargeIn
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
            available &= asr.is_some_and(|v| upper(v) <= 100_000);
        }
        let association = p.and_then(|p| p.associated_us.map(|at| at - t.end_us));
        if run.sensor == Sensor::SpeakingFace {
            available &= association.is_some_and(|v| upper(v) <= 5_000)
                && p.is_some_and(|p| {
                    p.associated_us
                        .is_some_and(|at| upper(at - p.input_end_us) <= 500_000)
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
                && face_delay.is_some_and(|v| upper(v) <= 1_000_000)
                && p.turns == Some(1)
        });
        let expected_output = if run.sensor == Sensor::BargeIn {
            if c.expected == "interrupt" {
                "stop"
            } else {
                "continue"
            }
        } else {
            c.expected.as_str()
        };
        let answerable_output =
            !v2 || !["unknown", "missing", "late_or_incomplete"].contains(&expected_output);
        let classification_correct = applicable
            && answerable_output
            && subject_correct
            && p.is_some_and(|p| p.output == expected_output);
        let mut correct = available && classification_correct;
        let stop_latency = p
            .and_then(|p| p.last_lamp_audio_us)
            .map(|at| at - t.start_us);
        let physical_outcome = (run.sensor == Sensor::BargeIn).then(|| {
            p.is_some_and(|p| {
                if c.expected == "interrupt" {
                    p.playback_stopped == Some(true)
                        && stop_latency.is_some_and(|delay| upper(delay) <= 400_000)
                        && p.last_lamp_audio_us
                            .zip(p.observed_until_us)
                            .is_some_and(|(last, until)| until >= last)
                        && p.last_lamp_audio_us
                            .zip(t.planned_lamp_audio_end_us)
                            .is_some_and(|(last, planned)| last < planned)
                } else {
                    p.playback_stopped == Some(false)
                        && p.last_lamp_audio_us
                            .zip(t.planned_lamp_audio_end_us)
                            .is_some_and(|(last, planned)| last >= planned)
                        && p.observed_until_us
                            .zip(c.observation_end_us)
                            .is_some_and(|(until, required)| until >= required)
                }
            })
        });
        if let Some(physical) = physical_outcome {
            correct &= physical;
        }
        if run.sensor == Sensor::Direction {
            correct &= if c.expected == "turn" {
                angle_correct && face_correct && p.is_some_and(|p| p.natural_motion == Some(true))
            } else {
                p.is_some_and(|p| p.turns == Some(0))
            };
        }
        if run.sensor == Sensor::Call && c.expected == "non_call" && holdout {
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
        if let Some(p) = p
            && holdout
        {
            safe &= !p.safety_violation && !p.held_line && p.added_wait_us == 0;
        }
        rows.push(Row {
            split: v2.then_some(session.split),
            applicable: v2.then_some(applicable),
            data_origin: session.data_origin,
            clock_verified: v2.then_some(session.clock_verified == Some(true)),
            sync_error_us: v2.then_some(session.sync_error_us),
            latency_upper_us: v2.then_some(latency.map(upper)).flatten(),
            asr_to_result_upper_us: v2.then_some(asr.map(upper)).flatten(),
            association_upper_us: v2.then_some(association.map(upper)).flatten(),
            stop_latency_upper_us: (v2
                && run.sensor == Sensor::BargeIn
                && c.expected == "interrupt")
                .then_some(stop_latency.map(upper))
                .flatten(),
            safe_abstention: v2.then_some(
                applicable
                    && c.fault
                    && c.expected == "unknown"
                    && available
                    && p.is_some_and(|p| {
                        p.output == "unknown"
                            && !p.held_line
                            && p.added_wait_us == 0
                            && !p.safety_violation
                    }),
            ),
            visible_face_count: v2.then_some(t.visible_faces.len()),
            head_shoulders_disagree: (v2 && run.sensor == Sensor::Head).then_some(matches!(
                (t.head_target.as_str(), t.shoulders_target.as_deref()),
                ("lamp", Some("away")) | ("away", Some("lamp"))
            )),
            head_gold_adjudicated: (v2 && run.sensor == Sensor::Head)
                .then_some(c.head_gold_adjudicated == Some(true)),
            stop_latency_us: (run.sensor == Sensor::BargeIn && c.expected == "interrupt")
                .then_some(stop_latency)
                .flatten(),
            physical_outcome_correct: physical_outcome,
            case_id: c.id.clone(),
            trial_id: t.id.clone(),
            session_id: t.session_id.clone(),
            speaker_id: t.speaker_id.clone(),
            expected: c.expected.clone(),
            output,
            raw_output: p.map(|p| p.output.clone()),
            classification_correct,
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
                    .map(|(ready, admission)| upper(ready) <= admission)
            }),
            speech_end_to_result_us: ready.map(|at| at.saturating_sub(t.end_us)),
            asr_to_result_us: asr,
            frame_work_us: p.and_then(|p| p.frame_work_us),
            association_us: association,
            within_admission_budget: ready
                .is_some_and(|at| upper(at) <= t.end_us.saturating_add(100_000)),
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
    if v2 {
        checks.push(Check::new(
            "producing_binary_recorded",
            run.producer_binary_sha256.is_some(),
            "v2 qualification identifies the exact producing executable".into(),
        ));
        checks.push(Check::new(
            "verified_source_clocks",
            !used_holdout_sessions.is_empty()
                && used_holdout_sessions.iter().all(|id| {
                    let session = manifest.session(id);
                    session.clock_verified == Some(true)
                        && session.sync_error_us <= V2_MAX_SYNC_ERROR_US
                }),
            format!(
                "{} used holdout sessions; verified mapping and error <=5000 us required",
                used_holdout_sessions.len()
            ),
        ));
        checks.push(Check::new("real_people_primary_holdout", primary_holdout_n > 0 && real_people,
            format!("{primary_holdout_n} primary holdout cases; all probe/reference origins real_people: {real_people}")));
    }
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
    let primary = || rows.iter().filter(|r| held_out(r) && eligible(r));
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
            let classes: &[&str] = if v2 {
                &["facing", "away"]
            } else {
                &["lamp", "screen", "phone"]
            };
            for class in classes {
                rate(
                    &mut checks,
                    class,
                    primary().filter(|r| r.expected == *class),
                    if v2 { 120 } else { 80 },
                    95,
                );
            }
            if v2 {
                for (head, shoulders) in [("lamp", "away"), ("away", "lamp")] {
                    let n = primary()
                        .filter(|r| {
                            let trial = manifest.trial(&r.trial_id);
                            trial.head_target == head
                                && trial.shoulders_target.as_deref() == Some(shoulders)
                        })
                        .count();
                    checks.push(Check::new(
                        &format!("head_{head}_shoulders_{shoulders}"),
                        n >= 20,
                        format!("{n}/20 primary disagreement cases"),
                    ));
                }
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
                rows.iter()
                    .filter(|r| held_out(r) && r.fault && r.expected == "no_turn"),
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
            rate(
                &mut checks,
                if v2 {
                    "exact_face_or_no_visible"
                } else {
                    "exact_face_or_abstention"
                },
                primary(),
                320,
                95,
            );
            if v2 {
                let answerable = primary().filter(|r| r.expected != "unknown").count();
                checks.push(Check::new(
                    "answerable_lines",
                    answerable >= 320,
                    format!("{answerable}/320 answerable lines with at least two visible faces"),
                ));
                for distance in [600, 1200] {
                    for light in ["daylight", "lamplight"] {
                        rate(
                            &mut checks,
                            &format!("2plus_faces-{distance}mm-{light}"),
                            primary().filter(|r| r.distance_mm == distance && r.light == light),
                            80,
                            95,
                        );
                        let in_cell = |r: &&Row| r.distance_mm == distance && r.light == light;
                        for slot in 0..2 {
                            let n = primary()
                                .filter(in_cell)
                                .filter(|r| {
                                    manifest.trial(&r.trial_id).visible_faces.get(slot)
                                        == Some(&r.expected)
                                })
                                .count();
                            checks.push(Check::new(
                                &format!("2plus_faces-{distance}mm-{light}_visible_slot_{slot}"),
                                n >= 24,
                                format!("{n}/24 clear labelled speakers in this position"),
                            ));
                        }
                        let no_visible = primary()
                            .filter(in_cell)
                            .filter(|r| r.expected == "no_visible")
                            .count();
                        checks.push(Check::new(
                            &format!("2plus_faces-{distance}mm-{light}_no_visible"),
                            no_visible >= 16,
                            format!("{no_visible}/16 off-camera speaker lines"),
                        ));
                    }
                }
            } else {
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
                                                && manifest
                                                    .trial(&r.trial_id)
                                                    .visible_faces
                                                    .get(slot)
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
                            for (kind, minimum) in
                                [("visible", 24), ("no_visible", 8), ("unknown", 8)]
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
        Sensor::BargeIn => {
            for class in ["interrupt", "backchannel", "side_talk", "noise"] {
                rate(
                    &mut checks,
                    class,
                    primary().filter(|r| r.expected == class),
                    120,
                    95,
                );
            }
        }
    }
    let latency: Vec<_> = primary().filter_map(|r| r.latency_us).collect();
    let limit = match run.sensor {
        Sensor::SpeakingFace => 350_000,
        Sensor::VoiceMatch => 500_000,
        _ if run.location == "server" => 300_000,
        _ => 100_000,
    };
    let latency_upper: Vec<_> = primary().filter_map(|r| r.latency_upper_us).collect();
    budget(
        &mut checks,
        "input_to_result",
        if v2 { &latency_upper } else { &latency },
        limit,
    );
    if run.sensor == Sensor::Addressed {
        budget(
            &mut checks,
            "final_asr_to_result",
            &primary()
                .filter_map(|r| {
                    if v2 {
                        r.asr_to_result_upper_us
                    } else {
                        r.asr_to_result_us
                    }
                })
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
                .filter_map(|r| {
                    if v2 {
                        r.association_upper_us
                    } else {
                        r.association_us
                    }
                })
                .collect::<Vec<_>>(),
            5_000,
        );
    }
    checks.push(Check::new(
        "first_use_recorded",
        rows.iter()
            .any(|r| held_out(r) && (!v2 || eligible(r)) && r.first_use),
        "first-use trials stay in totals".into(),
    ));
    for condition in ["slow", "disconnected"] {
        checks.push(Check::new(
            &format!("{condition}_dependency_recorded"),
            rows.iter()
                .any(|r| held_out(r) && r.applicable != Some(false) && r.condition == condition),
            condition.into(),
        ));
    }
    let counts = tally(primary());
    let mut confusion: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut slices: BTreeMap<String, Counts> = BTreeMap::new();
    for row in rows.iter().filter(|row| held_out(row)) {
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
        if v2 {
            let mut keys = vec![
                format!("applicable/{}", row.applicable.unwrap_or(true)),
                format!("visible_faces/{}", row.visible_face_count.unwrap_or(0)),
            ];
            if let Some(disagree) = row.head_shoulders_disagree {
                keys.push(format!("head_shoulders_disagree/{disagree}"));
                keys.push(format!(
                    "head_gold_adjudicated/{}",
                    row.head_gold_adjudicated.unwrap_or(false)
                ));
            }
            if let Some(physical) = row.physical_outcome_correct {
                keys.push(format!("physical_outcome_correct/{physical}"));
            }
            for key in keys {
                let c = slices.entry(key).or_default();
                c.total += 1;
                c.correct += usize::from(row.correct);
                c.available += usize::from(row.available);
            }
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
    if run.sensor == Sensor::BargeIn {
        let stops: Vec<_> = primary()
            .filter(|r| r.expected == "interrupt")
            .filter_map(|r| r.stop_latency_us)
            .collect();
        for p in [50, 95] {
            timing.insert(
                format!("interrupt_onset_to_last_audio_p{p}"),
                quantile(&stops, p),
            );
        }
    }
    if v2 {
        for (name, values) in [
            ("input_to_result_upper", latency_upper),
            (
                "final_asr_to_result_upper",
                primary().filter_map(|r| r.asr_to_result_upper_us).collect(),
            ),
            (
                "cached_line_association_upper",
                primary().filter_map(|r| r.association_upper_us).collect(),
            ),
            (
                "interrupt_onset_to_last_audio_upper",
                primary().filter_map(|r| r.stop_latency_upper_us).collect(),
            ),
            (
                "call_end_to_face_upper",
                primary()
                    .filter_map(|r| {
                        r.face_delay_us
                            .zip(r.sync_error_us)
                            .map(|(delay, error)| delay.saturating_add(error))
                    })
                    .collect(),
            ),
        ] {
            for p in [50, 95] {
                timing.insert(format!("{name}_p{p}"), quantile(&values, p));
            }
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
        version: manifest.version,
        rubric_id: manifest.rubric_id.clone(),
        raw_by_split: v2.then(|| {
            [Split::Development, Split::Holdout]
                .into_iter()
                .map(|split| {
                    let name = match split {
                        Split::Development => "development",
                        Split::Holdout => "holdout",
                    };
                    (
                        name.into(),
                        raw_summary(rows.iter().filter(|r| r.split == Some(split))),
                    )
                })
                .collect()
        }),
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
                .filter(|r| held_out(r) && eligible(r) && r.classification_correct)
                .count(),
            total: primary().count(),
            available: rows
                .iter()
                .filter(|r| held_out(r) && eligible(r) && r.raw_output.is_some())
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
        wrong_person_picks: rows
            .iter()
            .filter(|r| held_out(r) && r.wrong_person)
            .count(),
        within_admission_budget: rows
            .iter()
            .filter(|r| held_out(r) && eligible(r) && r.within_admission_budget)
            .count(),
        rows,
    })
}
