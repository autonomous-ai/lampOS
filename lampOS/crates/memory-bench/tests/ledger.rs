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
