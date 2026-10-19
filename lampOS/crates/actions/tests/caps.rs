//! Real Actions socket with private storage, bus and deterministic cap clocks.
use lamp_actions::{
    Executive, caps,
    caps_service::{Config, Moment, Service},
    light::Light,
    server::{self, CALL_SOCKET, CATALOG_FILE, READY_FILE},
    sound::{Mixer, Volume},
};
use lamp_msgs::OutputCaps;
use serde_json::{Value, json};
use std::{
    io::{self, BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const WALL: u64 = 1_800_000_000_000;
struct Shared {
    raw: Arc<AtomicU8>,
    writes: Arc<AtomicUsize>,
    fail: Arc<AtomicBool>,
}
impl Mixer for Shared {
    fn get(&mut self) -> io::Result<u8> {
        Ok(self.raw.load(Ordering::SeqCst))
    }
    fn set(&mut self, level: u8) -> io::Result<()> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        if self.fail.load(Ordering::SeqCst) {
            return Err(io::Error::other("fixture mixer failure"));
        }
        self.raw.store(level, Ordering::SeqCst);
        Ok(())
    }
}
struct NoLight;
impl Light for NoLight {
    fn there(&mut self) -> bool {
        false
    }
    fn shown(&mut self) -> Option<lamp_msgs::Light> {
        None
    }
    fn ask(&mut self, _: &lamp_msgs::Light) -> bool {
        panic!("no light is configured")
    }
}
fn request() -> caps::Put {
    caps::Put {
        source: "bedtime".into(),
        token: "one".into(),
        issued_unix_ms: WALL,
        valid_until_unix_ms: WALL + 60_000,
        light_channel_max: Some(12),
        volume_raw_max: Some(40),
    }
}
fn wait(mut predicate: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !predicate() {
        assert!(
            Instant::now() < until,
            "fixture did not reach its expected state"
        );
        thread::sleep(Duration::from_millis(5));
    }
}
fn ask(directory: &Path, value: Value) -> Value {
    let mut stream = UnixStream::connect(directory.join(CALL_SOCKET)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(format!("{value}\n").as_bytes()).unwrap();
    let mut response = String::new();
    BufReader::new(stream).read_line(&mut response).unwrap();
    serde_json::from_str(&response).unwrap()
}
struct Running {
    root: tempfile::TempDir,
    stop: Arc<AtomicBool>,
    stop_on_readiness_loss: Arc<AtomicBool>,
    worker: Option<JoinHandle<io::Result<()>>>,
    at: Arc<AtomicU64>,
    raw: Arc<AtomicU8>,
    writes: Arc<AtomicUsize>,
    fail: Arc<AtomicBool>,
}
impl Running {
    fn new(enabled: bool, restored: bool, failed_mixer: bool) -> Self {
        let root = tempfile::Builder::new().prefix("aci-").tempdir().unwrap();
        let config = Config {
            directory: root.path().join("caps"),
            bus_root: root.path().join("bus"),
        };
        if restored {
            let mut caps = caps::Controller::open(&config.directory, Some(WALL)).unwrap();
            caps.put(request(), Some(WALL)).unwrap();
        }
        let at = Arc::new(AtomicU64::new(1_000_000));
        let clock = at.clone();
        let service = enabled.then(|| {
            Service::with_clock(&config, "socket-fixture", move || Moment {
                at_us: clock.load(Ordering::SeqCst),
                unix_ms: Some(WALL),
            })
            .unwrap()
        });
        let raw = Arc::new(AtomicU8::new(80));
        let writes = Arc::new(AtomicUsize::new(0));
        let fail = Arc::new(AtomicBool::new(failed_mixer));
        let mixer = Shared {
            raw: raw.clone(),
            writes: writes.clone(),
            fail: fail.clone(),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let stop_on_readiness_loss = Arc::new(AtomicBool::new(false));
        let boundary = stop_on_readiness_loss.clone();
        let directory = root.path().join("run");
        let worker = thread::spawn(move || {
            let mut executive = Executive {
                sound: Some(Volume::new(mixer, 100)),
                light: None::<NoLight>,
                short: false,
            };
            let now = || {
                // The server reads this clock after updating readiness for
                // a reply, before returning to its stop check. A test can
                // request shutdown at that exact boundary without a sleep.
                if boundary.load(Ordering::Acquire) && !directory.join(READY_FILE).exists() {
                    boundary.store(false, Ordering::Release);
                    flag.store(true, Ordering::Release);
                }
                Instant::now()
            };
            server::serve_with_clock_and_caps(
                &directory,
                &mut executive,
                &flag,
                None,
                None,
                &now,
                service,
            )
        });
        let running = Self {
            root,
            stop,
            stop_on_readiness_loss,
            worker: Some(worker),
            at,
            raw,
            writes,
            fail,
        };
        wait(|| running.root.path().join("run").join(CALL_SOCKET).exists());
        running
    }
    fn ask(&self, value: Value) -> Value {
        ask(&self.root.path().join("run"), value)
    }
    fn ready(&self) -> bool {
        self.root.path().join("run").join(READY_FILE).exists()
    }
    fn finish(&mut self) -> io::Result<()> {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap()
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        if self.worker.is_some() {
            let _ = self.finish();
        }
    }
}

#[test]
fn socket_restores_before_ready_and_release_keeps_the_persons_newer_volume() {
    let mut running = Running::new(true, true, false);
    wait(|| running.ready());
    assert_eq!(running.raw.load(Ordering::SeqCst), 40);
    let bus = lamp_bus::Bus::new(running.root.path().join("bus")).unwrap();
    let kept: OutputCaps = bus
        .latest(OutputCaps::TOPIC)
        .unwrap()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(kept.entries[0].token, "one");
    let catalog =
        std::fs::read_to_string(running.root.path().join("run").join(CATALOG_FILE)).unwrap();
    assert!(!catalog.contains("caps_"));
    assert_eq!(
        running.ask(json!({"op":"caps_get"}))["snapshot"]["active"][0]["token"],
        "one"
    );
    assert_eq!(
        running.ask(json!({"op":"call","name":"volume_to","args":{"volume":25}}))["level"]["raw"],
        10
    );
    let writes = running.writes.load(Ordering::SeqCst);
    assert_eq!(
        running.ask(json!({"op":"caps_remove","source":"bedtime","token":"one"}))["accepted"],
        true
    );
    assert_eq!(running.raw.load(Ordering::SeqCst), 10);
    assert_eq!(running.writes.load(Ordering::SeqCst), writes);
    assert_eq!(
        running.ask(json!({"op":"caps_put","cap":request()}))["active"],
        false
    );
    assert_eq!(
        running.ask(json!({"op":"call","name":"volume_up","args":{}}))["ok"],
        true
    );
    assert_eq!(running.raw.load(Ordering::SeqCst), 30);
    running.finish().unwrap();
    assert!(!running.ready());
}

#[test]
fn unavailable_hardware_removes_readiness_but_exact_state_can_be_recovered_on_the_socket() {
    let mut running = Running::new(true, false, true);
    wait(|| running.ready());
    let reply = running.ask(json!({"op":"caps_put","cap":request()}));
    assert_eq!(reply["accepted"], true);
    assert_eq!(reply["volume"]["confirmed"], false);
    wait(|| !running.ready());
    assert_eq!(
        running.ask(json!({"op":"caps_get"}))["snapshot"]["active"][0]["token"],
        "one"
    );
    let writes = running.writes.load(Ordering::SeqCst);
    // More IPC activity is not a mixer polling loop or an implicit retry.
    for _ in 0..4 {
        running.ask(json!({"op":"caps_get"}));
    }
    assert_eq!(running.writes.load(Ordering::SeqCst), writes);
    running.fail.store(false, Ordering::SeqCst);
    running.at.fetch_add(1_000_000, Ordering::SeqCst);
    wait(|| running.ready());
    assert_eq!(running.raw.load(Ordering::SeqCst), 40);
    // A real storage failure must remove readiness and leave the last cap.
    std::fs::rename(
        running.root.path().join("caps"),
        running.root.path().join("moved"),
    )
    .unwrap();
    assert_eq!(
        running.ask(json!({"op":"caps_remove","source":"bedtime","token":"one"}))["code"],
        "storage_uncertain"
    );
    wait(|| !running.ready());
    assert!(running.finish().is_err());
    assert_eq!(running.raw.load(Ordering::SeqCst), 40);
}

#[test]
fn a_stop_request_cannot_hide_an_already_reported_storage_failure() {
    let mut running = Running::new(true, true, false);
    wait(|| running.ready());
    let bus = lamp_bus::Bus::new(running.root.path().join("bus")).unwrap();
    let before = bus.latest(OutputCaps::TOPIC).unwrap().unwrap();
    let writes = running.writes.load(Ordering::SeqCst);
    running
        .stop_on_readiness_loss
        .store(true, Ordering::Release);
    std::fs::rename(
        running.root.path().join("caps"),
        running.root.path().join("moved"),
    )
    .unwrap();
    let reply = running.ask(json!({"op":"caps_remove","source":"bedtime","token":"one"}));
    assert_eq!(reply["accepted"], false);
    assert_eq!(reply["code"], "storage_uncertain");
    let result = running.finish();
    assert!(
        !running.stop_on_readiness_loss.load(Ordering::Acquire),
        "shutdown must be requested before the next service tick"
    );
    assert!(result.is_err(), "shutdown hid a known storage failure");
    assert!(!running.ready());
    assert_eq!(running.raw.load(Ordering::SeqCst), 40);
    assert_eq!(running.writes.load(Ordering::SeqCst), writes);
    let after = bus.latest(OutputCaps::TOPIC).unwrap().unwrap();
    assert_eq!(after.header, before.header);
    assert_eq!(after.payload, before.payload);
}

#[test]
fn disabled_service_keeps_existing_volume_behavior_and_creates_no_cap_state() {
    let mut running = Running::new(false, false, false);
    wait(|| running.ready());
    assert_eq!(
        running.ask(json!({"op":"caps_put","cap":request()}))["code"],
        "unavailable"
    );
    assert_eq!(running.ask(json!({"op":"caps_get"}))["code"], "unavailable");
    assert!(!running.root.path().join("caps").exists());
    assert!(!running.root.path().join("bus").exists());
    assert_eq!(
        running.ask(json!({"op":"call","name":"volume_up","args":{}}))["ok"],
        true
    );
    assert_eq!(running.raw.load(Ordering::SeqCst), 100);
    running.finish().unwrap();
}
