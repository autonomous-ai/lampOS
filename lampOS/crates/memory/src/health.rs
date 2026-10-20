//! Optional, content-free health reports. The socket owner only updates fixed
//! atomic state. One worker owns all bus I/O and legacy count scans; it never
//! asks the socket owner to wait and is never replaced while still running.
use crate::{health_counts::Counts, protocol::Answer};
use lamp_msgs::{Diagnostic, Level};
use std::{
    io,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering::SeqCst},
        mpsc::{self, Receiver},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub const REPORT_EVERY: Duration = Duration::from_secs(1);
pub const SHUTDOWN_WAIT: Duration = Duration::from_millis(100);
const WINDOW: usize = 60;
const NOTE_US: u64 = 300_000;
const CALL_US: u64 = 1_500_000;
const OPENING: u8 = 0;
const SERVING: u8 = 1;
const FAILED: u8 = 2;
const STOPPED: u8 = 3;

#[derive(Clone)]
pub enum CountSource {
    Legacy(PathBuf),
    Days,
}

#[derive(Default)]
struct Bucket {
    second: AtomicU64,
    note: AtomicU64,
    call: AtomicU64,
    answers: AtomicU64,
}

/// Written by the single socket owner, read by the reporting worker. `version`
/// covers only short state updates; `generation` covers whole store operations.
/// A reader makes at most four attempts to copy state, never a retry spin.
pub struct State {
    origin: Instant,
    version: AtomicU64,
    generation: AtomicU64,
    phase: AtomicU8,
    storage_fault: AtomicBool,
    count_fault: AtomicBool,
    count_known: AtomicBool,
    exchanges: AtomicU64,
    kept: AtomicU64,
    counted_at: AtomicU64,
    active_at: AtomicU64,
    active_budget: AtomicU64,
    answered: AtomicU64,
    /// The database a newer build wrote, set aside at start (#1680).
    set_aside: Mutex<Option<String>>,
    buckets: [Bucket; WINDOW],
}

pub struct Ticket {
    started: u64,
    budget: u64,
}

#[derive(Default)]
struct Snapshot {
    phase: u8,
    storage_fault: bool,
    count_fault: bool,
    counts: Option<(Counts, u64)>,
    active_us: Option<u64>,
    active_budget: u64,
    answered: u64,
    recent_answers: u64,
    note_us: u64,
    call_us: u64,
    set_aside: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
            version: AtomicU64::new(0),
            generation: AtomicU64::new(0),
            phase: AtomicU8::new(OPENING),
            storage_fault: AtomicBool::new(false),
            count_fault: AtomicBool::new(false),
            count_known: AtomicBool::new(false),
            exchanges: AtomicU64::new(0),
            kept: AtomicU64::new(0),
            counted_at: AtomicU64::new(0),
            active_at: AtomicU64::new(0),
            active_budget: AtomicU64::new(0),
            answered: AtomicU64::new(0),
            set_aside: Mutex::new(None),
            buckets: std::array::from_fn(|_| Bucket::default()),
        }
    }
}

impl State {
    fn now(&self) -> u64 {
        self.origin.elapsed().as_micros().min(u64::MAX as u128 - 1) as u64 + 1
    }

    pub fn serving(&self) {
        self.phase.store(SERVING, SeqCst);
    }

    /// Said on every report while it is there: the memory a newer build
    /// wrote is kept under this file name and not read by this build
    /// (#1680). None once forgetting everything removed it (#2067).
    pub fn set_aside(&self, name: Option<&str>) {
        let mut held = self.set_aside.lock().unwrap_or_else(|e| e.into_inner());
        if held.as_deref() != name {
            *held = name.map(str::to_owned);
        }
    }

    pub fn counts(&self, counts: io::Result<Counts>) {
        self.version.fetch_add(1, SeqCst);
        self.set_counts(counts, self.now());
        self.version.fetch_add(1, SeqCst);
    }

