use lamp_memory_bench::{
    Result,
    campaign::{Campaign, Plan},
    hash, invalid,
    model::{Call, Client, Prices, Provider, Purpose, Receipt, Reply, Usage},
};
use serde_json::json;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "memory-campaign-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn plan(cap: u64) -> Plan {
    Plan {
        approved_cap_microusd: cap,
        policy_sha256: hash(b"offline fixture policy"),
    }
}
fn reservation(number: u64, amount: u64) -> Receipt {
    Receipt {
        provenance: None,
        audio_usage: None,
        call_number: number,
        request_hash: hash(format!("request-{number}").as_bytes()),
        reserved_microusd: amount,
        charged_microusd: amount,
        request_bytes: 100,
        outcome: "uncertain".into(),
        usage: None,
        cost_basis: "conservative_token_debit".into(),
    }
}
#[test]
fn restart_retains_unsettled_spending_and_refuses_a_larger_plan() {
    let temp = Temp::new();
    let mut c = Campaign::create(&temp.0, plan(100), 100).unwrap();
    c.audit("run-one", "model_reserved", &reservation(1, 70))
        .unwrap();
    drop(c);
    assert!(Campaign::open(&temp.0, plan(100), 31).is_err());
    let mut c = Campaign::open(&temp.0, plan(100), 30).unwrap();
    assert_eq!(c.charged_microusd(), 70);
    assert!(
        c.audit("run-two", "model_reserved", &reservation(1, 31))
            .is_err()
    );
    c.audit("run-two", "model_reserved", &reservation(1, 30))
        .unwrap();
    assert_eq!(c.remaining_microusd(), 0);
    assert!(c.check_plan(1).is_err());
}
#[test]
fn an_exclusive_handle_blocks_another_runner_and_the_cap_cannot_change() {
    let temp = Temp::new();
    let c = Campaign::create(&temp.0, plan(100), 10).unwrap();
    assert!(Campaign::open(&temp.0, plan(100), 10).is_err());
    drop(c);
    assert!(Campaign::open(&temp.0, plan(200), 10).is_err());
    let c = Campaign::open(&temp.0, plan(100), 10).unwrap();
    assert_eq!(c.charged_microusd(), 0);
    let untouched = Temp::new();
    assert!(Campaign::create(&untouched.0, plan(100), 101).is_err());
    assert!(!untouched.0.exists());
}
#[test]
fn known_usage_settles_once_and_unknown_usage_cannot_refund() {
    let temp = Temp::new();
    let mut c = Campaign::create(&temp.0, plan(100), 100).unwrap();
    let mut receipt = reservation(1, 70);
    c.audit("run-one", "model_reserved", &receipt).unwrap();
    receipt.charged_microusd = 20;
    assert!(c.audit("run-one", "model_completed", &receipt).is_err());
    assert_eq!(c.charged_microusd(), 70);
    receipt.usage = Some(Usage {
        input_tokens: 10,
        output_tokens: 10,
    });
    receipt.outcome = "accounted".into();
    c.audit("run-one", "model_completed", &receipt).unwrap();
    assert_eq!(c.charged_microusd(), 20);
    assert!(c.audit("run-one", "model_completed", &receipt).is_err());
    drop(c);
    let c = Campaign::open(&temp.0, plan(100), 80).unwrap();
    assert_eq!(c.charged_microusd(), 20);
}
#[test]
fn a_partial_journal_is_not_repaired_or_restarted_for_free() {
    let temp = Temp::new();
    let mut c = Campaign::create(&temp.0, plan(100), 100).unwrap();
    c.audit("run-one", "model_reserved", &reservation(1, 70))
        .unwrap();
    drop(c);
    let path = temp.0.join("costs.jsonl");
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"{partial")
        .unwrap();
    let before = fs::read(&path).unwrap();
    assert!(Campaign::open(&temp.0, plan(100), 1).is_err());
    assert_eq!(fs::read(path).unwrap(), before);
}
#[test]
fn an_over_reservation_charge_is_recorded_then_stops_all_new_work() {
    let temp = Temp::new();
    let mut c = Campaign::create(&temp.0, plan(100), 100).unwrap();
    let mut receipt = reservation(1, 70);
    c.audit("run-one", "model_reserved", &receipt).unwrap();
    receipt.charged_microusd = 110;
    receipt.usage = Some(Usage {
        input_tokens: 100,
        output_tokens: 10,
    });
    receipt.outcome = "reservation_exceeded".into();
    assert!(c.audit("run-one", "model_completed", &receipt).is_err());
    assert_eq!(c.charged_microusd(), 110);
    assert!(
        c.audit("run-two", "model_reserved", &reservation(1, 1))
            .is_err()
    );
    drop(c);
    assert!(Campaign::open(&temp.0, plan(100), 1).is_err());
    assert!(
        fs::read_to_string(temp.0.join("costs.jsonl"))
            .unwrap()
            .contains("\"charged\":110")
    );
}
struct Count(Arc<AtomicUsize>);
impl Provider for Count {
    fn invoke(&mut self, _: &Call, _: Instant) -> Result<Reply> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(invalid("offline fake provider interrupted"))
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
fn call() -> Call {
    Call::Chat {
        purpose: Purpose::Answer,
        messages: vec![json!({"role":"user","content":"hello"})],
        max_output_tokens: 4,
        response_format: None,
        tools: vec![],
        tool_choice: "none".into(),
    }
}
#[test]
fn global_budget_refusal_prevents_provider_dispatch_even_with_a_larger_run_budget() {
    let temp = Temp::new();
    let mut campaign = Campaign::create(&temp.0, plan(1), 1).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut client = Client::new(Count(calls.clone()), prices(), 10_000)
        .unwrap()
        .with_audit(move |stage, receipt| campaign.audit("run-one", stage, receipt));
    assert!(
        client
            .call(&call(), Instant::now() + Duration::from_secs(1))
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(client.stopped());
}
#[test]
fn failed_transport_and_failed_run_audit_both_retain_the_reservation() {
    for fail_run_audit in [false, true] {
        let temp = Temp::new();
        let campaign = Arc::new(Mutex::new(
            Campaign::create(&temp.0, plan(10_000), 10_000).unwrap(),
        ));
        let captured = campaign.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut client = Client::new(Count(calls.clone()), prices(), 10_000)
            .unwrap()
            .with_audit(move |stage, receipt| {
                captured.lock().unwrap().audit("run-one", stage, receipt)?;
                if fail_run_audit {
                    return Err(invalid("offline failed run-ledger write"));
                }
                Ok(())
            });
        assert!(
            client
                .call(&call(), Instant::now() + Duration::from_secs(1))
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), usize::from(!fail_run_audit));
        let charged = campaign.lock().unwrap().charged_microusd();
        assert!(charged > 0);
        drop(client);
        drop(campaign);
        let c = Campaign::open(&temp.0, plan(10_000), 1).unwrap();
        assert_eq!(c.charged_microusd(), charged);
    }
}
#[test]
fn only_an_explicit_pre_dispatch_expiry_can_refund_without_usage() {
    let temp = Temp::new();
    let mut c = Campaign::create(&temp.0, plan(100), 100).unwrap();
    let mut receipt = reservation(1, 70);
    c.audit("run-one", "model_reserved", &receipt).unwrap();
    receipt.charged_microusd = 0;
    receipt.outcome = "expired_before_dispatch".into();
    c.audit("run-one", "model_completed", &receipt).unwrap();
    assert_eq!(c.remaining_microusd(), 100);
    assert!(
        c.audit("run-one", "model_reserved", &reservation(1, 1))
            .is_err()
    );
}

struct KnownReply {
    output_tokens: u64,
}
impl Provider for KnownReply {
    fn invoke(&mut self, _: &Call, _: Instant) -> Result<Reply> {
        Ok(Reply {
            content: Some("yes".into()),
            usage: Some(Usage {
                input_tokens: 1,
                output_tokens: self.output_tokens,
            }),
            ..Reply::default()
        })
    }
}
#[test]
fn a_token_bound_failure_remains_fatal_even_when_the_charge_is_small() {
    let temp = Temp::new();
    let campaign = Arc::new(Mutex::new(
        Campaign::create(&temp.0, plan(10_000), 10_000).unwrap(),
    ));
    let captured = campaign.clone();
    let mut client = Client::new(KnownReply { output_tokens: 5 }, prices(), 10_000)
        .unwrap()
        .with_audit(move |stage, receipt| {
            captured.lock().unwrap().audit("run-one", stage, receipt)
        });
    assert!(
        client
            .call(&call(), Instant::now() + Duration::from_secs(1))
            .is_err()
    );
    assert!(client.stopped());
    let c = campaign.lock().unwrap();
    assert_eq!(c.charged_microusd(), 6);
    assert!(c.check_plan(1).is_err());
    drop(c);
    drop(client);
    drop(campaign);
    assert!(Campaign::open(&temp.0, plan(10_000), 1).is_err());
}
#[test]
fn a_completion_only_run_log_failure_does_not_erase_known_usage() {
    let temp = Temp::new();
    let campaign = Arc::new(Mutex::new(
        Campaign::create(&temp.0, plan(10_000), 10_000).unwrap(),
    ));
    let captured = campaign.clone();
    let mut client = Client::new(KnownReply { output_tokens: 2 }, prices(), 10_000)
        .unwrap()
        .with_audit(move |stage, receipt| {
            captured.lock().unwrap().audit("run-one", stage, receipt)?;
            if stage == "model_completed" {
                return Err(invalid("offline completion log failure"));
            }
            Ok(())
        });
    assert!(
        client
            .call(&call(), Instant::now() + Duration::from_secs(1))
            .is_err()
    );
    assert!(client.stopped());
    assert_eq!(campaign.lock().unwrap().charged_microusd(), 3);
    drop(client);
    drop(campaign);
    assert_eq!(
        Campaign::open(&temp.0, plan(10_000), 1)
            .unwrap()
            .charged_microusd(),
        3
    );
}

#[test]
fn reservation_binds_usage_kind_and_wrong_kind_cannot_refund_after_restart() {
    for audio in [false, true] {
        let temp = Temp::new();
        let mut c = Campaign::create(&temp.0, plan(100), 100).unwrap();
        let mut r = reservation(1, 70);
        if audio {
            r.cost_basis = "conservative_audio_duration_debit".into();
        }
        c.audit("typed", "model_reserved", &r).unwrap();
        drop(c);
        let journal = std::fs::read_to_string(temp.0.join("costs.jsonl")).unwrap();
        assert_eq!(journal.contains("\"billing\":\"audio\""), audio);
        let mut c = Campaign::open(&temp.0, plan(100), 30).unwrap();
        r.charged_microusd = 1;
        r.outcome = "accounted".into();
        if audio {
            r.usage = Some(lamp_memory_bench::model::Usage {
                input_tokens: 1,
                output_tokens: 0,
            });
        } else {
            r.audio_usage = Some(lamp_memory_bench::audio::Usage { audio_micros: 1 });
        }
        assert!(c.audit("typed", "model_completed", &r).is_err());
        assert_eq!(c.charged_microusd(), 70);
        drop(c);
        assert!(Campaign::open(&temp.0, plan(100), 1).is_err());
    }
}

#[test]
fn stopped_or_exhausted_campaign_can_be_reported_without_granting_more_spending() {
    let temp = Temp::new();
    let mut c = Campaign::create(&temp.0, plan(100), 100).unwrap();
    let mut r = reservation(1, 70);
    c.audit("one", "model_reserved", &r).unwrap();
    r.charged_microusd = 110;
    r.usage = Some(Usage {
        input_tokens: 100,
        output_tokens: 10,
    });
    r.outcome = "reservation_exceeded".into();
    assert!(c.audit("one", "model_completed", &r).is_err());
    drop(c);
    let before = fs::read(temp.0.join("costs.jsonl")).unwrap();
    let mut c = Campaign::inspect(&temp.0, plan(100)).unwrap();
    assert!(c.stopped());
    assert_eq!(c.charged_microusd(), 110);
    assert_eq!(c.run_accounting("one").calls, 1);
    assert_eq!(c.run_accounting("one").settled_calls, 1);
    assert_eq!(c.run_accounting("one").charged_microusd, 110);
    assert!(c.check_plan(1).is_err());
    assert!(
        c.audit("two", "model_reserved", &reservation(1, 1))
            .is_err()
    );
    drop(c);
    assert_eq!(before, fs::read(temp.0.join("costs.jsonl")).unwrap());
}
