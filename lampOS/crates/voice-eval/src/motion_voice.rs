//! `lamp-voice-eval motion-voice SESSIONS... [--plays FILE]... [--offset-us N]
//! [--json]` (the first builder on #1722, 21 Oct 05:49): what
//! `LAMP_MOTION_VOICE=record` says of each sound the body-moving rule let go
//! (#2371), so its bar comes straight from the lamp's own records.
//!
//! Each `SESSIONS` is one session folder (with `metadata/`) or a folder of
//! them (`/var/lib/lampOS/voice/sessions`). For each
//! `input_let_go_body_moving` (and `input_kept_body_moving`, with `on`) it
//! prints the sound's level against the asker's, `would_keep`, and whether a
//! played line was sounding at its onset, from the scenes' `#PLAY_US` lines
//! or a `plays.tsv` (`start_us<TAB>end_us`), on the records' clock (`at_us`;
//! `--offset-us` is added to the plays' times to line them up). Then the
//! levels' spread with a line and without one, and the bar that would keep
//! the lines and still let the rest go.
//!
//! Levels are the voice's face scale (`ring_face::level_of`): 0 is -48 dBFS
//! or under, 255 is -14 dBFS or over, 7.5 to a decibel. "Under" is how far
//! the sound is under the asker, in that scale: the rule keeps a sound when
//! its upper tenth is at most [`lamp_voice::motion_voice::MARGIN`] under.
//! Numbers only: never a word heard.

use lamp_voice::{asker_first::ENOUGH_BLOCKS, motion_voice::MARGIN};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// A played line counts as sounding from this long before its start.
pub const BEFORE_US: u64 = 200_000;
/// And until this long after its end.
pub const AFTER_US: u64 = 300_000;
/// A `#PLAY_US` line gives only its start: it is taken to end at the next
/// line's start, or this long after its own, whichever is sooner.
pub const LINE_MOST_US: u64 = 4_000_000;
/// The face scale's steps to a decibel.
const PER_DB: f64 = 7.5;

/// `steps` of the face scale in decibels, to a tenth.
fn db(steps: i32) -> f64 {
    (f64::from(steps) / PER_DB * 10.0).round() / 10.0
}

/// One sound the body-moving rule judged.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub at_us: u64,
    pub turn: Option<u64>,
    pub onset_us: u64,
    /// `on` kept it (`input_kept_body_moving`); otherwise let go.
    pub kept: bool,
    /// None when the switch was off, or no asker's level was known.
    pub asker: Option<u8>,
    pub heard: Option<u8>,
    pub upper: Option<u8>,
    pub blocks: u64,
    pub would_keep: Option<bool>,
    /// None with no plays given.
    pub during_line: Option<bool>,
}

impl Row {
    fn from_record(event: &Value) -> Option<Self> {
        let kept = match event["kind"].as_str()? {
            "input_let_go_body_moving" => false,
            "input_kept_body_moving" => true,
            _ => return None,
        };
        let level = |key: &str| {
            event[key]
                .as_u64()
                .and_then(|value| u8::try_from(value).ok())
        };
        Some(Self {
            at_us: event["at_us"].as_u64()?,
            turn: event["turn"].as_u64(),
            onset_us: event["onset_from_us"]
                .as_u64()
                .or_else(|| event["at_us"].as_u64())?,
            kept,
            asker: level("asker_level"),
            heard: level("heard_level"),
            upper: level("heard_upper_tenth"),
            blocks: event["blocks"].as_u64().unwrap_or(0),
            would_keep: event["would_keep"].as_bool().or(kept.then_some(true)),
            during_line: None,
        })
    }

    /// How far its upper tenth, and its middle, are under the asker.
    fn under(&self) -> Option<(i32, i32)> {
        let asker = i32::from(self.asker?);
        Some((
            asker - i32::from(self.upper?),
            asker - i32::from(self.heard?),
        ))
    }

    /// Judged on enough of its own blocks for the rule to read it.
    fn readable(&self) -> bool {
        self.under().is_some() && self.blocks >= ENOUGH_BLOCKS as u64
    }
}

/// The session folders `path` names: itself, or those under it.
pub fn sessions(path: &Path) -> Vec<PathBuf> {
    if path.join("metadata").is_dir() {
        return vec![path.to_path_buf()];
    }
    let mut found: Vec<PathBuf> = crate::waiting_cuts::find_sessions(path)
        .into_iter()
        .map(|(folder, _, _)| folder)
        .collect();
    found.sort();
    found
}

