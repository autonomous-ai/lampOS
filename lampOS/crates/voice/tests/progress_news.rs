//! Progress news end to end on a real bus of the test's own (#752): the
//! tasks part says a change on `tasks/status`, the policy decides
//! `say_now` and acts, and the voice's task news offers one note to tell,
//! with the words fetched from a stand-in tasks socket. No model, no Harness,
//! no lamp: the policy's judge is a stand-in that answers at once.
use lamp_bus::{Bus, Publisher};
use lamp_msgs::{
    AgentState, Decision, Sign, TaskProgress, TaskSettled, TaskStatus, Voice, VoiceState,
};
use lamp_policy::{
    decide::{Answer, Asked, Decider, Failed, Question},
    policy::{self, Options},
    progress,
};
use lamp_voice::task_news::{BootClock, News, TaskNews, progress_on};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixListener,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const STORE: &str = "0123456789abcdef0123456789abcdef";
const WAIT: Duration = Duration::from_secs(5);
const ABOUT: &str = "Fixed the tokenizer's end index / all 118 tests pass";
/// Two runs of the tasks part: each counts changes from nothing.
const RUN_A: u64 = 1_791_964_000_000;
const RUN_B: u64 = 1_791_965_000_000;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// What the stand-in tasks socket says of task 3's progress, and how long
/// it takes to answer each request.
struct Snapshot {
    state: &'static str,
    changes: u32,
    /// The run of the tasks part, and the line it holds.
    run: u64,
    about: String,
    delay: Duration,
}

/// A place of the test's own: a bus, a tasks socket that answers as
/// `lamp-tasks serve --progress` would, and a file of what was told.
struct Place {
    root: PathBuf,
    bus: Bus,
    told: PathBuf,
    /// Every operation the tasks socket was asked.
    asked: Arc<Mutex<Vec<String>>>,
    /// What it answers for `get_progress`.
    snapshot: Arc<Mutex<Snapshot>>,
    status: Publisher,
    decisions: Publisher,
}

