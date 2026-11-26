//! #7332: on the owner's lamp-74ff on 16 Nov (13500), in a busy room, a
//! back-and-forth got stuck. r43 called the lamp and r44 followed it up;
//! the model called `not_for_me` on both, so neither of the busy room's
//! 15 s clocks moved, and r45, judged 16 s after the call, was held ("a
//! busy room, and nothing shows it was called"); r46 was then held as "a
//! reply to a question the lamp left unanswered" (`AFTER_QUESTION`), the
//! question being the owner's own r45.
//!
//! Each scene is played with `LAMP_BUSY_FOLLOW_UP_RENEWS` off, as the
//! product runs today, and on; both with `LAMP_SPOKEN_TO` and
//! `LAMP_WINDOW_FROM_START`, as the product sets them, and the eyes'
//! speaking faces read.
use super::*;

/// One step of a scene, at `at` ms from its start.
#[derive(Clone, Copy)]
enum Step {
    /// A line said from `at` for `long` ms, judged at its end, with `face`
    /// seen speaking it. Let out, the model declines it: the lamp says
    /// nothing, as on lamp-74ff.
    Line {
        at: u64,
        long: u64,
        heard: &'static str,
        face: &'static str,
    },
    /// The lamp's own words (a reminder's), said from `at` for `long` ms.
    Says { at: u64, long: u64 },
}

const OWNER: &str = "f1";
const COLLEAGUE: &str = "f2";

const fn line(at: u64, long: u64, heard: &'static str, face: &'static str) -> Step {
    Step::Line {
        at,
        long,
        heard,
        face,
    }
}

/// One frame of the eyes: `face` seen speaking, turned to the lamp.
fn frame(face: &str) -> lamp_msgs::Faces {
    use lamp_msgs::{Face, Speaking, SpeakingState};
    lamp_msgs::Faces {
        faces: vec![Face {
            pitch: None,
            looking: None,
            id: face.to_owned(),
            xmin: 0.2,
            ymin: 0.2,
            xmax: 0.4,
            ymax: 0.5,
            confidence: 1.0,
            yaw: None,
            facing: Some(true),
            speaking: Some(Speaking {
                state: SpeakingState::Yes,
                score: 0.9,
                from: None,
            }),
            follows: None,
        }],
    }
}

/// What became of a line: why it was held, if it was, and the note the
/// model is told beside it.
type Judged = (Option<&'static str>, Option<&'static str>);

/// Plays `steps` in a busy room: each line's verdict and note, in order.
fn play(renews: bool, steps: &[Step]) -> Vec<Judged> {
    play_with(renews, false, steps)
}

/// [`play`], with `LAMP_DECLINE_SILENT` when `decline_silent`: a decline
/// then rolls back what letting its line out began.
fn play_with(renews: bool, decline_silent: bool, steps: &[Step]) -> Vec<Judged> {
    play_all(renews, decline_silent, false, steps)
}

/// [`play_with`], with `LAMP_BUSY_NO_FACE_RENEWS` when `no_face`. A line
/// whose face is `""` has no face seen speaking it.
fn play_all(renews: bool, decline_silent: bool, no_face: bool, steps: &[Step]) -> Vec<Judged> {
    play_full(renews, decline_silent, no_face, false, steps)
}

