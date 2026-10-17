use lamp_expression::{Catalogue, Engine, Input, Output, TICK_US, Trigger};
use lamp_msgs::{
    BodyOffsets, ClockStamp, Concentration, ExpressionFace, Face, Faces, JointState, Light,
    PersonActivity, Room, RoomState, Voice, VoiceEvent, VoicePlayout, VoiceProgress, VoiceSound,
    VoiceTiming,
};

const CLOCK: &str = "rest-test-boot";

fn catalogue() -> Catalogue {
    let mut value = Catalogue::from_json(include_str!("../rest-behaviours.json")).unwrap();
    // This is an injected zero lag for planner tests, not a motor measurement.
    value.timing.body_lag_us = Some(0);
    value
}

struct Harness {
    engine: Engine,
    now: u64,
    sequence: u64,
    session: String,
    turn: u64,
    generation: u64,
    state: Voice,
    held: bool,
    ready: bool,
    speaking: Option<bool>,
    room: Room,
}

impl Harness {
    fn new(state: Voice) -> Self {
        Self::configured(catalogue(), state, true)
    }

    fn configured(catalogue: Catalogue, state: Voice, enabled: bool) -> Self {
        let mut h = Self {
            engine: Engine::new(catalogue, enabled, CLOCK).unwrap(),
            now: 0,
            sequence: 0,
            session: "voice-one".into(),
            turn: 1,
            generation: 1,
            state,
            held: false,
            ready: true,
            speaking: None,
            room: Room::Quiet,
        };
        h.progress(0, state, 1, false, VoiceEvent::State);
        h.context(0);
        let stamp = h.stamp(0);
        h.send(Input::Light {
            stamp,
            value: Light::new(true, "blue", 2).unwrap(),
        });
        h
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
        self.now = input.stamp().at_us;
        self.engine.ingest(self.now, input).unwrap();
    }

    fn progress_input(
        &mut self,
        at: u64,
        state: Voice,
        generation: u64,
        held: bool,
        event: VoiceEvent,
    ) -> Input {
        Input::Progress(VoiceProgress {
            timing: VoiceTiming {
                stamp: self.stamp(at),
                session_id: self.session.clone(),
                turn: self.turn,
                request: self.turn + 10,
                state,
                accepting_input: self.ready,
                event,
                first_audio_us: None,
            },
            audio_generation: generation,
            held,
        })
    }

    fn progress(&mut self, at: u64, state: Voice, generation: u64, held: bool, event: VoiceEvent) {
        let input = self.progress_input(at, state, generation, held, event);
        self.send(input);
        self.state = state;
        self.generation = generation;
        self.held = held;
    }

    fn context(&mut self, at: u64) {
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
            value: RoomState { state: self.room },
        });
    }

    fn activity(&mut self, at: u64, speaking: bool) {
        let stamp = self.stamp(at);
        self.send(Input::Activity(PersonActivity {
            stamp,
            session_id: self.session.clone(),
            turn: self.turn,
            request: self.turn + 10,
            speaking,
        }));
        self.speaking = Some(speaking);
    }

    fn sound(&mut self, at: u64, generation: u64, start: u64, end: u64) -> Input {
        Input::Sound(VoiceSound {
            playout: VoicePlayout {
                stamp: self.stamp(at),
                session_id: self.session.clone(),
                turn: self.turn,
                request: self.turn + 10,
                start_us: start,
                end_us: end,
                level: 0.8,
            },
            audio_generation: generation,
        })
    }

    fn step(&mut self, at: u64) -> Output {
        self.progress(
            at,
            self.state,
            self.generation,
            self.held,
            VoiceEvent::State,
        );
        self.context(at);
        if self.state == Voice::Listening
            && let Some(speaking) = self.speaking
        {
            self.activity(at, speaking);
        }
        self.engine.tick(at)
    }

    fn until(&mut self, at: u64) -> Output {
        while self.now + 100_000 < at {
            self.step(self.now + 100_000);
        }
        self.step(at)
    }

    fn focus(&mut self, at: u64, concentrating: bool) {
        let stamp = self.stamp(at);
        self.send(Input::Concentration(Concentration {
            stamp,
            concentrating,
        }));
    }

    fn has(output: &Output, trigger: Trigger) -> bool {
        let name = catalogue()
            .behaviours
            .into_iter()
            .find(|b| b.trigger == trigger)
            .unwrap()
            .name;
        output.state.behaviours.contains(&name)
    }
}

