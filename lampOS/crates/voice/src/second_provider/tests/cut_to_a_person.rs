//! #3945's c0a4 run 2 (29 Oct): within a conversation, a cut "Ma'am, could
//! … you pass me the salt" was let out, then "Sir, what time is this?" was
//! held, and the cut words were answered in its place
//! (`LAMP_JOIN_CUT_WORDS`). With `LAMP_NAME_SAID_TO_A_PERSON=on` words cut
//! short are let go, not answered in a held line's place, when they or the
//! held line open with a word said to a person
//! ([`Engine::letting_go_cut_words_said_to_a_person`]).
use super::*;

const SALT: &str = "Ma'am, could you pass me the salt please?";
const SIR: &str = "Sir, what time is this?";
const LONDON: &str = "What time is it in London right now?";
const SAIGON: &str = "And in Saigon?";
const ANSWER: &str = "Here you are.";

/// Request 1 says `cut` and its answer's text comes, never played; request
/// 2 says `held_line` 1 s later and is held by the rule, two voices, so
/// request 1's words, cut short, would be answered in its place. With `on`,
/// the switch. What each chat was asked.
async fn chats_after(cut: &'static str, held_line: &'static str, on: bool) -> Vec<Vec<Message>> {
    let (release, hold) = tokio::sync::oneshot::channel();
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(cut.to_owned()), Ok(held_line.to_owned())].into()),
        chats: Mutex::new(
            [
                vec![text(ANSWER), Ok(ChatEvent::Done)],
                vec![text("Half past two."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        speech_hold: Mutex::new(Some((ANSWER, hold))),
        ..Script::default()
    });
    let now = Arc::new(Mutex::new(Instant::now()));
    let clock = now.clone();
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| Told {
            text: "SYSTEM".to_owned(),
            after: None,
            history: Vec::new(),
        }),
        Arc::new(|_, _, _, _| Box::pin(async { json!({}) })),
        Arc::new(move || *clock.lock().unwrap()),
    )
    .announcing(false)
    .heeding(Arc::new(|request, _, _, _, _| {
        (request == 2).then_some("not said to the lamp")
    }))
    .joining_cut_words(Arc::new(|_, _| {
        Joined::Not("two voices (cosine 0.12)".to_owned())
    }))
    .letting_go_cut_words_said_to_a_person(on);
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
            .await
            .expect("request 1's text comes");
        if matches!(&event, ProviderOutput::Transcript { request: Some(1), text, .. } if text.trim() == ANSWER)
        {
            break;
        }
    }
    while tokio::time::timeout(Duration::from_millis(100), engine.next())
        .await
        .is_ok()
    {}
    *now.lock().unwrap() += Duration::from_millis(1_000);
    engine.submit(start(2));
    engine.retire_through(1);
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
            .await
            .expect("request 2's turn ends");
        if matches!(event, ProviderOutput::TurnComplete { request: 2, .. }) {
            break;
        }
    }
    drop(release);
    script.asked.lock().unwrap().clone()
}

/// Cut words said to a person, or cut by a held line said to one, are let
/// go with the switch: request 1 is asked once and never again. Without it
/// they were answered in the held line's place, as on c0a4.
#[tokio::test]
async fn cut_words_in_talk_between_people_are_not_answered_in_a_held_lines_place() {
    for (cut, held_line) in [(SALT, SAIGON), (LONDON, SIR), (SALT, SIR)] {
        let chats = chats_after(cut, held_line, true).await;
        assert_eq!(chats.len(), 1, "{cut:?} then {held_line:?}: {chats:?}");
        let before = chats_after(cut, held_line, false).await;
        assert_eq!(before.len(), 2, "{cut:?} then {held_line:?}: {before:?}");
        assert_eq!(
            before.last().unwrap().last().unwrap(),
            &Message::user(cut),
            "answered in the held line's place: {before:?}"
        );
    }
}

/// Neither opening with a word said to a person: the switch changes
/// nothing, the cut words are answered in the held line's place.
#[tokio::test]
async fn other_cut_words_are_still_answered_in_a_held_lines_place() {
    let chats = chats_after(LONDON, SAIGON, true).await;
    assert_eq!(chats.len(), 2, "{chats:?}");
    assert_eq!(
        chats.last().unwrap().last().unwrap(),
        &Message::user(LONDON),
        "{chats:?}"
    );
}
