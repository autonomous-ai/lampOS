use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::{
        fs::{OpenOptionsExt, PermissionsExt},
        net::UnixStream,
        process::{CommandExt, ExitStatusExt},
    },
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

const MAX: usize = 128 * 1024;
const PAUSE: Duration = Duration::from_millis(3);

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn write(path: &Path, bytes: &[u8]) {
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    out.write_all(bytes).unwrap();
    out.sync_all().unwrap();
}
fn save(path: &Path, value: &Value) {
    write(path, &serde_json::to_vec_pretty(value).unwrap());
}
fn dir(path: &Path) {
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}
fn bounded(path: &Path) -> Vec<u8> {
    let meta = fs::symlink_metadata(path).unwrap();
    assert!(meta.is_file() && meta.len() <= MAX as u64);
    let file = File::open(path).unwrap();
    assert!(file.metadata().unwrap().len() <= MAX as u64);
    let mut bytes = Vec::new();
    file.take(MAX as u64 + 1).read_to_end(&mut bytes).unwrap();
    assert!(bytes.len() <= MAX);
    bytes
}

struct Artifact {
    path: PathBuf,
    original: PathBuf,
    hash: String,
    size: u64,
    mode: u32,
    release: u64,
}
struct Case {
    root: PathBuf,
    deadline: Instant,
    transcript: File,
    records: usize,
    children: usize,
    rpcs: usize,
    artifacts: Vec<Artifact>,
}
impl Case {
    fn new(test: &str, pair: Option<(u64, u64)>) -> Self {
        if pair.is_some() {
            assert_eq!(std::env::consts::OS, "linux");
            assert_eq!(std::env::consts::ARCH, "aarch64");
        }
        let parent = std::env::var_os("LAMP_COMPAT_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        assert!(parent.is_absolute() && parent.is_dir());
        let root = tempfile::Builder::new()
            .prefix("sr-")
            .tempdir_in(parent)
            .unwrap()
            .keep()
            .canonicalize()
            .unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(
            root.as_os_str().as_encoded_bytes().len() < 70,
            "use a short output parent"
        );
        println!("Settings pair evidence: {}", root.display());
        let transcript = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(root.join("transcript.jsonl"))
            .unwrap();
        let mut case = Self {
            root,
            deadline: Instant::now() + Duration::from_secs(90),
            transcript,
            records: 0,
            children: 0,
            rpcs: 0,
            artifacts: Vec::new(),
        };
        save(
            &case.root.join("case.json"),
            &json!({"test":test,"pair":pair,"boundary":"released Settings CLI and Unix-socket desired-state store only","consumers":false,"bus_root":null,"physical_or_power_loss":false}),
        );
        for release in pair.map(|(a, b)| vec![a, b]).unwrap_or_else(|| vec![0]) {
            case.artifact(release);
        }
        dir(&case.root.join("state"));
        case
    }
    fn artifact(&mut self, release: u64) {
        let original = if release == 0 {
            PathBuf::from(env!("CARGO_BIN_EXE_lamp-settings"))
        } else {
            PathBuf::from(
                std::env::var_os(format!("LAMP_COMPAT_SETTINGS_{release}"))
                    .expect("released Settings input required"),
            )
        };
        let bytes = fs::read(&original).unwrap();
        let hash = sha(&bytes);
        let (expected, size, asset, source) = match release {
            6408 => (
                "26079d50d404d1751793519a3179ae86f8a442889a99388e115cfa89f274bd2d",
                1529624,
                668607890,
                "ee53765c8d014eb00e41c1a5a7054d2546c18bdb",
            ),
            6498 => (
                "559410666aae9d415ff072fc9a8682a4f1f5837026e4ee535028f4f21fbc648c",
                1529744,
                670759896,
                "846b3d203fc00ec3886d71f7673d8cc29cc4a634",
            ),
            0 => (hash.as_str(), bytes.len(), 0, "current-test-binary"),
            _ => unreachable!(),
        };
        assert_eq!(bytes.len(), size);
        assert_eq!(hash, expected);
        if release != 0 {
            assert_eq!(&bytes[..6], b"\x7fELF\x02\x01");
            assert_eq!(&bytes[18..20], &[0xb7, 0]);
        }
        let mode = fs::metadata(&original).unwrap().permissions().mode() & 0o7777;
        let path = self.root.join(format!("lamp-settings-{release}"));
        write(&path, &bytes);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o555)).unwrap();
        save(
            &self.root.join(format!("artifact-{release}-before.json")),
            &json!({"release":release,"asset":asset,"source":source,"sha256":hash,"bytes":size,"original_mode":mode,"copy_mode":0o555,"original_read_only":mode&0o222==0}),
        );
        if release != 0 {
            assert_eq!(mode & 0o222, 0, "released inputs must be read-only");
        }
        self.artifacts.push(Artifact {
            path,
            original,
            hash,
            size: size as u64,
            mode,
            release,
        });
    }
    fn binary(&self, r: u64) -> PathBuf {
        self.artifacts
            .iter()
            .find(|a| a.release == r)
            .unwrap()
            .path
            .clone()
    }
    fn record(&mut self, value: Value) {
        assert!(self.records < 256);
        serde_json::to_writer(&mut self.transcript, &value).unwrap();
        self.transcript.write_all(b"\n").unwrap();
        self.transcript.flush().unwrap();
        self.records += 1;
    }
    fn until(&self) -> Instant {
        self.deadline.min(Instant::now() + Duration::from_secs(10))
    }
    fn snapshot(&mut self, name: &str) -> Vec<u8> {
        let bytes = bounded(&self.root.join("state/settings.json"));
        write(&self.root.join(format!("{name}.state.json")), &bytes);
        self.record(json!({"event":"snapshot","name":name,"bytes":bytes.len(),"sha256":sha(&bytes),"mode":fs::metadata(self.root.join("state/settings.json")).unwrap().permissions().mode()&0o7777}));
        bytes
    }
    fn call(&mut self, request: Value) -> Value {
        let until = self.deadline.min(Instant::now() + Duration::from_secs(2));
        self.record(json!({"event":"request","number":self.rpcs+1,"request":request}));
        let reply = exchange(&self.root.join("state/settings.sock"), &request, until).unwrap();
        self.rpcs += 1;
        self.record(json!({"event":"reply","number":self.rpcs,"reply":reply}));
        reply
    }
    fn finish(&mut self, test: &str, pair: bool) {
        assert_eq!(self.children, 7);
        assert_eq!(self.rpcs, 34);
        for a in &self.artifacts {
            for p in [&a.original, &a.path] {
                let b = fs::read(p).unwrap();
                assert_eq!(b.len() as u64, a.size);
                assert_eq!(sha(&b), a.hash);
            }
            assert_eq!(
                fs::metadata(&a.original).unwrap().permissions().mode() & 0o7777,
                a.mode
            );
            assert_eq!(
                fs::metadata(&a.path).unwrap().permissions().mode() & 0o7777,
                0o555
            );
            save(
                &self.root.join(format!("artifact-{}-after.json", a.release)),
                &json!({"release":a.release,"sha256":a.hash,"bytes":a.size,"original_and_copy_unchanged":true,"copy_mode":0o555}),
            );
        }
        assert!(Instant::now() < self.deadline);
        save(
            &self.root.join("result.json"),
            &json!({"test":test,"result":"passed","actual_released_pair":pair,"children_reaped":self.children,"groups_absent":true,"rpc_count":self.rpcs,"artifacts_unchanged":true,"hardware_or_device_acceptance":false}),
        );
        self.transcript.sync_all().unwrap();
    }
}

