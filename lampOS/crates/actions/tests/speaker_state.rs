//! The real Actions socket and bus, with a mixer that can stop at its I/O edge.
use lamp_actions::{
    Executive, caps,
    caps_service::{Config, Moment, Service as CapsService},
    light::Light,
    server::{CALL_SOCKET, READY_FILE, serve_with_clock_and_outputs, serve_with_clock_and_speaker},
    sound::{Mixer, Volume},
    speaker_state::Service,
};
use lamp_bus::Bus;
use lamp_msgs::{OutputCaps, SpeakerState};
use serde_json::{Value, json};
use std::{
    io::{self, BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

const WALL: u64 = 1_800_000_000_000;

fn restriction(token: &str, issued: u64, volume: Option<u8>, light: Option<u16>) -> caps::Put {
    caps::Put {
        source: "night".into(),
        token: token.into(),
        issued_unix_ms: issued,
        valid_until_unix_ms: issued + 60_000,
        light_channel_max: light,
        volume_raw_max: volume,
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
        false
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Edge {
    Read,
    Write,
}

struct Barrier {
    edge: Edge,
    entered: mpsc::SyncSender<Option<SpeakerState>>,
    release: mpsc::Receiver<()>,
}

type Block = (mpsc::Receiver<Option<SpeakerState>>, mpsc::SyncSender<()>);

struct Control {
    level: AtomicU8,
    reads: AtomicUsize,
    writes: AtomicUsize,
    fail_read: AtomicBool,
    fail_write: AtomicBool,
    fail_readback: AtomicBool,
    stuck: AtomicBool,
    barrier: Mutex<Option<Barrier>>,
    bus_root: PathBuf,
}

impl Control {
    fn edge(&self, edge: Edge) {
        let barrier = {
            let mut pending = self.barrier.lock().unwrap();
            if pending.as_ref().is_some_and(|barrier| barrier.edge == edge) {
                pending.take()
            } else {
                None
            }
        };
        if let Some(barrier) = barrier {
            let state = Bus::new(&self.bus_root)
                .unwrap()
                .latest(SpeakerState::TOPIC)
                .ok()
                .flatten()
                .map(|message| message.parse().unwrap());
            barrier.entered.send(state).unwrap();
            barrier
                .release
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
        }
    }

    fn block(&self, edge: Edge) -> Block {
        let (entered, receive) = mpsc::sync_channel(1);
        let (release, wait) = mpsc::sync_channel(1);
        *self.barrier.lock().unwrap() = Some(Barrier {
            edge,
            entered,
            release: wait,
        });
        (receive, release)
    }
}

struct Fake(Arc<Control>);
impl Mixer for Fake {
    fn get(&mut self) -> io::Result<u8> {
        self.0.edge(Edge::Read);
        self.0.reads.fetch_add(1, Ordering::AcqRel);
        if self.0.fail_read.load(Ordering::Acquire) {
            return Err(io::Error::other("read failed"));
        }
        Ok(self.0.level.load(Ordering::Acquire))
    }
    fn set(&mut self, level: u8) -> io::Result<()> {
        self.0.edge(Edge::Write);
        self.0.writes.fetch_add(1, Ordering::AcqRel);
        if self.0.fail_write.load(Ordering::Acquire) {
            return Err(io::Error::other("write failed"));
        }
        if !self.0.stuck.load(Ordering::Acquire) {
            self.0.level.store(level, Ordering::Release);
        }
        if self.0.fail_readback.load(Ordering::Acquire) {
            self.0.fail_read.store(true, Ordering::Release);
        }
        Ok(())
    }
}

struct Running {
    _home: tempfile::TempDir,
    dir: PathBuf,
    bus: Bus,
    control: Arc<Control>,
    stop: Arc<AtomicBool>,
    stop_on_readiness_loss: Arc<AtomicBool>,
    clock: Arc<AtomicU64>,
    join: Option<JoinHandle<io::Result<()>>>,
}

fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn ask(dir: &Path, name: &str, args: Value) -> Value {
    exchange(dir, json!({"op":"call","name":name,"args":args}))
}

fn exchange(dir: &Path, request: Value) -> Value {
    let mut socket = UnixStream::connect(dir.join(CALL_SOCKET)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    writeln!(socket, "{request}").unwrap();
    let mut line = String::new();
    BufReader::new(socket).read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

impl Running {
    fn new(enabled: bool, level: u8, block_initial: bool) -> (Self, Option<Block>) {
        Self::outputs(enabled, false, false, level, block_initial)
    }

    fn outputs(
        enabled: bool,
        caps_enabled: bool,
        restored: bool,
        level: u8,
        block_initial: bool,
    ) -> (Self, Option<Block>) {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("actions");
        let root = home.path().join("bus");
        let bus = Bus::new(&root).unwrap();
        let speaker =
            enabled.then(|| Service::new(Bus::new(&root).unwrap(), "fixture-host".into()).unwrap());
        let config = Config {
            directory: home.path().join("caps"),
            bus_root: root.clone(),
        };
        if restored {
            assert!(caps_enabled);
            let mut controller = caps::Controller::open(&config.directory, Some(WALL)).unwrap();
            controller
                .put(restriction("one", WALL, Some(40), None), Some(WALL))
                .unwrap();
        }
        let control = Arc::new(Control {
            level: AtomicU8::new(level),
            reads: AtomicUsize::new(0),
            writes: AtomicUsize::new(0),
            fail_read: AtomicBool::new(false),
            fail_write: AtomicBool::new(false),
            fail_readback: AtomicBool::new(false),
            stuck: AtomicBool::new(false),
            barrier: Mutex::new(None),
            bus_root: root,
        });
        let barrier = block_initial.then(|| control.block(Edge::Read));
        let stop = Arc::new(AtomicBool::new(false));
        let stop_on_readiness_loss = Arc::new(AtomicBool::new(false));
        let clock = Arc::new(AtomicU64::new(0));
        let caps = caps_enabled.then(|| {
            let clock = clock.clone();
            CapsService::with_clock(&config, "fixture-host", move || Moment {
                at_us: clock.load(Ordering::Acquire) + 1_000_000,
                unix_ms: Some(WALL + clock.load(Ordering::Acquire) / 1000),
            })
            .unwrap()
        });
        let join = {
            let (dir, control, stop, clock) =
                (dir.clone(), control.clone(), stop.clone(), clock.clone());
            let boundary = stop_on_readiness_loss.clone();
            std::thread::spawn(move || {
                let base = Instant::now();
                let now = || {
                    if boundary.load(Ordering::Acquire) && !dir.join(READY_FILE).exists() {
                        boundary.store(false, Ordering::Release);
                        stop.store(true, Ordering::Release);
                    }
                    base + Duration::from_micros(clock.load(Ordering::Acquire))
                };
                let mut executive = Executive {
                    sound: Some(Volume::new(Fake(control), 100)),
                    light: None::<NoLight>,
                    short: false,
                };
                serve_with_clock_and_outputs(
                    &dir,
                    &mut executive,
                    &stop,
                    None,
                    None,
                    &now,
                    caps,
                    speaker,
                )
            })
        };
        let running = Self {
            _home: home,
            dir,
            bus,
            control,
            stop,
            stop_on_readiness_loss,
            clock,
            join: Some(join),
        };
        until("serving boundary", || {
            if restored && block_initial {
                running.dir.join(CALL_SOCKET).exists()
            } else {
                running.dir.join(READY_FILE).exists()
            }
        });
        if enabled && !block_initial {
            until("first confirmed read", || {
                running.latest().volume_raw == Some(if restored { level.min(40) } else { level })
            });
        }
        (running, barrier)
    }

    fn start(enabled: bool, level: u8) -> Self {
        Self::new(enabled, level, false).0
    }
    fn latest(&self) -> SpeakerState {
        self.bus
            .latest(SpeakerState::TOPIC)
            .unwrap()
            .unwrap()
            .parse()
            .unwrap()
    }
    fn ask(&self, name: &str, args: Value) -> Value {
        ask(&self.dir, name, args)
    }
    fn private(&self, request: Value) -> Value {
        exchange(&self.dir, request)
    }
    fn begin_private(&self, request: Value) -> JoinHandle<Value> {
        let dir = self.dir.clone();
        std::thread::spawn(move || exchange(&dir, request))
    }
    fn begin(&self, name: &'static str, args: Value) -> JoinHandle<Value> {
        let dir = self.dir.clone();
        std::thread::spawn(move || ask(&dir, name, args))
    }
    fn finish(&mut self) -> io::Result<()> {
        self.stop.store(true, Ordering::Release);
        self.join.take().unwrap().join().unwrap()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

#[test]
fn every_output_switch_combination_keeps_one_mixer_owner_and_off_paths_idle() {
    for speaker in [false, true] {
        for caps in [false, true] {
            let (running, _) = Running::outputs(speaker, caps, false, 80, false);
            assert_eq!(
                running.control.reads.load(Ordering::Acquire),
                usize::from(speaker)
            );
            assert_eq!(running.control.writes.load(Ordering::Acquire), 0);
            assert_eq!(running._home.path().join("caps").exists(), caps);
            assert_eq!(
                running.bus.root().join(SpeakerState::TOPIC).exists(),
                speaker
            );
            assert_eq!(running.bus.root().join(OutputCaps::TOPIC).exists(), caps);
            let response = running.private(json!({
                "op":"caps_put","cap":restriction("one", WALL, Some(40), None)
            }));
            assert_eq!(response["accepted"], caps);
            assert_eq!(
                running.control.reads.load(Ordering::Acquire),
                usize::from(speaker) + if caps { 2 } else { 0 }
            );
            assert_eq!(
                running.control.writes.load(Ordering::Acquire),
                usize::from(caps)
            );
            assert_eq!(
                running.control.level.load(Ordering::Acquire),
                if caps { 40 } else { 80 }
            );
            if speaker {
                assert_eq!(
                    running.latest().volume_raw,
                    Some(if caps { 40 } else { 80 })
                );
            }
        }
    }
}

#[test]
fn restored_volume_cap_is_confirmed_before_ready_without_a_second_speaker_read() {
    let (running, barrier) = Running::outputs(true, true, true, 80, true);
    let (entered, release) = barrier.unwrap();
    let before = entered
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(before.volume_raw, None);
    assert!(!running.dir.join(READY_FILE).exists());
    release.send(()).unwrap();
    until("restored cap ready", || {
        running.dir.join(READY_FILE).exists()
    });
    assert_eq!(running.latest().volume_raw, Some(40));
    assert_eq!(running.control.reads.load(Ordering::Acquire), 2);
    assert_eq!(running.control.writes.load(Ordering::Acquire), 1);
}

#[test]
fn a_tighter_cap_withdraws_old_audio_before_blocking_mixer_io() {
    let (running, _) = Running::outputs(true, true, false, 80, false);
    let old = running.latest();
    let (entered, release) = running.control.block(Edge::Read);
    let call = running.begin_private(json!({
        "op":"caps_put","cap":restriction("one", WALL, Some(40), None)
    }));
    let during = entered
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(during.volume_raw, None);
    assert!(during.mute_generation > old.mute_generation);
    assert_eq!(running.control.level.load(Ordering::Acquire), 80);
    release.send(()).unwrap();
    assert_eq!(call.join().unwrap()["volume"]["confirmed"], true);
    let after = running.latest();
    assert_eq!(after.volume_raw, Some(40));
    assert!(
        after.mute_generation > old.mute_generation,
        "coalescing cannot revive the old fence"
    );
}

#[test]
fn failed_caps_never_authorize_positive_out_of_limit_readback_and_a_valid_read_recovers() {
    let (running, _) = Running::outputs(true, true, false, 80, false);
    let old = running.latest();
    running.control.stuck.store(true, Ordering::Release);
    let reply = running.private(json!({
        "op":"caps_put","cap":restriction("one", WALL, Some(40), None)
    }));
    assert_eq!(reply["accepted"], true);
    assert_eq!(reply["volume"]["confirmed"], false);
    until("failed cap removes ready", || {
        !running.dir.join(READY_FILE).exists()
    });
    assert_eq!(running.control.level.load(Ordering::Acquire), 80);
    assert_eq!(running.latest().volume_raw, None);
    assert_eq!(running.ask("volume_level", json!({}))["level"]["raw"], 80);
    assert_eq!(running.latest().volume_raw, None);
    let writes = running.control.writes.load(Ordering::Acquire);
    running.control.stuck.store(false, Ordering::Release);
    running.control.level.store(35, Ordering::Release);
    assert_eq!(running.ask("volume_level", json!({}))["level"]["raw"], 35);
    until("actual compliant read restores ready", || {
        running.dir.join(READY_FILE).exists()
    });
    assert_eq!(running.control.writes.load(Ordering::Acquire), writes);
    let recovered = running.latest();
    assert_eq!(recovered.volume_raw, Some(35));
    assert!(recovered.mute_generation > old.mute_generation);
}

#[test]
fn cap_only_zero_or_lost_observation_survives_recovery_and_latest_coalescing() {
    for lost in [false, true] {
        let (running, _) = Running::outputs(true, true, true, 40, false);
        let old = running.latest();
        running.control.level.store(0, Ordering::Release);
        running.control.fail_read.store(lost, Ordering::Release);
        // Release never restores volume, but its real read can discover zero
        // or a lost observation. No ordinary volume command runs here.
        let release = running.private(json!({"op":"caps_remove","source":"night","token":"one"}));
        assert_eq!(release["accepted"], true);
        assert_eq!(
            running.latest().volume_raw,
            if lost { None } else { Some(0) }
        );
        running.control.fail_read.store(false, Ordering::Release);
        running.control.level.store(30, Ordering::Release);
        running.clock.store(1_000, Ordering::Release);
        let replace = running.private(json!({
            "op":"caps_put","cap":restriction("two", WALL + 1, Some(40), None)
        }));
        assert_eq!(replace["accepted"], true);
        let recovered = running.latest();
        assert_eq!(recovered.volume_raw, Some(30));
        assert!(recovered.mute_generation > old.mute_generation);
        assert_eq!(running.control.writes.load(Ordering::Acquire), 0);
    }
}

#[test]
fn an_unrelated_light_publication_failure_does_not_withdraw_compliant_sound() {
    let (running, _) = Running::outputs(true, true, true, 40, false);
    let before = running.latest();
    let reads = running.control.reads.load(Ordering::Acquire);
    let latest = running.bus.root().join(OutputCaps::TOPIC).join("latest");
    std::fs::remove_file(&latest).unwrap();
    std::fs::create_dir(&latest).unwrap();
    let mut light = restriction("light", WALL, None, Some(12));
    light.source = "evening".into();
    let reply = running.private(json!({"op":"caps_put","cap":light}));
    assert_eq!(reply["accepted"], true);
    assert_eq!(reply["ring"]["published"], false);
    until("ring delivery removes readiness", || {
        !running.dir.join(READY_FILE).exists()
    });
    assert_eq!(running.control.reads.load(Ordering::Acquire), reads);
    assert_eq!(running.latest(), before);
    assert_eq!(running.ask("volume_level", json!({}))["ok"], true);
    let after = running.latest();
    assert_eq!(after.volume_raw, Some(40));
    assert_eq!(after.mute_generation, before.mute_generation);
}

#[test]
fn explicit_mute_still_precedes_mixer_io_under_a_volume_cap_and_release_stays_muted() {
    let (running, _) = Running::outputs(true, true, true, 40, false);
    let old = running.latest();
    let (entered, release) = running.control.block(Edge::Write);
    let call = running.begin("volume_to", json!({"volume":0}));
    let during = entered
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!((during.volume_raw, during.muted), (None, true));
    assert!(during.mute_generation > old.mute_generation);
    release.send(()).unwrap();
    assert_eq!(call.join().unwrap()["ok"], true);
    let writes = running.control.writes.load(Ordering::Acquire);
    assert_eq!(
        running.private(json!({"op":"caps_remove","source":"night","token":"one"}))["accepted"],
        true
    );
    assert_eq!(running.control.writes.load(Ordering::Acquire), writes);
    assert_eq!(
        (running.latest().volume_raw, running.latest().muted),
        (Some(0), true)
    );
}

#[test]
fn concurrent_stop_preserves_cap_failure_and_publishes_unknown_without_mixer_work() {
    let (mut running, _) = Running::outputs(true, true, true, 40, false);
    let saved = running
        .bus
        .latest(OutputCaps::TOPIC)
        .unwrap()
        .unwrap()
        .parse::<OutputCaps>()
        .unwrap();
    let reads = running.control.reads.load(Ordering::Acquire);
    let writes = running.control.writes.load(Ordering::Acquire);
    std::fs::rename(
        running._home.path().join("caps"),
        running._home.path().join("moved"),
    )
    .unwrap();
    running
        .stop_on_readiness_loss
        .store(true, Ordering::Release);
    let reply = running.private(json!({"op":"caps_remove","source":"night","token":"one"}));
    assert_eq!(reply["code"], "storage_uncertain");
    assert!(running.finish().is_err());
    assert!(!running.dir.join(READY_FILE).exists());
    assert_eq!(running.latest().volume_raw, None);
    assert_eq!(running.control.reads.load(Ordering::Acquire), reads);
    assert_eq!(running.control.writes.load(Ordering::Acquire), writes);
    assert_eq!(
        running
            .bus
            .latest(OutputCaps::TOPIC)
            .unwrap()
            .unwrap()
            .parse::<OutputCaps>()
            .unwrap(),
        saved
    );
}

#[test]
fn startup_is_unknown_before_the_first_mixer_read_and_restart_has_a_new_session() {
    let (mut running, barrier) = Running::new(true, 50, true);
    let (entered, release) = barrier.unwrap();
    let state = entered
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(
        (state.volume_raw, state.muted, state.mute_generation),
        (None, false, 0)
    );
    assert_eq!(running.control.reads.load(Ordering::Acquire), 0);
    release.send(()).unwrap();
    until("confirmed read", || running.latest().volume_raw == Some(50));
    let old = running.latest();
    running.finish().unwrap();
    let second =
        Service::new(Bus::new(running.bus.root()).unwrap(), "fixture-host".into()).unwrap();
    // A second fixture runs through the same serving boundary, with an
    // independently acquired Actions socket but the same host and bus.
    let second_dir = running._home.path().join("restarted");
    let mut executive = Executive {
        sound: None::<Volume<Fake>>,
        light: None::<NoLight>,
        short: false,
    };
    serve_with_clock_and_speaker(
        &second_dir,
        &mut executive,
        &AtomicBool::new(true),
        None,
        None,
        Some(second),
        &Instant::now,
    )
    .unwrap();
    let new = running.latest();
    assert_eq!(new.stamp.clock_id, old.stamp.clock_id);
    assert_ne!(new.session_id, old.session_id);
    assert_eq!((new.mute_generation, new.volume_raw), (0, None));
}

#[test]
fn model_and_app_mutes_publish_before_their_first_blocking_mixer_io() {
    for (name, args, edge) in [
        ("volume_mute", json!({}), Edge::Read),
        ("volume_to", json!({"volume":0}), Edge::Write),
    ] {
        let running = Running::start(true, 50);
        let (entered, release) = running.control.block(edge);
        let call = running.begin(name, args);
        let state = entered
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        assert_eq!(
            (state.mute_generation, state.volume_raw, state.muted),
            (1, None, true)
        );
        assert_eq!(running.control.reads.load(Ordering::Acquire), 1);
        assert_eq!(running.control.writes.load(Ordering::Acquire), 0);
        release.send(()).unwrap();
        assert_eq!(call.join().unwrap()["ok"], true);
        assert_eq!(running.latest().volume_raw, Some(0));
        assert!(running.latest().muted);
        assert_eq!(running.latest().mute_generation, 1);
    }
}

#[test]
fn already_muted_and_rapid_unmute_keep_the_consumed_generation() {
    let running = Running::start(true, 0);
    let initial = running.latest();
    for _ in 0..2 {
        assert_eq!(running.ask("volume_mute", json!({}))["ok"], true);
    }
    assert_eq!(running.control.writes.load(Ordering::Acquire), 0);
    assert_eq!(
        running.latest().mute_generation,
        initial.mute_generation + 2
    );
    assert_eq!(running.ask("volume_to", json!({"volume":30}))["ok"], true);
    let audible = running.latest();
    assert_eq!((audible.volume_raw, audible.muted), (Some(30), false));
    assert_eq!(audible.mute_generation, initial.mute_generation + 2);
    assert_eq!(audible.session_id, initial.session_id);
    assert_eq!(running.ask("volume_mute", json!({}))["ok"], true);
    assert_eq!(running.ask("volume_unmute", json!({}))["ok"], true);
    assert_eq!(
        running.latest().mute_generation,
        initial.mute_generation + 3
    );
    assert_eq!(running.latest().volume_raw, Some(30));
}

#[test]
fn failed_read_write_and_readback_cannot_erase_mute_intent() {
    for failure in 0..4 {
        let running = Running::start(true, 50);
        match failure {
            0 => running.control.fail_read.store(true, Ordering::Release),
            1 => running.control.fail_write.store(true, Ordering::Release),
            2 => running.control.fail_readback.store(true, Ordering::Release),
            _ => running.control.stuck.store(true, Ordering::Release),
        }
        let name = if failure == 1 {
            "volume_to"
        } else {
            "volume_mute"
        };
        let args = if failure == 1 {
            json!({"volume":0})
        } else {
            json!({})
        };
        assert_eq!(running.ask(name, args)["ok"], false);
        let state = running.latest();
        assert_eq!(
            (state.mute_generation, state.volume_raw, state.muted),
            (1, None, true)
        );
        running.control.fail_read.store(false, Ordering::Release);
        running.control.fail_write.store(false, Ordering::Release);
        running
            .control
            .fail_readback
            .store(false, Ordering::Release);
        running.control.stuck.store(false, Ordering::Release);
        assert_eq!(running.ask("volume_level", json!({}))["ok"], true);
        assert!(running.latest().muted, "observation is not unmute intent");
        assert_eq!(running.ask("volume_unmute", json!({}))["ok"], true);
        assert!(!running.latest().muted);
        assert_eq!(running.latest().mute_generation, 1);
    }
}

#[test]
fn all_volume_paths_publish_confirmed_state_without_extra_post_call_reads() {
    let running = Running::start(true, 50);
    let generation = running.latest().mute_generation;
    for (name, args, raw, reads) in [
        ("volume_up", json!({}), 70, 2),
        ("volume_down", json!({}), 50, 2),
        ("volume_level", json!({}), 50, 1),
        ("volume_to", json!({"volume":25}), 25, 1),
    ] {
        let before = running.control.reads.load(Ordering::Acquire);
        assert_eq!(running.ask(name, args)["ok"], true);
        assert_eq!(running.latest().volume_raw, Some(raw));
        assert_eq!(running.latest().mute_generation, generation);
        assert_eq!(
            running.control.reads.load(Ordering::Acquire) - before,
            reads
        );
    }
    let before = running.latest();
    assert_eq!(running.ask("volume_mute", json!({"by":1}))["ok"], false);
    assert_eq!(running.ask("volume_to", json!({"volume":101}))["ok"], false);
    assert_eq!(running.latest(), before);
}

#[test]
fn invalid_volume_arguments_leave_confirmed_caps_and_the_audio_fence_unchanged() {
    for speaker in [false, true] {
        let (running, _) = Running::outputs(speaker, true, true, 40, false);
        let before = speaker.then(|| running.latest());
        let reads = running.control.reads.load(Ordering::Acquire);
        let writes = running.control.writes.load(Ordering::Acquire);
        for (name, args) in [
            ("volume_mute", json!({"by":1})),
            ("volume_to", json!({"volume":101})),
        ] {
            assert_eq!(running.ask(name, args)["ok"], false);
            // A second socket reply proves the prior readiness update ran;
            // the frozen clock cannot mask a failure with an automatic retry.
            assert_eq!(running.private(json!({"op":"caps_get"}))["ok"], true);
            assert!(running.dir.join(READY_FILE).exists());
            assert_eq!(speaker.then(|| running.latest()), before);
            assert_eq!(running.control.reads.load(Ordering::Acquire), reads);
            assert_eq!(running.control.writes.load(Ordering::Acquire), writes);
        }
    }
}

#[test]
fn actual_mixer_failures_still_withdraw_confirmed_caps_and_audio_permission() {
    for speaker in [false, true] {
        for write in [false, true] {
            let (running, _) = Running::outputs(speaker, true, true, 40, false);
            let before = speaker.then(|| running.latest());
            let reads = running.control.reads.load(Ordering::Acquire);
            let writes = running.control.writes.load(Ordering::Acquire);
            let (name, args) = if write {
                running.control.fail_write.store(true, Ordering::Release);
                ("volume_to", json!({"volume":50}))
            } else {
                running.control.fail_read.store(true, Ordering::Release);
                ("volume_level", json!({}))
            };
            assert_eq!(running.ask(name, args)["ok"], false);
            assert_eq!(running.private(json!({"op":"caps_get"}))["ok"], true);
            assert!(!running.dir.join(READY_FILE).exists());
            assert_eq!(
                running.control.reads.load(Ordering::Acquire),
                reads + usize::from(!write)
            );
            assert_eq!(
                running.control.writes.load(Ordering::Acquire),
                writes + usize::from(write)
            );
            if let Some(before) = before {
                let after = running.latest();
                assert_eq!(after.volume_raw, None);
                assert!(after.mute_generation > before.mute_generation);
            }
        }
    }
}

#[test]
fn a_zero_observed_inside_up_or_unmute_cannot_disappear_from_the_audio_fence() {
    for (name, restored) in [("volume_up", 20), ("volume_unmute", 50)] {
        let running = Running::start(true, 50);
        let before = running.latest();
        let old_fence = (
            before.stamp.clock_id.clone(),
            before.session_id.clone(),
            before.mute_generation,
        );
        let reads = running.control.reads.load(Ordering::Acquire);
        // The physical control changed independently, between bounded owner
        // observations. This command sees zero and restores sound in one call.
        running.control.level.store(0, Ordering::Release);
        let (entered, release) = running.control.block(Edge::Write);
        let call = running.begin(name, json!({}));
        let during = entered
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        assert_eq!(running.control.reads.load(Ordering::Acquire), reads + 1);
        assert_eq!(running.control.level.load(Ordering::Acquire), 0);
        assert_eq!(during.mute_generation, before.mute_generation);
        release.send(()).unwrap();
        assert_eq!(call.join().unwrap()["ok"], true);

        // Observe only the final Latest snapshot, as a coalescing consumer may.
        let latest = running.latest();
        assert_eq!((latest.volume_raw, latest.muted), (Some(restored), false));
        assert_eq!(latest.mute_generation, before.mute_generation + 1);
        assert_ne!(
            (
                latest.stamp.clock_id.clone(),
                latest.session_id.clone(),
                latest.mute_generation,
            ),
            old_fence,
            "an old recording cannot regain its original audible fence"
        );
        assert_eq!(running.control.reads.load(Ordering::Acquire), reads + 2);
        assert_eq!(running.control.writes.load(Ordering::Acquire), 1);
        assert_eq!(running.ask("volume_up", json!({}))["ok"], true);
        assert_eq!(running.latest().mute_generation, latest.mute_generation);
    }
}

#[test]
fn failed_readback_followed_by_rapid_success_keeps_the_revocation_in_latest() {
    let running = Running::start(true, 50);
    let before = running.latest();
    running.control.fail_readback.store(true, Ordering::Release);
    assert_eq!(running.ask("volume_to", json!({"volume":60}))["ok"], false);
    running
        .control
        .fail_readback
        .store(false, Ordering::Release);
    running.control.fail_read.store(false, Ordering::Release);
    assert_eq!(running.ask("volume_to", json!({"volume":30}))["ok"], true);
    // A consumer that missed the intermediate unknown snapshot still sees a
    // different revocation counter and cannot resume the old recording.
    let latest = running.latest();
    assert_eq!((latest.volume_raw, latest.muted), (Some(30), false));
    assert_eq!(latest.mute_generation, before.mute_generation + 1);
    assert_eq!(latest.session_id, before.session_id);
}

#[test]
fn heartbeat_is_bounded_and_missing_readback_is_unknown() {
    let running = Running::start(true, 50);
    running.clock.store(999_999, Ordering::Release);
    running.ask("not_a_tool", json!({}));
    assert_eq!(running.control.reads.load(Ordering::Acquire), 1);
    running.control.fail_read.store(true, Ordering::Release);
    running.clock.store(1_000_000, Ordering::Release);
    until("one heartbeat read", || {
        running.latest().volume_raw.is_none()
    });
    assert_eq!(running.control.reads.load(Ordering::Acquire), 2);
    for _ in 0..3 {
        running.ask("not_a_tool", json!({}));
    }
    assert_eq!(running.control.reads.load(Ordering::Acquire), 2);
    running.control.fail_read.store(false, Ordering::Release);
    running.clock.store(2_000_000, Ordering::Release);
    until("recovered observation", || {
        running.latest().volume_raw == Some(50)
    });
    assert_eq!(running.control.reads.load(Ordering::Acquire), 3);
}

#[test]
fn publication_failure_withdraws_readiness_and_never_skips_the_physical_mute() {
    let mut running = Running::start(true, 50);
    let latest = running.bus.root().join(SpeakerState::TOPIC).join("latest");
    std::fs::remove_file(&latest).unwrap();
    std::fs::create_dir(&latest).unwrap();
    assert_eq!(running.ask("volume_mute", json!({}))["ok"], true);
    assert_eq!(running.control.level.load(Ordering::Acquire), 0);
    assert!(running.finish().is_err());
    assert!(!running.dir.join(READY_FILE).exists());
    assert!(!running.dir.join(CALL_SOCKET).exists());
}

#[test]
fn disabled_wrapper_keeps_original_mixer_counts_and_creates_no_speaker_topic() {
    let running = Running::start(false, 50);
    running.clock.store(30_000_000, Ordering::Release);
    for _ in 0..3 {
        running.ask("not_a_tool", json!({}));
    }
    assert_eq!(running.control.reads.load(Ordering::Acquire), 0);
    assert!(!running.bus.root().join(SpeakerState::TOPIC).exists());
    assert_eq!(running.ask("volume_mute", json!({}))["ok"], true);
    assert_eq!(running.control.reads.load(Ordering::Acquire), 2);
    assert_eq!(running.control.writes.load(Ordering::Acquire), 1);
    assert_eq!(running.ask("volume_to", json!({"volume":30}))["ok"], true);
    assert_eq!(running.control.reads.load(Ordering::Acquire), 3);
    assert_eq!(running.control.writes.load(Ordering::Acquire), 2);
    assert!(!running.bus.root().join(SpeakerState::TOPIC).exists());
}
