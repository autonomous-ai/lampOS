//! Authored host sockets and persistence exercise the actual consumer policy.
//! No provider, speaker, device, or audible-completion claim is involved.
use super::*;
use lamp_bus::{Header, JSON};
use lamp_msgs::{ClockStamp, ScheduleNotice};
use lamp_schedule::{
    delivery::{MAX_ENTRIES, QualifiedCompletion, Resolution, RetirementBasis},
    engine::Engine,
    protocol::{Guard, refusal},
    time::{ClockSample, When},
};
use serde_json::json;
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{fs::DirBuilderExt, net::UnixListener},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
};

const STORE_ID: &str = "0123456789abcdef0123456789abcdef";
const OTHER_STORE: &str = "11111111111111111111111111111111";
const LEDGER_ID: &str = "abcdef0123456789abcdef0123456789";
const CLOCK: &str = "authored-clock";
const SESSION: &str = "consumer-one";
const WALL: i64 = 1_791_964_800_000;
const TEXT: &str = "A private reminder that must not enter bus metadata";

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let path = fs::canonicalize("/tmp").unwrap().join(format!(
            "sc-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
    fn dir(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        path
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct Data {
    bytes: Option<Vec<u8>>,
    writes: usize,
    fail: bool,
    delay: Duration,
}
#[derive(Clone, Default)]
struct Disk(Arc<Mutex<Data>>);
impl Persistence for Disk {
    fn load(&self) -> io::Result<Option<Vec<u8>>> {
        Ok(self.0.lock().unwrap().bytes.clone())
    }
    fn commit(&mut self, bytes: &[u8]) -> io::Result<()> {
        let delay = self.0.lock().unwrap().delay;
        if !delay.is_zero() {
            thread::sleep(delay);
        }
        let mut data = self.0.lock().unwrap();
        if data.fail {
            return Err(io::Error::other("authored storage failure"));
        }
        data.bytes = Some(bytes.to_vec());
        data.writes += 1;
        Ok(())
    }
}

/// Every accepted fixture connection reads one request. Closing without a
/// response models uncertainty; it does not assert whether a commit happened.
struct Socket {
    root: Temp,
    stop: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<Request>>>,
    worker: Option<JoinHandle<()>>,
}
impl Socket {
    fn new(mut reply: impl FnMut(&Request) -> Option<Value> + Send + 'static) -> Self {
        let root = Temp::new();
        let listener = UnixListener::bind(root.0.join("call.sock")).unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
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
                if let Some(value) = reply(&request) {
                    let _ = writeln!(stream, "{value}");
                }
            }
        });
        Self {
            root,
            stop,
            requests,
            worker: Some(worker),
        }
    }
    fn path(&self) -> PathBuf {
        self.root.0.join("call.sock")
    }
    fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}
impl Drop for Socket {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
    }
}

fn sample(ms: u64) -> ClockSample {
    ClockSample {
        unix_ms: WALL + ms as i64,
        boot_ms: ms,
        boot_id: CLOCK.into(),
        wall_trusted: true,
    }
}
fn stamp(at: u64, sequence: u64) -> ClockStamp {
    ClockStamp {
        clock_id: CLOCK.into(),
        at_us: at,
        sequence,
    }
}
fn guard<P: Persistence>(engine: &Engine<P>, name: &str) -> Guard {
    Guard {
        store_id: engine.store_id().into(),
        revision: engine.revision(),
        request_id: name.into(),
    }
}
fn prepared_engine<P: Persistence>(disk: P, count: usize) -> Engine<P> {
    let mut engine = Engine::open(disk, "Etc/UTC", &sample(0), STORE_ID).unwrap();
    for index in 0..count {
        let request = Request::Set {
            person: None,
            guard: guard(&engine, &format!("set-{index}")),
            text: TEXT.into(),
            when: When::After { seconds: 1 },
        };
        engine.answer(request, &sample(0)).unwrap();
    }
    engine.tick(&sample(1000)).unwrap();
    engine
}
fn answer<P: Persistence>(engine: &mut Engine<P>, request: &Request) -> Value {
    engine
        .answer(request.clone(), &sample(1000))
        .unwrap_or_else(|e| refusal(e.code, e.message))
}
fn served(count: usize) -> (Arc<Mutex<Engine<Disk>>>, Socket) {
    let engine = Arc::new(Mutex::new(prepared_engine(Disk::default(), count)));
    let shared = engine.clone();
    let socket = Socket::new(move |request| Some(answer(&mut shared.lock().unwrap(), request)));
    (engine, socket)
}
fn due<P: Persistence>(engine: &Engine<P>, now: u64) -> ScheduleDue {
    engine.due(&sample(1000), stamp(now, 1), true)
}
fn consumer(disk: Disk) -> Consumer<Disk> {
    Consumer::new(
        DeliveryLedger::open(disk, LEDGER_ID).unwrap(),
        SESSION.into(),
    )
}
fn key(due: &ScheduleDue) -> DeliveryKey {
    let notice = due.oldest.as_ref().unwrap();
    DeliveryKey {
        store_id: due.store_id.clone(),
        id: notice.id,
        delivery: notice.delivery,
    }
}
fn take_offer<P: Persistence>(consumer: &mut Consumer<P>) -> Offer {
    let messages: Vec<_> = consumer.outgoing.drain(..).collect();
    assert_eq!(messages.len(), 1, "expected one offer");
    match messages.into_iter().next().unwrap() {
        FromWorker::Offer { offer } => offer,
        other => panic!("expected offer, got {other:?}"),
    }
}
fn no_offer<P: Persistence>(consumer: &Consumer<P>) {
    assert!(
        !consumer
            .outgoing
            .iter()
            .any(|m| matches!(m, FromWorker::Offer { .. }))
    );
}
fn completion(token: &PreparedToken) -> QualifiedCompletion {
    QualifiedCompletion {
        correlation: token.correlation.clone(),
        owner_boot: [7; 16],
        turn: 1,
        microphone_generation: 2,
        provider_session: 3,
        playback_sequence: 4,
        final_epoch: 0,
        final_frame: 4800,
        retired_generations: 2,
    }
}
fn begin<P: Persistence>(
    consumer: &mut Consumer<P>,
    engine: &Arc<Mutex<Engine<Disk>>>,
    socket: &Socket,
) -> Offer {
    let now = lamp_ipc::monotonic_us();
    let report = due(&engine.lock().unwrap(), now);
    consumer.tick(&socket.path(), Some(&report), now).unwrap();
    take_offer(consumer)
}
fn wire(due: &ScheduleDue, boot: &str, sequence: u64) -> Message {
    Message {
        header: Header {
            topic: ScheduleDue::TOPIC.into(),
            kind: Kind::Latest,
            enc: JSON.into(),
            type_name: Some(ScheduleDue::TYPE.into()),
            source: "schedule".into(),
            boot: boot.into(),
            seq: sequence,
            at_us: 999_999_999,
            at_unix_ms: 1,
            expires_ms: Some(3000),
            clock: None,
        },
        payload: serde_json::to_vec(due).unwrap(),
        gap: 0,
        publisher_restarted: false,
        expired: false,
    }
}
fn notice(at: u64, sequence: u64) -> ScheduleDue {
    ScheduleDue {
        stamp: stamp(at, sequence),
        store_id: STORE_ID.into(),
        revision: 2,
        enabled: true,
        ready: true,
        pending: 1,
        waiting_for_clock: 0,
        oldest: Some(ScheduleNotice {
            id: 1,
            delivery: 1,
            due_unix_ms: WALL + 1000,
            late_by_ms: Some(0),
            late: false,
        }),
    }
}

