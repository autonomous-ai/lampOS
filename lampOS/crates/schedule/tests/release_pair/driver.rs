use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

const SECOND: Duration = Duration::from_secs(1);
const BOUND: Duration = Duration::from_secs(10);
const MAX_REPLY: u64 = 8193;
const RELEASE_SHA: &str = "10ff8a223ad58da2afba2fa798bf9b07199c6af05fa0ffc678b8d714e49b7570";
const RELEASE_BYTES: u64 = 4_493_144;
const BOOT_A: &str = "release-pair-boot-a";
const BOOT_B: &str = "release-pair-boot-b";

struct Owned(Child);
impl Owned {
    fn finish(&mut self, signal: rustix::process::Signal) -> ExitStatus {
        if let Some(status) = self.0.try_wait().unwrap() {
            return status;
        }
        let pid = rustix::process::Pid::from_raw(self.0.id().try_into().unwrap()).unwrap();
        rustix::process::kill_process(pid, signal).unwrap();
        let until = Instant::now() + BOUND;
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < until,
                "child did not terminate within bound"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(Some(_))) {
            return;
        }
        let _ = self.0.kill();
        let until = Instant::now() + BOUND;
        loop {
            match self.0.try_wait() {
                Ok(Some(_)) => return,
                Err(_) => return,
                _ if Instant::now() >= until => {
                    eprintln!("release-pair child reap deadline exceeded");
                    return;
                }
                _ => std::thread::sleep(Duration::from_millis(10)),
            }
        }
    }
}

fn hash(path: &Path, logs: &Path, name: &str) -> String {
    let stdout = logs.join(format!("{name}.sha.stdout"));
    let stderr = logs.join(format!("{name}.sha.stderr"));
    let mut child = Owned(
        Command::new("sha256sum")
            .arg("--")
            .arg(path)
            .stdin(Stdio::null())
            .stdout(File::create(&stdout).unwrap())
            .stderr(File::create(stderr).unwrap())
            .spawn()
            .expect("native fixture requires sha256sum"),
    );
    let until = Instant::now() + BOUND;
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < until, "sha256sum exceeded bound");
        std::thread::sleep(Duration::from_millis(10));
    }
    let text = fs::read_to_string(stdout).unwrap();
    let digest = text.split_whitespace().next().unwrap();
    assert!(digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()));
    digest.into()
}

