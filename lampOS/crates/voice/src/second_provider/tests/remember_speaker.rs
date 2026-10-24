//! Requested enrollment gets the microphone interval and the original
//! line's words even when speech to text finishes much later.
use super::*;

async fn run(
    count_only: bool,
    scoped: bool,
    privacy_change: bool,
    room_publish: bool,
) -> (
    Engine,
    u64,
    u64,
    u64,
    Option<crate::speaking_faces::LineSeen>,
) {
    let request = 3_298_701
        + u64::from(count_only)
        + 2 * u64::from(!scoped)
        + 4 * u64::from(privacy_change)
        + 8 * u64::from(room_publish);
    let words = "My name is Kesh, remember my face";
    let gate = privacy_change.then(|| Arc::new(FinishGate::default()));
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(words.to_owned())].into()),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c1",
                        crate::known_faces::REMEMBER,
                        json!({"name":"Kesh"}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("Done."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        finish_after: Some(Duration::from_millis(30)),
        finish_gate: gate.clone(),
        ..Default::default()
    });
    let seen = Arc::new(Mutex::new(None));
    let found = seen.clone();
    let runner: ScopedToolRunner = Arc::new(move |of, heard, _, _, _| {
        assert!(heard);
        *found.lock().unwrap() = crate::speaking_faces::line_seen(of);
        Box::pin(async { json!({"ok":true}) })
    });
    let eyes = Arc::new(crate::speaking_faces::SpeakingFaces::default());
    let (engine, _, _) = scripted(script);
    let engine = if scoped {
        engine.with_memory_scope(
            lamp_turn_owner::BootId::new([79; 16]).unwrap(),
            Arc::new(crate::memory_scope::FaceCache::default()),
            runner,
            Some(Arc::new(|_| Told::from("SYSTEM".to_owned()))),
        )
    } else {
        engine
    };
    let mut engine = engine
        .remembering_speaker(true, Some(eyes.clone()))
        .announcing(false);
    engine.memory_camera(true, 1);
    engine.privacy((true, 1, true, 1));
    let at_us = lamp_ipc::monotonic_us();
    let at = Instant::now();
    // All observations and packets were captured before the delayed task
    // begins: using task start/finish would miss this speaker entirely.
    let began_us = at_us - 1_000_000;
    let ended_us = at_us - 500_000;
    let mut observed = Vec::new();
    for (seq, age) in [900_000, 800_000, 700_000, 600_000].into_iter().enumerate() {
        let faces = lamp_msgs::Faces {
            faces: vec![lamp_msgs::Face {
                id: "f1".into(),
                xmin: 0.2,
                ymin: 0.2,
                xmax: 0.5,
                ymax: 0.5,
                confidence: 1.0,
                yaw: None,
                facing: Some(true),
                speaking: Some(lamp_msgs::Speaking {
                    state: lamp_msgs::SpeakingState::Yes,
                    score: 0.9,
                    from: None,
                }),
            }],
        };
        eyes.look(
            at - Duration::from_micros(age),
            Some((("eyes", seq as u64), &faces)),
        );
        observed.push((at - Duration::from_micros(age), seq as u64, faces));
    }
    engine.submit(start(request));
    if count_only {
        engine.submit(ProviderInput::CountOnly {
            request,
            privacy_generation: 1,
        });
    }
    engine.submit(ProviderInput::Audio {
        request,
        privacy_generation: 1,
        sequence: 1,
        read_completed_at_us: ended_us,
        samples: vec![0; 8_000],
    });
    engine.submit(end(request));
    if let Some(gate) = gate {
        tokio::time::timeout(Duration::from_secs(1), gate.entered.notified())
            .await
            .unwrap();
        assert!(crate::speaking_faces::line_seen(request).is_none());
        engine.privacy((false, 2, true, 1));
        engine.privacy((true, 3, true, 1));
        // Even old face evidence arriving again after reopening cannot
        // revive the line begun before the privacy close.
        for (at, seq, faces) in &observed {
            eyes.look(*at, Some((("eyes", *seq), faces)));
        }
        gate.resume.notify_one();
    }
    turn(&mut engine).await;
    let line = seen.lock().unwrap().clone();
    (engine, request, began_us, ended_us, line)
}

