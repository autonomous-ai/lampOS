use lamp_bus::{Bus, Declaration, Kind, Publisher, Subscriber};
use lamp_msgs::{SkillEvent, SkillStage};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::{
        fd::OwnedFd,
        unix::{
            fs::{OpenOptionsExt, PermissionsExt},
            net::{UnixDatagram, UnixListener, UnixStream},
            process::{CommandExt, ExitStatusExt},
        },
    },
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const INPUT: &str = "fixture/pair";
const WITHIN: Duration = Duration::from_secs(15);
const IO_WITHIN: Duration = Duration::from_secs(2);
const PAUSE: Duration = Duration::from_millis(5);
const MAX_LOG: u64 = 64 * 1024;

#[path = "rested.rs"]
mod rested;
pub use rested::check as rested_night;

#[derive(Clone, Copy)]
pub enum Release {
    Old,
    Current,
}

impl Release {
    fn number(self) -> u64 {
        match self {
            Self::Old => 6408,
            Self::Current => 6498,
        }
    }

    fn pin(self) -> (&'static str, usize, &'static str) {
        match self {
            Self::Old => (
                "2c6bc8f34c9fd4a56784a54fa2d03b1e76be9d1833ccf37db905f88dba67d3d5",
                8_807_104,
                "ee53765c8d014eb00e41c1a5a7054d2546c18bdb",
            ),
            Self::Current => (
                "87e500127793db88df8bcd7af793879faeee95f36a8e0d2ef8483c59fef26984",
                8_786_048,
                "846b3d203fc00ec3886d71f7673d8cc29cc4a634",
            ),
        }
    }

    fn copy(self, root: &Path) {
        let variable = format!("LAMP_COMPAT_SKILLS_{}", self.number());
        let source = std::env::var_os(&variable)
            .unwrap_or_else(|| panic!("manual fixture requires {variable}"));
        let (expected, size, _) = self.pin();
        let metadata = fs::metadata(&source).unwrap();
        assert!(metadata.is_file());
        assert_eq!(metadata.len(), size as u64);
        let bytes = fs::read(source).unwrap();
        assert_eq!(sha(&bytes), expected, "{variable}: published SHA-256");
        assert_eq!(&bytes[..6], b"\x7fELF\x02\x01", "ELF64 little-endian");
        assert_eq!(&bytes[18..20], &[0xb7, 0], "AArch64 executable");
        let target = root.join(format!("lamp-skills-{}", self.number()));
        write(&target, &bytes);
        fs::set_permissions(&target, fs::Permissions::from_mode(0o555)).unwrap();
    }
}

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn directory(path: &Path) {
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn output(path: &Path) -> File {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap()
}

fn write(path: &Path, bytes: &[u8]) {
    let mut file = output(path);
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}

fn json_file(path: &Path, value: &Value) {
    write(path, &serde_json::to_vec_pretty(value).unwrap());
}

fn text(path: &Path) -> String {
    let file = File::open(path).unwrap();
    assert!(
        file.metadata().unwrap().len() <= MAX_LOG,
        "bounded fixture log"
    );
    let mut result = String::new();
    file.take(MAX_LOG + 1).read_to_string(&mut result).unwrap();
    assert!(result.len() as u64 <= MAX_LOG);
    result
}

fn wall_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .try_into()
        .unwrap()
}

struct Case {
    root: PathBuf,
    transcript: File,
    records: usize,
    deadline: Instant,
    offset: i32,
    day: i64,
}

