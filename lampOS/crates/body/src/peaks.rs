//! Each joint's highest load and temperature in each local day (#4501,
//! `docs/body.md`, "The strain guard"), kept in the body's state folder as
//! `peaks/YYYY-MM-DD.json` for [`KEEP_DAYS`] days, so a servo wearing out
//! is seen over days, not only in the journal's last hours. Written whole at
//! most every [`WRITE_EVERY`], and only when a peak rose. A failure to keep
//! them is said once and never stops the body.

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// Where the product body keeps them (`StateDirectory=lampos-body`).
pub const DIR: &str = "/var/lib/lampos-body/peaks";
pub const WRITE_EVERY: Duration = Duration::from_secs(5 * 60);
/// Today and the days before it, this many in all.
pub const KEEP_DAYS: i64 = 30;

/// One joint's highest readings in the day.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Peak {
    pub load: u16,
    pub temperature: u8,
}

/// One day's file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Day {
    pub day: String,
    pub joints: BTreeMap<String, Peak>,
}

pub struct Peaks {
    dir: PathBuf,
    days: i64,
    kept: Day,
    rose: bool,
    looked: Option<Instant>,
    failed: bool,
}

impl Peaks {
    /// The day `days` (since 1970-01-01, local): its peaks so far, from its
    /// file if the body kept one before it was started again.
    pub fn open(dir: impl Into<PathBuf>, days: i64) -> Self {
        let dir = dir.into();
        let kept = read(&dir, days);
        Self {
            dir,
            days,
            kept,
            rose: false,
            looked: None,
            failed: false,
        }
    }

    /// One step's readings: each joint's name, load and temperature.
    pub fn observe<'a>(&mut self, readings: impl IntoIterator<Item = (&'a str, u16, u8)>) {
        for (joint, load, temperature) in readings {
            let peak = self.kept.joints.entry(joint.to_owned()).or_default();
            if load > peak.load || temperature > peak.temperature {
                peak.load = peak.load.max(load);
                peak.temperature = peak.temperature.max(temperature);
                self.rose = true;
            }
        }
    }

    /// At most every [`WRITE_EVERY`]: keeps what rose, and on a new day
    /// (`today`, asked only then) closes the last one, starts afresh and
    /// lets files past [`KEEP_DAYS`] go. A failure is said once.
    // ponytail: readings up to WRITE_EVERY past midnight go to the day before.
    pub fn tick(&mut self, now: Instant, today: impl FnOnce() -> i64) {
        if self
            .looked
            .is_some_and(|at| now.duration_since(at) < WRITE_EVERY)
        {
            return;
        }
        self.looked = Some(now);
        let today = today();
        let mut result = Ok(());
        if today != self.days {
            if self.rose {
                result = self.write();
            }
            self.days = today;
            self.kept = read(&self.dir, today);
            self.rose = false;
            result = result.and(prune(&self.dir, today));
        }
        if self.rose {
            result = result.and(self.write());
            self.rose = false;
        }
        if let Err(error) = result
            && !self.failed
        {
            self.failed = true;
            eprintln!(
                "lamp-body: the day's peaks could not be kept in {}: {error}; the body goes on, and says this once",
                self.dir.display()
            );
        }
    }

    fn write(&self) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        crate::first_stand::write_json(&path(&self.dir, self.days), &self.kept)
    }
}

fn path(dir: &Path, days: i64) -> PathBuf {
    dir.join(format!("{}.json", date(days)))
}

/// The day's file, or an empty day.
fn read(dir: &Path, days: i64) -> Day {
    let day = date(days);
    fs::read(path(dir, days))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Day>(&bytes).ok())
        .filter(|kept| kept.day == day)
        .unwrap_or(Day {
            day,
            joints: BTreeMap::new(),
        })
}

/// Every `YYYY-MM-DD.json` older than the last [`KEEP_DAYS`] days removed.
fn prune(dir: &Path, today: i64) -> io::Result<()> {
    let oldest = date(today - KEEP_DAYS + 1);
    for entry in fs::read_dir(dir)? {
        let name = entry?.file_name();
        let Some(day) = name.to_str().and_then(|name| name.strip_suffix(".json")) else {
            continue;
        };
        if day.len() == 10 && day.starts_with(|c: char| c.is_ascii_digit()) && *day < *oldest {
            fs::remove_file(dir.join(&name))?;
        }
    }
    Ok(())
}

/// `days` since 1970-01-01 as `YYYY-MM-DD` (the proleptic Gregorian
/// calendar, H. Hinnant's `civil_from_days`).
pub fn date(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_are_named_as_the_calendar_has_them() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(19_782), "2024-02-29");
        assert_eq!(date(20_756), "2026-10-30");
    }

    #[test]
    fn peaks_are_kept_per_day_only_when_they_rise_and_for_thirty_days() {
        let dir = std::env::temp_dir().join(format!("lamp-body-peaks-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let old = path(&dir, 20_756 - 30);
        let kept = path(&dir, 20_756 - 29);
        fs::write(&old, "{}").unwrap();
        fs::write(&kept, "{}").unwrap();
        fs::write(dir.join("notes.json"), "{}").unwrap();
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        let today = path(&dir, 20_756);
        let read_today = || serde_json::from_slice::<Day>(&fs::read(&today).unwrap()).unwrap();

        let mut peaks = Peaks::open(&dir, 20_756);
        peaks.observe([("elbow", 210, 44), ("shoulder", 150, 41)]);
        peaks.tick(at(0), || 20_756);
        assert_eq!(
            read_today().joints["elbow"],
            Peak {
                load: 210,
                temperature: 44
            }
        );
        // A higher reading within five minutes waits for the next write.
        peaks.observe([("elbow", 580, 45)]);
        peaks.tick(at(60), || 20_756);
        assert_eq!(read_today().joints["elbow"].load, 210);
        peaks.tick(at(300), || 20_756);
        assert_eq!(
            read_today().joints["elbow"],
            Peak {
                load: 580,
                temperature: 45
            }
        );
        // Nothing rose: nothing written.
        fs::remove_file(&today).unwrap();
        peaks.observe([("elbow", 100, 40)]);
        peaks.tick(at(600), || 20_756);
        assert!(!today.exists());

        // Started again the same day: it goes on from what it kept.
        peaks.observe([("elbow", 590, 45)]);
        peaks.tick(at(900), || 20_756);
        let mut again = Peaks::open(&dir, 20_756);
        again.observe([("elbow", 300, 50)]);
        again.tick(at(0), || 20_756);
        assert_eq!(
            read_today().joints["elbow"],
            Peak {
                load: 590,
                temperature: 50
            }
        );

        // The next day starts afresh, and the day 30 days back goes.
        again.tick(at(300), || 20_757);
        again.observe([("elbow", 120, 40)]);
        again.tick(at(600), || 20_757);
        let tomorrow = path(&dir, 20_757);
        assert_eq!(
            serde_json::from_slice::<Day>(&fs::read(&tomorrow).unwrap())
                .unwrap()
                .joints["elbow"],
            Peak {
                load: 120,
                temperature: 40
            }
        );
        assert!(!old.exists() && !kept.exists(), "both now past 30 days");
        assert!(dir.join("notes.json").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
