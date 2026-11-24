//! #8475 (the first builder's P1s on #8550), through the real lamp-skills
//! process and its call socket: with `LAMP_TASK_NO_YES` on, the voice's
//! call of a skill whose run hands a task over runs only on the owner's
//! named ask (`spoken_ask`), and only that run's hand-over to lamp-tasks is
//! marked as the named ask's (`asked_aloud`), so lamp-control names it to
//! lamp-services and its write may go with no yes. A call the voice didn't
//! mark is refused with nothing handed over. A call that isn't the voice's
//! (the lamp's own Hermes or OpenClaw through `lamp-tasks skill call`) runs
//! on its declared grants, its hand-over unmarked, so every send it makes
//! keeps its yes. The skill is a Store skill whose tool is not its name
//! (ee17's `pear-probe`, tool `fruit_check`).
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

const STARTUP_WAIT: Duration = Duration::from_secs(20);
const REPLY_WAIT: Duration = Duration::from_secs(5);

struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// One call to lamp-skills' call socket, as the voice sends it; the answer.
fn call(socket: &Path, line: &Value) -> Value {
    let mut stream = UnixStream::connect(socket).unwrap();
    stream.set_read_timeout(Some(REPLY_WAIT)).unwrap();
    stream.set_write_timeout(Some(REPLY_WAIT)).unwrap();
    writeln!(stream, "{line}").unwrap();
    let mut answer = String::new();
    BufReader::new((&stream).take(8_192))
        .read_line(&mut answer)
        .unwrap();
    serde_json::from_str(&answer).unwrap()
}

/// A Store skill `pear-probe` whose tool is `fruit_check` and whose run
/// hands a task over, in `skills`.
fn store_task_skill(skills: &Path) {
    let folder = skills.join("pear-probe");
    fs::create_dir_all(folder.join("scripts")).unwrap();
    fs::write(
        folder.join("SKILL.md"),
        "---\nname: pear-probe\ndescription: Checks the fruit bowl.\n---\n",
    )
    .unwrap();
    fs::write(
        folder.join("skill.toml"),
        "version = \"1.0.0\"\ngrants = [\"light\"]\ntool = \"fruit_check\"\n\
         [[do]]\ntask = { task = \"Check the fruit bowl\", run = \"scripts/main.py\" }\n",
    )
    .unwrap();
    fs::write(folder.join("scripts/main.py"), "print('pear')\n").unwrap();
    let installed = lamp_skills::package::Installed {
        sha256: "cd".repeat(32),
        manifest: lamp_skills::package::Manifest {
            format: 1,
            name: "pear-probe".into(),
            version: "1.0.0".into(),
            files: Vec::new(),
        },
        from_store: true,
    };
    fs::write(
        folder.join(lamp_skills::package::INSTALLED_FROM),
        serde_json::to_vec(&installed).unwrap(),
    )
    .unwrap();
}

/// lamp-skills started with a skill's tasks and `LAMP_TASK_NO_YES` on, its
/// calls the voice's (`voice`: `LAMP_SKILLS_ANY_ASKER`, the tests' stand-in
/// for its cgroup) or another program's, and a stand-in lamp-tasks that
/// answers each hand-over `{"ok":true,"id":7}` and passes its line on.
fn start(voice: bool) -> (tempfile::TempDir, PathBuf, Running, mpsc::Receiver<Value>) {
    // Keep the Unix socket paths below macOS's 103-byte limit.
    let dir = tempfile::Builder::new()
        .prefix("tny")
        .tempdir_in("/tmp")
        .unwrap();
    let root = dir.path();
    let skills = root.join("skills");
    store_task_skill(&skills);
    let state = root.join("state");
    fs::create_dir_all(&state).unwrap();
    let tasks = root.join("tasks.sock");
    let listener = UnixListener::bind(&tasks).unwrap();
    let (handed, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut line = String::new();
            if BufReader::new(&stream).read_line(&mut line).is_err() {
                continue;
            }
            let _ = writeln!(&stream, "{}", json!({"ok": true, "id": 7}));
            if let Ok(line) = serde_json::from_str::<Value>(&line) {
                let _ = handed.send(line);
            }
        }
    });
    let tools = root.join("tools");
    let mut command = Command::new(env!("CARGO_BIN_EXE_lamp-skills"));
    command
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // The tests' stand-in for lamp-voice's cgroup: their calls are the
        // voice's; without it, this test's process is another program.
        .env(
            lamp_skills::package::ANY_ASKER,
            if voice { "on" } else { "" },
        )
        .env(lamp_skills::task::ENVIRONMENT, "on")
        .env(lamp_skills::TASK_NO_YES_ENVIRONMENT, "on")
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
            "--state-dir".as_ref(),
            state.as_os_str(),
            "--tasks-socket".as_ref(),
            tasks.as_os_str(),
        ]);
    let mut running = Running(command.spawn().unwrap());
    let until = Instant::now() + STARTUP_WAIT;
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
    (dir, socket, running, lines)
}

