//! A skill the person made or added, turned off and on by voice
//! (`LAMP_MADE_SKILL_OFF_SAID`, #7562's rows on lamp-9adf, 18 Nov):
//!
//! - "Lamp, use my btc price skill." with btc price turned off: its function
//!   answered that it is off, the model then called `make_skill` with
//!   `turn_on`, and the lamp said "Bitcoin price is back on." and never ran
//!   it. Now (the first builder, 18:41 on 18 Nov, from the product owner:
//!   no offer, nothing asked of the person) the owner's ask of a skill
//!   turned off turns it on and runs it in that turn, said with one line
//!   and its undo; a guest's is refused. Whether a line asks a skill on is
//!   the model's, by the call it makes: no words are read for it.
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
        (_, Some(on)) if on.contains("space") => json!({
            crate::made_turn::TURN_ON: "space-facts",
            "off": false,
            "next": "Say only that space facts is on again."
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
    went_as(
        before,
        heard,
        rounds,
        Setup {
            late,
            ..Setup::default()
        },
    )
}

/// How a turn is set up: `late` as for [`went_all`]; `switch`,
/// `LAMP_MADE_SKILL_OFF_SAID` on; `owner`, the lines the owner's (a guest's
/// turn is refused by `make_skill` as `made_turn::asked` refuses it);
/// `refuses`, every run once on refused ([`REFUSED`], and btc price's
/// function [`STILL_DOING`]), as a skill that does something only at its
/// own moment.
#[derive(Clone, Copy, Debug)]
struct Setup {
    late: usize,
    switch: bool,
    owner: bool,
    refuses: bool,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            late: 0,
            switch: true,
            owner: true,
            refuses: false,
        }
    }
}

/// lamp-skills' refusal of a run now of a skill that does something only at
/// its own moment.
const REFUSED: &str = "it has nothing to do but at its own moment";

/// btc price's function refusing its call once on.
const STILL_DOING: &str = "I'm still doing that.";

/// [`went_all`] as `setup` says.
fn went_as(
    before: Option<(&str, &str)>,
    heard: &[&str],
    rounds: Vec<Vec<net::Result<ChatEvent>>>,
    setup: Setup,
) -> Went {
    let Setup {
        late,
        switch,
        owner,
        refuses,
    } = setup;
    crate::made_skill::set_off_said_here(Some(switch));
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
    let (root, keeping) = space_facts_made();
    let tools: ToolRunner = Arc::new(move |_, _, name, args| {
        let answer = if name == crate::made_skill::MAKE
            && owner
            && let Some(asked) = args[crate::made_run::RUN].as_str()
        {
            run_now(asked, &keeping, &on, late, switch, refuses)
        } else if name == FUNCTION {
            let mut on = on.lock().unwrap();
            match on.as_mut() {
                Some(0) if refuses => json!({ "error": STILL_DOING }),
                Some(0) => json!({"price": "90,000 dollars"}),
                Some(late) => {
                    *late -= 1;
                    json!({ "error": crate::made_turn::FUNCTION_OFF })
                }
                None => json!({ "error": crate::made_turn::FUNCTION_OFF }),
            }
        } else if name == crate::made_skill::MAKE && !owner {
            json!({ "error": crate::made_turn::NOT_OWNER })
        } else if name == crate::made_skill::MAKE {
            let answer = make_skill_answers(&args);
            if answer["off"] == false {
                *on.lock().unwrap() = Some(late);
            } else if answer["off"] == true {
                *on.lock().unwrap() = None;
            }
            answer
        } else {
            json!({"ok": true})
        };
        seen.lock().unwrap().push((name, args));
        Box::pin(async move { answer })
    });
    let mut engine = Engine::new(
        script.clone(),
        move || Told {
            text: "SYSTEM".to_owned(),
            after: None,
            history: history.clone(),
        },
        tools,
    )
    .announcing(false);
    let labels = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let mut labels = Vec::new();
            for request in 1..=heard.len() as u64 {
                engine.submit(start(request));
                engine.submit(audio(request, 1));
                engine.submit(end(request));
                labels.extend(turn_within(&mut engine, Duration::from_secs(5)).await);
            }
            labels
        });
    crate::made_skill::set_off_said_here(None);
    let _ = std::fs::remove_dir_all(&root);
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

