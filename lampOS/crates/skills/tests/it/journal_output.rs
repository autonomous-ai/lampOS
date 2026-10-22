//! The real Skills process keeps answering when its journal breaks or fills.
//! Like `call_wait`, this uses a say-only tool: the answer is a note, with
//! no voice, body, actions service, device or provider needed.
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    net::Shutdown,
    os::{fd::OwnedFd, unix::net::UnixStream},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

const STARTUP_WAIT: Duration = Duration::from_secs(20);
const REPLY_WAIT: Duration = Duration::from_secs(2);
const UNKNOWN_TOOL: &str = "missing-journal-fixture-tool";

struct Running(Child);

impl Running {
    fn wait(&mut self) -> ExitStatus {
        let until = Instant::now() + STARTUP_WAIT;
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < until, "Skills did not exit");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_lamp-skills"));
    // No inherited optional feature or path may take this fixture outside
    // its own folders and sockets.
    command
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    command
}

/// Connect-to-complete-reply time, including the request write and the
/// service's handling. This is local IPC evidence, not acoustic latency.
fn call(socket: &Path, name: &str) -> io::Result<(Value, Duration)> {
    call_before(socket, name, Instant::now() + REPLY_WAIT * 2)
}

fn remaining(until: Instant) -> io::Result<Duration> {
    until
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
        .map(|left| left.min(REPLY_WAIT))
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "journal test deadline expired"))
}

fn call_before(socket: &Path, name: &str, until: Instant) -> io::Result<(Value, Duration)> {
    let began = Instant::now();
    let mut stream = UnixStream::connect(socket)?;
    stream.set_write_timeout(Some(remaining(until)?))?;
    writeln!(
        stream,
        "{}",
        json!({"op": "call", "name": name, "args": {}})
    )?;
    stream.set_read_timeout(Some(remaining(until)?))?;
    let mut line = String::new();
    let read = BufReader::new((&stream).take(8_192)).read_line(&mut line)?;
    remaining(until)?;
    let took = began.elapsed();
    if read == 0 || !line.ends_with('\n') {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "Skills closed the connection without a complete reply",
        ));
    }
    let reply = serde_json::from_str(&line).map_err(io::Error::other)?;
    Ok((reply, took))
}

fn start(child_journal: OwnedFd, until: Instant) -> (tempfile::TempDir, PathBuf, Running) {
    // Keep the Unix socket paths below macOS's 103-byte limit.
    let dir = tempfile::Builder::new()
        .prefix("sj")
        .tempdir_in("/tmp")
        .unwrap();
    let root = dir.path();
    let skills = root.join("skills");
    let wave = skills.join("wave");
    fs::create_dir_all(&wave).unwrap();
    fs::write(
        wave.join("SKILL.md"),
        "---\nname: wave\ndescription: A journal regression fixture.\n---\n",
    )
    .unwrap();
    fs::write(
        wave.join("skill.toml"),
        "tool = \"wave\"\n[[do]]\nsay = \"Say hello.\"\n[permissions]\nspeak = true\n",
    )
    .unwrap();
    let tools = root.join("tools");
    let mut running = Running(
        command()
            .args([
                "--dir".as_ref(),
                skills.as_os_str(),
                "--bus-root".as_ref(),
                root.join("bus").as_os_str(),
                "--gaze-socket".as_ref(),
                root.join("gaze.sock").as_os_str(),
                "--mood-socket".as_ref(),
                root.join("mood.sock").as_os_str(),
                "--actions-socket".as_ref(),
                root.join("actions.sock").as_os_str(),
                "--say-socket".as_ref(),
                root.join("say.sock").as_os_str(),
                "--tools-dir".as_ref(),
                tools.as_os_str(),
            ])
            .stderr(Stdio::from(child_journal))
            .spawn()
            .unwrap(),
    );
    let until = until.min(Instant::now() + STARTUP_WAIT);
    while !tools.join("tools.json").exists() {
        assert_eq!(
            running.0.try_wait().unwrap(),
            None,
            "Skills exited at startup"
        );
        assert!(Instant::now() < until, "Skills did not publish its tools");
        std::thread::sleep(Duration::from_millis(10));
    }
    let socket = tools.join("call.sock");
    (dir, socket, running)
}

