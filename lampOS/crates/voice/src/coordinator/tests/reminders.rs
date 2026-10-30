//! Real coordinator owner, speaker IPC, partial acceptance and retirement joins.
//! These synthetic PCM receipts establish neither acoustic nor model fidelity.
use super::*;
use crate::reminders::{Probe, RoundtripFixture, ToWorker, receipt_for_test};
use crate::say::{Answer, Outcome};
use lamp_audio::ownership::{PlaybackCursor, QueuedRange};
use std::sync::mpsc;

#[test]
fn reminder_quiet_uses_current_admission_time_after_new_capture_arrived_in_loop() {
    let loop_start = 2_000_000;
    let captured = loop_start + 1;
    let admission_at = loop_start + 2;
    assert!(!reminder_quiet(loop_start, 0, 0, Some(captured)));
    assert!(reminder_quiet(admission_at, 0, 0, Some(captured)));
    assert!(!reminder_quiet(admission_at, 0, 0, None));
    assert!(!reminder_quiet(admission_at, 0, 0, Some(admission_at + 1)));
    assert!(reminder_quiet(
        admission_at,
        0,
        0,
        Some(admission_at - INPUT_LEASE_US)
    ));
    assert!(!reminder_quiet(
        admission_at,
        0,
        0,
        Some(admission_at - INPUT_LEASE_US - 1)
    ));
    for (cycle, speech) in [
        (admission_at - 999_999, 0),
        (0, admission_at - 999_999),
        (admission_at + 1, 0),
    ] {
        assert!(!reminder_quiet(admission_at, cycle, speech, Some(captured)));
    }
    assert!(reminder_quiet(
        admission_at,
        admission_at - 1_000_000,
        0,
        Some(captured)
    ));
}

/// #1712, lamp-4ace 19 Oct 13:23: "Let me look that up" before a web search
/// took a picture twice. Its words come in pieces, and "Let me look" alone
/// is not yet a look: the next piece decides. A look only once the phrase
/// closes.
#[test]
fn an_answer_that_looks_something_up_takes_no_picture() {
    let look = |pieces: &[&str]| {
        let mut opening = None;
        let mut wanted = None;
        let mut trace = SessionState::default();
        for text in pieces {
            record_look_intent(
                true,
                Some(7),
                text,
                None,
                &mut opening,
                &mut wanted,
                &mut trace,
            )
            .unwrap();
        }
        wanted.map(|(turn, _)| turn)
    };
    assert_eq!(look(&["Let me look", " that up."]), None);
    assert_eq!(look(&["Let me ", "look into", " it."]), None);
    assert_eq!(look(&["Let me look", "."]), Some(7));
    assert_eq!(look(&["Let me look."]), Some(7));
}

#[test]
fn stored_reminder_words_cannot_start_a_camera_followup_even_when_transcript_is_late() {
    let mut opening = None;
    let mut wanted = None;
    let mut trace = SessionState::default();
    for text in ["Let me ", "take a look."] {
        record_look_intent(
            true,
            Some(1),
            text,
            None,
            &mut opening,
            &mut wanted,
            &mut trace,
        )
        .unwrap();
    }
    assert_eq!(wanted.map(|(turn, _)| turn), Some(1));
    assert_eq!(trace.len(), 1);
    wanted = None;
    for text in ["Let me ", "take a look."] {
        record_look_intent(
            true,
            Some(2),
            text,
            Some(2),
            &mut opening,
            &mut wanted,
            &mut trace,
        )
        .unwrap();
    }
    assert!(wanted.is_none());
    assert_eq!(opening.as_ref().unwrap().0, 1);
    assert_eq!(trace.len(), 1);
    record_look_intent(
        true,
        Some(3),
        "Let me look.",
        Some(2),
        &mut opening,
        &mut wanted,
        &mut trace,
    )
    .unwrap();
    assert_eq!(wanted.map(|(turn, _)| turn), Some(3));
    assert_eq!(trace.len(), 2);
    wanted = None;
    for turn in [1, 2] {
        record_look_intent(
            true,
            Some(turn),
            "Let me look.",
            Some(2),
            &mut opening,
            &mut wanted,
            &mut trace,
        )
        .unwrap();
    }
    assert!(wanted.is_none());
    assert_eq!(opening.as_ref().unwrap().0, 3);
    assert_eq!(trace.len(), 2);
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
struct Test {
    _directory: SessionDirectory,
    speaker: Speaker,
    peer: WorkerChannels,
    owner: Controller,
    reply: Option<Reply>,
    probe: Probe,
    pending: Option<PendingSpeaker>,
    snapshot: Snapshot,
    mask: PlaybackMask,
    trace: SessionState,
    sequence: u64,
    cursor: u64,
}
impl Test {
    fn new() -> Self {
        let directory = SessionDirectory::create().unwrap();
        let (channels, peer) = pair(&directory, "speaker");
        let (mut owner, mut reply, _) = active_reply();
        owner
            .playback_ended(now(), reply.playback.take().unwrap())
            .unwrap();
        reply.pcm.clear();
        reply.had_audio = false;
        let snapshot = owner.snapshot(now()).unwrap();
        let (receipt, probe) = receipt_for_test(reply.owner, snapshot.microphone_generation(), 1);
        reply.reminder = Some(receipt);
        let mut trace = SessionState::default();
        trace
            .receipts
            .admitted(reply.owner, monotonic_us())
            .unwrap();
        Self {
            _directory: directory,
            speaker: Speaker { channels },
            peer,
            owner,
            reply: Some(reply),
            probe,
            pending: None,
            snapshot,
            mask: PlaybackMask::new(TurnTaking::VadOnly),
            trace,
            sequence: 0,
            cursor: 0,
        }
    }
    fn ask_skill(&mut self) -> mpsc::Receiver<Answer> {
        let (ticket, answered) = crate::say::ticket_for_test("reminders");
        begun_for(Some(ticket), &mut self.reply);
        assert!(matches!(answered.try_recv(), Ok(Answer::Begun(_))));
        answered
    }
    fn event(&mut self, event: WorkerEvent) -> Result<()> {
        self.peer.control.send(event)?;
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
        )?;
        Ok(())
    }
    fn generation(&mut self, value: i16) -> SpeakerChunk {
        let current = self.reply.as_mut().unwrap();
        current.append_provider_audio(vec![value; 240], monotonic_us());
        current.had_audio = true;
        current.generation_done = true;
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
        })
        .unwrap();
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
        })
        .unwrap();
    }
    fn provider_complete(&mut self) {
        let request = self.reply.as_ref().unwrap().owner.turn();
        observe_provider_output(
            &mut CloudAvailability::default(),
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
        // As the coordinator's loop does: a skill's ask on the turn is
        // answered on the same proof (#939, (c)).
        current.answer_say(&self.pending, self.snapshot);
        self.reply = None;
        true
    }

    fn renew_input_evidence(&mut self) {
        // Explicit capture/admission heartbeats after fixture storage work;
        // provider output and reminder preparation do not renew readiness.
        let until = MonoTime::from_micros(monotonic_us() + 1_000_000);
        self.owner
            .set_capture(now(), CaptureState::RetainingUntil(until))
            .unwrap();
        self.owner
            .set_admission(now(), AdmissionState::OpenUntil(until))
            .unwrap();
        self.snapshot = self.owner.snapshot(now()).unwrap();
    }

    fn followup(&mut self, note: &str, look: bool, basis: &'static str) -> VecDeque<ProviderInput> {
        let mut queue = VecDeque::new();
        self.snapshot = commit_written_turn(
            &mut self.owner,
            &mut self.reply,
            &mut queue,
            note,
            look,
            basis,
            1,
            PlayoutStart::default(),
            &mut self.trace,
            |state| self.speaker.publish_speaker_snapshot(state),
        )
        .unwrap();
        queue
    }
}