impl Case {
    fn new(writer: Release, reader: Release, kind: &str) -> Self {
        assert_eq!(std::env::consts::OS, "linux", "manual native fixture");
        assert_eq!(std::env::consts::ARCH, "aarch64", "manual native fixture");
        let parent = std::env::var_os("LAMP_COMPAT_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        assert!(parent.is_absolute() && parent.is_dir());
        // Retained on success and failure, never reused by another case.
        let root = tempfile::Builder::new()
            .prefix("kr-")
            .tempdir_in(parent)
            .unwrap()
            .keep();
        assert!(
            root.as_os_str().as_encoded_bytes().len() <= 40,
            "use a short output parent"
        );
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        directory(&root.join("skills"));
        directory(&root.join("state"));
        writer.copy(&root);
        reader.copy(&root);
        // Freeze a valid fixed offset placing this instant near local noon.
        // The real released clock still supplies wall time. A test cannot
        // cross a midnight/window edge unnoticed, or move the host's clock.
        let wall = wall_ms();
        let minute = (wall / 60_000).rem_euclid(1440);
        let offset = ((720 - minute + 720).rem_euclid(1440) - 720) as i32;
        let day = (wall + i64::from(offset) * 60_000).div_euclid(86_400_000);
        json_file(
            &root.join("case.json"),
            &json!({
                "kind":kind,"writer":writer.number(),"reader":reader.number(),
                "return_reader":writer.number(),"writer_sha256":writer.pin().0,
                "reader_sha256":reader.pin().0,"writer_bytes":writer.pin().1,
                "reader_bytes":reader.pin().1,"writer_source":writer.pin().2,
                "reader_source":reader.pin().2,"sampled_unix_ms":wall,
                "utc_offset_minutes":offset,"local_day":day,
                "daily_window":{"from":"11:00","to":"13:00"},
                "boundary":"released daily/on/off readers and writers; Policy never approves; no actuator",
                "excluded":["rested","physical output","updater install","power-loss durability"]
            }),
        );
        println!("skills release-pair evidence: {}", root.display());
        Self {
            transcript: output(&root.join("transcript.jsonl")),
            root,
            records: 0,
            deadline: Instant::now() + Duration::from_secs(90),
            offset,
            day,
        }
    }

    fn record(&mut self, value: Value) {
        assert!(self.records < 1024, "bounded transcript");
        serde_json::to_writer(&mut self.transcript, &value).unwrap();
        self.transcript.write_all(b"\n").unwrap();
        self.transcript.flush().unwrap();
        self.records += 1;
    }

    fn within(&self) -> Instant {
        self.deadline.min(Instant::now() + WITHIN)
    }

    fn clock_unchanged(&self) {
        assert!(Instant::now() < self.deadline, "case deadline");
        let local = wall_ms() + i64::from(self.offset) * 60_000;
        assert_eq!(local.div_euclid(86_400_000), self.day, "local day changed");
        assert!(
            (660..780).contains(&(local / 60_000).rem_euclid(1440)),
            "outside frozen window"
        );
    }

    fn binary(&self, release: Release) -> PathBuf {
        self.root.join(format!("lamp-skills-{}", release.number()))
    }

    fn skill(&self, name: &str, enabled: bool, triggers: &str) {
        let folder = self.root.join("skills").join(name);
        directory(&folder);
        write(
            &folder.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: A synthetic state continuity check.\n---\n")
                .as_bytes(),
        );
        write(
            &folder.join("skill.toml"),
            format!(
                "kind = \"core\"\nversion = \"1.0.0\"\nenabled = {enabled}\n\n[permissions]\ntopics = [\"{INPUT}\"]\nbody = true\n\n{triggers}\n[ask]\npolicy = \"nudge\"\n\n[[do]]\nbody = {{ gesture = \"nod\" }}\n"
            ).as_bytes(),
        );
    }

    fn snapshot(&mut self, name: &str) -> BTreeMap<String, Vec<u8>> {
        self.clock_unchanged();
        let dest = self.root.join(name);
        directory(&dest);
        let mut bytes = BTreeMap::new();
        let mut files = Vec::new();
        for filename in ["daily.json", "on", "off"] {
            let path = self.root.join("state").join(filename);
            match fs::read(&path) {
                Ok(contents) => {
                    assert!(contents.len() <= 1024 * 1024);
                    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o7777;
                    write(&dest.join(filename), &contents);
                    files.push(json!({"file":filename,"bytes":contents.len(),"mode":mode,"sha256":sha(&contents)}));
                    bytes.insert(filename.to_owned(), contents);
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    files.push(json!({"file":filename,"absent":true}));
                }
                Err(error) => panic!("snapshot: {error}"),
            }
        }
        let manifests = dest.join("manifests");
        directory(&manifests);
        let mut folders: Vec<_> = fs::read_dir(self.root.join("skills"))
            .unwrap()
            .map(Result::unwrap)
            .filter(|entry| entry.file_type().unwrap().is_dir())
            .collect();
        folders.sort_by_key(|entry| entry.file_name());
        for folder in folders {
            let filename = format!("{}.toml", folder.file_name().to_str().unwrap());
            let contents = fs::read(folder.path().join("skill.toml")).unwrap();
            write(&manifests.join(&filename), &contents);
            files.push(json!({"file":format!("manifests/{filename}"),"bytes":contents.len(),"sha256":sha(&contents)}));
        }
        json_file(&dest.join("files.json"), &json!(files));
        self.record(json!({"event":"snapshot","name":name,"files":files}));
        bytes
    }

    fn finish(&mut self) {
        let mut artifacts = Vec::new();
        for release in [Release::Old, Release::Current] {
            let path = self.binary(release);
            let bytes = fs::read(&path).unwrap();
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o7777;
            assert_eq!(sha(&bytes), release.pin().0);
            assert_eq!(bytes.len(), release.pin().1);
            assert_eq!(mode, 0o555);
            artifacts.push(json!({"release":release.number(),"sha256":sha(&bytes),"bytes":bytes.len(),"mode":mode}));
        }
        json_file(&self.root.join("artifacts-after.json"), &json!(artifacts));
        self.record(json!({"event":"case_complete","artifacts_unchanged":true}));
        self.transcript.sync_all().unwrap();
    }
}

struct OwnedChild {
    child: Child,
    stage: String,
    root: PathBuf,
    reaped: bool,
}

impl OwnedChild {
    fn spawn(case: &mut Case, release: Release, stage: &str, mut command: Command) -> Self {
        let arguments: Vec<_> = command
            .get_args()
            .map(|word| word.to_string_lossy().into_owned())
            .collect();
        let environment: BTreeMap<_, _> = command
            .get_envs()
            .filter_map(|(name, value)| value.map(|value| (name.to_owned(), value.to_owned())))
            .collect();
        let child = command
            .env_clear()
            .envs(environment)
            .current_dir(&case.root)
            .stderr(Stdio::from(output(
                &case.root.join(format!("{stage}.stderr")),
            )))
            .process_group(0)
            .spawn()
            .unwrap();
        // Cleanup owns the process before /proc or any assertion can fail.
        let owned = Self {
            child,
            stage: stage.into(),
            root: case.root.clone(),
            reaped: false,
        };
        let executable = fs::read(format!("/proc/{}/exe", owned.child.id())).unwrap();
        assert_eq!(
            sha(&executable),
            release.pin().0,
            "actual running executable"
        );
        case.record(json!({"event":"spawn","stage":stage,"pid":owned.child.id(),
            "release":release.number(),"executable_sha256":sha(&executable),"argv":arguments}));
        owned
    }

