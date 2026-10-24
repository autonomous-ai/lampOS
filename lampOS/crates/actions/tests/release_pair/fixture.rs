use lamp_bus::Bus;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        net::UnixStream,
        process::{CommandExt, ExitStatusExt},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const PIN: &str = "b5359557611b4fa3aa09709b659c7243f77c9ae945c326937f55adbc928903d6";
const ARTIFACT_BYTES: usize = 4_517_592;
const STEP: Duration = Duration::from_secs(10);
const IO: Duration = Duration::from_secs(2);
const PAUSE: Duration = Duration::from_millis(5);
const MAX_LOG: u64 = 64 * 1024;

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

    fn source(self) -> &'static str {
        match self {
            Self::Old => "ee53765c8d014eb00e41c1a5a7054d2546c18bdb",
            Self::Current => "846b3d203fc00ec3886d71f7673d8cc29cc4a634",
        }
    }

    fn input(self) -> PathBuf {
        let key = format!("LAMP_COMPAT_ACTIONS_{}", self.number());
        let path = PathBuf::from(std::env::var_os(&key).unwrap_or_else(|| panic!("missing {key}")));
        assert!(path.is_absolute(), "absolute released artifact path");
        path
    }

    fn filename(self) -> String {
        format!("lamp-actions-{}", self.number())
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
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .unwrap()
}

fn write(path: &Path, bytes: &[u8]) {
    let mut file = output(path);
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}

fn json_file(path: &Path, value: Value) {
    write(path, &serde_json::to_vec_pretty(&value).unwrap());
}

fn read(path: &Path, most: u64) -> Vec<u8> {
    let metadata = fs::symlink_metadata(path).unwrap();
    assert!(metadata.is_file() && metadata.nlink() == 1);
    assert!(metadata.len() <= most, "bounded retained file");
    let mut bytes = Vec::new();
    File::open(path)
        .unwrap()
        .take(most + 1)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(bytes.len() as u64 <= most);
    bytes
}

fn artifact(path: &Path) -> Vec<u8> {
    let bytes = read(path, ARTIFACT_BYTES as u64);
    assert_eq!(bytes.len(), ARTIFACT_BYTES, "published byte count");
    assert_eq!(sha(&bytes), PIN, "published SHA-256");
    assert_eq!(&bytes[..6], b"\x7fELF\x02\x01", "ELF64 little-endian");
    assert_eq!(&bytes[18..20], &[0xb7, 0], "AArch64 executable");
    bytes
}

fn wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .try_into()
        .unwrap()
}

#[derive(PartialEq, Eq)]
struct Image {
    snapshot: Vec<u8>,
    mode: u32,
    inode: u64,
    device: u64,
}

impl Image {
    fn expect(&self, revision: u64, entries: &[(&Value, &str)]) {
        let actual: Value = serde_json::from_slice(&self.snapshot).unwrap();
        let sources: Vec<_> = entries
            .iter()
            .map(|(request, status)| json!({"request":request,"status":status}))
            .collect();
        assert_eq!(
            actual,
            json!({"schema":1,"revision":revision,"sources":sources}),
            "exact stored JSON, including cancellation tombstones"
        );
    }
}

struct Case {
    root: PathBuf,
    transcript: File,
    entries: usize,
    deadline: Instant,
    oldest_expiry: u64,
    reaped: usize,
}

