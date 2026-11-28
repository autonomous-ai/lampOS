//! The big cores (#8754): Phonon runs on the lamp's fastest cores only.
//! On lamp-361f (6 Cortex-A55 at 1.79 GHz, 2 Cortex-A76 at 2.0 GHz),
//! unpinned it ran at 1.02 to 2.09 of real time, and pinned to the two
//! A76 cores at 0.80 to 0.86. The fastest cores are those with the highest
//! `cpuinfo_max_freq`; the process pins itself to them before it starts any
//! thread, so every thread it starts later (the model's) stays there. With
//! no cpufreq, or on another system, nothing is pinned.
use std::path::Path;

/// The switch's name: `big` (the default) pins to the fastest cores, `all`
/// leaves the process on every core.
pub const PIN_ENVIRONMENT: &str = "LAMP_PHONON_PIN";

/// Whether to pin, as `value` gives it: anything but `all` pins.
pub fn pins(value: Option<&str>) -> bool {
    value.map(str::trim) != Some("all")
}

/// Each core's `cpuinfo_max_freq` in kHz, from `root`
/// (`/sys/devices/system/cpu`): the cores that have one.
pub fn max_freqs(root: &Path) -> Vec<(usize, u64)> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut cores: Vec<(usize, u64)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let core = name.to_str()?.strip_prefix("cpu")?.parse().ok()?;
            let khz = std::fs::read_to_string(entry.path().join("cpufreq/cpuinfo_max_freq"))
                .ok()?
                .trim()
                .parse()
                .ok()?;
            Some((core, khz))
        })
        .collect();
    cores.sort_unstable();
    cores
}

/// The fastest cores among `cores` and their frequency, or `None` with
/// none.
pub fn big(cores: &[(usize, u64)]) -> Option<(Vec<usize>, u64)> {
    let top = cores.iter().map(|(_, khz)| *khz).max()?;
    Some((
        cores
            .iter()
            .filter(|(_, khz)| *khz == top)
            .map(|(core, _)| *core)
            .collect(),
        top,
    ))
}

/// Pins the calling thread, and so every thread it starts after, to the
/// fastest cores; what it did, for the journal.
pub fn pin_big() -> String {
    let cores = max_freqs(Path::new("/sys/devices/system/cpu"));
    let Some((big, khz)) = big(&cores) else {
        return "no cpufreq: on every core".to_owned();
    };
    if big.len() == cores.len() {
        return format!("every core is as fast ({} MHz): on every core", khz / 1000);
    }
    match pin(&big) {
        Ok(()) => format!("on the fastest cores {big:?} ({} MHz)", khz / 1000),
        Err(why) => format!("pinning to {big:?} failed ({why}): on every core"),
    }
}

#[cfg(target_os = "linux")]
fn pin(cores: &[usize]) -> Result<(), String> {
    let mut set = rustix::thread::CpuSet::new();
    for core in cores {
        set.set(*core);
    }
    rustix::thread::sched_setaffinity(None, &set).map_err(|e| e.to_string())
}

#[cfg(not(target_os = "linux"))]
fn pin(_cores: &[usize]) -> Result<(), String> {
    Err("Linux only".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_big_cores_are_the_fastest_ones() {
        let lamp_361f = [
            (0, 1_794_000),
            (1, 1_794_000),
            (2, 1_794_000),
            (3, 1_794_000),
            (4, 1_794_000),
            (5, 1_794_000),
            (6, 2_002_000),
            (7, 2_002_000),
        ];
        assert_eq!(big(&lamp_361f), Some((vec![6, 7], 2_002_000)));
        assert_eq!(big(&[]), None);
        assert_eq!(big(&[(0, 5), (1, 5)]), Some((vec![0, 1], 5)));
    }

    #[test]
    fn max_freqs_reads_sysfs_and_skips_cores_without_cpufreq() {
        let root = tempfile::tempdir().unwrap();
        for (core, khz) in [(0, Some("1794000\n")), (7, Some("2002000\n")), (3, None)] {
            let dir = root.path().join(format!("cpu{core}/cpufreq"));
            std::fs::create_dir_all(&dir).unwrap();
            if let Some(khz) = khz {
                std::fs::write(dir.join("cpuinfo_max_freq"), khz).unwrap();
            }
        }
        std::fs::create_dir_all(root.path().join("cpufreq")).unwrap();
        std::fs::create_dir_all(root.path().join("cpuidle")).unwrap();
        assert_eq!(max_freqs(root.path()), [(0, 1_794_000), (7, 2_002_000)]);
        assert!(max_freqs(&root.path().join("none")).is_empty());
    }

    #[test]
    fn it_pins_unless_all_is_asked() {
        assert!(pins(None) && pins(Some("big")) && pins(Some("")));
        assert!(!pins(Some("all")) && !pins(Some(" all ")));
    }
}
