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
        shared: &shared,
        speech: &mut tasks,
        announcement: None,
        sentences,
        generation,
        started: false,
        request: REQUEST,
        heard_at: Instant::now(),
        tags: Some(speech_tags::Tags::new(true)),
        line: String::new(),
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
        shared: &shared,
        speech: &mut tasks,
        announcement: None,
        sentences,
        generation,
        started: false,
        request: REQUEST,
        heard_at: Instant::now(),
        tags: None,
        line: String::new(),
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

/// Probe the actual admission boundary, not the tool's successful return:
/// the runner remains blocked while old queued output and its token are checked.
async fn admitted_named_forget_cuts_speech(name: &'static str, heard: &'static str) {
    const QUEUED_PRIVATE: &str = "The second orchid is amber.";
    for stalled in [true, false] {
        let (tts_release, tts_wait) = tokio::sync::oneshot::channel();
        let script = Arc::new(Script {
            heard: Mutex::new([Ok(heard.into())].into()),
            chats: Mutex::new(
                [
                    vec![
                        text(&format!("{PRIVATE} ")),
                        text(&format!("{QUEUED_PRIVATE} ")),
                        Ok(ChatEvent::ToolCalls(vec![call(
                            "erase-named",
                            name,
                            json!({"name":"A"}),
                        )])),
                        Ok(ChatEvent::Done),
                    ],
                    vec![text(FRESH), Ok(ChatEvent::Done)],
                ]
                .into(),
            ),
            piece_gap: Some(Duration::from_millis(100)),
            speech_hold: Mutex::new(stalled.then_some((PRIVATE, tts_wait))),
            lasting: (!stalled).then_some((PRIVATE, PACKET_SAMPLES * 32)),
            ..Script::default()
        });
        let token: Arc<Mutex<Option<speech_generation::Generation>>> = Arc::default();
        let (admit, admitted) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let gate = Arc::new(Mutex::new(Some((admit, released))));
        let observed = token.clone();
        let runner: ToolRunner = Arc::new(move |request, spoken, called, _| {
            assert_eq!(request, REQUEST);
            assert!(spoken);
            assert_eq!(called, name);
            // Synchronous runner entry precedes polling the deletion future.
            let invalidated = !observed.lock().unwrap().as_ref().unwrap().current();
            let (admit, released) = gate
                .lock()
                .unwrap()
                .take()
                .expect("exactly one admitted forget");
            admit.send(invalidated).unwrap();
            Box::pin(async move {
                released.await.unwrap();
                if name == crate::known_faces::FORGET {
                    json!({"ok":true})
                } else {
                    json!({"forgotten":true})
                }
            })
        });
        let mut engine = Engine::new(
            script.clone(),
            || "Static character.".to_owned().into(),
            runner,
        )
        .announcing(false);
        let old = engine.shared.speech_generation(REQUEST).borrow().clone();
        *token.lock().unwrap() = Some(old.clone());
        engine.submit(start(REQUEST));
        engine.submit(audio(REQUEST, 1));
        engine.submit(end(REQUEST));
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(
            script
                .spoken
                .lock()
                .unwrap()
                .iter()
                .any(|line| line == PRIVATE)
        );
        if stalled {
            assert!(
                script.speech_hold.lock().unwrap().is_none(),
                "old TTS must be waiting"
            );
        } else {
            assert_eq!(
                engine.shared.out.capacity(),
                0,
                "old output must actually saturate the queue"
            );
        }
        let invalidated_before_io = tokio::time::timeout(Duration::from_secs(1), admitted)
            .await
            .unwrap()
            .unwrap();
        assert!(
            script
                .spoken
                .lock()
                .unwrap()
                .iter()
                .any(|line| line == QUEUED_PRIVATE),
            "second old sentence must actually be queued"
        );
        assert!(
            invalidated_before_io,
            "{name}: speech must be invalidated before entering the runner, stalled={stalled}"
        );
        assert!(!old.current());
        let cut = engine
            .speech_cut()
            .expect("admitted forget must publish the playback cut before tool I/O completes");
        assert_eq!(cut.request, REQUEST);
        if !stalled {
            assert!(cut.through > 0);
        }
        while let Some(event) = engine.try_output() {
            assert!(!matches!(event, ProviderOutput::Audio { .. }));
            assert!(
                !matches!(event, ProviderOutput::Transcript { finished:false, text, .. } if text.trim() == PRIVATE || text.trim() == QUEUED_PRIVATE)
            );
        }
        release.send(()).unwrap();
        let events = collect_named_forget(&mut engine, REQUEST).await;
        assert_fresh_audio(&events);
        if stalled {
            assert!(
                tts_release.send(()).is_err(),
                "old stalled stream must be cancelled"
            );
        }
    }
}

