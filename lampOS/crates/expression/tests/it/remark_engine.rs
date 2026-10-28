//! A glance at a remark (`voice/remarked`, #4175): someone in a busy room
//! said something about the lamp and the voice said nothing. Head only,
//! once, the startle's own move, at most once a minute, never while the
//! lamp thinks or speaks, and never sharing the startle's cooldown.
use lamp_expression::{Catalogue, Engine, Input, Output, Trigger, runtime, startle};
use lamp_msgs::{
    ClockStamp, JointState, Light, Room, RoomState, SuddenSound, Voice, VoiceEvent, VoiceProgress,
    VoiceRemarked, VoiceTiming,
};

const CLOCK: &str = "remark-test-boot";
const REMARK: &str = "glance at a remark";
const SOUND: &str = "glance at a sound";
const SECOND: u64 = 1_000_000;

fn catalogue(with_remark: bool) -> Catalogue {
    let mut value = Catalogue::from_json(include_str!("../../rest-behaviours.json")).unwrap();
    // This is an injected zero lag for planner tests, not a motor measurement.
    value.timing.body_lag_us = Some(0);
    let startle = startle::parse(include_str!("../../../../skills/startle/skill.toml")).unwrap();
    if with_remark {
        value.behaviours.push(startle::remark(&startle).unwrap());
    }
    value.behaviours.push(startle);
    value.check().unwrap();
    value
}

struct Lamp {
    engine: Engine,
    sequence: u64,
    state: Voice,
}

impl Lamp {
    fn new(with_remark: bool, state: Voice) -> Self {
        let mut lamp = Self {
            engine: Engine::new(catalogue(with_remark), true, CLOCK).unwrap(),
            sequence: 0,
            state,
        };
        let stamp = lamp.stamp(0);
        lamp.send(Input::Light {
            stamp,
            value: Light::new(true, "blue", 2).unwrap(),
        });
        lamp.step(0);
        lamp
    }

    fn stamp(&mut self, at_us: u64) -> ClockStamp {
        self.sequence += 1;
        ClockStamp {
            clock_id: CLOCK.into(),
            at_us,
            sequence: self.sequence,
        }
    }

    fn send(&mut self, input: Input) {
        let at = input.stamp().at_us;
        self.engine.ingest(at, input).unwrap();
    }

    /// The voice's state, fresh joints and a busy room, then a tick.
    fn step(&mut self, at: u64) -> Output {
        let timing = VoiceTiming {
            stamp: self.stamp(at),
            session_id: "voice-one".into(),
            turn: 1,
            request: 11,
            state: self.state,
            accepting_input: true,
            event: VoiceEvent::State,
            first_audio_us: None,
        };
        self.send(Input::Progress(VoiceProgress {
            timing,
            audio_generation: 1,
            held: false,
        }));
        let stamp = self.stamp(at);
        self.send(Input::Joints {
            stamp,
            value: JointState {
                name: ["base_yaw", "shoulder", "elbow", "head_roll", "head_pitch"]
                    .map(str::to_owned)
                    .to_vec(),
                position: vec![0.0; 5],
                commanded: vec![],
                velocity: vec![],
                effort: vec![],
            },
        });
        let stamp = self.stamp(at);
        self.send(Input::Room {
            stamp,
            value: RoomState { state: Room::Busy },
        });
        self.engine.tick(at)
    }

    fn remark(&mut self, at: u64) {
        let stamp = self.stamp(at);
        self.send(Input::Remarked {
            stamp,
            value: VoiceRemarked { request: 12 },
        });
    }

    fn sound(&mut self, at: u64) {
        let stamp = self.stamp(at);
        self.send(Input::Sudden {
            stamp,
            value: SuddenSound {
                peak_db: -18.0,
                floor_db: -52.0,
                loud_ms: 60,
                short: true,
                ago_ms: 300,
            },
        });
    }

