//! Real Actions dispatch with scripted sockets and an isolated Schedule service.
//! Time, storage and endpoint fixtures belong to this host test, never a lamp.
use lamp_actions::{
    Executive,
    light::Light,
    schedule::{Adapter, DEADLINE, MAX_RESULT_BYTES, NAMES, catalog},
    server::{self, CALL_SOCKET, CATALOG_FILE, READY_FILE},
    sound::Mixer,
};
use lamp_schedule::{
    client,
    runtime::{Clock, Config},
    time::ClockSample,
};
use serde_json::{Value, json};
use std::{
    ffi::OsStr,
    fs,
    io::{self, BufRead, BufReader, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// How long a test against the real Schedule service waits for one reply.
const REAL_WAIT: Duration = Duration::from_secs(10);
const STORE: &str = "0123456789abcdef0123456789abcdef";
const WALL: i64 = 1_791_964_800_000;
struct NoMixer;
impl Mixer for NoMixer {
    fn get(&mut self) -> io::Result<u8> {
        panic!("no mixer is configured")
    }
    fn set(&mut self, _: u8) -> io::Result<()> {
        panic!("no mixer is configured")
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
        panic!("no ring is configured")
    }
}
fn root() -> tempfile::TempDir {
    let root = tempfile::Builder::new()
        .prefix("as-")
        .tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap())
        .unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    root
}
fn wait(mut predicate: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !predicate() {
        assert!(Instant::now() < until, "fixture did not become ready");
        thread::sleep(Duration::from_millis(5));
    }
}
fn read(stream: &mut UnixStream) -> Value {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}
fn ask(path: &Path, value: Value) -> Value {
    client::exchange(path, &value, Instant::now() + Duration::from_secs(5)).unwrap()
}
struct Actions {
    root: tempfile::TempDir,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<io::Result<()>>>,
}
impl Actions {
    fn new(adapter: Option<Adapter>) -> Self {
        let root = root();
        let path = root.path().to_owned();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let worker = thread::spawn(move || {
            let mut executive = Executive {
                sound: None::<lamp_actions::sound::Volume<NoMixer>>,
                light: None::<NoLight>,
                short: false,
            };
            server::serve_reporting_with_schedule(
                &path,
                &mut executive,
                &flag,
                None,
                adapter.as_ref(),
            )
        });
        wait(|| root.path().join(READY_FILE).exists());
        Self {
            root,
            stop,
            worker: Some(worker),
        }
    }
    /// A call from the owner's own line, nobody sure who set it: it hears
    /// and changes the reminders set with no person (#4527's P2).
    fn call(&self, name: &str, args: Value) -> Value {
        self.call_as(name, args, json!({"asker":{"owner":true}}))
    }
    fn call_as(&self, name: &str, args: Value, caller: Value) -> Value {
        self.raw(json!({"op":"call","name":name,"args":args,"caller":caller}))
    }
    fn raw(&self, value: Value) -> Value {
        ask(&self.root.path().join(CALL_SOCKET), value)
    }
}
impl Drop for Actions {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap().unwrap();
    }
}
struct Script {
    root: tempfile::TempDir,
    requests: Arc<Mutex<Vec<Value>>>,
    worker: Option<JoinHandle<()>>,
}
impl Script {
    fn new(replies: Vec<Value>) -> Self {
        Self::with(replies.len(), move |_, index| replies[index].clone())
    }
    fn with(count: usize, mut answer: impl FnMut(&Value, usize) -> Value + Send + 'static) -> Self {
        let root = root();
        let listener = UnixListener::bind(root.path().join("s")).unwrap();
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let worker = thread::spawn(move || {
            for index in 0..count {
                let until = Instant::now() + Duration::from_secs(10);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((s, _)) => break s,
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < until, "missing scripted request {index}");
                            thread::sleep(Duration::from_millis(2));
                        }
                        Err(e) => panic!("fixture accept: {e}"),
                    }
                };
                let request = read(&mut stream);
                captured.lock().unwrap().push(request.clone());
                let reply = answer(&request, index);
                if !reply.is_null() {
                    let _ = writeln!(stream, "{reply}");
                }
            }
        });
        Self {
            root,
            requests,
            worker: Some(worker),
        }
    }
    fn adapter(&self) -> Adapter {
        Adapter::new(self.root.path().join("s")).unwrap()
    }
    fn finish(&mut self) -> Vec<Value> {
        self.worker.take().unwrap().join().unwrap();
        self.requests.lock().unwrap().clone()
    }
}
impl Drop for Script {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}
fn status() -> Value {
    json!({"ok":true,"store_id":STORE,"revision":0})
}
fn commit(id: u64) -> Value {
    json!({"ok":true,"store_id":STORE,"revision":1,"id":id,"delivery":1,
        "due_unix_ms":WALL+600_000,"local":lamp_schedule::time::resolve(
            &lamp_schedule::time::When::After{seconds:600}, &sample(0), "Etc/UTC").unwrap().local})
}
fn sample(ms: u64) -> ClockSample {
    ClockSample {
        unix_ms: WALL + ms as i64,
        boot_ms: ms,
        boot_id: "actions-fixture".into(),
        wall_trusted: true,
    }
}
#[derive(Clone)]
struct Injected(Arc<Mutex<ClockSample>>);
impl Clock for Injected {
    fn sample(&self) -> io::Result<ClockSample> {
        Ok(self.0.lock().unwrap().clone())
    }
}
struct Schedule {
    root: tempfile::TempDir,
    socket: PathBuf,
    clock: Injected,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<io::Result<()>>>,
}
impl Schedule {
    fn new() -> Self {
        let root = root();
        let socket = root.path().join("run/s");
        let clock = Injected(Arc::new(Mutex::new(sample(0))));
        let stop = Arc::new(AtomicBool::new(false));
        let config = Config {
            enabled: true,
            dir: root.path().join("data"),
            socket: socket.clone(),
            bus_root: root.path().join("b"),
            zone: "Etc/UTC".into(),
            utc_offset_minutes: None,
            run_for: None,
            report_every: lamp_schedule::runtime::REPORT_EVERY,
            request_deadline: REAL_WAIT,
        };
        let worker = {
            let (clock, stop) = (clock.clone(), stop.clone());
            thread::spawn(move || lamp_schedule::runtime::serve(config, clock, stop))
        };
        wait(|| socket.exists());
        Self {
            root,
            socket,
            clock,
            stop,
            worker: Some(worker),
        }
    }
}
impl Schedule {
    /// An adapter to this real service that waits up to [`REAL_WAIT`] for a
    /// reply instead of [`DEADLINE`]. These tests check what a call does, and
    /// a loaded host can hold one durable commit past 1250 ms (lamp-voice's
    /// push runs on the Studio, 29 Oct). The deadline itself is pinned
    /// against a scripted socket, in
    /// `all_adapter_hops_share_one_deadline_and_uncertain_call_is_not_reissued`.
    fn adapter(&self) -> Adapter {
        Adapter::new(self.socket.clone())
            .unwrap()
            .with_deadline(REAL_WAIT)
    }
}
impl Drop for Schedule {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap().unwrap();
        assert!(self.root.path().join("data/schedule.sqlite3").exists());
    }
}

