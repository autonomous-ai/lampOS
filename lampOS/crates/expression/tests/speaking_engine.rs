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
        self.piece_from(at, at, level);
    }

    /// One 20 ms piece that plays from `start`, said at `at`.
    fn piece_from(&mut self, at: u64, start: u64, level: f32) {
        let playout = VoicePlayout {
            stamp: self.stamp(at),
            session_id: "voice-one".into(),
            turn: 1,
            request: 11,
            start_us: start,
            end_us: start + TICK,
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
    answer_at(lamp, seconds, 1.0)
}

/// As [`answer`], from a voice whose sound is `gain` times this one's.
fn answer_at(lamp: &mut Lamp, seconds: u64, gain: f32) -> Vec<(u64, Output)> {
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
            lamp.piece(at, level(at - 1_000_000) * gain);
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
        // A nod on a stressed word in each of its three sentences, never
        // two in one, never two within 4 s (the first builder on #2030).
        let nods = starts(&outputs, "nodding on a stressed word");
        assert!((2..=3).contains(&nods), "{room:?} {nods}");
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

/// The first builder, 19 Oct 21:34 on #1722: the bars were tuned on Live's
/// sound, and the second route's ElevenLabs may sit lower. The same answer
/// from a voice a third as loud, or a fifth louder (the most a 0..1 level
/// allows its stressed words), nods on its stressed words and sways between
/// its sentences as Live's does.
#[test]
fn a_quiet_voice_and_a_loud_one_nod_and_sway_alike() {
    for gain in [0.33, 1.0, 1.2] {
        let mut lamp = Lamp::new(catalogue(true), Room::Quiet);
        let outputs = answer_at(&mut lamp, 8, gain);
        let nods = starts(&outputs, "nodding on a stressed word");
        assert!((2..=3).contains(&nods), "{gain} {nods}");
        let sways = starts(&outputs, "swaying between sentences");
        assert!(sways >= 1, "{gain} {sways}");
        assert!(
            outputs
                .iter()
                .filter(|(at, _)| *at >= 9_000_000)
                .any(|(_, output)| named(output, "settling as it finishes")),
            "{gain}"
        );
    }
}

/// Seventh on #1722, 22:25: the voice's meter stops at 1.0, and a voice
/// can fill it (the second route's coordinator test read 0.8 to 1.0, a mean
/// of 0.96). The stress bar stops under the meter's top (0.9), so it is
/// never out of reach. A voice at 1.0 through every sentence has no word
/// that stands out of its sentence, so it nods at most once a sentence,
/// never on every piece, and still sways between its sentences.
#[test]
fn a_voice_that_fills_the_meter_nods_at_most_once_a_sentence_and_still_sways() {
    let mut lamp = Lamp::new(catalogue(true), Room::Quiet);
    let mut outputs = Vec::new();
    let mut at = 0;
    while at < 10_500_000 {
        let speaking = (1_000_000..9_000_000).contains(&at);
        if at % 100_000 == 0 {
            lamp.parts(
                at,
                if speaking {
                    Voice::Speaking
                } else {
                    Voice::Resting
                },
            );
        }
        if speaking {
            // The meter full through every sentence, silent in the pauses.
            let level = if level(at - 1_000_000) == 0.0 {
                0.0
            } else {
                1.0
            };
            lamp.piece(at, level);
        }
        outputs.push((at, lamp.engine.tick(at)));
        at += TICK;
    }
    let nods = starts(&outputs, "nodding on a stressed word");
    assert!(nods <= 3, "{nods} nods");
    assert!(starts(&outputs, "swaying between sentences") >= 1);
}

/// Relative bars still keep silence silent: an answer that is all hiss
/// stresses no word and pauses no sentence.
#[test]
fn a_hiss_is_no_speech_however_quiet_the_voice() {
    let mut lamp = Lamp::new(catalogue(true), Room::Quiet);
    let outputs = answer_at(&mut lamp, 8, 0.01);
    for name in ["nodding on a stressed word", "swaying between sentences"] {
        assert_eq!(starts(&outputs, name), 0, "{name}");
    }
}

/// #1759, 20 Oct: the voice says its sound 200 ms ahead (`FORETELL_US`),
/// and the alive catalogue's body lag is 300 ms (as the product's and
/// lamp-4ace's). With no piece at the lag, the speaking motion took level
/// 0: on lamp-4ace a story nodded and swayed never, and only settled. It
/// now takes the farthest piece said, so the same answer nods and sways.
#[test]
fn an_answer_said_200_ms_ahead_nods_and_sways_at_the_catalogues_own_lag() {
    let mut catalogue = Catalogue::from_json(include_str!("../alive-behaviours.json")).unwrap();
    assert_eq!(catalogue.timing.body_lag_us, Some(300_000));
    catalogue.timing.body_lag_us = Some(300_000);
    let mut lamp = Lamp::new(catalogue, Room::Quiet);
    let mut outputs = Vec::new();
    let mut at = 0;
    let end = 9_000_000;
    while at < end + 1_500_000 {
        let speaking = (1_000_000..end).contains(&at);
        if at % 100_000 == 0 {
            lamp.parts(
                at,
                if speaking {
                    Voice::Speaking
                } else {
                    Voice::Resting
                },
            );
        }
        // Each tick of the turn, the piece that plays 200 ms on is said.
        let start = at + 200_000;
        if speaking && start < end {
            lamp.piece_from(at, start, level(start - 1_000_000));
        }
        outputs.push((at, lamp.engine.tick(at)));
        at += TICK;
    }
    let nods = starts(&outputs, "nodding on a stressed word");
    assert!((2..=3).contains(&nods), "{nods} nods");
    assert!(starts(&outputs, "swaying between sentences") >= 1);
    assert!(
        outputs
            .iter()
            .any(|(_, output)| named(output, "settling as it finishes"))
    );
}

/// The first builder on #2030, 20 Oct 03:22: a 150 s story in the second
/// route's ElevenLabs voice on lamp-4ace asked for 75 nods, one every 2 s.
/// Its spread, from `sound-shape.sh` (#1759, 01:52): p10 0.46, p25 0.67,
/// p50 0.80, p75 0.88, p90 0.94, a piece at 1.0 now and then, and 12 pauses
/// of 350 ms or more, a fifth of its frames under 0.12 (the dips between
/// words). Words drawn from that spread, the same each run,
/// said 200 ms ahead at the catalogue's own lag: a handful of nods, never
/// two in a sentence, and its sways.
/// The story above, through the alive catalogue changed by `tune`: its
/// nods, sway starts and settles.
fn loud_story(tune: impl FnOnce(&mut Catalogue)) -> (usize, usize, usize) {
    let spread = [0.46, 0.55, 0.67, 0.74, 0.80, 0.84, 0.88, 0.91, 0.94, 1.0];
    let mut seed: u64 = 0x2030;
    let mut draw = move || {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        spread[(seed >> 33) as usize % spread.len()]
    };
    let mut catalogue = Catalogue::from_json(include_str!("../alive-behaviours.json")).unwrap();
    tune(&mut catalogue);
    let mut lamp = Lamp::new(catalogue, Room::Quiet);
    let (from, end) = (1_000_000, 151_000_000);
    let mut outputs = Vec::new();
    let mut at = 0;
    while at < end + 1_500_000 {
        let speaking = (from..end).contains(&at);
        if at % 100_000 == 0 {
            lamp.parts(
                at,
                if speaking {
                    Voice::Speaking
                } else {
                    Voice::Resting
                },
            );
        }
        let start = at + 200_000;
        if speaking && start < end {
            // Twelve sentences of 12 s, each ending in a 0.5 s pause; in
            // them words of 240 ms with 80 ms between, nearly silent (a
            // fifth of lamp-4ace's frames were under 0.12).
            let into = (start - from) % 12_500_000;
            let level = if into >= 12_000_000 {
                0.0
            } else if into % 320_000 >= 240_000 {
                0.05
            } else {
                draw()
            };
            lamp.piece_from(at, start, level);
        }
        outputs.push((at, lamp.engine.tick(at)));
        at += TICK;
    }
    (
        starts(&outputs, "nodding on a stressed word"),
        starts(&outputs, "swaying between sentences"),
        starts(&outputs, "settling as it finishes"),
    )
}

#[test]
fn a_loud_story_nods_a_handful_of_times_not_every_two_seconds() {
    let (nods, sways, _) = loud_story(|_| {});
    assert!((3..=13).contains(&nods), "{nods} nods in 150 s");
    // A sway holds the head over to the next pause, so the name stays on
    // across sentences: at least one.
    assert!(sways >= 1, "{sways} sways");
}

/// The arm hour compares two rates side by side (the first builder on
/// #2030, 03:32): the same build, its catalogue with no limit a sentence,
/// no step and no spacing, nods as before the limits, many times more.
#[test]
fn the_catalogue_can_turn_the_nod_limits_off_for_a_dense_arm() {
    let (dense, _, _) = loud_story(|catalogue| {
        let speaking = catalogue.speaking.as_mut().unwrap();
        speaking.sentence_nods = Some(0);
        speaking.stress_step = Some(0.0);
        speaking.nod_spacing_us = Some(0);
    });
    let (sparse, _, _) = loud_story(|_| {});
    assert!(dense >= 5 * sparse, "dense {dense}, sparse {sparse}");
}

/// The first builder on #2030, 03:40: no settle on lamp-4ace in either arm
/// of `cb4453618`. A loud story that ends as it should (Speaking, then
/// rest) settles once, with the nod limits and without them.
#[test]
fn a_storys_end_settles_once() {
    let (_, _, settles) = loud_story(|_| {});
    assert_eq!(settles, 1);
    let (_, _, dense) = loud_story(|catalogue| {
        let speaking = catalogue.speaking.as_mut().unwrap();
        speaking.sentence_nods = Some(0);
        speaking.stress_step = Some(0.0);
        speaking.nod_spacing_us = Some(0);
    });
    assert_eq!(dense, 1);
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
    // At most one nod a sentence (#2030): its three sentences nod 2 or 3
    // times.
    assert!(starts(&outputs, "nodding on a stressed word") >= 2);
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

/// The sway and the nods read the speaking motion's level, sampled at the
/// catalogue's body lag (lamp-4ace's measured 300 ms), not the playout clip.
/// Here each piece is said 400 ms before it plays, so one covers now + lag.
/// (The voice says only 200 ms ahead today; #2030 on #1712 gives the speaking
/// motion its own `speech_level` for that, and the sway follows it.)
#[test]
fn with_the_measured_body_lag_the_speaking_level_still_nods_and_sways() {
    const AHEAD: u64 = 400_000;
    let mut catalogue = catalogue(true);
    catalogue.timing.body_lag_us = Some(300_000);
    let mut lamp = Lamp::new(catalogue, Room::Quiet);
    let (start, end) = (1_000_000, 9_000_000);
    let mut outputs = Vec::new();
    let mut at = 0;
    while at < end + 1_500_000 {
        let speaking = (start..end).contains(&at);
        if at % 100_000 == 0 {
            let state = if speaking {
                Voice::Speaking
            } else {
                Voice::Resting
            };
            lamp.parts(at, state);
        }
        let plays = at + AHEAD;
        if speaking && plays < end {
            let playout = VoicePlayout {
                stamp: lamp.stamp(at),
                session_id: "voice-one".into(),
                turn: 1,
                request: 11,
                start_us: plays,
                end_us: plays + TICK,
                level: level(plays - start),
            };
            lamp.send(
                at,
                Input::Sound(VoiceSound {
                    playout,
                    audio_generation: 1,
                }),
            );
        }
        outputs.push((at, lamp.engine.tick(at)));
        at += TICK;
    }
    let nods = starts(&outputs, "nodding on a stressed word");
    let sways = starts(&outputs, "swaying between sentences");
    let settled = outputs
        .iter()
        .any(|(_, output)| named(output, "settling as it finishes"));
    assert!(
        // At most one nod a sentence (#2030): 2 or 3 in its three.
        settled && nods >= 2 && sways >= 1,
        "nods {nods} sways {sways} settled {settled}"
    );
}

/// #933: the product's catalogue carries the speaking section, the same as
/// the alive catalogue's, base sway included. Without it a lamp installed
/// from the product never nods, sways or settles while it speaks, whatever
/// Expression can do (it had none until 17 Oct).
#[test]
fn the_products_catalogue_nods_sways_and_settles_as_the_alive_one_does() {
    let product = Catalogue::from_json(include_str!(
        "../../../deploy/product/settings/expression.json"
    ))
    .unwrap();
    let alive = Catalogue::from_json(include_str!("../alive-behaviours.json")).unwrap();
    let speaking = product.speaking.expect("the product's catalogue speaks");
    assert_eq!(Some(speaking), alive.speaking);
    assert!(speaking.sway_yaw > 0.0, "the base sways between sentences");
    assert!(speaking.sway_roll.is_none());
}
