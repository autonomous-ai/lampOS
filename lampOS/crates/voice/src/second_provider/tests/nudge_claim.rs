//! A wellbeing nudge said stopped, snoozed or turned off with no
//! `wellbeing_nudges` that did it (#7878: on lamp-06c1 at 14432, 19 Nov
//! 10:38, "Lamp, stop the water nudges for today." got no call and "I've
//! turned off the water nudge for today.", and the nudge came again). With
//! `LAMP_ACTION_CLAIM_CHECKED` and `LAMP_NUDGE_CLAIM_CHECKED`
//! ([`Engine::checking_nudge_claims`]), such a reply is not said: the model
//! is asked again once with [`crate::action_claim::NOTE`], and a second
//! miss is said as [`crate::action_claim::NOT_DONE_LINE`]. A real stop is
//! said as before, and off, nothing changes.
use super::*;
use crate::action_claim::{NOT_DONE_LINE, NOTE, NUDGE_TOOL};

/// One turn as it went.
#[derive(Debug, Default)]
struct Went {
    /// What was said, as `said:` labels.
    said: Vec<String>,
    /// The functions called, by name.
    called: Vec<String>,
    /// The notes the rounds were asked with, each once.
    notes: Vec<String>,
}

/// One turn heard as `heard`, its chat rounds `rounds`, with the nudge
/// claims checked or not (`nudge`). Every call answers `ok`.
fn went(heard: &str, nudge: bool, rounds: Vec<Vec<net::Result<ChatEvent>>>) -> Went {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new(rounds.into()),
        ..Script::default()
    });
    let called: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = called.clone();
    let tools: ToolRunner = Arc::new(move |_, _, name, _| {
        seen.lock().unwrap().push(name.to_owned());
        Box::pin(async move { json!({"ok": true, "snoozed": "sip-of-water"}) })
    });
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(move || Told {
            text: "SYSTEM".into(),
            after: None,
            history: Vec::new(),
        }),
        tools,
        Arc::new(Instant::now),
    )
    .announcing(false)
    .checking_action_claims(true)
    .checking_nudge_claims(nudge);
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
        called: called.lock().unwrap().clone(),
        notes,
    }
}

fn saying(words: &str) -> Vec<net::Result<ChatEvent>> {
    vec![text(words), Ok(ChatEvent::Done)]
}

/// The model's `wellbeing_nudges` call, by its own name.
fn stopping() -> Vec<net::Result<ChatEvent>> {
    vec![
        Ok(ChatEvent::ToolCalls(vec![call(
            "c1",
            NUDGE_TOOL,
            json!({"snooze": true}),
        )])),
        Ok(ChatEvent::Done),
    ]
}

fn said(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| format!("said:{line}")).collect()
}

/// Whether nothing said claims the nudge off.
fn never_claimed(went: &Went) -> bool {
    !went
        .said
        .iter()
        .any(|said| said.to_lowercase().contains("turned off the water nudge"))
}

const HEARD: &str = "Lamp, stop the water nudges for today.";
const CLAIMED: &str = "I've turned off the water nudge for today.";

/// lamp-06c1's 10:38 turn: the claim with no call is not said; asked again
/// with the note, the model calls `wellbeing_nudges`, and its own words
/// after the call are said.
#[test]
fn a_nudge_said_off_with_no_call_is_asked_again_and_never_said() {
    let went = went(
        HEARD,
        true,
        vec![
            saying(CLAIMED),
            stopping(),
            saying("Done, the water nudge is snoozed until tomorrow morning."),
        ],
    );
    assert!(never_claimed(&went), "{went:?}");
    assert_eq!(went.notes, [NOTE], "{went:?}");
    assert_eq!(went.called, [NUDGE_TOOL], "{went:?}");
    assert!(
        went.said
            .iter()
            .any(|said| said.contains("snoozed until tomorrow morning")),
        "{went:?}"
    );
}

/// The same claim twice with no call: said as not done, never as done.
#[test]
fn a_nudge_said_off_twice_with_no_call_is_said_not_done() {
    let went = went(HEARD, true, vec![saying(CLAIMED), saying(CLAIMED)]);
    assert!(never_claimed(&went), "{went:?}");
    assert_eq!(went.said, said(&[NOT_DONE_LINE]), "{went:?}");
    assert!(went.called.is_empty(), "{went:?}");
}

/// A real stop: `wellbeing_nudges` in round 1, its claim said as it is, with
/// no note.
#[test]
fn a_nudge_stopped_by_its_call_is_said_as_it_is() {
    let went = went(
        HEARD,
        true,
        vec![
            stopping(),
            saying("I've turned off the water nudge for today."),
        ],
    );
    assert_eq!(went.said, said(&[CLAIMED]), "{went:?}");
    assert!(went.notes.is_empty(), "{went:?}");
    assert_eq!(went.called, [NUDGE_TOOL], "{went:?}");
}

/// Off (`LAMP_NUDGE_CLAIM_CHECKED` unset): the claim is said as before,
/// exactly the fault, so the switch is what changes it.
#[test]
fn off_the_claim_is_said_as_before() {
    let went = went(HEARD, false, vec![saying(CLAIMED)]);
    assert_eq!(went.said, said(&[CLAIMED]), "{went:?}");
    assert!(went.notes.is_empty(), "{went:?}");
}

/// A reply about the nudges that claims nothing is untouched: an offer, a
/// question back, and how the nudge works.
#[test]
fn a_reply_that_claims_no_stop_is_untouched() {
    for reply in [
        "Do you want me to turn off the water nudge for today?",
        "The water nudge comes after about half an hour at the desk.",
        "I can snooze the water nudge until tomorrow if you like.",
    ] {
        let went = went("Lamp, what's the water nudge?", true, vec![saying(reply)]);
        assert_eq!(went.said, said(&[reply]), "{reply}: {went:?}");
        assert!(went.notes.is_empty(), "{reply}: {went:?}");
    }
}
