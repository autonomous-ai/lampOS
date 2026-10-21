//! Jev asked on this machine: a server on 127.0.0.1 that takes a
//! connection, reads the request and plays a script to it. No test here
//! sends anything off the machine.
use super::*;
use std::{
    net::TcpListener,
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicU16, AtomicUsize, Ordering},
    thread,
};

mod certificates;

/// A key no service has issued. It is looked for in everything a failure
/// says.
const KEY: &str = "sk-or-test-3f9c1a7e5b2d4086";
/// A state whose fields are not in the order of the alphabet, with letters
/// outside ASCII and something to escape.
const STATE: &str = "{\"zebra\":1,\"apple\":{\"heard\":\"Lamp ơi, \\\"mấy giờ\\\" rồi?\"}}";
/// An answer in the form the real service gives (`docs/policy.md`), to
/// these questions' names.
const GOOD: &str = "{\"model\":\"typesafe/jev-1.13-20260917\",\"answers\":{\"zeta_first\":{\"type\":\"noul\",\"noul\":0.88},\"alpha_second\":{\"type\":\"choice\",\"choice\":\"option_a\",\"probabilities\":{\"option_a\":0.94,\"option_b\":0.06},\"confidence\":0.91}},\"usage\":{\"input_tokens\":1066,\"output_tokens\":134,\"cost\":4.4772e-05}}";
const LONG: Duration = Duration::from_secs(5);
/// How far past its deadline an asking may come back. The deadline is
/// kept by the system's socket timeouts, which are not exact.
const MARGIN: Duration = Duration::from_millis(250);

/// Two questions, one of each type, named so that the order they are
/// asked in is not the order of the alphabet.
fn questions() -> Vec<(String, Question)> {
    vec![
        (
            "zeta_first".into(),
            Question::Noul {
                instructions: "Is it \"day\"?".into(),
            },
        ),
        (
            "alpha_second".into(),
            Question::Choice {
                instructions: "Which one?".into(),
                criteria: vec![
                    ("option_b".into(), "the second".into()),
                    ("option_a".into(), "the first".into()),
                ],
            },
        ),
    ]
}

fn key_file(name: &str, holds: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("lamp-policy-key-{name}-{}", std::process::id()));
    std::fs::write(&path, holds).unwrap();
    path
}

/// Jev at `url`, with the key in a file as an editor leaves it: a line
/// end after it. The file is gone again before the result is looked at, so
/// that a test that fails here leaves nothing behind.
fn jev(url: &str, name: &str) -> Jev {
    let path = key_file(name, &format!("{KEY}\n"));
    let jev = Jev::new(url, "typesafe/jev-1.13", &path);
    std::fs::remove_file(path).unwrap();
    jev.unwrap()
}

/// Whatever a failure says, it says nothing of the key or of the state.
fn says_nothing_secret(failed: &Failed) {
    for text in [failed.to_string(), format!("{failed:?}")] {
        assert!(!text.contains(KEY), "{text}");
        assert!(!text.contains("sk-or"), "{text}");
        assert!(!text.contains("Lamp ơi"), "{text}");
        assert!(!text.contains("zebra"), "{text}");
    }
}

/// One end of a connection, plain or secure.
trait Wire: Read + Write {}
impl<T: Read + Write> Wire for T {}

/// A server on 127.0.0.1 for the length of a test.
struct Server {
    port: u16,
    /// Each request as it was read: `None` for a connection that ended
    /// before it had sent a whole one.
    requests: mpsc::Receiver<Option<String>>,
    /// How many connections it has taken.
    connections: Arc<AtomicUsize>,
    done: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Server {
    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/api/alpha/decisions", self.port)
    }

    /// The next request that was read whole.
    fn request(&self) -> String {
        self.requests
            .recv_timeout(LONG)
            .expect("a connection")
            .expect("a whole request")
    }
}

impl Server {
    /// Wait until it has taken `count` connections. It takes one at a
    /// time, so the next is counted only once the one before has ended.
    fn has_taken(&self, count: usize) {
        let since = Instant::now();
        while self.connections.load(Ordering::SeqCst) < count {
            assert!(since.elapsed() < LONG, "fewer than {count} connections");
            thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(self.connections.load(Ordering::SeqCst), count);
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.done.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let ended = thread.join();
            if !thread::panicking() {
                ended.expect("the server's script");
            }
        }
    }
}

/// A request read whole: the head, and as much body as it says it has.
fn read_request(stream: &mut dyn Wire) -> Option<String> {
    let mut held = Vec::new();
    let mut piece = [0u8; 4096];
    let body = loop {
        if let Some(at) = held.windows(4).position(|four| four == b"\r\n\r\n") {
            break at + 4;
        }
        match stream.read(&mut piece) {
            Ok(read) if read > 0 => held.extend_from_slice(&piece[..read]),
            _ => return None,
        }
    };
    let head = String::from_utf8_lossy(&held[..body]).to_lowercase();
    let length: usize = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .and_then(|length| length.trim().parse().ok())?;
    while held.len() < body + length {
        match stream.read(&mut piece) {
            Ok(read) if read > 0 => held.extend_from_slice(&piece[..read]),
            _ => return None,
        }
    }
    String::from_utf8(held).ok()
}

/// Take connections until the test ends. Each is made into a `Wire` by
/// `wrap`, its request is read and told, and `script` is played to it:
/// once, and the connection is closed, or with `keep` for every request
/// that comes on it until the client ends it.
fn serve_as(
    wrap: impl Fn(TcpStream) -> Box<dyn Wire> + Send + 'static,
    script: impl Fn(&mut dyn Wire) + Send + 'static,
    keep: bool,
) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let (told, requests) = mpsc::channel();
    let done = Arc::new(AtomicBool::new(false));
    let ended = Arc::clone(&done);
    let connections = Arc::new(AtomicUsize::new(0));
    let taken = Arc::clone(&connections);
    let thread = thread::spawn(move || {
        while !ended.load(Ordering::SeqCst) {
            let Ok((socket, _)) = listener.accept() else {
                thread::sleep(Duration::from_millis(2));
                continue;
            };
            taken.fetch_add(1, Ordering::SeqCst);
            // A socket taken from a listener that does not wait may not
            // wait either, and a client that has gone must not hold the
            // test.
            socket.set_nonblocking(false).unwrap();
            socket.set_read_timeout(Some(LONG)).unwrap();
            socket.set_write_timeout(Some(LONG)).unwrap();
            let mut stream = wrap(socket);
            let mut first = true;
            loop {
                let request = read_request(stream.as_mut());
                let whole = request.is_some();
                // A kept connection that the client ends after its last
                // request is not a request that was cut short.
                if whole || first {
                    let _ = told.send(request);
                }
                if whole {
                    script(stream.as_mut());
                }
                first = false;
                if !(whole && keep) {
                    break;
                }
            }
        }
    });
    Server {
        port,
        requests,
        connections,
        done,
        thread: Some(thread),
    }
}

