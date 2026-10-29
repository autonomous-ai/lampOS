use lamp_eyes::who::{Ask, MOST_PRINTS, Taking, ask_of, live_ask_of};

#[test]
fn a_speaker_bound_enrollment_request_is_understood() {
    let ask = br#"{"remember_speaker_consented":{"name":"Fixture Speaker","face":"boot/f3","began_us":1000000,"ended_us":3000000,"replace":false,"apart":false}}"#;
    assert!(
        ask_of(ask).is_some(),
        "the guarded enrollment protocol is absent"
    );
}

const CONSENTED: &[u8] = br#"{"remember_speaker_consented":{"name":"Fixture Speaker","face":"boot/f3","began_us":1000000,"ended_us":3000000,"replace":false,"apart":false}}"#;
const BEFORE_CONSENT: &[u8] = br#"{"remember_speaker":{"name":"Fixture Speaker","face":"boot/f3","began_us":1000000,"ended_us":3000000,"replace":false,"apart":false}}"#;

#[test]
fn the_live_dispatcher_requires_the_consented_operation_only_when_guarded() {
    assert!(matches!(
        live_ask_of(CONSENTED, true),
        Ok(Ask::RememberSpeaker(_))
    ));
    for guarded in [false, true] {
        assert_eq!(live_ask_of(BEFORE_CONSENT, guarded), Err("not_understood"));
    }
    assert_eq!(live_ask_of(CONSENTED, false), Err("not_understood"));
    for plain in [
        br#"{"remember":"Fixture Speaker"}"#.as_slice(),
        br#"{"remember":"Fixture Speaker","one_face":true}"#.as_slice(),
        br#"{"remember":"Fixture Speaker","one_face":true,"replace":true,"apart":true}"#.as_slice(),
    ] {
        assert_eq!(live_ask_of(plain, true), Err("speaker_required"));
        assert!(matches!(
            live_ask_of(plain, false),
            Ok(Ask::Remember(_) | Ask::RememberOne { .. })
        ));
    }
    // Photo keeps its own path and does not become a consented live request.
    assert!(matches!(
        live_ask_of(
            br#"{"remember":"Fixture Speaker","photo":"fixture.jpg"}"#,
            true
        ),
        Ok(Ask::RememberPhoto { .. })
    ));
}

#[test]
fn the_new_operation_is_unknown_to_the_frozen_pre_consent_parser_schema() {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct BeforeConsent {
        remember_speaker: lamp_eyes::remember_speaker::Request,
    }
    // The exact six-field pre-consent Request and its old top-level key.
    // This is a schema control, not execution of an older Eyes process.
    let before = serde_json::from_slice::<BeforeConsent>(BEFORE_CONSENT).unwrap();
    assert_eq!(before.remember_speaker.name, "Fixture Speaker");
    assert!(serde_json::from_slice::<BeforeConsent>(CONSENTED).is_err());
    assert!(ask_of(BEFORE_CONSENT).is_none());
}

