//! The real gateway's recorded frames (`tests/fixtures/openclaw-2026.11.2`)
//! replayed through the parser and the run state: what the pinned gateway
//! sends must read as these tests say.

use std::collections::HashMap;

use serde_json::Value;

use super::frames::{self, Inbound, event};
use super::runs::Runs;
use crate::{Outcome, Phase, RunId, TurnKind};

struct Rec {
    conn: String,
    dir: String,
    frame: Value,
    meta: Value,
}

fn load(name: &str) -> Vec<Rec> {
    let path = format!(
        "{}/tests/fixtures/openclaw-2026.11.2/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{path}: {e}"))
        .lines()
        .map(|l| {
            let v: Value = serde_json::from_str(l).expect("a JSON line");
            Rec {
                conn: v["conn"].as_str().unwrap_or("").to_owned(),
                dir: v["dir"].as_str().unwrap_or("").to_owned(),
                frame: v.get("frame").cloned().unwrap_or(Value::Null),
                meta: v,
            }
        })
        .collect()
}

fn inbound(name: &str) -> Vec<(String, Inbound)> {
    load(name)
        .into_iter()
        .filter(|r| r.dir == "in")
        .map(|r| (r.conn, frames::parse(&r.frame.to_string())))
        .collect()
}

/// Every event and reply of `name` folded into `runs`, as the socket
/// thread folds them.
fn replay(name: &str, runs: &mut Runs) {
    for (_, frame) in inbound(name) {
        if let Inbound::Event(e) = frame {
            match e.name.as_str() {
                event::AGENT => {
                    let evt = frames::agent_event(&e.payload).expect("an agent event");
                    runs.on_agent(evt);
                }
                event::CHAT => runs.on_chat(frames::chat_event(&e.payload).expect("a chat event")),
                _ => {}
            }
        }
    }
}

fn run(id: &str, message: &str, runs: &mut Runs) -> RunId {
    let id = RunId(id.to_owned());
    runs.register(id.clone(), "t1", message, TurnKind::Spoken);
    id
}

const ALL: [&str; 17] = [
    "a-handshake.jsonl",
    "b-client-id-test.jsonl",
    "c-wrong-token.jsonl",
    "d-sessions-list.jsonl",
    "e1-chat-send-text.jsonl",
    "e2-chat-send-tool.jsonl",
    "e3-chat-send-model-error.jsonl",
    "f-chat-history-inflight.jsonl",
    "g1-chat-abort-before-model.jsonl",
    "g2-chat-abort-mid-stream.jsonl",
    "h1-chat-steer-before-model.jsonl",
    "h2-chat-steer-mid-stream.jsonl",
    "i-exec-approvals.jsonl",
    "j1-startup-first-boot.jsonl",
    "j2-startup-window-after-restart.jsonl",
    "k-term-mid-run-restart.jsonl",
    "l-sessions-reset.jsonl",
];

#[test]
fn every_recorded_frame_parses_and_no_connection_has_a_seq_gap() {
    for name in ALL {
        let mut last: HashMap<String, u64> = HashMap::new();
        for (conn, frame) in inbound(name) {
            match frame {
                Inbound::Other => panic!("{name}: a frame read as neither res nor event"),
                Inbound::Res(_) => {}
                Inbound::Event(e) => {
                    match e.name.as_str() {
                        event::AGENT => {
                            assert!(frames::agent_event(&e.payload).is_some(), "{name}")
                        }
                        event::CHAT => assert!(frames::chat_event(&e.payload).is_some(), "{name}"),
                        _ => {}
                    }
                    if e.name == event::CHALLENGE {
                        assert_eq!(e.seq, None, "{name}: the challenge has no seq");
                        continue;
                    }
                    let seq = e
                        .seq
                        .unwrap_or_else(|| panic!("{name}: {} without seq", e.name));
                    let prev = last.insert(conn.clone(), seq).unwrap_or(0);
                    assert_eq!(seq, prev + 1, "{name}/{conn}: our gap rule would drop this");
                }
            }
        }
    }
}

