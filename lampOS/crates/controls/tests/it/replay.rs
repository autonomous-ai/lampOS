use lamp_controls::{
    engine::Config,
    replay::{self, Frame, Record},
};
use lamp_msgs::{ControlFault, ControlIntent, Gesture};
use std::io::{self, Write};

fn fixture() -> (Config, Vec<Record>) {
    let config = serde_json::from_str(include_str!("../../fixtures/controls.json")).unwrap();
    let records =
        replay::read(include_bytes!("../../fixtures/hand-controls.jsonl").as_slice()).unwrap();
    (config, records)
}

fn frames(bytes: &[u8]) -> Vec<Frame> {
    std::str::from_utf8(bytes)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn written_controls_scenario_has_six_requests_and_no_false_hold_after_lost_input() {
    let (mut config, records) = fixture();
    config.enabled = true;
    let mut bytes = Vec::new();
    let summary = replay::run(config, "written-controls", &records, None, &mut bytes).unwrap();
    assert_eq!(
        (
            summary.records,
            summary.observations,
            summary.rejected,
            summary.faults,
            summary.gestures
        ),
        (901, 900, 0, 1, 6)
    );
    let output = frames(&bytes);
    let gestures: Vec<_> = output
        .iter()
        .flat_map(|frame| &frame.output.gestures)
        .collect();
    assert_eq!(
        gestures
            .iter()
            .map(|g| (g.stamp.at_us, g.gesture, g.intent))
            .collect::<Vec<_>>(),
        vec![
            (1_160_000, Gesture::Tap, ControlIntent::Stop),
            (1_410_000, Gesture::DoubleTap, ControlIntent::Stop),
            (1_690_000, Gesture::Tap, ControlIntent::Stop),
            (4_000_000, Gesture::LongPress, ControlIntent::SoftwareMute),
            (5_950_000, Gesture::Pet, ControlIntent::Pet),
            (8_580_000, Gesture::Tap, ControlIntent::Stop),
        ]
    );
    assert_eq!(
        output.last().unwrap().output.state.fault,
        Some(ControlFault::Stale)
    );
    for frame in output {
        frame.output.state.check().unwrap();
        for gesture in frame.output.gestures {
            gesture.check().unwrap();
        }
    }
}

#[test]
fn replay_is_byte_identical_for_the_same_inputs_and_has_no_clock_sleep() {
    let (config, records) = fixture();
    let mut first = Vec::new();
    let mut second = Vec::new();
    replay::run(
        config.clone(),
        "written-controls",
        &records,
        None,
        &mut first,
    )
    .unwrap();
    replay::run(config, "written-controls", &records, None, &mut second).unwrap();
    assert_eq!(first, second);
}

#[test]
fn shadow_reports_never_contain_an_enabled_request() {
    let (config, records) = fixture();
    let mut bytes = Vec::new();
    let summary = replay::run(config, "written-controls", &records, None, &mut bytes).unwrap();
    assert_eq!(summary.gestures, 6);
    for frame in frames(&bytes) {
        assert!(!frame.output.state.enabled);
        for gesture in frame.output.gestures {
            assert!(!gesture.fresh_at("written-controls", frame.now_us));
        }
    }
}

#[test]
fn malformed_records_empty_input_and_mixed_fault_acquisition_are_refused() {
    for input in [
        "",
        "\n",
        "not-json\n",
        "{\"now_us\":1,\"extra\":true}\n",
        "{\"now_us\":2}\n{\"now_us\":1}\n",
    ] {
        assert!(replay::read(input.as_bytes()).is_err(), "{input:?}");
    }
    let (_, mut records) = fixture();
    records[0].fault = Some(ControlFault::Invalid);
    let line = serde_json::to_string(&records[0]).unwrap();
    assert!(replay::read(line.as_bytes()).is_err());
}

#[test]
fn byte_line_record_and_duration_limits_are_enforced_before_running() {
    assert!(replay::read(vec![b' '; (replay::MAX_BYTES + 1) as usize].as_slice()).is_err());
    let line = format!(
        "{{\"now_us\":0,\"extra\":\"{}\"}}\n",
        "x".repeat(replay::MAX_LINE_BYTES)
    );
    assert!(replay::read(line.as_bytes()).is_err());
    let many = "{\"now_us\":0}\n".repeat(replay::MAX_LINES + 1);
    assert!(replay::read(many.as_bytes()).is_err());
    assert!(replay::read(b"{\"now_us\":0}\n{\"now_us\":600000001}\n".as_slice()).is_err());
}

#[test]
fn end_bounds_cannot_truncate_inputs_or_overflow_tick_time() {
    let (config, records) = fixture();
    for end in [0, 9_999_999, 601_000_001, u64::MAX] {
        assert!(
            replay::run(
                config.clone(),
                "written-controls",
                &records,
                Some(end),
                io::sink()
            )
            .is_err()
        );
    }
    let records = vec![Record {
        now_us: u64::MAX,
        observation: None,
        fault: None,

        privacy: None,
        privacy_fault: None,
    }];
    assert!(replay::run(config, "written-controls", &records, None, io::sink()).is_err());
}

#[test]
fn invalid_acquisition_stamps_are_preserved_as_rejected_evidence() {
    let (config, mut records) = fixture();
    records[10].observation.as_mut().unwrap().stamp.at_us = records[10].now_us + 1;
    let mut bytes = Vec::new();
    let summary = replay::run(config, "written-controls", &records, None, &mut bytes).unwrap();
    assert!(summary.rejected >= 1);
    assert!(
        frames(&bytes)
            .iter()
            .any(|frame| frame.output.state.fault == Some(ControlFault::Future))
    );
}

#[test]
fn a_terminal_fault_cancels_a_pending_pet_instead_of_fabricating_it_in_the_tail() {
    let (config, all) = fixture();
    let prefix: Vec<_> = all
        .into_iter()
        .take_while(|record| record.now_us <= 5_940_000)
        .collect();
    let mut before = Vec::new();
    replay::run(
        config.clone(),
        "written-controls",
        &prefix,
        None,
        &mut before,
    )
    .unwrap();
    assert!(
        frames(&before)
            .iter()
            .flat_map(|f| &f.output.gestures)
            .any(|g| g.intent == ControlIntent::Pet)
    );
    let mut stopped = prefix;
    stopped.push(Record {
        now_us: 5_940_000,
        observation: None,
        fault: Some(ControlFault::Overflow),

        privacy: None,
        privacy_fault: None,
    });
    let mut after = Vec::new();
    replay::run(config, "written-controls", &stopped, None, &mut after).unwrap();
    assert!(
        !frames(&after)
            .iter()
            .flat_map(|f| &f.output.gestures)
            .any(|g| g.intent == ControlIntent::Pet)
    );
}

struct BrokenWriter;
impl Write for BrokenWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("injected output failure"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn output_failure_is_returned_instead_of_reporting_a_complete_replay() {
    let (config, records) = fixture();
    assert!(replay::run(config, "written-controls", &records, None, BrokenWriter).is_err());
}

#[test]
fn a_full_ten_minute_clip_does_not_need_an_explicit_end_override() {
    let (config, _) = fixture();
    let records = [
        Record {
            now_us: 1_000_000,
            observation: None,
            fault: None,

            privacy: None,
            privacy_fault: None,
        },
        Record {
            now_us: 601_000_000,
            observation: None,
            fault: None,

            privacy: None,
            privacy_fault: None,
        },
    ];
    let summary = replay::run(config, "written-controls", &records, None, io::sink()).unwrap();
    assert_eq!(summary.end_us, 601_000_000);
}
