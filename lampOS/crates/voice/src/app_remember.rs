//! The writer of an installed app's facts (#7415, C166, `docs/apps.md`,
//! "What an app tells Lamp's memory"). lamp-control checks an app's
//! `apps.remember` (its grant, its words, the lamp not private, the rate,
//! `LAMP_APP_REMEMBER`) and tells it here on `apps/remember`; the voice picks
//! whose it is, the lamp's known person by the owner's memory ids
//! (`lamp_msgs::app_fact::whose` with no person asked: never another person,
//! never the room), sends memory's `remember_app_fact` with the app as its
//! source (`app:<name>`), and says how it went on `apps/remembered`. A
//! removed app (`apps/forget_facts`) has every fact of its source forgotten
//! (`forget_app_facts`).
//!
//! No switch of its own: nothing is told here unless lamp-control's
//! `LAMP_APP_REMEMBER` is on (a remove's forget aside, which takes only
//! what an app kept). The journal says the app and how it went, never a
//! word of the fact.
use lamp_msgs::app_fact::{self, AppFactsForget, AppRemember, AppRemembered};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long memory may take with one fact: inside lamp-control's 2.5 s wait.
const MEMORY_WITHIN: Duration = Duration::from_millis(1_500);
/// How long memory may take with a removed app's forget.
const FORGET_WITHIN: Duration = Duration::from_secs(5);

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

/// Memory's request that forgets every fact of the removed app.
pub fn forget_line(gone: &AppFactsForget) -> Option<Value> {
    let source = gone.source()?;
    Some(json!({"op": app_fact::FORGET_OP, "source": source.as_str()}))
}

/// How `fact` went, for the lamp's known person among `owners` (the
/// owner's memory ids, `person::owner_ids`), `memory` asked with one line
/// (none: not reached or no answer in time).
pub fn keep(
    fact: &AppRemember,
    owners: &[String],
    memory: impl FnOnce(&Value) -> Option<Value>,
) -> AppRemembered {
    let went = |outcome: &str| AppRemembered {
        ask: fact.ask,
        app: fact.app.clone(),
        outcome: outcome.to_owned(),
    };
    if fact.check().is_err() {
        return went("bad_request");
    }
    let person = match app_fact::whose(None, owners) {
        Ok(person) => person,
        Err(refused) => return went(refused.code()),
    };
    let Some(line) = remember_line(fact, &person) else {
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
    went(code)
}

/// Starts the writer on the bus at `root`, memory at `memory`: one thread
/// for the facts and one for the removed apps' forgets. `owners` gives the
/// owner's memory ids at each fact. A part that can't start is said.
pub fn start(root: &Path, memory: PathBuf, owners: fn() -> Vec<String>) {
    let Ok(bus) = lamp_bus::Bus::new(root) else {
        eprintln!("lamp-voice: no app's fact is kept: the bus can't be reached");
        return;
    };
    let facts = bus.subscribe(AppRemember::TOPIC, "voice");
    let said = bus.publisher_declared(AppRemembered::TOPIC, &AppRemembered::declaration(), "voice");
    match (facts, said) {
        (Ok(mut facts), Ok(mut said)) => {
            let memory = memory.clone();
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
    match bus.subscribe(AppFactsForget::TOPIC, "voice") {
        Ok(mut gone) => {
            let spawned = std::thread::Builder::new()
                .name("app-facts-forget".into())
                .spawn(move || {
                    loop {
                        match gone.recv(Duration::from_secs(1)) {
                            Ok(Some(message)) => {
                                let Some((app, line)) = message
                                    .parse::<AppFactsForget>()
                                    .ok()
                                    .and_then(|gone| Some((gone.app.clone(), forget_line(&gone)?)))
                                else {
                                    continue;
                                };
                                let removed = crate::tools::exchange(&memory, &line, FORGET_WITHIN)
                                    .filter(|answer| answer.get("ok") == Some(&json!(true)))
                                    .and_then(|answer| answer.get("removed").and_then(Value::as_u64));
                                match removed {
                                    Some(removed) => eprintln!(
                                        "lamp-voice: app {app} removed: {removed} of its facts forgotten"
                                    ),
                                    None => eprintln!(
                                        "lamp-voice: app {app} removed: its facts could not be forgotten now"
                                    ),
                                }
                            }
                            Ok(None) => {}
                            Err(error) => {
                                eprintln!("lamp-voice: apps/forget_facts can't be read: {error}");
                                std::thread::sleep(Duration::from_secs(1));
                            }
                        }
                    }
                });
            if let Err(error) = spawned {
                eprintln!("lamp-voice: a removed app's facts are not forgotten: {error}");
            }
        }
        Err(error) => eprintln!("lamp-voice: a removed app's facts are not forgotten: {error}"),
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
        let with = |answer: Option<Value>| keep(&fact("squats"), &owners, |_| answer).outcome;
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

    #[test]
    fn a_removed_apps_facts_are_forgotten_by_its_source() {
        assert_eq!(
            forget_line(&AppFactsForget {
                app: "gym-coach".into()
            }),
            Some(json!({"op": "forget_app_facts", "source": "app:gym-coach"}))
        );
        assert_eq!(forget_line(&AppFactsForget { app: "a:b".into() }), None);
    }

    /// On a real bus with a memory that answers: a fact told on
    /// `apps/remember` is kept and said on `apps/remembered`; a removed app's
    /// forget reaches memory.
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
                    let answer = if line["op"] == app_fact::FORGET_OP {
                        json!({"ok": true, "removed": 2})
                    } else {
                        json!({"ok": true, "id": 1})
                    };
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
        let mut forget = bus
            .publisher_declared(
                AppFactsForget::TOPIC,
                &AppFactsForget::declaration(),
                "control",
            )
            .unwrap();
        forget
            .publish_json(
                lamp_ipc::monotonic_us(),
                &AppFactsForget {
                    app: "gym-coach".into(),
                },
            )
            .unwrap();
        let begun = std::time::Instant::now();
        loop {
            let line = asked.recv_timeout(Duration::from_secs(5)).unwrap();
            if line["op"] == app_fact::FORGET_OP {
                assert_eq!(line["source"], "app:gym-coach");
                break;
            }
            assert!(begun.elapsed() < Duration::from_secs(5));
        }
        let _ = std::fs::remove_dir_all(&base);
    }
}
