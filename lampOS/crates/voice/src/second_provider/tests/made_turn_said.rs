//! A skill the person made or added, turned off and on by voice
//! (`LAMP_MADE_SKILL_OFF_SAID`, #7562's rows on lamp-9adf, 18 Nov):
//!
//! - "Lamp, use my btc price skill." with btc price turned off: its function
//!   answered that it is off, the model then called `make_skill` with
//!   `turn_on`, and the lamp said "Bitcoin price is back on." and never ran
//!   it. A model's `turn_on` runs only on a line that asks it on, as the
//!   turn's only `make_skill` call (#7562 point 2): in any order of the
//!   calls, a use, or a plain yes to whatever the lamp asked, is refused
//!   back to the model, never run. Only the lamp's own offer (#7684) takes
//!   a yes, in code.
//! - "Lamp, turn off my bitcoin price skill." found no skill by that name
//!   and offered theirs (btc price among them, on); the model made no
//!   second call and said "Bitcoin price is already off …" while it was on.
//!   A round with no call after such candidates is not said: the model is
//!   asked once more, told that nothing was turned off and which are
//!   offered.
use super::*;

/// The function of the skill the person added, btc price.
const FUNCTION: &str = "btc_price";

/// One turn as it went.
#[derive(Debug)]
struct Went {
    /// What was said, as `said:` labels.
    said: Vec<String>,
    /// The forced rounds' notes the chats were asked with.
    notes: Vec<String>,
    /// The calls run: name and arguments.
    ran: Vec<(String, Value)>,
    /// What each refused or answered call told the model, by call id.
    told: Vec<(String, String)>,
}

/// What `make_skill` answers a turn: a turn off of "bitcoin price" finds
/// none by that name and offers btc price; one of "btc price" turns it.
fn make_skill_answers(args: &Value) -> Value {
    let off = args[crate::made_turn::TURN_OFF].as_str();
    let on = args[crate::made_turn::TURN_ON].as_str();
    match (off, on) {
        (Some("bitcoin price"), _) | (_, Some("bitcoin price")) => json!({
            "error": crate::made_remove::NO_MADE,
            crate::made_turn::CANDIDATES: [
                {"name": "btc price", "does": "Says the bitcoin price in dollars."},
                {"name": "moon fact", "does": "Tells a fact about the moon."}
            ],
            "next": "No skill they made or added has that name."
        }),
        (Some(_), _) => json!({
            crate::made_turn::TURN_OFF: "btc-price",
            "off": true,
            "next": "Say only that btc price is off."
        }),
        (_, Some(_)) => json!({
            crate::made_turn::TURN_ON: "btc-price",
            "off": false,
            "next": "Say only that btc price is on again."
        }),
        _ => json!({"ok": true}),
    }
}

/// `heard` in request 1, answered with the model's `rounds`, with the
/// switch on. btc price's function answers that it is turned off.
fn went(heard: &str, rounds: Vec<Vec<net::Result<ChatEvent>>>) -> Went {
    went_after(None, heard, rounds)
}

/// [`went`], after the exchange `before` (the person's line, then the
/// lamp's words), as the thread gives it to the chat.
fn went_after(
    before: Option<(&str, &str)>,
    heard: &str,
    rounds: Vec<Vec<net::Result<ChatEvent>>>,
) -> Went {
    went_all(before, &[heard], rounds, 0)
}

/// Each line of `heard` in its own request (1, 2, ...) of one engine, after
/// the exchange `before`, answered with the model's `rounds` in order (a
/// round the lamp calls in code takes none). btc price's function answers
/// that it is off until a `turn_on` turned it on, and `late` times more
/// after that, as lamp-skills reads its `off` file again within 2 s.
fn went_all(
    before: Option<(&str, &str)>,
    heard: &[&str],
    rounds: Vec<Vec<net::Result<ChatEvent>>>,
    late: usize,
) -> Went {
    went_spaced(before, heard, rounds, late, Duration::ZERO)
}

