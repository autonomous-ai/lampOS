//! The rule for a staged package: lamp-cloud (unprivileged) downloads it into
//! a folder lampos owns, and the helper reads that file, never a URL. Any
//! lampos process can put a file there; that is safe because the helper
//! checks the signature and the owner's yes is keyed to the package's
//! SHA-256, but the helper still refuses a file it should not trust the shape
//! of, and reads it once.
use std::{
    fs,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

/// The most a staged package is: the Store's own cap, 16 MiB of files and the
/// tar's headers (`lamp_skills::package::MOST_PACKAGE_BYTES`).
pub const MOST_STAGED: u64 = 18 * 1024 * 1024;
const MOST_FILE_NAME: usize = 128;

/// The path of a staged file: `given` must be an absolute path to a plain
/// file name directly under `root`, nothing nested, no `..`.
pub fn staged_path(root: &Path, given: &str) -> Result<PathBuf, String> {
    let path = Path::new(given);
    if !path.is_absolute() || path.parent() != Some(root) {
        return Err(format!("not a file directly under {}", root.display()));
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("not a plain file name")?;
    let plain = !name.is_empty()
        && name.len() <= MOST_FILE_NAME
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if !plain || given.ends_with('/') {
        return Err("not a plain file name".into());
    }
    Ok(root.join(name))
}

/// Opens it without following a link, reads it once, and refuses what is not
/// a regular file of `owner`'s (lampos's, passed in; never looked up here),
/// with one hard link, within [`MOST_STAGED`]. Every check is on the open
/// file, not on the path, so a swap after the open changes nothing.
pub fn read_staged(root: &Path, given: &str, owner: u32) -> Result<Vec<u8>, String> {
    let path = staged_path(root, given)?;
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let meta = file.metadata().map_err(|error| error.to_string())?;
    if !meta.is_file() {
        return Err("not a regular file".into());
    }
    if meta.uid() != owner {
        return Err("not owned by the lamp's own user".into());
    }
    if meta.nlink() != 1 {
        return Err("more than one hard link".into());
    }
    if meta.len() > MOST_STAGED {
        return Err(format!(
            "staged file size {} is over {MOST_STAGED}",
            meta.len()
        ));
    }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    (&mut file)
        .take(MOST_STAGED + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 != meta.len() {
        return Err("changed while it was read".into());
    }
    Ok(bytes)
}
