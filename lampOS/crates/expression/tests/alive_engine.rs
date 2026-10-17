//! The idle layer, sleep and waking (task 36, #714), through the engine,
//! with the catalogue offered for lamp-4ace (`alive-behaviours.json`).
use lamp_expression::{Catalogue, Engine, Input, LEAN, Output};
use lamp_msgs::{
    BodyOffsets, ClockStamp, ControlGesture, ControlIntent, ControlSource, Face, Faces, Gesture,
    JointState, Light, NameHeard, PersonActivity, Room, RoomState, ScheduleDue, ScheduleNotice,
    Speech, SuddenSound, Voice, VoiceEvent, VoiceProgress, VoiceTiming,
};
use std::f64::consts::PI;
use std::time::Instant;

const CLOCK: &str = "alive-test-boot";
const TICK: u64 = 20_000;
const SECOND: u64 = 1_000_000;
const MINUTE: u64 = 60 * SECOND;
/// Well into the boot clock's first hour, where the ring's phase is plain.
const START: u64 = 1_000 * SECOND;

fn catalogue() -> Catalogue {
    Catalogue::from_json(include_str!("../alive-behaviours.json")).unwrap()
}

/// The same, falling asleep after one minute of nobody, to reach sleep soon.
fn drowsy() -> Catalogue {
    let mut value = catalogue();
    value
        .alive
        .as_mut()
        .unwrap()
        .sleep
        .as_mut()
        .unwrap()
        .nobody_us = MINUTE;
    value.check().unwrap();
    value
}

fn speech_gated() -> Catalogue {
    let mut value = drowsy();
    value
        .alive
        .as_mut()
        .unwrap()
        .sleep
        .as_mut()
        .unwrap()
        .name_requires_speech = true;
    value.check().unwrap();
    value
}

fn sleeping_with(catalogue: Catalogue) -> Lamp {
    let mut lamp = Lamp::new(catalogue).at_night();
    assert!(lamp.sleep_within(2 * MINUTE));
    lamp
}

/// The ring's resting breath at `now_us` (`crates/voice/src/ring_face.rs`).
fn ring(now_us: u64) -> f64 {
    let seconds = (now_us % 3_600_000_000) as f64 / 1e6;
    0.5 - 0.5 * (2.0 * PI * seconds / 5.2).cos()
}

fn face(xmin: f32, ymin: f32, side: f32) -> Face {
    Face {
        speaking: None,
        id: "face-one".into(),
        xmin,
        ymin,
        xmax: xmin + side,
        ymax: ymin + side,
        confidence: 0.9,
        yaw: None,
        facing: None,
    }
}

struct Lamp {
    engine: Engine,
    sequence: u64,
    now: u64,
    state: Voice,
    session: &'static str,
    accepting_input: bool,
    turn: u64,
    started: u64,
    /// What the eyes report every 600 ms; `None` while they are silent.
    faces: Option<Vec<Face>>,
    /// The local minute of the day at `START`, told every minute.
    minute: Option<u16>,
    /// The goals the body says for its five joints at a time (none by
    /// default).
    goals: fn(u64) -> Vec<f64>,
}

impl Lamp {
    fn new(catalogue: Catalogue) -> Self {
        Self::at(catalogue, START)
    }

    /// Started at `now`, with the voice, the joints and the room said once.
    fn at(catalogue: Catalogue, now: u64) -> Self {
        let mut lamp = Self {
            engine: Engine::new(catalogue, true, CLOCK).unwrap(),
            sequence: 0,
            now,
            started: now,
            state: Voice::Resting,
            session: "voice-one",
            accepting_input: true,
            turn: 1,
            faces: Some(vec![]),
            minute: None,
            goals: |_| vec![],
        };
        let stamp = lamp.stamp();
        lamp.send(Input::Light {
            stamp,
            value: Light::new(true, "warm", 1).unwrap(),
        });
        lamp.say_parts();
        lamp
    }

    fn at_night(mut self) -> Self {
        self.minute = Some(23 * 60 + 30);
        self
    }

    fn stamp(&mut self) -> ClockStamp {
        self.sequence += 1;
        ClockStamp {
            clock_id: CLOCK.into(),
            at_us: self.now,
            sequence: self.sequence,
        }
    }

    fn send(&mut self, input: Input) {
        self.engine.ingest(self.now, input).unwrap();
    }

    /// A new turn of the voice in `state`, said at once.
    fn voice(&mut self, state: Voice) {
        self.state = state;
        self.turn += 1;
        self.say_voice();
    }

    fn say_voice(&mut self) {
        self.say_voice_event(VoiceEvent::State);
    }

    fn say_voice_event(&mut self, event: VoiceEvent) {
        let timing = VoiceTiming {
            stamp: self.stamp(),
            session_id: self.session.into(),
            turn: self.turn,
            request: self.turn * 10 + 1,
            state: self.state,
            accepting_input: self.accepting_input,
            event,
            first_audio_us: None,
        };
        self.send(Input::Progress(VoiceProgress {
            timing,
            audio_generation: 1,
            held: false,
        }));
    }

    /// One 20 ms tick, with what the lamp's parts say at their own pace.
    fn step(&mut self) -> Output {
        self.now += TICK;
        if self.now.is_multiple_of(100_000) {
            self.say_parts();
        }
        if self.now.is_multiple_of(600_000)
            && let Some(faces) = self.faces.clone()
        {
            let stamp = self.stamp();
            self.send(Input::Faces {
                stamp,
                value: Faces { faces },
            });
        }
        if self.now.is_multiple_of(MINUTE)
            && let Some(minute) = self.minute
        {
            let since = (self.now - self.started) / MINUTE;
            let stamp = self.stamp();
            self.send(Input::DayTime {
                stamp,
                minute: ((u64::from(minute) + since) % 1_440) as u16,
            });
        }
        self.engine.tick(self.now)
    }

    /// The voice, fresh joints and a quiet room, as said every 100 ms.
    fn say_parts(&mut self) {
        self.say_voice();
        let stamp = self.stamp();
        self.send(Input::Joints {
            stamp,
            value: JointState {
                name: ["base_yaw", "shoulder", "elbow", "head_roll", "head_pitch"]
                    .map(str::to_owned)
                    .to_vec(),
                position: vec![0.0; 5],
                commanded: (self.goals)(self.now),
                velocity: vec![],
                effort: vec![],
            },
        });
        let stamp = self.stamp();
        self.send(Input::Room {
            stamp,
            value: RoomState { state: Room::Quiet },
        });
    }

    fn run(&mut self, micros: u64, mut each: impl FnMut(&Output, u64)) {
        let until = self.now + micros;
        while self.now < until {
            let output = self.step();
            each(&output, self.now);
        }
    }

    fn last(&mut self, micros: u64) -> Output {
        let mut last = None;
        self.run(micros, |output, _| last = Some(output.clone()));
        last.unwrap()
    }

    fn name(&mut self) {
        let stamp = self.stamp();
        self.send(Input::Name {
            stamp,
            value: NameHeard {
                score: 0.97,
                ago_ms: 400,
            },
        });
    }

    fn name_input(&mut self, published_us: u64, ago_ms: u32) -> Input {
        let mut stamp = self.stamp();
        stamp.at_us = published_us;
        Input::Name {
            stamp,
            value: NameHeard {
                score: 0.97,
                ago_ms,
            },
        }
    }

    fn name_at(&mut self, published_us: u64, ago_ms: u32) {
        let input = self.name_input(published_us, ago_ms);
        self.send(input);
    }

    fn activity_at(&mut self, observed_us: u64, speaking: bool) -> PersonActivity {
        let mut stamp = self.stamp();
        stamp.at_us = observed_us;
        PersonActivity {
            stamp,
            session_id: self.session.into(),
            turn: self.turn,
            request: self.turn * 10 + 1,
            speaking,
        }
    }

    fn speech_at(&mut self, observed_us: u64) {
        let activity = self.activity_at(observed_us, true);
        self.send(Input::Activity(activity));
    }

    /// Advance source/arrival time exactly, retaining ordinary owner and joint
    /// heartbeats. No host sleep or freshness override is involved.
    fn stay_asleep_until(&mut self, until_us: u64) {
        assert!(until_us >= self.now);
        while self.now < until_us {
            self.now = (self.now + 100_000).min(until_us);
            self.say_parts();
            let output = self.engine.tick(self.now);
            assert!(named(&output, "asleep"), "{:?}", output.state);
            assert!(!named(&output, "waking"));
        }
    }

    fn touch(&mut self, source: ControlSource, gesture: Gesture, enabled: bool) {
        let stamp = self.stamp();
        let (intent, duration_us) = match gesture {
            Gesture::Pet => (ControlIntent::Pet, 600_000),
            Gesture::LongPress => (ControlIntent::SoftwareMute, 2_100_000),
            _ => (ControlIntent::Stop, 120_000),
        };
        let value = ControlGesture {
            stamp: stamp.clone(),
            session_id: "controls-one".into(),
            source,
            name: "base".into(),
            gesture,
            intent,
            duration_us,
            released_us: self.now - 10_000,
            enabled,
        };
        self.send(Input::Touch { stamp, value });
    }

