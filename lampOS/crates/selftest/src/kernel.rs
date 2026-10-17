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
use std::{collections::HashMap, time::Instant};

/// The kernel's ring of messages since boot.
pub const KMSG: &str = "/dev/kmsg";

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
    if storage.records == 0 {
        return PartResult::new(part, Verdict::NotChecked, "the kernel's log was empty");
    }
    let often = storage
        .shortest_retune_gap_s
        .filter(|gap| *gap < RETUNE_OFTEN_S);
    let result = if let Some(fault) = storage.fault() {
        PartResult::new(part, Verdict::Fail, &fault)
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
    if storage.lost {
        result.with("oldest_lost", "yes")
    } else {
        result
    }
}

/// Every record in the kernel's ring now.
pub fn read(path: &str) -> Result<Storage, String> {
    let mut storage = Storage::default();
    if each_record(path, |line| storage.count(line))? {
        storage.lost = true;
    }
    Ok(storage)
}

/// Hands each line of the kernel's ring now to `line`; true if the ring
/// overwrote records while they were read.
#[cfg(target_os = "linux")]
pub fn each_record(path: &str, mut line: impl FnMut(&str)) -> Result<bool, String> {
    use std::{
        fs::OpenOptions,
        io::{ErrorKind, Read},
        os::unix::fs::OpenOptionsExt,
    };
    // O_NONBLOCK: a read at the end of the ring gives WouldBlock, not a wait.
    const O_NONBLOCK: i32 = 0o4000;
    let mut kmsg = OpenOptions::new()
        .read(true)
        .custom_flags(O_NONBLOCK)
        .open(path)
        .map_err(|e| e.to_string())?;
    let mut overwritten = false;
    // Each read gives one whole record; the kernel caps one at 8 KiB.
    let mut buffer = vec![0u8; 8192];
    loop {
        match kmsg.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                for each in String::from_utf8_lossy(&buffer[..n]).lines() {
                    line(each);
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            // EPIPE: the ring overwrote records before they were read.
            Err(e) if e.raw_os_error() == Some(32) => overwritten = true,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(overwritten)
}

/// Only Linux keeps `/dev/kmsg`.
#[cfg(not(target_os = "linux"))]
pub fn each_record(_path: &str, _line: impl FnMut(&str)) -> Result<bool, String> {
    Err("only Linux has a kernel log to read".into())
}

/// Reads the kernel's log and judges the storage.
pub struct Kernel {
    pub path: String,
}

impl Check for Kernel {
    fn part(&self) -> &str {
        "storage"
    }

    fn run(&mut self, _deadline: Instant) -> PartResult {
        storage(&read(&self.path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(read("/nonexistent/kmsg").is_err());
        let path = std::env::temp_dir().join(format!("lamp-selftest-kmsg-{}", std::process::id()));
        std::fs::write(
            &path,
            "6,1,0,-;Booting\n3,2,1,-;x: wait dma hold bit clear timeout\n",
        )
        .unwrap();
        let heard = read(path.to_str().unwrap()).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!((heard.records, heard.dma_timeouts), (2, 1));
    }
}