    fn alive(&mut self, until: Instant, waiting: &str) {
        let status = self.child.try_wait().unwrap();
        if status.is_some() {
            self.reaped = true;
        }
        assert!(
            status.is_none() && Instant::now() < until,
            "{waiting}: child={status:?}; {}",
            self.log()
        );
    }

    fn log(&self) -> String {
        text(&self.root.join(format!("{}.stderr", self.stage)))
    }

    fn wait(&mut self, case: &mut Case) -> ExitStatus {
        let until = case.within();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                self.reaped = true;
                let receipt = json!({"event":"reaped","stage":self.stage,"pid":self.child.id(),
                    "code":status.code(),"signal":status.signal()});
                json_file(
                    &self.root.join(format!("{}.exit.json", self.stage)),
                    &receipt,
                );
                case.record(receipt);
                return status;
            }
            assert!(
                Instant::now() < until,
                "child exit deadline: {}",
                self.log()
            );
            std::thread::sleep(PAUSE);
        }
    }

    fn kill(&mut self, case: &mut Case) {
        self.child.kill().unwrap();
        let status = self.wait(case);
        assert_eq!(status.signal(), Some(9), "actual SIGKILL and reap");
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        let _ = self.child.kill();
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(Some(status)) = self.child.try_wait() {
                let receipt = json!({"pid":self.child.id(),"code":status.code(),"signal":status.signal(),"reaped":true});
                let _ = fs::write(
                    self.root
                        .join(format!("{}.failure-cleanup.json", self.stage)),
                    receipt.to_string(),
                );
                return;
            }
            if Instant::now() >= until {
                let _ = fs::write(
                    self.root
                        .join(format!("{}.failure-cleanup.json", self.stage)),
                    b"{\"reaped\":false}",
                );
                eprintln!("Skills fixture cleanup did not confirm reap within five seconds");
                return;
            }
            std::thread::sleep(PAUSE);
        }
    }
}

