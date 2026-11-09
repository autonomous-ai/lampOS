//! Forget cancels provider-owned speech without retiring the current request.
use super::*;

const PRIVATE: &str = "The secret orchid is violet.";
const FRESH: &str = "Done.";
const REQUEST: u64 = 971;

fn erasing(id: &str) -> net::Result<ChatEvent> {
    Ok(ChatEvent::ToolCalls(vec![call(
        id,
        "forget",
        json!({"about": "orchid"}),
    )]))
}

fn engine(script: Arc<Script>) -> (Engine, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let runner: ToolRunner = Arc::new(move |request, heard, name, _| {
        assert_eq!(request, REQUEST);
        assert!(heard, "the forget must be admitted on a spoken request");
        assert_eq!(name, "forget");
        observed.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { json!({"forgotten": true}) })
    });
    let engine =
        Engine::new(script, || "Static character.".to_owned().into(), runner).announcing(false);
    (engine, calls)
}

async fn finish(engine: &mut Engine) -> Vec<ProviderOutput> {
    engine.submit(start(REQUEST));
    engine.submit(audio(REQUEST, 1));
    engine.submit(end(REQUEST));
    tokio::time::timeout(Duration::from_secs(2), async {
        let mut events = Vec::new();
        loop {
            let event = engine.next().await;
            assert!(
                !matches!(event, ProviderOutput::TurnFailed { .. }),
                "{event:?}"
            );
            let done = matches!(
                event,
                ProviderOutput::TurnComplete {
                    request: REQUEST,
                    ..
                }
            );
            events.push(event);
            if done {
                return events;
            }
        }
    })
    .await
    .expect("forget must not wait for the old speech service")
}

fn assert_fresh_audio(events: &[ProviderOutput]) {
    let mut samples = Vec::new();
    let mut fresh = 0;
    for event in events {
        match event {
            ProviderOutput::Audio {
                request,
                samples: part,
                ..
            } => {
                assert_eq!(*request, REQUEST, "fresh speech keeps the same request");
                samples.extend_from_slice(part);
            }
            ProviderOutput::Transcript {
                request: Some(request),
                text,
                finished: false,
            } if text.trim() == FRESH => {
                assert_eq!(*request, REQUEST);
                fresh += 1;
            }
            _ => {}
        }
    }
    assert_eq!(fresh, 1, "{events:?}");
    assert_eq!(
        samples,
        vec![FRESH.len() as i16; 1_500],
        "old or joined speech escaped"
    );
}

#[tokio::test(start_paused = true)]
async fn stalled_private_tts_is_dropped_and_fresh_reply_finishes_same_request() {
    let (release, held) = tokio::sync::oneshot::channel();
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Forget the orchid and say done.".into())].into()),
        chats: Mutex::new(
            [
                vec![
                    text(&format!("{PRIVATE} ")),
                    erasing("erase"),
                    Ok(ChatEvent::Done),
                ],
                vec![text(FRESH), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        // Separate chat events let saying take the old sentence before erase.
        piece_gap: Some(Duration::from_millis(10)),
        speech_hold: Mutex::new(Some((PRIVATE, held))),
        ..Script::default()
    });
    let (mut engine, calls) = engine(script.clone());
    let events = finish(&mut engine).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        script
            .spoken
            .lock()
            .unwrap()
            .iter()
            .any(|line| line == PRIVATE)
    );
    assert!(
        events.iter().any(|event| matches!(event,
        ProviderOutput::Transcript { text, finished: false, .. } if text.trim() == PRIVATE)),
        "fixture must actually enter saying with the old sentence"
    );
    assert_fresh_audio(&events);
    assert!(
        release.send(()).is_err(),
        "cancelled old TTS must release its waiter"
    );
}

#[tokio::test(start_paused = true)]
async fn unsaid_private_piece_is_not_joined_to_a_post_forget_reply() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Forget the orchid and say done.".into())].into()),
        chats: Mutex::new(
            [
                vec![
                    text(&format!("{PRIVATE} ")),
                    erasing("erase"),
                    Ok(ChatEvent::Done),
                ],
                vec![text(FRESH), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        piece_gap: Some(Duration::from_millis(10)),
        unsaid: &[PRIVATE],
        ..Script::default()
    });
    let (mut engine, calls) = engine(script.clone());
    let events = finish(&mut engine).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_fresh_audio(&events);
    assert_eq!(*script.spoken.lock().unwrap(), [PRIVATE, FRESH]);
}

