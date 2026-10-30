//! Authored arrival-clock scenarios and real local bus adaptation. These
//! checks establish planned requests, never speaker output or motor movement.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use lamp_bus::{Bus, Header, Kind, Message};
use lamp_expression::{
    Catalogue, Engine, Input, TICK_US, Trigger,
    replay::{self, Summary, WarningKind},
    runtime,
};
use lamp_msgs::{
    ExpressionState, JointState, Level, Light, RoomState, VoicePlayout, VoiceProgress, VoiceSound,
    VoiceTiming,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

const CLOCK: &str = "expression-rest-fixture";
const RESUMED: &str = include_str!("../../fixtures/held-resumed.jsonl");
const CANCELLED: &str = include_str!("../../fixtures/held-cancelled.jsonl");
const NOD: &str = include_str!("../../fixtures/finish-nod.jsonl");
const DEFAULT_CONCENTRATION: &str = include_str!("../../fixtures/no-concentration.jsonl");
const QUIET: &str = include_str!("../../fixtures/rest-quiet.jsonl");
const BUSY: &str = include_str!("../../fixtures/rest-busy.jsonl");
const CONCENTRATING: &str = include_str!("../../fixtures/rest-concentrating.jsonl");
const EXPIRED: &str = include_str!("../../fixtures/rest-expired.jsonl");
const NOD_NAME: &str = "nod as speech ends";
const REST_NAME: &str = "settle at rest";

fn catalogue() -> Catalogue {
    let mut catalogue = Catalogue::from_json(include_str!("../../rest-behaviours.json")).unwrap();
    // Authored zero lag isolates requested timing. The shipped catalogue
    // retains unknown physical lag; no test result fills in that measurement.
    catalogue.timing.body_lag_us = Some(0);
    catalogue
}

fn rows(text: &str) -> Vec<Value> {
    text.lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn encode(rows: &[Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for row in rows {
        serde_json::to_writer(&mut bytes, row).unwrap();
        bytes.push(b'\n');
    }
    bytes
}

fn run(
    bytes: &[u8],
    catalogue: Catalogue,
    enabled: bool,
    until: Option<u64>,
) -> (Summary, Vec<Value>, Vec<u8>) {
    let mut engine = Engine::new(catalogue, enabled, CLOCK).unwrap();
    let mut written = Vec::new();
    let summary = replay::run(bytes, &mut written, &mut engine, until).unwrap();
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

fn has(output: &Value, name: &str) -> bool {
    output["state"]["behaviours"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == name)
}

fn stamp(at_us: u64) -> Value {
    json!({"clock_id":CLOCK,"at_us":at_us,"sequence":at_us+1})
}

fn progress(at_us: u64, state: &str, generation: u64, held: bool) -> Value {
    json!({"at_us":at_us,"input":{"kind":"progress","stamp":stamp(at_us),
        "session_id":"fixture-voice","turn":1,"request":1,"state":state,"event":"state",
        "accepting_input":true,"first_audio_us":null,"audio_generation":generation,"held":held}})
}

#[test]
fn every_authored_fixture_has_exact_counts_and_only_its_declared_refusals() {
    for (name, text, records, refused) in [
        ("held-resumed", RESUMED, 18, 4),
        ("held-cancelled", CANCELLED, 16, 3),
        ("finish-nod", NOD, 37, 0),
        ("no-concentration", DEFAULT_CONCENTRATION, 7, 0),
        ("rest-quiet", QUIET, 2244, 0),
        ("rest-busy", BUSY, 804, 0),
        ("rest-concentrating", CONCENTRATING, 805, 0),
        ("rest-expired", EXPIRED, 766, 0),
    ] {
        let (summary, ticks, _) = run(text.as_bytes(), catalogue(), true, None);
        assert_eq!(summary.input_records, records, "{name}");
        assert_eq!(
            summary.accepted,
            records - refused,
            "{name}: {:?}",
            summary.warnings
        );
        assert_eq!(summary.rejected, refused, "{name}: {:?}", summary.warnings);
        assert_eq!(summary.warnings.len(), refused, "{name}");
        assert_eq!(summary.ticks, ticks.len());
        assert!(
            summary
                .warnings
                .iter()
                .all(|warning| warning.kind == WarningKind::RejectedInput)
        );
        let last = rows(text).last().unwrap()["at_us"].as_u64().unwrap();
        assert_eq!(summary.first_tick_us, Some(0));
        assert_eq!(summary.last_tick_us, Some(last + 200_000));
        assert_eq!(summary.ticks as u64, (last + 200_000) / TICK_US + 1);
    }
}

#[test]
fn held_face_is_next_tick_and_resume_accepts_only_new_generation_sound() {
    let (summary, ticks, output) = run(RESUMED.as_bytes(), catalogue(), true, Some(300_000));
    let expected = [
        (
            10,
            "sound is held or belongs to an inactive audio generation",
        ),
        (
            11,
            "sound is held or belongs to an inactive audio generation",
        ),
        (
            13,
            "sound is held or belongs to an inactive audio generation",
        ),
        (14, "sound predates the accepted generation or resume edge"),
    ];
    assert_eq!(summary.warnings.len(), expected.len());
    for (warning, (line, reason)) in summary.warnings.iter().zip(expected) {
        assert_eq!(warning.line, Some(line));
        assert_eq!(warning.detail, reason);
    }
    assert_eq!(tick(&ticks, 40_000)["light"]["face"], "speaking");
    // Hold arrives at 41,001 us; its next planner tick is 18,999 us later.
    for at in (60_000..=120_000).step_by(TICK_US as usize) {
        let output = tick(&ticks, at);
        assert_eq!(output["light"]["face"], "listening", "at {at}");
        assert!(!has(output, "follow played sound"));
        assert!(
            !output["body"]["gaze"].is_null(),
            "fresh face is usable during hold"
        );
    }
    let resumed = tick(&ticks, 160_000);
    assert_eq!(resumed["light"]["face"], "speaking");
    assert!(has(resumed, "follow played sound"));
    // The newly admitted quarter-level frame modulates brightness. Old
    // full-level frames were both queued and delivered late in this fixture.
    assert!((resumed["light"]["brightness"].as_f64().unwrap() - 0.4).abs() < 0.000_001);
    assert!(!has(tick(&ticks, 260_000), "follow played sound"));
    assert_eq!(
        output,
        run(RESUMED.as_bytes(), catalogue(), true, Some(300_000)).2
    );
}

#[test]
fn a_held_turn_without_input_admission_never_claims_listening_readiness() {
    let mut scenario = rows(RESUMED);
    for row in &mut scenario {
        if row["input"]["held"] == true {
            row["input"]["accepting_input"] = false.into();
        }
    }
    let (_, ticks, _) = run(&encode(&scenario), catalogue(), true, Some(300_000));
    for at in (60_000..=120_000).step_by(TICK_US as usize) {
        assert_eq!(tick(&ticks, at)["light"]["face"], "unavailable");
    }
}

#[test]
fn interrupted_and_cancelled_held_turns_cannot_be_resumed_even_with_a_new_generation() {
    for failed in [false, true] {
        let mut scenario = rows(CANCELLED);
        if failed {
            scenario
                .iter_mut()
                .find(|row| row["input"]["event"] == "interrupted")
                .unwrap()["input"]["event"] = "cancelled".into();
        }
        let (summary, ticks, _) = run(&encode(&scenario), catalogue(), true, Some(300_000));
        assert_eq!(summary.rejected, 3, "{:?}", summary.warnings);
        assert_eq!(
            summary.warnings[0].detail,
            "a retired voice turn cannot resume audio or thinking"
        );
        for warning in &summary.warnings[1..] {
            assert_eq!(
                warning.detail,
                "sound is held or belongs to an inactive audio generation"
            );
        }
        for at in [100_000, 120_000] {
            let output = tick(&ticks, at);
            assert!(!has(output, "follow played sound"));
            if failed {
                assert!(output["light"].is_null() && output["body"].is_null());
            } else {
                assert_eq!(output["light"]["face"], "listening");
            }
        }
        assert_eq!(tick(&ticks, 140_000)["light"]["face"], "listening");
        assert_eq!(tick(&ticks, 240_000)["light"]["face"], "resting");
    }
}

#[test]
fn a_missed_hold_snapshot_still_discards_pre_generation_audio_on_resume() {
    let scenario: Vec<_> = rows(RESUMED)
        .into_iter()
        .filter(|row| row["input"]["held"] != true)
        .collect();
    let (summary, ticks, _) = run(&encode(&scenario), catalogue(), true, Some(300_000));
    // No hold was received, so a same-generation old frame may be admitted
    // before resume. The higher resume generation must clear it at 120001 us.
    assert_eq!(summary.rejected, 4, "{:?}", summary.warnings);
    let expected = [
        "playout is expired, too far ahead, overlapping or out of order",
        "sound is held or belongs to an inactive audio generation",
        "sound is held or belongs to an inactive audio generation",
        "sound predates the accepted generation or resume edge",
    ];
    for (warning, reason) in summary.warnings.iter().zip(expected) {
        assert_eq!(warning.detail, reason);
    }
    let output = tick(&ticks, 160_000);
    assert_eq!(output["light"]["face"], "speaking");
    assert!((output["light"]["brightness"].as_f64().unwrap() - 0.4).abs() < 0.000_001);
}

#[test]
fn a_started_finish_nod_continues_into_thinking_and_ends_once_without_more_activity() {
    let (summary, ticks, _) = run(NOD.as_bytes(), catalogue(), true, Some(1_800_000));
    assert_eq!(summary.rejected, 0);
    assert!(!has(tick(&ticks, 420_000), NOD_NAME));
    assert!(has(tick(&ticks, 440_000), NOD_NAME));
    for at in [600_000, 800_000, 1_000_000, 1_600_000] {
        let output = tick(&ticks, at);
        assert!(has(output, NOD_NAME), "at {at}");
        assert_eq!(output["light"]["face"], "thinking");
    }
    assert!(!has(tick(&ticks, 1_660_000), NOD_NAME));
    assert!(!has(tick(&ticks, 1_800_000), NOD_NAME));
    let mut no_carry = catalogue();
    let clip = no_carry
        .behaviours
        .iter_mut()
        .find(|clip| clip.trigger == Trigger::Pause)
        .unwrap();
    clip.continue_in.clear();
    // Without continuation the catalogue permits at most 600 ms. This
    // shorter clip would still extend past the 600 ms phase change.
    clip.stroke_us /= 2;
    for frame in &mut clip.keyframes {
        frame.at_us /= 2;
    }
    let (_, ticks, _) = run(NOD.as_bytes(), no_carry, true, Some(1_800_000));
    assert!(has(tick(&ticks, 440_000), NOD_NAME));
    assert!(!has(tick(&ticks, 600_000), NOD_NAME));
}

#[test]
fn renewed_speech_cancels_the_carried_nod_and_thinking_alone_cannot_start_one() {
    let mut scenario = rows(NOD);
    scenario.push(
        json!({"at_us":800_001,"input":{"kind":"activity","stamp":stamp(800_001),
        "session_id":"fixture-voice","turn":1,"request":1,"speaking":true}}),
    );
    scenario.sort_by_key(|row| row["at_us"].as_u64().unwrap());
    let (summary, ticks, _) = run(&encode(&scenario), catalogue(), true, Some(1_800_000));
    assert_eq!(summary.rejected, 0, "{:?}", summary.warnings);
    assert!(has(tick(&ticks, 800_000), NOD_NAME));
    assert!(!has(tick(&ticks, 820_000), NOD_NAME));
    assert!(!has(tick(&ticks, 1_400_000), NOD_NAME));
    let scenario: Vec<_> = rows(NOD)
        .into_iter()
        .filter(|row| row["input"]["kind"] != "activity")
        .collect();
    let (_, ticks, _) = run(&encode(&scenario), catalogue(), true, Some(1_800_000));
    assert!(ticks.iter().all(|tick| !has(&tick["output"], NOD_NAME)));
}

#[test]
fn no_concentration_publication_means_not_concentrating_and_healthy_with_other_inputs_known() {
    let (summary, ticks, _) = run(
        DEFAULT_CONCENTRATION.as_bytes(),
        catalogue(),
        true,
        Some(200_000),
    );
    assert_eq!(summary.rejected, 0);
    for tick in ticks {
        let state: ExpressionState =
            serde_json::from_value(tick["output"]["state"].clone()).unwrap();
        assert!(state.missing.is_empty(), "{:?}", state.missing);
        assert_eq!(
            runtime::standing(&runtime::Counts::default(), None, &state).0,
            Level::Ok
        );
        assert_eq!(tick["output"]["body"]["hold"], false);
    }
}

#[test]
fn quiet_rest_is_finite_small_pitch_only_and_waits_ninety_seconds_after_completion() {
    let (_, ticks, _) = run(QUIET.as_bytes(), catalogue(), true, None);
    let mut starts = Vec::new();
    let mut active = false;
    for tick in &ticks {
        let at = tick["at_us"].as_u64().unwrap();
        let output = &tick["output"];
        let next = has(output, REST_NAME);
        if next && !active {
            starts.push(at);
        }
        active = next;
        assert_eq!(output["light"]["face"], "resting");
        assert!(output["body"]["gaze"].is_null());
        for axis in ["base_yaw", "shoulder", "elbow", "head_roll"] {
            assert_eq!(output["body"]["offsets"][axis], 0.0, "at {at}: {axis}");
        }
        let pitch = output["body"]["offsets"]["head_pitch"].as_f64().unwrap();
        assert!((0.0..=0.045).contains(&pitch), "at {at}: {pitch}");
        if !next {
            assert_eq!(pitch, 0.0, "at {at}");
        }
    }
    assert_eq!(starts, [45_000_000, 139_000_000]);
    assert!(has(tick(&ticks, 48_980_000), REST_NAME));
    assert!(!has(tick(&ticks, 49_000_000), REST_NAME));
    assert!(!has(tick(&ticks, 138_980_000), REST_NAME));
}

#[test]
fn busy_and_explicit_concentration_never_start_rest_even_after_the_delay() {
    for (text, held) in [(BUSY, false), (CONCENTRATING, true)] {
        let (_, ticks, _) = run(text.as_bytes(), catalogue(), true, None);
        for tick in ticks {
            let output = &tick["output"];
            assert!(!has(output, REST_NAME));
            assert_eq!(output["body"]["hold"], held);
            for axis in ["base_yaw", "shoulder", "elbow", "head_roll", "head_pitch"] {
                assert_eq!(output["body"]["offsets"][axis], 0.0);
            }
        }
    }
}

#[test]
fn quiet_expiry_stops_an_active_rest_at_the_exact_boundary() {
    let (_, ticks, _) = run(EXPIRED.as_bytes(), catalogue(), true, None);
    assert!(has(tick(&ticks, 46_980_000), REST_NAME));
    let expired = tick(&ticks, 47_000_000);
    assert!(!has(expired, REST_NAME));
    assert_eq!(expired["body"]["offsets"]["head_pitch"], 0.0);
    assert!(
        expired["state"]["missing"]
            .as_array()
            .unwrap()
            .contains(&json!("fresh room state"))
    );
}

#[test]
fn rest_requests_stop_when_joints_or_voice_expire_and_later_receipts_do_not_retroactively_extend_them()
 {
    for (kind, boundary) in [("joints", 45_900_000), ("progress", 46_100_000)] {
        let scenario: Vec<_> = rows(EXPIRED)
            .into_iter()
            .filter(|row| {
                row["input"]["kind"] != kind || row["at_us"].as_u64().unwrap() <= 45_600_000
            })
            .collect();
        let (_, ticks, _) = run(&encode(&scenario), catalogue(), true, None);
        assert!(!tick(&ticks, boundary - TICK_US)["body"].is_null());
        let expired = tick(&ticks, boundary);
        assert!(expired["body"].is_null());
        if kind == "progress" {
            assert!(expired["light"].is_null());
        }
    }
}

#[test]
fn all_opted_in_fixtures_remain_shadow_without_the_enable_switch() {
    for text in [
        RESUMED,
        CANCELLED,
        NOD,
        DEFAULT_CONCENTRATION,
        QUIET,
        BUSY,
        CONCENTRATING,
        EXPIRED,
    ] {
        let (_, ticks, _) = run(text.as_bytes(), catalogue(), false, None);
        assert!(
            ticks
                .iter()
                .all(|tick| tick["output"]["body"].is_null() && tick["output"]["light"].is_null())
        );
    }
}

fn message(input: &Value) -> Message {
    let (topic, type_name, kind) = match input["kind"].as_str().unwrap() {
        "progress" => (VoiceProgress::TOPIC, VoiceProgress::TYPE, Kind::Latest),
        "sound" => (VoiceSound::TOPIC, VoiceSound::TYPE, Kind::Event),
        _ => panic!("new typed protocol only"),
    };
    let mut payload = input.clone();
    payload.as_object_mut().unwrap().remove("kind");
    Message {
        header: Header {
            topic: topic.into(),
            kind,
            enc: "json".into(),
            type_name: Some(type_name.into()),
            source: "fixture-voice".into(),
            boot: "0123456789abcdef".into(),
            seq: input["stamp"]["sequence"].as_u64().unwrap(),
            at_us: 9_999_999_999,
            at_unix_ms: 1_000,
            expires_ms: None,
            clock: None,
        },
        payload: serde_json::to_vec(&payload).unwrap(),
        gap: 0,
        publisher_restarted: false,
        expired: false,
    }
}

#[test]
fn runtime_adapts_additive_topics_without_replacing_shared_stamps_or_generation() {
    for row in rows(RESUMED)
        .into_iter()
        .filter(|row| matches!(row["input"]["kind"].as_str(), Some("progress" | "sound")))
    {
        let value = &row["input"];
        let item = message(value);
        let input = runtime::input_from_message(&item, 200_000, 1_020, "wrong-adapter-clock", 9999)
            .unwrap();
        // Optional first_audio_us=None is omitted when reserialized; compare
        // the typed contract rather than the fixture's explicit null spelling.
        assert_eq!(
            input,
            serde_json::from_value::<Input>(value.clone()).unwrap()
        );
        let mut bad = item.clone();
        bad.header.type_name = Some("lamp_msgs/VoiceTiming".into());
        assert!(runtime::input_from_message(&bad, 200_000, 1_020, CLOCK, 9999).is_err());
        bad = item.clone();
        bad.expired = true;
        assert!(runtime::input_from_message(&bad, 200_000, 1_020, CLOCK, 9999).is_err());
        bad = item;
        bad.header.at_unix_ms = 1_071;
        assert!(runtime::input_from_message(&bad, 200_000, 1_020, CLOCK, 9999).is_err());
    }
}

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = PathBuf::from(format!(
            "/tmp/exr-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, rows: &[Value]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, encode(rows)).unwrap();
        path
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn bus_row(input: &Value, sequence: u64) -> Value {
    let mut item = message(input);
    item.header.seq = sequence;
    item.header.at_unix_ms += input["stamp"]["at_us"].as_u64().unwrap() / 1_000;
    json!({"header":item.header,"payload_b64":STANDARD.encode(item.payload)})
}

#[test]
fn bus_import_keeps_new_generations_declarations_sequence_loss_and_estimated_arrival_limits() {
    let root = Root::new();
    let scenario = rows(RESUMED);
    let progresses: Vec<_> = scenario
        .iter()
        .filter(|row| row["input"]["kind"] == "progress")
        .collect();
    let sounds: Vec<_> = scenario
        .iter()
        .filter(|row| row["input"]["kind"] == "sound")
        .collect();
    let mut progress_rows =
        vec![json!({"topic":VoiceProgress::TOPIC,"declaration":VoiceProgress::declaration()})];
    progress_rows.extend(
        progresses
            .iter()
            .enumerate()
            .map(|(index, row)| bus_row(&row["input"], index as u64 + 1)),
    );
    let sound_rows = vec![
        json!({"topic":VoiceSound::TOPIC,"declaration":VoiceSound::declaration()}),
        bus_row(&sounds[0]["input"], 1),
        bus_row(&sounds.last().unwrap()["input"], 3),
    ];
    let imported = replay::import_bus(
        &[
            root.write("progress.jsonl", &progress_rows),
            root.write("sound.jsonl", &sound_rows),
        ],
        CLOCK,
    )
    .unwrap();
    assert_eq!(imported.anchor.at_us, 0);
    assert_eq!(imported.records.len(), progresses.len() + 2);
    assert!(matches!(imported.records[0].input, Input::Progress(_)));
    assert!(
        imported.records.iter().any(
            |record| matches!(&record.input,Input::Sound(sound) if sound.audio_generation == 2)
        )
    );
    assert!(
        imported
            .records
            .windows(2)
            .all(|records| records[0].at_us <= records[1].at_us)
    );
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.kind == WarningKind::MissingSequence)
    );
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.kind == WarningKind::EstimatedArrival
                && warning.detail.contains("not recorder receipt"))
    );
    assert!(
        !imported
            .warnings
            .iter()
            .any(|warning| warning.kind == WarningKind::LegacyWallClock)
    );
    let mut wrong = sound_rows;
    wrong[0]["declaration"]["kind"] = "latest".into();
    assert!(replay::import_bus(&[root.write("wrong-kind.jsonl", &wrong)], CLOCK).is_err());
}

#[test]
fn bad_new_contracts_fail_before_output_or_engine_mutation() {
    for fault in 0..5 {
        let mut scenario = rows(RESUMED);
        let row = scenario
            .iter_mut()
            .find(|row| row["input"]["held"] == true)
            .unwrap();
        match fault {
            0 => row["input"]["audio_generation"] = 0.into(),
            1 => row["input"]["state"] = "speaking".into(),
            2 => row["input"]["stamp"]["clock_id"] = "another-boot".into(),
            3 => {
                row["input"]
                    .as_object_mut()
                    .unwrap()
                    .remove("audio_generation");
            }
            _ => row["at_us"] = 0.into(),
        }
        let mut engine = Engine::new(catalogue(), true, CLOCK).unwrap();
        let mut written = Vec::new();
        assert!(
            replay::run(
                encode(&scenario).as_slice(),
                &mut written,
                &mut engine,
                None
            )
            .is_err()
        );
        assert!(written.is_empty());
        assert_eq!(engine.queued_playout_frames(), 0);
        assert_eq!(engine.rejected_inputs(), 0);
    }
}

#[test]
fn real_service_subscribes_to_exactly_one_complete_voice_protocol() {
    for resumable in [false, true] {
        let root = Root::new();
        let bus = Bus::new(&root.0).unwrap();
        let mut selected = if resumable {
            catalogue()
        } else {
            Catalogue::builtin()
        };
        selected.timing.body_lag_us = Some(0);
        let mut service = runtime::Service::new(&bus, selected, true, CLOCK).unwrap();
        let mut source = rows(RESUMED);
        for row in &mut source {
            row["input"]["stamp"]["at_us"] = 10_000_000.into();
        }
        for (kind, topic, declaration) in [
            ("room", RoomState::TOPIC, RoomState::declaration()),
            ("light", Light::TOPICS[0], Light::declaration()),
        ] {
            let payload = &source
                .iter()
                .find(|row| row["input"]["kind"] == kind)
                .unwrap()["input"]["value"];
            bus.publisher_declared(topic, &declaration, "fixture")
                .unwrap()
                .publish_json(13, payload)
                .unwrap();
        }
        let mut payload = progress(10_000_000, "speaking", 1, false)["input"].clone();
        payload.as_object_mut().unwrap().remove("kind");
        let progress: VoiceProgress = serde_json::from_value(payload).unwrap();
        for (topic, declaration, payload) in [
            (
                VoiceTiming::TOPIC,
                VoiceTiming::declaration(),
                serde_json::to_value(&progress.timing).unwrap(),
            ),
            (
                VoiceProgress::TOPIC,
                VoiceProgress::declaration(),
                serde_json::to_value(&progress).unwrap(),
            ),
        ] {
            bus.publisher_declared(topic, &declaration, "voice")
                .unwrap()
                .publish_json(3, &payload)
                .unwrap();
        }
        // Loosest limit first, strictest last (#912): the room and the
        // light, the voice's progress (500 ms), the joints (300 ms in the
        // engine), its playout and sound (200 ms).
        let joints = &source
            .iter()
            .find(|row| row["input"]["kind"] == "joints")
            .unwrap()["input"]["value"];
        bus.publisher_declared(JointState::TOPIC, &JointState::declaration(), "fixture")
            .unwrap()
            .publish_json(13, joints)
            .unwrap();
        let mut frame = source
            .iter()
            .find(|row| row["input"]["kind"] == "sound")
            .unwrap()["input"]
            .clone();
        frame.as_object_mut().unwrap().remove("kind");
        frame["start_us"] = 10_000_000.into();
        frame["end_us"] = 10_100_000.into();
        let sound: VoiceSound = serde_json::from_value(frame).unwrap();
        for (topic, declaration, payload) in [
            (
                VoicePlayout::TOPIC,
                VoicePlayout::declaration(),
                serde_json::to_value(&sound.playout).unwrap(),
            ),
            (
                VoiceSound::TOPIC,
                VoiceSound::declaration(),
                serde_json::to_value(&sound).unwrap(),
            ),
        ] {
            bus.publisher_declared(topic, &declaration, "voice")
                .unwrap()
                .publish_json(4, &payload)
                .unwrap();
        }
        // The wall clock just after publishing: a loaded machine's delay
        // before the step cannot age the fixture past its freshness.
        let wall = runtime::unix_ms();
        let output = service.step(|| 10_000_000, wall);
        assert_eq!(service.counts.accepted, 5);
        assert_eq!(service.counts.refused, 0);
        assert!(output.body.is_some() && output.light.is_some());
        assert!(
            output
                .state
                .behaviours
                .iter()
                .any(|name| name == "follow played sound")
        );
    }
}

#[test]
fn built_cli_uses_explicit_catalogue_and_default_shadow_for_the_new_fixtures() {
    let root = Root::new();
    let input = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/held-resumed.jsonl");
    let catalogue = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("rest-behaviours.json");
    let output = root.0.join("plan.jsonl");
    let result = Command::new(env!("CARGO_BIN_EXE_lamp-expression"))
        .args(["replay", "--clock-id", CLOCK, "--input"])
        .arg(input)
        .arg("--catalogue")
        .arg(catalogue)
        .arg("--output")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stdout.is_empty());
    let ticks = rows(&fs::read_to_string(output).unwrap());
    assert!(!ticks.is_empty());
    assert!(
        ticks
            .iter()
            .all(|tick| tick["output"]["body"].is_null() && tick["output"]["light"].is_null())
    );
    assert!(String::from_utf8_lossy(&result.stderr).contains("rejected: 4"));
}
