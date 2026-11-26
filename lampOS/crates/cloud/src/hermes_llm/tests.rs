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
            on_from(hermes, openclaw).then_some(0),
            Some(dir.path()),
            None,
            KEY,
            None,
            std::future::pending(),
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

/// Off (no meter): the same answer, byte for byte, and nothing kept.
#[tokio::test]
async fn without_the_meter_nothing_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let base = metered_relay(vec![(200, Some("0.990"), "text/event-stream", STREAMED)]).await;
    let address = metered_forwarder(&base, None).await;
    let (status, _, answer) = ask(address, &chat(Some(TOKEN), ASKED)).await;
    assert_eq!((status, answer.as_slice()), (200, STREAMED.as_bytes()));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

/// #7342 through both processes' code (#8012, Twelve's, the first builder's
/// and Heron's ask): the relay's `X-Usage-Day-Used` through this forwarder's
/// real meter into `day-used.json`, then lamp-skills' bound on a run
/// (`task::would_refuse`, `task::begin`) reading that file, with the share
/// the product's lamp-skills unit sets. Under the voice's share a run goes
/// on; at or over it the run is refused, in words the lamp can say; with
/// the header absent it is let through.
#[tokio::test]
async fn the_relays_day_used_through_the_meter_holds_a_skills_run_to_the_voices_share() {
    use lamp_skills::{relay_share, task};
    let unit = include_str!("../../../../deploy/product/lamp-skills.service");
    let value = unit
        .lines()
        .find_map(|line| {
            line.strip_prefix("Environment=")?
                .strip_prefix(relay_share::SWITCH)?
                .strip_prefix('=')
        })
        .expect("the product's lamp-skills sets the switch");
    let share = relay_share::share_from(Some(value)).expect("the product's switch is on");
    assert_eq!(share, 0.5, "`on` keeps half the day for the voice");

    let dir = tempfile::tempdir().unwrap();
    let cloud = dir.path().join("cloud");
    let state = dir.path().join("skills");
    std::fs::create_dir_all(&cloud).unwrap();
    std::fs::create_dir_all(&state).unwrap();
    let base = metered_relay(vec![
        (200, None, "application/json", WHOLE),
        (200, Some("0.400"), "text/event-stream", STREAMED),
        (200, Some("0.500"), "application/json", WHOLE),
        (200, Some("0.800"), "application/json", WHOLE),
        (200, None, "application/json", WHOLE),
    ])
    .await;
    let address = metered_forwarder(&base, Some(&cloud)).await;
    task::set_on_here(Some(true));
    relay_share::set_here(Some(share), Some(&cloud));
    // One chat through the forwarder, and the bound as lamp-skills reads it
    // just after.
    let skills = state.as_path();
    let chat_then_bound = |at: &'static str| async move {
        let (status, _, _) = ask(address, &chat(Some(TOKEN), ASKED)).await;
        assert_eq!(status, 200, "{at}");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        (now, task::would_refuse("gold-price", Some(skills), now))
    };
    let plain = "the lamp has used {used}% of its thinking for today, and the last 50% is kept \
                 for talking, so skills' runs wait until its day starts again";

    // The header absent: not known, so the run is let through.
    let (_, refused) = chat_then_bound("absent").await;
    assert_eq!(relay_share::read(&cloud).unwrap().used, None);
    assert_eq!(refused, None, "no header lets the run through");

    // Under the share: 40% used leaves 60%, so the run goes on, and is
    // counted as it begins.
    let (now_under, refused) = chat_then_bound("under").await;
    assert_eq!(relay_share::read(&cloud).unwrap().used, Some(0.4));
    assert_eq!(refused, None, "under the share the run goes on");
    drop(task::begin("gold-price", Some(&state), now_under).unwrap());
    let counted = std::fs::read(state.join(task::RUNS_FILE)).unwrap();

    // At the share and over it: refused before it is handed over, said
    // plainly, and not counted.
    for (at, used) in [("at", "50"), ("over", "80")] {
        let (now, refused) = chat_then_bound(at).await;
        let refused = refused.unwrap_or_else(|| panic!("{at} the share the run is refused"));
        let told = refused
            .strip_prefix(task::NOT_HANDED)
            .unwrap_or_else(|| panic!("{refused}"));
        assert_eq!(told, plain.replace("{used}", used), "{at}");
        assert_eq!(
            task::begin("gold-price", Some(&state), now).unwrap_err(),
            refused,
            "{at}"
        );
        assert_eq!(
            std::fs::read(state.join(task::RUNS_FILE)).unwrap(),
            counted,
            "{at}"
        );
    }

    // The header absent again, after a day used past the share: let through.
    let (_, refused) = chat_then_bound("absent again").await;
    assert_eq!(relay_share::read(&cloud).unwrap().used, None);
    assert_eq!(refused, None, "no header lets the run through");
    relay_share::set_here(None, None);
    task::set_on_here(None);
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

