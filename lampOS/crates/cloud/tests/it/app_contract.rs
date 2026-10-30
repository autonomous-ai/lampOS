//! The fields of `lamp-cloud`'s replies that the Autonomous app's device page
//! reads, by name and type (Fourth on #1396, 09:44 and 10:10). The app reads each one
//! leniently, so a renamed or retyped field would go blank on its page with
//! nothing failing there: it fails here instead. Each reply is `lamp-cloud`'s
//! own answer from the real lamp (`Real`, on a lamp of files, a stand-in
//! `lamp-actions` and a bus with the ring's light on it), in V1's envelope,
//! through JSON as the broker carries it.
use lamp_bus::Bus;
use lamp_cloud::answer::answer;
use lamp_cloud::lamp::{Places, Real, Ring, Shutdown, Texts, Wifi};
use lamp_cloud::wire::Identity;
use lamp_msgs::{Light, LightRequest};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;

/// The type the app takes a field as.
#[derive(Clone, Copy, Debug)]
enum Type {
    Text,
    /// A whole number from 0 to 100.
    Percent,
    Whole,
    Flag,
    /// `[r, g, b]`, each 0-255.
    Rgb,
    /// An `[r, g, b]`, or null where there is none (V1's `color` unless the
    /// choice is `custom`).
    RgbOrNull,
}

/// What the app's page reads of each reply, as Fourth listed it.
const READ: &[(&str, &str, Type)] = &[
    ("system.info", "versions.firmware", Type::Text),
    ("system.info", "network.private_ip", Type::Text),
    ("system.info", "host.hostname", Type::Text),
    // The owner's language (#4473), always said: `en` with no file.
    ("system.info", "host.language", Type::Text),
    ("volume.get", "volume", Type::Percent),
    ("volume.get", "raw", Type::Whole),
    ("volume.get", "max_volume", Type::Whole),
    ("volume.set", "volume", Type::Percent),
    ("volume.set", "raw", Type::Whole),
    ("volume.set", "max_volume", Type::Whole),
    ("mic.get", "muted", Type::Flag),
    // Whether the voice is up: `false` makes the page say so instead of
    // offering the toggle (Fourth on #1396, 10:10).
    ("mic.get", "available", Type::Flag),
    // The Light row (`resting_light.dart`, Fourth on #1396, 22:27): V1's
    // snapshot, `docs/mqtt.md`'s `led.resting.*`.
    ("led.resting.get", "mode", Type::Text),
    ("led.resting.get", "color", Type::RgbOrNull),
    ("led.resting.get", "default.color", Type::Rgb),
    ("led.resting.get", "effective.color", Type::Rgb),
    ("led.resting.set", "mode", Type::Text),
    ("led.resting.set", "color", Type::RgbOrNull),
    ("led.resting.set", "default.color", Type::Rgb),
    ("led.resting.set", "effective.color", Type::Rgb),
];

fn field<'a>(data: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .try_fold(data, |value, name| value.get(name))
}

fn is(value: &Value, kind: Type) -> bool {
    match kind {
        Type::Text => value.is_string(),
        Type::Percent => value.as_u64().is_some_and(|n| n <= 100),
        Type::Whole => value.is_u64(),
        Type::Flag => value.is_boolean(),
        Type::Rgb => value.as_array().is_some_and(|channels| {
            channels.len() == 3
                && channels
                    .iter()
                    .all(|channel| channel.as_u64().is_some_and(|n| n <= 255))
        }),
        Type::RgbOrNull => value.is_null() || is(value, Type::Rgb),
    }
}