impl Case {
    fn new(writer: Release, reader: Release, name: &str) -> Self {
        assert_eq!(std::env::consts::OS, "linux", "explicit native fixture");
        assert_eq!(std::env::consts::ARCH, "aarch64", "explicit native fixture");
        let parent = std::env::var_os("LAMP_COMPAT_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        assert!(parent.is_absolute() && parent.is_dir());
        let root = tempfile::Builder::new()
            .prefix("cp-")
            .tempdir_in(parent)
            .unwrap()
            .keep();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(
            root.as_os_str().as_encoded_bytes().len() <= 45,
            "short private socket paths"
        );
        println!("Actions release-pair evidence: {}", root.display());
        directory(&root.join("caps"));
        directory(&root.join("snapshots"));
        json_file(
            &root.join("case.json"),
            json!({
                "test":name,"writer":writer.number(),"reader":reader.number(),"return_reader":writer.number(),
                "writer_source":writer.source(),"reader_source":reader.source(),
                "sha256":PIN,"artifact_bytes":ARTIFACT_BYTES,"identical_release_binaries":true,
                "boundary":"actual released Actions RPC and durable cap storage; no output owner",
                "case_deadline_seconds":90,"step_deadline_seconds":10,"rpc_timeout_seconds":2,
                "excluded":["hardware confirmation","unknown clock","expiry","newer schemas","power loss","signature trust"]
            }),
        );
        for release in [Release::Old, Release::Current] {
            let bytes = artifact(&release.input());
            let path = root.join(release.filename());
            write(&path, &bytes);
            fs::set_permissions(path, fs::Permissions::from_mode(0o555)).unwrap();
        }
        Self {
            transcript: output(&root.join("transcript.jsonl")),
            root,
            entries: 0,
            deadline: Instant::now() + Duration::from_secs(90),
            oldest_expiry: u64::MAX,
            reaped: 0,
        }
    }

    fn check(&self) {
        assert!(Instant::now() < self.deadline, "original case deadline");
        assert!(
            wall_ms() < self.oldest_expiry,
            "expiry is outside this continuity case"
        );
    }

    fn until(&self) -> Instant {
        self.check();
        self.deadline.min(Instant::now() + STEP)
    }

    fn record(&mut self, value: Value) {
        assert!(self.entries < 512, "bounded transcript");
        let bytes = serde_json::to_vec(&value).unwrap();
        assert!(bytes.len() <= 16 * 1024, "bounded transcript row");
        self.transcript.write_all(&bytes).unwrap();
        self.transcript.write_all(b"\n").unwrap();
        self.transcript.flush().unwrap();
        self.entries += 1;
    }

    fn image(&mut self, label: &str) -> Image {
        self.check();
        let store = self.root.join("caps");
        assert_eq!(fs::metadata(&store).unwrap().mode() & 0o7777, 0o700);
        let names: Vec<_> = fs::read_dir(&store)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(
            names,
            ["schedule.sqlite3"],
            "no fallback/archive/journal in settled store"
        );
        let path = store.join("schedule.sqlite3");
        let bytes = read(&path, 64 * 1024);
        assert!(bytes.len() >= 100);
        assert_eq!(&bytes[..16], b"SQLite format 3\0");
        assert_eq!(u32::from_be_bytes(bytes[60..64].try_into().unwrap()), 1);
        let meta = fs::symlink_metadata(&path).unwrap();
        assert_eq!(meta.mode() & 0o7777, 0o600);
        let copy = self.root.join("snapshots").join(format!("{label}.sqlite3"));
        write(&copy, &bytes);
        // Inspect an immutable copy with generic SQLite, never today's cap
        // Controller as a replacement for a released reader. The fixture has
        // completed its RPC and all deadlines remain in the future, so the
        // service has no scheduled state mutation while this copy is taken.
        let db = Connection::open_with_flags(
            &copy,
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .unwrap();
        db.busy_timeout(IO).unwrap();
        let check: String = db
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .unwrap();
        assert_eq!(check, "ok");
        let journal: String = db
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal, "delete");
        let count: u64 = db
            .query_row("SELECT count(*) FROM state", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1, "exact singleton state row");
        let snapshot: Vec<u8> = db
            .query_row(
                "SELECT snapshot FROM state WHERE singleton=1 AND typeof(snapshot)='blob'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!snapshot.is_empty() && snapshot.len() <= 16_384);
        let state: Value = serde_json::from_slice(&snapshot).unwrap();
        assert_eq!(state["schema"], 1);
        assert!(state["revision"].is_u64() && state["sources"].is_array());
        db.close().unwrap();
        assert_eq!(
            read(&copy, 64 * 1024),
            bytes,
            "read-only inspection did not write"
        );
        assert_eq!(
            read(&path, 64 * 1024),
            bytes,
            "settled database stayed consistent during copy"
        );
        write(
            &self.root.join("snapshots").join(format!("{label}.json")),
            &snapshot,
        );
        self.record(json!({"event":"store_snapshot","label":label,"sha256":sha(&bytes),
            "bytes":bytes.len(),"snapshot_sha256":sha(&snapshot),"revision":state["revision"],
            "mode":meta.mode() & 0o7777,"inode":meta.ino(),"device":meta.dev(),"database_version":1}));
        self.check();
        Image {
            snapshot,
            mode: meta.mode() & 0o7777,
            inode: meta.ino(),
            device: meta.dev(),
        }
    }

    fn finish(&mut self, name: &str) {
        self.check();
        assert_eq!(self.reaped, 3);
        let mut artifacts = Vec::new();
        for release in [Release::Old, Release::Current] {
            let original = artifact(&release.input());
            let copy = self.root.join(release.filename());
            assert_eq!(artifact(&copy), original);
            assert_eq!(fs::metadata(&copy).unwrap().mode() & 0o7777, 0o555);
            artifacts.push(json!({"release":release.number(),"source":release.source(),
                "sha256":PIN,"bytes":ARTIFACT_BYTES,"original_and_copy_unchanged":true}));
        }
        self.transcript.sync_all().unwrap();
        json_file(
            &self.root.join("result.json"),
            json!({"test":name,"result":"passed",
            "children_reaped":self.reaped,"artifacts":artifacts,"identical_release_binaries":true,
            "hardware_confirmed":false,"case_deadline_seconds":90}),
        );
    }
}

struct Daemon {
    child: Child,
    stage: String,
    root: PathBuf,
    runtime: PathBuf,
    bus: Bus,
    reaped: bool,
}

impl Daemon {
    fn spawn(case: &mut Case, release: Release, stage: &str) -> Self {
        let runtime = case.root.join(format!("{stage}-run"));
        let bus_root = case.root.join(format!("{stage}-bus"));
        directory(&runtime);
        directory(&bus_root);
        let bus = Bus::new(&bus_root).unwrap();
        let binary = case.root.join(release.filename());
        artifact(&binary);
        let arguments = [
            "--dir".into(),
            runtime.clone(),
            "--bus-root".into(),
            bus_root.clone(),
            "--caps-dir".into(),
            case.root.join("caps"),
        ];
        let child = Command::new(&binary)
            .args(&arguments)
            .env_clear()
            .env("LAMP_OUTPUT_CAPS", "on")
            .current_dir(&case.root)
            .stdin(Stdio::null())
            .stdout(Stdio::from(output(
                &case.root.join(format!("{stage}.stdout")),
            )))
            .stderr(Stdio::from(output(
                &case.root.join(format!("{stage}.stderr")),
            )))
            .process_group(0)
            .spawn()
            .unwrap();
        // All fallible setup is before spawn; own cleanup before /proc or any
        // assertion can fail after it.
        let mut owned = Self {
            child,
            stage: stage.into(),
            root: case.root.clone(),
            runtime,
            bus,
            reaped: false,
        };
        let mut actual = Vec::new();
        File::open(format!("/proc/{}/exe", owned.child.id()))
            .unwrap()
            .take(ARTIFACT_BYTES as u64 + 1)
            .read_to_end(&mut actual)
            .unwrap();
        assert_eq!(actual.len(), ARTIFACT_BYTES);
        assert_eq!(sha(&actual), PIN, "actual running executable bytes");
        case.record(
            json!({"event":"spawn","stage":stage,"release":release.number(),
            "pid":owned.child.id(),"argv":arguments,"environment":{"LAMP_OUTPUT_CAPS":"on"},
            "executable_sha256":sha(&actual),"direct_child":true}),
        );
        let until = case.until();
        while !owned.runtime.join("call.sock").exists() {
            owned.alive(until);
            std::thread::sleep(PAUSE);
        }
        owned.alive(until);
        owned.no_children();
        owned
    }

    fn alive(&mut self, until: Instant) {
        let status = self.child.try_wait().unwrap();
        if let Some(status) = status {
            self.reaped = true;
            json_file(
                &self
                    .root
                    .join(format!("{}.unexpected-exit.json", self.stage)),
                json!({"pid":self.child.id(),"code":status.code(),"signal":status.signal(),"reaped":true}),
            );
        }
        let error = read(&self.root.join(format!("{}.stderr", self.stage)), MAX_LOG);
        let _ = read(&self.root.join(format!("{}.stdout", self.stage)), MAX_LOG);
        assert!(
            status.is_none() && Instant::now() < until,
            "{}: child={status:?}; {}",
            self.stage,
            String::from_utf8_lossy(&error)
        );
    }

    fn no_children(&self) {
        let tasks: Vec<_> = fs::read_dir(format!("/proc/{}/task", self.child.id()))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(tasks.len() <= 64);
        for task in tasks {
            let children = fs::read_to_string(task.path().join("children")).unwrap();
            assert!(
                children.trim().is_empty(),
                "no mixer/helper child may be spawned"
            );
        }
    }

    fn ask(&mut self, case: &mut Case, request: Value) -> Value {
        let until = case.until();
        self.alive(until);
        let began = Instant::now();
        let mut stream = UnixStream::connect(self.runtime.join("call.sock")).unwrap();
        let deadline = until.min(began + IO);
        stream
            .set_write_timeout(Some(remaining(deadline).unwrap()))
            .unwrap();
        let mut bytes = serde_json::to_vec(&request).unwrap();
        assert!(bytes.len() < 8192);
        bytes.push(b'\n');
        stream.write_all(&bytes).unwrap();
        let line = reply_line(&mut stream, deadline).unwrap();
        let reply = serde_json::from_slice(&line).unwrap();
        case.record(json!({"event":"rpc","stage":self.stage,"request":request,
            "reply":reply,"elapsed_us":began.elapsed().as_micros()}));
        self.alive(until);
        case.check();
        reply
    }

    fn get(
        &mut self,
        case: &mut Case,
        revision: u64,
        active: &[Value],
        light: Option<u16>,
        volume: Option<u8>,
    ) -> Value {
        let expected = json!({"revision":revision,"active":active,
            "effective":{"light_channel_max":light,"volume_raw_max":volume}});
        let answer = self.ask(case, json!({"op":"caps_get"}));
        assert_eq!(answer, json!({"ok":true,"snapshot":expected}));
        // This stage has a new empty bus; no preceding process's snapshot can
        // stand in for its publication. Bus identities/time may legitimately differ.
        let until = case.until();
        loop {
            if let Some(message) = self.bus.latest("output/caps").unwrap() {
                let value: Value = message.parse().unwrap();
                if value["revision"] == revision {
                    assert_eq!(message.header.source, "actions");
                    assert_eq!(value["entries"], json!(active));
                    case.record(json!({"event":"published_snapshot","stage":self.stage,
                        "header":message.header,"snapshot":value,"hardware_confirmed":false}));
                    break;
                }
            }
            self.alive(until);
            std::thread::sleep(PAUSE);
        }
        while self.runtime.join("ready").exists() == volume.is_some() {
            self.alive(until);
            std::thread::sleep(PAUSE);
        }
        expected
    }

    fn kill(&mut self, case: &mut Case) {
        let until = case.until();
        self.alive(until);
        self.no_children();
        self.child.kill().unwrap();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                self.reaped = true;
                let receipt = json!({"event":"reaped","stage":self.stage,"pid":self.child.id(),
                    "signal":status.signal(),"code":status.code(),"reaped":true});
                json_file(
                    &self.root.join(format!("{}.exit.json", self.stage)),
                    receipt.clone(),
                );
                case.record(receipt);
                assert_eq!(status.signal(), Some(9), "real SIGKILL and waitpid result");
                case.reaped += 1;
                break;
            }
            assert!(Instant::now() < until, "bounded reap");
            std::thread::sleep(PAUSE);
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        let _ = self.child.kill();
        let until = Instant::now() + Duration::from_secs(5);
        let receipt = loop {
            if let Ok(Some(status)) = self.child.try_wait() {
                break json!({"reaped":true,"pid":self.child.id(),"signal":status.signal(),"code":status.code()});
            }
            if Instant::now() >= until {
                break json!({"reaped":false,"pid":self.child.id()});
            }
            std::thread::sleep(PAUSE);
        };
        let _ = fs::write(
            self.root
                .join(format!("{}.failure-cleanup.json", self.stage)),
            receipt.to_string(),
        );
    }
}

fn fresh(case: &mut Case, source: &str, token: &str, light: u16, volume: u8) -> Value {
    case.check();
    let now = wall_ms();
    let until = now.checked_add(3_600_000).unwrap();
    case.oldest_expiry = case.oldest_expiry.min(until);
    json!({"source":source,"token":token,"issued_unix_ms":now,"valid_until_unix_ms":until,
        "light_channel_max":light,"volume_raw_max":volume})
}

fn replacement(case: &mut Case, previous: &Value, token: &str, light: u16, volume: u8) -> Value {
    let until = case.until();
    while wall_ms() <= previous["issued_unix_ms"].as_u64().unwrap() {
        case.check();
        assert!(Instant::now() < until, "strictly later real clock sample");
        std::thread::sleep(PAUSE);
    }
    fresh(
        case,
        previous["source"].as_str().unwrap(),
        token,
        light,
        volume,
    )
}

fn accepted(reply: &Value, revision: u64, active: bool, light: Option<u16>, volume: Option<u8>) {
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["accepted"], true);
    assert_eq!(reply["active"], active);
    assert_eq!(reply["revision"], revision);
    assert_eq!(
        reply["effective"],
        json!({"light_channel_max":light,"volume_raw_max":volume})
    );
    assert_eq!(reply["volume"]["required"], volume.is_some());
    assert_eq!(reply["volume"]["confirmed"], false, "no mixer configured");
    assert_eq!(reply["ring"], json!({"published":true,"confirmed":false}));
    if volume.is_some() {
        assert_eq!(reply["volume"]["error"], "this lamp has no volume control");
    }
}

