//! Retain an optional, unready child without weakening mandatory startup faults.
use super::*;
use std::os::unix::fs::DirBuilderExt;

pub(crate) enum RingStartup {
    Ready(Box<Worker>),
    Unavailable {
        worker: Option<Box<Worker>>,
        error: io::Error,
    },
}

/// A fresh namespace belongs to one ring attempt, never a replacement. Peer
/// sockets can survive SIGKILL. Remember observed inodes; remove only the same
/// owned socket after this child has exited. An unexpected/replaced path is an
/// explicit cleanup failure, never a recursive deletion or permission repair.
pub(super) struct RingResources {
    directory: PathBuf,
    directory_identity: (u64, u64),
    peers: [Option<(u64, u64)>; 2],
    cleaned: bool,
}
impl RingResources {
    fn create(parent: &Path) -> io::Result<Self> {
        let directory = parent.join("ring");
        fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let metadata = match fs::symlink_metadata(&directory) {
            Ok(metadata) => metadata,
            Err(error) => {
                // No child or socket exists yet; remove only this empty path.
                let _ = fs::remove_dir(&directory);
                return Err(error);
            }
        };
        Ok(Self {
            directory,
            directory_identity: (metadata.dev(), metadata.ino()),
            peers: [None, None],
            cleaned: false,
        })
    }

    fn check_directory(&self) -> io::Result<()> {
        let metadata = fs::symlink_metadata(&self.directory)?;
        if !metadata.file_type().is_dir()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o077 != 0
            || (metadata.dev(), metadata.ino()) != self.directory_identity
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "ring startup namespace changed",
            ));
        }
        Ok(())
    }

    pub(super) fn observe_peers(&mut self) -> io::Result<()> {
        self.check_directory()?;
        for (index, leaf) in ["ring.c.w", "ring.d.w"].iter().enumerate() {
            let metadata = match fs::symlink_metadata(self.directory.join(leaf)) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            if !metadata.file_type().is_socket()
                || metadata.uid() != rustix::process::geteuid().as_raw()
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "ring cleanup peer is not an owned socket",
                ));
            }
            let identity = (metadata.dev(), metadata.ino());
            if self.peers[index].is_some_and(|old| old != identity) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "ring cleanup peer inode changed",
                ));
            }
            self.peers[index] = Some(identity);
        }
        Ok(())
    }

    /// Caller has already observed/reaped the only child allowed in this fresh
    /// namespace. Include sockets created after failure but before its exit.
    pub(super) fn cleanup_after_exit(&mut self) -> io::Result<()> {
        if self.cleaned {
            return Ok(());
        }
        self.observe_peers()?;
        for (index, leaf) in ["ring.c.w", "ring.d.w"].iter().enumerate() {
            let path = self.directory.join(leaf);
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            if !metadata.file_type().is_socket()
                || self.peers[index] != Some((metadata.dev(), metadata.ino()))
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "ring cleanup peer changed before removal",
                ));
            }
            fs::remove_file(path)?;
        }
        self.cleaned = true;
        Ok(())
    }
}
impl Drop for RingResources {
    fn drop(&mut self) {
        // Parent Endpoints have already removed their own socket inodes. Do not
        // erase foreign paths, or a replaced namespace, even during best effort.
        if self.check_directory().is_ok() {
            let _ = fs::remove_dir(&self.directory);
        }
    }
}

impl Worker {
    pub(crate) fn spawn_optional_ring_with_tick(
        executable: &Path,
        directory: &Path,
        parent: BootId,
        extra: &[&str],
        mut on_wait: impl FnMut() -> io::Result<()>,
    ) -> io::Result<RingStartup> {
        // Even an immediate bind/exec failure must not hide a mandatory fault.
        on_wait()?;
        let mut mandatory_error = None;
        let mut worker = None;
        let result = (|| {
            let resources = RingResources::create(directory)?;
            let path = resources.directory.clone();
            let boot = new_boot()?;
            let role = lamp_voice_processes::WorkerRole::Ring.as_str();
            let channels = WorkerChannels::bind(&path, role, parent, boot, false)?;
            let child = Command::new(executable)
                .arg(lamp_voice_processes::Command::Worker.as_str())
                .arg(role)
                .arg(&path)
                .arg(boot_text(parent))
                .arg(boot_text(boot))
                .args(extra)
                .env_clear()
                .envs(crate::stalls::passed_on())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()?;
            worker = Some(Box::new(Self {
                channels,
                child,
                boot,
                role: "ring".into(),
                diagnostic_start: None,
                ring_resources: Some(resources),
                #[cfg(test)]
                on_stop: None,
                #[cfg(test)]
                after_terminate: None,
            }));
            worker
                .as_mut()
                .unwrap()
                .wait_until_ready(&path, || match on_wait() {
                    Ok(()) => Ok(()),
                    Err(error) => {
                        mandatory_error = Some(error);
                        Err(io::Error::other("mandatory startup supervision failed"))
                    }
                })
        })();
        if let Some(error) = mandatory_error {
            // Existing mandatory startup behavior: owned child teardown, then
            // the original typed error, never an optional-capability success.
            return Err(error);
        }
        match result {
            Ok(()) => Ok(RingStartup::Ready(worker.unwrap())),
            Err(error) => {
                on_wait()?;
                Ok(RingStartup::Unavailable { worker, error })
            }
        }
    }
}
