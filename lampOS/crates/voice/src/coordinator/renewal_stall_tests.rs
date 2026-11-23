//! #8204: a worker that stops reading its control socket (its only thread
//! waiting on a slow call) costs a skipped renewal, never the voice.
use super::optional_ring_tests::{Peers, fixture};
use super::*;

/// Every message waiting on `peer`'s control socket read and dropped, as
/// a worker reading again after its stall reads them: a renewal stale with
/// the next behind it is passed over, and any fault fails the test.
fn read_backlog(peer: &mut WorkerChannels) {
    while peer
        .control
        .receive_renewed(Control::is_renewal)
        .expect("the backlog reads as a worker reads it")
        .is_some()
    {}
}

/// The rig with the second route alone (`LAMP_VOICE_ROUTE=second`): its
/// worker in the provider's slot and no standby, so it is mandatory, as on
/// lamp-06c1 when the forget's listing stalled it.
fn second_only(directory: &SessionDirectory) -> (Rig, Peers, WorkerChannels) {
    let (mut rig, peers) = fixture(directory);
    let (second, second_peer) = Worker::command_fixture(&directory.path, "second", "exec sleep 60");
    rig.provider = second;
    assert!(rig.second.is_none() && mandatory(&rig.provider, false));
    (rig, peers, second_peer)
}

/// Renewals published until the second worker's full socket has refused
/// some, as the 20 ms ticks do while it does not read: each is `Ok`.
fn fill(rig: &mut Rig, controller: &mut Controller) {
    for _ in 0..4_096 {
        let snapshot = controller.snapshot(now()).unwrap();
        rig.publish(snapshot)
            .expect("a full control socket is a skipped renewal, never an exit");
        if rig.provider.renewal_stall.skipped() >= 3 {
            return;
        }
    }
    panic!("the second worker's control socket never filled");
}

#[test]
fn a_full_control_socket_skips_the_renewal_and_the_next_tick_delivers_it() {
    let directory = SessionDirectory::create().unwrap();
    let (mut rig, mut peers, mut second) = second_only(&directory);
    let (mut controller, _, _) = super::tests::active_reply();
    fill(&mut rig, &mut controller);
    // The worker reads again: its backlog, then the next tick's renewal.
    for peer in [
        &mut second,
        &mut peers.speaker,
        &mut peers.capture,
        &mut peers.privacy,
    ] {
        read_backlog(peer);
    }
    let next = controller.snapshot(now()).unwrap();
    rig.publish(next).unwrap();
    let Some(Control::Authority { snapshot }) = second.control.receive().unwrap() else {
        panic!("the next tick's renewal reaches the worker");
    };
    assert_eq!(snapshot.revision(), next.revision());
    assert_eq!(rig.provider.renewal_stall.skipped(), 0, "the run is over");
    rig.running().unwrap();
}

#[test]
fn a_closed_control_socket_still_ends_the_voice() {
    let directory = SessionDirectory::create().unwrap();
    let (mut rig, mut peers, second) = second_only(&directory);
    let (mut controller, _, _) = super::tests::active_reply();
    drop(second);
    // A child another test forks as `second` drops holds a copy of its
    // socket until its exec (CLOEXEC), so for a moment the second worker is
    // still there and a send to it is taken. Publish until one is refused,
    // for at most 5 s, reading what the other workers were sent so that
    // none of them fills (as `send_stop_until_peer_gone` does).
    let until = std::time::Instant::now() + Duration::from_secs(5);
    let error = loop {
        match rig.publish(controller.snapshot(now()).unwrap()) {
            Err(error) => break error,
            Ok(()) => {
                assert!(
                    std::time::Instant::now() < until,
                    "the second worker's peer was not gone after 5 s"
                );
                for peer in [&mut peers.speaker, &mut peers.capture, &mut peers.privacy] {
                    read_backlog(peer);
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    };
    assert_ne!(
        error.kind(),
        io::ErrorKind::WouldBlock,
        "a peer gone is no skipped renewal"
    );
    assert_eq!(rig.provider.renewal_stall.skipped(), 0);
}

/// Twelve's HOLD on #8210: a socket that stays full past
/// `RENEWAL_SKIPS_MOST_US` (3 s) is a worker not coming back, deadlocked
/// with its process alive. Its refusal ends the voice as before, so it
/// restarts, never a lamp deaf until someone steps in.
#[test]
fn a_control_socket_full_past_the_bound_ends_the_voice_as_before() {
    let directory = SessionDirectory::create().unwrap();
    let (mut rig, _peers, _second) = second_only(&directory);
    let (mut controller, _, _) = super::tests::active_reply();
    fill(&mut rig, &mut controller);
    // Under the bound, still skipped.
    rig.provider.renewal_stall.began_before(
        monotonic_us(),
        crate::process::RENEWAL_SKIPS_MOST_US - 100_000,
    );
    rig.publish(controller.snapshot(now()).unwrap())
        .expect("under the bound, a skipped renewal");
    // Past it, the refusal it was.
    rig.provider.renewal_stall.began_before(
        monotonic_us(),
        crate::process::RENEWAL_SKIPS_MOST_US + 1_000,
    );
    let error = rig
        .publish(controller.snapshot(now()).unwrap())
        .expect_err("a socket full past the bound ends the voice");
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
}
