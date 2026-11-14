//! The Reviewer-lead's P1 on #6614: a made skill's clash (#6593) gives its
//! `clash_id` only in the make's own answer, and this route's history keeps
//! past exchanges' words, not their calls. So on the person's next line
//! ("Keep both.") the model had no clash_id, its call was told NO_REQUEST,
//! and the made skill was dropped after `made_clash::CLASH_WITHIN`. Each
//! later line while the clash waits is told its id and question
//! (`made_clash::waiting_note`), as a yes's approve_id is.
use super::*;
use crate::make_skill_local::tests::{chat_at, good, keeping, manage_helper, service};

/// What each `make_skill` call was answered, in order.
type Told = Arc<Mutex<Vec<Value>>>;

/// Lamp-9adf, 12 Nov: "Make a skill that dims the light at 10 pm" made on
/// the trigger a skill on the lamp has, then "Keep both." on the next line.
/// The second line's model is told the clash's id and question, its call
/// carries that id, and the made skill goes on beside the other one.
#[tokio::test]
async fn keep_both_on_the_next_line_goes_on_with_the_clash_id() {
    let (root, keeping) = keeping("clash-second-route");
    let other = keeping.installed.join("rest-glow");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(
        other.join("skill.toml"),
        "kind = \"installable\"\nversion = \"1.0.0\"\n\n[trigger]\ntopic = \"voice/state\"\n\
         field = \"state\"\nequals = \"resting\"\ncooldown = \"1h\"\n\n[[do]]\n\
         light = { colour = \"blue\" }\n",
    )
    .unwrap();
    let (url, served) = service(vec![(200, good())]);
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("Make a skill that dims the light at 10 pm.".to_owned()),
                Ok("Keep both.".to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c1",
                        crate::made_skill::MAKE,
                        json!({"request": "dim the light at 10 pm"}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![
                    text("Shall I replace it, keep both, or cancel?"),
                    Ok(ChatEvent::Done),
                ],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let told: Told = Arc::default();
    let seen = told.clone();
    let made_by = keeping.clone();
    let chat = Arc::new(chat_at(url));
    let tools: ToolRunner = Arc::new(move |request, heard, name, args| {
        assert_eq!(name, crate::made_skill::MAKE);
        let answer = if let Some(id) = args[crate::made_clash::CLASH_ID].as_str() {
            // The owner's own line on this route, as `make_skill_local::answer`
            // takes it, with the product's put-on through the manage helper.
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
                &crate::made_clash::helper_hands(),
            )
        } else if let Some(ask) = args["request"].as_str() {
            let chat = chat.clone();
            crate::make_skill_local::make_with(
                request,
                Some("Make a skill that dims the light at 10 pm."),
                ask,
                &made_by,
                move || chat(),
                1_800_000_000_000,
                || None,
            )
        } else {
            json!({"error": crate::make_skill_local::NO_REQUEST})
        };
        seen.lock().unwrap().push(answer.clone());
        Box::pin(async move { answer })
    });
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(Instant::now),
    )
    .announcing(false);
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    turn(&mut engine).await;
    assert_eq!(served.join().unwrap(), 1);
    let made = told.lock().unwrap()[0].clone();
    assert_eq!(made["on"], false, "{made}");
    assert_eq!(made["clashes_with"], "rest glow", "{made}");
    let id = made[crate::made_clash::CLASH_ID]
        .as_str()
        .unwrap()
        .to_owned();
    // The question, as the thread hears it played, whole and uncut.
    let question = made["next"].as_str().unwrap().split('"').nth(1).unwrap();
    crate::made_clash::lamp_said(1, question);
    let _ = crate::make_skill_local::take_said_line();

    // The person's next line: the model calls with the id it was told.
    script.chats.lock().unwrap().extend([
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "c2",
                crate::made_skill::MAKE,
                json!({"clash_id": id, "choice": "keep"}),
            )])),
            Ok(ChatEvent::Done),
        ],
        vec![text("Done: both are on."), Ok(ChatEvent::Done)],
    ]);
    let helper = manage_helper(&keeping.manage);
    engine.retire_through(1);
    engine.submit(start(2));
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    turn(&mut engine).await;

    let asked = script.asked.lock().unwrap().clone();
    let second = asked
        .iter()
        .find(|messages| messages.last() == Some(&Message::user("Keep both.")))
        .expect("the second line was asked");
    let notes: Vec<&str> = second
        .iter()
        .filter(|message| message.role == "system")
        .filter_map(Message::words)
        .filter(|words| words.contains("clash_id"))
        .collect();
    assert_eq!(notes.len(), 1, "{second:?}");
    assert!(
        notes[0].contains(&format!("clash_id \"{id}\"")),
        "{}",
        notes[0]
    );
    assert!(notes[0].contains(question), "{}", notes[0]);
    // Nothing else in its history holds the id: without the note the
    // model would have none.
    assert!(
        !second
            .iter()
            .filter(|message| message.role != "system")
            .filter_map(Message::words)
            .any(|words| words.contains(&id)),
        "{second:?}"
    );

    let kept = told.lock().unwrap()[1].clone();
    assert_eq!(kept["kept_both"], true, "{kept}");
    assert_eq!(kept["on"], true, "{kept}");
    assert!(helper.join().unwrap()["install_made"].is_string());
    assert!(crate::made_clash::waiting_note(3, Instant::now()).is_none());
    let _ = std::fs::remove_dir_all(&root);
}

/// With no clash waiting, a line is told no clash note.
#[tokio::test]
async fn no_clash_note_when_nothing_waits() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Keep both.".to_owned())].into()),
        chats: Mutex::new([vec![text("Keep what?"), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let mut engine = engine.announcing(false);
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    turn(&mut engine).await;
    let asked = script.asked.lock().unwrap().clone();
    assert!(!asked.is_empty());
    assert!(
        !asked
            .iter()
            .flatten()
            .filter_map(Message::words)
            .any(|words| words.contains("clash_id")),
        "{asked:?}"
    );
}
