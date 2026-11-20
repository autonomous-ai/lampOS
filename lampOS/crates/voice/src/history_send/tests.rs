use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;

/// Waits short enough for a test.
const QUICK: Waits = Waits {
    first: Duration::from_millis(5),
    longest: Duration::from_millis(20),
};
/// The epoch the fake memory tells, and two known people.
const STORE: &str = "0a1b2c3d";
const A: &str = "p-0123456789abcdef";
const B: &str = "p-fedcba9876543210";

/// The id the queue sends for `scope`'s exchange `id` in [`STORE`].
fn id(scope: &str, exchange: u64) -> String {
    history_turn_id(STORE, scope, exchange)
}

/// A post that answers from `answers` in turn (the last one again once
/// they run out) and hands each body it was given to the receiver.
fn recording(answers: Vec<Answer>) -> (Post, mpsc::Receiver<Value>) {
    let (tell, told) = mpsc::channel();
    let mut answers: VecDeque<Answer> = answers.into();
    let post: Post = Box::new(move |body: &Value| {
        let _ = tell.send(body.clone());
        if answers.len() > 1 {
            answers.pop_front().unwrap()
        } else {
            answers.front().copied().unwrap_or(Answer::Saved)
        }
    });
    (post, told)
}

/// A fake memory: epoch `store` and `issued` for `history_issued`, the
/// epoch for `history_epoch`, and each scope's exchanges (id, words) for
/// `exchanges_after`, paged by its `limit`.
fn memory(store: &str, issued: Value, kept: Vec<(&str, u64, &str)>) -> Ask {
    memory_at(Arc::new(Mutex::new(store.to_owned())), issued, kept)
}

/// [`memory`] whose epoch can change under it.
fn memory_at(store: Arc<Mutex<String>>, issued: Value, kept: Vec<(&str, u64, &str)>) -> Ask {
    let kept: Vec<(String, u64, String)> = kept
        .into_iter()
        .map(|(scope, id, words)| (scope.to_owned(), id, words.to_owned()))
        .collect();
    Box::new(move |line: &Value| match line["op"].as_str() {
        Some("history_issued") => {
            Some(json!({"ok":true,"epoch":*store.lock().unwrap(),"issued":issued}))
        }
        Some("history_epoch") => Some(json!({"ok":true,"epoch":*store.lock().unwrap()})),
        Some("exchanges_after") => {
            let scope = line["person"].as_str().unwrap_or(ROOM_SCOPE);
            let after = line["after"].as_u64().unwrap();
            let limit = line["limit"].as_u64().unwrap() as usize;
            let all: Vec<Value> = kept
                .iter()
                .filter(|(s, id, _)| s == scope && *id > after)
                .map(|(_, id, words)| json!({"id":id,"at_unix_ms":1_000 + id,"heard":words,"said":"said"}))
                .collect();
            let more = all.len() > limit;
            Some(
                json!({"ok":true,"exchanges":all.into_iter().take(limit).collect::<Vec<_>>(),"more":more}),
            )
        }
        _ => None,
    })
}

/// A memory that tells [`STORE`] and holds nothing to catch up on.
fn fresh() -> Ask {
    memory(STORE, json!({}), vec![])
}

