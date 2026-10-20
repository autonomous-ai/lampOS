//! The end sent early (`--early-end-ms`; docs/faster-answers.md, way 2),
//! through the coordinator's own loop. Capture, privacy and speaker are
//! emulated peers on the production channels, as in `endpoint_session`. The
//! provider is the real Coordinated worker and Gemini session, started as
//! `--provider-continuation kept-start` starts it, against the scripted
//! service of `lamp_gemini::testing`, which answers every request it is
//! asked to and stops an answer when a new activity begins over it, as the
//! real one does with `START_OF_ACTIVITY_INTERRUPTS`.
//!
//! Capture blocks follow a 10 ms schedule, and each carries its own number
//! as its samples, so that what reached the service can be read back block
//! by block. The detector's decisions are counted in blocks and are exact;
//! the times the service takes are asserted with room to spare. No audio
//! device, cloud service or model is used. Each coordinator scenario
//! explicitly selects bounded host scheduling tolerance for privacy; these
//! are ordering tests, not measurements of the lamp's 50 ms privacy response.
//!
//! The scenarios run on one logical clock (#823): `scenario_clock` for the
//! coordinator and its peers, and a paused Tokio clock for the provider
//! worker, its Gemini session and the scripted service. All of them run on
//! the test's thread inside the coordinator's step, and the clock moves
//! 2 ms only once each has run, so a conversation of seconds takes a
//! fraction of that. Child processes and their cleanup stay on the host
//! clock.
use super::endpoint_session::one_session;
use super::*;
use crate::{
    provider_worker::{Continuation, serve_coordinated_test},
    wire::{CaptureTiming, ReferenceTiming, SpeakerChunk},
};
use lamp_audio::ownership::PlaybackCursor;
use lamp_gemini::{
    BarrierPolicy, Credential, RecoveryPolicy, Resumption, SessionConfig, Supervisor,
    testing::{Dial, audio, connector, sent_audio},
};
use std::{
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::mpsc::UnboundedReceiver;

const SPEECH: f32 = 0.95;
const QUIET: f32 = 0.05;
// These provider integrations accept bounded host scheduling delay;
// it is not a measurement of the lamp's 50 ms privacy response. IPC/capture
// freshness and the real 300 ms shared cleanup budget are intact.
const FIXTURE_PRIVACY_FRESH_US: u64 = 100_000;
const FIXTURE_CONVERSATION_WATCHDOG: Duration = Duration::from_secs(12);
/// The host's bound on one scenario's logical clock: the provider's 3 s to
/// be ready, the conversation's watchdog, and cleanup.
const FIXTURE_SCENARIO_WATCHDOG: Duration = Duration::from_secs(20);
/// One logical step, as `scenario_clock` takes it.
const STEP: Duration = Duration::from_millis(2);
/// Speech probability of each 10 ms capture block, by block number.
type Script = fn(u64) -> f32;

/// One client message, as the service received it.
#[derive(Clone, Debug, PartialEq)]
enum Wire {
    Start,
    /// A capture block, by its number.
    Audio(i16),
    End,
    /// A note written inside the request's activity.
    Note(String),
    Other(String),
}

#[derive(Default)]
struct Seen {
    capture_started: Option<Instant>,
    /// Each block handed to the speaker: its turn, when, its first sample,
    /// and when by the coordinator's clock.
    chunks: Vec<(u64, Instant, i16, u64)>,
    /// What the service received: on which connection, what, and when.
    wire: Vec<(usize, Wire, Instant)>,
    /// Each answer the service began: its sample value and when.
    answers: Vec<(i16, Instant)>,
    connections: usize,
    /// Why the provider worker ended, if it ended by itself.
    worker: Option<String>,
}

impl Seen {
    /// When capture block `index` (1-based) was read.
    fn block(&self, index: u64) -> Instant {
        self.capture_started.expect("capture started") + Duration::from_millis(10 * (index - 1))
    }
    /// What connection `connection` received, in order.
    fn heard(&self, connection: usize) -> Vec<Wire> {
        self.wire
            .iter()
            .filter(|(on, ..)| *on == connection)
            .map(|(_, what, _)| what.clone())
            .collect()
    }
    fn count(&self, what: &Wire) -> usize {
        self.wire.iter().filter(|(_, seen, _)| seen == what).count()
    }
    /// When the `n`th (0-based) activityEnd was received.
    fn end_at(&self, n: usize) -> Instant {
        self.wire
            .iter()
            .filter(|(_, what, _)| *what == Wire::End)
            .nth(n)
            .expect("an end was received")
            .2
    }
    /// The first speaker block with the samples of answer `value`.
    fn first_played(&self, value: i16) -> Option<Instant> {
        self.chunks
            .iter()
            .find(|(_, _, sample, _)| *sample == value)
            .map(|(_, at, _, _)| *at)
    }
    fn played(&self, value: i16) -> usize {
        self.chunks
            .iter()
            .filter(|(_, _, sample, _)| *sample == value)
            .count()
    }
}

/// How the scripted service answers.
#[derive(Clone, Copy)]
struct Service {
    /// The first sound of an answer, this long after its request's end.
    answer_after: Duration,
    /// Packets of 40 ms in an answer, one every 40 ms.
    packets: usize,
    /// What the answer's words are.
    words: &'static str,
    /// The first connection is closed this long after the `n`th end (from
    /// 1) it receives, and that end is never answered: a link that drops.
    drop_after_end: Option<(usize, Duration)>,
}

impl Service {
    fn answering_after(ms: u64) -> Self {
        Self {
            answer_after: Duration::from_millis(ms),
            packets: 12,
            words: "Here is an answer.",
            drop_after_end: None,
        }
    }
}

/// The answers' sample values: 101 for the first begun, then 102, ...
const FIRST_ANSWER: i16 = 101;

fn interrupted() -> Value {
    json!({"serverContent":{"interrupted":true}})
}
fn idle() -> Value {
    json!({"serverContent":{"turnComplete":true,"interactionStatus":"IDLE"}})
}

fn wire(message: &Value) -> Wire {
    let input = &message["realtimeInput"];
    if input.get("activityStart").is_some() {
        Wire::Start
    } else if input.get("activityEnd").is_some() {
        Wire::End
    } else if let Some(samples) = sent_audio(message) {
        assert!(samples.iter().all(|sample| *sample == samples[0]));
        Wire::Audio(samples[0])
    } else if let Some(text) = input.get("text").and_then(Value::as_str) {
        Wire::Note(text.to_owned())
    } else {
        Wire::Other(message.to_string())
    }
}

async fn serve_dials(
    mut dials: UnboundedReceiver<Dial>,
    service: Service,
    seen: Arc<Mutex<Seen>>,
    stop: Arc<AtomicBool>,
) {
    let mut answers = 0i16;
    while !stop.load(Ordering::SeqCst) {
        let Ok(Some(dial)) = tokio::time::timeout(Duration::from_millis(20), dials.recv()).await
        else {
            continue;
        };
        let connection = {
            let mut seen = seen.lock().unwrap();
            seen.connections += 1;
            seen.connections
        };
        one_connection(dial, connection, service, &seen, &stop, &mut answers).await;
    }
}

async fn one_connection(
    mut dial: Dial,
    connection: usize,
    service: Service,
    seen: &Mutex<Seen>,
    stop: &AtomicBool,
    answers: &mut i16,
) {
    dial.service.accept().await;
    // A handle, so that a link that drops is taken up again.
    dial.service
        .send(json!({"sessionResumptionUpdate":{
            "newHandle":format!("early-end-{connection}"),"resumable":true}}))
        .await;
    let mut due: Option<(tokio::time::Instant, i16)> = None;
    let mut speaking: Option<(i16, usize, tokio::time::Instant)> = None;
    let mut drop_at = None;
    let mut ends = 0;
    while !stop.load(Ordering::SeqCst) {
        if let Some(message) = dial.service.rest(Duration::from_millis(1)).await {
            let what = wire(&message);
            let now = tokio::time::Instant::now();
            seen.lock().unwrap().wire.push((
                connection,
                what.clone(),
                crate::scenario_clock::instant(),
            ));
            match what {
                Wire::End => {
                    ends += 1;
                    match service.drop_after_end {
                        Some((nth, after)) if connection == 1 && ends == nth => {
                            drop_at = Some(now + after)
                        }
                        _ => {
                            *answers += 1;
                            due = Some((now + service.answer_after, FIRST_ANSWER - 1 + *answers));
                        }
                    }
                }
                // A new activity over an answer stops it.
                Wire::Start if speaking.is_some() || due.is_some() => {
                    speaking = None;
                    due = None;
                    dial.service.burst([interrupted(), idle()]).await;
                }
                _ => {}
            }
        }
        let now = tokio::time::Instant::now();
        if drop_at.is_some_and(|at| now >= at) {
            dial.service.close(1012, "the link drops").await;
            return;
        }
        if let Some((at, value)) = due
            && now >= at
        {
            due = None;
            speaking = Some((value, 0, now));
            seen.lock()
                .unwrap()
                .answers
                .push((value, crate::scenario_clock::instant()));
        }
        if let Some((value, sent, next)) = speaking.as_mut()
            && now >= *next
        {
            dial.service.send(audio(*value, 960)).await;
            if *sent == 0 {
                dial.service
                    .send(json!({"serverContent":{"outputTranscription":{"text":service.words}}}))
                    .await;
            }
            *sent += 1;
            *next += Duration::from_millis(40);
            if *sent == service.packets {
                dial.service
                    .burst([json!({"serverContent":{"generationComplete":true}}), idle()])
                    .await;
                speaking = None;
            }
        }
    }
}

fn drain_control(channels: &mut WorkerChannels, generation: &mut u64) -> bool {
    while let Some(control) = channels.control.receive::<Control>().unwrap() {
        match control {
            Control::Authority { snapshot } => *generation = snapshot.microphone_generation(),
            Control::Stop => return false,
            _ => {}
        }
    }
    true
}

/// The synthetic source acquires continuously even when delivery is delayed.
/// A late delivery keeps its original source age; it cannot freshen old audio.
struct CaptureSchedule {
    origin_us: u64,
    next_sequence: u64,
}

impl CaptureSchedule {
    fn new(origin_us: u64) -> Self {
        Self {
            origin_us,
            next_sequence: 1,
        }
    }

    fn next_due(&mut self, now_us: u64) -> Option<(u64, u64)> {
        let sequence = self.next_sequence;
        let acquired_at_us = self.origin_us + 10_000 * (sequence - 1);
        if acquired_at_us > now_us {
            return None;
        }
        self.next_sequence += 1;
        Some((sequence, acquired_at_us))
    }
}

/// Frames on a fixed 10 ms schedule, like a capture device; each block's
/// samples are its own number.
struct CapturePeer {
    channels: WorkerChannels,
    script: Script,
    seen: Arc<Mutex<Seen>>,
    generation: u64,
    started: Option<CaptureSchedule>,
}

impl CapturePeer {
    /// One step: false once told to stop.
    fn step(&mut self) -> bool {
        while let Some(control) = self.channels.control.receive::<Control>().unwrap() {
            match control {
                Control::Authority { snapshot } => {
                    self.generation = snapshot.microphone_generation()
                }
                Control::StartCapture if self.started.is_none() => {
                    let at = monotonic_us();
                    let instant = crate::scenario_clock::instant();
                    self.channels
                        .control
                        .send(WorkerEvent::CaptureStarted {
                            epoch: 1,
                            dsp_epoch: 1,
                            noise_suppression: NoiseSuppression::On,
                            privacy_generation: self.generation,
                            rate: 16_000,
                            channels: 1,
                            period_frames: 160,
                            buffer_frames: 640,
                            monotonic_status_timestamps: false,
                            open_started_at_us: at,
                            prepared_at_us: at,
                            capture_started_at_us: at,
                            start_completed_at_us: at,
                        })
                        .unwrap();
                    self.started = Some(CaptureSchedule::new(at));
                    self.seen.lock().unwrap().capture_started = Some(instant);
                }
                Control::Stop => return false,
                _ => {}
            }
        }
        while let Some(schedule) = self.started.as_mut() {
            let Some((sequence, at)) = schedule.next_due(monotonic_us()) else {
                break;
            };
            let frame = MicrophoneFrame {
                raw: None,
                linear: None,
                epoch: 1,
                dsp_epoch: 1,
                privacy_generation: self.generation,
                frame_sequence: sequence,
                read_completed_at_us: at,
                probability: (self.script)(sequence),
                echo: None,
                reference: Default::default(),
                samples: vec![sequence as i16; 160],
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
            self.channels.data.send(frame).unwrap();
        }
        true
    }
}

struct PrivacyPeer {
    channels: WorkerChannels,
    generation: u64,
    next: Instant,
}

impl PrivacyPeer {
    /// One step: false once told to stop.
    fn step(&mut self) -> bool {
        if !drain_control(&mut self.channels, &mut self.generation) {
            return false;
        }
        if crate::scenario_clock::instant() >= self.next {
            self.channels
                .control
                .send(WorkerEvent::Privacy {
                    receipt: None,
                    acquired_at_us: monotonic_us(),
                    muted: false,
                })
                .unwrap();
            self.next += Duration::from_millis(10);
        }
        true
    }
}

/// Takes every block at once, as ALSA would accept it.
struct SpeakerPeer {
    channels: WorkerChannels,
    seen: Arc<Mutex<Seen>>,
    generation: u64,
    frame: u64,
    /// Each block is said played out as it is taken, and the last of an
    /// answer retired, so that an answer that plays settles
    /// ([`Setup::retires`]).
    retires: bool,
}

impl SpeakerPeer {
    /// One step: false once told to stop.
    fn step(&mut self) -> bool {
        if !drain_control(&mut self.channels, &mut self.generation) {
            return false;
        }
        while let Some(chunk) = self.channels.data.receive::<SpeakerChunk>().unwrap() {
            let owner = chunk.permit.owner();
            self.seen.lock().unwrap().chunks.push((
                owner.turn(),
                crate::scenario_clock::instant(),
                chunk.samples[0],
                monotonic_us(),
            ));
            let frames = chunk.samples.len();
            let at = monotonic_us();
            self.channels
                .control
                .send(WorkerEvent::Playback {
                    owner,
                    first_sample: PlaybackCursor {
                        epoch: 1,
                        frame: self.frame,
                    },
                    end_sample: PlaybackCursor {
                        epoch: 1,
                        frame: self.frame + frames as u64,
                    },
                    queue_observed_at_us: at,
                    turn: owner.turn(),
                    sequence: chunk.chunk_sequence,
                    queued_frames: frames,
                    accepted_frames: frames,
                    observed_at_us: at,
                })
                .unwrap();
            self.frame += frames as u64;
            if self.retires {
                let end = PlaybackCursor {
                    epoch: 1,
                    frame: self.frame,
                };
                self.channels
                    .control
                    .send(WorkerEvent::SpeechDrained {
                        owner,
                        sequence: chunk.chunk_sequence,
                        last_sample: end,
                        observed_at_us: at,
                    })
                    .unwrap();
                if chunk.end_of_speech {
                    self.channels
                        .control
                        .send(WorkerEvent::SpeechRetired {
                            owner,
                            sequence: chunk.chunk_sequence,
                            final_sample: end,
                            observed_at_us: at,
                        })
                        .unwrap();
                }
            }
        }
        true
    }
}

/// Capture, privacy and the speaker, each run once a step. A peer told to
/// stop is dropped with its channels, as its process ends.
struct PeripheralPeers {
    capture: Option<CapturePeer>,
    privacy: Option<PrivacyPeer>,
    speaker: Option<SpeakerPeer>,
}

impl PeripheralPeers {
    fn new(
        channels: [WorkerChannels; 3],
        script: Script,
        seen: Arc<Mutex<Seen>>,
        retires: bool,
    ) -> Self {
        let [capture, privacy, speaker] = channels;
        Self {
            capture: Some(CapturePeer {
                channels: capture,
                script,
                seen: seen.clone(),
                generation: 0,
                started: None,
            }),
            privacy: Some(PrivacyPeer {
                channels: privacy,
                generation: 0,
                next: crate::scenario_clock::instant(),
            }),
            speaker: Some(SpeakerPeer {
                channels: speaker,
                seen,
                generation: 0,
                frame: 1_000,
                retires,
            }),
        }
    }

    fn step(&mut self) {
        if self.capture.as_mut().is_some_and(|peer| !peer.step()) {
            self.capture = None;
        }
        if self.privacy.as_mut().is_some_and(|peer| !peer.step()) {
            self.privacy = None;
        }
        if self.speaker.as_mut().is_some_and(|peer| !peer.step()) {
            self.speaker = None;
        }
    }
}

struct Session {
    result: Result<()>,
    trace: Vec<Value>,
    seen: Seen,
}

impl Session {
    /// The session ran to its end; if not, why, with the provider worker's
    /// own reason and the last records.
    fn ok(&self) {
        if let Err(error) = &self.result {
            let last: Vec<&Value> = self.trace.iter().rev().take(12).collect();
            panic!(
                "the session ended: {error}; the provider worker: {:?}; the last records, newest first: {last:#?}",
                self.seen.worker
            );
        }
    }
    fn events(&self, kind: &str) -> Vec<&Value> {
        self.trace
            .iter()
            .filter(|row| row["kind"] == kind)
            .collect()
    }
    fn outcomes(&self) -> Vec<String> {
        self.events("turn_finished")
            .iter()
            .map(|row| row["outcome"].as_str().unwrap().to_owned())
            .collect()
    }
    /// Requests given up because someone spoke again before the lamp was sure.
    fn given_up(&self) -> usize {
        self.events("early_end_given_up").len()
    }
    /// What was sent and given up, said on the test's standard error: the
    /// counts the report on this option gives (`--nocapture` shows them).
    /// `last` is the last block of speech before the answer that played.
    fn summary(&self, name: &str, last: u64) {
        let seen = &self.seen;
        let audio = seen
            .wire
            .iter()
            .filter(|(_, what, _)| matches!(what, Wire::Audio(_)))
            .count();
        let other = seen
            .wire
            .iter()
            .filter(|(_, what, _)| matches!(what, Wire::Other(_)))
            .count();
        let first = seen
            .chunks
            .iter()
            .map(|(_, at, _, _)| *at)
            .filter(|at| *at >= seen.block(last))
            .min()
            .map(|at| self.after_block(last, at));
        eprintln!(
            "{name}: connections {}; activityStart {}, audio blocks {audio}, activityEnd {}, other {other}; requests given up {}; answers begun {}, blocks played {}; last sound to first block handed to the speaker {first:?} ms",
            seen.connections,
            seen.count(&Wire::Start),
            seen.count(&Wire::End),
            self.given_up(),
            seen.answers.len(),
            seen.chunks.len(),
        );
    }
    /// Milliseconds from the read of capture block `index` to `at`.
    fn after_block(&self, index: u64, at: Instant) -> i64 {
        let block = self.seen.block(index);
        if at >= block {
            (at - block).as_millis() as i64
        } else {
            -((block - at).as_millis() as i64)
        }
    }
}

struct Setup {
    early_ms: Option<u16>,
    turn_taking: TurnTaking,
    end_silence_blocks: Option<u16>,
    seconds: u64,
    script: Script,
    service: Service,
    /// `LAMP_CUT_KEEPS_NAME=on` (#782).
    cut_keeps_name: bool,
    /// `LAMP_FOLLOWS_ON=on` (#829).
    follows_on: bool,
    /// The speaker says what it played out and retires each answer's last
    /// block, so that an answer that plays settles ([`SpeakerPeer`]).
    retires: bool,
}

/// The fixture enters a scenario clock of its own, so none may be active
/// already.
fn require_no_outer_scenario_clock() {
    assert!(
        !crate::scenario_clock::is_active(),
        "no outer scenario clock: the fixture enters its own"
    );
}

/// Cancellation is expected only after this fixture requests it. A task panic
/// must still fail the test, including one that happened before cleanup began.
fn fixture_task_result<T>(
    result: std::result::Result<T, tokio::task::JoinError>,
    cancellation_requested: bool,
) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
        Err(error) if error.is_cancelled() && cancellation_requested => None,
        Err(error) => panic!("fixture task cancelled without a fixture request: {error}"),
    }
}

