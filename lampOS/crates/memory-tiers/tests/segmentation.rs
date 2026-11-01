use lamp_memory_tiers::*;
use serde_json::json;

const NOW: i64 = DAY_MS;
const EMPTY: &str = r#"{"facts":[],"lines":[],"episodes":[]}"#;
fn incoming(text: &str, at: i64) -> Incoming {
    Incoming {
        text: text.into(),
        speaker: Speaker::Owner,
        at_ms: at,
        stt_confidence: Some(0.8),
        strength: Strength::Maybe,
        addressed_to_lamp: true,
        topic: "conversation".into(),
        remember: false,
    }
}
fn store(words: &[&str]) -> Store {
    let mut s = Store::in_memory("fictional-owner").unwrap();
    for (i, text) in words.iter().enumerate() {
        s.ingest(incoming(text, NOW + i as i64)).unwrap();
    }
    s.owner_left(NOW + words.len() as i64).unwrap();
    s
}
fn ranges(values: &[(i64, i64)]) -> String {
    json!({"segments":values.iter().map(|(first,last)|json!({"first_record_id":first,"last_record_id":last})).collect::<Vec<_>>()} ).to_string()
}
fn records(s: &Store) -> Vec<Record> {
    s.records(Access::OWNER_ALONE).unwrap()
}

#[test]
fn topic_cuts_preserve_words_ids_and_quotes() {
    let mut s = store(&[
        "Maybe I like blue paper.",
        "Paper folds easily.",
        "The fern needs water.",
    ]);
    s.remember(1, FactKind::Preference, None).unwrap();
    let old = s.facts(Access::OWNER_ALONE).unwrap();
    let before = records(&s);
    let input = s.prepare_topics(NOW + 100).unwrap().unwrap();
    let report = s.apply_topics(&input, &ranges(&[(1, 2), (3, 3)])).unwrap();
    assert_eq!(report.records_partitioned, 3);
    assert_eq!(report.episodes, 2);
    assert!(!report.pending);
    let after = records(&s);
    for (a, b) in before.iter().zip(&after) {
        assert_eq!(
            (a.id, &a.text, a.at_ms, a.stt_confidence, a.strength),
            (b.id, &b.text, b.at_ms, b.stt_confidence, b.strength)
        );
        assert_ne!(a.episode_id, b.episode_id);
    }
    assert_eq!(after[0].episode_id, after[1].episode_id);
    assert_ne!(after[1].episode_id, after[2].episode_id);
    assert_eq!(
        serde_json::to_value(old).unwrap(),
        serde_json::to_value(s.facts(Access::OWNER_ALONE).unwrap()).unwrap()
    );
    assert_eq!(s.episodes(Access::OWNER_ALONE).unwrap().len(), 2);
    assert!(s.prepare_topics(NOW + 100).unwrap().is_none());
}

#[test]
fn gaps_overlaps_reordering_and_foreign_endpoints_are_atomic_rejections() {
    for cuts in [
        vec![],
        vec![(1, 2)],
        vec![(2, 3)],
        vec![(1, 2), (2, 3)],
        vec![(3, 3), (1, 2)],
        vec![(1, 99)],
        vec![(1, 1), (3, 3)],
    ] {
        let mut s = store(&["One.", "Two.", "Three."]);
        let input = s.prepare_topics(NOW + 100).unwrap().unwrap();
        let before = serde_json::to_value(records(&s)).unwrap();
        assert!(s.apply_topics(&input, &ranges(&cuts)).is_err(), "{cuts:?}");
        assert_eq!(before, serde_json::to_value(records(&s)).unwrap());
        s.apply_topics(&input, &ranges(&[(1, 3)])).unwrap();
    }
}

#[test]
fn ranges_cannot_cross_sessions_and_deleted_numeric_gaps_are_valid() {
    let mut s = store(&["First.", "Forgettable middle.", "Last."]);
    s.ingest(incoming("Another conversation.", NOW + 10))
        .unwrap();
    s.owner_left(NOW + 11).unwrap();
    s.forget_records(&[2]).unwrap();
    let input = s.prepare_topics(NOW + 100).unwrap().unwrap();
    assert_eq!(
        input.records().iter().map(|r| r.id).collect::<Vec<_>>(),
        vec![1, 3]
    );
    assert!(s.apply_topics(&input, &ranges(&[(1, 4)])).is_err());
    assert!(s.apply_topics(&input, &ranges(&[(1, 3)])).unwrap().pending);
    let next = s.prepare_topics(NOW + 100).unwrap().unwrap();
    assert_eq!(next.records()[0].id, 4);
}

