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
    said_back_unasked_as(booth(), said)
}

/// [`said_back_unasked`] for the skill `task`.
fn said_back_unasked_as(task: TaskAsks, said: Option<&str>) -> String {
    yes::clear_for_test();
    task_yes::set_on_here(Some(true));
    let mut kept = None;
    let name = task.name.clone();
    let note = task_yes::asked_aloud(
        &name,
        task_yes::Here {
            quiet_hours: false,
            owner_sees_alone: true,
            asked_by_voice: false,
        },
        |_| Ok(Some(task.clone())),
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
    wired(heard).get("tool_choice").cloned()
}

/// The request body the second route's chat sends for `heard` now, as
/// [`forced`] builds it.
fn wired(heard: &str) -> Value {
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
    chat.wire(&line).2
}

/// The `make_skill` parameters the request for `heard` declares.
fn make_form(heard: &str) -> Value {
    let body = wired(heard);
    body["tools"]
        .as_array()
        .and_then(|tools| {
            tools
                .iter()
                .find(|tool| tool["function"]["name"] == crate::made_skill::MAKE)
        })
        .map(|tool| tool["function"]["parameters"].clone())
        .unwrap()
}

/// #6921 P3: the forced approval round is told only `approve_id`,
/// `answer` and `named` (#7545), all required, so the forced call carries no make or edit
/// beside the answer; the answer's choices are kept. Without the force
/// the form is told whole.
#[test]
fn photo2_the_forced_round_is_told_only_the_approval_fields() {
    said_back_unasked(Some(&whole()));
    let form = make_form("Yes.");
    let mut told: Vec<&str> = form["properties"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    told.sort_unstable();
    assert_eq!(told, [yes::ANSWER, yes::APPROVE_ID, yes::NAMED]);
    assert_eq!(form["required"], json!(yes::APPROVE_FIELDS));
    assert_eq!(form["properties"][yes::ANSWER]["enum"], json!(yes::ANSWERS));
    yes::clear_for_test();
    let whole_form = make_form("Yes.");
    assert!(whole_form["properties"].as_object().unwrap().len() > 2);
    task_yes::set_on_here(None);
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
            Some(id) if name == crate::made_skill::MAKE => yes::answered(
                id,
                request,
                args[yes::ANSWER].as_str(),
                args[yes::NAMED].as_str(),
            )
            .unwrap_or_else(|| {
                yes::approve(id, request, heard, true, Instant::now(), |_| {
                    *kept.lock().unwrap() += 1;
                    Ok(json!({"kept": true}))
                })
            }),
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
            json!({ yes::APPROVE_ID: id, yes::ANSWER: answer, yes::NAMED: yes::NAMED_NONE }),
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
        json!(["yes", "no", "unclear", "new_ask"])
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
    let again = yes::answered(&id, 2, Some("unclear"), None).unwrap();
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

/// page-title on the lamp (lamp-9adf, 18 Nov), its do.task with no yes.
fn page_title() -> TaskAsks {
    TaskAsks {
        name: "page-title".into(),
        sha256: "cd".repeat(32),
        grants: vec!["light".into(), "speak".into()],
        task: "Read the open page's title aloud.".into(),
        package: None,
        from: "your computer".into(),
        approved: false,
    }
}

/// The forced round 1: `make_skill` with the wait's id, `answer`, and the
/// skill the model named.
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

/// #7634 (lamp-9adf, 18 Nov 07:20:43): page-title's task said back and
/// waiting, then "Lamp, use my look title skill.", a line naming another
/// skill, read by the model as a yes naming page-title. Never its yes:
/// nothing is kept, the wait is let go as a new ask is, and the line is
/// answered as its own ask, the look title skill used in the next round.
#[test]
fn a_line_naming_another_skill_is_never_the_yes_to_a_tasks_wait() {
    for named in ["page-title", yes::NAMED_NONE] {
        let id = said_back_unasked_as(page_title(), Some(&task_yes::line(&page_title())));
        let heard = "Lamp, use my look title skill.";
        assert_eq!(forced(heard), make_skill_forced());
        let use_it = vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "c2",
                crate::use_skill::NAME,
                json!({ "skill": "look-title" }),
            )])),
            Ok(ChatEvent::Done),
        ];
        let went = went(
            heard,
            vec![
                answers_naming(&id, "yes", named),
                use_it,
                said("Page title stays as it was. Here's look title."),
            ],
        );
        assert_eq!(went.approved, 0, "{named}: {:?}", went.ran);
        assert_eq!(went.ran[0].2["approved"], false, "{named}: {:?}", went.ran);
        assert_eq!(
            went.ran[0].2["left"], "page-title",
            "{named}: {:?}",
            went.ran
        );
        assert_eq!(
            went.ran[0].2["next"],
            yes::new_ask_next("page title"),
            "{named}: {:?}",
            went.ran
        );
        assert_eq!(went.ran.len(), 2, "{named}: {:?}", went.ran);
        assert_eq!(
            went.ran[1].0,
            crate::use_skill::NAME,
            "{named}: {:?}",
            went.ran
        );
        assert!(!yes::awaited(Instant::now()), "{named}");
        assert!(!yes::waits(&id), "{heard} {named}");
    }
    yes::set_installed_here(None);
    task_yes::set_on_here(None);
}

