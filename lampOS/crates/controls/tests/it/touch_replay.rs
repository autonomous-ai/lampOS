use lamp_controls::{
    input::Level,
    touch::{GpioSample, StripSample, TouchConfig},
    touch_replay::{self, Frame, Input, Record},
};
use lamp_msgs::{ClockStamp, ControlFault, TouchGesture, TouchIntent, TouchSource};
use serde::Deserialize;
use std::io::{self, Write};

const CLOCK: &str = "touch-written";

fn config(enabled: bool) -> TouchConfig {
    TouchConfig {
        enabled,
        primary: None,
        reset: None,
        head_pads: Vec::new(),
        strip_pads: Some((0..12).collect()),
        ..TouchConfig::default()
    }
}

fn stamp(at_us: u64, sequence: u64) -> ClockStamp {
    ClockStamp {
        clock_id: CLOCK.into(),
        at_us,
        sequence,
    }
}

fn strip(at_us: u64, sequence: u64, mask: u16) -> Record {
    Record {
        now_us: at_us,
        input: Input::Strip {
            sample: StripSample {
                stamp: stamp(at_us, sequence),
                session_id: "written-strip".into(),
                mask,
            },
        },
    }
}

fn gpio(at_us: u64, sequence: u64, head: bool) -> Record {
    Record {
        now_us: at_us,
        input: Input::Gpio {
            sample: GpioSample {
                stamp: stamp(at_us, sequence),
                session_id: "written-gpio".into(),
                primary: None,
                reset: None,
                head_pads: vec![Level {
                    name: "head-1".into(),
                    active: head,
                }],
            },
        },
    }
}

fn held(end: u64) -> Vec<Record> {
    (0..=end)
        .step_by(20_000)
        .enumerate()
        .map(|(index, at)| strip(at, index as u64 + 1, if at >= 200_000 { 7 } else { 0 }))
        .collect()
}

