//! A held line's `reports_stop` on the second route (the first builder on
//! #1722, 16:44; #1683): done though its answer stays held, as on the Live
//! route (`crate::attention::DONE_THOUGH_HELD`). The rule holds the line;
//! with `LAMP_REPORTS=on` its words, when they name reports
//! (`crate::attention::HELD_ASKED_WORDS`), are still asked once, told only
//! `reports_stop`, and nothing of that round is said; any other held line
//! is sent nowhere.
use super::*;

/// A colleague's line, said to someone else: held by the rule.
const NOT_TO_THE_LAMP: &str = "Stop sending the reports to the client.";

/// An engine on `script` whose rule holds every line, doing the calls done
/// though held when `though_held`.
fn held_engine(script: Arc<Script>, though_held: bool) -> (Engine, Calls) {
    let (engine, calls, _) = scripted(script);
    let engine = engine
        .heeding(Arc::new(|_, _, _, _, _| Some("a busy room")))
        .doing_though_held(though_held);
    (engine, calls)
}

fn say(engine: &mut Engine, request: u64) {
    engine.submit(start(request));
    engine.submit(audio(request, 1));
    engine.submit(end(request));
}

/// A held line whose round calls `reports_stop`: the call is done, heard,
/// and nothing is said: no words, no sound, the turn ends as any held one.
/// The round is told `reports_stop` alone, and the note beside the words.
#[tokio::test]
async fn a_held_lines_reports_stop_is_done_and_its_answer_stays_held() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(NOT_TO_THE_LAMP.to_owned())].into()),
        chats: Mutex::new(
            [vec![
                text("I've stopped sending reports."),
                Ok(ChatEvent::ToolCalls(vec![call(
                    "c1",
                    crate::reports_stop::NAME,
                    json!({}),
                )])),
                Ok(ChatEvent::Done),
            ]]
            .into(),
        ),
        ..Script::default()
    });
    let (mut engine, calls) = held_engine(script.clone(), true);
    say(&mut engine, 1);
    assert_eq!(
        turn(&mut engine).await,
        [
            "started:1",
            format!("heard:{NOT_TO_THE_LAMP}").as_str(),
            "complete"
        ]
    );
    assert_eq!(
        *calls.lock().unwrap(),
        [(crate::reports_stop::NAME.to_owned(), json!({}), true)]
    );
    assert_eq!(
        *script.told_only.lock().unwrap(),
        [crate::attention::DONE_THOUGH_HELD]
    );
    let asked = script.asked.lock().unwrap();
    let round = &asked[0];
    let tail = &round[round.len() - 2..];
    assert_eq!(
        serde_json::to_value(tail).unwrap(),
        serde_json::to_value([Message::system(HELD_NOTE), Message::user(NOT_TO_THE_LAMP),])
            .unwrap()
    );
}

/// Only a call done though held is done for a held line: one the model
/// makes anyway, beside it or alone, stays held with the answer.
#[tokio::test]
async fn a_held_lines_other_calls_are_not_done() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Dance and then stop the reports.".to_owned())].into()),
        chats: Mutex::new(
            [vec![
                Ok(ChatEvent::ToolCalls(vec![
                    call("c1", "dance", json!({})),
                    call("c2", crate::reports_stop::NAME, json!({})),
                    call("c3", "light_on", json!({})),
                ])),
                Ok(ChatEvent::Done),
            ]]
            .into(),
        ),
        ..Script::default()
    });
    let (mut engine, calls) = held_engine(script, true);
    say(&mut engine, 1);
    assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    let names: Vec<String> = calls
        .lock()
        .unwrap()
        .iter()
        .map(|(name, ..)| name.clone())
        .collect();
    assert_eq!(names, [crate::reports_stop::NAME]);
}

/// Without `LAMP_REPORTS=on` a held line is never asked, as before.
#[tokio::test]
async fn without_reports_a_held_line_is_never_asked() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(NOT_TO_THE_LAMP.to_owned())].into()),
        ..Script::default()
    });
    let (mut engine, calls) = held_engine(script.clone(), false);
    say(&mut engine, 1);
    assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    assert!(script.asked.lock().unwrap().is_empty());
    assert!(calls.lock().unwrap().is_empty());
}

/// A held line's round that never ends is given up at `HELD_WITHIN`: the
/// turn ends then, with nothing done and nothing said.
#[tokio::test(start_paused = true)]
async fn a_held_lines_round_that_never_ends_is_given_up() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(NOT_TO_THE_LAMP.to_owned())].into()),
        chats: Mutex::new([vec![]].into()),
        hold_chat: true,
        ..Script::default()
    });
    let (mut engine, calls) = held_engine(script, true);
    say(&mut engine, 1);
    let begun = tokio::time::Instant::now();
    loop {
        if let ProviderOutput::TurnComplete { request: 1, .. } = engine.next().await {
            break;
        }
    }
    assert!(begun.elapsed() >= HELD_WITHIN);
    assert!(begun.elapsed() < HELD_WITHIN + Duration::from_secs(1));
    assert!(calls.lock().unwrap().is_empty());
}

