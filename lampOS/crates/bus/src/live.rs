//! Who publishes a topic now.
//!
//! A subscriber shows itself by its socket, which the kernel answers for. A
//! publisher holds nothing a stranger could ask about, so it keeps a file,
//! `<topic>/pubs/<source>.<pid>.<boot>`, and gives it a new time at most
//! once a second for as long as it publishes or says that it is alive. The
//! file is empty until the publisher drops a message for a subscriber; then
//! it says each such subscriber's total, one `<subscriber> <count>` a line.
//! That is ROS's liveliness with a lease: a publisher "has to indicate that
//! it is alive" within one, and a file whose time is older than the lease is
//! a dead publisher's. Whoever lists the publishers removes those.
//!
//! The time is the wall clock's, the only one two processes share. A clock
//! that is stepped by more than the lease makes every publisher look dead
//! until its next refresh, at most a second later, when it puts its file
//! back.
use crate::{FILE_MODE, name};
use std::{
    fs,
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process,
    time::{Duration, SystemTime},
};

/// One publisher that has shown itself alive within the lease.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    /// The publisher's name, as in its messages' `source`.
    pub name: String,
    /// The process it runs in.
    pub pid: u32,
    /// The instance, as in its messages' `boot`.
    pub boot: String,
    /// How long ago it last showed itself alive.
    pub age: Duration,
}

/// What one live publisher could not give one subscriber since it found it,
/// as its entry says at its last refresh.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drops {
    pub publisher: Source,
    /// The subscriber's socket without `.sock`: its name, process and count.
    pub subscriber: String,
    pub dropped: u64,
}

/// Where the publisher `source` of this process, instance `boot`, shows
/// itself in `pubs`.
pub(crate) fn entry(pubs: &Path, source: &str, boot: &str) -> PathBuf {
    pubs.join(format!("{source}.{}.{boot}", process::id()))
}

/// Where that entry is written before it is put in place: a name no entry
/// has, since no source begins with a dot.
pub(crate) fn writing(pubs: &Path, source: &str, boot: &str) -> PathBuf {
    pubs.join(format!(".{source}.{}.{boot}.new", process::id()))
}

/// `<source>.<pid>.<boot>`, or `None` for a file that is not an entry.
fn parts(file: &str) -> Option<(&str, u32, &str)> {
    let mut split = file.split('.');
    let (source, pid, boot) = (split.next()?, split.next()?, split.next()?);
    let hex = |byte: u8| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte);
    (split.next().is_none()
        && name::source(source).is_ok()
        && !pid.is_empty()
        && pid.bytes().all(|byte| byte.is_ascii_digit())
        && boot.len() == 16
        && boot.bytes().all(hex))
    .then_some(())?;
    Some((source, pid.parse().ok()?, boot))
}

/// Give the entry at `path` the time it is now, creating it if it is not
/// there: a publisher that was taken for dead finds its entry removed, and
/// this puts it back. Fails with `NotFound` when `pubs/` itself is missing.
pub(crate) fn touch(path: &Path) -> io::Result<()> {
    let file = match fs::OpenOptions::new().write(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .mode(FILE_MODE)
                .open(path)?;
            // The mode asked for, whatever the process's umask took from it.
            file.set_permissions(fs::Permissions::from_mode(FILE_MODE))?;
            file
        }
        Err(error) => return Err(error),
    };
    file.set_modified(SystemTime::now())
}

/// Write `said` beside the entry at `path` and put it in place: a reader
/// sees the entry before or this one, never part of either, and its time is
/// now. Fails with `NotFound` when `pubs/` itself is missing.
pub(crate) fn write(path: &Path, beside: &Path, said: &[u8]) -> io::Result<()> {
    let written = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(FILE_MODE)
        .open(beside)
        .and_then(|mut file| {
            file.write_all(said)?;
            // The mode asked for, whatever the process's umask took from it.
            file.set_permissions(fs::Permissions::from_mode(FILE_MODE))
        })
        .and_then(|()| fs::rename(beside, path));
    if written.is_err() {
        let _ = fs::remove_file(beside);
    }
    written
}