/// A lamp of files: its name, its zone, and a default route through this
/// host's loopback, so its address is found without a network.
fn lamp(root: &PathBuf) -> Places {
    let _ = std::fs::remove_dir_all(root);
    for dir in ["proc/sys/kernel", "proc/net", "sys/class/net/wlan0", "etc"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    std::fs::write(root.join("proc/sys/kernel/hostname"), "lamp-7f72\n").unwrap();
    std::fs::write(root.join("proc/uptime"), "86400.52 1000.0\n").unwrap();
    std::fs::write(root.join("etc/timezone"), "Asia/Ho_Chi_Minh\n").unwrap();
    std::fs::write(
        root.join("proc/net/route"),
        "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\n\
         wlan0\t00000000\t0100007F\t0003\t0\t0\t600\t00000000\n",
    )
    .unwrap();
    std::fs::write(
        root.join("sys/class/net/wlan0/address"),
        "02:00:00:00:7f:72\n",
    )
    .unwrap();
    Places {
        actions: root.join("call.sock"),
        proc: root.join("proc"),
        sys: root.join("sys"),
        etc: root.join("etc"),
        run: root.join("run"),
    }
}

/// A stand-in `lamp-actions` answering `calls` volume calls in its own line
/// protocol, with the level it says.
fn actions(places: &Places, calls: usize) -> std::thread::JoinHandle<()> {
    let listener = UnixListener::bind(&places.actions).unwrap();
    std::thread::spawn(move || {
        for _ in 0..calls {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            stream
                .write_all(
                    b"{\"ok\":true,\"level\":{\"volume\":44,\"raw\":35,\"max_volume\":80}}\n",
                )
                .unwrap();
        }
    })
}

#[test]
fn each_field_the_apps_page_reads_is_there_with_the_type_it_takes() {
    let root = PathBuf::from(format!("/tmp/lca-{}", std::process::id()));
    let places = lamp(&root);
    let serving = actions(&places, 2);
    let mut wifi = Wifi::default();
    wifi.program = "/nonexistent/iw".into();
    // The ring's side of the bus: the light it shows (the lamp's first,
    // which a save of `default` asks for again), and a ring that takes
    // previews.
    let bus_root = root.join("bus");
    std::fs::create_dir_all(&bus_root).unwrap();
    std::fs::create_dir_all(places.proc.join("sys/kernel/random")).unwrap();
    std::fs::write(places.proc.join("sys/kernel/random/boot_id"), "boot\n").unwrap();
    let ring = Bus::new(&bus_root).unwrap();
    let _previews = ring
        .subscribe_typed(LightRequest::TOPIC, "ring", LightRequest::TYPE)
        .unwrap();
    let mut shows = ring
        .publisher_declared(Light::TOPICS[1], &Light::declaration(), "ring")
        .unwrap();
    shows
        .publish_json(1, &Light::new(true, "warm", 1).unwrap())
        .unwrap();
    let mut real = Real {
        places,
        bus: Some(Bus::new(&bus_root).unwrap()),
        device_id: "d-42".into(),
        build: "812+460a1a1aa2b3".into(),
        // No `iw` here: the Wi-Fi's name is not one the page reads.
        wifi,
        ring: Ring::default(),
        texts: Texts::default(),
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
        shutdown: None,
    };
    let me = Identity {
        device: "lamp".into(),
        version: "812+460a1a1aa2b3".into(),
        id: "d-42".into(),
        mac: "lamp-7f72".into(),
        ..Identity::default()
    };
    let mut replies = std::collections::BTreeMap::new();
    for (kind, data) in [
        ("system.info", Value::Null),
        ("volume.get", Value::Null),
        ("volume.set", json!({"volume": 44})),
        ("mic.get", Value::Null),
        ("led.resting.get", Value::Null),
        ("led.resting.preview", json!({"color": [20, 0, 10]})),
        ("led.resting.set", json!({"mode": "default"})),
    ] {
        let result = answer(&mut real, kind, &data);
        let sent = me.data("2026-10-18T10:00:00Z", kind, &result).to_string();
        let reply: Value = serde_json::from_str(&sent).unwrap();
        assert_eq!(reply["status"], "success", "{kind}: {reply}");
        // The page is a Lamp 2's by this, in the envelope beside `device`
        // (`DeviceSystemInfo.generation`, #1396 19 Oct).
        assert_eq!(reply["generation"], "lamp-v2", "{kind}: {reply}");
        assert_eq!(reply["device"], "lamp", "{kind}: {reply}");
        replies.insert(kind, reply["data"].clone());
    }
    serving.join().unwrap();

    for (kind, path, kind_of_value) in READ {
        let value = field(&replies[kind], path);
        assert!(
            value.is_some_and(|value| is(value, *kind_of_value)),
            "{kind} {path}: the app takes {kind_of_value:?}, lamp-cloud said {value:?}"
        );
    }
    // The version and the lamp's name are shown as they are: never blank.
    let info = &replies["system.info"];
    assert_eq!(info["versions"]["firmware"], "812+460a1a1aa2b3");
    assert_eq!(info["host"]["hostname"], "lamp-7f72");
    assert_eq!(info["network"]["private_ip"], "127.0.0.1");

    // `hw_switch_muted` is a flag where the voice reads the privacy switch;
    // with no reading (no voice, or no switch) it is null, which the app's
    // lenient read shows as not muted.
    let switch = &replies["mic.get"]["hw_switch_muted"];
    assert!(switch.is_boolean() || switch.is_null(), "{switch}");
    // `versions.hal` is read too, but this lamp has no HAL: absent, or text.
    assert!(field(info, "versions.hal").is_none_or(Value::is_string));
    // Only V1's "Starting" rule waits for these; a Lamp 2 is Online on any
    // answered `system.info`, so they stay out of it.
    for absent in ["versions.openclaw_detected", "versions.openclaw"] {
        assert!(field(info, absent).is_none(), "{absent} in {info}");
    }
    let _ = std::fs::remove_dir_all(root);
}

/// The Lamp 2 page's faces (#2192, `face.list` and `face.remove`): what it
/// reads of each, by name and type, from `lamp-cloud`'s own answer to a
/// stand-in for the eyes' request socket, in V1's envelope. Without
/// `LAMP_FACES_PAGE` the kinds aren't on this lamp, and the page shows no
/// row.
#[test]
fn the_faces_page_reads_names_and_removes_one() {
    let root = PathBuf::from(format!("/tmp/lcf-{}", std::process::id()));
    let places = lamp(&root);
    let socket = root.join("who.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let eyes = std::thread::spawn(move || {
        let mut asked = Vec::new();
        for answer in [
            json!({"people": ["Lan"], "waiting": ["Bo"]}),
            json!({"forgot": true}),
            json!({"forgot": true}),
        ] {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            asked.push(serde_json::from_str::<Value>(line.trim()).unwrap());
            writeln!(&stream, "{answer}").unwrap();
        }
        asked
    });
    let mut real = Real {
        places,
        bus: None,
        device_id: "d-42".into(),
        build: "812+460a1a1aa2b3".into(),
        wifi: Wifi::default(),
        ring: Ring::default(),
        texts: Texts::default(),
        reports: None,
        skills: None,
        changes: None,
        photos: None,
        faces: Some(lamp_cloud::faces::Eyes {
            socket,
            memory: None,
        }),
        schedule: None,
        update_page: false,
        update_checks: Default::default(),
        restart: None,
        guard: None,
        make_skill: None,
        shutdown: None,
    };
    let me = Identity {
        device: "lamp".into(),
        version: "812+460a1a1aa2b3".into(),
        id: "d-42".into(),
        mac: "lamp-7f72".into(),
        ..Identity::default()
    };
    let reply = |real: &mut Real, kind: &str, data: Value| -> Value {
        let result = answer(real, kind, &data);
        let sent = me.data("2026-10-20T12:00:00Z", kind, &result).to_string();
        serde_json::from_str(&sent).unwrap()
    };
    let listed = reply(&mut real, "face.list", Value::Null);
    assert_eq!(listed["status"], "success", "{listed}");
    let faces = listed["data"]["faces"].as_array().unwrap();
    for face in faces {
        assert!(is(&face["name"], Type::Text), "{face}");
        assert!(is(&face["waiting"], Type::Flag), "{face}");
        // Names only: no print, photo or anything else of a face.
        assert_eq!(face.as_object().unwrap().len(), 2, "{face}");
    }
    assert_eq!(faces.len(), 2);
    let removed = reply(&mut real, "face.remove", json!({"name": "Lan"}));
    assert_eq!(removed["status"], "success", "{removed}");
    assert!(is(&removed["data"]["removed"], Type::Text), "{removed}");
    // Whether memory went too, and why not when it didn't (the first
    // builder on #2199, 13:08): `memory` text, and `reason` with `kept`.
    // This lamp gives lamp-cloud no memory socket.
    assert_eq!(removed["data"]["memory"], "kept", "{removed}");
    assert!(is(&removed["data"]["reason"], Type::Text), "{removed}");
    // The face only (`"entirely": false`): memory is not asked, and says so.
    let face_only = reply(
        &mut real,
        "face.remove",
        json!({"name": "Bo", "entirely": false}),
    );
    assert_eq!(face_only["status"], "success", "{face_only}");
    assert_eq!(face_only["data"]["memory"], "not asked", "{face_only}");
    // The app's probe for `entirely`: no name, refused by the name's check
    // and not "not on this lamp", and the eyes are not asked.
    let probe = reply(&mut real, "face.remove", json!({"entirely": true}));
    assert_ne!(probe["status"], "success", "{probe}");
    assert!(!probe.to_string().contains("not on this lamp"), "{probe}");
    // It names `entirely`, which a lamp of before the field never does: the
    // app offers its two choices only on that (Mobile on #2563).
    assert!(probe.to_string().contains("entirely is taken"), "{probe}");
    assert_eq!(
        eyes.join().unwrap(),
        [
            json!({"list": true, "waiting": true}),
            json!({"forget": "Lan"}),
            json!({"forget": "Bo"})
        ]
    );
    // Without the switch, neither is on this lamp.
    real.faces = None;
    for kind in ["face.list", "face.remove"] {
        let refused = reply(&mut real, kind, json!({"name": "Lan"}));
        assert_ne!(refused["status"], "success", "{kind}: {refused}");
    }
    let _ = std::fs::remove_dir_all(root);
}

