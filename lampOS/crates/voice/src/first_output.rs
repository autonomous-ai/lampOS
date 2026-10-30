//! The voice's record of whom it answered: for each request, the first
//! output the busy-room rule judged ([`crate::attention`]), what the rule saw
//! and what it decided, one JSON line each, for fitting and checking the
//! Policy's judges (#700; the contract proposed in
//! `docs/verification/local-judge-fit-20261014/voice-first-output.md`).
//!
//! Off unless set: `LAMP_FIRST_OUTPUT=/absolute/private/directory` in the
//! voice's unit, passed to the provider worker as `--first-output-log DIR`.
//! Each worker run writes its own file there. Writing never holds the
//! voice: a line goes to a bounded queue and a thread of its own writes it;
//! a full queue, a file at its limit or a failed write is counted, and the
//! counts are the file's last line. What was heard is in these lines, so the
//! directory is private to the voice and nothing here is published.
//!
//! Each function call the model makes is a line too (`voice_tool_call`,
//! #799): when it came, when our answer was sent, and whether whoever was
//! asked answered. With the request's first output it says where a turn
//! with a function call spends its time; the function's arguments and our
//! answer are not in it.
//!
//! Not recorded yet, from the contract: the request's outcome (whether any
//! sound was played), its start, and the identities of the transcript, name
//! and eyes messages the rule used.
use serde::Serialize;
use serde_json::json;
use std::{
    fs::OpenOptions,
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{Receiver, SyncSender, TrySendError, sync_channel},
    },
    thread::JoinHandle,
};

/// Where the coordinator reads the directory, if any.
pub const ENVIRONMENT: &str = "LAMP_FIRST_OUTPUT";
/// The provider worker's flag for it.
pub const FLAG: &str = "--first-output-log";
/// Lines waiting to be written at most; more are counted as dropped.
const QUEUE: usize = 64;
/// One run's file at most; lines past it are counted as dropped.
const MOST_BYTES: u64 = 8 * 1024 * 1024;

/// What the rule saw, in the seven fields the Policy's fitting rows take as
/// their `baseline` (`crates/policy`, `fit::Row`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Baseline {
    pub kind: &'static str,
    pub room: &'static str,
    pub own_turn: bool,
    pub going_on: bool,
    pub named_sure: bool,
    pub text: String,
    pub facing: Option<bool>,
}

/// One decision of the rule, with what is known around it. Times are this
/// process's monotonic clock, in microseconds.
#[derive(Clone, Debug)]
pub struct Decision {
    pub session: u64,
    pub request: u64,
    /// What came first: `audio`, `output_transcript` or `tool_call`.
    pub trigger: &'static str,
    /// The rule's busy form was in force (`--attention-busy`).
    pub strict: bool,
    pub baseline: Baseline,
    pub play: bool,
    pub reason: &'static str,
    /// When the room, the conversation and the eyes began and finished
    /// being read for it, and when the rule decided.
    pub sampled_start_us: u64,
    pub sampled_end_us: u64,
    pub evaluated_us: u64,
    /// When the request's sound began and, if known, ended.
    pub input_began_us: Option<u64>,
    pub input_ended_us: Option<u64>,
    /// Whether it was said to the lamp from close to it, when that is
    /// judged (`LAMP_ADDRESSED=on`, `crate::addressed`). Not in the line
    /// otherwise, so a line without the switch is as it was.
    pub addressed: Option<lamp_msgs::Addressed>,
    /// Whether the lamp's own last line before the request was a question
    /// (`crate::thread::Thread::lamp_asked`), as the rule read it: a short
    /// sound with a word in it then answers it (#800, 17 October).
    pub lamp_asked: bool,
}

/// One function call and our answer to it. Times are this process's
/// monotonic clock, in microseconds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCallLine {
    pub session: u64,
    pub request: u64,
    pub name: String,
    /// When the call reached the worker.
    pub called_us: u64,
    /// When our answer was handed to the connection, or found it gone.
    pub sent_us: u64,
    /// Whoever was asked answered in time.
    pub answered: bool,
    /// The connection took the answer.
    pub sent: bool,
}

