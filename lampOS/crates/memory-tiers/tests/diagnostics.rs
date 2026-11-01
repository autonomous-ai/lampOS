use lamp_memory_tiers::*;
use serde_json::json;

const NOW: i64 = 100 * DAY_MS;

fn ingest(store: &mut Store, text: &str, at: i64) -> i64 {
    store
        .ingest(Incoming {
            text: text.into(),
            speaker: Speaker::Owner,
            at_ms: at,
            stt_confidence: None,
            strength: Strength::Said,
            addressed_to_lamp: true,
            topic: "desk".into(),
            remember: false,
        })
        .unwrap()
        .unwrap()
}

fn populated() -> Store {
    let mut store = Store::in_memory("fictional-owner").unwrap();
    let first = ingest(&mut store, "café", NOW);
    let second = ingest(&mut store, "fern", NOW + 1);
    store.owner_left(NOW + 2).unwrap();
    let fact = store.remember(first, FactKind::Statement, None).unwrap();
    let other = store.remember(second, FactKind::Statement, None).unwrap();
    let lines: Vec<_> = [
        "owner_model", "chapter", "event", "person", "place", "taught_rule",
        "learned_pattern", "ahead", "feeling",
    ]
    .into_iter()
    .map(|kind| {
        json!({"replaces":null,"kind":kind,"text":"café","fact_ids":if kind=="learned_pattern"{vec![fact,other]}else{vec![fact]},"fact_keys":[]})
    })
    .collect();
    let batch = store
        .prepare_sleep(NOW + 3, PassKind::Nightly)
        .unwrap()
        .unwrap();
    store.apply_sleep(&batch,&json!({"facts":[],"lines":lines,"episodes":[{"episode_id":1,"record_ids":[first,second],"gist":"garden","importance":0.1}]}).to_string()).unwrap();
    store
}

#[test]
fn population_counts_stored_utf8_text_without_rendering_or_evidence_duplication() {
    let store = populated();
    let p = store
        .tier_population(Access::OWNER_ALONE, NOW + 4, Arm::Tiers)
        .unwrap();
    for (tier, items, bytes) in [
        ("record", 2, 9),
        ("episodes", 1, 16),
        ("facts", 4, 28),
        ("owner_model", 1, 5),
        ("life_story", 2, 10),
        ("how_to_be", 2, 10),
        ("ahead", 1, 5),
        ("feelings", 1, 5),
    ] {
        assert_eq!(p[tier].items, Some(items), "{tier}");
        assert_eq!(p[tier].text_bytes, Some(bytes), "{tier}");
    }
}

#[test]
fn removed_components_are_zero_and_proof_facts_still_support_owner_model() {
    let store = populated();
    for (arm, removed) in [
        (Arm::TiersMinusEpisodes, "episodes"),
        (Arm::TiersMinusLifeStory, "life_story"),
        (Arm::TiersMinusFacts, "facts"),
        (Arm::TiersMinusOwnerModel, "owner_model"),
        (Arm::TiersMinusHowToBe, "how_to_be"),
        (Arm::TiersMinusAhead, "ahead"),
        (Arm::TiersMinusFeelings, "feelings"),
    ] {
        let p = store
            .tier_population(Access::OWNER_ALONE, NOW + 4, arm)
            .unwrap();
        assert_eq!(p[removed].items, Some(0), "{removed}");
        assert_eq!(p[removed].text_bytes, Some(0), "{removed}");
        if arm == Arm::TiersMinusFacts {
            assert_eq!(p["evidence_only_facts"].items, Some(2));
            assert_eq!(p["evidence_only_facts"].text_bytes, Some(18));
            assert_eq!(p["owner_model"].items, Some(1));
        } else {
            assert!(!p.contains_key("evidence_only_facts"));
        }
    }
}

#[test]
fn inventory_ignores_capture_pause_and_never_credits_future_sources() {
    let mut store = populated();
    let before = store
        .tier_population(Access::OWNER_ALONE, NOW + 4, Arm::Tiers)
        .unwrap();
    store.pause(true).unwrap();
    assert_eq!(
        store
            .tier_population(Access::OWNER_ALONE, NOW + 4, Arm::Tiers)
            .unwrap(),
        before
    );
    let earlier = store
        .tier_population(Access::OWNER_ALONE, NOW, Arm::Tiers)
        .unwrap();
    assert_eq!(earlier["record"].items, Some(1));
    assert_eq!(earlier["facts"].items, Some(3));
    // One future source invalidates a whole summarized episode and the learned pattern.
    assert_eq!(earlier["episodes"].items, Some(0));
    assert_eq!(earlier["how_to_be"].items, Some(1));
    assert!(
        store
            .tier_population(
                Access {
                    owner: true,
                    alone: false
                },
                NOW + 4,
                Arm::Tiers
            )
            .unwrap()
            .is_empty()
    );
}

