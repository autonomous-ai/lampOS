//! Real owner admission, Skills socket authorization and speaker IPC. The
//! accepted PCM/retirement receipts here are authored host evidence, not sound
//! measured at a device. No test changes the machine's mixer or speakers.
use super::*;
use crate::{
    admission::{Candidate, CandidateId},
    requested_audio::Service,
    wire::PlaybackGap,
};
use lamp_audio::ownership::PlaybackCursor;
use lamp_bus::{Bus, Publisher};
use lamp_msgs::{
    ClockStamp, REQUESTED_AUDIO_LEASE_US, REQUESTED_AUDIO_PLAY_US, REQUESTED_AUDIO_START_US,
    RequestedAudioContext, RequestedAudioLease, RequestedAudioOffer, RequestedAudioOperation,
    RequestedAudioOutcome, RequestedAudioPhase, SpeakerState, Voice, VoiceState,
};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
};

const WAIT: Duration = Duration::from_secs(3);
const CLOCK: &str = "requested-recording-fixture";

fn fixture_clock() -> crate::scenario_clock::Scope {
    let scope = crate::scenario_clock::Scope::enter(Duration::from_secs(30)).unwrap();
    scope.set_pump(|_| Ok(())).unwrap();
    scope
}

/// One bounded fake daemon, on the real private call.sock protocol. A held
/// Start is released by an explicit test message, never a timing sleep.
struct Skills {
    socket: PathBuf,
    operations: Receiver<RequestedAudioOperation>,
    start_gate: Option<SyncSender<()>>,
    lease_until: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Skills {
    fn new(directory: &Path, lease_us: u64, hold_start: bool, clock: Arc<AtomicU64>) -> Self {
        let socket = directory.join(crate::tools::CALL_SOCKET);
        let listener = UnixListener::bind(&socket).unwrap();
        let (send, operations) = mpsc::sync_channel(32);
        let (release, gate) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let lease_until = Arc::new(AtomicU64::new(0));
        let worker = {
            let (stop, lease_until) = (stop.clone(), lease_until.clone());
            thread::spawn(move || {
                let mut held = hold_start;
                let mut active: Option<RequestedAudioOffer> = None;
                for stream in listener.incoming() {
                    let mut stream = stream.unwrap();
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    stream.set_read_timeout(Some(WAIT)).unwrap();
                    stream.set_write_timeout(Some(WAIT)).unwrap();
                    let mut line = String::new();
                    BufReader::new((&stream).take(16_384))
                        .read_line(&mut line)
                        .unwrap();
                    let operation: RequestedAudioOperation = serde_json::from_str(&line).unwrap();
                    operation.check().unwrap();
                    send.send(operation.clone()).unwrap();
                    let response = match operation {
                        RequestedAudioOperation::AudioAuthorize { offer, phase } => {
                            match phase {
                                RequestedAudioPhase::Start => {
                                    assert!(active.replace(offer.clone()).is_none());
                                    if held {
                                        gate.recv_timeout(WAIT).unwrap();
                                        held = false;
                                    }
                                }
                                RequestedAudioPhase::Renew => {
                                    assert_eq!(active.as_ref(), Some(&offer));
                                }
                            }
                            let until_us = (clock.load(Ordering::Acquire) + lease_us)
                                .min(offer.finish_before_us);
                            lease_until.store(until_us, Ordering::Release);
                            json!({"ok":true,"result":RequestedAudioLease {
                                skills_boot: offer.skills_boot,
                                definition_generation: offer.definition_generation,
                                offer_id: offer.offer_id,
                                until_us,
                            }})
                        }
                        RequestedAudioOperation::AudioFinish { offer, .. } => {
                            assert_eq!(active.take(), Some(offer));
                            json!({"ok":true,"result":{"finished":true}})
                        }
                    };
                    // A cancelled client may have already closed its bounded
                    // exchange. The daemon still records the exact operation.
                    let _ = writeln!(stream, "{response}");
                }
            })
        };
        Self {
            socket,
            operations,
            start_gate: hold_start.then_some(release),
            lease_until,
            stop,
            worker: Some(worker),
        }
    }

    fn started(&self, expected: &RequestedAudioOffer) {
        assert_eq!(
            self.operations.recv_timeout(WAIT).unwrap(),
            RequestedAudioOperation::AudioAuthorize {
                offer: expected.clone(),
                phase: RequestedAudioPhase::Start,
            }
        );
    }

    fn release_start(&mut self) {
        self.start_gate.take().unwrap().send(()).unwrap();
    }

    fn finished(&self, expected: &RequestedAudioOffer, outcome: RequestedAudioOutcome) {
        let until = Instant::now() + WAIT;
        loop {
            match self
                .operations
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .unwrap()
            {
                RequestedAudioOperation::AudioAuthorize { offer, phase } => {
                    assert_eq!(offer, *expected);
                    assert_eq!(phase, RequestedAudioPhase::Renew);
                }
                RequestedAudioOperation::AudioFinish {
                    offer,
                    outcome: actual,
                } => {
                    assert_eq!(offer, *expected);
                    assert_eq!(actual, outcome);
                    break;
                }
            }
        }
    }

    fn no_finish(&self) {
        for operation in self.operations.try_iter() {
            assert!(matches!(
                operation,
                RequestedAudioOperation::AudioAuthorize {
                    phase: RequestedAudioPhase::Renew,
                    ..
                }
            ));
        }
    }
}

impl Drop for Skills {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(release) = self.start_gate.take() {
            let _ = release.try_send(());
        }
        let _ = UnixStream::connect(&self.socket);
        if let Some(worker) = self.worker.take() {
            let result = worker.join();
            if !thread::panicking() {
                result.unwrap();
            }
        }
    }
}

struct Speaker {
    channels: WorkerChannels,
}

impl SpeakerControlIo for Speaker {
    fn next_speaker_event(&mut self) -> io::Result<Option<WorkerEvent>> {
        self.channels.control.receive()
    }

    fn publish_speaker_snapshot(&mut self, snapshot: Snapshot) -> io::Result<()> {
        self.channels.control.send(Control::Authority { snapshot })
    }
}

#[derive(Clone, Copy, Debug)]
enum Route {
    Spoken,
    Written,
    Reminder,
    Say,
    Offline,
}

struct Test {
    // Drop the ticket and service before joining the fake daemon, and keep its
    // files alive until the channels and publisher have closed.
    reply: Option<Reply>,
    service: Service,
    skills: Skills,
    publisher: Publisher,
    speaker: Speaker,
    peer: WorkerChannels,
    provider: WorkerChannels,
    provider_peer: WorkerChannels,
    worker: BootId,
    owner: Controller,
    pending: Option<PendingSpeaker>,
    snapshot: Snapshot,
    mask: PlaybackMask,
    trace: SessionState,
    heard_news: Vec<String>,
    cloud: CloudAvailability,
    sequence: u64,
    cursor: u64,
    asset: PathBuf,
    sha256: String,
    _directory: SessionDirectory,
}

impl Test {
    fn new(route: Route, lease_us: u64, hold_start: bool) -> Self {
        Self::in_directory(
            route,
            lease_us,
            hold_start,
            SessionDirectory::create().unwrap(),
        )
    }

