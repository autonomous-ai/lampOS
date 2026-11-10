//! The voice's stand-in on the say socket, and lamp-actions' on its call
//! socket (the volume). A line in, `begun`, the host's speech engine, then
//! the outcome `spoken`, as lamp-control reads them (`parts.rs`). One line at
//! a time: a second `say` waits for the first. A computer's `stop` writes
//! `{"cancel":true}` on its say's connection and the speech ends at once,
//! answered `dropped`. A computer's word-for-word `say` is also told on
//! `voice/line` as it begins, as the real voice does, so `lamp.said` reaches
//! a computer holding `speech`.
use lamp_bus::{Bus, Publisher};
use lamp_msgs::{LineBy, LineSource, SaidLine};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::Path,
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

/// The most a line is spoken for when the host has no speech engine.
const SILENT_MAX: Duration = Duration::from_secs(4);

/// Serves the say socket at `path`, forever, telling each line on `bus`'s
/// `voice/line`. The topic is declared before the socket exists, so a socket
/// that is there means a subscriber can already find the topic.
pub fn serve_say(path: &Path, bus: &Bus) -> std::io::Result<()> {
    let lines = Arc::new(Mutex::new(bus.publisher_declared(
        SaidLine::TOPIC,
        &SaidLine::declaration(),
        "voice",
    )?));
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    for stream in listener.incoming().flatten() {
        let lines = Arc::clone(&lines);
        thread::spawn(move || {
            let _ = say(stream, &lines);
        });
    }
    Ok(())
}

/// Tells a computer's word-for-word line on `voice/line`, as the real voice
/// does when it begins to play (`SaidLine`, docs/msgs.md): the words, `by`
/// `computer`, `source` `exact`, and the computer's id as `app`. Nothing else
/// is told: a line without `by` is a written turn for the model (there is
/// none here), a skill's or a cue's has no words to tell, and an empty one is none.
fn tell(lines: &Mutex<Publisher>, asked: &Value) {
    if asked["skill"] != "control" || asked["exact"] != true {
        return;
    }
    let (Some(text), Some(by)) = (asked["text"].as_str(), asked["by"].as_str()) else {
        return;
    };
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    let line = SaidLine {
        text: text.to_owned(),
        by: LineBy::Computer,
        source: LineSource::Exact,
        app: Some(by.to_owned()),
        request: None,
        cut: false,
        scope: None,
    };
    let mut lines = lines
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = lines.publish_json(lamp_ipc::monotonic_us(), &line);
}

/// One line is spoken at a time.
static SPEAKING: Mutex<()> = Mutex::new(());

fn say(mut stream: UnixStream, lines: &Mutex<Publisher>) -> std::io::Result<()> {
    let mut line = String::new();
    BufReader::new(stream.try_clone()?).read_line(&mut line)?;
    let Ok(asked) = serde_json::from_str::<Value>(&line) else {
        return Ok(());
    };
    // The cancel line, whenever it comes, on this connection.
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let (reader, cancelled) = (stream.try_clone()?, Arc::clone(&cancelled));
        thread::spawn(move || {
            for line in BufReader::new(reader).lines().map_while(Result::ok) {
                if serde_json::from_str::<Value>(&line).is_ok_and(|v| v["cancel"] == true) {
                    cancelled.store(true, Ordering::SeqCst);
                }
            }
            // The door closed the connection: nothing waits for the words.
            cancelled.store(true, Ordering::SeqCst);
        });
    }
    // A second line waits its turn (and is let go if it is cancelled waiting).
    let _turn = loop {
        match SPEAKING.try_lock() {
            Ok(guard) => break guard,
            // A line that panicked while speaking left no state to protect.
            Err(std::sync::TryLockError::Poisoned(poisoned)) => break poisoned.into_inner(),
            Err(_) if cancelled.load(Ordering::SeqCst) => {
                writeln!(
                    stream,
                    "{}",
                    json!({"outcome": "dropped", "why": "stopped by its asker"})
                )?;
                return Ok(());
            }
            Err(_) => thread::sleep(Duration::from_millis(20)),
        }
    };
    writeln!(
        stream,
        "{}",
        json!({"event": "begun", "at_us": lamp_ipc::monotonic_us(), "id": 1})
    )?;
    tell(lines, &asked);
    match (asked["text"].as_str(), asked["clip"].as_str()) {
        (Some(text), _) => speak(text, &cancelled),
        // A computer's clip, played by the host: the door gives the path of a clip it checked.
        (None, Some(clip)) => play(clip, &cancelled),
        // The lamp's own hum: a short while of sound.
        (None, None) => wait(Duration::from_millis(600), &cancelled),
    }
    if cancelled.load(Ordering::SeqCst) {
        writeln!(
            stream,
            "{}",
            json!({"outcome": "dropped", "why": "stopped by its asker"})
        )?;
    } else {
        writeln!(stream, "{}", json!({"outcome": "spoken"}))?;
    }
    Ok(())
}

