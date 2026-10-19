//! A skill's tool call, as the voice makes it, while the program also
//! listens on several bus topics for its other skills' triggers: on the
//! host's clock, as on a lamp. The voice waits for this answer before the
//! model speaks, so the person hears the time it takes (#1806).
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    process::{Child, Command},
    time::{Duration, Instant},
};

/// A tool that only says a line: its answer is the line, at once.
const WAVE: &str = r#"
tool = "wave"
[[do]]
say = "Say hello."
"#;

/// Skills on three other topics, so the program listens on three: each
/// skill's name, topic, condition and the Policy's question it asks.
const ON_TOPICS: [(&str, &str, &str, &str); 3] = [
    (
        "faces",
        "humans/faces/tracked",
        "when = \"appears\"",
        "hello",
    ),
    (
        "air",
        "air/state",
        "field = \"co2_ppm\"\nabove = 1200",
        "reminder",
    ),
    (
        "rests",
        "voice/state",
        "field = \"state\"\nequals = \"resting\"",
        "nudge",
    ),
];

/// A skill folder as the program loads it: its `SKILL.md` and `skill.toml`,
/// given the topic it listens on and its voice.
fn skill(dir: &std::path::Path, name: &str, toml: &str, topic: Option<&str>) {
    let folder = dir.join(name);
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: A test.\n---\n"),
    )
    .unwrap();
    let topics = topic.map_or(String::new(), |topic| format!("\"{topic}\""));
    fs::write(
        folder.join("skill.toml"),
        format!("{toml}\n[permissions]\ntopics = [{topics}]\nspeak = true\n"),
    )
    .unwrap();
}

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn a_tool_call_is_answered_while_the_program_listens_on_three_topics() {
    // Short: a socket's whole path has to fit in 103 bytes.
    let dir = tempfile::Builder::new()
        .prefix("cw")
        .tempdir_in("/tmp")
        .unwrap();
    let root = dir.path();
    let skills = root.join("skills");
    fs::create_dir(&skills).unwrap();
    skill(&skills, "wave", WAVE, None);
    for (name, topic, condition, policy) in ON_TOPICS {
        let toml = format!(
            "[trigger]\ntopic = \"{topic}\"\n{condition}\n[ask]\npolicy = \"{policy}\"\n\
             [[do]]\nsay = \"Something came.\"\n"
        );
        skill(&skills, name, &toml, Some(topic));
    }
    let tools = root.join("tools");
    let _running = Running(
        Command::new(env!("CARGO_BIN_EXE_lamp-skills"))
            .args([
                "--dir".as_ref(),
                skills.as_os_str(),
                "--bus-root".as_ref(),
                root.join("bus").as_os_str(),
                "--actions-socket".as_ref(),
                root.join("actions.sock").as_os_str(),
                "--mood-socket".as_ref(),
                root.join("mood.sock").as_os_str(),
                "--tools-dir".as_ref(),
                tools.as_os_str(),
            ])
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    while !tools.join("tools.json").exists() {
        assert!(Instant::now() < deadline, "no tools file");
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut took = Vec::new();
    for _ in 0..10 {
        // A moment apart, as calls come.
        std::thread::sleep(Duration::from_millis(30));
        let started = Instant::now();
        let mut stream = UnixStream::connect(tools.join("call.sock")).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        writeln!(stream, r#"{{"op":"call","name":"wave","args":{{}}}}"#).unwrap();
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        took.push(started.elapsed());
        assert!(line.contains("Say hello."), "{line}");
    }
    took.sort();
    eprintln!("tool calls, fastest to slowest: {took:?}");
    // Taken as it comes, not after a topic's turn: about 49 ms each before
    // #1806 on the Intel Mac, with these three topics, and under 1 ms after.
    let middle = took[took.len() / 2];
    assert!(middle < Duration::from_millis(25), "{took:?}");
}