    fn set_counts(&self, counts: io::Result<Counts>, now: u64) {
        match counts {
            Ok(counts) => {
                self.exchanges.store(counts.exchanges, SeqCst);
                self.kept.store(counts.kept, SeqCst);
                self.counted_at.store(now, SeqCst);
                self.count_known.store(true, SeqCst);
                self.count_fault.store(false, SeqCst);
            }
            Err(_) => {
                self.count_known.store(false, SeqCst);
                self.count_fault.store(true, SeqCst);
            }
        }
    }

    /// Includes request classification in handler time. No text or person ID
    /// is retained: unknown and malformed operations use the general budget.
    pub fn begin(&self, line: &[u8]) -> Ticket {
        let started = self.now();
        // Match both protocol parsers' last-key-wins Value semantics. A
        // derived struct rejects duplicate op keys that the store accepts,
        // which could otherwise give a session note the looser call budget.
        let note = line.len() <= crate::protocol::MOST_LINE_BYTES
            && serde_json::from_slice::<serde_json::Value>(line)
                .ok()
                .and_then(|value| {
                    value
                        .get("op")
                        .and_then(|op| op.as_str())
                        .map(|op| matches!(op, "kept" | "session_open"))
                })
                .unwrap_or(false);
        let budget = if note { NOTE_US } else { CALL_US };
        self.begin_at(started, budget)
    }

    fn begin_at(&self, started: u64, budget: u64) -> Ticket {
        self.version.fetch_add(1, SeqCst);
        self.generation.fetch_add(1, SeqCst);
        self.active_at.store(started, SeqCst);
        self.active_budget.store(budget, SeqCst);
        self.version.fetch_add(1, SeqCst);
        Ticket { started, budget }
    }

    /// Capture the end before the optional O(1) SQLite count read. Logging,
    /// response delivery, connection waiting and bus work are excluded.
    pub fn answered_at(&self) -> u64 {
        self.now()
    }

    pub fn finish(
        &self,
        ticket: Ticket,
        ended: u64,
        answer: &Answer,
        counts: Option<io::Result<Counts>>,
    ) {
        self.version.fetch_add(1, SeqCst);
        let second = ended / 1_000_000 + 1;
        let bucket = &self.buckets[second as usize % WINDOW];
        if bucket.second.load(SeqCst) != second {
            bucket.note.store(0, SeqCst);
            bucket.call.store(0, SeqCst);
            bucket.answers.store(0, SeqCst);
            bucket.second.store(second, SeqCst);
        }
        let peak = if ticket.budget == NOTE_US {
            &bucket.note
        } else {
            &bucket.call
        };
        peak.fetch_max(ended.saturating_sub(ticket.started).max(1), SeqCst);
        bucket.answers.fetch_add(1, SeqCst);
        self.answered.fetch_add(1, SeqCst);
        if answer.storage_fault {
            self.storage_fault.store(true, SeqCst);
        } else if answer.wrote {
            // A successful read or no-op deletion does not prove writes work.
            self.storage_fault.store(false, SeqCst);
        }
        if let Some(counts) = counts {
            self.set_counts(counts, ended);
        }
        self.active_at.store(0, SeqCst);
        self.generation.fetch_add(1, SeqCst);
        self.version.fetch_add(1, SeqCst);
    }

    fn snapshot(&self, now: u64) -> Option<Snapshot> {
        for _ in 0..4 {
            let before = self.version.load(SeqCst);
            if !before.is_multiple_of(2) {
                continue;
            }
            let active = self.active_at.load(SeqCst);
            let mut got = Snapshot {
                phase: self.phase.load(SeqCst),
                storage_fault: self.storage_fault.load(SeqCst),
                count_fault: self.count_fault.load(SeqCst),
                counts: self.count_known.load(SeqCst).then(|| {
                    (
                        Counts {
                            exchanges: self.exchanges.load(SeqCst),
                            kept: self.kept.load(SeqCst),
                        },
                        self.counted_at.load(SeqCst),
                    )
                }),
                active_us: (active != 0).then(|| now.saturating_sub(active)),
                active_budget: self.active_budget.load(SeqCst),
                answered: self.answered.load(SeqCst),
                set_aside: self
                    .set_aside
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone(),
                ..Snapshot::default()
            };
            let second = now / 1_000_000 + 1;
            for bucket in &self.buckets {
                let at = bucket.second.load(SeqCst);
                if at != 0 && at <= second && second - at < WINDOW as u64 {
                    got.note_us = got.note_us.max(bucket.note.load(SeqCst));
                    got.call_us = got.call_us.max(bucket.call.load(SeqCst));
                    got.recent_answers += bucket.answers.load(SeqCst);
                }
            }
            if before == self.version.load(SeqCst) {
                return Some(got);
            }
        }
        None
    }
}

