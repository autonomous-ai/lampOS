//! "Turn off the skill you just made" on the second route (#7660): on
//! lamp-9adf, after a make of daily stretch reminder, "Hmm, actually, turn
//! off the skill you just made." listed all 25 of theirs, since the model
//! was never told which skill the lamp had just made. The owner's next line
//! is told it by name (`crate::just_made::note_here`), so its call names
//! it; with no make in the conversation, nothing is told. The turn itself
//! is #7562's: the model's own `turn_on` runs on its judgment, and an ask
//! to use the skill while it is off turns it on and runs it, with the undo
//! line.
use super::*;

/// The note's own words.
const NOTE: &str = "(For you to know, not to answer: in this conversation with them";

/// The function of the skill the person made, water reminder.
const FUNCTION: &str = "water_reminder";

/// The one line the lamp says, through the model, of water reminder turned
/// on for an ask to use it (#7562), with its undo.
const UNDO: &str = "Water reminder was off, so I turned it on. Say 'turn it off' to undo.";

/// One engine's turns as they went.
#[derive(Debug)]
struct Went {
    /// Each chat's system notes, in the order the chats were asked.
    notes: Vec<Vec<String>>,
    /// The calls run: name and arguments.
    ran: Vec<(String, Value)>,
    /// What each refused or answered call told the model, by call id.
    told: Vec<(String, String)>,
    /// What was said, as `said:` labels.
    said: Vec<String>,
}

/// What was done with water reminder in the owner's conversation before
/// the test's lines.
#[derive(Clone, Copy)]
enum Before {
    Nothing,
    Made,
    MadeAndTurnedOff,
}

/// Each line of `heard` in its own request (1, 2, ...) of one engine in the
/// private room, with the switch on, answered with the model's `rounds` in
/// order, after `before` in the owner's conversation. Water reminder's
/// function answers that it is off, after a turn-off, until a `turn_on`
/// turned it on.
fn went(before: Before, heard: &[&str], rounds: Vec<Vec<net::Result<ChatEvent>>>) -> Went {
    crate::made_skill::set_off_said_here(Some(true));
    crate::person::set_private_room_for_test(Some(true));
    let owner = crate::services::owner_of(None);
    let now = Instant::now();
    if !matches!(before, Before::Nothing) {
        crate::just_made::kept_from_make(
            &json!({ "made": "water-reminder", "on": true }),
            owner.as_deref(),
            now,
        );
    }
    if matches!(before, Before::MadeAndTurnedOff) {
        let _ = crate::just_made::kept_from_turn(
            json!({ crate::made_turn::TURN_OFF: "water-reminder", "off": true }),
            owner.as_deref(),
            now,
        );
    }
    let script = Arc::new(Script {
        heard: Mutex::new(heard.iter().map(|line| Ok((*line).to_owned())).collect()),
        chats: Mutex::new(rounds.into()),
        ..Script::default()
    });
    let ran: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
    let seen = ran.clone();
    let on = Arc::new(AtomicBool::new(matches!(before, Before::Made)));
    let tools: ToolRunner = Arc::new(move |_, _, name, args| {
        let answer = if name == FUNCTION {
            if on.load(Ordering::Relaxed) {
                json!({"said": "Time for a glass of water."})
            } else {
                json!({ "error": crate::made_turn::FUNCTION_OFF })
            }
        } else if name == crate::made_skill::MAKE {
            if args[crate::made_turn::TURN_ON].is_string() {
                on.store(true, Ordering::Relaxed);
                json!({
                    crate::made_turn::TURN_ON: "water-reminder",
                    "off": false,
                    "next": "Say only that water reminder is on again."
                })
            } else {
                json!({
                    crate::made_turn::TURN_OFF: "daily-stretch-reminder",
                    "off": true,
                    "next": "Say only that daily stretch reminder is off."
                })
            }
        } else {
            json!({"ok": true})
        };
        seen.lock().unwrap().push((name, args));
        Box::pin(async move { answer })
    });
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(Instant::now),
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
    crate::person::set_private_room_for_test(None);
    let asked = script.asked.lock().unwrap();
    let notes = asked
        .iter()
        .map(|chat| {
            chat.iter()
                .filter(|message| message.role == "system")
                .filter_map(|message| message.content.as_ref().and_then(Value::as_str))
                .map(str::to_owned)
                .collect()
        })
        .collect();
    let told = asked
        .iter()
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
        notes,
        ran: ran.lock().unwrap().clone(),
        told,
        said: labels
            .into_iter()
            .filter(|label| label.starts_with("said:"))
            .collect(),
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

/// The notes of the chat `at` that are this note.
fn told_notes(went: &Went, at: usize) -> Vec<&String> {
    went.notes[at]
        .iter()
        .filter(|note| note.contains(NOTE))
        .collect()
}

fn told_for<'a>(went: &'a Went, id: &str) -> &'a str {
    went.told
        .iter()
        .find(|(call, _)| call == id)
        .map_or("", |(_, told)| told.as_str())
}