#[derive(Default)]
struct Counts {
    admitted: AtomicU64,
    written: AtomicU64,
    dropped: AtomicU64,
    failed: AtomicU64,
}

pub struct FirstOutputLog {
    lines: Option<SyncSender<String>>,
    writer: Option<JoinHandle<()>>,
    counts: Arc<Counts>,
    path: PathBuf,
    run: String,
    machine: Option<String>,
    boot: Option<String>,
    seq: u64,
    decisions: u64,
    tool_calls: u64,
    began_unix_ms: u64,
}

impl FirstOutputLog {
    /// A new file for this run in `directory`, which must exist, be a
    /// directory and be open to its owner only.
    pub fn open(directory: &Path) -> io::Result<Self> {
        Self::open_hashing(directory, std::env::current_exe().ok())
    }

    /// [`Self::open`], naming the program its first line hashes: only its
    /// path is taken here, and it is read on the writer's thread.
    fn open_hashing(directory: &Path, program: Option<PathBuf>) -> io::Result<Self> {
        let metadata = std::fs::metadata(directory)?;
        if !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the first-output directory must be a directory open to its owner only",
            ));
        }
        let began_unix_ms = unix_ms();
        let run = format!("{began_unix_ms}-{}", std::process::id());
        let path = directory.join(format!("first-output-{run}.jsonl"));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        let counts = Arc::new(Counts::default());
        let (lines, queued) = sync_channel(QUEUE);
        let machine = read_trimmed("/etc/machine-id");
        let boot = read_trimmed("/proc/sys/kernel/random/boot_id");
        let writer = {
            let counts = Arc::clone(&counts);
            let start = Start {
                run: run.clone(),
                machine: machine.clone(),
                boot: boot.clone(),
                began_unix_ms,
                program,
            };
            std::thread::Builder::new()
                .name("first-output".into())
                .spawn(move || write_lines(file, &start, &queued, &counts))?
        };
        Ok(Self {
            lines: Some(lines),
            writer: Some(writer),
            counts,
            path,
            run,
            machine,
            boot,
            seq: 0,
            decisions: 0,
            tool_calls: 0,
            began_unix_ms,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Record one decision. Never waits.
    pub fn decided(&mut self, decision: &Decision) {
        self.seq += 1;
        self.decisions += 1;
        let mut line = json!({
            "schema": 1,
            "event": "voice_first_output",
            "machine": self.machine,
            "os_boot": self.boot,
            "voice_run": self.run,
            "event_seq": self.seq,
            "session": decision.session,
            "request": decision.request,
            "trigger": decision.trigger,
            "attention": {"enabled": true, "strict": decision.strict},
            "baseline": decision.baseline,
            "permission": if decision.play { "play" } else { "hold" },
            "reason": decision.reason,
            "sampled_start_us": decision.sampled_start_us,
            "sampled_end_us": decision.sampled_end_us,
            "evaluated_us": decision.evaluated_us,
            "input_began_us": decision.input_began_us,
            "input_ended_us": decision.input_ended_us,
            "lamp_asked": decision.lamp_asked,
            "at_unix_ms": unix_ms(),
        });
        if let (Some(addressed), Some(fields)) = (&decision.addressed, line.as_object_mut()) {
            fields.insert("addressed".into(), json!(addressed));
        }
        self.send(line.to_string());
    }

    /// Record one function call and our answer to it. Never waits.
    pub fn tool_call(&mut self, call: &ToolCallLine) {
        self.seq += 1;
        self.tool_calls += 1;
        let line = json!({
            "schema": 1,
            "event": "voice_tool_call",
            "machine": self.machine,
            "os_boot": self.boot,
            "voice_run": self.run,
            "event_seq": self.seq,
            "session": call.session,
            "request": call.request,
            "name": call.name,
            "called_us": call.called_us,
            "sent_us": call.sent_us,
            "answered": call.answered,
            "sent": call.sent,
            "at_unix_ms": unix_ms(),
        });
        self.send(line.to_string());
    }

    fn send(&self, line: String) {
        let Some(lines) = &self.lines else { return };
        match lines.try_send(line) {
            Ok(()) => {
                self.counts.admitted.fetch_add(1, Ordering::Relaxed);
            }
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                self.counts.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

impl Drop for FirstOutputLog {
    /// The run's last line: how long it ran and what became of its lines.
    fn drop(&mut self) {
        let Some(lines) = self.lines.take() else {
            return;
        };
        let mut end = json!({
            "schema": 1,
            "event": "capture_end",
            "voice_run": self.run,
            "began_unix_ms": self.began_unix_ms,
            "at_unix_ms": unix_ms(),
            "decisions": self.decisions,
            "admitted": self.counts.admitted.load(Ordering::Relaxed),
            "dropped": self.counts.dropped.load(Ordering::Relaxed),
            "limits": {"queue": QUEUE, "bytes": MOST_BYTES},
        });
        // Only a run with calls says how many: a run without is as before.
        if self.tool_calls > 0 {
            end["tool_calls"] = json!(self.tool_calls);
        }
        // The end goes in whatever the queue holds; a full queue loses it,
        // and a file without one is a run that did not end cleanly.
        let _ = lines.try_send(end.to_string());
        drop(lines);
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

/// What the run's first line says of it, written by the writer's own thread.
struct Start {
    run: String,
    machine: Option<String>,
    boot: Option<String>,
    began_unix_ms: u64,
    /// The program to hash, read here on the writer's thread.
    program: Option<PathBuf>,
}

/// The writer's thread. Its first line names the run and the program that
/// ran it, by its SHA-256: reading and hashing the program takes too long
/// for the provider's own thread (on lamp-4ace on 2026-10-14 doing it there
/// delayed the worker's messages and the voice ended it at every start).
fn write_lines(mut file: std::fs::File, start: &Start, queued: &Receiver<String>, counts: &Counts) {
    let first = json!({
        "schema": 1,
        "event": "capture_start",
        "machine": start.machine,
        "os_boot": start.boot,
        "voice_run": start.run,
        "build_sha256": build_sha256(start.program.as_deref()),
        "began_unix_ms": start.began_unix_ms,
    });
    let mut written_bytes = 0u64;
    for line in std::iter::once(first.to_string()).chain(queued) {
        let length = line.len() as u64 + 1;
        if written_bytes + length > MOST_BYTES {
            counts.dropped.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        match file
            .write_all(line.as_bytes())
            .and_then(|()| file.write_all(b"\n"))
            .and_then(|()| file.flush())
        {
            Ok(()) => {
                written_bytes += length;
                counts.written.fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => {
                counts.failed.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// The provider worker's arguments for the setting in the environment.
pub fn arguments_from_environment() -> Vec<String> {
    arguments(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn arguments(value: Option<&str>) -> Vec<String> {
    match value {
        Some(directory) if Path::new(directory).is_absolute() => {
            vec![FLAG.to_owned(), directory.to_owned()]
        }
        _ => Vec::new(),
    }
}

/// Take a leading `--first-output-log DIR` off the provider worker's flags.
pub fn take(flags: &[String]) -> io::Result<(Option<PathBuf>, &[String])> {
    match flags {
        [flag, directory, rest @ ..] if flag == FLAG => {
            if !Path::new(directory).is_absolute() {
                return Err(io::Error::other("--first-output-log must be absolute"));
            }
            Ok((Some(PathBuf::from(directory)), rest))
        }
        [flag] if flag == FLAG => Err(io::Error::other("--first-output-log needs a directory")),
        _ => Ok((None, flags)),
    }
}

fn read_trimmed(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

/// The running program's SHA-256, which names its build exactly.
fn build_sha256(program: Option<&Path>) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(program?).ok()?;
    Some(format!("{:x}", Sha256::digest(&bytes)))
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decision(request: u64, play: bool) -> Decision {
        Decision {
            session: 7,
            request,
            trigger: "audio",
            strict: true,
            baseline: Baseline {
                kind: "voice_first_output",
                room: "busy",
                own_turn: false,
                going_on: false,
                named_sure: false,
                text: "And tomorrow?".into(),
                facing: None,
            },
            play,
            reason: "a reason",
            sampled_start_us: 10,
            sampled_end_us: 12,
            evaluated_us: 13,
            input_began_us: Some(1),
            input_ended_us: None,
            addressed: None,
            lamp_asked: false,
        }
    }

    fn private_directory(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("first-output-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        directory
    }

    #[test]
    fn each_decision_is_one_line_with_the_seven_fields_and_the_run_ends_with_its_counts() {
        let directory = private_directory("lines");
        let path = {
            let mut log = FirstOutputLog::open(&directory).unwrap();
            log.decided(&decision(3, false));
            log.decided(&decision(4, true));
            log.path().to_owned()
        };
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let lines: Vec<serde_json::Value> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0]["event"], "capture_start");
        assert_eq!(lines[0]["build_sha256"].as_str().map(str::len), Some(64));
        let lines = &lines[1..];
        assert_eq!(lines[0]["event"], "voice_first_output");
        assert_eq!(lines[0]["voice_run"], lines[2]["voice_run"]);
        assert_eq!(lines[0]["permission"], "hold");
        assert_eq!(lines[1]["permission"], "play");
        assert_eq!(lines[1]["event_seq"], 2);
        let baseline = lines[0]["baseline"].as_object().unwrap();
        assert_eq!(baseline.len(), 7);
        assert_eq!(baseline["kind"], "voice_first_output");
        assert_eq!(baseline["facing"], serde_json::Value::Null);
        assert_eq!(baseline["text"], "And tomorrow?");
        assert_eq!(lines[0]["input_ended_us"], serde_json::Value::Null);
        assert!(lines[0].get("addressed").is_none());
        assert_eq!(lines[0]["lamp_asked"], false);
        assert_eq!(lines[2]["event"], "capture_end");
        assert_eq!(lines[2]["decisions"], 2);
        assert_eq!(lines[2]["dropped"], 0);
        assert!(lines[2].get("tool_calls").is_none());
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// #799: a function call is a line of its own, between decisions in the
    /// order they came, and the run's end counts calls apart from decisions.
    #[test]
    fn each_function_call_is_a_line_with_when_it_came_and_when_it_was_answered() {
        let directory = private_directory("calls");
        let path = {
            let mut log = FirstOutputLog::open(&directory).unwrap();
            log.decided(&decision(3, false));
            log.tool_call(&ToolCallLine {
                session: 1,
                request: 3,
                name: "confirm_agent_task".into(),
                called_us: 1_000_000,
                sent_us: 1_046_000,
                answered: true,
                sent: true,
            });
            log.path().to_owned()
        };
        let lines: Vec<serde_json::Value> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 4);
        let call = &lines[2];
        assert_eq!(call["event"], "voice_tool_call");
        assert_eq!(call["event_seq"], 2);
        assert_eq!(call["voice_run"], lines[1]["voice_run"]);
        assert_eq!(
            (call["request"].clone(), call["name"].clone()),
            (
                serde_json::json!(3),
                serde_json::json!("confirm_agent_task")
            )
        );
        assert_eq!(
            (call["called_us"].clone(), call["sent_us"].clone()),
            (serde_json::json!(1_000_000), serde_json::json!(1_046_000))
        );
        assert_eq!(
            (call["answered"].clone(), call["sent"].clone()),
            (true.into(), true.into())
        );
        // No arguments, no answer.
        assert!(call.get("args").is_none() && call.get("told").is_none());
        assert_eq!(lines[3]["event"], "capture_end");
        assert_eq!(
            (
                lines[3]["decisions"].clone(),
                lines[3]["tool_calls"].clone()
            ),
            (1.into(), 1.into())
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn whether_it_was_said_to_the_lamp_goes_beside_the_decision_only_when_judged() {
        let directory = private_directory("addressed");
        let said = lamp_msgs::Addressed {
            session: 7,
            request: 5,
            began_us: 1,
            ended_us: Some(2),
            rise_db: Some(21.0),
            near: Some(true),
            looks: 6,
            facing_height: Some(0.2),
            facing_share: Some(0.5),
            facing_close: Some(true),
            name_score: 0.0,
            addressed: true,
            like_asker: Some(0.91),
        };
        let path = {
            let mut log = FirstOutputLog::open(&directory).unwrap();
            log.decided(&Decision {
                addressed: Some(said),
                lamp_asked: true,
                ..decision(5, true)
            });
            log.path().to_owned()
        };
        let lines: Vec<serde_json::Value> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let line = &lines[1];
        assert_eq!(line["event"], "voice_first_output");
        assert_eq!(
            serde_json::from_value::<lamp_msgs::Addressed>(line["addressed"].clone()).unwrap(),
            said
        );
        assert_eq!(line["lamp_asked"], true);
        // The Policy's seven baseline fields are untouched.
        assert_eq!(line["baseline"].as_object().unwrap().len(), 7);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_directory_others_can_enter_is_refused_and_the_switch_is_an_absolute_directory() {
        let directory = private_directory("open");
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(FirstOutputLog::open(&directory).is_err());
        std::fs::remove_dir_all(directory).unwrap();
        assert_eq!(
            arguments(Some("/var/lib/x")),
            [FLAG.to_owned(), "/var/lib/x".to_owned()]
        );
        assert!(arguments(Some("relative")).is_empty() && arguments(None).is_empty());
        let flags = vec![FLAG.to_owned(), "/d".to_owned(), "--other".to_owned()];
        let (taken, rest) = take(&flags).unwrap();
        assert_eq!(taken, Some(PathBuf::from("/d")));
        assert_eq!(rest, &flags[2..]);
        assert!(take(&[FLAG.to_owned()]).is_err());
        assert!(take(&[FLAG.to_owned(), "d".to_owned()]).is_err());
    }

    #[test]
    fn opening_reads_nothing_large_on_the_callers_thread() {
        // Hashing the program happens on the writer's thread: opening never
        // waits for it. The program here is a FIFO, whose read waits until
        // something is written to it, so `open` returning while the writer
        // is held there shows the read isn't on the caller's thread, with no
        // clock to miss under load (it was a 50 ms bound before).
        use sha2::{Digest, Sha256};
        let directory = private_directory("quick");
        let program = directory.join("program");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&program)
                .status()
                .unwrap()
                .success()
        );
        let (opened, open) = std::sync::mpsc::channel();
        let opening = {
            let (directory, program) = (directory.clone(), program.clone());
            std::thread::spawn(move || {
                let log = FirstOutputLog::open_hashing(&directory, Some(program));
                opened.send(()).unwrap();
                log
            })
        };
        // Generous, only so that a regression fails rather than hangs.
        open.recv_timeout(std::time::Duration::from_secs(30))
            .expect("open waited for the program to be read");
        // The program's bytes come, and the writer goes on. Opening the FIFO
        // to write waits for the writer's read, so a writer that never read
        // the program would hold this too: bounded the same way (Tests on
        // #1334).
        let (written, write) = std::sync::mpsc::channel();
        {
            let program = program.clone();
            std::thread::spawn(move || {
                std::fs::write(&program, b"a build").unwrap();
                written.send(()).unwrap();
            });
        }
        write
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("the writer never read the program");
        let log = opening.join().unwrap().unwrap();
        let path = log.path().to_owned();
        drop(log);
        let first: serde_json::Value = serde_json::from_str(
            std::fs::read_to_string(&path)
                .unwrap()
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            first["build_sha256"],
            format!("{:x}", Sha256::digest(b"a build"))
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_full_queue_is_counted_and_never_waited_on() {
        let directory = private_directory("full");
        let mut log = FirstOutputLog::open(&directory).unwrap();
        for request in 0..10_000 {
            log.decided(&decision(request, true));
        }
        let admitted = log.counts.admitted.load(Ordering::Relaxed);
        let dropped = log.counts.dropped.load(Ordering::Relaxed);
        assert_eq!(admitted + dropped, 10_000);
        drop(log);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
