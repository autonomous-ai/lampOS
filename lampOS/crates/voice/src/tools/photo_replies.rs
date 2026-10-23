//! Pending replies must not become live enrollment or broad deletion.
use super::*;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;

const PHOTO_SPEAKER: &str = "0123456789abcdef/f7";

fn photo_owner(request: u64, speaker: Option<&str>) -> crate::memory_scope::RequestScope {
    let key = crate::memory_scope::RequestKey {
        worker: lamp_turn_owner::BootId::new([6; 16]).unwrap(),
        session: 1,
        request,
    };
    let mut owner =
        crate::memory_scope::RequestScope::new(key, crate::memory_scope::ScopeSnapshot::room());
    if let Some(speaker) = speaker {
        assert!(owner.speaker_recorder().unwrap().seal(
            key,
            &[speaker.to_owned()],
            std::time::Instant::now() + Duration::from_secs(10),
        ));
    }
    owner
}

fn call(name: &str, args: Value) -> ToolCall {
    ToolCall {
        id: "photo-reply".into(),
        name: name.into(),
        args,
    }
}

fn socket(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("photo-route-{}-{label}.sock", std::process::id()))
}

fn accept(listener: &UnixListener) -> std::os::unix::net::UnixStream {
    listener.set_nonblocking(true).unwrap();
    let until = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                // macOS may inherit the listener's nonblocking mode.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("photo accept: {error}"),
        }
        assert!(
            std::time::Instant::now() < until,
            "photo operation never arrived"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn serve(path: &Path, answers: Vec<Value>) -> std::thread::JoinHandle<Vec<Value>> {
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path).unwrap();
    std::thread::spawn(move || {
        answers
            .into_iter()
            .map(|answer| {
                let mut stream = accept(&listener);
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                writeln!(stream, "{answer}").unwrap();
                serde_json::from_str(&line).unwrap()
            })
            .collect()
    })
}

/// Keep accepting until the caller returned, so a forbidden second operation
/// is recorded and answered rather than hidden by a closed fixture socket.
fn after_lookup(
    path: &Path,
    answer: Value,
    run: impl FnOnce() -> (Value, bool),
) -> ((Value, bool), Vec<Value>) {
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path).unwrap();
    let (finished, returned) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let mut stream = accept(&listener);
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        let mut requests = vec![serde_json::from_str::<Value>(&line).unwrap()];
        writeln!(stream, "{answer}").unwrap();
        drop(stream);
        let until = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            if let Ok((mut stream, _)) = listener.accept() {
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                requests.push(serde_json::from_str::<Value>(&line).unwrap());
                writeln!(stream, "{}", json!({"remembered": "Pending"})).unwrap();
            } else if returned.try_recv().is_ok() {
                return requests;
            }
            assert!(
                std::time::Instant::now() < until,
                "photo reply never finished"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    });
    let result = run();
    finished.send(()).unwrap();
    let requests = server.join().unwrap();
    std::fs::remove_file(path).unwrap();
    (result, requests)
}

fn route(path: &Path, enabled: bool) -> Route {
    Route::Faces(
        path.into(),
        crate::known_faces::FaceSwitches {
            checked: true,
            visits: false,
            photos: enabled,
        },
    )
}

