//! The built program, started as a unit would start it: the test plays
//! the voice on a bus of its own and the decision model's service on
//! 127.0.0.1, and reads what the program publishes, writes down and
//! prints. Nothing leaves the machine.
use lamp_bus::Bus;
use lamp_msgs::{Decision, Face, Faces, Speech, VoiceState};
use lamp_policy::attention;
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const PROGRAM: &str = env!("CARGO_BIN_EXE_lamp-policy");
const WAIT: Duration = Duration::from_secs(5);
/// A key no service has issued.
const KEY: &str = "sk-or-test-7d1e0c9a4b3f2685";

/// The program, killed when the test ends however it ends.
struct Running(Option<Child>);

impl Running {
    /// Kill it once it has printed a line starting with each of `last`,
    /// or after `WAIT`, and return everything it printed. Its lines go
    /// through `lamp_ipc::log_relay`'s thread, so a line written just
    /// before a kill may not have reached the pipe yet: killed at once, a
    /// slow ARM64 runner lost the last of them.
    fn end(mut self, last: &[&str]) -> Output {
        let mut child = self.0.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let (sent, printed) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            while let Ok(read @ 1..) = stderr.read(&mut chunk) {
                if sent.send(chunk[..read].to_vec()).is_err() {
                    break;
                }
            }
        });
        let mut all = Vec::new();
        let since = Instant::now();
        // Only whole lines count: a line still being written can already
        // start with what is wanted while the rest of it has not arrived.
        while !last.iter().all(|wanted| {
            let text = String::from_utf8_lossy(&all);
            let whole = text.rsplit_once('\n').map_or("", |(whole, _)| whole);
            whole.lines().any(|line| line.starts_with(wanted))
        }) {
            let left = WAIT.saturating_sub(since.elapsed());
            match printed.recv_timeout(left) {
                Ok(bytes) => all.extend(bytes),
                Err(_) => break,
            }
        }
        child.kill().unwrap();
        let mut output = child.wait_with_output().unwrap();
        reader.join().unwrap();
        all.extend(printed.try_iter().flatten());
        output.stderr = all;
        output
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Directories of the test's own, removed when it ends.
struct Places {
    /// The bus: short, because a socket's whole path has to fit in 103
    /// bytes and the system's temporary directory on a Mac is too long.
    root: PathBuf,
    /// The key file and the log's directory.
    kept: PathBuf,
}

impl Places {
    fn new(name: &str) -> Self {
        let root = PathBuf::from(format!("/tmp/lpp-{name}-{}", std::process::id()));
        let kept = std::env::temp_dir().join(format!("lamp-policy-{name}-{}", std::process::id()));
        for place in [&root, &kept] {
            let _ = fs::remove_dir_all(place);
            fs::create_dir_all(place).unwrap();
        }
        fs::write(kept.join("key"), format!("{KEY}\n")).unwrap();
        Self { root, kept }
    }
}

impl Drop for Places {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_dir_all(&self.kept);
    }
}

/// One request read whole from `stream`: its head and its body.
fn request(stream: &mut TcpStream) -> Option<(String, String)> {
    let mut held = Vec::new();
    let mut piece = [0u8; 4096];
    let body = loop {
        if let Some(at) = held.windows(4).position(|four| four == b"\r\n\r\n") {
            break at + 4;
        }
        match stream.read(&mut piece) {
            Ok(read) if read > 0 => held.extend_from_slice(&piece[..read]),
            _ => return None,
        }
    };
    let head = String::from_utf8(held[..body].to_vec()).ok()?;
    let length: usize = head
        .to_lowercase()
        .lines()
        .find_map(|line| line.strip_prefix("content-length:")?.trim().parse().ok())?;
    while held.len() < body + length {
        match stream.read(&mut piece) {
            Ok(read) if read > 0 => held.extend_from_slice(&piece[..read]),
            _ => return None,
        }
    }
    Some((head, String::from_utf8(held[body..].to_vec()).ok()?))
}

