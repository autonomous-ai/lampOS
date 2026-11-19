//! A keep claimed in a reply's later sentence (#7878: on lamp-e82d at
//! 14432, 19 Nov 09:52, "Actually, I moved my car. It's on Level B1, spot 7
//! now." got no call and "Got it — your car is on Level B1, spot 7 now. That
//! replaces wherever it was before.": its opening claims nothing, so it was
//! said, and nothing was kept). With `LAMP_KEEP_CLAIM_WHOLE`
//! ([`Engine::checking_keep_claims_whole`]), while no `remember` has run a
//! reply is said sentence by sentence, each judged when complete: one that
//! claims a keep or replace is held, with the rest, to the round's end, and
//! with no call it is asked again once with
//! [`crate::remember::KEEP_CLAIMED_UNKEPT`], then said as
//! [`crate::remember::NOT_KEPT_LINE`]. A restatement, a story and a light
//! reply beside its call are said as they are, and off nothing changes.
use super::*;
use crate::remember::{KEEP_CLAIMED_UNKEPT, NOT_KEPT_LINE, REMEMBER};

/// One turn as it went.
#[derive(Debug, Default)]
struct Went {
    /// What was said, as `said:` labels.
    said: Vec<String>,
    /// The functions called, by name.
    called: Vec<String>,
    /// How many chat rounds were asked.
    rounds: usize,
    /// Whether a round was told the keep-claim note.
    noted: bool,
}

fn went(heard: &str, whole: bool, rounds: Vec<Vec<net::Result<ChatEvent>>>) -> Went {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new(rounds.into()),
        ..Script::default()
    });
    let called: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = called.clone();
    let tools: ToolRunner = Arc::new(move |_, _, name, _| {
        seen.lock().unwrap().push(name.to_owned());
        Box::pin(async move { json!({"ok": true, "remembered": true}) })
    });
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(Instant::now),
    )
    .announcing(false)
    .checking_keep_claims_whole(whole);
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
    Went {
        said: labels
            .into_iter()
            .filter(|label| label.starts_with("said:"))
            .collect(),
        called: called.lock().unwrap().clone(),
        rounds: asked.len(),
        noted: asked.iter().any(|messages| {
            messages
                .iter()
                .any(|message| *message == Message::system(KEEP_CLAIMED_UNKEPT))
        }),
    }
}

fn saying(words: &str) -> Vec<net::Result<ChatEvent>> {
    vec![text(words), Ok(ChatEvent::Done)]
}

/// The model's `remember` call.
fn remembering() -> Vec<net::Result<ChatEvent>> {
    vec![
        Ok(ChatEvent::ToolCalls(vec![call(
            "c1",
            REMEMBER,
            json!({"text": "Their car is on Level B1, spot 7."}),
        )])),
        Ok(ChatEvent::Done),
    ]
}

/// Whether anything said claims the keep.
fn claimed(went: &Went) -> bool {
    went.said
        .iter()
        .any(|said| said.to_lowercase().contains("replaces"))
}

const HEARD: &str = "Actually, I moved my car. It's on Level B1, spot 7 now.";
const E82D: &str =
    "Got it — your car is on Level B1, spot 7 now. That replaces wherever it was before.";

/// e82d's 09:52 turn, twice with no call: its true first sentence is said,
/// its claim never is; asked again once with the note, then said not kept.
#[test]
fn a_keep_claimed_in_a_later_sentence_with_no_call_is_said_not_kept() {
    let went = went(HEARD, true, vec![saying(E82D), saying(E82D)]);
    assert!(!claimed(&went), "{went:?}");
    assert!(went.noted, "{went:?}");
    assert_eq!(went.rounds, 2, "{went:?}");
    assert!(
        went.said
            .last()
            .is_some_and(|said| said.trim_start_matches("said:").trim() == NOT_KEPT_LINE),
        "{went:?}"
    );
    assert!(
        went.said.iter().any(|said| said.contains("spot 7 now.")),
        "{went:?}"
    );
    assert!(went.called.is_empty(), "{went:?}");
}

/// Asked again, the model calls `remember`: its reply after the call is
/// said, and the first claim never was.
#[test]
fn asked_again_the_keep_is_made_and_its_reply_said() {
    let went = went(
        HEARD,
        true,
        vec![
            saying(E82D),
            remembering(),
            saying("Got it, your car is on Level B1, spot 7 now. That replaces where it was."),
        ],
    );
    assert!(went.noted, "{went:?}");
    assert_eq!(went.called, [REMEMBER], "{went:?}");
    assert!(
        went.said.iter().any(|said| said.contains("spot 7")),
        "{went:?}"
    );
}