fn frames(bytes: &[u8]) -> Vec<Frame> {
    std::str::from_utf8(bytes)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn run(
    cfg: TouchConfig,
    records: &[Record],
    until: Option<u64>,
) -> (touch_replay::Summary, Vec<Frame>) {
    let mut bytes = Vec::new();
    let summary = touch_replay::run(cfg, CLOCK, records, until, &mut bytes).unwrap();
    let output = frames(&bytes);
    for frame in &output {
        frame.output.state.check().unwrap();
        for event in &frame.output.events {
            event.check().unwrap();
        }
    }
    (summary, output)
}

#[test]
fn strip_hold_is_one_touch_one_sleep_and_tail_expiry_is_unknown_not_release() {
    let records = held(2_500_000);
    let (summary, output) = run(config(true), &records, None);
    assert_eq!(
        (
            summary.gpio_samples,
            summary.strip_samples,
            summary.rejected,
            summary.faults,
            summary.events
        ),
        (0, 126, 0, 0, 2)
    );
    let events: Vec<_> = output
        .iter()
        .flat_map(|frame| &frame.output.events)
        .collect();
    assert_eq!(events[0].gesture, TouchGesture::Touch);
    assert_eq!(
        (events[0].when_speaking, events[0].when_quiet),
        (TouchIntent::Stop, TouchIntent::Attend)
    );
    assert_eq!(events[1].gesture, TouchGesture::Hold);
    assert_eq!(events[1].when_quiet, TouchIntent::Sleep);
    assert_eq!(events[1].observed_us, 2_200_000);
    assert_eq!(events[1].released_us, None);
    let final_source = &output.last().unwrap().output.state.sources[0];
    assert_eq!(final_source.fault, Some(ControlFault::Stale));
    assert_eq!(final_source.active, None);
    assert_eq!(final_source.position, None);
}

#[test]
fn replay_is_byte_identical_and_shadow_requests_remain_disabled() {
    let records = held(2_500_000);
    let mut first = Vec::new();
    let mut second = Vec::new();
    touch_replay::run(config(false), CLOCK, &records, None, &mut first).unwrap();
    touch_replay::run(config(false), CLOCK, &records, None, &mut second).unwrap();
    assert_eq!(first, second);
    let (_, enabled) = run(config(true), &records, None);
    let shadow = frames(&first);
    let shadow_events: Vec<_> = shadow.iter().flat_map(|f| &f.output.events).collect();
    let enabled_events: Vec<_> = enabled.iter().flat_map(|f| &f.output.events).collect();
    assert_eq!(shadow_events.len(), enabled_events.len());
    for (shadow, enabled) in shadow_events.into_iter().zip(enabled_events) {
        assert!(!shadow.enabled);
        assert!(!shadow.fresh_at(CLOCK, shadow.stamp.at_us));
        let mut comparable = shadow.clone();
        comparable.enabled = true;
        assert_eq!(&comparable, enabled);
    }
}

#[test]
fn equal_receipt_fault_after_acquisition_cancels_unpublished_action_before_tick() {
    let mut records = held(2_180_000);
    records.push(strip(2_200_000, 111, 7));
    records.push(Record {
        now_us: 2_200_000,
        input: Input::StripFault {
            fault: ControlFault::Missing,
        },
    });
    let (summary, output) = run(config(true), &records, Some(2_200_000));
    assert_eq!(summary.faults, 1);
    assert_eq!(summary.events, 1);
    assert!(
        !output
            .iter()
            .flat_map(|f| &f.output.events)
            .any(|e| e.when_quiet == TouchIntent::Sleep)
    );
    assert_eq!(
        output.last().unwrap().output.state.sources[0].fault,
        Some(ControlFault::Missing)
    );
}

#[test]
fn between_tick_inputs_keep_exact_receipt_and_emit_at_next_two_millisecond_tick() {
    let records = vec![
        strip(0, 1, 0),
        strip(10_000, 2, 0),
        strip(20_001, 3, 7),
        strip(25_001, 4, 7),
        strip(30_001, 5, 7),
        strip(35_001, 6, 7),
    ];
    let (summary, output) = run(config(true), &records, Some(38_000));
    assert_eq!(summary.rejected, 0);
    let frame = output
        .iter()
        .find(|frame| !frame.output.events.is_empty())
        .unwrap();
    assert_eq!(frame.now_us, 36_000);
    assert_eq!(frame.output.events[0].stamp.at_us, 35_001);
    assert_eq!(frame.output.events[0].observed_us, 35_001);
    assert_eq!(frame.output.events[0].began_us, 20_001);
}

#[test]
fn blocked_or_faulted_strip_does_not_refresh_or_cancel_independent_gpio() {
    let mut cfg = config(true);
    cfg.head_pads = vec!["head-1".into()];
    let mut records = Vec::new();
    for (index, at) in (0..=1_700_000).step_by(20_000).enumerate() {
        records.push(gpio(at, index as u64 + 1, at >= 600_000));
        if at <= 500_000 {
            records.push(strip(at, index as u64 + 1, 0));
        }
        if at == 600_000 {
            records.push(Record {
                now_us: at,
                input: Input::StripFault {
                    fault: ControlFault::Missing,
                },
            });
        }
    }
    let (summary, output) = run(cfg, &records, Some(1_700_000));
    assert_eq!(
        (summary.rejected, summary.faults, summary.events),
        (0, 1, 2)
    );
    let events: Vec<_> = output
        .iter()
        .flat_map(|frame| &frame.output.events)
        .collect();
    assert!(events.iter().all(|event| event.source == TouchSource::Head));
    assert_eq!(events[0].observed_us, 620_000);
    assert_eq!(events[1].gesture, TouchGesture::Stroke);
    let sources = &output.last().unwrap().output.state.sources;
    assert!(
        sources
            .iter()
            .find(|s| s.source == TouchSource::Head)
            .unwrap()
            .ready
    );
    assert!(
        !sources
            .iter()
            .find(|s| s.source == TouchSource::Strip)
            .unwrap()
            .ready
    );
}

#[test]
fn all_fault_cancels_both_sources_without_fabricating_released_levels() {
    let mut cfg = config(true);
    cfg.head_pads = vec!["head-1".into()];
    let mut records = vec![gpio(0, 1, false), strip(0, 1, 0)];
    records.push(Record {
        now_us: 2_000,
        input: Input::AllFault {
            fault: ControlFault::Overflow,
        },
    });
    let (summary, output) = run(cfg, &records, Some(2_000));
    assert_eq!((summary.faults, summary.events), (1, 0));
    for source in &output.last().unwrap().output.state.sources {
        assert_eq!(source.fault, Some(ControlFault::Overflow));
        assert_eq!(source.active, None);
    }
}

#[test]
fn duplicate_future_wrong_clock_and_lost_sequence_are_retained_as_rejection_evidence() {
    for expected in [
        ControlFault::OutOfOrder,
        ControlFault::Future,
        ControlFault::WrongClock,
        ControlFault::Overflow,
    ] {
        let mut records = held(80_000);
        let mut bad = strip(100_000, 6, 7);
        let Input::Strip { sample } = &mut bad.input else {
            unreachable!()
        };
        match expected {
            ControlFault::OutOfOrder => sample.stamp.at_us = 80_000,
            ControlFault::Future => sample.stamp.at_us = 100_001,
            ControlFault::WrongClock => sample.stamp.clock_id = "other-boot".into(),
            ControlFault::Overflow => sample.stamp.sequence = 7,
            _ => unreachable!(),
        }
        records.push(bad);
        let text = records
            .iter()
            .map(|r| serde_json::to_string(r).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        let parsed = touch_replay::read(text.as_bytes()).unwrap();
        let (summary, output) = run(config(true), &parsed, Some(100_000));
        assert_eq!(summary.rejected, 1);
        assert_eq!(
            summary.rejected_by_fault,
            vec![touch_replay::FaultCount {
                fault: expected,
                count: 1
            }]
        );
        assert_eq!(
            output.last().unwrap().output.state.sources[0].fault,
            Some(expected)
        );
        assert_eq!(summary.events, 0);
    }
}

#[test]
fn gpio_fault_record_cannot_erase_a_healthy_strip_or_forge_its_baseline() {
    let mut records = held(300_000);
    records.push(Record {
        now_us: 300_000,
        input: Input::GpioFault {
            fault: ControlFault::Missing,
        },
    });
    let (summary, output) = run(config(true), &records, Some(300_000));
    assert_eq!((summary.faults, summary.events), (1, 1));
    assert!(output.last().unwrap().output.state.sources[0].ready);
}

#[test]
fn parser_rejects_missing_input_unknown_fields_invalid_shapes_and_backwards_receipts() {
    for input in [
        "",
        "\n",
        "not-json",
        "{\"now_us\":0}",
        "{\"now_us\":0,\"input\":{\"kind\":\"tick\"},\"extra\":1}",
        "{\"now_us\":0,\"input\":{\"kind\":\"tick\",\"sample\":{}}}",
        "{\"now_us\":0,\"input\":{\"kind\":\"strip\",\"fault\":\"read\"}}",
        "{\"now_us\":0,\"input\":{\"kind\":\"unknown\"}}",
        "{\"now_us\":1,\"input\":{\"kind\":\"tick\"}}\n{\"now_us\":0,\"input\":{\"kind\":\"tick\"}}",
    ] {
        assert!(touch_replay::read(input.as_bytes()).is_err(), "{input}");
    }
    assert!(touch_replay::read([0xff].as_slice()).is_err());
}

#[test]
fn byte_line_record_and_duration_bounds_match_the_legacy_recorder_limits() {
    assert!(
        touch_replay::read(vec![b' '; (touch_replay::MAX_BYTES + 1) as usize].as_slice()).is_err()
    );
    let long = format!(
        "{{\"now_us\":0,\"input\":{{\"kind\":\"tick\"}}}}{}",
        " ".repeat(touch_replay::MAX_LINE_BYTES)
    );
    assert!(touch_replay::read(long.as_bytes()).is_err());
    let many = "{\"now_us\":0,\"input\":{\"kind\":\"tick\"}}\n".repeat(touch_replay::MAX_LINES + 1);
    assert!(touch_replay::read(many.as_bytes()).is_err());
    let outside = "{\"now_us\":0,\"input\":{\"kind\":\"tick\"}}\n{\"now_us\":600000001,\"input\":{\"kind\":\"tick\"}}";
    assert!(touch_replay::read(outside.as_bytes()).is_err());
}

#[test]
fn end_cannot_truncate_inputs_exceed_ten_minutes_or_overflow_tick_clock() {
    let records = held(200_000);
    for end in [199_999, touch_replay::MAX_DURATION_US + 1, u64::MAX] {
        assert!(touch_replay::run(config(true), CLOCK, &records, Some(end), io::sink()).is_err());
    }
    assert!(
        touch_replay::run(
            config(true),
            CLOCK,
            &[Record {
                now_us: u64::MAX,
                input: Input::Tick {}
            }],
            None,
            io::sink()
        )
        .is_err()
    );
    assert!(touch_replay::run(TouchConfig::default(), CLOCK, &records, None, io::sink()).is_err());
}

#[test]
fn full_ten_minute_authored_horizon_is_bounded_and_has_no_implicit_input() {
    let records = vec![
        Record {
            now_us: 0,
            input: Input::Tick {},
        },
        Record {
            now_us: touch_replay::MAX_DURATION_US,
            input: Input::Tick {},
        },
    ];
    let summary = touch_replay::run(config(false), CLOCK, &records, None, io::sink()).unwrap();
    assert_eq!(
        summary.ticks,
        touch_replay::MAX_DURATION_US / touch_replay::TICK_US + 1
    );
    assert_eq!(
        summary.frames,
        touch_replay::MAX_DURATION_US / touch_replay::FRAME_US + 1
    );
    assert_eq!(
        (summary.events, summary.strip_samples, summary.gpio_samples),
        (0, 0, 0)
    );
}

struct Broken {
    flush: bool,
}
impl Write for Broken {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.flush {
            Ok(bytes.len())
        } else {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "authored failure",
            ))
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "authored failure",
        ))
    }
}