/// A lamp of the test's own with space facts made and on it (#7562's row
/// on lamp-9adf, 18 Nov, requests 6 and 9).
fn space_facts_made() -> (std::path::PathBuf, crate::made_skill::Keeping) {
    static MADE: AtomicUsize = AtomicUsize::new(0);
    let (root, keeping) = crate::make_skill_local::tests::keeping(&format!(
        "turn-said-space-{}",
        MADE.fetch_add(1, Ordering::Relaxed)
    ));
    crate::made_remove::tests::made_here(&keeping, &[("space-facts", true)]);
    (root, keeping)
}

/// `make_skill`'s run now of `asked`, by `made_run`'s own code, of space
/// facts: off in lamp-skills' `off` file while `on` is `None` (lamp-skills
/// answers it `made_run::STOPPED`); `on` as [`went_all`]'s; with the
/// switch, turned on for the ask by `make_skill`'s `turn_on` as stood in
/// here.
fn run_now(
    asked: &str,
    keeping: &crate::made_skill::Keeping,
    on: &Mutex<Option<usize>>,
    late: usize,
    switch: bool,
    refuses: bool,
) -> Value {
    let is_off = |_: &str| on.lock().unwrap().is_none();
    let turn_on = |_: &str| {
        *on.lock().unwrap() = Some(late);
        Ok(())
    };
    let turn_off = |_: &str| {
        *on.lock().unwrap() = None;
        Ok(())
    };
    let turns = switch.then_some(crate::made_run::TurnsOn {
        is_off: &is_off,
        turn_on: &turn_on,
        turn_off: &turn_off,
        wait: Duration::from_millis(1),
    });
    crate::made_run::asked_turning(
        crate::made_run::Ask {
            name: asked,
            request: 1,
            heard: true,
        },
        keeping,
        || true,
        |_| Ok(None),
        |_| Some((true, true)),
        |_| {
            let mut on = on.lock().unwrap();
            match on.as_mut() {
                Some(0) if refuses => Err(REFUSED.to_owned()),
                Some(0) => Ok(false),
                Some(late) => {
                    *late -= 1;
                    Err(crate::made_run::STOPPED.to_owned())
                }
                None => Err(crate::made_run::STOPPED.to_owned()),
            }
        },
        turns,
    )
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

/// The model judges whether a line asks a skill on, by the call it makes:
/// a `turn_on` it calls before the function, for "use my btc price skill",
/// runs as any call, no words read for it, and the function then runs.
#[test]
fn the_models_own_turn_on_runs_on_its_judgment() {
    let went = went(
        USE,
        vec![
            turn_on("c1"),
            calls("c2", FUNCTION, json!({})),
            saying("Btc price is on: bitcoin is 90,000 dollars."),
        ],
    );
    assert_eq!(
        names(&went),
        [crate::made_skill::MAKE, FUNCTION],
        "{went:?}"
    );
    assert!(told_for(&went, "c2").contains("90,000"), "{went:?}");
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
    // The function first: the lamp turned it on for the ask and ran it,
    // then the model's own `turn_on` runs as any call.
    assert_eq!(
        names(&after),
        [
            FUNCTION,
            crate::made_skill::MAKE,
            FUNCTION,
            crate::made_skill::MAKE
        ],
        "{after:?}"
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

/// The line that asks to use btc price while it is turned off.
const USE: &str = "Lamp, use my btc price skill.";

/// The one line the lamp says, through the model, of a skill it turned on
/// for an ask to use it, with its undo.
const UNDO: &str = "Btc price was off, so I turned it on. Say 'turn it off' to undo.";

/// The first builder, 18:41 on 18 Nov, from the product owner: nothing is
/// asked of the person, an offer becomes the action and one line with an
/// undo. The owner's "Lamp, use my btc price skill." with btc price turned
/// off: its function answers that it is off, so the lamp turns it on and
/// runs it again in that turn, asked of no chat between; the model is told
/// what it found and the one line to say first, in the conversation's
/// language. Never "want me to turn it back on?".
#[test]
fn asked_to_run_an_off_skill_it_is_turned_on_and_run_in_one_turn_with_the_undo_line() {
    let went = went(
        USE,
        vec![
            calls("c1", FUNCTION, json!({})),
            saying(&format!("{UNDO} Bitcoin is 90,000 dollars.")),
        ],
    );
    assert_eq!(
        names(&went),
        [FUNCTION, crate::made_skill::MAKE, FUNCTION],
        "{went:?}"
    );
    assert_eq!(
        went.ran[1].1,
        json!({ crate::made_turn::TURN_ON: "btc price" }),
        "{went:?}"
    );
    let told = told_for(&went, "c1");
    assert!(told.contains(UNDO), "{told}");
    assert!(told.contains("90,000"), "{told}");
    assert!(told.contains("conversation's language"), "{told}");
    assert!(!told.contains(crate::made_turn::FUNCTION_OFF), "{told}");
    assert!(!told.contains("Want me"), "{told}");
    assert!(
        went.said.iter().any(|said| said.contains("90,000 dollars")),
        "{went:?}"
    );
}

/// The same while lamp-skills still answers off just after the turn on (it
/// reads its `off` file again within 2 s): run again after a short wait;
/// still off after every try, told it was just turned on, with the undo
/// line, never that it is off.
#[test]
fn a_run_turned_on_for_the_ask_is_tried_again_while_still_off() {
    let rounds = || vec![calls("c1", FUNCTION, json!({})), saying(UNDO)];
    let late = went_all(None, &[USE], rounds(), 1);
    assert_eq!(
        names(&late),
        [FUNCTION, crate::made_skill::MAKE, FUNCTION, FUNCTION],
        "{late:?}"
    );
    let told = told_for(&late, "c1");
    assert!(told.contains("90,000") && told.contains(UNDO), "{told}");
    let never = went_all(None, &[USE], rounds(), 100);
    let told = told_for(&never, "c1");
    assert!(told.contains(crate::made_turn::JUST_ON), "{told}");
    assert!(told.contains(UNDO), "{told}");
    assert!(!told.contains(crate::made_turn::FUNCTION_OFF), "{told}");
}

/// A guest's ask of a turned-off skill turns nothing on: the lamp's turn on
/// is refused by `make_skill` as the owner's alone, and the model is told
/// plainly that it is off and didn't run, and why, never that it is on.
#[test]
fn a_guests_ask_of_an_off_skill_is_refused_and_turns_nothing_on() {
    let went = went_as(
        None,
        &[USE],
        vec![
            calls("c1", FUNCTION, json!({})),
            saying("It's turned off, and only the lamp's owner can turn it on."),
        ],
        Setup {
            owner: false,
            ..Setup::default()
        },
    );
    assert_eq!(
        names(&went),
        [FUNCTION, crate::made_skill::MAKE],
        "{went:?}"
    );
    let told = told_for(&went, "c1");
    assert!(told.contains(crate::made_turn::NOT_OWNER), "{told}");
    assert!(told.contains("didn't run"), "{told}");
    assert!(told.contains("Never say it is on"), "{told}");
    assert!(!told.contains(UNDO), "{told}");
    assert!(!told.contains("90,000"), "{told}");
}

/// The skill turned on and run for the ask: the model's call of the same
/// function after it is given that answer and never run again.
#[test]
fn the_skill_turned_on_and_run_for_the_ask_is_never_run_again() {
    let went = went(
        USE,
        vec![
            calls("c1", FUNCTION, json!({})),
            calls("c2", FUNCTION, json!({})),
            saying(&format!("{UNDO} Bitcoin is 90,000 dollars.")),
        ],
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

/// With the switch off, a use of a turned-off skill is as before: its
/// function's answer told as it is, nothing turned on.
#[test]
fn switch_off_a_use_of_an_off_skill_changes_nothing() {
    let went = went_as(
        None,
        &[USE],
        vec![
            calls("c1", FUNCTION, json!({})),
            saying("Btc price is off."),
        ],
        Setup {
            switch: false,
            ..Setup::default()
        },
    );
    assert_eq!(names(&went), [FUNCTION], "{went:?}");
    assert_eq!(
        told_for(&went, "c1"),
        json!({ "error": crate::made_turn::FUNCTION_OFF }).to_string(),
        "{went:?}"
    );
}

/// The one line said of space facts turned on for the ask, with its undo.
const SPACE_UNDO: &str = "Space facts was off, so I turned it on. Say 'turn it off' to undo.";

/// The model's run now of space facts, as `make_skill` takes it.
fn run_space(id: &str) -> Vec<net::Result<ChatEvent>> {
    calls(
        id,
        crate::made_skill::MAKE,
        json!({ crate::made_run::RUN: "space facts" }),
    )
}

/// #7562's row on lamp-9adf, 18 Nov, request 6: "Lamp, use my space facts
/// skill." with space facts turned off was the model's `make_skill` run,
/// not the skill's function, refused "it is turned off or snoozed", and the
/// lamp said "Space facts didn't run — it's turned off at the moment."
/// (the first builder, 21:17: a use or run ask does what
/// `made_turn::on_for_ask` does, whichever tool the model reaches for).
/// Now it is turned on, run, and the undo said, nothing asked.
#[test]
fn request_6_a_run_now_of_an_off_skill_turns_it_on_runs_it_and_says_the_undo() {
    let line = "Lamp, use my space facts skill.";
    let went = went(
        line,
        vec![
            run_space("c1"),
            saying(&format!("{SPACE_UNDO} Space facts is running now.")),
        ],
    );
    assert_eq!(names(&went), [crate::made_skill::MAKE], "{went:?}");
    let told = told_for(&went, "c1");
    assert!(told.contains(SPACE_UNDO), "{told}");
    assert!(told.contains("\"ran\":true"), "{told}");
    assert!(told.contains("running now"), "{told}");
    assert!(!told.contains(crate::made_run::STOPPED), "{told}");
    assert!(!told.contains("didn't run"), "{told}");
    assert!(
        went.said.iter().any(|said| said.contains("turn it off")),
        "{went:?}"
    );
    // lamp-skills still answering off just after the turn on: run again.
    let late = went_all(None, &[line], vec![run_space("c1"), saying(SPACE_UNDO)], 1);
    let told = told_for(&late, "c1");
    assert!(
        told.contains(SPACE_UNDO) && told.contains("\"ran\":true"),
        "{told}"
    );
}

/// #7562's row on lamp-9adf, 18 Nov, request 9: "Lamp, tell me a space fact
/// with my space facts skill." was the model's `make_skill` `turn_on`: "Space
/// facts is on again.", never run. Its answer tells the model to call the
/// run now when the line asked to use it (`made_turn::asked`); the run
/// after the model's own turn on of that skill in the turn is answered with
/// the undo line, tried again while lamp-skills still reads it off.
#[test]
fn request_9_a_turn_on_then_its_run_now_says_the_undo() {
    for late in [0, 1] {
        let went = went_all(
            None,
            &["Lamp, tell me a space fact with my space facts skill."],
            vec![
                calls(
                    "c1",
                    crate::made_skill::MAKE,
                    json!({ crate::made_turn::TURN_ON: "space facts" }),
                ),
                run_space("c2"),
                saying(&format!("{SPACE_UNDO} Space facts is running now.")),
            ],
            late,
        );
        // The turn on, its run now, and that run again `late` times.
        assert_eq!(
            names(&went),
            vec![crate::made_skill::MAKE; 2 + late],
            "{went:?}"
        );
        let told = told_for(&went, "c2");
        assert!(told.contains(SPACE_UNDO), "late {late}: {told}");
        assert!(told.contains("\"ran\":true"), "late {late}: {told}");
        assert!(!told.contains(crate::made_run::STOPPED), "{told}");
    }
}

/// With the switch off, the run now of a turned-off skill is as before:
/// "didn't run now", lamp-skills' why, nothing turned on, no undo.
#[test]
fn switch_off_a_run_now_of_an_off_skill_is_as_before() {
    let went = went_as(
        None,
        &["Lamp, use my space facts skill."],
        vec![run_space("c1"), saying("Space facts didn't run.")],
        Setup {
            switch: false,
            ..Setup::default()
        },
    );
    assert_eq!(names(&went), [crate::made_skill::MAKE], "{went:?}");
    let told = told_for(&went, "c1");
    assert!(told.contains(crate::made_run::STOPPED), "{told}");
    assert!(told.contains("space facts didn't run now"), "{told}");
    assert!(!told.contains("undo"), "{told}");
}

/// Heron's P3 on #7562: the model's own `turn_on` of space facts, then its
/// run now twice in the turn: the undo line is said once, with the first
/// run; the second is told as any run.
#[test]
fn the_undo_after_the_models_turn_on_is_said_once_in_the_turn() {
    let went = went(
        "Lamp, tell me a space fact with my space facts skill.",
        vec![
            calls(
                "c1",
                crate::made_skill::MAKE,
                json!({ crate::made_turn::TURN_ON: "space facts" }),
            ),
            run_space("c2"),
            run_space("c3"),
            saying(&format!("{SPACE_UNDO} Space facts is running now.")),
        ],
    );
    assert_eq!(names(&went), vec![crate::made_skill::MAKE; 3], "{went:?}");
    let first = told_for(&went, "c2");
    assert!(first.contains(SPACE_UNDO), "{first}");
    let again = told_for(&went, "c3");
    assert!(again.contains("\"ran\":true"), "{again}");
    assert!(!again.contains(SPACE_UNDO), "{again}");
    assert!(!again.contains("turned_on"), "{again}");
}

/// Heron's P2 on #7562, the second route: space facts turned on by the
/// model's own `turn_on`, then its run now refused (it does something only
/// at its own moment): turned off again by the lamp's own `make_skill`
/// `turn_off`, and the model told plainly it didn't run and is still off,
/// with no undo.
#[test]
fn a_run_refused_after_the_models_turn_on_turns_it_off_again() {
    let went = went_as(
        None,
        &["Lamp, tell me a space fact with my space facts skill."],
        vec![
            calls(
                "c1",
                crate::made_skill::MAKE,
                json!({ crate::made_turn::TURN_ON: "space facts" }),
            ),
            run_space("c2"),
            saying("Space facts didn't run, and it is still off."),
        ],
        Setup {
            refuses: true,
            ..Setup::default()
        },
    );
    assert_eq!(went.ran.len(), 3, "{went:?}");
    assert_eq!(
        went.ran[2],
        (
            crate::made_skill::MAKE.to_owned(),
            json!({ crate::made_turn::TURN_OFF: "space facts" })
        ),
        "{went:?}"
    );
    let told = told_for(&went, "c2");
    assert!(told.contains(REFUSED), "{told}");
    assert!(told.contains("still off"), "{told}");
    assert!(!told.contains(SPACE_UNDO), "{told}");
}

/// The same for the lamp's own turn on for a use of btc price's function:
/// its run again refused, btc price is turned off again and said still
/// off; a call of it again in the turn is never run.
#[test]
fn a_function_refused_after_the_turn_on_for_the_ask_turns_it_off_again() {
    let went = went_as(
        None,
        &[USE],
        vec![
            calls("c1", FUNCTION, json!({})),
            calls("c2", FUNCTION, json!({})),
            saying("Btc price didn't run, and it is still off."),
        ],
        Setup {
            refuses: true,
            ..Setup::default()
        },
    );
    assert_eq!(
        names(&went),
        [
            FUNCTION,
            crate::made_skill::MAKE,
            FUNCTION,
            crate::made_skill::MAKE
        ],
        "{went:?}"
    );
    assert_eq!(
        went.ran[3].1,
        json!({ crate::made_turn::TURN_OFF: "btc price" }),
        "{went:?}"
    );
    let told = told_for(&went, "c1");
    assert!(told.contains(STILL_DOING), "{told}");
    assert!(told.contains("still off"), "{told}");
    assert!(!told.contains(UNDO), "{told}");
    assert!(told_for(&went, "c2").contains("Not run again"), "{went:?}");
}
