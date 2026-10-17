//! A start that names the connection it was chosen for.
use super::{
    availability::{failed_source, replacement_without_output, unread, update},
    *,
};

#[tokio::test(start_paused = true)]
async fn start_chosen_for_a_replaced_connection_is_refused_before_anything_is_sent() {
    // An answer from connection 1 is still unread when that connection dies
    // and connection 2 takes its place.
    let (mut rig, _, expected) = unread(true, quick()).await;
    let (ready, mut second) = replacement_without_output(&mut rig).await;
    let (chosen, live) = (SessionId::new(1).unwrap(), ready.session);
    assert_eq!(live.get(), 2);
    // The decision to start was taken while connection 1 was the ready one,
    // and reaches the supervisor only now.
    assert_eq!(
        rig.0.try_start_for_session(chosen, request(2)),
        Err(StartRefusal::SessionMismatch { live })
    );
    assert!(
        second
            .service
            .try_receive(Duration::from_millis(40))
            .await
            .is_none(),
        "the replacement must not hear a request it was never chosen for"
    );
    // Refusing it superseded nothing: the earlier answer is whole.
    let mut actual = Vec::new();
    loop {
        match timeout(WAIT, rig.0.next_output()).await.unwrap().unwrap() {
            OutputNotice::Event(Event::Audio { lineage, pcm, .. }) => {
                assert_eq!((lineage.session.get(), lineage.request.get()), (1, 1));
                actual.extend(pcm);
            }
            OutputNotice::Event(_) => {}
            OutputNotice::Settled { lineage, .. } => {
                assert_eq!(lineage.request.get(), 1);
                break;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(actual, expected);
    // The same request, chosen again for the connection that is live, starts.
    assert_eq!(rig.0.try_start_for_session(live, request(2)), Ok(()));
    assert!(
        second.service.receive().await["realtimeInput"]
            .get("activityStart")
            .is_some()
    );
    assert!(matches!(
        timeout(WAIT, rig.0.next_output()).await.unwrap().unwrap(),
        OutputNotice::Event(Event::InputStarted { lineage, .. })
            if lineage.session == live && lineage.request.get() == 2
    ));
    rig.0.shutdown();
}

#[tokio::test(start_paused = true)]
async fn checked_start_never_reaches_a_connection_other_than_the_one_named() {
    let (mut rig, mut first) = connected(quick(), config()).await;
    let one = SessionId::new(1).unwrap();
    // A session that does not exist yet is not the live one either.
    assert_eq!(
        rig.0
            .try_start_for_session(SessionId::new(2).unwrap(), request(1)),
        Err(StartRefusal::SessionMismatch { live: one })
    );
    assert_eq!(rig.0.try_start_for_session(one, request(1)), Ok(()));
    assert!(
        first.service.receive().await["realtimeInput"]
            .get("activityStart")
            .is_some()
    );
    // On the right connection the ordinary rules still decide.
    assert_eq!(
        rig.0.try_start_for_session(one, request(1)),
        Err(StartRefusal::Refused(Error::StaleRequest))
    );
    // During the outage there is no connection to name.
    first.service.close(1011, "outage").await;
    failed_source(&mut rig.0).await;
    assert_eq!(
        rig.0.try_start_for_session(one, request(2)),
        Err(StartRefusal::NotConnected)
    );
    assert!(matches!(
        update(&mut rig.0).await.kind,
        LinkUpdateKind::Unavailable { .. }
    ));
    assert_eq!(
        rig.0.try_start_for_session(one, request(2)),
        Err(StartRefusal::NotConnected)
    );
    // None of the refusals spent the request number or the lost request's.
    let mut second = next_dial(&mut rig.1).await;
    second.service.accept().await;
    let ready = loop {
        let update = update(&mut rig.0).await;
        if matches!(update.kind, LinkUpdateKind::Ready { .. }) {
            break update;
        }
    };
    assert_eq!(
        rig.0.try_start_for_session(one, request(2)),
        Err(StartRefusal::SessionMismatch {
            live: ready.session
        })
    );
    assert_eq!(
        rig.0.try_start_for_session(ready.session, request(2)),
        Ok(())
    );
    assert!(
        second.service.receive().await["realtimeInput"]
            .get("activityStart")
            .is_some()
    );
    rig.0.shutdown();
}

#[tokio::test(start_paused = true)]
async fn a_refused_start_leaves_the_request_on_the_live_connection_untouched() {
    let (mut rig, _, _) = unread(false, quick()).await;
    let (ready, mut second) = replacement_without_output(&mut rig).await;
    let (old, live) = (SessionId::new(1).unwrap(), ready.session);
    // Request 2 is already speaking on the replacement.
    rig.0.try_start_for_session(live, request(2)).unwrap();
    assert!(
        second.service.receive().await["realtimeInput"]
            .get("activityStart")
            .is_some()
    );
    rig.0
        .try_audio(request(2), 0, std::time::Instant::now(), &[5; 160])
        .unwrap();
    assert!(
        second.service.receive().await["realtimeInput"]
            .get("audio")
            .is_some()
    );
    // A start chosen for the dead connection arrives now, for a newer request.
    assert_eq!(
        rig.0.try_start_for_session(old, request(3)),
        Err(StartRefusal::SessionMismatch { live })
    );
    // Request 2 was neither retired nor superseded: it goes on and ends.
    rig.0
        .try_audio(request(2), 1, std::time::Instant::now(), &[6; 160])
        .unwrap();
    rig.0.try_end(request(2)).unwrap();
    for field in ["audio", "activityEnd"] {
        assert!(
            second.service.receive().await["realtimeInput"]
                .get(field)
                .is_some(),
            "{field}"
        );
    }
    assert!(
        second
            .service
            .try_receive(Duration::from_millis(40))
            .await
            .is_none(),
        "nothing of the refused request reached the service"
    );
    // The refused number was not spent either.
    assert_eq!(rig.0.try_start_for_session(live, request(3)), Ok(()));
    rig.0.shutdown();
}

/// A connector whose connections have 16 bytes of room toward the service,
/// so that once the service stops reading, nothing more can be written.
struct Narrow {
    config: SessionConfig,
    services: tokio::sync::mpsc::UnboundedSender<crate::testing::FakeService>,
}
impl Connect for Narrow {
    fn connect(&mut self, session: SessionId, resume: Option<ResumptionHandle>) -> Attempt {
        let config = self.config.clone().resume(resume);
        let services = self.services.clone();
        Box::pin(async move {
            let (attempt, service) = crate::testing::pair(config, session, 16).await;
            let _ = services.send(service);
            attempt.await
        })
    }
}

/// Start a request, and optionally end it, on a connection that cannot take
/// a single byte more. Returns the outputs up to and including its outcome.
async fn lost_before_the_start_was_written(end: bool) -> Vec<OutputNotice> {
    let (services, mut accepted) = tokio::sync::mpsc::unbounded_channel();
    let mut supervisor = Supervisor::new(
        Narrow {
            config: config(),
            services,
        },
        quick(),
    )
    .unwrap();
    let mut first = timeout(WAIT, accepted.recv()).await.unwrap().unwrap();
    first.accept().await;
    first.send(handle("HANDLE-ONE")).await;
    let ready = async {
        loop {
            if let Some(update) = supervisor.poll_link() {
                break update;
            }
            tokio::task::yield_now().await;
        }
    };
    assert!(matches!(
        timeout(WAIT, ready).await.unwrap().kind,
        LinkUpdateKind::Ready { .. }
    ));
    // Accepted locally, all at once; the service reads none of it.
    supervisor.try_start(request(1)).unwrap();
    if end {
        supervisor
            .try_audio(request(1), 0, std::time::Instant::now(), &[1; 160])
            .unwrap();
        supervisor.try_end(request(1)).unwrap();
    }
    let mut outputs = Vec::new();
    let mut unavailable = None;
    loop {
        if let Some(update) = supervisor.poll_link() {
            if let LinkUpdateKind::Unavailable { error } = update.kind {
                unavailable = Some(error);
            }
            continue;
        }
        match timeout(Duration::from_millis(50), supervisor.next_output()).await {
            Ok(Ok(output)) => {
                let last = !matches!(output, OutputNotice::Event(_));
                outputs.push(output);
                if last {
                    break;
                }
            }
            Ok(Err(failure)) => panic!("{failure}"),
            Err(_) => {}
        }
    }
    assert!(
        matches!(
            unavailable,
            Some(Error::WriteTimeout {
                write: crate::Write::Activity,
                ..
            })
        ),
        "{unavailable:?}"
    );
    supervisor.shutdown();
    outputs
}

#[tokio::test(start_paused = true)]
async fn a_request_lost_before_its_start_was_written_is_reported_without_input_started() {
    // The stage is recorded when the caller's call is accepted. `InputStarted`
    // is emitted only once `activityStart` has been written. A connection
    // that fails in between reports the request lost with no `InputStarted`
    // before it, and a consumer must accept exactly that.
    for (end, expected) in [(false, Stage::InputOpen), (true, Stage::AwaitingResponse)] {
        let outputs = lost_before_the_start_was_written(end).await;
        assert!(
            !outputs
                .iter()
                .any(|output| matches!(output, OutputNotice::Event(Event::InputStarted { .. }))),
            "{outputs:?}"
        );
        assert!(
            matches!(
                outputs.last(),
                Some(OutputNotice::TurnLost {
                    lineage,
                    stage,
                    error: Error::WriteTimeout {
                        write: crate::Write::Activity,
                        ..
                    },
                }) if lineage.request.get() == 1 && *stage == expected
            ),
            "{outputs:?}"
        );
        assert_eq!(outputs.len(), 1, "{outputs:?}");
    }
}