fn ids(body: &Value) -> Vec<String> {
    body["turns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|turn| turn["id"].as_str().unwrap().to_owned())
        .collect()
}

fn next(told: &mpsc::Receiver<Value>) -> Value {
    told.recv_timeout(Duration::from_secs(5)).expect("a call")
}

fn none_within(told: &mpsc::Receiver<Value>, wait: Duration) {
    if let Ok(body) = told.recv_timeout(wait) {
        panic!("nothing should be sent: {body}");
    }
}

/// What waits to be sent, and what is held, as `<scope>:<id>`.
fn queued(sender: &Sender) -> (Vec<String>, Vec<String>) {
    let queue = locked(&sender.shared);
    let named = |pendings: &VecDeque<Pending>| {
        pendings
            .iter()
            .map(|pending| format!("{}:{}", pending.scope, pending.exchange))
            .collect()
    };
    (named(&queue.ready), named(&queue.held))
}

fn offer(sender: &Sender, person: Option<&str>, exchange: u64) {
    sender.offer(person, exchange, 1_000 + exchange, ("h", "s"), false);
}

/// A scratch folder for a marks file.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lamp-history-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The marks file, read back, once it says `mark` for `scope`.
fn marks_once(path: &Path, scope: &str, mark: u64) -> Value {
    let started = Instant::now();
    loop {
        if let Some(value) = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            && value["marks"][scope] == mark
        {
            return value;
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            std::fs::read_to_string(path)
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn the_switch_is_exactly_on() {
    assert!(on_from(Some("on")));
    for value in [
        None,
        Some(""),
        Some("off"),
        Some("ON"),
        Some("on "),
        Some("1"),
    ] {
        assert!(!on_from(value), "{value:?}");
    }
}

#[test]
fn a_time_is_the_stores_utc() {
    // 2025-11-18T11:40:02Z.
    assert_eq!(utc(1_763_466_002_123), "2025-11-18T11:40:02Z");
    assert_eq!(utc(0), "1970-01-01T00:00:00Z");
}

#[test]
fn the_stores_answers_are_read() {
    assert_eq!(answer_of(200, br#"{"saved":["a"]}"#), Answer::Saved);
    assert_eq!(
        answer_of(200, br#"{"status":1,"data":{"saved":["a"]}}"#),
        Answer::Saved
    );
    assert_eq!(answer_of(200, br#"{"enabled":false}"#), Answer::Off);
    assert_eq!(
        answer_of(200, br#"{"status":1,"data":{"enabled":false}}"#),
        Answer::Off
    );
    assert_eq!(
        answer_of(200, br#"{"status":0,"message":"bad"}"#),
        Answer::Refused(200)
    );
    assert_eq!(answer_of(500, b""), Answer::Again);
    assert_eq!(answer_of(503, b"busy"), Answer::Again);
    assert_eq!(answer_of(429, b""), Answer::Again);
    assert_eq!(answer_of(408, b""), Answer::Again);
    assert_eq!(
        answer_of(400, br#"{"error":"too_long"}"#),
        Answer::Refused(400)
    );
    assert_eq!(answer_of(401, b""), Answer::Refused(401));
    assert_eq!(answer_of(200, b"not json"), Answer::Again);
}

/// Ruling 2: the id is `<epoch>.<scope>:<exchange id>`, the one helper the
/// memory's forget answers name it by (`lamp_msgs::history_turn_id`), the
/// room's scope `everyone`, moved lines under their new person and id.
#[test]
fn a_saves_id_is_the_store_the_scope_and_the_memorys_exchange_id() {
    let (post, told) = recording(vec![Answer::Saved]);
    let sender = Sender::new();
    sender.spawn(post, fresh(), None, QUICK).unwrap();
    offer(&sender, Some(A), 42);
    assert_eq!(ids(&next(&told)), [format!("{STORE}.{A}:42")]);
    offer(&sender, None, 3);
    offer(&sender, None, 4);
    sender.moved(B, &[(4, 12)]);
    assert_eq!(
        ids(&next(&told)),
        [lamp_msgs::history_turn_id(STORE, B, 12)]
    );
    sender.settled(&[3]);
    assert_eq!(ids(&next(&told)), [format!("{STORE}.everyone:3")]);
}

/// Nothing is posted before the memory told its store's id: it is asked
/// again until it answers.
#[test]
fn nothing_is_posted_before_the_store_is_known() {
    let (post, told) = recording(vec![Answer::Saved]);
    let asked = Arc::new(Mutex::new(0u32));
    let counted = Arc::clone(&asked);
    let mut inner = fresh();
    let ask: Ask = Box::new(move |line: &Value| {
        let mut asked = counted.lock().unwrap();
        *asked += 1;
        (*asked > 3).then(|| inner(line)).flatten()
    });
    let sender = Sender::new();
    offer(&sender, Some(A), 1);
    sender.spawn(post, ask, None, QUICK).unwrap();
    assert_eq!(ids(&next(&told)), [id(A, 1)]);
    assert!(*asked.lock().unwrap() >= 4);
}

/// A failed call is tried again with the same ids, never new ones.
#[test]
fn a_retry_sends_the_same_ids() {
    let (post, told) = recording(vec![Answer::Again, Answer::Again, Answer::Saved]);
    let sender = Sender::new();
    sender.offer(
        Some(A),
        3,
        1_000,
        ("what time is it", "Ten past nine."),
        false,
    );
    sender.offer(Some(A), 4, 2_000, ("thanks", "Any time."), false);
    sender.spawn(post, fresh(), None, QUICK).unwrap();
    let first = next(&told);
    let second = next(&told);
    let third = next(&told);
    assert_eq!(ids(&first), [id(A, 3), id(A, 4)]);
    assert_eq!(first, second);
    assert_eq!(second, third);
    none_within(&told, Duration::from_millis(100));
    assert_eq!(sender.let_go(), 0);
}

/// A batch never saved in its tries waits for the next start, counted.
#[test]
fn a_batch_not_saved_in_its_tries_waits_for_the_next_start() {
    let (post, told) = recording(vec![Answer::Again]);
    let sender = Sender::new();
    offer(&sender, Some(A), 1);
    sender.spawn(post, fresh(), None, QUICK).unwrap();
    for _ in 0..TRIES {
        assert_eq!(ids(&next(&told)), [id(A, 1)]);
    }
    none_within(&told, Duration::from_millis(150));
    assert_eq!(sender.let_go(), 1);
    assert_eq!(locked(&sender.shared).deferred.get(A), Some(&1));
}

/// The store's switch off, or a refusal, is never tried again.
#[test]
fn off_and_refused_are_not_tried_again() {
    for answer in [Answer::Off, Answer::Refused(400)] {
        let (post, told) = recording(vec![answer]);
        let sender = Sender::new();
        offer(&sender, Some(A), 1);
        sender.spawn(post, fresh(), None, QUICK).unwrap();
        next(&told);
        none_within(&told, Duration::from_millis(100));
    }
}

/// The caps: 46 turns go as 20, 20 and 6, in order, and a text over 8 KB
/// is cut at a character's end.
#[test]
fn the_caps_split_a_batch() {
    let (post, told) = recording(vec![Answer::Saved]);
    let sender = Sender::new();
    for n in 1..=45 {
        offer(&sender, Some(A), n);
    }
    // 8,191 bytes of `a` then a 3-byte character: the cut leaves it out.
    let long = format!("{}{}", "a".repeat(TEXT_MOST - 1), "ệ".repeat(10));
    sender.offer(Some(A), 46, 2_000, (&long, "s"), false);
    sender.spawn(post, fresh(), None, QUICK).unwrap();
    let batches: Vec<Value> = (0..3).map(|_| next(&told)).collect();
    let sizes: Vec<usize> = batches.iter().map(|body| ids(body).len()).collect();
    assert_eq!(sizes, [20, 20, 6]);
    let all: Vec<String> = batches.iter().flat_map(ids).collect();
    let expected: Vec<String> = (1..=46).map(|n| id(A, n)).collect();
    assert_eq!(all, expected);
    let heard = batches[2]["turns"][5]["heard"].as_str().unwrap();
    assert!(heard.len() <= TEXT_MOST, "{}", heard.len());
    assert_eq!(heard, "a".repeat(TEXT_MOST - 1));
    assert_eq!(capped("short"), "short");
    let exact = "b".repeat(TEXT_MOST);
    assert_eq!(capped(&exact), exact);
}

/// The turn as the store takes it: id, time, person (empty for the room),
/// heard and said; a reply after a linked read goes as the placeholder.
#[test]
fn a_turn_is_sent_in_the_stores_shape() {
    let (post, told) = recording(vec![Answer::Saved]);
    let sender = Sender::new();
    sender.offer(
        Some(A),
        9,
        1_763_466_002_000,
        ("how are you", "Bright, thanks."),
        false,
    );
    sender.spawn(post, fresh(), None, QUICK).unwrap();
    assert_eq!(
        next(&told),
        json!({"turns":[{"id":id(A, 9),"at":"2025-11-18T11:40:02Z","person":A,"heard":"how are you","said":"Bright, thanks."}]})
    );
    sender.offer(
        Some(A),
        10,
        1_000,
        ("read my mail", "Ann wrote: the key is 4512."),
        true,
    );
    let body = next(&told);
    assert_eq!(body["turns"][0]["said"], crate::thread::PRIVATE_SAID);
    assert!(!body.to_string().contains("4512"), "{body}");
}

/// A scope no forget can reach (not a known person's `p-` id) is never
/// sent, nor is a move to one.
#[test]
fn only_scopes_a_forget_reaches_are_sent() {
    let sender = Sender::new();
    offer(&sender, Some("guest"), 1);
    offer(&sender, Some(A), 2);
    offer(&sender, None, 3);
    sender.moved("guest", &[(3, 9)]);
    assert_eq!(
        queued(&sender),
        (vec![format!("{A}:2")], vec!["everyone:3".to_owned()])
    );
}

/// The room's lines wait for their conversation: a moved one goes as its
/// new person's under its new id, the rest as the room's at the end.
#[test]
fn room_turns_wait_for_their_conversation_and_follow_a_move() {
    let (post, told) = recording(vec![Answer::Saved]);
    let sender = Sender::new();
    sender.spawn(post, fresh(), None, QUICK).unwrap();
    offer(&sender, None, 5);
    offer(&sender, None, 6);
    none_within(&told, Duration::from_millis(100));
    sender.moved(A, &[(5, 12), (40, 41)]);
    let moved = next(&told);
    assert_eq!(ids(&moved), [id(A, 12)]);
    assert_eq!(moved["turns"][0]["person"], A);
    // An end naming none of the held lines sends none.
    sender.settled(&[99]);
    none_within(&told, Duration::from_millis(50));
    sender.settled(&[6]);
    let settled = next(&told);
    assert_eq!(ids(&settled), [id(ROOM_SCOPE, 6)]);
    assert_eq!(settled["turns"][0]["person"], "");
    none_within(&told, Duration::from_millis(50));
}

/// Twelve's HOLD 3: a forget lets go of the ids it removed only; with none
/// named, the forgetting person's and the room's wait for the next start;
/// other people's unsent turns are never let go.
#[test]
fn a_forget_lets_go_of_its_own_ids_never_other_peoples() {
    let sender = Sender::new();
    for (person, n) in [(Some(A), 1), (Some(A), 2), (Some(B), 3), (None, 4)] {
        offer(&sender, person, n);
    }
    sender.forgetting(Some(A));
    sender.forget_answered(
        Some(A),
        false,
        Some(&json!({"ok":true,"removed":1,"exchanges":{A:[1]}})),
    );
    assert_eq!(
        queued(&sender),
        (
            vec![format!("{A}:2"), format!("{B}:3")],
            vec!["everyone:4".to_owned()]
        )
    );
    assert!(locked(&sender.shared).is_forgotten(A, 1));
    // A forget that names no ids (a memory before #7935's answer).
    sender.forgetting(Some(A));
    sender.forget_answered(Some(A), false, Some(&json!({"ok":true,"removed":1})));
    assert_eq!(queued(&sender), (vec![format!("{B}:3")], vec![]));
    assert_eq!(locked(&sender.shared).deferred.get(A), Some(&2));
    // Forget everything: all of theirs offered so far is forgotten.
    offer(&sender, Some(A), 5);
    sender.forgetting(Some(B));
    sender.forget_answered(Some(B), true, Some(&json!({"ok":true})));
    assert_eq!(queued(&sender), (vec![format!("{A}:5")], vec![]));
    assert!(locked(&sender.shared).is_forgotten(B, 3));
    assert!(!locked(&sender.shared).is_forgotten(B, 4));
    assert!(locked(&sender.shared).paused_until.is_none());
}

/// Ruling 1 and Twelve's HOLD 4: a forget holds every post, the first
/// included, until the memory answers; what it removed is never posted.
#[test]
fn a_forget_holds_the_first_post_and_what_it_removed_never_goes() {
    let (post, told) = recording(vec![Answer::Saved]);
    let sender = Sender::new();
    sender.spawn(post, fresh(), None, QUICK).unwrap();
    sender.forgetting(Some(A));
    offer(&sender, Some(A), 1);
    offer(&sender, Some(A), 2);
    none_within(&told, Duration::from_millis(150));
    // The memory's forget answer names the turn ids (#7982).
    sender.forget_answered(
        Some(A),
        false,
        Some(&json!({"ok":true,"exchanges":{A:[id(A, 1)]}})),
    );
    assert_eq!(ids(&next(&told)), [id(A, 2)]);
    // An id queued for deletion by its full id is never posted, though the
    // memory hands it again (a catch-up).
    sender.deleting(&[id(A, 7)]);
    offer(&sender, Some(A), 7);
    offer(&sender, Some(A), 8);
    assert_eq!(ids(&next(&told)), [id(A, 8)]);
    none_within(&told, Duration::from_millis(50));
}

/// Ruling 1's test: a forget while a save is in flight leaves the turn
/// gone, lamp side: the call under way fails, and its next try leaves the
/// forgotten turn out (one the store took is the server's tombstone's).
#[test]
fn a_forget_during_an_in_flight_save_leaves_the_turn_gone() {
    let (tell, told) = mpsc::channel();
    let (go, gate) = mpsc::channel::<Answer>();
    let post: Post = Box::new(move |body: &Value| {
        let _ = tell.send(body.clone());
        gate.recv_timeout(Duration::from_secs(5))
            .unwrap_or(Answer::Saved)
    });
    let sender = Sender::new();
    offer(&sender, Some(A), 1);
    offer(&sender, Some(A), 2);
    sender.spawn(post, fresh(), None, QUICK).unwrap();
    assert_eq!(ids(&next(&told)), [id(A, 1), id(A, 2)]);
    // The call is in flight: the forget is said and answered.
    sender.forgetting(Some(A));
    sender.forget_answered(
        Some(A),
        false,
        Some(&json!({"ok":true,"exchanges":{A:[1]}})),
    );
    go.send(Answer::Again).unwrap();
    assert_eq!(ids(&next(&told)), [id(A, 2)]);
    go.send(Answer::Saved).unwrap();
    none_within(&told, Duration::from_millis(100));
}

/// Friday's P2 on #7957: a forget that names no ids (a lost answer, a
/// timeout, a forget everything's room lines) covers a batch already taken
/// off the queue too. Its next try leaves the forgetting person's and the
/// room's turns out, to wait for the next start; another person's go.
#[test]
fn an_in_flight_batch_is_not_posted_after_a_forget_naming_no_ids() {
    let (tell, told) = mpsc::channel();
    let (go, gate) = mpsc::channel::<Answer>();
    let post: Post = Box::new(move |body: &Value| {
        let _ = tell.send(body.clone());
        gate.recv_timeout(Duration::from_secs(5))
            .unwrap_or(Answer::Saved)
    });
    let sender = Sender::new();
    offer(&sender, Some(A), 1);
    offer(&sender, Some(B), 2);
    sender.spawn(post, fresh(), None, QUICK).unwrap();
    assert_eq!(ids(&next(&told)), [id(A, 1), id(B, 2)]);
    // Taken off the queue and in flight: A's forget, its answer lost.
    sender.forgetting(Some(A));
    sender.forget_answered(Some(A), false, None);
    go.send(Answer::Again).unwrap();
    assert_eq!(ids(&next(&told)), [id(B, 2)]);
    go.send(Answer::Saved).unwrap();
    none_within(&told, Duration::from_millis(100));
    assert_eq!(locked(&sender.shared).deferred.get(A), Some(&1));
    // A forget everything: the room's lines it took are not named, and the
    // room's line in flight waits too; a new turn of A's goes.
    offer(&sender, None, 4);
    sender.settled(&[4]);
    assert_eq!(ids(&next(&told)), [id(ROOM_SCOPE, 4)]);
    sender.forgetting(Some(B));
    sender.forget_answered(Some(B), true, Some(&json!({"ok":true})));
    go.send(Answer::Again).unwrap();
    none_within(&told, Duration::from_millis(100));
    assert_eq!(locked(&sender.shared).deferred.get(ROOM_SCOPE), Some(&4));
    offer(&sender, Some(A), 5);
    assert_eq!(ids(&next(&told)), [id(A, 5)]);
    go.send(Answer::Saved).unwrap();
}

/// After a forget the epoch is read again: a changed one (a wipe whole)
/// lets the unsent turns wait for the next start and starts the marks
/// again under it, and new turns go under it.
#[test]
fn a_changed_epoch_after_a_forget_starts_the_marks_again() {
    let dir = scratch("epoch");
    let marks = dir.join("history-marks.json");
    let epoch = Arc::new(Mutex::new(STORE.to_owned()));
    let (post, told) = recording(vec![Answer::Saved]);
    let sender = Sender::new();
    sender
        .spawn(
            post,
            memory_at(Arc::clone(&epoch), json!({A:0}), vec![]),
            Some(marks.clone()),
            QUICK,
        )
        .unwrap();
    marks_once(&marks, A, 0);
    sender.forgetting(None);
    offer(&sender, Some(B), 4);
    *epoch.lock().unwrap() = "99999999".into();
    sender.forget_answered(None, true, Some(&json!({"ok":true})));
    let started = Instant::now();
    loop {
        let text = std::fs::read_to_string(&marks).unwrap_or_default();
        if text.contains("99999999") {
            let written: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(written["marks"], json!({}));
            assert_eq!(written["offered"], json!({}));
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(5), "{text}");
        std::thread::sleep(Duration::from_millis(5));
    }
    none_within(&told, Duration::from_millis(50));
    assert_eq!(sender.let_go(), 1);
    offer(&sender, Some(A), 1);
    assert_eq!(ids(&next(&told)), [history_turn_id("99999999", A, 1)]);
    // The sender may still be writing its marks: a best-effort cleanup.
    let _ = std::fs::remove_dir_all(&dir);
}

/// A forget whose memory never answers holds sends at most [`FORGET_HOLD`];
/// then the forgetting person's and the room's wait for the next start.
#[test]
fn a_forget_with_no_answer_lets_its_scope_wait_for_the_next_start() {
    let sender = Sender::new();
    offer(&sender, Some(A), 1);
    offer(&sender, Some(B), 2);
    sender.forgetting(Some(A));
    let mut queue = locked(&sender.shared);
    let later = Instant::now() + FORGET_HOLD + Duration::from_millis(1);
    assert!(!held_by_forget(&mut queue, later));
    drop(queue);
    assert_eq!(queued(&sender), (vec![format!("{B}:2")], vec![]));
}

/// A full queue lets the oldest wait for the next start, counted, and
/// never blocks.
#[test]
fn a_full_queue_lets_the_oldest_wait() {
    let sender = Sender::new();
    for n in 1..=(READY_MOST as u64 + 5) {
        offer(&sender, Some(A), n);
    }
    assert_eq!(sender.let_go(), 5);
    for n in 1..=(HELD_MOST as u64 + 3) {
        offer(&sender, None, n);
    }
    assert_eq!(sender.let_go(), 8);
    let queue = locked(&sender.shared);
    assert_eq!(queue.deferred.get(A), Some(&1));
    assert_eq!(queue.deferred.get(ROOM_SCOPE), Some(&1));
}

/// A scope's mark is the highest id taken with nothing unsent below it.
#[test]
fn a_mark_never_passes_an_unsent_turn() {
    let mut queue = Queue::default();
    queue.acked.insert(A.into(), 5);
    assert_eq!(queue.mark(A), Some(5));
    queue.outstanding.entry(A.into()).or_default().insert(3);
    assert_eq!(queue.mark(A), Some(2));
    // A lower one waits for the next start: the mark stays below it.
    queue.deferred.insert(A.into(), 2);
    assert_eq!(queue.mark(A), Some(1));
    queue.outstanding.clear();
    assert_eq!(queue.mark(A), Some(1));
    queue.marks.insert(A.into(), 4);
    assert_eq!(queue.mark(A), Some(4));
    assert_eq!(queue.mark(B), None);
}

/// Ruling 4's test: a restart with unsent turns sends them after, from
/// the memory, under the same ids; the marks file holds ids only, 0600.
#[test]
fn a_restart_with_unsent_turns_sends_them_after() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("restart");
    let marks = dir.join("history-marks.json");
    // The first run: the store took exchange 1, never 2 and 3.
    {
        let (post, told) = recording(vec![Answer::Saved, Answer::Again]);
        let sender = Sender::new();
        sender
            .spawn(
                post,
                memory(STORE, json!({A:0}), vec![]),
                Some(marks.clone()),
                QUICK,
            )
            .unwrap();
        marks_once(&marks, A, 0);
        sender.offer(Some(A), 1, 1_001, ("first words", "s"), false);
        assert_eq!(ids(&next(&told)), [id(A, 1)]);
        marks_once(&marks, A, 1);
        sender.offer(Some(A), 2, 1_002, ("second words", "s"), false);
        sender.offer(Some(A), 3, 1_003, ("third words", "s"), false);
        for _ in 0..TRIES {
            assert_eq!(ids(&next(&told)), [id(A, 2), id(A, 3)]);
        }
        none_within(&told, Duration::from_millis(100));
    }
    let written = marks_once(&marks, A, 1);
    assert_eq!(written["epoch"], STORE);
    assert_eq!(written["marks"], json!({A:1}));
    // What was handed over this run, written before its first post.
    assert_eq!(written["offered"], json!({A:3}));
    let mode = std::fs::metadata(&marks).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    // The restart: the memory still holds 2 and 3, and a 4 kept while
    // no sender ran (the switch off, or the Live route): never sent.
    let (post, told) = recording(vec![Answer::Saved]);
    let sender = Sender::new();
    sender
        .spawn(
            post,
            memory(
                STORE,
                json!({A:4, B:0}),
                vec![
                    (A, 1, "first words"),
                    (A, 2, "second words"),
                    (A, 3, "third words"),
                    (A, 4, "fourth words"),
                ],
            ),
            Some(marks.clone()),
            QUICK,
        )
        .unwrap();
    let body = next(&told);
    assert_eq!(ids(&body), [id(A, 2), id(A, 3)]);
    assert_eq!(body["turns"][0]["heard"], "second words");
    marks_once(&marks, A, 4);
    none_within(&told, Duration::from_millis(100));
    // New turns go on after the catch-up.
    offer(&sender, Some(A), 5);
    assert_eq!(ids(&next(&told)), [id(A, 5)]);
    marks_once(&marks, A, 5);
    let text = std::fs::read_to_string(&marks).unwrap();
    assert!(!text.contains("words"), "{text}");
    // The sender may still be writing its marks: a best-effort cleanup.
    let _ = std::fs::remove_dir_all(&dir);
}

/// A store unlike the marks' (a reset, or the first start) sends nothing
/// from before: the marks start at what the memory holds now.
#[test]
fn a_new_store_sends_nothing_from_before() {
    let dir = scratch("new-store");
    let marks = dir.join("history-marks.json");
    write_marks(
        &marks,
        "11111111",
        &BTreeMap::from([(A.to_owned(), 2)]),
        &BTreeMap::from([(A.to_owned(), 9)]),
    )
    .unwrap();
    let (post, told) = recording(vec![Answer::Saved]);
    let sender = Sender::new();
    sender
        .spawn(
            post,
            memory(STORE, json!({A:9}), vec![(A, 3, "old"), (A, 9, "old")]),
            Some(marks.clone()),
            QUICK,
        )
        .unwrap();
    let written = marks_once(&marks, A, 9);
    assert_eq!(written["epoch"], STORE);
    assert_eq!(written["marks"], json!({A:9}));
    none_within(&told, Duration::from_millis(100));
    offer(&sender, Some(A), 10);
    assert_eq!(ids(&next(&told)), [id(A, 10)]);
    // The sender may still be writing its marks: a best-effort cleanup.
    let _ = std::fs::remove_dir_all(&dir);
}

/// A forgotten id is left out of the catch-up too.
#[test]
fn the_catch_up_leaves_out_what_was_forgotten() {
    let dir = scratch("catch-up-forgotten");
    let marks = dir.join("history-marks.json");
    write_marks(
        &marks,
        STORE,
        &BTreeMap::from([(A.to_owned(), 0)]),
        &BTreeMap::from([(A.to_owned(), 2)]),
    )
    .unwrap();
    let (post, told) = recording(vec![Answer::Saved]);
    let sender = Sender::new();
    sender.deleting(&[id(A, 1)]);
    sender
        .spawn(
            post,
            memory(STORE, json!({A:2}), vec![(A, 1, "gone"), (A, 2, "kept")]),
            Some(marks.clone()),
            QUICK,
        )
        .unwrap();
    assert_eq!(ids(&next(&told)), [id(A, 2)]);
    marks_once(&marks, A, 2);
    // The sender may still be writing its marks: a best-effort cleanup.
    let _ = std::fs::remove_dir_all(&dir);
}

/// Twelve's P3 (#7957, owed on #8125), pinned as it is: memory does not
/// mark a turn's route, so a Live-route turn kept between the mark and
/// `offered` in the same run is sent at the catch-up with the rest (a
/// forget reaches it like any other). One kept above `offered` never is.
#[test]
fn a_live_turn_between_the_mark_and_offered_goes_at_the_catch_up() {
    let dir = scratch("catch-up-live");
    let marks = dir.join("history-marks.json");
    write_marks(
        &marks,
        STORE,
        &BTreeMap::from([(A.to_owned(), 0)]),
        &BTreeMap::from([(A.to_owned(), 3)]),
    )
    .unwrap();
    let (post, told) = recording(vec![Answer::Saved]);
    let sender = Sender::new();
    sender
        .spawn(
            post,
            memory(
                STORE,
                json!({A:4}),
                vec![
                    (A, 1, "handed"),
                    (A, 2, "said on the Live route"),
                    (A, 3, "handed"),
                    (A, 4, "said on the Live route after"),
                ],
            ),
            Some(marks.clone()),
            QUICK,
        )
        .unwrap();
    let body = next(&told);
    assert_eq!(ids(&body), [id(A, 1), id(A, 2), id(A, 3)]);
    assert_eq!(body["turns"][1]["heard"], "said on the Live route");
    marks_once(&marks, A, 4);
    none_within(&told, Duration::from_millis(100));
    // The sender may still be writing its marks: a best-effort cleanup.
    let _ = std::fs::remove_dir_all(&dir);
}

/// The first builder's must-fix (01:16 on #7957): turns the memory kept
/// while the switch was off (no sender ran) are never sent, on the
/// switch-on's start nor on any start after; only turns kept after it are.
#[test]
fn turns_kept_while_the_switch_was_off_are_never_sent() {
    let dir = scratch("switch-off");
    let marks = dir.join("history-marks.json");
    // On: exchange 3 is kept and saved.
    {
        let (post, told) = recording(vec![Answer::Saved]);
        let sender = Sender::new();
        sender
            .spawn(
                post,
                memory(STORE, json!({A:2}), vec![]),
                Some(marks.clone()),
                QUICK,
            )
            .unwrap();
        marks_once(&marks, A, 2);
        offer(&sender, Some(A), 3);
        assert_eq!(ids(&next(&told)), [id(A, 3)]);
        marks_once(&marks, A, 3);
    }
    // Off: 4, 5 and 6 are kept with no sender. On again: none is sent.
    let kept = vec![
        (A, 3, "on"),
        (A, 4, "said while off"),
        (A, 5, "said while off"),
        (A, 6, "said while off"),
    ];
    {
        let (post, told) = recording(vec![Answer::Saved]);
        let sender = Sender::new();
        sender
            .spawn(
                post,
                memory(STORE, json!({A:6}), kept.clone()),
                Some(marks.clone()),
                QUICK,
            )
            .unwrap();
        marks_once(&marks, A, 6);
        none_within(&told, Duration::from_millis(150));
        // A turn kept after the switch-on is sent.
        offer(&sender, Some(A), 7);
        assert_eq!(ids(&next(&told)), [id(A, 7)]);
        marks_once(&marks, A, 7);
    }
    // A restart after: still none of the off period.
    let (post, told) = recording(vec![Answer::Saved]);
    let sender = Sender::new();
    let mut all = kept;
    all.push((A, 7, "on"));
    sender
        .spawn(
            post,
            memory(STORE, json!({A:7}), all),
            Some(marks.clone()),
            QUICK,
        )
        .unwrap();
    none_within(&told, Duration::from_millis(200));
    drop(sender);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A plain local store that takes the connection and answers after
/// `after`, telling each request's head and body.
fn slow_store(after: Duration) -> (String, mpsc::Receiver<(String, Value)>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (tell, told) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            let tell = tell.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream);
                let mut head = String::new();
                let mut length = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse().unwrap_or(0);
                    }
                    if line == "\r\n" {
                        break;
                    }
                    head.push_str(&line);
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let _ = tell.send((head, serde_json::from_slice(&body).unwrap_or(Value::Null)));
                std::thread::sleep(after);
                let answer = br#"{"status":1,"data":{"saved":["x"]}}"#;
                let mut stream = reader.into_inner();
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    answer.len()
                );
                let _ = stream.write_all(answer);
            });
        }
    });
    (base, told)
}

/// The save's URL on a plain local store, as [`url`] makes it on the relay.
fn local(base: &str) -> Option<String> {
    Some(format!("{base}/api/v1/ai{PATH}"))
}

/// The host leg (the first builder's P2 on #7935): only the lamp's own cloud
/// API, with or without its `/v1`; a direct provider's base, plain HTTP or
/// any other host sends nothing, and nothing starts.
#[test]
fn only_the_lamps_cloud_api_is_sent_to() {
    const SAVE: &str = "https://campaign-api.autonomous.ai/api/v1/ai/history/turns";
    for base in [
        "https://campaign-api.autonomous.ai/api/v1/ai",
        "https://campaign-api.autonomous.ai/api/v1/ai/",
        "https://campaign-api.autonomous.ai/api/v1/ai/v1",
        " https://campaign-api.autonomous.ai/api/v1/ai/v1/ ",
    ] {
        assert_eq!(url(base).as_deref(), Some(SAVE), "{base:?}");
    }
    let nowhere = Path::new("/nonexistent/lamp-history");
    for base in [
        "https://api.deepseek.com",
        "https://openrouter.ai/api/v1",
        "http://campaign-api.autonomous.ai/api/v1/ai",
        "https://near.example.invalid/api/v1/ai/v1",
        "https://campaign-api.autonomous.ai.example.invalid/api/v1/ai",
        "",
    ] {
        assert_eq!(url(base), None, "{base:?}");
        assert!(
            start_from(Some("on"), base, "k", nowhere, nowhere).is_none(),
            "{base:?}"
        );
    }
}

/// With the switch anything but `on`, nothing starts and nothing reaches
/// the store.
#[test]
fn nothing_is_sent_when_the_switch_is_off() {
    let (base, told) = slow_store(Duration::ZERO);
    let nowhere = Path::new("/nonexistent/lamp-history");
    for value in [None, Some("off"), Some("ON"), Some("")] {
        assert!(
            start_at(value, local(&base), "k", fresh(), None).is_none(),
            "{value:?}"
        );
        let relay = "https://campaign-api.autonomous.ai/api/v1/ai";
        assert!(
            start_from(value, relay, "k", nowhere, nowhere).is_none(),
            "{value:?}"
        );
    }
    // Nor does the voice's own path, never started, send a thing.
    offered(Some(A), 1, 1_000, ("h", "s"), false);
    settled(&[1]);
    assert!(told.recv_timeout(Duration::from_millis(200)).is_err());
}

/// The real call: the lamp's cloud API, the lamp's key as `x-api-key`, the
/// turns as JSON; and none without a key.
#[test]
fn the_call_goes_to_the_lamps_cloud_api_with_its_key() {
    let (base, told) = slow_store(Duration::ZERO);
    let sender =
        start_at(Some("on"), local(&base), " lamp-key \n", fresh(), None).expect("started");
    offer(&sender, Some(A), 2);
    let (head, body) = told.recv_timeout(Duration::from_secs(5)).expect("a call");
    assert!(
        head.starts_with("POST /api/v1/ai/history/turns HTTP/1.1"),
        "{head}"
    );
    assert!(
        head.to_ascii_lowercase()
            .contains("x-api-key: lamp-key\r\n"),
        "{head}"
    );
    assert!(
        !head.to_ascii_lowercase().contains("authorization"),
        "{head}"
    );
    assert_eq!(ids(&body), [id(A, 2)]);
    assert!(start_at(Some("on"), local(&base), "  ", fresh(), None).is_none());
    assert!(start_at(Some("on"), None, "k", fresh(), None).is_none());
}

/// A store that takes 2 s to answer never delays the turn: every offer
/// returns at once while a call hangs; measured, not assumed.
#[test]
fn the_reply_is_not_delayed_by_a_slow_server() {
    let (base, told) = slow_store(Duration::from_secs(2));
    let sender = start_at(Some("on"), local(&base), "k", fresh(), None).expect("started");
    offer(&sender, Some(A), 1);
    // The first call is in, and hangs for 2 s.
    told.recv_timeout(Duration::from_secs(5)).expect("a call");
    let mut slowest = Duration::ZERO;
    let started = Instant::now();
    for n in 2..=101 {
        let one = Instant::now();
        offer(&sender, Some(A), n);
        slowest = slowest.max(one.elapsed());
    }
    let all = started.elapsed();
    println!("100 offers beside a hanging call: {all:?} in all, slowest {slowest:?}");
    assert!(slowest < Duration::from_millis(20), "{slowest:?}");
    assert!(all < Duration::from_millis(100), "{all:?}");
}

/// `sender` as this thread's for `act`, the voice's hooks included.
fn as_the_voices<R>(sender: &Sender, act: impl FnOnce() -> R) -> R {
    TEST_SENDER.with(|test| *test.borrow_mut() = Some(sender.clone()));
    let result = act();
    TEST_SENDER.with(|test| *test.borrow_mut() = None);
    result
}

/// The memory's writer hands on exactly what the memory wrote, under the
/// memory's own id, and only once it answered with one.
#[test]
fn the_writer_offers_what_the_memory_wrote_under_its_id() {
    let (post, told) = recording(vec![Answer::Saved]);
    let sender = Sender::new();
    sender.spawn(post, fresh(), None, QUICK).unwrap();
    as_the_voices(&sender, || {
        crate::remember::write_for_history_test(&[
            (Some(A), "kept by the memory"),
            (Some(A), "refused by the memory"),
        ]);
    });
    let body = next(&told);
    assert_eq!(ids(&body), [id(A, 17)]);
    assert_eq!(body["turns"][0]["heard"], "kept by the memory");
    none_within(&told, Duration::from_millis(100));
}

/// Ruling 3, through the writer: a turn in a request a linked service's
/// read (mail, calendar) answered in is saved with the thread's
/// placeholder, never what was read.
#[test]
fn a_linked_reads_turn_is_saved_as_the_placeholder() {
    let sender = Sender::new();
    crate::services::read_answered_for_test(1);
    as_the_voices(&sender, || {
        crate::remember::write_for_history_test(&[
            (Some(A), "kept 8 read my mail"),
            (Some(A), "kept 9 thanks"),
        ]);
    });
    let queue = locked(&sender.shared);
    let saids: Vec<(u64, &str)> = queue
        .ready
        .iter()
        .map(|pending| (pending.exchange, pending.said.as_str()))
        .collect();
    assert_eq!(
        saids,
        [(8, crate::thread::PRIVATE_SAID), (9, "answer")],
        "{saids:?}"
    );
}

/// The writer's end of a room conversation (`end`) sends its held lines
/// as the room's.
#[test]
fn the_writer_sends_a_rooms_line_when_its_conversation_ends() {
    let sender = Sender::new();
    as_the_voices(&sender, || {
        crate::remember::write_for_history_test(&[(None, "kept 4 hello lamp")]);
    });
    assert_eq!(queued(&sender), (vec!["everyone:4".to_owned()], vec![]));
}

/// The writer's move of the room's conversation to a known person
/// (`go_on_as`) sends its held lines as theirs, under their new ids.
#[test]
fn the_writers_move_sends_the_room_lines_as_the_persons() {
    let sender = Sender::new();
    offer(&sender, None, 5);
    offer(&sender, None, 6);
    as_the_voices(&sender, || {
        crate::remember::go_on_for_history_test(A, &[(5, 12), (6, 13)]);
    });
    assert_eq!(
        queued(&sender),
        (vec![format!("{A}:12"), format!("{A}:13")], vec![])
    );
}

/// A forget said on the lamp, through the voice's tools: sends are held
/// while the memory answers it, and the exchanges it names removed are
/// let go and kept as forgotten; the rest go on. Then a forget about a
/// thing (Twelve's P2 at 01:14 on #7957): the same.
#[test]
fn a_forget_said_on_the_lamp_holds_and_lets_go_of_what_it_removed() {
    let sender = Sender::new();
    offer(&sender, Some(A), 17);
    offer(&sender, Some(A), 18);
    offer(&sender, Some(B), 19);
    *FORGET_PROBE.lock().unwrap() = Some(sender.clone());
    HELD_AT_FORGET.store(false, std::sync::atomic::Ordering::SeqCst);
    as_the_voices(&sender, || crate::remember::forget_for_history_test(A));
    *FORGET_PROBE.lock().unwrap() = None;
    assert!(HELD_AT_FORGET.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(
        queued(&sender),
        (vec![format!("{A}:18"), format!("{B}:19")], vec![])
    );
    {
        let queue = locked(&sender.shared);
        assert!(queue.is_forgotten(A, 17));
        assert!(queue.paused_until.is_none());
    }
    // "Forget what I said about coffee", with 21 in `ready`: it never goes.
    offer(&sender, Some(A), 21);
    offer(&sender, Some(A), 22);
    *FORGET_PROBE.lock().unwrap() = Some(sender.clone());
    HELD_AT_FORGET.store(false, std::sync::atomic::Ordering::SeqCst);
    as_the_voices(&sender, || {
        crate::remember::forget_about_for_history_test(A, Some(21))
    });
    *FORGET_PROBE.lock().unwrap() = None;
    assert!(HELD_AT_FORGET.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(
        queued(&sender),
        (
            vec![format!("{A}:18"), format!("{B}:19"), format!("{A}:22")],
            vec![]
        )
    );
    let queue = locked(&sender.shared);
    assert!(queue.is_forgotten(A, 21));
    assert!(queue.paused_until.is_none());
}

/// Friday's P2 on #7957: a forget about whose exchanges forget timed out
/// does not tell the sender `{"exchanges":{}}` ("nothing removed"): what
/// went is not known, so the person's and the room's unsent turns wait for
/// the next start, none of them forgotten by id; another person's go on.
#[test]
fn a_forget_about_that_timed_out_is_not_taken_as_nothing_removed() {
    let sender = Sender::new();
    offer(&sender, Some(A), 21);
    offer(&sender, Some(A), 22);
    offer(&sender, Some(B), 19);
    offer(&sender, None, 23);
    as_the_voices(&sender, || {
        crate::remember::forget_about_for_history_test(A, None)
    });
    assert_eq!(queued(&sender), (vec![format!("{B}:19")], vec![]));
    let queue = locked(&sender.shared);
    assert!(!queue.is_forgotten(A, 21));
    assert_eq!(queue.deferred.get(A), Some(&21));
    assert_eq!(queue.deferred.get(ROOM_SCOPE), Some(&23));
    assert!(queue.paused_until.is_none());
}

/// A source forgotten outside this voice, seen by the writer: the
/// person's and the room's unsent turns wait for the next start, when the
/// memory no longer holds what went; another person's go on.
#[test]
fn a_source_forgotten_elsewhere_lets_its_scope_wait() {
    let sender = Sender::new();
    offer(&sender, Some(A), 3);
    offer(&sender, Some(B), 1);
    offer(&sender, None, 4);
    as_the_voices(&sender, || {
        crate::remember::write_for_history_test(&[(Some(A), "gone with its source")]);
    });
    assert_eq!(queued(&sender), (vec![format!("{B}:1")], vec![]));
    assert_eq!(sender.let_go(), 2);
}

/// A gate the test opens and closes.
fn gate_on(open: &Arc<std::sync::atomic::AtomicBool>) -> Gate {
    let open = open.clone();
    Arc::new(move |_: &str| open.load(std::sync::atomic::Ordering::SeqCst))
}

/// The first builder's privacy read on #8125, item 3: a turn said just
/// before the privacy switch closed is not sent while it is closed, and
/// goes once it opens.
#[test]
fn a_turn_said_just_before_the_switch_closes_is_sent_only_after_it_opens() {
    let (post, told) = recording(vec![Answer::Saved]);
    let open = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let sender = Sender::new();
    sender.guarded(gate_on(&open), Arc::new(|_: &[String]| {}));
    // Said, then the switch closes before the sender takes it.
    offer(&sender, Some(A), 1);
    open.store(false, std::sync::atomic::Ordering::SeqCst);
    sender.spawn(post, fresh(), None, QUICK).unwrap();
    none_within(&told, Duration::from_millis(300));
    open.store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(ids(&next(&told)), [id(A, 1)]);
}

/// The first builder's privacy read on #8125, item 1: while a forget of a
/// scope has not reached the account, no turn of that scope is sent; it
/// goes once the forget has.
#[test]
fn a_turn_waits_while_its_scopes_forget_has_not_reached_the_account() {
    let (post, told) = recording(vec![Answer::Saved]);
    let unreached = Arc::new(Mutex::new(Some(A.to_owned())));
    let held = unreached.clone();
    let sender = Sender::new();
    sender.guarded(
        Arc::new(move |scope: &str| held.lock().unwrap().as_deref() != Some(scope)),
        Arc::new(|_: &[String]| {}),
    );
    offer(&sender, Some(A), 3);
    sender.spawn(post, fresh(), None, QUICK).unwrap();
    none_within(&told, Duration::from_millis(300));
    *unreached.lock().unwrap() = None;
    assert_eq!(ids(&next(&told)), [id(A, 3)]);
}

/// The Reviewer-lead's must-fix on #8091: a POST already on the wire when
/// a forget naming no ids (or a forget everything) of its person came is
/// let land, and once it answered its turns' ids are handed to the
/// forget's delete, so they never stay on the account; another person's
/// turn in it is not.
#[test]
fn a_post_in_flight_when_a_forget_naming_no_ids_came_is_deleted_after() {
    let (tell, told) = mpsc::channel();
    let (go, gate) = mpsc::channel::<Answer>();
    let post: Post = Box::new(move |body: &Value| {
        let _ = tell.send(body.clone());
        gate.recv_timeout(Duration::from_secs(5))
            .unwrap_or(Answer::Saved)
    });
    let deleted = Arc::new(Mutex::new(Vec::<String>::new()));
    let kept = deleted.clone();
    let sender = Sender::new();
    sender.guarded(
        Arc::new(|_: &str| true),
        Arc::new(move |ids: &[String]| kept.lock().unwrap().extend(ids.iter().cloned())),
    );
    offer(&sender, Some(A), 1);
    offer(&sender, Some(B), 2);
    sender.spawn(post, fresh(), None, QUICK).unwrap();
    assert_eq!(ids(&next(&told)), [id(A, 1), id(B, 2)]);
    // In flight: A's forget everything (the whole-person delete) comes.
    sender.forgetting(Some(A));
    sender.forget_answered(Some(A), true, Some(&json!({"ok":true})));
    go.send(Answer::Saved).unwrap();
    let started = Instant::now();
    while deleted.lock().unwrap().is_empty() {
        assert!(started.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(*deleted.lock().unwrap(), [id(A, 1)]);
    none_within(&told, Duration::from_millis(100));
}

/// Heron's P1 on #8125: a person removed in the app (`Days::forget_person`)
/// with a turn waiting in the save while the privacy switch is closed.
/// The whole-person delete the memory kept is read back (`recover`) and
/// goes through; once the switch opens, nothing of theirs is posted.
#[test]
fn a_removed_persons_waiting_turn_is_never_posted_after_their_delete() {
    let dir = scratch("removed-waiting");
    let store = lamp_memory::days::Days::open(
        lamp_memory::store::Options {
            dir: dir.join("memory"),
            keep_days: 30,
            local: lamp_memory::store::LocalTime::Offset(0),
        },
        1_760_356_800_000,
    )
    .unwrap();
    let store = Mutex::new(store);
    let ask = |line: &Value| -> Option<Value> {
        let answer = lamp_memory::days_protocol::answer(
            &mut store.lock().unwrap(),
            line.to_string().as_bytes(),
            1_760_356_800_000,
        );
        serde_json::from_str(answer.line.trim()).ok()
    };
    let kept = ask(
        &json!({"op":"append","person":A,"at_unix_ms":1_760_356_800_001u64,
        "heard":"h","said":"s"}),
    )
    .unwrap();
    assert_eq!(kept["ok"], true, "{kept}");
    let deletes: &'static crate::history_forget::Queue = Box::leak(Box::new(
        crate::history_forget::Queue::at(dir.join("history-forget.json")),
    ));
    let open = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let switch = open.clone();
    let (post, told) = recording(vec![Answer::Saved]);
    let sender = Sender::new();
    sender.guarded(
        Arc::new(move |scope: &str| {
            switch.load(std::sync::atomic::Ordering::SeqCst) && !deletes.pending().reaches(scope)
        }),
        Arc::new(|_: &[String]| {}),
    );
    // Their turn waits: the switch is closed.
    offer(&sender, Some(A), 1);
    sender.spawn(post, fresh(), None, QUICK).unwrap();
    none_within(&told, Duration::from_millis(200));
    // The app removes them, and the worker reads what the memory kept.
    let removed = ask(&json!({"op":"forget_person","person":A})).unwrap();
    assert_eq!(removed["ok"], true, "{removed}");
    let bell = tokio::sync::Notify::new();
    assert!(crate::history_forget::recover(
        deletes,
        &bell,
        ask,
        |answer| {
            sender.seen(answer);
        }
    ));
    assert!(deletes.pending().reaches(A), "{:?}", deletes.pending());
    // Their whole-person delete reaches the account, then the switch opens.
    deletes
        .change(|pending| {
            let call = pending.next().unwrap();
            pending.done(&call);
        })
        .unwrap();
    assert!(deletes.pending().is_empty(), "{:?}", deletes.pending());
    open.store(true, std::sync::atomic::Ordering::SeqCst);
    none_within(&told, Duration::from_millis(400));
    assert_eq!(queued(&sender), (vec![], vec![]));
}
