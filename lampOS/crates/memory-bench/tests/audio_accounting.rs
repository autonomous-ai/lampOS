use lamp_memory_bench::{
    audio, hash,
    model::{Call, Client, Prices, Provenance, Provider, Purpose, Reply},
    model_worker::Worker,
    speech,
};
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
fn wav(samples: usize) -> Vec<u8> {
    let size = samples * 2;
    let mut bytes = b"RIFF".to_vec();
    bytes.extend(((size + 36) as u32).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend([1, 0, 1, 0]);
    bytes.extend(16000u32.to_le_bytes());
    bytes.extend(32000u32.to_le_bytes());
    bytes.extend([2, 0, 16, 0]);
    bytes.extend(b"data");
    bytes.extend((size as u32).to_le_bytes());
    bytes.resize(size + 44, 0);
    bytes
}
fn prices() -> Prices {
    Prices {
        source: "offline".into(),
        date: "2026-10-31".into(),
        input_microusd_per_million: 1_000_000,
        output_microusd_per_million: 1_000_000,
    }
}
fn speech_prices() -> audio::Prices {
    audio::Prices {
        source: "offline duration fixture".into(),
        date: "2026-10-31".into(),
        microusd_per_minute: 9_000,
    }
}
struct Fake {
    duration: Option<u64>,
    complete: bool,
    evidence: bool,
    mismatch: bool,
    error: bool,
    seen: Arc<AtomicUsize>,
    provenance: Option<Provenance>,
}
impl Fake {
    fn new(duration: Option<u64>) -> Self {
        Self {
            duration,
            complete: true,
            evidence: true,
            mismatch: false,
            error: false,
            seen: Arc::new(AtomicUsize::new(0)),
            provenance: None,
        }
    }
}
impl Provider for Fake {
    fn invoke(&mut self, call: &Call, _deadline: Instant) -> lamp_memory_bench::Result<Reply> {
        self.seen.fetch_add(1, Ordering::SeqCst);
        if let Call::Transcribe { audio } = call {
            assert_eq!(audio.pcm().len(), 32_000);
            self.provenance = Some(Provenance {
                transport: "offline_speech_fixture".into(),
                requested_model: "fixture".into(),
                reported_model: Some("fixture".into()),
                provider_request_id: Some("fixture-request".into()),
                system_fingerprint: None,
                finish_reason: None,
                http_status: Some(101),
                request_sha256: call.request_hash()?,
                response_sha256: hash(b"fake response"),
                artifact: "fixture".into(),
                response_complete: self.complete,
                evidence_complete: self.evidence,
                usage: None,
                audio: Some(audio::Evidence {
                    pcm_sha256: if self.mismatch {
                        hash(b"wrong PCM")
                    } else {
                        audio.pcm_sha256().into()
                    },
                    samples_sent: audio.samples(),
                    transcript_sha256: hash(b"heard words"),
                    confidence: None,
                    model_revision: Some("fixture-v1".into()),
                    model_uuid: None,
                    duration_usage: self
                        .duration
                        .map(|audio_micros| audio::Usage { audio_micros }),
                }),
            });
            if self.error {
                return Err(lamp_memory_bench::invalid("fake speech failure"));
            }
            Ok(Reply {
                content: Some("heard words".into()),
                ..Reply::default()
            })
        } else {
            Ok(Reply {
                content: Some("answer".into()),
                usage: Some(lamp_memory_bench::model::Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                }),
                ..Reply::default()
            })
        }
    }
    fn take_provenance(&mut self) -> Option<Provenance> {
        self.provenance.take()
    }
}
fn call() -> Call {
    Call::Transcribe {
        audio: audio::Input::from_wav(&wav(16_000)).unwrap(),
    }
}
fn client(fake: Fake, limit: u64) -> Client<Fake> {
    Client::new(fake, prices(), limit)
        .unwrap()
        .with_speech(speech_prices())
        .unwrap()
}
#[test]
fn verified_samples_define_the_identity_and_never_cross_the_arm_wire() {
    let w = wav(16_000);
    let mut changed = w.clone();
    changed[44] = 1;
    let input = audio::Input::from_wav(&w).unwrap();
    assert_eq!(input.samples(), 16_000);
    assert_eq!(input.reserve_micros(), 1_000_000);
    assert_eq!(input.wav_sha256(), hash(&w));
    let request = Call::Transcribe { audio: input };
    let wire = serde_json::to_value(&request).unwrap();
    assert!(wire["audio"].get("pcm").is_none());
    assert!(serde_json::from_value::<Call>(wire).is_err());
    assert!(!request.arm_permitted());
    assert_ne!(
        request.request_hash().unwrap(),
        Call::Transcribe {
            audio: audio::Input::from_wav(&changed).unwrap()
        }
        .request_hash()
        .unwrap()
    );
}
#[test]
fn pcm_accepts_the_unchanged_aging_line_and_has_a_strict_120_second_bound() {
    assert!(speech::pcm(&wav(16_000 * 67)).is_ok());
    assert!(speech::pcm(&wav(16_000 * 120)).is_ok());
    assert!(speech::pcm(&wav(16_000 * 120 + 1)).is_err());
    let w = wav(16_000 * 120);
    assert!(speech::pcm(&w[..w.len() - 1]).is_err());
    let input = audio::Input::from_wav(&wav(1)).unwrap();
    assert_eq!(input.reserve_micros(), 1000);
}
#[test]
fn duration_prices_use_integer_microseconds_and_no_minute_blocks() {
    let p = speech_prices();
    assert_eq!(p.charge(1_000_000).unwrap(), 150);
    assert_eq!(p.charge(30_000_000).unwrap(), 4500);
    assert_eq!(p.charge(60_000_000).unwrap(), 9000);
    assert_eq!(p.charge(1).unwrap(), 1);
    assert!(
        audio::Prices {
            microusd_per_minute: 0,
            ..p
        }
        .validate()
        .is_err()
    );
}
#[test]
fn speech_pricing_and_budget_are_checked_before_provider_dispatch() {
    let fake = Fake::new(Some(1_000_000));
    let seen = fake.seen.clone();
    let mut missing = Client::new(fake, prices(), 1000).unwrap();
    assert!(
        missing
            .call(&call(), Instant::now() + Duration::from_secs(1))
            .is_err()
    );
    assert_eq!(seen.load(Ordering::SeqCst), 0);
    let fake = Fake::new(Some(1_000_000));
    let seen = fake.seen.clone();
    let mut low = client(fake, 149);
    assert!(
        low.call(&call(), Instant::now() + Duration::from_secs(1))
            .is_err()
    );
    assert_eq!(seen.load(Ordering::SeqCst), 0);
    assert!(low.receipts().is_empty());
}
#[test]
fn completed_duration_reconciles_but_absent_or_incomplete_usage_keeps_reserve() {
    for (duration, complete, expected) in [
        (Some(500_000), true, 75),
        (None, true, 150),
        (Some(500_000), false, 150),
    ] {
        let mut fake = Fake::new(duration);
        fake.complete = complete;
        let mut c = client(fake, 1000);
        c.call(&call(), Instant::now() + Duration::from_secs(1))
            .unwrap();
        assert_eq!(c.charged_microusd(), expected);
        let r = &c.receipts()[0];
        assert!(r.usage.is_none());
        assert_eq!(r.cost_basis, "conservative_audio_duration_debit");
        assert_eq!(r.audio_usage.is_some(), duration.is_some() && complete);
    }
}
#[test]
fn duration_overrun_is_charged_and_stops_before_another_dispatch() {
    let fake = Fake::new(Some(2_000_000));
    let seen = fake.seen.clone();
    let mut c = client(fake, 1000);
    assert!(
        c.call(&call(), Instant::now() + Duration::from_secs(1))
            .is_err()
    );
    assert_eq!(c.charged_microusd(), 300);
    assert!(c.stopped());
    assert!(
        c.call(&call(), Instant::now() + Duration::from_secs(1))
            .is_err()
    );
    assert_eq!(seen.load(Ordering::SeqCst), 1);
}
#[test]
fn failed_audio_evidence_retains_reservation_or_higher_known_charge() {
    for (duration, mismatch) in [(500_000, false), (2_000_000, false), (2_000_000, true)] {
        let mut fake = Fake::new(Some(duration));
        fake.evidence = mismatch;
        fake.mismatch = mismatch;
        fake.error = true;
        let mut c = client(fake, 1000);
        assert!(
            c.call(&call(), Instant::now() + Duration::from_secs(1))
                .is_err()
        );
        assert!(c.stopped());
        assert_eq!(
            c.charged_microusd(),
            speech_prices().charge(duration).unwrap().max(150)
        );
    }
}
#[test]
fn duration_and_chat_share_the_same_worker_and_total_budget() {
    let mut worker = Worker::start(client(Fake::new(Some(1_000_000)), 2000)).unwrap();
    worker
        .call(&call(), Instant::now() + Duration::from_secs(1))
        .unwrap();
    let chat = Call::Chat {
        purpose: Purpose::Answer,
        messages: vec![json!({"role":"user","content":"hi"})],
        max_output_tokens: 10,
        response_format: None,
        tools: vec![],
        tool_choice: "none".into(),
    };
    worker
        .call(&chat, Instant::now() + Duration::from_secs(1))
        .unwrap();
    let account = worker
        .finish(Instant::now() + Duration::from_secs(1))
        .unwrap();
    assert_eq!(account.charged_microusd, 152);
    assert_eq!(account.receipts.len(), 2);
    assert!(account.receipts[1].audio_usage.is_none());
}
#[test]
fn duration_receipts_replay_in_the_same_campaign_after_restart() {
    use lamp_memory_bench::campaign::{Campaign, Plan};
    let root = std::env::temp_dir().join(format!("bench-audio-campaign-{}", std::process::id()));
    let plan = Plan {
        approved_cap_microusd: 1000,
        policy_sha256: hash(b"audio fixture policy"),
    };
    let mut campaign = Campaign::create(&root, plan.clone(), 1000).unwrap();
    let mut c = client(Fake::new(Some(500_000)), 1000)
        .with_audit(move |stage, receipt| campaign.audit("speech", stage, receipt));
    c.call(&call(), Instant::now() + Duration::from_secs(3))
        .unwrap();
    drop(c);
    let reopened = Campaign::open(&root, plan, 925).unwrap();
    assert_eq!(reopened.charged_microusd(), 75);
    drop(reopened);
    std::fs::remove_dir_all(root).unwrap();
}

