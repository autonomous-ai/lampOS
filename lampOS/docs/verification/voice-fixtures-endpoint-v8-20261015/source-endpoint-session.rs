//! Directed sessions through the real coordinator loop with opt-in endpoint
//! confirmation. Capture, privacy, provider and speaker are emulated peers on
//! the production channels; the endpoint worker is a real child process or an
//! in-process double. No audio device, cloud service or model is used.
use super::*;
use crate::{
    provider_link::{LinkContext, ProviderLinkState},
    provider_worker::{ProviderInput, ProviderOutput},
    scenario_clock,
    wire::{CaptureTiming, ReferenceTiming, SpeakerChunk},
};
use lamp_audio::ownership::PlaybackCursor;
use lamp_turn_end_wire::{
    Availability, Classifier, Limits, Outcome, Submit, Token, Verdict, WorkerCommand,
    frame::{PROTOCOL, Response},
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

const SPEECH: f32 = 0.95;
const QUIET: f32 = 0.05;
// Real-worker ordering tests accept bounded host scheduling delay. This does
// not measure the lamp's 50 ms privacy response; input and IPC still expire at
// 100 ms, and classifier startup/result/cleanup clocks remain real.
const PROCESS_FIXTURE_PRIVACY_FRESH_US: u64 = 100_000;
const PROCESS_FIXTURE_WATCHDOG: Duration = Duration::from_secs(12);

/// Stepped and mute/cleanup sessions retain the default 50 ms privacy bound.
/// Finite real-worker ordering sessions explicitly select the 100 ms fixture
/// policy and poll peers every millisecond. Stepped sessions complete every
/// peer step before advancing their clock.
/// A test takes this slot before starting the endpoint worker whose timing it
/// asserts, so other tests here cannot compete for the same scheduler slot.
static ONE_SESSION: Mutex<()> = Mutex::new(());

pub(super) fn one_session() -> std::sync::MutexGuard<'static, ()> {
    ONE_SESSION
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Speech probability of each 10 ms capture block, by block index.
type Script = fn(u64) -> f32;

#[derive(Default)]
struct Seen {
    provider: Vec<&'static str>,
    /// Every request the provider peer saw, in order.
    requests: Vec<SeenRequest>,
    /// Every speech chunk dispatched to the speaker: its turn and when.
    chunks: Vec<(u64, Instant)>,
    /// When the capture peer sent its first frame.
    capture_started: Option<Instant>,
    capture_delivery: CaptureDelivery,
    privacy_delivery: PrivacyDelivery,
}

/// Bounded source/delivery diagnostics. Delivery is observed immediately after
/// a successful channel send; it is not coordinator consumption or lamp latency.
#[derive(Debug, Default)]
struct CaptureDelivery {
    frames: u64,
    /// Last (sequence, acquired_at_us, after_send_at_us), on the selected clock.
    last: Option<(u64, u64, u64)>,
    max_source_gap_us: u64,
    max_delivery_gap_us: u64,
    max_delivery_age_us: u64,
}

impl CaptureDelivery {
    fn sent(&mut self, sequence: u64, acquired_at_us: u64, delivered_at_us: u64) {
        if let Some((_, source, delivery)) = self.last {
            self.max_source_gap_us = self
                .max_source_gap_us
                .max(acquired_at_us.saturating_sub(source));
            self.max_delivery_gap_us = self
                .max_delivery_gap_us
                .max(delivered_at_us.saturating_sub(delivery));
        }
        self.max_delivery_age_us = self
            .max_delivery_age_us
            .max(delivered_at_us.saturating_sub(acquired_at_us));
        self.frames += 1;
        self.last = Some((sequence, acquired_at_us, delivered_at_us));
    }
}

/// Bounded privacy producer observations, never coordinator consumption age.
/// The acquisition timestamp is captured before send and is never refreshed.
#[derive(Debug, Default)]
struct PrivacyDelivery {
    observations: u64,
    /// Last (acquired_at_us, after_send_at_us), on the selected clock.
    last: Option<(u64, u64)>,
    max_acquisition_gap_us: u64,
    max_after_send_gap_us: u64,
    max_after_send_age_us: u64,
}

impl PrivacyDelivery {
    fn sent(&mut self, acquired_at_us: u64, after_send_at_us: u64) {
        if let Some((acquired, sent)) = self.last {
            self.max_acquisition_gap_us = self
                .max_acquisition_gap_us
                .max(acquired_at_us.saturating_sub(acquired));
            self.max_after_send_gap_us = self
                .max_after_send_gap_us
                .max(after_send_at_us.saturating_sub(sent));
        }
        self.max_after_send_age_us = self
            .max_after_send_age_us
            .max(after_send_at_us.saturating_sub(acquired_at_us));
        self.observations += 1;
        self.last = Some((acquired_at_us, after_send_at_us));
    }
}

#[derive(Clone, Debug)]
struct SeenRequest {
    request: u64,
    audio_blocks: usize,
    ended_at: Option<Instant>,
}

impl Seen {
    /// Capture block `index` (1-based) as an instant.
    fn block(&self, index: u64) -> Instant {
        self.capture_started.expect("capture started") + Duration::from_millis(10 * (index - 1))
    }
}

/// How the emulated provider answers each request after its end.
#[derive(Clone, Copy)]
struct ProviderReply {
    after_end: Duration,
    packets: usize,
}

// The threaded and stepped modes share exactly the same peer state machines.
// No peer can keep a coordinator step busy by producing an endless queue.
const PEER_STEP_BUDGET: usize = 64;
type PeerStep = Box<dyn FnMut() -> io::Result<bool> + Send>;

fn drain_control(channels: &mut WorkerChannels, generation: &mut u64) -> io::Result<bool> {
    for _ in 0..PEER_STEP_BUDGET {
        let Some(control) = channels.control.receive::<Control>()? else {
            break;
        };
        match control {
            Control::Authority { snapshot } => *generation = snapshot.microphone_generation(),
            Control::Stop => return Ok(false),
            Control::StartCapture => {}
            _ => {}
        }
    }
    Ok(true)
}

fn capture_peer(mut channels: WorkerChannels, script: Script, seen: Arc<Mutex<Seen>>) -> PeerStep {
    let mut generation = 0;
    let mut started = None;
    let mut sequence = 0u64;
    Box::new(move || {
        for _ in 0..PEER_STEP_BUDGET {
            let Some(control) = channels.control.receive::<Control>()? else {
                break;
            };
            match control {
                Control::Authority { snapshot } => generation = snapshot.microphone_generation(),
                Control::StartCapture if started.is_none() => {
                    let at = monotonic_us();
                    let instant = scenario_clock::instant();
                    channels.control.send(WorkerEvent::CaptureStarted {
                        epoch: 1,
                        dsp_epoch: 1,
                        noise_suppression: NoiseSuppression::On,
                        privacy_generation: generation,
                        rate: 16_000,
                        channels: 1,
                        period_frames: 160,
                        buffer_frames: 640,
                        monotonic_status_timestamps: false,
                        open_started_at_us: at,
                        prepared_at_us: at,
                        capture_started_at_us: at,
                        start_completed_at_us: at,
                    })?;
                    started = Some(at);
                    seen.lock().unwrap().capture_started = Some(instant);
                }
                Control::Stop => return Ok(false),
                _ => {}
            }
        }
        // This synthetic PCM source acquires one block every 10 ms, regardless
        // of when its delivery thread runs. Keep that original source timestamp
        // when delivering a backlog: refreshing it here would hide stale audio
        // and turn a scheduling pause into a false gap between source samples.
        // Both modes use the same timeline; unscoped real-worker tests still use
        // real monotonic time for this origin, envelope delivery and all policy.
        for _ in 0..PEER_STEP_BUDGET {
            let Some(start) = started else {
                break;
            };
            let at = start + 10_000 * sequence;
            if at > monotonic_us() {
                break;
            }
            sequence += 1;
            let probability = script(sequence);
            let level = if probability >= 0.5 { 2_000 } else { 20 };
            let frame = MicrophoneFrame {
                epoch: 1,
                dsp_epoch: 1,
                privacy_generation: generation,
                frame_sequence: sequence,
                read_completed_at_us: at,
                probability,
                echo: None,
                reference: Default::default(),
                samples: (0..160)
                    .map(|i| if i % 2 == 0 { level } else { -level })
                    .collect(),
                timing: CaptureTiming {
                    first_read_started_at_us: at,
                    last_read_started_at_us: at,
                    successful_reads: 1,
                    alsa_status_monotonic_us: None,
                    status_observed_at_us: at,
                    available_frames: 0,
                    delayed_frames: 0,
                    processing_completed_at_us: at,
                    aec_queue_delay_ms: 0,
                    reference: ReferenceTiming {
                        playback_epoch: 1,
                        clock_started_at_us: Some(at),
                        ..ReferenceTiming::default()
                    },
                },
            };
            channels.data.send(frame)?;
            let delivered_at_us = monotonic_us();
            seen.lock()
                .unwrap()
                .capture_delivery
                .sent(sequence, at, delivered_at_us);
        }
        Ok(true)
    })
}

mod capture_timeline {
    use super::*;

    struct Probe {
        channels: WorkerChannels,
        step: PeerStep,
        seen: Arc<Mutex<Seen>>,
        owner: Controller,
        lineage: CaptureLineage,
        origin_us: u64,
        // The sockets must drop before their private directory.
        _directory: SessionDirectory,
    }

    impl Probe {
        fn start() -> Self {
            let directory = SessionDirectory::create().unwrap();
            let parent = new_boot().unwrap();
            let worker = new_boot().unwrap();
            let mut channels =
                WorkerChannels::bind(&directory.path, "capture", parent, worker, false).unwrap();
            let mut peer =
                WorkerChannels::bind(&directory.path, "capture", parent, worker, true).unwrap();
            channels.connect(&directory.path, "capture", false).unwrap();
            peer.connect(&directory.path, "capture", true).unwrap();
            let origin_us = monotonic_us();
            let at = MonoTime::from_micros(origin_us);
            let mut owner = Controller::new(parent, at);
            owner
                .set_microphone_permission(at, Permission::Allowed)
                .unwrap();
            let snapshot = owner.snapshot(at).unwrap();
            let lineage = CaptureLineage {
                worker,
                epoch: 1,
                dsp_epoch: 1,
                privacy_generation: snapshot.microphone_generation(),
            };
            channels
                .control
                .send(Control::Authority { snapshot })
                .unwrap();
            channels.control.send(Control::StartCapture).unwrap();
            let seen = Arc::new(Mutex::new(Seen::default()));
            let mut step = capture_peer(peer, |_| QUIET, seen.clone());
            assert!(step().unwrap());
            assert!(matches!(
                channels.control.receive::<WorkerEvent>().unwrap(),
                Some(WorkerEvent::CaptureStarted { capture_started_at_us, .. })
                    if capture_started_at_us == origin_us
            ));
            Self {
                channels,
                step,
                seen,
                owner,
                lineage,
                origin_us,
                _directory: directory,
            }
        }

        fn frame(&mut self) -> MicrophoneFrame {
            let frame = self
                .channels
                .data
                .receive::<MicrophoneFrame>()
                .unwrap()
                .unwrap();
            assert_eq!(frame.epoch, self.lineage.epoch);
            assert_eq!(frame.dsp_epoch, self.lineage.dsp_epoch);
            assert_eq!(frame.privacy_generation, self.lineage.privacy_generation);
            let source = self.origin_us + 10_000 * (frame.frame_sequence - 1);
            assert_eq!(frame.read_completed_at_us, source);
            assert_eq!(frame.timing.first_read_started_at_us, source);
            assert_eq!(frame.timing.last_read_started_at_us, source);
            assert_eq!(frame.timing.processing_completed_at_us, source);
            frame
        }

        fn context(&mut self, at_us: u64) -> AdmissionContext {
            let at = MonoTime::from_micros(at_us);
            let until = MonoTime::from_micros(at_us + 100_000);
            self.owner
                .set_capture(at, CaptureState::RetainingUntil(until))
                .unwrap();
            self.owner
                .set_admission(at, AdmissionState::OpenUntil(until))
                .unwrap();
            AdmissionContext {
                capture: self.lineage,
                authority: self.owner.snapshot(at).unwrap(),
            }
        }
    }

    fn observed(frame: MicrophoneFrame) -> ObservedAudio {
        ObservedAudio {
            sequence: frame.frame_sequence,
            captured_at_us: frame.read_completed_at_us,
            samples: frame.samples.try_into().unwrap(),
        }
    }

    fn delivery_clock() -> scenario_clock::Scope {
        let scope = scenario_clock::Scope::enter(Duration::from_secs(12)).unwrap();
        // Deliberately advance the source timeline while its delivery peer is
        // unscheduled. No host sleep or real process deadline is involved.
        scope.set_pump(|_| Ok(())).unwrap();
        scope
    }

    fn advance(milliseconds: u64) {
        assert_eq!(milliseconds % 2, 0);
        for _ in 0..milliseconds / 2 {
            assert!(scenario_clock::step(scenario_clock::Phase::Run).unwrap());
        }
    }

    #[test]
    fn delayed_delivery_preserves_contiguous_ten_millisecond_samples() {
        let _scope = delivery_clock();
        let mut probe = Probe::start();
        let mut detector = TurnDetector::v1_baseline();
        let first = probe.frame();
        assert_eq!(first.frame_sequence, 1);
        assert!(matches!(
            detector.push(observed(first), QUIET),
            Activity::Quiet
        ));

        advance(60);
        assert!((probe.step)().unwrap());
        for sequence in 2..=7 {
            let frame = probe.frame();
            assert_eq!(frame.frame_sequence, sequence);
            assert!(matches!(
                detector.push(observed(frame), QUIET),
                Activity::Quiet
            ));
        }
        assert!(
            probe
                .channels
                .data
                .receive::<MicrophoneFrame>()
                .unwrap()
                .is_none()
        );
        let seen = probe.seen.lock().unwrap();
        assert_eq!(seen.capture_delivery.frames, 7);
        assert_eq!(seen.capture_delivery.max_source_gap_us, 10_000);
        assert_eq!(seen.capture_delivery.max_delivery_gap_us, 60_000);
        assert_eq!(seen.capture_delivery.max_delivery_age_us, 50_000);
    }

    #[test]
    fn a_fresh_envelope_does_not_refresh_a_stale_source_sample() {
        for (queued_ms, source_age_us, accepted) in [
            (48, 98_000, true),
            (50, 100_000, false),
            (52, 102_000, false),
        ] {
            let _scope = delivery_clock();
            let mut probe = Probe::start();
            assert_eq!(probe.frame().frame_sequence, 1);
            advance(60);
            assert!((probe.step)().unwrap());
            // These six envelopes were just sent at +60 ms. Leave them queued
            // briefly: transport receipt must succeed even when source age
            // reaches the production admission gate's strict 100 ms boundary.
            advance(queued_ms);
            let frame = probe.frame();
            assert_eq!(frame.frame_sequence, 2);
            let at_us = monotonic_us();
            assert_eq!(at_us - frame.read_completed_at_us, source_age_us);
            let context = probe.context(at_us);
            let result = InputAdmission::default().observe(
                Activity::Start(vec![observed(frame)]),
                context,
                at_us,
            );
            if accepted {
                assert!(matches!(result.unwrap().step, AdmissionStep::Candidate(_)));
            } else {
                assert_eq!(
                    result.unwrap_err().to_string(),
                    "candidate trigger is stale or future"
                );
            }
        }
    }
}

/// Fresh privacy observations every 10 ms; muted from capture block
/// `mute_at` when given.
fn privacy_peer(
    mut channels: WorkerChannels,
    seen: Arc<Mutex<Seen>>,
    mute_at: Option<u64>,
) -> PeerStep {
    let mut generation = 0;
    let mut next = None;
    Box::new(move || {
        if !drain_control(&mut channels, &mut generation)? {
            return Ok(false);
        }
        let now = scenario_clock::instant();
        let next = next.get_or_insert(now);
        if now >= *next {
            let muted = {
                let seen = seen.lock().unwrap();
                match (mute_at, seen.capture_started) {
                    (Some(block), Some(_)) => now >= seen.block(block),
                    _ => false,
                }
            };
            let acquired_at_us = monotonic_us();
            channels.control.send(WorkerEvent::Privacy {
                acquired_at_us,
                muted,
            })?;
            let after_send_at_us = monotonic_us();
            seen.lock()
                .unwrap()
                .privacy_delivery
                .sent(acquired_at_us, after_send_at_us);
            *next += Duration::from_millis(10);
        }
        Ok(true)
    })
}

fn provider_peer(
    mut channels: WorkerChannels,
    seen: Arc<Mutex<Seen>>,
    reply: Option<ProviderReply>,
) -> PeerStep {
    assert!(reply.is_none_or(|reply| reply.packets <= PEER_STEP_BUDGET));
    let mut ready = false;
    // Output is credited: the coordinator grants two packets, then more.
    let (mut granted, mut sent) = (2u64, 0u64);
    let mut outbox: VecDeque<ProviderOutput> = VecDeque::new();
    let mut due: VecDeque<(Instant, u64)> = VecDeque::new();
    let mut sequence = 0u64;
    Box::new(move || {
        if !ready {
            let at = monotonic_us();
            channels.control.send(WorkerEvent::ProviderLink {
                link: crate::provider_link::ProviderLink {
                    revision: 1,
                    session: 1,
                    observed_at_us: at,
                    source_clock: None,
                    state: ProviderLinkState::Ready {
                        setup_us: 1_000,
                        attempts: 1,
                        context: LinkContext::Initial,
                        outage_started_at_us: None,
                        after: None,
                    },
                },
            })?;
            ready = true;
        }
        let mut open = true;
        for _ in 0..PEER_STEP_BUDGET {
            let Some(control) = channels.control.receive::<Control>()? else {
                break;
            };
            match control {
                Control::ProviderOutputCapacity { through } => granted = granted.max(through),
                Control::Stop => open = false,
                _ => {}
            }
        }
        if !open {
            return Ok(false);
        }
        for _ in 0..PEER_STEP_BUDGET {
            let Some(input) = channels.data.receive::<ProviderInput>()? else {
                break;
            };
            let mut seen = seen.lock().unwrap();
            match input {
                ProviderInput::Start {
                    request, session, ..
                } => {
                    seen.provider.push("start");
                    seen.requests.push(SeenRequest {
                        request,
                        audio_blocks: 0,
                        ended_at: None,
                    });
                    outbox.push_back(ProviderOutput::Started {
                        request,
                        session,
                        waiting_for_barrier: false,
                    });
                }
                ProviderInput::Audio { request, .. } => {
                    seen.provider.push("audio");
                    if let Some(entry) = seen.requests.iter_mut().find(|r| r.request == request) {
                        entry.audio_blocks += 1;
                    }
                }
                ProviderInput::End { request, .. } => {
                    seen.provider.push("end");
                    if let Some(entry) = seen.requests.iter_mut().find(|r| r.request == request) {
                        entry.ended_at = Some(scenario_clock::instant());
                    }
                    if let Some(reply) = reply {
                        due.push_back((scenario_clock::instant() + reply.after_end, request));
                    }
                }
                ProviderInput::Written { .. } => seen.provider.push("written"),
                ProviderInput::Reminder { .. } => panic!("reminders are disabled in this fixture"),
            }
        }
        let now = scenario_clock::instant();
        for _ in 0..PEER_STEP_BUDGET {
            let Some(&(at, request)) = due.front() else {
                break;
            };
            if at > now {
                break;
            }
            due.pop_front();
            let packets = reply.map_or(0, |reply| reply.packets);
            for _ in 0..packets {
                sequence += 1;
                outbox.push_back(ProviderOutput::Audio {
                    request,
                    sequence,
                    provider_event_at_us: monotonic_us(),
                    source_frames: 960,
                    source_offset: 0,
                    samples: vec![1_000; 960],
                });
            }
            outbox.push_back(ProviderOutput::GenerationComplete { request });
            outbox.push_back(ProviderOutput::TurnComplete {
                request,
                idle: true,
            });
        }
        for _ in 0..PEER_STEP_BUDGET {
            if sent >= granted {
                break;
            }
            let Some(output) = outbox.pop_front() else {
                break;
            };
            channels.data.send(output)?;
            sent += 1;
        }
        Ok(true)
    })
}

/// Records every chunk and acknowledges it as ALSA would, so the v1-baseline
/// mask sees genuine write evidence.
fn speaker_peer(mut channels: WorkerChannels, seen: Arc<Mutex<Seen>>) -> PeerStep {
    let mut generation = 0;
    let mut frame = 1_000u64;
    Box::new(move || {
        if !drain_control(&mut channels, &mut generation)? {
            return Ok(false);
        }
        for _ in 0..PEER_STEP_BUDGET {
            let Some(chunk) = channels.data.receive::<SpeakerChunk>()? else {
                break;
            };
            let owner = chunk.permit.owner();
            seen.lock()
                .unwrap()
                .chunks
                .push((owner.turn(), scenario_clock::instant()));
            let frames = chunk.samples.len();
            let at = monotonic_us();
            channels.control.send(WorkerEvent::Playback {
                owner,
                first_sample: PlaybackCursor { epoch: 1, frame },
                end_sample: PlaybackCursor {
                    epoch: 1,
                    frame: frame + frames as u64,
                },
                queue_observed_at_us: at,
                turn: owner.turn(),
                sequence: chunk.chunk_sequence,
                queued_frames: frames,
                accepted_frames: frames,
                observed_at_us: at,
            })?;
            frame += frames as u64;
        }
        Ok(true)
    })
}

fn run_peer(mut step: PeerStep, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::SeqCst) && step().unwrap() {
        thread::sleep(Duration::from_millis(1));
    }
}