#[test]
fn photo_replies_are_not_sent_when_off_unheard_or_mixed() {
    let path = socket("refused");
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    for name in [crate::photo_faces::YES, crate::photo_faces::NO] {
        assert_eq!(
            super::route(
                &Settings {
                    who: Some(path.clone()),
                    ..Default::default()
                },
                name
            ),
            Route::Nobody
        );
    }
    for name in [crate::known_faces::REMEMBER, crate::known_faces::FORGET] {
        let exact = call(name, json!({"photo_reply": true}));
        assert_eq!(
            run_call(&route(&path, false), &exact, 1, true).1,
            (unavailable(), false)
        );
        assert_eq!(
            run_call(&route(&path, true), &exact, 1, false).1.0["error"],
            FACES_IN_PERSON
        );
        for args in [
            json!({"photo_reply": true, "name": "Other"}),
            json!({"photo_reply": "yes"}),
            json!({"photo_reply": true, "replace": true}),
            json!({"photo_reply": true, "apart": true}),
            json!({"photo_reply": true, "photo_speaker": PHOTO_SPEAKER}),
            json!({"photo_reply": true, "face": PHOTO_SPEAKER}),
        ] {
            assert!(
                run_call(&route(&path, true), &call(name, args), 1, true)
                    .1
                    .0
                    .get("error")
                    .is_some()
            );
        }
    }
    assert!(listener.accept().is_err());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn photo_replies_mutate_only_the_asked_pending_face() {
    for (label, name, legacy, op, answer) in [
        (
            "yes",
            crate::known_faces::REMEMBER,
            crate::photo_faces::YES,
            "photo_yes",
            json!({"remembered": "Pending"}),
        ),
        (
            "no",
            crate::known_faces::FORGET,
            crate::photo_faces::NO,
            "photo_no",
            json!({"let_go": true}),
        ),
    ] {
        for shared in [false, true] {
            for repetition in 0..2 {
                let path = socket(&format!("{label}-{shared}-{repetition}"));
                let server = serve(
                    &path,
                    vec![
                        json!({"photo_here": "Pending", "asked": true, "face": PHOTO_SPEAKER}),
                        answer.clone(),
                    ],
                );
                let call = if shared {
                    call(name, json!({"photo_reply": true}))
                } else {
                    call(legacy, json!({}))
                };
                let owner = photo_owner(1, Some(PHOTO_SPEAKER));
                let started = std::time::Instant::now();
                let told = run_call_scoped(&route(&path, true), &call, 1, true, &owner.lease())
                    .1
                    .0;
                eprintln!(
                    "photo_latency route={} action={label} repetition={repetition} elapsed_us={}",
                    if shared { "shared" } else { "legacy" },
                    started.elapsed().as_micros()
                );
                assert_eq!(told["ok"], true);
                assert_eq!(
                    server.join().unwrap(),
                    [
                        json!({"photo_here": true}),
                        if op == "photo_yes" {
                            json!({"photo_yes": "Pending", "face": PHOTO_SPEAKER})
                        } else {
                            json!({"photo_no": "Pending"})
                        }
                    ]
                );
                std::fs::remove_file(path).unwrap();
            }
        }
    }
}

#[test]
fn missing_or_unasked_photo_replies_never_fall_back_to_live_tools() {
    for (label, here) in [
        ("none", json!({"photo_here": null})),
        ("unasked", json!({"photo_here": "Pending", "asked": false})),
        ("old-eyes", json!({"photo_here": "Pending"})),
    ] {
        for name in [crate::known_faces::REMEMBER, crate::known_faces::FORGET] {
            let path = socket(&format!("{label}-{name}"));
            let server = serve(&path, vec![here.clone()]);
            let told = run_call(
                &route(&path, true),
                &call(name, json!({"photo_reply": true})),
                1,
                true,
            )
            .1
            .0;
            assert!(told.get("error").is_some());
            assert_eq!(server.join().unwrap(), [json!({"photo_here": true})]);
            std::fs::remove_file(path).unwrap();
        }
    }
}

#[test]
fn photo_yes_preserves_the_eyes_presence_and_asked_refusals() {
    for error in ["not_here", "not_asked"] {
        let path = socket(error);
        let server = serve(
            &path,
            vec![
                json!({"photo_here": "Pending", "asked": true, "face": PHOTO_SPEAKER}),
                json!({"error": error}),
            ],
        );
        let owner = photo_owner(1, Some(PHOTO_SPEAKER));
        let told = run_call_scoped(
            &route(&path, true),
            &call(crate::known_faces::REMEMBER, json!({"photo_reply": true})),
            1,
            true,
            &owner.lease(),
        )
        .1
        .0;
        assert!(told.get("error").is_some());
        assert_eq!(
            server.join().unwrap()[1],
            json!({"photo_yes": "Pending", "face": PHOTO_SPEAKER})
        );
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn a_photo_yes_requires_the_exact_proved_speaker_even_for_an_inherited_person() {
    for (case, face, proved) in [
        ("no-proof", Some(PHOTO_SPEAKER), false),
        ("missing-face", None, true),
        ("unknown-face", Some("unknown"), true),
        ("different-face", Some("0123456789abcdef/f8"), true),
        ("old-boot", Some("fedcba9876543210/f7"), true),
    ] {
        for shared in [false, true] {
            let path = socket(&format!("proof-{case}-{shared}"));
            let owner = if proved {
                photo_owner(1, Some(PHOTO_SPEAKER))
            } else {
                // A known/inherited memory person is deliberately not proof
                // that this person spoke the answer to the photo question.
                crate::memory_scope::RequestScope::new(
                    crate::memory_scope::RequestKey {
                        worker: lamp_turn_owner::BootId::new([6; 16]).unwrap(),
                        session: 1,
                        request: 1,
                    },
                    crate::memory_scope::ScopeSnapshot::of("Pending"),
                )
            };
            let reply = if shared {
                call(crate::known_faces::REMEMBER, json!({"photo_reply": true}))
            } else {
                // Even a model-supplied identity cannot create local proof.
                call(
                    crate::photo_faces::YES,
                    json!({"photo_speaker": PHOTO_SPEAKER}),
                )
            };
            let (result, requests) = after_lookup(
                &path,
                json!({"photo_here": "Pending", "asked": true, "face": face}),
                || run_call_scoped(&route(&path, true), &reply, 1, true, &owner.lease()).1,
            );
            assert_eq!(result, (unavailable(), false), "{case}, shared={shared}");
            assert_eq!(requests, [json!({"photo_here": true})], "{case}");
        }
    }
}

#[test]
fn typed_wrong_request_and_bare_live_photo_yes_have_no_authority() {
    let path = socket("proof-admission");
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let owner = photo_owner(1, Some(PHOTO_SPEAKER));
    for name in [crate::known_faces::REMEMBER, crate::photo_faces::YES] {
        let args = if name == crate::known_faces::REMEMBER {
            json!({"photo_reply": true})
        } else {
            json!({})
        };
        let reply = call(name, args);
        assert_eq!(
            run_call_scoped(&route(&path, true), &reply, 1, false, &owner.lease())
                .1
                .0["error"],
            FACES_IN_PERSON
        );
        assert_eq!(
            run_call_scoped(&route(&path, true), &reply, 2, true, &owner.lease()).1,
            (unavailable(), false)
        );
    }
    assert!(listener.accept().is_err());
    drop(listener);
    std::fs::remove_file(&path).unwrap();
    let (result, requests) = after_lookup(
        &path,
        json!({"photo_here": "Pending", "asked": true, "face": PHOTO_SPEAKER}),
        || {
            run_call(
                &route(&path, true),
                &call(crate::photo_faces::YES, json!({})),
                1,
                true,
            )
            .1
        },
    );
    assert_eq!(result, (unavailable(), false));
    assert_eq!(requests, [json!({"photo_here": true})]);
}

#[test]
fn photo_authority_is_rechecked_after_connect_before_mutation_bytes_are_written() {
    let path = socket("proof-final-write");
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let owner = photo_owner(1, Some(PHOTO_SPEAKER));
    let checks = std::cell::Cell::new(0);
    let answer = exchange_if(
        &path,
        &json!({"photo_yes": "Pending", "face": PHOTO_SPEAKER}),
        Duration::from_secs(1),
        || {
            checks.set(checks.get() + 1);
            if checks.get() == 2 {
                owner.revoke();
            }
            owner.lease().speaker() == Some(PHOTO_SPEAKER)
        },
    );
    assert!(answer.is_none());
    assert_eq!(checks.get(), 2);
    let (stream, _) = listener
        .accept()
        .expect("the connection precedes the final check");
    // exchange_if returned and dropped its client stream. A nonblocking
    // read checks EOF/no queued mutation without setting a timeout on the
    // closed peer, which macOS rejects with EINVAL.
    stream.set_nonblocking(true).unwrap();
    let mut line = String::new();
    assert_eq!(BufReader::new(stream).read_line(&mut line).unwrap(), 0);
    assert!(line.is_empty(), "no mutation bytes may escape");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn ordinary_forget_keeps_its_existing_name_operation_with_photos_on() {
    for enabled in [false, true] {
        let path = socket(if enabled {
            "ordinary-on"
        } else {
            "ordinary-off"
        });
        let server = serve(&path, vec![json!({"forgot": true})]);
        let owner = crate::memory_scope::RequestScope::new(
            crate::memory_scope::RequestKey {
                worker: lamp_turn_owner::BootId::new([6; 16]).unwrap(),
                session: 1,
                request: 1,
            },
            crate::memory_scope::ScopeSnapshot::of("Known"),
        );
        let result = run_call_scoped(
            &route(&path, enabled),
            &call(crate::known_faces::FORGET, json!({"name": "Known"})),
            1,
            true,
            &owner.lease(),
        );
        assert!(result.1.1);
        assert_eq!(server.join().unwrap(), [json!({"forget": "Known"})]);
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn revocation_stops_a_photo_yes_but_still_sends_its_narrow_no() {
    let mut violations = Vec::new();
    for (label, name, args) in [
        (
            "shared-yes",
            crate::known_faces::REMEMBER,
            json!({"photo_reply": true}),
        ),
        (
            "shared-no",
            crate::known_faces::FORGET,
            json!({"photo_reply": true}),
        ),
        ("legacy-yes", crate::photo_faces::YES, json!({})),
        ("legacy-no", crate::photo_faces::NO, json!({})),
    ] {
        let path = socket(&format!("revoked-{label}"));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let owner = photo_owner(1, Some(PHOTO_SPEAKER));
        let lease = owner.lease();
        let (finished, returned) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let mut stream = accept(&listener);
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let mut requests = vec![serde_json::from_str::<Value>(&line).unwrap()];
            owner.revoke();
            writeln!(
                stream,
                "{}",
                json!({"photo_here": "Pending", "asked": true, "face": PHOTO_SPEAKER})
            )
            .unwrap();
            drop(stream);
            let until = std::time::Instant::now() + Duration::from_secs(3);
            loop {
                if let Ok((mut stream, _)) = listener.accept() {
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    stream
                        .set_write_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    let mut line = String::new();
                    BufReader::new(&stream).read_line(&mut line).unwrap();
                    requests.push(serde_json::from_str::<Value>(&line).unwrap());
                    writeln!(stream, "{}", json!({"ok": true})).unwrap();
                } else if returned.try_recv().is_ok() {
                    return requests;
                }
                assert!(
                    std::time::Instant::now() < until,
                    "photo call did not finish"
                );
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        let result = run_call_scoped(&route(&path, true), &call(name, args), 1, true, &lease);
        finished.send(()).unwrap();
        let requests = server.join().unwrap();
        assert_eq!(result.1, (unavailable(), false), "{label}");
        eprintln!(
            "photo_revocation route={label} requests={} mutations={}",
            requests.len(),
            requests.len().saturating_sub(1)
        );
        let mut expected = vec![json!({"photo_here": true})];
        if label.ends_with("-no") {
            expected.push(json!({"photo_no": "Pending"}));
        }
        if requests != expected {
            violations.push((label, requests));
        }
        std::fs::remove_file(path).unwrap();
    }
    assert!(
        violations.is_empty(),
        "photo revocation did not preserve grant/refusal direction: {violations:?}"
    );
}

#[test]
fn a_slow_pending_lookup_times_out_without_a_photo_mutation() {
    let path = socket("pending-timeout");
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let stream = accept(&listener);
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        std::thread::sleep(Duration::from_millis(400));
        assert!(
            listener.accept().is_err(),
            "a timed-out lookup mutated a photo"
        );
        serde_json::from_str::<Value>(&line).unwrap()
    });
    let started = std::time::Instant::now();
    let told = run_call(
        &route(&path, true),
        &call(crate::known_faces::REMEMBER, json!({"photo_reply": true})),
        1,
        true,
    )
    .1
    .0;
    eprintln!(
        "photo_latency route=shared action=pending_timeout elapsed_us={}",
        started.elapsed().as_micros()
    );
    assert!(told.get("error").is_some());
    assert_eq!(server.join().unwrap(), json!({"photo_here": true}));
    std::fs::remove_file(path).unwrap();
}

/// Observe every real routed request through caller completion. An older
/// Eyes' not_understood is an answer, never permission for a bare retry or
/// ordinary camera enrollment. This fixture emulates the documented wire;
/// it does not execute Eyes or prove a complete microphone route.
fn photo_wire_round_trip(shared: bool, older_eyes: bool) -> ((Value, bool), Vec<Value>) {
    let path = socket(&format!("wire-contract-{shared}-{older_eyes}"));
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let (finished, returned) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let until = std::time::Instant::now() + Duration::from_secs(3);
        let mut requests = Vec::new();
        loop {
            assert!(
                std::time::Instant::now() < until,
                "bounded photo wire fixture"
            );
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    stream
                        .set_write_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    let mut bytes = Vec::new();
                    BufReader::new(&stream)
                        .take(1024)
                        .read_until(b'\n', &mut bytes)
                        .unwrap();
                    assert_eq!(bytes.last(), Some(&b'\n'), "complete bounded request");
                    let line: Value = serde_json::from_slice(&bytes).unwrap();
                    let answer = if requests.is_empty() && line == json!({"photo_here": true}) {
                        json!({"photo_here": "fixture", "asked": true, "face": PHOTO_SPEAKER})
                    } else if !older_eyes
                        && line == json!({"photo_yes": "fixture", "face": PHOTO_SPEAKER})
                    {
                        json!({"remembered": "fixture"})
                    } else {
                        json!({"error": "not_understood"})
                    };
                    requests.push(line);
                    writeln!(stream, "{answer}").unwrap();
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    // A completed caller cannot start another operation;
                    // queued connections are drained before this receipt.
                    if returned.try_recv().is_ok() {
                        return requests;
                    }
                    assert!(std::time::Instant::now() < until, "bounded photo wire call");
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(error) => panic!("photo wire accept: {error}"),
            }
        }
    });
    let owner = photo_owner(1, Some(PHOTO_SPEAKER));
    let request = if shared {
        call(crate::known_faces::REMEMBER, json!({"photo_reply": true}))
    } else {
        call(crate::photo_faces::YES, json!({}))
    };
    let result = run_call_scoped(&route(&path, true), &request, 1, true, &owner.lease()).1;
    finished.send(()).unwrap();
    let requests = server.join().unwrap();
    std::fs::remove_file(path).unwrap();
    (result, requests)
}

#[test]
fn photo_yes_scoped_calls_use_the_eyes_face_field() {
    for shared in [false, true] {
        let ((told, answered), requests) = photo_wire_round_trip(shared, false);
        assert!(answered, "the actual scoped route must receive its answer");
        assert_eq!(told["ok"], true, "{told}");
        assert_eq!(
            requests,
            [
                json!({"photo_here": true}),
                json!({"photo_yes": "fixture", "face": PHOTO_SPEAKER}),
            ]
        );
    }
}

#[test]
fn photo_yes_not_understood_never_retries_or_claims_remembered() {
    for shared in [false, true] {
        let ((told, answered), requests) = photo_wire_round_trip(shared, true);
        assert!(answered, "a semantic refusal is an answered operation");
        assert!(told.get("error").is_some(), "{told}");
        assert_ne!(told["ok"], true, "{told}");
        assert!(
            !told.to_string().to_ascii_lowercase().contains("remembered"),
            "{told}"
        );
        assert_eq!(
            requests,
            [
                json!({"photo_here": true}),
                json!({"photo_yes": "fixture", "face": PHOTO_SPEAKER}),
            ],
            "no bare yes retry, ordinary enrollment or extra lookup"
        );
    }
}