struct Tweaked {
    inner: Fake,
    change: &'static str,
}
impl Provider for Tweaked {
    fn invoke(&mut self, request: &Call, deadline: Instant) -> lamp_memory_bench::Result<Reply> {
        let audio_request = call();
        let forwarded = if self.change == "audio-on-chat" {
            &audio_request
        } else {
            request
        };
        let mut reply = self.inner.invoke(forwarded, deadline)?;
        let provenance = self.inner.provenance.as_mut().unwrap();
        match self.change {
            "tokens-in-reply" => {
                reply.usage = Some(lamp_memory_bench::model::Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                })
            }
            "tokens-in-provenance" => {
                provenance.usage = Some(lamp_memory_bench::model::Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                })
            }
            "transcript" => {
                provenance.audio.as_mut().unwrap().transcript_sha256 = hash(b"different text")
            }
            "confidence" => provenance.audio.as_mut().unwrap().confidence = Some(f64::NAN),
            "audio-on-chat" => {
                provenance.response_complete = false;
                provenance.audio.as_mut().unwrap().duration_usage = None;
                reply = Reply {
                    content: Some("chat answer".into()),
                    usage: Some(lamp_memory_bench::model::Usage {
                        input_tokens: 1,
                        output_tokens: 1,
                    }),
                    ..Reply::default()
                };
            }
            _ => unreachable!(),
        }
        Ok(reply)
    }
    fn take_provenance(&mut self) -> Option<Provenance> {
        self.inner.take_provenance()
    }
}
#[test]
fn mixed_billing_and_transcript_mismatches_are_fatal_across_campaign_restart() {
    use lamp_memory_bench::campaign::{Campaign, Plan};
    for change in [
        "tokens-in-reply",
        "tokens-in-provenance",
        "transcript",
        "confidence",
        "audio-on-chat",
    ] {
        let root = std::env::temp_dir().join(format!(
            "bench-audio-mismatch-{}-{change}",
            std::process::id()
        ));
        let plan = Plan {
            approved_cap_microusd: 2000,
            policy_sha256: hash(b"audio mismatch policy"),
        };
        let mut campaign = Campaign::create(&root, plan.clone(), 2000).unwrap();
        let mut client = Client::new(
            Tweaked {
                inner: Fake::new(Some(500_000)),
                change,
            },
            prices(),
            2000,
        )
        .unwrap()
        .with_speech(speech_prices())
        .unwrap()
        .with_audit(move |stage, receipt| campaign.audit("mismatch", stage, receipt));
        let request = if change == "audio-on-chat" {
            Call::Chat {
                purpose: Purpose::Answer,
                messages: vec![json!({"role":"user","content":"hi"})],
                max_output_tokens: 10,
                response_format: None,
                tools: vec![],
                tool_choice: "none".into(),
            }
        } else {
            call()
        };
        assert!(
            client
                .call(&request, Instant::now() + Duration::from_secs(5))
                .is_err()
        );
        assert!(client.stopped());
        assert!(client.receipts()[0].charged_microusd >= client.receipts()[0].reserved_microusd);
        drop(client);
        assert!(Campaign::open(&root, plan, 1).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
