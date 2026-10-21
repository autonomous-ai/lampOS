use base64::{Engine as _, engine::general_purpose::STANDARD};
use lamp_expression::{
    Catalogue, Engine, LEASE_US, TICK_US,
    replay::{self, ClockAnchor, Summary, WarningKind},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Write},
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

const CLOCK: &str = "written-scenario";

fn stamp(at_us: u64, sequence: u64) -> Value {
    json!({"clock_id":CLOCK,"at_us":at_us,"sequence":sequence})
}

fn voice(at_us: u64, sequence: u64, state: &str, event: &str) -> Value {
    json!({
        "kind":"voice","stamp":stamp(at_us,sequence),"session_id":"voice-one",
        "turn":1,"request":1,"state":state,"event":event,"accepting_input":true
    })
}

fn record(at_us: u64, input: Value) -> Value {
    json!({"at_us":at_us,"input":input})
}

fn scenario(records: &[Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for record in records {
        serde_json::to_writer(&mut bytes, record).unwrap();
        bytes.push(b'\n');
    }
    bytes
}

fn engine() -> Engine {
    let mut engine = Engine::new(Catalogue::builtin(), true, CLOCK).unwrap();
    // An already-known lasting user setting is a precondition for the planner
    // to author a light face; missing settings must never be guessed.
    engine
        .ingest(
            0,
            serde_json::from_value(json!({
                "kind":"light","stamp":stamp(0,0),
                "value":{"on":true,"colour":"warm","level":3}
            }))
            .unwrap(),
        )
        .unwrap();
    engine
}

fn replay(bytes: &[u8], until: Option<u64>) -> (Summary, Vec<Value>, Vec<u8>) {
    let mut written = Vec::new();
    let summary = replay::run(bytes, &mut written, &mut engine(), until).unwrap();
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

#[test]
fn all_equal_time_arrivals_precede_the_tick_and_keep_file_order() {
    let input = scenario(&[
        record(0, voice(0, 1, "listening", "state")),
        record(0, voice(0, 2, "thinking", "state")),
    ]);
    let (summary, ticks, _) = replay(&input, Some(0));
    assert_eq!(summary.accepted, 2);
    assert_eq!(summary.rejected, 0);
    assert_eq!(summary.ticks, 1);
    assert_eq!(tick(&ticks, 0)["light"]["face"], "thinking");
}

#[test]
fn events_between_ticks_arrive_at_their_own_time_before_the_next_tick() {
    let input = scenario(&[
        record(0, voice(0, 1, "listening", "state")),
        record(21_001, voice(21_001, 2, "thinking", "state")),
    ]);
    let (summary, ticks, _) = replay(&input, Some(40_000));
    assert_eq!(summary.accepted, 2);
    assert_eq!(summary.ticks, 3);
    assert_eq!(tick(&ticks, 20_000)["light"]["face"], "listening");
    assert_eq!(tick(&ticks, 40_000)["light"]["face"], "thinking");
}

#[test]
fn nonzero_origin_is_preserved_without_replaying_the_time_before_the_recording() {
    let input = scenario(&[record(9_000_001, voice(9_000_001, 1, "listening", "state"))]);
    let (summary, ticks, _) = replay(&input, None);
    assert_eq!(summary.first_tick_us, Some(9_000_000));
    assert_eq!(summary.last_tick_us, Some(9_220_000));
    assert_eq!(summary.ticks, 12);
    assert_eq!(tick(&ticks, 9_020_000)["light"]["face"], "listening");
}

#[test]
fn old_payloads_arriving_late_are_reported_and_never_sorted_into_the_past() {
    let input = scenario(&[
        record(0, voice(0, 2, "listening", "state")),
        record(20_000, voice(0, 1, "thinking", "state")),
    ]);
    let (summary, ticks, _) = replay(&input, Some(20_000));
    assert_eq!(summary.accepted, 1);
    assert_eq!(summary.rejected, 1);
    assert_eq!(summary.warnings[0].kind, WarningKind::RejectedInput);
    assert_eq!(summary.warnings[0].line, Some(2));
    assert_eq!(summary.warnings[0].at_us, Some(20_000));
    assert_eq!(tick(&ticks, 20_000)["light"]["face"], "listening");
}

#[test]
fn a_terminal_event_precedes_later_audio_even_when_playout_was_already_queued() {
    let mut scheduled = voice(20_000, 2, "thinking", "scheduled");
    scheduled["first_audio_us"] = 100_000.into();
    let playout = |at_us, sequence| {
        json!({
            "kind":"playout","stamp":stamp(at_us,sequence),"session_id":"voice-one",
            "turn":1,"request":1,"start_us":100_000,"end_us":200_000,"level":1.0
        })
    };
    let input = scenario(&[
        record(0, voice(0, 1, "thinking", "state")),
        record(20_000, scheduled),
        record(40_000, playout(40_000, 1)),
        record(60_000, voice(60_000, 3, "listening", "interrupted")),
        record(80_000, playout(40_000, 2)),
        record(80_000, voice(70_000, 4, "speaking", "state")),
    ]);
    let (summary, ticks, _) = replay(&input, Some(160_000));
    assert!(summary.rejected >= 1);
    for at_us in (60_000..=160_000).step_by(TICK_US as usize) {
        let output = tick(&ticks, at_us);
        assert_eq!(output["light"]["face"], "listening", "at {at_us}");
        assert!(
            !output["state"]["behaviours"]
                .as_array()
                .unwrap()
                .contains(&json!("follow played sound"))
        );
    }
}

#[test]
fn input_validation_is_atomic_before_any_tick_or_engine_mutation() {
    let mut unknown_clock = record(20_000, voice(20_000, 2, "thinking", "state"));
    unknown_clock["input"]["stamp"]["clock_id"] = "another-boot".into();
    let invalid = [
        scenario(&[
            record(20_000, voice(20_000, 1, "listening", "state")),
            record(0, voice(0, 2, "thinking", "state")),
        ]),
        scenario(&[record(0, voice(0, 1, "listening", "state")), unknown_clock]),
        scenario(&[record(0, json!({"kind":"telepathy"}))]),
        scenario(&[record(0, voice(0, 1, "dance", "state"))]),
        b"{\"at_us\":0}\n".to_vec(),
        b"not json\n".to_vec(),
        b"{\"at_us\":0,\"input\":null}\n".to_vec(),
    ];
    for bytes in invalid {
        let mut engine = engine();
        let before = serde_json::to_value(engine.tick(0)).unwrap();
        let mut written = Vec::new();
        assert!(replay::run(bytes.as_slice(), &mut written, &mut engine, None).is_err());
        assert!(written.is_empty());
        let after = serde_json::to_value(engine.tick(0)).unwrap();
        assert_eq!(after["body"], before["body"]);
        assert_eq!(after["light"], before["light"]);
    }
}

#[test]
fn an_invalid_semantic_payload_is_fatal_instead_of_an_ignored_event() {
    let mut malformed = voice(0, 1, "thinking", "scheduled");
    malformed["first_audio_us"] = 9_000_000.into();
    let mut written = Vec::new();
    assert!(
        replay::run(
            scenario(&[record(0, malformed)]).as_slice(),
            &mut written,
            &mut engine(),
            None
        )
        .is_err()
    );
    assert!(written.is_empty());
}

#[test]
fn huge_lines_blank_line_floods_and_record_floods_are_bounded() {
    let lines = vec![b' '; replay::MAX_LINE_BYTES + 1];
    let blanks = vec![b'\n'; replay::MAX_LINES + 1];
    let mut full_blank = vec![b' '; replay::MAX_LINE_BYTES - 1];
    full_blank.push(b'\n');
    let too_many_bytes = full_blank.repeat(replay::MAX_BYTES / replay::MAX_LINE_BYTES + 1);
    let one = scenario(&[record(0, voice(0, 1, "listening", "state"))]);
    let flood = one.repeat(replay::MAX_RECORDS + 1);
    for (bytes, expected) in [
        (lines, "line exceeds"),
        (blanks, "recording exceeds"),
        (too_many_bytes, "recording exceeds"),
        (flood, "input records"),
    ] {
        let mut written = Vec::new();
        let error = replay::run(bytes.as_slice(), &mut written, &mut engine(), None).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
        assert!(written.is_empty());
    }
}

#[test]
fn gaps_and_explicit_ends_cannot_request_unbounded_ticks() {
    for (records, until, expected) in [
        (
            vec![
                record(0, voice(0, 1, "listening", "state")),
                record(
                    replay::MAX_DURATION_US + TICK_US,
                    voice(replay::MAX_DURATION_US + TICK_US, 2, "thinking", "state"),
                ),
            ],
            None,
            "horizon",
        ),
        (vec![], Some(replay::MAX_DURATION_US + TICK_US), "horizon"),
        (
            vec![
                record(0, voice(0, 1, "listening", "state")),
                record(21_000, voice(21_000, 2, "thinking", "state")),
            ],
            Some(39_999),
            "until_us",
        ),
        (
            vec![record(u64::MAX, voice(u64::MAX, 1, "listening", "state"))],
            None,
            "overflows",
        ),
    ] {
        let mut written = Vec::new();
        let error = replay::run(
            scenario(&records).as_slice(),
            &mut written,
            &mut engine(),
            until,
        )
        .unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
        assert!(written.is_empty());
    }
}

#[test]
fn the_default_tail_is_one_lease_and_empty_input_has_explicit_semantics() {
    let (summary, _, _) = replay(
        &scenario(&[record(0, voice(0, 1, "listening", "state"))]),
        None,
    );
    assert_eq!(summary.last_tick_us, Some(LEASE_US));
    let (empty, ticks, bytes) = replay(b"\n \n", None);
    assert_eq!(empty.input_records, 0);
    assert_eq!(empty.ticks, 0);
    assert!(ticks.is_empty() && bytes.is_empty());
    let (timed, _, _) = replay(b"", Some(40_000));
    assert_eq!(timed.ticks, 3);
}

struct RefusesWrites;

impl Write for RefusesWrites {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("the output disappeared"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_failed_output_write_is_an_error_and_never_a_successful_summary() {
    let error = replay::run(b"".as_slice(), RefusesWrites, &mut engine(), Some(0)).unwrap_err();
    assert!(error.to_string().contains("the output disappeared"));
}

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Files(PathBuf);

impl Files {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "expression-replay-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, rows: &[Value]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, scenario(rows)).unwrap();
        path
    }
}

impl Drop for Files {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn bus(
    topic: &str,
    type_name: &str,
    payload: Value,
    sequence: u64,
    wall_ms: u64,
    private_us: u64,
) -> Value {
    json!({
        "header": {"topic":topic,"kind":"latest","enc":"json","type":type_name,"source":"voice","boot":"0123456789abcdef","seq":sequence,"at_us":private_us,"at_unix_ms":wall_ms},
        "payload_b64":STANDARD.encode(serde_json::to_vec(&payload).unwrap())
    })
}

fn anchor_record() -> Value {
    let mut payload = voice(1_000_000, 1, "listening", "state");
    payload.as_object_mut().unwrap().remove("kind");
    bus(
        "voice/timing",
        "lamp_msgs/VoiceTiming",
        payload,
        1,
        10_000,
        4,
    )
}

#[test]
fn actual_bus_declarations_import_on_the_shared_anchor_not_private_header_clocks() {
    let files = Files::new();
    let legacy = files.write("legacy.jsonl", &[
        json!({"topic":"voice/state","declaration":{"type":"lamp_msgs/VoiceState","version":1,"kind":"latest","period_ms":1000}}),
        bus("voice/state", "lamp_msgs/VoiceState", json!({"state":"thinking"}), 98, 10_001, 9_999_999_999),
    ]);
    let typed = files.write("timing.jsonl", &[anchor_record()]);
    let imported = replay::import_bus(&[legacy, typed], CLOCK).unwrap();
    assert_eq!(imported.anchor.at_us, 1_000_000);
    assert_eq!(imported.records.len(), 2);
    assert_eq!(imported.records[0].at_us, 1_000_000);
    assert_eq!(imported.records[1].at_us, 1_001_000);
    assert_eq!(imported.records[1].input.stamp().sequence, 2);
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.kind == WarningKind::EstimatedArrival
                && warning.detail.contains("not recorder receipt"))
    );
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.kind == WarningKind::LegacyWallClock
                && warning.detail.contains("no inferred error bound"))
    );
}

