//! Consumer -> typed bus/private Get -> real claim IPC -> actual coordinator
//! turn -> real speaker control -> durable completion -> Schedule Ack.
//! The provider PCM and speaker receipts are authored, not acoustic evidence.
//! Each test runs under the stepped scenario clock (`crate::scenario_clock`):
//! no logical time passes inside a consumer tick, so the durable ledger and
//! Schedule commits a tick makes cannot spend its 250 ms socket budget on a
//! busy host's disk (lampOS#935). The budget itself is the consumer tests'.
use super::*;
use crate::reminders::{RoundtripFixture, ToWorker};
use lamp_audio::ownership::PlaybackCursor;
use lamp_schedule::delivery::{PreparedToken, Resolution};

/// A host safety net only: no logical time passes in these tests.
const WATCHDOG: Duration = Duration::from_secs(24);

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

struct Test {
    _directory: SessionDirectory,
    fixture: RoundtripFixture,
    token: PreparedToken,
    speaker: Speaker,
    peer: WorkerChannels,
    owner: Controller,
    reply: Option<Reply>,
    pending: Option<PendingSpeaker>,
    snapshot: Snapshot,
    mask: PlaybackMask,
    trace: SessionState,
    cloud: CloudAvailability,
    sequence: u64,
    cursor: u64,
    // Last, so the fixture and its consumer drop while the clock is scoped.
    _clock: crate::scenario_clock::Scope,
}
impl Test {
    fn new() -> Self {
        let clock = crate::scenario_clock::Scope::enter(WATCHDOG).unwrap();
        let mut fixture = RoundtripFixture::new();
        let offer = fixture.offer();
        let token = offer.token.clone();
        let directory = SessionDirectory::create().unwrap();
        let (channels, peer) = pair(&directory, "speaker");
        let (mut provider, mut provider_peer) = pair(&directory, "provider");
        let mut speaker = Speaker { channels };
        let mut owner = Controller::new(new_boot().unwrap(), now());
        owner
            .set_microphone_permission(now(), Permission::Allowed)
            .unwrap();
        let until = MonoTime::from_micros(monotonic_us() + 1_000_000);
        owner
            .set_capture(now(), CaptureState::RetainingUntil(until))
            .unwrap();
        owner
            .set_admission(now(), AdmissionState::OpenUntil(until))
            .unwrap();
        let mut reply = None;
        let mut queue = VecDeque::new();
        let mut trace = SessionState::default();
        let snapshot = commit_reminder_turn(
            &mut owner,
            &mut reply,
            &mut queue,
            offer.clone(),
            fixture.parent(),
            1,
            PlayoutStart::default(),
            &mut trace,
            |state| speaker.publish_speaker_snapshot(state),
        )
        .unwrap();
        let turn = reply.as_ref().unwrap().owner;
        assert!(fixture.parent().active_valid(turn, monotonic_us()));
        assert_eq!(queue.len(), 1);
        assert!(matches!(queue.front(), Some(ProviderInput::Reminder {
            request, session: 1, privacy_generation, text, late, late_by_ms, ..
        }) if *request == turn.turn() && *privacy_generation == snapshot.microphone_generation()
            && text == &offer.text && *late == offer.late && *late_by_ms == offer.late_by_ms));

        // Use the real coordinator submission gate and Channel send. The
        // fixture worker observes exactly one Reminder, never Written/Start.
        let mut cloud = CloudAvailability::default();
        cloud.observe(ready_link(1, 1), monotonic_us()).unwrap();
        flush_provider_input(&cloud, &mut reply, &mut queue, &mut provider.data).unwrap();
        assert!(queue.is_empty());
        let command = provider_peer
            .data
            .receive::<ProviderInput>()
            .unwrap()
            .unwrap();
        assert!(
            matches!(command, ProviderInput::Reminder { request, .. } if request == turn.turn())
        );
        assert!(
            provider_peer
                .data
                .receive::<ProviderInput>()
                .unwrap()
                .is_none()
        );
        observe_provider_output(
            &mut cloud,
            &mut reply,
            &ProviderOutput::Started {
                request: turn.turn(),
                session: 1,
                waiting_for_barrier: false,
            },
        )
        .unwrap();
        Self {
            _directory: directory,
            fixture,
            token,
            speaker,
            peer,
            owner,
            reply,
            pending: None,
            snapshot,
            mask: PlaybackMask::new(TurnTaking::VadOnly),
            trace,
            cloud,
            sequence: 0,
            cursor: 0,
            _clock: clock,
        }
    }

    fn renew_input_evidence(&mut self) {
        // Explicit fixture capture/admission heartbeats, not readiness inferred
        // from storage progress or provider output.
        let until = MonoTime::from_micros(monotonic_us() + 1_000_000);
        self.owner
            .set_capture(now(), CaptureState::RetainingUntil(until))
            .unwrap();
        self.owner
            .set_admission(now(), AdmissionState::OpenUntil(until))
            .unwrap();
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
    }

