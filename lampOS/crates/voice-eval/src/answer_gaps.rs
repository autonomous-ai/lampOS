//! Accepted zero PCM while a reply was unfinished, from exact event files.
//!
//! This reader preserves the nested owner and full receipt which the general
//! evaluator does not need. It reports no words, inferred causes, acoustic
//! measurements, or denominator of answered replies. All origins, including
//! notices and missing/null origins, are included regardless of listening state;
//! this is not the coordinator's origin- and listening-dependent gap counter.
use lamp_audio::RENDER_RATE;
use lamp_turn_owner::TurnOwner;
use lamp_voice::wire::{PlaybackGap, PlaybackGapPhase};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufRead, BufReader, Read},
    path::Path,
};

pub const THRESHOLD_MS: u64 = 300;
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 1024 * 1024;
const MAX_RECORDS: u64 = 250_000;

#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct RawPhases {
    pub started: usize,
    pub resumed: usize,
    pub cancelled: usize,
    pub unknown: usize,
}

#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct Terminals {
    pub resumed: usize,
    pub cancelled: usize,
}

#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct GapCount {
    pub gaps: usize,
    pub distinct_replies: usize,
}

#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct OverThreshold {
    pub resumed: GapCount,
    pub cancelled: GapCount,
    /// Union of the two sets above, within this file only.
    pub distinct_replies: usize,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputError {
    Open,
    NotRegularFile,
    Read,
    FileLimit,
    LineLimit,
    RecordLimit,
}

#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct Problems {
    pub malformed_json: usize,
    pub invalid_record: usize,
    pub invalid_gap: usize,
    pub duplicate_starts: usize,
    pub duplicate_terminals: usize,
    pub orphan_terminals: usize,
    pub mismatched_terminals: usize,
    pub no_gap_records: bool,
    pub input_error: Option<InputError>,
}

#[derive(Debug, Default, Serialize)]
pub struct FileReport {
    /// The operator-supplied file, never an identity read from an event payload.
    pub input: String,
    pub records_read: u64,
    pub playback_gap_records: usize,
    pub raw_phases: RawPhases,
    /// Unique, valid start/terminal pairs, including gaps at or below 300 ms.
    pub paired_terminals: Terminals,
    pub over_300_ms: OverThreshold,
    /// Starts without a valid terminal receipt; final durations are unknown.
    pub incomplete: GapCount,
    /// Identities excluded for invalid, conflicting or repeated evidence,
    /// including byte-identical repeated starts or terminals.
    pub excluded_identities: usize,
    pub problems: Problems,
    /// Describes only the supplied file's gap evidence, not session completeness.
    pub evidence_complete: bool,
}

type ReplyKey = ([u8; 16], u64, u64);
type GapKey = (ReplyKey, u64);

fn reply_key(owner: TurnOwner) -> ReplyKey {
    (owner.boot().bytes(), owner.turn(), owner.generation())
}

#[derive(Clone, Copy)]
struct Receipt {
    gap: PlaybackGap,
    phase: PlaybackGapPhase,
    observed_at_us: u64,
    record: u64,
}

#[derive(Default)]
struct GapEvidence {
    start: Option<Receipt>,
    terminal: Option<Receipt>,
    invalid: bool,
}

fn matches_start(start: Receipt, terminal: Receipt) -> bool {
    start.record < terminal.record
        && start.gap.first_sample == terminal.gap.first_sample
        && start.gap.zero_frames <= terminal.gap.zero_frames
        && start.gap.last_zero_accepted_at_us <= terminal.gap.last_zero_accepted_at_us
        && start.observed_at_us <= terminal.observed_at_us
}

