//! A held reply may refuse only the pending photo; it grants/deletes nothing else.
use super::*;

#[tokio::test]
async fn a_held_photo_no_runs_but_shared_yes_and_ordinary_forget_do_not() {
    use crate::memory_scope::{FaceCache, GAP, ScopeLease};
    for (label, enabled, name, args, runs) in [
        (
            "no",
            true,
            crate::known_faces::FORGET,
            json!({"photo_reply": true}),
            true,
        ),
        (
            "off",
            false,
            crate::known_faces::FORGET,
            json!({"photo_reply": true}),
            false,
        ),
        (
            "yes",
            true,
            crate::known_faces::REMEMBER,
            json!({"photo_reply": true}),
            false,
        ),
        (
            "ordinary",
            true,
            crate::known_faces::FORGET,
            json!({"name": "A"}),
            false,
        ),
        (
            "mixed",
            true,
            crate::known_faces::FORGET,
            json!({"photo_reply": true, "name": "A"}),
            false,
        ),
    ] {
        let path = std::env::temp_dir().join(format!("photo-held-{}-{label}", std::process::id()));
        std::fs::write(&path, "A").unwrap();
        let cache = Arc::new(FaceCache::holding_gaps(Some(GAP)));
        let script = Arc::new(Script {
            heard: Mutex::new([Ok("Hello.".to_owned()), Ok("No.".to_owned())].into()),
            chats: Mutex::new(
                [
                    vec![text("Hi."), Ok(ChatEvent::Done)],
                    vec![
                        Ok(ChatEvent::ToolCalls(vec![call("c1", name, args.clone())])),
                        Ok(ChatEvent::Done),
                    ],
                    vec![text("Okay."), Ok(ChatEvent::Done)],
                ]
                .into(),
            ),
            ..Script::default()
        });
        let (engine, _, _) = scripted(script.clone());
        let ran = Arc::new(Mutex::new(Vec::new()));
        let running = ran.clone();
        let runner: ScopedToolRunner = Arc::new(move |_, _, name, args, lease: ScopeLease| {
            running.lock().unwrap().push((
                name,
                args,
                lease.is_current(),
                lease.scope().unwrap().held(),
            ));
            Box::pin(async { json!({"ok": true}) })
        });
        let mut engine = engine.replying_to_photos(enabled).with_memory_scope(
            lamp_turn_owner::BootId::new([8; 16]).unwrap(),
            cache.clone(),
            runner,
            None,
        );
        engine.memory_camera(true, 1);
        let seen = || crate::known_faces::FaceObservation::read(&path).unwrap();
        cache.replace_seen(cache.begin_refresh(), Some(seen()), Some(1));
        engine.submit(start(1));
        engine.submit(audio(1, 1));
        engine.submit(end(1));
        turn(&mut engine).await;
        engine.memory_answered(1, Instant::now());
        cache.replace_seen(cache.begin_refresh(), None, Some(0));
        engine.submit(start(2));
        assert!(engine.memory_lease().unwrap().scope().unwrap().held());
        // The absent face returning cannot turn the captured held line into a grant.
        cache.replace_seen(cache.begin_refresh(), Some(seen()), Some(1));
        engine.submit(audio(2, 1));
        engine.submit(end(2));
        turn(&mut engine).await;
        let calls = ran.lock().unwrap().clone();
        if runs {
            assert_eq!(calls, [(name.to_owned(), args, true, true)]);
        } else {
            assert!(calls.is_empty(), "{label}: {calls:?}");
        }
        assert_eq!(script.photo_held.load(Ordering::Relaxed) > 0, enabled);
        std::fs::remove_file(path).unwrap();
    }
}
