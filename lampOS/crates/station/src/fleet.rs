//! Whether every pilot lamp's record is complete against a matrix version
//! (#2795): for each lamp's folder in the records' place (#2794), or each
//! lamp named, its latest routine or retest record judged by
//! [`complete::check`], one line per lamp.
//!
//! "Latest" is by the record's own `started`, the run's start by the
//! station's clock in UTC, then its `ended`, then its run id; never a
//! file's modified time, which a copy to a share or a restore changes. A
//! qualification run is not the lamp's production run and is passed over.
//!
//! A lamp with no routine or retest record is `no record`. A file in its
//! folder that is not its record is named, and the lamp does not stand
//! complete: that file may be its latest run. It reads the place and
//! changes nothing.
use crate::{
    Refused,
    complete::{self, Completeness},
    matrix::Matrix,
    record::{Occasion, Record, RunVerdict},
    store,
};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// One lamp, judged.
#[derive(Debug)]
pub struct Lamp {
    pub name: String,
    /// Its latest routine or retest record, judged; none when it has none.
    pub latest: Option<Completeness>,
    /// Files in its folder that are not its record, with why.
    pub not_records: Vec<(PathBuf, Refused)>,
}

impl Lamp {
    /// Complete: its latest run's record is, and every file in its folder
    /// is its record.
    pub fn complete(&self) -> bool {
        self.not_records.is_empty() && self.latest.as_ref().is_some_and(Completeness::complete)
    }

    /// What a person reads for the lamp: `<lamp> complete: run <id>
    /// <verdict>`, `<lamp> not complete: run <id> <verdict>; gaps: ...`
    /// with each gap's kind and check, or `<lamp> no record`; then a line
    /// for each file that is not its record. The verdict is the one its
    /// checks give, judged here.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![match &self.latest {
            None => format!("{} no record", self.name),
            Some(judged) => {
                let state = if self.complete() {
                    "complete"
                } else {
                    "not complete"
                };
                let mut line = format!(
                    "{} {state}: run {} {}",
                    self.name,
                    judged.run_id,
                    verdict(judged.verdict)
                );
                if !judged.gaps.is_empty() {
                    let gaps: Vec<String> = judged.gaps.iter().map(|g| g.short()).collect();
                    line.push_str(&format!("; gaps: {}", gaps.join(", ")));
                }
                line
            }
        }];
        for (path, why) in &self.not_records {
            lines.push(format!(
                "{} not a record of {}: {}: {why}",
                self.name,
                self.name,
                path.display()
            ));
        }
        lines
    }
}

fn verdict(verdict: RunVerdict) -> &'static str {
    match verdict {
        RunVerdict::Pass => "pass",
        RunVerdict::Fail => "fail",
        RunVerdict::Incomplete => "incomplete",
    }
}

/// The lamps with a folder in `place`, by name, and the entries there that
/// are not a lamp's folder (a file, a link, a name no lamp has). Names
/// starting with a dot are passed over unseen, as a share's own files.
pub fn lamps_in(place: &Path) -> io::Result<(Vec<String>, Vec<String>)> {
    let mut lamps = Vec::new();
    let mut others = Vec::new();
    for entry in fs::read_dir(place)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let folder = entry.file_type()?.is_dir();
        if folder && store::lamp_dir(place, &name).is_ok() {
            lamps.push(name);
        } else {
            others.push(name);
        }
    }
    lamps.sort();
    others.sort();
    Ok((lamps, others))
}

/// The latest routine or retest record among `found`.
pub fn latest(found: &[store::Found]) -> Option<&Record> {
    found
        .iter()
        .filter_map(|(_, read)| read.as_ref().ok())
        .filter(|record| record.occasion != Occasion::Qualification)
        .max_by(|a, b| (a.started, a.ended, &a.run_id).cmp(&(b.started, b.ended, &b.run_id)))
}