#[test]
fn reminder_waits_for_promised_look_after_final_pcm_and_delayed_provider_terminal() {
    // On the stepped scenario clock, as `reminder_roundtrip` (#935): the
    // turn's input evidence lasts 500 ms, and on the real clock a busy
    // host's SQLite commits in `fixture.offer()` outlasted it, so the turn was
    // revoked (`StaleOwner`, #1569). Taken first, so it is dropped last.
    let clock = crate::scenario_clock::Scope::enter(Duration::from_secs(24)).unwrap();
    clock.set_pump(|_| Ok(())).unwrap();
    let mut fixture = RoundtripFixture::new();
    let mut test = Test::new();
    // The preceding answer is ordinary, not a reminder quoting stored words.
    test.reply.as_mut().unwrap().reminder = None;
    test.mask = PlaybackMask::new(TurnTaking::V1Baseline);
    let previous = test.reply.as_ref().unwrap().owner;
    let mut opening = None;
    let mut wanted = None;
    let mut second_look = crate::second_look::SecondLook::default();
    record_look_intent(
        true,
        Some(previous.turn()),
        "Let me take a look.",
        None,
        &mut opening,
        &mut wanted,
        &mut test.trace,
    )
    .unwrap();
    assert_eq!(wanted.map(|(turn, _)| turn), Some(previous.turn()));

    let chunk = test.generation(17);
    test.accept(&chunk, 240);
    let drained_at = monotonic_us();
    test.event(WorkerEvent::SpeechDrained {
        owner: previous,
        sequence: chunk.chunk_sequence,
        last_sample: PlaybackCursor {
            epoch: 1,
            frame: test.cursor,
        },
        observed_at_us: drained_at,
    })
    .unwrap();
    test.retire(&chunk);
    assert!(!test.reply.as_ref().unwrap().provider_idle);
    assert!(!test.finish_if_settled());
    // A delayed provider terminal can arrive after the real mask's tail has
    // expired. Final speaker retirement alone therefore cannot protect the
    // promised look from a prepared reminder.
    let tail_ends = monotonic_us() + crate::turn_taking::TAIL_US + 1_000;
    while monotonic_us() < tail_ends {
        crate::scenario_clock::step(crate::scenario_clock::Phase::Run).unwrap();
    }
    assert!(test.mask.available(monotonic_us()));
    let offer = fixture.offer();
    test.renew_input_evidence();
    test.provider_complete();
    assert!(test.finish_if_settled());
    test.trace.receipts.completed(previous).unwrap();
    assert!(test.snapshot.listening_ready(now()));
    assert!(test.pending.is_none());
    assert!(fixture.parent().offer(monotonic_us()).is_some());

    assert!(
        idle_reminder_offer(
            fixture.parent(),
            &test.reply,
            wanted,
            &mut second_look,
            monotonic_us(),
        )
        .is_none(),
        "the promised camera action owns the next opportunity"
    );
    assert!(test.snapshot.owner().is_none());
    assert_eq!(wanted.take().unwrap().0, previous.turn());
    let queue = test.followup(crate::speak_up::LOOK_NOTE, true, "lamp_looks");
    let following = test.reply.as_ref().unwrap().owner;
    assert_ne!(following, previous);
    assert_eq!(queue.len(), 1);
    assert!(matches!(queue.front(), Some(ProviderInput::Written {
        request, text, look: true, ..
    }) if *request == following.turn() && text == crate::speak_up::LOOK_NOTE));
    assert!(
        idle_reminder_offer(
            fixture.parent(),
            &test.reply,
            wanted,
            &mut second_look,
            monotonic_us(),
        )
        .is_none()
    );
    assert_eq!(
        fixture.parent().offer(monotonic_us()).unwrap().token,
        offer.token
    );
    assert_eq!(fixture.settle_parent(), 0);
    assert!(fixture.ack_requests().is_empty());
}

#[test]
fn reminder_waits_for_named_request_second_look_before_its_window_can_expire() {
    // On the scenario clock, as its sibling that renews the same heartbeat
    // (`reminder_waits_for_promised_look_after_final_pcm_and_delayed_provider_terminal`)
    // is: the one input heartbeat (`renew_input_evidence`) lasts the owner's
    // whole input lease, `MAX_INPUT_LEASE_US` (1 s), of stepped time. On the
    // host's clock a stall of over 1 s before the follow-up's completion
    // revoked the turn, so that completion said `StaleOwner` (#1569).
    let clock = crate::scenario_clock::Scope::enter(Duration::from_secs(24)).unwrap();
    clock.set_pump(|_| Ok(())).unwrap();
    let mut fixture = RoundtripFixture::new();
    let mut test = Test::new();
    test.reply.as_mut().unwrap().reminder = None;
    let previous = test.reply.as_ref().unwrap().owner;
    let mut second_look = crate::second_look::SecondLook::default();
    let began = monotonic_us();
    second_look.name_heard(began, 1.0);
    second_look.request_began(previous.turn(), began);
    second_look.speech_ended(previous.turn(), monotonic_us());
    let offer = fixture.offer();
    test.renew_input_evidence();
    test.provider_complete();
    assert!(!test.reply.as_ref().unwrap().had_audio);
    assert!(test.finish_if_settled());
    test.trace.receipts.completed(previous).unwrap();
    let finished = monotonic_us();
    assert_eq!(
        second_look.finished(previous.turn(), true, finished),
        Some(1.0)
    );
    second_look.heard_on(1);
    assert!(test.snapshot.listening_ready(now()));
    assert!(test.mask.available(monotonic_us()));
    assert!(fixture.parent().offer(monotonic_us()).is_some());

    assert!(
        idle_reminder_offer(
            fixture.parent(),
            &test.reply,
            None,
            &mut second_look,
            monotonic_us(),
        )
        .is_none(),
        "a reminder cannot spend the unanswered call's second-look window"
    );
    assert_eq!(second_look.due(monotonic_us()), Some(previous.turn()));
    assert!(second_look.is_for(1));
    second_look.taken();
    let queue = test.followup(crate::second_look::NOTE, false, "lamp_looks_again");
    let following = test.reply.as_ref().unwrap().owner;
    assert_ne!(following, previous);
    assert_eq!(queue.len(), 1);
    assert!(matches!(queue.front(), Some(ProviderInput::Written {
        request, text, look: false, ..
    }) if *request == following.turn() && text == crate::second_look::NOTE));
    assert_eq!(
        second_look.due(finished + crate::second_look::WAIT_US + 1),
        None
    );
    assert!(
        idle_reminder_offer(
            fixture.parent(),
            &test.reply,
            None,
            &mut second_look,
            monotonic_us(),
        )
        .is_none()
    );
    // Once the owed follow-up actually finishes, the same prepared reminder
    // can be considered again; deferral does not consume it or strand it.
    test.reply
        .as_mut()
        .unwrap()
        .cloud
        .submitted(queue.front().unwrap())
        .unwrap();
    observe_provider_output(
        &mut CloudAvailability::default(),
        &mut test.reply,
        &ProviderOutput::Started {
            request: following.turn(),
            session: 1,
            waiting_for_barrier: false,
        },
    )
    .unwrap();
    test.provider_complete();
    assert!(test.finish_if_settled());
    test.trace.receipts.completed(following).unwrap();
    assert_eq!(
        idle_reminder_offer(
            fixture.parent(),
            &test.reply,
            None,
            &mut second_look,
            monotonic_us(),
        )
        .unwrap()
        .token,
        offer.token
    );
    assert_eq!(fixture.settle_parent(), 0);
    assert!(fixture.ack_requests().is_empty());
}

#[test]
fn whole_reminder_joins_two_real_retirements_and_successful_provider_terminal() {
    let clock = crate::scenario_clock::Scope::enter(Duration::from_secs(24)).unwrap();
    clock.set_pump(|_| Ok(())).unwrap();
    let mut test = Test::new();
    let answered = test.ask_skill();
    let first = test.generation(17);
    test.accept(&first, 120);
    assert!(test.pending.is_some());
    assert!(test.probe.settlement().is_none());
    assert!(
        answered.try_recv().is_err(),
        "partial acceptance is not spoken"
    );
    test.accept(&first, 120);
    assert!(test.pending.is_none());
    assert!(!test.finish_if_settled());
    test.retire(&first);
    assert!(
        !test.finish_if_settled(),
        "first generation is not the whole turn"
    );
    assert!(test.probe.settlement().is_none());
    let second = test.generation(23);
    test.provider_complete();
    test.accept(&second, 240);
    assert!(
        !test.finish_if_settled(),
        "accepted final PCM still needs retirement"
    );
    assert!(
        answered.try_recv().is_err(),
        "accepted PCM is not retired PCM"
    );
    test.retire(&second);
    assert!(test.finish_if_settled());
    assert_eq!(answered.try_recv(), Ok(Answer::Done(Outcome::Spoken)));
    assert!(
        answered.try_recv().is_err(),
        "one settlement answers the ticket once"
    );
    let Some(ToWorker::Complete { proof, token }) = test.probe.settlement() else {
        panic!("missing completion");
    };
    assert_eq!(proof.retired_generations, 2);
    assert_eq!(proof.final_frame, 480);
    assert_eq!(proof.final_epoch, 1);
    assert_eq!(proof.correlation, token.correlation);
    assert_eq!(proof.owner_boot, first.permit.owner().boot().bytes());
    assert_eq!(proof.turn, first.permit.owner().turn());
}

