//! News heard closes the harness it was of (#1390), end to end: the voice's
//! task news reads a real `lamp-tasks serve --close-told` on its two
//! sockets, the news is told, and once its turn was heard whole the tasks
//! part is told so and closes the harness it started. On lamp-4ace on
//! 2026-10-18 the voice told task 147 and nothing closed: its news path
//! never said so. No model, no lamp: Harness is a stand-in that answers at
//! once, and the coordinator's proof that a turn was heard whole is
//! `coordinator::tests::say`'s.
use lamp_tasks::{
    engine::{CLOSE_RETRY_MS, Clock as Sample},
    harness::{Closed, EngineKind, Launch, LaunchOutcome, Observation, Target},
    runtime::{
        Adapter, Clock, Config, DAEMON_RETRY_AFTER, REPORT_EVERY, request, serve_with_adapter,
    },
};
use lamp_voice::task_news::{BootClock, TaskNews};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const AGENT: &str = "11111111-1111-4111-8111-111111111111";
const MACHINE: &str = "0123456789abcdef0123456789abcdef";
const TASK: &str = "Suggest three friendly names for a desk lamp.";
const WAIT: Duration = Duration::from_secs(5);
/// How long the service is given to do what it must not: close untold.
const NOTHING_FOR: Duration = Duration::from_millis(300);

/// The one clock both parts read: the tasks part's monotonic time, and the
/// voice's (`start_with_clock`). Only ever stepped forward.
static NOW_MS: AtomicU64 = AtomicU64::new(1_000);

fn voice_clock() -> Option<BootClock> {
    Some(BootClock {
        boot_id: "test-boot".into(),
        ms: NOW_MS.load(Ordering::SeqCst),
    })
}

#[derive(Clone)]
struct Shared;
impl Clock for Shared {
    fn sample(&self) -> io::Result<Sample> {
        Ok(Sample {
            boot_id: "test-boot".into(),
            monotonic_ms: NOW_MS.load(Ordering::SeqCst),
        })
    }
}

/// Harness: the task starts at once, its agent is idle with its answer,
/// and each close is kept.
struct Harness(Arc<Mutex<Vec<String>>>);
impl Adapter for Harness {
    fn launch(&self, _: &Launch) -> LaunchOutcome {
        LaunchOutcome::Created {
            agent_id: AGENT.into(),
        }
    }
    fn observe(&self, _: &str, _: &str) -> Result<Observation, String> {
        Ok(Observation {
            agent_id: AGENT.into(),
            state: "idle".into(),
            ask: Some(TASK.into()),
            answer: "Lumi, Sunny and Archie.".into(),
            truncated: false,
            tools: 0,
        })
    }
    fn close(&self, machine: &str, id: &str) -> Result<Closed, String> {
        assert_eq!(machine, MACHINE);
        self.0.lock().unwrap().push(id.into());
        Ok(Closed::Closed)
    }
}

struct Place {
    root: PathBuf,
    config: Config,
    stop: Arc<AtomicBool>,
    closes: Arc<Mutex<Vec<String>>>,
    serving: Option<JoinHandle<io::Result<()>>>,
    /// The tasks part syncs its store.
    _flushing: crate::disk_flush::Flushing,
}

