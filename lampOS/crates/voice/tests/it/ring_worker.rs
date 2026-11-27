use lamp_ipc::monotonic_us;
use lamp_ring::{EncodedFrame, FrameSink};
use lamp_turn_owner::{
    AdmissionState, AdmittedInput, BootId, CaptureState, Controller, MonoTime, OutputKind,
    Permission, TurnOwner,
};
use lamp_voice::{
    process::{SessionDirectory, new_boot},
    ring_wire::{RingBlankReason, RingFeedback, RingFrame, RingPhase, RingRejection},
    ring_worker::{RingLink, Senses, run_with_sink, run_with_sink_senses},
    transport::WorkerChannels,
    wire::{Control, WorkerEvent, decode, encode},
};
use std::{
    collections::VecDeque,
    io,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

fn now() -> MonoTime {
    MonoTime::from_micros(monotonic_us())
}

#[derive(Default)]
struct Writes {
    frames: Vec<EncodedFrame>,
    fail_at: Vec<usize>,
    late_first: bool,
    /// Writes, by their place, that return over the budget, with how long
    /// each took (#1620).
    late_at: Vec<(usize, u64)>,
    /// How long the last write took.
    last_us: u64,
}
#[derive(Clone)]
struct Sink(Arc<Mutex<Writes>>);
impl FrameSink for Sink {
    fn write_frame(&mut self, frame: &EncodedFrame) -> io::Result<()> {
        let mut writes = self.0.lock().unwrap();
        writes.frames.push(frame.clone());
        let call = writes.frames.len();
        // Over lamp_ring::WRITE_BUDGET_US, as a bus held by SD/MMC re-tuning.
        // Said through the sink's seam, not slept, so host load cannot
        // change it.
        writes.last_us = match writes.late_at.iter().find(|(at, _)| *at == call) {
            Some(&(_, us)) => us,
            None if writes.late_first && call == 1 => 13_180,
            None => 100,
        };
        if writes.fail_at.contains(&call) {
            return Err(io::Error::other(format!("injected write {call}")));
        }
        Ok(())
    }
    fn simulated_elapsed_us(&self) -> Option<u64> {
        Some(self.0.lock().unwrap().last_us)
    }
}

struct SocketLink {
    channels: WorkerChannels,
    parent: Arc<Mutex<WorkerChannels>>,
    after_receive: Arc<Mutex<VecDeque<Control>>>,
    /// The rig's hold-ups, as a loaded runner makes them (#8466).
    holds: Arc<Mutex<Holds>>,
    /// When each control read began, to tell a held-up attempt (#8466).
    controls_at: Arc<Mutex<Vec<u64>>>,
}
impl RingLink for SocketLink {
    fn receive_control(&mut self) -> io::Result<Option<Control>> {
        self.controls_at.lock().unwrap().push(monotonic_us());
        let stall = self.holds.lock().unwrap().stall.take();
        if let Some(stall) = stall {
            thread::sleep(stall);
        }
        self.channels.control.receive()
    }
    fn receive_frame(&mut self) -> io::Result<Option<RingFrame>> {
        let frame = self.channels.data.receive()?;
        if frame.is_some() {
            for control in self.after_receive.lock().unwrap().drain(..) {
                self.parent.lock().unwrap().control.send(control)?;
            }
        }
        Ok(frame)
    }
    fn publish(&mut self, event: WorkerEvent) -> io::Result<()> {
        let blanked = matches!(
            event,
            WorkerEvent::Ring {
                report: RingFeedback::Blanked { .. }
            }
        );
        if blanked && self.holds.lock().unwrap().blanked_late {
            thread::sleep(BLANKED_LATE);
        }
        self.channels.control.send(event)
    }
}

/// The rig's own hold-ups, none unless a test sets them (#8466).
#[derive(Default)]
struct Holds {
    /// The worker held up once, at its next control read.
    stall: Option<Duration>,
    /// Each blank said [`BLANKED_LATE`] late, as a worker a little behind
    /// on a loaded runner says it.
    blanked_late: bool,
}

/// How late a [`Holds::blanked_late`] blank is said: a worker 20 ms behind,
/// well under `stalls::OVER`, as on #8466's ARM64 run.
const BLANKED_LATE: Duration = Duration::from_millis(20);

struct Rig {
    _directory: SessionDirectory,
    boot: BootId,
    parent: Arc<Mutex<WorkerChannels>>,
    writes: Arc<Mutex<Writes>>,
    after_receive: Arc<Mutex<VecDeque<Control>>>,
    holds: Arc<Mutex<Holds>>,
    controls_at: Arc<Mutex<Vec<u64>>>,
    completion: mpsc::Receiver<io::Result<()>>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Rig {
    fn start(fail_at: &[usize]) -> Self {
        Self::start_with(fail_at, false)
    }
    fn start_with(fail_at: &[usize], late_first: bool) -> Self {
        Self::start_full(fail_at, late_first, None, None)
    }
    /// With the all-day worker's senses, for the head's requests (#711).
    fn start_with_senses(senses: Senses) -> Self {
        Self::start_full(&[], false, Some(senses), None)
    }
    /// As [`Self::start_with_senses`], painting a resting face as the
    /// all-day service's ring does.
    fn start_resting(senses: Senses, resting: u16) -> Self {
        Self::start_full(&[], false, Some(senses), Some(resting))
    }
    fn start_full(
        fail_at: &[usize],
        late_first: bool,
        senses: Option<Senses>,
        resting: Option<u16>,
    ) -> Self {
        let directory = SessionDirectory::create().unwrap();
        let boot = new_boot().unwrap();
        let child_boot = new_boot().unwrap();
        let mut parent =
            WorkerChannels::bind(&directory.path, "ring", boot, child_boot, false).unwrap();
        let mut channels =
            WorkerChannels::bind(&directory.path, "ring", boot, child_boot, true).unwrap();
        parent.connect(&directory.path, "ring", false).unwrap();
        channels.connect(&directory.path, "ring", true).unwrap();
        let parent = Arc::new(Mutex::new(parent));
        let writes = Arc::new(Mutex::new(Writes {
            fail_at: fail_at.to_vec(),
            late_first,
            ..Writes::default()
        }));
        let after_receive = Arc::new(Mutex::new(VecDeque::new()));
        let holds = Arc::new(Mutex::new(Holds::default()));
        let controls_at = Arc::new(Mutex::new(Vec::new()));
        let link = SocketLink {
            channels,
            parent: parent.clone(),
            after_receive: after_receive.clone(),
            holds: holds.clone(),
            controls_at: controls_at.clone(),
        };
        let sink = Sink(writes.clone());
        let (tx, completion) = mpsc::channel();
        let worker = thread::spawn(move || {
            let _ = tx.send(match senses {
                Some(senses) => run_with_sink_senses(link, boot, sink, resting, senses),
                None => run_with_sink(link, boot, sink),
            });
        });
        Self {
            _directory: directory,
            boot,
            parent,
            writes,
            after_receive,
            holds,
            controls_at,
            completion,
            worker: Some(worker),
        }
    }
    fn ready(&self) {
        assert!(matches!(self.event(), WorkerEvent::Ready));
        assert_eq!(self.writes.lock().unwrap().frames, [EncodedFrame::black()]);
    }
    fn event(&self) -> WorkerEvent {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(event) = self.parent.lock().unwrap().control.receive().unwrap() {
                return event;
            }
            assert!(Instant::now() < deadline, "finite worker feedback deadline");
            thread::sleep(Duration::from_millis(1));
        }
    }
    /// Every event said so far, unread.
    fn drain(&self) {
        while self
            .parent
            .lock()
            .unwrap()
            .control
            .receive::<WorkerEvent>()
            .unwrap()
            .is_some()
        {}
    }
    fn control(&self, command: Control) {
        self.parent.lock().unwrap().control.send(command).unwrap();
    }
    fn frame(&self, frame: RingFrame) {
        self.parent.lock().unwrap().data.send(frame).unwrap();
    }
    fn finish(&mut self) -> io::Result<()> {
        let result = self
            .completion
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        self.worker.take().unwrap().join().unwrap();
        result
    }
    /// The worker held up `stall` once, at its next control read.
    fn stall_next_control(&self, stall: Duration) {
        self.holds.lock().unwrap().stall = Some(stall);
    }
    /// The longest the worker went between two control reads, or from its
    /// last one to now, over any of `from_us..to_us`: how long, and when it
    /// began.
    fn worker_gap(&self, from_us: u64, to_us: u64) -> Option<(u64, u64)> {
        let mut at = self.controls_at.lock().unwrap().clone();
        at.push(monotonic_us());
        at.windows(2)
            .filter(|read| read[0] < to_us && read[1] > from_us)
            .map(|read| (read[1] - read[0], read[0]))
            .max()
    }
    fn colored(&self) -> usize {
        self.writes
            .lock()
            .unwrap()
            .frames
            .iter()
            .filter(|v| **v != EncodedFrame::black())
            .count()
    }
}
impl Drop for Rig {
    fn drop(&mut self) {
        if self.worker.is_some() {
            let _ = self.parent.lock().unwrap().control.send(Control::Stop);
            if self.completion.recv_timeout(Duration::from_secs(2)).is_ok() {
                let _ = self.worker.take().unwrap().join();
            }
        }
    }
}

fn conversation(boot: BootId) -> (Controller, TurnOwner) {
    let mut controller = Controller::new(boot, now());
    controller
        .set_microphone_permission(now(), Permission::Allowed)
        .unwrap();
    controller
        .set_capture(
            now(),
            CaptureState::RetainingUntil(now().checked_add(900_000).unwrap()),
        )
        .unwrap();
    controller
        .set_admission(
            now(),
            AdmissionState::OpenUntil(now().checked_add(900_000).unwrap()),
        )
        .unwrap();
    let owner = controller.admit(now(), AdmittedInput::NewTurn).unwrap();
    (controller, owner)
}
fn frame(controller: &mut Controller, owner: TurnOwner, lease_us: u64) -> RingFrame {
    let plan = controller
        .plan_output(now(), owner, OutputKind::Light, None)
        .unwrap();
    let permit = controller.issue_output(now(), plan, lease_us).unwrap();
    let snapshot = controller.snapshot(now()).unwrap();
    RingFrame {
        snapshot,
        permit,
        phase: RingPhase::from_snapshot(snapshot).unwrap(),
        ceiling: 40,
        requested_at_us: monotonic_us(),
        level: 0,
        attending: false,
        offline: false,
        listening: false,
        accent: 0,
        accent_at_us: 0,
    }
}
fn present(rig: &Rig, frame: RingFrame) {
    rig.control(Control::Authority {
        snapshot: frame.snapshot,
    });
    rig.frame(frame);
    let event = rig.event();
    assert!(
        matches!(event, WorkerEvent::Ring { report: RingFeedback::Presented { requested_at_us, .. } } if requested_at_us == frame.requested_at_us),
        "{event:?}"
    );
}

fn cap_peak(frame: &EncodedFrame) -> u8 {
    frame.bytes()[lamp_ring::PRIMER_BYTES..lamp_ring::PRIMER_BYTES + lamp_ring::PIXEL_COUNT * 24]
        .chunks_exact(8)
        .map(|bits| {
            bits.iter()
                .fold(0, |value, bit| (value << 1) | u8::from(*bit == 0xfc))
        })
        .max()
        .unwrap_or(0)
}

fn publish_cap(publisher: &mut lamp_bus::Publisher, revision: u64, limit: u16) {
    let wall = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let value = lamp_msgs::OutputCaps {
        stamp: lamp_msgs::ClockStamp {
            clock_id: "caps-test-boot".into(),
            at_us: monotonic_us(),
            sequence: publisher.totals().published + 1,
        },
        session_id: publisher.boot().into(),
        revision,
        entries: vec![lamp_msgs::OutputCap {
            source: "daily-rest".into(),
            token: format!("revision-{revision}"),
            issued_unix_ms: wall,
            valid_until_unix_ms: wall + 60_000,
            light_channel_max: Some(limit),
            volume_raw_max: None,
        }],
    };
    publisher.publish_json(value.stamp.at_us, &value).unwrap();
}

fn cap_input(
    limit: u16,
) -> (
    SessionDirectory,
    lamp_bus::Publisher,
    lamp_voice::ring_caps::RingCaps,
) {
    let directory = SessionDirectory::create().unwrap();
    let bus = lamp_bus::Bus::new(&directory.path).unwrap();
    let mut publisher = bus
        .publisher_declared(
            lamp_msgs::OutputCaps::TOPIC,
            &lamp_msgs::OutputCaps::declaration(),
            lamp_msgs::OutputCaps::SOURCE,
        )
        .unwrap();
    let mut caps = lamp_voice::ring_caps::RingCaps::on(&directory.path, "caps-test-boot").unwrap();
    publish_cap(&mut publisher, 1, limit);
    let until = Instant::now() + Duration::from_secs(1);
    while caps.tick(monotonic_us()) != limit {
        assert!(Instant::now() < until, "initial cap never arrived");
        thread::sleep(Duration::from_millis(2));
    }
    (directory, publisher, caps)
}

#[test]
fn output_caps_start_dark_before_any_snapshot_even_with_a_valid_turn() {
    let mut rig = Rig::start_with_senses(Senses {
        caps: Some(lamp_voice::ring_caps::RingCaps::dark()),
        ..Senses::default()
    });
    rig.ready();
    let (mut controller, owner) = conversation(rig.boot);
    present(&rig, frame(&mut controller, owner, 200_000));
    assert_eq!(rig.colored(), 0);
    rig.control(Control::Stop);
    rig.finish().unwrap();
}

/// How a [`caps_dim_latched`] run is held up, or broken by the test itself.
#[derive(Clone, Copy, Default)]
struct DimCase {
    /// Each blank said [`BLANKED_LATE`] late ([`Holds::blanked_late`]).
    blanked_late: bool,
    /// The worker held up [`STALL`] once just after the cap drops.
    worker_stall: bool,
    /// The test's own thread held up [`STALL`], a time it did not ask for,
    /// just before the cap drop is sent.
    test_stall: bool,
    /// The cap is never dropped, so nothing dims: a regression's run.
    keep_cap: bool,
    /// The test asks for a [`STALL`]'s sleep before the cap drop, so the
    /// permit runs out with neither thread held up.
    late_cap_drop: bool,
}

/// Longer than a turn frame's 250 ms permit.
const STALL: Duration = Duration::from_millis(300);

/// Attempts of [`caps_dim_latched`] before a run held up in each fails, as
/// #8779's `STALLED_ATTEMPTS` in `privacy_bus.rs`.
const HELD_UP_ATTEMPTS: usize = 20;

#[test]
fn output_caps_clamp_turn_frames_and_tightening_dims_latched_pixels_without_a_new_frame() {
    caps_dim_latched(DimCase::default()).unwrap();
}

/// #8466, the Reviewer-lead's ARM64 log: a frame's own expiry blank said a
/// little late came after the drain, and the next frame's present read it
/// (`Blanked { AuthorityLost(ExpiredPermit) }`). The run waits for that
/// blank instead of sleeping the permit's length.
#[test]
fn a_frames_expiry_blank_said_late_is_waited_for() {
    caps_dim_latched(DimCase {
        blanked_late: true,
        ..DimCase::default()
    })
    .unwrap();
}

/// #8466: "tightening did not dim the latched frame before its permit
/// ended", the worker held up past the permit, is an attempt made again.
#[test]
fn a_worker_held_up_after_the_cap_drop_is_a_held_up_attempt() {
    let attempts = caps_dim_latched(DimCase {
        worker_stall: true,
        ..DimCase::default()
    })
    .unwrap();
    assert!(attempts >= 2, "{attempts}");
}

/// #8466, the Reviewer-lead's ruling (c7221951066): the test's own thread
/// held up past the permit before the cap drop is an attempt made again.
#[test]
fn the_test_thread_held_up_before_the_cap_drop_is_a_held_up_attempt() {
    let attempts = caps_dim_latched(DimCase {
        test_stall: true,
        ..DimCase::default()
    })
    .unwrap();
    assert!(attempts >= 2, "{attempts}");
}

/// No dim with neither thread held up still fails, at once.
#[test]
fn a_missing_dim_with_no_stall_still_fails() {
    let failed = caps_dim_latched(DimCase {
        keep_cap: true,
        ..DimCase::default()
    })
    .unwrap_err();
    assert!(failed.contains("did not dim"), "{failed}");
    assert!(!failed.contains("held up in all"), "{failed}");
}

/// A cap drop late only by the test's own asked sleep still fails, at once.
#[test]
fn a_cap_drop_late_only_by_an_asked_sleep_still_fails() {
    let failed = caps_dim_latched(DimCase {
        late_cap_drop: true,
        ..DimCase::default()
    })
    .unwrap_err();
    assert!(failed.contains("did not dim"), "{failed}");
    assert!(!failed.contains("held up in all"), "{failed}");
}

/// Capture and admission open for 900 ms from now, as the resting test
/// keeps them while it settles.
fn renew(controller: &mut Controller) {
    let until = now().checked_add(900_000).unwrap();
    controller
        .set_capture(now(), CaptureState::RetainingUntil(until))
        .unwrap();
    controller
        .set_admission(now(), AdmissionState::OpenUntil(until))
        .unwrap();
}

/// Waits, at most 2 s, for the blank of `shown`'s own expiry, then reads
/// every event said so far (#8466). Sleeping the permit's length instead
/// raced that blank: said a little late, it came after the drain and the
/// next present read it.
///
/// The blank is `AuthorityLost` with `ExpiredPermit` or `ExpiredState`,
/// written at or after the earlier of the frame's permit and snapshot
/// expiries, both on the one monotonic clock that issued them. Both reasons
/// are the same expiry: the snapshot and the permit are issued together,
/// each for 250 ms, and the guard checks the snapshot first, so which one
/// the ring names is only the order of the checks and of the microseconds
/// between the two issues. Events other than a blank are passed over; a
/// blank for any other reason, or one before the expiry, fails.
fn wait_expiry(rig: &Rig, shown: RingFrame, name: &str) {
    let expires = shown
        .permit
        .expires_at()
        .as_micros()
        .min(shown.snapshot.expires_at().as_micros());
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let event = rig.parent.lock().unwrap().control.receive().unwrap();
        match event {
            Some(WorkerEvent::Ring {
                report:
                    RingFeedback::Blanked {
                        reason:
                            RingBlankReason::AuthorityLost {
                                reason: RingRejection::ExpiredPermit | RingRejection::ExpiredState,
                            },
                        write_started_at_us,
                        ..
                    },
            }) if write_started_at_us >= expires => break,
            Some(
                event @ WorkerEvent::Ring {
                    report: RingFeedback::Blanked { .. },
                },
            ) => {
                panic!("waiting for {name}'s expiry blank at {expires} us, the ring said {event:?}")
            }
            Some(_) | None => {}
        }
        assert!(
            Instant::now() < deadline,
            "{name}'s expiry blank at {expires} us never came within 2 s"
        );
        thread::sleep(Duration::from_millis(1));
    }
    rig.drain();
}

