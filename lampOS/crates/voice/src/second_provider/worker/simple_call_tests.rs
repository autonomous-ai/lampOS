//! `LAMP_SIMPLE_CALL`, the product owner on 27 Nov 2026: "we just start with
//! a simple 'hey lamp' + 20s renewing. then we'll slowly add one thing at a
//! time and always make sure the quality is good.", and then "keep it
//! strictly 'Hey Lamp'. reason: false positive. someone may say Lamp or a
//! word similar to it."
//!
//! With the switch a call is only a line whose words open with "Hey Lamp"
//! (or a tap), and a hearing of the name alone never is, at any score. A line
//! begun within 20 s of the end of the lamp's last words is answered whatever
//! the camera, the room, the voices or its length, and every answer renews
//! the window. Any other line is held. Without the switch, as before: on
//! lamp-dbda (stable 16443) 21 lines were answered as "someone is turned to
//! the lamp" between 15:02 and 15:42 that day, with no "Hey Lamp".
use super::*;
use lamp_msgs::SpeakingState::{No, Yes};

const S: u64 = 1_000_000;

/// A heeding as the product's second route judges (`LAMP_SPOKEN_TO`,
/// `LAMP_SAID_TO_ME`), with the switch or without it; the lamp's last words
/// ended `ended_ago_s` before now, or it never spoke.
fn heeding(simple: bool, ended_ago_s: Option<u64>) -> Heeding {
    let mut heeding = Heeding::new(true, false, None, None);
    heeding.as_the_only_voice(false, true, false);
    heeding.hearing_only_said_to_me();
    if simple {
        heeding.taking_simple_calls();
    }
    heeding.said_until_us = ended_ago_s.map(|ago| monotonic_us().saturating_sub(ago * S));
    heeding
}

/// The eyes seeing one face turned to the lamp, facing and looking at it,
/// its mouth read as `speaking` in five looks through the line.
fn one_face(heeding: &mut Heeding, speaking: lamp_msgs::SpeakingState) {
    use lamp_msgs::{Face, Faces, Speaking};
    let eyes = Arc::new(crate::speaking_faces::SpeakingFaces::default());
    let now = Instant::now();
    for look in 0..5u64 {
        let faces = Faces {
            faces: vec![Face {
                pitch: Some(0.05),
                looking: Some(true),
                id: "f1".into(),
                xmin: 0.4,
                ymin: 0.3,
                xmax: 0.5,
                ymax: 0.45,
                confidence: 1.0,
                yaw: Some(0.1),
                facing: Some(true),
                speaking: Some(Speaking {
                    state: speaking,
                    score: 0.0,
                    from: None,
                }),
                follows: None,
            }],
        };
        eyes.look(
            now + Duration::from_millis(200 + look * 400),
            Some((("b", look + 1), &faces)),
        );
    }
    heeding.seeing_speaking_faces(eyes, None);
}

/// The signs of a line: a face turned to the lamp or none, in `room`, with
/// lamp-name's hearings of the name.
fn signs(turned: Option<bool>, room: crate::room_told::Room, names: Vec<(u64, f32)>) -> Signs {
    Signs {
        room,
        going_on: false,
        lamp_asked: false,
        turned,
        names,
        near: false,
        continues_from: None,
        soft_lead_us: 0,
    }
}

/// The signs of a line lamp-name heard the name at, 100 ms into it: its
/// call, whatever its words.
fn call_at(began_us: u64) -> Signs {
    signs(
        None,
        crate::room_told::Room::Unknown,
        vec![(began_us + 100_000, 0.99)],
    )
}

/// The verdict on `heard`, said from `began_us` for 2.2 s (`None`: let out).
fn judged(
    heeding: &mut Heeding,
    request: u64,
    began_us: u64,
    heard: &str,
    signs: Signs,
) -> Option<&'static str> {
    let verdict = heeding.heed_signs(request, (began_us, began_us + 2_200_000), heard, signs);
    *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
    verdict
}