/// `lamp`'s latest routine or retest record in `place`, judged against
/// `matrix`.
pub fn judge(place: &Path, lamp: &str, matrix: &Matrix) -> io::Result<Lamp> {
    let found = store::lamp_records(place, lamp)?;
    let latest = latest(&found).map(|record| complete::check(matrix, record));
    let not_records = found
        .into_iter()
        .filter_map(|(path, read)| read.err().map(|why| (path, why)))
        .collect();
    Ok(Lamp {
        name: lamp.to_owned(),
        latest,
        not_records,
    })
}

/// The lines to print for `lamps` and the exit status: 0 every lamp is
/// complete, 1 one is not (or has no record), 3 there is no lamp at all,
/// so an empty place never reads as complete. `others` are the place's
/// entries that are not a lamp's folder, listed and not judged.
pub fn summary(matrix: &Matrix, lamps: &[Lamp], others: &[String]) -> (Vec<String>, u8) {
    let mut lines: Vec<String> = lamps.iter().flat_map(Lamp::lines).collect();
    for other in others {
        lines.push(format!("not a lamp's folder: {other}"));
    }
    let complete = lamps.iter().filter(|lamp| lamp.complete()).count();
    lines.push(format!(
        "{complete} of {} lamps complete against matrix v{}",
        lamps.len(),
        matrix.suite.version
    ));
    let status = if lamps.is_empty() {
        3
    } else if complete == lamps.len() {
        0
    } else {
        1
    };
    (lines, status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::complete::tests::{matrix, passed};

    /// `record` for `lamp`, started at `hour` o'clock.
    fn on(record: &Record, lamp: &str, hour: u8) -> Record {
        let mut record = record.clone();
        record.device.id = lamp.into();
        record.run_id = format!("20261019T{hour:02}0000Z-station-01-{lamp}");
        record.started = format!("2026-10-19T{hour:02}:00:00Z").parse().unwrap();
        record.ended = format!("2026-10-19T{hour:02}:30:00Z").parse().unwrap();
        record
    }

    fn judge_all(place: &Path, matrix: &Matrix) -> (Vec<String>, u8) {
        let (names, others) = lamps_in(place).unwrap();
        let lamps: Vec<Lamp> = names
            .iter()
            .map(|name| judge(place, name, matrix).unwrap())
            .collect();
        summary(matrix, &lamps, &others)
    }

    #[test]
    fn every_lamp_complete_exits_0() {
        let place = tempfile::tempdir().unwrap();
        let matrix = matrix();
        for lamp in ["lamp-4ace", "lamp-52e6"] {
            store::keep_for_lamp(place.path(), &on(&passed(&matrix), lamp, 8)).unwrap();
        }
        let (lines, status) = judge_all(place.path(), &matrix);
        assert_eq!(status, 0, "{lines:?}");
        assert_eq!(
            lines,
            [
                "lamp-4ace complete: run 20261019T080000Z-station-01-lamp-4ace pass",
                "lamp-52e6 complete: run 20261019T080000Z-station-01-lamp-52e6 pass",
                "2 of 2 lamps complete against matrix v7",
            ]
        );
    }

    #[test]
    fn a_missing_check_is_named_and_exits_1() {
        let place = tempfile::tempdir().unwrap();
        let matrix = matrix();
        store::keep_for_lamp(place.path(), &on(&passed(&matrix), "lamp-4ace", 8)).unwrap();
        let mut short = on(&passed(&matrix), "lamp-52e6", 8);
        short.checks.retain(|c| c.id != "privacy_switch");
        store::keep_for_lamp(place.path(), &short).unwrap();
        let (lines, status) = judge_all(place.path(), &matrix);
        assert_eq!(status, 1, "{lines:?}");
        assert!(lines[0].starts_with("lamp-4ace complete: "), "{lines:?}");
        assert!(
            lines[1].starts_with(
                "lamp-52e6 not complete: run 20261019T080000Z-station-01-lamp-52e6 incomplete; gaps: "
            ),
            "{lines:?}"
        );
        assert!(lines[1].contains("missing privacy_switch"), "{lines:?}");
        assert_eq!(lines[2], "1 of 2 lamps complete against matrix v7");
    }

    /// A lamp named with no records, or a folder holding only a
    /// qualification run, is `no record`; a place with no lamp exits 3.
    #[test]
    fn a_lamp_with_no_record_is_not_complete() {
        let place = tempfile::tempdir().unwrap();
        let matrix = matrix();
        let mut qualification = on(&passed(&matrix), "lamp-52e6", 8);
        qualification.occasion = Occasion::Qualification;
        store::keep_for_lamp(place.path(), &qualification).unwrap();
        let named = judge(place.path(), "lamp-0c89", &matrix).unwrap();
        assert!(!named.complete());
        assert_eq!(named.lines(), ["lamp-0c89 no record"]);
        let (lines, status) = judge_all(place.path(), &matrix);
        assert_eq!(status, 1);
        assert_eq!(lines[0], "lamp-52e6 no record");
        let empty = tempfile::tempdir().unwrap();
        let (lines, status) = judge_all(empty.path(), &matrix);
        assert_eq!(status, 3);
        assert_eq!(lines, ["0 of 0 lamps complete against matrix v7"]);
    }

    /// The latest is the record's own start, not its file's name order or
    /// modified time: a retest that started later is the one judged though
    /// its file sorts first and was written first.
    #[test]
    fn the_latest_run_by_its_own_start_is_judged() {
        let place = tempfile::tempdir().unwrap();
        let matrix = matrix();
        let lamp = "lamp-4ace";
        let mut retest = on(&passed(&matrix), lamp, 9);
        retest.occasion = Occasion::Retest;
        retest.run_id = format!("0-retest-{lamp}");
        store::keep_for_lamp(place.path(), &retest).unwrap();
        let mut earlier = on(&passed(&matrix), lamp, 8);
        earlier.checks.retain(|c| c.id != "ring");
        store::keep_for_lamp(place.path(), &earlier).unwrap();
        let judged = judge(place.path(), lamp, &matrix).unwrap();
        assert!(judged.complete(), "{:?}", judged.lines());
        assert_eq!(judged.latest.unwrap().run_id, retest.run_id);

        // The other way round, the earlier complete run does not hide a
        // later incomplete one.
        let place = tempfile::tempdir().unwrap();
        let mut later = on(&passed(&matrix), lamp, 10);
        later.checks.retain(|c| c.id != "ring");
        store::keep_for_lamp(place.path(), &later).unwrap();
        store::keep_for_lamp(place.path(), &on(&passed(&matrix), lamp, 8)).unwrap();
        let judged = judge(place.path(), lamp, &matrix).unwrap();
        assert!(!judged.complete());
        assert!(
            judged.lines()[0].contains("missing ring"),
            "{:?}",
            judged.lines()
        );
    }

    /// A file in a lamp's folder that is not its record keeps the lamp from
    /// standing complete, and is named; a stray file in the place is
    /// listed, not judged.
    #[test]
    fn what_is_not_a_record_is_named() {
        let place = tempfile::tempdir().unwrap();
        let matrix = matrix();
        store::keep_for_lamp(place.path(), &on(&passed(&matrix), "lamp-4ace", 8)).unwrap();
        fs::write(place.path().join("lamp-4ace/half.json"), "{").unwrap();
        fs::write(place.path().join("README"), "records").unwrap();
        fs::write(place.path().join(".DS_Store"), "").unwrap();
        let (lines, status) = judge_all(place.path(), &matrix);
        assert_eq!(status, 1, "{lines:?}");
        assert!(
            lines[0].starts_with("lamp-4ace not complete: "),
            "{lines:?}"
        );
        assert!(lines[1].starts_with("lamp-4ace not a record of lamp-4ace: "));
        assert_eq!(lines[2], "not a lamp's folder: README");
        assert_eq!(lines.len(), 4);
    }
}