    fn in_directory(
        route: Route,
        lease_us: u64,
        hold_start: bool,
        directory: SessionDirectory,
    ) -> Self {
        assert!(crate::scenario_clock::is_active());
        let clock = Arc::new(AtomicU64::new(monotonic_us()));
        let skills = Skills::new(&directory.path, lease_us, hold_start, clock.clone());
        let bus = Bus::new(directory.path.join("bus")).unwrap();
        let mut publisher = bus
            .publisher_declared(SpeakerState::TOPIC, &SpeakerState::declaration(), "actions")
            .unwrap();
        publish_state(&mut publisher, 0, false);
        let service =
            Service::new_stepped(bus, directory.path.clone(), CLOCK.into(), clock).unwrap();
        assert!(service.wait_cached(WAIT));
        let asset = directory.path.join("recording.wav");
        let mut wav = hound::WavWriter::create(
            &asset,
            hound::WavSpec {
                channels: 1,
                sample_rate: 24_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..480 {
            wav.write_sample(100i16).unwrap();
        }
        wav.finalize().unwrap();
        let checked = crate::requested_audio::read_sound(&asset).unwrap();
        let (channels, peer) = pair(&directory, "speaker");
        let (provider, provider_peer) = pair(&directory, "provider");
        let worker = new_boot().unwrap();
        let mut owner = Controller::new(new_boot().unwrap(), now());
        owner
            .set_microphone_permission(now(), Permission::Allowed)
            .unwrap();
        let snapshot = owner.snapshot(now()).unwrap();
        let mut test = Self {
            reply: None,
            service,
            skills,
            publisher,
            speaker: Speaker { channels },
            peer,
            provider,
            provider_peer,
            worker,
            owner,
            pending: None,
            snapshot,
            mask: PlaybackMask::new(TurnTaking::VadOnly),
            trace: SessionState::default(),
            heard_news: Vec::new(),
            cloud: CloudAvailability::default(),
            sequence: 0,
            cursor: 0,
            asset,
            sha256: checked.sha256,
            _directory: directory,
        };
        test.cloud
            .observe(ready_link(1, 1), monotonic_us())
            .unwrap();
        test.begin(route);
        test
    }

    fn step(&self) {
        assert!(self.service.step_fixture(WAIT, || {
            assert!(crate::scenario_clock::step(crate::scenario_clock::Phase::Run).unwrap());
            monotonic_us()
        }));
    }

    fn renew_input(&mut self) {
        self.step();
        let until = MonoTime::from_micros(monotonic_us() + 1_000_000);
        self.owner
            .set_capture(now(), CaptureState::RetainingUntil(until))
            .unwrap();
        self.owner
            .set_admission(now(), AdmissionState::OpenUntil(until))
            .unwrap();
    }

    fn accepted(&self) -> Accepted {
        let at = monotonic_us();
        Accepted {
            candidate: Candidate {
                id: CandidateId::for_test(self.sequence + 1),
                capture: CaptureLineage {
                    worker: self.worker,
                    epoch: 1,
                    dsp_epoch: 1,
                    privacy_generation: self.snapshot.microphone_generation(),
                },
                displaced_owner: self.reply.as_ref().map(|reply| reply.owner),
                trigger_sequence: 1,
                trigger_read_at_us: at,
                first_read_at_us: at,
                decision_deadline_us: at + 100_000,
            },
            basis: AcceptanceBasis::DirectedSessionVadOnly,
            audio: vec![ObservedAudio {
                sequence: 1,
                captured_at_us: at,
                samples: [100; lamp_audio::CAPTURE_SAMPLES],
            }],
            ended: true,
        }
    }

    fn begin(&mut self, route: Route) {
        self.renew_input();
        self.snapshot = self.owner.snapshot(now()).unwrap();
        let accepted = self.accepted();
        let mut queue = VecDeque::new();
        self.snapshot = match route {
            Route::Spoken => commit_admitted_input(
                &mut self.owner,
                &mut self.reply,
                &mut self.pending,
                &mut queue,
                accepted,
                1,
                false,
                false,
                false,
                None,
                None,
                false,
                false,
                false,
                PlayoutStart::default(),
                &mut self.trace,
                |state| self.speaker.publish_speaker_snapshot(state),
            ),
            Route::Offline => commit_offline_input(
                &mut self.owner,
                &mut self.reply,
                &mut self.pending,
                &mut queue,
                accepted,
                PlayoutStart::default(),
                false,
                &mut self.trace,
                |state| self.speaker.publish_speaker_snapshot(state),
            ),
            Route::Written | Route::Reminder | Route::Say => commit_written_turn(
                &mut self.owner,
                &mut self.reply,
                &mut queue,
                "A written request to play a recording.",
                false,
                "requested_audio_fixture",
                1,
                PlayoutStart::default(),
                &mut self.trace,
                |state| self.speaker.publish_speaker_snapshot(state),
            ),
        }
        .unwrap();
        let current = self.reply.as_mut().unwrap();
        match route {
            Route::Reminder => {
                current.reminder = Some(
                    crate::reminders::receipt_for_test(
                        current.owner,
                        self.snapshot.microphone_generation(),
                        1,
                    )
                    .0,
                );
            }
            Route::Say => current.say = Some(crate::say::ticket_for_test("recording").0),
            _ => {}
        }
        let request = current.owner.turn();
        if matches!(route, Route::Offline) {
            assert!(queue.is_empty());
            return;
        }
        let spoken = matches!(route, Route::Spoken);
        assert_eq!(
            matches!(queue.front(), Some(ProviderInput::Start { .. })),
            spoken
        );
        let count = queue.len();
        flush_provider_input(
            &self.cloud,
            &mut self.reply,
            &mut queue,
            &mut self.provider.data,
        )
        .unwrap();
        assert!(queue.is_empty());
        for _ in 0..count {
            assert!(
                self.provider_peer
                    .data
                    .receive::<ProviderInput>()
                    .unwrap()
                    .is_some()
            );
        }
        observe_provider_output(
            &mut self.cloud,
            &mut self.reply,
            &ProviderOutput::Started {
                request,
                session: 1,
                waiting_for_barrier: false,
            },
        )
        .unwrap();
    }

    fn offer(&self, id: u64) -> RequestedAudioOffer {
        let issued_us = monotonic_us();
        RequestedAudioOffer {
            skills_boot: "skills-fixture".into(),
            definition_generation: 1,
            offer_id: id,
            context: RequestedAudioContext {
                worker_boot: boot_text(self.worker),
                provider_session: 1,
                request: self.reply.as_ref().unwrap().owner.turn(),
                privacy_generation: self.snapshot.microphone_generation(),
                call_id: format!("recording-{id}"),
                issued_us,
            },
            skill: "recording".into(),
            asset: self.asset.to_str().unwrap().into(),
            sha256: self.sha256.clone(),
            frames: 480,
            issued_us,
            start_before_us: issued_us + REQUESTED_AUDIO_START_US,
            finish_before_us: issued_us + REQUESTED_AUDIO_START_US + REQUESTED_AUDIO_PLAY_US,
        }
    }

    fn accept_offer(&mut self, offer: RequestedAudioOffer) -> bool {
        self.reply.as_mut().unwrap().accept_requested_audio(
            &self.service,
            self.worker,
            self.snapshot,
            offer,
        )
    }

    fn load(&mut self, id: u64) -> RequestedAudioOffer {
        let offer = self.offer(id);
        assert!(self.accept_offer(offer.clone()));
        self.skills.started(&offer);
        assert!(
            self.reply
                .as_ref()
                .unwrap()
                .wait_requested_audio_ready(WAIT)
        );
        offer
    }

    fn event(&mut self, event: WorkerEvent) {
        self.peer.control.send(event).unwrap();
        service_speaker_control(
            &mut self.speaker,
            &mut self.owner,
            &mut self.reply,
            &mut self.pending,
            &mut self.mask,
            &mut self.snapshot,
            &mut 0,
            &mut 16,
            &mut self.trace,
            &mut None,
        )
        .unwrap();
        // Keep the fake worker's authority channel bounded during repetitions.
        while self.peer.control.receive::<Control>().unwrap().is_some() {}
    }

    fn dispatch(&mut self) -> Option<SpeakerChunk> {
        if self.reply.is_some() {
            self.renew_input();
        }
        dispatch_speaker_chunk(
            &mut self.owner,
            &mut self.reply,
            &mut self.speaker.channels.data,
            &mut self.pending,
            &mut self.sequence,
            &mut self.snapshot,
            &mut self.mask,
        )
        .unwrap();
        let chunk = self.peer.data.receive::<SpeakerChunk>().unwrap();
        if let Some(chunk) = &chunk {
            self.trace
                .receipts
                .dispatched(chunk.permit.owner(), chunk.chunk_sequence)
                .unwrap();
        }
        chunk
    }

    fn generation(&mut self) -> SpeakerChunk {
        let request = self.reply.as_ref().unwrap().owner.turn();
        let at = monotonic_us();
        let samples = vec![200; 240];
        observe_provider_output(
            &mut self.cloud,
            &mut self.reply,
            &ProviderOutput::Audio {
                request,
                sequence: self.sequence + 1,
                provider_event_at_us: at,
                source_frames: samples.len(),
                source_offset: 0,
                samples: samples.clone(),
            },
        )
        .unwrap();
        let current = self.reply.as_mut().unwrap();
        current.append_provider_audio(samples, at);
        current.had_audio = true;
        observe_provider_output(
            &mut self.cloud,
            &mut self.reply,
            &ProviderOutput::GenerationComplete { request },
        )
        .unwrap();
        self.dispatch().unwrap()
    }

    fn provider_complete(&mut self) {
        let request = self.reply.as_ref().unwrap().owner.turn();
        observe_provider_output(
            &mut self.cloud,
            &mut self.reply,
            &ProviderOutput::TurnComplete {
                request,
                idle: true,
            },
        )
        .unwrap();
    }

    fn accept(&mut self, chunk: &SpeakerChunk, frames: usize) -> u64 {
        let at = monotonic_us();
        self.event(WorkerEvent::Playback {
            owner: chunk.permit.owner(),
            turn: chunk.permit.owner().turn(),
            sequence: chunk.chunk_sequence,
            first_sample: PlaybackCursor {
                epoch: 1,
                frame: self.cursor,
            },
            end_sample: PlaybackCursor {
                epoch: 1,
                frame: self.cursor + frames as u64,
            },
            queue_observed_at_us: at,
            queued_frames: 0,
            accepted_frames: frames,
            observed_at_us: at,
        });
        self.cursor += frames as u64;
        at
    }

    fn retire(&mut self, chunk: &SpeakerChunk) -> u64 {
        let at = monotonic_us();
        self.event(WorkerEvent::SpeechRetired {
            owner: chunk.permit.owner(),
            sequence: chunk.chunk_sequence,
            final_sample: PlaybackCursor {
                epoch: 1,
                frame: self.cursor,
            },
            observed_at_us: at,
        });
        at
    }

    fn prepare(&mut self) -> bool {
        self.reply
            .as_mut()
            .unwrap()
            .prepare_requested_audio(&self.pending, monotonic_us())
    }

    fn confirmed(&mut self) -> u64 {
        let chunk = self.generation();
        self.provider_complete();
        self.accept(&chunk, 240);
        self.retire(&chunk)
    }

    fn complete_media(&mut self) {
        self.reply
            .as_mut()
            .unwrap()
            .complete_requested_audio(monotonic_us());
    }

    fn answer_news(&mut self) {
        let request = self.reply.as_ref().unwrap().owner.turn();
        observe_provider_output(
            &mut self.cloud,
            &mut self.reply,
            &ProviderOutput::NewsAnswered {
                request,
                key: "tasks-fixture:2:4".into(),
            },
        )
        .unwrap();
    }

    /// The production loop's completion order, including its news handoff.
    fn finish_if_settled(&mut self) -> bool {
        let Some(current) = self.reply.as_mut() else {
            return false;
        };
        if !current.settled(&self.pending) {
            return false;
        }
        self.owner.complete_turn(now(), current.owner).unwrap();
        current.complete_requested_audio(monotonic_us());
        self.snapshot = self.owner.snapshot(now()).unwrap();
        self.speaker
            .publish_speaker_snapshot(self.snapshot)
            .unwrap();
        self.heard_news
            .extend(current.heard_news(&self.pending, self.snapshot));
        self.reply = None;
        true
    }

    fn cancel(&mut self, reason: &str) {
        if reason == "new_owner" {
            self.begin(Route::Spoken);
            return;
        }
        cancel_reply(&mut self.owner, &mut self.reply, &mut self.trace, reason).unwrap();
        self.pending = None;
        match reason {
            "privacy" => self
                .owner
                .set_microphone_permission(now(), Permission::Denied)
                .unwrap(),
            "capture_gap" => self
                .owner
                .set_capture(now(), CaptureState::Unavailable)
                .unwrap(),
            "hand_stop" => {}
            _ => panic!("unknown fixture cancellation"),
        }
        self.snapshot = self.owner.snapshot(now()).unwrap();
    }
}

fn publish_state(publisher: &mut Publisher, generation: u64, muted: bool) {
    let state = SpeakerState {
        stamp: ClockStamp {
            clock_id: CLOCK.into(),
            at_us: monotonic_us(),
            sequence: publisher.totals().published + 1,
        },
        session_id: publisher.boot().into(),
        mute_generation: generation,
        volume_raw: Some(40),
        muted,
    };
    publisher.publish_json(state.stamp.at_us, &state).unwrap();
}

#[test]
fn requested_audio_keeps_voice_state_non_resting_until_final_retirement() {
    let _clock = fixture_clock();
    let mut test = Test::new(Route::Spoken, REQUESTED_AUDIO_LEASE_US, false);
    let bus = Bus::new(test._directory.path.join("bus")).unwrap();
    let mut out = crate::bus_out::BusOut::on(&bus).unwrap();
    let mut state = |test: &Test, stage: &str, expected: Voice| {
        tell_voice_of_turn(
            &mut out,
            test.snapshot,
            test.reply.as_ref().map(|current| current.notice.origin()),
            monotonic_us(),
        );
        let message = bus.latest(VoiceState::TOPIC).unwrap().unwrap();
        assert_eq!(
            message.parse::<VoiceState>().unwrap().state,
            expected,
            "{stage}"
        );
    };

    let offer = test.load(1);
    let original_owner = test.reply.as_ref().unwrap().owner;
    test.confirmed();
    assert!(test.prepare());
    assert_eq!(
        test.reply.as_ref().unwrap().notice.origin(),
        SpeechOrigin::RequestedAudio
    );
    // MusicSway and LaughGlance require Resting. The recording keeps its
    // spoken turn even before the speaker accepts its first sample.
    state(&test, "recording prepared", Voice::Thinking);
    let first = test.dispatch().unwrap();
    assert_eq!(first.permit.owner(), original_owner);
    assert!(!first.end_of_speech);
    state(&test, "first chunk outstanding", Voice::Thinking);
    test.accept(&first, 120);
    state(&test, "first chunk partly accepted", Voice::Speaking);
    test.accept(&first, 120);
    state(&test, "first chunk accepted", Voice::Speaking);

    let last = test.dispatch().unwrap();
    assert_eq!(last.permit.owner(), original_owner);
    assert!(last.end_of_speech);
    state(&test, "last chunk outstanding", Voice::Speaking);
    test.accept(&last, 240);
    assert!(!test.finish_if_settled());
    state(&test, "last chunk accepted but unretired", Voice::Speaking);

    test.retire(&last);
    assert_eq!(test.snapshot.owner(), Some(original_owner));
    state(&test, "retired but turn not completed", Voice::Thinking);
    assert!(test.finish_if_settled());
    assert!(test.snapshot.owner().is_none());
    state(&test, "retired and turn completed", Voice::Resting);
    test.skills
        .finished(&offer, RequestedAudioOutcome::Completed);
}

#[test]
fn prepared_recordings_wait_for_retirement_keep_the_spoken_owner_and_finish_after_retirement() {
    let _clock = fixture_clock();
    let mut test = Test::new(Route::Spoken, REQUESTED_AUDIO_LEASE_US, false);
    let mut handoffs = Vec::new();
    let mut host_handoffs = Vec::new();
    for id in 1..=2 {
        if id > 1 {
            assert!(test.service.wait_idle(WAIT));
            test.begin(Route::Spoken);
        }
        let offer = test.load(id);
        let original_owner = test.reply.as_ref().unwrap().owner;
        let confirmation = test.generation();
        assert!(confirmation.end_of_speech);
        assert!(!test.prepare(), "GenerationComplete is not retirement");
        test.provider_complete();
        assert!(!test.prepare(), "provider completion is not retirement");
        assert!(test.dispatch().is_none(), "one chunk remains outstanding");
        test.accept(&confirmation, 120);
        assert!(!test.prepare());
        test.accept(&confirmation, 120);
        assert!(!test.prepare(), "accepted final PCM has not retired");
        assert!(!test.reply.as_ref().unwrap().settled(&test.pending));
        let host_retired = std::time::Instant::now();
        let retired = test.retire(&confirmation);
        assert!(test.prepare());
        let first = test.dispatch().unwrap();
        assert_eq!(first.permit.owner(), original_owner);
        assert_eq!(first.samples, [100; 240]);
        assert!(!first.end_of_speech);
        assert!(test.dispatch().is_none());
        let accepted = test.accept(&first, 240);
        handoffs.push(accepted - retired);
        assert!(accepted - retired <= 250_000);
        // Keep the host latency gate as well as the logical receipt/trace
        // assertion. This wider boundary includes both IPC event handlers.
        let host_handoff = host_retired.elapsed();
        assert!(host_handoff <= Duration::from_millis(250));
        host_handoffs.push(host_handoff.as_micros());
        let final_chunk = test.dispatch().unwrap();
        assert_eq!(final_chunk.permit.owner(), original_owner);
        assert!(final_chunk.end_of_speech);
        assert_eq!(final_chunk.samples, [100; 240]);
        test.complete_media();
        test.skills.no_finish();
        test.accept(&final_chunk, 240);
        assert!(
            test.dispatch().is_none(),
            "the final chunk cannot be replayed"
        );
        assert!(!test.reply.as_ref().unwrap().settled(&test.pending));
        test.complete_media();
        test.skills.no_finish();
        test.retire(&final_chunk);
        test.complete_media();
        assert!(test.reply.as_ref().unwrap().settled(&test.pending));
        test.skills
            .finished(&offer, RequestedAudioOutcome::Completed);
        test.owner.complete_turn(now(), original_owner).unwrap();
        test.reply = None;
    }
    let traced: Vec<_> = test
        .trace
        .iter()
        .filter(|event| event["kind"] == "requested_audio_handoff")
        .map(|event| event["confirmation_to_first_write_us"].as_u64().unwrap())
        .collect();
    assert_eq!(traced, handoffs);
    eprintln!(
        "requested audio prepared host handoff: first={}us repeated={}us; target<=250000us; authored speaker IPC acceptance, acoustic onset unmeasured",
        host_handoffs[0], host_handoffs[1]
    );
}

#[test]
fn news_in_a_music_confirmation_is_heard_once_after_both_parts_retire() {
    let _clock = fixture_clock();
    let mut test = Test::new(Route::Spoken, REQUESTED_AUDIO_LEASE_US, false);
    let offer = test.load(1);
    test.answer_news();
    let confirmation = test.generation();
    test.provider_complete();
    test.accept(&confirmation, 240);
    assert!(
        !test.finish_if_settled(),
        "confirmation acceptance is not retirement"
    );
    test.retire(&confirmation);
    assert!(!test.finish_if_settled(), "the recording is still owed");
    assert!(test.prepare());
    let first = test.dispatch().unwrap();
    test.accept(&first, 240);
    let last = test.dispatch().unwrap();
    test.accept(&last, 240);
    assert!(
        !test.finish_if_settled(),
        "recording acceptance is not retirement"
    );
    assert!(test.heard_news.is_empty());
    test.skills.no_finish();
    test.retire(&last);
    assert!(test.finish_if_settled());
    assert_eq!(test.heard_news, ["tasks-fixture:2:4"]);
    test.skills
        .finished(&offer, RequestedAudioOutcome::Completed);
    assert!(
        !test.finish_if_settled(),
        "a completed turn cannot hand news on again"
    );
    assert_eq!(test.heard_news, ["tasks-fixture:2:4"]);
}

#[test]
fn cancelled_gapped_or_expired_music_never_acknowledges_its_confirmation_news() {
    let _clock = fixture_clock();
    for reason in ["hand_stop", "new_owner", "privacy", "gap", "expiry"] {
        let mut test = Test::new(Route::Spoken, REQUESTED_AUDIO_LEASE_US, false);
        let offer = test.load(1);
        test.answer_news();
        test.confirmed();
        assert!(test.prepare());
        let first = test.dispatch().unwrap();
        test.accept(&first, 240);
        if reason == "gap" {
            let at = monotonic_us();
            test.event(WorkerEvent::PlaybackGap {
                phase: PlaybackGapPhase::Started,
                gap: PlaybackGap {
                    owner: first.permit.owner(),
                    first_sample: PlaybackCursor {
                        epoch: 1,
                        frame: test.cursor,
                    },
                    end_sample: PlaybackCursor {
                        epoch: 1,
                        frame: test.cursor + 240,
                    },
                    started_at_us: at,
                    last_zero_accepted_at_us: at,
                    zero_frames: 240,
                },
                observed_at_us: at,
            });
            // Accepted zeros occupy the physical cursor between the two
            // recording chunks; they are a local gap, not a provider gap.
            test.cursor += 240;
            let current = test.reply.as_ref().unwrap();
            assert_eq!(current.playback_gaps, 0);
            assert_eq!(current.notice.evidence()["playback_gaps"], 1);
        }
        let last = test.dispatch().unwrap();
        test.accept(&last, 240);
        test.retire(&last);
        match reason {
            "expiry" => {
                // Exercise the actual ticket deadline without waiting out a
                // recording's duration. Expiry is terminal even with receipts.
                assert!(test.reply.as_ref().unwrap().requested_audio_invalid(
                    test.worker,
                    test.snapshot,
                    offer.finish_before_us,
                ));
                assert!(test.finish_if_settled());
            }
            "gap" => assert!(test.finish_if_settled()),
            _ => {
                // A final speaker receipt cannot turn cancellation into
                // successful completion, or lend news to the next owner.
                test.cancel(reason);
                assert!(!test.finish_if_settled());
            }
        }
        assert!(test.heard_news.is_empty(), "{reason}");
        test.skills
            .finished(&offer, RequestedAudioOutcome::Cancelled);
    }
}

#[test]
fn offers_require_the_current_worker_session_request_and_privacy_identity() {
    let _clock = fixture_clock();
    let mut test = Test::new(Route::Spoken, REQUESTED_AUDIO_LEASE_US, false);
    let good = test.offer(1);
    for field in [
        "worker", "session", "request", "privacy", "digest", "future",
    ] {
        let mut bad = good.clone();
        match field {
            "worker" => bad.context.worker_boot = "another-worker".into(),
            "session" => bad.context.provider_session += 1,
            "request" => bad.context.request += 1,
            "privacy" => bad.context.privacy_generation += 1,
            "digest" => bad.sha256 = "invalid".into(),
            "future" => {
                bad.issued_us += REQUESTED_AUDIO_START_US;
                bad.context.issued_us = bad.issued_us;
                bad.start_before_us += REQUESTED_AUDIO_START_US;
                bad.finish_before_us += REQUESTED_AUDIO_START_US;
            }
            _ => unreachable!(),
        }
        assert!(!test.accept_offer(bad), "{field}");
        assert!(test.skills.operations.try_recv().is_err());
    }
    // A failed attempt at preparation is consumed even if a timestamp later
    // becomes plausible. Identity failures before preparation consume nothing.
    assert!(!test.accept_offer(good));
    assert!(test.skills.operations.try_recv().is_err());

    let mut test = Test::new(Route::Spoken, REQUESTED_AUDIO_LEASE_US, false);
    let original = test.offer(1);
    test.begin(Route::Spoken);
    assert!(!test.accept_offer(original));
    let current = test.offer(2);
    let mut foreign = Controller::new(new_boot().unwrap(), now());
    let foreign_snapshot = foreign.snapshot(now()).unwrap();
    assert!(!test.reply.as_mut().unwrap().accept_requested_audio(
        &test.service,
        test.worker,
        foreign_snapshot,
        current.clone(),
    ));
    assert!(test.accept_offer(current.clone()));
    test.skills.started(&current);
    assert!(
        test.reply
            .as_ref()
            .unwrap()
            .wait_requested_audio_ready(WAIT)
    );
    assert!(!test.accept_offer(current.clone()), "one offer per owner");
    test.cancel("hand_stop");
    test.skills
        .finished(&current, RequestedAudioOutcome::Cancelled);
}

/// #2793: on lamp-4ace a first "play me something to relax" was refused
/// with nothing to say why. A refused offer now names the first condition
/// that refused it, and one that would be taken names none.
#[test]
fn a_refused_offer_names_the_condition_that_refused_it() {
    let _clock = fixture_clock();
    let mut test = Test::new(Route::Spoken, REQUESTED_AUDIO_LEASE_US, false);
    let good = test.offer(1);
    let why = |test: &Test, offer: &RequestedAudioOffer| {
        test.reply
            .as_ref()
            .unwrap()
            .requested_audio_refusal(test.worker, test.snapshot, offer)
    };
    for (field, said) in [
        ("worker", "the offer is from another worker boot"),
        ("session", "the offer is for another provider session"),
        ("request", "the offer is for another request"),
        ("privacy", "the offer is from another privacy generation"),
        ("digest", "the offer does not check"),
    ] {
        let mut bad = good.clone();
        match field {
            "worker" => bad.context.worker_boot = "another-worker".into(),
            "session" => bad.context.provider_session += 1,
            "request" => bad.context.request += 1,
            "privacy" => bad.context.privacy_generation += 1,
            "digest" => bad.sha256 = "invalid".into(),
            _ => unreachable!(),
        }
        assert_eq!(why(&test, &bad), Some(said), "{field}");
    }
    let mut foreign = Controller::new(new_boot().unwrap(), now());
    let foreign_snapshot = foreign.snapshot(now()).unwrap();
    assert_eq!(
        test.reply
            .as_ref()
            .unwrap()
            .requested_audio_refusal(test.worker, foreign_snapshot, &good),
        Some("the speaker belongs to another owner")
    );
    assert_eq!(why(&test, &good), None);
    assert!(test.accept_offer(good.clone()));
    assert_eq!(
        why(&test, &good),
        Some("this turn already had its one offer")
    );
    let written = Test::new(Route::Written, REQUESTED_AUDIO_LEASE_US, false);
    let offer = written.offer(1);
    assert_eq!(why(&written, &offer), Some("not a spoken turn"));
}

// Exercise the same admission-and-record handler as ProviderOutput::RequestedAudio,
// with the fixture's explicit Service instead of process-global environment state.
fn private_boundary_offer(test: &Test) -> RequestedAudioOffer {
    let mut offer = test.offer(1);
    offer.skills_boot = "private-skills-boot-marker".into();
    offer.skill = "private-recording-marker".into();
    offer.context.call_id = "private-call-marker".into();
    offer.check().unwrap();
    offer
}

fn assert_offer_boundary(
    trace: &SessionState,
    from: usize,
    (request, session): (u64, u64),
    refused: Option<&str>,
    at_us: u64,
    offer: &RequestedAudioOffer,
) {
    let rows = &trace[from..];
    assert_eq!(rows.len(), 1, "one boundary record per handled offer");
    let row = &rows[0];
    // Equality fixes the complete key set, as well as the safe reason and
    // queue-only boundary. No offer payload or nested context may be added.
    assert_eq!(
        row,
        &json!({
            "kind":"requested_audio_offered", "turn":request,
            "provider_session":session, "accepted":refused.is_none(),
            "refused":refused, "at_us":at_us,
            "boundary":"queued preparation; no playback authority"
        })
    );
    if refused.is_none() {
        // Indexing an absent key also returns Null: require its presence.
        assert_eq!(row.get("refused"), Some(&Value::Null));
    }
    let encoded = serde_json::to_string(row).unwrap();
    for private in [
        &offer.asset,
        &offer.sha256,
        &offer.skills_boot,
        &offer.skill,
        &offer.context.worker_boot,
        &offer.context.call_id,
    ] {
        assert!(
            !encoded.contains(private),
            "offer data entered the boundary record"
        );
    }
}

#[test]
fn requested_audio_boundary_records_a_safe_fixed_refusal_without_offer_data() {
    let _clock = fixture_clock();
    let mut test = Test::new(Route::Spoken, REQUESTED_AUDIO_LEASE_US, false);
    let good = private_boundary_offer(&test);
    let mut bad = good.clone();
    bad.asset = "/unopened/private-audio-marker.wav".into();
    bad.sha256 = "private-invalid-digest-marker".into();
    let turn = (bad.context.request, bad.context.provider_session);
    let from = test.trace.len();
    let at_us = monotonic_us();
    handle_requested_audio(
        &mut test.reply,
        Some(&test.service),
        test.worker,
        test.snapshot,
        turn,
        bad.clone(),
        &mut test.trace,
    )
    .unwrap();
    assert_offer_boundary(
        &test.trace,
        from,
        turn,
        Some("the offer does not check"),
        at_us,
        &bad,
    );
    assert!(test.skills.operations.try_recv().is_err());
    let current = test.reply.as_ref().unwrap();
    assert!(!current.requested_audio_bound());
    assert_eq!(
        current.requested_audio_refusal(test.worker, test.snapshot, &good),
        None,
        "rejection before preparation must not consume the turn's offer"
    );
}

#[test]
fn requested_audio_boundary_records_acceptance_with_explicit_null_and_one_repeat_refusal() {
    let _clock = fixture_clock();
    let mut test = Test::new(Route::Spoken, REQUESTED_AUDIO_LEASE_US, false);
    let offer = private_boundary_offer(&test);
    let turn = (offer.context.request, offer.context.provider_session);
    for refused in [None, Some("this turn already had its one offer")] {
        let from = test.trace.len();
        let at_us = monotonic_us();
        handle_requested_audio(
            &mut test.reply,
            Some(&test.service),
            test.worker,
            test.snapshot,
            turn,
            offer.clone(),
            &mut test.trace,
        )
        .unwrap();
        assert_offer_boundary(&test.trace, from, turn, refused, at_us, &offer);
        assert!(test.reply.as_ref().unwrap().requested_audio_bound());
        assert_eq!(
            test.reply.as_ref().unwrap().notice.origin(),
            SpeechOrigin::Provider
        );
        assert!(
            test.pending.is_none(),
            "preparation has not dispatched audio"
        );
    }
    test.skills.started(&offer);
    assert!(
        test.reply
            .as_ref()
            .unwrap()
            .wait_requested_audio_ready(WAIT)
    );
    test.cancel("hand_stop");
    test.skills
        .finished(&offer, RequestedAudioOutcome::Cancelled);
}

#[test]
fn requested_audio_boundary_records_unavailable_and_stale_envelope_refusals() {
    let _clock = fixture_clock();
    for case in [
        "off",
        "off_and_stale",
        "request",
        "session",
        "missing_reply",
    ] {
        let mut test = Test::new(Route::Spoken, REQUESTED_AUDIO_LEASE_US, false);
        let offer = private_boundary_offer(&test);
        let mut turn = (offer.context.request, offer.context.provider_session);
        match case {
            "off_and_stale" | "request" => turn.0 += 1,
            "session" => turn.1 += 1,
            "missing_reply" => test.reply = None,
            "off" => {}
            _ => unreachable!(),
        }
        let off = case.starts_with("off");
        let refused = if off {
            "requested audio is off or unavailable"
        } else {
            "not the turn in hand"
        };
        let from = test.trace.len();
        let at_us = monotonic_us();
        handle_requested_audio(
            &mut test.reply,
            (!off).then_some(&test.service),
            test.worker,
            test.snapshot,
            turn,
            offer.clone(),
            &mut test.trace,
        )
        .unwrap();
        assert_offer_boundary(&test.trace, from, turn, Some(refused), at_us, &offer);
        assert!(test.skills.operations.try_recv().is_err(), "{case}");
        if let Some(current) = &test.reply {
            assert!(!current.requested_audio_bound(), "{case}");
            assert_eq!(
                current.requested_audio_refusal(test.worker, test.snapshot, &offer),
                None,
                "envelope refusal must not consume the current turn's offer: {case}"
            );
        }
    }
}

#[test]
fn requested_audio_boundary_records_preparation_failure_and_consumes_the_attempt() {
    let _clock = fixture_clock();
    let mut test = Test::new(Route::Spoken, REQUESTED_AUDIO_LEASE_US, false);
    let occupied = test.offer(2);
    let ticket = test.service.offer(occupied.clone()).unwrap();
    test.skills.started(&occupied);
    assert!(
        ticket.wait_ready(WAIT),
        "the independent ticket holds capacity"
    );
    let offer = private_boundary_offer(&test);
    let turn = (offer.context.request, offer.context.provider_session);
    assert_eq!(
        test.reply
            .as_ref()
            .unwrap()
            .requested_audio_refusal(test.worker, test.snapshot, &offer),
        None,
        "the reply itself has not tried an offer"
    );
    for refused in [
        "its recording could not be prepared",
        "this turn already had its one offer",
    ] {
        let from = test.trace.len();
        let at_us = monotonic_us();
        handle_requested_audio(
            &mut test.reply,
            Some(&test.service),
            test.worker,
            test.snapshot,
            turn,
            offer.clone(),
            &mut test.trace,
        )
        .unwrap();
        assert_offer_boundary(&test.trace, from, turn, Some(refused), at_us, &offer);
        let current = test.reply.as_ref().unwrap();
        assert!(!current.requested_audio_bound(), "no ticket was granted");
        assert!(!current.requested_audio_waiting());
        assert_eq!(
            current.requested_audio_refusal(test.worker, test.snapshot, &offer),
            Some("this turn already had its one offer")
        );
        assert!(test.pending.is_none(), "no recording was dispatched");
        assert!(test.skills.operations.try_recv().is_err());
    }
    assert!(
        ticket.live(monotonic_us()),
        "the occupied ticket was untouched"
    );
    drop(ticket);
    test.skills
        .finished(&occupied, RequestedAudioOutcome::Cancelled);
    assert!(test.service.wait_idle(WAIT));
}

const JOURNAL_CHILD_MODE: &str = "LAMP_REQUESTED_AUDIO_JOURNAL_TEST_MODE";
const JOURNAL_CHILD_ROOT: &str = "LAMP_REQUESTED_AUDIO_JOURNAL_TEST_ROOT";
const JOURNAL_CHILD_TEST: &str = "coordinator::tests::requested_audio::requested_audio_boundary_journals_one_refusal_and_no_acceptance";
const JOURNAL_CAPTURE_LIMIT: u64 = 65_536;

// The child has no child processes. Its threads, socket and audio fixtures all
// belong to this private root; reap it before removing any of those files.
struct JournalChild {
    child: Option<Child>,
    directory: SessionDirectory,
}

impl Drop for JournalChild {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = fs::remove_dir_all(&self.directory.path);
    }
}

fn journal_child_case(mode: &str, root: &Path) {
    let _clock = fixture_clock();
    let fixture = root.join("fixture");
    assert!(
        fixture.is_dir(),
        "the parent owns the child fixture directory"
    );
    let mut test = Test::in_directory(
        Route::Spoken,
        REQUESTED_AUDIO_LEASE_US,
        false,
        SessionDirectory { path: fixture },
    );
    let mut offer = private_boundary_offer(&test);
    let refused = match mode {
        "refused" => {
            offer.asset = "/unopened/private-audio-marker.wav".into();
            offer.sha256 = "private-invalid-digest-marker".into();
            Some("the offer does not check")
        }
        "accepted" => None,
        _ => panic!("unexpected journal child mode"),
    };
    let turn = (offer.context.request, offer.context.provider_session);
    assert_eq!(turn.0, 1, "the parent checks this request's exact line");
    let from = test.trace.len();
    let at_us = monotonic_us();
    handle_requested_audio(
        &mut test.reply,
        Some(&test.service),
        test.worker,
        test.snapshot,
        turn,
        offer.clone(),
        &mut test.trace,
    )
    .unwrap();
    assert_offer_boundary(&test.trace, from, turn, refused, at_us, &offer);
    if refused.is_none() {
        assert!(test.reply.as_ref().unwrap().requested_audio_bound());
        test.skills.started(&offer);
        assert!(
            test.reply
                .as_ref()
                .unwrap()
                .wait_requested_audio_ready(WAIT)
        );
        test.cancel("hand_stop");
        test.skills
            .finished(&offer, RequestedAudioOutcome::Cancelled);
        assert!(test.service.wait_idle(WAIT));
    } else {
        assert!(!test.reply.as_ref().unwrap().requested_audio_bound());
        assert!(test.skills.operations.try_recv().is_err());
    }
}

fn captured_journal_mode(mode: &str) -> String {
    let directory = SessionDirectory::create().unwrap();
    let mut child = JournalChild {
        child: None,
        directory,
    };
    let root = &child.directory.path;
    fs::create_dir(root.join("fixture")).unwrap();
    fs::set_permissions(root.join("fixture"), fs::Permissions::from_mode(0o700)).unwrap();
    let stderr = root.join("stderr");
    let stdout = root.join("stdout");
    child.child = Some(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                JOURNAL_CHILD_TEST,
                "--nocapture",
                "--test-threads=1",
            ])
            .env(JOURNAL_CHILD_MODE, mode)
            .env(JOURNAL_CHILD_ROOT, root)
            .stdin(Stdio::null())
            .stdout(Stdio::from(fs::File::create(&stdout).unwrap()))
            .stderr(Stdio::from(fs::File::create(&stderr).unwrap()))
            .spawn()
            .unwrap(),
    );
    // These are parent watchdog/storage limits, not a measured runtime or
    // physical-latency promise. A failed/oversized child is killed and reaped.
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        for path in [&stdout, &stderr] {
            assert!(
                fs::metadata(path).unwrap().len() <= JOURNAL_CAPTURE_LIMIT,
                "journal child exceeded its capture limit"
            );
        }
        if let Some(status) = child.child.as_mut().unwrap().try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "journal child did not finish");
        thread::sleep(Duration::from_millis(5));
    };
    let mut journal = String::new();
    fs::File::open(stderr)
        .unwrap()
        .take(JOURNAL_CAPTURE_LIMIT + 1)
        .read_to_string(&mut journal)
        .unwrap();
    assert!(journal.len() as u64 <= JOURNAL_CAPTURE_LIMIT);
    assert!(
        status.success(),
        "journal child assertions failed: {status}; {journal}"
    );
    assert_eq!(fs::read_to_string(root.join("finished")).unwrap(), mode);
    journal
}

