//! `LAMP_SECOND_CHAT_SECOND_TRY`: the engine and the real chat client against a
//! stand-in server on a local socket, so the connections, the requests and the
//! tries dropped are the ones a lamp would make. Real time, with the second try
//! brought forward to [`AFTER`]; the product's own times are read on the
//! paused clock at the end.
use super::*;
use crate::second_provider::services::tests::config_at;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// When the second try goes, in the tests on real sockets.
const AFTER: Duration = Duration::from_millis(600);

/// What the server does with a request, by the order they come.
#[derive(Clone)]
enum Says {
    /// Nothing, until the lamp lets go.
    Nothing,
    /// The response's head, then nothing.
    HeadOnly,
    /// The response's head after the first time, these words after the second
    /// more: a live connection and a slow answer.
    HeadThen(Duration, Duration, &'static str),
    /// These words, after this long.
    Words(Duration, &'static str),
    /// A call of this function, at once.
    Calls(&'static str),
    /// This status, after this long.
    Refuses(Duration, u16),
    /// Closes the connection, after this long.
    Cuts(Duration),
}

struct Seen {
    connection: usize,
    body: String,
}

struct Server {
    url: String,
    seen: Arc<Mutex<Vec<Seen>>>,
    connections: Arc<AtomicUsize>,
    /// Requests the lamp let go of before they were answered.
    let_go: Arc<AtomicUsize>,
}

impl Server {
    /// A server saying `plan`, taking at most `most` connections, and then
    /// refusing any more as a port nobody listens on does.
    async fn saying(plan: Vec<Says>, most: Option<usize>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
        let connections: Arc<AtomicUsize> = Arc::default();
        let let_go: Arc<AtomicUsize> = Arc::default();
        let plan = Arc::new(plan);
        let (saw, count, gone) = (seen.clone(), connections.clone(), let_go.clone());
        tokio::spawn(async move {
            let mut taken = 0;
            while most.is_none_or(|most| taken < most) {
                let Ok((socket, _)) = listener.accept().await else {
                    return;
                };
                taken += 1;
                let connection = count.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(serving(
                    socket,
                    connection,
                    plan.clone(),
                    saw.clone(),
                    gone.clone(),
                ));
            }
        });
        Self {
            url,
            seen,
            connections,
            let_go,
        }
    }

    fn requests(&self) -> usize {
        self.seen.lock().unwrap().len()
    }

    fn opened(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    /// The last two requests' bodies are the same bytes, on two connections.
    fn last_two_are_the_same_on_two_connections(&self) {
        let seen = self.seen.lock().unwrap();
        let [.., one, two] = seen.as_slice() else {
            panic!("fewer than two requests");
        };
        assert_eq!(one.body, two.body);
        assert_ne!(one.connection, two.connection);
    }

    async fn let_go_of(&self, count: usize) {
        for _ in 0..200 {
            if self.let_go.load(Ordering::SeqCst) >= count {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!(
            "the lamp let go of {} requests, not {count}",
            self.let_go.load(Ordering::SeqCst)
        );
    }
}

/// One request's head and body, or `None` once the lamp has closed.
async fn request(socket: &mut tokio::net::TcpStream) -> Option<String> {
    let mut read = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let n = socket.read(&mut buffer).await.ok()?;
        if n == 0 {
            return None;
        }
        read.extend_from_slice(&buffer[..n]);
        let text = String::from_utf8_lossy(&read);
        let Some(ends) = text.find("\r\n\r\n") else {
            continue;
        };
        let length: usize = text[..ends]
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .and_then(|n| n.trim().parse().ok())
            })
            .unwrap_or(0);
        if read.len() >= ends + 4 + length {
            return Some(text[ends + 4..].to_owned());
        }
    }
}

/// `body` as a kept-alive response of server-sent events.
fn events(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    )
}

fn words_events(words: &str) -> String {
    format!(
        "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{words}\"}},\"finish_reason\":\"stop\"}}]}}\n\ndata: [DONE]\n\n"
    )
}

fn words_answer(words: &str) -> String {
    events(&words_events(words))
}

const HEAD: &str =
    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n";

fn call_answer(name: &str) -> String {
    events(&format!(
        "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":\"c1\",\"function\":{{\"name\":\"{name}\",\"arguments\":\"{{}}\"}}}}]}},\"finish_reason\":\"tool_calls\"}}]}}\n\ndata: [DONE]\n\n"
    ))
}

async fn serving(
    mut socket: tokio::net::TcpStream,
    connection: usize,
    plan: Arc<Vec<Says>>,
    seen: Arc<Mutex<Vec<Seen>>>,
    let_go: Arc<AtomicUsize>,
) {
    while let Some(body) = request(&mut socket).await {
        let index = {
            let mut seen = seen.lock().unwrap();
            seen.push(Seen { connection, body });
            seen.len() - 1
        };
        let says = plan.get(index).cloned().unwrap_or(Says::Nothing);
        if let Says::HeadThen(head_after, words_after, words) = says {
            tokio::time::sleep(head_after).await;
            if socket.write_all(HEAD.as_bytes()).await.is_err() {
                return;
            }
            tokio::time::sleep(words_after).await;
            let data = words_events(words);
            let chunk = format!("{:x}\r\n{data}\r\n0\r\n\r\n", data.len());
            if socket.write_all(chunk.as_bytes()).await.is_err() {
                return;
            }
            continue;
        }
        let (wait, answer) = match says {
            Says::HeadThen(..) => unreachable!("answered above"),
            Says::Nothing => (None, None),
            Says::HeadOnly => (None, Some(HEAD.to_owned())),
            Says::Words(after, words) => (Some(after), Some(words_answer(words))),
            Says::Calls(name) => (Some(Duration::ZERO), Some(call_answer(name))),
            Says::Refuses(after, status) => (
                Some(after),
                Some(format!(
                    "HTTP/1.1 {status} No\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                )),
            ),
            Says::Cuts(after) => (Some(after), None),
        };
        let mut byte = [0; 1];
        match (wait, answer) {
            (Some(after), Some(answer)) => {
                tokio::select! {
                    _ = socket.read(&mut byte) => {
                        let_go.fetch_add(1, Ordering::SeqCst);
                        return;
                    }
                    () = tokio::time::sleep(after) => {}
                }
                if socket.write_all(answer.as_bytes()).await.is_err() {
                    return;
                }
            }
            (Some(after), None) => {
                tokio::time::sleep(after).await;
                return;
            }
            (None, answer) => {
                if let Some(head) = answer {
                    let _ = socket.write_all(head.as_bytes()).await;
                }
                let _ = socket.read(&mut byte).await;
                let_go.fetch_add(1, Ordering::SeqCst);
                return;
            }
        }
    }
}

/// The engine's services: the real chat client, and a script's ears and voice.
struct OnTheServer {
    net: Net,
    script: Script,
    spoken: Arc<Mutex<Vec<String>>>,
}

impl Services for OnTheServer {
    fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
        self.script.listen()
    }
    fn chat(
        &self,
        messages: Vec<Message>,
    ) -> futures_util::stream::BoxStream<'static, net::Result<ChatEvent>> {
        self.net.chat(messages)
    }
    fn chat_only(
        &self,
        messages: Vec<Message>,
        only: &'static [&'static str],
    ) -> futures_util::stream::BoxStream<'static, net::Result<ChatEvent>> {
        self.net.chat_only(messages, only)
    }
    fn chat_without(
        &self,
        messages: Vec<Message>,
        without: &'static [&'static str],
    ) -> futures_util::stream::BoxStream<'static, net::Result<ChatEvent>> {
        self.net.chat_without(messages, without)
    }
    fn chat_watched(
        &self,
        messages: Vec<Message>,
        told: ChatTold,
        watch: net::Watch,
    ) -> futures_util::stream::BoxStream<'static, net::Result<ChatEvent>> {
        self.net.chat_watched(messages, told, watch)
    }
    fn chat_second_try(
        &self,
        messages: Vec<Message>,
        told: ChatTold,
        watch: net::Watch,
    ) -> Option<futures_util::stream::BoxStream<'static, net::Result<ChatEvent>>> {
        self.net.chat_second_try(messages, told, watch)
    }
    fn speak(
        &self,
        text: String,
    ) -> futures_util::stream::BoxStream<'static, net::Result<Vec<i16>>> {
        self.spoken.lock().unwrap().push(text.clone());
        self.script.speak(text)
    }
}

