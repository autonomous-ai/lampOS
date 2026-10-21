//! The `lamp-session` relay's serve stop path against the fake serve runtime:
//! an explicit stop, a closed runner transport, the hard deadline and a
//! runtime that ignores SIGTERM. Each keeps its stop cause, signal, exit,
//! forced kill and confirmed reap separately. No hardware; no audio.
use crate::common;
#[path = "common/serve_relay_clock.rs"]
mod serve_relay_clock;

use lamp_voice_eval::physical::{
    protocol::{ServeEnd, SessionLine},
    session::{self, SessionOptions},
};
use std::{fs, time::Duration};

const BIN: &str = env!("CARGO_BIN_EXE_lamp-voice-eval");

fn options(dir: &common::Private, seconds: u16, fake: &[&str]) -> SessionOptions {
    options_with(dir, seconds, fake, &[])
}

/// `relay` holds extra `lamp-session` options, such as runtime policy flags.
fn options_with(
    dir: &common::Private,
    seconds: u16,
    fake: &[&str],
    relay: &[&str],
) -> SessionOptions {
    let plan = dir.join("plan.json");
    fs::write(&plan, lamp_voice_eval::plan::SERVE_PLAN).unwrap();
    let mut arguments: Vec<String> = vec!["--runtime".into(), BIN.into()];
    let plan = plan.display().to_string();
    let mut prefix = vec![
        "fake-lamp-voice",
        "--fake-plan",
        &plan,
        "--fake-scenario",
        "serve-conversation",
        "--fake-profile",
        "v2-serve-v1-reopen",
    ];
    prefix.extend(fake);
    for arg in prefix {
        arguments.extend(["--runtime-prefix-arg".into(), arg.into()]);
    }
    let work = dir.join("work").display().to_string();
    for arg in [
        "--work",
        &work,
        "--seconds",
        &seconds.to_string(),
        "--allowance-seconds",
        "5",
        "--serve-config",
        "/private/unused-fake-provider.json",
    ] {
        arguments.push(arg.into());
    }
    arguments.extend(relay.iter().map(|arg| arg.to_string()));
    SessionOptions::parse(&arguments).unwrap()
}

enum Runner {
    StopAfter(Duration),
    CloseAfter(Duration),
    Silent,
}

/// Run the relay in-process with a runner transport driven by `runner`.
fn relay(options: &SessionOptions, runner: Runner) -> (i32, Vec<SessionLine>) {
    let (code, out) = serve_relay_clock::run(options, runner).unwrap();
    let lines = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    (code, lines)
}

fn end(lines: &[SessionLine]) -> (&SessionLine, &ServeEnd) {
    let end = lines.last().unwrap();
    let SessionLine::SessionEnd {
        serve: Some(serve), ..
    } = end
    else {
        panic!("{end:?}")
    };
    (end, &**serve)
}

fn error(end: &SessionLine) -> String {
    let SessionLine::SessionEnd { error, .. } = end else {
        unreachable!()
    };
    error.clone().unwrap_or_default()
}

fn traces(lines: &[SessionLine]) -> usize {
    lines
        .iter()
        .filter(|line| matches!(line, SessionLine::Trace { .. }))
        .count()
}

#[test]
fn a_runner_stop_is_one_sigterm_and_a_clean_reaped_end() {
    let dir = common::Private::new("rly");
    let (code, lines) = relay(
        &options(&dir, 60, &[]),
        Runner::StopAfter(Duration::from_secs(3)),
    );
    let (end, serve) = end(&lines);
    assert_eq!(code, 0);
    assert_eq!(error(end), "", "{end:?}");
    assert!(matches!(
        end,
        SessionLine::SessionEnd {
            killed: false,
            exit_code: Some(0),
            stray_processes_stopped: false,
            ..
        }
    ));
    assert_eq!(serve.stop_cause, "runner_stop");
    assert_eq!(serve.stop_reason.as_deref(), Some("test"));
    assert!(!serve.forced_kill && serve.group_reaped);
    let signal = serve.signal_sent_us.unwrap();
    assert!(signal >= serve.stop_requested_us.unwrap());
    // The runtime records its end and finishes its metadata well inside the grace.
    let exit = serve.exited_us.unwrap() - signal;
    assert!(exit < session::SERVE_STOP_GRACE_US, "{exit} us");
    assert_eq!(serve.metadata.sequence_gaps, 0);
    assert_eq!(traces(&lines), serve.metadata.records);
    assert!(lines.iter().any(|line| matches!(line,
        SessionLine::Stdout { line, .. } if line.contains("service_metadata_finished"))));
}

#[test]
fn a_closed_runner_transport_stops_the_runtime_through_the_same_path() {
    let dir = common::Private::new("rly");
    let (code, lines) = relay(
        &options(&dir, 60, &[]),
        Runner::CloseAfter(Duration::from_secs(3)),
    );
    let (end, serve) = end(&lines);
    assert_eq!(code, 0);
    assert_eq!(serve.stop_cause, "transport_closed");
    assert!(serve.signal_sent_us.is_some() && !serve.forced_kill && serve.group_reaped);
    // Not a clean conversation end: the closure is retained as an error.
    assert!(error(end).contains("runner transport closed"), "{end:?}");
}

