//! A failed Days snapshot must not become a conversation's permanent note.
use super::{Sessions, Told, told_while_scope_holds};
use crate::{
    memory_scope::{RequestKey, RequestScope, ScopeSnapshot},
    remember::{DaysClient, FRESH_RETRY, NoteCause, SessionNote},
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
        cause: (!complete).then_some(NoteCause::Refused),
    }
}

/// What the child's incomplete note says: never in the journal.
const NOTE_WORDS: &str = "Their name is Pippa";
const JOURNAL_CHILD: &str = "NOTE_CAUSE_TEST_CHILD";

/// The journal's one "memory note incomplete" line for a note read that
/// failed for `cause`, from this `test` run again in a child process (the
/// line is its stderr); none in the child itself, which only asks.
fn incomplete_line(test: &str, cause: NoteCause) -> Option<String> {
    if std::env::var(JOURNAL_CHILD).ok().as_deref() == Some("1") {
        let mut sessions = Sessions::default();
        sessions.frozen(false, Instant::now(), |_| SessionNote {
            text: NOTE_WORDS.into(),
            complete: false,
            cause: Some(cause),
        });
        return None;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("second_provider::worker::memory_note_retries::{test}"),
            "--nocapture",
        ])
        .env(JOURNAL_CHILD, "1")
        .output()
        .unwrap();
    let (stdout, stderr) = (
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "{stdout}\n{stderr}"
    );
    assert!(!stderr.contains(NOTE_WORDS), "{stderr}");
    let lines: Vec<&str> = stderr
        .lines()
        .filter(|line| line.contains("memory note incomplete"))
        .collect();
    assert_eq!(lines.len(), 1, "{stderr}");
    Some(lines[0].to_owned())
}

/// The line's existing words, then its cause's code, nothing else.
fn says_cause(line: Option<String>, code: &str) {
    if let Some(line) = line {
        assert_eq!(
            line,
            format!(
                "lamp-voice second: memory note incomplete; retry on the next turn; cause={code}"
            )
        );
    }
}

/// #1693: the "memory note incomplete" line names why, as a code (lamp-4ace's
/// r07/r08: 23 lines each, why unknown).
#[test]
fn the_incomplete_line_names_a_timeout() {
    let test = "the_incomplete_line_names_a_timeout";
    says_cause(incomplete_line(test, NoteCause::Timeout), "timeout");
}

#[test]
fn the_incomplete_line_names_a_refusal() {
    let test = "the_incomplete_line_names_a_refusal";
    says_cause(incomplete_line(test, NoteCause::Refused), "refused");
}

#[test]
fn the_incomplete_line_names_an_invalid_snapshot() {
    let test = "the_incomplete_line_names_an_invalid_snapshot";
    says_cause(incomplete_line(test, NoteCause::Invalid), "invalid");
}

#[test]
fn the_incomplete_line_names_a_missing_own_snapshot() {
    let test = "the_incomplete_line_names_a_missing_own_snapshot";
    says_cause(incomplete_line(test, NoteCause::OwnMissing), "own_missing");
}

#[test]
fn the_incomplete_line_names_a_busy_client() {
    let test = "the_incomplete_line_names_a_busy_client";
    says_cause(incomplete_line(test, NoteCause::Busy), "busy");
}

#[test]
fn the_incomplete_line_names_no_client() {
    let test = "the_incomplete_line_names_no_client";
    says_cause(incomplete_line(test, NoteCause::NoClient), "no_client");
}

