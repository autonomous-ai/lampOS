//! Production dispatch and receipt handlers with owned local IPC; no audio device.
use super::*;
use crate::provider_worker::{FailedStage, Fault};
use lamp_audio::ownership::PlaybackCursor;

fn failed_reply(before_started: bool) -> (Controller, Option<Reply>, SessionState) {
    let (mut owner, mut current, _) = active_reply();
    owner
        .playback_ended(now(), current.playback.take().unwrap())
        .unwrap();
    current.pcm.clear();
    current.had_audio = false;
    if before_started {
        current.cloud = CloudRequest::new(1).unwrap();
        current
            .cloud
            .submitted(&ProviderInput::Start {
                session: 1,
                request: current.owner.turn(),
                privacy_generation: 1,
                continues: false,
                continues_cut_off: false,
                over_the_lamp: false,
                after_stop: false,
                follows_on: false,
                over_the_lamps_voice: false,
                heard_of_cut: None,
                soft_lead_us: 0,
            })
            .unwrap();
    }
    let request = current.owner.turn();
    let mut reply = Some(current);
    observe_provider_output(
        &mut CloudAvailability::default(),
        &mut reply,
        &ProviderOutput::TurnFailed {
            request,
            stage: if before_started {
                FailedStage::NotDelivered
            } else {
                FailedStage::AwaitingResponse
            },
            reason: Fault::Transport,
        },
    )
    .unwrap();
    let mut trace = SessionState::default();
    trace
        .receipts
        .admitted(reply.as_ref().unwrap().owner, monotonic_us())
        .unwrap();
    (owner, reply, trace)
}

struct Receipts {
    events: VecDeque<WorkerEvent>,
}
impl SpeakerControlIo for Receipts {
    fn next_speaker_event(&mut self) -> io::Result<Option<WorkerEvent>> {
        Ok(self.events.pop_front())
    }
    fn publish_speaker_snapshot(&mut self, _: Snapshot) -> io::Result<()> {
        Ok(())
    }
}
fn accepted(chunk: &SpeakerChunk, offset: u64, frames: usize) -> WorkerEvent {
    let at = monotonic_us();
    WorkerEvent::Playback {
        owner: chunk.permit.owner(),
        turn: chunk.permit.owner().turn(),
        sequence: chunk.chunk_sequence,
        first_sample: PlaybackCursor {
            epoch: 1,
            frame: offset,
        },
        end_sample: PlaybackCursor {
            epoch: 1,
            frame: offset + frames as u64,
        },
        queue_observed_at_us: at,
        queued_frames: 0,
        accepted_frames: frames,
        observed_at_us: at,
    }
}

