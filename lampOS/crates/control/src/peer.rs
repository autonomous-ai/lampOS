//! Who a unix socket's caller is, by its process: the cgroup its pid is in,
//! read from `/proc` (`SO_PEERCRED` gives the pid; docs/control.md, "The
//! local agent" and "Owner's yes records").
//!
//! The pid is read after the accept, so a pid recycled in between would be
//! misread. `SO_PEERPIDFD` closes that, but it needs a 6.5 kernel and the
//! lamps run 5.15. So the process's start time (`/proc/<pid>/stat`, field 22,
//! in clock ticks since boot) is read before and after its cgroup, and the
//! cgroup is taken only when the two match and the process started no later
//! than the time of the accept, read from `/proc/uptime` just after it. That
//! narrows the window and does not close it: a recycled pid whose process
//! started between the connection and that reading still passes (and needs a
//! full wrap of the pid space). Anything unreadable refuses; nothing here
//! ever allows on a failed read.
use std::fs;

/// Clock ticks a second in `/proc/<pid>/stat` (`USER_HZ`, 100 on Linux).
pub const TICKS_A_SECOND: u64 = 100;

/// The start time in field 22 of a `/proc/<pid>/stat` line, in clock ticks
/// since boot. The name in field 2 is in parentheses and may hold spaces and
/// parentheses of its own, so the fields are counted after the last `)`.
pub fn start_ticks(stat: &str) -> Option<u64> {
    let after = &stat[stat.rfind(')')? + 1..];
    // After the name: field 3 (the state) first, so field 22 is the 20th.
    after.split_whitespace().nth(19)?.parse().ok()
}

/// The time since boot in `/proc/uptime` (`"12345.67 8910.11"`), in clock ticks.
pub fn boot_ticks(uptime: &str) -> Option<u64> {
    let first = uptime.split_whitespace().next()?;
    let (seconds, hundredths) = first.split_once('.')?;
    let hundredths: String = hundredths.chars().chain("00".chars()).take(2).collect();
    let (seconds, hundredths): (u64, u64) = (seconds.parse().ok()?, hundredths.parse().ok()?);
    seconds.checked_mul(TICKS_A_SECOND)?.checked_add(hundredths)
}

/// The cgroup text of `pid`, only if the process is the one that connected:
/// its start time is no later than `accepted` (the time since boot, in
/// ticks, read right after the accept) and is the same before and after the
/// cgroup is read. `read` gives a `/proc` file's text, or none.
pub fn cgroup_checked(
    pid: i32,
    accepted: u64,
    read: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    let stat = format!("/proc/{pid}/stat");
    let started = start_ticks(&read(&stat)?)?;
    if started > accepted {
        return None;
    }
    let cgroup = read(&format!("/proc/{pid}/cgroup"))?;
    (start_ticks(&read(&stat)?)? == started).then_some(cgroup)
}

/// The cgroup of the peer `pid` (from `SO_PEERCRED`), checked as
/// [`cgroup_checked`] does. Call it straight after the accept: it takes the
/// time of the accept from its own first line.
pub fn cgroup_of(pid: Option<i32>) -> Option<String> {
    cgroup_with(pid, &|path: &str| fs::read_to_string(path).ok())
}

