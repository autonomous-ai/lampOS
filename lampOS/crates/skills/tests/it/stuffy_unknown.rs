//! lamp-52e6, build 12295 (#6592): Stuffy air never spoke with CO2 at
//! 2,200 to 2,300 ppm for the whole afternoon, and lamp-skills said no line
//! of it at all. The air part sends a reading whose CO2 its sensor does not
//! know as `co2_ppm: null` (`feel: unknown`, `crates/air/src/lib.rs`'s
//! `reading`): the 52e6 kit read 2,277 at 14:28, nothing over 1,200 in
//! its window's first minute, and 2,200+ again from 14:30:04, a fall and
//! rise no room's air makes that fast. A plain `above` trigger took such a
//! stretch, past its 1-minute `gap`, as the air having cleared: the spell
//! began again, its 10 minutes with it, and nothing was said. The skill
//! here is the shipped `skills/stuffy-air`, fed the air part's own
//! `AirState` messages from lamp-skills' start, the air already stuffy.
use lamp_msgs::{AirFeel, AirState, SkillStage};
use lamp_skills::{Call, Engine, Run, load};
use std::path::Path;

const S: u64 = 1_000_000;

/// The air part's message: `co2` ppm, or none while the sensor does not
/// know it.
fn air(co2: Option<u16>) -> Vec<u8> {
    serde_json::to_vec(&AirState {
        pm1_0: Some(3.0),
        pm2_5: Some(4.0),
        pm4_0: Some(4.0),
        pm10: Some(5.0),
        humidity: Some(55.0),
        temperature: Some(29.0),
        co2_ppm: co2,
        feel: AirFeel::of_co2(co2),
    })
    .unwrap()
}

/// What the skill did each second of `seconds`: its Triggered times, its
/// says, its dim given back, and every line it left (events and `not run`).
#[derive(Default)]
struct Seen {
    triggered: Vec<u64>,
    said: Vec<u64>,
    ended: Vec<u64>,
    lines: usize,
}

/// lamp-skills from its start (0 s) at 14:00 local, the voice resting, the
/// air every 5 s from `co2(at)`.
fn run(seconds: u64, co2: impl Fn(u64) -> Option<u16>) -> Seen {
    let skill =
        load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/stuffy-air")).unwrap();
    let mut engine = Engine::new(vec![skill], true);
    // A midnight, UTC (the offset given is 0).
    let midnight_ms = 1_792_022_400_000;
    let mut seen = Seen::default();
    for at in 0..seconds {
        engine.clock(0, at * S, midnight_ms + 14 * 3_600_000 + at * 1_000);
        let mut runs: Vec<Run> = engine.tick(at * S);
        runs.extend(engine.observe("voice/state", at * S, br#"{"state":"resting"}"#, None));
        if at % 5 == 0 {
            runs.extend(engine.observe("air/state", at * S, &air(co2(at)), None));
        }
        seen.lines += engine.take_dropped().len();
        for run in runs {
            seen.lines += run.events.len();
            if run
                .events
                .iter()
                .any(|event| event.stage == SkillStage::Triggered)
            {
                seen.triggered.push(at);
            }
            if run.calls.iter().any(|call| matches!(call, Call::Say(_))) {
                seen.said.push(at);
            }
            if run.calls == [Call::Ends] {
                seen.ended.push(at);
            }
        }
    }
    seen
}

/// Stuffy (2,250) from the start, the sensor not knowing its CO2 for 90 s
/// once every 4 minutes: one droop and line once its 10 minutes have
/// passed, the unknown stretches neither ending the spell nor counting as
/// good air; none again in the hour while it stays stuffy; again only once
/// it has been under 1,000 for 10 minutes and stuffy for 10 more.
#[test]
fn stuffy_from_the_start_through_unknown_readings_speaks_once_a_spell() {
    // 0 to 3,600 s stuffy, 3,600 to 4,320 s at 900 (12 minutes of good
    // air), then stuffy to the end.
    let seen = run(6_000, |at| {
        if at % 240 >= 150 {
            return None;
        }
        Some(if (3_600..4_320).contains(&at) {
            900
        } else {
            2_250
        })
    });
    // The first known reading at or past 600 s of the spell begun at 0.
    let first = 600;
    // The second spell begins at the first stuffy reading after the good
    // air (4,320 s) and comes due at the first known one 10 minutes on.
    let second = (4_920..).step_by(5).find(|at| at % 240 < 150).unwrap();
    assert_eq!(
        seen.triggered,
        [first, second],
        "lines: {}, said at {:?}",
        seen.lines,
        seen.said
    );
    assert_eq!(seen.said, [first, second]);
}

/// The sensor stops knowing its CO2 for good: what was held until the
/// spell ends (the dim) is given back 15 minutes later, as when it falls
/// silent (`SILENT_ENDS_US`), not kept for ever by its unknown readings.
#[test]
fn a_sensor_that_stops_knowing_its_co2_gives_the_dim_back() {
    let seen = run(3_000, |at| (at < 900).then_some(2_250));
    assert_eq!(seen.triggered, [600]);
    assert_eq!(seen.ended, [900 + 15 * 60]);
}
