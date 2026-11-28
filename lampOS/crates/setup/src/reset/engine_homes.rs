//! Every other engine's home emptied at a reset (#7771): Claude Code's,
//! Codex's and any engine's `HOME` under [`ENGINE_HOMES`] holds its
//! transcripts, history, memory files and sign-ins, which Hermes's and
//! OpenClaw's step ([`super::forget_agents`]) does not reach. Not plan
//! paths: one code step covers any engine, and the plan keeps its slots.

use super::{AgentStep, empty, is_dir, open_dir};
use rustix::fs::{AtFlags, Mode, OFlags};
use std::{
    ffi::OsStr,
    io,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

/// The engines' homes' folder (`lamp_update::agent_ns::wrapper::HOMES`),
/// each engine's home `<engine>` in it.
pub const ENGINE_HOMES: &str = lamp_update::agent_ns::wrapper::HOMES;

/// Each engine's home in [`ENGINE_HOMES`] under `root` (`/` on a lamp)
/// emptied and kept, its owner and mode with it, as the namespace and the
/// next round expect it; anything else there removed, a link as a link.
/// [`ENGINE_HOMES`] itself stays. Missing, or reached through a link, it
/// is passed over. How many entries were emptied (those that held
/// anything) or removed.
pub fn forget_engine_homes(root: &Path) -> io::Result<usize> {
    walk(root, true, &mut Vec::new())
}

/// What [`forget_engine_homes`] would do under `root`, one step an entry,
/// changing nothing (`lamp-setup forget-engine-homes --dry-run`): the same
/// walk, only not acted on. Paths only, never what a home holds.
pub fn plan_engine_homes(root: &Path) -> io::Result<Vec<AgentStep>> {
    let mut steps = Vec::new();
    walk(root, false, &mut steps)?;
    Ok(steps)
}

fn walk(root: &Path, act: bool, steps: &mut Vec<AgentStep>) -> io::Result<usize> {
    if !root.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the reset's engines' homes' root must be absolute",
        ));
    }
    let folder = root.join(ENGINE_HOMES.trim_start_matches('/'));
    let dir = match open_dir(&folder) {
        Ok(dir) => dir,
        Err(rustix::io::Errno::NOENT | rustix::io::Errno::NOTDIR | rustix::io::Errno::LOOP) => {
            return Ok(0);
        }
        Err(error) => return Err(error.into()),
    };
    let mut names = Vec::new();
    for entry in rustix::fs::Dir::read_from(&dir)? {
        let name = entry?.file_name().to_owned();
        if !matches!(name.as_bytes(), b"." | b"..") {
            names.push(name);
        }
    }
    names.sort();
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut forgotten = 0;
    for name in names {
        let path: PathBuf = folder.join(OsStr::from_bytes(name.as_bytes()));
        let stat = match rustix::fs::statat(&dir, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => stat,
            Err(rustix::io::Errno::NOENT) => continue,
            Err(error) => return Err(error.into()),
        };
        if is_dir(&stat) {
            let home = rustix::fs::openat(&dir, name.as_c_str(), flags, Mode::empty())?;
            let held = rustix::fs::Dir::read_from(&home)?
                .filter_map(Result::ok)
                .any(|entry| !matches!(entry.file_name().to_bytes(), b"." | b".."));
            steps.push(AgentStep::Empty(path));
            if act && held {
                empty(&home)?;
                rustix::fs::fsync(&home)?;
            }
            forgotten += usize::from(held);
        } else {
            steps.push(AgentStep::Remove(path));
            if act {
                rustix::fs::unlinkat(&dir, name.as_c_str(), AtFlags::empty())?;
            }
            forgotten += 1;
        }
    }
    if act && forgotten > 0 {
        rustix::fs::fsync(&dir)?;
    }
    Ok(forgotten)
}
