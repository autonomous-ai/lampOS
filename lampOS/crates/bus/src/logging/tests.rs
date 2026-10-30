use super::*;
use crate::Header;
use serde_json::json;
use std::{fs, path::PathBuf, sync::Mutex};

const WAIT: Duration = Duration::from_secs(20);

fn entry() -> Log {
    Log {
        part: "body".into(),
        level: LogLevel::Info,
        code: "ready".into(),
        message: "motor checks complete".into(),
        attempt: 1,
        created_unix_ms: 1_791_964_800_000,
    }
}

fn message(log: &Log) -> Message {
    Message {
        header: Header {
            topic: Log::TOPIC.into(),
            kind: Kind::Event,
            enc: JSON.into(),
            type_name: Some(Log::TYPE.into()),
            source: log.part.clone(),
            boot: "0123456789abcdef".into(),
            seq: 1,
            at_us: 10,
            at_unix_ms: log.created_unix_ms + 5,
            expires_ms: None,
            clock: None,
        },
        payload: serde_json::to_vec(log).unwrap(),
        gap: 0,
        publisher_restarted: false,
        expired: false,
    }
}

#[test]
fn schema_round_trip_bounds_and_forward_compatibility() {
    let log = entry();
    let mut value = serde_json::to_value(&log).unwrap();
    value["future_field"] = json!({"added": true});
    assert_eq!(serde_json::from_value::<Log>(value.clone()).unwrap(), log);
    value["level"] = json!("probably_ok");
    assert!(serde_json::from_value::<Log>(value).is_err());
    for field in [
        "part",
        "level",
        "code",
        "message",
        "attempt",
        "created_unix_ms",
    ] {
        let mut value = serde_json::to_value(&log).unwrap();
        value.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<Log>(value).is_err(), "{field}");
    }
    let mut bounded = log.clone();
    bounded.part = "a".repeat(32);
    bounded.code = "c".repeat(MAX_CODE_BYTES);
    bounded.message = "é".repeat(MAX_MESSAGE_BYTES / 2);
    assert!(bounded.check().is_ok());
    bounded.message.push('é');
    assert!(bounded.check().is_err());
    bounded.message = "\0".repeat(MAX_MESSAGE_BYTES);
    assert!(bounded.check().is_ok());
    assert!(Packet::encode(&bounded, 0).unwrap().len <= MAX_RECORD_BYTES);
    bounded.code.push('x');
    assert!(bounded.check().is_err());
    for code in ["", "Ready", "bad code", "escape\x1b", "../secret"] {
        let mut invalid = log.clone();
        invalid.code = code.into();
        assert!(invalid.check().is_err(), "{code:?}");
    }
    bounded = log;
    bounded.attempt = 0;
    assert!(bounded.check().is_err());
    assert!(LogLevel::Debug < LogLevel::Info && LogLevel::Error < LogLevel::Fatal);
    assert_eq!("warn".parse::<LogLevel>().unwrap(), LogLevel::Warn);
    assert!("warning".parse::<LogLevel>().is_err());
}

#[test]
fn strict_envelope_rejects_non_logs_and_identity_mismatch() {
    let log = entry();
    let original = message(&log);
    assert_eq!(Log::from_message(&original).unwrap(), log);
    for changed in [
        Header {
            type_name: None,
            ..original.header.clone()
        },
        Header {
            type_name: Some("lamp_msgs/Diagnostic".into()),
            ..original.header.clone()
        },
        Header {
            kind: Kind::Latest,
            ..original.header.clone()
        },
        Header {
            enc: "none".into(),
            ..original.header.clone()
        },
        Header {
            source: "voice".into(),
            ..original.header.clone()
        },
        Header {
            topic: "other".into(),
            ..original.header.clone()
        },
    ] {
        assert!(
            Log::from_message(&Message {
                header: changed,
                ..original.clone()
            })
            .is_err()
        );
    }
    let mut oversized = original.clone();
    oversized.payload.resize(MAX_RECORD_BYTES + 1, b' ');
    assert!(Log::from_message(&oversized).is_err());
    let mut malformed = original;
    malformed.payload = b"{broken".to_vec();
    assert!(Log::from_message(&malformed).is_err());
    let declaration = Log::declaration();
    assert_eq!(declaration.type_name, Log::TYPE);
    assert_eq!(declaration.kind, Kind::Event);
    assert_eq!(
        (declaration.period_ms, declaration.lifespan_ms),
        (None, None)
    );
}

