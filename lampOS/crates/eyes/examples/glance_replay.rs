//! Replays `lamp-eyes --glance-log` files offline and scores the change
//! bar's rules on them: `cargo run -q -p lamp-eyes --example glance_replay
//! LOG...`. For each log: the glances, the share of empty-room glances each
//! rule would skip, and, if a face was found, how many looks after the first
//! glance with a face the rule would have run the detector (0 or 1 is within
//! one look).
//!
//! The rules:
//! - `current`: the log's own mask (12 greys and 8x the cell's noise, after
//!   the row's median change), as the eyes judged it;
//! - `persist-2`: a cell counts only if it was past the bar in this glance
//!   and the one before;
//! - `ratio-12`, `ratio-16`: 12 greys and 12 or 16 times the noise, from
//!   the logged ratio;
//! - `learn R/F`: the noise learned again from the logged changes in greys,
//!   rising by R and falling by F a glance, at 8 times.
//!
//! What a replay cannot know: a rule that would have run the detector on a
//! different glance would have compared later glances to a different frame;
//! the log's changes are all against the frames the eyes really ran on.
use std::collections::BTreeSet;

const CELLS: usize = 768;
/// The change bar's constants, as in `lamp_eyes::change`.
const GREYS: u8 = 12;
const CHANGED_CELLS: usize = 3;
const SETTLE_GLANCES: u32 = 2;
const LOOK_AT_LEAST_US: u64 = 3_000_000;
const RATIO_STEPS: f32 = 8.0;
const NOISE_TIMES: f32 = 8.0;

#[derive(Debug, Clone, PartialEq)]
struct Glance {
    at_us: u64,
    lit: bool,
    detect: bool,
    why: String,
    faces: u64,
    mask: Vec<bool>,
    ratio: Vec<u8>,
    resid: Vec<u8>,
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    text.len()
        .is_multiple_of(2)
        .then(|| {
            (0..text.len())
                .step_by(2)
                .map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok())
                .collect()
        })
        .flatten()
}