#[test]
fn open_session_and_earlier_extraction_do_not_skip_partitioning() {
    let mut s = Store::in_memory("fictional-owner").unwrap();
    s.ingest(incoming("Early words.", NOW)).unwrap();
    assert!(s.prepare_topics(NOW + 10).unwrap().is_none());
    let early = s
        .prepare_sleep(NOW + 10, PassKind::Nightly)
        .unwrap()
        .unwrap();
    s.apply_sleep(&early, EMPTY).unwrap();
    s.ingest(incoming("Later words.", NOW + DAY_MS)).unwrap();
    s.owner_left(NOW + DAY_MS + 1).unwrap();
    let input = s.prepare_topics(NOW + DAY_MS + 2).unwrap().unwrap();
    assert_eq!(input.records().len(), 2);
    s.apply_topics(&input, &ranges(&[(1, 1), (2, 2)])).unwrap();
    let extraction = s
        .prepare_sleep(NOW + DAY_MS + 3, PassKind::Nightly)
        .unwrap()
        .unwrap();
    assert_eq!(extraction.records().len(), 2);
}

#[test]
fn record_and_byte_bounds_continue_without_repartitioning_any_record() {
    for (count, text) in [(129, "Short.".to_owned()), (24, "a".repeat(4000))] {
        let mut s = Store::in_memory("fictional-owner").unwrap();
        for i in 0..count {
            s.ingest(incoming(&text, NOW + i)).unwrap();
        }
        s.owner_left(NOW + count).unwrap();
        let mut seen = std::collections::BTreeSet::new();
        let mut batches = 0;
        while let Some(input) = s.prepare_topics(NOW + count + 1).unwrap() {
            assert!(input.json().unwrap().len() <= MAX_INPUT_BYTES);
            assert!(input.records().len() <= MAX_RECORDS);
            assert_eq!(input.continued_from_previous_batch, batches > 0);
            for record in input.records() {
                assert!(seen.insert(record.id));
            }
            let first = input.records().first().unwrap().id;
            let last = input.records().last().unwrap().id;
            let report = s.apply_topics(&input, &ranges(&[(first, last)])).unwrap();
            assert_eq!(report.pending, input.continues_in_next_batch);
            batches += 1;
        }
        assert!(batches >= 2);
        assert_eq!(seen.len(), count as usize);
    }
}

#[test]
fn forgetting_invalidates_pending_ranges_and_all_partition_metadata() {
    let mut s = store(&["A blue lantern.", "A green fern."]);
    let stale = s.prepare_topics(NOW + 100).unwrap().unwrap();
    s.forget_records(&[1]).unwrap();
    assert!(matches!(
        s.apply_topics(&stale, &ranges(&[(1, 2)])),
        Err(Error::Stale)
    ));
    assert!(matches!(stale.json(), Err(Error::Stale)));
    let current = s.prepare_topics(NOW + 100).unwrap().unwrap();
    assert!(!current.json().unwrap().contains("blue lantern"));
    assert!(s.apply_topics(&current, &ranges(&[(1, 2)])).is_err());
    s.apply_topics(&current, &ranges(&[(2, 2)])).unwrap();
    s.forget_records(&[2]).unwrap();
    assert!(records(&s).is_empty());
    assert!(s.episodes(Access::OWNER_ALONE).unwrap().is_empty());
    assert!(s.prepare_topics(NOW + 100).unwrap().is_none());
    assert!(
        s.ingest(incoming("A green fern.", NOW + 101))
            .unwrap()
            .is_none()
    );
}

#[test]
fn repartition_invalidates_old_extraction_and_weekly_does_not_cut_again() {
    let mut s = store(&["First subject.", "Second subject."]);
    let sleep = s
        .prepare_sleep(NOW + 100, PassKind::Nightly)
        .unwrap()
        .unwrap();
    let input = s.prepare_topics(NOW + 100).unwrap().unwrap();
    s.apply_topics(&input, &ranges(&[(1, 1), (2, 2)])).unwrap();
    assert!(matches!(s.apply_sleep(&sleep, EMPTY), Err(Error::Stale)));
    let before = records(&s).iter().map(|r| r.episode_id).collect::<Vec<_>>();
    let weekly = s
        .prepare_sleep(NOW + 100, PassKind::Weekly)
        .unwrap()
        .unwrap();
    s.apply_sleep(&weekly, EMPTY).unwrap();
    assert_eq!(
        before,
        records(&s).iter().map(|r| r.episode_id).collect::<Vec<_>>()
    );
    assert!(s.prepare_topics(NOW + 100).unwrap().is_none());
}

