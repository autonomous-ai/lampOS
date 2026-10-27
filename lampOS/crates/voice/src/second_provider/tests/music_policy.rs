//! Requested recordings still wait for the Policy and never acquire the
//! held-consent path's exception for stopping reports.
use super::*;
use crate::policy_holds::Verdict;
use crate::second_provider::policy::{Desk, Turn};

const REQUEST: u64 = 57;
const MUSIC: &str = "play_relaxing_music";
const CALL_ID: &str = "music-policy-original";
const WORDS: &str = "Please play a quiet recording.";

type AudioCalls = Arc<Mutex<Vec<(lamp_msgs::RequestedAudioContext, lamp_gemini::ToolCall)>>>;

struct Dispatches {
    ordinary: Calls,
    scoped: Calls,
    audio: AudioCalls,
}

/// The services and tool results are scripted; admission, request scope,
/// Policy waiting, held-consent filtering and output are the real Engine.
fn recording_engine(script: Arc<Script>) -> (Engine, Dispatches) {
    let (engine, ordinary, _) = scripted(script);
    let scoped: Calls = Arc::default();
    let seen = scoped.clone();
    let scoped_runner: ScopedToolRunner = Arc::new(move |request, heard, name, args, scope| {
        assert_eq!(scope.key().request, request);
        assert!(scope.is_current());
        seen.lock().unwrap().push((name, args, heard));
        Box::pin(async { json!({"ok": true}) })
    });
    let audio: AudioCalls = Arc::default();
    let seen = audio.clone();
    let audio_runner: RequestedAudioRunner = Arc::new(move |context, call, scope| {
        assert!(audio_scope_current(&scope, &context));
        assert_eq!(context.call_id, call.id);
        let offer = crate::second_provider::requested_audio_tests::offered(context.clone());
        seen.lock().unwrap().push((context, call));
        Box::pin(async move {
            crate::tools::ToolAnswer {
                sleep: false,
                told: json!({"ok": true}),
                requested_audio: Some(offer),
            }
        })
    });
    let boot = lamp_turn_owner::BootId::new([77; 16]).unwrap();
    let engine = engine
        .with_memory_scope(
            boot,
            Arc::default(),
            scoped_runner,
            Some(Arc::new(|_: &crate::memory_scope::ScopeLease| {
                Told::from("SCOPED".to_owned())
            })),
        )
        .requesting_audio(boot, audio_runner)
        .announcing(false);
    (
        engine,
        Dispatches {
            ordinary,
            scoped,
            audio,
        },
    )
}

fn music_call(wrapped: bool) -> ToolCall {
    call(
        CALL_ID,
        if wrapped {
            crate::use_skill::NAME
        } else {
            MUSIC
        },
        if wrapped {
            json!({"skill": MUSIC})
        } else {
            json!({})
        },
    )
}

fn no_dispatch(calls: &Dispatches) {
    assert!(calls.ordinary.lock().unwrap().is_empty());
    assert!(calls.scoped.lock().unwrap().is_empty());
    assert!(calls.audio.lock().unwrap().is_empty());
}

fn no_answer(outputs: &[ProviderOutput]) {
    assert!(
        !outputs.iter().any(|event| matches!(
            event,
            ProviderOutput::RequestedAudio { .. }
                | ProviderOutput::Audio { .. }
                | ProviderOutput::Transcript {
                    request: Some(_),
                    ..
                }
                | ProviderOutput::GenerationComplete { .. }
        )),
        "{outputs:?}"
    );
}