    fn generation(&mut self, value: i16) -> SpeakerChunk {
        self.renew_input_evidence();
        let request = self.reply.as_ref().unwrap().owner.turn();
        let received_at = monotonic_us();
        let samples = vec![value; 240];
        observe_provider_output(
            &mut self.cloud,
            &mut self.reply,
            &ProviderOutput::Audio {
                spoken: true,
                request,
                sequence: self.sequence + 1,
                provider_event_at_us: received_at,
                source_frames: samples.len(),
                source_offset: 0,
                samples: samples.clone(),
            },
        )
        .unwrap();
        let current = self.reply.as_mut().unwrap();
        current.append_provider_audio(samples, received_at);
        current.had_audio = true;
        observe_provider_output(
            &mut self.cloud,
            &mut self.reply,
            &ProviderOutput::GenerationComplete { request },
        )
        .unwrap();
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
        let chunk = self.peer.data.receive::<SpeakerChunk>().unwrap().unwrap();
        assert!(chunk.end_of_speech);
        assert_eq!(chunk.samples, vec![value; 240]);
        self.trace
            .receipts
            .dispatched(chunk.permit.owner(), chunk.chunk_sequence)
            .unwrap();
        chunk
    }

    fn accept(&mut self, chunk: &SpeakerChunk, frames: usize) {
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
    }