/// Final retirement and the candidate's last onset block arrive together on
/// real IPC. The production loop, not this test, decides which completes first.
/// Time is the stepped scenario clock; this establishes no acoustic latency.
#[test]
fn a_settled_reminder_finishes_before_the_live_loop_admits_its_successor() {
    use super::provider_terminal::{ReminderBoundary, reminder_admission};
    use crate::say::{Answer, Outcome};

    let run = reminder_admission(ReminderBoundary::Complete);
    let ToWorker::Complete { proof, token } = run.settlement else {
        panic!("the fully retired reminder was abandoned at successor admission");
    };
    assert_eq!(proof.correlation, token.correlation);
    assert_eq!(proof.turn, 1);
    assert_eq!(proof.retired_generations, 1);
    assert_eq!((proof.final_epoch, proof.final_frame), (1, 480));
    assert!(matches!(
        run.answers.as_slice(),
        [Answer::Begun(_), Answer::Done(Outcome::Spoken)]
    ));
    assert_eq!(
        run.starts,
        vec![(2, false)],
        "a finished reply is no cut-in"
    );

    let old_finished: Vec<_> = run
        .trace
        .iter()
        .filter(|event| event["kind"] == "turn_finished" && event["turn"] == 1)
        .collect();
    assert_eq!(old_finished.len(), 1);
    assert_eq!(old_finished[0]["outcome"], "audio_written_unscored");
    let finished_at = old_finished[0]["at_us"].as_u64().unwrap();
    let candidates: Vec<_> = run
        .trace
        .iter()
        .filter(|event| event["kind"] == "input_candidate")
        .collect();
    assert_eq!(candidates.len(), 1);
    assert_eq!(
        candidates[0]["candidate"]["trigger_sequence"],
        u64::from(crate::activity::START_BLOCKS)
    );
    assert_eq!(
        candidates[0]["candidate"]["trigger_read_at_us"],
        run.onset_at_us
    );
    assert_eq!(run.retirement_sent_at_us, run.onset_at_us);
    // A peer pump emits the receipts, then the clock advances one step before
    // the coordinator runs. Compare coordinator events to each other rather
    // than mistaking the synthetic capture's read time for processing time.
    assert_eq!(
        finished_at,
        candidates[0]["at_us"].as_u64().unwrap(),
        "same pass joins delivery"
    );
    let admitted: Vec<_> = run
        .trace
        .iter()
        .filter(|event| event["kind"] == "input_admitted")
        .collect();
    assert_eq!(admitted.len(), 1);
    assert_eq!(admitted[0]["turn"], 2);
    let admitted_at = admitted[0]["at_us"].as_u64().unwrap();
    let delay = admitted_at.checked_sub(run.onset_at_us).unwrap();
    assert!(
        (1..=12_000).contains(&delay),
        "next quiet capture opportunity took {delay} us"
    );
    let completion_to_admission = admitted_at.checked_sub(finished_at).unwrap();
    assert!((1..=10_000).contains(&completion_to_admission));
    eprintln!(
        "scenario capture onset read to successor admission: {delay} us; reminder completion to successor admission: {completion_to_admission} us"
    );

    // All first words survive, in order and once. Only six frames were voiced;
    // the seventh is quiet, so progress cannot depend on more voiced input.
    let onset = usize::from(crate::activity::START_BLOCKS);
    assert!(run.audio.len() > onset);
    assert_eq!(
        &run.audio[..onset + 1],
        &(1..=onset as u64 + 1)
            .map(|n| (n, n as i16))
            .collect::<Vec<_>>()
    );
    assert!(
        run.audio
            .windows(2)
            .all(|frames| frames[0].0 + 1 == frames[1].0 && frames[0].1 + 1 == frames[1].1)
    );
    let ended = run
        .trace
        .iter()
        .position(|event| event["kind"] == "turn_finished" && event["turn"] == 1)
        .unwrap();
    let began = run
        .trace
        .iter()
        .position(|event| event["kind"] == "input_admitted")
        .unwrap();
    assert!(ended < began);
}

#[test]
fn an_interruption_after_reminder_retirement_stays_held_without_ack_or_retry() {
    use super::provider_terminal::{ReminderBoundary, reminder_admission};
    // The sole generation's final retirement and onset arrive in the same
    // pump, but no provider terminal join authorizes a completed receipt.
    let run = reminder_admission(ReminderBoundary::NoTerminal);
    assert_eq!(run.starts.len(), 1, "real successor admitted");
    assert_eq!(run.starts[0].0, 2);
    assert_eq!(run.audio[0], (1, 1), "successor's first words kept");
    assert_eq!(run.retirement_sent_at_us, run.onset_at_us);
    assert!(
        matches!(run.settlement, ToWorker::Abandon { .. }),
        "a reminder with its own retired audio must stay held, not retry or Ack"
    );
    assert!(matches!(run.answers.as_slice(),
        [Answer::Begun(_), Answer::Done(Outcome::Dropped(reason))]
            if reason == "user_interrupted"));
}

#[test]
fn a_user_interrupted_reminder_is_retired_for_retry_after_successor_admission() {
    use super::provider_terminal::{ReminderBoundary, reminder_admission};
    let boundary = ReminderBoundary::Unretired;
    let run = reminder_admission(boundary);
    assert_eq!(run.starts.len(), 1, "{boundary:?}: real successor admitted");
    assert_eq!(run.starts[0].0, 2);
    assert_eq!(run.audio[0], (1, 1), "{boundary:?}: first words kept");
    let ToWorker::Retire { token, proof } = run.settlement else {
        panic!("{boundary:?}: known user interruption must retry after fencing");
    };
    assert_eq!(proof.correlation, token.correlation);
    assert!(matches!(
        proof.basis,
        lamp_schedule::delivery::RetirementBasis::OwnerFenced { turn: 1, .. }
    ));
    assert!(matches!(run.answers.as_slice(),
        [Answer::Begun(_), Answer::Done(Outcome::Dropped(reason))]
            if reason == "user_interrupted"));
    assert!(
        !run.trace
            .iter()
            .any(|event| event["kind"] == "turn_finished"
                && event["turn"] == 1
                && event["outcome"] == "completed")
    );
}

#[test]
fn the_live_reminder_boundary_keeps_incomplete_failed_reset_and_private_delivery_unacknowledged() {
    use super::provider_terminal::{ReminderBoundary, reminder_admission};
    use crate::say::{Answer, Outcome};

    for boundary in [
        ReminderBoundary::Failed,
        ReminderBoundary::NoTerminal,
        ReminderBoundary::Unretired,
        ReminderBoundary::ClockReset,
        ReminderBoundary::Privacy,
    ] {
        let run = reminder_admission(boundary);
        let retries = matches!(boundary, ReminderBoundary::Unretired);
        assert!(
            if retries {
                matches!(run.settlement, ToWorker::Retire { .. })
            } else {
                matches!(run.settlement, ToWorker::Abandon { .. })
            },
            "{boundary:?}"
        );
        assert!(
            matches!(
                run.answers.as_slice(),
                [Answer::Begun(_), Answer::Done(Outcome::Dropped(_))]
            ),
            "{boundary:?}"
        );
        let old_finished: Vec<_> = run
            .trace
            .iter()
            .filter(|event| event["kind"] == "turn_finished" && event["turn"] == 1)
            .collect();
        assert_eq!(old_finished.len(), 1, "{boundary:?}");
        if boundary == ReminderBoundary::Privacy {
            assert!(run.starts.is_empty());
            assert!(run.audio.is_empty());
            assert!(
                !run.trace
                    .iter()
                    .any(|event| event["kind"] == "input_admitted")
            );
        } else {
            assert_eq!(run.starts.len(), 1, "{boundary:?}");
            assert_eq!(run.starts[0].0, 2);
            assert_eq!(run.audio[0], (1, 1), "{boundary:?}: no lost prefix");
        }
        if boundary == ReminderBoundary::Unretired {
            // The fixture delivers the exact old retirement after it has seen
            // the successor Start. It cannot revive the retired receipt or
            // emit another completion for the old owner.
            assert_eq!(old_finished[0]["outcome"], "user_interrupted");
            let admitted_at = run
                .trace
                .iter()
                .find(|event| event["kind"] == "input_admitted")
                .unwrap()["at_us"]
                .as_u64()
                .unwrap();
            assert!(
                run.retirement_sent_at_us > admitted_at,
                "the old marker really arrived after successor admission"
            );
            assert!(
                !run.trace
                    .iter()
                    .any(|event| event["kind"] == "speech_final_sample_retired"
                        && event["turn"] == 1)
            );
        }
    }
}

