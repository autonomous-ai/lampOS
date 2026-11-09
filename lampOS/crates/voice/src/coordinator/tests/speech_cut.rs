//! Production IPC and coordinator receipt logic; no simulated acoustic claim.
use super::*;
use crate::speech_cut::ProviderCut;

struct CutIo {
    speaker: WorkerChannels,
    provider: WorkerChannels,
}
impl SpeakerControlIo for CutIo {
    fn next_speaker_event(&mut self) -> io::Result<Option<WorkerEvent>> {
        self.speaker.control.receive()
    }
    fn publish_speaker_snapshot(&mut self, _: Snapshot) -> io::Result<()> {
        Ok(())
    }
    fn speech_cut_applied(&mut self, _: BootId, cut: ProviderCut) -> io::Result<()> {
        self.provider
            .control
            .send(Control::ProviderSpeechCutApplied {
                request: cut.request,
                through: cut.through,
            })
    }
}

#[test]
fn priority_cut_clears_pcm_and_waits_for_physical_ack_before_releasing_same_request() {
    let dir = SessionDirectory::create().unwrap();
    let (speaker, mut speaker_peer) = pair(&dir, "speaker");
    let (provider, mut provider_peer) = pair(&dir, "provider");
    let mut io = CutIo { speaker, provider };
    let (mut owner, current, permit) = active_reply();
    let turn = current.owner;
    let original_plan = current.plan;
    let mut reply = Some(current);
    let mut pending = Some(PendingSpeaker {
        origin: SpeechOrigin::Provider,
        sequence: 9,
        permit,
        accepted: 0,
        final_chunk: true,
        report_due_us: monotonic_us() + 100_000,
    });
    let mut trace = SessionState::default();
    trace.receipts.dispatched(turn, 9).unwrap();
    let mut cuts = super::super::speech_cut::State::default();
    let boot = new_boot().unwrap();
    let cut = ProviderCut {
        request: turn.turn(),
        through: 42,
        admitted_at_us: monotonic_us(),
    };
    cuts.admit(
        boot,
        cut,
        &mut reply,
        9,
        |message| io.speaker.control.send(message),
        &mut trace,
    )
    .unwrap();
    assert!(cuts.waiting());
    assert!(reply.as_ref().unwrap().pcm.is_empty());
    assert!(reply.as_ref().unwrap().playback.is_some());
    assert!(
        pending.is_some(),
        "old accepted receipts retain their original pending permit"
    );
    assert!(
        provider_peer
            .control
            .receive::<Control>()
            .unwrap()
            .is_none()
    );
    assert!(
        matches!(speaker_peer.control.receive::<Control>().unwrap(), Some(Control::CutSpeech { owner, through: 9 }) if owner == turn)
    );
    let packet = |sequence| ProviderOutput::Audio {
        request: turn.turn(),
        sequence,
        provider_event_at_us: monotonic_us(),
        source_frames: 1,
        source_offset: 0,
        samples: vec![123],
    };
    assert!(cuts.discards(boot, &packet(42)));
    assert!(!cuts.discards(boot, &packet(43)));
    assert!(!cuts.discards(new_boot().unwrap(), &packet(42)));
    let mut snapshot = owner.snapshot(now()).unwrap();
    speaker_peer
        .control
        .send(WorkerEvent::SpeechCutApplied {
            owner: turn,
            through: 10,
            applied_at_us: monotonic_us(),
        })
        .unwrap();
    service_speaker_control_inner(
        &mut io,
        &mut owner,
        &mut reply,
        &mut pending,
        &mut PlaybackMask::new(TurnTaking::VadOnly),
        &mut snapshot,
        &mut 0,
        &mut 8,
        &mut trace,
        &mut None,
        Some(&mut cuts),
    )
    .unwrap();
    assert!(
        cuts.waiting(),
        "an unsent future cutoff cannot release fresh speech"
    );
    assert!(
        provider_peer
            .control
            .receive::<Control>()
            .unwrap()
            .is_none()
    );
    // This peer emits an acknowledgement, not evidence of a real device drop.
    speaker_peer
        .control
        .send(WorkerEvent::SpeechCutApplied {
            owner: turn,
            through: 9,
            applied_at_us: monotonic_us(),
        })
        .unwrap();

    service_speaker_control_inner(
        &mut io,
        &mut owner,
        &mut reply,
        &mut pending,
        &mut PlaybackMask::new(TurnTaking::VadOnly),
        &mut snapshot,
        &mut 0,
        &mut 8,
        &mut trace,
        &mut None,
        Some(&mut cuts),
    )
    .unwrap();
    assert!(!cuts.waiting());
    assert!(pending.is_none());
    let current = reply.as_ref().unwrap();
    assert_eq!(current.owner, turn);
    assert_eq!(current.plan, original_plan);
    assert!(current.playback.is_none());
    assert_eq!(current.playback_gaps, 1);
    assert_eq!(owner.snapshot(now()).unwrap().owner(), Some(turn));
    assert!(
        matches!(provider_peer.control.receive::<Control>().unwrap(), Some(Control::ProviderSpeechCutApplied { request, through: 42 }) if request == turn.turn())
    );
    assert!(
        cuts.discards(boot, &packet(42)),
        "late audio stays fenced after ACK"
    );
    // A later provider cut may reuse the same speaker watermark: no new
    // chunk crossed the barrier. Its cached physical ACK remains sufficient.
    let cached_at = monotonic_us();
    let second = ProviderCut {
        request: turn.turn(),
        through: 43,
        admitted_at_us: monotonic_us(),
    };
    cuts.admit(
        boot,
        second,
        &mut reply,
        9,
        |message| io.speaker.control.send(message),
        &mut trace,
    )
    .unwrap();
    speaker_peer.control.receive::<Control>().unwrap().unwrap();
    speaker_peer
        .control
        .send(WorkerEvent::SpeechCutApplied {
            owner: turn,
            through: 9,
            applied_at_us: cached_at,
        })
        .unwrap();
    service_speaker_control_inner(
        &mut io,
        &mut owner,
        &mut reply,
        &mut pending,
        &mut PlaybackMask::new(TurnTaking::VadOnly),
        &mut snapshot,
        &mut 0,
        &mut 8,
        &mut trace,
        &mut None,
        Some(&mut cuts),
    )
    .unwrap();
    assert!(!cuts.waiting());
    assert!(matches!(
        provider_peer.control.receive::<Control>().unwrap(),
        Some(Control::ProviderSpeechCutApplied { through: 43, .. })
    ));
    let fresh_permit = owner.issue_output(now(), original_plan, 100_000).unwrap();
    let fresh_playback = owner.playback_started(now(), fresh_permit).unwrap();
    reply.as_mut().unwrap().playback = Some(fresh_playback);
    reply.as_mut().unwrap().final_chunk = Some(10);
    speaker_peer
        .control
        .send(WorkerEvent::SpeechRetired {
            owner: turn,
            sequence: 9,
            final_sample: lamp_audio::ownership::PlaybackCursor {
                epoch: 1,
                frame: 240,
            },
            observed_at_us: monotonic_us(),
        })
        .unwrap();
    service_speaker_control_inner(
        &mut io,
        &mut owner,
        &mut reply,
        &mut pending,
        &mut PlaybackMask::new(TurnTaking::VadOnly),
        &mut snapshot,
        &mut 0,
        &mut 8,
        &mut trace,
        &mut None,
        Some(&mut cuts),
    )
    .unwrap();
    assert_eq!(reply.as_ref().unwrap().playback, Some(fresh_playback));
    assert_eq!(reply.as_ref().unwrap().final_chunk, Some(10));
    cuts.admit(
        boot,
        ProviderCut {
            request: turn.turn(),
            through: 44,
            admitted_at_us: monotonic_us(),
        },
        &mut reply,
        10,
        |message| io.speaker.control.send(message),
        &mut trace,
    )
    .unwrap();
    speaker_peer.control.receive::<Control>().unwrap().unwrap();
    let (_, replacement, _) = active_reply();
    let replacement_owner = replacement.owner;
    let replacement_pcm = replacement.pcm.clone();
    reply = Some(replacement);
    cuts.retire(boot, &reply);
    assert!(
        !cuts.waiting(),
        "retired request cannot hold or time out a new owner"
    );
    speaker_peer
        .control
        .send(WorkerEvent::SpeechCutApplied {
            owner: turn,
            through: 10,
            applied_at_us: monotonic_us(),
        })
        .unwrap();
    service_speaker_control_inner(
        &mut io,
        &mut owner,
        &mut reply,
        &mut pending,
        &mut PlaybackMask::new(TurnTaking::VadOnly),
        &mut snapshot,
        &mut 0,
        &mut 8,
        &mut trace,
        &mut None,
        Some(&mut cuts),
    )
    .unwrap();
    assert_eq!(reply.as_ref().unwrap().owner, replacement_owner);
    assert_eq!(reply.as_ref().unwrap().pcm, replacement_pcm);
    assert!(
        provider_peer
            .control
            .receive::<Control>()
            .unwrap()
            .is_none()
    );
}