struct Traps {
    datagrams: Vec<UnixDatagram>,
    streams: Vec<UnixListener>,
}

impl Traps {
    fn new(root: &Path) -> Self {
        let datagrams = ["gaze", "mood"]
            .into_iter()
            .map(|name| {
                let socket = UnixDatagram::bind(root.join(name)).unwrap();
                socket.set_nonblocking(true).unwrap();
                socket
            })
            .collect();
        let streams = ["actions", "say"]
            .into_iter()
            .map(|name| {
                let socket = UnixListener::bind(root.join(name)).unwrap();
                socket.set_nonblocking(true).unwrap();
                socket
            })
            .collect();
        Self { datagrams, streams }
    }

    fn empty(&self) {
        for socket in &self.datagrams {
            let mut bytes = [0; 4096];
            assert!(
                matches!(socket.recv(&mut bytes), Err(error) if error.kind() == io::ErrorKind::WouldBlock),
                "unexpected actuator datagram"
            );
        }
        for socket in &self.streams {
            assert!(
                matches!(socket.accept(), Err(error) if error.kind() == io::ErrorKind::WouldBlock),
                "unexpected Actions or Say connection"
            );
        }
    }
}

struct Daemon {
    process: OwnedChild,
    input: Publisher,
    events: Subscriber,
    clock: PathBuf,
    traps: Traps,
}

impl Daemon {
    fn spawn(case: &mut Case, release: Release, stage: &str, daily: bool) -> Self {
        let private = case.root.join(stage);
        directory(&private);
        let traps = Traps::new(&private);
        let bus = Bus::new(private.join("b"))
            .unwrap()
            .rescan_every(Duration::ZERO);
        let input = bus
            .publisher_declared(
                INPUT,
                &Declaration::new("fixture/Phase", 1, Kind::Latest),
                "fixture",
            )
            .unwrap();
        let events = bus.subscribe(SkillEvent::TOPIC, "fixture").unwrap();
        let clock = private.join("clock");
        let mut command = Command::new(case.binary(release));
        command
            .env_clear()
            .env("LAMP_SKILLS_ASK", "policy")
            .args([
                "--dir",
                "skills",
                "--state-dir",
                "state",
                "--trust-clock",
                "--utc-offset-minutes",
            ])
            .arg(case.offset.to_string())
            .arg("--bus-root")
            .arg(private.join("b"))
            .arg("--clock-control")
            .arg(format!("{stage}/clock"))
            .arg("--gaze-socket")
            .arg(format!("{stage}/gaze"))
            .arg("--mood-socket")
            .arg(format!("{stage}/mood"))
            .arg("--actions-socket")
            .arg(format!("{stage}/actions"))
            .arg("--say-socket")
            .arg(format!("{stage}/say"))
            .stdin(Stdio::null())
            .stdout(Stdio::from(output(
                &case.root.join(format!("{stage}.stdout")),
            )));
        if daily {
            command.env("LAMP_SKILLS_DAILY_FILE", "state/daily.json");
        }
        let mut process = spawn_recorded(case, release, stage, command);
        let until = case.within();
        while !clock.exists()
            || !bus
                .subscribers(INPUT)
                .unwrap()
                .iter()
                .any(|listener| listener.listening)
        {
            process.alive(until, "Skills loaded and subscribed");
            std::thread::sleep(PAUSE);
        }
        let mut daemon = Self {
            process,
            input,
            events,
            clock,
            traps,
        };
        daemon.barrier(case);
        assert!(daemon.drain(case).is_empty());
        daemon
    }