/// The row of #7660: a make, then the owner's "turn off the skill you just
/// made": the model's first round is told the skill made, by name.
#[test]
fn the_owners_line_after_a_make_is_told_the_skill_made_by_name() {
    let went = went(
        Before::Made,
        &["Hmm, actually, turn off the skill you just made."],
        vec![
            calls(
                "c1",
                crate::made_skill::MAKE,
                json!({ crate::made_turn::TURN_OFF: "water reminder" }),
            ),
            saying("Water reminder is off."),
        ],
    );
    let notes = told_notes(&went, 0);
    assert_eq!(notes.len(), 1, "{went:?}");
    assert!(
        notes[0].contains("the lamp made water reminder (just now)"),
        "{notes:?}"
    );
}

/// No make in the conversation: nothing is told, as before.
#[test]
fn with_no_make_in_the_conversation_nothing_is_told() {
    let went = went(
        Before::Nothing,
        &["Turn off the skill you just made."],
        vec![saying("Which skill do you mean?")],
    );
    assert!(told_notes(&went, 0).is_empty(), "{went:?}");
}

/// "Turn it back on" right after water reminder was turned off: the note
/// names it, and the model's `turn_on` of it runs (#7562: on its
/// judgment).
#[test]
fn turn_it_back_on_after_the_note_turns_it_on() {
    let went = went(
        Before::MadeAndTurnedOff,
        &["Turn it back on."],
        vec![
            calls(
                "c1",
                crate::made_skill::MAKE,
                json!({ crate::made_turn::TURN_ON: "water reminder" }),
            ),
            saying("Water reminder is on again."),
        ],
    );
    let notes = told_notes(&went, 0);
    assert!(
        notes
            .first()
            .is_some_and(|note| note.contains("turned water reminder off (just now)")),
        "{went:?}"
    );
    assert_eq!(went.ran.len(), 1, "{went:?}");
    assert_eq!(
        went.ran[0].1[crate::made_turn::TURN_ON],
        "water reminder",
        "{went:?}"
    );
    assert!(told_for(&went, "c1").contains("on again"), "{went:?}");
}

/// "Use the one you just made." with water reminder turned off: the note
/// names it, so the model calls its function; it answers off, and the lamp
/// turns it on and runs it again in the same turn (#7562), the model told
/// what it found and the one line with its undo, never an offer.
#[test]
fn use_the_one_you_just_made_turns_it_on_and_runs_it_with_the_undo_line() {
    let went = went(
        Before::MadeAndTurnedOff,
        &["Use the one you just made."],
        vec![
            calls("c1", FUNCTION, json!({})),
            saying(&format!("{UNDO} Time for a glass of water.")),
        ],
    );
    let notes = told_notes(&went, 0);
    assert!(
        notes
            .first()
            .is_some_and(|note| note.contains("turned water reminder off (just now)")),
        "{went:?}"
    );
    let names: Vec<&str> = went.ran.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [FUNCTION, crate::made_skill::MAKE, FUNCTION],
        "{went:?}"
    );
    assert_eq!(
        went.ran[1].1,
        json!({ crate::made_turn::TURN_ON: "water reminder" }),
        "{went:?}"
    );
    let told = told_for(&went, "c1");
    assert!(told.contains(UNDO), "{told}");
    assert!(told.contains("Time for a glass of water."), "{told}");
    assert!(!told.contains(crate::made_turn::FUNCTION_OFF), "{told}");
    assert!(!told.contains("Want me"), "{told}");
}