#[tokio::test]
async fn remember_speaker_records_an_ordinary_request_from_its_microphone_clock() {
    let (mut engine, request, began_us, ended_us, line) = run(false, true, false, false).await;
    let line = line.expect("the enrollment call read its line");
    let proof = line
        .enrollment
        .expect("ordinary remember requests are observed, not only forgets");
    assert_eq!((proof.began_us, proof.ended_us), (began_us, ended_us));
    assert_eq!(proof.face, "eyes/f1");
    assert_eq!(proof.heard, "My name is Kesh, remember my face");
    assert!(proof.is_current(lamp_ipc::monotonic_us()));
    engine.stop_through(request);
    assert!(!proof.is_current(lamp_ipc::monotonic_us()));
    assert!(
        crate::speaking_faces::line_seen(request)
            .unwrap()
            .enrollment
            .is_none()
    );
}

#[tokio::test]
async fn remember_speaker_count_only_never_records_enrollment_words_or_identity() {
    let (_, request, _, _, line) = run(true, true, false, false).await;
    assert!(line.is_none(), "onboarding must not run an enrollment tool");
    assert!(crate::speaking_faces::line_seen(request).is_none());
}

#[tokio::test]
async fn remember_speaker_without_a_live_request_scope_never_records_evidence() {
    let (_, request, _, _, _) = run(false, false, false, false).await;
    assert!(crate::speaking_faces::line_seen(request).is_none());
}

#[tokio::test]
async fn remember_speaker_privacy_close_and_reopen_during_stt_cannot_revive_enrollment() {
    let (engine, request, _, _, line) = run(false, true, true, false).await;
    let line = line.expect("the tool looked for its own line");
    assert!(line.enrollment.is_none());
    let lease = engine
        .turn
        .as_ref()
        .unwrap()
        .memory_scope
        .as_ref()
        .unwrap()
        .lease();
    assert!(
        lease.is_current(),
        "the epoch, rather than a revoked room lease, fences this line"
    );
    let (answer, _) = crate::known_faces::run_scoped(
        std::path::Path::new("/missing-enrollment.sock"),
        &lamp_gemini::ToolCall {
            id: "privacy".into(),
            name: crate::known_faces::REMEMBER.into(),
            args: json!({"name":"Kesh"}),
        },
        crate::known_faces::FaceSwitches {
            checked: true,
            remember_speaker: true,
            ..Default::default()
        },
        None,
        Some(&line),
        request,
        Some(&lease),
    );
    assert_eq!(answer, json!({"error":crate::remember_speaker::NOT_SEEN}));
}

#[tokio::test]
async fn remember_speaker_room_publish_keeps_only_its_live_request_proof() {
    let (mut engine, request, _, _, line) = run(false, true, false, true).await;
    let original = line.unwrap();
    let proof = original.enrollment.as_ref().unwrap();
    let lease = engine
        .turn
        .as_ref()
        .unwrap()
        .memory_scope
        .as_ref()
        .unwrap()
        .lease();
    let mut room = original.clone();
    room.speakers.clear();
    room.known = None;
    publish_room_line(room.clone(), true, Some(&lease));
    let after = crate::speaking_faces::line_seen(request).unwrap();
    assert_eq!(
        after
            .enrollment
            .as_ref()
            .map(|p| (&p.face, &p.heard, p.began_us, p.ended_us)),
        Some((&proof.face, &proof.heard, proof.began_us, proof.ended_us))
    );
    assert!(after.speakers.is_empty() && after.known.is_none());
    let mut other = room.clone();
    other.request += 100;
    publish_room_line(other.clone(), true, Some(&lease));
    assert!(
        crate::speaking_faces::line_seen(other.request).is_none(),
        "another request's lease cannot publish this proof"
    );
    publish_room_line(other.clone(), true, None);
    assert!(
        crate::speaking_faces::line_seen(other.request).is_none(),
        "missing scope cannot publish enrollment"
    );
    publish_room_line(other.clone(), false, None);
    let off = crate::speaking_faces::line_seen(other.request).unwrap();
    assert!(
        off.enrollment.is_none(),
        "off publishes only room attribution"
    );
    engine.stop_through(request);
    publish_room_line(room, true, Some(&lease));
    assert!(
        crate::speaking_faces::line_seen(request)
            .unwrap()
            .enrollment
            .is_none(),
        "late room publication cannot revive a cancelled enrollment"
    );
}
