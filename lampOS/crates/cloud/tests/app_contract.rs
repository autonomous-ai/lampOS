//! The fields of `lamp-cloud`'s replies that the Autonomous app's device page
//! reads, by name and type (Fourth on #1396, 09:44 and 10:10). The app reads each one
//! leniently, so a renamed or retyped field would go blank on its page with
//! nothing failing there: it fails here instead. Each reply is `lamp-cloud`'s
//! own answer from the real lamp (`Real`, on a lamp of files, a stand-in
//! `lamp-actions` and a bus with the ring's light on it), in V1's envelope,
//! through JSON as the broker carries it.
use lamp_bus::Bus;
use lamp_cloud::answer::answer;
use lamp_cloud::lamp::{Places, Real, Ring, Wifi};
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
