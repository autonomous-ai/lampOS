use lamp_mood::replay::{
    DAY_SECONDS, MAX_EVENTS, PROVENANCE, Replay, ReplayRecord, SAMPLE_COUNT, SAMPLE_SECONDS,
    run_authored_day,
};
use lamp_msgs::{AttentionTarget, Familiarity, MoodState};

fn state(replay: &Replay, hour: usize, minute: usize) -> &MoodState {
    &replay.samples[hour * 60 + minute].state
}

fn close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.000_01,
        "{actual} versus {expected}"
    );
}

#[test]
fn complete_day_is_bounded_and_explicitly_synthetic() {
    let replay = run_authored_day().unwrap();
    assert_eq!(replay.metadata.provenance, PROVENANCE);
    assert_eq!(replay.metadata.duration_s, DAY_SECONDS);
    assert_eq!(replay.samples.len(), SAMPLE_COUNT);
    assert_eq!(replay.summary.samples, SAMPLE_COUNT);
    assert!(replay.summary.applied_events < MAX_EVENTS);
    assert_eq!(
        replay.summary.event_counts.values().sum::<u32>(),
        replay.summary.applied_events
    );
    assert_eq!(replay.phases.first().unwrap().start_s, 0);
    assert_eq!(replay.phases.last().unwrap().end_s, DAY_SECONDS);
    for pair in replay.phases.windows(2) {
        assert_eq!(pair[0].end_s, pair[1].start_s);
    }
    for (index, sample) in replay.samples.iter().enumerate() {
        assert_eq!(sample.provenance, PROVENANCE);
        assert_eq!(sample.elapsed_s, index as u64 * SAMPLE_SECONDS);
        assert_eq!(sample.local_minute, (index % 1_440) as u16);
        assert_eq!(sample.state.stamp.sequence, index as u64 + 1);
        sample.state.check().unwrap();
        assert!(sample.state.energy <= 0.70);
        assert!((0.60..=0.85).contains(&sample.state.warmth));
        assert_eq!(sample.state.familiarity, Familiarity::Unknown);
        if sample.state.asleep {
            close(sample.state.awake, 0.0);
            assert_eq!(sample.state.attention, None);
        }
        if let Some(AttentionTarget::Face { publisher_boot, id }) = &sample.state.attention {
            assert_eq!(publisher_boot, replay.metadata.face_publisher);
            assert_eq!(id, replay.metadata.face_id);
        }
    }
}

#[test]
fn dawn_company_noise_and_replay_feedback_do_not_wake_or_warm() {
    let replay = run_authored_day().unwrap();
    for sample in &replay.samples[..8 * 60] {
        assert!(sample.state.asleep, "{}", sample.elapsed_s);
        close(sample.state.energy, 0.30);
        close(sample.state.warmth, 0.60);
    }
    let named = state(&replay, 8, 0);
    assert!(!named.asleep);
    close(named.awake, 0.85);
    assert!(matches!(
        named.attention,
        Some(AttentionTarget::Face { .. })
    ));
    assert_eq!(state(&replay, 8, 1).attention, None);
    close(state(&replay, 9, 30).awake, 0.85);
    for sample in &replay.samples[..13 * 60 + 31] {
        close(sample.state.warmth, 0.60);
    }
}

#[test]
fn quiet_dozes_and_only_authored_direct_causes_clear_the_latch() {
    let replay = run_authored_day().unwrap();
    for (hour, minute) in [
        (11, 0),
        (11, 5),
        (13, 0),
        (13, 5),
        (15, 20),
        (20, 31),
        (22, 5),
        (24, 0),
    ] {
        assert!(state(&replay, hour, minute).asleep, "{hour}:{minute}");
    }
    for (hour, minute) in [(11, 15), (13, 30), (16, 0)] {
        let value = state(&replay, hour, minute);
        assert!(!value.asleep, "{hour}:{minute}");
        close(value.awake, 0.85);
    }
    assert_eq!(
        state(&replay, 11, 15).attention,
        Some(AttentionTarget::Touch)
    );
    assert_eq!(state(&replay, 13, 30).attention, None);
    assert!(state(&replay, 19, 20).awake < state(&replay, 18, 50).awake - 0.25);
    assert!(!state(&replay, 19, 20).asleep);
    assert!(state(&replay, 10, 45).awake < state(&replay, 10, 15).awake);
}

#[test]
fn kindly_touch_and_explicit_feedback_warm_then_return_to_baseline() {
    let replay = run_authored_day().unwrap();
    let touch = state(&replay, 13, 31).warmth;
    let thanks = state(&replay, 13, 32).warmth;
    let laughter = state(&replay, 13, 33).warmth;
    close(touch, 0.65);
    assert!(thanks > touch);
    assert!(laughter > thanks);
    assert!(state(&replay, 14, 33).warmth < laughter);
    close(state(&replay, 24, 0).warmth, 0.60);
}

#[test]
fn air_qualifies_for_ten_minutes_and_bangs_remain_brief_with_cooldown() {
    let replay = run_authored_day().unwrap();
    close(state(&replay, 14, 9).energy, 0.30);
    close(state(&replay, 14, 10).energy, 0.30);
    close(state(&replay, 14, 15).energy, 0.26);
    assert!(state(&replay, 14, 30).energy < 0.23);
    assert!(state(&replay, 14, 35).energy > state(&replay, 14, 30).energy);
    assert!(state(&replay, 14, 45).energy > state(&replay, 14, 35).energy);
    close(state(&replay, 17, 0).energy, 0.55);
    // The 17:00:30 bang is within cooldown, so only the original pulse decays.
    close(state(&replay, 17, 1).energy, 0.30 + 0.25 / 16.0);
    assert!(state(&replay, 17, 2).energy > 0.55);
    assert!(state(&replay, 17, 5).energy < 0.301);
}

#[test]
fn replay_and_jsonl_records_are_reproducible_and_contain_no_words_or_face_history() {
    let first = run_authored_day().unwrap();
    let second = run_authored_day().unwrap();
    assert_eq!(first, second);
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
    let sample = serde_json::to_value(ReplayRecord::Sample(&first.samples[8 * 60])).unwrap();
    assert_eq!(sample["record"], "sample");
    assert_eq!(sample["provenance"], PROVENANCE);
    assert_eq!(sample["elapsed_s"], 8 * 60 * 60);
    assert!(sample["awake"].is_number());
    assert!(sample.get("text").is_none());
    assert!(sample.get("faces").is_none());
    assert!(sample.get("history").is_none());
    assert_eq!(sample["familiarity"], "unknown");
    let metadata = serde_json::to_value(ReplayRecord::Metadata(&first.metadata)).unwrap();
    assert_eq!(metadata["record"], "metadata");
    assert!(
        metadata["limitation"]
            .as_str()
            .unwrap()
            .contains("not a recorded day")
    );
    assert_eq!(first.summary.event_counts["thanks_replay_only"], 2);
    assert_eq!(first.summary.event_counts["laughter_replay_only"], 2);
}
