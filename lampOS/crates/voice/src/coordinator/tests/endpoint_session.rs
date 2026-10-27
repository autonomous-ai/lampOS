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
    Availability, Classifier, Limits, Monitor, Outcome, Submit, Teardown, Token, Verdict,
    WorkerCommand,
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
/// The amplitude of each block, by block index, where a scenario sets it.
type Levels = fn(u64) -> i16;

#[derive(Default)]
struct Seen {
    provider: Vec<&'static str>,
    /// Every request the provider peer saw, in order.
    requests: Vec<SeenRequest>,
    /// Every speech chunk dispatched to the speaker: its turn and when.
    chunks: Vec<(u64, Instant)>,
    /// Each chunk's samples summed, in order: what the speaker was handed.
    chunk_sums: Vec<i64>,
    /// When the speaker reported its authority lapsed ([`Lamp::lapses_after`]).
    lapsed_at: Option<Instant>,
    /// Chunks the speaker refused after that, for the answer it lapsed in.
    refused: Vec<(u64, Instant)>,
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
    /// Whether its Start said it goes on with the request before (#782).
    continues: bool,
    audio_blocks: usize,
    ended_at: Option<Instant>,
}

impl Seen {
    /// Capture block `index` (1-based) as an instant.
    fn block(&self, index: u64) -> Instant {
        self.capture_started.expect("capture started") + Duration::from_millis(10 * (index - 1))
    }
}