fn milliseconds(us: u64) -> String {
    format!("{}.{:03}", us / 1000, us % 1000)
}

fn measured(us: u64) -> String {
    if us == 0 {
        "none".into()
    } else {
        milliseconds(us)
    }
}

fn diagnostic(snapshot: &Snapshot, now: u64) -> Diagnostic {
    let why = match snapshot.phase {
        OPENING => "opening store",
        FAILED => "store or server failed",
        STOPPED => "memory stopped",
        _ if snapshot.storage_fault => "storage failed; awaiting a successful write",
        _ if snapshot.count_fault => "retained counts could not be read",
        _ if snapshot
            .active_us
            .is_some_and(|us| us > snapshot.active_budget) =>
        {
            "answer in progress exceeds its budget"
        }
        _ if snapshot.note_us > NOTE_US => "recent session note exceeded 300 ms",
        _ if snapshot.call_us > CALL_US => "recent answer exceeded 1500 ms",
        _ if snapshot.set_aside.is_some() => {
            "memory set aside a database from a newer build; its records are not read"
        }
        _ => "",
    };
    let report = if why.is_empty() {
        Diagnostic::ok("memory")
    } else {
        Diagnostic::new(Level::Warn, "memory", why)
    };
    let report = match snapshot.counts {
        Some((counts, at)) => report
            .with("exchanges", counts.exchanges)
            .with("kept", counts.kept)
            .with("counts_age_ms", now.saturating_sub(at) / 1000),
        None => report
            .with("exchanges", "unknown")
            .with("kept", "unknown")
            .with("counts_age_ms", "unknown"),
    };
    let report = match &snapshot.set_aside {
        Some(name) => report.with("set_aside", name.as_str()),
        None => report,
    };
    report
        .with("answered", snapshot.answered)
        .with("recent_answers", snapshot.recent_answers)
        .with("recent_seconds", WINDOW)
        .with(
            "slowest_answer_ms",
            measured(snapshot.note_us.max(snapshot.call_us)),
        )
        .with("slowest_note_ms", measured(snapshot.note_us))
        .with("slowest_call_ms", measured(snapshot.call_us))
        .with(
            "active_answer_ms",
            snapshot
                .active_us
                .map_or_else(|| "none".into(), milliseconds),
        )
        .with("note_budget_ms", NOTE_US / 1000)
        .with("call_budget_ms", CALL_US / 1000)
        .with(
            "timing",
            "complete request to serialized answer; excludes socket waits and delivery",
        )
}

fn count_when_idle(
    state: &State,
    read: impl FnOnce() -> io::Result<Counts>,
) -> Option<io::Result<Counts>> {
    let generation = state.generation.load(SeqCst);
    if state.phase.load(SeqCst) != SERVING || !generation.is_multiple_of(2) {
        return None;
    }
    let result = read();
    (state.generation.load(SeqCst) == generation).then_some(result)
}

/// The publication call's result, not a subscriber or downstream log receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicationOutcome {
    NotAttempted,
    ReturnedOk,
    ReturnedErr,
}

/// Completed worker work and a checked positive wait in an explicit fixture.
/// Neither observation claims that the worker parked, exited, or delivered
/// an Event or a line to the process's final log destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportProgress {
    CycleComplete {
        started: Instant,
        publication: PublicationOutcome,
    },
    WaitChecked {
        started: Instant,
        at: Instant,
        remaining: Duration,
    },
}

