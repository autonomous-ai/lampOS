//! A skill the person made, removed by voice (#6960, Twelve's three tries
//! on lamp-c0a4, 12843): once the lamp asked "Remove moon fact?" whole,
//! the person's next line is made to call `make_skill` with the wait's
//! `approve_id` and their answer, as a skill's task's is (#6835 gap 2).
//! Code decides when the call is made; the model what the person answered.
use super::*;
use crate::make_skill_local::tests::keeping;
use crate::skill_write_yes as yes;

/// Twelve's lamp: moon-fact made by voice and on, beside evening-dim. The
/// remove asked in request 0, as the model passed the name it heard
/// ("moon-fax"), and the lamp's question said (`said`, none yet when
/// `None`). The keeping and the wait's id.
fn asked_to_remove(
    test: &str,
    said: Option<&str>,
) -> (std::path::PathBuf, crate::made_skill::Keeping, String) {
    yes::clear_for_test();
    let (root, keeping) = keeping(test);
    std::fs::create_dir_all(&keeping.directory).unwrap();
    let entries: Vec<Value> = ["moon-fact", "evening-dim"]
        .iter()
        .enumerate()
        .map(|(at, name)| {
            std::fs::write(keeping.directory.join(format!("{name}.tar")), "tar").unwrap();
            // On the lamp as the package made, as the manage helper's
            // install leaves it.
            let sha256 = format!("{at:x}").repeat(64);
            let folder = keeping.installed.join(name);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(
                folder.join(".package.json"),
                json!({"sha256": sha256, "manifest": {"name": name}}).to_string(),
            )
            .unwrap();
            json!({"name": name, "version": "1.0.0", "sha256": sha256,
                   "package": format!("{name}.tar"), "on": true, "from": format!("local:{name}")})
        })
        .collect();
    std::fs::write(
        keeping.directory.join(crate::made_skill::INDEX),
        serde_json::to_vec(&entries).unwrap(),
    )
    .unwrap();
    let told = crate::made_remove::asked(
        crate::made_remove::Ask {
            name: "moon-fax",
            request: 0,
            heard: true,
            sends_here: true,
        },
        &keeping,
        || true,
        Instant::now(),
    );
    assert!(
        told["next"]
            .as_str()
            .unwrap()
            .contains("\"Remove moon fact?\""),
        "{told}"
    );
    if let Some(said) = said {
        yes::lamp_said(0, said);
    }
    let id = told[yes::APPROVE_ID].as_str().unwrap().to_owned();
    (root, keeping, id)
}

fn kept(keeping: &crate::made_skill::Keeping) -> Vec<String> {
    crate::made_skill::entries(&keeping.directory)
        .iter()
        .filter_map(|entry| entry["name"].as_str().map(str::to_owned))
        .collect()
}

/// The `tool_choice` the second route's chat sends for the person's line
/// `heard` now, with `make_skill` declared and the force on.
fn forced(heard: &str) -> Option<Value> {
    let declared = vec![crate::make_skill_local::tool()];
    let chat = crate::second_provider::net::Chat::deepseek(
        "deepseek-chat",
        "k",
        Arc::new(move || declared.clone()),
        false,
    )
    .unwrap();
    chat.forcing(yes::approve_force());
    let line = [Message::system("You are a lamp."), Message::user(heard)];
    chat.wire(&line).2.get("tool_choice").cloned()
}

fn make_skill_forced() -> Option<Value> {
    Some(json!({"type": "function", "function": {"name": crate::made_skill::MAKE}}))
}