/// Fourth on #1038: while a reminder is held for its skill, and is still
/// the one offered, the say service, the hello and the news wait for it;
/// one said or let go, or another due, holds nothing.
#[test]
fn a_reminder_held_for_its_skill_keeps_other_unasked_speech_waiting_while_it_is_offered() {
    let mut fixture = RoundtripFixture::new();
    let offer = fixture.offer();
    let key = reminder_key(&offer);
    assert!(reminder_waits(&Some((key.clone(), 0)), Some(&offer)));
    assert!(!reminder_waits(&None, Some(&offer)));
    assert!(!reminder_waits(&Some((key, 0)), None), "no longer offered");
    assert!(!reminder_waits(
        &Some(("0123/9/1".to_owned(), 0)),
        Some(&offer)
    ));
}

/// #939, (c): a reminder a skill asked for carries the ask's ticket on its
/// turn: `begun` as the turn is committed, and `spoken` on the very proof
/// the reminder's acknowledgement takes. Cut off, the reminder is
/// abandoned and the skill told `dropped`.
#[test]
fn a_reminder_a_skill_asked_for_is_answered_on_the_acknowledgements_own_proof() {
    use crate::say::{Answer, Outcome, ticket_for_test};
    let mut test = Test::new();
    let (ticket, answered) = ticket_for_test("reminders");
    begun_for(Some(ticket), &mut test.reply);
    assert!(matches!(answered.try_recv(), Ok(Answer::Begun(_))));
    let first = test.generation(17);
    test.accept(&first, 240);
    test.provider_complete();
    assert!(!test.finish_if_settled(), "its sound is still playing");
    assert!(answered.try_recv().is_err(), "not before the turn settles");
    test.retire(&first);
    assert!(test.finish_if_settled());
    assert!(matches!(
        test.probe.settlement(),
        Some(ToWorker::Complete { .. })
    ));
    assert_eq!(answered.try_recv(), Ok(Answer::Done(Outcome::Spoken)));

    let mut test = Test::new();
    let (ticket, answered) = ticket_for_test("reminders");
    begun_for(Some(ticket), &mut test.reply);
    let _begun = answered.try_recv();
    let first = test.generation(17);
    test.accept(&first, 240);
    cancel_reply(
        &mut test.owner,
        &mut test.reply,
        &mut test.trace,
        "user_interrupted",
    )
    .unwrap();
    assert!(matches!(
        test.probe.settlement(),
        Some(ToWorker::Abandon { .. })
    ));
    assert!(matches!(
        answered.try_recv(),
        Ok(Answer::Done(Outcome::Dropped(_)))
    ));
}

#[test]
fn interruption_after_first_retirement_never_acknowledges_later_audio() {
    for second_started in [false, true] {
        let mut test = Test::new();
        let first = test.generation(17);
        test.accept(&first, 240);
        test.retire(&first);
        let second = test.generation(23);
        if second_started {
            test.accept(&second, 240);
        }
        cancel_reply(
            &mut test.owner,
            &mut test.reply,
            &mut test.trace,
            "user_interrupted",
        )
        .unwrap();
        assert!(matches!(
            test.probe.settlement(),
            Some(ToWorker::Abandon { .. })
        ));
        // A late exact old retirement is ignored by the real handler after
        // cancellation; it cannot revive a detached reminder receipt.
        test.event(WorkerEvent::SpeechRetired {
            owner: second.permit.owner(),
            sequence: second.chunk_sequence,
            final_sample: PlaybackCursor {
                epoch: 1,
                frame: 480,
            },
            observed_at_us: monotonic_us(),
        })
        .unwrap();
        assert!(matches!(
            test.probe.settlement(),
            Some(ToWorker::Abandon { .. })
        ));
    }
}

#[test]
fn no_audio_or_provider_failure_stays_pending_and_selects_no_failure_recording() {
    let clock = crate::scenario_clock::Scope::enter(Duration::from_secs(24)).unwrap();
    clock.set_pump(|_| Ok(())).unwrap();
    for had_audio in [false, true] {
        for failed in [false, true] {
            let mut test = Test::new();
            let answered = test.ask_skill();
            if had_audio {
                let chunk = test.generation(17);
                test.accept(&chunk, 240);
                test.retire(&chunk);
            }
            if failed {
                let request = test.reply.as_ref().unwrap().owner.turn();
                observe_provider_output(
                    &mut CloudAvailability::default(),
                    &mut test.reply,
                    &ProviderOutput::TurnFailed {
                        request,
                        stage: crate::provider_worker::FailedStage::AwaitingResponse,
                        reason: crate::provider_worker::Fault::Transport,
                    },
                )
                .unwrap();
            } else {
                test.provider_complete();
            }
            select_failure_notice(
                &FailureNotices::synthetic(),
                &mut test.reply,
                &test.pending,
                &mut test.trace,
            )
            .unwrap();
            assert!(matches!(
                test.reply.as_ref().unwrap().notice,
                NoticeDelivery::Unselected
            ));
            assert!(test.finish_if_settled());
            if had_audio && !failed {
                assert!(matches!(
                    test.probe.settlement(),
                    Some(ToWorker::Complete { .. })
                ));
                assert_eq!(answered.try_recv(), Ok(Answer::Done(Outcome::Spoken)));
            } else {
                assert!(matches!(
                    test.probe.settlement(),
                    Some(ToWorker::Abandon { .. })
                ));
                let reason = if failed {
                    "provider_failed"
                } else {
                    "no_audio_answer"
                };
                assert_eq!(
                    answered.try_recv(),
                    Ok(Answer::Done(Outcome::Dropped(reason.into())))
                );
            }
        }
    }
}

#[test]
fn reset_or_playback_gap_cannot_be_relabeled_whole_reminder_completion() {
    let clock = crate::scenario_clock::Scope::enter(Duration::from_secs(24)).unwrap();
    clock.set_pump(|_| Ok(())).unwrap();
    for reset in [false, true] {
        let mut test = Test::new();
        let answered = test.ask_skill();
        let chunk = test.generation(17);
        test.accept(&chunk, 240);
        test.retire(&chunk);
        if reset {
            test.event(WorkerEvent::PlaybackClockReset {
                previous: QueuedRange {
                    epoch: 1,
                    retired_through: 240,
                    accepted_through: 240,
                },
                next_epoch: 2,
                observed_at_us: monotonic_us(),
            })
            .unwrap();
        } else {
            test.reply.as_mut().unwrap().playback_gaps = 1;
        }
        assert!(answered.try_recv().is_err());
        test.provider_complete();
        assert!(test.finish_if_settled());
        assert!(matches!(
            test.probe.settlement(),
            Some(ToWorker::Abandon { .. })
        ));
        let reason = if reset {
            "reminder_completion_unconfirmed"
        } else {
            "audio_written_with_playback_gaps_unscored"
        };
        assert_eq!(
            answered.try_recv(),
            Ok(Answer::Done(Outcome::Dropped(reason.into())))
        );
        assert!(answered.try_recv().is_err());
    }
}

#[test]
fn privacy_revocation_after_partial_reminder_playback_drops_the_skill_ticket() {
    let clock = crate::scenario_clock::Scope::enter(Duration::from_secs(24)).unwrap();
    clock.set_pump(|_| Ok(())).unwrap();
    let mut test = Test::new();
    let answered = test.ask_skill();
    let first = test.generation(17);
    test.accept(&first, 240);
    test.retire(&first);
    let second = test.generation(23);
    test.accept(&second, 120);
    test.provider_complete();
    assert!(!test.finish_if_settled());
    assert!(answered.try_recv().is_err());
    test.owner
        .set_microphone_permission(now(), Permission::Denied)
        .unwrap();
    let current = test.reply.take().unwrap();
    record_cancelled_reply(current, &mut test.trace, "physical_mute", monotonic_us()).unwrap();
    test.snapshot = test.owner.snapshot(now()).unwrap();
    test.speaker
        .publish_speaker_snapshot(test.snapshot)
        .unwrap();
    assert_eq!(test.snapshot.microphone_permission(), Permission::Denied);
    assert!(test.snapshot.owner().is_none());
    assert!(matches!(
        test.probe.settlement(),
        Some(ToWorker::Abandon { .. })
    ));
    assert_eq!(
        answered.try_recv(),
        Ok(Answer::Done(Outcome::Dropped("physical_mute".into())))
    );
    // The old final retirement cannot revise either of those outcomes.
    test.retire(&second);
    assert!(matches!(
        test.probe.settlement(),
        Some(ToWorker::Abandon { .. })
    ));
    assert!(answered.try_recv().is_err());
}

