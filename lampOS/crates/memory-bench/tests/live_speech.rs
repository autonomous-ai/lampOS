use futures_util::{SinkExt, StreamExt};
use lamp_memory_bench::{
    audio, hash,
    live_speech::{Config, Services, Speech},
    model::{Call, Client, Prices, Provider},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::{
        Message,
        handshake::server::{Callback, ErrorResponse, Request, Response},
        protocol::{CloseFrame, frame::coding::CloseCode},
    },
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "bench-ws-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
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
#[derive(Default)]
struct Seen {
    uri: String,
    audio: Vec<u8>,
    frames: Vec<(usize, Duration)>,
    controls: Vec<String>,
}
struct Scenario {
    finals: Vec<Value>,
    metadata: Option<Value>,
    close: Option<CloseCode>,
    block_evidence: Option<PathBuf>,
}
fn result(text: &str, start: f64, duration: f64) -> Value {
    json!({"type":"Results","channel_index":[0,1],"start":start,"duration":duration,"is_final":true,"from_finalize":true,"channel":{"alternatives":[{"transcript":text,"confidence":0.9}]},"metadata":{"request_id":"fake-request","model_uuid":"fake-model-uuid","model_info":{"name":"general","arch":"nova-3","version":"fake-v1"}}})
}
fn metadata(seconds: f64) -> Value {
    json!({"type":"Metadata","request_id":"fake-request","duration":seconds,"channels":1})
}
impl Scenario {
    fn normal() -> Self {
        Self {
            finals: vec![result("heard words", 0.0, 0.04)],
            metadata: Some(metadata(0.04)),
            close: Some(CloseCode::Normal),
            block_evidence: None,
        }
    }
}
struct CaptureHeaders(Arc<Mutex<Seen>>);
impl Callback for CaptureHeaders {
    fn on_request(self, request: &Request, response: Response) -> Result<Response, ErrorResponse> {
        assert_eq!(
            request.headers()["authorization"],
            "Token bench-offline-speech-credential"
        );
        self.0.lock().unwrap().uri = request.uri().to_string();
        Ok(response)
    }
}
struct Server {
    endpoint: String,
    seen: Arc<Mutex<Seen>>,
    worker: Option<JoinHandle<()>>,
}
impl Server {
    fn new(scenario: Scenario) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("ws://{}/v1/listen", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Seen::default()));
        let capture = seen.clone();
        let worker = thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                let (socket, _) = tokio::time::timeout(Duration::from_secs(10), listener.accept())
                    .await
                    .unwrap()
                    .unwrap();
                let mut socket = accept_hdr_async(socket, CaptureHeaders(capture.clone()))
                    .await
                    .unwrap();
                let start = Instant::now();
                while let Ok(Some(Ok(frame))) =
                    tokio::time::timeout(Duration::from_secs(5), socket.next()).await
                {
                    match frame {
                        Message::Binary(bytes) => {
                            let mut got = capture.lock().unwrap();
                            got.frames.push((bytes.len(), start.elapsed()));
                            got.audio.extend(bytes);
                        }
                        Message::Text(text) => {
                            let value: Value = serde_json::from_str(&text).unwrap();
                            let kind = value["type"].as_str().unwrap();
                            capture.lock().unwrap().controls.push(kind.into());
                            if kind == "Finalize" {
                                for value in &scenario.finals {
                                    if socket.send(Message::text(value.to_string())).await.is_err()
                                    {
                                        return;
                                    }
                                }
                            } else if kind == "CloseStream" {
                                if let Some(path) = &scenario.block_evidence {
                                    fs::create_dir(path).unwrap();
                                }
                                if let Some(value) = &scenario.metadata
                                    && socket.send(Message::text(value.to_string())).await.is_err()
                                {
                                    return;
                                }
                                if let Some(code) = scenario.close {
                                    let _ = socket
                                        .close(Some(CloseFrame {
                                            code,
                                            reason: "fake closure".into(),
                                        }))
                                        .await;
                                }
                                return;
                            }
                        }
                        Message::Close(_) => return,
                        _ => {}
                    }
                }
            });
        });
        Self {
            endpoint,
            seen,
            worker: Some(worker),
        }
    }
    fn finish(&mut self) {
        self.worker.take().unwrap().join().unwrap();
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(w) = self.worker.take() {
            w.join().unwrap();
        }
    }
}
fn input(samples: usize) -> audio::Input {
    let size = samples * 2;
    let mut b = b"RIFF".to_vec();
    b.extend(((size + 36) as u32).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend([1, 0, 1, 0]);
    b.extend(16000u32.to_le_bytes());
    b.extend(32000u32.to_le_bytes());
    b.extend([2, 0, 16, 0]);
    b.extend(b"data");
    b.extend((size as u32).to_le_bytes());
    b.resize(size + 44, 0);
    audio::Input::from_wav(&b).unwrap()
}
fn config(server: &Server) -> Config {
    Config {
        endpoint: server.endpoint.clone(),
        expected_model_arch: "nova-3".into(),
        expected_model_revision: Some("fake-v1".into()),
    }
}
fn provider(server: &Server, temp: &Temp) -> Speech {
    Speech::loopback_fixture(config(server), &temp.artifacts()).unwrap()
}
fn client(server: &Server, temp: &Temp) -> Client<Services> {
    let p = Services::new(None, Some(provider(server, temp)));
    Client::new(
        p,
        Prices {
            source: "offline".into(),
            date: "2026-10-31".into(),
            input_microusd_per_million: 1,
            output_microusd_per_million: 1,
        },
        10_000,
    )
    .unwrap()
    .with_speech(audio::Prices {
        source: "offline".into(),
        date: "2026-10-31".into(),
        microusd_per_minute: 60_000,
    })
    .unwrap()
}
fn call() -> Call {
    Call::Transcribe { audio: input(640) }
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(8)
}
#[test]
fn actual_websocket_matches_product_query_paces_pcm_and_preserves_evidence() {
    let temp = Temp::new();
    let mut s = Scenario::normal();
    let mut interim = result("wrong interim", 0.0, 0.04);
    interim["is_final"] = json!(false);
    s.finals = vec![
        interim,
        result("heard", 0.0, 0.02),
        result("words", 0.02, 0.02),
    ];
    let mut server = Server::new(s);
    let mut c = client(&server, &temp);
    let reply = c.call(&call(), deadline()).unwrap();
    server.finish();
    assert_eq!(reply.content.as_deref(), Some("heard words"));
    assert_eq!(c.charged_microusd(), 40);
    let got = server.seen.lock().unwrap();
    assert_eq!(got.audio, input(640).pcm());
    assert_eq!(got.controls, ["Finalize", "CloseStream"]);
    assert_eq!(got.frames.len(), 2);
    assert_eq!(got.frames[0].0, 640);
    assert!(got.frames[1].1 >= Duration::from_millis(15));
    let query = reqwest::Url::parse(&format!("ws://localhost{}", got.uri)).unwrap();
    let pairs: std::collections::BTreeMap<_, _> = query
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    assert_eq!(pairs.len(), 10);
    assert_eq!(pairs["model"], "nova-3-general");
    assert_eq!(pairs["sample_rate"], "16000");
    assert_eq!(pairs["keyterm"], "Lamp");
    assert_eq!(pairs["endpointing"], "300");
    let p = c.receipts()[0].provenance.as_ref().unwrap();
    assert!(p.evidence_complete && p.response_complete);
    assert_eq!(p.provider_request_id.as_deref(), Some("fake-request"));
    let raw = fs::read(temp.artifacts().join("000001/response.jsonl")).unwrap();
    assert_eq!(hash(&raw), p.response_sha256);
    assert!(
        !String::from_utf8(raw)
            .unwrap()
            .contains("bench-offline-speech-credential")
    );
    assert_eq!(p.audio.as_ref().unwrap().confidence, Some(0.9));
}
#[test]
fn missing_confidence_stays_unknown_and_missing_duration_keeps_reservation() {
    let temp = Temp::new();
    let mut s = Scenario::normal();
    s.finals[0]["channel"]["alternatives"][0]
        .as_object_mut()
        .unwrap()
        .remove("confidence");
    s.metadata = Some(json!({"type":"Metadata","request_id":"fake-request","channels":1}));
    let mut server = Server::new(s);
    let mut c = client(&server, &temp);
    assert!(c.call(&call(), deadline()).is_ok());
    server.finish();
    let r = &c.receipts()[0];
    assert!(r.audio_usage.is_none());
    assert_eq!(r.charged_microusd, r.reserved_microusd);
    assert!(
        r.provenance
            .as_ref()
            .unwrap()
            .audio
            .as_ref()
            .unwrap()
            .confidence
            .is_none()
    );
}
#[test]
fn final_duplicate_is_idempotent_but_conflicting_repeat_is_rejected() {
    for conflict in [false, true] {
        let temp = Temp::new();
        let mut s = Scenario::normal();
        let mut again = s.finals[0].clone();
        if conflict {
            again["channel"]["alternatives"][0]["transcript"] = json!("different");
        }
        s.finals.push(again);
        let mut server = Server::new(s);
        let mut p = provider(&server, &temp);
        let r = p.invoke(&call(), deadline());
        server.finish();
        assert_eq!(r.is_err(), conflict);
        if let Ok(r) = r {
            assert_eq!(r.content.as_deref(), Some("heard words"));
        }
    }
}
#[test]
fn interims_alone_never_become_final_transcripts() {
    let temp = Temp::new();
    let mut s = Scenario::normal();
    s.finals[0]["is_final"] = json!(false);
    let mut server = Server::new(s);
    let mut p = provider(&server, &temp);
    assert!(p.invoke(&call(), deadline()).is_err());
    server.finish();
}
#[test]
fn model_and_request_identity_changes_fail_closed() {
    for field in ["version", "request_id"] {
        let temp = Temp::new();
        let mut s = Scenario::normal();
        let mut bad = s.finals[0].clone();
        if field == "version" {
            bad["metadata"]["model_info"]["version"] = json!("wrong-v2");
        } else {
            bad["metadata"]["request_id"] = json!("wrong-request");
        }
        s.finals.push(bad);
        let mut server = Server::new(s);
        let mut p = provider(&server, &temp);
        assert!(p.invoke(&call(), deadline()).is_err());
        server.finish();
    }
}
#[test]
fn overlap_out_of_order_and_out_of_audio_finals_are_rejected() {
    for (start, duration) in [(0.01, 0.03), (0.0, 0.2)] {
        let temp = Temp::new();
        let mut s = Scenario::normal();
        s.finals.push(result("extra", start, duration));
        let mut server = Server::new(s);
        let mut p = provider(&server, &temp);
        assert!(p.invoke(&call(), deadline()).is_err());
        server.finish();
    }
}
#[test]
fn abnormal_close_keeps_terminal_duration_and_no_close_keeps_reservation() {
    for close in [Some(CloseCode::Error), None] {
        let temp = Temp::new();
        let mut s = Scenario::normal();
        s.close = close;
        if close.is_none() {
            s.metadata = None;
        }
        let mut server = Server::new(s);
        let mut c = client(&server, &temp);
        assert!(c.call(&call(), deadline()).is_err());
        server.finish();
        let r = &c.receipts()[0];
        assert_eq!(r.charged_microusd, 40);
        assert_eq!(r.audio_usage.is_some(), close.is_some());
    }
}
#[test]
fn provider_duration_overrun_is_charged_and_stops_more_calls() {
    let temp = Temp::new();
    let mut s = Scenario::normal();
    s.metadata = Some(metadata(0.08));
    let mut server = Server::new(s);
    let mut c = client(&server, &temp);
    assert!(c.call(&call(), deadline()).is_err());
    server.finish();
    assert_eq!(c.charged_microusd(), 80);
    assert!(c.stopped());
    assert!(c.call(&call(), deadline()).is_err());
}
#[test]
fn evidence_write_failure_keeps_debit_and_stops_spending() {
    let temp = Temp::new();
    let mut s = Scenario::normal();
    s.block_evidence = Some(temp.artifacts().join("000001/response.jsonl"));
    let mut server = Server::new(s);
    let mut c = client(&server, &temp);
    assert!(c.call(&call(), deadline()).is_err());
    server.finish();
    assert!(c.stopped());
    assert_eq!(c.charged_microusd(), 40);
    assert!(
        !c.receipts()[0]
            .provenance
            .as_ref()
            .unwrap()
            .evidence_complete
    );
}
#[test]
fn credential_echo_is_never_archived_or_returned_in_errors() {
    let temp = Temp::new();
    let mut s = Scenario::normal();
    s.finals[0]["channel"]["alternatives"][0]["transcript"] =
        json!("bench-offline-speech-credential");
    let mut server = Server::new(s);
    let mut c = client(&server, &temp);
    let error = c.call(&call(), deadline()).unwrap_err();
    server.finish();
    assert!(
        !error
            .to_string()
            .contains("bench-offline-speech-credential")
    );
    assert!(!temp.artifacts().join("000001/response.jsonl").exists());
    assert!(c.stopped());
    assert_eq!(c.charged_microusd(), 40);
}
#[test]
fn absolute_deadline_cancels_upload_and_preserves_partial_identity() {
    let temp = Temp::new();
    let mut server = Server::new(Scenario::normal());
    let mut c = client(&server, &temp);
    let start = Instant::now();
    let call = Call::Transcribe {
        audio: input(32_000),
    };
    assert!(c.call(&call, start + Duration::from_millis(1000)).is_err());
    server.finish();
    assert!(start.elapsed() < Duration::from_secs(3));
    assert_eq!(c.charged_microusd(), 2000);
    assert!(c.stopped());
    let p = c.receipts()[0].provenance.as_ref().unwrap();
    assert!(p.audio.as_ref().unwrap().samples_sent < 32_000);
    assert!(!p.response_complete);
}
#[test]
fn only_explicit_loopback_fixtures_can_use_plain_websockets() {
    for endpoint in [
        "ws://example.org/listen",
        "wss://127.0.0.1/listen",
        "ws://user:pass@127.0.0.1/listen",
        "ws://127.0.0.1/listen?token=value",
    ] {
        let temp = Temp::new();
        assert!(
            Speech::loopback_fixture(
                Config {
                    endpoint: endpoint.into(),
                    expected_model_arch: "nova-3".into(),
                    expected_model_revision: None
                },
                &temp.artifacts()
            )
            .is_err()
        );
        assert!(!temp.artifacts().exists());
    }
}

#[test]
fn terminal_model_map_is_checked_and_failed_identity_keeps_known_duration() {
    for mismatch in [false, true] {
        let temp = Temp::new();
        let mut s = Scenario::normal();
        let mut terminal = metadata(0.04);
        terminal["models"] = json!(["fake-model-uuid"]);
        terminal["model_info"] = json!({"fake-model-uuid":{"name":"general","arch":"nova-3","version":if mismatch {"wrong-v2"} else {"fake-v1"}}});
        s.metadata = Some(terminal);
        let mut server = Server::new(s);
        let mut c = client(&server, &temp);
        assert_eq!(c.call(&call(), deadline()).is_err(), mismatch);
        server.finish();
        assert_eq!(
            c.receipts()[0].audio_usage.as_ref().unwrap().audio_micros,
            40_000
        );
    }
}
#[test]
fn a_later_interim_cannot_be_silently_lost_at_normal_close() {
    let temp = Temp::new();
    let mut s = Scenario::normal();
    let mut interim = result("lost tail", 0.02, 0.02);
    interim["is_final"] = json!(false);
    s.finals = vec![result("first", 0.0, 0.02), interim];
    let mut server = Server::new(s);
    let mut p = provider(&server, &temp);
    assert!(p.invoke(&call(), deadline()).is_err());
    server.finish();
}
#[test]
fn processed_duration_does_not_prove_final_transcript_coverage() {
    let temp = Temp::new();
    let mut s = Scenario::normal();
    s.metadata = Some(metadata(0.2));
    let mut server = Server::new(s);
    let mut p = provider(&server, &temp);
    assert!(
        p.invoke(&Call::Transcribe { audio: input(3200) }, deadline())
            .is_err()
    );
    server.finish();
}

#[test]
fn a_later_final_cannot_hide_an_unresolved_earlier_interval() {
    let temp = Temp::new();
    let mut scenario = Scenario::normal();
    let mut interim = result("lost", 0.01, 0.01);
    interim["is_final"] = json!(false);
    scenario.finals = vec![
        result("first", 0.0, 0.01),
        interim,
        result("last", 0.02, 0.02),
    ];
    let mut server = Server::new(scenario);
    let mut provider = provider(&server, &temp);
    assert!(provider.invoke(&call(), deadline()).is_err());
    server.finish();
}
