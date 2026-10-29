//! Lost-line replay at the worker's queue and connection boundaries.
use super::*;

/// Like InputSender, a full queue refuses the command and fails its link.
/// The test returns capacity explicitly, without a task draining it between
/// consecutive offers from the same worker tick.
struct BoundedSink {
    inner: RecordingSink,
    room: Cell<usize>,
}

impl BoundedSink {
    fn reserve(&self) -> lamp_gemini::Result<()> {
        let Some(left) = self.room.get().checked_sub(1) else {
            self.inner.disconnected.set(true);
            return Err(Error::Backpressure);
        };
        self.room.set(left);
        Ok(())
    }
}

impl InputSink for BoundedSink {
    fn start(
        &self,
        session: SessionId,
        request: RequestId,
    ) -> std::result::Result<(), StartRefusal> {
        self.reserve().map_err(StartRefusal::Refused)?;
        self.inner.start(session, request)
    }

    fn audio(
        &self,
        request: RequestId,
        sequence: u64,
        captured: Instant,
        samples: &[i16],
    ) -> lamp_gemini::Result<()> {
        self.reserve()?;
        self.inner.audio(request, sequence, captured, samples)
    }

    fn end(&self, request: RequestId) -> lamp_gemini::Result<()> {
        self.reserve()?;
        self.inner.end(request)
    }

    fn retire(&self, request: RequestId) {
        self.inner.retire(request);
    }

    fn has_room(&self) -> bool {
        self.room.get() > 0
    }
}

fn replaying() -> (Controller, InputHandoff, BoundedSink, u64) {
    let mut owner = controller();
    let mut handoff = InputHandoff::default();
    let request = RequestId::new(authorize(&mut owner, &mut handoff)).unwrap();
    let turn = handoff.authority.unwrap().owner().unwrap();
    owner.input_ended(time(), turn).unwrap();
    handoff
        .observe_authority(owner.snapshot(time()).unwrap())
        .unwrap();
    let mut kept = Kept::new(request, generation(&handoff));
    kept.keep(1, &[3; 160]);
    kept.keep(2, &[4; 160]);
    kept.ended_us = Some(NOW);
    kept.resent = true;
    kept.replay = Some(0);
    handoff.kept = Some(kept);
    handoff.submitted = Some(SubmittedInput {
        request,
        session: SessionId::new(2).unwrap(),
        open: true,
        captured: None,
    });
    let sink = BoundedSink {
        inner: RecordingSink::default(),
        room: Cell::new(2),
    };
    sink.inner.open.set(Some(request.get()));
    (owner, handoff, sink, request.get())
}

/// Audio fills the final two queue slots. End must wait for capacity on a
/// later tick, with neither a failed connection nor repeated audio or End.
#[test]
fn a_replayed_lines_end_waits_when_its_last_audio_fills_the_queue() {
    let (_owner, mut handoff, sink, request) = replaying();
    let audio = vec![
        Call::Audio(request, 1, vec![3; 160]),
        Call::Audio(request, 2, vec![4; 160]),
    ];

    handoff.resend_upkeep(&sink, NOW);
    assert_eq!(*sink.inner.calls.borrow(), audio);
    assert!(!sink.inner.disconnected.get());
    assert_eq!(handoff.kept.as_ref().unwrap().replay, Some(2));
    assert!(handoff.submitted.unwrap().open);

    // Still full on the next tick: no offer and no loss of the pending End.
    handoff.resend_upkeep(&sink, NOW + 2_000);
    assert_eq!(*sink.inner.calls.borrow(), audio);
    assert!(!sink.inner.disconnected.get());
    assert_eq!(handoff.kept.as_ref().unwrap().replay, Some(2));

    sink.room.set(1);
    // Capacity returns between ticks: End goes on the very next 2 ms tick.
    handoff.resend_upkeep(&sink, NOW + 4_000);
    let mut finished = audio;
    finished.push(Call::End(request));
    assert_eq!(*sink.inner.calls.borrow(), finished);
    assert!(!sink.inner.disconnected.get());
    assert_eq!(handoff.kept.as_ref().unwrap().replay, None);
    assert!(!handoff.kept.as_ref().unwrap().refused);
    assert!(!handoff.submitted.unwrap().open);

    handoff.resend_upkeep(&sink, NOW + 6_000);
    assert_eq!(*sink.inner.calls.borrow(), finished);
}

