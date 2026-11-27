use super::super::{Forwarder, Started, find_head_end};
use super::*;
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const SEARCH: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
const KEY: &str = "relay-key-for-the-test";
const DEAD: &str = "http://127.0.0.1:9/none";

/// One answer of a stand-in server: status, content type, body, delay.
type Canned = (u16, &'static str, String, Duration);

/// A stand-in server on loopback answering each connection with the next
/// of `answers` (the last one again after them); every request kept whole.
async fn server(answers: Vec<Canned>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = seen.clone();
    tokio::spawn(async move {
        let mut index = 0;
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let (code, kind, body, delay) = answers[index.min(answers.len() - 1)].clone();
            index += 1;
            let kept = kept.clone();
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                let mut chunk = [0u8; 4096];
                let end = loop {
                    if let Some(end) = find_head_end(&bytes) {
                        break end;
                    }
                    match stream.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(read) => bytes.extend_from_slice(&chunk[..read]),
                    }
                };
                let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                let length: usize = head
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .map_or(0, |value| value.trim().parse().unwrap());
                while bytes.len() < end + 4 + length {
                    match stream.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(read) => bytes.extend_from_slice(&chunk[..read]),
                    }
                }
                kept.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&bytes).into_owned());
                tokio::time::sleep(delay).await;
                let head = format!(
                    "HTTP/1.1 {code} X\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\
                     Connection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes()).await;
                let _ = stream.write_all(body.as_bytes()).await;
            });
        }
    });
    (format!("http://{address}"), seen)
}

fn json_answer(body: &str) -> Canned {
    (200, "application/json", body.to_owned(), Duration::ZERO)
}

/// The forwarder with the search, DeepSeek under `relay`, the fallback at
/// `fallback`, its lines kept; and the forwarder to tune.
async fn searching(
    relay: &str,
    fallback: &str,
    tune: impl FnOnce(&mut Search),
) -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    let mut forwarder = Forwarder::new(&format!("{relay}/api/v1/ai/v1/deepseek"), KEY, TOKEN)
        .unwrap()
        .with_search_to(SEARCH, &format!("{fallback}/html/"))
        .unwrap();
    let lines = Arc::new(Mutex::new(Vec::new()));
    let kept = lines.clone();
    let search = forwarder.search.as_mut().unwrap();
    search.note = Arc::new(move |line: &str| kept.lock().unwrap().push(line.to_owned()));
    tune(search);
    let listener = Forwarder::bind(0).await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(Arc::new(forwarder).serve(listener));
    (address, lines)
}

/// `raw` sent: the status and the body.
async fn ask(address: SocketAddr, raw: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(address).await.unwrap();
    stream.write_all(raw.as_bytes()).await.unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await.unwrap();
    let end = find_head_end(&bytes).expect("an answer's head");
    let head = String::from_utf8_lossy(&bytes[..end]).into_owned();
    (
        head[9..12].parse().unwrap(),
        String::from_utf8_lossy(&bytes[end + 4..]).into_owned(),
    )
}

fn get(path: &str) -> String {
    format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
}

fn searched(token: &str, query: &str) -> String {
    get(&format!("/v1/search/{token}/search?q={query}&format=json"))
}

const DEEPSEEK: &str = r#"{"results":[
  {"title":"Lamp news","url":"https://example.com/a","summary":"The summary.","date":"2026-11-20"},
  {"title":"Second","url":"https://example.org/b","text":"Its text."},
  {"title":"No address","summary":"dropped"},
  {"title":"Bad address","url":"javascript:alert(1)","summary":"dropped"}
]}"#;

const DUCK_PAGE: &str = r#"<html><body>
<div class="result results_links results_links_deep result--ad">
  <a rel="nofollow" class="result__a" href="https://duckduckgo.com/y.js?ad_domain=ads.example&amp;u3=x">An ad</a>
  <a class="result__snippet" href="https://duckduckgo.com/y.js?x=1">Buy now</a>