    /// Ticks every 100 ms from `from` to `to`; whether `clip` ran and the
    /// highest the head went (the most negative pitch).
    fn watch(&mut self, clip: &str, from: u64, to: u64) -> (bool, f64) {
        let (mut ran, mut lifted) = (false, 0.0f64);
        let mut at = from;
        while at <= to {
            let out = self.step(at);
            ran |= out.state.behaviours.iter().any(|name| name == clip);
            if let Some(body) = out.body {
                lifted = lifted.min(body.offsets.head_pitch);
            }
            at += 100_000;
        }
        (ran, lifted)
    }
}

#[test]
fn a_remark_while_listening_gets_one_glance_and_the_head_comes_back() {
    let mut lamp = Lamp::new(true, Voice::Listening);
    // The listening posture's own tilt, before anything was said.
    let (_, posture) = lamp.watch(REMARK, 100_000, SECOND);
    lamp.remark(SECOND);
    let (ran, lifted) = lamp.watch(REMARK, SECOND, 2_800_000);
    assert!(ran);
    assert!(lifted - posture < -0.05, "{lifted} {posture}");
    let (ran, lifted) = lamp.watch(REMARK, 3_000_000, 4_000_000);
    assert!(!ran && (lifted - posture).abs() < 1e-9, "{ran} {lifted}");
}

#[test]
fn a_remark_while_resting_gets_a_glance() {
    let mut lamp = Lamp::new(true, Voice::Resting);
    lamp.remark(500_000);
    assert!(lamp.watch(REMARK, 500_000, 2_200_000).0);
}

#[test]
fn a_second_remark_inside_a_minute_gets_nothing_and_one_after_it_a_glance() {
    let mut lamp = Lamp::new(true, Voice::Listening);
    lamp.remark(500_000);
    assert!(lamp.watch(REMARK, 500_000, 2_200_000).0);
    lamp.remark(10 * SECOND);
    assert!(!lamp.watch(REMARK, 10 * SECOND, 12 * SECOND).0);
    lamp.remark(59 * SECOND);
    assert!(!lamp.watch(REMARK, 59 * SECOND, 60 * SECOND).0);
    lamp.remark(62 * SECOND);
    assert!(lamp.watch(REMARK, 62 * SECOND, 64 * SECOND).0);
}

#[test]
fn a_remark_while_the_lamp_thinks_or_speaks_gets_nothing() {
    for state in [Voice::Thinking, Voice::Speaking] {
        let mut lamp = Lamp::new(true, Voice::Listening);
        lamp.step(100_000);
        lamp.state = state;
        lamp.step(200_000);
        lamp.remark(300_000);
        let (ran, _) = lamp.watch(REMARK, 300_000, 2_000_000);
        assert!(!ran, "{state:?}");
    }
}

#[test]
fn a_remark_the_loop_heard_late_is_let_go() {
    let mut lamp = Lamp::new(true, Voice::Listening);
    lamp.step(SECOND);
    lamp.remark(SECOND);
    // The next tick comes 2 s after the remark: a late glance reads as a
    // machine catching up, so none.
    assert!(!lamp.watch(REMARK, 3 * SECOND, 5 * SECOND).0);
}

#[test]
fn without_the_switch_a_remark_gets_nothing() {
    let mut lamp = Lamp::new(false, Voice::Listening);
    let (_, posture) = lamp.watch(REMARK, 100_000, 400_000);
    lamp.remark(500_000);
    let (ran, lifted) = lamp.watch(REMARK, 500_000, 2_200_000);
    assert!(!ran && (lifted - posture).abs() < 1e-9, "{ran} {lifted}");
}

