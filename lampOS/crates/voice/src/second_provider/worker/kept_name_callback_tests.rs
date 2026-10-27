//! The worker callback and tool runner share the process's real memory client.
//! Synthetic store replies test propagation, not conflicting-fact ranking or
//! whether a model chooses the newest name when it answers aloud.
use super::{
    HISTORY_EXCHANGES, HISTORY_WITHIN, Sessions, Told, disclosure_message, memory_context,
    system_message,
};
use crate::memory_scope::{FaceCache, RequestKey, RequestScope, ScopeSnapshot};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const CHILD: &str = "LAMP_KEPT_NAME_CALLBACK_TEST_CHILD";
const OWNER: &str = "Synthetic owner";
const OLD: &str = "The synthetic name is Pippa.";
const NEW: &str = "The synthetic name is Tavi.";
/// The longest a note-ahead test here waits on an event (a read's done
/// mark, an open reaching the store): a bound for a faulty client only,
/// never a pace, so a loaded machine changes nothing.
const WAIT: Duration = Duration::from_secs(10);

fn child(name: &str, mode: &str, markers: usize) -> bool {
    if std::env::var(CHILD).ok().as_deref() == Some(mode) {
        return true;
    }
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            &format!("second_provider::worker::kept_name_callback::{name}"),
            "--nocapture",
        ])
        .env_clear()
        .env(CHILD, mode);
    match mode {
        "days" | "days-room" | "privacy" => {
            command.env(crate::remember::DAYS_ENVIRONMENT, "on");
        }
        "days-resumes" => {
            command
                .env(crate::remember::DAYS_ENVIRONMENT, "on")
                .env(crate::remember::JOURNAL_RESUMES_ENVIRONMENT, "on");
        }
        "unrecognized" => {
            command.env(crate::remember::DAYS_ENVIRONMENT, "ON");
        }
        "legacy" => {}
        _ => panic!("unknown fixture mode"),
    }
    let output = command.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "{mode}: {stdout}\n{stderr}"
    );
    assert_eq!(stderr.matches("(#2540)").count(), markers, "{stderr}");
    if name == "privacy::missing_private_source_is_reported_once_without_granting_context" {
        assert_eq!(
            stderr
                .matches("private context unavailable: no readable known-speaker source")
                .count(),
            1,
            "{stderr}"
        );
    }
    for line in stderr
        .lines()
        .filter(|line| line.starts_with("callback check:"))
    {
        eprintln!("{line}");
    }
    false
}

#[derive(Default)]
struct Store {
    person: Option<String>,
    refused: bool,
    kept: String,
    requests: Vec<Value>,
    // A repeated session name must retain its original snapshot, like Days.
    snapshots: BTreeMap<(String, Option<String>), Value>,
    /// The snapshot open, by its count from 0, held until a `release`, and
    /// where it is said that it came ([`Memory::hold_open`]).
    hold_open: Option<(usize, std::sync::mpsc::Sender<()>)>,
}

impl Store {
    /// Take the selected open's receipt; publish only after recording and
    /// installing its held response.
    fn take_hold(&mut self, request: &Value) -> Option<std::sync::mpsc::Sender<()>> {
        let count = self
            .requests
            .iter()
            .filter(|request| request["op"] == "session_open")
            .count();
        let held = request["op"] == "session_open"
            && self
                .hold_open
                .as_ref()
                .is_some_and(|(open, _)| *open == count);
        if held {
            self.hold_open.take().map(|(_, came)| came)
        } else {
            None
        }
    }

    fn answer(&mut self, request: Value) -> Value {
        self.requests.push(request.clone());
        match request["op"].as_str().unwrap() {
            "remember" if self.refused => json!({"ok":false,"error":"empty"}),
            "remember" => {
                self.kept = request["text"].as_str().unwrap().to_owned();
                self.person = request["person"].as_str().map(str::to_owned);
                json!({"ok":true,"id":7})
            }
            "session_open" => {
                let person = request["person"].as_str().map(str::to_owned);
                let key = (
                    request["session"].as_str().unwrap().to_owned(),
                    person.clone(),
                );
                let text = (person == self.person && !self.kept.is_empty()).then_some(&self.kept);
                self.snapshots.entry(key).or_insert_with(|| {
                    json!({"ok":true,"snapshot":{
                        "session":request["session"],"opened_unix_ms":1,"generation":1,
                        "invalidated":false,"source_ids":if text.is_some() { json!([7]) } else { json!([]) },
                        "note":json!({
                            "scope":if person.is_some() { "caller-supplied person; identity not verified" }
                                else { "said in this room; speaker unknown" },
                            "untrusted":true,
                            "facts":text.map_or(json!([]), |text| json!([{"id":7,"kind":"preference","text":text}]))
                        }).to_string()
                    }})
                }).clone()
            }
            "kept" => json!({"ok":true,"entries":[{"text":self.kept,"at_unix_ms":1,"id":7}]}),
            "forget_last" => {
                let removed = u64::from(!self.kept.is_empty());
                self.kept.clear();
                json!({"ok":true,"removed":removed})
            }
            "follow_up" => {
                json!({"ok":true,"follow_up":{"id":99,"category":"thread","text":"Synthetic future question","until_asked":true}})
            }
            "session_close" | "conversation_end" => json!({"ok":true}),
            op => panic!("unexpected memory operation: {op}"),
        }
    }
}

type BeforeReply = Arc<dyn Fn(&Value) + Send + Sync>;

/// A real per-RPC denial closes its client before this fixture replies. Keep
/// serving subsequent requests; only genuine peer disconnects are tolerated.
fn reply(stream: &mut UnixStream, answer: &Value) {
    match writeln!(stream, "{answer}") {
        Ok(()) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::NotConnected
            ) => {}
        Err(error) => panic!("memory fixture response write failed: {error}"),
    }
}

struct Memory {
    _camera_directory: Option<crate::process::SessionDirectory>,
    directory: PathBuf,
    socket: PathBuf,
    state: Arc<Mutex<Store>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
    before_reply: Arc<Mutex<Option<BeforeReply>>>,
}

impl Memory {
    fn new(person: Option<String>) -> Self {
        Self::publishing(person, write_known, None)
    }

    fn new_v3(person: Option<String>) -> Self {
        Self::publishing(
            person,
            write_known_v3,
            Some(crate::process::SessionDirectory::create().unwrap()),
        )
    }

    fn publishing(
        person: Option<String>,
        publish: fn(&std::path::Path, &str, &str, &str),
        camera_directory: Option<crate::process::SessionDirectory>,
    ) -> Self {
        let directory = if let Some(directory) = &camera_directory {
            directory.path.canonicalize().unwrap()
        } else {
            let directory =
                std::env::temp_dir().join(format!("lv-kept-callback-{}", std::process::id()));
            std::fs::create_dir(&directory).unwrap();
            directory
        };
        let socket = directory.join("memory.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        if person.is_some() {
            publish(&directory, OWNER, "0123456789abcdef", "f7");
        }
        let state = Arc::new(Mutex::new(Store {
            person,
            kept: OLD.into(),
            ..Store::default()
        }));
        let serving = state.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let before_reply: Arc<Mutex<Option<BeforeReply>>> = Arc::default();
        let hook = before_reply.clone();
        let worker = std::thread::spawn(move || {
            let mut held: Option<(UnixStream, Value)> = None;
            for stream in listener.incoming() {
                if stopping.load(Ordering::Acquire) {
                    break;
                }
                let mut stream = stream.unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut line = String::new();
                BufReader::new((&stream).take(8192))
                    .read_line(&mut line)
                    .unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                // The held open answered, the store going on meanwhile.
                if request["op"] == "release" {
                    if let Some((mut held, answer)) = held.take() {
                        reply(&mut held, &answer);
                    }
                    reply(&mut stream, &json!({"ok":true}));
                    continue;
                }
                let (receipt, answer) = {
                    let mut store = serving.lock().unwrap();
                    let receipt = store.take_hold(&request);
                    (receipt, store.answer(request.clone()))
                };
                let before = hook.lock().unwrap().clone();
                if let Some(before) = before {
                    before(&request);
                }
                if let Some(receipt) = receipt {
                    held = Some((stream, answer));
                    // The count and held response are both installed before
                    // the observer may begin denial or release assertions.
                    let _ = receipt.send(());
                    continue;
                }
                reply(&mut stream, &answer);
            }
        });
        Self {
            _camera_directory: camera_directory,
            directory,
            socket,
            state,
            stop,
            worker: Some(worker),
            before_reply,
        }
    }