#[test]
fn hello_says_the_grant_and_the_tick() {
    let hello = |name: &str| {
        inbound(name)
            .into_iter()
            .find_map(|(_, f)| match f {
                Inbound::Res(r) if r.error.is_none() && r.payload["type"] == "hello-ok" => {
                    Some(frames::hello(&r.payload))
                }
                _ => None,
            })
            .expect("a hello")
    };
    let a = hello("a-handshake.jsonl");
    assert_eq!(a.server_version, "2026.11.2");
    assert_eq!(a.tick_ms, 30_000);
    assert!(
        frames::NEEDED_SCOPES
            .iter()
            .all(|s| a.scopes.iter().any(|g| g == s))
    );
    // A client id that is not `gateway-client`, no device: ok, no scopes.
    assert!(hello("b-client-id-test.jsonl").scopes.is_empty());
}

#[test]
fn refusals_and_startup_read_as_refused_and_not_ready() {
    let first_error = |name: &str| {
        inbound(name)
            .into_iter()
            .find_map(|(_, f)| match f {
                Inbound::Res(r) => r.error,
                _ => None,
            })
            .expect("an error")
    };
    let wrong = first_error("c-wrong-token.jsonl");
    assert_eq!(wrong.code, "INVALID_REQUEST");
    assert!(
        !wrong.unavailable(),
        "a wrong token is a refusal, not a retry"
    );
    let starting = first_error("j1-startup-first-boot.jsonl");
    assert!(starting.unavailable() && starting.retryable);
    assert_eq!(starting.reason.as_deref(), Some("startup-sidecars"));
    let closes: Vec<u64> = load("j1-startup-first-boot.jsonl")
        .iter()
        .filter(|r| r.meta["kind"] == "ws-close")
        .filter_map(|r| r.meta["code"].as_u64())
        .collect();
    assert_eq!(closes, vec![u64::from(frames::CLOSE_STARTING)]);
    // The restart window: refused, then starting, then hello.
    let j2: Vec<Rec> = load("j2-startup-window-after-restart.jsonl");
    assert!(
        j2.iter()
            .any(|r| r.meta["kind"] == "ws-close" && r.meta["code"] == 1013)
    );
    assert!(j2.iter().any(|r| r.frame["payload"]["type"] == "hello-ok"));
}

fn answered(o: Option<Outcome>) -> (String, crate::Progress) {
    match o {
        Some(Outcome::Answered { text, progress }) => (text, progress),
        other => panic!("not answered: {other:?}"),
    }
}

#[test]
fn a_text_run_is_answered() {
    let mut runs = Runs::default();
    let id = run("lamp-t1-1", "say hi", &mut runs);
    replay("e1-chat-send-text.jsonl", &mut runs);
    let (text, p) = answered(runs.outcome(&id));
    assert_eq!(text, "hi there!");
    assert_eq!(p.phase, Phase::Done);
    assert_eq!(p.last_text.as_deref(), Some("hi there!"));
}

#[test]
fn a_tool_run_counts_its_tool_and_answers() {
    let mut runs = Runs::default();
    let id = run(
        "lamp-t1-2c",
        "please use the tool to run echo hi",
        &mut runs,
    );
    replay("e2-chat-send-tool.jsonl", &mut runs);
    let (text, p) = answered(runs.outcome(&id));
    assert_eq!(text, "The command printed hi.");
    assert_eq!(p.tools_done, 1);
    let tool = p.last_tool.expect("a tool");
    assert_eq!(
        (tool.name.as_str(), tool.brief.as_str(), tool.running),
        ("exec", "command=echo hi", false)
    );
}