    fn due(&mut self, id: u64) {
        let stamp = self.stamp();
        let value = ScheduleDue {
            stamp: stamp.clone(),
            store_id: "0123456789abcdef0123456789abcdef".into(),
            revision: id,
            enabled: true,
            ready: true,
            pending: 1,
            waiting_for_clock: 0,
            oldest: Some(ScheduleNotice {
                id,
                delivery: 1,
                due_unix_ms: 1_760_000_000_000,
                late_by_ms: None,
                late: false,
            }),
        };
        self.send(Input::Due { stamp, value });
    }

    fn sudden(&mut self) {
        let stamp = self.stamp();
        self.send(Input::Sudden {
            stamp,
            value: SuddenSound {
                peak_db: -18.0,
                floor_db: -52.0,
                loud_ms: 60,
                short: true,
                ago_ms: 20,
            },
        });
    }

    /// Run until it is asleep, at most `micros`.
    fn sleep_within(&mut self, micros: u64) -> bool {
        let until = self.now + micros;
        while self.now < until {
            if named(&self.step(), "asleep") {
                return true;
            }
        }
        false
    }
}

fn named(output: &Output, name: &str) -> bool {
    output
        .state
        .behaviours
        .iter()
        .any(|planned| planned == name)
}

fn asked(output: &Output) -> BodyOffsets {
    output.body.as_ref().expect("a body request").offsets
}

#[test]
fn a_catalogue_without_alive_asks_nothing_of_a_resting_lamp() {
    let mut without = catalogue();
    without.alive = None;
    let mut lamp = Lamp::new(without);
    lamp.run(3 * MINUTE, |output, _| {
        assert_eq!(asked(output), BodyOffsets::ZERO);
        for name in ["breathing", "drifting", "asleep", "waking"] {
            assert!(!named(output, name));
        }
    });
}

#[test]
fn at_rest_it_breathes_with_the_ring_and_only_lifts_its_head() {
    let mut lamp = Lamp::new(catalogue());
    lamp.run(SECOND, |_, _| {});
    // The first drift comes no sooner than 30 s after the layer came in.
    let mut lowest: f64 = 0.0;
    lamp.run(20 * SECOND, |output, now| {
        let offsets = asked(output);
        assert!(
            (offsets.head_pitch - -0.03 * ring(now)).abs() < 1e-9,
            "{offsets:?} at {now}"
        );
        assert_eq!(offsets.head_roll, 0.0);
        assert_eq!(offsets.base_yaw, 0.0);
        assert!(named(output, "breathing"));
        lowest = lowest.min(offsets.head_pitch);
    });
    // A positive pitch dips the head; a resting lamp's head only lifts.
    assert!((lowest - -0.03).abs() < 1e-4, "{lowest}");
}

#[test]
fn the_layer_comes_in_and_goes_out_within_its_blend() {
    let mut lamp = Lamp::new(catalogue());
    let first = lamp.step();
    assert_eq!(asked(&first).head_pitch, 0.0, "it comes in from nothing");
    let came_in = lamp.now;
    lamp.run(SECOND, |output, now| {
        if now >= came_in + 350_000 {
            let wanted = -0.03 * ring(now);
            assert!((asked(output).head_pitch - wanted).abs() < 1e-9);
        }
    });
    // Someone speaks to it: the layer is gone within 350 ms, and what is
    // left is exactly the listening posture.
    lamp.voice(Voice::Listening);
    let left = lamp.now;
    let listening = catalogue().postures.listening;
    lamp.run(SECOND, |output, now| {
        if now >= left + 350_000 {
            assert_eq!(asked(output), listening);
            assert!(!named(output, "breathing"));
        }
    });
}

#[test]
fn a_drift_comes_now_and_then_within_its_bounds_and_never_as_a_twitch() {
    let mut lamp = Lamp::new(catalogue());
    // Past the layer's coming in, which is quicker by design (0.35 s).
    lamp.run(SECOND, |_, _| {});
    let mut starts = Vec::new();
    let mut drifting = false;
    let mut last: Option<BodyOffsets> = None;
    let mut largest = [0.0_f64; 2];
    lamp.run(15 * MINUTE, |output, now| {
        let offsets = asked(output);
        // The breath lifts up to 0.03 and a drift up to 0.035; a drift
        // tilts up to 0.02 either way.
        assert!(
            (-0.065 - 1e-9..=0.0).contains(&offsets.head_pitch),
            "{offsets:?}"
        );
        assert!(offsets.head_roll.abs() <= 0.02 + 1e-9, "{offsets:?}");
        if let Some(last) = last {
            largest[0] = largest[0].max((offsets.head_pitch - last.head_pitch).abs());
            largest[1] = largest[1].max((offsets.head_roll - last.head_roll).abs());
        }
        last = Some(offsets);
        let now_drifting = named(output, "drifting");
        if now_drifting && !drifting {
            starts.push(now);
        }
        drifting = now_drifting;
    });
    // From one start to the next: the drift's 3 s and then 30 to 90 s.
    let gaps: Vec<u64> = starts.windows(2).map(|pair| pair[1] - pair[0]).collect();
    assert!(starts.len() >= 9, "{starts:?}");
    for gap in &gaps {
        assert!(
            (33 * SECOND - TICK..=93 * SECOND + TICK).contains(gap),
            "{gaps:?}"
        );
    }
    // The breath's fastest is pi * 0.03 / 5.2 = 0.018 rad/s and a drift's
    // 1.5 * 0.035 / 3 = 0.0175 rad/s of pitch and 0.02 of roll: under
    // 0.75 and 0.4 milliradians a tick. No leap anywhere.
    assert!(largest[0] <= 0.00075, "{largest:?}");
    assert!(largest[1] <= 0.00041, "{largest:?}");
}

#[test]
fn where_the_ring_jumps_on_the_hour_the_head_does_not() {
    let mut lamp = Lamp::at(catalogue(), 3_590 * SECOND);
    lamp.run(2 * SECOND, |_, _| {});
    let mut last: Option<f64> = None;
    let mut largest: f64 = 0.0;
    lamp.run(20 * SECOND, |output, now| {
        let pitch = asked(output).head_pitch;
        if let Some(last) = last {
            largest = largest.max((pitch - last).abs());
        }
        last = Some(pitch);
        // Some seconds after the hour it is with the ring again.
        if now >= 3_605 * SECOND {
            assert!((pitch - -0.03 * ring(now)).abs() < 1e-9, "{pitch} at {now}");
        }
    });
    assert!(largest <= 0.00075, "{largest}");
}

#[test]
fn someone_working_close_and_still_under_it_has_a_still_lamp() {
    let mut lamp = Lamp::new(catalogue());
    lamp.run(2 * SECOND, |_, _| {});
    // A face close (0.09 of the picture) and still.
    lamp.faces = Some(vec![face(0.35, 0.3, 0.3)]);
    lamp.run(5 * SECOND + 600_000, |_, _| {});
    let mut still = 0;
    lamp.run(5 * SECOND, |output, _| {
        if named(output, "still for work") {
            still += 1;
        }
    });
    let output = lamp.last(TICK);
    assert_eq!(asked(&output), BodyOffsets::ZERO);
    assert!(!named(&output, "breathing"));
    assert!(still > 200, "{still}");
    // Bent over the work, no face at all: still still for 30 s, then the
    // breath comes back.
    lamp.faces = Some(vec![]);
    lamp.run(28 * SECOND, |output, _| {
        assert_eq!(asked(output), BodyOffsets::ZERO);
    });
    let output = lamp.last(5 * SECOND);
    assert!(named(&output, "breathing"));
}

#[test]
fn a_close_face_that_moves_is_not_someone_working() {
    let mut lamp = Lamp::new(catalogue());
    let mut x = 0.1;
    for _ in 0..30 {
        lamp.faces = Some(vec![face(x, 0.3, 0.3)]);
        x = if x > 0.5 { 0.1 } else { x + 0.1 };
        lamp.run(SECOND, |output, _| {
            assert!(!named(output, "still for work"));
        });
    }
    assert!(named(&lamp.last(TICK), "breathing"));
}

#[test]
fn late_at_night_with_nobody_there_it_falls_asleep_into_a_slow_breath() {
    let mut lamp = Lamp::new(catalogue()).at_night();
    lamp.run(14 * MINUTE, |output, _| {
        assert!(!named(output, "asleep") && !named(output, "falling asleep"));
    });
    let mut falling = 0;
    let mut asleep_at = None;
    // Nodding off, the head goes no faster than the pose and the fading
    // breath and drift together allow: under a milliradian a tick.
    let mut last: Option<BodyOffsets> = None;
    let mut largest: f64 = 0.0;
    lamp.run(2 * MINUTE, |output, now| {
        let offsets = asked(output);
        if let Some(last) = last {
            for (now, then) in offsets.values().iter().zip(last.values()) {
                largest = largest.max((now - then).abs());
            }
        }
        last = Some(offsets);
        if named(output, "falling asleep") {
            falling += 1;
        }
        if named(output, "asleep") {
            asleep_at.get_or_insert(now);
        }
    });
    // 15 minutes of nobody, then 4 s into the pose.
    let asleep_at = asleep_at.expect("asleep");
    assert!(largest <= 0.001, "{largest}");
    assert!((15 * MINUTE..15 * MINUTE + 5 * SECOND).contains(&(asleep_at - START)));
    assert_eq!(falling, 200, "four seconds of 20 ms ticks");
    // Asleep: the head a little bowed, lifting only with a slow breath,
    // nothing else.
    lamp.run(MINUTE, |output, _| {
        let offsets = asked(output);
        assert!((0.008 - 1e-9..=0.023 + 1e-9).contains(&offsets.head_pitch));
        assert_eq!(offsets.head_roll, 0.0);
        assert!(named(output, "asleep"));
        assert!(!named(output, "breathing"));
    });
}

