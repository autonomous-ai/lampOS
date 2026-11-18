//! A made skill said removed, turned off or on with no `make_skill` that
//! did it (#7683, lamp-9adf, 18 Nov 14:38): "Lamp, remove planet fact." was
//! answered "planet fact" with `calls=none`, and "Yes." "planet fact is
//! removed." with `calls=none`; nothing was removed. With
//! `LAMP_ACTION_CLAIM_CHECKED` and the made skills' names read
//! ([`Engine::reading_made_names`]), such a reply is not said: the model is
//! asked again once with [`crate::action_claim::SKILL_NOTE`], and a second
//! miss is said as [`crate::action_claim::NOT_DONE_LINE`]. A real remove or
//! turn is said as before, and a reply naming a skill in passing is
//! untouched.
use super::*;
use crate::action_claim::{NOT_DONE_LINE, SKILL_NOTE};
use crate::make_skill_local::tests::keeping;
use crate::skill_write_yes as yes;

/// One turn as it went.
#[derive(Debug, Default)]
struct Went {
    /// What was said, as `said:` labels.
    said: Vec<String>,
    /// What each `make_skill` was told.
    told: Vec<Value>,
    /// The skills the manage helper was asked to remove.
    removed: Vec<String>,
    /// The skills turned off or on by the manage helper, and whether off.
    turned: Vec<(String, bool)>,
    /// The notes the rounds were asked with, each once.
    notes: Vec<String>,
    /// The calls handed on for a later turn's history, as (name, args,
    /// result).
    kept: Vec<(String, Value, Option<&'static str>)>,
}

/// The lamp after planet fact and evening dim were made, both on; moon fact
/// was made and removed earlier, so it is no longer listed.
fn lamp(test: &str) -> (std::path::PathBuf, crate::made_skill::Keeping) {
    yes::clear_for_test();
    let (root, keeping) = keeping(test);
    crate::made_remove::tests::made_here(&keeping, &[("planet-fact", true), ("evening-dim", true)]);
    (root, keeping)
}

fn listed(keeping: &crate::made_skill::Keeping) -> Vec<String> {
    crate::made_skill::entries(&keeping.directory)
        .iter()
        .filter_map(|entry| entry["name"].as_str().map(str::to_owned))
        .collect()
}

/// The history the lamp keeps of moon fact's remove before the fix: the
/// lamp's question and result as words alone.
fn moon_fact_removed_earlier() -> Vec<Message> {
    vec![
        Message::user("Lamp, remove moon fact."),
        Message::assistant("Remove moon fact?"),
        Message::user("Yes."),
        Message::assistant("moon fact is removed."),
    ]
}

/// `heard` asked in request 1 with `history` before it and the model's
/// `rounds`, the claim check on and the made skills' names read from
/// `keeping`; `make_skill` answered as the voice's own is.
fn went(
    keeping: &crate::made_skill::Keeping,
    heard: &str,
    history: Vec<Message>,
    rounds: Vec<Vec<net::Result<ChatEvent>>>,
) -> Went {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new(rounds.into()),
        ..Script::default()
    });
    let told: Arc<Mutex<Vec<Value>>> = Arc::default();
    let removed: Arc<Mutex<Vec<String>>> = Arc::default();
    let turned: Arc<Mutex<Vec<(String, bool)>>> = Arc::default();
    let (seen, asked, turns, lamp) = (
        told.clone(),
        removed.clone(),
        turned.clone(),
        keeping.clone(),
    );
    let tools: ToolRunner = Arc::new(move |request, heard, name, args| {
        let answer = if name != crate::made_skill::MAKE {
            json!({"ok": true})
        } else if let Some(id) = args[yes::APPROVE_ID].as_str() {
            yes::answered(
                id,
                request,
                args[yes::ANSWER].as_str(),
                args[yes::NAMED].as_str(),
            )
            .unwrap_or_else(|| {
                yes::approve(id, request, heard, true, Instant::now(), |asks| {
                    crate::made_remove::removed(&lamp, asks, |name| {
                        asked.lock().unwrap().push(name.to_owned());
                        Ok(())
                    })
                })
            })
        } else if let Some(name) = args[crate::made_remove::REMOVE].as_str() {
            crate::made_remove::asked(
                crate::made_remove::Ask {
                    name,
                    request,
                    heard,
                    sends_here: true,
                },
                &lamp,
                || true,
                Instant::now(),
            )
        } else if let Some((name, off)) = args[crate::made_turn::TURN_OFF]
            .as_str()
            .map(|name| (name, true))
            .or_else(|| {
                args[crate::made_turn::TURN_ON]
                    .as_str()
                    .map(|name| (name, false))
            })
        {
            crate::made_turn::asked(
                crate::made_turn::Ask {
                    name,
                    off,
                    request,
                    heard,
                },
                &lamp,
                || true,
                |name, off| {
                    turns.lock().unwrap().push((name.to_owned(), off));
                    Ok(())
                },
            )
        } else {
            json!({"error": "nothing asked"})
        };
        seen.lock().unwrap().push(answer.clone());
        Box::pin(async move { answer })
    });
    type Kept = Vec<(String, Value, Option<&'static str>)>;
    let kept: Arc<Mutex<Kept>> = Arc::default();
    let keep = kept.clone();
    let directory = keeping.directory.clone();
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(move || Told {
            text: "SYSTEM".into(),
            after: None,
            history: history.clone(),
        }),
        tools,
        Arc::new(Instant::now),
    )
    .announcing(false)
    .checking_action_claims(true)
    .reading_made_names(Some(Arc::new(move || {
        crate::made_skill::entries(&directory)
            .iter()
            .filter_map(|entry| entry["name"].as_str().map(str::to_owned))
            .collect()
    })))
    .keeping_calls_by(Some(Arc::new(
        move |_, name: &str, args: &Value, result| {
            keep.lock()
                .unwrap()
                .push((name.to_owned(), args.clone(), result));
        },
    )));
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
    let notes = script
        .asked
        .lock()
        .unwrap()
        .iter()
        .flatten()
        .filter_map(|message| message.content.as_ref().and_then(Value::as_str))
        .filter(|content| content.starts_with("(For you to know"))
        .map(str::to_owned)
        .fold(Vec::new(), |mut notes: Vec<String>, note| {
            if !notes.contains(&note) {
                notes.push(note);
            }
            notes
        });
    Went {
        said: labels
            .into_iter()
            .filter(|label| label.starts_with("said:"))
            .collect(),
        told: told.lock().unwrap().clone(),
        removed: removed.lock().unwrap().clone(),
        turned: turned.lock().unwrap().clone(),
        notes,
        kept: kept.lock().unwrap().clone(),
    }
}

