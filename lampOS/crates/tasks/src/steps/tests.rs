use super::*;
use std::time::SystemTime;

/// PLACEHOLDER events (every word invented), in the shapes read on 52e6:
/// one `transcript_events.event_json` a line.
const FIXTURE: &str = include_str!("../../tests/fixtures/openclaw-session-research.jsonl");
/// Harness's agent ids are uuids (`agent:main:harness-<uuid>` on 52e6).
const AGENT: &str = "1ac58eb4-9d87-4b2a-8c1e-0f2d3c4b5a69";
const TASK: u64 = 7;
/// Later than every event in the fixture.
const WALL: u64 = 1_794_812_500_000;

/// OpenClaw's agent database as 52e6 has it, with the columns lampOS reads.
const TABLES: &str = "
CREATE TABLE session_nodes(session_key TEXT PRIMARY KEY, current_session_id TEXT,
  entry_json TEXT, session_started_at INTEGER);
CREATE TABLE transcript_events(session_id TEXT, seq INTEGER, event_json TEXT,
  created_at INTEGER, event_zstd BLOB, event_utf8_bytes INTEGER,
  PRIMARY KEY (session_id, seq));
";

struct Lamp {
    root: tempfile::TempDir,
    db: Connection,
}

impl Lamp {
    /// OpenClaw's state with Harness's session for [`AGENT`] (id `s1`) and
    /// the lamp's own (`other`, the fixture in it), `text` already in `s1`.
    fn with(text: &str) -> Self {
        Self::with_mode(text, false)
    }
    fn with_mode(text: &str, wal: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("state/agents/main/agent");
        fs::create_dir_all(&folder).unwrap();
        let db = Connection::open(folder.join("openclaw-agent.sqlite")).unwrap();
        // A test's writes need not reach the disk: each insert is quick.
        db.pragma_update(None, "synchronous", "off").unwrap();
        if wal {
            db.pragma_update(None, "journal_mode", "wal").unwrap();
        }
        db.execute_batch(TABLES).unwrap();
        db.execute(
            "INSERT INTO session_nodes VALUES ('agent:main:main', 'other', '{}', 1), (?1, 's1', '{}', 1)",
            params![format!("agent:main:harness-{AGENT}")],
        )
        .unwrap();
        let lamp = Self { root, db };
        lamp.insert("other", FIXTURE);
        lamp.append(text);
        lamp
    }
    /// The state folder as it really is: on a Mac, `TMPDIR` is under
    /// `/var`, a link.
    fn state(&self) -> PathBuf {
        self.root.path().canonicalize().unwrap().join("state")
    }
    fn database(&self) -> PathBuf {
        self.state().join("agents/main").join(DATABASE)
    }
    fn steps(&self) -> Steps {
        switched(Some("on"), vec![self.state()]).unwrap()
    }
    /// Each line of `text` as the session's next event.
    fn insert(&self, session: &str, text: &str) {
        for line in text.lines().filter(|line| !line.is_empty()) {
            self.db
                .execute(
                    "INSERT INTO transcript_events (session_id, seq, event_json, created_at) \
                     SELECT ?1, COALESCE(MAX(seq) + 1, 0), ?2, 1794812400000 \
                     FROM transcript_events WHERE session_id = ?1",
                    params![session, line],
                )
                .unwrap();
        }
    }
    fn append(&self, text: &str) {
        self.insert("s1", text);
    }
}

fn watched(accepted_ago_ms: Option<u64>) -> Vec<Watched> {
    vec![task(TASK, accepted_ago_ms)]
}

fn task(task_id: u64, accepted_ago_ms: Option<u64>) -> Watched {
    Watched {
        task_id,
        machine_id: "lamp".into(),
        agent_id: AGENT.into(),
        engine: "openclaw".into(),
        accepted_ago_ms,
    }
}