struct Lamp {
    engine: Engine,
    spoken: Arc<Mutex<Vec<String>>>,
    journal: Arc<Mutex<Vec<String>>>,
}

impl Lamp {
    /// A lamp on `server`, its second try `after` the words, if on.
    fn on(server: &Server, name: &str, after: Option<Duration>) -> Self {
        Self::set(server, name, after, |engine| engine)
    }

    /// [`Self::on`], the engine set further by `set`.
    fn set(
        server: &Server,
        name: &str,
        after: Option<Duration>,
        set: impl FnOnce(Engine) -> Engine,
    ) -> Self {
        Self::built(server, name, after, Vec::new(), |net| net, set)
    }

    /// [`Self::on`], with a chat begun on the line's `early` words.
    fn set_hearing(
        server: &Server,
        name: &str,
        after: Option<Duration>,
        early: Vec<net::Early>,
    ) -> Self {
        Self::built(
            server,
            name,
            after,
            early,
            |net| net,
            |engine| engine.preparing(true),
        )
    }

    fn built(
        server: &Server,
        name: &str,
        after: Option<Duration>,
        early: Vec<net::Early>,
        set_net: impl FnOnce(Net) -> Net,
        set: impl FnOnce(Engine) -> Engine,
    ) -> Self {
        let net = set_net(
            Net::new(
                config_at(
                    &format!("second-try-{name}"),
                    "k-second-try",
                    &server.url,
                    &server.url,
                ),
                Arc::new(Vec::new),
            )
            .unwrap(),
        );
        let script = Script {
            heard: Mutex::new(
                std::iter::repeat_with(|| Ok("What time is it?".to_owned()))
                    .take(3)
                    .collect(),
            ),
            early: Mutex::new([early].into()),
            ..Script::default()
        };
        let spoken: Arc<Mutex<Vec<String>>> = Arc::default();
        let journal: Arc<Mutex<Vec<String>>> = Arc::default();
        let tools: ToolRunner = Arc::new(|_, _, _, _| Box::pin(async { json!({"ok": true}) }));
        let engine = Engine::with_clock(
            Arc::new(OnTheServer {
                net,
                script,
                spoken: spoken.clone(),
            }),
            Arc::new(|| "SYSTEM".to_owned().into()),
            tools,
            Arc::new(Instant::now),
        )
        .announcing(false)
        .trying_the_chat_twice(after)
        .keeping_second_try_journal(journal.clone());
        Self {
            engine: set(engine),
            spoken,
            journal,
        }
    }