/// How the emulated provider answers each request after its end; with
/// `only_first`, it answers the first request and no later one.
#[derive(Clone, Copy)]
struct ProviderReply {
    after_end: Duration,
    packets: usize,
    only_first: bool,
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

/// Whether a speech block, by its index and amplitude, holds a person over
/// the lamp's voice, which the render then does not explain (`Some(true)`),
/// is the lamp's own voice (`Some(false)`), or cannot be told yet, the lamp's
/// sound not yet matched in this reply (`None`).
type Over = fn(u64, i16) -> Option<bool>;

/// Speech at ±1,000 or more is plainly over the lamp; quieter talk is under
/// its voice, which explains the microphone.
fn over_at_1000(_block: u64, level: i16) -> Option<bool> {
    Some(level >= 1_000)
}

/// With `echo`, each block carries the canceller's evidence as lamp-4ace's
/// capture worker gives it: no render before the first answer is written,
/// and from then on the lamp's own voice, or a person over it as `echo`
/// says.
fn capture_peer(
    mut channels: WorkerChannels,
    script: Script,
    levels: Option<Levels>,
    echo: Option<Over>,
    seen: Arc<Mutex<Seen>>,
    starts_after: Duration,
) -> PeerStep {
    let mut generation = 0;
    let mut asked: Option<u64> = None;
    let mut started = None;
    let mut sequence = 0u64;
    Box::new(move || {
        for _ in 0..PEER_STEP_BUDGET {
            let Some(control) = channels.control.receive::<Control>()? else {
                break;
            };
            match control {
                Control::Authority { snapshot } => generation = snapshot.microphone_generation(),
                Control::StartCapture if asked.is_none() => asked = Some(monotonic_us()),
                Control::Stop => return Ok(false),
                _ => {}
            }
        }
        // A slow microphone (#1567) starts `starts_after` past its request.
        if started.is_none()
            && asked.is_some_and(|asked| monotonic_us() >= asked + starts_after.as_micros() as u64)
        {
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
            let level = match levels {
                Some(levels) => levels(sequence),
                None if probability >= 0.5 => 2_000,
                None => 20,
            };
            let frame = MicrophoneFrame {
                raw: None,
                linear: None,
                epoch: 1,
                dsp_epoch: 1,
                privacy_generation: generation,
                frame_sequence: sequence,
                read_completed_at_us: at,
                probability,
                echo: echo.map(|over| {
                    use super::duplex::{OVER, OWN, evidence};
                    if seen.lock().unwrap().chunks.is_empty() {
                        evidence(lamp_audio::evidence::QUIET_BLOCKS_CEILING, None)
                    } else if probability < 0.5 {
                        evidence(0, OWN)
                    } else {
                        match over(sequence, level) {
                            Some(true) => evidence(0, OVER),
                            Some(false) => evidence(0, OWN),
                            None => evidence(0, None),
                        }
                    }
                }),
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
            let mut step = capture_peer(peer, |_| QUIET, None, None, seen.clone(), Duration::ZERO);
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
                receipt: None,
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
    ready_after: Option<Duration>,
    refused: Option<crate::provider_worker::Fault>,
    shape: Option<fn(u64) -> i16>,
    words_end: Option<crate::unfinished_words::Trailing>,
) -> PeerStep {
    assert!(reply.is_none_or(|reply| reply.packets <= PEER_STEP_BUDGET));
    let mut ready = false;
    // With `refused`, the first session is refused at once, as the
    // provider worker says it when the model's account is out of quota
    // (#1567), and the link that comes later is a session of its own.
    let mut refused_at: Option<u64> = None;
    // With `ready_after`, the link is down until that long after the
    // peer's first step: a service started with no internet (#830).
    let mut first_step_us: Option<u64> = None;
    // Output is credited: the coordinator grants two packets, then more.
    let (mut granted, mut sent) = (2u64, 0u64);
    let mut outbox: VecDeque<ProviderOutput> = VecDeque::new();
    let mut due: VecDeque<(Instant, u64)> = VecDeque::new();
    let mut sequence = 0u64;
    Box::new(move || {
        let late = ready_after.is_some_and(|after| {
            let first = *first_step_us.get_or_insert_with(monotonic_us);
            monotonic_us().saturating_sub(first) < after.as_micros() as u64
        });
        if let Some(reason) = refused
            && refused_at.is_none()
        {
            let at = monotonic_us();
            channels.control.send(WorkerEvent::ProviderLink {
                link: crate::provider_link::ProviderLink {
                    revision: 1,
                    session: 1,
                    observed_at_us: at,
                    source_clock: None,
                    state: ProviderLinkState::Unavailable { reason },
                },
            })?;
            refused_at = Some(at);
        }
        if !ready && !late {
            let at = monotonic_us();
            let (revision, session, state) = match refused_at.zip(refused) {
                Some((since, reason)) => (
                    2,
                    2,
                    ProviderLinkState::Ready {
                        setup_us: 1_000,
                        attempts: 1,
                        context: LinkContext::Fresh,
                        outage_started_at_us: Some(since),
                        after: Some(reason),
                    },
                ),
                None => (
                    1,
                    1,
                    ProviderLinkState::Ready {
                        setup_us: 1_000,
                        attempts: 1,
                        context: LinkContext::Initial,
                        outage_started_at_us: None,
                        after: None,
                    },
                ),
            };
            channels.control.send(WorkerEvent::ProviderLink {
                link: crate::provider_link::ProviderLink {
                    revision,
                    session,
                    observed_at_us: at,
                    source_clock: None,
                    state,
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
                    request,
                    session,
                    continues,
                    ..
                } => {
                    seen.provider.push("start");
                    seen.requests.push(SeenRequest {
                        request,
                        continues,
                        audio_blocks: 0,
                        ended_at: None,
                    });
                    outbox.push_back(ProviderOutput::Started {
                        request,
                        session,
                        waiting_for_barrier: false,
                    });
                    if let Some(class) = words_end {
                        outbox.push_back(ProviderOutput::WordsEnd {
                            request,
                            unfinished: Some(class),
                        });
                    }
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
                    if let Some(reply) = reply
                        && !(reply.only_first && seen.requests.len() > 1)
                    {
                        due.push_back((scenario_clock::instant() + reply.after_end, request));
                    }
                }
                ProviderInput::Written { .. } => seen.provider.push("written"),
                ProviderInput::Called { .. } => seen.provider.push("called"),
                ProviderInput::Paused { .. } => seen.provider.push("paused"),
                ProviderInput::CountOnly { .. } => seen.provider.push("count_only"),
                ProviderInput::StepCall { .. } => seen.provider.push("step_call"),
                ProviderInput::Onboarding { .. } => seen.provider.push("onboarding"),
                ProviderInput::FaceQuestion { .. } | ProviderInput::FaceConsentDelivery { .. } => {
                    panic!("face consent is disabled in this fixture")
                }
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
            for packet in 0..packets as u64 {
                sequence += 1;
                outbox.push_back(ProviderOutput::Audio {
                    request,
                    sequence,
                    provider_event_at_us: monotonic_us(),
                    source_frames: 960,
                    source_offset: 0,
                    samples: (0..960)
                        .map(|index| shape.map_or(1_000, |shape| shape(packet * 960 + index)))
                        .collect(),
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
/// mask sees genuine write evidence. With `retires`, each chunk is drained
/// as it is accepted and an answer's last one retires it, so the lamp falls
/// silent when its answer is written; without, it plays to the end. With
/// `lapses_after`, see [`Lamp::lapses_after`].
fn speaker_peer(
    mut channels: WorkerChannels,
    seen: Arc<Mutex<Seen>>,
    retires: bool,
    lapses_after: Option<usize>,
) -> PeerStep {
    let mut generation = 0;
    let mut frame = 1_000u64;
    let mut epoch = 1u64;
    let mut accepted = 0usize;
    let mut lapsed = None;
    Box::new(move || {
        if !drain_control(&mut channels, &mut generation)? {
            return Ok(false);
        }
        for _ in 0..PEER_STEP_BUDGET {
            let Some(chunk) = channels.data.receive::<SpeakerChunk>()? else {
                break;
            };
            let owner = chunk.permit.owner();
            if lapsed == Some(owner) {
                seen.lock()
                    .unwrap()
                    .refused
                    .push((owner.turn(), scenario_clock::instant()));
                channels.control.send(WorkerEvent::PlaybackRejected {
                    turn: owner.turn(),
                    sequence: chunk.chunk_sequence,
                    reason: lamp_turn_owner::Error::StaleOwner.to_string(),
                    lapsed: false,
                })?;
                continue;
            }
            seen.lock()
                .unwrap()
                .chunks
                .push((owner.turn(), scenario_clock::instant()));
            seen.lock()
                .unwrap()
                .chunk_sums
                .push(chunk.samples.iter().map(|&sample| i64::from(sample)).sum());
            let frames = chunk.samples.len();
            let at = monotonic_us();
            channels.control.send(WorkerEvent::Playback {
                owner,
                first_sample: PlaybackCursor { epoch, frame },
                end_sample: PlaybackCursor {
                    epoch,
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
            accepted += 1;
            if lapses_after == Some(accepted) {
                // Held up past its lease: the device is reset and the
                // answer's queued speech discarded, as `lapse()` reports it.
                let at = monotonic_us();
                channels.control.send(WorkerEvent::PlaybackClockReset {
                    previous: lamp_audio::ownership::QueuedRange {
                        epoch,
                        retired_through: frame - frames as u64,
                        accepted_through: frame,
                    },
                    next_epoch: epoch + 1,
                    observed_at_us: at,
                })?;
                channels.control.send(WorkerEvent::PlaybackDiscarded {
                    owner,
                    reason: crate::wire::PlaybackDiscardReason::ExpiredState,
                    queued_frames: frames,
                    other_owners: false,
                    observed_at_us: at,
                })?;
                epoch += 1;
                lapsed = Some(owner);
                seen.lock().unwrap().lapsed_at = Some(scenario_clock::instant());
                continue;
            }
            if retires {
                let last = PlaybackCursor { epoch, frame };
                channels.control.send(WorkerEvent::SpeechDrained {
                    owner,
                    sequence: chunk.chunk_sequence,
                    last_sample: last,
                    observed_at_us: at,
                })?;
                if chunk.end_of_speech {
                    channels.control.send(WorkerEvent::SpeechRetired {
                        owner,
                        sequence: chunk.chunk_sequence,
                        final_sample: last,
                        observed_at_us: at,
                    })?;
                }
            }
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
        Lamp::default(),
    )
}

/// A real endpoint worker whose state no longer changes within the
/// scenario: it has failed, or it stays loading far longer than the scenario
/// lasts. The host waits for that state before the logical clock starts, so
/// the real process cannot race the stepped scenario (#823). Its startup,
/// exit and teardown stay on the host clock.
fn run_settled_process(
    turn_taking: TurnTaking,
    seconds: u64,
    script: Script,
    endpoint: EndpointRig,
    settled: fn(&Availability) -> bool,
) -> Session {
    let monitor = endpoint.monitor.as_ref().expect("real endpoint").clone();
    let by = Instant::now() + Duration::from_secs(5);
    while !settled(&monitor.availability()) {
        assert!(
            Instant::now() < by,
            "endpoint worker never settled: {:?}",
            monitor.report()
        );
        thread::sleep(Duration::from_millis(1));
    }
    run_policy_with_peers(
        turn_taking,
        seconds,
        script,
        endpoint,
        None,
        None,
        PeerMode::Stepped,
        Lamp::default(),
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

/// An in-process classifier on the logical scenario clock. A real
/// ProcessClassifier's readiness, deadlines and teardown stay on the host
/// clock; only a settled one runs in a stepped scenario
/// ([`run_settled_process`]).
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
        Lamp::default(),
    )
}

/// What the lamp is set to beyond its turn taking; the default is how
/// these scenarios have always run.
#[derive(Clone, Copy, Default)]
struct Lamp {
    end_silence_blocks: Option<u16>,
    cut_keeps_name: bool,
    asker_first: crate::asker_first::Mode,
    levels: Option<Levels>,
    /// The speaker retires each answer once it is written.
    retires: bool,
    /// The microphone's blocks carry echo evidence ([`capture_peer`]).
    echo: Option<Over>,
    /// The speaker's authority lapses after this many chunks, as the
    /// speaker worker reports it (#1178): its reset, then the discard of
    /// that answer's queued speech as `ExpiredState`. It refuses the rest of
    /// that answer, as one revoked.
    lapses_after: Option<usize>,
    /// The provider's link comes up only this long after it starts (#830).
    provider_ready_after: Option<Duration>,
    /// The provider refuses the first session at once for this reason,
    /// before the link of `provider_ready_after` (#1567).
    provider_refused: Option<crate::provider_worker::Fault>,
    /// The microphone starts only this long after it is asked to (#1567).
    capture_after: Option<Duration>,
    /// `LAMP_START_OFFLINE`, for this run's thread (#830).
    start_offline: Option<bool>,
    /// Run as the service does (`Lifetime::Serve`), ended by `mute_at`,
    /// not after `seconds`: starting offline is only for a service.
    serve: bool,
    /// Each answer's samples by their index in it; 1,000 throughout without.
    reply_shape: Option<fn(u64) -> i16>,
    /// `LAMP_LISTEN_GAPS`, for this run's thread (#828).
    listen_gaps: bool,
    /// `directed-fixture --fixture-at-ms` (diagnostic only).
    fixture_at_ms: Option<u64>,
    /// `LAMP_SECOND_EARLY_FINALIZE`: a line's words asked for at a pause.
    early_finalize: bool,
    /// `LAMP_SECOND_UNFINISHED_WAIT`: a due end held while the words so far
    /// end unfinished.
    unfinished_wait: bool,
    /// The provider tells this end of each line's words so far as it starts.
    words_end: Option<crate::unfinished_words::Trailing>,
}

#[allow(clippy::too_many_arguments)]
fn run_policy_with_peers(
    turn_taking: TurnTaking,
    seconds: u64,
    script: Script,
    endpoint: EndpointRig,
    reply: Option<ProviderReply>,
    mute_at: Option<u64>,
    mode: PeerMode,
    lamp: Lamp,
) -> Session {
    run_policy_with_peers_held(
        turn_taking,
        seconds,
        script,
        endpoint,
        reply,
        mute_at,
        mode,
        lamp,
        None,
    )
}

/// What a stepped scenario waits for on the host before each step: an event
/// of a real process that logical time must not run past.
type Hold = Box<dyn FnMut() -> io::Result<()>>;

/// As [`run_policy_with_peers`]; when stepped, `hold` runs before each step.
#[allow(clippy::too_many_arguments)]
fn run_policy_with_peers_held(
    turn_taking: TurnTaking,
    seconds: u64,
    script: Script,
    endpoint: EndpointRig,
    reply: Option<ProviderReply>,
    mute_at: Option<u64>,
    mode: PeerMode,
    lamp: Lamp,
    mut hold: Option<Hold>,
) -> Session {
    let fixture_privacy = mode.fixture_privacy();
    if fixture_privacy.is_some() {
        require_real_process_clock();
    }
    let dir = SessionDirectory::create().unwrap();
    // Children that end as their Stop is sent (their input closed), so the
    // shutdown does not wait out its 300 ms grace to kill four that cannot
    // stop, as `exec sleep 60` could not (#1806). The ready fixture's echo
    // proves each child reads its input before a stepped clock moves.
    let (mut capture, capture_peer_channels, capture_input) =
        Worker::cooperative_ready_fixture(&dir.path, "capture");
    let (mut speaker, speaker_peer_channels, speaker_input) =
        Worker::cooperative_ready_fixture(&dir.path, "speaker");
    let (mut provider, provider_peer_channels, provider_input) =
        Worker::cooperative_ready_fixture(&dir.path, "provider");
    let (mut privacy, privacy_peer_channels, privacy_input) =
        Worker::cooperative_ready_fixture(&dir.path, "privacy");
    capture.end_on_stop(capture_input);
    speaker.end_on_stop(speaker_input);
    provider.end_on_stop(provider_input);
    privacy.end_on_stop(privacy_input);
    // Starting children is real OS work. Enter only after their startup barriers
    // and keep the scope through shutdown and collection of scenario timestamps.
    let mut scope = match mode {
        PeerMode::RealProcessWithPrivacyTolerance => None,
        // A late link runs past the startup budget, so its scenario is
        // longer, still stepped.
        PeerMode::Stepped => Some(
            scenario_clock::Scope::enter(if lamp.provider_ready_after.is_some() {
                Duration::from_secs(60)
            } else {
                Duration::from_secs(12)
            })
            .unwrap(),
        ),
    };
    let stop = Arc::new(AtomicBool::new(false));
    let seen = Arc::new(Mutex::new(Seen::default()));
    let steps = [
        capture_peer(
            capture_peer_channels,
            script,
            lamp.levels,
            lamp.echo,
            seen.clone(),
            lamp.capture_after.unwrap_or_default(),
        ),
        privacy_peer(privacy_peer_channels, seen.clone(), mute_at),
        provider_peer(
            provider_peer_channels,
            seen.clone(),
            reply,
            lamp.provider_ready_after,
            lamp.provider_refused,
            lamp.reply_shape,
            lamp.words_end,
        ),
        speaker_peer(
            speaker_peer_channels,
            seen.clone(),
            lamp.retires,
            lamp.lapses_after,
        ),
    ];
    let peers: Vec<_> = if let Some(scope) = scope.as_mut() {
        let mut steps = steps.map(|step| (true, step));
        scope
            .set_pump(move |_| {
                if let Some(hold) = hold.as_mut() {
                    hold()?;
                }
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
        expression_out: None,
        second: None,
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: lamp.end_silence_blocks,
        early_end_blocks: None,
        early_finalize: lamp.early_finalize,
        open_name: false,
        call_takes_next: false,
        unfinished_wait: lamp.unfinished_wait,
        faint_level: None,
        provider,
        capture,
        speaker,
        privacy,
        privacy_controls: false,
        capture_held: false,
        privacy_receipts: false,
        ring: None.into(),
        camera: None,
        motor: None,
        ring_channel_ceiling: None,
        diagnostic_directory: None,
        noise_suppression: NoiseSuppression::On,
        turn_taking,
        playout_start: PlayoutStart::default(),
        fixture_at_ms: lamp.fixture_at_ms,
        endpoint: Some(endpoint),
        call_wait_blocks: None,
        cut_keeps_name: lamp.cut_keeps_name,
        asker_first: lamp.asker_first,
        follows_on: false,
        paused: Box::new(|| false),
    };
    let mut owner = Controller::new(new_boot().unwrap(), now());
    let mut trace = SessionState::finite();
    let mut reply = None;
    let started = Instant::now();
    crate::session_lifetime::set_start_offline_for_test(lamp.start_offline);
    crate::listen_gaps::set_for_test(Some(lamp.listen_gaps));
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
            if lamp.serve {
                Lifetime::Serve
            } else {
                Lifetime::Finite(seconds)
            },
            &mut trace,
            StartupEvents::default(),
            &mut reply,
            &mut None,
            &Termination::inactive(),
            &FailureNotices::none(),
        )
    };
    let elapsed = started.elapsed();
    crate::session_lifetime::set_start_offline_for_test(None);
    crate::listen_gaps::set_for_test(None);
    // Only the endpoint record matters here.
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

/// Owns the endpoint until the session takes it, then keeps the supervisor's
/// exit receipt through the assertions. This final fixture wait cannot change
/// the session's already-recorded cleanup outcome or its original deadline.
struct EndpointCleanup {
    endpoint: Option<EndpointRig>,
    monitor: Monitor,
}

impl EndpointCleanup {
    fn new(endpoint: EndpointRig) -> Self {
        let monitor = endpoint.monitor.as_ref().expect("real endpoint").clone();
        Self {
            endpoint: Some(endpoint),
            monitor,
        }
    }

    fn take(&mut self) -> EndpointRig {
        self.endpoint.take().expect("endpoint taken once")
    }
}

impl Drop for EndpointCleanup {
    fn drop(&mut self) {
        // Before handoff, close our classifier first. After handoff, the
        // session owns it and drops it on return or unwind before this guard.
        drop(self.endpoint.take());
        let teardown = self.monitor.wait_finished(Duration::from_secs(5));
        if teardown != Teardown::Reaped {
            let message = format!(
                "endpoint fixture cleanup did not reap within 5 s: {teardown:?}; {:?}",
                self.monitor.report()
            );
            if thread::panicking() {
                // Preserve the existing failure instead of double-panicking.
                eprintln!("{message}");
            } else {
                panic!("{message}");
            }
        }
    }
}

fn ready() -> Response {
    Response::Ready {
        protocol: PROTOCOL,
        load_us: 1,
        cold_us: 1,
        model_sha256: [0; 32],
    }
}

#[test]
fn endpoint_fixture_reaps_on_panic_before_and_after_handoff() {
    #[derive(Debug)]
    struct ScenarioFailure;

    fn fail_in_helper(_endpoint: EndpointRig) -> ! {
        std::panic::panic_any(ScenarioFailure);
    }

    for handoff in [false, true] {
        let endpoint = scripted_process(
            &[ready()],
            "exec sleep 60",
            Limits {
                grace: Duration::from_millis(1_500),
                ..Limits::default()
            },
        );
        let monitor = endpoint.monitor.as_ref().unwrap().clone();
        let observed = monitor.clone();
        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let mut cleanup = EndpointCleanup::new(endpoint);
            let ready_by = Instant::now() + Duration::from_secs(5);
            while observed.availability() != Availability::Ready {
                assert!(
                    Instant::now() < ready_by,
                    "endpoint fixture never became ready: {:?}",
                    observed.report()
                );
                thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(observed.wait_finished(Duration::ZERO), Teardown::Pending);
            if handoff {
                fail_in_helper(cleanup.take());
            }
            std::panic::panic_any(ScenarioFailure);
        }));
        assert!(failed.unwrap_err().is::<ScenarioFailure>());
        // A zero wait proves unwind completed cleanup, rather than making this
        // assertion finish the supervisor's work after the guard returned.
        assert_eq!(monitor.wait_finished(Duration::ZERO), Teardown::Reaped);
        assert_eq!(monitor.report().spawned, 1);
        assert_eq!(monitor.report().kills, 1);
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

/// 400 ms of speech, a 200 ms pause, then 900 ms more, then quiet.
fn question_with_a_pause(block: u64) -> f32 {
    if (50..90).contains(&block) || (110..200).contains(&block) {
        SPEECH
    } else {
        QUIET
    }
}

/// Jarvis on #2102 (P3 2): through the coordinator, a line's pause reaches
/// the provider as `Paused` with `LAMP_SECOND_EARLY_FINALIZE`: once at
/// 150 ms of the pause in the line, once as its last quiet begins, and
/// before its end. Off, none goes.
#[test]
fn a_lines_pause_reaches_the_provider_only_with_the_switch() {
    let _one = one_session();
    let run = |early_finalize| {
        run_policy_with_peers(
            TurnTaking::V1Baseline,
            4,
            question_with_a_pause,
            EndpointRig::double(delayed(0.01, Duration::from_millis(60))),
            None,
            None,
            PeerMode::Stepped,
            Lamp {
                early_finalize,
                ..Lamp::default()
            },
        )
    };
    let on = run(true);
    on.result.as_ref().unwrap();
    let kinds: Vec<&str> = on
        .seen
        .provider
        .iter()
        .copied()
        .filter(|kind| *kind != "audio")
        .collect();
    assert_eq!(kinds, ["start", "paused", "paused", "end"], "{kinds:?}");
    let off = run(false);
    off.result.as_ref().unwrap();
    assert!(!off.seen.provider.contains(&"paused"));
    assert!(off.seen.provider.contains(&"end"));
}

/// The Reviewer-lead on #3045 (P3): through the coordinator, the provider's
/// `WordsEnd` reaches the detector only with `LAMP_SECOND_UNFINISHED_WAIT`,
/// and the endpoint record's quiet includes the hold. An uncertain verdict
/// ends on the 1.2 s window, held 1.3 s more; the model's incomplete wait
/// (1.5 s) is not held again (El's P2).
#[test]
fn words_end_holds_the_lines_end_only_with_the_switch() {
    let _one = one_session();
    let run = |unfinished_wait, probability| {
        let session = run_policy_with_peers(
            TurnTaking::V1Baseline,
            6,
            one_question,
            EndpointRig::double(delayed(probability, Duration::from_millis(60))),
            None,
            None,
            PeerMode::Stepped,
            Lamp {
                unfinished_wait,
                words_end: Some(crate::unfinished_words::Trailing::Filler),
                ..Lamp::default()
            },
        );
        session.result.as_ref().unwrap();
        let rows = endpoint_rows(&session);
        assert_eq!(rows.len(), 1, "{rows:?}");
        let row = rows[0];
        (
            row["endpoint"]["basis"].as_str().unwrap().to_owned(),
            row["endpoint"]["unfinished_wait_ms"].as_u64(),
            row["includes_silence_wait_ms"].as_u64().unwrap(),
        )
    };
    assert_eq!(
        run(true, 0.5),
        ("model_uncertain".into(), Some(1_300), 2_500)
    );
    assert_eq!(run(false, 0.5), ("model_uncertain".into(), None, 1_200));
    assert_eq!(run(true, 0.001), ("model_incomplete".into(), None, 1_500));
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
    let session = run_settled_process(
        TurnTaking::V1Baseline,
        4,
        one_question,
        scripted_process(&[missing], "exit 2", Limits::default()),
        |state| matches!(state, Availability::Failed(_)),
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
    let endpoint = scripted_process(&[ready()], "exec sleep 60", limits);
    // Stepped, the conversation's own time is logical; the worker's
    // supervisor, its 700 ms hang detection, its backoff and its respawn stay
    // real. Logical time waits for them, in their real order: the loop gives
    // up on the request at its own 500 ms result timeout; at the hang bound
    // after the request went out, logical time holds until the supervisor has
    // found the hang (Recovering, which the loop's next pass records), and
    // then until it has respawned the worker. A busy host only makes the
    // wait longer (#823).
    let monitor = endpoint.monitor.as_ref().expect("real endpoint").clone();
    let hang = limits.hang;
    let mut stage = 0;
    let mut requested = None;
    let hold: Hold = Box::new(move || {
        let waited = |done: &dyn Fn() -> bool, what: &str| -> io::Result<()> {
            let by = Instant::now() + Duration::from_secs(5);
            while !done() {
                if Instant::now() >= by {
                    return Err(io::Error::other(format!(
                        "endpoint worker never {what}: {:?}",
                        monitor.report()
                    )));
                }
                thread::sleep(Duration::from_millis(1));
            }
            Ok(())
        };
        if stage == 0 && monitor.report().requests >= 1 {
            requested = Some(scenario_clock::instant());
            stage = 1;
        } else if stage == 1 && requested.is_some_and(|at| scenario_clock::instant() >= at + hang) {
            waited(
                &|| matches!(monitor.availability(), Availability::Recovering(_)),
                "found the hang",
            )?;
            stage = 2;
        } else if stage == 2 {
            waited(&|| monitor.report().spawned >= 2, "respawned")?;
            stage = 3;
        }
        Ok(())
    });
    let session = run_policy_with_peers_held(
        TurnTaking::V1Baseline,
        6,
        two_questions,
        endpoint,
        None,
        None,
        PeerMode::Stepped,
        Lamp::default(),
        Some(hold),
    );
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
    let (mut speaker, speaker_peer, speaker_input) =
        Worker::cooperative_fixture(&directory.path, "speaker");
    let (mut capture, capture_peer, capture_input) =
        Worker::cooperative_fixture(&directory.path, "capture");
    let (mut provider, provider_peer, provider_input) =
        Worker::cooperative_fixture(&directory.path, "provider");
    let (mut privacy, privacy_peer, privacy_input) =
        Worker::cooperative_fixture(&directory.path, "privacy");
    speaker.end_on_stop(speaker_input);
    capture.end_on_stop(capture_input);
    provider.end_on_stop(provider_input);
    privacy.end_on_stop(privacy_input);
    let peers = [
        (speaker_peer, false),
        (capture_peer, false),
        (provider_peer, false),
        (privacy_peer, true),
    ]
    .map(|(mut channels, privacy)| {
        thread::spawn(move || {
            let mut sent = false;
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match channels.control.receive::<Control>().unwrap() {
                    Some(Control::Authority { .. }) if privacy && !sent => {
                        channels
                            .control
                            .send(WorkerEvent::Privacy {
                                receipt: None,
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
            channels
        })
    });
    let mut rig = Rig {
        expression_out: None,
        second: None,
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
        early_end_blocks: None,
        early_finalize: false,
        open_name: false,
        call_takes_next: false,
        unfinished_wait: false,
        faint_level: None,
        speaker,
        capture,
        provider,
        privacy,
        privacy_controls: false,
        capture_held: false,
        privacy_receipts: false,
        ring: None.into(),
        camera: None,
        motor: None,
        ring_channel_ceiling: None,
        diagnostic_directory: None,
        noise_suppression: NoiseSuppression::On,
        turn_taking: TurnTaking::V1Baseline,
        playout_start: PlayoutStart::default(),
        fixture_at_ms: None,
        endpoint: Some(endpoint),
        call_wait_blocks: None,
        cut_keeps_name: false,
        asker_first: crate::asker_first::Mode::Off,
        follows_on: false,
        paused: Box::new(|| false),
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
    let mut cleanup = EndpointCleanup::new(endpoint);
    let (result, mut trace) = mute_with_endpoint(cleanup.take());
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
    // What was checked is done: the worker ended now, not at its 1.5 s
    // grace (#1806). The supervisor alone reaps it, so the PID is the
    // worker's until then; on a host slow enough to reach the grace first,
    // it is gone already and the kill finds nothing.
    let pid = cleanup.monitor.report().pid.expect("the worker's PID");
    let pid = rustix::process::Pid::from_raw(pid as i32).unwrap();
    let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
    drop(cleanup);
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
    // No Ready while the single owned worker sleeps for 6 s.
    let endpoint = scripted_process(&[], "exec sleep 6", Limits::default());
    let mut cleanup = EndpointCleanup::new(endpoint);
    let session = run_settled_process(
        TurnTaking::V1Baseline,
        4,
        one_question,
        cleanup.take(),
        |state| *state == Availability::Loading,
    );
    session.result.as_ref().unwrap();
    assert_eq!(
        bases(&session),
        [("silence_fallback".into(), Some("loading".into()), 1_200)]
    );
    drop(cleanup);
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
        only_first: false,
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
        only_first: false,
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
    // The alive check below can say yes, or its "gone" would pass unread.
    assert!(rustix::process::test_kill_process(rustix::process::getpid()).is_ok());
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
        // Asked of the kernel, not of a `kill -0` program: without procps
        // that is missing, and under qemu-user a missing one exits 127 and
        // read as "gone" (#1497).
        if let Some(old) = previous {
            let alive = rustix::process::Pid::from_raw(old as i32)
                .is_some_and(|old| rustix::process::test_kill_process(old).is_ok());
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
        only_first: false,
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
    let session = run_settled_process(
        TurnTaking::Duplex,
        4,
        one_question,
        scripted_process(&[missing], "exit 2", Limits::default()),
        |state| matches!(state, Availability::Failed(_)),
    );
    session.result.as_ref().unwrap();
    let endpoints = endpoint_rows(&session);
    assert_eq!(endpoints.len(), 1, "{endpoints:?}");
    assert_eq!(endpoints[0]["endpoint"]["basis"], "silence_fallback");
    assert_eq!(endpoints[0]["includes_silence_wait_ms"], 1_200);
    assert!(session.seen.provider.contains(&"start"));
    assert_eq!(session.events("endpoint_worker_final")[0]["reaped"], true);
}

/// lamp-4ace's path for #782 (`--endpoint smart-turn-fast`, its 600 ms
/// window, no `--early-end-ms`), as the reviewer found it on #809: a
/// confident verdict ends the turn 400 ms into a pause at a comma, and the
/// person goes on 100 ms later. The rest is a new request. With
/// `LAMP_CUT_KEEPS_NAME=on` its Start says it goes on with the first, so
/// the busy-room rule keeps the first part's name; without the switch the
/// Start and the record are as they were.
#[test]
fn a_turn_ended_at_a_comma_goes_on_in_the_next_request_only_with_the_switch() {
    let _one = one_session();
    // "Hey Lamp," and more (1 s), a 500 ms pause, the rest (0.8 s).
    fn comma(block: u64) -> f32 {
        if (50..150).contains(&block) || (200..280).contains(&block) {
            SPEECH
        } else {
            QUIET
        }
    }
    let run = |cut_keeps_name| {
        let session = run_comma(comma, None, cut_keeps_name);
        session.result.as_ref().unwrap();
        let (basis, _, silence_ms) = &bases(&session)[0];
        assert_eq!((basis.as_str(), *silence_ms), ("model_complete", 400));
        assert_eq!(
            session.seen.requests.len(),
            2,
            "{:?}",
            session.seen.requests
        );
        session
    };
    let on = run(true);
    let starts: Vec<_> = on.seen.requests.iter().map(|r| r.continues).collect();
    assert_eq!(starts, [false, true]);
    let admitted = on.events("input_admitted");
    assert_eq!(admitted[1]["continues"], true, "{admitted:?}");
    assert!(admitted[0].get("continues").is_none());

    let off = run(false);
    let starts: Vec<_> = off.seen.requests.iter().map(|r| r.continues).collect();
    assert_eq!(starts, [false, false]);
    assert!(
        off.events("input_admitted")
            .iter()
            .all(|row| row.get("continues").is_none())
    );
}

/// With the switch, speech is the same utterance going on only within a
/// second of the end. (Speech over an answer that has begun is cutting in;
/// `early_end`'s real provider, whose answers are played, covers that.)
#[test]
fn a_new_request_long_after_the_end_does_not_go_on() {
    let _one = one_session();
    // The pause is 1.8 s: the person spoke again 1.4 s after the end.
    fn long_pause(block: u64) -> f32 {
        if (50..150).contains(&block) || (330..410).contains(&block) {
            SPEECH
        } else {
            QUIET
        }
    }
    let late = run_comma(long_pause, None, true);
    late.result.as_ref().unwrap();
    let (basis, _, silence_ms) = &bases(&late)[0];
    assert_eq!((basis.as_str(), *silence_ms), ("model_complete", 400));
    let starts: Vec<_> = late.seen.requests.iter().map(|r| r.continues).collect();
    assert_eq!(starts, [false, false], "{:?}", late.seen.requests);
}

/// lamp-4ace's turn taking: duplex, the 600 ms window, a confident model.
fn run_comma(script: Script, reply: Option<ProviderReply>, cut_keeps_name: bool) -> Session {
    run_policy_with_peers(
        TurnTaking::Duplex,
        6,
        script,
        EndpointRig::double(delayed(0.95, Duration::from_millis(60))),
        reply,
        None,
        PeerMode::Stepped,
        Lamp {
            end_silence_blocks: Some(60),
            cut_keeps_name,
            ..Lamp::default()
        },
    )
}

/// One second of the asker at ±2,000 (blocks 50 to 150), an answer that
/// never plays, then `script`'s later speech at `levels`, with the rule as
/// `mode` says.
fn asker_then(mode: crate::asker_first::Mode, script: Script, levels: Levels) -> Session {
    let session = run_policy_with_peers(
        TurnTaking::Duplex,
        8,
        script,
        EndpointRig::double(delayed(0.95, Duration::from_millis(60))),
        None,
        None,
        PeerMode::Stepped,
        Lamp {
            end_silence_blocks: Some(60),
            asker_first: mode,
            levels: Some(levels),
            ..Lamp::default()
        },
    );
    session.result.as_ref().unwrap();
    session
}

/// The asker for a second, then someone else, 1.5 s later, for a second.
fn two_lines(block: u64) -> f32 {
    if (50..150).contains(&block) || (300..400).contains(&block) {
        SPEECH
    } else {
        QUIET
    }
}

/// #829, the first builder at 14:23: on lamp-4ace in the busy office at
/// 14:21, the answer to the person at the desk was awaited, and "¿Qué?"
/// from across the room was taken as a new request and replaced it. With
/// `LAMP_ASKER_FIRST=on`, speech well under the asker's takes no turn while
/// the answer is awaited, and the record says so once, with both levels.
/// The asker a little quieter than before still takes it; 8 dB down, which
/// leaning back from 30 to 60 cm can be, they do not (the reviewer's P3 on
/// #1092: the rule's cost). Recorded only, or without the switch, the quiet
/// speech takes the turn, as before.
#[test]
fn with_asker_first_quieter_talk_takes_no_turn_from_an_awaited_answer() {
    use crate::asker_first::Mode;
    let _one = one_session();
    // The second line 16.5 dB under the first (±300 against ±2,000).
    fn across_the_room(block: u64) -> i16 {
        match block {
            50..150 => 2_000,
            300..400 => 300,
            _ => 20,
        }
    }
    // Or 4.4 dB under it (±1,200): the asker, leaning back a little.
    fn leaning_back(block: u64) -> i16 {
        match block {
            50..150 => 2_000,
            300..400 => 1_200,
            _ => 20,
        }
    }
    // Or 8 dB under it (±800).
    fn leaning_far_back(block: u64) -> i16 {
        match block {
            50..150 => 2_000,
            300..400 => 800,
            _ => 20,
        }
    }
    let kept = asker_then(Mode::On, two_lines, across_the_room);
    assert_eq!(kept.seen.requests.len(), 1, "{:?}", kept.seen.requests);
    let said = kept.events("asker_first");
    assert_eq!(said.len(), 1, "{said:?}");
    // On the face's scale: ±2,000 is -24.3 dBFS, 178; ±300 is -40.8, 54.
    assert_eq!(
        (
            said[0]["asker_level"].as_u64(),
            said[0]["heard_level"].as_u64(),
            said[0]["acted"].as_bool()
        ),
        (Some(178), Some(54), Some(true)),
        "{said:?}"
    );
    assert_eq!(said[0]["turn"], kept.events("input_admitted")[0]["turn"]);
    // Judged on a quarter of a second of its own speech, not on its start.
    assert!(said[0]["blocks"].as_u64() >= Some(25), "{said:?}");
    // The asker's level is in the end-of-speech record.
    assert_eq!(kept.events("local_endpoint")[0]["asker_level"], 178);

    let taken = asker_then(Mode::On, two_lines, leaning_back);
    assert_eq!(taken.seen.requests.len(), 2, "{:?}", taken.seen.requests);
    assert!(taken.events("asker_first").is_empty());

    let far = asker_then(Mode::On, two_lines, leaning_far_back);
    assert_eq!(far.seen.requests.len(), 1, "{:?}", far.seen.requests);

    // Recorded only: the same record, and the quiet speech takes the turn.
    let recorded = asker_then(Mode::Record, two_lines, across_the_room);
    assert_eq!(
        recorded.seen.requests.len(),
        2,
        "{:?}",
        recorded.seen.requests
    );
    let said = recorded.events("asker_first");
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(said[0]["acted"], false, "{said:?}");

    let before = asker_then(Mode::Off, two_lines, across_the_room);
    assert_eq!(before.seen.requests.len(), 2, "{:?}", before.seen.requests);
    assert!(before.events("asker_first").is_empty());
}

/// The reviewer's probe on #1092 (`run_policy_with_peers`): the asker's own
/// follow-up 1.5 s after their question opens with 0.2 s at ±300 ("um,
/// and…") and goes on at ±2,000. Judged at 60 ms over the last half second,
/// it lost 0.4 s of its start. It now waits until its own speech says
/// whose it is, kept as it is, and reaches the provider whole, as without
/// the rule.
#[test]
fn with_asker_first_a_soft_start_to_the_askers_follow_up_is_kept_whole() {
    use crate::asker_first::Mode;
    let _one = one_session();
    fn soft_start(block: u64) -> i16 {
        match block {
            50..150 => 2_000,
            300..320 => 300,
            320..400 => 2_000,
            _ => 20,
        }
    }
    let with = asker_then(Mode::On, two_lines, soft_start);
    let without = asker_then(Mode::Off, two_lines, soft_start);
    assert_eq!(with.seen.requests.len(), 2, "{:?}", with.seen.requests);
    assert!(with.events("asker_first").is_empty());
    assert_eq!(
        with.seen.requests[1].audio_blocks, without.seen.requests[1].audio_blocks,
        "with {:?}, without {:?}",
        with.seen.requests, without.seen.requests
    );
}

/// The reviewer's second probe on #1092: someone across the room (±300)
/// for half a second, then, 0.1 s later, the asker at ±2,000 for a second.
/// The asker's own blocks are judged, not the half second before them:
/// their speech takes the turn, none of it cut, though the talk just before
/// them may go with it. Judged on the half second before, 0.22 s of the
/// asker's speech was lost.
#[test]
fn with_asker_first_the_asker_just_after_talk_across_the_room_is_heard_whole() {
    use crate::asker_first::Mode;
    let _one = one_session();
    fn across_then_asker(block: u64) -> f32 {
        if (50..150).contains(&block) || (300..350).contains(&block) || (360..460).contains(&block)
        {
            SPEECH
        } else {
            QUIET
        }
    }
    fn levels(block: u64) -> i16 {
        match block {
            50..150 => 2_000,
            300..350 => 300,
            360..460 => 2_000,
            _ => 20,
        }
    }
    let with = asker_then(Mode::On, across_then_asker, levels);
    assert_eq!(with.seen.requests.len(), 2, "{:?}", with.seen.requests);
    // The first of the talk across the room took no turn.
    assert_eq!(with.events("asker_first").len(), 1);
    // The turn taken holds all of the asker's speech: triggered no later
    // than their sixth block (360 + 5), its audio from before their first.
    // Here it is the rest of the talk across the room, run on into theirs
    // and taken once their speech made it loud: that talk goes with it, and
    // nothing of the asker's is cut.
    let admitted = with.events("input_candidate");
    let candidate = &admitted.last().unwrap()["candidate"];
    assert!(
        candidate["trigger_sequence"].as_u64() <= Some(365),
        "{candidate}"
    );
    let (first, trigger) = (
        candidate["first_read_at_us"].as_u64().unwrap(),
        candidate["trigger_read_at_us"].as_u64().unwrap(),
    );
    assert!(trigger - first >= 50_000, "{candidate}");
    assert!(
        with.seen.requests[1].audio_blocks >= 100,
        "{:?}",
        with.seen.requests
    );
}

/// The first builder's P1 on #1092: a short quiet line that ends while the
/// rule waits must be decided then. Kept pending past its end, the next
/// utterance's Start was refused and the conversation ended. A 15-block line
/// at ±300, then the asker 0.3 s or 0.65 s after it: the session goes on,
/// the quiet line takes no turn and the asker's does.
#[test]
fn with_asker_first_a_short_quiet_line_ended_while_waiting_ends_nothing() {
    use crate::asker_first::Mode;
    let _one = one_session();
    fn after_30(block: u64) -> f32 {
        if (50..150).contains(&block) || (300..315).contains(&block) || (345..445).contains(&block)
        {
            SPEECH
        } else {
            QUIET
        }
    }
    fn after_65(block: u64) -> f32 {
        if (50..150).contains(&block) || (300..315).contains(&block) || (380..480).contains(&block)
        {
            SPEECH
        } else {
            QUIET
        }
    }
    fn levels(block: u64) -> i16 {
        match block {
            50..150 => 2_000,
            300..315 => 300,
            315..500 => 2_000,
            _ => 20,
        }
    }
    for script in [after_30 as Script, after_65] {
        let session = asker_then(Mode::On, script, levels);
        assert_eq!(
            session.seen.requests.len(),
            2,
            "{:?}",
            session.seen.requests
        );
    }
}

/// The reviewer's P1 on #1092 at 00:00: a 0.2 s word at ±300, 4.0, 4.2 or
/// 4.5 s after the asker's end (block 189). Waiting until the 5 s passed, it
/// outlived the 0.8 s of reads kept for it: the voice ended, or it was
/// refused with no turn and no record. It is decided while its reads last
/// now: the session goes on, it takes no turn, and the rule says so.
#[test]
fn with_asker_first_a_late_short_word_is_decided_while_its_reads_last() {
    use crate::asker_first::Mode;
    let _one = one_session();
    fn at_400(block: u64) -> f32 {
        if (50..150).contains(&block) || (589..609).contains(&block) {
            SPEECH
        } else {
            QUIET
        }
    }
    fn at_420(block: u64) -> f32 {
        if (50..150).contains(&block) || (609..629).contains(&block) {
            SPEECH
        } else {
            QUIET
        }
    }
    fn at_450(block: u64) -> f32 {
        if (50..150).contains(&block) || (639..659).contains(&block) {
            SPEECH
        } else {
            QUIET
        }
    }
    fn levels(block: u64) -> i16 {
        match block {
            50..150 => 2_000,
            589..660 => 300,
            _ => 20,
        }
    }
    for script in [at_400 as Script, at_420, at_450] {
        let session = asker_then(Mode::On, script, levels);
        assert_eq!(
            session.seen.requests.len(),
            1,
            "{:?}",
            session.seen.requests
        );
        let said = session.events("asker_first");
        assert_eq!(said.len(), 1, "{said:?}");
        assert_eq!(said[0]["acted"], true, "{said:?}");
    }
}

/// The reviewer's first P2 on #1092 at 00:00: a 1 s line at ±300 begun
/// 4.7 s after the asker's end was kept at its start, and its next
/// candidate took the turn at 5 s, 0.29 s into it. A line kept is kept
/// until it pauses now, and never taken halfway.
#[test]
fn with_asker_first_a_line_kept_near_the_five_seconds_is_kept_whole() {
    use crate::asker_first::Mode;
    let _one = one_session();
    fn line(block: u64) -> f32 {
        if (50..150).contains(&block) || (659..759).contains(&block) {
            SPEECH
        } else {
            QUIET
        }
    }
    fn levels(block: u64) -> i16 {
        match block {
            50..150 => 2_000,
            659..760 => 300,
            _ => 20,
        }
    }
    let session = asker_then(Mode::On, line, levels);
    assert_eq!(
        session.seen.requests.len(),
        1,
        "{:?}",
        session.seen.requests
    );
}

/// The reviewer's second P2 on #1092 at 00:00: `record` gives `on`'s
/// judgement, made where `on` makes it. The asker's follow-up opening with
/// 0.2 s at ±300 is taken by `on`, so `record` writes nothing for it; the
/// line across the room is recorded once, as kept at its last chance.
#[test]
fn with_asker_first_record_judges_where_on_does() {
    use crate::asker_first::Mode;
    let _one = one_session();
    fn soft_start(block: u64) -> i16 {
        match block {
            50..150 => 2_000,
            300..320 => 300,
            320..400 => 2_000,
            _ => 20,
        }
    }
    fn across_the_room(block: u64) -> i16 {
        match block {
            50..150 => 2_000,
            300..400 => 300,
            _ => 20,
        }
    }
    let follow_up = asker_then(Mode::Record, two_lines, soft_start);
    assert!(follow_up.events("asker_first").is_empty());
    let across = asker_then(Mode::Record, two_lines, across_the_room);
    let said = across.events("asker_first");
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(
        (said[0]["acted"].as_bool(), said[0]["judged"].as_str()),
        (Some(false), Some("at_its_last_chance")),
        "{said:?}"
    );
}

/// The reviewer's P3 on #1092 at 00:00: new speech is judged on its own
/// blocks, never on talk just before it. A loud burst of four blocks, at
/// the asker's level, then soft speech 60 ms later: the soft speech is
/// still well under the asker, and takes no turn. Judged on the half second
/// before, the burst made it the asker's.
#[test]
fn with_asker_first_a_loud_burst_just_before_does_not_let_soft_speech_in() {
    use crate::asker_first::Mode;
    let _one = one_session();
    fn burst_then_soft(block: u64) -> f32 {
        if (50..150).contains(&block) || (300..304).contains(&block) || (310..340).contains(&block)
        {
            SPEECH
        } else {
            QUIET
        }
    }
    fn levels(block: u64) -> i16 {
        match block {
            50..150 => 2_000,
            300..304 => 2_000,
            310..340 => 300,
            _ => 20,
        }
    }
    let session = asker_then(Mode::On, burst_then_soft, levels);
    assert_eq!(
        session.seen.requests.len(),
        1,
        "{:?}",
        session.seen.requests
    );
    assert_eq!(session.events("asker_first").len(), 1);
}

/// The first builder's 21:35 on #1092: the rule ends 5 s after the asker
/// stopped, whether or not the answer has begun. An answer that never
/// plays (held back by the rule for whom it answers, or never sent) must
/// not leave the lamp deaf: softer speech 6 s after the asker takes the
/// turn.
#[test]
fn with_asker_first_the_rule_ends_after_its_time_though_no_answer_played() {
    use crate::asker_first::Mode;
    let _one = one_session();
    fn late_line(block: u64) -> f32 {
        if (50..150).contains(&block) || (750..850).contains(&block) {
            SPEECH
        } else {
            QUIET
        }
    }
    fn late_and_soft(block: u64) -> i16 {
        match block {
            50..150 => 2_000,
            750..850 => 300,
            _ => 20,
        }
    }
    let late = asker_then(Mode::On, late_line, late_and_soft);
    assert_eq!(late.seen.requests.len(), 2, "{:?}", late.seen.requests);
    assert!(late.events("asker_first").is_empty());
}

/// The asker's turn begun as the lamp's answer ends, as `asker_then` runs
/// it but with that answer played and retired: the first request is
/// answered 0.1 s after its end, and no later one.
fn asker_after_answer(mode: crate::asker_first::Mode, script: Script, levels: Levels) -> Session {
    let session = run_policy_with_peers(
        TurnTaking::Duplex,
        8,
        script,
        EndpointRig::double(delayed(0.95, Duration::from_millis(60))),
        Some(ProviderReply {
            after_end: Duration::from_millis(100),
            packets: 5,
            only_first: true,
        }),
        None,
        PeerMode::Stepped,
        Lamp {
            end_silence_blocks: Some(60),
            asker_first: mode,
            levels: Some(levels),
            retires: true,
            ..Lamp::default()
        },
    );
    session.result.as_ref().unwrap();
    session
}

/// The reviewer on #1092: the asker's level is their own speech, not the
/// lamp's echo. The lamp answers the first request (its last sound at block
/// 202, the room heard again from 212), and the next request opens on 0.2 s
/// of its echo at ±6,000 before the asker's own ±800. Only the blocks over
/// 0.3 s after the lamp's sound are the asker's level, so the line at ±700
/// while that answer is awaited, 1.2 dB under them, takes the turn. With
/// the echo counted the asker would be at ±6,000, and the line kept out.
#[test]
fn with_asker_first_the_lamps_echo_is_not_the_askers_level() {
    use crate::asker_first::Mode;
    let _one = one_session();
    fn script(block: u64) -> f32 {
        if (50..150).contains(&block) || (213..245).contains(&block) || (450..550).contains(&block)
        {
            SPEECH
        } else {
            QUIET
        }
    }
    fn levels(block: u64) -> i16 {
        match block {
            50..150 => 2_000,
            213..233 => 6_000,
            233..245 => 800,
            450..550 => 700,
            _ => 20,
        }
    }
    let session = asker_after_answer(Mode::On, script, levels);
    assert_eq!(
        session.events("speech_final_sample_retired").len(),
        1,
        "the first answer is played and retired"
    );
    let ends = session.events("local_endpoint");
    assert_eq!(ends.len(), 3, "{ends:?}");
    // On the face's scale ±800 is 118 and ±6,000 is 249.
    assert_eq!(ends[1]["asker_level"], 118, "{ends:?}");
    assert_eq!(
        session.seen.requests.len(),
        3,
        "{:?}",
        session.seen.requests
    );
    assert!(session.events("asker_first").is_empty());
}

/// The first request answered 0.6 s after its end, with echo evidence as
/// lamp-4ace's microphone gives it, and no later one; the asker at ±2,000
/// in blocks 50 to 150, then `script` at `levels`.
fn answer_begins_over(mode: crate::asker_first::Mode, script: Script, levels: Levels) -> Session {
    answer_begins_over_with(mode, script, levels, over_at_1000)
}

/// [`answer_begins_over`], with `over` saying which speech blocks hold a
/// person over the lamp's voice.
fn answer_begins_over_with(
    mode: crate::asker_first::Mode,
    script: Script,
    levels: Levels,
    over: Over,
) -> Session {
    let session = run_policy_with_peers(
        TurnTaking::Duplex,
        6,
        script,
        EndpointRig::double(delayed(0.95, Duration::from_millis(60))),
        Some(ProviderReply {
            after_end: Duration::from_millis(600),
            packets: 5,
            only_first: true,
        }),
        None,
        PeerMode::Stepped,
        Lamp {
            end_silence_blocks: Some(60),
            asker_first: mode,
            levels: Some(levels),
            echo: Some(over),
            ..Lamp::default()
        },
    );
    session.result.as_ref().unwrap();
    session
}

/// The answer arrives while the rule waits on a line from across the room.
/// Its first block waits for the line's judgement, the line is judged on
/// its own blocks until its last chance, well under the asker then it is
/// let go, and the answer plays. Left to the duplex rule, it was admitted a
/// block into the answer and replaced it, as "¿Qué?" replaced the answer at
/// 14:21. A line at ±300 begins 0.4 s after the asker's turn ended, and the
/// answer arrives 0.2 s into it.
#[test]
fn with_asker_first_a_line_the_answer_begins_over_is_let_go_and_the_answer_plays() {
    use crate::asker_first::Mode;
    let _one = one_session();
    fn script(block: u64) -> f32 {
        if (50..150).contains(&block) || (228..328).contains(&block) {
            SPEECH
        } else {
            QUIET
        }
    }
    fn levels(block: u64) -> i16 {
        match block {
            50..150 => 2_000,
            228..328 => 300,
            _ => 20,
        }
    }
    let session = answer_begins_over(Mode::On, script, levels);
    assert_eq!(
        session.seen.requests.len(),
        1,
        "{:?}",
        session.seen.requests
    );
    let said = session.events("asker_first");
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(
        (said[0]["acted"].as_bool(), said[0]["judged"].as_str()),
        (Some(true), Some("at_its_last_chance")),
        "{said:?}"
    );
    // The answer's first block waited for the line's judgement, no more than
    // 0.3 s, then played and was not cut.
    let held = session.events("asker_first_answer_held");
    assert_eq!(held.len(), 1, "{held:?}");
    let began = session.events("speaker_first_write")[0]["at_us"]
        .as_u64()
        .unwrap();
    let let_go = said[0]["at_us"].as_u64().unwrap();
    let arrived = session.events("provider_first_audio")[0]["at_us"]
        .as_u64()
        .unwrap();
    assert!(began >= let_go, "{began} {let_go}");
    assert!(began - arrived <= 300_000, "held {} us", began - arrived);
    let finished = session.events("turn_finished");
    assert!(
        finished
            .iter()
            .all(|row| row["outcome"] != "user_interrupted"),
        "{finished:?}"
    );
    // Without the rule the line takes the turn before the answer comes.
    let without = answer_begins_over(Mode::Off, script, levels);
    assert_eq!(
        without.seen.requests.len(),
        2,
        "{:?}",
        without.seen.requests
    );
}

/// The reviewer's P2 on #1197 at 04:50: over the reply's first 0.2 to
/// 0.45 s on lamp-4ace the lamp's own sound has not been matched, and the
/// evidence cannot tell a person from it. The asker's follow-up after a
/// soft "um", the answer arriving during it, is not judged over the answer
/// then: the answer's first block waits for the line's judgement, the
/// line is judged in the clear, and the asker takes the turn. Judged over
/// the answer's unmatched blocks, it was let go on its soft start.
#[test]
fn with_asker_first_the_answer_waits_for_a_line_judged_in_the_clear() {
    use crate::asker_first::Mode;
    let _one = one_session();
    fn script(block: u64) -> f32 {
        if (50..150).contains(&block) || (235..335).contains(&block) {
            SPEECH
        } else {
            QUIET
        }
    }
    fn levels(block: u64) -> i16 {
        match block {
            50..150 => 2_000,
            235..255 => 300,
            255..335 => 2_000,
            _ => 20,
        }
    }
    // Not matched for the answer's first 0.3 s (its first write at 248 if
    // it is not held), then as before.
    fn unmatched_first(block: u64, level: i16) -> Option<bool> {
        (block >= 278).then_some(level >= 1_000)
    }
    for mode in [Mode::On, Mode::Record, Mode::Off] {
        let session = answer_begins_over_with(mode, script, levels, unmatched_first);
        assert_eq!(
            session.seen.requests.len(),
            2,
            "{mode:?}: {:?}",
            session.seen.requests
        );
        assert!(
            session.events("asker_first").is_empty(),
            "{mode:?}: {:?}",
            session.events("asker_first")
        );
    }
}

/// The reviewer's probe on #1197: the asker goes on 0.47 s after their
/// turn ended with 0.2 s of "um" at ±300, then their own ±2,000, and the
/// answer begins during the "um". Judged at the answer's first block, on
/// its soft start alone, it was let go and the asker was not heard at all.
/// Judged on its own blocks until its last chance, it takes the turn, as
/// with the rule off; and nothing is recorded, as with `record`.
#[test]
fn with_asker_first_the_askers_soft_start_over_the_answers_start_takes_the_turn() {
    use crate::asker_first::Mode;
    let _one = one_session();
    fn script(block: u64) -> f32 {
        if (50..150).contains(&block) || (235..335).contains(&block) {
            SPEECH
        } else {
            QUIET
        }
    }
    fn levels(block: u64) -> i16 {
        match block {
            50..150 => 2_000,
            235..255 => 300,
            255..335 => 2_000,
            _ => 20,
        }
    }
    for mode in [Mode::On, Mode::Record, Mode::Off] {
        let session = answer_begins_over(mode, script, levels);
        assert_eq!(
            session.seen.requests.len(),
            2,
            "{mode:?}: {:?}",
            session.seen.requests
        );
        assert!(
            session.events("asker_first").is_empty(),
            "{mode:?}: {:?}",
            session.events("asker_first")
        );
    }
}

/// #1178, the reviewer's P3 on #1214, through the coordinator's own loop:
/// the speaker's authority lapses after the first answer's second chunk,
/// and it reports its reset and the discard of that answer's queued speech
/// (`ExpiredState`), as the speaker worker does. The loop cuts that answer
/// (`speaker_lapsed`), sends nothing more of it, and goes on: the next
/// question is answered. With the cut taken out of the loop, the session
/// ends in an error: the voice ends, as before #1214. (Its clearing of the
/// provider's queue is not what keeps the rest of the answer back: with no
/// current answer nothing is sent anyway, and this test passes without it.)
#[test]
fn a_speaker_lapse_mid_answer_cuts_it_and_the_next_question_is_answered() {
    let _one = one_session();
    fn script(block: u64) -> f32 {
        if (50..150).contains(&block) || (500..600).contains(&block) {
            SPEECH
        } else {
            QUIET
        }
    }
    let session = run_policy_with_peers(
        TurnTaking::V1Baseline,
        10,
        script,
        EndpointRig::double(delayed(0.95, Duration::from_millis(60))),
        Some(ProviderReply {
            after_end: Duration::from_millis(300),
            packets: 5,
            only_first: false,
        }),
        None,
        PeerMode::Stepped,
        Lamp {
            lapses_after: Some(2),
            ..Lamp::default()
        },
    );
    session.result.as_ref().unwrap();
    let seen = &session.seen;
    let lapsed_at = seen.lapsed_at.expect("the speaker lapsed");
    let first = seen.chunks[0].0;
    let finished = session.events("turn_finished");
    let cut: Vec<_> = finished
        .iter()
        .filter(|row| row["turn"] == first)
        .map(|row| row["outcome"].as_str())
        .collect();
    assert_eq!(cut, [Some("speaker_lapsed")], "{finished:?}");
    // Nothing more of it is sent: at most the one chunk already on its way
    // reaches the speaker after the lapse, and is refused.
    let after: Vec<_> = seen
        .chunks
        .iter()
        .chain(&seen.refused)
        .filter(|(turn, at)| *turn == first && *at > lapsed_at)
        .collect();
    assert!(after.len() <= 1, "{after:?}");
    // The voice goes on: the next question is asked and answered.
    assert_eq!(seen.requests.len(), 2, "{:?}", seen.requests);
    assert!(
        seen.chunks
            .iter()
            .any(|(turn, at)| *turn != first && *at > lapsed_at),
        "{:?}",
        seen.chunks
    );
}

/// #830, the first builder's 07:26: "listening with no link" never printed
/// on lamp-4ace. With `LAMP_START_OFFLINE=on`, a service whose link is down
/// past the startup budget keeps listening, says so once, and becomes ready
/// when the link comes; without it, it ends at the budget, as before.
#[test]
fn a_service_started_with_no_link_says_so_once_and_is_ready_when_the_link_comes() {
    // The link comes 3 s past the budget; a mute 5 s after that ends the
    // service's cycle, as a person's mute does.
    let link_after = INPUT_READINESS_TIMEOUT + Duration::from_secs(3);
    let mute_block = (link_after.as_millis() as u64 + 5_000) / 10;
    let run = |start_offline: bool, link_after: Duration| {
        run_policy_with_peers(
            TurnTaking::V1Baseline,
            0,
            |_| QUIET,
            EndpointRig::double(delayed(0.0, Duration::ZERO)),
            None,
            Some(mute_block),
            PeerMode::Stepped,
            Lamp {
                provider_ready_after: Some(link_after),
                start_offline: Some(start_offline),
                serve: true,
                ..Lamp::default()
            },
        )
    };
    let late = |start_offline: bool| run(start_offline, link_after);
    let waited = late(true);
    // It lived past the budget to the mute that ends the cycle.
    let end = format!("{:?}", waited.result.as_ref().unwrap_err());
    assert!(end.contains("PhysicalMute"), "{end}");
    let said = waited.events("waiting_for_link");
    assert_eq!(said.len(), 1, "said once: {said:?}");
    let ready = waited.events("provider_link_current");
    assert_eq!(ready.len(), 1, "ready once the link came: {ready:?}");
    // Said at the budget, not before it: the link came 3 s after the line,
    // so a line said at the first frame would be about 28 s early (the
    // reviewer, after the merge).
    let said_at = said[0]["at_us"].as_u64().unwrap();
    let ready_at = ready[0]["at_us"].as_u64().unwrap();
    let before = ready_at.saturating_sub(said_at);
    assert!(
        (2_900_000..=3_100_000).contains(&before),
        "said {before} us before ready, not about 3 s"
    );
    // A normal start with the switch on: the link up within the budget, and
    // no line at all.
    let normal = run(true, Duration::from_secs(1));
    let end = format!("{:?}", normal.result.as_ref().unwrap_err());
    assert!(end.contains("PhysicalMute"), "{end}");
    assert!(
        normal.events("waiting_for_link").is_empty(),
        "no line when the link comes in time"
    );
    assert_eq!(normal.events("provider_link_current").len(), 1);
    // Without the switch the startup budget covers the link, as before.
    let ended = late(false);
    let error = ended.result.as_ref().unwrap_err().to_string();
    assert!(error.contains("voice input never became ready"), "{error}");
    assert!(ended.events("waiting_for_link").is_empty());
}

/// #1567, lamp-75c9 on 2026-10-19: the model's account was out of quota, the
/// provider said the first session was refused (`unavailable`, quota) a
/// second after boot, and 25 s later the voice ended, "voice input never
/// became ready", until systemd gave up. A refused session is a link that is
/// not up yet. With `LAMP_START_OFFLINE=on` the service keeps listening past
/// the budget, says so once and becomes ready on the session that comes
/// later; asking again is the provider worker's, after its refusal pause
/// (`RecoveryPolicy::refusal_pause`). Without the switch the budget covers
/// the link and the voice ends at it, as on that lamp.
#[test]
fn a_session_refused_at_start_does_not_end_a_service_started_offline() {
    use crate::provider_worker::Fault;
    let link_after = INPUT_READINESS_TIMEOUT + Duration::from_secs(3);
    let mute_block = (link_after.as_millis() as u64 + 5_000) / 10;
    let run = |start_offline: bool, refused: Fault| {
        run_policy_with_peers(
            TurnTaking::V1Baseline,
            0,
            |_| QUIET,
            EndpointRig::double(delayed(0.0, Duration::ZERO)),
            None,
            Some(mute_block),
            PeerMode::Stepped,
            Lamp {
                provider_ready_after: Some(link_after),
                provider_refused: Some(refused),
                start_offline: Some(start_offline),
                serve: true,
                ..Lamp::default()
            },
        )
    };
    for refused in [Fault::QuotaExceeded, Fault::RateLimited] {
        let waited = run(true, refused);
        let end = format!("{:?}", waited.result.as_ref().unwrap_err());
        assert!(end.contains("PhysicalMute"), "{refused:?}: {end}");
        assert_eq!(waited.events("provider_unavailable").len(), 1);
        assert_eq!(waited.events("waiting_for_link").len(), 1);
        assert_eq!(waited.events("provider_link_current").len(), 1);
        let ended = run(false, refused);
        let error = ended.result.as_ref().unwrap_err().to_string();
        assert!(
            error.contains("voice input never became ready"),
            "{refused:?}: {error}"
        );
        assert!(ended.events("waiting_for_link").is_empty());
    }
}

/// #1567, slow ears: the startup budget's other half. A microphone that
/// starts late but within the budget, with the session refused, still makes
/// a service that waits for its link. One that has not started by the
/// budget ends the voice even with the switch on: input that is not
/// retained is not proven, and readiness is never said without it.
#[test]
fn slow_ears_within_the_budget_are_waited_for_and_past_it_end_the_voice() {
    let link_after = INPUT_READINESS_TIMEOUT + Duration::from_secs(3);
    let mute_block = (link_after.as_millis() as u64 + 5_000) / 10;
    let run = |capture_after: Duration| {
        run_policy_with_peers(
            TurnTaking::V1Baseline,
            0,
            |_| QUIET,
            EndpointRig::double(delayed(0.0, Duration::ZERO)),
            None,
            Some(mute_block),
            PeerMode::Stepped,
            Lamp {
                provider_ready_after: Some(link_after),
                provider_refused: Some(crate::provider_worker::Fault::QuotaExceeded),
                capture_after: Some(capture_after),
                start_offline: Some(true),
                serve: true,
                ..Lamp::default()
            },
        )
    };
    let slow = run(INPUT_READINESS_TIMEOUT - Duration::from_secs(5));
    let end = format!("{:?}", slow.result.as_ref().unwrap_err());
    assert!(end.contains("PhysicalMute"), "{end}");
    assert_eq!(slow.events("waiting_for_link").len(), 1);
    assert_eq!(slow.events("provider_link_current").len(), 1);
    let deaf = run(INPUT_READINESS_TIMEOUT + Duration::from_secs(1));
    let error = deaf.result.as_ref().unwrap_err().to_string();
    assert!(error.contains("voice input never became ready"), "{error}");
    assert!(deaf.events("waiting_for_link").is_empty());
    assert!(deaf.events("provider_link_current").is_empty());
}

/// #828, listening gaps, through the coordinator's own loop: the answer's
/// first sentence is 2.2 s, then 120 ms quiet, then the next. With
/// `LAMP_LISTEN_GAPS=on` the next sentence's first chunk waits until 250 ms
/// after the first quiet one, once (`listen_gap`), while the speaker writes
/// zeros. Off, nothing waits. The speaker is handed the same samples either
/// way: the gap is the speaker's zeros, never samples of its own.
#[test]
fn a_long_answers_sentence_end_is_held_quiet_once_only_with_the_switch() {
    let _one = one_session();
    fn shape(index: u64) -> i16 {
        let ms = index * 1_000 / u64::from(lamp_audio::RENDER_RATE);
        if (2_200..2_320).contains(&ms) {
            0
        } else {
            3_000
        }
    }
    let run = |listen_gaps| {
        run_policy_with_peers(
            TurnTaking::Duplex,
            10,
            one_question,
            EndpointRig::double(delayed(0.95, Duration::from_millis(60))),
            Some(ProviderReply {
                after_end: Duration::from_millis(300),
                packets: 64,
                only_first: false,
            }),
            None,
            PeerMode::Stepped,
            Lamp {
                reply_shape: Some(shape),
                listen_gaps,
                ..Lamp::default()
            },
        )
    };
    let longest_wait = |session: &Session| {
        session
            .seen
            .chunks
            .windows(2)
            .map(|pair| pair[1].1 - pair[0].1)
            .max()
            .unwrap()
    };
    let on = run(true);
    on.result.as_ref().unwrap();
    let gaps = on.events("listen_gap");
    assert_eq!(gaps.len(), 1, "{gaps:?}");
    assert_eq!(
        gaps[0]["sent_ms"].as_u64().map(|ms| ms >= 2_000),
        Some(true)
    );
    assert!(
        longest_wait(&on) >= Duration::from_millis(200),
        "{:?}",
        longest_wait(&on)
    );
    let off = run(false);
    off.result.as_ref().unwrap();
    assert!(off.events("listen_gap").is_empty());
    assert!(
        longest_wait(&off) < Duration::from_millis(100),
        "{:?}",
        longest_wait(&off)
    );
    assert_eq!(on.seen.chunk_sums, off.seen.chunk_sums);
    assert!(on.seen.chunk_sums.len() > 250);
}

/// `directed-fixture --fixture-at-ms 3000` (diagnostic only), through the
/// loop with nobody heard: the fixture's own turn starts within a block of
/// `listening_ready` + 3000 ms, and never while the lamp is muted.
#[test]
fn a_timed_fixture_starts_3000_ms_after_listening_ready_and_never_muted() {
    let _one = one_session();
    let run = |mute_at: Option<u64>| {
        run_policy_with_peers(
            TurnTaking::V1Baseline,
            6,
            |_| QUIET,
            EndpointRig::double(delayed(0.0, Duration::ZERO)),
            None,
            mute_at,
            PeerMode::Stepped,
            Lamp {
                fixture_at_ms: Some(3_000),
                ..Lamp::default()
            },
        )
    };
    let heard = run(None);
    heard.result.as_ref().unwrap();
    let ready = heard.events("listening_ready")[0]["at_us"]
        .as_u64()
        .unwrap();
    let started = heard.events("fixture_timed_start");
    assert_eq!(started.len(), 1, "{started:?}");
    let at = started[0]["at_us"].as_u64().unwrap();
    assert!(
        (ready + 3_000_000..=ready + 3_010_000).contains(&at),
        "started {} us after listening_ready",
        at - ready
    );
    let admitted = heard.events("input_admitted");
    assert!(
        admitted
            .iter()
            .all(|row| row["admission_basis"] == "fixture_at_ms"),
        "{admitted:?}"
    );
    // Muted at 1.5 s, before it is due: no start.
    let muted = run(Some(150));
    assert!(
        muted.events("fixture_timed_start").is_empty(),
        "{:?}",
        muted.result
    );
}
