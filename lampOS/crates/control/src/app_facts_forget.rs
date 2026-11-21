//! A removed app's facts in Lamp's memory (#7415, C166, Friday's P2 on
//! #8203): `app_forget` marks the app here (`--state-dir`'s [`PENDING`],
//! 0600) before anything else of it goes, and a thread of lamp-control's
//! sends memory's `forget_app_facts` for `app:<name>` on its forget socket
//! ([`SOCKET`], which takes this op and `forget_person` only) at once and
//! every [`RETRY_EVERY`] after, until memory answers `ok`; only then is the
//! mark let go. So a remove forgets the app's facts even while memory, the
//! voice or the bus is down, and across a restart of lamp-control. While a
//! mark is pending the app's new facts are refused (a reinstall's facts
//! would be forgotten with the old). The journal says the app and the
//! count, never a fact.
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::io::{BufRead as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Duration;

/// Memory's forget socket (`lamp-memory --forget-socket`, group `lampos`).
pub const SOCKET: &str = "/run/lampos-memory/forget.sock";
/// The pending marks, in `--state-dir`.
pub const PENDING: &str = "app-facts-forget.json";
/// How often the pending forgets are tried again.
pub const RETRY_EVERY: Duration = Duration::from_secs(30);
/// How long one forget may take memory.
pub const WITHIN: Duration = Duration::from_secs(5);

/// One writer of the marks at a time (the admin socket's remove, the thread).
static FILE: Mutex<()> = Mutex::new(());

/// The apps whose facts are still to be forgotten, from `state`.
pub fn pending(state: &Path) -> BTreeSet<String> {
    let _held = FILE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    read(state)
}

fn read(state: &Path) -> BTreeSet<String> {
    std::fs::read(state.join(PENDING))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<String>>(&bytes).ok())
        .map(|names| {
            names
                .into_iter()
                .filter(|name| lamp_skills::app_package::app_name_ok(name))
                .collect()
        })
        .unwrap_or_default()
}

fn write(state: &Path, names: &BTreeSet<String>) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let target = state.join(PENDING);
    let temporary = state.join(format!(".{PENDING}.new"));
    let _ = std::fs::remove_file(&temporary);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|error| format!("the app's facts' forget could not be kept: {error}"))?;
    file.write_all(json!(names).to_string().as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("the app's facts' forget could not be kept: {error}"))?;
    std::fs::rename(&temporary, &target)
        .map_err(|error| format!("the app's facts' forget could not be kept: {error}"))
}

/// Marks app `name`'s facts to be forgotten: kept on disk before it returns.
pub fn mark(state: &Path, name: &str) -> Result<(), String> {
    let _held = FILE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut names = read(state);
    if names.insert(name.to_owned()) {
        write(state, &names)?;
    }
    Ok(())
}

fn unmark(state: &Path, name: &str) {
    let _held = FILE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut names = read(state);
    if names.remove(name)
        && let Err(why) = write(state, &names)
    {
        eprintln!("lamp-control: app {name}: {why}");
    }
}

/// Memory's line for app `name`'s facts.
pub fn line(name: &str) -> Value {
    json!({"op": lamp_msgs::app_fact::FORGET_OP, "source": format!("app:{name}")})
}

/// Asks memory on `socket` to forget app `name`'s facts: how many went.
/// `unknown_op` is done too: a memory with no Days store keeps no app facts.
pub fn ask(socket: &Path, name: &str) -> Result<u64, String> {
    let mut stream = std::os::unix::net::UnixStream::connect(socket)
        .map_err(|error| format!("memory's forget socket: {}", error.kind()))?;
    stream
        .set_read_timeout(Some(WITHIN))
        .and_then(|()| stream.set_write_timeout(Some(WITHIN)))
        .map_err(|error| format!("memory's forget socket: {}", error.kind()))?;
    writeln!(stream, "{}", line(name))
        .map_err(|error| format!("memory's forget socket: {}", error.kind()))?;
    let mut answer = String::new();
    // One line, at most 4 KiB, as memory answers.
    std::io::BufReader::new(std::io::Read::take(&stream, 4096))
        .read_line(&mut answer)
        .map_err(|error| format!("memory's forget socket: {}", error.kind()))?;
    let answer: Value =
        serde_json::from_str(&answer).map_err(|_| "memory's answer is not JSON".to_owned())?;
    if answer.get("ok") == Some(&json!(true)) {
        return Ok(answer.get("removed").and_then(Value::as_u64).unwrap_or(0));
    }
    match answer.get("error").and_then(Value::as_str) {
        Some("unknown_op") => Ok(0),
        Some(code) => Err(format!("memory refused it ({code})")),
        None => Err("memory refused it".to_owned()),
    }
}