/// A plain restatement with no claim is said as it is, with no note.
#[test]
fn a_restatement_with_no_claim_is_said_as_it_is() {
    let reply = "Your car is on Level B1, spot 7 now. Good to know where to look.";
    let went = went(HEARD, true, vec![saying(reply)]);
    assert!(!went.noted, "{went:?}");
    assert_eq!(went.rounds, 1, "{went:?}");
    assert!(
        went.said.iter().any(|said| said.contains("spot 7")),
        "{went:?}"
    );
}

/// A first-person request with no claim streams as today: "Make my light
/// warmer." with its light call and "Done, I've updated your light." said
/// whole, no note.
#[test]
fn a_first_person_request_with_no_keep_claim_is_said_as_it_is() {
    let reply = "Done, I've updated your light. It's warmer now.";
    let went = went(
        "Make my light warmer.",
        true,
        vec![
            vec![
                Ok(ChatEvent::ToolCalls(vec![call(
                    "c1",
                    "light_warmer",
                    json!({}),
                )])),
                Ok(ChatEvent::Done),
            ],
            saying(reply),
        ],
    );
    assert!(!went.noted, "{went:?}");
    assert!(
        went.said.iter().any(|said| said.contains("warmer now")),
        "{went:?}"
    );
}

/// Off (`LAMP_KEEP_CLAIM_WHOLE` unset): e82d's reply is said as before,
/// the fault itself, so the switch is what changes it.
#[test]
fn off_the_reply_is_said_as_before() {
    let went = went(HEARD, false, vec![saying(E82D)]);
    assert!(!went.noted, "{went:?}");
    assert!(claimed(&went), "{went:?}");
}

/// The Reviewer-lead's and Friday's P2s on #7924: beside the round's own
/// other call, a change said is that call's, never a keep claim: no note,
/// and the reply said.
#[test]
fn a_change_said_beside_another_call_is_that_calls() {
    let reply = "Okay, I've updated that: your dentist reminder is at 3 now.";
    let went = went(
        "My dentist moved my appointment to three.",
        true,
        vec![
            vec![
                Ok(ChatEvent::ToolCalls(vec![call(
                    "c1",
                    "set_reminder",
                    json!({"time": "15:00", "text": "dentist"}),
                )])),
                text(reply),
                Ok(ChatEvent::Done),
            ],
            saying(reply),
        ],
    );
    assert!(!went.noted, "{went:?}");
    assert!(
        went.said.iter().any(|said| said.contains("dentist")),
        "{went:?}"
    );
}

/// A long story is said sentence by sentence, its first after one sentence,
/// whole and with no note.
#[test]
fn a_long_story_is_said_sentence_by_sentence() {
    let reply = "Okay. Here's a short one. A lamp once lit a room. Everyone smiled. The end.";
    let went = went("I'm bored, tell me a story.", true, vec![saying(reply)]);
    assert!(!went.noted, "{went:?}");
    assert_eq!(went.rounds, 1, "{went:?}");
    assert!(
        went.said
            .first()
            .is_some_and(|said| said.trim_start_matches("said:").trim() == "Okay."),
        "{went:?}"
    );
    assert!(
        went.said.iter().any(|said| said.contains("The end")),
        "{went:?}"
    );
}

/// V2's must-fix (3) on #7924: a keep in memory's own words beside another
/// call is still no keep: "I'll remember you're vegetarian." beside a
/// reminder set is not said, and the round is told.
#[test]
fn a_keep_said_beside_another_call_is_still_caught() {
    let went = went(
        "I'm vegetarian, and remind me to shop at five.",
        true,
        vec![
            vec![
                Ok(ChatEvent::ToolCalls(vec![call(
                    "c1",
                    "set_reminder",
                    json!({"time": "17:00", "text": "shop"}),
                )])),
                text("Reminder set for five. I'll remember you're vegetarian."),
                Ok(ChatEvent::Done),
            ],
            saying("Reminder set for five."),
        ],
    );
    assert!(went.noted, "{went:?}");
    assert!(
        !went.said.iter().any(|said| said.contains("vegetarian")),
        "{went:?}"
    );
}

/// Friday's P2 on #7924: the face offer that waits for a name is no keep
/// claim: said as it is, with no note.
#[test]
fn a_face_offer_that_waits_for_a_name_is_said_as_it_is() {
    let reply = "Nice to meet you. I'll remember your face once you tell me your name.";
    let went = went(
        "Hi lamp, it's my first day here.",
        true,
        vec![saying(reply)],
    );
    assert!(!went.noted, "{went:?}");
    assert!(
        went.said
            .iter()
            .any(|said| said.contains("once you tell me")),
        "{went:?}"
    );
}
