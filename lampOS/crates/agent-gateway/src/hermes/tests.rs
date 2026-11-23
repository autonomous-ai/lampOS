//! The session against the fake API server ([`super::fake`]).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;

use super::fake::{self, Fake};
use super::{ApprovalRequest, Config, Decision, Kept, Session, fence_policy};
use crate::{AgentSession, DROPPED, Error, Outcome, Phase, RunId, Turn, TurnKind};

const SECOND: Duration = Duration::from_secs(1);

fn quiet() -> super::Logger {
    Arc::new(|_: &str| {})
}

fn config(f: &Fake) -> Config {
    Config {
        url: f.url(),
        key_path: f.key_path(),
        state_path: Some(f.state_path()),
        call_timeout: 2 * SECOND,
        stream_idle: 2 * SECOND,
        settle: 2 * SECOND,
        poll: Duration::from_millis(50),
        recheck: Duration::from_millis(200),
        ..Config::default()
    }
}

fn session(f: &Fake) -> Session {
    Session::new(config(f)).with_logger(quiet())
}

fn ready(f: &Fake) -> Session {
    let mut s = session(f);
    s.ready(5 * SECOND).expect("ready");
    s
}

fn turn(task: &str, message: &str) -> Turn {
    Turn {
        task_id: task.to_owned(),
        message: message.to_owned(),
        kind: TurnKind::Spoken,
    }
}

fn eventually(within: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    cond()
}

fn answered(o: Outcome) -> (String, crate::Progress) {
    match o {
        Outcome::Answered { text, progress } => (text, progress),
        other => panic!("not answered: {other:?}"),
    }
}

fn kept(f: &Fake) -> Kept {
    serde_json::from_slice(&std::fs::read(f.state_path()).expect("state file")).expect("json")
}

/// The hand-overs (`POST /v1/runs` itself, not a run's `/stop`).
fn runs_posted(f: &Fake) -> Vec<fake::Rec> {
    f.reqs("POST", "/v1/runs")
        .into_iter()
        .filter(|r| r.path == "/v1/runs")
        .collect()
}

/// Waits until the run's stream is open (its `GET .../events` seen).
fn following(f: &Fake, run: &RunId, n: usize) {
    f.wait_req("GET", &format!("/v1/runs/{}/events", run.0), n, 5 * SECOND);
}

// ---- ready ---------------------------------------------------------------