#[test]
fn asleep_a_bang_or_talk_in_the_room_does_not_wake_it() {
    let mut lamp = Lamp::new(drowsy()).at_night();
    assert!(lamp.sleep_within(2 * MINUTE));
    lamp.run(5 * SECOND, |_, _| {});
    lamp.sudden();
    lamp.run(2 * SECOND, |output, _| {
        assert!(named(output, "asleep"));
        assert!(!named(output, "glance at a sound"));
        assert!(asked(output).head_pitch >= 0.008 - 1e-9);
    });
    // Someone talks, not to it: the voice listens, the lamp sleeps on and
    // Expression asks for no listening posture.
    lamp.voice(Voice::Listening);
    lamp.run(3 * SECOND, |output, _| {
        assert!(named(output, "asleep"));
        assert!(asked(output).head_pitch >= 0.008 - 1e-9);
    });
    lamp.voice(Voice::Resting);
    assert!(named(&lamp.last(SECOND), "asleep"));
}

#[test]
fn its_name_wakes_it_with_a_stretch_and_no_leap() {
    let mut lamp = Lamp::new(drowsy()).at_night();
    assert!(lamp.sleep_within(2 * MINUTE));
    lamp.run(10 * SECOND, |_, _| {});
    lamp.name();
    lamp.voice(Voice::Listening);
    let woken = lamp.now;
    let mut highest: f64 = 0.0;
    let mut last: Option<BodyOffsets> = None;
    let mut largest: f64 = 0.0;
    let mut waking = 0;
    lamp.run(4 * SECOND, |output, _| {
        let offsets = asked(output);
        if named(output, "waking") {
            waking += 1;
        }
        highest = highest.min(offsets.head_pitch);
        if let Some(last) = last {
            for (now, then) in offsets.values().iter().zip(last.values()) {
                largest = largest.max((now - then).abs());
            }
        }
        last = Some(offsets);
    });
    // The stretch: up to 0.065 radians of lift at energy 0.5, over 2.7 s.
    // From the tick it is woken to the one 2.7 s later, both counted.
    assert_eq!(waking, 136, "2.7 s of 20 ms ticks and the first");
    assert!(highest < -0.05, "{highest}");
    // A stretch at most 0.35 rad/s, the bow and the posture crossed over:
    // under 0.01 radians a tick.
    assert!(largest < 0.01, "{largest}");
    // Awake and listening: the listening posture exactly.
    let output = lamp.last(TICK);
    assert!(!named(&output, "asleep") && !named(&output, "waking"));
    assert_eq!(asked(&output), catalogue().postures.listening);
    assert!(lamp.now - woken > 2_700_000);
}

#[test]
fn a_tap_a_stroke_a_reminder_or_its_own_answer_wake_it() {
    let wakes = |act: &dyn Fn(&mut Lamp)| {
        let mut lamp = Lamp::new(drowsy()).at_night();
        assert!(lamp.sleep_within(2 * MINUTE));
        act(&mut lamp);
        named(&lamp.last(2 * TICK), "waking")
    };
    assert!(wakes(&|lamp| lamp.touch(
        ControlSource::Base,
        Gesture::Tap,
        true
    )));
    assert!(wakes(&|lamp| lamp.touch(
        ControlSource::Headpad,
        Gesture::Pet,
        true
    )));
    assert!(wakes(&|lamp| lamp.due(7)));
    assert!(wakes(&|lamp| lamp.voice(Voice::Speaking)));
    // A held base is the mute today and Sleep once #711 says so; a touch
    // read in shadow is no touch at all.
    assert!(!wakes(&|lamp| lamp.touch(
        ControlSource::Base,
        Gesture::LongPress,
        true
    )));
    assert!(!wakes(&|lamp| lamp.touch(
        ControlSource::Base,
        Gesture::Tap,
        false
    )));
}

#[test]
fn the_same_reminder_wakes_it_once() {
    let mut lamp = Lamp::new(drowsy()).at_night();
    assert!(lamp.sleep_within(2 * MINUTE));
    lamp.due(7);
    assert!(named(&lamp.last(2 * TICK), "waking"));
    assert!(lamp.sleep_within(3 * MINUTE));
    // Still pending, said again every second: it sleeps on.
    for _ in 0..5 {
        lamp.due(7);
        assert!(named(&lamp.last(SECOND), "asleep"));
    }
    lamp.due(8);
    assert!(named(&lamp.last(2 * TICK), "waking"));
}

#[test]
fn a_face_at_night_lets_it_sleep_and_the_first_face_of_the_morning_wakes_it() {
    let mut lamp = Lamp::new(drowsy());
    // 05:57: asleep a minute later, still night.
    lamp.minute = Some(5 * 60 + 57);
    assert!(lamp.sleep_within(2 * MINUTE));
    lamp.faces = Some(vec![face(0.1, 0.1, 0.1)]);
    lamp.run(5 * SECOND, |output, _| assert!(named(output, "asleep")));
    // Nobody again, past 06:00 (told on the minute), then the first person
    // of the day.
    lamp.faces = Some(vec![]);
    lamp.run(3 * MINUTE, |output, _| assert!(named(output, "asleep")));
    lamp.faces = Some(vec![face(0.1, 0.1, 0.1)]);
    assert!(named(&lamp.last(SECOND), "waking"));
}

#[test]
fn it_does_not_sleep_by_day_without_eyes_or_without_the_time() {
    let stays_awake = |lamp: &mut Lamp| !lamp.sleep_within(5 * MINUTE);
    // By day.
    let mut day = Lamp::new(drowsy());
    day.minute = Some(10 * 60);
    assert!(stays_awake(&mut day));
    // At night, but the eyes are silent: nobody cannot be said to be gone.
    let mut blind = Lamp::new(drowsy()).at_night();
    blind.faces = None;
    assert!(stays_awake(&mut blind));
    // At night with nobody, but no time of day was ever told.
    let mut timeless = Lamp::new(drowsy());
    assert!(stays_awake(&mut timeless));
    // At night, with someone passing every ten minutes.
    let mut visited = Lamp::new(catalogue()).at_night();
    for _ in 0..3 {
        visited.faces = Some(vec![face(0.1, 0.1, 0.1)]);
        visited.run(SECOND, |_, _| {});
        visited.faces = Some(vec![]);
        assert!(!visited.sleep_within(10 * MINUTE));
    }
}

#[test]
fn energy_changes_the_size_and_pace_of_the_same_moves() {
    let breath_at = |energy: f32| {
        let mut value = catalogue();
        value.alive.as_mut().unwrap().energy = energy;
        value.check().unwrap();
        let mut lamp = Lamp::new(value);
        lamp.run(SECOND, |_, _| {});
        // The breath alone, until the first drift.
        let mut lowest: f64 = 0.0;
        let mut first_drift = None;
        lamp.run(2 * MINUTE, |output, now| {
            if named(output, "drifting") {
                first_drift.get_or_insert(now - START);
            }
            if first_drift.is_none() {
                lowest = lowest.min(asked(output).head_pitch);
            }
        });
        (lowest, first_drift.unwrap())
    };
    let (quiet, quiet_drift) = breath_at(0.0);
    let (lively, lively_drift) = breath_at(1.0);
    // 0.7 and 1.3 times the breath's 0.03.
    assert!((quiet - -0.021).abs() < 1e-4, "{quiet}");
    assert!((lively - -0.039).abs() < 1e-4, "{lively}");
    // The same drift, waited for 1.25 and 0.75 times as long.
    let ratio = quiet_drift as f64 / lively_drift as f64;
    assert!(
        (ratio - 1.25 / 0.75).abs() < 0.02,
        "{quiet_drift} {lively_drift}"
    );
}

