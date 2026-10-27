//! Actual Eyes listener/at-lock -> exec-separated Voice -> local dispatch.
//! These are Voice unit-test driver execs hosting Eyes library / Voice APIs,
//! not shipping binary/main bootstrap. No hardware, model, Live session, global
//! environment mutation or deployment.
use super::*;
use crate::{
    memory_scope::{RequestKey, RequestScope, ScopeSnapshot},
    second_provider::{Prepared, PrivateContext, net},
};
use lamp_eyes::{
    camera_authority::{Binding, Service},
    permit::Permit,
    who::KnownSpeakerPublisher,
};
use lamp_turn_owner::{AdmissionState, AdmittedInput, CaptureState, Controller, Permission};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixDatagram, UnixListener},
    },
    process::{Child, Command, Stdio},
    sync::{atomic::AtomicUsize, mpsc},
    thread,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    task::JoinSet,
};
const CHILD: &str = "LAMP_VOICE_CAMERA_TICKET_EXEC_KNOWN";
const BOOT: &str = "0123456789abcdef";
const NAME: &str = "Fixture owner";
const CANARY: &str = "PRIVATE TICKET FIXTURE";
const NOW: u64 = 10_000_000;
struct Directory {
    path: PathBuf,
    _owner: crate::process::SessionDirectory,
}
impl Directory {
    fn new() -> Self {
        let owner = crate::process::SessionDirectory::create().unwrap();
        Self {
            path: owner.path.canonicalize().unwrap(),
            _owner: owner,
        }
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
fn wait<T>(mut read: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(value) = read() {
            return value;
        }
        assert!(Instant::now() < until, "bounded fixture readiness");
        thread::sleep(Duration::from_millis(1));
    }
}
fn report(line: &str) {
    println!("ticket:{line}");
    std::io::stdout().flush().unwrap();
}
struct Process {
    child: Child,
    lines: mpsc::Receiver<String>,
    reader: Option<thread::JoinHandle<()>>,
}
impl Process {
    fn start(known: &Path, test: &str) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture"])
            .env(CHILD, known)
            // Child-only proxy exclusion: all fixture HTTP targets are local.
            .env("NO_PROXY", "*")
            .env("no_proxy", "*")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, lines) = mpsc::channel();
        let reader = thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) => {
                        if send.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        let process = Self {
            child,
            lines,
            reader: Some(reader),
        };
        process.line("ticket:idle");
        process
    }
    fn line(&self, wanted: &str) {
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            let line = self
                .lines
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .expect("bounded exec child output");
            if line.ends_with(wanted) {
                return;
            }
        }
    }
    fn finish(&mut self) {
        self.child.stdin.take();
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            match self.child.try_wait().unwrap() {
                Some(status) => {
                    assert!(status.success(), "exec fixture child failed");
                    return;
                }
                None => {
                    assert!(Instant::now() < until, "exec fixture exit deadline");
                    thread::sleep(Duration::from_millis(2));
                }
            }
        }
    }
    fn command(&mut self, command: &str, answer: &str) {
        let input = self.child.stdin.as_mut().unwrap();
        writeln!(input, "{command}").unwrap();
        input.flush().unwrap();
        self.line(answer);
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.child.stdin.take();
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < until => thread::sleep(Duration::from_millis(2)),
                _ => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break;
                }
            }
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
struct HeldPublication {
    release: Option<mpsc::Sender<()>>,
    worker: Option<thread::JoinHandle<()>>,
}
impl HeldPublication {
    fn start(permit: Arc<Permit>, hold: u64) -> Self {
        let (entered, at) = mpsc::channel();
        let (release, done) = mpsc::channel();
        let worker = thread::spawn(move || {
            permit.with_frame_authority(Some(hold), lamp_ipc::monotonic_us(), |_, valid, _| {
                assert!(valid);
                entered.send(()).unwrap();
                done.recv_timeout(Duration::from_secs(3))
                    .expect("bounded held publication");
            })
        });
        let guard = Self {
            release: Some(release),
            worker: Some(worker),
        };
        at.recv_timeout(Duration::from_secs(2)).unwrap();
        guard
    }
}
impl Drop for HeldPublication {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
struct Heartbeat {
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Heartbeat {
    fn start(socket: UnixDatagram) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let ended = stop.clone();
        let worker = thread::spawn(move || {
            while !ended.load(Ordering::Acquire) {
                if socket.send(br#"{"camera":true}"#).is_err() {
                    break;
                }
                thread::park_timeout(Duration::from_millis(100));
            }
        });
        Self {
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for Heartbeat {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}
fn publish(publisher: &KnownSpeakerPublisher, binding: Binding) {
    publisher.camera_ticket(Some(binding));
    publisher.permission_until(Some(binding.until_us));
    publisher.seen_frame(NAME, None, BOOT, "f7", lamp_ipc::monotonic_us());
}

/// Dedicated exec child: real production Eyes APIs only, no camera/inference.
/// Its process boundary also bounds the listener thread's existing lifetime.
#[test]
fn fixture_producer() {
    let Some(known) = std::env::var_os(CHILD) else {
        return;
    };
    let known = PathBuf::from(known);
    let publisher = KnownSpeakerPublisher::new(known.clone());
    publisher.require_camera_ticket();
    let service = Service::start(&known, publisher.publisher_id().unwrap()).unwrap();
    let authority = service.authority();
    let (send, socket) = UnixDatagram::pair().unwrap();
    let permit = Arc::new(Permit::listen_with_authority(socket, Some(authority.clone())).unwrap());
    let mut heartbeat = None;
    let mut held = None;
    let mut hold = None;
    report("idle");
    for line in std::io::stdin().lock().lines() {
        match line.unwrap().as_str() {
            "open" => {
                send.send(br#"{"camera":true}"#).unwrap();
                heartbeat = Some(Heartbeat::start(send.try_clone().unwrap()));
                let current = wait(|| permit.held());
                hold = Some(current);
                publish(
                    &publisher,
                    wait(|| authority.frame(Some(current), lamp_ipc::monotonic_us())),
                );
                report("opened");
            }
            "refresh" => {
                publish(
                    &publisher,
                    wait(|| authority.frame(hold, lamp_ipc::monotonic_us())),
                );
                report("refreshed");
            }
            "hold-close" => {
                drop(heartbeat.take());
                send.send(br#"{"fixture_barrier":true}"#).unwrap();
                wait(|| {
                    permit
                        .heard()
                        .iter()
                        .any(|packet| packet == br#"{"fixture_barrier":true}"#)
                        .then_some(())
                });
                publish(
                    &publisher,
                    wait(|| authority.frame(hold, lamp_ipc::monotonic_us())),
                );
                held = Some(HeldPublication::start(permit.clone(), hold.unwrap()));
                send.send(br#"{"camera":false}"#).unwrap();
                report("held");
            }
            "release" => {
                drop(held.take());
                report("released");
            }
            other => panic!("unexpected fixture command {other}"),
        }
    }
    drop(held);
    drop(heartbeat);
    drop(service);
}

struct Endpoints {
    http: tokio::task::JoinHandle<()>,
    memory: Option<thread::JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    chat: Arc<AtomicUsize>,
    tts: Arc<AtomicUsize>,
    reads: Arc<AtomicUsize>,
    url: String,
    socket: PathBuf,
}
impl Endpoints {
    async fn start(directory: &Path) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let chat = Arc::new(AtomicUsize::new(0));
        let tts = Arc::new(AtomicUsize::new(0));
        let (chats, speech) = (chat.clone(), tts.clone());
        let http = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                tokio::time::timeout(Duration::from_secs(2),async {
                let mut bytes=Vec::new();let mut chunk=[0;1024];
                let head=loop {let n=stream.read(&mut chunk).await.unwrap();assert!(n>0);bytes.extend_from_slice(&chunk[..n]);assert!(bytes.len()<32768);if let Some(end)=bytes.windows(4).position(|w|w==b"\r\n\r\n"){break end+4;}};
                let header=String::from_utf8_lossy(&bytes[..head]);let is_tts=header.lines().next().unwrap().contains("text-to-speech");let length=header.lines().find_map(|line|line.to_ascii_lowercase().strip_prefix("content-length:").and_then(|v|v.trim().parse::<usize>().ok())).unwrap();
                while bytes.len()<head+length {let n=stream.read(&mut chunk).await.unwrap();assert!(n>0);bytes.extend_from_slice(&chunk[..n]);assert!(bytes.len()<32768);}
                let (kind,body)=if is_tts {speech.fetch_add(1,Ordering::AcqRel);("audio/pcm",vec![1,0,2,0])} else {chats.fetch_add(1,Ordering::AcqRel);("text/event-stream",b"data: {\"choices\":[{\"delta\":{\"content\":\"Fixture answer.\"}}]}\n\ndata: [DONE]\n\n".to_vec())};
                let header=format!("HTTP/1.1 200 OK\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",body.len());stream.write_all(header.as_bytes()).await.unwrap();stream.write_all(&body).await.unwrap();
            }).await.expect("bounded local HTTP request");
            }
        });
        let socket = directory.join("memory.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let ended = stop.clone();
        let reads = Arc::new(AtomicUsize::new(0));
        let count = reads.clone();
        let memory = thread::spawn(move || {
            while !ended.load(Ordering::Acquire) {
                if !lamp_ipc::readable(&listener, Duration::from_millis(10)).unwrap_or(false) {
                    continue;
                }
                let Ok((mut stream, _)) = listener.accept() else {
                    continue;
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut line = String::new();
                if BufReader::new(&stream).read_line(&mut line).is_ok() {
                    count.fetch_add(1, Ordering::AcqRel);
                    let _=stream.write_all(b"{\"ok\":true,\"entries\":[{\"text\":\"fixturememory\",\"at_unix_ms\":1}]}\n");
                }
            }
        });
        Self {
            http,
            memory: Some(memory),
            stop,
            chat,
            tts,
            reads,
            url,
            socket,
        }
    }
    fn counts(&self) -> (usize, usize, usize) {
        (
            self.chat.load(Ordering::Acquire),
            self.tts.load(Ordering::Acquire),
            self.reads.load(Ordering::Acquire),
        )
    }
    fn net(&self, directory: &Path) -> Net {
        let key = directory.join("key");
        fs::write(&key, "local-fixture-key").unwrap();
        fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
        let path = directory.join("config.json");
        fs::write(&path,json!({"llm_base_url":"https://relay.example.invalid/api/v1/ai","llm_model":"m","llm_credential_file":key,"tts_provider":"elevenlabs"}).to_string()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let mut config = crate::second_config::load_from_path(&path).unwrap();
        config.llm_base_url = self.url.clone();
        config.tts_base_url = self.url.clone();
        Net::new(config, Arc::new(Vec::new)).unwrap()
    }
}
impl Drop for Endpoints {
    fn drop(&mut self) {
        self.http.abort();
        self.stop.store(true, Ordering::Release);
        if let Some(memory) = self.memory.take() {
            let _ = memory.join();
        }
        let _ = fs::remove_file(&self.socket);
    }
}
struct Ipc {
    parent: WorkerChannels,
    worker: WorkerChannels,
    gate: Gate,
    pending: Option<QueuedOutput>,
}
impl Ipc {
    fn new(directory: &Path, engine: &mut Engine) -> Self {
        let boot = BootId::new([37; 16]).unwrap();
        let worker_boot = BootId::new([38; 16]).unwrap();
        let mut parent =
            WorkerChannels::bind(directory, "second", boot, worker_boot, false).unwrap();
        let mut worker =
            WorkerChannels::bind(directory, "second", boot, worker_boot, true).unwrap();
        parent.connect(directory, "second", false).unwrap();
        worker.connect(directory, "second", true).unwrap();
        let at = MonoTime::from_micros(NOW);
        let mut owner = Controller::new(boot, at);
        owner
            .set_microphone_permission(at, Permission::Allowed)
            .unwrap();
        owner
            .set_camera_permission(at, Permission::Allowed)
            .unwrap();
        let until = MonoTime::from_micros(NOW + 100_000);
        owner
            .set_capture(at, CaptureState::RetainingUntil(until))
            .unwrap();
        owner
            .set_admission(at, AdmissionState::OpenUntil(until))
            .unwrap();
        assert_eq!(owner.admit(at, AdmittedInput::NewTurn).unwrap().turn(), 1);
        let state = owner.snapshot(at).unwrap();
        let mut gate = Gate::new(boot, NOW);
        gate.install(NOW, state, engine).unwrap();
        Self {
            parent,
            worker,
            gate,
            pending: None,
        }
    }
    async fn positive(&mut self, engine: &mut Engine, guard: &PrivateContext) {
        let event = || ProviderOutput::Transcript {
            request: Some(1),
            text: CANARY.into(),
            finished: false,
        };
        assert!(engine.shared.emit_private(guard, event()).await);
        let pending = next_output(engine).unwrap();
        assert!(
            self.gate
                .send_or_discard(engine, NOW, &pending, |event| self
                    .worker
                    .data
                    .send_with(event, || NOW))
                .unwrap()
        );
        assert!(
            self.parent
                .data
                .receive_with::<ProviderOutput>(|| NOW)
                .unwrap()
                .is_some()
        );
        assert!(engine.shared.emit_private(guard, event()).await);
        self.pending = next_output(engine);
        assert!(self.pending.is_some(), "already queued before camera close");
    }
    fn closed(&mut self, engine: &Engine) {
        let pending = self.pending.take().unwrap();
        let mut sends = 0;
        assert!(
            self.gate
                .send_or_discard(engine, NOW, &pending, |event| {
                    sends += 1;
                    self.worker.data.send_with(event, || NOW)
                })
                .unwrap()
        );
        assert_eq!(
            sends, 0,
            "final IPC guard must deny already queued private bytes"
        );
        assert!(
            self.parent
                .data
                .receive_with::<ProviderOutput>(|| NOW)
                .unwrap()
                .is_none()
        );
    }
}
async fn probe(
    engine: &Engine,
    scope: &RequestScope,
    known: &Path,
    endpoints: &Endpoints,
    positive: bool,
) {
    use crate::second_provider::start_speech;
    let lease = scope.lease();
    let mut prepared = Prepared::begin(
        &engine.shared,
        Some(&lease),
        "question".into(),
        &[],
        None,
        false,
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        while prepared.events.recv().await.is_some() {}
    })
    .await
    .expect("bounded actual chat transport");
    let guard = PrivateContext::new(&engine.shared, Some(&lease), true);
    let mut speech = JoinSet::new();
    let mut packets = start_speech(
        &engine.shared,
        &mut speech,
        CANARY.into(),
        net::Turn {
            request: 1,
            heard_at: Instant::now(),
        },
        Some(guard),
    );
    let mut samples = 0;
    tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(packet) = packets.recv().await {
            samples += packet.unwrap().len();
        }
        while let Some(done) = speech.join_next().await {
            done.unwrap();
        }
    })
    .await
    .expect("bounded actual TTS transport");
    assert_eq!(samples > 0, positive);
    let person = lease
        .scope()
        .and_then(|scope| scope.person())
        .map(str::to_owned);
    let permission = memory_read_permission(Some(&lease), Some(known), &person);
    let hits = crate::remember::hits_note_permitted(
        &endpoints.socket,
        person.as_deref(),
        "fixturememory",
        1,
        None,
        &permission,
    );
    assert_eq!(hits.is_some(), positive);
    // Actual private Memory hits transport uses the same caller's permission
    // at each socket boundary, not a local boolean or a text-only assertion.
}

fn owner(known: &Path, request: u64) -> RequestScope {
    let cache = Arc::new(crate::memory_scope::FaceCache::default());
    assert!(
        cache.set_camera_client(
            crate::camera_authority::Client::start(known.to_path_buf()).unwrap()
        )
    );
    let mut owner = RequestScope::looking_later(
        RequestKey {
            worker: BootId::new([38; 16]).unwrap(),
            session: 1,
            request,
        },
        ScopeSnapshot::of(NAME),
        cache,
    );
    let key = owner.lease().key();
    assert!(owner.speaker_recorder().unwrap().seal(
        key,
        &[format!("{BOOT}/f7")],
        Instant::now() + Duration::from_secs(30)
    ));
    owner
}
fn ready(known: &Path, owner: &RequestScope) {
    wait(
        || match private_context_state(Some(known), &owner.lease()) {
            PermissionState::Ready => Some(()),
            PermissionState::Pending => None,
            PermissionState::Lost => panic!("actual first ticket/source grant refused"),
        },
    );
}

/// Runs as a separate native process with production client/source/Engine
/// guard paths, local Net HTTP/TTS, Memory socket and credited worker IPC.
#[tokio::test]
async fn fixture_consumer() {
    let Some(known) = std::env::var_os(CHILD) else {
        return;
    };
    let known = PathBuf::from(known);
    let directory = Directory::new();
    let endpoints = Endpoints::start(&directory.path).await;
    let net = endpoints.net(&directory.path);
    let validate = known.clone();
    let mut engine = Engine::new(
        Arc::new(net),
        || Told {
            text: CANARY.into(),
            requires_speaker: true,
            ..Told::default()
        },
        Arc::new(|_, _, _, _| panic!("no tools in exec fixture")),
    )
    .guarding_private_context(Arc::new(move |lease| {
        private_context_state(Some(&validate), lease)
    }));
    let mut ipc = Ipc::new(&directory.path, &mut engine);
    let mut scope = None;
    report("idle");
    for command in std::io::stdin().lock().lines() {
        match command.unwrap().as_str() {
            "bind" => {
                let owner = owner(&known, 1);
                ready(&known, &owner);
                tokio::time::timeout(
                    Duration::from_secs(2),
                    probe(&engine, &owner, &known, &endpoints, true),
                )
                .await
                .expect("positive actual boundary probes");
                let guard = PrivateContext::new(&engine.shared, Some(&owner.lease()), true);
                ipc.positive(&mut engine, &guard).await;
                assert_eq!(endpoints.counts(), (1, 1, 1));
                scope = Some(owner);
                report("positive counts=1,1,1,1");
            }
            "closed" => {
                let owner = scope.as_ref().unwrap();
                assert!(owner.lease().is_current());
                assert_eq!(
                    private_context_state(Some(&known), &owner.lease()),
                    PermissionState::Lost
                );
                tokio::time::timeout(
                    Duration::from_secs(2),
                    probe(&engine, owner, &known, &endpoints, false),
                )
                .await
                .expect("closed actual boundary probes");
                ipc.closed(&engine);
                tokio::task::yield_now().await;
                assert_eq!(endpoints.counts(), (1, 1, 1));
                assert!(owner.lease().is_current());
                report("closed counts=1,1,1,1");
            }
            other => panic!("unexpected consumer command {other}"),
        }
    }
    drop(scope);
    drop(ipc);
    drop(engine);
    drop(endpoints);
}

#[test]
fn actual_eyes_close_with_publication_held_stops_two_exec_voice_consumers() {
    let directory = Directory::new();
    let known = directory.path.join("known");
    let prefix = "second_provider::worker::camera_ticket_exec_tests::";
    let mut producer = Process::start(&known, &format!("{prefix}fixture_producer"));
    let mut first = Process::start(&known, &format!("{prefix}fixture_consumer"));
    let mut second = Process::start(&known, &format!("{prefix}fixture_consumer"));
    producer.command("open", "ticket:opened");
    first.command("bind", "ticket:positive counts=1,1,1,1");
    producer.command("refresh", "ticket:refreshed");
    second.command("bind", "ticket:positive counts=1,1,1,1");
    // Independent parent Voice observer confirms actual kernel loss before
    // either child attempts its final dispatch. No state-flag-only witness.
    let observer = owner(&known, 999);
    ready(&known, &observer);
    producer.command("hold-close", "ticket:held");
    let path = crate::speaking_faces::known_speaker_of(&known);
    let source = fs::read(&path).unwrap();
    let value: Value = serde_json::from_slice(&source).unwrap();
    let until = value["permission_until_us"].as_u64().unwrap();
    wait(|| {
        (private_context_state(Some(&known), &observer.lease()) == PermissionState::Lost)
            .then_some(())
    });
    assert!(lamp_ipc::monotonic_us() < until);
    first.command("closed", "ticket:closed counts=1,1,1,1");
    second.command("closed", "ticket:closed counts=1,1,1,1");
    assert!(
        lamp_ipc::monotonic_us() < until,
        "unexpired source must explain denial by kernel close, not timed expiry"
    );
    assert_eq!(
        fs::read(path).unwrap(),
        source,
        "actual Eyes at-lock still holds publication unchanged"
    );
    producer.command("release", "ticket:released");
    first.finish();
    second.finish();
    producer.finish();
}

#[tokio::test]
async fn ordinary_engine_scope_factory_carries_ticket_without_arming_scoped_context() {
    let directory = Directory::new();
    let known = directory.path.join("known");
    fs::write(&known, NAME).unwrap();
    let now = lamp_ipc::monotonic_us();
    fs::write(crate::speaking_faces::known_speaker_of(&known),json!({"version":3,"state":"seen","name":NAME,"id":null,"boot":BOOT,"face":"f7","publisher":"0123456789abcdef0123456789abcdef","generation":1,"camera_hold":1,"camera_began_us":1,"observed_us":now,"permission_until_us":now+1_500_000}).to_string()).unwrap();
    let publisher = [
        0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd,
        0xef,
    ];
    let fixture = crate::camera_authority::fixture::Fixture::start(known.clone(), publisher, 1);
    let cache = fixture.cache();
    let endpoints = Endpoints::start(&directory.path).await;
    let mut engine = Engine::new(
        Arc::new(endpoints.net(&directory.path)),
        Told::default,
        Arc::new(|_, _, _, _| panic!("no tools")),
    )
    .with_memory_scope(
        BootId::new([38; 16]).unwrap(),
        cache.clone(),
        Arc::new(|_, _, _, _, _| panic!("no scoped tools")),
        None,
    );
    assert!(!engine.scopes_context());
    engine.memory_camera(true, 1);
    // Camera startup invalidates earlier cache entries. Observe the actual
    // modeled face after that boundary, before admitting this audio scope.
    assert!(cache.replace(
        cache.begin_refresh(),
        crate::known_faces::FaceObservation::read(&known)
    ));
    let mut scope = engine.memory_scope(1, 1, true).unwrap();
    let person = crate::remember::person_id(NAME);
    assert_eq!(
        scope.lease().scope().and_then(ScopeSnapshot::person),
        Some(person.as_str())
    );
    let key = scope.lease().key();
    assert!(scope.speaker_recorder().unwrap().seal(
        key,
        &[format!("{BOOT}/f7")],
        Instant::now() + Duration::from_secs(3)
    ));
    assert_eq!(fixture.ready(&scope.lease()), PermissionState::Ready);
    assert_eq!(
        private_context_state(Some(&known), &scope.lease()),
        PermissionState::Ready
    );
    // An unconfigured client is policy-off for private disclosure even while
    // this real source/witness still works. No fallback to a legacy file.
    let off_cache = Arc::new(crate::memory_scope::FaceCache::default());
    let mut off = Engine::new(
        Arc::new(endpoints.net(&directory.path)),
        Told::default,
        Arc::new(|_, _, _, _| panic!("no tools")),
    )
    .with_memory_scope(
        BootId::new([39; 16]).unwrap(),
        off_cache.clone(),
        Arc::new(|_, _, _, _, _| panic!("no scoped tools")),
        None,
    );
    off.memory_camera(true, 1);
    // The negative control has the same actual person, but no camera client.
    assert!(off_cache.replace(
        off_cache.begin_refresh(),
        crate::known_faces::FaceObservation::read(&known)
    ));
    let mut unconfigured = off.memory_scope(2, 1, true).unwrap();
    assert_eq!(
        unconfigured.lease().scope().and_then(ScopeSnapshot::person),
        Some(person.as_str())
    );
    let key = unconfigured.lease().key();
    assert!(unconfigured.speaker_recorder().unwrap().seal(
        key,
        &[format!("{BOOT}/f7")],
        Instant::now() + Duration::from_secs(3)
    ));
    assert_eq!(
        private_context_state(Some(&known), &unconfigured.lease()),
        PermissionState::Lost
    );
    let room = engine.memory_scope(3, 1, false).unwrap();
    assert!(room.lease().scope().unwrap().person().is_none());
    let public = PrivateContext::new(&engine.shared, Some(&room.lease()), false);
    assert_eq!(public.state(), PermissionState::Ready);
    fixture.witness.revoke().unwrap();
    assert_eq!(
        private_context_state(Some(&known), &scope.lease()),
        PermissionState::Lost
    );
    assert_eq!(
        public.state(),
        PermissionState::Ready,
        "ordinary Room output is independent of a private camera ticket"
    );
}