    fn count(&self, op: &str) -> usize {
        self.state
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|request| request["op"] == op)
            .count()
    }

    /// Holds the snapshot open that is the `open`th from 0 until
    /// [`Self::release`]: the store answers everything else meanwhile. The
    /// receiver is notified after accounting and held-response installation.
    fn hold_open(&self, open: usize) -> std::sync::mpsc::Receiver<()> {
        let (came, comes) = std::sync::mpsc::channel();
        self.state.lock().unwrap().hold_open = Some((open, came));
        comes
    }

    /// The held open answered.
    fn release(&self) {
        let mut stream = UnixStream::connect(&self.socket).unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        stream.set_write_timeout(Some(WAIT)).unwrap();
        writeln!(stream, "{}", json!({"op":"release"})).unwrap();
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).unwrap();
    }

    fn settings(&self) -> crate::tools::Settings {
        crate::tools::Settings {
            memory: Some(self.socket.clone()),
            ..Default::default()
        }
    }

    fn held(&self) -> ScopeSnapshot {
        let known = self.directory.join("known");
        std::fs::write(&known, OWNER).unwrap();
        let cache = FaceCache::holding_gaps(Some(Duration::from_secs(12)));
        assert!(cache.replace_seen(
            cache.begin_refresh(),
            crate::known_faces::FaceObservation::read(&known),
            Some(1)
        ));
        let now = Instant::now();
        let seen = cache.snapshot(now);
        assert_eq!(
            seen.person(),
            Some(crate::remember::person_id(OWNER).as_str())
        );
        cache.answered(&seen, now);
        assert!(cache.replace_seen(cache.begin_refresh(), None, Some(0)));
        let held = cache.snapshot(now);
        assert!(held.held());
        held
    }
}

impl Drop for Memory {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = UnixStream::connect(&self.socket);
        self.worker.take().unwrap().join().unwrap();
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}

fn write_known(directory: &std::path::Path, name: &str, boot: &str, face: &str) {
    let known = directory.join("known");
    std::fs::write(&known, name).unwrap();
    std::fs::write(
        crate::speaking_faces::known_speaker_of(&known),
        json!({"state":"seen","publisher":"0123456789abcdef0123456789abcdef","generation":1,
            "observed_us":lamp_ipc::monotonic_us(),
            "permission_until_us":lamp_ipc::monotonic_us().checked_add(1_500_000).unwrap(),"name":name,"id":null,"boot":boot,"face":face}).to_string(),
    )
    .unwrap();
}

/// Explicit V3 publication for the real ticket controls. Capture time and the
/// original 1.5 second source ceiling are sampled where the old fixture did.
fn write_known_v3(directory: &std::path::Path, name: &str, boot: &str, face: &str) {
    let known = directory.join("known");
    std::fs::write(&known, name).unwrap();
    std::fs::write(
        crate::speaking_faces::known_speaker_of(&known),
        json!({"version":3,"state":"seen","publisher":"0123456789abcdef0123456789abcdef","generation":1,
            "camera_hold":1,"camera_began_us":1,
            "observed_us":lamp_ipc::monotonic_us(),
            "permission_until_us":lamp_ipc::monotonic_us().checked_add(1_500_000).unwrap(),"name":name,"id":null,"boot":boot,"face":face}).to_string(),
    )
    .unwrap();
}

fn camera_fixture(memory: &Memory) -> crate::camera_authority::fixture::Fixture {
    let known = memory.directory.join("known");
    let mut name = known.file_name().unwrap().to_os_string();
    name.push(".authority.d");
    let socket = known.parent().unwrap().join(name).join("hold.sock");
    use std::os::unix::ffi::OsStrExt;
    assert!(socket.as_os_str().as_bytes().len() <= 100);
    crate::camera_authority::fixture::Fixture::start(
        known,
        [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
            0xcd, 0xef,
        ],
        1,
    )
}

fn ticket_unproved_scope(
    authority: &crate::camera_authority::fixture::Fixture,
    request: u64,
    snapshot: ScopeSnapshot,
) -> RequestScope {
    RequestScope::looking_later(
        RequestKey {
            worker: lamp_turn_owner::BootId::new([7; 16]).unwrap(),
            session: 1,
            request,
        },
        snapshot,
        authority.cache(),
    )
}

fn ticket_scope(
    authority: &crate::camera_authority::fixture::Fixture,
    request: u64,
    snapshot: ScopeSnapshot,
) -> RequestScope {
    let prove = snapshot.person().is_some() && !snapshot.held();
    let mut owner = ticket_unproved_scope(authority, request, snapshot);
    if prove {
        let key = owner.lease().key();
        assert!(owner.speaker_recorder().unwrap().seal(
            key,
            &["0123456789abcdef/f7".into()],
            Instant::now() + Duration::from_secs(10)
        ));
        // Warm the actual bounded SCM_RIGHTS acquisition, never substitute
        // authority. Negative identity rows still reach the real matcher.
        let _ = authority.ready(&owner.lease());
    }
    owner
}

fn scope(request: u64, snapshot: ScopeSnapshot) -> RequestScope {
    let prove = snapshot.person().is_some() && !snapshot.held();
    let mut owner = unproved_scope(request, snapshot);
    if prove {
        let key = owner.lease().key();
        assert!(owner.speaker_recorder().unwrap().seal(
            key,
            &["0123456789abcdef/f7".into()],
            Instant::now() + Duration::from_secs(10)
        ));
    }
    owner
}

fn unproved_scope(request: u64, snapshot: ScopeSnapshot) -> RequestScope {
    RequestScope::new(
        RequestKey {
            worker: lamp_turn_owner::BootId::new([7; 16]).unwrap(),
            session: 1,
            request,
        },
        snapshot,
    )
}

fn keep(memory: &Memory, scope: &RequestScope, text: &str) -> Value {
    let runner = super::scoped_tool_runner(memory.settings(), None, None, false);
    let lease = scope.lease();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(runner(
            lease.key().request,
            true,
            crate::remember::REMEMBER.into(),
            json!({"text":text}),
            lease,
        ))
}

fn forget(memory: &Memory, scope: &RequestScope) -> Value {
    let runner = super::scoped_tool_runner(memory.settings(), None, None, false);
    let lease = scope.lease();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(runner(
            lease.key().request,
            true,
            crate::remember::FORGET.into(),
            json!({}),
            lease,
        ))
}

fn payload(told: Told) -> String {
    serde_json::to_string(&told.messages()).unwrap()
}

/// The worker's context callback for one turn, with the Days note asked as
/// the engine asks it: as the line begins, then waited for off the loop
/// just before the turn is told (`super::Ahead`). The callback itself only
/// takes the answer.
fn context(
    socket: PathBuf,
) -> impl Fn(Option<&crate::memory_scope::ScopeLease>) -> Told + Send + Sync {
    let sessions = Arc::new(Mutex::new(Sessions::default()));
    let days = crate::remember::days_from_environment();
    let known = socket.parent().unwrap().join("known");
    let ahead =
        days.then(|| super::Ahead::reading(sessions.clone(), Some(known.clone()), socket.clone()));
    let told = memory_context(
        sessions,
        Some(socket),
        Some(known),
        days,
        |_, person, note, _, history| {
            disclosure_message(
                false,
                "Synthetic character.\n",
                None,
                &note,
                None,
                person.as_deref(),
                history,
            )
        },
    );
    move |lease| {
        if let Some(ahead) = &ahead {
            ahead.begin(lease, Instant::now());
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(ahead.clone().ready(lease.cloned()));
        }
        told(lease, super::Chosen::ByLease)
    }
}

/// The second route's turns as the engine drives them, each step its own: a
/// line begins (`super::Ahead::begin`), and only an answered one is waited
/// for and told; a line cancelled before its context is never told. Each
/// read, and each turn's wait for it, may take up to [`WAIT`]: a turn waits
/// on the read's own done mark, never on the lamp's 300 ms.
struct Lines {
    ahead: super::Ahead,
    context: Context,
}

