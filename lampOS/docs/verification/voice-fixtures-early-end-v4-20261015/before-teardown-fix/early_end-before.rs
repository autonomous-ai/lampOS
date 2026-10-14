//! The end sent early (`--early-end-ms`; docs/faster-answers.md, way 2),
//! through the coordinator's own loop. Capture, privacy and speaker are
//! emulated peers on the production channels, as in `endpoint_session`. The
//! provider is the real Coordinated worker and Gemini session, started as
//! `--provider-continuation kept-start` starts it, against the scripted
//! service of `lamp_gemini::testing`, which answers every request it is
//! asked to and stops an answer when a new activity begins over it, as the
//! real one does with `START_OF_ACTIVITY_INTERRUPTS`.
//!
//! Capture blocks follow a 10 ms schedule in real time, and each carries
//! its own number as its samples, so that what reached the service can be
//! read back block by block. The detector's decisions are counted in
//! blocks and are exact; the times the service takes are real and are
//! asserted with room to spare. No audio device, cloud service or model
//! is used.
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
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};
use tokio::sync::mpsc::UnboundedReceiver;

const SPEECH: f32 = 0.95;
const QUIET: f32 = 0.05;
// This selected provider integration accepts bounded host scheduling delay;
// it is not a measurement of the lamp's 50 ms privacy response. IPC/capture
// freshness, real clocks and the real 300 ms shared cleanup budget are intact.
const FIXTURE_PRIVACY_FRESH_US: u64 = 100_000;
const FIXTURE_CONVERSATION_WATCHDOG: Duration = Duration::from_secs(12);
/// Speech probability of each 10 ms capture block, by block number.
type Script = fn(u64) -> f32;

/// One client message, as the service received it.
#[derive(Clone, Debug, PartialEq)]
enum Wire {
    Start,
    /// A capture block, by its number.
    Audio(i16),
    End,
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
    /// The first connection is closed this long after the first end it
    /// receives, and that end is never answered: a link that drops.
    drop_after_first_end: Option<Duration>,
}

