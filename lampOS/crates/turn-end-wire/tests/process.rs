//! Process supervision with real child processes and no model.
//!
//! This binary has no libtest harness: with `LAMP_FAKE_ENDPOINT` set it is a
//! scripted worker that owns its stdout, otherwise it runs the tests below.
//! The first sample of each request scripts the worker's behavior.
use lamp_ipc::monotonic_us;
use lamp_turn_end_wire::{
    Availability, Classifier, Limits, Outcome, ProcessClassifier, Submit, Teardown, Token,
    WINDOW_SAMPLES, WorkerCommand,
    frame::{self, PROTOCOL, Response},
    serve,
};
use std::{
    io::{self, Write},
    panic,
    process::ExitCode,
    thread,
    time::{Duration, Instant},
};

const ANSWER: i16 = 0;
const CRASH: i16 = 1;
const HANG: i16 = 2;
const GARBAGE: i16 = 3;
const FAIL: i16 = 4;
const WRONG_REQUEST: i16 = 5;
/// Signals receipt through `LAMP_FAKE_RECEIVED`, then answers only after the
/// test creates `LAMP_FAKE_RELEASE`.
const HANDSHAKE: i16 = 6;
/// The identity every scripted worker reports in `Ready`.
const FAKE_MODEL: [u8; 32] = [7; 32];

fn fake_worker(mode: &str) -> ExitCode {
    let mut out = io::stdout().lock();
    match mode {
        "missing" => {
            let _ = serve::startup_failed(&mut out, "model /m.onnx: No such file or directory");
            return ExitCode::from(2);
        }
        "silent" => {
            thread::sleep(Duration::from_secs(60));
            return ExitCode::SUCCESS;
        }
        "crash-at-start" => return ExitCode::from(11),
        "old-protocol" => {
            let ready = Response::Ready {
                protocol: PROTOCOL + 1,
                load_us: 1,
                cold_us: 1,
                model_sha256: [0; 32],
            };
            let _ = out.write_all(&ready.encode()).and_then(|()| out.flush());
            thread::sleep(Duration::from_secs(60));
            return ExitCode::SUCCESS;
        }
        _ => {}
    }
    let _ = serve::ready(&mut out, 1_000, 2_000, [7; 32]);
    if mode == "twice-ready" {
        let _ = serve::ready(&mut out, 1_000, 2_000, [7; 32]);
    }
    let mut input = io::stdin().lock();
    let (mut body, mut pcm) = (Vec::new(), Vec::new());
    while let Ok(Some(request)) = frame::read_request(&mut input, &mut body, &mut pcm) {
        let received_at_us = monotonic_us();
        let arg = |index: usize| pcm.get(index).copied().unwrap_or(0);
        let response = match arg(0) {
            CRASH => std::process::exit(9),
            HANG => loop {
                thread::sleep(Duration::from_secs(3600));
            },
            GARBAGE => {
                // A complete frame of an unknown kind. A truncated frame would
                // instead leave the reader waiting until the hang deadline.
                let _ = out
                    .write_all(&[1, 0, 0, 0, 0xee])
                    .and_then(|()| out.flush());
                thread::sleep(Duration::from_secs(60));
                return ExitCode::SUCCESS;
            }
            HANDSHAKE => {
                let received = std::env::var_os("LAMP_FAKE_RECEIVED").unwrap();
                let release =
                    std::path::PathBuf::from(std::env::var_os("LAMP_FAKE_RELEASE").unwrap());
                std::fs::write(received, b"received").unwrap();
                let deadline = Instant::now() + Duration::from_secs(10);
                while !release.exists() && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(1));
                }
                Response::Verdict {
                    request: request.request,
                    probability: 0.92,
                    received_at_us,
                    features_us: 1,
                    inference_us: 2,
                    sent_at_us: monotonic_us(),
                }
            }
            FAIL => {
                let error = Response::Error {
                    request: request.request,
                    reason: "scripted failure".into(),
                };
                let _ = out.write_all(&error.encode()).and_then(|()| out.flush());
                return ExitCode::from(4);
            }
            command => {
                thread::sleep(Duration::from_millis(arg(2) as u64));
                Response::Verdict {
                    request: request.request + if command == WRONG_REQUEST { 100 } else { 0 },
                    probability: f32::from(arg(1)) / 1000.0,
                    received_at_us,
                    // The sample count, so tests can see the window bound.
                    features_us: pcm.len() as u64,
                    inference_us: 2,
                    sent_at_us: monotonic_us(),
                }
            }
        };
        if out
            .write_all(&response.encode())
            .and_then(|()| out.flush())
            .is_err()
        {
            return ExitCode::from(5);
        }
    }
    ExitCode::SUCCESS
}

