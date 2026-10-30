//! Test-only bridge from the actual durable consumer to the actual parent IPC
//! manager. The coordinator suite supplies its own real Reply and receipts.
//! No completion proof is constructed here and no audio device is opened.
//! Its times come from `crate::scenario_clock`, so a coordinator test that
//! enters a scope runs the consumer, the parent and this fixture on one clock.
use super::*;
use crate::reminders::{Reminders, transport};
use lamp_bus::Publisher;
use lamp_msgs::ClockStamp;
use lamp_schedule::{
    delivery::DeliveryView,
    engine::Engine,
    protocol::{Guard, refusal},
    time::{ClockSample, When},
};
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

const CLOCK: &str = "roundtrip-boot";
const STORE_ID: &str = "0123456789abcdef0123456789abcdef";
const LEDGER_ID: &str = "abcdef0123456789abcdef0123456789";
const WALL: i64 = 1_791_964_800_000;
const TEXT: &str = "The private tea timer is due.";

fn clock() -> ClockSample {
    clock_after(0)
}

/// Schedule's clock `ms` after the fixture's fixed start: its wall and boot
/// clocks move together ([`RoundtripFixture::advance_wall`]).
fn clock_after(ms: u64) -> ClockSample {
    ClockSample {
        unix_ms: WALL + 1000 + i64::try_from(ms).unwrap(),
        boot_ms: 1000 + ms,
        boot_id: CLOCK.into(),
        wall_trusted: true,
    }
}

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = fs::canonicalize("/tmp").unwrap().join(format!(
            "sr-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
    fn child(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        path
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A private socket endpoint backed by the actual Schedule engine and Store.
/// Only its framing loop and deliberate lost-Ack reply are fixture behavior.
struct Schedule {
    socket: PathBuf,
    engine: Arc<Mutex<Engine<Store>>>,
    requests: Arc<Mutex<Vec<Request>>>,
    lose_acks: Arc<AtomicBool>,
    /// How far Schedule's clock has been moved on, in ms.
    advanced: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl Schedule {
    fn new(root: &Directory) -> Self {
        let mut created = clock();
        created.unix_ms -= 1000;
        created.boot_ms = 0;
        let mut engine = Engine::open(
            Store::open(&root.child("schedule")).unwrap(),
            "Etc/UTC",
            &created,
            STORE_ID,
        )
        .unwrap();
        engine
            .answer(
                Request::Set {
                    person: None,
                    guard: Guard {
                        store_id: STORE_ID.into(),
                        revision: 0,
                        request_id: "roundtrip-create".into(),
                    },
                    text: TEXT.into(),
                    when: When::After { seconds: 1 },
                },
                &created,
            )
            .unwrap();
        engine.tick(&clock()).unwrap();
        let engine = Arc::new(Mutex::new(engine));
        let socket = root.0.join("schedule.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let lose_acks = Arc::new(AtomicBool::new(false));
        let advanced = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (shared, captured, lose, moved, stopped) = (
            engine.clone(),
            requests.clone(),
            lose_acks.clone(),
            advanced.clone(),
            stop.clone(),
        );
        let thread = thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                let mut stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(error) => panic!("Schedule fixture accept: {error}"),
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
                let mutation = matches!(request, Request::Ack { .. });
                captured.lock().unwrap().push(request.clone());
                let reply = shared
                    .lock()
                    .unwrap()
                    .answer(request, &clock_after(moved.load(Ordering::Acquire)))
                    .unwrap_or_else(|error| refusal(error.code, error.message));
                if mutation && lose.load(Ordering::Acquire) {
                    continue; // The real commit happened; its reply is lost.
                }
                let _ = writeln!(stream, "{reply}");
            }
        });
        Self {
            socket,
            engine,
            requests,
            lose_acks,
            advanced,
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for Schedule {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.thread.take().unwrap().join().unwrap();
    }
}

/// Sole test-only view across the consumer/coordinator module boundary.
/// `settle_parent` accepts only bytes emitted by the real Reminders manager;
/// no method takes or manufactures a QualifiedCompletion.
pub(crate) struct RoundtripFixture {
    directory: Directory,
    ledger_directory: PathBuf,
    schedule: Option<Schedule>,
    consumer: Option<Consumer<Store>>,
    parent: Option<Reminders>,
    worker: Option<WorkerTransport>,
    bus: Bus,
    publisher: Option<Publisher>,
    freshness: Freshness,
    sequence: u64,
    incarnation: u64,
    offered: usize,
    received: Vec<ToWorker>,
}
impl RoundtripFixture {
    pub(crate) fn new() -> Self {
        let directory = Directory::new();
        let ledger_directory = directory.child("ledger");
        let schedule = Schedule::new(&directory);
        let bus = Bus::new(directory.0.join("bus")).unwrap();
        let publisher = bus
            .publisher_declared(ScheduleDue::TOPIC, &ScheduleDue::declaration(), "schedule")
            .unwrap();
        let mut fixture = Self {
            directory,
            ledger_directory,
            schedule: Some(schedule),
            consumer: None,
            parent: None,
            worker: None,
            bus,
            publisher: Some(publisher),
            freshness: Freshness::default(),
            sequence: 0,
            incarnation: 0,
            offered: 0,
            received: Vec::new(),
        };
        fixture.open_consumer();
        fixture
    }

    fn open_consumer(&mut self) {
        self.incarnation += 1;
        self.consumer = Some(Consumer::new(
            DeliveryLedger::open(
                transport::when_free(|| Store::open(&self.ledger_directory)).unwrap(),
                LEDGER_ID,
            )
            .unwrap(),
            format!("roundtrip-{}", self.incarnation),
        ));
        // A restart reopens both locks the dropped incarnation held.
        let (transport, worker) =
            transport::when_free(|| transport::test_pair(&self.directory.0.join("claim"))).unwrap();
        self.parent = Some(Reminders {
            transport,
            offer: None,
            active: None,
            unadmitted: None,
            sent: false,
            failed: false,
            by_skill: false,
            owed_told: None,
        });
        self.worker = Some(worker);
        self.parent().poll(monotonic_us());
        assert!(self.parent().transport.claim_held());
        assert!(!self.parent().failed);
    }

    pub(crate) fn parent(&mut self) -> &mut Reminders {
        self.parent.as_mut().unwrap()
    }

    /// Exercise the production metadata reader, one consumer tick and actual
    /// serialized datagrams. It never drains a parent completion implicitly.
    pub(crate) fn tick(&mut self) {
        self.sequence += 1;
        let now = monotonic_us();
        let schedule = self.schedule.as_ref().unwrap();
        let at = clock_after(schedule.advanced.load(Ordering::Acquire));
        let report = schedule.engine.lock().unwrap().due(
            &at,
            ClockStamp {
                clock_id: CLOCK.into(),
                at_us: now,
                sequence: self.sequence,
            },
            true,
        );
        self.publisher
            .as_mut()
            .unwrap()
            .publish_json(now, &report)
            .unwrap();
        let due = self
            .freshness
            .read(&self.bus, CLOCK, monotonic_us())
            .unwrap();
        let path = self.schedule.as_ref().unwrap().socket.clone();
        self.consumer
            .as_mut()
            .unwrap()
            .tick(&path, Some(&due), now)
            .unwrap();
        self.send_outgoing();
    }

    fn send_outgoing(&mut self) {
        while let Some(message) = self.consumer.as_mut().unwrap().outgoing.pop_front() {
            if matches!(message, FromWorker::Offer { .. }) {
                self.offered += 1;
            }
            assert!(self.worker.as_mut().unwrap().send(&message).unwrap());
        }
        // A parent lent to the muted wait ([`Self::lend_parent`]) is polled
        // by the wait itself.
        if let Some(parent) = self.parent.as_mut() {
            parent.poll(monotonic_us());
            assert!(!parent.failed);
        }
    }

    /// The parent, for a test to hand to the muted wait as the coordinator
    /// does; [`Self::tick`] goes on offering to it meanwhile, and
    /// [`Self::return_parent`] gives it back.
    pub(crate) fn lend_parent(&mut self) -> Reminders {
        self.parent.take().expect("parent not lent already")
    }
    pub(crate) fn return_parent(&mut self, parent: Reminders) {
        assert!(self.parent.is_none(), "parent lent once");
        self.parent = Some(parent);
    }

    /// Moves Schedule's wall and boot clocks on by `ms`, as time passing
    /// while the lamp is muted: a due item becomes that much later.
    pub(crate) fn advance_wall(&self, ms: u64) {
        self.schedule
            .as_ref()
            .unwrap()
            .advanced
            .fetch_add(ms, Ordering::AcqRel);
    }

    /// The person cancels the due item through Schedule, as the app does.
    pub(crate) fn cancel(&self) {
        let socket = &self.schedule.as_ref().unwrap().socket;
        let deadline = || Instant::now() + Duration::from_secs(2);
        let item = exchange(socket, &Request::Get { id: 1 }, deadline()).unwrap();
        let revision = item["revision"].as_u64().unwrap();
        let cancelled = exchange(
            socket,
            &Request::Cancel {
                asker: None,
                guard: Guard {
                    store_id: STORE_ID.into(),
                    revision,
                    request_id: "roundtrip-cancel".into(),
                },
                id: 1,
            },
            deadline(),
        )
        .unwrap();
        assert_eq!(cancelled["ok"], true, "{cancelled}");
    }

    pub(crate) fn offer(&mut self) -> Offer {
        // An offer is fresh for its lease (750 ms) from the tick that made
        // it. One that came and expired before it was read means the host
        // stalled that long: another tick offers it afresh, up to five times.
        // An offer missing or refused while fresh still fails at once (#1569).
        for _ in 0..5 {
            self.tick();
            let now = monotonic_us();
            if let Some(offer) = self.parent().offer(now) {
                return offer;
            }
            let expired = self
                .parent()
                .offer
                .as_ref()
                .is_some_and(|offer| now >= offer.expires_at_us);
            assert!(expired, "fresh durable offer");
        }
        panic!("the host stalled past the offer's lease in 5 of 5 ticks");
    }

    /// A real manager packet is applied to the durable consumer before the
    /// returned Settled message is allowed to clear the parent's active slot.
    pub(crate) fn settle_parent(&mut self) -> usize {
        self.parent().poll(monotonic_us());
        let mut count = 0;
        for _ in 0..QUEUE {
            let Some(command) = self.worker.as_mut().unwrap().receive::<ToWorker>().unwrap() else {
                break;
            };
            self.received.push(command.clone());
            self.consumer
                .as_mut()
                .unwrap()
                .command(command, monotonic_us())
                .unwrap();
            count += 1;
        }
        self.send_outgoing();
        count
    }

    pub(crate) fn view(&self, token: &PreparedToken) -> DeliveryView {
        self.consumer
            .as_ref()
            .unwrap()
            .ledger
            .view(&token.key)
            .unwrap()
            .unwrap()
    }
    pub(crate) fn ack_requests(&self) -> Vec<Request> {
        self.schedule
            .as_ref()
            .unwrap()
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| matches!(request, Request::Ack { .. }))
            .cloned()
            .collect()
    }
    pub(crate) fn received(&self) -> &[ToWorker] {
        &self.received
    }
    pub(crate) fn offered_count(&self) -> usize {
        self.offered
    }
    pub(crate) fn lose_ack_replies(&self) {
        self.schedule
            .as_ref()
            .unwrap()
            .lose_acks
            .store(true, Ordering::Release);
    }
    pub(crate) fn pending(&self) -> bool {
        let value = exchange(
            &self.schedule.as_ref().unwrap().socket,
            &Request::Get { id: 1 },
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
        value["ok"] == true && value["item"]["id"] == 1 && value["item"]["state"] == "pending"
    }

    pub(crate) fn restart_consumer(&mut self) {
        assert!(
            self.parent().active.is_none(),
            "old Reply must already have dropped"
        );
        assert!(self.parent().unadmitted.is_none());
        self.parent.take();
        self.worker.take();
        self.consumer.take();
        self.open_consumer();
    }
}
impl Drop for RoundtripFixture {
    fn drop(&mut self) {
        self.parent.take();
        self.worker.take();
        self.consumer.take();
        self.schedule.take();
        self.publisher.take();
    }
}
