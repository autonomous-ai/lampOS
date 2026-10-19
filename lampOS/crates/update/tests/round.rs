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
    Config, Outcome, World,
    fetch::{Channel, Fetch},
    judge::UnitState,
    manifest::{self, File, Manifest, Unit, sha256_hex},
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
}

impl Channel for FakeChannel {
    fn fetch(&mut self, url: &str, most: u64, out: &mut dyn Write) -> Result<u64, Fetch> {
        if self.offline {
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
    /// `systemctl show` fails from the first restart on.
    unreachable_after_restart: bool,
    unreachable: bool,
}

impl FakeSystemd {
    fn start(&mut self, unit: &str) {
        let text = fs::read_to_string(self.root.join("etc/systemd/system").join(unit)).unwrap();
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
        let running = if binary.contains("crash") || self.crashing.iter().any(|name| name == unit) {
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
    fn states(&mut self, units: &[String]) -> Result<Vec<UnitState>, String> {
        if self.unreachable {
            return Err("Failed to connect to bus".into());
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
        Ok(())
    }

    fn restart(&mut self, unit: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("restart {unit}"));
        self.restarted_at.push((unit.to_owned(), self.now.get()));
        self.unreachable |= self.unreachable_after_restart;
        self.start(unit);
        Ok(())
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
            unreachable_after_restart: false,
            unreachable: false,
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
        }
    }

    fn round(&mut self) -> Outcome {
        let clock = FakeClock(Rc::clone(&self.now));
        let talking = self.talking.take().unwrap_or_else(|| Box::new(|_| false));
        let mut ears = FakeEars {
            now: Rc::clone(&self.now),
            talking,
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
    let prepare = |root: &Path| install::provision_device_key(root, install::RECORDS, &mut |_| {});
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
    // Switched voice first, then the body; put back the body first, then
    // the voice (which failed, so without waiting).
    assert_eq!(
        lamp.calls(),
        [
            "reload",
            "restart lamp-voice.service",
            "restart lamp-body.service",
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
    // The voice first, then the rest by name; one reload for the voice's unit.
    assert_eq!(
        lamp.calls(),
        [
            "reload",
            "restart lamp-voice.service",
            "restart lamp-eyes.service"
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
    let listed =
        std::process::Command::new(lamp.root.join("opt/lampOS/skills/current/lamp-skills"))
            .args(["list", "--dir"])
            .arg(lamp.root.join("var/lib/lampOS/skills"))
            .output()
            .unwrap();
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
        let listed =
            std::process::Command::new(self.root.join("opt/lampOS/skills/current/lamp-skills"))
                .args(["list", "--dir"])
                .arg(self.root.join(PRODUCT_SKILLS))
                .output()
                .unwrap();
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
/// file of one the lamp changed, and a skill the person installed.
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
/// and switches, every one loaded. The person's own skill is not touched,
/// and a file of a product skill the lamp changed is kept.
#[test]
fn a_round_places_the_builds_product_skills_and_keeps_the_lamps_own() {
    let mut lamp = lamp_before_permissions();
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
    // The lamp's own file and the person's skill, as they were.
    assert_eq!(
        lamp.skill_file("reminders/SKILL.md").unwrap(),
        "Reminders, as this lamp's person wrote them."
    );
    assert_eq!(
        (
            lamp.skill_file("mine/skill.toml"),
            lamp.skill_file("mine/SKILL.md")
        ),
        mine_before
    );
    assert!(
        lamp.said
            .iter()
            .any(|line| line
                .contains("kept /var/lib/lampOS/skills/reminders/SKILL.md: this lamp's own")),
        "{:?}",
        lamp.said
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

    fn restart(&mut self, _: &str) -> Result<(), String> {
        Ok(())
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
        product_build(&mut lamp, 1);
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
        lamp_update::install::install(&config, &mut lamp.channel, &mut host, &mut |_| {}).unwrap();
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
