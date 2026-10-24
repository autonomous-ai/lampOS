//! Request-local evidence for saving only the face seen speaking the request.
//! No model argument can supply this proof. Eyes independently checks its
//! image captured during the same interval and its final write permission.
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

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
    static TESTS: Mutex<()> = Mutex::new(());
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
            tools::Route::Faces(_, chosen) if chosen.remember_speaker && chosen.checked));
        let declaration = known_faces::tools_checked(true)
            .into_iter()
            .find(|tool| tool.name == known_faces::REMEMBER)
            .unwrap();
        let parameters = declaration.parameters.unwrap().to_string();
        for private in ["began_us", "ended_us", "remember_speaker", "boot"] {
            assert!(!parameters.contains(private));
        }
    }

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
        let proof = proof("My name is Kesh, remember my face");
        let expected = json!({"remember_speaker": {
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
        let (actual, listener) = peer.join().unwrap();
        assert_eq!(actual, expected);
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
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
        let good = seen(proof("My name is Kesh"));
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
        let line = seen(proof("Yes, my name is Kesh"));
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
        let proof = proof("My name is Kesh");
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