#[test]
fn old_bus_recordings_require_an_explicit_clock_anchor() {
    let files = Files::new();
    let path = files.write(
        "old.jsonl",
        &[bus(
            "voice/state",
            "lamp_msgs/VoiceState",
            json!({"state":"thinking"}),
            1,
            10_000,
            999,
        )],
    );
    let error = replay::import_bus(std::slice::from_ref(&path), CLOCK).unwrap_err();
    assert!(error.to_string().contains("no typed timing record"));
    let imported = replay::import_bus_with_anchor(
        &[path],
        &ClockAnchor {
            clock_id: CLOCK.into(),
            at_us: 1_000_000,
            at_unix_ms: 10_000,
        },
    )
    .unwrap();
    assert_eq!(imported.records[0].at_us, 1_000_000);
}

#[test]
fn duplicate_old_boots_and_dropped_bus_sequences_are_reported_not_resurrected() {
    let files = Files::new();
    let legacy = |sequence, wall| {
        bus(
            "voice/state",
            "lamp_msgs/VoiceState",
            json!({"state":"listening"}),
            sequence,
            wall,
            sequence,
        )
    };
    let mut restarted = legacy(1, 10_001);
    restarted["header"]["boot"] = "fedcba9876543210".into();
    let mut next = legacy(3, 10_004);
    next["header"]["boot"] = "fedcba9876543210".into();
    let path = files.write(
        "restarts.jsonl",
        &[
            anchor_record(),
            legacy(5, 10_000),
            restarted.clone(),
            legacy(6, 10_002),
            restarted,
            next,
        ],
    );
    let imported = replay::import_bus(&[path], CLOCK).unwrap();
    assert_eq!(imported.records.len(), 4);
    assert_eq!(
        imported
            .warnings
            .iter()
            .filter(|warning| warning.kind == WarningKind::DroppedSequence)
            .count(),
        2
    );
    assert_eq!(
        imported
            .warnings
            .iter()
            .filter(|warning| warning.kind == WarningKind::PublisherRestart)
            .count(),
        1
    );
    assert_eq!(
        imported
            .warnings
            .iter()
            .filter(|warning| warning.kind == WarningKind::MissingSequence)
            .count(),
        1
    );
    let legacy_sequences: Vec<u64> = imported
        .records
        .iter()
        .filter(|record| record.input.topic() == "voice/state")
        .map(|record| record.input.stamp().sequence)
        .collect();
    assert_eq!(legacy_sequences, vec![2, 3, 4]);
}

