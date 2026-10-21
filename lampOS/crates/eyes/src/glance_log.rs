//! `lamp-eyes --look-on-change on --glance-log /var/lib/lampos-eyes/glance.log`
//! ([`PATH`], the one place a reset wipes): one JSON line per
//! compared glance, so the change bar's rules can be replayed offline
//! (`examples/glance_replay.rs`). Only counts and per-cell sizes of change
//! are written: which cells passed the bar, each cell's change over its
//! learned noise, and each cell's change in greys after its row's median
//! change is taken out. No cell's grey, no pixel and no picture is written.
//! Off unless given, and a test flag no deployed unit passes; the file is
//! new each run and stops at [`MOST_US`] or [`MOST_BYTES`], whichever
//! comes first.
use crate::change::Glance;
use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};

/// The one path the log is kept at: beside `seems.log`, in the folder whose
/// files a reset's plan names, so a reset wipes it.
pub const PATH: &str = "/var/lib/lampos-eyes/glance.log";
/// The log stops 20 minutes after its first line.
pub const MOST_US: u64 = 20 * 60 * 1_000_000;
/// Or at 50 MB (a line is about 3.4 KB: some 15,000 glances).
pub const MOST_BYTES: u64 = 50 * 1_000_000;

/// Lowercase hex of `bytes`.
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(DIGITS[usize::from(byte >> 4)] as char);
        text.push(DIGITS[usize::from(byte & 15)] as char);
    }
    text
}

/// One glance as a line: `at_us`, `changed`, `lit`, `cells`, `mask`,
/// `ratio`, `resid_greys_q` (the three per cell, in hex), `frame_shift`,
/// `detect`, `why` and `faces` (how many faces the detector found that
/// `--leave-still-faces` kept, so the faces the body was told of; 0 when
/// the detector did not run).
pub fn line(glance: &Glance, faces: usize) -> String {
    #[derive(serde::Serialize)]
    struct Line<'a> {
        at_us: u64,
        changed: bool,
        lit: bool,
        cells: usize,
        mask: String,
        ratio: String,
        resid_greys_q: String,
        frame_shift: f32,
        detect: bool,
        why: &'a str,
        faces: usize,
    }
    serde_json::to_string(&Line {
        at_us: glance.at_us,
        changed: glance.changed,
        lit: glance.lit,
        cells: glance.cells,
        mask: hex(&glance.mask),
        ratio: hex(&glance.ratio),
        resid_greys_q: hex(&glance.resid_greys_q),
        frame_shift: (glance.frame_shift * 10.0).round() / 10.0,
        detect: glance.detect,
        why: glance.why,
        faces,
    })
    .unwrap_or_default()
}

/// The log, open for appending.
pub struct Log {
    file: File,
    bytes: u64,
    first_us: Option<u64>,
    full_said: bool,
    failed_said: bool,
}

