//! `LAMP_SHORT_DONE` on the second route (#2877): a turn whose every call
//! is a light or volume change lamp-actions answered short is said with the
//! calls' own line when the round after them adds nothing; a request that
//! held more (the Reviewer-lead's P2 on #2930) gets its full answer.
use super::*;

/// What lamp-actions tells the voice of each call here, as its socket
/// answers with the switch on (`lamp_actions::Outcome::line_for`): a done
/// change with `"next"` and `"say"`, a refusal, an "already", and anyone
/// else's call.
fn told(name: &str) -> Value {
    let short = |result: &str, say: &str| json!({"result": result, "next": crate::second_provider::probe::tools::SHORT_NEXT, "say": say});
    match name {
        "light_brighter" => short("My light is a step brighter.", "Brighter."),
        "light_dimmer" => short("My light is a step dimmer.", "Dimmer."),
        "volume_down" => short("My volume is now 50 out of 100; it was 70.", "Quieter."),
        "light_warmer" => {
            json!({"error": "I asked for it but could not confirm my light changed."})
        }
        "light_colour" => json!({"result": "My light is already orange."}),
        // An "already" from today's lamp-actions: marked, with its own words
        // (the first builder on #4710, 17:35).
        "light_cooler" => {
            json!({"result": "My light is already as cool as it goes.", "already": true, "next": "Nothing changed: it already was so.", "say": "I'm already as cool as I go."})
        }
        // lamp-actions' schedule, committed (`schedule.rs`'s answer).
        "set_timer" => {
            json!({"status": "committed", "id": 1, "due_unix_ms": 1_792_454_700_000_u64, "local": "2026-10-31T07:52:00+07:00", "clock": "07:52"})
        }
        _ => json!({"ok": true}),
    }
}

/// One turn as it went.
#[derive(Debug, PartialEq)]
struct Went {
    /// What was said, as `said:` labels.
    said: Vec<String>,
    /// The chat rounds asked.
    asked: usize,
    /// The calls run.
    ran: usize,
    /// Whether a round was told the change is answered (`short_done_note`,
    /// or `said_at_once_note` once it was said at once).
    noted: bool,
}

/// One turn: `heard` asked with round 1's `first` events and the rounds
/// `later`, the switch `on`.
fn short_turn(
    on: bool,
    heard: &str,
    first: Vec<net::Result<ChatEvent>>,
    later: Vec<Vec<net::Result<ChatEvent>>>,
) -> Went {
    turn_with(on, false, heard, first, later)
}

/// [`short_turn`], with `LAMP_SHORT_DONE` as `short` and
/// `LAMP_SECOND_SAY_WITH_CALL`'s answer at once as `at_once`.
fn turn_with(
    short: bool,
    at_once: bool,
    heard: &str,
    first: Vec<net::Result<ChatEvent>>,
    later: Vec<Vec<net::Result<ChatEvent>>>,
) -> Went {
    turn_noted(short, at_once, heard, first, later).0
}

/// [`turn_with`], and every note the rounds were asked with.
fn turn_noted(
    short: bool,
    at_once: bool,
    heard: &str,
    first: Vec<net::Result<ChatEvent>>,
    later: Vec<Vec<net::Result<ChatEvent>>>,
) -> (Went, Vec<String>) {
    turn_noted_with(short, at_once, heard, first, later, told)
}