#[test]
fn bus_import_rejects_clock_conflicts_bad_declarations_encoding_and_wall_reversal() {
    let files = Files::new();
    let mut unknown_clock = voice(1_020_000, 2, "thinking", "state");
    unknown_clock.as_object_mut().unwrap().remove("kind");
    unknown_clock["stamp"]["clock_id"] = "foreign-boot".into();
    let mut bad_base64 = anchor_record();
    bad_base64["payload_b64"] = "not+base64!?".into();
    let mut wrong_type = anchor_record();
    wrong_type["header"]["type"] = "lamp_msgs/VoiceState".into();
    let mut wrong_kind = anchor_record();
    wrong_kind["header"]["kind"] = "event".into();
    for (index, rows) in [
        vec![anchor_record(),bus("voice/timing","lamp_msgs/VoiceTiming",unknown_clock,2,10_020,0)],
        vec![json!({"topic":"voice/state","declaration":{"type":"lamp_msgs/JointState","version":1,"kind":"latest"}})],
        vec![bad_base64],
        vec![wrong_type],
        vec![wrong_kind],
        vec![json!({"header":anchor_record()["header"]})],
        vec![anchor_record(),bus("voice/state","lamp_msgs/VoiceState",json!({"state":"thinking"}),1,10_000,0),bus("voice/state","lamp_msgs/VoiceState",json!({"state":"listening"}),2,9_999,1)],
    ].into_iter().enumerate() {
        let path = files.write(&format!("invalid-{index}.jsonl"), &rows);
        assert!(replay::import_bus(&[path],CLOCK).is_err(), "case {index}");
    }
}

