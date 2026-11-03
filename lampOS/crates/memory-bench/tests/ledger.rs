use lamp_memory_bench::{hash, ledger::*};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "memory-bench-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn manifest() -> Manifest {
    Manifest {
        run_id: "recorded-test".into(),
        mode: Mode::Recorded,
        arm: "none".into(),
        implementation_revision: "a".repeat(40),
        registration_revision: "b".repeat(40),
        registration_hash: hash(b"questions"),
        fixture_hash: hash(b"fixture"),
        transcript_hash: None,
        timezone: "Asia/Ho_Chi_Minh".into(),
        model: "recorded".into(),
        config: json!({}),
        budget_microusd: 100,
        seed: 1,
    }
}
#[test]
fn attempts_are_exclusive_and_interruptions_stay_incomplete() {
    let scratch = Scratch::new();
    let path = scratch.0.join("run");
    let ledger = Ledger::create(&path, &manifest()).unwrap();
    drop(ledger);
    assert!(Ledger::create(&path, &manifest()).is_err());
    let proof = verify(&path).unwrap();
    assert_eq!(proof.entries, 1);
    assert_eq!(proof.terminal, None);
}
#[test]
fn modified_results_and_partial_writes_are_detected() {
    let scratch = Scratch::new();
    let path = scratch.0.join("run");
    let mut ledger = Ledger::create(&path, &manifest()).unwrap();
    ledger.append("score", json!({"correct":false})).unwrap();
    ledger.append("completed", json!({})).unwrap();
    assert!(ledger.append("score", json!({})).is_err());
    drop(ledger);
    assert_eq!(
        verify(&path).unwrap().terminal.as_deref(),
        Some("completed")
    );
    let file = path.join("events.jsonl");
    let original = fs::read_to_string(&file).unwrap();
    fs::write(&file, original.replace("false", "true")).unwrap();
    assert!(verify(&path).is_err());
    fs::write(&file, original.trim_end()).unwrap();
    assert!(verify(&path).is_err());
}
#[test]
fn a_mean_that_parses_one_ulp_off_still_verifies() {
    // Preliminary run 7 on lamp-06c1 (3 Nov): the partial setup's overall mean
    // 0.9008333333333333 read back as ...332 without exact float parsing, so
    // its finished ledger failed its own hash chain.
    let scratch = Scratch::new();
    let path = scratch.0.join("run");
    let mut ledger = Ledger::create(&path, &manifest()).unwrap();
    ledger
        .append("completed", json!({"overall":0.9008333333333333}))
        .unwrap();
    drop(ledger);
    assert_eq!(
        verify(&path).unwrap().terminal.as_deref(),
        Some("completed")
    );
}
#[test]
fn the_chain_hashes_each_row_as_written_not_a_reparse() {
    // Without float_roundtrip (kept off: feature unification would turn it on
    // for every lamp binary) many means parse an ULP off. The chain must hash
    // the row's data bytes as stored, so no parse can move its hash.
    let means: Vec<f64> = (1..=2000)
        .map(|k| f64::from(k) / 1200.0 + 0.9008333333333333 / f64::from(k))
        .collect();
    let scratch = Scratch::new();
    let path = scratch.0.join("run");
    let mut ledger = Ledger::create(&path, &manifest()).unwrap();
    ledger.append("score", json!({"means":means})).unwrap();
    ledger
        .append("completed", json!({"overall":0.9008333333333333}))
        .unwrap();
    drop(ledger);
    assert_eq!(verify(&path).unwrap().entries, 3);
    let rows = fs::read_to_string(path.join("events.jsonl")).unwrap();
    for row in rows.lines() {
        let entry: serde_json::Value = serde_json::from_str(row).unwrap();
        let data = &row[row.find(",\"data\":").unwrap() + 8..row.rfind(",\"hash\":").unwrap()];
        let written = format!(
            "[{},{},{},{data}]",
            entry["sequence"], entry["previous_hash"], entry["kind"]
        );
        assert_eq!(entry["hash"].as_str().unwrap(), hash(written.as_bytes()));
    }
}