/// The task watched on an empty session, then `text` written by its agent,
/// then read: the lamp, its reader and the steps.
fn own_with(text: &str, accepted_ago_ms: Option<u64>) -> (Lamp, Steps, Vec<TaskStep>) {
    let lamp = Lamp::with("");
    let mut reader = lamp.steps();
    assert!(reader.tick(&watched(accepted_ago_ms), 0, WALL).is_empty());
    lamp.append(text);
    let steps = reader.tick(&watched(accepted_ago_ms), EVERY_MS, WALL);
    (lamp, reader, steps)
}

fn own(text: &str) -> Vec<TaskStep> {
    own_with(text, None).2
}

/// The latest step of each number, as a reader keeps them.
fn latest(steps: &[TaskStep]) -> BTreeMap<u32, TaskStep> {
    steps.iter().map(|step| (step.n, step.clone())).collect()
}

fn call(id: &str, tool: &str, at: u64) -> String {
    format!(
        r#"{{"type":"message","message":{{"role":"assistant","content":[{{"type":"toolCall","id":"{id}","name":"{tool}","arguments":{{"command":"ls"}}}}],"stopReason":"toolUse","timestamp":{at}}}}}"#
    ) + "\n"
}

fn result(id: &str, text: &str, at: u64) -> String {
    format!(
        r#"{{"type":"message","message":{{"role":"toolResult","toolCallId":"{id}","toolName":"exec","content":[{{"type":"text","text":"{text}"}}],"isError":false,"timestamp":{at}}}}}"#
    ) + "\n"
}

#[test]
fn the_fixtures_tool_calls_give_that_many_steps_with_their_tools() {
    let steps = own(FIXTURE);
    let latest = latest(&steps);
    let calls: Vec<&TaskStep> = latest
        .values()
        .filter(|step| step.kind == StepKind::Call)
        .collect();
    let tools: Vec<&str> = calls.iter().map(|step| step.tool.as_str()).collect();
    assert_eq!(tools, ["web_search", "exec", "web_fetch"]);
    assert_eq!(
        calls[0].args,
        r#"{"query":"desk lamp posture tracking paper 2026"}"#
    );
    assert_eq!(
        calls[1].args,
        "curl -s https://example.org/papers/posturelamp.pdf -o /tmp/p.pdf"
    );
    assert_eq!(calls[1].ok, Some(false), "the exec failed");
    assert_eq!(calls[0].ok, Some(true));
    assert!(calls[0].result.starts_with("1. PostureLamp"));
    // Each call is on the bus twice: running, then with its result.
    for call in &calls {
        let sent: Vec<&TaskStep> = steps.iter().filter(|step| step.n == call.n).collect();
        assert_eq!(sent.len(), 2);
        assert_eq!((sent[0].ok, sent[0].result.as_str()), (None, ""));
    }
    let replies: Vec<&str> = latest
        .values()
        .filter(|step| step.kind == StepKind::Reply)
        .map(|step| step.result.as_str())
        .collect();
    assert_eq!(replies.len(), 2);
    assert!(replies[1].starts_with("Found three"));
    assert_eq!(latest.keys().copied().collect::<Vec<_>>(), [1, 2, 3, 4, 5]);
    assert!(
        steps
            .iter()
            .all(|step| step.task == TASK && step.engine == "openclaw")
    );
    assert!(steps.iter().all(|step| step.check().is_ok()));
    assert_eq!(
        steps[0].at_unix_ms, 1_794_812_401_200,
        "the message's own time"
    );
    // The person's ask is never a step.
    assert!(
        steps
            .iter()
            .all(|step| !step.result.contains("track posture"))
    );
}

#[test]
fn a_result_is_cut_at_120_bytes_on_a_whole_character() {
    let long = "é".repeat(100);
    let steps = own(&(call("c1", "exec", 1) + &result("c1", &long, 2)));
    let done = steps.last().unwrap();
    assert_eq!(done.result, "é".repeat(60));
    // The fixture's fetched page has a dash and a curly quote past 120.
    let fetched = latest(&own(FIXTURE))
        .into_values()
        .find(|step| step.tool == "web_fetch")
        .unwrap();
    assert!(fetched.result.len() <= TASK_STEP_MOST_RESULT_BYTES);
    assert!(fetched.result.len() > 100);
    assert!(fetched.result.starts_with("PostureLamp: a desk lamp"));
}

