//! A board made of files, for the tests: a `/proc`, a `/sys` and a `/run`
//! in a directory of the test's own, with what the real board has in them.
// Each test file uses its own part of this.
#![allow(dead_code)]

use lamp_health::board::Places;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// The real board's eight processors, as `/proc/stat` begins.
pub const STAT: &str = "cpu  812344 1203 402211 97012345 5521 0 9012 0 0 0\n\
cpu0 101543 150 50276 12126543 690 0 1126 0 0 0\n\
cpu1 101543 150 50276 12126543 690 0 1126 0 0 0\n\
cpu2 101543 150 50276 12126543 690 0 1126 0 0 0\n\
cpu3 101543 150 50276 12126543 690 0 1126 0 0 0\n\
cpu4 101543 150 50276 12126543 690 0 1126 0 0 0\n\
cpu5 101543 150 50276 12126543 690 0 1126 0 0 0\n\
cpu6 101543 150 50276 12126543 690 0 1126 0 0 0\n\
cpu7 101543 150 50276 12126543 690 0 1126 0 0 0\n\
intr 123456789 0 0 0 0 0 0 0 0 0 0 0 0 0\n\
ctxt 987654321\n\
btime 1791878400\n\
processes 54321\n\
procs_running 3\n\
procs_blocked 0\n";

/// Memory as `/proc/meminfo` begins: 44% of it in use.
pub const MEMINFO: &str = "MemTotal:       16000000 kB\n\
MemFree:         1234567 kB\n\
MemAvailable:    8960000 kB\n\
Buffers:          123456 kB\n\
Cached:          4567890 kB\n\
SwapCached:            0 kB\n";

/// The real board's thermal zones, by number: each one's type and what it
/// reads when the lamp is idle and a little under its first trip point.
/// The two processor zones name their trip points; the tests give the
/// others none.
pub const ZONES: [(&str, i64); 8] = [
    ("cpul_thermal_zone", 63_500),
    ("cpub_thermal_zone", 64_250),
    ("cpul_idle_zone", 63_000),
    ("cpub_idle_zone", 64_000),
    ("gpu_thermal_zone", 63_250),
    ("npu_thermal_zone", 63_750),
    ("ddr_thermal_zone", 64_500),
    ("skin_zone", 37_000),
];

/// The processor zones' trip points on the real board.
pub const CPU_TRIPS: [(i64, &str); 3] = [
    (70_000, "passive"),
    (90_000, "passive"),
    (110_000, "critical"),
];

/// A directory of the test's own, removed when the test ends.
pub struct Tree {
    pub dir: PathBuf,
}

impl Tree {
    /// An empty board: the three directories and nothing in them.
    pub fn empty(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("lamp-health-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for part in ["proc", "sys", "run", "disk"] {
            fs::create_dir_all(dir.join(part)).unwrap();
        }
        Self { dir }
    }

    /// The real board, idle: eight processors with a load of 2.48 between
    /// them, 44% of the memory in use, up for two days and ten hours,
    /// eight thermal zones, no Wi-Fi file and no timesync directory.
    pub fn real_board(name: &str) -> Self {
        let tree = Self::empty(name);
        tree.file("proc/loadavg", "2.48 2.51 2.60 3/1234 56789\n");
        tree.file("proc/stat", STAT);
        tree.file("proc/meminfo", MEMINFO);
        tree.file("proc/uptime", "208800.52 1650000.11\n");
        for (number, (kind, milli_c)) in ZONES.into_iter().enumerate() {
            let trips: &[(i64, &str)] = if kind.starts_with("cpu") && kind.ends_with("thermal_zone")
            {
                &CPU_TRIPS
            } else {
                &[]
            };
            tree.zone(number, kind, milli_c, trips);
        }
        tree
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.dir.join(relative)
    }

    /// Write a file, making the directories above it.
    pub fn file(&self, relative: &str, text: &str) {
        let path = self.path(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    pub fn remove(&self, relative: &str) {
        let path = self.path(relative);
        if path.is_dir() {
            fs::remove_dir_all(path).unwrap();
        } else {
            fs::remove_file(path).unwrap();
        }
    }

    /// A thermal zone as the system shows it: its type, what it reads in
    /// thousandths of a degree, and its trip points from 0.
    pub fn zone(&self, number: usize, kind: &str, milli_c: i64, trips: &[(i64, &str)]) {
        let zone = format!("sys/class/thermal/thermal_zone{number}");
        self.file(&format!("{zone}/type"), &format!("{kind}\n"));
        self.file(&format!("{zone}/temp"), &format!("{milli_c}\n"));
        for (point, (at, kind)) in trips.iter().enumerate() {
            self.file(
                &format!("{zone}/trip_point_{point}_temp"),
                &format!("{at}\n"),
            );
            self.file(
                &format!("{zone}/trip_point_{point}_type"),
                &format!("{kind}\n"),
            );
        }
    }

    /// What one zone reads now.
    pub fn heat(&self, number: usize, milli_c: i64) {
        self.file(
            &format!("sys/class/thermal/thermal_zone{number}/temp"),
            &format!("{milli_c}\n"),
        );
    }

    pub fn places(&self) -> Places {
        Places {
            proc: self.path("proc"),
            sys: self.path("sys"),
            run: self.path("run"),
            disk: self.path("disk"),
            judged: Vec::new(),
            clock: lamp_health::board::ClockSource::Timesync,
        }
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// How much of the disk under `path` is free, asked the way the program
/// asks. The tests cannot make this number up: it is the machine's own.
pub fn disk_free(path: &Path) -> f32 {
    let held = rustix::fs::statvfs(path).unwrap();
    (held.f_bavail as f64 / held.f_blocks as f64) as f32
}