#[test]
fn a_model_error_fails_the_run() {
    let mut runs = Runs::default();
    let id = run("lamp-t1-3", "please fail now", &mut runs);
    replay("e3-chat-send-model-error.jsonl", &mut runs);
    match runs.outcome(&id) {
        Some(Outcome::Failed { why, .. }) => assert!(why.contains("LLM request rejected"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_abort_before_the_model_and_mid_stream_aborts() {
    for (name, key) in [
        ("g1-chat-abort-before-model.jsonl", "lamp-t1-g"),
        ("g2-chat-abort-mid-stream.jsonl", "lamp-t1-g2"),
    ] {
        let mut runs = Runs::default();
        let id = run(key, "slow reply please", &mut runs);
        replay(name, &mut runs);
        assert!(
            matches!(runs.outcome(&id), Some(Outcome::Aborted { .. })),
            "{name}"
        );
    }
}

#[test]
fn history_in_flight_then_lingering_after_final() {
    let histories: Vec<Value> = load("f-chat-history-inflight.jsonl")
        .into_iter()
        .filter(|r| r.dir == "in" && r.frame["payload"].get("messages").is_some())
        .map(|r| r.frame["payload"].clone())
        .collect();
    assert_eq!(histories.len(), 2);
    let during = frames::history(&histories[0]);
    assert_eq!(
        during.in_flight.as_ref().map(|f| f.run_id.as_str()),
        Some("lamp-t1-f")
    );
    let user = during.messages.iter().rfind(|m| m.role == "user").unwrap();
    assert_eq!(
        user.idem.as_deref(),
        Some("lamp-t1-f"),
        "the stored key, without `:user`"
    );

    // Mid-run: still in flight, stays open.
    let mut runs = Runs::default();
    let id = run("lamp-t1-f", "slow reply please", &mut runs);
    let evt = frames::agent_event(
        &serde_json::json!({"runId": "lamp-t1-f", "stream": "lifecycle",
        "data": {"phase": "start"}}),
    )
    .unwrap();
    runs.on_agent(evt);
    assert!(runs.resync(&during).is_empty());
    assert_eq!(runs.running().len(), 1);
    // Right after final the run still shows in flight with no text: the
    // transcript's answer closes it, the lingering entry does not hold it.
    let after = frames::history(&histories[1]);
    assert!(
        after.in_flight.is_some(),
        "the fixture shows the lingering entry"
    );
    assert_eq!(runs.resync(&after), vec![id.clone()]);
    assert_eq!(answered(runs.outcome(&id)).0, "slow reply done.");
    // And a closed run is never reopened by a later history.
    assert!(runs.resync(&during).is_empty());
    assert!(runs.outcome(&id).is_some());
}

#[test]
fn a_run_that_drained_across_a_gateway_restart_is_answered_from_history() {
    let recs = load("k-term-mid-run-restart.jsonl");
    assert!(recs.iter().any(|r| r.frame["event"] == "shutdown"));
    assert!(
        recs.iter()
            .any(|r| r.meta["kind"] == "ws-close" && r.meta["code"] == 1012)
    );
    let post = recs
        .iter()
        .find(|r| r.conn == "post" && r.frame["payload"].get("messages").is_some())
        .expect("history after the restart");
    let mut runs = Runs::default();
    let id = run("lamp-t1-k", "slow reply please", &mut runs);
    let evt = frames::agent_event(
        &serde_json::json!({"runId": "lamp-t1-k", "stream": "lifecycle",
        "data": {"phase": "start"}}),
    )
    .unwrap();
    runs.on_agent(evt);
    runs.resync(&frames::history(&post.frame["payload"]));
    assert_eq!(answered(runs.outcome(&id)).0, "slow reply done.");
}

#[test]
fn the_allowlist_edit_matches_the_recorded_set() {
    let recs = load("i-exec-approvals.jsonl");
    let get = &recs
        .iter()
        .find(|r| r.dir == "in" && r.frame["payload"].get("hash").is_some())
        .unwrap()
        .frame["payload"];
    let file = get["file"].clone();
    assert!(!frames::allowlist_has(&file, "main", "/bin/echo"));
    let edited = frames::allowlist_with(file, "main", "/bin/echo");
    let sent = recs
        .iter()
        .find(|r| r.dir == "out" && r.frame["method"] == "exec.approvals.set")
        .unwrap();
    assert_eq!(
        edited, sent.frame["params"]["file"],
        "the same edit the gateway took"
    );
    assert_eq!(
        frames::approvals_set(&edited, get["hash"].as_str()),
        sent.frame["params"]
    );
    // The stored entry gains an id; it still reads as present.
    let stored = recs
        .iter()
        .rfind(|r| r.dir == "in" && r.frame["payload"].get("hash").is_some())
        .unwrap();
    assert!(frames::allowlist_has(
        &stored.frame["payload"]["file"],
        "main",
        "/bin/echo"
    ));
}

#[test]
fn session_tokens_take_the_larger_of_total_and_the_estimate() {
    // Fixture f: `totalTokens` 10 (stale after a short reply), the context
    // estimate 15823.
    let history = load("f-chat-history-inflight.jsonl")
        .into_iter()
        .find(|r| r.dir == "in" && r.frame["payload"].get("messages").is_some())
        .expect("a history");
    assert_eq!(history.frame["payload"]["sessionInfo"]["totalTokens"], 10);
    assert_eq!(
        frames::history(&history.frame["payload"]).total_tokens,
        Some(15823)
    );
}