    fn barrier(&mut self, case: &mut Case) {
        let until = case.within();
        self.process.alive(case.within(), "clock barrier");
        case.clock_unchanged();
        let mut stream = UnixStream::connect(&self.clock).unwrap();
        stream.set_read_timeout(Some(IO_WITHIN)).unwrap();
        stream.set_write_timeout(Some(IO_WITHIN)).unwrap();
        let deadline = until.min(Instant::now() + IO_WITHIN);
        stream.write_all(b"{\"skip_us\":0}\n").unwrap();
        let answer = read_answer(stream, 1024, deadline);
        assert!(answer["now_us"].is_u64(), "{answer}");
        case.record(json!({"event":"drained_before_clock_barrier","stage":self.process.stage,"answer":answer}));
        self.traps.empty();
    }

    fn drain(&mut self, case: &mut Case) -> Vec<SkillEvent> {
        let mut events = Vec::new();
        while let Some(message) = self.events.recv(Duration::ZERO).unwrap() {
            assert_eq!(message.header.source, "skills");
            assert_eq!(message.gap, 0, "fixture lost a Skills event");
            let event: SkillEvent = message.parse().unwrap();
            assert_ne!(
                event.stage,
                SkillStage::Done,
                "Policy never approved an output"
            );
            if event.stage == SkillStage::Answered {
                assert_eq!(event.allowed, Some(false));
            }
            case.record(json!({"event":"skills_event","stage":self.process.stage,"message":event}));
            events.push(event);
            assert!(events.len() < 32);
        }
        events
    }

    fn drive(&mut self, case: &mut Case, phase: &str) -> Vec<SkillEvent> {
        self.barrier(case);
        assert!(self.drain(case).is_empty(), "unexpected prior run");
        let sent = self
            .input
            .publish_json(lamp_ipc::monotonic_us(), &json!({"phase":phase}))
            .unwrap();
        assert_eq!(sent.delivered, 1);
        assert_eq!(sent.dropped, 0);
        case.record(json!({"event":"input","stage":self.process.stage,"phase":phase,"sequence":self.input.totals().published}));
        self.barrier(case);
        self.drain(case)
    }

    fn denied(&mut self, case: &mut Case, phase: &str, skill: &str) {
        let events = self.drive(case, phase);
        assert_eq!(
            events.len(),
            3,
            "one triggered/asked/refused run: {events:?}"
        );
        assert!(events.iter().all(|event| event.skill == skill));
        assert_eq!(
            events.iter().map(|event| event.stage).collect::<Vec<_>>(),
            [
                SkillStage::Triggered,
                SkillStage::Asked,
                SkillStage::Answered
            ]
        );
        assert!(
            events[2].why.contains("nobody is subscribed to skills/ask"),
            "{events:?}"
        );
    }

    fn blocked(&mut self, case: &mut Case, phase: &str, why: &str) {
        assert!(self.drive(case, phase).is_empty());
        let until = case.within();
        while !self.process.log().contains(why) {
            self.process
                .alive(until, "the explicit persisted-state refusal");
            std::thread::sleep(PAUSE);
        }
        case.record(json!({"event":"persisted_state_refused_trigger","stage":self.process.stage,"phase":phase,"reason":why}));
    }

