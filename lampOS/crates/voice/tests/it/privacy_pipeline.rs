//! Deterministic composition of the public privacy boundaries. Envelopes stand
//! in for Bus transport; clocks and GPIO/PCM endpoints have no real I/O. Ring's
//! injected-clock constructor is private, so its authorization stays covered
//! by the Ring crate's own deterministic tests.
use lamp_bus::{Header, Kind, Message};
use lamp_controls::{
    engine::{Config, Engine},
    pins::Lines,
    privacy::{LineReader, Monitor, Reader},
};
use lamp_ears::{
    BLOCK, Settings,
    privacy::{Capture, CaptureOwner, Emission},
};
use lamp_msgs::{ClockStamp, ControlsState, MicrophonePrivacy};
use lamp_turn_owner::{BootId, BoundaryGuard, Controller, MonoTime, Permission, Snapshot};
use lamp_voice::{
    privacy::{PrivacyGate, PrivacyState},
    privacy_bus::{CaptureLeaseGate, Observation, SwitchFence, fresh, grants, lease},
};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    io,
    rc::Rc,
};

const CLOCK: &str = "privacy-pipeline-kernel";
const CONTROLS: &str = "0123456789abcdef";
const VOICE: &str = "1023456789abcdef";
const NEXT_VOICE: &str = "2023456789abcdef";
const START: u64 = 1_000_000;
const FRESH_US: u64 = lamp_msgs::MICROPHONE_PRIVACY_FRESH_US;

thread_local! {
    static TIME: Cell<u64> = const { Cell::new(START) };
}

fn now() -> u64 {
    TIME.get()
}

fn clock() -> MonoTime {
    MonoTime::from_micros(now())
}

fn advance(at_us: u64) {
    TIME.set(at_us);
}

fn wall(at_us: u64) -> u64 {
    10_000 + at_us / 1_000
}

fn boot() -> BootId {
    BootId::new([1; 16]).unwrap()
}

fn envelope(header: Header, payload: Vec<u8>) -> Message {
    Message {
        header,
        payload,
        gap: 0,
        publisher_restarted: false,
        expired: false,
    }
}

struct Line(Rc<Cell<Option<bool>>>);

impl Lines for Line {
    fn high(&mut self, line: u32) -> io::Result<bool> {
        assert_eq!(line, 9);
        self.0
            .get()
            .ok_or_else(|| io::Error::other("injected missing GPIO observation"))
    }
}

struct Controls {
    high: Rc<Cell<Option<bool>>>,
    reader: LineReader<Line>,
    monitor: Monitor,
    engine: Engine,
    sequence: u64,
    /// `no-slide`: no line is read, and each state says so.
    no_slide: bool,
}

impl Controls {
    fn new() -> Self {
        let high = Rc::new(Cell::new(None));
        Self {
            reader: LineReader::new(Line(high.clone()), CLOCK, "privacy-slide-reader"),
            high,
            monitor: Monitor::new(CLOCK),
            engine: Engine::new(Config::default(), CLOCK, CONTROLS).unwrap(),
            sequence: 0,
            no_slide: false,
        }
    }

    fn no_slide() -> Self {
        Self {
            no_slide: true,
            ..Self::new()
        }
    }

    fn acquire(&mut self, at_us: u64, high: Option<bool>) {
        self.high.set(high);
        let sample = self
            .reader
            .read(at_us)
            .expect("fixture obeys GPIO polling interval");
        self.monitor.observe(at_us, sample).unwrap();
    }

    fn message(&mut self, published_us: u64) -> Message {
        // The production Controls runtime attaches this independent monitor to
        // its gesture state. An unavailable gesture source cannot invent HIGH.
        let mut state = self.engine.tick(published_us).state;
        if self.no_slide {
            state.no_privacy_slide = true;
        } else {
            state.privacy = Some(self.monitor.snapshot(published_us));
        }
        state.check().unwrap();
        self.sequence += 1;
        envelope(
            Header {
                topic: ControlsState::TOPIC.into(),
                kind: Kind::Latest,
                enc: lamp_bus::JSON.into(),
                type_name: Some(ControlsState::TYPE.into()),
                source: "controls".into(),
                boot: state.session_id.clone(),
                seq: self.sequence,
                at_us: published_us,
                at_unix_ms: wall(published_us),
                expires_ms: Some(300),
                clock: None,
            },
            serde_json::to_vec(&state).unwrap(),
        )
    }
}

