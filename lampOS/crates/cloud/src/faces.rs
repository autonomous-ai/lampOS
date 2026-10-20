//! The faces a lamp knows, and removing one, for the app's Lamp 2 page
//! (`face.list`, `face.remove`; Mobile on #1396, mobile-P3 item 3; #2192).
//! Removing is the privacy half of the photo faces (`faces.add_photo`,
//! `crate::photos`). Both go to the eyes over their request socket, as the
//! voice's own "who do you know?" and "forget my face" do: lamp-cloud keeps
//! nothing. A list is the names only, never a print or a photo, with a face
//! from a photo still waiting for its person's yes marked so (an ask only
//! lamp-cloud makes: the plain list never names those).
//!
//! A remove takes the person off all of the lamp (the first builder on
//! #2199, 13:08): the eyes' own forget (their prints, their familiarity and
//! their name in any set-aside record; a forget that waits for those says
//! so, #1586), then everything memory keeps for them and their rules, as the
//! voice's `forget_everything` asks it (`LAMP_MEMORY_SOCKET`). The answer
//! says whether memory went, so the app never says gone when it isn't.
//!
//! They come over the lamp's own account channel only, as `faces.add_photo`
//! does. Off unless `LAMP_FACES_PAGE=on`, with the eyes' request socket
//! (`LAMP_EYES_WHO_SOCKET`).
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};

pub const ENVIRONMENT: &str = "LAMP_FACES_PAGE";
/// Memory's request socket, as the voice names it.
pub const MEMORY_ENVIRONMENT: &str = "LAMP_MEMORY_SOCKET";
/// A name as the eyes keep one, at most.
pub const MOST_NAME_CHARS: usize = crate::photos::MOST_NAME_CHARS;
/// The eyes answer between two of the camera's looks; a forget rewrites the
/// people's file. Bounded short, as `faces.add_photo`'s, since the answer is
/// waited for on the runtime that keeps the broker's connection alive.
pub const DEADLINE: Duration = Duration::from_secs(2);

/// The eyes to ask, and memory, when its socket is given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Eyes {
    pub socket: PathBuf,
    pub memory: Option<PathBuf>,
}

impl Eyes {
    /// From the environment: `None` unless the switch is on and the eyes'
    /// socket's path is absolute; memory's only if absolute.
    pub fn from_environment() -> Option<Self> {
        Self::from(|name| std::env::var(name).ok())
    }

    pub fn from(var: impl Fn(&str) -> Option<String>) -> Option<Self> {
        (var(ENVIRONMENT).as_deref() == Some("on")).then_some(())?;
        let absolute = |name| {
            var(name)
                .map(PathBuf::from)
                .filter(|path: &PathBuf| path.is_absolute())
        };
        Some(Self {
            socket: absolute(crate::photos::SOCKET_ENVIRONMENT)?,
            memory: absolute(MEMORY_ENVIRONMENT),
        })
    }
}