fn saying(words: &str) -> Vec<net::Result<ChatEvent>> {
    vec![text(words), Ok(ChatEvent::Done)]
}

/// The model's `make_skill` call with `args`.
fn making(args: Value) -> Vec<net::Result<ChatEvent>> {
    vec![
        Ok(ChatEvent::ToolCalls(vec![call(
            "c1",
            crate::made_skill::MAKE,
            args,
        )])),
        Ok(ChatEvent::Done),
    ]
}

fn said(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| format!("said:{line}")).collect()
}

/// Whether nothing said claims `words`.
fn never_said(went: &Went, words: &str) -> bool {
    !went
        .said
        .iter()
        .any(|said| said.to_lowercase().contains(words))
}

/// lamp-9adf's request 73: moon fact removed earlier, the history showing
/// it as words alone; "remove planet fact" asked nothing, and "Yes." is
/// answered "planet fact is removed." with no call. Not said: asked again
/// with the note, the model calls the remove, and the lamp asks its
/// question. Nothing is removed, and nothing false is said.
#[test]
fn a_remove_said_done_with_no_call_is_asked_again_and_never_said() {
    let (root, keeping) = lamp("skill-claim-removed");
    let mut history = moon_fact_removed_earlier();
    history.extend([
        Message::user("Lamp, remove planet fact."),
        Message::assistant("planet fact"),
    ]);
    let went = went(
        &keeping,
        "Yes.",
        history,
        vec![
            saying("planet fact is removed."),
            making(json!({ crate::made_remove::REMOVE: "planet fact" })),
            saying("Remove planet fact?"),
        ],
    );
    assert_eq!(went.said, said(&["Remove planet fact?"]), "{went:?}");
    assert_eq!(went.notes, [SKILL_NOTE], "{went:?}");
    assert!(never_said(&went, "is removed"), "{went:?}");
    assert!(went.removed.is_empty(), "{went:?}");
    assert_eq!(listed(&keeping), ["planet-fact", "evening-dim"]);
    assert!(yes::awaited(Instant::now()), "the remove waits for its yes");
    // Its call is kept for a later turn's history by the made skill's own
    // name, told as a question asked, nothing removed.
    assert_eq!(
        went.kept,
        [(
            crate::made_skill::MAKE.to_owned(),
            json!({ crate::made_remove::REMOVE: "planet-fact" }),
            Some(crate::second_provider::MADE_REMOVE_ASKED)
        )],
        "{went:?}"
    );
    yes::clear_for_test();
    let _ = std::fs::remove_dir_all(&root);
}

