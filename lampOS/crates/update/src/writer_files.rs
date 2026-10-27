//! Read-only file observations for a future writer guard (#3689).
//!
//! **Unwired:** this grants no path authority and writes or restores nothing.
//! The caller supplies a trusted stable root and excludes concurrent writers.
//! Only the root's final component is opened without following links; its
//! ancestry/ownership is a caller precondition, not checked by this module.
//! Relative traversal remains anchored to opened directory descriptors.
//! A no-follow stat refuses known nonregular leaves before opening them; this
//! does not eliminate a replacement race if the exclusion precondition fails.
//!
//! These are file bytes and selected Unix metadata, not ACL/xattr snapshots or
//! proof that an exact restoration is possible. Metadata checks detect changes
//! observed during the read; they cannot provide a filesystem transaction or
//! a kernel/storage wall-clock deadline. Reads may update access times.
use rustix::fs::{AtFlags, FileType, Mode, OFlags, Stat};
use std::{
    fmt,
    fs::{File, Metadata},
    io::{self, Read},
    os::unix::fs::MetadataExt,
    path::Path,
};

/// Hard content bound; the read buffer holds at most one additional byte.
/// Each caller also supplies its smaller explicit cap.
pub const MOST_BYTES: usize = 1024 * 1024;
const MOST_PATH_BYTES: usize = 4096;
const MOST_COMPONENTS: usize = 64;

/// Identity and metadata of the same descriptor before and after observation.
/// Access time is intentionally absent because the observation may advance it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    pub device: u64,
    pub inode: u64,
    pub links: u64,
    pub length: u64,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub modified_seconds: i64,
    pub modified_nanos: i64,
    pub changed_seconds: i64,
    pub changed_nanos: i64,
}

impl Identity {
    fn of(meta: &Metadata) -> Self {
        Self {
            device: meta.dev(),
            inode: meta.ino(),
            links: meta.nlink(),
            length: meta.len(),
            mode: meta.mode(),
            uid: meta.uid(),
            gid: meta.gid(),
            modified_seconds: meta.mtime(),
            modified_nanos: meta.mtime_nsec(),
            changed_seconds: meta.ctime(),
            changed_nanos: meta.ctime_nsec(),
        }
    }
}

pub enum Entry {
    Present {
        bytes: Vec<u8>,
        identity: Identity,
    },
    MissingLeaf,
    /// Zero-based component in the validated relative path. This component
    /// and its entire suffix are unresolved; ancestors includes root and
    /// exactly the existing prefix, so its length is component + 1.
    MissingAncestor {
        component: usize,
    },
}

impl fmt::Debug for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Present { bytes, .. } => f
                .debug_struct("Present")
                .field("byte_count", &bytes.len())
                .finish_non_exhaustive(),
            Self::MissingLeaf => f.write_str("MissingLeaf"),
            Self::MissingAncestor { component } => f
                .debug_struct("MissingAncestor")
                .field("component", component)
                .finish(),
        }
    }
}

/// The identities are ordered root first, followed by each existing relative
/// directory. The caller must bind this result to its validated requested path.
/// No path value or file contents are included by this type's Debug output.
pub struct Observation {
    pub ancestors: Vec<Identity>,
    pub entry: Entry,
}

impl fmt::Debug for Observation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Observation")
            .field("ancestor_count", &self.ancestors.len())
            .field("entry", &self.entry)
            .finish()
    }
}

fn refused(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn failure(error: impl Into<io::Error>, message: &'static str) -> io::Error {
    io::Error::new(error.into().kind(), message)
}

fn components(relative: &str) -> io::Result<Vec<&str>> {
    if relative.len() > MOST_PATH_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid relative observation path",
        ));
    }
    let parts: Vec<_> = relative.split('/').take(MOST_COMPONENTS + 1).collect();
    if parts.len() > MOST_COMPONENTS
        || parts
            .iter()
            .any(|part| part.is_empty() || *part == "." || *part == ".." || part.contains('\0'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid relative observation path",
        ));
    }
    Ok(parts)
}

fn metadata(file: &File) -> io::Result<Metadata> {
    file.metadata()
        .map_err(|error| failure(error, "observation metadata is unavailable"))
}

fn finish(directories: &[(File, Identity)], entry: Entry) -> io::Result<Observation> {
    for (file, before) in directories {
        if Identity::of(&metadata(file)?) != *before {
            return Err(refused("observation ancestor changed"));
        }
    }
    Ok(Observation {
        ancestors: directories.iter().map(|(_, identity)| *identity).collect(),
        entry,
    })
}

