use lamp_memory_bench::{
    hash,
    live_http::{Config, Http},
    model::{Call, Client, Prices, Provider, Purpose},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "bench-http-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn artifacts(&self) -> PathBuf {
        self.0.join("provider")
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
struct Server {
    endpoint: String,
    request: Arc<Mutex<Vec<u8>>>,
    worker: Option<JoinHandle<()>>,
}
impl Server {
    fn new(status: &str, mime: &str, body: Vec<u8>, bytewise: bool, delay: Duration) -> Self {
        Self::with_hook(status, mime, body, bytewise, delay, None)
    }
    fn with_hook(
        status: &str,
        mime: &str,
        body: Vec<u8>,
        bytewise: bool,
        delay: Duration,
        hook: Option<Box<dyn FnOnce() + Send>>,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/chat/completions", listener.local_addr().unwrap());
        let request = Arc::new(Mutex::new(vec![]));
        let seen = request.clone();
        let header = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "fake server was not contacted");
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(e) => panic!("fake server accept: {e}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            *seen.lock().unwrap() = read_request(&mut stream);
            if let Some(hook) = hook {
                hook();
            }
            thread::sleep(delay);
            if stream.write_all(header.as_bytes()).is_err() {
                return;
            }
            if bytewise {
                for byte in body {
                    if stream.write_all(&[byte]).is_err() {
                        break;
                    }
                }
            } else {
                let _ = stream.write_all(&body);
            }
        });
        Self {
            endpoint,
            request,
            worker: Some(worker),
        }
    }
    fn finish(&mut self) -> Vec<u8> {
        self.worker.take().unwrap().join().unwrap();
        self.request.lock().unwrap().clone()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}
fn read_request(stream: &mut TcpStream) -> Vec<u8> {
    let mut data = Vec::new();
    let mut block = [0; 4096];
    loop {
        let n = stream.read(&mut block).unwrap();
        assert_ne!(n, 0);
        data.extend_from_slice(&block[..n]);
        assert!(data.len() < 2 * 1024 * 1024);
        if let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8(data[..end].to_vec()).unwrap();
            let size: usize = head
                .lines()
                .find_map(|l| {
                    l.to_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse().unwrap())
                })
                .unwrap();
            if data.len() >= end + 4 + size {
                return data;
            }
        }
    }
}
fn config(endpoint: String) -> Config {
    Config {
        endpoint,
        requested_model: "deepseek-flash".into(),
        expected_reported_model: "fixture-pinned-model".into(),
        context_tokens: 1_048_576,
    }
}
fn call() -> Call {
    Call::Chat {
        purpose: Purpose::Answer,
        messages: vec![json!({"role":"user","content":"Fictional owner question."})],
        max_output_tokens: 512,
        response_format: None,
        tools: vec![],
        tool_choice: "none".into(),
    }
}
fn prices() -> Prices {
    Prices {
        source: "offline fixture".into(),
        date: "2026-10-31".into(),
        input_microusd_per_million: 1_000_000,
        output_microusd_per_million: 1_000_000,
    }
}
fn event(delta: Value, finish: Value, usage: Value) -> Value {
    json!({"id":"fixture-request","model":"fixture-pinned-model","system_fingerprint":"fixture-deployment","choices":[{"index":0,"delta":delta,"finish_reason":finish}],"usage":usage})
}
fn wire(events: &[Value], done: bool) -> Vec<u8> {
    let mut body = ": keepalive\r\n\r\n".to_string();
    for event in events {
        body.push_str(&format!("data: {event}\r\n\r\n"));
    }
    if done {
        body.push_str("data: [DONE]\r\n\r\n");
    }
    body.into_bytes()
}
fn good() -> Vec<u8> {
    wire(
        &[event(
            json!({"role":"assistant","content":"Okay."}),
            json!("stop"),
            json!({"prompt_tokens":12,"completion_tokens":3,"total_tokens":15}),
        )],
        true,
    )
}
fn client(server: &Server, temp: &Temp) -> Client<Http> {
    Client::new(
        Http::loopback_fixture(config(server.endpoint.clone()), &temp.artifacts()).unwrap(),
        prices(),
        1_000_000,
    )
    .unwrap()
}