#[test]
fn bounded_encoder_refuses_a_write_before_growing() {
    let mut packet = Packet::encode(&entry(), 0).unwrap();
    let before = packet.len;
    assert!(packet.write_all(&[b'x'; MAX_RECORD_BYTES]).is_err());
    assert_eq!(packet.len, before);
}

struct Gate {
    entered: SyncSender<()>,
    release: Receiver<()>,
}

impl Gate {
    fn wait(self) {
        self.entered.send(()).unwrap();
        self.release.recv_timeout(WAIT).unwrap();
    }
}

fn gate() -> (Gate, Receiver<()>, SyncSender<()>) {
    let (entered, arrived) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    (
        Gate {
            entered,
            release: released,
        },
        arrived,
        release,
    )
}

#[derive(Default)]
struct Memory {
    logs: Arc<Mutex<Vec<Log>>>,
    publish_gate: Option<Gate>,
    idle_gate: Option<Gate>,
    fail: bool,
    drops: u64,
    wrote: Option<SyncSender<()>>,
}

impl Sink for Memory {
    fn publish(&mut self, packet: &Packet) -> (io::Result<()>, u64) {
        if let Some(gate) = self.publish_gate.take() {
            gate.wait();
        }
        self.logs
            .lock()
            .unwrap()
            .push(serde_json::from_slice(&packet.bytes[..packet.len]).unwrap());
        if let Some(wrote) = &self.wrote {
            wrote.try_send(()).unwrap();
        }
        let result = if self.fail {
            Err(io::Error::other("injected bus failure"))
        } else {
            Ok(())
        };
        (result, self.drops)
    }

    fn alive(&mut self) -> io::Result<()> {
        if let Some(gate) = self.idle_gate.take() {
            gate.wait();
        }
        if self.fail {
            Err(io::Error::other("injected liveness failure"))
        } else {
            Ok(())
        }
    }
}

#[test]
fn default_is_disabled_and_never_validates_or_admits() {
    let mut logger = Logger::default();
    assert_eq!(
        logger.try_log(LogLevel::Info, "bad code", &"x".repeat(10_000)),
        Admission::Disabled
    );
    assert_eq!(logger.finish(Duration::ZERO), Report::default());
    assert_eq!(Logger::disabled().report(), Report::default());
}

#[test]
fn stalled_worker_has_fixed_capacity_and_attempt_gaps() {
    let (pause, entered, release) = gate();
    let logs = Arc::new(Mutex::new(Vec::new()));
    let (wrote, written) = mpsc::sync_channel(QUEUE_CAPACITY + 2);
    let mut logger = Logger::spawn(
        "body",
        Memory {
            logs: Arc::clone(&logs),
            publish_gate: Some(pause),
            wrote: Some(wrote),
            ..Memory::default()
        },
    )
    .unwrap();
    assert_eq!(
        logger.try_log(LogLevel::Info, "first", ""),
        Admission::Queued
    );
    entered.recv_timeout(WAIT).unwrap();
    for _ in 0..QUEUE_CAPACITY {
        assert_eq!(
            logger.try_log(LogLevel::Info, "queued", ""),
            Admission::Queued
        );
    }
    assert_eq!(
        logger.try_log(LogLevel::Info, "overflow", ""),
        Admission::Full
    );
    assert_eq!(
        logger.try_log(LogLevel::Info, "bad code", ""),
        Admission::Invalid
    );
    let waiting = logger.report();
    assert_eq!(
        (
            waiting.accepted,
            waiting.full,
            waiting.invalid,
            waiting.processed
        ),
        (65, 1, 1, 0)
    );
    release.send(()).unwrap();
    for _ in 0..=QUEUE_CAPACITY {
        written.recv_timeout(WAIT).unwrap();
    }
    assert_eq!(
        logger.try_log(LogLevel::Info, "after_gap", ""),
        Admission::Queued
    );
    let report = logger.finish(FINISH_WAIT);
    assert!(report.confirmed && report.worker_finished && report.closed);
    assert_eq!((report.processed, report.unconfirmed), (66, 0));
    let logs = logs.lock().unwrap();
    assert_eq!(
        logs.iter()
            .take(65)
            .map(|log| log.attempt)
            .collect::<Vec<_>>(),
        (1..=65).collect::<Vec<_>>()
    );
    assert_eq!(logs.last().unwrap().attempt, 68);
    assert_eq!(
        logger.try_log(LogLevel::Info, "closed", ""),
        Admission::Closed
    );
    assert_eq!(logger.report().rejected_closed, 1);
}

