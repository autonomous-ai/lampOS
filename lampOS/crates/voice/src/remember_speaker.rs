//! Request-local evidence for saving only the face seen speaking the request.
//! No model argument can supply this proof. Eyes independently checks its
//! image captured during the same interval and its final write permission.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

#[path = "remember_speaker/consent.rs"]
pub mod consent;

pub const ENVIRONMENT: &str = "LAMP_REMEMBER_SPEAKER";
pub const FLAG: &str = "--remember-speaker";
pub const FRESH_US: u64 = 30_000_000;
pub const HEARD_BYTES: usize = 4_096;
pub const FACE_BYTES: usize = 128;
pub const NOT_SEEN: &str = "I could not be sure which face said that request, so I have not remembered anyone. They should look at me and say their name again.";

worker_global! {
    fn epoch() -> AtomicU64 = AtomicU64::new(0);
}

/// Captured with the request, so a transport thread reads the originating
/// worker's epoch even when tests run several workers in one process.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Generation {
    epoch: &'static AtomicU64,
    value: u64,
}

impl Generation {
    pub(crate) fn same_epoch(self, other: Self) -> bool {
        std::ptr::eq(self.epoch, other.epoch)
    }
}

pub fn switched_on(value: Option<&str>) -> bool {
    value == Some("on")
}

pub(crate) fn generation() -> Generation {
    Generation {
        epoch: epoch(),
        value: epoch().load(Ordering::Acquire),
    }
}

/// Invalidates both stored proofs and one being collected while speech to
/// text runs. An in-flight exchange observes this before its next I/O.
pub fn forget() {
    consent::clear();
    let generation = generation();
    generation.epoch.fetch_add(1, Ordering::AcqRel);
    crate::speaking_faces::clear_enrollment(generation);
}

/// Private runtime evidence, never a tool parameter or persisted record.
#[derive(Clone, Debug)]
pub struct Proof {
    pub(crate) face: String,
    pub(crate) began_us: u64,
    pub(crate) ended_us: u64,
    pub(crate) heard: String,
    pub(crate) generation: Generation,
    active: Arc<AtomicBool>,
    pub(crate) consent: Arc<Mutex<Option<crate::memory_scope::RequestKey>>>,
}

impl PartialEq for Proof {
    fn eq(&self, other: &Self) -> bool {
        self.face == other.face
            && self.began_us == other.began_us
            && self.ended_us == other.ended_us
            && self.heard == other.heard
            && self.generation.value == other.generation.value
            && self.generation.same_epoch(other.generation)
            && Arc::ptr_eq(&self.active, &other.active)
    }
}

impl Proof {
    pub(crate) fn heard(
        face: String,
        began_us: u64,
        ended_us: u64,
        heard: &str,
        generation: Generation,
    ) -> Option<Self> {
        // Refuse, never truncate: a later correction must not disappear
        // merely because the name appeared near the beginning.
        if heard.len() > HEARD_BYTES || face.len() > FACE_BYTES {
            return None;
        }
        let proof = Self {
            face,
            began_us,
            ended_us,
            heard: heard.to_owned(),
            generation,
            active: Arc::new(AtomicBool::new(true)),
            consent: Arc::new(Mutex::new(None)),
        };
        proof.is_current(lamp_ipc::monotonic_us()).then_some(proof)
    }

    pub(crate) fn is_current(&self, now_us: u64) -> bool {
        self.active.load(Ordering::Acquire)
            && self.heard.len() <= HEARD_BYTES
            && self.face.len() <= FACE_BYTES
            && self.generation.value == self.generation.epoch.load(Ordering::Acquire)
            && self.began_us < self.ended_us
            && self.ended_us - self.began_us <= FRESH_US
            && now_us >= self.ended_us
            && now_us - self.ended_us <= FRESH_US
            && self.heard.chars().any(char::is_alphanumeric)
            && self.face.split_once('/').is_some_and(|(boot, face)| {
                !boot.is_empty() && face.starts_with('f') && face.len() > 1
            })
    }

    pub(crate) fn cancel(&self) {
        self.active.store(false, Ordering::Release);
    }
}

#[cfg(test)]
pub(crate) use tests::{TESTS as TEST_CONSENT, exchange_proof as test_consent_exchange};