#[test]
fn topic_schema_allows_no_generated_words_deletions_or_assigned_episode_ids() {
    for extra in ["gist", "delete", "episode_id"] {
        let mut value = json!({"segments":[{"first_record_id":1,"last_record_id":1}]});
        value["segments"][0][extra] = json!("forbidden");
        assert!(TopicProposal::parse(&value.to_string()).is_err());
    }
}

#[test]
fn forgetting_a_partitioned_prefix_leaves_no_old_timestamp_on_the_remainder() {
    let mut s = Store::in_memory("fictional-owner").unwrap();
    for i in 0..129 {
        s.ingest(incoming(&format!("Distinct source {i}."), NOW + i))
            .unwrap();
    }
    s.owner_left(NOW + 130).unwrap();
    let input = s.prepare_topics(NOW + 200).unwrap().unwrap();
    assert_eq!(input.records().len(), 128);
    s.apply_topics(&input, &ranges(&[(1, 128)])).unwrap();
    s.forget_records(&(1..=128).collect::<Vec<_>>()).unwrap();
    let remaining = s.episodes(Access::OWNER_ALONE).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].record_ids, vec![129]);
    assert_eq!(remaining[0].started_ms, NOW + 128);
    assert_eq!(remaining[0].ended_ms, Some(NOW + 128));
}

#[test]
fn schema_two_upgrade_preserves_fingerprints_and_completed_partition_progress() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("memory.sqlite");
    let mut s = Store::open(&path, "fictional-owner").unwrap();
    s.ingest(incoming("A private violet note.", NOW)).unwrap();
    s.ingest(incoming("A surviving green note.", NOW + 1))
        .unwrap();
    s.owner_left(NOW + 2).unwrap();
    s.forget_records(&[1]).unwrap();
    drop(s);
    let old = rusqlite::Connection::open(&path).unwrap();
    old.execute_batch(
        "DROP TABLE topic_assignments; DROP TABLE capture_sessions; PRAGMA user_version=2;",
    )
    .unwrap();
    drop(old);
    let mut s = Store::open(&path, "fictional-owner").unwrap();
    assert!(
        s.ingest(incoming("A private violet note.", NOW + 3))
            .unwrap()
            .is_none()
    );
    s.set_capture_session(2, "synthetic-session").unwrap();
    let input = s.prepare_topics(NOW + 4).unwrap().unwrap();
    s.apply_topics(&input, &ranges(&[(2, 2)])).unwrap();
    let episode = records(&s)[0].episode_id;
    drop(s);
    let mut s = Store::open(&path, "fictional-owner").unwrap();
    assert!(s.prepare_topics(NOW + 5).unwrap().is_none());
    assert_eq!(records(&s)[0].episode_id, episode);
    s.forget_records(&[2]).unwrap();
    drop(s);
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM topic_assignments", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM capture_sessions", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
}

#[test]
fn splitting_old_records_does_not_evict_the_newest_open_episode() {
    let mut s = Store::in_memory("fictional-owner").unwrap();
    for i in 0..8 {
        s.ingest(incoming(&format!("Earlier topic {i}."), NOW + i))
            .unwrap();
    }
    s.owner_left(NOW + 9).unwrap();
    s.ingest(incoming(
        "The newest open conversation has a compass.",
        NOW + 100,
    ))
    .unwrap();
    let input = s.prepare_topics(NOW + 101).unwrap().unwrap();
    s.apply_topics(
        &input,
        &ranges(&(1..=8).map(|id| (id, id)).collect::<Vec<_>>()),
    )
    .unwrap();
    let result = s
        .recall(&RecallQuery {
            access: Access::OWNER_ALONE,
            now_ms: NOW + 200,
            words: "compass".into(),
            cues: Cues::default(),
            max_bytes: 4096,
            budget: RECALL_BUDGET,
            arm: Arm::Tiers,
        })
        .unwrap();
    assert!(!result.timed_out, "recall deadline missed");
    assert!(result.context().contains("compass"));
    assert!(result.source_ids().contains(&9));
}

#[test]
fn forgetting_the_explicit_keep_removes_inherited_sibling_importance() {
    let mut s = Store::in_memory("fictional-owner").unwrap();
    let mut kept = incoming("Keep the violet lantern.", NOW);
    kept.remember = true;
    s.ingest(kept).unwrap();
    s.ingest(incoming("The fern needs water.", NOW + 1))
        .unwrap();
    s.owner_left(NOW + 2).unwrap();
    let input = s.prepare_topics(NOW + 3).unwrap().unwrap();
    s.apply_topics(&input, &ranges(&[(1, 1), (2, 2)])).unwrap();
    assert!(
        s.episodes(Access::OWNER_ALONE)
            .unwrap()
            .iter()
            .all(|e| e.importance == 1.0)
    );
    s.forget_records(&[1]).unwrap();
    let episodes = s.episodes(Access::OWNER_ALONE).unwrap();
    assert_eq!(episodes.len(), 1);
    assert_eq!(episodes[0].importance, 0.0);
    assert_eq!(records(&s)[0].text, "The fern needs water.");
}