/// The worker's context callback, boxed.
type Context = Box<dyn Fn(Option<&crate::memory_scope::ScopeLease>) -> Told + Send + Sync>;

impl Lines {
    fn new_v3(socket: PathBuf) -> Self {
        let sessions = Arc::new(Mutex::new(Sessions::default()));
        let reading = socket.clone();
        let known = socket.parent().unwrap().join("known");
        let ahead = super::Ahead::asking(
            sessions.clone(),
            Some(known.clone()),
            Arc::new(move |session, person, _, follow_up, permitted| {
                crate::remember::fresh_note_read(
                    &reading, session, person, WAIT, follow_up, permitted,
                )
            }),
        )
        .waiting_up_to(WAIT);
        let context = memory_context(
            sessions,
            Some(socket),
            Some(known),
            true,
            |_, person, note, _, history| {
                disclosure_message(
                    false,
                    "Synthetic character.\n",
                    None,
                    &note,
                    None,
                    person.as_deref(),
                    history,
                )
            },
        );
        let context: Context = Box::new(move |lease| context(lease, super::Chosen::ByLease));
        Self { ahead, context }
    }

    fn begins(&self, scope: &RequestScope) {
        self.observed(scope);
        self.ahead.begin(Some(&scope.lease()), Instant::now());
    }

    fn observed(&self, scope: &RequestScope) {
        if scope.lease().speaker().is_some() {
            write_known_v3(
                self.ahead.who.as_ref().unwrap().parent().unwrap(),
                OWNER,
                "0123456789abcdef",
                "f7",
            );
        }
    }

    /// The line answered: `(contains NEW, contains OLD)` of what it is told.
    fn answered(&self, scope: &RequestScope) -> (bool, bool) {
        self.observed(scope);
        runtime().block_on(self.ahead.clone().ready(Some(scope.lease())));
        let told = payload((self.context)(Some(&scope.lease())));
        (told.contains(NEW), told.contains(OLD))
    }

    /// Final positive request admission started the real read; cancellation
    /// may still precede model context/adoption. Start itself cannot do this.
    fn sealed_read(&self, scope: &RequestScope) {
        assert!(
            self.ahead
                .ready_now(Some(&scope.lease()), Instant::now())
                .is_some()
        );
    }

    /// Whether a read is going on.
    fn reading(&self) -> bool {
        self.ahead
            .sessions
            .lock()
            .unwrap()
            .asked
            .as_ref()
            .is_some_and(super::NoteAsk::reading)
    }

    /// Capture before another line tidies a completed denied ask away.
    fn read_completion(&self) -> tokio::sync::watch::Receiver<bool> {
        self.ahead
            .sessions
            .lock()
            .unwrap()
            .asked
            .as_ref()
            .expect("a read was asked")
            .done
            .clone()
    }

    /// Waits for this exact ask, even if Sessions has already tidied it away.
    fn wait_read_done(&self, mut done: tokio::sync::watch::Receiver<bool>) {
        runtime()
            .block_on(async {
                tokio::time::timeout(WAIT, done.wait_for(|done| *done))
                    .await
                    .map(|said| said.map(|_| ()))
            })
            .expect("the read never ended")
            .expect("its thread said it was done");
        assert!(!self.reading());
    }

    fn read_done(&self) {
        self.wait_read_done(self.read_completion());
    }
}

/// A runtime for one step of [`Lines`].
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// Reviewer-two's reproduction on #3016 (19:19), with the actual keep, the
/// note asked ahead and the worker's context callback: a keep's refresh,
/// asked as a line begins, whose line is cancelled before any context is
/// built, still reaches the person's next turns. Its read takes the kept
/// mark; before, the next line let the finished read go, and both turns
/// after were told the old name, `[(false, true), (false, true)]`.
#[test]
fn review_note_ahead_abandoned_refresh_keeps_the_confirmed_fact() {
    if !child(
        "review_note_ahead_abandoned_refresh_keeps_the_confirmed_fact",
        "days",
        1,
    ) {
        return;
    }
    let memory = Memory::new_v3(Some(crate::remember::person_id(OWNER)));
    let authority = camera_fixture(&memory);
    crate::remember::start(memory.socket.clone(), None);
    let lines = Lines::new_v3(memory.socket.clone());
    let first = ticket_scope(&authority, 1, ScopeSnapshot::of(OWNER));
    lines.begins(&first);
    assert_eq!(lines.answered(&first), (false, true));
    assert_eq!(memory.count("session_open"), 2, "room and admitted person");
    assert_eq!(
        keep(
            &memory,
            &ticket_scope(&authority, 2, ScopeSnapshot::of(OWNER)),
            NEW
        )["remembered"],
        true
    );
    // Canceled before its context: the refresh is read and done first.
    let cancelled = ticket_scope(&authority, 3, ScopeSnapshot::of(OWNER));
    lines.begins(&cancelled);
    lines.sealed_read(&cancelled);
    lines.read_done();
    cancelled.revoke();
    assert_eq!(memory.count("session_open"), 4, "the refresh was read");
    let next: Vec<(bool, bool)> = (4..=5)
        .map(|request| {
            let line = ticket_scope(&authority, request, ScopeSnapshot::of(OWNER));
            lines.begins(&line);
            lines.answered(&line)
        })
        .collect();
    assert_eq!(next, [(true, false), (true, false)]);
    assert_eq!(
        memory.count("session_open"),
        4,
        "the refresh is taken, not read again"
    );
}

/// The read completes after the cancel: still read when the next line
/// begins, it is waited for there and told.
#[test]
fn a_refresh_read_on_past_its_cancelled_line_is_the_next_lines() {
    if !child(
        "a_refresh_read_on_past_its_cancelled_line_is_the_next_lines",
        "days",
        // The denied partial read restores its keep, so the new request
        // logs its own refresh as well as the cancelled request's refresh.
        2,
    ) {
        return;
    }
    let memory = Memory::new_v3(Some(crate::remember::person_id(OWNER)));
    let authority = camera_fixture(&memory);
    crate::remember::start(memory.socket.clone(), None);
    let lines = Lines::new_v3(memory.socket.clone());
    let first = ticket_scope(&authority, 1, ScopeSnapshot::of(OWNER));
    lines.begins(&first);
    assert_eq!(lines.answered(&first), (false, true));
    assert_eq!(
        keep(
            &memory,
            &ticket_scope(&authority, 2, ScopeSnapshot::of(OWNER)),
            NEW
        )["remembered"],
        true
    );
    // The refresh's room open is held at the store until the next line has
    // begun: it is read on past its cancelled line, whatever the timing.
    let came = memory.hold_open(2);
    let cancelled = ticket_scope(&authority, 3, ScopeSnapshot::of(OWNER));
    lines.begins(&cancelled);
    lines.sealed_read(&cancelled);
    came.recv_timeout(WAIT)
        .expect("the refresh's open reached the store");
    let old_done = lines.read_completion();
    cancelled.revoke();
    let next = ticket_scope(&authority, 4, ScopeSnapshot::of(OWNER));
    lines.begins(&next);
    assert!(
        memory.count("session_open") < 4,
        "the refresh is still read as the next line begins"
    );
    memory.release();
    // The cancelled request's Room read must finish denying its private
    // continuation before the next turn gets its own bounded reader.
    lines.wait_read_done(old_done);
    assert_eq!(lines.answered(&next), (true, false));
    let after = ticket_scope(&authority, 5, ScopeSnapshot::of(OWNER));
    lines.begins(&after);
    assert_eq!(lines.answered(&after), (true, false));
    assert_eq!(
        memory.count("session_open"),
        5,
        "two original scopes, cancelled Room only, then two newly proved scopes"
    );
}

