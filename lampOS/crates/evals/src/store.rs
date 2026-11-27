//! One locked, private directory and one checksummed atomic checkpoint.
//!
//! An installer/caller creates the directory with mode 0700. Opening it neither
//! repairs permissions nor follows a directory symlink. Only one recorder may
//! hold it. All reads use the held descriptor, refuse links/special files, and
//! are bounded before parsing. A corrupt checkpoint is an error, never an empty
//! healthy history. The checksum detects accidental corruption, not forgery by
//! a party already allowed to write this private directory.
//!
//! Saving is blocking filesystem work, explicitly outside the ingestion path.
//! The future supervised process must call it from its bounded persistence
//! worker. This module does not claim a deadline for a kernel filesystem stall.
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rustix::fs::{FileType, Mode, OFlags};
use sha2::{Digest, Sha256};

use crate::Recorder;

/// Fixed name inside the caller's exclusively held directory.
pub const FILE: &str = "state.bin";
/// Bound on the recorder's JSON payload, before the fixed integrity header.
pub const MAX_CHECKPOINT_BYTES: usize = 8 * 1024 * 1024;
/// Includes the committed checkpoint and any unrelated entries, excluding dot
/// entries. A full/unexpected directory fails closed before adding candidates.
pub const MAX_DIRECTORY_ENTRIES: usize = 64;
const MAGIC: &[u8] = b"LAMP-EVALS-STATE\0\x01";
const HEADER: usize = MAGIC.len() + 4 + 32;
const MAX_FILE_BYTES: usize = MAX_CHECKPOINT_BYTES + HEADER;

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid evaluation checkpoint")
}

/// A lifetime-held exclusive lock on an existing private directory.
#[derive(Debug)]
pub struct Store {
    directory: OwnedFd,
    path: PathBuf,
}