/// The test thread's own time from each present's end to its next send,
/// less the sleeps it asked for: time a loaded runner took from it (#8466).
struct TestClock {
    since: Instant,
    since_us: u64,
    asked: Duration,
    sends: usize,
    /// Each send's gap: how long, when it began and ended, and after which
    /// send.
    gaps: Vec<(u64, u64, u64, usize)>,
}
impl TestClock {
    fn new() -> Self {
        Self {
            since: Instant::now(),
            since_us: monotonic_us(),
            asked: Duration::ZERO,
            sends: 0,
            gaps: Vec::new(),
        }
    }
    /// A sleep the test asks for, left out of the gap.
    fn sleep(&mut self, sleep: Duration) {
        thread::sleep(sleep);
        self.asked += sleep;
    }
    /// The test held up `hold`, a time it did not ask for, as a loaded
    /// runner holds it.
    fn hold(&mut self, hold: Duration) {
        thread::sleep(hold);
    }
    /// Just before a send.
    fn send(&mut self) {
        let gap = self.since.elapsed().saturating_sub(self.asked);
        self.gaps.push((
            gap.as_micros() as u64,
            self.since_us,
            monotonic_us(),
            self.sends,
        ));
        self.sends += 1;
    }
    /// From here the test's time is its own again: just after a send's
    /// answer, or a wait on the worker.
    fn mark(&mut self) {
        self.since = Instant::now();
        self.since_us = monotonic_us();
        self.asked = Duration::ZERO;
    }
    /// The longest gap over any of `from_us..to_us`: how long, when it
    /// began, and after which send.
    fn gap(&self, from_us: u64, to_us: u64) -> Option<(u64, u64, usize)> {
        self.gaps
            .iter()
            .filter(|(_, began, ended, _)| *began < to_us && *ended > from_us)
            .map(|&(us, began, _, after)| (us, began, after))
            .max()
    }
}

