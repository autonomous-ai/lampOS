//! A failed Days snapshot must not become a conversation's permanent note.
use super::{Sessions, Told, told_while_scope_holds};
use crate::{
    memory_scope::{RequestKey, RequestScope, ScopeSnapshot},
    remember::{DaysClient, FRESH_RETRY, SessionNote},
};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

fn note(text: &str, complete: bool) -> SessionNote {
    SessionNote {
        text: text.into(),
        complete,
    }
}

#[test]
fn an_incomplete_note_retries_at_thirty_seconds_and_then_stays_cached() {
    let mut sessions = Sessions::default();
    let start = Instant::now();
    let mut asked = Vec::new();
    for (seconds, expected) in [
        (0, "unavailable"),
        (1, "unavailable"),
        (29, "unavailable"),
        (30, "kept name"),
        (360, "kept name"),
    ] {
        let got = sessions.frozen(false, start + Duration::from_secs(seconds), |session| {
            asked.push(session);
            if session == 1 {
                note("unavailable", false)
            } else {
                note("kept name", true)
            }
        });
        assert_eq!(got, expected, "at {seconds} s");
    }
    assert_eq!(asked, [1, 2], "each attempt has its own Days session");
}

#[test]
fn repeated_failures_get_one_attempt_per_interval() {
    let mut sessions = Sessions::default();
    let start = Instant::now();
    let mut asked = Vec::new();
    for seconds in 0..=91 {
        sessions.frozen(false, start + Duration::from_secs(seconds), |session| {
            asked.push((seconds, session));
            note("room only", false)
        });
    }
    assert_eq!(asked, [(0, 1), (30, 2), (60, 3), (90, 4)]);
}

#[test]
fn a_valid_empty_note_is_cached_without_retries() {
    let mut sessions = Sessions::default();
    let start = Instant::now();
    assert_eq!(sessions.frozen(false, start, |_| note("", true)), "");
    assert_eq!(
        sessions.frozen(false, start + FRESH_RETRY * 100, |_| panic!(
            "valid empty note retried"
        )),
        ""
    );
}

#[test]
fn an_explicit_refresh_does_not_wait_for_failed_read_backoff() {
    let mut sessions = Sessions::default();
    let start = Instant::now();
    sessions.frozen(false, start, |_| note("old partial note", false));
    let next = start + Duration::from_secs(1);
    assert_eq!(
        sessions.frozen(true, next, |session| {
            assert_eq!(session, 2);
            note("after forced refresh", true)
        }),
        "after forced refresh"
    );
    assert_eq!(
        sessions.frozen(false, next + FRESH_RETRY, |_| panic!(
            "successful refresh retried"
        )),
        "after forced refresh"
    );
}

#[test]
fn a_held_person_change_cannot_leave_the_previous_persons_note_cached() {
    let mut sessions = Sessions::default();
    let now = Instant::now();
    let a = Some("person-a".to_owned());
    let b = Some("person-b".to_owned());
    assert!(sessions.told_for(&a));
    sessions.frozen(true, now, |_| note("A's note", true));
    // A held B updates admitted scope but skips the note and keep marker.
    assert!(sessions.told_for(&b));
    assert!(sessions.note.is_none());
    assert!(
        !sessions.told_for(&b),
        "the visible B is the same admitted person"
    );
    assert_eq!(
        sessions.frozen(false, now, |_| note("B's note", true)),
        "B's note"
    );
    assert!(sessions.told_for(&None));
    assert_eq!(
        sessions.frozen(true, now, |_| note("room only", false)),
        "room only"
    );
    assert!(
        !sessions
            .frozen(false, now, |_| panic!("backoff"))
            .contains("B's")
    );
}

fn scope() -> RequestScope {
    RequestScope::new(
        RequestKey {
            worker: lamp_turn_owner::BootId::new([7; 16]).unwrap(),
            session: 1,
            request: 1,
        },
        ScopeSnapshot::of("Test person"),
    )
}