/// Every pending forget tried once: the ones memory did are let go. How
/// many are still pending.
pub fn run(state: &Path, socket: &Path) -> usize {
    let mut left = 0;
    for name in pending(state) {
        match ask(socket, &name) {
            Ok(removed) => {
                unmark(state, &name);
                eprintln!("lamp-control: app {name} removed: {removed} of its facts forgotten");
            }
            Err(why) => {
                left += 1;
                eprintln!(
                    "lamp-control: app {name} removed: its facts are not forgotten yet, tried again in {} s: {why}",
                    RETRY_EVERY.as_secs()
                );
            }
        }
    }
    left
}

/// The thread that runs the pending forgets: at its start (a mark left by
/// a restart), at each kick (a remove), and every `every` while any is
/// pending. The kick is the returned sender.
pub fn start(state: PathBuf, socket: PathBuf, every: Duration) -> Option<mpsc::Sender<()>> {
    let (kick, kicked) = mpsc::channel::<()>();
    let spawned = std::thread::Builder::new()
        .name("app-facts-forget".into())
        .spawn(move || {
            loop {
                let left = run(&state, &socket);
                let next = if left > 0 {
                    kicked.recv_timeout(every)
                } else {
                    kicked
                        .recv()
                        .map_err(|_| mpsc::RecvTimeoutError::Disconnected)
                };
                if next == Err(mpsc::RecvTimeoutError::Disconnected) && left == 0 {
                    return;
                }
            }
        });
    match spawned {
        Ok(_) => Some(kick),
        Err(error) => {
            eprintln!("lamp-control: a removed app's facts are not forgotten: {error}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn folder(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lc-aff-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A memory on `socket` that answers each line `answer` and says what
    /// it was asked.
    fn memory(socket: &Path, answer: Value) -> mpsc::Receiver<Value> {
        let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
        let (asked, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut line = String::new();
                if std::io::BufReader::new(&stream)
                    .read_line(&mut line)
                    .is_ok()
                {
                    let _ = asked.send(serde_json::from_str::<Value>(&line).unwrap());
                    let mut stream = stream;
                    let _ = writeln!(stream, "{answer}");
                }
            }
        });
        lines
    }

    #[test]
    fn a_mark_is_kept_until_memory_says_it_was_done() {
        let state = folder("mark");
        let socket = state.join("f.sock");
        mark(&state, "gym-coach").unwrap();
        mark(&state, "gym-coach").unwrap();
        assert_eq!(pending(&state), BTreeSet::from(["gym-coach".to_owned()]));
        // Memory down: kept.
        assert_eq!(run(&state, &socket), 1);
        assert!(pending(&state).contains("gym-coach"));
        // Memory refuses: kept.
        let refusing = state.join("r.sock");
        let _asked = memory(&refusing, json!({"ok": false, "error": "storage"}));
        assert_eq!(run(&state, &refusing), 1);
        // Memory answers ok: let go, and exactly the app's source was asked.
        let asked = memory(&socket, json!({"ok": true, "removed": 2}));
        assert_eq!(run(&state, &socket), 0);
        assert!(pending(&state).is_empty());
        assert_eq!(
            asked.recv_timeout(Duration::from_secs(5)).unwrap(),
            json!({"op": "forget_app_facts", "source": "app:gym-coach"})
        );
        let _ = std::fs::remove_dir_all(&state);
    }

    /// Memory down at the remove: the thread tries again and the facts go
    /// once memory is reachable, with no second remove.
    #[test]
    fn a_forget_made_while_memory_is_down_is_done_once_it_is_back() {
        let state = folder("retry");
        let socket = state.join("f.sock");
        mark(&state, "gym-coach").unwrap();
        let kick = start(state.clone(), socket.clone(), Duration::from_millis(20)).unwrap();
        let _ = kick.send(());
        std::thread::sleep(Duration::from_millis(100));
        assert!(pending(&state).contains("gym-coach"), "memory is down");
        let asked = memory(&socket, json!({"ok": true, "removed": 1}));
        let begun = Instant::now();
        while !pending(&state).is_empty() {
            assert!(begun.elapsed() < Duration::from_secs(5), "never forgotten");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            asked.recv_timeout(Duration::from_secs(5)).unwrap()["source"],
            "app:gym-coach"
        );
        drop(kick);
        let _ = std::fs::remove_dir_all(&state);
    }

    #[test]
    fn a_memory_with_no_app_facts_is_done_and_a_bad_mark_is_none() {
        let state = folder("legacy");
        let socket = state.join("f.sock");
        let _asked = memory(&socket, json!({"ok": false, "error": "unknown_op"}));
        assert_eq!(ask(&socket, "gym-coach"), Ok(0));
        std::fs::write(state.join(PENDING), r#"["../x","gym-coach"]"#).unwrap();
        assert_eq!(pending(&state), BTreeSet::from(["gym-coach".to_owned()]));
        let _ = std::fs::remove_dir_all(&state);
    }
}
