//! Opt-in log following through the public CLI and real local bus sockets.
use lamp_bus::{
    Bus, Declaration, Kind,
    logging::{Log, LogLevel},
};
use serde_json::json;
use std::{
    fs,
    os::unix::net::UnixDatagram,
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

const BINARY: &str = env!("CARGO_BIN_EXE_lamp-bus");
const WAIT: Duration = Duration::from_secs(20);

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(std::env::temp_dir().join(format!(
            "lc{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
    fn bus(&self) -> Bus {
        Bus::new(&self.0).unwrap()
    }
    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = Command::new(BINARY);
        command
            .arg("--root")
            .arg(&self.0)
            .args(arguments)
            .env_remove("LAMP_BUS_ROOT")
            .stdin(Stdio::null());
        command
    }
    fn socket(&self) -> PathBuf {
        let started = Instant::now();
        loop {
            if let Ok(entries) = fs::read_dir(self.0.join("logs/subs")) {
                for entry in entries.flatten() {
                    if entry.file_name().to_string_lossy().ends_with(".sock") {
                        return entry.path();
                    }
                }
            }
            assert!(started.elapsed() < WAIT, "log follower did not subscribe");
            thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Running(Option<Child>);
impl Running {
    fn start(root: &Root, arguments: &[&str]) -> Self {
        Self(Some(
            root.command(arguments)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        ))
    }
    fn output(mut self) -> Output {
        self.0.take().unwrap().wait_with_output().unwrap()
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

fn log(part: &str, level: LogLevel, attempt: u64, message: &str) -> Log {
    Log {
        part: part.into(),
        level,
        code: "state_changed".into(),
        message: message.into(),
        attempt,
        created_unix_ms: 123,
    }
}

#[test]
fn filters_count_only_matching_logs_and_escape_terminal_text() {
    let root = Root::new();
    let follower = Running::start(
        &root,
        &[
            "logs",
            "--part",
            "body",
            "--level",
            "warn",
            "--count",
            "2",
            "--seconds",
            "10",
        ],
    );
    root.socket();
    let bus = root.bus();
    let mut body = bus
        .publisher_declared(Log::TOPIC, &Log::declaration(), "body")
        .unwrap();
    let mut voice = bus
        .publisher_declared(Log::TOPIC, &Log::declaration(), "voice")
        .unwrap();
    let mut legacy = bus.publisher(Log::TOPIC, Kind::Event, "legacy").unwrap();
    legacy
        .publish_json(1, &log("legacy", LogLevel::Error, 1, "untyped"))
        .unwrap();
    voice
        .publish_json(1, &log("voice", LogLevel::Error, 1, "other part"))
        .unwrap();
    body.publish_json(1, &log("body", LogLevel::Debug, 1, "too quiet"))
        .unwrap();
    body.publish_json(2, &json!({"level":"warn"})).unwrap();
    body.publish_json(
        3,
        &log("body", LogLevel::Warn, 3, "two\nlines\x1b[31m\u{85}"),
    )
    .unwrap();
    body.publish_json(
        4,
        &log("body", LogLevel::Info, 4, "filtered between matches"),
    )
    .unwrap();
    body.publish_json(5, &log("body", LogLevel::Error, 5, "second match"))
        .unwrap();
    let output = follower.output();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let shown = String::from_utf8(output.stdout).unwrap();
    assert_eq!(shown.lines().count(), 2, "{shown}");
    assert!(shown.contains("warn body") && shown.contains("error body"));
    assert!(shown.contains("two\\nlines"));
    assert!(!shown.contains('\x1b') && !shown.contains('\u{85}'));
    assert!(
        !shown.contains("too quiet") && !shown.contains("other part") && !shown.contains("untyped")
    );
    assert!(shown.contains("attempt=3") && shown.contains("attempt=5"));
    assert_eq!(
        shown.matches("bus_gap=0").count(),
        2,
        "filtering is not transport loss"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("passed over 2"));
}

#[test]
fn follower_refuses_wrong_type_encoding_kind_and_payload_before_counting() {
    let root = Root::new();
    let follower = Running::start(&root, &["logs", "--count", "1", "--seconds", "10"]);
    let path = root.socket();
    let socket = UnixDatagram::unbound().unwrap();
    let valid = log("raw", LogLevel::Info, 1, "valid after refusals");
    for (index, (kind, encoding, type_name, payload)) in [
        (
            "latest",
            "json",
            Log::TYPE,
            serde_json::to_vec(&valid).unwrap(),
        ),
        (
            "event",
            "none",
            Log::TYPE,
            serde_json::to_vec(&valid).unwrap(),
        ),
        (
            "event",
            "json",
            "lamp_msgs/Other",
            serde_json::to_vec(&valid).unwrap(),
        ),
        ("event", "json", Log::TYPE, b"{broken".to_vec()),
        (
            "event",
            "json",
            Log::TYPE,
            serde_json::to_vec(&log("other", LogLevel::Info, 1, "wrong identity")).unwrap(),
        ),
        (
            "event",
            "json",
            Log::TYPE,
            serde_json::to_vec(&valid).unwrap(),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut bytes = serde_json::to_vec(&json!({
            "topic":"logs", "kind":kind, "enc":encoding, "type":type_name,
            "source":"raw", "boot":"0123456789abcdef", "seq":index + 1,
            "at_us":0, "at_unix_ms":123
        }))
        .unwrap();
        bytes.push(b'\n');
        bytes.extend(payload);
        socket.send_to(&bytes, &path).unwrap();
    }
    let output = follower.output();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let shown = String::from_utf8(output.stdout).unwrap();
    assert_eq!(shown.lines().count(), 1);
    assert!(shown.contains("valid after refusals") && shown.contains("bus_seq=6"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("passed over 5"));
}

#[test]
fn logging_options_are_validated_before_creating_a_subscriber() {
    let root = Root::new();
    for arguments in [
        &["logs", "--part", "Bad Part"][..],
        &["logs", "--level", "warning"],
        &["logs", "--level", "INFO"],
        &["logs", "--seconds", "-1"],
        &["logs", "--count", "many"],
        &["logs", "unexpected"],
        &["logs", "--name", "other"],
    ] {
        let output = root.command(arguments).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("usage:"));
        assert!(!root.0.exists());
    }
}

#[test]
fn finite_empty_follow_and_wrong_declaration_have_truthful_status() {
    let root = Root::new();
    let empty = root
        .command(&["logs", "--seconds", "0", "--count", "1"])
        .output()
        .unwrap();
    assert_eq!(empty.status.code(), Some(1));
    assert!(empty.stdout.is_empty());
    assert!(root.bus().publishers(Log::TOPIC).unwrap().is_empty());
    assert!(
        root.command(&["logs", "--seconds", "0"])
            .output()
            .unwrap()
            .status
            .success()
    );
    let _other = root
        .bus()
        .publisher_declared(
            Log::TOPIC,
            &Declaration::new("lamp_msgs/Other", 1, Kind::Event),
            "other",
        )
        .unwrap();
    let refused = root.command(&["logs", "--seconds", "0"]).output().unwrap();
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("lamp_msgs/Other"));
}

#[test]
fn a_closed_stdout_ends_following_without_affecting_the_publisher() {
    let root = Root::new();
    let mut follower = Running::start(&root, &["logs", "--seconds", "10"]);
    drop(follower.0.as_mut().unwrap().stdout.take());
    root.socket();
    let mut publisher = root
        .bus()
        .publisher_declared(Log::TOPIC, &Log::declaration(), "body")
        .unwrap();
    assert_eq!(
        publisher
            .publish_json(0, &log("body", LogLevel::Info, 1, "pipe closed"))
            .unwrap()
            .delivered,
        1
    );
    assert!(follower.output().status.success());
    assert!(
        publisher
            .publish_json(1, &log("body", LogLevel::Info, 2, "still running"))
            .is_ok()
    );
}

/// #699: the shared log kept on disk, at its level, one JSON line each with
/// its bus identity, in a new file under the folder given.
#[test]
fn kept_logs_land_in_a_file_at_their_level_with_their_bus_identity() {
    let root = Root::new();
    let folder = std::env::temp_dir().join(format!("lk{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).unwrap();
    let keeper = Running::start(
        &root,
        &[
            "keep-logs",
            folder.to_str().unwrap(),
            "--level",
            "warn",
            "--seconds",
            // Counted from when it listens, and the logs go as soon as its
            // socket is seen; it syncs once more as it ends (#1128).
            "0.6",
        ],
    );
    root.socket();
    let mut body = root
        .bus()
        .publisher_declared(Log::TOPIC, &Log::declaration(), "body")
        .unwrap();
    body.publish_json(1, &log("body", LogLevel::Info, 1, "too quiet"))
        .unwrap();
    body.publish_json(2, &log("body", LogLevel::Warn, 2, "a hand on the head"))
        .unwrap();
    body.publish_json(3, &log("body", LogLevel::Error, 3, "two\nlines"))
        .unwrap();
    let output = keeper.output();
    let said = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "{said}");
    assert!(said.contains("kept 2, could not write 0"), "{said}");
    let text = fs::read_to_string(folder.join("logs.00000001.jsonl")).unwrap();
    let lines: Vec<serde_json::Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 2, "{text}");
    assert_eq!(lines[0]["level"], "warn");
    assert_eq!(lines[0]["bus_seq"], 2);
    assert_eq!(lines[1]["message"], "two\nlines");
    assert_eq!(lines[1]["bus_gap"], 0);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn keep_logs_options_are_checked_before_anything_is_kept() {
    let root = Root::new();
    for arguments in [
        &["keep-logs"][..],
        &["keep-logs", "/tmp", "/var"],
        &["keep-logs", "/tmp", "--file-kb", "63"],
        &["keep-logs", "/tmp", "--file-kb", "65537"],
        &["keep-logs", "/tmp", "--files", "1"],
        &["keep-logs", "/tmp", "--files", "101"],
        &["keep-logs", "/tmp", "--level", "warning"],
        &["keep-logs", "/tmp", "--part", "body"],
    ] {
        let output = root.command(arguments).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("usage:"));
    }
    let missing = root
        .command(&["keep-logs", "/nonexistent/lamp-logs", "--seconds", "0"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1));
}