#[test]
fn output_allowance_fits_every_partition_and_shortens_batches_at_the_call_ceiling() {
    let mut s = store(&[
        "One.", "Two.", "Three.", "Four.", "Five.", "Six.", "Seven.", "Eight.",
    ]);
    let all = s.prepare_topics(NOW + 100).unwrap().unwrap();
    assert_eq!(all.output_allowance().unwrap(), 406);
    for splits in 0..128 {
        let mut first = 1;
        let mut cuts = Vec::new();
        for last in 1..=8 {
            if last == 8 || splits & (1 << (last - 1)) != 0 {
                cuts.push((first, last));
                first = last + 1;
            }
        }
        assert!(ranges(&cuts).len() <= all.output_allowance().unwrap() as usize);
    }
    let one = s
        .prepare_topics_with_output_limit(NOW + 100, 150)
        .unwrap()
        .unwrap();
    assert_eq!(one.records().len(), 1);
    assert_eq!(one.output_allowance().unwrap(), 119);
    assert!(one.continues_in_next_batch);
    let overlong = format!("{}{}", ranges(&[(1, 1)]), " ".repeat(MAX_OUTPUT_BYTES));
    assert!(s.apply_topics(&one, &overlong).is_err());
    assert_eq!(s.episodes(Access::OWNER_ALONE).unwrap().len(), 1);
    s.apply_topics(&one, &ranges(&[(1, 1)])).unwrap();
    let next = s
        .prepare_topics_with_output_limit(NOW + 100, 150)
        .unwrap()
        .unwrap();
    assert!(next.continued_from_previous_batch);
    assert_eq!(next.records()[0].id, 2);
    assert!(s.prepare_topics_with_output_limit(NOW + 100, 100).is_err());
}

#[test]
fn immutable_wire_membership_changes_no_consolidation_input_and_isolates_ranges() {
    let mut s = store(&["A one.", "B one.", "A two.", "B two."]);
    let old = s
        .prepare_sleep(NOW + 100, PassKind::Nightly)
        .unwrap()
        .unwrap()
        .json()
        .unwrap();
    for (id, key) in [
        (1, "session-a"),
        (2, "session-b"),
        (3, "session-a"),
        (4, "session-b"),
    ] {
        s.set_capture_session(id, key).unwrap();
    }
    assert_eq!(
        old,
        s.prepare_sleep(NOW + 100, PassKind::Nightly)
            .unwrap()
            .unwrap()
            .json()
            .unwrap()
    );
    assert!(s.set_capture_session(1, "changed-session").is_err());
    let a = s.prepare_topics(NOW + 100).unwrap().unwrap();
    assert_eq!(
        a.records().iter().map(|r| r.id).collect::<Vec<_>>(),
        vec![1, 3]
    );
    assert!(s.apply_topics(&a, &ranges(&[(1, 4)])).is_err());
    s.apply_topics(&a, &ranges(&[(1, 3)])).unwrap();
    let b = s
        .prepare_topics_with_output_limit(NOW + 100, 150)
        .unwrap()
        .unwrap();
    assert!(!b.continued_from_previous_batch);
    assert!(b.continues_in_next_batch);
    assert_eq!(b.records()[0].id, 2);
    s.apply_topics(&b, &ranges(&[(2, 2)])).unwrap();
    let b = s.prepare_topics(NOW + 100).unwrap().unwrap();
    assert!(b.continued_from_previous_batch);
    assert_eq!(b.records()[0].id, 4);
}

#[test]
fn pretty_and_compact_valid_partitions_apply_identically() {
    let words = [
        "One.", "Two.", "Three.", "Four.", "Five.", "Six.", "Seven.", "Eight.",
    ];
    let mut compact = store(&words);
    let mut pretty = store(&words);
    let json = ranges(&(1..=8).map(|id| (id, id)).collect::<Vec<_>>());
    let formatted = serde_json::to_string_pretty(&TopicProposal::parse(&json).unwrap()).unwrap();
    let a = compact.prepare_topics(NOW + 100).unwrap().unwrap();
    let b = pretty.prepare_topics(NOW + 100).unwrap().unwrap();
    assert!(formatted.len() > b.output_allowance().unwrap() as usize);
    compact.apply_topics(&a, &json).unwrap();
    pretty.apply_topics(&b, &formatted).unwrap();
    assert_eq!(
        serde_json::to_value(records(&compact)).unwrap(),
        serde_json::to_value(records(&pretty)).unwrap()
    );
}
