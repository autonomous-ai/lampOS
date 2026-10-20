//! The page on a real bus, in a directory of the test's own, asked over
//! HTTP as a browser asks it: the test plays the eyes and the voice.
use lamp_bus::{Bus, Publisher};
use lamp_msgs::{
    BodyOffsets, Diagnostic, ExpressionBody, ExpressionGaze, ExpressionLease, Face, Faces, Health,
    HealthItem, JointState, Level, RoomPresence, Speech, VoiceState,
};
use lamp_peek::{
    page,
    serve::{self, Peek},
    units::UnitRow,
    watch::{self, Seen},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

/// How long a test waits for something that is to happen: it comes in
/// milliseconds, and the limit is for when it does not come at all.
const WAIT: Duration = Duration::from_secs(20);
const TOKEN: &str = "office-token-0123456789";
/// Every view, as a browser asks for it.
const VIEWS: [&str; 6] = [
    "/",
    "/cam",
    "/cam.svg",
    "/bus",
    "/bus?topic=voice",
    "/status",
];

struct Rig {
    root: PathBuf,
    bus: Bus,
    port: u16,
    stop: Arc<AtomicBool>,
    watching: Option<thread::JoinHandle<std::io::Result<()>>>,
}

impl Rig {
    fn new(name: &str, token: Option<&str>) -> Self {
        // A root short enough for a socket's path, which the system's own
        // temporary directory on a Mac is not.
        let root = PathBuf::from(format!("/tmp/lpk-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root)
            .unwrap()
            .rescan_every(Duration::from_millis(20));
        let seen = Arc::new(Mutex::new(Seen::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let watching = thread::spawn({
            let (bus, seen, stop) = (bus.clone(), Arc::clone(&seen), Arc::clone(&stop));
            move || watch::watch(&bus, &seen, &stop)
        });
        // Listening once its four subscribers are there.
        let started = Instant::now();
        while !watch::EVENTS.iter().all(|topic| {
            bus.subscribers(topic)
                .unwrap()
                .iter()
                .any(|listener| listener.socket.starts_with("peek.") && listener.listening)
        }) {
            assert!(started.elapsed() < WAIT, "the watcher did not subscribe");
            thread::sleep(Duration::from_millis(5));
        }
        let peek = Arc::new(Peek {
            bus: bus.clone(),
            seen,
            token: token.map(str::to_owned),
            sessions: root.join("sessions"),
            units: Arc::new(|| {
                vec![UnitRow {
                    id: "lamp-voice.service".into(),
                    active: "active".into(),
                    sub: "running".into(),
                    ..UnitRow::default()
                }]
            }),
        });
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                axum::serve(listener, serve::router(peek)).await.unwrap();
            });
        });
        Self {
            root,
            bus,
            port,
            stop,
            watching: Some(watching),
        }
    }

    /// `method path` with `headers` and `body`: the status, the head and
    /// the body.
    fn ask(&self, method: &str, path: &str, headers: &str, body: &str) -> (u16, String, String) {
        self.ask_as("localhost", method, path, headers, body)
    }

    /// The same, with `host` in `Host`.
    fn ask_as(
        &self,
        host: &str,
        method: &str,
        path: &str,
        headers: &str,
        body: &str,
    ) -> (u16, String, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\
             Content-Length: {}\r\n{headers}\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut text = String::new();
        stream.read_to_string(&mut text).unwrap();
        let (head, body) = text.split_once("\r\n\r\n").unwrap();
        let status = head.split(' ').nth(1).unwrap().parse().unwrap();
        (status, head.to_owned(), body.to_owned())
    }

    fn get(&self, path: &str) -> (u16, String) {
        let (status, _, body) = self.ask("GET", path, "", "");
        (status, body)
    }

    /// Ask for `path` until its body holds `wanted`.
    fn until(&self, path: &str, wanted: &str) -> String {
        let started = Instant::now();
        loop {
            let (status, body) = self.get(path);
            if status == 200 && body.contains(wanted) {
                return body;
            }
            assert!(
                started.elapsed() < WAIT,
                "{path} never held {wanted:?}: {body}"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn publisher(
        &self,
        topic: &str,
        declaration: &lamp_bus::Declaration,
        source: &str,
    ) -> Publisher {
        self.bus
            .publisher_declared(topic, declaration, source)
            .unwrap()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(watching) = self.watching.take() {
            let _ = watching.join();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn face(id: &str, xmin: f32, facing: bool) -> Face {
    Face {
        id: id.into(),
        xmin,
        ymin: 0.2,
        xmax: xmin + 0.2,
        ymax: 0.5,
        confidence: 0.9,
        yaw: None,
        facing: Some(facing),
        speaking: None,
    }
}

fn eyes_say(diagnostics: &mut Publisher, camera: &str) {
    diagnostics
        .publish_json(
            1,
            &Diagnostic::ok("eyes")
                .with("camera", camera)
                .with("set_aside", 0),
        )
        .unwrap();
}

/// Every path under `root`, and what each topic keeps.
fn tree(root: &Path) -> BTreeSet<(PathBuf, Vec<u8>)> {
    let mut found = BTreeSet::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(&folder).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                folders.push(path.clone());
            }
            let kept = if entry.file_name() == "latest" {
                fs::read(&path).unwrap()
            } else {
                Vec::new()
            };
            found.insert((path.strip_prefix(root).unwrap().to_path_buf(), kept));
        }
    }
    found
}

#[test]
fn every_view_reads_only_and_answers_only_get() {
    let rig = Rig::new("reads", None);
    let mut faces = rig.publisher(Faces::TOPIC, &Faces::declaration(), "eyes");
    faces
        .publish_json(
            1,
            &Faces {
                faces: vec![face("f1", 0.1, true)],
            },
        )
        .unwrap();
    let mut diagnostics = rig.publisher(Diagnostic::TOPIC, &Diagnostic::declaration(), "eyes");
    eyes_say(&mut diagnostics, "open");
    let mut state = rig.publisher(VoiceState::TOPIC, &VoiceState::declaration(), "voice");
    state
        .publish_json(
            1,
            &VoiceState {
                state: lamp_msgs::Voice::Listening,
            },
        )
        .unwrap();
    rig.until("/bus", "eyes ok, camera open");
    rig.until("/cam.svg", "class=\"face\"");
    let before = tree(&rig.root);
    for _ in 0..2 {
        for view in VIEWS {
            let (status, body) = rig.get(view);
            assert_eq!(status, 200, "{view}: {body}");
        }
    }
    assert_eq!(
        tree(&rig.root),
        before,
        "a view changed the bus's directory"
    );
    for view in VIEWS {
        for method in ["POST", "PUT", "DELETE", "PATCH"] {
            let (status, _, _) = rig.ask(method, view, "", "");
            assert_eq!(status, 405, "{method} {view}");
        }
    }
    for topic in rig.bus.topics().unwrap() {
        let publishers = rig.bus.publishers(&topic.name).unwrap();
        assert!(
            publishers
                .iter()
                .all(|source| source.name != watch::SUBSCRIBER),
            "{}: {publishers:?}",
            topic.name
        );
    }
}

#[test]
fn with_the_camera_closed_no_box_is_drawn() {
    let rig = Rig::new("closed", None);
    let mut faces = rig.publisher(Faces::TOPIC, &Faces::declaration(), "eyes");
    let mut diagnostics = rig.publisher(Diagnostic::TOPIC, &Diagnostic::declaration(), "eyes");
    let mut presence = rig.publisher(RoomPresence::TOPIC, &RoomPresence::declaration(), "eyes");
    // A part that is not the eyes saying the eyes' camera is open is not
    // taken (the reviewer on #2150, P3 5).
    let mut other = rig.publisher(Diagnostic::TOPIC, &Diagnostic::declaration(), "voice");
    eyes_say(&mut other, "open");
    rig.until("/bus", "<td>voice</td><td>eyes ok</td>");
    // Faces kept, but no word from the eyes on the camera: nothing drawn.
    faces
        .publish_json(
            1,
            &Faces {
                faces: vec![face("f1", 0.1, true), face("f2", 0.6, false)],
            },
        )
        .unwrap();
    presence
        .publish_json(
            1,
            &RoomPresence {
                someone: true,
                nobody_s: 0,
                by: None,
            },
        )
        .unwrap();
    let svg = rig.until("/cam.svg", "no word from the eyes");
    assert!(!svg.contains("class=\"face\""), "{svg}");
    // Open: both faces as boxes, with who is there.
    eyes_say(&mut diagnostics, "open");
    let svg = rig.until("/cam.svg", "class=\"face\"");
    assert_eq!(svg.matches("class=\"face\"").count(), 2, "{svg}");
    assert!(
        svg.contains("f1 facing") && svg.contains("f2 turned away"),
        "{svg}"
    );
    assert!(svg.contains("someone here"), "{svg}");
    assert!(!svg.contains("<image"), "never a picture: {svg}");
    // The privacy switch closes the camera: the faces are still kept on
    // the bus, but the page draws none of them, nor who is there.
    eyes_say(&mut diagnostics, "closed");
    let svg = rig.until("/cam.svg", "camera closed");
    for drawn in ["class=\"face\"", "class=\"gaze\"", "f1", "f2", "someone"] {
        assert!(
            !svg.contains(drawn),
            "{drawn} with the camera closed: {svg}"
        );
    }
    assert!(rig.bus.latest(Faces::TOPIC).unwrap().is_some());
}

#[test]
fn the_face_the_lamp_follows_is_marked_with_its_point() {
    let rig = Rig::new("gaze", None);
    let mut faces = rig.publisher(Faces::TOPIC, &Faces::declaration(), "eyes");
    let mut diagnostics = rig.publisher(Diagnostic::TOPIC, &Diagnostic::declaration(), "eyes");
    let mut body = rig.publisher(
        ExpressionBody::TOPIC,
        &ExpressionBody::declaration(),
        "expression",
    );
    faces
        .publish_json(
            1,
            &Faces {
                faces: vec![face("f1", 0.1, true), face("f2", 0.6, true)],
            },
        )
        .unwrap();
    eyes_say(&mut diagnostics, "open");
    let following = ExpressionBody {
        lease: ExpressionLease {
            clock_id: "c1".into(),
            session_id: "s1".into(),
            voice_session_id: "v1".into(),
            sequence: 1,
            turn: 1,
            request: 1,
            issued_us: 1,
            apply_us: 1,
            valid_until_us: 100_000,
        },
        offsets: BodyOffsets::ZERO,
        hold: false,
        gaze: Some(ExpressionGaze {
            face_id: "f2".into(),
            x: 0.4,
            y: -0.3,
            observed_us: 1,
            valid_until_us: 100_000,
        }),
    };
    // It lasts 200 ms on the bus and the expression sends it every 20 ms:
    // sent again before each look.
    let started = Instant::now();
    let svg = loop {
        body.publish_json(1, &following).unwrap();
        let (_, svg) = rig.get("/cam.svg");
        if svg.contains("(followed)") {
            break svg;
        }
        assert!(started.elapsed() < WAIT, "never followed: {svg}");
        thread::sleep(Duration::from_millis(20));
    };
    assert!(svg.contains("f2 facing (followed)"), "{svg}");
    assert!(!svg.contains("f1 facing (followed)"), "{svg}");
    // The point: x 0.4 is 70% across 320, y -0.3 is 35% down 256.
    assert!(svg.contains("cx=\"224.0\" cy=\"89.6\""), "{svg}");
}

#[test]
fn a_word_heard_never_reaches_any_view() {
    let rig = Rig::new("words", None);
    let mut heard = rig.publisher("voice/heard", &Speech::declaration(), "voice");
    let mut said = rig.publisher("voice/said", &Speech::declaration(), "voice");
    let mut diagnostics = rig.publisher(Diagnostic::TOPIC, &Diagnostic::declaration(), "voice");
    for finished in [false, true] {
        heard
            .publish_json(
                1,
                &Speech {
                    text: "PLANTED zebrafish words".into(),
                    finished,
                    locale: Some("en_GB".into()),
                    request: Some(7),
                },
            )
            .unwrap();
    }
    said.publish_json(
        2,
        &Speech {
            text: "PLANTED zebrafish answer".into(),
            finished: true,
            locale: None,
            request: Some(7),
        },
    )
    .unwrap();
    diagnostics
        .publish_json(
            3,
            &Diagnostic::new(Level::Warn, "voice", "heard PLANTED zebrafish"),
        )
        .unwrap();
    let page = rig.until("/bus", "voice warn");
    assert!(page.contains("a piece, request 7, en_GB"), "{page}");
    assert!(page.contains("finished, request 7"), "{page}");
    for view in VIEWS {
        let (status, body) = rig.get(view);
        assert_eq!(status, 200, "{view}");
        for planted in ["PLANTED", "zebrafish"] {
            assert!(!body.contains(planted), "{view} shows {planted}: {body}");
        }
    }
}

/// Without a token, it answers only to this machine's own names, so no
/// other page in the browser reaches it under a name of its own (DNS
/// rebinding; the reviewer on #2150, P3 6). With one, the token is the
/// gate wherever it is reached.
#[test]
fn without_a_token_it_answers_only_as_this_machine() {
    let rig = Rig::new("host", None);
    for host in ["localhost", "127.0.0.1", "LOCALHOST", "[::1]"] {
        for host in [host.to_owned(), format!("{host}:{}", rig.port)] {
            let (status, _, body) = rig.ask_as(&host, "GET", "/bus", "", "");
            assert_eq!(status, 200, "{host}: {body}");
        }
    }
    for host in [
        "rebound.example",
        "rebound.example:8077",
        "127.0.0.1.example",
        "[::2]:8077",
    ] {
        let (status, _, body) = rig.ask_as(host, "GET", "/bus", "", "");
        assert_eq!(status, 403, "{host}: {body}");
        assert!(!body.contains("voice/heard"), "{host}: {body}");
    }
    drop(rig);
    let rig = Rig::new("host-token", Some(TOKEN));
    let (status, _, _) = rig.ask_as(
        "lamp.office.example:8077",
        "GET",
        "/bus",
        &format!("Authorization: Bearer {TOKEN}\r\n"),
        "",
    );
    assert_eq!(status, 200);
}

#[test]
fn with_a_token_only_the_form_or_the_header_opens_it() {
    let rig = Rig::new("token", Some(TOKEN));
    let mut diagnostics = rig.publisher(Diagnostic::TOPIC, &Diagnostic::declaration(), "eyes");
    eyes_say(&mut diagnostics, "open");
    for view in VIEWS {
        let (status, body) = rig.get(view);
        assert_eq!(status, 401, "{view}");
        assert!(
            body.contains("name=\"token\"") && !body.contains("camera"),
            "{view}: {body}"
        );
    }
    // The right token in the address is refused: it would be kept in the
    // browser's history and a proxy's log (the first builder on #2141).
    let (status, body) = rig.get(&format!("/bus?token={TOKEN}"));
    assert_eq!(status, 400, "{body}");
    let (status, body) = rig.get(&format!("/bus?topic=voice&token={TOKEN}"));
    assert_eq!(status, 400, "{body}");
    let form = "Content-Type: application/x-www-form-urlencoded\r\n";
    let (status, head, _) = rig.ask("POST", "/token", form, "token=not-the-token-at-all");
    assert_eq!(status, 401, "{head}");
    assert!(!head.to_lowercase().contains("set-cookie"), "{head}");
    let (status, head, _) = rig.ask("POST", "/token", form, &format!("token={TOKEN}"));
    assert_eq!(status, 303, "{head}");
    let cookie = head
        .lines()
        .find_map(|line| line.strip_prefix("set-cookie: "))
        .unwrap_or_else(|| panic!("no cookie: {head}"));
    assert_eq!(
        cookie,
        format!("peek={TOKEN}; HttpOnly; SameSite=Strict; Path=/")
    );
    let (status, _, body) = rig.ask("GET", "/bus", &format!("Cookie: peek={TOKEN}\r\n"), "");
    assert_eq!(status, 200, "{body}");
    let (status, _, _) = rig.ask(
        "GET",
        "/cam.svg",
        &format!("Authorization: Bearer {TOKEN}\r\n"),
        "",
    );
    assert_eq!(status, 200);
    let (status, _, _) = rig.ask(
        "GET",
        "/bus",
        "Cookie: peek=office-token-0123456780\r\n",
        "",
    );
    assert_eq!(status, 401);
}

#[test]
fn beyond_this_machine_without_a_token_it_does_not_start() {
    let root = PathBuf::from(format!("/tmp/lpk-start-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("credentials")).unwrap();
    let run = |envs: &[(&str, &Path)], listen: &str| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_lamp-peek"));
        command
            .env_clear()
            .env("LAMP_BUS_ROOT", &root)
            .env("LAMP_PEEK", "on")
            .env("LAMP_PEEK_LISTEN", listen);
        for (key, value) in envs {
            command.env(key, value);
        }
        ended(command)
    };
    let (code, said) = run(&[], "0.0.0.0:0");
    assert_eq!(code, Some(2), "{said}");
    assert!(
        said.contains("no token was given") && said.contains("refused"),
        "{said}"
    );
    // A token too short to be one is no token.
    fs::write(root.join("credentials/peek-token"), "short\n").unwrap();
    let (code, said) = run(
        &[("CREDENTIALS_DIRECTORY", &root.join("credentials"))],
        "0.0.0.0:0",
    );
    assert_eq!(code, Some(2), "{said}");
    assert!(said.contains("under 16 characters"), "{said}");
    // Off unless switched on: nothing listens, and it says so.
    let mut off = Command::new(env!("CARGO_BIN_EXE_lamp-peek"));
    off.env_clear().env("LAMP_BUS_ROOT", &root);
    let (code, said) = ended(off);
    assert_eq!(code, Some(0), "{said}");
    assert!(said.contains("off (LAMP_PEEK is not on)"), "{said}");
    fs::remove_dir_all(root).unwrap();
}

/// The program's exit code and what it said, once it ended by itself: one
/// that started serving instead is stopped, and the test fails rather than
/// waiting on it for ever.
fn ended(mut command: Command) -> (Option<i32>, String) {
    let mut child = command
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > WAIT {
            let _ = child.kill();
            let _ = child.wait();
            panic!("lamp-peek started serving instead of ending");
        }
        thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// The one page's stream, read as a browser's `EventSource` reads it,
/// until every section has come at least once since `wanted` was first in
/// one: each section's newest HTML, by its name.
fn stream_until(rig: &Rig, headers: &str, wanted: &str) -> BTreeMap<String, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", rig.port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    write!(
        stream,
        "GET /events HTTP/1.1\r\nHost: localhost\r\n{headers}\r\n"
    )
    .unwrap();
    let started = Instant::now();
    let mut text = String::new();
    let mut buffer = [0; 8192];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => panic!("the stream ended: {text}"),
            Ok(read) => text.push_str(&String::from_utf8_lossy(&buffer[..read])),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(error) => panic!("{error}"),
        }
        let sections = sections_in(&text);
        let done = sections.values().any(|html| html.contains(wanted))
            && page::SECTIONS
                .iter()
                .all(|(id, _)| sections.contains_key(*id));
        if done {
            return sections;
        }
        assert!(
            started.elapsed() < WAIT,
            "the stream never held {wanted:?}: {text}"
        );
    }
}

/// Each event's newest data in a stream's text, by the event's name.
fn sections_in(text: &str) -> BTreeMap<String, String> {
    let mut sections = BTreeMap::new();
    let body = text.split_once("\r\n\r\n").map_or("", |(_, body)| body);
    for event in body.split("\n\n") {
        let mut name = None;
        let mut data = Vec::new();
        for line in event.lines() {
            // A chunk's size line, between the stream's chunks.
            if let Some(value) = line.strip_prefix("event: ") {
                name = Some(value.to_owned());
            } else if let Some(value) = line.strip_prefix("data: ") {
                data.push(value);
            }
        }
        if let Some(name) = name {
            sections.insert(name, data.join("\n"));
        }
    }
    sections
}

fn record(file: &Path, line: &str) {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .unwrap();
    writeln!(file, "{line}").unwrap();
}

/// #2141's second step (the lead on #1711, 20 Oct): the one page's stream
/// carries every section, each turn's steps from the voice's own records
/// with the slowest marked and why a turn held, the joints against where
/// they were asked to be, the body's own word, and each part's health as
/// it says it, escaped. A word heard, said, or in a record never reaches
/// it; another part speaking as the body is not taken.
#[test]
fn the_one_page_streams_each_section_and_never_a_word() {
    const WORD: &str = "Ossifrage";
    let rig = Rig::new("one", None);
    let session = rig.root.join("sessions/s1/metadata");
    fs::create_dir_all(&session).unwrap();
    let events = session.join("events-1.jsonl");
    for line in [
        r#"{"kind":"input_admitted","turn":4,"admission_basis":"local_speech","at_us":1}"#,
        r#"{"kind":"transcript","turn":4,"text":"Ossifrage","at_us":2}"#,
        r#"{"kind":"answer_stages","turn":4,"words_ms":300,"cue_ms":null,"chat_first_ms":900,"first_answer_audio_ms":1400}"#,
        r#"{"kind":"turn_finished","turn":4,"outcome":"audio_written_unscored"}"#,
        r#"{"kind":"input_admitted","turn":5,"admission_basis":"lamp_speaks_first","at_us":1}"#,
        r#"{"kind":"floor_held","turn":5,"at_us":3}"#,
    ] {
        record(&events, line);
    }
    let mut heard = rig.publisher(Speech::TOPICS[0], &Speech::declaration(), "voice");
    heard
        .publish_json(
            1,
            &Speech {
                text: WORD.into(),
                finished: true,
                request: Some(4),
                locale: Some("en-US".into()),
            },
        )
        .unwrap();
    let mut diagnostics = rig.publisher(Diagnostic::TOPIC, &Diagnostic::declaration(), "eyes");
    eyes_say(&mut diagnostics, "open");
    let mut joints = rig.publisher(JointState::TOPIC, &JointState::declaration(), "body");
    joints
        .publish_json(
            1,
            &JointState {
                name: vec!["base_yaw".into(), "elbow".into()],
                position: vec![0.10, 1.00],
                velocity: Vec::new(),
                effort: vec![0.05, 0.40],
                commanded: vec![0.10, 0.80],
            },
        )
        .unwrap();
    let mut body = rig.publisher(Diagnostic::TOPIC, &Diagnostic::declaration(), "body");
    body.publish_json(
        1,
        &Diagnostic::new(
            Level::Warn,
            "body",
            "a joint met resistance and stopped pushing, 1 time in the last minute",
        )
        .with("state", "ready")
        .with("standing", "0.95")
        .with("resisted_last_minute", 1)
        .with("note", WORD),
    )
    .unwrap();
    // Another part saying it is the body is not taken.
    let mut other = rig.publisher(Diagnostic::TOPIC, &Diagnostic::declaration(), "voice");
    other
        .publish_json(1, &Diagnostic::ok("body").with("standing", "0.11"))
        .unwrap();
    let mut health = rig.publisher(Health::TOPIC, &Health::declaration(), "health");
    health
        .publish_json(
            1,
            &Health {
                level: Level::Warn,
                message: "controls: head pads 97 of 98 wired".into(),
                items: vec![
                    HealthItem {
                        name: "controls".into(),
                        level: Level::Warn,
                        message: "head pads 97 of 98 wired <script>alert(1)</script>".into(),
                        age_ms: 1_200,
                    },
                    HealthItem {
                        name: "voice".into(),
                        level: Level::Ok,
                        message: String::new(),
                        age_ms: 300,
                    },
                ],
                missing: vec!["eyes2".into()],
                quiet_ms: 0,
            },
        )
        .unwrap();
    let sections = stream_until(&rig, "", "ready");
    let all = format!("{sections:?}");
    assert!(!all.contains(WORD), "{all}");
    assert!(!all.contains("<script>"), "{all}");
    let turns = &sections["turns"];
    // Turn 4: 300, then 600 to the first token, the slowest, then 500.
    assert!(
        turns.contains("300 ms") && turns.contains("900 ms (600)"),
        "{turns}"
    );
    assert!(
        turns.contains("<td class=\"slow\" title=\"first token\">"),
        "{turns}"
    );
    assert!(turns.contains("played"), "{turns}");
    assert!(
        turns.contains("lamp_speaks_first") && turns.contains("the busy room held the floor"),
        "{turns}"
    );
    let body = &sections["body"];
    assert!(body.contains("<td>elbow</td>"), "{body}");
    assert!(body.contains("0.200"), "the elbow is off by 0.2: {body}");
    assert!(body.contains("0.95") && !body.contains("0.11"), "{body}");
    assert!(body.contains("met resistance"), "{body}");
    let health = &sections["health"];
    assert!(
        health.contains("head pads 97 of 98 wired &lt;script&gt;"),
        "{health}"
    );
    assert!(health.contains("Never heard from: eyes2"), "{health}");
    let overview = &sections["overview"];
    assert!(overview.contains("camera open"), "{overview}");
    assert!(overview.contains("newest 1400 ms"), "{overview}");
    assert!(
        sections["camera"].contains("<svg"),
        "{:?}",
        sections["camera"]
    );
    // The one page, asked for as a browser does, is only the frame the
    // stream fills.
    let (status, page) = rig.get("/");
    assert_eq!(status, 200);
    assert!(page.contains("new EventSource('/events')"), "{page}");
    assert!(!page.contains(WORD), "{page}");
}

/// The stream answers only GET, and with a token only with it.
#[test]
fn the_stream_reads_only_and_asks_for_the_token() {
    let rig = Rig::new("stream", Some(TOKEN));
    for method in ["POST", "PUT", "DELETE", "PATCH"] {
        let (status, _, _) = rig.ask(method, "/events", &format!("Cookie: peek={TOKEN}\r\n"), "");
        assert_eq!(status, 405, "{method}");
    }
    let (status, _, body) = rig.ask("GET", "/events", "", "");
    assert_eq!(status, 401, "{body}");
    assert!(!body.contains("event:"), "{body}");
    let sections = stream_until(&rig, &format!("Cookie: peek={TOKEN}\r\n"), "<th>voice</th>");
    assert!(sections.contains_key("overview"));
}
