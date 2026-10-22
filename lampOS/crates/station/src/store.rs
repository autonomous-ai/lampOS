//! Where the station keeps a record: `<dir>/<run id>.json`, written whole
//! and then renamed, as the self-test keeps its reports
//! (`crates/selftest/src/report.rs`), so a reader or a power cut never finds
//! half a record. A record is evidence: one already there is never written
//! over.
//!
//! The records' place (#2794), the folder the hardware lead chooses for the
//! fleet's records (#1700), keeps each lamp's records in a folder of its
//! own, `<place>/<lamp>/<run id>.json`, so they are found by the lamp's
//! name, side by side and never written over. Each record carries its
//! matrix version and the tool's commit. More than one station may write
//! there at once: each writer's part is a file of its own, made new, so one
//! writer never changes another's published record (reviewer-two on #3054).
//! The place has to make hard links, as publishing without replacing needs
//! them; one that can't refuses every record, which stays in `--out`.
use crate::{Refused, record::Record};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
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
    let part = write_part(dir, &record.run_id, &json)?;
    place(&part, &path, |from, to| fs::hard_link(from, to))?;
    Ok(path)
}

/// Writes `bytes` to a part file of this writer's own in `dir`, made new
/// (`create_new`), synced, and returns its path. Its name is never another
/// writer's: the process, the clock and a count in this process.
fn write_part(dir: &Path, run_id: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let part = dir.join(format!(
        ".{run_id}.{}-{nanos}-{}.part",
        std::process::id(),
        COUNT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&part)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    if let Err(error) = written {
        let _ = fs::remove_file(&part);
        return Err(error);
    }
    Ok(part)
}

/// The folder of `lamp`'s records in `place`. A lamp's id is a plain name
/// (the record refuses any other). Here it must also be lower case, since a
/// share may not tell `Lamp-4ACE` from `lamp-4ace`, and not start with a
/// dot, such as `..`: either would not be a folder of its own.
pub fn lamp_dir(place: &Path, lamp: &str) -> io::Result<PathBuf> {
    if lamp.is_empty()
        || lamp.starts_with('.')
        || !lamp.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.')
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{lamp:?} cannot name a lamp's folder of records"),
        ));
    }
    Ok(place.join(lamp))
}

/// Keeps `record` in the records' place, in its lamp's folder, and returns
/// its path. Refuses, with `AlreadyExists`, when that run is already there.
/// A lamp's folder that is a link to elsewhere is refused, never followed.
pub fn keep_for_lamp(place: &Path, record: &Record) -> io::Result<PathBuf> {
    let dir = lamp_dir(place, &record.device.id)?;
    fs::create_dir_all(place)?;
    match fs::create_dir(&dir) {
        Err(error) if error.kind() != io::ErrorKind::AlreadyExists => return Err(error),
        _ => {}
    }
    if !fs::symlink_metadata(&dir)?.file_type().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a folder of its own", dir.display()),
        ));
    }
    keep(&dir, record)
}

/// One record file found in a lamp's folder: its path, and the record or
/// why it could not be read. A file that is not a record of this lamp is
/// listed with its reason, never left out unseen.
pub type Found = (PathBuf, Result<Record, Refused>);

/// `lamp`'s records in the place, oldest first (a run id begins with its
/// start, UTC). None when the lamp has no folder there yet.
pub fn lamp_records(place: &Path, lamp: &str) -> io::Result<Vec<Found>> {
    let dir = lamp_dir(place, lamp)?;
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut paths: Vec<PathBuf> = entries
        .map(|entry| entry.map(|e| e.path()))
        .collect::<io::Result<_>>()?;
    paths.retain(|path| path.extension().is_some_and(|ext| ext == "json"));
    paths.sort();
    Ok(paths
        .into_iter()
        .map(|path| {
            let read = fs::read_to_string(&path)
                .map_err(|error| Refused(format!("cannot be read: {error}")))
                .and_then(|text| Record::parse(&text))
                .and_then(|record| {
                    let name = format!("{}.json", record.run_id);
                    if record.device.id != lamp {
                        Err(Refused(format!(
                            "a record of {}, not {lamp}",
                            record.device.id
                        )))
                    } else if path.file_name().is_none_or(|file| file != name.as_str()) {
                        Err(Refused(format!(
                            "its run is {}, not its file's name",
                            record.run_id
                        )))
                    } else {
                        Ok(record)
                    }
                });
            (path, read)
        })
        .collect())
}

