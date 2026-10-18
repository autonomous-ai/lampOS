//! Who holds a microphone that answered busy at start (#1567): named once in
//! the journal, so a holder such as PulseAudio is shown, not guessed from
//! timing. Only each holder's pid and name (`comm`) are read, never its
//! command line. Every function takes the `/proc` root, so a test can build
//! a fake tree; the capture worker (Linux only) passes `/proc`.
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// The scan's bound: it runs once, on the capture thread before the
/// microphone is open, so a Stop or a mute waits for it. Past either bound
/// it stops and says `holder not visible (scan capped)`.
pub const SCAN_TIME: Duration = Duration::from_millis(100);
pub const SCAN_ENTRIES: usize = 4096;

/// The capture node an ALSA name opens, `/dev/snd/pcmC<card>D<dev>c`, for
/// `hw:2,0`, `plughw:CARD=device_micro2,DEV=0` or `plug:device_micro2` (a
/// name that is the card's own id). A card named, not numbered, is found by
/// its `<proc>/asound/<id>` link to `card<N>`. Otherwise the name is no card
/// (an `asound.conf` alias, as `device_micro2` is on the ReSpeaker and
/// XVF3800 images), and the error is that name.
pub fn capture_node(proc_root: &Path, device: &str) -> Result<PathBuf, String> {
    let args = device.rsplit_once(':').map_or(device, |(_, args)| args);
    let mut parts = args.split(',');
    let card = parts.next().unwrap_or_default().trim_start_matches("CARD=");
    let no_card = || card.to_owned();
    let dev: u32 = parts
        .next()
        .map_or(Some(0), |dev| dev.trim_start_matches("DEV=").parse().ok())
        .ok_or_else(no_card)?;
    let index: u32 = match card.parse() {
        Ok(index) => index,
        Err(_) => fs::read_link(proc_root.join("asound").join(card))
            .ok()
            .and_then(|link| link.to_str()?.strip_prefix("card")?.parse().ok())
            .ok_or_else(no_card)?,
    };
    Ok(PathBuf::from(format!("/dev/snd/pcmC{index}D{dev}c")))
}

/// Each process under `proc_root` with `node` open: its pid and `comm`,
/// sorted by pid. Processes whose descriptors cannot be read are skipped.
/// None when the scan reached `max_entries` `/proc` entries or `max_time`.
pub fn holders(
    proc_root: &Path,
    node: &Path,
    max_entries: usize,
    max_time: Duration,
) -> Option<Vec<(u32, String)>> {
    let started = Instant::now();
    holders_timed(proc_root, node, max_entries, max_time, || started.elapsed())
}

/// [`holders`] with its clock given, so a test can run out of time at a
/// chosen read. Only `/proc` entries count toward `max_entries`; the time
/// is checked at each entry and at each descriptor too, so one process with
/// very many descriptors cannot run the scan past `max_time`.
fn holders_timed(
    proc_root: &Path,
    node: &Path,
    max_entries: usize,
    max_time: Duration,
    mut elapsed: impl FnMut() -> Duration,
) -> Option<Vec<(u32, String)>> {
    let Ok(entries) = fs::read_dir(proc_root) else {
        return Some(Vec::new());
    };
    let mut found = Vec::new();
    for (seen, entry) in entries.flatten().enumerate() {
        if seen >= max_entries || elapsed() >= max_time {
            return None;
        }
        let Some(pid) = entry.file_name().to_str().and_then(|n| n.parse().ok()) else {
            continue;
        };
        let Ok(fds) = fs::read_dir(entry.path().join("fd")) else {
            continue;
        };
        let mut holds = false;
        for fd in fds.flatten() {
            if elapsed() >= max_time {
                return None;
            }
            if fs::read_link(fd.path()).is_ok_and(|target| target == node) {
                holds = true;
                break;
            }
        }
        if holds {
            let comm = fs::read_to_string(entry.path().join("comm")).unwrap_or_default();
            found.push((pid, comm.trim().to_owned()));
        }
    }
    found.sort();
    Some(found)
}

/// The journal's words for who holds `device`: `held by pulseaudio[80598]`;
/// `holder not visible` when none can be seen (no permission),
/// `holder not visible (scan capped)` past the scan's bound, or
/// `holder not looked up: device_micro2 is no card`.
pub fn said(proc_root: &Path, device: &str) -> String {
    said_within(proc_root, device, SCAN_ENTRIES, SCAN_TIME)
}

