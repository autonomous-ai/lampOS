//! The measure of how often the face read is wrong (#1671, step 0; the first
//! builder, 20 Oct 16:16): `lamp-eyes --seems-log PATH` keeps one line a
//! read, its time, kind, confidence and reader, and nothing of whose face.
//! The ground truth is a labelled session at a lamp, a person's own word on
//! how they feel at times a builder notes. `lamp-seems-measure` counts the
//! reads against those labels and deletes both files. Off unless given, and
//! removed with the measure.
use lamp_msgs::{HumanSeems, SeemsKind, SeemsReader};
use serde::Deserialize;
use std::{
    fs::{File, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};

/// The one place the log may be: the reset plans wipe it, so a factory reset
/// never leaves hours of face reads behind (the reviewer on #2306).
pub const PATH: &str = "/var/lib/lampos-eyes/seems.log";
/// Then it stops writing. A reader writes at most one line every 2 s while
/// a face is near, so about 11 hours of face time with one reader, and 5.5
/// with both side by side: a day at the desk, enough for a labelled session
/// (Reviewer-lead on #2306).
pub const MOST_LINES: u32 = 20_000;
/// A read counts against a label at most this far from it, either side.
pub const WINDOW_S: u64 = 10;
/// The confidences the error is counted at: a read is judged at each of
/// these it is at or over.
pub const AT_CONFIDENCE: [f32; 5] = [0.5, 0.6, 0.7, 0.8, 0.9];
/// The bar: wrong at most 1 in 10 at the confidence used (#1671).
pub const BAR: f64 = 0.1;
/// Fewer reads judged than this say nothing of the bar: one right read is
/// not "wrong at most 1 in 10" (Reviewer-lead on #2306).
pub const FEWEST_JUDGED: u32 = 20;

/// One read, as a line: `{"t": unix_s, "kind", "confidence", "from"}`. The
/// confidence is the reader's own, unrounded, so the measure's cut-offs
/// judge what the eyes judged (Reviewer-lead on #2306).
pub fn line(read: &HumanSeems, unix_s: u64) -> String {
    // A struct, not `json!`: an f32 there is widened to an f64 first, and
    // 0.734 would be written 0.7339999675750732.
    #[derive(serde::Serialize)]
    struct Line {
        t: u64,
        kind: SeemsKind,
        confidence: f32,
        from: SeemsReader,
    }
    serde_json::to_string(&Line {
        t: unix_s,
        kind: read.kind,
        confidence: read.confidence,
        from: read.from,
    })
    .unwrap_or_default()
}

/// The log, open for appending.
pub struct Log {
    file: File,
    lines: u32,
    full_said: bool,
    failed_said: bool,
}

impl Log {
    /// Opens `path` for appending, made 0600 if new. An existing file must
    /// be a plain file of its owner's only: a link is refused as it is
    /// opened (`O_NOFOLLOW`, so nothing can swap one in after a check,
    /// Reviewer-lead on #2306), and one that isn't a plain file, or that
    /// others may read, is refused from what was opened. Its lines are
    /// counted, so a restart goes on to the same stop.
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)?;
        let meta = file.metadata()?;
        if !meta.file_type().is_file() {
            return Err(io::Error::other("the seems log is not a plain file"));
        }
        if meta.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::other("the seems log is open to others"));
        }
        let lines = BufReader::new(file.try_clone()?).lines().count();
        Ok(Self {
            file,
            lines: u32::try_from(lines).unwrap_or(u32::MAX),
            full_said: false,
            failed_said: false,
        })
    }

    /// Writes one read, unless the log is full. What the journal says of
    /// it: a full log, or the first write that failed, each once, so a
    /// failing disk isn't said at every read (Reviewer-lead on #2306).
    pub fn write(&mut self, read: &HumanSeems, unix_s: u64) -> Option<String> {
        if self.lines >= MOST_LINES {
            return (!std::mem::replace(&mut self.full_said, true))
                .then(|| "the seems log is full; no more reads are written to it".to_owned());
        }
        match writeln!(self.file, "{}", line(read, unix_s)) {
            Ok(()) => {
                self.lines += 1;
                None
            }
            Err(error) => (!std::mem::replace(&mut self.failed_said, true)).then(|| {
                format!(
                    "a read wasn't written to the seems log ({error}); a later failure isn't said"
                )
            }),
        }
    }
}

#[derive(Debug, Deserialize)]
struct Read {
    t: u64,
    kind: SeemsKind,
    confidence: f32,
    #[serde(default)]
    from: SeemsReader,
}