fn serve(script: impl Fn(&mut dyn Wire) + Send + 'static) -> Server {
    serve_as(|socket| Box::new(socket), script, false)
}

/// A server that keeps each connection and answers every request on it
/// with `answer`.
fn keeping(answer: Vec<u8>) -> Server {
    serve_as(
        |socket| Box::new(socket),
        move |stream| {
            let _ = stream.write_all(&answer);
        },
        true,
    )
}

/// `body` as a 200 that says its length and nothing of closing.
fn kept_with_length(body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

/// `body` as a 200 in two chunks, with `end` after the last of them.
fn in_chunks(body: &str, end: &str) -> Vec<u8> {
    let (one, two) = body.split_at(body.len() / 2);
    format!(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{one}\r\n{:x}\r\n{two}\r\n0\r\n{end}",
        one.len(),
        two.len()
    )
    .into_bytes()
}

/// A server that answers every request with `answer`, bytes as they are.
/// A client that stops reading is the test's own doing.
fn answering(answer: Vec<u8>) -> Server {
    serve(move |stream| {
        let _ = stream.write_all(&answer);
    })
}

/// `body` as a 200 that says its length.
fn with_length(body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn ask(server: &Server, name: &str) -> Result<Asked, Failed> {
    jev(&server.url(), name).ask(STATE, &questions(), LONG)
}

/// The asking fails as `expected`, and says nothing secret doing so.
fn fails(server: &Server, name: &str, expected: Failed) {
    let failed = ask(server, name).expect_err("a failure");
    says_nothing_secret(&failed);
    assert_eq!(failed, expected);
}

fn good(asked: &Asked) {
    assert_eq!(asked.model, "typesafe/jev-1.13-20260917");
    assert_eq!(asked.answers.len(), 2);
    assert_eq!(asked.answers["zeta_first"], Answer::Noul(0.88));
    assert_eq!(
        asked.answers["alpha_second"],
        Answer::Choice {
            choice: "option_a".into(),
            probabilities: [
                ("option_a".to_string(), 0.94),
                ("option_b".to_string(), 0.06)
            ]
            .into(),
        }
    );
    assert_eq!(
        asked.usage,
        Some(Usage {
            input_tokens: Some(1066),
            output_tokens: Some(134),
            cost: Some(4.4772e-05),
        })
    );
    assert!(asked.took < LONG);
}

#[test]
fn a_good_answer_is_read_by_its_length_and_the_request_is_the_one_the_service_takes() {
    let server = answering(with_length(GOOD));
    let mut jev = jev(&server.url(), "length");
    assert_eq!(jev.name(), "typesafe/jev-1.13");
    good(&jev.ask(STATE, &questions(), LONG).unwrap());

    let request = server.request();
    let (head, body) = request.split_once("\r\n\r\n").unwrap();
    let mut lines = head.split("\r\n");
    assert_eq!(lines.next(), Some("POST /api/alpha/decisions HTTP/1.1"));
    let headers: BTreeMap<String, &str> = lines
        .map(|line| line.split_once(": ").unwrap())
        .map(|(name, value)| (name.to_lowercase(), value))
        .collect();
    // The key is the file's one line without its line end, and it is in
    // this header and nowhere else.
    assert_eq!(headers["authorization"], format!("Bearer {KEY}"));
    assert_eq!(request.matches(KEY).count(), 1);
    assert_eq!(headers["host"], format!("127.0.0.1:{}", server.port));
    assert_eq!(headers["content-type"], "application/json");
    assert_eq!(headers["connection"], "keep-alive");
    assert_eq!(headers["accept-encoding"], "identity");
    assert_eq!(headers["content-length"], body.len().to_string());
    // The state as it was given and the questions in the order they were
    // asked in, neither in the order of the alphabet.
    assert_eq!(
        body,
        "{\"model\":\"typesafe/jev-1.13\",\"state\":{\"zebra\":1,\"apple\":{\"heard\":\"Lamp ơi, \\\"mấy giờ\\\" rồi?\"}},\"questions\":{\"zeta_first\":{\"type\":\"noul\",\"instructions\":\"Is it \\\"day\\\"?\"},\"alpha_second\":{\"type\":\"choice\",\"instructions\":\"Which one?\",\"criteria\":{\"option_b\":\"the second\",\"option_a\":\"the first\"}}}}"
    );
    // And it is JSON that says what was asked.
    let sent: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(
        sent["state"],
        serde_json::from_str::<serde_json::Value>(STATE).unwrap()
    );
    assert_eq!(
        sent["questions"]["alpha_second"]["criteria"]["option_a"],
        "the first"
    );
}

#[test]
fn a_service_that_says_it_will_close_is_asked_again_on_a_new_connection() {
    let server = answering(with_length(GOOD));
    let mut jev = jev(&server.url(), "again");
    for asked in 1..=3 {
        let answer = jev.ask(STATE, &questions(), LONG).unwrap();
        good(&answer);
        assert_eq!(answer.connection, Some(Connection::New));
        assert!(server.request().contains("Connection: keep-alive\r\n"));
        assert!(jev.kept.is_none());
        assert_eq!(server.connections.load(Ordering::SeqCst), asked);
    }
}

#[test]
fn a_connection_is_kept_for_the_next_asking() {
    // By its length, and in chunks that end as they should.
    for (name, answer) in [
        ("kept-length", kept_with_length(GOOD)),
        ("kept-chunks", in_chunks(GOOD, "\r\n")),
    ] {
        let server = keeping(answer);
        let mut jev = jev(&server.url(), name);
        let mut on = Vec::new();
        for _ in 0..4 {
            let answer = jev.ask(STATE, &questions(), LONG).unwrap();
            good(&answer);
            on.push(answer.connection.unwrap());
            server.request();
        }
        assert_eq!(server.connections.load(Ordering::SeqCst), 1, "{name}");
        assert_eq!(
            on,
            [
                Connection::New,
                Connection::Kept,
                Connection::Kept,
                Connection::Kept
            ],
            "{name}"
        );
    }
}

#[test]
fn a_connection_whose_answer_did_not_end_cleanly_is_not_used_again() {
    for (name, answer) in [
        // Lines after the last chunk.
        ("trailer", in_chunks(GOOD, "Checksum: 1\r\n\r\n")),
        // More than its length said.
        ("surplus", {
            let mut answer = kept_with_length(GOOD);
            answer.extend_from_slice(b"\r\n");
            answer
        }),
    ] {
        let server = keeping(answer);
        let mut jev = jev(&server.url(), name);
        for asked in 1..=2 {
            good(&jev.ask(STATE, &questions(), LONG).unwrap());
            assert!(jev.kept.is_none(), "{name}");
            assert_eq!(server.connections.load(Ordering::SeqCst), asked, "{name}");
        }
    }
    // The end of the last chunk that has not come yet is not waited for:
    // the answer is whole without it.
    let server = serve(|stream| {
        let _ = stream.write_all(&in_chunks(GOOD, ""));
        let _ = stream.flush();
        thread::sleep(Duration::from_millis(600));
    });
    let mut jev = jev(&server.url(), "no-end");
    let asked = jev.ask(STATE, &questions(), LONG).unwrap();
    good(&asked);
    assert!(asked.took < Duration::from_millis(400), "{:?}", asked.took);
    assert!(jev.kept.is_none());
}

#[test]
fn a_kept_connection_the_service_has_closed_is_replaced_and_nothing_fails() {
    // It answers as if it would keep the connection, and closes it.
    let server = answering(kept_with_length(GOOD));
    let mut jev = jev(&server.url(), "closed-quietly");
    for asked in 1..=3 {
        let answer = jev.ask(STATE, &questions(), LONG).unwrap();
        good(&answer);
        let on = if asked == 1 {
            Connection::New
        } else {
            Connection::Replaced
        };
        assert_eq!(answer.connection, Some(on));
        assert!(jev.kept.is_some());
        assert_eq!(server.connections.load(Ordering::SeqCst), asked);
        // Time for the closing to arrive, as it would have between two
        // things said.
        thread::sleep(Duration::from_millis(30));
    }
    // Each request was read whole once: the one on the dead connection
    // reached nobody.
    for _ in 0..3 {
        server.request();
    }
    assert!(server.requests.try_recv().is_err());
}

#[test]
fn a_connection_is_not_kept_longer_than_it_is_taken_to_last() {
    let server = keeping(kept_with_length(GOOD));
    let mut jev = jev(&server.url(), "outlived");
    jev.keeping.answered = Duration::from_millis(80);
    good(&jev.ask(STATE, &questions(), LONG).unwrap());
    good(&jev.ask(STATE, &questions(), LONG).unwrap());
    assert_eq!(server.connections.load(Ordering::SeqCst), 1);
    thread::sleep(Duration::from_millis(120));
    let answer = jev.ask(STATE, &questions(), LONG).unwrap();
    good(&answer);
    // Not found out to have ended: let go for its age.
    assert_eq!(answer.connection, Some(Connection::New));
    assert_eq!(server.connections.load(Ordering::SeqCst), 2);
}

#[test]
fn getting_ready_makes_the_connection_the_question_is_then_asked_on() {
    let server = keeping(kept_with_length(GOOD));
    let mut jev = jev(&server.url(), "ready");
    jev.ready(LONG);
    assert!(jev.kept.is_some());
    // Made, with nothing on it.
    server.has_taken(1);
    assert!(server.requests.try_recv().is_err());
    // Ready already: nothing more is made.
    jev.ready(LONG);
    let answer = jev.ask(STATE, &questions(), LONG).unwrap();
    good(&answer);
    assert_eq!(answer.connection, Some(Connection::Ahead));
    server.request();
    assert_eq!(server.connections.load(Ordering::SeqCst), 1);
    let answer = jev.ask(STATE, &questions(), LONG).unwrap();
    assert_eq!(answer.connection, Some(Connection::Kept));
    // One that has not long to go is replaced ahead of the question.
    jev.keeping.ready_margin = Duration::from_secs(3_600);
    jev.ready(LONG);
    good(&jev.ask(STATE, &questions(), LONG).unwrap());
    assert_eq!(server.connections.load(Ordering::SeqCst), 2);
    // An unused one is taken to last its own, shorter time.
    jev.keeping.unused = Duration::from_millis(40);
    jev.ready(LONG);
    server.has_taken(3);
    thread::sleep(Duration::from_millis(70));
    good(&jev.ask(STATE, &questions(), LONG).unwrap());
    assert_eq!(server.connections.load(Ordering::SeqCst), 4);
}

#[test]
fn getting_ready_for_a_service_that_is_not_there_is_silent_and_the_asking_says_so() {
    let free = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = free.local_addr().unwrap().port();
    drop(free);
    let mut jev = jev(&format!("http://127.0.0.1:{port}/"), "ready-nobody");
    jev.ready(Duration::from_millis(300));
    assert!(jev.kept.is_none());
    let failed = jev.ask(STATE, &questions(), LONG).unwrap_err();
    assert!(matches!(failed, Failed::Unreachable { .. }), "{failed:?}");
    says_nothing_secret(&failed);
}

#[test]
fn the_same_answer_sent_in_chunks_is_read() {
    // Chunks of uneven sizes, one with an extension after its length,
    // sent a few bytes at a time so that every boundary is crossed
    // between two reads.
    let mut answer = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();
    let mut rest = GOOD.as_bytes();
    for size in [1, 7, 64, 2, 4096] {
        let (chunk, after) = rest.split_at(size.min(rest.len()));
        rest = after;
        if chunk.is_empty() {
            break;
        }
        answer.extend_from_slice(format!("{:X};note=1\r\n", chunk.len()).as_bytes());
        answer.extend_from_slice(chunk);
        answer.extend_from_slice(b"\r\n");
    }
    answer.extend_from_slice(b"0\r\n\r\n");
    let whole = answer.clone();
    let server = serve(move |stream| {
        for few in answer.chunks(5) {
            stream.write_all(few).unwrap();
            stream.flush().unwrap();
            thread::sleep(Duration::from_micros(300));
        }
    });
    good(&ask(&server, "chunked").unwrap());
    // All at once is read the same.
    let server = answering(whole);
    good(&ask(&server, "chunked-whole").unwrap());
}

#[test]
fn a_slow_server_does_not_hold_the_asker_past_the_deadline() {
    // The request is taken and nothing is answered until the test ends.
    let (release, held) = mpsc::channel::<()>();
    let server = serve(move |_| {
        let _ = held.recv_timeout(LONG);
    });
    let mut jev = jev(&server.url(), "slow");
    for deadline in [Duration::from_millis(300), Duration::from_millis(120)] {
        let started = Instant::now();
        let failed = jev.ask(STATE, &questions(), deadline).unwrap_err();
        let took = started.elapsed();
        assert_eq!(failed, Failed::TimedOut);
        says_nothing_secret(&failed);
        assert!(took >= deadline, "{took:?} before {deadline:?}");
        assert!(took < deadline + MARGIN, "{took:?} for {deadline:?}");
    }
    drop(release);
}

#[test]
fn an_answer_that_comes_a_little_at_a_time_cannot_stretch_the_deadline() {
    // Each read gets something, so no single read ever times out by
    // itself: the deadline has to be over all of them together.
    let server = serve(|stream| {
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5000\r\n\r\n");
        for _ in 0..100 {
            if stream
                .write_all(b"{")
                .and_then(|()| stream.flush())
                .is_err()
            {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
    });
    let deadline = Duration::from_millis(300);
    let started = Instant::now();
    let failed = jev(&server.url(), "drip")
        .ask(STATE, &questions(), deadline)
        .unwrap_err();
    let took = started.elapsed();
    assert_eq!(failed, Failed::TimedOut);
    assert!(took >= deadline && took < deadline + MARGIN, "{took:?}");
}

#[test]
fn a_status_other_than_200_is_a_refusal_and_its_body_is_not_passed_on() {
    for status in [500, 401, 429, 404, 204] {
        // A service's error may well repeat what it was sent.
        let body =
            format!("{{\"error\":{{\"message\":\"bad request: {STATE} with Bearer {KEY}\"}}}}");
        let server = answering(
            format!(
                "HTTP/1.1 {status} Whatever\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .into_bytes(),
        );
        fails(&server, "refused", Failed::Refused(status));
    }
    assert_eq!(Failed::Refused(500).kind(), "refused");
    assert_eq!(
        Failed::Refused(500).to_string(),
        "the decision model's service refused with HTTP 500"
    );
}

#[test]
fn a_body_that_is_not_an_answer_is_unreadable() {
    for (body, why) in [
        ("<html>busy</html>", "it is not JSON"),
        ("", "it is not JSON"),
        ("{\"answers\":", "it is not JSON"),
        ("[]", "it does not have the shape of an answer"),
        (
            "{\"model\":\"m\"}",
            "it does not have the shape of an answer",
        ),
        ("{\"answers\":3}", "it does not have the shape of an answer"),
        (
            "{\"model\":7,\"answers\":{}}",
            "it does not have the shape of an answer",
        ),
    ] {
        let server = answering(with_length(body));
        fails(&server, "unreadable", Failed::Unreadable(why));
    }
    assert_eq!(Failed::Unreadable("x").kind(), "unreadable");
}

#[test]
fn an_answer_that_leaves_out_a_question_has_failed_whole() {
    let server = answering(with_length(
        "{\"model\":\"m\",\"answers\":{\"zeta_first\":{\"type\":\"noul\",\"noul\":0.88}}}",
    ));
    fails(&server, "missing", Failed::Missing("alpha_second".into()));
    let server = answering(with_length("{\"model\":\"m\",\"answers\":{}}"));
    fails(&server, "missing-all", Failed::Missing("zeta_first".into()));
    assert_eq!(Failed::Missing("q".into()).kind(), "missing");
}

#[test]
fn a_probability_of_one_and_a_half_is_no_answer() {
    let server = answering(with_length(&GOOD.replace("0.88", "1.5")));
    fails(
        &server,
        "probability",
        Failed::Unreadable("a probability is not a number from 0 to 1"),
    );
}

#[test]
fn every_answer_is_held_to_its_question() {
    let asked = questions();
    let read = |text: &str| answers(text.as_bytes(), &asked);
    let good = read(GOOD).unwrap();
    assert_eq!(good.model.as_deref(), Some("typesafe/jev-1.13-20260917"));
    // The two ends of a probability are probabilities, a whole number is a
    // number, and the sum of a choice's may be 0.02 off.
    for fine in [
        GOOD.replace("0.88", "0"),
        GOOD.replace("0.88", "1"),
        GOOD.replace("0.88", "1.0"),
        GOOD.replace("0.06", "0.079"),
        GOOD.replace("0.06", "0.041"),
        // What nobody asked for is passed over, and so is what an answer
        // says beside what it must.
        GOOD.replace(
            "\"answers\":{",
            "\"answers\":{\"extra\":{\"type\":\"essay\"},",
        ),
        // No model named, and usage that is not usage.
        GOOD.replace("\"model\":\"typesafe/jev-1.13-20260917\",", ""),
        GOOD.replace(
            "{\"input_tokens\":1066,\"output_tokens\":134,\"cost\":4.4772e-05}",
            "\"free\"",
        ),
    ] {
        assert!(read(&fine).is_ok(), "{fine}");
    }
    assert_eq!(
        read(&GOOD.replace("\"model\":\"typesafe/jev-1.13-20260917\",", ""))
            .unwrap()
            .model,
        None
    );
    assert_eq!(
        read(&GOOD.replace(
            "{\"input_tokens\":1066,\"output_tokens\":134,\"cost\":4.4772e-05}",
            "\"free\""
        ))
        .unwrap()
        .usage,
        None
    );
    let part = read(&GOOD.replace(",\"output_tokens\":134,\"cost\":4.4772e-05", "")).unwrap();
    assert_eq!(
        part.usage,
        Some(Usage {
            input_tokens: Some(1066),
            output_tokens: None,
            cost: None
        })
    );
    let probability = "a probability is not a number from 0 to 1";
    let another = "an answer is of another type than its question";
    let unnamed = "the model it names has no usable name";
    for (wrong, why) in [
        (GOOD.replace("typesafe/jev-1.13-20260917", ""), unnamed),
        (
            GOOD.replace("jev-1.13-20260917", "jev\\nthe second line"),
            unnamed,
        ),
        (GOOD.replace("jev-1.13-20260917", &"v".repeat(200)), unnamed),
        (GOOD.replace("0.88", "1.0001"), probability),
        (GOOD.replace("0.88", "-0.1"), probability),
        (GOOD.replace("0.88", "\"0.88\""), probability),
        (GOOD.replace("0.88", "null"), probability),
        (GOOD.replace("\"noul\":0.88", "\"yes\":0.88"), probability),
        (GOOD.replace("0.94", "1.2"), probability),
        (GOOD.replace("0.94", "\"most\""), probability),
        (
            GOOD.replace("\"type\":\"noul\"", "\"type\":\"choice\""),
            another,
        ),
        (GOOD.replace("\"type\":\"noul\",", ""), another),
        (
            GOOD.replace("\"type\":\"choice\"", "\"type\":\"noul\""),
            another,
        ),
        (
            GOOD.replace(
                "\"zeta_first\":{\"type\":\"noul\",\"noul\":0.88}",
                "\"zeta_first\":0.88",
            ),
            another,
        ),
        (
            GOOD.replace("\"choice\":\"option_a\"", "\"choice\":\"option_c\""),
            "a choice is not one of the options",
        ),
        (
            GOOD.replace("\"choice\":\"option_a\",", ""),
            "a choice is not one of the options",
        ),
        (
            GOOD.replace("\"option_b\":0.06", "\"option_c\":0.06"),
            "a probability is for something that is not an option",
        ),
        (
            GOOD.replace(
                "\"probabilities\":{\"option_a\":0.94,\"option_b\":0.06},",
                "",
            ),
            "a choice has no probabilities",
        ),
        (
            GOOD.replace("0.06", "0.09"),
            "a choice's probabilities do not add up to 1",
        ),
        (
            GOOD.replace("0.06", "0.03"),
            "a choice's probabilities do not add up to 1",
        ),
        (
            GOOD.replace(",\"option_b\":0.06", ""),
            "a choice's probabilities do not add up to 1",
        ),
    ] {
        match read(&wrong) {
            Err(failed) => assert_eq!(failed, Failed::Unreadable(why), "{wrong}"),
            Ok(_) => panic!("taken: {wrong}"),
        }
    }
}

#[test]
fn a_service_that_names_no_model_is_taken_to_have_used_the_one_asked_for() {
    let server = answering(with_length(
        &GOOD.replace("\"model\":\"typesafe/jev-1.13-20260917\",", ""),
    ));
    assert_eq!(ask(&server, "unnamed").unwrap().model, "typesafe/jev-1.13");
}

#[test]
fn an_answer_larger_than_the_cap_is_not_read() {
    let too_large = Failed::Unreadable("it is larger than 256 KiB");
    // It says so itself: nothing of it is read.
    let server = answering(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
            MOST_ANSWER_BYTES + 1,
            " ".repeat(MOST_ANSWER_BYTES + 1)
        )
        .into_bytes(),
    );
    fails(&server, "large", too_large.clone());
    // It does not say: it is read up to the cap and no further.
    let mut answer = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
    for _ in 0..5 {
        answer.extend_from_slice(b"10000\r\n");
        answer.extend_from_slice(&[b' '; 0x10000]);
        answer.extend_from_slice(b"\r\n");
    }
    answer.extend_from_slice(b"0\r\n\r\n");
    let server = answering(answer);
    fails(&server, "large-chunks", too_large.clone());
    // One chunk that claims more than any machine holds.
    let server = answering(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nFFFFFFFFFFFFFFF\r\n".to_vec(),
    );
    fails(&server, "large-claim", too_large);
    // An answer of exactly the cap is read, and then found not to be one.
    let server = answering(with_length(&" ".repeat(MOST_ANSWER_BYTES)));
    fails(&server, "at-the-cap", Failed::Unreadable("it is not JSON"));
}

#[test]
fn an_answer_that_is_cut_short_or_cannot_be_measured_is_unreadable() {
    let early = Failed::Unreadable("it ended before it was whole");
    for (answer, expected) in [
        (
            "HTTP/1.1 200 OK\r\nContent-Length: 500\r\n\r\n{\"model\":",
            early.clone(),
        ),
        (
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n40\r\n{\"model\":",
            early.clone(),
        ),
        (
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4",
            early,
        ),
        (
            "HTTP/1.1 200 OK\r\nContent-Type: appl",
            Failed::Unreadable("it ended before its headers were whole"),
        ),
        (
            "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{}",
            Failed::Unreadable("it does not say how long it is"),
        ),
        (
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Length: 3\r\n\r\n{}",
            Failed::Unreadable("it gives two lengths"),
        ),
        (
            "HTTP/1.1 200 OK\r\nContent-Length: two\r\n\r\n{}",
            Failed::Unreadable("its length is not a number"),
        ),
        (
            "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 2\r\n\r\n{}",
            Failed::Unreadable("it is compressed"),
        ),
        (
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip, chunked\r\n\r\n0\r\n\r\n",
            Failed::Unreadable("it is sent in a coding this cannot read"),
        ),
        (
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nxyz\r\n",
            Failed::Unreadable("a chunk's length cannot be read"),
        ),
        (
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}xx0\r\n\r\n",
            Failed::Unreadable("a chunk does not end where it says"),
        ),
        (
            "SSH-2.0-OpenSSH_9.6\r\n",
            Failed::Unreadable("it is not an HTTP answer"),
        ),
        (
            "",
            Failed::Unreachable {
                during: "reading",
                kind: io::ErrorKind::UnexpectedEof,
            },
        ),
    ] {
        let server = answering(answer.as_bytes().to_vec());
        fails(&server, "cut", expected);
    }
    // Headers without end.
    let server =
        answering(format!("HTTP/1.1 200 OK\r\nX-Filler: {}", "x".repeat(20_000)).into_bytes());
    fails(
        &server,
        "headers",
        Failed::Unreadable("its headers are longer than 16 KiB"),
    );
}

#[test]
fn with_nothing_listening_the_service_is_unreachable() {
    // A port that was free a moment ago.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let started = Instant::now();
    let failed = jev(
        &format!("http://127.0.0.1:{port}/api/alpha/decisions"),
        "nobody",
    )
    .ask(STATE, &questions(), LONG)
    .unwrap_err();
    assert_eq!(
        failed,
        Failed::Unreachable {
            during: "connecting",
            kind: io::ErrorKind::ConnectionRefused,
        }
    );
    says_nothing_secret(&failed);
    assert_eq!(failed.kind(), "unreachable");
    assert_eq!(
        failed.to_string(),
        "the decision model was not reached while connecting: connection refused"
    );
    // It is told at once, not at the deadline.
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn a_key_file_that_is_missing_or_holds_no_key_is_refused_without_showing_it() {
    let url = "http://127.0.0.1:9/";
    let missing = std::env::temp_dir().join(format!("lamp-policy-no-key-{}", std::process::id()));
    let error = Jev::new(url, "m", &missing).err().expect("refused");
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert!(error.to_string().contains("lamp-policy-no-key-"), "{error}");
    for (name, holds) in [
        ("empty", String::new()),
        ("blank", " \n\t\n".to_owned()),
        ("spaced", format!("Bearer {KEY}\n")),
        ("two-lines", format!("{KEY}\n{KEY}\n")),
        ("control", format!("{KEY}\u{7}")),
        ("not-ascii", format!("{KEY}ơ")),
    ] {
        let path = key_file(name, &holds);
        let refused = Jev::new(url, "m", &path).err();
        std::fs::remove_file(&path).unwrap();
        let error = refused.expect("refused");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{name}");
        let said = error.to_string();
        assert!(said.contains("does not hold one key"), "{said}");
        assert!(!said.contains(KEY) && !said.contains("sk-or"), "{said}");
    }
    // Space around the one line is not part of the key.
    let path = key_file("padded", &format!("\n  {KEY} \r\n\n"));
    let padded = Jev::new(url, "m", &path);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(padded.unwrap().authorization, format!("Bearer {KEY}"));
    // The endpoint and the model are checked before the file is read.
    for (endpoint, model) in [("ftp://127.0.0.1/", "m"), (url, ""), (url, "a\nb")] {
        let error = Jev::new(endpoint, model, &missing).err().expect("refused");
        assert_eq!(
            error.kind(),
            io::ErrorKind::InvalidInput,
            "{endpoint} {model:?}"
        );
    }
}

#[test]
fn an_endpoint_is_a_scheme_a_host_perhaps_a_port_and_a_path() {
    let parsed = |url: &str| Endpoint::parse(url).unwrap();
    let default = parsed(DEFAULT_ENDPOINT);
    assert_eq!(
        default,
        Endpoint {
            secure: true,
            host: "openrouter.ai".into(),
            port: 443,
            path: "/api/alpha/decisions".into(),
        }
    );
    assert_eq!(default.authority(), "openrouter.ai");
    let local = parsed("http://127.0.0.1:8080/decisions?x=1");
    assert_eq!((local.secure, local.port), (false, 8080));
    assert_eq!(local.path, "/decisions?x=1");
    assert_eq!(local.authority(), "127.0.0.1:8080");
    assert_eq!(parsed("https://lamp.local").path, "/");
    assert_eq!(parsed("https://lamp.local").port, 443);
    assert_eq!(parsed("https://lamp.local:443/").authority(), "lamp.local");
    assert_eq!(parsed("http://localhost").port, 80);
    assert_eq!(parsed("http://localhost:80/").authority(), "localhost");
    assert_eq!(parsed("http://127.0.0.9/").host, "127.0.0.9");
    assert_eq!(
        parsed("https://lamp.local:8443/").authority(),
        "lamp.local:8443"
    );
    let six = parsed("http://[::1]:8080/x");
    assert_eq!((six.host.as_str(), six.port), ("::1", 8080));
    assert_eq!(six.authority(), "[::1]:8080");
    assert_eq!(parsed("https://[::1]/x").authority(), "[::1]");
    // Plain http is for this machine and no other.
    for elsewhere in [
        "http://openrouter.ai/api/alpha/decisions",
        "http://192.168.1.10:8080/",
        "http://lamp.local/",
        "http://localhost.example/",
        "http://[2001:db8::1]/",
        "http://0.0.0.0/",
    ] {
        let why = Endpoint::parse(elsewhere).expect_err(elsewhere);
        assert!(why.contains("plain http to another machine"), "{why}");
    }
    for refused in [
        "",
        "openrouter.ai/api",
        "ftp://openrouter.ai/",
        "HTTPS://openrouter.ai/",
        "https://",
        "https:///path",
        "https://:443/",
        "https://host:/",
        "https://host:0/",
        "https://host:65536/",
        "https://host:http/",
        "https://user@host/",
        "https://user:word@host/",
        "https://host/a b",
        "https://host/a\tb",
        "https://host/a#b",
        "https://ho st/",
        "https://host_name/",
        "https://[::1/",
        "https://[::1]8080/",
        "https://::1/",
        "https://[not:an:address]/",
    ] {
        let why = Endpoint::parse(refused).expect_err(refused);
        assert!(why.starts_with("endpoint "), "{why}");
    }
}

static LOOKUPS: AtomicUsize = AtomicUsize::new(0);
static LOOKED_UP_PORT: AtomicU16 = AtomicU16::new(0);
static LOOKUP_GATE: std::sync::Mutex<Option<mpsc::Receiver<()>>> = std::sync::Mutex::new(None);

/// The first lookup stays pending until the deadline checks release it.
/// Later lookups find the test's current server without waiting.
fn slow_lookup(host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
    assert_eq!((host, port), ("localhost", 8080));
    LOOKUPS.fetch_add(1, Ordering::SeqCst);
    let waiting = LOOKUP_GATE.lock().unwrap().take();
    if let Some(waiting) = waiting {
        waiting.recv_timeout(LONG).map_err(io::Error::other)?;
    }
    let port = LOOKED_UP_PORT.load(Ordering::SeqCst);
    Ok(vec![SocketAddr::from(([127, 0, 0, 1], port))])
}

#[test]
fn a_slow_lookup_is_not_waited_for_past_the_deadline_and_is_not_begun_twice() {
    let server = answering(with_length(GOOD));
    LOOKED_UP_PORT.store(server.port, Ordering::SeqCst);
    let mut jev = jev("http://localhost:8080/api/alpha/decisions", "lookup");
    jev.lookup.resolve = slow_lookup;
    // Keep the lookup pending through both timeout checks instead of racing
    // a fixed sleep against host scheduling.
    // Unwinding drops the only sender, disconnecting the lookup's bounded wait.
    let (release_lookup, waiting) = mpsc::channel();
    *LOOKUP_GATE.lock().unwrap() = Some(waiting);
    // Two askings give up while the one lookup is still out.
    for _ in 0..2 {
        let deadline = Duration::from_millis(120);
        let started = Instant::now();
        let failed = jev.ask(STATE, &questions(), deadline).unwrap_err();
        let took = started.elapsed();
        assert_eq!(failed, Failed::TimedOut);
        assert!(took >= deadline && took < deadline + MARGIN, "{took:?}");
    }
    assert_eq!(LOOKUPS.load(Ordering::SeqCst), 1);
    assert_eq!(server.connections.load(Ordering::SeqCst), 0);
    release_lookup.send(()).expect("release the pending lookup");
    drop(release_lookup);
    // The third uses the released lookup, whose result is then cached.
    good(&jev.ask(STATE, &questions(), LONG).unwrap());
    assert!(server.request().contains("\r\nHost: localhost:8080\r\n"));
    good(&jev.ask(STATE, &questions(), LONG).unwrap());
    assert_eq!(LOOKUPS.load(Ordering::SeqCst), 1);
    // An address that stops working is forgotten, so that the name is
    // looked up again: the service may have moved.
    drop(server);
    let failed = jev.ask(STATE, &questions(), LONG).unwrap_err();
    assert_eq!(failed.kind(), "unreachable");
    let server = answering(with_length(GOOD));
    LOOKED_UP_PORT.store(server.port, Ordering::SeqCst);
    good(&jev.ask(STATE, &questions(), LONG).unwrap());
    assert_eq!(LOOKUPS.load(Ordering::SeqCst), 2);
}

#[test]
fn a_name_that_is_not_found_is_unreachable() {
    let mut jev = jev("http://localhost/", "unfound");
    jev.lookup.resolve = |_, _| Err(io::ErrorKind::NotFound.into());
    let looking = |kind| Failed::Unreachable {
        during: "looking up the name",
        kind,
    };
    let failed = jev.ask(STATE, &questions(), LONG).unwrap_err();
    assert_eq!(failed, looking(io::ErrorKind::NotFound));
    jev.lookup.resolve = |_, _| Ok(Vec::new());
    let failed = jev.ask(STATE, &questions(), LONG).unwrap_err();
    assert_eq!(failed, looking(io::ErrorKind::NotFound));
}

#[test]
fn plain_http_is_never_sent_to_another_machine_whatever_a_name_is_looked_up_to() {
    // `localhost` looked up to an address that is not this machine's: one
    // set aside for examples, which nothing answers on.
    let mut jev = jev("http://localhost:8080/", "elsewhere");
    jev.lookup.resolve = |_, port| Ok(vec![SocketAddr::from(([192, 0, 2, 1], port))]);
    let started = Instant::now();
    let failed = jev.ask(STATE, &questions(), LONG).unwrap_err();
    // Nothing was tried: it is told at once.
    assert!(started.elapsed() < Duration::from_millis(500));
    assert_eq!(
        failed,
        Failed::Unreachable {
            during: "connecting",
            kind: io::ErrorKind::AddrNotAvailable,
        }
    );
    // The same name over a secure connection may be anywhere.
    assert!(Endpoint::parse("https://decisions.example/").is_ok());
}

/// A server that speaks TLS with the test certificate.
fn serve_securely(script: impl Fn(&mut dyn Wire) + Send + 'static) -> Server {
    serve_securely_as(script, false)
}

fn serve_securely_as(script: impl Fn(&mut dyn Wire) + Send + 'static, keep: bool) -> Server {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(certificates::bytes(
        certificates::SERVER_KEY,
    )));
    let chain = vec![CertificateDer::from(certificates::bytes(
        certificates::SERVER,
    ))];
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .unwrap();
    let config = Arc::new(config);
    serve_as(
        move |socket| {
            let session = rustls::ServerConnection::new(Arc::clone(&config)).unwrap();
            Box::new(rustls::StreamOwned::new(session, socket))
        },
        script,
        keep,
    )
}

/// Jev that trusts the test's own authority and no other.
fn jev_trusting_the_test(url: &str, name: &str) -> Jev {
    let path = key_file(name, KEY);
    let jev = Jev::trusting(url, "typesafe/jev-1.13", &path, || {
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(certificates::bytes(certificates::AUTHORITY).into())
            .unwrap();
        roots
    });
    std::fs::remove_file(path).unwrap();
    jev.unwrap()
}

#[test]
fn over_a_secure_connection_the_same_request_gets_the_same_answer() {
    let server = serve_securely(|stream| {
        let _ = stream.write_all(&with_length(GOOD));
    });
    // By address, which the certificate names.
    let url = format!("https://127.0.0.1:{}/api/alpha/decisions", server.port);
    let mut jev = jev_trusting_the_test(&url, "secure");
    good(&jev.ask(STATE, &questions(), LONG).unwrap());
    let request = server.request();
    assert!(request.starts_with("POST /api/alpha/decisions HTTP/1.1\r\n"));
    assert!(request.contains(&format!("\r\nAuthorization: Bearer {KEY}\r\n")));
    assert!(
        request
            .ends_with("\"criteria\":{\"option_b\":\"the second\",\"option_a\":\"the first\"}}}}")
    );
    // By name, looked up, which the certificate names too. In chunks.
    let server = serve_securely(|stream| {
        let _ = stream.write_all(
            format!(
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{GOOD}\r\n0\r\n\r\n",
                GOOD.len()
            )
            .as_bytes(),
        );
    });
    let url = format!("https://localhost:{}/api/alpha/decisions", server.port);
    let mut jev = jev_trusting_the_test(&url, "secure-name");
    jev.lookup.resolve = |_, port| Ok(vec![SocketAddr::from(([127, 0, 0, 1], port))]);
    good(&jev.ask(STATE, &questions(), LONG).unwrap());
    assert!(
        server
            .request()
            .contains(&format!("\r\nHost: localhost:{}\r\n", server.port))
    );
}

#[test]
fn a_secure_connection_is_kept_and_made_ahead_like_any_other() {
    let server = serve_securely_as(
        |stream| {
            let _ = stream.write_all(&in_chunks(GOOD, "\r\n"));
        },
        true,
    );
    let url = format!("https://127.0.0.1:{}/api/alpha/decisions", server.port);
    let mut jev = jev_trusting_the_test(&url, "secure-kept");
    jev.ready(LONG);
    assert!(jev.kept.is_some());
    for _ in 0..3 {
        good(&jev.ask(STATE, &questions(), LONG).unwrap());
        server.request();
    }
    assert_eq!(server.connections.load(Ordering::SeqCst), 1);
    // The service closes it without a word: the next asking makes another.
    let server = serve_securely(|stream| {
        let _ = stream.write_all(&kept_with_length(GOOD));
    });
    let url = format!("https://127.0.0.1:{}/api/alpha/decisions", server.port);
    let mut jev = jev_trusting_the_test(&url, "secure-closed");
    for asked in 1..=3 {
        good(&jev.ask(STATE, &questions(), LONG).unwrap());
        assert_eq!(server.connections.load(Ordering::SeqCst), asked);
        thread::sleep(Duration::from_millis(30));
    }
}

#[test]
fn a_server_whose_certificate_is_not_trusted_is_never_sent_the_key() {
    let server = serve_securely(|stream| {
        let _ = stream.write_all(&with_length(GOOD));
    });
    let insecure = Failed::Unreachable {
        during: "making the connection secure",
        kind: io::ErrorKind::InvalidData,
    };
    // The roots the program is built with do not know the test's authority.
    let url = format!("https://127.0.0.1:{}/api/alpha/decisions", server.port);
    let failed = jev(&url, "untrusted")
        .ask(STATE, &questions(), LONG)
        .unwrap_err();
    assert_eq!(failed, insecure);
    says_nothing_secret(&failed);
    // The connection ended before any request was on it.
    assert_eq!(server.requests.recv_timeout(LONG).unwrap(), None);
    // A certificate that is trusted, for another name than the one asked.
    let url = format!("https://decisions.test:{}/", server.port);
    let mut jev = jev_trusting_the_test(&url, "misnamed");
    jev.lookup.resolve = |_, port| Ok(vec![SocketAddr::from(([127, 0, 0, 1], port))]);
    assert_eq!(jev.ask(STATE, &questions(), LONG).unwrap_err(), insecure);
    assert_eq!(server.requests.recv_timeout(LONG).unwrap(), None);
    // A server that does not speak TLS at all, and says so in plain HTTP
    // as soon as it is connected to.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "https://127.0.0.1:{}/",
        listener.local_addr().unwrap().port()
    );
    let plain = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        // Until the other end has had enough of it.
        let _ = socket.read(&mut [0; 1024]);
    });
    let failed = jev_trusting_the_test(&url, "plain")
        .ask(STATE, &questions(), LONG)
        .unwrap_err();
    assert_eq!(failed, insecure);
    says_nothing_secret(&failed);
    plain.join().unwrap();
}