#[test]
fn the_catalogue_refuses_a_layer_that_could_twitch_or_reach_too_far() {
    let refused = |change: &dyn Fn(&mut Catalogue)| {
        let mut value = catalogue();
        change(&mut value);
        value.check().is_err()
    };
    assert!(!refused(&|_| {}));
    assert!(refused(&|v| v
        .alive
        .as_mut()
        .unwrap()
        .states
        .push(Voice::Speaking)));
    assert!(refused(&|v| v.alive.as_mut().unwrap().states.clear()));
    assert!(refused(&|v| v.alive.as_mut().unwrap().blend_us = 50_000));
    assert!(refused(&|v| v.alive.as_mut().unwrap().energy = 1.5));
    // A breath too large, too quick, or of the arm.
    assert!(refused(&|v| v
        .alive
        .as_mut()
        .unwrap()
        .breath
        .inhale
        .head_pitch = -0.05));
    assert!(refused(
        &|v| v.alive.as_mut().unwrap().breath.period_us = 2_000_000
    ));
    assert!(refused(&|v| v
        .alive
        .as_mut()
        .unwrap()
        .breath
        .inhale
        .shoulder = 0.01));
    // A drift with zero outside its range, too far, too quick.
    assert!(refused(&|v| v
        .alive
        .as_mut()
        .unwrap()
        .drift
        .low
        .head_pitch = 0.01));
    assert!(refused(&|v| v
        .alive
        .as_mut()
        .unwrap()
        .drift
        .low
        .head_pitch = -0.05));
    assert!(refused(
        &|v| v.alive.as_mut().unwrap().drift.move_us = 1_000_000
    ));
    assert!(refused(
        &|v| v.alive.as_mut().unwrap().drift.every_us = [90_000_000, 30_000_000]
    ));
    assert!(refused(&|v| v
        .alive
        .as_mut()
        .unwrap()
        .drift
        .high
        .base_yaw = 0.01));
    // Sleep: no night, too soon, a pose too far.
    assert!(refused(&|v| {
        v.alive.as_mut().unwrap().sleep.as_mut().unwrap().night = [600, 600]
    }));
    assert!(refused(&|v| {
        v.alive.as_mut().unwrap().sleep.as_mut().unwrap().nobody_us = 10_000_000
    }));
    assert!(refused(&|v| {
        v.alive
            .as_mut()
            .unwrap()
            .sleep
            .as_mut()
            .unwrap()
            .pose
            .head_pitch = 0.1
    }));
    // A stretch with no sleep, not back where it began, or a snap.
    assert!(refused(&|v| v.alive.as_mut().unwrap().sleep = None));
    assert!(refused(&|v| {
        let wake = v.alive.as_mut().unwrap().wake.as_mut().unwrap();
        wake.keyframes.last_mut().unwrap().offsets.head_pitch = -0.01;
    }));
    assert!(refused(&|v| {
        let wake = v.alive.as_mut().unwrap().wake.as_mut().unwrap();
        wake.keyframes[1].at_us = 100_000;
    }));
    assert!(refused(&|v| {
        let wake = v.alive.as_mut().unwrap().wake.as_mut().unwrap();
        wake.keyframes[2].offsets.elbow = 0.01;
    }));
}

#[test]
fn the_name_speech_switch_defaults_off_in_old_json_and_preserves_outputs() {
    let mut json = serde_json::to_value(drowsy()).unwrap();
    json["alive"]["sleep"]
        .as_object_mut()
        .unwrap()
        .remove("name_requires_speech");
    let absent = Catalogue::from_json(&json.to_string()).unwrap();
    assert!(
        !absent
            .alive
            .as_ref()
            .unwrap()
            .sleep
            .unwrap()
            .name_requires_speech
    );
    json["alive"]["sleep"]["name_requires_speech"] = false.into();
    let disabled = Catalogue::from_json(&json.to_string()).unwrap();
    assert_eq!(absent, disabled);
    json["alive"]["sleep"]["name_requires_speech"] = true.into();
    let enabled = Catalogue::from_json(&json.to_string()).unwrap();
    assert!(
        enabled
            .alive
            .as_ref()
            .unwrap()
            .sleep
            .unwrap()
            .name_requires_speech
    );

    let mut implicit = sleeping_with(absent);
    let mut explicit = sleeping_with(disabled);
    implicit.name();
    explicit.name();
    implicit.voice(Voice::Listening);
    explicit.voice(Voice::Listening);
    for _ in 0..150 {
        assert_eq!(
            serde_json::to_value(implicit.step()).unwrap(),
            serde_json::to_value(explicit.step()).unwrap()
        );
    }
    assert!(implicit.engine.take_sleep_name_notices().is_empty());
    assert!(explicit.engine.take_sleep_name_notices().is_empty());
}

#[test]
fn a_gated_name_without_person_speech_stays_asleep_and_reports_once() {
    let mut lamp = sleeping_with(speech_gated());
    lamp.voice(Voice::Listening);
    let onset = lamp.now;
    lamp.name_at(onset, 0);
    let paused = lamp.activity_at(lamp.now, false);
    lamp.send(Input::Activity(paused));
    let text = Speech {
        text: "Lamp, are you there?".into(),
        finished: true,
        locale: Some("en_GB".into()),
        request: Some(lamp.turn * 10 + 1),
    };
    let stamp = lamp.stamp();
    lamp.send(Input::Heard {
        stamp,
        value: text.clone(),
    });
    let stamp = lamp.stamp();
    lamp.send(Input::Said { stamp, value: text });
    lamp.sudden();
    lamp.stay_asleep_until(onset + 2 * SECOND + 300_000 - 1);
    assert!(lamp.engine.take_sleep_name_notices().is_empty());
    lamp.stay_asleep_until(onset + 2 * SECOND + 300_000);
    let notices = lamp.engine.take_sleep_name_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(notices[0].contains("no validated person speech"));
    assert!(lamp.engine.take_sleep_name_notices().is_empty());
    lamp.stay_asleep_until(lamp.now + SECOND);
    assert!(lamp.engine.take_sleep_name_notices().is_empty());
}

#[test]
fn a_pending_name_survives_ordinary_resting_until_the_next_listening_turn() {
    let mut lamp = sleeping_with(speech_gated());
    // Resting is normally not retaining input. Its heartbeats must not erase
    // a name that is waiting for the next real input owner.
    lamp.accepting_input = false;
    lamp.say_voice();
    lamp.name_at(lamp.now, 0);
    lamp.stay_asleep_until(lamp.now + SECOND);
    assert!(lamp.engine.take_sleep_name_notices().is_empty());
    lamp.accepting_input = true;
    lamp.voice(Voice::Listening);
    lamp.speech_at(lamp.now);
    assert!(named(&lamp.step(), "waking"));
    assert!(lamp.engine.take_sleep_name_notices().is_empty());
}

#[test]
fn a_repeated_finished_rest_heartbeat_does_not_erase_a_new_name() {
    let mut lamp = sleeping_with(speech_gated());
    lamp.accepting_input = false;
    lamp.say_voice_event(VoiceEvent::Finished);
    lamp.name_at(lamp.now, 0);
    lamp.now += TICK;
    lamp.say_voice_event(VoiceEvent::Finished);
    assert!(lamp.engine.take_sleep_name_notices().is_empty());
    lamp.accepting_input = true;
    lamp.voice(Voice::Listening);
    lamp.speech_at(lamp.now);
    assert!(named(&lamp.step(), "waking"));
    assert!(lamp.engine.take_sleep_name_notices().is_empty());
}

#[test]
fn a_corroborated_name_keeps_the_same_stretch_and_has_no_leap() {
    let mut lamp = sleeping_with(speech_gated());
    lamp.voice(Voice::Listening);
    lamp.speech_at(lamp.now);
    lamp.stay_asleep_until(lamp.now + 400_000);
    lamp.name_at(lamp.now, 400);
    let accepted = lamp.now;
    let mut waking = 0;
    let mut highest = 0.0_f64;
    let mut largest = 0.0_f64;
    let mut last: Option<BodyOffsets> = None;
    lamp.run(4 * SECOND, |output, now| {
        if now == accepted + TICK {
            assert!(named(output, "waking"));
        }
        waking += usize::from(named(output, "waking"));
        let offsets = asked(output);
        highest = highest.min(offsets.head_pitch);
        if let Some(before) = last {
            for (now, before) in offsets.values().iter().zip(before.values()) {
                largest = largest.max((now - before).abs());
            }
        }
        last = Some(offsets);
    });
    assert_eq!(waking, 136, "2.7 s at 20 ms, including the first frame");
    assert!(highest < -0.05, "{highest}");
    assert!(largest < 0.01, "{largest}");
    let output = lamp.step();
    assert!(!named(&output, "asleep") && !named(&output, "waking"));
    assert_eq!(asked(&output), catalogue().postures.listening);
    assert!(lamp.engine.take_sleep_name_notices().is_empty());
}

#[test]
fn source_speech_matches_both_inclusive_two_second_boundaries_only() {
    for offset in [-2_000_001_i64, -2_000_000, -1, 0, 1, 2_000_000, 2_000_001] {
        let mut lamp = sleeping_with(speech_gated());
        lamp.voice(Voice::Listening);
        let start = lamp.now;
        if offset < 0 {
            lamp.speech_at(start);
            lamp.stay_asleep_until(start + offset.unsigned_abs());
            lamp.name_at(lamp.now, 0);
        } else {
            lamp.name_at(start, 0);
            lamp.stay_asleep_until(start + offset as u64);
            lamp.speech_at(lamp.now);
        }
        let output = lamp.step();
        let matches = offset.unsigned_abs() <= 2 * SECOND;
        assert_eq!(named(&output, "waking"), matches, "offset={offset}");
        assert_eq!(named(&output, "asleep"), !matches, "offset={offset}");
        assert!(lamp.engine.take_sleep_name_notices().is_empty());
        if !matches {
            lamp.stay_asleep_until(lamp.now + 2 * SECOND + 300_000);
            let notices = lamp.engine.take_sleep_name_notices();
            assert_eq!(notices.len(), 1, "offset={offset}: {notices:?}");
            assert!(notices[0].contains("no validated person speech"));
        }
    }
}

#[test]
fn a_boundary_sample_delivered_before_its_age_limit_wakes_without_an_expiry_notice() {
    let mut lamp = sleeping_with(speech_gated());
    lamp.voice(Voice::Listening);
    let onset = lamp.now;
    lamp.name_at(onset, 0);
    lamp.stay_asleep_until(onset + 2 * SECOND + 300_000 - 1);
    lamp.speech_at(onset + 2 * SECOND);
    // This tick passes the pending deadline. The already matched name must
    // neither be lost nor be reported as a name without speech.
    assert!(named(&lamp.step(), "waking"));
    assert!(lamp.engine.take_sleep_name_notices().is_empty());
}