/// [`turn_noted`], with the tool's result supplied by the case.
fn turn_noted_with(
    short: bool,
    at_once: bool,
    heard: &str,
    first: Vec<net::Result<ChatEvent>>,
    later: Vec<Vec<net::Result<ChatEvent>>>,
    answer_for: fn(&str) -> Value,
) -> (Went, Vec<String>) {
    let (speech_ready, mut speech) = mpsc::unbounded_channel();
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new(std::iter::once(first).chain(later).collect()),
        speech_ready: Some(speech_ready),
        ..Script::default()
    });
    let ran: Calls = Arc::default();
    let seen = ran.clone();
    let tools: ToolRunner = Arc::new(move |_, heard, name, args| {
        let answer = answer_for(&name);
        seen.lock().unwrap().push((name, args, heard));
        Box::pin(async move { answer })
    });
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(Instant::now),
    )
    .announcing(false)
    .answering_short(short)
    .saying_at_once(at_once);
    let labels = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            engine.submit(start(1));
            engine.submit(audio(1, 1));
            engine.submit(end(1));
            turn(&mut engine).await
        });
    let said: Vec<String> = labels
        .into_iter()
        .filter(|label| label.starts_with("said:"))
        .collect();
    // Every TTS result has exactly one transcript label. A consumed marker
    // must reach neither, even when there are already words in this turn.
    let mut voiced = Vec::new();
    while let Ok(line) = speech.try_recv() {
        voiced.push(format!("said:{line}"));
    }
    voiced.sort();
    let mut labelled: Vec<String> = said
        .iter()
        .map(|line| format!("said:{}", line.trim_start_matches("said:").trim()))
        .collect();
    labelled.sort();
    assert_eq!(voiced, labelled);
    let asked = script.asked.lock().unwrap();
    let notes: Vec<String> = asked
        .iter()
        .flatten()
        .filter_map(|message| {
            message
                .content
                .as_ref()
                .and_then(|content| content.as_str())
        })
        .filter(|content| content.starts_with("(For you to know"))
        .map(str::to_owned)
        .collect();
    let noted = notes.iter().any(|note| {
        note.contains("the lamp itself says") || note.contains("the lamp itself already said")
    });
    let went = Went {
        said,
        asked: asked.len(),
        ran: ran.lock().unwrap().len(),
        noted,
    };
    (went, notes)
}

/// "Make it brighter." with round 2's words as today's.
fn brighter(on: bool, first: Vec<net::Result<ChatEvent>>) -> Went {
    short_turn(
        on,
        "Make it brighter.",
        first,
        vec![vec![
            text("There, a little brighter now."),
            Ok(ChatEvent::Done),
        ]],
    )
}

fn calling(names: &[&str]) -> Vec<net::Result<ChatEvent>> {
    let calls = names
        .iter()
        .enumerate()
        .map(|(at, name)| call(&format!("c{at}"), name, json!({})))
        .collect();
    vec![Ok(ChatEvent::ToolCalls(calls)), Ok(ChatEvent::Done)]
}

/// A round that adds nothing: no words, no calls.
fn nothing() -> Vec<net::Result<ChatEvent>> {
    vec![Ok(ChatEvent::Done)]
}

const TODAY: &str = "said:There, a little brighter now.";

fn said(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| format!("said:{line}")).collect()
}

/// One short call, and round 2 adds nothing: its own line is said, and
/// round 2 was told the change is answered.
#[test]
fn one_short_call_is_answered_with_its_own_line_when_nothing_more_was_asked() {
    for (heard, name, line) in [
        ("Make it brighter.", "light_brighter", "Brighter."),
        ("Dim the light.", "light_dimmer", "Dimmer."),
    ] {
        assert_eq!(
            short_turn(true, heard, calling(&[name]), vec![nothing()]),
            Went {
                said: said(&[line]),
                asked: 2,
                ran: 1,
                noted: true
            },
            "{heard}"
        );
    }
}

/// The Reviewer-lead's P2 on #2930: "dim the light and tell me a joke" is
/// the call alone in round 1; round 2 tells the joke, and the joke is the
/// answer, never "Dimmer." in its place. Round 2 calling more goes on too.
#[test]
fn a_short_call_with_more_asked_gets_its_full_answer() {
    let joke = "The lamp went to school to get brighter.";
    assert_eq!(
        short_turn(
            true,
            "Dim the light and tell me a joke.",
            calling(&["light_dimmer"]),
            vec![vec![text(joke), Ok(ChatEvent::Done)]],
        ),
        Went {
            said: said(&[joke]),
            asked: 2,
            ran: 1,
            noted: true
        }
    );
    let went = short_turn(
        true,
        "Brighter, and what's the weather?",
        calling(&["light_brighter"]),
        vec![
            calling(&["get_weather"]),
            vec![text("It is sunny."), Ok(ChatEvent::Done)],
        ],
    );
    assert_eq!((went.ran, went.noted), (2, true), "{went:?}");
    assert!(
        !went.said.iter().any(|said| said.contains("Brighter.")),
        "{went:?}"
    );
}