    /// A spoken turn: how long it took from its words, and its labelled outputs.
    async fn says(&mut self, request: u64) -> (Duration, Vec<String>) {
        spoken_turn_as(&mut self.engine, request);
        self.ends().await
    }

    async fn ends(&mut self) -> (Duration, Vec<String>) {
        let begun = tokio::time::Instant::now();
        let heard = tokio::time::timeout(Duration::from_secs(20), sent(&mut self.engine))
            .await
            .expect("the turn ends within its bound");
        (begun.elapsed(), heard)
    }

    fn spoken(&self) -> Vec<String> {
        self.spoken.lock().unwrap().clone()
    }

    /// What the voice was asked to say, less the down lines made ready ahead.
    fn answered(&self) -> Vec<String> {
        self.spoken()
            .into_iter()
            .filter(|words| !words.contains("slow") && !words.contains("can't answer"))
            .collect()
    }

    fn journal(&self) -> Vec<String> {
        self.journal.lock().unwrap().clone()
    }

    /// The journal has a line with each of `parts`.
    fn told(&self, parts: &[&str]) {
        let journal = self.journal();
        assert!(
            journal
                .iter()
                .any(|line| parts.iter().all(|part| line.contains(part))),
            "no line with {parts:?} in {journal:#?}"
        );
    }
}

/// About `expected`, allowing for a loaded machine above it, never below.
fn about(actual: Duration, expected: Duration) {
    assert!(
        actual >= expected - Duration::from_millis(50)
            && actual < expected + Duration::from_millis(1_500),
        "{actual:?} against {expected:?}"
    );
}

/// Off: one request, silent to the limit as before, and no second-try line.
#[tokio::test]
async fn off_no_second_try_goes() {
    let server = Server::saying(vec![Says::Nothing], None).await;
    let mut lamp = Lamp::on(&server, "off", None);
    let (took, _) = lamp.says(1).await;
    about(took, BEGIN_WITHIN);
    assert_eq!(server.requests(), 1);
    assert_eq!(server.opened(), 1);
    assert!(lamp.journal().is_empty(), "{:?}", lamp.journal());
    assert!(lamp.spoken().is_empty(), "{:?}", lamp.spoken());
}

/// A kept connection gone silent: the same request goes on a new one, which
/// answers; the first is let go, and the words are said once. The case the
/// switch is for: the journal reads the kept one as dead.
#[tokio::test]
async fn the_first_silent_on_a_kept_connection_and_the_second_answers() {
    let server = Server::saying(
        vec![
            Says::Words(Duration::ZERO, "Good morning."),
            Says::Nothing,
            Says::Words(Duration::ZERO, "It is noon."),
        ],
        None,
    )
    .await;
    let mut lamp = Lamp::on(&server, "kept", Some(AFTER));
    lamp.says(1).await;
    assert_eq!(server.opened(), 1);
    assert!(lamp.journal().is_empty(), "{:?}", lamp.journal());

    let (took, _) = lamp.says(2).await;
    assert!(took < BEGIN_WITHIN, "{took:?}");
    assert_eq!(server.requests(), 3);
    {
        let seen = server.seen.lock().unwrap();
        assert_eq!(seen[1].connection, seen[0].connection, "the kept one");
        assert_ne!(seen[2].connection, seen[1].connection, "a new one");
    }
    server.last_two_are_the_same_on_two_connections();
    server.let_go_of(1).await;
    assert_eq!(lamp.spoken(), ["Good morning.", "It is noon."]);
    lamp.told(&[
        "request 2 second try goes ",
        "ms after the words, on a new connection; the first stays open, having reached sent, on a kept connection (LAMP_SECOND_CHAT_SECOND_TRY)",
    ]);
    lamp.told(&["request 2 second try: its new connection opened in "]);
    lamp.told(&[
        "request 2 second try: the second answered, its first event ",
        "the first was dropped, having reached sent, on a kept connection",
    ]);
}

/// The first answers before the second try's time: none goes, and no line.
#[tokio::test]
async fn the_first_answers_in_time_and_no_second_try_goes() {
    let server = Server::saying(
        vec![Says::Words(Duration::from_millis(50), "It is noon.")],
        None,
    )
    .await;
    let mut lamp = Lamp::on(&server, "in-time", Some(AFTER));
    lamp.says(1).await;
    tokio::time::sleep(AFTER * 2).await;
    assert_eq!(server.requests(), 1);
    assert_eq!(lamp.spoken(), ["It is noon."]);
    assert!(lamp.journal().is_empty(), "{:?}", lamp.journal());
}

