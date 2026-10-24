use rusqlite::{Connection, OpenFlags};
use rustix::process::{Pid, Signal, WaitOptions, kill_process, kill_process_group, waitpid};
#[cfg(target_os = "linux")]
use rustix::process::{child_subreaper, set_child_subreaper};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        process::{CommandExt, ExitStatusExt},
    },
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

const MACHINE: &str = "0123456789abcdef0123456789abcdef";
const WAIT: Duration = Duration::from_secs(5);
// The subreaper flag is process-wide. These two ignored tests share this lock;
// no raw fork is used by fixture Rust and no unrelated PID is waited for.
static TURN: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy)]
struct Release {
    number: u32,
    variable: &'static str,
    sha: &'static str,
    bytes: u64,
}
const OLD: Release = Release {
    number: 6408,
    variable: "LAMP_COMPAT_TASKS_6408",
    sha: "7286dde761ab03155d0f867c3c8965759a010c5434e52df7d16756b32bbea003",
    bytes: 4_723_536,
};
const NEW: Release = Release {
    number: 6498,
    variable: "LAMP_COMPAT_TASKS_6498",
    sha: "385d06c433a474ded979fc5953a8e6cc4aec254f37f7ebe32eb86b3bf027a371",
    bytes: 4_728_024,
};
struct Artifact {
    release: Release,
    source: PathBuf,
    path: PathBuf,
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn read(path: &Path, maximum: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    File::open(path)
        .unwrap()
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(bytes.len() as u64 <= maximum, "file exceeds fixture bound");
    bytes
}
fn write(path: &Path, bytes: &[u8]) {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}
fn json_file(path: &Path, value: &Value) {
    write(path, &serde_json::to_vec_pretty(value).unwrap());
}
fn directory(path: &Path) {
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}
impl Artifact {
    fn checked(release: Release, root: &Path) -> Self {
        let path = PathBuf::from(std::env::var_os(release.variable).expect(release.variable));
        assert!(path.is_absolute(), "artifact path must be absolute");
        let artifact = Self {
            release,
            source: path.canonicalize().unwrap(),
            path: root.join(format!("lamp-tasks-{}", release.number)),
        };
        // Downloads normally have no executable bit. Validate every byte
        // first, then make only a unique fixture-owned copy executable.
        artifact.verify(&artifact.source);
        write(&artifact.path, &read(&artifact.source, release.bytes));
        fs::set_permissions(&artifact.path, fs::Permissions::from_mode(0o700)).unwrap();
        artifact.verify(&artifact.path);
        artifact
    }
    fn verify(&self, path: &Path) {
        let metadata = fs::metadata(path).unwrap();
        assert!(metadata.is_file());
        assert_eq!(metadata.len(), self.release.bytes);
        let bytes = read(path, self.release.bytes);
        assert_eq!(hash(&bytes), self.release.sha, "wrong released executable");
        assert_eq!(&bytes[..6], b"\x7fELF\x02\x01");
        assert_eq!(u16::from_le_bytes([bytes[18], bytes[19]]), 183);
    }
}

struct Subreaper {
    #[cfg(target_os = "linux")]
    before: Option<Pid>,
}
impl Subreaper {
    fn enter() -> Self {
        #[cfg(target_os = "linux")]
        {
            let before = child_subreaper().unwrap();
            // PR_SET_CHILD_SUBREAPER takes a boolean, represented by rustix as Pid.
            set_child_subreaper(Pid::from_raw(1)).unwrap();
            Self { before }
        }
        #[cfg(not(target_os = "linux"))]
        panic!("this manual artifact check requires Linux ARM64");
    }
}
impl Drop for Subreaper {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        if let Err(error) = set_child_subreaper(self.before) {
            eprintln!("fixture could not restore subreaper setting: {error}");
        }
    }
}