fn quick() -> Limits {
    Limits {
        startup: Duration::from_secs(5),
        hang: Duration::from_millis(400),
        restarts: 2,
        backoff: Duration::from_millis(50),
        grace: Duration::from_millis(200),
    }
}

fn fake(mode: &str, limits: Limits) -> ProcessClassifier {
    fake_with(mode, limits, FAKE_MODEL, Vec::new())
}

fn fake_with(
    mode: &str,
    limits: Limits,
    model_sha256: [u8; 32],
    mut env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
) -> ProcessClassifier {
    env.push(("LAMP_FAKE_ENDPOINT".into(), mode.into()));
    ProcessClassifier::spawn(
        WorkerCommand {
            program: std::env::current_exe().unwrap(),
            args: Vec::new(),
            env,
            model_sha256,
        },
        limits,
    )
}

/// Caller-facing calls must never wait for the worker. Every scripted worker
/// delay these calls could wait for is at least 150 ms; the bound leaves room
/// for scheduling on a loaded test host.
fn quick_call<T>(call: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let value = call();
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_millis(100),
        "caller waited {elapsed:?}"
    );
    value
}

fn until<T>(limit: Duration, what: &str, mut probe: impl FnMut() -> Option<T>) -> T {
    let started = Instant::now();
    loop {
        if let Some(value) = probe() {
            return value;
        }
        assert!(started.elapsed() < limit, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(2));
    }
}

fn ready(classifier: &ProcessClassifier) {
    until(Duration::from_secs(5), "ready", || {
        (quick_call(|| classifier.availability()) == Availability::Ready).then_some(())
    });
}

fn answer(classifier: &mut ProcessClassifier) -> Outcome {
    until(Duration::from_secs(5), "an answer", || {
        quick_call(|| classifier.poll())
    })
}

fn script(command: i16, probability: i16, delay_ms: i16) -> Vec<i16> {
    let mut pcm = vec![0i16; 1_600];
    pcm[..3].copy_from_slice(&[command, probability, delay_ms]);
    pcm
}

fn token(utterance: u64) -> Token {
    Token {
        utterance,
        generation: 0,
        check: 0,
    }
}

/// Zombies still answer `kill -0`, so this also detects an unreaped child.
/// Asked of the kernel, not of a `kill` program: without procps that is
/// missing, and under qemu-user a missing one exits 127 and reads as
/// "gone" (#1497).
fn exists(pid: u32) -> bool {
    rustix::process::Pid::from_raw(pid as i32)
        .is_some_and(|pid| rustix::process::test_kill_process(pid).is_ok())
}

fn submit(classifier: &mut ProcessClassifier, token: Token, pcm: &[i16]) -> Submit {
    quick_call(|| classifier.submit(token, pcm))
}

fn verdict_carries_identity_and_one_request_at_a_time() {
    let mut classifier = fake("ready", quick());
    assert_eq!(classifier.availability(), Availability::Loading);
    ready(&classifier);
    let report = classifier.report();
    assert_eq!((report.worker, report.spawned, report.ready), (1, 1, 1));
    assert_eq!(report.load_us, Some(1_000));
    assert_eq!(report.model_sha256.as_deref(), Some(&"07".repeat(32)[..]));
    let first = token(4);
    assert_eq!(
        submit(&mut classifier, first, &script(ANSWER, 730, 50)),
        Submit::Accepted
    );
    assert_eq!(
        submit(&mut classifier, token(5), &script(ANSWER, 1, 0)),
        Submit::Busy
    );
    let outcome = answer(&mut classifier);
    assert_eq!(
        (outcome.token, outcome.worker, outcome.request),
        (first, 1, 1)
    );
    let verdict = outcome.result.unwrap();
    assert_eq!(verdict.probability, 0.73);
    assert!(verdict.round_trip_us >= 50_000, "{verdict:?}");
    assert!(verdict.delivery_us < verdict.round_trip_us, "{verdict:?}");
    // Back to back: the slot is free as soon as the answer arrived.
    let long = vec![0i16; 3 * WINDOW_SAMPLES];
    assert_eq!(submit(&mut classifier, token(6), &long), Submit::Accepted);
    let outcome = answer(&mut classifier);
    assert_eq!(outcome.result.unwrap().features_us, WINDOW_SAMPLES as u64);
    let pid = classifier.report().pid.unwrap();
    let monitor = classifier.monitor();
    drop(classifier);
    assert_eq!(
        monitor.wait_finished(Duration::from_secs(2)),
        Teardown::Reaped
    );
    let report = monitor.report();
    assert_eq!(report.native_reaped, Some(true));
    assert!(report.native_exit_after_us.is_some());
    assert!(report.reader_finished_after_us.is_some());
    assert_eq!(report.kill_requested_after_us, None);
    // The closed input ends the worker without a kill.
    until(Duration::from_secs(2), "worker exit", || {
        (!exists(pid)).then_some(())
    });
}