#[test]
fn a_handshake_that_never_finishes_ends_at_the_deadline() {
    // The connection is taken and not a byte is said on it.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "https://127.0.0.1:{}/",
        listener.local_addr().unwrap().port()
    );
    let deadline = Duration::from_millis(300);
    let started = Instant::now();
    let failed = jev_trusting_the_test(&url, "handshake")
        .ask(STATE, &questions(), deadline)
        .unwrap_err();
    let took = started.elapsed();
    assert_eq!(failed, Failed::TimedOut);
    assert!(took >= deadline && took < deadline + MARGIN, "{took:?}");
    drop(listener);
}

/// #871: a picture goes as one of TypeSafe's `assets`, in standard base64
/// under the key the state names it by, with the header TypeSafe asks for;
/// a question without one is sent as it always was (the test above).
#[test]
fn a_picture_is_sent_as_an_asset_under_its_key_with_the_images_header() {
    let server = answering(with_length(GOOD));
    let mut jev = jev(&server.url(), "picture");
    let picture = Picture {
        key: "lamp-sight-7f3a".into(),
        jpeg: vec![0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, b'J', b'F', b'I', b'F'],
    };
    assert_eq!(
        format!("{picture:?}"),
        "Picture { key: \"lamp-sight-7f3a\", bytes: 10 }"
    );
    let state = "{\"picture\":\"lamp-sight-7f3a\"}";
    good(
        &jev.ask_with_pictures(
            state,
            &questions(),
            LONG,
            &|| Ok(vec![picture.clone()]),
            &|_| {},
        )
        .unwrap(),
    );
    let request = server.request();
    let (head, body) = request.split_once("\r\n\r\n").unwrap();
    assert!(
        head.split("\r\n").any(|line| line == IMAGES_HEADER),
        "{head}"
    );
    let sent: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(sent["state"]["picture"], "lamp-sight-7f3a");
    assert_eq!(
        sent["assets"],
        serde_json::json!({"lamp-sight-7f3a": {"type": "image", "format": "jpg", "data_base64": "/9j/4AAQSkZJRg=="}})
    );
    assert!(body.starts_with("{\"model\":\"typesafe/jev-1.13\",\"state\":{\"picture\""));
}