/// A stand-in lamp-schedule answering each request with `replies[op]`, in
/// its own one-line protocol, for as many requests as `replies` holds.
fn schedule(socket: &PathBuf, replies: Vec<(&'static str, Value)>) -> std::thread::JoinHandle<()> {
    let listener = UnixListener::bind(socket).unwrap();
    std::thread::spawn(move || {
        for (op, reply) in replies {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let asked: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(asked["op"], op, "{asked}");
            stream.write_all(format!("{reply}\n").as_bytes()).unwrap();
        }
    })
}

/// #2195 (Mobile on #1396): with `LAMP_APP_SCHEDULE=on`, the app's Lamp 2
/// page reads the lamp's timers and reminders, each as the lamp keeps it,
/// and cancels one, through lamp-schedule (a stand-in here) and the client
/// the voice's own reminder functions use. Its refusals are its fixed
/// sentences. Without the switch, neither kind is here.
#[test]
fn schedule_list_and_cancel_answer_with_the_switch_and_are_not_here_without() {
    const STORE: &str = "0123456789abcdef0123456789abcdef";
    let root = PathBuf::from(format!("/tmp/lcs-{}", std::process::id()));
    let places = lamp(&root);
    let socket = root.join("schedule.sock");
    let sample = lamp_schedule::time::ClockSample {
        unix_ms: 1_791_964_800_000,
        boot_ms: 0,
        boot_id: "cloud-fixture".into(),
        wall_trusted: true,
    };
    let resolved = lamp_schedule::time::resolve(
        &lamp_schedule::time::When::After { seconds: 600 },
        &sample,
        "Etc/UTC",
    )
    .unwrap();
    let item = json!({"id": 1, "delivery": 1, "text": "stretch", "kind": "reminder",
        "state": "scheduled", "due_unix_ms": resolved.unix_ms, "local": resolved.local,
        "repeat": null, "snoozed": false, "late": false});
    let serving = schedule(
        &socket,
        vec![
            (
                "list",
                json!({"ok": true, "store_id": STORE, "revision": 0, "items": [item], "next_after_id": null}),
            ),
            (
                "status",
                json!({"ok": true, "store_id": STORE, "revision": 0}),
            ),
            (
                "cancel",
                json!({"ok": true, "store_id": STORE, "revision": 1, "id": 1, "delivery": 1, "cancelled": true}),
            ),
            (
                "status",
                json!({"ok": true, "store_id": STORE, "revision": 1}),
            ),
            (
                "cancel",
                json!({"ok": false, "error": "not_found", "message": "no such item"}),
            ),
        ],
    );
    let mut real = Real {
        places,
        bus: None,
        device_id: "d-42".into(),
        build: "812+460a1a1aa2b3".into(),
        wifi: Wifi::default(),
        ring: Ring::default(),
        texts: Texts::default(),
        reports: None,
        skills: None,
        changes: None,
        photos: None,
        faces: None,
        schedule: Some(lamp_actions::schedule::Adapter::new(socket.clone()).unwrap()),
        update_page: false,
        update_checks: Default::default(),
        restart: None,
        guard: None,
        make_skill: None,
        shutdown: None,
    };
    let me = Identity {
        device: "lamp".into(),
        version: "812+460a1a1aa2b3".into(),
        id: "d-42".into(),
        mac: "lamp-7f72".into(),
        ..Identity::default()
    };
    let replied = |real: &mut Real, kind: &str, data: Value| -> Value {
        let result = answer(real, kind, &data);
        let sent = me.data("2026-10-20T13:00:00Z", kind, &result).to_string();
        serde_json::from_str(&sent).unwrap()
    };
    let listed = replied(&mut real, "schedule.list", json!({}));
    assert_eq!(listed["status"], "success", "{listed}");
    let page = &listed["data"];
    let first = &page["items"][0];
    for (within, path, kind_of_value) in [
        (first, "id", Type::Whole),
        (first, "kind", Type::Text),
        (first, "text", Type::Text),
        (first, "state", Type::Text),
        (first, "local", Type::Text),
        (first, "due_unix_ms", Type::Whole),
        (first, "snoozed", Type::Flag),
        (first, "late", Type::Flag),
        (page, "complete", Type::Flag),
    ] {
        let value = field(within, path);
        assert!(
            value.is_some_and(|value| is(value, kind_of_value)),
            "schedule.list {path}: the app takes {kind_of_value:?}, lamp-cloud said {value:?}"
        );
    }
    assert_eq!(page["items"][0]["text"], "stretch");
    assert!(page["next_after_id"].is_null(), "{page}");
    let cancelled = replied(&mut real, "schedule.cancel", json!({"id": 1}));
    assert_eq!(cancelled["status"], "success", "{cancelled}");
    assert_eq!(cancelled["data"], json!({"cancelled": true, "id": 1}));
    let refused = replied(&mut real, "schedule.cancel", json!({"id": 1}));
    assert_ne!(refused["status"], "success", "{refused}");
    assert!(
        refused
            .to_string()
            .contains("That reminder or timer is no longer active."),
        "{refused}"
    );
    serving.join().unwrap();
    // A bad id is refused before lamp-schedule is asked.
    assert!(answer(&mut real, "schedule.cancel", &json!({"id": 0})).is_err());
    assert!(answer(&mut real, "schedule.list", &json!({"after_id": "x"})).is_err());
    // Without the switch, neither kind is here.
    real.schedule = None;
    for kind in ["schedule.list", "schedule.cancel"] {
        assert!(
            answer(&mut real, kind, &json!({"id": 1})).is_err(),
            "{kind}"
        );
    }
    let _ = std::fs::remove_dir_all(root);
}

/// #2194: `update.status` for the Lamp 2 page, from lamp-update's kept
/// `update/status` on the bus, with the field types the app takes; `now`
/// is `unknown` before lamp-update has said anything, and without
/// `LAMP_APP_UPDATE=on` the kind is not here.
#[test]
fn the_software_update_is_answered_from_lamp_updates_status() {
    let root = PathBuf::from(format!("/tmp/lcu-{}", std::process::id()));
    let places = lamp(&root);
    let bus_root = root.join("bus");
    std::fs::create_dir_all(&bus_root).unwrap();
    let mut real = Real {
        places,
        bus: Some(Bus::new(&bus_root).unwrap()),
        device_id: "d-42".into(),
        build: "812+460a1a1aa2b3".into(),
        wifi: Wifi::default(),
        ring: Ring::default(),
        texts: Texts::default(),
        reports: None,
        skills: None,
        changes: None,
        photos: None,
        faces: None,
        schedule: None,
        update_page: true,
        update_checks: Default::default(),
        restart: None,
        guard: None,
        make_skill: None,
        shutdown: None,
    };
    let me = Identity {
        device: "lamp".into(),
        version: "812+460a1a1aa2b3".into(),
        id: "d-42".into(),
        mac: "lamp-7f72".into(),
        ..Identity::default()
    };
    let replied = |real: &mut Real| -> Value {
        let result = answer(real, "update.status", &json!({}));
        let sent = me
            .data("2026-10-20T17:00:00Z", "update.status", &result)
            .to_string();
        serde_json::from_str(&sent).unwrap()
    };
    let before = replied(&mut real);
    assert_eq!(before["status"], "success", "{before}");
    assert_eq!(before["data"]["now"], "unknown");
    assert!(before["data"]["running"].is_null() && before["data"]["last"].is_null());

    let update = Bus::new(&bus_root).unwrap();
    let mut says = update
        .publisher_declared(
            lamp_msgs::UpdateStatus::TOPIC,
            &lamp_msgs::UpdateStatus::declaration(),
            "update",
        )
        .unwrap();
    says.publish_json(
        0,
        &lamp_msgs::UpdateStatus {
            running: Some(lamp_msgs::UpdateBuild {
                sequence: 3309,
                commit: "4a39f0412".into(),
            }),
            now: "fetching".into(),
            last: Some(lamp_msgs::UpdateResult {
                outcome: "up-to-date".into(),
                sequence: Some(3309),
                at_unix_ms: 1_760_950_000_000,
                reason: None,
                started_at_unix_ms: None,
            }),
        },
    )
    .unwrap();
    let told = replied(&mut real);
    assert_eq!(told["status"], "success", "{told}");
    let data = &told["data"];
    for (path, kind_of_value) in [
        ("running.sequence", Type::Whole),
        ("running.commit", Type::Text),
        ("now", Type::Text),
        ("last.outcome", Type::Text),
        ("last.sequence", Type::Whole),
        ("last.at_unix_ms", Type::Whole),
        ("stale", Type::Flag),
    ] {
        let value = field(data, path);
        assert!(
            value.is_some_and(|value| is(value, kind_of_value)),
            "update.status {path}: the app takes {kind_of_value:?}, lamp-cloud said {value:?}"
        );
    }
    assert_eq!(data["now"], "fetching");
    assert_eq!(data["running"]["sequence"], 3309);

    // #4153: the app's Check for update, in the reply the app reads. While
    // the round runs it is refused with the reason the app shows.
    let asked = |real: &mut Real| -> Value {
        let result = answer(real, "update.check", &json!({}));
        let sent = me
            .data("2026-10-20T17:00:00Z", "update.check", &result)
            .to_string();
        serde_json::from_str(&sent).unwrap()
    };
    let running = asked(&mut real);
    assert_eq!(running["status"], "failure", "{running}");
    assert_eq!(running["error"], lamp_cloud::update::RUNNING);
    let idle = lamp_msgs::UpdateStatus {
        running: None,
        now: "idle".into(),
        last: None,
    };
    says.publish_json(0, &idle).unwrap();
    // Between rounds with no lamp-update reading checks: said, not counted.
    let nobody = asked(&mut real);
    assert_eq!(
        nobody["error"],
        lamp_cloud::update::NOT_LISTENING,
        "{nobody}"
    );
    let mut serve = update
        .subscribe_typed(
            lamp_msgs::UpdateCheck::TOPIC,
            "update",
            lamp_msgs::UpdateCheck::TYPE,
        )
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(250));
    let started = asked(&mut real);
    assert_eq!(started["status"], "success", "{started}");
    assert_eq!(started["data"]["check"], "started");
    assert_eq!(started["data"]["now"], "idle");
    assert!(
        serve
            .recv(std::time::Duration::from_secs(1))
            .unwrap()
            .is_some()
    );
    let again = asked(&mut real);
    assert_eq!(again["error"], lamp_cloud::update::TOO_SOON, "{again}");

    // Without the switch neither kind is here.
    real.update_page = false;
    assert!(answer(&mut real, "update.status", &json!({})).is_err());
    assert!(answer(&mut real, "update.check", &json!({})).is_err());
    let _ = std::fs::remove_dir_all(root);
}