#[test]
fn requested_audio_boundary_journals_one_refusal_and_no_acceptance() {
    if let Some(mode) = std::env::var_os(JOURNAL_CHILD_MODE) {
        let mode = mode.to_str().unwrap();
        let root = PathBuf::from(std::env::var_os(JOURNAL_CHILD_ROOT).unwrap());
        journal_child_case(mode, &root);
        // The fixture, ticket, service and fake daemon have all dropped before
        // the parent accepts this completion witness and the successful exit.
        fs::write(root.join("finished"), mode).unwrap();
        return;
    }
    for mode in ["refused", "accepted"] {
        let journal = captured_journal_mode(mode);
        let lines: Vec<_> = journal
            .lines()
            .filter(|line| line.contains("requested audio refused"))
            .collect();
        let expected = if mode == "refused" {
            vec!["lamp-voice: request 1: requested audio refused: the offer does not check"]
        } else {
            Vec::new()
        };
        assert_eq!(lines, expected, "exact refusal text and count for {mode}");
        for private in [
            "private-audio-marker",
            "private-invalid-digest-marker",
            "private-skills-boot-marker",
            "private-recording-marker",
            "private-call-marker",
        ] {
            assert!(!journal.contains(private), "offer data entered stderr");
        }
    }
}

#[test]
fn written_reminder_say_and_offline_turns_never_authorize_requested_recordings() {
    let _clock = fixture_clock();
    for route in [Route::Written, Route::Reminder, Route::Say, Route::Offline] {
        let mut test = Test::new(route, REQUESTED_AUDIO_LEASE_US, false);
        let offer = test.offer(1);
        assert!(!test.accept_offer(offer), "{route:?}");
        assert!(test.skills.operations.try_recv().is_err());
        assert!(!test.reply.as_ref().unwrap().requested_audio_waiting());
        assert!(test.dispatch().is_none());
    }
}