#[test]
fn exact_product_settings_fragmented_stream_and_durable_wire_provenance() {
    let temp = Temp::new();
    let raw = good();
    let mut server = Server::new(
        "200 OK",
        "text/event-stream; charset=utf-8",
        raw.clone(),
        true,
        Duration::ZERO,
    );
    let mut client = client(&server, &temp);
    let reply = client
        .call(&call(), Instant::now() + Duration::from_secs(3))
        .unwrap();
    assert_eq!(reply.content.as_deref(), Some("Okay."));
    assert_eq!(client.charged_microusd(), 15);
    let request = server.finish();
    let split = request.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let headers = String::from_utf8_lossy(&request[..split]);
    assert!(headers.contains("Bearer bench-offline-fixture-credential"));
    let body: Value = serde_json::from_slice(&request[split + 4..]).unwrap();
    assert_eq!(
        body,
        json!({"model":"deepseek-flash","messages":[{"role":"user","content":"Fictional owner question."}],"stream":true,"stream_options":{"include_usage":true},"thinking":{"type":"disabled"},"max_tokens":512})
    );
    let p = client.receipts()[0].provenance.as_ref().unwrap();
    assert_eq!(p.reported_model.as_deref(), Some("fixture-pinned-model"));
    assert_eq!(p.provider_request_id.as_deref(), Some("fixture-request"));
    assert!(p.response_complete);
    assert_eq!(p.response_sha256, hash(&raw));
    assert_eq!(
        fs::read(temp.artifacts().join(&p.artifact).join("response.sse")).unwrap(),
        raw
    );
    assert_eq!(
        p.request_sha256,
        hash(&fs::read(temp.artifacts().join(&p.artifact).join("request.json")).unwrap())
    );
    assert!(
        !serde_json::to_string(&client.receipts()[0])
            .unwrap()
            .contains("bench-offline-fixture-credential")
    );
}
#[test]
fn usage_only_terminal_chunk_and_opaque_tool_fragments_are_preserved() {
    let temp = Temp::new();
    let raw = wire(
        &[
            event(
                json!({"tool_calls":[{"index":0,"id":"call-fixture","type":"function","function":{"name":"recall","arguments":"{\"q\":"},"extra_content":{"opaque":"retained"}}],"reasoning_details":[{"type":"opaque","data":"keep"}]}),
                Value::Null,
                Value::Null,
            ),
            event(
                json!({"tool_calls":[{"index":0,"function":{"arguments":"  \"desk\"}"}}]}),
                json!("tool_calls"),
                Value::Null,
            ),
            json!({"choices":[],"usage":{"prompt_tokens":9,"completion_tokens":8}}),
        ],
        true,
    );
    let mut server = Server::new("200 OK", "text/event-stream", raw, false, Duration::ZERO);
    let mut client = client(&server, &temp);
    let mut request = call();
    if let Call::Chat {
        tools, tool_choice, ..
    } = &mut request
    {
        *tools = vec![
            json!({"type":"function","function":{"name":"recall","parameters":{"type":"object"}}}),
        ];
        *tool_choice = "auto".into();
    }
    let reply = client
        .call(&request, Instant::now() + Duration::from_secs(3))
        .unwrap();
    server.finish();
    assert_eq!(
        reply.tool_calls[0]["function"]["arguments"],
        "{\"q\":  \"desk\"}"
    );
    assert_eq!(
        reply.tool_calls[0]["extra_content"],
        json!({"opaque":"retained"})
    );
    assert_eq!(
        reply.reasoning_details,
        Some(json!([{"type":"opaque","data":"keep"}]))
    );
    assert_eq!(client.charged_microusd(), 17);
}
#[test]
fn missing_usage_is_unknown_and_keeps_the_full_reservation() {
    let temp = Temp::new();
    let raw = wire(
        &[event(
            json!({"content":"Okay."}),
            json!("stop"),
            Value::Null,
        )],
        true,
    );
    let mut server = Server::new("200 OK", "text/event-stream", raw, false, Duration::ZERO);
    let mut client = client(&server, &temp);
    assert!(
        client
            .call(&call(), Instant::now() + Duration::from_secs(3))
            .unwrap()
            .usage
            .is_none()
    );
    server.finish();
    let receipt = &client.receipts()[0];
    assert_eq!(receipt.reserved_microusd, receipt.charged_microusd);
    assert_eq!(receipt.outcome, "usage_unknown");
}
#[test]
fn premature_eof_does_not_turn_a_partial_usage_chunk_into_a_refund() {
    let temp = Temp::new();
    let raw = wire(
        &[event(
            json!({"content":"Okay."}),
            json!("stop"),
            json!({"prompt_tokens":12,"completion_tokens":3}),
        )],
        false,
    );
    let mut server = Server::new("200 OK", "text/event-stream", raw, false, Duration::ZERO);
    let mut client = client(&server, &temp);
    assert!(
        client
            .call(&call(), Instant::now() + Duration::from_secs(3))
            .is_err()
    );
    server.finish();
    let r = &client.receipts()[0];
    assert_eq!(r.charged_microusd, r.reserved_microusd);
    assert!(!r.provenance.as_ref().unwrap().response_complete);
    assert!(r.usage.is_none());
}
#[test]
fn truncation_is_a_failure_but_terminal_usage_is_still_charged() {
    let temp = Temp::new();
    let raw = wire(
        &[event(
            json!({"content":"unfinished"}),
            json!("length"),
            json!({"prompt_tokens":12,"completion_tokens":512}),
        )],
        true,
    );
    let mut server = Server::new("200 OK", "text/event-stream", raw, false, Duration::ZERO);
    let mut client = client(&server, &temp);
    assert!(
        client
            .call(&call(), Instant::now() + Duration::from_secs(3))
            .is_err()
    );
    server.finish();
    assert_eq!(client.charged_microusd(), 524);
    assert_eq!(
        client.receipts()[0]
            .provenance
            .as_ref()
            .unwrap()
            .finish_reason
            .as_deref(),
        Some("length")
    );
}
#[test]
fn over_limit_usage_stops_client_even_when_truncated_content_is_refused() {
    let temp = Temp::new();
    let raw = wire(
        &[event(
            json!({"content":"unfinished"}),
            json!("length"),
            json!({"prompt_tokens":12,"completion_tokens":513}),
        )],
        true,
    );
    let mut server = Server::new("200 OK", "text/event-stream", raw, false, Duration::ZERO);
    let mut client = client(&server, &temp);
    assert!(
        client
            .call(&call(), Instant::now() + Duration::from_secs(3))
            .is_err()
    );
    server.finish();
    assert!(client.stopped());
    assert_eq!(client.charged_microusd(), 525);
}
#[test]
fn changed_model_request_identity_and_usage_are_rejected() {
    for change in ["model", "id", "usage"] {
        let temp = Temp::new();
        let first = event(
            json!({"content":"start"}),
            if change == "usage" {
                json!("stop")
            } else {
                Value::Null
            },
            json!({"prompt_tokens":12,"completion_tokens":3}),
        );
        let mut last = event(
            json!({}),
            json!("stop"),
            json!({"prompt_tokens":12,"completion_tokens":3}),
        );
        if change == "usage" {
            last["usage"]["completion_tokens"] = json!(4);
        } else {
            last[change] = json!("unexpected");
        }
        let mut server = Server::new(
            "200 OK",
            "text/event-stream",
            wire(&[first, last], true),
            false,
            Duration::ZERO,
        );
        let mut client = client(&server, &temp);
        assert!(
            client
                .call(&call(), Instant::now() + Duration::from_secs(3))
                .is_err()
        );
        server.finish();
    }
}
#[test]
fn absolute_deadline_includes_headers_and_refuses_an_expired_call() {
    let temp = Temp::new();
    let mut server = Server::new(
        "200 OK",
        "text/event-stream",
        good(),
        false,
        Duration::from_secs(3),
    );
    let mut client = client(&server, &temp);
    assert!(client.call(&call(), Instant::now()).is_err());
    assert!(client.receipts().is_empty());
    let start = Instant::now();
    assert!(
        client
            .call(&call(), start + Duration::from_secs(2))
            .is_err()
    );
    assert!(start.elapsed() < Duration::from_millis(2800));
    server.finish();
    let r = &client.receipts()[0];
    assert_eq!(r.reserved_microusd, r.charged_microusd);
}
#[test]
fn redirects_service_errors_and_non_streams_are_not_answers() {
    for (status, mime, body) in [
        ("302 Found", "text/event-stream", good()),
        (
            "429 Too Many Requests",
            "application/json",
            br#"{"error":"fake refusal"}"#.to_vec(),
        ),
        (
            "200 OK",
            "application/json",
            br#"{"content":"not streamed"}"#.to_vec(),
        ),
    ] {
        let temp = Temp::new();
        let mut server = Server::new(status, mime, body, false, Duration::ZERO);
        let mut client = client(&server, &temp);
        assert!(
            client
                .call(&call(), Instant::now() + Duration::from_secs(3))
                .is_err()
        );
        server.finish();
        assert_eq!(
            client.receipts()[0].charged_microusd,
            client.receipts()[0].reserved_microusd
        );
    }
}
#[test]
fn huge_lines_duplicate_tools_and_inconsistent_totals_fail_closed() {
    let oversized = format!("data: {}\n\n", "x".repeat(256 * 1024 + 1)).into_bytes();
    let bad_tools = wire(
        &[event(
            json!({"tool_calls":[{"index":0,"id":"same","type":"function","function":{"name":"x","arguments":"{}"}},{"index":1,"id":"same","type":"function","function":{"name":"x","arguments":"{}"}}]}),
            json!("tool_calls"),
            Value::Null,
        )],
        true,
    );
    let bad_usage = wire(
        &[event(
            json!({"content":"x"}),
            json!("stop"),
            json!({"prompt_tokens":1,"completion_tokens":2,"total_tokens":4}),
        )],
        true,
    );
    for body in [oversized, bad_tools, bad_usage] {
        let temp = Temp::new();
        let mut server = Server::new("200 OK", "text/event-stream", body, false, Duration::ZERO);
        let mut client = client(&server, &temp);
        assert!(
            client
                .call(&call(), Instant::now() + Duration::from_secs(3))
                .is_err()
        );
        server.finish();
    }
}
#[test]
fn echoed_credential_is_never_written_to_response_evidence_or_errors() {
    let temp = Temp::new();
    let mut server = Server::new(
        "400 Bad Request",
        "application/json",
        br#"{"error":"bench-offline-fixture-credential"}"#.to_vec(),
        false,
        Duration::ZERO,
    );
    let mut client = client(&server, &temp);
    let error = client
        .call(&call(), Instant::now() + Duration::from_secs(3))
        .unwrap_err();
    server.finish();
    assert!(
        !error
            .to_string()
            .contains("bench-offline-fixture-credential")
    );
    assert!(!temp.artifacts().join("000001/response.sse").exists());
}
#[test]
fn configuration_and_context_refuse_before_any_connection() {
    let temp = Temp::new();
    for endpoint in [
        "http://example.invalid/chat",
        "http://localhost/chat",
        "http://user:password@127.0.0.1/chat",
        "http://127.0.0.1/chat?api_key=bad",
        "http://127.0.0.1/chat#bad",
    ] {
        assert!(Http::loopback_fixture(config(endpoint.into()), &temp.artifacts()).is_err());
    }
    let mut c = config("http://127.0.0.1:9/chat".into());
    c.context_tokens = 1024;
    let mut provider = Http::loopback_fixture(c, &temp.artifacts()).unwrap();
    let mut request = call();
    if let Call::Chat { messages, .. } = &mut request {
        messages[0]["content"] = json!("x".repeat(1024));
    }
    assert!(
        provider
            .invoke(&request, Instant::now() + Duration::from_secs(1))
            .unwrap_err()
            .to_string()
            .contains("context")
    );
    assert!(fs::read_dir(temp.artifacts()).unwrap().next().is_none());
}