/// Each request with pictures is told of as it went (the reviewer on #873):
/// on a kept connection the service had closed, written whole and not
/// answered, so not known to have arrived; then on a new one, answered.
#[test]
fn what_became_of_each_request_with_pictures_is_told() {
    let server = answering(kept_with_length(GOOD));
    let mut jev = jev(&server.url(), "told");
    let picture = Picture {
        key: "lamp-sight-told".into(),
        jpeg: vec![0xff, 0xd8, 0xff, 0xd9],
    };
    let told = std::cell::RefCell::new(Vec::new());
    let mut ask = || {
        jev.ask_with_pictures(
            "{\"picture\":\"lamp-sight-told\"}",
            &questions(),
            LONG,
            &|| Ok(vec![picture.clone()]),
            &|delivery| told.borrow_mut().push(delivery),
        )
    };
    good(&ask().unwrap());
    assert_eq!(*told.borrow(), [Delivery::Answered]);
    // Time for the closing to arrive, as between two things said.
    thread::sleep(Duration::from_millis(30));
    good(&ask().unwrap());
    assert_eq!(
        *told.borrow(),
        [Delivery::Answered, Delivery::Uncertain, Delivery::Answered]
    );
    assert!(Delivery::Uncertain.went() && Delivery::Partly(1).went());
    assert!(!Delivery::Partly(0).went());
}