#[test]
fn the_delivery_grace_does_not_admit_a_sample_at_the_stale_boundary() {
    let mut lamp = sleeping_with(speech_gated());
    lamp.voice(Voice::Listening);
    let onset = lamp.now;
    lamp.name_at(onset, 0);
    lamp.stay_asleep_until(onset + 2 * SECOND + 300_000);
    let activity = lamp.activity_at(onset + 2 * SECOND, true);
    assert!(
        lamp.engine
            .ingest(lamp.now, Input::Activity(activity))
            .is_err()
    );
    assert!(named(&lamp.step(), "asleep"));
    let notices = lamp.engine.take_sleep_name_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(lamp.engine.take_sleep_name_notices().is_empty());
}

#[test]
fn delayed_name_delivery_can_match_an_earlier_true_before_a_later_unrelated_true() {
    let mut value = speech_gated();
    value.timing.context_stale_us = 10 * SECOND;
    let mut lamp = sleeping_with(value);
    lamp.voice(Voice::Listening);
    let first_speech = lamp.now;
    lamp.speech_at(first_speech);
    // Name onset is first_speech + 1 s. The newest observation is one
    // millisecond beyond that name's upper bound, so latest-only memory fails.
    lamp.stay_asleep_until(first_speech + 3 * SECOND + 1_000);
    lamp.speech_at(lamp.now);
    lamp.name_at(lamp.now, 2_001);
    assert!(named(&lamp.step(), "waking"));
    assert!(lamp.engine.take_sleep_name_notices().is_empty());
}

#[test]
fn the_name_onset_not_its_publication_or_receipt_selects_the_speech_window() {
    let mut lamp = sleeping_with(speech_gated());
    lamp.voice(Voice::Listening);
    lamp.speech_at(lamp.now);
    // Receipt and publication coincide with speech, but onset is outside
    // the allowed source interval. Restamping it would spuriously wake.
    lamp.name_at(lamp.now, 2_001);
    lamp.stay_asleep_until(lamp.now + 300_000);
    let notices = lamp.engine.take_sleep_name_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(notices[0].contains("no validated person speech"));
}

#[test]
fn an_impossible_or_stale_name_onset_is_ignored_once_without_rejecting_the_message() {
    for ago_ms in [u32::MAX, 5_000] {
        let mut lamp = sleeping_with(speech_gated());
        // The message publication is fresh; only its stated onset is invalid.
        let input = lamp.name_input(lamp.now, ago_ms);
        assert!(lamp.engine.ingest(lamp.now, input).is_ok());
        assert!(named(&lamp.step(), "asleep"));
        let notices = lamp.engine.take_sleep_name_notices();
        assert_eq!(notices.len(), 1, "ago_ms={ago_ms}: {notices:?}");
        assert!(notices[0].contains("invalid or stale name onset"));
        assert!(lamp.engine.take_sleep_name_notices().is_empty());
    }
}

#[test]
fn a_pause_does_not_erase_recent_accepted_person_speech() {
    let mut lamp = sleeping_with(speech_gated());
    lamp.voice(Voice::Listening);
    lamp.speech_at(lamp.now);
    lamp.stay_asleep_until(lamp.now + 100_000);
    let paused = lamp.activity_at(lamp.now, false);
    lamp.send(Input::Activity(paused));
    lamp.stay_asleep_until(lamp.now + 700_000);
    lamp.name_at(lamp.now, 0);
    assert!(named(&lamp.step(), "waking"));
    assert!(lamp.engine.take_sleep_name_notices().is_empty());
}

#[test]
fn rejected_activity_cannot_corroborate_a_sleeping_name() {
    for refusal in [
        "session",
        "turn",
        "request",
        "boot",
        "future",
        "stale",
        "before phase",
        "duplicate",
    ] {
        let mut lamp = sleeping_with(speech_gated());
        lamp.voice(Voice::Listening);
        let onset = lamp.now;
        lamp.name_at(onset, 0);
        let paused = lamp.activity_at(lamp.now, false);
        lamp.send(Input::Activity(paused.clone()));
        let mut activity = lamp.activity_at(lamp.now, true);
        match refusal {
            "session" => activity.session_id = "other-voice".into(),
            "turn" => activity.turn += 1,
            "request" => activity.request += 1,
            "boot" => activity.stamp.clock_id = "other-boot".into(),
            "future" => activity.stamp.at_us += 1,
            "stale" => activity.stamp.at_us -= 300_000,
            "before phase" => activity.stamp.at_us -= 1,
            "duplicate" => activity.stamp = paused.stamp,
            _ => unreachable!(),
        }
        assert!(
            lamp.engine
                .ingest(lamp.now, Input::Activity(activity))
                .is_err(),
            "{refusal}"
        );
        lamp.stay_asleep_until(onset + 2 * SECOND + 300_000);
        let notices = lamp.engine.take_sleep_name_notices();
        assert_eq!(notices.len(), 1, "{refusal}: {notices:?}");
        assert!(notices[0].contains("no validated person speech"));
    }
}

#[test]
fn owner_and_input_fences_remove_pending_names_and_unexecuted_name_wakes() {
    for fence in ["session", "cancelled", "interrupted", "finished", "input"] {
        for already_matched in [false, true] {
            let mut lamp = sleeping_with(speech_gated());
            lamp.voice(Voice::Listening);
            lamp.name_at(lamp.now, 0);
            if already_matched {
                lamp.speech_at(lamp.now);
            }
            // Fence before the first planner tick can execute the name wake.
            lamp.now += TICK;
            match fence {
                "session" => {
                    lamp.session = "voice-two";
                    lamp.say_voice();
                }
                "cancelled" => lamp.say_voice_event(VoiceEvent::Cancelled),
                "interrupted" => lamp.say_voice_event(VoiceEvent::Interrupted),
                "finished" => lamp.say_voice_event(VoiceEvent::Finished),
                "input" => {
                    lamp.accepting_input = false;
                    lamp.say_voice();
                }
                _ => unreachable!(),
            }
            let notices = lamp.engine.take_sleep_name_notices();
            if !already_matched {
                assert_eq!(notices.len(), 1, "{fence}: {notices:?}");
                assert!(notices[0].contains("was lost"));
            }
            assert!(lamp.engine.take_sleep_name_notices().is_empty());
            lamp.accepting_input = true;
            lamp.voice(Voice::Listening);
            lamp.say_parts();
            // This new valid speech cannot resurrect the fenced name.
            lamp.speech_at(lamp.now);
            let output = lamp.step();
            assert!(
                named(&output, "asleep"),
                "{fence}, matched={already_matched}"
            );
            assert!(!named(&output, "waking"));
        }
    }
}

#[test]
fn resumed_input_does_not_reuse_speech_acquired_before_readiness_was_lost() {
    let mut lamp = sleeping_with(speech_gated());
    lamp.voice(Voice::Listening);
    let before_loss = lamp.now;
    lamp.speech_at(before_loss);
    lamp.now = before_loss + 50_000;
    lamp.accepting_input = false;
    lamp.say_voice();
    lamp.now = before_loss + 60_000;
    lamp.name_at(lamp.now, 0);
    lamp.now = before_loss + 70_000;
    lamp.accepting_input = true;
    lamp.say_voice();
    lamp.now = before_loss + 80_000;
    // The ordinary Activity contract permits this same-turn, fresh packet.
    // The sleep-name rule must still honor the later readiness boundary.
    let delayed = lamp.activity_at(before_loss, true);
    assert!(
        lamp.engine
            .ingest(lamp.now, Input::Activity(delayed))
            .is_ok()
    );
    assert!(named(&lamp.engine.tick(lamp.now), "asleep"));
    assert!(lamp.engine.take_sleep_name_notices().is_empty());
    lamp.speech_at(lamp.now);
    assert!(named(&lamp.step(), "waking"));
    assert!(lamp.engine.take_sleep_name_notices().is_empty());
}

#[test]
fn lost_voice_freshness_reports_once_even_when_the_tick_cannot_plan() {
    let mut lamp = sleeping_with(speech_gated());
    lamp.voice(Voice::Listening);
    lamp.name_at(lamp.now, 0);
    lamp.now += 500_000;
    let output = lamp.engine.tick(lamp.now);
    assert!(output.body.is_none());
    assert!(
        output
            .state
            .missing
            .iter()
            .any(|why| why == "fresh voice timing")
    );
    let notices = lamp.engine.take_sleep_name_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(notices[0].contains("freshness was lost"));
    lamp.now += 3 * SECOND;
    assert!(lamp.engine.tick(lamp.now).body.is_none());
    assert!(lamp.engine.take_sleep_name_notices().is_empty());
    lamp.voice(Voice::Listening);
    lamp.say_parts();
    lamp.speech_at(lamp.now);
    assert!(named(&lamp.step(), "asleep"));
}

#[test]
fn a_clock_fault_clears_a_pending_name_and_keeps_the_notice_drainable() {
    let mut lamp = sleeping_with(speech_gated());
    lamp.name_at(lamp.now, 0);
    let output = lamp.engine.tick(lamp.now - 1);
    assert!(output.body.is_none());
    assert!(
        output
            .state
            .warnings
            .iter()
            .any(|why| why.contains("clock moved backwards"))
    );
    let notices = lamp.engine.take_sleep_name_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(notices[0].contains("was lost"));
    assert!(lamp.engine.tick(lamp.now + 3 * SECOND).body.is_none());
    assert!(lamp.engine.take_sleep_name_notices().is_empty());
}