#[test]
fn at_most_50_steps_a_task_the_rest_counted() {
    let text: String = (0..60)
        .map(|i| call(&format!("c{i}"), "exec", 10 + i) + &result(&format!("c{i}"), "ok", 10 + i))
        .collect();
    let (_lamp, mut reader, steps) = own_with(&text, None);
    let numbers = latest(&steps);
    assert_eq!(numbers.len(), 50);
    assert_eq!(numbers.keys().max(), Some(&50));
    assert_eq!(reader.tails[&TASK].dropped, 10);
    assert!(reader.tick(&watched(None), 2 * EVERY_MS, WALL).is_empty());
}

#[test]
fn switched_off_unless_exactly_on() {
    for value in [
        None,
        Some("off"),
        Some("ON"),
        Some(" on"),
        Some("1"),
        Some(""),
    ] {
        assert!(
            switched(value, vec![PathBuf::from("/x")]).is_none(),
            "{value:?}"
        );
    }
    assert!(switched(Some("on"), vec![PathBuf::from("/x")]).is_some());
}

/// Every path under `root` with its length and time changed.
fn snapshot(root: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
    let mut all = Vec::new();
    let mut folders = vec![root.to_owned()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            let meta = fs::metadata(&path).unwrap();
            if meta.is_dir() {
                folders.push(path.clone());
            }
            all.push((path, meta.len(), meta.modified().unwrap()));
        }
    }
    all.sort();
    all
}

#[test]
fn nothing_is_written_to_disk() {
    let lamp = Lamp::with("");
    let mut reader = lamp.steps();
    reader.tick(&watched(None), 0, WALL);
    lamp.append(FIXTURE);
    let before = snapshot(lamp.root.path());
    assert!(!reader.tick(&watched(None), EVERY_MS, WALL).is_empty());
    reader.tick(&watched(None), 2 * EVERY_MS, WALL);
    // Let go: read once more, then forgotten.
    reader.tick(&[], 3 * EVERY_MS, WALL);
    assert!(reader.tails.is_empty());
    assert_eq!(snapshot(lamp.root.path()), before);
}

/// OpenClaw writes in WAL with its connection open: read-only reads go on.
#[test]
fn a_database_in_wal_is_read_while_its_writer_has_it_open() {
    let lamp = Lamp::with_mode("", true);
    let mut reader = lamp.steps();
    reader.tick(&watched(None), 0, WALL);
    lamp.append(&call("c1", "exec", 1));
    assert_eq!(reader.tick(&watched(None), EVERY_MS, WALL).len(), 1);
}

#[test]
fn a_read_goes_on_after_its_last_event() {
    let (lamp, mut reader, steps) = own_with(FIXTURE, None);
    assert_eq!(latest(&steps).len(), 5);
    assert!(reader.tick(&watched(None), 2_000, WALL).is_empty());
    lamp.append(&call("c9", "exec", 99));
    lamp.append(&result("c9", "done", 100));
    let steps = reader.tick(&watched(None), 3_000, WALL);
    assert_eq!(steps.len(), 2);
    assert!(steps.iter().all(|step| step.n == 6 && step.tool == "exec"));
    assert_eq!(steps[1].result, "done");
}

#[test]
fn a_step_is_on_the_bus_within_2_s_of_its_event() {
    let lamp = Lamp::with("");
    let mut reader = lamp.steps();
    assert!(reader.tick(&watched(None), 0, WALL).is_empty());
    // Written just after a read: the worst case.
    let written_ms = 1;
    lamp.append(&call("c1", "exec", 5));
    let mut now = written_ms;
    let published = loop {
        if !reader.tick(&watched(None), now, WALL).is_empty() || now > 10_000 {
            break now;
        }
        now += 50;
    };
    assert!(
        published - written_ms <= 2_000,
        "{} ms",
        published - written_ms
    );
}