/// A local fixture's scheduling source, independent of State's real latency
/// measurements. Reads must be pure and in one nondecreasing Instant domain.
pub trait ReportSchedule: Send + Sync + 'static {
    fn now(&self) -> Instant;

    /// Wait using the current thread's park token, returning when its Thread
    /// is unparked. This includes an unpark before this call and allows the
    /// unchanged bounded Reporter::finish to wake a frozen fixture.
    fn park(&self, remaining: Duration);

    /// Observe locally without blocking or waiting for a controller. A
    /// positive WaitChecked precedes parking; the retained unpark token
    /// handles a controller wake between this observation and park.
    fn observe(&self, _progress: ReportProgress) {}
}

struct SystemSchedule;

impl ReportSchedule for SystemSchedule {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn park(&self, remaining: Duration) {
        thread::park_timeout(remaining);
    }
}

/// One worker for this process's lifetime. A blocked filesystem operation is
/// not cancellable; shutdown waits at most 100 ms, then leaves that one worker
/// to exit when the operation returns. The binary's exit also ends the worker.
pub struct Reporter {
    state: Arc<State>,
    stop: Arc<AtomicBool>,
    done: Receiver<()>,
    thread: Option<JoinHandle<()>>,
}

impl Reporter {
    pub fn start(root: PathBuf, counts: CountSource) -> io::Result<Self> {
        Self::start_with_schedule(root, counts, Arc::new(SystemSchedule))
    }

    /// Use an explicit local schedule for this worker and its publisher's
    /// cadence. This does not change the State measurement clock or install
    /// a process-wide clock; the caller owns advancement and progress checks.
    pub fn start_with_schedule(
        root: PathBuf,
        counts: CountSource,
        schedule: Arc<dyn ReportSchedule>,
    ) -> io::Result<Self> {
        let publisher_schedule = Arc::clone(&schedule);
        let mut publisher = None;
        Self::start_scheduled_with(counts, schedule, move |report| {
            if publisher.is_none() {
                let cadence = Arc::clone(&publisher_schedule);
                publisher = Some(
                    lamp_bus::Bus::new(&root)?.publisher_declared_with_cadence_clock(
                        Diagnostic::TOPIC,
                        &Diagnostic::declaration(),
                        "memory",
                        move || cadence.now(),
                    )?,
                );
            }
            let result = publisher
                .as_mut()
                .expect("just initialized")
                .publish_json(lamp_ipc::monotonic_us(), report)
                .map(|_| ());
            if result.is_err() {
                // Publisher suppresses repeated filesystem errors. Recreate it
                // next cycle so a suppressed error cannot masquerade as recovery.
                publisher = None;
            }
            result
        })
    }

    #[cfg(test)]
    fn start_with(
        source: CountSource,
        publish: impl FnMut(&Diagnostic) -> io::Result<()> + Send + 'static,
    ) -> io::Result<Self> {
        Self::start_scheduled_with(source, Arc::new(SystemSchedule), publish)
    }