fn started_nod() -> Harness {
    let mut h = Harness::new(Voice::Listening);
    h.activity(0, true);
    h.step(100_000);
    h.activity(100_000, false);
    assert!(Harness::has(&h.until(340_000), Trigger::Pause));
    h.until(600_000);
    h.progress(600_000, Voice::Thinking, 1, false, VoiceEvent::State);
    assert!(Harness::has(&h.engine.tick(600_000), Trigger::Pause));
    h
}

#[test]
fn protocols_are_selected_once_and_cannot_refresh_each_other() {
    let mut h = Harness::new(Voice::Speaking);
    let Input::Progress(progress) =
        h.progress_input(10_000, Voice::Speaking, 1, false, VoiceEvent::State)
    else {
        unreachable!()
    };
    assert!(
        h.engine
            .ingest(10_000, Input::Voice(progress.timing))
            .is_err()
    );
    let Input::Sound(sound) = h.sound(10_000, 1, 10_000, 30_000) else {
        unreachable!()
    };
    assert!(
        h.engine
            .ingest(10_000, Input::Playout(sound.playout))
            .is_err()
    );
    let mut legacy = Engine::new(Catalogue::builtin(), true, CLOCK).unwrap();
    let progress = h.progress_input(10_000, Voice::Speaking, 1, false, VoiceEvent::State);
    assert!(legacy.ingest(10_000, progress).is_err());
    assert!(
        legacy
            .ingest(10_000, h.sound(10_000, 1, 10_000, 30_000))
            .is_err()
    );
    assert!(h.engine.tick(500_000).light.is_none());
}

#[test]
fn hold_shows_listening_and_fresh_gaze_on_the_next_tick() {
    let mut h = Harness::new(Voice::Thinking);
    h.until(260_000);
    let before = h.engine.tick(260_000).body.unwrap().offsets;
    let stamp = h.stamp(270_000);
    h.send(Input::Faces {
        stamp,
        value: Faces {
            faces: vec![Face {
                speaking: None,
                id: "speaker".into(),
                xmin: 0.5,
                ymin: 0.1,
                xmax: 0.9,
                ymax: 0.7,
                confidence: 0.9,
                yaw: None,
                facing: None,
            }],
        },
    });
    // This response to a person is allowed in Busy; unsolicited rest is not.
    h.room = Room::Busy;
    h.context(270_000);
    h.progress(270_000, Voice::Listening, 2, true, VoiceEvent::State);
    let out = h.engine.tick(270_000 + TICK_US);
    assert_eq!(out.light.unwrap().face, ExpressionFace::Listening);
    let body = out.body.unwrap();
    assert_eq!(body.gaze.unwrap().face_id, "speaker");
    assert!(body.offsets.base_yaw < before.base_yaw);
    assert!(body.offsets.base_yaw > 0.0);
    assert!(out.state.stamp.at_us - 270_000 <= 100_000);
}

#[test]
fn held_does_not_claim_listening_before_capture_can_retain_input() {
    let mut h = Harness::new(Voice::Speaking);
    let Input::Progress(mut value) =
        h.progress_input(10_000, Voice::Listening, 2, true, VoiceEvent::State)
    else {
        unreachable!()
    };
    value.timing.accepting_input = false;
    h.send(Input::Progress(value));
    assert_eq!(
        h.engine.tick(20_000).light.unwrap().face,
        ExpressionFace::Unavailable
    );
}

#[test]
fn hold_drops_queued_and_delayed_old_sound_then_accepts_new_sound() {
    let mut h = Harness::new(Voice::Speaking);
    let queued = h.sound(10_000, 1, 150_000, 170_000);
    h.send(queued);
    h.progress(20_000, Voice::Listening, 2, true, VoiceEvent::State);
    assert_eq!(h.engine.queued_playout_frames(), 0);
    let while_held = h.sound(30_000, 2, 150_000, 170_000);
    assert!(h.engine.ingest(30_000, while_held.clone()).is_err());
    h.progress(50_000, Voice::Speaking, 2, false, VoiceEvent::State);
    assert!(h.engine.ingest(50_000, while_held).is_err());
    let old = h.sound(60_000, 1, 150_000, 170_000);
    assert!(h.engine.ingest(60_000, old).is_err());
    let before_resume = h.sound(60_000, 2, 40_000, 170_000);
    assert!(h.engine.ingest(60_000, before_resume).is_err());
    let fresh = h.sound(70_000, 2, 150_000, 170_000);
    h.send(fresh);
    let out = h.engine.tick(150_000);
    assert_eq!(out.light.as_ref().unwrap().face, ExpressionFace::Speaking);
    assert!(Harness::has(&out, Trigger::Playout));
    assert_eq!(h.engine.queued_playout_frames(), 1);
}

