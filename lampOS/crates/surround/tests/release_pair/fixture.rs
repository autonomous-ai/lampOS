use rustix::process::{Pid, Signal, kill_process_group, test_kill_process_group};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        process::{CommandExt, ExitStatusExt},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};

const LIMIT: usize = 128 * 1024;
const PIN: &str = "f75b1a57dd2cfc4edcc521b0d8a254ca72c9ba4402de61a6873b3bcafd0ca765";
const SIZE: usize = 13_871_368;
const ORIGIN: &str = "https://fixture.example";
const CODE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SECRET: &str = "synthetic-surround-fixture-credential-never-a-real-provider-key";
static NEXT: AtomicU64 = AtomicU64::new(0);

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn directory(p: &Path) {
    fs::create_dir(p).unwrap();
    fs::set_permissions(p, fs::Permissions::from_mode(0o700)).unwrap();
}
fn write(p: &Path, bytes: &[u8]) {
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(p)
        .unwrap();
    f.write_all(bytes).unwrap();
    f.sync_all().unwrap();
}
fn bytes(p: &Path, max: usize) -> Vec<u8> {
    bounded_bytes(p, max, true)
}
fn bounded_bytes(p: &Path, max: usize, single_link: bool) -> Vec<u8> {
    let m = fs::symlink_metadata(p).unwrap();
    assert!(
        m.is_file() && (!single_link || m.nlink() == 1) && m.len() <= max as u64,
        "bounded plain input: {} (links {}, bytes {}, maximum {}, single-link required {})",
        p.display(),
        m.nlink(),
        m.len(),
        max,
        single_link
    );
    let mut out = Vec::new();
    File::open(p)
        .unwrap()
        .take(max as u64 + 1)
        .read_to_end(&mut out)
        .unwrap();
    assert!(out.len() <= max);
    out
}
fn value(p: &Path) -> Value {
    serde_json::from_slice(&bytes(p, LIMIT)).unwrap()
}
fn json_file(p: &Path, v: &Value) {
    write(p, &serde_json::to_vec_pretty(v).unwrap());
}
fn record(root: &Path, v: Value) {
    let p = root.join("transcript.jsonl");
    assert!(fs::metadata(&p).unwrap().len() < 1024 * 1024);
    let mut f = OpenOptions::new().append(true).open(p).unwrap();
    serde_json::to_writer(&mut f, &v).unwrap();
    writeln!(f).unwrap();
    f.flush().unwrap();
}
fn safe(b: &[u8]) {
    assert!(
        !b.windows(SECRET.len()).any(|w| w == SECRET.as_bytes()),
        "credential escaped its private boundary"
    );
    assert!(
        !b.windows(CODE.len()).any(|w| w == CODE.as_bytes()),
        "operator code escaped its private boundary"
    );
}
fn snapshot(root: &Path) -> BTreeMap<String, Value> {
    fn visit(base: &Path, p: &Path, out: &mut BTreeMap<String, Value>) {
        let m = fs::symlink_metadata(p).unwrap();
        assert!(m.is_dir() || m.is_file());
        assert!(out.len() < 128);
        if m.is_file() {
            let b = bytes(p, LIMIT);
            out.insert(p.strip_prefix(base).unwrap().to_str().unwrap().to_owned(), json!({"bytes":b.len(),"sha256":sha(&b),"mode":m.mode() & 0o7777,"inode":m.ino(),"device":m.dev()}));
        } else {
            for e in fs::read_dir(p).unwrap() {
                visit(base, &e.unwrap().path(), out);
            }
        }
    }
    let mut out = BTreeMap::new();
    visit(root, root, &mut out);
    out
}

