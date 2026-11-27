//! Diagnose the existing mouth journal offline (#8848):
//! `cargo run -q -p lamp-eyes --example speaking_replay -- JOURNAL.jsonl LINES.jsonl`.
//! See docs/speaking-face-sensor.md for the clocks, input and limits.
//! This reports the producer's decisions, never re-scores mouths or infers
//! a line's speaker from the largest anonymous face.
use lamp_eyes::bus_out::MouthObservation;
use lamp_msgs::SpeakingState;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{self, BufRead, BufReader, Read},
    process::ExitCode,
};

const PREFIX: &str = "lamp-eyes: mouth diagnostic ";
const MAX_ROW_BYTES: u64 = 65_536;
const MAX_JOURNAL_ROWS: usize = 1_000_000;
const MAX_OBSERVATIONS: usize = 100_000;
const MAX_LINES: usize = 10_000;

#[derive(Deserialize)]
struct JournalRow {
    #[serde(rename = "MESSAGE", default)]
    message: serde_json::Value,
    #[serde(rename = "_BOOT_ID", default)]
    boot: Option<String>,
    #[serde(rename = "_PID", default)]
    pid: Option<String>,
}

/// A numeric export of a voice line, not the Reviewer's gold label.
/// Missing `seen_speaking` is unreported, not false.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Line {
    id: u64,
    boot_id: String,
    from_us: u64,
    to_us: u64,
    seen_speaking: Option<bool>,
}

#[derive(Debug)]
struct Observation {
    pid: u32,
    mouth: MouthObservation,
}

#[derive(Debug, Default)]
struct Journal {
    boots: BTreeMap<String, Vec<Observation>>,
    observations: usize,
    duplicate_observations: usize,
    sha256: String,
}

#[derive(Debug, Default, Serialize)]
struct States {
    yes: usize,
    no: usize,
    unknown: usize,
}

impl States {
    fn add(&mut self, state: SpeakingState) {
        match state {
            SpeakingState::Yes => self.yes += 1,
            SpeakingState::No => self.no += 1,
            SpeakingState::Unknown => self.unknown += 1,
        }
    }
}