impl Setup {
    /// Today's lamp, but `vad-only`, whose own wait is the lamp's 600 ms
    /// and which leaves the lamp's own playback unmasked, so that a person
    /// can be heard over it without echo evidence.
    fn lamp(early_ms: Option<u16>, seconds: u64, script: Script, service: Service) -> Self {
        Self {
            early_ms,
            turn_taking: TurnTaking::VadOnly,
            end_silence_blocks: None,
            seconds,
            script,
            service,
            cut_keeps_name: false,
            follows_on: false,
            retires: false,
        }
    }

    /// Every caller explicitly selects the same bounded scheduling allowance.
    /// The gate belongs to this conversation; unrelated fixtures retain 50 ms.
    fn run_with_privacy_tolerance(self) -> Session {
        // Check before any process, provider runtime or sample producer
        // starts: the scenario's logical clock is its own, entered below.
        require_no_outer_scenario_clock();
        let fixture_privacy =
            PrivacyGate::with_fixture_freshness(FIXTURE_PRIVACY_FRESH_US).unwrap();
        let dir = SessionDirectory::create().unwrap();
        let (capture, capture_peer_channels) = Worker::sleeping_fixture(&dir.path, "capture");
        let (speaker, speaker_peer_channels) = Worker::sleeping_fixture(&dir.path, "speaker");
        let (provider, provider_peer_channels) = Worker::sleeping_fixture(&dir.path, "provider");
        let (privacy, privacy_peer_channels) = Worker::sleeping_fixture(&dir.path, "privacy");
        // Every child has crossed its startup barrier. From here the
        // coordinator, its peers, the provider worker and the scripted
        // service share one logical clock on this thread.
        let scope = crate::scenario_clock::Scope::enter(FIXTURE_SCENARIO_WATCHDOG).unwrap();
        let mut owner = Controller::new(new_boot().unwrap(), now());
        let boot = owner.snapshot(now()).unwrap().boot();
        let stop = Arc::new(AtomicBool::new(false));
        let seen = Arc::new(Mutex::new(Seen::default()));
        let service = self.service;
        let script = self.script;
        let peripheral_channels = [
            capture_peer_channels,
            privacy_peer_channels,
            speaker_peer_channels,
        ];
        // The real provider worker and session, and the scripted service, on
        // a paused current-thread runtime that runs only inside a step.
        let runtime = Rc::new(
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .start_paused(true)
                .build()
                .unwrap(),
        );
        let (worker, answering) = runtime.block_on(async {
            let config = Continuation::KeptStart
                .apply(
                    SessionConfig::new(
                        lamp_gemini::GOOGLE_ENDPOINT,
                        Credential::api_key("test-only").unwrap(),
                    )
                    .unwrap()
                    .resumption(Resumption::Retain)
                    .barrier_policy(BarrierPolicy::Require),
                )
                .unwrap();
            let (connect, dials) = connector(config);
            let provider = Supervisor::new(
                connect,
                RecoveryPolicy {
                    first_backoff: Duration::from_millis(20),
                    max_backoff: Duration::from_millis(40),
                    ..RecoveryPolicy::default()
                },
            )
            .unwrap();
            let worker = tokio::spawn(serve_coordinated_test(
                provider_peer_channels,
                boot,
                provider,
            ));
            let answering = tokio::spawn(serve_dials(dials, service, seen.clone(), stop.clone()));
            (worker, answering)
        });
        let mut rig = Rig {
            expression_out: None,
            second: None,
            microphone: crate::options::Microphone::Mic2,
            end_silence_blocks: self.end_silence_blocks,
            early_end_blocks: self.early_ms.map(|ms| ms / 10),
            faint_level: None,
            provider,
            capture,
            speaker,
            privacy,
            privacy_controls: false,
            ring: None.into(),
            camera: None,
            motor: None,
            ring_channel_ceiling: None,
            diagnostic_directory: None,
            noise_suppression: NoiseSuppression::On,
            turn_taking: self.turn_taking,
            // As lamp-4ace.
            playout_start: PlayoutStart::new(350).unwrap(),
            fixture_at_ms: None,
            endpoint: None,
            call_wait_blocks: None,
            cut_keeps_name: self.cut_keeps_name,
            asker_first: crate::asker_first::Mode::Off,
            follows_on: self.follows_on,
            paused: Box::new(|| false),
        };
        // The worker says it is ready before anything else. Until then only
        // the provider's runtime runs; the logical clock stands.
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            runtime.block_on(tokio::task::yield_now());
            match rig
                .provider
                .channels
                .control
                .receive::<WorkerEvent>()
                .unwrap()
            {
                Some(WorkerEvent::Ready) => break,
                Some(other) => panic!("before the worker was ready: {other:?}"),
                None => assert!(
                    Instant::now() < deadline,
                    "the provider worker never started"
                ),
            }
        }
        // Start sample producers only after the real provider's Ready barrier.
        // Startup cannot queue old privacy reads while the runtime is still
        // initializing; source observations keep their acquisition timestamps.
        let mut peers =
            PeripheralPeers::new(peripheral_channels, script, seen.clone(), self.retires);
        // Each step the provider's runtime first catches up with the step's
        // logical instant, its timers firing in order on the way, then each
        // peer runs; the clock moves only after. Cleanup holds the logical
        // instant, while the provider's runtime goes on a step at a time so
        // that its worker takes Stop as it would.
        let tokio_origin = runtime.block_on(async { tokio::time::Instant::now() });
        let logical_origin = crate::scenario_clock::instant();
        scope
            .set_pump({
                let runtime = runtime.clone();
                move |phase| {
                    runtime.block_on(async {
                        match phase {
                            crate::scenario_clock::Phase::Run => {
                                let at = crate::scenario_clock::instant() - logical_origin;
                                tokio::time::sleep_until(tokio_origin + at).await;
                            }
                            crate::scenario_clock::Phase::Shutdown { .. } => {
                                tokio::time::sleep(STEP).await
                            }
                        }
                        tokio::task::yield_now().await;
                    });
                    peers.step();
                    Ok(())
                }
            })
            .unwrap();
        let mut trace = SessionState::finite();
        let mut reply = None;
        let result = conversation_with_fixture_privacy(
            &mut rig,
            &mut owner,
            Lifetime::Finite(self.seconds),
            &mut trace,
            StartupEvents::default(),
            &mut reply,
            &mut None,
            &Termination::inactive(),
            &FailureNotices::none(),
            fixture_privacy,
            Instant::now() + FIXTURE_CONVERSATION_WATCHDOG,
        );
        // Stop future service sends before provider Stop can close its socket.
        // Both tasks share one current-thread runtime; the provider and every
        // peripheral peer remain active through the ordinary shared cleanup.
        answering.abort();
        let _ = rig.shutdown(&mut trace);
        stop.store(true, Ordering::SeqCst);
        worker.abort();
        runtime.block_on(async {
            // Join both before propagating either panic, so no task is left
            // running when this test unwinds.
            let worker_result = worker.await;
            let answering_result = answering.await;
            // A worker that ended by itself says why.
            if let Some(Err(error)) = fixture_task_result(worker_result, true) {
                seen.lock().unwrap().worker = Some(error.to_string());
            }
            let _ = fixture_task_result(answering_result, true);
        });
        drop(scope);
        let seen = std::mem::take(&mut *seen.lock().unwrap());
        Session {
            result,
            trace: trace.to_vec(),
            seen,
        }
    }
}