/// [`play_all`], with `LAMP_NO_FACE_FOLLOW_UP_BY_VOICE` when `by_voice`. No
/// line here has a print, so none is found in the conversation's own voice.
fn play_full(
    renews: bool,
    decline_silent: bool,
    no_face: bool,
    by_voice: bool,
    steps: &[Step],
) -> Vec<Judged> {
    let _one = crate::provider_worker::tests::one_thread();
    let _clock =
        crate::scenario_clock::Scope::enter_at(Duration::from_secs(10), 600_000_000).unwrap();
    let ms = Duration::from_millis;
    *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
    *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
    let mut heeding = Heeding::new(true, true, None, None);
    heeding.as_the_only_voice(false, true, false);
    heeding.measuring_windows_from_start();
    if renews {
        heeding.renewing_busy_follow_ups();
    }
    if decline_silent {
        heeding.declining_silently();
    }
    if no_face {
        heeding.renewing_no_face_follow_ups();
    }
    if by_voice {
        heeding.renewing_no_face_follow_ups_by_voice();
    }
    let eyes = Arc::new(crate::speaking_faces::SpeakingFaces::default());
    heeding.seeing_speaking_faces(eyes.clone(), None);
    let mut seq = 0;
    let mut said: Vec<(u64, u64)> = Vec::new();
    let mut was = 0;
    let mut out = Vec::new();
    let mut other = 900;
    for (request, step) in (1..).zip(steps) {
        let (at, long) = match *step {
            Step::Line { at, long, .. } | Step::Says { at, long } => (at, long),
        };
        if let Step::Says { .. } = step {
            said.push((at, at + long));
            continue;
        }
        let Step::Line { heard, face, .. } = *step else {
            continue;
        };
        // Judged at its end: time passes to it.
        let judged = at + long;
        let passed = ms(judged.saturating_sub(was));
        was = judged;
        heeding.called_at = heeding.called_at.map(|at| at - passed);
        heeding.call_renewed_at = heeding.call_renewed_at.map(|at| at - passed);
        let now = Instant::now();
        let now_us = monotonic_us();
        // The lamp's words so far, by the clocks the rule reads.
        let mut thread = crate::thread::Thread::new();
        for (from, until) in &said {
            thread.person(
                "Remind me when the tea is ready.",
                true,
                now - ms(judged - from),
            );
            thread.lamp(50, "Your tea is ready.", now - ms(judged - from));
            let until_us = Some(now_us - (judged - until) * 1_000);
            heeding.said_until_us = heeding.said_until_us.max(until_us);
            heeding.said_until_free_us = heeding.said_until_free_us.max(until_us);
        }
        *crate::thread::THREAD.lock().unwrap() = thread;
        // People talk at the desk: the room stays busy.
        {
            let mut room = crate::room_told::room().lock().unwrap();
            for _ in 0..3 {
                other += 1;
                room.unanswered(other, now_us);
            }
            assert_eq!(room.at(now_us), crate::room_told::Room::Busy);
        }
        // The scene's clock stands still: each line's sound has its own
        // span, so the eyes' looks of one line are never another's.
        let window = (10_000_000 + at * 1_000, 10_000_000 + judged * 1_000);
        let seen = frame(face);
        for look in 0..4u64 {
            if face.is_empty() {
                break;
            }
            seq += 1;
            let at_us = window.0 + (window.1 - window.0) * (2 * look + 1) / 8;
            eyes.look(
                now - Duration::from_micros(now_us - at_us),
                Some((("b", seq), &seen)),
            );
        }
        heeding.hear(
            request,
            crate::second_provider::Heard::Began {
                over_the_lamps_voice: false,
            },
        );
        let held = heeding.heed(request, window, heard, None);
        let note = heeding.conversation_note_for(request);
        // Let out, the model declines it: nothing is said. With
        // `LAMP_DECLINE_SILENT` the call itself is taken as answered (its
        // words aside), since its decline would roll the call back.
        if held.is_none() && !(decline_silent && request == 1) {
            heeding.lamp_did(crate::second_provider::Did::Declined(request));
        }
        out.push((held, note));
    }
    *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
    *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
    out
}

fn let_out(judged: &[Judged]) -> Vec<bool> {
    judged.iter().map(|(held, _)| held.is_none()).collect()
}

/// lamp-74ff's r43 to r46, in the owner's face: the call, a follow-up the
/// model declines, a question 17 s after the call, and one more 9 s after
/// that. Off, as on the lamp, the last two are held; on, each follow-up
/// the lamp let out renews the call, so all four are let out. A colleague's
/// line let out in the call's window renews nothing.
#[test]
fn busy_follow_up_declined_still_renews_the_call() {
    let steps = [
        line(0, 2_000, "Lamp, can you remember my face?", OWNER),
        line(
            8_000,
            3_000,
            "And what was the name I told you yesterday?",
            OWNER,
        ),
        line(17_000, 2_000, "Did you get that?", OWNER),
        line(26_000, 2_000, "Hello, are you listening to me?", OWNER),
    ];
    assert_eq!(let_out(&play(false, &steps)), [true, true, false, false]);
    assert_eq!(let_out(&play(true, &steps)), [true; 4]);
    // The follow-up in someone else's face renews nothing.
    let colleague = [
        steps[0],
        line(
            8_000,
            3_000,
            "And what was the name I told you yesterday?",
            COLLEAGUE,
        ),
        steps[2],
    ];
    assert_eq!(let_out(&play(true, &colleague)), [true, true, false]);
}