/// A stand-in for setup's change helper on its socket: each request
/// answered by the helper's own [`lamp_setup::change::power`] against a
/// lamp-update lock in `root`, held by an update or not, in turn; the lines
/// it was sent are handed back. Nothing is acted on.
fn helper(
    root: &std::path::Path,
    socket: &std::path::Path,
    installing: Vec<bool>,
) -> std::thread::JoinHandle<Vec<String>> {
    let listener = UnixListener::bind(socket).unwrap();
    let records = lamp_update::records::Records::new(root.join("update"));
    std::thread::spawn(move || {
        let mut asked = Vec::new();
        for installing in installing {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let update = installing.then(|| records.lock().unwrap());
            let (_, reply, _) = lamp_setup::change::power(&line, true, &records).unwrap();
            drop(update);
            stream.write_all(format!("{reply}\n").as_bytes()).unwrap();
            asked.push(line.trim().to_owned());
        }
        asked
    })
}

/// `system.reboot` (#2193, Mobile's restart button): lamp-cloud asks
/// setup's change helper and answers with what it said, before the lamp
/// goes down; under an update it is refused in plain words. Without the
/// switch it is a kind this lamp lacks.
#[test]
fn a_restart_from_the_app_is_the_helpers_answered_first_and_refused_under_an_update() {
    // Not ring.rs's `/tmp/lcr-`: since #1806 both run in one process, and
    // each removes its folder first.
    let root = PathBuf::from(format!("/tmp/lcr-restart-{}", std::process::id()));
    let places = lamp(&root);
    let socket = root.join("change.sock");
    let serving = helper(&root, &socket, vec![false, true]);
    let mut real = Real {
        places,
        bus: None,
        device_id: "d-42".into(),
        build: "812+460a1a1aa2b3".into(),
        wifi: Wifi::default(),
        ring: Ring::default(),
        texts: Texts::default(),
        reports: None,
        skills: None,
        changes: None,
        photos: None,
        faces: None,
        schedule: None,
        update_page: false,
        update_checks: Default::default(),
        restart: Some(socket),
        guard: None,
        make_skill: None,
        shutdown: None,
    };
    let me = Identity {
        device: "lamp".into(),
        id: "d-42".into(),
        ..Identity::default()
    };
    let said = |real: &mut Real| {
        let result = answer(real, "system.reboot", &Value::Null);
        let sent = me
            .data("2026-10-20T10:00:00Z", "system.reboot", &result)
            .to_string();
        serde_json::from_str::<Value>(&sent).unwrap()
    };
    let reply = said(&mut real);
    assert_eq!(reply["status"], "success", "{reply}");
    assert_eq!(reply["data"]["restarting"], true, "{reply}");
    let reply = said(&mut real);
    assert_eq!(reply["status"], "failure", "{reply}");
    assert!(
        reply.to_string().contains(lamp_keeper::restart::INSTALLING),
        "{reply}"
    );
    assert_eq!(
        serving.join().unwrap(),
        [lamp_keeper::restart::REQUEST, lamp_keeper::restart::REQUEST]
    );
    // Off: no helper asked.
    real.restart = None;
    let reply = said(&mut real);
    assert_eq!(reply["status"], "failure", "{reply}");
    assert!(
        reply
            .to_string()
            .contains("not on this lamp: system.reboot")
    );
    let _ = std::fs::remove_dir_all(root);
}