/// [`cgroup_of`] over `read`, a `/proc` file's text or none: the pid must be
/// a real one (above 0), and the time of the accept is `read`'s first file.
pub fn cgroup_with(pid: Option<i32>, read: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let accepted = boot_ticks(&read("/proc/uptime")?)?;
    cgroup_checked(pid.filter(|pid| *pid > 0)?, accepted, read)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// A `/proc/<pid>/stat` line whose name is `comm`, with the start time
    /// `start` in field 22 (the fields between are zeros).
    fn stat(comm: &str, start: u64) -> String {
        // Fields 3 to 9 are written out; 10 to 21 are zeros; field 22 is `start`.
        let zeros = ["0"; 12].join(" ");
        format!("4242 ({comm}) S 1 4242 4242 0 -1 4194560 {zeros} {start} 0 0\n")
    }

    #[test]
    fn the_start_time_is_field_22_whatever_the_name_holds() {
        assert_eq!(start_ticks(&stat("lamp-voice", 5_000)), Some(5_000));
        // Spaces and parentheses in the name, even a closing one.
        assert_eq!(start_ticks(&stat("a b) c", 77)), Some(77));
        assert_eq!(start_ticks(&stat("(x) (y)", 78)), Some(78));
        assert_eq!(start_ticks(&stat("", 79)), Some(79));
        // Not a stat line: no `)`, too few fields, a start that is no number.
        assert_eq!(start_ticks(""), None);
        assert_eq!(start_ticks("4242 lamp S 1"), None);
        assert_eq!(start_ticks("4242 (lamp) S 1 2 3"), None);
        assert_eq!(
            start_ticks(&stat("lamp", 1).replace(" 1 0 0\n", " x 0 0\n")),
            None
        );
    }

    #[test]
    fn the_time_since_boot_is_read_in_ticks() {
        assert_eq!(boot_ticks("12345.67 8910.11\n"), Some(1_234_567));
        assert_eq!(boot_ticks("0.05 0.00"), Some(5));
        assert_eq!(boot_ticks("7.5 1.0"), Some(750));
        assert_eq!(boot_ticks(""), None);
        assert_eq!(boot_ticks("soon"), None);
        assert_eq!(boot_ticks("12 3"), None);
    }

    const VOICE_CGROUP: &str = "0::/lampos.slice/lampos-voice.slice/lamp-voice.service\n";

    /// The `/proc` of a pid whose start time is `starts[n]` at its n-th stat
    /// read, with `cgroup` as its cgroup; `None` for a file that is missing.
    fn proc_of<'a>(
        starts: &'a [Option<u64>],
        cgroup: Option<&'a str>,
    ) -> impl Fn(&str) -> Option<String> + 'a {
        let reads = Cell::new(0);
        move |path: &str| {
            if path.ends_with("/stat") {
                let n = reads.get();
                reads.set(n + 1);
                starts
                    .get(n)
                    .copied()
                    .flatten()
                    .map(|start| stat("lamp-voice", start))
            } else if path.ends_with("/cgroup") {
                cgroup.map(str::to_owned)
            } else {
                None
            }
        }
    }

    #[test]
    fn a_process_that_started_before_the_accept_and_stayed_the_same_is_read() {
        let read = proc_of(&[Some(900), Some(900)], Some(VOICE_CGROUP));
        assert_eq!(
            cgroup_checked(4242, 1_000, &read).as_deref(),
            Some(VOICE_CGROUP)
        );
        // Started in the very tick of the accept: still the connecting one.
        let read = proc_of(&[Some(1_000), Some(1_000)], Some(VOICE_CGROUP));
        assert!(cgroup_checked(4242, 1_000, &read).is_some());
    }

    #[test]
    fn a_pid_whose_start_time_changed_between_the_reads_is_refused() {
        // The pid was recycled while its cgroup was read.
        let read = proc_of(&[Some(900), Some(950)], Some(VOICE_CGROUP));
        assert_eq!(cgroup_checked(4242, 1_000, &read), None);
    }

    #[test]
    fn a_process_that_started_after_the_accept_is_refused() {
        // A recycled pid: the process that connected is gone, and this one
        // started later.
        let read = proc_of(&[Some(1_001), Some(1_001)], Some(VOICE_CGROUP));
        assert_eq!(cgroup_checked(4242, 1_000, &read), None);
    }

    #[test]
    fn anything_unreadable_refuses_and_never_allows() {
        // No stat at all, no cgroup, an unreadable second stat, a garbled one.
        for (starts, cgroup) in [
            (vec![None], Some(VOICE_CGROUP)),
            (vec![Some(900), Some(900)], None),
            (vec![Some(900), None], Some(VOICE_CGROUP)),
        ] {
            let read = proc_of(&starts, cgroup);
            assert_eq!(cgroup_checked(4242, 1_000, &read), None, "{starts:?}");
        }
        let garbled = |path: &str| path.ends_with("/stat").then(|| "garbled".to_owned());
        assert_eq!(cgroup_checked(4242, 1_000, &garbled), None);
        assert_eq!(cgroup_of(None), None);
        assert_eq!(cgroup_of(Some(0)), None);
        assert_eq!(cgroup_of(Some(-3)), None);
    }

    /// A `/proc` that answers for any pid: the uptime, a stat that starts at
    /// `start` (read twice the same), and `cgroup`.
    fn any_pid(start: u64, cgroup: &'static str) -> impl Fn(&str) -> Option<String> {
        move |path: &str| {
            if path == "/proc/uptime" {
                Some("10.00 5.00\n".to_owned())
            } else if path.ends_with("/stat") {
                Some(stat("lamp-voice", start))
            } else if path.ends_with("/cgroup") {
                Some(cgroup.to_owned())
            } else {
                None
            }
        }
    }

    #[test]
    fn a_pid_at_or_below_zero_is_never_read_whatever_proc_would_answer() {
        let read = any_pid(900, VOICE_CGROUP);
        assert!(
            cgroup_with(Some(4242), &read).is_some(),
            "a real pid is read"
        );
        for pid in [None, Some(0), Some(-1), Some(-3)] {
            assert_eq!(cgroup_with(pid, &read), None, "{pid:?}");
        }
    }

    #[test]
    fn peer_is_voice_goes_through_this_check() {
        use crate::task_yes::peer_is_voice_with;
        // The voice's cgroup, a start before the accept (10.00 s = 1000 ticks).
        assert!(peer_is_voice_with(Some(4242), &any_pid(900, VOICE_CGROUP)));
        // The same process, started after the accept: not the voice, however
        // right its cgroup.
        assert!(!peer_is_voice_with(
            Some(4242),
            &any_pid(1_001, VOICE_CGROUP)
        ));
        // Another unit's cgroup, or a pid below 1: not the voice.
        assert!(!peer_is_voice_with(
            Some(4242),
            &any_pid(
                900,
                "0::/lampos.slice/lampos-parts.slice/lamp-skills.service\n"
            )
        ));
        assert!(!peer_is_voice_with(Some(0), &any_pid(900, VOICE_CGROUP)));
        assert!(!peer_is_voice_with(None, &any_pid(900, VOICE_CGROUP)));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn this_process_is_read_from_the_real_proc() {
        let me = i32::try_from(std::process::id()).unwrap();
        let cgroup = cgroup_of(Some(me)).expect("this process's own cgroup");
        assert!(cgroup.contains("::") || cgroup.contains(':'), "{cgroup}");
    }
}