/// The voice's call of the skill's tool with no `spoken_ask` (a half
/// match, or a model's call of a skill nobody named) is refused with
/// [`lamp_skills::NOT_NAMED`] and nothing is handed over; the same call
/// with `spoken_ask` true is handed over on its declared grants, its line
/// marked `asked_aloud`, so lamp-control names that run, and only that
/// run, to lamp-services. Fails on the old `tool_call` with the gate at
/// `lamp_skills::task_call_refused` deleted (the unnamed call is then
/// handed over), and on the old `hand`, whose line carried no mark.
#[test]
fn only_the_voices_named_call_runs_a_task_and_is_handed_over_marked() {
    let (_dir, socket, _running, lines) = start(true);
    let unnamed = call(
        &socket,
        &json!({"op": "call", "name": "fruit_check", "args": {}}),
    );
    assert_eq!(unnamed["ok"], false, "{unnamed}");
    assert_eq!(unnamed["error"], lamp_skills::NOT_NAMED, "{unnamed}");
    let marked_false = call(
        &socket,
        &json!({"op": "call", "name": "fruit_check", "args": {}, "spoken_ask": false}),
    );
    assert_eq!(
        marked_false["error"],
        lamp_skills::NOT_NAMED,
        "{marked_false}"
    );
    assert!(
        lines.recv_timeout(Duration::from_millis(500)).is_err(),
        "an unnamed call handed nothing over"
    );
    // A `spoken_ask` in the model's own args is never the voice's word.
    let in_args = call(
        &socket,
        &json!({"op": "call", "name": "fruit_check", "args": {"spoken_ask": true}}),
    );
    assert_eq!(in_args["ok"], false, "{in_args}");
    assert!(lines.recv_timeout(Duration::from_millis(500)).is_err());
    let named = call(
        &socket,
        &json!({"op": "call", "name": "fruit_check", "args": {}, "spoken_ask": true}),
    );
    assert_eq!(named["ok"], true, "{named}");
    let line = lines
        .recv_timeout(Duration::from_secs(10))
        .expect("the named call's run handed over");
    assert_eq!(line["skill"], "pear-probe", "{line}");
    assert_eq!(line["grants"], json!(["light"]), "{line}");
    assert_eq!(line[lamp_skills::task::ASKED_ALOUD], true, "{line}");
    assert!(
        lines.recv_timeout(Duration::from_millis(500)).is_err(),
        "one run handed over"
    );
}

/// A fresh privacy lease that permits capture, renewed every 10 ms as the
/// voice does, while it is kept: another program's call is served only on
/// one (#6128, as `requested_audio.rs`' own).
struct OpenLease {
    renewing: std::sync::Arc<std::sync::atomic::AtomicBool>,
    renewer: Option<std::thread::JoinHandle<()>>,
}

impl OpenLease {
    fn keep(bus_root: &Path) -> Self {
        let renewing = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let bus = lamp_bus::Bus::new(bus_root).unwrap();
        let flag = std::sync::Arc::clone(&renewing);
        let renewer = std::thread::spawn(move || {
            let mut lease = bus
                .publisher_declared(
                    lamp_msgs::MicrophonePrivacy::TOPIC,
                    &lamp_msgs::MicrophonePrivacy::declaration(),
                    "voice",
                )
                .unwrap();
            let clock = lamp_bus::kernel_boot_id().unwrap_or("host").to_owned();
            for sequence in 1u64.. {
                if !flag.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let now = lamp_ipc::monotonic_us();
                let open = lamp_msgs::MicrophonePrivacy {
                    stamp: lamp_msgs::ClockStamp {
                        clock_id: clock.clone(),
                        at_us: now,
                        sequence,
                    },
                    session_id: "0123456789abcdef".into(),
                    controller_boot: "0123456789abcdef0123456789abcdef".into(),
                    microphone_generation: 1,
                    observed_us: now,
                    expires_at_us: now + lamp_msgs::MICROPHONE_PRIVACY_FRESH_US,
                    permitted: true,
                    physical_muted: Some(false),
                    no_slide: false,
                };
                lease.publish_json(now, &open).unwrap();
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        std::thread::sleep(Duration::from_millis(30));
        Self {
            renewing,
            renewer: Some(renewer),
        }
    }
}

impl Drop for OpenLease {
    fn drop(&mut self) {
        self.renewing
            .store(false, std::sync::atomic::Ordering::Relaxed);
        if let Some(renewer) = self.renewer.take() {
            let _ = renewer.join();
        }
    }
}

/// The first builder's P1 1 on #8550: with `LAMP_TASK_NO_YES` on, a call
/// that isn't the voice's (the lamp's own Hermes or OpenClaw through
/// `lamp-tasks skill call`) runs a skill's task on its declared grants, as
/// #8475 says for a run nobody named just now, and its hand-over is never
/// marked as a named ask, even when its line says `spoken_ask`: only the
/// voice's word counts, so lamp-services holds every send it makes for its
/// yes (`serve::tests::only_the_asked_runs_write_is_sent_with_no_yes`,
/// which fails on the old `run_tool_answer` for such a run's write). Here
/// it fails on a lamp-skills that took `spoken_ask` from any peer, or
/// whose gate refused every peer's unnamed call.
#[test]
fn another_programs_call_runs_a_task_unmarked() {
    let (dir, socket, _running, lines) = start(false);
    let open = OpenLease::keep(&dir.path().join("bus"));
    let line = json!({"op": "call", "name": "fruit_check", "args": {}, "spoken_ask": true});
    // A renewal can come late on a loaded host (the lease lives 50 ms):
    // a reply that says the reading is not known is asked again.
    let until = Instant::now() + Duration::from_secs(2);
    let answered = loop {
        let answered = call(&socket, &line);
        let late = answered["error"]
            .as_str()
            .is_some_and(|error| error.contains("the privacy reading is not known"));
        if !late || Instant::now() >= until {
            break answered;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(answered["ok"], true, "{answered}");
    let handed = lines
        .recv_timeout(Duration::from_secs(10))
        .expect("another program's call handed over on its declared grants");
    assert_eq!(handed["skill"], "pear-probe", "{handed}");
    assert_eq!(handed["grants"], json!(["light"]), "{handed}");
    assert!(
        handed.get(lamp_skills::task::ASKED_ALOUD).is_none(),
        "{handed}"
    );
    drop(open);
}
