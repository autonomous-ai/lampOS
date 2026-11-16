//! The person's speaker level, kept across a power cut (#5551,
//! `LAMP_VOLUME_KEPT`, off unless set).
//!
//! A V2 lamp has no power button: it is unplugged, so it almost never gets
//! the clean shutdown at which `alsa-restore` stores the mixer, and
//! `/var/lib/alsa/asound.state` is never written. The softvol "Speaker"
//! control is made fresh at each boot, at 100 %, whatever the person chose.
//!
//! So `lamp-actions` keeps the level itself: after each change the person
//! asks for (louder, quieter, unmute, the app's slider), once the changes
//! have stopped for [`QUIET_FOR`], the level is written to [`FILE`] in its
//! own durable folder (`LAMP_OUTPUT_CAPS_DIR`, which the product reset
//! wipes), synced, then renamed into place. At its next start, before it
//! serves, it sets the control back to that level.
//!
//! A mute is never kept, so a lamp never comes back silent. A temporary
//! limit (`set_limit`) is not the person's choice, so it is not kept either.
//! With no file, nothing is set: a new or reset lamp keeps the level its
//! setup gave it (#5576's first volume).

use std::{
    ffi::OsStr,
    fs, io,
    io::Write,
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

/// The switch, in the product unit: `on` exactly.
pub const ENVIRONMENT: &str = "LAMP_VOLUME_KEPT";
/// The file in the durable folder: one line, the raw level, 1 to 100.
pub const FILE: &str = "kept-volume";
const NEW: &str = "kept-volume.new";
/// How long the level must stay unchanged before it is written: a held
/// "louder, louder, louder" is one write.
pub const QUIET_FOR: Duration = Duration::from_millis(1_500);
/// The start tries the control again while it fails (it is made by the
/// first open of the speaker, `lamp-cmedia-speaker`), beginning a new try
/// only within [`RESTORE_START_BY`]. With a try at most two mixer calls of
/// [`crate::sound::AMIXER_WAIT`] (0.8 s) each, the start waits 5 s at most,
/// mixer calls included.
pub const RESTORE_START_BY: Duration = Duration::from_millis(3_400);
pub const RESTORE_EVERY: Duration = Duration::from_millis(250);

#[cfg(test)]
const _: () =
    assert!(RESTORE_START_BY.as_millis() + 2 * crate::sound::AMIXER_WAIT.as_millis() <= 5_000);

/// Whether the switch is on.
pub fn on(value: Option<&OsStr>) -> bool {
    value == Some(OsStr::new("on"))
}

/// The level in a kept file's text, or none when it is not one.
pub fn parse(text: &str) -> Option<u8> {
    text.trim()
        .parse::<u8>()
        .ok()
        .filter(|level| (1..=100).contains(level))
}

/// The kept level in `directory`, if there is a readable one.
pub fn read(directory: &Path) -> Option<u8> {
    fs::read_to_string(directory.join(FILE))
        .ok()
        .as_deref()
        .and_then(parse)
}

/// Write `level` so a power cut leaves either the old file or the new one.
pub fn write(directory: &Path, level: u8) -> io::Result<()> {
    if !(1..=100).contains(&level) {
        return Err(io::Error::other("a kept level is 1 to 100"));
    }
    let new = directory.join(NEW);
    let mut file = fs::File::create(&new)?;
    file.write_all(format!("{level}\n").as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(&new, directory.join(FILE))?;
    fs::File::open(directory)?.sync_all()
}

/// Told each level the person's change left.
pub trait Keep {
    fn keep(&mut self, level: u8);
}

/// Writes on its own thread, so a change is answered before the write.
pub struct Keeper {
    levels: mpsc::Sender<u8>,
}

impl Keeper {
    pub fn start(directory: PathBuf) -> Self {
        Self::start_with(directory, QUIET_FOR)
    }

    fn start_with(directory: PathBuf, quiet_for: Duration) -> Self {
        let (levels, heard) = mpsc::channel::<u8>();
        let mut written = read(&directory);
        std::thread::spawn(move || {
            while let Ok(mut level) = heard.recv() {
                // The newest level, once the changes have stopped.
                loop {
                    match heard.recv_timeout(quiet_for) {
                        Ok(newer) => level = newer,
                        Err(mpsc::RecvTimeoutError::Timeout) => break,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
                if written == Some(level) {
                    continue;
                }
                match write(&directory, level) {
                    Ok(()) => written = Some(level),
                    Err(error) => {
                        eprintln!("lamp-actions: the volume {level} is not kept: {error}")
                    }
                }
            }
        });
        Self { levels }
    }
}

impl Keep for Keeper {
    fn keep(&mut self, level: u8) {
        let _ = self.levels.send(level);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn a_kept_level_is_one_whole_number_from_1_to_100() {
        assert_eq!(parse("56\n"), Some(56));
        assert_eq!(parse(" 1 "), Some(1));
        assert_eq!(parse("100"), Some(100));
        for refused in ["", "0", "101", "-3", "56%", "loud", "5 6"] {
            assert_eq!(parse(refused), None, "{refused:?}");
        }
    }

    #[test]
    fn a_written_level_is_read_back_and_a_bad_one_is_never_written() {
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(read(folder.path()), None);
        write(folder.path(), 56).unwrap();
        assert_eq!(read(folder.path()), Some(56));
        write(folder.path(), 30).unwrap();
        assert_eq!(read(folder.path()), Some(30));
        assert!(!folder.path().join(NEW).exists());
        for refused in [0, 101] {
            assert!(write(folder.path(), refused).is_err());
        }
        assert_eq!(read(folder.path()), Some(30));
    }

    #[test]
    fn the_keeper_writes_the_newest_level_once_the_changes_stop() {
        let folder = tempfile::tempdir().unwrap();
        let mut keeper = Keeper::start_with(folder.path().into(), Duration::from_millis(100));
        for level in [60, 70, 80] {
            keeper.keep(level);
        }
        assert_eq!(
            read(folder.path()),
            None,
            "written before the changes stopped"
        );
        let started = Instant::now();
        while read(folder.path()) != Some(80) {
            assert!(started.elapsed() < Duration::from_secs(5), "never written");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