/// The skills on the lamp beside page-title, for #7634's lines.
fn on_the_lamp() -> Vec<String> {
    ["page-title", "page-title-backup", "look-title", "weather"]
        .map(String::from)
        .to_vec()
}

/// #7634: "yes" with another skill's name after it ("Yes, and use my
/// weather skill.", plural too), or said after "skill" ("yes, run skill
/// look title"), is no yes to page-title's task either, whatever the model
/// named.
#[test]
fn a_yes_with_another_skills_name_is_not_a_tasks_yes() {
    for (heard, named) in [
        ("Yes, and use my weather skill.", "page-title"),
        ("Yes, and use my weather skill.", yes::NAMED_NONE),
        ("Yes, and use my weather skill.", "weather"),
        ("Yes, and use my weather skills.", "page-title"),
        ("yes, run skill look title", "page-title"),
        ("yes, use the skill look title", yes::NAMED_NONE),
        // The longest installed name wins (the re-read's P1 on #7634).
        ("yes, use skill page title backup", "page-title"),
        ("yes, use my backup page title skill", "page-title"),
    ] {
        let id = said_back_unasked_as(page_title(), Some(&task_yes::line(&page_title())));
        yes::set_installed_here(Some(on_the_lamp()));
        let went = went(
            heard,
            vec![
                answers_naming(&id, "yes", named),
                said("Okay, the weather."),
            ],
        );
        assert_eq!(went.approved, 0, "{named}: {:?}", went.ran);
        assert_eq!(went.ran[0].2["approved"], false, "{named}: {:?}", went.ran);
        assert!(!yes::waits(&id), "{named}");
    }
    task_yes::set_on_here(None);
}

/// #7634: a yes that names its own skill, or none, is still the yes:
/// "yes, use my page title", "Yes, use my page title skill." and "Yes.",
/// and so is one with a word after "skill" that names no skill on the
/// lamp ("yes, run the skill again").
#[test]
fn a_yes_naming_its_own_skill_or_none_is_the_tasks_yes() {
    for (heard, named) in [
        ("yes, use my page title", "page-title"),
        ("Yes, use my page title skill.", "page title"),
        ("Yes.", yes::NAMED_NONE),
        ("Yes.", "page-title"),
        ("Có, dùng skill page title", "page title"),
        ("yes, use my page title skills", "page title"),
        // A word after "skill" naming no installed skill, or a filler
        // before it, is no other skill (the re-read's P2 on #7634).
        ("yes the skill thanks", "page-title"),
        ("yes, run the skill again", yes::NAMED_NONE),
        ("Có, chạy skill đi", yes::NAMED_NONE),
    ] {
        let id = said_back_unasked_as(page_title(), Some(&task_yes::line(&page_title())));
        yes::set_installed_here(Some(on_the_lamp()));
        assert_eq!(forced(heard), make_skill_forced(), "{heard}");
        let went = went(
            heard,
            vec![
                answers_naming(&id, "yes", named),
                said("Okay, next time you ask."),
            ],
        );
        assert_eq!(went.ran.len(), 1, "{heard}: {:?}", went.ran);
        assert_eq!(went.ran[0].2["approved"], true, "{heard}: {:?}", went.ran);
        assert_eq!(went.approved, 1, "{heard}");
        assert!(!yes::awaited(Instant::now()), "{heard}");
    }
    yes::set_installed_here(None);
    task_yes::set_on_here(None);
}

