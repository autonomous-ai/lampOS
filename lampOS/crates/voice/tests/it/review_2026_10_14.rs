//! Review of what changed on the night of 2026-10-14
//! (`docs/voice-review-2026-10-14.md`): tests only.
//!
//! The second look (`lamp_voice::second_look`), the note while the person
//! is still speaking (`lamp_voice::called`) and the rule for whom the lamp
//! answers (`lamp_voice::attention`), each with its clock passed in.
//! Worker-level tests of the same night are in
//! `src/provider_worker/tests/review_2026_10_14.rs`.
//!
//! The tests whose names begin `review_` show a fault and fail today. The
//! ones that begin `review_sound_` are edges that were tried and held.
use lamp_voice::{
    attention::{Attention, NAME_BEFORE_US, Verdict},
    called::Called,
    room_told::Room,
    second_look::{BEFORE_US, LEAST_SCORE, SecondLook, WAIT_US},
};

const SECOND: u64 = 1_000_000;

/// Someone calls the lamp plainly ("Hey Lamp", 1.000) and the part that
/// listens for the name hears a little more of the same line a second
/// later, faintly (0.91; it says a hearing from 0.9). The voice is sure
/// someone is speaking only after both: in a busy room a new sound does
/// not take the turn while an earlier answer is awaited (`floor_held`, up
/// to 1.8 s), and over the lamp's own voice a start is held to be sure.
/// Both hearings are inside the 1.5 s before the request began. The model
/// says nothing to the request. The plain hearing should earn the second
/// look; only the newest hearing before a request is kept, and a faint one
/// replaces a plain one, so none is taken. Within a request the plainest
/// is kept (`the_plainest_hearing_in_a_request_is_the_one_that_counts`);
/// before it, the newest.
#[test]
fn review_a_plain_hearing_just_before_a_request_is_not_lost_to_a_fainter_one_after_it() {
    let mut look = SecondLook::default();
    look.name_heard(10 * SECOND, 1.0);
    // A hearing is said at most 0.8 s after it began and the part then
    // passes scores over for a second: 1.2 s on is the soonest another
    // can follow one that was said at once.
    look.name_heard(11 * SECOND + 200_000, 0.91);
    let began = 11 * SECOND + 400_000;
    assert!(10 * SECOND + BEFORE_US >= began, "both are in the window");
    look.request_began(7, began);
    assert_eq!(
        look.finished(7, true, 17 * SECOND),
        Some(1.0),
        "a request called plainly by name and left unanswered is owed a second look"
    );
}

/// A new session in a busy room, and its first heard request is a line to
/// someone with a name like the lamp's ("Hey Liam, the meeting moved."):
/// the name part reports it plainly (three of ten such lines reached 0.98
/// on lamp-4ace, `docs/attention.md`). The model rightly says nothing, so
/// the voice takes a second look: a written turn of its own, which starts
/// a request on the session like any other. The model rightly says nothing
/// to that either. The next line of the two people's talk is then only the
/// second thing the session has heard, inside the two the rule was made
/// for ("the answers to other people's talk came on a new session's first
/// or second request"), but it is counted as the third, and the rule no
/// longer holds its answer. A hello or a look counts the same way; the
/// second look is what makes it happen in a busy room.
#[test]
fn review_the_lamps_own_second_look_does_not_use_up_a_new_sessions_held_requests() {
    use lamp_voice::attention::own_request;
    use std::sync::atomic::Ordering;
    // Request numbers of this test alone: `own_request()` is one for the
    // whole process.
    let (line, look, talk) = (1_001, 1_002, 1_003);
    let mut attention = Attention::default();
    attention.name_heard(100 * SECOND);
    attention.began_at(line, 50, 100 * SECOND + 200_000);
    attention.heard("Hey Liam, the meeting moved.");
    // Nothing comes of it: no output, so nothing to decide.
    own_request().store(look, Ordering::Release);
    attention.began_at(look, 50, 105 * SECOND);
    // Nothing comes of the second look either.
    attention.began_at(talk, 50, 110 * SECOND);
    attention.heard("How was your weekend?");
    let verdict = attention.first_output(talk, Room::Busy, false);
    own_request().store(0, Ordering::Release);
    assert!(
        matches!(verdict, Some(Verdict::Hold(_))),
        "the session's second heard request in a busy room was not held: {verdict:?}"
    );
}