#[test]
fn authority_lost_during_a_note_read_discards_the_whole_context() {
    for narrowed in [false, true] {
        let scope = scope();
        let lease = scope.lease();
        let (entered, reading) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let working = lease.clone();
        let worker = std::thread::spawn(move || {
            told_while_scope_holds(&Mutex::default(), Some(&working), |_, captured| {
                assert!(captured.unwrap().person().is_some());
                entered.send(()).unwrap();
                released.recv_timeout(Duration::from_secs(2)).unwrap();
                Told {
                    text: "personal note".into(),
                    after: Some("private thread".into()),
                    history: vec![super::Message::user("private exchange")],
                }
            })
        });
        reading.recv_timeout(Duration::from_secs(2)).unwrap();
        if narrowed {
            lease.narrow_to_room();
        } else {
            scope.revoke();
        }
        release.send(()).unwrap();
        assert_eq!(
            worker.join().unwrap(),
            Told::default(),
            "narrowed={narrowed}"
        );
    }
    let scope = scope();
    scope.revoke();
    assert_eq!(
        told_while_scope_holds(&Mutex::default(), Some(&scope.lease()), |_, _| panic!(
            "revoked scope read memory"
        )),
        Told::default()
    );
}

#[test]
fn unchanged_authority_keeps_the_note_and_unleased_behavior() {
    let scope = scope();
    let expected = Told::from("available note".to_owned());
    assert_eq!(
        told_while_scope_holds(&Mutex::default(), Some(&scope.lease()), |_, _| expected
            .clone()),
        expected
    );
    assert_eq!(
        told_while_scope_holds(&Mutex::default(), None, |_, scope| {
            assert!(scope.is_none());
            expected.clone()
        }),
        expected
    );
}

#[test]
fn authority_is_checked_after_waiting_for_an_older_context() {
    for narrowed in [false, true] {
        let sessions = Arc::new(Mutex::new(Sessions::default()));
        let held = sessions.lock().unwrap();
        let scope = scope();
        let lease = scope.lease();
        let working = lease.clone();
        let waiting = sessions.clone();
        let (started, entering) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            started.send(()).unwrap();
            told_while_scope_holds(&waiting, Some(&working), |_, admitted| {
                assert!(
                    admitted.unwrap().person().is_none(),
                    "obsolete personal read"
                );
                Told::from("room".to_owned())
            })
        });
        entering.recv_timeout(Duration::from_secs(2)).unwrap();
        if narrowed {
            lease.narrow_to_room();
        } else {
            scope.revoke();
        }
        drop(held);
        assert_eq!(
            worker.join().unwrap(),
            if narrowed {
                Told::from("room".to_owned())
            } else {
                Told::default()
            }
        );
    }
}

