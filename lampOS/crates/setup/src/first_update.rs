//! The first update after a setup (#6284, `LAMP_FIRST_UPDATE`, off unless
//! set): just before its marker starts the lamp, setup writes
//! [`lamp_msgs::FIRST_UPDATE_FILE`], beside the marker, holding when it was
//! written. While it is there, lamp-update starts a round at once and the
//! voice holds the walk's beginning, so a new lamp's first five minutes run
//! on the current build; lamp-update removes it when that round ends, and it
//! counts as gone for everyone twenty minutes after it was written
//! ([`lamp_msgs::FIRST_UPDATE_WITHIN_MS`]).
//!
//! Best effort: a write that fails is a line in the log, never a failed
//! setup; the lamp then starts as it did before this switch. Recovery's
//! network-only run (`serve::NetworkOnly`) never writes it: that lamp was
//! set up before.

use std::{
    io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// The switch, in `lamp-setup`'s unit.
pub const ENVIRONMENT: &str = "LAMP_FIRST_UPDATE";

/// Whether the switch is on, as `var` gives it: `on` exactly.
pub fn on(var: Option<&str>) -> bool {
    var == Some("on")
}

/// Where the file goes for a setup whose marker is `marker`: beside it, under
/// the name the product's [`lamp_msgs::FIRST_UPDATE_FILE`] has.
pub fn beside(marker: &Path) -> PathBuf {
    let name = Path::new(lamp_msgs::FIRST_UPDATE_FILE)
        .file_name()
        .expect("the first update's file has a name");
    marker.with_file_name(name)
}

/// The file's bytes for a setup finished at `now`.
pub fn body(now: SystemTime) -> Vec<u8> {
    let at_unix_ms = now.duration_since(UNIX_EPOCH).map_or(0, |since| {
        u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
    });
    let mut bytes = serde_json::to_vec(&lamp_msgs::FirstUpdate {
        at_unix_ms,
        boot: lamp_msgs::BootClock::now(),
    })
    .expect("a number serializes");
    bytes.push(b'\n');
    bytes
}

/// Writes the file at `path`, 0644 (the voice and lamp-update read it), by a
/// temporary file renamed into place, so a reader never sees half of it.
pub fn write(path: &Path, now: SystemTime) -> io::Result<()> {
    use std::{fs, io::Write, os::unix::fs::OpenOptionsExt};
    let temporary = path.with_extension("tmp");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o644)
        .open(&temporary)?;
    file.write_all(&body(now))?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn only_on_exactly_turns_it_on() {
        assert!(on(Some("on")));
        for var in [None, Some(""), Some("ON"), Some("off"), Some("1")] {
            assert!(!on(var), "{var:?}");
        }
    }

    #[test]
    fn the_file_sits_beside_the_marker_under_the_products_name() {
        assert_eq!(
            beside(Path::new("/var/lib/lampos-setup/done")),
            Path::new(lamp_msgs::FIRST_UPDATE_FILE)
        );
        assert_eq!(
            beside(Path::new("/tmp/x/done")),
            Path::new("/tmp/x/first-update")
        );
    }

    #[test]
    fn the_file_says_when_setup_finished_and_reads_back_as_owed() {
        let folder =
            std::env::temp_dir().join(format!("lamp-setup-first-update-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("first-update");
        let now = UNIX_EPOCH + Duration::from_millis(1_794_000_000_000);
        write(&path, now).unwrap();
        let read = lamp_msgs::FirstUpdate::read(&path).unwrap();
        assert_eq!(read.at_unix_ms, 1_794_000_000_000);
        assert!(read.owed_at(1_794_000_000_000 + 1_199_999, None));
        assert!(!read.owed_at(1_794_000_000_000 + 1_200_000, None));
        // On a lamp it also carries this boot's clock, and is owed by it now.
        if cfg!(target_os = "linux") {
            let boot = read.boot.clone().expect("Linux has a boot clock");
            assert!(read.owed_at(0, Some(&boot)));
        }
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);
        assert!(!path.with_extension("tmp").exists());
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