/// Which thread, if either, was held up [`lamp_voice::stalls::OVER`] or
/// more inside the permit `from_us..to_us`, and how: the retry's reason.
fn held_up(rig: &Rig, clock: &TestClock, from_us: u64, to_us: u64) -> Option<String> {
    let over = lamp_voice::stalls::OVER.as_micros() as u64;
    let into = |at: u64| (at as i64 - from_us as i64) / 1_000;
    let worker = rig
        .worker_gap(from_us, to_us)
        .filter(|(us, _)| *us >= over)
        .map(|(us, at)| {
            format!(
                "worker thread {} ms between control reads, from {} ms into the permit",
                us / 1_000,
                into(at)
            )
        });
    let test = clock
        .gap(from_us, to_us)
        .filter(|(us, _, _)| *us >= over)
        .map(|(us, at, after)| {
            format!(
                "test thread {} ms between send {} and {}, from {} ms into the permit",
                us / 1_000,
                after,
                after + 1,
                into(at)
            )
        });
    match (worker, test) {
        (Some(worker), Some(test)) => Some(format!("{worker}; {test}")),
        (worker, test) => worker.or(test),
    }
}

/// A [`dim_attempt`] whose latched frame was not dimmed before its permit
/// ended: what it saw, the hold-up inside that permit, if either thread had
/// one, and that latched frame, whose expiry blank the retry waits for.
struct Undimmed {
    said: String,
    held: Option<String>,
    /// Boxed: a frame is large beside the rest.
    original: Box<RingFrame>,
}

/// A cap tightened from 30 to 12 dims the latched turn frame before its
/// permit ends. An attempt that missed the dim with the worker or the
/// test's own thread held up [`lamp_voice::stalls::OVER`] or more inside
/// that permit, which a loaded runner does (#8466), says nothing either way
/// and is made again, saying why; a missed dim with neither held up fails,
/// and so does anything else. The number of attempts made, or why it
/// failed.
fn caps_dim_latched(case: DimCase) -> Result<usize, String> {
    let (directory, mut publisher, caps) = cap_input(30);
    let mut rig = Rig::start_with_senses(Senses {
        caps: Some(caps),
        ..Senses::default()
    });
    rig.holds.lock().unwrap().blanked_late = case.blanked_late;
    rig.ready();
    let (mut controller, owner) = conversation(rig.boot);
    let mut revision = 1;
    let mut held = Vec::new();
    let mut ended = None;
    // The held-up attempt's latched frame, whose expiry blank the next
    // attempt waits for before it starts: drained before that blank came,
    // the next present would read it (#8466).
    let mut missed: Option<Box<RingFrame>> = None;
    for attempt in 1..=HELD_UP_ATTEMPTS {
        if let Some(original) = missed.take() {
            wait_expiry(&rig, *original, "the held-up attempt's latched turn frame");
            // Back to the first attempt's start: the cap at 30, eased up.
            renew(&mut controller);
            revision += 1;
            publish_cap(&mut publisher, revision, 30);
            thread::sleep(Duration::from_millis(250));
            rig.drain();
        }
        // Only the first attempt is held up by hand.
        let case = DimCase {
            worker_stall: case.worker_stall && attempt == 1,
            test_stall: case.test_stall && attempt == 1,
            ..case
        };
        match dim_attempt(
            &rig,
            &mut controller,
            owner,
            &mut publisher,
            &mut revision,
            case,
        ) {
            Ok(()) => {
                ended = Some(Ok(attempt));
                break;
            }
            Err(Undimmed {
                said,
                held: Some(why),
                original,
            }) => {
                eprintln!("retry {attempt}: {why}");
                held.push(format!("{said} ({why})"));
                missed = Some(original);
            }
            Err(Undimmed {
                said, held: None, ..
            }) => {
                ended = Some(Err(said));
                break;
            }
        }
    }
    let ended = ended.unwrap_or_else(|| {
        Err(format!(
            "held up in all {HELD_UP_ATTEMPTS} attempts: {held:?}"
        ))
    });
    rig.control(Control::Stop);
    rig.finish().unwrap();
    drop(publisher);
    std::fs::remove_dir_all(&directory.path).unwrap();
    ended
}

/// One [`present`] in a [`dim_attempt`], timed on its [`TestClock`].
fn show(rig: &Rig, clock: &mut TestClock, frame: RingFrame) {
    clock.send();
    present(rig, frame);
    clock.mark();
}