    fn start_scheduled_with(
        source: CountSource,
        schedule: Arc<dyn ReportSchedule>,
        mut publish: impl FnMut(&Diagnostic) -> io::Result<()> + Send + 'static,
    ) -> io::Result<Self> {
        let state = Arc::new(State::default());
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::clone(&state);
        let stopping = Arc::clone(&stop);
        let (done_send, done) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("memory-health".into())
            .spawn(move || {
                let mut counts = None;
                let mut count_fault = false;
                let mut delivery_failed = false;
                loop {
                    let started = schedule.now();
                    let mut publication = PublicationOutcome::NotAttempted;
                    if let CountSource::Legacy(dir) = &source
                        && let Some(read) =
                            count_when_idle(&shared, || crate::health_counts::legacy(dir))
                    {
                        match read {
                            Ok(found) => {
                                counts = Some((found, shared.now()));
                                count_fault = false;
                            }
                            Err(_) => {
                                counts = None;
                                count_fault = true;
                            }
                        }
                    }
                    let now = shared.now();
                    if let Some(mut snapshot) = shared.snapshot(now)
                        && snapshot.phase != OPENING
                    {
                        if matches!(source, CountSource::Legacy(_)) {
                            snapshot.counts = counts;
                            snapshot.count_fault = count_fault;
                        }
                        let failed = publish(&diagnostic(&snapshot, now)).is_err();
                        publication = if failed {
                            PublicationOutcome::ReturnedErr
                        } else {
                            PublicationOutcome::ReturnedOk
                        };
                        if failed != delivery_failed {
                            eprintln!(
                                "lamp-memory: health delivery {}",
                                if failed { "failed" } else { "recovered" }
                            );
                            delivery_failed = failed;
                        }
                    }
                    schedule.observe(ReportProgress::CycleComplete {
                        started,
                        publication,
                    });
                    if stopping.load(SeqCst) {
                        break;
                    }
                    // park_timeout may wake spuriously. Recheck the deadline
                    // so those wakes do not create extra reports or retries.
                    while !stopping.load(SeqCst) {
                        let at = schedule.now();
                        let left =
                            REPORT_EVERY.saturating_sub(at.saturating_duration_since(started));
                        if left.is_zero() {
                            break;
                        }
                        schedule.observe(ReportProgress::WaitChecked {
                            started,
                            at,
                            remaining: left,
                        });
                        schedule.park(left);
                    }
                }
                // Publisher cleanup can itself touch the filesystem. It must
                // finish before the bounded waiter is told work has ended.
                drop(publish);
                let _ = done_send.send(());
            })?;
        Ok(Self {
            state,
            stop,
            done,
            thread: Some(thread),
        })
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn finish(&mut self, failed: bool) {
        self.state
            .phase
            .store(if failed { FAILED } else { STOPPED }, SeqCst);
        self.stop.store(true, SeqCst);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = self.done.recv_timeout(SHUTDOWN_WAIT);
            // No unbounded join after a completion signal: the OS can still
            // hold thread teardown. This worker is never replaced.
        }
    }
}

impl Drop for Reporter {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.finish(true);
        }
    }
}

