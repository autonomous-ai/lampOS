//! A link that is kept for as long as its supervisor lives
//! (`RecoveryPolicy::endless`). The bounded policy's own tests are unchanged
//! and still hold for every policy that does not ask for this.
use super::availability::{finished_attempt, update};
use super::*;

const PAUSE: Duration = Duration::from_secs(1);

fn endless() -> RecoveryPolicy {
    RecoveryPolicy {
        endless: true,
        refusal_pause: PAUSE,
        ..quick()
    }
}

/// On lamp-4ace on 2026-10-13 a test cut the link every 24 seconds. Each
/// loss was cured in under a second; the fourth inside a minute ended the
/// service as unstable. Here the limit is two, and six losses follow.
#[tokio::test(start_paused = true)]
async fn a_link_that_keeps_dropping_is_paced_and_never_ended() {
    let policy = RecoveryPolicy {
        max_recoveries: 2,
        ..endless()
    };
    let (mut rig, mut dial) = connected(policy, config()).await;
    let mut waits = Vec::new();
    for _ in 0..6 {
        dial.service.close(1011, "internal").await;
        let lost = Instant::now();
        let (again, mut redial, _) = ready(&mut rig).await;
        waits.push(lost.elapsed());
        assert!(matches!(
            again,
            Notice::Ready {
                context: Context::Resumed,
                attempts: 1,
                after: Some(Error::PeerClosed { code: Some(1011) }),
                ..
            }
        ));
        assert!(redial.resumed);
        redial.service.send(handle("HANDLE-NEXT")).await;
        dial = redial;
    }
    // The first two were replaced at once. From the third on the link is
    // one that keeps dropping, and `max_backoff` is waited before each.
    assert!(
        waits[..2].iter().all(|wait| *wait < policy.max_backoff),
        "{waits:?}"
    );
    assert!(
        waits[2..].iter().all(|wait| *wait >= policy.max_backoff),
        "{waits:?}"
    );
    assert!(rig.0.is_connected());
    rig.0.try_start(request(1)).unwrap();
}

/// Twenty attempts fail, against a policy of two attempts and a tenth of a
/// second. The twenty-first comes up, resumed, on its own session, and the
/// count it reports has stopped at the bound the rest of the service checks.
#[tokio::test(start_paused = true)]
async fn an_outage_past_every_attempt_and_budget_is_ridden_out() {
    let policy = RecoveryPolicy {
        max_attempts: 2,
        outage_budget: Duration::from_millis(100),
        ..endless()
    };
    let (mut rig, mut dial) = connected(policy, config()).await;
    dial.service.close(1008, "idle").await;
    let lost = Instant::now();
    let (supervisor, dials) = (&mut rig.0, &mut rig.1);
    let serve = async {
        for _ in 0..20 {
            // Dropping the service before setup fails that attempt.
            drop(next_dial(dials).await);
        }
        let mut dial = next_dial(dials).await;
        dial.service.accept().await;
        dial
    };
    let (back, redial) = tokio::join!(notice(supervisor), serve);
    let Notice::Ready {
        session,
        context,
        attempts,
        elapsed,
        after,
    } = back
    else {
        panic!("expected the link back, got {back:?}")
    };
    assert_eq!(session.get(), 22);
    assert_eq!(context, Context::Resumed);
    assert_eq!(attempts, MAX_COUNTED_ATTEMPTS);
    assert_eq!(after, Some(Error::PeerClosed { code: Some(1008) }));
    // 20 ms after the first failure, then 40 ms after each of nineteen.
    assert!(elapsed >= Duration::from_millis(780), "{elapsed:?}");
    assert!(lost.elapsed() >= Duration::from_millis(780));
    assert!(redial.resumed);
    supervisor.try_start(request(1)).unwrap();
}