/// The rows in the records of `folders`, in their order.
pub fn rows(folders: &[PathBuf]) -> std::io::Result<Vec<Row>> {
    let mut rows = Vec::new();
    for folder in folders {
        for event in crate::waiting_cuts::records(folder)? {
            rows.extend(Row::from_record(&event));
        }
    }
    Ok(rows)
}

/// The played lines' spans in `text`, a scene file's `#PLAY_US START ...`
/// lines or a `plays.tsv`'s `START<TAB>END` lines, each moved by
/// `offset_us`. Other lines are passed over.
pub fn plays(text: &str, offset_us: i64) -> Vec<(u64, u64)> {
    let shift = |us: u64| u64::try_from(us as i64 + offset_us).ok();
    let mut starts = Vec::new();
    let mut spans = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("#PLAY_US ") {
            if let Some(start) = rest
                .split_whitespace()
                .next()
                .and_then(|value| value.parse().ok())
                .and_then(shift)
            {
                starts.push(start);
            }
            continue;
        }
        let mut fields = line.split('\t');
        if let (Some(Ok(start)), Some(Ok(end))) = (
            fields.next().map(str::parse::<u64>),
            fields.next().map(str::parse::<u64>),
        ) && let (Some(start), Some(end)) = (shift(start), shift(end))
            && end >= start
        {
            spans.push((start, end));
        }
    }
    starts.sort_unstable();
    for (index, start) in starts.iter().enumerate() {
        let most = start.saturating_add(LINE_MOST_US);
        let end = starts.get(index + 1).map_or(most, |next| (*next).min(most));
        spans.push((*start, end));
    }
    spans.sort_unstable();
    spans
}

/// Whether a played line was sounding at `onset_us`.
pub fn during(spans: &[(u64, u64)], onset_us: u64) -> bool {
    spans.iter().any(|(start, end)| {
        onset_us >= start.saturating_sub(BEFORE_US) && onset_us <= end.saturating_add(AFTER_US)
    })
}

/// The spread of `values` in decibels: count, least, tenth, middle, ninth
/// tenth, most.
fn spread(mut values: Vec<i32>) -> Value {
    if values.is_empty() {
        return json!({"count":0});
    }
    values.sort_unstable();
    let at = |share: usize| db(values[(values.len() - 1) * share / 10]);
    json!({"count":values.len(),"least_db":at(0),"tenth_db":at(1),"middle_db":at(5),
        "ninth_db":at(9),"most_db":at(10)})
}

/// The bar, in face steps under the asker, that keeps the most lines and
/// lets the most of the rest go: kept at or under it. `with` are the lines'
/// sounds, `without` the rest. The fewest wrong, then the tightest such bar,
/// which keeps the fewest motor sounds; when the two sides don't overlap,
/// the whole clean range is said too.
pub fn bar(with: &[i32], without: &[i32]) -> Option<Value> {
    if with.is_empty() || without.is_empty() {
        return None;
    }
    let mut candidates: Vec<i32> = with.iter().chain(without).copied().collect();
    candidates.push(with.iter().chain(without).min().copied()? - 1);
    candidates.sort_unstable();
    candidates.dedup();
    let wrong = |bar: i32| {
        (
            with.iter().filter(|under| **under > bar).count(),
            without.iter().filter(|under| **under <= bar).count(),
        )
    };
    let best = candidates.iter().copied().min_by_key(|bar| {
        let (lines_let_go, rest_kept) = wrong(*bar);
        (lines_let_go + rest_kept, *bar)
    })?;
    let (lines_let_go, rest_kept) = wrong(best);
    let clean = lines_let_go + rest_kept == 0;
    let mut value = json!({"bar":best,"bar_db":db(best),
        "lines_let_go":lines_let_go,"rest_kept":rest_kept,"clean":clean});
    if clean {
        // Any bar from the loudest line's to under the quietest of the rest.
        let low = with.iter().max().copied()?;
        let high = without.iter().min().copied()? - 1;
        value["clean_from_db"] = json!(db(low));
        value["clean_to_db"] = json!(db(high));
    }
    Some(value)
}

