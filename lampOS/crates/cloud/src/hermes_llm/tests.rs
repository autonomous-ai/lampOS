use super::*;
use std::os::unix::fs::PermissionsExt;
use tokio::sync::oneshot;

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const KEY: &str = "relay-key-for-the-test";
const ASKED: &str = r#"{"model":"deepseek-flash","messages":[]}"#;

/// A stand-in relay on loopback: one request, kept whole, answered with
/// `answer` sent in two pieces.
async fn relay(answer: &'static str, kind: &'static str) -> (String, oneshot::Receiver<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (kept, seen) = oneshot::channel();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 4096];
        let end = loop {
            if let Some(end) = find_head_end(&bytes) {
                break end;
            }
            let read = stream.read(&mut chunk).await.unwrap();
            assert!(read > 0);
            bytes.extend_from_slice(&chunk[..read]);
        };
        let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
        let length: usize = head
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .map(|value| value.trim().parse().unwrap())
            .unwrap_or(0);
        while bytes.len() < end + 4 + length {
            let read = stream.read(&mut chunk).await.unwrap();
            bytes.extend_from_slice(&chunk[..read]);
        }
        let _ = kept.send(bytes);
        let (one, two) = answer.split_at(answer.len() / 2);
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nTransfer-Encoding: chunked\r\n\r\n"
        );
        stream.write_all(head.as_bytes()).await.unwrap();
        for piece in [one, two] {
            let framed = format!("{:x}\r\n{piece}\r\n", piece.len());
            stream.write_all(framed.as_bytes()).await.unwrap();
            stream.flush().await.unwrap();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        stream.write_all(b"0\r\n\r\n").await.unwrap();
    });
    (format!("http://{address}/api/v1/ai/v1/deepseek"), seen)
}

/// The forwarder on a free loopback port, to `base`.
async fn forwarder(base: &str) -> SocketAddr {
    let listener = Forwarder::bind(0).await.unwrap();
    let address = listener.local_addr().unwrap();
    let forwarder = Arc::new(Forwarder::new(base, KEY, TOKEN).unwrap());
    tokio::spawn(forwarder.serve(listener));
    address
}

/// `raw` sent, and the answer's status, head and (de-chunked) body.
async fn ask(address: SocketAddr, raw: &[u8]) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(address).await.unwrap();
    stream.write_all(raw).await.unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await.unwrap();
    let end = find_head_end(&bytes).expect("an answer's head");
    let head = String::from_utf8_lossy(&bytes[..end]).into_owned();
    let status = head[9..12].parse().unwrap();
    let mut body = bytes[end + 4..].to_vec();
    if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        let mut plain = Vec::new();
        let mut rest = &body[..];
        loop {
            let line = find_head_end_line(rest);
            let size =
                usize::from_str_radix(std::str::from_utf8(&rest[..line]).unwrap(), 16).unwrap();
            rest = &rest[line + 2..];
            if size == 0 {
                break;
            }
            plain.extend_from_slice(&rest[..size]);
            rest = &rest[size + 2..];
        }
        body = plain;
    }
    (status, head, body)
}

fn find_head_end_line(bytes: &[u8]) -> usize {
    bytes
        .windows(2)
        .position(|window| window == b"\r\n")
        .unwrap()
}