/// The eyes' `{"people":[…],"waiting":[…]}` as the app's list: each name,
/// and whether it waits for its person's yes at the lamp. Anything else
/// in the eyes' answer is left out.
pub fn listed(answer: &Value) -> Result<Value, String> {
    let names = |key: &str| -> Vec<String> {
        answer[key]
            .as_array()
            .map(|names| {
                names
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    if !answer["people"].is_array() {
        return Err("the lamp's eyes could not say whom they know".to_owned());
    }
    let mut faces: Vec<Value> = names("people")
        .into_iter()
        .map(|name| json!({"name": name, "waiting": false}))
        .collect();
    faces.extend(
        names("waiting")
            .into_iter()
            .map(|name| json!({"name": name, "waiting": true})),
    );
    Ok(json!({"faces": faces}))
}

/// The app's `data` for a remove: `name`, checked as the eyes keep one.
pub fn asked(data: &Value) -> Result<String, String> {
    data.get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty() && name.chars().count() <= MOST_NAME_CHARS)
        .map(str::to_owned)
        .ok_or_else(|| "name must be a name of at most 40 characters".to_owned())
}

/// The eyes' answer to a forget, in the app's words.
pub fn removed(name: &str, answer: &Value) -> Result<Value, String> {
    let waits = answer["unsearched"].as_u64().unwrap_or(0) > 0;
    match answer["forgot"].as_bool() {
        // Some set-aside records could not be searched: the forget waits for
        // them, and the app says it's still finishing (#1586).
        Some(true) if waits => Ok(json!({"removed": name, "finishing": true})),
        Some(true) => Ok(json!({"removed": name})),
        Some(false) if waits => Ok(json!({"removed": name, "finishing": true})),
        Some(false) => Err("the lamp doesn't know that face".to_owned()),
        None => Err("the lamp's eyes could not forget that face".to_owned()),
    }
}

/// Memory's id for a person the eyes know by `name`, as the voice makes it
/// (`remember::person_id`): `p-` and the first 16 hex digits of SHA-256 of
/// the lower-cased name. Their rules are kept under `r-` and the same
/// digits (`rules::scope`).
pub fn person_id(name: &str) -> String {
    lamp_msgs::known::person_id(name)
}

/// The memory's requests for `name`: everything kept for the person, then
/// their rules, as the voice's `forget_everything` asks. `id` is the memory
/// id the eyes said they kept for them (`LAMP_MEMORY_STABLE_ID=on`, #2176):
/// a renamed person's memory is under their first name's id. The voice
/// keys on that id only when its own switch agrees with the eyes', else on
/// `person_id` of the name, so when the two may differ both are forgotten,
/// with both rule scopes, and nothing of the person is left half-removed.
///
/// But the by-name id goes only when it can be nobody else's (the
/// reviewer-lead's P2 on #2330): `name_id_free`, the eyes' word that no
/// other person they keep holds it. "Lan" renamed "Mai" keeps `p-` of
/// "Lan"; a later "Minh" renamed "Lan" removed must not take Mai's memory.
/// lamp-cloud can't know the voice's switch, so the eyes' word alone
/// decides. Without an id, `person_id` of the name alone, as before: the
/// name is then the only key there is.
pub fn memory_asks(name: &str, id: Option<&str>, name_id_free: bool) -> Vec<Value> {
    let by_name = person_id(name);
    let mut people = vec![];
    match id.filter(|id| lamp_msgs::known::is_person_id(id)) {
        Some(id) if id == by_name => people.push(by_name),
        Some(id) => {
            people.push(id.to_owned());
            if name_id_free {
                people.push(by_name);
            }
        }
        None => people.push(by_name),
    }
    people
        .iter()
        .flat_map(|person| {
            [
                json!({"op": "forget_all", "person": person}),
                json!({"op": "forget_all", "person": format!("r-{}", &person[2..])}),
            ]
        })
        .collect()
}

/// What memory's answers mean for the app: `forgotten` only when every
/// forget was done, else `kept` and the `reason`.
pub fn memory_told(answers: &[Option<Value>]) -> Value {
    if answers
        .iter()
        .all(|answer| answer.as_ref().is_some_and(|answer| answer["ok"] == true))
    {
        json!({"memory": "forgotten"})
    } else if answers.iter().any(Option::is_none) {
        json!({"memory": "kept", "reason": "the lamp's memory could not be reached"})
    } else {
        json!({"memory": "kept", "reason": "the lamp's memory refused to forget"})
    }
}

/// `face.list`: the names the eyes keep, and those that wait for a yes.
pub fn list(eyes: &Eyes) -> Result<Value, String> {
    let ask = json!({"list": true, "waiting": true});
    match crate::photos::exchange(&eyes.socket, &ask, DEADLINE) {
        Some(answer) => listed(&answer),
        None => Err("the lamp's eyes did not answer".to_owned()),
    }
}

/// `face.remove`: the eyes' own forget of `data`'s name, then memory's of
/// everything it keeps for them and their rules.
pub fn remove(eyes: &Eyes, data: &Value) -> Result<Value, String> {
    let name = asked(data)?;
    let (mut told, id, name_id_free) =
        match crate::photos::exchange(&eyes.socket, &json!({"forget": name}), DEADLINE) {
            Some(answer) => {
                let id = answer["id"].as_str().map(str::to_owned);
                // Eyes that say no word (an older build) said no id either:
                // the name is then theirs alone.
                let free = answer["name_id_free"].as_bool().unwrap_or(id.is_none());
                (removed(&name, &answer)?, id, free)
            }
            None => return Err("the lamp's eyes did not answer".to_owned()),
        };
    let memory = match &eyes.memory {
        Some(socket) => {
            let answers: Vec<Option<Value>> = memory_asks(&name, id.as_deref(), name_id_free)
                .iter()
                .map(|ask| crate::photos::exchange(socket, ask, DEADLINE))
                .collect();
            memory_told(&answers)
        }
        None => json!({"memory": "kept", "reason": "the lamp's memory isn't given to lamp-cloud"}),
    };
    if let (Some(told), Some(memory)) = (told.as_object_mut(), memory.as_object()) {
        told.extend(memory.clone());
    }
    Ok(told)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_is_off_unless_switched_on_with_the_eyes_socket() {
        let env = |on: &'static str, socket: &'static str| {
            move |name: &str| match name {
                ENVIRONMENT => Some(on.to_owned()),
                crate::photos::SOCKET_ENVIRONMENT => Some(socket.to_owned()),
                _ => None,
            }
        };
        assert_eq!(
            Eyes::from(env("on", "/run/lampos-eyes/who.sock")),
            Some(Eyes {
                socket: "/run/lampos-eyes/who.sock".into(),
                memory: None,
            })
        );
        assert_eq!(Eyes::from(env("off", "/run/lampos-eyes/who.sock")), None);
        assert_eq!(Eyes::from(env("on", "who.sock")), None);
        assert_eq!(Eyes::from(|_| None), None);
    }

    #[test]
    fn the_list_is_names_and_whether_each_waits_for_a_yes() {
        let told = listed(&json!({"people": ["Lan", "Minh"], "waiting": ["Bo"]})).unwrap();
        assert_eq!(
            told,
            json!({"faces": [
                {"name": "Lan", "waiting": false},
                {"name": "Minh", "waiting": false},
                {"name": "Bo", "waiting": true},
            ]})
        );
        // Eyes without photos say no `waiting`; nobody known is an empty list.
        assert_eq!(
            listed(&json!({"people": []})).unwrap(),
            json!({"faces": []})
        );
        // Only names: anything else the eyes might say is left out.
        let told = listed(&json!({"people": ["Lan"], "prints": [[0.1]]})).unwrap();
        assert_eq!(told, json!({"faces": [{"name": "Lan", "waiting": false}]}));
        assert!(listed(&json!({"error": "storage"})).is_err());
    }

    #[test]
    fn a_remove_names_one_and_says_how_the_forget_went() {
        assert_eq!(asked(&json!({"name": " Lan "})).unwrap(), "Lan");
        for wrong in [
            json!({}),
            json!({"name": ""}),
            json!({"name": 7}),
            json!({"name": "x".repeat(41)}),
        ] {
            assert!(asked(&wrong).is_err(), "{wrong}");
        }
        assert_eq!(
            removed("Lan", &json!({"forgot": true})).unwrap(),
            json!({"removed": "Lan"})
        );
        assert_eq!(
            removed("Lan", &json!({"forgot": true, "unsearched": 1})).unwrap(),
            json!({"removed": "Lan", "finishing": true})
        );
        // Not found in what could be searched, but a set-aside record waits.
        assert_eq!(
            removed("Lan", &json!({"forgot": false, "unsearched": 2})).unwrap(),
            json!({"removed": "Lan", "finishing": true})
        );
        assert_eq!(
            removed("Lan", &json!({"forgot": false})),
            Err("the lamp doesn't know that face".to_owned())
        );
        assert!(removed("Lan", &json!({"error": "storage"})).is_err());
    }

    /// Over the socket: the eyes are asked as the voice asks them, and
    /// nothing answering is said.
    #[test]
    fn the_eyes_are_asked_over_their_socket() {
        use std::io::{BufRead, BufReader, Write};
        let dir = std::env::temp_dir().join(format!("cloud-faces-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("who.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let eyes_side = std::thread::spawn(move || {
            let mut asked = Vec::new();
            for answer in [
                json!({"people": ["Lan"], "waiting": ["Bo"]}),
                json!({"forgot": true}),
            ] {
                let (stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                asked.push(serde_json::from_str::<Value>(line.trim()).unwrap());
                writeln!(&stream, "{answer}").unwrap();
            }
            asked
        });
        let eyes = Eyes {
            socket: socket.clone(),
            memory: None,
        };
        assert_eq!(
            list(&eyes).unwrap(),
            json!({"faces": [{"name": "Lan", "waiting": false}, {"name": "Bo", "waiting": true}]})
        );
        assert_eq!(
            remove(&eyes, &json!({"name": "Lan"})).unwrap(),
            json!({"removed": "Lan", "memory": "kept", "reason": "the lamp's memory isn't given to lamp-cloud"})
        );
        assert_eq!(
            eyes_side.join().unwrap(),
            [
                json!({"list": true, "waiting": true}),
                json!({"forget": "Lan"})
            ]
        );
        std::fs::remove_file(&socket).unwrap();
        assert_eq!(
            list(&eyes),
            Err("the lamp's eyes did not answer".to_owned())
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Memory's id is the voice's (`remember::person_id`): "Lan" is
    /// `p-094a367b026246fb`, whatever the case or the spaces, and their rules
    /// are `r-` and the same digits. The voice's test pins the same value,
    /// so neither copy changes alone.
    #[test]
    fn memory_is_asked_for_the_persons_own_id_and_their_rules() {
        assert_eq!(person_id("Lan"), "p-094a367b026246fb");
        assert_eq!(person_id(" lAN "), "p-094a367b026246fb");
        assert_eq!(
            memory_asks("Lan", None, true),
            [
                json!({"op": "forget_all", "person": "p-094a367b026246fb"}),
                json!({"op": "forget_all", "person": "r-094a367b026246fb"}),
            ]
        );
        // The eyes said the id they kept (#2176): a renamed Lan, first Minh,
        // has Minh's memory forgotten, never her new name's.
        let minh = person_id("Minh");
        assert_eq!(
            memory_asks("Lan", Some(&minh), true),
            [
                json!({"op": "forget_all", "person": minh}),
                json!({"op": "forget_all", "person": format!("r-{}", &minh[2..])}),
                json!({"op": "forget_all", "person": "p-094a367b026246fb"}),
                json!({"op": "forget_all", "person": "r-094a367b026246fb"}),
            ]
        );
        // Someone else holds the name's own id (Mai, renamed from Lan, #2330):
        // only the eyes' id and its rules are asked.
        assert_eq!(
            memory_asks("Lan", Some(&minh), false),
            [
                json!({"op": "forget_all", "person": minh}),
                json!({"op": "forget_all", "person": format!("r-{}", &minh[2..])}),
            ]
        );
        // No id said: the name is the only key there is.
        assert_eq!(
            memory_asks("Lan", None, false),
            memory_asks("Lan", None, true)
        );
        assert_eq!(
            memory_asks("Lan", Some("../x"), true),
            memory_asks("Lan", None, true)
        );
        // The eyes' id is the name's own (never renamed): asked once.
        assert_eq!(
            memory_asks("Lan", Some("p-094a367b026246fb"), true),
            memory_asks("Lan", None, true)
        );
    }

    /// The app is never told memory went when it didn't.
    #[test]
    fn memory_is_said_forgotten_only_when_every_forget_was_done() {
        let ok = Some(json!({"ok": true, "removed": 3}));
        assert_eq!(
            memory_told(&[ok.clone(), ok.clone()]),
            json!({"memory": "forgotten"})
        );
        assert_eq!(
            memory_told(&[ok.clone(), None]),
            json!({"memory": "kept", "reason": "the lamp's memory could not be reached"})
        );
        assert_eq!(
            memory_told(&[ok, Some(json!({"ok": false, "error": "storage"}))]),
            json!({"memory": "kept", "reason": "the lamp's memory refused to forget"})
        );
    }

    /// A remove with memory given: the eyes forget, then memory is asked
    /// for the person and their rules, and the answer says it went.
    #[test]
    fn a_remove_takes_the_person_off_the_eyes_and_memory() {
        use std::io::{BufRead, BufReader, Write};
        let dir = std::env::temp_dir().join(format!("cloud-faces-memory-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let answering = |socket: std::path::PathBuf, answers: Vec<Value>| {
            let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
            std::thread::spawn(move || {
                let mut asked = Vec::new();
                for answer in answers {
                    let (stream, _) = listener.accept().unwrap();
                    let mut line = String::new();
                    BufReader::new(&stream).read_line(&mut line).unwrap();
                    asked.push(serde_json::from_str::<Value>(line.trim()).unwrap());
                    writeln!(&stream, "{answer}").unwrap();
                }
                asked
            })
        };
        let (who, memory) = (dir.join("who.sock"), dir.join("memory.sock"));
        let eyes_side = answering(who.clone(), vec![json!({"forgot": true})]);
        let memory_side = answering(
            memory.clone(),
            vec![
                json!({"ok": true, "removed": 4}),
                json!({"ok": true, "removed": 0}),
            ],
        );
        let eyes = Eyes {
            socket: who,
            memory: Some(memory),
        };
        assert_eq!(
            remove(&eyes, &json!({"name": "Lan"})).unwrap(),
            json!({"removed": "Lan", "memory": "forgotten"})
        );
        assert_eq!(eyes_side.join().unwrap(), [json!({"forget": "Lan"})]);
        assert_eq!(memory_side.join().unwrap(), memory_asks("Lan", None, true));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A remove of "Lan" against eyes that answer `told`, and a memory that
    /// keeps the scopes `kept`: what memory keeps after.
    fn kept_after_removing_lan(
        label: &str,
        told: Value,
        kept: std::collections::HashSet<String>,
    ) -> std::collections::HashSet<String> {
        use std::io::{BufRead, BufReader, Write};
        let dir = std::env::temp_dir().join(format!("cloud-faces-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (who, memory) = (dir.join("who.sock"), dir.join("memory.sock"));
        let eyes_listener = std::os::unix::net::UnixListener::bind(&who).unwrap();
        let eyes_side = std::thread::spawn(move || {
            let (stream, _) = eyes_listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            writeln!(&stream, "{told}").unwrap();
        });
        let memory_listener = std::os::unix::net::UnixListener::bind(&memory).unwrap();
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        let memory_side = std::thread::spawn(move || {
            let mut kept = kept;
            memory_listener.set_nonblocking(true).unwrap();
            let until = std::time::Instant::now() + Duration::from_secs(5);
            while std::time::Instant::now() < until && done_rx.try_recv().is_err() {
                match memory_listener.accept() {
                    Ok((stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        let mut line = String::new();
                        BufReader::new(&stream).read_line(&mut line).unwrap();
                        let ask: Value = serde_json::from_str(line.trim()).unwrap();
                        let gone = kept.remove(ask["person"].as_str().unwrap());
                        writeln!(
                            &stream,
                            "{}",
                            json!({"ok": true, "removed": u8::from(gone)})
                        )
                        .unwrap();
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
            kept
        });
        let eyes = Eyes {
            socket: who,
            memory: Some(memory),
        };
        assert_eq!(
            remove(&eyes, &json!({"name": "Lan"})).unwrap(),
            json!({"removed": "Lan", "memory": "forgotten"})
        );
        done_tx.send(()).unwrap();
        eyes_side.join().unwrap();
        let left = memory_side.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
        left
    }

    fn with_rules(ids: &[&str]) -> std::collections::HashSet<String> {
        ids.iter()
            .flat_map(|id| [(*id).to_owned(), format!("r-{}", &id[2..])])
            .collect()
    }

    /// Eyes on, voice off (#2176): Minh was renamed Lan, the eyes keep
    /// Minh's id and say it, while the voice, its switch off, kept Lan's
    /// memory under `p-` of "Lan". Nobody else holds that, so the remove
    /// forgets both, and their rules: nothing of hers is left.
    #[test]
    fn a_remove_with_the_eyes_on_and_the_voice_off_leaves_nothing_half_removed() {
        let (minh, lan) = (person_id("Minh"), person_id("Lan"));
        let told = json!({"forgot": true, "id": minh, "name_id_free": true});
        let left = kept_after_removing_lan("mixed", told, with_rules(&[&lan, &minh]));
        assert!(left.is_empty(), "something was left: {left:?}");
    }

    /// The reviewer-lead's chain on #2330: "Lan" renamed "Mai" keeps `p-` of
    /// "Lan"; then "Minh" renamed "Lan" keeps `p-` of "Minh". Removing "Lan"
    /// forgets `p-` of "Minh" and its rules only: the eyes say the name's id
    /// is held, and Mai's memory and rules stay.
    #[test]
    fn a_remove_never_takes_the_memory_of_someone_renamed_from_the_name() {
        let (minh, lan) = (person_id("Minh"), person_id("Lan"));
        let told = json!({"forgot": true, "id": minh, "name_id_free": false});
        let left = kept_after_removing_lan("chain", told, with_rules(&[&lan, &minh]));
        assert_eq!(left, with_rules(&[&lan]), "Mai's memory and rules stay");
    }
}
