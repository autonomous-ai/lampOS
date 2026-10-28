//! Rounds of an update on a lamp made of files, with a channel, systemd,
//! a bus and a clock of the test's own: no network, no systemd, no lamp.
//!
//! The lamp has a voice (with a worker and a model beside it, as lamp-4ace
//! has), a body, the controls (with their settings beside them), a unit
//! that is not a part, and lamp-update's own. A part's "binary" is a line
//! of text: one with `crash` in it dies whenever it is started, and one
//! with `unwell` says it is in error in the lamp's health.
use lamp_msgs::{Health, HealthItem, Level, Voice};
use lamp_update::{
    Config, Outcome, World, apply,
    fetch::{Channel, Fetch},
    judge::UnitState,
    manifest::{self, File, ImageFile, Manifest, Model, Setting, Unit, sha256_hex},
    own,
    quiet::Heard,
    round, unit,
    world::{self, Clock, Disk, Ears, Systemd},
};
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

// The standalone single-threaded syscall harness reuses this fixture.
// The ordinary libtest binary does not invoke its custom entrypoint.
#[cfg(target_os = "linux")]
#[allow(dead_code)]
#[path = "round/process_cuts.rs"]
pub(crate) mod process_cuts;

#[path = "round/setup_lifecycle.rs"]
mod setup_lifecycle;

#[path = "round/head_touch.rs"]
mod head_touch;

#[path = "round/asleep_quiet.rs"]
mod asleep_quiet;

#[path = "round/product_adds.rs"]
mod product_adds;
#[path = "round/product_carrier.rs"]
mod product_carrier;
#[path = "round/product_companions.rs"]
mod product_companions;
#[path = "round/restart_order.rs"]
mod restart_order;
#[path = "round/restart_quiet.rs"]
mod restart_quiet;
#[path = "round/self_cuts.rs"]
mod self_cuts;
#[path = "round/self_recovery.rs"]
mod self_recovery;
#[path = "round/self_retry.rs"]
mod self_retry;
#[path = "round/writer_refresh.rs"]
mod writer_refresh;

const CHANNEL: &str = "https://channel.test/lamp-voice/";

/// A key made for these tests and thrown away with them.
fn throwaway() -> Ed25519KeyPair {
    let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    Ed25519KeyPair::from_pkcs8(document.as_ref()).unwrap()
}

#[derive(Default)]
struct FakeChannel {
    files: HashMap<String, Vec<u8>>,
    offline: bool,
    /// URLs under this one are out of reach, the rest answered.
    unreachable_under: Option<String>,
    /// Every URL asked for, in order.
    fetched: Vec<String>,
}

impl Channel for FakeChannel {
    fn fetch(&mut self, url: &str, most: u64, out: &mut dyn Write) -> Result<u64, Fetch> {
        self.fetched.push(url.to_owned());
        if self.offline
            || self
                .unreachable_under
                .as_ref()
                .is_some_and(|under| url.starts_with(under.as_str()))
        {
            return Err(Fetch::Unreachable("no network".into()));
        }
        let bytes = self.files.get(url).ok_or(Fetch::Missing)?;
        if bytes.len() as u64 > most {
            return Err(Fetch::Failed("larger than expected".into()));
        }
        out.write_all(bytes).unwrap();
        Ok(bytes.len() as u64)
    }
}

struct FakeClock(Rc<Cell<u64>>);

impl Clock for FakeClock {
    fn now_ms(&self) -> u64 {
        self.0.get()
    }
}

/// What the voice says between two times: a resting state every second,
/// and speech when the test says so.
type Talk = Box<dyn Fn(u64) -> bool>;

struct FakeEars {
    now: Rc<Cell<u64>>,
    talking: Talk,
    root: PathBuf,
}

impl Ears for FakeEars {
    fn listen(&mut self, wait: Duration) -> Vec<Heard> {
        let from = self.now.get();
        let to = from + wait.as_millis() as u64;
        self.now.set(to);
        let mut heard = Vec::new();
        for second in (from / 1000 + 1)..=(to / 1000) {
            let at = second * 1000;
            if (self.talking)(at) {
                heard.push(Heard::Speech);
                heard.push(Heard::State(Voice::Speaking));
            } else {
                heard.push(Heard::State(Voice::Resting));
            }
        }
        heard
    }

    /// Every part that runs reports: in error if its binary is unwell.
    fn health(&mut self) -> Option<Health> {
        let mut items = Vec::new();
        for (unit, binary) in running_binaries(&self.root) {
            let part = unit
                .trim_start_matches("lamp-")
                .trim_end_matches(".service");
            let level = if binary.contains("unwell") {
                Level::Error
            } else {
                Level::Ok
            };
            items.push(HealthItem {
                name: part.to_owned(),
                level,
                message: if level == Level::Ok {
                    String::new()
                } else {
                    "it says it is unwell".into()
                },
                age_ms: 200,
            });
        }
        items.sort_by_key(|item| std::cmp::Reverse(item.level.rank()));
        let level = items.first().map_or(Level::Ok, |item| item.level);
        Some(Health {
            level,
            message: String::new(),
            items,
            missing: Vec::new(),
            quiet_ms: 0,
        })
    }
}

/// Each part's unit and the text of the binary it starts now.
fn running_binaries(root: &Path) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for entry in fs::read_dir(root.join("etc/systemd/system")).unwrap() {
        let entry = entry.unwrap();
        // Product installation also places per-unit drop-in directories.
        if entry.file_type().unwrap().is_dir() {
            continue;
        }
        let text = fs::read_to_string(entry.path()).unwrap();
        if let Some(program) = unit::program(&text).or_else(|| unit::linked_program(&text, root)) {
            let path = root
                .join(program.folder().trim_start_matches('/'))
                .join(&program.binary);
            let binary = fs::read_to_string(path).unwrap_or_default();
            found.push((entry.file_name().to_string_lossy().into_owned(), binary));
        }
    }
    found
}

#[derive(Clone, Default)]
struct Running {
    active: String,
    sub: String,
    pid: u32,
    restarts: u32,
}

struct FakeSystemd {
    root: PathBuf,
    units: BTreeMap<String, Running>,
    calls: Rc<RefCell<Vec<String>>>,
    next_pid: u32,
    now: Rc<Cell<u64>>,
    /// When each restart was asked for.
    restarted_at: Vec<(String, u64)>,
    /// What each unit's program last wrote to the journal.
    lines: BTreeMap<String, String>,
    /// Each unit whose last line was asked, in order (#1505).
    lines_asked: Vec<String>,
    /// Units that crash whatever their build: the old build too.
    crashing: Vec<String>,
    /// Starts rejected by an unmet condition: systemctl succeeds, unit stays inactive.
    condition_skipped: Vec<String>,
    /// Explicit manager facts, independent of the filesystem classifier.
    setup_properties: Option<String>,
    remove_setup_marker_on_query: bool,
    /// `systemctl show` fails from the first restart on.
    unreachable_after_restart: bool,
    unreachable: bool,
    /// From the first restart on, this many `systemctl show` fail, then
    /// they answer again.
    failing_after_restart: usize,
    failing: usize,
    /// What starting lamp-update-apply runs: none, and its start is skipped
    /// (a lamp without it), doing nothing.
    applier: Option<Box<dyn FnMut() -> Result<(), String>>>,
    /// What starting the settings writer's one-shot runs: none, and the
    /// start is refused (a systemd that cannot).
    writer: Option<Box<dyn FnMut() -> Result<(), String>>>,
    /// When the settings writer's `back` last began, in Unix ms: none, and
    /// systemd cannot say.
    back_began: Option<u64>,
    /// The next reload stops the round dead, as a power cut would.
    cut_at_reload: bool,
    /// Independent recovery must be explicitly armed by a self-update fixture.
    self_recovery: Result<(), String>,
    self_recovery_queries: usize,
}

impl FakeSystemd {
    fn start(&mut self, unit: &str) {
        let text = fs::read_to_string(self.root.join("etc/systemd/system").join(unit)).unwrap();
        if unit.ends_with(".socket") || unit.ends_with(".timer") {
            // Listening, or waiting: no process of its own.
            let sub = if unit.ends_with(".socket") {
                "listening"
            } else {
                "waiting"
            };
            self.units.insert(
                unit.to_owned(),
                Running {
                    active: "active".into(),
                    sub: sub.into(),
                    ..Running::default()
                },
            );
            return;
        }
        let program = unit::program(&text)
            .or_else(|| unit::linked_program(&text, &self.root))
            .unwrap();
        let binary = fs::read_to_string(
            self.root
                .join(program.folder().trim_start_matches('/'))
                .join(&program.binary),
        )
        .unwrap_or_default();
        self.next_pid += 1;
        let running = if self.condition_skipped.iter().any(|name| name == unit) {
            Running {
                active: "inactive".into(),
                sub: "dead".into(),
                ..Running::default()
            }
        } else if binary.contains("crash") || self.crashing.iter().any(|name| name == unit) {
            Running {
                active: "activating".into(),
                sub: "auto-restart".into(),
                pid: 0,
                restarts: 1,
            }
        } else {
            Running {
                active: "active".into(),
                sub: "running".into(),
                pid: self.next_pid,
                restarts: 0,
            }
        };
        self.units.insert(unit.to_owned(), running);
    }
}

impl Systemd for FakeSystemd {
    fn setup_unit_properties(&mut self) -> Option<String> {
        if self.remove_setup_marker_on_query {
            fs::remove_file(self.root.join("var/lib/lampos-setup/done")).unwrap();
        }
        self.setup_properties.clone()
    }
    fn self_recovery_ready(&mut self) -> Result<(), String> {
        self.self_recovery_queries += 1;
        if self.unreachable {
            return Err("Failed to connect to bus".into());
        }
        self.self_recovery.clone()
    }

    fn states(&mut self, units: &[String]) -> Result<Vec<UnitState>, String> {
        if self.unreachable {
            return Err("Failed to connect to bus".into());
        }
        if self.failing > 0 {
            self.failing -= 1;
            return Err("systemctl did not answer within 5 s".into());
        }
        Ok(units
            .iter()
            .map(|unit| {
                let running = self.units.get(unit).cloned().unwrap_or(Running {
                    active: "inactive".into(),
                    sub: "dead".into(),
                    ..Running::default()
                });
                UnitState {
                    unit: unit.clone(),
                    active: running.active,
                    sub: running.sub,
                    pid: running.pid,
                    restarts: running.restarts,
                }
            })
            .collect())
    }

    fn reload(&mut self) -> Result<(), String> {
        self.calls.borrow_mut().push("reload".into());
        if std::mem::take(&mut self.cut_at_reload) {
            panic!("the lamp lost power");
        }
        Ok(())
    }

    fn restart(&mut self, unit: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("restart {unit}"));
        if unit == apply::UNIT {
            return self.applier.as_mut().map_or(Ok(()), |applier| applier());
        }
        self.restarted_at.push((unit.to_owned(), self.now.get()));
        self.unreachable |= self.unreachable_after_restart;
        self.failing += std::mem::take(&mut self.failing_after_restart);
        self.start(unit);
        Ok(())
    }

    fn stop(&mut self, unit: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("stop {unit}"));
        self.units.remove(unit);
        Ok(())
    }

    fn writer_unit_properties(&mut self) -> Result<String, String> {
        if self.writer.is_none() {
            return Err("this systemd cannot show the writer's unit".into());
        }
        Ok(format!(
            "Id={unit}\nLoadState=loaded\nFragmentPath=/etc/systemd/system/{unit}\nDropInPaths=\nUnitFileState=static\n",
            unit = lamp_update::writer_unit::UNIT
        ))
    }

    fn back_began_unix_ms(&mut self) -> Option<u64> {
        self.back_began
    }

    fn start_writer_unit(&mut self) -> Result<(), String> {
        self.calls
            .borrow_mut()
            .push(format!("start {}", lamp_update::writer_unit::UNIT));
        match self.writer.as_mut() {
            Some(writer) => writer(),
            None => Err("this systemd cannot start the writer's unit".into()),
        }
    }

    fn last_line(&mut self, unit: &str) -> Option<String> {
        self.lines_asked.push(unit.to_owned());
        self.lines.get(unit).cloned()
    }
}

struct FakeDisk(u64);

impl Disk for FakeDisk {
    fn free(&self, _: &Path) -> io::Result<u64> {
        Ok(self.0)
    }
}

/// The SHA-256 of a binary's text.
fn sha(text: &str) -> String {
    sha256_hex(text.as_bytes())
}

fn voice_unit(build: &str) -> String {
    format!(
        "# lamp-4ace's voice\n[Unit]\nDescription=lampOS voice\n\n[Service]\nExecStart=/opt/lampOS/releases/{build}/lamp-voice serve /etc/lampOS/voice/provider.json --endpoint smart-turn-fast\nRestart=on-failure\n"
    )
}

fn body_unit(build: &str, options: &str) -> String {
    format!(
        "[Unit]\nDescription=lampOS body\n\n[Service]\nExecStart=/opt/lampOS/body/{build}/lamp-body --device /dev/ttyACM0{options}\nRestart=on-failure\n"
    )
}

fn controls_unit(build: &str) -> String {
    format!(
        "[Service]\nExecStart=/opt/lampOS/controls/{build}/lamp-controls serve --config /opt/lampOS/controls/{build}/controls.json\n"
    )
}

/// A process one test starts holds, from its fork to its exec, a copy of
/// every file the other tests have open, a records' lock among them: that
/// lock, let go by its test and taken again, is then "another lamp-update
/// is at work". A test that starts a process holds `FORKS` to write until
/// it has started, and so does a round on Linux, which can start one
/// itself ([`rounding`]); each take of the lock holds it to read.
static FORKS: std::sync::RwLock<()> = std::sync::RwLock::new(());

fn no_fork() -> std::sync::RwLockReadGuard<'static, ()> {
    FORKS
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn forking() -> std::sync::RwLockWriteGuard<'static, ()> {
    FORKS
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// What a round holds. A round can start `lamp-skills list` (its skills
/// check), so on Linux, where a child holds the other tests' files from its
/// start to its exec, it holds `FORKS` to write (#1569 row 57: 5 of 12 runs
/// in the image failed on the lock without it). macOS starts a child in one
/// step, so there a round only holds it to read, as a take of the lock does.
#[cfg(target_os = "linux")]
fn rounding() -> std::sync::RwLockWriteGuard<'static, ()> {
    forking()
}

#[cfg(not(target_os = "linux"))]
fn rounding() -> std::sync::RwLockReadGuard<'static, ()> {
    no_fork()
}

/// A lamp made of files, its channel, and the rest of its world.
struct Lamp {
    _folder: tempfile::TempDir,
    root: PathBuf,
    key: Ed25519KeyPair,
    channel: FakeChannel,
    systemd: FakeSystemd,
    calls: Rc<RefCell<Vec<String>>>,
    now: Rc<Cell<u64>>,
    talking: Option<Talk>,
    free: u64,
    said: Vec<String>,
    /// The phases the round said, for `update/status` (#2194).
    phases: Vec<&'static str>,
    config: Config,
}

impl Lamp {
    /// lamp-4ace as it runs build 1, every part running.
    fn new() -> Self {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().to_owned();
        let write = |path: &str, text: &str| {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        };
        write("etc/hostname", "lamp-4ace\n");
        let (voice, body, controls) = (sha("voice 1"), sha("body 1"), sha("controls 1"));
        write(
            &format!("opt/lampOS/releases/{voice}/lamp-voice"),
            "voice 1",
        );
        write(
            &format!("opt/lampOS/releases/{voice}/lamp-turn-end"),
            "turn-end 1",
        );
        write(
            &format!("opt/lampOS/releases/{voice}/smart-turn.onnx"),
            "a model",
        );
        write(&format!("opt/lampOS/body/{body}/lamp-body"), "body 1");
        write(
            &format!("opt/lampOS/controls/{controls}/lamp-controls"),
            "controls 1",
        );
        write(
            &format!("opt/lampOS/controls/{controls}/controls.json"),
            "{\"pads\":2}",
        );
        write("etc/systemd/system/lamp-voice.service", &voice_unit(&voice));
        write(
            "etc/systemd/system/lamp-body.service",
            &body_unit(&body, ""),
        );
        write(
            "etc/systemd/system/lamp-controls.service",
            &controls_unit(&controls),
        );
        write(
            "etc/systemd/system/lamp-cmedia-speaker.service",
            "[Service]\nExecStart=/usr/bin/amixer set Speaker 80%\n",
        );
        write(
            "etc/systemd/system/lamp-update.service",
            &format!(
                "[Service]\nExecStart=/opt/lampOS/update/{}/lamp-update serve\n",
                sha("update 1")
            ),
        );
        let key = throwaway();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let now = Rc::new(Cell::new(1_000_000));
        let mut systemd = FakeSystemd {
            root: root.clone(),
            units: BTreeMap::new(),
            calls: Rc::clone(&calls),
            next_pid: 100,
            now: Rc::clone(&now),
            restarted_at: Vec::new(),
            lines: BTreeMap::new(),
            lines_asked: Vec::new(),
            crashing: Vec::new(),
            condition_skipped: Vec::new(),
            setup_properties: None,
            remove_setup_marker_on_query: false,
            unreachable_after_restart: false,
            unreachable: false,
            failing_after_restart: 0,
            failing: 0,
            applier: None,
            writer: None,
            back_began: None,
            cut_at_reload: false,
            self_recovery: Err("lamp-update-back.timer is not active".into()),
            self_recovery_queries: 0,
        };
        for unit in [
            "lamp-voice.service",
            "lamp-body.service",
            "lamp-controls.service",
        ] {
            systemd.start(unit);
        }
        let config = Config {
            root: root.clone(),
            units: "/etc/systemd/system/".into(),
            records: "/var/lib/lampos-update/".into(),
            channel: "lamp-voice".into(),
            channel_url: CHANNEL.into(),
            key: key.public_key().as_ref().try_into().unwrap(),
            lamp: "lamp-4ace".into(),
            apply: true,
            quiet_wait: Duration::from_secs(600),
            watch: Duration::from_secs(60),
            keep_free: 256_000_000,
            self_switch: false,
            running: None,
            updater: manifest::UPDATER,
            models: false,
            settings: false,
            product_settings: false,
            writer_from: None,
            image: false,
        };
        Self {
            _folder: folder,
            root,
            key,
            channel: FakeChannel::default(),
            systemd,
            calls,
            now,
            talking: None,
            free: 10_000_000_000,
            said: Vec::new(),
            phases: Vec::new(),
            config,
        }
    }

    /// Publishes build `sequence` with these binaries and these units of
    /// lamp-4ace's own folder, signed with the lamp's key.
    fn publish(
        &mut self,
        sequence: u64,
        binaries: &[(&str, &str)],
        units: &[(&str, &str)],
    ) -> Manifest {
        let manifest = self.manifest(sequence, binaries, units);
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let envelope = manifest::seal(&bytes, &self.key).unwrap();
        self.channel
            .files
            .insert(format!("{CHANNEL}{}", manifest::ENVELOPE), envelope);
        manifest
    }

    fn manifest(
        &mut self,
        sequence: u64,
        binaries: &[(&str, &str)],
        units: &[(&str, &str)],
    ) -> Manifest {
        let files_url = format!("https://channel.test/build-{sequence}/");
        let mut files = Vec::new();
        for (name, text) in binaries {
            self.channel
                .files
                .insert(format!("{files_url}{name}"), text.as_bytes().to_vec());
            files.push(File {
                name: (*name).into(),
                sha256: sha(text),
                size: text.len() as u64,
            });
        }
        let mut carried = Vec::new();
        for (name, text) in units {
            let unit = Unit {
                lamp: "lamp-4ace".into(),
                unit: (*name).into(),
                sha256: sha(text),
                size: text.len() as u64,
            };
            self.channel.files.insert(
                format!("{files_url}{}", unit.file_name()),
                text.as_bytes().to_vec(),
            );
            carried.push(unit);
        }
        Manifest {
            schema: 1,
            channel: "lamp-voice".into(),
            sequence,
            commit: format!("{sequence:040}"),
            files_url,
            files,
            units: carried,
            min_updater: None,
            models: Vec::new(),
            settings: Vec::new(),
            image: Vec::new(),
            reset_plan: None,
        }
    }

    fn round(&mut self) -> Outcome {
        self.round_observing(&mut |_| {})
    }

    /// Observe the existing journal callback, including a logical stop at
    /// a persisted self-update prefix. Every call constructs a fresh World.
    fn round_observing(&mut self, observe: &mut dyn FnMut(&str)) -> Outcome {
        let _rounding = rounding();
        let clock = FakeClock(Rc::clone(&self.now));
        let talking = self.talking.take().unwrap_or_else(|| Box::new(|_| false));
        let mut ears = FakeEars {
            now: Rc::clone(&self.now),
            talking,
            root: self.root.clone(),
        };
        let disk = FakeDisk(self.free);
        let said = &mut self.said;
        let mut say = |line: &str| {
            said.push(line.to_owned());
            observe(line);
        };
        let phases = &mut self.phases;
        let mut phase = |now: &'static str| phases.push(now);
        let mut world = World {
            channel: &mut self.channel,
            systemd: &mut self.systemd,
            ears: &mut ears,
            disk: &disk,
            clock: &clock,
            say: &mut say,
            phase: &mut phase,
        };
        let outcome = round(&self.config, &mut world);
        self.talking = Some(ears.talking);
        outcome
    }

    fn read(&self, path: &str) -> String {
        fs::read_to_string(self.root.join(path.trim_start_matches('/'))).unwrap()
    }

    fn exists(&self, path: &str) -> bool {
        self.root.join(path.trim_start_matches('/')).exists()
    }

    fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    /// Every file under the root but the records, with its bytes.
    fn files(&self) -> BTreeMap<String, Vec<u8>> {
        fn walk(folder: &Path, root: &Path, found: &mut BTreeMap<String, Vec<u8>>) {
            for entry in fs::read_dir(folder).unwrap() {
                let path = entry.unwrap().path();
                let name = path.strip_prefix(root).unwrap().display().to_string();
                if path.is_dir() {
                    found.insert(format!("{name}/"), Vec::new());
                    walk(&path, root, found);
                } else {
                    found.insert(name, fs::read(&path).unwrap());
                }
            }
        }
        let mut found = BTreeMap::new();
        walk(&self.root, &self.root, &mut found);
        found
    }

    /// The same, but lamp-update's own records (the lamp has nothing else
    /// under /var here).
    fn lamp_files(&self) -> BTreeMap<String, Vec<u8>> {
        self.files()
            .into_iter()
            .filter(|(name, _)| !name.starts_with("var/"))
            .collect()
    }

    fn state(&self) -> serde_json::Value {
        serde_json::from_str(&self.read("/var/lib/lampos-update/state.json")).unwrap()
    }
}

/// Build 2: a new voice and its worker; the body and the controls as they
/// are.
fn build_two(lamp: &mut Lamp) -> Manifest {
    lamp.publish(
        2,
        &[
            ("lamp-voice", "voice 2"),
            ("lamp-turn-end", "turn-end 2"),
            ("lamp-body", "body 1"),
            ("lamp-controls", "controls 1"),
        ],
        &[],
    )
}