/// Its response's head came early and its first word is slow: the connection is
/// plainly alive, so no second request goes, and the first answers. The case
/// that would count a healthy slow answer as a second try.
#[tokio::test]
async fn a_head_in_time_and_a_slow_first_word_send_no_second_try() {
    let server = Server::saying(
        vec![Says::HeadThen(AFTER / 2, AFTER * 2, "Slow, but alive.")],
        None,
    )
    .await;
    let mut lamp = Lamp::on(&server, "head-then", Some(AFTER));
    lamp.says(1).await;
    assert_eq!(server.requests(), 1);
    assert_eq!(server.opened(), 1);
    assert_eq!(lamp.spoken(), ["Slow, but alive."]);
    lamp.told(&[
        "request 1 second try: none goes, ",
        "ms after the words: the first's answer has begun",
    ]);
    assert_eq!(lamp.journal().len(), 1, "{:?}", lamp.journal());
}

/// A head and then nothing: still no second try, and the slow line comes at its
/// own time.
#[tokio::test]
async fn a_head_and_nothing_after_it_sends_no_second_try() {
    let server = Server::saying(vec![Says::HeadOnly], None).await;
    let mut lamp = Lamp::set(&server, "head-only", Some(AFTER), |engine| {
        engine.saying_when_down(true)
    });
    let (took, heard) = lamp.says(1).await;
    about(took, BEGIN_WITHIN);
    assert_eq!(server.requests(), 1);
    assert!(
        heard.contains(&format!("audio:{}", DOWN_LINE.len())),
        "{heard:?}"
    );
    lamp.told(&["second try: none goes, ", "the first's answer has begun"]);
}

/// Both refused the person's key: each still goes to the relay, and the refusal
/// is told once, as one turn's. The second is refused first and waits there; the
/// first, refused after it, is the one answered.
#[tokio::test]
async fn both_tries_refused_are_told_once() {
    let server = Server::saying(
        vec![
            Says::Refuses(AFTER * 3 / 2, 401),
            Says::Refuses(Duration::ZERO, 401),
            Says::Nothing,
            Says::Words(Duration::ZERO, "From the relay."),
        ],
        None,
    )
    .await;
    let told = Arc::new(Mutex::new(Vec::new()));
    let telling = told.clone();
    let relay = server.url.clone();
    let mut lamp = Lamp::built(
        &server,
        "refused-once",
        Some(AFTER),
        Vec::new(),
        move |net| {
            net.falling_back_to_the_relay(
                &relay,
                Arc::new(move |status| telling.lock().unwrap().push(status)),
            )
            .unwrap()
        },
        |engine| engine,
    );
    lamp.says(1).await;
    assert_eq!(server.requests(), 4);
    assert_eq!(lamp.spoken(), ["From the relay."]);
    assert_eq!(*told.lock().unwrap(), [401]);
}

/// A round's two tries tell a fall and a refusal once between them; another
/// round's try has its own to tell.
#[test]
fn a_rounds_tries_tell_a_fall_and_a_refusal_once() {
    let first = net::Watch::default();
    let second = net::Watch::second_to(&first.reach(), Arc::new(|_| {}));
    assert!(second.first_to_tell_a_fall());
    assert!(!first.first_to_tell_a_fall());
    assert!(!second.first_to_tell_a_fall());
    assert!(first.first_to_tell_a_refusal());
    assert!(!second.first_to_tell_a_refusal());
    assert!(net::Watch::default().first_to_tell_a_fall());
    assert!(net::Watch::default().first_to_tell_a_refusal());
}

/// The first begins after the second went: it answers, the second is dropped,
/// and none of the second's words are ever said.
#[tokio::test]
async fn the_first_begins_after_the_second_went_and_wins() {
    let server = Server::saying(
        vec![
            Says::Words(AFTER * 2, "The first answer."),
            Says::Words(AFTER * 3, "SECOND"),
        ],
        None,
    )
    .await;
    let mut lamp = Lamp::on(&server, "first-wins", Some(AFTER));
    lamp.says(1).await;
    server.let_go_of(1).await;
    tokio::time::sleep(AFTER * 2).await;
    assert_eq!(server.requests(), 2);
    assert_eq!(lamp.spoken(), ["The first answer."]);
    lamp.told(&[
        "second try: the first answered, its first event ",
        "the second was dropped, having reached sent, on a new connection",
    ]);
}

/// The second begins first, and the first would have answered later: only the
/// second's words, once.
#[tokio::test]
async fn the_second_begins_first_and_the_first_is_dropped() {
    let server = Server::saying(
        vec![
            Says::Words(AFTER * 3, "FIRST"),
            Says::Words(AFTER / 4, "The second answer."),
        ],
        None,
    )
    .await;
    let mut lamp = Lamp::on(&server, "second-wins", Some(AFTER));
    lamp.says(1).await;
    server.let_go_of(1).await;
    tokio::time::sleep(AFTER * 3).await;
    assert_eq!(lamp.spoken(), ["The second answer."]);
    lamp.told(&["second try: the second answered, its first event "]);
}

