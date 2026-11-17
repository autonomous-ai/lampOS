//! The desk and Hydrate bridge through `lamp-cloud`'s real bus wiring
//! (#7470, `crate::devices`): the voice's ask on `devices/ask` goes up in the
//! lamp's envelope, and the cloud's reply, as `online::reply` takes it, comes
//! back on `devices/answer` under the same `req_id`. A fake cloud answers as
//! Backend2 and Backend3 posted it on #7470; no desk is touched.
use lamp_bus::Bus;
use lamp_cloud::answer::Lamp;
use lamp_cloud::lamp::{Places, Real, Ring, Texts, Wifi};
use lamp_cloud::wire::Identity;
use lamp_msgs::{
    DeviceAnswer, DeviceAsk, DeviceCommand, DeviceStatus, DevicesAnswer, DevicesAsk, NoAnswer,
};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// A lamp on its own bus under `/tmp` (a socket's path must be short), the
/// bridge on or not.
fn lamp(name: &str, on: bool) -> (Real, PathBuf) {
    let root = PathBuf::from(format!("/tmp/ldv-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let bus_root = root.join("bus");
    std::fs::create_dir_all(&bus_root).unwrap();
    let lamp = Real {
        places: Places {
            actions: root.join("call.sock"),
            proc: root.join("proc"),
            sys: root.join("sys"),
            etc: root.join("etc"),
            run: root.join("run"),
        },
        bus: Some(Bus::new(&bus_root).unwrap()),
        device_id: "d-42".into(),
        build: String::new(),
        wifi: Wifi::default(),
        ring: Ring::default(),
        texts: Texts::on(false),
        reports: None,
        skills: None,
        changes: None,
        photos: None,
        faces: None,
        schedule: None,
        update_page: false,
        update_checks: Default::default(),
        restart: None,
        guard: None,
        make_skill: None,
        write_skill: None,
        share: None,
        services: None,
        control: None,
        consent: None,
        monitor: None,
        devices: on.then(Default::default),
        upload: None,
        shutdown: None,
        harness_sign_in: None,
    };
    (lamp, bus_root)
}

fn me() -> Identity {
    Identity {
        device: "lamp".into(),
        version: "1".into(),
        id: "d-42".into(),
        mac: "lamp-0042".into(),
        timezone: String::new(),
        local_ip: String::new(),
    }
}

/// The cloud's `cmd:"data"` message for `kind`.
fn down(kind: &str, data: Value) -> Vec<u8> {
    json!({"cmd": "data", "kind": kind, "data": data})
        .to_string()
        .into_bytes()
}

/// The lamp's first tick, which listens for the asks (the session's tick
/// does it within 50 ms of starting, long before the voice asks).
fn listening(lamp: &mut Real) {
    assert!(
        lamp.devices(lamp_cloud::chat::now_ms(), true)
            .sends
            .is_empty()
    );
}

fn voice_ask(voice: &mut lamp_bus::Publisher, ask: &DevicesAsk) {
    voice.publish_json(lamp_ipc::monotonic_us(), ask).unwrap();
}

/// What the lamp puts on its channel within a second, until `enough`.
fn sent_until(lamp: &mut Real, enough: usize) -> Vec<(String, Value)> {
    let until = Instant::now() + Duration::from_secs(1);
    let mut sent = Vec::new();
    while sent.len() < enough && Instant::now() < until {
        let polled = lamp.devices(lamp_cloud::chat::now_ms(), true);
        for send in polled.sends {
            let wire = me().data("2026-11-18T03:00:00Z", send.kind, &Ok(Some(send.data)));
            sent.push((wire["kind"].as_str().unwrap().to_owned(), wire));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    sent
}

fn answer_on_bus(heard: &mut lamp_bus::Subscriber) -> DevicesAnswer {
    heard
        .recv(Duration::from_secs(1))
        .unwrap()
        .expect("an answer on devices/answer")
        .parse()
        .unwrap()
}

#[test]
fn a_list_a_move_and_a_stop_go_up_in_the_lamps_envelope_and_the_answers_come_back_by_req_id() {
    let (mut lamp, bus_root) = lamp("devices", true);
    let voice = Bus::new(&bus_root).unwrap();
    let mut asks = voice
        .publisher_declared(DevicesAsk::TOPIC, &DevicesAsk::declaration(), "voice")
        .unwrap();
    let mut answers = voice
        .subscribe_typed(DevicesAnswer::TOPIC, "voice", DevicesAnswer::TYPE)
        .unwrap();
    listening(&mut lamp);
    let identity = me();
    let cloud_says = |lamp: &mut Real, kind: &str, data: Value| {
        lamp_cloud::online::reply(&identity, lamp, &down(kind, data), "t")
    };

    voice_ask(
        &mut asks,
        &DevicesAsk {
            req_id: "r1".into(),
            ask: DeviceAsk::List,
        },
    );
    let sent = sent_until(&mut lamp, 1);
    assert_eq!(sent.len(), 1);
    let wire = &sent[0].1;
    assert_eq!(wire["type"], "data");
    assert_eq!(wire["device"], "lamp");
    assert_eq!(wire["id"], "d-42");
    assert_eq!(wire["kind"], "devices.list.get");
    assert_eq!(wire["data"], json!({"req_id": "r1"}));
    // The cloud's reply is taken, and nothing is sent back to the cloud.
    let reply = cloud_says(
        &mut lamp,
        "devices.list",
        json!({"req_id": "r1", "devices": [
            {"device_id": "desk-1", "device_type": "desk", "name": "Desk", "commands": ["goto_height", "stop"]}]}),
    );
    assert_eq!(reply, None);
    let list = answer_on_bus(&mut answers);
    assert_eq!(list.req_id, "r1");
    assert!(matches!(list.answer, DeviceAnswer::List { ref devices } if devices.len() == 1));

    voice_ask(
        &mut asks,
        &DevicesAsk {
            req_id: "r2".into(),
            ask: DeviceAsk::Command {
                device_id: "desk-1".into(),
                command: DeviceCommand::GotoHeight { cm: 110 },
            },
        },
    );
    let sent = sent_until(&mut lamp, 1);
    assert_eq!(sent[0].1["kind"], "devices.command");
    assert_eq!(
        sent[0].1["data"],
        json!({"req_id": "r2", "device_id": "desk-1", "command": "goto_height", "data": {"value": "110"}})
    );
    assert_eq!(
        cloud_says(
            &mut lamp,
            "devices.command.result",
            json!({"req_id": "r2", "device_id": "desk-1", "status": "refused", "reason": "not_allowed"})
        ),
        None
    );
    assert_eq!(
        answer_on_bus(&mut answers).answer,
        DeviceAnswer::Command {
            device_id: "desk-1".into(),
            status: DeviceStatus::Refused,
            reason: Some("not_allowed".into())
        }
    );

    // The stop, timed from the voice's publish to the lamp's send: the
    // tick is 50 ms, the target for this boundary under 100 ms.
    let began = Instant::now();
    voice_ask(
        &mut asks,
        &DevicesAsk {
            req_id: "r3".into(),
            ask: DeviceAsk::Command {
                device_id: "desk-1".into(),
                command: DeviceCommand::Stop,
            },
        },
    );
    let sent = sent_until(&mut lamp, 1);
    let took = began.elapsed();
    assert_eq!(
        sent[0].1["data"],
        json!({"req_id": "r3", "device_id": "desk-1", "command": "stop"})
    );
    assert!(
        took < Duration::from_millis(100),
        "stop: ask on the bus to the send, {took:?}"
    );
}

#[test]
fn an_ask_from_anyone_but_the_voice_is_not_sent_and_off_takes_nothing() {
    let (mut lamp, bus_root) = lamp("devices-other", true);
    let other = Bus::new(&bus_root).unwrap();
    listening(&mut lamp);
    let mut asks = other
        .publisher_declared(DevicesAsk::TOPIC, &DevicesAsk::declaration(), "lamp_bus")
        .unwrap();
    voice_ask(
        &mut asks,
        &DevicesAsk {
            req_id: "x".into(),
            ask: DeviceAsk::List,
        },
    );
    assert!(sent_until(&mut lamp, 1).is_empty());

    // Off: a voice's ask is never read, and the cloud's devices kinds are
    // refused as any unknown kind (the lamp is as it was before #7470).
    let (mut off, bus_root) = lamp_off();
    listening(&mut off);
    let voice = Bus::new(&bus_root).unwrap();
    let mut asks = voice
        .publisher_declared(DevicesAsk::TOPIC, &DevicesAsk::declaration(), "voice")
        .unwrap();
    voice_ask(
        &mut asks,
        &DevicesAsk {
            req_id: "y".into(),
            ask: DeviceAsk::List,
        },
    );
    assert!(!off.devices_on());
    assert!(sent_until(&mut off, 1).is_empty());
    let refused = lamp_cloud::online::reply(
        &me(),
        &mut off,
        &down("devices.list", json!({"req_id": "y", "devices": []})),
        "t",
    )
    .expect("an unknown kind is answered, as before");
    let refused: Value = serde_json::from_str(&refused).unwrap();
    assert_eq!(refused["status"], "failure");
}

fn lamp_off() -> (Real, PathBuf) {
    lamp("devices-off", false)
}

#[test]
fn a_broker_away_is_answered_away_on_the_bus() {
    let (mut lamp, bus_root) = lamp("devices-away", true);
    let voice = Bus::new(&bus_root).unwrap();
    let mut asks = voice
        .publisher_declared(DevicesAsk::TOPIC, &DevicesAsk::declaration(), "voice")
        .unwrap();
    let mut answers = voice
        .subscribe_typed(DevicesAnswer::TOPIC, "voice", DevicesAnswer::TYPE)
        .unwrap();
    listening(&mut lamp);
    voice_ask(
        &mut asks,
        &DevicesAsk {
            req_id: "z".into(),
            ask: DeviceAsk::List,
        },
    );
    assert!(
        lamp.devices(lamp_cloud::chat::now_ms(), false)
            .sends
            .is_empty()
    );
    let answer = answer_on_bus(&mut answers);
    assert_eq!(answer.req_id, "z");
    assert_eq!(
        answer.answer,
        DeviceAnswer::None {
            why: NoAnswer::Away
        }
    );
}
