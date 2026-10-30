//! Fail-closed identification of live voice output directories.
//!
//! The caller must inventory and pin candidate directories BEFORE this call,
//! and must never add or revisit candidates during deletion. The trusted voice
//! producer exclusively creates NEW_OUTPUT, so a later start cannot acquire an
//! existing candidate. This is not protection against arbitrary privileged
//! writers or a producer that changes that creation contract.

use crate::ProcessObservations;
use std::{collections::BTreeSet, io, path::Path};

/// Return protected direct-child session names, or refuse if ownership cannot
/// be established. One successful call protects one fixed, pinned inventory;
/// retain its entire returned set throughout that pass. Never reuse its result
/// with a refreshed inventory. No environment/config contents are inspected.
pub fn live_sessions(
    unit: &str,
    root: &Path,
    observations: &mut ProcessObservations,
) -> io::Result<BTreeSet<String>> {
    #[cfg(target_os = "linux")]
    {
        implementation::live(unit, root, observations)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (unit, root, observations);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "live session protection requires Linux systemd and host procfs",
        ))
    }
}

#[cfg(any(target_os = "linux", test))]
mod implementation {
    use crate::{ProcessObservations, ProcessSkipReason};
    use lamp_voice_processes::{Command as VoiceCommand, HELPERS, WorkerRole};
    use rustix::fs::{Dir, Mode, OFlags, openat, readlinkat_raw};
    use std::{
        collections::{BTreeMap, BTreeSet},
        ffi::{OsStr, OsString},
        fs::{self, File},
        io::{self, Read},
        os::{
            fd::{AsFd, OwnedFd},
            unix::ffi::{OsStrExt, OsStringExt},
        },
        path::{Component, Path, PathBuf},
        process::{Child, Command, Stdio},
        thread,
        time::{Duration, Instant},
    };

    const PROPERTIES: &str = "Id,LoadState,ActiveState,SubState,InvocationID,MainPID,ControlPID,Job,ExecStart,ControlGroup,NeedDaemonReload,Type";
    const MAX_PROCESSES: usize = 65_536;
    const MAX_PROCESS_BYTES: usize = 65_536;
    const MAX_FDS: usize = 65_536;
    /// Programs the voice starts inside its own group as helpers, which
    /// write no session folder of their own: the voice's own list.
    const VOICE_HELPERS: [&str; 1] = HELPERS;
    const MAX_LINK_BYTES: usize = 4096;
    const MAX_SHOW_BYTES: usize = 65_536;
    const TOTAL_BUDGET: Duration = Duration::from_secs(10);
    const COMMAND_BUDGET: Duration = Duration::from_secs(2);

    fn refused(message: &'static str) -> io::Error {
        io::Error::other(message)
    }

    /// A worker role not on the voice's list, named when it is a plain word,
    /// so the journal and lamp-health say which (#1071). Anything else from
    /// a process's arguments is not repeated.
    fn unknown_worker(word: Option<&str>) -> io::Error {
        match word.filter(|word| crate::report::plain_word(word)) {
            Some(word) => io::Error::other(format!("{} `{word}`", crate::report::UNKNOWN_WORKER)),
            None => refused("unknown voice worker command"),
        }
    }