#[test]
fn a_broken_journal_does_not_end_skills_or_lose_its_tool_replies() {
    let (journal, child_journal) = UnixStream::pair().unwrap();
    let (_dir, socket, mut running) = start(child_journal.into(), Instant::now() + STARTUP_WAIT);
    let (ready, ready_took) = call(&socket, "wave").unwrap();
    assert_eq!(ready["ok"], true, "{ready}");
    assert_eq!(ready["result"]["next"], "Say hello.", "{ready}");

    // The valid call replies before publishing its events. A refusal is
    // logged before its reply and publishes no event, so this also proves
    // the main loop finished the first call's diagnostics before the cut.
    let (healthy_refusal, _) = call(&socket, UNKNOWN_TOOL).unwrap();
    let expected_refusal = json!({
        "ok": false,
        "error": "No skill of mine does that just now."
    });
    assert_eq!(healthy_refusal, expected_refusal);

    // Shutdown, rather than just close, also breaks any transient copy of
    // this endpoint inherited by a sibling test's spawning child.
    journal.shutdown(Shutdown::Both).unwrap();
    drop(journal);

    // An unknown tool logs from the main loop BEFORE sending its refusal.
    // Without the relay this panics on stderr and the reply is lost.
    let refused = call(&socket, UNKNOWN_TOOL);
    assert!(
        refused.is_ok(),
        "the broken journal lost the refusal: {refused:?}; child: {:?}",
        running.0.try_wait().unwrap()
    );
    let (refused, refusal_took) = refused.unwrap();
    assert_eq!(refused, expected_refusal);
    let (after, after_took) = call(&socket, "wave").unwrap();
    assert_eq!(after["ok"], true, "{after}");
    assert_eq!(after["result"]["next"], "Say hello.", "{after}");
    assert_eq!(
        running.0.try_wait().unwrap(),
        None,
        "Skills exited after replying"
    );
    eprintln!(
        "Skills journal connect-to-reply: ready_us={} broken_refusal_us={} broken_valid_us={}",
        ready_took.as_micros(),
        refusal_took.as_micros(),
        after_took.as_micros(),
    );
}

#[test]
fn a_full_unread_journal_does_not_hold_startup_or_tool_replies() {
    let began = Instant::now();
    let until = began + Duration::from_secs(30);
    let (journal, mut child_journal) = UnixStream::pair().unwrap();
    // Only preparation is nonblocking: on macOS MSG_DONTWAIT alone can
    // wait on a full socket. Restore blocking mode before any child starts.
    // The peer stays open and unread throughout startup and every call.
    child_journal.set_nonblocking(true).unwrap();
    let fill_until = began + Duration::from_secs(2);
    let buffer = [b'x'; 4096];
    let mut piece = buffer.len();
    let mut filled = 0;
    loop {
        assert!(Instant::now() < fill_until, "journal did not fill in time");
        assert!(filled < 4 * 1024 * 1024, "journal fill exceeded 4 MiB");
        match child_journal.write(&buffer[..piece]) {
            Ok(0) => panic!("journal accepted no bytes without reporting full"),
            Ok(n) => filled += n,
            // Require even a one-byte send to fail before starting Skills.
            Err(error) if error.kind() == io::ErrorKind::WouldBlock && piece == 1 => break,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => piece = 1,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("journal fill failed: {error}"),
        }
    }
    child_journal.set_nonblocking(false).unwrap();
    let mut retained = child_journal.try_clone().unwrap();
    eprintln!("Skills full journal prepared: bytes={filled}");
    let (_dir, socket, mut running) = start(child_journal.into(), until);
    let prepared_to_ready = began.elapsed();
    let (ready, ready_took) = call_before(&socket, "wave", until).unwrap();
    assert_eq!(ready["ok"], true, "{ready}");
    assert_eq!(ready["result"]["next"], "Say hello.", "{ready}");
    let (refused, refusal_took) = call_before(&socket, UNKNOWN_TOOL, until).unwrap();
    assert_eq!(
        refused,
        json!({"ok": false, "error": "No skill of mine does that just now."})
    );
    let (after, after_took) = call_before(&socket, "wave", until).unwrap();
    assert_eq!(after["ok"], true, "{after}");
    assert_eq!(after["result"]["next"], "Say hello.", "{after}");
    assert_eq!(running.0.try_wait().unwrap(), None, "Skills exited");
    assert!(Instant::now() < until, "journal test exceeded 30 seconds");
    eprintln!(
        "Skills full journal: prepared_to_ready_us={} ready_us={} refusal_us={} next_valid_us={}",
        prepared_to_ready.as_micros(),
        ready_took.as_micros(),
        refusal_took.as_micros(),
        after_took.as_micros(),
    );
    // Change the shared mode only after Skills is gone, so the probe cannot
    // make its diagnostics nonblocking and falsely qualify its replies.
    drop(running);
    retained.set_nonblocking(true).unwrap();
    assert_eq!(
        retained.write(b"x").unwrap_err().kind(),
        io::ErrorKind::WouldBlock,
        "the journal did not stay full"
    );
    drop(journal);
}

#[test]
fn a_normal_cli_usage_error_reaches_stderr_before_skills_exits() {
    let dir = tempfile::tempdir().unwrap();
    let stderr = dir.path().join("stderr");
    let mut running = Running(
        command()
            .arg("--unknown-journal-fixture-option")
            .stderr(fs::File::create(&stderr).unwrap())
            .spawn()
            .unwrap(),
    );
    assert_eq!(running.wait().code(), Some(2));
    let said = fs::read_to_string(stderr).unwrap();
    assert!(said.starts_with("usage: lamp-skills "), "{said:?}");
    assert!(said.contains("lamp-skills test SKILL_DIR...\n"), "{said:?}");
    assert!(
        said.ends_with("lamp-skills asked [--dir DIR] [--state-dir DIR] [--key-file PATH]\n"),
        "{said:?}"
    );
}
