//! A later stream replacing one that plays ([`open_stream_turn`]).
use super::*;

fn controller() -> Controller {
    let mut owner = Controller::new(new_boot().unwrap(), now());
    owner
        .set_microphone_permission(now(), Permission::Allowed)
        .unwrap();
    owner
}

/// Input leases for the next admit, from now.
fn renew_input(owner: &mut Controller) {
    let until = MonoTime::from_micros(now().as_micros() + INPUT_LEASE_US);
    owner
        .set_capture(now(), CaptureState::RetainingUntil(until))
        .unwrap();
    owner
        .set_admission(now(), AdmissionState::OpenUntil(until))
        .unwrap();
}

#[test]
fn a_later_stream_is_told_the_old_one_dropped_and_takes_the_turn() {
    let mut owner = controller();
    let mut reply = None;
    let mut trace = SessionState::default();
    renew_input(&mut owner);
    let (first, first_answered) = crate::say::ticket_for_test("control");
    let first_feed = crate::speaker_stream::Feed::new();
    open_stream_turn(
        &mut owner,
        &mut reply,
        first,
        std::sync::Arc::clone(&first_feed),
        false,
        PlayoutStart::continuity(),
        &mut trace,
        |_| Ok(()),
    )
    .unwrap();
    let first_owner = reply.as_ref().unwrap().owner;
    assert_eq!(
        reply.as_ref().unwrap().notice.origin(),
        SpeechOrigin::ComputerStream
    );
    assert!(reply.as_ref().unwrap().say.is_some());
    // A stream that plays, then a later one the voice admits.
    let mut published = Vec::new();
    renew_input(&mut owner);
    let (second, second_answered) = crate::say::ticket_for_test("control");
    let second_feed = crate::speaker_stream::Feed::new();
    open_stream_turn(
        &mut owner,
        &mut reply,
        second,
        std::sync::Arc::clone(&second_feed),
        true,
        PlayoutStart::continuity(),
        &mut trace,
        |state| {
            published.push(state.owner());
            Ok(())
        },
    )
    .unwrap();
    // The speaker is revoked first: the first authority published after the
    // old stream's cut has no owner, and only then the new turn's. So the
    // old stream's queued sound is an expected discard, never a cancelled
    // tail (lamp-75c9, 13 Nov: it ended the voice).
    assert_eq!(published.first(), Some(&None));
    assert!(published.iter().skip(1).any(Option::is_some));
    assert!(trace.receipts.discard(first_owner));
    // The old one is told why, and nothing else.
    match first_answered.recv_timeout(Duration::from_secs(1)).unwrap() {
        crate::say::Answer::Done(crate::say::Outcome::Dropped(why)) => {
            assert_eq!(why, crate::say::STREAM_REPLACED);
        }
        _ => panic!("the old stream was not told dropped, replaced"),
    }
    // The new turn is the lamp's, its ticket held and still unanswered.
    let current = reply.as_ref().unwrap();
    assert_ne!(current.owner, first_owner);
    assert_eq!(current.notice.origin(), SpeechOrigin::ComputerStream);
    assert!(current.say.is_some());
    assert!(second_answered.try_recv().is_err());
    // The old turn is cancelled: the controller no longer has it as its own.
    assert!(owner.cancel_turn(now(), first_owner).is_err());
}

#[test]
fn a_due_reminder_ends_the_stream_that_plays_and_nothing_else() {
    let mut owner = controller();
    let mut reply = None;
    let mut trace = SessionState::default();
    // Nothing plays: nothing is ended.
    assert!(
        !end_stream_for(
            &mut owner,
            &mut reply,
            &mut trace,
            "a reminder is due",
            crate::receipt_history::STREAM_REMINDER_DUE,
            |_| Ok(()),
        )
        .unwrap()
    );
    renew_input(&mut owner);
    let (ticket, answered) = crate::say::ticket_for_test("control");
    open_stream_turn(
        &mut owner,
        &mut reply,
        ticket,
        crate::speaker_stream::Feed::new(),
        false,
        PlayoutStart::continuity(),
        &mut trace,
        |_| Ok(()),
    )
    .unwrap();
    let turn = reply.as_ref().unwrap().owner;
    let mut published = Vec::new();
    assert!(
        end_stream_for(
            &mut owner,
            &mut reply,
            &mut trace,
            "a reminder is due",
            crate::receipt_history::STREAM_REMINDER_DUE,
            |state| {
                published.push(state.owner());
                Ok(())
            },
        )
        .unwrap()
    );
    // The speaker is revoked first: the authority published is ownerless,
    // and its queued sound is an expected discard, not a cancelled tail.
    assert_eq!(published, vec![None]);
    assert!(trace.receipts.discard(turn));
    // The stream is told why, the reply is gone, the turn is cancelled.
    match answered.recv_timeout(Duration::from_secs(1)).unwrap() {
        crate::say::Answer::Done(crate::say::Outcome::Dropped(why)) => {
            assert_eq!(why, "a reminder is due");
        }
        _ => panic!("the stream was not told dropped, a reminder is due"),
    }
    assert!(reply.is_none());
    assert!(owner.cancel_turn(now(), turn).is_err());
}

#[test]
fn an_invalid_dispatch_time_leaves_the_streams_chunk_in_its_feed() {
    let feed = crate::speaker_stream::Feed::new();
    feed.push(&vec![500i16; 3_000]);
    let mut delivery = NoticeDelivery::stream(std::sync::Arc::clone(&feed));
    // At 0 the dispatch is refused before the feed is touched.
    assert!(delivery.reserve_chunk(0).is_err());
    assert!(feed.ready(), "the chunk was taken by a refused dispatch");
    let (samples, last, first) = delivery.reserve_chunk(1).unwrap();
    assert_eq!((samples.len(), last, first), (240, false, true));
}
