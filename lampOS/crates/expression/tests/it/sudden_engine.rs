//! A glance at a sudden sound (`ears/sudden`): optional, head only, once.
use lamp_expression::{Behaviour, Catalogue, Engine, Input, Keyframe, Output, Trigger};
use lamp_msgs::{
    BodyOffsets, ClockStamp, JointState, Light, Room, RoomState, SuddenSound, Voice, VoiceEvent,
    VoiceProgress, VoiceTiming,
};

const CLOCK: &str = "sudden-test-boot";
const GLANCE: &str = "glance at a sound";

fn pose(head_pitch: f64, head_roll: f64) -> BodyOffsets {
    BodyOffsets {
        head_pitch,
        head_roll,
        ..BodyOffsets::ZERO
    }
}

fn glance() -> Behaviour {
    let frame = |at_us, offsets| Keyframe {
        at_us,
        offsets,
        brightness: 0.0,
    };
    Behaviour {
        name: GLANCE.into(),
        trigger: Trigger::Sudden,
        weight: 1.0,
        delay_us: 0,
        cooldown_us: 8_000_000,
        stroke_us: 200_000,
        repeat: false,
        hold: false,
        quiet_only: false,
        stop_when_concentrating: true,
        states: vec![Voice::Resting, Voice::Listening, Voice::Thinking],
        continue_in: vec![],
        keyframes: vec![
            frame(0, BodyOffsets::ZERO),
            frame(200_000, pose(-0.07, 0.0)),
            frame(800_000, pose(-0.07, 0.03)),
            frame(1_600_000, BodyOffsets::ZERO),
        ],
    }
}

fn catalogue(with_glance: bool) -> Catalogue {
    let mut value = Catalogue::from_json(include_str!("../../rest-behaviours.json")).unwrap();
    // This is an injected zero lag for planner tests, not a motor measurement.
    value.timing.body_lag_us = Some(0);
    if with_glance {
        value.behaviours.push(glance());
    }
    value.check().unwrap();
    value
}

struct Lamp {
    engine: Engine,
    sequence: u64,
    state: Voice,
}

impl Lamp {
    fn new(with_glance: bool, state: Voice) -> Self {
        let mut lamp = Self {
            engine: Engine::new(catalogue(with_glance), true, CLOCK).unwrap(),
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

    /// The voice's state, fresh joints and a quiet room, then a tick.
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
            value: RoomState { state: Room::Quiet },
        });
        self.engine.tick(at)
    }

    fn sound(&mut self, at: u64, short: bool) {
        let stamp = self.stamp(at);
        self.send(Input::Sudden {
            stamp,
            value: SuddenSound {
                peak_db: -18.0,
                floor_db: -52.0,
                loud_ms: if short { 60 } else { 300 },
                short,
                ago_ms: 300,
            },
        });
    }

    /// Ticks every 100 ms from `from` to `to`; whether the glance ran and
    /// the highest the head went (the most negative pitch).
    fn watch(&mut self, from: u64, to: u64) -> (bool, f64) {
        let (mut ran, mut lifted) = (false, 0.0f64);
        let mut at = from;
        while at <= to {
            let out = self.step(at);
            ran |= out.state.behaviours.iter().any(|name| name == GLANCE);
            if let Some(body) = out.body {
                lifted = lifted.min(body.offsets.head_pitch);
            }
            at += 100_000;
        }
        (ran, lifted)
    }
}

#[test]
fn a_short_sudden_sound_turns_the_head_up_once_and_it_comes_back() {
    let mut lamp = Lamp::new(true, Voice::Resting);
    lamp.step(1_000_000);
    lamp.sound(1_000_000, true);
    let (ran, lifted) = lamp.watch(1_000_000, 2_400_000);
    assert!(ran);
    assert!(lifted < -0.05, "{lifted}");
    // Back where it was once the clip is over.
    let (ran, lifted) = lamp.watch(2_800_000, 3_400_000);
    assert!(!ran && lifted.abs() < 1e-9, "{ran} {lifted}");
}