/// `heard` asked in request 1, the model's rounds `rounds`; the runner's
/// `make_skill` with an `approve_id` is answered as the voice's own is: a
/// no or unclear answer first, else the owner's yes with every guard, the
/// remove asked of the helper (`helper`, each name it was asked).
fn went(
    keeping: &crate::made_skill::Keeping,
    heard: &str,
    rounds: Vec<Vec<net::Result<ChatEvent>>>,
) -> (Vec<Value>, Vec<String>) {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new(rounds.into()),
        ..Script::default()
    });
    let told: Arc<Mutex<Vec<Value>>> = Arc::default();
    let helper: Arc<Mutex<Vec<String>>> = Arc::default();
    let (seen, asked, keeping) = (told.clone(), helper.clone(), keeping.clone());
    let tools: ToolRunner = Arc::new(move |request, heard, name, args| {
        let answer = match args[yes::APPROVE_ID].as_str() {
            Some(id) if name == crate::made_skill::MAKE => yes::answered(
                id,
                request,
                args[yes::ANSWER].as_str(),
                args[yes::NAMED].as_str(),
            )
            .unwrap_or_else(|| {
                yes::approve(id, request, heard, true, Instant::now(), |asks| {
                    crate::made_remove::removed(&keeping, asks, |name| {
                        asked.lock().unwrap().push(name.to_owned());
                        Ok(())
                    })
                })
            }),
            // A remove asked in the same line: asked as the voice's own
            // is, its own question and wait.
            _ if name == crate::made_skill::MAKE
                && args[crate::made_remove::REMOVE].is_string() =>
            {
                crate::made_remove::asked(
                    crate::made_remove::Ask {
                        name: args[crate::made_remove::REMOVE].as_str().unwrap(),
                        request,
                        heard,
                        sends_here: true,
                    },
                    &keeping,
                    || true,
                    Instant::now(),
                )
            }
            _ => json!({"next": "Say: done."}),
        };
        seen.lock().unwrap().push(answer.clone());
        Box::pin(async move { answer })
    });
    let mut engine = Engine::with_clock(
        script,
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(Instant::now),
    )
    .announcing(false);
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            engine.submit(start(1));
            engine.submit(audio(1, 1));
            engine.submit(end(1));
            turn(&mut engine).await
        });
    let told = told.lock().unwrap().clone();
    let helper = helper.lock().unwrap().clone();
    (told, helper)
}

fn said(line: &str) -> Vec<net::Result<ChatEvent>> {
    vec![text(line), Ok(ChatEvent::Done)]
}

/// The forced round 1: `make_skill` with the wait's id and `answer`, the
/// line naming no skill.
fn answers(id: &str, answer: &str) -> Vec<net::Result<ChatEvent>> {
    answers_naming(id, answer, yes::NAMED_NONE)
}

/// The forced round 1: `make_skill` with the wait's id, `answer` and the
/// skill the line names (`named`).
fn answers_naming(id: &str, answer: &str, named: &str) -> Vec<net::Result<ChatEvent>> {
    vec![
        Ok(ChatEvent::ToolCalls(vec![call(
            "c1",
            crate::made_skill::MAKE,
            json!({ yes::APPROVE_ID: id, yes::ANSWER: answer, yes::NAMED: named }),
        )])),
        Ok(ChatEvent::Done),
    ]
}

/// The round that asks to remove `name`, as the model calls it.
fn removes(name: &str) -> Vec<net::Result<ChatEvent>> {
    vec![
        Ok(ChatEvent::ToolCalls(vec![call(
            "c2",
            crate::made_skill::MAKE,
            json!({ crate::made_remove::REMOVE: name }),
        )])),
        Ok(ChatEvent::Done),
    ]
}

/// "Yes." after "Remove moon fact?": round 1 forced to `make_skill`, the
/// model's answer yes, and moon-fact removed through the helper.
#[test]
fn a_yes_after_remove_moon_fact_is_forced_and_removes_it() {
    let (root, keeping, id) = asked_to_remove("remove-words-yes", Some("Remove moon fact?"));
    assert_eq!(forced("Yes."), make_skill_forced());
    let (told, helper) = went(
        &keeping,
        "Yes.",
        vec![answers(&id, "yes"), said("Done, moon fact is removed.")],
    );
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(told[0]["removed"], "moon-fact", "{told:?}");
    assert_eq!(helper, ["moon-fact"]);
    assert_eq!(kept(&keeping), ["evening-dim"]);
    assert!(!yes::awaited(Instant::now()));
    let _ = std::fs::remove_dir_all(&root);
}

