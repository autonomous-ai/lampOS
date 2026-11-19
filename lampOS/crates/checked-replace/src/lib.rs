//! A settings file replaced whole, checked by its own program first (#7108,
//! #7868): the new text goes to a new file beside the old one, whole and
//! synced; `check` runs on that file, and only `Ok` renames it over the old
//! one. Refused, the new file is removed and the old one is left byte for
//! byte. lamp-tasks', lamp-update's and lamp-setup's writers of OpenClaw's
//! `openclaw.json` all go through it, each with OpenClaw's own
//! `config validate --json` ([`validate`]) as the check.
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsFd, BorrowedFd};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rustix::fs::{AtFlags, Gid, Mode, OFlags, Uid};

/// The longest OpenClaw's config check runs before it counts as a
/// refusal: a cold start of its Node CLI on a lamp.
// ponytail: a fixed bound, unmeasured on a lamp; tune once a lamp's check
// is timed.
pub const VALIDATE_WITHIN: Duration = Duration::from_secs(60);

/// `bytes` in place of `path`, with `mode`, through a new file beside it
/// ([`replace_checked_in`], the file's owner the caller's).
pub fn replace_checked(
    path: &Path,
    bytes: &[u8],
    mode: Mode,
    check: &mut dyn FnMut(&Path) -> Result<(), String>,
) -> io::Result<()> {
    let folder = match path.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => folder,
        _ => Path::new("."),
    };
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("no file name"))?;
    let dir = rustix::fs::open(
        folder,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    replace_checked_in(dir.as_fd(), folder, name, bytes, mode, None, check)
}

/// `bytes` in place of `name` in the open folder `dir` (at `folder`), with
/// `mode` and, when given, `owner`: a new file `.NAME.<random>.new` beside
/// it, never through a link, whole and synced, then `check` on it (by its
/// path under `folder`), then renamed over `name` and the folder synced.
/// `check` refused, or any step failed, the new file is removed, `name`
/// is left as it was, and the error says why (the check's own words).
pub fn replace_checked_in(
    dir: BorrowedFd<'_>,
    folder: &Path,
    name: &OsStr,
    bytes: &[u8],
    mode: Mode,
    owner: Option<(Uid, Gid)>,
    check: &mut dyn FnMut(&Path) -> Result<(), String>,
) -> io::Result<()> {
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|_| io::Error::other("no random bytes"))?;
    let suffix: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    let mut temporary = OsString::from(".");
    temporary.push(name);
    temporary.push(format!(".{suffix}.new"));
    let written = (|| {
        let out = rustix::fs::openat(
            dir,
            temporary.as_os_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            mode,
        )?;
        if let Some((uid, gid)) = owner {
            rustix::fs::fchown(&out, Some(uid), Some(gid))?;
        }
        // The mode is set, not only asked for at creation: the umask
        // narrows that.
        rustix::fs::fchmod(&out, mode)?;
        let mut out = File::from(out);
        out.write_all(bytes)?;
        out.sync_all()?;
        check(&folder.join(&temporary)).map_err(io::Error::other)?;
        rustix::fs::renameat(dir, temporary.as_os_str(), dir, name)?;
        rustix::fs::fsync(dir)?;
        Ok(())
    })();
    if written.is_err() {
        let _ = rustix::fs::unlinkat(dir, temporary.as_os_str(), AtFlags::empty());
    }
    written
}

/// OpenClaw's own config check: `argv` (ending in `config validate
/// --json`, pointed at the new file by `OPENCLAW_CONFIG_PATH`) run with
/// `env` and nothing else, from `/`, in a process group of its own, killed
/// with it past `within`. `Ok` only on status 0; else what OpenClaw said,
/// on one line and short.
pub fn validate(argv: &[String], env: &[(&str, &str)], within: Duration) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let (program, args) = argv
        .split_first()
        .ok_or("OpenClaw's config check: no command")?;
    let (mut said_rx, said_tx) =
        io::pipe().map_err(|error| format!("OpenClaw's config check: no pipe: {error}"))?;
    let mut child = Command::new(program)
        .args(args)
        .env_clear()
        .envs(env.iter().copied())
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(
            said_tx
                .try_clone()
                .map_err(|error| format!("OpenClaw's config check: no pipe: {error}"))?,
        )
        .stderr(said_tx)
        .process_group(0)
        .spawn()
        .map_err(|error| format!("OpenClaw's config check did not start: {error}"))?;
    let (sent, said) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = said_rx.read_to_string(&mut text);
        let _ = sent.send(text);
    });
    let began = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if began.elapsed() < within => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let pid = rustix::process::Pid::from_child(&child);
                let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
                let _ = child.wait();
                break None;
            }
        }
    };
    // Bounded: a pipe still held by a process outside the group is given
    // up, never waited on.
    let said = said
        .recv_timeout(Duration::from_secs(2))
        .unwrap_or_default();
    let said: String = said
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(400)
        .collect();
    match status.map(|status| status.code()) {
        Some(Some(0)) => Ok(()),
        Some(code) => Err(format!(
            "OpenClaw's own config check refused it (status {}): {said}",
            code.map_or("killed".into(), |code| code.to_string())
        )),
        None => Err(format!(
            "OpenClaw's own config check ran past {} s",
            within.as_secs()
        )),
    }
}