#[test]
fn delayed_hold_activity_cannot_reconstruct_a_pause_after_a_listening_resume() {
    let mut h = Harness::new(Voice::Speaking);
    h.progress(50_000, Voice::Listening, 2, true, VoiceEvent::State);
    let delayed: Vec<_> = [(60_000, true), (70_000, false)]
        .into_iter()
        .map(|(at, speaking)| {
            Input::Activity(PersonActivity {
                stamp: h.stamp(at),
                session_id: h.session.clone(),
                turn: h.turn,
                request: h.turn + 10,
                speaking,
            })
        })
        .collect();
    h.progress(100_000, Voice::Listening, 2, false, VoiceEvent::State);
    h.context(100_000);
    for (at, input) in [110_000, 120_000].into_iter().zip(delayed) {
        assert!(h.engine.ingest(at, input).is_err());
    }
    assert!(!Harness::has(&h.until(500_000), Trigger::Pause));
}

#[test]
fn missed_hold_is_fenced_by_the_next_active_generation() {
    let mut h = Harness::new(Voice::Speaking);
    let old = h.sound(10_000, 1, 100_000, 120_000);
    h.send(old);
    h.progress(20_000, Voice::Speaking, 2, false, VoiceEvent::State);
    assert_eq!(h.engine.queued_playout_frames(), 0);
    let old = h.sound(30_000, 1, 100_000, 120_000);
    assert!(h.engine.ingest(30_000, old).is_err());
    let fresh = h.sound(40_000, 2, 100_000, 120_000);
    h.send(fresh);
    assert!(Harness::has(&h.engine.tick(100_000), Trigger::Playout));
}

#[test]
fn a_missed_hold_may_first_be_observed_after_resume_while_still_listening() {
    let mut h = Harness::new(Voice::Speaking);
    h.progress(10_000, Voice::Listening, 2, false, VoiceEvent::State);
    // The higher generation proves the previous segment was discarded, even
    // when the first observed resume heartbeat still has no speech to play.
    h.progress(20_000, Voice::Thinking, 2, false, VoiceEvent::State);
    let fresh = h.sound(30_000, 2, 40_000, 60_000);
    h.send(fresh);
    assert!(Harness::has(&h.engine.tick(40_000), Trigger::Playout));
}

#[test]
fn rehold_requires_a_new_generation_and_regressions_do_not_mutate_state() {
    let mut h = Harness::new(Voice::Speaking);
    let invalid = h.progress_input(10_000, Voice::Listening, 1, true, VoiceEvent::State);
    assert!(h.engine.ingest(10_000, invalid).is_err());
    h.progress(20_000, Voice::Listening, 2, true, VoiceEvent::State);
    h.progress(30_000, Voice::Speaking, 2, false, VoiceEvent::State);
    for (at, generation) in [(40_000, 2), (50_000, 1)] {
        let invalid = h.progress_input(at, Voice::Listening, generation, true, VoiceEvent::State);
        assert!(h.engine.ingest(at, invalid).is_err());
        assert_eq!(
            h.engine.tick(at).light.unwrap().face,
            ExpressionFace::Speaking
        );
    }
    h.progress(60_000, Voice::Listening, 3, true, VoiceEvent::State);
    assert_eq!(
        h.engine.tick(60_000).light.unwrap().face,
        ExpressionFace::Listening
    );
}

#[test]
fn all_terminal_events_fence_every_later_generation() {
    for event in [
        VoiceEvent::Interrupted,
        VoiceEvent::Finished,
        VoiceEvent::Cancelled,
    ] {
        let mut h = Harness::new(Voice::Speaking);
        h.progress(10_000, Voice::Listening, 2, true, VoiceEvent::State);
        h.progress(20_000, Voice::Listening, 2, false, event);
        let revive = h.progress_input(30_000, Voice::Speaking, 3, false, VoiceEvent::State);
        assert!(h.engine.ingest(30_000, revive).is_err(), "{event:?}");
        let sound = h.sound(40_000, 2, 40_000, 60_000);
        assert!(h.engine.ingest(40_000, sound).is_err(), "{event:?}");
        assert_eq!(h.engine.queued_playout_frames(), 0);
    }
}