#[test]
fn close_after_an_empty_observation_still_drains_the_final_accepted_record() {
    let (pause, entered, release) = gate();
    let logs = Arc::new(Mutex::new(Vec::new()));
    let mut logger = Logger::spawn(
        "body",
        Memory {
            logs: Arc::clone(&logs),
            idle_gate: Some(pause),
            ..Memory::default()
        },
    )
    .unwrap();
    // alive is entered only after recv_timeout observed an empty channel.
    entered.recv_timeout(WAIT).unwrap();
    assert_eq!(
        logger.try_log(LogLevel::Info, "final", ""),
        Admission::Queued
    );
    let timed_out = logger.finish(Duration::ZERO);
    assert!(!timed_out.confirmed && timed_out.closed);
    assert_eq!(timed_out.unconfirmed, 1);
    release.send(()).unwrap();
    let finished = logger.finish(FINISH_WAIT);
    assert!(finished.confirmed && finished.worker_finished);
    assert_eq!((finished.processed, finished.unconfirmed), (1, 0));
    assert_eq!(logs.lock().unwrap()[0].code, "final");
    assert!(
        !timed_out.confirmed,
        "a previous timeout is never rewritten"
    );
}

#[test]
fn drop_does_not_wait_for_a_stalled_worker() {
    let (pause, entered, release) = gate();
    let (wrote, written) = mpsc::sync_channel(1);
    let mut logger = Logger::spawn(
        "body",
        Memory {
            publish_gate: Some(pause),
            wrote: Some(wrote),
            ..Memory::default()
        },
    )
    .unwrap();
    assert_eq!(
        logger.try_log(LogLevel::Info, "before_drop", ""),
        Admission::Queued
    );
    entered.recv_timeout(WAIT).unwrap();
    let (dropped, observed) = mpsc::sync_channel(1);
    let dropping = thread::spawn(move || {
        drop(logger);
        dropped.send(()).unwrap();
    });
    observed.recv_timeout(WAIT).unwrap();
    release.send(()).unwrap();
    written.recv_timeout(WAIT).unwrap();
    dropping.join().unwrap();
}

#[test]
fn failed_publish_is_counted_without_retrying_or_losing_subscriber_drop_counts() {
    let mut logger = Logger::spawn(
        "body",
        Memory {
            fail: true,
            drops: 2,
            ..Memory::default()
        },
    )
    .unwrap();
    assert_eq!(
        logger.try_log(LogLevel::Error, "failure", ""),
        Admission::Queued
    );
    let report = logger.finish(FINISH_WAIT);
    assert!(report.confirmed);
    assert_eq!(
        (
            report.accepted,
            report.processed,
            report.published,
            report.publish_errors
        ),
        (1, 1, 0, 1)
    );
    assert_eq!((report.subscriber_drops, report.unconfirmed), (2, 0));
}

struct Root(PathBuf);