fn group_gone(id: u32) -> bool {
    matches!(
        rustix::process::test_kill_process_group(
            rustix::process::Pid::from_raw(id as i32).unwrap()
        ),
        Err(rustix::io::Errno::SRCH)
    )
}
struct Owned {
    child: Child,
    root: PathBuf,
    stage: String,
    reaped: bool,
    group_absent: bool,
    began: Instant,
}
impl Owned {
    fn spawn(case: &mut Case, release: u64, stage: &str, args: &[String]) -> Self {
        let began = Instant::now();
        check(case.deadline).unwrap();
        let child = Command::new(case.binary(release))
            .args(args)
            .env_clear()
            .current_dir(&case.root)
            .stdin(Stdio::null())
            .stdout(Stdio::from(
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(case.root.join(format!("{stage}.stdout")))
                    .unwrap(),
            ))
            .stderr(Stdio::from(
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(case.root.join(format!("{stage}.stderr")))
                    .unwrap(),
            ))
            .process_group(0)
            .spawn()
            .unwrap();
        let owned = Self {
            child,
            root: case.root.clone(),
            stage: stage.into(),
            reaped: false,
            group_absent: false,
            began,
        };
        case.record(json!({"event":"spawn","stage":stage,"release":release,"pid":owned.child.id(),"pgid":owned.child.id(),"argv":args,"environment":{}}));
        if std::env::consts::OS == "linux"
            && args.first().is_some_and(|a| a == "serve")
            && !stage.starts_with("future-")
        {
            let running = fs::read(format!("/proc/{}/exe", owned.child.id())).unwrap();
            let artifact = case
                .artifacts
                .iter()
                .find(|a| a.release == release)
                .unwrap();
            assert_eq!(sha(&running), artifact.hash);
            case.record(json!({"event":"running_executable","stage":stage,"sha256":sha(&running),"bytes":running.len()}));
        }
        owned
    }
    fn wait(&mut self, case: &mut Case, until: Instant) -> ExitStatus {
        loop {
            check(until).expect("original child exit deadline");
            if let Some(status) = self.child.try_wait().unwrap() {
                self.reaped = true;
                while !group_gone(self.child.id()) {
                    assert!(Instant::now() < until, "owned group survived reap");
                    std::thread::sleep(PAUSE);
                }
                self.group_absent = true;
                case.children += 1;
                let receipt = json!({"event":"reaped","stage":self.stage,"pid":self.child.id(),"code":status.code(),"signal":status.signal(),"groups_absent":true});
                save(
                    &self.root.join(format!("{}.exit.json", self.stage)),
                    &receipt,
                );
                case.record(receipt);
                check(until).expect("child exited after original deadline");
                return status;
            }
            assert!(Instant::now() < until, "child exit deadline");
            std::thread::sleep(PAUSE);
        }
    }
    fn kill(&mut self, case: &mut Case) {
        let until = case.until();
        rustix::process::kill_process_group(
            rustix::process::Pid::from_raw(self.child.id() as i32).unwrap(),
            rustix::process::Signal::KILL,
        )
        .unwrap();
        assert_eq!(self.wait(case, until).signal(), Some(9));
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        if self.reaped && self.group_absent {
            return;
        }
        let pid = rustix::process::Pid::from_raw(self.child.id() as i32).unwrap();
        let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            if !self.reaped
                && let Ok(Some(_)) = self.child.try_wait()
            {
                self.reaped = true;
            }
            if self.reaped && group_gone(self.child.id()) {
                self.group_absent = true;
                break;
            }
            if Instant::now() >= until {
                break;
            }
            std::thread::sleep(PAUSE);
        }
        let _ = fs::write(
            self.root
                .join(format!("{}.failure-cleanup.json", self.stage)),
            json!({"reaped":self.reaped,"groups_absent":self.group_absent}).to_string(),
        );
    }
}