fn missing_model_is_final_and_never_restarted() {
    let mut classifier = fake("missing", quick());
    let reason = until(Duration::from_secs(5), "failure", || {
        match classifier.availability() {
            Availability::Failed(reason) => Some(reason),
            _ => None,
        }
    });
    assert!(reason.contains("No such file"), "{reason}");
    assert_eq!(
        submit(&mut classifier, token(1), &script(ANSWER, 1, 0)),
        Submit::Unavailable
    );
    assert_eq!(
        classifier.wait_finished(Duration::from_secs(2)),
        Teardown::Reaped
    );
    thread::sleep(quick().backoff * 3);
    let report = classifier.report();
    assert_eq!((report.spawned, report.ready, report.kills), (1, 0, 0));
    assert!(report.last_exit.unwrap().contains('2'));
}

fn missing_executable_fails_at_once() {
    let started = Instant::now();
    let mut classifier = ProcessClassifier::spawn(
        WorkerCommand::smart_turn(
            "/nonexistent/lamp-turn-end".into(),
            "/nonexistent/model.onnx".into(),
        ),
        quick(),
    );
    assert!(started.elapsed() < Duration::from_millis(20));
    let reason = until(Duration::from_secs(2), "failure", || {
        match classifier.availability() {
            Availability::Failed(reason) => Some(reason),
            _ => None,
        }
    });
    assert!(reason.contains("cannot start"), "{reason}");
    assert_eq!(
        submit(&mut classifier, token(1), &[0; 160]),
        Submit::Unavailable
    );
    assert_eq!(classifier.report().spawned, 0);
}

fn startup_failures_are_final() {
    for (mode, expected) in [
        ("silent", "not ready within"),
        ("crash-at-start", "closed its output"),
        ("old-protocol", "protocol"),
        ("twice-ready", "unexpected ready"),
    ] {
        // Only the silent worker needs the short bound: it is what times
        // out. The others end on an event (their exit, their `Ready`), and a
        // debug test binary can take over half a second to start on a Mac,
        // where a 300 ms bound failed every run as "not ready".
        let limits = Limits {
            startup: if mode == "silent" {
                Duration::from_millis(300)
            } else {
                quick().startup
            },
            ..quick()
        };
        let classifier = fake(mode, limits);
        let reason = until(Duration::from_secs(5), mode, || {
            match classifier.availability() {
                Availability::Failed(reason) => Some(reason),
                _ => None,
            }
        });
        // A double ready arrives after the first one made the worker usable,
        // so that worker counts as serving and may be replaced.
        if mode != "twice-ready" {
            assert!(reason.contains(expected), "{mode}: {reason}");
            assert_eq!(
                classifier.wait_finished(Duration::from_secs(2)),
                Teardown::Reaped,
                "{mode}"
            );
            assert_eq!(classifier.report().spawned, 1, "{mode}");
        } else {
            assert!(reason.contains(expected), "{mode}: {reason}");
        }
    }
}