    fn check_deadline(deadline: Instant) -> io::Result<()> {
        if Instant::now() >= deadline {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "live session protection deadline exceeded",
            ))
        } else {
            Ok(())
        }
    }

    fn session_id(value: &str) -> bool {
        value.len() == 32
            && value.bytes().all(|byte| byte.is_ascii_hexdigit())
            && value.bytes().any(|byte| byte != b'0')
    }

    fn valid_unit(unit: &str) -> bool {
        unit.len() > ".service".len()
            && unit.len() <= 255
            && unit.ends_with(".service")
            && !unit.starts_with('-')
            && !unit.ends_with("@.service")
            && unit
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.@:-".contains(&byte))
    }

    fn validate_root(root: &Path) -> io::Result<()> {
        if !root.is_absolute()
            || root.parent().is_none()
            || root.to_str().is_none()
            || root != fs::canonicalize(root)?
            || !root.is_dir()
        {
            return Err(refused(
                "session root is not an absolute canonical directory",
            ));
        }
        Ok(())
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct Snapshot {
        fields: BTreeMap<String, String>,
        invocation: String,
        main_pid: u32,
        executable: PathBuf,
        control_group: String,
    }

    impl Snapshot {
        /// The same, with the voice's binary as the file it is, through any
        /// links: a product unit names `/opt/lampOS/voice/current/lamp-voice`,
        /// and the running process's `exe` is the build that link points
        /// to. Compared as written they disagreed, and every run on a product
        /// lamp refused (Twelve on lamp-cb94, 17 October). A link switched to
        /// another build while the old one still runs still disagrees.
        fn resolved(mut self, resolve: impl Fn(&Path) -> io::Result<PathBuf>) -> io::Result<Self> {
            self.executable = resolve(&self.executable)?;
            if !self.executable.is_absolute()
                || self.executable.file_name() != Some(OsStr::new("lamp-voice"))
            {
                return Err(refused("the voice's binary resolves to another program"));
            }
            Ok(self)
        }

        fn parse(bytes: &[u8], unit: &str, root: &Path) -> io::Result<Self> {
            if !valid_unit(unit) || bytes.len() > MAX_SHOW_BYTES {
                return Err(refused("invalid unit or oversized manager response"));
            }
            let text =
                std::str::from_utf8(bytes).map_err(|_| refused("manager response is not UTF-8"))?;
            let expected: BTreeSet<_> = PROPERTIES.split(',').collect();
            let mut fields = BTreeMap::new();
            for line in text.lines().filter(|line| !line.is_empty()) {
                let (key, value) = line
                    .split_once('=')
                    .ok_or_else(|| refused("malformed manager property"))?;
                if !expected.contains(key)
                    || fields.insert(key.to_owned(), value.to_owned()).is_some()
                {
                    return Err(refused("unknown or duplicate manager property"));
                }
            }
            if fields.len() != expected.len() {
                return Err(refused("missing manager property"));
            }
            let get = |key: &str| fields.get(key).map(String::as_str).unwrap_or_default();
            if get("Id") != unit
                || get("LoadState") != "loaded"
                || get("Type") != "exec"
                || get("NeedDaemonReload") != "no"
            {
                return Err(refused(
                    "unit identity or loaded configuration is unverified",
                ));
            }
            if !matches!(
                get("ActiveState"),
                "active" | "reloading" | "activating" | "deactivating" | "inactive" | "failed"
            ) || get("SubState").is_empty()
            {
                return Err(refused("unknown service state"));
            }
            let invocation = get("InvocationID");
            if !session_id(invocation) || invocation.bytes().any(|byte| byte.is_ascii_uppercase()) {
                return Err(refused("manager did not provide a valid invocation ID"));
            }
            let main_pid = parse_pid(get("MainPID"), true)?;
            let _ = parse_pid(get("ControlPID"), true)?;
            // systemctl v255 prints an empty Job value for ID zero. Accept
            // numeric zero too, but never mistake arbitrary text for no job.
            if !get("Job").is_empty() {
                let _ = parse_pid(get("Job"), true)?;
            }
            if main_pid == 0 && get("ActiveState") == "active" {
                return Err(refused("active voice service has no main process"));
            }
            let control_group = get("ControlGroup");
            if (!control_group.is_empty() && !control_group.starts_with('/'))
                || control_group == "/"
                || Path::new(control_group)
                    .components()
                    .any(|part| !matches!(part, Component::RootDir | Component::Normal(_)))
                || (main_pid != 0 && control_group.is_empty())
            {
                return Err(refused("invalid service control group"));
            }
            let executable = loaded_executable(get("ExecStart"), root)?;
            Ok(Self {
                invocation: invocation.to_owned(),
                main_pid,
                executable,
                control_group: control_group.to_owned(),
                fields,
            })
        }
    }

    fn parse_pid(value: &str, zero_allowed: bool) -> io::Result<u32> {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(refused("invalid process identifier"));
        }
        let pid = value
            .parse::<u32>()
            .map_err(|_| refused("invalid process identifier"))?;
        if pid > i32::MAX as u32 || (!zero_allowed && pid == 0) {
            return Err(refused("invalid process identifier"));
        }
        Ok(pid)
    }

    // systemctl's ExecStart rendering is not a general shell language. Accept
    // only its unambiguous, single-command form used by the deployed unit.
    fn loaded_executable(value: &str, root: &Path) -> io::Result<PathBuf> {
        let value = value
            .strip_prefix("{ path=")
            .and_then(|value| value.strip_suffix(" }"))
            .ok_or_else(|| refused("unsupported loaded ExecStart format"))?;
        let (binary, rest) = value
            .split_once(" ; argv[]=")
            .ok_or_else(|| refused("loaded ExecStart has no argv"))?;
        let (arguments, status) = rest
            .split_once(" ; ")
            .ok_or_else(|| refused("unsupported loaded ExecStart argv"))?;
        if status.contains("argv[]=") || status.contains("{ path=") {
            return Err(refused(
                "multiple loaded ExecStart commands are unsupported",
            ));
        }
        let arguments: Vec<_> = arguments.split_ascii_whitespace().collect();
        let expected = format!("{}/${{INVOCATION_ID}}", root.display());
        let executable = PathBuf::from(binary);
        if !executable.is_absolute()
            || executable.file_name() != Some(OsStr::new("lamp-voice"))
            || arguments.len() < 4
            || arguments[0] != binary
            || arguments[1] != "serve"
            || !Path::new(arguments[2]).is_absolute()
            || arguments[3] != expected
            || arguments.iter().any(|argument| {
                argument
                    .bytes()
                    .any(|byte| matches!(byte, b'\\' | b'\'' | b'"' | b';'))
            })
        {
            return Err(refused(
                "loaded voice command does not use this invocation root",
            ));
        }
        Ok(executable)
    }

    fn show_command(unit: &str) -> io::Result<Command> {
        if !valid_unit(unit) {
            return Err(refused("an explicit service unit name is required"));
        }
        let mut command = Command::new("/usr/bin/systemctl");
        command
            .env_clear()
            .env("LC_ALL", "C")
            .env("SYSTEMD_COLORS", "0")
            .args([
                "--system",
                "--no-pager",
                "--no-ask-password",
                "show",
                "--all",
            ])
            .arg(format!("--property={PROPERTIES}"))
            .arg(unit);
        Ok(command)
    }

    struct Reap {
        child: Child,
        reaped: bool,
    }

    impl Drop for Reap {
        fn drop(&mut self) {
            if !self.reaped {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
    }

    // No reader threads or shell. Nonblocking stdout is drained with a cap;
    // every error/timeout kills and reaps the command. As with any local I/O,
    // an uninterruptible kernel wait can exceed the userspace deadline.
    fn run_bounded(command: &mut Command, deadline: Instant) -> io::Result<Vec<u8>> {
        check_deadline(deadline)?;
        let child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| refused("cannot start manager query"))?;
        let mut child = Reap {
            child,
            reaped: false,
        };
        let mut stdout = child
            .child
            .stdout
            .take()
            .ok_or_else(|| refused("manager query has no output pipe"))?;
        let flags = rustix::fs::fcntl_getfl(&stdout)?;
        rustix::fs::fcntl_setfl(&stdout, flags | OFlags::NONBLOCK)?;
        let mut output = Vec::new();
        let mut status = None;
        loop {
            check_deadline(deadline)?;
            let mut buffer = [0u8; 4096];
            let eof = loop {
                match stdout.read(&mut buffer) {
                    Ok(0) => break true,
                    Ok(count) => {
                        if output.len() + count > MAX_SHOW_BYTES {
                            return Err(refused("manager query output exceeds its limit"));
                        }
                        output.extend_from_slice(&buffer[..count]);
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break false,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(_) => return Err(refused("cannot read manager query output")),
                }
                check_deadline(deadline)?;
            };
            if status.is_none() {
                status = child.child.try_wait()?;
                child.reaped = status.is_some();
            }
            if eof && let Some(status) = status {
                return if status.success() {
                    Ok(output)
                } else {
                    Err(refused("manager query did not succeed"))
                };
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn directory_at(fd: impl AsFd, name: &OsStr) -> io::Result<OwnedFd> {
        Ok(openat(
            fd,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?)
    }

    fn read_at(fd: impl AsFd, name: &str, limit: usize) -> io::Result<Vec<u8>> {
        let descriptor = openat(
            fd,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        let mut bytes = Vec::new();
        File::from(descriptor)
            .take((limit + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(refused("process metadata exceeds its limit"));
        }
        Ok(bytes)
    }

    fn link_at(fd: impl AsFd, name: &OsStr) -> io::Result<PathBuf> {
        let mut bytes = [0u8; MAX_LINK_BYTES];
        let count = readlinkat_raw(fd, name, &mut bytes[..])?;
        if count == bytes.len() {
            return Err(refused("process link exceeds its limit"));
        }
        Ok(PathBuf::from(OsString::from_vec(bytes[..count].to_vec())))
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct Identity {
        pid: u32,
        start: u64,
        kernel_thread: bool,
        zombie: bool,
        exiting: bool,
        threads: u32,
    }

    fn identity(bytes: &[u8], expected_pid: u32) -> io::Result<Identity> {
        // comm is parenthesized and may itself contain spaces or ')'. Fields
        // following the LAST ')' have the stable proc stat field positions.
        let close = bytes
            .iter()
            .rposition(|byte| *byte == b')')
            .ok_or_else(|| refused("invalid process stat"))?;
        let open = bytes
            .iter()
            .position(|byte| *byte == b'(')
            .ok_or_else(|| refused("invalid process stat"))?;
        let prefix =
            std::str::from_utf8(&bytes[..open]).map_err(|_| refused("invalid process stat"))?;
        let pid = parse_pid(prefix.trim(), false)?;
        let rest = std::str::from_utf8(&bytes[close + 1..])
            .map_err(|_| refused("invalid process stat"))?;
        let fields: Vec<_> = rest.split_ascii_whitespace().collect();
        if pid != expected_pid || fields.len() < 20 {
            return Err(refused("process identity changed or stat is truncated"));
        }
        let flags = fields[6]
            .parse::<u64>()
            .map_err(|_| refused("invalid process flags"))?;
        let start = fields[19]
            .parse::<u64>()
            .map_err(|_| refused("invalid process start time"))?;
        let threads = fields[17]
            .parse::<u32>()
            .map_err(|_| refused("invalid process thread count"))?;
        Ok(Identity {
            pid,
            start,
            kernel_thread: flags & 0x20_0000 != 0,
            zombie: fields[0] == "Z" || fields[0] == "X",
            exiting: flags & 0x000004 != 0,
            threads,
        })
    }

    fn gone(error: &io::Error) -> bool {
        error.kind() == io::ErrorKind::NotFound
            || error.raw_os_error() == Some(rustix::io::Errno::SRCH.raw_os_error())
    }

    /// Only the process stat lookup establishes that a pinned process is gone.
    /// ENOENT for exe/cwd/fd/cmdline alone is not evidence of process exit.
    fn read_identity(process: &OwnedFd, pid: u32) -> io::Result<Identity> {
        identity(&read_at(process, "stat", 8192)?, pid)
    }

    fn zombie(before: &Identity) -> io::Result<bool> {
        if before.zombie && before.threads != 1 {
            return Err(refused("exited process leader still has other threads"));
        }
        Ok(before.zombie)
    }

    /// One final observation after a failed inspection. No delay or retry loop:
    /// hidden arguments, partial metadata and an alive process still refuse.
    fn after_failure(
        before: &Identity,
        mut latest: impl FnMut() -> io::Result<Identity>,
        mut cmdline: impl FnMut() -> io::Result<Vec<u8>>,
    ) -> io::Result<Option<ProcessSkipReason>> {
        let after = match latest() {
            Ok(after) => after,
            Err(error) if gone(&error) => return Ok(Some(ProcessSkipReason::Gone)),
            Err(error) => return Err(error),
        };
        if before.pid != after.pid
            || before.start != after.start
            || before.kernel_thread != after.kernel_thread
        {
            return Err(refused("process identity changed during exit check"));
        }
        if after.kernel_thread {
            return Ok(None);
        }
        if after.threads == 0 {
            return Err(refused("process thread count is unavailable"));
        }
        if zombie(&after)? {
            return Ok(Some(ProcessSkipReason::Zombie));
        }
        // PF_EXITING is set before exit_mm and exit_files in Linux do_exit.
        // Require an empty command line as well. A surviving thread can still
        // own the session even if its group leader has already exited.
        if after.exiting && after.threads == 1 {
            return match cmdline() {
                Ok(bytes) if bytes.is_empty() => Ok(Some(ProcessSkipReason::Exiting)),
                Err(error)
                    if error.raw_os_error() == Some(rustix::io::Errno::SRCH.raw_os_error()) =>
                {
                    Ok(Some(ProcessSkipReason::Gone))
                }
                _ => Ok(None),
            };
        }
        Ok(None)
    }

    fn arguments(bytes: &[u8]) -> io::Result<Vec<OsString>> {
        if bytes.is_empty() {
            return Ok(Vec::new());
        }
        if !bytes.ends_with(&[0]) {
            return Err(refused("process arguments are truncated"));
        }
        let arguments: Vec<_> = bytes[..bytes.len() - 1]
            .split(|byte| *byte == 0)
            .map(|word| OsString::from_vec(word.to_vec()))
            .collect();
        if arguments.len() > 1024 || arguments[0].is_empty() {
            return Err(refused("process arguments are ambiguous"));
        }
        Ok(arguments)
    }

    fn executable_path(path: PathBuf) -> PathBuf {
        let bytes = path.as_os_str().as_bytes();
        PathBuf::from(OsStr::from_bytes(
            bytes.strip_suffix(b" (deleted)").unwrap_or(bytes),
        ))
    }

    fn in_cgroup(bytes: &[u8], group: &str) -> io::Result<bool> {
        let text =
            std::str::from_utf8(bytes).map_err(|_| refused("invalid process control group"))?;
        let mut found = false;
        for line in text.lines() {
            let (_, rest) = line
                .split_once(':')
                .ok_or_else(|| refused("invalid process control group"))?;
            let (_, path) = rest
                .split_once(':')
                .ok_or_else(|| refused("invalid process control group"))?;
            if !group.is_empty()
                && (path == group
                    || path
                        .strip_prefix(group)
                        .is_some_and(|suffix| suffix.starts_with('/')))
            {
                found = true;
            }
        }
        Ok(found)
    }

    fn resolve_output(path: &Path, cwd: &Path) -> io::Result<PathBuf> {
        if path.as_os_str().as_bytes().len() > MAX_LINK_BYTES {
            return Err(refused("voice output path exceeds its limit"));
        }
        let absolute = if path.is_absolute() {
            path.to_owned()
        } else {
            if !cwd.is_absolute() {
                return Err(refused("voice working directory is not absolute"));
            }
            cwd.join(path)
        };
        // A new session may not yet exist. Resolve the nearest existing parent
        // so an alias to the configured root cannot hide a live session.
        let mut parent = absolute.as_path();
        let mut missing = Vec::new();
        loop {
            match fs::canonicalize(parent) {
                Ok(mut resolved) => {
                    for name in missing.into_iter().rev() {
                        resolved.push(name);
                    }
                    return Ok(resolved);
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    if missing.len() >= 64 {
                        return Err(refused("voice output path has too many missing ancestors"));
                    }
                    let name = parent
                        .file_name()
                        .ok_or_else(|| refused("voice output path cannot be resolved"))?;
                    missing.push(name);
                    parent = parent
                        .parent()
                        .ok_or_else(|| refused("voice output path cannot be resolved"))?;
                }
                Err(_) => return Err(refused("voice output path cannot be resolved")),
            }
        }
    }

    fn protect_path(path: &Path, root: &Path, sessions: &mut BTreeSet<String>) -> io::Result<()> {
        let Ok(relative) = path.strip_prefix(root) else {
            return Ok(());
        };
        let Some(Component::Normal(name)) = relative.components().next() else {
            return Err(refused("live process references the whole sessions root"));
        };
        let name = name
            .to_str()
            .ok_or_else(|| refused("live process uses an unknown session name"))?;
        if !session_id(name) {
            return Err(refused("live process uses an unknown session name"));
        }
        // Preserve the actual pathname's case, unlike a normalized UUID parser.
        sessions.insert(name.to_owned());
        Ok(())
    }

    fn voice_paths(arguments: &[OsString]) -> io::Result<Vec<&Path>> {
        let command = arguments
            .get(1)
            .and_then(|word| word.to_str())
            .ok_or_else(|| refused("voice process command is ambiguous"))?;
        // The voice's own list (lamp-voice-processes, #1071): a command or a
        // worker role the voice can start is one this guard knows.
        let command =
            VoiceCommand::parse(command).ok_or_else(|| refused("unknown voice process command"))?;
        let output = match command {
            VoiceCommand::Serve => Some(3),
            VoiceCommand::Directed => Some(4),
            VoiceCommand::DirectedFixture => Some(5),
            VoiceCommand::CameraInspect => Some(4),
            VoiceCommand::ProviderCheck => None,
            // Read-only reports run by hand on a lamp: no output argument,
            // any session file they hold open still protected below.
            VoiceCommand::ShortTurns
            | VoiceCommand::SttCheck
            | VoiceCommand::Marked
            | VoiceCommand::TurnTimes
            | VoiceCommand::Stalls
            | VoiceCommand::ProbeCascade
            | VoiceCommand::ProbeCache
            | VoiceCommand::ProbeTools
            | VoiceCommand::ProbeReminders => None,
            // The reminders' consumer the voice starts beside itself when
            // LAMP_SCHEDULE_TOOLS is on: no output argument, its open files
            // still protected below. On lamp-4ace from 2026-10-15 02:05 to
            // 20:00 it refused every check.
            VoiceCommand::ScheduleConsumer if arguments.len() == 2 => None,
            VoiceCommand::ScheduleConsumer => {
                return Err(refused("unknown voice process command"));
            }
            // The voice's workers, by its own list: `second` (the second
            // voice's worker) was missing from a copy kept here before, and
            // refused every hourly check on lamp-4ace from 2026-10-15 14:01.
            VoiceCommand::Worker => {
                let word = arguments.get(2).and_then(|word| word.to_str());
                let role = word.and_then(WorkerRole::parse);
                if arguments.len() < 6 {
                    return Err(refused("unknown voice worker command"));
                }
                let Some(role) = role else {
                    return Err(unknown_worker(word));
                };
                let mut paths = Vec::new();
                if matches!(role, WorkerRole::Capture | WorkerRole::Speaker) {
                    for (index, value) in arguments.iter().enumerate().skip(6) {
                        if value == "--diagnostics" {
                            let value = arguments
                                .get(index + 1)
                                .filter(|value| !value.is_empty())
                                .ok_or_else(|| refused("voice diagnostics path is missing"))?;
                            paths.push(Path::new(value));
                        }
                    }
                    if paths.len() > 1 {
                        return Err(refused("voice diagnostics path is ambiguous"));
                    }
                }
                return Ok(paths);
            }
        };
        output
            .map(|index| {
                arguments
                    .get(index)
                    .filter(|value| !value.is_empty())
                    .map(|value| vec![Path::new(value)])
                    .ok_or_else(|| refused("voice process output argument is missing"))
            })
            .unwrap_or_else(|| Ok(Vec::new()))
    }

    struct Scan {
        sessions: BTreeSet<String>,
        main_found: bool,
        fds: usize,
    }

    fn inspect_fds(
        process: &OwnedFd,
        root: &Path,
        scan: &mut Scan,
        deadline: Instant,
    ) -> io::Result<()> {
        let fd_directory = directory_at(process, OsStr::new("fd"))?;
        for entry in Dir::read_from(&fd_directory)? {
            check_deadline(deadline)?;
            let entry = entry?;
            let name = entry.file_name().to_bytes();
            if matches!(name, b"." | b"..") {
                continue;
            }
            scan.fds += 1;
            if scan.fds > MAX_FDS {
                return Err(refused("process descriptor inventory exceeds its limit"));
            }
            let path = match link_at(&fd_directory, OsStr::from_bytes(name)) {
                Ok(path) => executable_path(path),
                // A descriptor can close between readdir and readlink. Its
                // disappearance cannot keep a file alive; argv protection also
                // covers the writer opening descriptors during startup.
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(_) => return Err(refused("cannot inspect live process descriptors")),
            };
            if path.is_absolute() {
                protect_path(&path, root, &mut scan.sessions)?;
            }
        }
        Ok(())
    }

    fn inspect_live_process(
        process: &OwnedFd,
        pid: u32,
        snapshot: &Snapshot,
        root: &Path,
        scan: &mut Scan,
        deadline: Instant,
        before: &Identity,
    ) -> io::Result<()> {
        let argv_bytes = read_at(process, "cmdline", MAX_PROCESS_BYTES)?;
        let argv = arguments(&argv_bytes)?;
        if argv.is_empty() {
            return Err(refused("live process has no inspectable arguments"));
        }
        let executable = executable_path(link_at(process, OsStr::new("exe"))?);
        let groups = read_at(process, "cgroup", 8192)?;
        let same_group = in_cgroup(&groups, &snapshot.control_group)?;
        let named_voice = executable.file_name() == Some(OsStr::new("lamp-voice"))
            || Path::new(&argv[0]).file_name() == Some(OsStr::new("lamp-voice"))
            || executable == snapshot.executable;
        // Many independent lamp components have a `serve` command. Only an
        // identified voice executable gets the voice argv interpretation.
        let relevant = named_voice || same_group || pid == snapshot.main_pid;
        // The voice's own helpers, started by it inside its group: known by
        // name, given no voice argv reading, and their open files still
        // protected below. On lamp-4ace on 2026-10-14 the end-of-turn
        // worker refused every check once the voice ran it.
        let voice_helper = !named_voice
            && same_group
            && pid != snapshot.main_pid
            && [executable.file_name(), Path::new(&argv[0]).file_name()]
                .into_iter()
                .flatten()
                .any(|name| {
                    VOICE_HELPERS
                        .iter()
                        .any(|helper| name == OsStr::new(helper))
                });
        let mut direct_reference = false;
        for argument in &argv {
            let path = Path::new(argument);
            if path.is_absolute() && path.starts_with(root) {
                protect_path(path, root, &mut scan.sessions)?;
                direct_reference = true;
            }
        }
        if relevant && !voice_helper {
            if !named_voice {
                return Err(refused("unrecognized process shares voice ownership"));
            }
            let cwd = executable_path(link_at(process, OsStr::new("cwd"))?);
            let paths = voice_paths(&argv)?;
            let mut resolved = Vec::new();
            for path in paths {
                let path = resolve_output(path, &cwd)?;
                protect_path(&path, root, &mut scan.sessions)?;
                resolved.push(path);
            }
            if pid == snapshot.main_pid {
                if executable != snapshot.executable
                    || argv.get(1) != Some(&OsString::from("serve"))
                    || resolved != [root.join(&snapshot.invocation)]
                    || !same_group
                {
                    return Err(refused(
                        "main process disagrees with manager invocation mapping",
                    ));
                }
                scan.main_found = true;
            }
        }
        if relevant || direct_reference {
            // A renamed manual binary or a reader can be protected through its
            // explicit root reference without assuming any particular argv
            // layout. Unrelated services are not parsed as voice processes.
            inspect_fds(process, root, scan, deadline)?;
        }
        let after = read_identity(process, pid)?;
        if before.pid != after.pid
            || before.start != after.start
            || before.kernel_thread != after.kernel_thread
            || before.zombie != after.zombie
            || before.exiting != after.exiting
            || after.threads == 0
            || argv_bytes != read_at(process, "cmdline", MAX_PROCESS_BYTES)?
            || executable != executable_path(link_at(process, OsStr::new("exe"))?)
            || groups != read_at(process, "cgroup", 8192)?
        {
            return Err(refused(
                "process identity or command changed during protection check",
            ));
        }
        Ok(())
    }

    /// How many more times, and how far apart, a process still live but
    /// not inspectable after its one-shot exit check is looked at again
    /// before it refuses the root. A process ending with more than one
    /// thread, whose leader has let go of its memory, reads an empty
    /// `cmdline` for a few milliseconds, as does one between `execve` and
    /// its new arguments. Then it is gone, a zombie, or readable. On
    /// lamp-4ace on 17 October one such process failed every run, so the
    /// voice's sessions were never trimmed (the first builder on #705).
    const SETTLE_LOOKS: u32 = 3;
    const SETTLE_WAIT: Duration = Duration::from_millis(20);

    fn inspect_process(
        process: &OwnedFd,
        pid: u32,
        snapshot: &Snapshot,
        root: &Path,
        scan: &mut Scan,
        observations: &mut ProcessObservations,
        deadline: Instant,
    ) -> io::Result<()> {
        inspect_process_settling(
            process,
            pid,
            snapshot,
            root,
            scan,
            observations,
            deadline,
            &mut || thread::sleep(SETTLE_WAIT),
        )
    }

    /// [`inspect_process`], with `settle` between looks at a process that
    /// is still live and not inspectable: at most [`SETTLE_LOOKS`] more. One
    /// gone, a zombie or exiting by then is skipped, as on the first look;
    /// one readable is inspected as any live process; one still live and
    /// unreadable refuses.
    #[allow(clippy::too_many_arguments)]
    fn inspect_process_settling(
        process: &OwnedFd,
        pid: u32,
        snapshot: &Snapshot,
        root: &Path,
        scan: &mut Scan,
        observations: &mut ProcessObservations,
        deadline: Instant,
        settle: &mut dyn FnMut(),
    ) -> io::Result<()> {
        let mut looks = 0;
        loop {
            match inspect_once(process, pid, snapshot, root, scan, observations, deadline)? {
                None => return check_deadline(deadline),
                Some(_) if looks < SETTLE_LOOKS => {
                    looks += 1;
                    settle();
                    check_deadline(deadline)?;
                }
                Some(error) => return Err(error),
            }
        }
    }

    /// One look at a process: done (inspected, or skipped as gone, a
    /// zombie, exiting or a kernel thread), or the error of a process still
    /// live and not inspectable after its one-shot exit check.
    fn inspect_once(
        process: &OwnedFd,
        pid: u32,
        snapshot: &Snapshot,
        root: &Path,
        scan: &mut Scan,
        observations: &mut ProcessObservations,
        deadline: Instant,
    ) -> io::Result<Option<io::Error>> {
        check_deadline(deadline)?;
        let before = match read_identity(process, pid) {
            Ok(before) => before,
            Err(error) if gone(&error) => {
                skipped(pid, snapshot, observations, ProcessSkipReason::Gone)?;
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        // Preserve the existing explicit PF_KTHREAD exclusion. Empty argv is
        // never enough to classify a process as a kernel thread or as exited.
        if before.kernel_thread {
            observations.kernel_threads += 1;
            return if pid == snapshot.main_pid {
                Err(refused("voice main process is a kernel thread"))
            } else {
                Ok(None)
            };
        }
        let inspection = if before.threads == 0 {
            // do_task_stat can leave num_threads at zero if it cannot lock
            // sighand while the task is being reaped. Zero alone proves
            // nothing; route it through the same one-shot exit observation.
            Err(refused("process thread count is unavailable"))
        } else {
            match zombie(&before) {
                Ok(true) => {
                    skipped(pid, snapshot, observations, ProcessSkipReason::Zombie)?;
                    return Ok(None);
                }
                Ok(false) => {
                    inspect_live_process(process, pid, snapshot, root, scan, deadline, &before)
                }
                Err(error) => Err(error),
            }
        };
        let Err(error) = inspection else {
            return Ok(None);
        };
        check_deadline(deadline)?;
        match after_failure(
            &before,
            || read_identity(process, pid),
            || read_at(process, "cmdline", MAX_PROCESS_BYTES),
        )? {
            Some(reason) => {
                skipped(pid, snapshot, observations, reason)?;
                Ok(None)
            }
            None => Ok(Some(error)),
        }
    }

    fn skipped(
        pid: u32,
        snapshot: &Snapshot,
        observations: &mut ProcessObservations,
        reason: ProcessSkipReason,
    ) -> io::Result<()> {
        observations.record(pid, reason);
        if pid == snapshot.main_pid {
            Err(refused("voice main process is no longer live"))
        } else {
            Ok(())
        }
    }

    fn scan_processes(
        proc_root: &Path,
        snapshot: &Snapshot,
        root: &Path,
        deadline: Instant,
        observations: &mut ProcessObservations,
    ) -> io::Result<BTreeSet<String>> {
        let proc_root = directory_at(rustix::fs::CWD, proc_root.as_os_str())?;
        let mut scan = Scan {
            sessions: BTreeSet::from([snapshot.invocation.clone()]),
            main_found: snapshot.main_pid == 0,
            fds: 0,
        };
        let mut count = 0;
        for entry in Dir::read_from(&proc_root)? {
            check_deadline(deadline)?;
            let entry = entry?;
            let name = entry.file_name().to_bytes();
            if name.is_empty() || !name.iter().all(u8::is_ascii_digit) {
                continue;
            }
            count += 1;
            if count > MAX_PROCESSES {
                return Err(refused("process inventory exceeds its limit"));
            }
            let pid = parse_pid(
                std::str::from_utf8(name).map_err(|_| refused("invalid process directory name"))?,
                false,
            )?;
            let process = match directory_at(&proc_root, OsStr::from_bytes(name)) {
                Ok(process) => process,
                Err(error) if gone(&error) => {
                    skipped(pid, snapshot, observations, ProcessSkipReason::Gone)?;
                    continue;
                }
                Err(_) => return Err(refused("cannot pin a process directory")),
            };
            inspect_process(
                &process,
                pid,
                snapshot,
                root,
                &mut scan,
                observations,
                deadline,
            )
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("process {pid} protection is incomplete: {error}"),
                )
            })?;
        }
        if !scan.main_found {
            return Err(refused("manager main process was not found in host procfs"));
        }
        Ok(scan.sessions)
    }

    // The closures make ordering, restart races, and query failures testable
    // without systemctl, a Linux process tree, or a lamp.
    fn guard_with(
        mut snapshot: impl FnMut() -> io::Result<Snapshot>,
        mut inspect: impl FnMut(&Snapshot) -> io::Result<BTreeSet<String>>,
    ) -> io::Result<BTreeSet<String>> {
        let before = snapshot()?;
        let mut sessions = inspect(&before)?;
        // A finite diagnostic child can be born while /proc is enumerated and
        // outlive its coordinator. A second pass catches a surviving child
        // missed by the moving PID iterator. These trusted audio workers do
        // not spawn further record-writing processes. Keep both passes' sets.
        sessions.extend(inspect(&before)?);
        let after = snapshot()?;
        if before != after {
            return Err(refused("voice unit changed during protection check"));
        }
        sessions.insert(before.invocation);
        Ok(sessions)
    }

    #[cfg(target_os = "linux")]
    pub(super) fn live(
        unit: &str,
        root: &Path,
        observations: &mut ProcessObservations,
    ) -> io::Result<BTreeSet<String>> {
        let deadline = Instant::now() + TOTAL_BUDGET;
        validate_root(root)?;
        if !rustix::process::geteuid().is_root() {
            return Err(refused(
                "live session protection requires root process visibility",
            ));
        }
        let proc_root = directory_at(rustix::fs::CWD, OsStr::new("/proc"))?;
        let init = directory_at(&proc_root, OsStr::new("1"))?;
        let own_name = std::process::id().to_string();
        let own = directory_at(&proc_root, OsStr::new(&own_name))?;
        if read_at(&init, "comm", 128)? != b"systemd\n"
            || link_at(&init, OsStr::new("ns/pid"))? != link_at(&own, OsStr::new("ns/pid"))?
        {
            return Err(refused("host systemd process namespace is not established"));
        }
        // Deployment must supply the host procfs. Matching PID namespaces does
        // not distinguish a whole systemd container from the physical host.
        guard_with(
            || {
                let mut command = show_command(unit)?;
                let bytes =
                    run_bounded(&mut command, deadline.min(Instant::now() + COMMAND_BUDGET))?;
                Snapshot::parse(&bytes, unit, root)?.resolved(|path| fs::canonicalize(path))
            },
            |snapshot| scan_processes(Path::new("/proc"), snapshot, root, deadline, observations),
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{
            os::unix::fs::symlink,
            sync::atomic::{AtomicU64, Ordering},
        };

        const CURRENT: &str = "11111111111111111111111111111111";
        const OLD: &str = "22222222222222222222222222222222";
        const MANUAL: &str = "33333333333333333333333333333333";
        const BINARY: &str = "/opt/releases/example/lamp-voice";
        static NEXT: AtomicU64 = AtomicU64::new(0);

        struct Tree(PathBuf);

        impl Tree {
            fn new() -> Self {
                let path = std::env::temp_dir().join(format!(
                    "lamp-records-guard-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                fs::create_dir(&path).unwrap();
                Self(fs::canonicalize(path).unwrap())
            }

            fn directory(&self, name: &str) -> PathBuf {
                let path = self.0.join(name);
                fs::create_dir_all(&path).unwrap();
                path
            }
        }

        impl Drop for Tree {
            fn drop(&mut self) {
                fs::remove_dir_all(&self.0).unwrap();
            }
        }

        fn response(root: &Path) -> String {
            format!(
                "Id=lamp-voice.service\nLoadState=loaded\nActiveState=active\nSubState=running\nInvocationID={CURRENT}\nMainPID=123\nControlPID=0\nJob=\nExecStart={{ path={BINARY} ; argv[]={BINARY} serve /private/config.json {}/${{INVOCATION_ID}} --microphone mic2 ; ignore_errors=no ; pid=123 ; code=(null) ; status=0/0 }}\nControlGroup=/system.slice/lamp-voice.service\nNeedDaemonReload=no\nType=exec\n",
                root.display()
            )
        }

        fn snapshot(root: &Path) -> Snapshot {
            Snapshot::parse(response(root).as_bytes(), "lamp-voice.service", root).unwrap()
        }

        fn stat(pid: u32, start: u64, state: &str) -> String {
            stat_with(pid, start, state, 0, 1)
        }

        fn stat_with(pid: u32, start: u64, state: &str, flags: u64, threads: u32) -> String {
            let mut fields = vec!["0".to_owned(); 20];
            fields[0] = state.to_owned();
            fields[6] = flags.to_string();
            fields[17] = threads.to_string();
            fields[19] = start.to_string();
            format!("{pid} (lamp ) voice) {}\n", fields.join(" "))
        }

        fn process(tree: &Tree, pid: u32, words: &[&OsStr], group: &str) -> PathBuf {
            let path = tree.directory(&format!("proc/{pid}"));
            fs::write(path.join("stat"), stat(pid, 44, "S")).unwrap();
            let mut argv = Vec::new();
            for word in words {
                argv.extend_from_slice(word.as_bytes());
                argv.push(0);
            }
            fs::write(path.join("cmdline"), argv).unwrap();
            fs::write(path.join("cgroup"), format!("0::{group}\n")).unwrap();
            symlink(BINARY, path.join("exe")).unwrap();
            symlink(&tree.0, path.join("cwd")).unwrap();
            fs::create_dir(path.join("fd")).unwrap();
            path
        }

        fn serve(tree: &Tree, pid: u32, output: &Path, group: &str) -> PathBuf {
            process(
                tree,
                pid,
                &[
                    OsStr::new(BINARY),
                    OsStr::new("serve"),
                    OsStr::new("/private/config.json"),
                    output.as_os_str(),
                ],
                group,
            )
        }

        #[test]
        fn manager_identity_and_invocation_are_strict() {
            let root = Path::new("/sessions");
            let source = response(root);
            let parsed = snapshot(root);
            assert_eq!(parsed.invocation, CURRENT);
            for broken in [
                source.replace(CURRENT, "00000000000000000000000000000000"),
                source.replace(CURRENT, "../session"),
                source.replace("LoadState=loaded", "LoadState=not-found"),
                source.replace("MainPID=123", "MainPID=0"),
                source.replace("NeedDaemonReload=no", "NeedDaemonReload=yes"),
                source.replace("ActiveState=active", "ActiveState=new-unknown-state"),
                source.replace("Type=exec", "Type=forking"),
                source.replace("Job=\n", "Job=unknown\n"),
                source.replace("InvocationID=", "Missing="),
                format!("{source}MainPID=123\n"),
                source.replace("/${INVOCATION_ID}", "/fixed-directory"),
            ] {
                assert!(Snapshot::parse(broken.as_bytes(), "lamp-voice.service", root).is_err());
            }
        }

        #[test]
        fn retained_invocation_is_protected_in_all_known_states() {
            let root = Path::new("/sessions");
            for state in [
                "inactive",
                "failed",
                "activating",
                "deactivating",
                "reloading",
            ] {
                let text = response(root)
                    .replace("ActiveState=active", &format!("ActiveState={state}"))
                    .replace("MainPID=123", "MainPID=0");
                let parsed = Snapshot::parse(text.as_bytes(), "lamp-voice.service", root).unwrap();
                let result = guard_with(|| Ok(parsed.clone()), |_| Ok(BTreeSet::new())).unwrap();
                assert!(result.contains(CURRENT));
            }
        }

        #[test]
        fn any_manager_change_or_failure_refuses() {
            let first = snapshot(Path::new("/sessions"));
            let mut next = first.clone();
            next.invocation = OLD.to_owned();
            let mut calls = 0;
            assert!(
                guard_with(
                    || {
                        calls += 1;
                        Ok(if calls == 1 {
                            first.clone()
                        } else {
                            next.clone()
                        })
                    },
                    |_| Ok(BTreeSet::new()),
                )
                .is_err()
            );
            assert!(guard_with(|| Err(refused("unavailable")), |_| unreachable!()).is_err());
            assert!(guard_with(|| Ok(first.clone()), |_| Err(refused("unreadable"))).is_err());
        }

        #[test]
        fn both_process_passes_keep_their_protected_paths() {
            let snapshot = snapshot(Path::new("/sessions"));
            let mut calls = 0;
            let got = guard_with(
                || Ok(snapshot.clone()),
                |_| {
                    calls += 1;
                    Ok(BTreeSet::from([
                        if calls == 1 { OLD } else { MANUAL }.to_owned()
                    ]))
                },
            )
            .unwrap();
            assert_eq!(calls, 2);
            assert_eq!(
                got,
                BTreeSet::from([CURRENT.to_owned(), OLD.to_owned(), MANUAL.to_owned()])
            );
        }

        #[test]
        fn unit_argument_cannot_be_a_pattern_or_option() {
            for unit in [
                "*",
                "*.service",
                "--help",
                "../x.service",
                "lamp@.service",
                "",
            ] {
                assert!(show_command(unit).is_err());
            }
            let command = show_command("lamp-voice.service").unwrap();
            let arguments: Vec<_> = command.get_args().collect();
            assert!(arguments.contains(&OsStr::new("--system")));
            assert!(arguments.contains(&OsStr::new("--all")));
            assert_eq!(arguments.last(), Some(&OsStr::new("lamp-voice.service")));
        }

        /// Twelve on lamp-cb94 (17 Oct, 08:53 UTC): a product unit starts the
        /// voice through its `current` link, and the main process's `exe`
        /// is the file that link points to. Compared as written, every run
        /// refused ("main process disagrees with manager invocation
        /// mapping"); with the unit's binary resolved, its own main process
        /// is found. Once the link points to another build while the old one
        /// still runs, it disagrees again and refuses.
        #[test]
        fn a_voice_started_through_a_current_link_is_its_own_main_process() {
            let tree = Tree::new();
            let root = tree.directory("sessions");
            let build = tree.directory("voice/one");
            fs::write(build.join("lamp-voice"), b"").unwrap();
            symlink(&build, tree.0.join("voice/current")).unwrap();
            let named = tree.0.join("voice/current/lamp-voice");
            let text = response(&root).replace(BINARY, named.to_str().unwrap());
            let written = Snapshot::parse(text.as_bytes(), "lamp-voice.service", &root).unwrap();
            let main = serve(
                &tree,
                123,
                &root.join(CURRENT),
                "/system.slice/lamp-voice.service",
            );
            // The kernel's `exe` is the resolved file.
            fs::remove_file(main.join("exe")).unwrap();
            let running = fs::canonicalize(build.join("lamp-voice")).unwrap();
            symlink(&running, main.join("exe")).unwrap();
            let scan = |snapshot: &Snapshot| {
                scan_processes(
                    &tree.0.join("proc"),
                    snapshot,
                    &root,
                    Instant::now() + TOTAL_BUDGET,
                    &mut ProcessObservations::default(),
                )
            };
            let error = scan(&written).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("disagrees with manager invocation"),
                "{error}"
            );
            let resolved = written
                .clone()
                .resolved(|path| fs::canonicalize(path))
                .unwrap();
            assert_eq!(resolved.executable, running);
            assert_eq!(
                scan(&resolved).unwrap(),
                BTreeSet::from([CURRENT.to_owned()])
            );
            // An update switches the link while the old build still runs.
            let next = tree.directory("voice/two");
            fs::write(next.join("lamp-voice"), b"").unwrap();
            fs::remove_file(tree.0.join("voice/current")).unwrap();
            symlink(&next, tree.0.join("voice/current")).unwrap();
            let switched = written.resolved(|path| fs::canonicalize(path)).unwrap();
            assert!(scan(&switched).is_err());
        }

        /// A link that resolves to another program is refused.
        #[test]
        fn a_voice_binary_that_resolves_to_another_program_is_refused() {
            let root = Path::new("/sessions");
            assert!(
                snapshot(root)
                    .resolved(|_| Ok(PathBuf::from("/usr/bin/other")))
                    .is_err()
            );
            assert!(
                snapshot(root)
                    .resolved(|_| Err(io::Error::from(io::ErrorKind::NotFound)))
                    .is_err()
            );
        }

        #[test]
        fn scan_keeps_current_old_survivor_manual_and_diagnostic_child() {
            let tree = Tree::new();
            let root = tree.directory("sessions");
            let current = root.join(CURRENT);
            let old = tree.directory(&format!("sessions/{OLD}"));
            let manual = tree.directory(&format!("sessions/{MANUAL}"));
            // Current startup has not created its output yet.
            serve(&tree, 123, &current, "/system.slice/lamp-voice.service");
            let survivor = serve(&tree, 124, &old, "/system.slice/lamp-voice.service");
            symlink(old.join("metadata/events-0.jsonl"), survivor.join("fd/3")).unwrap();
            process(
                &tree,
                125,
                &[
                    OsStr::new(BINARY),
                    OsStr::new("directed"),
                    OsStr::new("/config"),
                    OsStr::new("60"),
                    manual.as_os_str(),
                ],
                "/user.slice/manual.scope",
            );
            let diagnostic = manual.join("capture-audio");
            process(
                &tree,
                126,
                &[
                    OsStr::new(BINARY),
                    OsStr::new("worker"),
                    OsStr::new("capture"),
                    OsStr::new("/tmp/ipc"),
                    OsStr::new(CURRENT),
                    OsStr::new(OLD),
                    OsStr::new("hw:1"),
                    OsStr::new("--diagnostics"),
                    diagnostic.as_os_str(),
                ],
                "/user.slice/manual.scope",
            );
            let got = scan_processes(
                &tree.0.join("proc"),
                &snapshot(&root),
                &root,
                Instant::now() + TOTAL_BUDGET,
                &mut ProcessObservations::default(),
            )
            .unwrap();
            assert_eq!(
                got,
                BTreeSet::from([CURRENT.to_owned(), OLD.to_owned(), MANUAL.to_owned()])
            );
        }

        #[test]
        fn relative_and_alias_outputs_are_resolved() {
            let tree = Tree::new();
            let root = tree.directory("sessions");
            let old = tree.directory(&format!("sessions/{OLD}"));
            symlink(&root, tree.0.join("alias")).unwrap();
            assert_eq!(
                resolve_output(Path::new(&format!("alias/{OLD}")), &tree.0).unwrap(),
                old
            );
            assert_eq!(
                resolve_output(&tree.0.join(format!("alias/{CURRENT}")), Path::new("/")).unwrap(),
                root.join(CURRENT)
            );
            validate_root(&root).unwrap();
            assert!(validate_root(&tree.0.join("alias")).is_err());
        }

        #[test]
        fn main_process_must_match_the_manager_and_be_present() {
            let tree = Tree::new();
            let root = tree.directory("sessions");
            tree.directory("proc");
            let check = || {
                scan_processes(
                    &tree.0.join("proc"),
                    &snapshot(&root),
                    &root,
                    Instant::now() + TOTAL_BUDGET,
                    &mut ProcessObservations::default(),
                )
            };
            assert!(check().is_err());
            serve(
                &tree,
                123,
                &root.join(OLD),
                "/system.slice/lamp-voice.service",
            );
            assert!(check().is_err());
        }

        #[test]
        fn main_executable_mismatch_refuses_even_with_the_right_binary_name() {
            let tree = Tree::new();
            let root = tree.directory("sessions");
            let current = serve(
                &tree,
                123,
                &root.join(CURRENT),
                "/system.slice/lamp-voice.service",
            );
            fs::remove_file(current.join("exe")).unwrap();
            symlink("/opt/another-release/lamp-voice", current.join("exe")).unwrap();
            assert!(
                scan_processes(
                    &tree.0.join("proc"),
                    &snapshot(&root),
                    &root,
                    Instant::now() + TOTAL_BUDGET,
                    &mut ProcessObservations::default(),
                )
                .is_err()
            );
        }

        #[test]
        fn failed_service_still_protects_an_old_live_coordinator() {
            let tree = Tree::new();
            let root = tree.directory("sessions");
            let old = tree.directory(&format!("sessions/{OLD}"));
            serve(&tree, 124, &old, "/system.slice/lamp-voice.service");
            let text = response(&root)
                .replace("ActiveState=active", "ActiveState=failed")
                .replace("SubState=running", "SubState=failed")
                .replace("MainPID=123", "MainPID=0");
            let snapshot = Snapshot::parse(text.as_bytes(), "lamp-voice.service", &root).unwrap();
            let got = scan_processes(
                &tree.0.join("proc"),
                &snapshot,
                &root,
                Instant::now() + TOTAL_BUDGET,
                &mut ProcessObservations::default(),
            )
            .unwrap();
            assert_eq!(got, BTreeSet::from([CURRENT.to_owned(), OLD.to_owned()]));
        }

        #[test]
        fn the_voices_end_of_turn_worker_is_known_and_its_files_still_checked() {
            let tree = Tree::new();
            let root = tree.directory("sessions");
            serve(
                &tree,
                123,
                &root.join(CURRENT),
                "/system.slice/lamp-voice.service",
            );
            let helper = process(
                &tree,
                124,
                &[
                    OsStr::new("/opt/lampOS/releases/x/lamp-turn-end"),
                    OsStr::new("--model"),
                ],
                "/system.slice/lamp-voice.service",
            );
            fs::remove_file(helper.join("exe")).unwrap();
            symlink("/opt/lampOS/releases/x/lamp-turn-end", helper.join("exe")).unwrap();
            assert!(
                scan_processes(
                    &tree.0.join("proc"),
                    &snapshot(&root),
                    &root,
                    Instant::now() + TOTAL_BUDGET,
                    &mut ProcessObservations::default(),
                )
                .is_ok()
            );
        }

        #[test]
        fn unknown_same_cgroup_process_refuses() {
            let tree = Tree::new();
            let root = tree.directory("sessions");
            serve(
                &tree,
                123,
                &root.join(CURRENT),
                "/system.slice/lamp-voice.service",
            );
            let unknown = process(
                &tree,
                124,
                &[OsStr::new("/opt/unknown"), OsStr::new("worker")],
                "/system.slice/lamp-voice.service",
            );
            fs::remove_file(unknown.join("exe")).unwrap();
            symlink("/opt/unknown", unknown.join("exe")).unwrap();
            assert!(
                scan_processes(
                    &tree.0.join("proc"),
                    &snapshot(&root),
                    &root,
                    Instant::now() + TOTAL_BUDGET,
                    &mut ProcessObservations::default(),
                )
                .is_err()
            );
        }

        #[test]
        fn unrelated_serve_commands_do_not_use_the_voice_argument_layout() {
            let tree = Tree::new();
            let root = tree.directory("sessions");
            serve(
                &tree,
                123,
                &root.join(CURRENT),
                "/system.slice/lamp-voice.service",
            );
            let health = process(
                &tree,
                200,
                &[OsStr::new("/opt/lamp-health"), OsStr::new("serve")],
                "/system.slice/lamp-health.service",
            );
            fs::remove_file(health.join("exe")).unwrap();
            symlink("/opt/lamp-health", health.join("exe")).unwrap();
            let memory = process(
                &tree,
                201,
                &[
                    OsStr::new("/opt/lamp-memory"),
                    OsStr::new("serve"),
                    OsStr::new("--socket"),
                    OsStr::new("/run/lamp-memory/memory.sock"),
                ],
                "/system.slice/lamp-memory.service",
            );
            fs::remove_file(memory.join("exe")).unwrap();
            symlink("/opt/lamp-memory", memory.join("exe")).unwrap();
            let got = scan_processes(
                &tree.0.join("proc"),
                &snapshot(&root),
                &root,
                Instant::now() + TOTAL_BUDGET,
                &mut ProcessObservations::default(),
            )
            .unwrap();
            assert_eq!(got, BTreeSet::from([CURRENT.to_owned()]));
        }

        #[test]
        fn renamed_manual_binary_uses_direct_root_evidence_without_voice_parsing() {
            let tree = Tree::new();
            let root = tree.directory("sessions");
            let old = tree.directory(&format!("sessions/{OLD}"));
            serve(
                &tree,
                123,
                &root.join(CURRENT),
                "/system.slice/lamp-voice.service",
            );
            // This argv does not satisfy the voice `serve` layout. Its direct
            // path and open record still establish conservative protection.
            let manual = process(
                &tree,
                202,
                &[
                    OsStr::new("/opt/renamed-reader"),
                    OsStr::new("serve"),
                    old.as_os_str(),
                ],
                "/user.slice/manual.scope",
            );
            fs::remove_file(manual.join("exe")).unwrap();
            symlink("/opt/renamed-reader", manual.join("exe")).unwrap();
            symlink(old.join("metadata/events-0.jsonl"), manual.join("fd/3")).unwrap();
            let got = scan_processes(
                &tree.0.join("proc"),
                &snapshot(&root),
                &root,
                Instant::now() + TOTAL_BUDGET,
                &mut ProcessObservations::default(),
            )
            .unwrap();
            assert_eq!(got, BTreeSet::from([CURRENT.to_owned(), OLD.to_owned()]));
        }

        #[test]
        fn incomplete_process_evidence_and_unknown_workers_refuse() {
            let tree = Tree::new();
            let root = tree.directory("sessions");
            let current = serve(
                &tree,
                123,
                &root.join(CURRENT),
                "/system.slice/lamp-voice.service",
            );
            let check = || {
                scan_processes(
                    &tree.0.join("proc"),
                    &snapshot(&root),
                    &root,
                    Instant::now() + TOTAL_BUDGET,
                    &mut ProcessObservations::default(),
                )
            };
            fs::remove_file(current.join("cmdline")).unwrap();
            assert!(check().is_err());
            assert!(
                voice_paths(&[
                    OsString::from(BINARY),
                    OsString::from("worker"),
                    OsString::from("unknown")
                ])
                .is_err()
            );
            assert!(voice_paths(&[OsString::from(BINARY), OsString::from("serve")]).is_err());
            assert!(
                voice_paths(&[
                    OsString::from(BINARY),
                    OsString::from("schedule-consumer"),
                    OsString::from("extra")
                ])
                .is_err()
            );
        }

        /// #1071: the guard knows every role and command on the voice's own
        /// list, so a role added there is known here without a second copy.
        #[test]
        fn every_voice_command_and_worker_role_on_the_voices_list_is_known() {
            let worker = |role: &str| {
                [BINARY, "worker", role, "/run/dir", "parent", "worker"]
                    .map(OsString::from)
                    .to_vec()
            };
            for role in WorkerRole::ALL {
                assert!(voice_paths(&worker(role.as_str())).is_ok(), "{role:?}");
            }
            for command in VoiceCommand::ALL {
                let argv = match command {
                    VoiceCommand::Worker => worker("probe"),
                    VoiceCommand::ScheduleConsumer => {
                        vec![OsString::from(BINARY), OsString::from(command.as_str())]
                    }
                    _ => [BINARY, command.as_str(), "a", "b", "c", "d"]
                        .map(OsString::from)
                        .to_vec(),
                };
                assert!(voice_paths(&argv).is_ok(), "{command:?}");
            }
            // A role not on the list is refused, and named when it is a
            // plain word; anything else is not repeated.
            let error = voice_paths(&worker("third")).unwrap_err().to_string();
            assert_eq!(error, "unknown voice worker `third`");
            let error = voice_paths(&worker("Third/../x")).unwrap_err().to_string();
            assert_eq!(error, "unknown voice worker command");
        }

        #[test]
        fn the_voices_schedule_consumer_is_known_and_its_files_still_checked() {
            let tree = Tree::new();
            let root = tree.directory("sessions");
            serve(
                &tree,
                123,
                &root.join(CURRENT),
                "/system.slice/lamp-voice.service",
            );
            let consumer = process(
                &tree,
                124,
                &[OsStr::new(BINARY), OsStr::new("schedule-consumer")],
                "/system.slice/lamp-voice.service",
            );
            let check = || {
                scan_processes(
                    &tree.0.join("proc"),
                    &snapshot(&root),
                    &root,
                    Instant::now() + TOTAL_BUDGET,
                    &mut ProcessObservations::default(),
                )
            };
            assert_eq!(check().unwrap(), BTreeSet::from([CURRENT.to_owned()]));
            // A session file it holds open is still protected.
            let old = tree.directory(&format!("sessions/{OLD}/metadata"));
            fs::write(old.join("events-0.jsonl"), b"{}\n").unwrap();
            symlink(old.join("events-0.jsonl"), consumer.join("fd/3")).unwrap();
            assert_eq!(
                check().unwrap(),
                BTreeSet::from([CURRENT.to_owned(), OLD.to_owned()])
            );
        }

        #[test]
        fn the_second_voices_worker_is_known_and_its_files_still_checked() {
            let tree = Tree::new();
            let root = tree.directory("sessions");
            serve(
                &tree,
                123,
                &root.join(CURRENT),
                "/system.slice/lamp-voice.service",
            );
            // As lamp-4ace starts it: role, directory, parent, worker,
            // provider config, then the second config and flags.
            let second = process(
                &tree,
                124,
                &[
                    OsStr::new(BINARY),
                    OsStr::new("worker"),
                    OsStr::new("second"),
                    OsStr::new("/tmp/lampOS-run"),
                    OsStr::new("2be051b82715b0fb40edf50e473f28de"),
                    OsStr::new("7cc9b7fad8492f6ac74e4f4bb2fc95e2"),
                    OsStr::new("/etc/lampOS/voice/provider.json"),
                    OsStr::new("/etc/lampOS/voice/second.json"),
                    OsStr::new("--tools"),
                ],
                "/system.slice/lamp-voice.service",
            );
            let check = || {
                scan_processes(
                    &tree.0.join("proc"),
                    &snapshot(&root),
                    &root,
                    Instant::now() + TOTAL_BUDGET,
                    &mut ProcessObservations::default(),
                )
            };
            assert_eq!(check().unwrap(), BTreeSet::from([CURRENT.to_owned()]));
            // A session file it holds open is still protected.
            let old = tree.directory(&format!("sessions/{OLD}/metadata"));
            fs::write(old.join("events-0.jsonl"), b"{}\n").unwrap();
            symlink(old.join("events-0.jsonl"), second.join("fd/3")).unwrap();
            assert_eq!(
                check().unwrap(),
                BTreeSet::from([CURRENT.to_owned(), OLD.to_owned()])
            );
            // Cut short, it is still refused.
            assert!(
                voice_paths(&[
                    OsString::from(BINARY),
                    OsString::from("worker"),
                    OsString::from("second")
                ])
                .is_err()
            );
        }

        #[test]
        fn pid_reuse_and_parenthesized_comm_do_not_confuse_identity() {
            let before = identity(stat(42, 100, "S").as_bytes(), 42).unwrap();
            let reused = identity(stat(42, 101, "S").as_bytes(), 42).unwrap();
            assert_ne!(before, reused);
            assert!(identity(stat(43, 100, "S").as_bytes(), 42).is_err());
            assert!(arguments(b"lamp-voice\0serve").is_err());
        }

        #[test]
        fn pinned_proc_directory_does_not_switch_to_reused_name() {
            let tree = Tree::new();
            let path = tree.directory("proc/42");
            fs::write(path.join("stat"), stat(42, 100, "S")).unwrap();
            let pinned = directory_at(rustix::fs::CWD, path.as_os_str()).unwrap();
            fs::rename(&path, tree.0.join("old-process")).unwrap();
            fs::create_dir(&path).unwrap();
            fs::write(path.join("stat"), stat(42, 200, "S")).unwrap();
            let got = identity(&read_at(&pinned, "stat", 8192).unwrap(), 42).unwrap();
            assert_eq!(got.start, 100);
        }

        #[test]
        fn a_single_recheck_distinguishes_exit_from_empty_live_arguments() {
            use std::cell::Cell;
            let before = identity(stat(42, 100, "S").as_bytes(), 42).unwrap();
            for (state, flags, bytes, expected, command_reads) in [
                ("Z", 0, b"".as_slice(), Some(ProcessSkipReason::Zombie), 0),
                ("X", 0, b"".as_slice(), Some(ProcessSkipReason::Zombie), 0),
                ("S", 4, b"".as_slice(), Some(ProcessSkipReason::Exiting), 1),
                ("S", 4, b"live\0".as_slice(), None, 1),
                ("S", 0, b"".as_slice(), None, 0),
                ("T", 0, b"".as_slice(), None, 0),
                ("D", 0, b"".as_slice(), None, 0),
            ] {
                let reads = Cell::new(0);
                let commands = Cell::new(0);
                let after = identity(stat_with(42, 100, state, flags, 1).as_bytes(), 42).unwrap();
                let result = after_failure(
                    &before,
                    || {
                        reads.set(reads.get() + 1);
                        Ok(after.clone())
                    },
                    || {
                        commands.set(commands.get() + 1);
                        Ok(bytes.to_vec())
                    },
                )
                .unwrap();
                assert_eq!(result, expected, "state={state}, flags={flags}");
                assert_eq!(reads.get(), 1);
                assert_eq!(commands.get(), command_reads);
            }
        }

        #[test]
        fn absent_stat_and_esrch_are_gone_but_denied_or_invalid_stat_refuse() {
            let before = identity(stat(42, 100, "S").as_bytes(), 42).unwrap();
            for errno in [rustix::io::Errno::NOENT, rustix::io::Errno::SRCH] {
                assert_eq!(
                    after_failure(&before, || Err(errno.into()), || panic!("no cmdline retry"))
                        .unwrap(),
                    Some(ProcessSkipReason::Gone)
                );
            }
            for errno in [
                rustix::io::Errno::ACCESS,
                rustix::io::Errno::PERM,
                rustix::io::Errno::IO,
            ] {
                assert!(
                    after_failure(&before, || Err(errno.into()), || panic!("no cmdline retry"))
                        .is_err()
                );
            }
            assert!(
                after_failure(
                    &before,
                    || identity(b"broken", 42),
                    || panic!("no cmdline retry")
                )
                .is_err()
            );
        }

        #[test]
        fn an_exiting_flag_does_not_hide_unreadable_arguments_or_other_live_threads() {
            let before = identity(stat(42, 100, "S").as_bytes(), 42).unwrap();
            let exiting = identity(stat_with(42, 100, "S", 4, 1).as_bytes(), 42).unwrap();
            for errno in [
                rustix::io::Errno::NOENT,
                rustix::io::Errno::ACCESS,
                rustix::io::Errno::IO,
            ] {
                assert_eq!(
                    after_failure(&before, || Ok(exiting.clone()), || Err(errno.into())).unwrap(),
                    None
                );
            }
            assert_eq!(
                after_failure(
                    &before,
                    || Ok(exiting.clone()),
                    || Err(rustix::io::Errno::SRCH.into())
                )
                .unwrap(),
                Some(ProcessSkipReason::Gone)
            );
            let threaded = identity(stat_with(42, 100, "S", 4, 2).as_bytes(), 42).unwrap();
            assert_eq!(
                after_failure(
                    &before,
                    || Ok(threaded.clone()),
                    || panic!("no cmdline retry")
                )
                .unwrap(),
                None
            );
            for state in ["Z", "X"] {
                let threaded = identity(stat_with(42, 100, state, 4, 2).as_bytes(), 42).unwrap();
                assert!(
                    after_failure(
                        &before,
                        || Ok(threaded.clone()),
                        || panic!("no cmdline retry")
                    )
                    .is_err()
                );
            }
        }

        #[test]
        fn pid_reuse_or_changed_process_kind_cannot_be_reclassified_as_exit() {
            let before = identity(stat(42, 100, "S").as_bytes(), 42).unwrap();
            for after in [
                identity(stat(42, 101, "Z").as_bytes(), 42).unwrap(),
                identity(stat(43, 100, "Z").as_bytes(), 43).unwrap(),
                identity(stat_with(42, 100, "S", 0x20_0000 | 4, 1).as_bytes(), 42).unwrap(),
            ] {
                assert!(
                    after_failure(&before, || Ok(after.clone()), || panic!("no cmdline retry"))
                        .is_err()
                );
            }
            let kernel =
                identity(stat_with(42, 100, "S", 0x20_0000 | 4, 1).as_bytes(), 42).unwrap();
            assert_eq!(
                after_failure(
                    &kernel,
                    || Ok(kernel.clone()),
                    || panic!("kernel is not exited")
                )
                .unwrap(),
                None
            );
            assert_eq!(
                identity(stat_with(42, 100, "S", 0, 0).as_bytes(), 42)
                    .unwrap()
                    .threads,
                0
            );
        }

        #[test]
        fn zero_threads_require_one_followup_that_positively_establishes_exit() {
            use std::cell::Cell;
            for state in ["S", "Z", "X"] {
                let before = identity(stat_with(42, 100, state, 0, 0).as_bytes(), 42).unwrap();
                for errno in [rustix::io::Errno::NOENT, rustix::io::Errno::SRCH] {
                    let reads = Cell::new(0);
                    let reason = after_failure(
                        &before,
                        || {
                            reads.set(reads.get() + 1);
                            Err(errno.into())
                        },
                        || panic!("zero is not proof of missing arguments"),
                    )
                    .unwrap();
                    assert_eq!(reason, Some(ProcessSkipReason::Gone));
                    assert_eq!(reads.get(), 1);
                }
                assert!(
                    after_failure(
                        &before,
                        || Ok(before.clone()),
                        || panic!("zero remains ambiguous")
                    )
                    .is_err()
                );
                assert!(
                    after_failure(
                        &before,
                        || identity(b"malformed", 42),
                        || panic!("invalid stat remains ambiguous")
                    )
                    .is_err()
                );
                let live = identity(stat(42, 100, "S").as_bytes(), 42).unwrap();
                assert_eq!(
                    after_failure(&before, || Ok(live.clone()), || panic!("still alive")).unwrap(),
                    None
                );
                let dead = identity(stat(42, 100, "Z").as_bytes(), 42).unwrap();
                assert_eq!(
                    after_failure(&before, || Ok(dead.clone()), || panic!("no cmdline needed"))
                        .unwrap(),
                    Some(ProcessSkipReason::Zombie)
                );
            }
        }

        #[test]
        fn persistent_zero_thread_metadata_never_passes_process_inspection() {
            for state in ["S", "Z", "X"] {
                let tree = Tree::new();
                let root = tree.directory("sessions");
                let path = serve(&tree, 42, &root.join(OLD), "/unrelated");
                fs::write(path.join("stat"), stat_with(42, 44, state, 4, 0)).unwrap();
                let pinned = directory_at(rustix::fs::CWD, path.as_os_str()).unwrap();
                let mut observations = ProcessObservations::default();
                let error = inspect_process(
                    &pinned,
                    42,
                    &snapshot(&root),
                    &root,
                    &mut empty_scan(),
                    &mut observations,
                    Instant::now() + TOTAL_BUDGET,
                )
                .unwrap_err();
                assert!(error.to_string().contains("thread count is unavailable"));
                assert_eq!(observations, ProcessObservations::default());
            }
        }

        fn empty_scan() -> Scan {
            Scan {
                sessions: BTreeSet::new(),
                main_found: false,
                fds: 0,
            }
        }

        #[test]
        fn process_exits_and_known_kernel_threads_are_reported_without_reading_hidden_arguments() {
            for (state, flags, threads, expected) in [
                ("Z", 0, 1, Some(ProcessSkipReason::Zombie)),
                ("X", 0, 1, Some(ProcessSkipReason::Zombie)),
                ("S", 4, 1, Some(ProcessSkipReason::Exiting)),
                ("S", 0x20_0000, 1, None),
                ("Z", 0x20_0000, 2, None),
                ("X", 0x20_0004, 0, None),
            ] {
                let tree = Tree::new();
                let root = tree.directory("sessions");
                let path = process(&tree, 42, &[], "/unrelated");
                fs::write(path.join("stat"), stat_with(42, 44, state, flags, threads)).unwrap();
                if state == "Z" || state == "X" || flags == 0x20_0000 {
                    fs::remove_file(path.join("cmdline")).unwrap();
                }
                let pinned = directory_at(rustix::fs::CWD, path.as_os_str()).unwrap();
                let mut observations = ProcessObservations::default();
                inspect_process(
                    &pinned,
                    42,
                    &snapshot(&root),
                    &root,
                    &mut empty_scan(),
                    &mut observations,
                    Instant::now() + TOTAL_BUDGET,
                )
                .unwrap();
                assert_eq!(observations.kernel_threads, u32::from(expected.is_none()));
                assert_eq!(
                    observations.examples.first().map(|item| item.reason),
                    expected
                );
                assert!(observations.examples.iter().all(|item| item.pid == 42));
            }
        }

        #[test]
        fn a_live_empty_command_or_missing_metadata_still_refuses() {
            for missing in ["empty_cmdline", "cmdline", "exe", "cgroup", "cwd", "fd"] {
                let tree = Tree::new();
                let root = tree.directory("sessions");
                let path = serve(&tree, 42, &root.join(OLD), "/unrelated");
                match missing {
                    "empty_cmdline" => fs::write(path.join("cmdline"), b"").unwrap(),
                    "fd" => fs::remove_dir(path.join("fd")).unwrap(),
                    _ => fs::remove_file(path.join(missing)).unwrap(),
                }
                let pinned = directory_at(rustix::fs::CWD, path.as_os_str()).unwrap();
                let mut observations = ProcessObservations::default();
                assert!(
                    inspect_process(
                        &pinned,
                        42,
                        &snapshot(&root),
                        &root,
                        &mut empty_scan(),
                        &mut observations,
                        Instant::now() + TOTAL_BUDGET
                    )
                    .is_err(),
                    "missing={missing}"
                );
                assert_eq!(observations, ProcessObservations::default());
            }
        }

        /// The first builder on #705 (17 Oct, 15:10): lamp-4ace's records
        /// run failed every time on a process live with no arguments when it
        /// was looked at, and its scan saw processes gone and zombies around
        /// it. A process ending with two threads, its leader having let go
        /// of its memory, reads so. Looked at again after a short wait, it
        /// is gone, a zombie or exiting, and skipped as on a first look.
        #[test]
        fn unreadable_at_first_and_gone_a_zombie_or_exiting_on_a_later_look_is_skipped() {
            for later in ["gone", "zombie", "exiting"] {
                let tree = Tree::new();
                let root = tree.directory("sessions");
                let path = process(&tree, 42, &[], "/unrelated");
                fs::write(path.join("stat"), stat_with(42, 44, "S", 4, 2)).unwrap();
                let pinned = directory_at(rustix::fs::CWD, path.as_os_str()).unwrap();
                let mut observations = ProcessObservations::default();
                let mut looks = 0;
                inspect_process_settling(
                    &pinned,
                    42,
                    &snapshot(&root),
                    &root,
                    &mut empty_scan(),
                    &mut observations,
                    Instant::now() + TOTAL_BUDGET,
                    &mut || {
                        looks += 1;
                        match later {
                            "gone" => fs::remove_dir_all(&path).unwrap(),
                            "zombie" => {
                                fs::write(path.join("stat"), stat_with(42, 44, "Z", 4, 1)).unwrap()
                            }
                            _ => {
                                fs::write(path.join("stat"), stat_with(42, 44, "S", 4, 1)).unwrap()
                            }
                        }
                    },
                )
                .unwrap_or_else(|error| panic!("{later}: {error}"));
                assert_eq!(looks, 1, "{later}");
                let counted = match later {
                    "gone" => observations.gone,
                    "zombie" => observations.zombie,
                    _ => observations.exiting,
                };
                assert_eq!(counted, 1, "{later}: {observations:?}");
                assert_eq!(observations.examples.len(), 1, "{later}");
            }
        }

        /// One between `execve` and its new arguments reads no arguments for
        /// a moment too. Readable on a later look, it is inspected as any
        /// live process, and the session it writes is protected.
        #[test]
        fn unreadable_at_first_and_readable_on_a_later_look_is_inspected_as_live() {
            let tree = Tree::new();
            let root = tree.directory("sessions");
            let path = serve(&tree, 42, &root.join(OLD), "/unrelated");
            let argv = fs::read(path.join("cmdline")).unwrap();
            fs::write(path.join("cmdline"), b"").unwrap();
            let pinned = directory_at(rustix::fs::CWD, path.as_os_str()).unwrap();
            let mut observations = ProcessObservations::default();
            let mut scan = empty_scan();
            inspect_process_settling(
                &pinned,
                42,
                &snapshot(&root),
                &root,
                &mut scan,
                &mut observations,
                Instant::now() + TOTAL_BUDGET,
                &mut || fs::write(path.join("cmdline"), &argv).unwrap(),
            )
            .unwrap();
            assert!(scan.sessions.contains(OLD), "{:?}", scan.sessions);
            assert_eq!(observations, ProcessObservations::default());
        }

        /// Still live with no arguments after every look, a process fails
        /// closed, as it did on a single look, and only after
        /// [`SETTLE_LOOKS`] waits.
        #[test]
        fn still_live_and_unreadable_after_every_look_refuses() {
            let tree = Tree::new();
            let root = tree.directory("sessions");
            let path = serve(&tree, 42, &root.join(OLD), "/unrelated");
            fs::write(path.join("cmdline"), b"").unwrap();
            let pinned = directory_at(rustix::fs::CWD, path.as_os_str()).unwrap();
            let mut observations = ProcessObservations::default();
            let mut looks = 0;
            let error = inspect_process_settling(
                &pinned,
                42,
                &snapshot(&root),
                &root,
                &mut empty_scan(),
                &mut observations,
                Instant::now() + TOTAL_BUDGET,
                &mut || looks += 1,
            )
            .unwrap_err();
            assert_eq!(looks, SETTLE_LOOKS);
            assert!(
                error.to_string().contains("no inspectable arguments"),
                "{error}"
            );
            assert_eq!(observations, ProcessObservations::default());
        }

        #[test]
        fn disappearing_pinned_process_is_reported_and_never_follows_a_reused_pid() {
            let tree = Tree::new();
            let root = tree.directory("sessions");
            let path = serve(&tree, 42, &root.join(OLD), "/unrelated");
            let pinned = directory_at(rustix::fs::CWD, path.as_os_str()).unwrap();
            fs::remove_dir_all(&path).unwrap();
            serve(&tree, 42, &root.join(MANUAL), "/unrelated");
            let mut observations = ProcessObservations::default();
            let mut scan = empty_scan();
            inspect_process(
                &pinned,
                42,
                &snapshot(&root),
                &root,
                &mut scan,
                &mut observations,
                Instant::now() + TOTAL_BUDGET,
            )
            .unwrap();
            assert!(scan.sessions.is_empty());
            assert_eq!(observations.gone, 1);
            assert_eq!(observations.examples[0].pid, 42);
        }

        #[test]
        fn an_exited_main_process_still_refuses_even_if_its_invocation_is_known() {
            for (state, flags, remove) in [
                ("Z", 0, false),
                ("S", 4, false),
                ("S", 0x20_0000, false),
                ("S", 0, true),
            ] {
                let tree = Tree::new();
                let root = tree.directory("sessions");
                let path = process(&tree, 123, &[], "/system.slice/lamp-voice.service");
                fs::write(path.join("stat"), stat_with(123, 44, state, flags, 1)).unwrap();
                let pinned = directory_at(rustix::fs::CWD, path.as_os_str()).unwrap();
                if remove {
                    fs::remove_dir_all(&path).unwrap();
                }
                let mut observations = ProcessObservations::default();
                assert!(
                    inspect_process(
                        &pinned,
                        123,
                        &snapshot(&root),
                        &root,
                        &mut empty_scan(),
                        &mut observations,
                        Instant::now() + TOTAL_BUDGET
                    )
                    .is_err()
                );
                assert_eq!(
                    observations.gone
                        + observations.zombie
                        + observations.exiting
                        + observations.kernel_threads,
                    1
                );
            }
        }

        #[test]
        fn ending_a_process_does_not_remove_protection_already_discovered() {
            let tree = Tree::new();
            let root = tree.directory("sessions");
            let path = serve(&tree, 42, &root.join(OLD), "/unrelated");
            fs::remove_dir(path.join("fd")).unwrap();
            let pinned = directory_at(rustix::fs::CWD, path.as_os_str()).unwrap();
            let before = read_identity(&pinned, 42).unwrap();
            let mut scan = empty_scan();
            assert!(
                inspect_live_process(
                    &pinned,
                    42,
                    &snapshot(&root),
                    &root,
                    &mut scan,
                    Instant::now() + TOTAL_BUDGET,
                    &before
                )
                .is_err()
            );
            assert!(scan.sessions.contains(OLD));
            fs::write(path.join("stat"), stat(42, 44, "Z")).unwrap();
            let reason = after_failure(
                &before,
                || read_identity(&pinned, 42),
                || panic!("zombie has no argv read"),
            )
            .unwrap()
            .unwrap();
            let mut observations = ProcessObservations::default();
            skipped(42, &snapshot(&root), &mut observations, reason).unwrap();
            assert!(scan.sessions.contains(OLD));
            assert_eq!(observations.zombie, 1);
        }

        #[test]
        fn exits_in_a_scan_do_not_weaken_protection_for_a_live_file_holder() {
            let tree = Tree::new();
            let root = tree.directory("sessions");
            serve(
                &tree,
                123,
                &root.join(CURRENT),
                "/system.slice/lamp-voice.service",
            );
            let held = serve(&tree, 42, &root.join(OLD), "/unrelated");
            symlink(
                root.join(MANUAL).join("metadata/events-0.jsonl"),
                held.join("fd/3"),
            )
            .unwrap();
            let exited = process(&tree, 43, &[], "/unrelated");
            fs::write(exited.join("stat"), stat(43, 44, "Z")).unwrap();
            let mut observations = ProcessObservations::default();
            let found = scan_processes(
                &tree.0.join("proc"),
                &snapshot(&root),
                &root,
                Instant::now() + TOTAL_BUDGET,
                &mut observations,
            )
            .unwrap();
            assert_eq!(
                found,
                BTreeSet::from([CURRENT.to_owned(), OLD.to_owned(), MANUAL.to_owned()])
            );
            assert_eq!(observations.zombie, 1);
        }

        #[test]
        fn an_expired_budget_cannot_turn_a_failed_inspection_into_a_skip() {
            let tree = Tree::new();
            let root = tree.directory("sessions");
            let path = tree.directory("proc/42");
            let pinned = directory_at(rustix::fs::CWD, path.as_os_str()).unwrap();
            let mut observations = ProcessObservations::default();
            assert_eq!(
                inspect_process(
                    &pinned,
                    42,
                    &snapshot(&root),
                    &root,
                    &mut empty_scan(),
                    &mut observations,
                    Instant::now()
                )
                .unwrap_err()
                .kind(),
                io::ErrorKind::TimedOut
            );
            assert_eq!(observations, ProcessObservations::default());
        }

        #[cfg(target_os = "linux")]
        #[test]
        fn real_linux_file_holder_zombie_and_reaped_process_are_distinct() {
            use std::os::unix::process::CommandExt;
            let tree = Tree::new();
            let root = tree.directory("sessions");
            let metadata = tree.directory(&format!("sessions/{OLD}/metadata"));
            let file = metadata.join("events-0.jsonl");
            fs::write(&file, b"disposable fixture\n").unwrap();
            let mut command = Command::new("/bin/sh");
            command
                .arg0("lamp-turn-end")
                .args([
                    "-c",
                    "exec 3<\"$FOURTH_TEST_RECORD_PATH\"; printf R; read value",
                ])
                .env("FOURTH_TEST_RECORD_PATH", &file)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped());
            let mut child = Reap {
                child: command.spawn().unwrap(),
                reaped: false,
            };
            let mut stdout = child.child.stdout.take().unwrap();
            let flags = rustix::fs::fcntl_getfl(&stdout).unwrap();
            rustix::fs::fcntl_setfl(&stdout, flags | OFlags::NONBLOCK).unwrap();
            let ready_deadline = Instant::now() + Duration::from_secs(2);
            loop {
                check_deadline(ready_deadline).unwrap();
                let mut ready = [0];
                match stdout.read(&mut ready) {
                    Ok(1) => {
                        assert_eq!(ready[0], b'R');
                        break;
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1))
                    }
                    other => panic!("fixture did not open its record: {other:?}"),
                }
            }
            let pid = child.child.id();
            let path = PathBuf::from(format!("/proc/{pid}"));
            let pinned = directory_at(rustix::fs::CWD, path.as_os_str()).unwrap();
            let mut cmdline = File::from(
                openat(
                    &pinned,
                    "cmdline",
                    OFlags::RDONLY | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .unwrap(),
            );
            let mut manager = snapshot(&root);
            manager.main_pid = 0;
            let cgroups = fs::read_to_string("/proc/self/cgroup").unwrap();
            manager.control_group = cgroups
                .lines()
                .next()
                .unwrap()
                .splitn(3, ':')
                .nth(2)
                .unwrap()
                .to_owned();
            let mut scan = empty_scan();
            let mut observations = ProcessObservations::default();
            inspect_process(
                &pinned,
                pid,
                &manager,
                &root,
                &mut scan,
                &mut observations,
                Instant::now() + TOTAL_BUDGET,
            )
            .unwrap();
            assert_eq!(scan.sessions, BTreeSet::from([OLD.to_owned()]));
            assert_eq!(observations, ProcessObservations::default());
            assert!(child.child.try_wait().unwrap().is_none());

            child.child.kill().unwrap();
            let exit_deadline = Instant::now() + Duration::from_secs(2);
            while !read_identity(&pinned, pid).unwrap().zombie {
                check_deadline(exit_deadline).unwrap();
                thread::sleep(Duration::from_millis(1));
            }
            inspect_process(
                &pinned,
                pid,
                &manager,
                &root,
                &mut scan,
                &mut observations,
                Instant::now() + TOTAL_BUDGET,
            )
            .unwrap();
            assert_eq!(observations.zombie, 1);
            child.child.wait().unwrap();
            child.reaped = true;
            let error = cmdline.read_to_end(&mut Vec::new()).unwrap_err();
            assert_eq!(
                error.raw_os_error(),
                Some(rustix::io::Errno::SRCH.raw_os_error())
            );
            inspect_process(
                &pinned,
                pid,
                &manager,
                &root,
                &mut scan,
                &mut observations,
                Instant::now() + TOTAL_BUDGET,
            )
            .unwrap();
            assert_eq!(observations.gone, 1);
            assert_eq!(scan.sessions, BTreeSet::from([OLD.to_owned()]));
        }

        #[test]
        fn deadline_and_metadata_limits_refuse() {
            assert!(check_deadline(Instant::now()).is_err());
            let tree = Tree::new();
            fs::write(tree.0.join("large"), [1u8; 10]).unwrap();
            let fd = directory_at(rustix::fs::CWD, tree.0.as_os_str()).unwrap();
            assert!(read_at(&fd, "large", 5).is_err());
            let mut sessions = BTreeSet::new();
            assert!(
                protect_path(
                    Path::new("/sessions/manual"),
                    Path::new("/sessions"),
                    &mut sessions
                )
                .is_err()
            );
        }

        #[test]
        fn fake_manager_success_failure_timeout_and_output_cap() {
            let mut success = Command::new("/bin/sh");
            success.args(["-c", "printf 'known-output'"]);
            assert_eq!(
                run_bounded(&mut success, Instant::now() + COMMAND_BUDGET).unwrap(),
                b"known-output"
            );
            let mut failed = Command::new("/bin/sh");
            failed.args(["-c", "exit 3"]);
            assert!(run_bounded(&mut failed, Instant::now() + COMMAND_BUDGET).is_err());
            let mut stalled = Command::new("/bin/sh");
            stalled.args(["-c", "exec sleep 60"]);
            let started = Instant::now();
            assert_eq!(
                run_bounded(&mut stalled, started + Duration::from_millis(50))
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::TimedOut
            );
            assert!(started.elapsed() < Duration::from_secs(2));
            let mut large = Command::new("/bin/sh");
            large.args(["-c", "exec /usr/bin/yes oversized"]);
            assert!(run_bounded(&mut large, Instant::now() + COMMAND_BUDGET).is_err());
        }
    }
}

#[cfg(all(test, not(target_os = "linux")))]
mod platform_tests {
    #[test]
    fn unsupported_platform_never_claims_an_empty_live_set() {
        assert_eq!(
            super::live_sessions(
                "lamp-voice.service",
                std::path::Path::new("/sessions"),
                &mut crate::ProcessObservations::default()
            )
            .unwrap_err()
            .kind(),
            std::io::ErrorKind::Unsupported
        );
    }
}
