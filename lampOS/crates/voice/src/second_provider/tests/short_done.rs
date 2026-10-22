//! `LAMP_SHORT_DONE` on the second route (#2877): a turn whose every call
//! is a light or volume change lamp-actions answered short is said with the
//! calls' own line, and the chat is asked no further round.
use super::*;

/// What lamp-actions tells the voice of each call here, as its socket
/// answers with the switch on (`lamp_actions::Outcome::line_for`): a done
/// change with `"next"` and `"say"`, a refusal, an "already", and anyone
/// else's call.
fn told(name: &str) -> Value {
    let short = |result: &str, say: &str| json!({"result": result, "next": crate::second_provider::probe::tools::SHORT_NEXT, "say": say});
    match name {
        "light_brighter" => short("My light is a step brighter.", "Brighter."),
        "volume_down" => short("My volume is now 50 out of 100; it was 70.", "Quieter."),
        "light_dimmer" => {
            json!({"error": "I asked for it but could not confirm my light changed."})
        }
        "light_colour" => json!({"result": "My light is already orange."}),
        _ => json!({"ok": true}),
    }
}

/// One turn: "Make it brighter." asked with round 1's `first` events, the
/// switch `on`. What was said, the chat rounds asked and the calls run.
fn short_turn(on: bool, first: Vec<net::Result<ChatEvent>>) -> (Vec<String>, usize, usize) {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Make it brighter.".to_owned())].into()),
        chats: Mutex::new(
            [
                first,
                vec![text("There, a little brighter now."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
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
    let asked = script.asked.lock().unwrap().len();
    let ran = ran.lock().unwrap().len();
    (said, asked, ran)
}

fn calling(names: &[&str]) -> Vec<net::Result<ChatEvent>> {
    let calls = names
        .iter()
        .enumerate()
        .map(|(at, name)| call(&format!("c{at}"), name, json!({})))
        .collect();
    vec![Ok(ChatEvent::ToolCalls(calls)), Ok(ChatEvent::Done)]
}

const TODAY: &str = "said:There, a little brighter now.";

/// One short call: its own line is said, and round 2 is never asked.
#[test]
fn one_short_call_is_answered_with_its_own_line_and_no_second_round() {
    assert_eq!(
        short_turn(true, calling(&["light_brighter"])),
        (vec!["said:Brighter.".to_owned()], 1, 1)
    );
}

/// Two short calls: one line, theirs when they agree, "Done." when they
/// differ, never two; round 2 is never asked.
#[test]
fn two_short_calls_are_answered_with_one_line() {
    assert_eq!(
        short_turn(true, calling(&["light_brighter", "volume_down"])),
        (vec!["said:Done.".to_owned()], 1, 2)
    );
    assert_eq!(
        short_turn(true, calling(&["light_brighter", "light_brighter"])),
        (vec!["said:Brighter.".to_owned()], 1, 2)
    );
}

/// A refused call, an "already", a mixed turn, the model's own words beside
/// the call, and the switch off: round 2 is asked and said, as today.
#[test]
fn anything_but_every_call_short_goes_on_as_today() {
    let mut worded = vec![text("Sure.")];
    worded.extend(calling(&["light_brighter"]));
    for (why, on, first, ran) in [
        ("refused", true, calling(&["light_dimmer"]), 1),
        (
            "refused beside a short one",
            true,
            calling(&["light_brighter", "light_dimmer"]),
            2,
        ),
        ("already", true, calling(&["light_colour"]), 1),
        ("mixed", true, calling(&["light_brighter", "set_timer"]), 2),
        ("off", false, calling(&["light_brighter"]), 1),
    ] {
        assert_eq!(
            short_turn(on, first),
            (vec![TODAY.to_owned()], 2, ran),
            "{why}"
        );
    }
    assert_eq!(
        short_turn(true, worded),
        (
            vec![
                "said:Sure.".to_owned(),
                "said: There, a little brighter now.".to_owned()
            ],
            2,
            1
        ),
        "worded"
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
}