#[test]
fn failure_notice_before_started_uses_real_permits_and_exact_partial_final_receipts() {
    let directory = SessionDirectory::create().unwrap();
    let (mut parent, mut speaker) = pair(&directory, "speaker");
    let (mut owner, mut reply, mut trace) = failed_reply(true);
    let mut pending = None;
    let catalog = FailureNotices::synthetic();
    select_failure_notice(&catalog, &mut reply, &pending, &mut trace).unwrap();
    let mut snapshot = owner.snapshot(now()).unwrap();
    let mut mask = PlaybackMask::new(TurnTaking::VadOnly);
    let mut sequence = 0;
    let mut last_publish = 0;
    let mut cursor = 0;
    let mut attempts = 0;
    let original_owner = reply.as_ref().unwrap().owner;
    let mut first_zero = true;
    for _ in 0..30 {
        let attempt = dispatch_speaker_chunk(
            &mut owner,
            &mut reply,
            &mut parent.data,
            &mut pending,
            &mut sequence,
            &mut snapshot,
            &mut mask,
        )
        .unwrap();
        attempts += usize::from(attempt.is_some());
        let chunk = speaker.data.receive::<SpeakerChunk>().unwrap().unwrap();
        assert_eq!(chunk.samples, vec![321; 240]);
        assert_eq!(chunk.permit.owner(), original_owner);
        assert!(
            !reply.as_ref().unwrap().cloud.started,
            "no invented provider Started"
        );
        trace.receipts.dispatched(original_owner, sequence).unwrap();
        if first_zero {
            let mut receipts = Receipts {
                events: vec![accepted(&chunk, cursor, 0)].into(),
            };
            service_speaker_control(
                &mut receipts,
                &mut owner,
                &mut reply,
                &mut pending,
                &mut mask,
                &mut snapshot,
                &mut last_publish,
                &mut 16,
                &mut trace,
                &mut None,
            )
            .unwrap();
            assert!(reply.as_ref().unwrap().playback.is_none());
            assert!(pending.is_some());
            assert!(
                dispatch_speaker_chunk(
                    &mut owner,
                    &mut reply,
                    &mut parent.data,
                    &mut pending,
                    &mut sequence,
                    &mut snapshot,
                    &mut mask
                )
                .unwrap()
                .is_none()
            );
            assert!(speaker.data.receive::<SpeakerChunk>().unwrap().is_none());
            first_zero = false;
        }
        let mut receipts = Receipts {
            events: vec![
                accepted(&chunk, cursor, 120),
                accepted(&chunk, cursor + 120, 120),
            ]
            .into(),
        };
        service_speaker_control(
            &mut receipts,
            &mut owner,
            &mut reply,
            &mut pending,
            &mut mask,
            &mut snapshot,
            &mut last_publish,
            &mut 16,
            &mut trace,
            &mut None,
        )
        .unwrap();
        cursor += 240;
        assert!(pending.is_none());
        if chunk.end_of_speech {
            assert_eq!(cursor, 7200);
            assert!(!reply.as_ref().unwrap().notice.settled());
            assert!(reply.as_ref().unwrap().playback.is_some());
            assert!(
                dispatch_speaker_chunk(
                    &mut owner,
                    &mut reply,
                    &mut parent.data,
                    &mut pending,
                    &mut sequence,
                    &mut snapshot,
                    &mut mask
                )
                .unwrap()
                .is_none()
            );
            let token = reply.as_ref().unwrap().playback.unwrap();
            // The local notice must not retire on a matching chunk number with
            // the wrong epoch/frame, or on another chunk's final receipt.
            for (bad_sequence, bad_epoch, bad_cursor) in [
                (chunk.chunk_sequence + 1, 1, cursor),
                (chunk.chunk_sequence, 2, cursor),
                (chunk.chunk_sequence, 1, cursor + 1),
            ] {
                let mut malformed = Receipts {
                    events: vec![WorkerEvent::SpeechRetired {
                        owner: original_owner,
                        sequence: bad_sequence,
                        final_sample: PlaybackCursor {
                            epoch: bad_epoch,
                            frame: bad_cursor,
                        },
                        observed_at_us: monotonic_us(),
                    }]
                    .into(),
                };
                assert!(
                    service_speaker_control(
                        &mut malformed,
                        &mut owner,
                        &mut reply,
                        &mut pending,
                        &mut mask,
                        &mut snapshot,
                        &mut last_publish,
                        &mut 16,
                        &mut trace,
                        &mut None,
                    )
                    .is_err()
                );
                assert!(!reply.as_ref().unwrap().notice.settled());
                assert_eq!(reply.as_ref().unwrap().playback, Some(token));
            }
            let mut receipts = Receipts {
                events: vec![WorkerEvent::SpeechRetired {
                    owner: original_owner,
                    sequence: chunk.chunk_sequence,
                    final_sample: PlaybackCursor {
                        epoch: 1,
                        frame: cursor,
                    },
                    observed_at_us: monotonic_us(),
                }]
                .into(),
            };
            service_speaker_control(
                &mut receipts,
                &mut owner,
                &mut reply,
                &mut pending,
                &mut mask,
                &mut snapshot,
                &mut last_publish,
                &mut 16,
                &mut trace,
                &mut None,
            )
            .unwrap();
            assert!(reply.as_ref().unwrap().notice.settled());
            assert!(reply.as_ref().unwrap().playback.is_none());
            let mut duplicate = Receipts {
                events: vec![WorkerEvent::SpeechRetired {
                    owner: original_owner,
                    sequence: chunk.chunk_sequence,
                    final_sample: PlaybackCursor {
                        epoch: 1,
                        frame: cursor,
                    },
                    observed_at_us: monotonic_us(),
                }]
                .into(),
            };
            assert!(
                service_speaker_control(
                    &mut duplicate,
                    &mut owner,
                    &mut reply,
                    &mut pending,
                    &mut mask,
                    &mut snapshot,
                    &mut last_publish,
                    &mut 16,
                    &mut trace,
                    &mut None,
                )
                .is_err()
            );
            assert_eq!(
                trace
                    .iter()
                    .find(|e| e["kind"] == "failure_notice_final_sample_retired")
                    .unwrap()["playback_sequence"],
                token.sequence()
            );
        }
    }
    select_failure_notice(&catalog, &mut reply, &pending, &mut trace).unwrap();
    assert_eq!(attempts, 1);
    assert!(!reply.as_ref().unwrap().had_audio);
    assert_eq!(
        reply.as_ref().unwrap().delivery_outcome(),
        "provider_failed"
    );
    assert_eq!(
        trace
            .iter()
            .filter(|e| e["kind"] == "failure_notice_first_write")
            .count(),
        1
    );
    assert!(
        !trace
            .iter()
            .any(|e| e["kind"] == "speaker_first_write" || e["kind"] == "provider_first_audio")
    );
}