struct Session {
    result: Result<()>,
    trace: Vec<Value>,
    seen: Seen,
    elapsed: Duration,
}

impl Session {
    fn events(&self, kind: &str) -> Vec<&Value> {
        self.trace
            .iter()
            .filter(|row| row["kind"] == kind)
            .collect()
    }
}

fn run_process_with_privacy_tolerance(
    seconds: u64,
    script: Script,
    endpoint: EndpointRig,
) -> Session {
    run_process_policy_with_privacy_tolerance(TurnTaking::V1Baseline, seconds, script, endpoint)
}

fn run_process_policy_with_privacy_tolerance(
    turn_taking: TurnTaking,
    seconds: u64,
    script: Script,
    endpoint: EndpointRig,
) -> Session {
    run_policy_with_peers(
        turn_taking,
        seconds,
        script,
        endpoint,
        None,
        None,
        PeerMode::RealProcessWithPrivacyTolerance,
    )
}

#[derive(Clone, Copy)]
enum PeerMode {
    RealProcessWithPrivacyTolerance,
    Stepped,
}

impl PeerMode {
    fn fixture_privacy(self) -> Option<PrivacyGate> {
        match self {
            Self::RealProcessWithPrivacyTolerance => {
                Some(PrivacyGate::with_fixture_freshness(PROCESS_FIXTURE_PRIVACY_FRESH_US).unwrap())
            }
            Self::Stepped => None,
        }
    }
}