/// Two short calls: one line, theirs when they agree, "Done." when they
/// differ, never two.
#[test]
fn two_short_calls_are_answered_with_one_line() {
    for (names, line) in [
        (["light_brighter", "volume_down"], "Done."),
        (["light_brighter", "light_brighter"], "Brighter."),
    ] {
        assert_eq!(
            short_turn(true, "Make it brighter.", calling(&names), vec![nothing()]),
            Went {
                said: said(&[line]),
                asked: 2,
                ran: 2,
                noted: true
            },
            "{names:?}"
        );
    }
}

/// A refused call, an "already", a mixed turn, the model's own words beside
/// the call, and the switch off: round 2 is asked untold and said, as today.
#[test]
fn anything_but_every_call_short_goes_on_as_today() {
    let mut worded = vec![text("Sure.")];
    worded.extend(calling(&["light_brighter"]));
    for (why, on, first, ran) in [
        ("refused", true, calling(&["light_warmer"]), 1),
        (
            "refused beside a short one",
            true,
            calling(&["light_brighter", "light_warmer"]),
            2,
        ),
        ("already", true, calling(&["light_colour"]), 1),
        ("mixed", true, calling(&["light_brighter", "set_timer"]), 2),
        ("off", false, calling(&["light_brighter"]), 1),
    ] {
        assert_eq!(
            brighter(on, first),
            Went {
                said: vec![TODAY.to_owned()],
                asked: 2,
                ran,
                noted: false
            },
            "{why}"
        );
    }
    assert_eq!(
        brighter(true, worded),
        Went {
            said: vec![
                "said:Sure.".to_owned(),
                "said: There, a little brighter now.".to_owned()
            ],
            asked: 2,
            ran: 1,
            noted: false
        },
        "worded"
    );
    // Off, a round 2 that adds nothing says nothing, as today.
    assert_eq!(
        short_turn(
            false,
            "Make it brighter.",
            calling(&["light_brighter"]),
            vec![nothing()]
        ),
        Went {
            said: Vec::new(),
            asked: 2,
            ran: 1,
            noted: false
        }
    );
}

/// The line itself: each call's `"say"` behind its `"next"`, or none.
#[test]
fn the_short_line_needs_every_call_marked_and_worded() {
    let short = |say: &str| json!({"result": "x", "next": "n", "say": say});
    assert_eq!(short_done_line(&[short("Brighter.")]), Some("Brighter."));
    assert_eq!(
        short_done_line(&[short("Brighter."), short("Brighter.")]),
        Some("Brighter.")
    );
    assert_eq!(
        short_done_line(&[short("Brighter."), short("Quieter.")]),
        Some(SHORT_DONE_TOGETHER)
    );
    assert_eq!(short_done_line(&[]), None);
    assert_eq!(
        short_done_line(&[short("Brighter."), json!({"result": "x"})]),
        None
    );
    assert_eq!(
        short_done_line(&[json!({"result": "x", "say": "Done."})]),
        None
    );
    assert_eq!(short_done_line(&[short(" ")]), None);
    assert_eq!(short_done_line(&[json!({"error": "no"})]), None);
    assert!(short_done_note("Dimmer.").contains("\"Dimmer.\""));
}

