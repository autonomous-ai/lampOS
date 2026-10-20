//! Authored clocks, not a captured lamp trace. Payload stamps represent
//! publication; Record.at_us separately represents injected receipt time.
//! These tests establish planned light/body requests, not physical delivery.
use lamp_expression::{
    Catalogue, Engine, TICK_US,
    replay::{self, Record, Summary, WarningKind},
};
use serde_json::Value;

const CLOCK: &str = "recent-playout-fixture";
const RECENT: &str = include_str!("../../fixtures/recent-playout.jsonl");
const GENERATION: &str = include_str!("../../fixtures/recent-playout-generation.jsonl");
const FOLLOW: &str = "follow played sound";
const EXPIRED: &str = "playout is expired, too far ahead, overlapping or out of order";

fn catalogue(resumable: bool) -> Catalogue {
    let mut catalogue =
        Catalogue::from_json(include_str!("../../playout-behaviours.json")).unwrap();
    catalogue.resumable_voice = resumable;
    catalogue
}

fn run(text: &str, catalogue: Catalogue, enabled: bool) -> (Summary, Vec<Value>, Vec<u8>) {
    let mut engine = Engine::new(catalogue, enabled, CLOCK).unwrap();
    let mut written = Vec::new();
    let summary = replay::run(text.as_bytes(), &mut written, &mut engine, None).unwrap();
    let ticks = written
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    (summary, ticks, written)
}

fn tick(ticks: &[Value], at_us: u64) -> &Value {
    &ticks.iter().find(|tick| tick["at_us"] == at_us).unwrap()["output"]
}

fn follows(output: &Value) -> bool {
    output["state"]["behaviours"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == FOLLOW)
}

fn brightness(output: &Value, expected: f64) {
    let actual = output["light"]["brightness"].as_f64().unwrap();
    assert!(
        (actual - expected).abs() < 0.000_002,
        "brightness {actual}, expected {expected}"
    );
}