fn require_real_process_clock() {
    assert!(
        !scenario_clock::is_active(),
        "real endpoint worker fixture requires the host clock"
    );
}

mod fixture_policy_tests {
    use super::*;

    #[test]
    fn only_real_process_ordering_tolerates_a_gap_beyond_fifty_milliseconds() {
        assert!(PeerMode::Stepped.fixture_privacy().is_none());
        let mut stepped = PeerMode::Stepped.fixture_privacy().unwrap_or_default();
        let mut process = PeerMode::RealProcessWithPrivacyTolerance
            .fixture_privacy()
            .unwrap();
        let mut unrelated = PrivacyGate::default();
        for (index, acquired) in (10_000..=70_000).step_by(10_000).enumerate() {
            for gate in [&mut stepped, &mut process, &mut unrelated] {
                gate.observe(
                    index as u64 + 1,
                    MonoTime::from_micros(acquired),
                    MonoTime::from_micros(acquired),
                    false,
                );
            }
        }
        let after_gap = MonoTime::from_micros(145_000);
        assert_eq!(stepped.state(after_gap), PrivacyState::Unavailable);
        assert_eq!(unrelated.state(after_gap), PrivacyState::Unavailable);
        assert_eq!(process.state(after_gap), PrivacyState::Allowed);
        assert_eq!(
            process.state(MonoTime::from_micros(169_999)),
            PrivacyState::Allowed
        );
        assert_eq!(
            process.state(MonoTime::from_micros(170_000)),
            PrivacyState::Unavailable
        );
        assert_eq!(PrivacyGate::default().freshness_us(), 50_000);
    }