#[tokio::test(start_paused = true)]
async fn repeated_forgets_clear_each_unsaid_generation() {
    const SECOND: &str = "Another private sentence.";
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Forget both things and say done.".into())].into()),
        chats: Mutex::new(
            [
                vec![
                    text(&format!("{PRIVATE} ")),
                    erasing("first"),
                    Ok(ChatEvent::Done),
                ],
                vec![
                    text(&format!("{SECOND} ")),
                    erasing("second"),
                    Ok(ChatEvent::Done),
                ],
                vec![text(FRESH), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        piece_gap: Some(Duration::from_millis(10)),
        unsaid: &[PRIVATE, SECOND],
        ..Script::default()
    });
    let (mut engine, calls) = engine(script.clone());
    let events = finish(&mut engine).await;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_fresh_audio(&events);
    assert_eq!(*script.spoken.lock().unwrap(), [PRIVATE, SECOND, FRESH]);
}

#[tokio::test(start_paused = true)]
async fn forget_then_policy_end_closes_without_draining_stalled_old_speech() {
    let (release, held) = tokio::sync::oneshot::channel();
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Forget the orchid, then set a timer.".into())].into()),
        chats: Mutex::new(
            [vec![
                text(&format!("{PRIVATE} ")),
                Ok(ChatEvent::ToolCalls(vec![
                    call("erase", "forget", json!({})),
                    call("held", "set_timer", json!({"seconds": 60})),
                ])),
                Ok(ChatEvent::Done),
            ]]
            .into(),
        ),
        piece_gap: Some(Duration::from_millis(10)),
        speech_hold: Mutex::new(Some((PRIVATE, held))),
        ..Script::default()
    });
    let (engine, calls) = engine(script.clone());
    let mut engine = engine.holding_for_the_policy(Arc::new(|_, name| {
        Box::pin(async move {
            if name == "set_timer" {
                Policed::Held
            } else {
                Policed::Run
            }
        })
    }));
    let events = finish(&mut engine).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(script.asked.lock().unwrap().len(), 1);
    assert!(
        events
            .iter()
            .all(|event| !matches!(event, ProviderOutput::Audio { .. }))
    );
    assert!(release.send(()).is_err());
}