/// One attempt of [`caps_dim_latched`], on its rig.
fn dim_attempt(
    rig: &Rig,
    controller: &mut Controller,
    owner: TurnOwner,
    publisher: &mut lamp_bus::Publisher,
    revision: &mut u64,
    case: DimCase,
) -> Result<(), Undimmed> {
    // Each attempt, and its end past the dim, under capture and admission
    // renewed: a 900 ms window from `conversation` would end the turn
    // within a retried run.
    renew(controller);
    let mut clock = TestClock::new();
    let mut bright = frame(controller, owner, 250_000);
    bright.ceiling = 120;
    show(rig, &mut clock, bright);
    // Past the startup's ease up to the cap (#8615): its permit's own end.
    wait_expiry(rig, bright, "the first bright frame");
    clock.mark();
    // The face eased up again over a few frames.
    for _ in 0..4 {
        let mut bright = frame(controller, owner, 250_000);
        bright.ceiling = 120;
        show(rig, &mut clock, bright);
        clock.sleep(Duration::from_millis(30));
    }
    let mut original = frame(controller, owner, 250_000);
    original.ceiling = 120;
    show(rig, &mut clock, original);
    let before = rig.writes.lock().unwrap().frames.len();
    let peak = cap_peak(&rig.writes.lock().unwrap().frames[before - 1]);
    assert!(peak > 12 && peak <= 30, "turn peak {peak}");
    if case.test_stall {
        clock.hold(STALL);
    } else if case.late_cap_drop {
        clock.sleep(STALL);
    }
    // Armed before the cap drop is sent, so a test thread taken between the
    // two cannot let the worker dim first and the held-up attempt pass.
    if case.worker_stall {
        rig.stall_next_control(STALL);
    }
    if !case.keep_cap {
        *revision += 1;
        // The time inside `publish_cap` is not counted as a test-thread
        // gap (`mark` follows it): a slow publish ends in a false fail,
        // never a hidden one.
        clock.send();
        publish_cap(publisher, *revision, 12);
        clock.mark();
    }
    // The latched frame is dimmed toward the cap with no new turn frame,
    // and never blanked: the cap eases (#8615).
    let (issued, expires) = (
        original.permit.issued_at().as_micros(),
        original.permit.expires_at().as_micros(),
    );
    loop {
        let dimmed = {
            let writes = rig.writes.lock().unwrap();
            writes.frames[before..]
                .iter()
                .any(|frame| (1..peak).contains(&cap_peak(frame)))
        };
        if dimmed {
            break;
        }
        // The cap eases over 200 ms, so the dim comes while the cue is
        // still displayed under its own permit, not at once.
        if monotonic_us() >= expires {
            return Err(Undimmed {
                said: format!(
                    "tightening did not dim the latched frame before its permit ended: {peak} then {:?}",
                    rig.writes.lock().unwrap().frames[before..]
                        .iter()
                        .map(cap_peak)
                        .collect::<Vec<_>>()
                ),
                held: held_up(rig, &clock, issued, expires),
                original: Box::new(original),
            });
        }
        thread::sleep(Duration::from_millis(1));
    }
    let dimmed_at = Instant::now();
    renew(controller);
    assert!(
        !rig.writes.lock().unwrap().frames[before..].contains(&EncodedFrame::black()),
        "a tighter cap blanked the ring"
    );
    // Once the ease is over, a new bright frame is under the new cap: past
    // the latched frame's own end, and 250 ms after the dim began.
    wait_expiry(rig, original, "the latched turn frame");
    thread::sleep(Duration::from_millis(250).saturating_sub(dimmed_at.elapsed()));
    rig.drain();
    let mut bright = frame(controller, owner, 200_000);
    bright.ceiling = 120;
    present(rig, bright);
    assert!(cap_peak(rig.writes.lock().unwrap().frames.last().unwrap()) <= 12);
    Ok(())
}

#[test]
fn output_caps_clamp_resting_frames_and_preserve_the_persons_light_off_setting() {
    let (directory, publisher, caps) = cap_input(12);
    let bus = lamp_bus::Bus::new(&directory.path).unwrap();
    let mut person = bus
        .publisher_declared(
            lamp_msgs::Light::TOPICS[0],
            &lamp_msgs::Light::declaration(),
            "actions",
        )
        .unwrap();
    person
        .publish_json(
            monotonic_us(),
            &lamp_msgs::Light::new(true, "white", 5).unwrap(),
        )
        .unwrap();
    let light = lamp_voice::ring_light::RingLight::on(&directory.path).unwrap();
    let mut rig = Rig::start_full(
        &[],
        false,
        Some(Senses {
            caps: Some(caps),
            light: Some(light),
            ..Senses::default()
        }),
        Some(40),
    );
    rig.ready();
    let mut controller = Controller::new(rig.boot, now());
    controller
        .set_microphone_permission(now(), Permission::Allowed)
        .unwrap();
    let until = now().checked_add(900_000).unwrap();
    controller
        .set_capture(now(), CaptureState::RetainingUntil(until))
        .unwrap();
    controller
        .set_admission(now(), AdmissionState::OpenUntil(until))
        .unwrap();
    let began = Instant::now();
    loop {
        rig.control(Control::Authority {
            snapshot: controller.snapshot(now()).unwrap(),
        });
        if began.elapsed() >= Duration::from_millis(150) && rig.colored() >= 4 {
            break;
        }
        assert!(began.elapsed() < Duration::from_millis(500));
        thread::sleep(Duration::from_millis(5));
    }
    assert!(
        rig.writes
            .lock()
            .unwrap()
            .frames
            .iter()
            .all(|frame| cap_peak(frame) <= 12)
    );
    person
        .publish_json(
            monotonic_us(),
            &lamp_msgs::Light::new(false, "white", 5).unwrap(),
        )
        .unwrap();
    let off = Instant::now();
    loop {
        // The existing face eases toward the ember, including fractional
        // pixel rounding. Keep input genuinely ready while it settles.
        let until = now().checked_add(900_000).unwrap();
        controller
            .set_capture(now(), CaptureState::RetainingUntil(until))
            .unwrap();
        controller
            .set_admission(now(), AdmissionState::OpenUntil(until))
            .unwrap();
        rig.control(Control::Authority {
            snapshot: controller.snapshot(now()).unwrap(),
        });
        let person_is_off = bus
            .latest(lamp_voice::ring_light::DRAWN_TOPIC)
            .unwrap()
            .is_some_and(|message| !message.parse::<lamp_msgs::Light>().unwrap().on);
        let peak = cap_peak(rig.writes.lock().unwrap().frames.last().unwrap());
        if person_is_off && (1..=3).contains(&peak) {
            break;
        }
        assert!(
            off.elapsed() < Duration::from_secs(1),
            "light-off ember did not settle: peak {peak}"
        );
        thread::sleep(Duration::from_millis(5));
    }
    // Existing light-off behavior is the three-step readiness ember, not a
    // request to turn the person's full light on when a cap is present.
    assert!(cap_peak(rig.writes.lock().unwrap().frames.last().unwrap()) <= 3);
    rig.control(Control::Stop);
    rig.finish().unwrap();
    drop((person, publisher));
    std::fs::remove_dir_all(&directory.path).unwrap();
}

#[test]
fn startup_stop_and_no_turn_never_invent_readiness_color() {
    let mut rig = Rig::start(&[]);
    rig.ready();
    let mut controller = Controller::new(rig.boot, now());
    let snapshot = controller.snapshot(now()).unwrap();
    assert_eq!(RingPhase::from_snapshot(snapshot), None);
    rig.control(Control::Authority { snapshot });
    rig.control(Control::Stop);
    assert!(matches!(
        rig.event(),
        WorkerEvent::Ring {
            report: RingFeedback::Blanked {
                reason: RingBlankReason::Shutdown,
                ..
            }
        }
    ));
    rig.finish().unwrap();
    assert_eq!(rig.colored(), 0);
}

#[test]
fn late_startup_black_write_is_reported_once_on_stderr() {
    // No other test's socket is copied into these children (`spawns`).
    let _spawning = crate::spawns::spawning();
    // The report is on stderr, so the scenario runs in a child copy of this
    // test binary and the parent reads what it printed.
    const CHILD: &str = "LAMP_RING_LATE_STARTUP_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let mut rig = Rig::start_with(&[], true);
        rig.ready();
        rig.control(Control::Stop);
        assert!(matches!(
            rig.event(),
            WorkerEvent::Ring {
                report: RingFeedback::Blanked {
                    reason: RingBlankReason::Shutdown,
                    ..
                }
            }
        ));
        rig.finish().unwrap();
        return;
    }
    // Its name as the test binary lists it: under this module's path in
    // the one binary of `lamp-voice`'s tests (#1806), bare at a crate root.
    let name = "late_startup_black_write_is_reported_once_on_stderr";
    let name = match module_path!().split_once("::") {
        Some((_, module)) => format!("{module}::{name}"),
        None => name.to_owned(),
    };
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &name, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "child failed: {stderr}");
    assert_eq!(
        stderr.matches("lamp-voice ring: SPI write ").count(),
        1,
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains("lamp-voice ring: SPI write 13180 us, over 10000 us, late for 13 ms"),
        "stderr: {stderr}"
    );
}