// The person's own model for the agent (#8735).

const OWN_KEY: &str = "own-key-for-the-test";

/// A stand-in endpoint that answers each of `answers` (status, body) on a
/// connection of its own, in order; each request it took, whole, sent on.
async fn endpoint(
    answers: Vec<(u16, &'static str)>,
) -> (String, tokio::sync::mpsc::UnboundedReceiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (took, taken) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        for (status, body) in answers {
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
            let _ = took.send(String::from_utf8(bytes).unwrap());
            let head = format!(
                "HTTP/1.1 {status} Answer\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(head.as_bytes()).await.unwrap();
        }
    });
    (format!("http://{address}/v1"), taken)
}

/// The person's OpenAI-compatible key handed in as lamp-cloud's unit hands
/// it (`own_<file>` in its credentials), at `url` with `model`, its check's
/// second line `tools`.
fn keep_compatible(dir: &Path, url: &str, model: &str, tools: &str) {
    for (name, text) in [
        ("openai_compatible.key", OWN_KEY),
        ("openai_compatible.url", url),
        ("openai_compatible.model", model),
        ("openai_compatible.checked", &format!("works\n{tools}")),
    ] {
        std::fs::write(dir.join(format!("own_{name}")), text).unwrap();
    }
}

/// Every credential the unit hands in, as its blank fallback (`\n`): a
/// lamp with no key of the person's.
fn blank(dir: &Path) {
    for name in handed_in() {
        std::fs::write(dir.join(format!("own_{name}")), "\n").unwrap();
    }
}

/// The forwarder to the relay at `base`, with the person's keys in `own`
/// and the relay's day in `meter` when given.
async fn own_forwarder(base: &str, own: &Path, meter: Option<&Path>) -> SocketAddr {
    let listener = Forwarder::bind(0).await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut forwarder = Forwarder::new(base, KEY, TOKEN)
        .unwrap()
        .with_own(&OwnKeys::credentials(own));
    if let Some(meter) = meter {
        forwarder = forwarder.with_meter(meter);
    }
    tokio::spawn(Arc::new(forwarder).serve(listener));
    address
}

fn models() -> Vec<u8> {
    format!("GET {MODELS_PATH} HTTP/1.1\r\nAuthorization: Bearer {TOKEN}\r\n\r\n").into_bytes()
}