#[test]
fn escaped_and_fragmented_credential_echoes_never_reach_artifacts_or_reply() {
    let encoded = String::from_utf8(wire(
        &[event(
            json!({"content":"bench-offline-fixture-credential"}),
            json!("stop"),
            Value::Null,
        )],
        true,
    ))
    .unwrap()
    .replace("credential", "\\u0063redential")
    .into_bytes();
    let fragmented = wire(
        &[
            event(
                json!({"content":"bench-offline-"}),
                Value::Null,
                Value::Null,
            ),
            event(
                json!({"content":"fixture-credential"}),
                json!("stop"),
                Value::Null,
            ),
        ],
        true,
    );
    for raw in [encoded, fragmented] {
        let temp = Temp::new();
        let mut server = Server::new("200 OK", "text/event-stream", raw, false, Duration::ZERO);
        let mut client = client(&server, &temp);
        let error = client
            .call(&call(), Instant::now() + Duration::from_secs(3))
            .unwrap_err();
        server.finish();
        assert!(
            !error
                .to_string()
                .contains("bench-offline-fixture-credential")
        );
        assert!(!temp.artifacts().join("000001/response.sse").exists());
        assert!(!temp.artifacts().join("000001/provenance.json").exists());
    }
}

#[test]
fn early_only_usage_cannot_refund_the_final_request() {
    let temp = Temp::new();
    let raw = wire(
        &[
            event(
                json!({"content":"first"}),
                Value::Null,
                json!({"prompt_tokens":1,"completion_tokens":1}),
            ),
            event(json!({"content":" more"}), json!("stop"), Value::Null),
        ],
        true,
    );
    let mut server = Server::new("200 OK", "text/event-stream", raw, false, Duration::ZERO);
    let mut client = client(&server, &temp);
    let reply = client
        .call(&call(), Instant::now() + Duration::from_secs(3))
        .unwrap();
    server.finish();
    assert!(reply.usage.is_none());
    assert_eq!(
        client.receipts()[0].charged_microusd,
        client.receipts()[0].reserved_microusd
    );
}
#[test]
fn evidence_failure_keeps_reservation_and_known_overrun_and_stops_campaign() {
    use lamp_memory_bench::campaign::{Campaign, Plan};
    for output in [3, 10_000] {
        let temp = Temp::new();
        let obstruction = temp.artifacts().join("000001/response.sse");
        let raw = wire(
            &[event(
                json!({"content":"Okay."}),
                json!("stop"),
                json!({"prompt_tokens":12,"completion_tokens":output}),
            )],
            true,
        );
        let mut server = Server::with_hook(
            "200 OK",
            "text/event-stream",
            raw,
            false,
            Duration::ZERO,
            Some(Box::new(move || {
                fs::write(obstruction, b"existing evidence").unwrap()
            })),
        );
        let plan = Plan {
            approved_cap_microusd: 1_000_000,
            policy_sha256: hash(b"fake policy"),
        };
        let campaign_path = temp.0.join("campaign");
        let mut campaign = Campaign::create(&campaign_path, plan.clone(), 1_000_000).unwrap();
        let mut client = client(&server, &temp)
            .with_audit(move |stage, receipt| campaign.audit("fake-run", stage, receipt));
        assert!(
            client
                .call(&call(), Instant::now() + Duration::from_secs(3))
                .is_err()
        );
        server.finish();
        assert!(client.stopped());
        let r = &client.receipts()[0];
        assert_eq!(r.usage.as_ref().unwrap().output_tokens, output);
        assert!(r.charged_microusd >= r.reserved_microusd && r.charged_microusd >= 12 + output);
        assert!(!r.provenance.as_ref().unwrap().evidence_complete);
        drop(client);
        assert!(Campaign::open(&campaign_path, plan, 1).is_err());
    }
}

#[test]
fn rejected_sse_is_checked_for_escaped_credentials() {
    let escaped = br#"data: {"error":"bench-offline-fixture-\u0063redential"}

"#
    .to_vec();
    {
        let status = "400 Bad Request";
        let raw = escaped;
        let temp = Temp::new();
        let mut server = Server::new(status, "text/event-stream", raw, false, Duration::ZERO);
        let mut client = client(&server, &temp);
        assert!(
            client
                .call(&call(), Instant::now() + Duration::from_secs(3))
                .is_err()
        );
        server.finish();
        assert!(!temp.artifacts().join("000001/response.sse").exists());
        assert!(client.stopped());
    }
}