/// The report for `rows`, each marked as during a line when `spans` are
/// given.
pub fn report(mut rows: Vec<Row>, spans: Option<&[(u64, u64)]>) -> Value {
    if let Some(spans) = spans {
        for row in &mut rows {
            row.during_line = Some(during(spans, row.onset_us));
        }
    }
    let listed: Vec<Value> = rows
        .iter()
        .map(|row| {
            json!({"at_us":row.at_us,"turn":row.turn,"onset_us":row.onset_us,"kept":row.kept,
                "asker_level":row.asker,"heard_level":row.heard,"heard_upper_tenth":row.upper,
                "blocks":row.blocks,"would_keep":row.would_keep,"during_line":row.during_line,
                "under_db":row.under().map(|(upper, _)| db(upper))})
        })
        .collect();
    let readable: Vec<&Row> = rows.iter().filter(|row| row.readable()).collect();
    let pick = |during: Option<bool>, upper: bool| -> Vec<i32> {
        readable
            .iter()
            .filter(|row| during.is_none() || row.during_line == during)
            .filter_map(|row| row.under().map(|(u, m)| if upper { u } else { m }))
            .collect()
    };
    let mut value = json!({
        "rows":listed,
        "judged":rows.len(),
        "no_askers_level":rows.iter().filter(|row| row.under().is_none()).count(),
        "too_few_blocks":rows.iter().filter(|row| row.under().is_some() && !row.readable()).count(),
        "would_keep":rows.iter().filter(|row| row.would_keep == Some(true)).count(),
        "margin":MARGIN,"margin_db":db(i32::from(MARGIN)),"enough_blocks":ENOUGH_BLOCKS,
        "before_us":BEFORE_US,"after_us":AFTER_US,
    });
    if spans.is_some() {
        value["upper_under_with_a_line"] = spread(pick(Some(true), true));
        value["upper_under_without"] = spread(pick(Some(false), true));
        value["middle_under_with_a_line"] = spread(pick(Some(true), false));
        value["middle_under_without"] = spread(pick(Some(false), false));
        value["bar_on_upper_tenth"] = json!(bar(&pick(Some(true), true), &pick(Some(false), true)));
        value["bar_on_middle"] = json!(bar(&pick(Some(true), false), &pick(Some(false), false)));
    } else {
        value["upper_under"] = spread(pick(None, true));
        value["middle_under"] = spread(pick(None, false));
    }
    value
}

