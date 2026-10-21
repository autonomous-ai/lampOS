//! Guard mode from the owner's app (#2562): `guard.on` and `guard.off`,
//! handed to lamp-skills' call socket as
//! `{"op":"guard","turn":"on","deadline_us":...}` (or `"off"`), as
//! `skills.remove` is handed to the manage helper. `deadline_us` is when
//! this side stops waiting, on the host's shared monotonic clock: that
//! socket answers one connection at a time, and an op that waited behind a
//! voice call past it is let go there, so the lamp never turns what the app
//! was told was not turned. Off unless
//! `LAMP_GUARD_FROM_APP=on`; lamp-skills checks the same switch in its own
//! unit, and takes the op only from lamp-cloud's unit, by its credentials
//! and cgroup, so this is not the only gate.
//!
//! The owner's alone, as every kind here is: only the lamp's own account
//! channel commands it (`online::commanded`). lamp-cloud turns nothing
//! itself, and the watch's own rules hold: its faint mark is asked of the
//! ring, and it counts only while the ring confirms it (`docs/guard-mode.md`).
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The switch, in lamp-cloud's unit: `on` exactly.
pub const ENVIRONMENT: &str = lamp_skills::watch::FROM_APP_ENVIRONMENT;

/// lamp-skills' call socket, in its own runtime folder
/// (`deploy/product/lamp-skills.service`, `--tools-dir`).
pub const SOCKET: &str = "/run/lampos-skills/call.sock";

/// lamp-skills answers within its own second for the main loop and one more
/// for the answer; the broker's loop waits on it, so it is bounded short.
/// lamp-skills' own, so the deadline it holds the op to is this wait.
pub const WAIT: Duration = lamp_skills::watch::FROM_APP_WAIT;

/// What the app is told when lamp-skills can't be reached or does not
/// answer: nothing was turned.
pub const UNREACHED: &str = "guard mode can't be reached on the lamp now";

/// The call socket, with the switch on.
pub fn from_environment() -> Option<PathBuf> {
    lamp_skills::watch::switched_on(std::env::var(ENVIRONMENT).ok().as_deref())
        .then(|| SOCKET.into())
}

/// `guard.on` or `guard.off` (`kind`), asked of lamp-skills on `socket`:
/// `{"guard": "on" | "off"}`, or lamp-skills' reason in its own words
/// (the privacy switch closed, the mark not showable, guard mode not
/// installed), or [`UNREACHED`].
pub fn turn(socket: &Path, kind: &str) -> Result<Option<Value>, String> {
    let turn = match kind {
        lamp_skills::watch::APP_ON => lamp_skills::watch::ON,
        lamp_skills::watch::APP_OFF => lamp_skills::watch::OFF,
        _ => return Err(crate::wire::not_here(kind)),
    };
    let asked = || -> std::io::Result<String> {
        // Stamped before anything waits: this side waits at least `WAIT`
        // from here, so lamp-skills, letting go at the deadline, never
        // turns an op this side gave up on.
        let deadline_us = lamp_ipc::monotonic_us()
            .saturating_add(u64::try_from(WAIT.as_micros()).unwrap_or(u64::MAX));
        let mut stream = UnixStream::connect(socket)?;
        stream.set_read_timeout(Some(WAIT))?;
        stream.set_write_timeout(Some(WAIT))?;
        let op = json!({
            "op": "guard",
            "turn": turn,
            lamp_skills::watch::DEADLINE: deadline_us,
        });
        stream.write_all(format!("{op}\n").as_bytes())?;
        let mut reply = String::new();
        BufReader::new(stream.take(4_096)).read_line(&mut reply)?;
        Ok(reply)
    };
    let reply = asked().map_err(|error| {
        eprintln!("lamp-cloud: {kind} from the app: lamp-skills did not answer: {error}");
        UNREACHED.to_owned()
    })?;
    let reply: Value = serde_json::from_str(reply.trim()).map_err(|_| UNREACHED.to_owned())?;
    match reply.get("ok").and_then(Value::as_bool) {
        Some(true) => {
            eprintln!("lamp-cloud: {kind} from the app: done");
            Ok(reply.get("result").cloned())
        }
        Some(false) => {
            let why = reply
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or(UNREACHED);
            eprintln!("lamp-cloud: {kind} from the app refused: {why}");
            Err(why.to_owned())
        }
        None => Err(UNREACHED.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    /// A short folder of its own: a socket's whole path fits in 103 bytes.
    fn folder(tag: &str) -> PathBuf {
        let dir = Path::new("/tmp").join(format!("cg-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A stand-in for lamp-skills' call socket: each line asked, and its
    /// answer.
    fn skills(dir: &Path, answer: &'static str) -> std::sync::mpsc::Receiver<String> {
        let listener = UnixListener::bind(dir.join("call.sock")).unwrap();
        let (sent, asked) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let _ = sent.send(line);
                let _ = (&stream).write_all(format!("{answer}\n").as_bytes());
            }
        });
        asked
    }

    /// #2562: `guard.on` and `guard.off` go to lamp-skills as its `guard`
    /// op, and its answer comes back as it is; a refusal in its own words;
    /// no socket, nothing turned.
    #[test]
    fn guard_from_the_app_is_handed_to_lamp_skills_call_socket() {
        let dir = folder("on");
        let asked = skills(&dir, r#"{"ok":true,"result":{"guard":"on"}}"#);
        let socket = dir.join("call.sock");
        let before = lamp_ipc::monotonic_us();
        assert_eq!(turn(&socket, "guard.on"), Ok(Some(json!({"guard": "on"}))));
        let after = lamp_ipc::monotonic_us();
        let mut line: Value = serde_json::from_str(&asked.recv().unwrap()).unwrap();
        // The reviewer's P2 on #2688: when this side stops waiting, on the
        // host's shared clock, `WAIT` after it sent the op.
        let deadline = line
            .as_object_mut()
            .unwrap()
            .remove("deadline_us")
            .and_then(|deadline| deadline.as_u64())
            .expect("the op carries deadline_us");
        let wait = u64::try_from(WAIT.as_micros()).unwrap();
        assert!(
            (before + wait..=after + wait).contains(&deadline),
            "{deadline} not {wait} µs after {before}..{after}"
        );
        assert_eq!(line, json!({"op": "guard", "turn": "on"}));
        turn(&socket, "guard.off").unwrap();
        let line: Value = serde_json::from_str(&asked.recv().unwrap()).unwrap();
        assert_eq!(line["turn"], "off");
        assert!(turn(&socket, "guard.later").is_err());
        let refusing = folder("no");
        let _asked = skills(
            &refusing,
            r#"{"ok":false,"error":"guard mode is turned on and off from afar only by the owner's app"}"#,
        );
        assert_eq!(
            turn(&refusing.join("call.sock"), "guard.on"),
            Err(lamp_skills::watch::NOT_THE_APP.to_owned())
        );
        assert_eq!(
            turn(&dir.join("none.sock"), "guard.on"),
            Err(UNREACHED.to_owned())
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&refusing);
    }
}