/// "No." is forced all the same; the model's answer is no, and it stays.
#[test]
fn a_no_after_remove_moon_fact_keeps_it() {
    let (root, keeping, id) = asked_to_remove("remove-words-no", Some("Remove moon fact?"));
    assert_eq!(forced("No, keep it."), make_skill_forced());
    let (told, helper) = went(
        &keeping,
        "No, keep it.",
        vec![answers(&id, "no"), said("Okay, moon fact stays.")],
    );
    assert_eq!(told[0]["approved"], false, "{told:?}");
    assert!(helper.is_empty());
    assert_eq!(kept(&keeping), ["moon-fact", "evening-dim"]);
    assert!(!yes::awaited(Instant::now()));
    let _ = std::fs::remove_dir_all(&root);
}

/// Never forced before the question was played whole, or after it was cut.
#[test]
fn no_force_before_the_question_is_said_whole() {
    let (root, _keeping, _) = asked_to_remove("remove-words-unsaid", None);
    assert_eq!(forced("Yes."), None);
    yes::lamp_said(0, "Remove moon fact?");
    yes::lamp_cut(0);
    assert_eq!(forced("Yes."), None);
    yes::clear_for_test();
    let _ = std::fs::remove_dir_all(&root);
}

/// The forced round's `make_skill` for the person's line `heard`: its
/// declared parameters, cut to the wait's fields.
fn forced_parameters(heard: &str) -> Value {
    let declared = vec![crate::make_skill_local::tool()];
    let chat = crate::second_provider::net::Chat::deepseek(
        "deepseek-chat",
        "k",
        Arc::new(move || declared.clone()),
        false,
    )
    .unwrap();
    chat.forcing(yes::approve_force());
    let line = [Message::system("You are a lamp."), Message::user(heard)];
    let body = chat.wire(&line).2;
    body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["function"]["name"] == crate::made_skill::MAKE)
        .map(|tool| tool["function"]["parameters"].clone())
        .unwrap()
}