#[test]
fn ready_checks_health_capabilities_and_makes_the_kept_session() {
    let f = Fake::start();
    let s = ready(&f);
    assert_eq!(s.server_version(), Some("0.22.2"));
    let id = s.session_id().to_owned();
    assert!(id.starts_with("lamp-main-"), "{id}");
    assert!(f.has_session(&id));
    let caps = &f.reqs("GET", "/v1/capabilities")[0];
    assert_eq!(
        caps.header("authorization"),
        Some(format!("Bearer {}", fake::KEY).as_str())
    );
    // /health takes no key and is never sent one.
    assert_eq!(f.reqs("GET", "/health")[0].header("authorization"), None);
    assert_eq!(f.reqs("POST", "/api/sessions")[0].body["id"], json!(id));
    let k = kept(&f);
    assert_eq!((k.session_id.as_str(), k.turns), (id.as_str(), 0));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(f.state_path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn a_restarted_lamp_tasks_adopts_the_kept_session() {
    let f = Fake::start();
    let id = ready(&f).session_id().to_owned();
    let s = ready(&f);
    assert_eq!(s.session_id(), id);
    // Found there: not made again.
    assert_eq!(f.reqs("POST", "/api/sessions").len(), 1);
}

#[test]
fn a_kept_session_gone_from_the_server_is_replaced() {
    let f = Fake::start();
    std::fs::write(
        f.state_path(),
        serde_json::to_vec(&Kept {
            session_id: "lamp-main-1".to_owned(),
            created: 1,
            turns: 7,
            tokens: 99,
        })
        .unwrap(),
    )
    .unwrap();
    let s = ready(&f);
    assert_ne!(s.session_id(), "lamp-main-1");
    assert_eq!((s.turns(), s.run_tokens()), (0, 0));
    assert!(f.has_session(s.session_id()));
    assert_eq!(kept(&f).session_id, s.session_id());
}

#[test]
fn a_bad_key_is_refused_at_once() {
    let f = Fake::start();
    std::fs::write(f.key_path(), "not-the-key").unwrap();
    let mut s = session(&f);
    let started = Instant::now();
    match s.ready(5 * SECOND) {
        Err(Error::Refused(why)) => {
            assert!(why.contains("gateway_auth_failed"), "{why}");
            assert!(!why.contains("not-the-key"), "{why}");
        }
        other => panic!("{other:?}"),
    }
    assert!(started.elapsed() < 2 * SECOND);
}

#[test]
fn no_key_or_no_health_is_not_ready() {
    let f = Fake::start();
    std::fs::remove_file(f.key_path()).unwrap();
    let mut s = session(&f);
    assert!(
        matches!(s.ready(Duration::from_millis(300)), Err(Error::NotReady(w)) if w.contains("no API key"))
    );
    let f = Fake::start();
    f.knobs(|k| k.health_down = true);
    let mut s = session(&f);
    assert!(matches!(
        s.ready(Duration::from_millis(300)),
        Err(Error::NotReady(_))
    ));
    f.knobs(|k| k.health_down = false);
    s.ready(5 * SECOND).expect("ready once healthy");
}

#[test]
fn a_server_without_the_runs_api_is_refused() {
    let f = Fake::start();
    f.knobs(|k| k.missing_feature = Some("run_events_sse"));
    let mut s = session(&f);
    assert!(matches!(s.ready(5 * SECOND), Err(Error::Refused(w)) if w.contains("run_events_sse")));
}

#[test]
fn a_dead_server_is_not_ready_and_backs_off() {
    let f = Fake::start();
    f.kill();
    let mut s = session(&f);
    assert!(matches!(
        s.ready(Duration::from_millis(200)),
        Err(Error::NotReady(_))
    ));
    f.revive();
    // The 1 s backoff is waited out within a longer bound.
    s.ready(5 * SECOND).expect("ready after the revive");
}

// ---- hand-over, progress, done -------------------------------------------

#[test]
fn a_turn_is_handed_over_into_the_kept_session_with_a_key() {
    let f = Fake::start();
    let mut s = ready(&f);
    let run = s.send(&turn("t1", "New task t1: hello")).expect("send");
    let post = &runs_posted(&f)[0];
    assert_eq!(post.body["input"], "New task t1: hello");
    assert_eq!(post.body["session_id"], json!(s.session_id()));
    let key = post.header("idempotency-key").unwrap();
    assert!(key.starts_with("lamp-t1-"), "{key}");
    assert_eq!(run.0, f.run_ids()[0]);
    assert_eq!(s.progress(&run).unwrap().phase, Phase::Queued);
    assert_eq!(s.running().len(), 1);
    assert_eq!((s.turns(), kept(&f).turns), (1, 1));
}

#[test]
fn events_become_progress_and_the_answer() {
    let f = Fake::start();
    let mut s = ready(&f);
    let run = s.send(&turn("t1", "look")).unwrap();
    following(&f, &run, 1);
    f.push(&run.0, "reasoning.available", json!({"text": "thinking"}));
    f.push(
        &run.0,
        "tool.started",
        json!({"tool": "terminal", "preview": "ls -la"}),
    );
    assert!(eventually(3 * SECOND, || s.progress(&run).unwrap().phase
        == Phase::Tool));
    let step = s.progress(&run).unwrap().last_tool.unwrap();
    assert_eq!((step.name.as_str(), step.running), ("terminal", true));
    assert!(step.brief.contains("ls -la"));
    f.push(
        &run.0,
        "tool.completed",
        json!({"tool": "terminal", "duration": 0.2, "error": false}),
    );
    f.push(&run.0, "message.delta", json!({"delta": "It is "}));
    f.push(&run.0, "message.delta", json!({"delta": "done."}));
    assert!(eventually(3 * SECOND, || {
        s.progress(&run).unwrap().last_text.as_deref() == Some("It is done.")
    }));
    assert!(matches!(
        s.wait(&run, Duration::from_millis(50)),
        Err(Error::TimedOut)
    ));
    f.push(
        &run.0,
        "run.completed",
        json!({"output": "It is done.", "usage": {"input_tokens": 1200, "output_tokens": 30}}),
    );
    let (text, p) = answered(s.wait(&run, 5 * SECOND).unwrap());
    assert_eq!(text, "It is done.");
    assert_eq!((p.phase, p.tools_done), (Phase::Done, 1));
    assert!(s.running().is_empty());
    assert_eq!((s.run_tokens(), kept(&f).tokens), (1200, 1200));
}

/// V1's 250k backstop compares one turn's usage: each ended run's
/// `usage.input_tokens` replaces the last, never summed across turns (a sum
/// rotated the kept session after one web search, ee17 24 Nov 02:55).
#[test]
fn the_kept_tokens_are_the_last_turns_never_a_sum() {
    let f = Fake::start();
    let mut s = ready(&f);
    for (task, input) in [("a", 200_000u64), ("b", 300)] {
        let run = s.send(&turn(task, "x")).unwrap();
        following(&f, &run, 1);
        f.push(
            &run.0,
            "run.completed",
            json!({"output": "ok", "usage": {"input_tokens": input, "output_tokens": 5}}),
        );
        answered(s.wait(&run, 5 * SECOND).unwrap());
    }
    assert_eq!((s.run_tokens(), kept(&f).tokens), (300, 300));
    assert_eq!(s.turns(), 2);
}

#[test]
fn failed_and_cancelled_runs_end_as_such() {
    let f = Fake::start();
    let mut s = ready(&f);
    let a = s.send(&turn("a", "x")).unwrap();
    following(&f, &a, 1);
    f.push(&a.0, "run.failed", json!({"error": "provider 400"}));
    assert!(
        matches!(s.wait(&a, 5 * SECOND).unwrap(), Outcome::Failed { why, .. } if why == "provider 400")
    );
    let b = s.send(&turn("b", "y")).unwrap();
    following(&f, &b, 1);
    f.push(&b.0, "run.cancelled", json!({}));
    assert!(matches!(
        s.wait(&b, 5 * SECOND).unwrap(),
        Outcome::Aborted { .. }
    ));
    assert!(matches!(
        s.wait(&RunId("nope".to_owned()), SECOND),
        Err(Error::UnknownRun(_))
    ));
}

#[test]
fn a_broken_stream_is_reopened_from_the_last_seq() {
    let f = Fake::start();
    let mut s = ready(&f);
    let run = s.send(&turn("t1", "x")).unwrap();
    following(&f, &run, 1);
    f.push(&run.0, "tool.started", json!({"tool": "a", "preview": ""}));
    f.push(&run.0, "tool.completed", json!({"tool": "a"}));
    assert!(eventually(3 * SECOND, || s
        .progress(&run)
        .unwrap()
        .tools_done
        == 1));
    f.break_streams();
    let again = f.wait_req("GET", &format!("/v1/runs/{}/events", run.0), 2, 5 * SECOND);
    assert_eq!(again.header("last-event-id"), Some("1"));
    // The status was read between: the run goes on.
    assert!(
        !f.reqs("GET", &format!("/v1/runs/{}", run.0))
            .iter()
            .all(|r| r.path.ends_with("/events"))
    );
    f.push(&run.0, "run.completed", json!({"output": "ok"}));
    let (text, p) = answered(s.wait(&run, 5 * SECOND).unwrap());
    assert_eq!((text.as_str(), p.tools_done), ("ok", 1));
}

#[test]
fn a_stream_lost_before_its_end_is_settled_from_the_status() {
    let f = Fake::start();
    let mut s = ready(&f);
    let run = s.send(&turn("t1", "x")).unwrap();
    following(&f, &run, 1);
    f.settle_quietly(&run.0, "completed", json!({"output": "from the status"}));
    f.break_streams();
    let (text, _) = answered(s.wait(&run, 5 * SECOND).unwrap());
    assert_eq!(text, "from the status");
}

#[test]
fn a_silent_stream_is_taken_as_broken() {
    let f = Fake::start();
    let mut s = Session::new(Config {
        stream_idle: Duration::from_millis(100),
        ..config(&f)
    })
    .with_logger(quiet());
    s.ready(5 * SECOND).unwrap();
    // The fake writes a keepalive every 200 ms: longer than the bound.
    let run = s.send(&turn("t1", "x")).unwrap();
    f.wait_req("GET", &format!("/v1/runs/{}/events", run.0), 2, 5 * SECOND);
    f.push(&run.0, "run.completed", json!({"output": "ok"}));
    assert!(matches!(
        s.wait(&run, 5 * SECOND).unwrap(),
        Outcome::Answered { .. }
    ));
}

// ---- drops ----------------------------------------------------------------

#[test]
fn the_server_gone_mid_run_ends_it_at_once_with_the_drop_words() {
    let f = Fake::start();
    let mut s = ready(&f);
    let run = s.send(&turn("t1", "New task t1: long")).unwrap();
    following(&f, &run, 1);
    f.push(&run.0, "tool.started", json!({"tool": "a", "preview": ""}));
    let started = Instant::now();
    f.kill();
    match s.wait(&run, 5 * SECOND).unwrap() {
        Outcome::Failed { why, .. } => assert_eq!(why, DROPPED),
        other => panic!("{other:?}"),
    }
    assert!(
        started.elapsed() < 2 * SECOND,
        "at once, not after a settle"
    );
    // The same turn again: the ended run back, nothing posted.
    f.revive();
    let again = s.send(&turn("t1", "New task t1: long")).unwrap();
    assert_eq!(again, run);
    assert_eq!(runs_posted(&f).len(), 1);
    // A new task works at once, with its own key.
    let next = s.send(&turn("t2", "New task t2: hi")).expect("next task");
    let keys: Vec<String> = runs_posted(&f)
        .iter()
        .map(|r| r.header("idempotency-key").unwrap().to_owned())
        .collect();
    assert_eq!(keys.len(), 2);
    assert_ne!(keys[0], keys[1]);
    following(&f, &next, 1);
    f.push(&next.0, "run.completed", json!({"output": "hi"}));
    assert!(matches!(
        s.wait(&next, 5 * SECOND).unwrap(),
        Outcome::Answered { .. }
    ));
}

#[test]
fn a_restart_that_interrupted_the_run_ends_it_with_the_drop_words() {
    let f = Fake::start();
    let mut s = ready(&f);
    let run = s.send(&turn("t1", "x")).unwrap();
    following(&f, &run, 1);
    f.restart();
    match s.wait(&run, 5 * SECOND).unwrap() {
        Outcome::Failed { why, .. } => assert_eq!(why, DROPPED),
        other => panic!("{other:?}"),
    }
}

#[test]
fn run_interrupted_by_a_shutdown_ends_with_the_drop_words() {
    let f = Fake::start();
    let mut s = ready(&f);
    let run = s.send(&turn("t1", "x")).unwrap();
    following(&f, &run, 1);
    f.push(
        &run.0,
        "run.interrupted",
        json!({"error": "Gateway shutdown interrupted the run."}),
    );
    assert!(
        matches!(s.wait(&run, 5 * SECOND).unwrap(), Outcome::Failed { why, .. } if why == DROPPED)
    );
}

#[test]
fn an_unconfirmed_hand_over_is_sent_again_with_its_key_and_replayed() {
    let f = Fake::start();
    let mut s = ready(&f);
    f.knobs(|k| k.runs_delay = 3 * SECOND);
    assert!(matches!(s.send(&turn("t1", "x")), Err(Error::Lost(_))));
    f.knobs(|k| k.runs_delay = Duration::ZERO);
    let run = s.send(&turn("t1", "x")).expect("replayed");
    let posts = runs_posted(&f);
    assert_eq!(posts.len(), 2);
    assert_eq!(
        posts[0].header("idempotency-key"),
        posts[1].header("idempotency-key")
    );
    assert_eq!(f.run_ids(), vec![run.0.clone()]);
    // A replay is not a second turn.
    assert_eq!(s.turns(), 0);
}

#[test]
fn refusals_and_a_busy_server_answer_as_such() {
    let f = Fake::start();
    let mut s = ready(&f);
    f.knobs(|k| k.runs_status = Some(429));
    assert!(matches!(s.send(&turn("t1", "x")), Err(Error::NotReady(_))));
    f.knobs(|k| k.runs_status = Some(400));
    assert!(matches!(s.send(&turn("t1", "x")), Err(Error::Refused(_))));
    f.knobs(|k| k.runs_status = None);
    std::fs::write(f.key_path(), "rotated").unwrap();
    f.knobs(|k| k.runs_status = Some(401));
    assert!(matches!(s.send(&turn("t1", "x")), Err(Error::Refused(_))));
}

// ---- approvals ------------------------------------------------------------

#[test]
fn approvals_are_answered_at_once_by_the_fence() {
    let f = Fake::start();
    let mut s = Session::new(Config {
        fence: "/usr/local/lib/lampos/skill-start".into(),
        ..config(&f)
    })
    .with_logger(quiet());
    s.ready(5 * SECOND).unwrap();
    let run = s.send(&turn("t1", "x")).unwrap();
    following(&f, &run, 1);
    let ask = |rid: &str, cmd: &str| {
        json!({"command": cmd, "pattern_key": "k", "pattern_keys": ["k"], "description": "d",
               "allow_permanent": true, "allow_session": true, "request_id": rid,
               "choices": ["once", "session", "always", "deny"]})
    };
    f.push(
        &run.0,
        "approval.request",
        ask("r1", "/usr/local/lib/lampos/skill-start run-42"),
    );
    f.push(&run.0, "approval.request", ask("r2", "rm -rf /"));
    f.push(
        &run.0,
        "approval.request",
        ask("r3", "/usr/local/lib/lampos/skill-start x; rm -rf /"),
    );
    f.push(&run.0, "approval.request", ask("r4", "skill-start run-43"));
    let path = format!("/v1/runs/{}/approval", run.0);
    let got: Vec<(String, String)> = (1..=4)
        .map(|n| {
            let r = f.wait_req("POST", &path, n, 5 * SECOND);
            (
                r.body["request_id"].as_str().unwrap().to_owned(),
                r.body["choice"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        got,
        vec![
            ("r1".to_owned(), "once".to_owned()),
            ("r2".to_owned(), "deny".to_owned()),
            ("r3".to_owned(), "deny".to_owned()),
            ("r4".to_owned(), "once".to_owned()),
        ]
    );
}

#[test]
fn the_fence_policy_allows_only_the_launcher_with_plain_words() {
    let p = fence_policy("/usr/local/lib/lampos/skill-start".into());
    let ask = |cmd: &str, choices: &[&str]| ApprovalRequest {
        request_id: "r".to_owned(),
        command: cmd.to_owned(),
        description: String::new(),
        pattern_keys: Vec::new(),
        choices: choices.iter().map(|c| (*c).to_owned()).collect(),
        raw: json!({}),
    };
    let launcher = "/usr/local/lib/lampos/skill-start abc-1";
    assert_eq!(p(&ask(launcher, &["once", "deny"])), Decision::Once);
    assert_eq!(p(&ask(launcher, &[])), Decision::Once);
    assert_eq!(p(&ask(launcher, &["deny"])), Decision::Deny);
    assert_eq!(
        p(&ask("skill-start abc-1", &["once", "deny"])),
        Decision::Once
    );
    for bad in [
        "skill-start",
        "skill-starter abc",
        "/tmp/skill-start abc",
        "/usr/local/lib/lampos/skill-start $(id)",
        "/usr/local/lib/lampos/skill-start a && b",
        "/usr/local/lib/lampos/skill-start 'a b'",
        "/usr/local/lib/lampos/skill-starter a",
        "sudo /usr/local/lib/lampos/skill-start a",
    ] {
        assert_eq!(p(&ask(bad, &["once", "deny"])), Decision::Deny, "{bad}");
    }
    assert_eq!(Decision::Once.word(), "once");
    assert_eq!(Decision::Deny.word(), "deny");
}

// ---- abort, reset, size -----------------------------------------------------

#[test]
fn abort_stops_the_run() {
    let f = Fake::start();
    let mut s = ready(&f);
    let run = s.send(&turn("t1", "x")).unwrap();
    following(&f, &run, 1);
    s.abort(&run).expect("abort");
    assert_eq!(f.reqs("POST", &format!("/v1/runs/{}/stop", run.0)).len(), 1);
    assert!(matches!(
        s.wait(&run, SECOND).unwrap(),
        Outcome::Aborted { .. }
    ));
    // Again: nothing to do.
    s.abort(&run).expect("abort again");
    assert_eq!(f.reqs("POST", &format!("/v1/runs/{}/stop", run.0)).len(), 1);
}

#[test]
fn a_stuck_run_is_aborted_and_the_next_task_runs() {
    let f = Fake::start();
    let mut s = ready(&f);
    let run = s.send(&turn("t1", "New task t1: hang")).unwrap();
    following(&f, &run, 1);
    f.push(
        &run.0,
        "tool.started",
        json!({"tool": "terminal", "preview": "sleep 999"}),
    );
    assert!(eventually(3 * SECOND, || s.progress(&run).unwrap().phase
        == Phase::Tool));
    // Keepalives are no events: the run's clock stands still, which is what
    // lamp-tasks' no-event stuck rule reads.
    let at = s.progress(&run).unwrap().updated_at;
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(s.progress(&run).unwrap().updated_at, at);
    s.abort(&run).expect("abort");
    f.wait_req("POST", &format!("/v1/runs/{}/stop", run.0), 1, SECOND);
    assert!(matches!(
        s.wait(&run, SECOND).unwrap(),
        Outcome::Aborted { .. }
    ));
    assert!(s.running().is_empty());
    let next = s
        .send(&turn("t2", "New task t2: hi"))
        .expect("the next task");
    following(&f, &next, 1);
    f.push(&next.0, "message.delta", json!({"delta": "hi"}));
    f.push(&next.0, "run.completed", json!({"output": "hi"}));
    let (text, _) = answered(s.wait(&next, 5 * SECOND).unwrap());
    assert_eq!(text, "hi");
    // The same kept session throughout.
    let posts = runs_posted(&f);
    assert_eq!(posts[0].body["session_id"], posts[1].body["session_id"]);
}

#[test]
fn abort_ends_the_run_here_when_its_stop_never_lands() {
    let f = Fake::start();
    let mut s = Session::new(Config {
        call_timeout: Duration::from_millis(500),
        ..config(&f)
    })
    .with_logger(quiet());
    s.ready(5 * SECOND).unwrap();
    let run = s.send(&turn("t1", "x")).unwrap();
    following(&f, &run, 1);
    f.knobs(|k| k.stop_ignored = true);
    s.abort(&run).expect("abort");
    assert!(matches!(
        s.wait(&run, SECOND).unwrap(),
        Outcome::Aborted { .. }
    ));
}

#[test]
fn reset_keeps_a_fresh_session_and_deletes_the_old() {
    let f = Fake::start();
    let mut s = ready(&f);
    let old = s.session_id().to_owned();
    let run = s.send(&turn("t1", "x")).unwrap();
    following(&f, &run, 1);
    std::thread::sleep(Duration::from_millis(5));
    s.reset().expect("reset");
    let new = s.session_id().to_owned();
    assert_ne!(new, old);
    assert!(f.has_session(&new) && !f.has_session(&old));
    assert_eq!(f.reqs("DELETE", &format!("/api/sessions/{old}")).len(), 1);
    assert!(matches!(
        s.wait(&run, SECOND).unwrap(),
        Outcome::Aborted { .. }
    ));
    let k = kept(&f);
    assert_eq!((k.session_id.as_str(), k.turns), (new.as_str(), 0));
    // The next turn goes into the new one.
    s.send(&turn("t2", "y")).unwrap();
    assert_eq!(runs_posted(&f)[1].body["session_id"], json!(new));
}

#[test]
fn size_is_the_live_contexts_messages_and_bytes() {
    let f = Fake::start();
    let mut s = ready(&f);
    let id = s.session_id().to_owned();
    f.set_messages(
        &id,
        vec![
            json!({"role": "user", "content": "a".repeat(400)}),
            json!({"role": "assistant", "content": null, "tool_calls": [{"id": "c"}]}),
            json!({"role": "tool", "content": "b".repeat(396)}),
        ],
    );
    let (tokens, messages) = s.size_now().expect("a size");
    assert_eq!(messages, 3);
    let calls = json!([{"id": "c"}]).to_string().len() as u64;
    assert_eq!(tokens, (796 + calls).div_ceil(4));
    // Read once until a run ends.
    s.size_now();
    assert_eq!(
        f.reqs("GET", &format!("/api/sessions/{id}/messages")).len(),
        1
    );
}

#[test]
fn logs_never_carry_the_key() {
    let f = Fake::start();
    let lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = lines.clone();
    let mut s = Session::new(config(&f)).with_logger(Arc::new(move |l: &str| {
        sink.lock().unwrap().push(l.to_owned())
    }));
    s.ready(5 * SECOND).unwrap();
    let run = s.send(&turn("t1", "x")).unwrap();
    following(&f, &run, 1);
    f.push(&run.0, "run.completed", json!({"output": "ok"}));
    s.wait(&run, 5 * SECOND).unwrap();
    let lines = lines.lock().unwrap();
    assert!(!lines.is_empty());
    assert!(lines.iter().all(|l| !l.contains(fake::KEY)), "{lines:?}");
}
