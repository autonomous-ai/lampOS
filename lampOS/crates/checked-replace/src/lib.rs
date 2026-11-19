//! A settings file replaced whole, checked by its own program first (#7108,
//! #7868): the new text goes to a new file beside the old one, whole and
//! synced; `check` runs on that file, and only `Ok` renames it over the old
//! one. Refused, the new file is removed and the old one is left byte for
//! byte. lamp-tasks', lamp-update's and lamp-setup's writers of OpenClaw's
//! `openclaw.json` all go through it, each with OpenClaw's own
//! `config validate --json` ([`validate`]) as the check, and each holds
//! the folder's lock ([`hold`], [`lock`]) from before its read until
//! after its rename, so no writer's rename lands inside another's check
//! and drops the other's keys (Seventh's P2 on #7872).
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rustix::fs::{AtFlags, Gid, Mode, OFlags, Uid};

/// The longest OpenClaw's config check runs before it counts as a
/// refusal: a cold start of its Node CLI on a lamp.
// ponytail: a fixed bound, unmeasured on a lamp; tune once a lamp's check
// is timed.
pub const VALIDATE_WITHIN: Duration = Duration::from_secs(60);

/// The longest a writer waits for another writer of the same folder to let
/// its lock go ([`lock`]): one checked write holds it at most about
/// [`VALIDATE_WITHIN`] (OpenClaw's check) and a few file steps.
// ponytail: a fixed bound over VALIDATE_WITHIN, unmeasured on a lamp.
pub const LOCK_WITHIN: Duration = Duration::from_secs(90);

