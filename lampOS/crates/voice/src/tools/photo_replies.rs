//! Pending replies must not become live enrollment or broad deletion.
use super::*;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;

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
                        json!({"photo_here": "Pending", "asked": true}),
                        answer.clone(),
                    ],
                );
                let call = if shared {
                    call(name, json!({"photo_reply": true}))
                } else {
                    call(legacy, json!({}))
                };
                let started = std::time::Instant::now();
                let told = run_call(&route(&path, true), &call, 1, true).1.0;
                eprintln!(
                    "photo_latency route={} action={label} repetition={repetition} elapsed_us={}",
                    if shared { "shared" } else { "legacy" },
                    started.elapsed().as_micros()
                );
                assert_eq!(told["ok"], true);
                assert_eq!(
                    server.join().unwrap(),
                    [json!({"photo_here": true}), json!({op: "Pending"})]
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
                json!({"photo_here": "Pending", "asked": true}),
                json!({"error": error}),
            ],
        );
        let told = run_call(
            &route(&path, true),
            &call(crate::known_faces::REMEMBER, json!({"photo_reply": true})),
            1,
            true,
        )
        .1
        .0;
        assert!(told.get("error").is_some());
        assert_eq!(server.join().unwrap()[1], json!({"photo_yes": "Pending"}));
        std::fs::remove_file(path).unwrap();
    }
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
        let result = run_call(
            &route(&path, enabled),
            &call(crate::known_faces::FORGET, json!({"name": "Known"})),
            1,
            true,
        );
        assert!(result.1.1);
        assert_eq!(server.join().unwrap(), [json!({"forget": "Known"})]);
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn a_revoked_photo_reply_stops_after_the_pending_lookup() {
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
        let owner = crate::memory_scope::RequestScope::new(
            crate::memory_scope::RequestKey {
                worker: lamp_turn_owner::BootId::new([6; 16]).unwrap(),
                session: 1,
                request: 1,
            },
            crate::memory_scope::ScopeSnapshot::room(),
        );
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
                json!({"photo_here": "Pending", "asked": true})
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
        if requests != [json!({"photo_here": true})] {
            violations.push((label, requests));
        }
        std::fs::remove_file(path).unwrap();
    }
    assert!(
        violations.is_empty(),
        "revoked photo mutations: {violations:?}"
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
