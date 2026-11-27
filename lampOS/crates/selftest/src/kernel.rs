//! What the kernel has said since boot about the lamp's storage. Only what
//! hurts the card (a line naming `mmcblk`) fails: an I/O error, an ext4 error, its filesystem
//! remounted read-only, or a task the kernel saw blocked for minutes. The
//! SD/MMC hosts' periodic re-tune (#784: about hourly, 3,670 s on lamp-52e6, on each of the two
//! sunxi hosts, often with "wait dma hold bit clear timeout") is counted
//! and passes; the parts ride it out since #791 and #911.
//! Read from `/dev/kmsg`, which every record since boot is in until the
//! kernel's ring buffer wraps; nothing is written to it.
use crate::Check;
use lamp_msgs::{PartResult, Verdict};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

/// The kernel's ring of messages since boot.
pub const KMSG: &str = "/dev/kmsg";

/// Each scan gets this much time, or the caller's earlier deadline. This is
/// checked between nonblocking reads and record callbacks; it cannot interrupt
/// a syscall or a callback already in progress.
pub const SCAN_BUDGET: Duration = Duration::from_millis(100);
pub const SCAN_BYTES: usize = 1024 * 1024;
pub const SCAN_RECORDS: usize = 4096;
pub const SCAN_ATTEMPTS: usize = 4096;
/// This reader's fixed per-read buffer, not a universal kernel record limit.
/// A record that cannot fit and causes a read error leaves the scan incomplete.
#[cfg(any(target_os = "linux", test))]
const RECORD_BYTES: usize = 8192;

/// Why a scan ended. Reading starts at the retained oldest record: a limit
/// leaves a prefix, not the newest records. `Complete` means the retained ring
/// was drained, not that it still contains the whole boot. No error or record
/// text is kept here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScanStop {
    #[default]
    Complete,
    Deadline,
    ByteLimit,
    RecordLimit,
    AttemptLimit,
    ReadError,
}

impl ScanStop {
    pub fn code(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Deadline => "deadline",
            Self::ByteLimit => "byte_limit",
            Self::RecordLimit => "record_limit",
            Self::AttemptLimit => "attempt_limit",
            Self::ReadError => "read_error",
        }
    }
}

/// Counts for one bounded scan. `records` counts delivered lines, including
/// continuations, so the cap also bounds inputs containing many short lines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scan {
    pub stop: ScanStop,
    pub overwritten: bool,
    pub bytes: usize,
    pub attempts: usize,
    pub records: usize,
}

impl Scan {
    pub fn complete(self) -> bool {
        self.stop == ScanStop::Complete
    }
}

/// A host starting a re-tune: one per host per round.
pub const RETUNE: &str = "start tuning";

/// A re-tune's DMA wait that timed out (sunxi, #784); the re-tune goes on.
pub const DMA_TIMEOUT: &str = "dma hold bit clear timeout";

/// Two re-tunes of one host closer than this are more often than the
/// hourly round: lamp-52e6 on 17 Oct re-tuned each host every 3,669.8 to
/// 3,670.0 s, lamp-4ace every 61 minutes (#784).
pub const RETUNE_OFTEN_S: f64 = 3_000.0;

/// The SD card's block devices (`mmcblk1`, `mmcblk1p1`; `mmcblk0` on a
/// board that numbers it so). A fault line counts only when it names one:
/// a USB stick's `sda1`, or a USB audio task hung after a hub drop, is
/// not the card.
pub const CARD: &str = "mmcblk";

/// ext4 reporting a fault it found (`__ext4_error`).
pub const EXT4_ERROR: &str = "EXT4-fs error";

/// ext4 giving up on writing (`errors=remount-ro`, the lamps' root).
pub const READ_ONLY: &str = "Remounting filesystem read-only";

/// The kernel's hung-task report: a task held 120 s or more.
pub const BLOCKED: &str = "blocked for more than";

/// What the kernel's storage messages since boot came to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Storage {
    /// Records read.
    pub records: usize,
    /// Re-tunes started, every host's.
    pub retunes: usize,
    /// DMA waits inside re-tunes that timed out.
    pub dma_timeouts: usize,
    /// The shortest time between two re-tunes of one host, in seconds.
    pub shortest_retune_gap_s: Option<f64>,
    /// I/O errors on an MMC block device.
    pub io_errors: usize,
    /// ext4 errors.
    pub ext4_errors: usize,
    /// The filesystem remounted read-only.
    pub read_only: bool,
    /// Tasks the kernel saw blocked on the card (`jbd2/mmcblk1p1-8`).
    pub blocked: usize,
    /// Tasks the kernel saw blocked that do not name the card; they pass.
    pub blocked_other: usize,
    /// Records of this boot the ring no longer held: the first one kept was
    /// not the boot's first (sequence 0), or some were overwritten while
    /// they were read.
    pub lost: bool,
    /// The sequence number of the first record kept, when the records say.
    pub first_sequence: Option<u64>,
    /// The scan's bounds and completion, when read from the kernel. A limited
    /// or interrupted scan keeps the observations above as lower bounds.
    pub scan: Option<Scan>,
    /// Each host's last re-tune, in the kernel's seconds since boot.
    last_retune: HashMap<String, f64>,
}