#[test]
fn the_hard_deadline_stops_the_runtime_and_is_an_error() {
    let dir = common::Private::new("rly");
    let (code, lines) = relay(&options(&dir, 1, &[]), Runner::Silent);
    let (end, serve) = end(&lines);
    assert_eq!(code, 0);
    assert_eq!(serve.stop_cause, "deadline");
    assert!(!serve.forced_kill && serve.group_reaped);
    assert!(error(end).contains("hard deadline"), "{end:?}");
}

#[test]
fn a_runtime_that_ignores_sigterm_is_force_killed_and_never_clean() {
    let dir = common::Private::new("rly");
    let (code, lines) = relay(
        &options(&dir, 60, &["--fake-ignore-sigterm", "yes"]),
        Runner::StopAfter(Duration::from_secs(3)),
    );
    let (end, serve) = end(&lines);
    assert_eq!(code, 3);
    assert!(matches!(
        end,
        SessionLine::SessionEnd {
            killed: true,
            exit_code: None,
            ..
        }
    ));
    assert_eq!(serve.stop_cause, "runner_stop");
    assert!(serve.signal_sent_us.is_some() && serve.forced_kill && serve.group_reaped);
    assert!(serve.exited_us.is_none());
    let error = error(end);
    assert!(error.contains("stop grace"), "{error}");
    // The killed runtime never acknowledged its metadata.
    assert!(error.contains("service_metadata_finished"), "{error}");
}

#[test]
fn the_pilot_runtime_settings_reach_the_serve_child_exactly() {
    let dir = common::Private::new("rly");
    let options = options_with(
        &dir,
        60,
        &[],
        &[
            "--noise-suppression",
            "off",
            "--turn-taking",
            "duplex",
            "--reference-clock",
            "capture",
            "--reference-recovery",
            "off",
            "--playout-start-ms",
            "200",
            "--provider-continuation",
            "kept-start",
            "--ring-channel-ceiling",
            "12",
        ],
    );
    let (code, lines) = relay(&options, Runner::StopAfter(Duration::from_secs(3)));
    assert_eq!(code, 0);
    let Some(SessionLine::SessionStart {
        mode, runtime_argv, ..
    }) = lines.first()
    else {
        panic!("{:?}", lines.first())
    };
    assert_eq!(mode, "serve");
    let work = dir.join("work");
    let serve = runtime_argv.iter().position(|a| a == "serve").unwrap();
    assert_eq!(
        runtime_argv[serve..],
        [
            "serve".to_owned(),
            "/private/unused-fake-provider.json".into(),
            work.join("runtime").display().to_string(),
            "--noise-suppression".into(),
            "off".into(),
            "--turn-taking".into(),
            "duplex".into(),
            "--reference-clock".into(),
            "capture".into(),
            "--reference-recovery".into(),
            "off".into(),
            "--playout-start-ms".into(),
            "200".into(),
            "--provider-continuation".into(),
            "kept-start".into(),
            "--cue-socket".into(),
            work.join("cue.sock").display().to_string(),
            "--ring-channel-ceiling".into(),
            "12".into(),
        ]
    );
    // The child received exactly that argv (after its program and command).
    let child: Vec<String> = lines
        .iter()
        .find_map(|line| match line {
            SessionLine::Stdout { line, .. } if line.contains("fake_lamp_argv") => {
                let value: serde_json::Value = serde_json::from_str(line).unwrap();
                Some(serde_json::from_value(value["argv"].clone()).unwrap())
            }
            _ => None,
        })
        .expect("the child reported its argv");
    assert_eq!(child, runtime_argv[2..]);
    let (end, serve) = end(&lines);
    assert_eq!(error(end), "", "{end:?}");
    assert!(!serve.forced_kill && serve.group_reaped);
}

#[test]
fn a_nonzero_runtime_exit_after_clean_metadata_is_a_relay_error() {
    let dir = common::Private::new("rly");
    let (code, lines) = relay(
        &options(&dir, 60, &["--fake-exit-code", "3"]),
        Runner::StopAfter(Duration::from_secs(3)),
    );
    let (end, serve) = end(&lines);
    assert_eq!(code, 0);
    assert!(matches!(
        end,
        SessionLine::SessionEnd {
            killed: false,
            exit_code: Some(3),
            ..
        }
    ));
    // The metadata itself is complete and acknowledged ...
    assert_eq!(serve.metadata.sequence_gaps, 0);
    assert!(serve.metadata.error.is_none() && !serve.forced_kill && serve.group_reaped);
    // ... but a nonzero exit never qualifies a serve session.
    assert!(error(end).contains("exited with status 3"), "{end:?}");
}
