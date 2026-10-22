//! Actual SIGKILL at completed persistence syscalls, without a runtime hook.
//! The custom driver must call these functions on its sole thread, never a
//! libtest worker. Forked children inherit an unlocked fixture and use _exit;
//! the parent alone owns its TempDir. This proves library-process death and
//! lock release, not daemon exec, a real recovery timer, or power-loss durability.
#![cfg(target_os = "linux")]
// Only this subprocess fixture needs fork/ptrace. Every unsafe operation below
// addresses our unreaped child or a live, sized syscall buffer. No runtime lint
// is relaxed. A single thread at fork avoids inheriting another thread's locks.
#![allow(unsafe_code)]

use super::*;
use std::{
    io::Read,
    mem::{offset_of, size_of},
    os::unix::{fs::PermissionsExt, process::CommandExt},
    process::{Command, Stdio},
    time::Instant,
};

#[cfg(not(all(
    target_pointer_width = "64",
    target_endian = "little",
    any(target_arch = "aarch64", target_arch = "x86_64")
)))]
compile_error!("process cuts require little-endian aarch64 or x86_64 Linux");

const OLD: &str = "#!/bin/sh\nprintf 'old-updater\\n'\n";
const NEW: &str = "#!/bin/sh\nprintf 'new-updater\\n'\n";
const WITHIN: Duration = Duration::from_secs(120);
pub(crate) const CASES: [&str; 7] = [
    "candidate_synced",
    "pending_synced",
    "current_synced",
    "confirmation_synced",
    "put_back_link_synced",
    "refusal_synced",
    "put_back_cleanup_synced",
];

// Linux PTRACE_GET_SYSCALL_INFO (since 5.3): 24-byte header and a union
// containing entry { u64 nr, args[6] }, exit { i64 rval, u8 is_error }, or
// seccomp. Use aligned storage, not architecture-specific register offsets.
// https://man7.org/linux/man-pages/man2/ptrace.2.html
#[repr(C)]
#[derive(Default)]
struct SyscallInfo {
    op: u8,
    pad: [u8; 3],
    arch: u32,
    ip: u64,
    sp: u64,
    data: [u64; 8],
}

fn single_thread() {
    assert_eq!(fs::read_dir("/proc/self/task").unwrap().count(), 1);
}

fn trace(request: libc::c_uint, pid: libc::pid_t, addr: usize, data: usize) -> libc::c_long {
    // SAFETY: the child is ours; pointer-bearing callers pass a live buffer
    // and its actual size, all other requests use documented integer operands.
    let result = unsafe {
        libc::ptrace(
            request,
            pid,
            addr as *mut libc::c_void,
            data as *mut libc::c_void,
        )
    };
    assert!(
        result >= 0,
        "ptrace {request:#x}: {}",
        io::Error::last_os_error()
    );
    result
}

struct Child {
    pid: libc::pid_t,
    reaped: bool,
}

impl Child {
    fn wait(&mut self, deadline: Instant) -> i32 {
        let mut pause = Duration::from_micros(20);
        loop {
            assert!(
                Instant::now() < deadline,
                "child {} exceeded its deadline",
                self.pid
            );
            let mut status = 0;
            // SAFETY: wait only for our still-owned PID; status is writable.
            let got = unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) };
            if got == self.pid {
                self.reaped = libc::WIFEXITED(status) || libc::WIFSIGNALED(status);
                return status;
            }
            if got < 0 {
                assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EINTR));
            }
            std::thread::sleep(pause);
            pause = (pause * 2).min(Duration::from_millis(1));
        }
    }

    fn signal(&self) {
        // SAFETY: only the group named by our unreaped child's PID is killed.
        // The direct PID also covers an error before its setpgid completed.
        unsafe {
            libc::kill(-self.pid, libc::SIGKILL);
            libc::kill(self.pid, libc::SIGKILL);
        }
    }

    fn kill_at_cut(&mut self) {
        self.signal();
        let status = self.wait(Instant::now() + Duration::from_secs(5));
        assert!(
            libc::WIFSIGNALED(status),
            "expected SIGKILL, wait status {status:#x}"
        );
        assert_eq!(libc::WTERMSIG(status), libc::SIGKILL);
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        self.signal();
        let deadline = Instant::now() + Duration::from_secs(5);
        // Bounded best-effort cleanup even while a parent assertion unwinds.
        loop {
            let mut status = 0;
            // SAFETY: same owned PID and valid status storage as wait().
            let got = unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) };
            if got == self.pid && (libc::WIFEXITED(status) || libc::WIFSIGNALED(status)) {
                break;
            }
            if got < 0 && io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD) {
                break;
            }
            if Instant::now() >= deadline {
                eprintln!("process-cut cleanup did not reap its child within 5 seconds");
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

fn fork(lamp: &mut Lamp, case: usize, recovery: bool) -> Child {
    single_thread();
    // SAFETY: called solely by the single-thread custom driver, outside all
    // FORKS/records/stdio guards. Children never return or drop the TempDir.
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0, "fork: {}", io::Error::last_os_error());
    if pid == 0 {
        // Assertion failures must not unwind and delete the parent's fixture.
        std::panic::set_hook(Box::new(|info| {
            eprintln!("process-cut child: {info}");
            // SAFETY: terminate only this child, without running destructors.
            unsafe { libc::_exit(102) }
        }));
        // SAFETY: isolate our child's helpers; ptrace is requested of its parent.
        unsafe {
            if libc::setpgid(0, 0) != 0 {
                libc::_exit(103);
            }
            if !recovery {
                if libc::ptrace(
                    libc::PTRACE_TRACEME,
                    0 as libc::pid_t,
                    std::ptr::null_mut::<libc::c_void>(),
                    std::ptr::null_mut::<libc::c_void>(),
                ) != 0
                {
                    libc::_exit(104);
                }
                libc::raise(libc::SIGSTOP);
            }
        }
        if recovery {
            recover(lamp, case);
        } else if case < 3 {
            let _ = lamp.round();
        } else if case == 3 {
            let _ = lamp.confirm(&sha(NEW));
        } else {
            let _ = lamp.back(update_unit("failed", "failed"), u64::MAX);
        }
        // A traced operation finishing before the cut is a parent-side failure.
        // SAFETY: raw exit does not release a guard by Rust unwinding.
        unsafe { libc::_exit(if recovery { 0 } else { 105 }) }
    }
    Child { pid, reaped: false }
}

