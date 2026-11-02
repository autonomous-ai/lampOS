//! The memory probe's tests (#4748). Those that install the clock or give
//! the observation pass and the summary their models run in a process of
//! their own, as this module's one view to a process asks: the test runs
//! itself again with the environment that says so.
use super::*;
use crate::remember::{FORGET_EVERYTHING, RECALL, REMEMBER};
use lamp_memory::{
    Server,
    days::Days,
    days_protocol,
    protocol::Answer,
    server::{Memory, Stopper},
    store::Options,
};
use std::{
    os::unix::net::{UnixListener, UnixStream},
    process::Command,
    sync::{
        Mutex,
        mpsc::{self, Sender},
    },
    thread::JoinHandle,
};

const ANA: &str = "p-0123456789abcdef";
const BUDGET: Duration = Duration::from_millis(1_500);
/// Set in the process a test runs itself again in.
const CHILD: &str = "LAMP_MEMORY_PROBE_CHILD";

/// 31 October 2026, 23:50 in New York (EDT, UTC-4).
const OCT31_2350: u64 = 1_793_505_000_000;
/// 1 November 2026, 00:10 EDT: past local midnight.
const NOV1_0010: u64 = 1_793_506_200_000;
/// 1 November 2026, 00:20 EDT.
const NOV1_0020: u64 = 1_793_506_800_000;
/// 1 November 2026, 23:30 EST (UTC-5): after the clocks went back at
/// 02:00 EDT, still 1 November there, though 2 November at UTC-4.
const NOV1_2330_EST: u64 = 1_793_593_800_000;
/// 2 November 2026, 12:00 EST.
const NOV2_1200_EST: u64 = 1_793_638_800_000;

fn call(name: &str, args: Value) -> ToolCall {
    ToolCall {
        id: String::new(),
        name: name.into(),
        args,
    }
}

fn scratch(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("lamp-probe-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    root
}

#[test]
fn the_clock_only_goes_forward_and_moves_its_instant_as_much() {
    let clock = Clock::new(1_000, None);
    let at = clock.instant();
    assert_eq!(clock.set(61_000), Ok(()));
    assert_eq!(clock.now_ms(), 61_000);
    assert_eq!(clock.instant() - at, Duration::from_secs(60));
    assert_eq!(clock.set(60_000), Err(Backwards));
    assert_eq!(clock.now_ms(), 61_000, "never back");
}

#[test]
fn only_a_barrier_without_a_job_is_a_barrier() {
    assert!(observe_barrier_reached(&json!({ OBSERVE_BARRIER: 7 })));
    assert!(OBSERVE_DONE.load(Ordering::Acquire) >= 7);
    // A memory's frozen job always has "job": never taken for a barrier.
    assert!(!observe_barrier_reached(
        &json!({ OBSERVE_BARRIER: 9, "job": "j-1" })
    ));
    assert!(!observe_barrier_reached(&json!({"job": "j-1"})));
}

/// A memory that takes every connection and never answers: the writer
/// stays with its exchange.
fn silent(label: &str) -> (PathBuf, PathBuf) {
    let root = scratch(label);
    let socket = root.join("m.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    std::thread::spawn(move || {
        let held: Mutex<Vec<UnixStream>> = Mutex::new(Vec::new());
        for stream in listener.incoming().flatten() {
            held.lock().unwrap().push(stream);
        }
    });
    (root, socket)
}

/// Reviewer-three's third bound: a drain that times out, or is cancelled,
/// gives no `Drained`, the only way to a checkpoint.
#[test]
fn a_drain_that_times_out_or_is_cancelled_gives_nothing_to_checkpoint_with() {
    let (root, socket) = silent("late");
    let client = DaysClient::new(socket).unwrap();
    assert!(client.finished_exchange(1, "synthetic: a line", "A reply"));
    let started = Instant::now();
    let late = drain(&client, Duration::from_millis(150), &Cancel::default());
    assert_eq!(late.unwrap_err(), NotDrained::TimedOut(Stage::Writer));
    assert!(started.elapsed() < Duration::from_secs(1), "bounded");
    let cancel = Cancel::default();
    cancel.cancel();
    let cancelled = drain(&client, Duration::from_secs(5), &cancel);
    assert_eq!(cancelled.unwrap_err(), NotDrained::Cancelled(Stage::Writer));
    let _ = fs::remove_dir_all(root);
}