#[test]
fn all_six_tools_reach_real_schedule_through_the_real_actions_server() {
    let schedule = Schedule::new();
    let actions = Actions::new(Some(schedule.adapter()));
    let offered: Value =
        serde_json::from_slice(&fs::read(actions.root.path().join(CATALOG_FILE)).unwrap()).unwrap();
    let names: Vec<_> = offered["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, NAMES);
    let timer = actions.call("set_timer", json!({"text":"Check the pasta","seconds":600}));
    assert_eq!(timer["result"]["status"], "committed");
    assert_eq!(timer["result"]["id"], 1);
    assert_eq!(timer["result"]["due_unix_ms"], WALL + 600_000);
    // The local HH:MM the model says, beside `local` (lamp-4ace, 25 Oct: it
    // read "07:34" out of `local` and said "seven thirty-four" at 21:07).
    let local = timer["result"]["local"].as_str().unwrap();
    assert_eq!(
        timer["result"]["clock"].as_str(),
        lamp_actions::schedule::clock_of(local).as_deref()
    );
    let once = actions.call("set_reminder", json!({"text":"Call Ana","time":"09:00","day":"tomorrow","zone":"Asia/Ho_Chi_Minh","fold":"reject"}));
    assert_eq!(once["result"]["id"], 2);
    for (args, id) in [
        (
            json!({"text":"Water plants","time":"10:00","every":"daily"}),
            3,
        ),
        (
            json!({"text":"Bins","time":"19:00","every":"weekly","weekday":"thursday","zone":"Etc/UTC"}),
            4,
        ),
    ] {
        assert_eq!(actions.call("repeat_reminder", args)["result"]["id"], id);
    }
    let list = actions.call("list_reminders", json!({}));
    assert_eq!(list["result"]["items"].as_array().unwrap().len(), 4);
    assert_eq!(list["result"]["complete"], true);
    assert_eq!(list["result"]["items"][2]["repeat"]["kind"], "daily");
    *schedule.clock.0.lock().unwrap() = sample(601_000);
    let snoozed = actions.call("snooze_reminder", json!({"id":1,"seconds":120}));
    assert_eq!(snoozed["result"]["status"], "committed");
    assert_eq!(snoozed["result"]["delivery"], 2);
    assert_eq!(snoozed["result"]["due_unix_ms"], WALL + 721_000);
    assert_eq!(
        actions.call("cancel_reminder", json!({"id":2}))["result"]["cancelled"],
        true
    );
    let item = ask(&schedule.socket, json!({"op":"get","id":1}));
    assert_eq!(item["item"]["delivery"], 2);
    assert_eq!(item["item"]["state"], "scheduled");
    let state = ask(&schedule.socket, json!({"op":"status"}));
    assert_eq!(state["items"], 3);
    assert_eq!(state["last_receipt"]["op"], "cancel");
}

#[test]
fn disabled_switch_preserves_the_old_catalog_and_refuses_all_six_names() {
    for switch in [
        None,
        Some(""),
        Some("off"),
        Some("ON"),
        Some("true"),
        Some("on "),
    ] {
        assert!(
            Adapter::configured(switch.map(OsStr::new), Some(Path::new("not absolute")))
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(
        Adapter::configured(Some(OsStr::new("on")), None)
            .unwrap()
            .unwrap()
            .socket(),
        Path::new(lamp_actions::schedule::DEFAULT_SOCKET)
    );
    assert!(Adapter::configured(Some(OsStr::new("on")), Some(Path::new("relative"))).is_err());
    let actions = Actions::new(None);
    assert_eq!(
        fs::read_to_string(actions.root.path().join(CATALOG_FILE)).unwrap(),
        lamp_actions::catalog_file(&lamp_actions::catalog(false, false))
    );
    for name in NAMES {
        assert_eq!(
            actions.call(name, json!({})),
            json!({"ok":false,"error":"I can't do that."})
        );
    }
}

#[test]
fn published_catalog_routes_relative_reminders_to_elapsed_timers() {
    let schedule = root();
    let actions = Actions::new(Some(
        Adapter::new(schedule.path().join("not-running.sock")).unwrap(),
    ));
    let offered: Value =
        serde_json::from_slice(&fs::read(actions.root.path().join(CATALOG_FILE)).unwrap()).unwrap();
    let description = |name| {
        offered["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap()["description"]
            .as_str()
            .unwrap()
    };
    let timer = description("set_timer");
    for guidance in [
        "relative reminder",
        "duration from now",
        "in two minutes",
        "in ten minutes",
        "in an hour",
        "requested label and whole seconds",
        "120, 600, or 3600 respectively",
        "never round to a clock time",
    ] {
        assert!(
            timer.contains(guidance),
            "missing timer guidance: {guidance}"
        );
    }
    let reminder = description("set_reminder");
    for guidance in [
        "requested local clock time",
        "at nine tomorrow morning",
        "relative times from now, use set_timer",
        "requested label and whole seconds",
        "do not round to HH:MM",
    ] {
        assert!(
            reminder.contains(guidance),
            "missing clock-time guidance: {guidance}"
        );
    }
}

#[test]
fn new_schemas_use_the_small_provider_vocabulary() {
    let tools = serde_json::to_value(catalog()).unwrap();
    let encoded = tools.to_string();
    assert!(!encoded.contains("additionalProperties"));
    assert!(!encoded.contains("maxLength"));
    assert_eq!(tools.as_array().unwrap().len(), 6);
    for tool in tools.as_array().unwrap() {
        assert_eq!(tool["parameters"]["type"], "object");
        assert!(tool["parameters"]["properties"].is_object());
    }
    assert_eq!(
        tools[3]["parameters"]["properties"]["after_id"]["type"],
        "integer"
    );
}

#[test]
fn malformed_arguments_and_unknown_fields_open_no_schedule_connection() {
    let root = root();
    let socket = root.path().join("s");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let adapter = Adapter::new(socket).unwrap();
    let cases = [
        ("set_timer", json!({"text":"x","seconds":0})),
        ("set_timer", json!({"text":"x","seconds":31_536_001})),
        ("set_timer", json!({"text":"x","seconds":1.5})),
        ("set_timer", json!({"text":"x","seconds":-1})),
        ("set_timer", json!({"text":"x","seconds":1,"guard":{}})),
        ("set_timer", json!({"text":"   ","seconds":1})),
        ("set_timer", json!({"text":"private\ntext","seconds":1})),
        ("set_timer", json!({"text":"é".repeat(151),"seconds":1})),
        ("set_reminder", json!({"text":"x","time":"24:00"})),
        ("set_reminder", json!({"text":"x","time":"9:00"})),
        ("set_reminder", json!({"text":"x","time":"12:60"})),
        (
            "set_reminder",
            json!({"text":"x","time":"12:00","day":"2026-02-29"}),
        ),
        (
            "set_reminder",
            json!({"text":"x","time":"12:00","day":"Friday"}),
        ),
        (
            "set_reminder",
            json!({"text":"x","time":"12:00","zone":"+07:00"}),
        ),
        (
            "set_reminder",
            json!({"text":"x","time":"12:00","fold":"first"}),
        ),
        (
            "repeat_reminder",
            json!({"text":"x","time":"12:00","every":"weekly"}),
        ),
        (
            "repeat_reminder",
            json!({"text":"x","time":"12:00","every":"daily","weekday":"monday"}),
        ),
        (
            "repeat_reminder",
            json!({"text":"x","time":"12:00","every":"monthly"}),
        ),
        ("list_reminders", json!({"after_id":-1})),
        ("list_reminders", json!({"pending_only":"yes"})),
        ("list_reminders", json!({"limit":1000})),
        ("list_reminders", json!({"pending_only":null})),
        (
            "set_reminder",
            json!({"text":"x","time":"12:00","zone":null}),
        ),
        (
            "repeat_reminder",
            json!({"text":"x","time":"12:00","every":"daily","weekday":null}),
        ),
        ("list_reminders", Value::Null),
        ("cancel_reminder", json!({"id":0})),
        ("cancel_reminder", json!({"id":1,"delivery":1})),
        ("snooze_reminder", json!({"id":1,"seconds":0})),
        ("snooze_reminder", json!({"id":1,"seconds":1,"delivery":1})),
    ];
    for (name, args) in cases {
        assert_eq!(
            adapter.call(name, &args)["code"],
            "bad_arguments",
            "{name}: {args}"
        );
    }
    let actions = Actions::new(Some(adapter));
    assert_eq!(
        actions.raw(
            json!({"op":"call","name":"set_timer","args":{"text":"x","seconds":1},"guard":{}})
        )["code"],
        "bad_arguments"
    );
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
}

#[test]
fn lost_commit_reply_is_unknown_and_never_retried_with_another_guard() {
    let mut script = Script::new(vec![status(), Value::Null]);
    let actions = Actions::new(Some(script.adapter()));
    let reply = actions.call("set_timer", json!({"text":"private label","seconds":600}));
    assert_eq!(reply["code"], "uncertain");
    assert!(
        reply["error"]
            .as_str()
            .unwrap()
            .contains("may have happened")
    );
    assert!(!reply.to_string().contains("private label"));
    let requests = script.finish();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0]["op"], "status");
    assert_eq!(requests[1]["op"], "set");
    assert_eq!(requests[1]["guard"]["revision"], 0);
    assert_eq!(
        requests[1]["guard"]["request_id"].as_str().unwrap().len(),
        32
    );
}

#[test]
fn definite_conflicts_are_not_retried_and_storage_failures_stay_uncertain() {
    for (code, expected) in [
        ("conflict", "conflict"),
        ("storage", "uncertain"),
        ("uncertain", "uncertain"),
        ("timeout", "uncertain"),
        ("mystery", "uncertain"),
        ("full", "full"),
        ("bad_time", "bad_time"),
        ("not_found", "not_found"),
        ("busy", "unavailable"),
    ] {
        let mut script = Script::new(vec![
            status(),
            json!({"ok":false,"error":code,"message":"arbitrary private service text"}),
        ]);
        let got = script
            .adapter()
            .call("set_timer", &json!({"text":"x","seconds":1}));
        assert_eq!(got["code"], expected, "{code}");
        assert!(!got.to_string().contains("arbitrary private"));
        assert_eq!(script.finish().len(), 2);
    }
}

#[test]
fn incorrect_success_identity_revision_or_deadline_never_becomes_confirmation() {
    let mut malformed = vec![json!({"ok":true}), commit(0)];
    for (key, value) in [
        ("store_id", json!("f".repeat(32))),
        ("revision", json!(2)),
        ("delivery", json!(2)),
        ("local", json!("next week")),
        ("due_unix_ms", json!(-1)),
    ] {
        let mut reply = commit(1);
        reply[key] = value;
        malformed.push(reply);
    }
    for reply in malformed {
        let mut script = Script::new(vec![status(), reply]);
        assert_eq!(
            script
                .adapter()
                .call("set_timer", &json!({"text":"x","seconds":600}))["code"],
            "uncertain"
        );
        script.finish();
    }
}

fn listed(id: u64, text: String) -> Value {
    let c = commit(id);
    json!({"id":id,"delivery":1,"text":text,"kind":"reminder","state":"scheduled","due_unix_ms":c["due_unix_ms"],"local":c["local"],"repeat":null,"snoozed":false,"late":false})
}

#[test]
fn listing_keeps_complete_escaped_labels_and_returns_the_actual_remaining_cursor() {
    let text = "\\\"".repeat(150);
    assert_eq!(text.len(), 300);
    let items: Vec<_> = (1..=8).map(|id| listed(id, text.clone())).collect();
    let mut script = Script::with(2, move |request, index| {
        assert_eq!(request["op"], "list_set");
        let after = request["after_id"].as_u64().unwrap();
        if index == 0 {
            assert_eq!(after, 0);
        } else {
            assert!(after > 0 && after < 8);
        }
        json!({"ok":true,"store_id":STORE,"revision":8,"items":items.iter().filter(|v|v["id"].as_u64().unwrap()>after).cloned().collect::<Vec<_>>(),"next_after_id":null})
    });
    let actions = Actions::new(Some(script.adapter()));
    let first = actions.call("list_reminders", json!({}));
    let result = &first["result"];
    assert!(result.to_string().len() <= MAX_RESULT_BYTES);
    assert_eq!(result["complete"], false);
    let included = result["items"].as_array().unwrap();
    assert!(included.len() < 8 && !included.is_empty());
    assert!(included.iter().all(|v| v["text"] == text));
    assert_eq!(result["next_after_id"], included.last().unwrap()["id"]);
    let second = actions.call(
        "list_reminders",
        json!({"after_id":result["next_after_id"]}),
    );
    assert!(second["result"].to_string().len() <= MAX_RESULT_BYTES);
    assert!(
        second["result"]["items"][0]["id"].as_u64().unwrap()
            > result["next_after_id"].as_u64().unwrap()
    );
    script.finish();
}

#[test]
fn empty_filtered_list_and_service_pagination_remain_explicit() {
    for (items, next, pending, complete) in [
        (json!([]), Value::Null, true, true),
        (json!([listed(3, "x".into())]), json!(3), false, false),
    ] {
        let mut script = Script::new(vec![
            json!({"ok":true,"store_id":STORE,"revision":1,"items":items,"next_after_id":next}),
        ]);
        let got = script.adapter().call(
            "list_reminders",
            &json!({"pending_only":pending,"after_id":2}),
        );
        assert_eq!(got["result"]["complete"], complete);
        assert_eq!(got["result"]["pending_only"], pending);
        script.finish();
    }
}

#[test]
fn malformed_lists_and_snooze_nonpending_state_never_make_a_mutation() {
    for response in [
        json!({"items":[listed(2,"x".into()),listed(1,"x".into())],"next_after_id":null}),
        json!({"items":[listed(1,"x".into())],"next_after_id":2}),
        json!({"items":[],"next_after_id":1}),
    ] {
        let mut reply = status();
        reply
            .as_object_mut()
            .unwrap()
            .extend(response.as_object().unwrap().clone());
        let mut script = Script::new(vec![reply]);
        assert_eq!(
            script.adapter().call("list_reminders", &json!({}))["code"],
            "unavailable"
        );
        script.finish();
    }
    let mut reply = status();
    reply["item"] = listed(1, "x".into());
    let mut script = Script::new(vec![reply]);
    assert_eq!(
        script
            .adapter()
            .call("snooze_reminder", &json!({"id":1,"seconds":10}))["code"],
        "not_pending"
    );
    assert_eq!(script.finish().len(), 1);
}

#[test]
fn all_adapter_hops_share_one_deadline_and_uncertain_call_is_not_reissued() {
    let mut script = Script::with(2, |_, index| {
        if index == 0 {
            thread::sleep(Duration::from_millis(150));
            status()
        } else {
            thread::sleep(DEADLINE);
            commit(1)
        }
    });
    let got = script
        .adapter()
        .call("set_timer", &json!({"text":"x","seconds":600}));
    assert_eq!(got["code"], "uncertain");
    assert_eq!(script.finish().len(), 2);
}

#[test]
fn missing_schedule_socket_is_truthful_and_mutation_free() {
    let root = root();
    let actions = Actions::new(Some(Adapter::new(root.path().join("missing")).unwrap()));
    assert_eq!(
        actions.call("set_timer", json!({"text":"x","seconds":1}))["code"],
        "unavailable"
    );
    assert_eq!(
        actions.call("list_reminders", json!({}))["code"],
        "unavailable"
    );
}

#[derive(Clone, Default)]
struct FaultDisk(Arc<Mutex<FaultState>>);
#[derive(Default)]
struct FaultState {
    bytes: Option<Vec<u8>>,
    fail: bool,
    after_commit: bool,
}
impl lamp_schedule::engine::Persistence for FaultDisk {
    fn load(&self) -> io::Result<Option<Vec<u8>>> {
        Ok(self.0.lock().unwrap().bytes.clone())
    }
    fn commit(&mut self, bytes: &[u8]) -> io::Result<()> {
        let mut state = self.0.lock().unwrap();
        if !state.fail || state.after_commit {
            state.bytes = Some(bytes.to_vec());
        }
        if state.fail {
            Err(io::Error::other("authored commit fault"))
        } else {
            Ok(())
        }
    }
}

#[test]
fn real_engine_failure_before_or_after_snapshot_commit_is_never_confirmed() {
    for after_commit in [false, true] {
        let disk = FaultDisk::default();
        let mut engine =
            lamp_schedule::engine::Engine::open(disk.clone(), "Etc/UTC", &sample(0), STORE)
                .unwrap();
        {
            let mut state = disk.0.lock().unwrap();
            state.fail = true;
            state.after_commit = after_commit;
        }
        let mut script = Script::with(2, move |request, _| {
            let request = serde_json::from_value(request.clone()).unwrap();
            match engine.answer(request, &sample(0)) {
                Ok(value) => value,
                Err(error) => lamp_schedule::protocol::refusal(error.code, error.message),
            }
        });
        let actions = Actions::new(Some(script.adapter()));
        let reply = actions.call("set_timer", json!({"text":"commit fault","seconds":30}));
        assert_eq!(reply["code"], "uncertain");
        assert_eq!(script.finish().len(), 2);
        let state: Value =
            serde_json::from_slice(disk.0.lock().unwrap().bytes.as_ref().unwrap()).unwrap();
        assert_eq!(
            state["items"].as_array().unwrap().len(),
            usize::from(after_commit)
        );
    }
}

#[test]
fn host_first_and_repeated_actions_to_durable_schedule_receipts_are_measured() {
    let schedule = Schedule::new();
    let actions = Actions::new(Some(schedule.adapter()));
    let mut durations = Vec::new();
    let mut failures = 0;
    for index in 0..21 {
        let started = Instant::now();
        let reply = actions.call(
            "set_timer",
            json!({"text":format!("host timing {index}"),"seconds":600}),
        );
        durations.push(started.elapsed().as_micros() as u64);
        if reply["result"]["status"] != "committed" {
            failures += 1;
        }
    }
    // Third's P3 on #1603: a list of what is due now also reads Status, so
    // it is timed too (within the same one deadline as every call).
    let mut due_lists = Vec::new();
    for _ in 0..10 {
        let started = Instant::now();
        let reply = actions.call("list_reminders", json!({"pending_only":true}));
        due_lists.push(started.elapsed().as_micros() as u64);
        if reply["result"]["not_yet_due"] != 21 {
            failures += 1;
        }
    }
    due_lists.sort_unstable();
    let first = durations.remove(0);
    durations.sort_unstable();
    let percentile = |p: usize| durations[(p * durations.len()).div_ceil(100) - 1];
    eprintln!(
        "schedule_actions_timing {}",
        json!({
            "boundary":"client entry before real Actions socket connect to parsed durable Schedule response through Actions",
            "machine_os":std::env::consts::OS,"machine_arch":std::env::consts::ARCH,
            "fixture":"real Actions server and real Schedule service, private SQLite FULL store, injected Schedule clock; no provider or lamp",
            "first_us":first,"repeated_count":durations.len(),"total_count":21,"failures":failures,
            "repeated_p50_us":percentile(50),"repeated_p95_us":percentile(95),"repeated_max_us":durations.last().unwrap(),
            "target_us":250_000,"target_is_not_a_functional_assertion":true,
            "cold_scope":"first mutation after both newly started host services published readiness; process/store startup excluded",
            "pending_only_list_p50_us":due_lists[4],"pending_only_list_max_us":due_lists[9]
        })
    );
    assert_eq!(failures, 0);
}

/// #1595, lamp-4ace on 18 Oct at 19:40: "cancel my timer" 15 s after a
/// two-minute timer was set listed with `pending_only`, which holds only
/// items already due, so the list was empty, the model said nothing was
/// set, and the timer rang. A just-set timer is in the plain list; a list of
/// what is due says how many are set and not yet due, and tells the model
/// to list again; cancelled by its id, it is gone and never comes due.
#[test]
fn a_just_set_timer_is_listed_counted_beside_a_due_only_list_and_cancelled_for_good() {
    let schedule = Schedule::new();
    let actions = Actions::new(Some(schedule.adapter()));
    let timer = actions.call("set_timer", json!({"text":"Tea","seconds":120}));
    assert_eq!(timer["result"]["status"], "committed");
    let id = timer["result"]["id"].as_u64().unwrap();
    *schedule.clock.0.lock().unwrap() = sample(15_000);
    let all = actions.call("list_reminders", json!({}));
    assert_eq!(all["result"]["items"][0]["id"], id);
    assert!(all["result"].get("not_yet_due").is_none(), "{all}");
    let due = actions.call("list_reminders", json!({"pending_only":true}));
    assert_eq!(due["result"]["items"], json!([]), "{due}");
    assert_eq!(due["result"]["not_yet_due"], 1, "{due}");
    assert_eq!(due["result"]["next"], lamp_actions::schedule::NOT_YET_DUE);
    assert!(due["result"].to_string().len() <= MAX_RESULT_BYTES);
    assert_eq!(
        actions.call("cancel_reminder", json!({"id":id}))["result"]["cancelled"],
        true
    );
    // Past its time it is neither due nor set.
    *schedule.clock.0.lock().unwrap() = sample(130_000);
    wait(|| ask(&schedule.socket, json!({"op":"status"}))["items"] == 0);
    let after = actions.call("list_reminders", json!({"pending_only":true}));
    assert_eq!(after["result"]["items"], json!([]));
    assert!(after["result"].get("not_yet_due").is_none(), "{after}");
    // The schema says what pending_only means, and cancel lists without it.
    let listing = catalog()
        .into_iter()
        .find(|f| f.name == "list_reminders")
        .unwrap();
    let said = serde_json::to_string(&listing.parameters).unwrap();
    assert!(said.contains("due now and not yet said"), "{said}");
    let cancel = catalog()
        .into_iter()
        .find(|f| f.name == "cancel_reminder")
        .unwrap();
    assert!(cancel.description.contains("without pending_only"));
}

/// #1595, its last part (First, 11:39 on 19 Oct): each listed item says its
/// kind, so "cancel my timer" with a timer and a reminder both due picks the
/// timer and leaves the reminder. A snoozed timer is still a timer.
#[test]
fn cancel_my_timer_with_a_reminder_also_due_cancels_only_the_timer() {
    let schedule = Schedule::new();
    let actions = Actions::new(Some(schedule.adapter()));
    let timer = actions.call("set_timer", json!({"text":"Pasta","seconds":60}));
    let reminder = actions.call(
        "set_reminder",
        json!({"text":"Call the plumber","time":"08:02","day":"today","zone":"Etc/UTC"}),
    );
    let (timer, reminder) = (
        timer["result"]["id"].as_u64().unwrap(),
        reminder["result"]["id"].as_u64().unwrap(),
    );
    *schedule.clock.0.lock().unwrap() = sample(130_000);
    wait(|| ask(&schedule.socket, json!({"op":"status"}))["pending"] == 2);
    let due = actions.call("list_reminders", json!({"pending_only":true}));
    let kinds: Vec<_> = due["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| (i["id"].as_u64().unwrap(), i["kind"].clone()))
        .collect();
    assert_eq!(
        kinds,
        [(timer, json!("timer")), (reminder, json!("reminder"))],
        "{due}"
    );
    // What the model does for "cancel my timer": the item whose kind is timer.
    let picked = kinds.iter().find(|(_, k)| k == "timer").unwrap().0;
    assert_eq!(
        actions.call("cancel_reminder", json!({"id":picked}))["result"]["cancelled"],
        true
    );
    let left = actions.call("list_reminders", json!({}));
    assert_eq!(
        left["result"]["items"].as_array().unwrap().len(),
        1,
        "{left}"
    );
    assert_eq!(left["result"]["items"][0]["id"], reminder);
    assert_eq!(left["result"]["items"][0]["kind"], "reminder");
    assert_eq!(left["result"]["items"][0]["state"], "pending");
    // A snoozed timer keeps its kind.
    let again = actions.call("set_timer", json!({"text":"Eggs","seconds":60}));
    let again = again["result"]["id"].as_u64().unwrap();
    *schedule.clock.0.lock().unwrap() = sample(200_000);
    wait(|| ask(&schedule.socket, json!({"op":"status"}))["pending"] == 2);
    let snoozed = actions.call("snooze_reminder", json!({"id":again,"seconds":60}));
    assert_eq!(snoozed["result"]["snoozed"], true, "{snoozed}");
    let all = actions.call("list_reminders", json!({}));
    let eggs = all["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == again)
        .unwrap();
    assert_eq!(eggs["kind"], "timer", "{all}");
    let cancel = catalog()
        .into_iter()
        .find(|f| f.name == "cancel_reminder")
        .unwrap();
    assert!(cancel.description.contains("kind is timer"));
}

/// The reviewer's P3 on #1603: a `pending_only` page kept full of long due
/// labels still has room for `not_yet_due` and its `next`, so a list of what
/// is due, with one more item set and not yet due, fits the provider's
/// allowance and says the count, instead of failing as uncertain.
#[test]
fn a_full_due_only_page_still_fits_its_count_of_what_is_not_yet_due() {
    // Labels of quotes (two bytes each once escaped) of every length from
    // 300 down to 150, so that some page lands just under the allowance
    // without the room kept for the count.
    for length in (150..=300).rev() {
        let text = "\"".repeat(length);
        let items: Vec<_> = (1..=8)
            .map(|id| {
                let mut item = listed(id, text.clone());
                item["state"] = json!("pending");
                item
            })
            .collect();
        let mut script = Script::with(2, move |request, index| match index {
            0 => {
                assert_eq!(request["op"], "list_set");
                assert_eq!(request["pending_only"], true);
                json!({"ok":true,"store_id":STORE,"revision":1,"items":items,"next_after_id":null})
            }
            _ => {
                assert_eq!(request["op"], "status");
                json!({"ok":true,"store_id":STORE,"revision":1,"items":9,"pending":8})
            }
        });
        let got = script
            .adapter()
            .call("list_reminders", &json!({"pending_only":true}));
        let result = &got["result"];
        assert!(
            result.to_string().len() <= MAX_RESULT_BYTES,
            "{length}: {}",
            result.to_string().len()
        );
        assert_eq!(result["not_yet_due"], 1, "{length}: {got}");
        assert_eq!(result["next"], lamp_actions::schedule::NOT_YET_DUE);
        // What did not fit is said by the cursor, never dropped.
        let shown = result["items"].as_array().unwrap().len() as u64;
        assert!(shown >= 1);
        if shown < 8 {
            assert_eq!(result["next_after_id"], shown, "{length}");
            assert_eq!(result["complete"], false);
        }
        script.finish();
    }
}

/// The clock the model says, from the schedule's `local`: the wall clock's
/// hour and minute, never the minute and second (lamp-4ace, 25 Oct).
#[test]
fn the_clock_is_the_local_hour_and_minute() {
    use lamp_actions::schedule::clock_of;
    assert_eq!(
        clock_of("2026-10-25T21:07:34.939+07:00[Asia/Ho_Chi_Minh]").as_deref(),
        Some("21:07")
    );
    assert_eq!(
        clock_of("2026-10-26T09:00:00+00:00[Etc/UTC]").as_deref(),
        Some("09:00")
    );
    assert_eq!(clock_of("next week"), None);
    assert_eq!(clock_of("2026-10-25T9:7"), None);
}

/// The person's call (30 Oct): asked when a reminder was set, the lamp says
/// it from the list. Each item set through the real service says its day
/// and local clock time; a day on it is `yesterday`.
#[test]
fn each_listed_item_says_when_it_was_set_and_a_day_on_it_is_yesterday() {
    let schedule = Schedule::new();
    let actions = Actions::new(Some(schedule.adapter()));
    *schedule.clock.0.lock().unwrap() = sample(3_600_000);
    let set = actions.call(
        "set_reminder",
        json!({"text":"Call Ana","time":"18:00","day":"tomorrow"}),
    );
    assert_eq!(set["result"]["status"], "committed", "{set}");
    let today = actions.call("list_reminders", json!({}));
    let item = &today["result"]["items"][0];
    assert_eq!(
        (item["set_day"].as_str(), item["set_clock"].as_str()),
        (Some("today"), Some("09:00")),
        "{today}"
    );
    *schedule.clock.0.lock().unwrap() = sample(25 * 3_600_000);
    let next_day = actions.call("list_reminders", json!({}));
    let item = &next_day["result"]["items"][0];
    assert_eq!(
        (item["set_day"].as_str(), item["set_clock"].as_str()),
        (Some("yesterday"), Some("09:00")),
        "{next_day}"
    );
}

/// An older Schedule service's item has no set time, and one whose set time
/// is malformed is not told: both list with it null, never guessed, and
/// the rest of the item stands.
#[test]
fn an_item_without_a_readable_set_time_lists_with_it_unknown() {
    let mut odd = listed(2, "Bins".into());
    odd["set_local"] = json!("yesterday morning");
    odd["set_day"] = json!("yesterday");
    let mut good = listed(3, "Tea".into());
    good["set_local"] = json!("2026-10-13T21:05:00+07:00[Asia/Ho_Chi_Minh]");
    good["set_day"] = json!("2026-10-13");
    let mut script = Script::new(vec![json!({"ok":true,"store_id":STORE,"revision":3,
        "items":[listed(1, "Call Ana".into()), odd, good],"next_after_id":null})]);
    let got = script.adapter().call("list_reminders", &json!({}));
    let items = got["result"]["items"].as_array().unwrap();
    let said: Vec<_> = items
        .iter()
        .map(|i| {
            (
                i["text"].as_str().unwrap(),
                i["set_day"].clone(),
                i["set_clock"].clone(),
            )
        })
        .collect();
    assert_eq!(
        said,
        [
            ("Call Ana", Value::Null, Value::Null),
            ("Bins", Value::Null, Value::Null),
            ("Tea", json!("2026-10-13"), json!("21:05")),
        ],
        "{got}"
    );
    script.finish();
}

/// An older Schedule service refuses `list_set` as a bad request: the same
/// list is asked once as `list`, its items told as set at an unknown time.
/// Any other refusal is not asked again.
#[test]
fn an_older_schedule_is_listed_without_set_times() {
    let mut script = Script::with(2, |request, index| match index {
        0 => {
            assert_eq!(request["op"], "list_set");
            json!({"ok":false,"error":"bad_request","message":"x"})
        }
        _ => {
            assert_eq!(request["op"], "list");
            assert_eq!(request["after_id"], 0);
            json!({"ok":true,"store_id":STORE,"revision":1,"items":[listed(1, "Call Ana".into())],"next_after_id":null})
        }
    });
    let got = script.adapter().call("list_reminders", &json!({}));
    let item = &got["result"]["items"][0];
    assert_eq!(item["text"], "Call Ana", "{got}");
    assert_eq!(
        (&item["set_day"], &item["set_clock"]),
        (&Value::Null, &Value::Null)
    );
    assert_eq!(script.finish().len(), 2);
    let mut script = Script::new(vec![json!({"ok":false,"error":"busy","message":"x"})]);
    assert_eq!(
        script.adapter().call("list_reminders", &json!({}))["code"],
        "unavailable"
    );
    assert_eq!(script.finish().len(), 1);
}

/// `list_reminders`' description names questions about reminders as its
/// own, not recall's, says how the set time reads, and never lists unasked
/// (rule 8); it stays within a function's 1,024 bytes.
#[test]
fn list_reminders_is_described_for_questions_about_reminders() {
    let listing = catalog()
        .into_iter()
        .find(|f| f.name == "list_reminders")
        .unwrap();
    assert_eq!(listing.description, lamp_actions::schedule::LIST_DESCRIBED);
    assert!(
        listing
            .description
            .starts_with(lamp_actions::schedule::LIST_BEFORE)
    );
    for words in [
        "not recall",
        "when they set it",
        "never say you do not remember a reminder before calling this",
        "set_day and set_clock",
        "say you don't know when",
        "Never list reminders unasked",
    ] {
        assert!(listing.description.contains(words), "{words}");
    }
    assert!(
        listing.description.len() <= 1_024,
        "{}",
        listing.description.len()
    );
}

const ANN: &str = "p-0123456789abcdef";
const BO: &str = "p-fedcba9876543210";

fn caller(person: Option<&str>, owner: bool) -> Value {
    json!({"asker":{"person":person,"owner":owner},"setter":person})
}
fn texts(list: &Value) -> Vec<String> {
    list["result"]["items"]
        .as_array()
        .unwrap_or_else(|| panic!("{list}"))
        .iter()
        .map(|i| i["text"].as_str().unwrap().to_owned())
        .collect()
}

/// Through both real servers, callers see their own and the room's items,
/// including long relative reminders. Other people's items stay inaccessible.
#[test]
fn each_person_hears_their_own_and_the_rooms_reminders_through_the_real_server() {
    let schedule = Schedule::new();
    let actions = Actions::new(Some(schedule.adapter()));
    let owner = caller(Some(ANN), true);
    let bo = caller(Some(BO), false);
    let unknown = caller(None, false);
    // Reminders, due at 08:10, 600 s after `WALL`: a short timer set by
    // nobody is every line's.
    let set = |text: &str, who: &Value| {
        let told = actions.call_as(
            "set_reminder",
            json!({"text":text,"time":"08:10"}),
            who.clone(),
        );
        assert_eq!(told["result"]["status"], "committed", "{told}");
        told["result"]["id"].as_u64().unwrap()
    };
    let pills = set("Ann's pills", &owner);
    set("Bo's call", &bo);
    let plant = set("Water the plant", &unknown);
    let list = |who: &Value| actions.call_as("list_reminders", json!({}), who.clone());
    assert_eq!(texts(&list(&owner)), ["Ann's pills", "Water the plant"]);
    assert_eq!(texts(&list(&bo)), ["Bo's call", "Water the plant"]);
    assert_eq!(texts(&list(&unknown)), ["Water the plant"]);
    // Not due yet: Bo is told of his own and the room's reminder.
    let due = actions.call_as("list_reminders", json!({"pending_only":true}), bo.clone());
    assert_eq!(due["result"]["not_yet_due"], 2, "{due}");
    // A call without its caller has only the room's view.
    let bare = actions.raw(json!({"op":"call","name":"list_reminders","args":{}}));
    assert_eq!(texts(&bare), ["Water the plant"]);
    for (id, who) in [(pills, &bo), (pills, &unknown)] {
        let refused = actions.call_as("cancel_reminder", json!({"id":id}), who.clone());
        assert_eq!(refused["code"], "not_found", "{refused}");
    }
    *schedule.clock.0.lock().unwrap() = sample(601_000);
    let refused = actions.call_as(
        "snooze_reminder",
        json!({"id":pills,"seconds":60}),
        bo.clone(),
    );
    assert_eq!(refused["code"], "not_found", "{refused}");
    // The app's view, the owner's, is every item.
    assert_eq!(
        texts(
            &json!({"result":schedule.adapter().call_plain("list_reminders", &json!({}))["result"]})
        ),
        ["Ann's pills", "Bo's call", "Water the plant"]
    );
    let snoozed = actions.call_as(
        "snooze_reminder",
        json!({"id":pills,"seconds":60}),
        owner.clone(),
    );
    assert_eq!(snoozed["result"]["status"], "committed", "{snoozed}");
    let cancelled = actions.call_as("cancel_reminder", json!({"id":plant}), unknown.clone());
    assert_eq!(cancelled["result"]["cancelled"], true, "{cancelled}");
    // With no face seen, both short timers and long reminders are the room's.
    let pasta = actions.call_as(
        "set_timer",
        json!({"text":"Pasta","seconds":600}),
        unknown.clone(),
    );
    let pasta = pasta["result"]["id"].as_u64().unwrap();
    let weeks = actions.call_as(
        "set_timer",
        json!({"text":"Renew the visa","seconds":21 * 86_400}),
        unknown.clone(),
    );
    assert_eq!(weeks["result"]["status"], "committed", "{weeks}");
    assert_eq!(texts(&list(&unknown)), ["Pasta", "Renew the visa"]);
    let cancelled = actions.call_as("cancel_reminder", json!({"id":pasta}), unknown.clone());
    assert_eq!(cancelled["result"]["cancelled"], true, "{cancelled}");
    assert_eq!(texts(&list(&unknown)), ["Renew the visa"]);
    let cancelled = actions.call_as(
        "cancel_reminder",
        json!({"id":weeks["result"]["id"]}),
        bo.clone(),
    );
    assert_eq!(cancelled["result"]["cancelled"], true, "{cancelled}");
    assert!(texts(&list(&unknown)).is_empty());
}

/// "Forget everything about me": the voice's own op cancels every reminder
/// the person set, and nobody else's. A model can't name it: it is no tool.
#[test]
fn a_persons_forget_cancels_their_reminders_only_through_the_voices_own_op() {
    let schedule = Schedule::new();
    let actions = Actions::new(Some(schedule.adapter()));
    for (text, who) in [("Ann's pills", ANN), ("Ann's call", ANN), ("Bo's call", BO)] {
        let told = actions.call_as(
            "set_timer",
            json!({"text":text,"seconds":600}),
            caller(Some(who), false),
        );
        assert_eq!(told["result"]["status"], "committed", "{told}");
    }
    let forgot = actions.raw(json!({"op":"schedule_forget","person":ANN}));
    assert_eq!(forgot["result"]["forgotten"], 2, "{forgot}");
    assert_eq!(
        texts(
            &json!({"result":schedule.adapter().call_plain("list_reminders", &json!({}))["result"]})
        ),
        ["Bo's call"]
    );
    let refused = actions.raw(json!({"op":"schedule_forget","person":"Ann"}));
    assert_eq!(refused["code"], "bad_arguments", "{refused}");
    let model = actions.call_as(
        "schedule_forget",
        json!({"person":BO}),
        caller(Some(BO), false),
    );
    assert_ne!(model["ok"], true, "{model}");
    assert_eq!(
        texts(
            &json!({"result":schedule.adapter().call_plain("list_reminders", &json!({}))["result"]})
        ),
        ["Bo's call"]
    );
}
