//! Real builtin-only children and test-owned descriptors. Timed gate release is
//! an intervention, never the condition that makes the observed fixture pass.
use super::super::{OLD, Running, assert_runnable_output, forking};
use super::*;
use std::{
    os::{fd::OwnedFd, unix::net::UnixStream},
    panic::{AssertUnwindSafe, catch_unwind},
    process::{ChildStdin, Command, Stdio},
    sync::mpsc,
    thread::{self, JoinHandle},
};

fn spawn(script: &str, stdout: Stdio) -> Running {
    Running(
        Command::new("/bin/sh")
            .args(["-c", script])
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(stdout)
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    )
}

fn observed(script: &str) -> Result<(ExitStatus, Vec<u8>), String> {
    let mut child = spawn(script, Stdio::piped());
    let stdout = child.0.stdout.take().unwrap();
    let flags = rustix::fs::fcntl_getfl(&stdout).unwrap();
    let result = observe(&mut child.0, &stdout, Instant::now());
    reap(&mut child);
    assert_eq!(rustix::fs::fcntl_getfl(&stdout).unwrap(), flags);
    result
}

fn reap(child: &mut Running) {
    let pid = child.0.id();
    let killed = child.0.kill();
    let waited = child.0.wait();
    let cached = child.0.try_wait();
    eprintln!("control cleanup: pid={pid} kill={killed:?} wait={waited:?} cached={cached:?}");
    let status = waited.unwrap();
    assert_eq!(
        cached.unwrap(),
        Some(status),
        "direct child must be reaped; pid={pid} kill={killed:?}"
    );
}

// Cancellation releases the controller promptly on every unwind. The only
// write is nonblocking and three bytes long; no join waits for the child.
type Releases = Vec<(Duration, usize)>;

struct Gate {
    cancel: mpsc::Sender<()>,
    worker: Option<JoinHandle<io::Result<Releases>>>,
}

impl Gate {
    fn release_after(stdin: ChildStdin, delay: Duration, started: Instant) -> Self {
        Self::release_at(stdin, vec![delay], started)
    }

    fn release_at(stdin: ChildStdin, times: Vec<Duration>, started: Instant) -> Self {
        let flags = rustix::fs::fcntl_getfl(&stdin).unwrap();
        rustix::fs::fcntl_setfl(&stdin, flags | rustix::fs::OFlags::NONBLOCK).unwrap();
        let (cancel, receive) = mpsc::channel();
        let worker = thread::spawn(move || {
            let mut releases = Vec::new();
            for time in times {
                let remaining = (started + time).saturating_duration_since(Instant::now());
                match receive.recv_timeout(remaining) {
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        let count = rustix::io::write(&stdin, b"go\n")?;
                        releases.push((started.elapsed(), count));
                    }
                    _ => return Ok(releases),
                }
            }
            // Keep stdin owned/open for an unfinished final builtin read. Our
            // owner cancels this channel before joining on success or unwind.
            let _ = receive.recv();
            Ok(releases)
        });
        Self {
            cancel,
            worker: Some(worker),
        }
    }

    fn finish(mut self) -> io::Result<Releases> {
        let _ = self.cancel.send(());
        self.worker.take().unwrap().join().unwrap()
    }
}

impl Drop for Gate {
    fn drop(&mut self) {
        let _ = self.cancel.send(());
        if let Some(worker) = self.worker.take() {
            // The controller waits on our channel, never on process completion.
            let _ = worker.join();
        }
    }
}

#[test]
fn first_output_can_follow_a_gate_longer_than_the_old_spawn_watchdog() {
    let _forking = forking();
    let mut child = spawn("read gate; printf 'old-updater\\n'", Stdio::piped());
    let started = Instant::now();
    let stdout = child.0.stdout.take().unwrap();
    let gate = Gate::release_after(
        child.0.stdin.take().unwrap(),
        Duration::from_millis(2200),
        started,
    );
    let result = observe(&mut child.0, &stdout, started);
    let releases = gate.finish();
    reap(&mut child);
    let (status, output) = result.unwrap();
    let releases = releases.unwrap();
    assert_eq!(releases.len(), 1);
    assert!(releases[0].0 > Duration::from_secs(2));
    assert_eq!(releases[0].1, 3);
    assert_runnable_output(status, output, OLD, "pre-output gate control");
}

#[test]
fn full_output_without_exit_hits_the_absolute_finish_watchdog() {
    let _forking = forking();
    let failed = observed("printf 'old-updater\\n'; read gate").unwrap_err();
    assert!(failed.contains("exit/EOF watchdog"), "{failed}");
    assert!(
        failed.contains("output=[111, 108, 100, 45, 117, 112, 100, 97, 116, 101, 114, 10]"),
        "{failed}"
    );
}