#[tokio::test(start_paused = true)]
async fn blocked_output_and_replay_copies_are_cleared_even_if_forget_is_uncertain() {
    for lasting in [None, Some((PRIVATE, PACKET_SAMPLES * 32))] {
        for outcome in [json!({"forgotten": true}), json!({"error": "uncertain"})] {
            let script = Arc::new(Script {
                heard: Mutex::new([Ok("Forget the orchid and say done.".into())].into()),
                chats: Mutex::new(
                    [
                        vec![
                            text(&format!("{PRIVATE} ")),
                            erasing("erase"),
                            Ok(ChatEvent::Done),
                        ],
                        vec![text(FRESH), Ok(ChatEvent::Done)],
                    ]
                    .into(),
                ),
                piece_gap: Some(Duration::from_millis(100)),
                lasting,
                ..Script::default()
            });
            let (admit, admitted) = tokio::sync::oneshot::channel();
            let (release, wait) = tokio::sync::oneshot::channel();
            let gate = Arc::new(Mutex::new(Some((admit, wait))));
            let runner: ToolRunner = Arc::new(move |request, heard, name, _| {
                assert_eq!(request, REQUEST);
                assert!(heard);
                assert_eq!(name, "forget");
                let (admit, wait) = gate.lock().unwrap().take().expect("one forget");
                let outcome = outcome.clone();
                Box::pin(async move {
                    admit.send(()).unwrap();
                    wait.await.unwrap();
                    outcome
                })
            });
            let mut engine = Engine::new(script, || "Static character.".to_owned().into(), runner)
                .announcing(false)
                .resuming_after_wordless_cut_ins(true)
                .holding_own_echo(true);
            let shared = engine.shared.clone();
            if lasting.is_none() {
                // One queued metadata event leaves room for only the first
                // chunk after the heard and spoken transcripts arrive.
                shared
                    .out
                    .try_send(ProviderOutput::GenerationComplete { request: REQUEST }.into())
                    .unwrap();
            }
            engine.submit(start(REQUEST));
            engine.submit(audio(REQUEST, 1));
            engine.submit(end(REQUEST));
            // No Engine output is consumed: saying must fill the bounded channel.
            tokio::time::sleep(Duration::from_millis(150)).await;
            assert_eq!(shared.out.capacity(), 0, "fixture must block speech output");
            let samples_before: u64 = shared
                .said
                .lock()
                .unwrap()
                .1
                .iter()
                .map(|p| p.samples)
                .sum();
            let tail_before = shared.said_until_us.load(Ordering::SeqCst);
            assert!(samples_before > 0);
            assert!(
                tail_before > 0,
                "even a partially emitted TTS packet must establish an echo tail"
            );
            if lasting.is_none() {
                assert!(
                    samples_before < 1_500,
                    "fixture must block midway through the single TTS packet"
                );
            }
            assert_ne!(shared.cut_rest(REQUEST + 1), CutRest::Nothing);
            assert!(!shared.written.lock().unwrap().pieces.is_empty());
            tokio::time::timeout(Duration::from_secs(1), admitted)
                .await
                .unwrap()
                .unwrap();
            assert!(shared.written.lock().unwrap().pieces.is_empty());
            assert!(shared.lamp_said.lock().unwrap().1.is_empty());
            assert!(
                shared
                    .said
                    .lock()
                    .unwrap()
                    .1
                    .iter()
                    .all(|p| p.words.is_none())
            );
            assert_eq!(shared.cut_rest(REQUEST + 1), CutRest::Nothing);
            let samples_after: u64 = shared
                .said
                .lock()
                .unwrap()
                .1
                .iter()
                .map(|p| p.samples)
                .sum();
            assert!(
                samples_after >= samples_before,
                "already queued sound still counts conservatively"
            );
            assert!(shared.said_until_us.load(Ordering::SeqCst) >= tail_before);
            while let Some(event) = engine.try_output() {
                assert!(!matches!(event, ProviderOutput::Audio { .. }));
                assert!(!matches!(
                    event,
                    ProviderOutput::Transcript {
                        finished: false,
                        ..
                    }
                ));
            }
            release.send(()).unwrap();
            let events = tokio::time::timeout(Duration::from_secs(2), async {
                let mut events = Vec::new();
                loop {
                    let event = engine.next().await;
                    let done = matches!(
                        event,
                        ProviderOutput::TurnComplete {
                            request: REQUEST,
                            ..
                        }
                    );
                    assert!(
                        !matches!(event, ProviderOutput::TurnFailed { .. }),
                        "{event:?}"
                    );
                    events.push(event);
                    if done {
                        break events;
                    }
                }
            })
            .await
            .unwrap();
            assert_fresh_audio(&events);
        }
    }
}

#[tokio::test(start_paused = true)]
async fn unasked_forget_held_for_confirmation_does_not_cancel_existing_speech() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Tell me about the orchid.".into())].into()),
        chats: Mutex::new(
            [
                vec![
                    text(&format!("{PRIVATE} ")),
                    erasing("held"),
                    Ok(ChatEvent::Done),
                ],
                vec![Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        piece_gap: Some(Duration::from_millis(10)),
        speech_after: Some(Duration::from_millis(100)),
        ..Script::default()
    });
    let (mut engine, calls) = engine(script);
    let events = finish(&mut engine).await;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let samples: Vec<i16> = events
        .iter()
        .filter_map(|event| match event {
            ProviderOutput::Audio { samples, .. } => Some(samples.as_slice()),
            _ => None,
        })
        .flatten()
        .copied()
        .collect();
    assert!(
        samples
            .windows(1_500)
            .any(|part| part == vec![PRIVATE.len() as i16; 1_500]),
        "old speech must remain available when forget was not admitted"
    );
}

