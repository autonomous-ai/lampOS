//! A production test station's record of one run (#1700, deliverable 4).
//! See `docs/production-testing.md`.
//!
//! - [`matrix`]: the versioned coverage matrix, `production/matrix-v<N>.toml`:
//!   each check's id, part, kind, method, whether it is mandatory, and the
//!   hardware revisions on which `not_fitted` is fine.
//! - [`inputs`]: what the station gives: the lamp's self-test report, read
//!   strictly, and the station's answers for its own steps.
//! - [`record`]: one run's record and its verdict, from the matrix and the
//!   inputs, by the rules in the doc.
//! - [`store`]: the record written whole and then renamed, never over
//!   another.
//!
//! Nothing here opens a device or a socket; the station tool reads files and
//! writes one.

// Nothing in this library prints; the program prints what it is given.
#![deny(clippy::print_stderr, clippy::print_stdout, clippy::dbg_macro)]

use jiff::Timestamp;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, str::FromStr};

pub mod inputs;
pub mod matrix;
pub mod record;
pub mod store;

/// Why an input was refused. Nothing is recorded for a refused run: the
/// station fixes its input and makes the record again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused(pub String);

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Refused {}

/// Refuses with a formatted reason.
macro_rules! refuse {
    ($($arg:tt)*) => {
        return Err($crate::Refused(format!($($arg)*)))
    };
}
pub(crate) use refuse;

/// A moment in UTC, written as RFC 3339 ending in `Z`
/// (`2026-10-19T08:00:00Z`). A time with any other offset is refused, so a
/// station's local clock never slips into a record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Utc(pub Timestamp);

impl Utc {
    /// From seconds since the Unix epoch, as the self-test gives its start.
    pub fn from_unix_seconds(seconds: f64) -> Result<Self, Refused> {
        if !(seconds.is_finite() && seconds >= 0.0) {
            refuse!("{seconds} is not a time");
        }
        Timestamp::from_microsecond((seconds * 1e6).round() as i64)
            .map(Self)
            .map_err(|error| Refused(format!("{seconds} is not a time: {error}")))
    }

    /// Milliseconds since the Unix epoch.
    pub fn millis(self) -> i64 {
        self.0.as_millisecond()
    }

    /// `20261019T080000Z`, for a run's id.
    pub fn compact(self) -> String {
        self.0.strftime("%Y%m%dT%H%M%SZ").to_string()
    }
}

impl fmt::Display for Utc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl FromStr for Utc {
    type Err = Refused;

    fn from_str(text: &str) -> Result<Self, Refused> {
        if !text.ends_with('Z') {
            refuse!("{text:?} is not a UTC time: write it as 2026-10-19T08:00:00Z");
        }
        text.parse::<Timestamp>()
            .map(Self)
            .map_err(|error| Refused(format!("{text:?} is not a UTC time: {error}")))
    }
}

impl Serialize for Utc {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Utc {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// How far the lamp's clock and the station's may disagree, in
/// milliseconds: the self-test's start is the lamp's own clock, and a lamp
/// has no battery clock (install.md, "The clock"). A proposal, like the
/// matrix's limits.
pub const CLOCK_SLACK_MS: i64 = 5 * 60 * 1000;

/// A plain name of 1 to `most` bytes: ASCII letters, digits, `-`, `_`, `.`.
pub(crate) fn plain(what: &str, text: &str, most: usize) -> Result<(), Refused> {
    if text.is_empty()
        || text.len() > most
        || !text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        refuse!(
            "{what} {text:?} is not a plain name of 1 to {most} letters, digits, '-', '_' or '.'"
        );
    }
    Ok(())
}

/// A line a person wrote or a part said: 1 to `most` bytes, no control
/// character.
pub(crate) fn line(what: &str, text: &str, most: usize) -> Result<(), Refused> {
    if text.trim().is_empty() || text.len() > most || text.chars().any(char::is_control) {
        refuse!("{what} must be one line of 1 to {most} bytes");
    }
    Ok(())
}

/// A git commit: 7 to 40 lower-case hexadecimal digits.
pub(crate) fn commit(what: &str, text: &str) -> Result<(), Refused> {
    if !(7..=40).contains(&text.len())
        || !text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        refuse!("{what} {text:?} is not a commit (7 to 40 lower-case hex digits)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_is_utc_or_refused() {
        let at: Utc = "2026-10-19T08:00:00Z".parse().unwrap();
        assert_eq!(at.to_string(), "2026-10-19T08:00:00Z");
        assert_eq!(at.compact(), "20261019T080000Z");
        for refused in [
            "2026-10-19T15:00:00+07:00",
            "2026-10-19T08:00:00",
            "2026-10-19",
            "yesterday",
            "2026-10-19T08:00:00z",
        ] {
            assert!(refused.parse::<Utc>().is_err(), "{refused}");
        }
        let json = serde_json::to_string(&at).unwrap();
        assert_eq!(json, "\"2026-10-19T08:00:00Z\"");
        assert_eq!(serde_json::from_str::<Utc>(&json).unwrap(), at);
        assert!(serde_json::from_str::<Utc>("\"2026-10-19T08:00:00+00:00\"").is_err());
    }

    #[test]
    fn the_self_tests_start_becomes_a_utc_time() {
        let at = Utc::from_unix_seconds(1_792_086_643.000421).unwrap();
        assert_eq!(at.to_string(), "2026-10-15T17:50:43.000421Z");
        assert!(Utc::from_unix_seconds(f64::NAN).is_err());
        assert!(Utc::from_unix_seconds(-1.0).is_err());
    }

    #[test]
    fn names_lines_and_commits_are_checked() {
        assert!(plain("device", "lamp-4ace", 64).is_ok());
        for refused in ["", "lamp 4ace", "lamp/4ace", "lämp"] {
            assert!(plain("device", refused, 64).is_err(), "{refused:?}");
        }
        assert!(plain("device", &"a".repeat(65), 64).is_err());
        assert!(line("reason", "pixel 12 stays dark", 200).is_ok());
        for refused in ["", "  ", "two\nlines"] {
            assert!(line("reason", refused, 200).is_err(), "{refused:?}");
        }
        assert!(line("reason", &"a".repeat(201), 200).is_err());
        assert!(commit("build", "dab3f12").is_ok());
        assert!(commit("build", &"d".repeat(40)).is_ok());
        for refused in ["dab3f1", "DAB3F12", "dab3f1g", &"d".repeat(41)] {
            assert!(commit("build", refused).is_err(), "{refused:?}");
        }
    }
}