#[test]
fn a_partial_first_output_also_starts_the_finish_watchdog() {
    let _forking = forking();
    let failed = observed("printf 'o'; read gate").unwrap_err();
    assert!(failed.contains("exit/EOF watchdog"), "{failed}");
    assert!(failed.contains("output=[111]"), "{failed}");
}

#[test]
fn an_exited_leader_does_not_make_a_held_output_descriptor_complete() {
    let _forking = forking();
    let (stdout, held_writer) = UnixStream::pair().unwrap();
    let child_writer: OwnedFd = held_writer.try_clone().unwrap().into();
    let mut child = spawn("printf 'old-updater\\n'", Stdio::from(child_writer));
    let result = observe(&mut child.0, &stdout, Instant::now());
    let status = child.0.try_wait();
    reap(&mut child);
    // No child/grandchild owns the extra writer; this test releases it itself.
    drop(held_writer);
    let failed = result.unwrap_err();
    assert!(failed.contains("exit/EOF watchdog"), "{failed}");
    assert!(failed.contains("eof=false"), "{failed}");
    assert!(status.unwrap().unwrap().success());
}

#[test]
fn wrong_payload_nonzero_status_and_non_utf8_cannot_satisfy_the_fixture() {
    let _forking = forking();
    for script in [
        "printf 'wrong\\n'",
        "printf 'old-updater\\n'; exit 7",
        "printf '\\377\\n'",
    ] {
        let (status, output) = observed(script).unwrap();
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                assert_runnable_output(status, output, OLD, script);
            }))
            .is_err(),
            "invalid fixture passed: {script}"
        );
    }
}

#[test]
fn no_output_is_not_readiness() {
    let _forking = forking();
    let failed = observed("exit 0").unwrap_err();
    assert!(failed.contains("without output"), "{failed}");
}

#[test]
fn output_beyond_the_previous_capture_limit_fails_explicitly() {
    let _forking = forking();
    let failed = observed("printf '%0130d' 0").unwrap_err();
    assert!(failed.contains("stdout exceeded 128 bytes"), "{failed}");
}

#[test]
fn a_read_error_restores_flags_and_leaves_the_direct_child_owned() {
    let _forking = forking();
    let mut child = spawn("read gate", Stdio::piped());
    let not_readable = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/null")
        .unwrap();
    let flags = rustix::fs::fcntl_getfl(&not_readable).unwrap();
    let result = observe(&mut child.0, &not_readable, Instant::now());
    let restored = rustix::fs::fcntl_getfl(&not_readable);
    reap(&mut child);
    let failed = result.unwrap_err();
    assert!(failed.contains("stdout read failed"), "{failed}");
    assert_eq!(restored.unwrap(), flags);
}

#[test]
fn a_never_released_first_output_gate_hits_the_startup_watchdog() {
    let _forking = forking();
    let failed = observed("read gate").unwrap_err();
    assert!(failed.contains("startup watchdog"), "{failed}");
    assert!(failed.contains("first_byte=None"), "{failed}");
    assert!(failed.contains("output=[]"), "{failed}");
}

#[test]
fn later_bytes_do_not_renew_the_first_byte_finish_deadline() {
    let _forking = forking();
    let mut child = spawn(
        "printf 'o'; read one; printf 'l'; read two; printf 'd'; read end",
        Stdio::piped(),
    );
    let started = Instant::now();
    let stdout = child.0.stdout.take().unwrap();
    let gate = Gate::release_at(
        child.0.stdin.take().unwrap(),
        vec![Duration::from_millis(700), Duration::from_millis(1400)],
        started,
    );
    let result = observe(&mut child.0, &stdout, started);
    let finished = started.elapsed();
    let releases = gate.finish();
    reap(&mut child);
    let failed = result.unwrap_err();
    let releases = releases.unwrap();
    eprintln!(
        "later-byte control: releases={releases:?} finished={finished:?} observation={failed}"
    );
    assert_eq!(releases.len(), 2);
    assert!(releases.iter().all(|(_, count)| *count == 3));
    assert!(failed.contains("exit/EOF watchdog"), "{failed}");
    assert!(failed.contains("output=[111, 108, 100]"), "{failed}");
    // If FINISH renewed at the last byte, its two seconds could not have elapsed yet.
    // A delayed controller or parent therefore fails this control as invalid.
    assert!(
        finished.saturating_sub(releases[1].0) < FINISH,
        "{releases:?}; {failed}"
    );
}