#[tokio::test]
async fn queue_forget_bookkeeping_timing_and_tag_reset() {
    let (engine, _) = engine(Arc::new(Script::default()));
    let engine = engine
        .resuming_after_wordless_cut_ins(true)
        .speaking_tags(Some(true));
    let shared = engine.shared.clone();
    let mut tasks = JoinSet::new();
    let (sentences, _received) = mpsc::channel(2);
    let generation = shared.speech_generation(REQUEST);
    let mut queue = Queue {
        line: String::new(),
        shared: &shared,
        speech: &mut tasks,
        announcement: None,
        sentences,
        generation,
        started: false,
        request: REQUEST,
        heard_at: Instant::now(),
        tags: Some(speech_tags::Tags::new(true)),
    };
    let mut times = Vec::new();
    for _ in 0..1_001 {
        shared.wrote_piece(REQUEST, PRIVATE);
        shared.said_piece(REQUEST, Some(PRIVATE));
        shared.said_samples(REQUEST, 1_500);
        *shared.lamp_said.lock().unwrap() = (REQUEST, PRIVATE.into());
        queue.started = true;
        queue.tags.as_mut().unwrap().piece("[whis");
        assert!(!shared.said.lock().unwrap().2.keeps(REQUEST, "set_timer{"));
        let old = queue.generation.borrow().clone();
        let start = std::time::Instant::now();
        queue.forget();
        times.push(start.elapsed());
        assert!(
            shared.said.lock().unwrap().2.keeps(REQUEST, "Done."),
            "old bare-call parser state must not swallow fresh words"
        );
        assert!(!old.current());
        assert!(queue.generation.borrow().current());
        assert!(!queue.started);
        assert_eq!(
            queue.tags.as_mut().unwrap().piece("Done."),
            ("Done.".into(), "Done.".into())
        );
        assert!(shared.written.lock().unwrap().pieces.is_empty());
        // Keep each sample's setup bounded; cumulative stress is a separate boundary.
        shared.said.lock().unwrap().1.clear();
    }
    let first = times.remove(0);
    times.sort_unstable();
    eprintln!(
        "Queue::forget local bookkeeping, one sentence/no tasks, first={first:?}, repeated n=1000 median={:?} p99={:?} max={:?}",
        times[500], times[990], times[999]
    );
}

#[tokio::test]
async fn queued_cue_and_prelude_are_invalidated_but_fresh_and_lifecycle_output_survive() {
    let (mut engine, _) = engine(Arc::new(Script::default()));
    let shared = engine.shared.clone();
    let mut tasks = JoinSet::new();
    let (sentences, _received) = mpsc::channel(2);
    let generation = shared.speech_generation(REQUEST);
    let mut queue = Queue {
        line: String::new(),
        shared: &shared,
        speech: &mut tasks,
        announcement: None,
        sentences,
        generation,
        started: false,
        request: REQUEST,
        heard_at: Instant::now(),
        tags: None,
    };
    assert!(cue(&shared, REQUEST, vec![3; 120]).await.is_ok());
    assert!(
        shared
            .emit(ProviderOutput::Transcript {
                request: Some(REQUEST),
                text: PRIVATE.into(),
                finished: false,
            })
            .await
    );
    // Lifecycle events retain their existing lifetime.
    assert!(
        shared
            .emit(ProviderOutput::GenerationComplete { request: REQUEST })
            .await
    );
    assert_eq!(shared.out.capacity(), OUTPUT_QUEUE - 3);
    queue.forget();
    assert!(cue(&shared, REQUEST, vec![7; 120]).await.is_ok());
    let mut samples = Vec::new();
    let mut completed = 0;
    while let Some(event) = engine.try_output() {
        match event {
            ProviderOutput::Audio {
                request,
                samples: part,
                ..
            } => {
                assert_eq!(request, REQUEST);
                samples.extend(part);
            }
            ProviderOutput::GenerationComplete { request: REQUEST } => completed += 1,
            other => panic!("unexpected cue output: {other:?}"),
        }
    }
    assert_eq!(samples, vec![7; 120]);
    assert_eq!(completed, 1);
}

#[tokio::test]
async fn older_request_cannot_replace_the_latest_speech_fence() {
    let (mut engine, _) = engine(Arc::new(Script::default()));
    let shared = engine.shared.clone();
    let latest = shared.speech_generation(REQUEST + 1);
    assert!(
        shared
            .emit(ProviderOutput::Transcript {
                request: Some(REQUEST + 1),
                text: PRIVATE.into(),
                finished: false,
            })
            .await
    );
    let older = shared.speech_generation(REQUEST);
    older.borrow().invalidate();
    assert!(
        latest.borrow().current(),
        "an older request has its own token"
    );
    assert_eq!(
        shared.speech_generation.lock().unwrap().as_ref().unwrap().0,
        REQUEST + 1
    );
    latest.borrow().invalidate();
    assert!(
        !shared.speech_generation(REQUEST + 1).borrow().current(),
        "looking up an older request must not detach the latest fence"
    );
    assert!(
        engine.try_output().is_none(),
        "latest queued text remains fenced"
    );
}