/// [`went_all`], the engine's clock moved on by `gap` before each line
/// after the first.
fn went_spaced(
    before: Option<(&str, &str)>,
    heard: &[&str],
    rounds: Vec<Vec<net::Result<ChatEvent>>>,
    late: usize,
    gap: Duration,
) -> Went {
    crate::made_skill::set_off_said_here(Some(true));
    let history: Vec<Message> = before
        .map(|(person, lamp)| vec![Message::user(person), Message::assistant(lamp)])
        .unwrap_or_default();
    let script = Arc::new(Script {
        heard: Mutex::new(heard.iter().map(|line| Ok((*line).to_owned())).collect()),
        chats: Mutex::new(rounds.into()),
        ..Script::default()
    });
    let ran: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
    let seen = ran.clone();
    // `None`, off; `Some(n)`, turned on, answering off `n` more times.
    let on: Arc<Mutex<Option<usize>>> = Arc::default();
    let tools: ToolRunner = Arc::new(move |_, _, name, args| {
        let answer = if name == FUNCTION {
            let mut on = on.lock().unwrap();
            match on.as_mut() {
                Some(0) => json!({"price": "90,000 dollars"}),
                Some(late) => {
                    *late -= 1;
                    json!({ "error": crate::made_turn::FUNCTION_OFF })
                }
                None => json!({ "error": crate::made_turn::FUNCTION_OFF }),
            }
        } else if name == crate::made_skill::MAKE {
            let answer = make_skill_answers(&args);
            if answer["off"] == false {
                *on.lock().unwrap() = Some(late);
            }
            answer
        } else {
            json!({"ok": true})
        };
        seen.lock().unwrap().push((name, args));
        Box::pin(async move { answer })
    });
    let moved: Arc<Mutex<Duration>> = Arc::default();
    let clock = moved.clone();
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(move || Told {
            text: "SYSTEM".to_owned(),
            after: None,
            history: history.clone(),
        }),
        tools,
        Arc::new(move || Instant::now() + *clock.lock().unwrap()),
    )
    .announcing(false);
    let labels = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let mut labels = Vec::new();
            for request in 1..=heard.len() as u64 {
                if request > 1 {
                    *moved.lock().unwrap() += gap;
                }
                engine.submit(start(request));
                engine.submit(audio(request, 1));
                engine.submit(end(request));
                labels.extend(turn_within(&mut engine, Duration::from_secs(5)).await);
            }
            labels
        });
    crate::made_skill::set_off_said_here(None);
    let asked = script.asked.lock().unwrap();
    let notes = asked
        .iter()
        .flatten()
        .filter(|message| message.role == "system")
        .filter_map(|message| message.content.as_ref().and_then(Value::as_str))
        .filter(|content| content.starts_with(crate::made_turn::UNCHOSEN_OPENS))
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let told = asked
        .last()
        .into_iter()
        .flatten()
        .filter(|message| message.role == "tool")
        .filter_map(|message| {
            Some((
                message.tool_call_id.clone()?,
                message.content.as_ref()?.as_str()?.to_owned(),
            ))
        })
        .collect();
    Went {
        said: labels
            .into_iter()
            .filter(|label| label.starts_with("said:"))
            .collect(),
        notes,
        ran: ran.lock().unwrap().clone(),
        told,
    }
}

fn calls(id: &str, name: &str, args: Value) -> Vec<net::Result<ChatEvent>> {
    vec![
        Ok(ChatEvent::ToolCalls(vec![call(id, name, args)])),
        Ok(ChatEvent::Done),
    ]
}

fn saying(words: &str) -> Vec<net::Result<ChatEvent>> {
    vec![text(words), Ok(ChatEvent::Done)]
}

fn said(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| format!("said:{line}")).collect()
}

fn names(went: &Went) -> Vec<&str> {
    went.ran.iter().map(|(name, _)| name.as_str()).collect()
}