/// The report as lines for a person.
pub fn table(report: &Value) -> String {
    let mut out = String::from(
        "at_us\tturn\tblocks\tasker\theard\tupper\tunder_db\twould_keep\tduring_line\n",
    );
    let text = |value: &Value| {
        if value.is_null() {
            "-".to_owned()
        } else if let Some(number) = value.as_f64().filter(|_| value.is_f64()) {
            format!("{number:.1}")
        } else {
            value.to_string()
        }
    };
    for row in report["rows"].as_array().into_iter().flatten() {
        let fields: Vec<String> = [
            "at_us",
            "turn",
            "blocks",
            "asker_level",
            "heard_level",
            "heard_upper_tenth",
            "under_db",
            "would_keep",
            "during_line",
        ]
        .iter()
        .map(|key| text(&row[*key]))
        .collect();
        out.push_str(&fields.join("\t"));
        out.push('\n');
    }
    out.push_str(&format!(
        "judged {} | no asker's level {} | too few blocks {} | would keep {} | the rule's bar {:.1} dB under, {} blocks\n",
        report["judged"],
        report["no_askers_level"],
        report["too_few_blocks"],
        report["would_keep"],
        report["margin_db"].as_f64().unwrap_or(0.0),
        report["enough_blocks"],
    ));
    for key in [
        "upper_under_with_a_line",
        "upper_under_without",
        "middle_under_with_a_line",
        "middle_under_without",
        "upper_under",
        "middle_under",
        "bar_on_upper_tenth",
        "bar_on_middle",
    ] {
        if !report[key].is_null() {
            out.push_str(&format!("{key}: {}\n", report[key]));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(kind: &str, onset: u64, asker: u8, upper: u8, heard: u8, blocks: u64) -> Value {
        json!({"kind":kind,"at_us":onset + 150_000,"turn":7,"onset_from_us":onset,
            "asker_level":asker,"heard_level":heard,"heard_upper_tenth":upper,
            "blocks":blocks,"would_keep":upper + MARGIN >= asker,"motion_voice":"record"})
    }

    #[test]
    fn rows_come_from_both_kinds_and_a_record_without_levels_has_none() {
        let let_go =
            Row::from_record(&record("input_let_go_body_moving", 1_000, 150, 140, 120, 8)).unwrap();
        assert!(!let_go.kept);
        assert_eq!(let_go.under(), Some((10, 30)));
        assert!(let_go.readable());
        let kept =
            Row::from_record(&record("input_kept_body_moving", 1_000, 150, 149, 140, 8)).unwrap();
        assert!(kept.kept);
        let off = Row::from_record(&json!({"kind":"input_let_go_body_moving","at_us":5,
            "onset_from_us":4,"turn":1}))
        .unwrap();
        assert_eq!((off.under(), off.blocks, off.would_keep), (None, 0, None));
        assert!(Row::from_record(&json!({"kind":"input_let_go_answer_waits","at_us":5})).is_none());
        let short = Row::from_record(&record(
            "input_let_go_body_moving",
            1_000,
            150,
            150,
            150,
            ENOUGH_BLOCKS as u64 - 1,
        ))
        .unwrap();
        assert!(!short.readable());
    }

    /// `#PLAY_US` lines end at the next line or 4 s on; a `plays.tsv`'s
    /// lines carry their end; the offset moves both; a line is sounding
    /// from 0.2 s before it to 0.3 s after.
    #[test]
    fn plays_read_both_files_and_a_line_sounds_from_just_before_to_just_after() {
        let scene = "header\n#PLAY_US 10000000 A: words\n#PLAY_US 12000000 B: more\nother\n";
        assert_eq!(
            plays(scene, 0),
            [(10_000_000, 12_000_000), (12_000_000, 16_000_000)]
        );
        assert_eq!(plays("1000\t3000\tA\n", 500), [(1_500, 3_500)]);
        let spans = plays(scene, 0);
        assert!(during(&spans, 10_000_000 - BEFORE_US));
        assert!(!during(&spans, 10_000_000 - BEFORE_US - 1));
        assert!(during(&spans, 16_000_000 + AFTER_US));
        assert!(!during(&spans, 16_000_000 + AFTER_US + 1));
    }

    /// The bar keeps every line and lets the rest go when they don't
    /// overlap: the tightest such bar, and the clean range; when they do,
    /// the fewest wrong.
    #[test]
    fn the_bar_keeps_the_lines_and_lets_the_rest_go() {
        let clean = bar(&[0, 10, 20], &[60, 80]).unwrap();
        assert_eq!(clean["bar"], 20);
        assert_eq!(clean["clean"], true);
        assert_eq!(clean["clean_from_db"], json!(2.7));
        assert_eq!(clean["clean_to_db"], json!(7.9));
        // One line as quiet as the rest: a bar at 10 lets that one go.
        let overlap = bar(&[0, 10, 70], &[30, 60, 90]).unwrap();
        assert_eq!(overlap["clean"], false);
        assert_eq!(overlap["bar"], 10);
        assert_eq!(
            (
                overlap["lines_let_go"].as_u64(),
                overlap["rest_kept"].as_u64()
            ),
            (Some(1), Some(0))
        );
        // Two bars as good (one wrong each): the tighter keeps fewer motors.
        let tie = bar(&[0, 50], &[30, 80]).unwrap();
        assert_eq!(tie["bar"], 0);
        assert!(bar(&[], &[1]).is_none());
    }

    /// The report splits the readable sounds by whether a line was sounding,
    /// counts the rest, and gives the bar on the upper tenth and the middle.
    #[test]
    fn the_report_splits_by_a_played_line_and_gives_both_bars() {
        let events = [
            record("input_let_go_body_moving", 10_100_000, 150, 145, 130, 10),
            record("input_let_go_body_moving", 12_500_000, 150, 140, 120, 12),
            record("input_let_go_body_moving", 30_000_000, 150, 60, 40, 9),
            record("input_let_go_body_moving", 31_000_000, 150, 150, 150, 2),
            json!({"kind":"input_let_go_body_moving","at_us":32_000_000,"onset_from_us":32_000_000}),
        ];
        let rows: Vec<Row> = events.iter().filter_map(Row::from_record).collect();
        let spans = plays("#PLAY_US 10000000 A: x\n#PLAY_US 12000000 B: y\n", 0);
        let report = report(rows, Some(&spans));
        assert_eq!(report["judged"], 5);
        assert_eq!(report["no_askers_level"], 1);
        assert_eq!(report["too_few_blocks"], 1);
        assert_eq!(report["upper_under_with_a_line"]["count"], 2);
        assert_eq!(report["upper_under_without"]["count"], 1);
        assert_eq!(report["bar_on_upper_tenth"]["clean"], true);
        assert_eq!(report["bar_on_middle"]["clean"], true);
        assert_eq!(report["rows"][0]["during_line"], true);
        assert_eq!(report["rows"][2]["during_line"], false);
        let text = table(&report);
        assert!(text.starts_with("at_us\tturn\tblocks"), "{text}");
        assert!(
            text.contains("judged 5 | no asker's level 1 | too few blocks 1"),
            "{text}"
        );
        // Without plays: one spread, no bar.
        let rows: Vec<Row> = events.iter().filter_map(Row::from_record).collect();
        let plain = super::report(rows, None);
        assert_eq!(plain["upper_under"]["count"], 3);
        assert!(plain["bar_on_upper_tenth"].is_null());
        assert!(plain["rows"][0]["during_line"].is_null());
    }
}
