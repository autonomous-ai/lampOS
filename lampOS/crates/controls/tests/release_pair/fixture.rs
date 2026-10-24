use rustix::{
    io::Errno,
    process::{Pid, Signal, kill_process_group, test_kill_process_group},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

const PIN: &str = "fc3f73157e21c6b3f662ee469f5febeeaa56bbc84091e0b781157192a3961555";
const BYTES: usize = 2_463_440;
const SOURCES: [&str; 2] = [
    "ee53765c8d014eb00e41c1a5a7054d2546c18bdb",
    "846b3d203fc00ec3886d71f7673d8cc29cc4a634",
];
const CASE: Duration = Duration::from_secs(60);
const STEP: Duration = Duration::from_secs(5);
const MAX_FILE: u64 = 131_072;
const CANARY: &str = "synthetic-value-never-for-the-journal";
static NEXT: AtomicU64 = AtomicU64::new(0);

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn read(path: &Path, limit: u64) -> Vec<u8> {
    let meta = fs::symlink_metadata(path).unwrap();
    assert!(
        meta.is_file() && meta.len() <= limit,
        "bounded regular file: {}",
        path.display()
    );
    let mut bytes = Vec::new();
    File::open(path)
        .unwrap()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(bytes.len() as u64 <= limit);
    bytes
}
fn write(path: &Path, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}

#[derive(Debug, PartialEq, Eq)]
struct Image {
    hash: String,
    size: u64,
    mode: u32,
    inode: u64,
    device: u64,
    mtime: i64,
    mtime_ns: i64,
}
fn image(path: &Path) -> Image {
    let bytes = read(path, MAX_FILE);
    let meta = fs::symlink_metadata(path).unwrap();
    assert_eq!(meta.nlink(), 1);
    Image {
        hash: sha(&bytes),
        size: meta.len(),
        mode: meta.mode() & 0o7777,
        inode: meta.ino(),
        device: meta.dev(),
        mtime: meta.mtime(),
        mtime_ns: meta.mtime_nsec(),
    }
}

struct Owned {
    child: Child,
    cleanup: PathBuf,
    done: bool,
}
impl Owned {
    fn group(&self) -> Option<Pid> {
        let id = i32::try_from(self.child.id()).ok()?;
        if id <= 1 {
            return None;
        }
        Pid::from_raw(id)
    }
    fn absent(&self) -> bool {
        self.group()
            .is_some_and(|group| test_kill_process_group(group) == Err(Errno::SRCH))
    }
    fn wait(&mut self, until: Instant) -> io::Result<ExitStatus> {
        loop {
            if Instant::now() >= until {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "original Controls command deadline",
                ));
            }
            if let Some(status) = self.child.try_wait()? {
                if !self.absent() {
                    return Err(io::Error::other("owned process group remains after reap"));
                }
                self.done = true;
                if Instant::now() >= until {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Controls command completed after original deadline",
                    ));
                }
                return Ok(status);
            }
            if Instant::now() >= until {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "original Controls command deadline",
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        if let Some(group) = self.group() {
            let _ = kill_process_group(group, Signal::KILL);
        } else {
            let _ = self.child.kill();
        }
        let until = Instant::now() + STEP;
        let mut reaped = false;
        loop {
            reaped |= self.child.try_wait().ok().flatten().is_some();
            if (reaped && self.absent()) || Instant::now() >= until {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let _ = fs::write(
            &self.cleanup,
            json!({"pid":self.child.id(),"reaped":reaped,"group_absent":self.absent()}).to_string(),
        );
    }
}

struct Binary {
    path: PathBuf,
    hash: String,
    bytes: usize,
    release: u64,
    source: &'static str,
}
struct Case {
    root: PathBuf,
    data: PathBuf,
    until: Instant,
    commands: usize,
    binaries: Vec<Binary>,
}
impl Case {
    fn new(actual: bool) -> Self {
        if actual {
            assert_eq!(std::env::consts::OS, "linux");
            assert_eq!(std::env::consts::ARCH, "aarch64");
        }
        let parent = std::env::var_os("LAMP_COMPAT_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        assert!(parent.is_absolute() && parent.is_dir());
        let root = (0..100)
            .find_map(|_| {
                let path = parent.join(format!(
                    "controls-pair-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::DirBuilder::new().mode(0o700).create(&path) {
                    Ok(()) => Some(path),
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => None,
                    Err(e) => panic!("cannot create private fixture: {e}"),
                }
            })
            .expect("exclusive fixture directory");
        println!("Controls pair evidence: {}", root.display());
        let data = root.join("data");
        fs::create_dir(&data).unwrap();
        fs::set_permissions(&data, fs::Permissions::from_mode(0o700)).unwrap();
        fs::create_dir(root.join("steps")).unwrap();
        let mut binaries = Vec::new();
        for (index, release) in [6408_u64, 6498].into_iter().enumerate() {
            let input = if actual {
                PathBuf::from(
                    std::env::var_os(format!("LAMP_COMPAT_CONTROLS_{release}"))
                        .expect("explicit pinned Controls input"),
                )
            } else {
                PathBuf::from(env!("CARGO_BIN_EXE_lamp-controls"))
            };
            assert!(input.is_absolute());
            let bytes = read(&input, 64 * 1024 * 1024);
            let hash = sha(&bytes);
            if actual {
                assert_eq!(bytes.len(), BYTES);
                assert_eq!(hash, PIN);
                assert_eq!(&bytes[..6], b"\x7fELF\x02\x01");
                assert_eq!(&bytes[18..20], &[0xb7, 0]);
            }
            let path = root.join(format!("lamp-controls-{release}"));
            write(&path, &bytes);
            fs::set_permissions(&path, fs::Permissions::from_mode(0o555)).unwrap();
            binaries.push(Binary {
                path,
                hash,
                bytes: bytes.len(),
                release,
                source: if actual {
                    SOURCES[index]
                } else {
                    "development-smoke-only"
                },
            });
        }
        Self {
            root,
            data,
            until: Instant::now() + CASE,
            commands: 0,
            binaries,
        }
    }
    fn file(&self, name: &str) -> PathBuf {
        self.data.join(name)
    }
    fn json(&self, name: &str, value: &Value) {
        write(&self.file(name), &serde_json::to_vec_pretty(value).unwrap());
    }
    fn data_image(&self) -> BTreeMap<String, Image> {
        let entries: Vec<_> = fs::read_dir(&self.data)
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(entries.len() <= 32);
        entries
            .into_iter()
            .map(|entry| {
                (
                    entry.file_name().into_string().unwrap(),
                    image(&entry.path()),
                )
            })
            .collect()
    }
    fn call(
        &mut self,
        which: usize,
        name: &str,
        args: Vec<String>,
        success: bool,
        created: Option<&str>,
    ) -> Vec<u8> {
        assert!(self.commands < 18 && Instant::now() < self.until);
        let before = self.data_image();
        let binary = &self.binaries[which];
        assert_eq!(sha(&read(&binary.path, 64 * 1024 * 1024)), binary.hash);
        assert_eq!(fs::metadata(&binary.path).unwrap().mode() & 0o7777, 0o555);
        let stem = self
            .root
            .join("steps")
            .join(format!("{:02}-{name}", self.commands));
        let stdout = stem.with_extension("stdout");
        let stderr = stem.with_extension("stderr");
        let output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&stdout)
            .unwrap();
        let error = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&stderr)
            .unwrap();
        let began = Instant::now();
        let child = Command::new(&binary.path)
            .args(&args)
            .env_clear()
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .stdout(output)
            .stderr(error)
            .process_group(0)
            .spawn()
            .unwrap();
        let mut owned = Owned {
            child,
            cleanup: stem.with_extension("failure-cleanup.json"),
            done: false,
        };
        assert!(owned.group().is_some());
        write(
            &stem.with_extension("started.json"),
            &serde_json::to_vec_pretty(&json!({"name":name,"program":binary.path,
                "release_label":binary.release,"source":binary.source,
                "executable_sha256":binary.hash,"argv":args,"environment":{},
                "pid":owned.child.id(),"step_budget_ms":STEP.as_millis(),
                "case_remaining_ms":self.until.saturating_duration_since(began).as_millis()}))
            .unwrap(),
        );
        let status = owned
            .wait(self.until.min(began + STEP))
            .unwrap_or_else(|error| {
                write(
                    &stem.with_extension("wait-error.json"),
                    &serde_json::to_vec_pretty(&json!({"error":error.to_string(),
                    "pid":owned.child.id(),"elapsed_us":began.elapsed().as_micros()}))
                    .unwrap(),
                );
                panic!("{name}: {error}");
            });
        let out = read(&stdout, MAX_FILE);
        let err = read(&stderr, MAX_FILE);
        let receipt = json!({"name":name,"program":binary.path,"release_label":binary.release,"source":binary.source,
            "executable_sha256":binary.hash,"argv":args,"environment":{},"pid":owned.child.id(),
            "exit":status.code(),"reaped":true,"group_absent":true,"elapsed_us":began.elapsed().as_micros(),
            "stdout_bytes":out.len(),"stdout_sha256":sha(&out),"stderr_bytes":err.len(),"stderr_sha256":sha(&err)});
        write(
            &stem.with_extension("json"),
            &serde_json::to_vec_pretty(&receipt).unwrap(),
        );
        self.commands += 1;
        assert_eq!(
            status.code(),
            Some(if success { 0 } else { 1 }),
            "{name}: {}",
            String::from_utf8_lossy(&err)
        );
        assert!(
            !String::from_utf8_lossy(&err).contains(CANARY),
            "synthetic private value leaked"
        );
        let mut after = self.data_image();
        if let Some(name) = created {
            assert!(!before.contains_key(name));
            let made = after
                .remove(name)
                .expect("actual writer created its pins file");
            assert_eq!(made.mode, 0o644);
        }
        assert_eq!(
            before, after,
            "{name}: input/store bytes, modes or identities changed"
        );
        out
    }
    fn calibrate(
        &mut self,
        which: usize,
        name: &str,
        input: &str,
        output: &str,
        create: bool,
    ) -> Value {
        let args = vec![
            "calibrate-touch".into(),
            "--pins".into(),
            self.file(input).display().to_string(),
            "--output".into(),
            self.file(output).display().to_string(),
            "--replay".into(),
            self.file("session.json").display().to_string(),
        ];
        let out = self.call(which, name, args, create, create.then_some(output));
        if create {
            serde_json::from_slice(&out).unwrap()
        } else {
            assert!(out.is_empty());
            Value::Null
        }
    }
    fn replay(&mut self, which: usize, name: &str, pins: &str, good: bool) -> Vec<u8> {
        let args = vec![
            "replay".into(),
            "--touch".into(),
            "--pins".into(),
            self.file(pins).display().to_string(),
            "--config".into(),
            self.file("controls.json").display().to_string(),
            "--clock-id".into(),
            "pair".into(),
            "--input".into(),
            self.file("replay.jsonl").display().to_string(),
            "--until-us".into(),
            "920000".into(),
            "--enable".into(),
        ];
        let out = self.call(which, name, args, good, None);
        if good {
            check_frames(&out);
        } else {
            assert!(out.is_empty());
        }
        out
    }
}

fn signals(electrode: Option<usize>, delta: u16) -> Value {
    let mut filtered = vec![512_u16; 12];
    if let Some(e) = electrode {
        filtered[e] -= delta;
    }
    json!({"touched":electrode.map_or(0, |e| 1_u16<<e),"filtered":filtered,"baseline":vec![512_u16;12]})
}
fn check_report(report: &Value) {
    let pads = report["pads"].as_array().unwrap();
    assert_eq!(pads.len(), 3);
    for (e, (touch, release)) in [(17, 10), (25, 14), (33, 18)].into_iter().enumerate() {
        assert_eq!(pads[e]["electrode"], e);
        assert_eq!(pads[e]["verdict"], "ok");
        assert_eq!(pads[e]["noise"], 3);
        assert_eq!(pads[e]["level"], 40 + 20 * e);
        assert_eq!(pads[e]["touch_threshold"], touch);
        assert_eq!(pads[e]["release_threshold"], release);
    }
}
fn check_frames(bytes: &[u8]) {
    let frames: Vec<Value> = std::str::from_utf8(bytes)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(frames.len(), 47);
    let mut events = Vec::new();
    for (i, frame) in frames.iter().enumerate() {
        assert_eq!(frame["now_us"], i * 20_000);
        let state = &frame["output"]["state"];
        assert_eq!(state["enabled"], true);
        assert_eq!(state["rejected_inputs"], 0);
        events.extend(frame["output"]["events"].as_array().unwrap());
    }
    assert_eq!(events.len(), 2);
    for (event, mask) in events.iter().zip([2, 4]) {
        assert_eq!(event["gesture"], "slide");
        assert_eq!(event["source"], "strip");
        assert_eq!(event["enabled"], true);
        assert_eq!(event["active_mask"], mask);
    }
    let last = &frames.last().unwrap()["output"]["state"]["sources"][0];
    assert_eq!(last["ready"], true);
    assert_eq!(last["fault"], Value::Null);
    assert_eq!(last["active"], false);
    assert_eq!(last["observed_us"], 920_000);
}

pub fn run(actual: bool, reverse: bool, name: &str) {
    let mut case = Case::new(actual);
    let (a, b) = if reverse { (1, 0) } else { (0, 1) };
    let mut seed: Value = serde_json::from_str(include_str!(
        "../../fixtures/strip-711-working-pins-example.json"
    ))
    .unwrap();
    seed["base"]["pads"] = json!([0, 1, 2]);
    case.json("seed.json", &seed);
    case.json(
        "controls.json",
        &json!({"enabled":false,"buttons":[],"head_pads":[],"base_selected_mask":7}),
    );
    let mut swipe = Vec::new();
    for e in 0..3 {
        swipe.extend((0..6).map(|_| signals(Some(e), 40 + 20 * e as u16)));
        swipe.extend((0..3).map(|_| signals(None, 0)));
    }
    case.json(
        "session.json",
        &json!({"every_ms":10,"quiet":vec![signals(None,0);6],"swipe":swipe,
        "charge":{"cdc":seed["base"]["profile"]["cdc"],"cdt":seed["base"]["profile"]["cdt"]}}),
    );
    let mut replay = Vec::new();
    for i in 0..47 {
        let mask = match i {
            5..=12 => 1,
            13..=20 => 2,
            21..=28 => 4,
            _ => 0,
        };
        let row = json!({"now_us":i*20_000,"input":{"kind":"strip","sample":{
            "stamp":{"clock_id":"pair","at_us":i*20_000,"sequence":i+1},"session_id":"pair-strip","mask":mask}}});
        writeln!(replay, "{row}").unwrap();
    }
    write(&case.file("replay.jsonl"), &replay);
    let first = case.calibrate(a, "a-writes", "seed.json", "pins-a.json", true);
    check_report(&first);
    let mut expected = seed.clone();
    for (e, (touch, release)) in [(17, 10), (25, 14), (33, 18)].into_iter().enumerate() {
        expected["base"]["profile"]["touch_thresholds"][e] = json!(touch);
        expected["base"]["profile"]["release_thresholds"][e] = json!(release);
    }
    assert_eq!(
        serde_json::from_slice::<Value>(&read(&case.file("pins-a.json"), MAX_FILE)).unwrap(),
        expected
    );
    let baseline = case.replay(a, "a-reads-own", "pins-a.json", true);
    assert_eq!(case.replay(b, "b-reads-a", "pins-a.json", true), baseline);
    let opposite = case.calibrate(b, "b-writes-a", "pins-a.json", "pins-b.json", true);
    check_report(&opposite);
    assert_eq!(first, opposite);
    assert_eq!(
        read(&case.file("pins-a.json"), MAX_FILE),
        read(&case.file("pins-b.json"), MAX_FILE)
    );
    assert_eq!(case.replay(a, "a-reads-b", "pins-b.json", true), baseline);
    assert_eq!(case.replay(b, "b-reads-own", "pins-b.json", true), baseline);
    case.calibrate(a, "no-overwrite", "pins-b.json", "pins-a.json", false);
    for key in ["future_privacy", "future_reset", "future_api_token"] {
        let mut bad = expected.clone();
        bad[key] = json!(CANARY);
        let file = format!("{key}.json");
        case.json(&file, &bad);
        case.replay(b, key, &file, false);
    }
    write(&case.file("broken.json"), b"{broken");
    case.replay(b, "malformed", "broken.json", false);
    let mut bad = expected.clone();
    bad["base"]["profile"]["esi_ms"] = json!(0);
    case.json("bad-profile.json", &bad);
    case.replay(b, "bad-profile", "bad-profile.json", false);
    let mut extra = expected.clone();
    extra["future_feature"] = json!(CANARY);
    case.json("extra.json", &extra);
    assert_eq!(
        case.replay(b, "benign-forward", "extra.json", true),
        baseline
    );
    assert_eq!(case.replay(b, "b-recovers", "pins-b.json", true), baseline);
    let returned = case.calibrate(
        a,
        "a-writes-return",
        "pins-b.json",
        "pins-return.json",
        true,
    );
    check_report(&returned);
    assert_eq!(first, returned);
    assert_eq!(
        read(&case.file("pins-a.json"), MAX_FILE),
        read(&case.file("pins-return.json"), MAX_FILE)
    );
    assert_eq!(
        case.replay(a, "a-reads-return", "pins-return.json", true),
        baseline
    );
    assert_eq!(
        case.replay(b, "b-reads-return", "pins-return.json", true),
        baseline
    );
    assert_eq!(
        case.replay(a, "a-reopens-return", "pins-return.json", true),
        baseline
    );
    assert_eq!(case.commands, 18);
    assert!(Instant::now() < case.until);
    let artifacts:Vec<_>=case.binaries.iter().map(|binary| {
        assert_eq!(sha(&read(&binary.path,64*1024*1024)),binary.hash);
        json!({"release_label":binary.release,"source":binary.source,"sha256":binary.hash,"bytes":binary.bytes})
    }).collect();
    write(&case.root.join("result.json"),&serde_json::to_vec_pretty(&json!({"test":name,"result":"passed",
        "actual_released_pair":actual,"commands_count":case.commands,"commands_reaped":case.commands,
        "groups_absent":true,"artifacts":artifacts,"identical_released_payloads":actual,
        "hardware_or_device_acceptance":false,"replay_clock":"authored, not measured acquisition"})).unwrap());
}
