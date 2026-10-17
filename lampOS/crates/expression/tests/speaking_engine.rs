//! The lamp's own speech in its head (#933), through the engine with the
//! alive catalogue: an answer's sound, as the voice's playout says it, with
//! stressed words and pauses between sentences.
use lamp_expression::{Catalogue, Engine, Input, Output};
use lamp_msgs::{
    ClockStamp, JointState, Light, Room, RoomState, Voice, VoiceEvent, VoicePlayout, VoiceProgress,
    VoiceSound, VoiceTiming,
};

const CLOCK: &str = "speaking-test-boot";
const TICK: u64 = 20_000;

fn catalogue(speaking: bool) -> Catalogue {
    let mut catalogue = Catalogue::from_json(include_str!("../alive-behaviours.json")).unwrap();
    // An injected zero lag for planner tests, not a motor measurement.
    catalogue.timing.body_lag_us = Some(0);
    if !speaking {
        catalogue.speaking = None;
    }
    catalogue
}

struct Lamp {
    engine: Engine,
    sequence: u64,
    room: Room,
}

impl Lamp {
    fn new(catalogue: Catalogue, room: Room) -> Self {
        let mut lamp = Self {
            engine: Engine::new(catalogue, true, CLOCK).unwrap(),
            sequence: 0,
            room,
        };
        let stamp = lamp.stamp(0);
        lamp.send(
            0,
            Input::Light {
                stamp,
                value: Light::new(true, "warm", 2).unwrap(),
            },
        );
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

    fn send(&mut self, at: u64, input: Input) {
        self.engine.ingest(at, input).unwrap();
    }

    /// What the voice and the other parts say every 100 ms.
    fn parts(&mut self, at: u64, state: Voice) {
        self.parts_of(at, 1, state, VoiceEvent::State);
    }

    /// As [`Lamp::parts`], for turn `turn` and with `event`.
    fn parts_of(&mut self, at: u64, turn: u64, state: Voice, event: VoiceEvent) {
        let timing = VoiceTiming {
            stamp: self.stamp(at),
            session_id: "voice-one".into(),
            turn,
            request: turn * 10 + 1,
            state,
            accepting_input: true,
            event,
            first_audio_us: None,
        };
        self.send(
            at,
            Input::Progress(VoiceProgress {
                timing,
                audio_generation: 1,
                held: false,
            }),
        );
        let stamp = self.stamp(at);
        self.send(
            at,
            Input::Joints {
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
            },
        );
        let stamp = self.stamp(at);
        let room = self.room;
        self.send(
            at,
            Input::Room {
                stamp,
                value: RoomState { state: room },
            },
        );
    }

    /// One 20 ms piece of the answer's sound, said as it plays.
    fn piece(&mut self, at: u64, level: f32) {
        let playout = VoicePlayout {
            stamp: self.stamp(at),
            session_id: "voice-one".into(),
            turn: 1,
            request: 11,
            start_us: at,
            end_us: at + TICK,
            level,
        };
        self.send(
            at,
            Input::Sound(VoiceSound {
                playout,
                audio_generation: 1,
            }),
        );
    }
}

/// An answer's sound: syllables at a speaking level, a stressed word about
/// every 1.1 s, and a half-second pause every 3 s.
fn level(at: u64) -> f32 {
    let into_sentence = at % 3_000_000;
    if into_sentence >= 2_500_000 {
        return 0.0;
    }
    if into_sentence % 1_100_000 < 140_000 {
        return 0.8;
    }
    if (at / TICK) % 10 < 6 { 0.35 } else { 0.1 }
}

/// An answer of `seconds` from 1 s, then rest: every tick's output.
fn answer(lamp: &mut Lamp, seconds: u64) -> Vec<(u64, Output)> {
    let mut outputs = Vec::new();
    let mut at = 0;
    let end = 1_000_000 + seconds * 1_000_000;
    while at < end + 1_500_000 {
        let speaking = (1_000_000..end).contains(&at);
        let state = if speaking {
            Voice::Speaking
        } else {
            Voice::Resting
        };
        if at % 100_000 == 0 {
            lamp.parts(at, state);
        }
        if speaking {
            lamp.piece(at, level(at - 1_000_000));
        }
        outputs.push((at, lamp.engine.tick(at)));
        at += TICK;
    }
    outputs
}

fn named(output: &Output, name: &str) -> bool {
    output
        .state
        .behaviours
        .iter()
        .any(|planned| planned == name)
}

fn starts(outputs: &[(u64, Output)], name: &str) -> usize {
    outputs
        .windows(2)
        .filter(|w| !named(&w[0].1, name) && named(&w[1].1, name))
        .count()
}

#[test]
fn while_it_speaks_it_nods_on_stressed_words_sways_its_base_between_sentences_and_settles() {
    for room in [Room::Quiet, Room::Busy] {
        let mut lamp = Lamp::new(catalogue(true), room);
        let outputs = answer(&mut lamp, 8);
        let during: Vec<_> = outputs
            .iter()
            .filter(|(at, _)| (1_000_000..9_000_000).contains(at))
            .filter_map(|(_, output)| output.body.as_ref().map(|body| body.offsets))
            .collect();
        assert!(!during.is_empty());
        let pitch =
            |f: fn(f64, f64) -> f64, start: f64| during.iter().map(|o| o.head_pitch).fold(start, f);
        // Visible: about two degrees of nod over the speaking pose, and the
        // base turned out after a pause (#955: about 52 counts at most,
        // inside the body's 70; which pauses sway, and to which side, the
        // unit tests check).
        assert!(
            pitch(f64::max, f64::MIN) - pitch(f64::min, f64::MAX) > 0.02,
            "{room:?}"
        );
        assert!(during.iter().any(|o| o.base_yaw.abs() > 0.055), "{room:?}");
        // Within the catalogue's limits.
        let limits = catalogue(true).limits;
        assert!(
            during
                .iter()
                .all(|o| o.head_pitch.abs() <= limits.head_pitch
                    && o.head_roll.abs() <= limits.head_roll
                    && o.base_yaw.abs() <= limits.base_yaw)
        );
        // What speaking adds, against the same answer without it, never
        // jumps between ticks. (The old played-sound clip follows each 20 ms
        // piece's level and is left as it was.)
        let mut plain = Lamp::new(catalogue(false), room);
        let before: Vec<_> = answer(&mut plain, 8)
            .into_iter()
            .filter(|(at, _)| (1_000_000..9_000_000).contains(at))
            .filter_map(|(_, output)| output.body.map(|body| body.offsets))
            .collect();
        assert_eq!(before.len(), during.len());
        let added: Vec<_> = during
            .iter()
            .zip(&before)
            .map(|(with, without)| {
                (
                    with.head_pitch - without.head_pitch,
                    with.base_yaw - without.base_yaw,
                )
            })
            .collect();
        let (pitch_step, yaw_step) = added.windows(2).fold((0.0_f64, 0.0_f64), |(p, r), w| {
            (
                p.max((w[1].0 - w[0].0).abs()),
                r.max((w[1].1 - w[0].1).abs()),
            )
        });
        assert!(
            pitch_step < 0.007 && yaw_step < 0.007,
            "{room:?} {pitch_step} {yaw_step}"
        );
        // A nod on most stressed words, never two within the gap.
        let nods = starts(&outputs, "nodding on a stressed word");
        assert!((5..=8).contains(&nods), "{room:?} {nods}");
        assert!(
            starts(&outputs, "swaying between sentences") >= 1,
            "{room:?}"
        );
        // And as it finishes, a settle.
        assert!(
            outputs
                .iter()
                .filter(|(at, _)| *at >= 9_000_000)
                .any(|(_, output)| named(output, "settling as it finishes")),
            "{room:?}"
        );
    }
}

#[test]
fn a_catalogue_without_speaking_plans_its_speech_as_before() {
    let mut lamp = Lamp::new(catalogue(false), Room::Quiet);
    let outputs = answer(&mut lamp, 8);
    for name in [
        "nodding on a stressed word",
        "swaying between sentences",
        "settling as it finishes",
    ] {
        assert_eq!(starts(&outputs, name), 0, "{name}");
    }
    let rolls: Vec<_> = outputs
        .iter()
        .filter_map(|(_, output)| output.body.as_ref().map(|body| body.offsets.head_roll))
        .collect();
    // The old clip's roll is at most 0.009 rad, scaled by the level.
    assert!(rolls.iter().all(|roll| roll.abs() <= 0.0091), "{rolls:?}");
    // And the base does not turn for it.
    assert!(
        outputs
            .iter()
            .filter_map(|(_, output)| output.body.as_ref())
            .all(|body| body.offsets.base_yaw == 0.0)
    );
}

/// #955's own `speaking` section, with the head roll it asked: read, its
/// roll ignored, no sway, and the rest of its speech as before.
#[test]
fn a_catalogue_with_the_old_head_roll_still_loads_and_does_not_sway() {
    let text = include_str!("../alive-behaviours.json")
        .replace("\"sway_yaw\": 0.08", "\"sway_roll\": 0.025")
        .replace("\"sway_us\": 800000", "\"sway_us\": 600000");
    let mut old = Catalogue::from_json(&text).unwrap();
    old.timing.body_lag_us = Some(0);
    let speaking = old.speaking.unwrap();
    assert_eq!((speaking.sway_roll, speaking.sway_yaw), (Some(0.025), 0.0));
    let mut lamp = Lamp::new(old, Room::Quiet);
    let outputs = answer(&mut lamp, 8);
    assert_eq!(starts(&outputs, "swaying between sentences"), 0);
    assert!(starts(&outputs, "nodding on a stressed word") >= 5);
    assert!(
        outputs
            .iter()
            .filter_map(|(_, output)| output.body.as_ref())
            .all(|body| body.offsets.base_yaw == 0.0 && body.offsets.head_roll.abs() <= 0.0091)
    );
}

/// Three seconds of the answer from 1 s, ticked; then `after` at 4 s, and
/// the next second as turn 2 listening, or turn 1 at rest without
/// `next_turn`: those ticks' outputs.
fn fenced(speaking: bool, after: VoiceEvent, next_turn: bool) -> Vec<(u64, Output)> {
    let mut lamp = Lamp::new(catalogue(speaking), Room::Quiet);
    let mut at = 0;
    while at < 4_000_000 {
        let talking = at >= 1_000_000;
        if at % 100_000 == 0 {
            lamp.parts(
                at,
                if talking {
                    Voice::Speaking
                } else {
                    Voice::Resting
                },
            );
        }
        if talking {
            lamp.piece(at, level(at - 1_000_000));
        }
        lamp.engine.tick(at);
        at += TICK;
    }
    // A terminal event ends turn 1 as Resting; with none, turn 2 simply
    // begins over it.
    if after != VoiceEvent::State {
        lamp.parts_of(at, 1, Voice::Resting, after);
    }
    let mut outputs = Vec::new();
    while at < 5_000_000 {
        if at % 100_000 == 0 {
            if next_turn {
                lamp.parts_of(at, 2, Voice::Listening, VoiceEvent::State);
            } else {
                lamp.parts_of(at, 1, Voice::Resting, VoiceEvent::State);
            }
        }
        outputs.push((at, lamp.engine.tick(at)));
        at += TICK;
    }
    outputs
}

/// #955 review: what moved the head with one answer ends at the fences that
/// end it. A new turn, or a cancelled answer, carries no nod, sway or settle
/// into the next turn; only an answer that finished is settled after, and a
/// new turn at once (someone speaking) cuts that settle short too.
#[test]
fn a_new_turn_or_a_cancelled_answer_carries_none_of_the_old_answers_motion() {
    let speaking_names = [
        "nodding on a stressed word",
        "swaying between sentences",
        "settling as it finishes",
    ];
    for after in [VoiceEvent::State, VoiceEvent::Cancelled] {
        let with = fenced(true, after, true);
        let without = fenced(false, after, true);
        for ((at, output), (_, plain)) in with.iter().zip(&without) {
            assert!(
                speaking_names.iter().all(|name| !named(output, name)),
                "{after:?} at {at}: {:?}",
                output.state.behaviours
            );
            assert_eq!(
                output.body.as_ref().map(|body| body.offsets),
                plain.body.as_ref().map(|body| body.offsets),
                "{after:?} at {at}"
            );
        }
    }
    // A finished one, with nobody speaking at once, is settled after.
    let finished = fenced(true, VoiceEvent::Finished, false);
    assert!(
        finished
            .iter()
            .any(|(_, output)| named(output, "settling as it finishes"))
    );
}
