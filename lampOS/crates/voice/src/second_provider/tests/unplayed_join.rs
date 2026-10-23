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
    chats_after_a_cut_of(LONDON, answer, held, unsaying, held_line).await
}

/// As [`chats_after_a_cut`], request 1 asking `question`.
async fn chats_after_a_cut_of(
    question: &'static str,
    answer: &'static str,
    held: &'static str,
    unsaying: bool,
    held_line: bool,
) -> Vec<Vec<Message>> {
    let (release, hold) = tokio::sync::oneshot::channel();
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(question.to_owned()), Ok(SAIGON.to_owned())].into()),
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
            requires_speaker: false,
            pending_private_context: false,
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

/// A thread kept from the engine's output as the worker keeps it, told to
/// each chat as its history, as in [`chats_after_a_cut`].
fn kept_thread() -> (Arc<Mutex<Thread>>, System) {
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
            requires_speaker: false,
            pending_private_context: false,
        }
    });
    (thread, system)
}

/// What the worker keeps of `event` (`worker::keep_scoped`).
fn keep(thread: &Mutex<Thread>, event: &ProviderOutput) {
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
}

/// El's P2 on #3129: request 2's join waits for a print, and request 3
/// takes the chain in that wait; request 3 is not joined (two voices), so
/// the chain's words are let go. Request 2 is never asked, so request 1's
/// cut exchange stays in the thread: the person's question is not lost
/// from the history (#2145's P2). With `as_the_print_comes`, request 3's
/// cut lands as request 2's print comes, after the join's last wait and
/// before it sees the chain was taken (the cut from another thread, or a
/// turn not yet stopped): before the fix the join had unsaid request 1
/// there. Else request 3 begins during the wait, which also stops request
/// 2's turn. What request 3's chat was asked, and each request unsaid.
async fn asked_when_a_newer_line_takes_the_chain(
    as_the_print_comes: bool,
) -> (Vec<Message>, Vec<u64>) {
    const RAIN: &str = "Will it rain tomorrow?";
    let (release, hold) = tokio::sync::oneshot::channel();
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok(LONDON.to_owned()),
                Ok(SAIGON.to_owned()),
                Ok(RAIN.to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![text(ANSWER), Ok(ChatEvent::Done)],
                vec![text("Light rain later."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        speech_hold: Mutex::new(Some((ANSWER, hold))),
        ..Script::default()
    });
    let (thread, system) = kept_thread();
    let unsaid: Arc<Mutex<Vec<u64>>> = Arc::default();
    let seen = unsaid.clone();
    let told = thread.clone();
    // Request 2's print is still to come (one face, once it does); request
    // 3 is another voice.
    let (waiting, waits) = std::sync::mpsc::channel();
    let engine_shared: Arc<std::sync::OnceLock<Arc<Shared>>> = Arc::default();
    let taking = engine_shared.clone();
    let now = Arc::new(Mutex::new(Instant::now()));
    let clock = now.clone();
    let mut engine = Engine::with_clock(
        script.clone(),
        system,
        Arc::new(|_, _, _, _| Box::pin(async { json!({}) })),
        Arc::new(move || *clock.lock().unwrap()),
    )
    .announcing(false)
    .joining_cut_words(Arc::new(move |_, request| {
        if request != 2 {
            return Joined::Not("two voices (cosine 0.12)".to_owned());
        }
        if !as_the_print_comes {
            let _ = waiting.send(());
            return Joined::Coming(Box::new(Joined::Same("one face was seen speaking both")));
        }
        // Request 3's cut, as `Engine::cut_short` makes it, lands now.
        let shared = taking.get().expect("the engine is built");
        let chain = shared
            .in_flight
            .lock()
            .unwrap()
            .take_if(|carried| carried.request <= 2);
        if chain.is_some() {
            *shared.cut.lock().unwrap() = chain;
            let _ = waiting.send(());
        }
        Joined::Same("one face was seen speaking both")
    }))
    .unsaying_joined(Arc::new(move |request| {
        seen.lock().unwrap().push(request);
        told.lock().unwrap().unsay(request);
    }));
    let _ = engine_shared.set(engine.shared.clone());
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
            .await
            .expect("request 1's text comes");
        keep(&thread, &event);
        if matches!(&event, ProviderOutput::Transcript { request: Some(1), text, .. } if text.trim() == ANSWER)
        {
            break;
        }
    }
    while let Ok(event) = tokio::time::timeout(Duration::from_millis(100), engine.next()).await {
        keep(&thread, &event);
    }
    *now.lock().unwrap() += Duration::from_millis(1_000);
    engine.submit(start(2));
    engine.retire_through(1);
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    // Until request 2's join waits for its print, or its chain is taken.
    while waits.try_recv().is_err() {
        if let Ok(event) = tokio::time::timeout(Duration::from_millis(5), engine.next()).await {
            keep(&thread, &event);
        }
    }
    engine.submit(start(3));
    engine.retire_through(2);
    // Past request 2's wait: its print came, and the chain was taken.
    let past = tokio::time::Instant::now() + JOIN_WAIT * 2;
    while let Ok(event) = tokio::time::timeout_at(past, engine.next()).await {
        keep(&thread, &event);
    }
    engine.submit(audio(3, 1));
    engine.submit(end(3));
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
            .await
            .expect("request 3's turn ends");
        keep(&thread, &event);
        if matches!(event, ProviderOutput::TurnComplete { request: 3, .. }) {
            break;
        }
    }
    drop(release);
    let asked = script.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 2, "request 2 is never asked: {asked:?}");
    let asked = asked.last().unwrap().clone();
    assert_eq!(asked.last().unwrap(), &Message::user(RAIN), "{asked:?}");
    let unsaid = unsaid.lock().unwrap().clone();
    (asked, unsaid)
}

