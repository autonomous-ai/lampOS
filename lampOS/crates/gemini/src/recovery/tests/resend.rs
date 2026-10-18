//! A line lost before any output, started once more on the next connection
//! (lampOS#705, Step B): only when asked for, only that line, only once.
use super::*;

fn session(id: u64) -> SessionId {
    SessionId::new(id).unwrap()
}

async fn resend(rig: &mut Rig, dial: &mut Dial, id: u64, end: bool) {
    rig.0
        .try_resend_for_session(dial.session, request(id))
        .unwrap();
    assert!(
        dial.service.receive().await["realtimeInput"]
            .get("activityStart")
            .is_some()
    );
    assert!(matches!(
        notice(&mut rig.0).await,
        Notice::Event(Event::InputStarted { lineage, .. })
            if lineage.request == request(id) && lineage.session == dial.session
    ));
    rig.0
        .try_audio(request(id), 0, std::time::Instant::now(), &[1; 160])
        .unwrap();
    assert!(
        dial.service.receive().await["realtimeInput"]
            .get("audio")
            .is_some()
    );
    if end {
        rig.0.try_end(request(id)).unwrap();
        assert!(
            dial.service.receive().await["realtimeInput"]
                .get("activityEnd")
                .is_some()
        );
    }
}

/// The connection is lost with a line in flight, at `stage`, and the next
/// one is ready.
async fn lost_at(stage: Stage, resend: bool) -> (Rig, Dial) {
    let (mut rig, mut dial) = connected(quick(), config()).await;
    rig.0.resend_lost_lines(resend);
    speak(&mut rig, &mut dial, 1, stage != Stage::InputOpen).await;
    if stage == Stage::Responding {
        dial.service.send(audio(7, 240)).await;
        assert!(matches!(
            notice(&mut rig.0).await,
            Notice::Event(Event::Audio { .. })
        ));
    }
    dial.service.close(1011, "internal").await;
    assert!(matches!(
        notice(&mut rig.0).await,
        Notice::TurnLost { lineage, stage: lost, .. }
            if lineage.request.get() == 1 && lineage.session.get() == 1 && lost == stage
    ));
    let (second, redial, _) = ready(&mut rig).await;
    assert!(matches!(second, Notice::Ready { .. }));
    (rig, redial)
}

/// Lost before any output, the line starts again on the next connection, its
/// input goes again, and the answer that comes is that same request's, on
/// the new connection. An ordinary start of it is still refused, and so is a
/// second resend.
#[tokio::test]
async fn a_line_lost_before_any_output_is_started_once_more_and_answered_as_itself() {
    for stage in [Stage::InputOpen, Stage::AwaitingResponse] {
        let (mut rig, mut dial) = lost_at(stage, true).await;
        assert_eq!(rig.0.lost().map(|lost| lost.request), Some(request(1)));
        assert_eq!(rig.0.try_start(request(1)), Err(Error::StaleRequest));
        assert_eq!(
            rig.0.try_resend_for_session(session(1), request(1)),
            Err(StartRefusal::SessionMismatch { live: session(2) })
        );
        rig.0
            .try_resend_for_session(session(2), request(1))
            .unwrap();
        assert!(
            dial.service.receive().await["realtimeInput"]
                .get("activityStart")
                .is_some()
        );
        assert!(matches!(
            notice(&mut rig.0).await,
            Notice::Event(Event::InputStarted { lineage, .. })
                if lineage.request.get() == 1 && lineage.session.get() == 2
        ));
        rig.0
            .try_audio(request(1), 0, std::time::Instant::now(), &[1; 160])
            .unwrap();
        assert!(
            dial.service.receive().await["realtimeInput"]
                .get("audio")
                .is_some()
        );
        rig.0.try_end(request(1)).unwrap();
        assert!(
            dial.service.receive().await["realtimeInput"]
                .get("activityEnd")
                .is_some()
        );
        dial.service.send(audio(9, 240)).await;
        assert!(
            matches!(
                notice(&mut rig.0).await,
                Notice::Event(Event::Audio { lineage, .. })
                    if lineage.request.get() == 1 && lineage.session.get() == 2
            ),
            "{stage:?}"
        );
        assert_eq!(rig.0.lost(), None);
        assert_eq!(
            rig.0.try_resend_for_session(session(2), request(1)),
            Err(StartRefusal::Refused(Error::StaleRequest))
        );
    }
}

/// Nothing is sent again without the switch, once an answer had begun, once
/// the caller has retired the line, or once a newer line has started.
#[tokio::test]
async fn a_lost_line_is_not_sent_again_unless_asked_and_untouched_since() {
    let refused = Err(StartRefusal::Refused(Error::StaleRequest));
    // Not asked for: as before, never replayed.
    let (rig, _dial) = lost_at(Stage::AwaitingResponse, false).await;
    assert_eq!(rig.0.lost(), None);
    assert_eq!(
        rig.0.try_resend_for_session(session(2), request(1)),
        refused
    );
    // An answer had begun: some of it may have been heard.
    let (rig, _dial) = lost_at(Stage::Responding, true).await;
    assert_eq!(rig.0.lost(), None);
    assert_eq!(
        rig.0.try_resend_for_session(session(2), request(1)),
        refused
    );
    // Retired by the caller (the coordinator gave the turn up).
    let (rig, _dial) = lost_at(Stage::AwaitingResponse, true).await;
    rig.0.retire(request(1));
    assert_eq!(rig.0.lost(), None);
    assert_eq!(
        rig.0.try_resend_for_session(session(2), request(1)),
        refused
    );
    // A newer line has started on the new connection.
    let (rig, _dial) = lost_at(Stage::AwaitingResponse, true).await;
    rig.0.try_start(request(2)).unwrap();
    assert_eq!(rig.0.lost(), None);
    assert_eq!(
        rig.0.try_resend_for_session(session(2), request(1)),
        refused
    );
}