/// What the person said they felt, at a time a builder noted.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Says {
    Positive,
    Negative,
    /// Neither: a read that names a kind is wrong here.
    Neutral,
}

#[derive(Debug, Deserialize)]
struct Label {
    t: u64,
    says: Says,
}

/// The error at one confidence, for one reader.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub from: SeemsReader,
    pub at: f32,
    /// Reads at or over `at` that named a kind, near a label.
    pub judged: u32,
    pub wrong: u32,
}

impl Row {
    pub fn rate(&self) -> Option<f64> {
        (self.judged > 0).then(|| f64::from(self.wrong) / f64::from(self.judged))
    }
}

/// The reads' lines against the labels' lines: for each reader and each
/// confidence, the reads that named a kind within [`WINDOW_S`] of a label,
/// and how many named the wrong one. An `unsure` read names none and isn't
/// judged. A line that isn't a read or a label is an error, with its number.
pub fn measure(reads: &str, labels: &str) -> Result<Vec<Row>, String> {
    let parse = |text: &str, what: &str| -> Result<Vec<serde_json::Value>, String> {
        text.lines()
            .enumerate()
            .filter(|(_, line)| !line.trim().is_empty())
            .map(|(n, line)| {
                serde_json::from_str(line).map_err(|_| format!("{what} line {} isn't one", n + 1))
            })
            .collect()
    };
    let reads: Vec<Read> = parse(reads, "read")?
        .into_iter()
        .enumerate()
        .map(|(n, v)| serde_json::from_value(v).map_err(|_| format!("read {} isn't one", n + 1)))
        .collect::<Result<_, _>>()?;
    let labels: Vec<Label> = parse(labels, "label")?
        .into_iter()
        .enumerate()
        .map(|(n, v)| serde_json::from_value(v).map_err(|_| format!("label {} isn't one", n + 1)))
        .collect::<Result<_, _>>()?;
    let mut rows = Vec::new();
    for from in [SeemsReader::FerPlus, SeemsReader::Blendshapes] {
        for at in AT_CONFIDENCE {
            let mut row = Row {
                from,
                at,
                judged: 0,
                wrong: 0,
            };
            for read in reads.iter().filter(|read| {
                read.from == from && read.confidence >= at && read.kind != SeemsKind::Unsure
            }) {
                // The nearest label within the window.
                let Some(label) = labels
                    .iter()
                    .filter(|label| label.t.abs_diff(read.t) <= WINDOW_S)
                    .min_by_key(|label| label.t.abs_diff(read.t))
                else {
                    continue;
                };
                row.judged += 1;
                let right = matches!(
                    (read.kind, label.says),
                    (SeemsKind::Positive, Says::Positive) | (SeemsKind::Negative, Says::Negative)
                );
                row.wrong += u32::from(!right);
            }
            if row.judged > 0 {
                rows.push(row);
            }
        }
    }
    Ok(rows)
}

