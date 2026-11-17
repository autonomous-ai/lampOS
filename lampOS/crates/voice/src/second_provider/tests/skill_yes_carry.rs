//! `LAMP_SKILL_YES_CARRY` (#7148): while a skill's yes is awaited after its
//! say-back (#6749 C), a known person's yes no face was seen saying is
//! carried for 45 s from the end of that say-back as played, not from their
//! last exchange's start; a short one is read by #7422's short-line rule.
//! Everything else of the carry (#4339) stays.
use super::*;
use crate::speaking_faces::SpeakingFaces;
use lamp_msgs::SpeakingState::{self, No, Yes};

/// The say-back the lamp says whole in the make's request.
const SAY_BACK: &str = "It mails Mai from your Gmail when you say you're leaving.";
/// The make's request, whose answer is the say-back; the yes is the next.
const MADE: u64 = 7;
const YES: u64 = 8;
/// The monotonic clock, in µs, as the yes is judged.
const NOW_US: u64 = 900_000_000;

/// lamp-9adf, 15 Nov: the agent's make and its say-back ran 70 s from the
/// start of the person's last exchange, the say-back about 30 s of it, and
/// the yes came 40 s after it ended. The thread holds A's exchange begun 70
/// s before `now`; a made skill's yes waits, its say-back said whole and
/// played until 40 s before `now` (`NOW_US`).
fn after_a_long_say_back(now: Instant) -> crate::thread::Thread {
    crate::skill_write_yes::clear_for_test();
    let began = now.checked_sub(Duration::from_secs(70)).unwrap();
    crate::skill_write_yes::wait_for_yes(
        crate::skill_write_yes::Asks {
            skill: "leaving-mail".to_owned(),
            sha256: "ab".repeat(32),
            writes: Vec::new(),
            say_back: SAY_BACK.to_owned(),
            grants: None,
            code: true,
            remove: false,
        },
        MADE,
        began,
    );
    let mut thread = crate::thread::Thread::new();
    thread.lamp_for(
        MADE,
        SAY_BACK,
        began,
        Some(&crate::remember::person_id("A")),
    );
    crate::skill_write_yes::played_until(MADE, NOW_US - 40_000_000);
    thread
}

/// Whose conversation the carry finds going on for the yes at `now`, the
/// monotonic clock at `now_us`, with the switch `on`.
fn going_on_for_the_yes(
    thread: &crate::thread::Thread,
    on: bool,
    now: Instant,
    now_us: u64,
) -> Option<(String, bool)> {
    let asked = asked_for_carry(
        on,
        || crate::skill_write_yes::asked_until(YES, now),
        now_us,
        now,
    );
    thread.going_on_asked(YES, now, asked)
}