#[test]
fn a_name_flood_is_bounded_and_cannot_extend_the_original_deadlines() {
    let mut lamp = sleeping_with(speech_gated());
    lamp.voice(Voice::Listening);
    let onset = lamp.now;
    for _ in 0..16 {
        lamp.name_at(onset, 0);
    }
    lamp.stay_asleep_until(onset + 2 * SECOND + 200_000);
    for _ in 0..1_000 {
        lamp.name_at(lamp.now, 0);
    }
    let notices = lamp.engine.take_sleep_name_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(notices[0].contains("(1000)") && notices[0].contains("pending name limit"));
    assert!(lamp.engine.take_sleep_name_notices().is_empty());
    lamp.stay_asleep_until(onset + 2 * SECOND + 300_000);
    let expired = lamp.engine.take_sleep_name_notices();
    assert_eq!(expired.len(), 1, "{expired:?}");
    assert!(expired[0].contains("(16)") && expired[0].contains("no validated person speech"));
    // This speech would match the overflow names if any had been retained.
    lamp.speech_at(lamp.now);
    assert!(named(&lamp.step(), "asleep"));
    lamp.stay_asleep_until(lamp.now + 3 * SECOND);
    assert!(lamp.engine.take_sleep_name_notices().is_empty());
}

#[test]
fn speech_history_keeps_128_observations_and_discards_the_oldest_fail_closed() {
    for later_observations in [127, 128] {
        let mut lamp = sleeping_with(speech_gated());
        lamp.voice(Voice::Listening);
        let original = lamp.now;
        lamp.speech_at(original);
        lamp.stay_asleep_until(original + 3 * SECOND);
        let publication = lamp.now;
        for _ in 0..later_observations {
            lamp.now += 1;
            lamp.speech_at(lamp.now);
        }
        // Only the oldest timestamp lies in this name's source window.
        lamp.name_at(publication, 3_000);
        let output = lamp.step();
        assert_eq!(named(&output, "waking"), later_observations == 127);
        assert_eq!(named(&output, "asleep"), later_observations == 128);
        let notices = lamp.engine.take_sleep_name_notices();
        assert_eq!(notices.len(), usize::from(later_observations == 128));
    }
}

#[test]
fn fencing_a_name_does_not_cancel_a_touch_or_reminder_wake() {
    for reminder in [false, true] {
        let mut lamp = sleeping_with(speech_gated());
        lamp.voice(Voice::Listening);
        lamp.name_at(lamp.now, 0);
        if reminder {
            lamp.due(9);
        } else {
            lamp.touch(ControlSource::Base, Gesture::Tap, true);
        }
        lamp.accepting_input = false;
        lamp.say_voice();
        assert!(named(&lamp.step(), "waking"), "reminder={reminder}");
        let notices = lamp.engine.take_sleep_name_notices();
        assert_eq!(notices.len(), 1, "{notices:?}");
        assert!(lamp.engine.take_sleep_name_notices().is_empty());
    }
}

#[test]
fn successful_first_and_repeated_name_wakes_need_only_the_next_planner_tick() {
    for speech_first in [true, false] {
        let mut lamp = sleeping_with(speech_gated());
        for repetition in 0..2 {
            if repetition != 0 {
                lamp.last(3 * SECOND);
                lamp.voice(Voice::Resting);
                assert!(lamp.sleep_within(2 * MINUTE));
            }
            lamp.voice(Voice::Listening);
            let second = if speech_first {
                lamp.speech_at(lamp.now);
                lamp.name_input(lamp.now, 0)
            } else {
                lamp.name_at(lamp.now, 0);
                Input::Activity(lamp.activity_at(lamp.now, true))
            };
            let received_us = lamp.now;
            let began = Instant::now();
            lamp.send(second);
            let output = lamp.step();
            let wall = began.elapsed();
            eprintln!(
                "sleep-name development-host planner calls: speech_first={speech_first} \
                 repetition={repetition} second_input_ingest_to_first_waking_tick_ns={} \
                 injected_tick_us={}; excludes detectors, bus, service and hardware; \
                 injected time is not host delay",
                wall.as_nanos(),
                lamp.now - received_us
            );
            assert!(named(&output, "waking"));
            assert!(lamp.now - received_us <= TICK);
            assert!(lamp.engine.take_sleep_name_notices().is_empty());
        }
    }
}

/// The head (#711): what `--touch` Controls says of a hand on it.
mod head {
    use super::*;
    use lamp_msgs::{
        MuteReason, TouchEvent, TouchGesture, TouchIntent, TouchSource, TouchSourceState,
        TouchState, VoiceMute,
    };

    impl Lamp {
        fn head_event(&mut self, gesture: TouchGesture, contact: u64, held: u64) -> Input {
            let stamp = self.stamp();
            let (speaking, quiet, asleep) = match gesture {
                TouchGesture::Tap => (
                    TouchIntent::Attend,
                    TouchIntent::Attend,
                    Some(TouchIntent::Wake),
                ),
                TouchGesture::Stroke => (TouchIntent::Stroke, TouchIntent::Stroke, None),
                TouchGesture::Hold => (TouchIntent::Sleep, TouchIntent::Sleep, None),
                _ => (TouchIntent::Stop, TouchIntent::Notice, None),
            };
            let tap = gesture == TouchGesture::Tap;
            let value = TouchEvent {
                stamp: stamp.clone(),
                session_id: "controls-one".into(),
                reader_session_id: "gpio".into(),
                enabled: true,
                source: TouchSource::Head,
                name: "head".into(),
                contact_id: contact,
                gesture,
                when_speaking: speaking,
                when_quiet: quiet,
                when_asleep: asleep,
                began_us: self.now - held - if tap { 150_000 } else { 0 },
                observed_us: self.now,
                released_us: tap.then(|| self.now - 150_000),
                active_mask: u16::from(!tap),
                active_names: if tap { vec![] } else { vec!["head-1".into()] },
                position: None,
                toward_pad: None,
                distance: 0.0,
                speed: 0.0,
            };
            value.check().unwrap();
            Input::HeadTouch { stamp, value }
        }

        fn head(&mut self, gesture: TouchGesture, contact: u64, held: u64) {
            let input = self.head_event(gesture, contact, held);
            self.send(input);
        }

        /// The head's state: `contact` there for `held`, or nobody.
        fn hand(&mut self, contact: Option<(u64, u64)>) {
            let stamp = self.stamp();
            let source = TouchSourceState {
                source: TouchSource::Head,
                name: "head".into(),
                reader_session_id: Some("gpio".into()),
                ready: true,
                fault: None,
                observed_us: Some(self.now),
                armed: true,
                active: Some(contact.is_some()),
                contact_id: contact.map(|(id, _)| id),
                began_us: contact.map(|(_, held)| self.now - held),
                held_us: contact.map(|(_, held)| held),
                active_mask: Some(u16::from(contact.is_some())),
                active_names: if contact.is_some() {
                    vec!["head-1".into()]
                } else {
                    vec![]
                },
                position: None,
                toward_pad: None,
                distance: 0.0,
                speed: 0.0,
            };
            let value = TouchState {
                stamp: stamp.clone(),
                session_id: "controls-one".into(),
                enabled: true,
                sources: vec![source],
                latest: vec![],
                event_sequence: 0,
                rejected_inputs: 0,
            };
            value.check().unwrap();
            self.send(Input::HeadHand { stamp, value });
        }

        /// The voice's mute: open, muted, or muted because it was put to
        /// sleep.
        fn mute(&mut self, muted: bool, asleep: bool) {
            let stamp = self.stamp();
            let value = VoiceMute {
                muted,
                reason: asleep.then_some(MuteReason::Sleep),
            };
            self.send(Input::VoiceMute { stamp, value });
        }

        /// Ticks for `micros`, saying the hand every 100 ms as Controls does
        /// at least every 20 ms; the last output.
        fn with_hand(&mut self, contact: Option<(u64, u64)>, micros: u64) -> Output {
            let until = self.now + micros;
            let (mut last, mut held) = (None, contact.map_or(0, |(_, held)| held));
            while self.now < until {
                last = Some(self.step());
                if self.now.is_multiple_of(100_000) {
                    held += 100_000;
                    self.hand(contact.map(|(id, _)| (id, held)));
                }
            }
            last.unwrap()
        }
    }

