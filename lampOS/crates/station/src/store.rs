//! Where the station keeps a record: `<dir>/<run id>.json`, written whole
//! and then renamed, as the self-test keeps its reports
//! (`crates/selftest/src/report.rs`), so a reader or a power cut never finds
//! half a record. A record is evidence: one already there is never written
//! over.
use crate::record::Record;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Keeps `record` in `dir` and returns its path. Refuses, with
/// `AlreadyExists`, when that run is already recorded.
pub fn keep(dir: &Path, record: &Record) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.json", record.run_id));
    if path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} is already recorded", path.display()),
        ));
    }
    let mut json = serde_json::to_vec_pretty(record).map_err(io::Error::other)?;
    json.push(b'\n');
    let part = path.with_extension("part");
    fs::write(&part, &json)?;
    fs::File::open(&part)?.sync_all()?;
    fs::rename(&part, &path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{Device, FORMAT, Occasion, Record, RunVerdict, Suite};

    fn record() -> Record {
        Record {
            format: FORMAT,
            run_id: "20261019T080000Z-station-01-lamp-4ace".into(),
            occasion: Occasion::Routine,
            verdict: RunVerdict::Incomplete,
            started: "2026-10-19T08:00:00Z".parse().unwrap(),
            ended: "2026-10-19T08:30:00Z".parse().unwrap(),
            device: Device {
                id: "lamp-4ace".into(),
                hardware_revision: "draft-a".into(),
                batch: "pilot-1".into(),
            },
            lampos_build: "dab3f12".into(),
            suite: Suite {
                matrix_version: "0".into(),
                matrix_sha256: "0".repeat(64),
                matrix_approved: false,
                tool_commit: "dab3f12".into(),
            },
            station: "station-01".into(),
            operator: "op-7".into(),
            retest_of: None,
            repairs: Vec::new(),
            selftest: None,
            checks: Vec::new(),
            failures: Vec::new(),
            incomplete: Vec::new(),
        }
    }

    #[test]
    fn a_record_is_kept_whole_and_never_written_over() {
        let dir = tempfile::tempdir().unwrap();
        let records = dir.path().join("records");
        let path = keep(&records, &record()).unwrap();
        assert_eq!(
            path,
            records.join("20261019T080000Z-station-01-lamp-4ace.json")
        );
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(Record::parse(&text).unwrap(), record());
        assert!(text.ends_with("}\n"));
        let names: Vec<_> = fs::read_dir(&records)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
        let mut other = record();
        other.verdict = RunVerdict::Pass;
        let error = keep(&records, &other).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            Record::parse(&fs::read_to_string(&path).unwrap()).unwrap(),
            record()
        );
    }
}