/// Puts the written `part` at `path` with `link` without ever replacing a
/// file there: a link fails with `AlreadyExists` where a rename would write
/// over a record that appeared after the check above (reviewer-two on
/// #1785). `part` is removed either way. A folder that can't link says so.
fn place(
    part: &Path,
    path: &Path,
    link: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    let linked = link(part, path);
    let removed = fs::remove_file(part);
    match linked {
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} is already recorded", path.display()),
        )),
        Err(error) => Err(io::Error::new(
            error.kind(),
            format!(
                "{} could not be put in place by a hard link, which the folder must allow: {error}",
                path.display()
            ),
        )),
        Ok(()) => removed,
    }
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

    /// reviewer-two on #3054: a second writer of the same run, between the
    /// first's link and the removal of its part, never changes the record
    /// the first published: each writes a part of its own.
    #[test]
    fn a_second_writer_of_the_same_run_never_changes_a_published_record() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.json");
        let first = write_part(dir.path(), "run", b"the first record\n").unwrap();
        let second = write_part(dir.path(), "run", b"the second record\n").unwrap();
        assert_ne!(first, second);
        // The first is published, and before its part is removed the second
        // has written its own and tries to publish it.
        fs::hard_link(&first, &path).unwrap();
        let error = place(&second, &path, |a, b| fs::hard_link(a, b)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        fs::remove_file(&first).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"the first record\n");
        let left: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left, ["run.json"]);
    }

    /// Eight stations keeping the same run at once: exactly one record is
    /// kept, whole and as its writer made it, and no part is left.
    #[test]
    fn writers_at_once_keep_exactly_one_whole_record() {
        let place = tempfile::tempdir().unwrap();
        let start = std::sync::Barrier::new(8);
        let results: Vec<(String, io::Result<PathBuf>)> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|n| {
                    let (place, start) = (place.path(), &start);
                    scope.spawn(move || {
                        let mut mine = record();
                        mine.operator = format!("op-{n}");
                        start.wait();
                        (mine.operator.clone(), keep_for_lamp(place, &mine))
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let kept: Vec<_> = results.iter().filter(|(_, r)| r.is_ok()).collect();
        assert_eq!(kept.len(), 1, "{results:?}");
        for (_, result) in &results {
            if let Err(error) = result {
                assert_eq!(error.kind(), io::ErrorKind::AlreadyExists, "{error}");
            }
        }
        let path = kept[0].1.as_ref().unwrap();
        let read = Record::parse(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(read.operator, kept[0].0);
        let left: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left.len(), 1, "{left:?}");
    }

    /// A folder that can't make hard links (El on #3054: many shares) is
    /// refused with why, the part removed and no record half there; the
    /// station's own `--out` copy is made first and kept (`main`).
    #[test]
    fn a_folder_that_cannot_link_refuses_with_why_and_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.json");
        let part = write_part(dir.path(), "run", b"a record\n").unwrap();
        let error = place(&part, &path, |_, _| {
            Err(io::Error::from(io::ErrorKind::Unsupported))
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        assert!(error.to_string().contains("hard link"), "{error}");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    /// A lamp's folder that is a link elsewhere is refused, not followed.
    #[cfg(unix)]
    #[test]
    fn a_lamp_folder_that_is_a_link_is_refused() {
        let place = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), place.path().join("lamp-4ace")).unwrap();
        let error = keep_for_lamp(place.path(), &record()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{error}");
        assert_eq!(fs::read_dir(elsewhere.path()).unwrap().count(), 0);
    }

    /// #2794's done-when: two runs on one lamp give two records side by
    /// side in its folder, both read back by the lamp's name, and the one
    /// with a check left out reads as not complete, so never as passed.
    #[test]
    fn two_runs_on_one_lamp_are_kept_side_by_side_and_read_back_by_its_name() {
        use crate::complete::{
            self,
            tests::{matrix, passed},
        };
        let place = tempfile::tempdir().unwrap();
        let matrix = matrix();
        let first = passed(&matrix);
        let mut second = passed(&matrix);
        second.run_id = "20261019T090000Z-station-01-lamp-4ace".into();
        second.started = "2026-10-19T09:00:00Z".parse().unwrap();
        second.ended = "2026-10-19T09:30:00Z".parse().unwrap();
        let left_out = second.checks.remove(0).id;
        let mut elsewhere = passed(&matrix);
        elsewhere.run_id = "20261019T080000Z-station-01-lamp-52e6".into();
        elsewhere.device.id = "lamp-52e6".into();
        for record in [&first, &second, &elsewhere] {
            keep_for_lamp(place.path(), record).unwrap();
        }
        assert_eq!(
            keep_for_lamp(place.path(), &first).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        let found = lamp_records(place.path(), "lamp-4ace").unwrap();
        let paths: Vec<_> = found.iter().map(|(path, _)| path.clone()).collect();
        assert_eq!(
            paths,
            [
                place
                    .path()
                    .join("lamp-4ace/20261019T080000Z-station-01-lamp-4ace.json"),
                place
                    .path()
                    .join("lamp-4ace/20261019T090000Z-station-01-lamp-4ace.json"),
            ]
        );
        let read: Vec<Record> = found.into_iter().map(|(_, r)| r.unwrap()).collect();
        assert_eq!(read, [first, second]);
        assert!(complete::check(&matrix, &read[0]).complete());
        let judged = complete::check(&matrix, &read[1]);
        assert!(!judged.complete());
        assert!(
            judged
                .gaps
                .iter()
                .any(|gap| gap.to_string().contains(&left_out)),
            "{:?}",
            judged.gaps
        );
        assert!(lamp_records(place.path(), "lamp-0c89").unwrap().is_empty());
    }

    /// A lamp's folder holds only its own runs: a record of another lamp,
    /// or one under another run's name, is listed with why, never as the
    /// lamp's; and a lamp id that would not be a folder of its own is
    /// refused.
    #[test]
    fn a_lamps_folder_lists_what_is_not_its_record_with_why() {
        let place = tempfile::tempdir().unwrap();
        let dir = place.path().join("lamp-4ace");
        fs::create_dir_all(&dir).unwrap();
        let mut other = record();
        other.device.id = "lamp-52e6".into();
        fs::write(
            dir.join(format!("{}.json", other.run_id)),
            serde_json::to_vec(&other).unwrap(),
        )
        .unwrap();
        fs::write(
            dir.join("renamed.json"),
            serde_json::to_vec(&record()).unwrap(),
        )
        .unwrap();
        fs::write(dir.join("half.json"), b"{").unwrap();
        fs::write(dir.join("notes.txt"), b"not a record").unwrap();
        let found = lamp_records(place.path(), "lamp-4ace").unwrap();
        assert_eq!(found.len(), 3);
        for (path, read) in &found {
            assert!(read.is_err(), "{}", path.display());
        }
        for lamp in ["..", ".", "", "a/b", ".hidden", "Lamp-4ACE"] {
            assert_eq!(
                lamp_dir(place.path(), lamp).unwrap_err().kind(),
                io::ErrorKind::InvalidInput,
                "{lamp:?}"
            );
        }
        let mut dotted = record();
        dotted.device.id = "..".into();
        assert!(keep_for_lamp(place.path(), &dotted).is_err());
        assert!(
            !place
                .path()
                .join("..")
                .join(format!("{}.json", dotted.run_id))
                .exists()
        );
    }

    /// A record that appears between the check and the write is kept: the
    /// new one is refused and its part is gone, never renamed over it.
    #[test]
    fn a_record_that_appears_meanwhile_is_not_written_over() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.json");
        let part = dir.path().join("run.part");
        fs::write(&path, b"the first record\n").unwrap();
        fs::write(&part, b"the second record\n").unwrap();
        let error = place(&part, &path, |a, b| fs::hard_link(a, b)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&path).unwrap(), b"the first record\n");
        assert!(!part.exists());
        // With nothing there, the part is put in place and removed.
        fs::remove_file(&path).unwrap();
        fs::write(&part, b"the second record\n").unwrap();
        place(&part, &path, |a, b| fs::hard_link(a, b)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"the second record\n");
        assert!(!part.exists());
    }
}
