use futures_util::{SinkExt, StreamExt};
use lamp_memory_bench::{
    audio, campaign, evaluation, files, hash,
    ledger::{self, Ledger, Manifest, Mode},
    live_http,
    live_plan::{Adapter, Fixture, Policy, Run},
    live_run, live_speech, model,
    protocol::EventKind,
    speech,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::fs::{DirBuilderExt, symlink},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tokio_tungstenite::{accept_async, tungstenite::Message};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "bench-campaign-command-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
        Self(p)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn source() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/memory-bench/development")
}
fn policy() -> Policy {
    let revision = "a".repeat(40);
    let arms: Vec<String> = ["none", "full_history", "lampos_today_partial"]
        .into_iter()
        .chain(files::ARMS)
        .map(str::to_owned)
        .collect();
    let binary = fs::read(env!("CARGO_BIN_EXE_lamp-memory-bench")).unwrap();
    Policy {
        version: 1,
        runner_revision: revision.clone(),
        runner_sha256: hash(&fs::read(std::env::current_exe().unwrap()).unwrap()),
        evaluation_sha256: hash(&serde_json::to_vec(&evaluation::policy()).unwrap()),
        chat: live_http::Config {
            endpoint: "http://127.0.0.1:1/chat/completions".into(),
            requested_model: "deepseek-flash".into(),
            expected_reported_model: "fixture-model".into(),
            context_tokens: 1_048_576,
        },
        chat_prices: model::Prices {
            source: "offline fixture".into(),
            date: "2026-11-01".into(),
            input_microusd_per_million: 1_000_000,
            output_microusd_per_million: 1_000_000,
        },
        speech: live_speech::Config {
            endpoint: "ws://127.0.0.1:1/v1/listen".into(),
            expected_model_arch: "nova-3".into(),
            expected_model_revision: Some("fixture-v1".into()),
        },
        speech_prices: audio::Prices {
            source: "offline fixture".into(),
            date: "2026-11-01".into(),
            microusd_per_minute: 60_000,
        },
        repetitions: 3,
        fixtures: BTreeMap::from([(
            "development".into(),
            Fixture {
                profile: "month".into(),
                setup_order: arms.clone(),
                registration_revision: revision.clone(),
                registration_sha256: hash(&fs::read(source().join("registration.json")).unwrap()),
                audio_manifest_sha256: hash(b"no prepared audio yet"),
                speech_budget_microusd: 1_000_000,
            },
        )]),
        adapters: arms
            .into_iter()
            .map(|name| {
                (
                    name,
                    Adapter {
                        revision: revision.clone(),
                        executable_sha256: hash(&binary),
                        arguments: vec!["arm".into(), revision.clone()],
                        budget_microusd: 1_000_000,
                    },
                )
            })
            .collect(),
    }
}
fn approval(policy: &Policy) -> campaign::Plan {
    campaign::Plan {
        approved_cap_microusd: policy.planned_microusd().unwrap(),
        policy_sha256: policy.hash().unwrap(),
    }
}
fn run(arm: &str, repetition: u32) -> Run {
    Run {
        fixture: "development".into(),
        arm: arm.into(),
        repetition,
        fixture_directory: source(),
        adapter_executable: PathBuf::from(env!("CARGO_BIN_EXE_lamp-memory-bench")),
    }
}
#[test]
fn policy_requires_frozen_scope_one_tiers_build_and_exact_approval() {
    let p = policy();
    p.check_approval(&approval(&p), true).unwrap();
    assert!(p.validate(false).is_err());
    let mut changed = p.clone();
    changed
        .adapters
        .get_mut("tiers_minus_facts")
        .unwrap()
        .revision = "b".repeat(40);
    assert!(changed.validate(true).is_err());
    let mut changed = p.clone();
    changed
        .fixtures
        .get_mut("development")
        .unwrap()
        .setup_order
        .swap(3, 4);
    assert!(changed.validate(true).is_err());
    let mut changed = p.clone();
    changed.evaluation_sha256 = hash(b"different prompts");
    assert!(changed.validate(true).is_err());
    let mut changed = p.clone();
    changed.adapters.get_mut("none").unwrap().budget_microusd = u64::MAX;
    assert!(changed.validate(true).is_err());
    let mut changed = p.clone();
    changed.chat.endpoint = "http://localhost/chat/completions".into();
    assert!(changed.validate(true).is_err());
    let mut changed = p.clone();
    changed.chat_prices.output_microusd_per_million += 1;
    assert!(changed.check_approval(&approval(&p), true).is_err());
    let mut low = approval(&p);
    low.approved_cap_microusd -= 1;
    assert!(p.check_approval(&low, true).is_err());
    assert!(
        p.admitted(&Run {
            fixture: "aging".into(),
            ..run("tiers", 1)
        })
        .is_err()
    );
    assert!(p.admitted(&run("none", 4)).is_err());
    assert_eq!(
        p.admitted(&run("none", 1)).unwrap().2,
        live_run::run_id("development", "none", 1).unwrap()
    );
}
#[test]
fn no_campaign_or_provider_is_created_above_cap_or_with_wrong_runner() {
    let d = Dir::new();
    let mut p = policy();
    let mut a = approval(&p);
    a.approved_cap_microusd -= 1;
    assert!(live_run::initialize(&p, a, &d.0.join("campaign"), true).is_err());
    assert!(!d.0.join("campaign").exists());
    p.runner_sha256 = hash(b"wrong executable");
    assert!(live_run::initialize(&p, approval(&p), &d.0.join("campaign"), true).is_err());
    assert!(!d.0.join("campaign").exists());
}
fn prior_manifest(p: &Policy) -> Manifest {
    let corpus = files::read_corpus(&source()).unwrap();
    Manifest {
        run_id: live_run::run_id("development", "none", 1).unwrap(),
        mode: Mode::Recorded,
        arm: "none".into(),
        implementation_revision: p.adapters["none"].revision.clone(),
        registration_revision: p.fixtures["development"].registration_revision.clone(),
        registration_hash: p.fixtures["development"].registration_sha256.clone(),
        fixture_hash: hash(&fs::read(source().join("events.jsonl")).unwrap()),
        transcript_hash: None,
        timezone: corpus.persona.timezone,
        model: p.chat.requested_model.clone(),
        config: json!({"policy_sha256":p.hash().unwrap(),"fixture":"development","repetition":1}),
        budget_microusd: 1_000_000,
        seed: corpus.persona.seed,
    }
}
#[test]
fn prior_order_is_bound_to_fixture_and_identity_even_when_attempt_failed() {
    let d = Dir::new();
    let p = policy();
    let root = d.0.join("campaign");
    live_run::initialize(&p, approval(&p), &root, true).unwrap();
    let err = live_run::execute(&p, approval(&p), &root, &run("none", 2), true).unwrap_err();
    assert!(err.to_string().contains("No such file"));
    let mut m = prior_manifest(&p);
    m.config["fixture"] = json!("another-fixture");
    let path = root.join("runs").join(&m.run_id);
    let mut l = Ledger::create(&path, &m).unwrap();
    l.append("failed", json!({"scores":null})).unwrap();
    drop(l);
    let err = live_run::execute(&p, approval(&p), &root, &run("none", 2), true).unwrap_err();
    assert!(err.to_string().contains("prior attempt differs"));
    assert!(
        !root
            .join("runs")
            .join(live_run::run_id("development", "none", 2).unwrap())
            .exists()
    );
}
#[test]
fn substituted_run_parent_is_refused_before_provider_construction() {
    let d = Dir::new();
    let p = policy();
    let root = d.0.join("campaign");
    live_run::initialize(&p, approval(&p), &root, true).unwrap();
    fs::remove_dir(root.join("runs")).unwrap();
    let elsewhere = d.0.join("elsewhere");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&elsewhere)
        .unwrap();
    symlink(&elsewhere, root.join("runs")).unwrap();
    assert!(
        live_run::execute(&p, approval(&p), &root, &run("none", 1), true)
            .unwrap_err()
            .to_string()
            .contains("private and real")
    );
    assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 0);
}
fn request(stream: &mut TcpStream) -> Value {
    let mut data = Vec::new();
    let mut block = [0; 4096];
    loop {
        let n = stream.read(&mut block).unwrap();
        assert_ne!(n, 0);
        data.extend_from_slice(&block[..n]);
        assert!(data.len() < 4 * 1024 * 1024);
        if let Some(end) = data.windows(4).position(|s| s == b"\r\n\r\n") {
            let head = std::str::from_utf8(&data[..end]).unwrap();
            assert!(head.contains("Bearer bench-offline-fixture-credential"));
            let size: usize = head
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|s| s.trim().parse().unwrap())
                })
                .unwrap();
            if data.len() >= end + 4 + size {
                return serde_json::from_slice(&data[end + 4..end + 4 + size]).unwrap();
            }
        }
    }
}
struct Chat {
    endpoint: String,
    stop: Arc<AtomicBool>,
    counts: Arc<AtomicUsize>,
    worker: Option<JoinHandle<()>>,
}
impl Chat {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/chat/completions", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let counts = Arc::new(AtomicUsize::new(0));
        let done = stop.clone();
        let seen = counts.clone();
        let worker = thread::spawn(move || {
            let until = Instant::now() + Duration::from_secs(300);
            while !done.load(Ordering::Relaxed) && Instant::now() < until {
                let (mut stream, _) = match listener.accept() {
                    Ok(s) => s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => panic!("fake accept: {e}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let req = request(&mut stream);
                let packet: Value =
                    serde_json::from_str(req["messages"][1]["content"].as_str().unwrap())
                        .unwrap_or(Value::Null);
                let content = if let Some(rubric) = packet["rubric"].as_array() {
                    assert!(packet.get("arm").is_none());
                    assert!(packet.get("run").is_none());
                    json!({"awarded":vec![false;rubric.len()],"explanation":"Offline transport check only.","evidence_ids":[]})
                } else {
                    json!({"answer":"I don't know","evidence_ids":[],"abstain":true})
                };
                let event = json!({"id":format!("fixture-{}",seen.fetch_add(1,Ordering::Relaxed)),"model":"fixture-model","system_fingerprint":"fixture-revision","choices":[{"index":0,"delta":{"role":"assistant","content":content.to_string()},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}});
                let body = format!("data: {event}\n\ndata: [DONE]\n\n");
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(head.as_bytes()).unwrap();
                stream.write_all(body.as_bytes()).unwrap();
            }
        });
        Self {
            endpoint,
            stop,
            counts,
            worker: Some(worker),
        }
    }
}
impl Drop for Chat {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let result = self.worker.take().unwrap().join();
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}
struct Speech {
    endpoint: String,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Speech {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("ws://{}/v1/listen", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let worker = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {let listener=tokio::net::TcpListener::from_std(listener).unwrap();let until=Instant::now()+Duration::from_secs(120);let socket=loop {if done.load(Ordering::Relaxed){return;}assert!(Instant::now()<until,"speech fixture was not contacted");if let Ok(accepted)=tokio::time::timeout(Duration::from_millis(100),listener.accept()).await{break accepted.unwrap().0;}};let mut socket=accept_async(socket).await.unwrap();let mut received=0;while let Ok(Some(Ok(frame)))=tokio::time::timeout(Duration::from_secs(5),socket.next()).await {match frame {Message::Binary(b)=>received+=b.len(),Message::Text(text)=>{let v:Value=serde_json::from_str(&text).unwrap();match v["type"].as_str(){Some("Finalize")=>{assert_eq!(received,1280);let v=json!({"type":"Results","channel_index":[0,1],"start":0.0,"duration":0.04,"is_final":true,"from_finalize":true,"channel":{"alternatives":[{"transcript":"offline observed words"}]},"metadata":{"request_id":"fixture-speech","model_uuid":"fixture-uuid","model_info":{"name":"general","arch":"nova-3","version":"fixture-v1"}}});socket.send(Message::text(v.to_string())).await.unwrap();},Some("CloseStream")=>{socket.send(Message::text(json!({"type":"Metadata","request_id":"fixture-speech","duration":0.04,"channels":1}).to_string())).await.unwrap();socket.close(None).await.unwrap();return;},_=>{}}},_=>{}}}panic!("fake speech did not finish");});
        });
        Self {
            endpoint,
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for Speech {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let result = self.worker.take().unwrap().join();
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}
fn prepare_audio(dir: &Path) -> String {
    fs::DirBuilder::new().mode(0o700).create(dir).unwrap();
    let mut wav = b"RIFF".to_vec();
    wav.extend(1316u32.to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend([1, 0, 1, 0]);
    wav.extend(16000u32.to_le_bytes());
    wav.extend(32000u32.to_le_bytes());
    wav.extend([2, 0, 16, 0]);
    wav.extend(b"data");
    wav.extend(1280u32.to_le_bytes());
    wav.resize(1324, 0);
    fs::write(dir.join("fixture.wav"), &wav).unwrap();
    let mut rows = Vec::new();
    for e in files::read_corpus(&source()).unwrap().events {
        if e.kind != EventKind::Utterance {
            continue;
        }
        let row = speech::Audio {
            record_id: e.record_id,
            script_hash: hash(e.heard.as_bytes()),
            wav_hash: hash(&wav),
            pcm_hash: hash(&wav[44..]),
            file: "fixture.wav".into(),
            duration_ms: 40,
            voice: "offline synthetic audio".into(),
            words_per_minute: 160,
        };
        serde_json::to_writer(&mut rows, &row).unwrap();
        rows.push(b'\n');
    }
    fs::write(dir.join("audio.jsonl"), &rows).unwrap();
    hash(&rows)
}
#[test]
fn fake_services_drive_shared_speech_then_complete_month_with_null_scores() {
    let d = Dir::new();
    let chat = Chat::start();
    let speech = Speech::start();
    let mut p = policy();
    p.chat.endpoint = chat.endpoint.clone();
    p.speech.endpoint = speech.endpoint.clone();
    p.fixtures
        .get_mut("development")
        .unwrap()
        .audio_manifest_sha256 = prepare_audio(&d.0.join("audio"));
    let root = d.0.join("campaign");
    live_run::initialize(&p, approval(&p), &root, true).unwrap();
    let cache = live_run::create_speech(
        &p,
        approval(&p),
        &root,
        "development",
        &source(),
        &d.0.join("audio"),
        true,
    )
    .unwrap();
    assert_eq!(cache["model_calls"], 1);
    let before = campaign::Campaign::open(&root, approval(&p), 1)
        .unwrap()
        .remaining_microusd();
    let result = live_run::execute(&p, approval(&p), &root, &run("none", 1), true).unwrap();
    assert_eq!(result["questions"], 200);
    assert_eq!(result["answer_turns"], 224);
    assert_eq!(result["model_calls"], 300);
    assert_eq!(result["label"], "plumbing check");
    assert_eq!(chat.counts.load(Ordering::Relaxed), 300);
    for kind in result["by_kind"].as_object().unwrap().values() {
        assert!(kind["mean"].is_null());
    }
    let path = root
        .join("runs")
        .join(live_run::run_id("development", "none", 1).unwrap());
    assert_eq!(
        ledger::verify(&path).unwrap().terminal.as_deref(),
        Some("completed")
    );
    assert_eq!(
        fs::read(path.join("source/events.jsonl")).unwrap(),
        fs::read(source().join("events.jsonl")).unwrap()
    );
    let after = campaign::Campaign::open(&root, approval(&p), 1)
        .unwrap()
        .remaining_microusd();
    assert!(after < before);
    assert!(live_run::execute(&p, approval(&p), &root, &run("none", 1), true).is_err());
    assert_eq!(
        campaign::Campaign::open(&root, approval(&p), 1)
            .unwrap()
            .remaining_microusd(),
        after
    );
    assert!(
        live_run::create_speech(
            &p,
            approval(&p),
            &root,
            "development",
            &source(),
            &d.0.join("audio"),
            true
        )
        .is_err()
    );
}