    fn reset_consumed_daily(&mut self, case: &mut Case, phase: &str) {
        let events = self.drive(case, "neutral");
        assert_eq!(
            events.len(),
            1,
            "one consumed daily watch reset: {events:?}"
        );
        let event = &events[0];
        assert_eq!(event.skill, "daily-check");
        assert_eq!(event.stage, SkillStage::Answered);
        assert_eq!(event.allowed, Some(false));
        let prefix = format!("{phase} watch reset after ");
        let suffix = format!(" s: stopped holding: {INPUT} phase no longer {phase}");
        let seconds = event
            .why
            .strip_prefix(prefix.as_str())
            .and_then(|why| why.strip_suffix(suffix.as_str()))
            .expect("the consumed named trigger must reset because neutral no longer holds it");
        assert!(seconds.parse::<u64>().is_ok(), "{event:?}");
    }

    fn stop(mut self, case: &mut Case) {
        self.barrier(case);
        assert!(self.drain(case).is_empty());
        self.process.kill(case);
        self.traps.empty();
        let log = self.process.log();
        assert!(
            !log.contains("not loaded:") && !log.contains("daily chances disabled:"),
            "{log}"
        );
    }
}

// Record only explicitly supplied variables. OwnedChild preserves that exact
// set while clearing inherited settings, including provider and Store switches.
fn spawn_recorded(case: &mut Case, release: Release, stage: &str, command: Command) -> OwnedChild {
    let environment = command
        .get_envs()
        .filter_map(|(name, value)| {
            value.map(|value| {
                (
                    name.to_string_lossy().into_owned(),
                    value.to_string_lossy().into_owned(),
                )
            })
        })
        .collect::<BTreeMap<_, _>>();
    case.record(json!({"event":"environment","stage":stage,"values":environment}));
    OwnedChild::spawn(case, release, stage, command)
}

fn read_answer(mut stream: UnixStream, most: u64, deadline: Instant) -> Value {
    let line = reply_bytes(&mut stream, most as usize, deadline).unwrap();
    serde_json::from_slice(&line).unwrap()
}

fn trigger(name: &str, daily: bool) -> String {
    let window = if daily {
        "during = { from = \"11:00\", to = \"13:00\" }\nonce_per_day = true\n"
    } else {
        ""
    };
    format!(
        "[[trigger]]\nname = \"{name}\"\ntopic = \"{INPUT}\"\nfield = \"phase\"\nequals = \"{name}\"\nrearm = \"ends\"\n{window}\n"
    )
}

fn daily_days(bytes: &[u8], names: &[&str], day: i64) {
    // Inspect the documented record bytes, never call today's Journal reader
    // to stand in for an old release. Each fixture trigger consumes one day.
    let state: Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(state["schema"], 2);
    let entries = state["consumed"].as_array().unwrap();
    assert_eq!(entries.len(), names.len());
    let mut seen = Vec::new();
    for entry in entries {
        assert_eq!(entry["skill"], "daily-check");
        let name = entry["trigger"].as_str().unwrap();
        assert!(names.contains(&name));
        assert!(!seen.contains(&name));
        seen.push(name);
        let hex = entry["days"].as_str().unwrap().as_bytes();
        assert!(hex.len().is_multiple_of(2));
        let mut set = Vec::new();
        for (index, pair) in hex.chunks_exact(2).enumerate() {
            let byte = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
            for bit in 0..8 {
                if byte & (1 << bit) != 0 {
                    // Schema 2's first representable day is two days before
                    // 2020-01-01 UTC, including supported local offsets.
                    set.push(18_260 + (index * 8 + bit) as i64);
                }
            }
        }
        assert_eq!(set, [day], "exact one-day claim for {name}");
    }
}

