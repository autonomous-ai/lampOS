use lamp_memory_tiers::*;
use serde_json::json;
use std::time::{Duration, Instant};
const NOW: i64 = DAY_MS * 100;
fn seed() -> Store {
    let mut s = Store::in_memory("fictional-owner").unwrap();
    s.ingest(Incoming {
        text: "Maybe a blue notebook helps my sketching.".into(),
        speaker: Speaker::Owner,
        at_ms: NOW,
        stt_confidence: None,
        strength: Strength::Said,
        addressed_to_lamp: true,
        topic: "sketching".into(),
        remember: false,
    })
    .unwrap();
    s.owner_left(NOW + 1).unwrap();
    let input = s
        .prepare_sleep(NOW + 10, PassKind::Nightly)
        .unwrap()
        .unwrap();
    s.apply_sleep(&input,&json!({"facts":[{"key":"blue","claim":"A blue notebook may help with sketching.","record_ids":[1],"strength":"said","kind":"preference","true_since_ms":NOW,"true_until_ms":null,"supersedes":null}],"lines":[{"replaces":null,"kind":"owner_model","text":"May prefer a blue notebook for sketching.","fact_ids":[],"fact_keys":["blue"]},{"replaces":null,"kind":"chapter","text":"Exploring sketching at a desk.","fact_ids":[],"fact_keys":["blue"]}],"episodes":[{"episode_id":1,"record_ids":[1],"gist":"A possible sketching aid.","importance":0.1}]}).to_string()).unwrap();
    s
}
fn query() -> RecallQuery {
    RecallQuery {
        access: Access::OWNER_ALONE,
        now_ms: NOW + 100,
        words: "sketching".into(),
        cues: Cues::default(),
        max_bytes: 16384,
        budget: RECALL_BUDGET,
        arm: Arm::Tiers,
    }
}
#[test]
fn recall_preserves_uncertainty_citations_and_separate_snapshot() {
    let s = seed();
    let q = query();
    let result = s.recall(&q).unwrap();
    assert!(!result.items.is_empty());
    assert_eq!(result.source_ids(), vec![1]);
    assert!(result.items.iter().all(|i| i.strength == Strength::Maybe));
    assert!(result.items.iter().all(|i| i.kind != "owner_model"));
    let snapshot = s.snapshot(&q).unwrap();
    assert_eq!(snapshot.items.len(), 1);
    assert_eq!(snapshot.items[0].kind, "owner_model");
    assert_eq!(snapshot.source_ids(), vec![1]);
}
#[test]
fn access_future_budget_and_byte_limits_are_fail_closed() {
    let s = seed();
    let mut q = query();
    q.access.alone = false;
    assert!(s.recall(&q).unwrap().items.is_empty());
    assert!(s.snapshot(&q).unwrap().items.is_empty());
    q.access = Access::OWNER_ALONE;
    q.now_ms = NOW - 1;
    assert!(s.recall(&q).unwrap().items.is_empty());
    q.now_ms = NOW + 100;
    q.max_bytes = MEMORY_NOTICE.len();
    assert!(s.recall(&q).unwrap().items.is_empty());
    q.max_bytes = 260;
    let r = s.recall(&q).unwrap();
    assert_eq!(r.bytes, r.context().len() + r.instructions.len());
    assert!(r.bytes <= q.max_bytes);
    q.budget = Duration::ZERO;
    let r = s.recall(&q).unwrap();
    assert!(r.timed_out);
    assert!(r.items.is_empty());
}
#[test]
fn facts_removal_keeps_proof_for_other_components_and_forgetting() {
    let mut s = seed();
    let mut q = query();
    q.arm = Arm::TiersMinusFacts;
    let r = s.recall(&q).unwrap();
    assert!(
        r.items
            .iter()
            .all(|i| !matches!(i.kind.as_str(), "fact" | "person" | "place"))
    );
    assert_eq!(s.snapshot(&q).unwrap().items.len(), 1);
    assert_eq!(s.facts(Access::OWNER_ALONE).unwrap().len(), 1);
    s.forget_records(&[1]).unwrap();
    assert!(s.recall(&q).unwrap().items.is_empty());
    assert!(s.snapshot(&q).unwrap().items.is_empty());
    assert_eq!(
        q.arm.description(),
        "facts not recalled; kept only as evidence"
    );
}
#[test]
fn fading_does_not_reconstruct_erased_detail() {
    let mut s = seed();
    s.maintain(NOW + DAY_MS * 91, Arm::Tiers).unwrap();
    let mut q = query();
    q.now_ms = NOW + DAY_MS * 91;
    let r = s.recall(&q).unwrap();
    let ep = r.items.iter().find(|i| i.kind == "episode").unwrap();
    assert!(!ep.text.contains("blue notebook"));
    assert!(ep.text.contains("possible sketching aid"));
    let mut s = seed();
    s.maintain(NOW + DAY_MS * 91, Arm::TiersMinusFading)
        .unwrap();
    assert!(
        !s.episodes(Access::OWNER_ALONE).unwrap()[0]
            .detail
            .is_empty()
    );
}
#[test]
fn all_registered_policies_are_unique_and_filter_exact_components() {
    let names: std::collections::BTreeSet<_> = Arm::ALL.iter().map(|a| a.name()).collect();
    assert_eq!(names.len(), 11);
    for arm in Arm::ALL {
        assert_eq!(Arm::parse(&arm.name()).unwrap(), arm);
        let mut q = query();
        q.arm = arm;
        let s = seed();
        let r = s.recall(&q).unwrap();
        if arm == Arm::TiersMinusEpisodes {
            assert!(r.items.iter().all(|i| i.kind != "episode"));
        }
        if arm == Arm::TiersMinusLifeStory {
            assert!(
                r.items
                    .iter()
                    .all(|i| i.kind != "chapter" && i.kind != "event")
            );
        }
        if arm == Arm::TiersMinusOwnerModel {
            assert!(s.snapshot(&q).unwrap().items.is_empty());
        }
    }
}
#[test]
fn cold_and_repeated_lookup_at_month_scale() {
    let mut s = seed();
    for i in 0..3_000 {
        s.ingest(Incoming {
            text: format!("Routine observed note number {i}."),
            speaker: Speaker::Owner,
            at_ms: NOW + 20,
            stt_confidence: None,
            strength: Strength::Said,
            addressed_to_lamp: true,
            topic: format!("topic {}", i / 20),
            remember: false,
        })
        .unwrap();
    }
    let q = query();
    let start = Instant::now();
    let first = s.recall(&q).unwrap();
    let cold = start.elapsed();
    let mut max = Duration::ZERO;
    for _ in 0..20 {
        let start = Instant::now();
        let r = s.recall(&q).unwrap();
        max = max.max(start.elapsed());
        assert!(!r.timed_out);
    }
    eprintln!(
        "recall month-scale: cold_us={}, repeated_max_us={}",
        cold.as_micros(),
        max.as_micros()
    );
    assert!(!first.timed_out);
    assert!(!first.items.is_empty());
    assert!(cold < RECALL_BUDGET);
    assert!(max < RECALL_BUDGET);
}
#[cfg(feature = "bench")]
#[test]
fn isolated_child_restores_fingerprints_and_cursors_by_discarding_only_child() {
    let mut main = seed();
    let mut child = main.bench_fork().unwrap();
    child.forget_records(&[1]).unwrap();
    assert!(child.records(Access::OWNER_ALONE).unwrap().is_empty());
    assert_eq!(main.records(Access::OWNER_ALONE).unwrap().len(), 1);
    drop(child);
    assert_eq!(main.snapshot(&query()).unwrap().items.len(), 1);
    main.forget_records(&[1]).unwrap();
    let child = main.bench_fork().unwrap();
    assert!(child.snapshot(&query()).unwrap().items.is_empty());
}