/// The reviewer on #1263: a round is the likeliest reader to find damaged
/// records first, and it saves them. What it saves keeps the key paths
/// marked as once written, so a reset after it does not let prepare (or
/// install) copy V1's key, which may be the previous owner's.
#[test]
fn a_round_that_sets_damaged_records_aside_keeps_the_keys_marked() {
    use lamp_update::install;
    const V1_KEY: &str = "previous-owners-key-0123456789";
    let prepare = |root: &Path| {
        let _no_fork = no_fork();
        install::provision_device_key(root, install::RECORDS, &mut |_| {})
    };
    let keys = |root: &Path| {
        install::KEY_PATHS.map(|path| root.join(path.trim_start_matches('/')).exists())
    };
    for reset_first in [false, true] {
        let mut lamp = Lamp::new();
        let config = lamp.root.join("root/config/config.json");
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        fs::write(&config, format!(r#"{{"llm_api_key":"{V1_KEY}"}}"#)).unwrap();
        assert_eq!(prepare(&lamp.root).unwrap().len(), 2);
        let reset = |root: &Path| {
            for path in install::KEY_PATHS {
                fs::remove_file(root.join(path.trim_start_matches('/'))).unwrap();
            }
        };
        if reset_first {
            reset(&lamp.root);
        }
        let state = lamp.root.join("var/lib/lampos-update/state.json");
        fs::write(&state, b"{ not json").unwrap();
        build_two(&mut lamp);
        assert_eq!(lamp.round(), Outcome::Switched(2));
        assert!(state.exists(), "the round saved fresh records");
        if !reset_first {
            reset(&lamp.root);
        }
        assert!(
            prepare(&lamp.root).unwrap().is_empty(),
            "reset first: {reset_first}"
        );
        assert_eq!(
            keys(&lamp.root),
            [false, false],
            "reset first: {reset_first}"
        );
    }
}

#[test]
fn a_new_build_is_installed_beside_the_old_and_switched() {
    let mut lamp = Lamp::new();
    build_two(&mut lamp);
    let started = lamp.now.get();
    assert_eq!(lamp.round(), Outcome::Switched(2));
    // `update/status`'s phases, in order (#2194).
    assert_eq!(
        lamp.phases,
        ["checking", "fetching", "switching", "watching"]
    );

    let (old, new) = (sha("voice 1"), sha("voice 2"));
    // The new folder beside the old one: the new voice and worker, and the
    // model carried from the old folder.
    assert_eq!(
        lamp.read(&format!("/opt/lampOS/releases/{new}/lamp-voice")),
        "voice 2"
    );
    assert_eq!(
        lamp.read(&format!("/opt/lampOS/releases/{new}/lamp-turn-end")),
        "turn-end 2"
    );
    assert_eq!(
        lamp.read(&format!("/opt/lampOS/releases/{new}/smart-turn.onnx")),
        "a model"
    );
    assert_eq!(
        lamp.read(&format!("/opt/lampOS/releases/{old}/lamp-voice")),
        "voice 1"
    );
    assert_eq!(
        lamp.read("/etc/systemd/system/lamp-voice.service"),
        voice_unit(&new)
    );
    // Nothing else changed, and only the voice was restarted.
    assert_eq!(
        lamp.read("/etc/systemd/system/lamp-body.service"),
        body_unit(&sha("body 1"), "")
    );
    assert_eq!(lamp.calls(), ["reload", "restart lamp-voice.service"]);
    // At a quiet moment, 45 s after watching began, and then watched for a
    // minute.
    assert!(
        lamp.now.get() - started >= 105_000,
        "{}",
        lamp.now.get() - started
    );
    let state = lamp.state();
    assert_eq!(state["installed"]["sequence"], 2);
    assert_eq!(state["made"][0], format!("/opt/lampOS/releases/{new}/"));
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
    assert_eq!(
        lamp.read("/var/lib/lampos-update/previous/lamp-voice.service"),
        voice_unit(&old)
    );
    assert!(
        lamp.read("/var/lib/lampos-update/log.jsonl")
            .contains("\"event\":\"switched\"")
    );

    // The same build again: nothing to do.
    assert_eq!(lamp.round(), Outcome::UpToDate(2));
    assert_eq!(lamp.calls().len(), 2);
}

/// #2194: a round with nothing to change says only that it checked, and a
/// round without `apply` says no phase at all.
#[test]
fn a_round_with_nothing_to_change_only_checks() {
    let mut lamp = Lamp::new();
    build_two(&mut lamp);
    assert_eq!(lamp.round(), Outcome::Switched(2));
    lamp.phases.clear();
    assert_eq!(lamp.round(), Outcome::UpToDate(2));
    assert_eq!(lamp.phases, ["checking"]);
    lamp.phases.clear();
    lamp.config.apply = false;
    lamp.round();
    assert!(lamp.phases.is_empty(), "{:?}", lamp.phases);
}

#[test]
fn a_unit_the_build_carries_is_carried_with_the_build_written_in() {
    let mut lamp = Lamp::new();
    // The body's own unit in the repository names some older build: the
    // build is written in, the new option stays.
    let carried = body_unit(&"0".repeat(64), " --stand-share 50");
    lamp.publish(
        2,
        &[
            ("lamp-voice", "voice 1"),
            ("lamp-turn-end", "turn-end 1"),
            ("lamp-body", "body 1"),
            ("lamp-controls", "controls 1"),
        ],
        &[("lamp-body.service", &carried)],
    );
    assert_eq!(lamp.round(), Outcome::Switched(2));
    assert_eq!(
        lamp.read("/etc/systemd/system/lamp-body.service"),
        body_unit(&sha("body 1"), " --stand-share 50")
    );
    assert_eq!(lamp.calls(), ["reload", "restart lamp-body.service"]);
}

#[test]
fn a_bad_signature_or_a_bad_checksum_is_refused_with_nothing_installed() {
    let mut lamp = Lamp::new();
    let before = lamp.lamp_files();

    // Signed with another key.
    let manifest = build_two(&mut lamp);
    let other = throwaway();
    let envelope = manifest::seal(&serde_json::to_vec(&manifest).unwrap(), &other).unwrap();
    lamp.channel
        .files
        .insert(format!("{CHANNEL}{}", manifest::ENVELOPE), envelope);
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::Refused(why) if why.contains("signed with key")),
        "{outcome:?}"
    );

    // Signed rightly, and one binary is not what it says.
    build_two(&mut lamp);
    lamp.channel.files.insert(
        "https://channel.test/build-2/lamp-turn-end".into(),
        b"turn-end 2, changed on the way".to_vec(),
    );
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::Refused(why) if why.contains("lamp-turn-end")),
        "{outcome:?}"
    );

    assert_eq!(lamp.lamp_files(), before);
    assert!(lamp.calls().is_empty());
    // Nothing is recorded as installed: only the log says what was refused.
    assert!(!lamp.exists("/var/lib/lampos-update/state.json"));
    let log = lamp.read("/var/lib/lampos-update/log.jsonl");
    assert_eq!(log.matches("\"event\":\"refused\"").count(), 2);
}

#[test]
fn a_part_that_dies_is_put_back_and_its_build_refused() {
    let mut lamp = Lamp::new();
    lamp.publish(
        2,
        &[
            ("lamp-voice", "voice 2, which crashes"),
            ("lamp-turn-end", "turn-end 1"),
            ("lamp-body", "body 2"),
            ("lamp-controls", "controls 1"),
        ],
        &[],
    );
    let before = lamp.lamp_files();
    let outcome = lamp.round();
    // `update/status` says the put-back as its own phase, not as the
    // forward watch (the reviewer-lead on #2271).
    assert_eq!(lamp.phases.last(), Some(&"restoring"), "{:?}", lamp.phases);
    let Outcome::RolledBack {
        sequence,
        why,
        after,
        ..
    } = &outcome
    else {
        panic!("{outcome:?}");
    };
    assert_eq!(*sequence, 2);
    assert!(why.contains("lamp-voice.service ended"), "{why}");
    assert_eq!(after, &Ok(()));
    // These fixture units declare no ordering edge: both directions use
    // name order. The failed voice is put back without waiting.
    assert_eq!(
        lamp.calls(),
        [
            "reload",
            "restart lamp-body.service",
            "restart lamp-voice.service",
            "reload",
            "restart lamp-body.service",
            "restart lamp-voice.service",
        ]
    );
    // Every unit as it was, and the new folders removed again.
    assert_eq!(lamp.lamp_files(), before);
    assert_eq!(lamp.state()["refused"][0]["sequence"], 2);

    // The same build is not tried again.
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::Refused(why) if why.contains("was put back")),
        "{outcome:?}"
    );
    assert_eq!(lamp.calls().len(), 6);
}

/// #1505 (lamp-4ace, 18 Oct 13:51): a part not well says why in the
/// round's outcome, with how often systemd started it again and its own
/// last journal line, read before it is put back. Kept nowhere: not in the
/// refusal in `state.json` and not in `log.jsonl` (the reviewer on #1509),
/// since a part's line can carry what a person said.
#[test]
fn a_part_put_back_says_its_restarts_and_last_line_in_the_outcome_only() {
    let mut lamp = Lamp::new();
    let said = "voice: storage: unavailable; restart and inspect before retrying.";
    lamp.systemd
        .lines
        .insert("lamp-voice.service".into(), said.into());
    lamp.publish(
        2,
        &[
            ("lamp-voice", "voice 2, which crashes"),
            ("lamp-turn-end", "turn-end 1"),
            ("lamp-body", "body 2"),
            ("lamp-controls", "controls 1"),
        ],
        &[],
    );
    let outcome = lamp.round();
    let Outcome::RolledBack {
        why, after, told, ..
    } = &outcome
    else {
        panic!("{outcome:?}");
    };
    assert_eq!(
        told.new,
        format!(" (systemd started it again once; its last line: \"{said}\")")
    );
    assert_eq!(told.old, "");
    assert_eq!(after, &Ok(()));
    assert!(
        outcome.to_string().contains(&format!("{why}{}", told.new)),
        "{outcome}"
    );
    // Asked once, of the failing unit, before it was put back.
    assert_eq!(lamp.systemd.lines_asked, ["lamp-voice.service"]);
    let kept = lamp.state()["refused"][0]["why"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(&kept, why);
    let log = lamp.read("/var/lib/lampos-update/log.jsonl");
    assert!(log.contains(why.as_str()), "{log}");
    assert!(!log.contains(said) && !log.contains("last line"), "{log}");
}

/// Today's case: the old build is not well either. The first unit found
/// not well after the put-back (the watch stops there) says its own
/// restarts and line, after `after`'s reason; `kept()` leaves it out.
#[test]
fn the_old_build_not_well_either_says_the_first_unit_not_wells_line() {
    let mut lamp = Lamp::new();
    lamp.systemd.crashing = vec!["lamp-body.service".into()];
    lamp.systemd.lines.insert(
        "lamp-body.service".into(),
        "body: the store is newer than this build".into(),
    );
    lamp.publish(
        2,
        &[
            ("lamp-voice", "voice 1"),
            ("lamp-turn-end", "turn-end 1"),
            ("lamp-body", "body 2"),
            ("lamp-controls", "controls 1"),
        ],
        &[],
    );
    let outcome = lamp.round();
    let Outcome::RolledBack { after, told, .. } = &outcome else {
        panic!("{outcome:?}");
    };
    let line = " (systemd started it again once; its last line: \"body: the store is newer than this build\")";
    assert_eq!(told.new, line);
    assert_eq!(told.old, line);
    let Err(after) = after else {
        panic!("{outcome:?}");
    };
    assert!(!after.contains("last line"), "{after}");
    assert!(
        outcome.to_string().contains(&format!(
            "the previous build is not well either: {after}{line}"
        )),
        "{outcome}"
    );
    assert!(!outcome.kept().contains("last line"), "{}", outcome.kept());
}

/// A switch that goes well asks no line; a part not well that said nothing
/// and was not started again gives the outcome as before.
#[test]
fn no_line_is_asked_of_a_good_switch_and_none_is_said_when_there_is_none() {
    let mut lamp = Lamp::new();
    build_two(&mut lamp);
    assert!(matches!(lamp.round(), Outcome::Switched(2)));
    assert!(lamp.systemd.lines_asked.is_empty());

    let mut lamp = Lamp::new();
    lamp.publish(
        2,
        &[
            ("lamp-voice", "voice 1"),
            ("lamp-turn-end", "turn-end 1"),
            ("lamp-body", "body 2, unwell"),
            ("lamp-controls", "controls 1"),
        ],
        &[],
    );
    let outcome = lamp.round();
    let Outcome::RolledBack { why, told, .. } = &outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(lamp.state()["refused"][0]["why"], why.as_str());
    assert_eq!(told, &Default::default());
    assert_eq!(outcome.kept(), outcome.to_string());
}

/// systemd itself not answering is no part found not well (the reviewer on
/// #1509): the build is put back, and no part's line is asked or said.
#[test]
fn no_line_is_asked_when_systemd_itself_cannot_be_asked() {
    let mut lamp = Lamp::new();
    lamp.systemd
        .lines
        .insert("lamp-voice.service".into(), "voice: a line".into());
    lamp.systemd.unreachable_after_restart = true;
    build_two(&mut lamp);
    let outcome = lamp.round();
    let Outcome::RolledBack { why, told, .. } = &outcome else {
        panic!("{outcome:?}");
    };
    assert!(why.contains("systemd cannot be asked"), "{why}");
    assert_eq!(told, &Default::default());
    assert!(
        lamp.systemd.lines_asked.is_empty(),
        "{:?}",
        lamp.systemd.lines_asked
    );
    // It was asked again until the watch's end before putting back.
    let restarted = lamp.systemd.restarted_at[0].1;
    assert!(
        lamp.now.get() >= restarted + 60_000,
        "{}",
        lamp.now.get() - restarted
    );
}

/// A slow systemd makes the watch late, never a good build put back: a
/// state query that fails mid-watch is asked again, and the watch passes
/// once it answers.
#[test]
fn a_state_query_failing_mid_watch_is_asked_again_and_puts_nothing_back() {
    let mut lamp = Lamp::new();
    lamp.systemd.failing_after_restart = 3;
    build_two(&mut lamp);
    let outcome = lamp.round();
    assert!(matches!(outcome, Outcome::Switched(2)), "{outcome:?}");
    assert_eq!(lamp.systemd.failing, 0, "the failures were not reached");
    assert_eq!(
        lamp.systemd.restarted_at.len(),
        1,
        "{:?}",
        lamp.systemd.restarted_at
    );
}

/// The real systemd's line, from `journalctl -o json`: the last of the
/// unit's own processes (`_SYSTEMD_UNIT`), what systemd, systemd-coredump
/// and journald said about it skipped, and the old build's main process
/// (its last words as it stopped) skipped; non-text messages skipped;
/// control characters gone; cut at 200 characters.
#[test]
fn the_journal_line_is_the_units_own_last_cleaned_and_cut() {
    let unit = "lamp-tasks.service";
    let json = [
        r#"{"_SYSTEMD_UNIT":"lamp-tasks.service","_PID":"700","MESSAGE":"tasks: stopping"}"#,
        r#"{"_SYSTEMD_UNIT":"lamp-tasks.service","_PID":"812","MESSAGE":"tasks: starting"}"#,
        r#"{"_SYSTEMD_UNIT":"lamp-tasks.service","_PID":"812","MESSAGE":"tasks: storage:\tunavailable\u001b[0m"}"#,
        r#"{"_SYSTEMD_UNIT":"lamp-tasks.service","_PID":"813","MESSAGE":[104,105]}"#,
        r#"{"_SYSTEMD_UNIT":"init.scope","_PID":"1","MESSAGE":"lamp-tasks.service: Failed with result 'exit-code'."}"#,
        r#"{"_SYSTEMD_UNIT":"systemd-coredump@0.service","_PID":"900","MESSAGE":"Process 812 (lamp-tasks) dumped core."}"#,
        r#"{"_SYSTEMD_UNIT":"systemd-journald.service","_PID":"300","MESSAGE":"Suppressed 40 messages from lamp-tasks.service"}"#,
        "not json",
    ]
    .join("\n");
    assert_eq!(
        world::last_own_line(&json, unit, Some(700)).as_deref(),
        Some("tasks: storage: unavailable [0m")
    );
    // A new build that died without a word: the old one's stopping line is
    // never its reason.
    let silent = json.lines().take(1).collect::<Vec<_>>().join("\n");
    assert_eq!(world::last_own_line(&silent, unit, Some(700)), None);
    assert_eq!(
        world::last_own_line(&silent, unit, None).as_deref(),
        Some("tasks: stopping")
    );
    assert_eq!(world::last_own_line("", unit, None), None);
    let long = "x".repeat(300);
    let cut = world::clean_line(&long);
    assert_eq!(cut.chars().count(), world::LINE_MOST);
    assert!(cut.ends_with('…'));
    assert_eq!(world::clean_line("  short \n"), "short");
}

/// `journalctl` is bounded (the reviewer on #1509): one that never ends is
/// killed and gives nothing, quickly; one that prints more than a pipe
/// holds is read whole; one that fails gives nothing.
#[test]
fn a_command_is_read_within_its_bound_and_killed_after() {
    let _forking = forking();
    let began = std::time::Instant::now();
    let hung = world::output_within(
        std::process::Command::new("sleep").arg("30"),
        Duration::from_millis(300),
    );
    assert_eq!(hung, None);
    assert!(
        began.elapsed() < Duration::from_secs(5),
        "{:?}",
        began.elapsed()
    );
    let big = world::output_within(
        std::process::Command::new("sh").args(["-c", "head -c 300000 /dev/zero | tr '\\0' x"]),
        Duration::from_secs(20),
    )
    .unwrap();
    assert_eq!(big.len(), 300_000);
    assert_eq!(
        world::output_within(
            &mut std::process::Command::new("false"),
            Duration::from_secs(5)
        ),
        None
    );
}

#[test]
fn a_part_that_says_it_is_in_error_is_put_back_after_its_minute() {
    let mut lamp = Lamp::new();
    lamp.publish(
        2,
        &[
            ("lamp-voice", "voice 1"),
            ("lamp-turn-end", "turn-end 1"),
            ("lamp-body", "body 2, unwell"),
            ("lamp-controls", "controls 1"),
        ],
        &[],
    );
    let outcome = lamp.round();
    let Outcome::RolledBack { why, after, .. } = &outcome else {
        panic!("{outcome:?}");
    };
    assert!(why.contains("body says it is in error"), "{why}");
    assert_eq!(after, &Ok(()));
    assert_eq!(
        lamp.read("/etc/systemd/system/lamp-body.service"),
        body_unit(&sha("body 1"), "")
    );
}

#[test]
fn no_network_changes_nothing_and_says_so() {
    let mut lamp = Lamp::new();
    build_two(&mut lamp);
    lamp.channel.offline = true;
    let before = lamp.lamp_files();
    let outcome = lamp.round();
    assert_eq!(
        outcome,
        Outcome::Unreachable("the channel cannot be reached: no network".into())
    );
    assert_eq!(
        outcome.to_string(),
        "nothing changed: the channel cannot be reached: no network"
    );
    assert_eq!(lamp.lamp_files(), before);
    assert!(lamp.calls().is_empty());
    assert!(
        lamp.read("/var/lib/lampos-update/log.jsonl")
            .contains("unreachable")
    );

    // Nothing published yet is not an error either.
    let mut lamp = Lamp::new();
    assert_eq!(lamp.round(), Outcome::NothingPublished);
}

#[test]
fn the_voice_is_not_switched_while_someone_talks_to_the_lamp() {
    let mut lamp = Lamp::new();
    build_two(&mut lamp);
    // Someone speaks every 30 seconds, for the whole wait.
    lamp.talking = Some(Box::new(|at| at % 30_000 == 0));
    lamp.config.quiet_wait = Duration::from_secs(300);
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::NotQuiet(why) if why.contains("someone was heard")),
        "{outcome:?}"
    );
    // The wait's line gives the reason after a look at the bus, never the
    // empty gate's "says nothing" for a voice that says its state.
    let waiting: Vec<&String> = lamp
        .said
        .iter()
        .filter(|line| line.starts_with("waiting for a quiet moment"))
        .collect();
    assert_eq!(waiting.len(), 1, "{:?}", lamp.said);
    assert!(!waiting[0].contains("says nothing"), "{waiting:?}");
    assert!(lamp.calls().is_empty());
    let new = sha("voice 2");
    assert_eq!(
        lamp.read("/etc/systemd/system/lamp-voice.service"),
        voice_unit(&sha("voice 1"))
    );
    // Installed beside, waiting: the next quiet round switches to it.
    assert!(lamp.exists(&format!("/opt/lampOS/releases/{new}/lamp-voice")));
    lamp.talking = Some(Box::new(|_| false));
    assert_eq!(lamp.round(), Outcome::Switched(2));
    assert_eq!(
        lamp.read("/etc/systemd/system/lamp-voice.service"),
        voice_unit(&new)
    );
}

#[test]
fn a_disk_too_full_for_a_second_copy_is_refused_before_anything_is_written() {
    let mut lamp = Lamp::new();
    build_two(&mut lamp);
    lamp.free = 256_000_010;
    let before = lamp.lamp_files();
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::Refused(why) if why.starts_with("the disk has 256 MB free")),
        "{outcome:?}"
    );
    assert_eq!(lamp.lamp_files(), before);
    assert!(lamp.calls().is_empty());
}

#[test]
fn without_apply_nothing_is_written_and_what_would_change_is_said() {
    let mut lamp = Lamp::new();
    build_two(&mut lamp);
    lamp.config.apply = false;
    let before = lamp.files();
    let outcome = lamp.round();
    let Outcome::Would(lines) = &outcome else {
        panic!("{outcome:?}");
    };
    assert!(lines[0].starts_with("build 2 of lamp-voice"));
    assert!(
        lines[1].starts_with("lamp-voice.service: /opt/lampOS/releases/"),
        "{lines:?}"
    );
    // Everything as it was, the records too, and systemd not even asked.
    assert_eq!(lamp.files(), before);
    assert!(!lamp.exists("/var/lib/lampos-update"));
    assert!(lamp.calls().is_empty());
}

/// #1680: a build for a newer updater is refused whole, with nothing
/// fetched, written or restarted, and it is said why. Nothing is held
/// against the build: once the lamp's updater can apply it, it is taken.
#[test]
fn a_build_for_a_newer_updater_is_refused_and_taken_once_the_updater_can() {
    let mut lamp = Lamp::new();
    let before = lamp.lamp_files();
    let mut needing = build_two(&mut lamp);
    needing.min_updater = Some(manifest::UPDATER + 1);
    let bytes = serde_json::to_vec(&needing).unwrap();
    lamp.channel.files.insert(
        format!("{CHANNEL}{}", manifest::ENVELOPE),
        manifest::seal(&bytes, &lamp.key).unwrap(),
    );
    let fetched = lamp.channel.fetched.len();
    let outcome = lamp.round();
    let why = format!(
        "build 2 needs updater {} or newer, and this is updater {}; \
         it carries no lamp-update, so waiting for a build that does",
        manifest::UPDATER + 1,
        manifest::UPDATER
    );
    assert_eq!(outcome, Outcome::Refused(why.clone()));
    assert_eq!(
        outcome.to_string(),
        format!("refused, nothing switched: {why}")
    );
    // Only the envelope was read: no unit, no binary.
    assert_eq!(
        lamp.channel.fetched[fetched..],
        [format!("{CHANNEL}{}", manifest::ENVELOPE)]
    );
    assert_eq!(lamp.lamp_files(), before);
    assert!(lamp.calls().is_empty());
    assert!(!lamp.exists("/var/lib/lampos-update/state.json"));
    let log = lamp.read("/var/lib/lampos-update/log.jsonl");
    assert!(log.contains("\"event\":\"refused\""), "{log}");
    assert!(
        log.contains(&format!("needs updater {}", manifest::UPDATER + 1)),
        "{log}"
    );
    // Said again the next round, and still nothing changed.
    assert!(matches!(lamp.round(), Outcome::Refused(_)));
    assert_eq!(lamp.lamp_files(), before);

    // The same build for this updater: taken.
    needing.min_updater = Some(manifest::UPDATER);
    let bytes = serde_json::to_vec(&needing).unwrap();
    lamp.channel.files.insert(
        format!("{CHANNEL}{}", manifest::ENVELOPE),
        manifest::seal(&bytes, &lamp.key).unwrap(),
    );
    assert_eq!(lamp.round(), Outcome::Switched(2));
}

#[test]
fn an_older_build_is_not_taken() {
    let mut lamp = Lamp::new();
    build_two(&mut lamp);
    assert_eq!(lamp.round(), Outcome::Switched(2));
    lamp.publish(1, &[("lamp-voice", "voice 1")], &[]);
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::Refused(why) if why.contains("older")),
        "{outcome:?}"
    );
}

