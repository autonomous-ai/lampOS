//! The voice's stand-in on the say socket, and lamp-actions' on its call
//! socket (the volume). A line in, `begun`, the host's speech engine, then
//! the outcome `spoken`, as lamp-control reads them (`parts.rs`). One line at
//! a time: a second `say` waits for the first. A computer's `stop` writes
//! `{"cancel":true}` on its say's connection and the speech ends at once,
//! answered `dropped`.
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

/// Serves the say socket at `path`, forever.
pub fn serve_say(path: &Path) -> std::io::Result<()> {
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    for stream in listener.incoming().flatten() {
        thread::spawn(move || {
            let _ = say(stream);
        });
    }
    Ok(())
}

/// One line is spoken at a time.
static SPEAKING: Mutex<()> = Mutex::new(());

fn say(mut stream: UnixStream) -> std::io::Result<()> {
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
    match asked["text"].as_str() {
        Some(text) => speak(text, &cancelled),
        // A cue or a clip: a short while of sound.
        None => wait(Duration::from_millis(600), &cancelled),
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

/// Speaks `text` on the host and returns when done: the host's own engine
/// where there is one, else a silent wait as long as the line. The text goes
/// to the engine on its standard input, never as an argument, so a line that
/// starts with `-` is words, not an option (`say -f FILE` would read a file
/// aloud).
fn speak(text: &str, cancelled: &AtomicBool) {
    let engines: &[(&str, &[&str])] = &[
        ("say", &["-f", "-"]),
        ("espeak-ng", &["--stdin"]),
        ("espeak", &["--stdin"]),
    ];
    if !speak_with(engines, text, cancelled) {
        wait(
            Duration::from_millis(60 * text.chars().count() as u64).min(SILENT_MAX),
            cancelled,
        );
    }
}

/// Speaks `text` with the first of `engines` that starts; false if none did.
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
            if matches!(child.try_wait(), Ok(Some(_))) {
                break;
            }
            if cancelled.load(Ordering::SeqCst) {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        return true;
    }
    false
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
}