/// Runs `test` again in a process of its own, with the summary's switch
/// on and [`CHILD`] set; whether this is that process.
fn in_its_own_process(test: &str) -> bool {
    if std::env::var(CHILD).as_deref() == Ok("1") {
        return true;
    }
    let ran = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .env(crate::conversation_summary::ENVIRONMENT, "on")
        .output()
        .unwrap();
    let (stdout, stderr) = (
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr),
    );
    assert!(ran.status.success(), "{}\n{stdout}{stderr}", ran.status);
    assert!(
        stdout.contains("test result: ok. 1 passed;"),
        "{test} did not run\n{stdout}{stderr}"
    );
    false
}

struct Traced {
    store: Days,
    trace: Sender<(Value, Value)>,
}

impl Memory for Traced {
    fn answer(&mut self, line: &[u8], now: u64) -> Answer {
        let answer = days_protocol::answer(&mut self.store, line, now);
        let _ = self.trace.send((
            serde_json::from_slice(line).unwrap_or_default(),
            serde_json::from_str(&answer.line).unwrap_or_default(),
        ));
        answer
    }
}

/// The real store, on the probe's clock in New York, stopped and started
/// as a checkpoint and a restore need.
struct Store {
    dir: PathBuf,
    socket: PathBuf,
    clock: &'static Clock,
    trace: mpsc::Receiver<(Value, Value)>,
    tracing: Sender<(Value, Value)>,
    running: Option<(Stopper, JoinHandle<()>)>,
}

impl Store {
    fn new(root: &Path, clock: &'static Clock) -> Self {
        let (tracing, trace) = mpsc::channel();
        let mut store = Self {
            dir: root.join("store"),
            socket: root.join("m.sock"),
            clock,
            trace,
            tracing,
            running: None,
        };
        store.start();
        store
    }

    fn start(&mut self) {
        let clock = self.clock;
        let days = Days::open(
            Options {
                dir: self.dir.clone(),
                keep_days: 30,
                local: LocalTime::zone("America/New_York").unwrap(),
            },
            clock.now_ms(),
        )
        .unwrap();
        let server = Server::bind(&self.socket).unwrap();
        let stop = server.stopper();
        let trace = self.tracing.clone();
        let worker = std::thread::spawn(move || {
            let mut memory = Traced { store: days, trace };
            server
                .run(&mut memory, &mut || clock.now_ms(), &mut |_| {})
                .unwrap();
        });
        self.running = Some((stop, worker));
    }

    fn stop(&mut self) {
        let (stop, worker) = self.running.take().unwrap();
        stop.stop();
        worker.join().unwrap();
    }

    /// The ops the store has answered so far, with their answers; never
    /// waits.
    fn answered(&self) -> Vec<(Value, Value)> {
        self.trace.try_iter().collect()
    }
}

/// What a recall at `at` of `days_ago` finds, as text.
fn recalled(client: &DaysClient, clock: &Clock, at: u64, days_ago: u64) -> String {
    clock.set(at).unwrap();
    let (told, ok) = client.run_probe_at(
        &call(RECALL, json!({"days_ago": days_ago})),
        BUDGET,
        Some(ANA),
        None,
        at,
        clock.zone(),
    );
    assert!(ok, "{told}");
    told.to_string()
}

/// `text` said to the lamp at `at`, its conversation ended and journalled:
/// the writer and its store reply drained.
fn said(client: &DaysClient, clock: &Clock, at: u64, text: &str) {
    clock.set(at).unwrap();
    assert!(client.finished_exchange_for(at, text, "Mm.", Some(ANA)));
    assert!(client.end_conversation());
    drain(client, Duration::from_secs(10), &Cancel::default()).unwrap();
}

/// The scripted summary model, as `voice_talk_notes`'s: the previous note
/// and each line the person said.
fn folding(input: &str) -> Option<String> {
    let job: Value = serde_json::from_str(input).ok()?;
    let mut note = job["previous"].as_str().unwrap_or_default().to_owned();
    if note.is_empty() {
        note = "Current activity: a rivers quiz\nKey facts:".into();
    }
    for line in job["exchanges"].as_str()?.lines() {
        if let Some(said) = line.strip_prefix("Person: ") {
            note.push_str(&format!("\n- {said}"));
        }
    }
    Some(note)
}

/// How long the scripted observation model takes: longer than the short
/// drain below, well within the long one.
const MODEL_TAKES: Duration = Duration::from_millis(700);