impl Place {
    fn new(name: &str) -> Self {
        let root = PathBuf::from(format!("/tmp/lvp-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("tasks")).unwrap();
        let bus = Bus::new(root.join("b")).unwrap().rescan_every(ms(20));
        let asked = Arc::new(Mutex::new(Vec::new()));
        let snapshot = Arc::new(Mutex::new(Snapshot {
            state: "working",
            changes: 1,
            run: RUN_A,
            about: ABOUT.into(),
            delay: Duration::ZERO,
        }));
        serve_tasks(
            &root.join("tasks/request.sock"),
            asked.clone(),
            snapshot.clone(),
        );
        Self {
            snapshot,
            told: root.join("told"),
            status: bus
                .publisher_declared(TaskStatus::TOPIC, &TaskStatus::declaration(), "tasks")
                .unwrap(),
            decisions: bus
                .publisher_declared(Decision::TOPIC, &Decision::declaration(), "policy")
                .unwrap(),
            asked,
            root,
            bus,
        }
    }

    /// The voice's task news, with progress on wherever there is a bus.
    fn news(&self, bus: Option<&Bus>) -> TaskNews {
        TaskNews::start(
            &self.root.join("tasks"),
            self.told.clone(),
            bus,
            bus.is_some(),
        )
        .unwrap()
    }

    /// The tasks part says task 3's agent settled and the watch let it go.
    fn settle(&mut self) {
        let run = self.snapshot.lock().unwrap().run;
        let status = TaskStatus {
            store_id: STORE.into(),
            run,
            unread: vec![],
            settled: vec![TaskSettled {
                task: 3,
                engine: "claude".into(),
                state: AgentState::Idle,
                settled: true,
                at_unix_ms: 1_792_022_712_345,
            }],
            tasks: Vec::new(),
        };
        status.check().unwrap();
        self.status.publish_json(0, &status).unwrap();
    }

    /// The tasks part says task 3 is at `changes`.
    fn work(&mut self, changes: u32) {
        self.work_as(AgentState::Working, changes);
    }

    /// The tasks part says task 3 is `state`, at `changes`.
    fn work_as(&mut self, state: AgentState, changes: u32) {
        let run = self.snapshot.lock().unwrap().run;
        let status = TaskStatus {
            store_id: STORE.into(),
            run,
            unread: vec![],
            settled: Vec::new(),
            tasks: vec![TaskProgress {
                task: 3,
                engine: "claude".into(),
                state,
                working_s: Some(600),
                last_active_unix_ms: None,
                read_unix_ms: 1_792_022_712_345,
                changes,
                changed: changes > 0,
                new_share: 0.6,
                signs: vec![Sign::TestsPass, Sign::Fixed],
                signs_seen: vec![Sign::TestsPass, Sign::Fixed],
            }],
        };
        status.check().unwrap();
        self.status.publish_json(0, &status).unwrap();
    }

    /// The tasks part no longer watches task 3 (it finished, or went).
    fn leave(&mut self) {
        let run = self.snapshot.lock().unwrap().run;
        let status = TaskStatus {
            store_id: STORE.into(),
            run,
            unread: vec![],
            settled: Vec::new(),
            tasks: Vec::new(),
        };
        self.status.publish_json(0, &status).unwrap();
    }

    /// What the tasks socket will say of task 3's progress.
    fn words(&self, state: &'static str, changes: u32) {
        let mut snapshot = self.snapshot.lock().unwrap();
        snapshot.state = state;
        snapshot.changes = changes;
    }

    /// The tasks part restarted as `run`, its task 3 holding `about`.
    fn restart(&self, run: u64, about: &str) {
        let mut snapshot = self.snapshot.lock().unwrap();
        snapshot.run = run;
        snapshot.about = about.into();
    }

    /// The decision's name for task 3's `change` in the run now.
    fn key(&self, change: u32) -> String {
        format!("{STORE}:3:{change}@{}", self.snapshot.lock().unwrap().run)
    }

    /// Wait until the tasks socket has been asked for progress `times`.
    fn asked_for_progress(&self, times: usize) {
        let until = Instant::now() + WAIT;
        while self.asked("get_progress") < times {
            assert!(Instant::now() < until, "progress was not asked for");
            thread::sleep(ms(10));
        }
    }

    /// A progress decision, as the policy publishes one.
    fn decide(&mut self, choice: &str, shadow: bool, task: &str) {
        let decision = Decision {
            about: "progress".into(),
            choice: choice.into(),
            probability: 0.8,
            answers: BTreeMap::new(),
            by: "stand-in".into(),
            took_ms: 1,
            shadow,
            request: None,
            task: Some(task.into()),
            arrival: None,
            ask: None,
        };
        self.decisions.publish_json(0, &decision).unwrap();
    }

    fn asked(&self, op: &str) -> usize {
        self.asked
            .lock()
            .unwrap()
            .iter()
            .filter(|o| *o == op)
            .count()
    }
}

impl Drop for Place {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// The tasks part's request socket: no settled news, and task 3 at work
/// with its progress line.
fn serve_tasks(socket: &Path, asked: Arc<Mutex<Vec<String>>>, snapshot: Arc<Mutex<Snapshot>>) {
    let listener = UnixListener::bind(socket).unwrap();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                return;
            };
            let mut line = String::new();
            if BufReader::new(&stream).read_line(&mut line).is_err() {
                continue;
            }
            let request: Value = serde_json::from_str(&line).unwrap();
            let op = request["op"].as_str().unwrap_or_default().to_owned();
            let (state, changes, run, about, delay) = {
                let snapshot = snapshot.lock().unwrap();
                (
                    snapshot.state,
                    snapshot.changes,
                    snapshot.run,
                    snapshot.about.clone(),
                    snapshot.delay,
                )
            };
            thread::sleep(delay);
            asked.lock().unwrap().push(op.clone());
            let base = json!({"ok":true,"store_id":STORE,"revision":7,"completion":"unverified"});
            let mut answer = base.clone();
            match (op.as_str(), request["id"].as_u64()) {
                ("list", _) => {
                    answer["items"] = json!([]);
                    answer["next_after_id"] = Value::Null;
                }
                ("get_progress", Some(3)) => {
                    answer["id"] = json!(3);
                    answer["progress"] = json!({"engine":"claude","state":state,"about":about,
                        "run":run,"changes":changes,"signs_seen":["tests_pass","fixed"],"last_active_unix_ms":null,"read_unix_ms":0});
                }
                ("get_task", Some(3)) => {
                    answer["text"] = json!("Fix the quoted string bug in the parser");
                    answer["target"] = json!({"engine":"claude"});
                }
                _ => answer = json!({"ok":false,"error":"not_found"}),
            }
            let _ = writeln!(stream, "{answer}");
        }
    });
}