/// #1620: an SD/MMC re-tune, 0.3 s of late writes in a row, is late frames
/// kept, and the cues go on.
#[test]
fn an_sd_mmc_retune_of_late_writes_is_kept_and_the_ring_goes_on() {
    let mut rig = Rig::start(&[]);
    rig.ready();
    rig.writes.lock().unwrap().late_at = (2..=16).map(|call| (call, 20_000)).collect();
    let (mut controller, owner) = conversation(rig.boot);
    for _ in 0..16 {
        present(&rig, frame(&mut controller, owner, 200_000));
    }
    assert_eq!(rig.colored(), 16);
    rig.control(Control::Stop);
    assert!(matches!(
        rig.event(),
        WorkerEvent::Ring {
            report: RingFeedback::Blanked {
                reason: RingBlankReason::Shutdown,
                ..
            }
        }
    ));
    rig.finish().unwrap();
}

/// #1620: a run of late writes lasting 2 s ends the worker with a fault;
/// the one the voice starts after it writes its black first, and then the
/// state's light again.
#[test]
fn a_late_run_past_two_seconds_faults_and_a_ring_started_again_writes_black_then_the_light() {
    let mut rig = Rig::start(&[]);
    rig.ready();
    // A late write, then one held 2.1 s: the run lasts past 2 s.
    rig.writes.lock().unwrap().late_at = vec![(2, 13_180), (3, 2_100_000), (4, 13_180)];
    let (mut controller, owner) = conversation(rig.boot);
    present(&rig, frame(&mut controller, owner, 200_000));
    let second = frame(&mut controller, owner, 200_000);
    rig.control(Control::Authority {
        snapshot: second.snapshot,
    });
    rig.frame(second);
    let WorkerEvent::Fault { code } = rig.event() else {
        panic!("expected the ring's fault");
    };
    assert!(code.contains("late for 2100 ms"), "fault: {code}");
    assert!(rig.finish().is_err());
    // The restart: a fresh worker on the same strip.
    let mut again = Rig::start(&[]);
    again.ready();
    let (mut controller, owner) = conversation(again.boot);
    present(&again, frame(&mut controller, owner, 200_000));
    let frames = again.writes.lock().unwrap().frames.clone();
    assert_eq!(frames[0], EncodedFrame::black());
    assert_ne!(frames[1], EncodedFrame::black());
    again.control(Control::Stop);
    assert!(matches!(
        again.event(),
        WorkerEvent::Ring {
            report: RingFeedback::Blanked {
                reason: RingBlankReason::Shutdown,
                ..
            }
        }
    ));
    again.finish().unwrap();
}

#[test]
fn cancellation_arriving_after_real_data_receive_prevents_any_color() {
    let mut rig = Rig::start(&[]);
    rig.ready();
    let (mut controller, owner) = conversation(rig.boot);
    let original = frame(&mut controller, owner, 200_000);
    rig.control(Control::Authority {
        snapshot: original.snapshot,
    });
    controller.cancel_turn(now(), owner).unwrap();
    rig.after_receive
        .lock()
        .unwrap()
        .push_back(Control::Authority {
            snapshot: controller.snapshot(now()).unwrap(),
        });
    rig.frame(original);
    assert!(matches!(
        rig.event(),
        WorkerEvent::Ring {
            report: RingFeedback::Rejected {
                reason: RingRejection::StaleOwner,
                ..
            }
        }
    ));
    rig.control(Control::Stop);
    rig.finish().unwrap();
    assert_eq!(rig.colored(), 0);
}

#[test]
fn exhausted_shared_control_budget_defers_pending_frame_until_revocation() {
    let mut rig = Rig::start(&[]);
    rig.ready();
    let (mut controller, owner) = conversation(rig.boot);
    let original = frame(&mut controller, owner, 200_000);
    rig.control(Control::Authority {
        snapshot: original.snapshot,
    });
    controller.cancel_turn(now(), owner).unwrap();
    let revoked = controller.snapshot(now()).unwrap();
    {
        let mut controls = rig.after_receive.lock().unwrap();
        for _ in 0..16 {
            controls.push_back(Control::Authority {
                snapshot: original.snapshot,
            });
        }
        controls.push_back(Control::Authority { snapshot: revoked });
    }
    rig.frame(original);
    assert!(matches!(
        rig.event(),
        WorkerEvent::Ring {
            report: RingFeedback::Rejected {
                reason: RingRejection::StaleOwner,
                ..
            }
        }
    ));
    rig.control(Control::Stop);
    rig.finish().unwrap();
    assert_eq!(rig.colored(), 0);
}

#[test]
fn heartbeat_overtaking_data_keeps_original_request_and_permit() {
    let mut rig = Rig::start(&[]);
    rig.ready();
    let (mut controller, owner) = conversation(rig.boot);
    let original = frame(&mut controller, owner, 200_000);
    let newer = controller.snapshot(now()).unwrap();
    rig.control(Control::Authority { snapshot: newer });
    rig.frame(original);
    assert!(
        matches!(rig.event(), WorkerEvent::Ring { report: RingFeedback::Presented { requested_at_us, .. } } if requested_at_us == original.requested_at_us)
    );
    rig.control(Control::Stop);
    rig.finish().unwrap();
    assert_eq!(rig.colored(), 1);
}

#[test]
fn stale_owner_or_phase_cannot_blank_a_newer_valid_cue() {
    let mut rig = Rig::start(&[]);
    rig.ready();
    let (mut controller, owner) = conversation(rig.boot);
    let listening = frame(&mut controller, owner, 200_000);
    controller.input_ended(now(), owner).unwrap();
    let waiting = frame(&mut controller, owner, 200_000);
    present(&rig, waiting);
    let count = rig.writes.lock().unwrap().frames.len();
    rig.control(Control::Authority {
        snapshot: listening.snapshot,
    });
    rig.frame(listening);
    assert!(matches!(
        rig.event(),
        WorkerEvent::Ring {
            report: RingFeedback::Rejected {
                reason: RingRejection::PhaseMismatch,
                ..
            }
        }
    ));
    assert_eq!(rig.writes.lock().unwrap().frames.len(), count);
    controller.cancel_turn(now(), owner).unwrap();
    let successor = controller
        .admit(now(), AdmittedInput::Interruption)
        .unwrap();
    let next = frame(&mut controller, successor, 200_000);
    rig.control(Control::Authority {
        snapshot: next.snapshot,
    });
    // The old cue fades out (#8615), then is black.
    assert!(matches!(
        rig.event(),
        WorkerEvent::Ring {
            report: RingFeedback::Blanked { .. }
        }
    ));
    let next = frame(&mut controller, successor, 200_000);
    rig.control(Control::Authority {
        snapshot: next.snapshot,
    });
    rig.frame(next);
    assert!(matches!(
        rig.event(),
        WorkerEvent::Ring {
            report: RingFeedback::Presented { .. }
        }
    ));
    let count = rig.writes.lock().unwrap().frames.len();
    // Asked for again now: the fade above outlasts a request's 100 ms.
    rig.frame(RingFrame {
        requested_at_us: monotonic_us(),
        ..waiting
    });
    assert!(matches!(
        rig.event(),
        WorkerEvent::Ring {
            report: RingFeedback::Rejected {
                reason: RingRejection::StaleOwner,
                ..
            }
        }
    ));
    assert_eq!(rig.writes.lock().unwrap().frames.len(), count);
    rig.control(Control::Stop);
    rig.finish().unwrap();
}

#[test]
fn permit_expires_without_another_frame_then_stop_remains_available() {
    let mut rig = Rig::start(&[]);
    rig.ready();
    let (mut controller, owner) = conversation(rig.boot);
    present(&rig, frame(&mut controller, owner, 50_000));
    assert!(matches!(
        rig.event(),
        WorkerEvent::Ring {
            report: RingFeedback::Blanked {
                reason: RingBlankReason::AuthorityLost {
                    reason: RingRejection::ExpiredPermit
                },
                ..
            }
        }
    ));
    rig.control(Control::Stop);
    rig.finish().unwrap();
    // The cue, then its fade (#8615): each lit frame dimmer, then black,
    // and the stop's black.
    let peaks: Vec<u8> = rig.writes.lock().unwrap().frames[1..]
        .iter()
        .map(cap_peak)
        .collect();
    assert!(peaks.len() > 3, "{peaks:?}");
    assert_eq!(
        peaks.iter().filter(|peak| **peak == 0).count(),
        2,
        "{peaks:?}"
    );
    assert!(
        peaks[..peaks.len() - 2]
            .windows(2)
            .all(|pair| pair[1] <= pair[0])
            && peaks[peaks.len() - 3] < peaks[0],
        "{peaks:?}"
    );
}