#[test]
fn bus_authority_requires_exact_type_source_clock_and_fresh_enabled_payload() {
    let at = 10_000_000;
    let good = wire(&notice(at, 1), "1111111111111111", 1);
    assert!(Freshness::default().accept(&good, CLOCK, at).is_ok());
    assert!(
        Freshness::default()
            .accept(&good, CLOCK, at + DUE_AGE_US)
            .is_ok()
    );
    assert!(
        Freshness::default()
            .accept(&good, CLOCK, at + DUE_AGE_US + 1)
            .is_err()
    );
    assert!(Freshness::default().accept(&good, CLOCK, at - 1).is_err());
    assert!(
        Freshness::default()
            .accept(&good, "other-boot", at)
            .is_err()
    );
    for field in [
        "expired", "topic", "kind", "encoding", "source", "type", "ttl", "large",
    ] {
        let mut bad = good.clone();
        match field {
            "expired" => bad.expired = true,
            "topic" => bad.header.topic = "schedule/elsewhere".into(),
            "kind" => bad.header.kind = Kind::Event,
            "encoding" => bad.header.enc = "bytes".into(),
            "source" => bad.header.source = "other".into(),
            "type" => bad.header.type_name = None,
            "ttl" => bad.header.expires_ms = Some(3001),
            "large" => bad.payload.resize(4097, b' '),
            _ => unreachable!(),
        }
        assert!(
            Freshness::default().accept(&bad, CLOCK, at).is_err(),
            "{field}"
        );
    }
    for field in ["disabled", "not_ready", "zero_sequence", "bad_store"] {
        let mut bad = notice(at, 1);
        match field {
            "disabled" => {
                bad.enabled = false;
                bad.oldest = None;
            }
            "not_ready" => {
                bad.ready = false;
                bad.oldest = None;
            }
            "zero_sequence" => bad.stamp.sequence = 0,
            "bad_store" => bad.store_id = "not-a-store".into(),
            _ => unreachable!(),
        }
        assert!(
            Freshness::default()
                .accept(&wire(&bad, "1111111111111111", 1), CLOCK, at)
                .is_err()
        );
    }
    assert!(!String::from_utf8(good.payload).unwrap().contains(TEXT));
}

#[test]
fn retained_bus_message_never_renews_age_and_reused_typed_sequence_is_refused() {
    let at = 10_000_000;
    let mut freshness = Freshness::default();
    let first = wire(&notice(at, 4), "1111111111111111", 8);
    freshness.accept(&first, CLOCK, at).unwrap();
    assert_eq!(
        freshness.accept(&first, CLOCK, at + 1).unwrap().stamp.at_us,
        at
    );
    let reused = wire(&notice(at + 1, 4), "1111111111111111", 9);
    assert!(freshness.accept(&reused, CLOCK, at + 1).is_err());
    let mut changed_same_header = first.clone();
    changed_same_header.payload = serde_json::to_vec(&notice(at + 1, 5)).unwrap();
    assert!(
        freshness
            .accept(&changed_same_header, CLOCK, at + 1)
            .is_err()
    );
    for (header, sequence, time) in [(7, 5, at + 1), (9, 3, at + 1), (9, 5, at - 1)] {
        assert!(
            freshness
                .accept(
                    &wire(&notice(time, sequence), "1111111111111111", header),
                    CLOCK,
                    at + 1
                )
                .is_err()
        );
    }
    freshness
        .accept(
            &wire(&notice(at + 1, 5), "1111111111111111", 9),
            CLOCK,
            at + 1,
        )
        .unwrap();
    assert!(
        freshness
            .accept(&first, CLOCK, at + DUE_AGE_US + 1)
            .is_err()
    );
}