/// `LAMP_SECOND_SAY_WITH_CALL` (#2135): the acting part's own words are
/// said at once, before the next round, and that round's words follow; the
/// line is never held for it (lamp-4ace on 31 Oct: the model's own words
/// came with the call 1 time in 8, and round 2 always ran).
/// With one ask, the part's own words end the turn: the next round is not
/// asked, so it cannot say the change again in other words (lamp-4ace on 31
/// Oct, the first builder on #4710).
#[test]
fn with_say_at_once_the_parts_own_words_come_first_and_the_next_rounds_follow() {
    let again = || vec![vec![text("Brighter it is!"), Ok(ChatEvent::Done)]];
    assert_eq!(
        turn_with(
            true,
            true,
            "Make it brighter.",
            calling(&["light_brighter"]),
            again()
        ),
        Went {
            said: said(&["Brighter."]),
            asked: 1,
            ran: 1,
            noted: false
        }
    );
    // More asked: the short line first, then the rest, where without it
    // the short line went unsaid.
    let sunny = "And yes, it is sunny.";
    let more = || vec![vec![text(sunny), Ok(ChatEvent::Done)]];
    assert_eq!(
        turn_with(
            true,
            true,
            "Brighter, and is it sunny?",
            calling(&["light_brighter"]),
            more()
        ),
        Went {
            // A piece after another is said with its space.
            said: vec!["said:Brighter.".to_owned(), format!("said: {sunny}")],
            asked: 2,
            ran: 1,
            noted: true
        }
    );
    assert_eq!(
        turn_with(
            true,
            false,
            "Brighter, and is it sunny?",
            calling(&["light_brighter"]),
            more()
        )
        .said,
        said(&[sunny])
    );
    // A timer: its own clock, said with or without LAMP_SHORT_DONE, and a
    // reminder named as one; with one ask the turn ends with it.
    for short in [false, true] {
        for (heard, line) in [
            (
                "Set a timer for ten minutes.",
                "Timer set. It rings at 7:52 am.",
            ),
            (
                "Remind me in ten minutes to stretch.",
                "I'll remind you at 7:52 am.",
            ),
        ] {
            assert_eq!(
                turn_with(short, true, heard, calling(&["set_timer"]), again()),
                Went {
                    said: said(&[line]),
                    asked: 1,
                    ran: 1,
                    noted: false
                },
                "short {short}: {heard}"
            );
        }
    }
    // A timer and more: the next round answers the rest.
    assert_eq!(
        turn_with(
            false,
            true,
            "Set a timer for ten minutes and is it sunny?",
            calling(&["set_timer"]),
            more()
        ),
        Went {
            said: vec![
                "said:Timer set. It rings at 7:52 am.".to_owned(),
                format!("said: {sunny}")
            ],
            asked: 2,
            ran: 1,
            noted: true
        }
    );
    // Off: a timer's turn waits for round 2's words, as today.
    assert_eq!(
        turn_with(
            false,
            false,
            "Set a timer for ten minutes.",
            calling(&["set_timer"]),
            more()
        )
        .said,
        said(&[sunny])
    );
}

/// Never for a keep beside the change (#4635's P2: a keep can wait for a
/// yes), a call refused or unchanged, two timers, or the model's own words
/// before the call: those turns go on as without the switch.
#[test]
fn say_at_once_never_answers_a_keep_a_refusal_or_the_models_own_words() {
    let okay = || vec![vec![text("Okay."), Ok(ChatEvent::Done)]];
    let worded = || {
        let mut first = vec![text("Sure.")];
        first.extend(calling(&["set_timer"]));
        first
    };
    for (why, first) in [
        (
            "a keep beside the timer",
            calling(&["set_timer", "remember"]),
        ),
        (
            "a keep beside the light",
            calling(&["light_brighter", "remember"]),
        ),
        ("refused", calling(&["light_warmer"])),
        ("already", calling(&["light_colour"])),
        ("two timers", calling(&["set_timer", "set_timer"])),
        ("its own words first", worded()),
    ] {
        let went = turn_with(true, true, "Do it.", first, okay());
        assert!(!went.noted, "{why}: {went:?}");
        assert!(
            !went
                .said
                .iter()
                .any(|said| said.contains("Brighter.") || said.contains("Timer set")),
            "{why}: {went:?}"
        );
        assert!(
            went.said.iter().any(|said| said.ends_with("Okay.")),
            "{why}: {went:?}"
        );
    }
}

/// The model's own words with calls that each did what was asked end a
/// single ask: the next round would only say it again (the first builder's
/// run of 31 Oct, 15:34). A change that didn't happen ("already"), or more
/// asked, still asks the next round, which says so.
#[test]
fn the_models_own_words_with_calls_that_did_what_was_asked_end_a_single_ask() {
    let again = || vec![vec![text("Brighter it is!"), Ok(ChatEvent::Done)]];
    let worded = |words: &str, names: &[&str]| {
        let mut first = vec![text(words)];
        first.extend(calling(names));
        first
    };
    assert_eq!(
        turn_with(
            true,
            true,
            "Make it brighter.",
            worded("Brighter it is.", &["light_brighter"]),
            again()
        ),
        Went {
            said: said(&["Brighter it is."]),
            asked: 1,
            ran: 1,
            noted: false
        }
    );
    assert_eq!(
        turn_with(
            false,
            true,
            "Set a timer for two and a half minutes.",
            worded("Two and a half minutes.", &["set_timer"]),
            again()
        )
        .asked,
        1
    );
    // Already orange: the change didn't happen, and the next round says so.
    let already = vec![vec![text("It's already orange."), Ok(ChatEvent::Done)]];
    let went = turn_with(
        true,
        true,
        "Make it orange.",
        worded("Orange it is.", &["light_colour"]),
        already,
    );
    assert_eq!(went.asked, 2, "{went:?}");
    assert!(
        went.said
            .iter()
            .any(|said| said.ends_with("It's already orange."))
    );
    // More asked: the next round answers it.
    let sunny = vec![vec![text("And yes, it is sunny."), Ok(ChatEvent::Done)]];
    let went = turn_with(
        true,
        true,
        "Brighter, and is it sunny?",
        worded("Brighter it is.", &["light_brighter"]),
        sunny,
    );
    assert_eq!(went.asked, 2, "{went:?}");
    // Without the switch, as today: the next round runs.
    assert_eq!(
        turn_with(
            true,
            false,
            "Make it brighter.",
            worded("Brighter it is.", &["light_brighter"]),
            again()
        )
        .asked,
        2
    );
}