/// The old worker is reaped before a replacement starts, and the request it
/// held is reported lost to its own token.
fn replaced(classifier: &mut ProcessClassifier, command: i16, expected: &str) {
    let old = classifier.report();
    let lost = token(old.worker * 10);
    assert_eq!(
        submit(classifier, lost, &script(command, 900, 0)),
        Submit::Accepted
    );
    let outcome = answer(classifier);
    assert_eq!(outcome.token, lost);
    assert_eq!(outcome.worker, old.worker);
    let error = outcome.result.unwrap_err();
    assert!(error.contains(expected), "{error}");
    assert!(matches!(
        quick_call(|| classifier.availability()),
        Availability::Recovering(_) | Availability::Loading | Availability::Ready
    ));
    assert_eq!(
        submit(classifier, token(99), &script(ANSWER, 1, 0)),
        Submit::Unavailable
    );
    until(Duration::from_secs(5), "replacement", || {
        (classifier.report().worker == old.worker + 1).then_some(())
    });
    // `exists` can say yes, or every "gone" here would pass unread.
    assert!(exists(std::process::id()), "this process reads as gone");
    assert!(!exists(old.pid.unwrap()), "old worker still exists");
    ready(classifier);
    let replacement = classifier.report();
    assert_eq!(replacement.native_reaped, None);
    assert_eq!(replacement.native_exit_after_us, None);
    assert_eq!(replacement.reader_finished_after_us, None);
    assert_eq!(replacement.kill_requested_after_us, None);
    assert_eq!(
        submit(classifier, token(7), &script(ANSWER, 250, 0)),
        Submit::Accepted
    );
    let outcome = answer(classifier);
    assert_eq!(outcome.worker, old.worker + 1);
    assert_eq!(outcome.result.unwrap().probability, 0.25);
}

fn serving_failures_are_replaced_within_budget() {
    let mut classifier = fake("ready", quick());
    ready(&classifier);
    replaced(&mut classifier, CRASH, "worker exited");
    replaced(&mut classifier, HANG, "worker exited");
    let report = classifier.report();
    assert_eq!((report.spawned, report.kills, report.lost), (3, 1, 2));
    assert!(
        report
            .last_failure
            .unwrap()
            .contains("no answer within 400 ms")
    );
    // The budget of two replacements is spent: the next failure is final.
    assert_eq!(
        submit(&mut classifier, token(8), &script(CRASH, 0, 0)),
        Submit::Accepted
    );
    assert!(answer(&mut classifier).result.is_err());
    let reason = until(
        Duration::from_secs(5),
        "final failure",
        || match classifier.availability() {
            Availability::Failed(reason) => Some(reason),
            _ => None,
        },
    );
    assert!(reason.contains("no replacement left"), "{reason}");
    assert_eq!(
        classifier.wait_finished(Duration::from_secs(2)),
        Teardown::Reaped
    );
    assert_eq!(classifier.report().spawned, 3);
}

fn protocol_violations_are_replaced() {
    for (command, expected) in [
        (GARBAGE, "worker exited"),
        (WRONG_REQUEST, "worker exited"),
        (FAIL, "scripted failure"),
    ] {
        let mut classifier = fake("ready", quick());
        ready(&classifier);
        replaced(&mut classifier, command, expected);
        let failure = classifier.report().last_failure.unwrap();
        let cause = match command {
            GARBAGE => "malformed answer",
            WRONG_REQUEST => "unknown request",
            _ => "scripted failure",
        };
        assert!(failure.contains(cause), "{failure}");
    }
}

fn cancelled_answers_are_never_delivered() {
    let mut classifier = fake("ready", quick());
    ready(&classifier);
    assert_eq!(
        submit(&mut classifier, token(1), &script(ANSWER, 800, 150)),
        Submit::Accepted
    );
    thread::sleep(Duration::from_millis(30));
    quick_call(|| classifier.cancel());
    // The worker still holds that request: the slot stays occupied.
    assert_eq!(
        submit(&mut classifier, token(2), &script(ANSWER, 1, 0)),
        Submit::Busy
    );
    until(Duration::from_secs(2), "discard", || {
        (classifier.report().discarded == 1).then_some(())
    });
    assert!(classifier.poll().is_none());
    assert_eq!(
        submit(&mut classifier, token(3), &script(ANSWER, 600, 0)),
        Submit::Accepted
    );
    assert_eq!(answer(&mut classifier).token, token(3));
}