#[test]
fn a_switch_cut_short_is_put_back_at_the_next_start() {
    let mut lamp = Lamp::new();
    // As a switch to build 2 leaves it when the lamp loses power after the
    // units are written: the journal, the units kept, the new unit in place.
    let (old, new) = (sha("voice 1"), sha("voice 2"));
    let records = lamp.root.join("var/lib/lampos-update");
    fs::create_dir_all(records.join("previous")).unwrap();
    fs::write(
        records.join("previous/lamp-voice.service"),
        voice_unit(&old),
    )
    .unwrap();
    fs::write(
        records.join("journal.json"),
        format!(
            "{{\"build\":{{\"sequence\":2,\"commit\":\"{}\"}},\"units\":[{{\"unit\":\"lamp-voice.service\",\"was_active\":true}}]}}",
            "2".repeat(40)
        ),
    )
    .unwrap();
    fs::create_dir_all(lamp.root.join(format!("opt/lampOS/releases/{new}"))).unwrap();
    fs::write(
        lamp.root
            .join(format!("opt/lampOS/releases/{new}/lamp-voice")),
        "voice 2",
    )
    .unwrap();
    fs::write(
        lamp.root.join("etc/systemd/system/lamp-voice.service"),
        voice_unit(&new),
    )
    .unwrap();

    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::RolledBack { sequence: 2, why, after: Ok(()), .. } if why.contains("cut short")),
        "{outcome:?}"
    );
    assert_eq!(
        lamp.read("/etc/systemd/system/lamp-voice.service"),
        voice_unit(&old)
    );
    assert_eq!(lamp.calls(), ["reload", "restart lamp-voice.service"]);
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
    // Build 2 is not taken again once published.
    build_two(&mut lamp);
    assert!(matches!(lamp.round(), Outcome::Refused(why) if why.contains("was put back")));
}

#[test]
fn a_new_worker_beside_an_unchanged_voice_gets_a_folder_named_by_its_list() {
    let mut lamp = Lamp::new();
    lamp.publish(
        2,
        &[
            ("lamp-voice", "voice 1"),
            ("lamp-turn-end", "turn-end 2"),
            ("lamp-body", "body 1"),
            ("lamp-controls", "controls 1"),
        ],
        &[],
    );
    assert_eq!(lamp.round(), Outcome::Switched(2));
    // As SHA256SUMS would list the build's two binaries in it, in order.
    let mut lines = [
        format!("{}  lamp-turn-end\n", sha("turn-end 2")),
        format!("{}  lamp-voice\n", sha("voice 1")),
    ];
    lines.sort();
    let list = sha256_hex(lines.concat().as_bytes());
    assert_eq!(
        lamp.read("/etc/systemd/system/lamp-voice.service"),
        voice_unit(&list)
    );
    assert_eq!(
        lamp.read(&format!("/opt/lampOS/releases/{list}/lamp-turn-end")),
        "turn-end 2"
    );
    // The folder named by the voice itself is untouched.
    assert_eq!(
        lamp.read(&format!(
            "/opt/lampOS/releases/{}/lamp-turn-end",
            sha("voice 1")
        )),
        "turn-end 1"
    );
}

#[test]
fn a_folder_made_two_builds_ago_is_removed_and_one_never_made_is_kept() {
    let mut lamp = Lamp::new();
    for sequence in 2..=4u64 {
        let voice = format!("voice {sequence}");
        lamp.publish(
            sequence,
            &[("lamp-voice", &voice), ("lamp-turn-end", "turn-end 1")],
            &[],
        );
        assert_eq!(lamp.round(), Outcome::Switched(sequence));
    }
    let folder = |text: &str| format!("/opt/lampOS/releases/{}/", sha(text));
    // The build and the one before it stay, and the first, which this
    // program did not make.
    assert!(lamp.exists(&folder("voice 4")));
    assert!(lamp.exists(&folder("voice 3")));
    assert!(!lamp.exists(&folder("voice 2")));
    assert!(lamp.exists(&folder("voice 1")));
    let made: Vec<String> = serde_json::from_value(lamp.state()["made"].clone()).unwrap();
    assert_eq!(made, [folder("voice 3"), folder("voice 4")]);
}

#[test]
fn a_part_whose_folder_is_gone_is_installed_afresh() {
    let mut lamp = Lamp::new();
    fs::remove_dir_all(lamp.root.join(format!("opt/lampOS/body/{}", sha("body 1")))).unwrap();
    lamp.publish(2, &[("lamp-body", "body 2")], &[]);
    assert_eq!(lamp.round(), Outcome::Switched(2));
    assert_eq!(
        lamp.read(&format!("/opt/lampOS/body/{}/lamp-body", sha("body 2"))),
        "body 2"
    );
}

#[test]
fn a_voice_that_is_well_is_put_back_between_two_turns() {
    let mut lamp = Lamp::new();
    lamp.publish(
        2,
        &[
            ("lamp-voice", "voice 2"),
            ("lamp-turn-end", "turn-end 1"),
            ("lamp-body", "body 2, unwell"),
        ],
        &[],
    );
    // Quiet until the switch, 45 s after the round begins. Toward the end
    // of the minute the body is watched, someone talks to the new voice,
    // and goes on after the body has been found unwell.
    lamp.talking = Some(Box::new(|at| (1_100_000..=1_110_000).contains(&at)));
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::RolledBack { why, after: Ok(()), .. } if why.contains("body says it is in error")),
        "{outcome:?}"
    );
    let at: BTreeMap<&str, Vec<u64>> =
        lamp.systemd
            .restarted_at
            .iter()
            .fold(BTreeMap::new(), |mut at, (unit, when)| {
                at.entry(unit.as_str()).or_default().push(*when);
                at
            });
    // Switched at the quiet moment and watched for a minute; the body put
    // back at once; the voice only once five seconds have passed since the
    // last word.
    assert_eq!(at["lamp-voice.service"][0], 1_045_000);
    assert_eq!(at["lamp-body.service"], [1_045_000, 1_105_000]);
    assert!(at["lamp-voice.service"][1] >= 1_115_000, "{at:?}");
    assert!(at["lamp-voice.service"][1] <= 1_117_000, "{at:?}");
    assert_eq!(
        lamp.read("/etc/systemd/system/lamp-voice.service"),
        voice_unit(&sha("voice 1"))
    );
}

/// A product part (docs/install.md): started through its `current` link.
fn eyes_unit() -> String {
    "# every lamp's eyes\n[Unit]\nDescription=lampOS eyes\n\n[Service]\nExecStart=/opt/lampOS/eyes/current/lamp-eyes --device /dev/video0\nRestart=on-failure\n".into()
}

impl Lamp {
    /// lamp-4ace being moved to the product units: its eyes are a product
    /// part already, running build `text`, while the rest names its build.
    fn with_linked_eyes(mut self, text: &str) -> Self {
        let build = sha(text);
        let folder = self.root.join(format!("opt/lampOS/eyes/{build}"));
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("lamp-eyes"), text).unwrap();
        std::os::unix::fs::symlink(&build, self.root.join("opt/lampOS/eyes/current")).unwrap();
        fs::write(
            self.root.join("etc/systemd/system/lamp-eyes.service"),
            eyes_unit(),
        )
        .unwrap();
        self.systemd.start("lamp-eyes.service");
        self
    }

    fn eyes_link(&self) -> String {
        fs::read_link(self.root.join("opt/lampOS/eyes/current"))
            .unwrap()
            .display()
            .to_string()
    }
}

fn build_with_eyes(lamp: &mut Lamp, sequence: u64, voice: &str, eyes: &str) -> Manifest {
    lamp.publish(
        sequence,
        &[
            ("lamp-voice", voice),
            ("lamp-turn-end", "turn-end 1"),
            ("lamp-body", "body 1"),
            ("lamp-controls", "controls 1"),
            ("lamp-eyes", eyes),
        ],
        &[],
    )
}

#[test]
fn both_kinds_of_part_switch_in_one_round_each_its_own_way() {
    let mut lamp = Lamp::new().with_linked_eyes("eyes 1");
    let eyes_unit_before = lamp.read("/etc/systemd/system/lamp-eyes.service");
    build_with_eyes(&mut lamp, 2, "voice 2", "eyes 2");
    let outcome = lamp.round();
    assert_eq!(outcome, Outcome::Switched(2), "{:?}", lamp.said);

    // The voice as today: its unit names the new build.
    assert_eq!(
        lamp.read("/etc/systemd/system/lamp-voice.service"),
        voice_unit(&sha("voice 2"))
    );
    // The eyes by their link: the unit's bytes the same, the link moved.
    assert_eq!(
        lamp.read("/etc/systemd/system/lamp-eyes.service"),
        eyes_unit_before
    );
    assert_eq!(lamp.eyes_link(), sha("eyes 2"));
    assert_eq!(lamp.read("/opt/lampOS/eyes/current/lamp-eyes"), "eyes 2");
    assert!(lamp.exists(&format!("/opt/lampOS/eyes/{}/lamp-eyes", sha("eyes 1"))));
    // No ordering edge in these fixtures: name order, with one reload for
    // the voice's unit. Real product declarations are covered separately.
    assert_eq!(
        lamp.calls(),
        [
            "reload",
            "restart lamp-eyes.service",
            "restart lamp-voice.service"
        ]
    );
    // The way back by hand: the link and the build it named.
    assert_eq!(
        lamp.read("/var/lib/lampos-update/previous/lamp-eyes.service.link"),
        format!("/opt/lampOS/eyes/current {}\n", sha("eyes 1"))
    );
}

#[test]
fn a_linked_part_that_dies_is_put_back_by_its_link() {
    let mut lamp = Lamp::new().with_linked_eyes("eyes 1");
    build_with_eyes(&mut lamp, 2, "voice 2", "eyes 2, which crashes");
    let before = lamp.lamp_files();
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::RolledBack { sequence: 2, why, after: Ok(()), .. } if why.contains("lamp-eyes.service")),
        "{outcome:?}"
    );
    assert_eq!(lamp.eyes_link(), sha("eyes 1"));
    // The voice's unit and the eyes' link both back; the new folders gone.
    assert_eq!(lamp.lamp_files(), before);
    assert!(!lamp.exists(&format!(
        "/opt/lampOS/eyes/{}",
        sha("eyes 2, which crashes")
    )));
}

#[test]
fn a_link_switch_cut_short_is_put_back_at_the_next_start() {
    let mut lamp = Lamp::new().with_linked_eyes("eyes 1");
    // As a switch to build 2 leaves it when the power goes after the link
    // moved: the journal with the link as it was, the link at the new build.
    let (old, new) = (sha("eyes 1"), sha("eyes 2"));
    let records = lamp.root.join("var/lib/lampos-update");
    fs::create_dir_all(records.join("previous")).unwrap();
    fs::write(records.join("previous/lamp-eyes.service"), eyes_unit()).unwrap();
    fs::write(
        records.join("journal.json"),
        format!(
            "{{\"build\":{{\"sequence\":2,\"commit\":\"{}\"}},\"units\":[{{\"unit\":\"lamp-eyes.service\",\"was_active\":true,\"link\":{{\"path\":\"/opt/lampOS/eyes/current\",\"build\":\"{old}\"}}}}]}}",
            "2".repeat(40)
        ),
    )
    .unwrap();
    let folder = lamp.root.join(format!("opt/lampOS/eyes/{new}"));
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("lamp-eyes"), "eyes 2").unwrap();
    fs::remove_file(lamp.root.join("opt/lampOS/eyes/current")).unwrap();
    std::os::unix::fs::symlink(&new, lamp.root.join("opt/lampOS/eyes/current")).unwrap();

    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::RolledBack { sequence: 2, why, after: Ok(()), .. } if why.contains("cut short")),
        "{outcome:?}"
    );
    assert_eq!(lamp.eyes_link(), old);
    assert_eq!(
        lamp.read("/etc/systemd/system/lamp-eyes.service"),
        eyes_unit()
    );
    assert_eq!(lamp.calls(), ["reload", "restart lamp-eyes.service"]);
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
}

#[test]
fn a_linked_part_keeps_its_build_and_the_one_before() {
    let mut lamp = Lamp::new().with_linked_eyes("eyes 1");
    for (sequence, eyes) in [(2, "eyes 2"), (3, "eyes 3"), (4, "eyes 4")] {
        build_with_eyes(&mut lamp, sequence, "voice 1", eyes);
        assert_eq!(lamp.round(), Outcome::Switched(sequence), "{:?}", lamp.said);
    }
    assert_eq!(lamp.eyes_link(), sha("eyes 4"));
    let folder = |text: &str| format!("/opt/lampOS/eyes/{}", sha(text));
    // The link's build, and the one the link named before.
    assert!(lamp.exists(&folder("eyes 4")));
    assert!(lamp.exists(&folder("eyes 3")));
    // Made by this program two builds ago: removed.
    assert!(!lamp.exists(&folder("eyes 2")));
    // Installed by hand: never removed.
    assert!(lamp.exists(&folder("eyes 1")));
}

/// A lamp-skills from before #1698: no `list`, and no `[permissions]`
/// checks, so every folder loads. Any other command is a wrong option.
const SKILLS_BEFORE_CHECKS: &str =
    "#!/bin/sh\n# lamp-skills 1, before the permission checks\nexit 2\n";

/// A lamp-skills with #1698's checks, as `lamp-skills list --dir DIR`
/// prints them: a folder whose skill.toml gives no `[permissions]` is not
/// loaded. `refuses` names a folder it refuses whatever it says.
fn skills_with_checks(build: &str, refuses: &str) -> String {
    format!(
        "#!/bin/sh\n# lamp-skills {build}, with the permission checks\n\
         [ \"$1\" = list ] && [ \"$2\" = --dir ] || exit 2\n\
         echo 'NAME  KIND  VERSION  MAY USE'\n\
         for folder in \"$3\"/*/; do\n\
         name=$(basename \"$folder\")\n\
         if [ \"$name\" != '{refuses}' ] && grep -q '^\\[permissions\\]' \"$folder/skill.toml\" 2>/dev/null; then\n\
         echo \"$name  core  -  topics: voice/heard\"\n\
         else\n\
         echo \"$name  core  -  not loaded: voice/heard is not in [permissions] topics\"\n\
         fi\n\
         done\n"
    )
}

fn skills_unit() -> String {
    "[Unit]\nDescription=lampOS skills (lamp-skills)\n\n[Service]\nExecStart=/opt/lampOS/skills/current/lamp-skills --dir /var/lib/lampOS/skills --bus-root /run/lampos-bus\nRestart=on-failure\n".into()
}

impl Lamp {
    /// A product lamp's skills part, running `binary` through its link,
    /// and these skill folders, each a skill.toml.
    fn with_skills(mut self, binary: &str, folders: &[(&str, &str)]) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let build = sha(binary);
        let folder = self.root.join(format!("opt/lampOS/skills/{build}"));
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("lamp-skills"), binary).unwrap();
        fs::set_permissions(
            folder.join("lamp-skills"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        std::os::unix::fs::symlink(&build, self.root.join("opt/lampOS/skills/current")).unwrap();
        fs::write(
            self.root.join("etc/systemd/system/lamp-skills.service"),
            skills_unit(),
        )
        .unwrap();
        for (name, toml) in folders {
            self.write_skill(name, toml);
        }
        self.systemd.start("lamp-skills.service");
        self
    }

    fn write_skill(&self, name: &str, toml: &str) {
        let folder = self.root.join(format!("var/lib/lampOS/skills/{name}"));
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("skill.toml"), toml).unwrap();
    }

    fn skills_link(&self) -> String {
        fs::read_link(self.root.join("opt/lampOS/skills/current"))
            .unwrap()
            .display()
            .to_string()
    }
}

fn build_with_skills(lamp: &mut Lamp, sequence: u64, skills: &str) -> Manifest {
    lamp.publish(
        sequence,
        &[
            ("lamp-voice", "voice 1"),
            ("lamp-turn-end", "turn-end 1"),
            ("lamp-body", "body 1"),
            ("lamp-controls", "controls 1"),
            ("lamp-skills", skills),
        ],
        &[],
    )
}

/// A skill folder from before #1698 declares no `[permissions]`.
const OLD_SKILL: &str = "name = \"dance\"\n[[watch]]\ntopic = \"voice/heard\"\n";
/// The same skill as the build's own declares what it reads.
const NEW_SKILL: &str = "name = \"dance\"\n[permissions]\ntopics = [\"voice/heard\"]\n[[watch]]\ntopic = \"voice/heard\"\n";

/// The first builder on #1722 (00:28): a round moves lamp-skills onto a
/// build with #1698's checks while the lamp's folders are from before
/// them, and every skill is refused, with lamp-skills up and well. The
/// round runs the build's own lamp-skills on the folders first: a skill
/// that loads now and would not under the build keeps lamp-skills, and
/// every part, where it is. Once the folders are the build's, it switches,
/// and they load.
#[test]
fn a_round_does_not_switch_lamp_skills_onto_checks_its_old_folders_fail() {
    let mut lamp = Lamp::new().with_skills(
        SKILLS_BEFORE_CHECKS,
        &[("dance", OLD_SKILL), ("reminders", OLD_SKILL)],
    );
    let old = sha(SKILLS_BEFORE_CHECKS);
    let checks = skills_with_checks("2", "");
    build_with_skills(&mut lamp, 2, &checks);
    let units_before: BTreeMap<String, Vec<u8>> = lamp
        .lamp_files()
        .into_iter()
        .filter(|(name, _)| name.starts_with("etc/"))
        .collect();

    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::Refused(why)
            if why.contains("dance (voice/heard is not in [permissions] topics)")
                && why.contains("reminders")
                && why.contains("/var/lib/lampOS/skills")),
        "{outcome:?}"
    );
    // Nothing switched: no unit written, no link moved, nothing restarted,
    // no journal, and the lamp still runs build 1.
    assert_eq!(lamp.skills_link(), old);
    assert!(lamp.calls().is_empty(), "{:?}", lamp.calls());
    let units_after: BTreeMap<String, Vec<u8>> = lamp
        .lamp_files()
        .into_iter()
        .filter(|(name, _)| name.starts_with("etc/"))
        .collect();
    assert_eq!(units_after, units_before);
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
    assert!(lamp.state()["installed"].is_null());

    // The build's skills placed (as `lamp-update install` does): the next
    // round switches, and under the new lamp-skills both load.
    lamp.write_skill("dance", NEW_SKILL);
    lamp.write_skill("reminders", NEW_SKILL);
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.skills_link(), sha(&checks));
    assert_eq!(lamp.calls(), ["reload", "restart lamp-skills.service"]);
    let forking = forking();
    let listed =
        std::process::Command::new(lamp.root.join("opt/lampOS/skills/current/lamp-skills"))
            .args(["list", "--dir"])
            .arg(lamp.root.join("var/lib/lampOS/skills"))
            .output()
            .unwrap();
    drop(forking);
    let listed = String::from_utf8(listed.stdout).unwrap();
    assert!(!listed.contains("not loaded"), "{listed}");
    assert!(
        listed.contains("dance") && listed.contains("reminders"),
        "{listed}"
    );
}

/// A folder the running lamp-skills already refuses (a lamp's own, broken
/// before the round) is no reason to keep the old build: only a skill that
/// loads now and would not after holds the switch back. The build's
/// lamp-skills that cannot list the folder at all switches nothing.
#[test]
fn only_a_skill_that_would_stop_loading_holds_lamp_skills_back() {
    let mut lamp = Lamp::new().with_skills(
        &skills_with_checks("1", "mine"),
        &[("dance", NEW_SKILL), ("mine", NEW_SKILL)],
    );
    let checks = skills_with_checks("2", "mine");
    build_with_skills(&mut lamp, 2, &checks);
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.skills_link(), sha(&checks));

    let broken = "#!/bin/sh\n# lamp-skills 3, which cannot read the folder\necho 'no such folder' >&2\nexit 1\n";
    build_with_skills(&mut lamp, 3, broken);
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::Failed(why) if why.contains("no such folder")),
        "{outcome:?}"
    );
    assert_eq!(lamp.skills_link(), sha(&checks));
    assert_eq!(lamp.calls(), ["reload", "restart lamp-skills.service"]);
}

/// Install's product skills folder, as the round names its files.
const PRODUCT_SKILLS: &str = "var/lib/lampOS/skills";

impl Lamp {
    /// Records these files as install wrote them: the build's before.
    fn record_as_installs(&self, files: &[(&str, &str)]) {
        let settings: serde_json::Map<String, serde_json::Value> = files
            .iter()
            .map(|(path, text)| (format!("/{PRODUCT_SKILLS}/{path}"), sha(text).into()))
            .collect();
        let state = serde_json::json!({
            "installed": null, "previous": null, "refused": [], "made": [],
            "settings": settings,
        });
        let at = self.root.join("var/lib/lampos-update/state.json");
        fs::create_dir_all(at.parent().unwrap()).unwrap();
        fs::write(at, serde_json::to_vec(&state).unwrap()).unwrap();
    }

    fn put_skill_file(&self, path: &str, text: &str) {
        let at = self.root.join(format!("{PRODUCT_SKILLS}/{path}"));
        fs::create_dir_all(at.parent().unwrap()).unwrap();
        fs::write(at, text).unwrap();
    }

    fn skill_file(&self, path: &str) -> Option<String> {
        fs::read_to_string(self.root.join(format!("{PRODUCT_SKILLS}/{path}"))).ok()
    }

    /// What `lamp-skills list` of the running build says of the folder.
    fn listed(&self) -> String {
        let forking = forking();
        let listed =
            std::process::Command::new(self.root.join("opt/lampOS/skills/current/lamp-skills"))
                .args(["list", "--dir"])
                .arg(self.root.join(PRODUCT_SKILLS))
                .output()
                .unwrap();
        drop(forking);
        String::from_utf8(listed.stdout).unwrap()
    }
}

/// The product skills a build carries, as `lamp-update manifest` writes
/// them.
fn bundle(files: &[(&str, &str)]) -> String {
    let mut skills: Vec<&str> = files
        .iter()
        .map(|(path, _)| path.split('/').next().unwrap())
        .collect();
    skills.sort_unstable();
    skills.dedup();
    let files: Vec<serde_json::Value> = files
        .iter()
        .map(|(path, text)| serde_json::json!({"path": path, "text": text}))
        .collect();
    serde_json::to_string(&serde_json::json!({
        "format": "lampos-skills/1", "skills": skills, "files": files,
    }))
    .unwrap()
}

/// The build's dance and reminders, with #1698's `[permissions]`, and a new
/// product skill with a file in a folder of its own.
fn product_skills_of_build() -> String {
    bundle(&[
        ("dance/skill.toml", NEW_SKILL),
        ("dance/SKILL.md", "Dance, the build's."),
        ("reminders/skill.toml", NEW_SKILL),
        ("reminders/SKILL.md", "Reminders, the build's."),
        ("welcome-back/skill.toml", NEW_SKILL),
        ("welcome-back/lights/warm_glow.toml", "glow"),
    ])
}

fn build_with_product_skills(
    lamp: &mut Lamp,
    sequence: u64,
    voice: &str,
    skills: &str,
    product: &str,
) -> Manifest {
    lamp.publish(
        sequence,
        &[
            ("lamp-voice", voice),
            ("lamp-turn-end", "turn-end 1"),
            ("lamp-body", "body 1"),
            ("lamp-controls", "controls 1"),
            ("lamp-skills", skills),
            ("lamp-product-skills", product),
        ],
        &[],
    )
}

/// A lamp with pre-#1698 product skill folders, as install wrote them, a
/// file of one changed on the lamp, and a skill the person installed.
fn lamp_before_permissions() -> Lamp {
    let lamp = Lamp::new().with_skills(
        SKILLS_BEFORE_CHECKS,
        &[
            ("dance", OLD_SKILL),
            ("reminders", OLD_SKILL),
            ("mine", NEW_SKILL),
        ],
    );
    lamp.put_skill_file("dance/SKILL.md", "Dance, install's.");
    lamp.put_skill_file(
        "reminders/SKILL.md",
        "Reminders, as this lamp's person wrote them.",
    );
    lamp.put_skill_file("mine/SKILL.md", "Mine.");
    lamp.record_as_installs(&[
        ("dance/skill.toml", OLD_SKILL),
        ("dance/SKILL.md", "Dance, install's."),
        ("reminders/skill.toml", OLD_SKILL),
        ("reminders/SKILL.md", "Reminders, install's."),
    ]);
    lamp
}

