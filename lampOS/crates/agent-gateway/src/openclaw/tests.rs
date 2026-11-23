//! The session against the fake gateway ([`super::fake`]).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::fake::{self, Fake, Knobs, agent, chat, event, res, res_err};
use super::{Config, Session};
use crate::{AgentSession, Error, Outcome, Phase, RunId, Turn, TurnKind};

const SECOND: Duration = Duration::from_secs(1);

fn quiet() -> super::Logger {
    Arc::new(|_: &str| {})
}

fn config(f: &Fake) -> Config {
    Config {
        url: f.url(),
        token_path: f.token_path(),
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

fn idem(req: &Value) -> String {
    req["params"]["idempotencyKey"].as_str().unwrap().to_owned()
}

fn answered(o: Outcome) -> (String, crate::Progress) {
    match o {
        Outcome::Answered { text, progress } => (text, progress),
        other => panic!("not answered: {other:?}"),
    }
}

// ---- M1: connect, refused, not ready, reconnect -------------------------

#[test]
fn connects_as_gateway_client_with_token_and_no_device() {
    let f = Fake::start();
    let s = ready(&f);
    assert!(s.connected());
    assert_eq!(s.server_version(), Some("2026.11.2"));
    let connect = &f.reqs("connect")[0];
    let p = &connect["params"];
    assert_eq!(p["minProtocol"], 4);
    assert_eq!(p["maxProtocol"], 4);
    assert_eq!(p["client"]["id"], "gateway-client");
    assert_eq!(p["client"]["mode"], "backend");
    assert_eq!(p["client"]["platform"], "linux");
    assert_eq!(p["role"], "operator");
    assert_eq!(
        p["scopes"],
        json!([
            "operator.read",
            "operator.write",
            "operator.admin",
            "operator.approvals"
        ])
    );
    assert_eq!(
        p["caps"],
        json!([
            "tool-events",
            "exec-approvals",
            "approvals",
            "plugin-approvals"
        ])
    );
    assert_eq!(p["auth"]["token"], fake::TOKEN);
    assert!(p.get("device").is_none());
    // The closed schema took it as is.
    assert_eq!(fake::schema_refusal(p), None);
    // Serving was confirmed with one cheap read.
    assert_eq!(f.reqs("sessions.list").len(), 1);
}

#[test]
fn the_fake_refuses_unknown_connect_fields_like_the_gateway() {
    let mut p = json!({"minProtocol": 4, "maxProtocol": 4,
        "client": {"id": "gateway-client", "version": "1", "platform": "linux", "mode": "backend"},
        "role": "operator", "scopes": [], "caps": [], "auth": {"token": "t"}});
    assert_eq!(fake::schema_refusal(&p), None);
    p["thinkingEvents"] = json!(true);
    assert!(fake::schema_refusal(&p).unwrap().contains("thinkingEvents"));
    p.as_object_mut().unwrap().remove("thinkingEvents");
    p["client"]["nonce"] = json!("x");
    assert!(fake::schema_refusal(&p).unwrap().contains("client.nonce"));
    p["client"].as_object_mut().unwrap().remove("nonce");
    p["minProtocol"] = json!(3);
    assert!(fake::schema_refusal(&p).is_some());
}

#[test]
fn a_bad_token_is_refused() {
    let f = Fake::start();
    std::fs::write(f.token_path(), "not-the-token").unwrap();
    let mut s = session(&f);
    let started = Instant::now();
    match s.ready(5 * SECOND) {
        Err(Error::Refused(why)) => {
            assert!(why.contains("INVALID_REQUEST"), "{why}");
            assert!(
                !why.contains("not-the-token"),
                "the token is never said: {why}"
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(
        started.elapsed() < 2 * SECOND,
        "refused at once, not retried"
    );
}

#[test]
fn another_client_id_without_a_device_gets_no_scopes_and_is_refused() {
    let f = Fake::start();
    let mut s = Session::new(Config {
        client_id: "cli".to_owned(),
        ..config(&f)
    })
    .with_logger(quiet());
    match s.ready(5 * SECOND) {
        Err(Error::Refused(why)) => assert!(why.contains("operator.write"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_gateway_not_up_is_not_ready_within_the_deadline() {
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("t"), "tok").unwrap();
    let mut s = Session::new(Config {
        url: format!("ws://127.0.0.1:{port}"),
        token_path: dir.path().join("t"),
        ..Config::default()
    })
    .with_logger(quiet());
    let within = Duration::from_millis(1500);
    let started = Instant::now();
    assert!(matches!(s.ready(within), Err(Error::NotReady(_))));
    assert!(
        started.elapsed() < within + Duration::from_millis(300),
        "{:?}",
        started.elapsed()
    );
    // No token yet is not ready either, and says where it looked.
    let mut s = Session::new(Config {
        token_path: dir.path().join("missing"),
        ..Config::default()
    })
    .with_logger(quiet());
    match s.ready(Duration::from_millis(200)) {
        Err(Error::NotReady(why)) => assert!(why.contains("missing"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn reconnects_after_the_socket_drops() {
    let f = Fake::start();
    let mut s = ready(&f);
    f.drop_conn();
    assert!(
        eventually(2 * SECOND, || !s.connected()),
        "the drop is seen"
    );
    // A call while the backoff runs answers at once.
    let started = Instant::now();
    assert!(matches!(s.send(&turn("t1", "hi")), Err(Error::NotReady(_))));
    assert!(started.elapsed() < Duration::from_millis(200));
    s.ready(5 * SECOND).expect("reconnected");
    assert!(s.connected());
    assert_eq!(f.conns(), 2);
}

#[test]
fn an_event_sequence_gap_drops_and_reconnects() {
    let f = Fake::start();
    let mut s = ready(&f);
    f.push([event("presence", json!({}))]);
    std::thread::sleep(Duration::from_millis(100));
    assert!(s.connected(), "an unbroken sequence keeps the connection");
    f.knobs(|k| k.skip_seq = true);
    f.push([event("presence", json!({}))]);
    assert!(eventually(2 * SECOND, || !s.connected()), "a gap drops it");
    s.ready(5 * SECOND).expect("reconnected");
    assert_eq!(f.conns(), 2);
}

#[test]
fn silence_past_two_ticks_drops_the_connection() {
    let f = Fake::with(Knobs {
        tick_ms: 100,
        ..Knobs::default()
    });
    let mut s = ready(&f);
    std::thread::sleep(Duration::from_millis(500));
    assert!(s.connected(), "ticks keep it alive");
    f.knobs(|k| k.silent = true);
    assert!(
        eventually(SECOND, || !s.connected()),
        "no tick for 2 x 100 ms"
    );
    f.knobs(|k| k.silent = false);
    s.ready(5 * SECOND).expect("reconnected");
}

// ---- M2: hand-over, answer, late end, approvals get/set -----------------

/// The full run of a task: its events, in the gateway's order.
fn a_whole_run(run: &str) -> Vec<Value> {
    vec![
        agent(run, "lifecycle", json!({"phase": "start"})),
        agent(
            run,
            "lifecycle",
            json!({"phase": "model", "provider": "stub", "model": "stub-1"}),
        ),
        agent(
            run,
            "assistant",
            json!({"text": "Hello ", "delta": "Hello ", "itemId": "i:1"}),
        ),
        agent(
            run,
            "tool",
            json!({"phase": "start", "name": "exec", "toolCallId": "t1",
                                   "args": {"command": "ls /tmp", "token": "sekret"}}),
        ),
        agent(
            run,
            "tool",
            json!({"phase": "update", "name": "exec", "toolCallId": "t1"}),
        ),
        agent(
            run,
            "tool",
            json!({"phase": "result", "name": "exec", "toolCallId": "t1",
                                   "result": "a b", "isError": false}),
        ),
        agent(
            run,
            "assistant",
            json!({"delta": "The answer", "text": "The answer", "itemId": "i:2"}),
        ),
        agent(
            run,
            "assistant",
            json!({"delta": " is 42.", "itemId": "i:2"}),
        ),
        agent(run, "lifecycle", json!({"phase": "finishing"})),
        agent(
            run,
            "lifecycle",
            json!({"phase": "end",
              "terminalReply": {"disposition": "visible", "text": "The answer is 42."}}),
        ),
        chat(
            run,
            "final",
            json!({"message": {"role": "assistant",
              "content": [{"type": "text", "text": "Hello\n\nThe answer is 42."}]}, "stopReason": "stop"}),
        ),
    ]
}

#[test]
fn a_task_is_handed_over_and_answered_with_its_whole_text() {
    let f = Fake::start();
    f.on("chat.send", |req| {
        let run = idem(req);
        let mut out = vec![res(req, json!({"runId": run, "status": "started"}))];
        out.extend(a_whole_run(&run));
        out
    });
    let mut s = ready(&f);
    let run = s.send(&turn("t-1", "What is the answer?")).expect("sent");
    assert!(run.0.starts_with("lamp-t-1-"), "{}", run.0);
    let req = &f.reqs("chat.send")[0];
    assert_eq!(req["params"]["sessionKey"], "agent:main:lamp");
    assert_eq!(req["params"]["message"], "What is the answer?");
    assert_eq!(req["params"]["deliver"], false);
    assert_eq!(req["params"]["queueMode"], "followup");
    assert_eq!(idem(req), run.0);

    let (text, p) = answered(s.wait(&run, 3 * SECOND).expect("ended"));
    assert_eq!(
        text, "Hello\n\nThe answer is 42.",
        "the whole text, every item"
    );
    assert_eq!(p.phase, Phase::Done);
    assert_eq!(p.task_id, "t-1");
    assert_eq!(p.tools_done, 1, "update and result count one tool once");
    let tool = p.last_tool.expect("a tool");
    assert_eq!(tool.name, "exec");
    assert!(!tool.running);
    assert!(tool.brief.contains("command=ls /tmp"), "{}", tool.brief);
    assert!(!tool.brief.contains("sekret"), "{}", tool.brief);
    assert_eq!(p.last_text.as_deref(), Some("Hello\n\nThe answer is 42."));
    assert!(p.updated_at >= p.started_at);
    assert!(s.running().is_empty());
}

/// Fail-first: with `register` moved after the `chat.send` reply (in
/// `Session::send`), these events are dropped as another's and the wait
/// times out.
#[test]
fn events_before_the_send_reply_are_the_runs() {
    let f = Fake::start();
    f.on("chat.send", |req| {
        let run = idem(req);
        vec![
            agent(&run, "lifecycle", json!({"phase": "start"})),
            agent(&run, "assistant", json!({"text": "fast answer"})),
            agent(&run, "lifecycle", json!({"phase": "end"})),
            chat(&run, "final", json!({"message": "fast answer"})),
            res(req, json!({"runId": run, "status": "started"})),
        ]
    });
    let mut s = ready(&f);
    let run = s.send(&turn("t-2", "quick")).expect("sent");
    let (text, _) = answered(s.wait(&run, Duration::from_millis(500)).expect("ended"));
    assert_eq!(text, "fast answer");
}

/// Fail-first: without the ended-run guard in `Runs::apply`, the late
/// `lifecycle error` turns the answered run into a failed one.
#[test]
fn a_late_or_duplicate_end_changes_nothing() {
    let f = Fake::start();
    let n = Arc::new(Mutex::new(0));
    {
        let n = n.clone();
        f.on("chat.send", move |req| {
            let run = idem(req);
            let mut k = n.lock().unwrap();
            *k += 1;
            let words = if *k == 1 { "first" } else { "second" };
            vec![
                res(req, json!({"runId": run})),
                agent(&run, "lifecycle", json!({"phase": "start"})),
                agent(&run, "assistant", json!({"text": words})),
                agent(&run, "lifecycle", json!({"phase": "end"})),
                chat(&run, "final", json!({"message": words})),
            ]
        });
    }
    let mut s = ready(&f);
    let a = s.send(&turn("a", "one")).unwrap();
    assert_eq!(answered(s.wait(&a, 2 * SECOND).unwrap()).0, "first");
    f.push([
        agent(&a.0, "lifecycle", json!({"phase": "end"})),
        agent(
            &a.0,
            "lifecycle",
            json!({"phase": "error", "error": "late"}),
        ),
        agent(&a.0, "assistant", json!({"text": "rewritten"})),
        chat(&a.0, "final", json!({"message": "other"})),
        chat(&a.0, "error", json!({"errorMessage": "late"})),
    ]);
    let b = s.send(&turn("b", "two")).unwrap();
    assert_eq!(answered(s.wait(&b, 2 * SECOND).unwrap()).0, "second");
    std::thread::sleep(Duration::from_millis(50));
    let (text, p) = answered(s.wait(&a, SECOND).unwrap());
    assert_eq!(text, "first");
    assert_eq!(p.phase, Phase::Done);
    assert_eq!(p.last_text.as_deref(), Some("first"));
}

#[test]
fn approvals_are_read_and_the_fence_added_over_rpc() {
    let f = Fake::start();
    let file = Arc::new(Mutex::new((
        json!({"version": 1, "defaults": {"security": "full", "ask": "off"}, "agents": {}}),
        1u32,
    )));
    let stale_once = Arc::new(Mutex::new(true));
    {
        let file = file.clone();
        f.on("exec.approvals.get", move |req| {
            let (f, h) = &*file.lock().unwrap();
            vec![res(
                req,
                json!({"path": "/x", "exists": true, "hash": format!("h{h}"), "file": f}),
            )]
        });
    }
    {
        let file = file.clone();
        let stale_once = stale_once.clone();
        f.on("exec.approvals.set", move |req| {
            let mut g = file.lock().unwrap();
            let mut stale = stale_once.lock().unwrap();
            if std::mem::take(&mut *stale) {
                // Someone else wrote first: the hash moved.
                g.1 += 1;
                return vec![res_err(
                    req,
                    "INVALID_REQUEST",
                    "stale hash",
                    json!({"reason": "stale-hash"}),
                )];
            }
            assert_eq!(req["params"]["baseHash"], format!("h{}", g.1));
            g.0 = req["params"]["file"].clone();
            g.1 += 1;
            vec![res(req, json!({"hash": format!("h{}", g.1)}))]
        });
    }
    let mut s = ready(&f);
    let read = s.approvals_get(2 * SECOND).unwrap();
    assert_eq!(read["exists"], true);
    assert!(read["file"]["agents"].is_object());

    let fence = "/usr/local/lib/lampos/skill-start";
    assert!(s.allowlist_add(fence, 3 * SECOND).unwrap(), "added");
    assert_eq!(
        f.reqs("exec.approvals.set").len(),
        2,
        "retried once on the stale hash"
    );
    let now = file.lock().unwrap().0.clone();
    assert_eq!(
        now["agents"]["main"]["allowlist"],
        json!([{"pattern": fence}])
    );
    assert_eq!(
        now["defaults"]["ask"], "off",
        "the rest of the file is kept"
    );
    assert!(
        !s.allowlist_add(fence, 3 * SECOND).unwrap(),
        "already there"
    );
    assert_eq!(f.reqs("exec.approvals.set").len(), 2);
}

// ---- M3: progress --------------------------------------------------------

#[test]
fn progress_follows_the_run_as_it_goes() {
    let f = Fake::start();
    let mut s = ready(&f);
    let run = s.send(&turn("t-3", "long task")).unwrap();
    let p = s.progress(&run).unwrap();
    assert_eq!(p.phase, Phase::Queued);
    let r = run.0.clone();
    let phase = |s: &Session| s.progress(&run).unwrap().phase;

    f.push([agent(&r, "lifecycle", json!({"phase": "start"}))]);
    assert!(eventually(SECOND, || phase(&s) == Phase::Thinking));
    let first_update = s.progress(&run).unwrap().updated_at;

    f.push([agent(
        &r,
        "tool",
        json!({"phase": "start", "name": "read", "toolCallId": "c1",
        "args": "{\"path\":\"/etc/hosts\",\"apiKey\":\"k-123\"}"}),
    )]);
    assert!(eventually(SECOND, || phase(&s) == Phase::Tool));
    let p = s.progress(&run).unwrap();
    let tool = p.last_tool.unwrap();
    assert_eq!((tool.name.as_str(), tool.running), ("read", true));
    assert!(
        tool.brief.contains("path=/etc/hosts") && !tool.brief.contains("k-123"),
        "{}",
        tool.brief
    );
    assert_eq!(p.tools_done, 0);
    assert!(p.updated_at >= first_update);
    assert_eq!(s.running().len(), 1);

    f.push([agent(
        &r,
        "tool",
        json!({"phase": "result", "name": "read", "toolCallId": "c1"}),
    )]);
    assert!(eventually(SECOND, || s.progress(&run).unwrap().tools_done == 1));
    let long = "x".repeat(300) + &"y".repeat(300);
    f.push([agent(&r, "assistant", json!({"delta": long}))]);
    assert!(eventually(SECOND, || phase(&s) == Phase::Writing));
    let text = s.progress(&run).unwrap().last_text.unwrap();
    assert_eq!(text.chars().count(), 400);
    assert!(text.ends_with(&"y".repeat(300)));
    assert!(matches!(
        s.wait(&run, Duration::from_millis(100)),
        Err(Error::TimedOut)
    ));
    assert!(matches!(
        s.wait(&RunId("nope".to_owned()), SECOND),
        Err(Error::UnknownRun(_))
    ));
}

// ---- Enhancements --------------------------------------------------------

#[test]
fn a_plan_is_progress() {
    let f = Fake::start();
    let mut s = ready(&f);
    let run = s.send(&turn("t-4", "plan it")).unwrap();
    f.push([
        agent(&run.0, "lifecycle", json!({"phase": "start"})),
        agent(
            &run.0,
            "plan",
            json!({"phase": "update", "title": "Plan", "explanation": "1/2 complete",
            "steps": [{"status": "completed", "step": "read the notes"},
                      {"status": "in_progress", "step": "write the answer"}]}),
        ),
    ]);
    assert!(eventually(SECOND, || s.progress(&run).unwrap().plan.len() == 2));
    let plan = s.progress(&run).unwrap().plan;
    assert_eq!(plan[0].text, "read the notes");
    assert!(plan[0].done && !plan[1].done);
}

#[test]
fn approval_prompts_are_answered_by_policy_at_once() {
    let f = Fake::start();
    let _s = ready(&f);
    f.push([
        event("exec.approval.requested",
              json!({"id": "ap1", "request": {"command": "/usr/local/lib/lampos/skill-start run-7"}})),
        event("exec.approval.requested", json!({"id": "ap2", "request": {"command": "rm -rf /"}})),
        event("exec.approval.requested",
              json!({"id": "ap3", "request": {"command": "/usr/local/lib/lampos/skill-start x; rm -rf /"}})),
        event("exec.approval.requested", json!({"id": "ap4", "request": {
            "command": "/usr/local/lib/lampos/skill-start run-8",
            "commandArgv": ["/usr/local/lib/lampos/skill-start", "run-8"], "envKeys": ["LD_PRELOAD"]}})),
        event("plugin.approval.requested", json!({"id": "pl1"})),
    ]);
    f.wait_req("exec.approval.resolve", 4, 2 * SECOND);
    let answers: Vec<(Value, Value)> = f
        .reqs("exec.approval.resolve")
        .iter()
        .map(|r| (r["params"]["id"].clone(), r["params"]["decision"].clone()))
        .collect();
    assert_eq!(
        answers,
        vec![
            (json!("ap1"), json!("allow-once")),
            (json!("ap2"), json!("deny")),
            (json!("ap3"), json!("deny")),
            (json!("ap4"), json!("deny")),
        ]
    );
    let plugin = f.wait_req("plugin.approval.resolve", 1, 2 * SECOND);
    assert_eq!(plugin["params"], json!({"id": "pl1", "decision": "deny"}));
}

#[test]
#[ignore = "resync on connect is off (RESYNC_ON_CONNECT)"]
fn a_run_that_ended_while_the_socket_was_down_is_closed_from_history() {
    let f = Fake::start();
    let mut s = ready(&f);
    let run = s.send(&turn("t-5", "Do the thing")).unwrap();
    f.push([
        agent(&run.0, "lifecycle", json!({"phase": "start"})),
        agent(&run.0, "assistant", json!({"text": "partial"})),
    ]);
    assert!(eventually(SECOND, || s.progress(&run).unwrap().phase == Phase::Writing));
    let r = run.0.clone();
    f.on("chat.history", move |req| {
        vec![res(req, json!({"messages": [
            {"role": "user", "content": "Do the thing", "idempotencyKey": format!("{r}:user")},
            {"role": "assistant", "content": [{"type": "toolCall", "name": "exec"}], "stopReason": "toolUse"},
            {"role": "toolResult", "content": "ok"},
            {"role": "assistant", "content": [{"type": "text", "text": "All done."}], "stopReason": "stop"}
        ]}))]
    });
    f.drop_conn();
    let (text, p) = answered(s.wait(&run, 5 * SECOND).expect("closed from history"));
    assert_eq!(text, "All done.");
    assert_eq!(p.phase, Phase::Done);
    assert_eq!(f.conns(), 2);
    assert_eq!(
        f.reqs("chat.history")[0]["params"]["sessionKey"],
        "agent:main:lamp"
    );
}

#[test]
#[ignore = "resync on connect is off (RESYNC_ON_CONNECT)"]
fn a_run_still_in_flight_after_a_reconnect_goes_on() {
    let f = Fake::start();
    let mut s = ready(&f);
    let run = s.send(&turn("t-6", "Keep going")).unwrap();
    f.push([agent(&run.0, "lifecycle", json!({"phase": "start"}))]);
    assert!(eventually(SECOND, || s.progress(&run).unwrap().phase
        == Phase::Thinking));
    let r = run.0.clone();
    f.on("chat.history", move |req| {
        vec![res(req, json!({"messages": [{"role": "user", "content": "Keep going", "idempotencyKey": format!("{r}:user")}],
            "inFlightRun": {"runId": r, "text": "halfway", "startedAt": 1,
                            "plan": {"steps": [{"status": "completed", "step": "a"}]}}}))]
    });
    f.drop_conn();
    assert!(eventually(2 * SECOND, || !s.connected()));
    s.ready(5 * SECOND).unwrap();
    let p = s.progress(&run).unwrap();
    assert_eq!(p.last_text.as_deref(), Some("halfway"));
    assert_eq!(p.plan.len(), 1);
    assert_eq!(s.running().len(), 1, "still going");
    f.push([
        agent(
            &run.0,
            "assistant",
            json!({"text": "halfway there and done"}),
        ),
        agent(&run.0, "lifecycle", json!({"phase": "end"})),
        chat(
            &run.0,
            "final",
            json!({"message": "halfway there and done"}),
        ),
    ]);
    assert_eq!(
        answered(s.wait(&run, 2 * SECOND).unwrap()).0,
        "halfway there and done"
    );
}

#[test]
#[ignore = "resync on connect is off (RESYNC_ON_CONNECT)"]
fn a_run_gone_with_a_restart_fails_instead_of_hanging() {
    let f = Fake::start();
    let mut s = ready(&f);
    let run = s.send(&turn("t-7", "Lost one")).unwrap();
    f.push([agent(&run.0, "lifecycle", json!({"phase": "start"}))]);
    assert!(eventually(SECOND, || s.progress(&run).unwrap().phase
        == Phase::Thinking));
    f.drop_conn(); // history now: empty, nothing in flight
    match s.wait(&run, 5 * SECOND).unwrap() {
        Outcome::Failed { why, .. } => assert!(why.contains("away"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn abort_stops_the_run() {
    let f = Fake::start();
    f.on("chat.abort", |req| {
        let run = req["params"]["runId"].as_str().unwrap().to_owned();
        vec![
            res(req, json!({"aborted": true})),
            chat(&run, "aborted", json!({})),
        ]
    });
    let mut s = ready(&f);
    let run = s.send(&turn("t-8", "stop me")).unwrap();
    f.push([agent(&run.0, "lifecycle", json!({"phase": "start"}))]);
    assert!(eventually(SECOND, || s.progress(&run).unwrap().phase
        == Phase::Thinking));
    s.abort(&run).unwrap();
    let req = &f.reqs("chat.abort")[0];
    assert_eq!(
        req["params"],
        json!({"sessionKey": "agent:main:lamp", "runId": run.0})
    );
    assert!(matches!(
        s.wait(&run, SECOND).unwrap(),
        Outcome::Aborted { .. }
    ));
    assert!(s.abort(&run).is_ok(), "aborting an ended run is a no-op");
}

#[test]
fn reset_starts_a_fresh_conversation_on_the_kept_key() {
    let f = Fake::start();
    let mut s = ready(&f);
    let run = s.send(&turn("t-10", "old")).unwrap();
    f.push([agent(&run.0, "lifecycle", json!({"phase": "start"}))]);
    assert!(eventually(SECOND, || s.progress(&run).unwrap().phase
        == Phase::Thinking));
    s.reset().unwrap();
    assert_eq!(
        f.reqs("sessions.reset")[0]["params"],
        json!({"key": "agent:main:lamp", "reason": "new"})
    );
    assert!(matches!(
        s.wait(&run, SECOND).unwrap(),
        Outcome::Aborted { .. }
    ));
    assert!(s.running().is_empty());
    assert_eq!(s.session_key(), "agent:main:lamp");
}

#[test]
fn an_empty_final_for_a_folded_send_is_the_active_runs_answer() {
    let f = Fake::start();
    let mut s = ready(&f);
    let a = s.send(&turn("a", "slow reply please")).unwrap();
    f.push([
        agent(&a.0, "lifecycle", json!({"phase": "start"})),
        agent(&a.0, "lifecycle", json!({"phase": "model"})),
        agent(
            &a.0,
            "assistant",
            json!({"text": "slow", "delta": "slow", "itemId": "x:1"}),
        ),
    ]);
    assert!(eventually(SECOND, || s.progress(&a).unwrap().phase == Phase::Writing));
    let b = s.send(&turn("b", "say hi")).unwrap();
    // As the gateway does for a message folded mid-stream (h2).
    f.push([chat(&b.0, "final", json!({}))]);
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        s.wait(&b, Duration::from_millis(50)).is_err(),
        "an empty final is not b's answer"
    );
    f.push([
        agent(
            &a.0,
            "assistant",
            json!({"text": "hi", "delta": "hi", "itemId": "x:2"}),
        ),
        agent(&a.0, "lifecycle", json!({"phase": "end"})),
        chat(&a.0, "final", json!({"message": "slow\n\nhi"})),
    ]);
    assert_eq!(answered(s.wait(&b, 2 * SECOND).unwrap()).0, "slow\n\nhi");
    assert_eq!(answered(s.wait(&a, SECOND).unwrap()).0, "slow\n\nhi");
}

#[test]
fn a_starting_gateway_is_retried_when_it_says() {
    // `UNAVAILABLE` with `retryAfterMs: 200`: tried again then, not after a
    // blind backoff.
    let f = Fake::with(Knobs {
        starting: 1,
        tick_ms: 15_000,
        ..Knobs::default()
    });
    let mut s = session(&f);
    let started = Instant::now();
    s.ready(5 * SECOND).expect("ready once started");
    assert_eq!(f.conns(), 2);
    assert!(
        started.elapsed() < Duration::from_millis(900),
        "{:?}",
        started.elapsed()
    );

    // Close 1013 "gateway starting": not ready, and tried again in 500 ms.
    let f = Fake::with(Knobs {
        close_1013: 1,
        tick_ms: 15_000,
        ..Knobs::default()
    });
    let mut s = session(&f);
    match s.ready(Duration::from_millis(300)) {
        Err(Error::NotReady(why)) => assert!(why.contains("1013"), "{why}"),
        other => panic!("{other:?}"),
    }
    let started = Instant::now();
    s.ready(5 * SECOND).expect("ready after 1013");
    assert!(
        started.elapsed() < Duration::from_millis(900),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn a_send_whose_reply_never_came_goes_again_with_the_same_key() {
    let f = Fake::start();
    let calls = Arc::new(Mutex::new(0));
    {
        let calls = calls.clone();
        f.on("chat.send", move |req| {
            let mut n = calls.lock().unwrap();
            *n += 1;
            if *n == 1 {
                vec![] // the reply is lost
            } else {
                vec![res(req, json!({"runId": idem(req), "status": "started"}))]
            }
        });
    }
    let mut s = Session::new(Config {
        call_timeout: Duration::from_millis(300),
        ..config(&f)
    })
    .with_logger(quiet());
    s.ready(5 * SECOND).unwrap();
    let t = turn("t-12", "only once");
    assert!(matches!(s.send(&t), Err(Error::TimedOut)));
    let run = s.send(&t).expect("confirmed the second time");
    let sends = f.reqs("chat.send");
    assert_eq!(sends.len(), 2);
    assert_eq!(
        idem(&sends[0]),
        idem(&sends[1]),
        "the gateway dedupes by key"
    );
    assert_eq!(run.0, idem(&sends[1]));
    assert_eq!(s.running().len(), 1, "one run, not two");
    // A new task gets a new key.
    let other = s.send(&turn("t-13", "another")).unwrap();
    assert_ne!(other, run);
}

#[test]
fn an_abort_the_gateway_did_not_do_leaves_the_run_to_its_events() {
    let f = Fake::start();
    f.on("chat.abort", |req| {
        vec![res(
            req,
            json!({"ok": true, "aborted": false, "runIds": []}),
        )]
    });
    let mut s = ready(&f);
    let run = s.send(&turn("t-14", "x")).unwrap();
    s.abort(&run).unwrap();
    assert!(
        s.wait(&run, Duration::from_millis(100)).is_err(),
        "not marked aborted"
    );
    f.push([
        agent(&run.0, "lifecycle", json!({"phase": "start"})),
        agent(&run.0, "lifecycle", json!({"phase": "end"})),
        chat(&run.0, "final", json!({"message": "done anyway"})),
    ]);
    assert_eq!(answered(s.wait(&run, 2 * SECOND).unwrap()).0, "done anyway");
}

#[test]
#[ignore = "resync on connect is off (RESYNC_ON_CONNECT)"]
fn a_failing_resync_is_paced_not_spun() {
    let f = Fake::start();
    let mut s = ready(&f);
    let run = s.send(&turn("t-15", "x")).unwrap();
    f.push([agent(&run.0, "lifecycle", json!({"phase": "start"}))]);
    assert!(eventually(SECOND, || s.progress(&run).unwrap().phase
        == Phase::Thinking));
    f.on("chat.history", |req| {
        vec![res_err(req, "UNAVAILABLE", "busy", json!({}))]
    });
    f.drop_conn();
    assert!(matches!(s.wait(&run, 3 * SECOND), Err(Error::TimedOut)));
    let n = f.reqs("chat.history").len();
    assert!((1..=3).contains(&n), "{n} history reads in ~2 s");
}

#[test]
fn runs_not_ours_are_ignored() {
    let f = Fake::start();
    let mut s = ready(&f);
    let run = s.send(&turn("t-16", "mine")).unwrap();
    // The gateway's own runs: a boot-time skill review, a random id.
    f.push([
        agent("skill-review-1", "lifecycle", json!({"phase": "start"})),
        agent(
            "09190561-d9e6-42c3-8b91-d1b41e57e74a",
            "run_status",
            json!({"phase": "preparing_workspace"}),
        ),
        agent("skill-review-1", "assistant", json!({"text": "not yours"})),
        agent("skill-review-1", "lifecycle", json!({"phase": "end"})),
        chat("skill-review-1", "final", json!({"message": "not yours"})),
    ]);
    std::thread::sleep(Duration::from_millis(150));
    let p = s.progress(&run).unwrap();
    assert_eq!(p.phase, Phase::Queued);
    assert_eq!(p.last_text, None);
    assert_eq!(s.running().len(), 1);
}

#[test]
fn ready_blocks_through_a_backoff_past_its_deadline() {
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("t"), "tok").unwrap();
    let mut s = Session::new(Config {
        url: format!("ws://127.0.0.1:{port}"),
        token_path: dir.path().join("t"),
        ..Config::default()
    })
    .with_logger(quiet());
    // The first failure starts a 1 s backoff; each later call within it
    // must still take its whole `within`, not return at once.
    let within = Duration::from_millis(200);
    let started = Instant::now();
    let mut calls = 0;
    while started.elapsed() < Duration::from_millis(900) {
        assert!(matches!(s.ready(within), Err(Error::NotReady(_))));
        calls += 1;
    }
    assert!(calls <= 6, "{calls} calls in 900 ms: ready() spins");
}

#[test]
fn session_tokens_are_read_from_history_once_a_run_ends() {
    let f = Fake::start();
    f.on("chat.history", |req| {
        vec![res(
            req,
            json!({"messages": [], "sessionInfo": {"totalTokens": 1234}}),
        )]
    });
    let mut s = ready(&f);
    let run = s.send(&turn("t-22", "x")).unwrap();
    f.push(a_whole_run(&run.0));
    answered(s.wait(&run, 2 * SECOND).unwrap());
    assert_eq!(s.session_tokens(), 1234);
    let _ = s.wait(&run, SECOND).unwrap();
    // One at connect, one after the run, none on a second wait.
    assert_eq!(f.reqs("chat.history").len(), 2, "once per run");
    // No `totalTokens`: the context budget's estimate.
    f.on("chat.history", |req| {
        vec![res(
            req,
            json!({"messages": [], "sessionInfo": {"contextBudgetStatus":
                {"estimatedPromptTokens": 15823}}}),
        )]
    });
    let run = s.send(&turn("t-23", "y")).unwrap();
    f.push(a_whole_run(&run.0));
    answered(s.wait(&run, 2 * SECOND).unwrap());
    assert_eq!(s.session_tokens(), 15823);
}

/// ee17: the size (the larger of `totalTokens` and the estimate, and
/// `totalMessages`) is read at connect, so the rotation never sees 0 right
/// after a (re)connect; read again before a send once a run grew it, and
/// not twice in a row.
#[test]
fn the_size_is_read_at_connect_and_again_once_a_run_grew_it() {
    let f = Fake::start();
    f.on("chat.history", |req| {
        vec![res(
            req,
            json!({"messages": [], "totalMessages": 224, "sessionInfo":
                {"totalTokens": 9, "contextBudgetStatus": {"estimatedPromptTokens": 127_000}}}),
        )]
    });
    let mut s = ready(&f);
    assert_eq!(f.reqs("chat.history").len(), 1, "read at connect");
    assert_eq!(s.size_now(), Some((127_000, 224)));
    assert_eq!(f.reqs("chat.history").len(), 1, "fresh: not read again");
    let run = s.send(&turn("t-24", "x")).unwrap();
    assert_eq!(s.size_now(), Some((127_000, 224)));
    assert_eq!(f.reqs("chat.history").len(), 2, "read again after a send");
    f.push(a_whole_run(&run.0));
    answered(s.wait(&run, 2 * SECOND).unwrap());
    assert_eq!(f.reqs("chat.history").len(), 3, "once after the run");
    let _ = s.size_now();
    assert_eq!(f.reqs("chat.history").len(), 3);
}

/// ee17 2026-11-23 15:59-16:07: a gateway busy starting up (event loop
/// saturated) answers the handshake slowly. A connect asked with a short
/// bound (the pump's 2 s ready look) still connects: the connect has its
/// own bound, and a slow handshake never grows the backoff.
#[test]
fn a_handshake_that_answers_after_15_s_still_connects() {
    let f = Fake::start();
    f.knobs(|k| k.challenge_delay = Duration::from_secs(15));
    let mut s = session(&f);
    let began = Instant::now();
    s.ready(2 * SECOND).expect("connected");
    assert!(began.elapsed() >= 15 * SECOND, "{:?}", began.elapsed());
    assert!(s.connected());
}

/// The owner's call (2026-11-23): a drop mid-run (a restart's 1012 or a
/// lost socket) fails the run at once with the person's "try again"
/// line, no wait for the gateway's restart recovery; one whose final came
/// before keeps its answer.
#[test]
fn a_drop_mid_run_fails_it_at_once_with_the_try_again_line() {
    for restart in [true, false] {
        let f = Fake::start();
        let mut s = ready(&f);
        let done = s.send(&turn("t-25", "quick")).unwrap();
        f.push(a_whole_run(&done.0));
        let (answer, _) = answered(s.wait(&done, 2 * SECOND).unwrap());
        let run = s.send(&turn("t-26", "slow")).unwrap();
        f.push([agent(&run.0, "lifecycle", json!({"phase": "start"}))]);
        assert!(eventually(SECOND, || s.progress(&run).unwrap().phase
            == Phase::Thinking));
        if restart {
            f.restart();
        } else {
            f.drop_conn();
        }
        assert!(eventually(SECOND, || matches!(
            s.progress(&run).unwrap().phase,
            Phase::Failed { .. }
        )));
        match s.wait(&run, SECOND).unwrap() {
            Outcome::Failed { why, .. } => assert_eq!(why, super::DROPPED),
            other => panic!("{other:?}"),
        }
        assert_eq!(answered(s.wait(&done, SECOND).unwrap()).0, answer);
    }
}

/// ee17 2026-11-23 17:0x: after the reconnect OpenClaw's restart recovery
/// re-ran the failed task (76 s after ready, under its own run id) and the
/// next task waited 4 min behind it. A run of the kept session that is not
/// one of ours going, after a drop failed ours, is aborted at its first
/// event (once), and the next task goes through.
#[test]
fn a_recovery_run_after_a_drop_is_aborted_and_the_next_task_goes_on() {
    let f = Fake::start();
    let mut s = ready(&f);
    let run = s.send(&turn("t-27", "slow")).unwrap();
    f.push([agent(&run.0, "lifecycle", json!({"phase": "start"}))]);
    assert!(eventually(SECOND, || s.progress(&run).unwrap().phase
        == Phase::Thinking));
    f.restart();
    assert!(matches!(
        s.wait(&run, SECOND).unwrap(),
        Outcome::Failed { .. }
    ));
    s.ready(10 * SECOND).unwrap();
    let stray = "d280be1c-0000-4000-8000-000000000000";
    f.push([
        agent(stray, "lifecycle", json!({"phase": "start"})),
        agent(stray, "assistant", json!({"text": "redoing it"})),
        agent(&run.0, "lifecycle", json!({"phase": "start"})),
    ]);
    let aborted = |id: &str| {
        f.reqs("chat.abort")
            .iter()
            .filter(|req| req["params"]["runId"] == id)
            .count()
    };
    assert!(eventually(2 * SECOND, || aborted(stray) == 1 && aborted(&run.0) == 1));
    let next = s.send(&turn("t-28", "next")).unwrap();
    f.push(a_whole_run(&next.0));
    answered(s.wait(&next, 2 * SECOND).unwrap());
    assert_eq!(aborted(stray), 1, "once");
    assert_eq!(aborted(&next.0), 0, "ours, live, never aborted");
}

#[test]
fn session_tokens_are_summed_from_usage_until_read() {
    let f = Fake::start();
    let mut s = session(&f);
    s.ready(5 * SECOND).unwrap();
    let run = s.send(&turn("t-19", "x")).unwrap();
    f.push([
        agent(&run.0, "lifecycle", json!({"phase": "start"})),
        agent(&run.0, "usage", json!({"outputTokens": 5})),
        agent(&run.0, "usage", json!({"outputTokens": 10})),
    ]);
    assert!(eventually(SECOND, || s.session_tokens() == 10));
    s.reset().unwrap();
    assert_eq!(s.session_tokens(), 0, "a fresh conversation");
}