#[test]
fn only_a_fenced_attempt_without_any_provider_pcm_can_retry() {
    for audio in [false, true] {
        let mut test = Test::new();
        if audio {
            let _ = test.generation(17);
        }
        let mut receipt = test.reply.as_mut().unwrap().reminder.take().unwrap();
        cancel_reply(
            &mut test.owner,
            &mut test.reply,
            &mut test.trace,
            "reminder_deferred",
        )
        .unwrap();
        let snapshot = test.owner.snapshot(now()).unwrap();
        test.speaker.publish_speaker_snapshot(snapshot).unwrap();
        receipt.deferred_before_audio();
        drop(receipt);
        if audio {
            assert!(matches!(
                test.probe.settlement(),
                Some(ToWorker::Abandon { .. })
            ));
        } else {
            assert!(matches!(
                test.probe.settlement(),
                Some(ToWorker::Retire { .. })
            ));
        }
    }
}

/// #926: a turn held as unclear is asked again as soon as it is over, by a
/// written turn of the lamp's own with the note to say only "Sorry?"; not
/// while it is going on, and not once someone else spoke or five seconds
/// went by.
#[test]
fn an_unclear_turn_is_asked_again_once_over_and_never_late_or_over_someone() {
    let at = 10_000_000;
    assert_eq!(ask_again_now((7, at), Some(7), at + 100), AskAgain::Wait);
    assert_eq!(ask_again_now((7, at), Some(8), at + 100), AskAgain::Drop);
    assert_eq!(ask_again_now((7, at), None, at + 100), AskAgain::Due);
    assert_eq!(
        ask_again_now((7, at), None, at + ASK_AGAIN_WITHIN_US + 1),
        AskAgain::Drop
    );
    // The unclear turn staying open is let go, not waited on for ever.
    assert_eq!(
        ask_again_now((7, at), Some(7), at + ASK_AGAIN_TURN_LONGEST_US + 1),
        AskAgain::Drop
    );
    // The note ends what was being said, and asks only "Sorry?" (lamp-4ace
    // 09:42: a softer one let the model go on with its story).
    for must in ["do not go on with it", "Sorry?", "Then stop and listen"] {
        assert!(crate::speak_up::ASK_AGAIN_NOTE.contains(must), "{must}");
    }
    let mut test = Test::new();
    let queue = test.followup(crate::speak_up::ASK_AGAIN_NOTE, false, "asked_again");
    let asking = test.reply.as_ref().unwrap().owner;
    assert!(matches!(queue.front(), Some(ProviderInput::Written {
        request, text, look: false, ..
    }) if *request == asking.turn() && text == crate::speak_up::ASK_AGAIN_NOTE));
}

/// #1417, the light row's one miss (lamp-4ace, 17 Oct 16:26): "make your
/// light a bit cooler" heard as Hindi ended `unclear` (4.68 s), the lamp's
/// own "Sorry?" was admitted (4.69 s) and ended `no_audio_answer` (8.70 s),
/// and the person got silence. With `LAMP_UNCLEAR_HUM=on` that empty
/// "Sorry?" owes one "hm?"; a "Sorry?" the model said, and a silent turn
/// that was not the lamp's asking, owe none.
#[test]
fn an_empty_sorry_owes_one_hm_and_a_said_one_or_another_silent_turn_none() {
    assert!(crate::head_cue::unclear(Some("on")));
    for off in [None, Some(""), Some("off"), Some("1")] {
        assert!(!crate::head_cue::unclear(off), "{off:?}");
    }
    // The transcript's two ends: the unclear turn, then the empty "Sorry?".
    let mut hum = UnclearHum::default();
    assert!(!hum.finished(7, true, 4_680_000), "the unclear turn itself");
    assert_eq!(hum.owed(), None);
    hum.asked(8);
    assert!(hum.finished(8, true, 8_700_000), "the empty Sorry?");
    assert_eq!(hum.owed(), Some(8_700_000));
    assert_eq!(hum.due(None, 8_720_000), HumDue::Now);
    // It plays through the tap's own rule: a quiet lamp, allowed to listen.
    let now = crate::head_cue::Now {
        at_us: 8_720_000,
        began_us: 8_700_000,
        allowed: true,
        quiet: true,
        listening_ready: true,
        reply_owed: false,
        last_said_us: None,
        last_hum_us: None,
    };
    assert_eq!(crate::head_cue::may_hum(&now), Ok(()));
    // Not over a mute, and not twice in 30 s with a tap's.
    let muted = crate::head_cue::Now {
        allowed: false,
        ..now
    };
    assert_eq!(
        crate::head_cue::may_hum(&muted),
        Err("microphone_not_allowed")
    );
    let lately = crate::head_cue::Now {
        last_hum_us: Some(8_000_000),
        ..now
    };
    assert_eq!(crate::head_cue::may_hum(&lately), Err("hummed_recently"));
    hum.done();
    assert_eq!(hum.owed(), None);

    // A "Sorry?" the model said: the answered turn is unchanged.
    let mut hum = UnclearHum::default();
    hum.asked(8);
    assert!(!hum.finished(8, false, 8_700_000));
    assert_eq!(hum.owed(), None);
    // A silent turn that was not the lamp's asking (the room's talk).
    let mut hum = UnclearHum::default();
    assert!(!hum.finished(9, true, 9_000_000));
    assert_eq!(hum.owed(), None);
    // A "Sorry?" cancelled without finishing leaves no watch on a later turn.
    let mut hum = UnclearHum::default();
    hum.asked(8);
    assert!(!hum.finished(9, true, 9_000_000));
    assert!(!hum.finished(8, true, 9_500_000));
    assert_eq!(hum.owed(), None);

    // Someone speaking first drops it; so does waiting past 2 s for quiet.
    let mut hum = UnclearHum::default();
    hum.asked(8);
    hum.finished(8, true, 8_700_000);
    assert_eq!(
        hum.due(Some(9), 8_800_000),
        HumDue::Drop("another_turn_began")
    );
    assert_eq!(
        hum.due(None, 8_700_000 + UNCLEAR_HUM_WITHIN_US + 1),
        HumDue::Drop("late")
    );
    assert_eq!(
        hum.due(None, 8_700_000 + UNCLEAR_HUM_WITHIN_US),
        HumDue::Now
    );
    // What held it is kept for the drop's row (the Seventh's P3 on #1421).
    hum.hold("listening_not_ready");
    assert_eq!(hum.held(), Some("listening_not_ready"));
    // A Stop drops an owed hum, and the watch, saying when it was owed.
    assert_eq!(hum.cancelled(), Some(8_700_000));
    assert_eq!((hum.owed(), hum.held()), (None, None));
    let mut hum = UnclearHum::default();
    hum.asked(8);
    assert_eq!(hum.cancelled(), None);
    assert!(!hum.finished(8, true, 9_000_000), "no watch after a Stop");
}

/// #941, the first builder on #1146: a turn the lamp wrote itself (its own
/// look, a hello, a "Sorry?") takes no still from "Let me take a look.":
/// pictures never chain with nobody speaking. A turn someone spoke still does.
#[test]
fn a_lamp_written_turn_s_phrase_asks_for_no_still() {
    for (note, look) in [
        (crate::speak_up::LOOK_FULL_NOTE, true),
        (crate::speak_up::ASK_AGAIN_NOTE, false),
    ] {
        let mut test = Test::new();
        test.followup(note, look, "lamp_looks");
        let written = test.reply.as_ref().unwrap().owner.turn();
        let (mut opening, mut wanted) = (None, None);
        record_look_intent(
            true,
            Some(written),
            "Let me take a look.",
            None,
            &mut opening,
            &mut wanted,
            &mut test.trace,
        )
        .unwrap();
        assert_eq!(wanted, None, "{note}");
        record_look_intent(
            true,
            Some(written + 1),
            "Let me take a look.",
            None,
            &mut opening,
            &mut wanted,
            &mut test.trace,
        )
        .unwrap();
        assert_eq!(wanted.map(|(turn, _)| turn), Some(written + 1), "{note}");
    }
}