/// #2014's P2 (the reviewer, 01:22; the first builder, 01:29): a lamp
/// whose product skills are from before #1698, given a round to a build
/// with #1698's checks, gets the build's product skills from the channel
/// and switches, every one loaded.
///
/// The first builder on #1722 (21 Oct, 16:2x): a core skill's files are
/// the build's, not the person's, so a file of one removed by hand comes
/// back and one changed is written over, each said. What #1913 keeps is the
/// person's: the skills they turned off or on and a skill's state, in
/// lamp-skills' own folder, and a skill they installed. None is touched.
#[test]
fn a_round_puts_a_core_skills_files_back_and_keeps_the_persons_own() {
    let mut lamp = lamp_before_permissions();
    fs::remove_file(lamp.root.join(format!("{PRODUCT_SKILLS}/dance/SKILL.md"))).unwrap();
    let state_dir = lamp.root.join("var/lib/lampos-skills-state");
    fs::create_dir_all(&state_dir).unwrap();
    let off = "reminders\n";
    let daily = "{\"consumed\":[]}\n";
    fs::write(state_dir.join("off"), off).unwrap();
    fs::write(state_dir.join("daily.json"), daily).unwrap();
    let checks = skills_with_checks("2", "");
    build_with_product_skills(&mut lamp, 2, "voice 1", &checks, &product_skills_of_build());
    let mine_before = (
        lamp.skill_file("mine/skill.toml"),
        lamp.skill_file("mine/SKILL.md"),
    );

    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.skills_link(), sha(&checks));
    assert_eq!(lamp.skill_file("dance/skill.toml").unwrap(), NEW_SKILL);
    assert_eq!(
        lamp.skill_file("dance/SKILL.md").unwrap(),
        "Dance, the build's."
    );
    assert_eq!(lamp.skill_file("reminders/skill.toml").unwrap(), NEW_SKILL);
    assert_eq!(
        lamp.skill_file("welcome-back/lights/warm_glow.toml")
            .unwrap(),
        "glow"
    );
    // A core skill's file changed by hand is the build's again.
    assert_eq!(
        lamp.skill_file("reminders/SKILL.md").unwrap(),
        "Reminders, the build's."
    );
    for said in [
        "put back /var/lib/lampOS/skills/dance/SKILL.md: removed on this lamp",
        "put back /var/lib/lampOS/skills/reminders/SKILL.md: changed on this lamp",
    ] {
        assert!(
            lamp.said.iter().any(|line| line.contains(said)),
            "{said}: {:?}",
            lamp.said
        );
    }
    // The person's: Reminders still off, its state, and their own skill.
    assert_eq!(fs::read_to_string(state_dir.join("off")).unwrap(), off);
    assert_eq!(
        fs::read_to_string(state_dir.join("daily.json")).unwrap(),
        daily
    );
    assert_eq!(
        (
            lamp.skill_file("mine/skill.toml"),
            lamp.skill_file("mine/SKILL.md")
        ),
        mine_before
    );
    let listed = lamp.listed();
    assert!(!listed.contains("not loaded"), "{listed}");
    for name in ["dance", "reminders", "welcome-back", "mine"] {
        assert!(listed.contains(name), "{name}: {listed}");
    }
    // Recorded as the build's: the next build's are written over them.
    let state = lamp.state();
    assert_eq!(
        state["settings"]["/var/lib/lampOS/skills/dance/skill.toml"],
        sha(NEW_SKILL)
    );
    assert!(state["settings"]["/var/lib/lampOS/skills/mine/skill.toml"].is_null());
    assert!(!lamp.exists("/var/lib/lampos-update/skills-staged"));
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
}

/// The first builder on #2585 (22:24): a core skill someone turned on stays
/// on through a round. The build ships Dance off; the lamp turned it on by
/// hand. The round writes the build's file turned on and keeps the choice in
/// lamp-skills' state folder, beside `off`.
#[test]
fn a_core_skill_turned_on_stays_on_through_a_round() {
    let mut lamp = lamp_before_permissions();
    let off = format!("enabled = false\n{NEW_SKILL}");
    lamp.put_skill_file("dance/skill.toml", &format!("enabled = true\n{NEW_SKILL}"));
    let state_dir = lamp.root.join("var/lib/lampos-skills-state");
    fs::create_dir_all(&state_dir).unwrap();
    let checks = skills_with_checks("2", "");
    let product = bundle(&[
        ("dance/skill.toml", off.as_str()),
        ("dance/SKILL.md", "Dance, the build's."),
        ("reminders/skill.toml", NEW_SKILL),
        ("reminders/SKILL.md", "Reminders, the build's."),
    ]);
    build_with_product_skills(&mut lamp, 2, "voice 1", &checks, &product);

    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    let on = format!("enabled = true\n{NEW_SKILL}");
    assert_eq!(lamp.skill_file("dance/skill.toml").unwrap(), on);
    assert_eq!(fs::read_to_string(state_dir.join("on")).unwrap(), "dance\n");
    assert_eq!(
        lamp.state()["settings"]["/var/lib/lampOS/skills/dance/skill.toml"],
        sha(&on)
    );
}

/// The first builder on #2585 (22:24): a skill the build removes is gone
/// whatever was chosen. Its name leaves the record of the core skills
/// turned on, with one journal line, and the rest of the record stays.
#[test]
fn a_skill_the_build_removes_leaves_the_turned_on_record_with_one_line() {
    let mut lamp = lamp_before_permissions();
    let state_dir = lamp.root.join("var/lib/lampos-skills-state");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("on"), "dance\nold-skill\n").unwrap();
    let checks = skills_with_checks("2", "");
    build_with_product_skills(&mut lamp, 2, "voice 1", &checks, &product_skills_of_build());

    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    let lines: Vec<&String> = lamp
        .said
        .iter()
        .filter(|line| line.contains("old-skill"))
        .collect();
    assert_eq!(lines.len(), 1, "{:?}", lamp.said);
    assert!(
        lines[0].starts_with("old-skill is no longer a core skill of this build"),
        "{lines:?}"
    );
    assert_eq!(fs::read_to_string(state_dir.join("on")).unwrap(), "dance\n");
}

/// The app's off or on for `name` (#2815), asked of the manage helper's own
/// code over this lamp's skills folder and lamp-skills' state folder.
fn app_turns(lamp: &Lamp, way: &str, name: &str) -> String {
    let skills = lamp.root.join(PRODUCT_SKILLS);
    let mut answer = Vec::new();
    lamp_skills::package::asked(
        &skills,
        &skills.join("no-made"),
        &lamp.root.join("var/lib/lampos-skills-state"),
        std::time::Duration::from_secs(1),
        None,
        format!("{{\"{way}\": \"{name}\"}}\n").as_bytes(),
        &mut answer,
    )
    .unwrap();
    let answer: serde_json::Value = serde_json::from_slice(&answer).unwrap();
    answer["done"]
        .as_str()
        .unwrap_or_else(|| panic!("{answer}"))
        .to_owned()
}

/// lamp-skills started again over this lamp's folders: the skills it runs,
/// and the ones it holds turned off.
fn restarted(lamp: &Lamp) -> (Vec<String>, Vec<String>) {
    let (skills, errors) = lamp_skills::load_all(&lamp.root.join(PRODUCT_SKILLS));
    assert!(errors.is_empty(), "{errors:?}");
    let mut engine = lamp_skills::Engine::new(skills, true);
    let off = engine
        .keep_off_in(&lamp.root.join("var/lib/lampos-skills-state"))
        .unwrap();
    let names = engine
        .skills()
        .iter()
        .map(|skill| skill.name.clone())
        .collect();
    (names, off)
}

/// #2815: the app's off and on are the person's own choice, kept through a
/// round and a restart. An off is kept in lamp-skills' `off`, which a round
/// never touches. A core skill the build ships off, turned on in the app,
/// is on in its file and in `on`, so the round keeps it on (#2585); turned
/// off again, its file is the build's again, it leaves `on`, and it stays
/// off through the next round and a restart.
#[test]
fn the_apps_off_and_on_hold_through_a_round_and_a_restart() {
    let repo = |path: &str| {
        fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../skills")
                .join(path),
        )
        .unwrap()
    };
    let dance_on = repo("dance/skill.toml");
    let dance_off = format!("{}{dance_on}", lamp_update::install::OFF_LINE);
    let files = [
        ("dance/skill.toml", dance_off.clone()),
        ("dance/SKILL.md", repo("dance/SKILL.md")),
        ("reminders/skill.toml", repo("reminders/skill.toml")),
        ("reminders/SKILL.md", repo("reminders/SKILL.md")),
    ];
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, text)| (*path, text.as_str()))
        .collect();
    let mut lamp = Lamp::new().with_skills(&skills_with_checks("1", ""), &[]);
    for (path, text) in &files {
        lamp.put_skill_file(path, text);
    }
    lamp.record_as_installs(&files);
    let state_dir = lamp.root.join("var/lib/lampos-skills-state");
    fs::create_dir_all(&state_dir).unwrap();
    let product = bundle(&files);
    let names = |text: &str| -> Vec<String> { text.lines().map(str::to_owned).collect() };

    assert_eq!(app_turns(&lamp, "on", "dance"), "turned dance on");
    assert_eq!(app_turns(&lamp, "off", "reminders"), "turned reminders off");
    let checks = skills_with_checks("2", "");
    build_with_product_skills(&mut lamp, 2, "voice 1", &checks, &product);
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.skill_file("dance/skill.toml").unwrap(), dance_on);
    assert_eq!(fs::read_to_string(state_dir.join("on")).unwrap(), "dance\n");
    assert_eq!(
        fs::read_to_string(state_dir.join("off")).unwrap(),
        "reminders\n"
    );
    let (running, off) = restarted(&lamp);
    assert_eq!(
        (running, off),
        (names("dance\nreminders"), names("reminders"))
    );

    // On again and off again, the other way round.
    assert_eq!(app_turns(&lamp, "on", "reminders"), "turned reminders on");
    assert_eq!(app_turns(&lamp, "off", "dance"), "turned dance off");
    assert_eq!(lamp.skill_file("dance/skill.toml").unwrap(), dance_off);
    let checks = skills_with_checks("3", "");
    build_with_product_skills(&mut lamp, 3, "voice 1", &checks, &product);
    assert_eq!(lamp.round(), Outcome::Switched(3), "{:?}", lamp.said);
    assert_eq!(lamp.skill_file("dance/skill.toml").unwrap(), dance_off);
    assert_eq!(fs::read_to_string(state_dir.join("on")).unwrap(), "");
    assert_eq!(
        fs::read_to_string(state_dir.join("off")).unwrap(),
        "dance\n"
    );
    let (running, off) = restarted(&lamp);
    assert_eq!((running, off), (names("reminders"), names("dance")));
}

/// The guard stays the net: a skill of the person's that the new
/// lamp-skills would not load refuses the build, judged on the folder as
/// the round would leave it, and nothing of the lamp's is written.
#[test]
fn the_guard_still_refuses_a_persons_skill_the_new_lamp_skills_will_not_load() {
    let mut lamp = lamp_before_permissions();
    lamp.write_skill("mine", OLD_SKILL);
    let checks = skills_with_checks("2", "");
    build_with_product_skills(&mut lamp, 2, "voice 1", &checks, &product_skills_of_build());
    // The skills, the units: what the lamp runs and reads.
    let lamps = |lamp: &Lamp| -> BTreeMap<String, Vec<u8>> {
        lamp.files()
            .into_iter()
            .filter(|(name, _)| name.starts_with("var/lib/lampOS/") || name.starts_with("etc/"))
            .collect()
    };
    let before = lamps(&lamp);

    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::Refused(why)
            if why.contains("mine (voice/heard is not in [permissions] topics)")
                && !why.contains("dance")
                && !why.contains("reminders")),
        "{outcome:?}"
    );
    assert!(lamp.calls().is_empty(), "{:?}", lamp.calls());
    // Not a file of the lamp's written, and nothing staged left behind.
    assert!(lamps(&lamp) == before, "a file of the lamp's was written");
    assert!(!lamp.exists("/var/lib/lampos-update/skills-staged"));
    assert_eq!(lamp.skills_link(), sha(SKILLS_BEFORE_CHECKS));
}

/// A build put back puts the product skills back too: each file as it was,
/// a folder made for the build gone, the records as before.
#[test]
fn a_build_put_back_puts_the_product_skills_back() {
    let mut lamp = lamp_before_permissions();
    let checks = skills_with_checks("2", "");
    build_with_product_skills(
        &mut lamp,
        2,
        "voice 2, which crashes",
        &checks,
        &product_skills_of_build(),
    );
    let outcome = lamp.round();
    assert!(
        matches!(
            &outcome,
            Outcome::RolledBack {
                sequence: 2,
                after: Ok(()),
                ..
            }
        ),
        "{outcome:?}"
    );
    assert_eq!(lamp.skill_file("dance/skill.toml").unwrap(), OLD_SKILL);
    assert_eq!(
        lamp.skill_file("dance/SKILL.md").unwrap(),
        "Dance, install's."
    );
    assert_eq!(lamp.skill_file("reminders/skill.toml").unwrap(), OLD_SKILL);
    // A file written over is put back as the lamp had it: it was written
    // (a lamp that kept it would say "kept"), then restored.
    assert!(
        lamp.said.iter().any(|line| line
            .contains("put back /var/lib/lampOS/skills/reminders/SKILL.md: changed on this lamp")),
        "{:?}",
        lamp.said
    );
    assert_eq!(
        lamp.skill_file("reminders/SKILL.md").unwrap(),
        "Reminders, as this lamp's person wrote them."
    );
    assert!(!lamp.exists(&format!("/{PRODUCT_SKILLS}/welcome-back")));
    assert_eq!(lamp.skills_link(), sha(SKILLS_BEFORE_CHECKS));
    assert_eq!(
        lamp.state()["settings"]["/var/lib/lampOS/skills/dance/skill.toml"],
        sha(OLD_SKILL)
    );
}

/// A switch cut short after the skills were written puts them back at the
/// next start, from the journal: a file still as written goes back, a
/// folder made for the build goes, and a file the lamp changed since stays.
#[test]
fn product_skills_written_by_a_switch_cut_short_are_put_back_at_the_next_start() {
    let mut lamp = lamp_before_permissions();
    // As a switch to build 2 leaves it when the lamp loses power after the
    // skills are written.
    let back = |path: &str, before: Option<&str>, wrote: &str, made: &[&str]| {
        serde_json::json!({
            "path": format!("/{PRODUCT_SKILLS}/{path}"),
            "before": before,
            "wrote": sha(wrote),
            "made": made.iter().map(|folder| format!("/{PRODUCT_SKILLS}/{folder}")).collect::<Vec<_>>(),
        })
    };
    let journal = serde_json::json!({
        "build": {"sequence": 2, "commit": "2".repeat(40)},
        "units": [],
        "skills": [
            back("dance/skill.toml", Some(OLD_SKILL), NEW_SKILL, &[]),
            back("dance/SKILL.md", Some("Dance, install's."), "Dance, the build's.", &[]),
            back("welcome-back/skill.toml", None, NEW_SKILL, &["welcome-back"]),
        ],
    });
    fs::write(
        lamp.root.join("var/lib/lampos-update/journal.json"),
        serde_json::to_vec(&journal).unwrap(),
    )
    .unwrap();
    lamp.put_skill_file("dance/skill.toml", NEW_SKILL);
    lamp.put_skill_file("dance/SKILL.md", "Dance, changed on the lamp after.");
    lamp.put_skill_file("welcome-back/skill.toml", NEW_SKILL);

    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::RolledBack { sequence: 2, why, .. } if why.contains("cut short")),
        "{outcome:?}"
    );
    assert_eq!(lamp.skill_file("dance/skill.toml").unwrap(), OLD_SKILL);
    assert_eq!(
        lamp.skill_file("dance/SKILL.md").unwrap(),
        "Dance, changed on the lamp after."
    );
    assert!(!lamp.exists(&format!("/{PRODUCT_SKILLS}/welcome-back")));
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
}

/// A build that changes only the product skills places them, restarting
/// nothing: lamp-skills reloads its folder by itself.
#[test]
fn a_build_of_only_new_product_skills_places_them_and_restarts_nothing() {
    let mut lamp = Lamp::new().with_skills(&skills_with_checks("1", ""), &[("dance", NEW_SKILL)]);
    lamp.record_as_installs(&[("dance/skill.toml", NEW_SKILL)]);
    let skills = skills_with_checks("1", "");
    let newer = NEW_SKILL.replace("dance\"", "dance\"\nabout = \"newer\"");
    build_with_product_skills(
        &mut lamp,
        2,
        "voice 1",
        &skills,
        &bundle(&[("dance/skill.toml", &newer)]),
    );
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert_eq!(lamp.skill_file("dance/skill.toml").unwrap(), newer);
    assert!(lamp.calls().is_empty(), "{:?}", lamp.calls());
    assert_eq!(lamp.state()["installed"]["sequence"], 2);
}

/// A lamp-skills reading a folder of the lamp's own (lamp-4ace's) is not
/// given the product skills; and a round that cannot write install's folder
/// (a lamp-update.service without it in `ReadWritePaths`) fails with
/// nothing switched, and the build is not refused.
#[test]
fn product_skills_go_only_into_installs_folder_and_only_when_it_can_be_written() {
    use std::os::unix::fs::PermissionsExt;
    let mut lamp = lamp_before_permissions();
    let unit = lamp.read("/etc/systemd/system/lamp-skills.service");
    fs::write(
        lamp.root.join("etc/systemd/system/lamp-skills.service"),
        unit.replace("--dir /var/lib/lampOS/skills", "--dir /var/lib/lamp-skills"),
    )
    .unwrap();
    build_with_product_skills(
        &mut lamp,
        2,
        "voice 1",
        SKILLS_BEFORE_CHECKS,
        &product_skills_of_build(),
    );
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert_eq!(lamp.skill_file("dance/skill.toml").unwrap(), OLD_SKILL);

    let mut lamp = lamp_before_permissions();
    let folder = lamp.root.join(PRODUCT_SKILLS);
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap();
    let checks = skills_with_checks("2", "");
    build_with_product_skills(&mut lamp, 2, "voice 1", &checks, &product_skills_of_build());
    let outcome = lamp.round();
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        matches!(&outcome, Outcome::Failed(why) if why.contains("ReadWritePaths")),
        "{outcome:?}"
    );
    assert!(lamp.calls().is_empty(), "{:?}", lamp.calls());
    assert!(lamp.state()["refused"].as_array().unwrap().is_empty());
    assert_eq!(lamp.skill_file("dance/skill.toml").unwrap(), OLD_SKILL);
}

/// The machine as `lamp-update install` asks it, for a lamp made of files:
/// users go into the root's /etc/passwd, and nothing else is done.
struct InstallHost(PathBuf);

impl lamp_update::install::Host for InstallHost {
    fn add_user(&mut self, name: &str, _: &[String]) -> Result<(), String> {
        let passwd = self.0.join("etc/passwd");
        let mut text = fs::read_to_string(&passwd).unwrap_or_default();
        text.push_str(&format!(
            "{name}:x:999:999::/nonexistent:/usr/sbin/nologin\n"
        ));
        fs::write(passwd, text).unwrap();
        Ok(())
    }

    fn enable(&mut self, _: &[String]) -> Result<(), String> {
        Ok(())
    }

    fn tmpfiles(&mut self, _: &str) -> Result<(), String> {
        Ok(())
    }

    fn utc_offset_minutes(&mut self, _: &str) -> Result<i32, String> {
        Ok(420)
    }

    fn harness_at_boot(&mut self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }

    fn harness_off(&mut self, _: &str, _: &str) -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }

    fn harness_on(&mut self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }

    fn harness_version(&mut self, _: &str, _: &str) -> Result<String, String> {
        Ok(String::new())
    }

    fn restart(&mut self, _: &str) -> Result<(), String> {
        Ok(())
    }

    fn running_sha256(&mut self) -> Result<String, String> {
        Err("not asked of this lamp".into())
    }

    fn handed_over(&mut self) -> bool {
        false
    }

    fn hand_over(&mut self, _: &std::path::Path) -> String {
        "not asked of this lamp".into()
    }
}

/// Every binary the product starts, and the voice's worker, each the line
/// `<name> <sequence>`; lamp-skills a script that lists the folders, since
/// a round runs it before a switch.
fn product_build(lamp: &mut Lamp, sequence: u64) -> Manifest {
    let texts: Vec<(String, String)> = lamp_update::install::parts()
        .into_iter()
        .map(|part| part.binary)
        .chain(["lamp-turn-end".to_owned()])
        .map(|name| {
            let text = if name == "lamp-skills" {
                skills_with_checks(&sequence.to_string(), "")
            } else {
                format!("{name} {sequence}")
            };
            (name, text)
        })
        .collect();
    let binaries: Vec<(&str, &str)> = texts
        .iter()
        .map(|(name, text)| (name.as_str(), text.as_str()))
        .collect();
    lamp.publish(sequence, &binaries, &[])
}

impl Lamp {
    /// A blank lamp given build 1 by `lamp-update install`, every part
    /// running through its `current` link.
    fn installed() -> Self {
        Self::installed_with_updater("lamp-update 1")
    }

    /// The same signed install, with an executable updater for cut tests.
    fn installed_with_updater(updater: &str) -> Self {
        let mut lamp = Lamp::new();
        for folder in ["etc", "opt"] {
            fs::remove_dir_all(lamp.root.join(folder)).unwrap();
        }
        fs::create_dir_all(lamp.root.join("etc")).unwrap();
        fs::write(lamp.root.join("etc/hostname"), "lamp-4ace\n").unwrap();
        fs::write(
            lamp.root.join("etc/passwd"),
            "root:x:0:0::/root:/bin/bash\n",
        )
        .unwrap();
        fs::write(
            lamp.root.join("etc/group"),
            "root:x:0:\naudio:x:29:\nvideo:x:44:\ndialout:x:20:\n",
        )
        .unwrap();
        fs::write(lamp.root.join("etc/timezone"), "Asia/Ho_Chi_Minh\n").unwrap();
        product_build_with_updater(&mut lamp, 1, updater);
        let config = lamp_update::install::Install {
            root: lamp.root.clone(),
            channel: "lamp-voice".into(),
            channel_url: CHANNEL.into(),
            key: lamp.config.key,
            records: lamp.config.records.clone(),
            setup: false,
            models: Vec::new(),
        };
        let mut host = InstallHost(lamp.root.clone());
        let no_fork = no_fork();
        lamp_update::install::install(&config, &mut lamp.channel, &mut host, &mut |_| {}).unwrap();
        drop(no_fork);
        lamp.systemd.units.clear();
        for entry in fs::read_dir(lamp.root.join("etc/systemd/system")).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            let text = fs::read_to_string(entry.path()).unwrap_or_default();
            if name.ends_with(".service") && unit::linked_program(&text, &lamp.root).is_some() {
                lamp.systemd.start(&name);
            }
        }
        lamp
    }
}

/// #1680: install records lamp-update's own folder as made, and its unit
/// starts through `update/current`. A round never switches lamp-update, so
/// its first switch must not remove that folder: the lamp would not start
/// lamp-update again after the next boot.
#[test]
fn a_round_after_install_keeps_lamp_updates_own_folder() {
    let mut lamp = Lamp::installed();
    let link = lamp.root.join("opt/lampOS/update/current");
    let own = fs::read_link(&link).unwrap();
    product_build(&mut lamp, 2);
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(fs::read_link(&link).unwrap(), own);
    assert!(
        lamp.exists("/opt/lampOS/update/current/lamp-update"),
        "update/current names a folder that is gone: {:?}",
        lamp.said
    );
    assert_eq!(
        lamp.read("/opt/lampOS/update/current/lamp-update"),
        "lamp-update 1"
    );
}

/// #1940, the reviewer's P1 and the first builder's 00:11: a lamp installed
/// before the second route was the lamps' voice has no `second.json`. A
/// round whose build's voice unit asks for it places it as install does,
/// private and install's own, before the unit is written, and the voice
/// comes up on it.
#[test]
fn a_round_places_second_json_before_a_voice_unit_that_needs_it() {
    use lamp_update::install;
    use std::os::unix::fs::PermissionsExt;
    let mut lamp = Lamp::new();
    assert!(!lamp.exists(install::SECOND_PATH));
    let second = format!(
        "{}Environment=LAMP_VOICE_ROUTE=second\nEnvironment=LAMP_VOICE_SECOND_CONFIG={}\n",
        voice_unit(&"0".repeat(64)),
        install::SECOND_PATH
    );
    lamp.publish(
        2,
        &[
            ("lamp-voice", "voice 2"),
            ("lamp-turn-end", "turn-end 2"),
            ("lamp-body", "body 1"),
            ("lamp-controls", "controls 1"),
        ],
        &[("lamp-voice.service", &second)],
    );
    assert_eq!(lamp.round(), Outcome::Switched(2));
    assert_eq!(lamp.read(install::SECOND_PATH), install::SECOND);
    let mode = std::fs::metadata(lamp.root.join(install::SECOND_PATH.trim_start_matches('/')))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    assert!(
        lamp.read("/etc/systemd/system/lamp-voice.service")
            .contains("LAMP_VOICE_ROUTE=second")
    );
    assert_eq!(lamp.calls(), ["reload", "restart lamp-voice.service"]);
    // Install's own now, so a later install keeps it up to date.
    assert_eq!(
        lamp.state()["settings"][install::SECOND_PATH],
        sha(install::SECOND)
    );
}