/// The agent's chat goes to the person's model with their key, `model`
/// made theirs and the rest as sent, and `/models` is answered here; the
/// relay's day is not counted. Removed, the helper restarts lamp-cloud with
/// the blank credentials: the relay.
#[tokio::test]
async fn an_own_model_takes_the_agents_chat_and_a_removed_one_gives_it_back() {
    let own = tempfile::tempdir().unwrap();
    let meter = tempfile::tempdir().unwrap();
    let (url, mut asked) = endpoint(vec![(200, WHOLE)]).await;
    let (relay_base, mut relayed) = endpoint(vec![(200, WHOLE)]).await;
    blank(own.path());
    keep_compatible(own.path(), &url, "their-model-1", "tools");
    let address = own_forwarder(&relay_base, own.path(), Some(meter.path())).await;
    let body = r#"{"model":"deepseek-flash","stream":false,"messages":[],"tools":[]}"#;
    let (status, _, answer) = ask(address, &chat(Some(TOKEN), body)).await;
    assert_eq!((status, answer.as_slice()), (200, WHOLE.as_bytes()));
    let upstream = asked.recv().await.unwrap();
    assert!(upstream.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
    let lower = upstream.to_ascii_lowercase();
    assert!(lower.contains(&format!("authorization: bearer {OWN_KEY}")));
    assert!(!upstream.contains(KEY) && !upstream.contains(TOKEN));
    let sent: Value = serde_json::from_str(upstream.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(sent["model"], "their-model-1");
    assert_eq!(
        (sent["stream"].clone(), sent["tools"].clone()),
        (Value::Bool(false), serde_json::json!([]))
    );
    let (status, _, listed) = ask(address, &models()).await;
    assert_eq!(status, 200);
    let listed: Value = serde_json::from_slice(&listed).unwrap();
    assert_eq!(listed["data"][0]["id"], MODEL);
    assert!(
        lamp_skills::relay_share::read(meter.path()).is_none(),
        "the person's own spend is not the relay's day"
    );
    assert!(relayed.try_recv().is_err(), "the relay was never asked");
    // Removed: lamp-cloud started again with the blank fallbacks.
    blank(own.path());
    let address = own_forwarder(&relay_base, own.path(), None).await;
    let (status, _, _) = ask(address, &chat(Some(TOKEN), ASKED)).await;
    assert_eq!(status, 200);
    let upstream = relayed.recv().await.unwrap();
    assert!(
        upstream
            .to_ascii_lowercase()
            .contains(&format!("authorization: bearer {KEY}"))
    );
    assert!(
        upstream.ends_with(ASKED),
        "the relay's chat as the agent sent it"
    );
}

/// A key that fails at the person's endpoint (401) sends that chat to the
/// relay, as the agent sent it.
#[tokio::test]
async fn an_own_401_falls_back_to_the_relay() {
    let own = tempfile::tempdir().unwrap();
    let (url, mut asked) = endpoint(vec![(401, r#"{"error":"bad key"}"#)]).await;
    let (relay_base, mut relayed) = endpoint(vec![(200, WHOLE)]).await;
    keep_compatible(own.path(), &url, "their-model-1", "tools");
    let address = own_forwarder(&relay_base, own.path(), None).await;
    let (status, _, answer) = ask(address, &chat(Some(TOKEN), ASKED)).await;
    assert_eq!((status, answer.as_slice()), (200, WHOLE.as_bytes()));
    assert!(
        asked.recv().await.is_some(),
        "the person's model was asked first"
    );
    let upstream = relayed.recv().await.unwrap();
    assert!(
        upstream
            .to_ascii_lowercase()
            .contains(&format!("authorization: bearer {KEY}"))
    );
    assert!(upstream.ends_with(ASKED));
}

/// A forged model is still refused with an own route, before anything is
/// asked.
#[tokio::test]
async fn a_forged_model_is_still_a_400_on_the_own_route() {
    let own = tempfile::tempdir().unwrap();
    let (url, mut asked) = endpoint(vec![(200, WHOLE)]).await;
    keep_compatible(own.path(), &url, "their-model-1", "tools");
    let address = own_forwarder("http://127.0.0.1:9/none", own.path(), None).await;
    let forged = r#"{"model":"their-model-1","messages":[]}"#;
    assert_eq!(ask(address, &chat(Some(TOKEN), forged)).await.0, 400);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(asked.try_recv().is_err());
}

/// The route from the credentials: the voice's precedence; blank
/// fallbacks, hearing and voice keys alone, or a failed or missing tool
/// check: the relay.
#[test]
fn the_route_is_the_voices_model_and_only_with_its_tool_check() {
    let own = tempfile::tempdir().unwrap();
    let keys = OwnKeys::credentials(own.path());
    blank(own.path());
    assert_eq!(keys.route().err().unwrap(), "no model key of the person's");
    let write = |name: &str, text: &str| {
        std::fs::write(own.path().join(format!("own_{name}")), text).unwrap()
    };
    // Never handed in, but the relay even if they were.
    write("deepgram.key", "dg");
    write("elevenlabs.key", "el");
    assert_eq!(keys.route().err().unwrap(), "no model key of the person's");
    write("deepseek.key", "ds");
    write("deepseek.checked", "works");
    assert!(
        keys.route().err().unwrap().contains("no tool call"),
        "kept before the check"
    );
    write("deepseek.checked", "works\ntools");
    assert_eq!(
        keys.route().unwrap().told(),
        "own deepseek deepseek-flash at api.deepseek.com"
    );
    // OpenRouter over DeepSeek; its tool check failed: the relay, not DeepSeek.
    write("openrouter.key", "or");
    write("openrouter.model", "google/gemini-3.6-flash");
    write("openrouter.checked", "works\nno_tools");
    assert!(
        keys.route()
            .err()
            .unwrap()
            .contains("openrouter model made no tool call")
    );
    write("openrouter.checked", "works\ntools");
    assert_eq!(
        keys.route().unwrap().told(),
        "own openrouter google/gemini-3.6-flash at openrouter.ai"
    );
    write("openrouter.key", "\n");
    write("gemini.key", "gm");
    write("gemini.model", "gemini-3.6-flash");
    write("gemini.checked", "works\ntools");
    let route = keys.route().unwrap();
    assert_eq!(route.chat_base, own_model::GEMINI_OPENAI_API);
    // An endpoint not https is never given the key.
    write("openai_compatible.key", "oc");
    write("openai_compatible.url", "http://models.example.com/v1");
    write("openai_compatible.model", "m");
    write("openai_compatible.checked", "works\ntools");
    assert!(
        keys.route()
            .err()
            .unwrap()
            .contains("openai_compatible URL")
    );
}

/// The product unit hands in each model service's files and nothing else
/// of the person's folder, each with a blank fallback first, so a lamp with
/// no key of the person's still starts lamp-cloud: systemd 252 fails a
/// unit whose `LoadCredential=` file is missing unless a `SetCredential=`
/// of the same name gives the fallback (lamp-skills' `owner`, lamp-75c9, 5
/// Nov), and an empty one fails too, so it is one newline.
#[test]
fn the_unit_hands_in_only_the_model_keys_each_with_a_blank_fallback() {
    assert_eq!(OWN_FOLDER, lamp_setup::keys::FOLDER);
    let unit = include_str!("../../../../deploy/product/lamp-cloud.service");
    let lines: Vec<&str> = unit.lines().collect();
    let mut loaded = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        let Some(load) = line.strip_prefix("LoadCredential=own") else {
            continue;
        };
        let (name, from) = load.split_once(':').unwrap();
        let file = name
            .strip_prefix('_')
            .expect("own_<file>, never the folder");
        assert_eq!(from, format!("{OWN_FOLDER}/{file}"));
        assert!(
            lines[..at].contains(&format!("SetCredential=own_{file}:\\n").as_str()),
            "{file}'s blank fallback first"
        );
        loaded.push(file.to_owned());
    }
    assert_eq!(loaded, handed_in());
    assert!(
        !loaded
            .iter()
            .any(|file| file.starts_with("deepgram") || file.starts_with("elevenlabs"))
    );
    assert!(!unit.contains(&format!("ReadOnlyPaths=-{OWN_FOLDER}")));
}

/// A stand-in relay that streams one chat answer in two pieces, `gap`
/// apart.
async fn slow_relay(gap: Duration) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 4096];
        while !String::from_utf8_lossy(&bytes).ends_with(ASKED) {
            let read = stream.read(&mut chunk).await.unwrap();
            assert!(read > 0);
            bytes.extend_from_slice(&chunk[..read]);
        }
        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                    Transfer-Encoding: chunked\r\n\r\n";
        stream.write_all(head.as_bytes()).await.unwrap();
        for piece in ["data: {\"a\":1}\n\n", "data: [DONE]\n\n"] {
            let framed = format!("{:x}\r\n{piece}\r\n", piece.len());
            stream.write_all(framed.as_bytes()).await.unwrap();
            stream.flush().await.unwrap();
            tokio::time::sleep(gap).await;
        }
        let _ = stream.write_all(b"0\r\n\r\n").await;
    });
    format!("http://{address}/v1")
}

