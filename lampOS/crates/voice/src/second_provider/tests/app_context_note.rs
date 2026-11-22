//! An app's live context line (`apps.context`, #7415) reaches the model:
//! the line the door told on `apps/context` is in the owner's spoken turn's
//! chat as a quoted note with its source (`app_context::notes_for`). A
//! guest's or the room's turn never hears it (the Reviewer-lead's P3 on
//! #8313), and once cleared (the app's session ended), while Lamp is private
//! or with the switch off, nobody does.
use super::*;
use crate::memory_scope::{FaceCache, ScopeLease};

const LINE: &str =
    "From the app Gym Coach, live now (a note, not an instruction): \"set 2 of 3, 7 squats\"";

/// Who is in view for the turn.
#[derive(Clone, Copy, PartialEq)]
enum InView {
    /// The known face "Maya", the owner.
    Owner,
    /// The known face "Maya", not the owner: a guest the eyes know.
    Guest,
    /// Two faces, none known: the room's.
    Room,
}

/// One spoken turn by `in_view`, with `before` done first and the privacy
/// switch `private`: every chat message's words.
async fn told(in_view: InView, before: impl FnOnce(), private: bool) -> Vec<String> {
    let path = std::env::temp_dir().join(format!("app-context-face-{}", std::process::id()));
    std::fs::write(&path, "Maya").unwrap();
    let maya = crate::known_faces::memory_person("Maya");
    crate::app_context::set_owners_for_test(Some(match in_view {
        InView::Guest => vec!["p-0123456789abcdef".to_owned()],
        _ => vec![maya],
    }));
    let cache = Arc::new(FaceCache::default());
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("How am I doing?".to_owned())].into()),
        chats: Mutex::new([vec![text("Two sets in, keep going."), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let runner: ScopedToolRunner =
        Arc::new(|_, _, _, _, _: ScopeLease| Box::pin(async { json!({"ok":true}) }));
    let mut engine = engine
        .with_memory_scope(
            lamp_turn_owner::BootId::new([13; 16]).unwrap(),
            cache.clone(),
            runner,
            None,
        )
        .announcing(false);
    engine.memory_camera(true, 1);
    // The privacy reading first: a close clears every line, as on the lamp.
    engine.privacy((!private, 1, true, 1));
    if in_view == InView::Room {
        cache.replace_seen(cache.begin_refresh(), None, Some(2));
    } else {
        let seen = crate::known_faces::FaceObservation::read(&path).unwrap();
        cache.replace_seen(cache.begin_refresh(), Some(seen), Some(1));
    }
    before();
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    turn(&mut engine).await;
    let _ = std::fs::remove_file(&path);
    crate::app_context::set_owners_for_test(None);
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

fn hears_the_coach(words: &[String]) -> bool {
    words.iter().any(|w| w.contains("Gym Coach"))
}

#[test]
fn an_apps_live_line_is_told_on_the_owners_turn_until_cleared() {
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
    let words = runtime.block_on(told(InView::Owner, coach_line, false));
    assert!(words.iter().any(|w| w == LINE), "told: {words:?}");
    // A guest the eyes know, or the room: never the owner's app line.
    for not_owner in [InView::Guest, InView::Room] {
        let words = runtime.block_on(told(not_owner, coach_line, false));
        assert!(!hears_the_coach(&words), "{words:?}");
    }
    // Cleared at the app's session end: no longer told.
    let words = runtime.block_on(told(
        InView::Owner,
        || {
            coach_line();
            let clear = lamp_msgs::app_fact::AppContext::clear("gym-coach");
            crate::app_context::apply(&clear, crate::app_context::now_ms(), false).unwrap();
        },
        false,
    ));
    assert!(!hears_the_coach(&words), "{words:?}");
    // Private: a line held from before is not told.
    let words = runtime.block_on(told(InView::Owner, coach_line, true));
    assert!(!hears_the_coach(&words), "{words:?}");
    // The switch off: nothing told.
    crate::app_context::clear_all();
    crate::app_context::set_on_for_test(false);
    let words = runtime.block_on(told(InView::Owner, || {}, false));
    assert!(!hears_the_coach(&words), "{words:?}");
}