/// The eyes' looks every 250 ms over 2 s under boot `b`, each the faces
/// `at(ms)` gives with their mouth reads; the line is the last 0.4 s, a
/// one-word "yes" spanning two looks (#7422's `short_yes_seen`).
fn yes_seen(
    at: &dyn Fn(u64) -> Vec<(&'static str, SpeakingState)>,
) -> (SpeakingFaces, Instant, Instant) {
    let eyes = SpeakingFaces::default();
    let start = Instant::now();
    for (seq, ms) in (0..=2000u64).step_by(250).enumerate() {
        let faces = lamp_msgs::Faces {
            faces: at(ms)
                .into_iter()
                .map(|(id, state)| lamp_msgs::Face {
                    id: id.to_owned(),
                    xmin: 0.1,
                    ymin: 0.1,
                    xmax: 0.3,
                    ymax: 0.4,
                    confidence: 1.0,
                    yaw: None,
                    pitch: None,
                    facing: Some(true),
                    looking: None,
                    speaking: Some(lamp_msgs::Speaking {
                        state,
                        score: 0.9,
                        from: None,
                    }),
                    follows: None,
                })
                .collect(),
        };
        eyes.look(
            start + Duration::from_millis(ms),
            Some((("b", seq as u64 + 1), &faces)),
        );
    }
    (
        eyes,
        start + Duration::from_millis(1600),
        start + Duration::from_millis(2000),
    )
}

fn engine() -> Engine {
    Engine::new(
        Arc::new(Script::default()),
        Told::default,
        Arc::new(|_, _, _, _| panic!("no call runs here")),
    )
}

/// #7148's failing case: a yes 40 s after a 30 s say-back, the person's
/// last exchange begun 70 s before it. With the switch on, the carry counts
/// from the say-back's end, so A's conversation is going on and their
/// unseen yes, their face alone in view, is carried; off, it counts from
/// the exchange, and the yes is the room's, as on lamp-9adf.
#[test]
fn an_unseen_yes_40_s_after_a_long_say_back_is_carried_with_the_switch() {
    let now = Instant::now();
    let thread = after_a_long_say_back(now);
    let a = crate::remember::person_id("A");
    assert_eq!(
        crate::skill_write_yes::asked_until(YES, now),
        Some((MADE, NOW_US - 40_000_000)),
        "the say-back's request and its end as played"
    );
    assert_eq!(
        going_on_for_the_yes(&thread, true, now, NOW_US),
        Some((a.clone(), false)),
        "on: 40 s from the say-back's end"
    );
    assert_eq!(
        going_on_for_the_yes(&thread, false, now, NOW_US),
        None,
        "off: 70 s from the exchange's start, past 45 s"
    );
    // The whole carry read: their face alone in view and never read
    // speaking, so nobody was seen saying it.
    let engine = engine();
    let (eyes, begun, finishing) = yes_seen(&|_| vec![("f1", No)]);
    let speakers = skill_yes_speakers(
        &eyes,
        eyes.speakers_with_boot(begun, finishing),
        true,
        begun,
        finishing,
    );
    assert!(speakers.is_empty());
    let carried = |on: bool| {
        unseen_carry(
            &engine.shared,
            &eyes,
            &speakers,
            Some("b/f1"),
            (begun, finishing),
            &a,
            || going_on_for_the_yes(&thread, on, now, NOW_US),
        )
    };
    assert_eq!(carried(true), Some(false), "on: carried in full");
    assert_eq!(carried(false), None, "off: the room's");
    crate::skill_write_yes::clear_for_test();
}

/// The window closes 45 s after the say-back's end as played: a yes 46 s
/// after it is the room's, switch on or not. A say-back still playing
/// counts from now, and no yes awaited leaves the carry as before.
#[test]
fn a_yes_past_45_s_from_the_say_back_is_not_carried() {
    let now = Instant::now();
    let thread = after_a_long_say_back(now);
    let later = now + Duration::from_secs(6);
    assert_eq!(
        going_on_for_the_yes(&thread, true, later, NOW_US + 6_000_000),
        None,
        "on: 46 s from the say-back's end"
    );
    // Its play not yet ended: from now at the soonest.
    assert_eq!(
        asked_for_carry(true, || Some((MADE, NOW_US + 1)), NOW_US, now),
        Some((MADE, now))
    );
    // No yes awaited: nothing changes.
    crate::skill_write_yes::clear_for_test();
    assert_eq!(crate::skill_write_yes::asked_until(YES, now), None);
    assert_eq!(going_on_for_the_yes(&thread, true, now, NOW_US), None);
}

/// Someone else answers after the say-back: an exchange of theirs after
/// A's ends A's conversation, and the say-back's end moves nothing.
#[test]
fn another_exchange_after_the_say_back_ends_the_carry() {
    let now = Instant::now();
    let mut thread = after_a_long_say_back(now);
    let b = crate::remember::person_id("B");
    thread.lamp_for(
        MADE + 1,
        "Hi B.",
        now.checked_sub(Duration::from_secs(50)).unwrap(),
        Some(&b),
    );
    assert_eq!(
        thread.going_on_asked(YES + 1, now, Some((MADE, now - Duration::from_secs(40)))),
        None,
        "B's own exchange, 50 s ago, is not A's say-back"
    );
}

/// The rest of the carry stays with the switch on: a yes another known
/// face is read saying, even a one-word one by the short-line rule, is
/// that face's, never carried as A's; a second face in view, silent,
/// carries nothing; and nobody in view carries nothing.
#[test]
fn another_face_still_refuses_a_skill_yes_with_the_switch() {
    let now = Instant::now();
    let thread = after_a_long_say_back(now);
    let a = crate::remember::person_id("A");
    let engine = engine();
    let in_line = |ms: u64| ms >= 1600;
    let read = |eyes: &SpeakingFaces, begun, finishing| {
        let speakers = skill_yes_speakers(
            eyes,
            eyes.speakers_with_boot(begun, finishing),
            true,
            begun,
            finishing,
        );
        let carried = unseen_carry(
            &engine.shared,
            eyes,
            &speakers,
            Some("b/f1"),
            (begun, finishing),
            &a,
            || going_on_for_the_yes(&thread, true, now, NOW_US),
        );
        (speakers, carried)
    };
    // Another known face says the short yes, A silent beside it.
    let (eyes, begun, finishing) =
        yes_seen(&|ms| vec![("f1", No), ("f2", if in_line(ms) { Yes } else { No })]);
    let (speakers, carried) = read(&eyes, begun, finishing);
    assert_eq!(
        speakers,
        ["b/f2"],
        "the other face's, by the short-line rule"
    );
    assert_eq!(carried, None);
    assert!(crate::speaking_faces::not_theirs(
        &speakers,
        &a,
        Some(("A".to_owned(), "b/f1".to_owned())),
        true
    ));
    // A second face in view, silent: nobody else may be shown.
    let (eyes, begun, finishing) = yes_seen(&|_| vec![("f1", No), ("f2", No)]);
    assert_eq!(read(&eyes, begun, finishing), (Vec::new(), None));
    // Nobody in view.
    let (eyes, begun, finishing) = yes_seen(&|_| Vec::new());
    assert_eq!(read(&eyes, begun, finishing), (Vec::new(), None));
    crate::skill_write_yes::clear_for_test();
}

/// #7422's short-line rule, the same read: A's one-word yes, their mouth
/// read speaking in the two looks it spans, is nobody's by the three looks
/// a line needs; with a skill's yes awaited and the switch on, it is A's
/// face's, so the yes is theirs, seen, and not a carry. Not awaited, or
/// off, nobody's as before.
#[test]
fn a_short_skill_yes_is_read_by_the_short_line_rule() {
    let in_line = |ms: u64| ms >= 1600;
    let (eyes, begun, finishing) = yes_seen(&|ms| vec![("f1", if in_line(ms) { Yes } else { No })]);
    let plain = eyes.speakers_with_boot(begun, finishing);
    assert!(plain.is_empty(), "two looks: nobody's");
    assert!(skill_yes_speakers(&eyes, plain.clone(), false, begun, finishing).is_empty());
    let read = skill_yes_speakers(&eyes, plain, true, begun, finishing);
    assert_eq!(read, ["b/f1"]);
    assert_eq!(read, eyes.short_line_speakers_with_boot(begun, finishing));
    assert!(!crate::speaking_faces::not_theirs(
        &read,
        &crate::remember::person_id("A"),
        Some(("A".to_owned(), "b/f1".to_owned())),
        true
    ));
}