impl Storage {
    /// Counts one record's text: `pri,seq,usec,flags;message`, or the bare
    /// message. Continuation lines (starting with a space) are not records.
    pub fn count(&mut self, record: &str) {
        if record.starts_with(' ') {
            return;
        }
        let (prefix, message) = record.split_once(';').unwrap_or(("", record));
        let mut fields = prefix.split(',');
        let sequence = fields.nth(1).and_then(|s| s.parse::<u64>().ok());
        let at_s = fields
            .next()
            .and_then(|s| s.parse::<u64>().ok())
            .map(|us| us as f64 / 1e6);
        if self.records == 0 {
            // `pri,seq,usec,flags`: the boot's first record has sequence 0.
            self.first_sequence = sequence;
            if self.first_sequence.is_some_and(|first| first > 0) {
                self.lost = true;
            }
        }
        self.records += 1;
        if let Some((host, _)) = message.split_once(RETUNE) {
            self.retunes += 1;
            if let Some(at_s) = at_s
                && let Some(last) = self.last_retune.insert(host.to_owned(), at_s)
            {
                let gap = at_s - last;
                if self
                    .shortest_retune_gap_s
                    .is_none_or(|shortest| gap < shortest)
                {
                    self.shortest_retune_gap_s = Some(gap);
                }
            }
        } else if message.contains(DMA_TIMEOUT) {
            self.dma_timeouts += 1;
        } else if !message.contains(CARD) {
            if message.contains(BLOCKED) {
                self.blocked_other += 1;
            }
        } else if message.contains("I/O error") {
            self.io_errors += 1;
        } else if message.contains(EXT4_ERROR) {
            self.ext4_errors += 1;
        } else if message.contains(READ_ONLY) {
            self.read_only = true;
        } else if message.contains(BLOCKED) {
            self.blocked += 1;
        }
    }

    /// What hurt, worst first, if anything did.
    fn fault(&self) -> Option<String> {
        if self.read_only {
            Some("the card's filesystem was remounted read-only".into())
        } else if self.io_errors > 0 {
            Some(format!(
                "{} on the card since boot",
                times(self.io_errors, "I/O error", "I/O errors")
            ))
        } else if self.ext4_errors > 0 {
            Some(format!(
                "{} on the card since boot",
                times(self.ext4_errors, "ext4 error", "ext4 errors")
            ))
        } else if self.blocked > 0 {
            Some(format!(
                "the kernel saw {} on the card for minutes since boot",
                times(self.blocked, "task blocked", "tasks blocked")
            ))
        } else {
            None
        }
    }
}