/// A finished refresh let go untold (the camera closed or opened before the
/// person's next turn) gives the kept mark back, so the person's next line
/// asks again and is told the new name, though nothing else changed.
#[test]
fn a_refresh_let_go_untold_gives_the_keep_back() {
    if !child("a_refresh_let_go_untold_gives_the_keep_back", "days", 2) {
        return;
    }
    let memory = Memory::new_v3(Some(crate::remember::person_id(OWNER)));
    let authority = camera_fixture(&memory);
    crate::remember::start(memory.socket.clone(), None);
    let lines = Lines::new_v3(memory.socket.clone());
    let first = ticket_scope(&authority, 1, ScopeSnapshot::of(OWNER));
    lines.begins(&first);
    assert_eq!(lines.answered(&first), (false, true));
    assert_eq!(
        keep(
            &memory,
            &ticket_scope(&authority, 2, ScopeSnapshot::of(OWNER)),
            NEW
        )["remembered"],
        true
    );
    let cancelled = ticket_scope(&authority, 3, ScopeSnapshot::of(OWNER));
    lines.begins(&cancelled);
    lines.sealed_read(&cancelled);
    lines.read_done();
    cancelled.revoke();
    lines.ahead.forget();
    let next = ticket_scope(&authority, 4, ScopeSnapshot::of(OWNER));
    lines.begins(&next);
    assert_eq!(lines.answered(&next), (true, false));
    assert_eq!(
        memory.count("session_open"),
        6,
        "asked again for the keep given back"
    );
}

/// Reviewer-two's reproduction on #3016 (20:16): the other side of the
/// boundary [`a_refresh_let_go_untold_gives_the_keep_back`] covers. A keep's
/// refresh, asked as a line begins whose line is cancelled, is voided by a
/// privacy change while it is still read; it ends after. The voided read
/// gives back the keep it took before any note is chosen, so the person's
/// next turns are told the new name, not the old one they were told before
/// (`[(false, true), (false, true)]`).
#[test]
fn review_keep_survives_privacy_void_while_the_refresh_is_reading() {
    if !child(
        "review_keep_survives_privacy_void_while_the_refresh_is_reading",
        "days",
        2,
    ) {
        return;
    }
    let memory = Memory::new_v3(Some(crate::remember::person_id(OWNER)));
    let authority = camera_fixture(&memory);
    crate::remember::start(memory.socket.clone(), None);
    let lines = Lines::new_v3(memory.socket.clone());
    let first = ticket_scope(&authority, 1, ScopeSnapshot::of(OWNER));
    lines.begins(&first);
    assert_eq!(lines.answered(&first), (false, true));
    assert_eq!(
        keep(
            &memory,
            &ticket_scope(&authority, 2, ScopeSnapshot::of(OWNER)),
            NEW
        )["remembered"],
        true
    );
    // The refresh's room open is held at the store until the privacy
    // change has voided it.
    let came = memory.hold_open(2);
    let cancelled = ticket_scope(&authority, 3, ScopeSnapshot::of(OWNER));
    lines.begins(&cancelled);
    lines.sealed_read(&cancelled);
    came.recv_timeout(WAIT)
        .expect("the refresh's open reached the store");
    cancelled.revoke();
    assert!(lines.reading(), "the refresh is still read");
    lines.ahead.forget();
    memory.release();
    lines.read_done();
    let next: Vec<(bool, bool)> = (4..=5)
        .map(|request| {
            let line = ticket_scope(&authority, request, ScopeSnapshot::of(OWNER));
            lines.begins(&line);
            lines.answered(&line)
        })
        .collect();
    assert_eq!(next, [(true, false), (true, false)]);
}

/// Reviewer-two's reproduction on #3016 (20:22), with journalling resumed
/// by a fresh session (`LAMP_MEMORY_JOURNAL_RESUMES`): the person's note,
/// cached with the old name, is forgotten in the store by an actual forget;
/// the resume read for their next line is held at the store until a second
/// forget is sent, so its answer is from before that forget; the fresh
/// read then backs off, and nothing kept asks for another. Their next turn
/// is told neither the forgotten old name nor anything read before the
/// forgets: once the store may have changed, the cached note and the
/// rejected read are both let go, whatever the pacing. Before, it was told
/// the old name, `(false, true)`.
#[test]
fn review_an_invalid_resume_read_never_returns_the_forgotten_cached_note() {
    if !child(
        "review_an_invalid_resume_read_never_returns_the_forgotten_cached_note",
        "days-resumes",
        0,
    ) {
        return;
    }
    let memory = Memory::new_v3(Some(crate::remember::person_id(OWNER)));
    let authority = camera_fixture(&memory);
    crate::remember::start(memory.socket.clone(), None);
    let lines = Lines::new_v3(memory.socket.clone());
    let first = ticket_scope(&authority, 1, ScopeSnapshot::of(OWNER));
    lines.begins(&first);
    assert_eq!(
        lines.answered(&first),
        (false, true),
        "the old name is cached"
    );
    assert_eq!(
        forget(
            &memory,
            &ticket_scope(&authority, 2, ScopeSnapshot::of(OWNER))
        )["forgotten"],
        true
    );
    assert!(
        crate::remember::wants_fresh_session(),
        "journalling stopped: a resume read is due"
    );
    let came = memory.hold_open(2);
    let reading = ticket_scope(&authority, 3, ScopeSnapshot::of(OWNER));
    lines.begins(&reading);
    lines.sealed_read(&reading);
    came.recv_timeout(WAIT)
        .expect("the read's open reached the store");
    assert!(lines.reading(), "a read is in flight");
    // Its send alone changes what a read may hold.
    let _ = forget(
        &memory,
        &ticket_scope(&authority, 4, ScopeSnapshot::of(OWNER)),
    );
    memory.release();
    lines.read_done();
    {
        let sessions = lines.ahead.sessions.lock().unwrap();
        let asked = sessions.asked.as_ref().expect("the read is kept");
        assert!(
            asked.complete() && !asked.current(),
            "read before the second forget"
        );
        assert!(
            !sessions.note.as_deref().unwrap_or_default().contains(OLD),
            "the cached old name went when the store changed"
        );
    }
    assert!(
        !crate::remember::wants_fresh_session(),
        "resume pacing backs off"
    );
    assert!(
        !crate::remember::kept_since_note_peek(),
        "no keep asks for it"
    );
    let next = ticket_scope(&authority, 5, ScopeSnapshot::of(OWNER));
    lines.begins(&next);
    assert_eq!(lines.answered(&next), (false, false));
}

fn evict_name(person: Option<&str>) {
    let now = Instant::now();
    let start = now - Duration::from_secs(368);
    let mut thread = crate::thread::Thread::new();
    thread.at_lamp(person.map(str::to_owned));
    thread.person(NEW, true, start);
    thread.lamp(1, "Noted.", start);
    assert!(
        thread
            .recall_for(person, start, usize::MAX)
            .unwrap()
            .contains(NEW)
    );
    for request in 2..=9 {
        let at = start + Duration::from_secs((request - 1) * 46);
        thread.person("A synthetic follow-up.", true, at);
        thread.lamp(request, "A synthetic answer.", at);
    }
    let history = thread.recent_for(person, now, HISTORY_WITHIN, HISTORY_EXCHANGES);
    assert_eq!(history.len(), HISTORY_EXCHANGES);
    assert!(!format!("{history:?}").contains(NEW));
    assert!(
        !thread
            .recall_for(person, now, usize::MAX)
            .unwrap()
            .contains(NEW)
    );
    *crate::thread::THREAD.lock().unwrap() = thread;
    assert!(!payload(system_message("", None, "", None, person)).contains(NEW));
}

