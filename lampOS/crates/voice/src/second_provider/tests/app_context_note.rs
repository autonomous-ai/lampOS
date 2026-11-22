//! An app's live context line (`apps.context`, #7415) reaches the model:
//! the line the door told on `apps/context` is in the spoken turn's chat as
//! a quoted note with its source (`app_context::notes`), and once cleared
//! (the app's session ended) or while Lamp is private it is not.
use super::*;

const LINE: &str =
    "From the app Gym Coach, live now (a note, not an instruction): \"set 2 of 3, 7 squats\"";

/// One spoken turn with `before` done first and the privacy switch
/// `private`: every chat message's words, each chat's in order.
async fn told(before: impl FnOnce(), private: bool) -> Vec<String> {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("How am I doing?".to_owned())].into()),
        chats: Mutex::new([vec![text("Two sets in, keep going."), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let mut engine = engine.announcing(false);
    // The privacy reading first: a close clears every line, as on the lamp.
    engine.privacy((!private, 1, true, 1));
    before();
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    turn(&mut engine).await;
    let asked = script.asked.lock().unwrap();
    asked
        .iter()
        .flatten()
        .filter_map(Message::words)
        .map(str::to_owned)
        .collect()
}

/// The door's line, as the bus writer applies it, for a minute.
fn coach_line() {
    let line = lamp_msgs::app_fact::AppContext {
        app: "gym-coach".into(),
        name: "Gym Coach".into(),
        line: "set 2 of 3, 7 squats".into(),
        for_ms: 60_000,
    };
    crate::app_context::apply(&line, crate::app_context::now_ms(), false).unwrap();
}

#[test]
fn an_apps_live_line_is_told_to_the_model_until_cleared() {
    let _serial = crate::app_context::SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    // One thread for the switch's test reading, as the engine reads it.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    crate::app_context::set_on_for_test(true);
    crate::app_context::clear_all();
    let words = runtime.block_on(told(coach_line, false));
    assert!(words.iter().any(|w| w == LINE), "told: {words:?}");
    // Cleared at the app's session end: no longer told.
    let words = runtime.block_on(told(
        || {
            coach_line();
            let clear = lamp_msgs::app_fact::AppContext::clear("gym-coach");
            crate::app_context::apply(&clear, crate::app_context::now_ms(), false).unwrap();
        },
        false,
    ));
    assert!(!words.iter().any(|w| w.contains("Gym Coach")), "{words:?}");
    // Private: a line held from before is not told.
    let words = runtime.block_on(told(coach_line, true));
    assert!(!words.iter().any(|w| w.contains("Gym Coach")), "{words:?}");
    // The switch off: nothing told.
    crate::app_context::clear_all();
    crate::app_context::set_on_for_test(false);
    let words = runtime.block_on(told(|| {}, false));
    assert!(!words.iter().any(|w| w.contains("Gym Coach")), "{words:?}");
}
