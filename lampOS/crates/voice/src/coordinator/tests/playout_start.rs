//! Production dispatch/ownership with private Unix sockets. No audio device.
use super::*;
use crate::provider_worker::{FailedStage, Fault};
use lamp_audio::playout::{OpenedBy, START_HORIZON_US};

fn fresh_reply() -> (Controller, Reply) {
    let (mut owner, mut reply, _) = active_reply();
    owner
        .playback_ended(now(), reply.playback.take().unwrap())
        .unwrap();
    reply.pcm.clear();
    reply.had_audio = false;
    reply.start = StartGate::new(START_HORIZON_US);
    (owner, reply)
}

#[test]
fn opening_requires_a_dispatchable_chunk_not_only_a_satisfied_time_condition() {
    for frames in [1, 240] {
        let (_, mut reply) = fresh_reply();
        reply.append_provider_audio(vec![7; frames], 1_000);
        assert!(!reply.may_start(501_000));
        assert!(reply.start.opened().is_none());
        assert_eq!(reply.pcm.len(), frames);
    }
    let (_, mut reply) = fresh_reply();
    reply.append_provider_audio(vec![7; 4_800], 1_000);
    reply.final_chunk = Some(37);
    assert!(!reply.may_start(501_000));
    assert!(reply.start.opened().is_none());
}

#[test]
fn actual_dispatch_holds_without_minting_a_permit_or_mutating_pcm() {
    let dir = SessionDirectory::create().unwrap();
    let (mut parent, mut speaker) = pair(&dir, "speaker");
    let (mut owner, mut current) = fresh_reply();
    let original: Vec<i16> = (0..3_120).map(|i| (i % 100) as i16).collect();
    current.append_provider_audio(original.clone(), monotonic_us());
    let mut reply = Some(current);
    let mut pending = None;
    let mut sequence = 0;
    let mut snapshot = owner.snapshot(now()).unwrap();
    let revision = snapshot.revision();
    dispatch_speaker_chunk(
        &mut owner,
        &mut reply,
        &mut parent.data,
        &mut pending,
        &mut sequence,
        &mut snapshot,
        &mut PlaybackMask::new(TurnTaking::V1Baseline),
    )
    .unwrap();
    assert!(speaker.data.receive::<SpeakerChunk>().unwrap().is_none());
    assert!(pending.is_none());
    assert_eq!(sequence, 0);
    // Publishing another snapshot itself increments its revision. The held
    // dispatch must not publish one or replace the caller's snapshot.
    assert_eq!(snapshot.revision(), revision);
    assert_eq!(owner.snapshot(now()).unwrap().revision(), revision + 1);
    assert_eq!(reply.unwrap().pcm, original);
}

#[test]
fn gate_does_not_replace_matching_started_or_real_input_endpoint() {
    for missing_started in [true, false] {
        let dir = SessionDirectory::create().unwrap();
        let (mut parent, mut speaker) = pair(&dir, "speaker");
        let (mut owner, mut current) = fresh_reply();
        current.append_provider_audio(vec![7; 4_800], monotonic_us());
        if missing_started {
            current.cloud.started = false;
        } else {
            current.local_input_ended = false;
        }
        let mut reply = Some(current);
        let mut pending = None;
        let mut sequence = 0;
        let mut snapshot = owner.snapshot(now()).unwrap();
        dispatch_speaker_chunk(
            &mut owner,
            &mut reply,
            &mut parent.data,
            &mut pending,
            &mut sequence,
            &mut snapshot,
            &mut PlaybackMask::new(TurnTaking::V1Baseline),
        )
        .unwrap();
        assert!(speaker.data.receive::<SpeakerChunk>().unwrap().is_none());
        assert!(reply.unwrap().start.opened().is_none());
    }
}

