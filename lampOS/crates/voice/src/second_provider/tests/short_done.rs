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
    .answering_short(on);
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