#[test]
fn failure_notice_cancelled_while_selected_or_dispatched_never_revives() {
    for (dispatched, reason) in [
        (false, "user_interrupted"),
        (true, "user_interrupted"),
        (false, "service_stopped"),
        (true, "service_stopped"),
        (false, "privacy_muted"),
        (true, "privacy_muted"),
    ] {
        let directory = SessionDirectory::create().unwrap();
        let (mut parent, mut speaker) = pair(&directory, "speaker");
        let (mut owner, mut reply, mut trace) = failed_reply(false);
        let catalog = FailureNotices::synthetic();
        let mut pending = None;
        let mut sequence = 0;
        let mut snapshot = owner.snapshot(now()).unwrap();
        let mut mask = PlaybackMask::new(TurnTaking::VadOnly);
        select_failure_notice(&catalog, &mut reply, &pending, &mut trace).unwrap();
        let old = reply.as_ref().unwrap().owner;
        if dispatched {
            dispatch_speaker_chunk(
                &mut owner,
                &mut reply,
                &mut parent.data,
                &mut pending,
                &mut sequence,
                &mut snapshot,
                &mut mask,
            )
            .unwrap();
            let chunk = speaker.data.receive::<SpeakerChunk>().unwrap().unwrap();
            trace.receipts.dispatched(old, sequence).unwrap();
            let mut receipts = Receipts {
                events: vec![accepted(&chunk, 0, 120)].into(),
            };
            service_speaker_control(
                &mut receipts,
                &mut owner,
                &mut reply,
                &mut pending,
                &mut mask,
                &mut snapshot,
                &mut 0,
                &mut 16,
                &mut trace,
                &mut None,
            )
            .unwrap();
            assert!(reply.as_ref().unwrap().playback.is_some());
        }
        cancel_reply(&mut owner, &mut reply, &mut trace, reason).unwrap();
        pending = None;
        let successor = owner.admit(now(), AdmittedInput::NewTurn).unwrap();
        assert_ne!(old, successor);
        select_failure_notice(&catalog, &mut reply, &pending, &mut trace).unwrap();
        assert!(
            dispatch_speaker_chunk(
                &mut owner,
                &mut reply,
                &mut parent.data,
                &mut pending,
                &mut sequence,
                &mut snapshot,
                &mut mask
            )
            .unwrap()
            .is_none()
        );
        assert!(speaker.data.receive::<SpeakerChunk>().unwrap().is_none());
        let finished = trace.iter().find(|e| e["kind"] == "turn_finished").unwrap();
        assert_eq!(finished["outcome"], reason);
        assert_eq!(
            finished["failure_notice"]["state"],
            if dispatched { "attempted" } else { "selected" }
        );
        assert_eq!(owner.snapshot(now()).unwrap().owner(), Some(successor));
    }
}

#[test]
fn failure_notice_privacy_or_owner_revocation_is_checked_before_cursor_reservation() {
    for privacy in [false, true] {
        let directory = SessionDirectory::create().unwrap();
        let (mut parent, mut speaker) = pair(&directory, "speaker");
        let (mut owner, mut reply, mut trace) = failed_reply(false);
        let mut pending = None;
        let mut sequence = 0;
        let mut snapshot = owner.snapshot(now()).unwrap();
        select_failure_notice(
            &FailureNotices::synthetic(),
            &mut reply,
            &pending,
            &mut trace,
        )
        .unwrap();
        if privacy {
            owner
                .set_microphone_permission(now(), Permission::Denied)
                .unwrap();
        } else {
            owner
                .cancel_turn(now(), reply.as_ref().unwrap().owner)
                .unwrap();
        }
        assert!(
            dispatch_speaker_chunk(
                &mut owner,
                &mut reply,
                &mut parent.data,
                &mut pending,
                &mut sequence,
                &mut snapshot,
                &mut PlaybackMask::new(TurnTaking::VadOnly)
            )
            .is_err()
        );
        assert!(speaker.data.receive::<SpeakerChunk>().unwrap().is_none());
        assert_eq!(
            reply.as_ref().unwrap().notice.evidence()["reserved_frames"],
            0
        );
        assert_eq!(
            reply.as_ref().unwrap().notice.evidence()["state"],
            "selected"
        );
    }
}

#[test]
fn failure_notice_ignores_old_request_and_does_not_weaken_duplicate_failure_fault() {
    let (mut owner, mut current, _) = active_reply();
    owner
        .playback_ended(now(), current.playback.take().unwrap())
        .unwrap();
    current.pcm.clear();
    let old_id = current.owner.turn();
    owner.cancel_turn(now(), current.owner).unwrap();
    current.owner = owner.admit(now(), AdmittedInput::NewTurn).unwrap();
    current.plan = owner
        .plan_output(now(), current.owner, OutputKind::Speech, None)
        .unwrap();
    owner.input_ended(now(), current.owner).unwrap();
    let id = current.owner.turn();
    let mut reply = Some(current);
    let mut cloud = CloudAvailability::default();
    let mut trace = SessionState::default();
    let catalog = FailureNotices::synthetic();
    let old = ProviderOutput::TurnFailed {
        request: old_id,
        stage: FailedStage::AwaitingResponse,
        reason: Fault::Transport,
    };
    observe_provider_output(&mut cloud, &mut reply, &old).unwrap();
    select_failure_notice(&catalog, &mut reply, &None, &mut trace).unwrap();
    assert!(matches!(
        reply.as_ref().unwrap().notice,
        NoticeDelivery::Unselected
    ));
    let event = ProviderOutput::TurnFailed {
        request: id,
        stage: FailedStage::AwaitingResponse,
        reason: Fault::Transport,
    };
    observe_provider_output(&mut cloud, &mut reply, &event).unwrap();
    select_failure_notice(&catalog, &mut reply, &None, &mut trace).unwrap();
    assert!(observe_provider_output(&mut cloud, &mut reply, &event).is_err());
    select_failure_notice(&catalog, &mut reply, &None, &mut trace).unwrap();
    assert_eq!(
        trace
            .iter()
            .filter(|e| e["kind"] == "failure_notice_selected")
            .count(),
        1
    );
}

