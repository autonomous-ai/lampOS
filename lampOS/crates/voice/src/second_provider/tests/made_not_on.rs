//! `LAMP_MADE_NOT_ON_TOLD` on the second route (#7270): on lamp-9adf a
//! voice make by the lamp's agent took about four minutes, its say-back
//! ended "It waits for you to agree before it runs any code.", and the
//! person's "Yeah." came over 200 s after those words, with no conversation
//! going on. The model was shown no skill waiting, made no call, and said
//! "The skill is on the lamp. …": nothing was on. With the switch, past the
//! wait the model is told the skill is not on, each round's reply is held
//! and judged by the chat model (a fake judge here: the script's next chat,
//! asked with no function), and one judged to say it is on is said as the
//! fixed line. A reply judged not to is said as it came; no judge in time
//! says it, then the fixed line. A yes that ran, a turn with nothing
//! waiting, and the switch off are as before.
use super::*;

/// The made skill, as `make_skill` keeps it.
const SKILL: &str = "newest-email-subject";

/// The reply on lamp-9adf.
const CLAIM: &str = "The skill is on the lamp. Next time you sit at your desk, the lamp will read your newest email and tell you its subject.";

/// Heron's replies on #7284 that a word list let through, each a claim
/// the judge holds.
const CLAIMS: [&str; 10] = [
    "I've switched it on.",
    "Got it, it's turned on.",
    "It's on now.",
    "Turned it on for you.",
    "All set!",
    "Your skill is good to go.",
    "It's working now.",
    "Done, newest email subject will now run when you sit down.",
    "Your skill is set up and will run once you're at the desk.",
    "Kỹ năng đã bật rồi.",
];

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
    /// The replies the judge was asked of, each told no function.
    judged: Vec<String>,
}

/// `heard` in request 1, answered with the model's `rounds` (the judge's
/// answers among them, in the order asked), each chat begun after its
/// `delays` entry, the switch `on`. A `make_skill` with an `approve_id`
/// takes the wait, as a yes that puts the skill on does.
fn went(
    on: bool,
    heard: &str,
    rounds: Vec<Vec<net::Result<ChatEvent>>>,
    delays: Vec<Duration>,
) -> Went {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new(rounds.into()),
        round_after: Mutex::new(delays.into()),
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
            turn_within(&mut engine, Duration::from_secs(5)).await
        });
    let asked = script.asked.lock().unwrap();
    let words = |message: &Message| {
        message
            .content
            .as_ref()
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let notes = asked
        .iter()
        .flatten()
        .filter_map(words)
        .filter(|content| content.starts_with("(For you to know"))
        .collect();
    let judged = asked
        .iter()
        .filter(|messages| {
            messages
                .first()
                .and_then(words)
                .is_some_and(|system| system.starts_with("You check one reply"))
        })
        .filter_map(|messages| messages.get(1).and_then(words))
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
        judged,
    };
    assert!(
        script
            .told_only
            .lock()
            .unwrap()
            .iter()
            .all(|only| only.is_empty()),
        "the judge is told no function"
    );
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
    went.notes
        .iter()
        .any(|note| note.contains("newest email subject that the lamp made is NOT on"))
}

/// lamp-9adf, 16 Nov 13:17: "Yeah." long after the say-back, no
/// conversation going on, `calls=none`, and "The skill is on the lamp. …".
/// With the switch the model is told the skill is not on, the judge asked
/// of the whole reply says yes, and the fixed line is said in its place.
/// Off, as on the lamp: no note, no judge, and the claim said.
#[test]
fn a_late_yes_with_no_call_is_told_not_on_and_its_claim_is_replaced() {
    waits(Duration::from_secs(220));
    let went_on = went(true, "Yeah.", vec![saying(CLAIM), saying("yes")], vec![]);
    assert_eq!(
        went_on.said,
        said(&[crate::made_not_on::NOT_ON_LATE]),
        "{went_on:?}"
    );
    assert!(told_not_on(&went_on), "{:?}", went_on.notes);
    assert_eq!(went_on.judged, [CLAIM]);
    assert!(went_on.ran.is_empty());
    waits(Duration::from_secs(220));
    let went_off = went(false, "Yeah.", vec![saying(CLAIM)], vec![]);
    assert_eq!(
        went_off.said,
        said(&[
            "The skill is on the lamp.",
            " Next time you sit at your desk, the lamp will read your newest email and tell you its subject."
        ])
    );
    assert!(went_off.notes.is_empty(), "{:?}", went_off.notes);
    assert!(went_off.judged.is_empty());
}

/// Heron's HOLD on #7284: every way of saying it, Vietnamese too, and on a
/// line that is more than a plain yes, goes to the judge whole; each the
/// judge calls a claim is said as the fixed line, within the wait.
#[test]
fn every_claim_the_judge_holds_is_said_not_on_yet() {
    for heard in ["Yes.", "Yes, turn it on."] {
        for claim in CLAIMS {
            waits(Duration::from_secs(10));
            let went = went(true, heard, vec![saying(claim), saying("Yes.")], vec![]);
            assert_eq!(
                went.said,
                said(&[crate::made_not_on::NOT_ON_YET]),
                "{claim}"
            );
            assert_eq!(went.judged, [claim]);
            assert!(!told_not_on(&went));
        }
    }
}

/// A reply the judge calls no claim is said as it came.
#[test]
fn a_reply_judged_no_claim_is_said() {
    waits(Duration::from_secs(10));
    let reply = "Sure. Before it goes on, I need your yes to what its code may use.";
    let went = went(true, "Okay.", vec![saying(reply), saying("No")], vec![]);
    assert_eq!(went.said, said(&[reply]));
    assert_eq!(went.judged, [reply]);
}

/// No judge in time (its chat begins past `JUDGE_WITHIN`): the reply is
/// said, then the fixed line, which is true, since the skill isn't on.
#[test]
fn a_judge_past_its_time_says_the_reply_then_the_fixed_line() {
    waits(Duration::from_secs(10));
    let went = went(
        true,
        "Yes.",
        vec![saying("It's on now!"), saying("yes")],
        vec![
            Duration::ZERO,
            crate::made_not_on::JUDGE_WITHIN + Duration::from_millis(500),
        ],
    );
    assert_eq!(
        went.said,
        said(&[
            "It's on now!",
            &format!(" {}", crate::made_not_on::NOT_ON_YET)
        ])
    );
}

/// A yes that ran: "It's on now." said before its call, in the same round,
/// is let go unsaid, and the next round's, after the yes put it on, is said
/// as it came. No judge, no not-on note, no fixed line.
#[test]
fn a_real_yes_still_puts_it_on_with_no_line() {
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
        vec![],
    );
    assert_eq!(
        went.said,
        said(&["It's on now, and it runs the next time you sit at your desk."])
    );
    assert_eq!(went.ran, [crate::made_skill::MAKE.to_owned()]);
    assert!(went.judged.is_empty());
    assert!(!told_not_on(&went));
}

/// With nothing waiting, "It's on." to a yes is said as it came: no judge,
/// no note.
#[test]
fn with_no_skill_waiting_nothing_changes() {
    crate::skill_write_yes::clear_for_test();
    let went = went(true, "Yes.", vec![saying("It's on.")], vec![]);
    assert_eq!(went.said, said(&["It's on."]));
    assert!(went.notes.is_empty(), "{:?}", went.notes);
    assert!(went.judged.is_empty());
}