#[test]
fn a_confirmed_keep_reaches_the_next_context_and_outlives_both_histories() {
    if !child(
        "a_confirmed_keep_reaches_the_next_context_and_outlives_both_histories",
        "days",
        1,
    ) {
        return;
    }
    let person = crate::remember::person_id(OWNER);
    let memory = Memory::new_v3(Some(person.clone()));
    let authority = camera_fixture(&memory);
    crate::remember::start(memory.socket.clone(), None);
    assert!(
        crate::tools::declared(&memory.settings())
            .iter()
            .any(|tool| tool.name == crate::remember::REMEMBER)
    );
    let context = context(memory.socket.clone());
    let admitted = ticket_scope(&authority, 1, ScopeSnapshot::of(OWNER));
    let first = Instant::now();
    let first_payload = payload(context(Some(&admitted.lease())));
    let first_us = first.elapsed().as_micros();
    assert!(first_payload.contains(OLD));
    assert_eq!(memory.count("session_open"), 2, "room and admitted person");

    memory.state.lock().unwrap().refused = true;
    let refused = ticket_scope(&authority, 2, ScopeSnapshot::of(OWNER));
    let answer = keep(&memory, &refused, "A synthetic refused fact.");
    assert!(answer.get("error").is_some());
    assert_ne!(answer["remembered"], true);
    assert!(payload(context(Some(&refused.lease()))).contains(OLD));
    assert_eq!(
        memory.count("session_open"),
        2,
        "a refusal does not refresh"
    );
    memory.state.lock().unwrap().refused = false;
    let kept = ticket_scope(&authority, 3, ScopeSnapshot::of(OWNER));
    assert_eq!(keep(&memory, &kept, NEW)["remembered"], true);

    let revoked = ticket_scope(&authority, 4, ScopeSnapshot::of(OWNER));
    revoked.revoke();
    assert_eq!(context(Some(&revoked.lease())), Told::default());
    assert_ne!(
        keep(&memory, &revoked, "A revoked synthetic fact.")["remembered"],
        true
    );
    assert_eq!(
        memory.count("remember"),
        2,
        "revocation suppresses dispatch"
    );
    let held = ticket_scope(&authority, 5, memory.held());
    let held_payload = payload(context(Some(&held.lease())));
    assert!(!held_payload.contains(OLD) && !held_payload.contains(NEW));
    assert_eq!(
        memory.count("session_open"),
        2,
        "held and revoked contexts do not consume the keep"
    );

    let next = ticket_scope(&authority, 6, ScopeSnapshot::of(OWNER));
    let refresh = Instant::now();
    let refreshed = payload(context(Some(&next.lease())));
    let refresh_us = refresh.elapsed().as_micros();
    assert!(
        refreshed.contains(NEW),
        "the actual callback must consume the keep and open a new snapshot"
    );
    assert!(
        !refreshed.contains(OLD),
        "the fixture's replacement snapshot is used"
    );
    assert_eq!(memory.count("session_open"), 4);
    let cached = Instant::now();
    let cached_payload = payload(context(Some(&next.lease())));
    let cached_us = cached.elapsed().as_micros();
    assert_eq!(cached_payload, refreshed);
    assert_eq!(
        memory.count("session_open"),
        4,
        "repeated callback asks no memory I/O"
    );
    assert_eq!(
        memory.state.lock().unwrap().snapshots.len(),
        4,
        "both scopes have a new unique session"
    );

    evict_name(Some(&person));
    let later = ticket_scope(&authority, 7, ScopeSnapshot::of(OWNER));
    let told = context(Some(&later.lease()));
    assert_eq!(told.history.len(), HISTORY_EXCHANGES * 2);
    assert!(
        payload(told).contains(NEW),
        "the later payload gets the name from memory alone"
    );
    assert_eq!(
        memory.count("session_open"),
        4,
        "no quiet or identity change masks a missing keep refresh"
    );
    eprintln!(
        "callback check: mode=days-person boundary=callback+messages_json first_us={first_us} refresh_us={refresh_us} cached_us={cached_us} opens=4 remembers=2"
    );
}

#[test]
fn a_room_keep_reaches_the_next_context_without_a_person_change() {
    if !child(
        "a_room_keep_reaches_the_next_context_without_a_person_change",
        "days-room",
        1,
    ) {
        return;
    }
    let memory = Memory::new(None);
    crate::remember::start(memory.socket.clone(), None);
    let context = context(memory.socket.clone());
    let admitted = scope(1, ScopeSnapshot::room());
    let first = Instant::now();
    let first_payload = payload(context(Some(&admitted.lease())));
    let first_us = first.elapsed().as_micros();
    assert!(first_payload.contains(OLD));
    assert_eq!(memory.count("session_open"), 1);
    assert_eq!(keep(&memory, &admitted, NEW)["remembered"], true);

    let next = scope(2, ScopeSnapshot::room());
    let refresh = Instant::now();
    let refreshed = payload(context(Some(&next.lease())));
    let refresh_us = refresh.elapsed().as_micros();
    assert!(
        refreshed.contains(NEW),
        "the room callback must consume the keep without a person change"
    );
    assert!(!refreshed.contains(OLD));
    assert_eq!(memory.count("session_open"), 2);
    let cached = Instant::now();
    let cached_payload = payload(context(Some(&next.lease())));
    let cached_us = cached.elapsed().as_micros();
    assert_eq!(cached_payload, refreshed);
    assert_eq!(memory.count("session_open"), 2);
    assert_eq!(memory.state.lock().unwrap().snapshots.len(), 2);

    evict_name(None);
    let later = scope(3, ScopeSnapshot::room());
    let told = context(Some(&later.lease()));
    assert_eq!(told.history.len(), HISTORY_EXCHANGES * 2);
    assert!(payload(told).contains(NEW));
    assert_eq!(memory.count("session_open"), 2);
    assert!(
        memory
            .state
            .lock()
            .unwrap()
            .requests
            .iter()
            .all(|request| request["person"].is_null()),
        "every operation stays in the room scope"
    );
    eprintln!(
        "callback check: mode=days-room boundary=callback+messages_json first_us={first_us} refresh_us={refresh_us} cached_us={cached_us} opens=2 remembers=1"
    );
}

#[test]
fn legacy_keeps_update_the_callback_without_a_days_marker() {
    if std::env::var(CHILD).is_err() {
        assert!(!child(
            "legacy_keeps_update_the_callback_without_a_days_marker",
            "legacy",
            0
        ));
        assert!(!child(
            "legacy_keeps_update_the_callback_without_a_days_marker",
            "unrecognized",
            0
        ));
        return;
    }
    assert!(!crate::remember::days_from_environment());
    let memory = Memory::new(None);
    crate::remember::start(memory.socket.clone(), None);
    let context = context(memory.socket.clone());
    let admitted = scope(1, ScopeSnapshot::room());
    assert!(payload(context(Some(&admitted.lease()))).contains(OLD));
    let before = memory.count("kept");
    let refreshed = Instant::now();
    assert_eq!(keep(&memory, &admitted, NEW)["remembered"], true);
    // Legacy refresh is queued, not synchronous. Observe its application;
    // a callback immediately after the keep is allowed the preceding note.
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if payload(context(Some(&admitted.lease()))).contains(NEW) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "legacy queued refresh was not applied"
        );
        std::thread::yield_now();
    }
    let refresh_us = refreshed.elapsed().as_micros();
    assert!(memory.count("kept") > before);
    assert_eq!(memory.count("session_open"), 0);
    assert!(!crate::remember::kept_since_note());
    evict_name(None);
    assert!(payload(context(Some(&admitted.lease()))).contains(NEW));
    eprintln!(
        "callback check: mode=legacy keep_to_refreshed_payload_us={refresh_us} opens=0 remembers=1"
    );
}

#[path = "kept_name_privacy_tests.rs"]
mod privacy;

/// Restore the real process thread after local socket controls, including a
/// panic: an explicitly private fixture must not leak into a later Room test.
struct LocalThread(Option<crate::thread::Thread>);
impl LocalThread {
    fn take() -> Self {
        Self(Some(std::mem::take(
            &mut *crate::thread::THREAD.lock().unwrap(),
        )))
    }
}
impl Drop for LocalThread {
    fn drop(&mut self) {
        *crate::thread::THREAD.lock().unwrap() = self.0.take().unwrap();
    }
}

/// Local client controls: real Days sockets, no child-global environment or
/// remembered callback substitute. The request uses the worker's actual Ahead.
fn local_ahead(memory: &Memory) -> (super::Ahead, Context) {
    let client = Arc::new(crate::remember::DaysClient::new(memory.socket.clone()).unwrap());
    client.following_up();
    local_ahead_asking(
        memory,
        Arc::new(move |session, person, within, follow_up, permitted| {
            client.read_note_permitted(
                session,
                person,
                false,
                within,
                (true, follow_up, &permitted),
            )
        }),
    )
}

fn local_ahead_asking(memory: &Memory, ask: super::AskNote) -> (super::Ahead, Context) {
    let sessions = Arc::new(Mutex::new(Sessions::default()));
    let known = memory.directory.join("known");
    let ahead =
        super::Ahead::asking(sessions.clone(), Some(known.clone()), ask).waiting_up_to(WAIT);
    let context = memory_context(
        sessions,
        Some(memory.socket.clone()),
        Some(known),
        true,
        |_, _, note, _, _| Told::from(note),
    );
    let context: Context = Box::new(move |lease| context(lease, super::Chosen::ByLease));
    (ahead, context)
}

