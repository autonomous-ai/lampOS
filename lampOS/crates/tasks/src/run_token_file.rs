//! A run's one-run token handed over as a file (#6510 Part 1): `harness
//! new` takes no environment, so once skill runs go through Harness the
//! token reaches the run as `/run/lampos-tasks/runs/<run-id>` instead of
//! `LAMP_CONTROL_TOKEN`. The file is 0600 in a 0700 folder of lamp-tasks'
//! own, made exclusively and never through a link, written in full and
//! synced; the engine's wrapper (lamp-update) reads it once and deletes it,
//! and [`RunTokenFile`]'s drop deletes it too however the run ends. The
//! token's text is never in a path, a debug line or an error. Not used yet:
//! runs still get the token in their environment until they go through
//! Harness (#6510 Part 1 step 2).
use std::fs::File;
use std::io::Write;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};

use rustix::fs::{FileType, Mode, OFlags};
use rustix::io::Errno;

use crate::skill_task::RunToken;

/// Where runs' token files are made: tmpfs, lamp-tasks' runtime folder.
pub const RUNS_DIR: &str = "/run/lampos-tasks/runs";

/// The longest run id a file is made for (lamp-control's own most).
const RUN_ID_MAX: usize = 64;

/// A run id is a plain name: `a-z`, `0-9` and `-`, 1 to 64 bytes, so the
/// file is always directly in its folder.
pub(crate) fn run_id_ok(run_id: &str) -> bool {
    !run_id.is_empty()
        && run_id.len() <= RUN_ID_MAX
        && run_id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// One run's token file. Dropping it deletes the file (already gone is
/// fine: the wrapper deletes it once read). Its debug line is the path only.
pub struct RunTokenFile {
    dir: OwnedFd,
    name: String,
    path: PathBuf,
}

impl RunTokenFile {
    /// Writes `token` to `<dir>/<run_id>`, 0600, made exclusively and never
    /// through a link; `dir` is made 0700 when missing, and must be a real
    /// folder (not a link) owned by this process's user, open to no one
    /// else. A file already there is refused, never replaced.
    pub fn write(dir: &Path, run_id: &str, token: &RunToken) -> Result<RunTokenFile, String> {
        if !run_id_ok(run_id) {
            return Err("run id is not a plain id (a-z, 0-9, -, at most 64)".into());
        }
        let dir_fd = open_dir(dir)?;
        let path = dir.join(run_id);
        let fd = rustix::fs::openat(
            &dir_fd,
            run_id,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )
        .map_err(|error| format!("run token file {}: not made: {error}", path.display()))?;
        let guard = RunTokenFile {
            dir: dir_fd,
            name: run_id.to_owned(),
            path,
        };
        // From here on, a failure drops the guard and the file with it.
        rustix::fs::fchmod(&fd, Mode::from_raw_mode(0o600))
            .map_err(|error| format!("run token file {}: mode: {error}", guard.path.display()))?;
        let mut file = File::from(fd);
        file.write_all(token.secret().as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|error| {
                format!(
                    "run token file {}: not written: {}",
                    guard.path.display(),
                    error.kind()
                )
            })?;
        Ok(guard)
    }

    /// The file's path, for the run's wrapper.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl std::fmt::Debug for RunTokenFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunTokenFile")
            .field("path", &self.path)
            .finish()
    }
}

impl Drop for RunTokenFile {
    fn drop(&mut self) {
        // Through the folder opened at write: a folder swapped since is
        // never where a file is deleted.
        match rustix::fs::unlinkat(&self.dir, self.name.as_str(), rustix::fs::AtFlags::empty()) {
            Ok(()) | Err(Errno::NOENT) => {}
            Err(error) => eprintln!(
                "lamp-tasks: run token file {}: not deleted: {error}",
                self.path.display()
            ),
        }
    }
}