    #[test]
    #[should_panic(expected = "real endpoint worker fixture requires the host clock")]
    fn a_logical_clock_is_rejected_before_the_process_constructor() {
        let _scope = scenario_clock::Scope::enter(Duration::from_secs(12)).unwrap();
        let _endpoint = scripted_process(&[], "exit 0", Limits::default());
    }
}

/// Only an in-process classifier can use the logical scenario clock. Real
/// ProcessClassifier readiness, deadlines and teardown stay on the host clock.
fn run_scenario(seconds: u64, script: Script, classifier: Box<dyn Classifier>) -> Session {
    run_scenario_with(seconds, script, classifier, None)
}

fn run_scenario_with(
    seconds: u64,
    script: Script,
    classifier: Box<dyn Classifier>,
    reply: Option<ProviderReply>,
) -> Session {
    run_scenario_policy(TurnTaking::V1Baseline, seconds, script, classifier, reply)
}

fn run_scenario_policy(
    turn_taking: TurnTaking,
    seconds: u64,
    script: Script,
    classifier: Box<dyn Classifier>,
    reply: Option<ProviderReply>,
) -> Session {
    run_policy_with_peers(
        turn_taking,
        seconds,
        script,
        EndpointRig::double(classifier),
        reply,
        None,
        PeerMode::Stepped,
    )
}