/// Both old sentences precede one forget; the second waits behind stalled TTS.
#[tokio::test(start_paused = true)]
async fn one_forget_discards_a_second_private_sentence_queued_behind_stalled_tts() {
    const SECOND: &str = "The second orchid is amber.";
    let (release, held) = tokio::sync::oneshot::channel();
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Forget the orchid and say done.".into())].into()),
        chats: Mutex::new(
            [
                vec![
                    text(&format!("{PRIVATE} ")),
                    text(&format!("{SECOND} ")),
                    erasing("erase-both"),
                    Ok(ChatEvent::Done),
                ],
                vec![text(FRESH), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        // Whitespace completes each sentence; separate events give its TTS
        // task time to start before the single forget is dispatched.
        piece_gap: Some(Duration::from_millis(10)),
        speech_hold: Mutex::new(Some((PRIVATE, held))),
        ..Script::default()
    });
    let (mut engine, calls) = engine(script.clone());
    let events = finish(&mut engine).await;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "one forget for both sentences"
    );
    assert_eq!(
        *script.spoken.lock().unwrap(),
        [PRIVATE, SECOND, FRESH],
        "both private sentences must actually start TTS before the fresh reply"
    );
    assert!(
        script.speech_hold.lock().unwrap().is_none(),
        "the first private TTS must take its blocking receiver"
    );
    assert!(
        events.iter().any(|event| matches!(event,
            ProviderOutput::Transcript { text, finished: false, .. }
                if text.trim() == PRIVATE)),
        "saying must actually reach the stalled first sentence"
    );
    assert!(
        !events.iter().any(|event| matches!(event,
            ProviderOutput::Transcript { text, finished: false, .. }
                if text.trim() == SECOND)),
        "the queued second sentence must never reach saying"
    );
    assert_fresh_audio(&events);
    assert!(
        release.send(()).is_err(),
        "forget must cancel the old TTS waiter without releasing it"
    );
}