/// A frame as the eyes keep one: 320 by 240, of one shade.
fn eyes_frame(shade: u8) -> Vec<u8> {
    let mut out = Vec::new();
    jpeg_encoder::Encoder::new(&mut out, 80)
        .encode(
            &vec![shade; 320 * 240 * 3],
            320,
            240,
            jpeg_encoder::ColorType::Rgb,
        )
        .unwrap();
    out
}

/// Written as the eyes write it: beside, then renamed over it.
fn keep_frame(path: &Path, bytes: &[u8]) {
    let beside = path.with_extension("jpg.new");
    std::fs::write(&beside, bytes).unwrap();
    std::fs::rename(&beside, path).unwrap();
}

static SCENE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
static SCENE_PORT: AtomicU16 = AtomicU16::new(0);
static SCENE_FILE: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// A slow lookup, during which the eyes' frame changes as [`SCENE`] says:
/// 1, it is removed (the camera closed, as `Kept::forget` does); 2, removed,
/// and 1.1 s later another frame kept (closed and opened again); 3, another
/// frame kept over it, with no gap.
fn scene_lookup(_: &str, _: u16) -> io::Result<Vec<SocketAddr>> {
    let path = SCENE_FILE.lock().unwrap().clone().unwrap();
    thread::sleep(Duration::from_millis(100));
    match SCENE.load(Ordering::SeqCst) {
        1 => std::fs::remove_file(&path).unwrap(),
        2 => {
            std::fs::remove_file(&path).unwrap();
            thread::sleep(Duration::from_millis(1_100));
            keep_frame(&path, &eyes_frame(200));
        }
        3 => keep_frame(&path, &eyes_frame(200)),
        _ => {}
    }
    let port = SCENE_PORT.load(Ordering::SeqCst);
    Ok(vec![SocketAddr::from(([127, 0, 0, 1], port))])
}

