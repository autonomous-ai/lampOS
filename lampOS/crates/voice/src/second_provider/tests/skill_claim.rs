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
    lamp_with(test, &[("planet-fact", true), ("evening-dim", true)], &[])
}

/// A lamp whose made skills are `made` (each on the lamp or not) and whose
/// lamp-skills `off` file names `off` ([`state_of`]).
fn lamp_with(
    test: &str,
    made: &[(&str, bool)],
    off: &[&str],
) -> (std::path::PathBuf, crate::made_skill::Keeping) {
    yes::clear_for_test();
    let (root, keeping) = keeping(test);
    crate::made_remove::tests::made_here(&keeping, made);
    let state = state_of(&keeping);
    std::fs::create_dir_all(&state).unwrap();
    let names: String = off.iter().map(|name| format!("{name}\n")).collect();
    std::fs::write(state.join(crate::skill_about::OFF_FILE), names).unwrap();
    (root, keeping)
}

/// lamp-skills' state folder beside the made skills', in a test.
fn state_of(keeping: &crate::made_skill::Keeping) -> std::path::PathBuf {
    keeping.directory.parent().unwrap().join("skills-state")
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
                    // A code make's yes: its package put on, no task kept.
                    if asks.code {
                        return Ok(json!({ "task": false }));
                    }
                    crate::made_remove::removed(&lamp, asks, |name| {
                        asked.lock().unwrap().push(name.to_owned());
                        Ok(())
                    })
                })
            })
        } else if let Some(id) = args[crate::made_clash::CLASH_ID].as_str() {
            // A clash's choice, its package put on and each other removed
            // by a helper that always does.
            crate::made_clash::chosen(
                crate::made_clash::Answer {
                    id,
                    choice: args[crate::made_clash::CHOICE].as_str(),
                    request,
                    heard,
                    here: true,
                },
                || true,
                Instant::now(),
                &crate::made_clash::Hands {
                    remove: &|_, _| Ok(()),
                    put_on: &|_, _| Ok(String::new()),
                    clock: &Instant::now,
                },
            )
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
                    state: None,
                },
                &lamp,
                || true,
                |name, off| {
                    turns.lock().unwrap().push((name.to_owned(), off));
                    Ok(())
                },
            )
        } else if args[crate::made_run::RUN].is_string() {
            // A run refused as `made_run` refuses one not on the lamp.
            json!({
                "error": "it isn't on the lamp now",
                "next": "Say plainly, in a few words, that planet fact didn't run, and why: it isn't on the lamp now. Call nothing."
            })
        } else {
            json!({"error": "nothing asked"})
        };
        seen.lock().unwrap().push(answer.clone());
        Box::pin(async move { answer })
    });
    type Kept = Vec<(String, Value, Option<&'static str>)>;
    let kept: Arc<Mutex<Kept>> = Arc::default();
    let keep = kept.clone();
    let (directory, state) = (keeping.directory.clone(), state_of(keeping));
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
        crate::action_claim::made_skills(&directory, &state)
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

/// The same for a turn on of one that is off, missed twice: said not
/// done, nothing turned.
#[test]
fn a_turn_on_said_done_twice_with_no_call_is_said_not_done() {
    let (root, keeping) = lamp_with(
        "skill-claim-on",
        &[("planet-fact", true), ("evening-dim", true)],
        &["planet-fact"],
    );
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

/// The forced yes round says the words and calls in ONE round (Twelve's
/// and the Reviewer-lead's P2 on #7730): "Okay, planet fact is removed."
/// beside `{approve_id, answer: "yes"}`. The words ran ahead of their own
/// call: not said, no note, the call removes it, and the next round says
/// the lamp's result. Never "Sorry, I didn't do that".
#[test]
fn words_beside_their_own_remove_call_are_never_told_not_done() {
    let (root, keeping) = lamp("skill-claim-one-round");
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
            vec![
                text("Okay, planet fact is removed."),
                Ok(ChatEvent::ToolCalls(vec![call(
                    "c1",
                    crate::made_skill::MAKE,
                    json!({ yes::APPROVE_ID: id, yes::ANSWER: "yes", yes::NAMED: yes::NAMED_NONE }),
                )])),
                Ok(ChatEvent::Done),
            ],
            saying("Planet fact is removed."),
        ],
    );
    assert_eq!(went.said, said(&["Planet fact is removed."]), "{went:?}");
    assert!(
        !went.notes.iter().any(|note| note == SKILL_NOTE),
        "{went:?}"
    );
    assert!(never_said(&went, "didn't do that"), "{went:?}");
    assert_eq!(went.removed, ["planet-fact"], "{went:?}");
    assert_eq!(listed(&keeping), ["evening-dim"]);
    let _ = std::fs::remove_dir_all(&root);
}