/// A memory slow once costs one turn its note, not the 30 s after it
/// (lamp-4ace, 22 Oct 17:45): the next turn asks again.
#[test]
fn an_incomplete_note_retries_on_the_next_turn_and_then_stays_cached() {
    let mut sessions = Sessions::default();
    let start = Instant::now();
    let mut asked = Vec::new();
    for (seconds, expected) in [
        (0, "unavailable"),
        (1, "kept name"),
        (29, "kept name"),
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

/// One failure is asked again on the next turn; two back to back, and the
/// store is asked no more than once each 30 s from the last attempt.
#[test]
fn repeated_failures_after_the_next_turn_get_one_attempt_per_interval() {
    let mut sessions = Sessions::default();
    let start = Instant::now();
    let mut asked = Vec::new();
    for seconds in 0..=91 {
        sessions.frozen(false, start + Duration::from_secs(seconds), |session| {
            asked.push((seconds, session));
            note("room only", false)
        });
    }
    assert_eq!(asked, [(0, 1), (1, 2), (31, 3), (61, 4), (91, 5)]);
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
    // The next turn asks the room's again; B's never comes back.
    assert!(
        !sessions
            .frozen(false, now, |_| note("room again", true))
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
    // The next turn asks again, and the store still refuses: two failures
    // back to back, so none is asked for 30 s from this one.
    assert!(
        !sessions
            .frozen(false, start + Duration::from_secs(2), ask)
            .contains("Pippa")
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
        3,
        "no I/O before retry eligibility"
    );

    // Eleven minutes of continued talk, without a new remember or person.
    // The original name has left the 600 s/twelve-exchange history and the
    // twelve-exchange thread note; neither can hide a broken snapshot retry.
    let mut thread = crate::thread::Thread::new();
    thread.person("My name is Pippa", true, start);
    thread.lamp(1, "All right.", start);
    for request in 2..=15 {
        let at = start + Duration::from_secs((request - 1) * 45);
        thread.person("And then?", true, at);
        thread.lamp(request, "Another thing.", at);
    }
    let later = start + Duration::from_secs(660);
    let recent = thread.recent_for(None, later, super::HISTORY_WITHIN, super::HISTORY_EXCHANGES);
    assert_eq!(recent.len(), super::HISTORY_EXCHANGES);
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
    assert_eq!(memory.opens.load(Ordering::Acquire), 4);
    assert_eq!(
        sessions.frozen(false, later + FRESH_RETRY, |_| panic!(
            "healthy note retried"
        )),
        recovered
    );
    eprintln!(
        "memory note timings: ask_us={:?} cached_us={cached_us} opens=4",
        timings.into_inner().unwrap()
    );
}

/// A controller on the test thread, not a mock worker to stop or join.
/// Real client writer closes may interleave with foreground snapshot opens.
struct SocketScript {
    socket: PathBuf,
    listener: UnixListener,
    requests: Vec<Value>,
}
impl SocketScript {
    fn new(label: &str) -> Self {
        let socket =
            std::env::temp_dir().join(format!("note-boundary-{label}-{}.sock", std::process::id()));
        let listener = UnixListener::bind(&socket).unwrap();
        Self {
            socket,
            listener,
            requests: Vec::new(),
        }
    }

    fn next_open(&mut self) -> (UnixStream, Value) {
        use rustix::event::{PollFd, PollFlags, Timespec, poll};
        use std::io::Read;
        // A faulty client must fail the fixture instead of blocking accept,
        // reading forever, or producing an unbounded stream of close jobs.
        for _ in 0..16 {
            let mut ready = [PollFd::new(&self.listener, PollFlags::IN)];
            let timeout = Timespec {
                tv_sec: 2,
                tv_nsec: 0,
            };
            assert_eq!(poll(&mut ready, Some(&timeout)).unwrap(), 1);
            let (mut stream, _) = self.listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .take(8_193)
                .read_line(&mut line)
                .unwrap();
            assert!(line.len() <= 8_192 && line.ends_with('\n'));
            let request: Value = serde_json::from_str(&line).unwrap();
            self.requests.push(request.clone());
            match request["op"].as_str() {
                Some("session_open") => return (stream, request),
                Some("session_close") => writeln!(stream, "{}", json!({"ok":true})).unwrap(),
                other => panic!("unexpected operation: {other:?}"),
            }
        }
        panic!("no snapshot open among sixteen bounded requests");
    }

    fn opens(&self) -> Vec<&Value> {
        self.requests
            .iter()
            .filter(|request| request["op"] == "session_open")
            .collect()
    }

    fn snapshot(request: &Value, id: u64, text: &str) -> Value {
        let scope = if request["person"].is_string() {
            "caller-supplied person; identity not verified"
        } else {
            "said in this room; speaker unknown"
        };
        json!({"ok":true,"snapshot":{
            "session":request["session"],"opened_unix_ms":1,"generation":1,
            "invalidated":false,"source_ids":[id],
            "note":json!({"scope":scope,"untrusted":true,
                "facts":[{"id":id,"kind":"preference","text":text}]}).to_string()
        }})
    }
}
impl Drop for SocketScript {
    fn drop(&mut self) {
        // The controller has no background accept/read or join. Production
        // writer closes remain best effort, with their existing I/O bound.
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// Start timing at the eligible Sessions ask, before real client I/O. Any
/// held replies stay open until that ask returns, so EOF cannot fake timeout.
fn socket_attempt(
    memory: &mut SocketScript,
    ask: impl FnOnce() -> String + Send,
    script: impl FnOnce(
        &mut SocketScript,
        &std::sync::mpsc::Receiver<(String, Duration)>,
    ) -> Vec<UnixStream>,
) -> (String, Duration) {
    std::thread::scope(|threads| {
        let (finished, result) = std::sync::mpsc::sync_channel(1);
        let asking = threads.spawn(move || {
            let began = Instant::now();
            let text = ask();
            finished.send((text, began.elapsed())).unwrap();
        });
        let held = script(memory, &result);
        let outcome = result.recv_timeout(Duration::from_secs(2));
        drop(held);
        asking.join().unwrap();
        outcome.expect("the bounded client ask must return")
    })
}

#[test]
fn a_timed_out_note_retries_and_recovers_without_a_keep() {
    let mut memory = SocketScript::new("timeout-recovery");
    let client = DaysClient::new(memory.socket.clone()).unwrap();
    let mut sessions = Sessions::default();
    let start = Instant::now();
    let ask = |session| {
        client.session_note_within(session, None, false, Duration::from_millis(300), true)
    };
    let (failed, _) = socket_attempt(
        &mut memory,
        || sessions.frozen(false, start, ask),
        |memory, _| {
            let (held, request) = memory.next_open();
            assert!(request["person"].is_null());
            vec![held]
        },
    );
    assert!(!failed.contains("recovered fact"));
    // Asked again on the next turn, not 30 s later.
    assert_eq!(sessions.note_retry_at, Some(start));
    assert_eq!(memory.opens().len(), 1);
    let (recovered, _) = socket_attempt(
        &mut memory,
        || sessions.frozen(false, start + Duration::from_secs(1), ask),
        |memory, _| {
            let (mut stream, request) = memory.next_open();
            writeln!(
                stream,
                "{}",
                SocketScript::snapshot(&request, 11, "recovered fact")
            )
            .unwrap();
            vec![]
        },
    );
    assert!(recovered.contains("recovered fact"));
    assert!(sessions.note_retry_at.is_none());
    assert_eq!(
        sessions.frozen(false, start + FRESH_RETRY * 2, |_| {
            panic!("healthy recovered note retried")
        }),
        recovered
    );
    let opens = memory.opens();
    assert_eq!(opens.len(), 2);
    assert!(opens[0]["session"].as_str().unwrap().ends_with("-1"));
    assert!(opens[1]["session"].as_str().unwrap().ends_with("-2"));
    assert_ne!(opens[0]["session"], opens[1]["session"]);
    assert!(!client.kept_since_note(), "no keep triggered recovery");
}

#[test]
fn a_non_held_person_change_bypasses_the_failed_persons_backoff() {
    let mut memory = SocketScript::new("person-change");
    let client = DaysClient::new(memory.socket.clone()).unwrap();
    let sessions = Mutex::new(Sessions::default());
    let start = Instant::now();
    let make_scope = |request, name| {
        RequestScope::new(
            RequestKey {
                worker: lamp_turn_owner::BootId::new([8; 16]).unwrap(),
                session: 1,
                request,
            },
            ScopeSnapshot::of(name),
        )
    };
    let a = make_scope(1, "Scope A");
    let b = make_scope(2, "Scope B");
    let loads = AtomicUsize::new(0);
    let context = |lease: &crate::memory_scope::ScopeLease, now| {
        told_while_scope_holds(&sessions, Some(lease), |sessions, captured| {
            let captured = captured.unwrap();
            assert!(!captured.held(), "this is the normal identified path");
            let person = captured.person().map(str::to_owned);
            // The real scope-change result drives refresh, exactly as the
            // worker's selection does. No forced-refresh literal is supplied.
            let changed = sessions.told_for(&person);
            Told::from(sessions.frozen(changed, now, |session| {
                loads.fetch_add(1, Ordering::Relaxed);
                client.session_note_within(
                    session,
                    person.as_deref(),
                    false,
                    Duration::from_millis(300),
                    true,
                )
            }))
        })
        .text
    };
    let (partial, _) = socket_attempt(
        &mut memory,
        || context(&a.lease(), start),
        |memory, _| {
            let (mut room, request) = memory.next_open();
            assert!(request["person"].is_null());
            writeln!(
                room,
                "{}",
                SocketScript::snapshot(&request, 11, "old partial room")
            )
            .unwrap();
            let (mut personal, request) = memory.next_open();
            assert_eq!(
                request["person"],
                a.lease().scope().unwrap().person().unwrap()
            );
            writeln!(personal, "{}", json!({"ok":false})).unwrap();
            vec![]
        },
    );
    assert!(partial.contains("old partial room"));
    assert_eq!(loads.load(Ordering::Relaxed), 1);
    assert_eq!(sessions.lock().unwrap().note_retry_at, Some(start));
    a.revoke();
    let (theirs, _) = socket_attempt(
        &mut memory,
        || context(&b.lease(), start + Duration::from_secs(1)),
        |memory, _| {
            let (mut room, request) = memory.next_open();
            assert!(request["person"].is_null());
            writeln!(room, "{}", SocketScript::snapshot(&request, 12, "new room")).unwrap();
            let (mut personal, request) = memory.next_open();
            assert_eq!(
                request["person"],
                b.lease().scope().unwrap().person().unwrap()
            );
            writeln!(
                personal,
                "{}",
                SocketScript::snapshot(&request, 22, "B-only fact")
            )
            .unwrap();
            vec![]
        },
    );
    assert!(theirs.contains("B-only fact") && theirs.contains("new room"));
    assert_eq!(loads.load(Ordering::Relaxed), 2);
    assert!(!theirs.contains("old partial room"));
    assert!(sessions.lock().unwrap().note_retry_at.is_none());
    let opens_before_cached = memory.opens().len();
    assert_eq!(context(&b.lease(), start + Duration::from_secs(2)), theirs);
    assert_eq!(loads.load(Ordering::Relaxed), 2);
    assert_eq!(memory.opens().len(), opens_before_cached);
    let opens = memory.opens();
    assert_eq!(opens.len(), 4);
    assert_eq!(opens[0]["session"], opens[1]["session"]);
    assert_eq!(opens[2]["session"], opens[3]["session"]);
    assert!(opens[0]["session"].as_str().unwrap().ends_with("-1"));
    assert!(opens[2]["session"].as_str().unwrap().ends_with("-2"));
    assert_ne!(opens[0]["session"], opens[2]["session"]);
}

#[test]
fn an_eligible_retry_shares_one_budget_between_room_and_person() {
    let mut memory = SocketScript::new("retry-budget");
    let client = DaysClient::new(memory.socket.clone()).unwrap();
    let person = crate::remember::person_id("Scope A");
    let mut sessions = Sessions::default();
    let start = Instant::now();
    // This measures Sessions -> the actual DaysClient API with its explicit
    // 300 ms argument. fresh_session_note/FIRST_READ wiring is source-only
    // here; the remember client tests assert that production constant too.
    let ask = |session| {
        client.session_note_within(
            session,
            Some(&person),
            false,
            Duration::from_millis(300),
            true,
        )
    };
    socket_attempt(
        &mut memory,
        || sessions.frozen(false, start, ask),
        |memory, _| {
            let (mut room, request) = memory.next_open();
            writeln!(
                room,
                "{}",
                SocketScript::snapshot(&request, 11, "room fact")
            )
            .unwrap();
            let (mut personal, _) = memory.next_open();
            writeln!(personal, "{}", json!({"ok":false})).unwrap();
            vec![]
        },
    );
    // One failure: eligible on the next turn; this retry is a later one.
    assert_eq!(sessions.note_retry_at, Some(start));
    let (_, elapsed) = socket_attempt(
        &mut memory,
        || sessions.frozen(false, start + FRESH_RETRY, ask),
        |memory, result| {
            let (mut room, request) = memory.next_open();
            assert!(matches!(
                result.recv_timeout(Duration::from_millis(200)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ));
            writeln!(
                room,
                "{}",
                SocketScript::snapshot(&request, 11, "room fact")
            )
            .unwrap();
            let (personal, request) = memory.next_open();
            assert_eq!(request["person"], person);
            vec![personal]
        },
    );
    assert!(elapsed >= Duration::from_millis(280), "{elapsed:?}");
    assert!(elapsed < Duration::from_millis(400), "{elapsed:?}");
    assert_eq!(sessions.note_retry_at, Some(start + FRESH_RETRY * 2));
    sessions.frozen(
        false,
        start + FRESH_RETRY * 2 - Duration::from_secs(1),
        |_| panic!("timed-out retry attempted again before eligibility"),
    );
    let opens = memory.opens();
    assert_eq!(opens.len(), 4);
    assert_eq!(opens[0]["session"], opens[1]["session"]);
    assert_eq!(opens[2]["session"], opens[3]["session"]);
    assert_ne!(opens[0]["session"], opens[2]["session"]);
    eprintln!(
        "retry note I/O: configured_us=300000 observed_us={} scopes=2",
        elapsed.as_micros()
    );
}

/// The thread [`Paced`] gives as a person's follow-up.
const THREAD: &str = "a synthetic trip";

/// What the memory does with a snapshot open ([`Paced`]): answers it, or
/// refuses it, after so many ms.
#[derive(Clone, Copy)]
enum Reply {
    After(u64),
    RefusedAfter(u64),
}

/// A memory on its own thread that answers each request in turn, as
/// lamp-memory does: each snapshot open as `pace` says for its count from
/// 0, an answer with one fact naming that count and whose it is, a
/// person's follow-up with [`THREAD`], and everything else at once. A wake
/// connection stops it.
struct Paced {
    socket: PathBuf,
    requests: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Paced {
    fn new(label: &str, pace: impl Fn(usize) -> Reply + Send + 'static) -> Self {
        let socket =
            std::env::temp_dir().join(format!("note-ahead-{label}-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (seen, stopping) = (requests.clone(), stop.clone());
        let worker = std::thread::spawn(move || {
            let mut opens = 0;
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else {
                    continue;
                };
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
                seen.lock().unwrap().push(request.clone());
                let answer = if request["op"] == "session_open" {
                    let count = opens;
                    opens += 1;
                    let (ms, answered) = match pace(count) {
                        Reply::After(ms) => (ms, true),
                        Reply::RefusedAfter(ms) => (ms, false),
                    };
                    std::thread::sleep(Duration::from_millis(ms));
                    let whose = request["person"].as_str().unwrap_or("the room");
                    if answered {
                        SocketScript::snapshot(
                            &request,
                            11 + count as u64,
                            &format!("fact {count} of {whose}"),
                        )
                    } else {
                        json!({"ok":false,"error":"temporarily unavailable"})
                    }
                } else if request["op"] == "follow_up" && request["person"].is_string() {
                    json!({"ok":true,"follow_up":{"id":99,"category":"thread","text":THREAD,"until_asked":true}})
                } else {
                    json!({"ok":true})
                };
                // A client that left before its answer is not an error here.
                let _ = writeln!(stream, "{answer}");
            }
        });
        Self {
            socket,
            requests,
            stop,
            worker: Some(worker),
        }
    }

    fn count(&self, op: &str) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request["op"] == op)
            .count()
    }

    fn opens(&self) -> usize {
        self.count("session_open")
    }
}
impl Drop for Paced {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = UnixStream::connect(&self.socket);
        self.worker.take().unwrap().join().unwrap();
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// The worker's note asked ahead and its turn's context, as the engine
/// uses them, against `memory`, with the process's one thread set aside
/// (a turn's context opens it) and put back after.
struct Turns {
    sessions: Arc<Mutex<Sessions>>,
    ahead: super::Ahead,
    client: Arc<DaysClient>,
    kept: Option<crate::thread::Thread>,
    _one: crate::provider_worker::tests::OneThread,
}
impl Turns {
    fn new(memory: &Paced) -> Self {
        let one = crate::provider_worker::tests::one_thread();
        let kept = std::mem::take(
            &mut *crate::thread::THREAD
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        let client = Arc::new(DaysClient::new(memory.socket.clone()).unwrap());
        let sessions = Arc::new(Mutex::new(Sessions::default()));
        let reading = client.clone();
        let ahead = super::Ahead::asking(
            sessions.clone(),
            None,
            Arc::new(move |session, person, within, follow_up| {
                reading.read_note_within(session, person, false, within, (true, follow_up))
            }),
        );
        Self {
            sessions,
            ahead,
            client,
            kept: Some(kept),
            _one: one,
        }
    }

    /// A line begins in `scope`, and nothing more is done with it: it is
    /// someone's talk in the room, never answered.
    fn begins(&self, scope: &RequestScope) {
        self.ahead.begin(Some(&scope.lease()), Instant::now());
    }

    /// The memory note a turn told in `scope` is given, as the worker's
    /// context callback composes it.
    fn told(&self, scope: &RequestScope) -> String {
        let socket = Some(PathBuf::from("/unused/the note is asked ahead"));
        let context = super::memory_context(
            self.sessions.clone(),
            socket,
            None,
            true,
            |_, _, _, note| Told::from(note),
        );
        context(Some(&scope.lease()), super::Chosen::ByLease).text
    }

    /// The line's end, for a line that is answered: waited for as the
    /// engine waits, then told.
    async fn answered(&self, scope: &RequestScope) -> String {
        self.ahead.clone().ready(Some(scope.lease())).await;
        self.told(scope)
    }
}
impl Drop for Turns {
    fn drop(&mut self) {
        *crate::thread::THREAD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = self.kept.take().unwrap();
    }
}

fn scoped(request: u64, snapshot: ScopeSnapshot) -> RequestScope {
    RequestScope::new(
        RequestKey {
            worker: lamp_turn_owner::BootId::new([9; 16]).unwrap(),
            session: 1,
            request,
        },
        snapshot,
    )
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// The longest the runtime's one thread went without running this, from
/// its start until `stop`: how long the worker's loop was held up.
async fn longest_gap(stop: Arc<AtomicBool>) -> Duration {
    let mut longest = Duration::ZERO;
    let mut last = Instant::now();
    while !stop.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(5)).await;
        let now = Instant::now();
        longest = longest.max(now - last);
        last = now;
    }
    longest
}

/// lamp-4ace, 22 Oct 17:45: the note asked as the line ended held the
/// worker's loop 300 ms and came 60 ms after the voice gave up. Here the
/// first turn's note is refused, so the next line, in the same scope, asks
/// it again as it begins: an answer 350 ms after that ask is in by the end
/// of the 1 s line, the turn is told it at once, and nothing on the loop
/// waits on the memory.
#[test]
fn a_note_asked_as_the_line_begins_is_in_by_its_end_and_the_loop_never_waits() {
    let memory = Paced::new("ahead", |count| match count {
        0 => Reply::RefusedAfter(0),
        _ => Reply::After(350),
    });
    let turns = Turns::new(&memory);
    let (first_line, line) = (
        scoped(1, ScopeSnapshot::room()),
        scoped(2, ScopeSnapshot::room()),
    );
    let (first, note, held_up, begin, end) = runtime().block_on(async {
        turns.begins(&first_line);
        let first = turns.answered(&first_line).await;
        let stop = Arc::new(AtomicBool::new(false));
        let gaps = tokio::spawn(longest_gap(stop.clone()));
        tokio::task::yield_now().await;
        let began = Instant::now();
        turns.begins(&line);
        let begin = began.elapsed();
        // The line.
        tokio::time::sleep(Duration::from_secs(1)).await;
        let ended = Instant::now();
        let note = turns.answered(&line).await;
        let end = ended.elapsed();
        stop.store(true, Ordering::Release);
        (first, note, gaps.await.unwrap(), begin, end)
    });
    assert!(!first.contains("fact"), "{first}");
    assert!(held_up < Duration::from_millis(150), "held up {held_up:?}");
    assert!(begin < Duration::from_millis(50), "{begin:?}");
    assert!(end < Duration::from_millis(100), "{end:?}");
    assert!(note.contains("fact 1 of the room"), "{note}");
    assert_eq!(memory.opens(), 2);
    eprintln!(
        "note ahead: begin_us={} end_us={} longest_gap_us={}",
        begin.as_micros(),
        end.as_micros(),
        held_up.as_micros()
    );
}

/// The reviewer-lead's P2 on #3016: a line begins before the lamp knows it
/// is for it. Lines someone else says in the room while the person talks to
/// the lamp, never answered, open and close no Days session and spend no
/// follow-up of anyone's; the person's next turn keeps their note.
#[test]
fn lines_never_answered_open_and_close_no_session_and_spend_no_follow_up() {
    let memory = Paced::new("unanswered", |_| Reply::After(0));
    let turns = Turns::new(&memory);
    turns.client.following_up();
    let p = crate::remember::person_id("Scope P");
    let (theirs, next) = (
        scoped(1, ScopeSnapshot::of("Scope P")),
        scoped(6, ScopeSnapshot::of("Scope P")),
    );
    let (first, counts, later) = runtime().block_on(async {
        turns.begins(&theirs);
        let first = turns.answered(&theirs).await;
        let counts = |memory: &Paced| {
            (
                memory.opens(),
                memory.count("session_close"),
                memory.count("follow_up"),
            )
        };
        let before = counts(&memory);
        for (request, snapshot) in [
            (2, ScopeSnapshot::of("Scope Q")),
            (3, ScopeSnapshot::room()),
            (4, ScopeSnapshot::of("Scope Q")),
            (5, ScopeSnapshot::held_of("Scope P")),
        ] {
            turns.begins(&scoped(request, snapshot));
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert_eq!(counts(&memory), before, "unanswered lines asked the memory");
        turns.begins(&next);
        (first, before, turns.answered(&next).await)
    });
    assert!(first.contains(&format!("fact 1 of {p}")), "{first}");
    assert_eq!(
        counts,
        (2, 0, 1),
        "one read, the person's follow-up asked once"
    );
    assert_eq!(later, first, "the person's note stands");
    assert_eq!(memory.opens(), 2);
    assert_eq!(memory.count("session_close"), 0);
    assert_eq!(memory.count("follow_up"), 1);
}

/// Reviewer-two's P2 on #3016: the memory counts a follow-up's offer as it
/// gives one, so a note read as a line begins asks none. The person's first
/// turn, answered, reads it with its note; a retry read as their next line
/// begins, a line never answered, asks the memory none; their next answered
/// turn takes that read, and only then is its follow-up read, and told.
#[test]
fn a_follow_up_is_read_only_once_its_turn_is_answered() {
    // Their first personal snapshot is refused, so their next line asks
    // again as it begins.
    let memory = Paced::new("follow", |count| match count {
        1 => Reply::RefusedAfter(0),
        _ => Reply::After(0),
    });
    let turns = Turns::new(&memory);
    turns.client.following_up();
    let line = |request| scoped(request, ScopeSnapshot::of("Scope P"));
    let (first, unanswered, last) = runtime().block_on(async {
        let first_line = line(1);
        turns.begins(&first_line);
        let first = turns.answered(&first_line).await;
        let follow_ups = memory.count("follow_up");
        turns.begins(&line(2));
        tokio::time::sleep(Duration::from_millis(200)).await;
        let unanswered = (memory.opens(), memory.count("follow_up") - follow_ups);
        let next = line(3);
        turns.begins(&next);
        (first, unanswered, turns.answered(&next).await)
    });
    assert!(
        first.contains(THREAD),
        "the answered turn's own read: {first}"
    );
    assert_eq!(unanswered, (4, 0), "read ahead, and no follow-up asked");
    assert!(
        last.contains(THREAD) && last.contains("fact 3 of"),
        "{last}"
    );
    assert_eq!(memory.count("follow_up"), 2);
    assert_eq!(memory.opens(), 4, "the read ahead is the one taken");
}

/// A note not in by what is left of its wait from the ask is late: the
/// turn waits only that (a run's first line's note, asked as its turn is
/// told, [`super::NOTE_TOLD_WITHIN`]), is told none, and the read goes on.
/// Its complete answer, 1.5 s after the ask, is the next turn's, with no
/// second read and no failure counted against the memory (the
/// reviewer-lead's P3 on #3016); a read that ends refused is asked again on
/// the next turn, not 30 s later.
#[test]
fn a_late_note_is_the_next_turns_and_a_failed_one_is_asked_again_on_the_next_turn() {
    for refused in [false, true] {
        let memory = Paced::new(
            if refused { "late-refused" } else { "late" },
            move |count| match count {
                0 if refused => Reply::RefusedAfter(1_500),
                0 => Reply::After(1_500),
                _ => Reply::After(0),
            },
        );
        let turns = Turns::new(&memory);
        let (first_line, second_line) = (
            scoped(1, ScopeSnapshot::room()),
            scoped(2, ScopeSnapshot::room()),
        );
        let (first, waited, second) = runtime().block_on(async {
            let start = Instant::now();
            turns.begins(&first_line);
            tokio::time::sleep(Duration::from_millis(200)).await;
            let ended = Instant::now();
            let first = turns.answered(&first_line).await;
            let waited = ended.elapsed();
            // The late read ends; the next line begins 2 s after the first.
            tokio::time::sleep_until((start + Duration::from_millis(2_000)).into()).await;
            turns.begins(&second_line);
            tokio::time::sleep(Duration::from_millis(200)).await;
            (first, waited, turns.answered(&second_line).await)
        });
        assert!(!first.contains("fact"), "refused={refused}: {first}");
        assert!(
            waited >= Duration::from_millis(800)
                && waited < super::NOTE_TOLD_WITHIN + Duration::from_millis(50),
            "refused={refused}: the turn's own ask waits its 900 ms, no more: {waited:?}"
        );
        let sessions = turns.sessions.lock().unwrap();
        if refused {
            assert!(second.contains("fact 1 of the room"), "{second}");
            assert_eq!(memory.opens(), 2);
        } else {
            assert!(second.contains("fact 0 of the room"), "{second}");
            assert_eq!(memory.opens(), 1, "the late answer is reused");
        }
        assert_eq!(sessions.failures, 0, "refused={refused}");
        assert!(sessions.note_retry_at.is_none(), "refused={refused}");
    }
}

/// A line begun as one person's and found by its end to be someone else's
/// (a face not theirs seen speaking it): the early answer is let go, and
/// the note asked again for the room.
#[test]
fn a_person_change_during_the_line_asks_again_for_whoever_it_is_at_its_end() {
    // Their own first snapshot is refused, so their next line asks again
    // as it begins.
    let memory = Paced::new("change", |count| match count {
        1 => Reply::RefusedAfter(0),
        _ => Reply::After(0),
    });
    let turns = Turns::new(&memory);
    let a = crate::remember::person_id("Scope A");
    let (first_line, line) = (
        scoped(1, ScopeSnapshot::of("Scope A")),
        scoped(2, ScopeSnapshot::of("Scope A")),
    );
    let note = runtime().block_on(async {
        turns.begins(&first_line);
        turns.answered(&first_line).await;
        turns.begins(&line);
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(memory.opens(), 4, "asked ahead as the line began");
        line.lease().narrow_to_room();
        turns.answered(&line).await
    });
    assert!(!note.contains(&a), "{note}");
    assert!(note.contains("fact 4 of the room"), "{note}");
    let opens: Vec<Value> = memory
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|request| request["op"] == "session_open")
        .cloned()
        .collect();
    assert_eq!(opens.len(), 5);
    assert_eq!(opens[3]["person"], a);
    assert!(opens[4]["person"].is_null());
    assert_ne!(opens[2]["session"], opens[4]["session"]);
}

/// A read still going on when the camera or the microphone closes or opens
/// is never told, and while it goes on, up to its 2 s, no other read
/// starts: the turns meanwhile keep the note they had. The first line
/// after it ends asks again.
#[test]
fn a_voided_read_is_never_told_and_holds_the_next_until_it_ends() {
    // Their first personal snapshot is refused; the next line's room
    // snapshot is held past the read's 2 s.
    let memory = Paced::new("void", |count| match count {
        1 => Reply::RefusedAfter(0),
        2 => Reply::After(2_200),
        _ => Reply::After(0),
    });
    let turns = Turns::new(&memory);
    let a = crate::remember::person_id("Scope A");
    let line = |request| scoped(request, ScopeSnapshot::of("Scope A"));
    let (first, held, still, after) = runtime().block_on(async {
        let first_line = line(1);
        turns.begins(&first_line);
        let first = turns.answered(&first_line).await;
        let start = Instant::now();
        let voided = line(2);
        turns.begins(&voided);
        tokio::time::sleep(Duration::from_millis(50)).await;
        turns.ahead.forget();
        let ended = Instant::now();
        let held = turns.answered(&voided).await;
        assert!(
            ended.elapsed() < Duration::from_millis(50),
            "not waited for"
        );
        tokio::time::sleep_until((start + Duration::from_millis(1_500)).into()).await;
        let blocked = line(3);
        turns.begins(&blocked);
        let still = turns.answered(&blocked).await;
        assert_eq!(memory.opens(), 3, "no read beside the voided one");
        tokio::time::sleep_until((start + Duration::from_millis(2_400)).into()).await;
        let next = line(4);
        turns.begins(&next);
        (first, held, still, turns.answered(&next).await)
    });
    assert!(
        first.contains("fact 0 of the room") && !first.contains(&a),
        "{first}"
    );
    assert_eq!(held, first, "the voided read isn't told");
    assert_eq!(still, first);
    assert!(after.contains(&format!("fact 4 of {a}")), "{after}");
    assert!(!after.contains("fact 2"), "{after}");
    assert_eq!(memory.opens(), 5);
}

/// A line held by a gap asks nothing and is told no note, and a revoked
/// scope is told nothing; the room's next line gets the room's note alone.
#[test]
fn a_held_line_and_a_revoked_scope_never_get_another_scopes_note() {
    let a = crate::remember::person_id("Scope A");
    // Held.
    {
        let memory = Paced::new("held", |_| Reply::After(0));
        let turns = Turns::new(&memory);
        let (theirs, held, room) = (
            scoped(1, ScopeSnapshot::of("Scope A")),
            scoped(2, ScopeSnapshot::held_of("Scope A")),
            scoped(3, ScopeSnapshot::room()),
        );
        let (told, during, after) = runtime().block_on(async {
            turns.begins(&theirs);
            let told = turns.answered(&theirs).await;
            turns.begins(&held);
            let during = turns.answered(&held).await;
            turns.begins(&room);
            (told, during, turns.answered(&room).await)
        });
        assert!(told.contains(&format!("fact 1 of {a}")), "{told}");
        assert!(during.is_empty(), "{during}");
        assert!(
            after.contains("fact 2 of the room") && !after.contains(&a),
            "{after}"
        );
        assert_eq!(memory.opens(), 3, "the held line asked nothing");
    }
    // Revoked.
    {
        let memory = Paced::new("revoked", |_| Reply::After(0));
        let turns = Turns::new(&memory);
        let (theirs, again, room) = (
            scoped(1, ScopeSnapshot::of("Scope A")),
            scoped(2, ScopeSnapshot::of("Scope A")),
            scoped(3, ScopeSnapshot::room()),
        );
        let (revoked, after) = runtime().block_on(async {
            turns.begins(&theirs);
            turns.answered(&theirs).await;
            turns.begins(&again);
            again.revoke();
            let revoked = turns.answered(&again).await;
            turns.begins(&room);
            (revoked, turns.answered(&room).await)
        });
        assert!(revoked.is_empty(), "{revoked}");
        assert!(
            after.contains("fact 2 of the room") && !after.contains(&a),
            "{after}"
        );
    }
}