fn wait_note(ahead: &super::Ahead) {
    let mut done = ahead
        .sessions
        .lock()
        .unwrap()
        .asked
        .as_ref()
        .unwrap()
        .done
        .clone();
    runtime().block_on(async {
        tokio::time::timeout(WAIT, done.wait_for(|done| *done))
            .await
            .unwrap()
            .unwrap();
    });
}

#[test]
fn private_note_start_waits_for_sealed_proof_and_room_is_distinct_from_held() {
    let _one = crate::provider_worker::tests::one_thread();
    let _thread = LocalThread::take();
    let memory = Memory::new_v3(Some(crate::remember::person_id(OWNER)));
    let authority = camera_fixture(&memory);
    let (ahead, context) = local_ahead(&memory);
    // Even a prior private answer is no authority for this new microphone piece.
    ahead.sessions.lock().unwrap().answered = Some(Some(crate::remember::person_id(OWNER)));
    let mut owner = ticket_unproved_scope(&authority, 1, ScopeSnapshot::of(OWNER));
    ahead.begin(Some(&owner.lease()), Instant::now());
    runtime().block_on(ahead.clone().ready(Some(owner.lease())));
    assert_eq!(memory.count("session_open"), 0);
    assert!(!payload(context(Some(&owner.lease()))).contains(OLD));
    let key = owner.lease().key();
    assert!(owner.speaker_recorder().unwrap().seal(
        key,
        &["0123456789abcdef/f7".into()],
        Instant::now() + WAIT
    ));
    assert_eq!(
        authority.ready(&owner.lease()),
        super::PermissionState::Ready
    );
    runtime().block_on(ahead.clone().ready(Some(owner.lease())));
    let told = context(Some(&owner.lease()));
    assert!(told.requires_speaker && payload(told).contains(OLD));
    assert_eq!(
        memory.count("session_open"),
        2,
        "one note read, room plus person"
    );
    assert_eq!(
        memory.count("follow_up"),
        1,
        "the proved final turn spends one offer"
    );
    let next = ticket_scope(&authority, 2, ScopeSnapshot::of(OWNER));
    runtime().block_on(ahead.clone().ready(Some(next.lease())));
    assert!(payload(context(Some(&next.lease()))).contains(OLD));
    assert_eq!(
        memory.count("session_open"),
        2,
        "same-person cache reused with new proof"
    );
    let held = ticket_unproved_scope(&authority, 3, ScopeSnapshot::held_of(OWNER));
    runtime().block_on(ahead.clone().ready(Some(held.lease())));
    assert!(!payload(context(Some(&held.lease()))).contains(OLD));
    let narrowed = ticket_scope(&authority, 4, ScopeSnapshot::of(OWNER));
    narrowed.lease().narrow_to_room();
    runtime().block_on(ahead.clone().ready(Some(narrowed.lease())));
    assert!(!payload(context(Some(&narrowed.lease()))).contains(OLD));
    assert_eq!(
        memory.count("session_open"),
        2,
        "held/narrowed private cannot read Room"
    );
    let room = ticket_unproved_scope(&authority, 5, ScopeSnapshot::room());
    runtime().block_on(ahead.clone().ready(Some(room.lease())));
    assert!(!context(Some(&room.lease())).requires_speaker);
    assert_eq!(
        memory.count("session_open"),
        3,
        "original unknown Room has its own read"
    );
    assert_eq!(memory.count("follow_up"), 1, "Room spends no private offer");
}

#[test]
fn private_note_queued_reader_checks_real_permission_before_any_socket() {
    let _one = crate::provider_worker::tests::one_thread();
    let _thread = LocalThread::take();
    for lose in [false, true] {
        let memory = Memory::new_v3(Some(crate::remember::person_id(OWNER)));
        let authority = camera_fixture(&memory);
        let (ahead, _) = local_ahead(&memory);
        let owner = ticket_scope(&authority, 1, ScopeSnapshot::of(OWNER));
        let permission = ahead
            .permission(
                Some(&owner.lease()),
                Some(&crate::remember::person_id(OWNER)),
            )
            .unwrap();
        let (entered, entry) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let released = Mutex::new(released);
        let first = AtomicBool::new(true);
        let permission = Arc::new(move || {
            if first.swap(false, Ordering::AcqRel) {
                entered.send(()).unwrap();
                released.lock().unwrap().recv_timeout(WAIT).unwrap();
            }
            // Subsequent per-RPC checks use the same real matcher, without
            // re-entering a scheduling barrier intended only for first entry.
            permission()
        });
        let ask = super::NoteAsk::start(
            1,
            Some(crate::remember::person_id(OWNER)),
            (Instant::now(), 1),
            &ahead.ask,
            false,
            permission,
        );
        let source = crate::speaking_faces::known_speaker_of(&memory.directory.join("known"));
        let original_source = std::fs::read(&source).unwrap();
        entry.recv_timeout(WAIT).unwrap();
        if lose {
            authority.witness.revoke().unwrap();
        }
        assert_eq!(std::fs::read(&source).unwrap(), original_source);
        assert!(
            owner.lease().is_current(),
            "loss is fresh speaker evidence, not revocation"
        );
        release.send(()).unwrap();
        let mut done = ask.done.clone();
        runtime().block_on(async {
            tokio::time::timeout(WAIT, done.wait_for(|done| *done))
                .await
                .unwrap()
                .unwrap();
        });
        assert_eq!(memory.count("session_open"), if lose { 0 } else { 2 });
        assert_eq!(
            memory.count("follow_up"),
            0,
            "private read stages its offer until checked ready"
        );
        assert_eq!(ask.denied.load(Ordering::Acquire), lose);
        let mut sessions = ahead.sessions.lock().unwrap();
        sessions.asked = Some(ask);
        sessions.tidy();
        assert_eq!(
            sessions.failures, 0,
            "permission loss is not a failed memory read"
        );
        assert!(sessions.note_retry_at.is_none());
        assert_eq!(sessions.asked.is_none(), lose);
    }
}

#[test]
fn private_note_slow_real_read_cannot_spend_offer_or_disclose_after_proof_loss() {
    let _one = crate::provider_worker::tests::one_thread();
    let _thread = LocalThread::take();
    for lose in [false, true] {
        let memory = Memory::new_v3(Some(crate::remember::person_id(OWNER)));
        let authority = camera_fixture(&memory);
        let (ahead, context) = local_ahead(&memory);
        let owner = ticket_scope(&authority, 1, ScopeSnapshot::of(OWNER));
        let arrived = memory.hold_open(0);
        runtime().block_on(async {
            let waiting = tokio::spawn(ahead.clone().ready(Some(owner.lease())));
            tokio::task::spawn_blocking(move || arrived.recv_timeout(WAIT).unwrap())
                .await
                .unwrap();
            if lose {
                std::fs::remove_file(crate::speaking_faces::known_speaker_of(
                    &memory.directory.join("known"),
                ))
                .unwrap();
            }
            assert!(owner.lease().is_current());
            memory.release();
            tokio::time::timeout(WAIT, waiting).await.unwrap().unwrap();
        });
        let told = context(Some(&owner.lease()));
        assert_eq!(payload(told).contains(OLD), !lose);
        // The held response was the original Room open. Completion above
        // observes the whole reader, so this counts complete parsed private
        // requests, not an absence poll before a possible late dispatch.
        let private_opens = memory
            .state
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|request| request["op"] == "session_open" && request["person"].is_string())
            .count();
        assert_eq!(
            (memory.count("session_open"), private_opens),
            if lose { (1, 0) } else { (2, 1) },
            "proof loss after Room reply must stop the next private subdispatch"
        );
        assert_eq!(
            memory.count("follow_up"),
            usize::from(!lose),
            "no offer after slow read loses proof"
        );
        if lose {
            // Neither a fresh different face nor a narrowed original private
            // admission may reuse this person's body as Room context.
            write_known_v3(&memory.directory, OWNER, "0123456789abcdef", "f8");
            let different = ticket_scope(&authority, 2, ScopeSnapshot::of(OWNER));
            runtime().block_on(ahead.clone().ready(Some(different.lease())));
            assert!(!payload(context(Some(&different.lease()))).contains(OLD));
            let narrowed = ticket_scope(&authority, 3, ScopeSnapshot::of(OWNER));
            narrowed.lease().narrow_to_room();
            runtime().block_on(ahead.clone().ready(Some(narrowed.lease())));
            assert!(!payload(context(Some(&narrowed.lease()))).contains(OLD));
            assert_eq!(memory.count("session_open"), 1);
            write_known_v3(&memory.directory, OWNER, "0123456789abcdef", "f7");
            let next = ticket_scope(&authority, 4, ScopeSnapshot::of(OWNER));
            runtime().block_on(ahead.clone().ready(Some(next.lease())));
            let told = context(Some(&next.lease()));
            assert!(
                told.requires_speaker && payload(told).contains(OLD),
                "new same-person proof reads the private body that was never dispatched"
            );
            assert_eq!(
                memory.count("session_open"),
                3,
                "a fresh proved request reads exactly one new two-scope note"
            );
            assert_eq!(memory.count("follow_up"), 1);
        }
    }
}