#[test]
fn publisher_restart_retires_the_previous_instance_and_has_a_finite_history() {
    let mut freshness = Freshness::default();
    let at = 10_000_000;
    for index in 0..=8 {
        let boot = format!("{index:016x}");
        freshness
            .accept(&wire(&notice(at + index, 1), &boot, 1), CLOCK, at + index)
            .unwrap();
    }
    assert!(
        freshness
            .accept(
                &wire(&notice(at + 9, 2), "0000000000000000", 2),
                CLOCK,
                at + 9
            )
            .is_err()
    );
    assert!(
        freshness
            .accept(
                &wire(&notice(at + 9, 1), "0000000000000009", 1),
                CLOCK,
                at + 9
            )
            .is_err()
    );
    assert_eq!(freshness.retired.len(), 8);
    freshness
        .accept(
            &wire(&notice(at + 10, 2), "0000000000000008", 2),
            CLOCK,
            at + 10,
        )
        .unwrap();
}

#[test]
fn real_bus_requires_one_live_declared_schedule_publisher() {
    let root = Temp::new();
    let bus = Bus::new(root.0.join("bus")).unwrap();
    let mut freshness = Freshness::default();
    let now = lamp_ipc::monotonic_us();
    assert!(freshness.read(&bus, CLOCK, now).is_err());
    let mut owner = bus
        .publisher_declared(ScheduleDue::TOPIC, &ScheduleDue::declaration(), "schedule")
        .unwrap();
    owner.publish_json(7, &notice(now, 1)).unwrap();
    freshness.read(&bus, CLOCK, now).unwrap();
    let mut foreign = bus
        .publisher_declared(ScheduleDue::TOPIC, &ScheduleDue::declaration(), "other")
        .unwrap();
    foreign.alive().unwrap();
    assert!(freshness.read(&bus, CLOCK, now).is_err());
    drop(foreign);
    freshness.read(&bus, CLOCK, now).unwrap();
    drop(owner);
    assert!(freshness.read(&bus, CLOCK, now).is_err());
}

#[test]
fn an_offer_is_committed_before_handoff_and_fresh_renewals_keep_the_same_token() {
    let (engine, socket) = served(1);
    let disk = Disk::default();
    let mut consumer = consumer(disk.clone());
    let first = begin(&mut consumer, &engine, &socket);
    assert_eq!(first.text, TEXT);
    assert!(first.fresh(lamp_ipc::monotonic_us()));
    assert_eq!(
        consumer
            .ledger
            .view(&first.token.key)
            .unwrap()
            .unwrap()
            .prepared,
        first.token
    );
    let writes = disk.0.lock().unwrap().writes;
    let second = begin(&mut consumer, &engine, &socket);
    assert_eq!(first.token, second.token);
    assert_eq!(writes, disk.0.lock().unwrap().writes);
    assert_eq!(
        socket.requests(),
        [Request::Get { id: 1 }, Request::Get { id: 1 }]
    );
    assert!(
        !String::from_utf8(disk.load().unwrap().unwrap())
            .unwrap()
            .contains(TEXT)
    );
}

#[test]
fn offered_tuple_must_match_current_private_get_store_delivery_and_revision() {
    for changed in ["delivery", "store", "revision", "not_pending", "id"] {
        let mut engine = prepared_engine(Disk::default(), 1);
        let mut reply = answer(&mut engine, &Request::Get { id: 1 });
        let now = lamp_ipc::monotonic_us();
        let due = due(&engine, now);
        match changed {
            "delivery" => reply["item"]["delivery"] = json!(2),
            "store" => reply["store_id"] = json!(OTHER_STORE),
            "revision" => reply["revision"] = json!(due.revision - 1),
            "not_pending" => reply["item"]["state"] = json!("scheduled"),
            "id" => reply["item"]["id"] = json!(2),
            _ => unreachable!(),
        }
        let socket = Socket::new(move |_| Some(reply.clone()));
        let mut consumer = consumer(Disk::default());
        consumer.tick(&socket.path(), Some(&due), now).unwrap();
        assert!(consumer.current.is_none(), "{changed}");
        assert!(consumer.ledger.entries().unwrap().is_empty(), "{changed}");
        no_offer(&consumer);
    }
}

#[test]
fn cancel_snooze_store_switch_and_missing_due_withdraw_existing_offer() {
    for change in ["cancel", "snooze", "store", "missing"] {
        let (engine, socket) = served(1);
        let mut consumer = consumer(Disk::default());
        let offered = begin(&mut consumer, &engine, &socket);
        let mut engine = engine.lock().unwrap();
        match change {
            "cancel" => {
                let request = Request::Cancel {
                    asker: None,
                    guard: guard(&engine, "cancel"),
                    id: 1,
                };
                engine.answer(request, &sample(1000)).unwrap();
            }
            "snooze" => {
                let request = Request::Snooze {
                    asker: None,
                    guard: guard(&engine, "snooze"),
                    id: 1,
                    delivery: 1,
                    seconds: 60,
                };
                engine.answer(request, &sample(1000)).unwrap();
            }
            _ => {}
        }
        let now = lamp_ipc::monotonic_us();
        let mut report = due(&engine, now);
        if change == "store" {
            report.store_id = OTHER_STORE.into();
        }
        drop(engine);
        consumer
            .tick(
                &socket.path(),
                (change != "missing").then_some(&report),
                now,
            )
            .unwrap();
        no_offer(&consumer);
        assert!(
            matches!(consumer.outgoing.pop_front(), Some(FromWorker::Withdraw { token }) if token == offered.token),
            "{change}"
        );
        assert!(
            consumer
                .ledger
                .view(&offered.token.key)
                .unwrap()
                .unwrap()
                .completion
                .is_none()
        );
    }
}