mod fixture_policy_tests {
    use super::*;

    #[tokio::test]
    async fn requested_task_cancellation_is_accepted_and_normal_results_survive() {
        let pending = tokio::spawn(std::future::pending::<()>());
        pending.abort();
        assert!(fixture_task_result(pending.await, true).is_none());
        let completed = tokio::spawn(async { 7u8 });
        assert_eq!(fixture_task_result(completed.await, false), Some(7));
    }

    #[tokio::test]
    async fn task_panic_payload_propagates_even_when_cancellation_was_requested() {
        struct TaskPanic;
        let panicking: tokio::task::JoinHandle<()> =
            tokio::spawn(async { std::panic::panic_any(TaskPanic) });
        let result = panicking.await;
        let propagated = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            fixture_task_result(result, true)
        }));
        assert!(propagated.unwrap_err().is::<TaskPanic>());
    }

    #[tokio::test]
    #[should_panic(expected = "fixture task cancelled without a fixture request")]
    async fn unrequested_task_cancellation_is_rejected() {
        let pending = tokio::spawn(std::future::pending::<()>());
        pending.abort();
        let _ = fixture_task_result(pending.await, false);
    }

    #[test]
    #[should_panic(expected = "no outer scenario clock: the fixture enters its own")]
    fn a_scoped_clock_is_rejected_before_provider_or_child_startup() {
        let _scope = crate::scenario_clock::Scope::enter(FIXTURE_CONVERSATION_WATCHDOG).unwrap();
        Setup::lamp(Some(200), 4, one_question, Service::answering_after(150))
            .run_with_privacy_tolerance();
    }

    #[test]
    fn delayed_capture_keeps_continuity_and_cannot_refresh_stale_audio() {
        let origin_us = 1_000_000;
        let mut schedule = CaptureSchedule::new(origin_us);
        let observed = |(sequence, captured_at_us)| ObservedAudio {
            sequence,
            captured_at_us,
            samples: [sequence as i16; 160],
        };
        let first = schedule.next_due(origin_us).unwrap();
        assert_eq!(first, (1, origin_us));
        assert!(schedule.next_due(origin_us + 9_999).is_none());
        let mut detector = TurnDetector::v1_baseline();
        assert!(matches!(
            detector.push(observed(first), QUIET),
            Activity::Quiet
        ));
        // The source continues for 60 ms without its delivery peer being run.
        // Every overdue block retains its own 10 ms acquisition timestamp.
        for sequence in 2..=7 {
            let next = schedule.next_due(origin_us + 60_000).unwrap();
            assert_eq!(next, (sequence, origin_us + 10_000 * (sequence - 1)));
            assert!(matches!(
                detector.push(observed(next), QUIET),
                Activity::Quiet
            ));
        }
        assert!(schedule.next_due(origin_us + 60_000).is_none());

        let mut overdue = CaptureSchedule::new(origin_us);
        assert_eq!(overdue.next_due(origin_us), Some(first));
        let at_us = origin_us + 110_000;
        let delayed = overdue.next_due(at_us).unwrap();
        assert_eq!(delayed, (2, origin_us + 10_000));
        assert_eq!(at_us - delayed.1, 100_000);
        // Exercise the actual admission gate with fixed observation times:
        // 99,999 us is fresh; exactly 100,000 us is refused without restamping.
        let at = MonoTime::from_micros(at_us - 1);
        let until = MonoTime::from_micros(at_us - 1 + 100_000);
        let mut owner = Controller::new(BootId::new([1; 16]).unwrap(), at);
        owner
            .set_microphone_permission(at, Permission::Allowed)
            .unwrap();
        owner
            .set_capture(at, CaptureState::RetainingUntil(until))
            .unwrap();
        owner
            .set_admission(at, AdmissionState::OpenUntil(until))
            .unwrap();
        let authority = owner.snapshot(at).unwrap();
        let context = AdmissionContext {
            capture: CaptureLineage {
                worker: BootId::new([2; 16]).unwrap(),
                epoch: 1,
                dsp_epoch: 1,
                privacy_generation: authority.microphone_generation(),
            },
            authority,
        };
        let fresh = InputAdmission::default()
            .observe(Activity::Start(vec![observed(delayed)]), context, at_us - 1)
            .unwrap();
        assert!(matches!(fresh.step, AdmissionStep::Candidate(_)));
        let stale = InputAdmission::default()
            .observe(Activity::Start(vec![observed(delayed)]), context, at_us)
            .unwrap_err();
        assert_eq!(stale.to_string(), "candidate trigger is stale or future");
    }
}