#[test]
fn an_empty_or_gapped_confirmation_cancels_instead_of_starting_the_recording() {
    let _clock = fixture_clock();
    for gapped in [false, true] {
        let mut test = Test::new(Route::Spoken, REQUESTED_AUDIO_LEASE_US, false);
        let offer = test.load(1);
        test.answer_news();
        if gapped {
            let chunk = test.generation();
            test.accept(&chunk, 240);
            let at = monotonic_us();
            test.event(WorkerEvent::PlaybackGap {
                phase: PlaybackGapPhase::Started,
                gap: PlaybackGap {
                    owner: chunk.permit.owner(),
                    first_sample: PlaybackCursor {
                        epoch: 1,
                        frame: test.cursor,
                    },
                    end_sample: PlaybackCursor {
                        epoch: 1,
                        frame: test.cursor + 240,
                    },
                    started_at_us: at,
                    last_zero_accepted_at_us: at,
                    zero_frames: 240,
                },
                observed_at_us: at,
            });
            test.retire(&chunk);
        }
        test.provider_complete();
        assert!(!test.prepare());
        assert!(!test.reply.as_ref().unwrap().requested_audio_waiting());
        assert!(test.dispatch().is_none());
        assert!(test.finish_if_settled());
        assert!(test.heard_news.is_empty());
        test.skills
            .finished(&offer, RequestedAudioOutcome::Cancelled);
    }
}

