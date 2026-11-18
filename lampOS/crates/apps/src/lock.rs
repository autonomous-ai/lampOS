//! The apps' own lock: one holder changes the apps folders at a time. A
//! step that touches the skills (the companions) is never run while this is
//! held, and the skills lock is never taken inside it, so there is no order
//! of the two to get wrong.
use std::{
    fs::{self, File, TryLockError},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::Path,
    time::{Duration, Instant},
};

/// The state folder: world-readable (lamp-cloud reads the marks in it), made
/// 0755 whatever the umask; the lock files in it are root's alone (0600), so
/// no other user can open one and hold it.
fn state_folder(state: &Path) -> Result<(), String> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o755)
        .create(state)
        .map_err(|e| format!("{}: {e}", state.display()))
}

/// A lock file: 0600, never through a link.
fn lock_file(path: &Path) -> Result<File, String> {
    File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// What a second asker is told when the first does not let go in time.
pub const BUSY: &str = "the lamp's apps can't be changed now";

/// Held until dropped (the file closes, the lock goes with it).
#[derive(Debug)]
pub struct AppsLock {
    _file: File,
}

impl AppsLock {
    /// Waits at most `wait` for the lock in `state`'s folder, then [`BUSY`].
    pub fn take(state: &Path, wait: Duration) -> Result<Self, String> {
        state_folder(state)?;
        let path = state.join(".lock");
        let file = lock_file(&path)?;
        let started = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self { _file: file }),
                Err(TryLockError::WouldBlock) => {
                    if started.elapsed() >= wait {
                        return Err(BUSY.to_owned());
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(TryLockError::Error(error)) => {
                    return Err(format!("{}: {error}", path.display()));
                }
            }
        }
    }
}

/// Held by every helper instance for its whole life (shared), and by the
/// start-up sweep (exclusive, only if no other instance is alive): a second
/// instance's live `installing` mark is never read as a cut one.
#[derive(Debug)]
pub struct Running {
    _file: File,
}

fn running_file(state: &Path) -> Result<(File, std::path::PathBuf), String> {
    state_folder(state)?;
    let path = state.join(".running");
    let file = lock_file(&path)?;
    Ok((file, path))
}

impl Running {
    /// Waits at most `wait` (the sweep holds it exclusively for a moment).
    pub fn shared(state: &Path, wait: Duration) -> Result<Self, String> {
        let (file, path) = running_file(state)?;
        let started = Instant::now();
        loop {
            match file.try_lock_shared() {
                Ok(()) => return Ok(Self { _file: file }),
                Err(TryLockError::WouldBlock) if started.elapsed() < wait => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(TryLockError::WouldBlock) => return Err(BUSY.to_owned()),
                Err(TryLockError::Error(e)) => return Err(format!("{}: {e}", path.display())),
            }
        }
    }

    /// None when any instance is alive.
    pub fn alone(state: &Path) -> Result<Option<Self>, String> {
        let (file, path) = running_file(state)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(e)) => Err(format!("{}: {e}", path.display())),
        }
    }
}