fn run_policy_with_peers(
    turn_taking: TurnTaking,
    seconds: u64,
    script: Script,
    endpoint: EndpointRig,
    reply: Option<ProviderReply>,
    mute_at: Option<u64>,
    mode: PeerMode,
) -> Session {
    let fixture_privacy = mode.fixture_privacy();
    if fixture_privacy.is_some() {
        require_real_process_clock();
    }
    let dir = SessionDirectory::create().unwrap();
    let (capture, capture_peer_channels) = Worker::sleeping_fixture(&dir.path, "capture");
    let (speaker, speaker_peer_channels) = Worker::sleeping_fixture(&dir.path, "speaker");
    let (provider, provider_peer_channels) = Worker::sleeping_fixture(&dir.path, "provider");
    let (privacy, privacy_peer_channels) = Worker::sleeping_fixture(&dir.path, "privacy");
    // Starting children is real OS work. Enter only after their startup barriers
    // and keep the scope through shutdown and collection of scenario timestamps.
    let mut scope = match mode {
        PeerMode::RealProcessWithPrivacyTolerance => None,
        PeerMode::Stepped => Some(scenario_clock::Scope::enter(Duration::from_secs(12)).unwrap()),
    };
    let stop = Arc::new(AtomicBool::new(false));
    let seen = Arc::new(Mutex::new(Seen::default()));
    let steps = [
        capture_peer(capture_peer_channels, script, seen.clone()),
        privacy_peer(privacy_peer_channels, seen.clone(), mute_at),
        provider_peer(provider_peer_channels, seen.clone(), reply),
        speaker_peer(speaker_peer_channels, seen.clone()),
    ];
    let peers: Vec<_> = if let Some(scope) = scope.as_mut() {
        let mut steps = steps.map(|step| (true, step));
        scope
            .set_pump(move |_| {
                for (open, step) in &mut steps {
                    if *open {
                        *open = step()?;
                    }
                }
                Ok(())
            })
            .unwrap();
        Vec::new()
    } else {
        steps
            .into_iter()
            .map(|step| {
                let stop = stop.clone();
                thread::spawn(move || run_peer(step, stop))
            })
            .collect()
    };
    let mut rig = Rig {
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
        early_end_blocks: None,
        faint_level: None,
        provider,
        capture,
        speaker,
        privacy,
        ring: None.into(),
        camera: None,
        motor: None,
        ring_channel_ceiling: None,
        diagnostic_directory: None,
        noise_suppression: NoiseSuppression::On,
        turn_taking,
        playout_start: PlayoutStart::default(),
        endpoint: Some(endpoint),
    };
    let mut owner = Controller::new(new_boot().unwrap(), now());
    let mut trace = SessionState::finite();
    let mut reply = None;
    let started = Instant::now();
    let result = if let Some(gate) = fixture_privacy {
        conversation_with_fixture_privacy(
            &mut rig,
            &mut owner,
            Lifetime::Finite(seconds),
            &mut trace,
            StartupEvents::default(),
            &mut reply,
            &mut None,
            &Termination::inactive(),
            &FailureNotices::none(),
            gate,
            started + PROCESS_FIXTURE_WATCHDOG,
        )
    } else {
        conversation(
            &mut rig,
            &mut owner,
            Lifetime::Finite(seconds),
            &mut trace,
            StartupEvents::default(),
            &mut reply,
            &mut None,
            &Termination::inactive(),
            &FailureNotices::none(),
        )
    };
    let elapsed = started.elapsed();
    // Sleeping fixtures need forced reaping; only the endpoint record matters.
    let _ = rig.shutdown(&mut trace);
    stop.store(true, Ordering::SeqCst);
    for peer in peers {
        peer.join().unwrap();
    }
    let seen = std::mem::take(&mut *seen.lock().unwrap());
    if let Err(error) = &result {
        let clock = match mode {
            PeerMode::RealProcessWithPrivacyTolerance => "host, explicit 100 ms privacy fixture",
            PeerMode::Stepped => "logical",
        };
        eprintln!(
            "endpoint fixture failed: {error}; packet/source clock {clock}; real conversation {elapsed:?}; capture after-send observations {:?}; privacy producer observations (not consumption) {:?}",
            seen.capture_delivery, seen.privacy_delivery
        );
    }
    drop(scope);
    Session {
        result,
        trace: trace.to_vec(),
        seen,
        elapsed,
    }
}

/// A real child process that replays fixed answer bytes and then sleeps or
/// exits, written with POSIX `printf` octal escapes.
fn scripted_process(responses: &[Response], then: &str, limits: Limits) -> EndpointRig {
    require_real_process_clock();
    let bytes: String = responses
        .iter()
        .flat_map(Response::encode)
        .map(|byte| format!("\\{byte:03o}"))
        .collect();
    EndpointRig::process(
        WorkerCommand {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), format!("printf '{bytes}'; {then}").into()],
            env: Vec::new(),
            // `ready()` reports this identity.
            model_sha256: [0; 32],
        },
        limits,
    )
}

fn ready() -> Response {
    Response::Ready {
        protocol: PROTOCOL,
        load_us: 1,
        cold_us: 1,
        model_sha256: [0; 32],
    }
}

/// 0.5 s quiet, 1.5 s speech, then quiet.
fn one_question(block: u64) -> f32 {
    if (50..200).contains(&block) {
        SPEECH
    } else {
        QUIET
    }
}

fn endpoint_rows(session: &Session) -> Vec<&Value> {
    session.events("local_endpoint")
}

#[test]
fn missing_model_session_ends_on_the_unchanged_silence_fallback() {
    let _one = one_session();
    let missing = Response::Error {
        request: 0,
        reason: "model /lamp/smart-turn-v3.2-gpu.onnx: No such file or directory".into(),
    };
    let session = run_process_with_privacy_tolerance(
        4,
        one_question,
        scripted_process(&[missing], "exit 2", Limits::default()),
    );
    session.result.as_ref().unwrap();
    let policy = &session.events("endpoint_policy")[0];
    assert_eq!(policy["basis"], "smart_turn_with_silence_fallback");
    assert_eq!(policy["silence_wait_ms"], 1_200);
    let states: Vec<_> = session
        .events("endpoint_worker")
        .iter()
        .map(|row| row["availability"]["state"].as_str().unwrap().to_owned())
        .collect();
    // The worker may fail before the loop first looks; it never became ready.
    assert_eq!(
        states.last().map(String::as_str),
        Some("failed"),
        "{states:?}"
    );
    assert!(!states.contains(&"ready".into()), "{states:?}");
    let failed = session.events("endpoint_worker").last().copied().unwrap();
    assert!(
        failed["availability"]["detail"]
            .as_str()
            .unwrap()
            .contains("No such file")
    );
    assert_eq!(failed["report"]["spawned"], 1);
    let endpoints = endpoint_rows(&session);
    assert_eq!(endpoints.len(), 1, "{endpoints:?}");
    let decision = &endpoints[0]["endpoint"];
    assert_eq!(decision["basis"], "silence_fallback");
    assert_eq!(decision["fallback"], "failed");
    assert_eq!(decision["silence_ms"], 1_200);
    assert_eq!(endpoints[0]["includes_silence_wait_ms"], 1_200);
    assert_eq!(
        session
            .seen
            .provider
            .iter()
            .filter(|kind| **kind == "end")
            .count(),
        1
    );
    assert!(session.seen.provider.contains(&"start"));
    let last = session.events("endpoint_worker_final")[0];
    assert_eq!(last["report"]["kills"], 0);
    assert_eq!(last["reaped"], true);
}