const DONE_WHEN: &str = "remember::probe::tests::a_month_crosses_midnight_and_daylight_saving_drains_checkpoints_and_restores";

/// #4748's "done when", through Voice's own Days client and the real
/// store: the clock crosses local midnight and New York's daylight-saving
/// change, a drain waits for a queued observation and a talk note, a
/// checkpoint is complete or nothing, and a restore is refused after a
/// forget. All content is synthetic.
#[test]
fn a_month_crosses_midnight_and_daylight_saving_drains_checkpoints_and_restores() {
    if !in_its_own_process(DONE_WHEN) {
        return;
    }
    let root = scratch("month");
    let clock = Clock::install(
        OCT31_2350,
        Some(LocalTime::zone("America/New_York").unwrap()),
    )
    .unwrap();
    assert!(Clock::install(0, None).is_none(), "one clock to a process");
    // As in the product: a forget of everything reaches everything kept.
    assert!(crate::remember::switch_forget_all(true));
    let mut store = Store::new(&root, clock);
    let client = DaysClient::new(store.socket.clone()).unwrap();

    // Local midnight: before it is 31 October, after it 1 November.
    said(
        &client,
        clock,
        OCT31_2350,
        "synthetic: A, said before midnight",
    );
    said(
        &client,
        clock,
        NOV1_0010,
        "synthetic: B, said after midnight",
    );
    let yesterday = recalled(&client, clock, NOV1_0020, 1);
    assert!(yesterday.contains("synthetic: A"), "{yesterday}");
    assert!(!yesterday.contains("synthetic: B"), "{yesterday}");
    let today = recalled(&client, clock, NOV1_0020, 0);
    assert!(today.contains("synthetic: B"), "{today}");
    assert!(!today.contains("synthetic: A"), "{today}");

    // Daylight saving: 23:30 EST is still 1 November in New York, though
    // at the summer's UTC-4 it would be 2 November.
    let zone = clock.zone().unwrap();
    assert_eq!(zone.day(NOV1_2330_EST), zone.day(NOV1_0010));
    assert_ne!(
        LocalTime::Offset(-240).day(NOV1_2330_EST),
        zone.day(NOV1_0010)
    );
    said(
        &client,
        clock,
        NOV1_2330_EST,
        "synthetic: C, said late on the long day",
    );
    let yesterday = recalled(&client, clock, NOV2_1200_EST, 1);
    assert!(yesterday.contains("synthetic: B"), "{yesterday}");
    assert!(yesterday.contains("synthetic: C"), "{yesterday}");
    assert!(!yesterday.contains("synthetic: A"), "{yesterday}");
    // The production dispatch reads the installed clock too.
    let (told, ok) = client.run_for(&call(RECALL, json!({"days_ago": 1})), BUDGET, Some(ANA));
    assert!(ok && told.to_string().contains("synthetic: C"), "{told}");
    // So does a kept thing's own time.
    let (told, ok) = client.run_probe_at(
        &call(REMEMBER, json!({"text": "synthetic: E, kept"})),
        BUDGET,
        Some(ANA),
        None,
        NOV2_1200_EST,
        clock.zone(),
    );
    assert!(ok && told["remembered"] == true, "{told}");

    // A conversation whose end queues an observation pass and a talk note.
    crate::remember::observe_with(Box::new(|input: &Value| {
        std::thread::sleep(MODEL_TAKES);
        Some(json!({"observations":[{
            "text":"The quiz is about rivers",
            "category":"thread",
            "source_ids":[input["source_ids"][0]],
            "supersedes":null
        }]}))
    }));
    crate::conversation_summary::summarize_with(Box::new(folding));
    crate::conversation_summary::fold(
        Some(ANA.into()),
        vec![(
            1,
            "synthetic: I am planning a trip to Hue".into(),
            "Mm.".into(),
        )],
    );
    let until = Instant::now() + Duration::from_secs(10);
    while crate::conversation_summary::for_scope(Some(ANA), clock.instant()).is_none() {
        assert!(Instant::now() < until, "the note was never made");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(client.finished_exchange_for(
        clock.now_ms(),
        "synthetic: the quiz is about rivers",
        "Then the first river",
        Some(ANA)
    ));
    assert!(client.end_conversation());
    let _ = store.answered();
    // Shorter than the model: no drain, and nothing to checkpoint with.
    let late = drain(&client, Duration::from_millis(100), &Cancel::default());
    // (The writer's own store reply may take that long on a busy disk.)
    assert!(
        matches!(
            late,
            Err(NotDrained::TimedOut(Stage::Writer | Stage::Observation))
        ),
        "{late:?}"
    );
    // In time: the observation's store reply and the talk note's are in
    // before the drain returns, read without waiting.
    let drained = drain(&client, Duration::from_secs(10), &Cancel::default()).unwrap();
    let answered = store.answered();
    let applied = answered
        .iter()
        .find(|(asked, _)| asked["op"] == "tidy_apply")
        .unwrap_or_else(|| panic!("no observation: {answered:?}"));
    assert_eq!(
        applied.0["observations"][0]["text"],
        "The quiz is about rivers"
    );
    assert_eq!(applied.1["ok"], true, "{}", applied.1);
    let talk = answered
        .iter()
        .find(|(asked, _)| asked["op"] == "talk_note")
        .unwrap_or_else(|| panic!("no talk note: {answered:?}"));
    assert_eq!(talk.1["ok"], true, "{}", talk.1);
    assert!(talk.0["text"].as_str().unwrap().contains("trip to Hue"));

    // A cancelled checkpoint leaves nothing.
    store.stop();
    let cancel = Cancel::default();
    cancel.cancel();
    let into = root.join("checkpoint-1");
    assert!(matches!(
        checkpoint(&client, drained, &store.dir, &into, &cancel),
        Err(NotTaken::Cancelled)
    ));
    assert!(!into.exists() && !into.with_extension("partial").exists());
    // A complete one.
    let drained = drain(&client, Duration::from_secs(5), &Cancel::default()).unwrap();
    let first = checkpoint(&client, drained, &store.dir, &into, &Cancel::default()).unwrap();
    assert!(first.dir().join(lamp_memory::days::DATABASE).is_file());

    // Something kept after it goes with a restore.
    store.start();
    said(
        &client,
        clock,
        NOV2_1200_EST + 60_000,
        "synthetic: D, after the checkpoint",
    );
    assert!(recalled(&client, clock, NOV2_1200_EST + 120_000, 0).contains("synthetic: D"));
    store.stop();
    restore(&client, &first, &store.dir, &Cancel::default()).unwrap();
    store.start();
    let today = recalled(&client, clock, NOV2_1200_EST + 180_000, 0);
    assert!(!today.contains("synthetic: D"), "{today}");
    let yesterday = recalled(&client, clock, NOV2_1200_EST + 180_000, 1);
    assert!(yesterday.contains("synthetic: C"), "{yesterday}");

    // A disposable child's store is the checkpoint's; the view is not
    // written.
    let child = root.join("child");
    copy_store(&first, &child).unwrap();
    assert!(child.join(lamp_memory::days::DATABASE).is_file());

    // After a forget, a restore is refused: what it removed stays removed.
    assert!(client.end_conversation());
    let drained = drain(&client, Duration::from_secs(5), &Cancel::default()).unwrap();
    store.stop();
    let second = checkpoint(
        &client,
        drained,
        &store.dir,
        &root.join("checkpoint-2"),
        &Cancel::default(),
    )
    .unwrap();
    store.start();
    clock.set(NOV2_1200_EST + 240_000).unwrap();
    let (told, ok) = client.run_probe_at(
        &call(FORGET_EVERYTHING, json!({})),
        BUDGET,
        Some(ANA),
        Some(7),
        NOV2_1200_EST + 240_000,
        clock.zone(),
    );
    assert!(ok && told["forgotten"] == true, "{told}");
    // The reply is kept out of the thread, as in production.
    assert!(crate::remember::forgetting_reply(7));
    store.stop();
    assert!(matches!(
        restore(&client, &second, &store.dir, &Cancel::default()),
        Err(Refused::ForgotSince)
    ));
    assert!(matches!(
        restore(&client, &first, &store.dir, &Cancel::default()),
        Err(Refused::ForgotSince)
    ));
    // Another client's checkpoint is never put on this one.
    let other = DaysClient::new(store.socket.clone()).unwrap();
    assert!(matches!(
        restore(&other, &second, &store.dir, &Cancel::default()),
        Err(Refused::OtherView)
    ));
    store.start();
    let yesterday = recalled(&client, clock, NOV2_1200_EST + 300_000, 1);
    assert!(!yesterday.contains("synthetic: C"), "{yesterday}");
    assert!(!yesterday.contains("synthetic: B"), "{yesterday}");
    store.stop();
    let _ = fs::remove_dir_all(root);
}