/// #2978, El's P1: the product's voice unit no longer names DeepSeek's
/// address. A round onto a build carrying that unit keeps the
/// `second.json` install placed (the previous updater's `SECOND`), and the
/// voice reading that kept file under the unit's environment still reaches
/// DeepSeek through the company relay, with no `deepseek.key` and nothing
/// pinned, so `LAMP_RELAY_ROUTES` may route its chat.
#[test]
fn a_round_keeping_second_json_still_reaches_deepseek_through_the_relay() {
    use lamp_update::install;
    let product = include_str!("../../../deploy/product/lamp-voice.service");
    assert!(!product.contains("Environment=LAMP_SECOND_DEEPSEEK_BASE_URL"));
    let mut lamp = Lamp::installed();
    let kept = lamp.read(install::SECOND_PATH);
    let texts: Vec<(String, String)> = lamp_update::install::parts()
        .into_iter()
        .map(|part| part.binary)
        .chain(["lamp-turn-end".to_owned()])
        .map(|name| {
            let text = if name == "lamp-skills" {
                skills_with_checks("2", "")
            } else {
                format!("{name} 2")
            };
            (name, text)
        })
        .collect();
    let binaries: Vec<(&str, &str)> = texts
        .iter()
        .map(|(name, text)| (name.as_str(), text.as_str()))
        .collect();
    lamp.publish(2, &binaries, &[("lamp-voice.service", product)]);
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.read(install::SECOND_PATH), kept);
    let unit = lamp.read("/etc/systemd/system/lamp-voice.service");
    let environment: std::collections::HashMap<String, std::ffi::OsString> = unit
        .lines()
        .filter_map(|line| line.strip_prefix("Environment="))
        .filter_map(|pair| pair.split_once('='))
        .map(|(name, value)| (name.to_owned(), value.into()))
        .collect();
    // The kept file as the voice reads it, its key at a path in the test's
    // root.
    use std::os::unix::fs::PermissionsExt;
    let private = |path: &std::path::Path, text: &str| {
        fs::write(path, text).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    };
    let key = lamp.root.join("relay-credential");
    private(&key, "relay-secret\n");
    let read = lamp.root.join("second-as-read.json");
    private(
        &read,
        &kept.replace("/etc/lampOS/voice/credential", &key.display().to_string()),
    );
    let config = lamp_voice::second_config::load_from_path(&read)
        .unwrap()
        .with_environment(|name| environment.get(name).cloned())
        .unwrap();
    assert_eq!(
        config.hosted.deepseek_base_url.as_deref(),
        Some("https://campaign-api.autonomous.ai/api/v1/ai/v1/deepseek")
    );
    assert!(!config.pinned.chat);
}

/// The same round when `second.json` cannot be placed (here a folder in
/// its place): nothing is written, the old voice unit stays and nothing is
/// restarted, so the voice that runs keeps running.
#[test]
fn a_round_that_cannot_place_second_json_writes_no_unit() {
    use lamp_update::install;
    let mut lamp = Lamp::new();
    std::fs::create_dir_all(lamp.root.join(install::SECOND_PATH.trim_start_matches('/'))).unwrap();
    let second = format!(
        "{}Environment=LAMP_VOICE_ROUTE=second\nEnvironment=LAMP_VOICE_SECOND_CONFIG={}\n",
        voice_unit(&"0".repeat(64)),
        install::SECOND_PATH
    );
    lamp.publish(
        2,
        &[
            ("lamp-voice", "voice 2"),
            ("lamp-turn-end", "turn-end 2"),
            ("lamp-body", "body 1"),
            ("lamp-controls", "controls 1"),
        ],
        &[("lamp-voice.service", &second)],
    );
    assert!(
        matches!(lamp.round(), Outcome::Failed(why) if why.contains("second.json cannot be placed"))
    );
    assert_eq!(
        lamp.read("/etc/systemd/system/lamp-voice.service"),
        voice_unit(&sha("voice 1"))
    );
    assert!(lamp.calls().is_empty(), "{:?}", lamp.calls());
}

/// Build `sequence` of the product, with lamp-update's own binary `updater`.
fn product_build_with_updater(lamp: &mut Lamp, sequence: u64, updater: &str) -> Manifest {
    let texts: Vec<(String, String)> = lamp_update::install::parts()
        .into_iter()
        .map(|part| part.binary)
        .chain(["lamp-turn-end".to_owned()])
        .map(|name| {
            let text = if name == "lamp-update" {
                updater.to_owned()
            } else if name == "lamp-skills" {
                skills_with_checks(&sequence.to_string(), "")
            } else {
                format!("{name} {sequence}")
            };
            (name, text)
        })
        .collect();
    let binaries: Vec<(&str, &str)> = texts
        .iter()
        .map(|(name, text)| (name.as_str(), text.as_str()))
        .collect();
    lamp.publish(sequence, &binaries, &[])
}

/// What `systemctl show` says of lamp-update.service.
fn update_unit(active: &str, sub: &str) -> UnitState {
    UnitState {
        unit: "lamp-update.service".into(),
        active: active.into(),
        sub: sub.into(),
        pid: 0,
        restarts: 0,
    }
}

impl Lamp {
    /// Installed, with lamp-update switching itself (`LAMP_UPDATE_SELF=on`).
    fn installed_switching_itself() -> Self {
        let mut lamp = Lamp::installed();
        lamp.config.self_switch = true;
        // Install enables the timer; this fixture also models it started.
        lamp.systemd.self_recovery = Ok(());
        lamp
    }

    fn records(&self) -> PathBuf {
        self.root.join("var/lib/lampos-update")
    }

    /// The build `update/current` names.
    fn own_build(&self) -> String {
        fs::read_link(self.root.join("opt/lampOS/update/current"))
            .unwrap()
            .display()
            .to_string()
    }

    /// The new lamp-update confirming itself, as the build `running` from:
    /// only after a dry round has read and checked the channel's manifest.
    fn confirm(&mut self, running: &str) -> own::Confirm {
        // Confirmation takes the records lock again after its dry round.
        // Keep that complete operation isolated from other fixture launches,
        // including rounds that may start a process themselves.
        let _forking = forking();
        let clock = FakeClock(Rc::clone(&self.now));
        let mut ears = FakeEars {
            now: Rc::clone(&self.now),
            talking: Box::new(|_| false),
            root: self.root.clone(),
        };
        let disk = FakeDisk(self.free);
        let said = &mut self.said;
        let mut say = |line: &str| said.push(line.to_owned());
        let mut world = World {
            channel: &mut self.channel,
            systemd: &mut self.systemd,
            ears: &mut ears,
            disk: &disk,
            clock: &clock,
            say: &mut say,
            phase: &mut |_| {},
        };
        own::confirm(&self.config, &mut world, Some(running)).unwrap()
    }

    /// lamp-update-back, as its timer or `OnFailure=` starts it.
    fn back(&mut self, unit: UnitState, now_unix_ms: u64) -> own::Back {
        let _no_fork = no_fork();
        let records = self.records();
        let said = &mut self.said;
        own::back(&records, &self.root, now_unix_ms, &unit, &mut |line| {
            said.push(line.to_owned())
        })
        .unwrap()
    }

    fn pending(&self) -> Option<own::Pending> {
        own::pending(&self.records()).unwrap()
    }
}

/// #1680: a new lamp-update that starts and confirms itself keeps its
/// switch: the fallback leaves it, and the next round is as any other.
#[test]
fn a_new_updater_that_confirms_keeps_its_switch() {
    let mut lamp = Lamp::installed_switching_itself();
    let old = lamp.own_build();
    product_build_with_updater(&mut lamp, 2, "lamp-update 2");
    let new = sha("lamp-update 2");
    assert_eq!(
        lamp.round(),
        Outcome::SelfSwitched {
            sequence: 2,
            from: old.clone(),
            to: new.clone()
        },
        "{:?}",
        lamp.said
    );
    assert_eq!(lamp.own_build(), new);
    assert_eq!(
        lamp.read("/opt/lampOS/update/current/lamp-update"),
        "lamp-update 2"
    );
    let pending = lamp.pending().unwrap();
    assert_eq!(
        (pending.old.as_str(), pending.new.as_str()),
        (old.as_str(), new.as_str())
    );

    assert_eq!(lamp.confirm(&new), own::Confirm::Confirmed);
    assert!(lamp.pending().is_none());
    // Neither a failure later nor the deadline puts back a confirmed one.
    assert_eq!(
        lamp.back(update_unit("failed", "failed"), u64::MAX),
        own::Back::Nothing
    );
    assert_eq!(lamp.own_build(), new);
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert_eq!(lamp.own_build(), new);
}

/// #1680, the first builder's test: a new binary that exits at once,
/// before it has read anything, never confirms; `OnFailure=` starts the
/// fallback, which puts the old link back and refuses that binary.
#[test]
fn a_new_updater_that_exits_at_once_is_put_back() {
    let mut lamp = Lamp::installed_switching_itself();
    let old = lamp.own_build();
    product_build_with_updater(&mut lamp, 2, "#!/bin/sh\nexit 1\n");
    let new = sha("#!/bin/sh\nexit 1\n");
    assert!(
        matches!(lamp.round(), Outcome::SelfSwitched { .. }),
        "{:?}",
        lamp.said
    );
    assert_eq!(lamp.own_build(), new);
    let forking = forking();
    let status =
        std::process::Command::new(lamp.root.join("opt/lampOS/update/current/lamp-update"))
            .args(["serve", "--channel-url", CHANNEL])
            .status()
            .unwrap();
    drop(forking);
    assert_eq!(status.code(), Some(1));
    assert!(lamp.pending().is_some(), "it exited without confirming");

    let back = lamp.back(update_unit("failed", "failed"), records_now());
    assert!(matches!(back, own::Back::PutBack(_)), "{back:?}");
    assert_eq!(lamp.own_build(), old);
    assert_eq!(
        lamp.read("/opt/lampOS/update/current/lamp-update"),
        "lamp-update 1"
    );
    assert!(lamp.pending().is_none());
    assert_eq!(own::refused(&lamp.records()), vec![new.clone()]);
    // The old updater, started again, does not take that binary again.
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert_eq!(lamp.own_build(), old);
}

/// #1680: a new lamp-update that runs but never confirms (it hangs) is put
/// back by the deadline's timer, and not before.
#[test]
fn a_new_updater_not_confirmed_by_the_deadline_is_put_back() {
    let mut lamp = Lamp::installed_switching_itself();
    let old = lamp.own_build();
    product_build_with_updater(&mut lamp, 2, "lamp-update 2");
    assert!(matches!(lamp.round(), Outcome::SelfSwitched { .. }));
    let at = lamp.pending().unwrap().at_unix_ms;
    let running = update_unit("active", "running");
    assert_eq!(lamp.back(running.clone(), at + 60_000), own::Back::Waiting);
    assert_eq!(lamp.own_build(), sha("lamp-update 2"));
    let late = at + own::DEADLINE.as_millis() as u64 + 1;
    assert!(matches!(lamp.back(running, late), own::Back::PutBack(_)));
    assert_eq!(lamp.own_build(), old);
    assert!(lamp.pending().is_none());
}

/// #1680, the race: every round, the confirmation and the fallback take
/// the one lock of the records, and no round starts while a switch of
/// lamp-update's own waits to be confirmed: a restart in phase 2 (or the
/// old binary run by hand) never begins a second round.
#[test]
fn no_second_round_begins_while_lamp_updates_own_switch_waits() {
    let mut lamp = Lamp::installed_switching_itself();
    product_build_with_updater(&mut lamp, 2, "lamp-update 2");
    let new = sha("lamp-update 2");
    assert!(matches!(lamp.round(), Outcome::SelfSwitched { .. }));
    let restarts = lamp.calls().len();

    // Not yet confirmed: a round is refused and changes nothing.
    let before = lamp.lamp_files();
    product_build_with_updater(&mut lamp, 3, "lamp-update 3");
    let refused = lamp.round();
    assert!(
        matches!(&refused, Outcome::Refused(why) if why.contains("not confirmed")),
        "{refused:?}"
    );
    assert_eq!(lamp.lamp_files(), before);
    assert_eq!(lamp.calls().len(), restarts, "nothing restarted");

    // A process that is not the new build does not confirm it.
    let old = lamp.pending().unwrap().old;
    assert_eq!(lamp.confirm(&old), own::Confirm::NotOwn);
    assert!(lamp.pending().is_some());

    // While another holds the lock, none of them acts.
    let held = {
        let _no_fork = no_fork();
        lamp_update::records::Records::new(lamp.records())
            .lock()
            .unwrap()
    };
    assert_eq!(lamp.confirm(&new), own::Confirm::Busy);
    assert_eq!(
        lamp.back(update_unit("failed", "failed"), u64::MAX),
        own::Back::Busy
    );
    assert!(matches!(lamp.round(), Outcome::Failed(_)));
    assert_eq!(lamp.own_build(), new);
    drop(held);

    assert_eq!(lamp.confirm(&new), own::Confirm::Confirmed);
    assert!(
        matches!(lamp.round(), Outcome::SelfSwitched { sequence: 3, .. }),
        "{:?}",
        lamp.said
    );
}

/// #1680: with `LAMP_UPDATE_SELF` off (or absent) a round is as before:
/// lamp-update's own link stays, nothing is waited for, and the fallback
/// and the confirmation have nothing to do.
#[test]
fn with_the_switch_off_lamp_update_never_switches_itself() {
    assert!(!own::on(None));
    assert!(!own::on(Some("off")));
    assert!(!own::on(Some("yes")));
    assert!(own::on(Some("on")));

    let mut lamp = Lamp::installed();
    assert!(!lamp.config.self_switch);
    let old = lamp.own_build();
    product_build_with_updater(&mut lamp, 2, "lamp-update 2");
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.own_build(), old);
    assert!(lamp.pending().is_none());
    assert!(!lamp.exists(&format!("/opt/lampOS/update/{}", sha("lamp-update 2"))));
    assert_eq!(lamp.systemd.self_recovery_queries, 0);
    let before = lamp.files();
    assert_eq!(lamp.confirm(&old), own::Confirm::Nothing);
    assert_eq!(
        lamp.back(update_unit("failed", "failed"), u64::MAX),
        own::Back::Nothing
    );
    assert_eq!(lamp.files(), before, "nothing written");
}

/// #1680: install writes the fallback at its fixed path, outside what
/// lamp-update.service may write, and starts it from lamp-update's failure
/// and from its timer; no round touches it, not even one that switches
/// lamp-update itself.
#[test]
fn install_writes_the_fallback_and_no_round_touches_it() {
    use std::os::unix::fs::PermissionsExt;
    let mut lamp = Lamp::installed_switching_itself();
    assert_eq!(lamp.read(own::BACK), "lamp-update 1");
    let mode = fs::metadata(lamp.root.join(own::BACK.trim_start_matches('/')))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o755);

    let unit = lamp.read("/etc/systemd/system/lamp-update.service");
    assert!(
        unit.lines()
            .any(|line| line == "OnFailure=lamp-update-back.service")
    );
    let writes = unit
        .lines()
        .find_map(|line| line.strip_prefix("ReadWritePaths="))
        .unwrap();
    assert!(
        !writes
            .split_whitespace()
            .any(|path| own::BACK.starts_with(path.trim_start_matches('-'))),
        "{writes}"
    );
    let back = lamp.read("/etc/systemd/system/lamp-update-back.service");
    assert!(
        back.contains(&format!("ExecStart={} back", own::BACK)),
        "{back}"
    );
    let timer = lamp.read("/etc/systemd/system/lamp-update-back.timer");
    assert!(timer.contains("Unit=lamp-update-back.service"), "{timer}");

    product_build_with_updater(&mut lamp, 2, "lamp-update 2");
    assert!(matches!(lamp.round(), Outcome::SelfSwitched { .. }));
    lamp.confirm(&sha("lamp-update 2"));
    product_build_with_updater(&mut lamp, 3, "lamp-update 3");
    assert!(matches!(lamp.round(), Outcome::SelfSwitched { .. }));
    assert_eq!(lamp.read(own::BACK), "lamp-update 1");
}

/// Now, on the clock the records keep their times by.
fn records_now() -> u64 {
    lamp_update::records::unix_ms()
}

/// #1680, the first builder at 11:40: a new lamp-update that starts but
/// cannot check a manifest (here, it reads another key) never confirms,
/// however often it tries, and the deadline puts the old one back. One
/// that cannot reach the channel does not confirm either.
#[test]
fn a_new_updater_that_refuses_every_manifest_is_put_back() {
    let mut lamp = Lamp::installed_switching_itself();
    let old = lamp.own_build();
    product_build_with_updater(&mut lamp, 2, "lamp-update 2");
    let new = sha("lamp-update 2");
    assert!(matches!(lamp.round(), Outcome::SelfSwitched { .. }));
    let key = lamp.config.key;
    lamp.config.key = throwaway().public_key().as_ref().try_into().unwrap();
    for _ in 0..3 {
        let confirm = lamp.confirm(&new);
        assert!(matches!(confirm, own::Confirm::NotProven(_)), "{confirm:?}");
    }
    lamp.config.key = key;
    lamp.channel.offline = true;
    let confirm = lamp.confirm(&new);
    assert!(matches!(confirm, own::Confirm::NotProven(_)), "{confirm:?}");
    assert!(lamp.pending().is_some());
    let late = lamp.pending().unwrap().at_unix_ms + own::DEADLINE.as_millis() as u64 + 1;
    let back = lamp.back(update_unit("active", "running"), late);
    assert!(matches!(back, own::Back::PutBack(_)), "{back:?}");
    assert_eq!(lamp.own_build(), old);
}

/// #1680, the first builder at 11:40: `self.json` is a contract for good.
/// lamp-update-back is written once, by install, so the one a lamp has
/// reads what any later lamp-update writes: a newer version, with fields it
/// does not know, still names the old build and the new and when.
#[test]
fn the_fallback_reads_a_self_json_written_by_a_later_updater() {
    let mut lamp = Lamp::installed_switching_itself();
    let old = lamp.own_build();
    product_build_with_updater(&mut lamp, 2, "lamp-update 2");
    let new = sha("lamp-update 2");
    assert!(matches!(lamp.round(), Outcome::SelfSwitched { .. }));
    let path = lamp.records().join("self.json");
    let written: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(written["version"], own::VERSION);
    let later = format!(
        r#"{{"version":5,"old":"{old}","new":"{new}","at_unix_ms":{},"why":"a field of V5's","models":["x"]}}"#,
        records_now()
    );
    fs::write(&path, later).unwrap();
    let back = lamp.back(update_unit("failed", "failed"), records_now());
    assert!(matches!(back, own::Back::PutBack(_)), "{back:?}");
    assert_eq!(lamp.own_build(), old);
    assert_eq!(own::refused(&lamp.records()), vec![new]);
}

/// #1680, the first builder's P3 on #1737: a round started by hand, on a
/// lamp with no unit of lamp-update's own to name its folder, keeps the
/// folder the running binary is in.
#[test]
fn a_round_by_hand_keeps_the_folder_it_runs_from() {
    let mut lamp = Lamp::installed();
    fs::remove_file(lamp.root.join("etc/systemd/system/lamp-update.service")).unwrap();
    let running = format!("/opt/lampOS/update/{}/", lamp.own_build());
    lamp.config.running = Some(running.clone());
    product_build(&mut lamp, 2);
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert!(
        lamp.exists(&format!("{running}lamp-update")),
        "{:?}",
        lamp.said
    );
}

/// #1680, the first builder's P3 on #1737: after phase 1, `update/current`
/// names the new build while the old binary still runs; a round it makes
/// then keeps the old binary's folder.
#[test]
fn a_round_after_its_own_switch_keeps_the_old_binarys_folder() {
    let mut lamp = Lamp::installed_switching_itself();
    let old = format!("/opt/lampOS/update/{}/", lamp.own_build());
    product_build_with_updater(&mut lamp, 2, "lamp-update 2");
    assert!(matches!(lamp.round(), Outcome::SelfSwitched { .. }));
    assert_eq!(lamp.confirm(&sha("lamp-update 2")), own::Confirm::Confirmed);
    // The old binary, still running, makes a round that prunes.
    lamp.config.running = Some(old.clone());
    product_build_with_updater(&mut lamp, 3, "lamp-update 2");
    assert_eq!(lamp.round(), Outcome::Switched(3), "{:?}", lamp.said);
    assert!(lamp.exists(&format!("{old}lamp-update")), "{:?}", lamp.said);
}

/// The build `manifest` again, signed with `min_updater` set to `needed`.
fn needing(lamp: &mut Lamp, mut manifest: Manifest, needed: u64) -> Manifest {
    manifest.min_updater = Some(needed);
    let bytes = serde_json::to_vec(&manifest).unwrap();
    lamp.channel.files.insert(
        format!("{CHANNEL}{}", manifest::ENVELOPE),
        manifest::seal(&bytes, &lamp.key).unwrap(),
    );
    manifest
}

/// Every file of the lamp but lamp-update's own folders and link and its
/// records: the parts, their units and links.
fn parts_files(lamp: &Lamp) -> BTreeMap<String, Vec<u8>> {
    lamp.lamp_files()
        .into_iter()
        .filter(|(name, _)| !name.starts_with("opt/lampOS/update/"))
        .collect()
}

/// #1680, the first builder's rule: an updater too old for a build takes
/// the newer lamp-update it carries and nothing else; that one confirms
/// itself, reads the same build again and takes the rest.
#[test]
fn an_updater_too_old_for_a_build_takes_only_the_updater_it_carries_then_the_rest() {
    let mut lamp = Lamp::installed_switching_itself();
    lamp.config.updater = 1;
    let old = lamp.own_build();
    let parts = parts_files(&lamp);
    let build = product_build_with_updater(&mut lamp, 2, "lamp-update 2");
    needing(&mut lamp, build, 2);
    let new = sha("lamp-update 2");

    // `check` says what it would take first.
    lamp.config.apply = false;
    let would = lamp.round();
    assert!(
        matches!(&would, Outcome::Would(lines) if lines[0].contains("lamp-update first")),
        "{would:?}"
    );
    lamp.config.apply = true;

    assert_eq!(
        lamp.round(),
        Outcome::Climbs {
            sequence: 2,
            needed: 2,
            from: old.clone(),
            to: new.clone()
        },
        "{:?}",
        lamp.said
    );
    assert_eq!(lamp.own_build(), new);
    assert_eq!(
        lamp.read("/opt/lampOS/update/current/lamp-update"),
        "lamp-update 2"
    );
    // Nothing else of the build: no part's file, unit or link, no restart.
    assert_eq!(parts_files(&lamp), parts);
    assert!(lamp.calls().is_empty(), "{:?}", lamp.calls());
    assert!(lamp.pending().is_some());

    // The new updater, of level 2, confirms itself and takes the rest.
    lamp.config.updater = 2;
    assert_eq!(lamp.confirm(&new), own::Confirm::Confirmed);
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_ne!(parts_files(&lamp), parts);
    assert_eq!(lamp.own_build(), new);
}

/// #1680: a build too new for this updater that carries no lamp-update is
/// refused, said clearly, with nothing fetched or written.
#[test]
fn an_updater_too_old_for_a_build_without_an_updater_is_refused_with_nothing_written() {
    let mut lamp = Lamp::installed_switching_itself();
    lamp.config.updater = 1;
    let old = lamp.own_build();
    let mut build = product_build_with_updater(&mut lamp, 2, "lamp-update 2");
    build.files.retain(|file| file.name != "lamp-update");
    needing(&mut lamp, build, 2);
    let before = lamp.lamp_files();
    let fetched = lamp.channel.fetched.len();
    assert_eq!(
        lamp.round(),
        Outcome::Refused(
            "build 2 needs updater 2 or newer, and this is updater 1; \
             it carries no lamp-update, so waiting for a build that does"
                .into()
        )
    );
    assert_eq!(
        lamp.channel.fetched[fetched..],
        [format!("{CHANNEL}{}", manifest::ENVELOPE)]
    );
    assert_eq!(lamp.lamp_files(), before);
    assert!(lamp.calls().is_empty());
    assert!(lamp.pending().is_none());
    assert_eq!(lamp.own_build(), old);
}

