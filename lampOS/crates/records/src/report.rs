//! The live reporter describes the last cleanup, not a new scan every second.
//!
//! Cleanup writes its bounded result separately. A persistent caller reads that
//! result and calls this module once a second: a one-shot diagnostics publisher
//! would become stale in lamp-health after five seconds. Neither this mapping
//! nor publication scans records or takes ownership of an active writer.

use crate::{Disk, REPORT_SCHEMA_VERSION as SCHEMA_VERSION, RunReport};
use lamp_bus::{Bus, Publisher};
use lamp_msgs::{Diagnostic, Level};
use std::{io, path::Path, time::Instant};

/// An hourly cleanup has missed a complete additional interval after this age.
pub const STALE_AFTER_SECONDS: u64 = 2 * 60 * 60;
const NAME: &str = "records";
/// How the guard begins a refusal for a worker role the voice's list does
/// not have, followed by the role in backticks (#1071).
pub const UNKNOWN_WORKER: &str = "unknown voice worker";
/// Cleanups in a row refused for an unknown voice worker before the
/// diagnostic names it: one can be a worker caught as it starts, two is the
/// voice running a role the guard does not know.
pub const NAMED_AFTER: u32 = 2;

/// A word the guard may repeat from a process's arguments: lower-case
/// letters, digits and dashes, at most 32 of them, as the voice's roles are.
pub fn plain_word(word: &str) -> bool {
    (1..=32).contains(&word.len())
        && word
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// The unknown worker role a root of this cleanup was refused for, if any.
pub fn unknown_worker(report: &RunReport) -> Option<&str> {
    let marker = format!("{UNKNOWN_WORKER} `");
    report
        .roots
        .iter()
        .filter_map(|root| root.error.as_deref())
        .find_map(|error| {
            let (_, rest) = error.split_once(&marker)?;
            let (word, _) = rest.split_once('`')?;
            plain_word(word).then_some(word)
        })
}

/// Counts the cleanups in a row, each told apart by its time, that were
/// refused for an unknown voice worker. The reporter keeps one; a cleanup
/// that was not refused for that sets it back to none.
#[derive(Debug, Default)]
pub struct RefusedInARow {
    last_at: Option<u64>,
    count: u32,
}

impl RefusedInARow {
    pub fn observe(&mut self, report: &RunReport) -> u32 {
        if self.last_at != Some(report.at_unix_s) {
            self.last_at = Some(report.at_unix_s);
            self.count = if unknown_worker(report).is_some() {
                self.count.saturating_add(1)
            } else {
                0
            };
        }
        self.count
    }
}

#[derive(Default)]
struct Counts {
    active: u128,
    planned: u128,
    removed: u128,
    refused: u128,
    before_bytes: u128,
    planned_bytes: u128,
    removed_bytes: u128,
    remaining_bytes: u128,
    limit_bytes: u128,
    failed_roots: usize,
    over_limit_roots: usize,
    unknown_disk: bool,
}

fn valid_disk(disk: &Disk) -> bool {
    disk.total_bytes > 0 && disk.available_bytes <= disk.total_bytes
}

/// Integer comparison preserves the exact 3% and 10% boundaries, including
/// filesystems whose byte counts are too large for exact floating-point values.
fn below_percent(disk: &Disk, percent: u128) -> bool {
    u128::from(disk.available_bytes) * 100 < u128::from(disk.total_bytes) * percent
}

fn free_percent(disk: &Disk) -> String {
    // Round down: a value just below a warning boundary must not display as
    // having reached it. This is a percentage, not a fraction or fresh reading.
    let hundredths = u128::from(disk.available_bytes) * 10_000 / u128::from(disk.total_bytes);
    format!("{}.{:02}", hundredths / 100, hundredths % 100)
}

/// A content-free summary with fixed-size output, independent of record names,
/// paths, error text and the number of individual records in the stored report.
///
/// A disabled cleanup need not inspect roots or disk space. Invalid report
/// identity/time and a reported top-level failure are still errors. Disk values
/// are explicitly from the cleanup, with the report's age alongside them.
pub fn diagnostic(report: Option<&RunReport>, now_unix_s: u64) -> Diagnostic {
    diagnostic_after(report, now_unix_s, 0)
}

/// [`diagnostic`], told how many cleanups in a row were refused for an
/// unknown voice worker ([`RefusedInARow`]). From [`NAMED_AFTER`] on, a
/// failed cleanup says which worker: `records refused: unknown voice worker
/// `X``, the one detail of a refusal it repeats, and only a plain word.
pub fn diagnostic_after(
    report: Option<&RunReport>,
    now_unix_s: u64,
    refused_in_a_row: u32,
) -> Diagnostic {
    let Some(report) = report else {
        return Diagnostic::new(Level::Error, NAME, "no cleanup report is available")
            .with("report_age_s", "unknown")
            .with("disk_free_at_cleanup_pct", "unknown");
    };
    if report.validate().is_err() {
        // Public DTOs can also be constructed directly. Never aggregate or
        // publish fields from a syntactically invalid or oversized report.
        return Diagnostic::new(Level::Error, NAME, "the cleanup report is invalid")
            .with("mode", "invalid")
            .with("report_age_s", "unknown")
            .with("disk_free_at_cleanup_pct", "unknown");
    }
    let age = now_unix_s.checked_sub(report.at_unix_s);
    let stale = age.is_some_and(|age| age > STALE_AFTER_SECONDS);
    let mode = match report.mode.as_str() {
        "apply" => "apply",
        "dry_run" => "dry_run",
        _ => "invalid",
    };
    let mut counts = Counts::default();
    let mut worst_disk: Option<&Disk> = None;
    for root in &report.roots {
        counts.active += root.active.len() as u128;
        counts.planned += root.planned.len() as u128;
        counts.removed += root.removed.len() as u128;
        counts.refused += root.refused.len() as u128;
        counts.before_bytes += u128::from(root.before_bytes);
        counts.planned_bytes += u128::from(root.planned_bytes);
        counts.removed_bytes += u128::from(root.removed_bytes);
        counts.remaining_bytes += u128::from(root.remaining_bytes);
        counts.limit_bytes += u128::from(root.limit_bytes);
        counts.failed_roots += usize::from(root.error.is_some());
        counts.over_limit_roots += usize::from(root.remaining_bytes > root.limit_bytes);
        match root.disk.as_ref().filter(|disk| valid_disk(disk)) {
            Some(disk) => {
                let is_worse = worst_disk.is_none_or(|before| {
                    u128::from(disk.available_bytes) * u128::from(before.total_bytes)
                        < u128::from(before.available_bytes) * u128::from(disk.total_bytes)
                });
                if is_worse {
                    worst_disk = Some(disk);
                }
            }
            None => counts.unknown_disk = true,
        }
    }

    let claims_removal = counts.removed != 0 || counts.removed_bytes != 0;
    let (mut level, mut message) = if report.schema_version != SCHEMA_VERSION {
        (
            Level::Error,
            "the cleanup report has an unsupported version",
        )
    } else if age.is_none() {
        (
            Level::Error,
            "the cleanup report timestamp is in the future",
        )
    } else if mode == "invalid" {
        (Level::Error, "the cleanup report has an invalid mode")
    } else if report.error.is_some() {
        (Level::Error, "the last cleanup failed")
    } else if (!report.enabled || mode == "dry_run") && claims_removal {
        (
            Level::Error,
            "the cleanup report contradicts its removal mode",
        )
    } else if !report.enabled {
        (Level::Ok, "cleanup is disabled; no retention is enforced")
    } else if report.roots.is_empty() {
        (Level::Error, "the last cleanup checked no record roots")
    } else if counts.failed_roots > 0 {
        (
            Level::Error,
            "the last cleanup failed for one or more roots",
        )
    } else if counts.unknown_disk {
        (Level::Error, "disk space was unknown at the last cleanup")
    } else if worst_disk.is_some_and(|disk| below_percent(disk, 3)) {
        (
            Level::Error,
            "disk had less than 3% free at the last cleanup",
        )
    } else if worst_disk.is_some_and(|disk| below_percent(disk, 10)) {
        (
            Level::Warn,
            "disk had less than 10% free at the last cleanup",
        )
    } else if counts.refused > 0 {
        (Level::Warn, "the last cleanup refused unverified records")
    } else if counts.over_limit_roots > 0 {
        (
            Level::Warn,
            "records exceeded a byte limit after the last cleanup",
        )
    } else if mode == "dry_run" {
        (
            Level::Warn,
            "the last run was a dry run; no retention was enforced",
        )
    } else {
        (
            Level::Ok,
            "the last cleanup completed within its byte limits",
        )
    };
    if stale && level.rank() < Level::Stale.rank() {
        level = Level::Stale;
        message = "the cleanup result is over two hours old";
    }
    let named = (refused_in_a_row >= NAMED_AFTER && level == Level::Error)
        .then(|| unknown_worker(report))
        .flatten()
        .map(|worker| format!("records refused: {UNKNOWN_WORKER} `{worker}`"));
    let message = named.unwrap_or_else(|| message.to_owned());
    let message = if stale && level.rank() > Level::Stale.rank() {
        format!("{message}; the result is over two hours old")
    } else {
        message
    };
    let disk_percent = if !report.enabled || counts.unknown_disk || age.is_none() {
        "unknown".to_owned()
    } else {
        worst_disk.map_or_else(|| "unknown".to_owned(), free_percent)
    };
    Diagnostic::new(level, NAME, &message)
        .with("enabled", report.enabled)
        .with("mode", mode)
        .with(
            "report_age_s",
            age.map_or_else(|| "unknown".to_owned(), |n| n.to_string()),
        )
        .with("stale", stale)
        .with("disk_free_at_cleanup_pct", disk_percent)
        .with("roots", report.roots.len())
        .with("failed_roots", counts.failed_roots)
        .with("over_limit_roots", counts.over_limit_roots)
        .with("active_items", counts.active)
        .with("planned_items", counts.planned)
        .with("removed_items", counts.removed)
        .with("refused_items", counts.refused)
        .with("before_bytes", counts.before_bytes)
        .with("planned_bytes", counts.planned_bytes)
        .with("removed_bytes", counts.removed_bytes)
        .with("remaining_bytes", counts.remaining_bytes)
        .with("limit_bytes", counts.limit_bytes)
}

/// One publisher for the persistent report loop, independent of cleanup.
///
/// Call `publish` once per second. A successful call means the bus accepted the
/// message, not that a subscriber received it. The caller handles errors and
/// retries/recreates this publisher; no wait, thread or retry loop lives here.
pub struct Reporter {
    publisher: Publisher,
    started: Instant,
}

impl Reporter {
    pub fn new(bus: &Path) -> io::Result<Self> {
        Self::new_with_cadence_clock(bus, Instant::now)
    }

    /// Use an explicit pure clock for Bus rescans and presence refreshes in a
    /// local fixture, installed before the first declared presence write.
    /// Reads must share one nondecreasing Instant domain. The caller advances
    /// that clock; report age, this reporter's header time, Bus UTC, filesystem
    /// lease timestamps and actual message delivery keep their ordinary clocks.
    pub fn new_with_cadence_clock(
        bus: &Path,
        now: impl Fn() -> Instant + Send + Sync + 'static,
    ) -> io::Result<Self> {
        let bus = Bus::new(bus)?;
        Ok(Self {
            publisher: bus.publisher_declared_with_cadence_clock(
                Diagnostic::TOPIC,
                &Diagnostic::declaration(),
                NAME,
                now,
            )?,
            started: Instant::now(),
        })
    }

    pub fn publish(&mut self, report: &Diagnostic) -> io::Result<()> {
        let at_us = u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX);
        self.publisher.publish_json(at_us, report).map(|_| ())
    }
}