/// Both silent: the slow line at the limit it has without the switch, and the
/// end says what each reached. The third outcome: the new connection opened
/// and stayed silent, so the wait is further along.
#[tokio::test]
async fn both_silent_the_slow_line_comes_at_its_own_time() {
    let server = Server::saying(vec![Says::Nothing, Says::HeadOnly], None).await;
    let mut lamp = Lamp::set(&server, "both", Some(AFTER), |engine| {
        engine.saying_when_down(true)
    });
    let (took, heard) = lamp.says(1).await;
    about(took, BEGIN_WITHIN);
    assert!(
        heard.contains(&format!("audio:{}", DOWN_LINE.len())),
        "{heard:?}"
    );
    assert_eq!(server.requests(), 2);
    server.last_two_are_the_same_on_two_connections();
    server.let_go_of(2).await;
    assert!(!heard.contains(&"said".to_owned()), "{heard:?}");
    lamp.told(&["second try: its new connection opened in "]);
    lamp.told(&[
        "second try: neither answered in ",
        "the first reached sent, on a new connection, the second reached answer begun",
    ]);
}

/// The second can't connect: the first keeps its wait, the slow line comes at
/// its own time, and the journal says the new connection could not open: the
/// lamp's network was down.
#[tokio::test]
async fn the_second_cannot_connect_and_the_first_keeps_waiting() {
    let server = Server::saying(vec![Says::Nothing], Some(1)).await;
    let mut lamp = Lamp::on(&server, "no-connect", Some(AFTER));
    let (took, _) = lamp.says(1).await;
    about(took, BEGIN_WITHIN);
    assert_eq!(server.requests(), 1);
    lamp.told(&["second try: its new connection could not open, after "]);
    lamp.told(&["second try: the second ended ", "the first still waits"]);
    lamp.told(&[
        "second try: neither answered in ",
        "the first reached sent, on a new connection, the second reached nothing, its new connection not open, then failed: ",
    ]);
}

/// The second can't connect and the first answers late: the turn is answered.
#[tokio::test]
async fn the_second_cannot_connect_and_the_first_answers_late() {
    let server = Server::saying(vec![Says::Words(AFTER * 3, "Late, but here.")], Some(1)).await;
    let mut lamp = Lamp::on(&server, "late", Some(AFTER));
    let (took, _) = lamp.says(1).await;
    assert!(took < BEGIN_WITHIN, "{took:?}");
    assert_eq!(lamp.spoken(), ["Late, but here."]);
    lamp.told(&["second try: its new connection could not open, after "]);
    lamp.told(&[
        "second try: the first answered, its first event ",
        "the second was dropped, having reached nothing, its new connection not open, then failed: ",
    ]);
}

/// A refusal on the first while the second waits doesn't end the turn, and no
/// down line comes for it.
#[tokio::test]
async fn a_failure_on_the_first_leaves_the_turn_to_the_second() {
    let server = Server::saying(
        vec![
            Says::Refuses(AFTER * 2, 503),
            Says::Words(AFTER * 3, "The second answer."),
        ],
        None,
    )
    .await;
    let mut lamp = Lamp::set(&server, "first-fails", Some(AFTER), |engine| {
        engine.saying_when_down(true)
    });
    let (_, heard) = lamp.says(1).await;
    assert_eq!(lamp.answered(), ["The second answer."]);
    assert!(heard.contains(&"complete".to_owned()), "{heard:?}");
    assert!(
        !heard.contains(&format!("audio:{}", DOWN_LINE.len())),
        "{heard:?}"
    );
    lamp.told(&[
        "second try: the first ended ",
        "(service answered HTTP 503); the second still waits",
    ]);
    lamp.told(&["second try: the second answered, its first event "]);
}

/// A refusal on the second while the first waits doesn't end the turn either.
#[tokio::test]
async fn a_failure_on_the_second_leaves_the_turn_to_the_first() {
    let server = Server::saying(
        vec![
            Says::Words(AFTER * 2, "The first answer."),
            Says::Refuses(Duration::ZERO, 503),
        ],
        None,
    )
    .await;
    let mut lamp = Lamp::on(&server, "second-fails", Some(AFTER));
    lamp.says(1).await;
    assert_eq!(lamp.spoken(), ["The first answer."]);
    lamp.told(&[
        "second try: the second ended ",
        "(service answered HTTP 503); the first still waits",
    ]);
}

/// Both refused: the turn ends as a refused chat does, on the service's answer.
#[tokio::test]
async fn both_failing_end_the_turn_as_one_failure() {
    let server = Server::saying(
        vec![
            Says::Refuses(AFTER * 2, 429),
            Says::Refuses(Duration::ZERO, 503),
        ],
        None,
    )
    .await;
    let mut lamp = Lamp::on(&server, "both-fail", Some(AFTER));
    let (_, heard) = lamp.says(1).await;
    assert!(lamp.spoken().is_empty(), "{:?}", lamp.spoken());
    assert_eq!(
        heard.last().map(String::as_str),
        Some("failed"),
        "{heard:?}"
    );
    lamp.told(&[
        "second try: neither answered in ",
        "then failed: service answered HTTP 429",
        "then failed: service answered HTTP 503",
    ]);
}

/// Before the second try's time a failure is the turn's as it is today, asked
/// again by the line-lost path: no second try goes.
#[tokio::test]
async fn a_failure_before_the_second_tries_time_is_as_today() {
    let server = Server::saying(
        vec![
            Says::Cuts(Duration::ZERO),
            Says::Words(Duration::ZERO, "It is noon."),
        ],
        None,
    )
    .await;
    let back: LinkBack = Arc::new(|| Box::pin(async { true }));
    let mut lamp = Lamp::set(&server, "resend", Some(AFTER), |engine| {
        engine.resending(back)
    });
    lamp.says(1).await;
    assert_eq!(server.requests(), 2);
    assert_eq!(lamp.spoken(), ["It is noon."]);
    assert!(lamp.journal().is_empty(), "{:?}", lamp.journal());
}

