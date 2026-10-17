//! Real coordinator owner, speaker IPC, partial acceptance and retirement joins.
//! These synthetic PCM receipts establish neither acoustic nor model fidelity.
use super::*;
use crate::reminders::{Probe, RoundtripFixture, ToWorker, receipt_for_test};
use lamp_audio::ownership::{PlaybackCursor, QueuedRange};

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

#[test]
fn stored_reminder_words_cannot_start_a_camera_followup_even_when_transcript_is_late() {
    let mut opening = None;
    let mut wanted = None;
    let mut trace = SessionState::default();
    for text in ["Let me ", "take a look"] {
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
    for text in ["Let me ", "take a look"] {
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
        "Let me look",
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
            "Let me look",
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
        "Let me take a look",
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
    std::thread::sleep(Duration::from_micros(crate::turn_taking::TAIL_US + 1_000));
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
    let mut test = Test::new();
    let first = test.generation(17);
    test.accept(&first, 120);
    assert!(test.pending.is_some());
    assert!(test.probe.settlement().is_none());
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
    test.retire(&second);
    assert!(test.finish_if_settled());
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
    for had_audio in [false, true] {
        for failed in [false, true] {
            let mut test = Test::new();
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
            } else {
                assert!(matches!(
                    test.probe.settlement(),
                    Some(ToWorker::Abandon { .. })
                ));
            }
        }
    }
}

#[test]
fn reset_or_playback_gap_cannot_be_relabeled_whole_reminder_completion() {
    for reset in [false, true] {
        let mut test = Test::new();
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
        test.provider_complete();
        assert!(test.finish_if_settled());
        assert!(matches!(
            test.probe.settlement(),
            Some(ToWorker::Abandon { .. })
        ));
    }
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
    assert!(asking.unclear(7, Some(7), 100));
    assert_eq!(asking.wanted(), Some((7, 100)));
    assert_eq!(asking.cancelled(), Some(7));
    assert_eq!(asking.wanted(), None);
    // After the Stop: no turn in hand, or another one.
    assert!(!asking.unclear(7, None, 200));
    assert!(!asking.unclear(7, Some(8), 200));
    assert_eq!(asking.wanted(), None);
    assert_eq!(asking.cancelled(), None);
}
