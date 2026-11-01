//! Voice's production Days client against the real socket/store. All content
//! is synthetic; no provider, credentials, hardware or network are involved.
use lamp_gemini::ToolCall;
use lamp_memory::{
    Options, Person, Server,
    days::{DATABASE, Days},
    days_protocol,
    protocol::Answer,
    server::{Memory, Stopper},
    store::{Day, LocalTime},
    tidy::{Category, Output, Proposal},
};
use lamp_voice::remember::{DaysClient, FORGET, RECALL, REMEMBER, gone, person_id};
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use std::{
    fs::{self, DirBuilder},
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const DAY: u64 = 86_400_000;
/// A session's deadline where a test checks what is opened, not how fast:
/// each open commits with a full fsync, which a loaded disk can take past
/// the voice's 300 ms (#948). The held-store test keeps the voice's own.
const SESSION: Duration = Duration::from_secs(10);
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Traced {
    store: Days,
    trace: Sender<(Value, Value)>,
    hold: Option<Receiver<()>>,
    refuse_forget: bool,
}

impl Memory for Traced {
    fn answer(&mut self, line: &[u8], now: u64) -> Answer {
        if let Some(hold) = self.hold.take() {
            hold.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        let request: Value = serde_json::from_slice(line).unwrap();
        let answer = if self.refuse_forget && request["op"] == "forget" {
            self.refuse_forget = false;
            Answer {
                line: json!({"ok":false,"error":"io"}).to_string(),
                log: "synthetic refused forget".into(),
                storage_fault: true,
                wrote: false,
                forgot: None,
            }
        } else {
            days_protocol::answer(&mut self.store, line, now)
        };
        let _ = self.trace.send((
            serde_json::from_slice(line).unwrap(),
            serde_json::from_str(&answer.line).unwrap(),
        ));
        answer
    }
}

struct Fixture {
    root: PathBuf,
    socket: PathBuf,
    now: u64,
    trace: Receiver<(Value, Value)>,
    stop: Stopper,
    worker: Option<JoinHandle<()>>,
    /// Held for the fixture's life, so real-store voice fixtures take turns.
    _turn: std::sync::MutexGuard<'static, ()>,
}

impl Fixture {
    fn new(seed: impl FnOnce(&mut Days, u64), hold: Option<Receiver<()>>) -> Self {
        Self::with_forget_refusal(seed, hold, false)
    }

    fn with_forget_refusal(
        seed: impl FnOnce(&mut Days, u64),
        hold: Option<Receiver<()>>,
        refuse_forget: bool,
    ) -> Self {
        let turn = crate::voice_store_turn();
        let root = std::env::temp_dir().join(format!(
            "lamp-mv-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let mut store = Days::open(
            Options {
                dir: root.join("store"),
                keep_days: 30,
                local: lamp_memory::store::LocalTime::Offset(
                    lamp_voice::local_time::offset_here().unwrap_or(0) / 60,
                ),
            },
            now,
        )
        .unwrap();
        seed(&mut store, now);
        let socket = root.join("m.sock");
        let server = Server::bind(&socket).unwrap();
        let stop = server.stopper();
        let (tx, trace) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut memory = Traced {
                store,
                trace: tx,
                hold,
                refuse_forget,
            };
            server.run(&mut memory, &mut || now, &mut |_| {}).unwrap();
        });
        Self {
            root,
            socket,
            now,
            trace,
            stop,
            worker: Some(worker),
            _turn: turn,
        }
    }

    fn client(&self) -> DaysClient {
        DaysClient::new(self.socket.clone()).unwrap()
    }

    fn next(&self, op: &str) -> (Value, Value) {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            let event = self
                .trace
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .unwrap();
            if event.0["op"] == op {
                return event;
            }
        }
    }

    fn count(&self, table: &str) -> u64 {
        assert!(matches!(
            table,
            "jobs" | "sessions" | "entries" | "follow_ups" | "follow_up_offers"
        ));
        let db = Connection::open_with_flags(
            self.root.join("store").join(DATABASE),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        db.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.stop();
        self.worker.take().unwrap().join().unwrap();
        fs::remove_dir_all(&self.root).unwrap();
    }
}

/// The room's snapshot for `session`, within [`SESSION`].
fn opened(client: &DaysClient, session: u64) -> Option<Value> {
    client
        .open_session_within(session, SESSION, None)
        .map(|(room, _)| room)
}

fn call(name: &str, args: Value) -> ToolCall {
    ToolCall {
        id: "voice-memory-test".into(),
        name: name.into(),
        args,
    }
}

#[test]
fn ranked_voice_recall_finds_an_earlier_day_and_respects_a_named_day() {
    let fixture = Fixture::new(
        |store, now| {
            let person = Person::everyone();
            store
                .append(
                    &person,
                    now - 2 * DAY,
                    "orchid orchid orchid",
                    "the blue pot",
                    &[],
                )
                .unwrap();
            store
                .append(
                    &person,
                    now - DAY,
                    "orchid among many plants and garden notes",
                    "a long general conversation with little detail about flowers",
                    &[],
                )
                .unwrap();
            for n in 0..10 {
                store
                    .append(
                        &person,
                        now - 1000 + n,
                        "today we discussed a different topic",
                        "nothing about flowers",
                        &[],
                    )
                    .unwrap();
            }
        },
        None,
    );
    let client = fixture.client();
    let (told, ok) = client.run(
        &call(RECALL, json!({"about":"orchid"})),
        Duration::from_millis(1500),
    );
    assert!(ok, "{told}");
    assert_eq!(told["found"][0]["source_id"], 1);
    assert_eq!(told["found"][0]["you_said"], "the blue pot");
    assert_eq!(told["found"][0]["when"], "2 days ago");
    assert_eq!(fixture.next("search").0["query"], "orchid");
    let (only, ok) = client.run(
        &call(RECALL, json!({"about":"orchid","days_ago":2})),
        Duration::from_millis(1500),
    );
    assert!(ok);
    assert_eq!(only["found"].as_array().unwrap().len(), 1);
    assert_eq!(only["found"][0]["source_id"], 1);
    let (none, ok) = client.run(
        &call(RECALL, json!({"about":"orchid","days_ago":0})),
        Duration::from_millis(1500),
    );
    assert!(ok && none["found"].as_array().unwrap().is_empty());
    assert!(
        client
            .run(&call(RECALL, json!({})), Duration::from_millis(1500))
            .1
    );
    assert_eq!(fixture.next("recall").0["query"], Value::Null);
}

#[test]
fn a_voice_session_receives_a_frozen_note_with_provenance_then_a_new_snapshot() {
    let fixture = Fixture::new(
        |store, now| {
            store
                .remember(&Person::everyone(), now - DAY, "coffee black")
                .unwrap();
        },
        None,
    );
    let client = fixture.client();
    let first = opened(&client, 1).unwrap();
    assert_eq!(first["source_ids"], json!([1]));
    assert!(first["note"].as_str().unwrap().contains("coffee black"));
    assert_eq!(first["invalidated"], false);
    let (told, ok) = client.run(
        &call(REMEMBER, json!({"text":"tea plain"})),
        Duration::from_millis(1500),
    );
    assert!(ok && told["remembered"] == true);
    assert_eq!(opened(&client, 1).unwrap(), first);
    let second = opened(&client, 2).unwrap();
    assert_ne!(first["session"], second["session"]);
    assert!(second["note"].as_str().unwrap().contains("tea plain"));
    fixture.next("session_close");
    assert_eq!(fixture.count("sessions"), 1);
    let another = opened(&fixture.client(), 1).unwrap();
    assert_ne!(
        another["session"], first["session"],
        "process-local client nonce prevents ID reuse"
    );
}

#[test]
fn a_remembered_fact_links_later_journal_echoes_and_forgetting_removes_them() {
    let fixture = Fixture::new(|_, _| {}, None);
    let client = fixture.client();
    assert!(
        client
            .run(
                &call(REMEMBER, json!({"text":"synthetic violet secret"})),
                Duration::from_millis(1500)
            )
            .1
    );
    let (_, added) = fixture.next("remember");
    let id = added["id"].as_u64().unwrap();
    assert!(client.finished_exchange(
        fixture.now,
        "what was the colour",
        "the synthetic violet secret"
    ));
    let (append, answer) = fixture.next("append_linked");
    assert_eq!(append["source_ids"], json!([id]));
    assert_eq!(answer["ok"], true);
    assert_eq!(fixture.count("entries"), 2);
    // The answer names what went only by the id the store removed
    // (#3504, Friday's N1 follow-up): here it was not shown, so it names
    // nothing.
    let (told, ok) = client.run(&call(FORGET, json!({})), Duration::from_millis(1500));
    assert!(ok && told["forgotten"] == true, "{told}");
    assert!(told.get("what_went").is_none(), "{told}");
    assert_eq!(fixture.count("entries"), 0);
    assert!(!client.finished_exchange(fixture.now, "late cached prose", "synthetic violet secret"));
    let (told, ok) = client.run(
        &call(RECALL, json!({"about":"violet"})),
        Duration::from_millis(1500),
    );
    assert!(ok && told["found"].as_array().unwrap().is_empty());
}

#[test]
fn repeated_real_conversation_ends_reach_memory_without_retaining_model_jobs() {
    let fixture = Fixture::new(|_, _| {}, None);
    let client = fixture.client();
    for n in 0..10 {
        let start = Instant::now();
        assert!(client.finished_exchange(
            fixture.now,
            &format!("synthetic question {n}"),
            "synthetic answer"
        ));
        assert!(client.end_conversation());
        assert!(
            start.elapsed() < Duration::from_millis(100),
            "foreground queued work must not wait on durable storage"
        );
        let (_, append) = fixture.next("append_linked");
        let (end, receipt) = fixture.next("conversation_end");
        assert_eq!(end["source_ids"], json!([append["id"]]));
        assert_eq!(end["submit"], true);
        assert_eq!(
            receipt,
            json!({"ok":true,"submitted":false,"reason":"model_unconfigured"})
        );
    }
    assert_eq!(fixture.count("jobs"), 0);
    assert_eq!(fixture.count("entries"), 10);
}

#[test]
fn a_held_real_store_costs_only_the_note_and_call_deadlines_and_cleans_a_late_note() {
    let (release, hold) = mpsc::channel();
    let fixture = Fixture::new(
        |store, now| {
            store
                .remember(&Person::everyone(), now, "coffee black")
                .unwrap();
        },
        Some(hold),
    );
    let client = fixture.client();
    let start = Instant::now();
    assert!(client.open_session(1, Duration::from_secs(10)).is_none());
    let note_elapsed = start.elapsed();
    assert!(
        note_elapsed >= Duration::from_millis(280) && note_elapsed < Duration::from_millis(450),
        "{note_elapsed:?}"
    );
    let start = Instant::now();
    assert_eq!(
        client.run(
            &call(RECALL, json!({"about":"coffee"})),
            Duration::from_secs(10)
        ),
        (gone(), false)
    );
    let call_elapsed = start.elapsed();
    assert!(
        call_elapsed >= Duration::from_millis(1400) && call_elapsed < Duration::from_millis(1650),
        "{call_elapsed:?}"
    );
    release.send(()).unwrap();
    fixture.next("session_open");
    fixture.next("session_close");
    assert_eq!(fixture.count("sessions"), 0);
    assert!(opened(&client, 2).is_some());
}

#[test]
fn a_missing_store_never_becomes_an_empty_or_successful_memory_answer() {
    let root = std::env::temp_dir().join(format!("lamp-mv-absent-{}", std::process::id()));
    assert!(!root.exists());
    let client = DaysClient::new(root).unwrap();
    let start = Instant::now();
    assert!(client.open_session(1, Duration::from_millis(300)).is_none());
    assert_eq!(
        client.run(
            &call(RECALL, json!({"about":"coffee"})),
            Duration::from_millis(1500)
        ),
        (gone(), false)
    );
    assert!(start.elapsed() < Duration::from_millis(300));
}

#[test]
fn a_timed_out_mutation_may_commit_but_cannot_seed_unlinked_journal_history() {
    for name in [REMEMBER, FORGET] {
        let (release, hold) = mpsc::channel();
        let fixture = Fixture::new(
            |store, now| {
                store
                    .remember(&Person::everyone(), now, "existing synthetic memory")
                    .unwrap();
            },
            Some(hold),
        );
        let client = fixture.client();
        // The caller gives up while the write is held. Its deadline covers
        // sending the line on a loaded host: with 50 ms a client held up
        // before it wrote gave up without sending, so nothing was held
        // (#1280). The hold, not the deadline, decides that it times out.
        assert_eq!(
            client.run(
                &call(name, json!({"text":"late synthetic memory"})),
                Duration::from_secs(1)
            ),
            (gone(), false)
        );
        assert!(!client.finished_exchange(
            fixture.now,
            "after an uncertain mutation",
            "a cached memory"
        ));
        release.send(()).unwrap();
        let (_, answer) = fixture.next(if name == REMEMBER {
            "remember"
        } else {
            "forget_last"
        });
        assert_eq!(
            answer["ok"], true,
            "the write really did commit after the caller gave up"
        );
        assert!(!client.finished_exchange(
            fixture.now,
            "even after the late receipt",
            "a cached memory"
        ));
        assert_eq!(
            fixture.count("entries"),
            if name == REMEMBER { 2 } else { 0 }
        );
    }
}

#[test]
fn deletion_invalidates_a_frozen_note_and_linked_writes_never_fall_back_to_plain_append() {
    let fixture = Fixture::new(
        |store, now| {
            store
                .remember(&Person::everyone(), now, "synthetic private fact")
                .unwrap();
        },
        None,
    );
    let client = fixture.client();
    assert!(opened(&client, 1).is_some());
    let deleted = lamp_voice::tools::exchange(
        &fixture.socket,
        &json!({"op":"forget_last","kind":"kept"}),
        Duration::from_millis(1500),
    )
    .unwrap();
    assert!(deleted["removed"].is_object());
    assert!(client.finished_exchange(fixture.now, "late echo", "synthetic private fact"));
    let (request, answer) = fixture.next("append_linked");
    assert_eq!(request["source_ids"], json!([1]));
    assert_eq!(answer["ok"], false);
    assert_eq!(fixture.count("entries"), 0);
    assert!(opened(&client, 1).is_none());
    // The server has replied to that request only after processing the failed
    // linked write; a plain-append retry would have made an entry by now.
    assert_eq!(fixture.count("entries"), 0);
}

/// What a person's bare "forget that" left of their kept lines and
/// observations, by text, after a session offered them a follow-up and,
/// when `asked`, the lamp asked about it, and, when `journalled`, the ask
/// and the person's reply went to the journal first. The follow-up is an
/// observation of the end-of-talk pass, or with `events` a kept line with a
/// day. With it, the follow-up's `rid` before the forget.
fn forget_that_after(events: bool, asked: bool, journalled: bool) -> (Fixture, Vec<String>, i64) {
    const ABOUT: &str = "Synthetic exam on the hill";
    const ASK: &str = "Good to see you! How did the exam on the hill go?";
    let id = person_id("Synthetic Q");
    let person = Person::new(&id).unwrap();
    let fixture = Fixture::new(
        |store, now| {
            store
                .remember(&person, now - 3 * DAY, "unrelated synthetic note")
                .unwrap();
            if events {
                let local =
                    LocalTime::Offset(lamp_voice::local_time::offset_here().unwrap_or(0) / 60);
                let on = Day::at(now - DAY, &local).unwrap().to_string();
                store
                    .remember_on(&person, now - 2 * DAY, ABOUT, &[], false, Some(&on))
                    .unwrap();
            } else {
                let said = store
                    .append(&person, now - DAY, ABOUT, "Good luck", &[])
                    .unwrap();
                let job = store
                    .conversation_end(&person, &format!("c{now}"), &[said])
                    .unwrap();
                let thread = Output {
                    observations: vec![Proposal {
                        text: ABOUT.into(),
                        category: Category::Thread,
                        source_ids: vec![said],
                        supersedes: None,
                        replaces: None,
                        on: None,
                    }],
                };
                store.tidy_apply(&person, &job.job, &thread).unwrap();
            }
        },
        None,
    );
    let client = fixture.client();
    client.following_up();
    if events {
        client.telling_events();
    }
    let note = client.told_for_session_within(1, Some(&id), false, SESSION);
    assert!(note.contains(ABOUT), "offered: {note}");
    let offered = fixture.next("follow_up").1["follow_up"]["id"].clone();
    assert_eq!(fixture.count("follow_up_offers"), 1);
    if asked {
        client.mark_if_asked(ASK, Some(&id));
        assert_eq!(fixture.next("follow_up_asked").1["ok"], true);
        assert_eq!(fixture.count("follow_ups"), 1);
    }
    if journalled {
        // As the voice hands a finished exchange on: marked, then journalled.
        let at = fixture.now;
        assert!(client.finished_exchange_of(1, at, "Hello lamp.", ASK, Some(&id)));
        assert_eq!(fixture.next("append_linked").1["ok"], true);
        client.mark_if_asked("Glad it went well.", Some(&id));
        assert!(client.finished_exchange_of(
            2,
            at + 1,
            "The exam on the hill went well.",
            "Glad it went well.",
            Some(&id),
        ));
        assert_eq!(fixture.next("append_linked").1["ok"], true);
    }
    let db = Connection::open_with_flags(
        fixture.root.join("store").join(DATABASE),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let rid: i64 = db
        .query_row(
            "SELECT rid FROM entries WHERE person=?1 AND id=?2",
            rusqlite::params![id.as_str(), offered.as_i64().unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    let said: u64 = db
        .query_row(
            "SELECT count(*) FROM entries WHERE kind='exchange' AND (heard LIKE '%hill%' OR said LIKE '%hill%')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    // The end-of-talk pass's evidence, without `events`, and the two new ones.
    assert_eq!(said, u64::from(!events) + 2 * u64::from(journalled));
    drop(db);
    let (told, ok) = client.run_for(
        &call(FORGET, json!({})),
        Duration::from_millis(1500),
        Some(&id),
    );
    assert!(ok, "{told}");
    assert_eq!(told["forgotten"], true, "{told}");
    if asked {
        let (request, _) = fixture.next("forget");
        assert_eq!(request["id"], offered);
        assert_eq!(request["person"], id.as_str());
    } else {
        assert_eq!(fixture.next("forget_last").0["kind"], "kept");
    }
    let db = Connection::open_with_flags(
        fixture.root.join("store").join(DATABASE),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let left = db
        .prepare("SELECT text FROM entries WHERE kind IN ('kept','observation') ORDER BY id")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<String>, _>>()
        .unwrap();
    drop(db);
    if journalled {
        // The end-of-talk pass is given what the conversation still has:
        // the store no longer has the exchanges it names, so nothing: it
        // closes with nothing to observe, not refused (N12).
        assert!(client.end_conversation());
        let (end, answer) = fixture.next("conversation_end");
        assert_eq!(end["source_ids"].as_array().map(Vec::len), Some(2), "{end}");
        assert!(answer.get("input").is_none(), "{answer}");
        assert_eq!(answer["ok"], true, "{answer}");
        assert_eq!(answer["reason"], "forgotten", "{answer}");
    }
    (fixture, left, rid)
}

/// The first builder, 03:29: "forget that" right after the lamp asked
/// about a follow-up forgets the entry it was about, kept by the
/// end-of-talk pass as an observation or by `remember`, and its follow-up
/// and offer rows; an older unrelated kept line stays.
#[test]
fn forget_that_right_after_a_follow_up_forgets_the_entry_it_was_about() {
    for events in [false, true] {
        let (fixture, left, _) = forget_that_after(events, true, false);
        assert_eq!(left, ["unrelated synthetic note"], "events {events}");
        assert_eq!(fixture.count("follow_ups"), 0, "events {events}");
        assert_eq!(fixture.count("follow_up_offers"), 0, "events {events}");
    }
}

/// A follow-up observation and an unrelated keep can cite one exchange:
/// this is the durable shape produced by `remember_from` with FORGET_ALL.
/// Exercise the real Voice client, protocol and store, not a synthetic reply.
/// Second's second P2 on #3130, from the store a voice is updated with.
#[test]
fn a_follow_up_forget_preserves_an_unrelated_keep_with_shared_evidence() {
    const ABOUT: &str = "Synthetic exam on the hill";
    const ASK: &str = "How did the exam on the hill go?";
    const KEEP: &str = "The synthetic lantern inventory is complete.";
    let id = person_id("Synthetic shared-evidence caller");
    let person = Person::new(&id).unwrap();
    let mut seeded = None;
    let fixture = Fixture::new(
        |store, now| {
            let said = store
                .append(
                    &person,
                    now - DAY,
                    "Synthetic exam on the hill. The synthetic lantern inventory is complete.",
                    "Good luck with the exam.",
                    &[],
                )
                .unwrap();
            let kept = store
                .remember_from(&person, now - DAY + 1, KEEP, &[said])
                .unwrap();
            let job = store
                .conversation_end(&person, &format!("shared{now}"), &[said])
                .unwrap();
            let noticed = store
                .tidy_apply(
                    &person,
                    &job.job,
                    &Output {
                        observations: vec![Proposal {
                            text: ABOUT.into(),
                            category: Category::Thread,
                            source_ids: vec![said],
                            supersedes: None,
                            replaces: None,
                            on: None,
                        }],
                    },
                )
                .unwrap();
            assert_eq!(noticed.len(), 1);
            let noticed = noticed[0];
            for entry in [kept, noticed] {
                assert_eq!(
                    store.record(&person, entry).unwrap().unwrap().source_ids,
                    [said],
                    "both entries must really share the same evidence"
                );
            }
            seeded = Some((said, kept, noticed));
        },
        None,
    );
    let (said, kept, noticed) = seeded.unwrap();
    let client = fixture.client();
    client.following_up();
    let note = client.told_for_session_within(1, Some(&id), false, SESSION);
    assert!(note.contains(ABOUT), "offered: {note}");
    assert_eq!(fixture.next("follow_up").1["follow_up"]["id"], noticed);
    client.mark_if_asked(ASK, Some(&id));
    assert_eq!(fixture.next("follow_up_asked").1["ok"], true);
    let (told, ok) = client.run_for(
        &call(FORGET, json!({})),
        Duration::from_millis(1500),
        Some(&id),
    );
    assert!(ok, "{told}");
    assert_eq!(told["forgotten"], true, "{told}");
    // #3113's older-memory probe first (Second's second P2 on #3130): this
    // memory lists, so the forget is sent. An older one refuses the probe
    // and is sent no forget (the voice's unit test).
    let (probe, listed) = fixture.next("kept");
    assert_eq!(probe["observations"], true);
    assert_eq!(listed["ok"], true, "{listed}");
    assert_eq!(
        fixture.next("forget").0,
        json!({"op":"forget","person":id.as_str(),"id":noticed,"selective":true})
    );
    let db = Connection::open_with_flags(
        fixture.root.join("store").join(DATABASE),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let left = db
        .prepare("SELECT id,kind,text FROM entries WHERE person=?1 ORDER BY id")
        .unwrap()
        .query_map([id.as_str()], |row| {
            Ok((
                row.get::<_, u64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        left,
        [(kept, "kept".to_owned(), KEEP.to_owned())],
        "forgetting follow-up {noticed} and its evidence {said} must preserve unrelated keep {kept}"
    );
    let links: u64 = db
        .query_row("SELECT count(*) FROM sources", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        links, 0,
        "the kept row must not retain a dangling evidence link"
    );
}

/// With no follow-up asked, "forget that" is the newest kept line, as
/// before: an observation is never it.
#[test]
fn forget_that_with_no_follow_up_asked_still_forgets_the_newest_kept_line() {
    let (_, left, _) = forget_that_after(false, false, false);
    assert_eq!(left, ["Synthetic exam on the hill"]);
    let (_, left, _) = forget_that_after(true, false, false);
    assert_eq!(left, ["unrelated synthetic note"]);
}

/// Reviewer-two's P2 on #3130: the lamp's ask and the person's reply, in
/// the journal before "forget that", go with the follow-up, so neither the
/// end-of-talk pass nor any table holds the topic's words or its row after.
#[test]
fn forget_that_after_a_follow_up_leaves_nothing_of_its_conversation() {
    for events in [false, true] {
        let (fixture, _, rid) = forget_that_after(events, true, true);
        let db = Connection::open_with_flags(
            fixture.root.join("store").join(DATABASE),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let tables = db
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<String>, _>>()
            .unwrap();
        for table in [
            "entries",
            "sources",
            "jobs",
            "sessions",
            "follow_ups",
            "follow_up_offers",
            "event_days",
            "search",
        ] {
            assert!(tables.iter().any(|t| t == table), "{table} in {tables:?}");
        }
        for table in &tables {
            let columns = db
                .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
                .unwrap()
                .query_map([], |row| row.get(0))
                .unwrap()
                .collect::<Result<Vec<String>, _>>()
                .unwrap();
            for column in columns {
                let words: u64 = db
                    .query_row(
                        &format!(
                            "SELECT count(*) FROM \"{table}\" WHERE instr(lower(CAST(\"{column}\" AS TEXT)),'exam')>0 OR instr(lower(CAST(\"{column}\" AS TEXT)),'hill')>0"
                        ),
                        [],
                        |row| row.get(0),
                    )
                    .unwrap();
                assert_eq!(
                    words, 0,
                    "events {events}: {table}.{column} holds the topic"
                );
                if ["rid", "child", "parent", "newer", "older"].contains(&column.as_str()) {
                    let row: u64 = db
                        .query_row(
                            &format!("SELECT count(*) FROM \"{table}\" WHERE \"{column}\"=?1"),
                            [rid],
                            |row| row.get(0),
                        )
                        .unwrap();
                    assert_eq!(row, 0, "events {events}: {table}.{column} holds rid {rid}");
                }
            }
        }
        let left: u64 = db
            .query_row("SELECT count(*) FROM search WHERE rowid=?1", [rid], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(left, 0, "events {events}: search");
    }
}

#[test]
fn a_refused_follow_up_forget_survives_the_next_turn_note_refresh() {
    const ABOUT: &str = "Synthetic exam on the hill";
    const ASK: &str = "How did the exam on the hill go?";
    const KEEP: &str = "The synthetic lantern inventory is complete.";
    let id = person_id("Synthetic shared-evidence caller");
    let person = Person::new(&id).unwrap();
    let mut seeded = None;
    let fixture = Fixture::with_forget_refusal(
        |store, now| {
            let said = store
                .append(
                    &person,
                    now - DAY,
                    "Synthetic exam on the hill. The synthetic lantern inventory is complete.",
                    "Good luck with the exam.",
                    &[],
                )
                .unwrap();
            let kept = store
                .remember_from(&person, now - DAY + 1, KEEP, &[said])
                .unwrap();
            let job = store
                .conversation_end(&person, &format!("shared{now}"), &[said])
                .unwrap();
            let noticed = store
                .tidy_apply(
                    &person,
                    &job.job,
                    &Output {
                        observations: vec![Proposal {
                            text: ABOUT.into(),
                            category: Category::Thread,
                            source_ids: vec![said],
                            supersedes: None,
                            replaces: None,
                            on: None,
                        }],
                    },
                )
                .unwrap();
            assert_eq!(noticed.len(), 1);
            let noticed = noticed[0];
            for entry in [kept, noticed] {
                assert_eq!(
                    store.record(&person, entry).unwrap().unwrap().source_ids,
                    [said],
                    "both entries must really share the same evidence"
                );
            }
            seeded = Some((said, kept, noticed));
        },
        None,
        true,
    );
    let (said, kept, noticed) = seeded.unwrap();
    let client = fixture.client();
    client.following_up();
    let note = client.told_for_session_within(1, Some(&id), false, SESSION);
    assert!(note.contains(ABOUT), "offered: {note}");
    assert_eq!(fixture.next("follow_up").1["follow_up"]["id"], noticed);
    client.mark_if_asked(ASK, Some(&id));
    assert_eq!(fixture.next("follow_up_asked").1["ok"], true);
    let (told, ok) = client.run_for(
        &call(FORGET, json!({})),
        Duration::from_millis(1500),
        Some(&id),
    );
    assert!(
        !ok && told["forgotten"] != true,
        "the first write must be refused: {told}"
    );
    assert_eq!(fixture.next("kept").0["observations"], true);
    let first = fixture.next("forget");
    assert_eq!(
        first.0,
        json!({"op":"forget","person":id.as_str(),"id":noticed,"selective":true})
    );
    assert_eq!(first.1, json!({"ok":false,"error":"io"}));
    client.mark_if_asked("Sorry, I could not forget that.", Some(&id));
    // Second-route Sessions::tidy invalidates the note after changing();
    // ask_if_due opens the next numbered session before the next turn.
    let refreshed = client.told_for_session_within(2, Some(&id), false, SESSION);
    assert!(
        refreshed.contains(KEEP),
        "real next-session snapshot must be adopted: {refreshed}"
    );
    let retry = client.run_for(
        &call(FORGET, json!({})),
        Duration::from_millis(1500),
        Some(&id),
    );
    let trace: Vec<_> = fixture.trace.try_iter().collect();
    let forgets: Vec<_> = trace
        .iter()
        .filter(|(asked, _)| {
            asked["op"]
                .as_str()
                .is_some_and(|op| op.starts_with("forget"))
        })
        .collect();
    let db = Connection::open_with_flags(
        fixture.root.join("store").join(DATABASE),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let left = db
        .prepare("SELECT id,kind,text FROM entries WHERE person=?1 ORDER BY id")
        .unwrap()
        .query_map([id.as_str()], |row| {
            Ok((
                row.get::<_, u64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        left.iter().any(|row| row.0 == kept && row.2 == KEEP),
        "a refused follow-up {noticed} retry after its automatic note refresh deleted unrelated keep {kept}: left={left:?}; evidence={said}; retry={retry:?}; trace={trace:?}"
    );
    assert_eq!(forgets.len(), 1, "the retry sends one forget: {forgets:?}");
    assert_eq!(
        forgets[0].0,
        json!({"op":"forget","person":id.as_str(),"id":noticed,"selective":true}),
        "the retry forgets the same id"
    );
    assert!(retry.1 && retry.0["forgotten"] == true, "retry: {retry:?}");
    assert!(!left.iter().any(|row| row.0 == noticed), "left={left:?}");
}