/// Read a regular, non-symlink JSONL file without following directories or
/// waiting on a FIFO. Failures are fixed categories, never parser payloads.
pub fn read_file(path: &Path) -> FileReport {
    let opened = (|| {
        let descriptor = rustix::fs::open(
            path,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::CLOEXEC
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::NONBLOCK,
            rustix::fs::Mode::empty(),
        )
        .map_err(|_| InputError::Open)?;
        let file = File::from(descriptor);
        let metadata = file.metadata().map_err(|_| InputError::Read)?;
        if !metadata.is_file() {
            return Err(InputError::NotRegularFile);
        }
        if metadata.len() > MAX_FILE_BYTES {
            return Err(InputError::FileLimit);
        }
        Ok(file)
    })();
    let mut report = match opened {
        Ok(file) => read(BufReader::new(file)),
        Err(error) => FileReport {
            problems: Problems {
                input_error: Some(error),
                no_gap_records: true,
                ..Problems::default()
            },
            ..FileReport::default()
        },
    };
    report.input = path.to_string_lossy().into_owned();
    report
}

fn read(mut reader: impl BufRead) -> FileReport {
    let mut report = FileReport::default();
    let mut gaps = BTreeMap::<GapKey, GapEvidence>::new();
    let mut line = Vec::new();
    let mut bytes = 0_u64;
    loop {
        line.clear();
        // Take bounds allocation even if a malformed file has no newlines.
        let count = match (&mut reader)
            .take(MAX_LINE_BYTES as u64 + 1)
            .read_until(b'\n', &mut line)
        {
            Ok(0) => break,
            Ok(count) => count,
            Err(_) => {
                report.problems.input_error = Some(InputError::Read);
                break;
            }
        };
        bytes += count as u64;
        if bytes > MAX_FILE_BYTES {
            report.problems.input_error = Some(InputError::FileLimit);
            break;
        }
        if count > MAX_LINE_BYTES {
            report.problems.input_error = Some(InputError::LineLimit);
            break;
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        if report.records_read == MAX_RECORDS {
            report.problems.input_error = Some(InputError::RecordLimit);
            break;
        }
        report.records_read += 1;
        let value: Value = match serde_json::from_slice(&line) {
            Ok(value) => value,
            Err(_) => {
                report.problems.malformed_json += 1;
                continue;
            }
        };
        let Some(kind) = value.get("kind").and_then(Value::as_str) else {
            report.problems.invalid_record += 1;
            continue;
        };
        if kind != "playback_gap" {
            continue;
        }
        report.playback_gap_records += 1;
        let phase = match value.get("phase").and_then(Value::as_str) {
            Some("started") => {
                report.raw_phases.started += 1;
                PlaybackGapPhase::Started
            }
            Some("resumed") => {
                report.raw_phases.resumed += 1;
                PlaybackGapPhase::Resumed
            }
            Some("cancelled") => {
                report.raw_phases.cancelled += 1;
                PlaybackGapPhase::Cancelled
            }
            _ => {
                report.raw_phases.unknown += 1;
                report.problems.invalid_gap += 1;
                continue;
            }
        };
        let Some(gap) = value
            .get("gap")
            .and_then(|gap| serde_json::from_value::<PlaybackGap>(gap.clone()).ok())
        else {
            report.problems.invalid_gap += 1;
            continue;
        };
        let evidence = gaps
            .entry((reply_key(gap.owner), gap.started_at_us))
            .or_default();
        let Some(observed_at_us) = value.get("at_us").and_then(Value::as_u64) else {
            report.problems.invalid_gap += 1;
            evidence.invalid = true;
            continue;
        };
        let receipt = Receipt {
            gap,
            phase,
            observed_at_us,
            record: report.records_read,
        };
        if !receipt.gap.is_valid_receipt(phase, observed_at_us) {
            report.problems.invalid_gap += 1;
            evidence.invalid = true;
            continue;
        }
        let destination = if phase == PlaybackGapPhase::Started {
            if evidence.start.is_some() {
                report.problems.duplicate_starts += 1;
            }
            &mut evidence.start
        } else {
            if evidence.terminal.is_some() {
                report.problems.duplicate_terminals += 1;
            }
            &mut evidence.terminal
        };
        if destination.is_some() {
            evidence.invalid = true;
        } else {
            *destination = Some(receipt);
        }
    }

    let mut resumed_replies = BTreeSet::new();
    let mut cancelled_replies = BTreeSet::new();
    let mut incomplete_replies = BTreeSet::new();
    for ((owner, _), mut evidence) in gaps {
        match (evidence.start, evidence.terminal) {
            (Some(start), Some(terminal)) => {
                if !matches_start(start, terminal) {
                    report.problems.mismatched_terminals += 1;
                    evidence.invalid = true;
                }
                if !evidence.invalid {
                    let (paired, over, replies) = match terminal.phase {
                        PlaybackGapPhase::Resumed => (
                            &mut report.paired_terminals.resumed,
                            &mut report.over_300_ms.resumed,
                            &mut resumed_replies,
                        ),
                        PlaybackGapPhase::Cancelled => (
                            &mut report.paired_terminals.cancelled,
                            &mut report.over_300_ms.cancelled,
                            &mut cancelled_replies,
                        ),
                        PlaybackGapPhase::Started => unreachable!("terminal phase checked above"),
                    };
                    *paired += 1;
                    // Widen before multiplying: one frame above exactly 300 ms counts.
                    if u128::from(terminal.gap.zero_frames) * 1000
                        > u128::from(THRESHOLD_MS) * u128::from(RENDER_RATE)
                    {
                        over.gaps += 1;
                        replies.insert(owner);
                    }
                }
            }
            (Some(_), None) => {
                report.incomplete.gaps += 1;
                incomplete_replies.insert(owner);
            }
            (None, Some(_)) => {
                report.problems.orphan_terminals += 1;
                evidence.invalid = true;
            }
            (None, None) => {}
        }
        report.excluded_identities += usize::from(evidence.invalid);
    }
    report.over_300_ms.resumed.distinct_replies = resumed_replies.len();
    report.over_300_ms.cancelled.distinct_replies = cancelled_replies.len();
    report.over_300_ms.distinct_replies = resumed_replies.union(&cancelled_replies).count();
    report.incomplete.distinct_replies = incomplete_replies.len();
    report.problems.no_gap_records = report.playback_gap_records == 0;
    report.evidence_complete =
        report.problems == Problems::default() && report.incomplete.gaps == 0;
    report
}

/// A compact per-file rendering. Problem names are fixed schema fields; no
/// words, free-form record values, or owner identities reach this output.
pub fn render(report: &FileReport) -> String {
    format!(
        "file: {}\n  records: {}; playback_gap records: {} (started {}, resumed {}, cancelled {}, unknown {})\n  paired terminals, all durations: resumed {}, cancelled {}\n  >300 ms: resumed {} gaps / {} replies; cancelled {} gaps / {} replies; {} distinct replies in their union\n  incomplete: {} gaps / {} replies; final duration unknown\n  excluded identities: {}; evidence complete: {}\n  problems: {}\n",
        report.input,
        report.records_read,
        report.playback_gap_records,
        report.raw_phases.started,
        report.raw_phases.resumed,
        report.raw_phases.cancelled,
        report.raw_phases.unknown,
        report.paired_terminals.resumed,
        report.paired_terminals.cancelled,
        report.over_300_ms.resumed.gaps,
        report.over_300_ms.resumed.distinct_replies,
        report.over_300_ms.cancelled.gaps,
        report.over_300_ms.cancelled.distinct_replies,
        report.over_300_ms.distinct_replies,
        report.incomplete.gaps,
        report.incomplete.distinct_replies,
        report.excluded_identities,
        report.evidence_complete,
        serde_json::to_string(&report.problems).expect("fixed scalar fields serialize"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Cursor;

    fn owner(boot: u8, turn: u64, generation: u64) -> Value {
        json!({"boot": vec![boot; 16], "turn": turn, "generation": generation})
    }

    fn receipt(owner: Value, phase: &str, start: u64, first: u64, frames: u64) -> Value {
        let last = start + frames * 1_000_000 / u64::from(RENDER_RATE);
        json!({
            "kind": "playback_gap", "phase": phase, "at_us": last,
            "gap": {
                "owner": owner,
                "first_sample": {"epoch": 1, "frame": first},
                "end_sample": {"epoch": 1, "frame": first + frames},
                "started_at_us": start, "last_zero_accepted_at_us": last,
                "zero_frames": frames
            }
        })
    }

    fn pair(owner: Value, phase: &str, start: u64, frames: u64) -> Vec<Value> {
        vec![
            receipt(owner.clone(), "started", start, 100, 240),
            receipt(owner, phase, start, 100, frames),
        ]
    }

    fn report(records: Vec<Value>) -> FileReport {
        let text = records
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        read(Cursor::new(text))
    }

    #[test]
    fn answer_gaps_threshold_is_strict_and_uses_accepted_frames() {
        let mut records = pair(owner(1, 1, 1), "resumed", 1_000_000, 7200);
        records.extend(pair(owner(1, 2, 1), "resumed", 2_000_000, 7201));
        records.extend(pair(owner(1, 3, 1), "cancelled", 3_000_000, 7200));
        // Delayed reporting does not turn 300 ms of zeros into a longer gap.
        records[1]["at_us"] = json!(10_000_000);
        let report = report(records);
        assert!(report.evidence_complete);
        assert_eq!(report.playback_gap_records, 6);
        assert_eq!(
            report.raw_phases,
            RawPhases {
                started: 3,
                resumed: 2,
                cancelled: 1,
                unknown: 0
            }
        );
        assert_eq!(
            report.paired_terminals,
            Terminals {
                resumed: 2,
                cancelled: 1
            }
        );
        assert_eq!(
            report.over_300_ms.resumed,
            GapCount {
                gaps: 1,
                distinct_replies: 1
            }
        );
        assert_eq!(report.over_300_ms.cancelled.gaps, 0);
    }

    #[test]
    fn answer_gaps_keep_resumed_when_the_turn_is_later_cancelled() {
        let mut records = pair(owner(1, 8, 1), "resumed", 1_000_000, 24_000);
        records.extend(pair(owner(1, 8, 1), "resumed", 3_000_000, 9600));
        records.extend(pair(owner(1, 8, 1), "cancelled", 4_000_000, 12_000));
        records.push(json!({
            "kind": "turn_finished", "owner": owner(1, 8, 1), "turn": 8,
            "at_us": 5_000_000, "outcome": "user_interrupted",
            "provider_audio_seen": true, "playback_gaps": 3
        }));
        let report = report(records);
        assert!(report.evidence_complete);
        assert_eq!(
            report.over_300_ms.resumed,
            GapCount {
                gaps: 2,
                distinct_replies: 1
            }
        );
        assert_eq!(
            report.over_300_ms.cancelled,
            GapCount {
                gaps: 1,
                distinct_replies: 1
            }
        );
        assert_eq!(report.over_300_ms.distinct_replies, 1);
        assert_eq!(report.playback_gap_records, 6);
    }

    #[test]
    fn answer_gaps_include_notices_listening_and_absent_or_null_origins() {
        let mut records = pair(owner(1, 1, 1), "resumed", 1_000_000, 9600);
        for record in &mut records {
            record["origin"] = json!("failure_notice");
        }
        let mut cancelled = pair(owner(1, 2, 1), "cancelled", 2_000_000, 9600);
        cancelled[0]["origin"] = json!("provider");
        // Cancellation can retire the reply before its terminal gap is recorded.
        cancelled[1]["origin"] = Value::Null;
        records.extend(cancelled);
        // Timing exports may omit origin entirely; pairing uses the gap owner.
        records.extend(pair(owner(1, 3, 1), "resumed", 3_000_000, 9600));
        let mut listening = pair(owner(1, 4, 1), "resumed", 4_000_000, 9600);
        for record in &mut listening {
            record["origin"] = json!("provider");
            record["listen_gap"] = json!(true);
            record["listen_gap_overran_us"] = Value::Null;
        }
        records.extend(listening);
        let report = report(records);
        assert!(report.evidence_complete);
        assert_eq!(report.playback_gap_records, 8);
        assert_eq!(
            report.paired_terminals,
            Terminals {
                resumed: 3,
                cancelled: 1
            }
        );
        assert_eq!(
            report.over_300_ms.resumed,
            GapCount {
                gaps: 3,
                distinct_replies: 3
            }
        );
        assert_eq!(
            report.over_300_ms.cancelled,
            GapCount {
                gaps: 1,
                distinct_replies: 1
            }
        );
        assert_eq!(report.over_300_ms.distinct_replies, 4);
    }

    #[test]
    fn answer_gaps_identity_uses_nested_boot_turn_generation_and_start() {
        let mut records = pair(owner(1, 9, 1), "resumed", 1_000_000, 9600);
        records.extend(pair(owner(2, 9, 1), "resumed", 1_000_000, 9600));
        records.extend(pair(owner(1, 9, 2), "resumed", 1_000_000, 9600));
        records.extend(pair(owner(1, 10, 1), "resumed", 1_000_000, 9600));
        records.extend(pair(owner(1, 9, 1), "resumed", 2_000_000, 9600));
        for record in &mut records {
            // Exported gaps may have no top-level turn; it is not their identity.
            record["turn"] = json!(999);
            record["owner"] = owner(9, 999, 1);
        }
        let report = report(records);
        assert!(report.evidence_complete);
        assert_eq!(
            report.over_300_ms.resumed,
            GapCount {
                gaps: 5,
                distinct_replies: 4
            }
        );
    }

    #[test]
    fn answer_gaps_duplicates_exclude_pairs_even_when_records_are_identical() {
        for duplicate_start in [true, false] {
            for conflict in [true, false] {
                let mut records = pair(owner(1, 1, 1), "resumed", 1_000_000, 9600);
                let mut duplicate = records[usize::from(!duplicate_start)].clone();
                if conflict && !duplicate_start {
                    duplicate["phase"] = json!("cancelled");
                } else if conflict {
                    duplicate["gap"]["first_sample"]["frame"] = json!(200);
                    duplicate["gap"]["end_sample"]["frame"] = json!(440);
                }
                if !conflict {
                    assert_eq!(
                        duplicate.to_string(),
                        records[usize::from(!duplicate_start)].to_string()
                    );
                }
                records.push(duplicate);
                let report = report(records);
                assert!(!report.evidence_complete);
                assert_eq!(report.playback_gap_records, 3);
                assert_eq!(report.excluded_identities, 1);
                assert_eq!(report.paired_terminals, Terminals::default());
                assert_eq!(
                    report.problems.duplicate_starts,
                    usize::from(duplicate_start)
                );
                assert_eq!(
                    report.problems.duplicate_terminals,
                    usize::from(!duplicate_start)
                );
            }
        }
    }

    #[test]
    fn answer_gaps_unclosed_start_has_unknown_duration_and_orphan_is_not_paired() {
        let records = vec![
            receipt(owner(1, 1, 1), "started", 1_000_000, 100, 240),
            receipt(owner(1, 2, 1), "resumed", 2_000_000, 100, 24_000),
            json!({"kind": "turn_completed", "at_us": 100_000_000}),
        ];
        let report = report(records);
        assert!(!report.evidence_complete);
        assert_eq!(
            report.incomplete,
            GapCount {
                gaps: 1,
                distinct_replies: 1
            }
        );
        assert_eq!(report.problems.orphan_terminals, 1);
        assert_eq!(report.paired_terminals, Terminals::default());
        assert_eq!(report.over_300_ms, OverThreshold::default());
    }

    #[test]
    fn answer_gaps_checks_pair_cursor_and_receipt_order() {
        for case in 0..5 {
            let mut records = pair(owner(1, 1, 1), "resumed", 1_000_000, 9600);
            match case {
                0 => {
                    records[1]["gap"]["first_sample"]["frame"] = json!(200);
                    records[1]["gap"]["end_sample"]["frame"] = json!(9800);
                }
                1 => records.swap(0, 1),
                2 => records[0]["at_us"] = json!(2_000_000),
                3 => {
                    records[0]["gap"]["last_zero_accepted_at_us"] = json!(2_000_000);
                    records[0]["at_us"] = json!(2_000_000);
                }
                _ => records[1] = receipt(owner(1, 1, 1), "resumed", 1_000_000, 100, 120),
            }
            let report = report(records);
            assert!(!report.evidence_complete, "case {case}");
            assert_eq!(report.problems.mismatched_terminals, 1, "case {case}");
            assert_eq!(report.paired_terminals, Terminals::default());
        }
    }

    #[test]
    fn answer_gaps_rejects_invalid_wire_receipts_without_parser_payloads() {
        let mutations = [
            ("/gap/first_sample/epoch", json!(0)),
            ("/gap/end_sample/epoch", json!(2)),
            ("/gap/end_sample/frame", json!(0)),
            ("/gap/zero_frames", json!(0)),
            ("/gap/started_at_us", json!(0)),
            ("/gap/last_zero_accepted_at_us", json!(0)),
            ("/at_us", json!(0)),
            ("/gap/owner/turn", json!(0)),
            ("/gap/owner/generation", json!(0)),
            ("/gap/owner/boot", json!(vec![0; 16])),
            ("/gap/zero_frames", json!("PRIVATE_SENTINEL")),
        ];
        for (field, value) in mutations {
            let mut records = pair(owner(1, 1, 1), "resumed", 1_000_000, 9600);
            *records[1].pointer_mut(field).unwrap() = value;
            let report = report(records);
            assert!(!report.evidence_complete, "{field}");
            assert_eq!(report.problems.invalid_gap, 1, "{field}");
            assert_eq!(report.incomplete.gaps, 1);
            assert_eq!(report.paired_terminals, Terminals::default());
            assert!(!render(&report).contains("PRIVATE_SENTINEL"));
        }
        let report = report(vec![receipt(
            owner(1, 1, 1),
            "started",
            1_000_000,
            100,
            241,
        )]);
        assert_eq!(report.problems.invalid_gap, 1);
    }

    #[test]
    fn answer_gaps_unknown_and_malformed_records_are_explicit_and_private() {
        let text = concat!(
            "{\"kind\":\"transcript\",\"text\":\"PRIVATE_SENTINEL\"}\n",
            "{\"kind\":\"playback_gap\",\"phase\":\"PRIVATE_SENTINEL\"}\n",
            "{\"PRIVATE_SENTINEL\":true}\n",
            "PRIVATE_SENTINEL\n",
        );
        let report = read(Cursor::new(text));
        assert!(!report.evidence_complete);
        assert_eq!(report.records_read, 4);
        assert_eq!(report.raw_phases.unknown, 1);
        assert_eq!(report.problems.invalid_gap, 1);
        assert_eq!(report.problems.invalid_record, 1);
        assert_eq!(report.problems.malformed_json, 1);
        assert!(
            !serde_json::to_string(&report)
                .unwrap()
                .contains("PRIVATE_SENTINEL")
        );
        assert!(!render(&report).contains("PRIVATE_SENTINEL"));
    }

    #[test]
    fn answer_gaps_empty_or_unrelated_input_is_not_clean_gap_evidence() {
        for text in ["", "\n \t\r\n", "{\"kind\":\"turn_completed\"}\n"] {
            let report = read(Cursor::new(text));
            assert!(report.problems.no_gap_records);
            assert!(!report.evidence_complete);
        }
    }

    #[test]
    fn answer_gaps_limits_bound_lines_and_records_and_preserve_partial_evidence() {
        let mut text = pair(owner(1, 1, 1), "resumed", 1_000_000, 9600)
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        text.push('\n');
        text.push_str(&" ".repeat(MAX_LINE_BYTES + 1));
        let report = read(Cursor::new(text));
        assert!(!report.evidence_complete);
        assert_eq!(report.problems.input_error, Some(InputError::LineLimit));
        assert_eq!(report.over_300_ms.resumed.gaps, 1);
        let report = read(Cursor::new(
            "{\"kind\":\"ignored\"}\n".repeat(MAX_RECORDS as usize + 1),
        ));
        assert_eq!(report.problems.input_error, Some(InputError::RecordLimit));
        assert_eq!(report.records_read, MAX_RECORDS);
    }
}