#[test]
fn hung_worker_never_delays_capture_endpoint_or_session_end() {
    let _one = one_session();
    let limits = Limits {
        hang: Duration::from_millis(700),
        backoff: Duration::from_millis(100),
        ..Limits::default()
    };
    // Two questions; the worker accepts each request and never answers.
    fn two_questions(block: u64) -> f32 {
        if (50..150).contains(&block) || (300..400).contains(&block) {
            SPEECH
        } else {
            QUIET
        }
    }
    let session = run_process_with_privacy_tolerance(
        6,
        two_questions,
        scripted_process(&[ready()], "exec sleep 60", limits),
    );
    // A stalled loop still fails the unchanged 100 ms capture lease and this
    // explicit 100 ms integration privacy bound, not the lamp's 50 ms bound.
    session.result.as_ref().unwrap();
    assert!(
        session.elapsed < Duration::from_secs(8),
        "{:?}",
        session.elapsed
    );
    let endpoints = endpoint_rows(&session);
    assert!(!endpoints.is_empty());
    let decision = &endpoints[0]["endpoint"];
    assert_eq!(decision["basis"], "silence_fallback");
    assert_eq!(decision["fallback"], "timeout");
    assert_eq!(decision["silence_ms"], 1_200);
    let states: Vec<_> = session
        .events("endpoint_worker")
        .iter()
        .map(|row| row["availability"]["state"].as_str().unwrap().to_owned())
        .collect();
    // Loading started with the other workers and may already be complete.
    assert!(
        matches!(states[0].as_str(), "loading" | "ready"),
        "{states:?}"
    );
    assert!(states.contains(&"recovering".into()), "{states:?}");
    let last = session.events("endpoint_worker_final")[0];
    // The worker hung during shutdown too: killed after its grace, then reaped.
    assert_eq!(last["reaped"], true, "{last}");
    assert!(last["report"]["kills"].as_u64().unwrap() >= 1, "{last}");
    assert!(last["report"]["spawned"].as_u64().unwrap() >= 2, "{last}");
}

/// Answers each request after a delay on the selected fixture clock. An
/// ordinary threaded session keeps the original real delay.
struct Delayed {
    probability: f32,
    delay: Duration,
    pending: Option<(Token, u64, Instant)>,
    requests: Arc<Mutex<Vec<Token>>>,
}

impl Classifier for Delayed {
    fn submit(&mut self, token: Token, _pcm: &[i16]) -> Submit {
        if self.pending.is_some() {
            return Submit::Busy;
        }
        let mut requests = self.requests.lock().unwrap();
        requests.push(token);
        self.pending = Some((token, requests.len() as u64, scenario_clock::instant()));
        Submit::Accepted
    }
    fn poll(&mut self) -> Option<Outcome> {
        let (token, request, at) = self.pending?;
        if scenario_clock::instant().duration_since(at) < self.delay {
            return None;
        }
        self.pending = None;
        Some(Outcome {
            token,
            worker: 1,
            request,
            result: Ok(Verdict {
                probability: self.probability,
                round_trip_us: self.delay.as_micros() as u64,
                ..Verdict::default()
            }),
        })
    }
    fn availability(&self) -> Availability {
        Availability::Ready
    }
    fn cancel(&mut self) {}
}

#[test]
fn complete_verdict_ends_early_but_resumed_speech_keeps_the_turn() {
    let _one = one_session();
    // Speech, a 350 ms pause (verdict arrives, commit point not reached), more
    // speech, then the final pause.
    fn resumed(block: u64) -> f32 {
        if (50..150).contains(&block) || (185..260).contains(&block) {
            SPEECH
        } else {
            QUIET
        }
    }
    let requests = Arc::new(Mutex::new(Vec::new()));
    let double = Delayed {
        probability: 0.95,
        delay: Duration::from_millis(60),
        pending: None,
        requests: requests.clone(),
    };
    let session = run_scenario(4, resumed, Box::new(double));
    session.result.as_ref().unwrap();
    let endpoints = endpoint_rows(&session);
    assert_eq!(endpoints.len(), 1, "split: {endpoints:?}");
    let decision = &endpoints[0]["endpoint"];
    assert_eq!(decision["basis"], "model_complete");
    assert_eq!(decision["silence_ms"], 1_000);
    // The verdict about the first pause was obsolete once speech resumed.
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert_eq!(requests[0].utterance, requests[1].utterance);
    assert_ne!(requests[0].generation, requests[1].generation);
    assert_eq!(decision["verdict"]["request"], 2);
    assert_eq!(
        session
            .seen
            .provider
            .iter()
            .filter(|kind| **kind == "end")
            .count(),
        1
    );
}

/// The installed pair: `LAMP_ENDPOINT_WORKER` and `LAMP_SMART_TURN_MODEL`.
/// Synthetic tones are not speech, so only provenance is asserted.
#[test]
#[ignore = "requires a built lamp-turn-end and the pinned model"]
fn real_worker_session_records_verdict_provenance() {
    let _one = one_session();
    require_real_process_clock();
    let worker = std::env::var_os("LAMP_ENDPOINT_WORKER").expect("LAMP_ENDPOINT_WORKER");
    let model = std::env::var_os("LAMP_SMART_TURN_MODEL").expect("LAMP_SMART_TURN_MODEL");
    let endpoint = EndpointRig::process(
        WorkerCommand::smart_turn(worker.into(), model.into()),
        Limits::default(),
    );
    let session = run_process_with_privacy_tolerance(4, one_question, endpoint);
    session.result.as_ref().unwrap();
    for row in session.events("endpoint_worker") {
        eprintln!("{row}");
    }
    let endpoints = endpoint_rows(&session);
    assert_eq!(endpoints.len(), 1, "{endpoints:?}");
    let decision = &endpoints[0]["endpoint"];
    assert!(
        matches!(
            decision["basis"].as_str(),
            Some("model_complete" | "model_uncertain" | "model_incomplete")
        ),
        "{decision}"
    );
    assert_eq!(decision["verdict"]["worker"], 1);
    let last = session.events("endpoint_worker_final")[0];
    assert_eq!(last["report"]["ready"], 1);
    assert_eq!(last["report"]["kills"], 0);
    eprintln!("{decision}\n{last}");
}