#[test]
fn completed_playout_survives_both_real_store_reopens_then_acks_without_respeech() {
    // The real Stores sync in full on each commit, and one tick plans and
    // sends the Ack. On the stepped scenario clock no logical time passes in
    // the tick, so a busy host's disk cannot spend its 250 ms socket budget
    // (lampOS#935); the budget is the other tests'. The scope is a safety net.
    let _clock = crate::scenario_clock::Scope::enter(Duration::from_secs(24)).unwrap();
    let root = Temp::new();
    let schedule_path = root.dir("schedule");
    let ledger_path = root.dir("ledger");
    let engine = Arc::new(Mutex::new(prepared_engine(
        Store::open(&schedule_path).unwrap(),
        1,
    )));
    let shared = engine.clone();
    let socket = Socket::new(move |request| Some(answer(&mut shared.lock().unwrap(), request)));
    let mut first = Consumer::new(
        DeliveryLedger::open(Store::open(&ledger_path).unwrap(), LEDGER_ID).unwrap(),
        SESSION.into(),
    );
    let now = monotonic_us();
    let report = due(&engine.lock().unwrap(), now);
    first.tick(&socket.path(), Some(&report), now).unwrap();
    let offered = take_offer(&mut first);
    first
        .command(
            ToWorker::Complete {
                token: offered.token.clone(),
                proof: completion(&offered.token),
            },
            now,
        )
        .unwrap();
    assert!(
        first
            .ledger
            .view(&offered.token.key)
            .unwrap()
            .unwrap()
            .completion
            .is_some()
    );
    assert!(
        !socket
            .requests()
            .iter()
            .any(|r| matches!(r, Request::Ack { .. }))
    );
    drop(first);
    drop(socket);
    drop(engine);

    // Both Stores are opened again just after their holders dropped.
    let engine = Arc::new(Mutex::new(
        Engine::open(
            crate::reminders::transport::when_free(|| Store::open(&schedule_path)).unwrap(),
            "Etc/UTC",
            &sample(1000),
            OTHER_STORE,
        )
        .unwrap(),
    ));
    let shared = engine.clone();
    let socket = Socket::new(move |request| Some(answer(&mut shared.lock().unwrap(), request)));
    let mut restored = Consumer::new(
        DeliveryLedger::open(
            crate::reminders::transport::when_free(|| Store::open(&ledger_path)).unwrap(),
            OTHER_STORE,
        )
        .unwrap(),
        "consumer-two".into(),
    );
    let now = monotonic_us();
    let report = due(&engine.lock().unwrap(), now);
    restored.tick(&socket.path(), Some(&report), now).unwrap();
    no_offer(&restored);
    assert!(matches!(
        restored
            .ledger
            .view(&offered.token.key)
            .unwrap()
            .unwrap()
            .resolution,
        Some(Resolution::AckConfirmed { .. })
    ));
    assert_eq!(engine.lock().unwrap().counts(&sample(1000)), (0, 0, 0));
    assert_eq!(
        socket
            .requests()
            .iter()
            .filter(|r| matches!(r, Request::Ack { .. }))
            .count(),
        1
    );
}

/// Given up with nothing proven, an attempt stays pending and this voice
/// does not offer it again; a voice started after it does (the first
/// builder, 2026-10-16), since that voice's claim proves the old one gone.
#[test]
fn abandon_keeps_pending_and_only_a_later_voice_offers_it_again() {
    let (engine, socket) = served(1);
    let disk = Disk::default();
    let mut first = consumer(disk.clone());
    let offered = begin(&mut first, &engine, &socket);
    first
        .command(
            ToWorker::Abandon {
                token: offered.token.clone(),
            },
            lamp_ipc::monotonic_us(),
        )
        .unwrap();
    assert!(
        matches!(first.outgoing.pop_front(), Some(FromWorker::Settled { token }) if token == offered.token)
    );
    // The same voice holds it.
    first.next_offer_us = 0;
    let now = lamp_ipc::monotonic_us();
    let report = due(&engine.lock().unwrap(), now);
    first.tick(&socket.path(), Some(&report), now).unwrap();
    no_offer(&first);
    drop(first);
    let mut restored = Consumer::new(
        DeliveryLedger::open(disk, OTHER_STORE).unwrap(),
        "consumer-two".into(),
    );
    // Opening the ledger alone proves nothing: still held.
    let now = lamp_ipc::monotonic_us();
    let report = due(&engine.lock().unwrap(), now);
    restored.tick(&socket.path(), Some(&report), now).unwrap();
    no_offer(&restored);
    assert_eq!(engine.lock().unwrap().counts(&sample(1000)), (1, 1, 0));
    // Holding the claim, as `run` does, it is offered again as a new attempt.
    restored.retire_earlier_voices().unwrap();
    let again = begin(&mut restored, &engine, &socket);
    assert_eq!(again.token.key, offered.token.key);
    assert!(again.token.attempt > offered.token.attempt);
    assert!(
        !socket
            .requests()
            .iter()
            .any(|r| matches!(r, Request::Ack { .. }))
    );
}

