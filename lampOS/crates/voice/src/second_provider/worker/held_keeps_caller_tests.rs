//! #5110: with `LAMP_UNTIED_GOING_ON=held`, the caller's own lines to the
//! lamp are answered again, and desk talk is still held (#5008).
//!
//! Seventh's quiet-room owner count on lamp-52e6 (stable 9788, 3 Nov 18:43,
//! #5085): OFF answered 37/37, HELD 25/28, and all three of HELD's misses
//! were the untied rule's:
//! - `owner_b-2`: "Lamp, I'm going out later. {1.0} Do I need an
//!   umbrella?", one request whose name was not read as a call;
//! - `talk_day-5`: "That's a good idea. Any tips to sleep better tonight?",
//!   a follow-up sharing no word with the lamp's last answer;
//! - `talk_day-6`: "Thanks. You're a good listener.", a thank-you.
//!
//! Each case is played off, then with `held`: off answers every line, and
//! `held` must too, while the desk lines beside them stay held.
use super::*;

/// One line: when it began after the first (ms), how long it was (ms),
/// what was heard, the lamp's answer if let out, and whether it began over
/// the lamp's answer.
struct Line {
    at: u64,
    long: u64,
    heard: &'static str,
    answer: &'static str,
    over: bool,
}

const fn line(at: u64, long: u64, heard: &'static str, answer: &'static str) -> Line {
    Line {
        at,
        long,
        heard,
        answer,
        over: false,
    }
}

/// How long the lamp's answer to a let-out line plays, from the line's end.
const ANSWER_MS: u64 = 3_000;

/// Plays `lines` through the rule, with `LAMP_UNTIED_GOING_ON=held` when
/// `held`, else with the switch off: whether each line was let out. As
/// `untied_talk_after_an_answer_keeps_no_conversation_going` plays them,
/// in milliseconds.
fn run(held: bool, lines: &[Line]) -> Vec<bool> {
    let _one = crate::provider_worker::tests::one_thread();
    let _clock =
        crate::scenario_clock::Scope::enter_at(Duration::from_secs(10), 600_000_000).unwrap();
    let ms = Duration::from_millis;
    *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
    *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
    crate::room_told::room()
        .lock()
        .unwrap()
        .count_only_talk(true);
    let mut heeding = Heeding::new(true, true, None, None);
    heeding.as_the_only_voice(false, true, false);
    if held {
        heeding.holding_untied();
    }
    let mut answered: Vec<(u64, &str, &str, u64)> = Vec::new();
    let mut was = 0;
    let mut out = Vec::new();
    for (request, line) in (1..).zip(lines) {
        let passed = ms(line.at - was);
        was = line.at;
        let older =
            |until: Option<u64>| until.map(|until| until.saturating_sub(passed.as_micros() as u64));
        heeding.said_until_us = older(heeding.said_until_us);
        heeding.said_until_free_us = older(heeding.said_until_free_us);
        let said = (
            10_000_000 + line.at * 1_000,
            10_000_000 + (line.at + line.long) * 1_000,
        );
        let now = Instant::now();
        let mut thread = crate::thread::Thread::new();
        for (asked, person, lamp, at) in &answered {
            let then = now - ms(line.at - at);
            thread.person(person, true, then);
            thread.lamp(*asked, lamp, then);
        }
        *crate::thread::THREAD.lock().unwrap() = thread;
        heeding.called_at = heeding.called_at.map(|at| at - passed);
        heeding.hear(
            request,
            crate::second_provider::Heard::Began {
                over_the_lamps_voice: line.over,
            },
        );
        let held_why = heeding.heed(request, said, line.heard, None);
        if held_why.is_none() {
            let ended = line.at + line.long;
            answered.push((request, line.heard, line.answer, ended));
            let end_us =
                (monotonic_us() + (ended + ANSWER_MS) * 1_000).saturating_sub(line.at * 1_000);
            heeding.lamp_did(crate::second_provider::Did::Said(request, end_us));
            crate::room_told::room()
                .lock()
                .unwrap()
                .spoke(request, monotonic_us());
        }
        out.push(held_why.is_none());
    }
    *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
    out
}

const TIRED: Line = line(
    0,
    1_800,
    "Lamp, I've been so tired lately.",
    "Sorry to hear that. Maybe a warm drink and no screens before bed.",
);

