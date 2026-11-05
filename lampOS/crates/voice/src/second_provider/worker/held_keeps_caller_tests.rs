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
    play(held, false, lines)
}

/// [`run`] with `held` and `LAMP_UNTIED_ANSWER_GRACE=on` (#5124).
fn run_grace(lines: &[Line]) -> Vec<bool> {
    play(true, true, lines)
}

fn play(held: bool, grace: bool, lines: &[Line]) -> Vec<bool> {
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
    if grace {
        heeding.granting_answer_grace();
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
        heeding.callers_own_line(2, &seen(false), false).is_some(),
        "the caller's"
    );
    assert_eq!(
        heeding.callers_own_line(2, &seen(true), false),
        None,
        "another voice"
    );
    *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
}

const SISTER: Line = line(
    0,
    2_000,
    "Lamp, remember my sister's name is Lan.",
    "Got it, your sister is Lan.",
);

const COOK: Line = line(
    7_000,
    1_800,
    "What should I cook tonight?",
    "Try a warm noodle soup with ginger.",
);

/// Seventh's 52e6 rerun of #5324 (5 Nov 08:07, sw5008f): `talk_sis-3`,
/// "Nice. And what was my sister's name again?", said right after the
/// lamp's answer to a line sharing no word with it, was held under HELD as
/// untied, and answered with the switch off: the one line the switch cost
/// (HELD 32/37, OFF 34/37). It asks the lamp what it knows of the
/// person's own. The same words asked of a colleague about theirs, or with
/// a colleague's name, stay held.
#[test]
fn held_keeps_a_question_of_the_callers_own() {
    let sis = [
        SISTER,
        COOK,
        line(
            12_000,
            2_400,
            "Nice. And what was my sister's name again?",
            "Your sister is Lan.",
        ),
    ];
    assert_eq!(run(false, &sis), [true; 3], "off: talk_sis-3");
    assert_eq!(run(true, &sis), [true; 3], "held: talk_sis-3");
    for desk in [
        "Nice. And what was your sister's name again?",
        "Sam, what was my sister's name again?",
    ] {
        let lines = [SISTER, COOK, line(12_000, 2_400, desk, "Lan.")];
        assert_eq!(run(true, &lines), [true, true, false], "held: {desk}");
    }
}

const LONDON: Line = line(
    0,
    1_800,
    "Lamp, what time is it in London?",
    "Twenty-seven past ten at night in London, still Saturday.",
);

/// Seventh's 52e6 FAIL of #5240 at 789644e0f (5 Nov 00:07, sw5008f):
/// owner_b-2 and owner_b-3, each begun 2 s after the lamp's answer to the
/// line before, were held under HELD as having no tie to the lamp, and
/// let out with the switch off as the conversation going on. The lamp's
/// name at their start was written as another word, set off (owner_b-2:
/// "its first word, set off, doesn't sound like it: not the name",
/// `LAMP_NAME_NEAR`). The rule reads no one's name in "Ma'am," or "Look,"
/// (`attention::addresses_someone_else_on`), but the untied rule's
/// `desk_talk` read a colleague called by name, which cancels "tell me …"
/// and "Do I …?" as asks of the lamp. Under `held` a set-off word is a
/// name only as the rule reads names; real names stay held by the rule
/// itself, held or not.
#[test]
fn held_keeps_a_callers_line_whose_name_was_written_as_another_word() {
    for name in ["Ma'am", "Look"] {
        let b2: &'static str = Box::leak(
            format!("{name}, I'm going out later. Do I need an umbrella?").into_boxed_str(),
        );
        let b3: &'static str = Box::leak(format!("{name}, tell me a joke.").into_boxed_str());
        let owner_b = [
            LONDON,
            line(6_800, 4_000, b2, "A little drizzle later, so take one."),
            line(13_800, 1_600, b3, "Why did the lamp go to school?"),
        ];
        assert_eq!(run(false, &owner_b), [true; 3], "off: {name}");
        assert_eq!(run(true, &owner_b), [true; 3], "held: {name}");
        // With no word tying it, the same opening stays held under `held`.
        let untied = [
            LONDON,
            line(
                6_800,
                2_400,
                Box::leak(format!("{name}, did you send the slides to Sam yet?").into_boxed_str()),
                "I hope so.",
            ),
        ];
        assert_eq!(run(true, &untied), [true, false], "held, untied: {name}");
    }
    // A set-off name the rule reads as someone's is still desk talk under
    // `held`, as #5008 holds it (off lets it out as the conversation going
    // on); other signs of desk talk stand.
    let to_sam = [LONDON, line(6_800, 2_400, "Sam, tell me a joke.", "Okay.")];
    assert_eq!(run(false, &to_sam), [true, true], "off: Sam");
    assert_eq!(run(true, &to_sam), [true, false], "held: Sam");
    for desk in [
        "Hey Sam, play some music.",
        "Tell Sam the meeting moved to four.",
        "Should I bring an umbrella, you guys?",
    ] {
        let lines = [LONDON, line(6_800, 2_400, desk, "Okay.")];
        assert_eq!(run(true, &lines), [true, false], "held: {desk}");
    }
}