/// #1680: one lamp-update per round, however old the lamp: 1 to 2, which
/// confirms on a channel that has meanwhile moved to a build for updater
/// 3, then 2 to 3, which takes the build.
#[test]
fn an_old_updater_climbs_one_updater_a_round() {
    let mut lamp = Lamp::installed_switching_itself();
    lamp.config.updater = 1;
    let first = lamp.own_build();
    let parts = parts_files(&lamp);
    let build = product_build_with_updater(&mut lamp, 2, "lamp-update 2");
    needing(&mut lamp, build, 2);
    let two = sha("lamp-update 2");
    assert_eq!(
        lamp.round(),
        Outcome::Climbs {
            sequence: 2,
            needed: 2,
            from: first,
            to: two.clone()
        },
        "{:?}",
        lamp.said
    );

    let build = product_build_with_updater(&mut lamp, 3, "lamp-update 3");
    needing(&mut lamp, build, 3);
    let three = sha("lamp-update 3");
    lamp.config.updater = 2;
    // Its dry round reads build 3 and finds the updater it would take: it
    // can still update, so it is confirmed.
    assert_eq!(lamp.confirm(&two), own::Confirm::Confirmed);
    assert_eq!(
        lamp.round(),
        Outcome::Climbs {
            sequence: 3,
            needed: 3,
            from: two,
            to: three.clone()
        },
        "{:?}",
        lamp.said
    );
    assert_eq!(parts_files(&lamp), parts);

    lamp.config.updater = 3;
    assert_eq!(lamp.confirm(&three), own::Confirm::Confirmed);
    assert_eq!(lamp.round(), Outcome::Switched(3), "{:?}", lamp.said);
    assert_eq!(lamp.own_build(), three);
}

/// #1680: with `LAMP_UPDATE_SELF` off, a build too new for this updater is
/// refused even when it carries a newer lamp-update: that one is not taken,
/// and nothing but the envelope is fetched or written.
#[test]
fn with_the_switch_off_an_updater_too_old_for_a_build_takes_nothing_it_carries() {
    let mut lamp = Lamp::installed();
    assert!(!lamp.config.self_switch);
    lamp.config.updater = 1;
    let old = lamp.own_build();
    let build = product_build_with_updater(&mut lamp, 2, "lamp-update 2");
    needing(&mut lamp, build, 2);
    let before = lamp.lamp_files();
    let fetched = lamp.channel.fetched.len();
    assert_eq!(
        lamp.round(),
        Outcome::Refused(
            "build 2 needs updater 2 or newer, and this is updater 1; \
             waiting for a newer updater"
                .into()
        ),
        "{:?}",
        lamp.said
    );
    assert_eq!(
        lamp.channel.fetched[fetched..],
        [format!("{CHANNEL}{}", manifest::ENVELOPE)]
    );
    assert_eq!(lamp.lamp_files(), before);
    assert!(lamp.calls().is_empty());
    assert!(lamp.pending().is_none());
    assert_eq!(lamp.own_build(), old);
}

/// #1680: a build too new for this updater, no newer than the build the
/// lamp runs, is refused: the newer lamp-update it carries is not taken.
#[test]
fn an_updater_too_old_for_a_build_not_newer_than_installed_takes_nothing_it_carries() {
    let mut lamp = Lamp::installed_switching_itself();
    lamp.config.updater = 1;
    let old = lamp.own_build();
    let build = product_build_with_updater(&mut lamp, 1, "lamp-update 2");
    needing(&mut lamp, build, 2);
    let before = lamp.lamp_files();
    let fetched = lamp.channel.fetched.len();
    assert_eq!(
        lamp.round(),
        Outcome::Refused(
            "build 1 needs updater 2 or newer, and this is updater 1; \
             waiting for a newer updater"
                .into()
        ),
        "{:?}",
        lamp.said
    );
    assert_eq!(
        lamp.channel.fetched[fetched..],
        [format!("{CHANNEL}{}", manifest::ENVELOPE)]
    );
    assert_eq!(lamp.lamp_files(), before);
    assert!(lamp.calls().is_empty());
    assert!(lamp.pending().is_none());
    assert_eq!(lamp.own_build(), old);
}

// Models carried in the manifest (#1680, step 3): with LAMP_UPDATE_MODELS=on.

const FACE: &str = "/opt/lampOS/eyes/models/face.onnx";
const TURN: &str = "/opt/lampOS/releases/current/smart-turn.onnx";

/// The eyes as the product runs them: started through their link, reading
/// a model at a path of its own.
fn eyes_reading_a_model() -> String {
    format!(
        "[Unit]\nDescription=lampOS eyes\n\n[Service]\nExecStart=/opt/lampOS/eyes/current/lamp-eyes --device /dev/video0 --model {FACE}\nRestart=on-failure\n"
    )
}

impl Lamp {
    /// lamp-4ace with linked eyes running `eyes 1` and the face model
    /// `face 1` that install placed (its SHA-256 recorded), the switch on.
    fn with_models(self) -> Self {
        let mut lamp = self.with_linked_eyes("eyes 1");
        let at = lamp.root.join(FACE.trim_start_matches('/'));
        fs::create_dir_all(at.parent().unwrap()).unwrap();
        fs::write(&at, "face 1").unwrap();
        fs::write(
            lamp.root.join("etc/systemd/system/lamp-eyes.service"),
            eyes_reading_a_model(),
        )
        .unwrap();
        let records = lamp.root.join("var/lib/lampos-update");
        fs::create_dir_all(&records).unwrap();
        fs::write(
            records.join("state.json"),
            format!(
                "{{\"installed\":null,\"previous\":null,\"refused\":[],\"made\":[],\"settings\":{{\"{FACE}\":\"{}\"}}}}",
                sha("face 1")
            ),
        )
        .unwrap();
        lamp.config.models = true;
        lamp
    }

    /// A model the channel serves as `text` at its own URL.
    fn model(&mut self, path: &str, text: &str) -> Model {
        let url = format!("https://models.test/{}", sha(text));
        self.channel
            .files
            .insert(url.clone(), text.as_bytes().to_vec());
        Model {
            path: path.into(),
            url,
            bytes: text.len() as u64,
            sha256: sha(text),
            archive: None,
        }
    }

    /// Build `sequence` with these eyes and these models, signed.
    fn publish_with_models(&mut self, sequence: u64, eyes: &str, models: Vec<Model>) {
        let mut manifest = self.manifest(
            sequence,
            &[
                ("lamp-voice", "voice 1"),
                ("lamp-turn-end", "turn-end 1"),
                ("lamp-body", "body 1"),
                ("lamp-controls", "controls 1"),
                ("lamp-eyes", eyes),
            ],
            &[],
        );
        manifest.min_updater = Some(manifest::MODELS_UPDATER);
        manifest.models = models;
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let envelope = manifest::seal(&bytes, &self.key).unwrap();
        self.channel
            .files
            .insert(format!("{CHANNEL}{}", manifest::ENVELOPE), envelope);
    }

    fn is_link(&self, path: &str) -> bool {
        fs::symlink_metadata(self.root.join(path.trim_start_matches('/')))
            .unwrap()
            .file_type()
            .is_symlink()
    }

    fn stored(&self) -> Vec<String> {
        let mut found: Vec<String> = fs::read_dir(self.root.join("opt/lampOS/models"))
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        found.sort();
        found
    }

    fn fetched_models(&self) -> Vec<String> {
        self.channel
            .fetched
            .iter()
            .filter(|url| url.starts_with("https://models.test/"))
            .cloned()
            .collect()
    }
}

fn sorted(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names
}

#[test]
fn a_changed_model_is_fetched_checked_and_switched_with_its_part() {
    let mut lamp = Lamp::new().with_models();
    let face = lamp.model(FACE, "face 2");
    let turn = lamp.model(TURN, "turn model 2");
    lamp.publish_with_models(2, "eyes 2", vec![face, turn]);
    let outcome = lamp.round();
    assert_eq!(outcome, Outcome::Switched(2), "{:?}", lamp.said);

    // The face model's path is now a link into the store, with the eyes.
    assert!(lamp.is_link(FACE));
    assert_eq!(lamp.read(FACE), "face 2");
    assert_eq!(lamp.eyes_link(), sha("eyes 2"));
    // install's file is kept in the store by its bytes, for the way back.
    assert_eq!(lamp.stored(), sorted(vec![sha("face 1"), sha("face 2")]));
    // The voice's own model goes beside its binary, in a new folder named
    // by its files, though the binary is the same.
    let voice = lamp.read("/etc/systemd/system/lamp-voice.service");
    assert_ne!(voice, voice_unit(&sha("voice 1")));
    let program = unit::program(&voice).unwrap();
    let folder = program.folder();
    assert_eq!(
        lamp.read(&format!("{folder}smart-turn.onnx")),
        "turn model 2"
    );
    assert_eq!(lamp.read(&format!("{folder}lamp-voice")), "voice 1");
    // Each model fetched once.
    assert_eq!(lamp.fetched_models().len(), 2, "{:?}", lamp.channel.fetched);
    assert!(
        lamp.calls()
            .contains(&"restart lamp-eyes.service".to_owned()),
        "{:?}",
        lamp.calls()
    );
}

#[test]
fn an_unchanged_model_is_not_fetched_and_its_file_stays() {
    let mut lamp = Lamp::new().with_models();
    let face = lamp.model(FACE, "face 1");
    let turn = lamp.model(TURN, "a model");
    lamp.publish_with_models(2, "eyes 2", vec![face, turn]);
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert!(
        lamp.fetched_models().is_empty(),
        "{:?}",
        lamp.channel.fetched
    );
    assert!(!lamp.is_link(FACE));
    assert_eq!(lamp.read(FACE), "face 1");
    assert!(lamp.stored().is_empty());
    // The voice changes nothing.
    assert_eq!(
        lamp.read("/etc/systemd/system/lamp-voice.service"),
        voice_unit(&sha("voice 1"))
    );
}

#[test]
fn a_model_that_does_not_match_its_sha_refuses_the_build_with_nothing_switched() {
    let mut lamp = Lamp::new().with_models();
    let face = lamp.model(FACE, "face 2");
    // What the URL serves is not what the build pins.
    lamp.channel
        .files
        .insert(face.url.clone(), b"face 2, damaged".to_vec());
    lamp.publish_with_models(2, "eyes 2", vec![face]);
    let before = lamp.lamp_files();
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::Refused(why) if why.contains(FACE)),
        "{outcome:?}"
    );
    assert_eq!(lamp.lamp_files(), before);
    assert_eq!(lamp.eyes_link(), sha("eyes 1"));
    assert!(lamp.calls().is_empty(), "{:?}", lamp.calls());
}

#[test]
fn a_model_switch_whose_part_dies_puts_the_model_back() {
    let mut lamp = Lamp::new().with_models();
    let face = lamp.model(FACE, "face 2");
    lamp.publish_with_models(2, "eyes 2, which crashes", vec![face]);
    let before = lamp.lamp_files();
    let outcome = lamp.round();
    assert!(
        matches!(
            &outcome,
            Outcome::RolledBack {
                sequence: 2,
                after: Ok(()),
                ..
            }
        ),
        "{outcome:?}"
    );
    // install's file again, as a file; the store gone with nothing to keep.
    assert!(!lamp.is_link(FACE));
    assert_eq!(lamp.read(FACE), "face 1");
    assert_eq!(lamp.lamp_files(), before);
}

impl Lamp {
    /// A store entry of `text`'s bytes.
    fn in_store(&self, text: &str) {
        let store = self.root.join("opt/lampOS/models");
        fs::create_dir_all(&store).unwrap();
        fs::write(store.join(sha(text)), text).unwrap();
    }

    /// `path` a link into the store, to `text`'s entry, as a switch leaves it.
    fn linked_to(&self, path: &str, text: &str) {
        let at = self.root.join(path.trim_start_matches('/'));
        fs::create_dir_all(at.parent().unwrap()).unwrap();
        let _ = fs::remove_file(&at);
        std::os::unix::fs::symlink(lamp_update::models::target(path, &sha(text)), &at).unwrap();
    }

    /// A switch to build 2 cut short after its models moved: the eyes' unit
    /// and link as they were, the model paths as `models` (the journal's
    /// JSON) says they were.
    fn cut_short_after_models(&self, models: &str) {
        let records = self.root.join("var/lib/lampos-update");
        fs::create_dir_all(records.join("previous")).unwrap();
        fs::write(
            records.join("previous/lamp-eyes.service"),
            eyes_reading_a_model(),
        )
        .unwrap();
        fs::write(
            records.join("journal.json"),
            format!(
                "{{\"build\":{{\"sequence\":2,\"commit\":\"{}\"}},\"units\":[{{\"unit\":\"lamp-eyes.service\",\"was_active\":true,\"link\":{{\"path\":\"/opt/lampOS/eyes/current\",\"build\":\"{}\"}}}}],\"models\":{models}}}",
                "2".repeat(40),
                sha("eyes 1")
            ),
        )
        .unwrap();
    }
}

/// A model path of its own a unit reads as a folder, as the name does.
const NAME: &str = "/opt/lampOS/name/models/name.onnx";

#[test]
fn a_model_switch_cut_short_is_put_back_at_the_next_start() {
    let mut lamp = Lamp::new().with_models();
    // As a switch to build 2 leaves it when the power goes after the face
    // model's link moved: install's file in the store, the path a link.
    lamp.in_store("face 1");
    lamp.in_store("face 2");
    lamp.linked_to(FACE, "face 2");
    lamp.cut_short_after_models(&format!(
        "[{{\"path\":\"{FACE}\",\"sha256\":\"{}\",\"file\":true}}]",
        sha("face 1")
    ));
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::RolledBack { sequence: 2, why, .. } if why.contains("cut short")),
        "{outcome:?}"
    );
    assert!(!lamp.is_link(FACE));
    assert_eq!(lamp.read(FACE), "face 1");
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
    // No kept build names either: the store is emptied, and goes.
    assert!(lamp.stored().is_empty());
}

/// The reviewer on #1879: a switch whose only change was a model, cut
/// short, restarts the part that reads it on the model put back. Its unit
/// and link never changed, but it may have started on the new model.
#[test]
fn a_model_only_switch_cut_short_restarts_the_part_that_reads_it() {
    let mut lamp = Lamp::new().with_models();
    lamp.in_store("face 1");
    lamp.in_store("face 2");
    lamp.linked_to(FACE, "face 2");
    lamp.cut_short_after_models(&format!(
        "[{{\"path\":\"{FACE}\",\"sha256\":\"{}\",\"file\":true}}]",
        sha("face 1")
    ));
    let outcome = lamp.round();
    assert!(
        matches!(
            &outcome,
            Outcome::RolledBack {
                sequence: 2,
                after: Ok(()),
                ..
            }
        ),
        "{outcome:?}"
    );
    assert_eq!(lamp.read(FACE), "face 1");
    assert!(
        lamp.calls()
            .contains(&"restart lamp-eyes.service".to_owned()),
        "{:?}",
        lamp.calls()
    );
}

/// The reviewer on #1879: a model path that cannot be put back does not
/// stop the others, and the journal stays for the next start, with no
/// store entry a path links to removed.
#[test]
fn a_model_that_cannot_be_put_back_leaves_the_others_put_back_and_the_journal_kept() {
    let mut lamp = Lamp::new().with_models();
    // install's face file was kept in the store, and is gone from it now:
    // it cannot come back. The name's link can.
    lamp.in_store("face 2");
    lamp.in_store("name 1");
    lamp.in_store("name 2");
    lamp.linked_to(FACE, "face 2");
    lamp.linked_to(NAME, "name 2");
    // Put back in reverse: the face first, then the name.
    lamp.cut_short_after_models(&format!(
        "[{{\"path\":\"{NAME}\",\"sha256\":\"{}\"}},{{\"path\":\"{FACE}\",\"sha256\":\"{}\",\"file\":true}}]",
        sha("name 1"),
        sha("face 1")
    ));
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::RolledBack { sequence: 2, after: Err(why), .. } if why.contains(FACE)),
        "{outcome:?}"
    );
    assert_eq!(lamp.read(NAME), "name 1");
    assert!(lamp.exists("/var/lib/lampos-update/journal.json"));
    // What the face still links to is kept, and the name's entry too.
    assert_eq!(lamp.read(FACE), "face 2");
    assert!(
        lamp.stored().contains(&sha("face 2")),
        "{:?}",
        lamp.stored()
    );
    assert!(
        lamp.stored().contains(&sha("name 1")),
        "{:?}",
        lamp.stored()
    );
    // The next start tries again: the name stays put back, the face waits.
    let again = lamp.round();
    assert!(
        matches!(&again, Outcome::RolledBack { after: Err(why), .. } if why.contains(FACE)),
        "{again:?}"
    );
    assert_eq!(lamp.read(NAME), "name 1");
    assert_eq!(lamp.read(FACE), "face 2");
}

/// The reviewer on #1879: with state.json set aside as unreadable, the
/// next switch's prune keeps every store entry a model path links to now.
#[test]
fn prune_keeps_what_a_model_path_links_to_when_the_state_was_set_aside() {
    let mut lamp = Lamp::new().with_models();
    let face = lamp.model(FACE, "face 2");
    lamp.publish_with_models(2, "eyes 1", vec![face.clone()]);
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert!(lamp.is_link(FACE));
    fs::write(lamp.root.join("var/lib/lampos-update/state.json"), "{").unwrap();
    // The eyes change, the face model does not.
    lamp.publish_with_models(3, "eyes 3", vec![face]);
    assert_eq!(lamp.round(), Outcome::Switched(3), "{:?}", lamp.said);
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("could not be read")),
        "{:?}",
        lamp.said
    );
    assert_eq!(lamp.read(FACE), "face 2");
    assert!(
        lamp.stored().contains(&sha("face 2")),
        "{:?}",
        lamp.stored()
    );
}

/// The reviewer on #1879: a store fetch the network cuts is unreachable,
/// as a build's file is, and tried again: not a refusal.
#[test]
fn a_model_fetch_cut_by_the_network_is_unreachable_not_refused() {
    let mut lamp = Lamp::new().with_models();
    let face = lamp.model(FACE, "face 2");
    lamp.publish_with_models(2, "eyes 2", vec![face]);
    lamp.channel.unreachable_under = Some("https://models.test/".into());
    let before = lamp.lamp_files();
    let outcome = lamp.round();
    assert!(matches!(&outcome, Outcome::Unreachable(_)), "{outcome:?}");
    assert_eq!(lamp.lamp_files(), before);
}

#[test]
fn prune_keeps_the_models_of_the_kept_builds() {
    let mut lamp = Lamp::new().with_models();
    for (sequence, face) in [(2, "face 2"), (3, "face 3"), (4, "face 4")] {
        // Only the model changes: the eyes, which read it, restart with it.
        let model = lamp.model(FACE, face);
        lamp.publish_with_models(sequence, "eyes 1", vec![model]);
        let restarts = lamp.calls().len();
        assert_eq!(lamp.round(), Outcome::Switched(sequence), "{:?}", lamp.said);
        let restarted: Vec<String> = lamp.calls()[restarts..]
            .iter()
            .filter(|call| call.starts_with("restart"))
            .cloned()
            .collect();
        assert_eq!(restarted, ["restart lamp-eyes.service"]);
        assert_eq!(lamp.read(FACE), face);
    }
    // The model of the build the lamp runs and of the one before it.
    assert_eq!(lamp.stored(), sorted(vec![sha("face 3"), sha("face 4")]));
}

#[test]
fn a_lamp_short_of_room_for_the_models_refuses_before_fetching() {
    let mut lamp = Lamp::new().with_models();
    let mut face = lamp.model(FACE, "face 2");
    // Fifty MB, as the build says: refused before the URL is read.
    face.bytes = 50_000_000;
    lamp.publish_with_models(2, "eyes 2", vec![face]);
    lamp.free = lamp.config.keep_free + 10_000_000;
    let before = lamp.lamp_files();
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::Refused(why) if why.contains("(50 MB of it models)")),
        "{outcome:?}"
    );
    assert!(
        lamp.fetched_models().is_empty(),
        "{:?}",
        lamp.channel.fetched
    );
    assert_eq!(lamp.lamp_files(), before);
    let log = lamp.read("/var/lib/lampos-update/log.jsonl");
    assert!(log.contains("50 MB of it models"), "{log}");
}

#[test]
fn with_the_switch_off_a_build_with_other_models_is_refused() {
    let mut lamp = Lamp::new().with_models();
    lamp.config.models = false;
    let face = lamp.model(FACE, "face 2");
    lamp.publish_with_models(2, "eyes 2", vec![face]);
    let before = lamp.lamp_files();
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::WaitsForModels(why) if why.contains("LAMP_UPDATE_MODELS=on")),
        "{outcome:?}"
    );
    assert!(
        outcome
            .to_string()
            .starts_with("refused, nothing switched: ")
    );
    assert!(lamp.fetched_models().is_empty());
    assert_eq!(lamp.lamp_files(), before);
    let log = lamp.read("/var/lib/lampos-update/log.jsonl");
    assert!(log.contains("LAMP_UPDATE_MODELS=on"), "{log}");
    // The same models as the lamp's: taken as today, nothing fetched.
    let face = lamp.model(FACE, "face 1");
    lamp.publish_with_models(3, "eyes 2", vec![face]);
    assert_eq!(lamp.round(), Outcome::Switched(3), "{:?}", lamp.said);
    assert!(lamp.fetched_models().is_empty());
    assert!(!lamp.is_link(FACE));
}

/// The reviewer on #1879: off agrees with on for a file at a model's path
/// that install did not place. It is the lamp's own, kept and said, and
/// the rest of the build is taken.
#[test]
fn with_the_switch_off_a_lamps_own_model_is_kept_as_with_it_on() {
    for on in [true, false] {
        let mut lamp = Lamp::new().with_models();
        lamp.config.models = on;
        // No record of install placing it.
        fs::write(
            lamp.root.join("var/lib/lampos-update/state.json"),
            "{\"installed\":null,\"previous\":null,\"refused\":[],\"made\":[]}",
        )
        .unwrap();
        let face = lamp.model(FACE, "face 2");
        lamp.publish_with_models(2, "eyes 2", vec![face]);
        assert_eq!(
            lamp.round(),
            Outcome::Switched(2),
            "on {on}: {:?}",
            lamp.said
        );
        assert_eq!(lamp.read(FACE), "face 1");
        assert!(!lamp.is_link(FACE));
        assert!(
            lamp.said
                .iter()
                .any(|line| line == &format!("kept {FACE}: it is not the pinned model")),
            "on {on}: {:?}",
            lamp.said
        );
        assert_eq!(lamp.eyes_link(), sha("eyes 2"));
    }
}