/// Out of a conversation, one person facing the lamp, seen speaking a
/// question with no "Hey Lamp": answered without the switch (the facing
/// door), held with it.
#[test]
fn simple_call_holds_a_facing_question_out_of_the_window() {
    let _one = crate::provider_worker::tests::one_thread();
    use crate::room_told::Room;
    let facing = |simple: bool| {
        let mut heeding = heeding(simple, None);
        one_face(&mut heeding, Yes);
        let now = monotonic_us();
        let signs = signs(Some(true), Room::Unknown, vec![]);
        judged(&mut heeding, 9, now, "What time is it?", signs)
    };
    assert_eq!(facing(false), None, "off: let in by the face");
    assert_eq!(
        facing(true),
        Some(crate::second_provider::SIMPLE_HELD),
        "on: a face is no call"
    );
}

/// The call is lamp-name's own wake for the line (the product owner, 27
/// Nov 2026, rejecting a call read from the transcript): a hearing of the
/// name for the line's own sound, begun within its start
/// (`HEY_AT_THE_START_US`), whatever the words, at the detector's own
/// decision with no score bar of the rule's. "Hey, Lynn." heard at its start
/// calls, at 0.98 and at 0.7; "Hey Lamp, what time is it?" with no hearing is
/// held; a hearing 3 s into the line, past the start, is held; a line with
/// neither a hearing nor a window is held.
#[test]
fn simple_call_the_detectors_start_hearing_is_the_call() {
    let _one = crate::provider_worker::tests::one_thread();
    use crate::room_told::Room;
    let call = |heard: &str, hearing: Option<(u64, f32)>| {
        let mut heeding = heeding(true, None);
        let now = monotonic_us();
        let names = hearing
            .map(|(into_us, score)| vec![(now + into_us, score)])
            .unwrap_or_default();
        judged(
            &mut heeding,
            9,
            now,
            heard,
            signs(None, Room::Unknown, names),
        )
    };
    let held = Some(crate::second_provider::SIMPLE_HELD);
    assert_eq!(
        call("Hey, Lynn.", Some((100_000, 0.98))),
        None,
        "a start hearing"
    );
    assert_eq!(
        call("Hey, Lynn.", Some((100_000, 0.7))),
        None,
        "no bar of the rule's"
    );
    assert_eq!(
        call("Hey Lamp, what time is it?", None),
        held,
        "words, no hearing"
    );
    assert_eq!(
        call("Hey, Lynn.", Some((3_000_000, 0.98))),
        held,
        "a late hearing"
    );
    assert_eq!(
        call("What time is it?", None),
        held,
        "no hearing, no window"
    );
}

/// In the window, 5 s after the lamp's words: every line is answered, with
/// no face, a face read silent, in a busy room, and a word or two ("Yes.",
/// "Sixty four."). Without the switch a short line a face was read silent
/// through is held.
#[test]
fn simple_call_answers_every_line_in_the_window() {
    let _one = crate::provider_worker::tests::one_thread();
    use crate::room_told::Room;
    let line = |simple: bool, heard: &str, silent_face: bool, room: Room| {
        let mut heeding = heeding(simple, Some(5));
        if silent_face {
            one_face(&mut heeding, No);
        }
        let turned = silent_face.then_some(true);
        let now = monotonic_us();
        judged(&mut heeding, 9, now, heard, signs(turned, room, vec![]))
    };
    for (heard, silent_face, room) in [
        ("And what about tomorrow?", false, Room::Unknown),
        ("Yes.", false, Room::Unknown),
        ("Sixty four.", false, Room::Quiet),
        ("Yes.", true, Room::Quiet),
        ("Can you make it shorter", true, Room::Busy),
        ("Sixty four.", false, Room::Busy),
    ] {
        assert_eq!(
            line(true, heard, silent_face, room),
            None,
            "{heard}, silent face {silent_face}, {room:?}"
        );
    }
    assert!(
        line(false, "Yes.", true, Room::Quiet).is_some(),
        "off: a short line the face was read silent through is held"
    );
}