/// `talk_day-5`: a follow-up in the conversation that shares no word with
/// the lamp's last answer, said right after it: it opens by taking up what
/// the lamp said ("That's a good idea.") and asks its advice ("Any
/// tips …?").
#[test]
fn held_keeps_a_follow_up_sharing_no_word_with_the_answer() {
    for follow_up in [
        "That's a good idea. Any tips to sleep better tonight?",
        "That's a good idea, I'll try it tonight.",
        "Any tips to sleep better tonight?",
        "Sounds good. How can I stop waking up at night?",
    ] {
        let lines = [
            TIRED,
            line(8_000, 2_400, follow_up, "Keep the room cool and dark."),
        ];
        assert_eq!(run(false, &lines), [true; 2], "off: {follow_up}");
        assert_eq!(run(true, &lines), [true; 2], "held: {follow_up}");
    }
    // After a desk line the rule held, the lamp's answer is no longer the
    // last thing said: the same words may answer the desk line, and stay
    // held.
    let after_desk = [
        TIRED,
        line(
            8_000,
            2_000,
            "Did you send the slides to Sam yet?",
            "I hope the slides went well.",
        ),
        line(
            11_000,
            1_200,
            "That's a good idea, I'll try it tonight.",
            "Glad you think so.",
        ),
    ];
    assert_eq!(
        run(true, &after_desk),
        [true, false, false],
        "held: a reply after desk talk"
    );
}

const CALM: Line = line(
    0,
    1_800,
    "Lamp, tell me something calming.",
    "Picture a slow tide coming in and going out.",
);

/// `talk_day-6`: a thank-you to the lamp right after its answer.
#[test]
fn held_keeps_a_thank_you_to_the_lamp() {
    for thanks in [
        "Thanks. You're a good listener.",
        "Thank you, that was nice.",
        "Okay, thanks a lot.",
    ] {
        let lines = [CALM, line(7_000, 1_800, thanks, "Any time.")];
        assert_eq!(run(false, &lines), [true; 2], "off: {thanks}");
        assert_eq!(run(true, &lines), [true; 2], "held: {thanks}");
    }
    // Thanks to someone by name, or after a desk line, is not the lamp's.
    let to_sam = [
        CALM,
        line(
            7_000,
            1_800,
            "Thanks Sam, I'll send it after lunch.",
            "Okay.",
        ),
    ];
    assert_eq!(run(true, &to_sam), [true, false], "held: thanks to Sam");
    let after_desk = [
        CALM,
        line(
            7_000,
            1_800,
            "Can you pass me the charger? The white one.",
            "I can't pass things.",
        ),
        line(9_500, 1_500, "Thanks, you're a star.", "Any time."),
    ];
    assert_eq!(
        run(true, &after_desk),
        [true, false, false],
        "held: thanks after desk talk"
    );
}

/// #5008's desk lines stay held with `held`: desk1 after a question to the
/// lamp, each line a person's to another, none a reply to the lamp's
/// answer or an ask of its advice.
#[test]
fn held_still_holds_desk_talk() {
    let desk = [
        line(
            0,
            1_800,
            "Lamp, what's the weather in Saigon?",
            "It's sunny in Saigon, about thirty-three degrees.",
        ),
        Line {
            over: true,
            ..line(
                6_000,
                2_000,
                "Did you send the slides to Sam yet?",
                "I hope so.",
            )
        },
        line(
            9_000,
            1_800,
            "Not yet, I'll do it after lunch.",
            "Enjoy lunch.",
        ),
        line(
            12_000,
            2_000,
            "Are you coming to the meeting at four?",
            "No meeting for me.",
        ),
        line(
            15_000,
            2_000,
            "I think so, but I might be a bit late.",
            "No rush.",
        ),
        line(
            18_000,
            2_000,
            "Can you pass me the charger? The white one.",
            "I can't.",
        ),
    ];
    assert_eq!(run(false, &desk), [true; 6], "off answers desk talk");
    assert_eq!(
        run(true, &desk),
        [true, false, false, false, false, false],
        "held: no desk line reaches the model"
    );
}

/// Twelve's P3 on #5240: a sure other voice than the conversation's gets no
/// caller's tie, whatever it says right after the lamp's answer: a
/// colleague's "Thanks." to the caller is not the caller's.
#[test]
fn a_sure_other_voice_gets_no_callers_tie() {
    let _one = crate::provider_worker::tests::one_thread();
    let _clock =
        crate::scenario_clock::Scope::enter_at(Duration::from_secs(10), 600_000_000).unwrap();
    let mut thread = crate::thread::Thread::new();
    let then = Instant::now() - Duration::from_secs(3);
    thread.person(CALM.heard, true, then);
    thread.lamp(1, CALM.answer, then);
    *crate::thread::THREAD.lock().unwrap() = thread;
    let mut heeding = Heeding::new(true, true, None, None);
    heeding.holding_untied();
    heeding
        .said_lately
        .push_back((1, (10_000_000, 11_800_000), true, 10_000_000));
    let seen = |other_voice| Seen {
        room: crate::room_told::Room::Quiet,
        going_on: true,
        turned: None,
        busy: false,
        lamp_asked: false,
        speakers: Vec::new(),
        turned_speakers: Vec::new(),
        heard: "Thanks. You're a good listener.".to_owned(),
        named_faced: false,
        other_voice,
        same_voice: false,
        likeness: None,
    };
    assert!(
        heeding.callers_own_line(2, &seen(false)).is_some(),
        "the caller's"
    );
    assert_eq!(
        heeding.callers_own_line(2, &seen(true)),
        None,
        "another voice"
    );
    *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
}