/// Both tries lost on the network: the second try was the turn's one more, so
/// a line lost is not asked a third time, even with the link back.
#[tokio::test]
async fn both_lost_are_not_asked_a_third_time() {
    let server = Server::saying(
        vec![Says::Cuts(AFTER * 2), Says::Cuts(Duration::ZERO)],
        None,
    )
    .await;
    let back: LinkBack = Arc::new(|| Box::pin(async { true }));
    let mut lamp = Lamp::set(&server, "no-third", Some(AFTER), |engine| {
        engine.resending(back)
    });
    lamp.says(1).await;
    assert_eq!(server.requests(), 2);
    lamp.told(&["second try: neither answered in "]);
}

/// A round after a tool ran never goes twice, however slow: what the second
/// try's safety rests on is that nothing has run yet.
#[tokio::test]
async fn a_round_after_a_tool_never_gets_a_second_try() {
    let server = Server::saying(
        vec![
            Says::Calls("check_the_time"),
            Says::Words(AFTER * 3, "It is noon."),
        ],
        None,
    )
    .await;
    let mut lamp = Lamp::on(&server, "tools", Some(AFTER));
    lamp.says(1).await;
    assert_eq!(server.requests(), 2);
    assert_eq!(server.opened(), 1, "round 2 on round 1's connection");
    assert_eq!(lamp.spoken(), ["It is noon."]);
    assert!(lamp.journal().is_empty(), "{:?}", lamp.journal());
}

/// A look's round never goes twice.
#[tokio::test]
async fn a_look_never_gets_a_second_try() {
    let server = Server::saying(vec![Says::Words(AFTER * 3, "A desk.")], None).await;
    let mut lamp = Lamp::on(&server, "look", Some(AFTER));
    lamp.engine.submit(ProviderInput::Written {
        search: false,
        request: 1,
        session: 1,
        privacy_generation: 1,
        text: "Take a look.".into(),
        look: true,
        quoted: false,
    });
    lamp.ends().await;
    assert_eq!(server.requests(), 1);
    assert_eq!(lamp.spoken(), ["A desk."]);
    assert!(lamp.journal().is_empty(), "{:?}", lamp.journal());
}

/// A stop while both are out lets go of both, says no word, and the journal
/// still tells what each reached.
#[tokio::test]
async fn a_stop_while_both_are_out_lets_go_of_both() {
    let server = Server::saying(
        vec![
            Says::Words(AFTER * 3, "FIRST"),
            Says::Words(AFTER * 3, "SECOND"),
        ],
        None,
    )
    .await;
    let mut lamp = Lamp::on(&server, "stop", Some(AFTER));
    spoken_turn(&mut lamp.engine);
    tokio::time::timeout(Duration::from_secs(5), async {
        while server.requests() < 2 {
            tokio::select! {
                _ = lamp.engine.next() => {}
                () = tokio::time::sleep(Duration::from_millis(10)) => {}
            }
        }
    })
    .await
    .expect("the second try went");
    lamp.engine.stop_through(1);
    server.let_go_of(2).await;
    tokio::time::sleep(AFTER * 3).await;
    assert!(lamp.spoken().is_empty(), "{:?}", lamp.spoken());
    lamp.told(&["second try: neither answered in "]);
}

/// A stop before the second try's time, and none goes.
#[tokio::test]
async fn a_stop_before_its_time_and_no_second_try_goes() {
    let server = Server::saying(vec![Says::Nothing], None).await;
    let mut lamp = Lamp::on(&server, "stop-early", Some(AFTER));
    spoken_turn(&mut lamp.engine);
    tokio::time::timeout(Duration::from_secs(5), async {
        while server.requests() < 1 {
            tokio::select! {
                _ = lamp.engine.next() => {}
                () = tokio::time::sleep(Duration::from_millis(10)) => {}
            }
        }
    })
    .await
    .expect("the first went");
    lamp.engine.stop_through(1);
    server.let_go_of(1).await;
    tokio::time::sleep(AFTER * 2).await;
    assert_eq!(server.requests(), 1);
    assert!(lamp.journal().is_empty(), "{:?}", lamp.journal());
}

/// Their scope revoked while the first waits (the camera closed, say): what was
/// read for it is not asked a second time, and the turn's limit stands.
#[tokio::test]
async fn a_scope_revoked_while_the_first_waits_sends_no_second_try() {
    let server = Server::saying(vec![Says::Nothing], None).await;
    let runner: ScopedToolRunner =
        Arc::new(|_, _, _, _, _| Box::pin(async { json!({"ok": true}) }));
    let mut lamp = Lamp::set(&server, "revoked", Some(AFTER), |engine| {
        engine.with_memory_scope(
            lamp_turn_owner::BootId::new([9; 16]).unwrap(),
            Arc::default(),
            runner,
            None,
        )
    });
    spoken_turn(&mut lamp.engine);
    tokio::time::timeout(Duration::from_secs(5), async {
        while server.requests() < 1 {
            tokio::select! {
                _ = lamp.engine.next() => {}
                () = tokio::time::sleep(Duration::from_millis(10)) => {}
            }
        }
    })
    .await
    .expect("the first went");
    lamp.engine
        .turn
        .as_ref()
        .unwrap()
        .memory_scope
        .as_ref()
        .unwrap()
        .revoke();
    tokio::time::sleep(AFTER * 2).await;
    assert_eq!(server.requests(), 1);
    assert!(lamp.journal().is_empty(), "{:?}", lamp.journal());
}

