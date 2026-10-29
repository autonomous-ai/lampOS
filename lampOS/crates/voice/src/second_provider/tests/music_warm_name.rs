use super::*;

/// A warm listener may carry the preceding name's words and time, but the
/// recording capability still requires this request's own captured words.
#[tokio::test]
async fn a_warm_name_prefix_carries_words_and_time_without_reusing_music_authority_or_pcm() {
    for (first, second, authorized) in [
        ("Lamp.", "play a quiet recording.", true),
        ("Okay.", "play a quiet recording.", false),
        ("Lamp.", "", false),
    ] {
        let script = Arc::new(WarmScript {
            words: Arc::new(vec![(11, first, None), (22, second, None)]),
            script: Arc::new(Script {
                chats: Mutex::new(music_rounds("after-name").into()),
                ..Default::default()
            }),
            ..Default::default()
        });
        let (mut engine, contexts, ordinary) = warm_music_engine(script.clone());
        let now = Arc::new(Mutex::new(Instant::now()));
        let clock = now.clone();
        Arc::get_mut(&mut engine.shared).unwrap().clock = Arc::new(move || *clock.lock().unwrap());
        let judged: Arc<Mutex<Vec<(u64, u64, String)>>> = Arc::default();
        let seen = judged.clone();
        let mut engine = engine.heeding(Arc::new(move |request, window, words, _, _| {
            seen.lock()
                .unwrap()
                .push((request, window.0, words.to_owned()));
            if request == 7 {
                Some("a short sound")
            } else {
                (!crate::attention::called_by_name(words)).then_some("no name")
            }
        }));
        let packet = |request, marker, at| {
            let mut input = warm_audio(request, 1, marker);
            if let ProviderInput::Audio {
                read_completed_at_us,
                ..
            } = &mut input
            {
                *read_completed_at_us = at;
            }
            input
        };
        engine.submit(warm_start(7, 41, 1));
        engine.submit(packet(7, 11, 1_000));
        engine.submit(warm_end(7, 1));
        let first_outputs = music_outputs(&mut engine).await;
        assert_eq!(heard_words(&first_outputs), [first]);
        assert!(!first_outputs.iter().any(|output| matches!(
            output,
            ProviderOutput::RequestedAudio { .. } | ProviderOutput::Audio { .. }
        )));
        assert!(contexts.lock().unwrap().is_empty());
        script.listeners_opened(2).await;

        *now.lock().unwrap() += Duration::from_millis(600);
        engine.submit(warm_start(8, 41, 1));
        engine.submit(packet(8, 22, 9_000));
        engine.submit(warm_end(8, 1));
        let outputs = music_outputs(&mut engine).await;
        assert_eq!(
            heard_words(&outputs),
            [second],
            "raw transcript stays current"
        );
        script.listeners_opened(3).await;
        for (index, request, marker, words) in [(0, 7, 11, first), (1, 8, 22, second)] {
            let listener = script.listener(index);
            assert_eq!(
                *listener.packets.lock().unwrap(),
                [vec![marker; lamp_audio::CAPTURE_SAMPLES]]
            );
            assert_eq!(
                *listener.finished.lock().unwrap(),
                Some((request, words.to_owned()))
            );
        }
        assert!(script.listener(2).packets.lock().unwrap().is_empty());
        let carried = first == "Lamp.";
        let whole = if carried {
            format!("{first} {second}")
        } else {
            second.to_owned()
        };
        assert_eq!(
            judged.lock().unwrap()[1],
            (8, if carried { 1_000 } else { 9_000 }, whole.clone())
        );
        let offers: Vec<_> = outputs
            .iter()
            .filter_map(|output| match output {
                ProviderOutput::RequestedAudio {
                    request,
                    session,
                    offer,
                } => {
                    assert_eq!((*request, *session), (8, 41));
                    Some(offer.context.clone())
                }
                _ => None,
            })
            .collect();
        assert_eq!(offers.len(), usize::from(authorized));
        assert_eq!(*contexts.lock().unwrap(), offers);
        if authorized {
            assert_eq!(offers[0].request, 8);
            assert_eq!(offers[0].provider_session, 41);
            assert_eq!(offers[0].privacy_generation, 1);
            assert_eq!(offers[0].call_id, "after-name");
        }
        let asked = script.script.asked.lock().unwrap();
        if carried {
            assert_eq!(asked[0].last(), Some(&Message::user(whole)));
            assert_eq!(
                *ordinary.lock().unwrap(),
                if authorized { vec![] } else { vec![(8, true)] }
            );
        } else {
            assert!(asked.is_empty());
            assert!(ordinary.lock().unwrap().is_empty());
        }
        assert!(
            !serde_json::to_string(&*asked)
                .unwrap()
                .contains("requested_audio")
        );
    }
}