/// El's P2, reviewer-three's required test: a third line begins in the
/// join's print wait, takes the chain, and is not joined (another voice):
/// London's question is still in the thread.
#[tokio::test]
async fn a_join_whose_chain_a_newer_line_took_unsays_nothing() {
    let (asked, unsaid) = asked_when_a_newer_line_takes_the_chain(false).await;
    assert!(
        asked.contains(&Message::user(LONDON)),
        "the cut exchange stays in the history: {asked:?}"
    );
    assert!(unsaid.is_empty(), "{unsaid:?}");
}

/// El's P2 in its narrow window: the newer line's cut lands as the print
/// comes, after the join's last wait. The join unsays nothing, since its
/// line is never asked, and London's question stays in the thread.
#[tokio::test]
async fn a_chain_taken_as_the_print_comes_unsays_nothing() {
    let (asked, unsaid) = asked_when_a_newer_line_takes_the_chain(true).await;
    assert!(
        asked.contains(&Message::user(LONDON)),
        "the cut exchange stays in the history: {asked:?}"
    );
    assert!(unsaid.is_empty(), "{unsaid:?}");
}

/// Three pieces, each joined to the line before: request 1's answer a
/// search never finished, request 2 (joined to it) cut before its text
/// played, request 3 joined to both. Request 3 unsays the two pieces before
/// it, newest first, so the oldest question leads; no unplayed answer is
/// left as the lamp's words.
#[tokio::test]
async fn a_three_piece_chain_unsays_both_pieces_oldest_first() {
    const TOKYO: &str = "And in Tokyo?";
    const SECOND: &str = "Eight and two.";
    let (release, hold) = tokio::sync::oneshot::channel();
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok(LONDON.to_owned()),
                Ok(SAIGON.to_owned()),
                Ok(TOKYO.to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c1",
                        "web_search",
                        json!({"query": "time in London"}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![text(SECOND), Ok(ChatEvent::Done)],
                vec![text("Eight, two and four."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        speech_hold: Mutex::new(Some((SECOND, hold))),
        ..Script::default()
    });
    let (searching, searched) = std::sync::mpsc::channel();
    let runner: ToolRunner = Arc::new(move |_, _, _, _| {
        let searching = searching.clone();
        Box::pin(async move {
            let _ = searching.send(());
            tokio::time::sleep(Duration::from_secs(30)).await;
            json!({"results": []})
        })
    });
    let (thread, system) = kept_thread();
    let unsaid: Arc<Mutex<Vec<u64>>> = Arc::default();
    let seen = unsaid.clone();
    let told = thread.clone();
    let now = Arc::new(Mutex::new(Instant::now()));
    let clock = now.clone();
    let mut engine = Engine::with_clock(
        script.clone(),
        system,
        runner,
        Arc::new(move || *clock.lock().unwrap()),
    )
    .announcing(false)
    .joining_cut_words(Arc::new(|_, _| Joined::Same("the same voice said both")))
    .unsaying_joined(Arc::new(move |request| {
        seen.lock().unwrap().push(request);
        told.lock().unwrap().unsay(request);
    }));
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    while searched.try_recv().is_err() {
        if let Ok(event) = tokio::time::timeout(Duration::from_millis(20), engine.next()).await {
            keep(&thread, &event);
        }
    }
    *now.lock().unwrap() += Duration::from_millis(600);
    engine.submit(start(2));
    engine.retire_through(1);
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
            .await
            .expect("request 2's text comes");
        keep(&thread, &event);
        if matches!(&event, ProviderOutput::Transcript { request: Some(2), text, .. } if text.trim() == SECOND)
        {
            break;
        }
    }
    while let Ok(event) = tokio::time::timeout(Duration::from_millis(100), engine.next()).await {
        keep(&thread, &event);
    }
    *now.lock().unwrap() += Duration::from_millis(1_000);
    engine.submit(start(3));
    engine.retire_through(2);
    engine.submit(audio(3, 1));
    engine.submit(end(3));
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
            .await
            .expect("request 3's turn ends");
        keep(&thread, &event);
        if matches!(event, ProviderOutput::TurnComplete { request: 3, .. }) {
            break;
        }
    }
    drop(release);
    assert_eq!(
        *unsaid.lock().unwrap(),
        [1, 2, 1],
        "request 2 unsays 1; request 3 unsays 2, then 1"
    );
    let asked = script.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 3, "{asked:?}");
    let asked = asked.last().unwrap();
    assert_eq!(
        asked.last().unwrap(),
        &Message::user(format!("{LONDON} {SAIGON} {TOKYO}")),
        "{asked:?}"
    );
    assert!(!format!("{asked:?}").contains(SECOND), "{asked:?}");
}