/// 25 s after the lamp's last words, no "Hey Lamp": held, face or none.
#[test]
fn simple_call_holds_a_line_past_the_window() {
    let _one = crate::provider_worker::tests::one_thread();
    use crate::room_told::Room;
    let mut heeding = heeding(true, Some(25));
    let now = monotonic_us();
    assert_eq!(
        judged(
            &mut heeding,
            9,
            now,
            "And what about tomorrow?",
            signs(None, Room::Unknown, vec![])
        ),
        Some(crate::second_provider::SIMPLE_HELD)
    );
}

/// Every answer renews the window: a follow-up 15 s after the lamp's words,
/// its answer, then a follow-up 15 s after that answer (35 s after the
/// first words) are answered; a line 30 s after the last answer is held.
#[test]
fn simple_call_renews_the_window_with_every_answer() {
    let _one = crate::provider_worker::tests::one_thread();
    use crate::room_told::Room;
    let mut heeding = heeding(true, Some(15));
    let now = monotonic_us();
    let none = || signs(None, Room::Unknown, vec![]);
    assert_eq!(judged(&mut heeding, 9, now, "And tomorrow?", none()), None);
    // Its answer's words end 5 s from now.
    heeding.lamp_did(crate::second_provider::Did::Said(9, now + 5 * S));
    assert_eq!(
        judged(&mut heeding, 10, now + 20 * S, "And the day after?", none()),
        None
    );
    assert_eq!(
        judged(&mut heeding, 11, now + 50 * S, "And the day after?", none()),
        Some(crate::second_provider::SIMPLE_HELD)
    );
}

/// The lamp's own news opens the window as an answer does: a reply 10 s
/// after it began, 7 s after its words ended, is answered.
#[test]
fn simple_call_answers_a_reply_to_the_lamps_news() {
    let _one = crate::provider_worker::tests::one_thread();
    use crate::room_told::Room;
    let mut heeding = heeding(true, None);
    let now = monotonic_us();
    heeding.lamp_did(crate::second_provider::Did::Spoke(now));
    heeding.lamp_did(crate::second_provider::Did::Said(7, now + 3 * S));
    assert_eq!(
        judged(
            &mut heeding,
            9,
            now + 10 * S,
            "Okay, thanks. Remind me later.",
            signs(None, Room::Unknown, vec![])
        ),
        None
    );
}

/// A bare "Hey Lamp." is a call, and its next line within 20 s is answered:
/// after the lamp's "Yes?", and as the line going on from the call before
/// the lamp said anything (`continues_from`).
#[test]
fn simple_call_answers_the_line_after_a_bare_hey_lamp() {
    let _one = crate::provider_worker::tests::one_thread();
    use crate::room_told::Room;
    let none = || signs(None, Room::Unknown, vec![]);
    // After the lamp's "Yes?".
    let mut heeding = heeding(true, None);
    let now = monotonic_us();
    assert_eq!(
        judged(&mut heeding, 9, now, "Hey Lamp.", call_at(now)),
        None
    );
    heeding.lamp_did(crate::second_provider::Did::Said(9, now + 3 * S));
    assert_eq!(
        judged(&mut heeding, 10, now + 5 * S, "What time is it?", none()),
        None
    );
    // Going on from the call, the lamp still waiting for it.
    let mut heeding = self::heeding(true, None);
    let now = monotonic_us();
    assert_eq!(
        judged(&mut heeding, 9, now, "Hey Lamp.", call_at(now)),
        None
    );
    let goes_on = Signs {
        continues_from: Some(9),
        ..none()
    };
    assert_eq!(
        judged(&mut heeding, 10, now + 2 * S, "What time is it?", goes_on),
        None
    );
}