// Test-owned framing only: no current Settings protocol/domain reader participates.
fn exchange(path: &Path, request: &Value, until: Instant) -> io::Result<Value> {
    let fd = rustix::net::socket(
        rustix::net::AddressFamily::UNIX,
        rustix::net::SocketType::STREAM,
        None,
    )?;
    rustix::io::fcntl_setfd(&fd, rustix::io::FdFlags::CLOEXEC)?;
    let mut stream = UnixStream::from(fd);
    stream.set_nonblocking(true)?;
    let address = rustix::net::SocketAddrUnix::new(path)?;
    match rustix::net::connect(&stream, &address) {
        Ok(()) => {}
        Err(e) if e == rustix::io::Errno::INPROGRESS => {
            while stream.peer_addr().is_err() {
                check(until)?;
                if let Some(e) = stream.take_error()? {
                    return Err(e);
                }
                std::thread::sleep(PAUSE);
            }
        }
        Err(e) => return Err(e.into()),
    }
    let mut bytes = serde_json::to_vec(request)?;
    assert!(bytes.len() < 8192);
    bytes.push(b'\n');
    let mut offset = 0;
    while offset < bytes.len() {
        check(until)?;
        match stream.write(&bytes[offset..]) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(n) => offset += n,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                std::thread::sleep(PAUSE)
            }
            Err(e) => return Err(e),
        }
    }
    read_reply(&mut stream, until)
}
fn check(until: Instant) -> io::Result<()> {
    if Instant::now() >= until {
        Err(io::ErrorKind::TimedOut.into())
    } else {
        Ok(())
    }
}
fn read_reply(stream: &mut UnixStream, until: Instant) -> io::Result<Value> {
    let mut bytes = Vec::new();
    let mut part = [0u8; 1024];
    loop {
        check(until)?;
        match stream.read(&mut part) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => {
                let end = part[..n].iter().position(|b| *b == b'\n');
                let count = end.unwrap_or(n);
                if count > MAX.saturating_sub(bytes.len()) {
                    return Err(io::ErrorKind::InvalidData.into());
                }
                bytes.extend_from_slice(&part[..count]);
                if let Some(end) = end {
                    if part[end + 1..n].iter().any(|b| !b.is_ascii_whitespace()) {
                        return Err(io::ErrorKind::InvalidData.into());
                    }
                    let value: Value = serde_json::from_slice(&bytes)?;
                    check(until)?;
                    if !value.is_object() {
                        return Err(io::ErrorKind::InvalidData.into());
                    }
                    return Ok(value);
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                std::thread::sleep(PAUSE)
            }
            Err(e) => return Err(e),
        }
    }
}
fn catalog(case: &mut Case, r: u64, stage: &str) -> Value {
    let mut child = Owned::spawn(case, r, stage, &["catalog".into()]);
    let until = case.deadline.min(child.began + Duration::from_secs(10));
    assert!(child.wait(case, until).success());
    serde_json::from_slice(&bounded(&case.root.join(format!("{stage}.stdout")))).unwrap()
}
fn serve(case: &mut Case, r: u64, stage: &str, state: &Path) -> Owned {
    let args = vec![
        "serve".into(),
        "--enable".into(),
        "--dir".into(),
        state.to_str().unwrap().into(),
        "--catalog".into(),
        case.root.join("catalog.json").to_str().unwrap().into(),
        "--seconds".into(),
        "60".into(),
    ];
    Owned::spawn(case, r, stage, &args)
}
fn ready(case: &mut Case, child: &mut Owned) {
    let until = case.deadline.min(child.began + Duration::from_secs(10));
    loop {
        assert!(
            child.child.try_wait().unwrap().is_none(),
            "server stopped before readiness"
        );
        check(until).unwrap();
        if let Ok(reply) = exchange(
            &case.root.join("state/settings.sock"),
            &json!({"op":"status"}),
            until.min(Instant::now() + Duration::from_millis(100)),
        ) {
            case.rpcs += 1;
            case.record(json!({"event":"readiness","stage":child.stage,"reply":reply}));
            assert_eq!(reply["ok"], true);
            assert_eq!(reply["ready"], true);
            assert_eq!(reply["reliable"], true);
            break;
        }
        std::thread::sleep(PAUSE);
    }
}
fn mutation(state: &Value, id: &str, level: i64) -> Value {
    json!({"op":"set","mutation":{"guard":{"store_id":state["store_id"],"revision":state["revision"],"request_id":id},"changes":[{"name":"light.level","value":{"kind":"integer","value":level}}]}})
}
fn read(case: &mut Case, catalog: &Value, id: Option<&Value>, revision: u64, level: i64) -> Value {
    let reply = case.call(json!({"op":"read"}));
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["reliable"], true);
    assert_eq!(reply["pending"], false);
    let state = &reply["state"];
    assert_eq!(state["version"], 1);
    assert_eq!(state["revision"], revision);
    assert_eq!(&state["catalog"], catalog);
    assert_eq!(state["store_id"].as_str().unwrap().len(), 32);
    if let Some(id) = id {
        assert_eq!(&state["store_id"], id);
    }
    let expected:Vec<_>=catalog["declarations"].as_array().unwrap().iter().map(|d|json!({"name":d["name"],"value":if d["name"]=="light.level" {json!({"kind":"integer","value":level})}else{d["default"].clone()}})).collect();
    assert_eq!(state["values"], json!(expected));
    state.clone()
}
fn commit(case: &mut Case, request: &Value, id: &Value, revision: u64, replayed: bool) {
    assert_eq!(
        case.call(request.clone()),
        json!({"ok":true,"application":"desired_only","commit":{"store_id":id,"revision":revision,"replayed":replayed}})
    );
}
fn refusals(case: &mut Case, state: &Value, original: &Value, stage: &str) {
    let before = case.snapshot(&format!("{stage}-before-refusals"));
    let mut stale = mutation(state, &format!("{stage}-stale"), 4);
    stale["mutation"]["guard"]["revision"] = json!(0);
    let mut wrong = mutation(state, &format!("{stage}-wrong"), 4);
    wrong["mutation"]["guard"]["store_id"] = json!("00000000000000000000000000000000");
    let mut reused = original.clone();
    reused["mutation"]["changes"][0]["value"]["value"] = json!(5);
    let mut unknown = mutation(state, &format!("{stage}-unknown"), 4);
    unknown["mutation"]["changes"][0]["value"] = json!({"kind":"future","value":4});
    let mut invalid = mutation(state, &format!("{stage}-invalid"), 4);
    invalid["mutation"]["changes"][0]["value"]["value"] = json!(99);
    for (request, error) in [
        (stale, "conflict"),
        (wrong, "conflict"),
        (reused, "conflict"),
        (json!({"op":"future"}), "bad_request"),
        (unknown, "bad_request"),
        (invalid, "invalid"),
    ] {
        let reply = case.call(request);
        assert_eq!(reply["ok"], false);
        assert_eq!(reply["error"], error);
        assert_eq!(
            bounded(&case.root.join("state/settings.json")),
            before,
            "refusal changed saved bytes"
        );
    }
    assert_eq!(case.snapshot(&format!("{stage}-after-refusals")), before);
}
fn stored_refusal(case: &mut Case, r: u64, source: &[u8], version: bool) {
    let stage = if version {
        "future-version"
    } else {
        "future-field"
    };
    let path = case.root.join(stage);
    dir(&path);
    let mut state: Value = serde_json::from_slice(source).unwrap();
    if version {
        state["snapshot"]["version"] = json!(2);
    } else {
        state["future_field"] = json!(true);
    }
    let bytes = serde_json::to_vec(&state).unwrap();
    write(&path.join("settings.json"), &bytes);
    let mut child = serve(case, r, stage, &path);
    let until = case.deadline.min(child.began + Duration::from_secs(10));
    assert_eq!(child.wait(case, until).code(), Some(2));
    assert!(
        String::from_utf8(bounded(&case.root.join(format!("{stage}.stderr"))))
            .unwrap()
            .contains("invalid_state")
    );
    assert!(!path.join("settings.sock").exists());
    assert_eq!(bounded(&path.join("settings.json")), bytes);
    case.record(json!({"event":"strict_stored_refusal","stage":stage,"sha256":sha(&bytes),"preserved":true}));
}

