//! Prepared Music keeps only the context of the face that spoke the line.
use super::*;
use crate::memory_scope::{FaceCache, ScopeLease};
use crate::speaking_faces::{SpeakingFaces, known_face_of};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

const WORDS: &str = "Please play a quiet recording.";
const PERSONAL_CALL: &str = "prepared-personal-music";
const ROOM_CALL: &str = "fresh-room-music";
const WITHIN: Duration = Duration::from_secs(3);

fn context_for(person: Option<&str>) -> Told {
    match person {
        Some(person) => Told {
            requires_speaker: false,
            pending_private_context: false,
            text: format!("personal context for {person}"),
            after: Some("Private preference: the quiet recording.".into()),
            history: vec![
                Message::user("Remember my private listening preference."),
                Message::assistant("The preference is kept for you."),
            ],
        },
        None => Told::from("room context with no personal history".to_owned()),
    }
}

/// These frames are placed after the prepared chat's barrier and before
/// End, inside the real turn's speaking interval. Distinct frames satisfy
/// the evidence threshold without a feeder thread or a scheduling delay.
fn show_speaker(eyes: &SpeakingFaces, speaker: &str) {
    let faces = lamp_msgs::Faces {
        faces: ["f1", "f2"]
            .map(|id| lamp_msgs::Face {
                id: id.to_owned(),
                xmin: 0.1,
                ymin: 0.1,
                xmax: 0.3,
                ymax: 0.4,
                confidence: 1.0,
                yaw: None,
                facing: None,
                speaking: Some(lamp_msgs::Speaking {
                    state: if id == speaker {
                        lamp_msgs::SpeakingState::Yes
                    } else {
                        lamp_msgs::SpeakingState::No
                    },
                    score: 0.9,
                    from: None,
                }),
            })
            .into(),
    };
    for frame in 1..=crate::speaking_faces::FEWEST_LOOKS {
        eyes.look(
            Instant::now(),
            Some((("music-scope", frame as u64), &faces)),
        );
    }
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
        )),
        "{outputs:?}"
    );
}