    #[test]
    fn a_hand_kept_on_its_head_is_leaned_into_until_it_goes() {
        let (mut stroked, mut alone) = (Lamp::new(catalogue()), Lamp::new(catalogue()));
        for lamp in [&mut stroked, &mut alone] {
            lamp.run(10 * SECOND, |_, _| {});
        }
        stroked.hand(Some((7, 1_000_000)));
        stroked.head(TouchGesture::Stroke, 7, 1_000_000);
        alone.hand(None);
        let leaning = stroked.with_hand(Some((7, 1_000_000)), SECOND);
        let resting = alone.with_hand(None, SECOND);
        assert!(
            named(&leaning, "leaning into a hand"),
            "{:?}",
            leaning.state
        );
        assert!(!named(&resting, "leaning into a hand"));
        let (lean, rest) = (asked(&leaning), asked(&resting));
        assert!(
            (lean.head_roll - rest.head_roll - LEAN.head_roll).abs() < 1e-9,
            "{lean:?} {rest:?}"
        );
        assert!((lean.head_pitch - rest.head_pitch - LEAN.head_pitch).abs() < 1e-9);
        // The hand goes (said 0.1 s later here): the lean lets go over
        // 0.6 s, a little each tick, and nothing leaps.
        let mut roll = LEAN.head_roll;
        for _ in 0..30 {
            let now = asked(&stroked.with_hand(None, TICK)).head_roll;
            // The easing is at most 1.5 times as steep as a straight line.
            let most = 1.5 * LEAN.head_roll * TICK as f64 / lamp_expression::LEAN_OUT_US as f64;
            assert!(roll - now <= most + 1e-12, "{roll} {now}");
            roll = now;
        }
        let gone = stroked.with_hand(None, 200_000);
        assert!(!named(&gone, "leaning into a hand"), "{:?}", gone.state);
    }

    #[test]
    fn a_lean_lets_go_when_the_head_says_nothing_more() {
        let mut lamp = Lamp::new(catalogue());
        lamp.run(10 * SECOND, |_, _| {});
        lamp.hand(Some((3, 1_000_000)));
        lamp.head(TouchGesture::Stroke, 3, 1_000_000);
        // Controls stopped: no state at all. After half a second the hand
        // counts as gone, and the lean lets go over the next 0.6 s.
        let mut last = None;
        for _ in 0..60 {
            last = Some(lamp.step());
        }
        assert!(!named(&last.unwrap(), "leaning into a hand"));
    }

    #[test]
    fn a_three_second_hold_puts_it_to_sleep_by_day_and_a_tap_wakes_it() {
        let mut lamp = Lamp::new(catalogue());
        lamp.run(10 * SECOND, |_, _| {});
        lamp.hand(Some((9, 3_000_000)));
        lamp.head(TouchGesture::Hold, 9, 3_000_000);
        let asleep = lamp.with_hand(None, SECOND);
        assert!(named(&asleep, "falling asleep"), "{:?}", asleep.state);
        // Asleep, a hand kept there is not leaned into.
        lamp.head(TouchGesture::Stroke, 10, 1_000_000);
        let still = lamp.with_hand(Some((10, 1_000_000)), SECOND);
        assert!(!named(&still, "leaning into a hand"));
        lamp.hand(None);
        lamp.head(TouchGesture::Tap, 11, 200_000);
        let woken = lamp.with_hand(None, 100_000);
        assert!(named(&woken, "waking"), "{:?}", woken.state);
    }

    #[test]
    fn put_to_sleep_by_the_voice_it_sleeps_until_the_voice_opens_whatever_calls() {
        let mut lamp = Lamp::new(catalogue());
        lamp.run(10 * SECOND, |_, _| {});
        lamp.mute(false, false);
        // Held three seconds: the voice takes it as sleep, and so it sleeps.
        lamp.hand(Some((9, 3_000_000)));
        lamp.head(TouchGesture::Hold, 9, 3_000_000);
        lamp.mute(true, true);
        let asleep = lamp.with_hand(None, SECOND);
        assert!(named(&asleep, "falling asleep"), "{:?}", asleep.state);
        // A tap the voice did not take as a wake (its microphones are still
        // off) does not wake it.
        lamp.mute(true, true);
        lamp.head(TouchGesture::Tap, 10, 200_000);
        let still = lamp.with_hand(None, 100_000);
        assert!(!named(&still, "waking"), "{:?}", still.state);
        // The voice opens: it wakes.
        lamp.mute(false, false);
        let woken = lamp.with_hand(None, 100_000);
        assert!(named(&woken, "waking"), "{:?}", woken.state);
    }

    #[test]
    fn asleep_by_the_voice_with_no_hand_seen_as_after_a_restart() {
        let mut lamp = Lamp::new(catalogue());
        lamp.run(10 * SECOND, |_, _| {});
        lamp.mute(true, true);
        let asleep = lamp.with_hand(None, SECOND);
        assert!(named(&asleep, "falling asleep"), "{:?}", asleep.state);
    }

    #[test]
    fn a_hold_the_voice_did_not_take_as_sleep_is_not_one() {
        // Open, or muted for a person's privacy: no sleep from the head.
        for (muted, label) in [(false, "open"), (true, "muted")] {
            let mut lamp = Lamp::new(catalogue());
            lamp.run(10 * SECOND, |_, _| {});
            lamp.mute(muted, false);
            lamp.hand(Some((9, 3_000_000)));
            lamp.head(TouchGesture::Hold, 9, 3_000_000);
            let awake = lamp.with_hand(None, SECOND);
            assert!(
                !named(&awake, "falling asleep") && !named(&awake, "asleep"),
                "{label}: {:?}",
                awake.state
            );
        }
    }

    #[test]
    fn when_the_voice_has_said_nothing_for_three_seconds_the_head_acts_as_before() {
        let mut lamp = Lamp::new(catalogue());
        lamp.run(10 * SECOND, |_, _| {});
        lamp.mute(false, false);
        lamp.run(3 * SECOND, |_, _| {});
        lamp.hand(Some((9, 3_000_000)));
        lamp.head(TouchGesture::Hold, 9, 3_000_000);
        let asleep = lamp.with_hand(None, SECOND);
        assert!(named(&asleep, "falling asleep"), "{:?}", asleep.state);
    }

    /// The tapped lamp's head pitch, less the same lamp's without the tap,
    /// and whether it says it is looking up, over `ticks` of 20 ms.
    fn looks(tapped: &mut Lamp, plain: &mut Lamp, ticks: usize) -> Vec<(f64, f64, bool)> {
        (0..ticks)
            .map(|_| {
                let (a, b) = (tapped.step(), plain.step());
                (
                    asked(&a).head_pitch - asked(&b).head_pitch,
                    asked(&a).base_yaw - asked(&b).base_yaw,
                    named(&a, "looking up at a tap"),
                )
            })
            .collect()
    }

    fn twins(faces: Vec<Face>) -> (Lamp, Lamp) {
        let (mut tapped, mut plain) = (Lamp::new(catalogue()), Lamp::new(catalogue()));
        for lamp in [&mut tapped, &mut plain] {
            lamp.faces = Some(faces.clone());
            lamp.run(10 * SECOND, |_, _| {});
        }
        (tapped, plain)
    }

    #[test]
    fn a_tap_on_an_awake_lamp_looks_up_at_once_for_two_seconds_and_settles_slowly() {
        let (mut tapped, mut plain) = twins(vec![]);
        tapped.head(TouchGesture::Tap, 5, 200_000);
        let pitch = lamp_expression::LOOK_UP.head_pitch;
        // Up within the moment of the hum: all the way by 160 ms.
        let first = looks(&mut tapped, &mut plain, 8);
        assert!(first.iter().all(|(_, _, named)| *named));
        assert!((first[7].0 - pitch).abs() < 1e-9, "{first:?}");
        assert!(first[0].0 < 0.0 && first[0].0 > pitch);
        // Held for two seconds from the tap; only the head, never the body.
        let held = looks(&mut tapped, &mut plain, 90);
        assert!(
            held.iter()
                .all(|(up, yaw, _)| (up - pitch).abs() < 1e-9 && *yaw == 0.0)
        );
        // Then, two seconds from the tap, down, eased over half a second:
        // never a snap.
        let after = looks(&mut tapped, &mut plain, 30);
        let easing = after
            .iter()
            .filter(|(up, _, _)| *up < 0.0 && *up > pitch + 1e-9)
            .count();
        assert!((20..=26).contains(&easing), "{after:?}");
        assert!(
            after
                .windows(2)
                .all(|w| (w[1].0 - w[0].0).abs() < 0.2 * -pitch)
        );
        assert!(after[29].0.abs() < 1e-9 && !after[29].2);
    }

    #[test]
    fn toward_a_face_seen_high_it_looks_up_more_and_never_turns() {
        // A face near the top of the picture (its centre at -0.8).
        let (mut tapped, mut plain) = twins(vec![face(0.4, 0.0, 0.2)]);
        tapped.head(TouchGesture::Tap, 5, 200_000);
        let up = looks(&mut tapped, &mut plain, 20);
        let wanted = lamp_expression::LOOK_UP.head_pitch - 0.8 * lamp_expression::LOOK_UP_TOWARD;
        assert!((up[19].0 - wanted).abs() < 1e-6, "{up:?}");
        assert!(up.iter().all(|(_, yaw, _)| *yaw == 0.0));
    }

    #[test]
    fn while_someone_speaks_it_keeps_looking_and_stops_when_the_lamp_thinks() {
        let (mut tapped, mut plain) = twins(vec![]);
        tapped.head(TouchGesture::Tap, 5, 200_000);
        looks(&mut tapped, &mut plain, 50);
        for lamp in [&mut tapped, &mut plain] {
            lamp.voice(Voice::Listening);
        }
        // Three seconds after the tap, still listening to them: still up.
        let speaking = looks(&mut tapped, &mut plain, 100);
        assert!(
            speaking.iter().all(|(up, _, named)| *named && *up < 0.0),
            "{speaking:?}"
        );
        for lamp in [&mut tapped, &mut plain] {
            lamp.voice(Voice::Thinking);
        }
        let thinking = looks(&mut tapped, &mut plain, 30);
        // Thinking: it eases down at once and is level within half a second.
        assert!(thinking[0].0 > lamp_expression::LOOK_UP.head_pitch);
        assert!(thinking[29].0.abs() < 1e-9 && !thinking[29].2);
    }

