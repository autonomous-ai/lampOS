//! `LAMP_CAPTURE_CORES` (#705, 18 Oct): the cores the capture worker runs on.
//! On lamp-4ace the worker took 17 % of a small core (0-5, 1794 MHz) and
//! 7 % of a big one (6-7, 2002 MHz); unset, the scheduler picks from all
//! eight. Linux only, and never a reason to stop the voice.
use std::collections::BTreeSet;

pub const ENVIRONMENT: &str = lamp_turn_end_wire::cores::CAPTURE_CORES;

use lamp_turn_end_wire::cores::beside_capture;
pub use lamp_turn_end_wire::cores::parse;

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
    apply_with(value, lamp_turn_end_wire::cores::fast, get, set)
}

/// `apply` over the thread's affinity as `get` and `set` see it, `set`
/// returning what reads back after it, beside the machine's `fast` cores
/// (read only when the variable is set); the tests fake a narrowing cpuset
/// and the fast cores.
fn apply_with(
    value: Option<&str>,
    fast: impl FnOnce() -> Option<Vec<usize>>,
    get: impl Fn() -> Result<BTreeSet<usize>, String>,
    set: impl Fn(&BTreeSet<usize>) -> Result<BTreeSet<usize>, String>,
) -> Option<String> {
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
    // The fast end-of-turn worker runs at RR 20 on the fast cores and leaves
    // capture's to it; a list taking all of them would leave it none, and
    // capture held there stalled 99 ms and the voice died (#1459).
    if let Some(fast) = fast()
        && beside_capture(&fast, &cores).is_none()
    {
        let fast: BTreeSet<usize> = fast.into_iter().collect();
        return Some(format!(
            "{ENVIRONMENT}={value}: cores {} take every one of the end-of-turn worker's \
             fast cores ({}), which run at RR 20 too; keeping the default cores",
            text(&cores),
            text(&fast)
        ));
    }
    let fault =
        |error: String| format!("{ENVIRONMENT}={value}: {error}; keeping the default cores");
    let before = match get() {
        Ok(before) => before,
        Err(error) => return Some(fault(error)),
    };
    let back = match set(&cores) {
        Ok(back) => back,
        Err(error) => return Some(fault(error)),
    };
    if back == cores {
        return Some(format!(
            "pinned to cores {} ({ENVIRONMENT}); read back {}: it took",
            text(&cores),
            text(&back)
        ));
    }
    // A cpuset allowing part of the list has narrowed the thread already:
    // set back the mask from before, so the default is really kept.
    let restored = match set(&before) {
        Ok(again) if again == before => format!("restored cores {}", text(&again)),
        Ok(again) => format!(
            "restoring cores {} read back {}",
            text(&before),
            text(&again)
        ),
        Err(error) => format!("restoring cores {}: {error}", text(&before)),
    };
    Some(format!(
        "pinned to cores {} ({ENVIRONMENT}); read back {}: it did not take; {restored}",
        text(&cores),
        text(&back)
    ))
}

fn text(cores: &BTreeSet<usize>) -> String {
    cores
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// This thread's affinity.
#[cfg(target_os = "linux")]
fn get() -> Result<BTreeSet<usize>, String> {
    use nix::sched::{CpuSet, sched_getaffinity};
    // Pid 0 is the calling thread.
    let back = sched_getaffinity(nix::unistd::Pid::from_raw(0))
        .map_err(|error| format!("sched_getaffinity: {error}"))?;
    Ok((0..CpuSet::count())
        .filter(|&core| back.is_set(core).unwrap_or(false))
        .collect())
}

/// Sets this thread's affinity and reads it back.
#[cfg(target_os = "linux")]
fn set(cores: &BTreeSet<usize>) -> Result<BTreeSet<usize>, String> {
    use nix::sched::{CpuSet, sched_setaffinity};
    let mut mask = CpuSet::new();
    for &core in cores {
        mask.set(core)
            .map_err(|error| format!("core {core}: {error}"))?;
    }
    sched_setaffinity(nix::unistd::Pid::from_raw(0), &mask)
        .map_err(|error| format!("sched_setaffinity: {error}"))?;
    get()
}

#[cfg(not(target_os = "linux"))]
fn get() -> Result<BTreeSet<usize>, String> {
    Err("core affinity is Linux's".to_owned())
}

#[cfg(not(target_os = "linux"))]
fn set(_: &BTreeSet<usize>) -> Result<BTreeSet<usize>, String> {
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
        // Unset, not even the fast cores are read.
        let read = || -> Option<Vec<usize>> { panic!("the fast cores were read") };
        assert_eq!(apply_with(None, read, get, set), None);
        for bad in ["big", "", "1023"] {
            let line = apply(Some(bad)).unwrap();
            assert!(line.contains("keeping the default cores"), "{line}");
        }
    }

    #[test]
    fn a_cpuset_allowing_part_of_the_list_gets_the_mask_from_before_back() {
        use std::cell::RefCell;
        let set_of = |cores: &[usize]| cores.iter().copied().collect::<BTreeSet<_>>();
        // A cpuset allowing core 0 only: asked for 0 and 1, the thread is
        // held to 0. Cores 0 and 1 are online on any machine the tests run on.
        let allowed = set_of(&[0]);
        let mask = RefCell::new(allowed.clone());
        let line = apply_with(
            Some("0,1"),
            || Some(vec![6, 7]),
            || Ok(mask.borrow().clone()),
            |cores| {
                let narrowed: BTreeSet<_> = cores.intersection(&allowed).copied().collect();
                *mask.borrow_mut() = narrowed.clone();
                Ok(narrowed)
            },
        )
        .unwrap();
        assert_eq!(
            line,
            "pinned to cores 0,1 (LAMP_CAPTURE_CORES); read back 0: it did not take; \
             restored cores 0"
        );
        assert_eq!(*mask.borrow(), allowed);
    }

    #[test]
    fn a_list_taking_every_fast_core_is_refused_before_anything_is_touched() {
        let touched = std::cell::Cell::new(false);
        let get = || {
            touched.set(true);
            Ok(BTreeSet::new())
        };
        let set = |_: &BTreeSet<usize>| {
            touched.set(true);
            Ok(BTreeSet::new())
        };
        // Fast cores 0 and 1, online on any machine the tests run on.
        for list in ["0-1", "0,1"] {
            let line = apply_with(Some(list), || Some(vec![0, 1]), get, set).unwrap();
            assert!(
                line.contains("end-of-turn worker's fast cores (0,1)")
                    && line.ends_with("keeping the default cores"),
                "{line}"
            );
        }
        assert!(!touched.get());
    }

    #[test]
    fn one_fast_core_for_capture_leaves_the_other_and_is_pinned() {
        let line = apply_with(
            Some("1"),
            || Some(vec![0, 1]),
            || Ok([0, 1].into()),
            |cores| Ok(cores.clone()),
        )
        .unwrap();
        assert_eq!(
            line,
            "pinned to cores 1 (LAMP_CAPTURE_CORES); read back 1: it took"
        );
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
