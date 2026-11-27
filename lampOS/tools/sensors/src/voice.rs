use crate::{
    quantile,
    schema::{Result, sha, token},
    score::Check,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VoiceCase {
    pub id: String,
    pub group: String,
    pub expected: String,
    pub kind: String,
    pub mode: String,
}

pub fn played_cases() -> Vec<VoiceCase> {
    serde_json::from_str(include_str!("../played-cases-v1.json"))
        .expect("tested frozen played manifest")
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceLine {
    pub id: String,
    pub observed: String,
    pub heard: bool,
    pub speech_end_us: u64,
    pub first_audio_us: Option<u64>,
    pub interrupt_onset_us: Option<u64>,
    pub last_audio_us: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceRun {
    pub leg: String,
    pub mode: String,
    pub build_sha: String,
    pub other_config_sha256: String,
    pub kit_sha256: String,
    pub scene_manifest_sha256: String,
    pub sensor_enabled: bool,
    pub safety_violation: bool,
    pub lines: Vec<VoiceLine>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Comparison {
    pub version: u32,
    pub baseline_build_sha: String,
    pub candidate_build_sha: String,
    pub changed_switch: String,
    pub kit_sha256: String,
    pub kit_reviewed: bool,
    pub scene_manifest_sha256: String,
    pub other_config_sha256: String,
    pub cases: Vec<VoiceCase>,
    pub runs: Vec<VoiceRun>,
}

#[derive(Debug, Serialize)]
pub struct Group {
    pub leg: String,
    pub mode: String,
    pub group: String,
    pub planned: usize,
    pub correct: usize,
    pub unheard: usize,
    pub missing: usize,
    pub native_scored: usize,
    pub answer_p50_us: Option<u64>,
    pub answer_p95_us: Option<u64>,
    pub stop_p50_us: Option<u64>,
    pub stop_p95_us: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct ComparisonReport {
    pub version: u32,
    pub passed: bool,
    pub checks: Vec<Check>,
    pub groups: Vec<Group>,
}

fn non_regressing(a: Option<u64>, b: Option<u64>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => b <= a,
        (None, None) => true,
        (None, Some(_)) => true,
        (Some(_), None) => false,
    }
}

pub fn compare(input: &Comparison) -> Result<ComparisonReport> {
    if input.version != 1
        || !sha(&input.baseline_build_sha, 40)
        || !sha(&input.candidate_build_sha, 40)
        || !sha(&input.kit_sha256, 64)
        || !sha(&input.scene_manifest_sha256, 64)
        || !sha(&input.other_config_sha256, 64)
        || !token(&input.changed_switch)
        || !input.changed_switch.starts_with("LAMP_")
    {
        return Err("invalid comparison identities or changed switch".into());
    }
    let mut ids = BTreeSet::new();
    for c in &input.cases {
        if !token(&c.id)
            || !token(&c.group)
            || !ids.insert((&c.mode, &c.id))
            || !["A", "Q", "S"].contains(&c.expected.as_str())
            || !["played", "live"].contains(&c.mode.as_str())
            || !["call", "followup", "room", "stop"].contains(&c.kind.as_str())
            || (c.kind == "room" && c.expected != "Q")
            || (c.kind == "stop" && c.expected != "S")
        {
            return Err("invalid/duplicate voice scene manifest".into());
        }
    }
    let mut checks = vec![Check::new(
        "reviewed_played_kit",
        input.kit_reviewed,
        input.kit_sha256.clone(),
    )];
    let expected_played: BTreeMap<_, _> = played_cases()
        .into_iter()
        .map(|c| (c.id.clone(), c))
        .collect();
    let actual_played: BTreeMap<_, _> = input
        .cases
        .iter()
        .filter(|c| c.mode == "played")
        .cloned()
        .map(|c| (c.id.clone(), c))
        .collect();
    checks.push(Check::new(
        "frozen_played_cases",
        actual_played == expected_played,
        format!(
            "{} of {} frozen scored lines; labels/groups must match",
            actual_played.len(),
            expected_played.len()
        ),
    ));
    for kind in ["call", "followup", "room", "stop"] {
        let n = input
            .cases
            .iter()
            .filter(|c| c.mode == "live" && c.kind == kind)
            .count();
        checks.push(Check::new(
            &format!("live_{kind}_quota"),
            n >= 10,
            format!("{n}/10 scenes"),
        ));
    }
    checks.push(Check::new(
        "played_set_nonempty",
        input.cases.iter().any(|c| c.mode == "played"),
        "pinned scene manifest".into(),
    ));
    let mut seen = BTreeSet::new();
    let mut modes: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut groups = Vec::new();
    for run in &input.runs {
        if !["A1", "B1", "B2", "A2"].contains(&run.leg.as_str())
            || !["played", "live"].contains(&run.mode.as_str())
            || !seen.insert((&run.mode, &run.leg))
        {
            return Err("invalid/duplicate A/B/B/A leg".into());
        }
        modes.entry(&run.mode).or_default().push(&run.leg);
        let candidate = run.leg.starts_with('B');
        checks.push(Check::new(&format!("{}_{}_identity", run.mode, run.leg),
            run.build_sha == if candidate { &input.candidate_build_sha } else { &input.baseline_build_sha }.as_str()
                && run.other_config_sha256 == input.other_config_sha256
                && run.kit_sha256 == input.kit_sha256 && run.scene_manifest_sha256 == input.scene_manifest_sha256
                && run.sensor_enabled == candidate && !run.safety_violation,
            "same kit/scenes/other settings; only candidate enables this sensor; no safety violation".into()));
        let mut lines = BTreeMap::new();
        for line in &run.lines {
            if !ids.contains(&(&run.mode, &line.id))
                || lines.insert(&line.id, line).is_some()
                || !["A", "Q", "S", "missing"].contains(&line.observed.as_str())
                || line
                    .first_audio_us
                    .is_some_and(|at| at < line.speech_end_us)
                || line
                    .interrupt_onset_us
                    .zip(line.last_audio_us)
                    .is_some_and(|(a, b)| b < a)
            {
                return Err("invalid/duplicate voice result or timing boundary".into());
            }
        }
        let names: BTreeSet<_> = input
            .cases
            .iter()
            .filter(|c| c.mode == run.mode)
            .map(|c| &c.group)
            .collect();
        for name in names {
            let mut group = Group {
                leg: run.leg.clone(),
                mode: run.mode.clone(),
                group: name.clone(),
                planned: 0,
                correct: 0,
                unheard: 0,
                missing: 0,
                native_scored: 0,
                answer_p50_us: None,
                answer_p95_us: None,
                stop_p50_us: None,
                stop_p95_us: None,
            };
            let mut answers = Vec::new();
            let mut stops = Vec::new();
            let mut timings_complete = true;
            for c in input
                .cases
                .iter()
                .filter(|c| c.mode == run.mode && &c.group == name)
            {
                group.planned += 1;
                let Some(line) = lines.get(&c.id) else {
                    group.missing += 1;
                    continue;
                };
                if line.observed == "missing" {
                    group.missing += 1;
                }
                if !line.heard {
                    group.unheard += 1;
                }
                if c.expected != "Q" || line.heard || line.observed == "A" {
                    group.native_scored += 1;
                }
                let timing_ok = match c.expected.as_str() {
                    "A" => {
                        if let Some(first) = line.first_audio_us {
                            answers.push(first - line.speech_end_us);
                            true
                        } else {
                            false
                        }
                    }
                    "S" => {
                        if let Some((onset, last)) = line.interrupt_onset_us.zip(line.last_audio_us)
                        {
                            stops.push(last - onset);
                            true
                        } else {
                            false
                        }
                    }
                    _ => true,
                };
                if line.observed == c.expected && !timing_ok {
                    timings_complete = false;
                }
                group.correct +=
                    usize::from(line.heard && line.observed == c.expected && timing_ok);
            }
            group.answer_p50_us = quantile(&answers, 50);
            group.answer_p95_us = quantile(&answers, 95);
            group.stop_p50_us = quantile(&stops, 50);
            group.stop_p95_us = quantile(&stops, 95);
            checks.push(Check::new(
                &format!("{}_{}_{}_complete", run.mode, run.leg, name),
                group.missing == 0 && timings_complete,
                format!(
                    "{} missing; timing complete={timings_complete}",
                    group.missing
                ),
            ));
            groups.push(group);
        }
    }
    for mode in ["played", "live"] {
        checks.push(Check::new(
            &format!("{mode}_abba"),
            modes
                .get(mode)
                .is_some_and(|legs| legs == &["A1", "B1", "B2", "A2"]),
            "exactly A1/B1/B2/A2 in recorded order".into(),
        ));
        for (a, b) in [("A1", "B1"), ("A2", "B2")] {
            for baseline in groups.iter().filter(|g| g.mode == mode && g.leg == a) {
                let candidate = groups
                    .iter()
                    .find(|g| g.mode == mode && g.leg == b && g.group == baseline.group);
                let pass = candidate.is_some_and(|g| {
                    g.planned == baseline.planned
                        && g.correct >= baseline.correct
                        && g.unheard <= baseline.unheard
                        && g.missing <= baseline.missing
                        && non_regressing(baseline.answer_p50_us, g.answer_p50_us)
                        && non_regressing(baseline.answer_p95_us, g.answer_p95_us)
                        && non_regressing(baseline.stop_p50_us, g.stop_p50_us)
                        && non_regressing(baseline.stop_p95_us, g.stop_p95_us)
                });
                checks.push(Check::new(
                    &format!("{mode}_{}_{a}_vs_{b}", baseline.group),
                    pass,
                    "fixed denominator; no score/coverage/latency regression in either pair".into(),
                ));
            }
        }
    }
    Ok(ComparisonReport {
        version: 1,
        passed: checks.iter().all(|c| c.passed),
        checks,
        groups,
    })
}