/// lamp-9adf, 18 Nov 10:53, pass 1: "Lamp, use my btc price skill." with
/// btc price turned off. Its function answers that it is off; the model's
/// `turn_on` after it is refused, never run, and told why, so the lamp says
/// it is off and didn't run.
#[test]
fn a_turned_off_skill_asked_for_use_is_never_turned_on() {
    let went = went(
        "Lamp, use my btc price skill.",
        vec![
            calls("c1", FUNCTION, json!({})),
            calls(
                "c2",
                crate::made_skill::MAKE,
                json!({ crate::made_turn::TURN_ON: "btc price" }),
            ),
            saying("Btc price is turned off, so it didn't run."),
        ],
    );
    assert_eq!(names(&went), [FUNCTION], "{went:?}");
    assert_eq!(
        went.said,
        said(&["Btc price is turned off,", " so it didn't run."]),
        "{went:?}"
    );
    let refused = went
        .told
        .iter()
        .find(|(id, _)| id == "c2")
        .map(|(_, told)| told.clone())
        .unwrap_or_default();
    assert!(refused.contains(crate::made_turn::NOT_ASKED_ON), "{went:?}");
    assert!(refused.contains("didn't run"), "{refused}");
    assert!(refused.contains("Never say it is on"), "{refused}");
}

/// A line that asks to turn it back on, with no function of a turned-off
/// skill answered in the turn, turns it on as before.
#[test]
fn a_turn_on_asked_is_run_as_before() {
    let went = went(
        "Lamp, turn my btc price skill back on.",
        vec![
            calls(
                "c1",
                crate::made_skill::MAKE,
                json!({ crate::made_turn::TURN_ON: "btc price" }),
            ),
            saying("Btc price is on again."),
        ],
    );
    assert_eq!(names(&went), [crate::made_skill::MAKE], "{went:?}");
    assert_eq!(went.said, said(&["Btc price is on again."]));
    assert!(went.notes.is_empty(), "{went:?}");
}

/// What the call `id` told the model in the turn's last chat.
fn told_for(went: &Went, id: &str) -> String {
    went.told
        .iter()
        .find(|(called, _)| called == id)
        .map(|(_, told)| told.clone())
        .unwrap_or_default()
}

/// A turn on of btc price, as the model calls it.
fn turn_on(id: &str) -> Vec<net::Result<ChatEvent>> {
    calls(
        id,
        crate::made_skill::MAKE,
        json!({ crate::made_turn::TURN_ON: "btc price" }),
    )
}

/// #7562 point 2, the first builder's ranking (18 Nov): no `turn_on` of a
/// turned-off skill in any order on a line that asked to use it. The
/// model's `turn_on` before the skill's function, for "use my btc price
/// skill", is refused, never run; the function runs and answers it is off.
#[test]
fn a_turn_on_called_before_the_function_for_a_use_is_refused() {
    let went = went(
        "Lamp, use my btc price skill.",
        vec![
            turn_on("c1"),
            calls("c2", FUNCTION, json!({})),
            saying("Btc price is off."),
        ],
    );
    assert_eq!(names(&went), [FUNCTION], "{went:?}");
    assert!(
        told_for(&went, "c1").contains(crate::made_turn::NOT_ASKED_ON),
        "{went:?}"
    );
}

/// The same with the `turn_on` alone, the function never called, for a use
/// or a run in other words: refused, never run.
#[test]
fn a_turn_on_alone_for_a_use_or_a_run_is_refused() {
    for line in [
        "Lamp, use my bitcoin skill.",
        "Run btc price.",
        "Play my btc price skill.",
        "Lamp, what's the bitcoin price?",
    ] {
        let went = went(line, vec![turn_on("c1"), saying("Btc price is off.")]);
        assert!(went.ran.is_empty(), "{line}: {went:?}");
        assert!(
            told_for(&went, "c1").contains(crate::made_turn::NOT_ASKED_ON),
            "{line}: {went:?}"
        );
    }
}

