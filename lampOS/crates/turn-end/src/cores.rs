//! Which processor cores are the fast ones, and keeping the worker on them.
//!
//! The lamp's Allwinner A733 has two Cortex-A76 cores (cpu6, cpu7) and six
//! Cortex-A55 cores; the worker's self-check took 383 ms a verdict on the
//! first pair and 1,292 ms on the second (docs/endpoint-detection.md). Linux
//! gives each core a relative `cpu_capacity` (1024 for the fastest); the
//! cores with the largest value are the fast ones. When every core reports
//! the same capacity, or none reports one, there is nothing to choose. Which
//! cores are fast lives in `lamp_turn_end_wire::cores`, so that the voice
//! keeps its capture worker off the same ones.

pub use lamp_turn_end_wire::cores::{capacities, fastest};

/// Keeps the calling thread, and every thread it starts afterwards, on
/// `cpus`. Linux only; elsewhere it does nothing.
#[cfg(target_os = "linux")]
pub fn keep_on(cpus: &[usize]) -> Result<(), String> {
    use rustix::thread::{CpuSet, sched_setaffinity};
    let mut set = CpuSet::new();
    for &cpu in cpus {
        if cpu >= CpuSet::MAX_CPU {
            return Err(format!("core {cpu} is out of range"));
        }
        set.set(cpu);
    }
    sched_setaffinity(None, &set).map_err(|error| format!("core affinity: {error}"))
}

#[cfg(not(target_os = "linux"))]
pub fn keep_on(_cpus: &[usize]) -> Result<(), String> {
    Ok(())
}

/// The cores the calling thread may run on, as Linux reports them.
#[cfg(target_os = "linux")]
pub fn allowed() -> Option<Vec<usize>> {
    use rustix::thread::{CpuSet, sched_getaffinity};
    let set = sched_getaffinity(None).ok()?;
    Some(
        (0..CpuSet::MAX_CPU)
            .filter(|&cpu| set.is_set(cpu))
            .collect(),
    )
}

#[cfg(not(target_os = "linux"))]
pub fn allowed() -> Option<Vec<usize>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// A core's number, capacity, highest frequency and `online` value.
    type Core = (usize, Option<u64>, Option<u64>, Option<u8>);

    fn sysfs(name: &str, cores: &[Core]) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("lamp-cores-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for &(cpu, capacity, frequency, online) in cores {
            let dir = root.join(format!("cpu{cpu}"));
            std::fs::create_dir_all(dir.join("cpufreq")).unwrap();
            if let Some(capacity) = capacity {
                std::fs::write(dir.join("cpu_capacity"), format!("{capacity}\n")).unwrap();
            }
            if let Some(frequency) = frequency {
                std::fs::write(
                    dir.join("cpufreq/cpuinfo_max_freq"),
                    format!("{frequency}\n"),
                )
                .unwrap();
            }
            if let Some(online) = online {
                std::fs::write(dir.join("online"), format!("{online}\n")).unwrap();
            }
        }
        // Entries that are not cores.
        std::fs::create_dir_all(root.join("cpufreq")).unwrap();
        std::fs::create_dir_all(root.join("cpuidle")).unwrap();
        std::fs::write(root.join("online"), "0-7\n").unwrap();
        root
    }

    #[test]
    fn the_a733_layout_picks_cpu6_and_cpu7() {
        // Six A55 cores at capacity 395 and two A76 cores at 1024.
        let layout: Vec<_> = (0..8)
            .map(|cpu| (cpu, Some(if cpu >= 6 { 1024 } else { 395 }), None, None))
            .collect();
        let root = sysfs("a733", &layout);
        let cores = capacities(&root);
        assert_eq!(cores.len(), 8);
        assert_eq!(fastest(&cores), Some(vec![6, 7]));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn frequency_stands_in_for_a_missing_capacity_and_offline_cores_are_skipped() {
        let root = sysfs(
            "frequency",
            &[
                (0, None, Some(1_794_000), None),
                (1, None, Some(1_794_000), Some(1)),
                (2, None, Some(2_002_000), Some(1)),
                (3, None, Some(2_002_000), Some(0)),
            ],
        );
        let cores = capacities(&root);
        assert_eq!(cores, [(0, 1_794_000), (1, 1_794_000), (2, 2_002_000)]);
        assert_eq!(fastest(&cores), Some(vec![2]));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn equal_or_unknown_cores_leave_nothing_to_choose() {
        assert_eq!(fastest(&[]), None);
        assert_eq!(fastest(&[(0, 1024), (1, 1024)]), None);
        let root = sysfs("none", &[(0, None, None, None), (1, None, None, None)]);
        assert!(capacities(&root).is_empty());
        std::fs::remove_dir_all(root).unwrap();
        assert!(capacities(Path::new("/nonexistent/cpu")).is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn keeping_a_thread_on_its_own_cores_is_reported_back() {
        // Pin to the cores this thread already has, so the test never
        // narrows anything else; a fresh thread keeps the change local.
        std::thread::spawn(|| {
            let before = allowed().unwrap();
            keep_on(&before[..1]).unwrap();
            assert_eq!(allowed().unwrap(), before[..1]);
            assert!(keep_on(&[usize::MAX]).is_err());
        })
        .join()
        .unwrap();
    }
}
