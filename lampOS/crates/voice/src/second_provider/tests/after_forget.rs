//! A forget cannot resend a turn's copied memory or earlier tool results.
use super::*;

/// The first read is followed by a forget and a call planned from that read.
/// Only a newly planned call after the clean boundary may run afterwards.
#[tokio::test]
async fn a_forget_discards_the_current_round_on_success_or_uncertain_failure() {
    for result in [
        json!({"forgotten": true}),
        json!({"error": "lost reply: old secret"}),
    ] {
        let script = Arc::new(Script {
            heard: Mutex::new([Ok("Forget that, then tell me the time.".into())].into()),
            chats: Mutex::new(
                [
                    vec![
                        Ok(ChatEvent::ToolCalls(vec![call(
                            "read",
                            "recall",
                            json!({}),
                        )])),
                        Ok(ChatEvent::Done),
                    ],
                    vec![
                        Ok(ChatEvent::ToolCalls(vec![
                            call("erase", "forget", json!({"about": "old secret"})),
                            call("stale", "set_timer", json!({"label": "old secret"})),
                        ])),
                        Ok(ChatEvent::Done),
                    ],
                    vec![
                        Ok(ChatEvent::ToolCalls(vec![call(
                            "fresh",
                            "clock",
                            json!({}),
                        )])),
                        Ok(ChatEvent::Done),
                    ],
                    vec![text("It is noon."), Ok(ChatEvent::Done)],
                ]
                .into(),
            ),
            ..Script::default()
        });
        let ran = Arc::new(Mutex::new(Vec::new()));
        let seen = ran.clone();
        let runner: ToolRunner = Arc::new(move |_, _, name, _| {
            seen.lock().unwrap().push(name.clone());
            let result = if name == "forget" {
                result.clone()
            } else {
                json!({"ok": true, "text": "old secret"})
            };
            Box::pin(async move { result })
        });
        let mut engine = Engine::new(
            script.clone(),
            || Told {
                text: "character with old secret".into(),
                after: Some("note with old secret".into()),
                history: vec![
                    Message::user("old secret"),
                    Message::assistant("old secret"),
                ],
            },
            runner,
        )
        .with_after_forget_instruction("Static character; answer in the owner's language.".into())
        .announcing(false);
        engine.submit(start(71));
        engine.submit(audio(71, 1));
        engine.submit(end(71));
        turn(&mut engine).await;
        assert_eq!(*ran.lock().unwrap(), ["recall", "forget", "clock"]);
        let asked = script.asked.lock().unwrap();
        assert!(
            serde_json::to_string(&asked[1])
                .unwrap()
                .contains("old secret")
        );
        let clean = serde_json::to_string(&asked[2]).unwrap();
        assert!(!clean.contains("old secret"), "{clean}");
        assert!(!clean.contains("tool_call_id"), "{clean}");
        assert!(clean.contains("1 later calls"), "{clean}");
        assert!(clean.contains("recall") && clean.contains("forget"));
        assert_eq!(
            asked[2].last(),
            Some(&Message::user("Forget that, then tell me the time."))
        );
        assert_eq!(
            asked[2][0],
            Message::system("Static character; answer in the owner's language.")
        );
    }
}

#[tokio::test]
async fn a_policy_hold_after_a_forget_still_ends_the_turn() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Forget that, then set a timer.".into())].into()),
        chats: Mutex::new(
            [vec![
                Ok(ChatEvent::ToolCalls(vec![
                    call("erase", "forget", json!({})),
                    call("held", "set_timer", json!({"minutes": 2})),
                ])),
                Ok(ChatEvent::Done),
            ]]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, calls, _) = scripted(script.clone());
    let mut engine = engine
        .announcing(false)
        .holding_for_the_policy(Arc::new(|_, name| {
            Box::pin(async move {
                if name == "set_timer" {
                    Policed::Held
                } else {
                    Policed::Run
                }
            })
        }));
    engine.submit(start(72));
    engine.submit(audio(72, 1));
    engine.submit(end(72));
    let labels = turn(&mut engine).await;
    assert!(
        labels.iter().all(|label| !label.starts_with("said:")),
        "{labels:?}"
    );
    assert_eq!(script.asked.lock().unwrap().len(), 1);
    assert_eq!(calls.lock().unwrap().len(), 1);
    assert_eq!(calls.lock().unwrap()[0].0, "forget");
}