/// A question from block 100 to block 200, then quiet. The detector takes
/// it from block 105 with its 30 blocks of pre-roll: blocks 76 to 200.
fn one_question(block: u64) -> f32 {
    if (100..=200).contains(&block) {
        SPEECH
    } else {
        QUIET
    }
}
const LAST: u64 = 200;
/// The blocks a request with that pre-roll carries, through `through`.
fn blocks(from: u64, through: u64) -> Vec<Wire> {
    (from..=through)
        .map(|block| Wire::Audio(block as i16))
        .collect()
}
fn request(from: u64, through: u64) -> Vec<Wire> {
    let mut all = vec![Wire::Start];
    all.extend(blocks(from, through));
    all.push(Wire::End);
    all
}
/// A successor's request: the same, with the note that it goes on from the
/// request given up just before its end.
fn continuation(from: u64, through: u64) -> Vec<Wire> {
    let mut all = request(from, through);
    all.insert(
        all.len() - 1,
        Wire::Note(crate::provider_worker::CUT_OFF_NOTE.to_owned()),
    );
    all
}

/// Quiet to the end. The end goes 200 ms after the last sound, the answer
/// is in hand 150 ms later, and its first sound is handed to the speaker
/// at the moment the 600 ms wait ends, not before. One request, none given
/// up.
#[test]
fn quiet_to_the_end_the_answer_starts_when_the_lamp_is_sure_and_not_before() {
    let _one = one_session();
    let session = Setup::lamp(Some(200), 4, one_question, Service::answering_after(150))
        .run_with_privacy_tolerance();
    session.ok();
    session.summary("quiet to the end", LAST);
    let seen = &session.seen;
    assert_eq!(seen.connections, 1);
    assert_eq!(seen.heard(1), request(76, LAST + 20), "{:?}", seen.heard(1));
    // Sent 200 ms after the last sound, by the capture schedule.
    let sent = session.after_block(LAST, seen.end_at(0));
    assert!((200..300).contains(&sent), "end sent {sent} ms after");
    // The answer was in hand before the lamp was sure, and waited for it.
    let answer = session.after_block(LAST, seen.answers[0].1);
    assert!(answer < 600, "the answer began {answer} ms after");
    let first = session.after_block(LAST, seen.first_played(FIRST_ANSWER).unwrap());
    assert!((600..700).contains(&first), "first sound at {first} ms");
    assert!(
        seen.chunks
            .iter()
            .all(|(_, at, _, _)| *at >= seen.block(LAST + 60))
    );
    assert_eq!(seen.played(FIRST_ANSWER), 12 * 4);
    // The records: the end sent early, then the lamp's own end of speech
    // carrying it, and no request given up.
    let early = &session.events("early_end")[0];
    assert_eq!(early["pause_ms"], 200);
    let endpoint = &session.events("local_endpoint")[0];
    assert_eq!(endpoint["includes_silence_wait_ms"], 600);
    assert_eq!(endpoint["early_end"]["at_us"], early["at_us"]);
    assert!(endpoint["early_end"]["before_this_ms"].as_u64().unwrap() >= 350);
    assert_eq!(session.given_up(), 0);
    assert_eq!(session.events("early_end_policy").len(), 1);
}