async fn collect_named_forget(engine: &mut Engine, request: u64) -> Vec<ProviderOutput> {
    tokio::time::timeout(Duration::from_secs(2), async {
        let mut events = Vec::new();
        loop {
            let event = engine.next().await;
            assert!(
                !matches!(event, ProviderOutput::TurnFailed { .. }),
                "{event:?}"
            );
            let done =
                matches!(event, ProviderOutput::TurnComplete { request: of, .. } if of == request);
            events.push(event);
            if done {
                return events;
            }
        }
    })
    .await
    .expect("cancelled old speech must not hold the fresh same-request reply")
}

#[tokio::test(start_paused = true)]
async fn face_forget_cuts_stalled_and_queued_speech_before_runner_io() {
    admitted_named_forget_cuts_speech(crate::known_faces::FORGET, "Forget my face and say done.")
        .await;
}

#[tokio::test(start_paused = true)]
async fn forget_everything_cuts_stalled_and_queued_speech_before_runner_io() {
    // No known-person scope: this exercises ordinary admission; the runner
    // may refuse storage, but admission already fences prepared speech.
    admitted_named_forget_cuts_speech(
        crate::remember::FORGET_EVERYTHING,
        "Forget everything about me and say done.",
    )
    .await;
}

#[tokio::test(start_paused = true)]
async fn unasked_face_forget_does_not_fence_or_cancel_old_speech() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Tell me about the orchid.".into())].into()),
        chats: Mutex::new(
            [
                vec![
                    text(&format!("{PRIVATE} ")),
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "unasked-face",
                        crate::known_faces::FORGET,
                        json!({"name":"A"}),
                    )])),
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
    let runner: ToolRunner =
        Arc::new(|_, _, _, _| panic!("an unasked face forget must not reach the runner"));
    let mut engine =
        Engine::new(script, || "Static character.".to_owned().into(), runner).announcing(false);
    let old = engine.shared.speech_generation(REQUEST).borrow().clone();
    let events = finish(&mut engine).await;
    assert!(old.current());
    assert!(engine.speech_cut().is_none());
    let samples: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            ProviderOutput::Audio { samples, .. } => Some(samples.as_slice()),
            _ => None,
        })
        .flatten()
        .copied()
        .collect();
    assert_eq!(samples, vec![PRIVATE.len() as i16; 1_500]);
}

