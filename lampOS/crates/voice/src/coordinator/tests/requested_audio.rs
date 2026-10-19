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
    RequestedAudioOutcome, RequestedAudioPhase, SpeakerState,
};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
};

const WAIT: Duration = Duration::from_secs(3);
const CLOCK: &str = "requested-recording-fixture";

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
    fn new(directory: &Path, lease_us: u64, hold_start: bool) -> Self {
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
                            let until_us =
                                (lamp_ipc::monotonic_us() + lease_us).min(offer.finish_before_us);
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
    cloud: CloudAvailability,
    sequence: u64,
    cursor: u64,
    asset: PathBuf,
    sha256: String,
    _directory: SessionDirectory,
}

impl Test {
    fn new(route: Route, lease_us: u64, hold_start: bool) -> Self {
        let directory = SessionDirectory::create().unwrap();
        let skills = Skills::new(&directory.path, lease_us, hold_start);
        let bus = Bus::new(directory.path.join("bus")).unwrap();
        let mut publisher = bus
            .publisher_declared(SpeakerState::TOPIC, &SpeakerState::declaration(), "actions")
            .unwrap();
        publish_state(&mut publisher, 0, false);
        let service = Service::new(bus, directory.path.clone(), CLOCK.into()).unwrap();
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

    fn renew_input(&mut self) {
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
                None,
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
        let issued_us = lamp_ipc::monotonic_us();
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
            at_us: lamp_ipc::monotonic_us(),
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
fn prepared_recordings_wait_for_retirement_keep_the_spoken_owner_and_finish_after_retirement() {
    let mut test = Test::new(Route::Spoken, REQUESTED_AUDIO_LEASE_US, false);
    let mut handoffs = Vec::new();
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
        handoffs[0], handoffs[1]
    );
}

#[test]
fn offers_require_the_current_worker_session_request_and_privacy_identity() {
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

#[test]
fn written_reminder_say_and_offline_turns_never_authorize_requested_recordings() {
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
    for gapped in [false, true] {
        let mut test = Test::new(Route::Spoken, REQUESTED_AUDIO_LEASE_US, false);
        let offer = test.load(1);
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
        test.skills
            .finished(&offer, RequestedAudioOutcome::Cancelled);
    }
}

#[test]
fn stop_privacy_capture_gap_and_new_owner_cancel_even_a_late_successful_loader() {
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
fn an_accepted_offer_stays_bound_to_its_original_worker_session_and_privacy() {
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
