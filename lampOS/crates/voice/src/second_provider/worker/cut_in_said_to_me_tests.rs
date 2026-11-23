//! #8205 (lamp-52e6, core row w3, builds 15259 to 15338, 21 Nov): while
//! the lamp told a story it was asked for by name, a "Hey Lamp, ..." line
//! cut in over it. When the lamp's name was not heard in it, it was held:
//! `not said to the lamp, held: no look in a conversation: 8 words, no
//! name`, then `its answer is held back: nobody spoke to the lamp`
//! (requests 3 and 7 of w3 x3 on 15338, request 17 of gate 1135), with
//! nobody in the eyes' view ("pitch unread, looking unread, 0 looks"). The
//! line had cut the lamp's own answer: `request 2: 9 of its answer's 99
//! words were played, 3720 ms of it, before it was cut; request 3 is
//! asked with only those`. Played as the product runs it there:
//! `LAMP_SAID_TO_ME` and `LAMP_NAME_SIGN_TIED` on, the eyes on with no face
//! in view.
use super::Signs;
use crate::second_provider::Heard;
use std::sync::Arc;

/// The story the lamp is asked for by name, then each later line's words,
/// as the speech to text wrote them without the name.
const STORY: &str = "Hey Lamp, tell me a story about a dragon.";
const CUT_IN: &str = "wait, what was the dragon's name again?";

/// The verdict of the story's request, then of one line per `over`, each
/// begun over the lamp's own answer (`over_the_lamps_voice`, which cuts
/// it) or not, in the conversation's window, with no name heard, no face
/// turned and no look. With `news`, the lamp's own news began 3 s before
/// the first such line, with `LAMP_NEWS_REPLY_HEEDS_HELD` on or off
/// (#8383).
fn scene(over: &[bool], news: Option<bool>) -> Vec<Option<&'static str>> {
    let _one = crate::provider_worker::tests::one_thread();
    *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
    *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
    let mut heeding = super::Heeding::new(true, true, None, None);
    heeding.as_the_only_voice(false, true, false);
    heeding.attention.ask_a_second_sign_of_the_name(true);
    heeding.attention.tie_the_second_sign(true);
    if let Some(on) = news {
        heeding.attention.speak_when_spoken_to(true);
        heeding.attention.heed_held_in_news_reply(on);
    }
    heeding.seeing_speaking_faces(
        Arc::new(crate::speaking_faces::SpeakingFaces::default()),
        None,
    );
    heeding.hearing_only_said_to_me();
    heeding.needing_the_turned_face_to_speak();
    let base = super::monotonic_us();
    let signs = |going_on: bool, names: Vec<(u64, f32)>| Signs {
        room: crate::room_told::Room::Unknown,
        going_on,
        lamp_asked: false,
        turned: Some(false),
        names,
        near: false,
        continues_from: None,
        soft_lead_us: 0,
    };
    heeding.hear(
        1,
        Heard::Began {
            over_the_lamps_voice: false,
        },
    );
    let mut verdicts = vec![heeding.heed_signs(
        1,
        (base, base + 3_000_000),
        STORY,
        signs(false, vec![(base + 300_000, 0.99)]),
    )];
    for (request, over) in (2u64..).zip(over) {
        heeding.hear(
            request,
            Heard::Began {
                over_the_lamps_voice: *over,
            },
        );
        let began = base + 8_000_000 * (request - 1);
        if news.is_some() && request == 2 {
            heeding.attention.news_told(began - 3_000_000);
        }
        verdicts.push(heeding.heed_signs(
            request,
            (began, began + 2_700_000),
            CUT_IN,
            signs(true, Vec::new()),
        ));
    }
    *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
    verdicts
}

/// A line that cut in over the lamp's own answer, with no name heard and
/// no look, is answered: the person is cutting the lamp short.
#[test]
fn a_cut_in_over_the_lamps_own_answer_with_no_name_and_no_look_is_answered() {
    assert_eq!(scene(&[true], None), [None, None], "w3's cut-in");
}

/// The same words said while the lamp is silent (not over its answer, so
/// cutting nothing: room talk in the window) are held as before.
#[test]
fn the_same_line_said_while_the_lamp_is_silent_is_held_as_before() {
    let verdicts = scene(&[false], None);
    assert_eq!(verdicts[0], None, "the story asked by name");
    assert!(verdicts[1].is_some(), "held: no look in a conversation");
}

/// A second cut-in with no name and no look, after the first was answered
/// so, is held: the exception is used once, until a line with a look or
/// the name is played (as `LAMP_NAME_SIGN_TIED`'s answer to the lamp's
/// waiting question, #8092), so the room's talk over the lamp's answers
/// carries no conversation.
#[test]
fn a_second_cut_in_with_no_look_or_name_is_held() {
    let verdicts = scene(&[true, true], None);
    assert_eq!(&verdicts[..2], [None, None]);
    assert!(verdicts[2].is_some(), "the exception is used once");
}

/// #8383 (lamp-74ff, 23 Nov 08:40:35, request 22): a line `said_to_me`
/// held, said just after the lamp's own news, was answered as the news's
/// reply. With `LAMP_NEWS_REPLY_HEEDS_HELD` it stays held; off, the news
/// lets it out as before.
#[test]
fn a_held_line_after_the_lamps_news_is_no_reply_with_the_switch() {
    let on = scene(&[false], Some(true));
    assert_eq!(on[0], None, "the story asked by name");
    assert!(on[1].is_some(), "held: said_to_me held it, no news reply");
    assert_eq!(
        scene(&[false], Some(false)),
        [None, None],
        "off: the news's reply"
    );
}