/// `1 I/O error`, `2 I/O errors`.
fn times(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The storage is healthy if the kernel reported no fault that hurts
/// since boot: no I/O error on the card, no ext4 error, no read-only
/// remount, no task blocked for minutes. Re-tunes pass; one host
/// re-tuning more often than the hourly round says so in the reason. A
/// fault in the records kept fails whatever was lost; with records of the
/// boot lost and no fault in the rest, the boot as a whole is not checked.
pub fn storage(heard: &Result<Storage, String>) -> PartResult {
    let part = "storage";
    let storage = match heard {
        Ok(storage) => storage,
        Err(why) => {
            return PartResult::new(
                part,
                Verdict::NotChecked,
                &format!("the kernel's log cannot be read: {why}"),
            );
        }
    };
    let incomplete = storage.scan.filter(|scan| !scan.complete());
    let often = storage
        .shortest_retune_gap_s
        .filter(|gap| *gap < RETUNE_OFTEN_S);
    let result = if let Some(fault) = storage.fault() {
        let fault = incomplete.map_or(fault.clone(), |scan| {
            format!(
                "{fault}; the kernel log scan was incomplete ({}); counts are lower bounds",
                scan.stop.code()
            )
        });
        PartResult::new(part, Verdict::Fail, &fault)
    } else if let Some(scan) = incomplete {
        PartResult::new(
            part,
            Verdict::NotChecked,
            &format!(
                "no fault observed, but the kernel log scan was incomplete ({})",
                scan.stop.code()
            ),
        )
    } else if storage.records == 0 {
        PartResult::new(part, Verdict::NotChecked, "the kernel's log was empty")
    } else if storage.lost {
        PartResult::new(
            part,
            Verdict::NotChecked,
            &format!(
                "no fault in the kernel's log, but it no longer holds all of this boot (first record kept: {}){}",
                storage
                    .first_sequence
                    .map_or("unknown".into(), |first| first.to_string()),
                often.map_or(String::new(), |gap| format!(
                    "; the SD/MMC host re-tuned more often than hourly, {gap:.0} s apart"
                ))
            ),
        )
    } else if let Some(gap) = often {
        PartResult::new(
            part,
            Verdict::Pass,
            &format!("the SD/MMC host re-tuned more often than hourly, {gap:.0} s apart (#784)"),
        )
    } else {
        PartResult::new(part, Verdict::Pass, "")
    };
    let result = result
        .with("retunes", storage.retunes)
        .with("dma_timeouts", storage.dma_timeouts)
        .with("io_errors", storage.io_errors)
        .with("ext4_errors", storage.ext4_errors)
        .with("blocked", storage.blocked)
        .with("blocked_other", storage.blocked_other)
        .with("records", storage.records);
    let result = match often {
        Some(gap) => result.with("shortest_retune_gap_s", format!("{gap:.0}")),
        None => result,
    };
    let result = if storage.read_only {
        result.with("read_only", "yes")
    } else {
        result
    };
    let result = if storage.lost {
        result.with("oldest_lost", "yes")
    } else {
        result
    };
    if let Some(scan) = storage.scan {
        result
            .with("scan_stop", scan.stop.code())
            .with("scan_order", "oldest_first")
            .with("scan_bytes", scan.bytes)
            .with("scan_attempts", scan.attempts)
    } else {
        result
    }
}

/// The records observed within one bounded scan of the kernel's retained ring.
pub fn read(path: &str, deadline: Instant) -> Result<Storage, String> {
    let mut storage = Storage::default();
    let scan = each_record(path, deadline, |line| storage.count(line))?;
    if scan.overwritten {
        storage.lost = true;
    }
    storage.scan = Some(scan);
    Ok(storage)
}

/// Hands bounded record lines to `line`. Open failures return an error; a
/// later read failure returns an incomplete scan, preserving earlier evidence.
/// An already expired deadline causes no open. Linux's O_NONBLOCK prevents
/// waiting for a new record, but does not promise a hard syscall deadline.
pub fn each_record(path: &str, deadline: Instant, line: impl FnMut(&str)) -> Result<Scan, String> {
    let deadline = (Instant::now() + SCAN_BUDGET).min(deadline);
    if Instant::now() >= deadline {
        return Ok(Scan {
            stop: ScanStop::Deadline,
            ..Scan::default()
        });
    }
    #[cfg(target_os = "linux")]
    {
        use std::{fs::OpenOptions, io::Read, os::unix::fs::OpenOptionsExt};
        // O_NONBLOCK: a read at the end of the ring gives WouldBlock, not a wait.
        const O_NONBLOCK: i32 = 0o4000;
        let mut kmsg = OpenOptions::new()
            .read(true)
            .custom_flags(O_NONBLOCK)
            .open(path)
            .map_err(|e| e.to_string())?;
        Ok(scan_records(
            |buffer| kmsg.read(buffer),
            deadline,
            Instant::now,
            line,
        ))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (path, line);
        Err("only Linux has a kernel log to read".into())
    }
}

/// The runtime reader seam, with an injected clock for deterministic tests.
/// A kernel read is a whole record. Never shrink its buffer to the remaining
/// byte allowance: stop before a record might exceed that allowance instead.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn scan_records(
    mut read: impl FnMut(&mut [u8]) -> std::io::Result<usize>,
    deadline: Instant,
    mut now: impl FnMut() -> Instant,
    mut line: impl FnMut(&str),
) -> Scan {
    use std::io::ErrorKind;
    let deadline = (now() + SCAN_BUDGET).min(deadline);
    let mut scan = Scan::default();
    let mut buffer = [0u8; RECORD_BYTES];
    let stop = 'scan: loop {
        if now() >= deadline {
            break ScanStop::Deadline;
        }
        if scan.bytes > SCAN_BYTES - RECORD_BYTES {
            break ScanStop::ByteLimit;
        }
        if scan.records >= SCAN_RECORDS {
            break ScanStop::RecordLimit;
        }
        if scan.attempts >= SCAN_ATTEMPTS {
            break ScanStop::AttemptLimit;
        }
        scan.attempts += 1;
        match read(&mut buffer) {
            // Check the deadline after a read too, including a late EOF. It
            // cannot retrospectively make an over-budget scan complete.
            Ok(0) => {
                break if now() >= deadline {
                    ScanStop::Deadline
                } else {
                    ScanStop::Complete
                };
            }
            Ok(n) if n <= buffer.len() => {
                scan.bytes += n;
                for each in String::from_utf8_lossy(&buffer[..n]).lines() {
                    if now() >= deadline {
                        break 'scan ScanStop::Deadline;
                    }
                    if scan.records >= SCAN_RECORDS {
                        break 'scan ScanStop::RecordLimit;
                    }
                    scan.records += 1;
                    line(each);
                }
            }
            Ok(_) => break ScanStop::ReadError,
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                break if now() >= deadline {
                    ScanStop::Deadline
                } else {
                    ScanStop::Complete
                };
            }
            // Every retry costs one attempt, even if time has not advanced.
            Err(e) if e.raw_os_error() == Some(32) => scan.overwritten = true,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(_) => break ScanStop::ReadError,
        }
    };
    scan.stop = stop;
    scan
}

/// Reads the kernel's log and judges the storage.
pub struct Kernel {
    pub path: String,
}

impl Check for Kernel {
    fn part(&self) -> &str {
        "storage"
    }

    fn run(&mut self, deadline: Instant) -> PartResult {
        storage(&read(&self.path, deadline))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, io};

    fn copied(buffer: &mut [u8], record: &[u8]) -> io::Result<usize> {
        buffer[..record.len()].copy_from_slice(record);
        Ok(record.len())
    }

    fn scanned(
        read: impl FnMut(&mut [u8]) -> io::Result<usize>,
        deadline: Instant,
        now: impl FnMut() -> Instant,
    ) -> Storage {
        let mut storage = Storage::default();
        let scan = scan_records(read, deadline, now, |line| storage.count(line));
        storage.lost |= scan.overwritten;
        storage.scan = Some(scan);
        storage
    }

