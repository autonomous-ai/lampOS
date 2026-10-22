//! The worker callback and tool runner share the process's real memory client.
//! Synthetic store replies test propagation, not conflicting-fact ranking or
//! whether a model chooses the newest name when it answers aloud.
use super::{HISTORY_EXCHANGES, HISTORY_WITHIN, Sessions, Told, memory_context, system_message};
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
        "days" | "days-room" => {
            command.env(crate::remember::DAYS_ENVIRONMENT, "on");
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
    /// Snapshot opens, by their count from 0, answered only after a wait.
    slow_opens: Option<(std::ops::RangeInclusive<usize>, Duration)>,
}

impl Store {
    /// How long `request` waits before its answer ([`Store::slow_opens`]).
    fn delay(&self, request: &Value) -> Option<Duration> {
        let (opens, delay) = self.slow_opens.clone()?;
        let count = self
            .requests
            .iter()
            .filter(|request| request["op"] == "session_open")
            .count();
        (request["op"] == "session_open" && opens.contains(&count)).then_some(delay)
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
                let text = (person == self.person).then_some(&self.kept);
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
            "kept" => json!({"ok":true,"entries":[{"text":self.kept}]}),
            "session_close" | "conversation_end" => json!({"ok":true}),
            op => panic!("unexpected memory operation: {op}"),
        }
    }
}

