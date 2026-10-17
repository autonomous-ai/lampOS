//! A line lost before any output, started once more on the next connection
//! (lampOS#705, Step B): only when asked for, only that line, only once.
use super::*;

fn session(id: u64) -> SessionId {
    SessionId::new(id).unwrap()
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