#[test]
fn stop_privacy_capture_gap_and_new_owner_cancel_even_a_late_successful_loader() {
    let _clock = fixture_clock();
    for playing in [false, true] {
        for reason in ["hand_stop", "privacy", "capture_gap", "new_owner"] {
            let mut test = Test::new(Route::Spoken, REQUESTED_AUDIO_LEASE_US, !playing);
            let offer = test.offer(1);
            assert!(test.accept_offer(offer.clone()));
            test.skills.started(&offer);
            if playing {
                assert!(
                    test.reply
                        .as_ref()
                        .unwrap()
                        .wait_requested_audio_ready(WAIT)
                );
                test.confirmed();
                assert!(test.prepare());
                let first = test.dispatch().unwrap();
                test.accept(&first, 240);
            }
            test.cancel(reason);
            if !playing {
                test.skills.release_start();
            }
            test.skills
                .finished(&offer, RequestedAudioOutcome::Cancelled);
            assert!(test.service.wait_idle(WAIT));
            assert!(test.dispatch().is_none(), "{reason}, playing={playing}");
            if reason == "new_owner" {
                assert!(!test.accept_offer(offer.clone()));
                assert_ne!(
                    test.reply.as_ref().unwrap().owner.turn(),
                    offer.context.request
                );
            }
            assert!(
                !test
                    .trace
                    .iter()
                    .any(|event| { event["kind"] == "requested_audio_final_sample_retired" })
            );
        }
    }
}