/// The news offered within `time`, if any.
fn offered(news: &mut TaskNews, time: Duration) -> Option<News> {
    let until = Instant::now() + time;
    while Instant::now() < until {
        if let Some(waiting) = news.waiting() {
            return Some(waiting.clone());
        }
        thread::sleep(ms(10));
    }
    None
}

/// The policy's judge: yes to everything.
struct Yes;
impl Decider for Yes {
    fn ask(
        &mut self,
        _: &str,
        questions: &[(String, Question)],
        _: Duration,
    ) -> Result<Asked, Failed> {
        Ok(Asked {
            answers: questions
                .iter()
                .map(|(name, _)| {
                    let yes = if name == "too_much" { 0.0 } else { 1.0 };
                    (name.clone(), Answer::Noul(yes))
                })
                .collect(),
            model: "stand-in".into(),
            took: Duration::ZERO,
            usage: None,
            connection: None,
        })
    }
    fn name(&self) -> &str {
        "stand-in"
    }
}

#[test]
fn a_change_then_a_say_now_from_the_acting_policy_then_one_note_to_tell() {
    let mut place = Place::new("chain");
    let mut news = place.news(Some(&place.bus));
    // The policy, acting for progress, with a judge that says yes.
    let stop = Arc::new(AtomicBool::new(false));
    let log_dir = place.root.join("log");
    let running = {
        let (bus, stop) = (place.bus.clone(), stop.clone());
        let options = Options {
            log_dir: log_dir.clone(),
            deadline: ms(900),
            pause: ms(300),
            act_attention: false,
            report_every: policy::REPORT_EVERY,
        };
        let progress = progress::Options {
            act: true,
            ..progress::Options::new(ms(900), None)
        };
        thread::spawn(move || {
            policy::run_with_progress(&bus, Yes, &options, &stop, &mut |_| {}, Yes, progress)
        })
    };
    let started = Instant::now();
    while !place
        .bus
        .subscribers(TaskStatus::TOPIC)
        .unwrap()
        .iter()
        .any(|listener| listener.socket.starts_with("policy.") && listener.listening)
    {
        assert!(started.elapsed() < WAIT, "the policy did not subscribe");
        thread::sleep(ms(5));
    }
    let mut voice = place
        .bus
        .publisher_declared(VoiceState::TOPIC, &VoiceState::declaration(), "voice")
        .unwrap();
    voice
        .publish_json(
            0,
            &VoiceState {
                state: Voice::Resting,
            },
        )
        .unwrap();
    // What the work said before is where judging begins; then a change.
    place.work(0);
    thread::sleep(ms(200));
    place.work(1);
    let told = offered(&mut news, WAIT).expect("a note to tell");
    assert_eq!(told.key, format!("{STORE}:3:progress-{RUN_A}-1"));
    let note = &told.note;
    for part in [
        "Claude Code is still working on the task they gave it: \"Fix the quoted string bug in the parser\"",
        &format!("Its latest progress line: \"{ABOUT}\""),
        "Signs of a step forward in it: the tests pass, something was fixed.",
        "about 10 minutes",
        "No question at the end.)",
    ] {
        assert!(note.contains(part), "{part:?} not in {note}");
    }
    lamp_gemini::wire::encode_written_turn(note, None).unwrap();
    assert!(told.until.is_some());
    // Told once: taken, kept as told, and nothing more offered.
    assert_eq!(news.tell().map(|told| told.key), Some(told.key.clone()));
    assert!(offered(&mut news, ms(500)).is_none());
    let until = Instant::now() + WAIT;
    while !fs::read_to_string(&place.told).is_ok_and(|kept| kept.contains(&told.key)) {
        assert!(Instant::now() < until, "the told key was not kept");
        thread::sleep(ms(20));
    }
    assert_eq!(place.asked("get_progress"), 1);
    stop.store(true, Ordering::Relaxed);
    running.join().unwrap().unwrap();
}

