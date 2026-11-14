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
            Some(id) if name == crate::made_skill::MAKE => {
                yes::answered(id, request, args[yes::ANSWER].as_str()).unwrap_or_else(|| {
                    yes::approve(id, request, heard, true, Instant::now(), |asks| {
                        crate::made_remove::removed(&keeping, asks, |name| {
                            asked.lock().unwrap().push(name.to_owned());
                            Ok(())
                        })
                    })
                })
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

/// The forced round 1: `make_skill` with the wait's id and `answer`.
fn answers(id: &str, answer: &str) -> Vec<net::Result<ChatEvent>> {
    vec![
        Ok(ChatEvent::ToolCalls(vec![call(
            "c1",
            crate::made_skill::MAKE,
            json!({ yes::APPROVE_ID: id, yes::ANSWER: answer }),
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