#[test]
fn expired_voice_cannot_resume_with_a_fresh_higher_generation() {
    let mut h = Harness::new(Voice::Speaking);
    h.progress(10_000, Voice::Listening, 2, true, VoiceEvent::State);
    assert!(h.engine.tick(510_000).light.is_none());
    let revive = h.progress_input(520_000, Voice::Speaking, 3, false, VoiceEvent::State);
    assert!(h.engine.ingest(520_000, revive).is_err());
    let sound = h.sound(520_000, 3, 530_000, 550_000);
    assert!(h.engine.ingest(520_000, sound).is_err());
}

#[test]
fn new_turn_can_reuse_generation_but_old_turn_sound_cannot_follow() {
    let mut h = Harness::new(Voice::Speaking);
    let old = h.sound(10_000, 1, 100_000, 120_000);
    h.send(old.clone());
    h.turn = 2;
    h.progress(20_000, Voice::Speaking, 1, false, VoiceEvent::State);
    assert_eq!(h.engine.queued_playout_frames(), 0);
    assert!(h.engine.ingest(20_000, old).is_err());
    let Input::Sound(mut delayed) = h.sound(25_000, 1, 100_000, 120_000) else {
        unreachable!()
    };
    delayed.playout.turn = 1;
    delayed.playout.request = 11;
    assert!(h.engine.ingest(25_000, Input::Sound(delayed)).is_err());
    let sound = h.sound(30_000, 1, 100_000, 120_000);
    h.send(sound);
    assert!(Harness::has(&h.engine.tick(100_000), Trigger::Playout));
}

#[test]
fn new_session_resets_stream_sequences_but_never_accepts_the_retired_session() {
    let mut h = Harness::new(Voice::Speaking);
    let old = h.sound(10_000, 1, 100_000, 120_000);
    h.send(old);
    h.session = "voice-two".into();
    h.sequence = 0;
    h.progress(20_000, Voice::Speaking, 1, false, VoiceEvent::State);
    let sound = h.sound(30_000, 1, 100_000, 120_000);
    h.send(sound);
    h.session = "voice-one".into();
    let old = h.progress_input(40_000, Voice::Speaking, 99, false, VoiceEvent::State);
    assert!(h.engine.ingest(40_000, old).is_err());
    let old = h.sound(50_000, 1, 120_000, 140_000);
    assert!(h.engine.ingest(50_000, old).is_err());
}

#[test]
fn late_join_held_then_resume_has_an_observation_floor() {
    let mut engine = Engine::new(catalogue(), true, CLOCK).unwrap();
    let mut h = Harness::new(Voice::Speaking);
    let held = h.progress_input(100_000, Voice::Listening, 8, true, VoiceEvent::State);
    engine.ingest(120_000, held).unwrap();
    let sound = h.sound(110_000, 8, 150_000, 170_000);
    assert!(engine.ingest(120_000, sound.clone()).is_err());
    let resume = h.progress_input(130_000, Voice::Speaking, 8, false, VoiceEvent::State);
    engine.ingest(140_000, resume).unwrap();
    assert!(engine.ingest(140_000, sound).is_err());
    let sound = h.sound(140_000, 8, 150_000, 170_000);
    engine.ingest(140_000, sound).unwrap();
}

#[test]
fn initial_active_progress_rejects_older_sound_even_when_its_generation_matches() {
    let mut engine = Engine::new(catalogue(), true, CLOCK).unwrap();
    let mut h = Harness::new(Voice::Speaking);
    let progress = h.progress_input(100_000, Voice::Speaking, 8, false, VoiceEvent::State);
    engine.ingest(120_000, progress).unwrap();
    let old = h.sound(90_000, 8, 150_000, 170_000);
    assert!(engine.ingest(120_000, old).is_err());
    let fresh = h.sound(100_000, 8, 150_000, 170_000);
    engine.ingest(120_000, fresh).unwrap();
    assert_eq!(engine.queued_playout_frames(), 1);
}