/// How long a held line's turn waits for its round, on the scripted
/// chat: the time from the line's end to its turn's end is the round's.
#[tokio::test(start_paused = true)]
async fn a_held_lines_turn_ends_as_its_round_does() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(NOT_TO_THE_LAMP.to_owned())].into()),
        chats: Mutex::new([vec![Ok(ChatEvent::Done)]].into()),
        first_event_after: Some(Duration::from_millis(900)),
        ..Script::default()
    });
    let (mut engine, _) = held_engine(script, true);
    say(&mut engine, 1);
    let begun = tokio::time::Instant::now();
    loop {
        if let ProviderOutput::TurnComplete { request: 1, .. } = engine.next().await {
            break;
        }
    }
    let took = begun.elapsed();
    assert!(
        took >= Duration::from_millis(900) && took < Duration::from_millis(1_000),
        "{took:?}"
    );
}

/// Busy-room talk with none of [`crate::attention::HELD_ASKED_WORDS`]:
/// held, and sent nowhere.
const NOT_ABOUT_REPORTS: &str = "Send me the file before lunch.";

/// A held line that names no reports makes no chat request, with
/// `LAMP_REPORTS=on`: its words are sent nowhere (#1925).
#[tokio::test]
async fn a_held_line_not_naming_reports_makes_no_chat_request() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(NOT_ABOUT_REPORTS.to_owned())].into()),
        chats: Mutex::new([vec![Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let (mut engine, calls) = held_engine(script.clone(), true);
    say(&mut engine, 1);
    assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    assert_eq!(script.asked.lock().unwrap().len(), 0);
    assert!(script.told_only.lock().unwrap().is_empty());
    assert!(calls.lock().unwrap().is_empty());
}

/// A held line that names reports makes exactly one chat request.
#[tokio::test]
async fn a_held_line_naming_reports_makes_one_chat_request() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Lamp, stop sharing my data.".to_owned())].into()),
        chats: Mutex::new([vec![Ok(ChatEvent::Done)], vec![Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let (mut engine, _) = held_engine(script.clone(), true);
    say(&mut engine, 1);
    assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    assert_eq!(script.asked.lock().unwrap().len(), 1);
}

/// The journal's line for an asked held line names no words.
#[test]
fn the_asked_line_is_journaled_without_its_words() {
    assert_eq!(
        held_asked_line(7),
        "lamp-voice second: request 7: a held line names reports: asked once"
    );
}

/// Second's P2 on #1925: with `LAMP_SECOND_SCOPED_CONTEXT=on`, a held
/// reports line is told for the person admitted with it, never for the
/// session's sticky person. A is the session's person, whose memory and
/// thread the unscoped builder tells; B is at the lamp when the line is
/// admitted. The held round carries B's context alone, none of A's.
#[tokio::test]
async fn a_held_reports_line_is_told_only_its_admitted_persons_context() {
    use crate::memory_scope::{FaceCache, ScopeLease};
    const STICKY: &str = "A's memory and thread";
    let path = std::env::temp_dir().join(format!(
        "jarvis-held-scoped-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let cache = Arc::new(FaceCache::default());
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Lamp, stop sharing my data.".to_owned())].into()),
        chats: Mutex::new([vec![Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let tools: ToolRunner = Arc::new(|_, _, _, _| Box::pin(async { json!({"ok": true}) }));
    let runner: ScopedToolRunner =
        Arc::new(|_, _, _, _, _: ScopeLease| Box::pin(async { json!({"ok": true}) }));
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| STICKY.to_owned().into()),
        tools,
        Arc::new(Instant::now),
    )
    .with_memory_scope(
        lamp_turn_owner::BootId::new([25; 16]).unwrap(),
        cache.clone(),
        runner,
        Some(Arc::new(|lease: &ScopeLease| {
            Told::from(format!("for {:?}", lease.scope().unwrap().person()))
        })),
    )
    .announcing(false)
    .heeding(Arc::new(|_, _, _, _, _| Some("a busy room")))
    .doing_though_held(true);
    engine.memory_camera(true, 1);
    // B is at the lamp when the line is admitted.
    std::fs::write(&path, "B").unwrap();
    cache.replace(
        cache.begin_refresh(),
        crate::known_faces::FaceObservation::read(&path),
    );
    say(&mut engine, 1);
    assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    let asked = script.asked.lock().unwrap();
    assert_eq!(asked.len(), 1, "{asked:?}");
    let b = crate::remember::person_id("B");
    assert_eq!(
        asked[0][0],
        Message::system(format!("for {:?}", Some(b.as_str())))
    );
    let round = serde_json::to_string(&asked[0]).unwrap();
    assert!(!round.contains(STICKY), "{round}");
    std::fs::remove_file(path).unwrap();
}
