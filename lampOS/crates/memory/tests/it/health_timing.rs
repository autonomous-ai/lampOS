//! Synthetic host measurements, not lamp qualification. Run alone in release:
//! `cargo test -p lamp-memory --release --test it health_timing:: --offline -- --ignored --nocapture`
use lamp_bus::Bus;
use lamp_memory::{
    Options, Server, Store,
    days::Days,
    health::{CountSource, Reporter, State},
    health_counts::{self, Counts},
    protocol::Answer,
    server::{Memory, Stopper},
};
use lamp_msgs::{Diagnostic, Level};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, BufRead, BufReader, Write},
    os::unix::{fs::DirBuilderExt, net::UnixStream},
    path::{Path, PathBuf},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const NOW: u64 = 1_791_892_800_000;
const WAIT: Duration = Duration::from_secs(5);
const INITIAL: Counts = Counts {
    exchanges: 6000,
    kept: 200,
};

struct Scratch(PathBuf);
impl Scratch {
    fn new(case: usize) -> Self {
        // Short paths also fit the bus's Unix sockets on macOS.
        let path = PathBuf::from(format!("/tmp/lm-ht-{}-{case}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
    fn fixture(&self) -> Options {
        let person = self.0.join("store/everyone");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&person)
            .unwrap();
        let (heard, said) = (
            "what did we say about the coffee order ".repeat(3),
            "we said ".repeat(26),
        );
        let mut id = 0u64;
        for day in 0..30u64 {
            let at = NOW - (29 - day) * 86_400_000;
            let mut lines = String::new();
            for n in 0..200 {
                id += 1;
                lines.push_str(&format!("{}\n", json!({"id":id,"at_unix_ms":at+n*1000,"heard":format!("{heard}{n}"),"said":format!("{said}{n}")})));
            }
            let name = format!(
                "journal-2026-{:02}-{:02}.jsonl",
                if day < 17 { 9 } else { 10 },
                if day < 17 { day + 14 } else { day - 16 }
            );
            fs::write(person.join(name), lines).unwrap();
        }
        let mut kept = String::new();
        for n in 0..200 {
            id += 1;
            kept.push_str(&format!("{}\n", json!({"id":id,"at_unix_ms":NOW,"text":format!("takes the coffee black on day {n}")})));
        }
        fs::write(person.join("kept.jsonl"), kept).unwrap();
        Options {
            dir: self.0.join("store"),
            keep_days: 30,
            local: lamp_memory::store::LocalTime::Offset(0),
        }
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Stop/join even if a measurement assertion panics. The server thread ends
/// its reporter only after serving has ended.
struct Running {
    stop: Stopper,
    worker: Option<JoinHandle<io::Result<()>>>,
}
impl Running {
    fn finish(&mut self) -> io::Result<()> {
        self.stop.stop();
        self.worker
            .take()
            .expect("one join")
            .join()
            .expect("server panicked")
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            self.stop.stop();
            let _ = worker.join();
        }
    }
}

fn emit(mut result: Value) {
    result["os"] = json!(std::env::consts::OS);
    result["arch"] = json!(std::env::consts::ARCH);
    result["build"] = json!(if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    });
    println!("{result}");
}

fn measure(mut record: Value, repeated: usize, mut sample: impl FnMut() -> Duration) -> u128 {
    let first = sample().as_nanos();
    let mut samples: Vec<_> = (0..repeated).map(|_| sample().as_nanos()).collect();
    samples.sort_unstable();
    let percentile = |p: usize| samples[(repeated * p).div_ceil(100) - 1];
    record["first_ns"] = json!(first);
    record["repeated"] = json!(repeated);
    record["median_ns"] = json!(percentile(50));
    record["p95_ns"] = json!(percentile(95));
    record["p99_ns"] = json!(percentile(99));
    record["max_ns"] = json!(samples[repeated - 1]);
    emit(record);
    percentile(99)
}

fn request(socket: &Path, body: &Value) -> Duration {
    let started = Instant::now();
    let mut stream = UnixStream::connect(socket).unwrap();
    stream.set_write_timeout(Some(WAIT)).unwrap();
    writeln!(stream, "{body}").unwrap();
    let remaining = WAIT
        .checked_sub(started.elapsed())
        .expect("request exceeded 5 s");
    stream.set_read_timeout(Some(remaining)).unwrap();
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).unwrap();
    let elapsed = started.elapsed();
    assert!(elapsed <= WAIT, "request exceeded 5 s");
    assert!(line.ends_with('\n'), "response was not complete");
    let response: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(response["ok"], true, "{response}");
    elapsed
}

#[test]
#[ignore = "release host measurement; does not qualify a lamp"]
fn health_costs_and_socket_requests_with_identical_synthetic_stores() {
    emit(
        json!({"measurement":"context","corpus":"30 days, 6000 exchanges, 200 kept; synthetic only; each socket case starts fresh; append adds 51 exchanges","first":"first measured operation in this test process after fixture creation/store opening; socket cases wait for ready; filesystem caches are not evicted","percentiles":"nearest rank","enabled_socket_start":"also waits for an actual health Diagnostic before its first request; excludes that startup wait","publication":"normal bus publication; held-publisher measurements are separate in health.rs"}),
    );
    let mut case = 0;
    for days in [false, true] {
        let backend = if days { "days" } else { "jsonl" };
        let scratch = Scratch::new(case);
        case += 1;
        let options = scratch.fixture();
        let store: Box<dyn Memory + Send> = if days {
            let mut store = Days::open(options, NOW).unwrap();
            let started = Instant::now();
            store.enable_health_counts().unwrap();
            let elapsed = started.elapsed();
            emit(
                json!({"measurement":"counter_enable","backend":backend,"health":true,"samples":1,"ns":elapsed.as_nanos(),"boundary":"enable_health_counts only; excludes store open/import and subsequent count read"}),
            );
            assert_eq!(store.health_counts().unwrap(), INITIAL);
            Box::new(store)
        } else {
            let store = Store::open(options, NOW).unwrap();
            let legacy_dir = scratch.0.join("store");
            measure(
                json!({"measurement":"legacy_count_scan","backend":backend,"health":true,"boundary":"read and parse retained legacy files; excludes store open and bus publication"}),
                10,
                || {
                    let started = Instant::now();
                    let counts = health_counts::legacy(&legacy_dir).unwrap();
                    let elapsed = started.elapsed();
                    assert_eq!(counts, INITIAL);
                    elapsed
                },
            );
            Box::new(store)
        };
        let state = State::default();
        state.serving();
        let answer = Answer {
            line: String::new(),
            log: String::new(),
            storage_fault: false,
            wrote: false,
        };
        let p99 = measure(
            json!({"measurement":"health_bookkeeping","backend":backend,"health":true,"p99_target_ns":1_000_000,"boundary":"State.begin classification, answered_at, optional Days count read, State.finish; excludes store.answer, legacy scans and bus work"}),
            1000,
            || {
                let started = Instant::now();
                let ticket = state.begin(br#"{"op":"kept","max_bytes":1200}"#);
                let ended = state.answered_at();
                state.finish(ticket, ended, &answer, store.health_counts());
                started.elapsed()
            },
        );
        assert!(
            p99 < 1_000_000,
            "health bookkeeping p99 target is 1 ms on this host"
        );
        let large =
            json!({"op":"append","at_unix_ms":NOW,"heard":"x".repeat(60 * 1024)}).to_string();
        let p99 = measure(
            json!({"measurement":"health_bookkeeping_large_request","backend":backend,"health":true,"request_bytes":large.len(),"p99_target_ns":1_000_000,"boundary":"same bookkeeping boundary, with a valid append request carrying 60 KiB of heard text; no store operation"}),
            1000,
            || {
                let started = Instant::now();
                let ticket = state.begin(large.as_bytes());
                let ended = state.answered_at();
                state.finish(ticket, ended, &answer, store.health_counts());
                started.elapsed()
            },
        );
        assert!(
            p99 < 1_000_000,
            "large-request health bookkeeping p99 target is 1 ms"
        );
        drop(store);
        for enabled in [false, true] {
            for op in ["kept", "append"] {
                let scratch = Scratch::new(case);
                case += 1;
                let options = scratch.fixture();
                let mut store: Box<dyn Memory + Send> = if days {
                    let mut store = Days::open(options, NOW).unwrap();
                    if enabled {
                        store.enable_health_counts().unwrap();
                    }
                    Box::new(store)
                } else {
                    Box::new(Store::open(options, NOW).unwrap())
                };
                let mut subscriber = enabled.then(|| {
                    Bus::new(scratch.0.join("b"))
                        .unwrap()
                        .subscribe_typed(Diagnostic::TOPIC, "t", Diagnostic::TYPE)
                        .unwrap()
                });
                let mut reporter = enabled.then(|| {
                    Reporter::start(
                        scratch.0.join("b"),
                        if days {
                            CountSource::Days
                        } else {
                            CountSource::Legacy(scratch.0.join("store"))
                        },
                    )
                    .unwrap()
                });
                if let Some(reporter) = &reporter
                    && let Some(counts) = store.health_counts()
                {
                    reporter.state().counts(counts);
                }
                let socket = scratch.0.join("m");
                let server = Server::bind(&socket).unwrap();
                let stop = server.stopper();
                let worker = thread::spawn(move || {
                    let result = server.run_reporting(
                        &mut *store,
                        &mut || NOW,
                        &mut |_| {},
                        reporter.as_ref().map(Reporter::state),
                    );
                    if let Some(reporter) = &mut reporter {
                        reporter.finish(result.is_err());
                    }
                    result
                });
                let mut running = Running {
                    stop,
                    worker: Some(worker),
                };
                let until = Instant::now() + WAIT;
                while !scratch.0.join("ready").exists() {
                    assert!(Instant::now() < until, "server did not become ready in 5 s");
                    thread::sleep(Duration::from_millis(1));
                }
                if let Some(subscriber) = &mut subscriber {
                    let report: Diagnostic = subscriber
                        .recv(WAIT)
                        .unwrap()
                        .expect("no health report within 5 s")
                        .parse()
                        .unwrap();
                    assert_eq!(report.name, "memory");
                    assert_eq!(report.level, Level::Ok);
                    for (key, expected) in [("exchanges", "6000"), ("kept", "200")] {
                        assert_eq!(
                            report
                                .values
                                .iter()
                                .find(|v| v.key == key)
                                .map(|v| v.value.as_str()),
                            Some(expected)
                        );
                    }
                }
                let body = if op == "kept" {
                    json!({"op":op,"max_bytes":1200})
                } else {
                    json!({"op":op,"at_unix_ms":NOW,"heard":"synthetic coffee request","said":"synthetic reply"})
                };
                measure(
                    json!({"measurement":"socket_request","backend":backend,"health":enabled,"op":op,"boundary":"connect, serialize/send request, read full response and close connection; excludes response JSON decoding and startup"}),
                    50,
                    || request(&socket, &body),
                );
                running.finish().unwrap();
            }
        }
    }
}
