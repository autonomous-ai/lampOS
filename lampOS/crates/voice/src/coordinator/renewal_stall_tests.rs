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
    let (mut rig, _peers, second) = second_only(&directory);
    let (mut controller, _, _) = super::tests::active_reply();
    drop(second);
    let error = rig
        .publish(controller.snapshot(now()).unwrap())
        .expect_err("a peer gone is no skipped renewal");
    assert_ne!(error.kind(), io::ErrorKind::WouldBlock);
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

/// The Reviewer-lead's P2 on #8687: the link's check asked before a ring
/// on a full control socket (the worker stuck in a slow call, as in an
/// outage) never ends the voice. It is left unasked, and asked once room
/// frees.
#[test]
fn a_link_check_on_a_full_control_socket_waits_for_room_and_is_asked_once() {
    let directory = SessionDirectory::create().unwrap();
    let (mut rig, mut peers, mut second) = second_only(&directory);
    let (mut controller, _, _) = super::tests::active_reply();
    fill(&mut rig, &mut controller);
    let key = (4, 1);
    let mut asked = None;
    let full = ask_link_check(&mut rig.provider, key, &mut asked)
        .expect("a full control socket is no exit");
    assert!(!full && asked.is_none(), "not asked yet");
    for peer in [
        &mut second,
        &mut peers.speaker,
        &mut peers.capture,
        &mut peers.privacy,
    ] {
        read_backlog(peer);
    }
    assert!(ask_link_check(&mut rig.provider, key, &mut asked).unwrap());
    assert_eq!(asked, Some(key));
    assert!(matches!(
        second.control.receive().unwrap(),
        Some(Control::CheckLink)
    ));
    assert!(second.control.receive().unwrap().is_none(), "asked once");
    rig.running().unwrap();
}

/// A peer gone still ends the voice, as for every other send.
#[test]
fn a_link_check_on_a_closed_control_socket_still_ends_the_voice() {
    let directory = SessionDirectory::create().unwrap();
    let (mut rig, _peers, second) = second_only(&directory);
    drop(second);
    let mut asked = None;
    let error = ask_link_check(&mut rig.provider, (4, 1), &mut asked)
        .expect_err("a peer gone is no full socket");
    assert_ne!(error.kind(), io::ErrorKind::WouldBlock);
    assert!(asked.is_none());
}