#[test]
fn failure_notice_retains_raw_gap_without_tainting_provider_answer_metrics() {
    let directory = SessionDirectory::create().unwrap();
    let (mut parent, mut speaker) = pair(&directory, "speaker");
    let (mut owner, mut reply, mut trace) = failed_reply(false);
    let mut pending = None;
    let mut sequence = 0;
    let mut snapshot = owner.snapshot(now()).unwrap();
    let mut mask = PlaybackMask::new(TurnTaking::VadOnly);
    select_failure_notice(
        &FailureNotices::synthetic(),
        &mut reply,
        &pending,
        &mut trace,
    )
    .unwrap();
    dispatch_speaker_chunk(
        &mut owner,
        &mut reply,
        &mut parent.data,
        &mut pending,
        &mut sequence,
        &mut snapshot,
        &mut mask,
    )
    .unwrap();
    let chunk = speaker.data.receive::<SpeakerChunk>().unwrap().unwrap();
    trace
        .receipts
        .dispatched(chunk.permit.owner(), sequence)
        .unwrap();
    let mut receipts = Receipts {
        events: vec![accepted(&chunk, 0, 240)].into(),
    };
    service_speaker_control(
        &mut receipts,
        &mut owner,
        &mut reply,
        &mut pending,
        &mut mask,
        &mut snapshot,
        &mut 0,
        &mut 16,
        &mut trace,
        &mut None,
    )
    .unwrap();
    let gap = crate::wire::PlaybackGap {
        owner: chunk.permit.owner(),
        first_sample: PlaybackCursor {
            epoch: 1,
            frame: 240,
        },
        end_sample: PlaybackCursor {
            epoch: 1,
            frame: 480,
        },
        started_at_us: 11,
        last_zero_accepted_at_us: 11,
        zero_frames: 240,
    };
    handle_playback_gap(
        &mut reply,
        None,
        &mut trace,
        WorkerEvent::PlaybackGap {
            phase: PlaybackGapPhase::Started,
            gap,
            observed_at_us: 11,
        },
        false,
        None,
    )
    .unwrap();
    assert_eq!(reply.as_ref().unwrap().playback_gaps, 0);
    assert_eq!(
        reply.as_ref().unwrap().notice.evidence()["playback_gaps"],
        1
    );
    assert_eq!(
        reply.as_ref().unwrap().delivery_outcome(),
        "provider_failed"
    );
    let raw = trace.iter().find(|e| e["kind"] == "playback_gap").unwrap();
    assert_eq!(raw["origin"], "failure_notice");
    assert_eq!(raw["gap"]["zero_frames"], 240);
    assert_eq!(raw["gap"]["first_sample"]["frame"], 240);
    let mut malformed = gap;
    malformed.zero_frames = 239;
    assert!(
        handle_playback_gap(
            &mut reply,
            None,
            &mut trace,
            WorkerEvent::PlaybackGap {
                phase: PlaybackGapPhase::Started,
                gap: malformed,
                observed_at_us: 11
            },
            false,
            None
        )
        .is_err()
    );
    assert_eq!(
        reply.as_ref().unwrap().notice.evidence()["playback_gaps"],
        1
    );
    cancel_reply(&mut owner, &mut reply, &mut trace, "user_interrupted").unwrap();
    let (_, next, _) = active_reply();
    assert_ne!(next.owner, gap.owner);
    reply = Some(next);
    handle_playback_gap(
        &mut reply,
        None,
        &mut trace,
        WorkerEvent::PlaybackGap {
            phase: PlaybackGapPhase::Cancelled,
            gap,
            observed_at_us: 12,
        },
        false,
        None,
    )
    .unwrap();
    assert_eq!(reply.as_ref().unwrap().playback_gaps, 0);
    assert_eq!(trace.last().unwrap()["origin"], serde_json::Value::Null);
}