/// `system.shutdown` (#3697, Mobile's off button on #3698): off, a kind
/// this lamp lacks and no helper asked; on, setup's change helper's
/// answer, refused under an update; once taken, asked again (any data) it
/// is answered the same and the helper is asked no more, so the lamp is
/// turned off once.
#[test]
fn a_shutdown_from_the_app_is_the_helpers_answered_once_and_refused_under_an_update() {
    let root = PathBuf::from(format!("/tmp/lcr-shutdown-{}", std::process::id()));
    let places = lamp(&root);
    let socket = root.join("change.sock");
    let serving = helper(&root, &socket, vec![true, false, true]);
    let mut real = Real {
        places,
        bus: None,
        device_id: "d-42".into(),
        build: "812+460a1a1aa2b3".into(),
        wifi: Wifi::default(),
        ring: Ring::default(),
        texts: Texts::default(),
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
        shutdown: None,
    };
    let me = Identity {
        device: "lamp".into(),
        id: "d-42".into(),
        ..Identity::default()
    };
    let said = |real: &mut Real, data: &Value| {
        let result = answer(real, "system.shutdown", data);
        let sent = me
            .data("2026-10-27T10:00:00Z", "system.shutdown", &result)
            .to_string();
        serde_json::from_str::<Value>(&sent).unwrap()
    };
    // Off: not here, and no helper asked.
    let reply = said(&mut real, &Value::Null);
    assert_eq!(reply["status"], "failure", "{reply}");
    assert!(
        reply
            .to_string()
            .contains("not on this lamp: system.shutdown"),
        "{reply}"
    );
    real.shutdown = Some(Shutdown {
        socket,
        accepted: None,
    });
    let reply = said(&mut real, &Value::Null);
    assert_eq!(reply["status"], "failure", "{reply}");
    assert!(
        reply.to_string().contains(lamp_keeper::restart::INSTALLING),
        "{reply}"
    );
    let reply = said(&mut real, &Value::Null);
    assert_eq!(reply["status"], "success", "{reply}");
    assert_eq!(reply["data"]["shutting_down"], true, "{reply}");
    for data in [Value::Null, json!({"now": "please"}), json!("junk")] {
        assert_eq!(said(&mut real, &data)["data"], reply["data"], "{data}");
    }
    // A lamp still on once the yes has lapsed (YES_KEPT) wasn't turned off:
    // the runner refused, as an update took the lock first. The helper is
    // asked again, and the app hears why, not the old `success` (the
    // Reviewer-lead's P3 on #3735).
    // Lapsed by setting its moment to now, with no clock arithmetic: a
    // freshly booted host's monotonic clock may be under YES_KEPT (the
    // Reviewer-lead's P3 on #4267).
    real.shutdown.as_mut().unwrap().accepted.as_mut().unwrap().1 = std::time::Instant::now();
    let reply = said(&mut real, &Value::Null);
    assert_eq!(reply["status"], "failure", "{reply}");
    assert!(
        reply.to_string().contains(lamp_keeper::restart::INSTALLING),
        "{reply}"
    );
    assert_eq!(
        serving.join().unwrap(),
        [lamp_keeper::restart::SHUTDOWN_REQUEST; 3]
    );
    let _ = std::fs::remove_dir_all(root);
}