/// The reviewer on #1879, the dead end: with LAMP_UPDATE_SELF=on and
/// LAMP_UPDATE_MODELS off, a build with models the lamp lacks. The new
/// updater its climb takes confirms itself, since its dry round counts only
/// what it is switched to apply, and then waits for the models switch with
/// nothing of the parts written; no later round climbs again.
#[test]
fn with_models_off_a_new_updater_confirms_and_the_build_waits_for_models() {
    let mut lamp = Lamp::installed_switching_itself();
    lamp.config.updater = 1;
    // A model install placed, its SHA-256 recorded.
    let at = lamp.root.join(FACE.trim_start_matches('/'));
    fs::create_dir_all(at.parent().unwrap()).unwrap();
    fs::write(&at, "face 1").unwrap();
    let mut state = lamp.state();
    state["settings"][FACE] = sha("face 1").into();
    fs::write(
        lamp.records().join("state.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    let old = lamp.own_build();
    let parts = parts_files(&lamp);
    let mut build = product_build_with_updater(&mut lamp, 2, "lamp-update 2");
    build.models = vec![lamp.model(FACE, "face 2")];
    needing(&mut lamp, build, manifest::MODELS_UPDATER);
    let new = sha("lamp-update 2");
    assert_eq!(
        lamp.round(),
        Outcome::Climbs {
            sequence: 2,
            needed: 2,
            from: old,
            to: new.clone()
        },
        "{:?}",
        lamp.said
    );

    lamp.config.updater = 2;
    assert_eq!(lamp.confirm(&new), own::Confirm::Confirmed);
    assert!(lamp.pending().is_none());
    // The fallback leaves it.
    assert_eq!(
        lamp.back(update_unit("failed", "failed"), u64::MAX),
        own::Back::Nothing
    );
    for _ in 0..2 {
        let outcome = lamp.round();
        assert!(
            matches!(&outcome, Outcome::WaitsForModels(why) if why.contains("LAMP_UPDATE_MODELS=on")),
            "{outcome:?}"
        );
        assert_eq!(lamp.own_build(), new);
        assert_eq!(parts_files(&lamp), parts);
        assert!(lamp.pending().is_none());
    }
    assert!(lamp.fetched_models().is_empty());

    // The models switch on: the build is taken, its model with it.
    lamp.config.models = true;
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.read(FACE), "face 2");
    assert_eq!(lamp.own_build(), new);
}

/// The controls' settings at their product path, as install wrote and
/// recorded them, the controls reading them, and lamp-update-apply ready:
/// what a product lamp has (#1680).
fn lamp_with_settings() -> (Lamp, Rc<RefCell<Vec<String>>>) {
    let mut lamp = Lamp::new();
    let controls = sha("controls 1");
    let write = |root: &Path, path: &str, text: &str| {
        let at = root.join(path.trim_start_matches('/'));
        fs::create_dir_all(at.parent().unwrap()).unwrap();
        fs::write(at, text).unwrap();
    };
    write(
        &lamp.root,
        "/etc/systemd/system/lamp-controls.service",
        &format!(
            "[Service]\nExecStart=/opt/lampOS/controls/{controls}/lamp-controls serve --config /etc/lampOS/controls/controls.json\n"
        ),
    );
    write(&lamp.root, CONTROLS_JSON, "controls 1");
    write(&lamp.root, RECORDS_JSON, "records 1");
    let state = serde_json::json!({
        "installed": {"sequence": 1, "commit": format!("{:040}", 1)},
        "previous": null,
        "refused": [],
        "made": [],
        "settings": {
            CONTROLS_JSON: sha("controls 1"),
            RECORDS_JSON: sha("records 1"),
        },
    });
    write(
        &lamp.root,
        "/var/lib/lampos-update/state.json",
        &state.to_string(),
    );
    lamp.config.settings = true;
    let tmpfiles = Rc::new(RefCell::new(Vec::new()));
    let (root, key, ran) = (lamp.root.clone(), lamp.config.key, Rc::clone(&tmpfiles));
    lamp.systemd.applier = Some(Box::new(move || {
        apply::run(
            &root,
            &root.join("var/lib/lampos-update"),
            &key,
            // The tmpfiles' path, or any other command whole.
            &mut |parts| {
                ran.borrow_mut().push(if parts[0] == "systemd-tmpfiles" {
                    parts[2].to_owned()
                } else {
                    parts.join(" ")
                });
                Ok(())
            },
            &mut |_| {},
        )
    }));
    (lamp, tmpfiles)
}

const CONTROLS_JSON: &str = "/etc/lampOS/controls/controls.json";
const RECORDS_JSON: &str = "/etc/lampOS/records.json";
const TMPFILES_CONF: &str = "/etc/tmpfiles.d/lampos.conf";

/// Build `sequence`: build 1's binaries, and these settings.
fn publish_settings(lamp: &mut Lamp, sequence: u64, settings: Vec<Setting>) {
    publish_settings_with_controls(lamp, sequence, settings, "controls 1");
}

fn publish_settings_with_controls(
    lamp: &mut Lamp,
    sequence: u64,
    settings: Vec<Setting>,
    controls: &str,
) {
    let mut manifest = lamp.manifest(
        sequence,
        &[
            ("lamp-voice", "voice 1"),
            ("lamp-turn-end", "turn-end 1"),
            ("lamp-body", "body 1"),
            ("lamp-controls", controls),
        ],
        &[],
    );
    manifest.min_updater = Some(manifest::SETTINGS_UPDATER);
    manifest.settings = settings;
    let bytes = serde_json::to_vec(&manifest).unwrap();
    let envelope = manifest::seal(&bytes, &lamp.key).unwrap();
    lamp.channel
        .files
        .insert(format!("{CHANNEL}{}", manifest::ENVELOPE), envelope);
}

const RECEIPT_REMOVED: &str = "/etc/lampOS/controls/receipt-removed.json";
const RECEIPT_NEW: &str = "/etc/lampOS/controls/receipt-new/new.json";

fn receipt_restore_settings(lamp: &mut Lamp) -> Vec<Setting> {
    use std::os::unix::fs::PermissionsExt;

    for (path, mode) in [(CONTROLS_JSON, 0o600), (RECORDS_JSON, 0o640)] {
        fs::set_permissions(
            lamp.root.join(path.trim_start_matches('/')),
            fs::Permissions::from_mode(mode),
        )
        .unwrap();
    }
    let removed = lamp.root.join(RECEIPT_REMOVED.trim_start_matches('/'));
    fs::write(&removed, "removed setting before").unwrap();
    fs::set_permissions(&removed, fs::Permissions::from_mode(0o600)).unwrap();
    let mut state = lamp.state();
    state["settings"][RECEIPT_REMOVED] = sha("removed setting before").into();
    fs::write(
        lamp.root.join("var/lib/lampos-update/state.json"),
        state.to_string(),
    )
    .unwrap();
    vec![
        Setting::new(CONTROLS_JSON, "controls 2"),
        Setting::new(RECORDS_JSON, "records 2"),
        Setting::new(RECEIPT_NEW, "new setting"),
        Setting {
            path: RECEIPT_REMOVED.into(),
            text: None,
            sha256: None,
            mode: None,
            removed: true,
        },
    ]
}

fn assert_receipt_settings_written(root: &Path) {
    for (path, text) in [
        (CONTROLS_JSON, "controls 2"),
        (RECORDS_JSON, "records 2"),
        (RECEIPT_NEW, "new setting"),
    ] {
        assert_eq!(
            fs::read_to_string(root.join(path.trim_start_matches('/'))).unwrap(),
            text
        );
    }
    assert!(!root.join(RECEIPT_REMOVED.trim_start_matches('/')).exists());
    let back: apply::Back = serde_json::from_slice(
        &fs::read(root.join("var/lib/lampos-update/apply/back.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(back.sequence, 2);
    assert_eq!(back.files.len(), 4);
}

fn assert_receipt_settings_restored(root: &Path) {
    use std::os::unix::fs::PermissionsExt;

    for (path, text, mode) in [
        (CONTROLS_JSON, "controls 1", 0o600),
        (RECORDS_JSON, "records 1", 0o640),
        (RECEIPT_REMOVED, "removed setting before", 0o600),
    ] {
        let path = root.join(path.trim_start_matches('/'));
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o7777,
            mode
        );
    }
    assert!(!root.join(RECEIPT_NEW.trim_start_matches('/')).exists());
    assert!(!root.join("etc/lampOS/controls/receipt-new").exists());
}

#[test]
fn a_rejected_apply_receipt_runs_real_back_and_restores_written_settings_modes_and_records() {
    let (mut lamp, _) = lamp_with_settings();
    let settings = receipt_restore_settings(&mut lamp);
    let before = lamp.state();
    let mut applier = lamp.systemd.applier.take().unwrap();
    let root = lamp.root.clone();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let observed = Rc::clone(&calls);
    let mut first_apply = true;
    lamp.systemd.applier = Some(Box::new(move || {
        let request: apply::Request = serde_json::from_slice(
            &fs::read(root.join("var/lib/lampos-update/apply/request.json")).unwrap(),
        )
        .unwrap();
        applier()?;
        observed.borrow_mut().push((request.op, request.sequence));
        if request.op == apply::Op::Apply && std::mem::take(&mut first_apply) {
            // These are real apply effects and its real before-images, not
            // a fake receipt for a helper that never changed the files.
            assert_receipt_settings_written(&root);
            let path = root.join("var/lib/lampos-update/apply/done.json");
            let mut done: apply::Done = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            done.sequence += 1;
            fs::write(path, serde_json::to_vec(&done).unwrap()).unwrap();
        }
        Ok(())
    }));
    publish_settings(&mut lamp, 2, settings);

    let outcome = lamp.round();

    assert!(
        matches!(&outcome, Outcome::RolledBack { sequence: 2, why, after: Ok(()), .. }
        if why.contains("done.json is not for the requested settings operation and build")),
        "{outcome:?}"
    );
    assert_receipt_settings_restored(&lamp.root);
    assert_eq!(lamp.state()["settings"], before["settings"]);
    assert_eq!(lamp.state()["installed"], before["installed"]);
    assert_eq!(lamp.state()["refused"][0]["sequence"], 2);
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
    assert_eq!(
        *calls.borrow(),
        [(apply::Op::Apply, 2), (apply::Op::Back, 2)]
    );
}

#[test]
fn a_rejected_back_receipt_keeps_the_journal_until_real_back_is_acknowledged() {
    let (mut lamp, _) = lamp_with_settings();
    let settings = receipt_restore_settings(&mut lamp);
    let before = lamp.state();
    let mut applier = lamp.systemd.applier.take().unwrap();
    let root = lamp.root.clone();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let observed = Rc::clone(&calls);
    let mut first_back = true;
    lamp.systemd.applier = Some(Box::new(move || {
        let request: apply::Request = serde_json::from_slice(
            &fs::read(root.join("var/lib/lampos-update/apply/request.json")).unwrap(),
        )
        .unwrap();
        applier()?;
        observed.borrow_mut().push((request.op, request.sequence));
        if request.op == apply::Op::Apply {
            assert_receipt_settings_written(&root);
        } else if std::mem::take(&mut first_back) {
            assert_receipt_settings_restored(&root);
            let path = root.join("var/lib/lampos-update/apply/done.json");
            let mut done: apply::Done = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            done.op = apply::Op::Apply;
            fs::write(path, serde_json::to_vec(&done).unwrap()).unwrap();
        }
        Ok(())
    }));
    // The running candidate's health really fails; the restored old binary
    // is healthy without changing the fixture's fault policy between rounds.
    publish_settings_with_controls(&mut lamp, 2, settings, "controls 2 unwell");

    let outcome = lamp.round();

    assert!(
        matches!(&outcome, Outcome::RolledBack { sequence: 2, after: Err(why), .. }
        if why.contains("done.json is not for the requested settings operation and build")),
        "{outcome:?}"
    );
    assert_receipt_settings_restored(&lamp.root);
    let unresolved = lamp.state();
    assert_eq!(unresolved["installed"], before["installed"]);
    assert_eq!(unresolved["settings"][CONTROLS_JSON], sha("controls 2"));
    assert_eq!(unresolved["settings"][RECORDS_JSON], sha("records 2"));
    assert!(unresolved["refused"].as_array().unwrap().is_empty());
    let journal: serde_json::Value =
        serde_json::from_str(&lamp.read("/var/lib/lampos-update/journal.json")).unwrap();
    assert_eq!(journal["build"]["sequence"], 2);
    assert_eq!(journal["settings"], true);
    // Files are already restored, but an unaccepted receipt has not settled
    // their records. The journal remains the explicit incomplete transaction.
    let recovered = lamp.round();
    assert!(
        matches!(
            &recovered,
            Outcome::RolledBack {
                sequence: 2,
                after: Ok(()),
                ..
            }
        ),
        "{recovered:?}"
    );
    assert_receipt_settings_restored(&lamp.root);
    assert_eq!(lamp.state()["settings"], before["settings"]);
    assert_eq!(lamp.state()["installed"], before["installed"]);
    assert_eq!(lamp.state()["refused"][0]["sequence"], 2);
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
    assert_eq!(
        *calls.borrow(),
        [
            (apply::Op::Apply, 2),
            (apply::Op::Back, 2),
            (apply::Op::Back, 2)
        ]
    );
    let settled_calls = calls.borrow().clone();
    let again = lamp.round();
    assert!(!matches!(again, Outcome::Switched(_)), "{again:?}");
    assert_eq!(
        *calls.borrow(),
        settled_calls,
        "the refused build was applied again"
    );
    assert_eq!(lamp.state()["installed"], before["installed"]);
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
}

#[test]
fn the_round_rejects_a_done_matching_a_rewritten_request_instead_of_its_own_request() {
    for (op, sequence) in [(apply::Op::Back, 2), (apply::Op::Apply, 99)] {
        let (mut lamp, _) = lamp_with_settings();
        let before = lamp.state()["settings"].clone();
        let records = lamp.root.join("var/lib/lampos-update/apply");
        lamp.systemd.applier = Some(Box::new(move || {
            // A helper's mutable request and receipt agree with each other,
            // but neither names what the waiting parent actually asked.
            let request = apply::Request {
                version: apply::VERSION,
                op,
                sequence,
                image: false,
            };
            let done = apply::Done {
                version: apply::VERSION,
                op,
                sequence,
                records: [
                    (CONTROLS_JSON.to_owned(), Some(sha("wrong build"))),
                    (RECORDS_JSON.to_owned(), None),
                ]
                .into(),
            };
            fs::write(
                records.join("request.json"),
                serde_json::to_vec(&request).unwrap(),
            )
            .unwrap();
            fs::write(
                records.join("done.json"),
                serde_json::to_vec(&done).unwrap(),
            )
            .unwrap();
            Ok(())
        }));
        publish_settings(
            &mut lamp,
            2,
            vec![Setting::new(CONTROLS_JSON, "controls 2")],
        );

        let outcome = lamp.round();

        assert!(
            matches!(outcome, Outcome::RolledBack { sequence: 2, .. }),
            "{outcome:?}"
        );
        assert_eq!(lamp.state()["settings"], before);
        assert_eq!(lamp.read(CONTROLS_JSON), "controls 1");
        assert_eq!(lamp.read(RECORDS_JSON), "records 1");
        assert_eq!(lamp.state()["installed"]["sequence"], 1);
        assert_eq!(lamp.state()["refused"][0]["sequence"], 2);
    }
}

#[test]
fn with_apply_on_a_builds_settings_are_written_at_the_switch_and_the_part_reading_one_restarted() {
    let (mut lamp, tmpfiles) = lamp_with_settings();
    // The lamp's own records bound, changed since install wrote it.
    fs::write(lamp.root.join("etc/lampOS/records.json"), "this lamp's").unwrap();
    publish_settings(
        &mut lamp,
        2,
        vec![
            Setting::new(CONTROLS_JSON, "controls 2"),
            Setting::new(RECORDS_JSON, "records 2"),
            Setting::new(TMPFILES_CONF, "d /run/lampos-bus 2770 root lampos -\n"),
        ],
    );
    assert_eq!(lamp.round(), Outcome::Switched(2));
    assert_eq!(lamp.read(CONTROLS_JSON), "controls 2");
    assert_eq!(lamp.read(RECORDS_JSON), "this lamp's");
    assert_eq!(
        lamp.read(TMPFILES_CONF),
        "d /run/lampos-bus 2770 root lampos -\n"
    );
    assert_eq!(*tmpfiles.borrow(), [TMPFILES_CONF]);
    assert!(
        lamp.said
            .iter()
            .any(|line| line.starts_with(&format!("kept {RECORDS_JSON}"))),
        "{:?}",
        lamp.said
    );
    // The settings before the parts, and only the part reading one restarted.
    let calls = lamp.calls();
    let applied = calls
        .iter()
        .position(|call| call == &format!("restart {}", apply::UNIT))
        .unwrap();
    let controls = calls
        .iter()
        .position(|call| call == "restart lamp-controls.service")
        .unwrap();
    assert!(applied < controls, "{calls:?}");
    assert!(
        !calls
            .iter()
            .any(|call| call == "restart lamp-voice.service"),
        "{calls:?}"
    );
    let state = lamp.state();
    assert_eq!(state["settings"][CONTROLS_JSON], sha("controls 2"));
    assert_eq!(state["settings"][RECORDS_JSON], sha("records 1"));
    assert_eq!(
        state["settings"][TMPFILES_CONF],
        sha("d /run/lampos-bus 2770 root lampos -\n")
    );
    assert!(!lamp.exists("/var/lib/lampos-update/apply/request.json"));
    assert!(lamp.exists("/var/lib/lampos-update/apply/back.json"));
}

#[test]
fn with_apply_off_a_builds_settings_are_not_written_and_that_is_said() {
    let (mut lamp, _) = lamp_with_settings();
    lamp.config.settings = false;
    publish_settings(
        &mut lamp,
        2,
        vec![Setting::new(CONTROLS_JSON, "controls 2")],
    );
    assert_eq!(lamp.round(), Outcome::UpToDate(2));
    assert_eq!(lamp.read(CONTROLS_JSON), "controls 1");
    assert!(!lamp.calls().iter().any(|call| call.contains(apply::UNIT)));
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("settings are not written") && line.contains(apply::SWITCH)),
        "{:?}",
        lamp.said
    );
}

#[test]
fn a_build_put_back_puts_its_settings_back_byte_for_byte() {
    let (mut lamp, _) = lamp_with_settings();
    let before = lamp.state()["settings"].clone();
    lamp.systemd.crashing.push("lamp-controls.service".into());
    publish_settings(
        &mut lamp,
        2,
        vec![Setting::new(CONTROLS_JSON, "controls 2")],
    );
    let outcome = lamp.round();
    assert!(
        matches!(outcome, Outcome::RolledBack { sequence: 2, .. }),
        "{outcome:?}"
    );
    assert_eq!(lamp.read(CONTROLS_JSON), "controls 1");
    assert_eq!(lamp.state()["settings"], before);
    assert_eq!(lamp.state()["refused"][0]["sequence"], 2);
}

#[test]
fn a_lamp_without_lamp_update_apply_puts_the_build_back_with_nothing_written() {
    let (mut lamp, _) = lamp_with_settings();
    lamp.systemd.applier = None;
    publish_settings(
        &mut lamp,
        2,
        vec![Setting::new(CONTROLS_JSON, "controls 2")],
    );
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::RolledBack { sequence: 2, why, .. } if why.contains("did nothing")),
        "{outcome:?}"
    );
    assert_eq!(lamp.read(CONTROLS_JSON), "controls 1");
    assert!(
        !lamp
            .calls()
            .iter()
            .any(|call| call == "restart lamp-controls.service")
    );
    // Nothing was written, so nothing is asked back: the round ends, the
    // journal cleared and the build refused, and the next start is quiet.
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
    assert_eq!(lamp.state()["refused"][0]["sequence"], 2);
    assert!(!lamp.exists("/var/lib/lampos-update/apply/request.json"));
    let restarts = |lamp: &Lamp| {
        lamp.calls()
            .iter()
            .filter(|call| *call == &format!("restart {}", apply::UNIT))
            .count()
    };
    let asked = restarts(&lamp);
    let again = lamp.round();
    assert!(!matches!(again, Outcome::RolledBack { .. }), "{again:?}");
    assert_eq!(restarts(&lamp), asked, "{again:?}");
}

/// The reviewer on #1913, 19:50: a product lamp installed before #1680 has
/// install's tmpfiles with no record; a build's newer tmpfiles reach it.
#[test]
fn a_product_lamp_without_a_tmpfiles_record_takes_the_builds_tmpfiles() {
    let (mut lamp, tmpfiles) = lamp_with_settings();
    let at = lamp.root.join(TMPFILES_CONF.trim_start_matches('/'));
    fs::create_dir_all(at.parent().unwrap()).unwrap();
    fs::write(&at, lamp_update::install::TMPFILES).unwrap();
    assert!(lamp.state()["settings"].get(TMPFILES_CONF).is_none());
    let new = format!(
        "{}d /run/lampos-new 0755 root root -\n",
        lamp_update::install::TMPFILES
    );
    publish_settings(&mut lamp, 2, vec![Setting::new(TMPFILES_CONF, &new)]);
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.read(TMPFILES_CONF), new);
    assert_eq!(*tmpfiles.borrow(), [TMPFILES_CONF]);
    assert_eq!(lamp.state()["settings"][TMPFILES_CONF], sha(&new));
}

#[test]
fn a_switch_cut_short_after_its_settings_puts_them_back_at_the_next_start() {
    let (mut lamp, _) = lamp_with_settings();
    publish_settings(
        &mut lamp,
        2,
        vec![Setting::new(CONTROLS_JSON, "controls 2")],
    );
    lamp.systemd.cut_at_reload = true;
    let cut = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lamp.round()));
    assert!(cut.is_err());
    assert_eq!(
        lamp.read(CONTROLS_JSON),
        "controls 2",
        "written before the cut"
    );
    assert!(lamp.exists("/var/lib/lampos-update/journal.json"));
    let outcome = lamp.round();
    assert!(
        matches!(outcome, Outcome::RolledBack { sequence: 2, .. }),
        "{outcome:?}"
    );
    assert_eq!(lamp.read(CONTROLS_JSON), "controls 1");
    assert_eq!(lamp.state()["settings"][CONTROLS_JSON], sha("controls 1"));
}

const ASOUND: &str = "/etc/asound.conf";
const SPEAKER: &str = "/etc/systemd/system/lamp-cmedia-speaker.service";
const JOURNAL: &str = "/etc/systemd/journald.conf.d/zz-lamp-journal.conf";

fn image_file(path: &str, text: &str, shipped: &[&str]) -> ImageFile {
    ImageFile {
        variant: "standard".into(),
        path: path.into(),
        text: text.into(),
        sha256: sha(text),
        shipped: shipped.iter().map(|text| sha(text)).collect(),
    }
}

/// A product lamp flashed with an earlier image, `LAMP_UPDATE_IMAGE=on`
/// (#1369): its own `/etc/asound.conf` and speaker unit the image's.
fn lamp_with_image() -> (Lamp, Rc<RefCell<Vec<String>>>) {
    let (mut lamp, ran) = lamp_with_settings();
    lamp.config.image = true;
    for (path, text) in [
        (ASOUND, "asound 1"),
        (SPEAKER, "speaker 1"),
        (JOURNAL, "journal 1"),
    ] {
        fs::write(lamp.root.join(path.trim_start_matches('/')), text)
            .or_else(|_| {
                let at = lamp.root.join(path.trim_start_matches('/'));
                fs::create_dir_all(at.parent().unwrap())?;
                fs::write(at, text)
            })
            .unwrap();
    }
    (lamp, ran)
}

/// Build `sequence`: build 1's binaries, and these of the image's files.
fn publish_image(lamp: &mut Lamp, sequence: u64, image: Vec<ImageFile>) {
    let mut manifest = lamp.manifest(
        sequence,
        &[
            ("lamp-voice", "voice 1"),
            ("lamp-turn-end", "turn-end 1"),
            ("lamp-body", "body 1"),
            ("lamp-controls", "controls 1"),
        ],
        &[],
    );
    manifest.min_updater = Some(manifest::IMAGE_UPDATER);
    manifest.image = image;
    let bytes = serde_json::to_vec(&manifest).unwrap();
    let envelope = manifest::seal(&bytes, &lamp.key).unwrap();
    lamp.channel
        .files
        .insert(format!("{CHANNEL}{}", manifest::ENVELOPE), envelope);
}

fn asound_build(lamp: &mut Lamp, sequence: u64) {
    publish_image(
        lamp,
        sequence,
        vec![
            image_file(ASOUND, "asound 2", &["asound 1", "asound 2"]),
            image_file(SPEAKER, "speaker 2", &["speaker 1", "speaker 2"]),
            image_file(JOURNAL, "journal 2", &["journal 1", "journal 2"]),
        ],
    );
}