/// A light that can't change (the first builder's run of 31 Oct, 17:35:
/// 2.8 to 3.3 s waiting for the next round): its own words that it already
/// is are said at once and end a single ask. But the model's own words over
/// it still ask the next round, which says nothing changed, and calls that
/// differ, one of them unchanged, are never answered "Done.".
#[test]
fn a_light_that_cant_change_says_so_at_once_but_never_ends_on_the_models_words() {
    let again = || {
        vec![vec![
            text("Nothing more to give, I'm afraid."),
            Ok(ChatEvent::Done),
        ]]
    };
    assert_eq!(
        turn_with(
            true,
            true,
            "Make it cooler.",
            calling(&["light_cooler"]),
            again()
        ),
        Went {
            said: said(&["I'm already as cool as I go."]),
            asked: 1,
            ran: 1,
            noted: false
        }
    );
    let mut worded = vec![text("Cooler it is.")];
    worded.extend(calling(&["light_cooler"]));
    let went = turn_with(true, true, "Make it cooler.", worded, again());
    assert_eq!(went.asked, 2, "{went:?}");
    let went = turn_with(
        true,
        true,
        "Make it brighter and cooler.",
        calling(&["light_brighter", "light_cooler"]),
        again(),
    );
    assert_eq!(went.asked, 2, "{went:?}");
    assert!(
        !went.said.iter().any(|said| said.contains("Done.")),
        "{went:?}"
    );
    assert_eq!(
        short_done_line(&[told("light_cooler")]),
        Some("I'm already as cool as I go.")
    );
    assert_eq!(
        short_done_line(&[told("light_brighter"), told("light_cooler")]),
        None
    );
}

/// The clock said on a 12-hour clock (Thirteen's P3 on #4700).
#[test]
fn a_clock_is_said_on_a_12_hour_clock() {
    for (clock, said) in [
        ("07:52", Some("7:52 am")),
        ("19:52", Some("7:52 pm")),
        ("00:05", Some("12:05 am")),
        ("12:30", Some("12:30 pm")),
        ("23:59", Some("11:59 pm")),
        ("24:00", None),
        ("07:60", None),
        ("7:52", None),
        ("seven", None),
    ] {
        assert_eq!(spoken_clock(clock).as_deref(), said, "{clock}");
    }
}

/// The timer's line is its own answer's clock, or none: one `set_timer`,
/// committed, with an HH:MM clock.
#[test]
fn the_timer_line_is_its_own_clock_or_none() {
    let call = |name: &str| net::ToolCall {
        id: "c0".to_owned(),
        name: name.to_owned(),
        args: json!({}),
        extra: None,
    };
    let committed = json!({"status": "committed", "clock": "07:52"});
    assert_eq!(
        timer_line(
            &[call("set_timer")],
            std::slice::from_ref(&committed),
            false
        )
        .as_deref(),
        Some("Timer set. It rings at 7:52 am.")
    );
    assert_eq!(
        timer_line(&[call("set_timer")], std::slice::from_ref(&committed), true).as_deref(),
        Some("I'll remind you at 7:52 am.")
    );
    for (why, calls, told) in [
        (
            "not set",
            vec![call("set_timer")],
            vec![json!({"error": "no"})],
        ),
        (
            "no clock",
            vec![call("set_timer")],
            vec![json!({"status": "committed"})],
        ),
        (
            "not a clock",
            vec![call("set_timer")],
            vec![json!({"status": "committed", "clock": "seven"})],
        ),
        (
            "a reminder",
            vec![call("set_reminder")],
            vec![committed.clone()],
        ),
        (
            "two",
            vec![call("set_timer"), call("set_timer")],
            vec![committed.clone(), committed.clone()],
        ),
    ] {
        for reminded in [false, true] {
            assert_eq!(timer_line(&calls, &told, reminded), None, "{why}");
        }
    }
}