struct Fixture {
    root: PathBuf,
    binaries: Vec<(String, PathBuf, String)>,
    transcript: File,
    stage: usize,
    began: Instant,
}
impl Fixture {
    fn new(first: &str, second: &str, changed: bool, last: &str) -> Self {
        let base = std::env::var_os("LAMP_COMPAT_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        assert!(
            base.is_absolute() && base.is_dir(),
            "evidence parent must already exist"
        );
        let folder = tempfile::Builder::new()
            .prefix("sr-")
            .tempdir_in(&base)
            .unwrap();
        let root = folder.keep();
        assert!(
            root.join("run/c.sock").as_os_str().len() <= 103,
            "use a shorter evidence parent for Unix sockets"
        );
        // Synthetic evidence is retained on both success and failure, never
        // credentials or a person’s reminder text. No implicit external path.
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        fs::create_dir(root.join("run")).unwrap();
        fs::set_permissions(root.join("run"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(root.join("artifact-metadata.json"), serde_json::to_vec_pretty(&json!({
            "6408":{"sha256":RELEASE_SHA,"bytes":RELEASE_BYTES},
            "6498":{"sha256":RELEASE_SHA,"bytes":RELEASE_BYTES},
            "scope":"published Schedule payload is byte-identical; this pair is not a Schedule code-delta qualification"
        })).unwrap()).unwrap();
        let mut binaries = Vec::new();
        for release in ["6408", "6498"] {
            let input = std::env::var_os(format!("LAMP_COMPAT_SCHEDULE_{release}"))
                .expect("supply both released Schedule binary paths");
            let expected = RELEASE_SHA;
            let size = fs::metadata(&input).unwrap();
            assert!(size.is_file());
            assert_eq!(size.len(), RELEASE_BYTES);
            let bytes = fs::read(input).unwrap();
            assert!(bytes.starts_with(b"\x7fELF\x02\x01"));
            assert_eq!(
                bytes.get(18..20),
                Some(&[0xb7, 0][..]),
                "artifact must be AArch64 ELF"
            );
            let path = root.join(format!("lamp-schedule-{release}"));
            fs::write(&path, bytes).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            let digest = hash(&path, &root, release);
            assert_eq!(
                digest, expected,
                "artifact mismatch before any released executable runs"
            );
            binaries.push((release.into(), path, digest));
        }
        let transcript = File::create(root.join("protocol.jsonl")).unwrap();
        let mut fixture = Self {
            root,
            binaries,
            transcript,
            stage: 0,
            began: Instant::now(),
        };
        fixture.record(json!({"kind":"case","first":first,"second":second,"changed_boot_identity":changed,"last_receipt":last,"evidence":"actual released ARM64 writers; synthetic boot identity/trust; no device reboot or speech"}));
        eprintln!("Schedule release-pair evidence: {}", fixture.root.display());
        fixture
    }
    fn record(&mut self, value: Value) {
        writeln!(self.transcript, "{value}").unwrap();
        self.transcript.flush().unwrap();
    }
    fn socket(&self) -> PathBuf {
        self.root.join("run/c.sock")
    }
    fn ask(&mut self, request: &Value) -> Value {
        // Bound every exchange, including reads that return no newline.
        let mut socket = UnixStream::connect(self.socket()).unwrap();
        socket.set_read_timeout(Some(SECOND)).unwrap();
        socket.set_write_timeout(Some(SECOND)).unwrap();
        let mut bytes = serde_json::to_vec(request).unwrap();
        assert!(bytes.len() <= 8192);
        bytes.push(b'\n');
        socket.write_all(&bytes).unwrap();
        let mut reply = Vec::new();
        BufReader::new(socket.take(MAX_REPLY))
            .read_until(b'\n', &mut reply)
            .unwrap();
        assert!(reply.ends_with(b"\n") && reply.len() <= 8193);
        let value: Value = serde_json::from_slice(&reply).unwrap();
        self.record(json!({"kind":"exchange","stage":self.stage,"request":request,"reply":value}));
        value
    }
    fn start(&mut self, release: &str, boot: &str, trust: bool) -> Owned {
        self.stage += 1;
        let binary = &self
            .binaries
            .iter()
            .find(|(id, _, _)| id == release)
            .unwrap()
            .1;
        let mut command = Command::new(binary);
        command
            .args(["serve", "--enable", "--dir"])
            .arg(self.root.join("store"))
            .arg("--socket")
            .arg(self.socket())
            .arg("--bus-root")
            .arg(self.root.join("bus"))
            .args([
                "--zone",
                "Etc/UTC",
                "--boot-id",
                boot,
                "--run-for-ms",
                "60000",
            ])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C")
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(File::create(self.root.join(format!("stage-{}.stdout", self.stage))).unwrap())
            .stderr(File::create(self.root.join(format!("stage-{}.stderr", self.stage))).unwrap());
        if trust {
            command.arg("--trust-clock");
        }
        let arguments: Vec<_> = command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect();
        self.record(json!({"kind":"start","stage":self.stage,"release":release,"arguments":arguments,"environment":{"PATH":"/usr/bin:/bin","LANG":"C","LC_ALL":"C"},"boot_id":boot,"trust_clock":trust}));
        let mut child = Owned(command.spawn().unwrap());
        let until = Instant::now() + BOUND;
        loop {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "released Schedule exited during startup"
            );
            if UnixStream::connect(self.socket()).is_ok() {
                let status = self.ask(&json!({"op":"status"}));
                assert_eq!(status["ok"], true);
                assert_eq!(
                    status["wall_trusted"], trust,
                    "untrusted phase requires an isolated container without timesync marker"
                );
                break;
            }
            assert!(
                Instant::now() < until,
                "released Schedule did not become ready"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        child
    }
    fn stop(&mut self, child: &mut Owned) {
        let status = child.finish(rustix::process::Signal::TERM);
        self.record(json!({"kind":"exit","stage":self.stage,"code":status.code(),"success":status.success()}));
        assert!(status.success(), "Schedule must stop cleanly");
        let destination = self.root.join(format!("store-after-{}", self.stage));
        fs::create_dir(&destination).unwrap();
        for entry in fs::read_dir(self.root.join("store")).unwrap() {
            let entry = entry.unwrap();
            let metadata = entry.file_type().unwrap();
            assert!(metadata.is_file(), "unexpected store directory or link");
            let name = entry.file_name();
            assert!(
                !name.to_string_lossy().contains("newer"),
                "no store may be set aside"
            );
            fs::copy(entry.path(), destination.join(name)).unwrap();
        }
        assert!(
            self.began.elapsed() < Duration::from_secs(90),
            "fixture exceeded its 120-second snooze safety margin"
        );
    }
    fn guard(&mut self, request_id: &str) -> Value {
        let status = self.ask(&json!({"op":"status"}));
        assert_eq!(status["ok"], true);
        json!({"store_id":status["store_id"],"revision":status["revision"],"request_id":request_id})
    }
    fn set(&mut self, name: &str, seconds: u64) -> Value {
        let guard = self.guard(name);
        let reply = self.ask(&json!({"op":"set","guard":guard,"text":format!("Synthetic {name}"),"when":{"kind":"after","seconds":seconds}}));
        assert_eq!(reply["ok"], true);
        reply
    }
    fn get(&mut self, id: &Value) -> Value {
        self.ask(&json!({"op":"get","id":id}))
    }
    fn pending(&mut self, id: &Value) -> Value {
        let until = Instant::now() + BOUND;
        loop {
            let reply = self.get(id);
            assert_eq!(reply["ok"], true);
            if reply["item"]["state"] == "pending" {
                return reply;
            }
            assert!(Instant::now() < until, "timer did not become pending");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

pub(crate) fn scenario(first: &str, second: &str, changed: bool) {
    let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    assert_eq!(
        platform, "linux-aarch64",
        "manual actual-release fixture requires native Linux ARM64"
    );
    for last in ["ack", "snooze"] {
        run(first, second, changed, last);
    }
}
fn run(first: &str, second: &str, changed: bool, last: &str) {
    let mut fixture = Fixture::new(first, second, changed, last);
    let mut writer = fixture.start(first, BOOT_A, true);
    let p = fixture.set("pending", 1);
    let s = fixture.set("snoozed", 1);
    let c = fixture.set("completed", 1);
    for item in [&p, &s, &c] {
        fixture.pending(&item["id"]);
    }
    let mut saved_request = Value::Null;
    let mut saved_receipt = Value::Null;
    let mut snooze_due = Value::Null;
    for op in if last == "ack" {
        ["snooze", "ack"]
    } else {
        ["ack", "snooze"]
    } {
        let guard = fixture.guard(op);
        let request = if op == "ack" {
            json!({"op":"ack","guard":guard,"id":c["id"],"delivery":1})
        } else {
            json!({"op":"snooze","guard":guard,"id":s["id"],"delivery":1,"seconds":120})
        };
        let receipt = fixture.ask(&request);
        assert_eq!(receipt["ok"], true);
        if op == "snooze" {
            snooze_due = receipt["due_unix_ms"].clone();
        }
        saved_request = request;
        saved_receipt = receipt;
    }
    let identity = saved_receipt["store_id"].clone();
    fixture.stop(&mut writer);
    let boot = if changed { BOOT_B } else { BOOT_A };
    let mut reader = fixture.start(second, boot, false);
    inspect(
        &mut fixture,
        &p,
        &s,
        &c,
        &identity,
        &snooze_due,
        (changed, 2),
    );
    replay(&mut fixture, &saved_request, &saved_receipt);
    if changed {
        let guard = fixture.guard("must-not-create-untrusted");
        let refused = fixture.ask(&json!({"op":"set","guard":guard,"text":"Synthetic refused","when":{"kind":"after","seconds":120}}));
        assert_eq!(refused["ok"], false);
        assert_eq!(refused["error"], "bad_time");
        // A rejected mutation is not a new durable receipt and must not
        // displace the completed ACK or snooze retry across a release.
        replay(&mut fixture, &saved_request, &saved_receipt);
    }
    fixture.stop(&mut reader);
    let mut writer_b = fixture.start(second, boot, true);
    inspect(&mut fixture, &p, &s, &c, &identity, &snooze_due, (false, 2));
    replay(&mut fixture, &saved_request, &saved_receipt);
    let extra_guard = fixture.guard("opposite-writes");
    let extra_request = json!({"op":"set","guard":extra_guard,"text":"Synthetic opposite writer","when":{"kind":"after","seconds":120}});
    let extra = fixture.ask(&extra_request);
    assert_eq!(extra["ok"], true);
    fixture.stop(&mut writer_b);
    let mut original = fixture.start(first, boot, true);
    inspect(&mut fixture, &p, &s, &c, &identity, &snooze_due, (false, 3));
    let reread = fixture.get(&extra["id"]);
    assert_eq!(reread["ok"], true);
    assert_eq!(reread["item"]["text"], "Synthetic opposite writer");
    assert_eq!(reread["item"]["delivery"], 1);
    assert_eq!(reread["item"]["original_due_unix_ms"], extra["due_unix_ms"]);
    assert_eq!(reread["item"]["state"], "scheduled");
    replay(&mut fixture, &extra_request, &extra);
    fixture.stop(&mut original);
    fixture.record(json!({"kind":"passed","first":first,"second":second,"changed_boot_identity":changed,"last_receipt":last}));
}
fn replay(f: &mut Fixture, request: &Value, receipt: &Value) {
    let before = f.ask(&json!({"op":"status"}));
    assert_eq!(before["ok"], true);
    assert_eq!(
        f.ask(request),
        *receipt,
        "exact receipt must survive reopening"
    );
    let after = f.ask(&json!({"op":"status"}));
    assert_eq!(after["ok"], true);
    assert_eq!(
        after["revision"], before["revision"],
        "receipt replay must not mutate the store"
    );
}
fn inspect(
    f: &mut Fixture,
    p: &Value,
    s: &Value,
    c: &Value,
    identity: &Value,
    snooze_due: &Value,
    view: (bool, u64),
) {
    let (waiting, expected_items) = view;
    let status = f.ask(&json!({"op":"status"}));
    assert_eq!(&status["store_id"], identity);
    assert_eq!(status["items"], expected_items, "no duplicate/revived item");
    let pending = f.get(&p["id"]);
    assert_eq!(pending["ok"], true);
    assert_eq!(pending["item"]["text"], "Synthetic pending");
    assert_eq!(pending["item"]["state"], "pending");
    assert_eq!(pending["item"]["delivery"], 1);
    assert_eq!(pending["item"]["original_due_unix_ms"], p["due_unix_ms"]);
    let snoozed = f.get(&s["id"]);
    assert_eq!(snoozed["ok"], true);
    assert_eq!(snoozed["item"]["text"], "Synthetic snoozed");
    assert_eq!(snoozed["item"]["delivery"], 2);
    assert_eq!(snoozed["item"]["snoozed"], true);
    assert_eq!(&snoozed["item"]["due_unix_ms"], snooze_due);
    assert_eq!(snoozed["item"]["original_due_unix_ms"], s["due_unix_ms"]);
    assert_eq!(
        snoozed["item"]["state"],
        if waiting {
            "waiting_for_clock"
        } else {
            "scheduled"
        }
    );
    assert!(snooze_due.as_i64().unwrap() > s["due_unix_ms"].as_i64().unwrap());
    if waiting {
        assert!(snoozed["item"]["remaining_ms"].is_null());
    } else {
        assert!((1..=120_000).contains(&snoozed["item"]["remaining_ms"].as_u64().unwrap()));
    }
    let complete = f.get(&c["id"]);
    assert_eq!(complete["ok"], false);
    assert_eq!(complete["error"], "not_found");
    let list = f.ask(&json!({"op":"list","limit":8}));
    assert_eq!(list["ok"], true);
    assert!(
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["id"] != c["id"])
    );
}