/// The reviewer's 23:47 P2 on #1172: a new service cycle (a mute) numbers its
/// turns from 1 again, so the turn the lamp wrote last cycle is not its own
/// now: the same number spoken asks for the still.
#[test]
fn after_a_new_cycle_the_last_cycle_s_written_turn_number_spoken_asks_for_the_still() {
    let mut test = Test::new();
    test.followup(crate::speak_up::LOOK_FULL_NOTE, true, "lamp_looks");
    let written = test.reply.as_ref().unwrap().owner.turn();
    // Through the service cycle's own start (`run_inner`), which stops at a
    // head with no camera right after it makes the cycle's turn owner.
    let motor = crate::options::MotorOptions {
        worker: "absent-motor-worker".into(),
        config: "absent-motor-config".into(),
        head: Some(crate::options::HeadOptions {
            plan: "absent-plan".into(),
            channel: "absent-channel".into(),
        }),
    };
    let error = run_inner(
        ProviderSource::Fixture {
            wav: Path::new("absent.wav"),
            sha256: "",
            at_ms: None,
        },
        Lifetime::Serve,
        &mut SessionState::default(),
        None,
        crate::options::DirectedOptions::default(),
        None,
        Some(&motor),
        &mut true,
        &FailureNotices::none(),
        &mut None,
        &Termination::inactive(),
        &mut None,
        &mut None,
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("requires prepared camera"),
        "{error}"
    );
    let (mut opening, mut wanted) = (None, None);
    record_look_intent(
        true,
        Some(written),
        "Let me take a look.",
        None,
        &mut opening,
        &mut wanted,
        &mut test.trace,
    )
    .unwrap();
    assert_eq!(wanted.map(|(turn, _)| turn), Some(written));
}

/// #926, the reviewer: an Unclear consumed before a hand's Stop is dropped
/// with its turn, and one arriving after it (its turn gone) is never taken;
/// only one for the turn in hand is.
#[test]
fn a_stop_ends_the_asking_and_an_unclear_after_it_is_never_taken() {
    let mut asking = AskAgainState::default();
    assert!(asking.unclear(7, Some(7), 100, false));
    assert_eq!(asking.wanted(), Some((7, 100)));
    assert_eq!(asking.cancelled(), Some(7));
    assert_eq!(asking.wanted(), None);
    // After the Stop: no turn in hand, or another one.
    assert!(!asking.unclear(7, None, 200, false));
    assert!(!asking.unclear(7, Some(8), 200, false));
    assert_eq!(asking.wanted(), None);
    assert_eq!(asking.cancelled(), None);
}

/// #1545: the muted wait as the coordinator runs it, with the real consumer
/// offering a due reminder at each look and the wait's own health on a bus.
/// `between` runs before each look, as the consumer's ticks come between
/// them. Returns the trace's kinds and the health reports, in order.
fn muted_with_a_due_reminder(
    fixture: &mut RoundtripFixture,
    looks: usize,
    between: impl FnMut(&mut RoundtripFixture, usize),
) -> (Vec<String>, Vec<(lamp_msgs::Level, String, bool)>) {
    waiting_with_a_due_reminder(
        fixture,
        Some(crate::privacy::PrivacyState::Muted),
        looks,
        between,
    )
}

/// The same, with the switch read as `switch` (`None`: not read yet, the
/// wait's first looks at every start of the voice).
fn waiting_with_a_due_reminder(
    fixture: &mut RoundtripFixture,
    switch: Option<crate::privacy::PrivacyState>,
    looks: usize,
    mut between: impl FnMut(&mut RoundtripFixture, usize),
) -> (Vec<String>, Vec<(lamp_msgs::Level, String, bool)>) {
    use crate::service_privacy::WaitOutcome;
    use lamp_msgs::Diagnostic;
    // Named by a count, not the clock: two tests reached here in the same
    // microsecond, shared one bus and heard each other's reports (#1569).
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let root = std::path::PathBuf::from(format!(
        "/tmp/lvo-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let bus = lamp_bus::Bus::new(&root).unwrap();
    let mut hears = bus
        .subscribe_typed(Diagnostic::TOPIC, "test", Diagnostic::TYPE)
        .unwrap();
    let reminders = RefCell::new(Some(fixture.lend_parent()));
    let controls = RefCell::new(None);
    let mut trace = SessionState::default();
    let outcome = muted_wait(
        &mut trace,
        &mut None,
        &controls,
        &reminders,
        None::<crate::ring_asleep::TapFace<fn() -> Option<crate::ring_asleep::Asleep>>>,
        Some(crate::bus_out::WaitHealth::on(&bus).unwrap()),
        |on_state, gate| {
            if let Some(switch) = switch {
                on_state(switch, crate::process::new_boot().unwrap(), 1)?;
            }
            for look in 0..looks {
                between(fixture, look);
                gate(&mut |_| Ok(()))?;
            }
            Ok(WaitOutcome::Allowed)
        },
    )
    .unwrap();
    assert_eq!(outcome, WaitOutcome::Allowed);
    fixture.return_parent(reminders.into_inner().unwrap());
    let kinds = trace
        .iter()
        .map(|row| row["kind"].as_str().unwrap_or_default().to_owned())
        .collect();
    let mut said = Vec::new();
    while let Some(message) = hears.recv(Duration::from_millis(50)).unwrap() {
        let report = message.parse::<Diagnostic>().unwrap();
        let owed = report
            .values
            .iter()
            .any(|fact| fact.key == "reminder" && fact.value == "owed");
        said.push((report.level, report.message, owed));
    }
    let _ = std::fs::remove_dir_all(root);
    (kinds, said)
}

/// #1545 (the first builder's product call, 16:47): a reminder due while
/// the microphones are off waits, still offered. The trace says once that it
/// is owed, no turn is begun and nothing is settled, and the health says it
/// beside why the microphones are off. On lamp-cb94 on 17 October one came
/// due 21 minutes into a mute and nothing said it was owed.
#[test]
fn a_reminder_due_while_muted_is_owed_once_and_no_turn_begins() {
    let mut fixture = RoundtripFixture::new();
    let offer = fixture.offer();
    let (kinds, said) = muted_with_a_due_reminder(&mut fixture, 5, |fixture, _| fixture.tick());
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| *kind == "reminder_owed_while_muted")
            .count(),
        1,
        "{kinds:?}"
    );
    assert!(
        !kinds
            .iter()
            .any(|kind| kind.starts_with("input_") || kind.starts_with("provider_")),
        "a muted lamp begins no turn: {kinds:?}"
    );
    assert!(fixture.received().is_empty(), "nothing settled while muted");
    let owed = format!(
        "the microphones are off (the privacy switch); {}",
        crate::bus_out::OWED
    );
    assert_eq!(
        said.first(),
        Some(&(lamp_msgs::Level::Ok, owed, true)),
        "{said:?}"
    );
    // The wait ends on the look that read the switch open: the voice is
    // opening, and owes nothing while it does (the reviewer on #1551).
    assert_eq!(
        said.last(),
        Some(&(lamp_msgs::Level::Ok, String::new(), false)),
        "{said:?}"
    );
    // The same attempt in a later muted wait is not said again.
    let (kinds, _) = muted_with_a_due_reminder(&mut fixture, 3, |fixture, _| fixture.tick());
    assert!(
        !kinds.iter().any(|kind| kind == "reminder_owed_while_muted"),
        "{kinds:?}"
    );
    assert_eq!(
        fixture.parent().offer(monotonic_us()).unwrap().token,
        offer.token
    );
}