/// Waits `long`, or until `cancelled`.
fn wait(long: Duration, cancelled: &AtomicBool) {
    let end = std::time::Instant::now() + long;
    while std::time::Instant::now() < end && !cancelled.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(20));
    }
}

/// Whether `path` may be handed to a player as its argument: an absolute path,
/// never one that starts with `-` (read as an option).
fn playable(path: &str) -> bool {
    path.starts_with('/')
}

/// Plays the clip at `path` on the host: the first of the host's players that
/// starts, else a silent wait; cut off when `cancelled`.
fn play(path: &str, cancelled: &AtomicBool) {
    // The players read a path that starts with `-` as an option (`ffplay -f ...`):
    // lamp-control gives the path of a clip it checked, which is absolute; anything
    // else is not played.
    if !playable(path) {
        wait(Duration::from_millis(600), cancelled);
        return;
    }
    let engines: [(&str, &[&str]); 4] = [
        ("paplay", &[path]),
        ("aplay", &["-q", path]),
        ("afplay", &[path]),
        (
            "ffplay",
            &["-nodisp", "-autoexit", "-loglevel", "quiet", path],
        ),
    ];
    if !speak_with(&engines, "", cancelled) {
        wait(Duration::from_millis(600), cancelled);
    }
}

/// The host's speech engines, in the order tried, each with the arguments that
/// make it read its standard input and (for `spd-say`) wait until it has spoken.
/// `spd-say` is last: speech-dispatcher (Ubuntu and most Linux desktops have it,
/// and often no other engine) hands the line to its own synthesizer.
const ENGINES: &[(&str, &[&str])] = &[
    ("say", &["-f", "-"]),
    ("espeak-ng", &["--stdin"]),
    ("espeak", &["--stdin"]),
    ("spd-say", &["--wait", "--pipe-mode"]),
];

/// Speaks `text` on the host and returns when done: the host's own engine
/// where there is one, else a silent wait as long as the line. The text goes
/// to the engine on its standard input, never as an argument, so a line that
/// starts with `-` is words, not an option (`say -f FILE` would read a file
/// aloud).
fn speak(text: &str, cancelled: &AtomicBool) {
    if !speak_with(ENGINES, text, cancelled) {
        wait(
            Duration::from_millis(60 * text.chars().count() as u64).min(SILENT_MAX),
            cancelled,
        );
    }
}