fn socket_capture(line: &[u8], guarded: bool) -> (serde_json::Value, usize, bool) {
    use lamp_eyes::{remember_speaker::Anchors, who::People};
    use std::{
        io::{BufRead, BufReader, Read, Write},
        os::unix::{fs::DirBuilderExt, net::UnixStream},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    struct Folder(std::path::PathBuf);
    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    // One folder per call: two tests in this binary can take the same
    // microsecond (macOS's clock), and then the second `create` failed.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let folder = std::env::temp_dir().join(format!(
        "lamp-eyes-consented-socket-{}-{at}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&folder)
        .unwrap();
    let folder = Folder(folder.canonicalize().unwrap());
    let file = folder.0.join("people.json");
    let observed_file = file.clone();
    let (mut client, mut server) = UnixStream::pair().unwrap();
    for stream in [&client, &server] {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
    }
    let worker = std::thread::spawn(move || {
        let mut line = Vec::new();
        BufReader::new((&mut server).take(1024))
            .read_until(b'\n', &mut line)
            .unwrap();
        let mut captures = 0;
        let answer = match live_ask_of(line.trim_ascii(), guarded) {
            Err(error) => serde_json::json!({"error": error}),
            Ok(Ask::RememberSpeaker(request)) => {
                let mut anchors = Anchors::default();
                anchors.refresh(1, Some(7));
                anchors.observe("boot/f3", &print(0), 2_000_000, 2_010_000, Some(7), Some(7));
                let mut taking = anchors.capture(&request, 3_100_000, Some(7)).unwrap();
                captures += 1;
                for n in 0..MOST_PRINTS {
                    let frame = 3_200_000 + n as u64 * 100_000;
                    taking.look_seen(
                        &[0.2],
                        Some(print(0)),
                        Some("boot/f3"),
                        frame,
                        frame + 10_000,
                        Some(7),
                    );
                }
                let prints = taking.taken().unwrap();
                assert_eq!(prints.len(), MOST_PRINTS);
                // The same Guard that production checks immediately before
                // People writes; a caller still reading must remain connected.
                taking
                    .speaker
                    .as_ref()
                    .unwrap()
                    .checked(
                        4_000_000,
                        Some(7),
                        lamp_eyes::photo::Caller::closed(&server),
                    )
                    .unwrap();
                let mut people = People::open(file).unwrap();
                let name = people
                    .remember_one(&taking.name, prints, 1, request.replace, request.apart)
                    .unwrap();
                assert_eq!(people.names(), ["Fixture Speaker"]);
                serde_json::json!({"remembered": name})
            }
            Ok(_) => panic!("this socket fixture expects only a guarded capture or refusal"),
        };
        writeln!(server, "{answer}").unwrap();
        captures
    });
    client.write_all(line).unwrap();
    client.write_all(b"\n").unwrap();
    // Keep both halves open until the response: write EOF means caller loss
    // to the live collector and must not be used as an implicit positive.
    let mut answer = String::new();
    BufReader::new(&mut client).read_line(&mut answer).unwrap();
    let captures = worker.join().unwrap();
    (
        serde_json::from_str(&answer).unwrap(),
        captures,
        observed_file.is_file(),
    )
}

#[test]
fn the_consented_socket_route_reaches_the_real_collector_and_people_store() {
    let (answer, captures, stored) = socket_capture(CONSENTED, true);
    assert_eq!(answer, serde_json::json!({"remembered": "Fixture Speaker"}));
    assert_eq!(captures, 1);
    assert!(stored);
}

#[test]
fn obsolete_or_unconsented_live_socket_requests_never_start_a_capture() {
    for (request, guarded, expected) in [
        (BEFORE_CONSENT, true, "not_understood"),
        (BEFORE_CONSENT, false, "not_understood"),
        (CONSENTED, false, "not_understood"),
        (
            br#"{"remember":"Fixture Speaker"}"#.as_slice(),
            true,
            "speaker_required",
        ),
        (
            br#"{"remember":"Fixture Speaker","one_face":true,"replace":true,"apart":true}"#
                .as_slice(),
            true,
            "speaker_required",
        ),
    ] {
        let (answer, captures, stored) = socket_capture(request, guarded);
        assert_eq!(answer, serde_json::json!({"error": expected}));
        assert_eq!(captures, 0);
        assert!(!stored);
    }
}

#[test]
fn the_legacy_capture_contract_stays_available_when_not_guarded() {
    let mut taking = Taking::new("Fixture Speaker".into(), 1);
    for _ in 0..MOST_PRINTS {
        taking.look(&[0.2], Some(vec![1.0; lamp_eyes::who::PRINT]));
    }
    assert_eq!(taking.taken().unwrap().len(), MOST_PRINTS);
}

fn print(index: usize) -> Vec<f32> {
    let mut p = vec![0.0; lamp_eyes::who::PRINT];
    p[index] = 1.0;
    p
}
fn guarded() -> Taking {
    use lamp_eyes::remember_speaker::{Anchors, Request};
    let mut anchors = Anchors::default();
    anchors.refresh(1, Some(7));
    anchors.observe("boot/f3", &print(0), 2_000_000, 2_010_000, Some(7), Some(7));
    let request = Request {
        name: "Fixture Speaker".into(),
        face: "boot/f3".into(),
        began_us: 1_000_000,
        ended_us: 3_000_000,
        replace: false,
        apart: false,
    };
    anchors.capture(&request, 3_100_000, Some(7)).unwrap()
}

#[test]
fn the_real_capture_collector_never_mixes_in_a_replacement_under_the_same_id() {
    let mut taking = guarded();
    for n in 0..MOST_PRINTS {
        let at = 3_200_000 + n as u64 * 100_000;
        taking.look_seen(
            &[0.2],
            Some(print(usize::from(n > 0))),
            Some("boot/f3"),
            at,
            at + 10_000,
            Some(7),
        );
    }
    assert!(taking.taken().is_none());
    assert_eq!(
        taking
            .speaker
            .as_ref()
            .unwrap()
            .checked(4_000_000, Some(7), false),
        Err("not_that_face")
    );
}

#[test]
fn five_fresh_prints_of_the_requesting_speaker_reach_the_final_fence() {
    let mut taking = guarded();
    for n in 0..MOST_PRINTS {
        let at = 3_200_000 + n as u64 * 100_000;
        taking.look_seen(
            &[0.2],
            Some(print(0)),
            Some("boot/f3"),
            at,
            at + 10_000,
            Some(7),
        );
    }
    assert_eq!(taking.taken().unwrap().len(), MOST_PRINTS);
    let guard = taking.speaker.as_ref().unwrap();
    assert!(guard.checked(4_000_000, Some(7), false).is_ok());
    assert_eq!(guard.checked(4_000_000, Some(7), true), Err("peer_closed"));
    assert_eq!(guard.checked(4_000_000, Some(8), false), Err("eyes_shut"));
}

#[test]
fn missing_or_ambiguous_frames_end_a_capture_and_a_return_cannot_rescue_it() {
    for heights in [vec![], vec![0.2, 0.2]] {
        let mut taking = guarded();
        taking.look_seen(&heights, None, None, 3_200_000, 3_210_000, Some(7));
        for n in 0..MOST_PRINTS {
            let at = 3_300_000 + n as u64 * 100_000;
            taking.look_seen(
                &[0.2],
                Some(print(0)),
                Some("boot/f3"),
                at,
                at + 10_000,
                Some(7),
            );
        }
        assert!(taking.taken().is_none());
    }
}

#[test]
fn guarded_proof_cannot_be_mixed_with_a_legacy_operation_or_unknown_fields() {
    for line in [
        r#"{"remember":"Fixture Speaker","speaker":{"face":"boot/f3"}}"#,
        r#"{"remember_speaker_consented":{"name":"Fixture Speaker","face":"boot/f3","began_us":1,"ended_us":2,"replace":false,"apart":false},"remember":"Other"}"#,
        r#"{"remember_speaker_consented":{"name":"Fixture Speaker","face":"boot/f3","began_us":1,"ended_us":2,"replace":false,"apart":false,"fallback":true}}"#,
        r#"{"remember_speaker_consented":{"name":"Fixture Speaker","face":"boot/f3","began_us":1,"ended_us":2}}"#,
        r#"{"remember_speaker_consented":{"name":"Fixture Speaker","face":"boot/f3","began_us":1,"ended_us":2,"replace":false,"apart":false,"consent":true}}"#,
        r#"{"remember_speaker_consented":{"name":"Fixture Speaker","face":"boot/f3","began_us":1,"ended_us":2,"replace":false,"apart":false},"photo_yes":"Fixture Speaker"}"#,
        r#"{"remember_speaker_consented":{"name":"Fixture Speaker","face":"boot/f3","began_us":1,"ended_us":2,"replace":false,"apart":false},"remember_speaker":{"name":"Fixture Speaker","face":"boot/f3","began_us":1,"ended_us":2,"replace":false,"apart":false}}"#,
    ] {
        assert!(ask_of(line.as_bytes()).is_none(), "{line}");
    }
}

#[test]
fn a_short_line_between_recognition_ticks_has_no_anchor_and_never_falls_back() {
    use lamp_eyes::{
        remember_speaker::{Anchors, Request},
        who::recognise_due,
    };
    use std::time::{Duration, Instant};

    // Both 1.8 s lines have a visible speaking face at every look. Only
    // their phase against the real recognition policy differs: the first
    // lies between ticks, while the second includes the tick at 3 s.
    for (began_us, ended_us, accepted) in
        [(1_100_000, 2_900_000, false), (2_100_000, 3_900_000, true)]
    {
        let start = Instant::now();
        let mut looked_for = None;
        let mut anchors = Anchors::default();
        anchors.refresh(1, Some(7));
        let mut prints = 0;
        for step in 0..=30 {
            let at = start + Duration::from_millis(step * 100);
            if recognise_due(looked_for, false, at) {
                looked_for = Some(at);
                prints += 1;
                let frame_us = 1_000_000 + step * 100_000;
                anchors.observe("boot/f3", &print(0), frame_us, frame_us, Some(7), Some(7));
            }
        }
        assert_eq!(
            prints, 2,
            "enrollment evidence adds no background inference"
        );
        let request = Request {
            name: "Fixture Speaker".into(),
            face: "boot/f3".into(),
            began_us,
            ended_us,
            replace: false,
            apart: false,
        };
        let result = anchors.begin(&request, 4_100_000, Some(7));
        if accepted {
            assert!(result.is_ok());
        } else {
            assert_eq!(result.unwrap_err(), "speaker_not_seen");
        }
    }
}