    /// Opt-in host evidence, not a timing assertion or a device benchmark.
    /// Inputs are generated before timing; the measured boundary starts before
    /// Storage construction and ends when the reader returns. It includes
    /// buffer copies, UTF-8/line parsing and the actual Storage::count callback,
    /// but no file open, kernel I/O, result formatting or aggregate destruction.
    #[test]
    #[ignore = "explicit host timing measurement; no timing assertion"]
    #[allow(
        clippy::print_stdout,
        reason = "Only this opt-in test prints fixed timing and coverage fields; the runtime library remains silent"
    )]
    fn measure_bounded_kernel_scan_host() {
        use std::{collections::BTreeMap, hint::black_box};

        const REPEATS: usize = 31;

        struct Measurement {
            elapsed: Duration,
            scan: Scan,
            counted: usize,
        }

        fn records(bytes: usize) -> Vec<Vec<u8>> {
            [
                "Booting synthetic kernel",
                "host0: start tuning",
                "host0: dma hold bit clear timeout",
                "mmcblk1: synthetic status",
                "host1: start tuning",
                "host1: dma hold bit clear timeout",
                "synthetic USB status",
                "synthetic scheduler status",
            ]
            .iter()
            .enumerate()
            .map(|(index, text)| {
                let mut record = format!("6,{index},{},-;{text}", index * 1000).into_bytes();
                record.resize(bytes - 1, b'.');
                record.push(b'\n');
                record
            })
            .collect()
        }

        fn measure(records: &[Vec<u8>], count: Option<usize>, bounded: bool) -> Measurement {
            // The previous success-path loop is compared only on finite input.
            // Count its work too; do not put the continuous reader through it.
            assert!(bounded || count.is_some());
            let mut next = 0;
            let mut read = |buffer: &mut [u8]| {
                if count.is_some_and(|count| next >= count) {
                    return Ok(0);
                }
                let record = &records[next % records.len()];
                next += 1;
                copied(buffer, record)
            };
            let start = Instant::now();
            let mut storage = Storage::default();
            let scan = if bounded {
                scan_records(
                    &mut read,
                    start + Duration::from_secs(1),
                    Instant::now,
                    |line| storage.count(line),
                )
            } else {
                // This reproduces the old finite-drain success path, with
                // aggregate counters added for comparable coverage reporting.
                let mut scan = Scan::default();
                let mut buffer = vec![0u8; RECORD_BYTES];
                loop {
                    scan.attempts += 1;
                    let bytes = read(&mut buffer).unwrap();
                    if bytes == 0 {
                        break;
                    }
                    scan.bytes += bytes;
                    for line in String::from_utf8_lossy(&buffer[..bytes]).lines() {
                        scan.records += 1;
                        storage.count(line);
                    }
                }
                scan
            };
            let elapsed = start.elapsed();
            let counted = storage.records;
            black_box(storage);
            Measurement {
                elapsed,
                scan,
                counted,
            }
        }

        println!(
            "kernel_scan_host os={} arch={} debug_assertions={} repeats={REPEATS}; boundary=Storage_new_through_reader_return; excludes=open_kernel_io_format_drop",
            std::env::consts::OS,
            std::env::consts::ARCH,
            cfg!(debug_assertions),
        );
        for (workload, bytes, count) in [
            ("small_finite", 256, Some(128)),
            ("burst_finite", 1024, Some(16_384)),
            ("continuous", 128, None),
        ] {
            let records = records(bytes);
            for bounded in [false, true] {
                if !bounded && count.is_none() {
                    continue;
                }
                let first = measure(&records, count, bounded);
                let repeated: Vec<Measurement> = (0..REPEATS)
                    .map(|_| measure(&records, count, bounded))
                    .collect();
                let mut elapsed: Vec<Duration> = repeated.iter().map(|run| run.elapsed).collect();
                elapsed.sort_unstable();
                let mut stops = BTreeMap::<&str, usize>::new();
                for run in &repeated {
                    *stops.entry(run.scan.stop.code()).or_default() += 1;
                }
                let byte_range = repeated.iter().map(|run| run.scan.bytes);
                let record_range = repeated.iter().map(|run| run.counted);
                println!(
                    "kernel_scan_host workload={workload} mode={} input_records={} record_bytes={bytes}; first_us={} first_bytes={} first_records={} first_stop={}; repeated_median_us={} repeated_p95_us={} repeated_max_us={} repeated_bytes={}..{} repeated_records={}..{} stops={stops:?}",
                    if bounded {
                        "bounded"
                    } else {
                        "old_finite_drain"
                    },
                    count.map_or_else(|| "continuous".into(), |count| count.to_string()),
                    first.elapsed.as_micros(),
                    first.scan.bytes,
                    first.counted,
                    first.scan.stop.code(),
                    elapsed[REPEATS / 2].as_micros(),
                    elapsed[(REPEATS * 95).div_ceil(100) - 1].as_micros(),
                    elapsed[REPEATS - 1].as_micros(),
                    byte_range.clone().min().unwrap(),
                    byte_range.max().unwrap(),
                    record_range.clone().min().unwrap(),
                    record_range.max().unwrap(),
                );
            }
        }
    }

    #[test]
    fn eof_and_would_block_complete_only_the_retained_scan() {
        let start = Instant::now();
        for end in [false, true] {
            let mut first = true;
            let heard = scanned(
                |buffer| {
                    if std::mem::take(&mut first) {
                        copied(
                            buffer,
                            b"6,0,0,-;Booting\n SUBSYSTEM=platform\n6,1,1,-;ok\n",
                        )
                    } else if end {
                        Err(io::ErrorKind::WouldBlock.into())
                    } else {
                        Ok(0)
                    }
                },
                start + Duration::from_secs(1),
                || start,
            );
            let scan = heard.scan.unwrap();
            assert_eq!(scan.stop, ScanStop::Complete);
            assert_eq!((scan.attempts, scan.records, heard.records), (2, 3, 2));
            assert_eq!(storage(&Ok(heard)).verdict, Verdict::Pass);
        }
    }

    #[test]
    fn continuous_records_stop_at_the_record_limit_with_a_frozen_clock() {
        let start = Instant::now();
        let calls = Cell::new(0);
        let heard = scanned(
            |buffer| {
                calls.set(calls.get() + 1);
                copied(buffer, b"6,0,0,-;ok\n")
            },
            start + Duration::from_secs(1),
            || start,
        );
        let scan = heard.scan.unwrap();
        assert_eq!(scan.stop, ScanStop::RecordLimit);
        assert_eq!(calls.get(), SCAN_RECORDS);
        assert_eq!((scan.attempts, scan.records), (SCAN_RECORDS, SCAN_RECORDS));
        assert!(scan.bytes < SCAN_BYTES);
        assert_eq!(storage(&Ok(heard)).verdict, Verdict::NotChecked);
    }

    #[test]
    fn many_lines_in_one_read_are_bounded_too() {
        let start = Instant::now();
        let mut bytes = vec![b'\n'; RECORD_BYTES];
        // Empty lines are still callbacks to the existing counters, so they
        // consume the same bound as any other line.
        bytes[0] = b'x';
        let scan = scan_records(
            |buffer| copied(buffer, &bytes),
            start + SCAN_BUDGET,
            || start,
            |_| {},
        );
        assert_eq!(scan.stop, ScanStop::RecordLimit);
        assert_eq!((scan.attempts, scan.records), (1, SCAN_RECORDS));
    }

    #[test]
    fn continuous_full_records_stop_at_one_mebibyte() {
        let start = Instant::now();
        let mut record = vec![b'x'; RECORD_BYTES];
        record[RECORD_BYTES - 1] = b'\n';
        let scan = scan_records(
            |buffer| copied(buffer, &record),
            start + SCAN_BUDGET,
            || start,
            |_| {},
        );
        assert_eq!(scan.stop, ScanStop::ByteLimit);
        assert_eq!(scan.bytes, SCAN_BYTES);
        assert_eq!(scan.attempts, SCAN_BYTES / RECORD_BYTES);
        assert_eq!(scan.records, scan.attempts);
    }

    #[test]
    fn it_never_reads_a_record_that_could_exceed_the_byte_allowance() {
        let start = Instant::now();
        let mut record = vec![b'x'; RECORD_BYTES - 1];
        record[RECORD_BYTES - 2] = b'\n';
        let scan = scan_records(
            |buffer| copied(buffer, &record),
            start + SCAN_BUDGET,
            || start,
            |_| {},
        );
        assert_eq!(scan.stop, ScanStop::ByteLimit);
        assert!(scan.bytes <= SCAN_BYTES);
        assert!(SCAN_BYTES - scan.bytes < RECORD_BYTES);
    }

    #[test]
    fn interrupted_and_overwritten_storms_have_an_attempt_bound() {
        let start = Instant::now();
        for overwritten in [false, true] {
            let scan = scan_records(
                |_| {
                    Err(if overwritten {
                        io::Error::from_raw_os_error(32)
                    } else {
                        io::ErrorKind::Interrupted.into()
                    })
                },
                start + SCAN_BUDGET,
                || start,
                |_| panic!("an error has no record"),
            );
            assert_eq!(scan.stop, ScanStop::AttemptLimit);
            assert_eq!(scan.attempts, SCAN_ATTEMPTS);
            assert_eq!((scan.bytes, scan.records), (0, 0));
            assert_eq!(scan.overwritten, overwritten);
        }
    }

    #[test]
    fn both_the_local_budget_and_an_earlier_deadline_stop_reads() {
        let start = Instant::now();
        for allowance in [Duration::from_millis(3), SCAN_BUDGET] {
            let elapsed = Cell::new(Duration::ZERO);
            let scan = scan_records(
                |buffer| {
                    elapsed.set(elapsed.get() + Duration::from_millis(1));
                    copied(buffer, b"6,0,0,-;ok\n")
                },
                if allowance == SCAN_BUDGET {
                    start + Duration::from_secs(10)
                } else {
                    start + allowance
                },
                || start + elapsed.get(),
                |_| {},
            );
            assert_eq!(scan.stop, ScanStop::Deadline);
            assert_eq!(elapsed.get(), allowance);
            assert_eq!(scan.attempts, allowance.as_millis() as usize);
            assert_eq!(scan.records + 1, scan.attempts);
        }
    }

    #[test]
    fn an_expired_deadline_neither_opens_nor_reads_the_log() {
        let now = Instant::now();
        let scan = scan_records(
            |_| panic!("no read after the deadline"),
            now,
            || now,
            |_| panic!("no callback after the deadline"),
        );
        assert_eq!(scan.stop, ScanStop::Deadline);
        assert_eq!((scan.attempts, scan.bytes, scan.records), (0, 0, 0));
        // This path would fail if opened, including on Linux. A deadline
        // returns before platform or path access instead.
        let unopened = each_record("/nonexistent/kmsg", now, |_| panic!("no record"));
        assert_eq!(unopened.unwrap(), scan);
        let result = storage(&read("/nonexistent/kmsg", now));
        assert_eq!(result.verdict, Verdict::NotChecked);
        assert!(result.reason.contains("deadline"));
        assert!(!result.reason.contains("empty"));
    }

    #[test]
    fn a_read_that_returns_at_the_deadline_cannot_claim_completion() {
        let start = Instant::now();
        for would_block in [false, true] {
            let elapsed = Cell::new(Duration::ZERO);
            let scan = scan_records(
                |_| {
                    elapsed.set(SCAN_BUDGET);
                    if would_block {
                        Err(io::ErrorKind::WouldBlock.into())
                    } else {
                        Ok(0)
                    }
                },
                start + SCAN_BUDGET,
                || start + elapsed.get(),
                |_| panic!("no record"),
            );
            assert_eq!(scan.stop, ScanStop::Deadline);
            assert_eq!(scan.attempts, 1);
        }
    }

    #[test]
    fn a_fault_observed_before_time_runs_out_is_kept() {
        let start = Instant::now();
        let elapsed = Cell::new(Duration::ZERO);
        let mut heard = Storage::default();
        let scan = scan_records(
            |buffer| copied(buffer, b"3,0,0,-;I/O error on mmcblk1\n"),
            start + SCAN_BUDGET,
            || start + elapsed.get(),
            |line| {
                heard.count(line);
                elapsed.set(SCAN_BUDGET);
            },
        );
        assert_eq!(scan.stop, ScanStop::Deadline);
        assert_eq!((scan.attempts, scan.records), (1, 1));
        heard.scan = Some(scan);
        let result = storage(&Ok(heard));
        assert_eq!(result.verdict, Verdict::Fail);
        assert!(result.reason.contains("1 I/O error"));
        assert!(result.reason.contains("lower bounds"));
    }

    #[test]
    fn read_errors_keep_earlier_faults_and_never_export_error_text() {
        let start = Instant::now();
        for fault in [false, true] {
            let mut first = true;
            let heard = scanned(
                |buffer| {
                    if std::mem::take(&mut first) {
                        copied(
                            buffer,
                            if fault {
                                b"3,0,0,-;I/O error on mmcblk1\n"
                            } else {
                                b"6,0,0,-;PRIVATE_CANARY\n"
                            },
                        )
                    } else {
                        Err(io::Error::other("PRIVATE_CANARY"))
                    }
                },
                start + SCAN_BUDGET,
                || start,
            );
            assert_eq!(heard.scan.unwrap().stop, ScanStop::ReadError);
            let result = storage(&Ok(heard));
            assert_eq!(
                result.verdict,
                if fault {
                    Verdict::Fail
                } else {
                    Verdict::NotChecked
                }
            );
            assert!(
                !serde_json::to_string(&result)
                    .unwrap()
                    .contains("PRIVATE_CANARY")
            );
            assert!(
                result
                    .values
                    .iter()
                    .any(|value| value.key == "scan_stop" && value.value == "read_error")
            );
        }
    }

    #[test]
    fn a_record_that_does_not_fit_the_fixed_buffer_is_incomplete() {
        let start = Instant::now();
        let record = vec![b'x'; RECORD_BYTES + 1];
        let heard = scanned(
            |buffer| {
                assert_eq!(buffer.len(), RECORD_BYTES);
                // Model a whole-record device rejecting a buffer too small
                // for this record. The reader must not enlarge or retry it.
                if record.len() > buffer.len() {
                    Err(io::ErrorKind::InvalidInput.into())
                } else {
                    copied(buffer, &record)
                }
            },
            start + SCAN_BUDGET,
            || start,
        );
        let scan = heard.scan.unwrap();
        assert_eq!(scan.stop, ScanStop::ReadError);
        assert_eq!((scan.attempts, scan.bytes, scan.records), (1, 0, 0));
        assert_eq!(storage(&Ok(heard)).verdict, Verdict::NotChecked);
    }

    #[test]
    fn an_overwrite_then_end_preserves_observations_but_not_boot_coverage() {
        let start = Instant::now();
        for fault in [false, true] {
            let mut step = 0;
            let heard = scanned(
                |buffer| {
                    step += 1;
                    match step {
                        1 => copied(buffer, b"6,0,0,-;Booting\n"),
                        2 => Err(io::Error::from_raw_os_error(32)),
                        3 => copied(
                            buffer,
                            if fault {
                                b"3,9,9,-;I/O error on mmcblk1\n"
                            } else {
                                b"6,9,9,-;ok\n"
                            },
                        ),
                        _ => Err(io::ErrorKind::WouldBlock.into()),
                    }
                },
                start + SCAN_BUDGET,
                || start,
            );
            assert!(heard.lost);
            assert_eq!(heard.scan.unwrap().stop, ScanStop::Complete);
            assert_eq!(
                storage(&Ok(heard)).verdict,
                if fault {
                    Verdict::Fail
                } else {
                    Verdict::NotChecked
                }
            );
        }
    }

    fn counted(records: &[&str]) -> Storage {
        let mut storage = Storage::default();
        for record in records {
            storage.count(record);
        }
        storage
    }

    /// One hourly round of lamp-52e6's on 17 Oct (`dmesg`), at `at_us`
    /// for the first host and a second later for the other.
    fn round(at_us: u64, seq: u64) -> Vec<String> {
        let host1 = "sunxi:sunxi_mmc_host-4021000.sdmmc";
        let host0 = "sunxi:sunxi_mmc_host-4020000.sdmmc";
        vec![
            format!(
                "6,{seq},{at_us},-;{host1}:[INFO]: start tuning, tuning clk = 200000000  opcode=19"
            ),
            format!(
                "3,{},{},-;{host1}:[ERR]: wait dma hold bit clear timeout",
                seq + 1,
                at_us + 19_000
            ),
            format!(
                "3,{},{},-;{host1}:[ERR]: wait dma hold bit clear timeout",
                seq + 2,
                at_us + 44_000
            ),
            format!(
                "6,{},{},-;{host1}:[INFO]: tuning result: 23 - 59,  best: 41",
                seq + 3,
                at_us + 60_000
            ),
            format!(
                "6,{},{},-;{host0}:[INFO]: start tuning, tuning clk = 150000000  opcode=19",
                seq + 4,
                at_us + 1_000_000
            ),
            format!(
                "3,{},{},-;{host0}:[ERR]: wait dma hold bit clear timeout",
                seq + 5,
                at_us + 1_019_000
            ),
        ]
    }

    fn heard_all(records: &[String]) -> Storage {
        let mut storage = Storage::default();
        for record in records {
            storage.count(record);
        }
        storage
    }

    #[test]
    fn a_quiet_kernel_passes() {
        let quiet = counted(&[
            "6,0,0,-;Booting Linux on physical CPU 0x0",
            " SUBSYSTEM=platform",
            "6,1,15,-;mmc0: new high speed SDHC card at address aaaa",
            "6,2,16,-;EXT4-fs (mmcblk1p1): re-mounted. Opts: commit=600,errors=remount-ro",
        ]);
        assert_eq!(quiet.records, 3);
        let result = storage(&Ok(quiet));
        assert_eq!(result.verdict, Verdict::Pass, "{result:?}");
    }

    #[test]
    fn hourly_retunes_with_dma_timeouts_pass_and_are_counted() {
        let mut records = vec!["6,0,0,-;Booting Linux on physical CPU 0x0".to_owned()];
        // 124816.284246 s, then 3,669.83 s later, as lamp-52e6.
        records.extend(round(124_816_284_246, 1));
        records.extend(round(128_486_117_881, 7));
        let heard = heard_all(&records);
        assert_eq!((heard.retunes, heard.dma_timeouts), (4, 6));
        assert!(
            heard
                .shortest_retune_gap_s
                .is_some_and(|gap| (gap - 3_669.83).abs() < 0.01)
        );
        let result = storage(&Ok(heard));
        assert_eq!(result.verdict, Verdict::Pass, "{result:?}");
        assert_eq!(result.reason, "");
        assert!(
            result
                .values
                .iter()
                .any(|v| v.key == "retunes" && v.value == "4")
        );
    }

    #[test]
    fn retunes_more_often_than_hourly_pass_and_say_so() {
        let mut records = vec!["6,0,0,-;Booting Linux on physical CPU 0x0".to_owned()];
        records.extend(round(124_816_284_246, 1));
        records.extend(round(124_816_284_246 + 1_200_000_000, 7));
        let result = storage(&Ok(heard_all(&records)));
        assert_eq!(result.verdict, Verdict::Pass, "{result:?}");
        assert!(
            result.reason.contains("more often than hourly"),
            "{}",
            result.reason
        );
        assert!(result.reason.contains("1200 s"), "{}", result.reason);
    }

    #[test]
    fn a_log_that_lost_the_start_of_the_boot_is_not_checked_unless_a_fault_is_kept() {
        let late = counted(&[
            "6,10000,9000000,-;usb 3-1: new device",
            "6,10001,9000100,-;ok",
        ]);
        assert!(late.lost);
        assert_eq!(late.first_sequence, Some(10_000));
        let result = storage(&Ok(late));
        assert_eq!(result.verdict, Verdict::NotChecked, "{result:?}");
        assert!(result.reason.contains("10000"), "{}", result.reason);
        let overwritten = Storage {
            records: 1,
            lost: true,
            ..Storage::default()
        };
        assert_eq!(storage(&Ok(overwritten)).verdict, Verdict::NotChecked);
        // lamp-52e6's ring on 17 Oct began at 119018 s: re-tunes kept are
        // no fault, and the boot is still not checked.
        let mut retuned = vec!["6,4100,119018110579,-;x".to_owned()];
        retuned.extend(round(124_816_284_246, 4101));
        assert_eq!(
            storage(&Ok(heard_all(&retuned))).verdict,
            Verdict::NotChecked
        );
        let faulty = counted(&[
            "6,10000,1,-;x",
            "3,10001,2,-;blk_update_request: I/O error, dev mmcblk1, sector 2048 op 0x1:(WRITE) flags 0x800 phys_seg 1 prio class 0",
        ]);
        assert_eq!(storage(&Ok(faulty)).verdict, Verdict::Fail);
    }

    /// Each line the kernel writes for a fault that hurts, as its source
    /// prints it, fails on its own beside an hourly re-tune.
    #[test]
    fn each_fault_that_hurts_fails() {
        for (line, says) in [
            (
                "3,9,1,-;blk_update_request: I/O error, dev mmcblk1, sector 2048 op 0x1:(WRITE) flags 0x800 phys_seg 1 prio class 0",
                "1 I/O error on the card",
            ),
            (
                "3,9,1,-;Buffer I/O error on dev mmcblk1p1, logical block 0, lost sync page write",
                "1 I/O error on the card",
            ),
            (
                "2,9,1,-;EXT4-fs error (device mmcblk1p1): ext4_journal_check_start:83: comm systemd-journal: Detected aborted journal",
                "1 ext4 error on the card",
            ),
            (
                "2,9,1,-;EXT4-fs (mmcblk1p1): Remounting filesystem read-only",
                "read-only",
            ),
            (
                "3,9,1,-;INFO: task jbd2/mmcblk1p1-8:187 blocked for more than 120 seconds.",
                "1 task blocked on the card",
            ),
        ] {
            let mut records = vec!["6,0,0,-;Booting".to_owned(), line.to_owned()];
            records.extend(round(124_816_284_246, 10));
            let result = storage(&Ok(heard_all(&records)));
            assert_eq!(result.verdict, Verdict::Fail, "{line}");
            assert!(result.reason.contains(says), "{line}: {}", result.reason);
        }
    }

    /// A fault line that names another device than the card (a USB
    /// stick's `sda1`, an audio task hung after a hub drop) passes.
    #[test]
    fn faults_off_the_card_pass() {
        let heard = counted(&[
            "6,0,0,-;Booting",
            "2,9,1,-;EXT4-fs error (device sda1): ext4_find_entry:1455: inode #2: comm ls: reading directory lblock 0",
            "2,10,2,-;EXT4-fs (sda1): Remounting filesystem read-only",
            "3,11,3,-;blk_update_request: I/O error, dev sda, sector 2048 op 0x0:(READ) flags 0x0 phys_seg 1 prio class 0",
            "3,12,4,-;INFO: task lamp-voice:812 blocked for more than 120 seconds.",
        ]);
        assert_eq!(heard.blocked_other, 1);
        let result = storage(&Ok(heard));
        assert_eq!(result.verdict, Verdict::Pass, "{result:?}");
        assert!(
            result
                .values
                .iter()
                .any(|v| v.key == "blocked_other" && v.value == "1")
        );
    }

    #[test]
    fn many_faults_are_counted_in_the_plural() {
        let line = "3,9,1,-;blk_update_request: I/O error, dev mmcblk1, sector 2048 op 0x1:(WRITE) flags 0x800 phys_seg 1 prio class 0";
        let result = storage(&Ok(counted(&["6,0,0,-;Booting", line, line])));
        assert_eq!(result.reason, "2 I/O errors on the card since boot");
    }

    /// A ring that lost the boot's start is not checked even when what it
    /// kept re-tunes often; the gap stays in the reason.
    #[test]
    fn a_lost_log_with_frequent_retunes_is_not_checked_and_keeps_the_gap() {
        let mut records = vec!["6,4100,119018110579,-;x".to_owned()];
        records.extend(round(124_816_284_246, 4101));
        records.extend(round(124_816_284_246 + 1_200_000_000, 4107));
        let result = storage(&Ok(heard_all(&records)));
        assert_eq!(result.verdict, Verdict::NotChecked, "{result:?}");
        assert!(result.reason.contains("1200 s apart"), "{}", result.reason);
    }

    #[test]
    fn a_log_that_cannot_be_read_or_is_empty_is_not_checked() {
        let denied = storage(&Err("Permission denied (os error 13)".into()));
        assert_eq!(denied.verdict, Verdict::NotChecked);
        assert!(denied.reason.contains("Permission denied"));
        let empty = storage(&Ok(Storage::default()));
        assert_eq!(empty.verdict, Verdict::NotChecked);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_missing_log_is_an_error_and_a_plain_file_is_read_whole() {
        assert!(read("/nonexistent/kmsg", Instant::now() + SCAN_BUDGET).is_err());
        let path = std::env::temp_dir().join(format!("lamp-selftest-kmsg-{}", std::process::id()));
        std::fs::write(
            &path,
            "6,1,0,-;Booting\n3,2,1,-;x: wait dma hold bit clear timeout\n",
        )
        .unwrap();
        let heard = read(path.to_str().unwrap(), Instant::now() + SCAN_BUDGET).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!((heard.records, heard.dma_timeouts), (2, 1));
    }
}
