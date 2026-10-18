//! Which cores are the fast ones, shared by `lamp-turn-end` (which keeps its
//! fast model there) and the voice (which keeps its capture worker off them).
//!
//! Both run at the voice unit's `RR 20`. Held to the same cores, the one
//! that waits can wait a whole RR slice (100 ms), longer than the 40 ms the
//! microphone buffers: on lamp-4ace (#1459, 18 Oct) capture held to 6-7
//! beside the fast model stalled 99 ms and the voice died. So the capture
//! cores a person names are taken from the fast model's, and a list that
//! would take all of them is refused.
use std::{collections::BTreeSet, path::Path};

/// The environment variable naming the capture worker's cores; the voice
/// passes it to the fast end-of-turn worker as well.
pub const CAPTURE_CORES: &str = "LAMP_CAPTURE_CORES";

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

/// This machine's fast cores, from `/sys/devices/system/cpu`; `None` when
/// the cores do not differ.
pub fn fast() -> Option<Vec<usize>> {
    fastest(&capacities(Path::new("/sys/devices/system/cpu")))
}

/// The fast cores the end-of-turn worker keeps beside a capture worker on
/// `capture`: the ones capture leaves free, or `None` when it leaves none.
/// `None` is the case the capture worker refuses, so the end-of-turn worker
/// then keeps every fast core.
pub fn beside_capture(fast: &[usize], capture: &BTreeSet<usize>) -> Option<Vec<usize>> {
    let left: Vec<usize> = fast
        .iter()
        .copied()
        .filter(|core| !capture.contains(core))
        .collect();
    (!left.is_empty()).then_some(left)
}

/// The fast cores the end-of-turn worker keeps and the ONNX threads it runs,
/// from the machine's `fast` cores, the `capture` list the voice passed, the
/// cores this process may run on (`allowed`: the online cores within the
/// unit's cpuset, which the capture worker shares; `None` when unknown) and
/// the `threads` asked. It leaves capture's cores only when capture takes
/// them, a cpu list within `allowed` that leaves a fast core, and then runs
/// one thread a kept core. Otherwise, unset or refused, it keeps every fast
/// core and the threads asked.
pub fn kept(
    fast: &[usize],
    capture: Option<&str>,
    allowed: Option<&BTreeSet<usize>>,
    threads: usize,
) -> (Vec<usize>, usize) {
    let left = capture
        .and_then(parse)
        .filter(|capture| allowed.is_none_or(|allowed| capture.is_subset(allowed)))
        .and_then(|capture| beside_capture(fast, &capture))
        .filter(|left| left.len() < fast.len());
    match left {
        Some(left) => {
            let threads = threads.min(left.len());
            (left, threads)
        }
        None => (fast.to_vec(), threads),
    }
}

/// Each online core's number and its capacity, from a sysfs CPU directory
/// (normally `/sys/devices/system/cpu`). Falls back to the highest frequency
/// a core can run at when the kernel reports no capacity.
pub fn capacities(cpu_dir: &Path) -> Vec<(usize, u64)> {
    let Ok(entries) = std::fs::read_dir(cpu_dir) else {
        return Vec::new();
    };
    let read = |path: std::path::PathBuf| {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| text.trim().parse::<u64>().ok())
    };
    let mut cores = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name();
            let cpu = name.to_str()?.strip_prefix("cpu")?.parse::<usize>().ok()?;
            let path = entry.path();
            // cpu0 often has no `online` file; a 0 in it means offline.
            if read(path.join("online")) == Some(0) {
                return None;
            }
            read(path.join("cpu_capacity"))
                .or_else(|| read(path.join("cpufreq/cpuinfo_max_freq")))
                .map(|capacity| (cpu, capacity))
        })
        .collect::<Vec<_>>();
    cores.sort_unstable();
    cores
}

/// The cores with the largest capacity, when the cores differ; otherwise
/// `None`.
pub fn fastest(cores: &[(usize, u64)]) -> Option<Vec<usize>> {
    let top = cores.iter().map(|&(_, capacity)| capacity).max()?;
    let fast = cores
        .iter()
        .filter(|&&(_, capacity)| capacity == top)
        .map(|&(cpu, _)| cpu)
        .collect::<Vec<_>>();
    (fast.len() < cores.len()).then_some(fast)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_takes_its_cores_from_the_fast_ones_and_never_all_of_them() {
        let set = |cores: &[usize]| cores.iter().copied().collect::<BTreeSet<_>>();
        let fast = [6, 7];
        assert_eq!(beside_capture(&fast, &set(&[7])), Some(vec![6]));
        assert_eq!(beside_capture(&fast, &set(&[6])), Some(vec![7]));
        assert_eq!(beside_capture(&fast, &set(&[0, 1])), Some(vec![6, 7]));
        assert_eq!(beside_capture(&fast, &set(&[6, 7])), None);
        assert_eq!(beside_capture(&fast, &set(&[5, 6, 7])), None);
    }

    #[test]
    fn the_end_of_turn_worker_narrows_only_beside_a_capture_list_that_takes() {
        let all: BTreeSet<usize> = (0..8).collect();
        let fast = [6, 7];
        let kept = |capture| kept(&fast, capture, Some(&all), 2);
        assert_eq!(kept(None), (vec![6, 7], 2));
        assert_eq!(kept(Some("7")), (vec![6], 1));
        for left_alone in ["6-7", "0-1", "big", "", "7,8"] {
            assert_eq!(kept(Some(left_alone)), (vec![6, 7], 2), "{left_alone:?}");
        }
        // A cpuset without core 7: capture restores its mask, so this keeps both.
        let without_7: BTreeSet<usize> = (0..7).collect();
        assert_eq!(
            super::kept(&fast, Some("7"), Some(&without_7), 2),
            (vec![6, 7], 2)
        );
        // One top core and capture elsewhere: the threads asked stay.
        assert_eq!(super::kept(&[7], Some("0-1"), Some(&all), 2), (vec![7], 2));
    }
}