impl Place {
    fn new(name: &str) -> Self {
        let flushing = crate::disk_flush::flushing();
        let root = PathBuf::from(format!("/tmp/lvt-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let config = Config {
            enabled: true,
            dir: root.join("data"),
            socket: root.join("tasks/request.sock"),
            voice_socket: root.join("tasks/voice.sock"),
            bus_root: root.join("b"),
            targets: BTreeMap::from([(
                "coding".into(),
                Target {
                    machine_id: MACHINE.into(),
                    project: "project".into(),
                    engine: EngineKind::Claude,
                },
            )]),
            run_for: None,
            watch: false,
            progress: false,
            push_wait: false,
            close_told: true,
            targets_newer: None,
            set_up_later: None,
            sign_in_by_code: false,
            report_every: REPORT_EVERY,
            daemon_retry_after: DAEMON_RETRY_AFTER,
        };
        let mut place = Self {
            root,
            config,
            stop: Arc::new(AtomicBool::new(false)),
            closes: Arc::new(Mutex::new(Vec::new())),
            serving: None,
            _flushing: flushing,
        };
        place.serve();
        place
    }

    /// The tasks part, started on its store.
    fn serve(&mut self) {
        self.stop = Arc::new(AtomicBool::new(false));
        let (config, stop, closes) = (self.config.clone(), self.stop.clone(), self.closes.clone());
        self.serving = Some(thread::spawn(move || {
            serve_with_adapter(config, Shared, stop, move |_| Ok(Harness(closes)))
        }));
        self.until(|| self.ask(json!({"op":"status"}))["ok"] == true);
    }

    /// The tasks part, stopped: its sockets answer nothing.
    fn halt(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(serving) = self.serving.take() {
            serving.join().unwrap().unwrap();
        }
    }

    fn tasks(&self) -> &Path {
        self.config.socket.parent().unwrap()
    }

    fn ask(&self, line: Value) -> Value {
        request(&self.config.socket, &line, Duration::from_secs(2)).unwrap_or_default()
    }

    fn trusted(&self, line: Value) -> Value {
        request(&self.config.voice_socket, &line, Duration::from_secs(2)).unwrap()
    }

    fn guard(&self) -> Value {
        let status = self.ask(json!({"op":"status"}));
        json!({"store_id":status["store_id"],"revision":status["revision"],
            "request_id":format!("test-{}", status["revision"])})
    }

    fn task(&self, id: &Value) -> Value {
        self.ask(json!({"op":"get_task","id":id}))
    }

    fn closed(&self) -> Vec<String> {
        self.closes.lock().unwrap().clone()
    }

    /// A failure names the line that waited (#1569: one failed once in a
    /// loaded run, and its panic was not kept).
    #[track_caller]
    fn until(&self, done: impl Fn() -> bool) {
        let begun = Instant::now();
        while !done() {
            assert!(begun.elapsed() < WAIT, "not within {WAIT:?}");
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Handed over as the voice does it (prepared, then the person's yes on
    /// the trusted socket), and its agent's answer read and kept. Not
    /// tracked: a timeout names which of its two waits ran out.
    fn handed_and_read(&self) -> Value {
        let prepared =
            self.ask(json!({"op":"prepare","guard":self.guard(),"text":TASK,"target":"coding"}));
        assert_eq!(prepared["ok"], true, "{prepared}");
        let id = prepared["id"].clone();
        let session = self.ask(json!({"op":"status"}))["summary"]["session_id"].clone();
        let confirmed = self.trusted(json!({"op":"confirm","guard":self.guard(),"id":id,
            "prepared_digest":prepared["prepared_digest"],"session_id":session}));
        assert_eq!(confirmed["ok"], true, "{confirmed}");
        self.until(|| self.task(&id)["task"]["dispatch"]["state"] == "accepted");
        let observe = self.ask(json!({"op":"observe","guard":self.guard(),"id":id}));
        assert_eq!(observe["ok"], true, "{observe}");
        self.until(|| {
            self.task(&id)["observations"]
                .as_array()
                .is_some_and(|seen| !seen.is_empty())
        });
        id
    }

    /// The news of task `id` the voice's reader offers, taken to tell: its
    /// key.
    #[track_caller]
    fn told(&self, news: &mut TaskNews, id: &Value) -> String {
        let begun = Instant::now();
        let key = loop {
            if let Some(waiting) = news.waiting() {
                break waiting.key.clone();
            }
            assert!(begun.elapsed() < WAIT, "no news offered");
            thread::sleep(Duration::from_millis(10));
        };
        let observation = self.task(id)["observations"][0]["id"].clone();
        let store = self.ask(json!({"op":"status"}))["store_id"].clone();
        assert_eq!(
            key,
            format!("{}:{id}:{observation}", store.as_str().unwrap())
        );
        assert_eq!(news.tell().unwrap().key, key);
        key
    }

    /// Nothing closed, nor marked told, though by the clock a close would
    /// be due three times over.
    #[track_caller]
    fn nothing_closed(&self, id: &Value) {
        NOW_MS.fetch_add(3 * CLOSE_RETRY_MS, Ordering::SeqCst);
        thread::sleep(NOTHING_FOR);
        assert!(self.closed().is_empty(), "closed untold");
        assert!(self.task(id)["observations"][0]["told"].is_null());
    }
}

impl Drop for Place {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(serving) = self.serving.take() {
            let _ = serving.join();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// The news is read from the tasks part and taken to tell: that closes
/// nothing (the reviewer on #1425: a Stop or a cut-off may follow), though
/// it is kept as told at once. Once the coordinator hands it on as heard
/// whole, the tasks part hears so on its trusted socket, marks the
/// observation `told`, and closes the harness the lamp started, once.
#[test]
fn news_heard_whole_closes_the_harness_it_was_of() {
    let place = Place::new("heard");
    let id = place.handed_and_read();
    let mut news = TaskNews::start_with_clock(
        place.tasks(),
        place.root.join("told"),
        None,
        false,
        voice_clock,
    )
    .unwrap();
    let key = place.told(&mut news, &id);
    place.until(|| {
        fs::read_to_string(place.root.join("told"))
            .is_ok_and(|told| told.lines().any(|line| line == key))
    });
    place.nothing_closed(&id);

    news.heard(5, vec![key]);
    place.until(|| !place.closed().is_empty());
    let task = place.task(&id);
    assert!(task["observations"][0]["told"].is_object(), "{task}");
    place.until(|| place.task(&id)["task"]["close"]["state"] == "closed");
    assert_eq!(place.closed(), [AGENT]);
}

/// The reviewer on #1425: news heard while the tasks part is down (here,
/// restarting) is not lost. It is sent again at the next pass, which the
/// next piece of news heard brings forward, and the harness is closed. A
/// hand-over's news is of no observation, and is never sent itself.
#[test]
fn news_heard_while_the_tasks_part_is_down_is_told_once_it_is_back() {
    let mut place = Place::new("down");
    let id = place.handed_and_read();
    let mut news = TaskNews::start_with_clock(
        place.tasks(),
        place.root.join("told"),
        None,
        false,
        voice_clock,
    )
    .unwrap();
    let key = place.told(&mut news, &id);
    place.halt();
    news.heard(5, vec![key]);
    thread::sleep(NOTHING_FOR);
    place.serve();
    place.nothing_closed(&id);
    news.heard(5, vec!["s1:9:hand-over".into()]);
    place.until(|| !place.closed().is_empty());
    let task = place.task(&id);
    assert!(task["observations"][0]["told"].is_object(), "{task}");
    assert_eq!(place.closed(), [AGENT]);
}