#[test]
fn a_real_bus_mute_generation_is_terminal_even_after_unmute() {
    let _clock = fixture_clock();
    for playing in [false, true] {
        let mut test = Test::new(Route::Spoken, REQUESTED_AUDIO_LEASE_US, false);
        let offer = test.load(1);
        test.confirmed();
        assert!(test.prepare());
        if playing {
            let first = test.dispatch().unwrap();
            test.accept(&first, 240);
        }
        publish_state(&mut test.publisher, 1, true);
        publish_state(&mut test.publisher, 1, false);
        assert!(test.service.observe_fixture(WAIT));
        // Even if Latest coalesces the mute, the generation still revokes the
        // old recording. Finish is a positive monitor/worker synchronization.
        test.skills
            .finished(&offer, RequestedAudioOutcome::Cancelled);
        assert!(test.reply.as_ref().unwrap().requested_audio_invalid(
            test.worker,
            test.snapshot,
            monotonic_us()
        ));
        assert!(test.service.wait_cached(WAIT));
        assert!(test.dispatch().is_none());
        assert!(!test.accept_offer(offer));
        test.complete_media();
        assert!(test.skills.operations.try_recv().is_err());
    }
}

#[test]
fn the_actual_speaker_permit_cannot_outlive_a_short_skills_lease() {
    let _clock = fixture_clock();
    let mut test = Test::new(Route::Spoken, 90_000, true);
    let offer = test.offer(1);
    assert!(test.accept_offer(offer.clone()));
    test.skills.started(&offer);
    // Prepare the confirmation before releasing authorization, so the short
    // lease measures dispatch rather than unrelated fixture setup.
    test.confirmed();
    test.skills.release_start();
    assert!(
        test.reply
            .as_ref()
            .unwrap()
            .wait_requested_audio_ready(WAIT)
    );
    assert!(test.prepare());
    let chunk = test.dispatch().unwrap();
    let until = test.skills.lease_until.load(Ordering::Acquire);
    assert!(chunk.permit.expires_at().as_micros() <= until);
    assert!(chunk.permit.expires_at().as_micros() < chunk.permit.issued_at().as_micros() + 100_000);
    assert_eq!(chunk.permit.owner(), test.reply.as_ref().unwrap().owner);
    test.cancel("hand_stop");
    test.skills
        .finished(&offer, RequestedAudioOutcome::Cancelled);
}