#[cfg(test)]
#[path = "remember_speaker/consent_flow_tests.rs"]
mod consent_flow_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{known_faces, memory_scope, speaking_faces, tools};
    use lamp_gemini::ToolCall;
    use serde_json::{Value, json};
    use std::{
        io::{BufRead, BufReader, Read, Write},
        os::unix::net::UnixListener,
        path::Path,
        sync::{Mutex, mpsc},
        time::{Duration, Instant},
    };

    // These tests deliberately invalidate the process's enrollment epoch.
    // Keep those actions apart from each other's otherwise independent proof.
    pub(crate) static TESTS: Mutex<()> = Mutex::new(());
    const REQUEST: u64 = 3_298_500;

    fn proof(words: &str) -> Proof {
        let now = lamp_ipc::monotonic_us();
        Proof::heard("eyes/f1".into(), now - 1_000_000, now, words, generation()).unwrap()
    }

    fn seen(proof: Proof) -> speaking_faces::LineSeen {
        speaking_faces::LineSeen {
            request: REQUEST,
            enrollment: Some(proof),
            ..Default::default()
        }
    }

    fn owner() -> memory_scope::RequestScope {
        memory_scope::RequestScope::new(
            memory_scope::RequestKey {
                worker: lamp_turn_owner::BootId::new([78; 16]).unwrap(),
                session: 91,
                request: REQUEST,
            },
            memory_scope::ScopeSnapshot::room(),
        )
    }

    fn switches() -> known_faces::FaceSwitches {
        known_faces::FaceSwitches {
            checked: true,
            remember_speaker: true,
            ..Default::default()
        }
    }

    fn call(args: Value) -> ToolCall {
        ToolCall {
            id: "enroll".into(),
            name: known_faces::REMEMBER.into(),
            args,
        }
    }

    fn run(
        socket: &Path,
        call: &ToolCall,
        proof: Option<&speaking_faces::LineSeen>,
        lease: Option<&memory_scope::ScopeLease>,
    ) -> (Value, bool) {
        known_faces::run_scoped(socket, call, switches(), None, proof, REQUEST, lease)
    }

    /// A real Eyes socket that records even an unwanted connection. The
    /// response makes a broken refusal finish promptly rather than timing out.
    fn own_name_exchange(
        words: &str,
        name: &str,
        apart: bool,
        guarded: bool,
        reply: Value,
    ) -> (Value, Vec<Value>) {
        let owner = owner();
        exchange_proof(
            proof(words),
            owner.lease().key(),
            json!({"name": name, "apart": apart}),
            guarded,
            reply,
        )
    }

    /// Use the actual scoped tool, request lease, JSON writer and socket reader.
    /// In particular, this does not write the proof's private consent token.
    pub(crate) fn exchange_proof(
        proof: Proof,
        key: memory_scope::RequestKey,
        arguments: Value,
        guarded: bool,
        reply: Value,
    ) -> (Value, Vec<Value>) {
        let home = crate::process::SessionDirectory::create().unwrap();
        let socket = home.path.join("eyes.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let (done, finished) = mpsc::channel();
        let peer = std::thread::spawn(move || {
            let mut requests = Vec::new();
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        // macOS inherits the listener's nonblocking mode.
                        // The existing socket read timeout needs blocking I/O.
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let mut line = String::new();
                        BufReader::new(stream.try_clone().unwrap())
                            .read_line(&mut line)
                            .unwrap();
                        requests.push(serde_json::from_str(&line).unwrap());
                        writeln!(stream, "{reply}").unwrap();
                    }
                    Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock),
                }
                match finished.recv_timeout(Duration::from_millis(1)) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return requests,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        });
        let owner = memory_scope::RequestScope::new(key, memory_scope::ScopeSnapshot::room());
        let line = speaking_faces::LineSeen {
            request: key.request,
            enrollment: Some(proof),
            ..Default::default()
        };
        let answer = known_faces::run_scoped(
            &socket,
            &call(arguments),
            if guarded {
                switches()
            } else {
                known_faces::FaceSwitches::default()
            },
            None,
            Some(&line),
            key.request,
            Some(&owner.lease()),
        )
        .0;
        done.send(()).unwrap();
        (answer, peer.join().unwrap())
    }

    #[test]
    fn remember_speaker_own_name_refuses_other_people_before_connect() {
        let _serial = TESTS.lock().unwrap_or_else(|poison| poison.into_inner());
        let mut sent = Vec::new();
        for words in [
            "Remember her face, she's Kess.",
            "This is my friend Kess, remember her face.",
            "Remember my son's face, his name is Kess.",
            "I'm Alex. Remember her face, she's Kess.",
            "My name is Alex and her name is Kess.",
            "My name is Mary. Her name is Ann. Remember her face.",
        ] {
            let name = if words.contains("Mary") {
                "Mary Ann"
            } else {
                "Kess"
            };
            let (answer, requests) =
                own_name_exchange(words, name, false, true, json!({"remembered": name}));
            if !requests.is_empty() || answer.get("error").is_none() {
                sent.push((words, requests, answer));
            }
        }
        assert!(
            sent.is_empty(),
            "Other people's names reached Eyes: {sent:?}"
        );
    }

    #[test]
    fn remember_speaker_own_name_refuses_reported_quoted_negated_or_ambiguous_lines() {
        let _serial = TESTS.lock().unwrap_or_else(|poison| poison.into_inner());
        let mut sent = Vec::new();
        for words in [
            "She said I'm Kess.",
            "She told me my name is Kess.",
            "He said: My name is Kess.",
            "\"My name is Kess.\"",
            "'I'm Kess.'",
            "She said “call me Kess.”",
            "I'm not Kess.",
            "My name is not Kess.",
            "Don't call me Kess.",
            "Never call me Kess.",
            "My name is Kess, no, that's wrong.",
            "If I say my name is Kess, will you remember it?",
            "Pretend I'm Kess.",
            "I'm Kess?",
            "Do you know what my name is, Kess?",
            "It's Kess.",
            "This is Kess.",
            "Hi Kess.",
            "Hello, Kess!",
            "Hey Kess.",
            "Okay, Kess.",
            "Yes, Kess.",
            "Please, Kess.",
            "Kess, please.",
            "My name is Kesh, hers is spelled K E S S.",
            "My name is Kess. Her name is Kess too.",
            "Remember my face, I'm Kess, and this is Mai.",
            "Remember me please, I'm Kess.",
            "Im Kess.",
            "Anh là Kess.",
            "Chị là Kess.",
            "Remember my face, Kess.",
            "Kess, please remember my face.",
            "Remember my face, K E S S.",
            "Remember my face. I'm Kess?",
            "I'm Kess? Remember my face.",
            "Can you remember my face? I'm Kess?",
            "K E S S, three letters.",
        ] {
            let (answer, requests) =
                own_name_exchange(words, "Kess", true, true, json!({"remembered": "Kess"}));
            if !requests.is_empty() || answer.get("error").is_none() {
                sent.push((words, requests, answer));
            }
        }
        assert!(
            sent.is_empty(),
            "Unproven declarations reached Eyes: {sent:?}"
        );
    }

    #[test]
    fn remember_speaker_own_name_accepts_direct_names_and_exact_spelling() {
        let _serial = TESTS.lock().unwrap_or_else(|poison| poison.into_inner());
        for (name, words, spelled) in [
            ("Kesh", "My name is Kesh, remember my face.", false),
            ("Kesh", "Lamp, remember my face, I’m Kesh.", false),
            ("Kesh", "Please remember my face. My name is Kesh.", false),
            ("Kesh", "Can you remember my face? I'm Kesh.", false),
            (
                "Kesh",
                "Could you remember my face? My name is Kesh.",
                false,
            ),
            ("Kesh", "I'm Kesh, can you remember my face?", false),
            ("Kesh", "My name is Kesh, please remember my name.", false),
            (
                "Kesh",
                "My name is Kesh and remember my face, please.",
                false,
            ),
            ("Kesh", "I'm Kesh.", false),
            ("Kesh", "I am Kesh.", false),
            ("Kesh", "I am called Kesh.", false),
            ("Kesh", "I'm called Kesh.", false),
            ("Kesh", "Call me Kesh.", false),
            ("Kesh", "You can call me Kesh.", false),
            ("Kesh", "Can you call me Kesh?", false),
            ("Kesh", "Could you remember that my name is Kesh?", false),
            ("Kesh", "Yes, my name is Kesh.", false),
            ("Kesh", "Hey Lamp, my name's Kesh.", false),
            ("Kesh", "Kesh.", false),
            ("Mary Ann", "My name is Mary Ann.", false),
            ("May", "My name is May.", false),
            ("Will", "Will.", false),
            ("O'Neill", "Hi, I'm O’Neill.", false),
            ("Minh", "Tên tôi là Minh.", false),
            ("Phương", "Tôi là Phuong.", false),
            ("Đức", "I'm Đu\u{31b}\u{301}c.", false),
            ("Kesh", "K E S H.", true),
            ("Kesh", "My name is K, E, S, H.", true),
            ("Kesh", "K E S H, four letters.", true),
            ("Kesh", "My name is Kesh. K E S H. Four letters.", true),
            ("Kesh", "My name is Kesh, spelled K E S H.", true),
            ("Mary Ann", "M A R Y A N N.", true),
            ("Minh", "M I N H.", true),
            ("Said", "S A I D.", true),
            ("Ann", "A, double N.", true),
            ("Wu", "Double U, U.", true),
        ] {
            let (answer, requests) =
                own_name_exchange(words, name, true, true, json!({"remembered": name}));
            // The original explicit face requests remain positive. The other
            // rows still prove their name grammar, without treating a name
            // request or a bare introduction as biometric consent.
            let explicit_face_request = [
                "My name is Kesh, remember my face.",
                "Lamp, remember my face, I’m Kesh.",
                "Please remember my face. My name is Kesh.",
                "Can you remember my face? I'm Kesh.",
                "Could you remember my face? My name is Kesh.",
                "I'm Kesh, can you remember my face?",
                "My name is Kesh and remember my face, please.",
            ]
            .contains(&words);
            assert_eq!(
                known_faces::asks_to_save_face(name, words),
                explicit_face_request,
                "{words}"
            );
            if explicit_face_request {
                assert_eq!(answer, json!({"ok": true, "remembered": name}), "{words}");
                assert_eq!(requests.len(), 1, "{words}");
                assert_eq!(
                    requests[0]["remember_speaker_consented"]["name"], name,
                    "{words}"
                );
                assert_eq!(
                    requests[0]["remember_speaker_consented"]["apart"], spelled,
                    "{words}"
                );
            } else {
                // Every original name/spelling row still reaches the consent
                // fence after its name checks, but a name is not a face yes.
                assert!(requests.is_empty(), "{words}: {requests:?}");
                assert!(
                    answer["error"].as_str().is_some_and(
                        |error| error.starts_with("They have not agreed to saving their face")
                    ),
                    "{words}: {answer}"
                );
            }
        }
    }

    #[test]
    fn remember_speaker_own_name_refuses_folded_delimiters_and_sentence_arguments() {
        let _serial = TESTS.lock().unwrap_or_else(|poison| poison.into_inner());
        for (name, words) in [
            ("Kess", "「I'm Kess.」"),
            ("Kess", "‹My name is Kess.›"),
            ("Kess", "I'm Kess？"),
            ("Kess", "Call me Kess??"),
            ("Not Kess", "I'm not Kess."),
            ("Maybe Kess", "My name is maybe Kess."),
            ("Her Name Is Kess", "Her name is Kess."),
            ("Kess She Said", "Kess, she said."),
        ] {
            let (answer, requests) =
                own_name_exchange(words, name, false, true, json!({"remembered": name}));
            assert!(requests.is_empty(), "{name}: {words}: {requests:?}");
            assert!(answer.get("error").is_some(), "{name}: {words}: {answer}");
        }
    }

    #[test]
    fn remember_speaker_own_name_caps_raw_candidate_bytes_before_trimming() {
        let _serial = TESTS.lock().unwrap_or_else(|poison| poison.into_inner());
        for name in [
            format!("{}Kesh", " ".repeat(HEARD_BYTES)),
            format!("{}Kesh", "\u{2003}".repeat(HEARD_BYTES / 3 + 1)),
        ] {
            let (answer, requests) = own_name_exchange(
                "Please remember my face. My name is Kesh.",
                &name,
                false,
                true,
                json!({"remembered": "Kesh"}),
            );
            assert!(requests.is_empty());
            assert!(answer.get("error").is_some(), "{answer}");
        }
        let name = format!("{}Kesh", " ".repeat(HEARD_BYTES - "Kesh".len()));
        let (answer, requests) = own_name_exchange(
            "Please remember my face. My name is Kesh.",
            &name,
            false,
            true,
            json!({"remembered": "Kesh"}),
        );
        assert_eq!(answer, json!({"ok": true, "remembered": "Kesh"}));
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["remember_speaker_consented"]["name"], "Kesh");
    }

    #[test]
    fn remember_speaker_own_name_known_as_does_not_disclose_or_offer_replacement() {
        let _serial = TESTS.lock().unwrap_or_else(|poison| poison.into_inner());
        let (answer, requests) = own_name_exchange(
            "Please remember my face. My name is Kesh.",
            "Kesh",
            false,
            true,
            json!({"error": "known_as", "known_as": "Stored-Private-Name"}),
        );
        assert_eq!(requests.len(), 1);
        let words = answer["error"].as_str().unwrap();
        assert!(!words.contains("Stored-Private-Name"), "{answer}");
        assert!(!words.contains("replace true"), "{answer}");
        assert!(
            !words.contains("ask whether") && !words.contains("say yes"),
            "{answer}"
        );
        assert!(words.contains("not changed"), "{answer}");
        assert!(answer.get("ok").is_none(), "{answer}");
    }

    #[test]
    fn remember_speaker_own_name_off_keeps_other_names_and_replacement_response() {
        let _serial = TESTS.lock().unwrap_or_else(|poison| poison.into_inner());
        let (answer, requests) = own_name_exchange(
            "Remember her face, she's Kess.",
            "Kess",
            false,
            false,
            json!({"error": "known_as", "known_as": "Stored-Private-Name"}),
        );
        assert_eq!(requests, [json!({"remember": "Kess"})]);
        assert_eq!(
            answer,
            json!({"error": "You already know this face as Stored-Private-Name. Say the new name back and ask whether they want to be known by it instead. Only if they say yes, call this again with the new name and replace true."})
        );
    }

    #[test]
    fn remember_speaker_is_an_exact_opt_in_and_is_passed_to_tool_routing() {
        assert!(switched_on(Some("on")));
        for value in [None, Some("off"), Some("true"), Some("ON"), Some(" on")] {
            assert!(!switched_on(value));
        }
        let flags: Vec<_> = [
            "--who-socket",
            "/tmp/eyes.sock",
            "--names-checked",
            FLAG,
            "--session",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let (settings, rest) = tools::take(&flags).unwrap();
        assert!(settings.remember_speaker && settings.names_checked);
        assert_eq!(rest, ["--session"]);
        assert!(matches!(tools::route(&settings, known_faces::REMEMBER),
            tools::Route::Faces(_, chosen, _) if chosen.remember_speaker && chosen.checked));
        let declaration = known_faces::tools_checked(true)
            .into_iter()
            .find(|tool| tool.name == known_faces::REMEMBER)
            .unwrap();
        let parameters = declaration.parameters.unwrap().to_string();
        for private in ["began_us", "ended_us", "remember_speaker", "boot"] {
            assert!(!parameters.contains(private));
        }
    }

    /// Voice's switch is on via switches(). Current Eyes with its enrollment
    /// switch off returns this same not_understood reply (eyes/main.rs), as
    /// an older peer does. This is Voice's actual reader/socket path, not a
    /// two-process switch or persistence test. The paired successful exchange
    /// below makes a regression that refuses every request fail as well.
    #[test]
    fn remember_speaker_wire_uses_internal_evidence_and_old_eyes_never_get_a_retry() {
        let _serial = TESTS.lock().unwrap();
        let home = crate::process::SessionDirectory::create().unwrap();
        let socket = home.path.join("eyes.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            writeln!(stream, "{{\"error\":\"not_understood\"}}").unwrap();
            listener.set_nonblocking(true).unwrap();
            (serde_json::from_str::<Value>(&line).unwrap(), listener)
        });
        let proof = proof("Please remember my face. My name is Kesh");
        let expected = json!({"remember_speaker_consented": {
            "name": "Kesh", "face": "eyes/f1", "began_us": proof.began_us,
            "ended_us": proof.ended_us, "replace": false, "apart": false,
        }});
        let owner = owner();
        let line = seen(proof);
        let (answer, answered) = run(
            &socket,
            &call(json!({
                "name": " Kesh ", "apart": true, "face": "intruder/f9",
                "began_us": 1, "ended_us": u64::MAX,
            })),
            Some(&line),
            Some(&owner.lease()),
        );
        assert!(answered);
        assert!(answer.get("error").is_some());
        assert_ne!(answer["ok"], true);
        assert!(answer.get("remembered").is_none());
        let (actual, listener) = peer.join().unwrap();
        assert_eq!(actual, expected);
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        drop(listener);

        // Same enabled Voice path with a supporting peer. The existing
        // bounded fixture records the actual guarded wire request, so an
        // all-refusal implementation cannot satisfy this positive control.
        let (paired, requests) = own_name_exchange(
            "Please remember my face. My name is Kesh",
            "Kesh",
            false,
            true,
            json!({"remembered": "Kesh"}),
        );
        assert_eq!(paired, json!({"ok": true, "remembered": "Kesh"}));
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["remember_speaker_consented"]["name"], "Kesh");
        assert_eq!(requests[0]["remember_speaker_consented"]["face"], "eyes/f1");
        assert!(requests[0].get("remember").is_none(), "no legacy save");
    }

    #[test]
    fn remember_speaker_requires_its_own_current_proof_names_and_lease_before_connect() {
        let _serial = TESTS.lock().unwrap();
        let home = crate::process::SessionDirectory::create().unwrap();
        let socket = home.path.join("eyes.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let owner = owner();
        let lease = owner.lease();
        let call = call(json!({"name": "Kesh"}));
        let good = seen(proof("Please remember my face. My name is Kesh"));
        let refused = |line: Option<&speaking_faces::LineSeen>, lease| {
            let (answer, _) = run(&socket, &call, line, lease);
            assert_ne!(answer["ok"], true);
            assert!(answer.get("error").is_some());
        };
        refused(None, Some(&lease));
        refused(Some(&good), None);
        let mut wrong = good.clone();
        wrong.request += 1;
        refused(Some(&wrong), Some(&lease));
        let mut stale = good.clone();
        let proof = stale.enrollment.as_mut().unwrap();
        proof.ended_us -= FRESH_US + 1;
        proof.began_us = proof.ended_us - 1;
        refused(Some(&stale), Some(&lease));
        let mut future = good.clone();
        future.enrollment.as_mut().unwrap().ended_us += 1_000_000;
        refused(Some(&future), Some(&lease));
        let unchecked = known_faces::FaceSwitches {
            checked: false,
            ..switches()
        };
        assert!(
            known_faces::run_scoped(
                &socket,
                &call,
                unchecked,
                None,
                Some(&good),
                REQUEST,
                Some(&lease)
            )
            .0
            .get("error")
            .is_some()
        );
        // A legacy helper without request authority cannot bypass the switch.
        assert!(
            known_faces::run_for(&socket, &call, switches(), None, Some(&good))
                .0
                .get("error")
                .is_some()
        );
        owner.revoke();
        refused(Some(&good), Some(&lease));
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn remember_speaker_never_borrows_an_earlier_name_or_replacement_confirmation() {
        let _serial = TESTS.lock().unwrap();
        let home = crate::process::SessionDirectory::create().unwrap();
        let socket = home.path.join("eyes.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let owner = owner();
        let line = seen(proof("Yes, remember my face"));
        let refused_name = run(
            &socket,
            &call(json!({"name":"Kesh"})),
            Some(&line),
            Some(&owner.lease()),
        )
        .0;
        assert!(
            refused_name["error"]
                .as_str()
                .unwrap()
                .starts_with("You did not hear that name")
        );
        let line = seen(proof("Please remember my face. My name is Kesh"));
        let refused_replace = run(
            &socket,
            &call(json!({"name":"Kesh", "replace":true})),
            Some(&line),
            Some(&owner.lease()),
        )
        .0;
        assert!(
            refused_replace["error"]
                .as_str()
                .unwrap()
                .starts_with("I cannot safely replace")
        );
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    fn closes_when(cancel: impl FnOnce(&memory_scope::RequestScope, &Proof)) {
        let home = crate::process::SessionDirectory::create().unwrap();
        let socket = home.path.join("eyes.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let (sent, received) = mpsc::channel();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            sent.send(()).unwrap();
            let mut byte = [0];
            stream.read(&mut byte).unwrap()
        });
        let owner = owner();
        let proof = proof("Please remember my face. My name is Kesh");
        let line = seen(proof.clone());
        speaking_faces::saw_line(line.clone());
        let lease = owner.lease();
        let worker = std::thread::spawn(move || {
            run(
                &socket,
                &call(json!({"name":"Kesh"})),
                Some(&line),
                Some(&lease),
            )
        });
        received.recv_timeout(Duration::from_secs(2)).unwrap();
        let start = Instant::now();
        cancel(&owner, &proof);
        let (answer, answered) = worker.join().unwrap();
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(!answered);
        assert!(answer.get("error").is_some());
        assert_eq!(
            peer.join().unwrap(),
            0,
            "Eyes must observe the request socket closing"
        );
    }

    #[test]
    fn remember_speaker_stop_and_forget_close_the_live_socket() {
        let _serial = TESTS.lock().unwrap();
        closes_when(|owner, _| owner.revoke());
        closes_when(|_, _| forget());
    }

    #[test]
    fn remember_speaker_expiry_and_forget_remove_stored_and_in_flight_evidence() {
        let _serial = TESTS.lock().unwrap();
        let proof = proof("My name is Kesh");
        speaking_faces::saw_line(seen(proof.clone()));
        assert!(proof.is_current(proof.ended_us + FRESH_US));
        assert!(!proof.is_current(proof.ended_us + FRESH_US + 1));
        speaking_faces::expire_enrollment(proof.ended_us + FRESH_US + 1);
        assert!(
            speaking_faces::line_seen(REQUEST)
                .unwrap()
                .enrollment
                .is_none()
        );
        let began_under = generation();
        forget();
        assert!(!proof.is_current(lamp_ipc::monotonic_us()));
        let now = lamp_ipc::monotonic_us();
        assert!(
            Proof::heard(
                "eyes/f1".into(),
                now - 1_000,
                now,
                "My name is Kesh",
                began_under
            )
            .is_none()
        );
    }

    #[test]
    fn remember_speaker_revoked_before_publication_never_inserts_evidence() {
        let _serial = TESTS.lock().unwrap();
        let request = REQUEST + 10;
        let owner = memory_scope::RequestScope::new(
            memory_scope::RequestKey {
                worker: lamp_turn_owner::BootId::new([78; 16]).unwrap(),
                session: 91,
                request,
            },
            memory_scope::ScopeSnapshot::room(),
        );
        let proof = proof("My name is Kesh");
        let mut line = seen(proof);
        line.request = request;
        owner.revoke();
        assert!(!speaking_faces::saw_line_scoped(line, &owner.lease()));
        assert!(speaking_faces::line_seen(request).is_none());
    }

    #[test]
    fn remember_speaker_proof_caps_count_bytes_and_refuse_instead_of_truncating() {
        let _serial = TESTS.lock().unwrap();
        let now = lamp_ipc::monotonic_us();
        let words = "é".repeat(HEARD_BYTES / 2);
        let face = format!("{}/f1", "b".repeat(FACE_BYTES - 3));
        assert_eq!((words.len(), face.len()), (HEARD_BYTES, FACE_BYTES));
        let accepted = Proof::heard(face.clone(), now - 1_000, now, &words, generation()).unwrap();
        assert_eq!(accepted.heard, words);
        assert_eq!(accepted.face, face);
        assert!(
            Proof::heard(
                face.clone(),
                now - 1_000,
                now,
                &format!("{words}a"),
                generation()
            )
            .is_none()
        );
        assert!(Proof::heard(format!("b{face}"), now - 1_000, now, &words, generation()).is_none());
        let mut invalid = accepted;
        invalid.heard.push('a');
        assert!(!invalid.is_current(now));
    }

    #[test]
    fn remember_speaker_off_keeps_the_legacy_wire() {
        let _serial = TESTS.lock().unwrap();
        let home = crate::process::SessionDirectory::create().unwrap();
        let socket = home.path.join("eyes.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            writeln!(stream, "{{\"remembered\":\"Kesh\"}}").unwrap();
            line
        });
        let answer = known_faces::run_scoped(
            &socket,
            &call(json!({"name":"Kesh"})),
            known_faces::FaceSwitches::default(),
            None,
            None,
            REQUEST,
            None,
        );
        assert_eq!(answer, (json!({"ok":true,"remembered":"Kesh"}), true));
        assert_eq!(peer.join().unwrap(), "{\"remember\":\"Kesh\"}\n");
    }
}
