//! `lamp-cloud face-remove NAME`: the app's "Remove" of a person entirely
//! (`face.remove` with `"entirely": true`, [`crate::faces::remove`]), run by
//! hand on the lamp, so a builder can check a whole person's removal with no
//! app (A8, #2192). The same two steps the app's remove takes: the eyes'
//! own forget on their request socket, then memory's `forget_person` for each
//! id the eyes' answer names ([`crate::faces::memory_asks`]). No switch, no
//! broker and no new socket: it only connects to the eyes' and memory's
//! sockets, which their modes keep to root and the parts' own users.
//!
//! Stricter than the app's remove in one way: a name the eyes don't list
//! (known, or from a photo waiting for a yes) is refused and nothing is
//! asked to forget, so a mistyped name never forgets a by-name id. Run as
//! root (memory's main socket is 0600 to `lampos-memory`) or as `lampos`
//! with `LAMP_MEMORY_FORGET_SOCKET` naming memory's narrow socket, when a
//! lamp opens one; anyone else is refused before any socket is touched.
//! Memory's socket is checked first: one this user can't open is refused
//! before the eyes are asked anything, so the face never goes while memory
//! keeps the person (Reviewer-lead's P3 on #6580).
use crate::faces::{self, Eyes};
use serde_json::{Value, json};
use std::path::PathBuf;

/// The eyes' request socket on a product lamp (`lamp-eyes --who-socket`).
pub const WHO_SOCKET: &str = "/run/lampos-eyes/who.sock";
/// Memory's own socket on a product lamp (`lamp-memory --socket`), which
/// takes `forget_person` from root.
pub const MEMORY_SOCKET: &str = "/run/lampos-memory/memory.sock";
pub const USAGE: &str = "usage: lamp-cloud face-remove NAME";

/// Root, or the user named `lampos` in `passwd` (`/etc/passwd`'s text).
pub fn may_run(uid: u32, passwd: &str) -> bool {
    uid == 0
        || passwd.lines().any(|line| {
            let mut fields = line.split(':');
            fields.next() == Some("lampos")
                && fields.nth(1).and_then(|id| id.parse::<u32>().ok()) == Some(uid)
        })
}

/// The sockets from the environment when absolute, else the product's.
pub fn eyes_from(var: impl Fn(&str) -> Option<String>) -> Eyes {
    let path = |name: &str, default: &str| {
        var(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| PathBuf::from(default))
    };
    Eyes {
        socket: path(crate::photos::SOCKET_ENVIRONMENT, WHO_SOCKET),
        memory: Some(path(faces::MEMORY_ENVIRONMENT, MEMORY_SOCKET)),
    }
}

/// Memory's socket, if this user may open it: write permission on it and a
/// way through its directories (`access(2)`, as the real user), which a
/// connect needs. `Err` is the refusal, nothing having been asked.
pub fn memory_reachable(eyes: &Eyes) -> Result<(), String> {
    let Some(socket) = &eyes.memory else {
        return Err("refused: memory's socket isn't given; nothing was removed".to_owned());
    };
    rustix::fs::access(socket, rustix::fs::Access::WRITE_OK).map_err(|why| {
        format!(
            "refused: memory's socket {} can't be opened ({why}): run as root, or name memory's narrow socket in {}; nothing was removed",
            socket.display(),
            faces::MEMORY_ENVIRONMENT
        )
    })
}

/// `face-remove NAME` as the user running it, on the lamp's sockets.
pub fn command(arguments: &[String]) -> (String, u8) {
    let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
    if !may_run(rustix::process::getuid().as_raw(), &passwd) {
        return ("refused: run as root or lampos".to_owned(), 1);
    }
    command_at(&eyes_from(|name| std::env::var(name).ok()), arguments)
}