</div>
<div class="result results_links results_links_deep web-result">
  <h2 class="result__title">
    <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fducks%3Fa%3D1%26b%3D2&amp;rut=abc">Ducks &amp; <b>lamps</b></a>
  </h2>
  <a class="result__snippet" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fducks">All about &lt;ducks&gt; &#x27;and&#39; <b>lamps</b>.</a>
</div>
<div class="result results_links results_links_deep web-result">
  <h2 class="result__title"><a rel="nofollow" class="result__a" href="https://example.net/direct">Direct link</a></h2>
  <a class="result__snippet" href="https://example.net/direct">Plain snippet</a>
</div>
</body></html>"#;

#[tokio::test]
async fn only_the_search_token_opens_the_search_and_never_the_chat() {
    let (relay, seen) = server(vec![json_answer(DEEPSEEK)]).await;
    let (address, _) = searching(&relay, DEAD, |_| {}).await;
    // A wrong, missing or the chat's token in the path: 401.
    for path in [
        format!("/v1/search/{}/search?q=x", "f".repeat(64)),
        "/v1/search//search?q=x".to_owned(),
        "/v1/search/search?q=x".to_owned(),
        format!("/v1/search/{TOKEN}/search?q=x"),
    ] {
        assert_eq!(ask(address, &get(&path)).await.0, 401, "{path}");
    }
    // The chat's token as a bearer does not open the search either.
    let raw = format!(
        "GET /v1/search/{TOKEN}/search?q=x HTTP/1.1\r\nAuthorization: Bearer {TOKEN}\r\n\r\n"
    );
    assert_eq!(ask(address, &raw).await.0, 401);
    let raw =
        format!("GET /v1/search/search?q=x HTTP/1.1\r\nAuthorization: Bearer {TOKEN}\r\n\r\n");
    assert_eq!(ask(address, &raw).await.0, 401);
    // The search's token does not open the chat or the models.
    let chat = format!(
        "POST {} HTTP/1.1\r\nAuthorization: Bearer {SEARCH}\r\nContent-Length: 2\r\n\r\n{{}}",
        super::super::CHAT_PATH
    );
    assert_eq!(ask(address, &chat).await.0, 401);
    let models = format!(
        "GET {} HTTP/1.1\r\nAuthorization: Bearer {SEARCH}\r\n\r\n",
        super::super::MODELS_PATH
    );
    assert_eq!(ask(address, &models).await.0, 401);
    assert!(seen.lock().unwrap().is_empty(), "DeepSeek never asked");
    // The right token, in the path or as the bearer: answered.
    assert_eq!(ask(address, &searched(SEARCH, "x")).await.0, 200);
    let raw = format!(
        "GET /v1/search/search?q=x&format=json HTTP/1.1\r\nAuthorization: Bearer {SEARCH}\r\n\r\n"
    );
    let (status, body) = ask(address, &raw).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(seen.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn without_the_search_its_paths_are_not_found() {
    let listener = Forwarder::bind(0).await.unwrap();
    let address = listener.local_addr().unwrap();
    let forwarder = Forwarder::new(DEAD, KEY, TOKEN).unwrap();
    tokio::spawn(Arc::new(forwarder).serve(listener));
    assert_eq!(ask(address, &searched(SEARCH, "x")).await.0, 404);
}

#[test]
fn the_search_needs_a_token_of_its_own() {
    let forwarder = || Forwarder::new(DEAD, KEY, TOKEN).unwrap();
    assert!(forwarder().with_search_to(TOKEN, DEAD).is_err());
    assert!(forwarder().with_search_to("short", DEAD).is_err());
    assert!(forwarder().with_search_to(SEARCH, DEAD).is_ok());
}

#[test]
fn the_search_token_is_kept_private_and_never_the_chats() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let search = super::super::search_token(dir.path()).unwrap();
    let chat = super::super::token(dir.path()).unwrap();
    assert!(plain(&search) && search != chat);
    let mode = std::fs::metadata(dir.path().join(super::super::SEARCH_TOKEN_FILE))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    assert_eq!(super::super::search_token(dir.path()).unwrap(), search);
    // A search token equal to the chat's (a copied file) is made anew.
    std::fs::write(
        dir.path().join(super::super::SEARCH_TOKEN_FILE),
        format!("{chat}\n"),
    )
    .unwrap();
    let remade = super::super::search_token(dir.path()).unwrap();
    assert!(plain(&remade) && remade != chat);
}

#[tokio::test]
async fn deepseeks_answer_is_searxngs_json() {
    let (relay, seen) = server(vec![json_answer(DEEPSEEK)]).await;
    let (address, _) = searching(&relay, DEAD, |_| {}).await;
    let (status, body) = ask(
        address,
        &get(&format!(
            "/v1/search/{SEARCH}/search?q=lamp+news%21&format=json&pageno=1&language=en"
        )),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let asked = seen.lock().unwrap()[0].clone();
    assert!(asked.starts_with("POST /api/v1/ai/v1/deepseek/v1/tools/search HTTP/1.1\r\n"));
    assert!(asked.contains(&format!("authorization: Bearer {KEY}")));
    let sent: Value =
        serde_json::from_str(&asked[find_head_end(asked.as_bytes()).unwrap() + 4..]).unwrap();
    assert_eq!(sent, json!({"query": "lamp news!", "count": 5}));
    let answer: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        answer,
        json!({
            "query": "lamp news!",
            "number_of_results": 2,
            "results": [
                {"title": "Lamp news", "url": "https://example.com/a", "content": "The summary.",
                 "publishedDate": "2026-11-20", "engine": "deepseek"},
                {"title": "Second", "url": "https://example.org/b", "content": "Its text.",
                 "engine": "deepseek"},
            ],
            "answers": [], "suggestions": [], "infoboxes": [], "unresponsive_engines": [],
        })
    );
}

#[tokio::test]
async fn bad_asks_are_refused_before_any_engine_and_a_long_query_is_cut() {
    let (relay, seen) = server(vec![json_answer(r#"{"results":[]}"#)]).await;
    let (address, _) = searching(&relay, DEAD, |_| {}).await;
    for (raw, want) in [
        (get(&format!("/v1/search/{SEARCH}/search?format=json")), 400),
        (
            get(&format!("/v1/search/{SEARCH}/search?q=+++&format=json")),
            400,
        ),
        (
            get(&format!("/v1/search/{SEARCH}/search?q=x&format=html")),
            400,
        ),
        (get(&format!("/v1/search/{SEARCH}/other?q=x")), 404),
        (get(&format!("/v1/search/{SEARCH}?q=x")), 404),
        (
            format!("POST /v1/search/{SEARCH}/search?q=x HTTP/1.1\r\nContent-Length: 0\r\n\r\n"),
            405,
        ),
    ] {
        assert_eq!(ask(address, &raw).await.0, want, "{raw}");
    }
    assert!(seen.lock().unwrap().is_empty(), "no engine asked");
    let long = "a".repeat(MOST_QUERY_CHARS + 100);
    let (status, body) = ask(address, &searched(SEARCH, &long)).await;
    assert_eq!(status, 200);
    let answer: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(answer["query"].as_str().unwrap().len(), MOST_QUERY_CHARS);
    let asked = seen.lock().unwrap()[0].clone();
    assert!(asked.contains(&format!("\"query\":\"{}\"", "a".repeat(MOST_QUERY_CHARS))));
    assert!(!asked.contains(&"a".repeat(MOST_QUERY_CHARS + 1)));
}

#[tokio::test]
async fn deepseeks_429_or_5xx_or_timeout_falls_back_to_duckduckgo() {
    let page = (200, "text/html", DUCK_PAGE.to_owned(), Duration::ZERO);
    for (failure, why) in [
        (
            (429, "application/json", "{}".to_owned(), Duration::ZERO),
            "too many requests",
        ),
        (
            (503, "text/plain", "down".to_owned(), Duration::ZERO),
            "HTTP error",
        ),
        (
            (
                200,
                "application/json",
                DEEPSEEK.to_owned(),
                Duration::from_secs(3),
            ),
            "timeout",
        ),
        (
            (
                200,
                "application/json",
                "not json".to_owned(),
                Duration::ZERO,
            ),
            "unreadable answer",
        ),
    ] {
        let (relay, _) = server(vec![failure]).await;
        let (duck, duck_seen) = server(vec![page.clone()]).await;
        let (address, _) = searching(&relay, &duck, |search| {
            search.within = (Duration::from_millis(300), Duration::from_secs(2));
        })
        .await;
        let (status, body) = ask(address, &searched(SEARCH, "ducks+%26+lamps")).await;
        assert_eq!(status, 200, "{why}: {body}");
        let answer: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(answer["unresponsive_engines"], json!([["deepseek", why]]));
        assert_eq!(answer["number_of_results"], 2, "{why}");
        assert_eq!(answer["results"][0]["engine"], "duckduckgo");
        let asked = duck_seen.lock().unwrap()[0].clone();
        assert!(
            asked.starts_with("GET /html/?q=ducks+%26+lamps HTTP/1.1\r\n"),
            "{asked}"
        );
        assert!(asked.contains("user-agent: Mozilla/5.0"));
        assert!(!asked.contains(KEY), "the key never goes to the fallback");
    }
}

#[test]
fn duckduckgos_page_is_read_into_results_without_its_ads() {
    assert_eq!(
        from_duckduckgo(DUCK_PAGE),
        vec![
            json!({"title": "Ducks & lamps", "url": "https://example.com/ducks?a=1&b=2",
                   "content": "All about <ducks> 'and' lamps.", "engine": "duckduckgo"}),
            json!({"title": "Direct link", "url": "https://example.net/direct",
                   "content": "Plain snippet", "engine": "duckduckgo"}),
        ]
    );
    assert!(from_duckduckgo("<html>no results</html>").is_empty());
    let many = (0..9)
        .map(|n| format!(r#"<a class="result__a" href="https://example.com/{n}">R{n}</a>"#))
        .collect::<String>();
    assert_eq!(from_duckduckgo(&many).len(), COUNT);
}

#[tokio::test]
async fn both_down_is_a_502_with_no_results_and_both_named() {
    let (relay, _) = server(vec![(500, "text/plain", "x".to_owned(), Duration::ZERO)]).await;
    let (address, _) = searching(&relay, DEAD, |_| {}).await;
    let (status, body) = ask(address, &searched(SEARCH, "x")).await;
    assert_eq!(status, 502);
    let answer: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(answer["results"], json!([]));
    assert_eq!(answer["number_of_results"], 0);
    assert_eq!(
        answer["unresponsive_engines"],
        json!([["deepseek", "HTTP error"], ["duckduckgo", "unreachable"]])
    );
    // DuckDuckGo's challenge page (202) is its limit, not results.
    let (relay, _) = server(vec![(500, "text/plain", "x".to_owned(), Duration::ZERO)]).await;
    let (duck, _) = server(vec![(
        202,
        "text/html",
        "anomaly".to_owned(),
        Duration::ZERO,
    )])
    .await;
    let (address, _) = searching(&relay, &duck, |_| {}).await;
    let (status, body) = ask(address, &searched(SEARCH, "x")).await;
    assert_eq!(status, 502);
    assert!(
        body.contains(r#"["duckduckgo","too many requests"]"#),
        "{body}"
    );
}

#[test]
fn each_engine_keeps_to_one_a_second_and_its_minutes_budget() {
    let mut rate = Started::new(PER_MINUTE, SPACED);
    let now = Instant::now();
    assert_eq!(rate.take(now), Some(Duration::ZERO));
    // The next waits its turn; one more is passed over.
    assert_eq!(rate.take(now), Some(SPACED));
    assert_eq!(rate.take(now), None);
    // Calls a second apart: up to the minute's budget, then none.
    let mut rate = Started::new(PER_MINUTE, SPACED);
    for second in 0..PER_MINUTE as u64 {
        assert_eq!(
            rate.take(now + Duration::from_secs(second)),
            Some(Duration::ZERO)
        );
    }
    assert_eq!(
        rate.take(now + Duration::from_secs(PER_MINUTE as u64)),
        None
    );
    assert_eq!(
        rate.take(now + Duration::from_secs(61)),
        Some(Duration::ZERO)
    );
}

#[tokio::test]
async fn past_the_lamps_deepseek_budget_the_search_goes_to_duckduckgo() {
    let (relay, seen) = server(vec![json_answer(DEEPSEEK)]).await;
    let (duck, _) = server(vec![(
        200,
        "text/html",
        DUCK_PAGE.to_owned(),
        Duration::ZERO,
    )])
    .await;
    let (address, _) = searching(&relay, &duck, |search| {
        let mut rate = search.deepseek_rate.lock().unwrap();
        let now = Instant::now();
        rate.at = vec![now; PER_MINUTE];
    })
    .await;
    let (status, body) = ask(address, &searched(SEARCH, "x")).await;
    assert_eq!(status, 200);
    let answer: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(answer["results"][0]["engine"], "duckduckgo");
    assert_eq!(
        answer["unresponsive_engines"],
        json!([["deepseek", "rate limited on the lamp"]])
    );
    assert!(seen.lock().unwrap().is_empty(), "DeepSeek never asked");
}

#[tokio::test]
async fn past_the_searches_at_once_it_is_429_and_the_chats_places_are_untouched() {
    let (relay, _) = server(vec![json_answer(DEEPSEEK)]).await;
    let mut forwarder = Forwarder::new(&relay, KEY, TOKEN)
        .unwrap()
        .with_search_to(SEARCH, DEAD)
        .unwrap();
    let search = forwarder.search.as_mut().unwrap();
    let held = search
        .running
        .clone()
        .try_acquire_many_owned(AT_ONCE as u32)
        .unwrap();
    let forwarder = Arc::new(forwarder);
    let listener = Forwarder::bind(0).await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(forwarder.clone().serve(listener));
    assert_eq!(ask(address, &searched(SEARCH, "x")).await.0, 429);
    assert!(forwarder.started.lock().unwrap().at.is_empty());
    assert_eq!(forwarder.running.available_permits(), super::super::AT_ONCE);
    drop(held);
    assert_eq!(ask(address, &searched(SEARCH, "x")).await.0, 200);
    assert!(
        forwarder.started.lock().unwrap().at.is_empty(),
        "no chat start used"
    );
}

#[tokio::test]
async fn no_query_token_or_key_is_ever_in_its_lines() {
    let query = "secret-words-of-the-person";
    let (good, _) = server(vec![json_answer(DEEPSEEK)]).await;
    let (bad, _) = server(vec![(
        429,
        "application/json",
        "{}".to_owned(),
        Duration::ZERO,
    )])
    .await;
    let (duck, _) = server(vec![(
        200,
        "text/html",
        DUCK_PAGE.to_owned(),
        Duration::ZERO,
    )])
    .await;
    let mut lines = Vec::new();
    for (relay, fallback) in [(&good, DEAD), (&bad, duck.as_str()), (&bad, DEAD)] {
        let (address, said) = searching(relay, fallback, |_| {}).await;
        ask(address, &searched(SEARCH, query)).await;
        lines.extend(said.lock().unwrap().clone());
    }
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[0].contains("2 results from deepseek"), "{lines:?}");
    assert!(lines[1].contains("from duckduckgo, after deepseek (too many requests)"));
    assert!(lines[2].contains("no results"));
    for line in &lines {
        for secret in [query, SEARCH, TOKEN, KEY] {
            assert!(!line.contains(secret), "{line}");
        }
    }
}

#[test]
fn entities_and_markup_come_out_as_text() {
    assert_eq!(
        text("a <b>b</b>&amp;&#99;&#x64; &bogus; &"),
        "a b&cd &bogus; &"
    );
    assert_eq!(entities("&lt;&gt;&quot;&#39;&nbsp;"), "<>\"' ");
}