/// The rows as a table, each against [`BAR`].
pub fn table(rows: &[Row]) -> String {
    let mut out = String::from("reader\tat\tjudged\twrong\trate\tunder the bar\n");
    for row in rows {
        let rate = row.rate().unwrap_or(0.0);
        let under = if row.judged < FEWEST_JUDGED {
            "too few"
        } else if rate <= BAR {
            "yes"
        } else {
            "no"
        };
        out.push_str(&format!(
            "{}\t{:.1}\t{}\t{}\t{:.3}\t{}\n",
            match row.from {
                SeemsReader::FerPlus => "fer_plus",
                SeemsReader::Blendshapes => "blendshapes",
            },
            row.at,
            row.judged,
            row.wrong,
            rate,
            under
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("seems-log-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn read(kind: SeemsKind, confidence: f32, from: SeemsReader) -> HumanSeems {
        HumanSeems {
            kind,
            confidence,
            from,
        }
    }

    #[test]
    fn a_read_is_its_time_kind_confidence_and_reader_only() {
        let line = line(
            &read(SeemsKind::Negative, 0.734, SeemsReader::Blendshapes),
            1_800_000_000,
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&line).unwrap(),
            serde_json::json!({"t": 1_800_000_000u64, "kind": "negative", "confidence": 0.734, "from": "blendshapes"})
        );
    }

    #[test]
    fn the_log_is_its_owners_only_and_stops_at_its_most() {
        let dir = dir("open");
        let path = dir.join("seems.log");
        let mut log = Log::open(&path).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let one = read(SeemsKind::Positive, 0.9, SeemsReader::FerPlus);
        assert_eq!(log.write(&one, 1), None);
        assert_eq!(log.write(&one, 2), None);
        // Reopened: it counts on from its lines.
        let mut log = Log::open(&path).unwrap();
        assert_eq!(log.lines, 2);
        log.lines = MOST_LINES;
        assert!(log.write(&one, 3).is_some(), "full is said once");
        assert_eq!(log.write(&one, 4), None);
        assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 2);
        // Open to others, or a link: refused.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Log::open(&path).is_err());
        // A link to a file of its owner's only: refused for being a link.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let link = dir.join("link.log");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(Log::open(&link).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A write that fails is said once, not at every read.
    #[test]
    fn a_failed_write_is_said_once() {
        let mut log = Log {
            file: OpenOptions::new().append(true).open("/dev/full").unwrap(),
            lines: 0,
            full_said: false,
            failed_said: false,
        };
        let one = read(SeemsKind::Positive, 0.9, SeemsReader::FerPlus);
        assert!(
            log.write(&one, 1)
                .is_some_and(|said| said.contains("wasn't written"))
        );
        assert_eq!(log.write(&one, 2), None);
        assert_eq!(log.lines, 0);
    }

    /// The measure judges the reader's own confidence: 0.695 is under 0.7.
    #[test]
    fn a_read_just_under_a_cut_off_is_not_judged_at_it() {
        let line = line(&read(SeemsKind::Positive, 0.695, SeemsReader::FerPlus), 100);
        let rows = measure(&line, r#"{"t":100,"says":"positive"}"#).unwrap();
        assert!(rows.iter().any(|row| (row.at - 0.6).abs() < 1e-6));
        assert!(
            !rows.iter().any(|row| (row.at - 0.7).abs() < 1e-6),
            "{rows:?}"
        );
    }

    #[test]
    fn reads_are_counted_wrong_or_right_against_the_nearest_label_by_reader_and_confidence() {
        let reads = [
            // Near "positive" at 100: right, then wrong.
            r#"{"t":98,"kind":"positive","confidence":0.92,"from":"blendshapes"}"#,
            r#"{"t":103,"kind":"negative","confidence":0.65,"from":"blendshapes"}"#,
            // Unsure: not judged.
            r#"{"t":104,"kind":"unsure","confidence":0.4,"from":"blendshapes"}"#,
            // Too far from any label: not judged.
            r#"{"t":150,"kind":"negative","confidence":0.95,"from":"blendshapes"}"#,
            // Near "neutral" at 200: any named kind is wrong.
            r#"{"t":205,"kind":"positive","confidence":0.55,"from":"blendshapes"}"#,
            // FER+, from a line before `from` was written: right.
            r#"{"t":301,"kind":"negative","confidence":0.8}"#,
        ]
        .join("\n");
        let labels = [
            r#"{"t":100,"says":"positive"}"#,
            r#"{"t":200,"says":"neutral"}"#,
            r#"{"t":300,"says":"negative"}"#,
        ]
        .join("\n");
        let rows = measure(&reads, &labels).unwrap();
        let at = |from, at: f32| {
            rows.iter()
                .find(|row| row.from == from && (row.at - at).abs() < 1e-6)
                .map(|row| (row.judged, row.wrong))
        };
        assert_eq!(at(SeemsReader::Blendshapes, 0.5), Some((3, 2)));
        assert_eq!(at(SeemsReader::Blendshapes, 0.6), Some((2, 1)));
        assert_eq!(at(SeemsReader::Blendshapes, 0.7), Some((1, 0)));
        assert_eq!(at(SeemsReader::FerPlus, 0.8), Some((1, 0)));
        assert_eq!(at(SeemsReader::FerPlus, 0.9), None);
        let table = table(&rows);
        // So few reads say nothing of the bar, either way.
        assert!(
            table.contains("blendshapes\t0.5\t3\t2\t0.667\ttoo few"),
            "{table}"
        );
        assert!(
            table.contains("blendshapes\t0.7\t1\t0\t0.000\ttoo few"),
            "{table}"
        );
        let many = |judged, wrong| Row {
            from: SeemsReader::FerPlus,
            at: 0.8,
            judged,
            wrong,
        };
        assert!(super::table(&[many(20, 2)]).contains("\tyes\n"));
        assert!(super::table(&[many(20, 3)]).contains("\tno\n"));
        assert!(super::table(&[many(19, 0)]).contains("\ttoo few\n"));
        assert!(measure("not json", &labels).is_err());
        assert!(measure(&reads, r#"{"t":1,"says":"happy"}"#).is_err());
    }
}