fn said_within(proc_root: &Path, device: &str, max_entries: usize, max_time: Duration) -> String {
    let node = match capture_node(proc_root, device) {
        Ok(node) => node,
        Err(name) => return format!("holder not looked up: {name} is no card"),
    };
    match holders(proc_root, &node, max_entries, max_time) {
        None => "holder not visible (scan capped)".to_owned(),
        Some(held) if held.is_empty() => "holder not visible".to_owned(),
        Some(held) => {
            let names: Vec<String> = held
                .iter()
                .map(|(pid, comm)| format!("{comm}[{pid}]"))
                .collect();
            format!("held by {}", names.join(", "))
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::{capture_node, holders_timed, said, said_within};
    use std::{fs, os::unix::fs::symlink, path::Path, time::Duration};

    /// A fake `/proc`: card 2 is `device_micro2`, PulseAudio (pid 80598)
    /// holds its capture node, pid 4242 holds only the speaker's, pid 7 has
    /// descriptors that cannot be read, and `device_cmedia` is no card.
    #[test]
    fn the_holder_of_a_busy_microphone_is_named_by_pid_and_comm() {
        let root = std::env::temp_dir().join(format!("lamp-mic-holder-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("asound")).unwrap();
        symlink("card2", root.join("asound/device_micro2")).unwrap();
        for (pid, comm, node) in [
            (80598, "pulseaudio\n", "/dev/snd/pcmC2D0c"),
            (4242, "lamp-voice\n", "/dev/snd/pcmC3D0p"),
        ] {
            let fd = root.join(format!("{pid}/fd"));
            fs::create_dir_all(&fd).unwrap();
            symlink("/dev/null", fd.join("0")).unwrap();
            symlink(node, fd.join("7")).unwrap();
            fs::write(root.join(format!("{pid}/comm")), comm).unwrap();
        }
        fs::create_dir_all(root.join("7")).unwrap();

        assert_eq!(
            capture_node(&root, "plug:device_micro2").as_deref(),
            Ok(Path::new("/dev/snd/pcmC2D0c"))
        );
        assert_eq!(
            capture_node(&root, "plughw:CARD=device_micro2,DEV=1").as_deref(),
            Ok(Path::new("/dev/snd/pcmC2D1c"))
        );
        assert_eq!(
            capture_node(&root, "hw:3,0").as_deref(),
            Ok(Path::new("/dev/snd/pcmC3D0c"))
        );
        assert_eq!(
            capture_node(&root, "plug:device_cmedia"),
            Err("device_cmedia".to_owned())
        );

        assert_eq!(
            said(&root, "plug:device_micro2"),
            "held by pulseaudio[80598]"
        );
        assert_eq!(said(&root, "hw:3,0"), "holder not visible");
        // An asound.conf alias (ReSpeaker, XVF3800): no card to look up.
        assert_eq!(
            said(&root, "plug:device_cmedia"),
            "holder not looked up: device_cmedia is no card"
        );
        // Four entries (asound, 7, 4242, 80598): a cap of 3 stops the scan,
        // 4 lets it finish; descriptors do not count.
        assert_eq!(
            said_within(&root, "plug:device_micro2", 3, Duration::from_secs(1)),
            "holder not visible (scan capped)"
        );
        assert_eq!(
            said_within(&root, "plug:device_micro2", 4, Duration::from_secs(1)),
            "held by pulseaudio[80598]"
        );
        assert_eq!(
            said_within(&root, "plug:device_micro2", 4, Duration::ZERO),
            "holder not visible (scan capped)"
        );
        assert_eq!(said(&root.join("absent"), "hw:2,0"), "holder not visible");
        fs::remove_dir_all(&root).unwrap();
    }

    /// One process with many descriptors is stopped by the time, inside its
    /// descriptor walk: the clock is in time for the `/proc` entry and out
    /// of it from the first descriptor on. Many processes are stopped by
    /// the count of `/proc` entries.
    #[test]
    fn the_scan_is_capped_by_time_in_a_long_descriptor_walk_and_by_count_over_many_processes() {
        let root = std::env::temp_dir().join(format!("lamp-mic-holder-cap-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let node = Path::new("/dev/snd/pcmC2D0c");
        let many = root.join("9000/fd");
        fs::create_dir_all(&many).unwrap();
        for fd in 0..100 {
            symlink("/dev/null", many.join(fd.to_string())).unwrap();
        }
        let max = Duration::from_millis(100);
        let mut reads = 0;
        let clock = || {
            reads += 1;
            if reads == 1 { Duration::ZERO } else { max }
        };
        assert_eq!(holders_timed(&root, node, 4096, max, clock), None);
        assert_eq!(reads, 2, "stopped at its first descriptor");
        assert_eq!(
            holders_timed(&root, node, 4096, max, || Duration::ZERO),
            Some(Vec::new()),
            "in time, its 100 descriptors are all read"
        );

        for pid in 1..=10 {
            fs::create_dir_all(root.join(format!("{pid}/fd"))).unwrap();
        }
        assert_eq!(holders_timed(&root, node, 10, max, || Duration::ZERO), None);
        assert_eq!(
            holders_timed(&root, node, 11, max, || Duration::ZERO),
            Some(Vec::new())
        );
        fs::remove_dir_all(&root).unwrap();
    }
}