#[cfg(test)]
#[path = "reporter_schedule_tests.rs"]
mod schedule_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Options, Server, Store};
    use std::{
        fs,
        io::{BufRead, BufReader, Write},
        os::unix::net::UnixStream,
        sync::mpsc,
    };

    const NOW: u64 = 1_791_892_800_000;

    fn answer(storage_fault: bool, wrote: bool) -> Answer {
        Answer {
            line: "{}".into(),
            log: String::new(),
            storage_fault,
            wrote,
        }
    }

    fn report(state: &State, now: u64) -> Diagnostic {
        diagnostic(&state.snapshot(now).unwrap(), now)
    }

    fn value<'a>(report: &'a Diagnostic, key: &str) -> &'a str {
        &report.values.iter().find(|v| v.key == key).unwrap().value
    }

    #[test]
    fn exact_budgets_recent_window_and_in_flight_answers_have_distinct_meanings() {
        let state = State::default();
        state.serving();
        let empty = report(&state, 1);
        assert_eq!(empty.level, Level::Ok);
        assert_eq!(value(&empty, "slowest_answer_ms"), "none");
        assert_eq!(value(&empty, "exchanges"), "unknown");

        let note = state.begin_at(1, NOTE_US);
        assert_eq!(report(&state, NOTE_US + 1).level, Level::Ok);
        assert_eq!(
            report(&state, NOTE_US + 2).message,
            "answer in progress exceeds its budget"
        );
        state.finish(note, NOTE_US + 1, &answer(false, false), None);
        assert_eq!(report(&state, NOTE_US + 1).level, Level::Ok);
        let note = state.begin_at(400_000, NOTE_US);
        state.finish(note, 400_000 + NOTE_US + 1, &answer(false, false), None);
        let slow = report(&state, 800_000);
        assert_eq!(slow.level, Level::Warn);
        assert_eq!(value(&slow, "slowest_note_ms"), "300.001");
        assert_eq!(value(&slow, "slowest_call_ms"), "none");
        assert_eq!(value(&slow, "recent_answers"), "2");
        assert_eq!(report(&state, 59_999_999).level, Level::Warn);
        let aged = report(&state, 60_000_000);
        assert_eq!(aged.level, Level::Ok);
        assert_eq!(value(&aged, "recent_answers"), "0");
        assert_eq!(value(&aged, "answered"), "2");
        assert_eq!(value(&aged, "slowest_answer_ms"), "none");

        let call = state.begin_at(60_000_001, CALL_US);
        state.finish(call, 61_500_002, &answer(false, false), None);
        assert_eq!(
            report(&state, 62_000_000).message,
            "recent answer exceeded 1500 ms"
        );
    }

    #[test]
    fn counts_and_storage_faults_are_independent_and_reads_do_not_prove_recovery() {
        let state = State::default();
        state.serving();
        state.counts(Ok(Counts {
            exchanges: 7,
            kept: 2,
        }));
        let fail = state.begin_at(1, CALL_US);
        state.finish(fail, 2, &answer(true, false), None);
        assert_eq!(report(&state, 2).level, Level::Warn);
        let read = state.begin_at(3, NOTE_US);
        state.finish(read, 4, &answer(false, false), None);
        assert_eq!(
            report(&state, 4).message,
            "storage failed; awaiting a successful write"
        );
        let write = state.begin_at(5, CALL_US);
        state.finish(
            write,
            6,
            &answer(false, true),
            Some(Err(io::Error::other("synthetic private path"))),
        );
        let unknown = report(&state, 6);
        assert_eq!(unknown.message, "retained counts could not be read");
        assert_eq!(value(&unknown, "kept"), "unknown");
        assert!(
            !serde_json::to_string(&unknown)
                .unwrap()
                .contains("private path")
        );
        state.counts(Ok(Counts {
            exchanges: 8,
            kept: 2,
        }));
        let recovered = report(&state, 7);
        assert_eq!(recovered.level, Level::Ok);
        assert_eq!(value(&recovered, "exchanges"), "8");
    }

    #[test]
    fn count_scans_overlapping_even_a_finished_request_are_discarded() {
        let state = State::default();
        state.serving();
        assert!(
            count_when_idle(&state, || {
                let ticket = state.begin_at(1, CALL_US);
                state.finish(ticket, 2, &answer(false, false), None);
                Ok(Counts::default())
            })
            .is_none()
        );
        let ticket = state.begin_at(3, CALL_US);
        assert!(count_when_idle(&state, || panic!("must not scan during a request")).is_none());
        state.finish(ticket, 4, &answer(false, false), None);
        assert_eq!(
            count_when_idle(&state, || Ok(Counts::default()))
                .unwrap()
                .unwrap(),
            Counts::default()
        );
        // A busy state copy skips this report instead of spinning or blocking.
        state.version.store(1, SeqCst);
        assert!(state.snapshot(5).is_none());
    }

    #[test]
    fn session_note_classification_retains_no_request_content() {
        let state = State::default();
        state.serving();
        for (line, budget) in [
            (br#"{"op":"kept"}"#.as_slice(), NOTE_US),
            (
                br#"{"op":"session_open","session":"private-name"}"#,
                NOTE_US,
            ),
            (br#"{"op":"recall","op":"kept"}"#, NOTE_US),
            (br#"{"op":null,"op":"session_open"}"#, NOTE_US),
            (br#"{"op":"kept","op":"status"}"#, CALL_US),
            (br#"{"op":"recall","query":"private-words"}"#, CALL_US),
            (br#"{"op":"unknown"}"#, CALL_US),
            (b"not json", CALL_US),
        ] {
            let ticket = state.begin(line);
            assert_eq!(ticket.budget, budget);
            state.finish(ticket, state.answered_at(), &answer(false, false), None);
        }
        let text = serde_json::to_string(&report(&state, state.now())).unwrap();
        assert!(!text.contains("private"));
    }

    struct Scratch(PathBuf);
    impl Scratch {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("mh-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn store(&self) -> Store {
            Store::open(
                Options {
                    dir: self.0.join("store"),
                    keep_days: 30,
                    local: crate::store::LocalTime::Offset(0),
                },
                NOW,
            )
            .unwrap()
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn ask(socket: &std::path::Path) {
        let mut stream = UnixStream::connect(socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream.write_all(b"{\"op\":\"kept\"}\n").unwrap();
        let mut answer = String::new();
        BufReader::new(stream).read_line(&mut answer).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&answer).unwrap()["ok"],
            true
        );
    }

    #[test]
    fn held_publication_cannot_block_a_real_socket_or_bounded_shutdown() {
        let scratch = Scratch::new("held");
        let (entered, waiting) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::sync_channel(1);
        let mut reporter = Reporter::start_with(CountSource::Days, move |_| {
            let _ = entered.try_send(());
            gate.recv().unwrap();
            Ok(())
        })
        .unwrap();
        reporter.state.serving();
        waiting.recv_timeout(Duration::from_secs(5)).unwrap();
        let state = Arc::clone(&reporter.state);
        let socket = scratch.0.join("m");
        let server = Server::bind(&socket).unwrap();
        let stopper = server.stopper();
        let mut store = scratch.store();
        let serving = thread::spawn(move || {
            server.run_reporting(&mut store, &mut || NOW, &mut |_| {}, Some(&state))
        });
        // Causal test: the publisher is still held throughout every request.
        for _ in 0..20 {
            ask(&socket);
        }
        stopper.stop();
        serving.join().unwrap().unwrap();
        let started = Instant::now();
        reporter.finish(false);
        assert!(started.elapsed() < Duration::from_millis(500));
        release.send(()).unwrap();
        reporter.done.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            report(&reporter.state, reporter.state.now()).message,
            "memory stopped"
        );
    }

    #[test]
    fn extra_thread_wakes_do_not_burst_reports() {
        let (sent, reports) = mpsc::channel();
        let mut reporter = Reporter::start_with(CountSource::Days, move |report| {
            sent.send(report.clone()).unwrap();
            Ok(())
        })
        .unwrap();
        reporter.state.serving();
        reports.recv_timeout(Duration::from_secs(5)).unwrap();
        for _ in 0..5 {
            reporter.thread.as_ref().unwrap().thread().unpark();
        }
        assert!(reports.recv_timeout(Duration::from_millis(50)).is_err());
        assert_eq!(
            reports.recv_timeout(Duration::from_secs(5)).unwrap().level,
            Level::Ok
        );
        reporter.finish(false);
    }

    #[test]
    fn held_publisher_cleanup_also_has_a_bounded_shutdown_wait() {
        struct HeldDrop {
            entered: mpsc::SyncSender<()>,
            release: mpsc::Receiver<()>,
        }
        impl Drop for HeldDrop {
            fn drop(&mut self) {
                self.entered.send(()).unwrap();
                self.release.recv().unwrap();
            }
        }
        let (entered, waiting) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::sync_channel(1);
        let held = HeldDrop {
            entered,
            release: gate,
        };
        let reporter = Reporter::start_with(CountSource::Days, move |_| {
            std::hint::black_box(&held);
            Ok(())
        })
        .unwrap();
        let (finished, result) = mpsc::sync_channel(1);
        let shutdown = thread::spawn(move || {
            let mut reporter = reporter;
            let started = Instant::now();
            reporter.finish(false);
            finished.send((started.elapsed(), reporter)).unwrap();
        });
        waiting.recv_timeout(Duration::from_secs(5)).unwrap();
        let stopped = result.recv_timeout(Duration::from_millis(500));
        // Release even on a failed timing assertion, so the test owns cleanup.
        release.send(()).unwrap();
        let (elapsed, reporter) = stopped.expect("publisher Drop held shutdown");
        assert!(elapsed < Duration::from_millis(500));
        reporter.done.recv_timeout(Duration::from_secs(5)).unwrap();
        shutdown.join().unwrap();
    }

    #[test]
    fn an_unblocked_reporter_says_open_failed_without_ever_claiming_readiness() {
        let (sent, reports) = mpsc::channel();
        let mut reporter = Reporter::start_with(CountSource::Days, move |report| {
            sent.send(report.clone()).unwrap();
            Ok(())
        })
        .unwrap();
        assert!(reports.recv_timeout(Duration::from_millis(20)).is_err());
        reporter.finish(true);
        let report = reports.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(report.level, Level::Warn);
        assert_eq!(report.message, "store or server failed");
        assert_eq!(value(&report, "exchanges"), "unknown");
        assert!(reports.try_iter().all(|report| report.level == Level::Warn));
    }

    #[test]
    fn a_hung_handler_is_visible_while_the_socket_owner_cannot_finish() {
        struct HeldMemory {
            entered: mpsc::SyncSender<()>,
            release: mpsc::Receiver<()>,
        }
        impl crate::server::Memory for HeldMemory {
            fn answer(&mut self, _: &[u8], _: u64) -> Answer {
                self.entered.send(()).unwrap();
                self.release.recv().unwrap();
                Answer {
                    line: "{\"ok\":true}".into(),
                    ..answer(false, false)
                }
            }
        }
        let scratch = Scratch::new("hung");
        let socket = scratch.0.join("m");
        let server = Server::bind(&socket).unwrap();
        let stopper = server.stopper();
        let (sent, reports) = mpsc::channel();
        let mut reporter = Reporter::start_with(CountSource::Days, move |report| {
            sent.send(report.clone()).unwrap();
            Ok(())
        })
        .unwrap();
        let state = Arc::clone(&reporter.state);
        let (entered, waiting) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::sync_channel(1);
        let mut store = HeldMemory {
            entered,
            release: gate,
        };
        let serving = thread::spawn(move || {
            server.run_reporting(&mut store, &mut || NOW, &mut |_| {}, Some(&state))
        });
        let client = thread::spawn(move || ask(&socket));
        waiting.recv_timeout(Duration::from_secs(5)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let report = reports
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap();
            if report.message == "answer in progress exceeds its budget" {
                assert_eq!(report.level, Level::Warn);
                break;
            }
        }
        release.send(()).unwrap();
        client.join().unwrap();
        stopper.stop();
        serving.join().unwrap().unwrap();
        reporter.finish(false);
    }

    #[test]
    #[ignore = "host timing measurement; run in release with --ignored --nocapture"]
    fn measure_health_update_and_socket_with_a_held_publisher() {
        let scratch = Scratch::new("timing");
        let (entered, waiting) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::sync_channel(1);
        let mut reporter = Reporter::start_with(CountSource::Days, move |_| {
            let _ = entered.try_send(());
            gate.recv().unwrap();
            Ok(())
        })
        .unwrap();
        reporter.state.serving();
        waiting.recv_timeout(Duration::from_secs(5)).unwrap();
        let sample = |samples: &mut Vec<u128>| {
            let started = Instant::now();
            let ticket = reporter.state.begin(br#"{"op":"kept","max_bytes":1200}"#);
            reporter
                .state
                .finish(ticket, reporter.state.now(), &answer(false, false), None);
            samples.push(started.elapsed().as_nanos());
        };
        let mut updates = Vec::new();
        sample(&mut updates);
        let first = updates[0];
        updates.clear();
        for _ in 0..10_000 {
            sample(&mut updates);
        }
        updates.sort_unstable();
        println!(
            "health_update_ns first={first} samples={} median={} p95={} p99={} max={}",
            updates.len(),
            updates[5000],
            updates[9500],
            updates[9900],
            updates[9999]
        );
        assert!(
            updates[9900] < 1_000_000,
            "host health update p99 target is 1 ms"
        );
        for enabled in [false, true] {
            let socket = scratch.0.join("m");
            let server = Server::bind(&socket).unwrap();
            let stopper = server.stopper();
            let mut store = scratch.store();
            let state = Arc::clone(&reporter.state);
            let serving = thread::spawn(move || {
                server.run_reporting(
                    &mut store,
                    &mut || NOW,
                    &mut |_| {},
                    enabled.then_some(&state),
                )
            });
            let mut times = Vec::new();
            for _ in 0..101 {
                let started = Instant::now();
                ask(&socket);
                times.push(started.elapsed().as_nanos());
            }
            let first = times.remove(0);
            times.sort_unstable();
            println!(
                "socket_kept_ns reporting={enabled} first={first} samples=100 median={} p95={} p99={} max={}",
                times[50], times[95], times[99], times[99]
            );
            stopper.stop();
            serving.join().unwrap().unwrap();
        }
        reporter.finish(false);
        release.send(()).unwrap();
        reporter.done.recv_timeout(Duration::from_secs(5)).unwrap();
    }
}