#[test]
fn held_heartbeats_and_final_interruption_do_not_restart_the_body_return() {
    let mut h = Harness::new(Voice::Thinking);
    h.until(260_000);
    h.progress(270_000, Voice::Listening, 2, true, VoiceEvent::State);
    let first = h.engine.tick(270_000).body.unwrap().offsets.base_yaw;
    h.step(370_000);
    let middle = h.engine.tick(370_000).body.unwrap().offsets.base_yaw;
    assert!(middle < first && middle > 0.0);
    h.progress(470_000, Voice::Listening, 2, false, VoiceEvent::Interrupted);
    h.context(470_000);
    h.progress(570_000, Voice::Listening, 2, false, VoiceEvent::Interrupted);
    h.context(570_000);
    let out = h.engine.tick(620_000);
    assert_eq!(out.body.unwrap().offsets, catalogue().postures.listening);
}

#[test]
fn hold_discards_the_previous_audio_schedule() {
    let mut h = Harness::new(Voice::Thinking);
    let Input::Progress(mut scheduled) =
        h.progress_input(10_000, Voice::Thinking, 1, false, VoiceEvent::Scheduled)
    else {
        unreachable!()
    };
    scheduled.timing.first_audio_us = Some(400_000);
    h.send(Input::Progress(scheduled));
    h.progress(20_000, Voice::Listening, 2, true, VoiceEvent::State);
    h.progress(30_000, Voice::Thinking, 2, false, VoiceEvent::State);
    assert!(
        h.engine
            .tick(30_000)
            .state
            .missing
            .contains(&"scheduled first audio".into())
    );
    let Input::Progress(mut scheduled) =
        h.progress_input(40_000, Voice::Thinking, 2, false, VoiceEvent::Scheduled)
    else {
        unreachable!()
    };
    scheduled.timing.first_audio_us = Some(600_000);
    h.send(Input::Progress(scheduled));
    assert!(
        !h.engine
            .tick(40_000)
            .state
            .missing
            .contains(&"scheduled first audio".into())
    );
}

#[test]
fn concentration_is_false_without_a_publisher_and_true_until_explicitly_cleared() {
    let mut h = Harness::new(Voice::Listening);
    h.activity(0, false);
    let out = h.engine.tick(0);
    assert!(out.state.missing.is_empty(), "{:?}", out.state.missing);
    assert!(!out.body.unwrap().hold);
    h.focus(10_000, true);
    let out = h.until(6_000_000);
    assert!(out.body.unwrap().hold);
    h.focus(6_000_000, false);
    assert!(!h.engine.tick(6_000_000).body.unwrap().hold);
}

#[test]
fn finishing_nod_starts_at_a_genuine_pause_and_continues_without_thinking_activity() {
    let mut h = started_nod();
    let out = h.until(1_000_000);
    assert!(Harness::has(&out, Trigger::Pause));
    assert!(out.body.unwrap().offsets.head_pitch > 0.0);
    assert!(!Harness::has(&h.until(1_540_001), Trigger::Pause));
}

#[test]
fn thinking_never_starts_a_nod_that_did_not_start_while_listening() {
    let mut h = Harness::new(Voice::Listening);
    h.activity(0, true);
    h.activity(100_000, false);
    h.progress(200_000, Voice::Thinking, 1, false, VoiceEvent::State);
    assert!(!Harness::has(&h.until(700_000), Trigger::Pause));
    let mut h = Harness::new(Voice::Listening);
    h.activity(0, false);
    assert!(!Harness::has(&h.until(600_000), Trigger::Pause));
}

#[test]
fn resumed_person_speech_cancels_a_carried_nod_without_restarting_it() {
    let mut h = started_nod();
    h.ready = false;
    h.step(650_000);
    h.activity(650_000, true);
    assert!(!Harness::has(&h.engine.tick(650_000), Trigger::Pause));
    assert!(!Harness::has(&h.until(1_000_000), Trigger::Pause));
}