fn prefix(lamp: &Lamp, case: usize) -> bool {
    let current = if matches!(case, 2 | 3) { NEW } else { OLD };
    let wants_pending = matches!(case, 1 | 2 | 4 | 5);
    let pending = lamp.pending();
    let right_pending = match pending {
        Some(p) => wants_pending && p.version == 1 && p.old == sha(OLD) && p.new == sha(NEW),
        None => !wants_pending,
    };
    let refused = if case >= 5 { vec![sha(NEW)] } else { vec![] };
    lamp.own_build() == sha(current)
        && right_pending
        && own::refused(&lamp.records()) == refused
        && fs::read(
            lamp.config
                .on(&unit::folder(own::FOLDER, &sha(NEW)))
                .join(own::BINARY),
        )
        .is_ok_and(|bytes| bytes == NEW.as_bytes())
}

fn cut(lamp: &mut Lamp, case: usize) -> String {
    let mut child = fork(lamp, case, false);
    let deadline = Instant::now() + WITHIN;
    let stopped = child.wait(deadline);
    assert!(libc::WIFSTOPPED(stopped) && libc::WSTOPSIG(stopped) == libc::SIGSTOP);
    trace(
        libc::PTRACE_SETOPTIONS,
        child.pid,
        0,
        (libc::PTRACE_O_TRACESYSGOOD | libc::PTRACE_O_EXITKILL) as usize,
    );
    let folder = if matches!(case, 0 | 2 | 4) {
        lamp.config.on(own::FOLDER)
    } else {
        lamp.records()
    };
    let mut fsync_fd = None;
    let mut signal = 0;
    loop {
        trace(libc::PTRACE_SYSCALL, child.pid, 0, signal);
        let status = child.wait(deadline);
        assert!(
            libc::WIFSTOPPED(status),
            "cut not reached: wait status {status:#x}"
        );
        let sig = libc::WSTOPSIG(status);
        // round/confirm may reap their existing Skills-check subprocess. Forward
        // that normal notification, never treat it as a persistence boundary.
        if sig == libc::SIGCHLD {
            signal = libc::SIGCHLD as usize;
            continue;
        }
        signal = 0;
        assert_eq!(sig, libc::SIGTRAP | 0x80, "unexpected trace signal");
        let mut info = SyscallInfo::default();
        let available = trace(
            0x420e,
            child.pid,
            size_of::<SyscallInfo>(),
            (&mut info as *mut SyscallInfo) as usize,
        ) as usize;
        assert!(available >= 24, "GET_SYSCALL_INFO requires Linux >= 5.3");
        let arch = if cfg!(target_arch = "aarch64") {
            0xc00000b7
        } else {
            0xc000003e
        };
        assert_eq!(info.arch, arch, "unexpected syscall architecture");
        match info.op {
            1 => {
                assert!(available >= 80);
                fsync_fd = (info.data[0] == libc::SYS_fsync as u64)
                    .then(|| i32::try_from(info.data[1]).expect("fsync fd"));
            }
            2 => {
                assert!(available >= 33);
                if let Some(fd) = fsync_fd.take() {
                    let path = fs::read_link(format!("/proc/{}/fd/{fd}", child.pid)).unwrap();
                    if path == folder
                        && info.data[0] == 0
                        && info.data[1] & 0xff == 0
                        && prefix(lamp, case)
                    {
                        assert_eq!(lamp.state()["installed"]["sequence"], 2);
                        child.kill_at_cut();
                        return path.strip_prefix(&lamp.root).unwrap().display().to_string();
                    }
                }
            }
            other => panic!("unexpected syscall-info operation {other}"),
        }
    }
}