#[test]
fn the_switch_is_off_unless_on_and_the_remark_is_no_ordinary_input() {
    assert_eq!(runtime::REMARK_GLANCE_ENVIRONMENT, "LAMP_REMARK_GLANCE");
    assert!(!runtime::remark_glance_on(None));
    assert!(!runtime::remark_glance_on(Some("off")));
    assert!(!runtime::remark_glance_on(Some("1")));
    assert!(runtime::remark_glance_on(Some("on")));
    assert!(
        runtime::INPUTS
            .iter()
            .chain(&runtime::HEAD_INPUTS)
            .all(|(topic, _)| *topic != VoiceRemarked::TOPIC)
    );
    assert_eq!(
        runtime::REMARK_INPUTS,
        [(VoiceRemarked::TOPIC, VoiceRemarked::TYPE)]
    );
}

#[test]
fn the_startle_and_the_remark_glance_keep_their_own_cooldowns() {
    // A bang first: the startle's 30 s does not hold back a remark.
    let mut lamp = Lamp::new(true, Voice::Listening);
    lamp.sound(500_000);
    assert!(lamp.watch(SOUND, 500_000, 2_200_000).0);
    lamp.remark(4 * SECOND);
    assert!(lamp.watch(REMARK, 4 * SECOND, 6 * SECOND).0);
    // A remark first: its minute does not hold back a startle.
    let mut lamp = Lamp::new(true, Voice::Listening);
    lamp.remark(500_000);
    assert!(lamp.watch(REMARK, 500_000, 2_200_000).0);
    lamp.sound(4 * SECOND);
    let (sound, remark) = (4 * SECOND..=6 * SECOND)
        .step_by(100_000)
        .map(|at| lamp.step(at).state.behaviours)
        .fold((false, false), |(sound, remark), names| {
            (
                sound || names.iter().any(|name| name == SOUND),
                remark || names.iter().any(|name| name == REMARK),
            )
        });
    assert!(sound && !remark, "{sound} {remark}");
}

#[test]
fn the_remark_glance_is_the_startles_move_and_its_bounds_hold() {
    let startle = startle::parse(include_str!("../../../../skills/startle/skill.toml")).unwrap();
    let remark = startle::remark(&startle).unwrap();
    assert_eq!(remark.name, REMARK);
    assert_eq!(remark.trigger, Trigger::Remarked);
    assert_eq!(remark.keyframes, startle.keyframes);
    assert_eq!(remark.cooldown_us, 60 * SECOND);
    assert_eq!(remark.stroke_us, 1_500_000);
    assert_eq!(remark.states, [Voice::Resting, Voice::Listening]);
    assert!(!remark.quiet_only && remark.stop_when_concentrating);
    let mut thinking = remark.clone();
    thinking.states.push(Voice::Thinking);
    assert!(thinking.check().is_err());
    let mut eager = remark.clone();
    eager.cooldown_us = 30 * SECOND;
    assert!(eager.check().is_err());
    let mut late = remark.clone();
    late.stroke_us = 3 * SECOND;
    assert!(late.check().is_err());
    let mut moving = remark;
    moving.keyframes[1].offsets.base_yaw = 0.05;
    assert!(moving.check().is_err());
}

/// A catalogue file's own glance at a remark is named and dropped, as its
/// sudden clip is: the remark glance is the startle skill's.
#[test]
fn a_catalogues_own_remark_glance_is_never_played() {
    let startle = startle::parse(include_str!("../../../../skills/startle/skill.toml")).unwrap();
    let mut written: serde_json::Value =
        serde_json::from_str(include_str!("../../rest-behaviours.json")).unwrap();
    let behaviours = written["behaviours"].as_array_mut().unwrap();
    let at = behaviours.len();
    behaviours.push(serde_json::to_value(startle::remark(&startle).unwrap()).unwrap());
    let (read, unread) = Catalogue::from_json_naming(&written.to_string()).unwrap();
    assert!(
        read.behaviours
            .iter()
            .all(|clip| clip.trigger != Trigger::Remarked)
    );
    assert_eq!(
        unread,
        [format!(
            "behaviours[{at}] (the glance at a remark is the startle skill's)"
        )]
    );
}
