//! Real Voice routing and Actions server, with an isolated in-process Schedule
//! engine behind a fixture socket. No provider, account, audio or lamp is used.
use super::*;
use lamp_actions::{Executive, light::Light, schedule::Adapter, sound::Mixer};
use lamp_schedule::{
    engine::{Engine, Persistence},
    protocol::{Guard, Request, refusal},
    time::{ClockSample, When},
};
use std::{
    fs, io,
    os::unix::{fs::DirBuilderExt, net::UnixListener},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
};

const STORE: &str = "0123456789abcdef0123456789abcdef";
const WALL: i64 = 1_791_964_800_000;

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let path = fs::canonicalize("/tmp").unwrap().join(format!(
            "vs-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[derive(Default)]
struct Memory(Option<Vec<u8>>);
impl Persistence for Memory {
    fn load(&self) -> io::Result<Option<Vec<u8>>> {
        Ok(self.0.clone())
    }
    fn commit(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.0 = Some(bytes.to_vec());
        Ok(())
    }
}
fn sample(ms: u64) -> ClockSample {
    ClockSample {
        unix_ms: WALL + ms as i64,
        boot_ms: ms,
        boot_id: "fixture-boot".into(),
        wall_trusted: true,
    }
}
struct Schedule {
    root: Temp,
    engine: Arc<Mutex<Engine<Memory>>>,
    clock: Arc<AtomicU64>,
    lose_mutation: Arc<AtomicBool>,
    calls: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Schedule {
    fn new() -> Self {
        let root = Temp::new();
        let listener = UnixListener::bind(root.0.join("schedule.sock")).unwrap();
        listener.set_nonblocking(true).unwrap();
        let engine = Arc::new(Mutex::new(
            Engine::open(Memory::default(), "Etc/UTC", &sample(0), STORE).unwrap(),
        ));
        let clock = Arc::new(AtomicU64::new(0));
        let lose_mutation = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (shared, time, lost, captured, flag) = (
            engine.clone(),
            clock.clone(),
            lose_mutation.clone(),
            calls.clone(),
            stop.clone(),
        );
        let worker = thread::spawn(move || {
            while !flag.load(Ordering::Acquire) {
                let mut stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(error) => panic!("fixture accept: {error}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut line = Vec::new();
                if BufReader::new(&mut stream)
                    .read_until(b'\n', &mut line)
                    .unwrap()
                    == 0
                {
                    continue;
                }
                let request = Request::parse(&line).unwrap();
                captured.lock().unwrap().push(request.clone());
                let response = shared
                    .lock()
                    .unwrap()
                    .answer(request.clone(), &sample(time.load(Ordering::Acquire)))
                    .unwrap_or_else(|error| refusal(error.code, error.message));
                if request.guard().is_some() && lost.swap(false, Ordering::AcqRel) {
                    continue;
                }
                let _ = writeln!(stream, "{response}");
            }
        });
        Self {
            root,
            engine,
            clock,
            lose_mutation,
            calls,
            stop,
            worker: Some(worker),
        }
    }
    fn adapter(&self) -> Adapter {
        Adapter::new(self.root.0.join("schedule.sock")).unwrap()
    }
    fn populate(&self, count: usize, text: &str) {
        let mut engine = self.engine.lock().unwrap();
        for index in 0..count {
            let guard = Guard {
                store_id: engine.store_id().into(),
                revision: engine.revision(),
                request_id: format!("fixture-{index}"),
            };
            engine
                .answer(
                    Request::Set {
                        person: None,
                        guard,
                        text: text.into(),
                        when: When::After { seconds: 60 },
                    },
                    &sample(0),
                )
                .unwrap();
        }
    }
}
impl Drop for Schedule {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
    }
}
struct NoMixer;
impl Mixer for NoMixer {
    fn get(&mut self) -> io::Result<u8> {
        panic!("fixture has no mixer")
    }
    fn set(&mut self, _: u8) -> io::Result<()> {
        panic!("fixture has no mixer")
    }
}
struct NoLight;
impl Light for NoLight {
    fn there(&mut self) -> bool {
        false
    }
    fn shown(&mut self) -> Option<lamp_msgs::Light> {
        None
    }
    fn ask(&mut self, _: &lamp_msgs::Light) -> bool {
        panic!("fixture has no light")
    }
}
struct Actions {
    root: Temp,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<io::Result<()>>>,
}
impl Actions {
    fn new(adapter: Option<Adapter>) -> Self {
        let root = Temp::new();
        let path = root.0.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let worker = thread::spawn(move || {
            let mut executive = Executive {
                sound: None::<lamp_actions::sound::Volume<NoMixer>>,
                light: None::<NoLight>,
                short: false,
            };
            lamp_actions::server::serve_reporting_with_schedule(
                &path,
                &mut executive,
                &flag,
                None,
                adapter.as_ref(),
            )
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while !root.0.join(lamp_actions::server::READY_FILE).exists() {
            assert!(
                Instant::now() < deadline,
                "Actions fixture did not become ready"
            );
            thread::sleep(Duration::from_millis(2));
        }
        Self {
            root,
            stop,
            worker: Some(worker),
        }
    }
    fn settings(&self, enabled: bool) -> Settings {
        Settings {
            exec: Some(self.root.0.clone()),
            schedule_tools: enabled,
            ..Settings::default()
        }
    }
    /// A call from the owner's own line, nobody sure who set it: it hears
    /// and changes the reminders set with no person (#4527's P2).
    fn call(&self, name: &str, args: Value) -> (Value, bool) {
        let Route::Schedule(path) = route(&self.settings(true), name) else {
            panic!("wrong Voice route")
        };
        ask_schedule_for(
            &path,
            &ToolCall {
                id: "provider-call".into(),
                name: name.into(),
                args,
            },
            ASK_DEADLINE,
            Some(&json!({"asker":{"owner":true}})),
        )
    }
    /// A call run as the voice runs it, on `lease`'s line.
    fn call_on(&self, name: &str, args: Value, lease: &crate::memory_scope::ScopeLease) -> Value {
        let route = route(&self.settings(true), name);
        let call = ToolCall {
            id: "provider-call".into(),
            name: name.into(),
            args,
        };
        run_call_scoped(&route, &call, lease.key().request, true, lease)
            .1
            .0
    }
}
impl Drop for Actions {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap().unwrap();
    }
}

#[test]
fn all_six_voice_routes_reach_actual_actions_and_return_schedule_resolved_results() {
    let schedule = Schedule::new();
    let actions = Actions::new(Some(schedule.adapter()));
    let declared = declared(&actions.settings(true));
    assert_eq!(
        declared
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        SCHEDULE_NAMES
    );
    assert!(declared.iter().all(|tool| tool.validate().is_ok()));
    for (name, args) in [
        ("set_timer", json!({"text":"Tea","seconds":1})),
        (
            "set_reminder",
            json!({"text":"Meeting","time":"09:00","day":"tomorrow","zone":"Asia/Ho_Chi_Minh"}),
        ),
        (
            "repeat_reminder",
            json!({"text":"Stretch","time":"12:00","every":"weekly","weekday":"monday","zone":"Etc/UTC"}),
        ),
    ] {
        let (told, received) = actions.call(name, args);
        assert!(received && sendable(&told), "{name}: {told}");
        assert_eq!(told["status"], "committed");
        assert!(told["local"].as_str().is_some());
    }
    let (listed, received) = actions.call("list_reminders", json!({}));
    assert!(received && sendable(&listed));
    assert_eq!(listed["items"].as_array().unwrap().len(), 3);
    schedule.clock.store(1000, Ordering::Release);
    let (snoozed, received) = actions.call("snooze_reminder", json!({"id":1,"seconds":120}));
    assert!(received && sendable(&snoozed));
    assert_eq!(snoozed["delivery"], 2);
    assert_eq!(snoozed["due_unix_ms"], WALL + 121_000);
    let (cancelled, received) = actions.call("cancel_reminder", json!({"id":2}));
    assert!(received && sendable(&cancelled));
    assert_eq!(cancelled["cancelled"], true);
    let calls = schedule.calls.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|r| matches!(r, Request::Set { .. }))
            .count(),
        3
    );
    assert_eq!(
        calls
            .iter()
            .filter(|r| matches!(r, Request::Snooze { .. }))
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(|r| matches!(r, Request::Cancel { .. }))
            .count(),
        1
    );
    assert!(!calls.iter().any(|r| matches!(r, Request::Ack { .. })));
}

#[test]
fn voice_off_filters_a_real_enabled_actions_catalog_and_never_routes_schedule_calls() {
    let schedule = Schedule::new();
    let actions = Actions::new(Some(schedule.adapter()));
    let off = actions.settings(false);
    assert!(declared(&off).is_empty());
    for name in SCHEDULE_NAMES {
        assert_eq!(route(&off, name), Route::Nobody);
    }
    assert!(schedule.calls.lock().unwrap().is_empty());
    let absent_actions = Actions::new(None);
    assert!(declared(&absent_actions.settings(true)).is_empty());
}

#[test]
fn voice_receives_unknown_for_a_lost_durable_mutation_reply_then_can_read_current_items() {
    let schedule = Schedule::new();
    let actions = Actions::new(Some(schedule.adapter()));
    schedule.lose_mutation.store(true, Ordering::Release);
    let (told, received) = actions.call(
        "set_timer",
        json!({"text":"Private tea reminder","seconds":60}),
    );
    assert!(
        received,
        "Actions returned its explicit uncertainty envelope"
    );
    assert!(
        told["error"]
            .as_str()
            .unwrap()
            .contains("may have happened")
    );
    assert!(!told.to_string().contains("Private tea reminder"));
    assert!(sendable(&told));
    let (listed, received) = actions.call("list_reminders", json!({}));
    assert!(received);
    assert_eq!(listed["items"].as_array().unwrap().len(), 1);
    assert_eq!(listed["items"][0]["text"], "Private tea reminder");
    assert_eq!(
        schedule
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|r| matches!(r, Request::Set { .. }))
            .count(),
        1
    );
}

#[test]
fn voice_keeps_all_escaped_list_text_across_explicit_sendable_pages() {
    let schedule = Schedule::new();
    let label = "\"\\".repeat(150);
    schedule.populate(8, &label);
    let actions = Actions::new(Some(schedule.adapter()));
    let mut cursor = 0;
    let mut ids = Vec::new();
    for _ in 0..8 {
        let (told, received) = actions.call("list_reminders", json!({"after_id":cursor}));
        assert!(received && sendable(&told));
        for item in told["items"].as_array().unwrap() {
            assert_eq!(item["text"], label);
            ids.push(item["id"].as_u64().unwrap());
        }
        let Some(next) = told["next_after_id"].as_u64() else {
            assert_eq!(told["complete"], true);
            break;
        };
        assert!(next > cursor);
        assert_eq!(told["complete"], false);
        cursor = next;
    }
    assert_eq!(ids, (1..=8).collect::<Vec<u64>>());
    assert!(
        schedule.calls.lock().unwrap().len() > 1,
        "escaped labels must use explicit pages"
    );
}

#[test]
fn voice_actions_boundary_rejects_malformed_model_arguments_without_schedule_ipc() {
    let schedule = Schedule::new();
    let actions = Actions::new(Some(schedule.adapter()));
    for (name, args) in [
        ("set_timer", json!({"text":"Tea","seconds":1,"guard":{}})),
        ("set_reminder", json!({"text":"Meeting","time":"25:00"})),
        (
            "repeat_reminder",
            json!({"text":"Stretch","time":"12:00","every":"weekly"}),
        ),
        ("list_reminders", json!({"pending_only":"yes"})),
        ("cancel_reminder", json!({"id":0})),
        ("snooze_reminder", json!({"id":1,"seconds":0})),
    ] {
        let (told, received) = actions.call(name, args);
        assert!(received && sendable(&told));
        assert!(told["error"].as_str().is_some());
    }
    assert!(schedule.calls.lock().unwrap().is_empty());
}

/// #4527's P2 through the voice's own route: each line's caller is its
/// scope's, so two people each asking "my reminders" hear their own, an
/// unknown face hears none, and a guest's cancel by the other's id is
/// refused as no longer active. A timer is every line's (option 1 on #4656's
/// after-merge P2): the room's line hears and cancels the one it set.
#[test]
fn voice_lines_hear_and_cancel_only_their_own_reminders() {
    use crate::memory_scope::{RequestKey, RequestScope, ScopeSnapshot};
    let schedule = Schedule::new();
    let actions = Actions::new(Some(schedule.adapter()));
    let line = |snapshot: ScopeSnapshot, request: u64| {
        RequestScope::new(
            RequestKey {
                worker: lamp_turn_owner::BootId::new([5; 16]).unwrap(),
                session: 1,
                request,
            },
            snapshot,
        )
    };
    let ann = line(ScopeSnapshot::of("Ann"), 1);
    let bo = line(ScopeSnapshot::of("Bo"), 2);
    let unknown = line(ScopeSnapshot::room(), 3);
    // Reminders, at 08:10, 600 s after `WALL`: a timer is every line's.
    let set = actions.call_on(
        "set_reminder",
        json!({"text":"Ann's pills","time":"08:10"}),
        &ann.lease(),
    );
    assert_eq!(set["status"], "committed", "{set}");
    let set = actions.call_on(
        "set_reminder",
        json!({"text":"Bo's call","time":"08:10"}),
        &bo.lease(),
    );
    assert_eq!(set["status"], "committed", "{set}");
    let texts = |told: Value| -> Vec<String> {
        told["items"]
            .as_array()
            .unwrap_or_else(|| panic!("{told}"))
            .iter()
            .map(|i| i["text"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(
        texts(actions.call_on("list_reminders", json!({}), &ann.lease())),
        ["Ann's pills"]
    );
    assert_eq!(
        texts(actions.call_on("list_reminders", json!({}), &bo.lease())),
        ["Bo's call"]
    );
    assert!(texts(actions.call_on("list_reminders", json!({}), &unknown.lease())).is_empty());
    let refused = actions.call_on("cancel_reminder", json!({"id":1}), &bo.lease());
    assert!(
        refused.to_string().contains("no longer active"),
        "{refused}"
    );
    let cancelled = actions.call_on("cancel_reminder", json!({"id":1}), &ann.lease());
    assert_eq!(cancelled["cancelled"], true, "{cancelled}");
    // At the stove, no face seen: the room's line finds the timer it set,
    // and cancels it, and still hears none of the reminders.
    let set = actions.call_on(
        "set_timer",
        json!({"text":"Pasta","seconds":600}),
        &unknown.lease(),
    );
    assert_eq!(set["status"], "committed", "{set}");
    assert_eq!(
        texts(actions.call_on("list_reminders", json!({}), &unknown.lease())),
        ["Pasta"]
    );
    let pasta = set["id"].as_u64().unwrap();
    let cancelled = actions.call_on("cancel_reminder", json!({"id":pasta}), &unknown.lease());
    assert_eq!(cancelled["cancelled"], true, "{cancelled}");
}