/// A chat begun on the line's early words stands for round 1, and goes twice as
/// any first round does.
#[tokio::test]
async fn a_chat_begun_on_early_words_goes_twice_too() {
    let server = Server::saying(
        vec![Says::Nothing, Says::Words(Duration::ZERO, "It is noon.")],
        None,
    )
    .await;
    let mut lamp = Lamp::set_hearing(
        &server,
        "prepared",
        Some(AFTER),
        vec![net::Early::Words("what time is it".to_owned())],
    );
    lamp.says(1).await;
    assert_eq!(server.requests(), 2);
    server.last_two_are_the_same_on_two_connections();
    assert_eq!(lamp.spoken(), ["It is noon."]);
    lamp.told(&[
        "second try: the second answered, its first event ",
        "the first was dropped, having reached sent, on a new connection",
    ]);
}

/// The chat client's own account of a request: on a new connection, then on
/// the one it kept, and how far each got, for a request the server never answers.
#[tokio::test]
async fn a_request_says_which_connection_it_went_on_and_how_far_it_got() {
    let server = Server::saying(
        vec![
            Says::Nothing,
            Says::Words(Duration::ZERO, "Hello."),
            Says::Nothing,
            Says::HeadOnly,
        ],
        None,
    )
    .await;
    let chat = net::Chat::with(&server.url, "m", "k", Arc::new(Vec::new)).unwrap();
    let asked = [Message::user("hello")];
    let silent = Duration::from_millis(150);

    // No connection kept: a new one opens, and the request is sent on it.
    let watch = net::Watch::default();
    assert_eq!(watch.reach().told(), "nothing");
    let waited = chat.send_watched(&asked, None, &[], Some(&watch));
    assert!(tokio::time::timeout(silent, waited).await.is_err());
    assert_eq!(watch.reach().told(), "sent, on a new connection");

    // That one was let go. Answered on a new one, and the answer's head read.
    let watch = net::Watch::default();
    let mut answer = chat
        .send_watched(&asked, None, &[], Some(&watch))
        .await
        .unwrap();
    assert_eq!(watch.reach().told(), "answer begun");
    while answer.next().await.unwrap().is_some() {}
    drop(answer);

    // That one went back to the pool: the next takes it, and no new one opens.
    let opened = server.opened();
    let watch = net::Watch::default();
    let waited = chat.send_watched(&asked, None, &[], Some(&watch));
    assert!(tokio::time::timeout(silent, waited).await.is_err());
    assert_eq!(watch.reach().told(), "sent, on a kept connection");
    assert_eq!(server.opened(), opened);
}

/// A second try's own client: a new connection every time, its opening told,
/// and nothing kept from it for the chat's next request.
#[tokio::test]
async fn a_second_try_opens_its_own_connection_and_says_how_it_went() {
    let server = Server::saying(vec![Says::Words(Duration::ZERO, "Hello."); 3], None).await;
    let chat = net::Chat::with(&server.url, "m", "k", Arc::new(Vec::new)).unwrap();
    let asked = [Message::user("hello")];
    for tries in 1..=2 {
        let opening: Arc<Mutex<Vec<net::Opening>>> = Arc::default();
        let told = opening.clone();
        let watch = net::Watch::on_a_new_connection(Arc::new(move |went| {
            told.lock().unwrap().push(went);
        }));
        let mut answer = chat
            .send_watched(&asked, None, &[], Some(&watch))
            .await
            .unwrap();
        while answer.next().await.unwrap().is_some() {}
        assert_eq!(server.opened(), tries);
        assert!(matches!(
            opening.lock().unwrap().as_slice(),
            [net::Opening::Opened(_)]
        ));
    }

    // On a port nobody listens on: told it couldn't open, and nothing reached.
    let unused = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", unused.local_addr().unwrap());
    drop(unused);
    let chat = net::Chat::with(&url, "m", "k", Arc::new(Vec::new)).unwrap();
    let opening: Arc<Mutex<Vec<net::Opening>>> = Arc::default();
    let told = opening.clone();
    let watch = net::Watch::on_a_new_connection(Arc::new(move |went| {
        told.lock().unwrap().push(went);
    }));
    let refused = chat.send_watched(&asked, None, &[], Some(&watch)).await;
    assert!(matches!(refused.err(), Some(net::Error::Transport(_))));
    assert!(matches!(
        opening.lock().unwrap().as_slice(),
        [net::Opening::Failed(_)]
    ));
    assert_eq!(watch.reach().told(), "nothing, its new connection not open");
}

/// Scripted services that keep when the second try was asked for, on the test's
/// clock, and may have none to give.
struct Timed {
    script: Arc<Script>,
    second: Mutex<Vec<tokio::time::Instant>>,
    offers: bool,
}

