//! The writer of an installed app's facts (#7415, C166, `docs/apps.md`,
//! "What an app tells Lamp's memory"). lamp-control checks an app's
//! `apps.remember` (its grant, its words, the lamp not private, the rate,
//! `LAMP_APP_REMEMBER`) and tells it here on `apps/remember`; the voice keeps
//! it for the lamp's known person, never another and never the room, and
//! says how it went on `apps/remembered`. The owner may go by two memory ids
//! (a typed "Minh Nguyen" whose face was kept as "Minh", 30 Oct): the face's
//! is tried first, as the person's own memory is keyed by it, then each other
//! in turn while memory answers `not_known` ([`owners_face_first`], [`keep`]).
//! A removed app's facts are lamp-control's to forget, on memory's forget
//! socket.
//!
//! No switch of its own: nothing is told here unless lamp-control's
//! `LAMP_APP_REMEMBER` is on. The journal says the app and how it went,
//! never a word of the fact.
use lamp_msgs::app_fact::{self, AppRemember, AppRemembered};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long memory may take with one fact: inside lamp-control's 2.5 s wait.
const MEMORY_WITHIN: Duration = Duration::from_millis(1_500);

/// Memory's request that keeps `fact` for `person`.
pub fn remember_line(fact: &AppRemember, person: &str) -> Option<Value> {
    let source = fact.source()?;
    Some(json!({
        "op": app_fact::REMEMBER_OP,
        "person": person,
        "source": source.as_str(),
        "name": fact.name,
        "at_unix_ms": fact.at_unix_ms,
        "text": fact.text,
    }))
}