#[test]
fn with_image_on_the_images_files_are_written_and_asound_restarts_the_voice_at_a_quiet_moment() {
    let (mut lamp, ran) = lamp_with_image();
    asound_build(&mut lamp, 2);
    let before = lamp.now.get();
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.read(ASOUND), "asound 2");
    assert_eq!(lamp.read(SPEAKER), "speaker 2");
    assert_eq!(lamp.read(JOURNAL), "journal 2");
    assert_eq!(
        *ran.borrow(),
        [
            "systemd-analyze verify /run/lampos-update-apply/lamp-cmedia-speaker.service",
            "systemctl daemon-reload",
            "systemctl start lamp-cmedia-speaker.service"
        ]
    );
    // The voice restarted for asound.conf, after 45 s of quiet, and the
    // files written only then.
    let calls = lamp.calls();
    let applied = calls
        .iter()
        .position(|call| call == &format!("restart {}", apply::UNIT))
        .unwrap();
    let voice = calls
        .iter()
        .position(|call| call == "restart lamp-voice.service")
        .unwrap();
    assert!(applied < voice, "{calls:?}");
    let (_, at) = lamp
        .systemd
        .restarted_at
        .iter()
        .find(|(unit, _)| unit == "lamp-voice.service")
        .unwrap();
    assert!(*at >= before + 45_000, "{at} {before}");
    // journald: never restarted, nor anything of it run.
    assert!(
        !calls.iter().any(|call| call.contains("journald")),
        "{calls:?}"
    );
    assert!(
        !ran.borrow()
            .iter()
            .any(|command| command.contains("journald"))
    );
    assert_eq!(lamp.state()["settings"][ASOUND], sha("asound 2"));
}

/// The first builder on #1680, 19:48: asound.conf restarts the voice only
/// when the lamp is quiet. Someone talks through the whole bound: nothing
/// is written, the voice is not restarted, and the next quiet round does it.
#[test]
fn asound_waits_for_a_quiet_moment_within_the_bound_and_then_for_the_next_round() {
    let (mut lamp, ran) = lamp_with_image();
    asound_build(&mut lamp, 2);
    lamp.talking = Some(Box::new(|at| at % 30_000 == 0));
    lamp.config.quiet_wait = Duration::from_secs(120);
    let start = lamp.now.get();
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::NotQuiet(why) if why.contains("no quiet moment in 120 s")),
        "{outcome:?}"
    );
    // The bound held: the round gave up after it, not before, not later.
    let waited = lamp.now.get() - start;
    assert!((120_000..=125_000).contains(&waited), "{waited}");
    assert_eq!(lamp.read(ASOUND), "asound 1");
    assert!(ran.borrow().is_empty());
    assert!(
        !lamp
            .calls()
            .iter()
            .any(|call| call == "restart lamp-voice.service" || call.contains(apply::UNIT)),
        "{:?}",
        lamp.calls()
    );
    // A turn going on, the voice speaking, is not quiet either: the gate
    // reads the voice's state as well as its speech.
    lamp.talking = Some(Box::new(|_| false));
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.read(ASOUND), "asound 2");
    assert!(
        lamp.calls()
            .iter()
            .any(|call| call == "restart lamp-voice.service")
    );
}

#[test]
fn with_image_off_the_images_files_are_not_written_and_that_is_said() {
    let (mut lamp, ran) = lamp_with_image();
    lamp.config.image = false;
    asound_build(&mut lamp, 2);
    assert_eq!(lamp.round(), Outcome::UpToDate(2));
    assert_eq!(lamp.read(ASOUND), "asound 1");
    assert!(ran.borrow().is_empty());
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("image files are not written")
                && line.contains("LAMP_UPDATE_IMAGE=on")),
        "{:?}",
        lamp.said
    );
}

/// The reviewer on #1945, 21:43: with the switch off the hardware profile
/// and the image's files are never read, so one that can't be read fails
/// nothing. On, it is said, and the rest of the build goes on.
#[test]
fn a_hardware_profile_that_cannot_be_read_never_fails_the_round() {
    for on in [false, true] {
        let (mut lamp, ran) = lamp_with_image();
        lamp.config.image = on;
        // A folder where the profile is: reading it fails, not "not found".
        fs::create_dir_all(lamp.root.join("etc/autonomous/hardware-profile")).unwrap();
        publish_image(
            &mut lamp,
            2,
            vec![image_file(ASOUND, "asound 2", &["asound 1", "asound 2"])],
        );
        assert_eq!(lamp.round(), Outcome::UpToDate(2), "{on} {:?}", lamp.said);
        assert_eq!(lamp.read(ASOUND), "asound 1");
        assert!(ran.borrow().is_empty());
        let said = |text: &str| lamp.said.iter().any(|line| line.contains(text));
        if on {
            assert!(said("cannot be read"), "{:?}", lamp.said);
        } else {
            assert!(said("LAMP_UPDATE_IMAGE=on is not set"), "{:?}", lamp.said);
            assert!(!said("cannot be read"), "{:?}", lamp.said);
        }
    }
}

#[test]
fn a_lamps_own_asound_is_kept_and_the_voice_not_restarted_for_it() {
    let (mut lamp, _) = lamp_with_image();
    fs::write(lamp.root.join("etc/asound.conf"), "this lamp's").unwrap();
    asound_build(&mut lamp, 2);
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.read(ASOUND), "this lamp's");
    assert!(
        !lamp
            .calls()
            .iter()
            .any(|call| call == "restart lamp-voice.service")
    );
    assert!(
        lamp.said
            .iter()
            .any(|line| line.starts_with(&format!("kept {ASOUND}"))),
        "{:?}",
        lamp.said
    );
}

#[test]
fn a_build_put_back_puts_the_images_files_back() {
    let (mut lamp, ran) = lamp_with_image();
    lamp.systemd.crashing.push("lamp-voice.service".into());
    asound_build(&mut lamp, 2);
    let outcome = lamp.round();
    assert!(
        matches!(outcome, Outcome::RolledBack { sequence: 2, .. }),
        "{outcome:?}"
    );
    assert_eq!(lamp.read(ASOUND), "asound 1");
    assert_eq!(lamp.read(SPEAKER), "speaker 1");
    // Reloaded once for the write and once for the put-back.
    let reloads = ran
        .borrow()
        .iter()
        .filter(|command| *command == "systemctl daemon-reload")
        .count();
    assert_eq!(reloads, 2, "{:?}", ran.borrow());
}

/// A lamp-update-back from before the field ignores `image`: the round says
/// so, and the rest of the build goes on.
#[test]
fn an_older_lamp_update_back_ignores_the_images_files_and_the_round_says_so() {
    let (mut lamp, _) = lamp_with_image();
    let (root, key) = (lamp.root.clone(), lamp.config.key);
    lamp.systemd.applier = Some(Box::new(move || {
        let records = root.join("var/lib/lampos-update");
        // An older copy: the request's `image` is a field it doesn't know.
        let request = records.join("apply/request.json");
        let mut asked: serde_json::Value =
            serde_json::from_slice(&fs::read(&request).unwrap()).unwrap();
        asked.as_object_mut().unwrap().remove("image");
        fs::write(&request, asked.to_string()).unwrap();
        apply::run(&root, &records, &key, &mut |_| Ok(()), &mut |_| {})
    }));
    asound_build(&mut lamp, 2);
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.read(ASOUND), "asound 1");
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("lamp-update-back is older") && line.contains(ASOUND)),
        "{:?}",
        lamp.said
    );
    // The reviewer on #1945, 21:43: no asound.conf written, so the voice
    // is not restarted for it.
    assert!(
        !lamp
            .calls()
            .iter()
            .any(|call| call == "restart lamp-voice.service"),
        "{:?}",
        lamp.calls()
    );
}

impl Lamp {
    /// `self.json` turned into bytes that are not JSON, written `ago`
    /// before now: before the link moved when `ago` is long enough.
    fn spoil_pending(&self, ago: Duration) {
        let path = self.records().join("self.json");
        fs::write(&path, b"{\"version\": 1, \"old\": \"").unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - ago)
            .unwrap();
    }

    /// The names of what the records set aside, as `<name>.corrupt-<ms>`.
    fn set_aside(&self, name: &str) -> Vec<String> {
        fs::read_dir(self.records())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|found| found.starts_with(&format!("{name}.corrupt-")))
            .collect()
    }

    fn logged(&self, event: &str) -> bool {
        fs::read_to_string(self.records().join("log.jsonl"))
            .unwrap_or_default()
            .contains(&format!("\"event\":\"{event}\""))
    }
}

/// #1680, #1505: a `self.json` that cannot be read, while a round knows
/// the two builds from elsewhere (the link moved after the file was
/// written, and `update/` holds one other build): the switch is taken as
/// failed, never as confirmed. The updater that runs puts the old build
/// back and refuses the new one, the file is kept aside, and the next
/// round runs.
#[test]
fn an_unreadable_self_json_with_both_builds_known_is_put_back() {
    let mut lamp = Lamp::installed_switching_itself();
    let old = lamp.own_build();
    product_build_with_updater(&mut lamp, 2, "lamp-update 2");
    let new = sha("lamp-update 2");
    assert!(matches!(lamp.round(), Outcome::SelfSwitched { .. }));
    lamp.spoil_pending(Duration::from_secs(60));

    // A round by hand is refused while it waits, as for a readable one.
    let refused = lamp.round();
    assert!(
        matches!(&refused, Outcome::Refused(why) if why.contains("cannot be read")),
        "{refused:?}"
    );
    assert_eq!(lamp.own_build(), new);

    assert_eq!(
        lamp.confirm(&new),
        own::Confirm::Unreadable(own::Unreadable::PutBack {
            old: old.clone(),
            new: new.clone()
        })
    );
    assert_eq!(lamp.own_build(), old);
    assert_eq!(own::refused(&lamp.records()), vec![new.clone()]);
    assert!(lamp.pending().is_none());
    assert_eq!(lamp.set_aside("self.json").len(), 1);
    assert!(lamp.logged("self-put-back"));
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert_eq!(lamp.own_build(), old);
}

/// The same file seen by lamp-update-back: a new updater still running
/// within its deadline (counted from the file's own time) is waited for;
/// one that failed is put back.
#[test]
fn lamp_update_back_puts_back_an_unreadable_self_json() {
    let mut lamp = Lamp::installed_switching_itself();
    let old = lamp.own_build();
    product_build_with_updater(&mut lamp, 2, "lamp-update 2");
    let new = sha("lamp-update 2");
    assert!(matches!(lamp.round(), Outcome::SelfSwitched { .. }));
    lamp.spoil_pending(Duration::from_secs(60));

    assert_eq!(
        lamp.back(update_unit("active", "running"), records_now()),
        own::Back::Waiting
    );
    assert_eq!(lamp.own_build(), new);
    assert_eq!(
        lamp.back(update_unit("failed", "failed"), records_now()),
        own::Back::Unreadable(own::Unreadable::PutBack {
            old: old.clone(),
            new: new.clone()
        })
    );
    assert_eq!(lamp.own_build(), old);
    assert_eq!(own::refused(&lamp.records()), vec![new]);
    assert_eq!(lamp.set_aside("self.json").len(), 1);
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
}

/// When the builds are not known (here the file was written after the
/// link last moved, so the link may still name the old build), nothing is
/// guessed: after the dry round a confirmation needs, the file is set
/// aside with a line, the link is left, and the next round runs. Before
/// that dry round passes, it waits.
#[test]
fn an_unreadable_self_json_with_the_builds_unknown_is_set_aside() {
    let mut lamp = Lamp::installed_switching_itself();
    product_build_with_updater(&mut lamp, 2, "lamp-update 2");
    let new = sha("lamp-update 2");
    assert!(matches!(lamp.round(), Outcome::SelfSwitched { .. }));
    std::thread::sleep(Duration::from_millis(20));
    lamp.spoil_pending(Duration::ZERO);

    let key = lamp.config.key;
    lamp.config.key = throwaway().public_key().as_ref().try_into().unwrap();
    assert!(matches!(lamp.confirm(&new), own::Confirm::NotProven(_)));
    assert!(lamp.set_aside("self.json").is_empty());
    lamp.config.key = key;

    assert_eq!(
        lamp.confirm(&new),
        own::Confirm::Unreadable(own::Unreadable::SetAside)
    );
    assert_eq!(lamp.own_build(), new);
    assert!(own::refused(&lamp.records()).is_empty());
    assert_eq!(lamp.set_aside("self.json").len(), 1);
    assert!(lamp.logged("self-set-aside"));
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
}

/// The reviewer on #2075, 06:49: a power cut inside the put-back of an
/// unreadable `self.json` never turns it around. Cut after `self-refused`
/// is written (here the link's move fails, as a cut would leave it), then
/// after the link moves: each start ends with the link on the good build,
/// the failed one refused and the file set aside.
#[test]
fn a_cut_inside_the_put_back_of_an_unreadable_self_json_never_turns_it_around() {
    // A cut after `self-refused`, before the link moves.
    let mut lamp = Lamp::installed_switching_itself();
    let old = lamp.own_build();
    product_build_with_updater(&mut lamp, 2, "lamp-update 2");
    let new = sha("lamp-update 2");
    assert!(matches!(lamp.round(), Outcome::SelfSwitched { .. }));
    lamp.spoil_pending(Duration::from_secs(60));
    let blocker = lamp.root.join("opt/lampOS/update/.current.lamp-update");
    fs::create_dir_all(blocker.join("held")).unwrap();
    let cut = own::back(
        &lamp.records(),
        &lamp.root,
        records_now(),
        &update_unit("failed", "failed"),
        &mut |_| {},
    );
    assert!(cut.is_err(), "{cut:?}");
    assert_eq!(own::refused(&lamp.records()), vec![new.clone()]);
    assert_eq!(lamp.own_build(), new);
    assert!(lamp.set_aside("self.json").is_empty());
    fs::remove_dir_all(&blocker).unwrap();
    assert_eq!(
        lamp.confirm(&new),
        own::Confirm::Unreadable(own::Unreadable::PutBack {
            old: old.clone(),
            new: new.clone()
        })
    );
    assert_eq!(lamp.own_build(), old);
    assert_eq!(own::refused(&lamp.records()), vec![new.clone()]);
    assert_eq!(lamp.set_aside("self.json").len(), 1);

    // A cut after the link moves, before the file is set aside.
    let mut lamp = Lamp::installed_switching_itself();
    let old = lamp.own_build();
    product_build_with_updater(&mut lamp, 2, "lamp-update 2");
    let new = sha("lamp-update 2");
    assert!(matches!(lamp.round(), Outcome::SelfSwitched { .. }));
    lamp.spoil_pending(Duration::from_secs(60));
    fs::write(lamp.records().join("self-refused"), format!("{new}\n")).unwrap();
    lamp_update::records::replace_link(&lamp.root.join("opt/lampOS/update/current"), &old).unwrap();
    assert_eq!(
        lamp.back(update_unit("failed", "failed"), records_now()),
        own::Back::Unreadable(own::Unreadable::PutBack {
            old: old.clone(),
            new: new.clone()
        })
    );
    assert_eq!(lamp.own_build(), old);
    assert_eq!(own::refused(&lamp.records()), vec![new]);
    assert_eq!(lamp.set_aside("self.json").len(), 1);
    assert!(lamp.pending().is_none());
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
}

/// The reviewer on #2075, 06:49: a `self.json` the disk will not give
/// (here a folder in its place), not bad JSON, is never settled: the file
/// may be fine. The round waits with a line, and the next one tries again.
#[test]
fn a_self_json_the_disk_cannot_give_is_tried_again_next_round() {
    let mut lamp = Lamp::installed_switching_itself();
    let path = lamp.records().join("self.json");
    fs::create_dir(&path).unwrap();
    let refused = lamp.round();
    assert!(
        matches!(&refused, Outcome::Refused(why) if why.contains("the next one tries again")),
        "{refused:?}"
    );
    assert!(lamp.logged("self-unread"));
    assert!(lamp.said.iter().any(|line| line.contains("tries again")));
    let back = own::back(
        &lamp.records(),
        &lamp.root,
        records_now(),
        &update_unit("failed", "failed"),
        &mut |_| {},
    );
    assert!(back.is_err(), "{back:?}");
    assert!(path.is_dir());
    assert!(lamp.set_aside("self.json").is_empty());
    fs::remove_dir(&path).unwrap();
    assert_eq!(lamp.round(), Outcome::UpToDate(1), "{:?}", lamp.said);
}

/// #1505: a `self.json` from a later version, with a field this build
/// does not know, reads as a switch that waits and is confirmed as ever.
/// A guard: it read so before, and lamp-update-back must keep reading it.
#[test]
fn a_self_json_from_a_later_version_still_reads() {
    let mut lamp = Lamp::installed_switching_itself();
    let old = lamp.own_build();
    product_build_with_updater(&mut lamp, 2, "lamp-update 2");
    let new = sha("lamp-update 2");
    assert!(matches!(lamp.round(), Outcome::SelfSwitched { .. }));
    let later = serde_json::json!({
        "version": 2,
        "old": old,
        "new": new,
        "at_unix_ms": records_now(),
        "why": "a later build's reason",
    });
    fs::write(
        lamp.records().join("self.json"),
        serde_json::to_vec(&later).unwrap(),
    )
    .unwrap();
    let pending = lamp.pending().unwrap();
    assert_eq!((pending.version, pending.new.as_str()), (2, new.as_str()));
    assert_eq!(lamp.confirm(&new), own::Confirm::Confirmed);
}

/// #1505: a `journal.json` that cannot be read no longer fails every round
/// for good: it is set aside with a line and the round goes on.
#[test]
fn an_unreadable_journal_is_set_aside_and_the_round_goes_on() {
    let mut lamp = Lamp::installed();
    fs::write(lamp.records().join("journal.json"), b"{not json").unwrap();
    assert_eq!(lamp.round(), Outcome::UpToDate(1), "{:?}", lamp.said);
    assert_eq!(lamp.set_aside("journal.json").len(), 1);
    assert!(!lamp.records().join("journal.json").exists());
    assert!(lamp.logged("journal-set-aside"));
}

const RESET_PLAN: &str = "/etc/lampOS/setup/reset-plan.json";
/// The product's reset plan as of 4de8ebe1c (19 Oct): lamp-06c1's until
/// 21 Oct, which lacked ten of the build's stores (#806).
const PLAN_19_OCT: &str = include_str!("fixtures/reset-plan-4de8ebe1c.json");
/// As of ef6a2b9a7 (20 Oct 10:36): lamp-0c89's, from its app setup.
const PLAN_20_OCT: &str = include_str!("fixtures/reset-plan-ef6a2b9a7.json");

/// Build `sequence`: build 1's binaries, the controls' setting `controls`,
/// and the build's own reset plan, as `lamp-update manifest --settings on`
/// writes it.
fn publish_with_plan(lamp: &mut Lamp, sequence: u64, controls: &str) -> Vec<u8> {
    let mut manifest = lamp.manifest(
        sequence,
        &[
            ("lamp-voice", "voice 1"),
            ("lamp-turn-end", "turn-end 1"),
            ("lamp-body", "body 1"),
            ("lamp-controls", "controls 1"),
        ],
        &[],
    );
    manifest.min_updater = Some(manifest::RESET_PLAN_UPDATER);
    manifest.settings = vec![Setting::new(CONTROLS_JSON, controls)];
    manifest.reset_plan = Some(Box::new(lamp_update::install::reset_plan_setting()));
    let bytes = serde_json::to_vec(&manifest).unwrap();
    let envelope = manifest::seal(&bytes, &lamp.key).unwrap();
    lamp.channel
        .files
        .insert(format!("{CHANNEL}{}", manifest::ENVELOPE), envelope);
    // What a lamp-update-back from before the field reads of it: the same
    // build with the plan passed over, as its lenient read does.
    manifest.reset_plan = None;
    manifest::seal(&serde_json::to_vec(&manifest).unwrap(), &lamp.key).unwrap()
}

fn put_plan(lamp: &Lamp, text: &str) {
    let at = lamp.root.join(RESET_PLAN.trim_start_matches('/'));
    fs::create_dir_all(at.parent().unwrap()).unwrap();
    fs::write(at, text).unwrap();
}

/// The first builder on #1722, 08:24: a lamp with the 19 Oct plan (and
/// lamp-0c89's of 20 Oct), updated by a round, ends with the build's plan,
/// its record, and the old one kept in the records. Never "this lamp's own".
#[test]
fn a_lamp_with_an_old_reset_plan_updated_by_a_round_ends_with_the_builds_and_a_backup() {
    for (old, sha_start) in [(PLAN_19_OCT, "5dff6f81"), (PLAN_20_OCT, "de872e78cb60")] {
        assert!(sha(old).starts_with(sha_start));
        let (mut lamp, _) = lamp_with_settings();
        put_plan(&lamp, old);
        assert!(lamp.state()["settings"].get(RESET_PLAN).is_none());
        publish_with_plan(&mut lamp, 2, "controls 2");
        assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
        assert_eq!(lamp.read(RESET_PLAN), lamp_update::install::RESET_PLAN);
        assert_eq!(
            lamp.read("/var/lib/lampos-update/reset-plan.json.before-2"),
            old
        );
        assert_eq!(
            lamp.state()["settings"][RESET_PLAN],
            sha(lamp_update::install::RESET_PLAN)
        );
        assert_eq!(lamp.read(CONTROLS_JSON), "controls 2");
        assert!(
            !lamp
                .said
                .iter()
                .any(|line| line.contains(RESET_PLAN) && line.contains("own")),
            "{:?}",
            lamp.said
        );
        // The watcher reads the plan again at each reset: nothing restarted
        // for it.
        assert!(
            !lamp.calls().iter().any(|call| call.contains("lamp-setup")),
            "{:?}",
            lamp.calls()
        );
        // The next round: nothing more to write, and no second backup.
        publish_with_plan(&mut lamp, 3, "controls 2");
        lamp.round();
        assert!(!lamp.exists("/var/lib/lampos-update/reset-plan.json.before-3"));
    }
}

/// A plan the lamp changed by hand is replaced too, and kept: a changed plan
/// can only make the reset wipe less.
#[test]
fn a_reset_plan_changed_on_the_lamp_is_replaced_by_a_round_never_kept() {
    let (mut lamp, _) = lamp_with_settings();
    put_plan(&lamp, "{\"the lamp's own\": true}\n");
    publish_with_plan(&mut lamp, 2, "controls 2");
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.read(RESET_PLAN), lamp_update::install::RESET_PLAN);
    assert_eq!(
        lamp.read("/var/lib/lampos-update/reset-plan.json.before-2"),
        "{\"the lamp's own\": true}\n"
    );
}

/// A lamp with no plan (set up by hand, as lamp-4ace) gets none from a
/// round: with setup's marker, a plan would arm the 5 s reset.
#[test]
fn a_lamp_with_no_reset_plan_gets_none_from_a_round() {
    let (mut lamp, _) = lamp_with_settings();
    publish_with_plan(&mut lamp, 2, "controls 2");
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert!(!lamp.exists(RESET_PLAN));
    assert!(lamp.state()["settings"].get(RESET_PLAN).is_none());
}

/// A build put back puts the old plan back too, byte for byte.
#[test]
fn a_build_put_back_puts_the_old_reset_plan_back() {
    let (mut lamp, _) = lamp_with_settings();
    put_plan(&lamp, PLAN_19_OCT);
    lamp.systemd.crashing.push("lamp-controls.service".into());
    publish_with_plan(&mut lamp, 2, "controls 2");
    let outcome = lamp.round();
    assert!(
        matches!(outcome, Outcome::RolledBack { sequence: 2, .. }),
        "{outcome:?}"
    );
    assert_eq!(lamp.read(RESET_PLAN), PLAN_19_OCT);
    assert!(lamp.state()["settings"].get(RESET_PLAN).is_none());
}

/// The stores rule (#1505, #1680): a lamp-update-back one version behind,
/// which knows no `reset_plan`, passes it over and writes the rest; the
/// round goes on, and says the plan waits for an install.
#[test]
fn a_lamp_update_back_from_before_the_plan_writes_the_rest_and_the_round_says_so() {
    let (mut lamp, _) = lamp_with_settings();
    put_plan(&lamp, PLAN_19_OCT);
    let older = publish_with_plan(&mut lamp, 2, "controls 2");
    let (root, key) = (lamp.root.clone(), lamp.config.key);
    lamp.systemd.applier = Some(Box::new(move || {
        let records = root.join("var/lib/lampos-update");
        fs::write(records.join("apply/envelope.json"), &older).unwrap();
        apply::run(&root, &records, &key, &mut |_| Ok(()), &mut |_| {})
    }));
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.read(CONTROLS_JSON), "controls 2");
    assert_eq!(lamp.read(RESET_PLAN), PLAN_19_OCT);
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains(RESET_PLAN) && line.contains("lamp-update-back is older")),
        "{:?}",
        lamp.said
    );
}
