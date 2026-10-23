//! The Lamp-side relay (`lamp-session`) with a shell script standing in for
//! lamp-voice. Each case checks one explicit framing, trace or cleanup
//! failure; nothing is truncated, lossily repaired or left running.
use crate::common;

use lamp_voice_eval::physical::{
    protocol::{MAX_LINE_BYTES, SessionLine},
    session::{SessionOptions, run},
};
use std::{
    fs,
    os::unix::net::UnixStream,
    path::Path,
    time::{Duration, Instant},
};

/// Run the relay with `/bin/sh -c SCRIPT`. The script receives lamp-voice's
/// arguments; `$5` is the runtime output directory it must create. With
/// `close_runner_after`, the runner end closes once that file exists.
fn relay(
    dir: &common::Private,
    script: &str,
    close_runner_after: Option<&Path>,
) -> (i32, Vec<SessionLine>) {
    let work = dir.join("work");
    let arguments: Vec<String> = [
        "--runtime",
        "/bin/sh",
        "--runtime-prefix-arg",
        "-c",
        "--runtime-prefix-arg",
        script,
        "--runtime-prefix-arg",
        "fake-lamp-voice",
        "--work",
        &work.display().to_string(),
        "--seconds",
        "5",
        "--allowance-seconds",
        "5",
        "--fixture-reply",
        "/nonexistent/reply.wav",
        "--fixture-sha256",
        &"0".repeat(64),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let options = SessionOptions::parse(&arguments).unwrap();
    let (input, runner) = UnixStream::pair().unwrap();
    // Without a close condition the runner end stays open for the whole run.
    let mut held = Some(runner);
    let closer = close_runner_after.map(|path| {
        let path = path.to_owned();
        let runner = held.take();
        std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !path.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            drop(runner);
        })
    });
    let mut out = Vec::new();
    let code = run(&options, input, &mut out).unwrap();
    if let Some(closer) = closer {
        closer.join().unwrap();
    }
    drop(held);
    let lines = out
        .split(|&b| b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| {
            assert!(
                line.len() <= MAX_LINE_BYTES,
                "relay emitted an oversized line"
            );
            serde_json::from_slice(line).unwrap()
        })
        .collect();
    (code, lines)
}

fn end(lines: &[SessionLine]) -> (Option<String>, usize, Option<String>, bool, bool) {
    match lines.last() {
        Some(SessionLine::SessionEnd {
            error,
            events_lines,
            events_sha256,
            killed,
            stray_processes_stopped,
            ..
        }) => (
            error.clone(),
            *events_lines,
            events_sha256.clone(),
            *killed,
            *stray_processes_stopped,
        ),
        other => panic!("no session_end: {other:?}"),
    }
}

fn traces(lines: &[SessionLine]) -> usize {
    lines
        .iter()
        .filter(|l| matches!(l, SessionLine::Trace { .. }))
        .count()
}

const TRACE: &str = r#"mkdir "$5" && printf '%s\n' '{"kind":"run_start","at_us":1}' '{"kind":"run_end","at_us":2}' > "$5/events.jsonl""#;