#[test]
fn a_second_sound_inside_the_cooldown_gets_nothing_and_one_after_it_a_glance() {
    let mut lamp = Lamp::new(true, Voice::Listening);
    lamp.sound(500_000, true);
    assert!(lamp.watch(500_000, 2_200_000).0);
    lamp.sound(4_000_000, true);
    assert!(!lamp.watch(4_000_000, 5_800_000).0);
    lamp.sound(9_000_000, true);
    assert!(lamp.watch(9_000_000, 10_800_000).0);
}

#[test]
fn a_sound_that_went_on_gets_no_glance() {
    let mut lamp = Lamp::new(true, Voice::Resting);
    lamp.sound(500_000, false);
    assert!(!lamp.watch(500_000, 2_200_000).0);
}

#[test]
fn a_sound_while_the_lamp_thinks_gets_a_glance_as_a_person_would() {
    let mut lamp = Lamp::new(true, Voice::Listening);
    lamp.step(100_000);
    lamp.state = Voice::Thinking;
    lamp.step(200_000);
    lamp.sound(300_000, true);
    assert!(lamp.watch(300_000, 1_900_000).0);
}

#[test]
fn without_the_behaviour_in_the_catalogue_nothing_changes() {
    let mut with = Lamp::new(false, Voice::Resting);
    with.sound(500_000, true);
    let (ran, lifted) = with.watch(500_000, 2_200_000);
    assert!(!ran && lifted.abs() < 1e-9);
}

#[test]
fn a_glance_that_moves_the_body_or_lasts_or_repeats_is_refused() {
    let mut moving = catalogue(false);
    let mut clip = glance();
    clip.keyframes[1].offsets.base_yaw = 0.05;
    moving.behaviours.push(clip);
    assert!(moving.check().is_err());
    let mut speaking = catalogue(false);
    let mut clip = glance();
    clip.states = vec![Voice::Speaking];
    speaking.behaviours.push(clip);
    assert!(speaking.check().is_err());
    let mut eager = catalogue(false);
    let mut clip = glance();
    clip.cooldown_us = 1_000_000;
    eager.behaviours.push(clip);
    assert!(eager.check().is_err());
}

#[test]
fn lamp_4ace_catalogue_is_valid_and_glances_at_a_sound() {
    let lamp = Catalogue::from_json(include_str!(
        "../../../../deploy/lamp-4ace/expression-rest-lamp-4ace.json"
    ))
    .unwrap();
    let glance = lamp
        .behaviours
        .iter()
        .find(|behaviour| behaviour.trigger == Trigger::Sudden)
        .expect("lamp-4ace glances at a sudden sound");
    assert!(glance.cooldown_us >= 30_000_000);
}

#[test]
fn a_glance_under_way_finishes_when_the_voice_turns_from_thinking_to_listening() {
    let mut lamp = Lamp::new(true, Voice::Listening);
    lamp.step(100_000);
    lamp.state = Voice::Thinking;
    lamp.step(200_000);
    lamp.sound(300_000, true);
    assert!(lamp.watch(300_000, 600_000).0);
    // The turn ends: back to listening, as on lamp-4ace when the glance was cut at 0.48 s.
    lamp.state = Voice::Listening;
    let (ran, lifted) = lamp.watch(700_000, 1_400_000);
    assert!(ran && lifted < -0.05, "{ran} {lifted}");
}

#[test]
fn someone_beginning_to_speak_does_not_cut_a_glance_short() {
    let mut lamp = Lamp::new(true, Voice::Listening);
    lamp.sound(300_000, true);
    assert!(lamp.watch(300_000, 500_000).0);
    let stamp = lamp.stamp(550_000);
    lamp.send(Input::Activity(lamp_msgs::PersonActivity {
        stamp,
        session_id: "voice-one".into(),
        turn: 1,
        request: 11,
        speaking: true,
    }));
    let (ran, lifted) = lamp.watch(600_000, 1_300_000);
    assert!(ran && lifted < -0.05, "{ran} {lifted}");
}