/// A verified physical mute during an active reply, with an endpoint worker
/// attached, through `conversation` and `finish_active_run` as the service
/// runs them. The four audio/provider fixtures stop cleanly, so only the
/// endpoint worker's teardown can fail the cleanup.
fn mute_with_endpoint(endpoint: EndpointRig) -> (Result<()>, SessionState) {
    use crate::service_privacy::PhysicalMute;
    require_real_process_clock();
    let directory = SessionDirectory::create().unwrap();
    let (speaker, speaker_peer, speaker_input) =
        Worker::cooperative_fixture(&directory.path, "speaker");
    let (capture, capture_peer, capture_input) =
        Worker::cooperative_fixture(&directory.path, "capture");
    let (provider, provider_peer, provider_input) =
        Worker::cooperative_fixture(&directory.path, "provider");
    let (privacy, privacy_peer, privacy_input) =
        Worker::cooperative_fixture(&directory.path, "privacy");
    let peers = [
        (speaker_peer, speaker_input, false),
        (capture_peer, capture_input, false),
        (provider_peer, provider_input, false),
        (privacy_peer, privacy_input, true),
    ]
    .map(|(mut channels, input, privacy)| {
        thread::spawn(move || {
            let mut sent = false;
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match channels.control.receive::<Control>().unwrap() {
                    Some(Control::Authority { .. }) if privacy && !sent => {
                        channels
                            .control
                            .send(WorkerEvent::Privacy {
                                acquired_at_us: monotonic_us(),
                                muted: true,
                            })
                            .unwrap();
                        sent = true;
                    }
                    Some(Control::Stop) => break,
                    _ => {}
                }
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(1));
            }
            drop(input);
            channels
        })
    });
    let mut rig = Rig {
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
        early_end_blocks: None,
        faint_level: None,
        speaker,
        capture,
        provider,
        privacy,
        ring: None.into(),
        camera: None,
        motor: None,
        ring_channel_ceiling: None,
        diagnostic_directory: None,
        noise_suppression: NoiseSuppression::On,
        turn_taking: TurnTaking::V1Baseline,
        playout_start: PlayoutStart::default(),
        endpoint: Some(endpoint),
    };
    let (mut controller, current, _) = active_reply();
    let mut reply = Some(current);
    let mut trace = SessionState::default();
    let result = conversation(
        &mut rig,
        &mut controller,
        Lifetime::Serve,
        &mut trace,
        StartupEvents::default(),
        &mut reply,
        &mut None,
        &Termination::inactive(),
        &FailureNotices::none(),
    );
    assert!(
        result
            .as_ref()
            .unwrap_err()
            .downcast_ref::<PhysicalMute>()
            .is_some()
    );
    let result = finish_active_run(
        &mut rig,
        &mut controller,
        &mut reply,
        &mut trace,
        &mut None,
        result,
    );
    for peer in peers {
        drop(peer.join().unwrap());
    }
    drop(rig);
    let _ = fs::remove_dir(&directory.path);
    (result, trace)
}

fn final_record(trace: &SessionState) -> Value {
    trace
        .iter()
        .find(|row| row["kind"] == "endpoint_worker_final")
        .cloned()
        .expect("endpoint teardown record")
}

#[test]
fn unconfirmed_endpoint_exit_fails_mute_cleanup_and_never_reopens_service() {
    use crate::service_privacy::{PhysicalMute, WaitOutcome};
    // A worker that ignores its closed input and is only killed after 1.5 s:
    // teardown's 500 ms bound passes with its process still alive.
    let limits = Limits {
        grace: Duration::from_millis(1_500),
        ..Limits::default()
    };
    let endpoint = scripted_process(&[ready()], "exec sleep 60", limits);
    let (result, mut trace) = mute_with_endpoint(endpoint);
    let error = result.unwrap_err();
    assert!(
        error.downcast_ref::<PhysicalMute>().is_none(),
        "unconfirmed endpoint exit kept a restartable mute: {error}"
    );
    assert!(error.to_string().contains("physical mute"), "{error}");
    assert!(
        error
            .to_string()
            .contains("endpoint worker exit not confirmed"),
        "{error}"
    );
    let record = final_record(&trace);
    assert_eq!(record["reaped"], false, "{record}");
    assert_eq!(record["teardown"], "Pending");
    let waits = std::cell::Cell::new(0);
    let mut failure = Some(error);
    let restarted = crate::coordinator::service::supervise(
        &mut trace,
        &mut None,
        &Termination::inactive(),
        |_, _| {
            waits.set(waits.get() + 1);
            Ok(WaitOutcome::Allowed)
        },
        |_, _| {
            Err(failure
                .take()
                .expect("must never reopen after failed cleanup"))
        },
    );
    assert!(restarted.is_err());
    assert_eq!(waits.get(), 1);
}

#[test]
fn confirmed_endpoint_exit_keeps_the_restartable_mute() {
    use crate::service_privacy::PhysicalMute;
    let endpoint = scripted_process(&[ready()], "exec sleep 60", Limits::default());
    let (result, trace) = mute_with_endpoint(endpoint);
    let error = result.unwrap_err();
    assert!(
        error.downcast_ref::<PhysicalMute>().is_some(),
        "confirmed teardown lost the mute: {error}"
    );
    let record = final_record(&trace);
    assert_eq!(record["reaped"], true, "{record}");
    assert_eq!(record["teardown"], "Reaped");
}

// Pilot stress: the conservative `smart-turn` path through the real loop.

fn delayed(probability: f32, delay: Duration) -> Box<dyn Classifier> {
    Box::new(Delayed {
        probability,
        delay,
        pending: None,
        requests: Arc::new(Mutex::new(Vec::new())),
    })
}

fn bases(session: &Session) -> Vec<(String, Option<String>, u64)> {
    endpoint_rows(session)
        .iter()
        .map(|row| {
            let endpoint = &row["endpoint"];
            (
                endpoint["basis"].as_str().unwrap().to_owned(),
                endpoint["fallback"].as_str().map(str::to_owned),
                endpoint["silence_ms"].as_u64().unwrap(),
            )
        })
        .collect()
}

#[test]
fn a_worker_still_loading_leaves_the_baseline_window() {
    let _one = one_session();
    // Ready only after 6 s: the whole session runs while it loads.
    let endpoint = scripted_process(&[], "sleep 6; exit 0", Limits::default());
    let session = run_process_with_privacy_tolerance(4, one_question, endpoint);
    session.result.as_ref().unwrap();
    assert_eq!(
        bases(&session),
        [("silence_fallback".into(), Some("loading".into()), 1_200)]
    );
}

#[test]
fn verdicts_after_the_result_timeout_fall_back_on_every_turn() {
    let _one = one_session();
    fn two(block: u64) -> f32 {
        if (50..150).contains(&block) || (300..400).contains(&block) {
            SPEECH
        } else {
            QUIET
        }
    }
    // Every verdict arrives 700 ms after its request, beyond the 500 ms bound.
    let session = run_scenario(6, two, delayed(0.99, Duration::from_millis(700)));
    session.result.as_ref().unwrap();
    assert_eq!(
        bases(&session),
        [
            ("silence_fallback".into(), Some("timeout".into()), 1_200),
            ("silence_fallback".into(), Some("timeout".into()), 1_200),
        ]
    );
}

#[test]
fn rapid_resumes_never_end_the_turn_early() {
    let _one = one_session();
    // Five bursts separated by 300 ms pauses, each judged complete.
    fn bursts(block: u64) -> f32 {
        let speaking = (50..330).contains(&block) && (block - 50) % 56 < 26;
        if speaking { SPEECH } else { QUIET }
    }
    let reply = ProviderReply {
        after_end: Duration::from_millis(200),
        packets: 2,
    };
    let session = run_scenario_with(
        5,
        bursts,
        delayed(0.99, Duration::from_millis(50)),
        Some(reply),
    );
    session.result.as_ref().unwrap();
    assert_eq!(bases(&session), [("model_complete".into(), None, 1_000)]);
    let seen = &session.seen;
    assert_eq!(seen.requests.len(), 1, "{:?}", seen.requests);
    // Every resumed burst reached the provider in the one request.
    assert!(
        seen.requests[0].audio_blocks >= 5 * 26,
        "{:?}",
        seen.requests
    );
    assert!(!seen.chunks.is_empty());
}

