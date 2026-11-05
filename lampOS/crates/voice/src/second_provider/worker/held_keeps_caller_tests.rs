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
/// what was heard, the lamp's answer if let out, whether it began over
/// the lamp's answer, and the faces the eyes saw through it, each with
/// whether it read speaking (none: no look at all, as with the eyes off).
#[derive(Clone, Copy)]
struct Line {
    at: u64,
    long: u64,
    heard: &'static str,
    answer: &'static str,
    over: bool,
    faces: &'static [(&'static str, bool)],
}

const fn line(at: u64, long: u64, heard: &'static str, answer: &'static str) -> Line {
    Line {
        at,
        long,
        heard,
        answer,
        over: false,
        faces: &[],
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
    play_seen(
        Switches {
            held,
            grace,
            ..Switches::default()
        },
        lines,
    )
}

/// The switches a run plays with, and whether the eyes' speaking faces are
/// read (each line's [`Line::faces`]).
#[derive(Clone, Copy, Default)]
struct Switches {
    held: bool,
    grace: bool,
    /// `LAMP_UNTIED_CALLER_SPEAKING=on` (#5124's lever (a)).
    speaking: bool,
    eyes: bool,
}

/// Off, `held`, `held` with the grace, with the caller seen speaking, and
/// with both, all with the eyes read.
const OFF: Switches = Switches {
    held: false,
    grace: false,
    speaking: false,
    eyes: true,
};
const HELD: Switches = Switches { held: true, ..OFF };
const GRACE: Switches = Switches {
    grace: true,
    ..HELD
};
const SPEAKING: Switches = Switches {
    speaking: true,
    ..HELD
};
const BOTH: Switches = Switches {
    grace: true,
    ..SPEAKING
};

/// One frame of the eyes: each face, speaking (`Yes`) or still (`No`),
/// turned to the lamp.
fn frame(faces: &[(&str, bool)]) -> lamp_msgs::Faces {
    use lamp_msgs::{Face, Speaking, SpeakingState};
    lamp_msgs::Faces {
        faces: faces
            .iter()
            .enumerate()
            .map(|(index, (id, speaking))| Face {
                id: (*id).to_owned(),
                xmin: 0.1 + 0.4 * index as f32,
                ymin: 0.2,
                xmax: 0.3 + 0.4 * index as f32,
                ymax: 0.5,
                confidence: 1.0,
                yaw: None,
                facing: Some(true),
                speaking: Some(Speaking {
                    state: if *speaking {
                        SpeakingState::Yes
                    } else {
                        SpeakingState::No
                    },
                    score: 0.9,
                    from: None,
                }),
                follows: None,
            })
            .collect(),
    }
}

fn play_seen(switches: Switches, lines: &[Line]) -> Vec<bool> {
    let Switches {
        held,
        grace,
        speaking,
        eyes: read_eyes,
    } = switches;
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
    if speaking {
        heeding.keeping_the_callers_speaking_face();
    }
    let eyes = Arc::new(crate::speaking_faces::SpeakingFaces::default());
    if read_eyes {
        heeding.seeing_speaking_faces(eyes.clone(), None);
    }
    let mut seq = 0;
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
        // Four looks through the line, by the clock the rule reads them
        // with (`speakers_between`); none for a line with no faces.
        if !line.faces.is_empty() {
            let (now, now_us) = (Instant::now(), monotonic_us());
            let seen = frame(line.faces);
            for look in 0..4u64 {
                seq += 1;
                let at_us = said.0 + (said.1 - said.0) * (2 * look + 1) / 8;
                eyes.look(
                    now - Duration::from_micros(now_us - at_us),
                    Some((("b", seq), &seen)),
                );
            }
        }
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

/// #4175's busy_day on lamp-c0a4 (3 Nov): c01-c03, d11, d12, c04-c06.
const BUSY_DAY: [Line; 8] = [
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

/// #5124 (busy_day, #4175's 3+3 on lamp-c0a4, 3 Nov): the caller talks
/// with the lamp (c01-c03), two colleagues talk at the desk (d11, d12), and
/// the caller goes on (c04-c06). Timed as if each caller line were
/// answered, the kindest case for the rule. Off answers them; does `held`?
#[test]
fn busy_day_callers_lines_after_desk_talk() {
    let day = BUSY_DAY;
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

/// The room lines `the_grace_takes_no_room_line` plays, each set's last
/// line a desk line after the caller's.
fn grace_room_sets() -> Vec<Vec<Line>> {
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
    sets
}

/// #5124's pass 2: the grace takes no room line. Every desk line these
/// tests hold under `held` stays held with the grace too, the ones begun
/// within its 3 s among them, and the caller's lines `held` keeps stay kept.
#[test]
fn the_grace_takes_no_room_line() {
    let sets = grace_room_sets();
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

/// The grace keeps no line the eyes saw another face than the caller's
/// speaking, and no question, a full-width '？' too (the reviewer-lead's P2
/// and P3 on #5423). The call is seen spoken by the caller's face, f1; a
/// statement begun 2 s after the answer is kept with no face seen, and held
/// with a colleague's face, or the caller's still and a colleague's
/// speaking.
#[test]
fn the_grace_keeps_no_line_another_face_speaks_and_no_question() {
    let quiet = "I don't know. I'll probably just work late tomorrow.";
    let call = Line {
        faces: CALLER_SPEAKS,
        ..LONDON
    };
    let after = |heard, faces| {
        [
            call,
            Line {
                faces,
                ..line(6_800, 2_400, heard, "Okay.")
            },
        ]
    };
    let grace = |lines: &[Line]| play_seen(GRACE, lines);
    assert_eq!(grace(&after(quiet, NO_FACE)), [true, true], "no face seen");
    assert_eq!(
        grace(&after(quiet, B_SPEAKS)),
        [true, false],
        "another face"
    );
    assert_eq!(
        grace(&after(quiet, CALLER_STILL_B_SPEAKS)),
        [true, false],
        "the caller's face still, another speaking"
    );
    for question in [
        "Will you finish the report tonight?",
        "Will you finish the report tonight？",
    ] {
        assert_eq!(
            grace(&after(question, NO_FACE)),
            [true, false],
            "a question: {question}"
        );
    }
}

/// No look through a line: no reading, or only stale ones from before it.
const NO_FACE: &[(&str, bool)] = &[];
/// The caller's face, f1, read speaking through a line.
const CALLER_SPEAKS: &[(&str, bool)] = &[("f1", true)];
/// A colleague's face read speaking, and no other face in view.
const B_SPEAKS: &[(&str, bool)] = &[("f2", true)];
const C_SPEAKS: &[(&str, bool)] = &[("f3", true)];
/// The caller's face in view and turned to the lamp, read still, while a
/// colleague's face speaks.
const CALLER_STILL_B_SPEAKS: &[(&str, bool)] = &[("f1", false), ("f2", true)];
/// The caller's face in view and turned to the lamp, read still, and no
/// face speaking: a voice from off the picture.
const CALLER_STILL: &[(&str, bool)] = &[("f1", false)];

/// `lines` with the eyes' faces: the caller's seen speaking each line
/// but those at `desk`, which show `desk_faces` (none: no look through
/// them, as with no reading or only stale ones from earlier lines).
fn seen(lines: &[Line], desk: &[usize], desk_faces: &'static [(&'static str, bool)]) -> Vec<Line> {
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| Line {
            faces: if desk.contains(&index) {
                desk_faces
            } else {
                CALLER_SPEAKS
            },
            ..*line
        })
        .collect()
}

/// #5124's lever (a), failing first: busy_day with the eyes, the caller's
/// face seen speaking c01-c06 and the colleagues' faces d11 and d12. With
/// `held` and the grace, c04-c06 after the desk talk are still held; with
/// `LAMP_UNTIED_CALLER_SPEAKING` too, all eight are as they should be:
/// every caller line answered, both desk lines held.
#[test]
fn busy_day_the_callers_face_seen_speaking_keeps_the_tie() {
    let mut day = seen(&BUSY_DAY, &[3], B_SPEAKS);
    day[4].faces = C_SPEAKS;
    let [off, held, grace, speaking, both] =
        [OFF, HELD, GRACE, SPEAKING, BOTH].map(|switches| play_seen(switches, &day));
    let right = [true, true, true, false, false, true, true, true];
    let count = |out: &[bool]| out.iter().zip(&right).filter(|(a, b)| a == b).count();
    eprintln!(
        "BUSY_DAY_SEEN off={off:?} held={held:?} grace={grace:?} speaking={speaking:?} both={both:?} right: off {} held {} grace {} speaking {} both {} of 8",
        count(&off),
        count(&held),
        count(&grace),
        count(&speaking),
        count(&both)
    );
    assert_eq!(off, [true; 8], "off answers every line");
    assert_eq!(both, right, "held, the grace and the caller seen speaking");
}

/// #5124's pass 2 for lever (a): no room line is taken. Every desk line of
/// busy_day and of #5110's quiet-room sets (the grace's, `held_still_holds_
/// desk_talk`'s desk1, desk talk before a reply or a thank-you, a question
/// of a colleague's own, a set-off word before desk talk), with the eyes
/// showing only the desk speaker's face speaking, no face read, the
/// caller's face in view but still while another face speaks, or the
/// caller's face still and no face speaking: each held under `held` stays
/// held with both levers, and the caller's lines `held` keeps stay kept.
#[test]
fn the_callers_face_speaking_takes_no_room_line() {
    let busy_day: Vec<(Vec<Line>, Vec<usize>)> = vec![(BUSY_DAY.into(), vec![3, 4])];
    let mut quiet: Vec<(Vec<Line>, Vec<usize>)> = grace_room_sets()
        .into_iter()
        .map(|lines| {
            let desk = (1..lines.len()).collect();
            (lines, desk)
        })
        .collect();
    let desk1 = [
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
    quiet.push((desk1.into(), (1..6).collect()));
    quiet.push((
        vec![
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
        ],
        vec![1, 2],
    ));
    quiet.push((
        vec![
            CALM,
            line(
                7_000,
                1_800,
                "Can you pass me the charger? The white one.",
                "I can't pass things.",
            ),
            line(9_500, 1_500, "Thanks, you're a star.", "Any time."),
        ],
        vec![1, 2],
    ));
    for desk in [
        "Nice. And what was your sister's name again?",
        "Sam, what was my sister's name again?",
    ] {
        quiet.push((
            vec![SISTER, COOK, line(12_000, 2_400, desk, "Lan.")],
            vec![2],
        ));
    }
    for desk in [
        "Ma'am, did you send the slides to Sam yet?",
        "Look, did you send the slides to Sam yet?",
    ] {
        quiet.push((
            vec![LONDON, line(6_800, 2_400, desk, "I hope so.")],
            vec![1],
        ));
    }
    for (name, sets) in [("busy_day", &busy_day), ("quiet_room", &quiet)] {
        for (reading, desk_faces) in [
            ("desk_face", B_SPEAKS),
            ("no_face", NO_FACE),
            ("caller_still_desk_face", CALLER_STILL_B_SPEAKS),
            ("caller_still", CALLER_STILL),
        ] {
            let (mut room, mut taken) = (0, 0);
            for (lines, desk) in sets {
                let lines = seen(lines, desk, desk_faces);
                let (held, both) = (play_seen(HELD, &lines), play_seen(BOTH, &lines));
                for (index, (held, both)) in held.iter().zip(&both).enumerate() {
                    if desk.contains(&index) {
                        assert!(!held, "held let out a desk line: {:?}", lines[index].heard);
                        room += 1;
                        taken += usize::from(*both);
                    } else if *held {
                        assert!(*both, "a caller's line lost: {:?}", lines[index].heard);
                    }
                }
                assert!(
                    desk.iter().all(|index| !both[*index]),
                    "{name} {reading}: a room line taken in {:?}",
                    lines.iter().map(|line| line.heard).collect::<Vec<_>>()
                );
            }
            eprintln!("ROOM_LINES_SEEN {name} {reading}: held={room} taken={taken}");
            assert_eq!(taken, 0);
        }
    }
}

/// The guards `callers_own_line` reads before the caller's face: a sure
/// other voice, desk talk and a reply naming someone are not the caller's
/// with their face seen speaking; nor a face in view but still, two faces
/// speaking, or the switch off. A question seen spoken by the caller's
/// face is theirs.
#[test]
fn the_callers_face_speaking_yields_to_the_guards() {
    let _one = crate::provider_worker::tests::one_thread();
    // No answer in the thread, so nothing but the face can keep a line.
    *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
    let mut heeding = Heeding::new(true, true, None, None);
    heeding.holding_untied();
    heeding.caller_faces = vec!["b/f1".to_owned()];
    heeding.caller_faces_from_call = true;
    let seen = |heard: &str, speakers: &[&str], other_voice| Seen {
        room: crate::room_told::Room::Quiet,
        going_on: true,
        turned: None,
        busy: false,
        lamp_asked: false,
        speakers: speakers.iter().map(|face| (*face).to_owned()).collect(),
        turned_speakers: Vec::new(),
        heard: heard.to_owned(),
        named_faced: false,
        other_voice,
        same_voice: false,
        likeness: None,
    };
    let kept = Some(crate::untied_going_on::CALLER_SPEAKING_KEPT);
    let question = "Maybe. What would you do?";
    assert_eq!(
        heeding.callers_own_line(9, &seen(question, &["b/f1"], false), false),
        None,
        "the switch off"
    );
    heeding.keeping_the_callers_speaking_face();
    assert_eq!(
        heeding.callers_own_line(9, &seen(question, &["b/f1"], false), false),
        kept,
        "a question seen spoken by the caller's face"
    );
    for (heard, speakers, other_voice, why) in [
        (question, &["b/f1"][..], true, "a sure other voice"),
        (
            "Should I bring an umbrella, you guys?",
            &["b/f1"][..],
            false,
            "desk talk",
        ),
        (
            "Thanks Sam, I'll send it after lunch.",
            &["b/f1"][..],
            false,
            "a reply naming someone",
        ),
        (question, &[][..], false, "no face seen speaking"),
        (question, &["b/f2"][..], false, "another face"),
        (question, &["b/f1", "b/f2"][..], false, "two faces"),
    ] {
        assert_eq!(
            heeding.callers_own_line(9, &seen(heard, speakers, other_voice), false),
            None,
            "{why}"
        );
    }
}

/// The Reviewer-lead's P2 on #5440: the lever trusts only a face seen
/// speaking the call itself. A call with no face seen speaking it; a
/// colleague, f2, turns to the lamp and says "I'll be back after lunch."
/// 2 s after the answer (the grace may keep it, and the fallback then takes
/// f2 for the caller's); f2's next questions to the desk stay held. The
/// control: with the call seen spoken by f1, f1's line after desk talk is
/// still kept.
#[test]
fn the_callers_face_speaking_trusts_only_a_face_seen_speaking_the_call() {
    let colleague = |at, heard, answer| Line {
        faces: B_SPEAKS,
        ..line(at, 2_000, heard, answer)
    };
    let probe = [
        LONDON,
        colleague(6_800, "I'll be back after lunch.", "See you then."),
        colleague(
            12_000,
            "Did you finish the report for the client?",
            "Not yet.",
        ),
        colleague(16_000, "Can you send it to me before four?", "Sure."),
    ];
    let out = play_seen(BOTH, &probe);
    eprintln!("FALLBACK_FACE_PROBE both={out:?}");
    assert!(out[0], "the call is answered");
    assert_eq!(
        out[2..],
        [false, false],
        "the colleague's desk questions stay held"
    );
    let control = [
        Line {
            faces: CALLER_SPEAKS,
            ..LONDON
        },
        colleague(7_000, "Are you coming to the meeting at four?", "No."),
        Line {
            faces: CALLER_SPEAKS,
            ..line(12_000, 1_800, "Maybe. What would you do?", "I'd go.")
        },
    ];
    let (grace, both) = (play_seen(GRACE, &control), play_seen(BOTH, &control));
    eprintln!("FALLBACK_FACE_CONTROL grace={grace:?} both={both:?}");
    assert_eq!(grace, [true, false, false], "without the lever");
    assert_eq!(
        both,
        [true, false, true],
        "the caller seen speaking the call"
    );
}
