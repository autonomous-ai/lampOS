//! A copy of lamp-update placed at a fixed root path, and run once before
//! it is used (#3689, docs/updates.md, "A fixed copy, checked").
//!
//! **Unwired:** no round or unit calls [`place`] yet. It is the step the
//! settings writer's refresh needs whichever way it is finished: a new copy
//! of the confirmed updater, written beside the fixed path, run once with
//! [`PROBE`], and renamed over the old copy only when it answers with its
//! own SHA-256. The copy it replaces may be kept first ([`place`]'s `kept`).
//!
//! The probe shows that the copy starts on this lamp and that its bytes
//! are the ones written. It does not show that its `back` or `apply` work:
//! those are proved by their own first use.
//!
//! The caller keeps other writers out of the folder while it runs: the
//! copy there is read once, and renamed over without being read again.
use crate::{
    manifest::sha256_hex,
    records::{sync_parent, write_whole},
};
use std::{
    fs, io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

/// The command a copy is run with once before it is used.
pub const PROBE: &str = "writer-probe";
/// The version of the probe's answer.
pub const PROTOCOL: u32 = 1;
/// How long the probe may take: it reads its own bytes and prints a line.
pub const PROBE_WITHIN: Duration = Duration::from_secs(20);
/// The most bytes a copy may have. lamp-update is about 10 MiB.
pub const MOST_BYTES: u64 = 64 << 20;

/// The one line a copy of `sha256` answers [`PROBE`] with.
pub fn answer(sha256: &str) -> String {
    format!("lamp-update {PROBE} {PROTOCOL} {sha256}")
}

/// This program's answer to [`PROBE`]: the SHA-256 of the file it runs from.
/// On Linux that is the running file itself (`/proc/self/exe`), even if
/// another has since been put at its path.
pub fn own_answer() -> Result<String, String> {
    Ok(answer(&sha256_hex(&own_bytes()?)))
}

/// The bytes of the file this program runs from: on Linux the running file
/// itself (`/proc/self/exe`), within [`MOST_BYTES`].
pub fn own_bytes() -> Result<Vec<u8>, String> {
    let exe = if cfg!(target_os = "linux") {
        PathBuf::from("/proc/self/exe")
    } else {
        std::env::current_exe().map_err(|error| format!("its own file: {error}"))?
    };
    read_bounded(&exe)
}

/// What [`place`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Placed {
    /// The fixed path already held these bytes: nothing was written.
    Already,
    /// The new copy is at the fixed path. `before` is the SHA-256 of the
    /// copy it replaced, none when there was none. `synced` is false when
    /// the folder could not be synced after the rename: the new copy is in
    /// place, though a power cut could still bring the old one back. It is
    /// not "nothing replaced" (jarvis's P3 on #4007).
    Replaced {
        before: Option<String>,
        synced: bool,
    },
}

/// Puts `bytes` at `path`, executable, only once a copy of them beside it
/// has answered `probe` with [`answer`] of their SHA-256.
///
/// With `kept`, the copy at `path` is first written there too, so that it
/// can be put back. A `kept` file already there is used only when it holds
/// the same bytes as `path`: a different one may be the only good copy
/// left, and nothing is replaced.
///
/// A copy that does not answer, or answers wrong, is removed and `path` is
/// left as it was. A cut at any step leaves `path` whole, old or new: the
/// new copy is renamed over it, and the folder synced.
pub fn place(
    path: &Path,
    bytes: &[u8],
    kept: Option<&Path>,
    probe: &mut dyn FnMut(&Path) -> Result<String, String>,
) -> Result<Placed, String> {
    if bytes.len() as u64 > MOST_BYTES {
        return Err(format!("the copy has more than {MOST_BYTES} bytes"));
    }
    let sha256 = sha256_hex(bytes);
    let before = existing(path)?;
    if let Some((old, mode)) = &before
        && sha256_hex(old) == sha256
        && mode & 0o111 != 0
    {
        return Ok(Placed::Already);
    }
    if let (Some(kept), Some((old, _))) = (kept, &before) {
        match existing(kept)? {
            Some((there, _)) if there != *old => {
                return Err(format!(
                    "{} holds another copy than {}: nothing was replaced",
                    kept.display(),
                    path.display()
                ));
            }
            _ => {}
        }
    }
    let candidate = beside(path)?;
    write_whole(&candidate, bytes, 0o755)
        .map_err(|error| format!("{}: {error}", candidate.display()))?;
    let answered = probe(&candidate);
    let wanted = answer(&sha256);
    match answered {
        Ok(line) if line.trim_end() == wanted => {}
        Ok(line) => {
            remove(&candidate);
            return Err(format!(
                "the new copy answered {:?}, not {wanted:?}: nothing was replaced",
                crate::world::clean_line(&line)
            ));
        }
        Err(why) => {
            remove(&candidate);
            return Err(format!(
                "the new copy did not answer ({why}): nothing was replaced"
            ));
        }
    }
    if let (Some(kept), Some((old, mode))) = (kept, &before)
        && let Err(error) = write_whole(kept, old, mode & 0o7777)
    {
        remove(&candidate);
        return Err(format!("{}: {error}: nothing was replaced", kept.display()));
    }
    if let Err(error) = fs::rename(&candidate, path) {
        remove(&candidate);
        return Err(format!("{}: {error}: nothing was replaced", path.display()));
    }
    Ok(Placed::Replaced {
        before: before.map(|(old, _)| sha256_hex(&old)),
        synced: sync_parent(path).is_ok(),
    })
}

/// Runs `path` with [`PROBE`] and returns what it printed: within
/// [`PROBE_WITHIN`], its process group owned and killed if it runs on.
pub fn run_probe(path: &Path) -> Result<String, String> {
    let output = crate::world::query::output_and_stderr_within(
        Command::new(path).arg(PROBE),
        Instant::now() + PROBE_WITHIN,
    )
    .map_err(|error| error.to_string())?;
    String::from_utf8(output).map_err(|_| "its answer is not text".to_owned())
}

/// The bytes and mode at `path`, none when nothing is there. Anything but
/// a regular file is refused, a link included.
fn existing(path: &Path) -> Result<Option<(Vec<u8>, u32)>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => {
            Ok(Some((read_bounded(path)?, metadata.permissions().mode())))
        }
        Ok(_) => Err(format!("{} is not a regular file", path.display())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(MOST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if bytes.len() as u64 > MOST_BYTES {
        return Err(format!(
            "{} has more than {MOST_BYTES} bytes",
            path.display()
        ));
    }
    Ok(bytes)
}

/// The new copy's name beside `path`: hidden, and named for this step.
fn beside(path: &Path) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .ok_or_else(|| format!("{} has no file name", path.display()))?;
    Ok(path.with_file_name(format!(".{}.new", name.to_string_lossy())))
}

fn remove(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = sync_parent(path);
}

#[cfg(test)]
mod tests;