#[test]
fn unsupported_bus_topics_are_reported_and_ties_keep_input_file_order() {
    let files = Files::new();
    let first = files.write(
        "a.jsonl",
        &[
            anchor_record(),
            bus(
                "voice/state",
                "lamp_msgs/VoiceState",
                json!({"state":"listening"}),
                1,
                10_001,
                9,
            ),
            bus(
                "battery/state",
                "lamp_msgs/Battery",
                json!({"charge":1}),
                1,
                10_001,
                999,
            ),
        ],
    );
    let second = files.write(
        "b.jsonl",
        &[bus(
            "room/state",
            "lamp_msgs/RoomState",
            json!({"state":"quiet"}),
            1,
            10_001,
            9999,
        )],
    );
    let imported = replay::import_bus(&[first, second], CLOCK).unwrap();
    assert_eq!(imported.records.len(), 3);
    assert_eq!(imported.records[1].input.topic(), "voice/state");
    assert_eq!(imported.records[2].input.topic(), "room/state");
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.kind == WarningKind::IgnoredTopic)
    );
}

#[test]
fn earlier_legacy_wall_times_map_backwards_from_the_anchor_without_underflow() {
    let files = Files::new();
    let path = files.write(
        "earlier.jsonl",
        &[
            bus(
                "voice/state",
                "lamp_msgs/VoiceState",
                json!({"state":"thinking"}),
                1,
                9_999,
                u64::MAX,
            ),
            anchor_record(),
        ],
    );
    let imported = replay::import_bus(std::slice::from_ref(&path), CLOCK).unwrap();
    assert_eq!(imported.records[0].at_us, 999_000);
    assert_eq!(imported.records[1].at_us, 1_000_000);
    let impossible = ClockAnchor {
        clock_id: CLOCK.into(),
        at_us: 0,
        at_unix_ms: 10_000,
    };
    let error = replay::import_bus_with_anchor(&[path], &impossible).unwrap_err();
    assert!(error.to_string().contains("outside the shared monotonic"));
}