fn process(pid: Pid) -> Option<(u32, u32, u64)> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    if text.len() > 8192 {
        return None;
    }
    let fields: Vec<_> = text.rsplit_once(") ")?.1.split_whitespace().collect();
    Some((
        fields.get(1)?.parse().ok()?,
        fields.get(2)?.parse().ok()?,
        fields.get(19)?.parse().ok()?,
    ))
}
struct Held {
    pid: Pid,
    began: u64,
    reaped: bool,
}
fn capture_held(marker: &Path, script: &Path, parent: u32) -> Option<Held> {
    let bytes = fs::read(marker).ok()?;
    if bytes.len() > 32 {
        return None;
    }
    let pid = Pid::from_raw(std::str::from_utf8(&bytes).ok()?.trim().parse().ok()?)?;
    let (found_parent, group, began) = process(pid)?;
    if found_parent != parent || group != pid.as_raw_pid() as u32 {
        return None;
    }
    let command = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    if command.len() > 16_384
        || !command
            .split(|byte| *byte == 0)
            .any(|arg| arg == script.as_os_str().as_encoded_bytes())
        || !command
            .split(|byte| *byte == 0)
            .any(|arg| arg == b"Pair blocked")
    {
        return None;
    }
    Some(Held {
        pid,
        began,
        reaped: false,
    })
}
impl Held {
    fn reap(&mut self) -> Value {
        let (parent, group, began) = process(self.pid).expect("held CLI disappeared");
        assert_eq!(parent, std::process::id(), "held CLI was not adopted");
        assert_eq!(group, self.pid.as_raw_pid() as u32);
        assert_eq!(began, self.began, "held CLI identity changed");
        kill_process_group(self.pid, Signal::KILL).unwrap();
        let until = Instant::now() + WAIT;
        loop {
            if let Some((pid, status)) = waitpid(Some(self.pid), WaitOptions::NOHANG).unwrap() {
                self.reaped = true;
                assert_eq!(pid, self.pid);
                assert_eq!(status.terminating_signal(), Some(9));
                return json!({"pid":pid.as_raw_pid(),"starttime":began,"signal":9,"reaped":true});
            }
            assert!(Instant::now() < until, "held CLI did not reap");
            thread::sleep(Duration::from_millis(10));
        }
    }
}
struct Running {
    child: Child,
    held: Option<Held>,
    marker: PathBuf,
    script: PathBuf,
}
impl Running {
    fn wait(&mut self) -> ExitStatus {
        let until = Instant::now() + WAIT;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < until,
                "Tasks did not exit within five seconds"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn stop(&mut self) {
        assert!(self.child.try_wait().unwrap().is_none());
        kill_process(Pid::from_child(&self.child), Signal::TERM).unwrap();
        assert!(self.wait().success(), "Tasks graceful stop failed");
    }
    fn cut(&mut self) -> Value {
        assert!(self.child.try_wait().unwrap().is_none());
        let pid = self.child.id();
        kill_process_group(Pid::from_child(&self.child), Signal::KILL).unwrap();
        let status = self.wait();
        assert_eq!(status.signal(), Some(9));
        let held = self
            .held
            .as_mut()
            .expect("held CLI bound before Tasks cut")
            .reap();
        self.held.take();
        json!({"tasks_pid":pid,"tasks_signal":9,"tasks_reaped":true,"held_cli":held})
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        if self.held.is_none() {
            self.held = capture_held(&self.marker, &self.script, self.child.id());
        }
        if matches!(self.child.try_wait(), Ok(None)) {
            // An error before the held PID was observed still gives the real
            // adapter a chance to cancel/reap its worker before forced cleanup.
            let _ = kill_process(Pid::from_child(&self.child), Signal::TERM);
            let graceful = Instant::now() + Duration::from_secs(1);
            while Instant::now() < graceful && matches!(self.child.try_wait(), Ok(None)) {
                thread::sleep(Duration::from_millis(10));
            }
            if matches!(self.child.try_wait(), Ok(None)) {
                if self.held.is_none() {
                    self.held = capture_held(&self.marker, &self.script, self.child.id());
                }
                let _ = kill_process_group(Pid::from_child(&self.child), Signal::KILL);
            }
            let until = Instant::now() + WAIT;
            while Instant::now() < until {
                if matches!(self.child.try_wait(), Ok(Some(_))) {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            if !matches!(self.child.try_wait(), Ok(Some(_))) {
                eprintln!("fixture cleanup could not confirm Tasks reap");
            }
        }
        if self.held.is_none() {
            self.held = capture_held(&self.marker, &self.script, std::process::id());
        }
        if let Some(held) = self.held.take().filter(|held| !held.reaped) {
            // Best effort only during unwinding; successful cases explicitly
            // verify signal and reap in cut(), rather than depending on Drop.
            if process(held.pid).is_some_and(|(_, group, began)| {
                group == held.pid.as_raw_pid() as u32 && began == held.began
            }) {
                let _ = kill_process_group(held.pid, Signal::KILL);
            }
            let until = Instant::now() + WAIT;
            while Instant::now() < until {
                match waitpid(Some(held.pid), WaitOptions::NOHANG) {
                    Ok(Some(_)) => return,
                    // The graceful adapter may already have reaped its child.
                    Err(rustix::io::Errno::CHILD) => return,
                    _ => {}
                }
                thread::sleep(Duration::from_millis(10));
            }
            eprintln!("fixture cleanup could not confirm held CLI reap");
        }
    }
}

struct Case {
    root: PathBuf,
    home: PathBuf,
    sockets: tempfile::TempDir,
    until: Instant,
    requests: u64,
    engine: &'static str,
    targets: Vec<u8>,
    names: Vec<u8>,
}
impl Case {
    fn new(parent: &Path, engine: &'static str) -> Self {
        let root = parent.join(engine);
        directory(&root);
        let home = root.join("home");
        directory(&home);
        directory(&root.join("store"));
        directory(&root.join("tmp"));
        write(&home.join("engine"), format!("{engine}\n").as_bytes());
        write(&home.join("harness"), include_bytes!("harness.sh"));
        fs::set_permissions(home.join("harness"), fs::Permissions::from_mode(0o700)).unwrap();
        #[cfg(target_os = "linux")]
        rustix::fs::mkfifoat(
            rustix::fs::CWD,
            home.join("release.fifo"),
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        )
        .unwrap();
        let targets = serde_json::to_vec(&json!({"version":1,"targets":{"desk":{
            "machine_id":MACHINE,"project":if engine == "devin-cloud" { "" } else { "/fixture/project" },"engine":engine
        }}})).unwrap();
        let names = br#"{"version":1,"names":{"desk":"Fixture desk"}}"#.to_vec();
        write(&root.join("targets.json"), &targets);
        write(&root.join("targets.names.json"), &names);
        // The released socket binder requires a private parent; tempfile's
        // default directory mode follows the caller's umask.
        let sockets = tempfile::Builder::new()
            .prefix("ltp-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in("/tmp")
            .unwrap();
        Self {
            root,
            home,
            sockets,
            until: Instant::now() + Duration::from_secs(45),
            requests: 0,
            engine,
            targets,
            names,
        }
    }
    fn start(&mut self, artifact: &Artifact, phase: &str) -> Running {
        artifact.verify(&artifact.path);
        assert_eq!(fs::metadata(&artifact.path).unwrap().mode() & 0o7777, 0o700);
        let stdout = self.root.join(format!("{phase}.stdout"));
        let stderr = self.root.join(format!("{phase}.stderr"));
        let socket_parent = fs::symlink_metadata(self.sockets.path()).unwrap();
        assert!(socket_parent.is_dir());
        assert_eq!(socket_parent.mode() & 0o7777, 0o700);
        assert_eq!(socket_parent.uid(), rustix::process::geteuid().as_raw());
        let args: Vec<OsString> = vec![
            "serve".into(),
            "--enable".into(),
            "--dir".into(),
            self.root.join("store").into_os_string(),
            "--socket".into(),
            self.sockets.path().join("public").into_os_string(),
            "--voice-socket".into(),
            self.sockets.path().join("voice").into_os_string(),
            "--targets".into(),
            self.root.join("targets.json").into_os_string(),
            "--harness".into(),
            self.home.join("harness").into_os_string(),
            "--bus-root".into(),
            self.sockets.path().join("bus").into_os_string(),
            "--boot-id".into(),
            "release-pair-boot".into(),
            "--run-for-ms".into(),
            "60000".into(),
            "--no-watch".into(),
            "--set-up-later".into(),
        ];
        json_file(
            &self.root.join(format!("{phase}.command.json")),
            &json!({
                "release":artifact.release.number,"sha256":artifact.release.sha,
                "socket_parent":{"mode":socket_parent.mode() & 0o7777,"uid":socket_parent.uid()},
                "arguments":args.iter().map(|arg| arg.to_str().unwrap()).collect::<Vec<_>>()
            }),
        );
        let child = Command::new(&artifact.path)
            .args(&args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C")
            .env("LC_ALL", "C")
            .env("HOME", &self.home)
            .env("TMPDIR", self.root.join("tmp"))
            .stdin(Stdio::null())
            .stdout(
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(stdout)
                    .unwrap(),
            )
            .stderr(
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(stderr)
                    .unwrap(),
            )
            .process_group(0)
            .spawn()
            .unwrap();
        let mut running = Running {
            child,
            held: None,
            marker: self.home.join("blocked.pid"),
            script: self.home.join("harness"),
        };
        let until = Instant::now() + WAIT;
        loop {
            assert!(
                running.child.try_wait().unwrap().is_none(),
                "Tasks exited before readiness"
            );
            if let Ok(value) = lamp_tasks::runtime::request(
                &self.sockets.path().join("public"),
                &json!({"op":"targets"}),
                Duration::from_millis(200),
            ) && value["targets"]
                == json!([{"alias":"desk","engine":self.engine,"name":"Fixture desk"}])
            {
                break;
            }
            assert!(Instant::now() < until, "Tasks and target names not ready");
            thread::sleep(Duration::from_millis(10));
        }
        artifact.verify(&PathBuf::from(format!("/proc/{}/exe", running.child.id())));
        assert_eq!(
            fs::read_link(format!("/proc/{}/exe", running.child.id())).unwrap(),
            artifact.path
        );
        self.companions();
        running
    }
    fn call(&mut self, trusted: bool, value: Value) -> Value {
        assert!(Instant::now() < self.until, "case exceeded 45 seconds");
        let began = Instant::now();
        let response = lamp_tasks::runtime::request(
            &self
                .sockets
                .path()
                .join(if trusted { "voice" } else { "public" }),
            &value,
            Duration::from_secs(2),
        )
        .unwrap();
        let mut log = OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .open(self.root.join("requests.jsonl"))
            .unwrap();
        writeln!(log, "{}", json!({"trusted_fixture":trusted,"request":value,"response":response,"elapsed_us":began.elapsed().as_micros()})).unwrap();
        response
    }
    fn guard(&mut self) -> Value {
        let status = self.call(false, json!({"op":"status"}));
        assert_eq!(status["ok"], true);
        self.requests += 1;
        json!({"store_id":status["store_id"],"revision":status["revision"],"request_id":format!("pair-{}",self.requests)})
    }
    fn prepare(&mut self, text: &str) -> Value {
        let guard = self.guard();
        self.call(
            false,
            json!({"op":"prepare","guard":guard,"text":text,"target":"desk"}),
        )
    }
    fn confirm(&mut self, prepared: &Value) -> Value {
        assert_eq!(prepared["ok"], true, "{prepared}");
        let guard = self.guard();
        let status = self.call(false, json!({"op":"status"}));
        let request = json!({"op":"confirm","guard":guard,"id":prepared["id"],"prepared_digest":prepared["prepared_digest"],"session_id":status["summary"]["session_id"]});
        assert_eq!(self.call(true, request.clone())["ok"], true);
        request
    }
    fn task(&mut self, id: &Value) -> Value {
        let response = self.call(false, json!({"op":"get_task","id":id}));
        assert_eq!(response["ok"], true, "{response}");
        response
    }
    fn wait_task(&mut self, id: &Value, ready: impl Fn(&Value) -> bool) -> Value {
        let until = Instant::now() + WAIT;
        loop {
            let value = self.task(id);
            if ready(&value) {
                return value;
            }
            assert!(
                Instant::now() < until,
                "Tasks state did not arrive: {value}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn observed(&mut self, text: &str, expected: &str) -> (Value, Value, Value) {
        let prepared = self.prepare(text);
        self.confirm(&prepared);
        let id = &prepared["id"];
        self.wait_task(id, |task| task["task"]["dispatch"]["state"] == "accepted");
        let guard = self.guard();
        assert_eq!(
            self.call(false, json!({"op":"observe","guard":guard,"id":id}))["ok"],
            true
        );
        let task = self.wait_task(id, |task| {
            task["observations"]
                .as_array()
                .is_some_and(|observations| observations.len() == 1)
        });
        let observation = task["observations"][0]["id"].clone();
        let found = self.observation(id, &observation);
        assert_eq!(found["data"]["state"], expected);
        assert_eq!(found["data"]["ask"], text);
        assert_eq!(found["completion"], "unverified");
        (prepared, observation, found["data"].clone())
    }
    fn observation(&mut self, id: &Value, observation: &Value) -> Value {
        self.call(
            false,
            json!({"op":"get_observation","id":id,"observation_id":observation}),
        )
    }
    fn speech(&mut self, id: &Value, observation: &Value, played: bool) {
        let session = self.call(false, json!({"op":"status"}))["summary"]["session_id"].clone();
        let guard = self.guard();
        let claim = self.call(true, json!({"op":"claim_speech","guard":guard,"id":id,"observation_id":observation,"text":"Fixture observation recorded.","session_id":session}));
        assert_eq!(claim["ok"], true, "{claim}");
        if played {
            let guard = self.guard();
            let ack = self.call(true, json!({"op":"ack_speech","guard":guard,"id":id,"observation_id":observation,"claim_id":claim["claim_id"],"text_digest":claim["text_digest"],"session_id":session,"played":true}));
            assert_eq!(ack["reported_spoken"], true, "{ack}");
        }
    }
    fn held(&self, running: &mut Running) {
        let until = Instant::now() + WAIT;
        loop {
            if let Some(held) = capture_held(
                &self.home.join("blocked.pid"),
                &self.home.join("harness"),
                running.child.id(),
            ) {
                running.held = Some(held);
                return;
            }
            assert!(
                Instant::now() < until,
                "fake Harness did not reach held new"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn companions(&self) {
        for (name, expected) in [
            ("targets.json", &self.targets),
            ("targets.names.json", &self.names),
        ] {
            let path = self.root.join(name);
            assert_eq!(read(&path, 16_384), *expected);
            assert_eq!(fs::symlink_metadata(path).unwrap().mode() & 0o7777, 0o600);
        }
        assert!(
            !self.home.join("unexpected").exists(),
            "unexpected fake Harness command"
        );
    }
    fn snapshot(&self, phase: &str, schema: u64) -> Value {
        let path = self.root.join("store/tasks.sqlite3");
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o7777, 0o600);
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .unwrap();
        assert_eq!(
            connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, u64>(0))
                .unwrap(),
            1
        );
        let bytes: Vec<u8> = connection
            .query_row(
                "SELECT snapshot FROM state WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let state: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(state["schema"], schema);
        write(&self.root.join(format!("{phase}.snapshot.json")), &bytes);
        json_file(
            &self.root.join(format!("{phase}.snapshot-sha.json")),
            &json!({"sha256":hash(&bytes),"bytes":bytes.len(),"schema":schema,"sqlite_user_version":1}),
        );
        state
    }
    fn calls(&self) -> Vec<String> {
        String::from_utf8(read(&self.home.join("calls.tsv"), 65_536))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

fn retained(before: &Value, after: &Value) {
    for task in before["tasks"].as_array().unwrap() {
        let kept = after["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|kept| kept["id"] == task["id"])
            .expect("stored task disappeared");
        for field in [
            "id",
            "target_alias",
            "launch",
            "prepared_digest",
            "prepare_request_id",
            "created",
            "expires_monotonic_ms",
        ] {
            assert_eq!(kept[field], task[field], "retained task field {field}");
        }
        assert_eq!(
            kept["observations"].as_array().unwrap().len(),
            task["observations"].as_array().unwrap().len()
        );
        for (old, new) in task["observations"]
            .as_array()
            .unwrap()
            .iter()
            .zip(kept["observations"].as_array().unwrap())
        {
            for field in [
                "id",
                "data",
                "digest",
                "observed",
                "last_seen",
                "seen_count",
                "told",
            ] {
                assert_eq!(new[field], old[field], "retained observation {field}");
            }
        }
    }
}

fn stored<'a>(state: &'a Value, id: &Value) -> &'a Value {
    state["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["id"] == *id)
        .expect("expected task is absent from the durable snapshot")
}

fn scenario(
    parent: &Path,
    writer: &Artifact,
    reader: &Artifact,
    engine: &'static str,
    schema: u64,
) -> Value {
    let began = Instant::now();
    let mut case = Case::new(parent, engine);
    let mut first = case.start(writer, "writer");
    let working = case.observed("Pair working", "working");
    let answered = case.observed("Pair answered", "idle");
    case.speech(&working.0["id"], &working.1, false);
    case.speech(&answered.0["id"], &answered.1, true);
    let blocked = case.prepare("Pair blocked");
    case.confirm(&blocked);
    case.held(&mut first);
    case.wait_task(&blocked["id"], |task| {
        task["task"]["dispatch"]["state"] == "attempting"
    });
    let queued = case.prepare("Pair queued");
    case.confirm(&queued);
    assert_eq!(
        case.task(&queued["id"])["task"]["dispatch"]["state"],
        "queued"
    );
    let prepared = case.prepare("Pair prepared");
    assert_eq!(prepared["ok"], true);
    let old_session = case.call(false, json!({"op":"status"}));
    let stale_confirm = json!({"op":"confirm","guard":case.guard(),"id":prepared["id"],"prepared_digest":prepared["prepared_digest"],"session_id":old_session["summary"]["session_id"]});
    // This is an actual old-binary write, not an edited JSON snapshot: 1,000
    // raw bytes fit its limit and the wire; 6,000 escaped bytes exceed 6498's.
    let escaped = (writer.release.number == 6408 && schema == 1).then(|| {
        let value = case.prepare(&"\u{1}".repeat(1000));
        assert_eq!(value["ok"], true, "{value}");
        value
    });
    let cut = first.cut();
    let original = case.snapshot("writer", schema);
    for (task, expected) in [
        (&working.0, "accepted"),
        (&answered.0, "accepted"),
        (&blocked, "attempting"),
        (&queued, "queued"),
        (&prepared, "prepared"),
    ] {
        assert_eq!(
            stored(&original, &task["id"])["dispatch"]["state"],
            expected
        );
    }
    let original_calls = case.calls();
    assert_eq!(
        original_calls
            .iter()
            .filter(|line| line.starts_with("new\t"))
            .count(),
        3
    );
    assert_eq!(
        original_calls
            .iter()
            .filter(|line| line.starts_with("show\t"))
            .count(),
        2
    );
    let mut second = case.start(reader, "opposite");
    let reopened = case.call(false, json!({"op":"status"}));
    assert_eq!(reopened["store_id"], original["store_id"]);
    assert_ne!(reopened["summary"]["session_id"], original["session_id"]);
    assert!(reopened["revision"].as_u64().unwrap() > original["revision"].as_u64().unwrap());
    assert!(reopened["summary"]["set_aside"].is_null());
    assert_eq!(reopened["summary"]["unread"], 0);
    for (task, state) in [
        (&blocked, "uncertain"),
        (&queued, "invalidated"),
        (&prepared, "invalidated"),
    ] {
        let value = case.task(&task["id"]);
        assert_eq!(value["task"]["dispatch"]["state"], state);
        assert_eq!(value["task"]["dispatch"]["code"], "restart");
    }
    assert_eq!(
        case.call(false, stale_confirm.clone())["error"],
        "untrusted"
    );
    assert_eq!(case.call(true, stale_confirm)["error"], "stale_session");
    for (observed, speech) in [(&working, "uncertain"), (&answered, "spoken")] {
        let value = case.observation(&observed.0["id"], &observed.1);
        assert_eq!(value["data"], observed.2);
        assert_eq!(value["speech"]["state"], speech);
        assert_eq!(value["completion"], "unverified");
        assert_eq!(
            case.task(&observed.0["id"])["task"]["dispatch"]["state"],
            "accepted"
        );
    }
    if let Some(task) = &escaped {
        assert_eq!(case.task(&task["id"])["text"], "\u{1}".repeat(1000));
        assert_eq!(case.prepare(&"\u{1}".repeat(1000))["error"], "bad_text");
    }
    let added = case.prepare("Pair reader write");
    assert_eq!(added["ok"], true);
    let guard = case.guard();
    assert_eq!(
        case.call(false, json!({"op":"cancel","guard":guard,"id":added["id"]}))["ok"],
        true
    );
    second.stop();
    let modified = case.snapshot("opposite", schema);
    retained(&original, &modified);
    assert_eq!(
        modified["tasks"].as_array().unwrap().len(),
        original["tasks"].as_array().unwrap().len() + 1
    );
    let mut third = case.start(writer, "writer-reopen");
    assert_eq!(
        case.task(&added["id"])["task"]["dispatch"]["state"],
        "cancelled"
    );
    assert_eq!(
        case.call(false, json!({"op":"status"}))["store_id"],
        original["store_id"]
    );
    third.stop();
    let final_state = case.snapshot("writer-reopen", schema);
    retained(&modified, &final_state);
    assert_eq!(final_state["tasks"], modified["tasks"]);
    let final_calls = case.calls();
    for command in ["new\t", "show\t", "machines\t"] {
        assert_eq!(
            final_calls
                .iter()
                .filter(|line| line.starts_with(command))
                .count(),
            original_calls
                .iter()
                .filter(|line| line.starts_with(command))
                .count(),
            "restart performed {command}"
        );
    }
    case.companions();
    let receipt = json!({"engine":engine,"schema":schema,"writer":writer.release.number,"opposite":reader.release.number,
        "cut":cut,"accepted_tasks":2,"observations":2,"launch_calls":3,"replayed_launches":0,
        "store_id_preserved":true,"opposite_write_reopened":true,"companions_unchanged":true,
        "old_escaped_prompt_preserved":escaped.is_some(),"completion":"unverified","seconds":began.elapsed().as_secs_f64()});
    json_file(&case.root.join("receipt.json"), &receipt);
    receipt
}

pub(super) fn run(old_first: bool) {
    assert_eq!(
        std::env::consts::OS,
        "linux",
        "manual artifact check requires Linux"
    );
    assert_eq!(
        std::env::consts::ARCH,
        "aarch64",
        "released artifacts require native ARM64"
    );
    let _turn = TURN.lock().unwrap_or_else(|poison| poison.into_inner());
    let _subreaper = Subreaper::enter();
    let (writer_release, reader_release) = if old_first { (OLD, NEW) } else { (NEW, OLD) };
    let output = std::env::var_os("LAMP_COMPAT_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    assert!(output.is_absolute());
    fs::create_dir_all(&output).unwrap();
    let root = tempfile::Builder::new()
        .prefix(&format!(
            "tasks-{}-to-{}-",
            writer_release.number, reader_release.number
        ))
        .tempdir_in(output)
        .unwrap()
        .keep()
        .canonicalize()
        .unwrap();
    eprintln!("Tasks release-pair evidence: {}", root.display());
    let old = Artifact::checked(OLD, &root);
    let new = Artifact::checked(NEW, &root);
    let (writer, reader) = if old_first {
        (&old, &new)
    } else {
        (&new, &old)
    };
    json_file(
        &root.join("artifacts.json"),
        &json!({"writer":{"release":writer.release.number,"sha256":writer.release.sha,"bytes":writer.release.bytes},"opposite":{"release":reader.release.number,"sha256":reader.release.sha,"bytes":reader.release.bytes},"platform":"linux-aarch64","scope":"actual Tasks store and sockets; fake Harness and Voice, no network or audio"}),
    );
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        [("claude", 1), ("devin", 2), ("devin-cloud", 3)]
            .map(|(engine, schema)| scenario(&root, writer, reader, engine, schema))
    }));
    match result {
        Ok(cases) => {
            old.verify(&old.path);
            new.verify(&new.path);
            old.verify(&old.source);
            new.verify(&new.source);
            json_file(
                &root.join("result.json"),
                &json!({"passed":true,"cases":cases,"artifact_bytes_unchanged":true}),
            );
        }
        Err(failure) => {
            let reason = failure
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| failure.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "non-string panic".into());
            json_file(
                &root.join("result.json"),
                &json!({"passed":false,"reason":reason}),
            );
            std::panic::resume_unwind(failure);
        }
    }
}
