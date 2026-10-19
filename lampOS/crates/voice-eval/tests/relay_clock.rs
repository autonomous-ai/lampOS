//! Relay clock boundaries with an inert child and explicit event barriers.
//! The clock advances only in the test driver; child/transport I/O stays real.
mod common;

use lamp_turn_owner::MonoTime;
use lamp_voice_eval::physical::{
    protocol::SessionLine,
    session::{self, SERVE_STOP_GRACE_US, SessionOptions},
};
use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    net::Shutdown,
    os::unix::net::{UnixDatagram, UnixStream},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

const EPOCH_US: u64 = 1_000_000;
const SESSION_US: u64 = 6_000_000; // One second plus the minimum five-second allowance.
const WATCHDOG: Duration = Duration::from_secs(10);

// No workers or audio. The FIFO keeps the child blocked without elapsed-time
// synchronization. SIGTERM is acknowledged but intentionally does not exit.
// Perl, not /bin/sh: on macOS /bin/sh is Bash 3.2, whose `read` drops bytes
// of the next line once a TERM trap has run during it ("probe" read as
// "prob", in 11 to 21 of 1000 rounds of this exchange alone; #1569 row 31).
// Perl runs its handler between operations and its read keeps what it has.
const CHILD: &str = r#"
$| = 1;
$SIG{TERM} = sub { print "signal\n" };
open(my $control, '<', $ARGV[0]) or die "control: $!";
print "ready\n";
while (1) {
    my $command = <$control>;
    next unless defined $command;
    chomp $command;
    exit 0 if $command eq 'exit';
    print "probe\n" if $command eq 'probe';
}
"#;

struct Lines {
    pending: Vec<u8>,
    sender: Sender<SessionLine>,
    abort: Arc<AtomicBool>,
}
impl Write for Lines {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.abort.load(Ordering::SeqCst) {
            return Err(io::Error::other("test driver ended"));
        }
        self.pending.extend_from_slice(bytes);
        while let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
            let line = serde_json::from_slice(&self.pending[..end])?;
            self.pending.drain(..=end);
            self.sender
                .send(line)
                .map_err(|error| io::Error::other(error.to_string()))?;
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Relay {
    thread: Option<JoinHandle<Result<i32, String>>>,
    done: Receiver<()>,
    reads: Arc<AtomicU64>,
    read_events: Receiver<u64>,
    runner: UnixStream,
    child_control: File,
    lines: Receiver<SessionLine>,
    clock: Arc<AtomicU64>,
    abort: Arc<AtomicBool>,
    directory: common::Private,
}
impl Relay {
    fn start() -> Self {
        let directory = common::Private::new("clk");
        let fifo = directory.join("control");
        assert!(
            Command::new("mkfifo")
                .args(["-m", "600"])
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        // Keep a reader/writer open so neither endpoint's open needs a race.
        let child_control = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&fifo)
            .unwrap();
        let mut args = vec!["--runtime".into(), "/usr/bin/perl".into()];
        for arg in ["-e", CHILD, fifo.to_str().unwrap()] {
            args.extend(["--runtime-prefix-arg".into(), arg.into()]);
        }
        args.extend([
            "--work".into(),
            directory.join("work").display().to_string(),
            "--seconds".into(),
            "1".into(),
            "--allowance-seconds".into(),
            "5".into(),
            "--serve-config".into(),
            "/unused/inert-child.json".into(),
        ]);
        let options = SessionOptions::parse(&args).unwrap();
        let (runner, input) = UnixStream::pair().unwrap();
        let (sender, lines) = mpsc::channel();
        let clock = Arc::new(AtomicU64::new(EPOCH_US));
        let source = Arc::clone(&clock);
        let abort = Arc::new(AtomicBool::new(false));
        let reads = Arc::new(AtomicU64::new(0));
        let observed_reads = Arc::clone(&reads);
        let (read_sender, read_events) = mpsc::channel();
        let (finished, done) = mpsc::channel();
        let mut output = Lines {
            pending: Vec::new(),
            sender,
            abort: Arc::clone(&abort),
        };
        let thread = thread::spawn(move || {
            let result = session::run_with_clock(&options, input, &mut output, || {
                let at_us = source.load(Ordering::SeqCst);
                let read = observed_reads.fetch_add(1, Ordering::SeqCst) + 1;
                let _ = read_sender.send(read);
                MonoTime::from_micros(at_us)
            })
            .map_err(|error| error.to_string());
            if let Err(error) = &result {
                eprintln!("relay failed: {error}");
            }
            let _ = finished.send(());
            result
        });
        let relay = Self {
            thread: Some(thread),
            done,
            reads,
            read_events,
            runner,
            child_control,
            lines,
            clock,
            abort,
            directory,
        };
        assert!(matches!(
            relay.next(),
            SessionLine::SessionStart {
                lamp_us: EPOCH_US,
                ..
            }
        ));
        assert_eq!(relay.stdout("ready"), EPOCH_US);
        relay
    }
    fn next(&self) -> SessionLine {
        self.lines.recv_timeout(WATCHDOG).expect("relay progress")
    }
    fn stdout(&self, expected: &str) -> u64 {
        match self.next() {
            SessionLine::Stdout {
                lamp_received_us,
                line,
            } if line == expected => lamp_received_us,
            other => panic!("expected stdout {expected:?}, got {other:?}"),
        }
    }
    fn set_time(&self, at_us: u64) {
        assert!(at_us >= self.clock.load(Ordering::SeqCst));
        self.clock.store(at_us, Ordering::SeqCst);
    }
    fn ping(&mut self, seq: u64) {
        writeln!(self.runner, "{{\"type\":\"ping\",\"seq\":{seq}}}").unwrap();
        assert_eq!(
            self.next(),
            SessionLine::Pong {
                seq,
                lamp_us: self.clock.load(Ordering::SeqCst),
            }
        );
    }
    fn complete_quiet_checks(&self) {
        // A Pong/Stdout is emitted before that pass checks its deadlines. With
        // no new input/output pending, three later clock reads prove at least
        // one hard-deadline and (when present) grace check completed at t.
        // Instrumentation observes reads; it never moves the clock.
        let after = self.reads.load(Ordering::SeqCst) + 3;
        while self
            .read_events
            .recv_timeout(WATCHDOG)
            .expect("clock checks")
            < after
        {}
    }
    fn child_command(&mut self, command: &str) {
        writeln!(self.child_control, "{command}").unwrap();
    }
    fn finish(&mut self) -> SessionLine {
        let end = self.next();
        assert!(matches!(end, SessionLine::SessionEnd { .. }), "{end:?}");
        let expected = if matches!(end, SessionLine::SessionEnd { killed: true, .. }) {
            3
        } else {
            0
        };
        self.done.recv_timeout(WATCHDOG).expect("relay cleanup");
        assert_eq!(
            self.thread.take().unwrap().join().unwrap().unwrap(),
            expected
        );
        end
    }
}
impl Drop for Relay {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            // On assertion failure, wake both endpoints. The next output fails
            // and the relay's Cleanup still kills/reaps its owned child group.
            self.abort.store(true, Ordering::SeqCst);
            let _ = writeln!(self.child_control, "exit");
            let _ = writeln!(self.runner, "{{\"type\":\"ping\",\"seq\":999}}");
            let _ = self.runner.shutdown(Shutdown::Both);
            // The driver never waits without a host bound after a failure.
            // Both transport ends have been woken so Cleanup can still run.
            if self.done.recv_timeout(WATCHDOG).is_ok() {
                let _ = thread.join();
            }
        }
    }
}

