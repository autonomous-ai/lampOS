use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;

/// Waits short enough for a test.
const QUICK: Waits = Waits {
    first: Duration::from_millis(5),
    longest: Duration::from_millis(20),
};

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
fn a_turns_id_is_the_memorys_exchange_id_in_its_scope() {
    assert_eq!(
        turn_id(Some("p-0123456789abcdef"), 42),
        "p-0123456789abcdef:42"
    );
    assert_eq!(turn_id(None, 7), "everyone:7");
    let longest = turn_id(Some(&"a".repeat(32)), u64::MAX);
    assert!(longest.len() <= 64, "{}", longest.len());
}

#[test]
fn a_time_is_the_stores_utc() {
    // 2026-11-18T11:40:02Z.
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

/// The first leg the task names: a failed call is tried again with the
/// same ids, never new ones.
#[test]
fn a_retry_sends_the_same_ids() {
    let (post, told) = recording(vec![Answer::Again, Answer::Again, Answer::Saved]);
    let sender = Sender::new();
    sender.offer(Some("p-a"), 3, 1_000, "what time is it", "Ten past nine.");
    sender.offer(Some("p-a"), 4, 2_000, "thanks", "Any time.");
    sender.spawn(post, QUICK).unwrap();
    let first = next(&told);
    let second = next(&told);
    let third = next(&told);
    assert_eq!(ids(&first), ["p-a:3", "p-a:4"]);
    assert_eq!(first, second);
    assert_eq!(second, third);
    none_within(&told, Duration::from_millis(100));
    assert_eq!(sender.let_go(), 0);
}

/// A batch that never goes through is let go after its tries, counted.
#[test]
fn a_batch_is_let_go_after_its_tries() {
    let (post, told) = recording(vec![Answer::Again]);
    let sender = Sender::new();
    sender.offer(Some("p-a"), 1, 1_000, "h", "s");
    sender.spawn(post, QUICK).unwrap();
    for _ in 0..TRIES {
        assert_eq!(ids(&next(&told)), ["p-a:1"]);
    }
    none_within(&told, Duration::from_millis(150));
    assert_eq!(sender.let_go(), 1);
}

/// The store's switch off, or a refusal, is never tried again.
#[test]
fn off_and_refused_are_not_tried_again() {
    for answer in [Answer::Off, Answer::Refused(400)] {
        let (post, told) = recording(vec![answer]);
        let sender = Sender::new();
        sender.offer(Some("p-a"), 1, 1_000, "h", "s");
        sender.spawn(post, QUICK).unwrap();
        next(&told);
        none_within(&told, Duration::from_millis(100));
    }
}

/// The caps: 45 turns go as 20, 20 and 5, in order, and a text over 8 KB
/// is cut at a character's end.
#[test]
fn the_caps_split_a_batch() {
    let (post, told) = recording(vec![Answer::Saved]);
    let sender = Sender::new();
    for n in 1..=45 {
        sender.offer(Some("p-a"), n, 1_000 + n, "h", "s");
    }
    // 8,191 bytes of `a` then a 3-byte character: the cut leaves it out.
    let long = format!("{}{}", "a".repeat(TEXT_MOST - 1), "ệ".repeat(10));
    sender.offer(Some("p-a"), 46, 2_000, &long, "s");
    sender.spawn(post, QUICK).unwrap();
    let batches: Vec<Value> = (0..3).map(|_| next(&told)).collect();
    let sizes: Vec<usize> = batches.iter().map(|body| ids(body).len()).collect();
    assert_eq!(sizes, [20, 20, 6]);
    let all: Vec<String> = batches.iter().flat_map(ids).collect();
    let expected: Vec<String> = (1..=46).map(|n| format!("p-a:{n}")).collect();
    assert_eq!(all, expected);
    let heard = batches[2]["turns"][5]["heard"].as_str().unwrap();
    assert!(heard.len() <= TEXT_MOST, "{}", heard.len());
    assert_eq!(heard, "a".repeat(TEXT_MOST - 1));
    assert_eq!(capped("short"), "short");
    let exact = "b".repeat(TEXT_MOST);
    assert_eq!(capped(&exact), exact);
}

/// The turn as the store takes it: id, time, person (empty for the room),
/// heard and said.
#[test]
fn a_turn_is_sent_in_the_stores_shape() {
    let (post, told) = recording(vec![Answer::Saved]);
    let sender = Sender::new();
    sender.offer(
        Some("p-a"),
        9,
        1_763_466_002_000,
        "how are you",
        "Bright, thanks.",
    );
    sender.spawn(post, QUICK).unwrap();
    assert_eq!(
        next(&told),
        json!({"turns":[{"id":"p-a:9","at":"2025-11-18T11:40:02Z","person":"p-a","heard":"how are you","said":"Bright, thanks."}]})
    );
}

/// The room's lines wait for their conversation: a moved one goes as its
/// new person's under its new id, the rest as the room's at the end.
#[test]
fn room_turns_wait_for_their_conversation_and_follow_a_move() {
    let (post, told) = recording(vec![Answer::Saved]);
    let sender = Sender::new();
    sender.spawn(post, QUICK).unwrap();
    sender.offer(None, 5, 1_000, "hi lamp", "Hello.");
    sender.offer(None, 6, 2_000, "it's me", "Hi again.");
    none_within(&told, Duration::from_millis(100));
    sender.moved("p-a", &[(5, 12), (40, 41)]);
    let moved = next(&told);
    assert_eq!(moved["turns"][0]["id"], "p-a:12");
    assert_eq!(moved["turns"][0]["person"], "p-a");
    assert_eq!(moved["turns"].as_array().unwrap().len(), 1);
    // An end naming none of the held lines sends none.
    sender.settled(&[99]);
    none_within(&told, Duration::from_millis(50));
    sender.settled(&[6]);
    let settled = next(&told);
    assert_eq!(settled["turns"][0]["id"], "everyone:6");
    assert_eq!(settled["turns"][0]["person"], "");
    none_within(&told, Duration::from_millis(50));
}

/// A forget lets go of what is unsent and stops a batch's next try.
#[test]
fn a_forget_lets_go_of_what_is_unsent() {
    let (post, told) = recording(vec![Answer::Again]);
    let sender = Sender::new();
    sender.offer(Some("p-a"), 1, 1_000, "h", "s");
    sender.offer(None, 2, 1_000, "room", "s");
    let slow = Waits {
        first: Duration::from_millis(300),
        longest: Duration::from_millis(300),
    };
    sender.spawn(post, slow).unwrap();
    assert_eq!(ids(&next(&told)), ["p-a:1"]);
    sender.forgot();
    // Neither the batch's next try nor the held room line.
    none_within(&told, Duration::from_millis(500));
    sender.settled(&[2]);
    none_within(&told, Duration::from_millis(100));
    assert_eq!(sender.let_go(), 2);
    // A turn after the forget is sent.
    sender.offer(Some("p-a"), 3, 3_000, "h", "s");
    assert_eq!(ids(&next(&told)), ["p-a:3"]);
}

/// A full queue lets the oldest go, counted, and never blocks.
#[test]
fn a_full_queue_lets_the_oldest_go() {
    let sender = Sender::new();
    for n in 1..=(READY_MOST as u64 + 5) {
        sender.offer(Some("p-a"), n, n, "h", "s");
    }
    assert_eq!(sender.let_go(), 5);
    for n in 1..=(HELD_MOST as u64 + 3) {
        sender.offer(None, n, n, "h", "s");
    }
    assert_eq!(sender.let_go(), 8);
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
    for base in [
        "https://api.deepseek.com",
        "https://openrouter.ai/api/v1",
        "http://campaign-api.autonomous.ai/api/v1/ai",
        "https://near.example.invalid/api/v1/ai/v1",
        "https://campaign-api.autonomous.ai.example.invalid/api/v1/ai",
        "",
    ] {
        assert_eq!(url(base), None, "{base:?}");
        assert!(start_from(Some("on"), base, "k").is_none(), "{base:?}");
    }
}

/// The id leg (the first builder's HOLD on #7935): the save's id for an
/// exchange is the one Friday's forget sends for it, `<scope>:<id>` with the
/// room's scope `everyone` (`history_forget::turn_id`, whose tests read a
/// forget of the room's exchange 3 as `everyone:3`).
#[test]
fn a_saves_id_is_the_one_a_forget_sends() {
    let forget_sends = |scope: &str, id: u64| format!("{scope}:{id}");
    assert_eq!(turn_id(None, 3), "everyone:3");
    assert_eq!(turn_id(None, 3), forget_sends("everyone", 3));
    assert_eq!(turn_id(Some("p-0a1b"), 12), forget_sends("p-0a1b", 12));
    // And what the queue sends is that id, moved lines included.
    let (post, told) = recording(vec![Answer::Saved]);
    let sender = Sender::new();
    sender.spawn(post, QUICK).unwrap();
    sender.offer(None, 3, 1_000, "h", "s");
    sender.offer(None, 4, 1_000, "h", "s");
    sender.moved("p-0a1b", &[(4, 12)]);
    assert_eq!(ids(&next(&told)), [forget_sends("p-0a1b", 12)]);
    sender.settled(&[3]);
    assert_eq!(ids(&next(&told)), [forget_sends("everyone", 3)]);
}

/// The second leg: with the switch anything but `on`, nothing starts and
/// nothing reaches the store.
#[test]
fn nothing_is_sent_when_the_switch_is_off() {
    let (base, told) = slow_store(Duration::ZERO);
    for value in [None, Some("off"), Some("ON"), Some("")] {
        assert!(start_at(value, local(&base), "k").is_none(), "{value:?}");
        let relay = "https://campaign-api.autonomous.ai/api/v1/ai";
        assert!(start_from(value, relay, "k").is_none(), "{value:?}");
    }
    // Nor does the voice's own path, never started, send a thing.
    offered(Some("p-a"), 1, 1_000, "h", "s");
    settled(&[1]);
    assert!(told.recv_timeout(Duration::from_millis(200)).is_err());
}

/// The real call: the lamp's cloud API, the lamp's key as `x-api-key`, the
/// turns as JSON; and none without a key.
#[test]
fn the_call_goes_to_the_lamps_cloud_api_with_its_key() {
    let (base, told) = slow_store(Duration::ZERO);
    let sender = start_at(Some("on"), local(&base), " lamp-key \n").expect("started");
    sender.offer(Some("p-a"), 2, 1_000, "h", "s");
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
    assert_eq!(ids(&body), ["p-a:2"]);
    assert!(start_at(Some("on"), local(&base), "  ").is_none());
    assert!(start_at(Some("on"), None, "k").is_none());
}

/// The third leg: a store that takes 2 s to answer never delays the turn.
/// Every offer returns at once while a call hangs; measured, not assumed.
#[test]
fn the_reply_is_not_delayed_by_a_slow_server() {
    let (base, told) = slow_store(Duration::from_secs(2));
    let sender = start_at(Some("on"), local(&base), "k").expect("started");
    sender.offer(Some("p-a"), 1, 1_000, "h", "s");
    // The first call is in, and hangs for 2 s.
    told.recv_timeout(Duration::from_secs(5)).expect("a call");
    let mut slowest = Duration::ZERO;
    let started = Instant::now();
    for n in 2..=101 {
        let one = Instant::now();
        sender.offer(Some("p-a"), n, 1_000 + n, "h", "s");
        slowest = slowest.max(one.elapsed());
    }
    let all = started.elapsed();
    println!("100 offers beside a hanging call: {all:?} in all, slowest {slowest:?}");
    assert!(slowest < Duration::from_millis(20), "{slowest:?}");
    assert!(all < Duration::from_millis(100), "{all:?}");
}

/// The memory's writer hands on exactly what the memory wrote, under the
/// memory's own id, and only once it answered with one.
#[test]
fn the_writer_offers_what_the_memory_wrote_under_its_id() {
    let (post, told) = recording(vec![Answer::Saved]);
    let sender = Sender::new();
    sender.spawn(post, QUICK).unwrap();
    TEST_SENDER.with(|test| *test.borrow_mut() = Some(sender.clone()));
    crate::remember::write_for_history_test(&[
        (Some("p-a"), "kept by the memory"),
        (Some("p-a"), "refused by the memory"),
    ]);
    TEST_SENDER.with(|test| *test.borrow_mut() = None);
    let body = next(&told);
    assert_eq!(ids(&body), ["p-a:17"]);
    assert_eq!(body["turns"][0]["heard"], "kept by the memory");
    none_within(&told, Duration::from_millis(100));
}