#[test]
fn an_earlier_audio_schedule_returns_the_carried_nod_before_audio_without_delay() {
    for first in [720_000, 1_000_000] {
        let mut h = started_nod();
        let Input::Progress(mut scheduled) =
            h.progress_input(610_000, Voice::Thinking, 1, false, VoiceEvent::Scheduled)
        else {
            unreachable!()
        };
        scheduled.timing.first_audio_us = Some(1_500_000);
        h.send(Input::Progress(scheduled));
        let Input::Progress(mut corrected) =
            h.progress_input(620_000, Voice::Thinking, 1, false, VoiceEvent::Scheduled)
        else {
            unreachable!()
        };
        corrected.timing.first_audio_us = Some(first);
        h.send(Input::Progress(corrected));
        let out = h.until(first - 40_000);
        assert!(!Harness::has(&out, Trigger::Pause));
        assert_eq!(out.body.unwrap().offsets, catalogue().postures.speaking);
        h.progress(first, Voice::Speaking, 1, false, VoiceEvent::State);
        let sound = h.sound(first, 1, first, first + 20_000);
        h.send(sound);
        let out = h.engine.tick(first);
        assert_eq!(out.light.as_ref().unwrap().face, ExpressionFace::Speaking);
        assert!(Harness::has(&out, Trigger::Playout));
        assert!(!Harness::has(&out, Trigger::Pause));
    }
}

#[test]
fn malformed_out_of_order_and_wrong_clock_progress_never_clear_the_current_queue() {
    let mut h = Harness::new(Voice::Speaking);
    let sound = h.sound(10_000, 1, 100_000, 120_000);
    h.send(sound);
    let Input::Progress(mut wrong_clock) =
        h.progress_input(20_000, Voice::Listening, 2, true, VoiceEvent::State)
    else {
        unreachable!()
    };
    wrong_clock.timing.stamp.clock_id = "another-boot".into();
    assert!(
        h.engine
            .ingest(20_000, Input::Progress(wrong_clock))
            .is_err()
    );
    let Input::Progress(mut future) =
        h.progress_input(30_001, Voice::Listening, 2, true, VoiceEvent::State)
    else {
        unreachable!()
    };
    assert!(
        h.engine
            .ingest(30_000, Input::Progress(future.clone()))
            .is_err()
    );
    future.timing.stamp.at_us = 0;
    future.timing.stamp.sequence = 1;
    assert!(h.engine.ingest(40_000, Input::Progress(future)).is_err());
    assert_eq!(h.engine.queued_playout_frames(), 1);
    assert_eq!(
        h.engine.tick(40_000).light.unwrap().face,
        ExpressionFace::Speaking
    );
}

#[test]
fn generation_overflow_cannot_wrap_or_resurrect_old_sound() {
    let mut h = Harness::new(Voice::Speaking);
    h.progress(10_000, Voice::Speaking, u64::MAX, false, VoiceEvent::State);
    for generation in [0, 1, u64::MAX] {
        let invalid = h.progress_input(
            20_000,
            Voice::Listening,
            generation,
            true,
            VoiceEvent::State,
        );
        assert!(h.engine.ingest(20_000, invalid).is_err());
    }
    let valid = h.sound(30_000, u64::MAX, 30_000, 50_000);
    h.send(valid);
    assert!(Harness::has(&h.engine.tick(30_000), Trigger::Playout));
}

#[test]
fn an_input_inhibited_lamp_does_not_rest_and_waits_again_after_readiness_returns() {
    let mut h = Harness::new(Voice::Resting);
    h.until(46_000_000);
    h.ready = false;
    assert!(!Harness::has(&h.step(46_000_000), Trigger::Rest));
    assert!(!Harness::has(&h.until(146_000_000), Trigger::Rest));
    h.ready = true;
    h.step(146_000_000);
    assert!(!Harness::has(&h.until(190_999_999), Trigger::Rest));
    assert!(Harness::has(&h.step(191_000_000), Trigger::Rest));
}

#[test]
fn focus_busy_room_hold_and_speaking_cancel_carried_nods() {
    for reason in ["focus", "busy", "hold", "speaking", "new turn"] {
        let mut h = started_nod();
        match reason {
            "focus" => h.focus(650_000, true),
            "busy" => {
                h.room = Room::Busy;
                h.context(650_000);
            }
            "hold" => h.progress(650_000, Voice::Listening, 2, true, VoiceEvent::State),
            "speaking" => h.progress(650_000, Voice::Speaking, 1, false, VoiceEvent::State),
            "new turn" => {
                h.turn += 1;
                h.progress(650_000, Voice::Thinking, 1, false, VoiceEvent::State);
            }
            _ => unreachable!(),
        }
        assert!(
            !Harness::has(&h.engine.tick(650_000), Trigger::Pause),
            "{reason}"
        );
    }
}