#[test]
fn complete_or_failed_short_reply_flushes_exact_pcm_and_only_final_padding() {
    for failed in [false, true] {
        let dir = SessionDirectory::create().unwrap();
        let (mut parent, mut speaker) = pair(&dir, "speaker");
        let (mut owner, mut current) = fresh_reply();
        let request = current.owner.turn();
        current.append_provider_audio(vec![7; 73], monotonic_us());
        current.had_audio = true;
        let mut reply = Some(current);
        let terminal = if failed {
            ProviderOutput::TurnFailed {
                request,
                stage: FailedStage::Responding,
                reason: Fault::Transport,
            }
        } else {
            ProviderOutput::TurnComplete {
                request,
                idle: true,
            }
        };
        observe_provider_output(&mut CloudAvailability::default(), &mut reply, &terminal).unwrap();
        let mut pending = None;
        let mut sequence = 0;
        let mut snapshot = owner.snapshot(now()).unwrap();
        dispatch_speaker_chunk(
            &mut owner,
            &mut reply,
            &mut parent.data,
            &mut pending,
            &mut sequence,
            &mut snapshot,
            &mut PlaybackMask::new(TurnTaking::V1Baseline),
        )
        .unwrap();
        let chunk = speaker.data.receive::<SpeakerChunk>().unwrap().unwrap();
        assert!(chunk.end_of_speech);
        assert_eq!(&chunk.samples[..73], &[7; 73]);
        assert_eq!(&chunk.samples[73..], &[0; 167]);
        assert!(
            pending.is_some(),
            "generation completion is not speaker retirement"
        );
        assert_eq!(
            owner.snapshot(now()).unwrap().owner(),
            Some(reply.as_ref().unwrap().owner)
        );
        let current = reply.as_ref().unwrap();
        assert_eq!(current.start.opened().unwrap().by, OpenedBy::Complete);
        assert_eq!(
            current.delivery_outcome(),
            if failed {
                "provider_failed"
            } else {
                "audio_written_unscored"
            }
        );
        dispatch_speaker_chunk(
            &mut owner,
            &mut reply,
            &mut parent.data,
            &mut pending,
            &mut sequence,
            &mut snapshot,
            &mut PlaybackMask::new(TurnTaking::V1Baseline),
        )
        .unwrap();
        assert!(speaker.data.receive::<SpeakerChunk>().unwrap().is_none());
    }
}

#[test]
fn cancellation_during_hold_discards_it_without_lending_age_to_the_next_owner() {
    let dir = SessionDirectory::create().unwrap();
    let (mut parent, mut speaker) = pair(&dir, "speaker");
    let (mut owner, mut current) = fresh_reply();
    current.append_provider_audio(vec![7; 3_120], monotonic_us());
    let old_owner = current.owner;
    let mut reply = Some(current);
    let mut pending = None;
    let mut sequence = 0;
    let mut snapshot = owner.snapshot(now()).unwrap();
    dispatch_speaker_chunk(
        &mut owner,
        &mut reply,
        &mut parent.data,
        &mut pending,
        &mut sequence,
        &mut snapshot,
        &mut PlaybackMask::new(TurnTaking::V1Baseline),
    )
    .unwrap();
    cancel_reply(
        &mut owner,
        &mut reply,
        &mut SessionState::default(),
        "user_interrupted",
    )
    .unwrap();
    dispatch_speaker_chunk(
        &mut owner,
        &mut reply,
        &mut parent.data,
        &mut pending,
        &mut sequence,
        &mut snapshot,
        &mut PlaybackMask::new(TurnTaking::V1Baseline),
    )
    .unwrap();
    assert!(speaker.data.receive::<SpeakerChunk>().unwrap().is_none());
    assert!(pending.is_none());
    assert!(reply.is_none());
    assert_ne!(owner.snapshot(now()).unwrap().owner(), Some(old_owner));
    let (_, mut next) = fresh_reply();
    next.append_provider_audio(vec![9; 960], 1_000_000);
    assert!(!next.may_start(1_000_000));
    assert!(next.start.opened().is_none());
}

#[test]
fn a_satisfied_gate_does_not_override_revocation_or_a_newer_owner() {
    for privacy in [false, true] {
        let dir = SessionDirectory::create().unwrap();
        let (mut parent, mut speaker) = pair(&dir, "speaker");
        let (mut owner, mut current) = fresh_reply();
        current.append_provider_audio(vec![7; 4_800], monotonic_us());
        let old = current.owner;
        if privacy {
            owner
                .set_microphone_permission(now(), Permission::Denied)
                .unwrap();
        } else {
            owner.admit(now(), AdmittedInput::Interruption).unwrap();
        }
        let mut reply = Some(current);
        let mut pending = None;
        let mut sequence = 0;
        let mut snapshot = owner.snapshot(now()).unwrap();
        assert!(
            dispatch_speaker_chunk(
                &mut owner,
                &mut reply,
                &mut parent.data,
                &mut pending,
                &mut sequence,
                &mut snapshot,
                &mut PlaybackMask::new(TurnTaking::V1Baseline),
            )
            .is_err()
        );
        assert!(speaker.data.receive::<SpeakerChunk>().unwrap().is_none());
        assert!(pending.is_none());
        assert_ne!(owner.snapshot(now()).unwrap().owner(), Some(old));
    }
}

#[test]
fn continuous_generation_is_not_rebuffered_and_previous_final_cursor_still_blocks() {
    let (_, mut current) = fresh_reply();
    current.append_provider_audio(vec![7; 480], 1_000);
    current.generation_done = true;
    assert!(current.may_start(1_000));
    let opened = current.start.opened();
    current.pcm.clear();
    current.final_chunk = Some(44);
    current.append_provider_audio(vec![9; 480], 1_000_000);
    assert!(!current.may_start(1_000_000));
    assert!(current.next_speaker_chunk().is_none());
    current.final_chunk = None; // Corresponds to the real retirement path.
    assert!(current.may_start(1_000_000));
    assert_eq!(current.start.opened(), opened);
    let (samples, final_chunk) = current.next_speaker_chunk().unwrap();
    assert_eq!(samples, vec![9; 240]);
    assert!(!final_chunk);
    assert_eq!(current.pcm, vec![9; 240]);
}