fn chat(token: Option<&str>, body: &str) -> Vec<u8> {
    let bearer = token
        .map(|token| format!("Authorization: Bearer {token}\r\n"))
        .unwrap_or_default();
    format!(
        "POST {CHAT_PATH} HTTP/1.1\r\nHost: 127.0.0.1\r\n{bearer}\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

#[tokio::test]
async fn it_listens_on_loopback_only_and_answers_only_loopback_peers() {
    let listener = Forwarder::bind(0).await.unwrap();
    assert!(listener.local_addr().unwrap().ip().is_loopback());
    assert_eq!(
        listener.local_addr().unwrap().ip(),
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    );
    assert!(admits("127.0.0.1:5000".parse().unwrap()));
    assert!(admits("[::1]:5000".parse().unwrap()));
    assert!(!admits("192.168.1.20:5000".parse().unwrap()));
    assert!(!admits("0.0.0.0:5000".parse().unwrap()));
}

#[tokio::test]
async fn a_chat_goes_to_the_relay_with_the_key_and_streams_back() {
    let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"rain\"}}]}\n\ndata: [DONE]\n\n";
    let (base, seen) = relay(sse, "text/event-stream").await;
    let address = forwarder(&base).await;
    let body = r#"{"model":"deepseek-flash","stream":true,"messages":[]}"#;
    let (status, head, answer) = ask(address, &chat(Some(TOKEN), body)).await;
    assert_eq!(status, 200);
    assert!(head.contains("Content-Type: text/event-stream"));
    assert_eq!(String::from_utf8(answer).unwrap(), sse);
    let upstream = String::from_utf8(seen.await.unwrap()).unwrap();
    assert!(upstream.starts_with("POST /api/v1/ai/v1/deepseek/chat/completions HTTP/1.1\r\n"));
    let lower = upstream.to_ascii_lowercase();
    assert!(
        lower.contains(&format!("authorization: bearer {KEY}")),
        "the key added"
    );
    assert!(
        !upstream.contains(TOKEN),
        "the local token never leaves the lamp"
    );
    assert!(upstream.ends_with(body), "the body as sent");
}

#[tokio::test]
async fn the_models_list_goes_to_the_relay() {
    let (base, seen) = relay("{\"data\":[]}", "application/json").await;
    let address = forwarder(&base).await;
    let raw = format!("GET {MODELS_PATH} HTTP/1.1\r\nAuthorization: Bearer {TOKEN}\r\n\r\n");
    let (status, _, answer) = ask(address, raw.as_bytes()).await;
    assert_eq!((status, answer.as_slice()), (200, &b"{\"data\":[]}"[..]));
    let upstream = String::from_utf8(seen.await.unwrap()).unwrap();
    assert!(upstream.starts_with("GET /api/v1/ai/v1/deepseek/models HTTP/1.1\r\n"));
}

#[tokio::test]
async fn no_token_or_a_wrong_one_is_refused_and_nothing_reaches_the_relay() {
    let (base, mut seen) = relay("{}", "application/json").await;
    let address = forwarder(&base).await;
    let wrong = "f".repeat(64);
    for token in [None, Some(wrong.as_str()), Some("short")] {
        let (status, _, body) = ask(address, &chat(token, "{}")).await;
        assert_eq!(status, 401, "{token:?}");
        assert!(!String::from_utf8_lossy(&body).contains(KEY));
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(seen.try_recv().is_err(), "the relay was never asked");
}

#[tokio::test]
async fn other_paths_and_methods_are_refused() {
    let address = forwarder("http://127.0.0.1:9/none").await;
    for (raw, want) in [
        (
            format!(
                "POST /v1/deepseek/v1/tools/search HTTP/1.1\r\nAuthorization: Bearer {TOKEN}\r\nContent-Length: 2\r\n\r\n{{}}"
            ),
            404,
        ),
        (
            format!(
                "POST /v1/chat/completions HTTP/1.1\r\nAuthorization: Bearer {TOKEN}\r\nContent-Length: 2\r\n\r\n{{}}"
            ),
            404,
        ),
        (
            format!("GET / HTTP/1.1\r\nAuthorization: Bearer {TOKEN}\r\n\r\n"),
            404,
        ),
        (
            format!("GET {CHAT_PATH} HTTP/1.1\r\nAuthorization: Bearer {TOKEN}\r\n\r\n"),
            405,
        ),
        (
            format!(
                "POST {CHAT_PATH} HTTP/1.1\r\nAuthorization: Bearer {TOKEN}\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n"
            ),
            411,
        ),
    ] {
        assert_eq!(ask(address, raw.as_bytes()).await.0, want, "{raw}");
    }
}

#[tokio::test]
async fn a_body_over_the_cap_is_refused_before_it_is_read() {
    let address = forwarder("http://127.0.0.1:9/none").await;
    let raw = format!(
        "POST {CHAT_PATH} HTTP/1.1\r\nAuthorization: Bearer {TOKEN}\r\nContent-Length: {}\r\n\r\n",
        MOST_BODY + 1
    );
    assert_eq!(ask(address, raw.as_bytes()).await.0, 413);
}

#[tokio::test]
async fn an_unreachable_relay_is_a_502_naming_no_key() {
    let address = forwarder("http://127.0.0.1:9/none").await;
    let (status, _, body) = ask(address, &chat(Some(TOKEN), ASKED)).await;
    assert_eq!(status, 502);
    assert!(!String::from_utf8_lossy(&body).contains(KEY));
}

#[test]
fn starts_are_capped_per_minute() {
    let mut started = Started::default();
    let now = Instant::now();
    for _ in 0..PER_MINUTE {
        assert!(started.take(now));
    }
    assert!(!started.take(now));
    assert!(started.take(now + Duration::from_secs(61)));
}

/// #6226: the forwarder serves Hermes or OpenClaw on the lamp, with either
/// switch exactly `on`; neither on, nothing is served.
#[test]
fn either_engines_switch_serves_the_forwarder() {
    assert!(on_from(Some("on"), None));
    assert!(on_from(None, Some("on")));
    assert!(on_from(Some("on"), Some("on")));
    assert!(on_from(Some("off"), Some("on")));
    assert!(!on_from(None, None));
    assert!(!on_from(Some("off"), Some("off")));
    assert!(!on_from(Some("ON"), Some("yes")));
    assert_eq!(OPENCLAW_SWITCH, "LAMP_OPENCLAW_ON_LAMP");
}

/// #6226: either switch alone starts the listener, which then answers
/// (401 without the token); neither on, nothing is listened on.
#[tokio::test]
async fn either_switch_alone_starts_the_listener() {
    for (hermes, openclaw, listens) in [
        (Some("on"), None, true),
        (None, Some("on"), true),
        (None, None, false),
        (Some("off"), Some("off"), false),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let (said, mut lines) = tokio::sync::mpsc::unbounded_channel::<String>();
        let mut say = |line: &str| {
            let _ = said.send(line.to_owned());
        };
        // The forwarder never returns: it is driven beside the look, on
        // this task, and dropped with it.
        let serving = serve_when(
            on_from(hermes, openclaw),
            false,
            0,
            Some(dir.path()),
            None,
            KEY,
            &mut say,
        );
        let look = async {
            let line = tokio::time::timeout(Duration::from_secs(2), lines.recv())
                .await
                .ok()
                .flatten()?;
            let at: SocketAddr = line
                .strip_prefix("the Hermes forwarder listens on ")
                .unwrap_or_else(|| panic!("{line}"))
                .parse()
                .unwrap();
            assert!(at.ip().is_loopback());
            let (status, _, _) = ask(
                at,
                b"GET /v1/deepseek/models HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
            )
            .await;
            Some(status)
        };
        let status = tokio::select! {
            () = serving => unreachable!("the forwarder never returns"),
            status = look => status,
        };
        if listens {
            assert_eq!(status, Some(401), "{hermes:?} {openclaw:?}");
        } else {
            assert_eq!(status, None, "{hermes:?} {openclaw:?}");
            assert!(!dir.path().join(TOKEN_FILE).exists());
        }
    }
}

/// #6226: the product's lamp-cloud unit turns on both engines' switch.
#[test]
fn the_product_unit_turns_on_both_switches() {
    let unit = include_str!("../../../../deploy/product/lamp-cloud.service");
    for switch in [SWITCH, OPENCLAW_SWITCH] {
        assert!(
            unit.lines()
                .any(|line| line == format!("Environment={switch}=on")),
            "{switch}"
        );
    }
}

#[test]
fn the_relay_is_the_voices_deepseek() {
    assert_eq!(relay_base(None), COMPANY_DEEPSEEK);
    let product = include_str!("../../../../deploy/product/settings/second.json");
    assert_eq!(relay_base(Some(product)), COMPANY_DEEPSEEK);
    assert_eq!(
        relay_base(Some(
            r#"{"deepseek_base_url":"https://near.invalid/deepseek/"}"#
        )),
        COMPANY_DEEPSEEK,
        "a base on another host is never given the key"
    );
    assert_eq!(
        relay_base(Some(
            r#"{"deepseek_base_url":"https://campaign-api.autonomous.ai.invalid/deepseek"}"#
        )),
        COMPANY_DEEPSEEK
    );
    assert_eq!(
        relay_base(Some(
            r#"{"deepseek_base_url":"https://campaign-api.autonomous.ai/other/deepseek/"}"#
        )),
        "https://campaign-api.autonomous.ai/other/deepseek"
    );
    assert_eq!(
        relay_base(Some(r#"{"llm_base_url":"https://example.invalid/v1"}"#)),
        COMPANY_DEEPSEEK,
        "only the company relay carries the lamp's key"
    );
}

#[test]
fn the_token_is_made_once_private_and_kept() {
    let dir = tempfile::tempdir().unwrap();
    let first = token(dir.path()).unwrap();
    assert!(plain(&first) && first.len() == 64);
    let mode = std::fs::metadata(dir.path().join(TOKEN_FILE))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    assert_eq!(token(dir.path()).unwrap(), first);
    std::fs::write(dir.path().join(TOKEN_FILE), "bad\n").unwrap();
    let remade = token(dir.path()).unwrap();
    assert!(plain(&remade) && remade != first);
}

#[test]
fn the_debug_line_hides_the_key_and_token() {
    let forwarder = Forwarder::new(COMPANY_DEEPSEEK, KEY, TOKEN).unwrap();
    let shown = format!("{forwarder:?}");
    assert!(!shown.contains(KEY) && !shown.contains(TOKEN));
    assert!(Forwarder::new(COMPANY_DEEPSEEK, "", TOKEN).is_err());
    assert!(Forwarder::new(COMPANY_DEEPSEEK, KEY, "short").is_err());
}

#[tokio::test]
async fn a_chat_for_another_model_or_none_is_refused_before_the_relay() {
    let (base, mut seen) = relay("{}", "application/json").await;
    let address = forwarder(&base).await;
    for body in [
        r#"{"model":"deepseek-v4-pro","messages":[]}"#,
        r#"{"messages":[]}"#,
        r#"{"model":7}"#,
        "not json",
    ] {
        assert_eq!(
            ask(address, &chat(Some(TOKEN), body)).await.0,
            400,
            "{body}"
        );
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(seen.try_recv().is_err(), "the relay was never asked");
}

#[tokio::test]
async fn with_no_place_free_it_is_429_and_no_minute_start_is_used() {
    let listener = Forwarder::bind(0).await.unwrap();
    let address = listener.local_addr().unwrap();
    let forwarder = Arc::new(Forwarder::new("http://127.0.0.1:9/none", KEY, TOKEN).unwrap());
    let held = forwarder
        .running
        .clone()
        .acquire_many_owned(AT_ONCE as u32)
        .await
        .unwrap();
    tokio::spawn(forwarder.clone().serve(listener));
    // A body over the cap: refused for want of a place before it is looked at.
    let raw = format!(
        "POST {CHAT_PATH} HTTP/1.1\r\nAuthorization: Bearer {TOKEN}\r\nContent-Length: {}\r\n\r\n",
        MOST_BODY + 1
    );
    assert_eq!(ask(address, raw.as_bytes()).await.0, 429);
    assert!(forwarder.started.lock().unwrap().0.is_empty());
    drop(held);
    assert_eq!(ask(address, &chat(Some(TOKEN), ASKED)).await.0, 502);
    assert_eq!(forwarder.started.lock().unwrap().0.len(), 1);
}

/// One answer of [`metered_relay`]: its status, its `X-Usage-Day-Used` (or
/// none), its `Content-Type` and its body.
type Metered = (u16, Option<&'static str>, &'static str, &'static str);

/// A stand-in relay that answers each of `answers` on a connection of its
/// own, in order, each body sent in two pieces.
async fn metered_relay(answers: Vec<Metered>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        for (status, used, kind, body) in answers {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let mut chunk = [0u8; 4096];
            let end = loop {
                if let Some(end) = find_head_end(&bytes) {
                    break end;
                }
                let read = stream.read(&mut chunk).await.unwrap();
                assert!(read > 0);
                bytes.extend_from_slice(&chunk[..read]);
            };
            let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let length: usize = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .map(|value| value.trim().parse().unwrap())
                .unwrap_or(0);
            while bytes.len() < end + 4 + length {
                let read = stream.read(&mut chunk).await.unwrap();
                bytes.extend_from_slice(&chunk[..read]);
            }
            let used = used
                .map(|used| format!("X-Usage-Day-Used: {used}\r\n"))
                .unwrap_or_default();
            let head = format!(
                "HTTP/1.1 {status} Answer\r\nContent-Type: {kind}\r\n{used}\
                 Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(head.as_bytes()).await.unwrap();
            let (one, two) = body.split_at(body.len() / 2);
            for piece in [one, two] {
                let framed = format!("{:x}\r\n{piece}\r\n", piece.len());
                stream.write_all(framed.as_bytes()).await.unwrap();
                stream.flush().await.unwrap();
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            stream.write_all(b"0\r\n\r\n").await.unwrap();
        }
    });
    format!("http://{address}/api/v1/ai/v1/deepseek")
}

/// The forwarder on a free loopback port, to `base`, keeping the relay's
/// day in `meter` when given.
async fn metered_forwarder(base: &str, meter: Option<&Path>) -> SocketAddr {
    let listener = Forwarder::bind(0).await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut forwarder = Forwarder::new(base, KEY, TOKEN).unwrap();
    if let Some(meter) = meter {
        forwarder = forwarder.with_meter(meter);
    }
    tokio::spawn(Arc::new(forwarder).serve(listener));
    address
}

const STREAMED: &str = "data: {\"choices\":[{\"delta\":{\"content\":\"rain\"}}],\"usage\":null}\n\n\
     data: {\"choices\":[],\"usage\":{\"prompt_tokens\":9000,\"completion_tokens\":120,\
     \"prompt_cache_hit_tokens\":8000}}\n\ndata: [DONE]\n\n";
const WHOLE: &str = r#"{"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":5}}"#;
const LIMIT: &str = r#"{"error":{"code":"usage_limit","message":"day used"}}"#;

/// #7342: with the switch's meter, each answer's day used and each chat's
/// tokens are kept for lamp-skills; and every chat is still forwarded as it
/// came, past the voice's share and at the relay's own limit: the forwarder
/// never refuses for it, so the owner's own agent tasks go on.
#[tokio::test]
async fn the_meter_keeps_the_day_used_and_every_chat_still_goes() {
    use lamp_skills::relay_share::{self, Reading};
    let dir = tempfile::tempdir().unwrap();
    let base = metered_relay(vec![
        (200, Some("0.412"), "text/event-stream", STREAMED),
        (200, Some("0.990"), "application/json", WHOLE),
        (429, Some("1.000"), "application/json", LIMIT),
        (200, None, "application/json", WHOLE),
    ])
    .await;
    let address = metered_forwarder(&base, Some(dir.path())).await;
    let today = relay_share::day_of(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
    );
    let (status, _, answer) = ask(address, &chat(Some(TOKEN), ASKED)).await;
    assert_eq!((status, answer.as_slice()), (200, STREAMED.as_bytes()));
    let kept = relay_share::read(dir.path()).unwrap();
    assert_eq!((kept.day, kept.used, kept.answers), (today, Some(0.412), 1));
    assert_eq!(
        (
            kept.prompt_tokens,
            kept.cached_tokens,
            kept.completion_tokens
        ),
        (9000, 8000, 120)
    );
    // Past any share: forwarded whole all the same.
    let (status, _, answer) = ask(address, &chat(Some(TOKEN), ASKED)).await;
    assert_eq!((status, answer.as_slice()), (200, WHOLE.as_bytes()));
    // The relay's own refusal is the relay's, passed on as it came.
    let (status, _, answer) = ask(address, &chat(Some(TOKEN), ASKED)).await;
    assert_eq!((status, answer.as_slice()), (429, LIMIT.as_bytes()));
    let kept = relay_share::read(dir.path()).unwrap();
    assert_eq!(
        kept,
        Reading {
            day: today,
            used: Some(1.0),
            at_ms: kept.at_ms,
            answers: 3,
            uncounted: 1,
            prompt_tokens: 9100,
            cached_tokens: 8000,
            completion_tokens: 125,
        }
    );
    // A successful answer with no header: no day limit.
    assert_eq!(ask(address, &chat(Some(TOKEN), ASKED)).await.0, 200);
    assert_eq!(relay_share::read(dir.path()).unwrap().used, None);
}

/// Off, the folder given and the header said: the same answer, byte for
/// byte, and nothing kept; on, the same forwarder keeps it (Twelve's P3 3
/// on #7538: the off guard is held by a test that can fail).
#[tokio::test]
async fn without_the_meter_nothing_is_kept() {
    for on in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let base = metered_relay(vec![(200, Some("0.990"), "text/event-stream", STREAMED)]).await;
        let listener = Forwarder::bind(0).await.unwrap();
        let address = listener.local_addr().unwrap();
        let forwarder = Forwarder::new(&base, KEY, TOKEN)
            .unwrap()
            .metered(on, dir.path());
        tokio::spawn(Arc::new(forwarder).serve(listener));
        let (status, _, answer) = ask(address, &chat(Some(TOKEN), ASKED)).await;
        assert_eq!((status, answer.as_slice()), (200, STREAMED.as_bytes()));
        let kept = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(kept, usize::from(on), "metered {on}");
    }
}

/// The meter's own switch, as `run` reads it, reaches the forwarder
/// `serve_when` starts: off, it says nothing of a reading; on, it does.
#[tokio::test]
async fn the_meter_is_started_only_with_its_own_switch() {
    for metered in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (said, mut lines) = tokio::sync::mpsc::unbounded_channel::<String>();
        let mut say = |line: &str| {
            let _ = said.send(line.to_owned());
        };
        let serving = serve_when(true, metered, 0, Some(dir.path()), None, KEY, &mut say);
        let look = async {
            let mut told = Vec::new();
            while let Ok(Some(line)) =
                tokio::time::timeout(Duration::from_millis(300), lines.recv()).await
            {
                told.push(line);
            }
            told
        };
        let told = tokio::select! {
            () = serving => unreachable!("the forwarder never returns"),
            told = look => told,
        };
        assert!(
            told[0].starts_with("the Hermes forwarder listens on "),
            "{told:?}"
        );
        let keeps = told
            .iter()
            .any(|line| line.contains("keeps the relay's day used"));
        assert_eq!(keeps, metered, "{told:?}");
    }
}

#[test]
fn a_streams_usage_is_found_across_its_pieces_and_a_chats_line_is_numbers() {
    let mut usage = Usage::new("text/event-stream");
    for piece in STREAMED.as_bytes().chunks(7) {
        usage.feed(piece);
    }
    let found = usage.end().unwrap();
    assert_eq!(found["prompt_tokens"], 9000);
    assert_eq!(
        told(Some(&found), Some(0.4125)),
        "a chat answer: 9000 prompt tokens (8000 from the cache), 120 completion tokens; \
         the relay's day used 0.412"
    );
    assert_eq!(
        told(None, None),
        "a chat answer: no usage given; the relay's day used not limited"
    );
    let mut whole = Usage::new("application/json");
    whole.feed(WHOLE.as_bytes());
    assert_eq!(whole.end().unwrap()["completion_tokens"], 5);
}

/// The models list asked with no body, as an agent asks it at a run's start.
fn models() -> Vec<u8> {
    format!("GET {MODELS_PATH} HTTP/1.1\r\nAuthorization: Bearer {TOKEN}\r\n\r\n").into_bytes()
}

/// #7538's P2 (blue, Twelve): only the metered chat route may say "no day
/// limit". A models list with no header after a chat that said 0.9 leaves
/// 0.9, and so does a chat whose header can't be read as text; a value
/// read from the models list still counts.
#[tokio::test]
async fn only_a_chat_with_no_header_says_the_day_is_not_limited() {
    use lamp_skills::relay_share;
    let dir = tempfile::tempdir().unwrap();
    let base = metered_relay(vec![
        (200, Some("0.900"), "application/json", WHOLE),
        (200, None, "application/json", "{\"data\":[]}"),
        (200, Some("0.9\u{e9}"), "application/json", WHOLE),
        (200, Some("0.950"), "application/json", "{\"data\":[]}"),
        (200, None, "application/json", WHOLE),
    ])
    .await;
    let address = metered_forwarder(&base, Some(dir.path())).await;
    let used = || relay_share::read(dir.path()).unwrap().used;
    assert_eq!(ask(address, &chat(Some(TOKEN), ASKED)).await.0, 200);
    assert_eq!(used(), Some(0.9));
    assert_eq!(ask(address, &models()).await.0, 200);
    assert_eq!(used(), Some(0.9), "a models list with no header");
    assert_eq!(ask(address, &chat(Some(TOKEN), ASKED)).await.0, 200);
    assert_eq!(used(), Some(0.9), "a header that isn't text");
    assert_eq!(ask(address, &models()).await.0, 200);
    assert_eq!(used(), Some(0.95), "a models list that says");
    assert_eq!(ask(address, &chat(Some(TOKEN), ASKED)).await.0, 200);
    assert_eq!(used(), None, "a chat with no header: not limited");
}

/// A stand-in relay that answers each connection on a task of its own, the
/// `n`th accepted with `answers[n]`: its `X-Usage-Day-Used` and how long it
/// waits before its body's second half.
async fn overlapping_relay(answers: Vec<(&'static str, u64)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        for (used, wait_ms) in answers {
            let (mut stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                let mut chunk = [0u8; 4096];
                let end = loop {
                    if let Some(end) = find_head_end(&bytes) {
                        break end;
                    }
                    let read = stream.read(&mut chunk).await.unwrap();
                    assert!(read > 0);
                    bytes.extend_from_slice(&chunk[..read]);
                };
                while bytes.len() < end + 4 + ASKED.len() {
                    let read = stream.read(&mut chunk).await.unwrap();
                    bytes.extend_from_slice(&chunk[..read]);
                }
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                     X-Usage-Day-Used: {used}\r\nTransfer-Encoding: chunked\r\n\
                     Connection: close\r\n\r\n"
                );
                stream.write_all(head.as_bytes()).await.unwrap();
                let (one, two) = WHOLE.split_at(WHOLE.len() / 2);
                for (piece, wait) in [(one, wait_ms), (two, 0)] {
                    let framed = format!("{:x}\r\n{piece}\r\n", piece.len());
                    stream.write_all(framed.as_bytes()).await.unwrap();
                    stream.flush().await.unwrap();
                    tokio::time::sleep(Duration::from_millis(wait)).await;
                }
                stream.write_all(b"0\r\n\r\n").await.unwrap();
            });
        }
    });
    format!("http://{address}/api/v1/ai/v1/deepseek")
}

/// Twelve's P3 1: a long answer that began at 0.40 and ends after a short
/// one that said 0.55 leaves 0.55: the header is taken as it arrives, not
/// when its answer ends.
#[tokio::test]
async fn the_newest_header_wins_over_a_longer_answer_begun_before_it() {
    let dir = tempfile::tempdir().unwrap();
    let base = overlapping_relay(vec![("0.400", 600), ("0.550", 0)]).await;
    let address = metered_forwarder(&base, Some(dir.path())).await;
    let long = tokio::spawn(async move { ask(address, &chat(Some(TOKEN), ASKED)).await.0 });
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(ask(address, &chat(Some(TOKEN), ASKED)).await.0, 200);
    assert_eq!(long.await.unwrap(), 200);
    let kept = lamp_skills::relay_share::read(dir.path()).unwrap();
    assert_eq!((kept.used, kept.answers), (Some(0.55), 2));
}