/// The decision model's service, played: every question it is asked gets
/// the probability 0.9, and each request is told to the test.
fn service() -> (u16, mpsc::Receiver<(String, Value)>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (told, requests) = mpsc::channel();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            stream.set_read_timeout(Some(WAIT)).unwrap();
            let Some((head, body)) = request(&mut stream) else {
                continue;
            };
            let asked: Value = serde_json::from_str(&body).unwrap();
            let answers: serde_json::Map<String, Value> = asked["questions"]
                .as_object()
                .unwrap()
                .keys()
                .map(|name| (name.clone(), json!({"type": "noul", "noul": 0.9})))
                .collect();
            let answer = json!({
                "model": "typesafe/jev-1.13-20260917",
                "answers": answers,
                "usage": {"input_tokens": 1066, "output_tokens": 134, "cost": 4.4772e-05},
            })
            .to_string();
            let _ = stream.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                    answer.len()
                )
                .as_bytes(),
            );
            // The test may have ended.
            if told.send((head, asked)).is_err() {
                break;
            }
        }
    });
    (port, requests)
}

fn start(places: &Places, endpoint: &str) -> Running {
    start_with(places, endpoint, &[])
}

/// As [`start`], with `more` options after the rest.
fn start_with(places: &Places, endpoint: &str, more: &[&str]) -> Running {
    let child = Command::new(PROGRAM)
        .arg("--key-file")
        .arg(places.kept.join("key"))
        .arg("--log-dir")
        .arg(places.kept.join("decisions"))
        .args(["--endpoint", endpoint, "--deadline-ms", "2000"])
        .arg("--bus-root")
        .arg(&places.root)
        .args(more)
        // Whatever the test was started with, the option decides.
        .env_remove("LAMP_BUS_ROOT")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    Running(Some(child))
}

/// Wait until the program listens on `topic`.
fn listening(bus: &Bus, topic: &str) {
    let started = Instant::now();
    while !bus
        .subscribers(topic)
        .unwrap()
        .iter()
        .any(|listener| listener.listening && listener.socket.starts_with("policy."))
    {
        assert!(started.elapsed() < WAIT, "nobody listens on {topic}");
        thread::sleep(Duration::from_millis(10));
    }
}