fn executable(path: &Path, script: &str) {
    assert_eq!(fs::read(path).unwrap(), script.as_bytes());
    let metadata = fs::symlink_metadata(path).unwrap();
    assert!(metadata.is_file());
    assert_eq!(metadata.permissions().mode() & 0o777, 0o755);
    single_thread();
    let _forking = forking();
    let mut child = Command::new(path)
        .process_group(0)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut guard = Child {
        pid: child.id() as libc::pid_t,
        reaped: false,
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            guard.reaped = true;
            break status;
        }
        assert!(Instant::now() < deadline, "updater fixture did not exit");
        std::thread::sleep(Duration::from_millis(5));
    };
    // try_wait already reaped it; wait returns Child's cached terminal status.
    // Keep Rust's ownership explicit while the raw guard bounds error cleanup.
    assert_eq!(child.wait().unwrap(), status);
    assert!(status.success(), "updater fixture exited as {status}");
    let mut text = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .take(128)
        .read_to_string(&mut text)
        .unwrap();
    assert_eq!(
        text,
        if script == OLD {
            "old-updater\n"
        } else {
            "new-updater\n"
        }
    );
}

fn runnable(lamp: &Lamp, current: &str) {
    executable(&lamp.config.on(own::CURRENT).join(own::BINARY), current);
    for script in [OLD, NEW] {
        executable(
            &lamp
                .config
                .on(&unit::folder(own::FOLDER, &sha(script)))
                .join(own::BINARY),
            script,
        );
    }
    executable(&lamp.config.on(own::BACK), OLD);
}

fn recover(lamp: &mut Lamp, case: usize) {
    // This fork starts from the parent's untouched channel/key and no inherited
    // World or records lock. Each existing Lamp method constructs a fresh World.
    if case == 0 {
        assert!(matches!(
            lamp.round(),
            Outcome::SelfSwitched { sequence: 2, .. }
        ));
        assert_eq!(lamp.confirm(&sha(NEW)), own::Confirm::Confirmed);
    } else if matches!(case, 2 | 3) {
        assert_eq!(
            lamp.confirm(&sha(NEW)),
            if case == 2 {
                own::Confirm::Confirmed
            } else {
                own::Confirm::Nothing
            }
        );
    } else {
        let saved = lamp.pending();
        assert_eq!(
            lamp.back(update_unit("failed", "failed"), u64::MAX),
            saved.map_or(own::Back::Nothing, own::Back::PutBack)
        );
    }
    assert!(lamp.pending().is_none());
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
}

pub(crate) fn run_case(name: &str) {
    let case = CASES
        .iter()
        .position(|known| *known == name)
        .expect("known cut case");
    single_thread();
    assert_eq!(size_of::<SyscallInfo>(), 88);
    assert_eq!(offset_of!(SyscallInfo, data), 24);
    let mut lamp = Lamp::installed_with_updater(OLD);
    lamp.config.self_switch = true;
    lamp.systemd.self_recovery = Ok(());
    product_build_with_updater(&mut lamp, 2, NEW);
    if case >= 3 {
        assert!(matches!(
            lamp.round(),
            Outcome::SelfSwitched { sequence: 2, .. }
        ));
    }
    let path = cut(&mut lamp, case);
    assert!(prefix(&lamp, case));
    runnable(&lamp, if matches!(case, 2 | 3) { NEW } else { OLD });
    let mut restarted = fork(&mut lamp, case, true);
    let status = restarted.wait(Instant::now() + WITHIN);
    assert!(
        libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0,
        "recovery failed: wait status {status:#x}"
    );
    let current = if matches!(case, 0 | 2 | 3) { NEW } else { OLD };
    assert!(lamp.pending().is_none());
    assert_eq!(
        own::refused(&lamp.records()),
        if current == OLD {
            vec![sha(NEW)]
        } else {
            vec![]
        }
    );
    runnable(&lamp, current);
    println!(
        "{}",
        serde_json::json!({"case": name, "cut": "fsync exit=0",
        "path": path, "signal": 9, "recovery": "passed", "next_round": "UpToDate(2)"})
    );
}

pub(crate) fn run() {
    for name in CASES {
        run_case(name);
    }
}