#[test]
fn raw_episode_inventory_has_exact_age_boundary_and_uses_record_text_storage() {
    let mut store = Store::in_memory("fictional-owner").unwrap();
    ingest(&mut store, "café", NOW);
    let at_boundary = store
        .tier_population(Access::OWNER_ALONE, NOW + 2 * DAY_MS, Arm::Tiers)
        .unwrap();
    assert_eq!(at_boundary["episodes"].items, Some(1));
    assert_eq!(at_boundary["episodes"].text_bytes, Some(0));
    ingest(&mut store, "recent line", NOW + 2 * DAY_MS + 1);
    let older_start = store
        .tier_population(Access::OWNER_ALONE, NOW + 2 * DAY_MS + 1, Arm::Tiers)
        .unwrap();
    assert_eq!(older_start["episodes"].items, Some(0));
    assert_eq!(older_start["record"].items, Some(2));
}

#[test]
fn fading_and_forgetting_change_population_without_restoring_old_text() {
    let mut store = populated();
    store.maintain(NOW + 91 * DAY_MS, Arm::Tiers).unwrap();
    let p = store
        .tier_population(Access::OWNER_ALONE, NOW + 91 * DAY_MS, Arm::Tiers)
        .unwrap();
    assert_eq!(p["episodes"].items, Some(1));
    assert_eq!(p["episodes"].text_bytes, Some(6));
    store.forget_records(&[1, 2]).unwrap();
    let empty = store
        .tier_population(Access::OWNER_ALONE, NOW + 91 * DAY_MS, Arm::Tiers)
        .unwrap();
    assert!(
        empty
            .values()
            .all(|v| v.items == Some(0) && v.text_bytes == Some(0))
    );
}

#[test]
fn expired_fact_and_line_are_ineligible_without_mutating_the_sleep_revision() {
    let mut store = Store::in_memory("fictional-owner").unwrap();
    let id = ingest(&mut store, "A sketching plan.", NOW);
    let fact = store.remember(id, FactKind::Plan, None).unwrap();
    let batch = store
        .prepare_sleep(NOW + 1, PassKind::Nightly)
        .unwrap()
        .unwrap();
    store.apply_sleep(&batch,&json!({"facts":[],"episodes":[],"lines":[{"replaces":null,"kind":"ahead","text":"Sketching later.","fact_ids":[fact],"fact_keys":[]}]}).to_string()).unwrap();
    ingest(&mut store, "Pending source.", NOW + 2);
    let pending = store
        .prepare_sleep(NOW + 3, PassKind::Nightly)
        .unwrap()
        .unwrap();
    let expired = store
        .tier_population(Access::OWNER_ALONE, NOW + 14 * DAY_MS, Arm::Tiers)
        .unwrap();
    assert_eq!(expired["facts"].items, Some(0));
    assert_eq!(expired["ahead"].items, Some(0));
    assert_eq!(store.facts(Access::OWNER_ALONE).unwrap().len(), 1);
    store
        .apply_sleep(&pending, r#"{"facts":[],"lines":[],"episodes":[]}"#)
        .unwrap();
}

#[test]
fn population_is_not_limited_to_the_recall_selection_window() {
    let mut store = Store::in_memory("fictional-owner").unwrap();
    for i in 0..20 {
        let id = ingest(&mut store, &format!("Synthetic note {i}."), NOW + i);
        store.remember(id, FactKind::Statement, None).unwrap();
    }
    let p = store
        .tier_population(Access::OWNER_ALONE, NOW + 100, Arm::Tiers)
        .unwrap();
    assert_eq!(p["facts"].items, Some(20));
}

#[test]
fn historical_claim_cannot_make_a_future_quote_eligible() {
    let mut store = Store::in_memory("fictional-owner").unwrap();
    let id = ingest(&mut store, "A previously chosen notebook.", NOW + 10);
    let batch = store
        .prepare_sleep(NOW + 20, PassKind::Nightly)
        .unwrap()
        .unwrap();
    store.apply_sleep(&batch,&json!({"facts":[{"key":"historical","claim":"A prior notebook choice.","record_ids":[id],"strength":"said","kind":"statement","true_since_ms":NOW,"true_until_ms":null,"supersedes":null}],"lines":[{"replaces":null,"kind":"owner_model","text":"Uses a notebook.","fact_ids":[],"fact_keys":["historical"]}],"episodes":[]}).to_string()).unwrap();
    let future_quote = store
        .tier_population(Access::OWNER_ALONE, NOW + 5, Arm::Tiers)
        .unwrap();
    assert_eq!(future_quote["facts"].items, Some(0));
    assert_eq!(future_quote["owner_model"].items, Some(0));
    let available = store
        .tier_population(Access::OWNER_ALONE, NOW + 10, Arm::Tiers)
        .unwrap();
    assert_eq!(available["facts"].items, Some(1));
    assert_eq!(available["owner_model"].items, Some(1));
}