fn close_during_inference_never_waits() {
    let mut classifier = fake("ready", quick());
    ready(&classifier);
    let pid = classifier.report().pid.unwrap();
    assert_eq!(
        submit(&mut classifier, token(1), &script(ANSWER, 500, 3_000)),
        Submit::Accepted
    );
    thread::sleep(Duration::from_millis(30));
    quick_call(|| classifier.close());
    assert_eq!(classifier.availability(), Availability::Closed);
    assert_eq!(
        submit(&mut classifier, token(2), &[0; 160]),
        Submit::Unavailable
    );
    // The busy worker cannot see its input close; it is killed after grace.
    assert_eq!(
        classifier.wait_finished(Duration::from_secs(2)),
        Teardown::Reaped
    );
    assert!(!exists(pid));
    let report = classifier.report();
    assert_eq!(report.kills, 1);
    assert_eq!(report.native_reaped, Some(true));
    assert!(report.native_exit_after_us.unwrap() >= report.kill_requested_after_us.unwrap());
    assert!(report.reader_finished_after_us.is_some());
    assert!(classifier.poll().is_none());
}

fn sustained_requests_stay_bounded() {
    let mut classifier = fake("ready", quick());
    ready(&classifier);
    let mut window = script(ANSWER, 500, 0);
    window.resize(WINDOW_SAMPLES, 1);
    let mut worst = Duration::ZERO;
    for index in 0..300u64 {
        let started = Instant::now();
        let submitted = classifier.submit(token(index), &window);
        assert_eq!(
            submitted,
            Submit::Accepted,
            "{index} {:?} {:?}",
            classifier.availability(),
            classifier.report()
        );
        worst = worst.max(started.elapsed());
        let outcome = answer(&mut classifier);
        assert_eq!(outcome.token, token(index));
    }
    let report = classifier.report();
    assert_eq!(
        (report.requests, report.verdicts, report.spawned),
        (300, 300, 1)
    );
    // An 8 s window is copied on the caller's thread. Unoptimized and on a
    // loaded host this is a few milliseconds; the bound catches a pathology.
    assert!(worst < Duration::from_millis(200), "{worst:?}");
    println!("    full-window submit worst case {worst:?}");
}

