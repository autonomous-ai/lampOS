//! The executive as the voice meets it: a directory, a list, a socket.
use lamp_actions::{
    Executive,
    light::{BusLight, Light, STATE},
    server::{
        CALL_SOCKET, CATALOG_FILE, READY_FILE, serve, serve_with_clock, serve_with_clock_and_caps,
    },
    sound::{Mixer, Volume},
};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// A volume control the test can read while the executive holds it.
struct Shared(Arc<AtomicU8>);
impl Mixer for Shared {
    fn get(&mut self) -> std::io::Result<u8> {
        Ok(self.0.load(Ordering::Acquire))
    }
    fn set(&mut self, level: u8) -> std::io::Result<()> {
        self.0.store(level, Ordering::Release);
        Ok(())
    }
}
/// No ring owner at all.
struct NoLight;
impl Light for NoLight {
    fn there(&mut self) -> bool {
        false
    }
    fn shown(&mut self) -> Option<lamp_msgs::Light> {
        None
    }
    fn ask(&mut self, _: &lamp_msgs::Light) -> bool {
        false
    }
}

fn directory(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "lamp-actions-{name}-{}-{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}
fn until(what: &str, mut ready: impl FnMut() -> bool) {
    let started = Instant::now();
    while !ready() {
        assert!(started.elapsed() < Duration::from_secs(5), "never: {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn ask(home: &Path, line: &[u8]) -> Value {
    let mut stream = UnixStream::connect(home.join(CALL_SOCKET)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream.write_all(line).unwrap();
    let mut answer = String::new();
    BufReader::new(stream).read_line(&mut answer).unwrap();
    serde_json::from_str(&answer).unwrap()
}
fn names(home: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(home.join(CATALOG_FILE)).unwrap_or_default();
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|file| {
            Some(
                file["tools"]
                    .as_array()?
                    .iter()
                    .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
                    .collect(),
            )
        })
        .unwrap_or_default()
}

/// The whole of what the voice does with it: reads the list, asks on the
/// socket in one line, gets one line back; and what it is told is what
/// happened to the control.
#[test]
fn the_list_is_there_a_call_changes_the_control_and_anything_else_is_refused_in_words() {
    let home = directory("calls");
    let level = Arc::new(AtomicU8::new(50));
    let stop = Arc::new(AtomicBool::new(false));
    let serving = {
        let (home, level, stop) = (home.clone(), level.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut executive = Executive {
                sound: Some(Volume::new(Shared(level), 100)),
                light: None::<NoLight>,
                short: false,
            };
            serve(&home, &mut executive, &stop)
        })
    };
    until("ready", || home.join(READY_FILE).exists());
    assert_eq!(
        names(&home),
        ["volume_up", "volume_down", "volume_mute", "volume_unmute"]
    );
    assert_eq!(
        ask(
            &home,
            b"{\"op\":\"call\",\"name\":\"volume_up\",\"args\":{}}\n"
        ),
        json!({"ok":true,"result":{"result":"My volume is now 70 out of 100; it was 50."}})
    );
    assert_eq!(level.load(Ordering::Acquire), 70);
    assert_eq!(
        ask(&home, b"{\"op\":\"call\",\"name\":\"volume_down\"}\n")["result"]["result"],
        "My volume is now 50 out of 100; it was 70."
    );
    // Not offered, not a call, not JSON, an argument a function does not
    // take: each gets a sentence, and nothing is changed.
    for refused in [
        b"{\"op\":\"call\",\"name\":\"light_on\",\"args\":{}}\n".as_slice(),
        b"{\"op\":\"call\",\"name\":\"open_the_door\",\"args\":{}}\n".as_slice(),
        b"{\"op\":\"call\",\"name\":\"volume_up\",\"args\":{\"by\":50}}\n".as_slice(),
        b"{\"op\":\"status\"}\n".as_slice(),
        b"hello\n".as_slice(),
    ] {
        assert_eq!(
            ask(&home, refused),
            json!({"ok":false,"error":"I can't do that."}),
            "{}",
            String::from_utf8_lossy(refused)
        );
    }
    // #1396: the app's two calls, answered in numbers on the same socket,
    // and never in the model's list.
    assert_eq!(
        ask(
            &home,
            b"{\"op\":\"call\",\"name\":\"volume_level\",\"args\":{}}\n"
        ),
        json!({"ok":true,"level":{"volume":50,"raw":50,"max_volume":100}})
    );
    assert_eq!(
        ask(
            &home,
            b"{\"op\":\"call\",\"name\":\"volume_to\",\"args\":{\"volume\":35}}\n"
        ),
        json!({"ok":true,"level":{"volume":35,"raw":35,"max_volume":100}})
    );
    assert_eq!(level.load(Ordering::Acquire), 35);
    assert_eq!(
        ask(
            &home,
            b"{\"op\":\"call\",\"name\":\"volume_to\",\"args\":{\"volume\":101}}\n"
        )["ok"],
        false
    );
    assert_eq!(level.load(Ordering::Acquire), 35);
    assert!(
        !names(&home)
            .iter()
            .any(|name| name.starts_with("volume_level") || name == "volume_to")
    );
    assert_eq!(
        ask(
            &home,
            b"{\"op\":\"call\",\"name\":\"volume_to\",\"args\":{\"volume\":50}}\n"
        )["level"]["raw"],
        50
    );
    assert_eq!(level.load(Ordering::Acquire), 50);
    // A caller that connects and says nothing costs the next one a second,
    // not the service.
    let silent = UnixStream::connect(home.join(CALL_SOCKET)).unwrap();
    let started = Instant::now();
    assert_eq!(
        ask(&home, b"{\"op\":\"call\",\"name\":\"volume_mute\"}\n")["result"]["result"],
        "My speaker is muted."
    );
    assert!(started.elapsed() < Duration::from_millis(2_500));
    drop(silent);
    stop.store(true, Ordering::Release);
    serving.join().unwrap().unwrap();
    // Stopped in order, it leaves nothing the voice would read.
    assert!(!home.join(CATALOG_FILE).exists() && !home.join(READY_FILE).exists());
    std::fs::remove_dir_all(home).unwrap();
}

/// `LAMP_SHORT_DONE` (the first builder on #705, 13:4x): with it, a volume
/// change the model asked for is answered with what was established and
/// told to say only a word or two after it, and one at its end with its own
/// words that it already is. A call not done, and the app's numbers, are
/// answered as without it.
#[test]
fn with_the_switch_a_volume_change_is_answered_short_and_nothing_else_changes() {
    let home = directory("short");
    let level = Arc::new(AtomicU8::new(50));
    let stop = Arc::new(AtomicBool::new(false));
    let serving = {
        let (home, level, stop) = (home.clone(), level.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut executive = Executive {
                sound: Some(Volume::new(Shared(level), 100)),
                light: None::<NoLight>,
                short: true,
            };
            serve(&home, &mut executive, &stop)
        })
    };
    until("ready", || home.join(READY_FILE).exists());
    assert_eq!(
        ask(
            &home,
            b"{\"op\":\"call\",\"name\":\"volume_up\",\"args\":{}}\n"
        ),
        json!({"ok":true,"result":{
            "result":"My volume is now 70 out of 100; it was 50.",
            "next":lamp_actions::SHORT_NEXT,
            "say":"Louder."
        }})
    );
    assert_eq!(level.load(Ordering::Acquire), 70);
    // At the ceiling, nothing changes: never answered as if it had been
    // made louder (the reviewer and Third on #1520), but with its own words
    // that it already is, for the voice to say at once (the first builder
    // on #4710, 17:35).
    for _ in 0..2 {
        ask(
            &home,
            b"{\"op\":\"call\",\"name\":\"volume_up\",\"args\":{}}\n",
        );
    }
    assert_eq!(level.load(Ordering::Acquire), 100);
    assert_eq!(
        ask(
            &home,
            b"{\"op\":\"call\",\"name\":\"volume_up\",\"args\":{}}\n"
        ),
        json!({"ok":true,"result":{
            "result":"My speaker is already as loud as it is allowed to go.",
            "already":true,
            "next":lamp_actions::SHORT_ALREADY_NEXT,
            "say":"I'm already as loud as I go."
        }})
    );
    // Not done: the sentence alone, for the model to say.
    assert_eq!(
        ask(
            &home,
            b"{\"op\":\"call\",\"name\":\"light_on\",\"args\":{}}\n"
        ),
        json!({"ok":false,"error":"I can't do that."})
    );
    assert_eq!(
        ask(
            &home,
            b"{\"op\":\"call\",\"name\":\"volume_level\",\"args\":{}}\n"
        ),
        json!({"ok":true,"level":{"volume":100,"raw":100,"max_volume":100}})
    );
    stop.store(true, Ordering::Release);
    serving.join().unwrap().unwrap();
    std::fs::remove_dir_all(home).unwrap();
}

/// Whether what the ring's owner last said still holds by the bus's clock.
/// Expired here, it is past what the executive allows it as well.
fn owner_says(bus: &lamp_bus::Bus) -> bool {
    bus.latest(STATE)
        .unwrap()
        .is_some_and(|state| !state.expired)
}

/// The light's functions are offered while the ring's owner says what it
/// shows, and for a grace after it stops (`LIGHT_GRACE`, here 300 ms). Here
/// it starts after the executive, shows what it is asked, and then goes:
/// the list follows it both ways.
#[test]
fn the_light_is_offered_when_its_owner_appears_and_withdrawn_when_it_goes() {
    let home = directory("light");
    let root = directory("light-bus");
    let stop = Arc::new(AtomicBool::new(false));
    // The executive's periods on a clock the test moves on: the list is
    // looked at again once the owner's change is on the bus and the test
    // has moved the clock by two seconds, not after two seconds of waiting.
    // Two seconds is the period docs/actions.md gives, so an executive that
    // looked again less often would fail here.
    let base = Instant::now();
    let ahead_ms = Arc::new(AtomicU64::new(0));
    let serving = {
        let (home, root, stop) = (home.clone(), root.clone(), stop.clone());
        let ahead_ms = ahead_ms.clone();
        std::thread::spawn(move || {
            // A short grace on the wall clock, which the test does not move:
            // what the owner said expires 600 ms after it stops, longer.
            let bus = lamp_bus::Bus::new(&root).unwrap();
            let mut executive = Executive {
                sound: None::<Volume<Shared>>,
                light: Some(BusLight::graced(bus, Duration::from_millis(300)).unwrap()),
                short: false,
            };
            let now = move || base + Duration::from_millis(ahead_ms.load(Ordering::Acquire));
            serve_with_clock(&home, &mut executive, &stop, None, None, &now)
        })
    };
    let look_again = || {
        ahead_ms.fetch_add(2_000, Ordering::AcqRel);
    };
    let bus = lamp_bus::Bus::new(&root).unwrap();
    until("ready", || home.join(READY_FILE).exists());
    assert!(names(&home).is_empty());
    assert_eq!(
        ask(&home, b"{\"op\":\"call\",\"name\":\"light_on\"}\n"),
        json!({"ok":false,"error":"I can't do that."})
    );

    // The ring's owner: says what it shows once a second, and shows what
    // it is asked for.
    let owner_stop = Arc::new(AtomicBool::new(false));
    let owner = {
        let (root, owner_stop) = (root.clone(), owner_stop.clone());
        std::thread::spawn(move || {
            let bus = lamp_bus::Bus::new(&root).unwrap();
            let mut state = bus
                .publisher_declared(STATE, &lamp_msgs::Light::declaration(), "ring")
                .unwrap();
            state.expires_after(Some(Duration::from_millis(600)));
            // A lamp that was left off, purple, at its third level.
            let mut shown = lamp_msgs::Light::new(false, "purple", 3).unwrap();
            let mut tick = 0u64;
            while !owner_stop.load(Ordering::Acquire) {
                if let Ok(Some(wanted)) = bus.latest(lamp_actions::light::WANTED)
                    && let Ok(wanted) = wanted.parse::<lamp_msgs::Light>()
                {
                    shown = wanted;
                }
                tick += 1;
                state.publish_json(tick, &shown).unwrap();
                std::thread::sleep(Duration::from_millis(20));
            }
        })
    };
    until("the owner says what it shows", || owner_says(&bus));
    look_again();
    until("the light is offered", || names(&home).len() == 7);
    assert_eq!(
        ask(&home, b"{\"op\":\"call\",\"name\":\"light_on\"}\n"),
        json!({"ok":true,"result":{"result":"My light is on, purple."}})
    );
    assert_eq!(
        ask(
            &home,
            b"{\"op\":\"call\",\"name\":\"light_colour\",\"args\":{\"colour\":\"blue\"}}\n"
        )["result"]["result"],
        "My light is blue now."
    );
    // Warmer, through the socket as the voice asks it: blue is off the
    // warmth scale, so it becomes warm, said as what it is now (a warm
    // white); then a step cooler, white, said from and to (the first
    // builder, 17 Oct 14:16, and #1367's scene).
    assert_eq!(
        ask(&home, b"{\"op\":\"call\",\"name\":\"light_warmer\"}\n")["result"]["result"],
        "My light is warm white now."
    );
    assert_eq!(
        ask(&home, b"{\"op\":\"call\",\"name\":\"light_cooler\"}\n")["result"]["result"],
        "My light is a step cooler, from warm white to white."
    );
    assert_eq!(
        ask(
            &home,
            b"{\"op\":\"call\",\"name\":\"light_colour\",\"args\":{\"colour\":\"blue\"}}\n"
        )["result"]["result"],
        "My light is blue now."
    );
    // One word out of ten, and nothing else.
    for refused in [
        b"{\"op\":\"call\",\"name\":\"light_colour\",\"args\":{\"colour\":\"beige\"}}\n".as_slice(),
        b"{\"op\":\"call\",\"name\":\"light_colour\",\"args\":{\"colour\":\"blue\",\"rgb\":[1,2,3]}}\n".as_slice(),
        b"{\"op\":\"call\",\"name\":\"light_colour\"}\n".as_slice(),
    ] {
        assert_eq!(
            ask(&home, refused)["error"],
            "That is not a colour my light can show."
        );
    }
    assert_eq!(
        ask(&home, b"{\"op\":\"call\",\"name\":\"light_dimmer\"}\n")["result"]["result"],
        "My light is a step dimmer, at level 2 of 5."
    );
    // What is on the bus is the light by name, as the ring's owner reads it.
    let asked: lamp_msgs::Light = lamp_bus::Bus::new(&root)
        .unwrap()
        .latest(lamp_actions::light::WANTED)
        .unwrap()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(asked, lamp_msgs::Light::new(true, "blue", 2).unwrap());
    // The owner goes. What it last said goes stale, and after the grace
    // (`LIGHT_GRACE`, here 300 ms) the list empties.
    owner_stop.store(true, Ordering::Release);
    owner.join().unwrap();
    until("what the owner said has expired", || !owner_says(&bus));
    look_again();
    until("the light is withdrawn", || names(&home).is_empty());
    assert_eq!(
        ask(&home, b"{\"op\":\"call\",\"name\":\"light_off\"}\n"),
        json!({"ok":false,"error":"I can't do that."})
    );
    stop.store(true, Ordering::Release);
    serving.join().unwrap().unwrap();
    std::fs::remove_dir_all(home).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

/// A second executive on the same directory is refused while the first
/// answers, and takes over a socket nobody answers on.
#[test]
fn one_executive_to_a_directory() {
    let home = directory("one");
    let stop = Arc::new(AtomicBool::new(false));
    let serving = {
        let (home, stop) = (home.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut executive = Executive {
                sound: None::<Volume<Shared>>,
                light: None::<NoLight>,
                short: false,
            };
            serve(&home, &mut executive, &stop)
        })
    };
    until("ready", || home.join(READY_FILE).exists());
    let mut second = Executive {
        sound: None::<Volume<Shared>>,
        light: None::<NoLight>,
        short: false,
    };
    let refused = serve(&home, &mut second, &AtomicBool::new(true)).unwrap_err();
    assert_eq!(refused.kind(), std::io::ErrorKind::AddrInUse);
    stop.store(true, Ordering::Release);
    serving.join().unwrap().unwrap();
    // A socket file left by a process that died: taken over.
    drop(std::os::unix::net::UnixListener::bind(home.join(CALL_SOCKET)).unwrap());
    serve(&home, &mut second, &AtomicBool::new(true)).unwrap();
    std::fs::remove_dir_all(home).unwrap();
}

/// The part says it is well, with what it can do now and how many calls it
/// answered, for the lamp's health.
#[test]
fn its_report_says_what_it_can_do_and_how_much_it_did() {
    use lamp_actions::server::report;
    let none = report(&[], 0);
    assert_eq!(none.level, lamp_msgs::Level::Ok);
    let value = |d: &lamp_msgs::Diagnostic, key: &str| {
        d.values
            .iter()
            .find(|fact| fact.key == key)
            .map(|fact| fact.value.clone())
    };
    assert_eq!(value(&none, "can_do").as_deref(), Some("nothing now"));
    assert_eq!(value(&none, "answered").as_deref(), Some("0"));
    assert_eq!(none.name, "actions");
}

/// A caller is taken as soon as it connects, not at the end of the server's
/// wait for one (`IDLE`): calls a moment apart, as the voice makes them, are
/// each answered well inside it (#1806; a sleep there kept each call waiting
/// up to 50 ms). While nobody calls, the server still wakes for its health
/// report and its stop flag.
#[test]
fn a_caller_is_answered_at_once_and_an_idle_server_still_reports_and_stops() {
    use lamp_actions::server::{IDLE, REPORT_EVERY};
    let home = directory("idle");
    // A short root: a Unix socket's address holds 103 bytes.
    let root = PathBuf::from(format!("/tmp/la-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let bus = lamp_bus::Bus::new(&root).unwrap();
    let mut heard = bus
        .subscribe_typed(
            lamp_msgs::Diagnostic::TOPIC,
            "test",
            lamp_msgs::Diagnostic::TYPE,
        )
        .unwrap();
    let health = bus
        .publisher_declared(
            lamp_msgs::Diagnostic::TOPIC,
            &lamp_msgs::Diagnostic::declaration(),
            "actions",
        )
        .unwrap();
    let level = Arc::new(AtomicU8::new(50));
    let stop = Arc::new(AtomicBool::new(false));
    // The report's period on a clock the test moves on.
    let base = Instant::now();
    let ahead_ms = Arc::new(AtomicU64::new(0));
    let serving = {
        let (home, level, stop) = (home.clone(), level.clone(), stop.clone());
        let ahead_ms = ahead_ms.clone();
        std::thread::spawn(move || {
            let mut executive = Executive {
                sound: Some(Volume::new(Shared(level), 100)),
                light: None::<NoLight>,
                short: false,
            };
            let now = move || base + Duration::from_millis(ahead_ms.load(Ordering::Acquire));
            serve_with_clock(&home, &mut executive, &stop, Some(health), None, &now)
        })
    };
    until("ready", || home.join(READY_FILE).exists());
    let report = |heard: &mut lamp_bus::Subscriber| -> lamp_msgs::Diagnostic {
        heard
            .recv(Duration::from_secs(5))
            .unwrap()
            .expect("a health report")
            .parse()
            .unwrap()
    };
    let answered = |report: &lamp_msgs::Diagnostic| {
        report
            .values
            .iter()
            .find(|fact| fact.key == "answered")
            .map(|fact| fact.value.clone())
    };
    assert_eq!(answered(&report(&mut heard)).as_deref(), Some("0"));

    // Ten calls, each a moment after the last answer, while the server
    // waits for its next caller.
    let mut took = Vec::new();
    for call in 0..10 {
        std::thread::sleep(Duration::from_millis(5));
        let name = if call % 2 == 0 {
            "volume_up"
        } else {
            "volume_down"
        };
        let started = Instant::now();
        let answer = ask(
            &home,
            format!("{{\"op\":\"call\",\"name\":\"{name}\"}}\n").as_bytes(),
        );
        took.push(started.elapsed());
        assert_eq!(answer["ok"], true, "{answer}");
    }
    took.sort();
    let middle = took[took.len() / 2];
    assert!(
        middle < IDLE / 2,
        "the middle call took {middle:?}, against IDLE {IDLE:?}: {took:?}"
    );

    // Nobody calls. A period on, it says it is well again, with the ten.
    ahead_ms.fetch_add(REPORT_EVERY.as_millis() as u64, Ordering::AcqRel);
    assert_eq!(answered(&report(&mut heard)).as_deref(), Some("10"));

    // And it stops within a few of its waits.
    let stopping = Instant::now();
    stop.store(true, Ordering::Release);
    until("the server stops", || serving.is_finished());
    assert!(
        stopping.elapsed() < Duration::from_secs(1),
        "{:?}",
        stopping.elapsed()
    );
    serving.join().unwrap().unwrap();
    std::fs::remove_dir_all(home).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

/// The caps' store is from a later build (`LAMP_OUTPUT_CAPS`; the first
/// builder and Jarvis, 21 Oct, for "Updates cover the whole lamp, V3 to
/// V5"): the server still starts and answers, the caps are off with an empty
/// snapshot for the ring (Jarvis's P2 on #2425), a cap request is refused as
/// `held_back`, and the part's health is a warning that says why.
#[test]
fn caps_held_back_leave_the_server_serving_with_a_warning() {
    use lamp_actions::caps_service::{Config, Moment, Service};
    let home = directory("held-back");
    // A short root: a Unix socket's address holds 103 bytes.
    let root = PathBuf::from(format!("/tmp/lah-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let config = Config {
        directory: root.join("caps"),
        bus_root: root.join("bus"),
    };
    // A store a later build wrote, in the caps' own folder.
    drop(lamp_actions::caps::Controller::open(&config.directory, Some(1_800_000_000_000)).unwrap());
    lamp_schedule::store::Store::open(&config.directory)
        .unwrap()
        .commit(b"{\"schema\":2}")
        .unwrap();
    let service = Service::with_clock(&config, "held-fixture", || Moment {
        at_us: 1_000_000,
        unix_ms: Some(1_800_000_000_000),
    })
    .unwrap();
    let why = service.held_back().unwrap().to_owned();
    let bus = lamp_bus::Bus::new(&config.bus_root).unwrap();
    let mut heard = bus
        .subscribe_typed(
            lamp_msgs::Diagnostic::TOPIC,
            "test",
            lamp_msgs::Diagnostic::TYPE,
        )
        .unwrap();
    let health = bus
        .publisher_declared(
            lamp_msgs::Diagnostic::TOPIC,
            &lamp_msgs::Diagnostic::declaration(),
            "actions",
        )
        .unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let serving = {
        let (home, stop) = (home.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut executive = Executive {
                sound: Some(Volume::new(Shared(Arc::new(AtomicU8::new(50))), 100)),
                light: None::<NoLight>,
                short: false,
            };
            serve_with_clock_and_caps(
                &home,
                &mut executive,
                &stop,
                Some(health),
                None,
                &Instant::now,
                Some(service),
            )
        })
    };
    until("ready", || home.join(READY_FILE).exists());
    let report: lamp_msgs::Diagnostic = heard
        .recv(Duration::from_secs(5))
        .unwrap()
        .expect("a health report")
        .parse()
        .unwrap();
    assert_eq!(
        (report.level, report.message.as_str()),
        (lamp_msgs::Level::Warn, why.as_str())
    );
    let answer = ask(&home, b"{\"op\":\"call\",\"name\":\"volume_up\"}\n");
    assert_eq!(answer["ok"], true, "{answer}");
    let refused = ask(&home, b"{\"op\":\"caps_get\"}\n");
    assert_eq!(refused["code"], "held_back", "{refused}");
    let snapshot: lamp_msgs::OutputCaps = bus
        .latest(lamp_msgs::OutputCaps::TOPIC)
        .unwrap()
        .expect("an empty snapshot for the ring")
        .parse()
        .unwrap();
    assert!(snapshot.entries.is_empty(), "{snapshot:?}");
    stop.store(true, Ordering::Release);
    serving.join().unwrap().unwrap();
    let _ = std::fs::remove_dir_all(&root);
}