/// The 1st's point 1 on #7332: a follow-up let out in the caller's face is
/// told it is their follow-up, to be answered unless plainly said to
/// someone else, not that nobody called the lamp. Off, or in another face,
/// it is told [`CONVERSATION_ALONE`] as before.
#[test]
fn busy_follow_up_is_told_it_is_the_callers() {
    let follow_up = |renews: bool, face: &'static str| {
        play(
            renews,
            &[
                line(0, 2_000, "Lamp, can you remember my face?", OWNER),
                line(
                    8_000,
                    3_000,
                    "And what was the name I told you yesterday?",
                    face,
                ),
            ],
        )[1]
    };
    assert_eq!(follow_up(false, OWNER), (None, Some(CONVERSATION_ALONE)));
    assert_eq!(follow_up(true, OWNER), (None, Some(FOLLOW_UP_NOTE)));
    assert_eq!(follow_up(true, COLLEAGUE), (None, Some(CONVERSATION_ALONE)));
}

/// Sixth's second bug on #7332: the busy room's 15 s were measured to the
/// line's judging, though the conversation's check measures to its start
/// (`LAMP_WINDOW_FROM_START`). A follow-up begun 12 s after the call and
/// spoken for 5 s is judged 17 s after it: off, held; on, let out.
#[test]
fn busy_window_is_measured_to_the_line_start() {
    let steps = [
        line(
            0,
            2_000,
            "Lamp, what's the weather like this afternoon?",
            OWNER,
        ),
        line(
            14_000,
            5_000,
            "And what about tomorrow, will I need an umbrella for the walk to work?",
            OWNER,
        ),
    ];
    assert_eq!(let_out(&play(false, &steps)), [true, false]);
    assert_eq!(let_out(&play(true, &steps)), [true, true]);
}

/// r46's hold: the owner's own question, held, made the owner's next line
/// "a reply to a question the lamp left unanswered". Here the owner's
/// question is held 20 s after the call; the lamp then says a reminder, and
/// the owner's follow-up to it, in the conversation going on from those
/// words, is held off as that reply, and let out on. In a colleague's face
/// it is still held.
#[test]
fn own_held_question_is_no_reply_to_another() {
    let steps = |face: &'static str| {
        [
            line(0, 2_000, "Lamp, can you remember my face?", OWNER),
            line(20_000, 2_000, "Are you still there?", OWNER),
            Step::Says {
                at: 23_000,
                long: 2_000,
            },
            line(
                26_000,
                2_000,
                "Thanks, and can you remind me again in an hour?",
                face,
            ),
        ]
    };
    let off = play(false, &steps(OWNER));
    assert_eq!(let_out(&off), [true, false, false]);
    // Held as no conversation: the reply to the held question
    // (`attention::AFTER_QUESTION`) is going on no more.
    assert!(off[2].0.is_some());
    assert_eq!(let_out(&play(true, &steps(OWNER))), [true, false, true]);
    assert_eq!(
        let_out(&play(true, &steps(COLLEAGUE))),
        [true, false, false]
    );
}

/// Sixth's P2 1 on #7349: with `LAMP_DECLINE_SILENT` too, the follow-up's
/// decline rolls back to the call before it, but keeps the caller's
/// renewal, so lamp-74ff's r45 and r46 are still let out; with it alone
/// they are held as before.
#[test]
fn a_silent_decline_keeps_the_callers_renewal() {
    let steps = [
        line(0, 2_000, "Lamp, can you remember my face?", OWNER),
        line(
            8_000,
            3_000,
            "And what was the name I told you yesterday?",
            OWNER,
        ),
        line(17_000, 2_000, "Did you get that?", OWNER),
        line(26_000, 2_000, "Hello, are you listening to me?", OWNER),
    ];
    assert_eq!(
        let_out(&play_with(false, true, &steps)),
        [true, true, false, false]
    );
    assert_eq!(let_out(&play_with(true, true, &steps)), [true; 4]);
}

/// The Reviewer-lead's P2 on #7349, #7297's half on the owner's lamp: the
/// owner's declined follow-up renews the call for the owner alone. A
/// colleague's line 20 s after the call, their own face seen speaking it,
/// is held as without the switch, while the owner's line there is let out.
#[test]
fn the_renewed_call_carries_only_the_caller() {
    let steps = |face: &'static str| {
        [
            line(0, 2_000, "Lamp, can you remember my face?", OWNER),
            line(
                8_000,
                3_000,
                "And what was the name I told you yesterday?",
                OWNER,
            ),
            line(
                20_000,
                2_000,
                "Did you send the slides to the client?",
                face,
            ),
        ]
    };
    assert_eq!(
        let_out(&play(false, &steps(COLLEAGUE))),
        [true, true, false]
    );
    assert_eq!(let_out(&play(true, &steps(COLLEAGUE))), [true, true, false]);
    assert_eq!(let_out(&play(true, &steps(OWNER))), [true, true, true]);
}