fn parse(text: &str) -> Result<Vec<Glance>, String> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(n, line)| {
            let bad = || format!("line {} isn't a glance", n + 1);
            let value: serde_json::Value = serde_json::from_str(line).map_err(|_| bad())?;
            let bytes = |key: &str, length: usize| {
                value[key]
                    .as_str()
                    .and_then(unhex)
                    .filter(|bytes| bytes.len() == length)
                    .ok_or_else(bad)
            };
            let mask = bytes("mask", CELLS / 8)?;
            Ok(Glance {
                at_us: value["at_us"].as_u64().ok_or_else(bad)?,
                lit: value["lit"].as_bool().ok_or_else(bad)?,
                detect: value["detect"].as_bool().ok_or_else(bad)?,
                why: value["why"].as_str().ok_or_else(bad)?.to_owned(),
                faces: value["faces"].as_u64().ok_or_else(bad)?,
                mask: (0..CELLS)
                    .map(|cell| mask[cell / 8] >> (cell % 8) & 1 == 1)
                    .collect(),
                ratio: bytes("ratio", CELLS)?,
                resid: bytes("resid_greys_q", CELLS)?,
            })
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Rule {
    Current,
    Persist2,
    Ratio(u8),
    Learn(f32, f32),
}

impl Rule {
    fn name(self) -> String {
        match self {
            Rule::Current => "current".to_owned(),
            Rule::Persist2 => "persist-2".to_owned(),
            Rule::Ratio(times) => format!("ratio-{times}"),
            Rule::Learn(rise, fall) => format!("learn {rise}/{fall}"),
        }
    }
}

const RULES: [Rule; 7] = [
    Rule::Current,
    Rule::Persist2,
    Rule::Ratio(12),
    Rule::Ratio(16),
    Rule::Learn(0.01, 0.05),
    Rule::Learn(0.03, 0.05),
    Rule::Learn(0.05, 0.1),
];

fn busy(why: &str) -> bool {
    matches!(why, "face" | "asked" | "remembering")
}

/// Whether the rule runs the detector at each glance. With `face_from`,
/// glances from that one on that were busy only for a face are judged as
/// if the room were empty: when would the rule itself have looked.
fn replay(rule: Rule, glances: &[Glance], face_from: Option<usize>) -> Vec<bool> {
    let mut noise = vec![0f32; CELLS];
    let mut since_change: Option<u32> = None;
    // The glance before the first logged one ran the detector (the first
    // glance, or the one compared against).
    let mut last_at = glances.first().map_or(0, |glance| glance.at_us);
    let mut detects = Vec::with_capacity(glances.len());
    for (n, glance) in glances.iter().enumerate() {
        let busy =
            busy(&glance.why) && !(glance.why == "face" && face_from.is_some_and(|from| n >= from));
        let cells = (0..CELLS)
            .filter(|&cell| match rule {
                Rule::Current => glance.mask[cell],
                Rule::Persist2 => glance.mask[cell] && n > 0 && glances[n - 1].mask[cell],
                Rule::Ratio(times) => {
                    glance.resid[cell] >= GREYS
                        && f32::from(glance.ratio[cell]) >= f32::from(times) * RATIO_STEPS
                }
                Rule::Learn(..) => {
                    let size = f32::from(glance.resid[cell]);
                    size >= f32::from(GREYS) && size >= NOISE_TIMES * noise[cell]
                }
            })
            .count();
        let changed = !glance.lit && cells >= CHANGED_CELLS;
        since_change = if changed {
            Some(0)
        } else {
            since_change.map(|still| still.saturating_add(1))
        };
        let moving = since_change.is_some_and(|still| still <= SETTLE_GLANCES);
        let due = glance.at_us.saturating_sub(last_at) >= LOOK_AT_LEAST_US;
        let detect = busy || moving || due;
        if detect {
            last_at = glance.at_us;
        }
        if let Rule::Learn(rise, fall) = rule
            && !busy
            && !glance.lit
        {
            for (noise, size) in noise.iter_mut().zip(&glance.resid) {
                let towards = f32::from(*size) - *noise;
                *noise += towards * if towards > 0.0 { rise } else { fall };
            }
        }
        detects.push(detect);
    }
    detects
}

#[derive(Debug, PartialEq)]
struct Score {
    rule: String,
    empty: usize,
    skipped: usize,
    /// Looks from the first glance with a face until the rule detects.
    face_late: Option<usize>,
}

fn score(glances: &[Glance]) -> Vec<Score> {
    let first_face = glances.iter().position(|glance| glance.faces > 0);
    RULES
        .iter()
        .map(|&rule| {
            let detects = replay(rule, glances, None);
            let empty: Vec<bool> = glances
                .iter()
                .zip(&detects)
                .filter(|(glance, _)| !busy(&glance.why))
                .map(|(_, detect)| *detect)
                .collect();
            let face_late = first_face.and_then(|first| {
                replay(rule, glances, Some(first))[first..]
                    .iter()
                    .position(|detect| *detect)
            });
            Score {
                rule: rule.name(),
                empty: empty.len(),
                skipped: empty.iter().filter(|detect| !**detect).count(),
                face_late,
            }
        })
        .collect()
}

fn main() -> Result<(), String> {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        return Err("usage: glance_replay LOG...".to_owned());
    }
    for path in paths {
        let text = std::fs::read_to_string(&path).map_err(|error| format!("{path}: {error}"))?;
        let glances = parse(&text).map_err(|error| format!("{path}: {error}"))?;
        let logged = glances
            .iter()
            .filter(|glance| !busy(&glance.why) && !glance.detect)
            .count();
        let whys: BTreeSet<&str> = glances.iter().map(|glance| glance.why.as_str()).collect();
        let face = glances.iter().position(|glance| glance.faces > 0);
        println!(
            "{path}: {} glances, {} skipped as logged, whys {whys:?}, first face {}",
            glances.len(),
            logged,
            face.map_or("none".to_owned(), |at| format!("at glance {at}"))
        );
        for score in score(&glances) {
            let share = 100.0 * score.skipped as f64 / score.empty.max(1) as f64;
            let face = match (face, score.face_late) {
                (None, _) => String::new(),
                (Some(_), Some(late)) if late <= 1 => {
                    format!(", face detected {late} look(s) after it appeared (within one)")
                }
                (Some(_), Some(late)) => {
                    format!(", face detected {late} looks after it appeared (LATE)")
                }
                (Some(_), None) => ", face never detected in the log (MISSED)".to_owned(),
            };
            println!(
                "  {:<14} skipped {:>5}/{:<5} empty-room glances ({share:.1}%){face}",
                score.rule, score.skipped, score.empty
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glance(at_us: u64, cells: &[usize], why: &str, faces: u64) -> String {
        let mut mask = [0u8; CELLS / 8];
        for cell in cells {
            mask[cell / 8] |= 1 << (cell % 8);
        }
        let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let mut ratio = [0u8; CELLS];
        let mut resid = [0u8; CELLS];
        for &cell in cells {
            ratio[cell] = 100;
            resid[cell] = 20;
        }
        serde_json::json!({
            "at_us": at_us, "changed": cells.len() >= 3, "lit": false, "cells": cells.len(),
            "mask": hex(&mask), "ratio": hex(&ratio), "resid_greys_q": hex(&resid),
            "frame_shift": 0.0, "detect": true, "why": why, "faces": faces,
        })
        .to_string()
    }

    #[test]
    fn persisting_cells_skip_a_flicker_and_still_find_a_face() {
        // A flicker in three cells for one glance, a quiet room, then a
        // person in the same cells for two glances, found on the second.
        let lines = [
            glance(600_000, &[], "skipped", 0),
            glance(1_200_000, &[1, 2, 3], "changed", 0),
            glance(1_800_000, &[], "settling", 0),
            glance(2_400_000, &[], "settling", 0),
            glance(5_400_000, &[], "every", 0),
            glance(6_000_000, &[10, 11, 12], "changed", 0),
            glance(6_600_000, &[10, 11, 12], "changed", 1),
            glance(7_200_000, &[], "face", 1),
        ]
        .join("\n");
        let glances = parse(&lines).unwrap();
        assert_eq!(glances.len(), 8);
        let scores = score(&glances);
        let current = &scores[0];
        assert_eq!((current.empty, current.skipped), (7, 1));
        assert_eq!(current.face_late, Some(0));
        let persist = &scores[1];
        assert_eq!(persist.rule, "persist-2");
        // The flicker and its two settling glances are skipped too, and
        // the person's first glance, found at their second.
        assert_eq!(persist.skipped, 5);
        assert_eq!(persist.face_late, Some(0));
        assert!(parse("{\"at_us\":1}").is_err());
    }
}