/// The audit's #6: a face turned to the lamp opens no call. A facing
/// question is held, and a line 5 s after it, with the lamp silent, is held
/// too: no 15 s call was opened by the face.
#[test]
fn simple_call_a_face_opens_no_call() {
    let _one = crate::provider_worker::tests::one_thread();
    use crate::room_told::Room;
    let mut heeding = heeding(true, None);
    one_face(&mut heeding, Yes);
    let now = monotonic_us();
    let held = Some(crate::second_provider::SIMPLE_HELD);
    let facing = || signs(Some(true), Room::Quiet, vec![]);
    assert_eq!(
        judged(&mut heeding, 9, now, "What time is it?", facing()),
        held
    );
    assert_eq!(
        judged(&mut heeding, 10, now + 5 * S, "And the weather?", facing()),
        held
    );
}

/// The call's 15 s (`CALL_CARRIES_FOR`, kept): a line 5 s after "Hey Lamp,
/// …", the lamp still silent, is the call's; one 16 s after it is held.
#[test]
fn simple_call_carries_the_call_for_15_s() {
    let _one = crate::provider_worker::tests::one_thread();
    use crate::room_told::Room;
    let none = || signs(None, Room::Unknown, vec![]);
    let mut heeding = heeding(true, None);
    let now = monotonic_us();
    assert_eq!(
        judged(
            &mut heeding,
            9,
            now,
            "Hey Lamp, what's the weather",
            call_at(now)
        ),
        None
    );
    assert_eq!(
        judged(&mut heeding, 10, now + 5 * S, "in Hanoi tomorrow?", none()),
        None
    );
    let mut heeding = self::heeding(true, None);
    let now = monotonic_us();
    assert_eq!(
        judged(
            &mut heeding,
            9,
            now,
            "Hey Lamp, what's the weather",
            call_at(now)
        ),
        None
    );
    assert_eq!(
        judged(&mut heeding, 10, now + 16 * S, "And in Hanoi?", none()),
        Some(crate::second_provider::SIMPLE_HELD)
    );
}

/// A turn a computer opened is a call, as a tap is (the Reviewer-lead's
/// P2 1 on #8892: an explicit call the Harness team relies on,
/// `docs/control.md`; the coordinator opens it only within 8 s of
/// `turn.start`). A line in it with no "Hey Lamp" is answered; so is one
/// just after a head tap.
#[test]
fn simple_call_a_computers_turn_and_a_tap_are_calls() {
    let _one = crate::provider_worker::tests::one_thread();
    use crate::room_told::Room;
    let mut heeding = heeding(true, None);
    heeding.attention.turn_opened(9);
    let now = monotonic_us();
    assert_eq!(
        judged(
            &mut heeding,
            9,
            now,
            "What time is it?",
            signs(None, Room::Unknown, vec![])
        ),
        None
    );
    let mut heeding = self::heeding(true, None);
    let now = monotonic_us();
    // A tap counts for a line it came just before (`TAPPED_BEFORE_US`).
    heeding.attention.head_tapped(now.saturating_sub(100_000));
    assert_eq!(
        judged(
            &mut heeding,
            10,
            now,
            "What time is it?",
            signs(None, Room::Unknown, vec![])
        ),
        None
    );
}

/// The audit's #28 and #29: no other window keeps a line past 20 s. Said
/// 30 s after the lamp's words, a "Yes." to its question with every other
/// window's sign on (the thread's, a skill's yes, the caller's renewal:
/// `going_on`, `lamp_asked`) is held.
#[test]
fn simple_call_no_other_window_keeps_a_line_past_20_s() {
    let _one = crate::provider_worker::tests::one_thread();
    use crate::room_told::Room;
    let mut heeding = heeding(true, Some(30));
    let now = monotonic_us();
    let every_window = Signs {
        going_on: true,
        lamp_asked: true,
        ..signs(None, Room::Quiet, vec![])
    };
    assert_eq!(
        judged(&mut heeding, 9, now, "Yes.", every_window),
        Some(crate::second_provider::SIMPLE_HELD)
    );
}