    fn retire(&mut self, chunk: &SpeakerChunk) {
        self.event(WorkerEvent::SpeechRetired {
            owner: chunk.permit.owner(),
            sequence: chunk.chunk_sequence,
            final_sample: PlaybackCursor {
                epoch: 1,
                frame: self.cursor,
            },
            observed_at_us: monotonic_us(),
        });
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

    fn finish_if_settled(&mut self) -> bool {
        let current = self.reply.as_mut().unwrap();
        if !current.settled(&self.pending) {
            return false;
        }
        self.owner.complete_turn(now(), current.owner).unwrap();
        self.snapshot = self.owner.snapshot(now()).unwrap();
        self.speaker
            .publish_speaker_snapshot(self.snapshot)
            .unwrap();
        current.complete_reminder(&self.pending, self.snapshot);
        // Only the real Reply drop releases its real manager receipt. The
        // consumer will not see it until the separate IPC pump below.
        self.reply = None;
        true
    }

    fn assert_not_completed(&mut self) {
        assert_eq!(self.fixture.settle_parent(), 0);
        let view = self.fixture.view(&self.token);
        assert!(view.completion.is_none() && view.ack.is_none() && view.resolution.is_none());
        assert!(self.fixture.ack_requests().is_empty());
        assert!(self.fixture.pending());
        self.fixture.tick(); // Real fresh due/Get renews the same active offer.
        assert_eq!(self.fixture.settle_parent(), 0);
    }

    fn completed_two_generations(&mut self) {
        let first = self.generation(17);
        self.accept(&first, 120);
        assert!(self.pending.is_some());
        self.assert_not_completed();
        self.accept(&first, 120);
        assert!(self.pending.is_none());
        assert!(!self.finish_if_settled());
        self.retire(&first);
        assert!(
            !self.finish_if_settled(),
            "one retired generation is not the whole turn"
        );
        self.assert_not_completed();
        let second = self.generation(23);
        self.provider_complete();
        self.accept(&second, 120);
        self.accept(&second, 120);
        assert!(
            !self.finish_if_settled(),
            "final PCM acceptance is not its retirement"
        );
        self.assert_not_completed();
        self.retire(&second);
        assert!(self.finish_if_settled());
        assert!(
            self.fixture.view(&self.token).completion.is_none(),
            "IPC has not persisted the proof yet"
        );
        assert!(self.fixture.ack_requests().is_empty());
        assert_eq!(self.fixture.settle_parent(), 1);
        let [ToWorker::Complete { token, proof }] = self.fixture.received() else {
            panic!("the actual Reply did not emit one completion");
        };
        assert_eq!(token, &self.token);
        assert_eq!(proof.correlation, self.token.correlation);
        assert_eq!(proof.owner_boot, first.permit.owner().boot().bytes());
        assert_eq!(proof.turn, first.permit.owner().turn());
        assert_eq!(proof.retired_generations, 2);
        assert_eq!((proof.final_epoch, proof.final_frame), (1, 480));
        assert_eq!(
            self.fixture.view(&self.token).completion.as_ref(),
            Some(proof)
        );
        assert!(
            self.fixture.ack_requests().is_empty(),
            "durable completion alone has not called Schedule Ack"
        );
        assert!(self.fixture.pending());
    }
}

#[test]
fn actual_consumer_ipc_coordinator_and_two_retirements_ack_only_after_whole_completion() {
    let mut test = Test::new();
    test.completed_two_generations();
    let offered = test.fixture.offered_count();
    test.fixture.tick();
    assert!(!test.fixture.pending());
    assert_eq!(test.fixture.ack_requests().len(), 1);
    assert!(matches!(
        test.fixture.view(&test.token).resolution,
        Some(Resolution::AckConfirmed { .. })
    ));
    assert_eq!(test.fixture.offered_count(), offered);
    assert!(test.fixture.parent().offer(monotonic_us()).is_none());
}

fn cancellation_stays_pending(microphone_denied: bool) {
    let mut test = Test::new();
    let first = test.generation(17);
    test.accept(&first, 240);
    test.retire(&first);
    test.assert_not_completed();
    let second = test.generation(23);
    test.provider_complete();
    test.accept(&second, 240);
    if microphone_denied {
        test.owner
            .set_microphone_permission(now(), Permission::Denied)
            .unwrap();
        let reply = test.reply.take().unwrap();
        record_cancelled_reply(reply, &mut test.trace, "physical_mute", monotonic_us()).unwrap();
    } else {
        cancel_reply(
            &mut test.owner,
            &mut test.reply,
            &mut test.trace,
            "user_interrupted",
        )
        .unwrap();
    }
    test.snapshot = test.owner.snapshot(now()).unwrap();
    test.speaker
        .publish_speaker_snapshot(test.snapshot)
        .unwrap();
    assert!(test.snapshot.owner().is_none());
    if microphone_denied {
        assert_eq!(test.snapshot.microphone_permission(), Permission::Denied);
    }
    // Even this exact old final retirement passes through the real speaker
    // handler after cancellation; it cannot revive a detached reminder.
    test.retire(&second);
    assert_eq!(test.fixture.settle_parent(), 1);
    assert!(
        matches!(test.fixture.received(), [ToWorker::Abandon { token, .. }] if token == &test.token)
    );
    test.fixture.tick();
    assert!(test.fixture.pending());
    assert!(test.fixture.ack_requests().is_empty());
    let view = test.fixture.view(&test.token);
    assert!(view.completion.is_none() && view.ack.is_none());
    let offered = test.fixture.offered_count();
    test.fixture.restart_consumer();
    test.fixture.tick();
    assert_eq!(test.fixture.offered_count(), offered);
    assert!(test.fixture.parent().offer(monotonic_us()).is_none());
    assert!(test.fixture.pending());
    assert!(test.fixture.ack_requests().is_empty());
}

#[test]
fn interruption_after_final_acceptance_keeps_the_durable_occurrence_pending() {
    cancellation_stays_pending(false);
}

#[test]
fn microphone_denial_after_final_acceptance_keeps_the_durable_occurrence_pending() {
    cancellation_stays_pending(true);
}

#[test]
fn completed_real_reply_reopens_before_ack_and_recovers_lost_ack_without_re_speech() {
    let mut test = Test::new();
    test.completed_two_generations();
    let original_completion = test.fixture.view(&test.token).completion.unwrap();
    let offered = test.fixture.offered_count();

    // Crash window one: completion is durable but no Ack intent has been sent.
    // Close and reopen the real Store, manager and claim IPC under a new session.
    test.fixture.restart_consumer();
    assert_eq!(
        test.fixture.view(&test.token).completion.as_ref(),
        Some(&original_completion)
    );
    assert!(test.fixture.pending());
    test.fixture.lose_ack_replies();
    test.fixture.tick();
    let pending_ack = test.fixture.view(&test.token);
    assert!(pending_ack.ack.is_some() && pending_ack.resolution.is_none());
    assert!(
        !test.fixture.pending(),
        "the real Schedule committed Ack despite its lost reply"
    );
    assert_eq!(test.fixture.offered_count(), offered);
    assert!(test.fixture.parent().offer(monotonic_us()).is_none());

    // Crash window two: keep the exact durable Ack request. Its retransmitted
    // reply is lost too; the private Status receipt establishes the commit.
    test.fixture.restart_consumer();
    test.fixture.tick();
    let recovered = test.fixture.view(&test.token);
    assert_eq!(recovered.completion.as_ref(), Some(&original_completion));
    assert!(matches!(
        recovered.resolution,
        Some(Resolution::AckConfirmed { .. })
    ));
    let acks = test.fixture.ack_requests();
    assert_eq!(acks.len(), 2);
    assert_eq!(acks[0], acks[1]);
    assert_eq!(test.fixture.offered_count(), offered);
    assert!(test.fixture.parent().offer(monotonic_us()).is_none());
    assert!(!test.fixture.pending());
}