#[test]
fn writer_and_flush_failures_are_returned_without_a_success_summary() {
    for flush in [false, true] {
        let error = touch_replay::run(
            config(false),
            CLOCK,
            &held(20_000),
            Some(20_000),
            Broken { flush },
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }
}

#[derive(Deserialize)]
struct SourceRow {
    at_us: u64,
    mask: u16,
}

/// Explicit authored adaptation: the source has changed wall-time rows only.
/// Session, monotonic clock, receipt equality and consecutive sequence below
/// are test assumptions, never claims about the original acquisition stream.
fn authored_adaptation(text: &str) -> Vec<Record> {
    text.lines()
        .enumerate()
        .map(|(index, line)| {
            let row: SourceRow = serde_json::from_str(line).unwrap();
            strip(row.at_us, index as u64 + 1, row.mask)
        })
        .collect()
}

#[test]
fn issue711_changed_only_adaptation_has_explicit_stale_faults_and_no_actions() {
    let records = authored_adaptation(include_str!("../../fixtures/strip-711-changes.jsonl"));
    let (summary, output) = run(config(true), &records, Some(19_948_000));
    assert_eq!(summary.records, 57);
    assert_eq!(summary.events, 0);
    assert_eq!(summary.rejected, 9);
    assert_eq!(
        summary.rejected_by_fault,
        vec![touch_replay::FaultCount {
            fault: ControlFault::Stale,
            count: 9
        }]
    );
    assert!(
        output
            .iter()
            .any(|f| f.output.state.sources[0].fault == Some(ControlFault::Stale))
    );
}

#[test]
fn issue711_reconstructed_continuity_has_one_contact_and_no_slide_end_tap() {
    let records = authored_adaptation(include_str!(
        "../../fixtures/strip-711-reconstruction.jsonl"
    ));
    let (summary, output) = run(config(true), &records, Some(19_948_000));
    assert_eq!(
        (
            summary.records,
            summary.strip_samples,
            summary.rejected,
            summary.faults
        ),
        (1052, 1052, 0, 0)
    );
    let events: Vec<_> = output
        .iter()
        .flat_map(|frame| &frame.output.events)
        .collect();
    eprintln!(
        "authored issue711 replay: {}",
        serde_json::json!({
            "summary": summary,
            "touch": events.iter().filter(|e| e.gesture == TouchGesture::Touch).count(),
            "sleep_request": events.iter().filter(|e| e.when_quiet == TouchIntent::Sleep).count(),
            "slide": events.iter().filter(|e| e.gesture == TouchGesture::Slide).count(),
            "stroke": events.iter().filter(|e| e.gesture == TouchGesture::Stroke).count(),
        })
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| e.gesture == TouchGesture::Touch)
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| e.when_quiet == TouchIntent::Sleep)
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| e.gesture == TouchGesture::Slide)
            .count(),
        52
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| e.gesture == TouchGesture::Stroke)
            .count(),
        0
    );
    assert_eq!(
        (summary.events, summary.ticks, summary.frames),
        (54, 9975, 1050)
    );
    assert!(events.iter().all(|e| e.contact_id == 1));
    assert!(!events.iter().any(|e| e.gesture == TouchGesture::Tap));
    let source = &output.last().unwrap().output.state.sources[0];
    assert!(source.ready);
    assert_eq!(source.active, Some(false));
    assert_eq!(source.contact_id, None);
}