/// The person goes on 0.3 s after they stopped: "Hey Lamp," (blocks 100 to
/// 160), 300 ms of quiet, and the question (191 to 290). The end of the
/// first part went at 200 ms; at the sixth block of speech again it is
/// given up exactly as a continued utterance is. The service hears the
/// first part, then (once its answer has begun and been stopped) the rest
/// in a request of its own. Nothing of the first answer is played; the
/// second is, after the lamp is sure of the whole.
fn name_then_question(block: u64) -> f32 {
    if (100..=160).contains(&block) || (191..=290).contains(&block) {
        SPEECH
    } else {
        QUIET
    }
}

fn resumed_at_three_tenths(name: &str, setup: Setup) {
    let session = setup.run_with_privacy_tolerance();
    session.ok();
    session.summary(name, 290);
    let seen = &session.seen;
    // Given up once, at the sixth block of speech again (196), with what
    // was heard since its end (181 to 196) the successor's opening.
    assert_eq!(session.given_up(), 1);
    let given_up = &session.events("early_end_given_up")[0];
    assert_eq!(given_up["opening_blocks"], 16);
    // The successor is told, just before its end, that it goes on from
    // the request given up (docs/early-end-taken-back.md).
    let mut expected = request(76, 180);
    let mut successor = request(181, 290 + 20);
    successor.insert(
        successor.len() - 1,
        Wire::Note(crate::provider_worker::CUT_OFF_NOTE.to_owned()),
    );
    expected.extend(successor);
    assert_eq!(seen.heard(1), expected, "{:?}", seen.heard(1));
    assert_eq!(seen.count(&Wire::Start), 2);
    assert_eq!(seen.count(&Wire::End), 2);
    // Two answers were begun; only the second was heard.
    assert_eq!(seen.answers.len(), 2);
    assert_eq!(seen.played(FIRST_ANSWER), 0);
    assert_eq!(seen.played(FIRST_ANSWER + 1), 12 * 4);
    let first = session.after_block(290, seen.first_played(FIRST_ANSWER + 1).unwrap());
    assert!(first >= 600, "first sound {first} ms after the last");
    assert!(
        seen.chunks
            .iter()
            .all(|(_, at, _, _)| *at >= seen.block(290 + 60))
    );
    // One turn heard: the first part is a turn given up as continued, and
    // the lamp's own end of speech came once, after the whole.
    let outcomes = session.outcomes();
    assert_eq!(outcomes[0], "user_interrupted", "{outcomes:?}");
    assert_eq!(session.events("local_endpoint").len(), 1);
    let admitted = session.events("input_admitted");
    assert_eq!(admitted.len(), 2);
    assert_eq!(admitted[1]["admission_basis"], "continued_after_early_end");
    assert_eq!(given_up["successor"], admitted[1]["turn"]);
    assert!(session.events("provider_input_discarded").is_empty());
}

#[test]
fn speech_resumed_at_three_tenths_nothing_of_the_first_answer_is_played_and_one_turn_is_heard() {
    let _one = one_session();
    resumed_at_three_tenths(
        "speech resumed at 0.3 s, vad-only",
        Setup::lamp(
            Some(200),
            6,
            name_then_question,
            Service::answering_after(900),
        ),
    );
}

/// The same with lamp-4ace's own policy: duplex, 600 ms.
#[test]
fn speech_resumed_at_three_tenths_under_the_lamps_duplex_policy() {
    let _one = one_session();
    resumed_at_three_tenths(
        "speech resumed at 0.3 s, duplex 600 ms",
        Setup {
            early_ms: Some(200),
            turn_taking: TurnTaking::Duplex,
            end_silence_blocks: Some(60),
            seconds: 6,
            script: name_then_question,
            service: Service::answering_after(900),
            cut_keeps_name: false,
            follows_on: false,
            retires: false,
        },
    );
}

/// A question in four pieces, as lamp-4ace heard one at 13:45: three pauses
/// of 300 ms (blocks 161 to 190, 241 to 270 and 321 to 350), each longer
/// than the 200 ms of the early end and shorter than the lamp's own 600 ms.
/// The end goes early 200 ms into each pause (after blocks 180, 260 and
/// 340) and is given up at the sixth block of speech again (196, 276 and
/// 356), the 16 blocks heard since the end the successor's opening: the
/// requests carry 76 to 180, 181 to 260, 261 to 340 and 341 to 470. Each
/// successor is told, just before its own end, that it goes on from the
/// request cut off; the first is not. The service answers 50 ms after each
/// end, so each piece's answer has begun, and its sound is held by the lamp,
/// when the person goes on, as at 13:45. Of the four answers begun only the
/// last is heard, once the lamp is sure of the whole.
fn four_pieces(block: u64) -> f32 {
    if (100..=160).contains(&block)
        || (191..=240).contains(&block)
        || (271..=320).contains(&block)
        || (351..=450).contains(&block)
    {
        SPEECH
    } else {
        QUIET
    }
}

#[test]
fn a_question_in_four_pieces_tells_each_successor_and_plays_only_the_last_answer() {
    let _one = one_session();
    let session = Setup::lamp(Some(200), 7, four_pieces, Service::answering_after(50))
        .run_with_privacy_tolerance();
    session.ok();
    session.summary("a question in four pieces", 450);
    let seen = &session.seen;
    assert_eq!(seen.connections, 1);
    // The first piece alone, then each successor from the end before it:
    // the note once in each, just before its end, and never in the first.
    let mut expected = request(76, 180);
    expected.extend(continuation(181, 260));
    expected.extend(continuation(261, 340));
    expected.extend(continuation(341, 450 + 20));
    assert_eq!(seen.heard(1), expected, "{:?}", seen.heard(1));
    let note = Wire::Note(crate::provider_worker::CUT_OFF_NOTE.to_owned());
    assert_eq!(seen.count(&note), 3);
    assert_eq!(seen.count(&Wire::Start), 4);
    assert_eq!(seen.count(&Wire::End), 4);
    // Four answers were begun; nothing of the three abandoned was played,
    // and the last was, after the lamp was sure of the whole.
    assert_eq!(seen.answers.len(), 4);
    for abandoned in FIRST_ANSWER..FIRST_ANSWER + 3 {
        assert_eq!(seen.played(abandoned), 0, "answer {abandoned}");
    }
    assert_eq!(seen.played(FIRST_ANSWER + 3), 12 * 4);
    assert_eq!(seen.chunks.len(), 12 * 4);
    assert!(
        seen.chunks
            .iter()
            .all(|(_, at, _, _)| *at >= seen.block(450 + 60))
    );
    // Given up three times, each at the sixth block of speech again, with
    // what was heard since its end (16 blocks) its successor's opening.
    assert_eq!(session.given_up(), 3);
    assert_eq!(session.events("early_end").len(), 4);
    let admitted = session.events("input_admitted");
    assert_eq!(admitted.len(), 4);
    assert_ne!(admitted[0]["admission_basis"], "continued_after_early_end");
    for (given_up, pair) in session
        .events("early_end_given_up")
        .iter()
        .zip(admitted.windows(2))
    {
        assert_eq!(given_up["opening_blocks"], 16);
        assert_eq!(given_up["turn"], pair[0]["turn"]);
        assert_eq!(given_up["successor"], pair[1]["turn"]);
        assert_eq!(pair[1]["admission_basis"], "continued_after_early_end");
    }
    // One turn heard: the lamp's own end of speech came once, after the
    // whole, and the three pieces before it are turns given up.
    assert_eq!(session.events("local_endpoint").len(), 1);
    let outcomes = session.outcomes();
    assert_eq!(outcomes[..3], ["user_interrupted"; 3], "{outcomes:?}");
    assert!(session.events("provider_input_discarded").is_empty());
}