/// Stale repeats never renew the authority. When it runs out the cue is
/// blanked, and the ring waits dark, alive, for a fresh one, then paints
/// again (#1178: a voice whose loop was held up a moment no longer ends).
#[test]
fn repeated_stale_control_does_not_renew_authority_and_a_ring_run_out_waits_for_a_fresh_one() {
    let mut rig = Rig::start(&[]);
    rig.ready();
    let (mut controller, owner) = conversation(rig.boot);
    let original = frame(&mut controller, owner, 200_000);
    present(&rig, original);
    for _ in 0..8 {
        rig.control(Control::Authority {
            snapshot: original.snapshot,
        });
    }
    assert!(matches!(
        rig.event(),
        WorkerEvent::Ring {
            report: RingFeedback::Blanked {
                reason: RingBlankReason::AuthorityLost { .. },
                ..
            }
        }
    ));
    // Alive and dark well past it: no lit frame after the cue and its fade.
    let lit = rig.colored();
    thread::sleep(Duration::from_millis(300));
    assert!(rig.completion.try_recv().is_err(), "the ring went on");
    assert_eq!(rig.colored(), lit);
    assert_eq!(
        rig.writes.lock().unwrap().frames.last(),
        Some(&EncodedFrame::black())
    );
    // A fresh authority: it paints again.
    present(&rig, frame(&mut controller, owner, 200_000));
    assert_eq!(rig.colored(), lit + 1);
    rig.control(Control::Stop);
    rig.finish().unwrap();
}

#[test]
fn newer_expired_privacy_revocation_blanks_before_old_allowed_lease_expires() {
    let mut rig = Rig::start(&[]);
    rig.ready();
    let (mut controller, owner) = conversation(rig.boot);
    let allowed = frame(&mut controller, owner, 200_000);
    present(&rig, allowed);
    controller
        .set_microphone_permission(now(), Permission::Denied)
        .unwrap();
    let denied = controller.snapshot(now()).unwrap();
    // A well-shaped short successor lease can expire in transit while an
    // already-installed longer allowed snapshot is still fresh.
    let mut wire = serde_json::to_value(denied).unwrap();
    wire["expires_at"] = (denied.issued_at().as_micros() + 1).into();
    let expired = serde_json::from_value(wire).unwrap();
    rig.control(Control::Authority { snapshot: expired });
    // Blanked at once, while the older allowed lease is still fresh.
    let WorkerEvent::Ring {
        report:
            RingFeedback::Blanked {
                reason:
                    RingBlankReason::AuthorityLost {
                        reason: RingRejection::ExpiredState,
                    },
                write_finished_at_us,
                ..
            },
    } = rig.event()
    else {
        panic!("missing the blank")
    };
    assert!(write_finished_at_us < allowed.snapshot.expires_at().as_micros());
    // Nothing is painted under the older lease after it (#1178: the ring
    // goes on, dark, until a newer authority installs).
    let mut again = allowed;
    again.requested_at_us = monotonic_us();
    rig.frame(again);
    assert!(matches!(
        rig.event(),
        WorkerEvent::Ring {
            report: RingFeedback::Rejected {
                reason: RingRejection::ExpiredState,
                ..
            }
        }
    ));
    assert_eq!(rig.colored(), 1);
    assert_eq!(rig.writes.lock().unwrap().frames.len(), 3);
    assert_eq!(
        rig.writes.lock().unwrap().frames.last(),
        Some(&EncodedFrame::black())
    );
    rig.control(Control::Stop);
    rig.finish().unwrap();
}

/// #1182's review: newer authorities that came late blank the ring once,
/// however many follow; a frame under the older lease is refused; and a
/// fresh, newer authority brings the light back.
#[test]
fn late_authorities_blank_once_and_a_fresh_one_brings_the_light_back() {
    let mut rig = Rig::start(&[]);
    rig.ready();
    let (mut controller, owner) = conversation(rig.boot);
    let first = frame(&mut controller, owner, 200_000);
    present(&rig, first);
    let late = |controller: &mut Controller| {
        let snapshot = controller.snapshot(now()).unwrap();
        let mut wire = serde_json::to_value(snapshot).unwrap();
        wire["expires_at"] = (snapshot.issued_at().as_micros() + 1).into();
        serde_json::from_value(wire).unwrap()
    };
    for _ in 0..2 {
        rig.control(Control::Authority {
            snapshot: late(&mut controller),
        });
    }
    assert!(matches!(
        rig.event(),
        WorkerEvent::Ring {
            report: RingFeedback::Blanked {
                reason: RingBlankReason::AuthorityLost {
                    reason: RingRejection::ExpiredState
                },
                ..
            }
        }
    ));
    let mut again = first;
    again.requested_at_us = monotonic_us();
    rig.frame(again);
    // The next report is the refusal: the second late one wrote no blank.
    assert!(matches!(
        rig.event(),
        WorkerEvent::Ring {
            report: RingFeedback::Rejected {
                reason: RingRejection::ExpiredState,
                ..
            }
        }
    ));
    assert_eq!(rig.writes.lock().unwrap().frames.len(), 3);
    present(&rig, frame(&mut controller, owner, 200_000));
    assert_eq!(rig.colored(), 2);
    rig.control(Control::Stop);
    rig.finish().unwrap();
}

#[test]
fn genuinely_older_expired_control_cannot_clear_a_newer_valid_cue() {
    let mut rig = Rig::start(&[]);
    rig.ready();
    let (mut controller, owner) = conversation(rig.boot);
    let old = controller.snapshot(now()).unwrap();
    let mut wire = serde_json::to_value(old).unwrap();
    wire["expires_at"] = (old.issued_at().as_micros() + 1).into();
    let old_expired = serde_json::from_value(wire).unwrap();
    let current = frame(&mut controller, owner, 200_000);
    present(&rig, current);
    rig.control(Control::Authority {
        snapshot: old_expired,
    });
    rig.frame(current);
    assert!(matches!(
        rig.event(),
        WorkerEvent::Ring {
            report: RingFeedback::Presented { .. }
        }
    ));
    assert_eq!(rig.colored(), 2);
    assert_eq!(rig.writes.lock().unwrap().frames.len(), 3);
    rig.control(Control::Stop);
    rig.finish().unwrap();
}

#[test]
fn invalid_control_kind_or_malformed_frame_blanks_and_exits() {
    for invalid_control in [true, false] {
        let mut rig = Rig::start(&[]);
        rig.ready();
        let (mut controller, owner) = conversation(rig.boot);
        present(&rig, frame(&mut controller, owner, 200_000));
        if invalid_control {
            rig.control(Control::StartCapture);
        } else {
            rig.parent
                .lock()
                .unwrap()
                .data
                .send(serde_json::json!({"phase":"off"}))
                .unwrap();
        }
        assert!(rig.finish().is_err());
        assert_eq!(rig.colored(), 1);
        assert_eq!(rig.writes.lock().unwrap().frames.len(), 3);
        assert_eq!(
            rig.writes.lock().unwrap().frames.last(),
            Some(&EncodedFrame::black())
        );
    }
}

#[test]
fn future_data_snapshot_never_installs_or_renews_authority() {
    let mut rig = Rig::start(&[]);
    rig.ready();
    let (mut controller, owner) = conversation(rig.boot);
    let original = frame(&mut controller, owner, 200_000);
    rig.frame(original);
    assert!(matches!(
        rig.event(),
        WorkerEvent::Ring {
            report: RingFeedback::Rejected {
                reason: RingRejection::ExpiredRequest,
                ..
            }
        }
    ));
    // No authority ever installed: past the start's allowance the ring ends.
    assert!(
        rig.finish()
            .unwrap_err()
            .to_string()
            .contains("authority never came")
    );
    assert_eq!(rig.colored(), 0);
}