/// `mic.get`'s `muted_by` (#4332): who or what muted the lamp, as the voice
/// says it on `voice/mute`, so the app's setup screen can tell the restart
/// guard's mute after a fault from one the person chose. Only while muted;
/// absent when it listens, and from a voice that does not say it, but a
/// sleep's reason still reads as `sleep`.
#[test]
fn mic_get_says_who_muted_the_lamp_only_while_muted() {
    use lamp_msgs::{MuteReason, MutedBy, VoiceMute};
    let root = PathBuf::from(format!("/tmp/lcm-{}", std::process::id()));
    let places = lamp(&root);
    let bus_root = root.join("bus");
    std::fs::create_dir_all(&bus_root).unwrap();
    std::fs::create_dir_all(places.proc.join("sys/kernel/random")).unwrap();
    std::fs::write(places.proc.join("sys/kernel/random/boot_id"), "boot\n").unwrap();
    let voice = Bus::new(&bus_root).unwrap();
    let mut says = voice
        .publisher_declared(VoiceMute::TOPIC, &VoiceMute::declaration(), "voice")
        .unwrap();
    let mut real = Real {
        places,
        bus: Some(Bus::new(&bus_root).unwrap()),
        device_id: "d-42".into(),
        build: "812+460a1a1aa2b3".into(),
        wifi: Wifi::default(),
        ring: Ring::default(),
        texts: Texts::default(),
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
        shutdown: None,
    };
    let me = Identity {
        device: "lamp".into(),
        version: "812+460a1a1aa2b3".into(),
        id: "d-42".into(),
        mac: "lamp-7f72".into(),
        ..Identity::default()
    };
    let mut at = 0;
    let mut mic = |real: &mut Real, said: &Value| -> Value {
        at += 1;
        says.publish_json(at, said).unwrap();
        let result = answer(real, "mic.get", &Value::Null);
        let sent = me
            .data("2026-10-30T10:00:00Z", "mic.get", &result)
            .to_string();
        let reply: Value = serde_json::from_str(&sent).unwrap();
        assert_eq!(reply["status"], "success", "{reply}");
        reply["data"].clone()
    };
    for (reason, by, word) in [
        (Some(MuteReason::Sleep), MutedBy::Sleep, "sleep"),
        (None, MutedBy::Button, "button"),
        (None, MutedBy::RestartGuard, "restart_guard"),
        (None, MutedBy::Unreadable, "unreadable"),
    ] {
        let said = VoiceMute {
            muted: true,
            reason,
            muted_by: Some(by),
        };
        let data = mic(&mut real, &serde_json::to_value(said).unwrap());
        assert_eq!(data["muted"], true, "{data}");
        assert_eq!(data["muted_by"], word, "{data}");
    }
    // Listening: no `muted_by` at all.
    let data = mic(&mut real, &json!({"muted": false}));
    assert_eq!(data["muted"], false, "{data}");
    assert!(data.get("muted_by").is_none(), "{data}");
    // A voice before the field: a plain mute says none, a sleep says sleep.
    let data = mic(&mut real, &json!({"muted": true}));
    assert_eq!(data["muted"], true, "{data}");
    assert!(data.get("muted_by").is_none(), "{data}");
    let data = mic(&mut real, &json!({"muted": true, "reason": "sleep"}));
    assert_eq!(data["muted_by"], "sleep", "{data}");
    let _ = std::fs::remove_dir_all(root);
}