/// One ask or more, as the person's words say it (#2135, Reviewer-three's
/// P2s on #4726 and #4729): a clause with no word of a light, the volume or
/// a timer, a question after the first clause, or more clauses than calls is
/// more; "too" meaning "very", two changes both called, a reminder's own
/// "and", a thanks after, or a decimal point are not; a second ask joined by
/// a comma is, and so is one beside a clause that took two calls.
#[test]
fn a_line_asks_more_with_a_clause_its_calls_did_not_answer() {
    let calls = |names: &[&str]| -> Vec<net::ToolCall> {
        names
            .iter()
            .map(|name| call("c0", name, json!({})))
            .collect()
    };
    let reminder = |text: &str| {
        vec![call(
            "c0",
            "set_timer",
            json!({"text": text, "seconds": 60}),
        )]
    };
    for (one, called) in [
        ("Make it brighter.", calls(&["light_brighter"])),
        ("Remind me in two minutes to stretch.", reminder("stretch")),
        ("Set a timer for one minute", calls(&["set_timer"])),
        ("Hey Lamp, warmer please.", calls(&["light_warmer"])),
        ("Can you turn it down?", calls(&["volume_down"])),
        ("It's too bright.", calls(&["light_dimmer"])),
        ("Too dark in here.", calls(&["light_brighter"])),
        (
            "Make it warmer and brighter.",
            calls(&["light_warmer", "light_brighter"]),
        ),
        (
            "Dim it a bit and make it warmer.",
            calls(&["light_dimmer", "light_warmer"]),
        ),
        (
            "Remind me in an hour to take out the trash and recycling.",
            reminder("Take out the trash and recycling"),
        ),
        ("Make it warmer. Thanks!", calls(&["light_warmer"])),
        ("Set a timer for 2.5 minutes.", calls(&["set_timer"])),
        ("Okay, set a timer for five minutes.", calls(&["set_timer"])),
        (
            "Lamp, make it a little warmer, thank you.",
            calls(&["light_warmer"]),
        ),
        (
            "Lamp, make it warmer, thank you so much.",
            calls(&["light_warmer"]),
        ),
        ("Make it cozy.", calls(&["light_warmer", "light_dimmer"])),
        (
            "Set a timer for two and a half minutes.",
            calls(&["set_timer"]),
        ),
        ("Set a timer for an hour and a half.", calls(&["set_timer"])),
        (
            "Remind me in one and a half hours to call.",
            reminder("call"),
        ),
        // The lamp called, its name set off however it was heard: candidate
        // 9276 on lamp-4ace heard macOS `say`'s "Hey Lamp" as "Amen.",
        // "Hey, Lynn." and "Hey, man." (the first builder on #4710, 21:4x).
        ("Amen. Make it warmer. Then", calls(&["light_warmer"])),
        ("Hey, Lynn. It's too bright.", calls(&["light_dimmer"])),
        ("Hey, man. Make it warmers.", calls(&["light_warmer"])),
        ("Hey Lynn, it's too bright.", calls(&["light_dimmer"])),
        (
            "Hey Lynn, make it warmer. Thanks!",
            calls(&["light_warmer"]),
        ),
        (
            "Okay Lamb, set a timer for five minutes.",
            calls(&["set_timer"]),
        ),
        ("Hello, make it warmer.", calls(&["light_warmer"])),
        ("hey lamb, make it warmer.", calls(&["light_warmer"])),
        ("Okay lam, make it brighter.", calls(&["light_brighter"])),
    ] {
        assert!(!asks_more(one, &called), "{one}");
    }
    for more in [
        "Brighter, and is it sunny?",
        "Set a timer for ten minutes. What's the weather?",
        "Dim the light, then tell me a joke.",
        "Make it warmer, also how are you?",
        "Dim the light, tell me a joke.",
        "Make it warmer, what's the weather?",
        "Make it warmer and brighter.",
        "Make it warmer, how warm is it outside?",
        "What's the weather and make it warmer.",
        "Set a timer for two minutes and a half and tell me a joke.",
        "Hey Lynn, what's the weather?",
        "Hey Lynn, make it warmer, what's the weather?",
        "Hey there Lynn, make it warmer.",
        // Never past the line's first word, before a question mark, or a
        // word of an ask (Reviewer-three's P3 on #4789).
        "Make it warmer. Okay music.",
        "Weather? Make it warmer.",
        "Music, and make it warmer.",
    ] {
        assert!(asks_more(more, &calls(&["light_warmer"])), "{more}");
    }
    // One clause taking two calls never covers a question beside it
    // (Reviewer-three's P2 on #4729).
    for more in [
        "Make it cozy and what's the weather?",
        "Make it cozy, what's on my calendar today?",
        "Set the reading light and tell me a joke.",
    ] {
        assert!(
            asks_more(more, &calls(&["light_warmer", "light_dimmer"])),
            "{more}"
        );
    }
}