/// [`command`] on `eyes`: one line, `removed: {…}` with the app's answer and
/// the memory ids asked (`people`), or `refused: WHY`. The exit is 0 only
/// when the face and memory both went; 1 otherwise; 2 for a wrong call.
pub fn command_at(eyes: &Eyes, arguments: &[String]) -> (String, u8) {
    let [name] = arguments else {
        return (USAGE.to_owned(), 2);
    };
    let name = match faces::asked(&json!({ "name": name })) {
        Ok(name) => name,
        Err(why) => return (format!("refused: {why}"), 1),
    };
    if let Err(refused) = memory_reachable(eyes) {
        return (refused, 1);
    }
    let listed = match faces::list(eyes) {
        Ok(listed) => listed,
        Err(why) => return (format!("refused: {why}"), 1),
    };
    let names: Vec<&str> = listed["faces"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|face| face["name"].as_str())
        .collect();
    // As the eyes match a name (`who::same_name`), and in their spelling.
    let Some(known) = names
        .iter()
        .find(|known| known.to_lowercase() == name.to_lowercase())
    else {
        return (
            format!(
                "refused: the lamp knows no face called {name} (it knows: {})",
                if names.is_empty() {
                    "nobody".to_owned()
                } else {
                    names.join(", ")
                }
            ),
            1,
        );
    };
    match faces::remove_asking(eyes, &json!({"name": known, "entirely": true}), None) {
        Ok((mut told, asks)) => {
            let people: Vec<Value> = asks.iter().map(|ask| ask["person"].clone()).collect();
            told["people"] = Value::Array(people);
            let done = told["memory"] == "forgotten" && told.get("face").is_none();
            (format!("removed: {told}"), if done { 0 } else { 1 })
        }
        Err(why) => (format!("refused: {why}"), 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc::Receiver;

    /// A stand-in on a short path answering `answers` in turn: what was asked.
    fn stand_in(label: &str, file: &str, answers: Vec<Value>) -> (PathBuf, Receiver<Value>) {
        let dir = PathBuf::from(format!("/tmp/fr-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join(file);
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket).unwrap();
        let (seen, asked) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for answer in answers {
                let (stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                // A test that doesn't read what was asked drops its end.
                let _ = seen.send(serde_json::from_str(line.trim()).unwrap());
                writeln!(&stream, "{answer}").unwrap();
            }
        });
        (socket, asked)
    }

    fn arguments(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn only_root_and_lampos_may_run_it() {
        let passwd = "root:x:0:0::/root:/bin/sh\nlampos:x:998:998::/:/usr/sbin/nologin\nlamposx:x:997:997::/:/bin/false\norangepi:x:1000:1000::/home/orangepi:/bin/bash\n";
        assert!(may_run(0, passwd));
        assert!(may_run(998, passwd));
        assert!(
            !may_run(997, passwd),
            "a user whose name only starts lampos"
        );
        assert!(!may_run(1000, passwd));
        assert!(!may_run(998, ""), "no passwd: root only");
    }

    #[test]
    fn the_sockets_are_the_products_unless_given_absolute() {
        assert_eq!(
            eyes_from(|_| None),
            Eyes {
                socket: WHO_SOCKET.into(),
                memory: Some(MEMORY_SOCKET.into()),
            }
        );
        let given = eyes_from(|name| match name {
            "LAMP_EYES_WHO_SOCKET" => Some("/x/who.sock".to_owned()),
            "LAMP_MEMORY_FORGET_SOCKET" => Some("forget.sock".to_owned()),
            _ => None,
        });
        assert_eq!(given.socket, PathBuf::from("/x/who.sock"));
        assert_eq!(
            given.memory,
            Some(MEMORY_SOCKET.into()),
            "relative is ignored"
        );
    }

    #[test]
    fn a_wrong_call_is_the_usage_and_asks_nothing() {
        let eyes = Eyes {
            socket: "/nonexistent/who.sock".into(),
            memory: None,
        };
        assert_eq!(command_at(&eyes, &[]), (USAGE.to_owned(), 2));
        assert_eq!(
            command_at(&eyes, &arguments(&["Lan", "Mai"])),
            (USAGE.to_owned(), 2)
        );
    }

    /// A name the eyes don't list: refused with the names they know, and
    /// nothing forgotten: the eyes are asked for their list only, memory
    /// never.
    #[test]
    fn an_unknown_name_is_refused_and_nothing_is_forgotten() {
        let (who, asked) = stand_in(
            "unknown",
            "who.sock",
            vec![json!({"people": ["Lan", "Minh"], "waiting": []})],
        );
        let (memory, memory_asked) = stand_in("unknown", "memory.sock", vec![json!({"ok": true})]);
        let eyes = Eyes {
            socket: who,
            memory: Some(memory),
        };
        assert_eq!(
            command_at(&eyes, &arguments(&["Zara"])),
            (
                "refused: the lamp knows no face called Zara (it knows: Lan, Minh)".to_owned(),
                1
            )
        );
        assert_eq!(
            asked.recv().unwrap(),
            json!({"list": true, "waiting": true})
        );
        assert!(asked.try_recv().is_err(), "the eyes were asked to forget");
        assert!(
            memory_asked
                .recv_timeout(std::time::Duration::from_millis(200))
                .is_err(),
            "memory was asked"
        );
    }

    /// A known name, typed in another case: the eyes forget it in their own
    /// spelling, memory forgets each id, and the line says what went.
    #[test]
    fn a_known_name_is_removed_from_the_eyes_and_memory() {
        let (minh, lan) = (faces::person_id("Minh"), faces::person_id("Lan"));
        let (who, asked) = stand_in(
            "known",
            "who.sock",
            vec![
                json!({"people": ["Lan"], "waiting": []}),
                json!({"forgot": true, "id": minh, "name_id_free": true}),
            ],
        );
        let (memory, memory_asked) = stand_in(
            "known",
            "memory.sock",
            vec![
                json!({"ok": true, "removed": 3, "rules": 1}),
                json!({"ok": true, "removed": 0, "rules": 0}),
            ],
        );
        let eyes = Eyes {
            socket: who,
            memory: Some(memory),
        };
        let (line, code) = command_at(&eyes, &arguments(&["lan"]));
        assert_eq!(code, 0, "{line}");
        let told: Value = serde_json::from_str(line.strip_prefix("removed: ").unwrap()).unwrap();
        assert_eq!(
            told,
            json!({"removed": "Lan", "memory": "forgotten", "people": [minh, lan]})
        );
        assert_eq!(
            asked.recv().unwrap(),
            json!({"list": true, "waiting": true})
        );
        assert_eq!(asked.recv().unwrap(), json!({"forget": "Lan"}));
        assert_eq!(
            memory_asked.recv().unwrap(),
            json!({"op": "forget_person", "person": minh})
        );
        assert_eq!(
            memory_asked.recv().unwrap(),
            json!({"op": "forget_person", "person": lan})
        );
    }

    /// A face from a photo still waiting for a yes is a face the lamp has:
    /// removed as the app's remove takes it.
    #[test]
    fn a_waiting_face_is_removed_too() {
        let bo = faces::person_id("Bo");
        let (who, _) = stand_in(
            "waiting",
            "who.sock",
            vec![
                json!({"people": [], "waiting": ["Bo"]}),
                json!({"forgot": true, "name_id_free": true}),
            ],
        );
        let (memory, _) = stand_in("waiting", "memory.sock", vec![json!({"ok": true})]);
        let eyes = Eyes {
            socket: who,
            memory: Some(memory),
        };
        let (line, code) = command_at(&eyes, &arguments(&["Bo"]));
        assert_eq!(code, 0, "{line}");
        assert!(line.contains(&bo), "{line}");
    }

    /// Memory that refuses: the face went, the line says memory was kept,
    /// and the exit is 1, never a pass.
    #[test]
    fn memory_that_refuses_is_said_and_fails() {
        let (who, _) = stand_in(
            "refused",
            "who.sock",
            vec![
                json!({"people": ["Lan"]}),
                json!({"forgot": true, "name_id_free": true}),
            ],
        );
        let (memory, _) = stand_in(
            "refused",
            "memory.sock",
            vec![json!({"ok": false, "error": "not_allowed"})],
        );
        let eyes = Eyes {
            socket: who,
            memory: Some(memory),
        };
        let (line, code) = command_at(&eyes, &arguments(&["Lan"]));
        assert_eq!(code, 1, "{line}");
        assert!(line.starts_with("removed: "), "{line}");
        assert!(line.contains(r#""memory":"kept""#), "{line}");
    }

    /// Memory this user can't open (lampos on the product's main socket,
    /// or none there): refused before the eyes are asked anything, so the
    /// face stays with the memory (Reviewer-lead's P3 on #6580).
    #[test]
    fn memory_out_of_reach_is_refused_before_the_eyes_are_asked() {
        let (who, asked) = stand_in(
            "unreachable",
            "who.sock",
            vec![
                json!({"people": ["Lan"], "waiting": []}),
                json!({"forgot": true, "name_id_free": true}),
            ],
        );
        for memory in [Some(PathBuf::from("/nonexistent/memory.sock")), None] {
            let eyes = Eyes {
                socket: who.clone(),
                memory,
            };
            let (line, code) = command_at(&eyes, &arguments(&["Lan"]));
            assert_eq!(code, 1, "{line}");
            assert!(line.starts_with("refused: memory's socket"), "{line}");
            assert!(line.ends_with("nothing was removed"), "{line}");
        }
        assert!(
            asked
                .recv_timeout(std::time::Duration::from_millis(200))
                .is_err(),
            "the eyes were asked"
        );
    }

    #[test]
    fn eyes_that_do_not_answer_are_refused() {
        let memory = std::env::temp_dir().join(format!("fr-mute-{}.sock", std::process::id()));
        std::fs::write(&memory, b"").unwrap();
        let eyes = Eyes {
            socket: "/nonexistent/who.sock".into(),
            memory: Some(memory.clone()),
        };
        assert_eq!(
            command_at(&eyes, &arguments(&["Lan"])),
            ("refused: the lamp's eyes did not answer".to_owned(), 1)
        );
        let _ = std::fs::remove_file(memory);
    }
}