#[test]
fn empty_terminal_and_all_zero_pcm_do_not_invent_audio_or_an_audible_answer() {
    let (_, mut current) = fresh_reply();
    current.generation_done = true;
    assert!(!current.may_start(1_000_000));
    assert!(current.start.opened().is_none());
    assert!(current.next_speaker_chunk().is_none());
    assert_eq!(current.delivery_outcome(), "no_audio_answer");
    current.append_provider_audio(vec![0; 240], 1_000_000);
    current.generation_done = true;
    current.had_audio = true;
    assert!(current.may_start(1_000_000));
    assert_eq!(current.next_speaker_chunk(), Some((vec![0; 240], true)));
    assert_eq!(current.delivery_outcome(), "audio_written_unscored");
}

#[test]
fn arrival_need_does_not_certify_playable_coverage_while_final_block_is_held() {
    let (_, mut current) = fresh_reply();
    current.append_provider_audio(vec![7; 4_800], 0);
    assert!(current.may_start(0));
    for slot in 0..19 {
        assert!(current.may_start(slot * 10_000));
        assert_eq!(current.next_speaker_chunk(), Some((vec![7; 240], false)));
    }
    // The arrival-only accounting has 200 ms in hand, but the 190 ms slot
    // cannot dispatch its held final block before completion or more PCM.
    assert_eq!(current.pcm.len(), 240);
    assert!(!current.may_start(190_000));
    assert!(current.next_speaker_chunk().is_none());
    current.append_provider_audio(vec![9; 4_800], 195_000);
    assert_eq!(current.need.deficit_us(), 0);
    assert_eq!(current.need.horizon_us(), 0);
    assert!(current.may_start(200_000));
    assert_eq!(current.next_speaker_chunk(), Some((vec![7; 240], false)));
}

#[test]
fn trace_labels_queue_age_holdback_and_censored_cancellation_without_latency_claim() {
    let (mut owner, mut current) = fresh_reply();
    current.append_provider_audio(vec![7; 3_120], 1_000);
    assert!(current.may_start(71_000));
    let evidence = current.start_trace();
    assert_eq!(evidence["age_since_first_audio_us"], 70_000);
    assert_eq!(evidence["opened_at_us"], 71_000);
    assert_eq!(evidence["queued_frames_at_open"], 3_120);
    assert_eq!(evidence["included_final_holdback_frames"], 240);
    assert!(evidence["added_latency_us"].is_null());
    assert_eq!(evidence["opened_by_complete_flag"], false);
    let mut reply = Some(current);
    let mut trace = SessionState::default();
    cancel_reply(&mut owner, &mut reply, &mut trace, "user_interrupted").unwrap();
    assert_eq!(trace.len(), 1);
    assert_eq!(trace[0]["start_gate"], evidence);
    assert_eq!(trace[0]["start_need_censored"], true);
    assert_eq!(trace[0]["outcome"], "user_interrupted");
}

#[test]
fn held_speech_does_not_block_priority_provider_control() {
    let dir = SessionDirectory::create().unwrap();
    let (mut provider_parent, mut provider_child) = pair(&dir, "provider");
    let (mut speaker_parent, mut speaker_child) = pair(&dir, "speaker");
    let (mut owner, mut current) = fresh_reply();
    current.append_provider_audio(vec![7; 960], monotonic_us());
    let mut reply = Some(current);
    let mut pending = None;
    let mut sequence = 0;
    let mut snapshot = owner.snapshot(now()).unwrap();
    dispatch_speaker_chunk(
        &mut owner,
        &mut reply,
        &mut speaker_parent.data,
        &mut pending,
        &mut sequence,
        &mut snapshot,
        &mut PlaybackMask::new(TurnTaking::V1Baseline),
    )
    .unwrap();
    let lost = unavailable_link(1, 1);
    provider_child
        .control
        .send(WorkerEvent::ProviderLink { link: lost })
        .unwrap();
    let mut cloud = CloudAvailability::default();
    let mut remaining = 16;
    let mut trace = SessionState::default();
    assert!(
        service_provider_control(
            &mut provider_parent.control,
            &mut cloud,
            &mut remaining,
            &mut trace,
        )
        .unwrap()
    );
    assert_eq!(remaining, 15);
    assert!(!cloud.available());
    assert!(pending.is_none());
    assert!(
        speaker_child
            .data
            .receive::<SpeakerChunk>()
            .unwrap()
            .is_none()
    );
    assert_eq!(reply.as_ref().unwrap().pcm, vec![7; 960]);
    // Outage status alone does not discard already-owned queued speech.
    assert_eq!(
        owner.snapshot(now()).unwrap().owner(),
        Some(reply.unwrap().owner)
    );
}