/// Stop/privacy controls are taken before replay upkeep. Capacity returning
/// afterwards must not flush the abandoned request's pending End.
#[test]
fn a_replayed_lines_pending_end_is_dropped_after_cancel_or_privacy_change() {
    for privacy in [false, true] {
        let (mut owner, mut handoff, sink, request) = replaying();
        handoff.resend_upkeep(&sink, NOW);
        assert_eq!(handoff.kept.as_ref().unwrap().replay, Some(2));
        let now = MonoTime::from_micros(NOW + 2_000);
        if privacy {
            owner
                .set_microphone_permission(now, Permission::Denied)
                .unwrap();
        } else {
            owner
                .cancel_turn(now, handoff.authority.unwrap().owner().unwrap())
                .unwrap();
        }
        handoff
            .observe_authority(owner.snapshot(now).unwrap())
            .unwrap();
        sink.room.set(1);
        handoff.resend_upkeep(&sink, NOW + 2_000);
        assert!(handoff.kept.is_none());
        assert_eq!(
            *sink.inner.calls.borrow(),
            vec![
                Call::Audio(request, 1, vec![3; 160]),
                Call::Audio(request, 2, vec![4; 160])
            ],
            "privacy change: {privacy}"
        );
        assert!(!sink.inner.disconnected.get());
    }
}

/// A tool may already have acted even when no spoken answer arrived. Keep
/// the replacement live and verify that neither the request nor its timer
/// is asked for again after the failure is reported.
#[tokio::test]
async fn a_lost_line_that_called_a_tool_is_not_replayed_on_the_ready_replacement() {
    let desk = super::policy_holds::Desk::new("resend-tool");
    let mut rig = LiveRig::start_supervised(
        quick_policy(),
        desk.options(false),
        BARRIER,
        false,
        Continuation::Off,
        lamp_gemini::MAX_WIRE_BYTES * 2,
        |config| config,
        true,
    );
    let mut dial = rig.ready().await;
    dial.service.send(resumable()).await;
    let lost = rig.speak(&mut dial, false, true).await;
    dial.service
        .send(json!({"toolCall":{"functionCalls":[
            {"id":"timer-1","name":"set_timer","args":{"minutes":1}}
        ]}}))
        .await;
    let reply = rig.service_message(&mut dial).await;
    assert_eq!(
        reply["toolResponse"]["functionResponses"][0]["id"],
        "timer-1"
    );
    assert_eq!(desk.asked.try_recv().unwrap()["name"], "set_timer");
    assert!(desk.asked.try_recv().is_err());
    dial.service.close(1011, "internal").await;

    let (seen, _) = after_a_lost_line(
        &mut rig,
        lost,
        FailedStage::Responding,
        Fault::PeerClosed { code: Some(1011) },
    )
    .await;
    assert!(seen.contains(&"recovered"), "{seen:?}");
    assert_eq!(
        seen.iter().filter(|kind| **kind == "turn_failed").count(),
        1
    );
    assert!(!seen.contains(&"activityStart"), "{seen:?}");
    assert!(!seen.contains(&"answered"), "{seen:?}");
    assert!(
        desk.asked.try_recv().is_err(),
        "the timer was asked for again"
    );
    rig.stop().await;
}

/// cb94's distinct failure: a successor's audio reaches the old connection
/// while its End waits behind a cut-off answer. That barrier expires 3.5 s
/// after End, and reconnect takes 1.2 s. The whole successor is then sent
/// exactly once, before its five-second replay deadline, as the same request.
#[tokio::test(start_paused = true)]
async fn a_line_lost_to_a_barrier_timeout_is_replayed_once_after_reconnect() {
    let _clock = logical_clock();
    let mut rig = LiveRig::start_supervised(
        quick_policy(),
        Options::reporting(Reporting::Typed),
        BarrierPolicy::Require,
        false,
        Continuation::Off,
        lamp_gemini::MAX_WIRE_BYTES * 2,
        |config| {
            config
                .barrier_wait(BarrierWait::AtCommit)
                .timeouts(lamp_gemini::Timeouts {
                    barrier: Duration::from_millis(3_500),
                    ..Default::default()
                })
                .unwrap()
        },
        true,
    );
    let mut dial = rig.ready().await;
    dial.service.send(resumable()).await;
    let cut = rig.speak(&mut dial, false, true).await;
    dial.service.send(provider_audio(5, 480)).await;
    assert!(matches!(rig.output().await, ProviderOutput::Audio { request, .. } if request == cut));

    let lost = rig.speak(&mut dial, true, false).await;
    assert!(lost > cut);
    let owner = rig.snapshot.owner().unwrap();
    rig.owner.input_ended(real_time(), owner).unwrap();
    rig.publish();
    let ended_at = tokio::time::Instant::now();
    rig.parent
        .data
        .send(ProviderInput::End {
            request: lost,
            privacy_generation: rig.snapshot.microphone_generation(),
            sound: false,
            cut_in: false,
        })
        .unwrap();

    // No scripted close: the real transport enforces the Require barrier.
    let (seen, _) = after_a_lost_line_with_setup_delay(
        &mut rig,
        lost,
        FailedStage::AwaitingResponse,
        Fault::BarrierTimeout,
        Duration::from_millis(1_200),
    )
    .await;
    assert!(ended_at.elapsed() >= Duration::from_millis(4_700));
    assert!(ended_at.elapsed() < Duration::from_secs(5));
    assert!(!seen.contains(&"turn_failed"), "{seen:?}");
    assert!(
        !seen.contains(&"started"),
        "the original Start was already reported: {seen:?}"
    );
    for kind in ["activityStart", "audio", "activityEnd", "answered"] {
        assert_eq!(
            seen.iter().filter(|seen| **seen == kind).count(),
            1,
            "{seen:?}"
        );
    }
    // The timed-out connection never committed this successor. Its End
    // was sent only with the replay, on the replacement connection.
    assert!(dial.service.try_receive(Duration::ZERO).await.is_none());
    rig.stop().await;
}