    #[test]
    fn a_tap_that_wakes_it_or_comes_while_it_speaks_is_no_look_up() {
        let (mut tapped, mut plain) = twins(vec![]);
        for lamp in [&mut tapped, &mut plain] {
            lamp.voice(Voice::Speaking);
        }
        tapped.head(TouchGesture::Tap, 5, 200_000);
        assert!(
            looks(&mut tapped, &mut plain, 20)
                .iter()
                .all(|(_, _, named)| !named)
        );
        // Asleep, a tap wakes it and does not look up.
        let mut lamp = Lamp::new(catalogue());
        lamp.run(10 * SECOND, |_, _| {});
        lamp.hand(Some((9, 3_000_000)));
        lamp.head(TouchGesture::Hold, 9, 3_000_000);
        lamp.with_hand(None, SECOND);
        lamp.head(TouchGesture::Tap, 11, 200_000);
        let woken = lamp.with_hand(None, 300_000);
        assert!(named(&woken, "waking") && !named(&woken, "looking up at a tap"));
    }

    /// First on #711, 17 Oct 10:18: muted for privacy, by the voice's own
    /// mute or by the privacy switch, a tap gets no look up, since looking
    /// up says "I'm listening". Open again, it looks up as before. A mute
    /// that begins while it looks up ends the look. The voice's sleep is
    /// not this: a tap still wakes it (above).
    #[test]
    fn muted_for_privacy_a_tap_gets_no_look_up() {
        let none = |looked: &[(f64, f64, bool)]| {
            looked
                .iter()
                .all(|(up, yaw, named)| !named && *up == 0.0 && *yaw == 0.0)
        };
        // The voice's own mute.
        let (mut tapped, mut plain) = twins(vec![]);
        for lamp in [&mut tapped, &mut plain] {
            lamp.mute(true, false);
        }
        tapped.head(TouchGesture::Tap, 5, 200_000);
        let looked = looks(&mut tapped, &mut plain, 20);
        assert!(none(&looked), "{looked:?}");
        // The privacy switch closed, as `voice/privacy` said at the tap.
        let (mut tapped, mut plain) = twins(vec![]);
        tapped.engine.set_privacy_muted(true);
        tapped.head(TouchGesture::Tap, 5, 200_000);
        let looked = looks(&mut tapped, &mut plain, 20);
        assert!(none(&looked), "{looked:?}");
        // Open again: a tap looks up as before.
        tapped.engine.set_privacy_muted(false);
        tapped.head(TouchGesture::Tap, 6, 200_000);
        let up = looks(&mut tapped, &mut plain, 20);
        assert!(up.iter().all(|(_, _, named)| *named), "{up:?}");
        // Muted while it looks up: down, eased, and no longer looking.
        for lamp in [&mut tapped, &mut plain] {
            lamp.mute(true, false);
        }
        let ended = looks(&mut tapped, &mut plain, 30);
        assert!(ended[29].0.abs() < 1e-9 && !ended[29].2, "{ended:?}");
    }

    /// The reviewer on #1282: the voice's mute holds until the voice says
    /// it is open, however long it goes unsaid (a voice ending or
    /// restarting while muted never reads as open).
    #[test]
    fn the_voices_mute_holds_until_it_says_open() {
        let (mut tapped, mut plain) = twins(vec![]);
        for lamp in [&mut tapped, &mut plain] {
            lamp.mute(true, false);
            lamp.run(5 * SECOND, |_, _| {});
        }
        tapped.head(TouchGesture::Tap, 5, 200_000);
        let looked = looks(&mut tapped, &mut plain, 20);
        assert!(looked.iter().all(|(_, _, named)| !named), "{looked:?}");
    }

    /// The seventh on #1282: the tap that clears the voice's mute (the
    /// restart guard's, `LAMP_RESTORED_MUTE`) gets its look up once the
    /// voice says it is open. A mute opened later than that, by something
    /// else, gives no look up for an old tap.
    #[test]
    fn the_tap_that_clears_a_mute_looks_up_once_it_is_open() {
        let (mut tapped, mut plain) = twins(vec![]);
        for lamp in [&mut tapped, &mut plain] {
            lamp.mute(true, false);
        }
        tapped.head(TouchGesture::Tap, 5, 200_000);
        looks(&mut tapped, &mut plain, 5);
        // The voice's once-a-second "still muted" between the tap and the
        // open uses nothing up (the reviewer on #1282).
        for lamp in [&mut tapped, &mut plain] {
            lamp.mute(true, false);
        }
        looks(&mut tapped, &mut plain, 5);
        for lamp in [&mut tapped, &mut plain] {
            lamp.mute(false, false);
        }
        let up = looks(&mut tapped, &mut plain, 20);
        assert!(up.iter().all(|(_, _, named)| *named), "{up:?}");
        // Muted again, a tap, and the mute opened only 2 s later.
        let (mut tapped, mut plain) = twins(vec![]);
        for lamp in [&mut tapped, &mut plain] {
            lamp.mute(true, false);
        }
        tapped.head(TouchGesture::Tap, 6, 200_000);
        looks(&mut tapped, &mut plain, 100);
        for lamp in [&mut tapped, &mut plain] {
            lamp.mute(false, false);
        }
        let late = looks(&mut tapped, &mut plain, 20);
        assert!(late.iter().all(|(_, _, named)| !named), "{late:?}");
    }

    /// The same tap does not wake a lamp asleep while it is muted for
    /// privacy: waking moves it, and nothing moves. Open again, it does.
    #[test]
    fn muted_for_privacy_a_tap_does_not_wake_it() {
        let mut lamp = Lamp::new(catalogue());
        lamp.run(10 * SECOND, |_, _| {});
        lamp.hand(Some((9, 3_000_000)));
        lamp.head(TouchGesture::Hold, 9, 3_000_000);
        let asleep = lamp.with_hand(None, SECOND);
        assert!(named(&asleep, "falling asleep"), "{:?}", asleep.state);
        lamp.engine.set_privacy_muted(true);
        lamp.head(TouchGesture::Tap, 11, 200_000);
        let still = lamp.with_hand(None, 300_000);
        assert!(!named(&still, "waking"), "{:?}", still.state);
        lamp.engine.set_privacy_muted(false);
        lamp.head(TouchGesture::Tap, 12, 200_000);
        let woken = lamp.with_hand(None, 100_000);
        assert!(named(&woken, "waking"), "{:?}", woken.state);
    }

    #[test]
    fn a_request_not_enabled_or_from_elsewhere_does_nothing() {
        let mut lamp = Lamp::new(catalogue());
        lamp.run(10 * SECOND, |_, _| {});
        let mut input = lamp.head_event(TouchGesture::Hold, 4, 3_000_000);
        if let Input::HeadTouch { value, .. } = &mut input {
            value.enabled = false;
        }
        lamp.send(input);
        let awake = lamp.with_hand(None, SECOND);
        assert!(!named(&awake, "falling asleep") && !named(&awake, "asleep"));
    }
}

/// When a drift began, in a run of `minutes` with the body's goals.
fn drift_starts(goals: fn(u64) -> Vec<f64>, minutes: u64) -> Vec<u64> {
    let mut lamp = Lamp::new(catalogue());
    lamp.goals = goals;
    lamp.run(SECOND, |_, _| {});
    let mut starts = Vec::new();
    let mut drifting = false;
    lamp.run(minutes * MINUTE, |output, now| {
        let now_drifting = named(output, "drifting");
        if now_drifting && !drifting {
            starts.push(now);
        }
        drifting = now_drifting;
    });
    starts
}

/// One quiet window with the body's own shifts at rest (#933): no drift
/// begins within 30 s of the body moving by itself, seen on
/// `body/joint_states`; a goal that jumps once is not such a move.
#[test]
fn no_drift_begins_within_thirty_seconds_of_a_shift_of_the_body() {
    // Every 70 s the body lifts its head 0.08 rad and back over 3.2 s,
    // eased, as its `look_up` does.
    fn shifting(now: u64) -> Vec<f64> {
        let into = (now - START) % (70 * SECOND);
        let share = (into as f64 / 3_200_000.0).min(1.0);
        let lift = -0.08 * (1.0 - (share * std::f64::consts::TAU).cos()) / 2.0;
        vec![0.0, 0.0, 0.0, 0.0, lift]
    }
    let starts = drift_starts(shifting, 15);
    assert!(starts.len() >= 3, "{starts:?}");
    for start in &starts {
        let since_shift = (start - START) % (70 * SECOND);
        assert!(
            since_shift >= 30 * SECOND,
            "a drift {} s after a shift began",
            since_shift / SECOND
        );
    }
    // A goal that leaps once every 10 s and stays (a head led as it
    // lifts, then not) holds no drift off.
    fn leaping(now: u64) -> Vec<f64> {
        let lead = if ((now - START) / (10 * SECOND)).is_multiple_of(2) {
            0.06
        } else {
            0.0
        };
        vec![0.0, 0.0, 0.0, 0.0, lead]
    }
    let still = drift_starts(|_| vec![0.0; 5], 15);
    assert!(still.len() >= 9, "{still:?}");
    assert_eq!(drift_starts(leaping, 15), still);
}
