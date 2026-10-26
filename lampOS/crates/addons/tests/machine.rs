//! `lamp-addons machine` on a bus of its own, against a stand-in for the
//! lamp's own Harness daemon on the loopback: the routes of openharness#1717
//! (`machine-addon/hello|publish|decision|proposals`), with its hook
//! credential checked.
use lamp_bus::Bus;
use lamp_msgs::{AddonDecision, AddonProposal, Room, RoomState, Speech, Voice, VoiceState};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const TOKEN: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQ";
const INSTANCE: &str = "11111111-2222-4333-8444-555555555555";
const OWNER: &str = "mac-of-the-owner";

/// What the stand-in daemon was asked, and the proposals in its inbox.
#[derive(Default)]
struct Daemon {
    asked: Vec<(String, Value)>,
    inbox: Vec<Value>,
    refused_credential: u32,
}

struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn daemon() -> (u16, Arc<Mutex<Daemon>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let state = Arc::new(Mutex::new(Daemon::default()));
    let shared = state.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let state = shared.clone();
            std::thread::spawn(move || answer(stream, &state));
        }
    });
    (port, state)
}

fn answer(mut stream: TcpStream, state: &Mutex<Daemon>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let mut parts = line.split(' ');
    let (method, target) = (
        parts.next().unwrap().to_owned(),
        parts.next().unwrap().to_owned(),
    );
    let (mut length, mut bearer) = (0, String::new());
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).unwrap();
        if header == "\r\n" {
            break;
        }
        let lower = header.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            length = value.trim().parse().unwrap();
        }
        if let Some(value) = header.strip_prefix("Authorization: Bearer ") {
            bearer = value.trim().to_owned();
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let reply = |stream: &mut TcpStream, status: u16, value: Value| {
        let text = value.to_string();
        let _ = write!(
            stream,
            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{text}",
            text.len()
        );
    };
    if bearer != TOKEN {
        state.lock().unwrap().refused_credential += 1;
        return reply(&mut stream, 401, json!({"error": {"code": "UNAUTHORIZED"}}));
    }
    let path = target.split('?').next().unwrap().to_owned();
    let route = path
        .trim_start_matches("/api/autonomous-device/machine-addon/")
        .to_owned();
    // Harness answers a wrong method 405, so a request sent so is never taken.
    if method != if route == "proposals" { "GET" } else { "POST" } {
        return reply(
            &mut stream,
            405,
            json!({"error": {"code": "METHOD_NOT_ALLOWED"}}),
        );
    }
    if route == "proposals" {
        let query: Vec<(&str, u64)> = target
            .split_once('?')
            .unwrap()
            .1
            .split('&')
            .filter_map(|pair| {
                let (key, value) = pair.split_once('=')?;
                Some((key, value.parse().ok()?))
            })
            .collect();
        let get = |key| query.iter().find(|(k, _)| *k == key).map_or(0, |(_, v)| *v);
        let (after, wait) = (get("after"), get("waitMs"));
        let until = Instant::now() + Duration::from_millis(wait.min(300));
        loop {
            let held = state.lock().unwrap();
            let next = held.inbox.len() as u64;
            if next > after || Instant::now() >= until {
                let proposals: Vec<Value> = held
                    .inbox
                    .iter()
                    .enumerate()
                    .skip(after as usize)
                    .map(|(at, proposal)| json!({"seq": at + 1, "proposal": proposal}))
                    .collect();
                drop(held);
                return reply(
                    &mut stream,
                    200,
                    json!({"proposals": proposals, "next": next, "instance": INSTANCE}),
                );
            }
            drop(held);
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    state
        .lock()
        .unwrap()
        .asked
        .push((route.clone(), body.clone()));
    let value = match route.as_str() {
        "hello" => json!({"topics": 0, "instance": INSTANCE}),
        "publish" => {
            json!({"topics": body["values"].as_array().map_or(0, Vec::len), "instance": INSTANCE})
        }
        "decision" => json!({"proposalId": body["proposalId"], "state": body["decision"]}),
        _ => return reply(&mut stream, 404, json!({"error": {"code": "NOT_FOUND"}})),
    };
    reply(&mut stream, 200, value)
}

fn proposal(id: &str, machine: &str) -> Value {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    json!({"proposalId": id, "addon": "evening-glow", "act": "light", "why": "evening",
        "sure": 0.6, "holdsMs": 30000, "params": {"colour": "warm", "level": 2},
        "expiresAt": now + 30_000, "from": {"machineId": machine, "label": null}})
}

fn harness_dir(root: &Path) -> std::path::PathBuf {
    let dir = root.join("harness");
    std::fs::create_dir(&dir).unwrap();
    let file = dir.join("hook-credential");
    std::fs::write(&file, format!("{TOKEN}\n")).unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    dir
}

fn service(root: &Path, port: u16, enabled: bool) -> Running {
    service_with(root, port, enabled, &[])
}

fn service_with(root: &Path, port: u16, enabled: bool, more: &[&str]) -> Running {
    let mut command = Command::new(env!("CARGO_BIN_EXE_lamp-addons"));
    command
        .args(["machine", "--bus-root"])
        .arg(root.join("b"))
        .arg("--yes-file")
        .arg(root.join("yes.json"))
        .arg("--harness-dir")
        .arg(root.join("harness"))
        .args(["--port", &port.to_string()])
        .env_remove("LAMP_MACHINE_ADDONS")
        .env_remove("LAMP_MACHINE_ADDONS_STREAM")
        .args(more)
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(root.join("machine.log")).unwrap());
    if enabled {
        command.arg("--enable");
    }
    Running(command.spawn().unwrap())
}

fn until(root: &Path, what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done() {
        assert!(
            Instant::now() < deadline,
            "never {what}; its journal:\n{}",
            std::fs::read_to_string(root.join("machine.log")).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn asked(state: &Mutex<Daemon>, route: &str) -> Vec<Value> {
    let held = state.lock().unwrap();
    held.asked
        .iter()
        .filter(|(r, _)| r == route)
        .map(|(_, body)| body.clone())
        .collect()
}

#[test]
fn off_it_reads_no_credential_and_asks_harness_nothing() {
    let root = tempfile::tempdir().unwrap();
    let (port, state) = daemon();
    let mut off = service(root.path(), port, false);
    assert!(off.0.wait().unwrap().success());
    std::thread::sleep(Duration::from_millis(200));
    let held = state.lock().unwrap();
    assert!(held.asked.is_empty());
    assert_eq!(held.refused_credential, 0);
}

#[test]
fn a_wrong_credential_is_refused_and_nothing_is_published() {
    let root = tempfile::tempdir().unwrap();
    let dir = harness_dir(root.path());
    std::fs::write(dir.join("hook-credential"), format!("{}\n", "Z".repeat(43))).unwrap();
    std::fs::write(
        root.path().join("yes.json"),
        format!(r#"{{"machines":["{OWNER}"]}}"#),
    )
    .unwrap();
    let (port, state) = daemon();
    let _service = service(root.path(), port, true);
    until(root.path(), "was refused", || {
        state.lock().unwrap().refused_credential > 0
    });
    assert!(state.lock().unwrap().asked.is_empty());
}

#[test]
fn a_proposal_from_the_owners_computer_goes_to_the_judge_and_its_decision_back() {
    let root = tempfile::tempdir().unwrap();
    harness_dir(root.path());
    std::fs::write(
        root.path().join("yes.json"),
        format!(r#"{{"machines":["{OWNER}"]}}"#),
    )
    .unwrap();
    let bus = Bus::new(root.path().join("b")).unwrap();
    let mut proposals = bus.subscribe(AddonProposal::TOPIC, "test").unwrap();
    let mut voice = bus
        .publisher_declared(VoiceState::TOPIC, &VoiceState::declaration(), "voice")
        .unwrap();
    voice
        .publish_json(
            lamp_ipc::monotonic_us(),
            &VoiceState {
                state: Voice::Resting,
            },
        )
        .unwrap();
    let (port, state) = daemon();
    // Made before the service ran: never its to take.
    state
        .lock()
        .unwrap()
        .inbox
        .push(proposal("00000000-0000-4000-8000-000000000001", OWNER));
    let _service = service(root.path(), port, true);

    // Hello first, then the summary the lamp had, as its part says it.
    until(root.path(), "published voice/state", || {
        !asked(&state, "publish").is_empty()
    });
    {
        let held = state.lock().unwrap();
        assert_eq!(held.asked[0].0, "hello");
    }
    let batch = &asked(&state, "publish")[0]["values"];
    assert_eq!(batch[0]["topic"], "voice/state");
    assert_eq!(batch[0]["value"], json!({"state": "resting"}));
    assert!(batch[0]["at"].as_u64().unwrap() > 1_700_000_000_000);

    // A stranger's proposal is answered dropped and never reaches the bus.
    let stranger = "00000000-0000-4000-8000-000000000002";
    let owners = "00000000-0000-4000-8000-000000000003";
    state
        .lock()
        .unwrap()
        .inbox
        .push(proposal(stranger, "another-mac"));
    state.lock().unwrap().inbox.push(proposal(owners, OWNER));
    let written = {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            assert!(Instant::now() < deadline, "no proposal on the bus");
            if let Some(message) = proposals.recv(Duration::from_millis(100)).unwrap() {
                break message;
            }
        }
    };
    assert_eq!(written.header.source, "machine_addons");
    let written: AddonProposal = written.parse().unwrap();
    assert!(written.addon.starts_with("own-"));
    assert_eq!(written.version, "1.0.0");
    assert_eq!(written.light.as_ref().unwrap().colour, "warm");
    assert!(
        proposals
            .recv(Duration::from_millis(300))
            .unwrap()
            .is_none()
    );
    until(root.path(), "answered the stranger", || {
        !asked(&state, "decision").is_empty()
    });
    let first = &asked(&state, "decision")[0];
    assert_eq!(first["proposalId"], stranger);
    assert_eq!(first["decision"], "dropped");
    assert_eq!(first["reason"], "no yes for this computer");

    // The judge's decision, and only the judge's, goes back.
    let decide = |source: &str, choice: &str| {
        bus.publisher_declared(AddonDecision::TOPIC, &AddonDecision::declaration(), source)
            .unwrap()
            .publish_json(
                lamp_ipc::monotonic_us(),
                &AddonDecision {
                    proposal: written.proposal,
                    addon: written.addon.clone(),
                    act: "light".into(),
                    choice: choice.into(),
                    by: "rule".into(),
                    why: "asked of the ring at idle for 30000 ms".into(),
                },
            )
            .unwrap();
    };
    decide("voice", "dropped");
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(asked(&state, "decision").len(), 1);
    decide("addons", "kept");
    until(root.path(), "sent the judge's decision", || {
        asked(&state, "decision").len() == 2
    });
    let second = &asked(&state, "decision")[1];
    assert_eq!(second["proposalId"], owners);
    assert_eq!(second["decision"], "kept");
    assert_eq!(second["reason"], "asked of the ring at idle for 30000 ms");
    // None for the proposal made before it ran.
    assert!(
        asked(&state, "decision")
            .iter()
            .all(|d| d["proposalId"] != "00000000-0000-4000-8000-000000000001")
    );
}

#[test]
fn without_the_yes_nothing_is_published_and_withdrawing_it_clears_harness() {
    let root = tempfile::tempdir().unwrap();
    harness_dir(root.path());
    let bus = Bus::new(root.path().join("b")).unwrap();
    let mut voice = bus
        .publisher_declared(VoiceState::TOPIC, &VoiceState::declaration(), "voice")
        .unwrap();
    voice
        .publish_json(
            lamp_ipc::monotonic_us(),
            &VoiceState {
                state: Voice::Resting,
            },
        )
        .unwrap();
    let (port, state) = daemon();
    let _service = service(root.path(), port, true);
    until(root.path(), "said hello", || {
        !asked(&state, "hello").is_empty()
    });
    std::thread::sleep(Duration::from_millis(1_500));
    assert!(asked(&state, "publish").is_empty(), "published with no yes");

    std::fs::write(
        root.path().join("yes.json"),
        format!(r#"{{"machines":["{OWNER}"]}}"#),
    )
    .unwrap();
    until(root.path(), "published after the yes", || {
        !asked(&state, "publish").is_empty()
    });

    std::fs::remove_file(root.path().join("yes.json")).unwrap();
    until(root.path(), "cleared Harness", || {
        asked(&state, "hello").len() == 2
    });
    let published = asked(&state, "publish").len();
    voice
        .publish_json(
            lamp_ipc::monotonic_us(),
            &VoiceState {
                state: Voice::Listening,
            },
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(1_500));
    assert_eq!(
        asked(&state, "publish").len(),
        published,
        "published after the yes was withdrawn"
    );
}

/// The lines Harness was given in the stream's slots, in order: (topic, value).
fn streamed(state: &Mutex<Daemon>) -> Vec<(String, Value)> {
    let mut lines: Vec<(u64, String, Value)> = asked(state, "publish")
        .iter()
        .flat_map(|batch| batch["values"].as_array().cloned().unwrap_or_default())
        .filter(|value| {
            value["topic"]
                .as_str()
                .is_some_and(|t| t.starts_with("lamp/log/s"))
        })
        .flat_map(|slot| {
            slot["value"]["lines"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .map(|line| {
            (
                line[0].as_u64().unwrap(),
                line[2].as_str().unwrap().to_owned(),
                line[3].clone(),
            )
        })
        .collect();
    lines.sort_by_key(|line| line.0);
    lines
        .into_iter()
        .map(|(_, topic, value)| (topic, value))
        .collect()
}

#[test]
fn with_the_switch_and_the_yes_for_it_the_bus_reaches_the_computer_as_lines() {
    let root = tempfile::tempdir().unwrap();
    harness_dir(root.path());
    std::fs::write(
        root.path().join("yes.json"),
        format!(r#"{{"machines":["{OWNER}"],"stream":true}}"#),
    )
    .unwrap();
    let bus = Bus::new(root.path().join("b")).unwrap();
    let mut room = bus
        .publisher_declared(RoomState::TOPIC, &RoomState::declaration(), "voice")
        .unwrap();
    let mut heard = bus
        .publisher_declared("voice/heard", &Speech::declaration(), "voice")
        .unwrap();
    let (port, state) = daemon();
    let _service = service_with(root.path(), port, true, &["--stream"]);
    until(root.path(), "said hello", || {
        !asked(&state, "hello").is_empty()
    });
    // The service looks for topics at start; say the room until a line comes.
    until(root.path(), "streamed room/state", || {
        room.publish_json(lamp_ipc::monotonic_us(), &RoomState { state: Room::Quiet })
            .unwrap();
        heard
            .publish_json(
                lamp_ipc::monotonic_us(),
                &serde_json::json!({"text": "words", "finished": true}),
            )
            .ok();
        streamed(&state)
            .iter()
            .any(|(topic, _)| topic == "room/state")
    });
    room.publish_json(lamp_ipc::monotonic_us(), &RoomState { state: Room::Busy })
        .unwrap();
    until(root.path(), "streamed the change", || {
        streamed(&state)
            .iter()
            .any(|(topic, value)| topic == "room/state" && value["state"] == "busy")
    });
    let lines = streamed(&state);
    // Said once each: the same meaning again is no new line.
    assert_eq!(
        lines
            .iter()
            .filter(|(topic, _)| topic == "room/state")
            .count(),
        2,
        "{lines:?}"
    );
    // What was heard goes only with the yes for words.
    assert!(
        lines.iter().all(|(topic, _)| topic != "voice/heard"),
        "{lines:?}"
    );
}

#[test]
fn without_the_switch_the_yes_for_the_stream_sends_nothing_of_it() {
    let root = tempfile::tempdir().unwrap();
    harness_dir(root.path());
    std::fs::write(
        root.path().join("yes.json"),
        format!(r#"{{"machines":["{OWNER}"],"stream":true,"words":true}}"#),
    )
    .unwrap();
    let bus = Bus::new(root.path().join("b")).unwrap();
    let mut room = bus
        .publisher_declared(RoomState::TOPIC, &RoomState::declaration(), "voice")
        .unwrap();
    let (port, state) = daemon();
    let _service = service(root.path(), port, true);
    until(root.path(), "said hello", || {
        !asked(&state, "hello").is_empty()
    });
    for _ in 0..15 {
        room.publish_json(lamp_ipc::monotonic_us(), &RoomState { state: Room::Quiet })
            .unwrap();
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(streamed(&state).is_empty());
}
