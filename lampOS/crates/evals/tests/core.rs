//! Synthetic evidence only. None of these tests run a lamp or qualify acoustics.
use lamp_evals::core::Rejection;
use lamp_evals::model::*;
use lamp_evals::{Error, Ingest, Recorder};
use serde_json::{Value, json};

const END: u64 = 500_000;
const CANARY: &str = "private_words_person_location";

fn observation(turn: u64) -> Observation {
    Observation {
        epoch: 71,
        turn,
        time: SourceTime::UtcMinute((END - 2) * 60 + 17),
        build: Some(Build::parse("17001+abcdef123456").unwrap()),
        origin: Origin::Microphone,
        terminal: Terminal::Completed,
        audio_started: Some(true),
        endpoint_to_first_write_ms: Some(1500),
        contract: Contract::VoiceDeliveryV1,
    }
}

fn fixture() -> Recorder {
    let mut recorder = Recorder::new(END).unwrap();
    assert_eq!(recorder.observe(observation(1)), Ingest::Accepted);
    recorder
}

fn state(recorder: &Recorder) -> Value {
    serde_json::from_slice(&recorder.checkpoint().unwrap()).unwrap()
}

fn restore(value: &Value) -> Result<Recorder, Error> {
    Recorder::restore(&serde_json::to_vec(value).unwrap())
}

fn refuses(value: &Value) {
    assert!(matches!(restore(value), Err(Error::InvalidCheckpoint)));
}

fn outcome(counts: &AggregateCounts, terminal: Terminal) -> &OutcomeCounts {
    counts
        .outcomes
        .iter()
        .find(|outcome| outcome.terminal == terminal)
        .unwrap()
}

#[test]
fn replay_and_restart_restore_share_one_dedupe_checkpoint() {
    let mut recorder = fixture();
    assert_eq!(recorder.observe(observation(1)), Ingest::Duplicate);
    let bytes = recorder.checkpoint().unwrap();
    let mut restored = Recorder::restore(&bytes).unwrap();
    assert_eq!(restored.snapshot(), recorder.snapshot());
    assert_eq!(restored.observe(observation(1)), Ingest::Duplicate);
    let snapshot = restored.snapshot();
    assert_eq!(snapshot.hourly[0].counts.total, 1);
    assert_eq!(snapshot.receipts.calls, 3);
    assert_eq!(snapshot.receipts.accepted, 1);
    assert_eq!(snapshot.receipts.duplicates, 2);
    assert_eq!(
        Recorder::restore(&restored.checkpoint().unwrap())
            .unwrap()
            .snapshot(),
        snapshot
    );
}