pub fn daily(writer: Release, reader: Release) {
    let mut case = Case::new(writer, reader, "daily");
    case.skill(
        "daily-check",
        true,
        &["first", "second", "third"]
            .map(|name| trigger(name, true))
            .concat(),
    );
    case.skill("control-check", true, &trigger("control", false));
    let mut first = Daemon::spawn(&mut case, writer, "a", true);
    first.denied(&mut case, "first", "daily-check");
    first.stop(&mut case);
    let a = case.snapshot("after-a");
    daily_days(&a["daily.json"], &["first"], case.day);
    assert_eq!(
        fs::metadata(case.root.join("state/daily.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o600
    );

    let mut second = Daemon::spawn(&mut case, reader, "b", true);
    second.blocked(
        &mut case,
        "first",
        "daily-check not run: its day already claimed",
    );
    second.reset_consumed_daily(&mut case, "first");
    assert_eq!(
        fs::read(case.root.join("state/daily.json")).unwrap(),
        a["daily.json"],
        "read/replay changed the original journal"
    );
    second.denied(&mut case, "control", "control-check");
    second.denied(&mut case, "second", "daily-check");
    second.stop(&mut case);
    let b = case.snapshot("after-b");
    daily_days(&b["daily.json"], &["first", "second"], case.day);

    let mut returned = Daemon::spawn(&mut case, writer, "c", true);
    returned.blocked(
        &mut case,
        "first",
        "daily-check not run: its day already claimed",
    );
    returned.reset_consumed_daily(&mut case, "first");
    // The production diagnostic is rate-limited per skill, so the second
    // key is checked against the explicit drain barrier and byte snapshot,
    // then a fresh third key proves this reader still admits new claims.
    assert!(returned.drive(&mut case, "second").is_empty());
    returned.reset_consumed_daily(&mut case, "second");
    assert_eq!(
        fs::read(case.root.join("state/daily.json")).unwrap(),
        b["daily.json"]
    );
    returned.denied(&mut case, "control", "control-check");
    returned.denied(&mut case, "third", "daily-check");
    returned.stop(&mut case);
    let c = case.snapshot("after-return");
    daily_days(&c["daily.json"], &["first", "second", "third"], case.day);
    assert_eq!(c.len(), 1, "daily fixture creates no on/off records");
    assert!(!case.root.join("state/rested").exists());
    case.finish();
}

fn asked(case: &mut Case, release: Release, stage: &str, op: &str, name: &str) {
    let until = case.within();
    case.clock_unchanged();
    let (mut parent, child) = UnixStream::pair().unwrap();
    parent.set_read_timeout(Some(IO_WITHIN)).unwrap();
    parent.set_write_timeout(Some(IO_WITHIN)).unwrap();
    let mut command = Command::new(case.binary(release));
    command
        .env_clear()
        .env("LAMP_SKILLS_ANY_ASKER", "on")
        .args(["asked", "--dir", "skills", "--state-dir", "state"])
        .stdin(Stdio::from(OwnedFd::from(child.try_clone().unwrap())))
        .stdout(Stdio::from(OwnedFd::from(child)));
    let mut child = spawn_recorded(case, release, stage, command);
    let request = json!({(op):name});
    let deadline = until.min(Instant::now() + IO_WITHIN);
    writeln!(parent, "{request}").unwrap();
    parent.shutdown(std::net::Shutdown::Write).unwrap();
    let answer = read_answer(parent, 4096, deadline);
    json_file(&case.root.join(format!("{stage}.response.json")), &answer);
    case.record(json!({"event":"asked","stage":stage,"request":request,"answer":answer}));
    assert_eq!(answer, json!({"done":format!("turned {name} {op}")}));
    assert!(child.wait(case).success());
}

fn enabled(case: &Case, name: &str, expected: bool) {
    let contents = text(&case.root.join("skills").join(name).join("skill.toml"));
    let line = format!("enabled = {expected}");
    assert!(contents.lines().any(|given| given == line));
}

pub fn on_off(writer: Release, reader: Release) {
    let mut case = Case::new(writer, reader, "on-off");
    case.skill("toggle-check", false, &trigger("toggle", false));
    case.skill("plain-check", true, &trigger("plain", false));
    case.skill("control-check", true, &trigger("control", false));
    asked(&mut case, writer, "a-on", "on", "toggle-check");
    asked(&mut case, writer, "a-off", "off", "plain-check");
    enabled(&case, "toggle-check", true);
    enabled(&case, "plain-check", true);
    let a = case.snapshot("after-a");
    assert_eq!(a["on"], b"toggle-check\n");
    assert_eq!(a["off"], b"plain-check\n");

    let mut second = Daemon::spawn(&mut case, reader, "b", false);
    second.blocked(&mut case, "plain", "plain-check not run: off");
    second.denied(&mut case, "toggle", "toggle-check");
    second.denied(&mut case, "control", "control-check");
    second.stop(&mut case);
    assert_eq!(
        case.snapshot("after-b-read"),
        a,
        "daemon reads do not rewrite on/off"
    );

    // The opposite actual helper reads the `on` entry to restore the
    // shipped-off manifest, and removes the other actual writer's `off`.
    asked(&mut case, reader, "b-off", "off", "toggle-check");
    asked(&mut case, reader, "b-on", "on", "plain-check");
    enabled(&case, "toggle-check", false);
    enabled(&case, "plain-check", true);
    let b = case.snapshot("after-b-write");
    assert_eq!(b["on"], b"");
    assert_eq!(b["off"], b"toggle-check\n");

    let mut returned = Daemon::spawn(&mut case, writer, "c", false);
    returned.denied(&mut case, "plain", "plain-check");
    returned.denied(&mut case, "control", "control-check");
    assert!(returned.drive(&mut case, "toggle").is_empty());
    returned.stop(&mut case);
    assert_eq!(case.snapshot("after-return-read"), b);
    asked(&mut case, writer, "c-on", "on", "toggle-check");
    asked(&mut case, writer, "c-off", "off", "plain-check");
    enabled(&case, "toggle-check", true);
    assert_eq!(case.snapshot("after-return-write"), a);
    assert!(!case.root.join("state/daily.json").exists());
    assert!(!case.root.join("state/rested").exists());
    case.finish();
}

// Socket timeouts are per read. One deadline prevents partial bytes from
// renewing the original I/O, step or case budget.
fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "absolute reply deadline"))
}
fn reply_bytes(stream: &mut UnixStream, most: usize, deadline: Instant) -> io::Result<Vec<u8>> {
    let mut line = Vec::new();
    loop {
        stream.set_read_timeout(Some(remaining(deadline)?))?;
        let mut chunk = [0; 1024];
        let n = match stream.read(&mut chunk) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            other => other?,
        };
        remaining(deadline)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "incomplete reply",
            ));
        }
        if let Some(end) = chunk[..n].iter().position(|b| *b == b'\n') {
            line.extend_from_slice(&chunk[..=end]);
            if line.len() > most {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "oversized reply",
                ));
            }
            return Ok(line);
        }
        line.extend_from_slice(&chunk[..n]);
        if line.len() >= most {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "oversized reply",
            ));
        }
    }
}
#[test]
fn complete_reply_finishes_inside_absolute_deadline() {
    let (mut reader, mut peer) = UnixStream::pair().unwrap();
    peer.write_all(b"{\"ok\":true}\n").unwrap();
    let line = reply_bytes(&mut reader, 1024, Instant::now() + Duration::from_secs(1)).unwrap();
    assert_eq!(line, b"{\"ok\":true}\n");
}
#[test]
fn dribbling_reply_cannot_renew_absolute_deadline() {
    let (mut reader, mut peer) = UnixStream::pair().unwrap();
    peer.set_write_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let writer = std::thread::spawn(move || {
        for _ in 0..100 {
            if peer.write_all(b" ").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    let began = Instant::now();
    let error = reply_bytes(&mut reader, 1024, began + Duration::from_millis(100)).unwrap_err();
    assert!(matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    ));
    assert!(
        began.elapsed() < Duration::from_millis(500),
        "dribbling renewed the budget"
    );
    drop(reader);
    writer.join().unwrap();
}