/// A real confirmation question followed by the same known person's yes must
/// invalidate that yes request's queued speech before the held runner awaits I/O.
#[tokio::test]
async fn confirmed_held_memory_forgets_cut_yes_request_speech_before_runner_io() {
    for (name, words, args, question) in [
        (
            crate::remember::FORGET,
            "Tell me about the orchid.",
            json!({"about":"orchid"}),
            "Shall I forget the orchid?",
        ),
        (
            crate::remember::FORGET_EVERYTHING,
            "Forget everything about me.",
            json!({}),
            "Shall I forget everything I know about you, including your name?",
        ),
    ] {
        let script = Arc::new(Script {
            heard: Mutex::new([Ok(words.to_owned()), Ok("Yes.".to_owned())].into()),
            chats: Mutex::new(
                [
                    vec![
                        Ok(ChatEvent::ToolCalls(vec![call("held-speech", name, args)])),
                        Ok(ChatEvent::Done),
                    ],
                    vec![text(question), Ok(ChatEvent::Done)],
                    vec![text(FRESH), Ok(ChatEvent::Done)],
                ]
                .into(),
            ),
            ..Script::default()
        });
        let path = std::env::temp_dir().join(format!(
            "held-speech-{}-{:?}-{name}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&path, "A").unwrap();
        let cache = Arc::new(crate::memory_scope::FaceCache::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let token = Arc::new(Mutex::new(None::<speech_generation::Generation>));
        let probe = token.clone();
        let (admit, admitted) = tokio::sync::oneshot::channel();
        let admit = Arc::new(Mutex::new(Some(admit)));
        let (release, released) = tokio::sync::oneshot::channel();
        let released = Arc::new(Mutex::new(Some(released)));
        let runner: ScopedToolRunner = Arc::new(move |request, heard, called, _, scope| {
            assert_eq!(
                request, REQUEST,
                "the question itself must not run a forget"
            );
            assert!(heard);
            assert_eq!(called, name);
            assert!(scope.is_current());
            assert_eq!(
                scope.scope().and_then(|scope| scope.person()),
                Some(crate::remember::person_id("A").as_str())
            );
            if name == crate::remember::FORGET_EVERYTHING {
                assert!(
                    scope.everything_marked(),
                    "the actual yes must authorize everything"
                );
            }
            // This is synchronous runner entry, before constructing its awaited future.
            assert!(
                !probe.lock().unwrap().as_ref().unwrap().current(),
                "held {name} must invalidate the yes-request generation before tool I/O"
            );
            seen.fetch_add(1, Ordering::SeqCst);
            let admit = admit.lock().unwrap().take().unwrap();
            let wait = released.lock().unwrap().take().unwrap();
            admit.send(()).unwrap();
            Box::pin(async move {
                wait.await.unwrap();
                json!({"forgotten":true,"everything":true})
            })
        });
        let mut engine = Engine::with_clock(
            script,
            Arc::new(|| "Static character.".to_owned().into()),
            Arc::new(|_, _, _, _| panic!("held forget must use its scoped runner")),
            Arc::new(Instant::now),
        )
        .announcing(false)
        .with_memory_scope(
            lamp_turn_owner::BootId::new([12; 16]).unwrap(),
            cache.clone(),
            runner,
            None,
        );
        engine.memory_camera(true, 1);
        cache.replace(
            cache.begin_refresh(),
            crate::known_faces::FaceObservation::read(&path),
        );
        let first = REQUEST - 1;
        engine.submit(start(first));
        engine.submit(audio(first, 1));
        engine.submit(end(first));
        let asked = tokio::time::timeout(Duration::from_secs(3), async {
            let mut asked = Vec::new();
            loop {
                match engine.next().await {
                    ProviderOutput::Transcript {
                        request: Some(request),
                        text,
                        finished: false,
                    } if request == first => asked.push(text),
                    ProviderOutput::TurnComplete { request, .. } if request == first => {
                        break asked;
                    }
                    ProviderOutput::TurnFailed { .. } => panic!("confirmation question failed"),
                    _ => {}
                }
            }
        })
        .await
        .expect("the real confirmation question finishes");
        // A long question may be emitted in multiple speech chunks.
        let asked = asked.join(" ");
        assert_eq!(
            asked.split_whitespace().collect::<Vec<_>>(),
            question.split_whitespace().collect::<Vec<_>>(),
            "the whole confirmation question must be emitted before their yes"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        engine.retire_through(first);
        cache.replace(
            cache.begin_refresh(),
            crate::known_faces::FaceObservation::read(&path),
        );
        let shared = engine.shared.clone();
        let old = shared.speech_generation(REQUEST).borrow().clone();
        *token.lock().unwrap() = Some(old.clone());
        assert!(
            shared
                .emit(ProviderOutput::Transcript {
                    request: Some(REQUEST),
                    text: PRIVATE.into(),
                    finished: false,
                })
                .await
        );
        assert!(
            shared
                .emit(ProviderOutput::Audio {
                    request: REQUEST,
                    sequence: shared.sequence.fetch_add(1, Ordering::Relaxed) + 1,
                    provider_event_at_us: monotonic_us(),
                    source_frames: 120,
                    source_offset: 0,
                    samples: vec![-123; 120],
                })
                .await
        );
        assert!(
            old.current(),
            "queued preludes are live before confirmation"
        );
        assert_eq!(shared.out.capacity(), OUTPUT_QUEUE - 2);
        engine.submit(start(REQUEST));
        engine.submit(audio(REQUEST, 1));
        engine.submit(end(REQUEST));
        // Consume no output until actual admission: the old envelopes must still
        // be queued when the held-forget branch invalidates their generation.
        tokio::time::timeout(Duration::from_secs(3), admitted)
            .await
            .expect("the same person's yes admits the held forget")
            .expect("runner observed synchronous invalidation");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!old.current());
        let mut events = Vec::new();
        while let Some(event) = engine.try_output() {
            assert!(
                !matches!(event, ProviderOutput::Audio { .. }),
                "old queued audio survived: {event:?}"
            );
            if let ProviderOutput::Transcript {
                text,
                finished: false,
                ..
            } = &event
            {
                assert!(!text.contains(PRIVATE), "old queued words survived");
            }
            events.push(event);
        }
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let event = engine.next().await;
                assert!(
                    !matches!(event, ProviderOutput::TurnFailed { .. }),
                    "{event:?}"
                );
                let done = matches!(
                    event,
                    ProviderOutput::TurnComplete {
                        request: REQUEST,
                        ..
                    }
                );
                events.push(event);
                if done {
                    break;
                }
            }
        })
        .await
        .expect("fresh speech completes on the same yes request");
        assert_fresh_audio(&events);
        std::fs::remove_file(path).unwrap();
    }
}