/// `dir` opened as a folder, made 0700 if missing; a link, another user's
/// folder or one open to others is refused.
fn open_dir(dir: &Path) -> Result<OwnedFd, String> {
    let made = match rustix::fs::mkdir(dir, Mode::from_raw_mode(0o700)) {
        Ok(()) => true,
        Err(Errno::EXIST) => false,
        Err(error) => return Err(format!("runs folder {}: not made: {error}", dir.display())),
    };
    let fd = rustix::fs::open(
        dir,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| format!("runs folder {}: not a real folder: {error}", dir.display()))?;
    if made {
        // The umask may have taken bits off; 0700 exactly.
        rustix::fs::fchmod(&fd, Mode::from_raw_mode(0o700))
            .map_err(|error| format!("runs folder {}: mode: {error}", dir.display()))?;
    }
    let stat = rustix::fs::fstat(&fd)
        .map_err(|error| format!("runs folder {}: {error}", dir.display()))?;
    if FileType::from_raw_mode(stat.st_mode as _) != FileType::Directory {
        return Err(format!("runs folder {}: not a folder", dir.display()));
    }
    if stat.st_uid != rustix::process::geteuid().as_raw() {
        return Err(format!(
            "runs folder {}: not lamp-tasks' own",
            dir.display()
        ));
    }
    if stat.st_mode as u32 & 0o077 != 0 {
        return Err(format!("runs folder {}: open to others", dir.display()));
    }
    Ok(fd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    const SECRET: &str = "s3cret-token-text-6510";

    fn token() -> RunToken {
        RunToken::new(SECRET.into())
    }

    fn mode(path: &Path) -> u32 {
        std::fs::symlink_metadata(path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777
    }

    #[test]
    fn the_file_holds_exactly_the_token_at_0600_in_a_0700_folder_made_for_it() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("runs");
        let guard = RunTokenFile::write(&dir, "task-1-2-3", &token()).unwrap();
        assert_eq!(guard.path(), dir.join("task-1-2-3"));
        assert_eq!(std::fs::read(guard.path()).unwrap(), SECRET.as_bytes());
        assert_eq!(mode(guard.path()), 0o600);
        assert_eq!(mode(&dir), 0o700);
    }

    #[test]
    fn a_second_file_for_the_same_run_is_refused_and_the_first_kept() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("runs");
        let first = RunTokenFile::write(&dir, "task-1", &token()).unwrap();
        let other = RunToken::new("another".into());
        assert!(RunTokenFile::write(&dir, "task-1", &other).is_err());
        assert_eq!(std::fs::read(first.path()).unwrap(), SECRET.as_bytes());
    }

    #[test]
    fn a_run_id_that_is_not_plain_is_refused_before_the_folder_is_touched() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("runs");
        let long = "a".repeat(65);
        for run_id in ["", "a/b", "..", "../x", "Task-1", "task 1", long.as_str()] {
            assert!(
                RunTokenFile::write(&dir, run_id, &token()).is_err(),
                "{run_id:?}"
            );
        }
        assert!(!dir.exists());
        assert!(RunTokenFile::write(&dir, &"a".repeat(64), &token()).is_ok());
    }

    #[test]
    fn a_link_at_the_file_path_is_refused_and_its_target_untouched() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("runs");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let target = root.path().join("target");
        std::fs::write(&target, "untouched").unwrap();
        symlink(&target, dir.join("task-1")).unwrap();
        assert!(RunTokenFile::write(&dir, "task-1", &token()).is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "untouched");
        // A dangling link is refused too, and nothing is made at its target.
        let absent = root.path().join("absent");
        symlink(&absent, dir.join("task-2")).unwrap();
        assert!(RunTokenFile::write(&dir, "task-2", &token()).is_err());
        assert!(!absent.exists());
    }

    #[test]
    fn a_folder_that_is_a_link_is_refused_and_its_target_untouched() {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("real");
        std::fs::create_dir(&real).unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o700)).unwrap();
        let dir = root.path().join("runs");
        symlink(&real, &dir).unwrap();
        assert!(RunTokenFile::write(&dir, "task-1", &token()).is_err());
        assert_eq!(std::fs::read_dir(&real).unwrap().count(), 0);
    }

    #[test]
    fn a_folder_open_to_others_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("runs");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(RunTokenFile::write(&dir, "task-1", &token()).is_err());
        assert!(!dir.join("task-1").exists());
    }

    #[test]
    fn dropping_the_guard_deletes_the_file() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("runs");
        let guard = RunTokenFile::write(&dir, "task-1", &token()).unwrap();
        let path = guard.path().to_owned();
        drop(guard);
        assert!(!path.exists());
    }

    #[test]
    fn dropping_after_the_wrapper_deleted_the_file_is_fine() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("runs");
        let guard = RunTokenFile::write(&dir, "task-1", &token()).unwrap();
        std::fs::remove_file(guard.path()).unwrap();
        drop(guard);
        assert!(!dir.join("task-1").exists());
    }

    #[test]
    fn a_panicking_run_still_deletes_the_file() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("runs");
        let path = dir.join("task-1");
        let ended = std::panic::catch_unwind(|| {
            let _guard = RunTokenFile::write(&dir, "task-1", &token()).unwrap();
            panic!("the run panicked");
        });
        assert!(ended.is_err());
        assert!(!path.exists());
    }

    #[test]
    fn the_token_text_is_never_in_the_debug_line_the_path_or_an_error() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("runs");
        let guard = RunTokenFile::write(&dir, "task-1", &token()).unwrap();
        assert!(!format!("{guard:?}").contains(SECRET));
        assert!(!guard.path().to_string_lossy().contains(SECRET));
        let mut errors = vec![
            RunTokenFile::write(&dir, "task-1", &token()).unwrap_err(),
            RunTokenFile::write(&dir, "Bad", &token()).unwrap_err(),
        ];
        let link = root.path().join("link");
        symlink(&dir, &link).unwrap();
        errors.push(RunTokenFile::write(&link, "task-2", &token()).unwrap_err());
        for error in errors {
            assert!(!error.contains(SECRET), "{error}");
        }
    }
}
