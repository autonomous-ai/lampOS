//! Requested Music across holding continuations and lost-line retries.
use super::*;

fn music_tools() -> (crate::process::SessionDirectory, crate::tools::Settings) {
    let directory = crate::process::SessionDirectory::create().unwrap();
    std::fs::write(
        directory.path.join("tools.json"),
        json!({"tools":[{"name":"play_relaxing_music","description":"A requested recording."}]})
            .to_string(),
    )
    .unwrap();
    let settings = crate::tools::Settings {
        skills: Some(directory.path.clone()),
        requested_audio: true,
        ..Default::default()
    };
    (directory, settings)
}

fn music_engine(
    script: Arc<Script>,
    settings: crate::tools::Settings,
    boot: lamp_turn_owner::BootId,
) -> Engine {
    Engine::new(
        script,
        Told::default,
        worker::tool_runner(settings.clone(), None, None, false),
    )
    .with_memory_scope(
        boot,
        Arc::default(),
        worker::scoped_tool_runner(settings.clone(), None, None, false),
        Some(Arc::new(|lease: &crate::memory_scope::ScopeLease| {
            Told::from(format!("context {:?}", lease.scope().unwrap().person()))
        })),
    )
    .requesting_audio(
        boot,
        worker::requested_audio_runner(settings, None, None, false),
    )
    .announcing(false)
}

fn music_reply(asked: &Value) -> Value {
    let context = serde_json::from_value(asked["requested_audio_context"].clone()).unwrap();
    json!({"ok":true,"result":{
        "next":"Say one short confirmation.",
        "requested_audio":crate::second_provider::requested_audio_tests::offered(context),
    }})
}

fn one_offer<'a>(
    outputs: &'a [ProviderOutput],
    boot: lamp_turn_owner::BootId,
    request: u64,
    call_id: &str,
) -> &'a lamp_msgs::RequestedAudioOffer {
    let offers: Vec<_> = outputs
        .iter()
        .filter_map(|event| match event {
            ProviderOutput::RequestedAudio {
                request,
                session,
                offer,
            } => Some((*request, *session, offer)),
            _ => None,
        })
        .collect();
    assert_eq!(offers.len(), 1, "{outputs:?}");
    let (of, session, offer) = offers[0];
    assert_eq!((of, session), (request, 1));
    offer.check().unwrap();
    assert_eq!(offer.context.worker_boot, crate::process::boot_text(boot));
    assert_eq!(
        (
            offer.context.request,
            offer.context.provider_session,
            offer.context.privacy_generation,
        ),
        (request, 1, 1),
    );
    assert_eq!(offer.context.call_id, call_id);
    offer
}

fn assert_private_model(script: &Script) {
    let model = serde_json::to_string(&*script.asked.lock().unwrap()).unwrap();
    assert!(!model.contains("requested_audio"), "{model}");
    assert!(!model.contains("relaxing.wav"), "{model}");
}