struct Artifact {
    release: u64,
    original: PathBuf,
    executable: PathBuf,
    mode: u32,
    pin: String,
    size: usize,
    actual: bool,
    original_links: u64,
}
impl Artifact {
    fn new(root: &Path, release: u64, actual: bool) -> Self {
        let original = if actual {
            PathBuf::from(
                std::env::var_os(format!("LAMP_COMPAT_SURROUND_{release}"))
                    .expect("explicit released artifact"),
            )
        } else {
            PathBuf::from(env!("CARGO_BIN_EXE_lamp-surround"))
        };
        assert!(original.is_absolute());
        // Cargo may hard-link its current executable into target/deps on Linux.
        // Read it only, verify bytes/mode/link count before and after, and
        // execute only a fresh single-link private copy. Released downloads
        // and private saved inputs still require exactly one link.
        let b = bounded_bytes(&original, 64 * 1024 * 1024, actual);
        if actual {
            assert_eq!(b.len(), SIZE);
            assert_eq!(sha(&b), PIN);
            assert_eq!(&b[..6], b"\x7fELF\x02\x01");
            assert_eq!(&b[18..20], &[0xb7, 0]);
        }
        let executable = root.join(format!("surround-{release}"));
        write(&executable, &b);
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o555)).unwrap();
        let original_links = fs::symlink_metadata(&original).unwrap().nlink();
        let a = Self {
            release,
            mode: fs::symlink_metadata(&original).unwrap().mode() & 0o7777,
            original,
            executable,
            pin: sha(&b),
            size: b.len(),
            actual,
            original_links,
        };
        a.verify();
        a
    }
    fn verify(&self) {
        for p in [&self.original, &self.executable] {
            let b = bounded_bytes(p, self.size, self.actual || p != &self.original);
            assert_eq!(b.len(), self.size);
            assert_eq!(sha(&b), self.pin);
        }
        assert_eq!(
            fs::symlink_metadata(&self.original).unwrap().nlink(),
            self.original_links
        );
        assert_eq!(
            fs::symlink_metadata(&self.original).unwrap().mode() & 0o7777,
            self.mode
        );
        assert_eq!(
            fs::symlink_metadata(&self.executable).unwrap().mode() & 0o7777,
            0o555
        );
    }
    fn receipt(&self) -> Value {
        json!({"release":self.release,"sha256":self.pin,"bytes":self.size,"original_mode":self.mode,"copy_mode":0o555,"actual_released_payload":self.actual,"original_links":self.original_links})
    }
}
struct Owned {
    child: Child,
    root: PathBuf,
    stage: String,
    reaped: bool,
    group_absent: bool,
}
impl Owned {
    fn spawn(root: &Path, stage: &str, a: &Artifact, config: &Path) -> Self {
        a.verify();
        let file = |suffix: &str| {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(root.join(format!("{stage}.{suffix}")))
                .unwrap()
        };
        let child = Command::new(&a.executable)
            .args(["serve", "--enable", "--config"])
            .arg(config)
            .args(["--clock-id", "fixture-clock", "--seconds", "60"])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C")
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::from(file("stdout")))
            .stderr(Stdio::from(file("stderr")))
            .process_group(0)
            .spawn()
            .unwrap();
        record(
            root,
            json!({"event":"spawn","stage":stage,"pid":child.id(),"artifact":a.receipt(),"maintenance":"save_provider only","environment_cleared":true}),
        );
        Self {
            child,
            root: root.into(),
            stage: stage.into(),
            reaped: false,
            group_absent: false,
        }
    }
    fn alive(&mut self) {
        assert!(
            self.child.try_wait().unwrap().is_none(),
            "operator exited before exchange"
        );
    }
    async fn finish(&mut self, until: Instant) {
        self.alive();
        kill_process_group(Pid::from_child(&self.child), Signal::TERM).unwrap();
        loop {
            if let Some(s) = self.child.try_wait().unwrap() {
                self.reaped = true;
                let absent = test_kill_process_group(Pid::from_child(&self.child))
                    == Err(rustix::io::Errno::SRCH);
                self.group_absent = absent;
                record(
                    &self.root,
                    json!({"event":"reaped","stage":self.stage,"pid":self.child.id(),"code":s.code(),"signal":s.signal(),"group_absent":absent}),
                );
                assert!(s.success());
                assert!(absent);
                for suffix in ["stdout", "stderr"] {
                    safe(&bytes(
                        &self.root.join(format!("{}.{suffix}", self.stage)),
                        LIMIT,
                    ));
                }
                return;
            }
            assert!(Instant::now() < until, "bounded graceful exit");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        if self.reaped && self.group_absent {
            return;
        }
        let pid = Pid::from_child(&self.child);
        let _ = kill_process_group(pid, Signal::KILL);
        let until = Instant::now() + Duration::from_secs(3);
        let mut status = None;
        while !self.reaped && Instant::now() < until {
            match self.child.try_wait() {
                Ok(Some(s)) => {
                    self.reaped = true;
                    status = Some(s);
                    break;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                Err(_) => break,
            }
        }
        let absent = test_kill_process_group(pid) == Err(rustix::io::Errno::SRCH);
        if let Ok(mut f) = OpenOptions::new()
            .append(true)
            .open(self.root.join("transcript.jsonl"))
        {
            let _ = writeln!(
                f,
                "{}",
                json!({"event":"failure_cleanup","stage":self.stage,"pid":self.child.id(),"reaped":self.reaped,"code":status.and_then(|s|s.code()),"signal":status.and_then(|s|s.signal()),"group_absent":absent})
            );
        }
    }
}

struct Reply {
    status: u16,
    cookie: Option<String>,
    body: Vec<u8>,
}
fn invalid() -> io::Error {
    io::Error::other("invalid bounded HTTP framing")
}
async fn framed(stream: &mut UnixStream) -> io::Result<Reply> {
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        let b = stream.read_u8().await?;
        head.push(b);
        if head.len() > 16384 {
            return Err(invalid());
        }
    }
    let text = std::str::from_utf8(&head).map_err(|_| invalid())?;
    let mut lines = text.split("\r\n");
    let status_line = lines.next().ok_or_else(invalid)?;
    let mut words = status_line.split_whitespace();
    if words.next() != Some("HTTP/1.1") {
        return Err(invalid());
    }
    let status = words
        .next()
        .ok_or_else(invalid)?
        .parse()
        .map_err(|_| invalid())?;
    let mut length = None;
    let mut chunked = false;
    let mut cookie = None;
    for line in lines.filter(|s| !s.is_empty()) {
        let (key, val) = line.split_once(':').ok_or_else(invalid)?;
        let val = val.trim();
        match key.to_ascii_lowercase().as_str() {
            "content-length" => {
                if length.is_some() {
                    return Err(invalid());
                }
                length = Some(val.parse::<usize>().map_err(|_| invalid())?);
            }
            "transfer-encoding" => {
                if chunked || !val.eq_ignore_ascii_case("chunked") {
                    return Err(invalid());
                }
                chunked = true;
            }
            "set-cookie" => {
                if cookie.is_some() {
                    return Err(invalid());
                }
                cookie = Some(val.split(';').next().ok_or_else(invalid)?.to_owned());
            }
            _ => {}
        }
    }
    let mut body = Vec::new();
    if chunked {
        if length.is_some() {
            return Err(invalid());
        }
        loop {
            let mut line = Vec::new();
            while !line.ends_with(b"\r\n") {
                line.push(stream.read_u8().await?);
                if line.len() > 32 {
                    return Err(invalid());
                }
            }
            let n = usize::from_str_radix(
                std::str::from_utf8(&line[..line.len() - 2]).map_err(|_| invalid())?,
                16,
            )
            .map_err(|_| invalid())?;
            if n == 0 {
                if stream.read_u8().await? != b'\r' || stream.read_u8().await? != b'\n' {
                    return Err(invalid());
                }
                break;
            }
            if n > LIMIT - body.len() {
                return Err(invalid());
            }
            let start = body.len();
            body.resize(start + n, 0);
            stream.read_exact(&mut body[start..]).await?;
            if stream.read_u8().await? != b'\r' || stream.read_u8().await? != b'\n' {
                return Err(invalid());
            }
        }
    } else {
        let n = length.ok_or_else(invalid)?;
        if n > LIMIT {
            return Err(invalid());
        }
        body.resize(n, 0);
        stream.read_exact(&mut body).await?;
    }
    Ok(Reply {
        status,
        cookie,
        body,
    })
}
async fn exchange(
    socket: &Path,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    body: &[u8],
    until: Instant,
) -> io::Result<Reply> {
    // One absolute budget covers connect, all partial writes, headers and body.
    tokio::time::timeout_at(tokio::time::Instant::from_std(until),async {
        let mut stream=UnixStream::connect(socket).await?;
        let cookie=cookie.map(|s|format!("Cookie: {s}\r\n")).unwrap_or_default();
        let request=format!("{method} {path} HTTP/1.1\r\nHost: fixture.example\r\nOrigin: {ORIGIN}\r\n{cookie}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len());
        stream.write_all(request.as_bytes()).await?;stream.write_all(body).await?; framed(&mut stream).await
    }).await.map_err(|_|io::Error::new(io::ErrorKind::TimedOut,"original exchange deadline"))?
}

struct Case {
    root: PathBuf,
    config: PathBuf,
    socket: PathBuf,
    store: PathBuf,
    journal: PathBuf,
    deadline: Instant,
    exchanges: usize,
    children: usize,
}
impl Case {
    fn new(name: &str, actual: bool, first: u64, second: u64) -> Self {
        let parent = if actual {
            assert_eq!(std::env::consts::OS, "linux");
            assert_eq!(std::env::consts::ARCH, "aarch64");
            let marker = Path::new("/run/lamp-surround-pair-isolated");
            assert_eq!(
                bytes(marker, 128),
                b"lamp-surround-candidate-pair network-none tmpfs-v1\n"
            );
            let m = fs::symlink_metadata(marker).unwrap();
            assert_eq!(m.uid(), 0);
            assert_eq!(m.mode() & 0o022, 0);
            let interfaces = fs::read_dir("/sys/class/net")
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect::<Vec<_>>();
            assert_eq!(interfaces, [std::ffi::OsString::from("lo")]);
            assert!(
                fs::read_to_string("/proc/net/route")
                    .unwrap()
                    .lines()
                    .skip(1)
                    .all(|l| l.split_whitespace().nth(1) != Some("00000000"))
            );
            PathBuf::from(std::env::var_os("LAMP_COMPAT_OUTPUT").expect("explicit evidence parent"))
        } else {
            fs::canonicalize("/tmp").unwrap()
        };
        assert!(parent.is_absolute());
        assert!(fs::symlink_metadata(&parent).unwrap().is_dir());
        assert_eq!(fs::canonicalize(&parent).unwrap(), parent);
        assert!(parent.starts_with("/tmp") || parent.starts_with("/private/tmp"));
        let root = parent.join(format!(
            "sr-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        directory(&root);
        let settings = root.join("settings");
        directory(&settings);
        let config = settings.join("surround.json");
        let socket = root.join("run/operator.sock");
        assert!(socket.as_os_str().as_encoded_bytes().len() < 103);
        let store = root.join("configuration");
        let state = root.join("operations");
        let mut doc: Value =
            serde_json::from_slice(include_bytes!("../../fixtures/surround.json")).unwrap();
        doc["enabled"] = true.into();
        doc["state_dir"] = state.to_str().unwrap().into();
        doc["configuration_dir"] = store.to_str().unwrap().into();
        doc["socket"] = socket.to_str().unwrap().into();
        doc["operator_origin"] = ORIGIN.into();
        doc["operator_code_file"] = settings.join("operator.key").to_str().unwrap().into();
        for k in ["owner_socket", "release", "network", "bus_root"] {
            assert!(doc[k].is_null());
        }
        write(&settings.join("operator.key"), CODE.as_bytes());
        json_file(&config, &doc);
        write(&root.join("transcript.jsonl"), b"");
        json_file(
            &root.join("case.json"),
            &json!({"test":name,"actual_released_pair":actual,"stages":[first,second,first,second],"synthetic_inputs_only":true,"transport":"private Unix HTTP/1.1","origin":ORIGIN,"operations":["save_provider"],"excluded":["configuration activation","provider or model requests","network, release, reset, probe and startup operations","hardware","power-loss durability","future stored schema compatibility"],"artifacts_identical":actual,"credential_and_cookie_values_omitted":true,"private_payload_paths_not_for_publication":["settings","configuration"]}),
        );
        println!(
            "Surround candidate release-pair evidence: {}",
            root.display()
        );
        Self {
            root,
            config,
            socket,
            store,
            journal: state.join("operations.json"),
            deadline: Instant::now() + Duration::from_secs(90),
            exchanges: 0,
            children: 0,
        }
    }
    fn until(&self) -> Instant {
        self.deadline.min(Instant::now() + Duration::from_secs(3))
    }
    async fn request(
        &mut self,
        stage: &str,
        method: &str,
        path: &str,
        cookie: Option<&str>,
        body: &Value,
    ) -> Reply {
        let b = if method == "GET" {
            Vec::new()
        } else {
            serde_json::to_vec(body).unwrap()
        };
        let r = exchange(&self.socket, method, path, cookie, &b, self.until())
            .await
            .expect("bounded actual HTTP exchange");
        safe(&r.body);
        self.exchanges += 1;
        record(
            &self.root,
            json!({"event":"http_exchange","stage":stage,"method":method,"path":path,"http_status":r.status,"request_bytes":b.len(),"response_bytes":r.body.len(),"response_sha256":sha(&r.body),"cookie_value_retained":false}),
        );
        r
    }
    async fn start(&mut self, stage: &str, a: &Artifact) -> (Owned, String) {
        let mut child = Owned::spawn(&self.root, stage, a, &self.config);
        self.children += 1;
        let until = self.until();
        loop {
            child.alive();
            match exchange(
                &self.socket,
                "POST",
                "/api/login",
                None,
                &serde_json::to_vec(&json!({"secret":CODE})).unwrap(),
                until,
            )
            .await
            {
                Ok(reply) => {
                    safe(&reply.body);
                    assert_eq!(reply.status, 200);
                    self.exchanges += 1;
                    record(
                        &self.root,
                        json!({"event":"http_exchange","stage":stage,"path":"/api/login","http_status":200,"response_bytes":reply.body.len(),"response_sha256":sha(&reply.body),"cookie_value_retained":false}),
                    );
                    let cookie = reply.cookie.expect("actual session cookie");
                    assert!(cookie.starts_with("lamp_session="));
                    assert_eq!(cookie.len(), "lamp_session=".len() + 64);
                    return (child, cookie);
                }
                Err(error) => {
                    assert!(Instant::now() < until, "bounded operator startup: {error}");
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }
    }
    async fn status(&mut self, stage: &str, cookie: &str) -> Value {
        let r = self
            .request(stage, "GET", "/api/status", Some(cookie), &Value::Null)
            .await;
        assert_eq!(r.status, 200);
        let v: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(v["status"], 1);
        assert_eq!(v["data"]["enabled"], true);
        assert!(v["data"]["current_configuration"].is_null());
        assert!(!self.store.join("current.json").exists());
        v["data"].clone()
    }
    async fn readback(&mut self, stage: &str, cookie: &str, ids: &[String]) {
        let before = snapshot(&self.store);
        let journal = bytes(&self.journal, LIMIT);
        let v = self.status(stage, cookie).await;
        assert_eq!(v["operation"]["id"], *ids.last().unwrap());
        assert_eq!(v["operation"]["phase"], "complete");
        assert_eq!(v["candidate_configuration"]["id"], *ids.last().unwrap());
        tokio::time::sleep(Duration::from_millis(30)).await;
        self.status(stage, cookie).await;
        assert_eq!(
            snapshot(&self.store),
            before,
            "restart/readback changed candidate generation"
        );
        assert_eq!(
            bytes(&self.journal, LIMIT),
            journal,
            "restart automatically re-executed operation"
        );
        record(
            &self.root,
            json!({"event":"restart_readback","stage":stage,"operation_ids":ids,"journal_sha256":sha(&journal),"configuration_files":before,"no_reexecution":true}),
        );
    }
    async fn refused_inputs(&mut self, stage: &str, cookie: &str) {
        let mut unknown_field = provider(8);
        unknown_field["document"]["provider"]["future_field"] = true.into();
        let mut unknown_enum = provider(8);
        unknown_enum["document"]["provider"]["authentication"] = "future_authentication".into();
        let inputs = [
            json!({"operation":"future_operation"}),
            unknown_field,
            unknown_enum,
        ];
        let before = snapshot(&self.store);
        let journal = bytes(&self.journal, LIMIT);
        for (i, input) in inputs.iter().enumerate() {
            let r = self
                .request(stage, "POST", "/api/operation", Some(cookie), input)
                .await;
            assert_eq!(r.status, 400);
            let v: Value = serde_json::from_slice(&r.body).unwrap();
            assert_eq!(v["data"], "invalid_input");
            assert_eq!(snapshot(&self.store), before);
            assert_eq!(bytes(&self.journal, LIMIT), journal);
            record(
                &self.root,
                json!({"event":"synthetic_input_refused","stage":stage,"control":i,"request_sha256":sha(&serde_json::to_vec(input).unwrap()),"no_admission_or_store_mutation":true}),
            );
        }
    }

    async fn stored_refusals_and_recovery(&mut self, a: &Artifact, ids: &[String]) {
        let candidate = self.store.join("candidate.json");
        let manifest = self
            .store
            .join("generations")
            .join(ids.last().unwrap())
            .join("manifest.json");
        for (index, path) in [candidate, manifest].into_iter().enumerate() {
            let original = bytes(&path, LIMIT);
            let mut bad: Value = serde_json::from_slice(&original).unwrap();
            if index == 0 {
                bad["future_field"] = true.into();
            } else {
                bad["schema_version"] = 2.into();
            }
            let bad = serde_json::to_vec(&bad).unwrap();
            fs::write(&path, &bad).unwrap();
            let before = snapshot(&self.store);
            let journal = bytes(&self.journal, LIMIT);
            let stage = format!("stored-refusal-{index}");
            let mut child = Owned::spawn(&self.root, &stage, a, &self.config);
            self.children += 1;
            let until = self.until();
            loop {
                if let Some(status) = child.child.try_wait().unwrap() {
                    child.reaped = true;
                    let absent = test_kill_process_group(Pid::from_child(&child.child))
                        == Err(rustix::io::Errno::SRCH);
                    child.group_absent = absent;
                    record(
                        &self.root,
                        json!({"event":"reaped","stage":stage,"pid":child.child.id(),"code":status.code(),"signal":status.signal(),"group_absent":absent,"expected_stored_refusal":true}),
                    );
                    assert_eq!(status.code(), Some(1));
                    assert_eq!(status.signal(), None);
                    assert!(absent);
                    break;
                }
                assert!(Instant::now() < until, "bounded stored-input refusal");
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            for suffix in ["stdout", "stderr"] {
                safe(&bytes(&self.root.join(format!("{stage}.{suffix}")), LIMIT));
            }
            assert_eq!(snapshot(&self.store), before, "bad saved input was changed");
            assert_eq!(bytes(&self.journal, LIMIT), journal);
            assert!(bytes(&path, LIMIT) == bad, "exact bad bytes retained");
            record(
                &self.root,
                json!({"event":"synthetic_stored_refusal","stage":stage,"input_sha256":sha(&bad),"original_sha256":sha(&original),"configuration_files":before,"journal_unchanged":true,"restore":"fixture explicitly restores its own exact original bytes next"}),
            );
            fs::write(&path, &original).unwrap();
            assert!(bytes(&path, LIMIT) == original);
        }
        let before = snapshot(&self.store);
        let journal = bytes(&self.journal, LIMIT);
        let (mut child, cookie) = self.start("healthy-recovery", a).await;
        self.readback("healthy-recovery", &cookie, ids).await;
        assert_eq!(
            snapshot(&self.store),
            before,
            "healthy startup changed generation bytes"
        );
        assert_eq!(
            bytes(&self.journal, LIMIT),
            journal,
            "healthy startup rewrote the completed operation"
        );
        child.finish(self.until()).await;
    }
    async fn save(&mut self, stage: &str, cookie: &str, index: usize, ids: &mut Vec<String>) {
        let previous = if ids.is_empty() {
            None
        } else {
            Some((
                ids.last().unwrap().clone(),
                snapshot(&self.store.join("generations").join(ids.last().unwrap())),
            ))
        };
        let r = self
            .request(
                stage,
                "POST",
                "/api/operation",
                Some(cookie),
                &provider(index),
            )
            .await;
        assert_eq!(r.status, 202);
        let answer: Value = serde_json::from_slice(&r.body).unwrap();
        let id = answer["data"]["id"].as_str().unwrap().to_owned();
        assert_eq!(id.len(), 32);
        assert!(
            id.bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        );
        assert!(!ids.contains(&id));
        ids.push(id.clone());
        let until = self.until();
        loop {
            let v = self.status(stage, cookie).await;
            if v["operation"]["id"] == id && v["operation"]["phase"] == "complete" {
                assert_eq!(v["operation"]["kind"], "save_provider");
                assert!(v["operation"]["error"].is_null());
                assert_eq!(v["candidate_configuration"]["id"], id);
                break;
            }
            assert!(Instant::now() < until, "bounded candidate completion");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let journal = value(&self.journal);
        assert_eq!(journal["schema_version"], 2);
        assert_eq!(journal["admission_generation"], ids.len());
        assert_eq!(journal["used_ids"], json!(ids));
        assert_eq!(journal["operation"]["id"], id);
        assert_eq!(journal["operation"]["phase"], "complete");
        safe(&bytes(&self.journal, LIMIT));
        let candidate = value(&self.store.join("candidate.json"));
        assert_eq!(candidate["id"], id);
        let dir = self.store.join("generations").join(&id);
        let manifest = value(&dir.join("manifest.json"));
        assert_eq!(manifest["schema_version"], 1);
        assert_eq!(manifest["generation"], candidate);
        assert_eq!(manifest["files"].as_object().unwrap().len(), 4);
        for name in [
            "document.json",
            "provider.json",
            "provider.key",
            "instructions.txt",
        ] {
            let b = bytes(&dir.join(name), LIMIT);
            assert_eq!(manifest["files"][name], sha(&b));
            assert_eq!(fs::metadata(dir.join(name)).unwrap().mode() & 0o7777, 0o600);
        }
        assert_eq!(
            fs::metadata(dir.join("manifest.json")).unwrap().mode() & 0o7777,
            0o600
        );
        assert!(
            bytes(&dir.join("provider.key"), LIMIT) == SECRET.as_bytes(),
            "synthetic credential bytes preserved"
        );
        assert_eq!(
            bytes(&dir.join("instructions.txt"), LIMIT),
            format!("Synthetic inert instructions {index}.\n").as_bytes()
        );
        let document = value(&dir.join("document.json"));
        assert_eq!(document["schema_version"], 1);
        assert_eq!(document["provider"]["voice"], voices(index));
        assert_eq!(document["provider"]["model"], "gemini-3.8-live");
        let payload = value(&dir.join("provider.json"));
        assert_eq!(payload, document["provider"]);
        assert_eq!(
            payload["credential_file"],
            dir.join("provider.key").to_str().unwrap()
        );
        assert_eq!(
            payload["instruction_file"],
            dir.join("instructions.txt").to_str().unwrap()
        );
        assert_eq!(
            candidate["document_sha256"],
            sha(&bytes(&dir.join("document.json"), LIMIT))
        );
        if let Some((old, tree)) = previous {
            assert_eq!(
                snapshot(&self.store.join("generations").join(&old)),
                tree,
                "prior candidate generation was mutated"
            );
        }
        if ids.len() == 3 {
            assert!(
                !self.store.join("generations").join(&ids[0]).exists(),
                "unselected superseded first generation is collected by the released implementation"
            );
        }
        assert_eq!(
            fs::read_dir(self.store.join("generations"))
                .unwrap()
                .count(),
            ids.len().min(2)
        );
        record(
            &self.root,
            json!({"event":"candidate_verified","stage":stage,"operation_id":id,"admission_generation":ids.len(),"used_ids":ids,"candidate":candidate,"files":snapshot(&self.store),"credential_sha256":sha(SECRET.as_bytes()),"selected":false,"manifest_and_payload_readback":true}),
        );
    }
}
fn voices(index: usize) -> &'static str {
    match index % 3 {
        0 => "Kore",
        1 => "Aoede",
        _ => "Puck",
    }
}
fn provider(index: usize) -> Value {
    json!({"operation":"save_provider","document":{"schema_version":1,"provider":{"endpoint":"wss://provider.example/live","model":"gemini-3.8-live","voice":voices(index),"credential_file":"/unused/private.key"}},"credential":SECRET,"instructions":format!("Synthetic inert instructions {index}.\n")})
}
async fn run(name: &str, first: u64, second: u64, actual: bool) {
    let mut c = Case::new(name, actual, first, second);
    let a = Artifact::new(&c.root, first, actual);
    let b = if actual {
        Some(Artifact::new(&c.root, second, true))
    } else {
        None
    };
    let b = b.as_ref().unwrap_or(&a);
    json_file(
        &c.root.join("artifacts.json"),
        &json!([a.receipt(), b.receipt()]),
    );
    let config_before = bytes(&c.config, LIMIT);
    let mut ids = Vec::new();
    for (index, artifact) in [&a, b, &a, b].into_iter().enumerate() {
        let before = if ids.is_empty() {
            None
        } else {
            Some((snapshot(&c.store), bytes(&c.journal, LIMIT)))
        };
        let stage = format!("stage-{index}");
        let (mut child, cookie) = c.start(&stage, artifact).await;
        if let Some((tree, journal)) = before {
            c.readback(&stage, &cookie, &ids).await;
            assert_eq!(
                snapshot(&c.store),
                tree,
                "startup changed candidate generation bytes"
            );
            assert_eq!(
                bytes(&c.journal, LIMIT),
                journal,
                "startup rewrote the completed operation"
            );
        }
        if index < 3 {
            c.save(&stage, &cookie, index, &mut ids).await;
        }
        if index == 0 {
            c.refused_inputs(&stage, &cookie).await;
        }
        child.finish(c.until()).await;
        artifact.verify();
        assert_eq!(bytes(&c.config, LIMIT), config_before);
    }
    c.stored_refusals_and_recovery(b, &ids).await;
    a.verify();
    b.verify();
    json_file(
        &c.root.join("result.json"),
        &json!({"test":name,"result":"passed","actual_released_pair":actual,"children_reaped":c.children,"groups_absent":c.children,"exchanges":c.exchanges,"candidate_writes":3,"synthetic_request_refusals":3,"synthetic_stored_refusals":2,"operation_ids":ids,"hardware_or_device_acceptance":false,"no_selected_configuration":true,"no_automatic_reexecution":true,"identical_selected_artifacts":actual}),
    );
}
pub async fn current() {
    run("current_cli_candidate_smoke", 0, 0, false).await;
}
pub async fn released(first: u64, second: u64, name: &str) {
    run(name, first, second, true).await;
}

pub async fn http_control(dribble: bool) {
    let root = tempfile::Builder::new()
        .prefix("sr-http-")
        .tempdir_in(fs::canonicalize("/tmp").unwrap())
        .unwrap();
    let socket = root.path().join("http.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let server = tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            match s.read_u8().await {
                Ok(b) => request.push(b),
                Err(_) => return,
            }
            if request.len() > 4096 {
                return;
            }
        }
        let reply: &[u8] = if dribble {
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}"
        } else {
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n0\r\n\r\n"
        };
        if dribble {
            for b in reply {
                if s.write_all(&[*b]).await.is_err() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(15)).await;
            }
        } else {
            s.write_all(reply).await.unwrap();
        }
    });
    let start = Instant::now();
    let result = exchange(
        &socket,
        "GET",
        "/api/status",
        None,
        b"",
        start + Duration::from_millis(140),
    )
    .await;
    if dribble {
        assert_eq!(
            result.err().expect("dribble must time out").kind(),
            io::ErrorKind::TimedOut
        );
        assert!(start.elapsed() < Duration::from_secs(2));
    } else {
        let r = result.unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.body, b"{}");
    }
    server.abort();
    let _ = server.await;
}