/// #7634 with a guest: the line naming another skill is no yes whoever
/// says it (read before the owner check, as a new ask is); a guest's line
/// naming page title's own skill is a yes in words, but never the
/// owner's, so the owner check drops the wait and approves nothing.
#[test]
fn a_guests_line_approves_nothing_either_way() {
    let id = said_back_unasked_as(page_title(), Some(&task_yes::line(&page_title())));
    yes::set_installed_here(Some(on_the_lamp()));
    yes::line_heard(1, false, "Yes, and use my weather skill.");
    let told = yes::answered(&id, 1, Some("yes"), Some("page-title")).unwrap();
    assert_eq!(told["approved"], false, "{told}");
    assert!(!yes::waits(&id));

    let id = said_back_unasked_as(page_title(), Some(&task_yes::line(&page_title())));
    yes::set_installed_here(Some(on_the_lamp()));
    yes::line_heard(1, false, "Yes, use my page title skill.");
    assert_eq!(yes::answered(&id, 1, Some("yes"), Some("page title")), None);
    let refused = crate::make_skill_local::not_owners_yes_refused(&id, || false).unwrap();
    assert_eq!(refused["error"], crate::services::OTHER_VOICE, "{refused}");
    assert!(!yes::waits(&id));
    yes::set_installed_here(None);
    task_yes::set_on_here(None);
}

/// #7634 with a skill's tasks switched off (`LAMP_SKILL_TASKS` not on): no
/// task's wait begins, so a line naming another skill, or a yes, finds
/// nothing waiting and approves nothing.
#[test]
fn with_tasks_off_no_wait_begins_and_no_line_approves() {
    yes::clear_for_test();
    task_yes::set_on_here(Some(false));
    let refused = task_yes::asked_aloud(
        "page-title",
        task_yes::Here {
            quiet_hours: false,
            owner_sees_alone: true,
            asked_by_voice: false,
        },
        |_| Ok(Some(page_title())),
        |_| Ok(()),
        1_000,
    );
    assert_eq!(refused.unwrap_err(), task_yes::OFF);
    yes::line_heard(1, false, "Lamp, use my look title skill.");
    assert!(!yes::awaited(Instant::now()));
    assert_eq!(forced("Yes."), None);
    task_yes::set_on_here(None);
}

/// A skill's task waiting under `name`, as [`page_title`]'s.
fn task_named(name: &str) -> TaskAsks {
    TaskAsks {
        name: name.into(),
        ..page_title()
    }
}

/// The adversarial read's P2 on #7634: a yes said without its comma, as
/// speech-to-text may give it, is still the yes of the skill it names
/// ("okay page title skill", "yeah weather skill", "go ahead with weather
/// skill"), and so is a Vietnamese line naming the skill as its made name
/// is written, without marks ("ừ đèn ngủ skill" for `den-ngu`). "okay
/// timer skill" with weather waiting is no yes to weather.
#[test]
fn a_yes_said_without_its_comma_is_still_its_skills_yes() {
    for (heard, waiting, named) in [
        ("okay page title skill", "page-title", "page title"),
        ("yeah weather skill", "weather", "weather"),
        ("go ahead with weather skill", "weather", "weather"),
        ("ừ skill thời tiết", "thoi-tiet", "thời tiết"),
        ("ừ đèn ngủ skill", "den-ngu", "đèn ngủ"),
    ] {
        let task = task_named(waiting);
        let id = said_back_unasked_as(task.clone(), Some(&task_yes::line(&task)));
        yes::set_installed_here(Some(on_the_lamp()));
        let went = went(
            heard,
            vec![answers_naming(&id, "yes", named), said("Okay.")],
        );
        assert_eq!(went.ran[0].2["approved"], true, "{heard}: {:?}", went.ran);
        assert_eq!(went.approved, 1, "{heard}");
    }
    let task = task_named("weather");
    let id = said_back_unasked_as(task.clone(), Some(&task_yes::line(&task)));
    yes::set_installed_here(Some(on_the_lamp()));
    let went = went(
        "okay timer skill",
        vec![
            answers_naming(&id, "yes", "weather"),
            said("Okay, the timer."),
        ],
    );
    assert_eq!(went.approved, 0, "{:?}", went.ran);
    assert_eq!(went.ran[0].2["approved"], false, "{:?}", went.ran);
    yes::set_installed_here(None);
    task_yes::set_on_here(None);
}