fn log(dir: &Path, lines: usize) -> Vec<Value> {
    let started = Instant::now();
    loop {
        let mut read = Vec::new();
        for file in fs::read_dir(dir).into_iter().flatten() {
            let text = fs::read_to_string(file.unwrap().path()).unwrap();
            read.extend(
                text.lines()
                    .map(|line| serde_json::from_str::<Value>(line).unwrap()),
            );
        }
        if read.len() >= lines {
            return read;
        }
        assert!(started.elapsed() < WAIT, "{} of {lines} lines", read.len());
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_program_judges_what_the_voice_hears_and_prints_nothing_of_it() {
    let places = Places::new("run");
    let (port, requests) = service();
    let endpoint = format!("http://127.0.0.1:{port}/api/alpha/decisions");
    let bus = Bus::new(&places.root).unwrap();
    // The voice, as the lamp's publishes: each topic declared.
    let [heard_topic, said_topic] = Speech::TOPICS;
    let mut heard = bus
        .publisher_declared(heard_topic, &Speech::declaration(), "voice")
        .unwrap();
    let mut said = bus
        .publisher_declared(said_topic, &Speech::declaration(), "voice")
        .unwrap();
    let mut decisions = bus
        .subscribe_typed(Decision::TOPIC, "test", Decision::TYPE)
        .unwrap();

    let running = start(&places, &endpoint);
    for topic in [heard_topic, said_topic, VoiceState::TOPIC] {
        listening(&bus, topic);
    }
    // It declares its own topic and is listed as its publisher, a moment
    // after it has subscribed.
    let started = Instant::now();
    while bus.publishers(Decision::TOPIC).unwrap().is_empty() {
        assert!(started.elapsed() < WAIT, "nobody publishes decisions");
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(bus.publishers(Decision::TOPIC).unwrap()[0].name, "policy");
    assert_eq!(
        bus.declaration(Decision::TOPIC).unwrap(),
        Some(Decision::declaration())
    );

    let piece = |text: &str, request| Speech {
        text: text.into(),
        finished: false,
        locale: None,
        request,
    };
    said.publish_json(0, &piece("Xin chào, tôi là cái đèn.", Some(4)))
        .unwrap();
    thread::sleep(Duration::from_millis(100));
    for text in [" Lamp ơi,", " mấy giờ", " rồi?"] {
        assert_eq!(
            heard.publish_json(0, &piece(text, None)).unwrap().delivered,
            1
        );
        thread::sleep(Duration::from_millis(50));
    }
    let message = decisions.recv(WAIT).unwrap().expect("a decision");
    let decision: Decision = message.parse().unwrap();
    assert_eq!(decision.check(), Ok(()));
    assert_eq!(
        (
            decision.about.as_str(),
            decision.by.as_str(),
            decision.shadow
        ),
        ("attention", "typesafe/jev-1.13-20260917", true)
    );
    assert_eq!(decision.answers.len(), 7);
    assert!(decision.answers.values().all(|yes| *yes == 0.9));
    // Every answer 0.9, the room unknown, in conversation: the sum is
    // 0.263 - 1.0683 - 0.206 = -1.0113, which is 0.2667.
    assert!(
        (decision.probability - 0.2667).abs() < 0.0005,
        "{}",
        decision.probability
    );
    assert_eq!(decision.choice, "stay_quiet");

    // What the service was sent: the key in its header, the model, the
    // state in the measured order and the seven questions in theirs.
    let (head, asked) = requests.recv_timeout(WAIT).unwrap();
    assert!(
        head.starts_with("POST /api/alpha/decisions HTTP/1.1\r\n"),
        "{head}"
    );
    assert!(head.contains(&format!("\r\nAuthorization: Bearer {KEY}\r\n")));
    assert_eq!(asked["model"], "typesafe/jev-1.13");
    assert_eq!(asked["state"]["heard"], "Lamp ơi, mấy giờ rồi?");
    assert_eq!(
        asked["state"]["lamp_last_said"],
        "Xin chào, tôi là cái đèn."
    );
    assert_eq!(asked["state"]["room"], "unknown");
    let questions = asked["questions"].as_object().unwrap();
    for (name, question) in attention::questions() {
        let lamp_policy::decide::Question::Noul { instructions } = question else {
            panic!("{name} is not a yes-or-no question");
        };
        assert_eq!(
            questions[&name],
            json!({"type": "noul", "instructions": instructions})
        );
    }
    assert_eq!(questions.len(), 7);

    // The log is the owner's alone and holds what was heard.
    let log_dir = places.kept.join("decisions");
    let lines = log(&log_dir, 1);
    assert_eq!(lines[0]["context"]["heard"], "Lamp ơi, mấy giờ rồi?");
    assert_eq!(lines[0]["state"], asked["state"]);
    assert_eq!(lines[0]["by"], "typesafe/jev-1.13-20260917");
    assert_eq!(lines[0]["asked"], "typesafe/jev-1.13");
    assert_eq!(lines[0]["usage"]["output_tokens"], 134);
    use std::os::unix::fs::PermissionsExt;
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o7777;
    assert_eq!(mode(&log_dir), 0o700);
    assert_eq!(
        mode(
            &fs::read_dir(&log_dir)
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path()
        ),
        0o600
    );

    // Something on the topic that is not what the topic carries, with
    // words in it. It is passed over, and its words are not repeated.
    let wrong = json!({"text": 7, "finished": "zanzibar marmalade"});
    assert_eq!(heard.publish_json(0, &wrong).unwrap().delivered, 1);

    // The service goes away: it answers once more, finds nobody to tell,
    // and closes. What is heard after that is decided by the rule.
    drop(requests);
    let mut gone = false;
    for _ in 0..4 {
        let whole = Speech {
            finished: true,
            ..piece("Bạn có khỏe không?", None)
        };
        assert_eq!(heard.publish_json(0, &whole).unwrap().delivered, 1);
        let message = decisions.recv(WAIT).unwrap().expect("a decision");
        if message.parse::<Decision>().unwrap().by == "rule" {
            gone = true;
            break;
        }
    }
    assert!(gone, "the service was still answering");

    let output = running.end(&[
        "lamp-policy: the decision model is not answering (",
        "lamp-policy: a message on voice/heard is not of its type",
    ]);
    assert!(output.stdout.is_empty());
    let printed = String::from_utf8(output.stderr).unwrap();
    let lines: Vec<&str> = printed.lines().collect();
    assert!(
        lines.iter().all(|line| line.starts_with("lamp-policy: ")),
        "{printed}"
    );
    assert!(
        lines[0].starts_with(&format!(
            "lamp-policy: started endpoint={endpoint} model=typesafe/jev-1.13 deadline_ms=2000 pause_ms=300 log_dir="
        )),
        "{printed}"
    );
    assert!(lines[0].ends_with(&format!("bus_root={} shadow=true", places.root.display())));
    assert!(
        lines.iter().any(|line| line.starts_with(
            "lamp-policy: decision 1 about=attention choice=stay_quiet probability=0.267 by=model took_ms="
        )),
        "{printed}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("lamp-policy: the decision model is not answering (")),
        "{printed}"
    );
    assert!(
        lines.contains(
            &"lamp-policy: a message on voice/heard is not of its type: such messages are passed over"
        ),
        "{printed}"
    );
    // Not the key, and not a word of what was heard or said.
    for secret in [
        KEY,
        "sk-or",
        "Lamp ơi",
        "mấy giờ",
        "rồi",
        "Xin chào",
        "cái đèn",
        "khỏe",
        "zanzibar",
        "marmalade",
    ] {
        assert!(!printed.contains(secret), "{secret:?} in {printed}");
    }
}

#[test]
fn bad_arguments_print_the_usage_and_end_with_status_2() {
    let run = |arguments: &[&str]| Command::new(PROGRAM).args(arguments).output().unwrap();
    for arguments in [
        &[][..],
        &["--key-file", "/k"],
        &["--key-file", "relative", "--log-dir", "/d"],
        &["--key-file", "/k", "--log-dir", "/d", "--deadline-ms", "0"],
        &[
            "--key-file",
            "/k",
            "--log-dir",
            "/d",
            "--endpoint",
            "ftp://x/",
        ],
        &["--key-file", "/k", "--log-dir", "/d", "--loud", "yes"],
    ] {
        let output = run(arguments);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        let printed = String::from_utf8(output.stderr).unwrap();
        assert!(printed.starts_with("lamp-policy: "), "{printed}");
        assert!(
            printed
                .contains("\nusage: lamp-policy --key-file ABSOLUTE_PATH --log-dir ABSOLUTE_DIR")
        );
        assert!(output.stdout.is_empty());
    }
    let help = run(&["--help"]);
    assert_eq!(help.status.code(), Some(0));
    let printed = String::from_utf8(help.stdout).unwrap();
    assert!(printed.starts_with("usage: lamp-policy "), "{printed}");
    assert!(printed.contains("--deadline-ms") && printed.contains("played tests"));
}

#[test]
fn a_log_directory_others_can_enter_or_a_missing_key_ends_it_before_it_listens() {
    use std::os::unix::fs::PermissionsExt;
    let places = Places::new("refused");
    let root = places.root.to_str().unwrap();
    let key = places.kept.join("key");
    let open = places.kept.join("open");
    fs::create_dir(&open).unwrap();
    fs::set_permissions(&open, fs::Permissions::from_mode(0o755)).unwrap();
    let run = |key: &Path, log_dir: &Path| {
        Command::new(PROGRAM)
            .arg("--key-file")
            .arg(key)
            .arg("--log-dir")
            .arg(log_dir)
            .args(["--bus-root", root, "--endpoint", "http://127.0.0.1:9/"])
            .output()
            .unwrap()
    };
    // The caller's mistake, like a wrong option.
    let output = run(&key, &open);
    assert_eq!(output.status.code(), Some(2));
    let printed = String::from_utf8(output.stderr).unwrap();
    assert!(
        printed.contains("can be entered by its group or by everyone (mode 755)"),
        "{printed}"
    );
    // No key: nothing can be asked, so nothing is started.
    let output = run(&places.kept.join("no-key"), &places.kept.join("decisions"));
    assert_eq!(output.status.code(), Some(1));
    let printed = String::from_utf8(output.stderr).unwrap();
    assert!(printed.starts_with("lamp-policy: key file "), "{printed}");
    // A key file that holds something else is refused without showing it.
    fs::write(&key, format!("Bearer {KEY}\n")).unwrap();
    let output = run(&key, &places.kept.join("decisions"));
    assert_eq!(output.status.code(), Some(1));
    let printed = String::from_utf8(output.stderr).unwrap();
    assert!(
        printed.contains("does not hold one key") && !printed.contains(KEY),
        "{printed}"
    );
    // Neither run subscribed to anything.
    assert!(Bus::new(&places.root).unwrap().topics().unwrap().is_empty());
}

/// `--act attention` (#800): "is this said to the lamp?" is published, and
/// written down, with `shadow: false`, for the voice to hold the lamp's
/// actions by; the start line says so. Without it every attention decision
/// stays shadow (the tests above).
#[test]
fn with_act_attention_the_attention_decision_is_published_for_real() {
    let places = Places::new("act");
    let (port, _requests) = service();
    let endpoint = format!("http://127.0.0.1:{port}/api/alpha/decisions");
    let bus = Bus::new(&places.root).unwrap();
    let [heard_topic, said_topic] = Speech::TOPICS;
    let mut heard = bus
        .publisher_declared(heard_topic, &Speech::declaration(), "voice")
        .unwrap();
    let _said = bus
        .publisher_declared(said_topic, &Speech::declaration(), "voice")
        .unwrap();
    let mut decisions = bus
        .subscribe_typed(Decision::TOPIC, "test", Decision::TYPE)
        .unwrap();
    let running = start_with(&places, &endpoint, &["--act", "attention"]);
    for topic in [heard_topic, said_topic, VoiceState::TOPIC] {
        listening(&bus, topic);
    }
    for text in ["I left", " the lamp on", " in the kitchen."] {
        heard
            .publish_json(
                0,
                &Speech {
                    text: text.into(),
                    finished: false,
                    locale: None,
                    request: Some(4),
                },
            )
            .unwrap();
        thread::sleep(Duration::from_millis(50));
    }
    let message = decisions.recv(WAIT).unwrap().expect("a decision");
    let decision: Decision = message.parse().unwrap();
    assert_eq!(decision.check(), Ok(()));
    assert_eq!(
        (decision.about.as_str(), decision.shadow, decision.request),
        ("attention", false, Some(4))
    );
    let lines = log(&places.kept.join("decisions"), 1);
    assert_eq!(lines[0]["shadow"], false);
    let output = running.end(&["lamp-policy: started", "lamp-policy: decision"]);
    let printed = String::from_utf8_lossy(&output.stderr);
    let started = printed
        .lines()
        .find(|line| line.starts_with("lamp-policy: started"))
        .unwrap();
    assert!(started.contains(" shadow=false"), "{started}");
    assert!(started.contains(" act=attention"), "{started}");
}

/// #871: with `--images`, each question the policy asks is asked again of
/// the images model, without the eyes' frame and with it, and written to
/// `pictures-YYYY-MM-DD.jsonl`; the decision is published as before, and
/// the frame is sent once, with TypeSafe's header, and written nowhere.
#[test]
fn with_images_the_question_is_asked_again_with_the_eyes_frame_beside_the_decision() {
    let places = Places::new("pictures");
    let (port, requests) = service();
    let endpoint = format!("http://127.0.0.1:{port}/api/alpha/decisions");
    let bus = Bus::new(&places.root).unwrap();
    let [heard_topic, _] = Speech::TOPICS;
    let mut heard = bus
        .publisher_declared(heard_topic, &Speech::declaration(), "voice")
        .unwrap();
    let mut decisions = bus
        .subscribe_typed(Decision::TOPIC, "test", Decision::TYPE)
        .unwrap();
    let sight = places.kept.join("sight.jpg");
    let sight_arg = sight.to_str().unwrap().to_owned();
    let running = start_with(
        &places,
        &endpoint,
        &["--images", "typesafe/jev-1.14", "--sight-file", &sight_arg],
    );
    listening(&bus, heard_topic);
    // The eyes' frame, as they keep one: 320 by 240, fresh.
    let mut frame = Vec::new();
    jpeg_encoder::Encoder::new(&mut frame, 80)
        .encode(
            &vec![100u8; 320 * 240 * 3],
            320,
            240,
            jpeg_encoder::ColorType::Rgb,
        )
        .unwrap();
    fs::write(&sight, &frame).unwrap();
    // Someone at the lamp, as the eyes see: the picture is asked about only
    // then (#871).
    let mut eyes = bus
        .publisher_declared(Faces::TOPIC, &Faces::declaration(), "eyes")
        .unwrap();
    listening(&bus, Faces::TOPIC);
    let near = Face {
        speaking: None,
        id: "f1".into(),
        xmin: 0.3,
        ymin: 0.2,
        xmax: 0.6,
        ymax: 0.6,
        confidence: 0.9,
        yaw: None,
        facing: Some(true),
    };
    eyes.publish_json(0, &Faces { faces: vec![near] }).unwrap();
    thread::sleep(Duration::from_millis(50));
    for text in ["Hey Lamp,", " what time is it?"] {
        let piece = Speech {
            text: text.into(),
            finished: false,
            locale: None,
            request: None,
        };
        heard.publish_json(0, &piece).unwrap();
        thread::sleep(Duration::from_millis(50));
    }
    let decision: Decision = decisions
        .recv(WAIT)
        .unwrap()
        .expect("a decision")
        .parse()
        .unwrap();
    assert_eq!(decision.answers.len(), 7);

    // The picture's line, beside the decisions.
    let started = Instant::now();
    let line: Value = loop {
        let found = fs::read_dir(places.kept.join("decisions"))
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| entry.file_name().to_string_lossy().starts_with("pictures-"))
            .and_then(|entry| fs::read_to_string(entry.path()).ok())
            .and_then(|text| text.lines().next().map(str::to_owned));
        if let Some(text) = found {
            break serde_json::from_str(&text).unwrap();
        }
        assert!(started.elapsed() < WAIT, "no picture line");
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(line["about"], "attention_picture");
    assert_eq!(line["picture"]["sent"], true, "{}", line["picture"]);
    assert_eq!(line["picture"]["width"], 640);
    assert_eq!(line["without"]["by"], "typesafe/jev-1.13-20260917");
    assert_eq!(line["with"]["answers"].as_object().unwrap().len(), 13);
    let output = running.end(&["lamp-policy: started"]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(" images=typesafe/jev-1.14"), "{stderr}");

    // Three askings: the decision's, and the picture's two; only one with
    // the frame and the header.
    let asked: Vec<(String, Value)> = requests.try_iter().collect();
    assert_eq!(asked.len(), 3, "{asked:?}");
    let with: Vec<&(String, Value)> = asked
        .iter()
        .filter(|(_, body)| body.get("assets").is_some())
        .collect();
    assert_eq!(with.len(), 1);
    assert!(with[0].0.contains("TypeSafe-Danger-Zone-Images: true"));
    assert_eq!(with[0].1["model"], "typesafe/jev-1.14");
    for (head, _) in asked
        .iter()
        .filter(|(_, body)| body.get("assets").is_none())
    {
        assert!(!head.contains("TypeSafe-Danger-Zone-Images"));
    }
}