#[test]
fn stale_request_cut_cannot_clear_the_current_reply() {
    let (_, current, _) = active_reply();
    let request = current.owner.turn();
    let before = current.pcm.clone();
    let mut reply = Some(current);
    let mut cuts = super::super::speech_cut::State::default();
    cuts.admit(
        new_boot().unwrap(),
        ProviderCut {
            request: request + 1,
            through: 99,
            admitted_at_us: monotonic_us(),
        },
        &mut reply,
        4,
        |_| panic!("noncurrent request must not stop speaker"),
        &mut SessionState::default(),
    )
    .unwrap();
    assert_eq!(reply.unwrap().pcm, before);
    assert!(!cuts.waiting());
}

#[test]
fn first_zero_audio_cut_installs_actual_owner_before_the_cut_on_control_ipc() {
    let dir = SessionDirectory::create().unwrap();
    let (mut speaker, mut peer) = pair(&dir, "speaker");
    let (mut owner, mut current, _) = active_reply();
    owner
        .playback_ended(now(), current.playback.take().unwrap())
        .unwrap();
    current.pcm.clear();
    current.had_audio = false;
    let turn = current.owner;
    let authority = owner.snapshot(now()).unwrap();
    let mut reply = Some(current);
    let mut cuts = super::super::speech_cut::State::default();
    cuts.admit(
        new_boot().unwrap(),
        ProviderCut {
            request: turn.turn(),
            through: 0,
            admitted_at_us: monotonic_us(),
        },
        &mut reply,
        0,
        |cut| super::super::speech_cut::send_speaker_cut(&mut speaker.control, authority, cut),
        &mut SessionState::default(),
    )
    .unwrap();
    let Some(Control::Authority { snapshot }) = peer.control.receive::<Control>().unwrap() else {
        panic!("authority must precede cut")
    };
    assert_eq!(snapshot.owner(), Some(turn));
    assert_eq!(
        snapshot.microphone_permission(),
        authority.microphone_permission()
    );
    assert!(
        matches!(peer.control.receive::<Control>().unwrap(), Some(Control::CutSpeech { owner, through: 0 }) if owner == turn)
    );
    assert!(peer.data.receive::<SpeakerChunk>().unwrap().is_none());
    assert!(cuts.waiting());
}
