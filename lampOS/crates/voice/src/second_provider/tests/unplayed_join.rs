//! The first builder on #2602 (23 Oct 02:43), lamp-4ace's request 29 then
//! 30: "Hey Lamp, what time is it in London right now?" got the text
//! "Twenty past eight in…", never played, and "And in Saigon?" was joined to
//! it (`LAMP_JOIN_CUT_WORDS`). The thread kept the unplayed text as the
//! lamp's words, so the model took London as answered and said only
//! Saigon's time. A line joined into a later one had none of its answer
//! heard: that answer leaves the thread ([`Engine::unsaying_joined`]).
use super::*;
use crate::thread::Thread;

const LONDON: &str = "What time is it in London right now?";
const SAIGON: &str = "And in Saigon?";
const ANSWER: &str = "Twenty past eight in London.";

/// Request 1 asks London's time and its answer's text comes, the speech of
/// `held` never played; request 2 cuts it 1 s after its end and is joined
/// by one person. The thread is kept from the engine's output as the worker
/// keeps it, and told to each chat as its history. With `unsaying`, the
/// joined line's unplayed answer is taken out of the thread
/// (`crate::thread::unsay` in the worker). What the last chat was asked.
async fn asked_after_a_cut(
    answer: &'static str,
    held: &'static str,
    unsaying: bool,
) -> Vec<Message> {
    let asked = chats_after_a_cut(answer, held, unsaying, false).await;
    asked.last().unwrap().clone()
}

/// As [`asked_after_a_cut`]; with `held_line`, request 2 is held by the
/// rule and not joined (two voices), so request 1's words, cut short, are
/// answered in its place (#1712's carry). What each chat was asked.
async fn chats_after_a_cut(
    answer: &'static str,
    held: &'static str,
    unsaying: bool,
    held_line: bool,
) -> Vec<Vec<Message>> {
    let (release, hold) = tokio::sync::oneshot::channel();
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(LONDON.to_owned()), Ok(SAIGON.to_owned())].into()),
        chats: Mutex::new(
            [
                vec![text(answer), Ok(ChatEvent::Done)],
                vec![text("Half past two here."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        speech_hold: Mutex::new(Some((held, hold))),
        ..Script::default()
    });
    let thread = Arc::new(Mutex::new(Thread::new()));
    let told = thread.clone();
    let system: System = Arc::new(move || {
        let recent = told.lock().unwrap().recent_for(
            None,
            std::time::Instant::now(),
            Duration::from_secs(300),
            6,
        );
        let mut history = Vec::new();
        for exchange in recent {
            if !exchange.person.is_empty() {
                history.push(Message::user(exchange.person));
            }
            history.push(Message::assistant(exchange.lamp));
        }
        Told {
            text: "SYSTEM".to_owned(),
            after: None,
            history,
        }
    });
    let now = Arc::new(Mutex::new(Instant::now()));
    let clock = now.clone();
    let mut engine = Engine::with_clock(
        script.clone(),
        system,
        Arc::new(|_, _, _, _| Box::pin(async { json!({}) })),
        Arc::new(move || *clock.lock().unwrap()),
    )
    .announcing(false)
    .heeding(Arc::new(move |request, _, _, _| {
        (held_line && request == 2).then_some("not said to the lamp")
    }))
    .joining_cut_words(Arc::new(move |_, _| {
        if held_line {
            Joined::Not("two voices (cosine 0.12)".to_owned())
        } else {
            Joined::Same("the same voice said both")
        }
    }));
    if unsaying {
        let unsaid = thread.clone();
        engine = engine.unsaying_joined(Arc::new(move |request| {
            unsaid.lock().unwrap().unsay(request);
        }));
    }
    // What the worker keeps of each output (`worker::keep_scoped`).
    let keep = |event: &ProviderOutput| {
        let at = std::time::Instant::now();
        match event {
            ProviderOutput::Transcript {
                request: None,
                text,
                finished,
            } => thread.lock().unwrap().person(text, *finished, at),
            ProviderOutput::Transcript {
                request: Some(request),
                text,
                ..
            } if !text.is_empty() => {
                let text = format!("{} ", text.trim_start());
                thread.lock().unwrap().lamp(*request, &text, at);
            }
            _ => {}
        }
    };
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    // Until the held sentence's text is out: all of it before, played.
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
            .await
            .expect("request 1's text comes");
        keep(&event);
        if matches!(&event, ProviderOutput::Transcript { request: Some(1), text, .. } if text.trim() == held)
        {
            break;
        }
    }
    // Whatever was ready of it goes out before the line that cuts it.
    while let Ok(event) = tokio::time::timeout(Duration::from_millis(100), engine.next()).await {
        keep(&event);
    }
    *now.lock().unwrap() += Duration::from_millis(1_000);
    engine.submit(start(2));
    engine.retire_through(1);
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
            .await
            .expect("request 2's turn ends");
        keep(&event);
        if matches!(event, ProviderOutput::TurnComplete { request: 2, .. }) {
            break;
        }
    }
    drop(release);
    script.asked.lock().unwrap().clone()
}