/// The first builder's test for lamp-4ace's ids 12 and 26 (2026-10-16):
/// prepared, the voice restarts, offered again, said once, acknowledged.
#[test]
fn prepared_then_a_restart_is_offered_again_said_once_and_acknowledged() {
    let (engine, socket) = served(1);
    let disk = Disk::default();
    let mut first = consumer(disk.clone());
    let offered = begin(&mut first, &engine, &socket);
    // The voice restarts with the attempt prepared and nothing after.
    drop(first);
    let mut restored = Consumer::new(
        DeliveryLedger::open(disk.clone(), OTHER_STORE).unwrap(),
        "consumer-two".into(),
    );
    restored.retire_earlier_voices().unwrap();
    let again = begin(&mut restored, &engine, &socket);
    assert_eq!(again.token.key, offered.token.key);
    restored
        .command(
            ToWorker::Complete {
                token: again.token.clone(),
                proof: completion(&again.token),
            },
            lamp_ipc::monotonic_us(),
        )
        .unwrap();
    restored.outgoing.clear();
    for _ in 0..3 {
        let now = lamp_ipc::monotonic_us();
        let report = due(&engine.lock().unwrap(), now);
        restored.tick(&socket.path(), Some(&report), now).unwrap();
        no_offer(&restored);
    }
    assert_eq!(engine.lock().unwrap().counts(&sample(1000)), (0, 0, 0));
    assert_eq!(
        socket
            .requests()
            .iter()
            .filter(|r| matches!(r, Request::Ack { .. }))
            .count(),
        1
    );
    let lines = &restored.journal.lines;
    assert_eq!(
        lines[0],
        "reminder id 1 delivery 1: prepared by an earlier voice and never said: to be offered again"
    );
    assert!(
        lines
            .last()
            .unwrap()
            .starts_with("reminder id 1 delivery 1: resolved (AckConfirmed"),
        "{lines:?}"
    );
    // A voice started after that has nothing to offer again.
    let mut later = Consumer::new(
        DeliveryLedger::open(disk, OTHER_STORE).unwrap(),
        "consumer-three".into(),
    );
    later.retire_earlier_voices().unwrap();
    assert!(later.retry.is_empty());
}

#[test]
fn only_exact_qualified_completion_can_change_the_durable_prepared_record() {
    let (engine, socket) = served(1);
    let disk = Disk::default();
    let mut consumer = consumer(disk.clone());
    let offered = begin(&mut consumer, &engine, &socket);
    let before = disk.load().unwrap();
    let mut wrong = offered.token.clone();
    wrong.attempt += 1;
    assert!(
        consumer
            .command(ToWorker::Abandon { token: wrong }, lamp_ipc::monotonic_us())
            .is_err()
    );
    let mut proof = completion(&offered.token);
    proof.final_frame = 0;
    assert!(
        consumer
            .command(
                ToWorker::Complete {
                    token: offered.token.clone(),
                    proof
                },
                lamp_ipc::monotonic_us()
            )
            .is_err()
    );
    let mut proof = completion(&offered.token);
    proof.correlation.sequence += 1;
    assert!(
        consumer
            .command(
                ToWorker::Complete {
                    token: offered.token.clone(),
                    proof
                },
                lamp_ipc::monotonic_us()
            )
            .is_err()
    );
    assert_eq!(disk.load().unwrap(), before);
    assert_eq!(consumer.current.as_ref().unwrap().token, offered.token);
}

#[test]
fn qualified_pre_audio_retirement_retries_once_with_a_new_durable_correlation() {
    let (engine, socket) = served(1);
    let mut consumer = consumer(Disk::default());
    let first = begin(&mut consumer, &engine, &socket);
    let now = lamp_ipc::monotonic_us();
    let proof = QualifiedRetirement {
        correlation: first.token.correlation.clone(),
        retired_by_session: SESSION.into(),
        basis: RetirementBasis::OwnerFenced {
            owner_boot: [7; 16],
            turn: 1,
            microphone_generation: 1,
        },
    };
    consumer
        .command(
            ToWorker::Retire {
                token: first.token.clone(),
                proof,
            },
            now,
        )
        .unwrap();
    assert_eq!(consumer.next_offer_us, now + PACE_US);
    consumer.outgoing.clear();
    let report = due(&engine.lock().unwrap(), now);
    consumer.tick(&socket.path(), Some(&report), now).unwrap();
    no_offer(&consumer);
    // Exercise the retry boundary directly; the separate tick above proves
    // the pacing gate, without spending five host seconds on a fixture wait.
    let now = lamp_ipc::monotonic_us();
    let report = due(&engine.lock().unwrap(), now);
    consumer
        .offer(
            &socket.path(),
            &report,
            &key(&report),
            now,
            Instant::now() + REQUEST_BUDGET,
        )
        .unwrap();
    let next = take_offer(&mut consumer);
    assert!(next.token.attempt > first.token.attempt);
    assert!(next.token.correlation.sequence > first.token.correlation.sequence);
    assert_eq!(next.token.key, first.token.key);
    assert!(
        consumer
            .command(
                ToWorker::Complete {
                    token: first.token.clone(),
                    proof: completion(&first.token)
                },
                now
            )
            .is_err()
    );
    assert!(
        !socket
            .requests()
            .iter()
            .any(|r| matches!(r, Request::Ack { .. }))
    );
}