/// #1545: once the microphones open, the owed reminder is the first thing
/// the voice's own loop may say: the same attempt, still offered, nothing
/// settled, and the turn's gate takes it as soon as nothing else holds the
/// lamp. Its words are said as any late reminder's, with its lateness.
#[test]
fn on_unmute_the_owed_reminder_is_offered_to_the_first_turn() {
    let mut fixture = RoundtripFixture::new();
    let offer = fixture.offer();
    let _ = muted_with_a_due_reminder(&mut fixture, 4, |fixture, _| fixture.tick());
    fixture.tick();
    let mut second_look = crate::second_look::SecondLook::default();
    let first = idle_reminder_offer(
        fixture.parent(),
        &None,
        None,
        &mut second_look,
        monotonic_us(),
    )
    .expect("the owed reminder is offered to the first turn after unmute");
    assert_eq!(first.token, offer.token);
    assert_eq!(first.text, offer.text);
    assert!(fixture.received().is_empty());
    assert!(fixture.pending());
    let said = commit_owed(&mut fixture, first.clone());
    assert!(
        matches!(said, ProviderInput::Reminder { text, late, late_by_ms, .. }
        if text == first.text && late == first.late && late_by_ms == first.late_by_ms)
    );
}

/// The owed reminder taken as the first turn after the wait, as the voice's
/// loop takes it: what the provider is given.
fn commit_owed(fixture: &mut RoundtripFixture, offer: crate::reminders::Offer) -> ProviderInput {
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
    commit_reminder_turn(
        &mut owner,
        &mut reply,
        &mut queue,
        offer,
        fixture.parent(),
        1,
        PlayoutStart::default(),
        &mut trace,
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(queue.len(), 1);
    queue.pop_front().unwrap()
}

/// #1545: a reminder cancelled while the lamp is muted is no longer owed:
/// the health stops saying it at once, and after unmute nothing is offered.
#[test]
fn a_reminder_cancelled_while_muted_is_never_said() {
    let mut fixture = RoundtripFixture::new();
    fixture.offer();
    let (kinds, said) = muted_with_a_due_reminder(&mut fixture, 6, |fixture, look| {
        if look == 2 {
            fixture.cancel();
        }
        fixture.tick();
    });
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| *kind == "reminder_owed_while_muted")
            .count(),
        1,
        "{kinds:?}"
    );
    // The wait's last report is the opening one, never owed since #1565, so
    // it says nothing of the cancel: the health stopped saying it owed on
    // the wait's own last report, before that one (#1569).
    let (opening, waited) = said.split_last().expect("the wait reported");
    assert!(!opening.2, "{said:?}");
    let owed: Vec<bool> = waited.iter().map(|(_, _, owed)| *owed).collect();
    assert_eq!(owed.first(), Some(&true), "{said:?}");
    assert_eq!(owed.last(), Some(&false), "{said:?}");
    assert!(!fixture.pending());
    fixture.settle_parent();
    fixture.tick();
    let mut second_look = crate::second_look::SecondLook::default();
    assert!(
        idle_reminder_offer(
            fixture.parent(),
            &None,
            None,
            &mut second_look,
            monotonic_us()
        )
        .is_none()
    );
    assert!(
        !fixture
            .received()
            .iter()
            .any(|command| matches!(command, ToWorker::Complete { .. })),
        "{:?}",
        fixture.received()
    );
}

/// #1545, and the reviewer on #1551: a mute that lasts past an hour. The
/// reminder came due on time; while the lamp waits muted, Schedule's clock
/// moves on past `OVERDUE_MS`. It stays owed throughout. After unmute the
/// offer is late by over an hour, so the one-hour rule holds it for
/// someone at the lamp, and once it begins, the provider is given that
/// lateness with its words.
#[test]
fn a_mute_past_an_hour_leaves_the_reminder_to_the_one_hour_rule() {
    let mut fixture = RoundtripFixture::new();
    let on_time = fixture.offer();
    assert!(
        on_time
            .late_by_ms
            .is_some_and(|ms| ms < crate::reminders::OVERDUE_MS),
        "{on_time:?}"
    );
    let (kinds, said) = muted_with_a_due_reminder(&mut fixture, 6, |fixture, look| {
        if look == 3 {
            fixture.advance_wall(crate::reminders::OVERDUE_MS + 21 * 60_000);
        }
        fixture.tick();
    });
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| *kind == "reminder_owed_while_muted")
            .count(),
        1,
        "the same attempt, owed once: {kinds:?}"
    );
    assert!(
        said.iter().rev().skip(1).all(|(_, _, owed)| *owed),
        "{said:?}"
    );
    let late = fixture.offer();
    assert_eq!(late.token, on_time.token);
    assert!(late.late, "{late:?}");
    assert!(
        late.late_by_ms
            .is_some_and(|ms| ms >= crate::reminders::OVERDUE_MS),
        "{late:?}"
    );
    let mut wait = crate::reminders::OverdueWait::default();
    assert!(!wait.may_begin(&late, false), "not into an empty room");
    assert!(wait.may_begin(&late, true), "with someone at the lamp");
    let given = commit_owed(&mut fixture, late.clone());
    assert!(
        matches!(given, ProviderInput::Reminder { late: true, late_by_ms, .. }
        if late_by_ms == late.late_by_ms),
        "{given:?}"
    );
}

/// The reviewer on #1551: the wait also runs at every start of the voice,
/// while the switch is first read and once it has read open. A reminder
/// pending then is owed to no mute: no line, no trace row, and the health
/// says nothing of it. The person muting later in the same attempt still
/// gets the line.
#[test]
fn a_reminder_pending_at_a_start_is_owed_to_no_mute() {
    use crate::privacy::PrivacyState;
    let mut fixture = RoundtripFixture::new();
    fixture.offer();
    for switch in [None, Some(PrivacyState::Allowed)] {
        let (kinds, said) =
            waiting_with_a_due_reminder(&mut fixture, switch, 4, |fixture, _| fixture.tick());
        assert!(
            !kinds.iter().any(|kind| kind == "reminder_owed_while_muted"),
            "{switch:?}: {kinds:?}"
        );
        assert!(
            said.iter().all(|(_, _, owed)| !owed),
            "{switch:?}: {said:?}"
        );
    }
    let (kinds, _) = muted_with_a_due_reminder(&mut fixture, 3, |fixture, _| fixture.tick());
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| *kind == "reminder_owed_while_muted")
            .count(),
        1,
        "{kinds:?}"
    );
}

/// One muted wait of `looks` looks with the fixture's reminder, for #1545's
/// pulse: before each look, `between` may change the switch the wait reads
/// (`Unavailable` is read as one that can't be read at once) and act on
/// the fixture. After each, what the asleep face was told: since when the
/// reminder has been owed, while its pulse may show.
fn told_the_face(
    fixture: &mut RoundtripFixture,
    looks: usize,
    mut between: impl FnMut(&mut RoundtripFixture, usize) -> Option<crate::privacy::PrivacyState>,
) -> Vec<Option<Instant>> {
    use crate::service_privacy::WaitOutcome;
    let reminders = RefCell::new(Some(fixture.lend_parent()));
    let controls = RefCell::new(None);
    let mut trace = SessionState::default();
    let owed = std::sync::Arc::new(crate::ring_asleep::Owed::default());
    let mut told = Vec::new();
    muted_wait_pulsing(
        &mut trace,
        &mut None,
        &controls,
        &reminders,
        None::<crate::ring_asleep::TapFace<fn() -> Option<crate::ring_asleep::Asleep>>>,
        None::<crate::ring_asleep::SleepFace<fn() -> Option<crate::ring_asleep::Asleep>>>,
        None,
        Some(owed.clone()),
        None,
        |on_state, gate| {
            for look in 0..looks {
                if let Some(switch) = between(fixture, look) {
                    on_state(switch, crate::process::new_boot().unwrap(), 1)?;
                }
                fixture.tick();
                gate(&mut |_| Ok(()))?;
                told.push(owed.showing());
            }
            Ok(WaitOutcome::Allowed)
        },
    )
    .unwrap();
    fixture.return_parent(reminders.into_inner().unwrap());
    told
}

