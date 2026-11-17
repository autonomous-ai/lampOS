//! A line that is only the lamp's name answers no made skill's say-back
//! (#6307; lamp-9adf, 17 Nov 13:58): an app's make with code of its own,
//! morning-mail-nine, reading Gmail, was said back and its yes awaited;
//! the next line, a lone "Hello?" heard as the name, was asked as a call,
//! its round forced to call `make_skill`, and the model's yes put the code
//! on ("It's on now"). No answer is taken in such a line, its round is not
//! forced, and the wait stays for the next real line.
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

/// `heard` asked in `request`, the model's rounds `rounds`; the runner's
/// `make_skill` with an `approve_id` is answered as the voice's own is
/// (`make_skill_local::answer`): a no or unclear first, else the yes with
/// every guard. What each call was told, and how many times the code was
/// put on.
fn went(
    heard: &str,
    request: u64,
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

/// The lamp's name alone, its model's `make_skill` a yes: the code stays
/// off, the wait stays open, and its round is not forced; the yes on the
/// next real line is taken.
#[test]
fn the_lamps_name_alone_is_no_yes_to_a_code_skill_said_back() {
    for name in ["Lamp.", "Hey Lamp?"] {
        let id = said_back();
        assert!(yes::approve_forced_now(Instant::now()));
        let (told, on) = went(name, 1, vec![answers(&id, "yes"), said("Yes? I'm here.")]);
        assert_eq!(on, 0, "{name}: the code went on: {told:?}");
        assert_eq!(told.len(), 1, "{name}: {told:?}");
        assert_eq!(told[0]["approved"], false, "{name}: {told:?}");
        assert_eq!(
            told[0]["error"],
            yes::NAME_ONLY_NOT_TAKEN,
            "{name}: {told:?}"
        );
        assert!(yes::waits(&id), "{name}: the wait ended");
        // Its own round is not forced, and its note gives no approve_id.
        assert!(!yes::approve_forced_now(Instant::now()), "{name}");
        let note = yes::waiting_note(1, Instant::now()).unwrap();
        assert!(!note.contains(&id), "{name}: {note}");
        // A no or unclear in it is no answer either.
        for answer in ["no", "unclear"] {
            let told = yes::answered(&id, 1, Some(answer)).unwrap();
            assert_eq!(told["error"], yes::NAME_ONLY_NOT_TAKEN, "{name}: {told}");
            assert!(yes::waits(&id), "{name}: {answer} ended the wait");
        }
        // The lamp answers the call; the yes on the next real line still
        // puts the code on.
        yes::lamp_said(1, "Yes? I'm here.");
        let (told, on) = went(
            "Yes, turn it on.",
            2,
            vec![answers(&id, "yes"), said("It's on now.")],
        );
        assert_eq!(on, 1, "{name}: {told:?}");
        assert_eq!(told[0]["approved"], true, "{name}: {told:?}");
        assert!(!yes::waits(&id), "{name}");
    }
}

/// A line with words of the person's own is judged as before: its yes is
/// taken in it.
#[test]
fn a_line_with_words_still_carries_the_yes() {
    let id = said_back();
    let (told, on) = went("Yes.", 1, vec![answers(&id, "yes"), said("It's on now.")]);
    assert_eq!(on, 1, "{told:?}");
    assert_eq!(told[0]["approved"], true, "{told:?}");
}