#[test]
fn stale_shadow_other_store_and_older_change_decisions_are_let_go() {
    let mut place = Place::new("dropped");
    let mut news = place.news(Some(&place.bus));
    place.work(2);
    thread::sleep(ms(100));
    // In shadow, a wait, another store, an older change, no task: let go.
    place.decide("say_now", true, &place.key(2));
    place.decide("wait", false, &place.key(2));
    place.decide("say_now", false, &format!("{}:3:2@{RUN_A}", "f".repeat(32)));
    place.decide("say_now", false, &place.key(1));
    place.decide("say_now", false, "");
    assert!(offered(&mut news, ms(600)).is_none());
    assert_eq!(place.asked("get_progress"), 0);
    // The newest change, acting: taken (the reader was listening all along).
    place.words("working", 2);
    place.decide("say_now", false, &place.key(2));
    let told = offered(&mut news, WAIT).expect("the newest change");
    assert_eq!(told.key, format!("{STORE}:3:progress-{RUN_A}-2"));
    assert_eq!(place.asked("get_progress"), 1);
}

/// #765 review: the task finished between the status the decision was
/// judged on and the words fetched for it.
#[test]
fn a_task_that_finished_before_its_words_came_is_not_told_as_working() {
    let mut place = Place::new("finished");
    let mut news = place.news(Some(&place.bus));
    place.work(1);
    thread::sleep(ms(100));
    place.words("idle", 1);
    place.decide("say_now", false, &place.key(1));
    place.asked_for_progress(1);
    assert!(offered(&mut news, ms(300)).is_none());
}

/// #765 review: words of an older change are not told on the strength of
/// a decision about a newer one.
#[test]
fn words_of_an_older_change_are_not_told_under_a_newer_decision() {
    let mut place = Place::new("older");
    let mut news = place.news(Some(&place.bus));
    place.work(2);
    thread::sleep(ms(100));
    place.words("working", 1);
    place.decide("say_now", false, &place.key(2));
    place.asked_for_progress(1);
    assert!(offered(&mut news, ms(300)).is_none());
}

/// #765 review: a note queued while the lamp waits for its moment is
/// withdrawn once the task leaves the watch, finishes, or moves on.
#[test]
fn queued_progress_is_withdrawn_when_its_task_goes_finishes_or_moves_on() {
    let mut place = Place::new("withdrawn");
    let mut news = place.news(Some(&place.bus));
    fn queued(place: &mut Place, news: &mut TaskNews, change: u32) {
        place.work(change);
        thread::sleep(ms(100));
        place.words("working", change);
        place.decide("say_now", false, &place.key(change));
        assert!(offered(news, WAIT).is_some(), "change {change}");
    }
    // Gone from the watch.
    queued(&mut place, &mut news, 1);
    place.leave();
    thread::sleep(ms(600));
    assert!(news.waiting().is_none());
    // Finished.
    queued(&mut place, &mut news, 2);
    place.work_as(AgentState::Idle, 2);
    thread::sleep(ms(600));
    assert!(news.waiting().is_none());
    // Moved on to a newer change.
    queued(&mut place, &mut news, 3);
    place.work(4);
    thread::sleep(ms(600));
    assert!(news.waiting().is_none());
}

/// #765 review: the task finished while the coordinator waited for quiet,
/// someone there and a provider; the last look before telling lets it go.
#[test]
fn work_that_finished_while_the_lamp_waited_for_its_moment_is_not_told() {
    let mut place = Place::new("waited");
    let mut news = place.news(Some(&place.bus));
    place.work(1);
    thread::sleep(ms(100));
    place.decide("say_now", false, &place.key(1));
    assert!(offered(&mut news, WAIT).is_some());
    place.work_as(AgentState::Idle, 1);
    thread::sleep(ms(600));
    // Straight to telling, as the coordinator does once its gates open.
    assert!(news.tell().is_none());
    assert!(!place.told.exists());
}

/// #765 review: the minute a progress note may be told in counts from its
/// decision, the time spent fetching its words included.
#[test]
fn the_minute_counts_from_the_decision_not_from_the_fetching() {
    let mut place = Place::new("minute");
    let mut news = place.news(Some(&place.bus));
    place.work(1);
    thread::sleep(ms(100));
    place.snapshot.lock().unwrap().delay = ms(400);
    let decided = Instant::now();
    place.decide("say_now", false, &place.key(1));
    let told = offered(&mut news, WAIT).expect("a note");
    assert!(decided.elapsed() >= ms(800), "two slow reads");
    let until = told.until.unwrap();
    assert!(
        until <= decided + Duration::from_millis(60_000 + 150),
        "{:?} after the decision",
        until - decided
    );
}