#[test]
fn stale_joints_cancel_a_finishing_nod_even_after_feedback_recovers() {
    let mut h = started_nod();
    h.progress(800_000, Voice::Thinking, 1, false, VoiceEvent::State);
    assert!(h.engine.tick(900_000).body.is_none());
    h.context(920_000);
    assert!(!Harness::has(&h.engine.tick(920_000), Trigger::Pause));
}

#[test]
fn default_catalogue_has_no_unsolicited_rest_motion() {
    assert!(!Catalogue::builtin().resumable_voice);
    assert!(
        !Catalogue::builtin()
            .behaviours
            .iter()
            .any(|clip| clip.trigger == Trigger::Rest)
    );
}

#[test]
fn rest_waits_for_45_seconds_and_cooldown_starts_after_clip_completion() {
    let mut h = Harness::new(Voice::Resting);
    assert!(!Harness::has(&h.until(44_999_999), Trigger::Rest));
    let start = h.step(45_000_000);
    assert!(Harness::has(&start, Trigger::Rest));
    assert_eq!(start.body.unwrap().offsets, BodyOffsets::ZERO);
    assert!(Harness::has(&h.until(47_000_000), Trigger::Rest));
    assert!(!Harness::has(&h.until(49_000_000), Trigger::Rest));
    assert!(!Harness::has(&h.until(138_999_999), Trigger::Rest));
    assert!(Harness::has(&h.step(139_000_000), Trigger::Rest));
}

#[test]
fn rest_is_small_and_never_commands_yaw_or_the_arm() {
    let mut h = Harness::new(Voice::Resting);
    h.until(45_000_000);
    for at in (45_000_000..=49_000_000).step_by(TICK_US as usize) {
        let out = h.step(at);
        let body = out.body.unwrap();
        body.check().unwrap();
        assert_eq!(
            [
                body.offsets.base_yaw,
                body.offsets.shoulder,
                body.offsets.elbow
            ],
            [0.0; 3]
        );
        assert!(body.offsets.head_pitch.abs() <= 0.06);
        assert!(body.offsets.head_roll.abs() <= 0.06);
    }
}

#[test]
fn rest_aborts_for_busy_unknown_or_concentration_and_needs_new_quiet_time() {
    for reason in ["busy", "unknown", "focus"] {
        let mut h = Harness::new(Voice::Resting);
        h.until(46_000_000);
        match reason {
            "busy" => h.room = Room::Busy,
            "unknown" => h.room = Room::Unknown,
            "focus" => h.focus(46_000_000, true),
            _ => unreachable!(),
        }
        assert!(!Harness::has(&h.step(46_000_000), Trigger::Rest));
        assert!(!Harness::has(&h.until(96_000_000), Trigger::Rest));
        h.room = Room::Quiet;
        h.focus(96_000_000, false);
        h.step(96_000_000);
        assert!(
            !Harness::has(&h.until(140_999_999), Trigger::Rest),
            "{reason}"
        );
        assert!(
            Harness::has(&h.step(141_000_000), Trigger::Rest),
            "{reason}"
        );
    }
}

#[test]
fn rest_eligibility_does_not_accumulate_across_a_turn_or_input_gap() {
    for gap in [false, true] {
        let mut h = Harness::new(Voice::Resting);
        h.until(44_000_000);
        if gap {
            assert!(h.engine.tick(44_500_000).body.is_none());
        } else {
            h.turn += 1;
            h.progress(44_100_000, Voice::Listening, 1, false, VoiceEvent::State);
        }
        h.progress(44_600_000, Voice::Resting, 1, false, VoiceEvent::State);
        h.context(44_600_000);
        assert!(!Harness::has(&h.until(89_599_999), Trigger::Rest));
        assert!(Harness::has(&h.step(89_600_000), Trigger::Rest));
    }
}

#[test]
fn rest_stalls_never_emit_a_catchup_backlog_and_shadow_never_commands() {
    let mut h = Harness::configured(catalogue(), Voice::Resting, false);
    let out = h.until(45_000_000);
    assert!(Harness::has(&out, Trigger::Rest));
    assert!(out.body.is_none() && out.light.is_none());
    assert!(h.engine.tick(200_000_000).body.is_none());
    h.progress(200_000_000, Voice::Resting, 1, false, VoiceEvent::State);
    h.context(200_000_000);
    assert!(!Harness::has(&h.engine.tick(200_000_000), Trigger::Rest));
    assert!(!Harness::has(&h.until(244_999_999), Trigger::Rest));
}