/// Tests on #1434 (07:15): with the switch off, a line keeps nothing, and a
/// command refused while the link is down goes through `refused()` and
/// fails the line at once, as before Step B; no outcome is held.
#[test]
fn with_the_switch_off_nothing_is_kept_and_a_refusal_fails_at_once() {
    let mut owner = controller();
    let mut handoff = InputHandoff {
        tolerate_outage: true,
        ..InputHandoff::default()
    };
    assert!(!handoff.resend);
    let sink = RecordingSink::default();
    let request = authorize(&mut owner, &mut handoff);
    handoff
        .submit(start(request, &handoff), NOW, &sink)
        .unwrap();
    handoff
        .submit(audio(request, 1, 7, &handoff), NOW, &sink)
        .unwrap();
    assert!(handoff.kept.is_none());
    sink.disconnected.set(true);
    handoff
        .submit(audio(request, 2, 7, &handoff), NOW, &sink)
        .unwrap();
    assert!(handoff.kept.is_none());
    assert_eq!(handoff.failed_through, request);
    assert!(handoff.submitted.is_none());
    handoff.resend_upkeep(&sink, NOW + 10_000_000);
    assert!(handoff.unresent.is_empty());
}

/// A held line, lost after its End to a mendable fault, on a link that
/// never comes back: its kept end.
fn held() -> (Controller, InputHandoff, RecordingSink, u64) {
    let (owner, mut handoff, _, request) = replaying();
    let kept = handoff.kept.as_mut().unwrap();
    kept.resent = false;
    kept.replay = None;
    kept.refused = true;
    kept.lost = Some(KeptLoss {
        session: SessionId::new(2).unwrap(),
        stage: FailedStage::AwaitingResponse,
        fault: Fault::Transport,
        at_us: NOW,
    });
    (owner, handoff, RecordingSink::default(), request)
}

/// The reviewer on #1434 (07:14): `give_up` is reached. No session within
/// 5 s of the line's end: its held `TurnFailed` is owed, `TooLate`, and
/// nothing was offered. Retired meanwhile (a newer line's owner, or a
/// cancel): dropped, and nothing is owed.
#[test]
fn a_held_line_fails_after_five_seconds_and_is_dropped_owing_nothing_when_retired() {
    let (_owner, mut handoff, sink, request) = held();
    handoff.resend_upkeep(&sink, NOW + crate::resend::RESEND_WITHIN_US);
    assert!(handoff.kept.is_some(), "held up to the bound");
    assert!(handoff.unresent.is_empty());
    handoff.resend_upkeep(&sink, NOW + crate::resend::RESEND_WITHIN_US + 1);
    assert!(handoff.kept.is_none());
    let owed = handoff
        .unresent
        .pop_front()
        .expect("its TurnFailed is owed");
    assert_eq!(
        (owed.request, owed.stage, owed.why),
        (
            request,
            FailedStage::AwaitingResponse,
            crate::resend::Refused::TooLate
        )
    );
    assert!(sink.calls.borrow().is_empty());

    let (_owner, mut handoff, sink, request) = held();
    handoff.retired_through = request;
    handoff.resend_upkeep(&sink, NOW + 1_000);
    assert!(handoff.kept.is_none());
    assert!(handoff.unresent.is_empty());
    assert!(sink.calls.borrow().is_empty());
}