/// The real Voice socket client, with only the store's replies controlled.
/// A wake connection stops the fixture; no polling thread is left behind.
struct Memory {
    socket: PathBuf,
    refused: Arc<AtomicBool>,
    opens: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Memory {
    fn new() -> Self {
        let socket =
            std::env::temp_dir().join(format!("fourth-note-retry-{}.sock", std::process::id()));
        let listener = UnixListener::bind(&socket).unwrap();
        let refused = Arc::new(AtomicBool::new(false));
        let opens = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (fail, count, stopping) = (refused.clone(), opens.clone(), stop.clone());
        let worker = std::thread::spawn(move || {
            let mut kept = false;
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                if stopping.load(Ordering::Acquire) {
                    break;
                }
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut line = String::new();
                if BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap_or(0)
                    == 0
                {
                    continue;
                }
                let request: Value = serde_json::from_str(&line).unwrap();
                let answer = match request["op"].as_str() {
                    Some("remember") => {
                        kept = true;
                        json!({"ok":true,"id":7})
                    }
                    Some("session_open") => {
                        count.fetch_add(1, Ordering::Release);
                        if fail.load(Ordering::Acquire) {
                            json!({"ok":false,"error":"temporarily unavailable"})
                        } else {
                            let facts = if kept {
                                json!([{"id":7,"kind":"preference","text":"My name is Pippa"}])
                            } else {
                                json!([])
                            };
                            json!({"ok":true,"snapshot":{
                                "session":request["session"],"opened_unix_ms":1,"generation":1,
                                "invalidated":false,"source_ids":if kept { json!([7]) } else { json!([]) },
                                "note":json!({"scope":"said in this room; speaker unknown","untrusted":true,"facts":facts}).to_string()
                            }})
                        }
                    }
                    _ => json!({"ok":true}),
                };
                let _ = writeln!(stream, "{answer}");
            }
        });
        Self {
            socket,
            refused,
            opens,
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for Memory {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = UnixStream::connect(&self.socket);
        self.worker.take().unwrap().join().unwrap();
        std::fs::remove_file(&self.socket).unwrap();
    }
}

#[test]
fn a_kept_name_recovers_after_a_failed_refresh_without_another_keep() {
    let memory = Memory::new();
    let client = DaysClient::new(memory.socket.clone()).unwrap();
    let mut sessions = Sessions::default();
    let start = Instant::now();
    let timings = Mutex::new(Vec::new());
    let ask = |session| {
        let began = Instant::now();
        let note =
            client.session_note_within(session, None, false, Duration::from_millis(300), true);
        timings.lock().unwrap().push(began.elapsed().as_micros());
        note
    };
    assert!(!sessions.frozen(false, start, ask).contains("Pippa"));
    let call = lamp_gemini::ToolCall {
        id: "kept-name".into(),
        name: crate::remember::REMEMBER.into(),
        args: json!({"text":"My name is Pippa"}),
    };
    assert_eq!(
        client.run(&call, Duration::from_millis(300)).0["remembered"],
        true
    );
    memory.refused.store(true, Ordering::Release);
    assert!(
        !sessions
            .frozen(
                client.kept_since_note(),
                start + Duration::from_secs(1),
                ask
            )
            .contains("Pippa")
    );
    assert!(
        !client.kept_since_note(),
        "the original keep marker is consumed once"
    );
    memory.refused.store(false, Ordering::Release);
    let cached = Instant::now();
    assert!(
        !sessions
            .frozen(false, start + Duration::from_secs(30), ask)
            .contains("Pippa")
    );
    let cached_us = cached.elapsed().as_micros();
    assert_eq!(
        memory.opens.load(Ordering::Acquire),
        2,
        "no I/O before retry eligibility"
    );

    // Six minutes of continued talk, without a new remember or person.
    // The original name has left the 300 s/six-exchange history and the
    // eight-exchange thread note; neither can hide a broken snapshot retry.
    let mut thread = crate::thread::Thread::new();
    thread.person("My name is Pippa", true, start);
    thread.lamp(1, "All right.", start);
    for request in 2..=9 {
        let at = start + Duration::from_secs((request - 1) * 45);
        thread.person("And then?", true, at);
        thread.lamp(request, "Another thing.", at);
    }
    let later = start + Duration::from_secs(360);
    let recent = thread.recent_for(None, later, super::HISTORY_WITHIN, super::HISTORY_EXCHANGES);
    assert_eq!(recent.len(), 6);
    assert!(!format!("{recent:?}").contains("Pippa"));
    assert!(
        !thread
            .recall_for(None, later, usize::MAX)
            .unwrap()
            .contains("Pippa")
    );
    let recovered = sessions.frozen(false, later, ask);
    assert!(
        recovered.contains("Pippa"),
        "the snapshot recovers the name independently of chat history"
    );
    assert_eq!(memory.opens.load(Ordering::Acquire), 3);
    assert_eq!(
        sessions.frozen(false, later + FRESH_RETRY, |_| panic!(
            "healthy note retried"
        )),
        recovered
    );
    eprintln!(
        "memory note timings: ask_us={:?} cached_us={cached_us} opens=3",
        timings.into_inner().unwrap()
    );
}