#[tokio::test]
async fn a_music_call_followed_by_one_moment_is_not_a_holding_answer() {
    let (directory, settings) = music_tools();
    let peer = music_tool_peer(&directory.path.join("call.sock"), 1, music_reply);
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Please play a quiet recording.".to_owned())].into()),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "music-before-holding",
                        "play_relaxing_music",
                        json!({}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("One moment."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let boot = lamp_turn_owner::BootId::new([71; 16]).unwrap();
    let mut engine = music_engine(script.clone(), settings, boot).holding_answers(true);
    engine.submit(start(47));
    engine.submit(audio(47, 1));
    engine.submit(end(47));
    let outputs = music_outputs(&mut engine).await;
    let seen = tokio::time::timeout(Duration::from_secs(3), peer)
        .await
        .expect("the single Music call must finish within three seconds")
        .expect("the Music peer must finish normally");
    let offer = one_offer(&outputs, boot, 47, "music-before-holding");
    assert_eq!(
        seen,
        [json!({"op":"call","name":"play_relaxing_music","args":{},
            "requested_audio_context":offer.context})]
    );
    assert!(
        !outputs
            .iter()
            .any(|event| matches!(event, ProviderOutput::HoldingAnswer { .. })),
        "a tool call disqualifies its holding phrase: {outputs:?}"
    );
    let said: String = outputs
        .iter()
        .filter_map(|event| match event {
            ProviderOutput::Transcript {
                request: Some(47),
                text,
                ..
            } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(said, "One moment.");
    assert!(outputs.iter().any(|event| matches!(
        event,
        ProviderOutput::Audio { request: 47, samples, .. }
            if !samples.is_empty() && samples.iter().all(|sample| *sample == 11)
    )));
    assert_eq!(script.asked.lock().unwrap().len(), 2);
    assert_private_model(&script);
}

#[tokio::test]
async fn the_written_holding_continuation_has_no_music_capability() {
    let (directory, settings) = music_tools();
    let boot = lamp_turn_owner::BootId::new([72; 16]).unwrap();
    // A well-formed ticket in an ordinary reply is still not authority.
    let unasked =
        crate::second_provider::requested_audio_tests::offered(lamp_msgs::RequestedAudioContext {
            worker_boot: crate::process::boot_text(boot),
            provider_session: 1,
            request: 48,
            privacy_generation: 1,
            call_id: "written-holding-music".into(),
            issued_us: monotonic_us(),
        });
    unasked.check().unwrap();
    let peer = music_tool_peer(&directory.path.join("call.sock"), 1, move |asked| {
        assert!(asked.get("requested_audio_context").is_none(), "{asked}");
        json!({"ok":true,"result":{
            "next":"Say one short confirmation.",
            "requested_audio":unasked,
        }})
    });
    let script = Arc::new(Script {
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "written-holding-music",
                        "play_relaxing_music",
                        json!({}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("One moment."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let mut engine = music_engine(script.clone(), settings, boot).holding_answers(true);
    engine.submit(written(48, crate::holding_answer::NOTE));
    let outputs = music_outputs(&mut engine).await;
    let seen = tokio::time::timeout(Duration::from_secs(3), peer)
        .await
        .expect("the ordinary Written call must finish within three seconds")
        .expect("the Music peer must finish normally");
    assert_eq!(
        seen,
        [json!({"op":"call","name":"play_relaxing_music","args":{}})]
    );
    assert!(
        !outputs.iter().any(|event| matches!(
            event,
            ProviderOutput::RequestedAudio { .. } | ProviderOutput::HoldingAnswer { .. }
        )),
        "the lamp's Written continuation cannot grant Music or another continuation: {outputs:?}"
    );
    assert_eq!(script.listened.load(Ordering::Relaxed), 0);
    let asked = script.asked.lock().unwrap();
    assert_eq!(asked.len(), 2);
    assert_eq!(
        asked[0].last(),
        Some(&Message::user(crate::holding_answer::NOTE))
    );
    assert_eq!(
        asked[1].last(),
        Some(&Message::tool(
            "written-holding-music",
            json!({"next":"Say one short confirmation."}).to_string()
        ))
    );
    drop(asked);
    assert_private_model(&script);
}

/// The longer Written wait changes when a task-news round may begin, never
/// who asked it. A late Music call remains an ordinary, unheard call even
/// while task news is extracted for the coordinator beside it.
#[tokio::test(start_paused = true)]
async fn delayed_written_news_keeps_music_unheard_with_the_longer_wait() {
    const REQUEST: u64 = 51;
    const NOTE: &str = "(Nobody has spoken. News from work you handed over.)";
    const NEWS: &str = "written-music-wait:2:4";
    const ANSWER: &str = "The task is ready.";
    const FIRST_EVENT: Duration = Duration::from_secs(5);
    for enabled in [false, true] {
        for wrapped in [false, true] {
            let music_name = if wrapped {
                crate::use_skill::NAME
            } else {
                "play_relaxing_music"
            };
            let music_args = if wrapped {
                json!({"skill":"play_relaxing_music"})
            } else {
                json!({})
            };
            let script = Arc::new(Script {
                first_event_after: Some(FIRST_EVENT),
                chats: Mutex::new(
                    [
                        vec![
                            Ok(ChatEvent::ToolCalls(vec![
                                call("written-late-music", music_name, music_args.clone()),
                                call("written-late-news", crate::agent_tasks::CHECK, json!({})),
                            ])),
                            Ok(ChatEvent::Done),
                        ],
                        vec![text(ANSWER), Ok(ChatEvent::Done)],
                    ]
                    .into(),
                ),
                ..Default::default()
            });
            let (engine, legacy, _) = scripted(script.clone());
            let ordinary: Calls = Arc::default();
            let seen = ordinary.clone();
            let dispatched_at = Arc::new(Mutex::new(Vec::new()));
            let at = dispatched_at.clone();
            let scoped: ScopedToolRunner = Arc::new(move |request, heard, name, args, scope| {
                assert_eq!(request, REQUEST);
                assert_eq!(scope.key().request, REQUEST);
                assert!(scope.is_current());
                assert!(scope.scope().unwrap().person().is_none());
                assert!(!heard, "waiting longer cannot turn task news into speech");
                let result = if name == crate::agent_tasks::CHECK {
                    json!({"answer":ANSWER, "news_key":NEWS})
                } else {
                    assert_eq!(name, music_name);
                    json!({"error":"A recording needs a current spoken request."})
                };
                seen.lock().unwrap().push((name, args, heard));
                at.lock().unwrap().push(tokio::time::Instant::now());
                Box::pin(async move { result })
            });
            let audio_runner: RequestedAudioRunner = Arc::new(|_, _, _| {
                panic!("neither a late Written Music call nor task news has spoken authority")
            });
            let boot = lamp_turn_owner::BootId::new([75; 16]).unwrap();
            let mut engine = engine
                .with_memory_scope(boot, Arc::default(), scoped, None)
                .requesting_audio(boot, audio_runner)
                .waiting_for_written(enabled)
                .announcing(false);
            let begun = tokio::time::Instant::now();
            engine.submit(written(REQUEST, NOTE));
            // The Script delays both rounds by five seconds. This bound
            // covers both deliberately, without changing the production
            // first-round deadline or the existing output helper's bound.
            let outputs = tokio::time::timeout(WRITTEN_BEGIN_WITHIN * 2, async {
                let mut outputs = Vec::new();
                for _ in 0..64 {
                    let event = engine.next().await;
                    assert!(
                        !matches!(event, ProviderOutput::TurnFailed { .. }),
                        "{event:?}"
                    );
                    let complete = matches!(event, ProviderOutput::TurnComplete { .. });
                    outputs.push(event);
                    if complete {
                        return outputs;
                    }
                }
                panic!("the delayed Written turn exceeded its event bound");
            })
            .await
            .expect("the two delayed Written rounds must finish within fourteen seconds");
            assert!(matches!(
                outputs.last(),
                Some(ProviderOutput::TurnComplete {
                    request: REQUEST,
                    idle: true,
                })
            ));
            assert!(
                !outputs
                    .iter()
                    .any(|event| matches!(event, ProviderOutput::RequestedAudio { .. })),
                "{outputs:?}"
            );
            assert!(legacy.lock().unwrap().is_empty());
            assert_eq!(script.listened.load(Ordering::Relaxed), 0);
            assert_eq!(script.samples_heard.load(Ordering::Relaxed), 0);
            let said: String = outputs
                .iter()
                .filter_map(|event| match event {
                    ProviderOutput::Transcript {
                        request: Some(REQUEST),
                        text,
                        ..
                    } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            let news: Vec<_> = outputs
                .iter()
                .filter_map(|event| match event {
                    ProviderOutput::NewsAnswered { request, key } => Some((*request, key.as_str())),
                    _ => None,
                })
                .collect();
            let asked = script.asked.lock().unwrap();
            assert_eq!(asked[0].last(), Some(&Message::user(NOTE)));
            if enabled {
                assert_eq!(
                    *ordinary.lock().unwrap(),
                    [
                        (music_name.to_owned(), music_args, false),
                        (crate::agent_tasks::CHECK.to_owned(), json!({}), false),
                    ]
                );
                assert!(dispatched_at.lock().unwrap().iter().all(|at| {
                    let after = at.duration_since(begun);
                    (FIRST_EVENT..WRITTEN_BEGIN_WITHIN).contains(&after)
                }));
                assert_eq!(said, ANSWER);
                assert_eq!(news, [(REQUEST, NEWS)]);
                assert_eq!(asked.len(), 2);
                assert_eq!(
                    asked[1].last(),
                    Some(&Message::tool(
                        "written-late-news",
                        json!({"answer":ANSWER}).to_string()
                    ))
                );
                assert!(outputs.iter().any(|event| matches!(
                    event,
                    ProviderOutput::Audio { request: REQUEST, samples, .. }
                        if !samples.is_empty()
                            && samples.iter().all(|sample| *sample == ANSWER.len() as i16)
                )));
            } else {
                assert!(ordinary.lock().unwrap().is_empty());
                assert!(dispatched_at.lock().unwrap().is_empty());
                assert!(said.is_empty());
                assert!(news.is_empty());
                assert_eq!(asked.len(), 1);
                assert!(
                    !outputs
                        .iter()
                        .any(|event| matches!(event, ProviderOutput::Audio { .. })),
                    "{outputs:?}"
                );
                assert!(begun.elapsed() >= BEGIN_WITHIN);
                assert!(begun.elapsed() < FIRST_EVENT);
            }
            let model = serde_json::to_string(&*asked).unwrap();
            assert!(!model.contains(NEWS) && !model.contains(crate::agent_tasks::NEWS_KEY));
            drop(asked);
            assert_private_model(&script);
        }
    }
}

#[tokio::test]
async fn a_lost_line_retry_dispatches_music_once_with_the_original_identity() {
    let (directory, settings) = music_tools();
    let peer = music_tool_peer(&directory.path.join("call.sock"), 1, music_reply);
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Please play a quiet recording.".to_owned())].into()),
        chats: Mutex::new(
            [
                vec![Err(net::Error::Transport("chat connect"))],
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "music-on-retry",
                        "play_relaxing_music",
                        json!({}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("Ready."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let boot = lamp_turn_owner::BootId::new([73; 16]).unwrap();
    let (back, probes) = link_back_after(Duration::ZERO);
    let mut engine = music_engine(script.clone(), settings, boot).resending(back);
    engine.submit(start(49));
    engine.submit(audio(49, 1));
    engine.submit(end(49));
    let outputs = music_outputs(&mut engine).await;
    let seen = tokio::time::timeout(Duration::from_secs(3), peer)
        .await
        .expect("the retried Music call must finish within three seconds")
        .expect("the Music peer must finish normally");
    let offer = one_offer(&outputs, boot, 49, "music-on-retry");
    assert_eq!(
        seen,
        [json!({"op":"call","name":"play_relaxing_music","args":{},
            "requested_audio_context":offer.context})]
    );
    assert_eq!(probes.load(Ordering::Relaxed), 1);
    assert_eq!(script.listened.load(Ordering::Relaxed), 1);
    assert_eq!(
        script.samples_heard.load(Ordering::Relaxed),
        lamp_audio::CAPTURE_SAMPLES
    );
    let asked = script.asked.lock().unwrap();
    assert_eq!(asked.len(), 3);
    assert_eq!(asked[0], asked[1], "the retry keeps the original question");
    assert_eq!(
        asked[1].last(),
        Some(&Message::user("Please play a quiet recording."))
    );
    assert_eq!(
        asked[2].last(),
        Some(&Message::tool(
            "music-on-retry",
            json!({"next":"Say one short confirmation. The recording has only been offered for preparation; playback has not started and may still be refused. Confirm only that you will try to play it. Do not say it is playing, has started, or is ready."}).to_string()
        ))
    );
    drop(asked);
    assert_private_model(&script);
}

#[tokio::test]
async fn a_failure_after_a_music_call_does_not_resend_or_offer_again() {
    let (directory, settings) = music_tools();
    let peer = music_tool_peer(&directory.path.join("call.sock"), 1, music_reply);
    let music = || {
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "music-before-loss",
                "play_relaxing_music",
                json!({}),
            )])),
            Ok(ChatEvent::Done),
        ]
    };
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Please play a quiet recording.".to_owned())].into()),
        chats: Mutex::new(
            [
                music(),
                vec![Err(net::Error::Transport("chat read"))],
                // A wrong retry would have another call available; the
                // probe and chat counts must reject even an attempted one.
                music(),
                vec![text("Ready."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let boot = lamp_turn_owner::BootId::new([74; 16]).unwrap();
    let (back, probes) = link_back_after(Duration::ZERO);
    let mut engine = music_engine(script.clone(), settings, boot)
        .resending(back)
        .holding_answers(true);
    engine.submit(start(50));
    engine.submit(audio(50, 1));
    engine.submit(end(50));
    let outputs = tokio::time::timeout(Duration::from_secs(3), async {
        let mut outputs = Vec::new();
        for _ in 0..64 {
            let event = engine.next().await;
            let done = matches!(
                event,
                ProviderOutput::TurnFailed { .. } | ProviderOutput::TurnComplete { .. }
            );
            outputs.push(event);
            if done {
                return outputs;
            }
        }
        panic!("the failed Music turn exceeded its event bound");
    })
    .await
    .expect("the failed Music turn must finish within three seconds");
    let seen = tokio::time::timeout(Duration::from_secs(3), peer)
        .await
        .expect("the original Music call must finish within three seconds")
        .expect("the Music peer must finish normally");
    let offer = one_offer(&outputs, boot, 50, "music-before-loss");
    assert_eq!(
        seen,
        [json!({"op":"call","name":"play_relaxing_music","args":{},
            "requested_audio_context":offer.context})]
    );
    assert!(
        matches!(
            outputs.last(),
            Some(ProviderOutput::TurnFailed {
                request: 50,
                stage: FailedStage::AwaitingResponse,
                reason: Fault::Transport,
            })
        ),
        "{outputs:?}"
    );
    assert!(
        !outputs.iter().any(|event| matches!(
            event,
            ProviderOutput::Audio { .. }
                | ProviderOutput::GenerationComplete { .. }
                | ProviderOutput::HoldingAnswer { .. }
        )),
        "a failed confirmation supplies no playback or holding continuation: {outputs:?}"
    );
    assert_eq!(probes.load(Ordering::Relaxed), 0);
    assert_eq!(script.asked.lock().unwrap().len(), 2);
    assert_eq!(script.chats.lock().unwrap().len(), 2);
    assert_private_model(&script);
}