#[test]
fn a_reply_an_error_and_unknown_records() {
    let text = String::from("not json\n")
        + r#"{"type":"custom","data":{}}"#
        + "\n"
        + r#"{"type":"message","message":{"role":"user","content":"Private words.","timestamp":2}}"#
        + "\n"
        + r#"{"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"Done, three papers."}],"stopReason":"stop","timestamp":3}}"#
        + "\n"
        + r#"{"type":"message","message":{"role":"assistant","content":[],"stopReason":"error","errorMessage":"429 rate limited","timestamp":4}}"#
        + "\n"
        + r#"{"type":"message","message":{"role":"toolResult","toolCallId":"never","content":[],"timestamp":5}}"#
        + "\n";
    let steps = own(&text);
    assert_eq!(steps.len(), 2);
    assert_eq!(
        (
            steps[0].kind,
            steps[0].tool.as_str(),
            steps[0].result.as_str(),
            steps[0].ok
        ),
        (StepKind::Reply, "", "Done, three papers.", None)
    );
    assert_eq!(
        (steps[1].kind, steps[1].result.as_str(), steps[1].ok),
        (StepKind::Error, "429 rate limited", Some(false))
    );
    // A reply and an error have no tool, and are valid as they are.
    assert!(steps.iter().all(|step| step.check().is_ok()));
}

