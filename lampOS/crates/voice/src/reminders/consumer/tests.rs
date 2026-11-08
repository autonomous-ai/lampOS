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
fn abandoned_twice_keeps_pending_and_only_a_later_voice_offers_it_again() {
    let (engine, socket) = served(1);
    let disk = Disk::default();
    let mut first = consumer(disk.clone());
    let once = begin(&mut first, &engine, &socket);
    abandon(&mut first, &once.token);
    first.next_offer_us = 0;
    let offered = begin(&mut first, &engine, &socket);
    abandon(&mut first, &offered.token);
    // Given up twice, the same voice holds it.
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

/// The voice gives up `token`'s attempt with nothing proven: its reply
/// ended (cut by talk, say) with no completion and no fenced retirement.
fn abandon(worker: &mut Consumer<Disk>, token: &PreparedToken) -> u64 {
    abandon_as(worker, token, false)
}
/// As [`abandon`], saying whether any of its audio came.
fn abandon_as(worker: &mut Consumer<Disk>, token: &PreparedToken, no_audio: bool) -> u64 {
    abandon_with(worker, token, no_audio, false)
}
/// As [`abandon_as`], saying too whether the person stopped it over its ring.
fn abandon_with(
    worker: &mut Consumer<Disk>,
    token: &PreparedToken,
    no_audio: bool,
    stopped: bool,
) -> u64 {
    let now = lamp_ipc::monotonic_us();
    worker
        .command(
            ToWorker::Abandon {
                token: token.clone(),
                no_audio,
                stopped,
            },
            now,
        )
        .unwrap();
    assert!(
        matches!(worker.outgoing.pop_front(), Some(FromWorker::Settled { token: settled }) if &settled == token)
    );
    now
}

/// Ticks past the pacing `times` over, offering nothing.
fn ticks_offer_nothing(
    worker: &mut Consumer<Disk>,
    engine: &Arc<Mutex<Engine<Disk>>>,
    socket: &Socket,
    times: usize,
) {
    for _ in 0..times {
        worker.next_offer_us = 0;
        let now = lamp_ipc::monotonic_us();
        let report = due(&engine.lock().unwrap(), now);
        worker.tick(&socket.path(), Some(&report), now).unwrap();
        no_offer(worker);
        worker.outgoing.clear();
    }
}

/// #5219, stable 9980 on lamp-52e6: a one-minute timer came due while
/// someone was talking, its attempt was cut, and it was held for good and
/// never rang. An attempt given up with nothing proven is offered once
/// more, after the pacing, under a new attempt; given up a second time it
/// is held, so it is said twice at most.
/// A fenced cut-in of `token`'s attempt, some of its audio played or not.
fn cut_in(
    worker: &mut Consumer<Disk>,
    token: &PreparedToken,
    heard_some: bool,
    light_cut: bool,
) -> u64 {
    let now = lamp_ipc::monotonic_us();
    worker
        .command(
            ToWorker::Retire {
                token: token.clone(),
                proof: QualifiedRetirement {
                    correlation: token.correlation.clone(),
                    retired_by_session: SESSION.into(),
                    basis: RetirementBasis::OwnerFenced {
                        owner_boot: [7; 16],
                        turn: 1,
                        microphone_generation: 1,
                    },
                },
                heard_some,
                light_cut,
            },
            now,
        )
        .unwrap();
    worker.outgoing.clear();
    now
}

/// 06c1, 5 Nov 17:04 (build 10343): "Time to drink some water." (req 256)
/// was cut by a wordless cut-in (req 257) and offered again as new, and
/// said again (req 258). A cut-in after some of its audio offers it once
/// more, said as a repeat; a second such cut holds it. One cut before any
/// audio is offered again as before, and not as a repeat.
#[test]
fn a_cut_in_after_some_audio_offers_it_once_more_as_a_repeat_then_holds_it() {
    let (engine, socket) = served(1);
    let mut worker = consumer(Disk::default());
    let first = begin(&mut worker, &engine, &socket);
    assert!(!first.again);
    // Cut before any of it played: offered again, not as a repeat.
    cut_in(&mut worker, &first.token, false, false);
    worker.next_offer_us = 0;
    let second = begin(&mut worker, &engine, &socket);
    assert!(!second.again, "nothing of it was heard");
    // Cut after some of it played: once more, as a repeat.
    cut_in(&mut worker, &second.token, true, false);
    assert_eq!(
        worker.journal.lines.last().unwrap(),
        &format!("reminder id 1 delivery 1: {CUT_ONCE_MORE}")
    );
    worker.next_offer_us = 0;
    let third = begin(&mut worker, &engine, &socket);
    assert_eq!(third.token.key, first.token.key);
    assert!(third.again, "the re-offer is said as a repeat");
    assert!(
        worker
            .journal
            .lines
            .iter()
            .any(|line| line.ends_with(", said as a repeat)")),
        "{:?}",
        worker.journal.lines
    );
    // Cut again after some of it: held, never a third time.
    cut_in(&mut worker, &third.token, true, false);
    assert_eq!(
        worker.journal.lines.last().unwrap(),
        &format!("reminder id 1 delivery 1: {HELD}")
    );
    ticks_offer_nothing(&mut worker, &engine, &socket, 4);
    assert!(
        socket
            .requests()
            .iter()
            .all(|request| !matches!(request, Request::Ack { .. }))
    );
}

/// Heron's P2 on #5509: two cuts a few frames in each (the lamp's own
/// echo, a cough's tail) never hold it: it is still offered a third time,
/// not as a repeat; light cuts are capped at `LIGHT_CUT_RETRIES`, then held.
/// A cut with a real share heard after them still counts toward the hold.
#[test]
fn light_cuts_never_count_toward_the_hold_and_are_capped() {
    let (engine, socket) = served(1);
    let mut worker = consumer(Disk::default());
    let mut offer = begin(&mut worker, &engine, &socket);
    for n in 1..=LIGHT_CUT_RETRIES {
        cut_in(&mut worker, &offer.token, false, true);
        worker.next_offer_us = 0;
        offer = begin(&mut worker, &engine, &socket);
        assert!(
            !offer.again,
            "light cut {n}: nothing of it was heard to repeat"
        );
    }
    // A real share heard: once more, as a repeat.
    cut_in(&mut worker, &offer.token, true, false);
    worker.next_offer_us = 0;
    let repeat = begin(&mut worker, &engine, &socket);
    assert!(repeat.again);
    // A light cut now: past the cap, held.
    cut_in(&mut worker, &repeat.token, false, true);
    assert_eq!(
        worker.journal.lines.last().unwrap(),
        &format!("reminder id 1 delivery 1: {LIGHT_CUT_HELD}")
    );
    ticks_offer_nothing(&mut worker, &engine, &socket, 4);
}

/// The same run's line as the coordinator now settles it: cut after most
/// of it played, it is completed, acknowledged and never offered again.
#[test]
fn a_reminder_completed_at_its_cut_is_acknowledged_and_not_offered_again() {
    let (engine, socket) = served(1);
    let mut worker = consumer(Disk::default());
    let first = begin(&mut worker, &engine, &socket);
    let now = lamp_ipc::monotonic_us();
    worker
        .command(
            ToWorker::Complete {
                token: first.token.clone(),
                proof: completion(&first.token),
            },
            now,
        )
        .unwrap();
    assert!(
        worker
            .ledger
            .view(&first.token.key)
            .unwrap()
            .unwrap()
            .completion
            .is_some()
    );
    assert!(!worker.retry.contains_key(&first.token.key));
    assert!(
        !worker
            .journal
            .lines
            .iter()
            .any(|line| line.contains("to be offered again"))
    );
}

#[test]
fn abandoned_once_is_offered_again_after_the_pace_and_twice_is_held() {
    let (engine, socket) = served(1);
    let mut worker = consumer(Disk::default());
    let first = begin(&mut worker, &engine, &socket);
    let now = abandon(&mut worker, &first.token);
    assert_eq!(worker.next_offer_us, now + PACE_US);
    assert_eq!(
        worker.journal.lines.last().unwrap(),
        &format!("reminder id 1 delivery 1: {ONCE_MORE}")
    );
    // Not before the pacing.
    let report = due(&engine.lock().unwrap(), now);
    worker.tick(&socket.path(), Some(&report), now).unwrap();
    no_offer(&worker);
    // After it, offered again under a new attempt and correlation.
    worker.next_offer_us = 0;
    let second = begin(&mut worker, &engine, &socket);
    assert_eq!(second.token.key, first.token.key);
    assert!(second.token.attempt > first.token.attempt);
    assert_ne!(second.token.correlation, first.token.correlation);
    // The old attempt can settle nothing now.
    assert!(
        worker
            .command(
                ToWorker::Complete {
                    token: first.token.clone(),
                    proof: completion(&first.token),
                },
                now
            )
            .is_err()
    );
    // Given up again: held for good.
    abandon(&mut worker, &second.token);
    assert_eq!(
        worker.journal.lines.last().unwrap(),
        &format!("reminder id 1 delivery 1: {HELD}")
    );
    ticks_offer_nothing(&mut worker, &engine, &socket, 4);
    assert_eq!(engine.lock().unwrap().counts(&sample(1000)).0, 1);
    assert!(
        !socket
            .requests()
            .iter()
            .any(|r| matches!(r, Request::Ack { .. }))
    );
}

/// Seventh's P3 on #5258: "stop" said over a timer's ring, or the head or
/// the button, after some of it played, is the person saying they heard it.
/// It is held at once, never rung again 5 s later; with nothing played, a
/// stop is no proof it was heard, and it is offered again as before.
#[test]
fn stopped_over_its_ring_it_is_held_at_once_and_never_rung_again() {
    let (engine, socket) = served(1);
    let mut worker = consumer(Disk::default());
    let first = begin(&mut worker, &engine, &socket);
    abandon_with(&mut worker, &first.token, false, true);
    assert_eq!(
        worker.journal.lines.last().unwrap(),
        &format!("reminder id 1 delivery 1: {STOPPED_HELD}")
    );
    ticks_offer_nothing(&mut worker, &engine, &socket, 4);
    assert!(
        !socket
            .requests()
            .iter()
            .any(|r| matches!(r, Request::Ack { .. }))
    );

    let (engine, socket) = served(1);
    let mut worker = consumer(Disk::default());
    let first = begin(&mut worker, &engine, &socket);
    let now = abandon_with(&mut worker, &first.token, true, true);
    assert_eq!(worker.next_offer_us, now + PACE_US);
    assert_eq!(
        worker.journal.lines.last().unwrap(),
        &format!("reminder id 1 delivery 1: {}", no_audio_again(1))
    );
}

/// The Reviewer-lead's P2 on #5351: on a product lamp, "Stop." said over a
/// ring is a cut-in, which retires the attempt (`OwnerFenced`) for a retry
/// 5 s later; the second route then answers the line as a stop. That stop
/// takes the ring off the retry and holds it. A stop for another attempt,
/// or none, leaves the retry as it was.
#[test]
fn a_stop_for_the_ring_a_cut_in_retired_takes_it_off_the_retry() {
    let fenced = |token: &PreparedToken| QualifiedRetirement {
        correlation: token.correlation.clone(),
        retired_by_session: SESSION.into(),
        basis: RetirementBasis::OwnerFenced {
            owner_boot: [7; 16],
            turn: 1,
            microphone_generation: 1,
        },
    };
    for stop_for_it in [true, false] {
        let (engine, socket) = served(1);
        let mut worker = consumer(Disk::default());
        let first = begin(&mut worker, &engine, &socket);
        let now = lamp_ipc::monotonic_us();
        worker
            .command(
                ToWorker::Retire {
                    token: first.token.clone(),
                    proof: fenced(&first.token),
                    heard_some: false,
                    light_cut: false,
                },
                now,
            )
            .unwrap();
        worker.outgoing.clear();
        let mut other = first.token.clone();
        other.attempt += 1;
        worker
            .command(
                ToWorker::RingStopped {
                    token: if stop_for_it {
                        first.token.clone()
                    } else {
                        other
                    },
                },
                now,
            )
            .unwrap();
        assert!(worker.outgoing.is_empty(), "nothing is settled for it");
        if stop_for_it {
            assert_eq!(
                worker.journal.lines.last().unwrap(),
                &format!("reminder id 1 delivery 1: {STOPPED_HELD}")
            );
            ticks_offer_nothing(&mut worker, &engine, &socket, 4);
        } else {
            worker.next_offer_us = 0;
            let again = begin(&mut worker, &engine, &socket);
            assert_eq!(again.token.key, first.token.key, "offered again as before");
        }
    }
}

/// lamp-9adf, build 10012 (#5219): a timer's ring is a model turn, and one
/// whose chat had not begun in 3 s ended `GaveUp` with none of its audio,
/// then was held for good. An attempt given up before any of its audio came
/// was certainly not heard: it is offered again after the pacing, and does
/// not use up the once more an attempt that played some sound has.
#[test]
fn a_never_voiced_attempt_is_offered_again_without_using_the_once_more() {
    let (engine, socket) = served(1);
    let mut worker = consumer(Disk::default());
    let first = begin(&mut worker, &engine, &socket);
    let now = abandon_as(&mut worker, &first.token, true);
    assert_eq!(worker.next_offer_us, now + PACE_US);
    assert_eq!(
        worker.journal.lines.last().unwrap(),
        &format!("reminder id 1 delivery 1: {}", no_audio_again(1))
    );
    let report = due(&engine.lock().unwrap(), now);
    worker.tick(&socket.path(), Some(&report), now).unwrap();
    no_offer(&worker);
    worker.next_offer_us = 0;
    let second = begin(&mut worker, &engine, &socket);
    assert!(second.token.attempt > first.token.attempt);
    assert_ne!(second.token.correlation, first.token.correlation);
    // Played, then cut: still its one more.
    abandon(&mut worker, &second.token);
    assert_eq!(
        worker.journal.lines.last().unwrap(),
        &format!("reminder id 1 delivery 1: {ONCE_MORE}")
    );
    worker.next_offer_us = 0;
    let third = begin(&mut worker, &engine, &socket);
    assert!(third.token.attempt > second.token.attempt);
    // Played and cut again: held.
    abandon(&mut worker, &third.token);
    assert_eq!(
        worker.journal.lines.last().unwrap(),
        &format!("reminder id 1 delivery 1: {HELD}")
    );
    ticks_offer_nothing(&mut worker, &engine, &socket, 4);
}

/// Given up before any of its audio came, an item is offered again
/// [`NO_AUDIO_RETRIES`] (5) times, each after the pacing; given up so once
/// more, it is held with a line saying why. A played attempt after those
/// still has its one more.
#[test]
fn never_voiced_attempts_are_offered_again_five_times_then_held() {
    let (engine, socket) = served(1);
    let mut worker = consumer(Disk::default());
    let mut offered = begin(&mut worker, &engine, &socket);
    for n in 1..=NO_AUDIO_RETRIES {
        let now = abandon_as(&mut worker, &offered.token, true);
        assert_eq!(worker.next_offer_us, now + PACE_US);
        assert_eq!(
            worker.journal.lines.last().unwrap(),
            &format!("reminder id 1 delivery 1: {}", no_audio_again(n))
        );
        worker.next_offer_us = 0;
        let next = begin(&mut worker, &engine, &socket);
        assert!(next.token.attempt > offered.token.attempt);
        offered = next;
    }
    abandon_as(&mut worker, &offered.token, true);
    assert_eq!(
        worker.journal.lines.last().unwrap(),
        &format!("reminder id 1 delivery 1: {NO_AUDIO_HELD}")
    );
    ticks_offer_nothing(&mut worker, &engine, &socket, 4);
    assert_eq!(engine.lock().unwrap().counts(&sample(1000)).0, 1);

    // Played and cut after its five: one more, then held.
    let (engine, socket) = served(1);
    let mut worker = consumer(Disk::default());
    let mut offered = begin(&mut worker, &engine, &socket);
    for _ in 1..=NO_AUDIO_RETRIES {
        abandon_as(&mut worker, &offered.token, true);
        worker.next_offer_us = 0;
        offered = begin(&mut worker, &engine, &socket);
    }
    abandon(&mut worker, &offered.token);
    worker.next_offer_us = 0;
    let last = begin(&mut worker, &engine, &socket);
    abandon(&mut worker, &last.token);
    assert_eq!(
        worker.journal.lines.last().unwrap(),
        &format!("reminder id 1 delivery 1: {HELD}")
    );
    ticks_offer_nothing(&mut worker, &engine, &socket, 4);
}

/// Four reminders set for three to four hours, so kept as reminders, not
/// timers, and when each is due, in seconds: oldest first, Drink water,
/// Water timer, Send the report, Buy the gift. 29e7's were 13 to 38 hours
/// late, too late to be said now (#5258).
const LATE_FOUR: [(&str, u64); 4] = [
    ("Send the report", 3 * 3_600 + 600),
    ("Buy the gift", 4 * 3_600),
    ("Drink water", 3 * 3_600),
    ("Water timer", 3 * 3_600 + 60),
];
/// When they are served: half an hour to an hour and a half late, said late
/// and not too late (#5258).
const LATE_FOUR_MS: u64 = 4 * 3_600_000 + 1_800_000;

/// Four reminders late, served as one late line, and the sample time they
/// are served at.
fn late_four() -> (Arc<Mutex<Engine<Disk>>>, Socket, u64) {
    const LATE: u64 = LATE_FOUR_MS;
    let mut engine = Engine::open(Disk::default(), "Etc/UTC", &sample(0), STORE_ID).unwrap();
    for (index, (text, seconds)) in LATE_FOUR.into_iter().enumerate() {
        engine
            .answer(
                Request::Set {
                    person: None,
                    guard: guard(&engine, &format!("set-{index}")),
                    text: text.into(),
                    when: When::After { seconds },
                },
                &sample(0),
            )
            .unwrap();
    }
    engine.tick(&sample(LATE)).unwrap();
    let engine = Arc::new(Mutex::new(engine));
    let shared = engine.clone();
    let socket = Socket::new(move |request| {
        Some(
            shared
                .lock()
                .unwrap()
                .answer(request.clone(), &sample(LATE))
                .unwrap_or_else(|e| refusal(e.code, e.message)),
        )
    });
    (engine, socket, LATE)
}

/// Each reminder joined to a late line that was given up is treated as the
/// line's own: offered once more, each under its own new attempt, and held
/// when given up again.
#[test]
fn joined_ones_in_an_abandoned_line_are_offered_once_more_then_held() {
    let (engine, socket, late) = late_four();
    let tick = |worker: &mut Consumer<Disk>| {
        worker.next_offer_us = 0;
        let now = monotonic_us();
        let report = engine
            .lock()
            .unwrap()
            .due(&sample(late), stamp(now, 1), true);
        worker.tick(&socket.path(), Some(&report), now).unwrap();
    };
    let mut worker = consumer(Disk::default());
    tick(&mut worker);
    let first = take_offer(&mut worker);
    assert_eq!(first.also.len(), 3);
    abandon(&mut worker, &first.token);
    for joined in &first.also {
        assert!(
            worker.journal.lines.contains(&format!(
                "reminder id {} delivery {}: {ONCE_MORE}",
                joined.token.key.id, joined.token.key.delivery
            )),
            "{:?}",
            worker.journal.lines
        );
    }
    tick(&mut worker);
    let second = take_offer(&mut worker);
    assert_eq!(second.token.key, first.token.key);
    let keys = |offer: &Offer| {
        offer
            .also
            .iter()
            .map(|joined| joined.token.key.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(keys(&second), keys(&first));
    for (again, before) in second.also.iter().zip(&first.also) {
        assert!(again.token.attempt > before.token.attempt);
        assert_ne!(again.token.correlation, before.token.correlation);
    }
    abandon(&mut worker, &second.token);
    for joined in &second.also {
        assert!(
            worker.journal.lines.contains(&format!(
                "reminder id {} delivery {}: {HELD}",
                joined.token.key.id, joined.token.key.delivery
            )),
            "{:?}",
            worker.journal.lines
        );
    }
    for _ in 0..6 {
        tick(&mut worker);
        no_offer(&worker);
        worker.outgoing.clear();
    }
    assert_eq!(engine.lock().unwrap().counts(&sample(late)).0, 4);
}

/// An item said on its attempt after the cut is acknowledged and never
/// offered again, and one cancelled while it waits to be offered once more
/// is resolved, not offered.
#[test]
fn an_abandoned_one_completed_or_resolved_is_never_offered_again() {
    // Said on the attempt after the cut.
    let (engine, socket) = served(1);
    let mut worker = consumer(Disk::default());
    let first = begin(&mut worker, &engine, &socket);
    abandon(&mut worker, &first.token);
    worker.next_offer_us = 0;
    let second = begin(&mut worker, &engine, &socket);
    worker
        .command(
            ToWorker::Complete {
                token: second.token.clone(),
                proof: completion(&second.token),
            },
            lamp_ipc::monotonic_us(),
        )
        .unwrap();
    worker.outgoing.clear();
    ticks_offer_nothing(&mut worker, &engine, &socket, 4);
    assert_eq!(engine.lock().unwrap().counts(&sample(1000)), (0, 0, 0));
    assert_eq!(
        socket
            .requests()
            .iter()
            .filter(|r| matches!(r, Request::Ack { .. }))
            .count(),
        1
    );
    assert!(worker.retry.is_empty());

    // Cancelled while it waits to be offered once more.
    let (engine, socket) = served(1);
    let mut worker = consumer(Disk::default());
    let first = begin(&mut worker, &engine, &socket);
    abandon(&mut worker, &first.token);
    {
        let mut engine = engine.lock().unwrap();
        let request = Request::Cancel {
            asker: None,
            guard: guard(&engine, "cancel"),
            id: 1,
        };
        engine.answer(request, &sample(1000)).unwrap();
    }
    ticks_offer_nothing(&mut worker, &engine, &socket, 4);
    assert!(
        worker
            .journal
            .lines
            .iter()
            .any(|line| line.starts_with("reminder id 1 delivery 1: resolved (Gone")),
        "{:?}",
        worker.journal.lines
    );
    assert!(worker.retry.is_empty());
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
            .command(
                ToWorker::Abandon {
                    token: wrong,
                    no_audio: false,
                    stopped: false,
                },
                lamp_ipc::monotonic_us()
            )
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
                heard_some: false,
                light_cut: false,
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
                heard_some: false,
                light_cut: false,
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

/// One due item that cannot be offered, oldest or not, never silences the
/// later ones, and each one passed over says why once (#5498).
#[test]
fn an_unofferable_oldest_does_not_silence_later_items_and_each_failure_is_journaled() {
    let engine = Arc::new(Mutex::new(prepared_engine(Disk::default(), 3)));
    let shared = engine.clone();
    let socket = Socket::new(move |request| {
        Some(match request {
            Request::Get { id: 1 | 2 } => refusal("stale_revision", "refused for the test"),
            request => answer(&mut shared.lock().unwrap(), request),
        })
    });
    let mut consumer = consumer(Disk::default());
    let now = lamp_ipc::monotonic_us();
    let report = due(&engine.lock().unwrap(), now);
    assert_eq!(key(&report).id, 1);
    consumer.tick(&socket.path(), Some(&report), now).unwrap();
    assert_eq!(take_offer(&mut consumer).token.key.id, 3);
    // The oldest is asked for once in the tick, not again by the scan.
    let gets = |id| {
        socket
            .requests()
            .iter()
            .filter(|r| **r == Request::Get { id })
            .count()
    };
    assert_eq!((gets(1), gets(2)), (1, 1));
    for id in [1, 2] {
        let said = format!("reminder id {id} delivery ");
        assert_eq!(
            consumer
                .journal
                .lines
                .iter()
                .filter(|line| line.contains(&said) && line.contains("not offered"))
                .count(),
            1,
            "{:?}",
            consumer.journal.lines
        );
    }
}

/// A compacted ledger fenced four revisions ahead of `engine`'s store, under
/// the same `store_id`; with the switch on, armed as `run` arms it.
fn ahead_of(engine: &Engine<Disk>, switch: Option<&str>) -> Consumer<Disk> {
    let ahead = json!({
        "kind": "lamp_schedule_delivery_ledger",
        "schema": 1,
        "ledger_id": LEDGER_ID,
        "revision": 1,
        "next_attempt": 2,
        "schedule": { "store_id": STORE_ID, "revision": engine.revision() + 4 },
        "entries": [],
    });
    let disk = Disk(Arc::new(Mutex::new(Data {
        bytes: Some(serde_json::to_vec(&ahead).unwrap()),
        ..Data::default()
    })));
    let mut ledger = DeliveryLedger::open(disk, LEDGER_ID).unwrap();
    if crate::reminders::refence_on(switch) {
        ledger.refence_at_start();
    }
    Consumer::new(ledger, SESSION.into())
}

/// A ledger whose fence is ahead of the store under the same `store_id`
/// (lamp-52e6, 6 Nov: fence 96, store 92) refused every offer with
/// `stale_revision`, so a timer the voice set was never said. With
/// LAMP_LEDGER_REFENCE on, a voice that starts on such a ledger says a timer
/// due now (#5560).
#[test]
fn a_voice_started_on_a_ledger_ahead_of_the_store_says_a_due_timer() {
    let (engine, socket) = served(1);
    let store = engine.lock().unwrap().revision();
    let mut consumer = ahead_of(&engine.lock().unwrap(), Some("on"));
    assert_eq!(consumer.ledger.schedule_revision(), Some(store + 4));
    consumer.retire_earlier_voices().unwrap();
    let now = lamp_ipc::monotonic_us();
    let report = due(&engine.lock().unwrap(), now);
    consumer.tick(&socket.path(), Some(&report), now).unwrap();
    assert!(
        consumer
            .outgoing
            .iter()
            .any(|m| matches!(m, FromWorker::Offer { .. })),
        "the due timer was not offered: {:?}",
        consumer.journal.lines
    );
    assert_eq!(take_offer(&mut consumer).token.key, key(&report));
    assert_eq!(consumer.ledger.schedule_revision(), Some(store));
}

/// With the switch off, or set to anything but `on`, a ledger ahead of the
/// store refuses as before: nothing offered, `stale_revision` journaled, the
/// fence unmoved.
#[test]
fn with_the_refence_switch_off_a_ledger_ahead_of_the_store_still_refuses() {
    for switch in [None, Some("1"), Some("ON"), Some("")] {
        let (engine, socket) = served(1);
        let store = engine.lock().unwrap().revision();
        let mut consumer = ahead_of(&engine.lock().unwrap(), switch);
        consumer.retire_earlier_voices().unwrap();
        let now = lamp_ipc::monotonic_us();
        let report = due(&engine.lock().unwrap(), now);
        consumer.tick(&socket.path(), Some(&report), now).unwrap();
        no_offer(&consumer);
        assert!(
            consumer
                .journal
                .lines
                .iter()
                .any(|line| line.contains("not offered") && line.contains("stale_revision")),
            "{switch:?}: {:?}",
            consumer.journal.lines
        );
        assert_eq!(consumer.ledger.schedule_revision(), Some(store + 4));
    }
}

/// #5560's fallback: while the ledger refuses every item and can't be
/// re-fenced (here: not armed), the consumer keeps its cannot-ring file, and
/// a `set_timer` is then refused before it reaches the schedule, told as not
/// set, so no "Timer set." line can come of it. Once the ledger takes items
/// (armed at a start with the switch on), the file is gone and sets go on.
#[test]
fn a_ledger_that_cannot_ring_refuses_a_new_timer_and_says_it_is_not_set() {
    let dir = std::env::temp_dir().join(format!("lamp-cannot-ring-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(crate::reminders::CANNOT_RING_FILE);
    let _ = std::fs::remove_file(&file);
    let env = |on: &'static str| {
        let dir = dir.clone();
        move |name: &str| match name {
            crate::reminders::REFENCE_VARIABLE => Some(on.into()),
            crate::reminders::LEDGER_VARIABLE => Some(dir.clone().into_os_string()),
            _ => None,
        }
    };
    let (engine, socket) = served(1);
    let mut consumer = ahead_of(&engine.lock().unwrap(), None);
    consumer.cannot_ring = Some((file.clone(), None));
    let now = lamp_ipc::monotonic_us();
    let report = due(&engine.lock().unwrap(), now);
    consumer.tick(&socket.path(), Some(&report), now).unwrap();
    no_offer(&consumer);
    assert!(file.exists(), "{:?}", consumer.journal.lines);

    let told = crate::reminders::cannot_ring("set_timer", env("on")).unwrap();
    assert_eq!(told["error"], crate::reminders::CANNOT_RING);
    let calls = [crate::second_provider::net::ToolCall {
        id: "c0".into(),
        name: "set_timer".into(),
        args: json!({}),
        extra: None,
    }];
    for reminded in [false, true] {
        assert_eq!(
            crate::second_provider::timer_line(&calls, std::slice::from_ref(&told), reminded),
            None
        );
    }
    assert!(crate::reminders::cannot_ring("set_reminder", env("on")).is_some());
    assert!(crate::reminders::cannot_ring("repeat_reminder", env("on")).is_some());
    // Reads and cancels go on; with the switch anything but `on`, so do sets.
    assert!(crate::reminders::cannot_ring("list_reminders", env("on")).is_none());
    assert!(crate::reminders::cannot_ring("cancel_reminder", env("on")).is_none());
    assert!(crate::reminders::cannot_ring("set_timer", env("ON")).is_none());
    assert!(crate::reminders::cannot_ring("set_timer", |_| None).is_none());

    // A file left by a consumer that stopped renewing it expires: older
    // than `CANNOT_RING_FRESH`, sets go on. The next tick renews it.
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(
            std::time::SystemTime::now()
                - crate::reminders::CANNOT_RING_FRESH
                - std::time::Duration::from_secs(1),
        )
        .unwrap();
    assert!(crate::reminders::cannot_ring("set_timer", env("on")).is_none());
    consumer.tick(&socket.path(), Some(&report), now).unwrap();
    assert!(crate::reminders::cannot_ring("set_timer", env("on")).is_some());

    // A voice started with the switch on re-fences, so the file goes.
    let (engine, socket) = served(1);
    let mut consumer = ahead_of(&engine.lock().unwrap(), Some("on"));
    consumer.cannot_ring = Some((file.clone(), None));
    let report = due(&engine.lock().unwrap(), now);
    consumer.tick(&socket.path(), Some(&report), now).unwrap();
    assert!(!file.exists(), "{:?}", consumer.journal.lines);
    assert!(crate::reminders::cannot_ring("set_timer", env("on")).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The Reviewer-lead's second P2 on #5668: a store put back while the
/// voice runs left the ledger unarmed, refusing every item, with the
/// cannot-ring file kept for good. After a minute of refusing, the re-fence
/// is armed again: the next tick re-fences, the file goes and the due timer
/// is offered.
#[test]
fn a_ledger_refusing_for_a_minute_is_rearmed_and_rings_again() {
    let dir = std::env::temp_dir().join(format!("lamp-rearm-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(crate::reminders::CANNOT_RING_FILE);
    let _ = std::fs::remove_file(&file);
    let (engine, socket) = served(1);
    let store = engine.lock().unwrap().revision();
    let mut consumer = ahead_of(&engine.lock().unwrap(), None);
    consumer.cannot_ring = Some((file.clone(), None));
    let now = lamp_ipc::monotonic_us();
    let report = due(&engine.lock().unwrap(), now);
    consumer.tick(&socket.path(), Some(&report), now).unwrap();
    no_offer(&consumer);
    assert!(file.exists());
    let later = now + super::REARM_US - 1;
    let report = due(&engine.lock().unwrap(), later);
    consumer.tick(&socket.path(), Some(&report), later).unwrap();
    no_offer(&consumer);
    assert!(file.exists(), "re-armed too soon");
    let minute = now + super::REARM_US;
    let report = due(&engine.lock().unwrap(), minute);
    consumer
        .tick(&socket.path(), Some(&report), minute)
        .unwrap();
    let after = minute + 1;
    let report = due(&engine.lock().unwrap(), after);
    consumer.tick(&socket.path(), Some(&report), after).unwrap();
    assert!(!file.exists(), "{:?}", consumer.journal.lines);
    assert_eq!(consumer.ledger.schedule_revision(), Some(store));
    // The item is offered again (its notice, stamped a minute ahead here,
    // is then withdrawn as aged: a fresh one is said, as in
    // `a_voice_started_on_a_ledger_ahead_of_the_store_says_a_due_timer`).
    assert!(
        consumer
            .journal
            .lines
            .iter()
            .any(|line| line.contains("re-fence is armed again")),
        "{:?}",
        consumer.journal.lines
    );
    assert!(
        !consumer
            .journal
            .lines
            .last()
            .is_some_and(|line| line.contains("stale_revision")),
        "{:?}",
        consumer.journal.lines
    );
    let _ = std::fs::remove_dir_all(&dir);
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

    // Given up with nothing proven twice: held, said once however often
    // it ticks.
    let (engine, socket) = served(1);
    let mut held = consumer(Disk::default());
    let once = begin(&mut held, &engine, &socket);
    abandon(&mut held, &once.token);
    held.next_offer_us = 0;
    let offered = begin(&mut held, &engine, &socket);
    abandon(&mut held, &offered.token);
    for _ in 0..3 {
        // Past the pacing after a settlement, so each tick reaches `held()`
        // (the reviewer on #1081: without this the check could not fail).
        held.next_offer_us = 0;
        let now = lamp_ipc::monotonic_us();
        let report = due(&engine.lock().unwrap(), now);
        held.tick(&socket.path(), Some(&report), now).unwrap();
    }
    assert_eq!(
        held.journal.lines,
        [
            "reminder id 1 delivery 1: due, offered to the voice (attempt 1)".to_owned(),
            format!("reminder id 1 delivery 1: {ONCE_MORE}"),
            "reminder id 1 delivery 1: due, offered to the voice (attempt 2, said as a repeat)"
                .to_owned(),
            format!("reminder id 1 delivery 1: {HELD}"),
        ]
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

/// 29e7, 2 Nov: four one-time reminders 13 to 38 hours late, the
/// microphones just opened. They are offered once, as one line: the oldest
/// with the other three joined, each under its own prepared attempt. One
/// completion settles all four, each is acknowledged and kept by Schedule
/// as said and waiting for an answer, and a line heard after it notices
/// them once every one is acknowledged.
#[test]
fn late_ones_are_offered_as_one_line_and_noticed_after_their_acks() {
    const LATE: u64 = LATE_FOUR_MS;
    let mut engine = Engine::open(Disk::default(), "Etc/UTC", &sample(0), STORE_ID).unwrap();
    for (index, (text, seconds)) in LATE_FOUR.into_iter().enumerate() {
        engine
            .answer(
                Request::Set {
                    person: None,
                    guard: guard(&engine, &format!("set-{index}")),
                    text: text.into(),
                    when: When::After { seconds },
                },
                &sample(0),
            )
            .unwrap();
    }
    engine.tick(&sample(LATE)).unwrap();
    let engine = Arc::new(Mutex::new(engine));
    let shared = engine.clone();
    let socket = Socket::new(move |request| {
        Some(
            shared
                .lock()
                .unwrap()
                .answer(request.clone(), &sample(LATE))
                .unwrap_or_else(|e| refusal(e.code, e.message)),
        )
    });
    let mut consumer = consumer(Disk::default());
    let now = monotonic_us();
    let report = engine
        .lock()
        .unwrap()
        .due(&sample(LATE), stamp(now, 1), true);
    consumer.tick(&socket.path(), Some(&report), now).unwrap();
    let offer = take_offer(&mut consumer);
    assert!(offer.late && offer.late_line());
    // The oldest due is the one said first; the other three with it.
    assert_eq!(offer.text, "Drink water");
    assert_eq!(
        offer.also_texts(),
        ["Water timer", "Send the report", "Buy the gift"]
    );
    assert!(offer.fresh(monotonic_us()));
    // A line is heard after it: noticed only once all are acknowledged.
    consumer
        .command(
            ToWorker::Complete {
                token: offer.token.clone(),
                proof: completion(&offer.token),
            },
            now,
        )
        .unwrap();
    consumer.command(ToWorker::Noticed {}, now).unwrap();
    for joined in &offer.also {
        let view = consumer.ledger.view(&joined.token.key).unwrap().unwrap();
        assert!(view.completion.is_some(), "{view:?}");
    }
    for step in 0..24 {
        let now = monotonic_us() + step;
        let report = engine
            .lock()
            .unwrap()
            .due(&sample(LATE), stamp(now, 1), true);
        consumer.tick(&socket.path(), Some(&report), now).unwrap();
        no_offer(&consumer);
    }
    assert_eq!(engine.lock().unwrap().counts(&sample(LATE)), (0, 0, 0));
    let acks = socket
        .requests()
        .iter()
        .filter(|r| matches!(r, Request::Ack { .. }))
        .count();
    assert_eq!(acks, 4);
    let noticed: Vec<_> = socket
        .requests()
        .into_iter()
        .enumerate()
        .filter(|(_, r)| matches!(r, Request::Noticed {}))
        .collect();
    assert_eq!(noticed.len(), 1, "told once");
    let last_ack = socket
        .requests()
        .iter()
        .rposition(|r| matches!(r, Request::Ack { .. }))
        .unwrap();
    assert!(noticed[0].0 > last_ack, "after every ack");
    let said = engine
        .lock()
        .unwrap()
        .answer(
            Request::ListSet {
                after_id: 0,
                limit: 8,
                pending_only: false,
                asker: None,
            },
            &sample(LATE),
        )
        .unwrap();
    let said = said["said"].as_array().unwrap();
    assert_eq!(said.len(), 4);
    assert!(said.iter().all(|s| s["waiting_for_answer"] == false));
}

/// One on time is offered alone, and a late one alone when nothing else
/// is late.
#[test]
fn an_on_time_one_joins_nothing() {
    let (engine, socket) = served(2);
    let mut consumer = consumer(Disk::default());
    let offer = begin(&mut consumer, &engine, &socket);
    assert!(offer.also.is_empty());
    assert!(!offer.late_line());
}
