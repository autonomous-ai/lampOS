//! The apps' own lock: one holder changes the apps folders at a time. A
//! step that touches the skills (the companions) is never run while this is
//! held, and the skills lock is never taken inside it, so there is no order
//! of the two to get wrong.
use std::{
    fs::{self, File, TryLockError},
    path::Path,
    time::{Duration, Instant},
};

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
        fs::create_dir_all(state).map_err(|e| format!("{}: {e}", state.display()))?;
        let path = state.join(".lock");
        let file = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
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
