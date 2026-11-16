//! #1684: Smoky air and Dusty air against four synthetic traces, the
//! project lead's ruling in place of real smoke and dust: smoke, dust, a
//! cooking spike and a slow rise (`tests/fixtures/air-traces/`, with a
//! README that says where each number comes from). Each is replayed here in
//! virtual time through the shipped skills (`skills/smoky-air`,
//! `skills/dusty-air`) with the case runner `lamp-skills test` uses, by day,
//! a face in view and the voice resting, so a silence is the air's and not a
//! missing person's or the night's.
//!
//! What they show with the skills' present numbers: the smoke makes Smoky
//! air speak once and the dust makes Dusty air speak once, as meant; the
//! slow rise makes neither speak; and THE COOKING SPIKE MAKES SMOKY AIR
//! SPEAK ONCE, as its design allows for a pan
//! (`a_cooking_spike_gets_smoky_airs_line_once_...`). The numbers are not
//! changed here and the trace is not bent to hide it.
use lamp_msgs::AirState;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// The sensor's period (`AirState::declaration`), and each trace's step.
const STEP_S: u64 = 5;
/// Smoky air's level and rise, and Dusty air's level and hour, as their
/// `skill.toml` say them, for the facts each trace is held to below.
const SMOKY_LEVEL: f32 = 150.0;
const SMOKY_RISE: f32 = 100.0;
const SMOKY_RISE_WITHIN_S: u64 = 180;
const SMOKY_HOLD_S: u64 = 30;
const DUSTY_LEVEL: f32 = 125.0;
const DUSTY_HOLD_S: u64 = 3_600;

fn skills_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills")
}

fn traces() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/air-traces")
}

/// One trace: each line's offset in seconds and its `air/state` payload,
/// the bytes between `"air":` and the line's last brace, as
/// `tools/air-replay.sh` cuts them for a lamp's bus.
fn trace(name: &str) -> Vec<(u64, String)> {
    let path = traces().join(format!("{name}.jsonl"));
    let text = fs::read_to_string(&path).unwrap_or_else(|why| panic!("{}: {why}", path.display()));
    text.lines()
        .enumerate()
        .map(|(index, line)| {
            line.strip_prefix("{\"at_s\":")
                .and_then(|rest| rest.split_once(",\"air\":"))
                .and_then(|(at, rest)| {
                    Some((at.parse::<u64>().ok()?, rest.strip_suffix('}')?.to_owned()))
                })
                .unwrap_or_else(|| {
                    panic!(
                        "{name} line {}: not {{\"at_s\":N,\"air\":{{…}}}}",
                        index + 1
                    )
                })
        })
        .collect()
}

/// A trace's PM2.5, a reading a step.
fn pm2_5(name: &str) -> Vec<(u64, f32)> {
    trace(name)
        .into_iter()
        .map(|(at, payload)| {
            let air: AirState = serde_json::from_str(&payload).unwrap();
            (at, air.pm2_5.unwrap())
        })
        .collect()
}

/// What a trace is, in the terms of the skills' own numbers.
#[derive(Debug, PartialEq)]
struct Shape {
    /// Its length in seconds, and its highest PM2.5.
    lasts_s: u64,
    peak: f32,
    /// The most PM2.5 rose above the lowest reading of the 3 minutes before.
    most_rise: f32,
    /// The longest unbroken stretch at or over each skill's level, in seconds.
    over_smoky_s: u64,
    over_dusty_s: u64,
}