pub fn run(test: &str, pair: Option<(u64, u64)>) {
    let mut case = Case::new(test, pair);
    let (a, b) = pair.unwrap_or((0, 0));
    let catalog_a = catalog(&mut case, a, "catalog-a");
    let catalog_b = catalog(&mut case, b, "catalog-b");
    assert_eq!(catalog_a, catalog_b);
    assert_eq!(catalog_a["version"], 1);
    assert_eq!(catalog_a["declarations"].as_array().unwrap().len(), 3);
    save(&case.root.join("catalog.json"), &catalog_a);
    let path = case.root.join("state");
    let mut first = serve(&mut case, a, "a-writes", &path);
    ready(&mut case, &mut first);
    let initial = read(&mut case, &catalog_a, None, 0, 1);
    let id = initial["store_id"].clone();
    let first_request = mutation(&initial, "first-commit", 2);
    commit(&mut case, &first_request, &id, 1, false);
    let one = read(&mut case, &catalog_a, Some(&id), 1, 2);
    commit(&mut case, &first_request, &id, 1, true);
    refusals(&mut case, &one, &first_request, "a");
    first.kill(&mut case);
    let saved_a = case.snapshot("after-a");
    let mut second = serve(&mut case, b, "b-reads-updates", &path);
    ready(&mut case, &mut second);
    let one = read(&mut case, &catalog_a, Some(&id), 1, 2);
    assert_eq!(case.snapshot("b-reopened"), saved_a);
    commit(&mut case, &first_request, &id, 1, true);
    assert_eq!(case.snapshot("b-retried-a"), saved_a);
    let second_request = mutation(&one, "second-commit", 3);
    commit(&mut case, &second_request, &id, 2, false);
    let two = read(&mut case, &catalog_a, Some(&id), 2, 3);
    refusals(&mut case, &two, &first_request, "b");
    second.kill(&mut case);
    let saved_b = case.snapshot("after-b");
    let mut returned = serve(&mut case, a, "a-returns", &path);
    ready(&mut case, &mut returned);
    let two = read(&mut case, &catalog_a, Some(&id), 2, 3);
    assert_eq!(case.snapshot("a-return-reopened"), saved_b);
    commit(&mut case, &first_request, &id, 1, true);
    commit(&mut case, &second_request, &id, 2, true);
    assert_eq!(case.snapshot("a-return-retried-both"), saved_b);
    refusals(&mut case, &two, &first_request, "return");
    let third_request = mutation(&two, "return-commit", 4);
    commit(&mut case, &third_request, &id, 3, false);
    read(&mut case, &catalog_a, Some(&id), 3, 4);
    returned.kill(&mut case);
    let final_bytes = case.snapshot("after-return");
    let state: Value = serde_json::from_slice(&final_bytes).unwrap();
    let receipts = state["receipts"].as_array().unwrap();
    assert_eq!(receipts.len(), 3);
    for (index, receipt) in receipts.iter().enumerate() {
        assert_eq!(receipt["store_id"], id);
        assert_eq!(receipt["previous_revision"], index as u64);
        assert_eq!(receipt["revision"], index as u64 + 1);
        assert_eq!(receipt["digest"].as_str().unwrap().len(), 64);
    }
    assert_eq!(
        receipts
            .iter()
            .map(|r| r["request_id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["first-commit", "second-commit", "return-commit"]
    );
    stored_refusal(&mut case, b, &final_bytes, false);
    stored_refusal(&mut case, b, &final_bytes, true);
    case.finish(test, pair.is_some());
}

#[test]
fn settings_fixture_complete_reply_within_original_deadline() {
    let (mut reader, mut writer) = UnixStream::pair().unwrap();
    reader.set_nonblocking(true).unwrap();
    writer.write_all(b"{\"ok\":true}\n").unwrap();
    assert_eq!(
        read_reply(&mut reader, Instant::now() + Duration::from_secs(1)).unwrap(),
        json!({"ok":true})
    );
}
#[test]
fn settings_fixture_dribbling_reply_cannot_renew_original_deadline() {
    let (mut reader, mut writer) = UnixStream::pair().unwrap();
    reader.set_nonblocking(true).unwrap();
    let task = std::thread::spawn(move || {
        for _ in 0..100 {
            if writer.write_all(b" ").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    });
    let error = read_reply(&mut reader, Instant::now() + Duration::from_millis(40)).unwrap_err();
    drop(reader);
    task.join().unwrap();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
}