#[test]
fn interrupted_owner_retry_keeps_old_proof_out_of_the_new_attempt_and_ack() {
    let (engine, socket) = served(1);
    let mut consumer = consumer(Disk::default());
    let first = begin(&mut consumer, &engine, &socket);
    let now = lamp_ipc::monotonic_us();
    consumer
        .command(
            ToWorker::Retire {
                token: first.token.clone(),
                proof: QualifiedRetirement {
                    correlation: first.token.correlation.clone(),
                    retired_by_session: SESSION.into(),
                    basis: RetirementBasis::OwnerFenced {
                        owner_boot: [7; 16],
                        turn: 1,
                        microphone_generation: 1,
                    },
                },
            },
            now,
        )
        .unwrap();
    assert_eq!(consumer.next_offer_us, now + PACE_US);
    assert!(
        consumer
            .journal
            .lines
            .last()
            .unwrap()
            .contains("this attempt can no longer speak")
    );
    consumer.outgoing.clear();
    let report = due(&engine.lock().unwrap(), now);
    consumer.tick(&socket.path(), Some(&report), now).unwrap();
    no_offer(&consumer);
    assert!(
        consumer
            .ledger
            .view(&first.token.key)
            .unwrap()
            .unwrap()
            .completion
            .is_none()
    );
    assert!(
        socket
            .requests()
            .iter()
            .all(|request| !matches!(request, Request::Ack { .. }))
    );
    // The pacing boundary above is tested without a five-second host sleep.
    // The next offer still reads the real current Schedule item and commits
    // the replacement token before any sound can be admitted.
    consumer.next_offer_us = 0;
    let next = begin(&mut consumer, &engine, &socket);
    assert_eq!(next.token.key, first.token.key);
    assert!(next.token.attempt > first.token.attempt);
    assert_ne!(next.token.correlation, first.token.correlation);
    assert!(
        consumer
            .command(
                ToWorker::Complete {
                    token: first.token.clone(),
                    proof: completion(&first.token),
                },
                now
            )
            .is_err()
    );
    assert_eq!(consumer.current.as_ref().unwrap().token, next.token);
    assert!(
        socket
            .requests()
            .iter()
            .all(|request| !matches!(request, Request::Ack { .. }))
    );
    consumer
        .command(
            ToWorker::Complete {
                token: next.token.clone(),
                proof: completion(&next.token),
            },
            now,
        )
        .unwrap();
    consumer.outgoing.clear();
    let now = lamp_ipc::monotonic_us();
    let report = due(&engine.lock().unwrap(), now);
    consumer.tick(&socket.path(), Some(&report), now).unwrap();
    assert_eq!(engine.lock().unwrap().counts(&sample(1000)), (0, 0, 0));
    assert_eq!(
        socket
            .requests()
            .iter()
            .filter(|request| matches!(request, Request::Ack { .. }))
            .count(),
        1
    );
    assert!(matches!(
        consumer
            .ledger
            .view(&next.token.key)
            .unwrap()
            .unwrap()
            .resolution,
        Some(Resolution::AckConfirmed { .. })
    ));
}

fn complete_current(
    consumer: &mut Consumer<Disk>,
    engine: &Arc<Mutex<Engine<Disk>>>,
    socket: &Socket,
) -> PreparedToken {
    let offered = begin(consumer, engine, socket);
    consumer
        .command(
            ToWorker::Complete {
                token: offered.token.clone(),
                proof: completion(&offered.token),
            },
            lamp_ipc::monotonic_us(),
        )
        .unwrap();
    consumer.outgoing.clear();
    offered.token
}

#[test]
fn lost_committed_ack_is_recovered_after_restart_without_another_offer() {
    let engine = Arc::new(Mutex::new(prepared_engine(Disk::default(), 1)));
    let shared = engine.clone();
    let socket = Socket::new(move |request| {
        let value = answer(&mut shared.lock().unwrap(), request);
        (!matches!(request, Request::Ack { .. })).then_some(value)
    });
    let disk = Disk::default();
    let mut first = consumer(disk.clone());
    let token = complete_current(&mut first, &engine, &socket);
    first
        .recover_one(&socket.path(), Instant::now() + REQUEST_BUDGET)
        .unwrap();
    assert!(
        first
            .ledger
            .view(&token.key)
            .unwrap()
            .unwrap()
            .ack
            .is_some()
    );
    assert!(
        first
            .ledger
            .view(&token.key)
            .unwrap()
            .unwrap()
            .resolution
            .is_none()
    );
    drop(first);
    let mut restored = Consumer::new(
        DeliveryLedger::open(disk, OTHER_STORE).unwrap(),
        "consumer-two".into(),
    );
    restored
        .recover_one(&socket.path(), Instant::now() + REQUEST_BUDGET)
        .unwrap();
    no_offer(&restored);
    assert!(matches!(
        restored
            .ledger
            .view(&token.key)
            .unwrap()
            .unwrap()
            .resolution,
        Some(Resolution::AckConfirmed { .. })
    ));
    let acks: Vec<_> = socket
        .requests()
        .into_iter()
        .filter(|r| matches!(r, Request::Ack { .. }))
        .collect();
    assert_eq!(acks.len(), 2);
    assert_eq!(acks[0], acks[1]);
    assert_eq!(engine.lock().unwrap().counts(&sample(1000)), (0, 0, 0));
}

#[test]
fn evicted_ack_receipt_resolves_by_current_absence_without_claiming_its_cause() {
    let engine = Arc::new(Mutex::new(prepared_engine(Disk::default(), 1)));
    let shared = engine.clone();
    let socket = Socket::new(move |request| {
        let value = answer(&mut shared.lock().unwrap(), request);
        (!matches!(request, Request::Ack { .. })).then_some(value)
    });
    let mut consumer = consumer(Disk::default());
    let token = complete_current(&mut consumer, &engine, &socket);
    consumer
        .recover_one(&socket.path(), Instant::now() + REQUEST_BUDGET)
        .unwrap();
    {
        let mut engine = engine.lock().unwrap();
        let request = Request::Set {
            person: None,
            guard: guard(&engine, "evict-ack-receipt"),
            text: "Later".into(),
            when: When::After { seconds: 60 },
        };
        engine.answer(request, &sample(1000)).unwrap();
    }
    consumer
        .recover_one(&socket.path(), Instant::now() + REQUEST_BUDGET)
        .unwrap();
    assert!(matches!(
        consumer
            .ledger
            .view(&token.key)
            .unwrap()
            .unwrap()
            .resolution,
        Some(Resolution::Gone { .. })
    ));
    no_offer(&consumer);
    assert!(socket.requests().contains(&Request::List {
        after_id: 0,
        limit: 1,
        pending_only: false
    }));
}