fn shape(name: &str) -> Shape {
    let readings = pm2_5(name);
    let longest = |level: f32| {
        let (mut longest, mut since) = (0, None);
        for &(at, pm) in &readings {
            if pm >= level {
                longest = longest.max(at - *since.get_or_insert(at));
            } else {
                since = None;
            }
        }
        longest
    };
    let most_rise = readings
        .iter()
        .map(|&(at, pm)| {
            let lowest = readings
                .iter()
                .filter(|(before, _)| *before < at && at - before <= SMOKY_RISE_WITHIN_S)
                .map(|&(_, before)| before)
                .fold(f32::INFINITY, f32::min);
            if lowest.is_finite() { pm - lowest } else { 0.0 }
        })
        .fold(0.0, f32::max);
    Shape {
        lasts_s: readings.last().unwrap().0,
        peak: readings.iter().map(|&(_, pm)| pm).fold(0.0, f32::max),
        // To a tenth, as the readings are.
        most_rise: (most_rise * 10.0).round() / 10.0,
        over_smoky_s: longest(SMOKY_LEVEL),
        over_dusty_s: longest(DUSTY_LEVEL),
    }
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Padded base64, as `lamp-bus record` writes a payload.
fn base64(bytes: &[u8]) -> String {
    let mut text = String::new();
    for chunk in bytes.chunks(3) {
        let mut group = [0u8; 3];
        group[..chunk.len()].copy_from_slice(chunk);
        let bits = u32::from(group[0]) << 16 | u32::from(group[1]) << 8 | u32::from(group[2]);
        for place in 0..4 {
            if place <= chunk.len() {
                text.push(ALPHABET[(bits >> (18 - 6 * place) & 63) as usize] as char);
            } else {
                text.push('=');
            }
        }
    }
    text
}

/// One message as a line of a recording (`lamp-bus record`'s format, which
/// the case runner reads).
fn recorded(topic: &str, type_name: &str, seq: usize, at_us: u64, payload: &str) -> String {
    format!(
        "{{\"header\":{{\"topic\":\"{topic}\",\"kind\":\"latest\",\"enc\":\"json\",\"type\":\"{type_name}\",\
         \"source\":\"air_traces\",\"boot\":\"00000000000a1684\",\"seq\":{seq},\"at_us\":{at_us},\
         \"at_unix_ms\":{}}},\"payload_b64\":\"{}\"}}\n",
        1_792_400_000_000 + at_us / 1_000,
        base64(payload.as_bytes())
    )
}

/// Readings as a recording in `dir`: each `air/state` at its own second,
/// the voice resting from the start, and one face at the desk half a second
/// before each reading (a face lasts 2 s to `lamp-skills`), so Dusty air's
/// `[[while]]` conditions hold throughout.
fn recording(dir: &Path, readings: &[(u64, String)]) -> PathBuf {
    const FACE: &str =
        r#"{"faces":[{"confidence":0.9,"id":"p","xmax":0.6,"xmin":0.4,"ymax":0.6,"ymin":0.35}]}"#;
    let folder = dir.join("recording");
    fs::create_dir_all(&folder).unwrap();
    let (mut air, mut faces) = (String::new(), String::new());
    for (index, (at, payload)) in readings.iter().enumerate() {
        let at_us = at * 1_000_000;
        air += &recorded("air/state", "lamp_msgs/AirState", index + 1, at_us, payload);
        faces += &recorded(
            "humans/faces/tracked",
            "lamp_msgs/Faces",
            index + 1,
            at_us.saturating_sub(500_000),
            FACE,
        );
    }
    fs::write(folder.join("air_state.jsonl"), air).unwrap();
    fs::write(folder.join("humans_faces_tracked.jsonl"), faces).unwrap();
    fs::write(
        folder.join("voice_state.jsonl"),
        recorded(
            "voice/state",
            "lamp_msgs/VoiceState",
            1,
            0,
            r#"{"state":"resting"}"#,
        ),
    )
    .unwrap();
    folder
}

/// What the shipped `skill` does on `readings`, replayed in virtual time
/// from 10:00: each call's second from the first reading and its name.
fn does_on(skill: &str, readings: &[(u64, String)]) -> Vec<(u64, String)> {
    let dir = tempfile::tempdir().unwrap();
    let recording = recording(dir.path(), readings);
    lamp_skills::replay_with(&skills_root().join(skill), &recording, &[], Some("10:00"))
        .unwrap_or_else(|why| panic!("{skill}: {why}"))
        .iter()
        .map(|made| (made.at_us / 1_000_000, made.call.named().0.to_owned()))
        .collect()
}

/// What the shipped `skill` does on the trace `name`.
fn does(skill: &str, name: &str) -> Vec<(u64, String)> {
    does_on(skill, &trace(name))
}

/// Smoky air's run: the small lift, the amber breath and its line, at once.
fn smoky_run(at: u64) -> Vec<(u64, String)> {
    ["body.gesture", "light", "say"]
        .map(|call| (at, call.to_owned()))
        .to_vec()
}

/// Every line of every trace is an `air/state` the sensor could send: an
/// `AirState` in its range (`AirState::check`: PM from 0 to 1,000 µg/m³),
/// one every 5 s from 0, CO2 left unknown. And each trace is what its README
/// says it is, in the skills' own terms.
#[test]
fn the_traces_are_air_states_in_the_sensors_range_and_as_their_readme_says() {
    for name in ["smoke", "dust", "cooking-spike", "slow-rise"] {
        let lines = trace(name);
        for (index, (at, payload)) in lines.iter().enumerate() {
            assert_eq!(*at, index as u64 * STEP_S, "{name} line {}", index + 1);
            let air: AirState = serde_json::from_str(payload)
                .unwrap_or_else(|why| panic!("{name} line {}: {why}", index + 1));
            air.check()
                .unwrap_or_else(|why| panic!("{name} line {}: {}", index + 1, why.0));
            let (pm1, pm2_5, pm4, pm10) = (
                air.pm1_0.unwrap(),
                air.pm2_5.unwrap(),
                air.pm4_0.unwrap(),
                air.pm10.unwrap(),
            );
            assert!(
                pm1 <= pm2_5 && pm2_5 <= pm4 && pm4 <= pm10,
                "{name} at {at} s"
            );
            assert_eq!(air.co2_ppm, None, "{name} at {at} s");
        }
    }
    // Smoke: 468 up within a minute, then 5 minutes at 480.
    assert_eq!(
        shape("smoke"),
        Shape {
            lasts_s: 690,
            peak: 480.0,
            most_rise: 468.0,
            over_smoky_s: 420,
            over_dusty_s: 435,
        }
    );
    // Dust: 20 minutes up to 180, never more than 22.5 in 3 minutes, and
    // over Dusty air's level for 74 minutes.
    assert_eq!(
        shape("dust"),
        Shape {
            lasts_s: 5_760,
            peak: 180.0,
            most_rise: 22.5,
            over_smoky_s: 4_200,
            over_dusty_s: 4_450,
        }
    );
    // A cooking spike: 288 up in 90 s, a minute at 300, down in 2 minutes.
    assert_eq!(
        shape("cooking-spike"),
        Shape {
            lasts_s: 480,
            peak: 300.0,
            most_rise: 288.0,
            over_smoky_s: 165,
            over_dusty_s: 180,
        }
    );
    // A slow rise: 180 up over 90 minutes, 6 in any 3.
    assert_eq!(
        shape("slow-rise"),
        Shape {
            lasts_s: 5_850,
            peak: 200.0,
            most_rise: 6.0,
            over_smoky_s: 1_650,
            over_dusty_s: 2_400,
        }
    );
    // Each trace is clearly on one side of each number, as the README says:
    // at least twice it or at most half of it, but for Dusty air's hour,
    // which the dust passes by 14 minutes and the slow rise misses by 20.
    let (smoke, dust, spike, slow) = (
        shape("smoke"),
        shape("dust"),
        shape("cooking-spike"),
        shape("slow-rise"),
    );
    assert!(smoke.most_rise >= 2.0 * SMOKY_RISE && smoke.peak >= 2.0 * SMOKY_LEVEL);
    assert!(smoke.over_smoky_s >= 2 * SMOKY_HOLD_S && smoke.over_dusty_s <= DUSTY_HOLD_S / 2);
    assert!(dust.most_rise <= SMOKY_RISE / 2.0 && dust.over_dusty_s >= DUSTY_HOLD_S + 14 * 60);
    assert!(spike.most_rise >= 2.0 * SMOKY_RISE && spike.peak >= 2.0 * SMOKY_LEVEL);
    assert!(spike.over_smoky_s >= 2 * SMOKY_HOLD_S && spike.over_dusty_s <= DUSTY_HOLD_S / 2);
    assert!(slow.most_rise <= SMOKY_RISE / 2.0 && slow.peak > SMOKY_LEVEL);
    assert!(slow.over_dusty_s <= DUSTY_HOLD_S - 20 * 60);
}

/// Smoke: 12 µg/m³, then up to 480 within a minute and 5 minutes there.
/// Smoky air lifts, breathes amber and says its line once, 30 s after the
/// air passed both its lines (at 200 s: 168, and 156 above the 3 minutes
/// before), and nothing more while the smoke stays or clears. Dusty air
/// says nothing: 7 minutes over its level is not its hour.
#[test]
fn smoke_makes_smoky_air_speak_once_and_dusty_air_never() {
    assert_eq!(does("smoky-air", "smoke"), smoky_run(230));
    assert_eq!(does("dusty-air", "smoke"), []);
}

/// Dust: 30 µg/m³, up to 180 over 20 minutes and 65 minutes there. Dusty air
/// says its line once, an hour after the air passed 125 (at 1,060 s). Smoky
/// air says nothing, though the air is over its level for 70 minutes: it
/// never rose by 100 within 3 minutes.
#[test]
fn dust_makes_dusty_air_speak_once_and_smoky_air_never() {
    assert_eq!(does("dusty-air", "dust"), [(4_660, "say".to_owned())]);
    assert_eq!(does("smoky-air", "dust"), []);
}

/// ONE LINE FOR A PAN, as the design allows (#1684): a cooking spike,
/// 12 µg/m³ up to 300 in 90 s, a minute there and back down in 2 minutes,
/// makes Smoky air say "The air reads like smoke. If something's burning,
/// please check it." at 255 s, 30 s after the spike passed 150. To Smoky
/// air's rule a spike is not smoke only if it is over 150 for under 30 s
/// (its `never-on-a-short-puff` case), and no pan clears that fast: this
/// one is over 150 for 165 s. On PM2.5 alone a pan and burnt toast are the
/// same shape, and the skill's `toast` case wants the line within 2
/// minutes, so its SKILL.md says "Frying in the same room may set it off,
/// once". The numbers are the skill's own, unchanged, and the trace is not
/// shortened to pass. Dusty air says nothing.
#[test]
fn a_cooking_spike_gets_smoky_airs_line_once_as_its_design_allows() {
    assert_eq!(does("smoky-air", "cooking-spike"), smoky_run(255));
    assert_eq!(does("dusty-air", "cooking-spike"), []);
}

/// A slow rise: 20 µg/m³ up to 200 over 90 minutes. Neither speaks. Smoky
/// air: the air ends over its level, but rose 6 in any 3 minutes, not 100.
/// Dusty air: the air has been over 125 for 40 minutes when the trace ends,
/// not an hour; left there longer it speaks at its hour, and rightly.
#[test]
fn a_slow_rise_makes_neither_speak() {
    assert_eq!(does("smoky-air", "slow-rise"), []);
    assert_eq!(does("dusty-air", "slow-rise"), []);
    // Not the trace's shape but its length: the same air left at 200 for
    // 25 minutes more has been over 125 (from 3,450 s) for an hour at
    // 7,050 s, and Dusty air says so then. Smoky air still says nothing.
    let mut longer = trace("slow-rise");
    let (end, top) = longer.last().cloned().unwrap();
    longer.extend(
        (end + STEP_S..=end + 1_500)
            .step_by(STEP_S as usize)
            .map(|at| (at, top.clone())),
    );
    assert_eq!(does_on("dusty-air", &longer), [(7_050, "say".to_owned())]);
    assert_eq!(does_on("smoky-air", &longer), []);
}

/// Why `tools/air-replay.sh` stops lamp-air for a replay on a lamp: the
/// room's own readings between a trace's spoil it. Here the slow rise, on
/// which Smoky air rightly says nothing, with a clean room's 12 µg/m³ two
/// seconds after each of its readings, as a live sensor beside the replay
/// would say them. Every real reading is in the 3 minutes the rise looks
/// back over, so the replayed level is "100 above" from 112 on; each real
/// reading is under 150, but the replayed one before it is 2 s old, inside
/// the 10 s `gap`, so the hold never ends; and Smoky air speaks at 4,230 s,
/// 30 s after the climb reached 150, for a rise of 6 in 3 minutes. So a
/// replay beside a live sensor judges nothing.
#[test]
fn a_rooms_own_readings_between_a_traces_spoil_the_replay() {
    const ROOM: &str = r#"{"pm1_0":10.8,"pm2_5":12.0,"pm4_0":12.2,"pm10":12.4,"humidity":45.0,"temperature":36.0,"co2_ppm":650,"feel":"fresh"}"#;
    let mut crossed = Vec::new();
    for (at, payload) in trace("slow-rise") {
        crossed.push((at, payload));
        crossed.push((at + 2, ROOM.to_owned()));
    }
    assert_eq!(does_on("smoky-air", &crossed), smoky_run(4_230));
}

/// A skill's `[trigger]` table as `tools/air-replay.sh` reads it on a lamp:
/// its lines without comments, trailing spaces or blanks.
fn trigger_of(skill_toml: &str) -> String {
    skill_toml
        .lines()
        .skip_while(|line| *line != "[trigger]")
        .skip(1)
        .take_while(|line| !line.starts_with('['))
        .map(|line| line.split('#').next().unwrap_or_default().trim_end())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The lamp kit refuses a lamp whose Smoky air or Dusty air has another
/// `[trigger]` than the one the traces were shaped against. That one is
/// written out in the kit, and is each skill's own, to the letter: a number
/// changed in a `skill.toml` fails here until the traces, their README and
/// the kit have been looked at again.
#[test]
fn the_lamp_kit_holds_each_skills_present_trigger() {
    let kit =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/air-replay.sh"))
            .unwrap();
    for skill in ["smoky-air", "dusty-air"] {
        let own = fs::read_to_string(skills_root().join(skill).join("skill.toml")).unwrap();
        let own = trigger_of(&own);
        assert!(own.contains("field = \"pm2_5\""), "{skill}: {own}");
        let written = kit
            .split_once(&format!("    {skill})\n      cat <<'EOF'\n"))
            .and_then(|(_, rest)| rest.split_once("\nEOF\n"))
            .unwrap_or_else(|| panic!("the kit writes out no trigger for {skill}"))
            .0;
        assert_eq!(written, own, "{skill}");
    }
}