async fn policy_music(verdict: Verdict, delayed: bool, wrapped: bool) {
    let directory = crate::process::SessionDirectory::create().unwrap();
    std::fs::write(
        directory.path.join("tools.json"),
        json!({"tools":[{"name":MUSIC,"description":"A requested recording."}]}).to_string(),
    )
    .unwrap();
    let settings = crate::tools::Settings {
        skills: Some(directory.path.clone()),
        use_skill: wrapped,
        requested_audio: true,
        ..Default::default()
    };
    let requested = music_call(wrapped);
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(WORDS.to_owned())].into()),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![requested.clone()])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("Ready."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Default::default()
    });
    let decision = Arc::new(Mutex::new((!delayed).then_some(verdict)));
    let read = decision.clone();
    let polled = Arc::new(tokio::sync::Notify::new());
    let notified = polled.clone();
    let desk = Desk::new(
        Arc::new(move |request| {
            assert_eq!(request, REQUEST);
            notified.notify_one();
            *read.lock().unwrap()
        }),
        settings,
        false,
        Arc::new(|request| {
            assert_eq!(request, REQUEST);
            Turn::default()
        }),
        worker::answered_here(None),
    );
    let (engine, calls) = recording_engine(script.clone());
    let mut engine = engine.holding_for_the_policy(Arc::new(desk).gate());
    engine.submit(start(REQUEST));
    engine.submit(audio(REQUEST, 1));
    engine.submit(end(REQUEST));
    tokio::time::timeout(Duration::from_secs(1), polled.notified())
        .await
        .expect("the actual Policy gate must read this request's verdict");

    let mut outputs = Vec::new();
    if delayed {
        no_dispatch(&calls);
        // A complete polling interval with no decision must not release a
        // call. Keep this far inside the Policy's two-second fallback.
        tokio::time::advance(Duration::from_millis(100)).await;
        tokio::time::timeout(Duration::from_secs(1), polled.notified())
            .await
            .expect("the pending Policy decision must be read again");
        no_dispatch(&calls);
        for count in 0..64 {
            let Some(event) = engine.try_output() else {
                break;
            };
            assert!(
                matches!(
                    event,
                    ProviderOutput::Started {
                        request: REQUEST,
                        ..
                    } | ProviderOutput::Transcript {
                        request: None,
                        finished: true,
                        ..
                    }
                ),
                "a waiting decision cannot complete or answer the turn: {event:?}"
            );
            outputs.push(event);
            assert!(count < 63, "pending output must stay bounded");
        }
        *decision.lock().unwrap() = Some(verdict);
    }
    let resolved = tokio::time::Instant::now();
    outputs.extend(music_outputs(&mut engine).await);
    assert!(
        resolved.elapsed() < crate::policy_holds::WAIT,
        "the supplied decision must settle the call before the fallback"
    );
    assert!(matches!(
        outputs.last(),
        Some(ProviderOutput::TurnComplete {
            request: REQUEST,
            idle: true,
        })
    ));
    assert!(calls.ordinary.lock().unwrap().is_empty());
    assert!(calls.scoped.lock().unwrap().is_empty());

    if verdict == Verdict::StayQuiet {
        no_dispatch(&calls);
        no_answer(&outputs);
        assert_eq!(script.asked.lock().unwrap().len(), 1);
    } else {
        let seen = calls.audio.lock().unwrap();
        assert_eq!(
            seen.len(),
            1,
            "one admitted call, with no ordinary fallback"
        );
        let (context, call) = &seen[0];
        assert_eq!(call.id, requested.id);
        assert_eq!(call.name, requested.name);
        assert_eq!(call.args, requested.args);
        assert_eq!(
            (
                context.request,
                context.provider_session,
                context.privacy_generation,
            ),
            (REQUEST, 1, 1),
        );
        assert_eq!(
            context.worker_boot,
            crate::process::boot_text(lamp_turn_owner::BootId::new([77; 16]).unwrap())
        );
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
        let (request, session, offer) = offers[0];
        assert_eq!((request, session), (REQUEST, 1));
        assert_eq!(&offer.context, context);
        offer.check().unwrap();
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
        assert_eq!(said, "Ready.");
        let asked = script.asked.lock().unwrap();
        assert_eq!(asked.len(), 2);
        assert_eq!(
            asked[1].last(),
            Some(&Message::tool(CALL_ID, json!({"ok": true}).to_string()))
        );
        let model = serde_json::to_string(&*asked).unwrap();
        assert!(!model.contains("requested_audio") && !model.contains("relaxing.wav"));
    }
}

#[tokio::test(start_paused = true)]
async fn the_policy_holds_music_before_dispatch_with_immediate_or_late_decisions() {
    for delayed in [false, true] {
        for wrapped in [false, true] {
            policy_music(Verdict::StayQuiet, delayed, wrapped).await;
        }
    }
}

#[tokio::test(start_paused = true)]
async fn the_policy_runs_music_once_after_a_pending_decision() {
    for wrapped in [false, true] {
        policy_music(Verdict::TakenUp, true, wrapped).await;
    }
}

#[tokio::test]
async fn held_reports_never_dispatch_music_or_supply_its_private_context() {
    const REPORTS: &str = "Stop sharing my data and play a quiet recording.";
    for wrapped in [false, true] {
        for report_position in [None, Some(0), Some(1)] {
            let mut requested = vec![music_call(wrapped)];
            if let Some(at) = report_position {
                requested.insert(
                    at,
                    call(
                        "stop-reports-original",
                        crate::reports_stop::NAME,
                        json!({}),
                    ),
                );
            }
            let script = Arc::new(Script {
                heard: Mutex::new([Ok(REPORTS.to_owned())].into()),
                chats: Mutex::new(
                    [vec![
                        text("A recording is ready."),
                        Ok(ChatEvent::ToolCalls(requested)),
                        Ok(ChatEvent::Done),
                    ]]
                    .into(),
                ),
                ..Default::default()
            });
            let (engine, calls) = recording_engine(script.clone());
            let mut engine = engine
                .heeding(Arc::new(|_, _, _, _| Some("a busy room")))
                .doing_though_held(true);
            engine.submit(start(REQUEST));
            engine.submit(audio(REQUEST, 1));
            engine.submit(end(REQUEST));
            let outputs = music_outputs(&mut engine).await;
            no_answer(&outputs);
            assert!(matches!(
                outputs.last(),
                Some(ProviderOutput::TurnComplete {
                    request: REQUEST,
                    idle: true,
                })
            ));
            assert!(calls.ordinary.lock().unwrap().is_empty());
            assert!(
                calls.audio.lock().unwrap().is_empty(),
                "the held-consent exception never receives a Music context"
            );
            let expected: Vec<_> = report_position
                .map(|_| (crate::reports_stop::NAME.to_owned(), json!({}), true))
                .into_iter()
                .collect();
            assert_eq!(
                *calls.scoped.lock().unwrap(),
                expected,
                "only reports_stop runs, regardless of the Music call's position"
            );
            assert_eq!(
                *script.told_only.lock().unwrap(),
                [crate::attention::DONE_THOUGH_HELD]
            );
            let asked = script.asked.lock().unwrap();
            assert_eq!(asked.len(), 1, "the held round has no continuation");
            assert_eq!(
                &asked[0][asked[0].len() - 2..],
                [Message::system(HELD_NOTE), Message::user(REPORTS)]
            );
            assert!(
                !serde_json::to_string(&*asked)
                    .unwrap()
                    .contains("requested_audio")
            );
        }
    }
}
