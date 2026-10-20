//! The built `lamp-memory` itself: what it refuses to start with, and what
//! a process that is killed leaves for the next one.
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const BINARY: &str = env!("CARGO_BIN_EXE_lamp-memory");
const USAGE: &str = "usage: lamp-memory --dir ABSOLUTE_DIR --socket ABSOLUTE_SOCKET_PATH [--keep-days N] [--utc-offset-minutes M | --zone system|NAME] [--memory-days] [--bus-root ABSOLUTE_DIR]";
const LOG_WAIT: Duration = Duration::from_secs(5);

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("lm-{}-b-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }

    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(scratch: &Scratch, args: &[&str]) -> Output {
    Command::new(BINARY)
        .args(args)
        .current_dir(&scratch.0)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

/// A running `lamp-memory`, killed when the test lets go of it.
struct Process {
    child: Child,
    log: PathBuf,
}

/// Only complete records count. A repeated terminal record must be the
/// requested occurrence, after all the requests whose logs will be checked.
fn log_reached(bytes: &[u8], records: usize, terminal: &str, occurrence: usize) -> bool {
    let mut complete = 0;
    let mut matched = 0;
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        if !line.ends_with(b"\n") {
            continue;
        }
        complete += 1;
        if line.starts_with(terminal.as_bytes()) {
            matched += 1;
            if complete >= records && matched >= occurrence {
                return true;
            }
        }
    }
    false
}

impl Process {
    /// Start it with its log in a file: a pipe nobody reads would fill.
    fn start(scratch: &Scratch, dir: &str, socket: &str, log: &str) -> Self {
        Self::start_with(scratch, dir, socket, log, &[])
    }

    fn start_with(scratch: &Scratch, dir: &str, socket: &str, log: &str, extra: &[&str]) -> Self {
        let log_path = scratch.0.join(log);
        let log = fs::File::create(&log_path).unwrap();
        let child = Command::new(BINARY)
            .args([
                "--dir",
                &scratch.path(dir),
                "--socket",
                &scratch.path(socket),
            ])
            .args(["--utc-offset-minutes", "420"])
            .args(extra)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .unwrap();
        Self {
            child,
            log: log_path,
        }
    }

    /// A reply follows the request log's submission to the relay, not its
    /// delivery. Observe the final complete record before killing the relay
    /// with the process. The caller still reads the whole file after the kill.
    fn kill(mut self, records: usize, terminal: &str, occurrence: usize) {
        let started = Instant::now();
        loop {
            let bytes = fs::read(&self.log).unwrap_or_else(|error| {
                panic!(
                    "cannot read {} before killing lamp-memory: {error}",
                    self.log.display()
                )
            });
            let status = self.child.try_wait().unwrap_or_else(|error| {
                panic!(
                    "cannot check lamp-memory before killing it ({}): {error}",
                    self.log.display()
                )
            });
            assert!(
                status.is_none(),
                "lamp-memory exited before SIGKILL: {status:?}; {}: {}",
                self.log.display(),
                String::from_utf8_lossy(&bytes)
            );
            let left = LOG_WAIT.saturating_sub(started.elapsed());
            assert!(
                !left.is_zero(),
                "{} did not reach {records} complete records and occurrence {occurrence} of \
                 {terminal:?} before SIGKILL in {LOG_WAIT:?}; {} complete records: {}",
                self.log.display(),
                bytes.iter().filter(|byte| **byte == b'\n').count(),
                String::from_utf8_lossy(&bytes)
            );
            if log_reached(&bytes, records, terminal, occurrence) {
                break;
            }
            thread::sleep(left.min(Duration::from_millis(10)));
        }
        self.child.kill().unwrap();
        self.child.wait().unwrap();
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// One request on a connection of its own, once somebody answers on the
/// socket. Waiting here is only for the process to have started.
fn ask(socket: &Path, request: Value) -> Value {
    let mut tries = 0;
    let stream = loop {
        match UnixStream::connect(socket) {
            Ok(stream) => break stream,
            Err(error) if tries == 1000 => panic!("nobody answers on the socket: {error}"),
            Err(_) => {
                tries += 1;
                thread::sleep(Duration::from_millis(10));
            }
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let mut reader = BufReader::new(stream);
    reader
        .get_mut()
        .write_all(format!("{request}\n").as_bytes())
        .unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o7777
}

#[test]
fn a_relative_path_or_an_unknown_option_gets_the_usage_line_and_exit_code_two() {
    let scratch = Scratch::new("usage");
    let (dir, socket) = (scratch.path("store"), scratch.path("m.sock"));
    for args in [
        &["--dir", "store", "--socket", &socket][..],
        &["--dir", &dir, "--socket", "m.sock"],
        &["--dir", &dir, "--socket", &socket, "--verbose"],
        &["--dir", &dir, "--socket", &socket, "--keep-days", "0"],
        &[
            "--dir",
            &dir,
            "--socket",
            &socket,
            "--utc-offset-minutes",
            "east",
        ],
        &["--dir", &dir],
        &[],
    ] {
        let output = run(&scratch, args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let said = String::from_utf8(output.stderr).unwrap();
        assert!(said.starts_with("lamp-memory: "), "{said}");
        assert!(said.ends_with(&format!("{USAGE}\n")), "{said}");
        assert!(output.stdout.is_empty());
    }
    // Nothing was made, by the paths given or by the names in them.
    assert_eq!(scratch.names(), [""; 0]);

    let help = run(&scratch, &["--help"]);
    assert_eq!(help.status.code(), Some(0));
    assert!(String::from_utf8(help.stdout).unwrap().starts_with(USAGE));
    assert!(help.stderr.is_empty());
}

#[test]
fn a_directory_others_can_enter_is_refused_with_the_reason_and_exit_code_two() {
    let scratch = Scratch::new("open");
    let (dir, socket) = (scratch.path("store"), scratch.path("m.sock"));
    fs::create_dir(&dir).unwrap();
    for open in [0o750, 0o701, 0o777] {
        fs::set_permissions(&dir, fs::Permissions::from_mode(open)).unwrap();
        let output = run(&scratch, &["--dir", &dir, "--socket", &socket]);
        assert_eq!(output.status.code(), Some(2), "{open:o}");
        let said = String::from_utf8(output.stderr).unwrap();
        assert!(
            said.contains("can be entered by its group or by everyone"),
            "{said}"
        );
        assert!(said.contains(&format!("(mode {open:o})")), "{said}");
        assert_eq!(mode(Path::new(&dir)), open);
    }
    // Something that is not a directory at all.
    fs::write(scratch.0.join("file"), "").unwrap();
    let output = run(
        &scratch,
        &["--dir", &scratch.path("file"), "--socket", &socket],
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(scratch.names(), ["file", "store"]);
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
}

#[test]
fn a_killed_process_leaves_a_store_the_next_one_loads_and_a_socket_it_replaces() {
    let scratch = Scratch::new("killed");
    let socket = scratch.0.join("m.sock");
    let at = 1_791_892_800_000u64;
    let first = Process::start(&scratch, "made/store", "m.sock", "log1");
    let added = ask(
        &socket,
        json!({"op":"append","at_unix_ms":at,"heard":"where is the orchid","said":"On the sill."}),
    );
    assert_eq!(added, json!({"ok":true,"id":1}));
    let kept = ask(
        &socket,
        json!({"op":"remember","at_unix_ms":at,"text":"the orchid wants no sun"}),
    );
    assert_eq!(kept, json!({"ok":true,"id":2}));
    // What it made is its owner's alone.
    assert_eq!(mode(&scratch.0.join("made/store")), 0o700);
    assert_eq!(mode(&scratch.0.join("made/store/everyone")), 0o700);
    assert_eq!(
        mode(&scratch.0.join("made/store/everyone/kept.jsonl")),
        0o600
    );
    assert_eq!(mode(&socket), 0o600);
    assert_eq!(
        fs::read_to_string(scratch.0.join("ready")).unwrap(),
        "ready\n"
    );

    // While it runs, a second one is turned away from its socket, and one
    // given another socket is turned away from its directory. Neither
    // leaves anything behind or disturbs the first.
    let dir = scratch.path("made/store");
    let same = run(
        &scratch,
        &["--dir", &dir, "--socket", &scratch.path("m.sock")],
    );
    assert_eq!(same.status.code(), Some(1));
    assert!(
        String::from_utf8(same.stderr)
            .unwrap()
            .contains("another process is answering")
    );
    fs::create_dir(scratch.0.join("other")).unwrap();
    let other = run(
        &scratch,
        &["--dir", &dir, "--socket", &scratch.path("other/m.sock")],
    );
    assert_eq!(other.status.code(), Some(1));
    assert!(
        String::from_utf8(other.stderr)
            .unwrap()
            .contains("is open in another lamp-memory")
    );
    assert_eq!(fs::read_dir(scratch.0.join("other")).unwrap().count(), 0);
    assert_eq!(ask(&socket, json!({"op":"status"}))["people"][0]["kept"], 1);

    // Killed, with no chance to tidy: the socket and the word stay, and
    // nobody answers.
    first.kill(4, "lamp-memory: status person=* ok ", 1);
    assert_eq!(
        scratch.names(),
        ["log1", "m.sock", "made", "other", "ready"]
    );
    assert!(UnixStream::connect(&socket).is_err());
    // A power cut in the middle of a line, too.
    let journal = fs::read_dir(scratch.0.join("made/store/everyone"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.to_str().unwrap().contains("journal-"))
        .unwrap();
    let mut cut = fs::OpenOptions::new().append(true).open(&journal).unwrap();
    cut.write_all(b"{\"id\":3,\"at_unix_ms\":17918928").unwrap();
    drop(cut);

    let second = Process::start(&scratch, "made/store", "m.sock", "log2");
    let found = ask(&socket, json!({"op":"recall","query":"orchid"}));
    assert_eq!(
        found,
        json!({"ok":true,"entries":[
            {"kind":"exchange","id":1,"at_unix_ms":at,"heard":"where is the orchid","said":"On the sill."},
            {"kind":"kept","id":2,"at_unix_ms":at,"text":"the orchid wants no sun"}
        ]})
    );
    let next = ask(
        &socket,
        json!({"op":"append","at_unix_ms":at,"heard":"and the fern","said":"In the hall."}),
    );
    assert_eq!(next, json!({"ok":true,"id":3}));
    let status = ask(&socket, json!({"op":"status"}));
    assert_eq!(status["people"][0]["exchanges"], 2);
    assert_eq!(status["people"][0]["skipped_lines"], 1);
    assert_eq!(
        fs::read_to_string(scratch.0.join("ready")).unwrap(),
        "ready\n"
    );
    second.kill(4, "lamp-memory: status person=* ok ", 1);

    // Each said what it found as it started, that its health reports are
    // off (no `--bus-root`), and one line for each request, with none of the
    // text in any of them.
    let (log1, log2) = (
        fs::read_to_string(scratch.0.join("log1")).unwrap(),
        fs::read_to_string(scratch.0.join("log2")).unwrap(),
    );
    assert!(log1.contains(" people=0 skipped_lines=0 "), "{log1}");
    assert!(
        log1.contains("lamp-memory: append person=everyone ok id=1\n"),
        "{log1}"
    );
    assert!(
        log2.contains(" utc_offset_minutes=420 people=1 skipped_lines=1 "),
        "{log2}"
    );
    assert!(
        log2.contains("lamp-memory: recall person=everyone ok entries=2 bytes=54\n"),
        "{log2}"
    );
    assert_eq!((log1.lines().count(), log2.lines().count()), (5, 5));
    for log in [log1, log2] {
        assert!(!log.contains("orchid") && !log.contains("fern") && !log.contains("sill"));
    }
}

#[test]
fn opted_in_memory_migrates_and_forgets_linked_facts_across_sigkill_restarts() {
    let scratch = Scratch::new("days-killed");
    let socket = scratch.0.join("m.sock");
    let database = scratch.0.join("store").join(lamp_memory::days::DATABASE);
    let now = || {
        u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap()
    };
    let ids = |answer: Value| {
        assert_eq!(answer["ok"], true, "{answer}");
        let mut ids: Vec<u64> = answer["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["id"].as_u64().unwrap())
            .collect();
        ids.sort_unstable();
        ids
    };

    // Start in the unchanged default mode so the opt-in process really imports
    // an existing store. A current wall-clock stamp cannot age out as this test
    // is rerun in a later month.
    let legacy = Process::start(&scratch, "store", "m.sock", "legacy.log");
    let source = ask(&socket, json!({"op":"append","at_unix_ms":now(),"heard":"My heliotrope ritual uses saffron","said":"Saffron ritual noted"}))["id"].as_u64().unwrap();
    let kept = ask(
        &socket,
        json!({"op":"remember","at_unix_ms":now(),"text":"An orchid lives on the sill"}),
    )["id"]
        .as_u64()
        .unwrap();
    assert!(scratch.0.join("store/everyone/kept.jsonl").exists());
    assert!(!database.exists());
    legacy.kill(3, "lamp-memory: remember person=everyone ok ", 1);
    assert!(UnixStream::connect(&socket).is_err());

    let first = Process::start_with(&scratch, "store", "m.sock", "days1.log", &["--memory-days"]);
    assert_eq!(
        ids(ask(&socket, json!({"op":"recall","query":"heliotrope"}))),
        [source]
    );
    assert!(database.exists());
    assert!(!scratch.0.join("store/everyone").exists());
    assert_eq!(mode(&scratch.0.join("store")), 0o700);
    assert_eq!(mode(&database), 0o600);
    assert_eq!(mode(&socket), 0o600);
    assert_eq!(mode(&scratch.0.join("ready")), 0o600);
    assert_eq!(
        fs::read_to_string(scratch.0.join("ready")).unwrap(),
        "ready\n"
    );
    let prepared = ask(
        &socket,
        json!({"op":"conversation_end","conversation":"first","source_ids":[source]}),
    );
    assert_eq!(prepared["ok"], true, "{prepared}");
    let job = prepared["input"]["job"].as_str().unwrap();
    // The only model in this test is this fixed proposal, submitted through the
    // real protocol. The running memory process never calls a model itself.
    let proposed = ask(
        &socket,
        json!({"op":"tidy_apply","job":job,"observations":[{"text":"Heliotrope ritual uses saffron","category":"preference","source_ids":[source],"supersedes":null}]}),
    );
    assert_eq!(proposed["ok"], true, "{proposed}");
    let fact = proposed["ids"][0].as_u64().unwrap();
    let linked = ask(&socket, json!({"op":"append_linked","at_unix_ms":now(),"heard":"What is my heliotrope ritual?","said":"It uses saffron","source_ids":[fact]}))["id"].as_u64().unwrap();
    assert!(source < kept && kept < fact && fact < linked);
    let frozen = ask(
        &socket,
        json!({"op":"session_open","session":"before-forgetting"}),
    );
    assert_eq!(frozen["snapshot"]["invalidated"], false);
    let delayed = ask(
        &socket,
        json!({"op":"conversation_end","conversation":"later","source_ids":[linked]}),
    );
    assert_eq!(delayed["ok"], true, "{delayed}");
    let delayed_job = delayed["input"]["job"].as_str().unwrap().to_owned();
    assert_eq!(
        ids(ask(&socket, json!({"op":"search","query":"heliotrope"}))),
        [source, fact, linked]
    );
    first.kill(8, "lamp-memory: search person=everyone ok ", 1);
    assert!(UnixStream::connect(&socket).is_err());

    // Every acknowledged record and the prepared context survive SIGKILL.
    let second = Process::start_with(&scratch, "store", "m.sock", "days2.log", &["--memory-days"]);
    assert_eq!(
        ids(ask(&socket, json!({"op":"search","query":"heliotrope"}))),
        [source, fact, linked]
    );
    assert_eq!(
        ask(
            &socket,
            json!({"op":"session_open","session":"before-forgetting"})
        ),
        frozen
    );
    let status = ask(&socket, json!({"op":"status","person":"everyone"}));
    assert_eq!(status["people"][0]["exchanges"], 2);
    assert_eq!(status["people"][0]["kept"], 1);
    assert_eq!(
        ask(&socket, json!({"op":"forget","id":fact})),
        json!({"ok":true,"removed":3})
    );
    assert!(ids(ask(&socket, json!({"op":"search","query":"heliotrope"}))).is_empty());
    let cleared = ask(
        &socket,
        json!({"op":"session_open","session":"before-forgetting"}),
    );
    assert_eq!(cleared["snapshot"]["invalidated"], true);
    assert_eq!(cleared["snapshot"]["note"], "");
    second.kill(7, "lamp-memory: session_open person=everyone ok ", 2);
    assert!(UnixStream::connect(&socket).is_err());

    // Deletion, snapshot invalidation and job invalidation also survive a kill.
    let third = Process::start_with(&scratch, "store", "m.sock", "days3.log", &["--memory-days"]);
    assert!(ids(ask(&socket, json!({"op":"search","query":"heliotrope"}))).is_empty());
    assert!(ids(ask(&socket, json!({"op":"recall","query":"heliotrope"}))).is_empty());
    let status = ask(&socket, json!({"op":"status","person":"everyone"}));
    assert_eq!(status["people"][0]["exchanges"], 0);
    assert_eq!(status["people"][0]["kept"], 1);
    assert_eq!(ids(ask(&socket, json!({"op":"kept"}))), [kept]);
    assert_eq!(
        ask(
            &socket,
            json!({"op":"session_open","session":"before-forgetting"})
        ),
        cleared
    );
    let stale = ask(
        &socket,
        json!({"op":"tidy_apply","job":delayed_job,"observations":[{"text":"Heliotrope ritual uses saffron","category":"preference","source_ids":[linked],"supersedes":null}]}),
    );
    assert_eq!(stale["ok"], false);
    assert_eq!(stale["error"], "stale");
    let fresh = ask(&socket, json!({"op":"append","at_unix_ms":now(),"heard":"A pear grows in the orchard","said":"Noted"}))["id"].as_u64().unwrap();
    assert!(fresh > linked);
    assert!(ids(ask(&socket, json!({"op":"search","query":"heliotrope"}))).is_empty());
    third.kill(9, "lamp-memory: search person=everyone ok ", 2);

    // Turning the option off must not silently revive the removed JSONL store
    // or make an empty legacy store beside the authoritative SQLite database.
    let no_flag = run(
        &scratch,
        &[
            "--dir",
            &scratch.path("store"),
            "--socket",
            &scratch.path("disabled.sock"),
        ],
    );
    assert_eq!(no_flag.status.code(), Some(1));
    let refusal = String::from_utf8(no_flag.stderr).unwrap();
    assert!(
        refusal.contains("reopen it with --memory-days"),
        "{refusal}"
    );
    assert!(!scratch.0.join("store/everyone").exists());
    assert_eq!(mode(&database), 0o600);

    for name in ["legacy.log", "days1.log", "days2.log", "days3.log"] {
        let log = fs::read_to_string(scratch.0.join(name)).unwrap();
        assert!(!log.is_empty());
        for private_word in [
            "heliotrope",
            "saffron",
            "ritual",
            "orchid",
            "sill",
            "pear",
            "orchard",
        ] {
            assert!(
                !log.to_lowercase().contains(private_word),
                "private text in {name}"
            );
        }
    }
    assert!(
        fs::read_to_string(scratch.0.join("days1.log"))
            .unwrap()
            .contains("append_linked person=everyone ok")
    );
    assert!(
        fs::read_to_string(scratch.0.join("days2.log"))
            .unwrap()
            .contains("forget person=everyone ok")
    );
    assert!(
        fs::read_to_string(scratch.0.join("days3.log"))
            .unwrap()
            .contains("tidy_apply person=everyone refused error=stale")
    );
}

#[test]
fn log_completion_waits_for_the_newline_after_a_private_suffix() {
    let terminal = "lamp-memory: search person=everyone ok ";
    let record = b"lamp-memory: search person=everyone ok entries=0 private=orchid\n";
    assert!(!log_reached(terminal.as_bytes(), 1, terminal, 1));
    assert!(!log_reached(&record[..record.len() - 1], 1, terminal, 1));
    // Accept the complete record including its bad suffix, so the existing
    // whole-file privacy assertion can inspect it rather than a filtered log.
    assert!(log_reached(record, 1, terminal, 1));
}

#[test]
fn log_completion_waits_for_the_later_repeated_terminal_record() {
    let terminal = "lamp-memory: session_open person=everyone ok ";
    let first =
        b"started\nlamp-memory: session_open person=everyone ok entries=0\nstatus\nforget\n";
    assert!(!log_reached(first, 4, terminal, 2));
    let incomplete = [first.as_slice(), terminal.as_bytes()].concat();
    assert!(!log_reached(&incomplete, 4, terminal, 2));
    let complete = [incomplete.as_slice(), b"entries=0\n"].concat();
    assert!(log_reached(&complete, 5, terminal, 2));
    assert!(!log_reached(&complete, 6, terminal, 2));
    let later_unrelated = [complete.as_slice(), b"unrelated\n"].concat();
    assert!(!log_reached(&later_unrelated, 6, terminal, 2));
}