/// The second look's own turn is not a heard request: whatever comes of
/// it, and whatever is heard while it is in hand, nothing more is owed
/// for it. A second look not taken in time is dropped and not taken
/// later. No loop.
#[test]
fn review_sound_a_second_look_is_owed_once_and_its_own_turn_owes_nothing() {
    let mut look = SecondLook::default();
    look.name_heard(10 * SECOND, 1.0);
    look.request_began(1, 10 * SECOND + 300_000);
    assert_eq!(look.finished(1, true, 15 * SECOND), Some(1.0));
    assert_eq!(look.due(15 * SECOND + 10_000), Some(1));
    look.taken();
    // The lamp's own turn is in hand. The name is heard during it (its
    // own voice, or anyone's), and the turn ends with nothing said.
    look.name_heard(16 * SECOND, 1.0);
    assert_eq!(look.finished(2, true, 20 * SECOND), None);
    assert_eq!(look.due(20 * SECOND), None);
    // Owed and never taken: dropped once its moment has gone.
    let mut look = SecondLook::default();
    look.name_heard(30 * SECOND, 0.999);
    look.request_began(3, 30 * SECOND);
    assert!(look.finished(3, true, 35 * SECOND).is_some());
    assert_eq!(look.due(35 * SECOND + WAIT_US + 1), None);
    assert_eq!(look.due(35 * SECOND + 1), None);
    // The least score is the one the note may lean on, and no less.
    let mut look = SecondLook::default();
    look.name_heard(40 * SECOND, LEAST_SCORE);
    look.request_began(4, 40 * SECOND);
    assert_eq!(look.finished(4, true, 45 * SECOND), Some(LEAST_SCORE));
}

/// The note while the person is still speaking is offered once for a
/// request, never for a faint hearing, never for one that began more than
/// 1.5 s before the request was first seen open, and not for a request that
/// opens later however plain the hearing was.
#[test]
fn review_sound_the_note_is_offered_once_for_the_request_the_name_began() {
    let mut called = Called::default();
    called.name_heard(10 * SECOND, 1.0);
    assert_eq!(called.due(Some(1), 10 * SECOND + 300_000), Some(1));
    called.told(1);
    // Hearings go on coming: still once.
    called.name_heard(10 * SECOND + 900_000, 1.0);
    assert_eq!(called.due(Some(1), 11 * SECOND), None);
    // The next request opens after the window of the last hearing.
    assert_eq!(called.due(None, 12 * SECOND), None);
    assert_eq!(
        called.due(Some(2), 10 * SECOND + 900_000 + NAME_BEFORE_US + 1),
        None
    );
    // A faint one, in the middle of a request, tells nothing. (Under the
    // least score, which was 0.98 when this was written and is 0.9 now.)
    called.name_heard(13 * SECOND, LEAST_SCORE - 0.05);
    assert_eq!(called.due(Some(2), 13 * SECOND + 100_000), None);
}

/// Whom the lamp answers: the name heard counts for the request that
/// began with it and for no later one, across a new session too, and in a
/// quiet room or a running session it changes nothing.
#[test]
fn review_sound_a_name_heard_counts_for_its_own_request_only() {
    let mut attention = Attention::default();
    attention.name_heard(10 * SECOND);
    attention.began_at(1, 1, 10 * SECOND + 200_000);
    assert!(matches!(
        attention.first_output(1, Room::Busy, false),
        Some(Verdict::Play(_))
    ));
    // A new session's first request, ten seconds on: the hearing is old.
    attention.began_at(2, 2, 20 * SECOND);
    attention.heard("How was your weekend?");
    assert!(matches!(
        attention.first_output(2, Room::Busy, false),
        Some(Verdict::Hold(_))
    ));
    // Decided once, at the first output: a name heard after that changes
    // nothing for it.
    attention.name_heard(21 * SECOND);
    assert_eq!(attention.first_output(2, Room::Busy, false), None);
    assert!(attention.holds(2));
}