impl Root {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(std::env::temp_dir().join(format!(
            "lg{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
    fn bus(&self) -> Bus {
        Bus::new(&self.0).unwrap().rescan_every(Duration::ZERO)
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn startup_refuses_bad_root_names_and_conflicting_declarations() {
    let root = Root::new();
    assert!(Logger::start(&root.bus(), "Bad Part").is_err());
    assert!(!root.0.exists());
    fs::write(&root.0, b"not a directory").unwrap();
    assert!(Logger::start(&root.bus(), "body").is_err());
    fs::remove_file(&root.0).unwrap();
    let other = root
        .bus()
        .publisher_declared(
            Log::TOPIC,
            &Declaration::new("lamp_msgs/Other", 1, Kind::Event),
            "other",
        )
        .unwrap();
    assert!(Logger::start(&root.bus(), "body").is_err());
    drop(other);
}

#[test]
fn real_bus_preserves_attempt_identity_separately_from_publication_sequence() {
    let root = Root::new();
    let bus = root.bus();
    let mut follower = bus.subscribe_typed(Log::TOPIC, "test", Log::TYPE).unwrap();
    let mut logger = Logger::start(&bus, "body").unwrap();
    assert_eq!(
        logger.try_log(LogLevel::Info, "first", "ready"),
        Admission::Queued
    );
    assert_eq!(
        logger.try_log(LogLevel::Info, "bad code", ""),
        Admission::Invalid
    );
    assert_eq!(
        logger.try_log(LogLevel::Warn, "third", "check"),
        Admission::Queued
    );
    assert!(logger.finish(FINISH_WAIT).confirmed);
    for (sequence, attempt) in [(1, 1), (2, 3)] {
        let message = follower.recv(WAIT).unwrap().unwrap();
        let log = Log::from_message(&message).unwrap();
        assert_eq!(
            (message.header.seq, message.gap, log.attempt),
            (sequence, 0, attempt)
        );
        assert!(log.created_unix_ms <= message.header.at_unix_ms);
    }
    assert!(bus.latest(Log::TOPIC).unwrap().is_none());
    assert!(bus.publishers(Log::TOPIC).unwrap().is_empty());
}

#[test]
fn a_slow_log_follower_does_not_hold_a_fast_one_and_loss_remains_visible() {
    let root = Root::new();
    let bus = root.bus();
    let mut slow = bus.subscribe_typed(Log::TOPIC, "slow", Log::TYPE).unwrap();
    let mut fast = bus.subscribe_typed(Log::TOPIC, "fast", Log::TYPE).unwrap();
    let mut logger = Logger::start(&bus, "body").unwrap();
    for sequence in 1..=3_000 {
        assert_eq!(
            logger.try_log(LogLevel::Info, "tick", "operational summary"),
            Admission::Queued
        );
        let message = fast.recv(WAIT).unwrap().unwrap();
        assert_eq!((message.header.seq, message.gap), (sequence, 0));
    }
    // The worker sends to `fast` before `slow` (subscribers in name order),
    // so `fast` having the last record does not mean `slow` was tried with
    // it. Sent while the drain below makes room, it would end the drain at
    // 3,000 (#1204): wait until the worker has published it to both.
    let deadline = Instant::now() + WAIT;
    while logger.report().processed < 3_000 {
        assert!(
            Instant::now() < deadline,
            "the worker published every record"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let (mut last, mut delivered, mut lost) = (0, 0, 0);
    while let Some(message) = slow.recv(Duration::ZERO).unwrap() {
        (last, delivered, lost) = (message.header.seq, delivered + 1, lost + message.gap);
    }
    assert!(last > 0 && last < 3_000, "the slow queue must have filled");
    assert_eq!(
        logger.try_log(LogLevel::Warn, "after_loss", ""),
        Admission::Queued
    );
    // A send to the slow follower already under way when the drain ended can
    // still land before the new record (seen on the ARM64 runner): every
    // loss is still said, as the gap before what came next.
    let message = loop {
        let message = slow.recv(WAIT).unwrap().unwrap();
        lost += message.gap;
        if message.header.seq == 3_001 {
            break message;
        }
        (last, delivered) = (message.header.seq, delivered + 1);
    };
    assert_eq!((message.header.seq, message.gap), (3_001, 3_000 - last));
    assert_eq!(Log::from_message(&message).unwrap().attempt, 3_001);
    assert!(lost > 0, "the slow follower lost records, and was told so");
    assert_eq!(lost, 3_000 - delivered);
    let report = logger.finish(FINISH_WAIT);
    assert!(report.confirmed);
    assert_eq!(report.subscriber_drops, lost);
}

#[test]
#[ignore = "manual host admission timing; no device or end-to-end claim"]
fn admission_cost_with_a_stalled_worker() {
    let (pause, entered, release) = gate();
    let mut logger = Logger::spawn(
        "body",
        Memory {
            publish_gate: Some(pause),
            ..Memory::default()
        },
    )
    .unwrap();
    let started = Instant::now();
    assert_eq!(
        logger.try_log(LogLevel::Info, "first", "host timing"),
        Admission::Queued
    );
    let first = started.elapsed();
    entered.recv_timeout(WAIT).unwrap();
    let mut samples = Vec::with_capacity(10_000);
    for _ in 0..10_000 {
        let started = Instant::now();
        let _ = logger.try_log(LogLevel::Info, "repeated", "host timing");
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    eprintln!(
        "host try_log admission, stalled worker, first={first:?}, n={}, median={:?}, p95={:?}, worst={:?}; construction and delivery excluded",
        samples.len(),
        samples[5_000],
        samples[9_499],
        samples.last().unwrap()
    );
    release.send(()).unwrap();
    assert!(logger.finish(FINISH_WAIT).confirmed);
}