#[test]
fn an_accepted_offer_survives_ready_session_renewal_and_plays_for_its_original_reply() {
    let _clock = fixture_clock();
    let mut test = Test::new(Route::Spoken, REQUESTED_AUDIO_LEASE_US, false);
    let offer = test.load(1);
    let current = test.reply.as_ref().unwrap();
    let original_owner = current.owner;
    assert!(current.local_input_ended && current.cloud.input_end_submitted);
    assert!(current.cloud.started && current.cloud.outcome.is_none());
    assert_eq!(current.cloud.session, offer.context.provider_session);
    // The priority Ready can overtake the old connection's queued output
    // receipts. Idle renewal uses the same close reason as server GoAway.
    let mut renewed = ready_link(2, offer.context.provider_session + 1);
    renewed.state = crate::provider_link::ProviderLinkState::Ready {
        setup_us: 1234,
        attempts: 1,
        context: crate::provider_link::LinkContext::Fresh,
        outage_started_at_us: Some(renewed.observed_at_us - 10),
        after: Some(crate::provider_worker::Fault::ServerGoAway),
    };
    test.cloud.observe(renewed, monotonic_us()).unwrap();
    assert_eq!(
        test.cloud.ready_session(),
        Some(offer.context.provider_session + 1)
    );
    assert!(
        !test
            .reply
            .as_mut()
            .unwrap()
            .cloud
            .abandon_if_changed(&test.cloud)
    );
    let mut queue = VecDeque::new();
    let mut draining = false;
    apply_submission_gate(
        &test.cloud,
        &mut test.reply,
        &mut queue,
        &mut InputAdmission::default(),
        &mut draining,
        &mut test.trace,
    )
    .unwrap();
    let current = test.reply.as_ref().unwrap();
    assert!(!draining && queue.is_empty() && !current.cloud.input_abandoned);
    assert_eq!(current.owner, original_owner);
    assert_eq!(current.cloud.session, offer.context.provider_session);
    assert!(current.requested_audio_bound());
    assert!(!current.requested_audio_invalid(test.worker, test.snapshot, monotonic_us()));
    assert!(
        test.provider_peer
            .data
            .receive::<ProviderInput>()
            .unwrap()
            .is_none()
    );

    let confirmation = test.generation();
    test.provider_complete();
    test.accept(&confirmation, 240);
    assert!(
        !test.prepare(),
        "renewal cannot substitute for confirmation retirement"
    );
    test.retire(&confirmation);
    assert!(test.prepare());
    assert_eq!(
        test.reply.as_ref().unwrap().notice.origin(),
        SpeechOrigin::RequestedAudio
    );
    let first = test.dispatch().unwrap();
    assert_eq!(first.permit.owner(), original_owner);
    assert_eq!(first.samples, [100; 240]);
    assert!(!first.end_of_speech);
    test.accept(&first, 240);
    let last = test.dispatch().unwrap();
    assert_eq!(last.permit.owner(), original_owner);
    assert_eq!(last.samples, [100; 240]);
    assert!(last.end_of_speech);
    test.accept(&last, 240);
    test.retire(&last);
    test.complete_media();
    assert!(test.dispatch().is_none());
    assert!(!test.prepare());
    assert_eq!(
        test.reply.as_ref().unwrap().cloud.session,
        offer.context.provider_session
    );
    // The real socket's final receipt compares the entire original offer,
    // including call ID, worker/session/request and both original deadlines.
    test.skills
        .finished(&offer, RequestedAudioOutcome::Completed);
    assert_eq!(
        test.trace
            .iter()
            .filter(|event| event["kind"] == "requested_audio_handoff")
            .count(),
        1
    );
}