#[test]
fn actual_provider_capacity_and_speaker_channel_continue_while_start_is_held() {
    use crate::provider_flow::OutputSender;
    let dir = SessionDirectory::create().unwrap();
    let (mut provider_parent, mut provider_child) = pair(&dir, "provider");
    let (mut speaker_parent, mut speaker_child) = pair(&dir, "speaker");
    let (mut owner, current) = fresh_reply();
    let mut reply = Some(current);
    let mut sender = OutputSender::default();
    let mut receiver = OutputReceiver::default();
    let mut pending = None;
    let mut sequence = 0;
    let mut snapshot = owner.snapshot(now()).unwrap();
    let mut mask = PlaybackMask::new(TurnTaking::V1Baseline);
    for (packet, frames) in [960, 960, 240, 960, 960, 960].into_iter().enumerate() {
        let event = provider_audio(packet as u64 + 1, vec![7; frames]);
        assert!(sender.try_send(|| provider_child.data.send(event)).unwrap());
        let event = ProviderSlice::new(monotonic_us())
            .receive(&mut provider_parent.data, monotonic_us())
            .unwrap()
            .unwrap();
        receiver.received().unwrap();
        observe_provider_output(&mut CloudAvailability::default(), &mut reply, &event).unwrap();
        let ProviderOutput::Audio {
            request, samples, ..
        } = event
        else {
            panic!("audio expected")
        };
        let current = reply.as_mut().unwrap();
        assert_eq!(current.owner.turn(), request);
        current.append_provider_audio(samples, monotonic_us());
        receiver
            .replenish(current.pcm.len(), &mut provider_parent.control)
            .unwrap();
        let Some(Control::ProviderOutputCapacity { through }) =
            provider_child.control.receive().unwrap()
        else {
            panic!("capacity grant expected")
        };
        sender.grant(through).unwrap();
        dispatch_speaker_chunk(
            &mut owner,
            &mut reply,
            &mut speaker_parent.data,
            &mut pending,
            &mut sequence,
            &mut snapshot,
            &mut mask,
        )
        .unwrap();
        if packet < 5 {
            assert!(
                speaker_child
                    .data
                    .receive::<SpeakerChunk>()
                    .unwrap()
                    .is_none()
            );
            assert!(pending.is_none());
            assert!(
                mask.available(monotonic_us()),
                "holding unplayed PCM must not mask input"
            );
        } else {
            let chunk = speaker_child
                .data
                .receive::<SpeakerChunk>()
                .unwrap()
                .unwrap();
            assert_eq!(chunk.samples, vec![7; 240]);
            assert_eq!(chunk.permit.owner(), reply.as_ref().unwrap().owner);
            assert!(!chunk.end_of_speech);
            assert!(pending.is_some());
        }
    }
    assert_eq!(sequence, 1);
    assert_eq!(reply.as_ref().unwrap().pcm.len(), 4_800);
    assert!(
        reply.as_ref().unwrap().playback.is_none(),
        "dispatch is not accepted speech"
    );
}

/// `LAMP_SECOND_PLAYOUT_START_MS` (#2135): a reply begun while the second
/// route's worker is the provider takes the second route's own start
/// horizon; one begun while Gemini's is, or with the switch unset, the
/// unit's, continuity included. Only what `--playout-start-ms` takes is a
/// horizon: anything else leaves the unit's.
#[test]
fn the_second_route_starts_on_its_own_horizon_only_while_it_answers() {
    let unit = PlayoutStart::new(350).unwrap();
    let second = second_playout_start_from(Some("120"));
    assert_eq!(second, Some(PlayoutStart::new(120).unwrap()));
    assert_eq!(playout_for(unit, second, "second").as_micros(), 120_000);
    assert_eq!(playout_for(unit, second, "provider").as_micros(), 350_000);
    assert_eq!(playout_for(unit, None, "second").as_micros(), 350_000);
    let continuity = PlayoutStart::continuity();
    assert_eq!(playout_for(continuity, None, "second"), continuity);
    assert_eq!(playout_for(continuity, second, "provider"), continuity);
    assert_eq!(
        playout_for(unit, second_playout_start_from(Some("0")), "second").as_micros(),
        0
    );
    for refused in [
        None,
        Some(""),
        Some("501"),
        Some("-1"),
        Some("1e2"),
        Some(" 120"),
    ] {
        assert_eq!(second_playout_start_from(refused), None, "{refused:?}");
    }
}