#[tokio::test(start_paused = true)]
async fn a_resent_line_lost_again_cannot_be_resent_on_a_third_connection() {
    for initial_stage in [Stage::InputOpen, Stage::AwaitingResponse] {
        for retry_stage in [Stage::InputOpen, Stage::AwaitingResponse] {
            let (mut rig, mut second) = lost_at(initial_stage, true).await;
            resend(&mut rig, &mut second, 1, retry_stage != Stage::InputOpen).await;
            // Changing the opt-in must not restore a consumed retry.
            rig.0.resend_lost_lines(false);
            rig.0.resend_lost_lines(true);
            second.service.close(1011, "retry lost").await;
            assert!(matches!(
                notice(&mut rig.0).await,
                Notice::TurnLost { lineage, stage, .. }
                    if lineage.request == request(1)
                        && lineage.session == session(2)
                        && stage == retry_stage
            ));
            let (third_ready, mut third, _) = ready(&mut rig).await;
            assert!(matches!(
                third_ready,
                Notice::Ready { session: live, .. } if live == session(3)
            ));
            assert_eq!(rig.0.lost(), None);
            assert_eq!(
                rig.0.try_resend_for_session(session(3), request(1)),
                Err(StartRefusal::Refused(Error::StaleRequest))
            );
            assert_eq!(rig.0.try_start(request(1)), Err(Error::StaleRequest));
            assert!(
                third
                    .service
                    .try_receive(Duration::from_millis(1))
                    .await
                    .is_none()
            );
            // The retry limit belongs to the lost request, not the whole
            // supervisor: a newer request still gets its own single retry.
            speak(&mut rig, &mut third, 2, true).await;
            third.service.close(1011, "new request lost").await;
            assert!(matches!(
                notice(&mut rig.0).await,
                Notice::TurnLost { lineage, stage: Stage::AwaitingResponse, .. }
                    if lineage.request == request(2) && lineage.session == session(3)
            ));
            let (fourth_ready, mut fourth, _) = ready(&mut rig).await;
            assert!(matches!(
                fourth_ready,
                Notice::Ready { session: live, .. } if live == session(4)
            ));
            assert_eq!(rig.0.lost().map(|lost| lost.request), Some(request(2)));
            resend(&mut rig, &mut fourth, 2, true).await;
            fourth.service.send(audio(11, 240)).await;
            assert!(matches!(
                notice(&mut rig.0).await,
                Notice::Event(Event::Audio { lineage, pcm, .. })
                    if lineage.request == request(2)
                        && lineage.session == session(4)
                        && pcm == [11; 240]
            ));
            rig.0.shutdown();
        }
    }
}

#[tokio::test(start_paused = true)]
async fn output_or_a_tool_call_drained_after_loss_prevents_resending_the_line() {
    for (kind, output) in [
        ("audio", audio(7, 240)),
        (
            "text",
            json!({"serverContent":{"modelTurn":{"parts":[{"text":"Working."}]}}}),
        ),
        (
            "transcript",
            json!({"serverContent":{"outputTranscription":{"text":"Working."}}}),
        ),
        (
            "tool",
            json!({"toolCall":{"functionCalls":[{"id":"call-1","name":"light_on","args":{}}]}}),
        ),
    ] {
        let (mut rig, mut first) = connected(quick(), config()).await;
        rig.0.resend_lost_lines(true);
        speak(&mut rig, &mut first, 1, true).await;
        first.service.send(output).await;
        first.service.close(1011, "output then loss").await;
        let output = notice(&mut rig.0).await;
        let lineage = match (kind, output) {
            ("audio", Notice::Event(Event::Audio { lineage, .. }))
            | ("text", Notice::Event(Event::ModelText { lineage, .. }))
            | ("transcript", Notice::Event(Event::OutputTranscript { lineage, .. }))
            | ("tool", Notice::Event(Event::ToolCall { lineage, .. })) => lineage,
            (_, other) => panic!("expected {kind} output, got {other:?}"),
        };
        assert_eq!(lineage.request, request(1));
        assert_eq!(lineage.session, session(1));
        assert!(matches!(
            notice(&mut rig.0).await,
            Notice::TurnLost { lineage: lost, stage: Stage::Responding, .. } if lost == lineage
        ));
        let (second_ready, mut second, _) = ready(&mut rig).await;
        assert!(matches!(
            second_ready,
            Notice::Ready { session: live, .. } if live == session(2)
        ));
        assert_eq!(rig.0.lost(), None, "{kind}");
        assert_eq!(
            rig.0.try_resend_for_session(session(2), request(1)),
            Err(StartRefusal::Refused(Error::StaleRequest)),
            "{kind}"
        );
        assert!(
            second
                .service
                .try_receive(Duration::from_millis(1))
                .await
                .is_none(),
            "{kind}"
        );
        rig.0.shutdown();
    }
}
