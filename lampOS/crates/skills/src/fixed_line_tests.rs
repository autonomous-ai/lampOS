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
fn fixed_line_origin_requires_voice_and_stays_with_its_run() {
    let first = origin();
    let call = json!({"fixed_line_origin": first});
    assert_eq!(fixed_line_origin(&call, true), Some(first.clone()));
    assert_eq!(fixed_line_origin(&call, false), None);
    let mut bad = call.clone();
    bad["fixed_line_origin"]["route_epoch"] = 0.into();
    assert_eq!(fixed_line_origin(&bad, true), None);
    let mut sockets = Sockets {
        gaze: PathBuf::new(),
        mood: PathBuf::new(),
        actions: PathBuf::new(),
        say: PathBuf::new(),
        may: May::default(),
        fixed_origin: Some(first.clone()),
    };
    let old_run = sockets.clone();
    sockets.fixed_origin.as_mut().unwrap().request += 1;
    let line = exact(lamp_voice::language::UNPLUG_STAY, 8);
    let old = fixed_line_asking(line.clone(), old_run.fixed_origin.as_ref());
    let new = fixed_line_asking(line, sockets.fixed_origin.as_ref());
    assert_eq!(old["fixed_line_origin"], json!(first));
    assert_ne!(old["fixed_line_origin"], new["fixed_line_origin"]);
    for asking in [
        json!({"text":"arbitrary exact words","exact":true}),
        json!({"text":lamp_voice::language::UNPLUG_STAY}),
        json!({"note":"say something"}),
    ] {
        assert!(
            fixed_line_asking(asking, Some(&first))
                .get("fixed_line_origin")
                .is_none()
        );
    }
    assert!(
        fixed_line_asking(
            json!({"text":lamp_voice::language::UNPLUG_STAY,
        "exact":true,"fixed_line_origin":first}),
            None
        )
        .get("fixed_line_origin")
        .is_none()
    );
}

#[test]
fn fixed_line_legacy_parser_retry_is_once_and_keeps_the_original_deadline() {
    let dir = tempfile::Builder::new()
        .prefix("fixed-legacy")
        .tempdir_in("/tmp")
        .unwrap();
    let path = dir.path().join("say");
    let listener = UnixListener::bind(&path).unwrap();
    let server = std::thread::spawn(move || {
        let mut asks = Vec::new();
        for n in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            asks.push(serde_json::from_str::<serde_json::Value>(&line).unwrap());
            if n == 0 {
                writeln!(
                    stream,
                    "{}",
                    json!({"outcome":"refused","why":"not a say request"})
                )
                .unwrap();
            } else {
                writeln!(stream, "{}", json!({"event":"begun","at_us":1})).unwrap();
                writeln!(stream, "{}", json!({"outcome":"spoken"})).unwrap();
            }
        }
        listener.set_nonblocking(true).unwrap();
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        asks
    });
    let asking = fixed_line_asking(exact(lamp_voice::language::UNPLUG_STAY, 8), Some(&origin()));
    let mut begun = 0;
    assert_eq!(
        say(&path, "line", "test", asking, &[], || begun += 1),
        "line: spoken"
    );
    assert_eq!(begun, 1);
    let asks = server.join().unwrap();
    assert!(asks[0].get("fixed_line_origin").is_some());
    assert!(asks[1].get("fixed_line_origin").is_none());
    assert_eq!(asks[0]["start_before_us"], asks[1]["start_before_us"]);
    assert!(asks[0]["start_before_us"].as_u64().unwrap() > 0);
}

#[test]
fn fixed_line_does_not_retry_after_begun_or_other_refusal() {
    for (begins, why) in [(true, "not a say request"), (false, "private")] {
        let dir = tempfile::Builder::new()
            .prefix("fixed-no-retry")
            .tempdir_in("/tmp")
            .unwrap();
        let path = dir.path().join("say");
        let listener = UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            if begins {
                writeln!(stream, "{}", json!({"event":"begun","at_us":1})).unwrap();
            }
            writeln!(stream, "{}", json!({"outcome":"refused","why":why})).unwrap();
            listener
        });
        let asking =
            fixed_line_asking(exact(lamp_voice::language::UNPLUG_STAY, 8), Some(&origin()));
        let mut begun = 0;
        assert_eq!(
            say(&path, "line", "test", asking, &[], || begun += 1),
            format!("line: refused ({why})")
        );
        assert_eq!(begun, usize::from(begins));
        let listener = server.join().unwrap();
        listener.set_nonblocking(true).unwrap();
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    }
}
