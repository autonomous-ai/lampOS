//! The idle layer, sleep and waking in replay, import and the live service
//! (task 36, #714): an hour replayed, touch and reminders imported, and the
//! local time of day told.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use lamp_bus::Bus;
use lamp_expression::{Catalogue, Engine, Input, replay, runtime};
use lamp_msgs::{
    ClockStamp, ControlGesture, ControlIntent, ControlSource, Faces, Gesture, JointState, Light,
    Room, RoomState, ScheduleDue, ScheduleNotice, Voice, VoiceEvent, VoiceProgress, VoiceTiming,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Cursor},
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

const CLOCK: &str = "alive-replay-boot";
const SECOND: u64 = 1_000_000;
const MINUTE: u64 = 60 * SECOND;

fn catalogue() -> Catalogue {
    Catalogue::from_json(include_str!("../../alive-behaviours.json")).unwrap()
}

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Folder(PathBuf);

impl Folder {
    fn new() -> Self {
        // Short, under /tmp: a bus's socket paths must fit a Unix address.
        let path = PathBuf::from(format!(
            "/tmp/al-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A written quiet stretch at rest, as the lamp's parts say it: the voice,
/// the joints and the room every 100 ms, nobody seen every 600 ms.
fn quiet(minutes: u64) -> Vec<u8> {
    let start = 1_000 * SECOND;
    let mut sequence = 0;
    let mut stamp = |at_us| {
        sequence += 1;
        ClockStamp {
            clock_id: CLOCK.into(),
            at_us,
            sequence,
        }
    };
    let mut records = vec![replay::Record {
        at_us: start,
        input: Input::Light {
            stamp: stamp(start),
            value: Light::new(true, "warm", 1).unwrap(),
        },
    }];
    let mut at_us = start;
    while at_us <= start + minutes * MINUTE {
        let timing = VoiceTiming {
            stamp: stamp(at_us),
            session_id: "voice-one".into(),
            turn: 1,
            request: 11,
            state: Voice::Resting,
            accepting_input: true,
            event: VoiceEvent::State,
            first_audio_us: None,
        };
        records.push(replay::Record {
            at_us,
            input: Input::Progress(VoiceProgress {
                timing,
                audio_generation: 1,
                held: false,
            }),
        });
        records.push(replay::Record {
            at_us,
            input: Input::Joints {
                stamp: stamp(at_us),
                value: JointState {
                    name: ["base_yaw", "shoulder", "elbow", "head_roll", "head_pitch"]
                        .map(str::to_owned)
                        .to_vec(),
                    position: vec![0.0; 5],
                    commanded: vec![],
                    velocity: vec![],
                    effort: vec![],
                },
            },
        });
        records.push(replay::Record {
            at_us,
            input: Input::Room {
                stamp: stamp(at_us),
                value: RoomState { state: Room::Quiet },
            },
        });
        if at_us.is_multiple_of(600_000) {
            records.push(replay::Record {
                at_us,
                input: Input::Faces {
                    stamp: stamp(at_us),
                    value: Faces { faces: vec![] },
                },
            });
        }
        at_us += 100_000;
    }
    let mut bytes = Vec::new();
    for record in records {
        serde_json::to_writer(&mut bytes, &record).unwrap();
        bytes.push(b'\n');
    }
    bytes
}

#[test]
fn a_stretch_longer_than_ten_minutes_replays_only_when_asked_for_as_long() {
    let bytes = quiet(15);
    let mut engine = Engine::new(catalogue(), true, CLOCK).unwrap();
    let refused = replay::run(Cursor::new(&bytes), io::sink(), &mut engine, None);
    assert!(refused.unwrap_err().0.contains("horizon"));
    let mut engine = Engine::new(catalogue(), true, CLOCK).unwrap();
    let summary = replay::run_bounded(
        Cursor::new(&bytes),
        io::sink(),
        &mut engine,
        None,
        replay::LONG_BOUNDS,
    )
    .unwrap();
    assert_eq!(summary.rejected, 0, "{:?}", summary.warnings);
    // One breath layer that came in once and stayed, and a drift now and
    // then: nine or more in fifteen minutes, each of 30 to 90 s apart.
    let breathing = summary.behaviours["breathing"];
    assert_eq!(breathing.starts, 1);
    assert!(breathing.ticks > summary.ticks - 30, "{breathing:?}");
    let drifting = summary.behaviours["drifting"];
    assert!((9..=28).contains(&drifting.starts), "{drifting:?}");
    // 150 ticks of 3 s for each drift.
    assert!(drifting.ticks >= drifting.starts * 150, "{drifting:?}");
    assert!(!summary.behaviours.contains_key("asleep"));
    // The largest step is the layer coming in over 0.35 s; the farthest a
    // breath and a drift together.
    let step = summary.largest_step.unwrap();
    assert!(step.head_pitch < 0.0026, "{step:?}");
    assert_eq!(step.shoulder, 0.0);
    let farthest = summary.farthest.unwrap();
    assert!(farthest.head_pitch <= 0.065 + 1e-9, "{farthest:?}");
    assert_eq!((farthest.base_yaw, farthest.elbow), (0.0, 0.0));
}

#[test]
fn the_long_bounds_hold_an_hour_and_no_more() {
    const { assert!(replay::LONG_BOUNDS.duration_us >= 60 * MINUTE) };
    const { assert!(replay::LONG_BOUNDS.duration_us < 70 * MINUTE) };
    assert_eq!(replay::DEFAULT_BOUNDS.duration_us, replay::MAX_DURATION_US);
    assert_eq!(replay::DEFAULT_BOUNDS.records, replay::MAX_RECORDS);
    // A written hour at rest is about 108,000 inputs.
    const { assert!(replay::LONG_BOUNDS.records >= 108_000 * 4) };
}

fn bus_line(
    topic: &str,
    type_name: &str,
    kind: &str,
    payload: Value,
    sequence: u64,
    wall_ms: u64,
) -> Value {
    json!({
        "header": {"topic":topic,"kind":kind,"enc":"json","type":type_name,"source":"part","boot":"0123456789abcdef","seq":sequence,"at_us":sequence,"at_unix_ms":wall_ms},
        "payload_b64":STANDARD.encode(serde_json::to_vec(&payload).unwrap())
    })
}

fn write(folder: &Folder, name: &str, rows: &[Value]) -> PathBuf {
    let path = folder.0.join(name);
    let mut text = String::new();
    for row in rows {
        text.push_str(&row.to_string());
        text.push('\n');
    }
    fs::write(&path, text).unwrap();
    path
}

#[test]
fn an_import_takes_touch_and_reminders_and_can_be_told_the_time_of_day() {
    let folder = Folder::new();
    // 07:00 in Vietnam (UTC+7) at the anchor; the touch 150 s later.
    let anchor_wall = 0;
    let anchor = VoiceTiming {
        stamp: ClockStamp {
            clock_id: CLOCK.into(),
            at_us: 5 * MINUTE,
            sequence: 1,
        },
        session_id: "voice-one".into(),
        turn: 1,
        request: 11,
        state: Voice::Resting,
        accepting_input: true,
        event: VoiceEvent::State,
        first_audio_us: None,
    };
    let inner = ClockStamp {
        clock_id: CLOCK.into(),
        at_us: 1_000,
        sequence: 3,
    };
    let touch = ControlGesture {
        stamp: inner.clone(),
        session_id: "controls-one".into(),
        source: ControlSource::Base,
        name: "base".into(),
        gesture: Gesture::Tap,
        intent: ControlIntent::Stop,
        duration_us: 100,
        released_us: 900,
        enabled: true,
    };
    let due = ScheduleDue {
        stamp: inner,
        store_id: "0123456789abcdef0123456789abcdef".into(),
        revision: 1,
        enabled: true,
        ready: true,
        pending: 1,
        waiting_for_clock: 0,
        oldest: Some(ScheduleNotice {
            id: 4,
            delivery: 1,
            due_unix_ms: 0,
            late_by_ms: None,
            late: false,
        }),
    };
    let files = [
        write(
            &folder,
            "voice.jsonl",
            &[bus_line(
                VoiceTiming::TOPIC,
                VoiceTiming::TYPE,
                "latest",
                serde_json::to_value(&anchor).unwrap(),
                1,
                anchor_wall,
            )],
        ),
        write(
            &folder,
            "controls.jsonl",
            &[bus_line(
                ControlGesture::TOPIC,
                ControlGesture::TYPE,
                "event",
                serde_json::to_value(&touch).unwrap(),
                1,
                anchor_wall + 150_000,
            )],
        ),
        write(
            &folder,
            "schedule.jsonl",
            &[bus_line(
                ScheduleDue::TOPIC,
                ScheduleDue::TYPE,
                "latest",
                serde_json::to_value(&due).unwrap(),
                1,
                anchor_wall + 90_000,
            )],
        ),
    ];
    let mut imported = replay::import_bus(&files, CLOCK).unwrap();
    let kinds: Vec<&str> = imported
        .records
        .iter()
        .map(|record| record.input.topic())
        .collect();
    assert_eq!(
        kinds,
        [
            VoiceTiming::TOPIC,
            ScheduleDue::TOPIC,
            ControlGesture::TOPIC
        ]
    );
    assert_eq!(imported.records[2].at_us, 5 * MINUTE + 150 * SECOND);
    replay::add_daytime(&mut imported, 420).unwrap();
    let days: Vec<(u64, u16)> = imported
        .records
        .iter()
        .filter_map(|record| match &record.input {
            Input::DayTime { stamp, minute } => Some((stamp.at_us, *minute)),
            _ => None,
        })
        .collect();
    assert_eq!(
        days,
        [(5 * MINUTE, 420), (6 * MINUTE, 421), (7 * MINUTE, 422)]
    );
    // In arrival order, every one accepted (this voice speaks the first
    // protocol, as the anchor of an older recording would).
    let mut first = catalogue();
    first.resumable_voice = false;
    let mut engine = Engine::new(first, true, CLOCK).unwrap();
    for record in imported.records {
        engine.ingest(record.at_us, record.input).unwrap();
    }
}

#[test]
fn the_minute_of_the_day_is_east_of_utc_and_wraps() {
    assert_eq!(runtime::minute_of_day(0, 420), 420);
    assert_eq!(runtime::minute_of_day(17 * 3_600_000 + 59_999, 420), 0);
    assert_eq!(runtime::minute_of_day(30 * 60_000, -60), 1_410);
    assert_eq!(runtime::minute_of_day(1_760_430_000_000, 0), 500);
}

#[test]
fn the_live_service_tells_the_planner_the_time_of_day_every_minute() {
    let folder = Folder::new();
    let bus = Bus::new(folder.0.clone()).unwrap();
    let mut service = runtime::Service::new(&bus, catalogue(), false, "test")
        .unwrap()
        .with_local_time(420);
    let at = |us: u64| move || us;
    service.step(at(10 * SECOND), 1_000);
    assert_eq!((service.counts.accepted, service.counts.refused), (1, 0));
    service.step(at(40 * SECOND), 31_000);
    assert_eq!(service.counts.accepted, 1);
    service.step(at(70 * SECOND), 61_000);
    assert_eq!(service.counts.accepted, 2);
    // Without it, no time of day is told.
    drop(service);
    let mut quiet = runtime::Service::new(&bus, catalogue(), false, "test").unwrap();
    quiet.step(at(10 * SECOND), 1_000);
    assert_eq!(quiet.counts.accepted, 0);
}

#[test]
fn serving_a_catalogue_with_sleep_needs_the_local_time() {
    let folder = Folder::new();
    let catalogue = folder.0.join("alive.json");
    fs::write(&catalogue, include_str!("../../alive-behaviours.json")).unwrap();
    let bus = folder.0.join("b");
    fs::create_dir(&bus).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_lamp-expression"))
        .args([
            "serve",
            "--clock-id",
            "test",
            "--seconds",
            "1",
            "--bus-root",
        ])
        .arg(&bus)
        .arg("--catalogue")
        .arg(&catalogue)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("--utc-offset-minutes"));
    let result = Command::new(env!("CARGO_BIN_EXE_lamp-expression"))
        .args(["serve", "--clock-id", "test", "--seconds", "1"])
        .args(["--utc-offset-minutes", "420", "--bus-root"])
        .arg(&bus)
        .arg("--catalogue")
        .arg(&catalogue)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