#[test]
fn an_accepted_offer_stays_bound_to_its_original_worker_session_and_privacy() {
    let _clock = fixture_clock();
    let mut test = Test::new(Route::Spoken, REQUESTED_AUDIO_LEASE_US, false);
    let offer = test.load(1);
    let current = test.reply.as_ref().unwrap();
    assert!(current.requested_audio_bound());
    assert!(!current.cloud.movable);
    assert!(!current.cloud.same_session_only);
    assert!(!current.requested_audio_invalid(test.worker, test.snapshot, monotonic_us()));
    assert!(current.requested_audio_invalid(new_boot().unwrap(), test.snapshot, monotonic_us()));
    let mut replacement = CloudAvailability::default();
    replacement
        .observe(ready_link(1, 2), monotonic_us())
        .unwrap();
    let mut replay = VecDeque::from([ProviderInput::Start {
        request: offer.context.request,
        session: offer.context.provider_session,
        privacy_generation: offer.context.privacy_generation,
        continues: false,
        continues_cut_off: false,
        over_the_lamp: false,
        after_stop: false,
        follows_on: false,
        over_the_lamps_voice: false,
        heard_of_cut: None,
    }]);
    assert!(second::rebind(&replacement, test.reply.as_mut().unwrap(), &mut replay,).is_err());
    assert_eq!(
        test.reply.as_ref().unwrap().cloud.session,
        offer.context.provider_session
    );
    assert!(
        matches!(replay.front(), Some(ProviderInput::Start { session, .. })
        if *session == offer.context.provider_session)
    );
    test.reply.as_mut().unwrap().cloud.session += 1;
    assert!(test.reply.as_ref().unwrap().requested_audio_invalid(
        test.worker,
        test.snapshot,
        monotonic_us()
    ));
    test.reply.as_mut().unwrap().cloud.session -= 1;
    test.confirmed();
    assert!(test.prepare());
    test.owner
        .set_microphone_permission(now(), Permission::Denied)
        .unwrap();
    let closed = test.owner.snapshot(now()).unwrap();
    assert!(test.reply.as_ref().unwrap().requested_audio_invalid(
        test.worker,
        closed,
        monotonic_us()
    ));
    test.owner
        .set_microphone_permission(now(), Permission::Allowed)
        .unwrap();
    test.renew_input();
    let reopened = test.owner.snapshot(now()).unwrap();
    assert_ne!(
        reopened.microphone_generation(),
        offer.context.privacy_generation
    );
    assert!(test.reply.as_ref().unwrap().requested_audio_invalid(
        test.worker,
        reopened,
        monotonic_us()
    ));
    assert!(
        dispatch_speaker_chunk(
            &mut test.owner,
            &mut test.reply,
            &mut test.speaker.channels.data,
            &mut test.pending,
            &mut test.sequence,
            &mut test.snapshot,
            &mut test.mask,
        )
        .is_err()
    );
    assert!(test.peer.data.receive::<SpeakerChunk>().unwrap().is_none());
    // Privacy has already revoked the controller's owner. The coordinator
    // drops its detached Reply rather than trying to cancel a new owner.
    record_cancelled_reply(
        test.reply.take().unwrap(),
        &mut test.trace,
        "privacy",
        monotonic_us(),
    )
    .unwrap();
    test.skills
        .finished(&offer, RequestedAudioOutcome::Cancelled);
}