impl Service {
    fn answering_after(ms: u64) -> Self {
        Self {
            answer_after: Duration::from_millis(ms),
            packets: 12,
            words: "Here is an answer.",
            drop_after_first_end: None,
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
            seen.lock()
                .unwrap()
                .wire
                .push((connection, what.clone(), Instant::now()));
            match what {
                Wire::End => {
                    ends += 1;
                    match service.drop_after_first_end {
                        Some(after) if connection == 1 && ends == 1 => drop_at = Some(now + after),
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
            seen.lock().unwrap().answers.push((value, Instant::now()));
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

/// Frames on a fixed 10 ms schedule, like a capture device; each block's
/// samples are its own number.
fn capture_peer(
    mut channels: WorkerChannels,
    script: Script,
    seen: Arc<Mutex<Seen>>,
    stop: Arc<AtomicBool>,
) {
    let mut generation = 0;
    let mut started = None;
    let mut sequence = 0u64;
    while !stop.load(Ordering::SeqCst) {
        while let Some(control) = channels.control.receive::<Control>().unwrap() {
            match control {
                Control::Authority { snapshot } => generation = snapshot.microphone_generation(),
                Control::StartCapture if started.is_none() => {
                    let at = monotonic_us();
                    channels
                        .control
                        .send(WorkerEvent::CaptureStarted {
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
                        })
                        .unwrap();
                    let now = Instant::now();
                    started = Some(now);
                    seen.lock().unwrap().capture_started = Some(now);
                }
                Control::Stop => return,
                _ => {}
            }
        }
        while let Some(start) = started
            && start + Duration::from_millis(10 * sequence) <= Instant::now()
        {
            sequence += 1;
            let at = monotonic_us();
            let frame = MicrophoneFrame {
                epoch: 1,
                dsp_epoch: 1,
                privacy_generation: generation,
                frame_sequence: sequence,
                read_completed_at_us: at,
                probability: script(sequence),
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
            channels.data.send(frame).unwrap();
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn privacy_peer(mut channels: WorkerChannels, stop: Arc<AtomicBool>) {
    let mut generation = 0;
    let mut next = Instant::now();
    while !stop.load(Ordering::SeqCst) && drain_control(&mut channels, &mut generation) {
        if Instant::now() >= next {
            channels
                .control
                .send(WorkerEvent::Privacy {
                    acquired_at_us: monotonic_us(),
                    muted: false,
                })
                .unwrap();
            next += Duration::from_millis(10);
        }
        thread::sleep(Duration::from_millis(1));
    }
}

/// Takes every block at once, as ALSA would accept it.
fn speaker_peer(mut channels: WorkerChannels, seen: Arc<Mutex<Seen>>, stop: Arc<AtomicBool>) {
    let mut generation = 0;
    let mut frame = 1_000u64;
    while !stop.load(Ordering::SeqCst) && drain_control(&mut channels, &mut generation) {
        while let Some(chunk) = channels.data.receive::<SpeakerChunk>().unwrap() {
            let owner = chunk.permit.owner();
            seen.lock().unwrap().chunks.push((
                owner.turn(),
                Instant::now(),
                chunk.samples[0],
                monotonic_us(),
            ));
            let frames = chunk.samples.len();
            let at = monotonic_us();
            channels
                .control
                .send(WorkerEvent::Playback {
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
                })
                .unwrap();
            frame += frames as u64;
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn peripheral_peers(
    channels: [WorkerChannels; 3],
    script: Script,
    seen: Arc<Mutex<Seen>>,
    stop: Arc<AtomicBool>,
) -> [thread::JoinHandle<()>; 3] {
    let [capture, privacy, speaker] = channels;
    [
        thread::spawn({
            let (stop, seen) = (stop.clone(), seen.clone());
            move || capture_peer(capture, script, seen, stop)
        }),
        thread::spawn({
            let stop = stop.clone();
            move || privacy_peer(privacy, stop)
        }),
        thread::spawn(move || speaker_peer(speaker, seen, stop)),
    ]
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
        }
    }

    fn run(self) -> Session {
        self.run_with_gate(None)
    }

    fn run_with_privacy_tolerance(self) -> Session {
        self.run_with_gate(Some(
            PrivacyGate::with_fixture_freshness(FIXTURE_PRIVACY_FRESH_US).unwrap(),
        ))
    }

    fn run_with_gate(self, fixture_privacy: Option<PrivacyGate>) -> Session {
        let dir = SessionDirectory::create().unwrap();
        let (capture, capture_peer_channels) = Worker::sleeping_fixture(&dir.path, "capture");
        let (speaker, speaker_peer_channels) = Worker::sleeping_fixture(&dir.path, "speaker");
        let (provider, provider_peer_channels) = Worker::sleeping_fixture(&dir.path, "provider");
        let (privacy, privacy_peer_channels) = Worker::sleeping_fixture(&dir.path, "privacy");
        let mut owner = Controller::new(new_boot().unwrap(), now());
        let boot = owner.snapshot(now()).unwrap().boot();
        let stop = Arc::new(AtomicBool::new(false));
        let seen = Arc::new(Mutex::new(Seen::default()));
        let service = self.service;
        let script = self.script;
        let mut peripheral_channels = Some([
            capture_peer_channels,
            privacy_peer_channels,
            speaker_peer_channels,
        ]);
        let mut peers = Vec::with_capacity(4);
        if fixture_privacy.is_none() {
            // Preserve the original startup order for all ordinary scenarios.
            peers.extend(peripheral_peers(
                peripheral_channels.take().unwrap(),
                script,
                seen.clone(),
                stop.clone(),
            ));
        }
        peers.push(
            // The real provider worker and session, and the scripted service.
            thread::spawn({
                let (stop, seen) = (stop.clone(), seen.clone());
                move || {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .unwrap();
                    runtime.block_on(async move {
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
                        let answering =
                            tokio::spawn(serve_dials(dials, service, seen.clone(), stop.clone()));
                        while !stop.load(Ordering::SeqCst) {
                            tokio::time::sleep(Duration::from_millis(5)).await;
                        }
                        worker.abort();
                        answering.abort();
                        // A worker that ended by itself says why.
                        if let Ok(Err(error)) = worker.await {
                            seen.lock().unwrap().worker = Some(error.to_string());
                        }
                        let _ = answering.await;
                    });
                }
            }),
        );
        let mut rig = Rig {
            microphone: crate::options::Microphone::Mic2,
            end_silence_blocks: self.end_silence_blocks,
            early_end_blocks: self.early_ms.map(|ms| ms / 10),
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
            turn_taking: self.turn_taking,
            // As lamp-4ace.
            playout_start: PlayoutStart::new(350).unwrap(),
            endpoint: None,
        };
        // The worker says it is ready before anything else.
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match rig
                .provider
                .channels
                .control
                .receive::<WorkerEvent>()
                .unwrap()
            {
                Some(WorkerEvent::Ready) => break,
                Some(other) => panic!("before the worker was ready: {other:?}"),
                None => {
                    assert!(
                        Instant::now() < deadline,
                        "the provider worker never started"
                    );
                    thread::sleep(Duration::from_millis(1));
                }
            }
        }
        if let Some(channels) = peripheral_channels {
            // This explicit mode starts sample producers only after the real
            // provider's Ready barrier. Startup cannot queue old privacy reads
            // while the runtime is still initializing; no sample is re-stamped.
            peers.extend(peripheral_peers(
                channels,
                script,
                seen.clone(),
                stop.clone(),
            ));
        }
        let mut trace = SessionState::finite();
        let mut reply = None;
        let result = if let Some(gate) = fixture_privacy {
            conversation_with_fixture_privacy(
                &mut rig,
                &mut owner,
                Lifetime::Finite(self.seconds),
                &mut trace,
                StartupEvents::default(),
                &mut reply,
                &mut None,
                &Termination::inactive(),
                &FailureNotices::none(),
                gate,
                Instant::now() + FIXTURE_CONVERSATION_WATCHDOG,
            )
        } else {
            conversation(
                &mut rig,
                &mut owner,
                Lifetime::Finite(self.seconds),
                &mut trace,
                StartupEvents::default(),
                &mut reply,
                &mut None,
                &Termination::inactive(),
                &FailureNotices::none(),
            )
        };
        let _ = rig.shutdown(&mut trace);
        stop.store(true, Ordering::SeqCst);
        for peer in peers {
            peer.join().unwrap();
        }
        let seen = std::mem::take(&mut *seen.lock().unwrap());
        Session {
            result,
            trace: trace.to_vec(),
            seen,
        }
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

/// Quiet to the end. The end goes 200 ms after the last sound, the answer
/// is in hand 150 ms later, and its first sound is handed to the speaker
/// at the moment the 600 ms wait ends, not before. One request, none given
/// up.
#[test]
fn quiet_to_the_end_the_answer_starts_when_the_lamp_is_sure_and_not_before() {
    let _one = one_session();
    let session = Setup::lamp(Some(200), 4, one_question, Service::answering_after(150)).run();
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
    let session = setup.run();
    session.ok();
    session.summary(name, 290);
    let seen = &session.seen;
    // Given up once, at the sixth block of speech again (196), with what
    // was heard since its end (181 to 196) the successor's opening.
    assert_eq!(session.given_up(), 1);
    let given_up = &session.events("early_end_given_up")[0];
    assert_eq!(given_up["opening_blocks"], 16);
    let mut expected = request(76, 180);
    expected.extend(request(181, 290 + 20));
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
        },
    );
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
    .run();
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
    .run();
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
    let on = Setup::lamp(Some(200), 5, one_question, Service::answering_after(900)).run();
    let off = Setup::lamp(None, 5, one_question, Service::answering_after(900)).run();
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
            drop_after_first_end: Some(Duration::from_millis(100)),
            ..Service::answering_after(150)
        },
    )
    .run();
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
    .run();
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
    assert_eq!(heard[heard.len() - 2], Wire::Audio(380 + 20));
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
    let session = Setup::lamp(None, 6, name_then_question, Service::answering_after(150)).run();
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
            provider_arguments(ProviderSource::Gemini(config), &on, lifetime, None).unwrap(),
            provider_arguments(ProviderSource::Gemini(config), &options, lifetime, None).unwrap()
        );
        let mut with = run_start_record(&on, lifetime, false, false, None, false);
        let without = run_start_record(&options, lifetime, false, false, None, false);
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
/// with no owner after it (`ProviderIntake::synchronize`).
#[tokio::test]
async fn a_name_note_told_with_a_request_ended_early_does_not_survive_it() {
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