/// A cut answer with no sound played, then a joined follow-up: the model is
/// asked both questions, with no word of the unplayed answer as the lamp's.
/// Without the unsaying (as before it), that text was the lamp's words in
/// the history.
#[tokio::test]
async fn a_joined_lines_unplayed_answer_is_not_history() {
    let asked = asked_after_a_cut(ANSWER, ANSWER, true).await;
    assert_eq!(
        asked.last().unwrap(),
        &Message::user(format!("{LONDON} {SAIGON}")),
        "{asked:?}"
    );
    assert!(
        !format!("{asked:?}").contains("Twenty past eight"),
        "{asked:?}"
    );
    assert!(
        !asked.contains(&Message::user(LONDON)),
        "London is asked once, in the joined line: {asked:?}"
    );
    let before = asked_after_a_cut(ANSWER, ANSWER, false).await;
    assert!(
        before.contains(&Message::assistant(ANSWER)),
        "without it the unplayed text was history: {before:?}"
    );
}

/// An answer that began playing and was cut is never joined, and the thread
/// keeps it as it did: the next line is asked alone, after it.
#[tokio::test]
async fn an_answer_cut_after_it_began_playing_keeps_its_words() {
    let answer = "Twenty past eight in London. It is a fine evening there.";
    for unsaying in [true, false] {
        let asked = asked_after_a_cut(answer, "It is a fine evening there.", unsaying).await;
        assert_eq!(asked.last().unwrap(), &Message::user(SAIGON), "{asked:?}");
        assert!(asked.contains(&Message::user(LONDON)), "{asked:?}");
        assert!(
            asked.contains(&Message::assistant(answer)),
            "{unsaying}: {asked:?}"
        );
    }
}

/// Reviewer-three's P3 on #3129: words answered in a held line's place, not
/// joined (#1712's carry), had none of their answer played either. With the
/// unsaying, the model is asked London again with no partial answer in the
/// history; without it (as before), "Twenty past eight" was the lamp's words
/// there, after which London was asked again.
#[tokio::test]
async fn words_carried_into_a_held_line_leave_their_unplayed_answer() {
    let chats = chats_after_a_cut(ANSWER, ANSWER, true, true).await;
    assert_eq!(chats.len(), 2, "London answered in the held line's place");
    let asked = chats.last().unwrap();
    assert_eq!(asked.last().unwrap(), &Message::user(LONDON), "{asked:?}");
    assert!(
        !format!("{asked:?}").contains("Twenty past eight"),
        "{asked:?}"
    );
    assert!(
        !format!("{asked:?}").contains(SAIGON),
        "the held line's words are not asked: {asked:?}"
    );
    let before = chats_after_a_cut(ANSWER, ANSWER, false, true).await;
    let before = before.last().unwrap();
    assert_eq!(before.last().unwrap(), &Message::user(LONDON), "{before:?}");
    assert!(
        before.contains(&Message::assistant(ANSWER)),
        "without it the unplayed text was history: {before:?}"
    );
}

/// An answer that began playing is never carried: a held line after it
/// answers nothing in its place, and the thread keeps the answer as before.
#[tokio::test]
async fn a_held_line_after_a_played_answer_changes_nothing() {
    let answer = "Twenty past eight in London. It is a fine evening there.";
    for unsaying in [true, false] {
        let chats = chats_after_a_cut(answer, "It is a fine evening there.", unsaying, true).await;
        assert_eq!(chats.len(), 1, "{unsaying}: nothing asked again: {chats:?}");
        assert_eq!(
            chats[0].last().unwrap(),
            &Message::user(LONDON),
            "{chats:?}"
        );
    }
}