/// Known miss of the set-off name heuristic (Reviewer-two on #4789): the
/// first word after a greeting is treated as the lamp's name, even when
/// "weather" could be another ask. Pin that tradeoff explicitly; this
/// follow-up does not change which requests are judged to ask more.
#[test]
fn a_set_off_weather_after_a_greeting_is_a_known_missed_extra_ask() {
    assert!(!asks_more(
        "Okay weather, make it warmer.",
        &[call("c0", "light_warmer", json!({}))],
    ));
}

/// Candidate 9276 on lamp-4ace (the first builder on #4710, 21:4x): lights
/// at their limit said their "already" at once, then round 2 said it again
/// ("Already as warm as I go — that's my limit."). "Hey Lamp" was heard as
/// "Amen." and "Hey, Lynn.": the name set off asks nothing, so a single ask
/// ends with the at-once line; and a round still asked after one is told
/// the line was said aloud, never only that the lamp says it.
#[test]
fn an_already_said_at_once_is_said_once() {
    let again = || {
        vec![vec![
            text("Already as cool as I go — that's my limit."),
            Ok(ChatEvent::Done),
        ]]
    };
    let line = "I'm already as cool as I go.";
    for heard in [
        "Amen. Make it cooler. Then",
        "Hey, Lynn. It's too warm.",
        "Hey Lynn, make it cooler. Thanks!",
        "Okay Lamb, make it cooler.",
    ] {
        assert_eq!(
            turn_with(true, true, heard, calling(&["light_cooler"]), again()),
            Went {
                said: said(&[line]),
                asked: 1,
                ran: 1,
                noted: false
            },
            "{heard}"
        );
    }
    // "It's freezing in here" asks no change, so the round is asked, told
    // the line was said.
    let (went, notes) = turn_noted(
        true,
        true,
        "Make it cooler, it's freezing in here.",
        calling(&["light_cooler"]),
        again(),
    );
    assert_eq!(went.asked, 2, "{went:?}");
    assert_eq!(
        went.said.first().map(String::as_str),
        Some("said:I'm already as cool as I go.")
    );
    assert!(
        notes
            .iter()
            .any(|note| note.contains(&format!("already said aloud \"{line}\""))),
        "{notes:?}"
    );
    assert!(
        !notes
            .iter()
            .any(|note| note.contains("the lamp itself says")),
        "{notes:?}"
    );
    // Without the answer at once the line still waits for the round, which
    // is told the lamp says it.
    let (went, notes) = turn_noted(
        true,
        false,
        "Make it cooler, it's freezing in here.",
        calling(&["light_cooler"]),
        again(),
    );
    assert_eq!(went.asked, 2, "{went:?}");
    assert!(
        notes
            .iter()
            .any(|note| note.contains("the lamp itself says")),
        "{notes:?}"
    );
}

