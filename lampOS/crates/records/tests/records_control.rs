#[path = "support/records_control.rs"]
mod control;

use control::{Controller, RunningReporter, Window, protocol::*};
use lamp_msgs::Diagnostic;
use lamp_records::{config::Config, json_file};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    os::unix::fs::PermissionsExt,
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

type Pipes = Channel<File, File>;

// Standard-library pipes coordinate close-on-exec setup with process spawning.
fn private_pipe() -> io::Result<(std::os::fd::OwnedFd, std::os::fd::OwnedFd)> {
    let (reader, writer) = std::io::pipe()?;
    Ok((reader.into(), writer.into()))
}
type ChildPipes = Channel<std::process::ChildStdout, std::process::ChildStdin>;
const NONCE: &str = "recordsParentBoundary";

fn deadline() -> Instant {
    Instant::now() + HOST_WAIT
}

fn channels() -> (Pipes, Pipes) {
    let (requests, input) = private_pipe().unwrap();
    let (output, replies) = private_pipe().unwrap();
    (
        Channel::new(File::from(output), File::from(input)).unwrap(),
        Channel::new(File::from(requests), File::from(replies)).unwrap(),
    )
}

fn reply(step: &Step, cycles: u64, started: u64) -> Checked {
    Checked {
        version: VERSION,
        nonce: step.nonce.clone(),
        round: step.round,
        at_ns: step.at_ns,
        completed_cycles: cycles,
        cycle_started_ns: started,
        next_due_ns: started + PERIOD_NS,
        construction_or_publish: Outcome::ReturnedOk,
    }
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = fs::canonicalize("/tmp").unwrap().join(format!(
            "rct-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
    fn config(&self) -> (PathBuf, Config) {
        let mut config = Config::customer_default();
        config.bus_root = self.0.join("bus");
        let path = self.0.join("config.json");
        json_file::write(&path, &config).unwrap();
        (path, config)
    }
    fn command(&self, seconds: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_lamp-records-fixture"));
        command
            .arg("report")
            .arg(self.0.join("config.json"))
            .arg(self.0.join("last.json"))
            .args(["--seconds", seconds])
            .stderr(Stdio::inherit());
        command
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

// This owner is used only for raw protocol fault tests which deliberately do
// not construct the validated parent Controller. It still owns actual reap.
struct RawChild(Child);
impl Drop for RawChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn raw_child(scratch: &Scratch, seconds: &str) -> (RawChild, ChildPipes, File) {
    let child = scratch
        .command(seconds)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut owned = RawChild(child);
    let input = owned.0.stdin.take().unwrap();
    let output = owned.0.stdout.take().unwrap();
    let probe = File::from(rustix::io::fcntl_dupfd_cloexec(&output, 0).unwrap());
    let channel = Channel::new(output, input).unwrap();
    (owned, channel, probe)
}

fn assert_reaped(pid: u32) {
    let pid = rustix::process::Pid::from_raw(i32::try_from(pid).unwrap()).unwrap();
    assert_eq!(
        rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::NOHANG).unwrap_err(),
        rustix::io::Errno::CHILD
    );
}

#[test]
fn actual_child_returns_three_distinct_events_at_full_due_intervals() {
    let scratch = Scratch::new();
    let (_, config) = scratch.config();
    let bus = lamp_bus::Bus::new(&config.bus_root).unwrap();
    let mut listener = bus
        .subscribe_typed(Diagnostic::TOPIC, "actual", Diagnostic::TYPE)
        .unwrap();
    let mut owned = RunningReporter::spawn(&mut scratch.command("30")).unwrap();
    let pid = owned.0.id();
    let window = Window::new(deadline(), owned.elapsed_ns().unwrap()).unwrap();
    let first = owned.settle_current(&window).unwrap();
    assert_eq!((first.at_ns, first.completed_cycles), (0, 1));
    for count in 1..=3 {
        if count > 1 {
            let checked = owned.step_due(&window).unwrap();
            assert_eq!(
                (checked.at_ns, checked.completed_cycles),
                ((count - 1) * PERIOD_NS, count)
            );
        }
        window.receiving(owned.elapsed_ns().unwrap()).unwrap();
        assert!(owned.0.try_wait().unwrap().is_none());
        let message = listener
            .recv(Duration::ZERO)
            .unwrap()
            .expect("actual Event");
        let diagnostic: Diagnostic = serde_json::from_value(message.json().unwrap()).unwrap();
        assert_eq!(diagnostic.name, "records");
        assert_eq!(message.header.seq, count);
        assert!(listener.recv(Duration::ZERO).unwrap().is_none());
    }
    drop(owned);
    assert_reaped(pid);
}

#[test]
fn returned_ok_without_a_subscriber_is_not_a_delivered_event() {
    let scratch = Scratch::new();
    let (_, config) = scratch.config();
    let mut owned = RunningReporter::spawn(&mut scratch.command("30")).unwrap();
    let window = Window::new(deadline(), 0).unwrap();
    assert_eq!(
        owned
            .settle_current(&window)
            .unwrap()
            .construction_or_publish,
        Outcome::ReturnedOk
    );
    let bus = lamp_bus::Bus::new(&config.bus_root).unwrap();
    let mut listener = bus
        .subscribe_typed(Diagnostic::TOPIC, "late", Diagnostic::TYPE)
        .unwrap();
    assert!(listener.recv(Duration::ZERO).unwrap().is_none());
    let same = owned.settle_current(&window).unwrap();
    assert_eq!(same.completed_cycles, 1);
    assert!(listener.recv(Duration::ZERO).unwrap().is_none());
    assert_eq!(owned.step_due(&window).unwrap().completed_cycles, 2);
    assert!(listener.recv(Duration::ZERO).unwrap().is_some());
}

#[test]
fn blocked_control_keeps_the_real_one_second_lifetime_and_exits_with_error_64() {
    let scratch = Scratch::new();
    scratch.config();
    let began = Instant::now();
    let (mut child, mut channel, _) = raw_child(&scratch, "1");
    let end = deadline();
    channel
        .write_until(
            &Bootstrap {
                version: VERSION,
                nonce: NONCE.into(),
            },
            end,
        )
        .unwrap();
    channel
        .write_until(
            &Step {
                nonce: NONCE.into(),
                round: 1,
                at_ns: 0,
            },
            end,
        )
        .unwrap();
    let checked: Checked = channel.read_until(end).unwrap();
    assert_eq!(checked.completed_cycles, 1);
    assert!(child.0.try_wait().unwrap().is_none());
    assert_eq!(
        channel.read_until::<Checked>(end).unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );
    // EOF is only a transport fact. The actual wait supplies the exit status.
    let status = child.0.wait().unwrap();
    assert_eq!(status.code(), Some(64));
    assert!(began.elapsed() >= Duration::from_secs(1));
    assert_eq!(child.0.try_wait().unwrap(), Some(status));
}

#[test]
fn actual_control_close_requires_actual_error_exit_and_reap() {
    let scratch = Scratch::new();
    scratch.config();
    let (mut child, mut channel, probe) = raw_child(&scratch, "30");
    let end = deadline();
    channel
        .write_until(
            &Bootstrap {
                version: VERSION,
                nonce: NONCE.into(),
            },
            end,
        )
        .unwrap();
    channel
        .write_until(
            &Step {
                nonce: NONCE.into(),
                round: 1,
                at_ns: 0,
            },
            end,
        )
        .unwrap();
    assert_eq!(
        channel.read_until::<Checked>(end).unwrap().completed_cycles,
        1
    );
    assert!(child.0.try_wait().unwrap().is_none());
    channel.close();
    let (_unused, output) = private_pipe().unwrap();
    let mut eof = Channel::new(probe, File::from(output)).unwrap();
    assert_eq!(
        eof.read_until::<Checked>(end).unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );
    assert_eq!(child.0.wait().unwrap().code(), Some(64));
    let pid = child.0.id();
    drop(child);
    assert_reaped(pid);
}

#[test]
fn invalid_bootstrap_is_an_actual_child_error_before_any_reporting_event() {
    for bootstrap in [
        Bootstrap {
            version: VERSION + 1,
            nonce: NONCE.into(),
        },
        Bootstrap {
            version: VERSION,
            nonce: "bad nonce".into(),
        },
    ] {
        let scratch = Scratch::new();
        let (_, config) = scratch.config();
        let bus = lamp_bus::Bus::new(&config.bus_root).unwrap();
        let mut listener = bus
            .subscribe_typed(Diagnostic::TOPIC, "invalid", Diagnostic::TYPE)
            .unwrap();
        let (mut child, mut channel, _) = raw_child(&scratch, "30");
        let end = deadline();
        channel.write_until(&bootstrap, end).unwrap();
        assert_eq!(
            channel.read_until::<Checked>(end).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
        assert_eq!(child.0.wait().unwrap().code(), Some(64));
        assert!(listener.recv(Duration::ZERO).unwrap().is_none());
    }
}

#[test]
fn partial_setup_and_parent_unwind_both_reap_the_owned_child() {
    let scratch = Scratch::new();
    scratch.config();
    let child = scratch
        .command("30")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    assert!(RunningReporter::from_child(child).is_err());
    assert_reaped(pid);
    let owned = RunningReporter::spawn(&mut scratch.command("30")).unwrap();
    let pid = owned.0.id();
    let failure = catch_unwind(AssertUnwindSafe(move || {
        let _owned = owned;
        panic!("injected parent failure");
    }));
    assert!(failure.is_err());
    assert_reaped(pid);
}

#[test]
fn a_complete_checked_queued_before_peer_close_is_consumed_and_committed() {
    {
        let (mut parent, mut peer) = channels();
        let end = deadline();
        let expected = reply(
            &Step {
                nonce: NONCE.into(),
                round: 1,
                at_ns: 0,
            },
            1,
            0,
        );
        peer.write_until(&expected, end).unwrap();
        peer.close();
        let checked: Checked = parent.read_until(end).unwrap();
        assert_eq!(checked, expected);
        assert_eq!(
            parent.read_until::<Checked>(end).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
    thread::scope(|threads| {
        let (parent, mut peer) = channels();
        let end = deadline();
        let server = threads.spawn(move || {
            let bootstrap: Bootstrap = peer.read_until(end).unwrap();
            bootstrap.validate().unwrap();
            let step: Step = peer.read_until(end).unwrap();
            peer.write_until(&reply(&step, 1, 0), end).unwrap();
        });
        let mut controller = Controller::new(parent, NONCE.into()).unwrap();
        let window = Window::new(end, 0).unwrap();
        let checked = controller.settle_current(&window).unwrap();
        assert_eq!(checked.completed_cycles, 1);
        assert_eq!(controller.elapsed_ns().unwrap(), 0);
        server.join().unwrap();
        assert!(controller.step_due(&window).is_err());
        assert!(controller.elapsed_ns().is_err());
    });
}

#[test]
fn every_initial_response_identity_and_cycle_field_is_validated() {
    for fault in 0..7 {
        thread::scope(|threads| {
            let (parent, mut peer) = channels();
            let end = deadline();
            let server = threads.spawn(move || {
                let _: Bootstrap = peer.read_until(end).unwrap();
                let step: Step = peer.read_until(end).unwrap();
                let mut checked = reply(&step, 1, 0);
                match fault {
                    0 => checked.version += 1,
                    1 => checked.nonce = "different".into(),
                    2 => checked.round += 1,
                    3 => checked.at_ns += 1,
                    4 => checked.completed_cycles += 1,
                    5 => checked.cycle_started_ns += 1,
                    _ => checked.next_due_ns += 1,
                }
                peer.write_until(&checked, end).unwrap();
            });
            let mut controller = Controller::new(parent, NONCE.into()).unwrap();
            let window = Window::new(end, 0).unwrap();
            assert!(controller.settle_current(&window).is_err());
            assert!(controller.elapsed_ns().is_err());
            assert!(controller.settle_current(&window).is_err());
            server.join().unwrap();
        });
    }
}

#[test]
fn same_time_and_pre_due_replies_cannot_fabricate_new_work_or_change_outcome() {
    for fault in 0..4 {
        thread::scope(|threads| {
            let (parent, mut peer) = channels();
            let end = deadline();
            let server = threads.spawn(move || {
                let _: Bootstrap = peer.read_until(end).unwrap();
                let first: Step = peer.read_until(end).unwrap();
                peer.write_until(&reply(&first, 1, 0), end).unwrap();
                let step: Step = peer.read_until(end).unwrap();
                let mut checked = reply(&step, 1, 0);
                match fault {
                    0 => checked.completed_cycles += 1,
                    1 => checked.cycle_started_ns += 1,
                    2 => checked.next_due_ns += 1,
                    _ => checked.construction_or_publish = Outcome::ReturnedErr,
                }
                peer.write_until(&checked, end).unwrap();
            });
            let mut controller = Controller::new(parent, NONCE.into()).unwrap();
            let window = Window::new(end, 0).unwrap();
            controller.settle_current(&window).unwrap();
            assert!(
                controller
                    .step_to(if fault == 0 { 0 } else { PERIOD_NS - 1 }, &window)
                    .is_err()
            );
            assert!(controller.elapsed_ns().is_err());
            server.join().unwrap();
        });
    }
}

#[test]
fn malformed_due_receipt_after_peer_advance_latches_without_parent_time_commit() {
    for fault in 0..3 {
        thread::scope(|threads| {
            let (parent, mut peer) = channels();
            let end = deadline();
            let server = threads.spawn(move || {
                let _: Bootstrap = peer.read_until(end).unwrap();
                let first: Step = peer.read_until(end).unwrap();
                peer.write_until(&reply(&first, 1, 0), end).unwrap();
                let due: Step = peer.read_until(end).unwrap();
                assert_eq!(due.at_ns, PERIOD_NS);
                let mut checked = reply(&due, 2, PERIOD_NS);
                match fault {
                    0 => checked.completed_cycles = 1,
                    1 => checked.cycle_started_ns = 0,
                    _ => checked.next_due_ns += 1,
                }
                peer.write_until(&checked, end).unwrap();
            });
            let mut controller = Controller::new(parent, NONCE.into()).unwrap();
            let window = Window::new(end, 0).unwrap();
            controller.settle_current(&window).unwrap();
            assert!(controller.step_due(&window).is_err());
            assert!(controller.elapsed_ns().is_err());
            assert!(controller.step_due(&window).is_err());
            server.join().unwrap();
        });
    }
}

#[test]
fn parent_rejects_future_steps_and_one_window_retains_full_periods_and_end() {
    thread::scope(|threads| {
        let (parent, mut peer) = channels();
        let end = deadline();
        let server = threads.spawn(move || {
            let _: Bootstrap = peer.read_until(end).unwrap();
            for cycle in 0..=4 {
                let step: Step = peer.read_until(end).unwrap();
                assert_eq!(step.at_ns, cycle * PERIOD_NS);
                peer.write_until(&reply(&step, cycle + 1, step.at_ns), end)
                    .unwrap();
            }
        });
        let mut controller = Controller::new(parent, NONCE.into()).unwrap();
        let window = Window::new(end, 0).unwrap();
        controller.settle_current(&window).unwrap();
        for at in 1..=4 {
            assert_eq!(controller.step_due(&window).unwrap().at_ns, at * PERIOD_NS);
            assert_eq!(
                window.receiving(controller.elapsed_ns().unwrap()).is_ok(),
                at < 4
            );
        }
        assert!(controller.step_due(&window).is_err());
        assert!(controller.elapsed_ns().is_err());
        server.join().unwrap();
    });
    let (parent, _peer) = channels();
    let mut controller = Controller::new(parent, NONCE.into()).unwrap();
    let window = Window::new(deadline(), 0).unwrap();
    assert!(controller.step_due(&window).is_err());
    assert!(controller.elapsed_ns().is_err());
    assert!(Window::new(deadline(), u64::MAX).is_err());
    assert!(Window::new(Instant::now(), 0).is_err());
}

fn raw_channel(bytes: &[u8]) -> Pipes {
    let (reader, writer) = private_pipe().unwrap();
    let (unused, output) = private_pipe().unwrap();
    // These malformed/truncated frames are much smaller than PIPE_BUF.
    File::from(writer).write_all(bytes).unwrap();
    drop(unused);
    Channel::new(File::from(reader), File::from(output)).unwrap()
}

#[test]
fn framing_rejects_truncation_unknown_fields_and_malformed_json() {
    for bytes in [
        b"{\"version\":1".as_slice(),
        b"not-json\n",
        b"{\"version\":1,\"nonce\":\"ok\",\"extra\":1}\n",
    ] {
        assert!(
            raw_channel(bytes)
                .read_until::<Bootstrap>(deadline())
                .is_err()
        );
    }
    let (mut parent, _peer) = channels();
    assert_eq!(
        parent
            .write_until(&"x".repeat(MAX_FRAME_BYTES), deadline())
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(
        parent
            .read_until::<Bootstrap>(Instant::now())
            .unwrap_err()
            .kind(),
        io::ErrorKind::TimedOut
    );
}

fn write_raw(file: &mut File, bytes: &[u8], end: Instant) {
    let flags = rustix::fs::fcntl_getfl(&*file).unwrap();
    rustix::fs::fcntl_setfl(&*file, flags | rustix::fs::OFlags::NONBLOCK).unwrap();
    let mut sent = 0;
    while sent < bytes.len() {
        let left = remaining(end).unwrap();
        match file.write(&bytes[sent..sent + 1]) {
            Ok(1) => sent += 1,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                let mut fds = [rustix::event::PollFd::new(
                    &*file,
                    rustix::event::PollFlags::OUT,
                )];
                let timeout = rustix::event::Timespec::try_from(left).unwrap();
                match rustix::event::poll(&mut fds, Some(&timeout)) {
                    Ok(_) => {}
                    Err(error) if error == rustix::io::Errno::INTR => {}
                    Err(error) => panic!("raw frame poll: {error}"),
                }
            }
            other => panic!("raw frame write: {other:?}"),
        }
    }
}

#[test]
fn fragmented_real_pipe_frames_work_and_oversized_reads_fail_at_the_byte_bound() {
    for oversized in [false, true] {
        thread::scope(|threads| {
            let (read, write) = private_pipe().unwrap();
            let (_unused, output) = private_pipe().unwrap();
            let end = deadline();
            let mut frame = if oversized {
                vec![b' '; MAX_FRAME_BYTES]
            } else {
                serde_json::to_vec(&Bootstrap {
                    version: VERSION,
                    nonce: NONCE.into(),
                })
                .unwrap()
            };
            frame.push(b'\n');
            let sender = threads.spawn(move || write_raw(&mut File::from(write), &frame, end));
            let mut channel = Channel::new(File::from(read), File::from(output)).unwrap();
            let result = channel.read_until::<Bootstrap>(end);
            if oversized {
                assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
            } else {
                let bootstrap = result.unwrap();
                bootstrap.validate().unwrap();
                assert_eq!(bootstrap.nonce, NONCE);
            }
            sender.join().unwrap();
        });
    }
}

#[test]
fn filled_real_pipe_write_uses_the_single_absolute_host_deadline() {
    let (read, write) = private_pipe().unwrap();
    let mut filler = File::from(write);
    let duplicate = filler.try_clone().unwrap();
    let flags = rustix::fs::fcntl_getfl(&filler).unwrap();
    rustix::fs::fcntl_setfl(&filler, flags | rustix::fs::OFlags::NONBLOCK).unwrap();
    let fill_end = deadline();
    loop {
        remaining(fill_end).unwrap();
        match filler.write(&[0; 4096]) {
            Ok(0) => panic!("pipe fill wrote zero"),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => panic!("pipe fill: {error}"),
        }
    }
    // Fill any last sub-PIPE_BUF space, so even the short frame cannot fit.
    loop {
        remaining(fill_end).unwrap();
        match filler.write(&[0]) {
            Ok(1) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            other => panic!("pipe tail fill: {other:?}"),
        }
    }
    let (dummy_read, _dummy_write) = private_pipe().unwrap();
    let mut channel = Channel::new(File::from(dummy_read), duplicate).unwrap();
    // The test's own deadline, not the fixtures' `HOST_WAIT` (4 s): the write
    // waits the whole of it on the full pipe, and the second write finds it
    // already past. Any length checks the same; a short one costs no time
    // (#1128).
    let end = Instant::now() + Duration::from_millis(200);
    assert_eq!(
        channel
            .write_until(
                &Bootstrap {
                    version: VERSION,
                    nonce: NONCE.into()
                },
                end
            )
            .unwrap_err()
            .kind(),
        io::ErrorKind::TimedOut
    );
    assert!(Instant::now() >= end);
    let mut drain = File::from(read);
    let mut bytes = [0; 1];
    drain.read_exact(&mut bytes).unwrap();
    assert_eq!(bytes[0], 0);
    assert_eq!(
        channel
            .write_until(
                &Bootstrap {
                    version: VERSION,
                    nonce: NONCE.into()
                },
                end
            )
            .unwrap_err()
            .kind(),
        io::ErrorKind::TimedOut
    );
}