/// Crossed skills (Twelve's P2 on #7730): turning rain joke off backs no
/// "Planet fact is off.", which is held and asked again.
#[test]
fn another_skills_turn_backs_no_claim() {
    let (root, keeping) = lamp_with(
        "skill-claim-crossed",
        &[("planet-fact", true), ("rain-joke", true)],
        &[],
    );
    let went = went(
        &keeping,
        "Lamp, turn off planet fact.",
        Vec::new(),
        vec![
            making(json!({ crate::made_turn::TURN_OFF: "rain joke" })),
            saying("Planet fact is off."),
            saying("Sorry, I turned off rain joke, not planet fact."),
        ],
    );
    assert_eq!(went.notes, [SKILL_NOTE], "{went:?}");
    assert_eq!(
        went.said,
        said(&["Sorry, I turned off rain joke,", " not planet fact."]),
        "{went:?}"
    );
    assert_eq!(went.turned, [("rain-joke".to_owned(), true)], "{went:?}");
    let _ = std::fs::remove_dir_all(&root);
}

/// A true state is never held (the Reviewer-lead's P2 2 on #7730): a run of
/// planet fact refused as it isn't on the lamp gets "Planet fact is off, so
/// it didn't run.", said as it is.
#[test]
fn a_true_off_after_a_refused_run_is_said() {
    let (root, keeping) = lamp_with(
        "skill-claim-run-refused",
        &[("planet-fact", false), ("evening-dim", true)],
        &[],
    );
    let went = went(
        &keeping,
        "Lamp, run planet fact.",
        Vec::new(),
        vec![
            making(json!({ crate::made_run::RUN: "planet fact" })),
            saying("Planet fact is off, so it didn't run."),
        ],
    );
    assert_eq!(
        went.said,
        said(&["Planet fact is off,", " so it didn't run."]),
        "{went:?}"
    );
    assert!(
        !went.notes.iter().any(|note| note == SKILL_NOTE),
        "{went:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A polite status ask answered from the history with no call (the
/// Reviewer-lead's P2 3 on #7730), and "Tell me about planet fact." while
/// it is off (Twelve's P3): each true, each said as it is.
#[test]
fn a_true_state_told_with_no_call_is_said() {
    let (root, keeping) = lamp_with(
        "skill-claim-true-state",
        &[("planet-fact", true), ("rain-joke", true)],
        &["rain-joke"],
    );
    for (heard, reply, lines) in [
        (
            "Can you tell me which skills are on?",
            "Planet fact is on, and rain joke is off.",
            &["Planet fact is on,", " and rain joke is off."][..],
        ),
        (
            "Tell me about rain joke.",
            "Rain joke is off.",
            &["Rain joke is off."][..],
        ),
    ] {
        let went = went(&keeping, heard, Vec::new(), vec![saying(reply)]);
        assert_eq!(went.said, said(lines), "{heard}: {went:?}");
        assert!(went.notes.is_empty(), "{heard}: {went:?}");
    }
    let _ = std::fs::remove_dir_all(&root);
    let (root, keeping) = lamp_with(
        "skill-claim-true-off",
        &[("planet-fact", true), ("evening-dim", true)],
        &["planet-fact"],
    );
    let went = went(
        &keeping,
        "Tell me about planet fact.",
        Vec::new(),
        vec![saying("Planet fact is off.")],
    );
    assert_eq!(went.said, said(&["Planet fact is off."]), "{went:?}");
    assert!(went.notes.is_empty(), "{went:?}");
    let _ = std::fs::remove_dir_all(&root);
}

/// Twelve's P2 at c881d48f19 on #7730: a code make's yes puts the skill on,
/// and its answer names it. "Okay, planet fact is on now." after it is
/// true and said as it is, with no note. Before the yes, made.json kept it
/// not on.
#[test]
fn a_code_makes_yes_backs_its_on() {
    let (root, keeping) = lamp_with(
        "skill-claim-code-yes",
        &[("planet-fact", false), ("evening-dim", true)],
        &[],
    );
    let say_back = "It runs code of its own on the lamp.";
    let id = yes::wait_for_yes(
        yes::Asks {
            skill: "planet-fact".into(),
            sha256: "cd".repeat(32),
            writes: Vec::new(),
            say_back: say_back.into(),
            grants: None,
            code: true,
            remove: false,
        },
        0,
        Instant::now(),
    );
    let asked = format!("I made planet fact. {say_back} Shall I turn it on?");
    yes::lamp_said(0, &asked);
    let went = went(
        &keeping,
        "Yes.",
        vec![
            Message::user("Lamp, make a skill that tells me a planet fact."),
            Message::assistant(asked.as_str()),
        ],
        vec![
            making(json!({ yes::APPROVE_ID: id, yes::ANSWER: "yes", yes::NAMED: yes::NAMED_NONE })),
            saying("Okay, planet fact is on now."),
        ],
    );
    assert_eq!(went.told[0]["approved"], true, "{went:?}");
    assert_eq!(went.told[0]["made"], "planet-fact", "{went:?}");
    assert_eq!(
        went.said,
        said(&["Okay, planet fact is on now."]),
        "{went:?}"
    );
    assert!(
        !went.notes.iter().any(|note| note == SKILL_NOTE),
        "{went:?}"
    );
    yes::clear_for_test();
    let _ = std::fs::remove_dir_all(&root);
}

/// The same for a clash's keep, reworded with no "Done:": moon fact, made
/// and not on as it clashes with planet fact, is put on beside it, and
/// "Moon fact is on." is said as it is.
#[test]
fn a_clash_keep_backs_its_on() {
    let (root, keeping) = lamp_with(
        "skill-claim-clash-keep",
        &[("planet-fact", true), ("moon-fact", false)],
        &[],
    );
    let told = crate::made_clash::asked(
        &keeping,
        &crate::made_skill::Made {
            name: "moon-fact".into(),
            version: "1.0.0".into(),
            sha256: "ab".repeat(32),
            description: String::new(),
            uses: Default::default(),
            package: std::path::PathBuf::from("/nonexistent"),
        },
        &[crate::made_clash::Clash {
            name: "planet-fact".into(),
            shares: "the light",
        }],
        0,
        Instant::now(),
    );
    let question = told["next"].as_str().unwrap().split('"').nth(1).unwrap();
    crate::made_clash::lamp_said(0, question);
    let id = told[crate::made_clash::CLASH_ID].as_str().unwrap();
    let went = went(
        &keeping,
        "Keep both.",
        Vec::new(),
        vec![
            making(json!({ crate::made_clash::CLASH_ID: id, crate::made_clash::CHOICE: "keep" })),
            saying("Moon fact is on."),
        ],
    );
    assert_eq!(went.told[0]["on"], true, "{went:?}");
    assert_eq!(went.said, said(&["Moon fact is on."]), "{went:?}");
    assert!(
        !went.notes.iter().any(|note| note == SKILL_NOTE),
        "{went:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Twelve's P3s at c881d48f19 on #7730: words beside a call that changed
/// nothing are held with the note, never let go unnoted: a `make_skill`
/// that only asked its question, with "Okay, planet fact is removed." in
/// its own round, and "Done, planet fact is removed." beside it, in its
/// round or after it, as "Done" opens any call's own line.
#[test]
fn a_remove_said_done_beside_an_ask_only_call_is_noted() {
    for (test, words) in [
        ("skill-claim-ask-one-round", "Okay, planet fact is removed."),
        ("skill-claim-ask-done", "Done, planet fact is removed."),
    ] {
        let (root, keeping) = lamp(test);
        let went = went(
            &keeping,
            "Lamp, remove planet fact.",
            Vec::new(),
            vec![
                vec![
                    text(words),
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c1",
                        crate::made_skill::MAKE,
                        json!({ crate::made_remove::REMOVE: "planet fact" }),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                saying("Remove planet fact?"),
            ],
        );
        assert_eq!(
            went.said,
            said(&["Remove planet fact?"]),
            "{words}: {went:?}"
        );
        assert_eq!(went.notes, [SKILL_NOTE], "{words}: {went:?}");
        assert!(went.removed.is_empty(), "{words}: {went:?}");
        yes::clear_for_test();
        let _ = std::fs::remove_dir_all(&root);
    }
    let (root, keeping) = lamp("skill-claim-ask-done-apart");
    let went = went(
        &keeping,
        "Lamp, remove planet fact.",
        Vec::new(),
        vec![
            making(json!({ crate::made_remove::REMOVE: "planet fact" })),
            saying("Done, planet fact is removed."),
            saying("Remove planet fact?"),
        ],
    );
    assert_eq!(went.said, said(&["Remove planet fact?"]), "{went:?}");
    assert_eq!(went.notes, [SKILL_NOTE], "{went:?}");
    yes::clear_for_test();
    let _ = std::fs::remove_dir_all(&root);
}

/// Twelve's P3 at c881d48f19 on #7730: a one-word made skill said removed
/// ("Pomodoro is removed.") with no call is held as a two-word one is; a
/// one-word skill said off is still never judged ("Music is off.").
#[test]
fn a_one_word_skill_said_removed_is_held() {
    let (root, keeping) = lamp_with(
        "skill-claim-one-word",
        &[("pomodoro", true), ("music", true)],
        &[],
    );
    let removed = went(
        &keeping,
        "Lamp, remove pomodoro.",
        Vec::new(),
        vec![
            saying("Pomodoro is removed."),
            making(json!({ crate::made_remove::REMOVE: "pomodoro" })),
            saying("Remove pomodoro?"),
        ],
    );
    assert_eq!(removed.said, said(&["Remove pomodoro?"]), "{removed:?}");
    assert_eq!(removed.notes, [SKILL_NOTE], "{removed:?}");
    yes::clear_for_test();
    let went = went(
        &keeping,
        "Lamp, turn the music down.",
        Vec::new(),
        vec![saying("Music is off.")],
    );
    assert_eq!(went.said, said(&["Music is off."]), "{went:?}");
    assert!(went.notes.is_empty(), "{went:?}");
    let _ = std::fs::remove_dir_all(&root);
}

/// Heron's P3 on #7830: the turn off that acts at once (#7562, a skill
/// started only by its words too, `made_turn::TURN_OFF_AT_ONCE`) is backed
/// under `LAMP_SKILL_CLAIM_CHECKED`: "Lamp, turn off my space facts skill."
/// is `make_skill`'s `turn_off` in round 1, and "Space facts is off." with
/// its undo is said as it is, backed by the call's `turned_off`; no note,
/// nothing asked, and the call is kept for the next turn's history.
#[test]
fn a_turn_off_at_once_is_backed_by_its_turned_off() {
    let (root, keeping) = lamp_with(
        "skill-claim-off-at-once",
        &[("space-facts", true), ("planet-fact", true)],
        &[],
    );
    let went = went(
        &keeping,
        "Lamp, turn off my space facts skill.",
        Vec::new(),
        vec![
            making(json!({ crate::made_turn::TURN_OFF: "space facts" })),
            saying("Space facts is off. Say turn it back on whenever you like."),
        ],
    );
    assert_eq!(
        went.said,
        said(&[
            "Space facts is off.",
            " Say turn it back on whenever you like."
        ]),
        "{went:?}"
    );
    assert!(went.said.iter().all(|line| !line.contains('?')), "{went:?}");
    assert!(
        !went.notes.iter().any(|note| note == SKILL_NOTE),
        "{went:?}"
    );
    assert_eq!(went.turned, [("space-facts".to_owned(), true)], "{went:?}");
    assert_eq!(went.told[0][crate::made_turn::TURN_OFF], "space-facts");
    assert_eq!(
        went.kept,
        [(
            crate::made_skill::MAKE.to_owned(),
            json!({ crate::made_turn::TURN_OFF: "space-facts" }),
            Some(crate::second_provider::MADE_TURNED)
        )],
        "{went:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