/// A line that asks it on, in each of the fixed ways (the first builder's
/// 12:51 ruling on #7683), English or Vietnamese: turned on.
#[test]
fn a_line_that_asks_it_on_turns_it_on() {
    for line in [
        "Lamp, turn my btc price skill back on.",
        "Turn on btc price.",
        "Turn btc price on.",
        "Switch on my btc price skill.",
        "Enable btc price.",
        "Put btc price back on.",
        "Resume btc price.",
        "Unpause the btc price skill.",
        "Lamp, bật lại btc price.",
    ] {
        let went = went(line, vec![turn_on("c1"), saying("Btc price is on again.")]);
        assert_eq!(names(&went), [crate::made_skill::MAKE], "{line}: {went:?}");
    }
}

/// "Turn on btc price and run it": turned on, in either order of the calls.
/// A run called after the turn on in the same turn goes to the skill and is
/// answered.
#[test]
fn turn_on_and_run_it_turns_it_on_in_either_order() {
    let line = "Lamp, turn on my btc price skill and run it.";
    let first = went(
        line,
        vec![
            turn_on("c1"),
            calls("c2", FUNCTION, json!({})),
            saying("Btc price is on again."),
        ],
    );
    assert_eq!(
        names(&first),
        [crate::made_skill::MAKE, FUNCTION],
        "{first:?}"
    );
    assert!(told_for(&first, "c2").contains("90,000"), "{first:?}");
    let after = went(
        line,
        vec![
            calls("c1", FUNCTION, json!({})),
            turn_on("c2"),
            saying("Btc price is on again."),
        ],
    );
    assert_eq!(
        names(&after),
        [FUNCTION, crate::made_skill::MAKE],
        "{after:?}"
    );
}

/// A plain yes to a "turn it back on?" the model asked itself, with no
/// offer of the lamp's waiting, turns nothing on (Thirteen's P2 on #7562):
/// only the lamp's own offer (#7684), its yes read in code, turns it on;
/// nor does the model's `turn_on` of another skill on that yes.
#[test]
fn a_yes_to_the_models_own_turn_it_back_on_question_turns_nothing_on() {
    const ASKED: &str = "Btc price is off, so it didn't run. Want me to turn it back on?";
    for yes in ["Yes.", "Yeah, go ahead.", "Có."] {
        let went = went_after(
            Some((USE, ASKED)),
            yes,
            vec![turn_on("c1"), saying("Btc price is on again.")],
        );
        assert!(went.ran.is_empty(), "{yes}: {went:?}");
        assert!(
            told_for(&went, "c1").contains(crate::made_turn::NOT_ASKED_ON),
            "{yes}: {went:?}"
        );
        let other = went_after(
            Some((USE, ASKED)),
            yes,
            vec![moon_on("c1"), saying("Moon fact is on again.")],
        );
        assert!(other.ran.is_empty(), "{yes}: {other:?}");
    }
}

/// A turn on of moon fact, another skill than the one offered.
fn moon_on(id: &str) -> Vec<net::Result<ChatEvent>> {
    calls(
        id,
        crate::made_skill::MAKE,
        json!({ crate::made_turn::TURN_ON: "moon fact" }),
    )
}

/// A turn on beside another `make_skill` call in its turn is not the turn's
/// only change to a skill: refused, though the line asks it on.
#[test]
fn a_turn_on_beside_another_skill_change_is_refused() {
    let went = went(
        "Turn btc price back on.",
        vec![
            vec![
                Ok(ChatEvent::ToolCalls(vec![
                    call(
                        "c1",
                        crate::made_skill::MAKE,
                        json!({ crate::made_turn::TURN_ON: "btc price" }),
                    ),
                    call(
                        "c2",
                        crate::made_skill::MAKE,
                        json!({ crate::made_turn::TURN_OFF: "moon fact" }),
                    ),
                ])),
                Ok(ChatEvent::Done),
            ],
            saying("Done."),
        ],
    );
    assert_eq!(names(&went), [crate::made_skill::MAKE], "{went:?}");
    assert_eq!(
        went.ran[0].1[crate::made_turn::TURN_OFF],
        "moon fact",
        "{went:?}"
    );
    assert!(
        told_for(&went, "c1").contains(crate::made_turn::NOT_ASKED_ON),
        "{went:?}"
    );
}