#[test]
fn episode_cites_the_exact_later_batch_and_never_returns_future_detail() {
    let mut s = Store::in_memory("fictional-owner").unwrap();
    for i in 0..129 {
        s.ingest(Incoming {
            text: if i == 128 {
                "Maybe coral paint is suitable.".into()
            } else {
                format!("Plain earlier line {i}.")
            },
            speaker: Speaker::Owner,
            at_ms: NOW + i,
            stt_confidence: None,
            strength: Strength::Said,
            addressed_to_lamp: true,
            topic: "paint".into(),
            remember: false,
        })
        .unwrap();
    }
    let batch = s
        .prepare_sleep(NOW + 200, PassKind::Nightly)
        .unwrap()
        .unwrap();
    s.apply_sleep(&batch, r#"{"facts":[],"lines":[],"episodes":[]}"#)
        .unwrap();
    let batch = s
        .prepare_sleep(NOW + 200, PassKind::Nightly)
        .unwrap()
        .unwrap();
    assert_eq!(batch.records()[0].id, 129);
    s.apply_sleep(&batch,&json!({"facts":[],"lines":[],"episodes":[{"episode_id":1,"record_ids":[129],"gist":"A tentative coral-paint choice.","importance":0.2}]}).to_string()).unwrap();
    let mut q = query();
    q.now_ms = NOW + 200;
    q.words = "coral".into();
    let result = s.recall(&q).unwrap();
    let ep = result.items.iter().find(|i| i.kind == "episode").unwrap();
    assert_eq!(ep.source_ids, vec![129]);
    assert_eq!(ep.strength, Strength::Maybe);
    q.now_ms = NOW + 10;
    assert!(
        s.recall(&q)
            .unwrap()
            .items
            .iter()
            .all(|i| !i.text.contains("coral"))
    );
}

#[test]
fn cue_terms_survive_a_long_question_and_excluded_raw_hits() {
    let mut s = seed();
    for i in 0..170 {
        let id = s
            .ingest(Incoming {
                text: format!("sketching sketching sketching noisy source {i}"),
                speaker: Speaker::Owner,
                at_ms: NOW + 20,
                stt_confidence: None,
                strength: Strength::Said,
                addressed_to_lamp: true,
                topic: format!("noise {i}"),
                remember: false,
            })
            .unwrap()
            .unwrap();
        if i < 20 {
            s.remember(id, FactKind::Statement, None).unwrap();
        }
    }
    let mut q = query();
    q.arm = Arm::TiersMinusEpisodes;
    q.words = "unrelated ".repeat(40);
    q.cues.objects = vec!["blue".into(), "notebook".into()];
    let result = s.recall(&q).unwrap();
    assert!(
        result
            .items
            .iter()
            .any(|i| i.text.contains("A blue notebook may help"))
    );
}

#[test]
fn storage_contention_is_a_failure_not_absent_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("memory.sqlite");
    let store = Store::open(&path, "fictional-owner").unwrap();
    let blocker = rusqlite::Connection::open(&path).unwrap();
    blocker.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let start = Instant::now();
    assert!(store.recall(&query()).is_err());
    assert!(start.elapsed() < RECALL_BUDGET);
    blocker.execute_batch("ROLLBACK").unwrap();
    assert!(store.recall(&query()).is_ok());
}

#[test]
fn manifest_freezes_all_arms_before_scores() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../arm-manifest.json")).unwrap();
    assert_eq!(manifest["repetitions"], 3);
    assert_eq!(manifest["protocol_version"], 2);
    assert_eq!(manifest["arms"].as_array().unwrap().len(), Arm::ALL.len());
    for (i, arm) in Arm::ALL.iter().enumerate() {
        assert_eq!(manifest["arms"][i]["name"], arm.name());
        assert_eq!(manifest["arms"][i]["label"], arm.description());
    }
}
