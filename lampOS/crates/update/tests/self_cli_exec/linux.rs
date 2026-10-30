//! Real once -> Command::exec -> new-main confirmation, with a local signed
//! channel and exact systemctl fixture. Appended ELF trailers give two hashes
//! of the same implementation, not distinct updater versions. No service,
//! deadline timer, fixed-root back, physical power cut or quiet switch is tested.
#![allow(unsafe_code)]

use base64::{Engine, engine::general_purpose::STANDARD};
use lamp_update::{
    manifest::{self, File as BuildFile, Manifest, sha256_hex},
    own,
};
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};
use serde_json::Value;
use std::{
    ffi::CString,
    fs::{self, File, OpenOptions},
    io::{self, Read},
    os::{
        fd::{AsRawFd, RawFd},
        unix::{ffi::OsStrExt, fs::PermissionsExt},
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tempfile::TempDir;

const COMMIT: &str = "0000000000000000000000000000000000000002";
const WITHIN: Duration = Duration::from_secs(30);
const MOST_LOG: u64 = 64 * 1024;
// No external service is queried. A different command fails, including a
// restart/reload that the self-only channel must never request.
const SYSTEMCTL: &str = r#"#!/bin/sh
[ "$#" -eq 6 ] && [ "$1" = show ] && [ "$2" = --no-pager ] &&
[ "$3" = --property=Id,LoadState,UnitFileState,ActiveState,SubState,Unit ] &&
[ "$4" = -- ] && [ "$5" = lamp-update-back.service ] &&
[ "$6" = lamp-update-back.timer ] || exit 73
printf 'ready\n' >> "$CLI_SYSTEMCTL_CALLS" || exit 74
printf 'Id=lamp-update-back.service\nLoadState=loaded\nUnitFileState=static\nActiveState=inactive\nSubState=dead\n\nId=lamp-update-back.timer\nLoadState=loaded\nUnitFileState=enabled\nActiveState=active\nSubState=waiting\nUnit=lamp-update-back.service\n'
"#;

fn read(path: impl AsRef<Path>, most: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    File::open(path.as_ref())
        .unwrap()
        .take(most + 1)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(
        bytes.len() as u64 <= most,
        "fixture read exceeded its bound"
    );
    bytes
}

fn text(path: impl AsRef<Path>) -> String {
    String::from_utf8(read(path, MOST_LOG)).unwrap()
}

fn single_thread() {
    assert_eq!(fs::read_dir("/proc/self/task").unwrap().count(), 1);
}

fn executable(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

struct Payloads {
    original: Vec<u8>,
    old: Vec<u8>,
    new: Vec<u8>,
    old_hash: String,
    new_hash: String,
}

impl Payloads {
    fn new() -> Self {
        let original = read(
            env!("CARGO_BIN_EXE_lamp-update"),
            manifest::MOST_FILE_BYTES - 128,
        );
        assert!(original.len() >= 64);
        assert_eq!(&original[..6], b"\x7fELF\x02\x01");
        let machine = u16::from_le_bytes(original[18..20].try_into().unwrap());
        assert_eq!(
            machine,
            if cfg!(target_arch = "aarch64") {
                183
            } else {
                62
            }
        );
        let phoff = u64::from_le_bytes(original[32..40].try_into().unwrap()) as usize;
        let phsize = u16::from_le_bytes(original[54..56].try_into().unwrap()) as usize;
        let phnum = u16::from_le_bytes(original[56..58].try_into().unwrap()) as usize;
        assert_eq!(phsize, 56);
        assert!(phnum <= 256 && phoff.checked_add(phsize * phnum).unwrap() <= original.len());
        for header in original[phoff..phoff + phsize * phnum].chunks_exact(phsize) {
            let offset = u64::from_le_bytes(header[8..16].try_into().unwrap());
            let size = u64::from_le_bytes(header[32..40].try_into().unwrap());
            assert!(offset.checked_add(size).unwrap() <= original.len() as u64);
        }
        let mut old = original.clone();
        old.extend_from_slice(b"\nCLI TEST: old identity, same updater implementation\n");
        let mut new = original.clone();
        new.extend_from_slice(b"\nCLI TEST: new identity, same updater implementation\n");
        let old_hash = sha256_hex(&old);
        let new_hash = sha256_hex(&new);
        assert_ne!(old_hash, new_hash);
        assert_eq!(&old[..original.len()], original);
        assert_eq!(&new[..original.len()], original);
        Self {
            original,
            old,
            new,
            old_hash,
            new_hash,
        }
    }
}

struct Fixture {
    folder: TempDir,
    root: PathBuf,
    old: PathBuf,
    candidate: PathBuf,
    current: PathBuf,
    records: PathBuf,
    args: Vec<String>,
    environment: Vec<String>,
}

impl Fixture {
    fn new(payload: &Payloads, enabled: bool) -> Self {
        // Use a native Linux temporary filesystem, not inherited TMPDIR, which
        // may be a host share that cannot bind Unix sockets.
        let folder = tempfile::Builder::new()
            .prefix("lamp-cli-exec-")
            .tempdir_in("/tmp")
            .unwrap();
        let base = folder.path();
        let root = base.join("root");
        let channel = base.join("channel");
        let bin = base.join("bin");
        let bus = base.join("bus");
        for path in [
            root.join("etc/systemd/system"),
            root.join("opt/lampOS/releases"),
            channel.clone(),
            bin.clone(),
            bus.clone(),
        ] {
            fs::create_dir_all(path).unwrap();
        }
        let old = root
            .join("opt/lampOS/update")
            .join(&payload.old_hash)
            .join("lamp-update");
        let candidate = root
            .join("opt/lampOS/update")
            .join(&payload.new_hash)
            .join("lamp-update");
        let current = root.join("opt/lampOS/update/current");
        executable(&old, &payload.old);
        executable(&channel.join("lamp-update"), &payload.new);
        executable(
            &root.join(own::BACK.trim_start_matches('/')),
            &payload.original,
        );
        std::os::unix::fs::symlink(&payload.old_hash, &current).unwrap();
        executable(&bin.join("systemctl"), SYSTEMCTL.as_bytes());
        let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let key = Ed25519KeyPair::from_pkcs8(document.as_ref()).unwrap();
        let public = base.join("test.pub");
        fs::write(&public, STANDARD.encode(key.public_key().as_ref())).unwrap();
        let url = format!("file://{}/", channel.display());
        let manifest = Manifest {
            schema: 1,
            channel: "self-cli-test".into(),
            sequence: 2,
            commit: COMMIT.into(),
            files_url: url.clone(),
            files: vec![BuildFile {
                name: "lamp-update".into(),
                sha256: payload.new_hash.clone(),
                size: payload.new.len() as u64,
            }],
            units: Vec::new(),
            min_updater: None,
            models: Vec::new(),
            settings: Vec::new(),
            image: Vec::new(),
            reset_plan: None,
            harness: false,
        };
        fs::write(
            channel.join(manifest::ENVELOPE),
            manifest::seal(&serde_json::to_vec(&manifest).unwrap(), &key).unwrap(),
        )
        .unwrap();
        let args = vec![
            "once".into(),
            "--channel-url".into(),
            url,
            "--channel".into(),
            "self-cli-test".into(),
            "--key-file".into(),
            public.display().to_string(),
            "--lamp".into(),
            "fixture".into(),
            "--apply".into(),
            "on".into(),
            "--root".into(),
            root.display().to_string(),
            "--bus-root".into(),
            bus.display().to_string(),
            "--keep-free-mb".into(),
            "0".into(),
            "--quiet-wait-s".into(),
            "1".into(),
            "--watch-s".into(),
            "1".into(),
        ];
        let environment = vec![
            format!("PATH={}:/usr/bin:/bin", bin.display()),
            "LANG=C".into(),
            "LC_ALL=C".into(),
            format!("LAMP_UPDATE_SELF={}", if enabled { "on" } else { "off" }),
            "LAMP_UPDATE_MODELS=off".into(),
            "LAMP_UPDATE_APPLY=off".into(),
            "LAMP_UPDATE_IMAGE=off".into(),
            format!(
                "CLI_SYSTEMCTL_CALLS={}",
                base.join("systemctl.calls").display()
            ),
        ];
        let records = root.join("var/lib/lampos-update");
        assert!(
            !records.exists(),
            "the real CLI must create its own records"
        );
        Self {
            folder,
            root,
            old,
            candidate,
            current,
            records,
            args,
            environment,
        }
    }

    fn json(&self, name: &str) -> Value {
        serde_json::from_slice(&read(self.records.join(name), MOST_LOG)).unwrap()
    }

    fn confirmations(&self) -> usize {
        text(self.records.join("log.jsonl"))
            .lines()
            .filter(|line| {
                serde_json::from_str::<Value>(line).unwrap()["event"] == "self-confirmed"
            })
            .count()
    }

    fn files(&self, payload: &Payloads) {
        for (path, expected) in [
            (&self.old, &payload.old),
            (&self.candidate, &payload.new),
            (
                &self.root.join(own::BACK.trim_start_matches('/')),
                &payload.original,
            ),
        ] {
            let metadata = fs::symlink_metadata(path).unwrap();
            assert!(metadata.is_file());
            assert_eq!(metadata.permissions().mode() & 0o777, 0o755);
            assert_eq!(read(path, manifest::MOST_FILE_BYTES), *expected);
        }
    }

    fn candidate_prefix(&self, payload: &Payloads) -> Vec<u8> {
        assert_eq!(
            fs::read_link(&self.current).unwrap(),
            Path::new(&payload.new_hash)
        );
        let pending = self.json("self.json");
        assert_eq!(pending["version"], 1);
        assert_eq!(pending["old"], payload.old_hash);
        assert_eq!(pending["new"], payload.new_hash);
        assert!(pending["at_unix_ms"].as_u64().unwrap() > 0);
        assert_eq!(pending.as_object().unwrap().len(), 4);
        let state = self.json("state.json");
        assert_eq!(
            state["installed"],
            serde_json::json!({"sequence":2,"commit":COMMIT})
        );
        assert!(
            state["made"]
                .as_array()
                .unwrap()
                .contains(&Value::String(format!(
                    "/opt/lampOS/update/{}/",
                    payload.new_hash
                )))
        );
        assert_eq!(self.confirmations(), 0);
        self.files(payload);
        read(self.records.join("state.json"), MOST_LOG)
    }

    fn confirmed(&self, payload: &Payloads, state: &[u8]) {
        assert_eq!(
            fs::read_link(&self.current).unwrap(),
            Path::new(&payload.new_hash)
        );
        assert!(!self.records.join("self.json").exists());
        assert_eq!(read(self.records.join("state.json"), MOST_LOG), state);
        assert_eq!(self.confirmations(), 1);
        self.files(payload);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if std::thread::panicking() {
            for name in ["help-old.log", "help-new.log", "round.log", "next.log"] {
                let path = self.folder.path().join(name);
                if let Ok(file) = File::open(path) {
                    let mut log = String::new();
                    let _ = file.take(MOST_LOG).read_to_string(&mut log);
                    eprintln!("CLI fixture {name}:\n{log}");
                }
            }
        }
    }
}

// CString storage and descriptor owners stay live in the parent until fork has
// copied their memory. Raw operands below are all prepared before fork.
struct Prepared {
    program: CString,
    _args: Vec<CString>,
    _environment: Vec<CString>,
    argv: Vec<*const libc::c_char>,
    envp: Vec<*const libc::c_char>,
    null: File,
    log: File,
}

impl Prepared {
    fn new(program: &Path, args: &[String], environment: &[String], log: &Path) -> Self {
        let program = CString::new(program.as_os_str().as_bytes()).unwrap();
        let mut words = vec![program.clone()];
        words.extend(
            args.iter()
                .map(|word| CString::new(word.as_bytes()).unwrap()),
        );
        let variables: Vec<_> = environment
            .iter()
            .map(|word| CString::new(word.as_bytes()).unwrap())
            .collect();
        let mut argv: Vec<_> = words.iter().map(|word| word.as_ptr()).collect();
        let mut envp: Vec<_> = variables.iter().map(|word| word.as_ptr()).collect();
        argv.push(std::ptr::null());
        envp.push(std::ptr::null());
        let null = File::open("/dev/null").unwrap();
        let log = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(log)
            .unwrap();
        assert!(null.as_raw_fd() > 2 && log.as_raw_fd() > 2);
        Self {
            program,
            _args: words,
            _environment: variables,
            argv,
            envp,
            null,
            log,
        }
    }
}

struct Child {
    pid: libc::pid_t,
    reaped: bool,
    log: File,
}

impl Child {
    fn wait(&mut self, deadline: Instant) -> i32 {
        loop {
            assert!(Instant::now() < deadline, "CLI child exceeded deadline");
            assert!(
                self.log.metadata().unwrap().len() <= MOST_LOG,
                "CLI child exceeded log bound"
            );
            let mut status = 0;
            // SAFETY: only our unreaped child's PID, with valid status storage.
            let got = unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) };
            if got == self.pid {
                self.reaped = libc::WIFEXITED(status) || libc::WIFSIGNALED(status);
                return status;
            }
            if got < 0 {
                assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EINTR));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        // SAFETY: the child owns this process group; the direct PID also
        // covers failure before setpgid. It has not been reaped/reused.
        unsafe {
            libc::kill(-self.pid, libc::SIGKILL);
            libc::kill(self.pid, libc::SIGKILL);
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let mut status = 0;
            // SAFETY: same owned child and live status buffer as wait().
            let got = unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) };
            if (got == self.pid && (libc::WIFEXITED(status) || libc::WIFSIGNALED(status)))
                || (got < 0 && io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD))
            {
                break;
            }
            if Instant::now() >= deadline {
                eprintln!("CLI exec cleanup did not reap its child within 5 seconds");
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

fn fork(prepared: &Prepared, traced: bool) -> Child {
    let program = prepared.program.as_ptr();
    let argv = prepared.argv.as_ptr();
    let envp = prepared.envp.as_ptr();
    let input: RawFd = prepared.null.as_raw_fd();
    let output: RawFd = prepared.log.as_raw_fd();
    let log = prepared.log.try_clone().unwrap();
    single_thread();
    // SAFETY: the driver is single-threaded, holds no library/stdio/record
    // guard, and all argv/env/descriptor operands were prepared above. The
    // child calls only the libc operations below, then execve or _exit; it
    // never formats, allocates, unwinds, drops Rust values or touches a lock.
    let pid = unsafe { libc::fork() };
    if pid == 0 {
        // SAFETY: scalar/prepared pointer operands remain valid in this fork;
        // all called operations are async-signal-safe on this Linux path.
        unsafe {
            if libc::setpgid(0, 0) != 0
                || libc::dup2(input, 0) < 0
                || libc::dup2(output, 1) < 0
                || libc::dup2(output, 2) < 0
            {
                libc::_exit(120);
            }
            if traced {
                if libc::ptrace(
                    libc::PTRACE_TRACEME,
                    0,
                    std::ptr::null_mut::<libc::c_void>(),
                    std::ptr::null_mut::<libc::c_void>(),
                ) != 0
                {
                    libc::_exit(121);
                }
                if libc::kill(libc::getpid(), libc::SIGSTOP) != 0 {
                    libc::_exit(122);
                }
            }
            libc::execve(program, argv, envp);
            libc::_exit(123);
        }
    }
    assert!(pid > 0, "fork: {}", io::Error::last_os_error());
    Child {
        pid,
        reaped: false,
        log,
    }
}

fn trace(request: libc::c_uint, child: &Child, data: usize) {
    // SAFETY: only our stopped, unreaped child, no pointer-bearing requests.
    let result = unsafe {
        libc::ptrace(
            request,
            child.pid,
            std::ptr::null_mut::<libc::c_void>(),
            data as *mut libc::c_void,
        )
    };
    assert_eq!(
        result,
        0,
        "ptrace {request:#x}: {}",
        io::Error::last_os_error()
    );
}

fn proc_state(pid: libc::pid_t) -> (char, u64) {
    let raw = text(format!("/proc/{pid}/stat"));
    let fields: Vec<_> = raw[raw.rfind(')').unwrap() + 2..]
        .split_whitespace()
        .collect();
    (
        fields[0].chars().next().unwrap(),
        fields[19].parse().unwrap(),
    )
}

fn identity(
    child: &Child,
    start: u64,
    executable: &Path,
    hash: &str,
    arg0: &Path,
    args: &[String],
    environment: &[String],
) {
    let before = proc_state(child.pid);
    assert!(matches!(before.0, 't' | 'T'));
    assert_eq!(before.1, start);
    assert_eq!(
        fs::read_link(format!("/proc/{}/exe", child.pid)).unwrap(),
        executable.canonicalize().unwrap()
    );
    assert_eq!(
        sha256_hex(&read(
            format!("/proc/{}/exe", child.pid),
            manifest::MOST_FILE_BYTES
        )),
        hash
    );
    let mut wanted = arg0.as_os_str().as_bytes().to_vec();
    wanted.push(0);
    for arg in args {
        wanted.extend_from_slice(arg.as_bytes());
        wanted.push(0);
    }
    let actual = read(format!("/proc/{}/cmdline", child.pid), 16 * 1024);
    assert!(
        actual == wanted,
        "CLI argv mismatch: got {}, expected {}",
        sha256_hex(&actual),
        sha256_hex(&wanted)
    );
    let actual_environment = read(format!("/proc/{}/environ", child.pid), 16 * 1024);
    assert_eq!(actual_environment.last(), Some(&0));
    let mut actual_entries: Vec<_> = actual_environment[..actual_environment.len() - 1]
        .split(|byte| *byte == 0)
        .collect();
    let mut expected_entries: Vec<_> = environment.iter().map(|entry| entry.as_bytes()).collect();
    actual_entries.sort_unstable();
    expected_entries.sort_unstable();
    assert!(
        actual_entries == expected_entries,
        "CLI environment mismatch: got {}, expected {}",
        sha256_hex(&actual_environment),
        sha256_hex(environment.join("\0").as_bytes())
    );
    assert_eq!(proc_state(child.pid), before);
}

#[derive(Debug)]
struct MissingCandidateExec {
    status: i32,
}

fn candidate_exec(
    child: &mut Child,
    fixture: &Fixture,
    payload: &Payloads,
    deadline: Instant,
) -> Result<(), MissingCandidateExec> {
    let stopped = child.wait(deadline);
    assert!(libc::WIFSTOPPED(stopped) && libc::WSTOPSIG(stopped) == libc::SIGSTOP);
    // SAFETY: same owned child, now stopped after its successful setpgid.
    assert_eq!(unsafe { libc::getpgid(child.pid) }, child.pid);
    let start = proc_state(child.pid).1;
    trace(
        libc::PTRACE_SETOPTIONS,
        child,
        (libc::PTRACE_O_TRACEEXEC | libc::PTRACE_O_EXITKILL) as usize,
    );
    let mut signal = 0;
    let mut execs = 0;
    loop {
        trace(libc::PTRACE_CONT, child, signal);
        let status = child.wait(deadline);
        if libc::WIFEXITED(status) || libc::WIFSIGNALED(status) {
            assert_eq!(
                execs, 1,
                "old CLI did not execute before the candidate check"
            );
            return Err(MissingCandidateExec { status });
        }
        assert!(libc::WIFSTOPPED(status));
        if libc::WSTOPSIG(status) == libc::SIGCHLD {
            signal = libc::SIGCHLD as usize;
            continue;
        }
        signal = 0;
        assert_eq!(libc::WSTOPSIG(status), libc::SIGTRAP);
        assert_eq!(status >> 16, libc::PTRACE_EVENT_EXEC);
        execs += 1;
        if execs == 1 {
            identity(
                child,
                start,
                &fixture.old,
                &payload.old_hash,
                &fixture.old,
                &fixture.args,
                &fixture.environment,
            );
        } else {
            assert_eq!(execs, 2);
            identity(
                child,
                start,
                &fixture.candidate,
                &payload.new_hash,
                &fixture.current.join("lamp-update"),
                &fixture.args,
                &fixture.environment,
            );
            return Ok(());
        }
    }
}

fn zero(status: i32) {
    assert!(
        libc::WIFEXITED(status),
        "expected exit 0, wait status {status:#x}"
    );
    assert_eq!(libc::WEXITSTATUS(status), 0);
}

fn ordinary(
    fixture: &Fixture,
    program: &Path,
    args: &[String],
    name: &str,
    deadline: Instant,
) -> String {
    let log = fixture.folder.path().join(name);
    let prepared = Prepared::new(program, args, &fixture.environment, &log);
    let mut child = fork(&prepared, false);
    zero(child.wait(deadline));
    text(log)
}

pub(super) fn run() {
    single_thread();
    let deadline = Instant::now() + Duration::from_secs(120);
    let payload = Payloads::new();
    let fixture = Fixture::new(&payload, true);
    for (program, name) in [
        (&fixture.old, "help-old.log"),
        (
            &fixture.folder.path().join("channel/lamp-update"),
            "help-new.log",
        ),
    ] {
        assert!(
            ordinary(
                &fixture,
                program,
                &["--help".into()],
                name,
                (Instant::now() + WITHIN).min(deadline)
            )
            .contains("lamp-update check|once|serve")
        );
    }
    let prepared = Prepared::new(
        &fixture.old,
        &fixture.args,
        &fixture.environment,
        &fixture.folder.path().join("round.log"),
    );
    let mut child = fork(&prepared, true);
    candidate_exec(
        &mut child,
        &fixture,
        &payload,
        (Instant::now() + WITHIN).min(deadline),
    )
    .expect("real CLI did not exec its candidate");
    let state = fixture.candidate_prefix(&payload);
    trace(libc::PTRACE_DETACH, &child, 0);
    zero(child.wait((Instant::now() + WITHIN).min(deadline)));
    fixture.confirmed(&payload, &state);
    let said = text(fixture.folder.path().join("round.log"));
    assert_eq!(
        said.matches("confirmed: lamp-update runs its new build")
            .count(),
        1
    );
    assert!(said.lines().any(|line| line == "up to date: build 2"));
    assert!(
        !said.contains("cannot be subscribed"),
        "fixture bus unavailable: {said}"
    );
    let said = ordinary(
        &fixture,
        &fixture.current.join("lamp-update"),
        &fixture.args,
        "next.log",
        (Instant::now() + WITHIN).min(deadline),
    );
    assert!(said.lines().any(|line| line == "up to date: build 2"));
    assert!(!said.contains("confirmed: lamp-update runs its new build"));
    fixture.confirmed(&payload, &state);
    assert_eq!(
        text(fixture.folder.path().join("systemctl.calls")),
        "ready\n"
    );
    println!(
        "CLI positive: old exec, real candidate exec/pending, detach, one confirmation, next round build 2"
    );

    let control = Fixture::new(&payload, false);
    let prepared = Prepared::new(
        &control.old,
        &control.args,
        &control.environment,
        &control.folder.path().join("round.log"),
    );
    let mut child = fork(&prepared, true);
    let missing = candidate_exec(
        &mut child,
        &control,
        &payload,
        (Instant::now() + WITHIN).min(deadline),
    )
    .expect_err("self switch off unexpectedly reached candidate exec");
    zero(missing.status);
    // This control ends successfully with the same sequence, but is rejected
    // specifically at the missing candidate exec, before any prefix assertion.
    assert_eq!(control.json("state.json")["installed"]["sequence"], 2);
    assert_eq!(
        fs::read_link(&control.current).unwrap(),
        Path::new(&payload.old_hash)
    );
    assert!(!control.records.join("self.json").exists());
    assert!(!control.candidate.exists());
    assert!(!control.folder.path().join("systemctl.calls").exists());
    assert_eq!(control.confirmations(), 0);
    println!(
        "CLI control: LAMP_UPDATE_SELF=off rejected for missing candidate exec despite exit 0/build 2"
    );
}