#[test]
fn bus_file_count_and_imported_time_horizon_are_bounded_before_replay() {
    assert!(replay::import_bus(&[], CLOCK).is_err());
    let paths = vec![PathBuf::from("must-not-be-opened"); replay::MAX_FILES + 1];
    assert!(
        replay::import_bus(&paths, CLOCK)
            .unwrap_err()
            .to_string()
            .contains("bus recordings")
    );
    let files = Files::new();
    let path = files.write(
        "long.jsonl",
        &[
            anchor_record(),
            bus(
                "voice/state",
                "lamp_msgs/VoiceState",
                json!({"state":"thinking"}),
                1,
                10_000 + replay::MAX_DURATION_US / 1_000 + 1,
                0,
            ),
        ],
    );
    assert!(
        replay::import_bus(&[path], CLOCK)
            .unwrap_err()
            .to_string()
            .contains("horizon")
    );
}

#[test]
fn cli_import_and_replay_keep_stdout_as_jsonl_and_default_to_shadow() {
    let files = Files::new();
    let path = files.write(
        "bus.jsonl",
        &[
            anchor_record(),
            bus(
                "light/command",
                "lamp_msgs/Light",
                json!({"on":true,"colour":"warm","level":3}),
                1,
                10_001,
                99,
            ),
        ],
    );
    let imported = Command::new(env!("CARGO_BIN_EXE_lamp-expression"))
        .args(["import", "--clock-id", CLOCK])
        .arg(path)
        .output()
        .unwrap();
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    assert!(String::from_utf8_lossy(&imported.stderr).contains("not recorder receipt"));
    let input = files.0.join("imported.jsonl");
    fs::write(&input, imported.stdout).unwrap();
    for enabled in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_lamp-expression"));
        command.args([
            "replay",
            "--clock-id",
            CLOCK,
            "--until-us",
            "1020000",
            "--input",
        ]);
        command.arg(&input);
        if enabled {
            command.arg("--enable");
        }
        let replayed = command.output().unwrap();
        assert!(
            replayed.status.success(),
            "{}",
            String::from_utf8_lossy(&replayed.stderr)
        );
        let ticks: Vec<Value> = replayed
            .stdout
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        assert_eq!(ticks.len(), 2);
        let output = tick(&ticks, 1_020_000);
        assert_eq!(output["state"]["enabled"], enabled);
        assert_eq!(output["light"].is_object(), enabled);
        assert!(output["body"].is_null(), "no measured joints were supplied");
    }
}