/// lamp-9adf, 18 Nov 10:57, pass 2: "Lamp, turn off my bitcoin price
/// skill." found none by that name and offered theirs; the model made no
/// second call and said "Bitcoin price is already off" while it was on.
/// That round is not said: asked once more, told nothing was turned off
/// and which are offered, the model turns btc price off, and only then is
/// it said off.
#[test]
fn already_off_with_no_turn_off_is_not_said_and_the_candidates_are_asked_again() {
    let went = went(
        "Lamp, turn off my bitcoin price skill.",
        vec![
            calls(
                "c1",
                crate::made_skill::MAKE,
                json!({ crate::made_turn::TURN_OFF: "bitcoin price" }),
            ),
            saying("Bitcoin price is already off as of a moment ago."),
            calls(
                "c2",
                crate::made_skill::MAKE,
                json!({ crate::made_turn::TURN_OFF: "btc price" }),
            ),
            saying("Btc price is off."),
        ],
    );
    assert_eq!(went.said, said(&["Btc price is off."]), "{went:?}");
    assert_eq!(
        names(&went),
        [crate::made_skill::MAKE, crate::made_skill::MAKE]
    );
    assert_eq!(went.notes.len(), 1, "{went:?}");
    let note = &went.notes[0];
    assert!(note.contains("Nothing was turned off"), "{note}");
    assert!(
        note.contains("never say one is off or already off"),
        "{note}"
    );
    assert!(note.contains("btc price, moon fact"), "{note}");
    assert!(note.contains("turn_off"), "{note}");
}

/// The same for a turn back on by another name: never said on before a
/// `turn_on` turned it.
#[test]
fn back_on_with_no_turn_on_is_not_said_and_the_candidates_are_asked_again() {
    let went = went(
        "Lamp, turn my bitcoin price skill back on.",
        vec![
            calls(
                "c1",
                crate::made_skill::MAKE,
                json!({ crate::made_turn::TURN_ON: "bitcoin price" }),
            ),
            saying("Bitcoin price is back on."),
            calls(
                "c2",
                crate::made_skill::MAKE,
                json!({ crate::made_turn::TURN_ON: "btc price" }),
            ),
            saying("Btc price is on again."),
        ],
    );
    assert_eq!(went.said, said(&["Btc price is on again."]), "{went:?}");
    assert_eq!(went.notes.len(), 1, "{went:?}");
    assert!(went.notes[0].contains("Nothing was turned on"), "{went:?}");
}

/// None of the candidates is the skill they mean: asked once more, the
/// model says so again, and that is said; never a third round.
#[test]
fn none_of_the_candidates_is_said_after_one_ask_again() {
    let went = went(
        "Lamp, turn off my bitcoin price skill.",
        vec![
            calls(
                "c1",
                crate::made_skill::MAKE,
                json!({ crate::made_turn::TURN_OFF: "bitcoin price" }),
            ),
            saying("I found no skill like that."),
            saying("I found no skill you made or added like that."),
            saying("A third round is never asked."),
        ],
    );
    assert_eq!(
        went.said,
        said(&["I found no skill you made or added like that."]),
        "{went:?}"
    );
    assert_eq!(went.notes.len(), 1, "{went:?}");
    assert_eq!(names(&went), [crate::made_skill::MAKE]);
}

/// A turn off that turned it in its first call is said at once, as before:
/// no round held, no note.
#[test]
fn a_turn_off_found_by_its_name_is_said_as_before() {
    let went = went(
        "Lamp, turn off my btc price skill.",
        vec![
            calls(
                "c1",
                crate::made_skill::MAKE,
                json!({ crate::made_turn::TURN_OFF: "btc price" }),
            ),
            saying("Btc price is off."),
        ],
    );
    assert_eq!(went.said, said(&["Btc price is off."]), "{went:?}");
    assert!(went.notes.is_empty(), "{went:?}");
}