#[tokio::test(start_paused = true)]
async fn held_yes_face_forget_fences_queued_prelude_before_runner_io() {
    struct KnownFile(std::path::PathBuf);
    impl Drop for KnownFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let known = KnownFile(std::env::temp_dir().join(format!(
        "face-speech-held-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    )));
    std::fs::write(&known.0, "A").unwrap();
    let cache = Arc::new(crate::memory_scope::FaceCache::default());
    cache.replace(
        cache.begin_refresh(),
        crate::known_faces::FaceObservation::read(&known.0),
    );
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Who am I?".into()), Ok("Yes.".into())].into()),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "held-face",
                        crate::known_faces::FORGET,
                        json!({"name":"A"}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("Shall I forget your face?"), Ok(ChatEvent::Done)],
                vec![text(FRESH), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let token: Arc<Mutex<Option<speech_generation::Generation>>> = Arc::default();
    let observed = token.clone();
    let (admit, admitted) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let gate = Arc::new(Mutex::new(Some((admit, released))));
    let runner: ScopedToolRunner = Arc::new(move |request, heard, name, _, scope| {
        assert_eq!(
            request, REQUEST,
            "the original unasked call must remain held"
        );
        assert!(heard);
        assert_eq!(name, crate::known_faces::FORGET);
        assert_eq!(
            scope.scope().unwrap().person(),
            Some(crate::remember::person_id("A").as_str())
        );
        let invalidated = !observed.lock().unwrap().as_ref().unwrap().current();
        let (admit, released) = gate
            .lock()
            .unwrap()
            .take()
            .expect("one confirmed face forget");
        admit.send(invalidated).unwrap();
        Box::pin(async move {
            released.await.unwrap();
            json!({"ok":true})
        })
    });
    let mut engine = Engine::new(
        script.clone(),
        || "Static character.".to_owned().into(),
        Arc::new(|_, _, _, _| panic!("face call must use its admitted scope")),
    )
    .with_memory_scope(
        lamp_turn_owner::BootId::new([174; 16]).unwrap(),
        cache.clone(),
        runner,
        None,
    )
    .announcing(false);
    engine.memory_camera(true, 1);
    // Camera admission starts a new cache epoch; publish the observed face after it.
    cache.replace(
        cache.begin_refresh(),
        crate::known_faces::FaceObservation::read(&known.0),
    );
    engine.submit(start(REQUEST - 1));
    engine.submit(audio(REQUEST - 1, 1));
    engine.submit(end(REQUEST - 1));
    collect_named_forget(&mut engine, REQUEST - 1).await;
    assert!(
        engine.speech_cut().is_none(),
        "holding the original call must not cut speech"
    );
    {
        let held = engine.shared.held_forget.lock().unwrap();
        let held = held
            .held
            .as_ref()
            .expect("real first request must hold the call");
        assert!(
            held.asked,
            "the question must have been spoken before yes can admit it"
        );
        assert_eq!(held.call.name, crate::known_faces::FORGET);
    }
    // Held calls run before round one. Queue the same-request fixed-prelude
    // output through Shared::emit, the path a prelude uses before answer().
    let old = engine.shared.speech_generation(REQUEST).borrow().clone();
    *token.lock().unwrap() = Some(old.clone());
    let sequence = engine.shared.sequence.fetch_add(1, Ordering::Relaxed) + 1;
    assert!(
        engine
            .shared
            .emit(ProviderOutput::Transcript {
                request: Some(REQUEST),
                text: PRIVATE.into(),
                finished: false,
            })
            .await
    );
    assert!(
        engine
            .shared
            .emit(ProviderOutput::Audio {
                request: REQUEST,
                sequence,
                provider_event_at_us: monotonic_us(),
                source_frames: PACKET_SAMPLES,
                source_offset: 0,
                samples: vec![7; PACKET_SAMPLES],
            })
            .await
    );
    engine.submit(start(REQUEST));
    engine.submit(audio(REQUEST, 1));
    engine.submit(end(REQUEST));
    let invalidated_before_io = tokio::time::timeout(Duration::from_secs(1), admitted)
        .await
        .unwrap()
        .unwrap();
    assert!(
        invalidated_before_io,
        "held yes must fence old speech before polling the deletion future"
    );
    assert!(!old.current());
    let cut = engine
        .speech_cut()
        .expect("held face forget must publish a playback cut");
    assert_eq!(cut.request, REQUEST);
    assert!(cut.through >= sequence);
    assert_eq!(
        script.asked.lock().unwrap().len(),
        2,
        "held forget runs before the yes turn's first model round"
    );
    while let Some(event) = engine.try_output() {
        assert!(!matches!(event, ProviderOutput::Audio { .. }));
        assert!(
            !matches!(event, ProviderOutput::Transcript { finished:false, text, .. } if text.trim() == PRIVATE)
        );
    }
    release.send(()).unwrap();
    assert_fresh_audio(&collect_named_forget(&mut engine, REQUEST).await);
}