fn records(text: &str) -> Vec<Record> {
    text.lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn authored_fixtures_have_exact_counts_and_only_named_intentional_refusals() {
    let (legacy, ticks, bytes) = run(RECENT, catalogue(false), true);
    assert_eq!(
        (legacy.input_records, legacy.accepted, legacy.rejected),
        (15, 15, 0)
    );
    assert!(legacy.warnings.is_empty());
    assert_eq!((legacy.ticks, legacy.last_tick_us), (31, Some(600_000)));
    assert_eq!(legacy.ticks, ticks.len());
    assert_eq!(bytes, run(RECENT, catalogue(false), true).2);

    let (generation, ticks, bytes) = run(GENERATION, catalogue(true), true);
    assert_eq!(
        (
            generation.input_records,
            generation.accepted,
            generation.rejected
        ),
        (19, 13, 6)
    );
    assert_eq!(
        (generation.ticks, generation.last_tick_us),
        (23, Some(440_000))
    );
    assert_eq!(generation.ticks, ticks.len());
    let expected = [
        (
            8,
            "sound is held or belongs to an inactive audio generation",
        ),
        (
            9,
            "sound is held or belongs to an inactive audio generation",
        ),
        (
            11,
            "sound is held or belongs to an inactive audio generation",
        ),
        (12, "sound predates the accepted generation or resume edge"),
        (16, "turn audio was cancelled, finished or expired"),
        (17, "a retired voice turn cannot resume audio or thinking"),
    ];
    assert_eq!(generation.warnings.len(), expected.len());
    for (warning, (line, detail)) in generation.warnings.iter().zip(expected) {
        assert_eq!(warning.kind, WarningKind::RejectedInput);
        assert_eq!(warning.line, Some(line));
        assert_eq!(warning.detail, detail);
    }
    assert_eq!(bytes, run(GENERATION, catalogue(true), true).2);
}

#[test]
fn publication_receipt_and_next_tick_are_separate_authored_boundaries() {
    let input = records(RECENT);
    let (_, ticks, _) = run(RECENT, catalogue(false), true);
    for (index, start, end, published, arrived, next_tick, expected) in [
        (4, 0, 20_000, 1_700, 1_700, 20_000, 0.7),
        (5, 20_000, 40_000, 21_700, 41_700, 60_000, 0.46),
        (6, 40_000, 60_000, 41_700, 61_700, 80_000, 0.38),
    ] {
        let lamp_expression::Input::Playout(frame) = &input[index].input else {
            panic!("fixture row must remain a playout frame");
        };
        assert_eq!((frame.start_us, frame.end_us), (start, end));
        assert_eq!(frame.stamp.at_us, published);
        assert_eq!(frame.stamp.at_us - frame.start_us, 1_700);
        assert_eq!(input[index].at_us, arrived);
        assert_eq!(next_tick - arrived, 18_300);
        assert_eq!(next_tick % TICK_US, 0);
        assert_eq!(frame.end_us - frame.start_us, TICK_US);
        brightness(tick(&ticks, next_tick), expected);
        assert!(follows(tick(&ticks, next_tick)));
    }
    // The two delayed receipts author 20 ms of transport delay. No lamp
    // transport observation is inferred from the 1.7 ms publication offset.
    assert_eq!(input[5].at_us - input[5].input.stamp().at_us, 20_000);
    assert_eq!(input[6].at_us - input[6].input.stamp().at_us, 20_000);
    // At 40 ms the second frame has not arrived: only the first frame's
    // decayed level can be used. Receipt later does not rewrite this output.
    brightness(tick(&ticks, 40_000), 0.62);
}

#[test]
fn current_zero_replaces_recent_sound_and_future_frames_never_play_early() {
    let (_, ticks, _) = run(RECENT, catalogue(false), true);
    for at_us in [100_000, 120_000, 140_000, 160_000, 180_000] {
        brightness(tick(&ticks, at_us), 0.3);
        assert!(!follows(tick(&ticks, at_us)), "at {at_us}");
    }
    brightness(tick(&ticks, 200_000), 0.62);
    assert!(follows(tick(&ticks, 200_000)));
    // The zero frame arrives at 91.7 ms, so its 100 ms tick is 8.3 ms later.
    assert_eq!(100_000 - records(RECENT)[7].at_us, 8_300);
}

#[test]
fn recent_level_fades_from_immutable_end_and_heartbeats_cannot_extend_it() {
    let (_, ticks, _) = run(RECENT, catalogue(false), true);
    for (at_us, expected) in [
        (220_000, 0.62),
        (240_000, 0.556),
        (280_000, 0.428),
        (300_000, 0.364),
    ] {
        brightness(tick(&ticks, at_us), expected);
        assert!(follows(tick(&ticks, at_us)));
    }
    // The Voice heartbeat at 300 ms cannot renew the end=220 ms sample.
    for at_us in (320_000..=400_000).step_by(TICK_US as usize) {
        brightness(tick(&ticks, at_us), 0.3);
        assert!(!follows(tick(&ticks, at_us)));
        assert!(
            tick(&ticks, at_us)["state"]["missing"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == "fresh current or recent playout level")
        );
    }
}

#[test]
fn hold_and_terminal_edges_clear_recent_level_and_queued_old_generation() {
    let (_, ticks, _) = run(GENERATION, catalogue(true), true);
    brightness(tick(&ticks, 20_000), 0.7);
    for at_us in [60_000, 80_000, 100_000] {
        let output = tick(&ticks, at_us);
        assert_eq!(output["light"]["face"], "listening");
        assert!(!follows(output));
    }
    // Resume precedes any valid new sound. The queued full-level 120..140
    // ms generation-one interval must not appear at its original start.
    brightness(tick(&ticks, 120_000), 0.3);
    assert!(!follows(tick(&ticks, 120_000)));
    brightness(tick(&ticks, 140_000), 0.380_001);
    assert!(follows(tick(&ticks, 140_000)));
    for at_us in (200_000..=300_000).step_by(TICK_US as usize) {
        assert!(!follows(tick(&ticks, at_us)));
        assert_eq!(tick(&ticks, at_us)["light"]["face"], "listening");
    }
}

#[test]
fn ended_frames_are_not_queued_and_hold_discards_the_one_future_frame() {
    for (text, resumable, expected_refused) in [(RECENT, false, 0), (GENERATION, true, 6)] {
        let mut engine = Engine::new(catalogue(resumable), true, CLOCK).unwrap();
        let mut refused = 0;
        for record in records(text) {
            if engine.ingest(record.at_us, record.input).is_err() {
                refused += 1;
            }
            assert!(engine.queued_playout_frames() <= 1);
            if resumable && (41_001..=121_701).contains(&record.at_us) {
                assert_eq!(engine.queued_playout_frames(), 0);
            }
            if !resumable && [41_700, 61_700].contains(&record.at_us) {
                assert_eq!(engine.queued_playout_frames(), 0);
            }
        }
        assert_eq!(refused, expected_refused);
        assert_eq!(engine.queued_playout_frames(), 0);
    }
}

#[test]
fn existing_catalogues_keep_strict_default_and_same_output_when_window_is_zero() {
    assert_eq!(Catalogue::builtin().timing.recent_playout_us, 0);
    assert_eq!(
        Catalogue::from_json(include_str!("../../rest-behaviours.json"))
            .unwrap()
            .timing
            .recent_playout_us,
        0
    );
    let (summary, ticks, strict) = run(RECENT, Catalogue::builtin(), true);
    let mut disabled_recent = catalogue(false);
    disabled_recent.timing.recent_playout_us = 0;
    assert_eq!(strict, run(RECENT, disabled_recent, true).2);
    assert_eq!((summary.accepted, summary.rejected), (13, 2));
    assert_eq!(summary.warnings.len(), 2);
    for (warning, line) in summary.warnings.iter().zip([6, 7]) {
        assert_eq!(warning.line, Some(line));
        assert_eq!(warning.detail, EXPIRED);
    }
    assert!(!follows(tick(&ticks, 20_000)));
    assert!(!follows(tick(&ticks, 60_000)));
    assert!(follows(tick(&ticks, 200_000)));
    assert!(!follows(tick(&ticks, 220_000)));
}

#[test]
fn opted_in_shadow_replay_has_the_same_admission_and_no_actuator_requests() {
    for (text, resumable) in [(RECENT, false), (GENERATION, true)] {
        let (enabled_summary, enabled, _) = run(text, catalogue(resumable), true);
        let (shadow_summary, shadow, _) = run(text, catalogue(resumable), false);
        assert_eq!(enabled_summary.accepted, shadow_summary.accepted);
        assert_eq!(enabled_summary.rejected, shadow_summary.rejected);
        for (enabled, shadow) in enabled.iter().zip(shadow) {
            assert_eq!(enabled["at_us"], shadow["at_us"]);
            assert!(shadow["output"]["body"].is_null());
            assert!(shadow["output"]["light"].is_null());
            assert_eq!(shadow["output"]["state"]["enabled"], false);
            assert_eq!(
                enabled["output"]["state"]["behaviours"],
                shadow["output"]["state"]["behaviours"]
            );
        }
    }
}