/// A skill's function still answered off right after a `turn_on` turned it
/// on in the turn (lamp-skills reads its `off` file again within 2 s): run
/// again after a short wait, and answered; still off after every try, the
/// model is told it was just turned on, never that it is off.
#[test]
fn a_run_just_after_its_turn_on_is_tried_again_and_never_said_off() {
    let line = "Lamp, turn on my btc price skill and run it.";
    let rounds = || {
        vec![
            turn_on("c1"),
            calls("c2", FUNCTION, json!({})),
            saying("Btc price is on again."),
        ]
    };
    let late = went_all(None, &[line], rounds(), 1);
    assert_eq!(
        names(&late),
        [crate::made_skill::MAKE, FUNCTION, FUNCTION],
        "{late:?}"
    );
    assert!(told_for(&late, "c2").contains("90,000"), "{late:?}");
    let never = went_all(None, &[line], rounds(), 100);
    let told = told_for(&never, "c2");
    assert!(told.contains(crate::made_turn::JUST_ON), "{told}");
    assert!(!told.contains(crate::made_turn::FUNCTION_OFF), "{told}");
}

/// The line that asks to use btc price while it is turned off (#7684).
const USE: &str = "Lamp, use my btc price skill.";

/// The lamp's offer for it, as the model is told to say it.
const OFFER: &str = "Btc price is turned off. Want me to turn it back on and check?";

/// #7684, the first builder's 11:05 ruling on #7562: "use X" while X is off
/// is answered with the offer, "X is turned off. Want me to turn it back on
/// and check?", and no `turn_on` is made.
#[test]
fn a_use_while_off_offers_to_turn_it_back_on_and_turns_nothing_on() {
    let went = went(USE, vec![calls("c1", FUNCTION, json!({})), saying(OFFER)]);
    assert_eq!(names(&went), [FUNCTION], "{went:?}");
    let told = told_for(&went, "c1");
    assert!(told.contains(OFFER), "{told}");
    assert!(!told.contains(crate::made_turn::FUNCTION_OFF), "{told}");
}

/// A plain yes on the very next line turns it on and runs it once for the
/// ask, in code, asked of no chat; its answer is then said, and it stays on.
#[test]
fn a_plain_yes_to_the_offer_turns_it_on_and_runs_it_once() {
    for yes in [
        "Yes.",
        "Lamp, yeah, go ahead.",
        "Okay, please.",
        "Turn it on.",
        "Vâng.",
    ] {
        let went = went_all(
            None,
            &[USE, yes],
            vec![
                calls("c1", FUNCTION, json!({})),
                saying(OFFER),
                saying("Btc price is on again: bitcoin is 90,000 dollars."),
            ],
            0,
        );
        assert_eq!(
            names(&went),
            [FUNCTION, crate::made_skill::MAKE, FUNCTION],
            "{yes}: {went:?}"
        );
        assert_eq!(
            went.ran[1].1[crate::made_turn::TURN_ON],
            "btc price",
            "{yes}: {went:?}"
        );
        assert!(
            went.said.iter().any(|said| said.contains("90,000 dollars")),
            "{yes}: {went:?}"
        );
    }
}

/// "No." leaves it off, with nothing run: a `turn_on` the model makes for
/// it is refused.
#[test]
fn a_no_to_the_offer_leaves_it_off() {
    let went = went_all(
        None,
        &[USE, "No."],
        vec![
            calls("c1", FUNCTION, json!({})),
            saying(OFFER),
            turn_on("c2"),
            saying("Okay, it stays off."),
        ],
        0,
    );
    assert_eq!(names(&went), [FUNCTION], "{went:?}");
}

/// A new ask in place of the yes leaves it off and is answered as itself;
/// a yes a line later is no yes to the offer, nor is more than a plain yes.
#[test]
fn a_new_ask_in_place_of_the_yes_leaves_it_off_and_is_answered() {
    let went = went_all(
        None,
        &[USE, "What time is it?", "Yes."],
        vec![
            calls("c1", FUNCTION, json!({})),
            saying(OFFER),
            calls("c2", "the_time", json!({})),
            saying("It's three."),
            saying("Yes to what?"),
        ],
        0,
    );
    assert_eq!(names(&went), [FUNCTION, "the_time"], "{went:?}");
    assert!(
        went.said.iter().any(|said| said == "said:It's three."),
        "{went:?}"
    );
    let more = went_all(
        None,
        &[USE, "Yes, and play some music."],
        vec![
            calls("c1", FUNCTION, json!({})),
            saying(OFFER),
            saying("Sure."),
        ],
        0,
    );
    assert_eq!(names(&more), [FUNCTION], "{more:?}");
}

