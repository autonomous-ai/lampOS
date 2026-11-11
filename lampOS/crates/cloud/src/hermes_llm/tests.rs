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