#[test]
fn receipt_and_exit_timestamps_use_only_the_explicit_clock() {
    let mut relay = Relay::start();
    relay.ping(1);
    relay.ping(2);
    assert_eq!(relay.clock.load(Ordering::SeqCst), EPOCH_US);

    let advanced = EPOCH_US + 1_234;
    relay.set_time(advanced);
    relay.child_command("probe");
    assert_eq!(relay.stdout("probe"), advanced);
    relay.ping(3);
    UnixDatagram::unbound()
        .unwrap()
        .send_to(b"invalid cue", relay.directory.join("work/cue.sock"))
        .unwrap();
    assert!(
        matches!(relay.next(), SessionLine::CueInvalid { lamp_received_us, .. } if lamp_received_us == advanced)
    );

    relay.child_command("exit");
    let end = relay.finish();
    let SessionLine::SessionEnd {
        lamp_us,
        exit_code,
        killed,
        serve: Some(serve),
        ..
    } = end
    else {
        panic!("{end:?}")
    };
    assert_eq!(lamp_us, advanced);
    assert_eq!(exit_code, Some(0));
    assert!(!killed && serve.group_reaped);
    assert_eq!(serve.stop_cause, "runtime_exited");
    assert_eq!(serve.exited_us, Some(advanced));
    assert_eq!(serve.stop_requested_us, None);
}

#[test]
fn hard_deadline_and_stop_grace_expire_at_their_original_boundaries() {
    let mut relay = Relay::start();
    let deadline = EPOCH_US + SESSION_US;
    relay.set_time(deadline - 1);
    relay.ping(1);
    relay.child_command("probe");
    assert_eq!(relay.stdout("probe"), deadline - 1);
    relay.complete_quiet_checks();

    relay.set_time(deadline);
    assert_eq!(relay.stdout("signal"), deadline);
    let grace_end = deadline + SERVE_STOP_GRACE_US;
    relay.set_time(grace_end - 1);
    relay.ping(2);
    relay.child_command("probe");
    assert_eq!(relay.stdout("probe"), grace_end - 1);
    relay.complete_quiet_checks();

    relay.set_time(grace_end);
    let end = relay.finish();
    let SessionLine::SessionEnd {
        lamp_us,
        killed,
        serve: Some(serve),
        ..
    } = end
    else {
        panic!("{end:?}")
    };
    assert_eq!(lamp_us, grace_end);
    assert!(killed && serve.forced_kill && serve.group_reaped);
    assert_eq!(serve.stop_cause, "deadline");
    assert_eq!(serve.stop_requested_us, Some(deadline));
    assert_eq!(serve.signal_sent_us, Some(deadline));
    assert_eq!(serve.exited_us, None);
}
