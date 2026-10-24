use lamp_bus::{Bus, Publisher};
use lamp_msgs::{ClockStamp, ScheduleDue, ScheduleNotice};
use lamp_schedule::{protocol::Request, store::Store};
use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags, recvmsg};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, IoSliceMut, Read, Write},
    mem::MaybeUninit,
    os::{
        fd::OwnedFd,
        unix::{
            fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
            net::{UnixDatagram, UnixListener, UnixStream},
            process::{CommandExt, ExitStatusExt},
        },
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};

const STORE: &str = "0123456789abcdef0123456789abcdef";
const CLAIM: &[u8] = b"\0lamp-schedule-consumer-claim-v1";
const WALL: i64 = 1_761_000_000_000;
const WITHIN: Duration = Duration::from_secs(20);
// The production claim is per UID, not per ledger directory. Even an explicit
// ignored run with parallel test threads must not start competing consumers.
static SERIAL: Mutex<()> = Mutex::new(());

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
                "af7cd0d1e384ebddcca2b58eea94176c80aba573a5f47f4cd95cc8ce1a7ad084",
                28_375_944,
                "ee53765c8d014eb00e41c1a5a7054d2546c18bdb",
            ),
            Self::Current => (
                "67ab7d3fd77bca2b6e5cf962bc0ec7a7db3520de1de3620f25e3899cb3a5a1f2",
                28_403_184,
                "846b3d203fc00ec3886d71f7673d8cc29cc4a634",
            ),
        }
    }
    fn copy(self, root: &Path) -> PathBuf {
        let variable = format!("LAMP_COMPAT_VOICE_{}", self.number());
        let source = std::env::var_os(&variable)
            .unwrap_or_else(|| panic!("manual fixture requires {variable}"));
        let (expected, size, _) = self.pin();
        assert_eq!(fs::metadata(&source).unwrap().len(), size as u64);
        let bytes = fs::read(source).unwrap();
        assert_eq!(sha(&bytes), expected, "{variable}: published SHA-256");
        assert_eq!(&bytes[..6], b"\x7fELF\x02\x01", "ELF64 little-endian");
        assert_eq!(&bytes[18..20], &[0xb7, 0], "AArch64 executable");
        let path = root.join(format!("voice-{}", self.number()));
        write(&path, &bytes);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o555)).unwrap();
        path
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Cut {
    Unfinished,
    Completed,
    AckIntent,
}
impl Cut {
    fn name(self) -> &'static str {
        match self {
            Self::Unfinished => "unfinished",
            Self::Completed => "completed",
            Self::AckIntent => "ack-intent",
        }
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

struct Voice {
    child: Child,
    peer: UnixDatagram,
    claim: Option<OwnedFd>,
    release: Release,
    stage: &'static str,
    offers: Vec<Value>,
    settled: Vec<Value>,
    idle: usize,
    allow_withdraw: bool,
    stopped: bool,
}
impl Voice {
    fn spawn(fixture: &mut Fixture, release: Release, stage: &'static str, binary: &Path) -> Self {
        let (parent, child) = UnixDatagram::pair().unwrap();
        parent.set_nonblocking(true).unwrap();
        let stdin: OwnedFd = child.into();
        let child = Command::new(binary)
            .arg("schedule-consumer")
            .env_clear()
            .env("LAMP_SCHEDULE_SPEAK", "on")
            .env("LAMP_SCHEDULE_DELIVERY_DIR", fixture.root.join("ledger"))
            .env("LAMP_SCHEDULE_SOCKET", fixture.root.join("s.sock"))
            .env("LAMP_BUS_ROOT", fixture.root.join("bus"))
            .current_dir(&fixture.root)
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::from(output(
                &fixture.root.join(format!("{stage}.stdout")),
            )))
            .stderr(Stdio::from(output(
                &fixture.root.join(format!("{stage}.stderr")),
            )))
            .process_group(0)
            .spawn()
            .unwrap();
        // Own cleanup before any fallible inspection of the running process.
        // A failed identity read/assertion must not leak a consumer or claim.
        let owned = Self {
            child,
            peer: parent,
            claim: None,
            release,
            stage,
            offers: Vec::new(),
            settled: Vec::new(),
            idle: 0,
            allow_withdraw: false,
            stopped: false,
        };
        let executable = fs::read(format!("/proc/{}/exe", owned.child.id())).unwrap();
        assert_eq!(
            sha(&executable),
            release.pin().0,
            "actual running executable"
        );
        fixture.record(
            json!({"event":"spawn","stage":stage,"release":release.number(),
            "pid":owned.child.id(),"executable_sha256":sha(&executable),
            "argv":["lamp-voice","schedule-consumer"]}),
        );
        owned
    }
    fn receive(&mut self, fixture: &mut Fixture) -> bool {
        let mut bytes = [0; 4097];
        let mut ancillary = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(2))];
        let mut control = RecvAncillaryBuffer::new(&mut ancillary);
        let received = match recvmsg(
            &self.peer,
            &mut [IoSliceMut::new(&mut bytes)],
            &mut control,
            RecvFlags::DONTWAIT,
        ) {
            Ok(received) => received,
            Err(rustix::io::Errno::AGAIN | rustix::io::Errno::INTR) => return false,
            Err(error) => panic!("consumer datagram: {error}"),
        };
        assert!(
            !received
                .flags
                .intersects(ReturnFlags::TRUNC | ReturnFlags::CTRUNC)
        );
        assert!((1..=4096).contains(&received.bytes));
        let mut rights = Vec::new();
        for message in control.drain() {
            let RecvAncillaryMessage::ScmRights(fds) = message else {
                panic!("unexpected consumer ancillary message");
            };
            for fd in fds {
                rustix::io::fcntl_setfd(&fd, rustix::io::FdFlags::CLOEXEC).unwrap();
                rights.push(fd);
            }
        }
        let packet = &bytes[..received.bytes];
        if packet == CLAIM {
            assert!(self.claim.is_none());
            assert_eq!(rights.len(), 1);
            self.claim = rights.pop();
            fixture.record(json!({"event":"claim_received","stage":self.stage}));
        } else {
            assert!(self.claim.is_some(), "protocol data must follow the claim");
            assert!(rights.is_empty());
            let message: Value = serde_json::from_slice(packet).unwrap();
            fixture.record(json!({"event":"consumer_packet","stage":self.stage,"message":message}));
            match message["kind"].as_str().unwrap() {
                "offer" => self.offers.push(message["offer"].clone()),
                "settled" => self.settled.push(message["token"].clone()),
                "idle" => self.idle += 1,
                "withdraw" => assert!(
                    self.allow_withdraw,
                    "fixture unexpectedly lost due authority: {message}"
                ),
                _ => panic!("unexpected consumer message: {message}"),
            }
        }
        true
    }
    fn complete(&self, fixture: &mut Fixture, token: &Value) -> Value {
        let proof = json!({"correlation":token["correlation"],"owner_boot":vec![7u8;16],
            "turn":1,"microphone_generation":2,"provider_session":3,
            "playback_sequence":4,"final_epoch":0,"final_frame":4800,
            "retired_generations":2});
        let message = json!({"kind":"complete","token":token,"proof":proof});
        let bytes = serde_json::to_vec(&message).unwrap();
        assert_eq!(self.peer.send(&bytes).unwrap(), bytes.len());
        fixture.record(json!({"event":"authored_completion","stage":self.stage,"message":message}));
        proof
    }
    fn kill_and_reap(&mut self, fixture: &mut Fixture) {
        assert!(
            self.child.try_wait().unwrap().is_none(),
            "consumer exited before the cut"
        );
        self.child.kill().unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < until, "consumer SIGKILL was not reaped");
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(status.signal(), Some(9), "actual SIGKILL wait status");
        self.stopped = true;
        assert!(
            self.claim.take().is_some(),
            "retain claim until verified exit"
        );
        fixture.record(
            json!({"event":"killed_reaped_claim_closed","stage":self.stage,
            "release":self.release.number(),"pid":self.child.id(),"exit_signal":status.signal()}),
        );
    }
}
impl Drop for Voice {
    fn drop(&mut self) {
        if self.stopped || matches!(self.child.try_wait(), Ok(Some(_))) {
            return;
        }
        let _ = self.child.kill();
        let until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < until {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        eprintln!("release-pair consumer cleanup did not confirm reap within five seconds");
    }
}

struct Incoming {
    stream: UnixStream,
    bytes: Vec<u8>,
    until: Instant,
}

struct Fixture {
    root: PathBuf,
    transcript: File,
    events: usize,
    listener: UnixListener,
    incoming: Vec<Incoming>,
    publisher: Publisher,
    clock: String,
    sequence: u64,
    next_due: Instant,
    get_allowed: bool,
    ack_allowed: bool,
    pending: bool,
    ack_requests: Vec<Value>,
    committed_ack: Option<Value>,
    ack_commits: usize,
    deadline: Instant,
}
impl Fixture {
    fn new(writer: Release, reader: Release, cut: Cut) -> Self {
        let parent = std::env::var_os("LAMP_COMPAT_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        assert!(parent.is_absolute() && parent.is_dir());
        let nonce = lamp_voice::process::boot_text(lamp_voice::process::new_boot().unwrap());
        let root = parent.join(format!("vr-{}", &nonce[..12]));
        assert!(root.join("s.sock").as_os_str().as_encoded_bytes().len() <= 103);
        directory(&root);
        directory(&root.join("ledger"));
        directory(&root.join("bus"));
        let listener = UnixListener::bind(root.join("s.sock")).unwrap();
        listener.set_nonblocking(true).unwrap();
        fs::set_permissions(root.join("s.sock"), fs::Permissions::from_mode(0o600)).unwrap();
        let publisher = Bus::new(root.join("bus"))
            .unwrap()
            .publisher_declared(ScheduleDue::TOPIC, &ScheduleDue::declaration(), "schedule")
            .unwrap();
        let clock = fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .unwrap()
            .trim()
            .to_owned();
        json_file(
            &root.join("case.json"),
            &json!({
                "writer":writer.number(),"reader":reader.number(),"return_reader":writer.number(),
                "cut":cut.name(),"writer_sha256":writer.pin().0,"reader_sha256":reader.pin().0,
                "writer_source":writer.pin().2,"reader_source":reader.pin().2,
                "writer_bytes":writer.pin().1,"reader_bytes":reader.pin().1,
                "boundary":"released delivery-ledger consumer; authored Schedule API and completion; no audio"
            }),
        );
        println!("voice release-pair evidence: {}", root.display());
        Self {
            transcript: output(&root.join("transcript.jsonl")),
            root,
            events: 0,
            listener,
            incoming: Vec::new(),
            publisher,
            clock,
            sequence: 0,
            next_due: Instant::now(),
            get_allowed: true,
            ack_allowed: false,
            pending: true,
            ack_requests: Vec::new(),
            committed_ack: None,
            ack_commits: 0,
            deadline: Instant::now() + Duration::from_secs(90),
        }
    }
    fn record(&mut self, mut value: Value) {
        self.events += 1;
        assert!(self.events <= 1024, "bounded fixture transcript");
        value["at_us"] = lamp_ipc::monotonic_us().into();
        serde_json::to_writer(&mut self.transcript, &value).unwrap();
        self.transcript.write_all(b"\n").unwrap();
    }
    fn publish(&mut self) {
        if Instant::now() < self.next_due {
            return;
        }
        self.sequence += 1;
        let now = lamp_ipc::monotonic_us();
        let due = ScheduleDue {
            stamp: ClockStamp {
                clock_id: self.clock.clone(),
                at_us: now,
                sequence: self.sequence,
            },
            store_id: STORE.into(),
            revision: if self.pending { 2 } else { 3 },
            enabled: true,
            ready: true,
            pending: u32::from(self.pending),
            waiting_for_clock: 0,
            oldest: self.pending.then_some(ScheduleNotice {
                id: 1,
                delivery: 1,
                due_unix_ms: WALL,
                late_by_ms: Some(0),
                late: false,
            }),
        };
        self.publisher.publish_json(now, &due).unwrap();
        self.next_due = Instant::now() + Duration::from_millis(50);
    }
    fn item(&self) -> Value {
        json!({"id":1,"delivery":1,"text":"Fixture reminder","kind":"timer",
            "request_id":"fixture-set","state":"pending","due_unix_ms":WALL,
            "local":"2025-10-20T22:40:00+00:00","original_due_unix_ms":WALL,
            "repeat":null,"snoozed":false,"remaining_ms":0,"late":false,"late_by_ms":0})
    }
    fn answer(&mut self, value: Value) -> Option<Value> {
        let request = Request::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
        self.record(json!({"event":"schedule_request","request":value}));
        match request {
            Request::Get { id: 1 } => {
                if !self.get_allowed {
                    return None;
                }
                Some(if self.pending {
                    json!({"ok":true,"store_id":STORE,"revision":2,"item":self.item()})
                } else {
                    json!({"ok":false,"error":"not_found"})
                })
            }
            Request::Ack {
                guard,
                id: 1,
                delivery: 1,
            } => {
                assert_eq!(guard.store_id, STORE);
                assert_eq!(guard.revision, 2);
                assert!(guard.request_id.starts_with("due."));
                self.ack_requests.push(value.clone());
                assert!(self.ack_requests.len() <= 64);
                if !self.ack_allowed {
                    return None;
                }
                if let Some(previous) = &self.committed_ack {
                    assert_eq!(previous, &value, "only the exact Ack can replay");
                } else {
                    assert!(self.pending);
                    self.committed_ack = Some(value);
                    self.pending = false;
                    self.ack_commits += 1;
                }
                Some(json!({"ok":true,"store_id":STORE,"revision":3,
                    "acknowledged":true,"id":1,"delivery":1}))
            }
            Request::Status {} => Some(json!({"ok":true,"store_id":STORE,
            "revision":if self.pending {2} else {3},"items":u32::from(self.pending),
            "pending":u32::from(self.pending),"waiting_for_clock":0,"wall_trusted":true,
            "zone":"Etc/UTC","max_items":128,"last_receipt":self.committed_ack.as_ref().map(|ack| json!({
                "request_id":ack["guard"]["request_id"],"op":"ack",
                "response":{"ok":true,"store_id":STORE,"revision":3,"acknowledged":true,"id":1,"delivery":1}
            }))})),
            Request::List {
                after_id,
                limit,
                pending_only,
            } => {
                assert!(after_id <= 1 && (1..=8).contains(&limit));
                assert!(pending_only || (after_id == 0 && limit == 1));
                let items = if self.pending && after_id == 0 {
                    vec![self.item()]
                } else {
                    Vec::new()
                };
                Some(
                    json!({"ok":true,"store_id":STORE,"revision":if self.pending {2} else {3},
                    "items":items,"next_after_id":null}),
                )
            }
            other => panic!("unexpected Schedule operation, no side effect performed: {other:?}"),
        }
    }
    fn serve(&mut self) {
        for _ in 0..8 {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    assert!(self.incoming.len() < 8);
                    stream.set_nonblocking(true).unwrap();
                    self.incoming.push(Incoming {
                        stream,
                        bytes: Vec::new(),
                        until: Instant::now() + Duration::from_secs(1),
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("fixture Schedule accept: {error}"),
            }
        }
        let mut index = 0;
        while index < self.incoming.len() {
            let incoming = &mut self.incoming[index];
            assert!(
                Instant::now() < incoming.until,
                "fixture Schedule incomplete request"
            );
            let mut bytes = [0; 8193];
            match incoming.stream.read(&mut bytes) {
                Ok(0) => {
                    self.incoming.swap_remove(index);
                    continue;
                }
                Ok(count) => incoming.bytes.extend_from_slice(&bytes[..count]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    index += 1;
                    continue;
                }
                Err(error) => panic!("fixture Schedule read: {error}"),
            }
            assert!(incoming.bytes.len() <= 8192);
            if !incoming.bytes.ends_with(b"\n") {
                index += 1;
                continue;
            }
            let mut incoming = self.incoming.swap_remove(index);
            let request: Value = serde_json::from_slice(&incoming.bytes).unwrap();
            if let Some(reply) = self.answer(request) {
                self.record(json!({"event":"schedule_reply","reply":reply}));
                incoming.stream.set_nonblocking(false).unwrap();
                incoming
                    .stream
                    .set_write_timeout(Some(Duration::from_millis(200)))
                    .unwrap();
                writeln!(incoming.stream, "{reply}").unwrap();
            }
        }
    }
    fn until(
        &mut self,
        voice: &mut Voice,
        label: &str,
        mut condition: impl FnMut(&Self, &Voice) -> bool,
    ) {
        let until = (Instant::now() + WITHIN).min(self.deadline);
        loop {
            assert!(
                voice.child.try_wait().unwrap().is_none(),
                "{} consumer exited during {label}",
                voice.stage
            );
            self.publish();
            self.serve();
            for _ in 0..32 {
                if !voice.receive(self) {
                    break;
                }
            }
            if condition(self, voice) {
                return;
            }
            assert!(
                Instant::now() < until,
                "consumer timed out during {label}; evidence retained"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn snapshot(&mut self, label: &str) -> Value {
        // Inspect a copy only. Opening today's Store must never pre-recover or
        // rewrite the live database before the opposite released reader sees it.
        let live = self.root.join("ledger");
        let before = manifest(&live);
        let copy = self.root.join(format!("{label}-store"));
        directory(&copy);
        for entry in fs::read_dir(&live).unwrap() {
            let entry = entry.unwrap();
            assert!(entry.file_type().unwrap().is_file());
            fs::copy(entry.path(), copy.join(entry.file_name())).unwrap();
        }
        let snapshot = Store::open(&copy).unwrap().load().unwrap().unwrap();
        write(&self.root.join(format!("{label}-snapshot.json")), &snapshot);
        let after = manifest(&live);
        assert_eq!(
            before, after,
            "forensic copy must not alter the live ledger"
        );
        json_file(&self.root.join(format!("{label}-live-files.json")), &after);
        serde_json::from_slice(&snapshot).unwrap()
    }
}

fn manifest(directory: &Path) -> Value {
    let mut entries = serde_json::Map::new();
    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        assert!(entry.file_type().unwrap().is_file());
        let metadata = entry.metadata().unwrap();
        assert!(metadata.len() <= 16 * 1_048_576);
        entries.insert(
            entry.file_name().into_string().unwrap(),
            json!({
                "bytes":metadata.len(),"mode":metadata.mode()&0o7777,
                "sha256":sha(&fs::read(entry.path()).unwrap())
            }),
        );
    }
    Value::Object(entries)
}

pub fn check(writer: Release, reader: Release, cut: Cut) {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(std::env::consts::OS, "linux", "manual native fixture");
    assert_eq!(
        std::env::consts::ARCH,
        "aarch64",
        "no emulated or host reader"
    );
    let mut fixture = Fixture::new(writer, reader, cut);
    let writer_binary = writer.copy(&fixture.root);
    let reader_binary = reader.copy(&fixture.root);
    let mut first = Voice::spawn(&mut fixture, writer, "writer", &writer_binary);
    fixture.until(&mut first, "first durable offer", |_, voice| {
        !voice.offers.is_empty()
    });
    let token = first.offers[0]["token"].clone();
    let mut proof = None;
    if cut != Cut::Unfinished {
        // Deny every later Get before submitting completion. Consequently the
        // completed-only cut cannot race the next 200 ms recovery tick into Ack.
        fixture.get_allowed = cut == Cut::AckIntent;
        // A Get already in flight may first withdraw the offered lease. The
        // same token remains eligible for its completed whole-reply receipt.
        first.allow_withdraw = cut == Cut::Completed;
        proof = Some(first.complete(&mut fixture, &token));
        fixture.until(&mut first, "durable completion settlement", |_, voice| {
            voice.settled.contains(&token)
        });
        if cut == Cut::AckIntent {
            fixture.until(
                &mut first,
                "durable Ack before refused transmission",
                |fixture, _| !fixture.ack_requests.is_empty(),
            );
        }
    }
    first.kill_and_reap(&mut fixture);
    let saved = fixture.snapshot("cut");
    assert_eq!(saved["kind"], "lamp_schedule_delivery_ledger");
    assert_eq!(saved["schema"], 1);
    let entries = saved["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    let view = &entries[0]["view"];
    assert_eq!(view["prepared"], token);
    assert_eq!(entries[0]["first_attempt"], token["attempt"]);
    assert!(view["resolution"].is_null());
    match cut {
        Cut::Unfinished => {
            assert!(view["completion"].is_null() && view["ack"].is_null());
            assert!(fixture.ack_requests.is_empty());
        }
        Cut::Completed => {
            assert_eq!(view["completion"], *proof.as_ref().unwrap());
            assert!(view["ack"].is_null() && fixture.ack_requests.is_empty());
        }
        Cut::AckIntent => {
            assert_eq!(view["completion"], *proof.as_ref().unwrap());
            assert_eq!(view["ack"]["request"], fixture.ack_requests[0]);
        }
    }
    fixture.get_allowed = true;
    let ack_before = fixture.ack_requests.len();
    let mut second = Voice::spawn(&mut fixture, reader, "reader", &reader_binary);
    let completion_token = if cut == Cut::Unfinished {
        fixture.until(
            &mut second,
            "unfinished occurrence offered by other release",
            |_, voice| !voice.offers.is_empty(),
        );
        let retry = second.offers[0]["token"].clone();
        assert_eq!(retry["key"], token["key"]);
        assert!(retry["attempt"].as_u64().unwrap() > token["attempt"].as_u64().unwrap());
        assert_ne!(
            retry["correlation"]["consumer_session"],
            token["correlation"]["consumer_session"]
        );
        assert_eq!(
            fixture.ack_requests.len(),
            ack_before,
            "unfinished is never acknowledged before completion"
        );
        second.complete(&mut fixture, &retry);
        fixture.until(&mut second, "retry completion settled", |_, voice| {
            voice.settled.contains(&retry)
        });
        retry
    } else {
        token.clone()
    };
    fixture.until(
        &mut second,
        "opposite reader recovered Ack",
        |fixture, _| fixture.ack_requests.len() > ack_before,
    );
    let idles = second.idle;
    fixture.until(
        &mut second,
        "completed occurrence stays quiet through fresh due ticks",
        |_, voice| voice.idle >= idles + 3,
    );
    if cut != Cut::Unfinished {
        assert!(second.offers.is_empty());
    } else {
        assert!(
            second
                .offers
                .iter()
                .all(|offer| offer["token"] == completion_token)
        );
    }
    let expected_ack = fixture.ack_requests[ack_before].clone();
    assert!(
        fixture.ack_requests[ack_before..]
            .iter()
            .all(|ack| *ack == expected_ack)
    );
    assert_eq!(
        expected_ack["guard"]["request_id"],
        format!(
            "due.{}.{}",
            saved["ledger_id"].as_str().unwrap(),
            // Delivery retries get new completion tokens, but the occurrence's
            // Ack idempotency key retains its first durable attempt number.
            entries[0]["first_attempt"].as_u64().unwrap()
        )
    );
    if cut == Cut::AckIntent {
        assert_eq!(expected_ack, view["ack"]["request"]);
    }
    fixture.ack_allowed = true;
    fixture.until(&mut second, "fixture Ack accepted once", |fixture, _| {
        fixture.ack_commits == 1
    });
    let idles = second.idle;
    fixture.until(
        &mut second,
        "accepted Ack durably resolved and compacted",
        |_, voice| voice.idle >= idles + 3,
    );
    second.kill_and_reap(&mut fixture);
    let recovered = fixture.snapshot("recovered");
    assert_eq!(
        recovered["ledger_id"], saved["ledger_id"],
        "no empty replacement ledger"
    );
    assert_eq!(recovered["schema"], 1);
    assert_eq!(
        recovered["next_attempt"].as_u64().unwrap(),
        completion_token["attempt"].as_u64().unwrap() + 1
    );
    assert!(
        recovered["entries"].as_array().unwrap().is_empty(),
        "only confirmed Ack resolves and compacts"
    );
    let acks = fixture.ack_requests.len();
    let mut returned = Voice::spawn(&mut fixture, writer, "return-reader", &writer_binary);
    fixture.until(
        &mut returned,
        "original release reads recovered state",
        |_, voice| voice.idle >= 3,
    );
    assert!(returned.offers.is_empty());
    assert_eq!(
        fixture.ack_requests.len(),
        acks,
        "resolved occurrence does not Ack again"
    );
    returned.kill_and_reap(&mut fixture);
    assert_eq!(fixture.snapshot("returned"), recovered);
    assert_eq!(fixture.ack_commits, 1);
    let mut after = Vec::new();
    for (release, path) in [(writer, &writer_binary), (reader, &reader_binary)] {
        let metadata = fs::metadata(path).unwrap();
        let digest = sha(&fs::read(path).unwrap());
        assert_eq!(digest, release.pin().0, "copied executable after execution");
        assert_eq!(metadata.len(), release.pin().1 as u64);
        assert_eq!(metadata.mode() & 0o7777, 0o555);
        after.push(json!({"release":release.number(),"sha256":digest,
            "bytes":metadata.len(),"mode":metadata.mode()&0o7777}));
    }
    json_file(&fixture.root.join("artifacts-after.json"), &json!(after));
    fixture.record(
        json!({"event":"passed","cut":cut.name(),"writer":writer.number(),
        "reader":reader.number(),"ack_commits":fixture.ack_commits,"ledger_id":saved["ledger_id"]}),
    );
    fixture.transcript.sync_all().unwrap();
}
