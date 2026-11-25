use super::*;

fn origin() -> lamp_msgs::FixedLineOrigin {
    lamp_msgs::FixedLineOrigin {
        worker_boot: "worker".into(),
        provider_session: 2,
        request: 3,
        call_id: "call".into(),
        route_epoch: 4,
        microphone_generation: 5,
        camera_generation: 6,
        issued_us: 7,
    }
}

#[test]
fn fixed_line_metadata_requires_verified_skills_and_builtin_exact_words() {
    let mut wire = json!({"skill":"prepare-unplug", "text":crate::language::UNPLUG_STAY,
        "exact":true, "expires_in_ms":5000,"fixed_line_origin":origin()});
    for caller in [Caller::Other, Caller::WifiHelper] {
        let request = parse_from(&wire.to_string(), caller).unwrap();
        assert!(request.line.fixed_origin.is_none());
        assert!(
            matches!(request.words, Words::Exact(_)),
            "ordinary playback is preserved"
        );
    }
    assert!(
        parse_from(&wire.to_string(), Caller::Skills)
            .unwrap()
            .line
            .fixed_origin
            .is_some()
    );
    wire["text"] = "arbitrary computer or service text".into();
    assert!(
        parse_from(&wire.to_string(), Caller::Skills)
            .unwrap()
            .line
            .fixed_origin
            .is_none()
    );
    wire["text"] = crate::language::UNPLUG_STAY.into();
    wire["fixed_line_origin"]["route_epoch"] = 0.into();
    assert!(
        parse_from(&wire.to_string(), Caller::Skills)
            .unwrap()
            .line
            .fixed_origin
            .is_none()
    );
    for mut control in [
        json!({"skill":"control","listen":false}),
        json!({"skill":"control","turn":"start","expires_in_ms":5000}),
        json!({"skill":"control","stream":{"by":"computer"},"expires_in_ms":5000}),
    ] {
        control["fixed_line_origin"] = json!(origin());
        assert!(parse_from(&control.to_string(), Caller::Skills).is_err());
    }
    assert!(skills_cgroup("0::/system.slice/lamp-skills.service"));
    assert!(skills_cgroup(
        "1:name=systemd:/system.slice/lamp-skills.service/child"
    ));
    for other in [
        "0::/system.slice/lamp-skills.service-other",
        "0::/user.slice/lamp-skills.service",
        "0::/system.slice/lamp-voice.service",
    ] {
        assert!(!skills_cgroup(other));
    }
}

#[test]
fn fixed_line_rendering_keeps_canonical_and_actual_language_through_ticket() {
    use crate::language::Language;
    let dir = std::env::temp_dir().join(format!("lamp-fixed-origin-render-{}", std::process::id()));
    for language in [None, Some(Language::Vietnamese)] {
        let recordings = Recordings::none().with_cache(Some(Arc::new(
            tts_cache::TtsCache::new(
                dir.join(language.map_or("en", Language::code)),
                "test",
                crate::language::SPEECH_MODEL,
                language.map_or("en", Language::code),
                Box::new(|_| Ok(vec![1_i16; 2400])),
            )
            .speaking(language),
        )));
        let canonical = crate::language::UNPLUG_STAY;
        let line = json!({"skill":"prepare-unplug", "text":canonical,"exact":true,
            "expires_in_ms":5000,"fixed_line_origin":origin()})
        .to_string();
        let mut request = parse_from(&line, Caller::Skills).unwrap();
        let (samples, said) = recordings.rendered(canonical).unwrap();
        request.line.fixed_line =
            recordings.fixed_line(request.line.fixed_origin.take(), canonical, said);
        request.words = Words::ExactSound {
            samples,
            text: Some(said.to_owned()),
            pieces: None,
        };
        let (mut says, asks) = Says::by_hand();
        let (answer, _answered) = mpsc::channel();
        asks.send(Asked {
            request,
            answer,
            read_at_us: 1,
            expires_at_us: 5_000_001,
            guard: None,
            gone: None,
            cancel: Default::default(),
        })
        .unwrap();
        says.poll(2);
        let (_, ticket) = says.take_exact(3).unwrap();
        let fixed = ticket.fixed_line().unwrap();
        assert_eq!(fixed.canonical, canonical);
        assert_eq!(fixed.language, language.map_or("en", Language::code));
        assert_eq!(
            fixed.text,
            language
                .and_then(|language| language.fixed(canonical))
                .unwrap_or(canonical)
        );
        let mut tampered = fixed.clone();
        tampered.text.push('x');
        assert!(!tampered.check());
        tampered = fixed.clone();
        tampered.language = " vi ".into();
        assert!(!tampered.check());
    }
    let _ = std::fs::remove_dir_all(dir);
}