#[test]
fn private_note_blocking_follow_up_rechecks_after_queue_and_consumes_once() {
    let _one = crate::provider_worker::tests::one_thread();
    let _thread = LocalThread::take();
    for lose in [false, true] {
        let memory = Memory::new_v3(Some(crate::remember::person_id(OWNER)));
        let authority = camera_fixture(&memory);
        let (ahead, context) = local_ahead(&memory);
        let owner = ticket_scope(&authority, 1, ScopeSnapshot::of(OWNER));
        assert!(
            ahead
                .ready_now(Some(&owner.lease()), Instant::now())
                .is_some()
        );
        wait_note(&ahead);
        assert_eq!(memory.count("follow_up"), 0);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()
            .unwrap();
        let (blocked, blocking) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        runtime.block_on(async {
            let blocker = tokio::task::spawn_blocking(move || {
                blocked.send(()).unwrap();
                released.recv_timeout(WAIT).unwrap();
            });
            blocking.recv_timeout(WAIT).unwrap();
            let ready = ahead.clone().ready(Some(owner.lease()));
            tokio::pin!(ready);
            let first = std::future::poll_fn(|cx| {
                std::task::Poll::Ready(std::future::Future::poll(ready.as_mut(), cx))
            })
            .await;
            assert!(
                first.is_pending(),
                "follow-up is actually queued behind the blocking worker"
            );
            if lose {
                std::fs::remove_file(crate::speaking_faces::known_speaker_of(
                    &memory.directory.join("known"),
                ))
                .unwrap();
            }
            assert!(owner.lease().is_current());
            release.send(()).unwrap();
            tokio::time::timeout(WAIT, ready).await.unwrap();
            blocker.await.unwrap();
        });
        assert_eq!(memory.count("follow_up"), usize::from(!lose));
        assert_eq!(payload(context(Some(&owner.lease()))).contains(OLD), !lose);
        if lose {
            write_known_v3(&memory.directory, OWNER, "0123456789abcdef", "f7");
            let next = ticket_scope(&authority, 2, ScopeSnapshot::of(OWNER));
            runtime.block_on(ahead.clone().ready(Some(next.lease())));
            assert!(payload(context(Some(&next.lease()))).contains(OLD));
            assert_eq!(
                memory.count("follow_up"),
                1,
                "new proved turn consumes the still-staged offer exactly once"
            );
        }
    }
}

#[test]
fn private_note_session_lock_wait_rechecks_proof_before_read_entry() {
    let _one = crate::provider_worker::tests::one_thread();
    let _thread = LocalThread::take();
    let memory = Memory::new_v3(Some(crate::remember::person_id(OWNER)));
    let authority = camera_fixture(&memory);
    let (ahead, _) = local_ahead(&memory);
    let owner = ticket_scope(&authority, 1, ScopeSnapshot::of(OWNER));
    let locked = ahead.sessions.lock().unwrap();
    let (entered, entry) = std::sync::mpsc::channel();
    let queued = ahead.clone();
    let lease = owner.lease();
    let (done, ended) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        entered.send(()).unwrap();
        done.send(queued.ready_now(Some(&lease), Instant::now()).is_some())
            .unwrap();
    });
    entry.recv_timeout(WAIT).unwrap();
    std::fs::remove_file(crate::speaking_faces::known_speaker_of(
        &memory.directory.join("known"),
    ))
    .unwrap();
    assert!(owner.lease().is_current());
    drop(locked);
    assert!(!ended.recv_timeout(WAIT).unwrap());
    worker.join().unwrap();
    assert_eq!(memory.count("session_open"), 0);
}

#[test]
fn staged_private_note_cannot_cross_person_or_reused_face_from_another_boot() {
    let _one = crate::provider_worker::tests::one_thread();
    let _thread = LocalThread::take();
    for change in ["person", "boot"] {
        let memory = Memory::new_v3(Some(crate::remember::person_id(OWNER)));
        let authority = camera_fixture(&memory);
        let (ahead, context) = local_ahead(&memory);
        let owner = ticket_scope(&authority, 1, ScopeSnapshot::of(OWNER));
        assert!(
            ahead
                .ready_now(Some(&owner.lease()), Instant::now())
                .is_some()
        );
        wait_note(&ahead);
        assert_eq!(memory.count("session_open"), 2);
        let next = if change == "person" {
            write_known_v3(
                &memory.directory,
                "Synthetic other",
                "0123456789abcdef",
                "f7",
            );
            ticket_scope(&authority, 2, ScopeSnapshot::of("Synthetic other"))
        } else {
            write_known_v3(&memory.directory, OWNER, "fedcba9876543210", "f7");
            ticket_scope(&authority, 2, ScopeSnapshot::of(OWNER))
        };
        runtime().block_on(ahead.clone().ready(Some(next.lease())));
        assert!(!payload(context(Some(&next.lease()))).contains(OLD));
        assert_eq!(
            memory.count("session_open"),
            if change == "person" { 4 } else { 2 },
            "new person reads only their own; an old boot's proof authorizes no read"
        );
        assert_eq!(memory.count("follow_up"), usize::from(change == "person"));
    }
}

#[test]
fn private_note_same_request_cannot_revive_after_ready_observed_proof_loss() {
    let _one = crate::provider_worker::tests::one_thread();
    let _thread = LocalThread::take();
    let memory = Memory::new_v3(Some(crate::remember::person_id(OWNER)));
    let authority = camera_fixture(&memory);
    let (ahead, context) = local_ahead(&memory);
    let owner = ticket_scope(&authority, 1, ScopeSnapshot::of(OWNER));
    // The private request has already reached the real store under valid
    // proof. Hold its response, not the Room response used by the other test.
    let arrived = memory.hold_open(1);
    runtime().block_on(async {
        let waiting = tokio::spawn(ahead.clone().ready(Some(owner.lease())));
        tokio::task::spawn_blocking(move || arrived.recv_timeout(WAIT).unwrap())
            .await
            .unwrap();
        assert_eq!(
            memory.count("session_open"),
            2,
            "Room and private read really dispatched"
        );
        std::fs::remove_file(crate::speaking_faces::known_speaker_of(
            &memory.directory.join("known"),
        ))
        .unwrap();
        assert!(owner.lease().is_current(), "the lease is not revoked");
        memory.release();
        // Completion here means ready checked the actually missing source
        // after its awaited read and returned without spending the offer.
        tokio::time::timeout(WAIT, waiting).await.unwrap().unwrap();
    });
    assert_eq!(memory.count("follow_up"), 0);
    assert!(owner.lease().is_current());
    write_known_v3(&memory.directory, OWNER, "0123456789abcdef", "f7");
    assert_eq!(
        owner.lease().speaker(),
        Some("0123456789abcdef/f7"),
        "restoration really matches this still-current original sealed request"
    );
    let revived = context(Some(&owner.lease()));
    assert!(
        !payload(revived).contains(OLD),
        "the same request must not revive private adoption after observed proof loss"
    );
    assert_eq!(
        memory.count("session_open"),
        2,
        "same-request restoration performs no new read"
    );
    assert_eq!(
        memory.count("follow_up"),
        0,
        "same-request restoration spends no offer"
    );
    let next = ticket_scope(&authority, 2, ScopeSnapshot::of(OWNER));
    runtime().block_on(ahead.clone().ready(Some(next.lease())));
    let told = context(Some(&next.lease()));
    assert!(
        told.requires_speaker && payload(told).contains(OLD),
        "a new positively proved same-person request reads after the interrupted private response"
    );
    assert_eq!(
        memory.count("session_open"),
        4,
        "the new request reads exactly one fresh two-scope note"
    );
    assert_eq!(
        memory.count("follow_up"),
        1,
        "only the new proved request spends one offer"
    );
}