/// An attempt that never completes does not hold an endless link beyond its
/// own timeouts: there is no budget to cut it, and none is needed.
#[tokio::test(start_paused = true)]
async fn an_attempt_that_hangs_is_ended_by_its_own_timeouts_and_followed_by_another() {
    let policy = RecoveryPolicy {
        outage_budget: Duration::from_millis(150),
        ..endless()
    };
    let (mut rig, mut dial) = connected(policy, config()).await;
    dial.service.close(1008, "idle").await;
    let lost = Instant::now();
    // Seeing the loss is what starts the replacement.
    let unavailable = update(&mut rig.0).await;
    assert!(matches!(
        unavailable.kind,
        LinkUpdateKind::Unavailable { .. }
    ));
    let _held = next_dial(&mut rig.1).await;
    // Long past the budget, nothing has ended.
    tokio::time::advance(Duration::from_secs(1)).await;
    assert_eq!(rig.0.poll_link(), None);
    // The held attempt ends at its own setup timeout; the next is served.
    let (supervisor, dials) = (&mut rig.0, &mut rig.1);
    let upkeep = async {
        loop {
            assert_eq!(supervisor.poll_link(), None);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    let mut next = tokio::select! {
        dial = dial_within(dials, Duration::from_secs(120)) => dial,
        () = upkeep => unreachable!(),
    };
    assert!(lost.elapsed() >= Timeouts::default().setup);
    next.service.accept().await;
    finished_attempt(&rig.0).await;
    let ready = update(&mut rig.0).await;
    assert!(matches!(
        ready.kind,
        LinkUpdateKind::Ready { attempts: 2, .. }
    ));
    assert!(rig.0.is_connected());
}

/// A lamp that starts before its network does. The bounded policy ends on
/// the first failed connection; this one says once that there is no
/// connection and asks until there is. The session that comes up has no
/// earlier conversation, and is reported so on a session of its own, as the
/// consumers of link updates require of anything but a first attempt.
#[tokio::test(start_paused = true)]
async fn the_first_connection_is_asked_for_until_it_comes_and_is_reported_fresh() {
    let (dialer, mut dials) = connector(config());
    let mut supervisor = Supervisor::new(dialer, endless()).unwrap();
    let serve = async {
        for _ in 0..3 {
            drop(next_dial(&mut dials).await);
        }
        let mut dial = next_dial(&mut dials).await;
        assert!(!dial.resumed);
        dial.service.accept().await;
        dial
    };
    let watch = async {
        let mut seen = Vec::new();
        loop {
            match supervisor.poll_link() {
                Some(update) if matches!(update.kind, LinkUpdateKind::Ready { .. }) => {
                    seen.push(update);
                    return seen;
                }
                Some(update) => seen.push(update),
                None => tokio::time::sleep(Duration::from_millis(2)).await,
            }
        }
    };
    let (seen, _dial) = timeout(WAIT, async { tokio::join!(watch, serve) })
        .await
        .expect("the link in time");
    let [unavailable, ready] = seen[..] else {
        panic!("one outage announcement and one ready, got {seen:?}")
    };
    assert_eq!((unavailable.revision, unavailable.session.get()), (1, 1));
    let LinkUpdateKind::Unavailable { error } = unavailable.kind else {
        panic!("unavailable first")
    };
    assert_eq!(error.recovery(), Recovery::Reconnect);
    assert_eq!((ready.revision, ready.session.get()), (2, 4));
    let LinkUpdateKind::Ready {
        context,
        attempts,
        started_at,
        completed_at,
        outage_started_at,
        after,
    } = ready.kind
    else {
        panic!("ready second")
    };
    assert_eq!(context, Context::Fresh);
    assert_eq!(attempts, 4);
    assert_eq!(after, Some(error));
    assert_eq!(outage_started_at, Some(unavailable.observed_at));
    assert!(unavailable.observed_at <= started_at && started_at <= completed_at);
    assert!(supervisor.is_connected());
    supervisor.try_start(request(1)).unwrap();
}

/// Quota, rate and usage refusals pass with time. The bounded policy ends on
/// one; this one waits the pause it was given and asks once more, whether
/// the refusal closed a live connection or answered a reconnect.
#[tokio::test(start_paused = true)]
async fn a_refusal_that_passes_with_time_is_asked_again_after_the_pause_and_not_before() {
    let (mut rig, mut dial) = connected(endless(), config()).await;
    dial.service
        .close(1011, "You exceeded your current quota")
        .await;
    let refused = Instant::now();
    let (back, mut redial, _) = ready(&mut rig).await;
    assert!(refused.elapsed() >= PAUSE, "{:?}", refused.elapsed());
    assert!(matches!(
        back,
        Notice::Ready {
            attempts: 1,
            after: Some(Error::QuotaExceeded),
            ..
        }
    ));

    redial.service.send(handle("HANDLE-TWO")).await;
    redial.service.close(1008, "idle").await;
    let (supervisor, dials) = (&mut rig.0, &mut rig.1);
    let serve = async {
        let mut refused = next_dial(dials).await;
        refused.service.receive().await;
        refused.service.close(4029, "usage limit reached").await;
        let at = Instant::now();
        let mut again = next_dial(dials).await;
        let waited = at.elapsed();
        again.service.accept().await;
        (again, waited, refused)
    };
    let (back, (_again, waited, _refused)) = tokio::join!(notice(supervisor), serve);
    assert!(waited >= PAUSE, "{waited:?}");
    assert!(matches!(
        back,
        Notice::Ready {
            attempts: 2,
            after: Some(Error::PeerClosed { code: Some(1008) }),
            ..
        }
    ));
    assert!(supervisor.is_connected());
}

/// #1567, lamp-75c9 on 2026-10-19: the account was out of quota from the
/// start. The first connection refused for quota is said once as
/// unavailable and asked again only after the pause, not hammered, and the
/// session that comes up is reported fresh.
#[tokio::test(start_paused = true)]
async fn a_first_connection_refused_for_quota_is_asked_again_after_the_pause() {
    let (dialer, mut dials) = connector(config());
    let mut supervisor = Supervisor::new(dialer, endless()).unwrap();
    let serve = async {
        let mut refused = next_dial(&mut dials).await;
        refused.service.receive().await;
        refused
            .service
            .close(1011, "You exceeded your current quota")
            .await;
        let at = Instant::now();
        let mut again = next_dial(&mut dials).await;
        let waited = at.elapsed();
        again.service.accept().await;
        (again, waited, refused)
    };
    let watch = async {
        let mut seen = Vec::new();
        loop {
            match supervisor.poll_link() {
                Some(update) if matches!(update.kind, LinkUpdateKind::Ready { .. }) => {
                    seen.push(update);
                    return seen;
                }
                Some(update) => seen.push(update),
                None => tokio::time::sleep(Duration::from_millis(2)).await,
            }
        }
    };
    let (seen, (_again, waited, _refused)) = timeout(WAIT, async { tokio::join!(watch, serve) })
        .await
        .expect("the link in time");
    assert!(waited >= PAUSE, "asked again after {waited:?}");
    let [unavailable, ready] = seen[..] else {
        panic!("one refusal and one ready, got {seen:?}")
    };
    assert!(matches!(
        unavailable.kind,
        LinkUpdateKind::Unavailable {
            error: Error::QuotaExceeded
        }
    ));
    assert!(matches!(
        ready.kind,
        LinkUpdateKind::Ready {
            context: Context::Fresh,
            attempts: 2,
            after: Some(Error::QuotaExceeded),
            ..
        }
    ));
    assert!(
        dials.try_recv().is_err(),
        "one connection asked again, no more"
    );
}

/// Endless is not blind. A fault that a new connection would only repeat
/// ends the supervisor as before, and so do a first connection refused
/// outright, a conversation lost under a policy that requires it kept, and
/// a local stop.
#[tokio::test]
async fn what_a_new_connection_cannot_cure_still_ends_it() {
    let (mut rig, mut dial) = connected(endless(), config()).await;
    dial.service
        .send(json!({"serverContent":{"modelTurn":{"parts":[{"fileData":{}}]}}}))
        .await;
    assert_eq!(
        failure(&mut rig.0).await,
        Failure {
            error: Error::UnsupportedMessage,
            reason: FailureReason::NotRecoverable,
            attempts: 0,
        }
    );
    assert!(
        timeout(Duration::from_millis(120), rig.1.recv())
            .await
            .is_err()
    );

    let (dialer, mut dials) = connector(config());
    let mut supervisor = Supervisor::new(dialer, endless()).unwrap();
    let refuse = async {
        let mut dial = next_dial(&mut dials).await;
        dial.service.receive().await;
        dial.service
            .send(json!({"error":{"code":400,"status":"INVALID_ARGUMENT"}}))
            .await;
        dial
    };
    let (ended, _dial) = tokio::join!(failure(&mut supervisor), refuse);
    assert_eq!(
        ended,
        Failure {
            error: Error::ServerRejected,
            reason: FailureReason::InitialConnect,
            attempts: 0,
        }
    );
    assert!(
        timeout(Duration::from_millis(120), dials.recv())
            .await
            .is_err()
    );

    let (dialer, dials) = connector(config());
    let mut rig = (Supervisor::new(dialer, endless()).unwrap(), dials);
    let (_, mut dial, _) = ready(&mut rig).await;
    dial.service.close(1008, "idle").await;
    assert_eq!(
        failure(&mut rig.0).await.reason,
        FailureReason::NoResumableContext
    );

    let (mut rig, _dial) = connected(endless(), config()).await;
    rig.0.shutdown();
    assert_eq!(failure(&mut rig.0).await.reason, FailureReason::Shutdown);
}

/// The same lost conversation under the policy a service that runs all day
/// uses: no handle is required, the link comes back, and it says that the
/// conversation was not kept.
#[tokio::test]
async fn without_a_handle_an_endless_link_that_accepts_it_comes_back_fresh() {
    let policy = RecoveryPolicy {
        require_context: false,
        ..endless()
    };
    let (dialer, dials) = connector(config());
    let mut rig = (Supervisor::new(dialer, policy).unwrap(), dials);
    let (_, mut dial, _) = ready(&mut rig).await;
    dial.service.close(1008, "idle").await;
    let (second, redial, _) = ready(&mut rig).await;
    assert!(matches!(
        second,
        Notice::Ready {
            context: Context::Fresh,
            attempts: 1,
            ..
        }
    ));
    assert!(!redial.resumed);
}

#[test]
fn an_endless_policy_must_be_able_to_pace_itself() {
    assert!(endless().validate().is_ok());
    assert!(
        RecoveryPolicy {
            endless: true,
            ..RecoveryPolicy::default()
        }
        .validate()
        .is_ok()
    );
    for unpaced in [
        RecoveryPolicy {
            first_backoff: Duration::ZERO,
            ..endless()
        },
        RecoveryPolicy {
            refusal_pause: Duration::from_millis(39),
            ..endless()
        },
        RecoveryPolicy {
            refusal_pause: Duration::from_secs(3601),
            ..endless()
        },
        RecoveryPolicy {
            endless: true,
            ..RecoveryPolicy::DISABLED
        },
    ] {
        assert_eq!(unpaced.validate(), Err(Error::InvalidConfiguration));
    }
    // A policy that is not endless does not read the pause.
    assert!(
        RecoveryPolicy {
            refusal_pause: Duration::ZERO,
            ..quick()
        }
        .validate()
        .is_ok()
    );
}