#[test]
fn issue711_working_set_keeps_the_reconstructed_trace_and_full_span_coordinates() {
    let records = authored_adaptation(include_str!(
        "../../fixtures/strip-711-reconstruction.jsonl"
    ));
    let mut complete = Vec::new();
    let mut working = Vec::new();
    touch_replay::run(
        config(true),
        CLOCK,
        &records,
        Some(19_948_000),
        &mut complete,
    )
    .unwrap();
    let mut cfg = config(true);
    cfg.strip_pads = Some(vec![0, 1, 2, 3, 5, 6, 7, 9, 10, 11]);
    let summary = touch_replay::run(cfg, CLOCK, &records, Some(19_948_000), &mut working).unwrap();
    assert_eq!(
        (summary.records, summary.rejected, summary.events),
        (1052, 0, 54)
    );
    // The original rows contain neither faulty electrode. Selection changes
    // topology, not physical coordinates, contact identity or output timing.
    assert_eq!(working, complete);
}

#[test]
fn cli_ready_replay_is_the_exact_labelled_reconstruction_with_authored_clock_and_sequence() {
    let records =
        touch_replay::read(include_bytes!("../../fixtures/strip-711-replay.jsonl").as_slice())
            .unwrap();
    let reconstruction = authored_adaptation(include_str!(
        "../../fixtures/strip-711-reconstruction.jsonl"
    ));
    assert_eq!(records.len(), 1052);
    for (index, (record, reference)) in records.iter().zip(&reconstruction).enumerate() {
        assert_eq!(record.now_us, reference.now_us);
        let Input::Strip { sample } = &record.input else {
            panic!("strip-only fixture")
        };
        let Input::Strip { sample: expected } = &reference.input else {
            unreachable!()
        };
        assert_eq!(sample.mask, expected.mask);
        assert_eq!(sample.stamp.at_us, record.now_us);
        assert_eq!(sample.stamp.sequence, index as u64 + 1);
        assert_eq!(sample.stamp.clock_id, "strip-711-authored");
        assert_eq!(sample.session_id, "strip-711-authored");
    }
    let summary = touch_replay::run(
        config(false),
        "strip-711-authored",
        &records,
        Some(19_948_000),
        io::sink(),
    )
    .unwrap();
    assert_eq!(summary.rejected, 0);
}

/// #711: a push recorded from the body, before a head touch, makes the
/// replay ask nothing of that touch, as the live runtime would.
#[test]
fn a_recorded_push_silences_the_head_in_replay() {
    let mut cfg = config(true);
    cfg.head_pads = vec!["head-1".into()];
    cfg.strip_pads = None;
    let touch: Vec<Record> = (0..=1_200_000)
        .step_by(20_000)
        .enumerate()
        .map(|(index, at)| gpio(at, index as u64 + 1, at >= 600_000))
        .collect();
    let events = |records: &[Record]| -> usize {
        let (_, output) = run(cfg.clone(), records, Some(1_200_000));
        output.iter().map(|frame| frame.output.events.len()).sum()
    };
    assert_eq!(events(&touch), 1, "the touch, which stops a talking lamp");
    let mut pushed = touch.clone();
    let at = pushed
        .iter()
        .position(|record| record.now_us > 550_000)
        .unwrap();
    pushed.insert(
        at,
        Record {
            now_us: 550_000,
            input: Input::HeadPushed { pushed: true },
        },
    );
    assert_eq!(events(&pushed), 0);
}