#[test]
fn invalid_frame_bounds_reject_without_a_write_and_speaking_requires_playback() {
    let mut rig = Rig::start(&[]);
    rig.ready();
    let (mut controller, owner) = conversation(rig.boot);
    let original = frame(&mut controller, owner, 200_000);
    present(&rig, original);
    let count = rig.writes.lock().unwrap().frames.len();
    let cases = [
        (
            RingFrame {
                ceiling: 121,
                ..original
            },
            RingRejection::InvalidCeiling,
        ),
        (
            RingFrame {
                requested_at_us: u64::MAX,
                ..original
            },
            RingRejection::FutureRequest,
        ),
        (
            RingFrame {
                phase: RingPhase::Speaking,
                ..original
            },
            RingRejection::PhaseMismatch,
        ),
    ];
    for (invalid, expected) in cases {
        rig.frame(invalid);
        assert!(
            matches!(rig.event(), WorkerEvent::Ring { report: RingFeedback::Rejected { reason, .. } } if reason == expected)
        );
    }
    assert_eq!(rig.writes.lock().unwrap().frames.len(), count);
    controller.input_ended(now(), owner).unwrap();
    let plan = controller
        .plan_output(now(), owner, OutputKind::Speech, None)
        .unwrap();
    let speech = controller.issue_output(now(), plan, 200_000).unwrap();
    controller.playback_started(now(), speech).unwrap();
    let speaking = frame(&mut controller, owner, 200_000);
    assert_eq!(speaking.phase, RingPhase::Speaking);
    rig.control(Control::Authority {
        snapshot: speaking.snapshot,
    });
    // The listening cue fades out (#8615), then is black.
    assert!(matches!(
        rig.event(),
        WorkerEvent::Ring {
            report: RingFeedback::Blanked { .. }
        }
    ));
    let speaking = frame(&mut controller, owner, 200_000);
    rig.control(Control::Authority {
        snapshot: speaking.snapshot,
    });
    rig.frame(speaking);
    assert!(matches!(
        rig.event(),
        WorkerEvent::Ring {
            report: RingFeedback::Presented {
                phase: RingPhase::Speaking,
                ..
            }
        }
    ));
    rig.control(Control::Stop);
    rig.finish().unwrap();
}

#[test]
fn malformed_control_blanks_and_reports_cleanup_failure_without_retry() {
    let mut rig = Rig::start(&[3]);
    rig.ready();
    let (mut controller, owner) = conversation(rig.boot);
    present(&rig, frame(&mut controller, owner, 200_000));
    rig.parent
        .lock()
        .unwrap()
        .control
        .send(serde_json::json!({"kind":"stop","extra":1}))
        .unwrap();
    let error = rig.finish().unwrap_err().to_string();
    assert!(error.contains("malformed runtime message"));
    assert!(error.contains("injected write 3"));
    assert_eq!(rig.writes.lock().unwrap().frames.len(), 3);
    assert!(matches!(rig.event(), WorkerEvent::Fault { code } if code.contains("cleanup")));
}

#[test]
fn primary_and_cleanup_write_errors_are_both_reported_once() {
    let mut rig = Rig::start(&[2, 3]);
    rig.ready();
    let (mut controller, owner) = conversation(rig.boot);
    let original = frame(&mut controller, owner, 200_000);
    rig.control(Control::Authority {
        snapshot: original.snapshot,
    });
    rig.frame(original);
    let error = rig.finish().unwrap_err().to_string();
    assert!(error.contains("injected write 2"));
    assert!(error.contains("injected write 3"));
    assert_eq!(rig.writes.lock().unwrap().frames.len(), 3);
    assert!(matches!(rig.event(), WorkerEvent::Fault { .. }));
}

#[test]
fn startup_failure_never_reports_ready_and_stop_failure_is_visible() {
    let mut failed = Rig::start(&[1]);
    assert!(matches!(failed.event(), WorkerEvent::Fault { .. }));
    assert!(failed.finish().is_err());
    assert_eq!(failed.writes.lock().unwrap().frames.len(), 1);
    let mut stopped = Rig::start(&[2]);
    stopped.ready();
    stopped.control(Control::Stop);
    assert!(
        stopped
            .finish()
            .unwrap_err()
            .to_string()
            .contains("injected write 2")
    );
    assert_eq!(stopped.writes.lock().unwrap().frames.len(), 2);
}

