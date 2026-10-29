//! Retire actual output ownership before classifying face-question delivery.
use super::{
    Controller, PendingSpeaker, ProviderInput, Reply, Result, Snapshot, monotonic_us, now,
};
use crate::remember_speaker::consent::Questions;

pub(super) fn completed(
    owner: &mut Controller,
    current: &mut Reply,
    pending: &Option<PendingSpeaker>,
    questions: &mut Questions,
) -> Result<(Snapshot, Option<ProviderInput>)> {
    owner.complete_turn(now(), current.owner)?;
    current.complete_requested_audio(monotonic_us());
    let snapshot = owner.snapshot(now())?;
    let receipt = questions.finished(
        current.owner.boot().into(),
        current.owner.turn(),
        current.heard_whole(pending, snapshot),
        monotonic_us(),
    );
    Ok((snapshot, receipt))
}

/// Invalidate a prior question as soon as unrelated output reaches the
/// speaker, before a following microphone request can be flushed to a worker.
pub(super) fn interposed(
    provider_worker: lamp_turn_owner::BootId,
    reply: &Option<Reply>,
    questions: &mut Questions,
    queue: &mut std::collections::VecDeque<ProviderInput>,
    now_us: u64,
) {
    if let Some(current) = reply.as_ref().filter(|current| current.reached_speaker())
        && let Some(receipt) = questions.other_output(
            current.owner.boot().into(),
            crate::remember_speaker::consent::OutputKey {
                worker: provider_worker.into(),
                session: current.cloud.session,
                request: current.owner.turn(),
            },
            now_us,
        )
    {
        queue.push_front(receipt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::SpeechPlayback;
    use crate::{
        coordinator::{
            PlaybackMask, SessionDirectory, SessionState, SpeakerChunk, SpeakerControlIo,
            TurnTaking, WorkerChannels, WorkerEvent, dispatch_speaker_chunk, new_boot,
            service_speaker_control,
        },
        memory_scope::{RequestKey, RequestScope, ScopeSnapshot},
        remember_speaker::{Proof, TEST_CONSENT, consent, generation, test_consent_exchange},
    };
    use lamp_audio::ownership::{PlaybackCursor, QueuedRange};
    use serde_json::json;
    use std::{collections::VecDeque, io};

    struct Events(VecDeque<WorkerEvent>);
    impl SpeakerControlIo for Events {
        fn next_speaker_event(&mut self) -> io::Result<Option<WorkerEvent>> {
            Ok(self.0.pop_front())
        }
        fn publish_speaker_snapshot(&mut self, _: Snapshot) -> io::Result<()> {
            Ok(())
        }
    }
    fn key(request: u64) -> RequestKey {
        RequestKey {
            worker: lamp_turn_owner::BootId::new([78; 16]).unwrap(),
            session: 91,
            request,
        }
    }
    fn candidate() -> consent::Candidate {
        consent::clear();
        let at = lamp_ipc::monotonic_us() - 2_000_000;
        let mut proof = Proof::heard(
            "eyes/f1".into(),
            at - 10_000,
            at - 1_000,
            "My name is Kesh.",
            generation(),
        )
        .unwrap();
        let owner = RequestScope::new(key(8001), ScopeSnapshot::room());
        let candidate = consent::heard(&mut proof, owner.lease().key(), at).unwrap();
        drop(owner);
        candidate
    }
    fn questions(current: &Reply, privacy: u64) -> Questions {
        let candidate = candidate();
        let mut questions = Questions::default();
        questions.offer(candidate.clone(), privacy, lamp_ipc::monotonic_us());
        questions.source_finished(candidate.request, true);
        assert!(questions.bind(
            &candidate,
            current.owner.boot().into(),
            current.owner.turn()
        ));
        assert!(consent::bind(
            &candidate,
            current.owner.boot().into(),
            current.owner.turn()
        ));
        questions
    }
    /// Real production dispatch and both accepted/final-retired typed receipts.
    /// This is a host fixture of the speaker protocol, not an acoustic claim.
    fn delivered_reply() -> (Controller, Reply) {
        let (owner, current, _) = super::super::tests::active_reply();
        accepted_and_retired(owner, current)
    }
    fn accepted_and_retired(mut owner: Controller, mut current: Reply) -> (Controller, Reply) {
        let dir = SessionDirectory::create().unwrap();
        let parent_boot = new_boot().unwrap();
        let worker_boot = new_boot().unwrap();
        let mut parent =
            WorkerChannels::bind(&dir.path, "speaker", parent_boot, worker_boot, false).unwrap();
        let mut speaker =
            WorkerChannels::bind(&dir.path, "speaker", parent_boot, worker_boot, true).unwrap();
        parent.connect(&dir.path, "speaker", false).unwrap();
        speaker.connect(&dir.path, "speaker", true).unwrap();
        current.pcm = vec![11; 240].into();
        current.had_audio = true;
        current.generation_done = true;
        current.provider_idle = true;
        current.cloud.complete(true).unwrap();
        let mut reply = Some(current);
        let mut pending = None;
        let mut sequence = 0;
        let mut snapshot = owner.snapshot(now()).unwrap();
        let mut mask = PlaybackMask::new(TurnTaking::VadOnly);
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
        assert!(chunk.end_of_speech);
        let turn = reply.as_ref().unwrap().owner;
        let observed = monotonic_us();
        let first = PlaybackCursor { epoch: 1, frame: 0 };
        let end = PlaybackCursor {
            epoch: 1,
            frame: 240,
        };
        let mut playback = SpeechPlayback::default();
        playback
            .speech_accepted(turn, chunk.chunk_sequence, first, end, observed, true)
            .unwrap();
        let retired = playback
            .take_retired(QueuedRange {
                epoch: 1,
                retired_through: 240,
                accepted_through: 240,
            })
            .unwrap();
        let mut events = Events(VecDeque::from([
            WorkerEvent::Playback {
                owner: turn,
                first_sample: first,
                end_sample: end,
                queue_observed_at_us: observed,
                turn: turn.turn(),
                sequence: chunk.chunk_sequence,
                queued_frames: 240,
                accepted_frames: 240,
                observed_at_us: observed,
            },
            WorkerEvent::SpeechRetired {
                owner: turn,
                sequence: retired.sequence,
                final_sample: retired.cursor,
                observed_at_us: observed,
            },
        ]));
        let mut trace = SessionState::default();
        trace.receipts.admitted(turn, observed - 1_000).unwrap();
        trace
            .receipts
            .dispatched(turn, chunk.chunk_sequence)
            .unwrap();
        let mut last_publish = 0;
        let mut budget = 8;
        let mut lapsed = None;
        service_speaker_control(
            &mut events,
            &mut owner,
            &mut reply,
            &mut pending,
            &mut mask,
            &mut snapshot,
            &mut last_publish,
            &mut budget,
            &mut trace,
            &mut lapsed,
        )
        .unwrap();
        assert!(pending.is_none());
        let current = reply.unwrap();
        assert!(current.settled(&pending));
        assert!(current.whole_provider_output(&pending));
        assert!(
            !current.heard_whole(&pending, snapshot),
            "C still owns output before completion"
        );
        (owner, current)
    }
    fn apply(receipt: ProviderInput) -> bool {
        let ProviderInput::FaceConsentDelivery {
            candidate,
            voice_boot,
            request,
            whole,
            delivered_us,
            by_output,
            pending_only,
            ..
        } = receipt
        else {
            panic!("completion must preserve the face-question purpose")
        };
        if !whole && pending_only {
            consent::expired(&candidate, voice_boot, request);
        } else {
            consent::delivered_from(
                &candidate,
                voice_boot,
                request,
                whole,
                delivered_us,
                by_output,
            );
        }
        whole
    }
    fn reply_sends() -> Vec<serde_json::Value> {
        // Synchronize distinct actual monotonic ticks: the reply begins
        // strictly after delivery even on a coarse host clock. No deadline is
        // extended, and a stalled clock fails within the outer test bound.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        let next_tick = |after| loop {
            let now = lamp_ipc::monotonic_us();
            if now > after {
                break now;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "monotonic clock stalled"
            );
            std::thread::yield_now();
        };
        let began = next_tick(lamp_ipc::monotonic_us());
        let now = next_tick(began);
        let mut proof = Proof::heard(
            "eyes/f1".into(),
            began,
            now,
            "Yes, my name is Kesh.",
            generation(),
        )
        .unwrap();
        let _ = consent::heard(&mut proof, key(8002), now);
        test_consent_exchange(
            proof,
            key(8002),
            json!({"name":"Kesh"}),
            true,
            json!({"remembered":"Kesh"}),
        )
        .1
    }

    #[test]
    fn real_playback_completion_releases_c_before_whole_can_admit_b() {
        let _serial = TEST_CONSENT.lock().unwrap_or_else(|p| p.into_inner());
        let (mut owner, mut current) = delivered_reply();
        let mut questions = questions(
            &current,
            owner.snapshot(now()).unwrap().microphone_generation(),
        );
        let (snapshot, receipt) =
            completed(&mut owner, &mut current, &None, &mut questions).unwrap();
        assert!(snapshot.owner().is_none());
        assert!(current.heard_whole(&None, snapshot));
        assert!(apply(receipt.unwrap()));
        assert!(
            questions
                .finished(
                    current.owner.boot().into(),
                    current.owner.turn(),
                    true,
                    monotonic_us()
                )
                .is_none(),
            "another Whole callback cannot renew the delivered question"
        );
        let requests = reply_sends();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["remember_speaker_consented"]["name"], "Kesh");
        consent::clear();
    }

    fn unsaid_reply() -> (Controller, Reply) {
        let (mut owner, mut current, _) = super::super::tests::active_reply();
        // The fixture's planned playback ends with no speaker acceptance.
        owner
            .playback_ended(now(), current.playback.take().unwrap())
            .unwrap();
        current.pcm.clear();
        current.had_audio = false;
        current.generation_done = true;
        current.provider_idle = true;
        current.cloud.complete(true).unwrap();
        assert!(current.settled(&None));
        assert!(!current.whole_provider_output(&None));
        (owner, current)
    }

    #[test]
    fn a_playback_gap_or_an_unsaid_reply_never_grants_face_consent() {
        let _serial = TEST_CONSENT.lock().unwrap_or_else(|p| p.into_inner());
        for kind in ["gap", "unsaid"] {
            let (mut owner, mut current) = if kind == "gap" {
                delivered_reply()
            } else {
                unsaid_reply()
            };
            let mut questions = questions(
                &current,
                owner.snapshot(now()).unwrap().microphone_generation(),
            );
            if kind == "gap" {
                current.playback_gaps = 1;
            }
            let (_, receipt) = completed(&mut owner, &mut current, &None, &mut questions).unwrap();
            assert!(!apply(receipt.unwrap()), "{kind}");
            assert!(reply_sends().is_empty(), "{kind}");
        }
        consent::clear();
    }

    #[test]
    fn a_different_completed_output_owner_does_not_complete_the_face_question() {
        let _serial = TEST_CONSENT.lock().unwrap_or_else(|p| p.into_inner());
        let (mut question_controller, question_owner) = delivered_reply();
        let mut questions = questions(
            &question_owner,
            question_controller
                .snapshot(now())
                .unwrap()
                .microphone_generation(),
        );
        let (mut other_owner, mut other) = delivered_reply();
        let (_, receipt) = completed(&mut other_owner, &mut other, &None, &mut questions).unwrap();
        assert!(receipt.is_none());
        assert!(reply_sends().is_empty());
        consent::clear();
    }

    #[test]
    fn a_stop_after_question_completion_revokes_it_even_with_no_current_output_owner() {
        let _serial = TEST_CONSENT.lock().unwrap_or_else(|p| p.into_inner());
        let (mut owner, mut current) = delivered_reply();
        let privacy = owner.snapshot(now()).unwrap().microphone_generation();
        let mut questions = questions(&current, privacy);
        let (snapshot, receipt) =
            completed(&mut owner, &mut current, &None, &mut questions).unwrap();
        assert!(snapshot.owner().is_none());
        assert!(apply(receipt.unwrap()));
        assert!(
            questions.unsettled(None, privacy, monotonic_us()).is_none(),
            "normal C retirement preserves the separate bounded question"
        );
        assert!(!apply(questions.cancel(monotonic_us()).unwrap()));
        assert!(reply_sends().is_empty());
        consent::clear();
    }

    #[test]
    fn duplicate_whole_receipts_cannot_renew_the_original_answer_window() {
        let _serial = TEST_CONSENT.lock().unwrap_or_else(|p| p.into_inner());
        consent::clear();
        let now = lamp_ipc::monotonic_us();
        let original_end = now - 20_000_000;
        let mut a = Proof::heard(
            "eyes/f1".into(),
            original_end - 10_000,
            original_end,
            "My name is Kesh.",
            generation(),
        )
        .unwrap();
        let candidate = consent::heard(&mut a, key(8001), now - 15_000_000).unwrap();
        let boot = [83; 16];
        let request = 9001;
        assert!(consent::bind(&candidate, boot, request));
        let original_delivery = now - consent::WITHIN_US - 1_000;
        assert!(candidate.fresh(original_delivery));
        consent::delivered(&candidate, boot, request, true, original_delivery);
        // A duplicate is fresh enough on its own; only preserving the first
        // receipt prevents it from incorrectly renewing an expired answer.
        consent::delivered(&candidate, boot, request, true, now - 1_000);
        assert!(reply_sends().is_empty());
        consent::clear();
    }

    #[test]
    fn actual_unrelated_written_playback_cancels_c_before_the_next_raw_reply_is_flushed() {
        use crate::coordinator::{
            CloudAvailability, CloudRequest, Permission, PlayoutStart, ProviderLink,
            ProviderLinkState, commit_written_turn, flush_provider_input,
        };
        let _serial = TEST_CONSENT.lock().unwrap_or_else(|p| p.into_inner());
        let (mut owner, mut c) = delivered_reply();
        let privacy = owner.snapshot(now()).unwrap().microphone_generation();
        let mut questions = questions(&c, privacy);
        let (_, receipt) = completed(&mut owner, &mut c, &None, &mut questions).unwrap();
        assert!(apply(receipt.unwrap()));
        let dir = SessionDirectory::create().unwrap();
        let boot = new_boot().unwrap();
        let peer_boot = new_boot().unwrap();
        let mut parent =
            WorkerChannels::bind(&dir.path, "provider", boot, peer_boot, false).unwrap();
        let mut peer = WorkerChannels::bind(&dir.path, "provider", boot, peer_boot, true).unwrap();
        parent.connect(&dir.path, "provider", false).unwrap();
        peer.connect(&dir.path, "provider", true).unwrap();
        let mut cloud = CloudAvailability::default();
        cloud
            .observe(
                ProviderLink {
                    revision: 1,
                    session: 1,
                    observed_at_us: monotonic_us(),
                    source_clock: None,
                    state: ProviderLinkState::Ready {
                        setup_us: 1,
                        attempts: 1,
                        context: crate::provider_link::LinkContext::Initial,
                        outage_started_at_us: None,
                        after: None,
                    },
                },
                monotonic_us(),
            )
            .unwrap();
        let mut d = None;
        let mut queue = VecDeque::new();
        let mut trace = SessionState::default();
        commit_written_turn(
            &mut owner,
            &mut d,
            &mut queue,
            "Would you like the weather?",
            false,
            "consent_fixture_unrelated_written",
            1,
            PlayoutStart::default(),
            &mut trace,
            |_| Ok(()),
        )
        .unwrap();
        flush_provider_input(&cloud, &mut d, &mut queue, &mut parent.data).unwrap();
        assert!(
            matches!(peer.data.receive::<ProviderInput>().unwrap(),Some(ProviderInput::Written {text,..})
            if text=="Would you like the weather?")
        );
        let mut d = d.unwrap();
        d.cloud.confirm_start(1).unwrap();
        let (_, d) = accepted_and_retired(owner, d);
        assert!(d.reached_speaker());
        let (mut b_controller, mut b, _) = super::super::tests::active_reply();
        b.cloud = CloudRequest::new(1).unwrap();
        let b_request = b.owner.turn();
        queue.push_back(ProviderInput::Start {
            request: b_request,
            session: 1,
            privacy_generation: privacy,
            continues: false,
            continues_cut_off: false,
            over_the_lamp: false,
            after_stop: false,
            follows_on: false,
            over_the_lamps_voice: false,
            heard_of_cut: None,
            soft_lead_us: 0,
        });
        interposed(
            key(8002).worker,
            &Some(d),
            &mut questions,
            &mut queue,
            monotonic_us(),
        );
        assert!(matches!(
            queue.front(),
            Some(ProviderInput::FaceConsentDelivery { whole: false, .. })
        ));
        let mut b = Some(b);
        flush_provider_input(&cloud, &mut b, &mut queue, &mut parent.data).unwrap();
        let cancel = peer.data.receive::<ProviderInput>().unwrap().unwrap();
        assert!(matches!(
            cancel,
            ProviderInput::FaceConsentDelivery { whole: false, .. }
        ));
        assert!(!apply(cancel));
        assert!(
            matches!(peer.data.receive::<ProviderInput>().unwrap(),Some(ProviderInput::Start {request,..})
            if request==b_request)
        );
        assert!(peer.data.receive::<ProviderInput>().unwrap().is_none());
        assert!(queue.is_empty());
        assert!(
            reply_sends().is_empty(),
            "B cannot answer C after an unrelated actual spoken question"
        );
        // The control does not need a privacy close to deny the old question.
        assert_eq!(
            b_controller
                .snapshot(now())
                .unwrap()
                .microphone_permission(),
            Permission::Allowed
        );
        consent::clear();
    }

    #[test]
    fn a_provider_speaker_lapse_cuts_d_audio_but_preserves_exact_c_metadata() {
        use crate::coordinator::cut_provider_after_speaker_lapse;
        let _serial = TEST_CONSENT.lock().unwrap_or_else(|p| p.into_inner());
        for matching_owner in [true, false] {
            let (mut c_owner, mut c) = delivered_reply();
            let privacy = c_owner.snapshot(now()).unwrap().microphone_generation();
            let mut questions = questions(&c, privacy);
            let (_, receipt) = completed(&mut c_owner, &mut c, &None, &mut questions).unwrap();
            let metadata = receipt.unwrap();
            let exact_metadata = serde_json::to_value(&metadata).unwrap();
            let (mut owner, d, permit) = super::super::tests::active_reply();
            let d_owner = d.owner;
            let stale_owner = super::super::tests::active_reply().1.owner;
            let mut reply = Some(d);
            let mut pending = Some(PendingSpeaker {
                origin: crate::coordinator::SpeechOrigin::Provider,
                sequence: 7,
                permit,
                accepted: 0,
                final_chunk: false,
                report_due_us: monotonic_us() + 100_000,
            });
            let mut queue = VecDeque::from([
                metadata,
                ProviderInput::Audio {
                    request: d_owner.turn(),
                    privacy_generation: privacy,
                    sequence: 1,
                    read_completed_at_us: monotonic_us(),
                    samples: vec![3; 160],
                },
            ]);
            let unchanged_queue = serde_json::to_value(&queue).unwrap();
            let mut trace = SessionState::default();
            let cut = cut_provider_after_speaker_lapse(
                if matching_owner { d_owner } else { stale_owner },
                &mut owner,
                &mut reply,
                &mut queue,
                &mut pending,
                &mut trace,
            )
            .unwrap();
            assert_eq!(cut, matching_owner);
            if matching_owner {
                assert!(reply.is_none());
                assert!(owner.snapshot(now()).unwrap().owner().is_none());
                assert!(pending.is_none());
                assert_eq!(queue.len(), 1);
                assert_eq!(
                    serde_json::to_value(queue.front().unwrap()).unwrap(),
                    exact_metadata
                );
            } else {
                assert_eq!(reply.as_ref().unwrap().owner, d_owner);
                assert_eq!(owner.snapshot(now()).unwrap().owner(), Some(d_owner));
                assert_eq!(pending.as_ref().unwrap().sequence, 7);
                assert_eq!(serde_json::to_value(&queue).unwrap(), unchanged_queue);
                assert_eq!(trace.len(), 0);
            }
        }
        consent::clear();
    }
}