struct Worker {
    session: &'static str,
    fence: SwitchFence,
    gate: CaptureLeaseGate,
    authority: BoundaryGuard,
    last: Option<Observation>,
    sequence: u64,
}

impl Worker {
    fn new(session: &'static str) -> Self {
        Self::with_fence(session, SwitchFence::new(CLOCK.into()))
    }

    fn no_slide(session: &'static str) -> Self {
        Self::with_fence(session, SwitchFence::no_slide(CLOCK.into()))
    }

    fn with_fence(session: &'static str, fence: SwitchFence) -> Self {
        Self {
            session,
            fence,
            gate: CaptureLeaseGate::default(),
            authority: BoundaryGuard::new(boot(), clock()),
            last: None,
            sequence: 0,
        }
    }

    fn accept(&mut self, message: &Message) -> io::Result<()> {
        if let Some(observed) = self.fence.observe(message, now(), wall(now()))? {
            let stamp = observed.stamp();
            self.gate
                .observe(stamp.sequence, stamp.at_us, now(), observed.muted()?);
            self.last = Some(observed);
        }
        Ok(())
    }

    fn message(&mut self, snapshot: Snapshot) -> Result<Message, lamp_turn_owner::Error> {
        self.authority.install(clock(), snapshot)?;
        let state = self.authority.state().unwrap();
        let physical = fresh(self.last.as_ref(), now());
        self.sequence += 1;
        // Only transport is local to the fixture. Permission, switch
        // ordering, authority admission, freshness, the grant, the lease's
        // fields and its validation use the production components.
        let lease = lease(
            ClockStamp {
                clock_id: CLOCK.into(),
                at_us: now(),
                sequence: self.sequence,
            },
            self.session,
            state.boot(),
            state,
            physical,
            grants(&self.gate, state, physical, true, now()),
        );
        lease.check().unwrap();
        Ok(envelope(
            Header {
                topic: MicrophonePrivacy::TOPIC.into(),
                kind: Kind::Latest,
                enc: lamp_bus::JSON.into(),
                type_name: Some(MicrophonePrivacy::TYPE.into()),
                source: "voice".into(),
                boot: self.session.into(),
                seq: self.sequence,
                at_us: now(),
                at_unix_ms: wall(now()),
                expires_ms: Some(50),
                clock: None,
            },
            serde_json::to_vec(&lease).unwrap(),
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CaptureEvent {
    Open,
    Read(usize),
    Discard,
    Drop,
}

#[derive(Default)]
struct Audio {
    events: Vec<(u64, CaptureEvent)>,
    discarded_samples: usize,
    dropped_samples: usize,
}

struct Device {
    audio: Rc<RefCell<Audio>>,
    limit: Rc<Cell<usize>>,
    buffered: VecDeque<i16>,
}

impl Capture for Device {
    fn read(&mut self, into: &mut [i16]) -> io::Result<usize> {
        let count = into.len().min(self.limit.get()).min(self.buffered.len());
        for sample in &mut into[..count] {
            *sample = self.buffered.pop_front().unwrap();
        }
        self.audio
            .borrow_mut()
            .events
            .push((now(), CaptureEvent::Read(count)));
        Ok(count)
    }

    fn discard(&mut self) -> io::Result<()> {
        let mut audio = self.audio.borrow_mut();
        audio.events.push((now(), CaptureEvent::Discard));
        audio.discarded_samples += self.buffered.len();
        self.buffered.clear();
        Ok(())
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        let mut audio = self.audio.borrow_mut();
        audio.events.push((now(), CaptureEvent::Drop));
        audio.dropped_samples += self.buffered.len();
    }
}

struct Pipeline {
    controls: Controls,
    worker: Worker,
    controller: Controller,
    ears: CaptureOwner<Device>,
    audio: Rc<RefCell<Audio>>,
    read_limit: Rc<Cell<usize>>,
}

impl Pipeline {
    fn new(permission: Permission) -> Self {
        advance(START);
        let mut controller = Controller::new(boot(), clock());
        controller
            .set_microphone_permission(clock(), permission)
            .unwrap();
        controller
            .set_camera_permission(clock(), Permission::Denied)
            .unwrap();
        Self {
            controls: Controls::new(),
            worker: Worker::new(VOICE),
            controller,
            ears: CaptureOwner::new(CLOCK.into(), Settings::default()),
            audio: Rc::new(RefCell::new(Audio::default())),
            read_limit: Rc::new(Cell::new(BLOCK)),
        }
    }

    fn sample(
        &mut self,
        acquired_us: u64,
        high: Option<bool>,
    ) -> io::Result<(Snapshot, MicrophonePrivacy)> {
        self.controls.acquire(acquired_us, high);
        let message = self.controls.message(acquired_us + 1_000);
        // Deliberately distinct acquisition, publication and receipt times.
        advance(acquired_us + 3_000);
        self.worker.accept(&message)?;
        Ok(self.forward())
    }

    fn forward(&mut self) -> (Snapshot, MicrophonePrivacy) {
        let state = self.controller.snapshot(clock()).unwrap();
        let message = self.worker.message(state).unwrap();
        let lease = MicrophonePrivacy::from_message(&message, CLOCK, now(), wall(now())).unwrap();
        self.ears.observe(&message, now(), wall(now())).unwrap();
        (state, lease)
    }

    fn tick(&mut self, limit: usize) -> Option<Emission> {
        self.read_limit.set(limit);
        let audio = self.audio.clone();
        let read_limit = self.read_limit.clone();
        self.ears
            .tick(
                now,
                || {
                    audio.borrow_mut().events.push((now(), CaptureEvent::Open));
                    Ok(Device {
                        audio: audio.clone(),
                        limit: read_limit.clone(),
                        buffered: std::iter::repeat_n(2_000, BLOCK * 2).collect(),
                    })
                },
                false,
            )
            .unwrap()
    }

    /// A lamp with no slide: Controls says so, and nothing is read.
    fn no_slide(permission: Permission) -> Self {
        let mut pipeline = Self::new(permission);
        pipeline.controls = Controls::no_slide();
        pipeline.worker = Worker::no_slide(VOICE);
        pipeline
    }

    /// One `controls/state` from a lamp with no slide, published at
    /// `published_us` and received 2 ms later.
    fn marker(&mut self, published_us: u64) -> io::Result<(Snapshot, MicrophonePrivacy)> {
        let message = self.controls.message(published_us);
        advance(published_us + 2_000);
        self.worker.accept(&message)?;
        Ok(self.forward())
    }

    fn stable_high(&mut self) -> (Snapshot, MicrophonePrivacy) {
        for offset in [0, 20_000, 40_000] {
            let (_, lease) = self.sample(START + offset, Some(true)).unwrap();
            assert!(!lease.permitted);
            assert!(self.tick(BLOCK).is_none());
        }
        self.sample(START + 60_000, Some(true)).unwrap()
    }
}

#[test]
fn stable_high_opens_then_low_discards_pcm_and_drops_the_actual_handle() {
    let mut pipeline = Pipeline::new(Permission::Allowed);
    let (_, high) = pipeline.stable_high();
    assert!(high.permitted);
    assert_eq!(high.observed_us, START + 60_000);
    assert_eq!(high.stamp.at_us, START + 63_000);
    assert!(pipeline.tick(127).is_none());
    assert!(pipeline.ears.is_open());

    advance(START + 65_000);
    pipeline
        .controller
        .set_microphone_permission(clock(), Permission::Denied)
        .unwrap();
    let (_, low) = pipeline.sample(now(), Some(false)).unwrap();
    assert!(!low.permitted);
    assert_eq!(low.physical_muted, Some(true));
    assert_eq!(low.observed_us, START + 65_000);
    assert!(!pipeline.ears.is_open());
    assert!(pipeline.tick(BLOCK).is_none());
    let audio = pipeline.audio.borrow();
    assert_eq!(
        audio.events,
        [
            (START + 63_000, CaptureEvent::Open),
            (START + 63_000, CaptureEvent::Read(127)),
            (START + 68_000, CaptureEvent::Discard),
            (START + 68_000, CaptureEvent::Drop),
        ]
    );
    assert_eq!(audio.discarded_samples, BLOCK * 2 - 127);
    assert_eq!(audio.dropped_samples, 0);
}

#[test]
fn low_then_high_needs_a_fresh_worker_and_restored_controller_generation() {
    let mut pipeline = Pipeline::new(Permission::Allowed);
    let (_, initial) = pipeline.stable_high();
    assert!(pipeline.tick(127).is_none());
    advance(START + 65_000);
    pipeline
        .controller
        .set_microphone_permission(clock(), Permission::Denied)
        .unwrap();
    let (_, low) = pipeline.sample(now(), Some(false)).unwrap();
    assert!(low.microphone_generation > initial.microphone_generation);
    for offset in [70_000, 90_000, 110_000, 130_000] {
        let (_, lease) = pipeline.sample(START + offset, Some(true)).unwrap();
        assert!(!lease.permitted);
        assert!(pipeline.tick(BLOCK).is_none());
    }
    pipeline
        .controller
        .set_microphone_permission(clock(), Permission::Allowed)
        .unwrap();
    let (_, same_worker) = pipeline.forward();
    assert!(same_worker.microphone_generation > low.microphone_generation);
    assert!(
        !same_worker.permitted,
        "an old worker cannot undo its LOW latch"
    );
    assert!(pipeline.tick(BLOCK).is_none());

    // The supervisor owns actual cleanup. This fixture explicitly creates the
    // next worker only after the old fake handle's Discard/Drop were observed.
    assert_eq!(
        pipeline.audio.borrow().events.last().unwrap().1,
        CaptureEvent::Drop
    );
    pipeline
        .controller
        .set_microphone_permission(clock(), Permission::Denied)
        .unwrap();
    pipeline.worker = Worker::new(NEXT_VOICE);
    for offset in [135_000, 155_000, 175_000, 195_000] {
        let (_, lease) = pipeline.sample(START + offset, Some(true)).unwrap();
        assert!(
            !lease.permitted,
            "fresh HIGH cannot clear controller software denial"
        );
    }
    pipeline
        .controller
        .set_microphone_permission(clock(), Permission::Allowed)
        .unwrap();
    let (_, restored) = pipeline.forward();
    assert!(restored.permitted);
    assert_eq!(restored.session_id, NEXT_VOICE);
    assert!(restored.microphone_generation > same_worker.microphone_generation);
    assert!(
        pipeline.tick(BLOCK - 1).is_none(),
        "old partial PCM must be gone"
    );
    assert!(pipeline.tick(1).is_some());
    assert_eq!(pipeline.audio.borrow().dropped_samples, 0);
}

#[test]
fn repeated_publication_preserves_original_acquisition_and_cannot_extend_capture() {
    let mut pipeline = Pipeline::new(Permission::Allowed);
    let (_, first) = pipeline.stable_high();
    assert!(pipeline.tick(127).is_none());
    advance(first.observed_us + FRESH_US - 1);
    let message = pipeline.controls.message(now());
    pipeline.worker.accept(&message).unwrap();
    let (_, renewed) = pipeline.forward();
    assert!(renewed.permitted);
    assert_eq!(renewed.observed_us, first.observed_us);
    assert_eq!(renewed.expires_at_us, first.observed_us + FRESH_US);
    assert!(pipeline.ears.is_open());

    advance(renewed.expires_at_us);
    let message = pipeline.controls.message(now());
    let state = message.parse::<ControlsState>().unwrap();
    assert_eq!(state.privacy.unwrap().muted, None);
    assert!(pipeline.worker.accept(&message).is_err());
    assert!(pipeline.ears.maintain(now()).unwrap().is_none());
    assert!(!pipeline.ears.is_open());
    assert_eq!(
        pipeline.audio.borrow().events.last().unwrap(),
        &(now(), CaptureEvent::Drop)
    );
}

#[test]
fn missing_controls_or_a_publisher_restart_cannot_renew_an_open_capture() {
    for failure in 0..3 {
        let mut pipeline = Pipeline::new(Permission::Allowed);
        let (_, permitted) = pipeline.stable_high();
        assert!(pipeline.tick(127).is_none());
        let acquired = START + 65_000;
        pipeline
            .controls
            .acquire(acquired, if failure == 0 { None } else { Some(true) });
        let mut message = pipeline.controls.message(acquired + 1_000);
        advance(acquired + 3_000);
        match failure {
            1 => message.publisher_restarted = true,
            2 => {
                let mut state = message.parse::<ControlsState>().unwrap();
                state.session_id = "fedcba9876543210".into();
                message.header.boot = state.session_id.clone();
                message.payload = serde_json::to_vec(&state).unwrap();
            }
            _ => {}
        }
        assert!(pipeline.worker.accept(&message).is_err());
        // Production terminates this invalid worker. No replacement lease is
        // supplied, so the independent capture owner must expire by itself.
        advance(permitted.expires_at_us);
        assert!(pipeline.ears.maintain(now()).unwrap().is_none());
        assert!(!pipeline.ears.is_open());
        assert!(pipeline.tick(BLOCK).is_none());
        assert_eq!(
            pipeline.audio.borrow().events.last().unwrap().1,
            CaptureEvent::Drop
        );
    }
}

/// Voice's own microphone gate, fed as the privacy worker feeds it: each
/// accepted acquisition with its original time.
struct Mic2 {
    gate: PrivacyGate,
    sequence: u64,
}

impl Mic2 {
    fn new() -> Self {
        Self {
            gate: PrivacyGate::default(),
            sequence: 0,
        }
    }

    fn feed(&mut self, worker: &Worker) {
        let observed = worker.last.as_ref().unwrap();
        self.sequence += 1;
        self.gate.observe(
            self.sequence,
            MonoTime::from_micros(observed.stamp().at_us),
            clock(),
            observed.muted().unwrap(),
        );
    }

    fn open(&self) -> bool {
        self.gate.allowed(clock())
    }
}

/// First's case for #795: Controls stops while the slide is open. Both
/// microphones close no later than 50 ms after the last acquisition, stay
/// closed, and reopen only after fresh evidence has been steady for 60 ms.
#[test]
fn controls_stopping_with_the_slide_open_closes_both_microphones_within_50_ms() {
    let mut pipeline = Pipeline::new(Permission::Allowed);
    let mut mic2 = Mic2::new();
    for offset in [0, 20_000, 40_000, 60_000] {
        let (_, lease) = pipeline.sample(START + offset, Some(true)).unwrap();
        mic2.feed(&pipeline.worker);
        assert_eq!(lease.permitted, offset == 60_000, "{offset}");
        assert_eq!(mic2.open(), offset == 60_000, "{offset}");
    }
    assert!(pipeline.tick(127).is_none());
    assert!(pipeline.ears.is_open());

    // Controls stops: nothing more is acquired or published.
    let last = START + 60_000;
    advance(last + FRESH_US - 1);
    assert!(mic2.open());
    assert!(pipeline.ears.maintain(now()).unwrap().is_some());
    assert!(pipeline.ears.is_open());
    advance(last + FRESH_US);
    assert_eq!(mic2.gate.state(clock()), PrivacyState::Unavailable);
    assert!(!mic2.open());
    assert!(pipeline.ears.maintain(now()).unwrap().is_none());
    assert!(!pipeline.ears.is_open());
    assert_eq!(
        pipeline.audio.borrow().events.last(),
        Some(&(last + FRESH_US, CaptureEvent::Drop))
    );
    // A lease Voice would still publish now grants nothing.
    let (_, lease) = pipeline.forward();
    assert!(!lease.permitted);
    assert_eq!(lease.physical_muted, None);

    // Closed while nothing comes, with Voice's permission still allowed.
    for later in [100_000, 200_000, 290_000] {
        advance(last + later);
        assert!(!mic2.open());
        assert!(pipeline.ears.maintain(now()).unwrap().is_none());
        assert!(pipeline.tick(BLOCK).is_none());
    }
    let opened = |pipeline: &Pipeline| {
        pipeline
            .audio
            .borrow()
            .events
            .iter()
            .filter(|(_, event)| *event == CaptureEvent::Open)
            .count()
    };
    assert_eq!(opened(&pipeline), 1);

    // Controls comes back. The old worker has ended on its expired source, so
    // the supervisor starts a new one and a new gate; the first fresh HIGH
    // opens nothing, and only 60 ms of steady HIGH reopens both.
    let back = last + 300_000;
    pipeline.controls = Controls::new();
    pipeline.worker = Worker::new(NEXT_VOICE);
    let mut mic2 = Mic2::new();
    for offset in [0, 20_000, 40_000] {
        let (_, lease) = pipeline.sample(back + offset, Some(true)).unwrap();
        mic2.feed(&pipeline.worker);
        assert!(!lease.permitted, "{offset}");
        assert_eq!(mic2.gate.state(clock()), PrivacyState::Stabilizing);
        assert!(pipeline.tick(BLOCK).is_none());
    }
    assert_eq!(opened(&pipeline), 1);
    let (_, lease) = pipeline.sample(back + 60_000, Some(true)).unwrap();
    mic2.feed(&pipeline.worker);
    assert!(lease.permitted);
    assert!(mic2.open());
    assert!(pipeline.tick(127).is_none());
    assert!(pipeline.ears.is_open());
    assert_eq!(opened(&pipeline), 2);
}

#[test]
fn stable_high_never_grants_a_software_denied_controller() {
    let mut pipeline = Pipeline::new(Permission::Denied);
    let (_, lease) = pipeline.stable_high();
    assert!(!lease.permitted);
    assert_eq!(lease.physical_muted, Some(false));
    assert!(pipeline.tick(BLOCK).is_none());
    assert!(pipeline.audio.borrow().events.is_empty());
}

#[test]
fn unavailable_startup_has_no_capture_and_does_not_claim_a_deliberate_mute() {
    let mut pipeline = Pipeline::new(Permission::Denied);
    let message = pipeline.controls.message(START);
    assert!(pipeline.worker.accept(&message).is_err());
    let (_, unknown) = pipeline.forward();
    assert!(!unknown.permitted);
    assert_eq!(unknown.physical_muted, None);
    assert_eq!(unknown.observed_us, 0);
    assert!(pipeline.tick(BLOCK).is_none());
    assert!(pipeline.audio.borrow().events.is_empty());
}

#[test]
fn a_different_controller_cannot_replace_the_workers_private_authority() {
    let mut pipeline = Pipeline::new(Permission::Denied);
    pipeline.stable_high();
    let mut other = Controller::new(BootId::new([2; 16]).unwrap(), clock());
    other
        .set_microphone_permission(clock(), Permission::Allowed)
        .unwrap();
    assert!(
        pipeline
            .worker
            .message(other.snapshot(clock()).unwrap())
            .is_err()
    );
    assert!(pipeline.tick(BLOCK).is_none());
    assert!(pipeline.audio.borrow().events.is_empty());
}

/// A lamp with no slide (`no-slide`): steady fresh markers open MIC1 for an
/// Allowed controller after the usual 60 ms, each lease bound to its Controls
/// publication with no physical level; the software mute (a Denied
/// controller) closes it at once and never grants while it stays.
#[test]
fn with_no_slide_steady_markers_open_capture_and_the_software_mute_closes_it() {
    let mut pipeline = Pipeline::no_slide(Permission::Allowed);
    for offset in [0, 20_000, 40_000] {
        let (_, lease) = pipeline.marker(START + offset).unwrap();
        assert!(!lease.permitted, "{offset}");
        assert!(lease.no_slide);
        assert_eq!(lease.physical_muted, None);
        assert_eq!(lease.observed_us, START + offset);
        assert!(pipeline.tick(BLOCK).is_none());
    }
    let (_, open) = pipeline.marker(START + 60_000).unwrap();
    assert!(open.permitted);
    assert!(open.no_slide);
    assert_eq!(open.physical_muted, None);
    assert_eq!(open.observed_us, START + 60_000);
    assert_eq!(open.expires_at_us, START + 60_000 + FRESH_US);
    assert!(pipeline.tick(127).is_none());
    assert!(pipeline.ears.is_open());

    advance(START + 65_000);
    pipeline
        .controller
        .set_microphone_permission(clock(), Permission::Denied)
        .unwrap();
    let (_, muted) = pipeline.forward();
    assert!(!muted.permitted);
    assert!(muted.microphone_generation > open.microphone_generation);
    assert!(!pipeline.ears.is_open());
    for offset in [80_000, 100_000, 120_000, 140_000] {
        let (_, lease) = pipeline.marker(START + offset).unwrap();
        assert!(!lease.permitted, "a fresh marker cannot clear the mute");
        assert!(pipeline.tick(BLOCK).is_none());
    }
    let audio = pipeline.audio.borrow();
    assert_eq!(
        audio.events,
        [
            (START + 62_000, CaptureEvent::Open),
            (START + 62_000, CaptureEvent::Read(127)),
            (START + 65_000, CaptureEvent::Discard),
            (START + 65_000, CaptureEvent::Drop),
        ]
    );
}

/// Controls stops with no slide: renewed leases keep their publication, so
/// MIC1 closes no later than 50 ms after it, whatever the controller says.
#[test]
fn with_no_slide_capture_ends_fifty_ms_after_the_last_marker() {
    let mut pipeline = Pipeline::no_slide(Permission::Allowed);
    for offset in [0, 20_000, 40_000, 60_000] {
        pipeline.marker(START + offset).unwrap();
    }
    assert!(pipeline.tick(127).is_none());
    assert!(pipeline.ears.is_open());
    let last = START + 60_000;
    advance(last + FRESH_US - 1);
    let (_, renewed) = pipeline.forward();
    assert!(renewed.permitted);
    assert_eq!(renewed.observed_us, last);
    assert_eq!(renewed.expires_at_us, last + FRESH_US);
    advance(last + FRESH_US);
    assert!(pipeline.ears.maintain(now()).unwrap().is_none());
    assert!(!pipeline.ears.is_open());
    let (_, lease) = pipeline.forward();
    assert!(!lease.permitted);
    assert!(!lease.no_slide, "nothing fresh is observed: unknown");
    assert_eq!((lease.physical_muted, lease.observed_us), (None, 0));
}

/// Before any marker the lease is unknown and grants nothing; a publisher
/// restart, a changed session, slide evidence or a state with no marker
/// ends the worker, and MIC1 closes at the old lease's expiry by itself.
#[test]
fn with_no_slide_missing_restarted_or_unmarked_controls_never_keep_capture() {
    let mut pipeline = Pipeline::no_slide(Permission::Allowed);
    let (_, unknown) = pipeline.forward();
    assert!(!unknown.permitted && !unknown.no_slide);
    assert_eq!((unknown.physical_muted, unknown.observed_us), (None, 0));
    assert!(pipeline.tick(BLOCK).is_none());
    assert!(pipeline.audio.borrow().events.is_empty());
    for failure in 0..4 {
        let mut pipeline = Pipeline::no_slide(Permission::Allowed);
        for offset in [0, 20_000, 40_000, 60_000] {
            pipeline.marker(START + offset).unwrap();
        }
        let (_, permitted) = pipeline.forward();
        assert!(permitted.permitted);
        assert!(pipeline.tick(127).is_none());
        let published = START + 80_000;
        let mut message = pipeline.controls.message(published);
        advance(published + 2_000);
        let mut state = message.parse::<ControlsState>().unwrap();
        match failure {
            0 => message.publisher_restarted = true,
            1 => {
                state.session_id = "fedcba9876543210".into();
                message.header.boot = state.session_id.clone();
            }
            2 => {
                state.no_privacy_slide = false;
                let mut slide = Controls::new();
                slide.acquire(published, Some(true));
                state.privacy = slide
                    .message(published)
                    .parse::<ControlsState>()
                    .unwrap()
                    .privacy;
            }
            _ => state.no_privacy_slide = false,
        }
        if failure != 0 {
            message.payload = serde_json::to_vec(&state).unwrap();
        }
        assert!(
            pipeline.worker.accept(&message).is_err(),
            "failure {failure}"
        );
        advance(permitted.expires_at_us);
        assert!(pipeline.ears.maintain(now()).unwrap().is_none());
        assert!(!pipeline.ears.is_open());
        assert_eq!(
            pipeline.audio.borrow().events.last().unwrap().1,
            CaptureEvent::Drop
        );
    }
}
