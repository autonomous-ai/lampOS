//! The `lamp-bus` binary, run the way a shell runs it, with the test as the
//! other end of the bus.
use lamp_bus::{Bus, Declaration, Kind, Published};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    os::unix::net::UnixDatagram,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const BINARY: &str = env!("CARGO_BIN_EXE_lamp-bus");
/// Long enough for a process to have started, on a machine doing other
/// things as well.
const WAIT: Duration = Duration::from_secs(20);

/// A directory of one test's own, removed when the test ends however it
/// ends. The name is short: a socket's whole path has to fit in 103 bytes.
struct Root(PathBuf);

/// A number in base 36, the shortest a file name can carry it.
fn short(mut number: u32) -> String {
    let mut digits = Vec::new();
    loop {
        digits.push(char::from_digit(number % 36, 36).unwrap());
        number /= 36;
        if number == 0 {
            break digits.iter().rev().collect();
        }
    }
}

impl Root {
    fn new() -> Self {
        static MADE: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "c{}_{}",
            short(std::process::id()),
            short(MADE.fetch_add(1, Ordering::Relaxed))
        ));
        let _ = fs::remove_dir_all(&path);
        Self(path)
    }

    fn bus(&self) -> Bus {
        Bus::new(&self.0).unwrap()
    }

    /// The subscribers a publisher would find: a socket has its `.sock`
    /// name from the moment it can be sent to, and another before.
    fn sockets(&self, topic: &str) -> Vec<String> {
        fs::read_dir(self.0.join(topic).join("subs"))
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .filter(|name| name.ends_with(".sock"))
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A `lamp-bus` still running, killed if the test ends before it does.
struct Running(Option<Child>);

impl Running {
    fn start(root: &Root, arguments: &[&str]) -> Self {
        Self(Some(
            command(root, arguments)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        ))
    }

    fn output(mut self) -> Output {
        self.0.take().unwrap().wait_with_output().unwrap()
    }

    fn kill(mut self) {
        let mut child = self.0.take().unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
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

fn command(root: &Root, arguments: &[&str]) -> Command {
    let mut command = Command::new(BINARY);
    command
        .arg("--root")
        .arg(&root.0)
        .args(arguments)
        .env_remove("LAMP_BUS_ROOT");
    command
}

fn run(root: &Root, arguments: &[&str]) -> Output {
    command(root, arguments)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap()
}

fn until(what: &str, mut there: impl FnMut() -> bool) {
    let started = Instant::now();
    while !there() {
        assert!(started.elapsed() < WAIT, "{what} did not come");
        thread::sleep(Duration::from_millis(5));
    }
}

/// A printed message: the header, and what follows it on the line.
fn printed(line: &str) -> (Value, &str) {
    let mut values = serde_json::Deserializer::from_str(line).into_iter::<Value>();
    let header = values.next().expect("a header").expect("of JSON");
    (header, line[values.byte_offset()..].trim_start())
}

fn lines(path: &Path) -> Vec<Value> {
    let Ok(recording) = fs::read_to_string(path) else {
        return Vec::new();
    };
    assert!(
        recording.is_empty() || recording.ends_with('\n'),
        "half a line"
    );
    recording
        .lines()
        .map(|line| serde_json::from_str(line).expect("a line of JSON"))
        .collect()
}

/// A live recorder may be observed partway through its current write. Count
/// only a snapshot ending at a record boundary; the unchanged `lines` reader
/// still parses every byte and rejects partial records after the child exits.
fn written_lines(path: &Path) -> Option<usize> {
    let recording = match fs::read(path) {
        Ok(recording) => recording,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => panic!("read live recording {}: {error}", path.display()),
    };
    if !recording.is_empty() && !recording.ends_with(b"\n") {
        return None;
    }
    Some(recording.iter().filter(|byte| **byte == b'\n').count())
}

#[test]
fn a_live_recording_waits_for_the_whole_snapshot_before_counting_lines() {
    let root = Root::new();
    fs::create_dir_all(&root.0).unwrap();
    let file = root.0.join("live.jsonl");
    assert_eq!(written_lines(&file), None);
    fs::write(&file, b"").unwrap();
    assert_eq!(written_lines(&file), Some(0));

    // Even a complete JSON value is not a committed line without its LF.
    fs::write(&file, b"{\"seq\":1}").unwrap();
    assert_eq!(written_lines(&file), None);
    fs::write(&file, b"{\"seq\":1}\n").unwrap();
    assert_eq!(written_lines(&file), Some(1));

    // Do not accept the expected first line while a later write is partial.
    fs::write(&file, b"{\"seq\":1}\n{\"seq\":").unwrap();
    assert_eq!(written_lines(&file), None);
    assert!(std::panic::catch_unwind(|| lines(&file)).is_err());
    fs::write(&file, b"{\"seq\":1}\n{\"seq\":2}\n").unwrap();
    assert_eq!(written_lines(&file), Some(2));
    assert_eq!(lines(&file), vec![json!({"seq": 1}), json!({"seq": 2})]);

    // A boundary only gates observation: it never replaces JSON validation.
    fs::write(&file, b"not JSON\n").unwrap();
    assert_eq!(written_lines(&file), Some(1));
    assert!(std::panic::catch_unwind(|| lines(&file)).is_err());
}

fn sound() -> Vec<u8> {
    (0..640u32).map(|index| (index * 7) as u8).collect()
}

/// A publisher in a thread of its own, as another process would be one:
/// it declares its topic if told what it carries, publishes every `every`
/// until it is stopped, and then ends as a publisher does.
struct Publishing {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<u64>>,
}

impl Publishing {
    fn start(
        bus: Bus,
        topic: &'static str,
        declaration: Option<Declaration>,
        every: Duration,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = thread::spawn({
            let stop = Arc::clone(&stop);
            move || {
                let mut publisher = match &declaration {
                    Some(declaration) => bus.publisher_declared(topic, declaration, "body"),
                    None => bus.publisher(topic, Kind::Event, "body"),
                }
                .unwrap();
                let started = Instant::now();
                let mut sent = 0u64;
                while !stop.load(Ordering::SeqCst) {
                    sent += 1;
                    let at_us = started.elapsed().as_micros() as u64;
                    publisher.publish_json(at_us, &json!({"n": sent})).unwrap();
                    thread::sleep(every);
                }
                sent
            }
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }

    /// Stop it, and say how many it published.
    fn end(mut self) -> u64 {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap()
    }
}

impl Drop for Publishing {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The `key=value` words of a line the tool printed.
fn words(line: &str) -> HashMap<&str, &str> {
    line.split(' ')
        .filter_map(|word| word.split_once('='))
        .collect()
}

#[test]
fn pub_then_latest_prints_what_was_published_and_a_topic_that_keeps_nothing_is_status_1() {
    let root = Root::new();
    let nothing = run(&root, &["latest", "servos"]);
    assert_eq!(nothing.status.code(), Some(1));
    assert!(nothing.stdout.is_empty());
    assert!(text(&nothing.stderr).contains("servos keeps no value"));
    assert!(!text(&nothing.stderr).contains("usage:"));

    let value = r#"{"yaw": 1990, "note": "two\nlines"}"#;
    let published = run(&root, &["pub", "servos", "latest", value, "--at-us", "42"]);
    assert!(published.status.success(), "{}", text(&published.stderr));
    assert_eq!(text(&published.stdout), "delivered 0 dropped 0 removed 0\n");

    let latest = run(&root, &["latest", "servos"]);
    assert!(latest.status.success());
    let shown = text(&latest.stdout);
    assert_eq!(shown.matches('\n').count(), 1, "one line: {shown:?}");
    let (header, payload) = printed(shown.trim_end());
    assert_eq!(header["topic"], "servos");
    assert_eq!(header["kind"], "latest");
    assert_eq!(header["enc"], "json");
    assert_eq!(header["source"], "lamp_bus_cli");
    assert_eq!(header["seq"], 1);
    assert_eq!(header["at_us"], 42);
    assert_eq!(header["boot"].as_str().unwrap().len(), 16);
    assert_eq!(
        serde_json::from_str::<Value>(payload).unwrap(),
        json!({"yaw": 1990, "note": "two\nlines"})
    );
    // What the tool published is what the library reads.
    let kept = root.bus().latest("servos").unwrap().unwrap();
    assert_eq!(kept.json().unwrap()["yaw"], 1990);

    // The root from the environment when none is given.
    let by_environment = Command::new(BINARY)
        .args(["latest", "servos"])
        .env("LAMP_BUS_ROOT", &root.0)
        .output()
        .unwrap();
    assert_eq!(by_environment.stdout, latest.stdout);

    // An event is over when it has been sent: nothing of it is kept.
    assert!(run(&root, &["pub", "mic2", "event", "1"]).status.success());
    assert_eq!(run(&root, &["latest", "mic2"]).status.code(), Some(1));
}

#[test]
fn tap_prints_each_message_published_while_it_listens_and_ends_at_its_count() {
    let root = Root::new();
    let topic = "gemini/transcript";
    let tap = Running::start(
        &root,
        &[
            "tap",
            topic,
            "--count",
            "2",
            "--seconds",
            "60",
            "--name",
            "ear",
        ],
    );
    until("the tap's socket", || root.sockets(topic).len() == 1);
    assert!(root.sockets(topic)[0].starts_with("ear."));
    let mut publisher = root.bus().publisher(topic, Kind::Event, "voice").unwrap();
    let said = json!({"said": "hello\nlamp"});
    assert_eq!(publisher.publish_json(7, &said).unwrap().delivered, 1);
    let outcome = publisher.publish_bytes(8, "pcm_s16le_16000_mono", &sound());
    assert_eq!(outcome.unwrap().delivered, 1);
    let output = tap.output();
    assert!(output.status.success(), "{}", text(&output.stderr));
    let shown: Vec<&str> = text(&output.stdout).lines().collect();
    assert_eq!(shown.len(), 2, "{shown:?}");
    let (header, payload) = printed(shown[0]);
    assert_eq!(header["topic"], topic);
    assert_eq!(header["source"], "voice");
    assert_eq!((&header["seq"], &header["at_us"]), (&json!(1), &json!(7)));
    assert_eq!(serde_json::from_str::<Value>(payload).unwrap(), said);
    let (header, payload) = printed(shown[1]);
    assert_eq!(header["enc"], "pcm_s16le_16000_mono");
    assert_eq!(header["seq"], 2);
    assert_eq!(payload, "<640 bytes>");
    // It unsubscribed on its way out.
    assert!(root.sockets(topic).is_empty());

    // One asked for and none come in the time: status 1 and nothing printed.
    let silent = run(&root, &["tap", topic, "--count", "1", "--seconds", "0.3"]);
    assert_eq!(silent.status.code(), Some(1));
    assert!(silent.stdout.is_empty());
    // With no count, the time running out is the ordinary end.
    assert!(
        run(&root, &["tap", topic, "--seconds", "0.1"])
            .status
            .success()
    );
    assert!(root.sockets(topic).is_empty());
}

#[test]
fn record_then_replay_gives_a_subscriber_the_messages_again_at_their_spacing() {
    let root = Root::new();
    let kept = Root::new();
    let bus = root.bus();
    let directory = kept.0.to_str().unwrap();
    let recorder = Running::start(
        &root,
        &[
            "record",
            directory,
            "servos",
            "gemini/transcript",
            "--seconds",
            // Counted from when every topic is bound, and all is sent as soon
            // as both sockets are seen: room enough (#1128).
            "0.75",
        ],
    );
    until("the recorder's sockets", || {
        root.sockets("servos").len() == 1 && root.sockets("gemini/transcript").len() == 1
    });
    let mut pose = bus.publisher("servos", Kind::Latest, "body").unwrap();
    pose.expires_after(Some(Duration::from_millis(500)));
    let mut words = bus
        .publisher("gemini/transcript", Kind::Event, "voice")
        .unwrap();
    // Forty milliseconds apart by the publisher's clock, whatever the time
    // between them here.
    let at = |n: u64| 1_000_000 + n * 40_000;
    for n in 0..6u64 {
        let outcome = pose.publish_json(at(n), &json!({"yaw": n})).unwrap();
        assert_eq!(outcome.delivered, 1);
    }
    let every_byte: Vec<u8> = (0..=255).collect();
    let text_of = json!({"text": "hello"});
    assert_eq!(words.publish_json(5, &text_of).unwrap().delivered, 1);
    assert_eq!(
        words
            .publish_bytes(6, "x-test", &every_byte)
            .unwrap()
            .delivered,
        1
    );
    assert_eq!(words.publish_bytes(7, "none", b"").unwrap().delivered, 1);
    let output = recorder.output();
    let said = text(&output.stderr);
    assert!(output.status.success(), "{said}");
    assert!(
        said.contains("6 of servos in ") && said.contains(", 0 missed"),
        "{said}"
    );
    assert!(said.contains("3 of gemini/transcript in "), "{said}");
    assert!(root.sockets("servos").is_empty());

    // A line is the header and the payload in base64, and nothing else.
    let pose_file = kept.0.join("servos.jsonl");
    let words_file = kept.0.join("gemini_transcript.jsonl");
    let recorded = lines(&pose_file);
    assert_eq!(recorded.len(), 6);
    for (n, line) in recorded.iter().enumerate() {
        assert_eq!(line.as_object().unwrap().len(), 2);
        assert_eq!(line["header"]["seq"], n as u64 + 1);
        assert_eq!(line["header"]["source"], "body");
        assert_eq!(line["header"]["kind"], "latest");
        assert_eq!(line["header"]["expires_ms"], 500);
    }
    assert_eq!(recorded[0]["payload_b64"], "eyJ5YXciOjB9");
    let recorded = lines(&words_file);
    assert_eq!(recorded.len(), 3);
    assert_eq!(recorded[0]["payload_b64"], "eyJ0ZXh0IjoiaGVsbG8ifQ==");
    let all = recorded[1]["payload_b64"].as_str().unwrap();
    assert!(
        all.starts_with("AAECAwQFBgcICQoL") && all.ends_with("+fr7/P3+/w=="),
        "{all}"
    );
    assert_eq!(all.len(), 344);
    assert_eq!(recorded[2]["payload_b64"], "");

    // Played back to a subscriber. The topic's kept value is taken first,
    // so that what follows is the replay and nothing else.
    let mut follow = bus.subscribe("servos", "follow").unwrap();
    assert_eq!(follow.latest().unwrap().unwrap().header.seq, 6);
    let started = Instant::now();
    let replayed = run(
        &root,
        &["replay", pose_file.to_str().unwrap(), "--speed", "2"],
    );
    let took = started.elapsed();
    let said = text(&replayed.stderr);
    assert!(replayed.status.success(), "{said}");
    assert!(
        said.contains("6 messages, 6 delivered, 0 dropped"),
        "{said}"
    );
    // Five spaces of 40 ms at twice the speed.
    assert!(took >= Duration::from_millis(100), "{took:?}");
    for n in 0..6u64 {
        let message = follow.recv(WAIT).unwrap().expect("a replayed message");
        assert_eq!(message.header.source, "replay");
        assert_eq!(message.header.kind, Kind::Latest);
        assert_eq!((message.header.seq, message.gap), (n + 1, 0));
        assert_eq!(message.header.at_us, at(n));
        assert_eq!(message.header.expires_ms, Some(500));
        assert_eq!(message.json().unwrap(), json!({"yaw": n}));
    }
    let now = bus.latest("servos").unwrap().unwrap();
    assert_eq!((now.header.source.as_str(), now.header.seq), ("replay", 6));

    // At once, and bytes that are not text come back as they were.
    let mut ear = bus.subscribe("gemini/transcript", "ear").unwrap();
    let replayed = run(
        &root,
        &["replay", words_file.to_str().unwrap(), "--speed", "0"],
    );
    assert!(replayed.status.success(), "{}", text(&replayed.stderr));
    let message = ear.recv(WAIT).unwrap().expect("the words");
    assert_eq!(
        (message.header.kind, message.header.at_us),
        (Kind::Event, 5)
    );
    assert_eq!(message.json().unwrap(), text_of);
    let message = ear.recv(WAIT).unwrap().expect("the bytes");
    assert_eq!(message.header.enc, "x-test");
    assert_eq!(message.payload, every_byte);
    let message = ear.recv(WAIT).unwrap().expect("the bare one");
    assert_eq!(
        (message.header.enc.as_str(), message.payload.len()),
        ("none", 0)
    );
    assert_eq!((message.header.seq, message.header.expires_ms), (3, None));

    // A file that is not a recording is said to be wrong where it is wrong.
    fs::write(&words_file, "{\"header\":{},\"payload_b64\":\"\"}\n").unwrap();
    let wrong = run(&root, &["replay", words_file.to_str().unwrap()]);
    assert_eq!(wrong.status.code(), Some(1));
    assert!(text(&wrong.stderr).contains("gemini_transcript.jsonl line 1: "));
    let missing = run(
        &root,
        &["replay", kept.0.join("none.jsonl").to_str().unwrap()],
    );
    assert_eq!(missing.status.code(), Some(1));
}

#[test]
fn a_killed_recorder_leaves_whole_lines_and_its_socket_goes_with_the_next_publish() {
    let root = Root::new();
    let kept = Root::new();
    let directory = kept.0.to_str().unwrap();
    let file = kept.0.join("mic2.jsonl");
    let recorder = Running::start(&root, &["record", directory, "mic2"]);
    until("the recorder's socket", || root.sockets("mic2").len() == 1);
    let mut publisher = root.bus().publisher("mic2", Kind::Event, "voice").unwrap();
    for n in 0..5u64 {
        let outcome = publisher.publish_bytes(n, "pcm_s16le_16000_mono", &sound());
        assert_eq!(outcome.unwrap().delivered, 1);
        // Each is on the disk before the next: nothing waits in the process.
        until("the line", || written_lines(&file) == Some(n as usize + 1));
    }
    recorder.kill();
    let recorded = lines(&file);
    assert_eq!(recorded.len(), 5);
    for (n, line) in recorded.iter().enumerate() {
        assert_eq!(line["header"]["seq"], n as u64 + 1);
    }
    // Killed, it could not take its socket with it, and the tool says so.
    assert_eq!(root.sockets("mic2").len(), 1);
    let listed = run(&root, &["topics"]);
    assert_eq!(
        text(&listed.stdout),
        "mic2 subscribers=0 stale=1 latest=no\n"
    );
    let outcome = publisher.publish_bytes(9, "pcm_s16le_16000_mono", &sound());
    assert_eq!(
        outcome.unwrap(),
        Published {
            delivered: 0,
            dropped: 0,
            removed: 1
        }
    );
    assert!(root.sockets("mic2").is_empty());
    // Recording to the same place again adds to what is there.
    let again = run(&root, &["record", directory, "mic2", "--seconds", "0.2"]);
    assert!(again.status.success(), "{}", text(&again.stderr));
    assert!(text(&again.stderr).contains("0 of mic2 in "));
    assert_eq!(lines(&file).len(), 5);
}

#[test]
fn a_recording_stops_at_its_size_and_says_so() {
    let root = Root::new();
    let kept = Root::new();
    let directory = kept.0.to_str().unwrap();
    // 2,621 bytes: two lines of sound, a little over a thousand bytes
    // each, and not a third.
    let recorder = Running::start(
        &root,
        &[
            "record",
            directory,
            "mic2",
            "--max-mb",
            "0.0025",
            "--seconds",
            "60",
        ],
    );
    until("the recorder's socket", || root.sockets("mic2").len() == 1);
    let mut publisher = root.bus().publisher("mic2", Kind::Event, "voice").unwrap();
    for n in 0..3u64 {
        let outcome = publisher.publish_bytes(n, "pcm_s16le_16000_mono", &sound());
        assert_eq!(outcome.unwrap().delivered, 1);
        if n < 2 {
            until("the line", || {
                written_lines(&kept.0.join("mic2.jsonl")) == Some(n as usize + 1)
            });
        }
    }
    // Its one topic full, it has nothing left to do and ends by itself.
    let started = Instant::now();
    let output = recorder.output();
    assert!(started.elapsed() < Duration::from_secs(30));
    let said = text(&output.stderr);
    assert!(output.status.success(), "{said}");
    assert!(
        said.contains("mic2.jsonl has reached 0.0025 MiB: no more of mic2 is recorded"),
        "{said}"
    );
    assert!(
        said.contains("2 of mic2 in ") && said.contains(", file full"),
        "{said}"
    );
    assert_eq!(lines(&kept.0.join("mic2.jsonl")).len(), 2);
    assert!(fs::metadata(kept.0.join("mic2.jsonl")).unwrap().len() <= 2_621);
    assert!(root.sockets("mic2").is_empty());
}

#[test]
fn topics_lists_each_topic_with_who_listens_and_whether_a_value_is_kept() {
    let root = Root::new();
    let empty = run(&root, &["topics"]);
    assert!(empty.status.success() && empty.stdout.is_empty());
    let bus = root.bus();
    let listeners = (
        bus.subscribe("servos", "follow").unwrap(),
        bus.subscribe("servos", "eyes").unwrap(),
        bus.subscribe("gemini/transcript", "memory").unwrap(),
    );
    let mut pose = bus.publisher("servos", Kind::Latest, "body").unwrap();
    assert_eq!(pose.publish_json(1, &1).unwrap().delivered, 2);
    let mut state = bus.publisher("room/state", Kind::Latest, "eyes").unwrap();
    state.publish_json(1, &json!({"people": 1})).unwrap();
    let listed = run(&root, &["topics"]);
    assert!(listed.status.success());
    assert_eq!(
        text(&listed.stdout),
        "gemini/transcript subscribers=1 latest=no\n\
         room/state subscribers=0 latest=yes\n\
         servos subscribers=2 latest=yes\n"
    );
    drop(listeners);
}

#[test]
fn bad_input_prints_the_usage_and_is_status_2_with_nothing_touched() {
    let root = Root::new();
    let elsewhere = root.0.join("recordings");
    let elsewhere = elsewhere.to_str().unwrap();
    let cases: [&[&str]; 32] = [
        &[],
        &["frobnicate"],
        &["topics", "extra"],
        &["tap"],
        &["tap", "servos", "mic2"],
        &["tap", "../x"],
        &["tap", "/tmp/x"],
        &["tap", "servos", "--name", "A.b"],
        &["tap", "room.state"],
        &["tap", "servos", "--name", "lamp-tap"],
        &["tap", "servos", "--count", "many"],
        &["tap", "servos", "--seconds"],
        &["tap", "servos", "--seconds", "-1"],
        &["tap", "servos", "--speed", "2"],
        &["tap", "servos", "--colour", "red"],
        &["latest"],
        &["latest", "A"],
        &["pub", "servos", "latest"],
        &["pub", "servos", "sometimes", "1"],
        &["pub", "servos", "latest", "{not json"],
        &["record", elsewhere],
        &["record", elsewhere, "a/b", "a_b"],
        &["replay"],
        &["replay", "recording.jsonl", "--speed", "-1"],
        &["info"],
        &["info", "servos", "mic2"],
        &["info", "room.state"],
        &["hz"],
        &["hz", "servos", "--seconds", "soon"],
        &["hz", "servos", "--count", "3"],
        &["pub", "servos", "latest", "1", "--type", "joints"],
        &["tap", "servos", "--type", "lamp_msgs/JointState"],
    ];
    for arguments in cases {
        let output = run(&root, arguments);
        let said = text(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}: {said}");
        assert!(said.starts_with("lamp-bus: "), "{arguments:?}: {said}");
        assert!(
            said.contains("usage: lamp-bus [--root PATH] COMMAND"),
            "{arguments:?}"
        );
        assert!(output.stdout.is_empty(), "{arguments:?}");
    }
    assert!(!root.0.exists());
    // A root that is not the same place to every process is refused too.
    let relative = Command::new(BINARY)
        .args(["--root", "run/bus", "topics"])
        .output()
        .unwrap();
    assert_eq!(relative.status.code(), Some(2));
    assert!(text(&relative.stderr).contains("not an absolute path"));
    // Asked for, the usage is the answer and not an error.
    let help = run(&root, &["--help"]);
    assert!(help.status.success() && help.stderr.is_empty());
    assert!(text(&help.stdout).starts_with("usage: lamp-bus"));
}

#[test]
fn info_says_what_a_topic_carries_who_publishes_it_who_listens_and_how_old_its_value_is() {
    let root = Root::new();
    let bus = root.bus();
    let topic = "body/joint_states";
    // A topic nobody has touched is not on the bus.
    let unknown = run(&root, &["info", topic]);
    assert_eq!(unknown.status.code(), Some(1));
    assert!(unknown.stdout.is_empty());
    assert!(
        text(&unknown.stderr).contains("body/joint_states is not on the bus"),
        "{}",
        text(&unknown.stderr)
    );
    assert!(!root.0.exists());

    let joints = Declaration::new("lamp_msgs/JointState", 1, Kind::Latest)
        .every(Duration::from_millis(20))
        .lasting(Duration::from_secs(60));
    let publishing = Publishing::start(
        bus.clone(),
        topic,
        Some(joints.clone()),
        Duration::from_millis(20),
    );
    let follow = bus.subscribe(topic, "f").unwrap();
    until("the value", || {
        bus.latest(topic).is_ok_and(|kept| kept.is_some())
    });
    // A subscriber whose process died, for the tool to tell from a live one.
    drop(UnixDatagram::bind(root.0.join(topic).join("subs/ghost.1.0.sock")).unwrap());
    // Whether the publisher has found and removed it by now is its affair.
    let shown = run(&root, &["info", topic]);
    assert!(shown.status.success(), "{}", text(&shown.stderr));
    let said = text(&shown.stdout);
    let lines: Vec<&str> = said
        .lines()
        .filter(|line| !line.starts_with("subscriber ghost."))
        .collect();
    assert_eq!(lines.len(), 5, "{said}");
    assert_eq!(lines[0], "topic body/joint_states");
    assert_eq!(
        lines[1],
        "declaration type=lamp_msgs/JointState version=1 kind=latest period_ms=20 \
         lifespan_ms=60000"
    );
    let publisher = words(lines[2]);
    assert!(lines[2].starts_with("publisher body pid="), "{said}");
    assert_eq!(publisher["pid"], std::process::id().to_string());
    assert_eq!(publisher["boot"].len(), 16);
    // Refreshed every second: never a lease old while it publishes.
    assert!(
        publisher["alive_ms"].parse::<u64>().unwrap() < 3_000,
        "{said}"
    );
    assert!(
        lines[3].starts_with(&format!("subscriber f.{}.", std::process::id()))
            && lines[3].ends_with(" listening"),
        "{said}"
    );
    let latest = words(lines[4]);
    assert!(lines[4].starts_with("latest source=body seq="), "{said}");
    assert!(latest["seq"].parse::<u64>().unwrap() >= 1);
    assert!(latest["age_ms"].parse::<u64>().unwrap() < 5_000, "{said}");
    assert_eq!(latest["expired"], "no");

    // `topics` says the type beside what it said before.
    let listed = run(&root, &["topics"]);
    assert!(
        text(&listed.stdout).starts_with("body/joint_states subscribers=1 "),
        "{}",
        text(&listed.stdout)
    );
    assert!(
        text(&listed.stdout).ends_with("latest=yes type=lamp_msgs/JointState\n"),
        "{}",
        text(&listed.stdout)
    );

    // The tool publishing with the topic's type joins it and leaves what
    // its own publisher promised of it alone.
    let joined = run(
        &root,
        &[
            "pub",
            topic,
            "latest",
            "{\"n\":0}",
            "--type",
            "lamp_msgs/JointState",
        ],
    );
    assert!(joined.status.success(), "{}", text(&joined.stderr));
    assert_eq!(bus.declaration(topic).unwrap(), Some(joints));
    // With another type, or the same as another kind, it is told why not.
    for (kind, type_name) in [
        ("latest", "lamp_msgs/Faces"),
        ("event", "lamp_msgs/JointState"),
    ] {
        let refused = run(&root, &["pub", topic, kind, "1", "--type", type_name]);
        assert_eq!(refused.status.code(), Some(1), "{kind} {type_name}");
        let why = text(&refused.stderr);
        assert!(
            why.contains("already declared as lamp_msgs/JointState (latest)")
                && !why.contains("usage:"),
            "{why}"
        );
    }

    // The publisher and the subscriber end: the topic keeps what it was
    // declared to be and its last value, and has nobody.
    assert!(publishing.end() >= 1);
    drop(follow);
    let _ = fs::remove_file(root.0.join(topic).join("subs/ghost.1.0.sock"));
    let shown = run(&root, &["info", topic]);
    let said = text(&shown.stdout);
    let lines: Vec<&str> = said.lines().collect();
    assert_eq!(lines.len(), 5, "{said}");
    assert!(lines[1].starts_with("declaration type=lamp_msgs/JointState "));
    assert_eq!(lines[2], "publishers none");
    assert_eq!(lines[3], "subscribers none");
    assert!(lines[4].starts_with("latest source="), "{said}");

    // A topic of events that the tool itself declared, and a subscriber
    // left behind by a process that died.
    let said = run(
        &root,
        &[
            "pub",
            "voice/heard",
            "event",
            "{\"text\":\"lamp\"}",
            "--type",
            "lamp_msgs/Speech",
        ],
    );
    assert!(said.status.success(), "{}", text(&said.stderr));
    fs::create_dir(root.0.join("voice/heard/subs")).unwrap();
    drop(UnixDatagram::bind(root.0.join("voice/heard/subs/ghost.1.0.sock")).unwrap());
    let shown = run(&root, &["info", "voice/heard"]);
    assert_eq!(
        text(&shown.stdout),
        "topic voice/heard\n\
         declaration type=lamp_msgs/Speech version=1 kind=event\n\
         publishers none\n\
         subscriber ghost.1.0 stale\n\
         latest none\n"
    );
    // A value that has outlived its time is printed, and said to be old.
    let mut ring = bus.publisher("light/state", Kind::Latest, "ring").unwrap();
    ring.expires_after(Some(Duration::from_millis(1)));
    ring.publish_json(1, &json!({"on": true})).unwrap();
    thread::sleep(Duration::from_millis(80));
    let shown = run(&root, &["info", "light/state"]);
    assert!(text(&shown.stdout).ends_with(" expired=yes\n"));
    assert!(text(&shown.stdout).contains("\ndeclaration none\npublisher ring pid="));
    let latest = run(&root, &["latest", "light/state"]);
    assert!(latest.status.success());
    assert!(text(&latest.stdout).contains("\"expires_ms\":1"));
    assert!(text(&latest.stderr).contains("light/state: the value expired "));
}

#[test]
fn hz_measures_a_topic_while_a_publisher_runs_and_is_status_1_when_nothing_comes() {
    let root = Root::new();
    let bus = root.bus();
    let topic = "mic2/audio";
    let publishing = Publishing::start(bus.clone(), topic, None, Duration::from_millis(10));
    until("the publisher", || {
        bus.publishers(topic).is_ok_and(|found| !found.is_empty())
    });
    let measured = run(&root, &["hz", topic, "--seconds", "0.8"]);
    let published = publishing.end();
    assert!(measured.status.success(), "{}", text(&measured.stderr));
    let said = text(&measured.stdout);
    assert_eq!(said.matches('\n').count(), 1, "one line: {said:?}");
    assert!(said.starts_with("mic2/audio messages="), "{said}");
    let measured = words(said.trim_end());
    let number = |key: &str| measured[key].parse::<f64>().unwrap();
    // A message every 10 ms and a little: some sixty of them in 0.8 s less
    // the time the publisher takes to find a new subscriber. The bounds are
    // wide: what is tested is that the numbers are those of this stream.
    let messages = number("messages");
    assert!(messages >= 10.0 && messages <= published as f64, "{said}");
    assert!((0.8..5.0).contains(&number("seconds")), "{said}");
    assert!((10.0..=101.0).contains(&number("rate_hz")), "{said}");
    assert!((9.9..100.0).contains(&number("gap_mean_ms")), "{said}");
    assert!(number("gap_max_ms") >= number("gap_mean_ms"), "{said}");
    assert!(number("age_mean_ms") < 1_000.0, "{said}");
    assert_eq!((measured["missed"], measured["expired"]), ("0", "0"));
    // The rate is the one the mean gap gives.
    let from_gap = 1_000.0 / number("gap_mean_ms");
    assert!(
        (number("rate_hz") - from_gap).abs() < 0.02 * from_gap,
        "{said}"
    );
    assert!(root.sockets(topic).is_empty());

    // Nothing comes: status 1, and nothing is made up.
    let silent = run(&root, &["hz", "room/state", "--seconds", "0.2"]);
    assert_eq!(silent.status.code(), Some(1));
    let said = text(&silent.stdout);
    assert!(
        said.starts_with("room/state messages=0 seconds=0.")
            && said.ends_with(
                " rate_hz=- gap_mean_ms=- gap_max_ms=- age_mean_ms=- missed=0 expired=0\n"
            ),
        "{said}"
    );
    // A value the topic keeps was sent before the tool listened: it is not
    // a message that came.
    let mut state = bus.publisher("room/state", Kind::Latest, "voice").unwrap();
    state.publish_json(1, &json!("quiet")).unwrap();
    let kept = run(&root, &["hz", "room/state", "--seconds", "0.3"]);
    assert_eq!(kept.status.code(), Some(1));
    assert!(text(&kept.stdout).starts_with("room/state messages=0 "));
    // One message is a message and not yet a rate. The tool's seconds start
    // once it listens, and the message goes as soon as its socket is seen,
    // so half a second is room enough (#1128).
    let listening = Running::start(&root, &["hz", "room/state", "--seconds", "0.5"]);
    until("the tool's socket", || {
        root.sockets("room/state").len() == 1
    });
    assert_eq!(state.publish_json(2, &json!("busy")).unwrap().delivered, 1);
    let one = listening.output();
    assert!(one.status.success(), "{}", text(&one.stderr));
    let said = text(&one.stdout);
    assert!(
        said.starts_with("room/state messages=1 ") && said.contains(" rate_hz=- gap_mean_ms=- "),
        "{said}"
    );
    assert_ne!(words(said.trim_end())["age_mean_ms"], "-");
}

#[test]
fn a_recording_begins_with_what_its_topic_was_declared_to_carry_and_replay_declares_it_again() {
    let root = Root::new();
    let kept = Root::new();
    let bus = root.bus();
    let directory = kept.0.to_str().unwrap();
    let joints = Declaration::new("lamp_msgs/JointState", 2, Kind::Latest)
        .every(Duration::from_millis(50))
        .lasting(Duration::from_secs(60));
    // One topic is declared before the recorder starts, one after.
    let mut body = bus
        .publisher_declared("body/joint_states", &joints, "body")
        .unwrap();
    let recorder = Running::start(
        &root,
        &["record", directory, "body/joint_states", "voice/heard"],
    );
    until("the recorder's sockets", || {
        root.sockets("body/joint_states").len() == 1 && root.sockets("voice/heard").len() == 1
    });
    let joints_file = kept.0.join("body_joint_states.jsonl");
    let speech_file = kept.0.join("voice_heard.jsonl");
    // Declared already: the file's first line, before any message.
    until("the declaration", || written_lines(&joints_file) == Some(1));
    assert_eq!(
        lines(&joints_file)[0],
        json!({"topic": "body/joint_states", "declaration": {
            "type": "lamp_msgs/JointState", "version": 2, "kind": "latest",
            "period_ms": 50, "lifespan_ms": 60000}})
    );
    assert!(lines(&speech_file).is_empty());
    let speech = Declaration::new("lamp_msgs/Speech", 1, Kind::Event);
    let mut voice = bus
        .publisher_declared("voice/heard", &speech, "voice")
        .unwrap();
    for n in 0..3u64 {
        assert_eq!(
            body.publish_json(n * 1_000, &json!({"n": n}))
                .unwrap()
                .delivered,
            1
        );
        assert_eq!(
            voice
                .publish_json(n * 1_000, &json!({"text": n}))
                .unwrap()
                .delivered,
            1
        );
    }
    until("the lines", || {
        written_lines(&joints_file) == Some(4) && written_lines(&speech_file) == Some(4)
    });
    recorder.kill();
    // Declared after the recorder began: written before its first message.
    let recorded = lines(&speech_file);
    assert_eq!(
        recorded[0],
        json!({"topic": "voice/heard", "declaration": {
            "type": "lamp_msgs/Speech", "version": 1, "kind": "event"}})
    );
    for (n, line) in recorded[1..].iter().enumerate() {
        assert_eq!(line["header"]["seq"], n as u64 + 1);
        assert_eq!(line["header"]["type"], "lamp_msgs/Speech");
        assert!(line["header"].get("expires_ms").is_none());
    }
    for line in &lines(&joints_file)[1..] {
        assert_eq!(line["header"]["type"], "lamp_msgs/JointState");
        assert_eq!(line["header"]["expires_ms"], 60_000);
    }

    // Replayed on a bus that has never had the topic: declared as it was,
    // and each message of the type it was.
    let other = Root::new();
    let mut follow = other
        .bus()
        .subscribe_typed("body/joint_states", "f", "lamp_msgs/JointState")
        .unwrap();
    assert!(
        other
            .bus()
            .declaration("body/joint_states")
            .unwrap()
            .is_none()
    );
    let replayed = run(
        &other,
        &["replay", joints_file.to_str().unwrap(), "--speed", "0"],
    );
    let said = text(&replayed.stderr);
    assert!(replayed.status.success(), "{said}");
    assert!(
        said.contains("3 messages, 3 delivered, 0 dropped"),
        "{said}"
    );
    assert_eq!(
        other.bus().declaration("body/joint_states").unwrap(),
        Some(joints)
    );
    for n in 0..3u64 {
        let message = follow.recv(WAIT).unwrap().expect("a replayed message");
        assert_eq!(message.header.source, "replay");
        assert_eq!(
            message.header.type_name.as_deref(),
            Some("lamp_msgs/JointState")
        );
        assert_eq!(message.header.expires_ms, Some(60_000));
        assert_eq!(message.json().unwrap(), json!({"n": n}));
    }
    assert_eq!(follow.wrong_type(), 0);

    // On a bus where the topic is something else the replay is refused
    // where the recording declares it, and nothing of it is published.
    let _eyes = other
        .bus()
        .publisher_declared(
            "voice/heard",
            &Declaration::new("lamp_msgs/Faces", 1, Kind::Event),
            "eyes",
        )
        .unwrap();
    let refused = run(&other, &["replay", speech_file.to_str().unwrap()]);
    assert_eq!(refused.status.code(), Some(1));
    let why = text(&refused.stderr);
    assert!(
        why.contains("voice_heard.jsonl line 1: ")
            && why.contains("already declared as lamp_msgs/Faces (event)"),
        "{why}"
    );
    // A line that is neither a message nor a declaration is said to be so.
    fs::write(&speech_file, "{\"topic\":\"voice/heard\"}\n").unwrap();
    let wrong = run(&other, &["replay", speech_file.to_str().unwrap()]);
    assert_eq!(wrong.status.code(), Some(1));
    assert!(
        text(&wrong.stderr).contains("line 1: neither a message"),
        "{}",
        text(&wrong.stderr)
    );
}