/// The dispatcher and request scope are real. The local Skills peer builds
/// a valid reply, then waits for a one-shot release before writing it.
async fn prepared_music_for(speaker: &str, revoke: bool) {
    let narrowed = speaker != "f1";
    let expected_call = if narrowed { ROOM_CALL } else { PERSONAL_CALL };
    let directory = crate::process::SessionDirectory::create().unwrap();
    let known = directory.path.join("known");
    std::fs::write(&known, "music-known-face").unwrap();
    std::fs::write(known_face_of(&known), "f1\n").unwrap();
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
    let listener = tokio::net::UnixListener::bind(directory.path.join("call.sock")).unwrap();
    let (arrived, pending) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let mut peer = AbortOnDrop(Some(tokio::spawn(async move {
        tokio::time::timeout(WITHIN, async move {
            let (socket, _) = listener.accept().await.unwrap();
            let (read, mut write) = socket.into_split();
            let mut reader = tokio::io::BufReader::new(read.take(64 * 1024));
            let mut line = String::new();
            assert!(reader.read_line(&mut line).await.unwrap() > 0);
            assert!(line.ends_with('\n'), "one complete bounded request");
            let asked: Value = serde_json::from_str(&line).unwrap();
            let context = serde_json::from_value(asked["requested_audio_context"].clone()).unwrap();
            let offer = crate::second_provider::requested_audio_tests::offered(context);
            offer.check().unwrap();
            arrived.send((asked, offer.clone())).unwrap();
            released.await.expect("the test releases the valid reply");
            let response = json!({"ok":true,"result":{
                "next":"Confirm.",
                "requested_audio":offer,
            }});
            write
                .write_all(format!("{response}\n").as_bytes())
                .await
                .unwrap();
        })
        .await
        .expect("the local Skills peer finishes within its bound");
    })));
    let [prepared, confirmation] = music_rounds(PERSONAL_CALL);
    let mut chats = VecDeque::from([prepared]);
    if narrowed {
        chats.push_back(music_rounds(ROOM_CALL)[0].clone());
    }
    chats.push_back(confirmation);
    let began = Arc::new(tokio::sync::Notify::new());
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(WORDS.to_owned())].into()),
        early: Mutex::new([vec![net::Early::Words(WORDS.into())]].into()),
        chats: Mutex::new(chats),
        chat_began: Some(began.clone()),
        ..Default::default()
    });
    let (engine, ordinary, _) = scripted(script.clone());
    let boot = lamp_turn_owner::BootId::new([75; 16]).unwrap();
    let person = crate::remember::person_id("music-known-face");
    let expected_person = (!narrowed).then(|| person.clone());
    let calls = Arc::new(AtomicUsize::new(0));
    let dispatched = calls.clone();
    let installed = worker::requested_audio_runner(settings, None, None, false);
    let runner: RequestedAudioRunner = Arc::new(move |context, call, scope| {
        assert!(audio_scope_current(&scope, &context));
        assert_eq!(scope.scope().unwrap().person(), expected_person.as_deref());
        assert_eq!(context.call_id, expected_call);
        assert_eq!(call.id, expected_call);
        assert_eq!(call.name, "play_relaxing_music");
        dispatched.fetch_add(1, Ordering::Relaxed);
        installed(context, call, scope)
    });
    let cache = Arc::new(FaceCache::default());
    let eyes = Arc::new(SpeakingFaces::default());
    let mut engine = engine
        .with_memory_scope(
            boot,
            cache.clone(),
            Arc::new(|_, _, _, _, _| panic!("Music uses its admitted audio capability")),
            Some(Arc::new(|lease: &ScopeLease| {
                context_for(lease.scope().unwrap().person())
            })),
        )
        .requesting_audio(boot, runner)
        .seeing_who_speaks(eyes.clone(), known.clone())
        .announcing(false)
        .preparing(true);
    engine.memory_camera(true, 1);
    assert!(cache.replace(
        cache.begin_refresh(),
        crate::known_faces::FaceObservation::read(&known),
    ));
    engine.submit(start(67));
    let lease = engine
        .turn
        .as_ref()
        .unwrap()
        .memory_scope
        .as_ref()
        .unwrap()
        .lease();
    assert_eq!(lease.scope().unwrap().person(), Some(person.as_str()));
    engine.submit(audio(67, 1));
    tokio::time::timeout(WITHIN, began.notified())
        .await
        .expect("the personal chat begins before final input");
    let mut personal = context_for(Some(&person)).messages();
    personal.push(Message::user(WORDS));
    assert_eq!(*script.asked.lock().unwrap(), [personal.clone()]);
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    no_answer(&std::iter::from_fn(|| engine.try_output()).collect::<Vec<_>>());
    show_speaker(&eyes, speaker);
    engine.submit(end(67));

    let (asked, offered) = tokio::time::timeout(WITHIN, pending)
        .await
        .expect("the admitted call reaches the local Skills peer")
        .expect("the peer prepares a valid reply");
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        lease.scope().unwrap().person(),
        (!narrowed).then_some(person.as_str()),
        "the admitted personal scope is narrowed only when another face speaks"
    );
    assert_eq!(
        lease.key(),
        crate::memory_scope::RequestKey {
            worker: boot,
            session: 1,
            request: 67,
        },
        "narrowing keeps the original request owner"
    );
    assert_eq!(offered.context.worker_boot, crate::process::boot_text(boot));
    assert_eq!(
        (
            offered.context.request,
            offered.context.provider_session,
            offered.context.privacy_generation,
        ),
        (67, 1, 1)
    );
    assert_eq!(offered.context.call_id, expected_call);
    assert_eq!(
        asked,
        json!({"op":"call","name":"play_relaxing_music","args":{},
            "requested_audio_context":offered.context}),
    );
    let before_reply = 1 + usize::from(narrowed);
    {
        let chats = script.asked.lock().unwrap();
        assert_eq!(chats.len(), before_reply, "no follow-up before the reply");
        if narrowed {
            let mut room = context_for(None).messages();
            room.push(Message::user(WORDS));
            assert_eq!(
                chats[1], room,
                "fresh room context drops the prepared personal history and preference"
            );
        }
    }
    no_answer(&std::iter::from_fn(|| engine.try_output()).collect::<Vec<_>>());
    if revoke {
        assert!(narrowed, "this case revokes the narrowed room owner");
        engine
            .turn
            .as_ref()
            .unwrap()
            .memory_scope
            .as_ref()
            .unwrap()
            .revoke();
        assert!(!lease.is_current());
    }
    release.send(()).unwrap();
    tokio::time::timeout(WITHIN, peer.0.as_mut().unwrap())
        .await
        .expect("the gated peer finishes")
        .unwrap();
    peer.0.take();
    if revoke {
        tokio::time::timeout(WITHIN, &mut engine.turn.as_mut().unwrap().task)
            .await
            .expect("the revoked turn finishes after the reply")
            .unwrap();
        // Inspect the raw queue too: the final output filter must not hide
        // an offer incorrectly enqueued by a revoked answer.
        let mut outputs: Vec<_> = engine.urgent.drain(..).collect();
        outputs.extend(std::iter::from_fn(|| {
            engine.output.try_recv().ok().map(|queued| queued.event)
        }));
        no_answer(&outputs);
        assert_eq!(script.asked.lock().unwrap().len(), before_reply);
        assert_eq!(script.chats.lock().unwrap().len(), 1);
    } else {
        let outputs = music_outputs(&mut engine).await;
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
        assert_eq!(offers, [(67, 1, &offered)]);
        assert!(outputs.iter().any(|event| matches!(
            event,
            ProviderOutput::Audio { request: 67, samples, .. } if !samples.is_empty()
        )));
        let chats = script.asked.lock().unwrap();
        assert_eq!(chats.len(), before_reply + 1);
        let follow_up = chats.last().unwrap();
        let base = if narrowed {
            context_for(None).messages()
        } else {
            context_for(Some(&person)).messages()
        };
        assert_eq!(&follow_up[..base.len()], &base);
        assert_eq!(
            follow_up.last(),
            Some(&Message::tool(
                expected_call,
                json!({"next":"Confirm. The recording has only been offered for preparation; playback has not started and may still be refused. Confirm only that you will try to play it. Do not say it is playing, has started, or is ready."}).to_string()
            ))
        );
        if narrowed {
            assert!(
                !serde_json::to_string(&chats[1..])
                    .unwrap()
                    .contains("private")
            );
        } else {
            assert_eq!(chats[0], personal, "the prepared personal round is reused");
        }
    }
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(ordinary.lock().unwrap().is_empty());
    let model = serde_json::to_string(&*script.asked.lock().unwrap()).unwrap();
    assert!(!model.contains("requested_audio") && !model.contains("relaxing.wav"));
    drop(engine);
    for path in [
        known_face_of(&known),
        known,
        directory.path.join("tools.json"),
        directory.path.join("call.sock"),
    ] {
        std::fs::remove_file(path).unwrap();
    }
}

#[tokio::test]
async fn another_speaking_face_discards_prepared_personal_music_for_fresh_room_context() {
    prepared_music_for("f2", false).await;
}

#[tokio::test]
async fn the_matching_speaking_face_keeps_prepared_personal_music() {
    prepared_music_for("f1", false).await;
}

#[tokio::test]
async fn revoking_the_narrowed_room_during_a_valid_music_reply_stops_all_follow_up() {
    prepared_music_for("f2", true).await;
}
