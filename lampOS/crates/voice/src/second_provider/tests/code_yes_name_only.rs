//! A line that is a call with nothing past it answers no made skill's
//! say-back (#6307; lamp-9adf, 17 Nov 13:58): an app's make with code of
//! its own, morning-mail-nine, reading Gmail, was said back and its yes
//! awaited; the next line, a lone "Hello?" heard as the name, was asked as
//! a call, its round forced to call `make_skill`, and the model's yes put
//! the code on ("It's on now"). No answer is taken in such a line, its
//! round is not forced, and the wait stays for the next real line, whose
//! round is forced as before.
use super::*;
use crate::skill_write_yes as yes;

const SAY_BACK: &str = "It reads your Gmail, and runs code of its own on the lamp.";

/// morning-mail-nine made from the app, said back whole in written request
/// 0, its yes awaited as an app's make's is (noted, forced). The wait's id.
fn said_back() -> String {
    yes::clear_for_test();
    let id = yes::wait_for_yes_noted(
        yes::Asks {
            skill: "morning-mail-nine".into(),
            sha256: "cd".repeat(32),
            writes: Vec::new(),
            say_back: SAY_BACK.into(),
            grants: Some(vec!["gmail".into()]),
            code: true,
            remove: false,
        },
        0,
        Instant::now(),
    );
    yes::lamp_said(
        0,
        &format!("I made morning mail nine. {SAY_BACK} Shall I turn it on?"),
    );
    id
}