/// lamp-74ff on 17 Nov (13706), r47 to r53: the owner calls the lamp, then
/// follows up three times with no face seen speaking, the model declining
/// each, and speaks again 22 s and 35 s after the call. With
/// `LAMP_BUSY_FOLLOW_UP_RENEWS` alone, as on the lamp, the no-face
/// follow-ups renew nothing and the late lines are held; with
/// `LAMP_BUSY_NO_FACE_RENEWS` too, each follow-up renews the call and all
/// are let out. A colleague's face seen speaking a late line still renews
/// nothing and is held.
#[test]
fn a_no_face_follow_up_renews_the_call() {
    let steps = |late: &'static str| {
        [
            line(0, 2_000, "Lamp, can you remember my face?", OWNER),
            line(
                6_000,
                4_000,
                "And the one I told you about yesterday too.",
                "",
            ),
            line(12_000, 2_000, "You know, the one from the office.", ""),
            line(22_000, 1_000, "Are you still there?", ""),
            line(35_000, 1_000, "Hello, can you hear me?", late),
        ]
    };
    assert_eq!(
        let_out(&play_all(true, false, false, &steps(""))),
        [true, true, true, false, false]
    );
    assert_eq!(let_out(&play_all(true, false, true, &steps(""))), [true; 5]);
    assert_eq!(
        let_out(&play_all(true, false, true, &steps(COLLEAGUE))),
        [true, true, true, true, false]
    );
    // The switch alone, without the renewal, changes nothing.
    assert_eq!(
        let_out(&play_all(false, false, true, &steps(""))),
        let_out(&play_all(false, false, false, &steps("")))
    );
}

/// `LAMP_NO_FACE_FOLLOW_UP_BY_VOICE` (lamp-74ff, 26 Nov 2026: in October's
/// two-person battery, 21 lines of two people's talk with no face seen and
/// no print were each told they were the caller's follow-up, and all were
/// answered). r47 to r53's scene again, its no-face lines with no print:
/// with the switch none is the caller's own, so each let out in the call's
/// window is told it may be people's talk ([`CONVERSATION_ALONE`]) and
/// renews nothing, and the late lines are held, as with
/// `LAMP_BUSY_FOLLOW_UP_RENEWS` alone. Without it each is the caller's.
#[test]
fn by_voice_a_no_face_follow_up_without_the_conversations_voice_renews_nothing() {
    let steps = [
        line(0, 2_000, "Lamp, can you remember my face?", OWNER),
        line(
            6_000,
            4_000,
            "And the one I told you about yesterday too.",
            "",
        ),
        line(12_000, 2_000, "You know, the one from the office.", ""),
        line(22_000, 1_000, "Are you still there?", ""),
        line(35_000, 1_000, "Hello, can you hear me?", ""),
    ];
    let by_voice = play_full(true, false, true, true, &steps);
    assert_eq!(let_out(&by_voice), [true, true, true, false, false]);
    assert_eq!(by_voice[1].1, Some(CONVERSATION_ALONE));
    assert_eq!(by_voice[2].1, Some(CONVERSATION_ALONE));
    let before = play_full(true, false, true, false, &steps);
    assert_eq!(let_out(&before), [true; 5]);
    assert_eq!(before[1].1, Some(FOLLOW_UP_NOTE));
}

/// The rule: with no face seen, only the conversation's own voice keeps a
/// line the caller's; a face seen speaking it is the caller's-face rule's,
/// and off nothing changes.
#[test]
fn by_voice_only_the_conversations_own_voice_keeps_a_no_face_line_the_callers() {
    let none = crate::speaking_faces::Speakers::new();
    let one = vec![OWNER.to_owned()];
    assert!(unvoiced_follow_up(true, &none, false), "no print");
    assert!(
        !unvoiced_follow_up(true, &none, true),
        "the conversation's own voice"
    );
    assert!(!unvoiced_follow_up(false, &none, false), "off");
    assert!(
        !unvoiced_follow_up(true, &one, false),
        "a face seen speaking"
    );
}