/// #5124 (busy_day, #4175's 3+3 on lamp-c0a4, 3 Nov): the caller talks
/// with the lamp (c01-c03), two colleagues talk at the desk (d11, d12), and
/// the caller goes on (c04-c06). Timed as if each caller line were
/// answered, the kindest case for the rule. Off answers them; does `held`?
#[test]
fn busy_day_callers_lines_after_desk_talk() {
    let day = [
        line(
            0,
            1_800,
            "Lamp, I had a really long day today.",
            "Long ones wear on you. Want to tell me what made it long?",
        ),
        line(
            6_300,
            2_800,
            "Yeah. My boss moved our deadline up by a whole week.",
            "A whole week is a lot to swallow at once.",
        ),
        line(
            14_100,
            3_000,
            "I don't know. I'll probably just work late tomorrow.",
            "That's one way to hold it together.",
        ),
        line(
            21_100,
            2_500,
            "Do you want coffee? I'm going downstairs.",
            "I can't get coffee.",
        ),
        line(24_100, 1_800, "Yes please, a latte, no sugar.", "Noted."),
        line(
            27_900,
            1_600,
            "Maybe. What would you do?",
            "I'd ask which part of the week really moved.",
        ),
        line(
            36_000,
            3_000,
            "That's a good idea. Any tips to sleep better tonight?",
            "Keep the room cool and dark.",
        ),
        line(44_000, 2_200, "Thanks. You're a good listener.", "Anytime."),
    ];
    let off = run(false, &day);
    let held = run(true, &day);
    let grace = run_grace(&day);
    eprintln!("BUSY_DAY off={off:?} held={held:?} grace={grace:?}");
    assert_eq!(off, [true; 8], "off");
    // `held` holds c03, begun 2 s after the lamp's answer and sharing no
    // word with it, and every line after it (#5124's failing-first run).
    assert_eq!(held, [true, true, false, false, false, false, false, false]);
    // The grace keeps c03, and no room line: d11 and d12 stay held.
    assert!(grace[2], "grace: c03 (grace {grace:?})");
    assert_eq!(
        &grace[3..5],
        &[false, false],
        "grace: d11, d12 (grace {grace:?})"
    );
}

/// #5124's pass 2: the grace takes no room line. Every desk line these
/// tests hold under `held` stays held with the grace too, the ones begun
/// within its 3 s among them, and the caller's lines `held` keeps stay kept.
#[test]
fn the_grace_takes_no_room_line() {
    let after_tired = [
        TIRED,
        line(
            8_000,
            2_000,
            "Did you send the slides to Sam yet?",
            "I hope so.",
        ),
    ];
    let after_calm = [
        CALM,
        line(
            7_000,
            1_800,
            "Can you pass me the charger? The white one.",
            "I can't.",
        ),
    ];
    let thanks_sam = [
        CALM,
        line(
            7_000,
            1_800,
            "Thanks Sam, I'll send it after lunch.",
            "Okay.",
        ),
    ];
    let mut sets: Vec<Vec<Line>> = vec![after_tired.into(), after_calm.into(), thanks_sam.into()];
    for (at, desk) in [
        (6_800, "Sam, tell me a joke."),
        (6_800, "Hey Sam, play some music."),
        (6_800, "Tell Sam the meeting moved to four."),
        (6_800, "Should I bring an umbrella, you guys?"),
        (6_800, "Ma'am, did you send the slides to Sam yet?"),
        (6_800, "Look, did you send the slides to Sam yet?"),
        (5_000, "Did you send the slides to Sam yet?"),
        (5_000, "Are you coming to the meeting at four?"),
        (5_000, "Can you pass me the charger? The white one."),
        (5_000, "Do you want coffee? I'm going downstairs."),
    ] {
        sets.push(vec![LONDON, line(at, 2_400, desk, "Okay.")]);
    }
    let mut room = (0, 0);
    for lines in &sets {
        let (held, grace) = (run(true, lines), run_grace(lines));
        for (index, (held, grace)) in held.iter().zip(&grace).enumerate() {
            if !held {
                room.0 += 1;
                room.1 += usize::from(*grace);
            }
            assert!(
                held == grace || !grace,
                "grace took a line held: {:?} (held {held}, grace {grace})",
                lines[index].heard
            );
        }
    }
    eprintln!("ROOM_LINES held={} taken_by_grace={}", room.0, room.1);
    assert_eq!(room.1, 0);
}

/// The grace keeps one line an answer: a second line before the lamp
/// answers the first is judged as without it, and a line begun over the
/// answer, or past 3 s after it, is not in it.
#[test]
fn the_grace_is_one_line_an_answer_and_three_seconds() {
    let quiet = "I don't know. I'll probably just work late tomorrow.";
    // 2 s after the answer: kept; 4 s after: held.
    let in_grace = [LONDON, line(6_800, 2_400, quiet, "Okay.")];
    assert_eq!(run(true, &in_grace), [true, false], "held");
    assert_eq!(run_grace(&in_grace), [true, true], "grace");
    let late = [LONDON, line(8_800, 2_400, quiet, "Okay.")];
    assert_eq!(run_grace(&late), [true, false], "past the grace");
    let over = [
        LONDON,
        Line {
            over: true,
            ..line(4_000, 2_400, quiet, "Okay.")
        },
    ];
    assert_eq!(run_grace(&over), [true, false], "over the answer");
}