/// #1545's pulse (`LAMP_REMINDER_PULSE`): the muted wait tells the asleep
/// face a reminder is owed on each look while a mute holds the microphones,
/// with one time for the attempt; nothing while the switch is first read,
/// when nothing is offered, or once it is cancelled; and nothing while the
/// switch can't be read, a wait that is closed but does not pulse (the
/// reviewer and Thirteen on #1581).
#[test]
fn the_muted_wait_tells_the_asleep_face_a_reminder_is_owed_only_while_it_may_pulse() {
    use crate::privacy::PrivacyState::{Muted, Unavailable};
    let muted_at_first = |_: &mut RoundtripFixture, look: usize| (look == 0).then_some(Muted);
    // Muted with one offered: told on every look, the same time.
    let mut fixture = RoundtripFixture::new();
    let _offer = fixture.offer();
    let muted = told_the_face(&mut fixture, 4, muted_at_first);
    assert!(muted[0].is_some(), "{muted:?}");
    assert!(muted.iter().all(|since| *since == muted[0]), "{muted:?}");
    // The switch first read, not yet closed: nothing.
    let mut fixture = RoundtripFixture::new();
    let _offer = fixture.offer();
    let reading = told_the_face(&mut fixture, 4, |_, _| None);
    assert!(reading.iter().all(Option::is_none), "{reading:?}");
    // Muted with nothing offered (cancelled before the wait): nothing.
    let mut fixture = RoundtripFixture::new();
    fixture.cancel();
    let none = told_the_face(&mut fixture, 4, muted_at_first);
    assert!(none.iter().all(Option::is_none), "{none:?}");
    // Cancelled while muted: told, then not.
    let mut fixture = RoundtripFixture::new();
    let _offer = fixture.offer();
    let cancelled = told_the_face(&mut fixture, 6, |fixture, look| {
        if look == 2 {
            fixture.cancel();
        }
        (look == 0).then_some(Muted)
    });
    assert!(cancelled[0].is_some(), "{cancelled:?}");
    assert_eq!(cancelled.last(), Some(&None), "{cancelled:?}");
    // A switch that can't be read: closed and owed, but no pulse.
    let mut fixture = RoundtripFixture::new();
    let _offer = fixture.offer();
    let unread = told_the_face(&mut fixture, 4, |_, look| {
        (look == 0).then_some(Unavailable)
    });
    assert!(unread.iter().all(Option::is_none), "{unread:?}");
}

/// Thirteen and the reviewer on #1581: a pulse held back for a while (here
/// a switch that can't be read, mid-mute) keeps the attempt's time, so it
/// neither starts the half-hour again nor pulses at once when it may show.
#[test]
fn a_pulse_held_back_keeps_the_time_it_was_owed_from() {
    use crate::privacy::PrivacyState::{Muted, Unavailable};
    let mut fixture = RoundtripFixture::new();
    let _offer = fixture.offer();
    let told = told_the_face(&mut fixture, 6, |_, look| match look {
        0 | 4 => Some(Muted),
        2 => Some(Unavailable),
        _ => None,
    });
    let since = told[0];
    assert!(since.is_some(), "{told:?}");
    assert_eq!(told, [since, since, None, None, since, since]);
}

/// The reviewer on #1581: the pulse is for something owed under the switch
/// or a mute, never under the sleep a hand held on the head chose, even
/// with the switch closed too.
#[test]
fn an_owed_reminder_pulses_under_the_switch_or_a_mute_and_never_asleep() {
    use crate::bus_out::Waiting::{self, *};
    let all: [Waiting; 7] = [
        Switch,
        Muted,
        Asleep,
        RestartGuard,
        Unreadable,
        Opening,
        Reading,
    ];
    for waiting in all {
        for owed in [None, Some(true), Some(false)] {
            for asleep in [false, true] {
                let shown = owed.is_some() && matches!(waiting, Switch | Muted) && !asleep;
                assert_eq!(
                    pulse_shown(owed, waiting, asleep),
                    shown,
                    "{owed:?} {waiting:?} asleep {asleep}"
                );
            }
        }
    }
}

/// Thirteen on #1545: the pulse is for the switch and the person's mute, not
/// a hand on the head (whose face the asleep face is), the restart guard, a
/// switch that can't be read, or a wait not closed.
#[test]
fn only_the_switch_and_a_mute_let_an_owed_reminder_pulse() {
    use crate::bus_out::Waiting::*;
    for waiting in [Switch, Muted] {
        assert!(waiting.pulses(), "{waiting:?}");
    }
    for waiting in [Asleep, RestartGuard, Unreadable, Opening, Reading] {
        assert!(!waiting.pulses(), "{waiting:?}");
    }
}

#[test]
fn a_pulse_switched_on_without_the_asleep_face_says_why_once() {
    // (pulse on, ring, LAMP_RING_ASLEEP on, Controls privacy route)
    for quiet in [
        (false, true, true, false),
        (false, false, false, true),
        (true, true, true, false),
    ] {
        let (pulse, ring, asleep, controls) = quiet;
        assert_eq!(
            pulse_without_face(pulse, ring, asleep, controls),
            None,
            "{quiet:?}"
        );
    }
    for (said, why) in [
        ((true, false, true, false), "there is no ring"),
        ((true, true, false, false), "LAMP_RING_ASLEEP is off"),
        ((true, true, false, true), "LAMP_RING_ASLEEP is off"),
        ((true, true, true, true), "the Controls privacy route"),
    ] {
        let (pulse, ring, asleep, controls) = said;
        let line = pulse_without_face(pulse, ring, asleep, controls).expect("said");
        assert!(
            line.starts_with("lamp-voice: LAMP_REMINDER_PULSE is on but "),
            "{line}"
        );
        assert!(line.contains(why), "{said:?}: {line}");
        assert!(
            line.ends_with("so an owed reminder does not pulse"),
            "{line}"
        );
    }
}

#[test]
fn offline_user_interruption_retries_only_after_successful_authority_publication() {
    for publish_fails in [false, true] {
        let mut test = Test::new();
        let chunk = test.generation(17);
        test.accept(&chunk, 120);
        let mut admission = InputAdmission::default();
        let decision = local_policy_decision(
            &mut test.owner,
            &mut admission,
            Verdict::Accept(AcceptanceBasis::DirectedSessionVadOnly),
        );
        let accepted = accepted_decision(decision, &mut test.trace)
            .unwrap()
            .unwrap();
        let mut queue = VecDeque::new();
        let result = commit_offline_input(
            &mut test.owner,
            &mut test.reply,
            &mut test.pending,
            &mut queue,
            accepted,
            PlayoutStart::default(),
            false,
            &mut test.trace,
            |snapshot| {
                assert_ne!(snapshot.owner(), Some(chunk.permit.owner()));
                if publish_fails {
                    Err(io::Error::other("injected authority publication failure"))
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(result.is_err(), publish_fails);
        assert!(queue.is_empty(), "offline input never reaches a provider");
        if publish_fails {
            assert!(matches!(
                test.probe.settlement(),
                Some(ToWorker::Abandon { .. })
            ));
        } else {
            assert!(matches!(
                test.probe.settlement(),
                Some(ToWorker::Retire { .. })
            ));
        }
    }
}

#[test]
fn user_interruption_cannot_retry_a_failed_provider_with_unretired_audio() {
    let clock = crate::scenario_clock::Scope::enter(Duration::from_secs(24)).unwrap();
    clock.set_pump(|_| Ok(())).unwrap();
    let mut test = Test::new();
    let answered = test.ask_skill();
    let chunk = test.generation(17);
    test.accept(&chunk, 120);
    let request = test.reply.as_ref().unwrap().owner.turn();
    observe_provider_output(
        &mut CloudAvailability::default(),
        &mut test.reply,
        &ProviderOutput::TurnFailed {
            request,
            stage: crate::provider_worker::FailedStage::AwaitingResponse,
            reason: crate::provider_worker::Fault::Transport,
        },
    )
    .unwrap();
    assert!(matches!(
        test.reply.as_ref().unwrap().cloud.outcome,
        Some(RequestOutcome::Failed { .. })
    ));
    assert!(!test.reply.as_ref().unwrap().settled(&test.pending));
    let mut admission = InputAdmission::default();
    let decision = local_policy_decision(
        &mut test.owner,
        &mut admission,
        Verdict::Accept(AcceptanceBasis::DirectedSessionVadOnly),
    );
    let accepted = accepted_decision(decision, &mut test.trace)
        .unwrap()
        .unwrap();
    let mut queue = VecDeque::new();
    commit_offline_input(
        &mut test.owner,
        &mut test.reply,
        &mut test.pending,
        &mut queue,
        accepted,
        PlayoutStart::default(),
        false,
        &mut test.trace,
        |snapshot| {
            assert_ne!(snapshot.owner(), Some(chunk.permit.owner()));
            Ok(())
        },
    )
    .unwrap();
    assert!(matches!(
        test.probe.settlement(),
        Some(ToWorker::Abandon { .. })
    ));
    assert!(
        matches!(answered.try_recv(), Ok(Answer::Done(Outcome::Dropped(reason)))
        if reason == "user_interrupted")
    );
    assert!(queue.is_empty());
    assert!(test.pending.is_none());
}
