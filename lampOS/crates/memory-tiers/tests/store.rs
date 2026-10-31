use lamp_memory_tiers::*;
use serde_json::{Value, json};
const NOW: i64 = DAY_MS;
const EMPTY: &str = include_str!("fixtures/empty.json");
const MAYBE: &str = include_str!("fixtures/maybe.json");
fn incoming(text: &str) -> Incoming {
    Incoming {
        text: text.into(),
        speaker: Speaker::Owner,
        at_ms: NOW,
        stt_confidence: Some(0.83),
        strength: Strength::Said,
        addressed_to_lamp: true,
        topic: "home".into(),
        remember: false,
    }
}
fn store() -> Store {
    Store::in_memory("fictional-owner").unwrap()
}
fn input(s: &mut Store) -> SleepInput {
    s.prepare_sleep(NOW + 100, PassKind::Nightly)
        .unwrap()
        .unwrap()
}
fn fact(id: i64, key: &str) -> Value {
    json!({"key":key,"claim":format!("A statement {key}."),"record_ids":[id],"strength":"said","kind":"preference","true_since_ms":NOW,"true_until_ms":null,"supersedes":null})
}
fn line(key: &str) -> Value {
    json!({"replaces":null,"kind":"owner_model","text":format!("Preference {key}."),"fact_ids":[],"fact_keys":[key]})
}
fn proposal(facts: Vec<Value>, lines: Vec<Value>) -> String {
    json!({"facts":facts,"lines":lines,"episodes":[]}).to_string()
}
#[test]
fn exact_quotes_metadata_and_maybe_are_preserved() {
    let mut s = store();
    let text = "Maybe I'll move to the coast — I haven't decided.\n  Ask next month.";
    s.ingest(incoming(text)).unwrap();
    let batch = input(&mut s);
    s.apply_sleep(&batch, MAYBE).unwrap();
    let facts = s.facts(Access::OWNER_ALONE).unwrap();
    let q = &facts[0].quotes[0];
    assert_eq!(q.text, text);
    assert_eq!(q.speaker, Speaker::Owner);
    assert_eq!(q.at_ms, NOW);
    assert_eq!(q.stt_confidence, Some(0.83));
    assert_eq!(facts[0].strength, Strength::Maybe);
    assert_eq!(q.strength, Strength::Maybe);
    assert_eq!(
        s.owner_model(Access::OWNER_ALONE, NOW + 100).unwrap()[0].strength,
        Strength::Maybe
    );
}
#[test]
fn model_cannot_supply_quotes_or_delete() {
    let mut s = store();
    s.ingest(incoming("An ordinary statement.")).unwrap();
    let batch = input(&mut s);
    let mut output: Value = serde_json::from_str(MAYBE).unwrap();
    output["facts"][0]["quotes"] = json!(["fabricated"]);
    assert!(s.apply_sleep(&batch, &output.to_string()).is_err());
    assert!(
        s.apply_sleep(
            &batch,
            r#"{"facts":[],"lines":[],"episodes":[],"delete":[1]}"#
        )
        .is_err()
    );
    assert!(s.facts(Access::OWNER_ALONE).unwrap().is_empty());
}
#[test]
fn forget_clears_sources_views_and_inflight_result() {
    let mut s = store();
    s.ingest(incoming("Maybe I will move to the coast."))
        .unwrap();
    let batch = input(&mut s);
    s.apply_sleep(&batch, MAYBE).unwrap();
    let pending = s
        .prepare_sleep(NOW + 200, PassKind::Weekly)
        .unwrap()
        .unwrap();
    assert_eq!(s.forget_fact(1).unwrap(), 1);
    assert!(s.records(Access::OWNER_ALONE).unwrap().is_empty());
    assert!(s.facts(Access::OWNER_ALONE).unwrap().is_empty());
    assert!(s.lines(Access::OWNER_ALONE).unwrap().is_empty());
    assert!(matches!(s.apply_sleep(&pending, MAYBE), Err(Error::Stale)));
    assert!(
        s.ingest(incoming("MAYBE I WILL MOVE TO THE COAST!"))
            .unwrap()
            .is_none()
    );
    assert!(
        s.prepare_sleep(NOW + 300, PassKind::Nightly)
            .unwrap()
            .is_none()
    );
    assert!(
        s.episodes(Access::OWNER_ALONE).unwrap()[0]
            .detail
            .is_empty()
    );
}
#[test]
fn visitor_assistant_and_unsure_never_prove_owner_facts() {
    let mut s = store();
    let mut r = incoming("The owner likes expensive gifts.");
    r.speaker = Speaker::Visitor;
    assert!(s.ingest(r).unwrap().is_none());
    let mut r = incoming("You are a racing champion.");
    r.speaker = Speaker::Assistant;
    let other = s.ingest(r).unwrap().unwrap();
    let mut r = incoming("I might be the other person.");
    r.speaker = Speaker::Unsure;
    s.ingest(r).unwrap();
    let owner = s
        .ingest(incoming("I prefer a quiet room."))
        .unwrap()
        .unwrap();
    let batch = input(&mut s);
    assert_eq!(batch.records().len(), 1);
    assert_eq!(batch.records()[0].id, owner);
    assert!(
        s.apply_sleep(&batch, &proposal(vec![fact(other, "wrong")], vec![]))
            .is_err()
    );
    assert!(s.remember(other, FactKind::Preference, None).is_err());
    assert!(
        s.owner_model(
            Access {
                owner: true,
                alone: false
            },
            NOW
        )
        .unwrap()
        .is_empty()
    );
    assert!(s.records(Access::default()).unwrap().is_empty());
}
#[test]
fn rewrite_over_one_quarter_rolls_back_the_whole_night() {
    let mut s = store();
    let mut fs = vec![];
    let mut ls = vec![];
    for n in 0..4 {
        let id = s
            .ingest(incoming(&format!("Preference number {n} is lasting.")))
            .unwrap()
            .unwrap();
        fs.push(fact(id, &format!("f{n}")));
        ls.push(line(&format!("f{n}")));
    }
    let batch = input(&mut s);
    s.apply_sleep(&batch, &proposal(fs, ls)).unwrap();
    s.ingest(incoming("An additional source today.")).unwrap();
    let batch = input(&mut s);
    let before = s.lines(Access::OWNER_ALONE).unwrap();
    let replacements=before.iter().take(2).map(|l|json!({"replaces":l.id,"kind":"owner_model","text":"Changed preference.","fact_ids":l.fact_ids,"fact_keys":[]})).collect();
    assert!(
        s.apply_sleep(&batch, &proposal(vec![], replacements))
            .unwrap_err()
            .to_string()
            .contains("one quarter")
    );
    assert_eq!(
        s.lines(Access::OWNER_ALONE).unwrap()[0].text,
        before[0].text
    );
    let one = json!({"replaces":before[0].id,"kind":"owner_model","text":"One changed preference.","fact_ids":before[0].fact_ids,"fact_keys":[]});
    s.apply_sleep(&batch, &proposal(vec![], vec![one])).unwrap();
    assert_eq!(s.lines(Access::OWNER_ALONE).unwrap().len(), 4);
}
#[test]
fn empty_night_advances_but_failed_batch_does_not() {
    let mut s = store();
    s.ingest(incoming("No durable information here.")).unwrap();
    let batch = input(&mut s);
    assert!(s.apply_sleep(&batch, "not json").is_err());
    assert!(
        s.prepare_sleep(NOW + 100, PassKind::Nightly)
            .unwrap()
            .is_some()
    );
    s.apply_sleep(&batch, EMPTY).unwrap();
    assert!(
        s.prepare_sleep(NOW + 100, PassKind::Nightly)
            .unwrap()
            .is_none()
    );
    assert!(matches!(s.apply_sleep(&batch, EMPTY), Err(Error::Stale)));
}
#[test]
fn arrivals_during_sleep_are_not_lost() {
    let mut s = store();
    s.ingest(incoming("The first conversation.")).unwrap();
    let batch = input(&mut s);
    let later = s
        .ingest(incoming("Something said while the model was running."))
        .unwrap()
        .unwrap();
    s.apply_sleep(&batch, EMPTY).unwrap();
    assert_eq!(input(&mut s).records()[0].id, later);
}
#[test]
fn episodes_cut_on_topics_and_departure_not_silence() {
    let mut s = store();
    s.ingest(incoming("Morning home talk.")).unwrap();
    let mut r = incoming("More on home, hours later.");
    r.at_ms += 3_600_000;
    s.ingest(r.clone()).unwrap();
    assert_eq!(s.episodes(Access::OWNER_ALONE).unwrap().len(), 1);
    r.topic = "work".into();
    r.at_ms += 1;
    s.ingest(r.clone()).unwrap();
    s.owner_left(r.at_ms + 1).unwrap();
    r.at_ms += 2;
    s.ingest(r).unwrap();
    let eps = s.episodes(Access::OWNER_ALONE).unwrap();
    assert_eq!(eps.len(), 3);
    assert!(eps[0].ended_ms.is_some());
    assert!(eps[1].ended_ms.is_some());
    assert!(eps[2].ended_ms.is_none());
}
#[test]
fn plans_expire_views_but_keep_history_and_preferences() {
    let mut s = store();
    let a = s
        .ingest(incoming("I plan to build a toy boat."))
        .unwrap()
        .unwrap();
    let b = s
        .ingest(incoming("I like quiet mornings."))
        .unwrap()
        .unwrap();
    let batch = input(&mut s);
    let mut p = fact(a, "plan");
    p["kind"] = json!("plan");
    s.apply_sleep(
        &batch,
        &proposal(vec![p, fact(b, "like")], vec![line("plan"), line("like")]),
    )
    .unwrap();
    let model = s
        .owner_model(Access::OWNER_ALONE, NOW + 15 * DAY_MS)
        .unwrap();
    assert_eq!(model.len(), 1);
    assert_eq!(model[0].text, "Preference like.");
    let facts = s.facts(Access::OWNER_ALONE).unwrap();
    assert!(!facts[0].active);
    assert!(facts[1].active);
    assert_eq!(facts[0].strength, Strength::Plan);
    assert_eq!(s.records(Access::OWNER_ALONE).unwrap().len(), 2);
}
#[test]
fn appointments_need_absolute_end_times() {
    let mut s = store();
    s.ingest(incoming("I have an appointment tomorrow."))
        .unwrap();
    let batch = input(&mut s);
    let mut f = fact(1, "meeting");
    f["kind"] = json!("appointment");
    assert!(
        s.apply_sleep(&batch, &proposal(vec![f.clone()], vec![]))
            .is_err()
    );
    f["true_until_ms"] = json!(NOW + DAY_MS);
    s.apply_sleep(&batch, &proposal(vec![f], vec![])).unwrap();
    assert_eq!(s.expire(NOW + DAY_MS).unwrap(), 1);
}
#[test]
fn taught_rule_copies_owner_words_not_model_rephrasing() {
    let mut s = store();
    let text = "Please keep answers short before breakfast.";
    s.ingest(incoming(text)).unwrap();
    let batch = input(&mut s);
    let mut l = line("rule");
    l["kind"] = json!("taught_rule");
    l["text"] = json!("Always be terse.");
    s.apply_sleep(&batch, &proposal(vec![fact(1, "rule")], vec![l]))
        .unwrap();
    assert_eq!(s.lines(Access::OWNER_ALONE).unwrap()[0].text, text);
}
#[test]
fn input_from_other_store_or_before_pause_is_refused() {
    let mut a = store();
    a.ingest(incoming("Private original.")).unwrap();
    let batch = input(&mut a);
    let mut b = Store::in_memory("other-owner").unwrap();
    b.ingest(incoming("Private original.")).unwrap();
    assert!(matches!(b.apply_sleep(&batch, EMPTY), Err(Error::Stale)));
    a.pause(true).unwrap();
    a.pause(false).unwrap();
    assert!(matches!(a.apply_sleep(&batch, EMPTY), Err(Error::Stale)));
}
#[test]
fn disk_forget_clears_source_quote_and_fts_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("memory.sqlite");
    let marker = "incomparablemarigold-secretvalue";
    {
        let mut s = Store::open(&path, "fictional-owner").unwrap();
        s.ingest(incoming(marker)).unwrap();
        s.remember(1, FactKind::Statement, None).unwrap();
        s.forget_records(&[1]).unwrap();
    }
    let bytes = std::fs::read(&path).unwrap();
    assert!(!bytes.windows(marker.len()).any(|w| w == marker.as_bytes()));
    let mut s = Store::open(&path, "fictional-owner").unwrap();
    assert!(s.ingest(incoming(marker)).unwrap().is_none());
    assert!(Store::open(&path, "another-owner").is_err());
}
#[test]
fn input_is_bounded_without_silently_truncating_records() {
    let mut s = store();
    let text = "字".repeat(5000);
    for _ in 0..10 {
        s.ingest(incoming(&text)).unwrap();
    }
    let batch = input(&mut s);
    assert!(batch.json().unwrap().len() <= MAX_INPUT_BYTES);
    assert!(batch.records().len() < 10);
    assert_eq!(s.records(Access::OWNER_ALONE).unwrap()[9].text, text);
    s.apply_sleep(&batch, EMPTY).unwrap();
    assert!(input(&mut s).records()[0].id > 1);
}
#[test]
fn fading_removes_only_derived_detail() {
    let mut s = store();
    s.ingest(incoming("Maybe I will move to the coast."))
        .unwrap();
    s.owner_left(NOW + 1).unwrap();
    let batch = input(&mut s);
    let mut p: Value = serde_json::from_str(MAYBE).unwrap();
    p["episodes"][0]["importance"] = json!(0.1);
    s.apply_sleep(&batch, &p.to_string()).unwrap();
    assert_eq!(s.fade(NOW + 100 * DAY_MS).unwrap(), 1);
    assert!(
        s.episodes(Access::OWNER_ALONE).unwrap()[0]
            .detail
            .is_empty()
    );
    assert!(!s.episodes(Access::OWNER_ALONE).unwrap()[0].gist.is_empty());
    assert_eq!(s.records(Access::OWNER_ALONE).unwrap().len(), 1);
}
#[test]
fn pause_and_overheard_lines_are_not_kept() {
    let mut s = store();
    s.pause(true).unwrap();
    assert!(s.ingest(incoming("Do not retain this.")).unwrap().is_none());
    s.pause(false).unwrap();
    let mut r = incoming("Overheard talk.");
    r.addressed_to_lamp = false;
    assert!(s.ingest(r).unwrap().is_none());
    assert!(s.records(Access::OWNER_ALONE).unwrap().is_empty());
}