#[test]
fn uncommitted_lost_ack_at_the_same_revision_keeps_the_exact_attempt_and_recovers() {
    let engine = Arc::new(Mutex::new(prepared_engine(Disk::default(), 1)));
    let allow_ack = Arc::new(AtomicBool::new(false));
    let shared = engine.clone();
    let allowed = allow_ack.clone();
    let socket = Socket::new(move |request| {
        if matches!(request, Request::Ack { .. }) && !allowed.load(Ordering::Acquire) {
            return None;
        }
        Some(answer(&mut shared.lock().unwrap(), request))
    });
    let mut consumer = consumer(Disk::default());
    let token = complete_current(&mut consumer, &engine, &socket);
    consumer
        .recover_one(&socket.path(), Instant::now() + REQUEST_BUDGET)
        .unwrap();
    let original = consumer
        .ledger
        .view(&token.key)
        .unwrap()
        .unwrap()
        .ack
        .unwrap();
    consumer
        .recover_one(&socket.path(), Instant::now() + REQUEST_BUDGET)
        .unwrap();
    assert_eq!(
        consumer
            .ledger
            .view(&token.key)
            .unwrap()
            .unwrap()
            .ack
            .as_ref(),
        Some(&original)
    );
    assert!(!consumer.ledger.failed());
    no_offer(&consumer);
    allow_ack.store(true, Ordering::Release);
    consumer
        .recover_one(&socket.path(), Instant::now() + REQUEST_BUDGET)
        .unwrap();
    assert!(matches!(
        consumer
            .ledger
            .view(&token.key)
            .unwrap()
            .unwrap()
            .resolution,
        Some(Resolution::AckConfirmed { .. })
    ));
    assert!(
        socket
            .requests()
            .iter()
            .filter(|r| matches!(r, Request::Ack { .. }))
            .all(|r| *r == original.request)
    );
}

#[test]
fn old_uncertain_item_does_not_starve_the_next_pending_occurrence() {
    let (engine, socket) = served(2);
    let disk = Disk::default();
    let mut first = consumer(disk.clone());
    let abandoned = begin(&mut first, &engine, &socket);
    assert_eq!(abandoned.token.key.id, 1);
    drop(first); // Crash after durable prepare, with no qualified retirement.
    let mut restored = Consumer::new(
        DeliveryLedger::open(disk, OTHER_STORE).unwrap(),
        "consumer-two".into(),
    );
    let now = lamp_ipc::monotonic_us();
    let report = due(&engine.lock().unwrap(), now);
    restored.tick(&socket.path(), Some(&report), now).unwrap();
    let next = take_offer(&mut restored);
    assert_eq!(next.token.key.id, 2);
    assert!(socket.requests().contains(&Request::List {
        after_id: 0,
        limit: 8,
        pending_only: true
    }));
    assert!(
        !socket
            .requests()
            .iter()
            .any(|r| matches!(r, Request::Ack { .. }))
    );
}

#[test]
fn full_ledger_refuses_new_admission_without_evicting_any_uncertain_delivery() {
    let mut engine = prepared_engine(Disk::default(), 1);
    let original = answer(&mut engine, &Request::Get { id: 1 });
    let disk = Disk::default();
    let mut consumer = consumer(disk.clone());
    for id in 1..=MAX_ENTRIES as u64 {
        let mut reply = original.clone();
        reply["item"]["id"] = json!(id);
        let pending = PendingEvidence::from_get(&Request::Get { id }, &reply).unwrap();
        let correlation = consumer.correlation().unwrap();
        consumer.ledger.prepare(&pending, correlation).unwrap();
    }
    let before = disk.load().unwrap();
    let mut next = original;
    next["item"]["id"] = json!(MAX_ENTRIES as u64 + 1);
    let socket = Socket::new(move |_| Some(next.clone()));
    let now = lamp_ipc::monotonic_us();
    let mut report = due(&engine, now);
    report.oldest.as_mut().unwrap().id = MAX_ENTRIES as u64 + 1;
    assert!(
        consumer
            .offer(
                &socket.path(),
                &report,
                &key(&report),
                now,
                Instant::now() + REQUEST_BUDGET
            )
            .is_err()
    );
    no_offer(&consumer);
    assert_eq!(consumer.ledger.entries().unwrap().len(), MAX_ENTRIES);
    assert_eq!(disk.load().unwrap(), before);
}

#[test]
fn storage_failure_before_prepare_withholds_the_offer_and_latches_failure() {
    let (engine, socket) = served(1);
    let disk = Disk::default();
    let mut consumer = consumer(disk.clone());
    disk.0.lock().unwrap().fail = true;
    let now = lamp_ipc::monotonic_us();
    let report = due(&engine.lock().unwrap(), now);
    consumer.tick(&socket.path(), Some(&report), now).unwrap();
    assert!(consumer.ledger.failed());
    assert!(consumer.current.is_none());
    no_offer(&consumer);
}