/// A stop keeps the window (the product owner, 27 Nov 2026: "any
/// conversation has a 20s window so reminder, proactive, chatting, etc."):
/// it runs from the end of the lamp's last words, where the stop cut its
/// answer or what it said after (its "Okay, I'll stop there."). On Fifth's
/// gate on lamp-52e6, after "Okay, okay, never mind." and the lamp's "Okay,
/// I'll stop there.", "So which one is bigger?" was held. After a
/// stop, a line begun 5 s after the lamp's last words, the cut or its
/// acknowledgment, is answered as the conversation going on; one begun 25 s
/// after is held, and so is one with no words; "Hey Lamp" still calls.
#[test]
fn simple_call_a_stop_keeps_the_window_from_the_lamps_last_words() {
    let _one = crate::provider_worker::tests::one_thread();
    use crate::attention::Verdict;
    use crate::room_told::Room;
    let none = || signs(None, Room::Unknown, vec![]);
    // The lamp's last words ended `ended_ago_s` before the stop is told:
    // where the stop cut its answer, or its acknowledgment, said after the
    // cut and ended 9 s after it.
    let stopped = |ended_ago_s: u64, acknowledged: bool| {
        let cut_ago_s = ended_ago_s + if acknowledged { 9 } else { 0 };
        let mut heeding = heeding(true, Some(cut_ago_s));
        if acknowledged {
            let ended_us = monotonic_us().saturating_sub(ended_ago_s * S);
            heeding.lamp_did(crate::second_provider::Did::Said(8, ended_us));
        }
        // Told as the next line begins (its Start's `after_stop`).
        heeding.lamp_did(crate::second_provider::Did::Stopped);
        heeding
    };
    let heard = "So which one is bigger?";
    for acknowledged in [false, true] {
        let mut heeding = stopped(5, acknowledged);
        let now = monotonic_us();
        assert_eq!(
            judged(&mut heeding, 9, now, heard, none()),
            None,
            "5 s after, acknowledged {acknowledged}"
        );
        assert_eq!(
            Heeding::simple_verdict(
                &heeding.attention,
                heeding.said_until_us,
                heeding.called_at,
                now,
                heard
            ),
            Verdict::Play(SIMPLE_GOING_ON),
            "5 s after, acknowledged {acknowledged}"
        );
        let mut heeding = stopped(25, acknowledged);
        let now = monotonic_us();
        assert_eq!(
            judged(&mut heeding, 9, now, heard, none()),
            Some(crate::second_provider::SIMPLE_HELD),
            "25 s after, acknowledged {acknowledged}"
        );
        let mut heeding = stopped(5, acknowledged);
        let now = monotonic_us();
        assert_eq!(
            judged(&mut heeding, 9, now, "", none()),
            Some(crate::second_provider::SIMPLE_NO_WORDS),
            "no words, acknowledged {acknowledged}"
        );
        let mut heeding = stopped(25, acknowledged);
        let now = monotonic_us();
        assert_eq!(
            judged(
                &mut heeding,
                10,
                now,
                "Hey Lamp, and tomorrow?",
                call_at(now)
            ),
            None,
            "a call, acknowledged {acknowledged}"
        );
    }
}

/// A stop ends the lamp's words where it cut them: the rest of an answer
/// sent ahead and never played counts for nothing. Words sent to play 30 s
/// more, stopped now: a line 5 s later is answered as the conversation going
/// on, and one 25 s later is held, though both began before the words sent
/// would have ended.
#[test]
fn simple_call_a_stop_ends_the_lamps_words_where_it_cut_them() {
    let _one = crate::provider_worker::tests::one_thread();
    use crate::room_told::Room;
    let none = || signs(None, Room::Unknown, vec![]);
    let mut heeding = heeding(true, None);
    let now = monotonic_us();
    heeding.lamp_did(crate::second_provider::Did::Said(8, now + 30 * S));
    heeding.lamp_did(crate::second_provider::Did::Stopped);
    assert_eq!(
        judged(
            &mut heeding,
            9,
            now + 5 * S,
            "So which one is bigger?",
            none()
        ),
        None,
        "5 s after the cut"
    );
    assert_eq!(
        judged(&mut heeding, 10, now + 25 * S, "And the other one?", none()),
        Some(crate::second_provider::SIMPLE_HELD),
        "25 s after the cut"
    );
}

