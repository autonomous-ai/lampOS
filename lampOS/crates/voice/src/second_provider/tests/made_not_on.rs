//! `LAMP_MADE_NOT_ON_TOLD` on the second route (#7270): on lamp-9adf a
//! voice make by the lamp's agent took about four minutes, its say-back
//! ended "It waits for you to agree before it runs any code.", and the
//! person's "Yeah." came over 200 s after those words, with no conversation
//! going on. The model was shown no skill waiting, made no call, and said
//! "The skill is on the lamp. …": nothing was on. With the switch, past the
//! wait the model is told the skill is not on, and a reply that says it is
//! on is said as the fixed line. A yes that ran, a turn with nothing
//! waiting, and the switch off are as before.
use super::*;

/// The made skill, as `make_skill` keeps it.
const SKILL: &str = "newest-email-subject";

/// The reply on lamp-9adf.
const CLAIM: &str = "The skill is on the lamp. Next time you sit at your desk, the lamp will read your newest email and tell you its subject.";

/// What the skill's say-back must have said whole.
const SAY_BACK: &str = " The lamp's model wrote its code, and that code may use your Gmail. It \
                        waits for you to agree before it runs any code.";

/// The skill's code waits for its yes, its wait begun `ago`, made and said
/// back whole in a request before the test's line (request 1): its
/// `approve_id`.
fn waits(ago: Duration) -> String {
    crate::skill_write_yes::clear_for_test();
    let id = crate::skill_write_yes::wait_for_yes(
        crate::skill_write_yes::Asks {
            skill: SKILL.into(),
            sha256: "ab".repeat(32),
            writes: Vec::new(),
            say_back: SAY_BACK.into(),
            grants: Some(vec!["gmail-read".into()]),
            code: true,
            remove: false,
        },
        0,
        Instant::now()
            .checked_sub(ago)
            .expect("the clock is that old"),
    );
    crate::skill_write_yes::lamp_said(0, &format!("I made newest email subject:{SAY_BACK}"));
    id
}

/// One turn as it went.
#[derive(Debug)]
struct Went {
    /// What was said, as `said:` labels.
    said: Vec<String>,
    /// The notes the rounds were asked with.
    notes: Vec<String>,
    /// The calls run, by name.
    ran: Vec<String>,
}

/// `heard` in request 1, answered with the model's `rounds`, the switch
/// `on`. A `make_skill` with an `approve_id` takes the wait, as a yes that
/// puts the skill on does.
fn went(on: bool, heard: &str, rounds: Vec<Vec<net::Result<ChatEvent>>>) -> Went {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new(rounds.into()),
        ..Script::default()
    });
    let ran: Calls = Arc::default();
    let seen = ran.clone();
    let tools: ToolRunner = Arc::new(move |_, heard, name, args| {
        let answer = match args[crate::skill_write_yes::APPROVE_ID].as_str() {
            Some(id) if name == crate::made_skill::MAKE => {
                assert!(crate::skill_write_yes::drop_wait(id), "its wait");
                json!({"approved": true, "on": true, "next": crate::skill_write_yes::CODE_APPROVED_NEXT})
            }
            _ => json!({"ok": true}),
        };
        seen.lock().unwrap().push((name, args, heard));
        Box::pin(async move { answer })
    });
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(Instant::now),
    )
    .announcing(false)
    .telling_made_not_on(on);
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
    let asked = script.asked.lock().unwrap();
    let notes = asked
        .iter()
        .flatten()
        .filter_map(|message| message.content.as_ref().and_then(Value::as_str))
        .filter(|content| content.starts_with("(For you to know"))
        .map(str::to_owned)
        .collect();
    let went = Went {
        said: labels
            .into_iter()
            .filter(|label| label.starts_with("said:"))
            .collect(),
        notes,
        ran: ran
            .lock()
            .unwrap()
            .iter()
            .map(|(name, ..)| name.clone())
            .collect(),
    };
    crate::skill_write_yes::clear_for_test();
    went
}

fn saying(words: &str) -> Vec<net::Result<ChatEvent>> {
    vec![text(words), Ok(ChatEvent::Done)]
}

fn said(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| format!("said:{line}")).collect()
}

fn told_not_on(went: &Went) -> bool {
    went.notes.iter().any(|note| {
        note.contains(&format!(
            "{} that the lamp made is NOT on",
            "newest email subject"
        ))
    })
}

/// lamp-9adf, 16 Nov 13:17: "Yeah." long after the say-back, no
/// conversation going on, `calls=none`, and "The skill is on the lamp. …".
/// With the switch the model is told the skill is not on, and its claim is
/// said as the fixed line, the rest of its reply with it. Off, as on the
/// lamp: no note, and the claim said.
#[test]
fn a_late_yes_with_no_call_is_told_not_on_and_its_claim_is_replaced() {
    waits(Duration::from_secs(220));
    let went_on = went(true, "Yeah.", vec![saying(CLAIM)]);
    assert_eq!(
        went_on.said,
        said(&[crate::made_not_on::NOT_ON_LATE]),
        "{went_on:?}"
    );
    assert!(told_not_on(&went_on), "{:?}", went_on.notes);
    assert!(went_on.ran.is_empty());
    waits(Duration::from_secs(220));
    let went_off = went(false, "Yeah.", vec![saying(CLAIM)]);
    assert_eq!(
        went_off.said,
        said(&[
            "The skill is on the lamp.",
            " Next time you sit at your desk, the lamp will read your newest email and tell you its subject."
        ])
    );
    assert!(went_off.notes.is_empty(), "{:?}", went_off.notes);
}

/// Within the wait, a yes the model answered with words alone ("It's on
/// now!", no call) is said as the line asking for the yes again; the wait
/// goes on, its note as before.
#[test]
fn a_yes_in_the_wait_answered_with_no_call_is_said_not_on_yet() {
    let id = waits(Duration::from_secs(10));
    let went = went(true, "Yes.", vec![saying("It's on now!")]);
    assert_eq!(went.said, said(&[crate::made_not_on::NOT_ON_YET]));
    assert!(
        went.notes.iter().any(|note| note.contains(&id)),
        "{:?}",
        went.notes
    );
    assert!(!told_not_on(&went));
}

/// A yes that ran: "It's on now." said before its call, in the same round,
/// is let go unsaid, and the next round's, after the yes put it on, is said
/// as it came. No not-on note, no fixed line.
#[test]
fn a_real_yes_still_puts_it_on_with_no_note() {
    let id = waits(Duration::from_secs(10));
    let went = went(
        true,
        "Yes.",
        vec![
            vec![
                text("It's on now."),
                Ok(ChatEvent::ToolCalls(vec![call(
                    "c1",
                    crate::made_skill::MAKE,
                    json!({ crate::skill_write_yes::APPROVE_ID: id }),
                )])),
                Ok(ChatEvent::Done),
            ],
            saying("It's on now, and it runs the next time you sit at your desk."),
        ],
    );
    assert_eq!(
        went.said,
        said(&["It's on now, and it runs the next time you sit at your desk."])
    );
    assert_eq!(went.ran, [crate::made_skill::MAKE.to_owned()]);
    assert!(!told_not_on(&went));
}

/// With nothing waiting, "It's on." to a yes is said as it came, and no note
/// is told.
#[test]
fn with_no_skill_waiting_nothing_changes() {
    crate::skill_write_yes::clear_for_test();
    let went = went(true, "Yes.", vec![saying("It's on.")]);
    assert_eq!(went.said, said(&["It's on."]));
    assert!(went.notes.is_empty(), "{:?}", went.notes);
}