#[test]
fn ring_wire_rejects_unknown_commands_and_unknown_nested_fields() {
    let (mut controller, owner) = conversation(new_boot().unwrap());
    let original = frame(&mut controller, owner, 200_000);
    let mut value = serde_json::to_value(original).unwrap();
    value["extra"] = true.into();
    assert!(decode::<RingFrame>(&encode(&value).unwrap()).is_err());
    value.as_object_mut().unwrap().remove("extra");
    value["phase"] = "off".into();
    assert!(decode::<RingFrame>(&encode(&value).unwrap()).is_err());
    value["phase"] = "listening".into();
    value["snapshot"]["extra"] = true.into();
    assert!(decode::<RingFrame>(&encode(&value).unwrap()).is_err());
    assert!(decode::<RingFeedback>(br#"{"kind":"blanked","reason":{"kind":"shutdown","extra":true},"write_started_at_us":1,"write_finished_at_us":2}"#).is_err());
}

#[test]
fn local_socket_request_to_fake_sink_latency_is_measured_without_physical_claim() {
    let mut rig = Rig::start(&[]);
    rig.ready();
    let (mut controller, owner) = conversation(rig.boot);
    let mut delays = Vec::new();
    for _ in 0..40 {
        let frame = frame(&mut controller, owner, 200_000);
        rig.control(Control::Authority {
            snapshot: frame.snapshot,
        });
        rig.frame(frame);
        let WorkerEvent::Ring {
            report:
                RingFeedback::Presented {
                    requested_at_us,
                    write_finished_at_us,
                    ..
                },
        } = rig.event()
        else {
            panic!("missing present receipt")
        };
        delays.push(write_finished_at_us - requested_at_us);
    }
    rig.control(Control::Stop);
    rig.finish().unwrap();
    delays.sort_unstable();
    let p95 = delays[(delays.len() * 95).div_ceil(100) - 1];
    println!(
        "{{\"scope\":\"local_unix_sockets_fake_sink_only\",\"samples\":{},\"p50_us\":{},\"p95_us\":{p95},\"max_us\":{},\"target_us\":50000,\"target_met\":{}}}",
        delays.len(),
        delays[19],
        delays[39],
        p95 <= 50_000
    );
}

/// A tap on the head as `--touch` Controls says it (#711).
fn head_tap() -> lamp_msgs::TouchEvent {
    use lamp_msgs::{ClockStamp, TouchEvent, TouchGesture, TouchIntent, TouchSource};
    let at = monotonic_us();
    let event = TouchEvent {
        stamp: ClockStamp {
            clock_id: "boot".into(),
            at_us: at,
            sequence: 1,
        },
        session_id: "controls-one".into(),
        reader_session_id: "gpio".into(),
        enabled: true,
        source: TouchSource::Head,
        name: "head".into(),
        contact_id: 1,
        gesture: TouchGesture::Tap,
        when_speaking: TouchIntent::Attend,
        when_quiet: TouchIntent::Attend,
        when_asleep: Some(TouchIntent::Wake),
        began_us: at - 350_000,
        observed_us: at,
        released_us: Some(at - 150_000),
        active_mask: 0,
        active_names: vec![],
        position: None,
        toward_pad: None,
        distance: 0.0,
        speed: 0.0,
    };
    event.check().unwrap();
    event
}

/// Controls' own recognizer for the head (#711): the head settled, then a
/// hand on it, stepped every 10 ms of this host's clock. Its states are the
/// ones `--touch` Controls publishes on `controls/touch_state`, each fresh.
struct HeldHead {
    engine: lamp_controls::touch::TouchEngine,
    next_us: u64,
    sequence: u64,
    state: Option<lamp_msgs::TouchState>,
}

impl HeldHead {
    fn new() -> Self {
        let config = lamp_controls::touch::TouchConfig {
            enabled: true,
            head_pads: vec!["head-1".into()],
            ..lamp_controls::touch::TouchConfig::default()
        };
        let now = monotonic_us();
        let mut head = Self {
            engine: lamp_controls::touch::TouchEngine::new(config, "boot", "controls").unwrap(),
            next_us: now - 600_000,
            sequence: 0,
            state: None,
        };
        head.run(now - 10_000, false);
        head
    }

    fn run(&mut self, until_us: u64, on: bool) {
        while self.next_us <= until_us {
            let at = self.next_us;
            self.sequence += 1;
            let sample = lamp_controls::touch::GpioSample {
                stamp: lamp_msgs::ClockStamp {
                    clock_id: "boot".into(),
                    at_us: at,
                    sequence: self.sequence,
                },
                session_id: "gpio".into(),
                primary: None,
                reset: None,
                head_pads: vec![lamp_controls::input::Level {
                    name: "head-1".into(),
                    active: on,
                }],
            };
            self.engine.ingest_gpio(at, sample).unwrap();
            self.state = Some(self.engine.tick(at).state);
            self.next_us += 10_000;
        }
    }

    /// The state with the hand on the head now: its newest 10 ms step.
    fn now(&mut self) -> &lamp_msgs::TouchState {
        self.run(monotonic_us(), true);
        self.state.as_ref().unwrap()
    }
}

/// Senders flooding `controls/touch_state`, the head's topic since #1469,
/// until dropped, or at most 5 s: on a failed assertion too they are
/// stopped and joined, never left running.
struct Flood {
    flooding: Arc<std::sync::atomic::AtomicBool>,
    sent: Arc<std::sync::atomic::AtomicU64>,
    senders: Vec<thread::JoinHandle<()>>,
}

impl Flood {
    /// Four senders to the topic's kept message, through the bus, as fast
    /// as they go: two of Controls' states with a hand held, which the head
    /// takes; one of the same states from another source, and one from
    /// Controls that is another type, which it refuses.
    fn start(root: &std::path::Path) -> Self {
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        let flooding = Arc::new(AtomicBool::new(true));
        let sent = Arc::new(AtomicU64::new(0));
        // Only if `stop` is never reached: the test may wait 4 s for the
        // flood on a slow host, and the head's look and the ring's run
        // come after, still in the flood (#1569 row 24).
        let deadline = Instant::now() + Duration::from_secs(10);
        let senders = (0..4)
            .map(|number| {
                let (root, flooding, sent) = (root.to_path_buf(), flooding.clone(), sent.clone());
                thread::spawn(move || {
                    let going = || flooding.load(Ordering::Acquire) && Instant::now() < deadline;
                    let bus = lamp_bus::Bus::new(&root).unwrap();
                    let mut out = bus
                        .publisher_declared(
                            lamp_msgs::TouchState::TOPIC,
                            &lamp_msgs::TouchState::declaration(),
                            if number == 2 { "replay" } else { "controls" },
                        )
                        .unwrap();
                    let mut head = HeldHead::new();
                    let tap = head_tap();
                    while going() {
                        let published = if number == 3 {
                            out.publish_json(1, &tap)
                        } else {
                            out.publish_json(1, head.now())
                        };
                        if published.is_ok() {
                            sent.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                })
            })
            .collect();
        Self {
            flooding,
            sent,
            senders,
        }
    }

    /// Whether it was still flooding, and stop it.
    fn stop(&mut self) -> bool {
        let was = self
            .flooding
            .swap(false, std::sync::atomic::Ordering::AcqRel);
        for sender in self.senders.drain(..) {
            let _ = sender.join();
        }
        was
    }
}

impl Drop for Flood {
    fn drop(&mut self) {
        self.stop();
    }
}

/// #849 review: head requests are optional input, read by the head's own
/// thread. Since #1469 that thread reads Controls' state, the newest kept
/// on `controls/touch_state`, not `controls/touch`, so the flood is on it.
/// From four senders, half of it what the head refuses (another source,
/// another type), it never holds back a permit's expiry or a Stop sent
/// while it goes on: both are measured during the flood. And the head still
/// finds the held hand in it (Third on #705, 19 October 03:36).
#[test]
fn a_flood_of_head_requests_never_holds_back_revocation_or_expiry() {
    let root = std::path::PathBuf::from(format!("/tmp/lrw-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let head = lamp_voice::ring_head::RingHead::on(&root).unwrap();
    // Reading the same bus as the ring's own head does.
    let mut looking = lamp_voice::ring_head::RingHead::on(&root).unwrap();
    // The flood is at full strength before the ring starts: 1,000 sent.
    // Counted after a fixed 100 ms instead, its two bus senders once sent
    // fewer on a loaded Mac, and `sent > 1_000` below failed (Fourth on
    // #705, 19 October 02:34; #1569 row 24). It is waited for here, not
    // after the ring is ready, which ends itself with no authority 250 ms
    // into its start (`INITIAL_AUTHORITY_US`).
    //
    // On Linux, where the lamp and the ARM64 run are, it gets there in
    // milliseconds, and must. On this Mac beside busy cores it may not: each
    // send writes and renames the topic's kept file, and four senders doing
    // that in one folder sent 24 to 661 in 4 s beside 8 busy cores, none
    // refused (#1569 row 24, 17 of 20 runs). There the bounds below are
    // measured under what it sent, and the test says so.
    let mut flood = Flood::start(&root);
    let started = Instant::now();
    let full = |flood: &Flood| flood.sent.load(std::sync::atomic::Ordering::Relaxed) >= 1_000;
    while !full(&flood) && started.elapsed() < Duration::from_secs(4) {
        thread::sleep(Duration::from_millis(1));
    }
    if !full(&flood) {
        if cfg!(target_os = "linux") {
            panic!("the flood sent fewer than 1,000 in 4 s");
        }
        eprintln!(
            "the flood sent {} in 4 s on this host: the bounds are measured under that",
            flood.sent.load(std::sync::atomic::Ordering::Relaxed)
        );
    }
    // The head is read, not only not in the way: a look of its whose newest
    // state is one of Controls' finds the held hand.
    let until = Instant::now() + Duration::from_secs(2);
    while looking.tick(monotonic_us()).is_none() {
        assert!(
            Instant::now() < until,
            "the head never found the held hand in the flood"
        );
        thread::sleep(Duration::from_millis(2));
    }
    drop(looking);
    let mut rig = Rig::start_with_senses(Senses {
        head: Some(head),
        ..Senses::default()
    });
    rig.ready();
    thread::sleep(Duration::from_millis(100));
    // A colored frame whose permit lasts 50 ms fades when it expires, then
    // is blanked.
    let (mut controller, owner) = conversation(rig.boot);
    let shown = Instant::now();
    present(&rig, frame(&mut controller, owner, 50_000));
    assert!(matches!(
        rig.event(),
        WorkerEvent::Ring {
            report: RingFeedback::Blanked {
                reason: RingBlankReason::AuthorityLost {
                    reason: RingRejection::ExpiredPermit
                },
                ..
            }
        }
    ));
    let expired_after = shown.elapsed();
    // A Stop sent later, while it goes on, ends the worker at once.
    thread::sleep(Duration::from_millis(10));
    let asked = Instant::now();
    rig.control(Control::Stop);
    rig.finish().unwrap();
    let stopped_after = asked.elapsed();
    let sent = flood.sent.load(std::sync::atomic::Ordering::Relaxed);
    let still_flooding = flood.stop();
    let _ = std::fs::remove_dir_all(&root);
    assert!(still_flooding);
    assert!(sent > 1_000 || !cfg!(target_os = "linux"), "{sent}");
    // Its fade (#8615) is the only time past the old bound.
    assert!(
        expired_after < Duration::from_millis(150) + Duration::from_micros(lamp_ring::FADE_US),
        "{expired_after:?}"
    );
    assert!(
        stopped_after < Duration::from_millis(50),
        "{stopped_after:?}"
    );
}

/// #1620: lamp-52e6 (`LAMP_RING_EYE=on`) failed a service stop on "no
/// verified black-write receipt"; lamp-4ace (`strong`) stopped cleanly. The
/// eye only draws the face: with no eye, the eye and the strong eye alike, a
/// ring resting under a listening authority, then given the stop's last
/// authority (both privacy permissions denied) and Stop, as the service's
/// stop sends them, writes black for the shutdown, says so, and exits clean.
#[test]
fn every_eye_mode_writes_and_says_the_shutdown_black_at_a_service_stop() {
    for (eye, eye_strong) in [(false, false), (true, false), (true, true)] {
        let mut rig = Rig::start_resting(
            Senses {
                eye,
                eye_strong,
                ..Senses::default()
            },
            24,
        );
        rig.ready();
        let mut controller = Controller::new(rig.boot, now());
        controller
            .set_microphone_permission(now(), Permission::Allowed)
            .unwrap();
        let until = now().checked_add(900_000).unwrap();
        controller
            .set_capture(now(), CaptureState::RetainingUntil(until))
            .unwrap();
        controller
            .set_admission(now(), AdmissionState::OpenUntil(until))
            .unwrap();
        rig.control(Control::Authority {
            snapshot: controller.snapshot(now()).unwrap(),
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while rig.colored() == 0 {
            assert!(
                Instant::now() < deadline,
                "no resting face (eye {eye}, strong {eye_strong})"
            );
            thread::sleep(Duration::from_millis(2));
        }
        controller
            .set_camera_permission(now(), Permission::Denied)
            .unwrap();
        controller
            .set_microphone_permission(now(), Permission::Denied)
            .unwrap();
        rig.control(Control::Authority {
            snapshot: controller.snapshot(now()).unwrap(),
        });
        rig.control(Control::Stop);
        loop {
            match rig.event() {
                WorkerEvent::Ring {
                    report:
                        RingFeedback::Blanked {
                            reason: RingBlankReason::Shutdown,
                            ..
                        },
                } => break,
                WorkerEvent::Ring {
                    report: RingFeedback::Blanked { .. },
                } => {}
                other => panic!("eye {eye}, strong {eye_strong}: {other:?}"),
            }
        }
        rig.finish()
            .unwrap_or_else(|error| panic!("eye {eye}, strong {eye_strong}: {error}"));
        assert_eq!(
            rig.writes.lock().unwrap().frames.last(),
            Some(&EncodedFrame::black())
        );
    }
}
