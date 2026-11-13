//! The person's line right after a skill's task said back unasked is made
//! to call `make_skill` with the wait's `approve_id` and their answer
//! (#6835 gap 2, the first builder's option (b) on #6811; Seventh's
//! row-photo2 on lamp-52e6, 13 Nov): the model called photo_booth itself
//! after an offer, the read-back was said unasked, and the owner's "Yes."
//! got "Oh, sure — … Want me to go ahead with that?" with no call, so no
//! yes was kept. Code decides when the call is made; the model what it
//! answers.
use super::*;
use crate::skill_task_yes::{self as task_yes, TaskAsks};
use crate::skill_write_yes as yes;

/// photo-booth on the lamp, its do.task with no yes, three grants.
fn booth() -> TaskAsks {
    TaskAsks {
        name: "photo-booth".into(),
        sha256: "ab".repeat(32),
        grants: vec!["camera".into(), "light".into(), "speak".into()],
        task: "Take a photo and say it's saved.".into(),
        package: None,
        from: "your computer".into(),
        approved: false,
    }
}

/// The trigger fired with no yes: the read-back said unasked, its wait
/// begun in written request 0, and the lamp's words of it (`said`, none
/// yet when `None`) kept as played. The wait's id.
fn said_back_unasked(said: Option<&str>) -> String {
    yes::clear_for_test();
    task_yes::set_on_here(Some(true));
    let mut kept = None;
    let note = task_yes::asked_aloud(
        "photo-booth",
        task_yes::Here {
            quiet_hours: false,
            owner_sees_alone: true,
        },
        |_| Ok(Some(booth())),
        |file| {
            kept = Some(file.clone());
            Ok(())
        },
        1_000,
    )
    .unwrap();
    let told = task_yes::asked_in(0, note, || kept, Instant::now(), 2_000);
    if let Some(said) = said {
        yes::lamp_said(0, said);
    }
    told.split("approve_id \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap()
        .to_owned()
}

/// The read-back as the lamp says it whole.
fn whole() -> String {
    task_yes::line(&booth())
}

/// The `tool_choice` the second route's chat sends for the person's line
/// `heard` now, with `make_skill` declared and the force on, as the worker
/// gives it.
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

/// One turn as it went: every call the runner ran, with its result, and
/// whether the approval itself (lamp-control's record) ran.
struct Went {
    ran: Vec<(String, Value, Value)>,
    approved: usize,
}

/// `heard` asked in request 1, the model's rounds `rounds`; the runner's
/// `make_skill` with an `approve_id` is answered as the voice's own is: a
/// no or unclear answer first ([`yes::answered`]), else the approval with
/// every guard, lamp-control keeping it.
fn went(heard: &str, rounds: Vec<Vec<net::Result<ChatEvent>>>) -> Went {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new(rounds.into()),
        ..Script::default()
    });
    let ran: Arc<Mutex<Vec<(String, Value, Value)>>> = Arc::default();
    let approved: Arc<Mutex<usize>> = Arc::default();
    let (seen, kept) = (ran.clone(), approved.clone());
    let tools: ToolRunner = Arc::new(move |request, heard, name, args| {
        let answer = match args[yes::APPROVE_ID].as_str() {
            Some(id) if name == crate::made_skill::MAKE => {
                yes::answered(id, request, args[yes::ANSWER].as_str()).unwrap_or_else(|| {
                    yes::approve(id, request, heard, true, Instant::now(), |_| {
                        *kept.lock().unwrap() += 1;
                        Ok(json!({"kept": true}))
                    })
                })
            }
            _ => json!({"next": "Say: done."}),
        };
        seen.lock().unwrap().push((name, args, answer.clone()));
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
    let ran = ran.lock().unwrap().clone();
    let approved = *approved.lock().unwrap();
    Went { ran, approved }
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

/// photo2 yes: the self-started call's read-back said unasked, whole and
/// uncut; the owner's "Yes." has its round 1 forced to `make_skill`, whose
/// form takes the answer, and the yes is kept through the approval with
/// every guard. Nothing waits after it.
#[test]
fn photo2_a_yes_after_a_task_said_back_unasked_is_forced_to_the_approval_and_kept() {
    let id = said_back_unasked(Some(&whole()));
    assert_eq!(forced("Yes."), make_skill_forced());
    let form = crate::make_skill_local::tool().parameters.unwrap();
    assert_eq!(
        form["properties"][yes::ANSWER]["enum"],
        json!(["yes", "no", "unclear"])
    );
    let went = went(
        "Yes.",
        vec![answers(&id, "yes"), said("Okay, next time you ask.")],
    );
    assert_eq!(went.ran.len(), 1, "{:?}", went.ran);
    assert_eq!(went.ran[0].2["approved"], true, "{:?}", went.ran);
    assert_eq!(went.approved, 1);
    assert!(!yes::awaited(Instant::now()));
    task_yes::set_on_here(None);
}

/// photo2 no: forced all the same; the model's answer is no, so nothing is
/// kept and the wait ends.
#[test]
fn photo2_a_no_is_forced_too_and_keeps_nothing() {
    let id = said_back_unasked(Some(&whole()));
    assert_eq!(forced("No."), make_skill_forced());
    let went = went("No.", vec![answers(&id, "no"), said("Okay, it stays off.")]);
    assert_eq!(went.ran.len(), 1, "{:?}", went.ran);
    assert_eq!(went.ran[0].2["approved"], false, "{:?}", went.ran);
    assert_eq!(went.ran[0].2["next"], yes::DECLINED_NEXT);
    assert_eq!(went.approved, 0);
    assert!(!yes::awaited(Instant::now()));
    task_yes::set_on_here(None);
}

/// A forced approval call that leaves the answer out is read as unclear for
/// a skill's task: nothing kept, asked once more, never a yes nobody gave.
#[test]
fn photo2_a_forced_call_with_no_answer_keeps_nothing_and_asks_once() {
    let id = said_back_unasked(Some(&whole()));
    assert_eq!(forced("No."), make_skill_forced());
    let bare = vec![
        Ok(ChatEvent::ToolCalls(vec![call(
            "c1",
            crate::made_skill::MAKE,
            json!({ yes::APPROVE_ID: id }),
        )])),
        Ok(ChatEvent::Done),
    ];
    let went = went("No.", vec![bare, said("Sorry: may it go ahead?")]);
    assert_eq!(went.ran.len(), 1, "{:?}", went.ran);
    assert_eq!(went.ran[0].2["approved"], false, "{:?}", went.ran);
    assert_eq!(went.ran[0].2["next"], yes::UNCLEAR_NEXT, "{:?}", went.ran);
    assert_eq!(went.approved, 0);
    assert!(yes::awaited(Instant::now()));
    task_yes::set_on_here(None);
}

/// photo2 unclear: nothing kept, asked once more; the line after that ask
/// is not forced (the newest answer is the ask, no say-back), and a second
/// unclear keeps nothing and ends the wait: asked once, never twice.
#[test]
fn photo2_unclear_keeps_nothing_and_asks_once_more_only_once() {
    let id = said_back_unasked(Some(&whole()));
    assert_eq!(forced("Hmm, the what?"), make_skill_forced());
    let went = went(
        "Hmm, the what?",
        vec![answers(&id, "unclear"), said("Sorry: may it go ahead?")],
    );
    assert_eq!(went.ran[0].2["next"], yes::UNCLEAR_NEXT, "{:?}", went.ran);
    assert_eq!(went.approved, 0);
    assert!(yes::awaited(Instant::now()));
    yes::lamp_said(1, "Sorry: may it go ahead?");
    assert_eq!(forced("Maybe."), None);
    let again = yes::answered(&id, 2, Some("unclear")).unwrap();
    assert_eq!(again["next"], yes::DECLINED_NEXT);
    assert!(!yes::awaited(Instant::now()));
    task_yes::set_on_here(None);
}

/// Never forced: a cut read-back, a partial one, the say-back's own written
/// turn (nothing played yet), nothing waiting, and the line after the one
/// that was forced (one round only). With nothing forced, nothing is kept.
#[test]
fn photo2_no_force_after_a_cut_or_partial_read_back_or_with_nothing_waiting() {
    let id = said_back_unasked(Some(&whole()));
    yes::lamp_cut(0);
    assert_eq!(forced("Yes."), None);
    let went = went(
        "Yes.",
        vec![said("Oh, sure. Want me to go ahead with that?")],
    );
    assert!(went.ran.is_empty(), "{:?}", went.ran);
    assert!(yes::waits(&id));
    said_back_unasked(Some("Photo booth hands one task to the lamp's agent."));
    assert_eq!(forced("Yes."), None);
    said_back_unasked(None);
    assert_eq!(forced("(Nobody has spoken.)"), None);
    said_back_unasked(Some(&whole()));
    yes::lamp_said(1, "Okay.");
    assert_eq!(forced("Yes."), None, "one round only");
    yes::clear_for_test();
    assert_eq!(forced("Yes."), None);
    task_yes::set_on_here(None);
}