/// What each live publisher in `pubs` says it could not give a subscriber:
/// one line `<subscriber> <count>` for each that it dropped for. A line
/// that is not one is passed over.
pub(crate) fn drops(pubs: &Path, lease: Duration) -> io::Result<Vec<Drops>> {
    let mut found = Vec::new();
    for publisher in list(pubs, lease)? {
        let file = pubs.join(format!(
            "{}.{}.{}",
            publisher.name, publisher.pid, publisher.boot
        ));
        // An entry that went since it was listed says nothing.
        let Ok(said) = fs::read_to_string(&file) else {
            continue;
        };
        for line in said.lines() {
            let mut words = line.split(' ');
            if let (Some(subscriber), Some(count), None) =
                (words.next(), words.next(), words.next())
                && !subscriber.is_empty()
                && let Ok(dropped) = count.parse()
            {
                found.push(Drops {
                    publisher: publisher.clone(),
                    subscriber: subscriber.into(),
                    dropped,
                });
            }
        }
    }
    Ok(found)
}

/// The publishers in `pubs` that showed themselves within `lease`, by name.
/// An entry older than that is removed, and so is one with a time further
/// ahead than the lease, which only a clock set back can leave: its
/// publisher, if it lives, puts it back within a second.
pub(crate) fn list(pubs: &Path, lease: Duration) -> io::Result<Vec<Source>> {
    let entries = match fs::read_dir(pubs) {
        Ok(entries) => entries,
        // Nobody has published yet.
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let now = SystemTime::now();
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry?;
        let file = entry.file_name();
        let Some((source, pid, boot)) = file.to_str().and_then(parts) else {
            continue;
        };
        // An entry that went while the directory was read is nobody's.
        let Ok(modified) = entry.metadata().and_then(|metadata| metadata.modified()) else {
            continue;
        };
        let age = match now.duration_since(modified) {
            Ok(age) => age,
            Err(ahead) if ahead.duration() <= lease => Duration::ZERO,
            Err(_) => Duration::MAX,
        };
        if age > lease {
            let _ = fs::remove_file(entry.path());
            continue;
        }
        found.push(Source {
            name: source.into(),
            pid,
            boot: boot.into(),
            age,
        });
    }
    found.sort_by(|a, b| (&a.name, a.pid, &a.boot).cmp(&(&b.name, b.pid, &b.boot)));
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_is_a_source_a_process_and_an_instance_and_nothing_else_is_one() {
        assert_eq!(
            parts("body.412.9f3a6c1e04b7d25a"),
            Some(("body", 412, "9f3a6c1e04b7d25a"))
        );
        assert_eq!(
            parts("lamp_bus_cli.1.0000000000000000"),
            Some(("lamp_bus_cli", 1, "0000000000000000"))
        );
        for file in [
            "",
            "body",
            "body.412",
            "body.412.9f3a",
            "body.412.9F3A6C1E04B7D25A",
            "body.x.9f3a6c1e04b7d25a",
            "body..9f3a6c1e04b7d25a",
            "Body.412.9f3a6c1e04b7d25a",
            "body.412.9f3a6c1e04b7d25a.new",
            ".body.412.9f3a6c1e04b7d25a",
            "body.99999999999.9f3a6c1e04b7d25a",
        ] {
            assert_eq!(parts(file), None, "{file:?}");
        }
        let path = entry(
            Path::new("/run/lampos-bus/servos/pubs"),
            "body",
            "9f3a6c1e04b7d25a",
        );
        let file = path.file_name().unwrap().to_str().unwrap();
        assert_eq!(
            parts(file),
            Some(("body", process::id(), "9f3a6c1e04b7d25a"))
        );
    }
}