/// An answer that has all arrived, and completed, before the lamp is sure
/// (four packets, 160 ms, sent at once after the end): held whole, and
/// begun at that moment.
#[test]
fn an_answer_that_arrives_before_the_lamp_is_sure_begins_at_that_moment() {
    let _one = one_session();
    let session = Setup::lamp(
        Some(200),
        4,
        one_question,
        Service {
            packets: 4,
            ..Service::answering_after(0)
        },
    )
    .run_with_privacy_tolerance();
    session.ok();
    session.summary("an answer before the lamp is sure", LAST);
    let seen = &session.seen;
    assert_eq!(seen.heard(1), request(76, LAST + 20));
    let answer = session.after_block(LAST, seen.answers[0].1);
    assert!(answer < 400, "answer began {answer} ms after");
    let first = session.after_block(LAST, seen.first_played(FIRST_ANSWER).unwrap());
    assert!((600..700).contains(&first), "first sound at {first} ms");
    assert_eq!(seen.played(FIRST_ANSWER), 4 * 4);
    // The service had finished it before the lamp was sure.
    let complete = session.events("provider_turn_complete")[0]["at_us"]
        .as_u64()
        .unwrap();
    let sure = session.events("local_endpoint")[0]["at_us"]
        .as_u64()
        .unwrap();
    assert!(complete < sure);
    assert_eq!(session.given_up(), 0);
}

/// One that opens with nothing said ("..."), before the lamp is sure: noted
/// then, and ended unheard at that moment.
#[test]
fn an_answer_with_nothing_said_before_the_lamp_is_sure_is_never_played() {
    let _one = one_session();
    let session = Setup::lamp(
        Some(200),
        4,
        one_question,
        Service {
            words: "...",
            ..Service::answering_after(0)
        },
    )
    .run_with_privacy_tolerance();
    session.ok();
    session.summary("nothing said before the lamp is sure", LAST);
    assert!(session.seen.chunks.is_empty());
    assert_eq!(session.outcomes(), ["nothing_said"]);
    let finished = session.events("turn_finished")[0]["at_us"]
        .as_u64()
        .unwrap();
    let sure = session.events("local_endpoint")[0]["at_us"]
        .as_u64()
        .unwrap();
    assert!(finished >= sure);
}

/// One that arrives after the lamp is sure: begun on its own start gate as
/// any answer is, 400 ms sooner than without the option.
#[test]
fn an_answer_that_arrives_after_the_lamp_is_sure_begins_400_ms_sooner_than_without() {
    let _one = one_session();
    let on = Setup::lamp(Some(200), 5, one_question, Service::answering_after(900))
        .run_with_privacy_tolerance();
    let off = Setup::lamp(None, 5, one_question, Service::answering_after(900))
        .run_with_privacy_tolerance();
    on.ok();
    off.ok();
    on.summary("an answer after the lamp is sure, with the option", LAST);
    off.summary("an answer after the lamp is sure, without it", LAST);
    let (on_first, off_first) = (
        on.after_block(LAST, on.seen.first_played(FIRST_ANSWER).unwrap()),
        off.after_block(LAST, off.seen.first_played(FIRST_ANSWER).unwrap()),
    );
    let (on_sent, off_sent) = (
        on.after_block(LAST, on.seen.end_at(0)),
        off.after_block(LAST, off.seen.end_at(0)),
    );
    eprintln!(
        "last sound to the end sent: off {off_sent} ms, on {on_sent} ms; to the first block handed to the speaker: off {off_first} ms, on {on_first} ms"
    );
    assert!((600..700).contains(&off_sent) && (200..300).contains(&on_sent));
    // 900 ms of service and the 350 ms start gate after the end, either
    // way; the difference is the 400 ms, within the clocks' play.
    assert!(on_first >= 200 + 900 && off_first >= 600 + 900);
    assert!(
        (300..500).contains(&(off_first - on_first)),
        "{off_first} {on_first}"
    );
    assert_eq!(on.seen.heard(1), request(76, LAST + 20));
    assert_eq!(off.seen.heard(1), request(76, LAST + 60));
}

/// The link drops between the end sent early and the lamp being sure. The
/// request fails there, nothing is played, nothing is sent again on the
/// new connection, and the next question is heard and answered on it.
fn two_questions(block: u64) -> f32 {
    if (100..=200).contains(&block) || (350..=450).contains(&block) {
        SPEECH
    } else {
        QUIET
    }
}

#[test]
fn the_link_dropping_in_between_fails_the_request_unheard_and_the_next_is_answered() {
    let _one = one_session();
    let session = Setup::lamp(
        Some(200),
        6,
        two_questions,
        Service {
            drop_after_end: Some((1, Duration::from_millis(100))),
            ..Service::answering_after(150)
        },
    )
    .run_with_privacy_tolerance();
    session.ok();
    session.summary("the link dropping in between", 450);
    let seen = &session.seen;
    assert_eq!(seen.connections, 2);
    assert_eq!(seen.heard(1), request(76, LAST + 20));
    // The second question, from its own pre-roll, on the new connection.
    assert_eq!(seen.heard(2), request(326, 450 + 20), "{:?}", seen.heard(2));
    assert_eq!(seen.answers.len(), 1);
    assert_eq!(seen.played(FIRST_ANSWER), 12 * 4);
    assert!(
        seen.chunks
            .iter()
            .all(|(_, at, _, _)| *at >= seen.block(450 + 60))
    );
    let outcomes = session.outcomes();
    assert_eq!(outcomes[0], "provider_failed", "{outcomes:?}");
    assert_eq!(session.given_up(), 0);
}

/// The service fails in the middle of a question in pieces, as at 13:45:
/// "Hey Lamp," (blocks 100 to 160), 300 ms of quiet, and the rest (191 to
/// 240). The end of the first part goes after block 180, its answer begins
/// 50 ms later, and it is given up at 196; the rest, told it goes on from
/// the request cut off, ends early after 260, and the link drops 100 ms
/// later, before the lamp is sure. That request fails unheard, nothing is
/// sent again on the new connection, and a later question of its own (400
/// to 500) is heard there as a plain request, with no note, and answered.
fn pieces_then_another_question(block: u64) -> f32 {
    if (100..=160).contains(&block) || (191..=240).contains(&block) || (400..=500).contains(&block)
    {
        SPEECH
    } else {
        QUIET
    }
}

#[test]
fn the_link_dropping_after_a_continued_request_fails_it_unheard_and_the_next_has_no_note() {
    let _one = one_session();
    let session = Setup::lamp(
        Some(200),
        7,
        pieces_then_another_question,
        Service {
            drop_after_end: Some((2, Duration::from_millis(100))),
            ..Service::answering_after(50)
        },
    )
    .run_with_privacy_tolerance();
    session.ok();
    session.summary("the link dropping after a continued request", 500);
    let seen = &session.seen;
    assert_eq!(seen.connections, 2);
    // The first part, then the rest with its note, on the link that drops.
    let mut expected = request(76, 180);
    expected.extend(continuation(181, 240 + 20));
    assert_eq!(seen.heard(1), expected, "{:?}", seen.heard(1));
    // Nothing of either sent again; the next question from its own
    // pre-roll, with no note.
    assert_eq!(seen.heard(2), request(376, 500 + 20), "{:?}", seen.heard(2));
    // The first part's answer was begun and given up; the rest's never
    // came; only the next question's was heard, once the lamp was sure.
    assert_eq!(seen.answers.len(), 2);
    assert_eq!(seen.played(FIRST_ANSWER), 0);
    assert_eq!(seen.played(FIRST_ANSWER + 1), 12 * 4);
    assert_eq!(seen.chunks.len(), 12 * 4);
    assert!(
        seen.chunks
            .iter()
            .all(|(_, at, _, _)| *at >= seen.block(500 + 60))
    );
    assert_eq!(session.given_up(), 1);
    let admitted = session.events("input_admitted");
    assert_eq!(admitted.len(), 3);
    assert_eq!(admitted[1]["admission_basis"], "continued_after_early_end");
    assert_ne!(admitted[2]["admission_basis"], "continued_after_early_end");
    let outcomes = session.outcomes();
    assert_eq!(
        outcomes[..2],
        ["user_interrupted", "provider_failed"],
        "{outcomes:?}"
    );
}