/// `LAMP_SIGNED_OUT`: `system.info` and `lamp.health` read the state from
/// `account/state`. No state at all (the switch off, an older lamp-cloud) and
/// a state of signed in say no field.
#[test]
fn system_info_and_health_say_signed_out_only_while_the_bus_says_so() {
    use lamp_msgs::{AccountState, Health};
    let root = tempfile::tempdir().unwrap();
    let places = lamp(&root.path().to_path_buf());
    let bus_root = root.path().join("bus");
    let bus = Bus::new(&bus_root).unwrap();
    let mut health = bus
        .publisher_declared(Health::TOPIC, &Health::declaration(), "health")
        .unwrap();
    health
        .publish_json(
            1,
            &json!({"level":"ok","message":"","items":[],"missing":[]}),
        )
        .unwrap();
    let mut real = Real {
        places,
        bus: Some(Bus::new(&bus_root).unwrap()),
        device_id: "d-42".into(),
        build: "812+460a1a1aa2b3".into(),
        wifi: Wifi::default(),
        ring: Ring::default(),
        texts: Texts::default(),
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
        shutdown: None,
    };
    let said = |real: &mut Real, kind: &str| answer(real, kind, &Value::Null).unwrap().unwrap();
    assert!(said(&mut real, "system.info").get("account").is_none());
    assert!(said(&mut real, "lamp.health").get("account").is_none());
    let mut state = bus
        .publisher_declared(AccountState::TOPIC, &AccountState::declaration(), "cloud")
        .unwrap();
    state
        .publish_json(lamp_ipc::monotonic_us(), &AccountState::default())
        .unwrap();
    assert!(said(&mut real, "system.info").get("account").is_none());
    state
        .publish_json(
            lamp_ipc::monotonic_us(),
            &AccountState {
                signed_out: true,
                for_s: 3,
            },
        )
        .unwrap();
    assert_eq!(said(&mut real, "system.info")["account"], "signed_out");
    assert_eq!(said(&mut real, "lamp.health")["account"], "signed_out");
}