#[test]
fn failure_notice_failed_transport_keeps_attempt_then_uses_existing_cancellation() {
    let directory = SessionDirectory::create().unwrap();
    let (mut parent, speaker) = pair(&directory, "speaker");
    let (mut owner, mut reply, mut trace) = failed_reply(false);
    let mut pending = None;
    let mut snapshot = owner.snapshot(now()).unwrap();
    select_failure_notice(
        &FailureNotices::synthetic(),
        &mut reply,
        &pending,
        &mut trace,
    )
    .unwrap();
    drop(speaker);
    // A child another test forks as `speaker` drops holds a copy of its
    // socket until its exec (CLOEXEC), so the dispatch below could still be
    // taken (#1569 row 73): sent to until refused first, as
    // `Worker::send_stop_until_peer_gone` does.
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while !parent.data.send(0u8).is_err_and(|error| {
        matches!(
            error.kind(),
            std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::ConnectionRefused
                | std::io::ErrorKind::NotConnected
                | std::io::ErrorKind::BrokenPipe
        )
    }) {
        assert!(
            std::time::Instant::now() < until,
            "the speaker's peer was not gone after 5 s"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        dispatch_speaker_chunk(
            &mut owner,
            &mut reply,
            &mut parent.data,
            &mut pending,
            &mut 0,
            &mut snapshot,
            &mut PlaybackMask::new(TurnTaking::VadOnly)
        )
        .is_err()
    );
    assert!(pending.is_none());
    assert_eq!(
        reply.as_ref().unwrap().notice.evidence()["state"],
        "attempted"
    );
    assert_eq!(
        reply.as_ref().unwrap().notice.evidence()["reserved_frames"],
        240
    );
    // conversation propagates this error; finish_active_run uses this same
    // cancellation before cleanup. No provider retry or second dispatch occurs.
    cancel_reply(&mut owner, &mut reply, &mut trace, "runtime_failed").unwrap();
    assert!(reply.is_none());
    let terminal = trace.iter().find(|e| e["kind"] == "turn_finished").unwrap();
    assert_eq!(terminal["failure_notice"]["state"], "attempted");
    assert_eq!(terminal["outcome"], "runtime_failed");
}

#[test]
fn independent_notice_revocation_after_first_complete_chunk_blocks_next_reservation() {
    for privacy in [false, true] {
        let directory = SessionDirectory::create().unwrap();
        let (mut parent, mut speaker) = pair(&directory, "speaker");
        let (mut owner, mut reply, mut trace) = failed_reply(false);
        let mut pending = None;
        let mut snapshot = owner.snapshot(now()).unwrap();
        let mut mask = PlaybackMask::new(TurnTaking::V1Baseline);
        let mut sequence = 0;
        select_failure_notice(
            &FailureNotices::synthetic(),
            &mut reply,
            &pending,
            &mut trace,
        )
        .unwrap();
        dispatch_speaker_chunk(
            &mut owner,
            &mut reply,
            &mut parent.data,
            &mut pending,
            &mut sequence,
            &mut snapshot,
            &mut mask,
        )
        .unwrap();
        let chunk = speaker.data.receive::<SpeakerChunk>().unwrap().unwrap();
        trace
            .receipts
            .dispatched(chunk.permit.owner(), sequence)
            .unwrap();
        let mut receipts = Receipts {
            events: vec![accepted(&chunk, 0, 240)].into(),
        };
        service_speaker_control(
            &mut receipts,
            &mut owner,
            &mut reply,
            &mut pending,
            &mut mask,
            &mut snapshot,
            &mut 0,
            &mut 16,
            &mut trace,
            &mut None,
        )
        .unwrap();
        assert!(reply.as_ref().unwrap().playback.is_some());
        assert!(pending.is_none());
        if privacy {
            owner
                .set_microphone_permission(now(), Permission::Denied)
                .unwrap();
        } else {
            owner.cancel_turn(now(), chunk.permit.owner()).unwrap();
        }
        assert!(
            dispatch_speaker_chunk(
                &mut owner,
                &mut reply,
                &mut parent.data,
                &mut pending,
                &mut sequence,
                &mut snapshot,
                &mut mask
            )
            .is_err()
        );
        assert!(speaker.data.receive::<SpeakerChunk>().unwrap().is_none());
        assert_eq!(
            reply.as_ref().unwrap().notice.evidence()["reserved_frames"],
            240
        );
        assert_eq!(
            reply.as_ref().unwrap().delivery_outcome(),
            "provider_failed"
        );
    }
}

#[test]
fn independent_notice_late_original_acceptance_cannot_revive_cancelled_owner() {
    let directory = SessionDirectory::create().unwrap();
    let (mut parent, mut speaker) = pair(&directory, "speaker");
    let (mut owner, mut reply, mut trace) = failed_reply(false);
    let mut pending = None;
    let mut snapshot = owner.snapshot(now()).unwrap();
    let mut mask = PlaybackMask::new(TurnTaking::V1Baseline);
    let mut sequence = 0;
    select_failure_notice(
        &FailureNotices::synthetic(),
        &mut reply,
        &pending,
        &mut trace,
    )
    .unwrap();
    dispatch_speaker_chunk(
        &mut owner,
        &mut reply,
        &mut parent.data,
        &mut pending,
        &mut sequence,
        &mut snapshot,
        &mut mask,
    )
    .unwrap();
    let chunk = speaker.data.receive::<SpeakerChunk>().unwrap().unwrap();
    trace
        .receipts
        .dispatched(chunk.permit.owner(), sequence)
        .unwrap();
    // This acceptance happened before cancellation, but its receipt is delayed.
    let receipt = accepted(&chunk, 0, 240);
    cancel_reply(&mut owner, &mut reply, &mut trace, "user_interrupted").unwrap();
    pending = None;
    let successor = owner.admit(now(), AdmittedInput::NewTurn).unwrap();
    trace.receipts.admitted(successor, monotonic_us()).unwrap();
    let mut receipts = Receipts {
        events: vec![receipt].into(),
    };
    service_speaker_control(
        &mut receipts,
        &mut owner,
        &mut reply,
        &mut pending,
        &mut mask,
        &mut snapshot,
        &mut 0,
        &mut 16,
        &mut trace,
        &mut None,
    )
    .unwrap();
    assert_eq!(owner.snapshot(now()).unwrap().owner(), Some(successor));
    assert!(reply.is_none());
    assert!(
        dispatch_speaker_chunk(
            &mut owner,
            &mut reply,
            &mut parent.data,
            &mut pending,
            &mut sequence,
            &mut snapshot,
            &mut mask
        )
        .unwrap()
        .is_none()
    );
    assert!(speaker.data.receive::<SpeakerChunk>().unwrap().is_none());
    assert_eq!(
        trace
            .iter()
            .filter(|e| e["kind"] == "turn_finished")
            .count(),
        1
    );
    assert!(
        !trace
            .iter()
            .any(|e| e["kind"] == "failure_notice_first_write")
    );
}

/// A local-only turn (#830): admitted and ended, its request failed locally.
fn offline_reply(speech: bool) -> (Controller, Option<Reply>) {
    let (mut owner, mut current, _) = active_reply();
    owner
        .playback_ended(now(), current.playback.take().unwrap())
        .unwrap();
    current.pcm.clear();
    owner.cancel_turn(now(), current.owner).unwrap();
    current.owner = owner.admit(now(), AdmittedInput::NewTurn).unwrap();
    current.plan = owner
        .plan_output(now(), current.owner, OutputKind::Speech, None)
        .unwrap();
    owner.input_ended(now(), current.owner).unwrap();
    current.cloud = CloudRequest::local_only();
    current.provider_idle = true;
    current.generation_done = true;
    current.local_input_ended = true;
    current.notice = NoticeDelivery::default();
    current.local_only = Some(notices::LocalOnly {
        speech,
        back: false,
        service: false,
    });
    (owner, Some(current))
}

fn selected(reply: &Option<Reply>, kind: lamp_audio::notice::NoticeKind) -> bool {
    matches!(&reply.as_ref().unwrap().notice, NoticeDelivery::Selected { recording, .. }
        if recording.kind() == Some(kind))
}

fn selected_offline(reply: &Option<Reply>) -> bool {
    selected(reply, lamp_audio::notice::NoticeKind::ProviderUnavailable)
}

#[test]
fn back_online_is_its_own_recording_and_does_not_count_as_the_outages_line() {
    let catalog = FailureNotices::synthetic_offline();
    let mut outage = Outage::default();
    let mut trace = SessionState::default();
    let (_owner, mut back) = offline_reply(true);
    back.as_mut().unwrap().local_only = Some(notices::LocalOnly {
        speech: true,
        back: true,
        service: false,
    });
    select_failure_notice_in(&catalog, &mut back, &None, &mut outage, &mut trace).unwrap();
    assert!(selected(&back, lamp_audio::notice::NoticeKind::BackOnline));
    assert!(!outage.said);
    // Without its recording: silent, as every notice is.
    let (_owner, mut silent) = offline_reply(true);
    silent.as_mut().unwrap().local_only = Some(notices::LocalOnly {
        speech: true,
        back: true,
        service: false,
    });
    select_failure_notice_in(
        &FailureNotices::synthetic(),
        &mut silent,
        &None,
        &mut outage,
        &mut trace,
    )
    .unwrap();
    assert!(matches!(
        silent.as_ref().unwrap().notice,
        NoticeDelivery::Skipped(_)
    ));
}

/// The speaker has taken the turn's first block: its playback has begun.
fn speaker_took(owner: &mut Controller, reply: &mut Option<Reply>) {
    let current = reply.as_mut().unwrap();
    let permit = owner.issue_output(now(), current.plan, 100_000).unwrap();
    current.playback = Some(owner.playback_started(now(), permit).unwrap());
}

#[test]
fn offline_the_line_is_said_once_an_outage_and_only_for_speech() {
    let catalog = FailureNotices::synthetic_offline();
    let mut outage = Outage::default();
    let mut trace = SessionState::default();
    // A sound let through (a cough): nothing, and the outage keeps its line.
    let (_owner, mut cough) = offline_reply(false);
    select_failure_notice_in(&catalog, &mut cough, &None, &mut outage, &mut trace).unwrap();
    assert!(matches!(
        cough.as_ref().unwrap().notice,
        NoticeDelivery::Unselected
    ));
    outage.note_delivery(&cough);
    assert!(!outage.said);
    // Speech: the offline line, never turn_lost. Chosen is not said.
    let (mut owner, mut first) = offline_reply(true);
    select_failure_notice_in(&catalog, &mut first, &None, &mut outage, &mut trace).unwrap();
    assert!(selected_offline(&first));
    outage.note_delivery(&first);
    assert!(!outage.said, "chosen, not yet taken by the speaker");
    speaker_took(&mut owner, &mut first);
    outage.note_delivery(&first);
    assert!(outage.said);
    // A second try in the same outage: no words.
    let (_owner, mut second) = offline_reply(true);
    select_failure_notice_in(&catalog, &mut second, &None, &mut outage, &mut trace).unwrap();
    assert!(matches!(
        second.as_ref().unwrap().notice,
        NoticeDelivery::Unselected
    ));
    // It settles at once, so the next turn is not held.
    assert!(second.as_ref().unwrap().settled(&None));
    // A new outage says it again.
    let mut next = Outage::default();
    let (_owner, mut again) = offline_reply(true);
    select_failure_notice_in(&catalog, &mut again, &None, &mut next, &mut trace).unwrap();
    assert!(selected_offline(&again));
}

/// The reviewer's case on #884: the line was chosen and then the turn was
/// cancelled (a Stop) before the speaker took any of it. The outage still
/// owes the line, and so nothing makes "back online" owed.
#[test]
fn offline_a_line_cancelled_before_the_speaker_took_it_is_still_owed() {
    let catalog = FailureNotices::synthetic_offline();
    let mut outage = Outage::default();
    let mut trace = SessionState::default();
    let (mut owner, mut first) = offline_reply(true);
    select_failure_notice_in(&catalog, &mut first, &None, &mut outage, &mut trace).unwrap();
    assert!(selected_offline(&first));
    outage.note_delivery(&first);
    let current = first.take().unwrap();
    owner.cancel_turn(now(), current.owner).unwrap();
    outage.note_delivery(&first);
    assert!(!outage.said, "cancelled before acceptance: not said");
    // So the next speech in the same outage gets the line.
    let (_owner, mut next) = offline_reply(true);
    select_failure_notice_in(&catalog, &mut next, &None, &mut outage, &mut trace).unwrap();
    assert!(selected_offline(&next));
}

#[test]
fn offline_with_no_recording_is_silent_and_a_local_turn_sends_nothing() {
    let mut outage = Outage::default();
    let mut trace = SessionState::default();
    // No asset provisioned: skipped, said in the capability, nothing played.
    let (_owner, mut reply) = offline_reply(true);
    select_failure_notice_in(
        &FailureNotices::synthetic(),
        &mut reply,
        &None,
        &mut outage,
        &mut trace,
    )
    .unwrap();
    assert!(matches!(
        reply.as_ref().unwrap().notice,
        NoticeDelivery::Skipped(_)
    ));
    assert!(!outage.said);
    // Nothing of a local-only turn goes to the provider.
    let current = reply.as_mut().unwrap();
    let mut queue = VecDeque::new();
    let audio = ObservedAudio {
        sequence: 1,
        captured_at_us: monotonic_us(),
        samples: [0; lamp_audio::CAPTURE_SAMPLES],
    };
    queue_input(&mut queue, current, 1, audio, 1).unwrap();
    queue_input_end(&mut queue, current, 1, 1, false, false).unwrap();
    assert!(queue.is_empty());
    // Its request can match no provider input or output.
    assert!(current.cloud.outcome.is_some());
    assert!(current.cloud.confirm_start(0).is_err());
}

#[test]
fn offline_only_with_the_switch_and_after_the_links_grace() {
    let grace = crate::bus_out::LINK_GRACE_US;
    // Without the switch: never, however long the link is down.
    assert!(!offline_now(false, Some(0), 3_600_000_000, None));
    // With it: not while the link is up, nor within its grace.
    assert!(!offline_now(true, None, 3_600_000_000, None));
    assert!(!offline_now(true, Some(1_000), 1_000 + grace - 1, None));
    assert!(offline_now(true, Some(1_000), 1_000 + grace, None));
}

/// El on #1247: with the probe on, the first seconds of a relay outage never
/// say no internet. The voice waits for the probe's verdict: the internet
/// answering is never offline, its not answering is, at once past the grace;
/// with no verdict, only after `REACH_WAIT_US`.
#[test]
fn with_the_probe_the_first_seconds_of_an_outage_wait_for_its_verdict() {
    let grace = crate::bus_out::LINK_GRACE_US;
    let since = 1_000;
    // The internet answering (a relay down): never offline, at any moment.
    for at in (0..=60_000_000).step_by(250_000) {
        assert!(
            !offline_now(true, Some(since), since + at, Some(Some(true))),
            "{at}"
        );
    }
    // No verdict yet: not offline until the wait, however the grace stands.
    assert!(!offline_now(true, Some(since), since + grace, Some(None)));
    assert!(!offline_now(
        true,
        Some(since),
        since + REACH_WAIT_US - 1,
        Some(None)
    ));
    assert!(offline_now(
        true,
        Some(since),
        since + REACH_WAIT_US,
        Some(None)
    ));
    // The internet not answering: offline once past the grace, as before.
    assert!(!offline_now(
        true,
        Some(since),
        since + grace - 1,
        Some(Some(false))
    ));
    assert!(offline_now(
        true,
        Some(since),
        since + grace,
        Some(Some(false))
    ));
    // The wait leaves room for the probe: grace, its 2 s bound, two reports.
    assert!(REACH_WAIT_US >= grace + 2_000_000 + 2 * 1_000_000);
}

/// #830 (C): the voice service alone down, the internet answering, is its
/// own outage only with the switch and past the link's grace, and never at
/// the same moment as the internet's: the probe's verdict decides which.
#[test]
fn the_service_alone_down_is_its_own_outage_on_the_probes_verdict() {
    let grace = crate::bus_out::LINK_GRACE_US;
    let since = 1_000;
    assert!(!service_down_now(
        false,
        Some(since),
        since + grace,
        Some(Some(true))
    ));
    assert!(!service_down_now(
        true,
        None,
        since + grace,
        Some(Some(true))
    ));
    assert!(!service_down_now(
        true,
        Some(since),
        since + grace - 1,
        Some(Some(true))
    ));
    assert!(service_down_now(
        true,
        Some(since),
        since + grace,
        Some(Some(true))
    ));
    for verdict in [None, Some(None), Some(Some(false))] {
        assert!(
            !service_down_now(true, Some(since), since + 60_000_000, verdict),
            "{verdict:?}"
        );
    }
    // Never both: whatever the verdict and the moment, one outage at a time.
    for verdict in [None, Some(None), Some(Some(false)), Some(Some(true))] {
        for at in (0..=30_000_000).step_by(500_000) {
            assert!(
                !(offline_now(true, Some(since), since + at, verdict)
                    && service_down_now(true, Some(since), since + at, verdict)),
                "{verdict:?} {at}"
            );
        }
    }
}

/// #830 (C): a local-only turn taken while only the service is down says the
/// service's line, once an outage and only for speech, and the outage
/// remembers it was the service's, so no "back online" follows it. The
/// internet's local-only turn still says the offline line.
#[test]
fn the_service_line_is_said_once_and_is_not_the_internets() {
    let catalog = FailureNotices::synthetic_offline();
    let mut outage = Outage::default();
    let mut trace = SessionState::default();
    let service = |reply: &mut Option<Reply>| {
        reply.as_mut().unwrap().local_only.as_mut().unwrap().service = true;
    };
    let (_owner, mut cough) = offline_reply(false);
    service(&mut cough);
    select_failure_notice_in(&catalog, &mut cough, &None, &mut outage, &mut trace).unwrap();
    assert!(matches!(
        cough.as_ref().unwrap().notice,
        NoticeDelivery::Unselected
    ));
    let (mut owner, mut first) = offline_reply(true);
    service(&mut first);
    select_failure_notice_in(&catalog, &mut first, &None, &mut outage, &mut trace).unwrap();
    assert!(selected(
        &first,
        lamp_audio::notice::NoticeKind::VoiceServiceUnreachable
    ));
    speaker_took(&mut owner, &mut first);
    outage.note_delivery(&first);
    assert!(outage.said && outage.service);
    let (_owner, mut second) = offline_reply(true);
    service(&mut second);
    select_failure_notice_in(&catalog, &mut second, &None, &mut outage, &mut trace).unwrap();
    assert!(matches!(
        second.as_ref().unwrap().notice,
        NoticeDelivery::Unselected
    ));
    // The internet's outage: its own line, and "back online" may follow.
    let mut internet = Outage::default();
    let (mut owner, mut offline) = offline_reply(true);
    select_failure_notice_in(&catalog, &mut offline, &None, &mut internet, &mut trace).unwrap();
    assert!(selected_offline(&offline));
    speaker_took(&mut owner, &mut offline);
    internet.note_delivery(&offline);
    assert!(internet.said && !internet.service);
    // Without its recording the service's turn is silent, as every notice is.
    let (_owner, mut silent) = offline_reply(true);
    service(&mut silent);
    select_failure_notice_in(
        &FailureNotices::synthetic(),
        &mut silent,
        &None,
        &mut Outage::default(),
        &mut trace,
    )
    .unwrap();
    assert!(matches!(
        silent.as_ref().unwrap().notice,
        NoticeDelivery::Skipped(_)
    ));
}