#[test]
fn short_forgotten_span_embedded_in_new_text_is_rejected() {
    let mut s = store();
    s.ingest(incoming("My code is orange")).unwrap();
    s.forget_records(&[1]).unwrap();
    assert!(
        s.ingest(incoming("Remember my code is orange today"))
            .unwrap()
            .is_none()
    );
}
#[test]
fn forget_removes_independently_sourced_duplicate_claims_and_views() {
    let mut s = store();
    s.ingest(incoming("I might move near the water.")).unwrap();
    s.ingest(incoming("A coastal home is an option.")).unwrap();
    let batch = input(&mut s);
    let mut a = fact(1, "a");
    let mut b = fact(2, "b");
    a["claim"] = json!("A coastal move is possible.");
    b["claim"] = a["claim"].clone();
    s.apply_sleep(&batch, &proposal(vec![a, b], vec![line("a"), line("b")]))
        .unwrap();
    s.forget_fact(1).unwrap();
    assert!(s.facts(Access::OWNER_ALONE).unwrap().is_empty());
    assert!(s.lines(Access::OWNER_ALONE).unwrap().is_empty());
    assert!(
        s.prepare_sleep(NOW + 100, PassKind::Weekly)
            .unwrap()
            .is_none()
    );
}
#[test]
fn weekly_batches_reach_every_record_and_then_finish() {
    let mut s = store();
    for n in 0..140 {
        s.ingest(incoming(&format!("A short event number {n}.")))
            .unwrap();
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut batches = 0;
    while let Some(batch) = s.prepare_sleep(NOW + 100, PassKind::Weekly).unwrap() {
        batches += 1;
        assert!(batches <= 3);
        for r in batch.records() {
            assert!(seen.insert(r.id));
        }
        s.apply_sleep(&batch, EMPTY).unwrap();
    }
    assert_eq!(seen.len(), 140);
    assert!(batches >= 2);
}
#[test]
fn rewrite_budget_is_cumulative_across_batches_of_one_night() {
    let mut s = store();
    let mut fs = vec![];
    let mut ls = vec![];
    for n in 0..4 {
        let id = s
            .ingest(incoming(&format!("A lasting preference number {n}.")))
            .unwrap()
            .unwrap();
        fs.push(fact(id, &format!("f{n}")));
        ls.push(line(&format!("f{n}")));
    }
    let batch = input(&mut s);
    s.apply_sleep(&batch, &proposal(fs, ls)).unwrap();
    let lines = s.lines(Access::OWNER_ALONE).unwrap();
    for (n, line) in lines.iter().take(2).enumerate() {
        s.ingest(incoming(&format!("Fresh evidence number {n}.")))
            .unwrap();
        let batch = input(&mut s);
        let update = json!({"replaces":line.id,"kind":"owner_model","text":format!("Changed preference {n}."),"fact_ids":line.fact_ids,"fact_keys":[]});
        let result = s.apply_sleep(&batch, &proposal(vec![], vec![update]));
        if n == 0 {
            assert!(result.is_ok());
        } else {
            assert!(result.unwrap_err().to_string().contains("one quarter"));
        }
    }
}
#[test]
fn unknown_confidence_stays_unknown_and_plan_cannot_become_said() {
    let mut s = store();
    let mut r = incoming("I plan to visit the coast.");
    r.stt_confidence = None;
    s.ingest(r).unwrap();
    let batch = input(&mut s);
    s.apply_sleep(&batch, &proposal(vec![fact(1, "plan")], vec![]))
        .unwrap();
    let facts = s.facts(Access::OWNER_ALONE).unwrap();
    assert_eq!(facts[0].strength, Strength::Plan);
    assert_eq!(facts[0].quotes[0].stt_confidence, None);
}

#[test]
fn deleting_a_line_does_not_delete_its_unrelated_parent_fact() {
    let mut s = store();
    s.ingest(incoming("I like apples.")).unwrap();
    s.ingest(incoming("I work late.")).unwrap();
    let batch = input(&mut s);
    let combined = json!({"replaces":null,"kind":"owner_model","text":"Two separate preferences.","fact_ids":[],"fact_keys":["food","work"]});
    s.apply_sleep(
        &batch,
        &proposal(vec![fact(1, "food"), fact(2, "work")], vec![combined]),
    )
    .unwrap();
    s.forget_records(&[1]).unwrap();
    assert!(s.lines(Access::OWNER_ALONE).unwrap().is_empty());
    let facts = s.facts(Access::OWNER_ALONE).unwrap();
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].quotes[0].record_id, 2);
}
#[test]
fn explicit_keep_accepts_the_entire_long_source_without_model() {
    let mut s = store();
    let text = "A careful standing rule.\n".repeat(300);
    s.ingest(incoming(&text)).unwrap();
    s.remember(1, FactKind::Statement, None).unwrap();
    let facts = s.facts(Access::OWNER_ALONE).unwrap();
    assert_eq!(facts[0].claim, text);
    assert_eq!(facts[0].quotes[0].text, text);
}
#[test]
fn delayed_sleep_does_not_restart_an_old_plans_expiry() {
    let mut s = store();
    s.ingest(incoming("I plan a short trip.")).unwrap();
    let batch = s
        .prepare_sleep(NOW + 20 * DAY_MS, PassKind::Nightly)
        .unwrap()
        .unwrap();
    let mut f = fact(1, "trip");
    f["kind"] = json!("plan");
    s.apply_sleep(&batch, &proposal(vec![f], vec![])).unwrap();
    let facts = s.facts(Access::OWNER_ALONE).unwrap();
    assert!(!facts[0].active);
    assert_eq!(facts[0].expires_ms, Some(NOW + 14 * DAY_MS));
}
#[test]
fn sleep_input_declares_the_fixed_fixture_zone() {
    let mut s = store();
    s.set_time_zone("Asia/Ho_Chi_Minh").unwrap();
    let mut r = incoming("A late local event.");
    r.at_ms = DAY_MS - 1;
    s.ingest(r).unwrap();
    let batch = s
        .prepare_sleep(DAY_MS - 1, PassKind::Nightly)
        .unwrap()
        .unwrap();
    let data: Value = serde_json::from_str(&batch.json().unwrap()).unwrap();
    assert_eq!(data["time_zone"], "Asia/Ho_Chi_Minh");
    assert_eq!(data["cycle"], 1);
    assert!(s.set_time_zone("UTC").is_err());
}

#[test]
fn learned_pattern_requires_two_distinct_owner_sources() {
    let mut s = store();
    s.ingest(incoming("Short answers suit this morning."))
        .unwrap();
    let batch = input(&mut s);
    let mut pattern = line("style");
    pattern["kind"] = json!("learned_pattern");
    assert!(
        s.apply_sleep(
            &batch,
            &proposal(vec![fact(1, "style")], vec![pattern.clone()])
        )
        .is_err()
    );
    s.ingest(incoming("Again, keep this morning answer short."))
        .unwrap();
    let batch = input(&mut s);
    let mut f = fact(1, "style");
    f["record_ids"] = json!([1, 2]);
    s.apply_sleep(&batch, &proposal(vec![f], vec![pattern]))
        .unwrap();
    assert_eq!(
        s.lines(Access::OWNER_ALONE).unwrap()[0].kind,
        LineKind::LearnedPattern
    );
}