/// The exact heard line from the lamp-4ace case (`rUX4789-2245`, 31 Oct
/// 22:45), with lamp-actions' own already result. The marker is scripted:
/// a real model replay still has to show that it uses the new instruction.
#[test]
fn the_recorded_warmer_line_can_end_after_its_already_answer() {
    fn already_warm(name: &str) -> Value {
        if name == "light_warmer" {
            lamp_actions::Outcome::Already(
                "My light is already as warm as it goes, orange.".to_owned(),
            )
            .line_for(name, true)["result"]
                .clone()
        } else {
            told(name)
        }
    }
    let (went, notes) = turn_noted_with(
        true,
        true,
        "Make it warmer. It's freezing in here.",
        calling(&["light_warmer"]),
        vec![vec![
            text("<no_"),
            text("further_answer>"),
            Ok(ChatEvent::Done),
        ]],
        already_warm,
    );
    assert_eq!(
        went,
        Went {
            said: said(&["I'm already as warm as I go."]),
            asked: 2,
            ran: 1,
            noted: true,
        },
    );
    assert!(notes.iter().any(|note| note.contains(SILENT_AFTER_MARKER)));
}

/// A silence marker is control text only after the acting part answered.
/// In particular, ordinary "Nothing." can answer a further real question.
#[test]
fn an_answer_at_once_can_end_without_speaking_its_silence() {
    let own = "I'm already as cool as I go.";
    for chunks in [
        vec![SILENT_AFTER_MARKER],
        vec!["<no_", "further_", "answer>"],
        vec![" \n", "<", "no_further_answer", ">", " \n"],
    ] {
        let reply = chunks
            .into_iter()
            .map(text)
            .chain([Ok(ChatEvent::Done)])
            .collect();
        assert_eq!(
            turn_with(
                true,
                true,
                "Make it cooler. It's freezing in here.",
                calling(&["light_cooler"]),
                vec![reply]
            ),
            Went {
                said: said(&[own]),
                asked: 2,
                ran: 1,
                noted: true
            },
        );
    }
    for (chunks, expected) in [
        (vec!["Nothing."], "Nothing."),
        (
            vec!["Nothing more to give, I'm afraid."],
            "Nothing more to give, I'm afraid.",
        ),
        (vec!["<no_further_answer>", " It is sunny."], "It is sunny."),
        (vec!["<no_further_answer>It is sunny."], "It is sunny."),
        (vec!["<no_"], "<no_"),
        (vec!["<no_", "problem>"], "<no_problem>"),
    ] {
        let reply = chunks
            .into_iter()
            .map(text)
            .chain([Ok(ChatEvent::Done)])
            .collect();
        let went = turn_with(
            true,
            true,
            "Make it cooler and tell me what else I owe.",
            calling(&["light_cooler"]),
            vec![reply],
        );
        assert_eq!(went.said, said(&[own, &format!(" {expected}")]), "{went:?}");
        assert_eq!(went.asked, 2, "{went:?}");
    }
    // The switch off does not give an ordinary reply a new control marker.
    let went = turn_with(
        false,
        false,
        "Tell me that marker.",
        vec![text(SILENT_AFTER_MARKER), Ok(ChatEvent::Done)],
        vec![],
    );
    assert_eq!(went.said, said(&[SILENT_AFTER_MARKER]));
    assert_eq!(went.asked, 1);
}

#[test]
fn the_silence_marker_holds_only_its_bounded_prefix() {
    for split in 0..=SILENT_AFTER_MARKER.len() {
        let mut silent = SilentAfter::default();
        assert_eq!(silent.piece(SILENT_AFTER_MARKER[..split].to_owned()), "");
        assert!(silent.held.len() <= SILENT_AFTER_MARKER.len());
        assert_eq!(silent.piece(SILENT_AFTER_MARKER[split..].to_owned()), "");
        assert_eq!(silent.finish(), "");
    }
    let mut silent = SilentAfter::default();
    assert_eq!(silent.piece("<no_".to_owned()), "");
    assert_eq!(silent.piece("problem>".to_owned()), "<no_problem>");
    assert!(silent.held.is_empty());
    assert_eq!(silent.piece(" More.".to_owned()), " More.");
    let mut ordinary = SilentAfter::default();
    assert_eq!(ordinary.piece("Nothing".to_owned()), "Nothing");
    assert!(ordinary.held.is_empty());
    let mut partial = SilentAfter::default();
    assert_eq!(partial.piece("<no_".to_owned()), "");
    assert_eq!(partial.finish(), "<no_");
}