/// The reviewer's finding on #765 (2026-10-15, 16:02): the tasks part
/// counts changes afresh in each run and keeps its store and task numbers.
/// Run A's change 1 was told; after a restart run B's change 1 is another
/// line, and is fetched and offered, not taken for the one told.
#[test]
fn after_a_restart_another_line_at_the_same_count_is_fetched_and_offered() {
    let mut place = Place::new("restart");
    let mut news = place.news(Some(&place.bus));
    place.work(0);
    place.work(1);
    thread::sleep(ms(100));
    place.decide("say_now", false, &place.key(1));
    let first = offered(&mut news, WAIT).expect("run A's change 1");
    assert_eq!(first.key, format!("{STORE}:3:progress-{RUN_A}-1"));
    assert_eq!(news.tell().map(|told| told.key), Some(first.key.clone()));
    let until = Instant::now() + WAIT;
    while !fs::read_to_string(&place.told).is_ok_and(|kept| kept.contains(&first.key)) {
        assert!(Instant::now() < until, "the told key was not kept");
        thread::sleep(ms(20));
    }
    // The tasks part restarts: same store, same task, counting again.
    place.restart(RUN_B, "Rewrote the parser's escape handling from scratch");
    place.work(0);
    place.work(1);
    thread::sleep(ms(100));
    place.decide("say_now", false, &place.key(1));
    let second = offered(&mut news, WAIT).expect("run B's change 1");
    assert_eq!(second.key, format!("{STORE}:3:progress-{RUN_B}-1"));
    assert!(
        second
            .note
            .contains("Rewrote the parser's escape handling from scratch"),
        "{}",
        second.note
    );
    assert_eq!(place.asked("get_progress"), 2);
    // A decision about run A's count, now that run B is there: let go.
    place.decide("say_now", false, &format!("{STORE}:3:2@{RUN_A}"));
    thread::sleep(ms(400));
    assert_eq!(place.asked("get_progress"), 2);
}

#[test]
fn without_the_switch_no_progress_is_listened_for_asked_or_kept() {
    // Only `on` turns it on.
    for value in [
        None,
        Some(""),
        Some("off"),
        Some("1"),
        Some("true"),
        Some("ON"),
    ] {
        assert!(!progress_on(value), "{value:?}");
    }
    assert!(progress_on(Some("on")));
    // As the voice starts without the switch: the lamp's bus, progress off;
    // with a clock of the test's own, so that it reads here too (on the
    // lamp it is `/proc`'s).
    let mut place = Place::new("off");
    let mut news = TaskNews::start_with_clock(
        &place.root.join("tasks"),
        place.told.clone(),
        Some(&place.bus),
        false,
        || {
            Some(BootClock {
                boot_id: "boot-a".into(),
                ms: 50_000_000,
            })
        },
    )
    .unwrap();
    place.work(1);
    place.decide("say_now", false, &place.key(1));
    assert!(offered(&mut news, ms(600)).is_none());
    // No socket of the voice's on the decisions; on `tasks/status` only
    // the one that listens for a task newly settled (#560).
    let voice = |topic: &str| {
        place
            .bus
            .subscribers(topic)
            .unwrap_or_default()
            .iter()
            .filter(|listener| listener.socket.starts_with("voice."))
            .count()
    };
    assert_eq!(voice(Decision::TOPIC), 0);
    assert_eq!(voice(TaskStatus::TOPIC), 1);
    // Nothing but the settled news' own reads on the tasks socket, and no
    // file of what was told.
    let asked = place.asked.lock().unwrap().clone();
    assert!(asked.iter().all(|op| op == "list"), "{asked:?}");
    assert!(!place.told.exists());
    // A task settled has it read at once, not at the next ten-second read
    // (#560).
    let lists = asked.len();
    assert!(lists > 0, "the first read, as the reader starts");
    place.settle();
    let deadline = Instant::now() + ms(2_000);
    while place.asked.lock().unwrap().len() == lists && Instant::now() < deadline {
        thread::sleep(ms(20));
    }
    let asked = place.asked.lock().unwrap().clone();
    assert!(asked.len() > lists, "{asked:?}");
    assert!(asked.iter().all(|op| op == "list"), "{asked:?}");
}