/// A yes past the offer's wait (`OFFER_FOR`, 60 s) is no yes to it: nothing
/// is turned on, whatever the model calls (Thirteen's P2 on #7562).
#[test]
fn a_yes_after_the_offers_wait_turns_nothing_on() {
    let went = went_spaced(
        Some((USE, OFFER)),
        &[USE, "Yes."],
        vec![
            calls("c1", FUNCTION, json!({})),
            saying(OFFER),
            turn_on("c2"),
            saying("Btc price is on again."),
        ],
        0,
        crate::made_turn::OFFER_FOR + Duration::from_secs(1),
    );
    assert_eq!(names(&went), [FUNCTION], "{went:?}");
    assert!(
        told_for(&went, "c2").contains(crate::made_turn::NOT_ASKED_ON),
        "{went:?}"
    );
    // Within its wait, the same yes is taken.
    let within = went_spaced(
        Some((USE, OFFER)),
        &[USE, "Yes."],
        vec![
            calls("c1", FUNCTION, json!({})),
            saying(OFFER),
            saying("Btc price is on again: bitcoin is 90,000 dollars."),
        ],
        0,
        crate::made_turn::OFFER_FOR - Duration::from_secs(1),
    );
    assert_eq!(
        names(&within),
        [FUNCTION, crate::made_skill::MAKE, FUNCTION],
        "{within:?}"
    );
}

/// A yes that names another skill than the one offered is no yes to the
/// offer, and the model's `turn_on` of that other skill on it is refused;
/// nor does the model turn another skill on beside the offer's own yes.
#[test]
fn a_yes_naming_another_skill_turns_nothing_on() {
    let went = went_all(
        Some((USE, OFFER)),
        &[USE, "Yes, moon fact."],
        vec![
            calls("c1", FUNCTION, json!({})),
            saying(OFFER),
            moon_on("c2"),
            saying("Moon fact is on again."),
        ],
        0,
    );
    assert_eq!(names(&went), [FUNCTION], "{went:?}");
    assert!(
        told_for(&went, "c2").contains(crate::made_turn::NOT_ASKED_ON),
        "{went:?}"
    );
    let beside = went_all(
        Some((USE, OFFER)),
        &[USE, "Yes."],
        vec![
            calls("c1", FUNCTION, json!({})),
            saying(OFFER),
            moon_on("c2"),
            saying("Btc price is on again: bitcoin is 90,000 dollars."),
        ],
        0,
    );
    assert_eq!(
        names(&beside),
        [FUNCTION, crate::made_skill::MAKE, FUNCTION],
        "{beside:?}"
    );
    assert_eq!(
        beside.ran[1].1[crate::made_turn::TURN_ON],
        "btc price",
        "{beside:?}"
    );
}

/// The offer's yes ran the skill once in code: the model's call of the same
/// function after it is given that answer and never run again (Thirteen's
/// P3 on #7562).
#[test]
fn the_offers_run_is_never_run_again_by_the_model() {
    let went = went_all(
        None,
        &[USE, "Yes."],
        vec![
            calls("c1", FUNCTION, json!({})),
            saying(OFFER),
            calls("c2", FUNCTION, json!({})),
            saying("Btc price is on again: bitcoin is 90,000 dollars."),
        ],
        0,
    );
    assert_eq!(
        names(&went),
        [FUNCTION, crate::made_skill::MAKE, FUNCTION],
        "{went:?}"
    );
    let told = told_for(&went, "c2");
    assert!(told.contains("Not run again"), "{told}");
    assert!(told.contains("90,000"), "{told}");
}