/// The reviewer on #873: what is sent is the eyes' frame at the moment the
/// request is written, after any slow looking up and connecting, never a
/// frame taken before. Removed meanwhile, nothing is sent; closed and opened
/// again, or replaced, the new frame is sent, with its own age, and the old
/// one never.
#[test]
fn a_picture_is_the_eyes_frame_when_it_is_written_never_one_taken_before() {
    use base64::Engine as _;
    let dir = std::env::temp_dir().join(format!("policy-scenes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sight.jpg");
    *SCENE_FILE.lock().unwrap() = Some(path.clone());
    let encoded = |shade: u8| {
        let (jpeg, _) = crate::pictures::for_jev(&eyes_frame(shade)).unwrap();
        base64::engine::general_purpose::STANDARD.encode(jpeg)
    };
    for scene in [1, 2, 3] {
        keep_frame(&path, &eyes_frame(40));
        let server = answering(with_length(GOOD));
        SCENE_PORT.store(server.port, Ordering::SeqCst);
        SCENE.store(scene, Ordering::SeqCst);
        let url = format!("http://localhost:{}/api/alpha/decisions", server.port);
        let mut jev = jev(&url, "scene");
        jev.lookup.resolve = scene_lookup;
        let taken = std::cell::RefCell::new(None);
        let told = std::cell::RefCell::new(Vec::new());
        let asked = jev.ask_with_pictures(
            "{\"picture\":\"lamp-sight-picture-871\"}",
            &questions(),
            LONG,
            &|| {
                let now = crate::pictures::take_now(&path).map_err(|_| Failed::Withdrawn)?;
                let picture = Picture {
                    key: "lamp-sight-picture-871".into(),
                    jpeg: now.jpeg.clone(),
                };
                *taken.borrow_mut() = Some(now);
                Ok(vec![picture])
            },
            &|delivery| told.borrow_mut().push(delivery),
        );
        if scene == 1 {
            assert_eq!(asked, Err(Failed::Withdrawn));
            assert!(taken.borrow().is_none());
            // Nothing was written, so nothing is told of.
            assert!(told.borrow().is_empty());
            // Nothing whole reached the service.
            assert!(!matches!(
                server.requests.recv_timeout(Duration::from_millis(500)),
                Ok(Some(_))
            ));
            continue;
        }
        good(&asked.unwrap());
        assert_eq!(*told.borrow(), [Delivery::Answered], "scene {scene}");
        let request = server.request();
        let (_, body) = request.split_once("\r\n\r\n").unwrap();
        let sent: serde_json::Value = serde_json::from_str(body).unwrap();
        let picture = sent["assets"]["lamp-sight-picture-871"]["data_base64"]
            .as_str()
            .unwrap();
        assert_eq!(picture, encoded(200), "scene {scene}");
        assert_ne!(picture, encoded(40), "scene {scene}");
        let age = taken.borrow().as_ref().unwrap().age;
        assert!(age < Duration::from_millis(500), "scene {scene}: {age:?}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