/// Heron's P2 on #7715: "Run the one you just made." right after a make:
/// the note says a use calls the skill as any skill (its own function,
/// `use_skill` or `make_skill` `run`), never a second make, and the model
/// following it runs water reminder, with no second make.
#[test]
fn run_the_one_you_just_made_calls_its_function_never_a_second_make() {
    let went = went(
        Before::Made,
        &["Run the one you just made."],
        vec![
            calls("c1", FUNCTION, json!({})),
            saying("Time for a glass of water."),
        ],
    );
    let notes = told_notes(&went, 0);
    assert!(
        notes.first().is_some_and(|note| {
            note.contains("the lamp made water reminder (just now)")
                && note.contains(
                    "to use or run it, call it as for any skill, by its own function, by \
                     use_skill with that name, or by make_skill with run and that name, never a \
                     second make of it",
                )
        }),
        "{went:?}"
    );
    let names: Vec<&str> = went.ran.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, [FUNCTION], "{went:?}");
    assert!(
        told_for(&went, "c1").contains("Time for a glass of water."),
        "{went:?}"
    );
}

/// The first builder, 20:55 on #7715: the note never rides into answers
/// about anything else. After a make, ten owner lines about other things
/// are each told it only as a system note "not to answer", which says a
/// line about anything else is answered as it is; no line runs a call,
/// and nothing said names the skill. By construction the lamp's own code
/// puts the note in no tool answer and nothing said: it is the model's to
/// read, and only these notes carry it.
#[test]
fn ten_owner_lines_about_other_things_say_nothing_of_the_skill() {
    let lines = [
        ("What time is it?", "It's ten past nine."),
        ("Turn it off.", "Okay, the light is off."),
        ("Turn the light on.", "The light is on."),
        ("Play some music.", "I can't play music yet."),
        ("How's the weather?", "It's sunny and 30 degrees."),
        ("Set a timer for five minutes.", "Done, five minutes."),
        ("Tell me a joke.", "A lamp went to school to get brighter."),
        ("What's two plus two?", "Four."),
        ("Make it warmer.", "The light is warmer now."),
        ("Thanks, that's all.", "You're welcome."),
    ];
    let heard: Vec<&str> = lines.iter().map(|(line, _)| *line).collect();
    let went = went(
        Before::Made,
        &heard,
        lines.iter().map(|(_, answer)| saying(answer)).collect(),
    );
    assert_eq!(went.notes.len(), lines.len(), "{went:?}");
    for at in 0..lines.len() {
        let notes = told_notes(&went, at);
        assert_eq!(notes.len(), 1, "line {at}: {went:?}");
        assert!(notes[0].contains("not to answer"), "{}", notes[0]);
        assert!(
            notes[0].contains(
                "A line about anything else, the light, music or a timer, is answered as it is: \
                 no question about skills, and no skill turned on for it."
            ),
            "{}",
            notes[0]
        );
    }
    assert!(went.ran.is_empty(), "{went:?}");
    assert!(went.told.is_empty(), "{went:?}");
    assert_eq!(went.said.len(), lines.len(), "{went:?}");
    for said in &went.said {
        let said = said.to_lowercase();
        assert!(
            !said.contains("water") && !said.contains("reminder") && !said.contains("skill"),
            "{said}"
        );
    }
}