impl Log {
    /// Makes `path` new (0600): a log left by an earlier start is removed
    /// first (a link is removed, not followed), then the file is made with
    /// `O_EXCL` and `O_NOFOLLOW`. Its folder must be a folder no one else
    /// may read (0700).
    pub fn open(path: &Path) -> io::Result<Self> {
        let folder = path
            .parent()
            .ok_or_else(|| io::Error::other("the glance log has no folder"))?;
        let meta = std::fs::symlink_metadata(folder)?;
        if !meta.file_type().is_dir() || meta.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::other(
                "the glance log's folder must be a folder of its owner's only (0700)",
            ));
        }
        match std::fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        let file = OpenOptions::new()
            .create_new(true)
            .append(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        Ok(Self {
            file,
            bytes: 0,
            first_us: None,
            full_said: false,
            failed_said: false,
        })
    }

    /// Writes one glance, unless the log is full. What the journal says of
    /// it: a full log, or the first failed write, each once.
    pub fn write(&mut self, glance: &Glance, faces: usize) -> Option<String> {
        let first_us = *self.first_us.get_or_insert(glance.at_us);
        let text = line(glance, faces) + "\n";
        if glance.at_us.saturating_sub(first_us) >= MOST_US
            || self.bytes + text.len() as u64 > MOST_BYTES
        {
            return (!std::mem::replace(&mut self.full_said, true)).then(|| {
                "the glance log is full (20 min or 50 MB); no more glances are written to it"
                    .to_owned()
            });
        }
        match self.file.write_all(text.as_bytes()) {
            Ok(()) => {
                self.bytes += text.len() as u64;
                None
            }
            Err(error) => (!std::mem::replace(&mut self.failed_said, true)).then(|| {
                format!(
                    "a glance wasn't written to the glance log ({error}); a later failure isn't said"
                )
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change::{ACROSS, Change, DOWN, Grid};

    /// A grid whose cells are a known pattern of greys, all different
    /// from their neighbours.
    fn pattern(add: f32) -> Grid {
        Grid::from_greys(
            (0..ACROSS * DOWN)
                .map(|cell| 20.0 + ((cell * 37) % 200) as f32 + add)
                .collect(),
        )
    }

    #[test]
    fn a_line_per_compared_glance_with_no_grey_in_it() {
        let folder = std::env::temp_dir().join(format!("glance-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir(&folder).unwrap();
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = folder.join("glance.log");
        let mut log = Log::open(&path).unwrap();
        let mut change = Change::default();
        change.keep_glances();
        // The first glance has nothing to compare: no line.
        let frames = [pattern(0.0), pattern(0.0), pattern(30.0), pattern(1.0)];
        let mut at = 0;
        for frame in frames {
            at += 600_000;
            let detect = change.detect(frame, None, at);
            if let Some(glance) = change.take_glance() {
                assert_eq!(glance.detect, detect);
                assert!(log.write(&glance, usize::from(detect)).is_none());
            }
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_dir_all(&folder);
        let lines: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 3);
        let greys: Vec<u8> = (0..ACROSS * DOWN)
            .map(|cell| (20 + (cell * 37) % 200) as u8)
            .collect();
        for line in &lines {
            let mut fields: Vec<&str> = line
                .as_object()
                .unwrap()
                .keys()
                .map(|key| key.as_str())
                .collect();
            fields.sort_unstable();
            assert_eq!(
                fields,
                [
                    "at_us",
                    "cells",
                    "changed",
                    "detect",
                    "faces",
                    "frame_shift",
                    "lit",
                    "mask",
                    "ratio",
                    "resid_greys_q",
                    "why"
                ]
            );
            assert_eq!(line["mask"].as_str().unwrap().len(), ACROSS * DOWN / 4);
            assert_eq!(line["ratio"].as_str().unwrap().len(), ACROSS * DOWN * 2);
            assert_eq!(
                line["resid_greys_q"].as_str().unwrap().len(),
                ACROSS * DOWN * 2
            );
        }
        // The pattern's greys, as any run of eight cells in hex, appear in
        // no line: the frames' whole shift is taken out, not written.
        for (n, window) in greys.windows(8).enumerate().step_by(7) {
            for shifted in [0u8, 1, 30] {
                let run: Vec<u8> = window
                    .iter()
                    .map(|grey| grey.wrapping_add(shifted))
                    .collect();
                assert!(!text.contains(&hex(&run)), "cells {n}.. appear in the log");
            }
        }
        // The whole frame 30 greys brighter is the camera's exposure: no
        // cell changed, and the shift is said.
        assert_eq!(lines[1]["frame_shift"], 30.0);
        assert_eq!(lines[1]["cells"], 0);
        assert!(
            lines[1]["resid_greys_q"]
                .as_str()
                .unwrap()
                .bytes()
                .all(|b| b == b'0')
        );
    }

    #[test]
    fn the_log_stops_at_its_cap() {
        let folder = std::env::temp_dir().join(format!("glance-cap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir(&folder).unwrap();
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = folder.join("glance.log");
        let mut log = Log::open(&path).unwrap();
        let mut change = Change::default();
        change.keep_glances();
        change.detect(pattern(0.0), None, 0);
        change.detect(pattern(0.0), None, 600_000);
        let mut glance = change.take_glance().unwrap();
        assert!(log.write(&glance, 0).is_none());
        glance.at_us += MOST_US;
        assert!(log.write(&glance, 0).is_some());
        assert!(log.write(&glance, 0).is_none(), "said once");
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_dir_all(&folder);
        assert_eq!(text.lines().count(), 1);
    }

    #[test]
    fn an_open_folder_is_refused() {
        let folder = std::env::temp_dir().join(format!("glance-open-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir(&folder).unwrap();
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
        let refused = Log::open(&folder.join("glance.log")).is_err();
        let _ = std::fs::remove_dir_all(&folder);
        assert!(refused);
    }

    #[test]
    fn each_start_makes_the_log_new() {
        let folder = std::env::temp_dir().join(format!("glance-new-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir(&folder).unwrap();
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (path, other) = (folder.join("glance.log"), folder.join("other"));
        std::fs::write(&path, "an earlier start's line\n").unwrap();
        std::fs::write(&other, "kept").unwrap();
        drop(Log::open(&path).unwrap());
        let earlier = std::fs::read_to_string(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        // A link left at the path is removed, not followed.
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&other, &path).unwrap();
        drop(Log::open(&path).unwrap());
        let linked = std::fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink();
        let kept = std::fs::read_to_string(&other).unwrap();
        let _ = std::fs::remove_dir_all(&folder);
        assert_eq!((earlier.as_str(), mode), ("", 0o600));
        assert!(!linked);
        assert_eq!(kept, "kept");
    }
}