/// `LAMP_ENDPOINT_IDLE_SECS` (default 2) of quiet, several times the hang
/// limit: only a request in flight is timed, so an idle worker is neither
/// killed nor replaced. The pilot evidence uses 600.
fn an_idle_worker_stays_ready_across_a_long_quiet() {
    let idle = std::env::var("LAMP_ENDPOINT_IDLE_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(2u64);
    let mut classifier = fake("ready", quick());
    ready(&classifier);
    let pid = classifier.report().pid.unwrap();
    for (index, quiet) in [(1, false), (2, true)] {
        if quiet {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(idle) {
                assert_eq!(
                    quick_call(|| classifier.availability()),
                    Availability::Ready
                );
                thread::sleep(Duration::from_millis(100));
            }
        }
        assert_eq!(
            submit(&mut classifier, token(index), &script(ANSWER, 800, 0)),
            Submit::Accepted
        );
        let outcome = answer(&mut classifier);
        assert_eq!(outcome.token, token(index));
        assert_eq!(outcome.result.unwrap().probability, 0.8);
    }
    let report = classifier.report();
    assert_eq!(report.pid, Some(pid));
    assert_eq!(
        (report.spawned, report.kills, report.verdicts, report.errors),
        (1, 0, 2, 0)
    );
    classifier.close();
    assert_eq!(
        classifier.wait_finished(Duration::from_secs(2)),
        Teardown::Reaped
    );
    println!("    idle {idle} s without a restart");
}

fn a_worker_reporting_another_model_is_refused_before_any_request() {
    let mut classifier = fake_with("ready", quick(), [9; 32], Vec::new());
    let reason = until(Duration::from_secs(5), "failure", || {
        match quick_call(|| classifier.availability()) {
            Availability::Failed(reason) => Some(reason),
            Availability::Ready => panic!("ready with the wrong model"),
            _ => None,
        }
    });
    assert!(
        reason.contains(&format!("worker model {}", "07".repeat(32))),
        "{reason}"
    );
    assert!(
        reason.contains(&format!("expected {}", "09".repeat(32))),
        "{reason}"
    );
    assert_eq!(
        submit(&mut classifier, token(1), &script(ANSWER, 900, 0)),
        Submit::Unavailable
    );
    assert_eq!(
        classifier.wait_finished(Duration::from_secs(2)),
        Teardown::Reaped
    );
    let report = classifier.report();
    assert_eq!((report.spawned, report.ready, report.requests), (1, 0, 0));
    assert_eq!(report.model_sha256, None);
}

fn no_outcome_is_delivered_after_close_even_for_an_answer_on_its_way() {
    let directory = std::env::temp_dir().join(format!(
        "lamp-turn-end-handshake-{}-{}",
        std::process::id(),
        monotonic_us()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let (received, release) = (directory.join("received"), directory.join("release"));
    let mut classifier = fake_with(
        "ready",
        quick(),
        FAKE_MODEL,
        vec![
            ("LAMP_FAKE_RECEIVED".into(), received.clone().into()),
            ("LAMP_FAKE_RELEASE".into(), release.clone().into()),
        ],
    );
    ready(&classifier);
    let sent = token(92);
    assert_eq!(
        submit(&mut classifier, sent, &script(HANDSHAKE, 0, 0)),
        Submit::Accepted
    );
    // The worker holds the request before the client closes.
    until(Duration::from_secs(5), "receipt", || {
        received.exists().then_some(())
    });
    quick_call(|| classifier.close());
    std::fs::write(&release, b"release").unwrap();
    assert_eq!(
        classifier.wait_finished(Duration::from_secs(5)),
        Teardown::Reaped
    );
    assert!(classifier.poll().is_none(), "an outcome after close");
    assert_eq!(classifier.availability(), Availability::Closed);
    let report = classifier.report();
    assert_eq!(report.requests, 1);
    // The answer either arrived and was discarded, or died with the worker.
    assert_eq!(report.discarded + report.lost, 1, "{report:?}");
    assert_eq!(report.verdicts, report.discarded);
    std::fs::remove_dir_all(directory).unwrap();
}

fn main() -> ExitCode {
    if let Some(mode) = std::env::var_os("LAMP_FAKE_ENDPOINT") {
        return fake_worker(mode.to_str().unwrap_or_default());
    }
    let filters: Vec<String> = std::env::args()
        .skip(1)
        .filter(|argument| !argument.starts_with('-'))
        .collect();
    let tests: [(&str, fn()); 12] = [
        (
            "an_idle_worker_stays_ready_across_a_long_quiet",
            an_idle_worker_stays_ready_across_a_long_quiet,
        ),
        (
            "a_worker_reporting_another_model_is_refused_before_any_request",
            a_worker_reporting_another_model_is_refused_before_any_request,
        ),
        (
            "no_outcome_is_delivered_after_close_even_for_an_answer_on_its_way",
            no_outcome_is_delivered_after_close_even_for_an_answer_on_its_way,
        ),
        (
            "verdict_carries_identity_and_one_request_at_a_time",
            verdict_carries_identity_and_one_request_at_a_time,
        ),
        (
            "missing_model_is_final_and_never_restarted",
            missing_model_is_final_and_never_restarted,
        ),
        (
            "missing_executable_fails_at_once",
            missing_executable_fails_at_once,
        ),
        ("startup_failures_are_final", startup_failures_are_final),
        (
            "serving_failures_are_replaced_within_budget",
            serving_failures_are_replaced_within_budget,
        ),
        (
            "protocol_violations_are_replaced",
            protocol_violations_are_replaced,
        ),
        (
            "cancelled_answers_are_never_delivered",
            cancelled_answers_are_never_delivered,
        ),
        (
            "close_during_inference_never_waits",
            close_during_inference_never_waits,
        ),
        (
            "sustained_requests_stay_bounded",
            sustained_requests_stay_bounded,
        ),
    ];
    let (mut passed, mut failed) = (0, 0);
    for (name, test) in tests {
        if !filters.is_empty() && !filters.iter().any(|filter| name.contains(filter)) {
            continue;
        }
        let started = Instant::now();
        let ok = panic::catch_unwind(test).is_ok();
        println!(
            "test {name} ... {} ({} ms)",
            if ok { "ok" } else { "FAILED" },
            started.elapsed().as_millis()
        );
        if ok {
            passed += 1;
        } else {
            failed += 1;
        }
    }
    println!("test result: {passed} passed; {failed} failed");
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