/// A stop ends the call's carry (kept): "Hey Lamp, …", stopped before the
/// lamp said anything, and a line 5 s after it is held. The carry covers
/// only a line said right after the call, before the lamp spoke.
#[test]
fn simple_call_a_stop_ends_the_calls_carry() {
    let _one = crate::provider_worker::tests::one_thread();
    use crate::room_told::Room;
    let mut heeding = heeding(true, None);
    let now = monotonic_us();
    assert_eq!(
        judged(
            &mut heeding,
            9,
            now,
            "Hey Lamp, what's the weather",
            call_at(now)
        ),
        None
    );
    heeding.lamp_did(crate::second_provider::Did::Stopped);
    assert_eq!(
        judged(
            &mut heeding,
            10,
            now + 5 * S,
            "in Hanoi tomorrow?",
            signs(None, Room::Unknown, vec![])
        ),
        Some(crate::second_provider::SIMPLE_HELD)
    );
}

/// A line with no words and no call is never answered, in the window or
/// out (#8875's room noise); a line lamp-name heard the name at the start of
/// is a call whatever came back as its words, none included: "Hey Lamp."
/// that speech to text lost (lamp-74ff, 27 Nov, request 143, heard at 1.000).
#[test]
fn simple_call_a_line_with_no_words_is_answered_only_as_a_call() {
    let _one = crate::provider_worker::tests::one_thread();
    use crate::room_told::Room;
    let mut heeding = heeding(true, Some(5));
    let now = monotonic_us();
    assert_eq!(
        judged(&mut heeding, 9, now, "", signs(None, Room::Unknown, vec![])),
        Some(crate::second_provider::SIMPLE_NO_WORDS)
    );
    let mut heeding = self::heeding(true, None);
    let now = monotonic_us();
    assert_eq!(judged(&mut heeding, 10, now, "", call_at(now)), None);
}

/// The audit's #10 and the switches read through
/// [`crate::second_provider::unless_simple`] (the second look, the name
/// note, the room's facts, the cut-in notes and asks, the other language):
/// off with the rule, as set without it.
#[test]
fn simple_call_turns_the_switches_read_through_it_off() {
    use crate::second_provider::unless_simple_with;
    assert_eq!(unless_simple_with(Some("on".into()), true), None);
    assert_eq!(
        unless_simple_with(Some("on".into()), false).as_deref(),
        Some("on")
    );
    assert_eq!(unless_simple_with(None, false), None);
}

/// The window's edges (the Reviewer-lead's P3): a line begun 19 s after the
/// end of the lamp's last words is answered, one begun 21 s after is held.
#[test]
fn simple_call_the_windows_edges_are_19_and_21_s() {
    let _one = crate::provider_worker::tests::one_thread();
    use crate::room_told::Room;
    let line = |ended_ago_s: u64| {
        let mut heeding = heeding(true, Some(ended_ago_s));
        let now = monotonic_us();
        judged(
            &mut heeding,
            9,
            now,
            "Sixty four.",
            signs(None, Room::Unknown, vec![]),
        )
    };
    assert_eq!(line(19), None, "19 s");
    assert_eq!(line(21), Some(crate::second_provider::SIMPLE_HELD), "21 s");
}

/// A spoken "Stop." in the window is judged as a line, as any line is: let
/// out (the Reviewer-lead's P3). A stop said over the lamp's own voice is
/// the coordinator's spoken stop, unchanged.
#[test]
fn simple_call_a_spoken_stop_in_the_window_is_judged_as_a_line() {
    let _one = crate::provider_worker::tests::one_thread();
    use crate::room_told::Room;
    let mut heeding = heeding(true, Some(5));
    let now = monotonic_us();
    assert_eq!(
        judged(
            &mut heeding,
            9,
            now,
            "Stop.",
            signs(None, Room::Unknown, vec![])
        ),
        None
    );
}