/// Speaks `text` with the first of `engines` that starts and does not fail at
/// once (a player that is installed but exits with an error, such as `paplay`
/// with no sound server, is passed over for the next); false if none spoke.
fn speak_with(engines: &[(&str, &[&str])], text: &str, cancelled: &AtomicBool) -> bool {
    for (program, args) in engines {
        let Ok(mut child) = Command::new(program)
            .args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        // Until it ends, or the words are cancelled: then it is cut off.
        loop {
            match child.try_wait() {
                // It ended: spoken if it says so, else the next engine tries.
                Ok(Some(status)) if status.success() => return true,
                Ok(Some(_)) => break,
                _ => {}
            }
            if cancelled.load(Ordering::SeqCst) {
                let _ = child.kill();
                let _ = child.wait();
                cut_off(program);
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
    false
}

/// Cuts off what an engine has already handed on, when its client is killed:
/// speech-dispatcher goes on speaking a queued line after `spd-say` is gone, so
/// `spd-say --cancel` stops it (measured: a killed client's line ran to its end,
/// and was silent within half a second of the cancel). The other engines speak
/// only as long as they run. Known limit: `--cancel` is "CANCEL ALL", so it also
/// cuts any other speech of the same user's speech-dispatcher (a screen reader);
/// speech-dispatcher's own socket has a per-client `CANCEL SELF`, not used here.
fn cut_off(program: &str) {
    if Path::new(program)
        .file_name()
        .is_some_and(|name| name == "spd-say")
    {
        let _ = Command::new(program)
            .arg("--cancel")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// Serves lamp-actions' call socket at `path`, forever: `volume_to` only.
pub fn serve_actions(path: &Path) -> std::io::Result<()> {
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    let level = Arc::new(AtomicU64::new(50));
    for stream in listener.incoming().flatten() {
        let level = Arc::clone(&level);
        thread::spawn(move || {
            let _ = action(stream, &level);
        });
    }
    Ok(())
}

fn action(mut stream: UnixStream, level: &AtomicU64) -> std::io::Result<()> {
    let mut line = String::new();
    BufReader::new(stream.try_clone()?).read_line(&mut line)?;
    let Ok(asked) = serde_json::from_str::<Value>(&line) else {
        return Ok(());
    };
    if asked["name"] == "volume_to"
        && let Some(to) = asked["args"]["volume"].as_u64()
    {
        level.store(to.min(100), Ordering::Relaxed);
    }
    let now = level.load(Ordering::Relaxed);
    writeln!(
        stream,
        "{}",
        json!({"ok": true, "level": {"volume": now, "raw": now, "max_volume": 100}})
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder to run a fake engine from: under the crate, since a temporary
    /// folder may not allow running a program.
    fn scratch(name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("test-scratch-{name}-{}", std::process::id()))
    }

    #[test]
    fn a_text_that_starts_with_a_dash_reaches_the_engine_as_words_on_stdin() {
        let _one_at_a_time = crate::FAKE_PROGRAM
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = scratch("voice");
        std::fs::create_dir_all(&dir).unwrap();
        let engine = dir.join("engine.sh");
        let (args, said) = (dir.join("args"), dir.join("said"));
        std::fs::write(
            &engine,
            format!(
                "#!/bin/sh\necho \"$@\" > {}\ncat > {}\n",
                args.display(),
                said.display()
            ),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let program = engine.to_str().unwrap();
        let never = AtomicBool::new(false);
        assert!(speak_with(
            &[(program, &["--stdin"])],
            "-f /etc/hostname",
            &never
        ));
        assert_eq!(std::fs::read_to_string(&args).unwrap().trim(), "--stdin");
        assert_eq!(std::fs::read_to_string(&said).unwrap(), "-f /etc/hostname");
        assert!(!speak_with(
            &[("lamp-sim-no-such-engine", &[])],
            "hello",
            &never
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cancel_cuts_the_speech_off_at_once() {
        let _one_at_a_time = crate::FAKE_PROGRAM
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = scratch("cancel");
        std::fs::create_dir_all(&dir).unwrap();
        let engine = dir.join("slow.sh");
        std::fs::write(&engine, "#!/bin/sh\ncat > /dev/null\nsleep 30\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let later = Arc::clone(&cancelled);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(200));
            later.store(true, Ordering::SeqCst);
        });
        let started = std::time::Instant::now();
        assert!(speak_with(
            &[(engine.to_str().unwrap(), &[])],
            "a long line",
            &cancelled
        ));
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_clip_is_handed_to_the_player_as_its_path() {
        let _one_at_a_time = crate::FAKE_PROGRAM
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = scratch("clip");
        std::fs::create_dir_all(&dir).unwrap();
        let engine = dir.join("player.sh");
        let args = dir.join("args");
        std::fs::write(
            &engine,
            format!("#!/bin/sh\necho \"$@\" > {}\n", args.display()),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let never = AtomicBool::new(false);
        assert!(speak_with(
            &[(engine.to_str().unwrap(), &["-q", "/clips/a.wav"])],
            "",
            &never
        ));
        assert_eq!(
            std::fs::read_to_string(&args).unwrap().trim(),
            "-q /clips/a.wav"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_computers_exact_line_is_told_on_voice_line_and_nothing_else_is() {
        let dir = std::env::temp_dir().join(format!("lsv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bus = Bus::new(dir.join("bus")).unwrap();
        let lines = Mutex::new(
            bus.publisher_declared(SaidLine::TOPIC, &SaidLine::declaration(), "voice")
                .unwrap(),
        );
        let mut heard = bus
            .subscribe_typed(SaidLine::TOPIC, "test", SaidLine::TYPE)
            .unwrap();
        // Told: a computer's word-for-word say, as lamp-control sends it.
        tell(
            &lines,
            &json!({"skill": "control", "text": " Hello there. ", "exact": true, "by": "c_7"}),
        );
        // Not told: a written turn for the model (no `by`), a skill's line, a cue, an empty one.
        tell(&lines, &json!({"skill": "control", "text": "no by"}));
        tell(
            &lines,
            &json!({"skill": "control", "text": "no by", "exact": true}),
        );
        tell(
            &lines,
            &json!({"skill": "timer", "text": "skill", "exact": true, "by": "c_7"}),
        );
        tell(&lines, &json!({"skill": "control", "cue": "hum"}));
        tell(
            &lines,
            &json!({"skill": "control", "text": "  ", "exact": true, "by": "c_7"}),
        );
        let message = heard.recv(Duration::from_secs(2)).unwrap().unwrap();
        let line: SaidLine = message.parse().unwrap();
        assert_eq!(
            line,
            SaidLine {
                text: "Hello there.".to_owned(),
                by: LineBy::Computer,
                source: LineSource::Exact,
                app: Some("c_7".to_owned()),
                request: None,
                cut: false,
                scope: None,
            }
        );
        assert!(heard.recv(Duration::from_millis(300)).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn spd_say_is_an_engine_that_reads_stdin_and_waits_and_is_cancelled_by_its_cancel() {
        let _one_at_a_time = crate::FAKE_PROGRAM
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Listed, with the text on stdin (never an argument), after the engines that were.
        assert_eq!(
            ENGINES.last(),
            Some(&("spd-say", &["--wait", "--pipe-mode"][..]))
        );
        assert_eq!(ENGINES.len(), 4);
        // A fake `spd-say` that logs how it was called and speaks "for ever".
        let dir = scratch("spd");
        std::fs::create_dir_all(&dir).unwrap();
        let engine = dir.join("spd-say");
        let log = dir.join("calls");
        std::fs::write(
            &engine,
            format!(
                "#!/bin/sh\necho \"$@\" >> {}\n[ \"$1\" = --cancel ] && exit 0\ncat > /dev/null\nsleep 30\n",
                log.display()
            ),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let program = engine.to_str().unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancelled);
        let late = thread::spawn(move || {
            thread::sleep(Duration::from_millis(400));
            flag.store(true, Ordering::SeqCst);
        });
        let began = std::time::Instant::now();
        assert!(speak_with(
            &[(program, &["--wait", "--pipe-mode"])],
            "hello",
            &cancelled
        ));
        late.join().unwrap();
        // Cut off at the cancel (not after the fake's 30 s), and the client told
        // speech-dispatcher to stop what it had queued.
        assert!(began.elapsed() < Duration::from_secs(10));
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "--wait --pipe-mode\n--cancel\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_player_that_fails_at_once_is_passed_over_for_the_next_and_a_dash_path_is_no_clip() {
        let _one_at_a_time = crate::FAKE_PROGRAM
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = scratch("fails");
        std::fs::create_dir_all(&dir).unwrap();
        let script = |name: &str, body: &str| {
            use std::os::unix::fs::PermissionsExt;
            let path = dir.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            path
        };
        let (log, first, second) = (dir.join("log"), script("first", ""), script("second", ""));
        std::fs::write(
            &first,
            format!("#!/bin/sh\necho first >> {}\nexit 1\n", log.display()),
        )
        .unwrap();
        std::fs::write(
            &second,
            format!("#!/bin/sh\necho second >> {}\nexit 0\n", log.display()),
        )
        .unwrap();
        let cancelled = AtomicBool::new(false);
        let (a, b) = (first.to_str().unwrap(), second.to_str().unwrap());
        // The first fails at once, so the second is tried and speaks.
        assert!(speak_with(&[(a, &[]), (b, &[])], "", &cancelled));
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "first\nsecond\n");
        // Every engine failing is no engine: the caller's silent wait follows.
        assert!(!speak_with(&[(a, &[]), (a, &[])], "", &cancelled));
        // A path that starts with a dash is no clip for a player.
        assert!(playable("/clips/a.wav"));
        assert!(!playable("-f/etc/hostname") && !playable("clips/a.wav"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
