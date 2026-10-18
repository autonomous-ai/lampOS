//! `LAMP_CAPTURE_CORES` (#705, 18 Oct): the cores the capture worker runs on.
//! On lamp-4ace the worker took 17 % of a small core (0-5, 1794 MHz) and
//! 7 % of a big one (6-7, 2002 MHz); unset, the scheduler picks from all
//! eight. Linux only, and never a reason to stop the voice.
use std::collections::BTreeSet;

pub const ENVIRONMENT: &str = "LAMP_CAPTURE_CORES";

/// A kernel cpu-list ("6-7", "6,7", "6", "0-2,6") as its cores; `None` for
/// anything else, the empty string included.
pub fn parse(list: &str) -> Option<BTreeSet<usize>> {
    let mut cores = BTreeSet::new();
    for part in list.trim().split(',') {
        let (first, last) = part.split_once('-').unwrap_or((part, part));
        let (first, last): (usize, usize) = (first.parse().ok()?, last.parse().ok()?);
        // 1024 is the kernel's `cpu_set_t`.
        if first > last || last >= 1024 {
            return None;
        }
        cores.extend(first..=last);
    }
    Some(cores)
}

/// Pins the calling thread (and the threads it starts afterwards) to the
/// cores this worker's environment names, logging one line. Unset, nothing.
pub fn apply_from_environment() {
    let value = std::env::var_os(ENVIRONMENT).map(|value| value.to_string_lossy().into_owned());
    if let Some(line) = apply(value.as_deref()) {
        eprintln!("lamp-voice: capture: {line}");
    }
}

/// What pinning to `value` did, as the line to log; `None`, and nothing
/// touched, when the variable is unset. Any fault keeps the default cores.
pub fn apply(value: Option<&str>) -> Option<String> {
    let value = value?;
    let Some(cores) = parse(value) else {
        return Some(format!(
            "{ENVIRONMENT}={value:?} is not a cpu list; keeping the default cores"
        ));
    };
    let online = std::fs::read_to_string("/sys/devices/system/cpu/online")
        .ok()
        .and_then(|online| parse(&online));
    if let Some(online) = online
        && !cores.is_subset(&online)
    {
        return Some(format!(
            "{ENVIRONMENT}={value}: cores {} are not all online ({}); keeping the default cores",
            text(&cores),
            text(&online)
        ));
    }
    Some(match pin(&cores) {
        Ok(back) => format!(
            "pinned to cores {} ({ENVIRONMENT}); read back {}: {}",
            text(&cores),
            text(&back),
            if back == cores {
                "it took"
            } else {
                "it did not take"
            }
        ),
        Err(error) => format!("{ENVIRONMENT}={value}: {error}; keeping the default cores"),
    })
}

fn text(cores: &BTreeSet<usize>) -> String {
    cores
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// Sets this thread's affinity and reads it back.
#[cfg(target_os = "linux")]
fn pin(cores: &BTreeSet<usize>) -> Result<BTreeSet<usize>, String> {
    use nix::sched::{CpuSet, sched_getaffinity, sched_setaffinity};
    use nix::unistd::Pid;
    let mut set = CpuSet::new();
    for &core in cores {
        set.set(core)
            .map_err(|error| format!("core {core}: {error}"))?;
    }
    // Pid 0 is the calling thread.
    let this = Pid::from_raw(0);
    sched_setaffinity(this, &set).map_err(|error| format!("sched_setaffinity: {error}"))?;
    let back = sched_getaffinity(this).map_err(|error| format!("sched_getaffinity: {error}"))?;
    Ok((0..CpuSet::count())
        .filter(|&core| back.is_set(core).unwrap_or(false))
        .collect())
}

#[cfg(not(target_os = "linux"))]
fn pin(_: &BTreeSet<usize>) -> Result<BTreeSet<usize>, String> {
    Err("core affinity is Linux's".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_lists_parse() {
        let set = |cores: &[usize]| cores.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(parse("6-7"), Some(set(&[6, 7])));
        assert_eq!(parse("6,7"), Some(set(&[6, 7])));
        assert_eq!(parse("6"), Some(set(&[6])));
        assert_eq!(parse("0-2,6\n"), Some(set(&[0, 1, 2, 6])));
        for bad in [
            "", " ", "7-6", "six", "6-", ",6", "6,,7", "-1", "1024", "6 7",
        ] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn unset_applies_nothing_and_a_bad_value_keeps_the_default() {
        assert_eq!(apply(None), None);
        for bad in ["big", "", "1023"] {
            let line = apply(Some(bad)).unwrap();
            assert!(line.contains("keeping the default cores"), "{line}");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn core_0_is_applied_to_a_thread_and_read_back() {
        // Its own thread: affinity is per thread, and the test's own stays.
        let line = std::thread::spawn(|| {
            let line = apply(Some("0")).unwrap();
            let back = nix::sched::sched_getaffinity(nix::unistd::Pid::from_raw(0)).unwrap();
            assert!(back.is_set(0).unwrap() && !back.is_set(1).unwrap_or(false));
            line
        })
        .join()
        .unwrap();
        assert_eq!(
            line,
            "pinned to cores 0 (LAMP_CAPTURE_CORES); read back 0: it took"
        );
    }
}