/// The owner's memory ids, the face's first (`known_faces::owner_now`, the
/// id the person's own memory is kept under once their face is), then the
/// typed name's and any other (`person::owner_ids`), each once.
pub fn owners_face_first() -> Vec<String> {
    let mut ids: Vec<String> = crate::known_faces::owner_now()
        .map(|name| crate::known_faces::memory_person(&name))
        .into_iter()
        .collect();
    for id in crate::person::owner_ids() {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

/// How `fact` went, for the lamp's known person among `owners` (the
/// owner's memory ids, [`owners_face_first`]): each known id is tried in
/// turn while memory answers `not_known` (it holds nothing of that id), and
/// the first other answer is the outcome. `memory` asks one line (none: not
/// reached or no answer in time).
pub fn keep(
    fact: &AppRemember,
    owners: &[String],
    mut memory: impl FnMut(&Value) -> Option<Value>,
) -> AppRemembered {
    let went = |outcome: &str| AppRemembered {
        ask: fact.ask,
        app: fact.app.clone(),
        outcome: outcome.to_owned(),
    };
    if fact.check().is_err() {
        return went("bad_request");
    }
    // `whose` with each id asked: only the lamp's known person, never the room.
    let known: Vec<String> = owners
        .iter()
        .filter_map(|id| app_fact::whose(Some(id), owners).ok())
        .collect();
    if known.is_empty() {
        return went(app_fact::Refused::NotKnown.code());
    }
    for person in &known {
        let Some(line) = remember_line(fact, person) else {
            return went("bad_source");
        };
        let Some(answer) = memory(&line) else {
            return went(app_fact::UNAVAILABLE);
        };
        if answer.get("ok") == Some(&json!(true)) {
            return went(app_fact::KEPT);
        }
        // Memory's own code, when it is one word; any other is unavailable.
        let code = answer
            .get("error")
            .and_then(Value::as_str)
            .filter(|code| {
                (1..=32).contains(&code.len())
                    && code.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
            })
            .unwrap_or(app_fact::UNAVAILABLE);
        if code != app_fact::Refused::NotKnown.code() {
            return went(code);
        }
    }
    went(app_fact::Refused::NotKnown.code())
}

/// Starts the writer on the bus at `root`, memory at `memory`, on a thread
/// of its own. `owners` gives the owner's memory ids at each fact
/// ([`owners_face_first`]). A part that can't start is said.
pub fn start(root: &Path, memory: PathBuf, owners: fn() -> Vec<String>) {
    let Ok(bus) = lamp_bus::Bus::new(root) else {
        eprintln!("lamp-voice: no app's fact is kept: the bus can't be reached");
        return;
    };
    let facts = bus.subscribe(AppRemember::TOPIC, "voice");
    let said = bus.publisher_declared(AppRemembered::TOPIC, &AppRemembered::declaration(), "voice");
    match (facts, said) {
        (Ok(mut facts), Ok(mut said)) => {
            let spawned = std::thread::Builder::new()
                .name("app-remember".into())
                .spawn(move || {
                    loop {
                        match facts.recv(Duration::from_secs(1)) {
                            Ok(Some(message)) => {
                                let Ok(fact) = message.parse::<AppRemember>() else {
                                    continue;
                                };
                                let went = keep(&fact, &owners(), |line| {
                                    crate::tools::exchange(&memory, line, MEMORY_WITHIN)
                                });
                                eprintln!(
                                    "lamp-voice: app {}: a fact for the known person: {}",
                                    went.app, went.outcome
                                );
                                if let Err(error) =
                                    said.publish_json(lamp_ipc::monotonic_us(), &went)
                                {
                                    eprintln!(
                                        "lamp-voice: app {}: how its fact went was not told: {error}",
                                        went.app
                                    );
                                }
                            }
                            Ok(None) => {}
                            Err(error) => {
                                eprintln!("lamp-voice: apps/remember can't be read: {error}");
                                std::thread::sleep(Duration::from_secs(1));
                            }
                        }
                    }
                });
            if let Err(error) = spawned {
                eprintln!("lamp-voice: no app's fact is kept: {error}");
            }
        }
        (Err(error), _) | (_, Err(error)) => {
            eprintln!("lamp-voice: no app's fact is kept: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const OWNER: &str = "p-0123456789abcdef";

    fn fact(text: &str) -> AppRemember {
        AppRemember {
            ask: 4,
            app: "gym-coach".into(),
            name: "Gym Coach".into(),
            text: text.into(),
            at_unix_ms: 1_763_632_020_000,
        }
    }

    /// The known person's, marked with the app as its source: memory is
    /// asked exactly `remember_app_fact` with the door's words.
    #[test]
    fn a_fact_is_kept_for_the_known_person_with_the_app_as_source() {
        let asked = RefCell::new(None);
        let went = keep(
            &fact("20 Nov 16:47: squats 3×10, 6 too shallow"),
            &[OWNER.to_owned()],
            |line| {
                *asked.borrow_mut() = Some(line.clone());
                Some(json!({"ok": true, "id": 12}))
            },
        );
        assert_eq!(
            went,
            AppRemembered {
                ask: 4,
                app: "gym-coach".into(),
                outcome: app_fact::KEPT.into()
            }
        );
        assert_eq!(
            asked.into_inner().unwrap(),
            json!({
                "op": "remember_app_fact",
                "person": OWNER,
                "source": "app:gym-coach",
                "name": "Gym Coach",
                "at_unix_ms": 1_763_632_020_000u64,
                "text": "20 Nov 16:47: squats 3×10, 6 too shallow",
            })
        );
    }

    /// No known person: refused before memory is asked, never the room's.
    #[test]
    fn with_no_known_person_nothing_is_asked() {
        for owners in [vec![], vec!["everyone".to_owned()]] {
            let went = keep(&fact("squats"), &owners, |_| panic!("memory asked"));
            assert_eq!(went.outcome, "not_known");
        }
        // Words the door never sends are refused here too.
        let went = keep(&fact("squats \"x\""), &[OWNER.to_owned()], |_| {
            panic!("memory asked")
        });
        assert_eq!(went.outcome, "bad_request");
    }

    /// Memory's refusal is its code; no answer, or one not a code, is
    /// unavailable.
    #[test]
    fn memorys_word_is_the_outcome() {
        let owners = [OWNER.to_owned()];
        let with =
            |answer: Option<Value>| keep(&fact("squats"), &owners, |_| answer.clone()).outcome;
        assert_eq!(
            with(Some(
                json!({"ok": false, "error": "too_many", "message": "m"})
            )),
            "too_many"
        );
        assert_eq!(
            with(Some(json!({"ok": false, "error": "forgotten"}))),
            "forgotten"
        );
        assert_eq!(with(None), app_fact::UNAVAILABLE);
        assert_eq!(
            with(Some(json!({"ok": false, "error": "Not One"}))),
            app_fact::UNAVAILABLE
        );
    }

    /// The owner by two ids (a typed "Minh Nguyen", a face kept as "Minh"):
    /// memory holds only the face's, so the typed one's `not_known` moves on
    /// to it, and the fact is kept where the person's own recall looks.
    #[test]
    fn a_fact_goes_to_the_owner_id_memory_holds() {
        const FACE: &str = "p-fedcba9876543210";
        let asked = RefCell::new(Vec::<String>::new());
        let memory = |line: &Value| {
            let person = line["person"].as_str().unwrap().to_owned();
            asked.borrow_mut().push(person.clone());
            Some(if person == FACE {
                json!({"ok": true, "id": 3})
            } else {
                json!({"ok": false, "error": "not_known", "message": "m"})
            })
        };
        let went = keep(
            &fact("squats"),
            &[OWNER.to_owned(), FACE.to_owned()],
            memory,
        );
        assert_eq!(went.outcome, app_fact::KEPT);
        assert_eq!(*asked.borrow(), [OWNER, FACE]);
        // The face's first: asked once, kept at once.
        asked.borrow_mut().clear();
        let went = keep(
            &fact("squats"),
            &[FACE.to_owned(), OWNER.to_owned()],
            memory,
        );
        assert_eq!(went.outcome, app_fact::KEPT);
        assert_eq!(*asked.borrow(), [FACE]);
        // Neither held: not_known after both; another refusal stops there.
        let none = keep(&fact("squats"), &[OWNER.to_owned()], |_| {
            Some(json!({"ok": false, "error": "not_known"}))
        });
        assert_eq!(none.outcome, "not_known");
        let mut tries = 0;
        let stopped = keep(
            &fact("squats"),
            &[OWNER.to_owned(), FACE.to_owned()],
            |_| {
                tries += 1;
                Some(json!({"ok": false, "error": "too_many"}))
            },
        );
        assert_eq!((stopped.outcome.as_str(), tries), ("too_many", 1));
    }

    /// On a real bus with a memory that answers: a fact told on
    /// `apps/remember` is kept and said on `apps/remembered`.
    #[test]
    fn on_a_real_bus_a_fact_is_kept_and_said() {
        use std::io::{BufRead as _, Write as _};
        let base = std::env::temp_dir().join(format!("lv-remember-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let socket = base.join("m.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let (lines, asked) = std::sync::mpsc::channel::<Value>();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_ok() {
                    let line: Value = serde_json::from_str(&line).unwrap();
                    let answer = json!({"ok": true, "id": 1});
                    let _ = lines.send(line);
                    let mut stream = stream;
                    let _ = writeln!(stream, "{answer}");
                }
            }
        });
        fn owners() -> Vec<String> {
            vec![OWNER.to_owned()]
        }
        let bus = lamp_bus::Bus::new(base.join("bus")).unwrap();
        let mut said = bus.subscribe(AppRemembered::TOPIC, "test").unwrap();
        start(&base.join("bus"), socket.clone(), owners);
        let mut door = bus
            .publisher_declared(AppRemember::TOPIC, &AppRemember::declaration(), "control")
            .unwrap();
        // The writer may subscribe just after: said again until answered.
        let begun = std::time::Instant::now();
        let went = loop {
            door.publish_json(lamp_ipc::monotonic_us(), &fact("squats 3x10"))
                .unwrap();
            if let Ok(Some(message)) = said.recv(Duration::from_millis(200)) {
                break message.parse::<AppRemembered>().unwrap();
            }
            assert!(begun.elapsed() < Duration::from_secs(5), "not said");
        };
        assert_eq!(went.outcome, app_fact::KEPT);
        let line = asked.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(line["person"], OWNER);
        assert_eq!(line["source"], "app:gym-coach");
        let _ = std::fs::remove_dir_all(&base);
    }
}