/// A next session that is live, has room and takes a resend.
struct LiveSink {
    inner: RecordingSink,
    resent: Cell<bool>,
}

impl InputSink for LiveSink {
    fn start(
        &self,
        session: SessionId,
        request: RequestId,
    ) -> std::result::Result<(), StartRefusal> {
        self.inner.start(session, request)
    }

    fn audio(
        &self,
        request: RequestId,
        sequence: u64,
        captured: Instant,
        samples: &[i16],
    ) -> lamp_gemini::Result<()> {
        self.inner.audio(request, sequence, captured, samples)
    }

    fn end(&self, request: RequestId) -> lamp_gemini::Result<()> {
        self.inner.end(request)
    }

    fn retire(&self, request: RequestId) {
        self.inner.retire(request);
    }

    fn has_room(&self) -> bool {
        true
    }

    fn note(&self, request: RequestId, text: &str) -> lamp_gemini::Result<()> {
        self.inner.note(request, text)
    }

    fn live_session(&self) -> Option<SessionId> {
        SessionId::new(3).ok()
    }

    fn resend(
        &self,
        _session: SessionId,
        _request: RequestId,
    ) -> std::result::Result<(), StartRefusal> {
        self.resent.set(true);
        Ok(())
    }
}

/// The reviewer on #1434 (07:14, 09:30): what went with a line. The rest
/// of a line cut off by a pause (`continues`, `continues_cut_off`), or one
/// the eyes' picture went with, is half of what the lost session heard: with
/// a next session live, it is not sent again, its held `TurnFailed` is owed
/// (`Attached`), and nothing is offered. A note told beside it (here the
/// lamp's name heard, which the product tells) goes again with it, on the
/// new session, before its blocks. The same line with nothing beside it is
/// sent again.
#[test]
fn a_held_line_goes_again_with_its_note_but_not_without_its_first_part_or_picture() {
    let live = || LiveSink {
        inner: RecordingSink::default(),
        resent: Cell::new(false),
    };
    // A Start that said `continues_cut_off`, through the worker's own submit.
    let mut owner = controller();
    let mut handoff = InputHandoff {
        tolerate_outage: true,
        resend: true,
        ..InputHandoff::default()
    };
    let sink = RecordingSink::default();
    let request = authorize(&mut owner, &mut handoff);
    let cut_off = match start(request, &handoff) {
        ProviderInput::Start {
            session,
            request,
            privacy_generation,
            ..
        } => ProviderInput::Start {
            session,
            request,
            privacy_generation,
            continues: false,
            continues_cut_off: true,
            over_the_lamp: false,
            after_stop: false,
            follows_on: false,
            over_the_lamps_voice: false,
            heard_of_cut: None,
            soft_lead_us: 0,
        },
        other => other,
    };
    handoff.submit(cut_off, NOW, &sink).unwrap();
    assert!(handoff.kept.as_ref().unwrap().attached);
    let (_owner, mut handoff, _, request) = held();
    handoff.kept.as_mut().unwrap().attached = true;
    let sink = live();
    handoff.resend_upkeep(&sink, NOW + 1_000);
    assert!(!sink.resent.get());
    let owed = handoff
        .unresent
        .pop_front()
        .expect("its TurnFailed is owed");
    assert_eq!(
        (owed.request, owed.why),
        (request, crate::resend::Refused::Attached)
    );
    assert!(sink.inner.calls.borrow().is_empty());
    // The eyes' picture went with it.
    let (_owner, mut handoff, _, request) = held();
    handoff.pictured(RequestId::new(request).unwrap());
    let sink = live();
    handoff.resend_upkeep(&sink, NOW + 1_000);
    assert!(!sink.resent.get());
    // Its name heard, told beside it: sent again, and told again first.
    let (_owner, mut handoff, _, request) = held();
    let note = note_text(None, request, crate::called::NOTE);
    handoff.noted = Some((request, note.clone()));
    let sink = live();
    handoff.resend_upkeep(&sink, NOW + 1_000);
    assert!(sink.resent.get());
    assert!(handoff.unresent.is_empty());
    assert_eq!(
        sink.inner.calls.borrow().first(),
        Some(&Call::Note(request, note))
    );
    // Nothing beside it: sent again, with no note.
    let (_owner, mut handoff, _, _) = held();
    let sink = live();
    handoff.resend_upkeep(&sink, NOW + 1_000);
    assert!(sink.resent.get());
    assert!(
        !sink
            .inner
            .calls
            .borrow()
            .iter()
            .any(|call| matches!(call, Call::Note(..)))
    );
}