#[test]
fn lost_private_read_permission_does_not_revoke_an_ordinary_remember_write() {
    if !child(
        "lost_private_read_permission_does_not_revoke_an_ordinary_remember_write",
        "privacy",
        0,
    ) {
        return;
    }
    let memory = Memory::new_v3(Some(crate::remember::person_id(OWNER)));
    let authority = camera_fixture(&memory);
    crate::remember::start(memory.socket.clone(), None);
    let owner = ticket_scope(&authority, 1, ScopeSnapshot::of(OWNER));
    let known = memory.directory.join("known");
    assert!(super::private_context_current(Some(&known), &owner.lease()));
    std::fs::remove_file(crate::speaking_faces::known_speaker_of(&known)).unwrap();
    assert!(!super::private_context_current(
        Some(&known),
        &owner.lease()
    ));
    assert!(owner.lease().private_context_lost());
    write_known_v3(&memory.directory, OWNER, "0123456789abcdef", "f7");
    assert!(
        !super::private_context_current(Some(&known), &owner.lease()),
        "private reads stay lost"
    );
    assert!(owner.lease().is_current());
    assert_eq!(
        owner.lease().scope().unwrap().person(),
        Some(crate::remember::person_id(OWNER).as_str())
    );
    assert_eq!(owner.lease().speaker(), Some("0123456789abcdef/f7"));
    assert_eq!(
        keep(&memory, &owner, NEW)["remembered"],
        true,
        "the actual non-read tool path retains its original scope permission"
    );
    assert_eq!(memory.count("remember"), 1);
    assert_eq!(memory.count("session_open"), 0);
}

#[test]
fn completed_private_read_needs_new_request_proof_after_ready_observed_loss() {
    let _one = crate::provider_worker::tests::one_thread();
    let _thread = LocalThread::take();
    let memory = Memory::new_v3(Some(crate::remember::person_id(OWNER)));
    let authority = camera_fixture(&memory);
    let client = Arc::new(crate::remember::DaysClient::new(memory.socket.clone()).unwrap());
    client.following_up();
    let (completed, complete) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let released = Mutex::new(released);
    let first = AtomicBool::new(true);
    let (ahead, context) = local_ahead_asking(
        &memory,
        Arc::new(move |session, person, within, follow_up, permitted| {
            let read = client.read_note_permitted(
                session,
                person,
                false,
                within,
                (true, follow_up, &permitted),
            );
            if first.swap(false, Ordering::AcqRel) {
                // Both complete parsed responses returned under valid permission.
                // The barrier is strictly after the real client, before NoteAsk's
                // answer slot/publication and Ahead's post-wait adoption check.
                assert!(read.complete());
                completed.send(()).unwrap();
                released.lock().unwrap().recv_timeout(WAIT).unwrap();
            }
            read
        }),
    );
    let owner = ticket_scope(&authority, 1, ScopeSnapshot::of(OWNER));
    runtime().block_on(async {
        let waiting = tokio::spawn(ahead.clone().ready(Some(owner.lease())));
        tokio::task::spawn_blocking(move || complete.recv_timeout(WAIT).unwrap())
            .await
            .unwrap();
        assert_eq!(memory.count("session_open"), 2);
        std::fs::remove_file(crate::speaking_faces::known_speaker_of(
            &memory.directory.join("known"),
        ))
        .unwrap();
        assert!(owner.lease().is_current());
        release.send(()).unwrap();
        tokio::time::timeout(WAIT, waiting).await.unwrap().unwrap();
    });
    assert!(owner.lease().private_context_lost());
    assert_eq!(memory.count("follow_up"), 0);
    write_known_v3(&memory.directory, OWNER, "0123456789abcdef", "f7");
    assert!(
        !payload(context(Some(&owner.lease()))).contains(OLD),
        "observed loss remains sticky even though the completed body can stand for a new turn"
    );
    let next = ticket_scope(&authority, 2, ScopeSnapshot::of(OWNER));
    runtime().block_on(ahead.clone().ready(Some(next.lease())));
    let told = context(Some(&next.lease()));
    assert!(told.requires_speaker && payload(told).contains(OLD));
    assert_eq!(
        memory.count("session_open"),
        2,
        "a complete same-person read is reused under NEW actual proof"
    );
    assert_eq!(memory.count("follow_up"), 1);
}

#[test]
fn new_request_already_waiting_retries_after_the_old_reader_actually_denies() {
    let _one = crate::provider_worker::tests::one_thread();
    let _thread = LocalThread::take();
    let memory = Memory::new_v3(Some(crate::remember::person_id(OWNER)));
    let authority = camera_fixture(&memory);
    memory.state.lock().unwrap().kept = NEW.into();
    let (ahead, context) = local_ahead(&memory);
    let old = ticket_scope(&authority, 1, ScopeSnapshot::of(OWNER));
    let next = ticket_scope(&authority, 2, ScopeSnapshot::of(OWNER));
    let arrived = memory.hold_open(0);
    let elapsed = runtime().block_on(async {
        let old_waiting = tokio::spawn(ahead.clone().ready(Some(old.lease())));
        tokio::task::spawn_blocking(move || arrived.recv_timeout(WAIT).unwrap())
            .await
            .unwrap();
        assert_eq!(
            memory.count("session_open"),
            1,
            "old Room response is still held"
        );
        let began = Instant::now();
        let new_waiting = ahead.clone().ready(Some(next.lease()));
        tokio::pin!(new_waiting);
        // Poll the actual new ready future before letting the old reader end.
        // It now owns its answer Arc and is awaiting that exact done receipt.
        let first = std::future::poll_fn(|cx| {
            std::task::Poll::Ready(std::future::Future::poll(new_waiting.as_mut(), cx))
        })
        .await;
        assert!(
            first.is_pending(),
            "new ready is actually waiting on the old reader"
        );
        assert!(
            ahead
                .sessions
                .lock()
                .unwrap()
                .asked
                .as_ref()
                .unwrap()
                .reading()
        );
        assert_eq!(
            memory.count("session_open"),
            1,
            "no overlap while old RPC is held"
        );
        old.revoke();
        assert!(next.lease().is_current());
        assert_eq!(next.lease().speaker(), Some("0123456789abcdef/f7"));
        memory.release();
        // The same absolute fixture wait covers old denial and the one allowed
        // fresh read. No second wait budget or pre-completion read_done helper.
        tokio::time::timeout(WAIT, new_waiting).await.unwrap();
        tokio::time::timeout(WAIT, old_waiting)
            .await
            .unwrap()
            .unwrap();
        began.elapsed()
    });
    let told = context(Some(&next.lease()));
    let private = told.requires_speaker;
    let body = payload(told);
    assert!(
        private && body.contains(NEW) && !body.contains(OLD),
        "the already-waiting new request must receive its own freshly proved note"
    );
    let opens: Vec<Value> = memory
        .state
        .lock()
        .unwrap()
        .requests
        .iter()
        .filter(|request| request["op"] == "session_open")
        .cloned()
        .collect();
    assert_eq!(
        opens.len(),
        3,
        "cancelled Room only plus exactly one fresh two-scope read"
    );
    assert!(opens[0]["person"].is_null() && opens[1]["person"].is_null());
    assert_eq!(opens[2]["person"], crate::remember::person_id(OWNER));
    assert_ne!(opens[0]["session"], opens[1]["session"]);
    assert_eq!(opens[1]["session"], opens[2]["session"]);
    assert_eq!(
        memory.count("follow_up"),
        1,
        "only the new request spends an offer"
    );
    assert!(elapsed < WAIT);
    eprintln!(
        "note denied waiter: opens=3 private_opens=1 ready_us={}",
        elapsed.as_micros()
    );
}