/// An interruption, the second case most likely to go wrong: "Lamp, stop."
/// over an answer. The answer to a first question is playing when "Lamp,"
/// (blocks 300 to 325) is said over it, then 300 ms of quiet, then "stop."
/// (356 to 380). "Lamp," interrupts the answer at once, as always; its own
/// end goes after 200 ms, and "stop." gives that request up and goes on as
/// its successor. Nothing of the answer to "Lamp," is played; the answer to
/// "Lamp, stop." is, once the lamp is sure.
fn lamp_stop_over_an_answer(block: u64) -> f32 {
    if (100..=200).contains(&block) || (300..=325).contains(&block) || (356..=380).contains(&block)
    {
        SPEECH
    } else {
        QUIET
    }
}

#[test]
fn lamp_stop_over_an_answer_stops_it_and_only_the_answer_to_the_whole_is_played() {
    let _one = one_session();
    let session = Setup::lamp(
        Some(200),
        5,
        lamp_stop_over_an_answer,
        Service {
            packets: 50,
            ..Service::answering_after(150)
        },
    )
    .run_with_privacy_tolerance();
    session.ok();
    session.summary("Lamp, stop. over an answer", 380);
    let seen = &session.seen;
    // Three requests: the question, "Lamp," (written at once over an
    // answer that had begun) and "stop." (kept until the answer to "Lamp,"
    // began, then written over it).
    let heard = seen.heard(1);
    assert_eq!(seen.count(&Wire::Start), 3, "{heard:?}");
    assert_eq!(seen.count(&Wire::End), 3, "{heard:?}");
    let starts: Vec<usize> = heard
        .iter()
        .enumerate()
        .filter(|(_, what)| **what == Wire::Start)
        .map(|(at, _)| at)
        .collect();
    // "Lamp," from its pre-roll to 200 ms after it; "stop." from then on.
    assert_eq!(heard[starts[1] + 1], Wire::Audio(276));
    assert_eq!(heard[starts[2] + 1], Wire::Audio(346));
    assert_eq!(*heard.last().unwrap(), Wire::End);
    // "stop." goes on from "Lamp," given up: the model is told so just
    // before its end, and no other request is.
    assert_eq!(
        heard[heard.len() - 2],
        Wire::Note(crate::provider_worker::CUT_OFF_NOTE.to_owned())
    );
    assert_eq!(
        heard
            .iter()
            .filter(|what| matches!(what, Wire::Note(_)))
            .count(),
        1
    );
    assert_eq!(heard[heard.len() - 3], Wire::Audio(380 + 20));
    // The first answer played until "Lamp," was heard over it, and not a
    // block of it after: every one came before any block of a later turn,
    // and was taken by the speaker within its permit's lease of the
    // coordinator stopping that answer (the coordinator's own clock,
    // whatever the load on the machine running the test).
    assert!(seen.played(FIRST_ANSWER) > 0);
    let stopped = session.events("turn_finished")[0]["at_us"]
        .as_u64()
        .unwrap();
    let first_turn = seen.chunks[0].0;
    let later = seen
        .chunks
        .iter()
        .position(|(turn, ..)| *turn != first_turn)
        .expect("a later answer was played");
    assert!(seen.chunks[..later].iter().all(|(_, _, sample, at_us)| {
        *sample == FIRST_ANSWER && *at_us <= stopped + SPEAKER_PERMIT_LEASE_US
    }));
    assert!(
        seen.chunks[later..]
            .iter()
            .all(|(turn, _, sample, _)| *turn != first_turn && *sample != FIRST_ANSWER)
    );
    // The answer to "Lamp," alone was begun and never heard; the answer to
    // the whole was, after the lamp was sure.
    assert_eq!(seen.answers.len(), 3);
    assert_eq!(seen.played(FIRST_ANSWER + 1), 0);
    let last = seen
        .first_played(FIRST_ANSWER + 2)
        .expect("the last answer");
    assert!(last >= seen.block(380 + 60));
    assert_eq!(session.given_up(), 1);
    let outcomes = session.outcomes();
    assert_eq!(outcomes[..2], ["user_interrupted", "user_interrupted"]);
}

/// A lone "Lamp," is too short to be sent by itself: it would be thrown
/// out as a sound if the quiet went on. Its end is not sent early, its
/// pause of 450 ms is part of the utterance as it always was, and the name
/// and the question go as one request.
fn bare_name_then_question(block: u64) -> f32 {
    if (100..=124).contains(&block) || (170..=270).contains(&block) {
        SPEECH
    } else {
        QUIET
    }
}

#[test]
fn a_lone_lamp_and_a_pause_and_the_question_go_as_one_request() {
    let _one = one_session();
    let session = Setup::lamp(
        Some(200),
        5,
        bare_name_then_question,
        Service::answering_after(900),
    )
    .run_with_privacy_tolerance();
    session.ok();
    session.summary("a lone Lamp, a pause and the question", 270);
    let seen = &session.seen;
    assert_eq!(seen.heard(1), request(76, 270 + 20), "{:?}", seen.heard(1));
    assert_eq!(session.given_up(), 0);
    assert_eq!(session.events("early_end").len(), 1);
    assert_eq!(seen.played(FIRST_ANSWER), 12 * 4);
}

/// With the option off every message to the service and every record is
/// what it was: the end at the 600 ms decision, the same requests, and no
/// record or field of the option's.
#[test]
fn without_the_option_every_message_and_record_is_what_it_was() {
    let _one = one_session();
    let session = Setup::lamp(None, 6, name_then_question, Service::answering_after(150))
        .run_with_privacy_tolerance();
    session.ok();
    session.summary("without the option: Hey Lamp, 0.3 s, the question", 290);
    let seen = &session.seen;
    // "Hey Lamp," and the question are one utterance: 300 ms of quiet is
    // inside the 600 ms wait.
    assert_eq!(seen.heard(1), request(76, 290 + 60));
    let sent = session.after_block(290, seen.end_at(0));
    assert!((600..700).contains(&sent), "{sent}");
    for kind in ["early_end_policy", "early_end", "early_end_given_up"] {
        assert!(session.events(kind).is_empty(), "{kind}");
    }
    let endpoint = &session.events("local_endpoint")[0];
    let mut fields: Vec<&str> = endpoint
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    assert_eq!(
        fields,
        [
            "acoustic_speech_end",
            "at_us",
            "includes_silence_wait_ms",
            "kind",
            "last_block_host_read_us",
            "owner",
            "speech_level",
            "turn",
        ]
    );
    assert!(
        session
            .events("input_admitted")
            .iter()
            .all(|row| row["admission_basis"] != "continued_after_early_end")
    );
    // The run's first record and the provider worker's arguments, with the
    // option and without: the option adds one field to the one and
    // nothing to the other.
    let options = DirectedOptions::parse(
        &[
            "--turn-taking",
            "duplex",
            "--provider-continuation",
            "kept-start",
            "--end-silence-ms",
            "600",
        ]
        .map(str::to_owned),
    )
    .unwrap();
    let on = DirectedOptions {
        early_end_blocks: Some(20),
        ..options
    };
    let config = std::path::Path::new("/private/provider.json");
    for lifetime in [Lifetime::Serve, Lifetime::Finite(60)] {
        assert_eq!(
            provider_arguments(ProviderSource::Gemini(config), &on, lifetime, None, false).unwrap(),
            provider_arguments(
                ProviderSource::Gemini(config),
                &options,
                lifetime,
                None,
                false
            )
            .unwrap()
        );
        let mut with = run_start_record(&on, lifetime, false, false, None, None, false);
        let without = run_start_record(&options, lifetime, false, false, None, None, false);
        assert!(without.get("early_end_ms").is_none());
        assert_eq!(with["early_end_ms"], 200);
        with.as_object_mut().unwrap().remove("early_end_ms");
        with["at_us"] = without["at_us"].clone();
        assert_eq!(with, without);
    }
}