fn unchanged(
    case: &mut Case,
    daemon: &mut Daemon,
    label: &str,
    request: Value,
    expected_code: Option<&str>,
) -> Value {
    let before = case.image(&format!("{label}-before"));
    let reply = daemon.ask(case, request);
    if let Some(code) = expected_code {
        assert_eq!(reply["ok"], false);
        assert_eq!(reply["accepted"], false);
        assert_eq!(reply["code"], code);
    }
    let after = case.image(&format!("{label}-after"));
    assert!(
        before == after,
        "{label}: unchanged exact JSON BLOB, mode and identity"
    );
    reply
}

pub fn run(writer: Release, reader: Release, name: &str) {
    let mut case = Case::new(writer, reader, name);
    let mut a = Daemon::spawn(&mut case, writer, "a");
    a.get(&mut case, 0, &[], None, None);
    let primary = fresh(&mut case, "pair-primary", "first", 72, 60);
    accepted(
        &a.ask(&mut case, json!({"op":"caps_put","cap":primary})),
        1,
        true,
        Some(72),
        Some(60),
    );
    let cancelled = fresh(&mut case, "pair-cancelled", "retired", 24, 30);
    accepted(
        &a.ask(&mut case, json!({"op":"caps_put","cap":cancelled})),
        2,
        true,
        Some(24),
        Some(30),
    );
    accepted(
        &a.ask(
            &mut case,
            json!({"op":"caps_remove","source":"pair-cancelled","token":"retired"}),
        ),
        3,
        false,
        Some(72),
        Some(60),
    );
    a.get(
        &mut case,
        3,
        std::slice::from_ref(&primary),
        Some(72),
        Some(60),
    );
    let seeded = case.image("a-committed");
    seeded.expect(3, &[(&primary, "active"), (&cancelled, "cancelled")]);
    a.kill(&mut case);
    assert!(seeded == case.image("a-after-kill"));

    let mut b = Daemon::spawn(&mut case, reader, "b");
    b.get(
        &mut case,
        3,
        std::slice::from_ref(&primary),
        Some(72),
        Some(60),
    );
    assert!(
        seeded == case.image("b-reopened"),
        "opposite reader must not rewrite unexpired state"
    );
    let reply = unchanged(
        &mut case,
        &mut b,
        "b-cancelled-retry",
        json!({"op":"caps_put","cap":cancelled}),
        None,
    );
    accepted(&reply, 3, false, Some(72), Some(60));
    let mut changed = primary.clone();
    changed["light_channel_max"] = json!(64);
    unchanged(
        &mut case,
        &mut b,
        "b-changed-token",
        json!({"op":"caps_put","cap":changed}),
        Some("conflict"),
    );
    let mut stale = primary.clone();
    stale["token"] = json!("older");
    unchanged(
        &mut case,
        &mut b,
        "b-stale",
        json!({"op":"caps_put","cap":stale}),
        Some("stale"),
    );
    unchanged(
        &mut case,
        &mut b,
        "b-wrong-remove",
        json!({"op":"caps_remove","source":"pair-primary","token":"wrong"}),
        Some("token_mismatch"),
    );
    unchanged(
        &mut case,
        &mut b,
        "b-extra-field",
        json!({"op":"caps_put","cap":primary,"unexpected":true}),
        Some("bad_arguments"),
    );
    b.get(
        &mut case,
        3,
        std::slice::from_ref(&primary),
        Some(72),
        Some(60),
    );
    let added = fresh(&mut case, "pair-added", "second", 48, 40);
    accepted(
        &b.ask(&mut case, json!({"op":"caps_put","cap":added})),
        4,
        true,
        Some(48),
        Some(40),
    );
    b.get(
        &mut case,
        4,
        &[primary.clone(), added.clone()],
        Some(48),
        Some(40),
    );
    accepted(
        &b.ask(
            &mut case,
            json!({"op":"caps_remove","source":"pair-primary","token":"first"}),
        ),
        5,
        false,
        Some(48),
        Some(40),
    );
    b.get(
        &mut case,
        5,
        std::slice::from_ref(&added),
        Some(48),
        Some(40),
    );
    let changed = case.image("b-committed");
    changed.expect(
        5,
        &[
            (&primary, "cancelled"),
            (&cancelled, "cancelled"),
            (&added, "active"),
        ],
    );
    b.kill(&mut case);
    assert!(changed == case.image("b-after-kill"));

    let mut again = Daemon::spawn(&mut case, writer, "return-a");
    again.get(
        &mut case,
        5,
        std::slice::from_ref(&added),
        Some(48),
        Some(40),
    );
    assert!(changed == case.image("a-returned"));
    for (label, cap) in [("a-old-cancel", &cancelled), ("a-new-cancel", &primary)] {
        let reply = unchanged(
            &mut case,
            &mut again,
            label,
            json!({"op":"caps_put","cap":cap}),
            None,
        );
        accepted(&reply, 5, false, Some(48), Some(40));
    }
    let reply = unchanged(
        &mut case,
        &mut again,
        "a-live-retry",
        json!({"op":"caps_put","cap":added}),
        None,
    );
    accepted(&reply, 5, true, Some(48), Some(40));
    unchanged(
        &mut case,
        &mut again,
        "a-wrong-remove",
        json!({"op":"caps_remove","source":"pair-added","token":"wrong"}),
        Some("token_mismatch"),
    );
    again.get(
        &mut case,
        5,
        std::slice::from_ref(&added),
        Some(48),
        Some(40),
    );
    // A persisted tombstone prevents replay, but a genuinely new restriction
    // from its source is still admitted without disturbing another source.
    let reborn = replacement(&mut case, &primary, "replacement", 60, 50);
    accepted(
        &again.ask(&mut case, json!({"op":"caps_put","cap":reborn})),
        6,
        true,
        Some(48),
        Some(40),
    );
    again.get(
        &mut case,
        6,
        &[reborn.clone(), added.clone()],
        Some(48),
        Some(40),
    );
    case.image("a-fresh-replacement").expect(
        6,
        &[
            (&reborn, "active"),
            (&cancelled, "cancelled"),
            (&added, "active"),
        ],
    );
    unchanged(
        &mut case,
        &mut again,
        "a-replaced-token",
        json!({"op":"caps_put","cap":primary}),
        Some("stale"),
    );
    let last = fresh(&mut case, "pair-final", "third", 36, 20);
    accepted(
        &again.ask(&mut case, json!({"op":"caps_put","cap":last})),
        7,
        true,
        Some(36),
        Some(20),
    );
    again.get(
        &mut case,
        7,
        &[reborn.clone(), added.clone(), last.clone()],
        Some(36),
        Some(20),
    );
    accepted(
        &again.ask(
            &mut case,
            json!({"op":"caps_remove","source":"pair-primary","token":"replacement"}),
        ),
        8,
        false,
        Some(36),
        Some(20),
    );
    again.get(
        &mut case,
        8,
        &[added.clone(), last.clone()],
        Some(36),
        Some(20),
    );
    accepted(
        &again.ask(
            &mut case,
            json!({"op":"caps_remove","source":"pair-added","token":"second"}),
        ),
        9,
        false,
        Some(36),
        Some(20),
    );
    again.get(
        &mut case,
        9,
        std::slice::from_ref(&last),
        Some(36),
        Some(20),
    );
    accepted(
        &again.ask(
            &mut case,
            json!({"op":"caps_remove","source":"pair-final","token":"third"}),
        ),
        10,
        false,
        None,
        None,
    );
    again.get(&mut case, 10, &[], None, None);
    let final_state = case.image("a-final");
    final_state.expect(
        10,
        &[
            (&reborn, "cancelled"),
            (&cancelled, "cancelled"),
            (&added, "cancelled"),
            (&last, "cancelled"),
        ],
    );
    again.kill(&mut case);
    assert!(final_state == case.image("a-final-after-kill"));
    case.finish(name);
}

// Socket timeouts are per read. One deadline prevents partial bytes from
// renewing the original I/O, step or case budget.
fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "absolute reply deadline"))
}
fn reply_line(stream: &mut UnixStream, deadline: Instant) -> io::Result<Vec<u8>> {
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
            if line.len() > 16 * 1024 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "oversized reply",
                ));
            }
            return Ok(line);
        }
        line.extend_from_slice(&chunk[..n]);
        if line.len() >= 16 * 1024 {
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
    let line = reply_line(&mut reader, Instant::now() + Duration::from_secs(1)).unwrap();
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
    let error = reply_line(&mut reader, began + Duration::from_millis(100)).unwrap_err();
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