/// Asked again, the model says it removed it again, after a lead-in: said
/// as the plain line that it wasn't done, never as it is.
#[test]
fn a_remove_said_done_twice_with_no_call_is_said_not_done() {
    let (root, keeping) = lamp("skill-claim-removed-twice");
    let went = went(
        &keeping,
        "Yes.",
        moon_fact_removed_earlier(),
        vec![
            saying("Planet fact is removed."),
            saying("Okay, the planet fact skill is removed."),
        ],
    );
    assert_eq!(went.said, said(&[NOT_DONE_LINE]), "{went:?}");
    assert_eq!(went.notes, [SKILL_NOTE], "{went:?}");
    assert!(went.told.is_empty(), "{went:?}");
    assert_eq!(listed(&keeping), ["planet-fact", "evening-dim"]);
    let _ = std::fs::remove_dir_all(&root);
}

/// The same for a turn off: "Planet fact is off." with no call is asked
/// again; the model then turns it off, and the lamp's result is said.
#[test]
fn a_turn_off_said_done_with_no_call_is_asked_again() {
    let (root, keeping) = lamp("skill-claim-off");
    let went = went(
        &keeping,
        "Lamp, turn off planet fact.",
        vec![
            Message::user("Lamp, turn off evening dim."),
            Message::assistant("evening dim is off."),
        ],
        vec![
            saying("Planet fact is off."),
            making(json!({ crate::made_turn::TURN_OFF: "planet fact" })),
            saying("Planet fact is off. Say turn it back on whenever you like."),
        ],
    );
    assert_eq!(
        went.said,
        said(&[
            "Planet fact is off.",
            " Say turn it back on whenever you like."
        ]),
        "{went:?}"
    );
    assert_eq!(went.notes, [SKILL_NOTE], "{went:?}");
    assert_eq!(went.turned, [("planet-fact".to_owned(), true)], "{went:?}");
    assert_eq!(
        went.kept,
        [(
            crate::made_skill::MAKE.to_owned(),
            json!({ crate::made_turn::TURN_OFF: "planet-fact" }),
            Some(crate::second_provider::MADE_TURNED)
        )],
        "{went:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The same for a turn on, missed twice: said not done, nothing turned.
#[test]
fn a_turn_on_said_done_twice_with_no_call_is_said_not_done() {
    let (root, keeping) = lamp("skill-claim-on");
    let went = went(
        &keeping,
        "Turn planet fact back on.",
        Vec::new(),
        vec![
            saying("Planet fact is on again."),
            saying("Sure, planet fact is on."),
        ],
    );
    assert_eq!(went.said, said(&[NOT_DONE_LINE]), "{went:?}");
    assert_eq!(went.notes, [SKILL_NOTE], "{went:?}");
    assert!(went.turned.is_empty(), "{went:?}");
    let _ = std::fs::remove_dir_all(&root);
}

/// A remove that only asked its question removed nothing: "planet fact is
/// removed." beside it is not said either, and the model is told the call
/// ran (never asked to call it again blindly).
#[test]
fn a_remove_said_done_beside_its_question_is_held() {
    let (root, keeping) = lamp("skill-claim-beside-question");
    let went = went(
        &keeping,
        "Lamp, remove planet fact.",
        Vec::new(),
        vec![
            making(json!({ crate::made_remove::REMOVE: "planet fact" })),
            saying("planet fact is removed."),
            saying("Remove planet fact?"),
        ],
    );
    assert_eq!(went.said, said(&["Remove planet fact?"]), "{went:?}");
    assert_eq!(went.notes, [SKILL_NOTE], "{went:?}");
    assert!(went.removed.is_empty(), "{went:?}");
    yes::clear_for_test();
    let _ = std::fs::remove_dir_all(&root);
}

/// A real remove: the question said whole, then "Yes." forced to
/// `make_skill` with the wait's id, planet fact removed, and the lamp's
/// result said as it is, with no note.
#[test]
fn a_real_remove_on_the_yes_is_said() {
    let (root, keeping) = lamp("skill-claim-real-remove");
    let told = crate::made_remove::asked(
        crate::made_remove::Ask {
            name: "planet fact",
            request: 0,
            heard: true,
            sends_here: true,
        },
        &keeping,
        || true,
        Instant::now(),
    );
    yes::lamp_said(0, "Remove planet fact?");
    let id = told[yes::APPROVE_ID].as_str().unwrap().to_owned();
    let went = went(
        &keeping,
        "Yes.",
        vec![
            Message::user("Lamp, remove planet fact."),
            Message::assistant("Remove planet fact?"),
        ],
        vec![
            making(json!({ yes::APPROVE_ID: id, yes::ANSWER: "yes", yes::NAMED: yes::NAMED_NONE })),
            saying("Done, planet fact is removed."),
        ],
    );
    assert_eq!(
        went.said,
        said(&["Done, planet fact is removed."]),
        "{went:?}"
    );
    assert!(
        !went.notes.iter().any(|note| note == SKILL_NOTE),
        "{went:?}"
    );
    assert_eq!(went.removed, ["planet-fact"], "{went:?}");
    assert_eq!(listed(&keeping), ["evening-dim"]);
    assert_eq!(
        went.kept,
        [(
            crate::made_skill::MAKE.to_owned(),
            json!({ crate::made_remove::REMOVE: "planet-fact" }),
            Some(crate::second_provider::MADE_REMOVED)
        )],
        "{went:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A reply that names a skill in passing, or tells its state when asked,
/// or names one already removed (no longer listed), is said as it is in
/// one round.
#[test]
fn a_skill_named_in_passing_is_said() {
    let (root, keeping) = lamp("skill-claim-passing");
    for (heard, reply) in [
        ("Lamp, is planet fact gone?", "planet fact is still there."),
        ("Lamp, is planet fact on?", "Planet fact is on."),
        (
            "Lamp, tell me about planet fact.",
            "Planet fact tells you a fact about a planet.",
        ),
        ("Lamp, what happened to moon fact?", "moon fact is removed."),
        ("Lamp, remove planet fact.", "Planet fact is removed?"),
    ] {
        let went = went(&keeping, heard, Vec::new(), vec![saying(reply)]);
        assert_eq!(went.said, said(&[reply]), "{heard}: {went:?}");
        assert!(
            !went.notes.iter().any(|note| note == SKILL_NOTE),
            "{heard}: {went:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}