fn process_gone(pid: i32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(3);
    let pid = rustix::process::Pid::from_raw(pid).unwrap();
    while Instant::now() < deadline {
        if rustix::process::test_kill_process(pid).is_err() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn read_pid(path: &Path) -> i32 {
    fs::read_to_string(path).unwrap().trim().parse().unwrap()
}

#[test]
fn oversized_and_non_utf8_stdout_lines_are_explicit_failures() {
    let dir = common::Private::new("rso");
    let script = format!(
        r#"{TRACE}
head -c 20000 /dev/zero | tr '\0' a; echo
printf '\377\376\n'
echo '{{"status":"listening_ready"}}'"#
    );
    let (code, lines) = relay(&dir, &script, None);
    assert_eq!(code, 0);
    let invalid: Vec<_> = lines
        .iter()
        .filter_map(|l| match l {
            SessionLine::StdoutInvalid { error, .. } => Some(error.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(invalid.len(), 2, "{lines:?}");
    assert!(invalid[0].contains("20000 bytes exceeds the 16384-byte limit"));
    assert!(invalid[1].contains("not valid UTF-8"));
    // The valid line after the failures is still relayed intact.
    assert!(lines.iter().any(|l| matches!(l,
        SessionLine::Stdout { line, .. } if line == r#"{"status":"listening_ready"}"#)));
    let (error, events, sha, killed, _) = end(&lines);
    assert!(error.unwrap().contains("exceeds the 16384-byte limit"));
    assert_eq!((events, traces(&lines), killed), (2, 2, false));
    assert!(sha.is_some());
}

#[test]
fn a_cut_off_final_trace_record_is_reported_and_the_digest_covers_the_whole_file() {
    let dir = common::Private::new("rst");
    let script = r#"mkdir "$5" && printf '%s\n%s' '{"kind":"run_start","at_us":1}' '{"kind":"run_end","at' > "$5/events.jsonl""#;
    let (_, lines) = relay(&dir, script, None);
    let (error, events, sha, _, _) = end(&lines);
    assert!(error.unwrap().contains("trace closed inside a line"));
    assert_eq!((events, traces(&lines)), (1, 1));
    let bytes = fs::read(dir.join("work/runtime/events.jsonl")).unwrap();
    assert_eq!(sha.unwrap(), lamp_sound_fixtures::hash(&bytes));
}

#[test]
fn oversized_or_non_utf8_trace_records_stop_the_trace_explicitly() {
    for (label, record, expected) in [
        (
            "big",
            r#"head -c 70000 /dev/zero | tr '\0' a"#,
            "exceeds the 64512-byte limit",
        ),
        ("utf", r#"printf '\377'"#, "not valid UTF-8"),
        ("json", "printf '{not json'", "malformed trace record 2"),
    ] {
        let dir = common::Private::new(&format!("rs{label}"));
        let script = format!(
            r#"mkdir "$5" && {{ echo '{{"kind":"run_start","at_us":1}}'; {record}; echo; echo '{{"kind":"run_end","at_us":3}}'; }} > "$5/events.jsonl""#
        );
        let (_, lines) = relay(&dir, &script, None);
        let (error, events, sha, _, _) = end(&lines);
        let error = error.unwrap();
        assert!(error.contains(expected), "{label}: {error}");
        // Streaming stops at the failure; the later valid record is not
        // relayed as if the trace were whole, but the digest still covers it.
        assert_eq!((events, traces(&lines)), (1, 1), "{label}");
        let bytes = fs::read(dir.join("work/runtime/events.jsonl")).unwrap();
        assert_eq!(sha.unwrap(), lamp_sound_fixtures::hash(&bytes), "{label}");
    }
}

#[test]
fn a_closed_runner_transport_stops_the_runtime_and_its_workers() {
    let dir = common::Private::new("rsc");
    let pid_file = dir.join("worker.pid");
    let ready_file = dir.join("worker.ready");
    let script = format!(
        r#"{TRACE}
sleep 30 &
echo $! > {} && : > {}
exec sleep 30"#,
        pid_file.display(),
        ready_file.display()
    );
    let started = Instant::now();
    let (code, lines) = relay(&dir, &script, Some(&ready_file));
    // The runtime would otherwise run for its full 5 s session.
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "runtime kept running"
    );
    assert_eq!(code, 3);
    let (error, events, _, killed, _) = end(&lines);
    assert!(killed);
    assert!(
        error
            .unwrap()
            .contains("runner transport closed before the runtime ended")
    );
    // The trace written before the stop is still relayed.
    assert_eq!(events, 2);
    assert!(
        process_gone(read_pid(&pid_file)),
        "worker survived the stop"
    );
}

#[test]
fn workers_left_after_a_normal_exit_are_stopped_and_reported() {
    let dir = common::Private::new("rsw");
    let pid_file = dir.join("worker.pid");
    let script = format!(
        r#"{TRACE}
sleep 30 &
echo $! > {}"#,
        pid_file.display()
    );
    let (code, lines) = relay(&dir, &script, None);
    assert_eq!(code, 0);
    let (error, events, _, killed, stray) = end(&lines);
    assert_eq!((error, events, killed, stray), (None, 2, false, true));
    assert!(
        process_gone(read_pid(&pid_file)),
        "worker survived the relay"
    );
}

#[test]
fn a_clean_session_reports_no_relay_failure() {
    let dir = common::Private::new("rsk");
    let script = format!("{TRACE}\necho '{{\"status\":\"listening_ready\"}}'");
    let (code, lines) = relay(&dir, &script, None);
    assert_eq!(code, 0);
    assert!(matches!(
        lines.first(),
        Some(SessionLine::SessionStart { .. })
    ));
    let (error, events, sha, killed, stray) = end(&lines);
    assert_eq!((error, events, killed, stray), (None, 2, false, false));
    assert!(sha.is_some());
}

/// The first write (session_start) succeeds; later writes fail like a broken
/// SSH pipe, after a delay that lets the runtime exit first.
struct BrokenAfter {
    writes: usize,
}
impl std::io::Write for BrokenAfter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.writes += 1;
        if self.writes > 1 {
            std::thread::sleep(Duration::from_millis(300));
            return Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe));
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn an_early_relay_error_after_the_runtime_exited_still_stops_its_workers() {
    let dir = common::Private::new("rse");
    let pid_file = dir.join("worker.pid");
    // The runtime leaves a worker behind, prints a line and exits at once.
    let script = format!(
        "sleep 30 &\necho $! > {}\necho '{{\"status\":\"listening_ready\"}}'",
        pid_file.display()
    );
    let work = dir.join("work");
    let arguments: Vec<String> = [
        "--runtime",
        "/bin/sh",
        "--runtime-prefix-arg",
        "-c",
        "--runtime-prefix-arg",
        &script,
        "--runtime-prefix-arg",
        "fake-lamp-voice",
        "--work",
        &work.display().to_string(),
        "--seconds",
        "5",
        "--allowance-seconds",
        "5",
        "--fixture-reply",
        "/nonexistent/reply.wav",
        "--fixture-sha256",
        &"0".repeat(64),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let options = SessionOptions::parse(&arguments).unwrap();
    let (input, _runner) = UnixStream::pair().unwrap();
    // Only the session_start line is written; relaying the stdout line fails.
    let mut out = BrokenAfter { writes: 0 };
    assert!(run(&options, input, &mut out).is_err());
    assert!(
        process_gone(read_pid(&pid_file)),
        "a worker survived the relay's early return"
    );
}