/// `heard` asked in `request` (`called`: the coordinator heard it as the
/// lamp's name alone, `ProviderInput::Called`), the model's rounds
/// `rounds`; the runner's `make_skill` with an `approve_id` is answered as
/// the voice's own is (`make_skill_local::answer`): a no or unclear first,
/// else the yes with every guard. What each call was told, and how many
/// times the code was put on.
fn went(
    heard: &str,
    request: u64,
    called: bool,
    rounds: Vec<Vec<net::Result<ChatEvent>>>,
) -> (Vec<Value>, usize) {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new(rounds.into()),
        ..Script::default()
    });
    let told: Arc<Mutex<Vec<Value>>> = Arc::default();
    let on: Arc<Mutex<usize>> = Arc::default();
    let (seen, put_on) = (told.clone(), on.clone());
    let tools: ToolRunner = Arc::new(move |request, heard, name, args| {
        let answer = match args[yes::APPROVE_ID].as_str() {
            Some(id) if name == crate::made_skill::MAKE => {
                yes::answered(id, request, args[yes::ANSWER].as_str()).unwrap_or_else(|| {
                    yes::approve(id, request, heard, true, Instant::now(), |_| {
                        *put_on.lock().unwrap() += 1;
                        Ok(json!({"task": true}))
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
            engine.submit(start(request));
            engine.submit(audio(request, 1));
            if called {
                engine.submit(ProviderInput::Called {
                    request,
                    privacy_generation: 1,
                });
            }
            engine.submit(end(request));
            turn(&mut engine).await
        });
    let told = told.lock().unwrap().clone();
    let on = *on.lock().unwrap();
    (told, on)
}

fn said(line: &str) -> Vec<net::Result<ChatEvent>> {
    vec![text(line), Ok(ChatEvent::Done)]
}

/// Round 1 calls `make_skill` with the wait's id and `answer`, as a forced
/// round does.
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

/// Whether the second route's chat, `make_skill` declared and the force on
/// as the lamp runs it, forces `make_skill` for the person's line `heard`
/// now (`skill_write_yes::approve_force`).
fn forced(heard: &str) -> bool {
    let declared = vec![crate::make_skill_local::tool()];
    let chat = net::Chat::deepseek(
        "deepseek-chat",
        "k",
        Arc::new(move || declared.clone()),
        false,
    )
    .unwrap();
    chat.forcing(yes::approve_force());
    let line = [Message::system("You are a lamp."), Message::user(heard)];
    chat.wire(&line).2.get("tool_choice")
        == Some(&json!({"type": "function", "function": {"name": crate::made_skill::MAKE}}))
}

/// A model that calls `make_skill` only when its round is forced, and
/// otherwise answers in words: the yes of `heard` reaches the wait only
/// when code makes the call.
fn only_when_forced(id: &str, heard: &str, words: &str) -> Vec<Vec<net::Result<ChatEvent>>> {
    if forced(heard) {
        vec![answers(id, "yes"), said(words)]
    } else {
        vec![said(words)]
    }
}

/// A call with nothing past it, its model's `make_skill` a yes: the code
/// stays off, the wait stays open, and its round is not forced. The lamp's
/// own answer to the call stands not between the say-back and the next
/// line: that line's round is forced, and a model that calls `make_skill`
/// only when forced puts the code on by its yes. A call as the route's own
/// name alone ("Lamp.", "Hey Lamp?"), as the coordinator reads the name
/// alone ("Hi Lamp", "Hello Lamp?"), or as the coordinator heard it, its
/// words past the name ("Hi Lamp", "Hello Lamp?", "Lamp ơi").
#[test]
fn a_call_with_nothing_past_it_is_no_yes_to_a_code_skill_said_back() {
    for (name, called) in [
        ("Lamp.", false),
        ("Hey Lamp?", false),
        ("Hi Lamp", false),
        ("Hello Lamp?", false),
        ("Hi Lamp", true),
        ("Hello Lamp?", true),
        ("Lamp ơi", true),
    ] {
        let id = said_back();
        assert!(yes::approve_forced_now(Instant::now()));
        let (told, on) = went(
            name,
            1,
            called,
            vec![answers(&id, "yes"), said("Yes? I'm here.")],
        );
        assert_eq!(on, 0, "{name} {called}: the code went on: {told:?}");
        assert_eq!(told.len(), 1, "{name} {called}: {told:?}");
        assert_eq!(told[0]["approved"], false, "{name} {called}: {told:?}");
        assert_eq!(
            told[0]["error"],
            yes::NAME_ONLY_NOT_TAKEN,
            "{name} {called}: {told:?}"
        );
        assert!(yes::waits(&id), "{name} {called}: the wait ended");
        // Its own round is not forced, and its note gives no approve_id.
        assert!(!yes::approve_forced_now(Instant::now()), "{name} {called}");
        assert!(!forced(name), "{name} {called}");
        let note = yes::waiting_note(1, Instant::now()).unwrap();
        assert!(!note.contains(&id), "{name} {called}: {note}");
        // A no or unclear in it is no answer either.
        for answer in ["no", "unclear"] {
            let told = yes::answered(&id, 1, Some(answer)).unwrap();
            assert_eq!(told["error"], yes::NAME_ONLY_NOT_TAKEN, "{name}: {told}");
            assert!(yes::waits(&id), "{name} {called}: {answer} ended the wait");
        }
        // The lamp answers the call. The next real line is heard (as the
        // route marks it before its chat): its round is forced again, and
        // it is the answer the say-back awaits.
        yes::lamp_said(1, "Yes? I'm here.");
        yes::line_heard(2, false);
        assert!(yes::approve_forced_now(Instant::now()), "{name} {called}");
        assert!(yes::answer_awaited(2, Instant::now()), "{name} {called}");
        let (told, on) = went(
            "Yes.",
            2,
            false,
            only_when_forced(&id, "Yes.", "It's on now."),
        );
        assert_eq!(on, 1, "{name} {called}: {told:?}");
        assert_eq!(told[0]["approved"], true, "{name} {called}: {told:?}");
        assert!(!yes::waits(&id), "{name} {called}");
    }
}

/// A line with words of the person's own is judged as before: its yes is
/// taken in it.
#[test]
fn a_line_with_words_still_carries_the_yes() {
    for heard in ["Yes.", "Yes, turn it on."] {
        let id = said_back();
        let (told, on) = went(
            heard,
            1,
            false,
            vec![answers(&id, "yes"), said("It's on now.")],
        );
        assert_eq!(on, 1, "{heard}: {told:?}");
        assert_eq!(told[0]["approved"], true, "{heard}: {told:?}");
    }
}