#[test]
fn written_five_behaviour_scenario_is_repeatable_and_declares_requests_only() {
    let input = include_bytes!("../../fixtures/five-behaviours.jsonl");
    let (summary, ticks, bytes) = replay(input, Some(2_000_000));
    let (again, _, repeated) = replay(input, Some(2_000_000));
    assert_eq!(bytes, repeated);
    assert_eq!(summary.accepted, again.accepted);
    assert_eq!(summary.rejected, 0, "{:?}", summary.warnings);
    assert_eq!(summary.ticks, 101);
    let behaviours: BTreeSet<&str> = ticks
        .iter()
        .flat_map(|tick| tick["output"]["state"]["behaviours"].as_array().unwrap())
        .filter_map(Value::as_str)
        .collect();
    for name in [
        "look away and back",
        "nod in a pause",
        "follow played sound",
        "yield when interrupted",
        "stay still for work",
    ] {
        assert!(
            behaviours.contains(name),
            "missing {name}; found {behaviours:?}"
        );
    }
    assert!(
        tick(&ticks, 760_000)["body"]["offsets"]["base_yaw"]
            .as_f64()
            .unwrap()
            > 0.05
    );
    assert_eq!(
        tick(&ticks, 1_160_000)["body"]["offsets"]["base_yaw"],
        0.0,
        "the authored return is complete 40 ms before the scheduled first sample"
    );
    assert_eq!(tick(&ticks, 1_500_000)["light"]["face"], "listening");
    assert_eq!(tick(&ticks, 1_700_000)["body"]["hold"], true);
    for item in ticks {
        assert!(item.get("physical_completion_us").is_none());
        assert!(
            item["output"]["state"]["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .all(|warning| warning.is_string())
        );
    }
}

#[test]
fn the_same_written_scenario_in_default_shadow_never_commands_an_owner() {
    let mut shadow = Engine::new(Catalogue::builtin(), false, CLOCK).unwrap();
    let mut output = Vec::new();
    let summary = replay::run(
        include_bytes!("../../fixtures/five-behaviours.jsonl").as_slice(),
        &mut output,
        &mut shadow,
        None,
    )
    .unwrap();
    assert_eq!(summary.rejected, 0);
    for line in output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let value: Value = serde_json::from_slice(line).unwrap();
        assert!(value["output"]["body"].is_null());
        assert!(value["output"]["light"].is_null());
        assert_eq!(value["output"]["state"]["enabled"], false);
    }
}