/// lamp-9adf, 17 Nov 18:12: after "Remove mail count?", "Lamp, remove the
/// joke time skill" named another skill, was forced to a yes, no or
/// unclear, and was taken as the yes: mail count was lost. The forced
/// round may now answer that the line is a new ask (`new_ask`), the
/// model's judgment, never a list of words.
#[test]
fn the_forced_round_may_answer_that_the_line_is_a_new_ask() {
    let (root, _keeping, _id) =
        asked_to_remove("remove-words-new-ask-offered", Some("Remove moon fact?"));
    assert_eq!(forced("Remove the evening dim skill."), make_skill_forced());
    // As on the lamps, where a made skill's writes are on and the answer
    // is declared.
    crate::made_skill::set_writes_here(Some(true));
    let parameters = forced_parameters("Remove the evening dim skill.");
    crate::made_skill::set_writes_here(None);
    assert_eq!(
        parameters["required"],
        json!([yes::APPROVE_ID, yes::ANSWER, yes::NAMED]),
        "{parameters}"
    );
    // The skill the line names, the model's to fill, bound in code (#7545).
    let named = parameters["properties"][yes::NAMED]["description"]
        .as_str()
        .unwrap();
    assert!(named.contains("none"), "{named}");
    assert!(named.contains("skill asked about"), "{named}");
    let answer = &parameters["properties"][yes::ANSWER];
    let offered: Vec<&str> = answer["enum"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(offered.contains(&"new_ask"), "{answer}");
    let told = answer["description"].as_str().unwrap();
    assert!(told.contains("new_ask"), "{told}");
    assert!(told.contains("another skill"), "{told}");
    // #7561's row: a line naming the skill asked about answers it.
    assert!(told.contains("never new_ask"), "{told}");
    assert!(named.contains("copy"), "{named}");
    // The note each later line is told of the wait says so too.
    let note = yes::waiting_note(1, Instant::now()).unwrap();
    assert!(note.contains("\"new_ask\""), "{note}");
    yes::clear_for_test();
    let _ = std::fs::remove_dir_all(&root);
}

/// The same line answered `new_ask`: nothing is removed, the wait is
/// dropped, and the model is let to say it left moon fact as it is and to
/// answer the new ask in its next round (here, a remove of evening dim).
#[test]
fn a_new_ask_after_remove_moon_fact_keeps_it_and_is_answered() {
    let (root, keeping, id) = asked_to_remove("remove-words-new-ask", Some("Remove moon fact?"));
    let (told, helper) = went(
        &keeping,
        "Remove the evening dim skill.",
        vec![
            answers(&id, "new_ask"),
            removes("evening dim"),
            said("I left moon fact as it is. Remove evening dim?"),
        ],
    );
    assert_eq!(told.len(), 2, "{told:?}");
    assert_eq!(told[0]["approved"], false, "{told:?}");
    assert_eq!(told[0]["left"], "moon-fact", "{told:?}");
    let next = told[0]["next"].as_str().unwrap();
    assert!(next.contains("left moon fact as it is"), "{next}");
    assert!(!next.to_lowercase().contains("call nothing"), "{next}");
    assert!(helper.is_empty(), "{helper:?}");
    assert_eq!(kept(&keeping), ["moon-fact", "evening-dim"]);
    // Only the new ask's own question waits now: evening dim's.
    assert_eq!(told[1]["remove"], "evening-dim", "{told:?}");
    assert!(!yes::waits(&id));
    yes::clear_for_test();
    let _ = std::fs::remove_dir_all(&root);
}

/// Twelve's and El's HOLDs on #7561, #7545's P1 in code: the same line,
/// "Remove the evening dim skill.", labelled `yes` by the model (as
/// DeepSeek did on lamp-9adf), but naming evening dim. Bound to its skill,
/// that yes is no yes for moon fact: nothing is removed, the wait is
/// dropped, and the line is answered as a new ask in the same turn, with
/// evening dim's own question.
#[test]
fn a_yes_naming_another_skill_removes_nothing_and_is_answered() {
    let (root, keeping, id) = asked_to_remove("remove-words-yes-other", Some("Remove moon fact?"));
    let (told, helper) = went(
        &keeping,
        "Remove the evening dim skill.",
        vec![
            answers_naming(&id, "yes", "evening dim"),
            removes("evening dim"),
            said("I left moon fact as it is. Remove evening dim?"),
        ],
    );
    assert_eq!(told.len(), 2, "{told:?}");
    assert_eq!(told[0]["approved"], false, "{told:?}");
    assert_eq!(told[0]["left"], "moon-fact", "{told:?}");
    assert!(told[0].get("removed").is_none(), "{told:?}");
    assert!(helper.is_empty(), "{helper:?}");
    assert_eq!(kept(&keeping), ["moon-fact", "evening-dim"]);
    assert_eq!(told[1]["remove"], "evening-dim", "{told:?}");
    assert!(!yes::waits(&id));
    yes::clear_for_test();
    let _ = std::fs::remove_dir_all(&root);
}

/// El's P2 on #7561: "Yes, and remove the evening dim one too." The yes
/// names moon fact among others, so it is moon fact's: moon fact is
/// removed, and the rest of the line is answered in the next round, never
/// lost to "ask nothing, call nothing": evening dim gets its own question
/// and wait, and nothing more is removed without its own yes.
#[test]
fn a_yes_with_a_second_remove_removes_the_asked_one_and_asks_the_other() {
    let (root, keeping, id) = asked_to_remove("remove-words-yes-and", Some("Remove moon fact?"));
    let (told, helper) = went(
        &keeping,
        "Yes, and remove the evening dim one too.",
        vec![
            answers_naming(&id, "yes", "moon fact"),
            removes("evening dim"),
            said("Moon fact is removed. Remove evening dim?"),
        ],
    );
    assert_eq!(told.len(), 2, "{told:?}");
    assert_eq!(told[0]["removed"], "moon-fact", "{told:?}");
    let next = told[0]["next"].as_str().unwrap();
    assert!(next.contains("moon fact is removed"), "{next}");
    assert!(!next.to_lowercase().contains("call nothing"), "{next}");
    assert!(next.contains("asked for more"), "{next}");
    assert_eq!(helper, ["moon-fact"]);
    assert_eq!(kept(&keeping), ["evening-dim"]);
    assert_eq!(told[1]["remove"], "evening-dim", "{told:?}");
    assert!(
        yes::awaited(Instant::now()),
        "evening dim waits for its own yes"
    );
    yes::clear_for_test();
    let _ = std::fs::remove_dir_all(&root);
}
