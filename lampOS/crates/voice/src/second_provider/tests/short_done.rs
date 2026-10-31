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
    /// Whether a round was told the change is answered (`short_done_note`).
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
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new(std::iter::once(first).chain(later).collect()),
        ..Script::default()
    });
    let ran: Calls = Arc::default();
    let seen = ran.clone();
    let tools: ToolRunner = Arc::new(move |_, heard, name, args| {
        let answer = told(&name);
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
    let said = labels
        .into_iter()
        .filter(|label| label.starts_with("said:"))
        .collect();
    let asked = script.asked.lock().unwrap();
    let noted = asked.iter().flatten().any(|message| {
        message
            .content
            .as_ref()
            .and_then(|content| content.as_str())
            .is_some_and(|content| content.contains("the lamp itself says"))
    });
    Went {
        said,
        asked: asked.len(),
        ran: ran.lock().unwrap().len(),
        noted,
    }
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
#[test]
fn with_say_at_once_the_parts_own_words_come_first_and_the_next_rounds_follow() {
    assert_eq!(
        turn_with(
            true,
            true,
            "Make it brighter.",
            calling(&["light_brighter"]),
            vec![nothing()]
        ),
        Went {
            said: said(&["Brighter."]),
            asked: 2,
            ran: 1,
            noted: true
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
    // A timer: its own clock, said with or without LAMP_SHORT_DONE.
    for short in [false, true] {
        assert_eq!(
            turn_with(
                short,
                true,
                "Set a timer for ten minutes.",
                calling(&["set_timer"]),
                vec![nothing()]
            ),
            Went {
                said: said(&["Timer set. It rings at 7:52 am."]),
                asked: 2,
                ran: 1,
                noted: true
            },
            "short {short}"
        );
    }
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
        timer_line(&[call("set_timer")], std::slice::from_ref(&committed)).as_deref(),
        Some("Timer set. It rings at 7:52 am.")
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
        assert_eq!(timer_line(&calls, &told), None, "{why}");
    }
}