#[test]
fn slow_get_withdraws_current_authority_without_refreshing_its_observation() {
    let engine = Arc::new(Mutex::new(prepared_engine(Disk::default(), 1)));
    let slow = Arc::new(AtomicBool::new(false));
    let shared = engine.clone();
    let delay = slow.clone();
    let socket = Socket::new(move |request| {
        if delay.load(Ordering::Acquire) {
            thread::sleep(REQUEST_BUDGET + Duration::from_millis(100));
        }
        Some(answer(&mut shared.lock().unwrap(), request))
    });
    let mut consumer = consumer(Disk::default());
    let first = begin(&mut consumer, &engine, &socket);
    slow.store(true, Ordering::Release);
    let started = Instant::now();
    let now = lamp_ipc::monotonic_us();
    let report = due(&engine.lock().unwrap(), now);
    consumer.tick(&socket.path(), Some(&report), now).unwrap();
    // This is a generous fixture deadlock bound, not an asserted host or lamp
    // latency target. The 250 ms socket budget is authored production policy.
    assert!(started.elapsed() < Duration::from_secs(5));
    no_offer(&consumer);
    assert_eq!(
        consumer.current.as_ref().unwrap().observed_at_us,
        first.observed_at_us
    );
    assert!(
        matches!(consumer.outgoing.pop_front(), Some(FromWorker::Withdraw { token }) if token == first.token)
    );
    assert_eq!(socket.requests().len(), 2);
}

#[test]
fn delayed_durable_prepare_cannot_publish_an_already_expired_offer() {
    let (engine, socket) = served(1);
    let disk = Disk::default();
    let mut consumer = consumer(disk.clone());
    disk.0.lock().unwrap().delay = Duration::from_micros(LEASE_US + 50_000);
    let now = lamp_ipc::monotonic_us();
    let report = due(&engine.lock().unwrap(), now);
    consumer.tick(&socket.path(), Some(&report), now).unwrap();
    no_offer(&consumer);
    assert_eq!(consumer.ledger.entries().unwrap().len(), 1);
    let current = consumer.current.as_ref().unwrap();
    assert!(!current.fresh(lamp_ipc::monotonic_us()));
    assert_eq!(current.observed_at_us, now);
    assert!(matches!(
        consumer.outgoing.front(),
        Some(FromWorker::Withdraw { .. })
    ));
}

#[test]
fn outgoing_queue_is_bounded_and_replacing_an_offer_never_discards_settlement() {
    let (engine, socket) = served(1);
    let mut consumer = consumer(Disk::default());
    let offer = begin(&mut consumer, &engine, &socket);
    consumer
        .push(FromWorker::Settled {
            token: offer.token.clone(),
        })
        .unwrap();
    consumer
        .push(FromWorker::Offer {
            offer: offer.clone(),
        })
        .unwrap();
    consumer.push(FromWorker::Offer { offer }).unwrap();
    assert_eq!(consumer.outgoing.len(), 2);
    assert!(matches!(
        consumer.outgoing.front(),
        Some(FromWorker::Settled { .. })
    ));
    consumer.push(FromWorker::Idle {}).unwrap();
    consumer.push(FromWorker::Idle {}).unwrap();
    assert!(consumer.push(FromWorker::Idle {}).is_err());
    assert_eq!(consumer.outgoing.len(), QUEUE);
}

/// The first builder, 2026-10-16: due reminders on lamp-4ace were never
/// said, and nothing said where they stopped. What became of each due item
/// is one journal line when it changes, never one per tick.
#[test]
fn the_journal_says_what_became_of_each_due_item_once_per_change() {
    let (engine, socket) = served(1);
    let mut said = consumer(Disk::default());
    complete_current(&mut said, &engine, &socket);
    for _ in 0..3 {
        let now = lamp_ipc::monotonic_us();
        let report = due(&engine.lock().unwrap(), now);
        said.tick(&socket.path(), Some(&report), now).unwrap();
    }
    let lines = &said.journal.lines;
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert_eq!(
        lines[0],
        "reminder id 1 delivery 1: due, offered to the voice (attempt 1)"
    );
    assert_eq!(
        lines[1],
        "reminder id 1 delivery 1: said; its completion is kept, to be acknowledged"
    );
    assert!(
        lines[2].starts_with("reminder id 1 delivery 1: resolved (AckConfirmed"),
        "{lines:?}"
    );

    // Given up with nothing proven: held, said once however often it ticks.
    let (engine, socket) = served(1);
    let mut held = consumer(Disk::default());
    let offered = begin(&mut held, &engine, &socket);
    held.command(
        ToWorker::Abandon {
            token: offered.token,
        },
        lamp_ipc::monotonic_us(),
    )
    .unwrap();
    for _ in 0..3 {
        // Past the pacing after a settlement, so each tick reaches `held()`
        // (the reviewer on #1081: without this the check could not fail).
        held.next_offer_us = 0;
        let now = lamp_ipc::monotonic_us();
        let report = due(&engine.lock().unwrap(), now);
        held.tick(&socket.path(), Some(&report), now).unwrap();
    }
    assert_eq!(held.journal.lines.len(), 2, "{:?}", held.journal.lines);
    assert_eq!(
        held.journal.lines[1],
        format!("reminder id 1 delivery 1: {HELD}")
    );

    // At start the first good read is said, so the journal shows the
    // consumer is reading due work at all (lamp-4ace, 2026-10-16).
    let mut fresh = Journal::default();
    fresh.due(None);
    fresh.due(None);
    assert_eq!(fresh.lines, ["due reminders available"]);
    // Due work that cannot be read is said once, and its return once.
    let mut journal = Journal::default();
    for _ in 0..3 {
        journal.due(Some("Schedule due is missing".into()));
    }
    journal.due(None);
    journal.due(None);
    assert_eq!(
        journal.lines,
        [
            "due reminders unavailable: Schedule due is missing",
            "due reminders available"
        ]
    );
}