#[cfg(test)]
#[path = "reporter_clock_tests.rs"]
mod clock_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RootReport;

    fn root() -> RootReport {
        RootReport {
            root: "/private/no-path-in-diagnostics".into(),
            unit: "private-unit-name.service".into(),
            before_bytes: 140,
            planned_bytes: 40,
            removed_bytes: 40,
            remaining_bytes: 100,
            limit_bytes: 100,
            active: vec!["private-active-invocation".into()],
            planned: vec!["private-planned-invocation".into()],
            removed: vec!["private-removed-invocation".into()],
            refused: vec![],
            processes: crate::ProcessObservations::default(),
            disk: Some(Disk {
                total_bytes: 1_000,
                available_bytes: 470,
            }),
            error: None,
        }
    }

    fn report() -> RunReport {
        RunReport {
            schema_version: SCHEMA_VERSION,
            config_fingerprint: "0123456789abcdef".repeat(4),
            at_unix_s: 10_000,
            mode: "apply".into(),
            enabled: true,
            roots: vec![root()],
            error: None,
        }
    }

    fn value<'a>(report: &'a Diagnostic, key: &str) -> &'a str {
        &report
            .values
            .iter()
            .find(|pair| pair.key == key)
            .unwrap()
            .value
    }

    /// #1071: a cleanup the guard refused for a worker it does not know is
    /// named after two in a row, and only then; one that was not refused
    /// for it starts the count again.
    #[test]
    fn two_cleanups_in_a_row_refused_for_an_unknown_worker_name_it() {
        let refused = |at| {
            let mut run = report();
            run.at_unix_s = at;
            run.roots[0].removed.clear();
            run.roots[0].removed_bytes = 0;
            run.roots[0].error =
                Some("process 4242 protection is incomplete: unknown voice worker `second`".into());
            run
        };
        let mut row = RefusedInARow::default();
        let first = refused(10_000);
        assert_eq!(row.observe(&first), 1);
        // The same result read again, once a second, is still one cleanup.
        assert_eq!(row.observe(&first), 1);
        let once = diagnostic_after(Some(&first), 10_001, 1);
        assert_eq!(once.level, Level::Error);
        assert_eq!(
            once.message,
            "the last cleanup failed for one or more roots"
        );
        let second = refused(13_600);
        assert_eq!(row.observe(&second), 2);
        let named = diagnostic_after(Some(&second), 13_601, 2);
        assert_eq!(named.level, Level::Error);
        assert_eq!(
            named.message,
            "records refused: unknown voice worker `second`"
        );
        // A cleanup that was not refused for it: none in a row.
        let mut fine = report();
        fine.at_unix_s = 17_200;
        assert_eq!(row.observe(&fine), 0);
        assert_eq!(diagnostic_after(Some(&fine), 17_201, 0).level, Level::Ok);
        // Another refusal, or an unplain word, is never named.
        let mut other = refused(20_800);
        other.roots[0].error = Some("unknown voice worker command".into());
        assert_eq!(row.observe(&other), 0);
        let mut odd = refused(24_400);
        odd.roots[0].error = Some("unknown voice worker `Two Words`".into());
        assert_eq!(unknown_worker(&odd), None);
        assert_eq!(
            diagnostic_after(Some(&odd), 24_401, 5).message,
            "the last cleanup failed for one or more roots"
        );
    }

    #[test]
    fn healthy_report_identifies_snapshot_age_and_aggregates_without_record_ids() {
        let mut run = report();
        run.roots.push(root());
        let result = diagnostic(Some(&run), 10_123);
        assert_eq!(result.name, NAME);
        assert_eq!(result.level, Level::Ok);
        assert_eq!(value(&result, "report_age_s"), "123");
        assert_eq!(value(&result, "disk_free_at_cleanup_pct"), "47.00");
        assert_eq!(value(&result, "roots"), "2");
        assert_eq!(value(&result, "active_items"), "2");
        assert_eq!(value(&result, "planned_items"), "2");
        assert_eq!(value(&result, "removed_items"), "2");
        assert_eq!(value(&result, "before_bytes"), "280");
        assert_eq!(value(&result, "removed_bytes"), "80");
        assert_eq!(value(&result, "remaining_bytes"), "200");
        assert_eq!(value(&result, "limit_bytes"), "200");
        assert!(!serde_json::to_string(&result).unwrap().contains("private"));
    }

    #[test]
    fn no_report_and_an_enabled_empty_report_are_errors() {
        let missing = diagnostic(None, 1);
        assert_eq!(missing.level, Level::Error);
        assert_eq!(value(&missing, "report_age_s"), "unknown");
        let mut run = report();
        run.roots.clear();
        assert_eq!(diagnostic(Some(&run), 10_000).level, Level::Error);
    }

    #[test]
    fn disabled_cleanup_is_visible_without_claiming_a_disk_scan() {
        let mut run = report();
        run.enabled = false;
        run.roots.clear();
        let result = diagnostic(Some(&run), 10_010);
        assert_eq!(result.level, Level::Ok);
        assert!(result.message.contains("disabled"));
        assert_eq!(value(&result, "enabled"), "false");
        assert_eq!(value(&result, "disk_free_at_cleanup_pct"), "unknown");
        assert_eq!(value(&result, "removed_items"), "0");
    }

    #[test]
    fn a_disabled_flag_does_not_hide_a_failure_or_impossible_removal() {
        let mut run = report();
        run.enabled = false;
        assert_eq!(diagnostic(Some(&run), 10_000).level, Level::Error);
        run.roots.clear();
        run.error = Some("private configuration failure".into());
        let result = diagnostic(Some(&run), 10_000);
        assert_eq!(result.level, Level::Error);
        assert!(!result.message.contains("private"));
    }

    #[test]
    fn unsupported_schema_invalid_mode_and_clock_rollback_are_errors() {
        let mut run = report();
        run.schema_version += 1;
        assert_eq!(diagnostic(Some(&run), 10_000).level, Level::Error);
        run.schema_version = SCHEMA_VERSION;
        run.mode = "private\nsecret-mode".into();
        let result = diagnostic(Some(&run), 10_000);
        assert_eq!(result.level, Level::Error);
        assert_eq!(value(&result, "mode"), "invalid");
        assert!(!serde_json::to_string(&result).unwrap().contains("private"));
        run.mode = "apply".into();
        let result = diagnostic(Some(&run), 9_999);
        assert_eq!(result.level, Level::Error);
        assert_eq!(value(&result, "report_age_s"), "unknown");
        assert_eq!(value(&result, "disk_free_at_cleanup_pct"), "unknown");
        assert!(result.message.contains("future"));
        run.at_unix_s = u64::MAX;
        assert_eq!(diagnostic(Some(&run), 0).level, Level::Error);
    }

    #[test]
    fn two_hour_boundary_is_exact_and_large_clock_jumps_do_not_wrap() {
        let run = report();
        assert_eq!(diagnostic(Some(&run), 17_200).level, Level::Ok);
        let stale = diagnostic(Some(&run), 17_201);
        assert_eq!(stale.level, Level::Stale);
        assert_eq!(value(&stale, "report_age_s"), "7201");
        assert_eq!(value(&stale, "stale"), "true");
        let old = diagnostic(Some(&run), u64::MAX);
        assert_eq!(old.level, Level::Stale);
        assert_eq!(
            value(&old, "report_age_s"),
            (u64::MAX - run.at_unix_s).to_string()
        );
    }

    #[test]
    fn repeated_reports_do_not_refresh_the_cleanup_timestamp() {
        let run = report();
        for now in [10_000, 10_001, 11_000, 17_201] {
            let result = diagnostic(Some(&run), now);
            assert_eq!(value(&result, "report_age_s"), (now - 10_000).to_string());
        }
        assert_eq!(run.at_unix_s, 10_000);
    }

    #[test]
    fn stale_warning_or_error_is_not_downgraded() {
        let mut run = report();
        run.roots[0].refused.push("private-refused".into());
        let warning = diagnostic(Some(&run), 17_201);
        assert_eq!(warning.level, Level::Warn);
        assert!(warning.message.contains("two hours"));
        run.error = Some("private run error".into());
        let error = diagnostic(Some(&run), 17_201);
        assert_eq!(error.level, Level::Error);
        assert!(error.message.contains("two hours"));
        assert!(!serde_json::to_string(&error).unwrap().contains("private"));
    }

    #[test]
    fn exact_disk_boundaries_do_not_round_a_warning_away() {
        let mut run = report();
        for (available, expected, printed) in [
            (0, Level::Error, "0.00"),
            (299, Level::Error, "2.99"),
            (300, Level::Warn, "3.00"),
            (999, Level::Warn, "9.99"),
            (1_000, Level::Ok, "10.00"),
            (10_000, Level::Ok, "100.00"),
        ] {
            run.roots[0].disk = Some(Disk {
                total_bytes: 10_000,
                available_bytes: available,
            });
            let result = diagnostic(Some(&run), 10_000);
            assert_eq!(result.level, expected, "available={available}");
            assert_eq!(value(&result, "disk_free_at_cleanup_pct"), printed);
        }
        run.roots[0].disk = Some(Disk {
            total_bytes: u64::MAX,
            available_bytes: u64::MAX / 10,
        });
        let result = diagnostic(Some(&run), 10_000);
        assert_eq!(result.level, Level::Warn);
        assert_eq!(value(&result, "disk_free_at_cleanup_pct"), "9.99");
    }

    #[test]
    fn missing_zero_or_impossible_disk_values_are_errors() {
        let mut run = report();
        for disk in [
            None,
            Some(Disk {
                total_bytes: 0,
                available_bytes: 0,
            }),
            Some(Disk {
                total_bytes: 1,
                available_bytes: 2,
            }),
        ] {
            run.roots[0].disk = disk;
            let result = diagnostic(Some(&run), 10_000);
            assert_eq!(result.level, Level::Error);
            assert_eq!(value(&result, "disk_free_at_cleanup_pct"), "unknown");
        }
    }

    #[test]
    fn the_worst_root_disk_is_not_hidden_by_averaging_or_double_counting() {
        let mut run = report();
        run.roots[0].disk = Some(Disk {
            total_bytes: 100,
            available_bytes: 2,
        });
        let mut other = root();
        other.disk = Some(Disk {
            total_bytes: u64::MAX,
            available_bytes: u64::MAX,
        });
        run.roots.push(other);
        let result = diagnostic(Some(&run), 10_000);
        assert_eq!(result.level, Level::Error);
        assert_eq!(value(&result, "disk_free_at_cleanup_pct"), "2.00");
        run.roots.reverse();
        assert_eq!(diagnostic(Some(&run), 10_000), result);
    }

    #[test]
    fn refusals_byte_overruns_and_root_failures_remain_visible() {
        let mut run = report();
        run.roots[0].remaining_bytes = 101;
        let over = diagnostic(Some(&run), 10_000);
        assert_eq!(over.level, Level::Warn);
        assert_eq!(value(&over, "over_limit_roots"), "1");
        run.roots[0].remaining_bytes = 100;
        run.roots[0].refused.push("private-refusal-reason".into());
        let refused = diagnostic(Some(&run), 10_000);
        assert_eq!(refused.level, Level::Warn);
        assert_eq!(value(&refused, "refused_items"), "1");
        run.roots[0].error = Some("private root error".into());
        let failed = diagnostic(Some(&run), 10_000);
        assert_eq!(failed.level, Level::Error);
        assert_eq!(value(&failed, "failed_roots"), "1");
        assert!(!serde_json::to_string(&failed).unwrap().contains("private"));
    }

    #[test]
    fn dry_run_never_claims_enforced_retention_or_removals() {
        let mut run = report();
        run.mode = "dry_run".into();
        run.roots[0].removed.clear();
        run.roots[0].removed_bytes = 0;
        let result = diagnostic(Some(&run), 10_000);
        assert_eq!(result.level, Level::Warn);
        assert_eq!(value(&result, "mode"), "dry_run");
        assert_eq!(value(&result, "planned_items"), "1");
        assert_eq!(value(&result, "removed_items"), "0");
        assert_eq!(value(&result, "removed_bytes"), "0");
        run.roots[0].removed_bytes = 1;
        assert_eq!(diagnostic(Some(&run), 10_000).level, Level::Error);
    }

    #[test]
    fn large_counts_and_private_strings_do_not_overflow_or_expand_the_message() {
        let mut run = report();
        let private = "private-secret\n".repeat(60);
        let mut item = root();
        item.active = vec![private.clone()];
        item.planned = vec![private.clone()];
        item.removed = vec![private.clone()];
        item.refused = vec![private.clone()];
        item.error = Some(private.clone());
        item.before_bytes = u64::MAX;
        item.planned_bytes = u64::MAX;
        item.removed_bytes = u64::MAX;
        item.remaining_bytes = u64::MAX;
        item.limit_bytes = u64::MAX;
        run.roots = vec![item; crate::config::MAX_ROOTS];
        run.error = Some(private);
        let result = diagnostic(Some(&run), u64::MAX);
        assert_eq!(result.level, Level::Error);
        assert_eq!(
            value(&result, "removed_bytes"),
            (u128::from(u64::MAX) * crate::config::MAX_ROOTS as u128).to_string()
        );
        let bytes = serde_json::to_vec(&result).unwrap();
        assert!(bytes.len() <= 1_536, "{} bytes", bytes.len());
        assert!(result.message.len() <= 160);
        assert!(!String::from_utf8(bytes).unwrap().contains("private-secret"));
    }

    #[test]
    fn invalid_in_memory_reports_never_become_ok_or_publish_unvalidated_counters() {
        let mut malformed = report();
        malformed.config_fingerprint = "private invalid digest".into();
        let mut oversized = report();
        oversized.roots[0].refused = vec![String::new(); crate::MAX_REPORT_ITEMS + 1];
        let mut disabled = report();
        disabled.enabled = false;
        disabled.roots.clear();
        disabled.config_fingerprint.clear();
        for run in [malformed, oversized, disabled] {
            let result = diagnostic(Some(&run), 10_000);
            assert_eq!(result.level, Level::Error);
            assert_eq!(value(&result, "disk_free_at_cleanup_pct"), "unknown");
            assert!(!result.values.iter().any(|pair| pair.key == "removed_bytes"));
            assert!(!serde_json::to_string(&result).unwrap().contains("private"));
        }
    }

    #[test]
    fn reporter_refuses_a_relative_bus_without_creating_a_directory() {
        assert!(matches!(
            Reporter::new(Path::new("relative-bus")),
            Err(error) if error.kind() == io::ErrorKind::InvalidInput
        ));
    }
}