impl Store {
    /// Refuse shared/wrong-owner folders and a second writer immediately.
    pub fn open(path: &Path) -> io::Result<Self> {
        // Trailing slash/dot syntax can make the kernel follow a final
        // symlink despite O_NOFOLLOW. Remove only lexical separators/dots;
        // do not canonicalize (which would follow links before this check).
        let path: PathBuf = path.components().collect();
        let directory = rustix::fs::open(
            &path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        let metadata = rustix::fs::fstat(&directory)?;
        if metadata.st_uid != rustix::process::geteuid().as_raw() || metadata.st_mode & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "evaluation storage requires a private owned directory",
            ));
        }
        lamp_checked_replace::lock(directory.as_fd(), Duration::ZERO)?;
        let store = Self { directory, path };
        store.recover_candidates()?;
        Ok(store)
    }

    /// Missing is distinct from malformed, truncated or unreadable storage.
    pub fn load(&self) -> io::Result<Option<Recorder>> {
        let bytes = match self.read(FILE.as_ref()) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        decode(&bytes).map(Some)
    }

    /// Save the aggregate state and its dedupe checkpoint in one replacement.
    /// Validation happens before writing and again on the synced candidate.
    /// Exclusive access also serializes candidate recovery with replacement;
    /// the lifetime filesystem lock alone does not serialize caller threads.
    pub fn save(&mut self, recorder: &Recorder) -> io::Result<()> {
        self.recover_candidates()?;
        let payload = recorder.checkpoint().map_err(|_| invalid())?;
        let bytes = encode(&payload)?;
        let mut check = |candidate: &Path| {
            let name = candidate.file_name().ok_or("invalid checkpoint name")?;
            let stored = self.read(name).map_err(|_| "checkpoint read failed")?;
            if stored != bytes {
                return Err("checkpoint bytes changed".to_owned());
            }
            decode(&stored)
                .map(|_| ())
                .map_err(|_| "checkpoint validation failed".to_owned())
        };
        lamp_checked_replace::replace_checked_in(
            self.directory.as_fd(),
            &self.path,
            FILE.as_ref(),
            &bytes,
            Mode::RUSR | Mode::WUSR,
            None,
            &mut check,
        )
    }

    /// Remove only the atomic writer's exact crash-leftover names. Incomplete
    /// contents are expected here, so validation is metadata-only. The held
    /// directory lock excludes another writer, and `save` borrows exclusively.
    /// All candidates are checked before any removal; unknown names survive.
    fn recover_candidates(&self) -> io::Result<()> {
        let mut names = Vec::new();
        let mut entries = 0_usize;
        let mut has_checkpoint = false;
        for entry in rustix::fs::Dir::read_from(&self.directory)? {
            let entry = entry?;
            let bytes = entry.file_name().to_bytes();
            if matches!(bytes, b"." | b"..") {
                continue;
            }
            entries += 1;
            if entries > MAX_DIRECTORY_ENTRIES {
                return Err(invalid());
            }
            has_checkpoint |= bytes == FILE.as_bytes();
            let Some(nonce) = bytes
                .strip_prefix(b".state.bin.")
                .and_then(|name| name.strip_suffix(b".new"))
            else {
                continue;
            };
            if nonce.len() != 16
                || !nonce
                    .iter()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
            {
                continue;
            }
            let name = std::ffi::OsStr::from_bytes(bytes);
            let metadata = rustix::fs::statat(
                self.directory.as_fd(),
                name,
                rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
            )?;
            if FileType::from_raw_mode(metadata.st_mode) != FileType::RegularFile
                || metadata.st_uid != rustix::process::geteuid().as_raw()
                || metadata.st_mode & 0o077 != 0
                || metadata.st_nlink != 1
                || metadata.st_size < 0
                || metadata.st_size as u64 > MAX_FILE_BYTES as u64
            {
                return Err(invalid());
            }
            names.push(name.to_owned());
        }
        // A replacement needs one candidate slot. The first save also needs
        // the fixed name, so that its resulting directory remains reopenable.
        // Unrelated entries are never deleted to make space.
        let reserved = if has_checkpoint { 1 } else { 2 };
        if entries - names.len() > MAX_DIRECTORY_ENTRIES - reserved {
            return Err(invalid());
        }
        for name in &names {
            rustix::fs::unlinkat(self.directory.as_fd(), name, rustix::fs::AtFlags::empty())?;
        }
        if !names.is_empty() {
            rustix::fs::fsync(self.directory.as_fd())?;
        }
        Ok(())
    }

    fn read(&self, name: &std::ffi::OsStr) -> io::Result<Vec<u8>> {
        let descriptor = rustix::fs::openat(
            self.directory.as_fd(),
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        let before = rustix::fs::fstat(&descriptor)?;
        if FileType::from_raw_mode(before.st_mode) != FileType::RegularFile
            || before.st_size < 0
            || before.st_size as u64 > MAX_FILE_BYTES as u64
            || before.st_uid != rustix::process::geteuid().as_raw()
            || before.st_mode & 0o077 != 0
            || before.st_nlink != 1
        {
            return Err(invalid());
        }
        let file = File::from(descriptor);
        let mut bytes = Vec::with_capacity(before.st_size as usize);
        (&file)
            .take((MAX_FILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        let after = rustix::fs::fstat(&file)?;
        if bytes.len() != before.st_size as usize
            || bytes.len() > MAX_FILE_BYTES
            || before.st_size != after.st_size
            || before.st_mtime != after.st_mtime
            || before.st_ctime != after.st_ctime
        {
            return Err(invalid());
        }
        Ok(bytes)
    }
}

fn encode(payload: &[u8]) -> io::Result<Vec<u8>> {
    if payload.len() > MAX_CHECKPOINT_BYTES {
        return Err(invalid());
    }
    Recorder::restore(payload).map_err(|_| invalid())?;
    let mut bytes = Vec::with_capacity(HEADER + payload.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&Sha256::digest(payload));
    bytes.extend_from_slice(payload);
    Ok(bytes)
}

fn decode(bytes: &[u8]) -> io::Result<Recorder> {
    if bytes.len() < HEADER || bytes.len() > MAX_FILE_BYTES || !bytes.starts_with(MAGIC) {
        return Err(invalid());
    }
    let length = u32::from_le_bytes(
        bytes[MAGIC.len()..MAGIC.len() + 4]
            .try_into()
            .map_err(|_| invalid())?,
    ) as usize;
    let payload = &bytes[HEADER..];
    if length != payload.len() || Sha256::digest(payload)[..] != bytes[MAGIC.len() + 4..HEADER] {
        return Err(invalid());
    }
    Recorder::restore(payload).map_err(|_| invalid())
}