impl Services for Timed {
    fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
        self.script.listen()
    }
    fn chat(
        &self,
        messages: Vec<Message>,
    ) -> futures_util::stream::BoxStream<'static, net::Result<ChatEvent>> {
        self.script.chat(messages)
    }
    fn chat_second_try(
        &self,
        messages: Vec<Message>,
        _: ChatTold,
        _: net::Watch,
    ) -> Option<futures_util::stream::BoxStream<'static, net::Result<ChatEvent>>> {
        self.second
            .lock()
            .unwrap()
            .push(tokio::time::Instant::now());
        self.offers.then(|| self.script.chat(messages))
    }
    fn speak(
        &self,
        text: String,
    ) -> futures_util::stream::BoxStream<'static, net::Result<Vec<i16>>> {
        self.script.speak(text)
    }
}

/// A turn on the paused clock: how long it took from the words, when each second
/// try was asked for, and the journal's lines.
async fn on_the_clock(
    switch: Option<Duration>,
    offers: bool,
    sound: bool,
) -> (Duration, Vec<Duration>, Vec<String>) {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("What time is it?".to_owned())].into()),
        chats: Mutex::new([vec![], vec![]].into()),
        hold_chat: true,
        ..Script::default()
    });
    let services = Arc::new(Timed {
        script,
        second: Mutex::default(),
        offers,
    });
    let words_at = Arc::new(Mutex::new(None));
    let heard_at = words_at.clone();
    let journal: Arc<Mutex<Vec<String>>> = Arc::default();
    let mut engine = Engine::with_clock(
        services.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        Arc::new(|_, _, _, _| Box::pin(async { json!({"ok": true}) })),
        Arc::new(Instant::now),
    )
    .announcing(false)
    .waiting_with_a_sound(sound)
    .saying_when_down(true)
    .trying_the_chat_twice(switch)
    .keeping_second_try_journal(journal.clone())
    .hearing_words(Arc::new(move |_, _| {
        *heard_at.lock().unwrap() = Some(tokio::time::Instant::now());
    }));
    if sound {
        wait_clip_ready(&engine, true);
    }
    spoken_turn(&mut engine);
    let _ = sent(&mut engine).await;
    let words = words_at.lock().unwrap().expect("the words came");
    let tries = services
        .second
        .lock()
        .unwrap()
        .iter()
        .map(|at| at.duration_since(words))
        .collect();
    let lines = journal.lock().unwrap().clone();
    (words.elapsed(), tries, lines)
}

fn near(actual: Duration, expected: Duration) {
    let gap = actual.max(expected) - actual.min(expected);
    assert!(gap < Duration::from_millis(50), "{actual:?} {expected:?}");
}

/// With the wait's sound off the wait is 3 s, so 2.5 s leaves too little to answer
/// in: no second try, and the journal says so. The turn's end stays where it was.
#[tokio::test(start_paused = true)]
async fn with_too_little_of_the_wait_left_no_second_try_goes() {
    let (ended, tries, lines) = on_the_clock(Some(SECOND_TRY_AFTER), true, false).await;
    near(ended, BEGIN_WITHIN);
    assert!(tries.is_empty(), "{tries:?}");
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].contains("second try: none goes, ")
            && lines[0].contains("ms after the words: only ")
            && lines[0].contains("ms of the wait left"),
        "{lines:?}"
    );
    assert!(lines.iter().all(|line| !line.contains("chat round")));

    // The rule's edge: with 1.6 s left one goes; with 1.4 s left none does.
    let (_, tries, _) = on_the_clock(Some(Duration::from_millis(1_400)), true, false).await;
    assert_eq!(tries.len(), 1, "{tries:?}");
    let (_, tries, lines) = on_the_clock(Some(Duration::from_millis(1_600)), true, false).await;
    assert!(tries.is_empty(), "{tries:?}");
    assert!(lines[0].contains("none goes"), "{lines:?}");
}

/// On the product's times, with the wait's sound: the sound at 1.5 s, the
/// second try at 2.5 s, the slow line and the end at 4.5 s, each from the words,
/// as they are with the switch off. Services that can't offer a second try keep
/// the same times, and say nothing of one.
#[tokio::test(start_paused = true)]
async fn on_the_products_times_only_the_second_try_is_new() {
    let run = |switch, offers| on_the_clock(switch, offers, true);
    let limit = CUE_AFTER + BEGIN_WITHIN_AFTER_CUE;

    let (ended, tries, lines) = run(None, true).await;
    near(ended, limit);
    assert!(tries.is_empty() && lines.is_empty(), "{tries:?} {lines:?}");

    let (ended, tries, lines) = run(Some(SECOND_TRY_AFTER), true).await;
    near(ended, limit);
    assert_eq!(tries.len(), 1);
    near(tries[0], SECOND_TRY_AFTER);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].contains(" second try goes "), "{lines:?}");
    assert!(lines[1].contains(": neither answered in "), "{lines:?}");
    // The journal's words, as `lamp-voice-eval` must read them: the end's line
    // never reads as a second unanswered round.
    assert!(lines.iter().all(|line| !line.contains("chat round")));

    let (ended, tries, lines) = run(Some(SECOND_TRY_AFTER), false).await;
    near(ended, limit);
    assert_eq!(tries.len(), 1, "asked, and none offered");
    assert!(lines.is_empty(), "{lines:?}");
}