#[test]
fn a_complete_verdict_ends_at_one_second_and_its_answer_plays() {
    let _one = one_session();
    let reply = ProviderReply {
        after_end: Duration::from_millis(300),
        packets: 3,
    };
    let session = run_scenario_with(
        5,
        one_question,
        delayed(0.95, Duration::from_millis(60)),
        Some(reply),
    );
    session.result.as_ref().unwrap();
    assert_eq!(bases(&session), [("model_complete".into(), None, 1_000)]);
    let seen = &session.seen;
    // Speech ends after block 199: the end of input follows 1.0 s of quiet,
    // and the answer is spoken only after it.
    let ended = seen.requests[0].ended_at.unwrap();
    assert!(
        ended >= seen.block(298) && ended < seen.block(330),
        "{seen:?}",
        seen = seen.requests
    );
    assert!(seen.chunks.first().unwrap().1 > ended);
    assert!(!session.events("speaker_first_write").is_empty());
}

/// A worker that fails its first request, then recovers after `recovery`.
struct Recovering {
    recovery: Duration,
    failed_at: Option<Instant>,
    requests: u64,
    pending: Option<(Token, Instant)>,
}

impl Classifier for Recovering {
    fn submit(&mut self, token: Token, _pcm: &[i16]) -> Submit {
        if self.availability() != Availability::Ready {
            return Submit::Unavailable;
        }
        if self.pending.is_some() {
            return Submit::Busy;
        }
        self.requests += 1;
        self.pending = Some((token, scenario_clock::instant()));
        Submit::Accepted
    }
    fn poll(&mut self) -> Option<Outcome> {
        let (token, at) = self.pending?;
        if scenario_clock::instant().duration_since(at) < Duration::from_millis(50) {
            return None;
        }
        self.pending = None;
        let result = if self.requests == 1 {
            self.failed_at = Some(scenario_clock::instant());
            Err("worker exited: signal 9".into())
        } else {
            Ok(Verdict {
                probability: 0.95,
                ..Verdict::default()
            })
        };
        Some(Outcome {
            token,
            worker: if self.requests == 1 { 1 } else { 2 },
            request: self.requests,
            result,
        })
    }
    fn availability(&self) -> Availability {
        match self.failed_at {
            Some(at) if scenario_clock::instant().duration_since(at) < self.recovery => {
                Availability::Recovering("worker exited: signal 9".into())
            }
            _ => Availability::Ready,
        }
    }
    fn cancel(&mut self) {
        self.pending = None;
    }
}

#[test]
fn a_worker_failure_falls_back_through_recovery_and_then_resumes() {
    let _one = one_session();
    fn three(block: u64) -> f32 {
        if (50..150).contains(&block) || (300..380).contains(&block) || (560..640).contains(&block)
        {
            SPEECH
        } else {
            QUIET
        }
    }
    let endpoint = Box::new(Recovering {
        recovery: Duration::from_secs(3),
        failed_at: None,
        requests: 0,
        pending: None,
    });
    let session = run_scenario(9, three, endpoint);
    session.result.as_ref().unwrap();
    assert_eq!(
        bases(&session),
        [
            ("silence_fallback".into(), Some("failed".into()), 1_200),
            ("silence_fallback".into(), Some("recovering".into()), 1_200),
            ("model_complete".into(), None, 1_000),
        ]
    );
    let states: Vec<_> = session
        .events("endpoint_worker")
        .iter()
        .map(|row| row["availability"]["state"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(states, ["ready", "recovering", "ready"]);
}

#[test]
fn mute_then_reopen_never_overlaps_endpoint_workers() {
    use crate::service_privacy::PhysicalMute;
    let mut previous: Option<u64> = None;
    for cycle in 0..2 {
        let endpoint = scripted_process(&[ready()], "exec sleep 60", Limits::default());
        let (result, trace) = mute_with_endpoint(endpoint);
        assert!(
            result.unwrap_err().downcast_ref::<PhysicalMute>().is_some(),
            "cycle {cycle} not restartable"
        );
        let record = final_record(&trace);
        assert_eq!(record["reaped"], true, "{record}");
        let pid = record["report"]["pid"].as_u64().unwrap();
        // The previous cycle's worker is gone before this one ever existed.
        if let Some(old) = previous {
            let alive = Command::new("kill")
                .args(["-0", &old.to_string()])
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success();
            assert!(!alive, "worker {old} outlived its cycle");
            assert_ne!(old, pid);
        }
        previous = Some(pid);
    }
}

/// Duplex gates the same baseline detector: a confident complete verdict
/// still ends at 1.0 s and its answer plays, under the cached-speech cohorts'
/// duplex policy.
#[test]
fn duplex_complete_verdict_ends_at_one_second_and_its_answer_plays() {
    let _one = one_session();
    let reply = ProviderReply {
        after_end: Duration::from_millis(300),
        packets: 3,
    };
    let session = run_scenario_policy(
        TurnTaking::Duplex,
        5,
        one_question,
        delayed(0.95, Duration::from_millis(60)),
        Some(reply),
    );
    session.result.as_ref().unwrap();
    assert_eq!(
        session.events("endpoint_policy")[0]["turn_taking"],
        "duplex"
    );
    assert_eq!(bases(&session), [("model_complete".into(), None, 1_000)]);
    let ended = session.seen.requests[0].ended_at.unwrap();
    assert!(session.seen.chunks.first().unwrap().1 > ended);
    assert!(!session.events("speaker_first_write").is_empty());
}

/// Under duplex, a missing model still ends every pause on the unchanged
/// 1.2 s silence window, with the worker's failure recorded.
#[test]
fn duplex_missing_model_session_ends_on_the_unchanged_silence_fallback() {
    let _one = one_session();
    let missing = Response::Error {
        request: 0,
        reason: "model /lamp/smart-turn-v3.2-gpu.onnx: No such file or directory".into(),
    };
    let session = run_process_policy_with_privacy_tolerance(
        TurnTaking::Duplex,
        4,
        one_question,
        scripted_process(&[missing], "exit 2", Limits::default()),
    );
    session.result.as_ref().unwrap();
    let endpoints = endpoint_rows(&session);
    assert_eq!(endpoints.len(), 1, "{endpoints:?}");
    assert_eq!(endpoints[0]["endpoint"]["basis"], "silence_fallback");
    assert_eq!(endpoints[0]["includes_silence_wait_ms"], 1_200);
    assert!(session.seen.provider.contains(&"start"));
    assert_eq!(session.events("endpoint_worker_final")[0]["reaped"], true);
}