/// With no time in the message, the row's own `created_at` is its time;
/// with neither, the event is not taken for this task's.
#[test]
fn an_event_with_no_time_of_its_own_is_skipped() {
    let lamp = Lamp::with("");
    let mut reader = lamp.steps();
    reader.tick(&watched(None), 0, WALL);
    let untimed = r#"{"type":"message","message":{"role":"assistant","content":"No time."}}"#;
    lamp.db
        .execute(
            "INSERT INTO transcript_events (session_id, seq, event_json, created_at) VALUES ('s1', 1, ?1, NULL), ('s1', 2, ?2, 1794812400)",
            params![untimed, r#"{"type":"message","message":{"role":"assistant","content":"Row time."}}"#],
        )
        .unwrap();
    let steps = reader.tick(&watched(None), EVERY_MS, WALL);
    assert_eq!(steps.len(), 1);
    assert_eq!(
        (steps[0].result.as_str(), steps[0].at_unix_ms),
        ("Row time.", 1_794_812_400_000)
    );
}

#[test]
fn events_before_the_hand_over_are_an_earlier_tasks() {
    // Handed over 1 s before WALL: after every event in the fixture.
    assert!(own_with(FIXTURE, Some(1_000)).2.is_empty());
    assert_eq!(latest(&own_with(FIXTURE, Some(WALL)).2).len(), 5);
}

/// #7258's one pane: an ask already in the session when the task is first
/// watched is another task's, even one handed over in an earlier boot.
#[test]
fn an_ask_already_in_the_session_is_never_this_tasks() {
    let lamp = Lamp::with(FIXTURE);
    let mut reader = lamp.steps();
    assert!(reader.tick(&watched(None), 0, WALL).is_empty());
    assert!(reader.tick(&watched(None), EVERY_MS, WALL).is_empty());
    lamp.append(&call("mine", "exec", 50));
    let steps = reader.tick(&watched(None), 2 * EVERY_MS, WALL);
    assert_eq!(steps.len(), 1);
    assert_eq!((steps[0].n, steps[0].tool.as_str()), (1, "exec"));
}

/// Two asks in one session: each task gets only its own steps.
#[test]
fn two_asks_in_one_session_each_get_only_their_own() {
    let lamp = Lamp::with("");
    let mut reader = lamp.steps();
    let first = [task(7, None)];
    let both = [task(7, None), task(8, None)];
    assert!(reader.tick(&first, 0, WALL).is_empty());
    lamp.append(&(call("a1", "first_tool", 10) + &result("a1", "first words", 11)));
    // The second ask is handed over before the first is read.
    assert!(reader.tick(&both, EVERY_MS / 2, WALL).is_empty());
    lamp.append(&(call("b1", "second_tool", 20) + &result("b1", "second words", 21)));
    let steps = reader.tick(&both, EVERY_MS, WALL);
    let of = |task: u64| -> Vec<(String, String)> {
        steps
            .iter()
            .filter(|step| step.task == task)
            .map(|step| (step.tool.clone(), step.result.clone()))
            .collect()
    };
    assert_eq!(
        of(7),
        [
            ("first_tool".to_owned(), String::new()),
            ("first_tool".to_owned(), "first words".to_owned())
        ]
    );
    assert_eq!(
        of(8),
        [
            ("second_tool".to_owned(), String::new()),
            ("second_tool".to_owned(), "second words".to_owned())
        ]
    );
    // Later events are the newer task's only.
    lamp.append(&call("b2", "later_tool", 30));
    let steps = reader.tick(&both, 2 * EVERY_MS, WALL);
    assert_eq!(steps.len(), 1);
    assert_eq!((steps[0].task, steps[0].tool.as_str()), (8, "later_tool"));
}

/// The fixture's `call_1` toolResult event, compressed with the `zstd` CLI
/// (`zstd -19`, one frame, as OpenClaw keeps `event_zstd`) on the Mac.
const TOOL_RESULT_ZST: &[u8] = include_bytes!("../../tests/fixtures/openclaw-event-toolresult.zst");
/// A toolResult event of 70,176 bytes ("word " 14,000 times), compressed
/// the same way: past 64 KiB decoded.
const OVER_64K_ZST: &[u8] = include_bytes!("../../tests/fixtures/openclaw-event-over-64k.zst");

/// The task's session with the fixture's `call_1` call, then a row kept
/// compressed as `blob`, then a call: the steps and the skipped count.
fn compressed(blob: &[u8]) -> (Vec<TaskStep>, u64) {
    let lamp = Lamp::with("");
    let mut reader = lamp.steps();
    reader.tick(&watched(None), 0, WALL);
    let first_call = FIXTURE
        .lines()
        .find(|line| line.contains(r#""id":"call_1""#))
        .unwrap();
    lamp.append(first_call);
    lamp.db
        .execute(
            "INSERT INTO transcript_events (session_id, seq, event_json, created_at, event_zstd) VALUES ('s1', 1, NULL, 1, ?1)",
            params![blob],
        )
        .unwrap();
    lamp.append(&call("c2", "exec", 1_794_812_404_000));
    let steps = reader.tick(&watched(None), EVERY_MS, WALL);
    (steps, reader.tails[&TASK].undecoded)
}

/// 27 of 115 events on 52e6 are kept compressed (the large tool results).
#[test]
fn a_compressed_event_is_decoded() {
    let (steps, undecoded) = compressed(TOOL_RESULT_ZST);
    assert_eq!(undecoded, 0);
    let done = steps
        .iter()
        .find(|step| step.tool == "web_search" && step.ok.is_some())
        .unwrap();
    assert_eq!((done.tool.as_str(), done.ok), ("web_search", Some(true)));
    assert!(done.result.starts_with("1. PostureLamp"));
}

#[test]
fn a_compressed_event_past_64_kib_is_skipped_and_counted() {
    let (steps, undecoded) = compressed(OVER_64K_ZST);
    assert_eq!(undecoded, 1);
    // The call stays running; the reply and the call after it are read.
    assert!(steps.iter().all(|step| step.ok.is_none()));
    assert_eq!(steps.len(), 3, "a reply and two calls");
}

#[test]
fn a_corrupt_compressed_event_is_skipped_and_counted() {
    let mut corrupt = TOOL_RESULT_ZST.to_vec();
    corrupt.truncate(20);
    for (blob, what) in [
        (&b"\x28\xb5\x2f\xfd"[..], "a frame header cut"),
        (&corrupt[..], "a frame cut"),
        (&b"not zstd at all"[..], "no frame"),
    ] {
        let (steps, undecoded) = compressed(blob);
        assert_eq!(undecoded, 1, "{what}");
        assert_eq!(steps.len(), 3, "{what}");
    }
}

#[test]
fn the_session_is_harness_named_one() {
    let lamp = Lamp::with(FIXTURE);
    let state = vec![lamp.state()];
    let database = lamp.database();
    assert_eq!(
        session(&state, AGENT),
        Ok(Some((database.clone(), "s1".into())))
    );
    assert_eq!(
        session(&state, &AGENT.to_ascii_uppercase()),
        Ok(Some((database, "s1".into())))
    );
    assert_eq!(session(&state, "nobody"), Ok(None));
    assert_eq!(session(&state, "../x"), Ok(None));
    assert_eq!(session(&state, "main"), Ok(None));
    // Not an OpenClaw task: not read.
    let mut codex = watched(None);
    codex[0].engine = "codex".into();
    assert!(lamp.steps().tick(&codex, 0, WALL).is_empty());
}

/// The agent has a shell in that folder: a database that is a link is
/// never opened, and one not in OpenClaw's schema is read as nothing.
#[test]
fn a_link_or_another_schema_is_never_read() {
    let lamp = Lamp::with("");
    let real = lamp.root.path().join("real.sqlite");
    fs::rename(lamp.database(), &real).unwrap();
    std::os::unix::fs::symlink(&real, lamp.database()).unwrap();
    assert_eq!(session(&[lamp.state()], AGENT), Err(Unread));
    fs::remove_file(lamp.database()).unwrap();
    let other = Connection::open(lamp.database()).unwrap();
    other
        .execute_batch(
            "CREATE TABLE session_nodes(session_key TEXT, current_session_id TEXT);
             INSERT INTO session_nodes VALUES ('agent:main:harness-1ac58eb4-9d87-4b2a-8c1e-0f2d3c4b5a69', 's1');",
        )
        .unwrap();
    assert_eq!(session(&[lamp.state()], AGENT), Err(Unread));
}

#[test]
fn the_root_is_the_wrappers_state_unless_set() {
    assert_eq!(
        roots(None),
        [PathBuf::from("/var/lib/lampos-openclaw/state")]
    );
    assert_eq!(roots(Some("/s")), [PathBuf::from("/s")]);
    assert_eq!(
        roots(Some("rel")),
        [PathBuf::from("/var/lib/lampos-openclaw/state")]
    );
    // A later root is read when the first has no such session.
    let lamp = Lamp::with("");
    let mut reader = switched(
        Some("on"),
        vec![PathBuf::from("/nonexistent"), lamp.state()],
    )
    .unwrap();
    reader.tick(&watched(None), 0, WALL);
    lamp.append(&call("c1", "exec", 1));
    assert!(!reader.tick(&watched(None), EVERY_MS, WALL).is_empty());
}

/// A database locked when the task is handed over gives it no bound: it is
/// not read, never from the session's start, until the bound is read.
#[test]
fn a_locked_database_at_the_hand_over_never_lets_earlier_asks_out() {
    let lamp = Lamp::with(FIXTURE);
    lamp.db.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let mut reader = lamp.steps();
    assert!(reader.tick(&watched(None), 0, WALL).is_empty());
    assert!(reader.tails.is_empty(), "no bound while it is locked");
    // Another earlier ask goes in while it is locked.
    lamp.append(&(call("e1", "earlier_tool", 20) + &result("e1", "earlier words", 21)));
    lamp.db.execute_batch("COMMIT").unwrap();
    assert!(reader.tick(&watched(None), EVERY_MS, WALL).is_empty());
    lamp.append(&call("mine", "exec", 50));
    let steps = reader.tick(&watched(None), 2 * EVERY_MS, WALL);
    assert_eq!(steps.len(), 1);
    assert_eq!((steps[0].n, steps[0].tool.as_str()), (1, "exec"));
}

/// A database busy in the middle of a task reads nothing, and the next
/// read goes on from where it was: no reset, nothing twice.
#[test]
fn a_busy_database_mid_task_reads_nothing_and_resets_nothing() {
    let (lamp, mut reader, steps) = own_with(FIXTURE, None);
    assert_eq!(latest(&steps).len(), 5);
    lamp.db.execute_batch("BEGIN EXCLUSIVE").unwrap();
    lamp.append(&call("c9", "exec", 99));
    assert!(reader.tick(&watched(None), 2 * EVERY_MS, WALL).is_empty());
    lamp.db.execute_batch("COMMIT").unwrap();
    let steps = reader.tick(&watched(None), 3 * EVERY_MS, WALL);
    assert_eq!(steps.len(), 1);
    assert_eq!((steps[0].n, steps[0].tool.as_str()), (6, "exec"));
}

/// One read takes at most 200 events, every task together; the rest wait.
#[test]
fn a_read_takes_at_most_200_events() {
    let many: String = (0..300)
        .map(|i| format!(r#"{{"type":"custom","customType":"c","data":{{"i":{i}}}}}"#) + "\n")
        .collect();
    let (lamp, mut reader, steps) = own_with(&many, None);
    assert!(steps.is_empty());
    assert_eq!(reader.tails[&TASK].after, 199);
    lamp.append(&call("c1", "exec", 5));
    let steps = reader.tick(&watched(None), 2 * EVERY_MS, WALL);
    assert_eq!(reader.tails[&TASK].after, 300);
    assert_eq!(steps.len(), 1);
}

/// One read takes at most 256 KiB of event text; the rest wait.
#[test]
fn a_read_takes_at_most_256_kib() {
    let big = format!(
        r#"{{"type":"custom","customType":"c","data":"{}"}}"#,
        "x".repeat(100 * 1024)
    ) + "\n";
    let (_lamp, mut reader, _) = own_with(&big.repeat(5), None);
    assert_eq!(reader.tails[&TASK].after, 1, "two events of 100 KiB");
    reader.tick(&watched(None), 2 * EVERY_MS, WALL);
    assert_eq!(reader.tails[&TASK].after, 3);
    reader.tick(&watched(None), 3 * EVERY_MS, WALL);
    assert_eq!(reader.tails[&TASK].after, 4);
}

/// Every step of two tasks, as (task, tool), in order.
fn by_task(steps: &[TaskStep]) -> Vec<(u64, String)> {
    let mut seen: Vec<(u64, String)> = steps
        .iter()
        .map(|step| (step.task, step.tool.clone()))
        .collect();
    seen.dedup();
    seen
}

/// The Reviewer-lead's race (#7286, 17:00): task A begins before OpenClaw
/// made its session, B begins (on the same agent) before A's next read.
/// Each step still goes out under one task only.
#[test]
fn a_task_begun_before_its_session_is_bounded_by_the_next() {
    let lamp = Lamp::with("");
    let key = format!("agent:main:harness-{AGENT}");
    lamp.db
        .execute(
            "DELETE FROM session_nodes WHERE session_key = ?1",
            params![key],
        )
        .unwrap();
    let mut reader = lamp.steps();
    let first = [task(7, None)];
    let both = [task(7, None), task(8, None)];
    assert!(reader.tick(&first, 0, WALL).is_empty());
    // OpenClaw makes the session for A's ask, then B is handed over.
    lamp.db
        .execute(
            "INSERT INTO session_nodes VALUES (?1, 's1', '{}', 1)",
            params![key],
        )
        .unwrap();
    lamp.append(&(call("a1", "first_tool", 10) + &result("a1", "first words", 11)));
    assert!(reader.tick(&both, EVERY_MS / 2, WALL).is_empty());
    lamp.append(&(call("b1", "second_tool", 20) + &result("b1", "second words", 21)));
    let steps = reader.tick(&both, EVERY_MS, WALL);
    assert_eq!(
        by_task(&steps),
        [(7, "first_tool".to_owned()), (8, "second_tool".to_owned())]
    );
}

/// Both tasks begun before OpenClaw made the session: it is the newer one's.
#[test]
fn two_tasks_begun_before_their_session_give_it_to_the_newer() {
    let lamp = Lamp::with("");
    let key = format!("agent:main:harness-{AGENT}");
    lamp.db
        .execute(
            "DELETE FROM session_nodes WHERE session_key = ?1",
            params![key],
        )
        .unwrap();
    let mut reader = lamp.steps();
    assert!(reader.tick(&[task(7, None)], 0, WALL).is_empty());
    let both = [task(7, None), task(8, None)];
    assert!(reader.tick(&both, EVERY_MS / 2, WALL).is_empty());
    lamp.db
        .execute(
            "INSERT INTO session_nodes VALUES (?1, 's1', '{}', 1)",
            params![key],
        )
        .unwrap();
    lamp.append(&call("b1", "second_tool", 20));
    let steps = reader.tick(&both, EVERY_MS, WALL);
    assert_eq!(by_task(&steps), [(8, "second_tool".to_owned())]);
}

/// A frame whose header asks for a window past 1 MiB is never decoded: the
/// decoder would reserve that window (up to 100 MiB) first.
#[test]
fn a_compressed_event_asking_a_large_window_is_skipped() {
    // Magic, descriptor 0x00 (no single segment, no checksum, no size),
    // window descriptor 0x80: exponent 16, a 64 MiB window.
    let header = [0x28, 0xb5, 0x2f, 0xfd, 0x00, 0x80];
    assert_eq!(decoded(&header), Err(WINDOW_TOO_LARGE));
    let (steps, undecoded) = compressed(&header);
    assert_eq!(undecoded, 1);
    assert_eq!(steps.len(), 3);
    // The fixture's own frames ask for small windows and decode.
    assert!(decoded(TOOL_RESULT_ZST).is_ok());
}

/// Thirteen's HOLD (#7286, 16:27): the steps' SQLite reads never run on
/// the serve loop that accepts the voice's socket.
#[test]
fn the_serve_loop_never_reads_steps() {
    let runtime = include_str!("../runtime.rs");
    assert!(
        !runtime.contains(".tick(&report.watched"),
        "the serve loop reads steps itself"
    );
    assert!(runtime.contains("crate::steps::spawn("));
}

/// Thirteen's HOLD: SQLite's NOFOLLOW refuses a link anywhere in the path,
/// so a state folder reached through a link (a data partition) must still
/// be read; only what lies under it must not be a link.
#[test]
fn a_state_folder_reached_through_a_link_is_read() {
    let lamp = Lamp::with("");
    let elsewhere = tempfile::tempdir().unwrap();
    let linked = elsewhere.path().join("state");
    std::os::unix::fs::symlink(lamp.state(), &linked).unwrap();
    let mut reader = switched(Some("on"), vec![linked]).unwrap();
    reader.tick(&watched(None), 0, WALL);
    lamp.append(&call("c1", "exec", 1));
    assert_eq!(reader.tick(&watched(None), EVERY_MS, WALL).len(), 1);
}

/// Heron's read (#7286, 15:45): a key or token in a call's arguments or a
/// tool's result never reaches the bus.
#[test]
fn secrets_in_args_and_results_are_redacted() {
    let calls = [
        (
            r#"{"command":"curl -H 'Authorization: Bearer abc123def456' https://example.org"}"#,
            "abc123def456",
        ),
        (
            r#"{"command":"export X_API_KEY=hunter2 && run"}"#,
            "hunter2",
        ),
        (r#"{"url":"https://example.org","api_key":"hunter3"}"#, "hunter3"),
        (
            r#"{"command":"echo sk-proj-AbCdEf0123456789xyz"}"#,
            "sk-proj-AbCdEf0123456789xyz",
        ),
    ];
    for (i, (arguments, secret)) in calls.iter().enumerate() {
        let line = format!(
            r#"{{"type":"message","message":{{"role":"assistant","content":[{{"type":"toolCall","id":"c{i}","name":"exec","arguments":{arguments}}}],"timestamp":5}}}}"#
        ) + "\n";
        let steps = own(&line);
        assert_eq!(steps.len(), 1, "{arguments}");
        assert!(!steps[0].args.contains(secret), "{}", steps[0].args);
        assert!(steps[0].args.contains("***"), "{}", steps[0].args);
    }
    let token = "ghp_0123456789abcdefABCDEF0123456789abcd";
    let steps = own(&(call("c1", "exec", 1) + &result("c1", &format!("token is {token}"), 2)));
    let done = steps.last().unwrap();
    assert_eq!(done.result, "token is ***");
    // Plain words stay.
    let steps = own(&(call("c2", "exec", 1) + &result("c2", "3 files changed", 2)));
    assert_eq!(steps.last().unwrap().result, "3 files changed");
}