/// A name note told with a request whose end went early stays inside that
/// request's own activity, just before its end. When the request is then
/// given up, continued or abandoned, the next request is an activity of its
/// own with no note in it; and a note offered once the early end has gone
/// is refused, with the link left as it was (`docs/voice-review-2026-10-14.md`,
/// finding 1). Through the supervisor and a real session, configured as
/// `--provider-continuation kept-start` configures them, called in the
/// order the provider worker calls them for the coordinator's early end:
/// `retire` for a request continued by a newer owner, `discard` for one
/// with no owner after it (`ProviderIntake::synchronize`). On a paused
/// clock: the session's timers and each wait for quiet pass only once
/// every task is idle (#823).
#[tokio::test(start_paused = true)]
async fn a_name_note_told_with_a_request_ended_early_does_not_survive_it() {
    require_no_outer_scenario_clock();
    use lamp_gemini::{Notice, RequestId, testing::next_dial};
    const NOTE: &str = crate::called::NOTE;
    const WAIT: Duration = Duration::from_secs(3);
    async fn everything(dial: &mut Dial, quiet: Duration) -> Vec<String> {
        let mut all = Vec::new();
        while let Some(message) = dial.service.try_receive(quiet).await {
            all.push(if message["realtimeInput"]["text"] == NOTE {
                "note".to_owned()
            } else {
                match wire(&message) {
                    Wire::Start => "start".to_owned(),
                    Wire::End => "end".to_owned(),
                    Wire::Audio(value) => format!("audio of {value}"),
                    // As this test read any other note before notes had
                    // a kind of their own.
                    Wire::Note(_) => message.to_string(),
                    Wire::Other(other) => other,
                }
            });
        }
        all
    }
    for continued in [true, false] {
        let config = Continuation::KeptStart
            .apply(
                SessionConfig::new(
                    lamp_gemini::GOOGLE_ENDPOINT,
                    Credential::api_key("test-only").unwrap(),
                )
                .unwrap()
                .resumption(Resumption::Retain)
                .barrier_policy(BarrierPolicy::Require),
            )
            .unwrap();
        let (dialer, mut dials) = connector(config);
        let mut supervisor = Supervisor::new(
            dialer,
            RecoveryPolicy {
                first_backoff: Duration::from_millis(20),
                max_backoff: Duration::from_millis(40),
                ..RecoveryPolicy::default()
            },
        )
        .unwrap();
        let (ready, mut dial) = tokio::join!(
            async {
                tokio::time::timeout(WAIT, supervisor.next())
                    .await
                    .expect("ready in time")
            },
            async {
                let mut dial = next_dial(&mut dials).await;
                dial.service.accept().await;
                dial
            }
        );
        assert!(matches!(ready, Ok(Notice::Ready { .. })), "{ready:?}");
        let (one, two) = (RequestId::new(1).unwrap(), RequestId::new(2).unwrap());
        // "Hey Lamp,": its sound, the note while it is heard, and its end
        // after 200 ms of quiet.
        supervisor.try_start(one).unwrap();
        supervisor
            .try_audio(one, 0, std::time::Instant::now(), &[1; 160])
            .unwrap();
        supervisor.try_note(one, NOTE).unwrap();
        supervisor.try_end(one).unwrap();
        // The name heard a moment later is not told: that request's sound
        // has ended. The link stays.
        assert!(supervisor.try_note(one, NOTE).is_err());
        let mut sent = everything(&mut dial, Duration::from_millis(150)).await;
        assert_eq!(sent, ["start", "audio of 1", "note", "end"]);
        // The person goes on before the lamp is sure (continued), or the
        // request is given up with nothing after it (abandoned) and someone
        // speaks later.
        if continued {
            supervisor.retire(one);
        } else {
            supervisor.discard(one);
        }
        supervisor.try_start(two).unwrap();
        supervisor
            .try_audio(two, 0, std::time::Instant::now(), &[2; 160])
            .unwrap();
        // Kept until the given-up request's answer begins, then over it.
        assert!(
            everything(&mut dial, Duration::from_millis(100))
                .await
                .is_empty()
        );
        dial.service.send(audio(9, 960)).await;
        sent.extend(everything(&mut dial, Duration::from_millis(150)).await);
        dial.service.burst([interrupted(), idle()]).await;
        supervisor.try_end(two).unwrap();
        sent.extend(everything(&mut dial, Duration::from_millis(150)).await);
        assert_eq!(
            sent,
            [
                "start",
                "audio of 1",
                "note",
                "end",
                "start",
                "audio of 2",
                "end"
            ],
            "continued: {continued}"
        );
        assert!(supervisor.is_connected());
    }
}

/// "Hey Lamp," ended by the 600 ms window, the question begun 100 ms after
/// that end, and "stop" over the answer to it once it is heard, 700 ms
/// after the question's end.
fn name_window_question_then_over_the_answer(block: u64) -> f32 {
    if (100..=150).contains(&block) || (221..=320).contains(&block) || (450..=480).contains(&block)
    {
        SPEECH
    } else {
        QUIET
    }
}

/// With `LAMP_CUT_KEEPS_NAME=on` and no end sent early, as lamp-4ace runs
/// (the follow-up to #809): the question goes on with the name the window
/// cut it from, and "stop" over the answer that is played is cutting in,
/// and does not. Without the switch no admission says it goes on.
#[test]
fn a_question_after_the_window_ended_its_name_goes_on_and_cutting_in_does_not() {
    let _one = one_session();
    let run = |cut_keeps_name| {
        Setup {
            cut_keeps_name,
            ..Setup::lamp(
                None,
                8,
                name_window_question_then_over_the_answer,
                Service::answering_after(300),
            )
        }
        .run_with_privacy_tolerance()
    };
    let continues = |session: &Session| -> Vec<bool> {
        session
            .events("input_admitted")
            .iter()
            .map(|row| row.get("continues") == Some(&Value::Bool(true)))
            .collect()
    };
    let on = run(true);
    on.ok();
    on.summary("with the switch: Hey Lamp, 0.7 s, the question, stop", 480);
    assert_eq!(continues(&on), [false, true, false]);
    // The answer to the question was being played when "stop" began.
    let answered = on
        .seen
        .first_played(FIRST_ANSWER + 1)
        .expect("the answer to the question");
    assert!(answered < on.seen.block(450));
    assert_eq!(on.outcomes()[..2], ["user_interrupted", "user_interrupted"]);

    let off = run(false);
    off.ok();
    assert_eq!(continues(&off), [false, false, false]);
    assert!(
        off.events("input_admitted")
            .iter()
            .all(|row| row.get("continues").is_none())
    );
}

/// A question from block 100 to 200, then another from 1550 to 1650: about
/// 12 s after the answer to the first finishes playing.
fn question_then_one_twelve_seconds_on(block: u64) -> f32 {
    if (100..=200).contains(&block) || (1550..=1650).contains(&block) {
        SPEECH
    } else {
        QUIET
    }
}

/// The same, the second from 2550 to 2650: about 22 s after.
fn question_then_one_twenty_two_seconds_on(block: u64) -> f32 {
    if (100..=200).contains(&block) || (2550..=2650).contains(&block) {
        SPEECH
    } else {
        QUIET
    }
}

/// #829, the first builder at 09:01, and El on #1278: with
/// `LAMP_FOLLOWS_ON=on` an answer that plays to its end starts the clock in
/// the coordinator. A question begun about 12 s after its last sound is
/// marked in its record and sent with the note beside it; one about 22 s
/// after is not. Without the switch neither is.
#[test]
fn with_the_switch_a_question_soon_after_the_answer_is_told_it_may_follow_on() {
    let _one = one_session();
    let run = |script: Script, seconds: u64, follows_on: bool| {
        let session = Setup {
            follows_on,
            retires: true,
            ..Setup::lamp(Some(200), seconds, script, Service::answering_after(150))
        }
        .run_with_privacy_tolerance();
        session.ok();
        session
    };
    let noted = |session: &Session| {
        session
            .seen
            .count(&Wire::Note(crate::follows_on::NOTE.to_owned()))
    };
    let marked = |session: &Session| {
        session
            .events("input_admitted")
            .iter()
            .filter(|row| row["follows_on"] == true)
            .count()
    };
    // From the first answer's last sound to the second question's first.
    let gap = |session: &Session, second: u64| {
        let last = session
            .seen
            .chunks
            .iter()
            .filter(|(_, _, sample, _)| *sample == FIRST_ANSWER)
            .map(|(_, at, _, _)| *at)
            .max()
            .expect("the first answer played");
        session.seen.block(second) - last
    };
    let near = run(question_then_one_twelve_seconds_on, 18, true);
    let near_gap = gap(&near, 1550);
    assert!(
        (Duration::from_secs(11)..Duration::from_secs(13)).contains(&near_gap),
        "{near_gap:?}"
    );
    assert_eq!(near.seen.played(FIRST_ANSWER), 12 * 4);
    assert_eq!((marked(&near), noted(&near)), (1, 1));
    let far = run(question_then_one_twenty_two_seconds_on, 28, true);
    let far_gap = gap(&far, 2550);
    assert!(
        (Duration::from_secs(21)..Duration::from_secs(23)).contains(&far_gap),
        "{far_gap:?}"
    );
    assert_eq!((marked(&far), noted(&far)), (0, 0));
    let off = run(question_then_one_twelve_seconds_on, 18, false);
    assert_eq!((marked(&off), noted(&off)), (0, 0));
}