/// `folder` opened and locked for one writer ([`lock`]): the lock is held
/// while the returned folder stays open, so a writer takes it before its
/// read and drops it after its rename.
pub fn hold(folder: &Path, within: Duration) -> io::Result<OwnedFd> {
    let dir = rustix::fs::open(
        folder,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    lock(dir.as_fd(), within)?;
    Ok(dir)
}

/// An exclusive `flock` on the open folder `dir` itself (Seventh's P2 on
/// #7872): every writer of a settings file in it that reads, merges, checks
/// and renames takes it first, so each one's read sees the other's rename.
/// The folder, not a file beside the settings: nothing to create, own or
/// leave behind, and a reset that removes every name a folder does not
/// keep cannot split it. Let go when every descriptor of that open folder
/// is closed (never inherited: `O_CLOEXEC`). Waited for at most `within`;
/// past it the write fails with why, as a check that could not run does.
pub fn lock(dir: BorrowedFd<'_>, within: Duration) -> io::Result<()> {
    use rustix::fs::FlockOperation;
    use rustix::io::Errno;
    let began = Instant::now();
    loop {
        match rustix::fs::flock(dir, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => return Ok(()),
            Err(Errno::INTR) => {}
            Err(Errno::WOULDBLOCK) if began.elapsed() < within => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(Errno::WOULDBLOCK) => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "another writer held the folder's lock past {} s",
                        within.as_secs()
                    ),
                ));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// What lamp-update's wrapper says when it finds no running gateway to
/// join, and fails closed.
pub const NO_GATEWAY: &str = "is not running in its own namespaces";
/// What it says joining a gateway just started whose namespace does not
/// show Node yet (re-exported as `lamp_update::openclaw_install::NODE_NOT_READY`).
pub const NODE_NOT_READY: &str = "'node': No such file or directory";
/// The waits before each new try of a write OpenClaw's gateway was not
/// there to check (lamp-tasks' `write_waiting`, lamp-update's one-shot
/// `gateway_block`), 5 min 15 s in all: lamp-tasks can
/// start before the gateway, at a boot or an update that restarts both
/// (#7268's HOLD), and a gateway takes 27-53 s to start on lamp-c0a4,
/// about 3 min with a reinstall beside it.
// ponytail: a fixed schedule; past it the write waits for lamp-tasks' next
// start (lamp-update's: its next round), as a refusal does.
pub const GATEWAY_WAITS: [Duration; 8] = [
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(20),
    Duration::from_secs(40),
    Duration::from_secs(60),
    Duration::from_secs(60),
    Duration::from_secs(60),
    Duration::from_secs(60),
];

/// Whether a write was refused only because OpenClaw's gateway was not
/// there to check it ([`NO_GATEWAY`], [`NODE_NOT_READY`]), not for its
/// text.
pub fn gateway_not_there(error: &str) -> bool {
    error.contains(NO_GATEWAY) || error.contains(NODE_NOT_READY)
}

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
    let said: String = redact(&said.split_whitespace().collect::<Vec<_>>().join(" "))
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

/// `said` with every value it quotes from a config masked as `***`
/// (Seventh's P3 on #7872: OpenClaw's words go to the journal): a quoted
/// value after a name holding `token`, `key`, `secret` or `password`
/// (`"token":"…"`, `apiKey: '…'`), any value after such a name and `=`
/// (`secret=…`), and any run of 24 or more letters, digits, `-` or `_` (a
/// token's shape). What it says of the config's shape stays.
pub fn redact(said: &str) -> String {
    const SECRET: [&str; 4] = ["token", "key", "secret", "password"];
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    let chars: Vec<char> = said.chars().collect();
    let len = chars.len();
    let mut out = String::with_capacity(said.len());
    let mut i = 0;
    while i < len {
        if !word(chars[i]) {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let start = i;
        while i < len && word(chars[i]) {
            i += 1;
        }
        if i - start >= 24 {
            out.push_str("***");
            continue;
        }
        out.extend(&chars[start..i]);
        let name = chars[start..i].iter().collect::<String>().to_lowercase();
        if !SECRET.into_iter().any(|secret| name.contains(secret)) {
            continue;
        }
        // The name's closing quote, spaces, then `:` or `=`, spaces.
        let mut j = i;
        if j < len && (chars[j] == '"' || chars[j] == '\'') {
            j += 1;
        }
        while j < len && chars[j] == ' ' {
            j += 1;
        }
        let Some(&sep) = chars.get(j).filter(|&&c| c == ':' || c == '=') else {
            continue;
        };
        j += 1;
        while j < len && chars[j] == ' ' {
            j += 1;
        }
        match chars.get(j) {
            Some(&quote) if quote == '"' || quote == '\'' => {
                out.extend(&chars[i..j]);
                out.push(quote);
                out.push_str("***");
                // An unclosed quote masks the rest.
                i = match chars[j + 1..].iter().position(|&c| c == quote) {
                    Some(at) => {
                        out.push(quote);
                        j + 1 + at + 1
                    }
                    None => len,
                };
            }
            Some(_) if sep == '=' => {
                out.extend(&chars[i..j]);
                out.push_str("***");
                i = (j..len)
                    .find(|&at| chars[at].is_whitespace() || ",;}])\"'".contains(chars[at]))
                    .unwrap_or(len);
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Seventh's P3 on #7872: OpenClaw's words go to the journal, so a
    /// value its error quotes from the config (a gateway token, a
    /// provider's key, a secret) comes out masked; the rest of what it
    /// said stays, so the reason can still be read.
    #[test]
    fn a_value_openclaws_error_quotes_is_masked() {
        let token = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let dir = std::env::temp_dir().join(format!("lamp-checked-replace-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let program = dir.join("openclaw");
        std::fs::write(
            &program,
            format!(
                "#!/bin/sh\necho 'gateway.auth.token: Invalid input: \"{token}\"'\necho '{{\"gateway\":{{\"auth\":{{\"token\":\"{token}\"}}}},\"apiKey\": \"sk-person-1\", secret=hunter2}}'\nexit 1\n"
            ),
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let argv = [program.to_string_lossy().into_owned()];
        let error = validate(&argv, &[], Duration::from_secs(10)).unwrap_err();
        std::fs::remove_dir_all(&dir).unwrap();
        for value in [token, "sk-person-1", "hunter2"] {
            assert!(!error.contains(value), "{value} in {error}");
        }
        assert!(
            error.contains("gateway.auth.token: Invalid input"),
            "{error}"
        );
        assert!(error.contains("status 1"), "{error}");
        assert!(error.contains("***"), "{error}");
    }
}