fn same_stat(before: &Stat, after: &Stat) -> bool {
    before.st_dev == after.st_dev
        && before.st_ino == after.st_ino
        && before.st_mode == after.st_mode
        && before.st_nlink == after.st_nlink
        && before.st_size == after.st_size
        && before.st_uid == after.st_uid
        && before.st_gid == after.st_gid
        && before.st_mtime == after.st_mtime
        && before.st_mtime_nsec == after.st_mtime_nsec
        && before.st_ctime == after.st_ctime
        && before.st_ctime_nsec == after.st_ctime_nsec
}

pub fn observe(root: &Path, relative: &str, cap: usize) -> io::Result<Observation> {
    observe_inner(root, relative, cap, || {}, || {})
}

// Private fixture boundaries, with no-op production callbacks. Neither is a
// runtime flag or public hook; before_open tests pre-stat replacement refusal.
fn observe_inner(
    root: &Path,
    relative: &str,
    cap: usize,
    before_open: impl FnOnce(),
    after_read: impl FnOnce(),
) -> io::Result<Observation> {
    if cap > MOST_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "observation byte cap exceeds the hard bound",
        ));
    }
    let parts = components(relative)?;
    let directory_flags =
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let root = rustix::fs::open(root, directory_flags, Mode::empty())
        .map_err(|error| failure(error, "observation root cannot be opened"))?;
    let root = File::from(root);
    let root_identity = Identity::of(&metadata(&root)?);
    let mut directories = vec![(root, root_identity)];
    for (component, part) in parts[..parts.len() - 1].iter().enumerate() {
        let parent = &directories.last().unwrap().0;
        let child = match rustix::fs::openat(parent, *part, directory_flags, Mode::empty()) {
            Ok(fd) => File::from(fd),
            Err(rustix::io::Errno::NOENT) => {
                return finish(&directories, Entry::MissingAncestor { component });
            }
            Err(error) => {
                return Err(failure(error, "observation ancestor cannot be opened"));
            }
        };
        let identity = Identity::of(&metadata(&child)?);
        directories.push((child, identity));
    }
    let parent = &directories.last().unwrap().0;
    let leaf = parts[parts.len() - 1];
    let expected = match rustix::fs::statat(parent, leaf, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(rustix::io::Errno::NOENT) => return finish(&directories, Entry::MissingLeaf),
        Err(error) => return Err(failure(error, "observation leaf metadata is unavailable")),
    };
    if FileType::from_raw_mode(expected.st_mode) != FileType::RegularFile {
        return Err(refused("observation leaf is not a regular file"));
    }
    if expected.st_nlink != 1 {
        return Err(refused("observation leaf has an unsupported link count"));
    }
    if expected.st_size < 0 || expected.st_size > cap as i64 {
        return Err(refused("observation file exceeds its byte cap"));
    }
    before_open();
    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let file = match rustix::fs::openat(parent, leaf, flags, Mode::empty()) {
        Ok(fd) => File::from(fd),
        Err(rustix::io::Errno::NOENT) => {
            return Err(refused("observation leaf changed before reading"));
        }
        Err(error) => return Err(failure(error, "observation leaf cannot be opened")),
    };
    let meta = metadata(&file)?;
    if !meta.is_file() {
        return Err(refused("observation leaf is not a regular file"));
    }
    let before = Identity::of(&meta);
    let opened = rustix::fs::fstat(&file)
        .map_err(|error| failure(error, "opened observation metadata is unavailable"))?;
    if !same_stat(&expected, &opened) {
        return Err(refused("observation leaf changed before reading"));
    }
    if before.links != 1 {
        return Err(refused("observation leaf has an unsupported link count"));
    }
    if before.length > cap as u64 {
        return Err(refused("observation file exceeds its byte cap"));
    }
    // One extra byte detects growth without allowing read_to_end to grow its
    // allocation. The descriptor's validated length is already within cap.
    let mut bytes = vec![0; before.length as usize + 1];
    let mut used = 0;
    let mut reader = &file;
    while used < bytes.len() {
        match reader.read(&mut bytes[used..]) {
            Ok(0) => break,
            Ok(count) => used += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(failure(error, "observation file cannot be read")),
        }
    }
    bytes.truncate(used);
    after_read();
    let after = Identity::of(&metadata(&file)?);
    if before != after || bytes.len() as u64 != before.length {
        return Err(refused("observation file changed"));
    }
    if bytes.len() > cap {
        return Err(refused("observation file exceeds its byte cap"));
    }
    finish(
        &directories,
        Entry::Present {
            bytes,
            identity: after,
        },
    )
}

#[cfg(test)]
mod tests;