#[derive(Debug, Serialize)]
struct LineReport {
    id: u64,
    from_us: u64,
    to_us: u64,
    voice_seen_speaking: Option<bool>,
    diagnostic_looks: usize,
    largest_face_states: States,
    reasons: BTreeMap<String, usize>,
    heads: BTreeMap<String, usize>,
    multi_face_looks: usize,
    mesh_missing_looks: usize,
    window_resets: usize,
    eyes_processes: usize,
    /// Only observed gaps within one process; missing looks cannot be
    /// counted from this journal, which logs no zero-face observations.
    max_observed_gap_us: Option<u64>,
    /// The age logged BEFORE the look's slow work, not inference latency.
    max_logged_frame_age_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
struct Report {
    kind: &'static str,
    time_basis: &'static str,
    journal_sha256: String,
    lines_sha256: String,
    observations: usize,
    duplicate_observations: usize,
    lines: usize,
    lines_without_diagnostics: usize,
    voice_seen: usize,
    voice_not_seen: usize,
    voice_unreported: usize,
    per_line: Vec<LineReport>,
    limitations: [&'static str; 4],
}

/// Read one bounded row at a time. Error messages never echo an input
/// value: an unrelated journal message may contain a person's words.
fn rows(
    mut input: impl BufRead,
    limit: usize,
    mut visit: impl FnMut(&str, usize) -> Result<(), String>,
) -> Result<String, String> {
    let mut digest = Sha256::new();
    let mut row = Vec::new();
    for n in 1..=limit + 1 {
        row.clear();
        (&mut input)
            .take(MAX_ROW_BYTES + 1)
            .read_until(b'\n', &mut row)
            .map_err(|_| format!("row {n}: read failed"))?;
        if row.is_empty() {
            return Ok(format!("{:x}", digest.finalize()));
        }
        if n > limit || row.len() as u64 > MAX_ROW_BYTES {
            return Err(format!("row {n}: input limit exceeded"));
        }
        digest.update(&row);
        let text = std::str::from_utf8(&row)
            .map_err(|_| format!("row {n}: expected UTF-8"))?
            .trim();
        if !text.is_empty() {
            visit(text, n)?;
        }
    }
    unreachable!("the extra row returns an input limit error")
}

fn boot_valid(boot: &str) -> bool {
    boot.len() == 32 && boot.bytes().all(|b| b.is_ascii_hexdigit())
}

fn observation_valid(mouth: &MouthObservation) -> bool {
    let detail = &mouth.decision;
    mouth.face_count > 0
        && mouth.filled.iter().all(|v| *v > 0)
        && mouth.face_box_px.iter().all(|v| v.is_finite())
        && mouth.face_box_px[2] > 0.0
        && mouth.face_box_px[3] > 0.0
        && [
            detail.eyes_px,
            detail.window_mean_eyes_px,
            detail.turn_deg,
            detail.jitter_rad,
            detail.mouth_open,
            Some(detail.speaking.score),
        ]
        .into_iter()
        .flatten()
        .all(f32::is_finite)
}

fn journal(input: impl BufRead) -> Result<Journal, String> {
    // Deduplicate a repeated export by boot, process and judgement stamp;
    // never join monotonic clocks from different operating-system boots.
    let mut unique = BTreeMap::new();
    let mut duplicate_observations = 0;
    let sha256 = rows(input, MAX_JOURNAL_ROWS, |text, n| {
        let row: JournalRow = serde_json::from_str(text)
            .map_err(|_| format!("journal row {n}: invalid journal JSON"))?;
        let Some(payload) = row.message.as_str().and_then(|m| m.strip_prefix(PREFIX)) else {
            return Ok(());
        };
        let boot = row
            .boot
            .filter(|v| boot_valid(v))
            .map(|v| v.to_ascii_lowercase())
            .ok_or_else(|| format!("journal row {n}: missing or invalid _BOOT_ID"))?;
        let pid = row
            .pid
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|v| *v > 0)
            .ok_or_else(|| format!("journal row {n}: missing or invalid _PID"))?;
        let mouth: MouthObservation = serde_json::from_str(payload)
            .map_err(|_| format!("journal row {n}: invalid mouth diagnostic"))?;
        if !observation_valid(&mouth) {
            return Err(format!("journal row {n}: invalid numeric observation"));
        }
        let key = (boot, mouth.at_us, pid);
        if let Some(previous) = unique.get(&key) {
            if previous != &mouth {
                return Err(format!(
                    "journal row {n}: conflicting duplicate observation"
                ));
            }
            duplicate_observations += 1;
        } else {
            if unique.len() >= MAX_OBSERVATIONS {
                return Err(format!("journal row {n}: too many observations"));
            }
            unique.insert(key, mouth);
        }
        Ok(())
    })?;
    let mut result = Journal {
        observations: unique.len(),
        duplicate_observations,
        sha256,
        ..Journal::default()
    };
    for ((boot, _, pid), mouth) in unique {
        result
            .boots
            .entry(boot)
            .or_default()
            .push(Observation { pid, mouth });
    }
    Ok(result)
}

fn lines(input: impl BufRead) -> Result<(Vec<Line>, String), String> {
    let mut result = Vec::new();
    let mut ids = BTreeSet::new();
    let sha256 = rows(input, MAX_LINES, |text, n| {
        let mut line: Line = serde_json::from_str(text)
            .map_err(|_| format!("line row {n}: invalid numeric line export"))?;
        if !boot_valid(&line.boot_id) || line.from_us >= line.to_us || !ids.insert(line.id) {
            return Err(format!(
                "line row {n}: invalid boot, interval or duplicate id"
            ));
        }
        line.boot_id.make_ascii_lowercase();
        result.push(line);
        Ok(())
    })?;
    if result.is_empty() {
        return Err("line export is empty".to_owned());
    }
    Ok((result, sha256))
}

fn count_word(counts: &mut BTreeMap<String, usize>, value: impl Serialize) {
    if let Ok(serde_json::Value::String(word)) = serde_json::to_value(value) {
        *counts.entry(word).or_default() += 1;
    }
}

fn summarize_line(line: &Line, journal: &Journal) -> LineReport {
    let all = journal
        .boots
        .get(&line.boot_id)
        .map_or(&[][..], Vec::as_slice);
    let start = all.partition_point(|o| o.mouth.at_us < line.from_us);
    let end = all.partition_point(|o| o.mouth.at_us <= line.to_us);
    let looks = &all[start..end];
    let mut result = LineReport {
        id: line.id,
        from_us: line.from_us,
        to_us: line.to_us,
        voice_seen_speaking: line.seen_speaking,
        diagnostic_looks: looks.len(),
        largest_face_states: States::default(),
        reasons: BTreeMap::new(),
        heads: BTreeMap::new(),
        multi_face_looks: 0,
        mesh_missing_looks: 0,
        window_resets: 0,
        eyes_processes: 0,
        max_observed_gap_us: None,
        max_logged_frame_age_ms: None,
    };
    let mut last_by_pid = BTreeMap::new();
    for look in looks {
        let detail = &look.mouth.decision;
        result.largest_face_states.add(detail.speaking.state);
        count_word(&mut result.reasons, detail.reason);
        if let Some(head) = detail.head {
            count_word(&mut result.heads, head);
        } else {
            *result.heads.entry("unreported".to_owned()).or_default() += 1;
        }
        result.multi_face_looks += usize::from(look.mouth.face_count > 1);
        result.mesh_missing_looks += usize::from(!detail.mesh_current);
        result.window_resets += usize::from(detail.window_reset);
        result.max_logged_frame_age_ms = result.max_logged_frame_age_ms.max(detail.frame_age_ms);
        if let Some(last) = last_by_pid.insert(look.pid, look.mouth.at_us) {
            result.max_observed_gap_us = result
                .max_observed_gap_us
                .max(Some(look.mouth.at_us - last));
        }
    }
    result.eyes_processes = last_by_pid.len();
    result
}

fn report(journal_input: impl BufRead, line_input: impl BufRead) -> Result<Report, String> {
    let journal = journal(journal_input)?;
    let (lines, lines_sha256) = lines(line_input)?;
    let per_line: Vec<_> = lines
        .iter()
        .map(|line| summarize_line(line, &journal))
        .collect();
    Ok(Report {
        kind: "diagnostic_summary_not_accuracy",
        time_basis: "mouth_judgement_monotonic_us_within_os_boot",
        journal_sha256: journal.sha256,
        lines_sha256,
        observations: journal.observations,
        duplicate_observations: journal.duplicate_observations,
        lines: lines.len(),
        lines_without_diagnostics: per_line.iter().filter(|l| l.diagnostic_looks == 0).count(),
        voice_seen: lines
            .iter()
            .filter(|l| l.seen_speaking == Some(true))
            .count(),
        voice_not_seen: lines
            .iter()
            .filter(|l| l.seen_speaking == Some(false))
            .count(),
        voice_unreported: lines.iter().filter(|l| l.seen_speaking.is_none()).count(),
        per_line,
        limitations: [
            "Only the largest anonymous face is logged; this cannot establish whose line it is.",
            "No diagnostic means missing evidence, not a silent face or an empty view.",
            "at_us is judgement time; frame_age_ms is sampled before slow work, not capture-to-result latency.",
            "Voice seen counts are supplied observations, not ground truth; no accuracy or 95% verdict is computed.",
        ],
    })
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [journal_path, lines_path] = args.as_slice() else {
        return Err("usage: speaking_replay JOURNAL.jsonl LINES.jsonl".to_owned());
    };
    let journal = File::open(journal_path).map_err(|_| "cannot open journal input")?;
    let lines = File::open(lines_path).map_err(|_| "cannot open line input")?;
    let result = report(BufReader::new(journal), BufReader::new(lines))?;
    serde_json::to_writer_pretty(io::stdout().lock(), &result)
        .map_err(|_| "cannot write report".to_owned())?;
    println!();
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("speaking_replay: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_eyes::speaking::{HeadAtFrame, Mouths, Reason};

    const BOOT: &str = "0123456789abcdef0123456789abcdef";
    const OTHER_BOOT: &str = "fedcba9876543210fedcba9876543210";

    /// Synthetic geometry, serialized by the production observation type.
    fn observation(at_us: u64) -> MouthObservation {
        let marks = [
            (0.0, 0.0),
            (30.0, 0.0),
            (15.0, 10.0),
            (3.0, 20.0),
            (27.0, 20.0),
        ];
        let (_, decision) =
            Mouths::default().judge_with_diagnostics(1, &marks, Some(0.0), Some(0.01), at_us);
        MouthObservation {
            at_us,
            filled: [320, 240],
            face_box_px: [10.0, 10.0, 50.0, 60.0],
            face_count: 1,
            decision,
        }
    }

    fn entry(mouth: &MouthObservation, boot: &str, pid: u32) -> String {
        format!(
            "{}\n",
            serde_json::json!({
                "_BOOT_ID": boot, "_PID": pid.to_string(),
                "MESSAGE": format!("{PREFIX}{}", serde_json::to_string(mouth).unwrap())
            })
        )
    }

    fn line(id: u64, boot: &str, from_us: u64, to_us: u64, seen: Option<bool>) -> String {
        format!(
            "{}\n",
            serde_json::json!({
                "id": id, "boot_id": boot, "from_us": from_us, "to_us": to_us, "seen_speaking": seen
            })
        )
    }

    #[test]
    fn missing_diagnostics_and_voice_observations_remain_in_the_denominator() {
        let data = entry(&observation(1_000), BOOT, 12);
        let lines = line(1, BOOT, 900, 1_100, Some(false))
            + &line(2, BOOT, 2_000, 3_000, None)
            + &line(3, OTHER_BOOT, 900, 1_100, Some(true));
        let result = report(data.as_bytes(), lines.as_bytes()).unwrap();
        assert_eq!(result.lines, 3);
        assert_eq!(result.lines_without_diagnostics, 2);
        assert_eq!(
            (
                result.voice_seen,
                result.voice_not_seen,
                result.voice_unreported
            ),
            (1, 1, 1)
        );
        assert_eq!(result.per_line[1].largest_face_states.no, 0);
        assert_eq!(result.per_line[2].diagnostic_looks, 0);
    }

    #[test]
    fn a_largest_face_yes_does_not_make_a_voice_line_seen() {
        let mut mouth = observation(1_000);
        mouth.face_count = 2;
        mouth.decision.speaking.state = SpeakingState::Yes;
        let result = report(
            entry(&mouth, BOOT, 12).as_bytes(),
            line(1, BOOT, 900, 1_100, None).as_bytes(),
        )
        .unwrap();
        assert_eq!(result.per_line[0].largest_face_states.yes, 1);
        assert_eq!(result.per_line[0].multi_face_looks, 1);
        assert_eq!(result.voice_seen, 0);
        assert_eq!(result.voice_unreported, 1);
    }

    #[test]
    fn repeated_exports_do_not_inflate_looks_and_restart_gaps_do_not_join() {
        let a = entry(&observation(1_000), BOOT, 12);
        let data = a.clone()
            + &entry(&observation(2_000), BOOT, 13)
            + &a
            + &entry(&observation(1_250), BOOT, 12);
        let result = report(data.as_bytes(), line(1, BOOT, 900, 3_000, None).as_bytes()).unwrap();
        assert_eq!(result.observations, 3);
        assert_eq!(result.duplicate_observations, 1);
        assert_eq!(result.per_line[0].diagnostic_looks, 3);
        assert_eq!(result.per_line[0].eyes_processes, 2);
        assert_eq!(result.per_line[0].max_observed_gap_us, Some(250));
    }

    #[test]
    fn conflicting_duplicates_are_errors_not_a_choice_of_favourable_evidence() {
        let mut mouth = observation(1_000);
        let a = entry(&mouth, BOOT, 12);
        mouth.decision.speaking.state = SpeakingState::Yes;
        let data = a + &entry(&mouth, BOOT, 12);
        assert!(
            journal(data.as_bytes())
                .unwrap_err()
                .contains("conflicting duplicate")
        );
    }

    #[test]
    fn old_diagnostics_without_frame_age_are_not_invented_timestamps() {
        let mouth = observation(1_000);
        let result = report(
            entry(&mouth, BOOT, 12).as_bytes(),
            line(1, BOOT, 900, 1_100, None).as_bytes(),
        )
        .unwrap();
        let row = &result.per_line[0];
        assert_eq!(row.max_logged_frame_age_ms, None);
        assert_eq!(row.heads.get("unreported"), Some(&1));
        assert_eq!(row.reasons.get("too_few_looks"), Some(&1));
    }

    #[test]
    fn motion_and_mesh_diagnostics_keep_their_recorded_reason() {
        let mut mouth = observation(1_000);
        mouth.decision.head = Some(HeadAtFrame::Unknown);
        mouth.decision.reason = Reason::FewSteadyPairs;
        mouth.decision.frame_age_ms = Some(230);
        mouth.decision.mesh_current = false;
        let result = report(
            entry(&mouth, BOOT, 12).as_bytes(),
            line(1, BOOT, 900, 1_100, Some(false)).as_bytes(),
        )
        .unwrap();
        let row = &result.per_line[0];
        assert_eq!(row.reasons.get("few_steady_pairs"), Some(&1));
        assert_eq!(row.heads.get("unknown"), Some(&1));
        assert_eq!(row.mesh_missing_looks, 1);
        assert_eq!(row.max_logged_frame_age_ms, Some(230));
    }

    #[test]
    fn time_is_judgement_time_not_frame_age_subtracted_after_the_mesh() {
        let mut mouth = observation(1_100_000);
        mouth.decision.frame_age_ms = Some(230);
        let result = report(
            entry(&mouth, BOOT, 12).as_bytes(),
            line(1, BOOT, 800_000, 900_000, None).as_bytes(),
        )
        .unwrap();
        assert_eq!(result.per_line[0].diagnostic_looks, 0);
    }

    #[test]
    fn malformed_diagnostics_fail_without_echoing_private_content() {
        let data = serde_json::json!({"_BOOT_ID": BOOT, "_PID": "12", "MESSAGE": format!("{PREFIX}private words")}).to_string();
        let error = journal(data.as_bytes()).unwrap_err();
        assert!(error.contains("invalid mouth diagnostic"));
        assert!(!error.contains("private"));
        let data = serde_json::json!({"MESSAGE": "an unrelated private message"}).to_string();
        assert_eq!(journal(data.as_bytes()).unwrap().observations, 0);
    }

    #[test]
    fn anonymous_clocks_invalid_intervals_and_unknown_line_fields_are_refused() {
        let data = serde_json::json!({"MESSAGE": format!("{PREFIX}{}", serde_json::to_string(&observation(1_000)).unwrap())}).to_string();
        assert!(journal(data.as_bytes()).is_err());
        for data in [
            line(1, BOOT, 2, 1, None),
            line(1, "", 1, 2, None),
            line(1, BOOT, 1, 2, None) + &line(1, BOOT, 3, 4, None),
            "{\"id\":1,\"words\":\"private\"}".to_owned(),
        ] {
            assert!(lines(data.as_bytes()).is_err());
        }
    }

    #[test]
    fn empty_journal_is_missing_evidence_but_an_empty_line_set_is_invalid() {
        let result = report(&b""[..], line(1, BOOT, 1, 2, None).as_bytes()).unwrap();
        assert_eq!(result.lines_without_diagnostics, 1);
        assert_eq!(result.voice_unreported, 1);
        assert!(report(&b""[..], &b""[..]).is_err());
    }

    #[test]
    fn oversized_records_and_row_limits_are_errors_not_truncation() {
        let too_large = vec![b' '; MAX_ROW_BYTES as usize + 1];
        assert!(rows(too_large.as_slice(), 10, |_, _| Ok(())).is_err());
        assert!(rows(&b"a\nb\n"[..], 1, |_, _| Ok(())).is_err());
        let hash = rows(&b"abc"[..], 1, |_, _| Ok(())).unwrap();
        assert_eq!(hash, format!("{:x}", Sha256::digest(b"abc")));
    }
}