struct Memory {
    directory: PathBuf,
    socket: PathBuf,
    state: Arc<Mutex<Store>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Memory {
    fn new(person: Option<String>) -> Self {
        let directory =
            std::env::temp_dir().join(format!("lv-kept-callback-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let socket = directory.join("memory.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let state = Arc::new(Mutex::new(Store {
            person,
            kept: OLD.into(),
            ..Store::default()
        }));
        let serving = state.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = std::thread::spawn(move || {
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
                // Waited without the store held, so nothing else waits on it.
                let delay = serving.lock().unwrap().delay(&request);
                if let Some(delay) = delay {
                    std::thread::sleep(delay);
                }
                let answer = serving.lock().unwrap().answer(request);
                writeln!(stream, "{answer}").unwrap();
            }
        });
        Self {
            directory,
            socket,
            state,
            stop,
            worker: Some(worker),
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

fn scope(request: u64, snapshot: ScopeSnapshot) -> RequestScope {
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
    let ahead = days.then(|| super::Ahead::reading(sessions.clone(), None, socket.clone()));
    let told = memory_context(sessions, Some(socket), None, days, |_, person, note| {
        system_message(
            "Synthetic character.\n",
            None,
            &note,
            None,
            person.as_deref(),
        )
    });
    move |lease| {
        if let Some(ahead) = &ahead {
            ahead.begin(lease, Instant::now());
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(ahead.clone().ready(lease.cloned()));
        }
        told(lease)
    }
}

/// The second route's turns as the engine drives them, each step its own: a
/// line begins (`super::Ahead::begin`), and only an answered one is waited
/// for and told; a line cancelled before its context is never told.
struct Lines {
    ahead: super::Ahead,
    context: Context,
}

/// The worker's context callback, boxed.
type Context = Box<dyn Fn(Option<&crate::memory_scope::ScopeLease>) -> Told + Send + Sync>;

impl Lines {
    fn new(socket: PathBuf) -> Self {
        let sessions = Arc::new(Mutex::new(Sessions::default()));
        let ahead = super::Ahead::reading(sessions.clone(), None, socket.clone());
        let context = Box::new(memory_context(
            sessions,
            Some(socket),
            None,
            true,
            |_, person, note| {
                system_message(
                    "Synthetic character.\n",
                    None,
                    &note,
                    None,
                    person.as_deref(),
                )
            },
        ));
        Self { ahead, context }
    }

    fn begins(&self, scope: &RequestScope) {
        self.ahead.begin(Some(&scope.lease()), Instant::now());
    }

    /// The line answered: `(contains NEW, contains OLD)` of what it is told.
    fn answered(&self, scope: &RequestScope) -> (bool, bool) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(self.ahead.clone().ready(Some(scope.lease())));
        let told = payload((self.context)(Some(&scope.lease())));
        (told.contains(NEW), told.contains(OLD))
    }

    /// Waits, at most 2 s, until no read is going on.
    fn read_done(&self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while self
            .ahead
            .sessions
            .lock()
            .unwrap()
            .asked
            .as_ref()
            .is_some_and(super::NoteAsk::reading)
        {
            assert!(Instant::now() < deadline, "the read never ended");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
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
    let memory = Memory::new(Some(crate::remember::person_id(OWNER)));
    crate::remember::start(memory.socket.clone(), None);
    let lines = Lines::new(memory.socket.clone());
    let first = scope(1, ScopeSnapshot::of(OWNER));
    lines.begins(&first);
    assert_eq!(lines.answered(&first), (false, true));
    assert_eq!(memory.count("session_open"), 2, "room and admitted person");
    assert_eq!(
        keep(&memory, &scope(2, ScopeSnapshot::of(OWNER)), NEW)["remembered"],
        true
    );
    // Canceled before its context: the refresh is read and done first.
    let cancelled = scope(3, ScopeSnapshot::of(OWNER));
    lines.begins(&cancelled);
    cancelled.revoke();
    lines.read_done();
    assert_eq!(memory.count("session_open"), 4, "the refresh was read");
    let next: Vec<(bool, bool)> = (4..=5)
        .map(|request| {
            let line = scope(request, ScopeSnapshot::of(OWNER));
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
        1,
    ) {
        return;
    }
    let memory = Memory::new(Some(crate::remember::person_id(OWNER)));
    crate::remember::start(memory.socket.clone(), None);
    memory.state.lock().unwrap().slow_opens = Some((2..=3, Duration::from_millis(100)));
    let lines = Lines::new(memory.socket.clone());
    let first = scope(1, ScopeSnapshot::of(OWNER));
    lines.begins(&first);
    assert_eq!(lines.answered(&first), (false, true));
    assert_eq!(
        keep(&memory, &scope(2, ScopeSnapshot::of(OWNER)), NEW)["remembered"],
        true
    );
    let cancelled = scope(3, ScopeSnapshot::of(OWNER));
    lines.begins(&cancelled);
    cancelled.revoke();
    let next = scope(4, ScopeSnapshot::of(OWNER));
    lines.begins(&next);
    assert!(
        memory.count("session_open") < 4,
        "the refresh is still read as the next line begins"
    );
    assert_eq!(lines.answered(&next), (true, false));
    let after = scope(5, ScopeSnapshot::of(OWNER));
    lines.begins(&after);
    assert_eq!(lines.answered(&after), (true, false));
    assert_eq!(memory.count("session_open"), 4);
}

/// A finished refresh let go untold (the camera closed or opened before the
/// person's next turn) gives the kept mark back, so the person's next line
/// asks again and is told the new name, though nothing else changed.
#[test]
fn a_refresh_let_go_untold_gives_the_keep_back() {
    if !child("a_refresh_let_go_untold_gives_the_keep_back", "days", 2) {
        return;
    }
    let memory = Memory::new(Some(crate::remember::person_id(OWNER)));
    crate::remember::start(memory.socket.clone(), None);
    let lines = Lines::new(memory.socket.clone());
    let first = scope(1, ScopeSnapshot::of(OWNER));
    lines.begins(&first);
    assert_eq!(lines.answered(&first), (false, true));
    assert_eq!(
        keep(&memory, &scope(2, ScopeSnapshot::of(OWNER)), NEW)["remembered"],
        true
    );
    let cancelled = scope(3, ScopeSnapshot::of(OWNER));
    lines.begins(&cancelled);
    cancelled.revoke();
    lines.read_done();
    lines.ahead.forget();
    let next = scope(4, ScopeSnapshot::of(OWNER));
    lines.begins(&next);
    assert_eq!(lines.answered(&next), (true, false));
    assert_eq!(
        memory.count("session_open"),
        6,
        "asked again for the keep given back"
    );
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
    let memory = Memory::new(Some(person.clone()));
    crate::remember::start(memory.socket.clone(), None);
    assert!(
        crate::tools::declared(&memory.settings())
            .iter()
            .any(|tool| tool.name == crate::remember::REMEMBER)
    );
    let context = context(memory.socket.clone());
    let admitted = scope(1, ScopeSnapshot::of(OWNER));
    let first = Instant::now();
    let first_payload = payload(context(Some(&admitted.lease())));
    let first_us = first.elapsed().as_micros();
    assert!(first_payload.contains(OLD));
    assert_eq!(memory.count("session_open"), 2, "room and admitted person");

    memory.state.lock().unwrap().refused = true;
    let refused = scope(2, ScopeSnapshot::of(OWNER));
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
    let kept = scope(3, ScopeSnapshot::of(OWNER));
    assert_eq!(keep(&memory, &kept, NEW)["remembered"], true);

    let revoked = scope(4, ScopeSnapshot::of(OWNER));
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
    let held = scope(5, memory.held());
    let held_payload = payload(context(Some(&held.lease())));
    assert!(!held_payload.contains(OLD) && !held_payload.contains(NEW));
    assert_eq!(
        memory.count("session_open"),
        2,
        "held and revoked contexts do not consume the keep"
    );

    let next = scope(6, ScopeSnapshot::of(OWNER));
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
    let later = scope(7, ScopeSnapshot::of(OWNER));
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