/// A piece let go past the chain's word cap is not asked again, so its
/// exchange stays in the thread: only the pieces asked are unsaid.
#[tokio::test]
async fn a_piece_dropped_by_the_caps_keeps_its_exchange() {
    let long: &'static str =
        Box::leak(format!("{LONDON} {}", ["and"; CHAIN_MOST_WORDS].join(" ")).into_boxed_str());
    let asked = chats_after_a_cut_of(long, ANSWER, ANSWER, true, false).await;
    let asked = asked.last().unwrap();
    assert_eq!(asked.last().unwrap(), &Message::user(SAIGON), "{asked:?}");
    assert!(
        asked.contains(&Message::user(long)),
        "the dropped piece's question stays: {asked:?}"
    );
    assert!(asked.contains(&Message::assistant(ANSWER)), "{asked:?}");
}

/// The first builder on #3126 (23 Oct 08:15), lamp-4ace's r12 on build G:
/// request 22 was joined (21's unplayed answer unsaid), 23 cut it and was
/// not joined, so 22's words were let go and 23 was answered and played;
/// the follow-up 25 was held in a busy room as "the line began 37 s after
/// the lamp's words ended". The journal's times showed it did begin 37 s
/// after 23's words (the kit waited too long, 08:53), so the hold was
/// right; this pins the let-go path. Here, each line judged by the
/// worker's own rule (`LAMP_NAME_OR_FOLLOW_UP`): request 1 calls the lamp
/// and is answered "Eight and two.", its speech held; request 2 is joined
/// to it (1 unsaid) and its answer is a search never finished; request 3
/// cuts it, is not joined (two voices), and is answered and played. The
/// call is then 40 s old and the room busy, and request 4 begins just
/// after 3's words: request 3's played answer is the lamp's words, in the
/// thread and in their end, so 4 is in the conversation going on. The
/// chain (`LAMP_GOING_ON_NOT_OWN`) is empty here: `Heeding` is never told
/// of a let-go, so it leaves the chain as it was, and the chain's own end
/// (`said_until_free_us`) is pinned by the worker's
/// `the_lamps_own_answers_to_talk_keep_no_conversation_going`.
#[tokio::test]
async fn a_played_answer_after_words_let_go_keeps_the_conversation_going() {
    use super::worker::Heeding;
    use crate::room_told::Room;
    let _one = crate::provider_worker::tests::one_thread();
    const CALL: &str = "Lamp, what time is it in London right now?";
    const RAIN: &str = "Will it rain tomorrow?";
    const SECOND: &str = "Eight and two.";
    const PLAYED: &str = "Light rain later.";
    const AFTER: &str = "And the day after?";
    let s = Duration::from_secs;
    *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
    // The lamp's words before the call: 44 s ago, or now on a clock booted
    // under 44 s ago, never a panic (#1569).
    {
        let mut thread = Thread::new();
        let now = std::time::Instant::now();
        let before = now.checked_sub(s(44)).unwrap_or(now);
        thread.person("Lamp, what's the weather today?", true, before);
        thread.lamp(100, "Sunny and warm today.", before);
        *crate::thread::THREAD.lock().unwrap() = thread;
    }
    let heeding = Arc::new(Mutex::new(Heeding::new(false, false, None, None)));
    {
        let mut heeding = heeding.lock().unwrap();
        heeding.as_the_only_voice(false, true, false);
        heeding.needing_name_or_follow_up();
    }
    let (release, hold) = tokio::sync::oneshot::channel();
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok(CALL.to_owned()),
                Ok(SAIGON.to_owned()),
                Ok(RAIN.to_owned()),
                Ok(AFTER.to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![text(SECOND), Ok(ChatEvent::Done)],
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c1",
                        "web_search",
                        json!({"query": "time in London"}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![text(PLAYED), Ok(ChatEvent::Done)],
                vec![text("Sunny again."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        speech_hold: Mutex::new(Some((SECOND, hold))),
        ..Script::default()
    });
    let (searching, searched) = std::sync::mpsc::channel();
    let runner: ToolRunner = Arc::new(move |_, _, _, _| {
        let searching = searching.clone();
        Box::pin(async move {
            let _ = searching.send(());
            tokio::time::sleep(Duration::from_secs(30)).await;
            json!({"results": []})
        })
    });
    let system: System = Arc::new(|| "SYSTEM".to_owned().into());
    let now = Arc::new(Mutex::new(Instant::now()));
    let clock = now.clone();
    let judged = heeding.clone();
    let did = heeding.clone();
    type Verdicts = Mutex<Vec<(u64, Option<&'static str>)>>;
    let verdicts: Arc<Verdicts> = Arc::default();
    let told = verdicts.clone();
    let mut engine = Engine::with_clock(
        script.clone(),
        system,
        runner,
        Arc::new(move || *clock.lock().unwrap()),
    )
    .announcing(false)
    // Each line a second long, ended now, as the microphone would read it.
    .heeding(Arc::new(move |request, _, heard, from| {
        let now_us = monotonic_us();
        let held = judged.lock().unwrap().heed(
            request,
            (now_us.saturating_sub(1_000_000), now_us),
            heard,
            from,
        );
        told.lock().unwrap().push((request, held));
        held
    }))
    .telling_what_it_did(Arc::new(move |what| did.lock().unwrap().lamp_did(what)))
    .joining_cut_words(Arc::new(|_, request| {
        if request == 2 {
            Joined::Same("the same voice said both")
        } else {
            Joined::Not("two voices (cosine 0.12)".to_owned())
        }
    }))
    .unsaying_joined(Arc::new(crate::thread::unsay));
    // What the worker keeps of each output (`worker::keep_scoped`).
    let keep = |event: &ProviderOutput| {
        let at = std::time::Instant::now();
        let mut thread = crate::thread::THREAD.lock().unwrap();
        match event {
            ProviderOutput::Transcript {
                request: None,
                text,
                finished,
            } => thread.person(text, *finished, at),
            ProviderOutput::Transcript {
                request: Some(request),
                text,
                ..
            } if !text.is_empty() => {
                thread.lamp(*request, &format!("{} ", text.trim_start()), at);
            }
            _ => {}
        }
    };
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
            .await
            .expect("request 1's text comes");
        keep(&event);
        if matches!(&event, ProviderOutput::Transcript { request: Some(1), text, .. } if text.trim() == SECOND)
        {
            break;
        }
    }
    while let Ok(event) = tokio::time::timeout(Duration::from_millis(100), engine.next()).await {
        keep(&event);
    }
    *now.lock().unwrap() += Duration::from_millis(600);
    engine.submit(start(2));
    engine.retire_through(1);
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while searched.try_recv().is_err() {
        assert!(
            std::time::Instant::now() < deadline,
            "request 2's search begins within 5 s: {verdicts:?}"
        );
        if let Ok(event) = tokio::time::timeout(Duration::from_millis(20), engine.next()).await {
            keep(&event);
        }
    }
    *now.lock().unwrap() += Duration::from_millis(1_000);
    engine.submit(start(3));
    engine.retire_through(2);
    engine.submit(audio(3, 1));
    engine.submit(end(3));
    let mut played = false;
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
            .await
            .expect("request 3's turn ends");
        keep(&event);
        played |= matches!(event, ProviderOutput::Audio { request: 3, .. });
        if matches!(event, ProviderOutput::TurnComplete { request: 3, .. }) {
            break;
        }
    }
    assert!(played, "request 3's answer is played: {verdicts:?}");
    let said = crate::thread::THREAD
        .lock()
        .unwrap()
        .said_last_ago_but(std::time::Instant::now(), &[]);
    assert!(
        said.is_some_and(|ago| ago < s(10)),
        "request 3's played answer is in the thread: {said:?}"
    );
    let ended = heeding.lock().unwrap().said_ended_ago(monotonic_us());
    assert!(
        ended.is_some_and(|ago| ago < s(10)),
        "request 3's played answer is the end of the lamp's words: {ended:?}"
    );
    // The call 40 s ago, and the room busy.
    heeding.lock().unwrap().call_older(s(40));
    {
        let mut room = crate::room_told::room().lock().unwrap();
        for other in 901..=903 {
            room.unanswered(other, monotonic_us());
        }
        assert_eq!(room.at(monotonic_us()), Room::Busy);
    }
    *now.lock().unwrap() += Duration::from_millis(2_000);
    engine.submit(start(4));
    engine.submit(audio(4, 1));
    engine.submit(end(4));
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
            .await
            .expect("request 4's turn ends");
        keep(&event);
        if matches!(event, ProviderOutput::TurnComplete { request: 4, .. }) {
            break;
        }
    }
    drop(release);
    let asked = script.asked.lock().unwrap().clone();
    *crate::thread::THREAD.lock().unwrap() = Thread::new();
    *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
    assert_eq!(
        asked.last().and_then(|asked| asked.last()),
        Some(&Message::user(AFTER)),
        "the follow-up is answered: {verdicts:?} {asked:?}"
    );
}