#[test]
fn out_of_order_ids_merge_only_matching_segments() {
    let mut recorder = Recorder::new(END).unwrap();
    for id in [5, 1, 3, 2, 4, 0, 6] {
        assert_eq!(recorder.observe(observation(id)), Ingest::Accepted);
    }
    assert_eq!(state(&recorder)["epochs"][0]["ranges"], json!([[0, 6, 0]]));
    let mut other = observation(7);
    other.origin = Origin::Text;
    assert_eq!(recorder.observe(other), Ingest::Accepted);
    assert_eq!(recorder.observe(observation(8)), Ingest::Accepted);
    assert_eq!(
        state(&recorder)["epochs"][0]["ranges"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(recorder.snapshot().hourly.len(), 2);
    assert!(Recorder::restore(&recorder.checkpoint().unwrap()).is_ok());
}

#[test]
fn new_local_epoch_can_reuse_turn_ids_without_losing_replay_protection() {
    let mut recorder = fixture();
    let mut restarted = observation(1);
    restarted.epoch += 1;
    assert_eq!(recorder.observe(restarted.clone()), Ingest::Accepted);
    assert_eq!(recorder.observe(restarted), Ingest::Duplicate);
    assert_eq!(recorder.snapshot().hourly[0].counts.total, 2);
    assert_eq!(state(&recorder)["epochs"].as_array().unwrap().len(), 2);
}

#[test]
fn all_terminal_and_audio_unknown_partitions_are_preserved() {
    let mut recorder = Recorder::new(END).unwrap();
    for (index, terminal) in Terminal::ALL.into_iter().enumerate() {
        let mut value = observation(index as u64);
        value.terminal = terminal;
        value.audio_started = None;
        value.endpoint_to_first_write_ms = None;
        assert_eq!(recorder.observe(value), Ingest::Accepted);
    }
    let counts = &recorder.snapshot().hourly[0].counts;
    assert_eq!(counts.total, Terminal::ALL.len() as u64);
    for partition in &counts.outcomes {
        assert_eq!(partition.total, 1);
        assert_eq!(partition.audio_unknown, 1);
        assert_eq!(partition.audio_started, 0);
        assert_eq!(partition.timing.samples, 0);
        assert_eq!(partition.timing.unavailable, 0);
    }
}

#[test]
fn timing_is_conditional_and_boundary_bins_are_not_percentile_averages() {
    let mut recorder = Recorder::new(END).unwrap();
    let measurements = [0, 499, 500, 999, 1000, 1500, 2000, 3000, 5000, u32::MAX];
    for (id, milliseconds) in measurements.into_iter().enumerate() {
        let mut value = observation(id as u64);
        value.endpoint_to_first_write_ms = Some(milliseconds);
        assert_eq!(recorder.observe(value), Ingest::Accepted);
    }
    let mut unavailable = observation(10);
    unavailable.endpoint_to_first_write_ms = None;
    assert_eq!(recorder.observe(unavailable), Ingest::Accepted);
    for (id, audio) in [(11, None), (12, Some(false))] {
        let mut value = observation(id);
        value.audio_started = audio;
        assert_eq!(
            recorder.observe(value),
            Ingest::Rejected(Rejection::InvalidEvidence)
        );
    }
    let snapshot = recorder.snapshot();
    let completed = outcome(&snapshot.hourly[0].counts, Terminal::Completed);
    assert_eq!(completed.audio_started, 11);
    assert_eq!(completed.timing.samples, 10);
    assert_eq!(completed.timing.unavailable, 1);
    assert_eq!(completed.timing.bins, [2, 2, 1, 1, 1, 1, 2]);
    assert_eq!(
        completed.timing.boundary,
        "endpoint_decision_to_first_speaker_write"
    );
    assert_eq!(snapshot.receipts.rejected.invalid_evidence, 2);
    assert_eq!(snapshot.receipts.accepted, 11);
}

#[test]
fn different_outcomes_keep_separate_timing_denominators() {
    let mut recorder = fixture();
    let mut interrupted = observation(2);
    interrupted.terminal = Terminal::PersonInterrupt;
    interrupted.endpoint_to_first_write_ms = Some(7000);
    assert_eq!(recorder.observe(interrupted), Ingest::Accepted);
    let snapshot = recorder.snapshot();
    assert_eq!(
        outcome(&snapshot.hourly[0].counts, Terminal::Completed)
            .timing
            .bins[3],
        1
    );
    assert_eq!(
        outcome(&snapshot.hourly[0].counts, Terminal::PersonInterrupt)
            .timing
            .bins[6],
        1
    );
}

#[test]
fn unknown_time_and_unknown_build_are_never_filled_from_the_recorder_clock() {
    let mut recorder = Recorder::new(END).unwrap();
    let mut value = observation(1);
    value.time = SourceTime::Unknown;
    value.build = None;
    value.origin = Origin::Unknown;
    assert_eq!(recorder.observe(value.clone()), Ingest::Accepted);
    recorder.advance_window(END + 1).unwrap();
    value.turn = 2;
    assert_eq!(recorder.observe(value), Ingest::Accepted);
    let snapshot = recorder.snapshot();
    assert!(snapshot.hourly.is_empty());
    assert!(snapshot.daily.is_empty());
    assert_eq!(snapshot.unanchored.len(), 1);
    assert_eq!(snapshot.unanchored[0].counts.total, 2);
    assert_eq!(snapshot.unanchored[0].build, None);
    let public = serde_json::to_value(snapshot).unwrap();
    assert!(public["unanchored"][0].get("utc_hour").is_none());
    assert!(public["unanchored"][0].get("receipt_cohort").is_none());
    assert_eq!(state(&recorder)["segments"].as_array().unwrap().len(), 2);
}

#[test]
fn empty_evidence_stays_unknown_and_creates_no_buckets() {
    let snapshot = Recorder::new(END).unwrap().snapshot();
    assert_eq!(snapshot.coverage, Coverage::Unknown);
    assert_eq!(snapshot.opportunity_coverage, Coverage::Unknown);
    assert!(
        snapshot.hourly.is_empty() && snapshot.daily.is_empty() && snapshot.unanchored.is_empty()
    );
    assert_eq!(snapshot.receipts.calls, 0);
}

#[test]
fn builds_and_origins_remain_distinct_in_all_rollups() {
    let mut recorder = Recorder::new(END).unwrap();
    let origins = [
        Origin::Microphone,
        Origin::Reminder,
        Origin::Proactive,
        Origin::Text,
        Origin::Computer,
        Origin::Unknown,
    ];
    let builds = [
        None,
        Some(Build::parse("1+abcdef0").unwrap()),
        Some(Build::parse("2+abcdef0").unwrap()),
    ];
    let mut turn = 0;
    for origin in origins {
        for build in builds.clone() {
            let mut value = observation(turn);
            value.time = SourceTime::UtcMinute(((END - 1) / 24 * 24 - 1) * 60);
            value.origin = origin;
            value.build = build;
            assert_eq!(recorder.observe(value), Ingest::Accepted);
            turn += 1;
        }
    }
    let snapshot = recorder.snapshot();
    assert_eq!(snapshot.hourly.len(), 18);
    assert_eq!(snapshot.daily.len(), 18);
    assert!(
        snapshot
            .daily
            .iter()
            .all(|row| row.observed_closed_hours == 1 && row.counts.total == 1)
    );
}

#[test]
fn daily_counts_wait_for_utc_midnight_and_never_fill_missing_hours() {
    let midnight = END / 24 * 24;
    let end = midnight + 24; // The current/open hour is 23:00 of this UTC day.
    let mut recorder = Recorder::new(end).unwrap();
    let mut old = observation(1);
    old.time = SourceTime::UtcMinute((midnight - 1) * 60);
    assert_eq!(recorder.observe(old), Ingest::Accepted);
    let mut closed_today = observation(2);
    closed_today.time = SourceTime::UtcMinute((end - 2) * 60);
    assert_eq!(recorder.observe(closed_today), Ingest::Accepted);
    let mut current = observation(3);
    current.time = SourceTime::UtcMinute((end - 1) * 60 + 59);
    assert_eq!(recorder.observe(current), Ingest::Accepted);
    let before = recorder.snapshot();
    assert_eq!(before.daily.len(), 1);
    assert_eq!(before.daily[0].counts.total, 1);
    assert_eq!(before.daily[0].observed_closed_hours, 1);
    assert_eq!(before.daily[0].utc_day, midnight / 24 - 1);
    assert!(before.hourly[1].closed);
    assert!(!before.hourly[2].closed);
    recorder.advance_window(end + 1).unwrap(); // Current hour is next day 00:00.
    let after = recorder.snapshot();
    assert!(after.hourly.iter().all(|row| row.closed));
    assert_eq!(
        after.daily.iter().map(|row| row.counts.total).sum::<u64>(),
        3
    );
    assert_eq!(
        after
            .daily
            .iter()
            .map(|row| row.observed_closed_hours)
            .sum::<u8>(),
        3
    );
    assert!(
        after
            .daily
            .iter()
            .all(|row| row.coverage == Coverage::Unknown)
    );
    let completed = after
        .daily
        .iter()
        .find(|row| row.utc_day == midnight / 24)
        .unwrap();
    assert_eq!(completed.counts.total, 2);
    assert_eq!(completed.observed_closed_hours, 2);
}

#[test]
fn rolling_retention_can_clip_the_oldest_completed_utc_day() {
    let mut recorder = Recorder::new(END).unwrap();
    let start = END - WINDOW_HOURS;
    assert_ne!(start % 24, 0);
    for (turn, hour) in [(1, start), (2, start / 24 * 24 + 23)] {
        let mut value = observation(turn);
        value.time = SourceTime::UtcMinute(hour * 60);
        assert_eq!(recorder.observe(value), Ingest::Accepted);
    }
    let snapshot = recorder.snapshot();
    assert_eq!(snapshot.daily.len(), 1);
    assert_eq!(snapshot.daily[0].utc_day, start / 24);
    assert_eq!(snapshot.daily[0].observed_closed_hours, 2);
    assert_eq!(snapshot.daily[0].coverage, Coverage::Unknown);
    assert_eq!(snapshot.daily[0].counts.total, 2);
}

#[test]
fn retention_clock_rejects_backward_and_implausible_jumps_without_mutation() {
    let mut recorder = fixture();
    let before = recorder.checkpoint().unwrap();
    assert_eq!(recorder.advance_window(END - 1), Err(Error::BackwardWindow));
    assert_eq!(recorder.advance_window(END + 25), Err(Error::WindowJump));
    assert_eq!(recorder.advance_window(0), Err(Error::InvalidWindow));
    assert_eq!(
        recorder.advance_window(MAX_WINDOW_END_HOUR + 1),
        Err(Error::InvalidWindow)
    );
    assert_eq!(recorder.checkpoint().unwrap(), before);
    recorder.advance_window(END).unwrap();
    assert_eq!(recorder.checkpoint().unwrap(), before);
    assert!(matches!(Recorder::new(0), Err(Error::InvalidWindow)));
    assert!(matches!(Recorder::new(u64::MAX), Err(Error::InvalidWindow)));
}

#[test]
fn event_time_cannot_advance_retention_and_expired_replays_are_rejected_first() {
    let mut recorder = Recorder::new(END).unwrap();
    let mut oldest = observation(1);
    oldest.time = SourceTime::UtcMinute((END - WINDOW_HOURS) * 60);
    assert_eq!(recorder.observe(oldest.clone()), Ingest::Accepted);
    let mut future = observation(2);
    future.time = SourceTime::UtcMinute((END + 1000) * 60);
    assert_eq!(
        recorder.observe(future),
        Ingest::Rejected(Rejection::Future)
    );
    let mut invalid = observation(3);
    invalid.time = SourceTime::UtcMinute(u64::MAX);
    assert_eq!(
        recorder.observe(invalid),
        Ingest::Rejected(Rejection::InvalidTime)
    );
    assert_eq!(recorder.snapshot().window_end_hour, END);
    recorder.advance_window(END + 1).unwrap();
    assert_eq!(
        recorder.observe(oldest),
        Ingest::Rejected(Rejection::Expired)
    );
    let snapshot = recorder.snapshot();
    assert!(snapshot.hourly.is_empty());
    assert_eq!(snapshot.receipts.retired, 1);
    assert_eq!(snapshot.receipts.duplicates, 0);
    assert!(state(&recorder)["epochs"].as_array().unwrap().is_empty());
    assert!(Recorder::restore(&recorder.checkpoint().unwrap()).is_ok());
}

#[test]
fn unknown_receipt_cohorts_expire_without_creating_source_timestamps() {
    let mut recorder = Recorder::new(END).unwrap();
    let mut value = observation(1);
    value.time = SourceTime::Unknown;
    assert_eq!(recorder.observe(value), Ingest::Accepted);
    for step in 1..=30 {
        recorder.advance_window(END + 24 * step).unwrap();
    }
    let snapshot = recorder.snapshot();
    assert!(
        snapshot.hourly.is_empty() && snapshot.daily.is_empty() && snapshot.unanchored.is_empty()
    );
    assert_eq!(snapshot.receipts.accepted, 1);
    assert_eq!(snapshot.receipts.retired, 1);
    assert!(Recorder::restore(&recorder.checkpoint().unwrap()).is_ok());
}

#[test]
fn epoch_capacity_is_explicit_and_does_not_discard_existing_dedupe() {
    let mut recorder = Recorder::new(END).unwrap();
    for epoch in 0..MAX_EPOCHS as u64 {
        let mut value = observation(1);
        value.epoch = epoch;
        assert_eq!(recorder.observe(value), Ingest::Accepted);
    }
    let mut extra = observation(1);
    extra.epoch = MAX_EPOCHS as u64;
    assert_eq!(
        recorder.observe(extra),
        Ingest::Rejected(Rejection::EpochCapacity)
    );
    assert_eq!(recorder.observe(observation(1)), Ingest::Duplicate);
    assert_eq!(recorder.snapshot().receipts.rejected.epoch_capacity, 1);
    assert!(Recorder::restore(&recorder.checkpoint().unwrap()).is_ok());
}

#[test]
fn total_range_capacity_allows_a_bridge_but_never_evicts_retained_ids() {
    let mut recorder = Recorder::new(END).unwrap();
    for index in 0..MAX_RANGES as u64 {
        assert_eq!(recorder.observe(observation(index * 2)), Ingest::Accepted);
    }
    assert_eq!(
        recorder.observe(observation(100_000)),
        Ingest::Rejected(Rejection::DedupeCapacity)
    );
    assert_eq!(recorder.observe(observation(1)), Ingest::Accepted);
    assert_eq!(recorder.observe(observation(100_000)), Ingest::Accepted);
    assert_eq!(recorder.observe(observation(0)), Ingest::Duplicate);
    assert_eq!(recorder.observe(observation(8190)), Ingest::Duplicate);
    let snapshot = recorder.snapshot();
    assert_eq!(snapshot.receipts.accepted, MAX_RANGES as u64 + 2);
    assert_eq!(snapshot.receipts.rejected.dedupe_capacity, 1);
    assert!(Recorder::restore(&recorder.checkpoint().unwrap()).is_ok());
}

#[test]
fn range_cap_is_global_across_producer_epochs() {
    let mut recorder = Recorder::new(END).unwrap();
    for index in 0..MAX_RANGES as u64 {
        let mut value = observation(index / 2 * 2);
        value.epoch = index % 2;
        assert_eq!(recorder.observe(value), Ingest::Accepted);
    }
    let mut extra = observation(100_000);
    extra.epoch = 2;
    assert_eq!(
        recorder.observe(extra),
        Ingest::Rejected(Rejection::DedupeCapacity)
    );
    assert_eq!(state(&recorder)["epochs"].as_array().unwrap().len(), 2);
}

#[test]
fn segment_capacity_and_full_checkpoint_are_bounded() {
    let mut recorder = Recorder::new(END).unwrap();
    for index in 0..MAX_SEGMENTS as u64 {
        let mut value = observation(index);
        value.build = Some(Build::parse(&format!("{}+abcdef0", index + 1)).unwrap());
        assert_eq!(recorder.observe(value), Ingest::Accepted);
    }
    let mut extra = observation(MAX_SEGMENTS as u64);
    extra.build = Some(Build::parse("999999+abcdef0").unwrap());
    assert_eq!(
        recorder.observe(extra),
        Ingest::Rejected(Rejection::SegmentCapacity)
    );
    let bytes = recorder.checkpoint().unwrap();
    assert!(bytes.len() < MAX_CHECKPOINT_BYTES);
    assert_eq!(
        Recorder::restore(&bytes).unwrap().snapshot(),
        recorder.snapshot()
    );
    assert_eq!(recorder.snapshot().receipts.rejected.segment_capacity, 1);
}

#[test]
fn turn_id_boundaries_do_not_wrap_ranges() {
    let mut recorder = Recorder::new(END).unwrap();
    for id in [u64::MAX, 0, u64::MAX - 1, 1] {
        assert_eq!(recorder.observe(observation(id)), Ingest::Accepted);
    }
    assert_eq!(
        state(&recorder)["epochs"][0]["ranges"],
        json!([[0, 1, 0], [u64::MAX - 1, u64::MAX, 0]])
    );
    assert!(Recorder::restore(&recorder.checkpoint().unwrap()).is_ok());
}

#[test]
fn malformed_oversized_unknown_field_and_duplicate_key_checkpoints_fail_closed() {
    for bytes in [
        b"".as_slice(),
        b"[]",
        b"null",
        b"{}",
        b"{broken",
        b"{\"version\":1,\"version\":1}",
    ] {
        assert!(matches!(
            Recorder::restore(bytes),
            Err(Error::InvalidCheckpoint)
        ));
    }
    assert!(matches!(
        Recorder::restore(&vec![b' '; MAX_CHECKPOINT_BYTES + 1]),
        Err(Error::CheckpointTooLarge)
    ));
    let mut value = state(&fixture());
    value[CANARY] = json!(CANARY);
    refuses(&value);
    let mut value = state(&fixture());
    value["segments"][0]["key"]["message"] = json!(CANARY);
    refuses(&value);
    let mut value = state(&fixture());
    value["version"] = json!(2);
    refuses(&value);
}

#[test]
fn restored_arithmetic_and_attribution_must_match_dedupe_cardinality() {
    let changes: &[fn(&mut Value)] = &[
        |v| v["receipts"]["calls"] = json!(2),
        |v| v["receipts"]["accepted"] = json!(2),
        |v| v["receipts"]["retired"] = json!(1),
        |v| v["segments"][0]["counts"][0][0] = json!(2),
        |v| v["segments"][0]["counts"][0][3] = json!(2),
        |v| v["epochs"][0]["ranges"][0][1] = json!(2),
        |v| v["epochs"][0]["ranges"][0][2] = json!(1),
        |v| v["epochs"][0]["ranges"][0] = json!([0, u64::MAX, 0]),
        |v| v["epochs"][0]["ranges"] = json!([]),
        |v| v["segments"][0]["key"]["hour"] = json!(END),
        |v| v["segments"][0]["key"]["hour"] = json!(END - WINDOW_HOURS - 1),
        |v| v["receipts"]["exhausted"] = json!(true),
        |v| v["receipts"]["rejected"]["future"] = json!(u64::MAX),
    ];
    for change in changes {
        let mut value = state(&fixture());
        change(&mut value);
        refuses(&value);
    }
    let mut recorder = fixture();
    let mut other = observation(2);
    other.origin = Origin::Computer;
    assert_eq!(recorder.observe(other), Ingest::Accepted);
    let mut value = state(&recorder);
    value["epochs"][0]["ranges"][1][2] = json!(0);
    refuses(&value);
}

#[test]
fn checkpoint_requires_sorted_nonoverlapping_normalized_ranges_and_unique_segments() {
    let mut recorder = fixture();
    assert_eq!(recorder.observe(observation(3)), Ingest::Accepted);
    let mut value = state(&recorder);
    value["epochs"][0]["ranges"]
        .as_array_mut()
        .unwrap()
        .reverse();
    refuses(&value);
    let mut value = state(&recorder);
    value["epochs"][0]["ranges"][1][0] = json!(1);
    refuses(&value);
    let mut value = state(&recorder);
    value["epochs"][0]["ranges"][1] = json!([2, 2, 0]);
    refuses(&value);
    let mut value = state(&recorder);
    let epoch = value["epochs"][0].clone();
    value["epochs"].as_array_mut().unwrap().push(epoch);
    refuses(&value);
    let mut value = state(&recorder);
    let segment = value["segments"][0].clone();
    value["segments"].as_array_mut().unwrap().push(segment);
    refuses(&value);
}

#[test]
fn unknown_enums_builds_strings_and_numbers_cannot_survive_restoration() {
    for key in ["origin", "contract", "time_kind", "build", "hour"] {
        let mut value = state(&fixture());
        value["segments"][0]["key"][key] = json!(CANARY);
        let error = restore(&value).err().unwrap();
        assert_eq!(error.to_string(), "invalid_checkpoint");
        assert!(!error.to_string().contains(CANARY));
    }
    for token in [
        CANARY,
        "01+abcdef0",
        "0+abcdef0",
        "1+ABCDEF0",
        "1+abc",
        "1+abcdef0 words",
        "18446744073709551616+abcdef0",
    ] {
        assert_eq!(Build::parse(token), Err(Error::InvalidBuild));
    }
    assert!(Build::parse(&format!("1+{}", "f".repeat(41))).is_err());
    assert_eq!(
        Build::parse("18446744073709551615+abcdef0")
            .unwrap()
            .as_str(),
        "18446744073709551615+abcdef0"
    );
}

#[test]
fn public_snapshot_never_contains_local_identity_or_individual_time() {
    let mut recorder = Recorder::new(END).unwrap();
    let mut value = observation(8_735_468_923_456);
    value.epoch = 7_321_987_432_678;
    value.time = SourceTime::UtcMinute((END - 2) * 60 + 23);
    assert_eq!(recorder.observe(value), Ingest::Accepted);
    let public = serde_json::to_string(&recorder.snapshot()).unwrap();
    for forbidden in [
        "8735468923456",
        "7321987432678",
        "\"epoch\"",
        "\"turn\"",
        "\"receipt_cohort\"",
        "\"source_minute\"",
        CANARY,
    ] {
        assert!(
            !public.contains(forbidden),
            "forbidden field category: {forbidden}"
        );
    }
    assert!(!public.contains(&((END - 2) * 60 + 23).to_string()));
    assert!(public.contains("17001+abcdef123456"));
    assert!(public.contains("opportunity_coverage\":\"unknown"));
}

#[test]
fn exhausted_receipt_counters_fail_without_wrapping_or_mutating_outcomes() {
    let mut value = state(&fixture());
    value["receipts"]["calls"] = json!(u64::MAX);
    value["receipts"]["accepted"] = json!(u64::MAX);
    value["segments"][0]["counts"] = serde_json::to_value([[0_u64; 10]; 10]).unwrap();
    value["segments"][0]["counts"][0][0] = json!(u64::MAX);
    value["epochs"][0]["ranges"][0] = json!([1, u64::MAX, 0]);
    let mut recorder = restore(&value).unwrap();
    assert_eq!(
        recorder.observe(observation(0)),
        Ingest::Rejected(Rejection::CounterOverflow)
    );
    let snapshot = recorder.snapshot();
    assert_eq!(snapshot.receipts.calls, u64::MAX);
    assert_eq!(snapshot.receipts.accepted, u64::MAX);
    assert!(snapshot.receipts.exhausted);
    assert_eq!(snapshot.hourly[0].counts.total, u64::MAX);
    assert!(Recorder::restore(&recorder.checkpoint().unwrap()).is_ok());
}

#[test]
fn checkpoint_capacity_bounds_are_validated_before_state_is_exposed() {
    for (field, maximum) in [("segments", MAX_SEGMENTS), ("epochs", MAX_EPOCHS)] {
        let mut value = state(&fixture());
        let entry = value[field][0].clone();
        value[field] = Value::Array(vec![entry; maximum + 1]);
        refuses(&value);
    }
    let mut value = state(&fixture());
    value["epochs"][0]["ranges"] = Value::Array(vec![json!([1, 1, 0]); MAX_RANGES + 1]);
    refuses(&value);
}

#[test]
fn retirement_remaps_segments_and_preserves_retained_replay_after_restore() {
    let mut recorder = Recorder::new(END).unwrap();
    let mut old = observation(1);
    old.time = SourceTime::UtcMinute((END - WINDOW_HOURS) * 60);
    assert_eq!(recorder.observe(old), Ingest::Accepted);
    assert_eq!(recorder.observe(observation(2)), Ingest::Accepted);
    recorder.advance_window(END + 1).unwrap();
    let mut restored = Recorder::restore(&recorder.checkpoint().unwrap()).unwrap();
    assert_eq!(restored.observe(observation(2)), Ingest::Duplicate);
    assert_eq!(restored.snapshot().receipts.retired, 1);
    assert_eq!(state(&restored)["epochs"][0]["ranges"], json!([[2, 2, 0]]));
}

#[test]
fn dense_max_capacity_checkpoint_with_large_counters_stays_within_byte_bound() {
    let mut value = state(&fixture());
    let base = value["segments"][0].clone();
    // Populate every cell, near the total u64 count bound, with maximum-length
    // build identities. This checks space at the dense end as well as one-count
    // segments; no event replay or per-event allocation creates this fixture.
    let per_cell = u64::MAX / (MAX_SEGMENTS as u64 * 90);
    let per_segment = per_cell * 90;
    let counters = [[
        per_cell * 7,
        per_cell,
        per_cell,
        per_cell,
        per_cell,
        per_cell,
        per_cell,
        per_cell,
        per_cell,
        per_cell,
    ]; 10];
    let mut segments = Vec::new();
    let mut ranges = Vec::new();
    for index in 0..MAX_SEGMENTS as u64 {
        let mut segment = base.clone();
        segment["key"]["build"] = json!(format!("{}+{}", u64::MAX - index, "f".repeat(40)));
        segment["counts"] = serde_json::to_value(counters).unwrap();
        segments.push(segment);
        ranges.push(json!([
            index * per_segment + 1,
            (index + 1) * per_segment,
            index
        ]));
    }
    value["segments"] = Value::Array(segments);
    value["epochs"][0]["ranges"] = Value::Array(ranges);
    value["receipts"]["calls"] = json!(per_segment * MAX_SEGMENTS as u64);
    value["receipts"]["accepted"] = value["receipts"]["calls"].clone();
    let bytes = serde_json::to_vec(&value).unwrap();
    assert!(bytes.len() < MAX_CHECKPOINT_BYTES);
    let recorder = Recorder::restore(&bytes).unwrap();
    assert!(recorder.checkpoint().unwrap().len() < MAX_CHECKPOINT_BYTES);
    assert_eq!(recorder.snapshot().hourly.len(), MAX_SEGMENTS);
}