/// A stop while `gap` holds a streamed chat, drained `within`: the chat's
/// answer as the agent saw it, and the forwarder's lines.
async fn stopped_during_a_chat(gap: Duration, within: Duration) -> (Vec<u8>, Vec<String>) {
    let base = slow_relay(gap).await;
    let listener = Forwarder::bind(0).await.unwrap();
    let address = listener.local_addr().unwrap();
    let forwarder = Arc::new(Forwarder::new(&base, KEY, TOKEN).unwrap());
    let (stop, stopped) = oneshot::channel::<()>();
    let serving = tokio::spawn(forwarder.clone().serve_until(
        listener,
        async {
            let _ = stopped.await;
        },
        within,
    ));
    let asking = tokio::spawn(async move {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(&chat(Some(TOKEN), ASKED)).await.unwrap();
        let mut bytes = Vec::new();
        let _ = stream.read_to_end(&mut bytes).await;
        bytes
    });
    while forwarder.flights.lock().unwrap().is_empty() {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    stop.send(()).unwrap();
    let cut = serving.await.unwrap();
    // A new chat once the stop began is refused, never half-answered.
    assert!(forwarder.take_off().is_none());
    drop(forwarder);
    let answer = tokio::time::timeout(Duration::from_secs(5), asking)
        .await
        .map(Result::unwrap)
        .unwrap_or_default();
    (answer, cut)
}

/// #8735: lamp-cloud's stop (the keys helper's restart) lets a streamed
/// chat in flight end within the drain, its answer whole.
#[tokio::test]
async fn a_stop_drains_a_chat_in_flight() {
    let (answer, cut) =
        stopped_during_a_chat(Duration::from_millis(100), Duration::from_secs(5)).await;
    assert_eq!(
        cut,
        [
            "the Hermes forwarder: stopped, 1 chats in flight ended, 0 cut; \
          a chat sent before lamp-cloud is back is refused"
        ]
    );
    let answer = String::from_utf8_lossy(&answer);
    assert!(
        answer.contains("[DONE]") && answer.ends_with("0\r\n\r\n"),
        "{answer}"
    );
}

/// #8735: a chat past the drain's bound is cut, said with how long it had
/// run.
#[tokio::test]
async fn a_chat_past_the_drain_is_cut_and_said() {
    let (_, cut) = stopped_during_a_chat(Duration::from_secs(3), Duration::from_millis(200)).await;
    assert_eq!(cut.len(), 2, "{cut:?}");
    assert!(
        cut[0].contains("0 chats in flight ended, 1 cut"),
        "{}",
        cut[0]
    );
    assert!(
        cut[1].starts_with("the Hermes forwarder: a chat in flight was cut by a stop (")
            && cut[1].ends_with(" ms in)"),
        "{}",
        cut[1]
    );
}
