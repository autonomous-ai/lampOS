use lamp_expression::{Catalogue, Engine, Error, Input, MAX_PLAYOUT_FRAMES, Output, Trigger};
use lamp_msgs::{
    BodyOffsets, ClockStamp, Concentration, ExpressionFace, JointState, Light, Room, RoomState,
    Voice, VoiceEvent, VoicePlayout, VoiceProgress, VoiceSound, VoiceTiming,
};

const WINDOW: u64 = 100_000;
const CLOCK: &str = "recent-level-test";
const MISSING: &str = "fresh current or recent playout level";

struct Harness {
    engine: Engine,
    sequence: u64,
    turn: u64,
    session: String,
    generation: u64,
    resumable: bool,
}

impl Harness {
    fn new(window: u64, lag: u64, resumable: bool) -> Self {
        let mut catalogue = Catalogue::builtin();
        catalogue.timing.recent_playout_us = window;
        catalogue.timing.body_lag_us = Some(lag);
        catalogue.resumable_voice = resumable;
        // Constant authored increments isolate the level from animation phase.
        let clip = catalogue
            .behaviours
            .iter_mut()
            .find(|c| c.trigger == Trigger::Playout)
            .unwrap();
        for frame in &mut clip.keyframes {
            frame.offsets = BodyOffsets {
                head_pitch: 0.04,
                ..BodyOffsets::ZERO
            };
            frame.brightness = 0.4;
        }
        Self::configured(catalogue)
    }

    fn configured(catalogue: Catalogue) -> Self {
        let resumable = catalogue.resumable_voice;
        let mut h = Self {
            engine: Engine::new(catalogue, true, CLOCK).unwrap(),
            sequence: 0,
            turn: 1,
            session: "voice-one".into(),
            generation: 1,
            resumable,
        };
        h.voice(0, Voice::Speaking, VoiceEvent::State, false);
        h.context(0, Room::Quiet);
        let stamp = h.stamp(0);
        h.engine
            .ingest(
                0,
                Input::Light {
                    stamp,
                    value: Light::new(true, "blue", 2).unwrap(),
                },
            )
            .unwrap();
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

    fn voice_input(&mut self, at: u64, state: Voice, event: VoiceEvent, held: bool) -> Input {
        let timing = VoiceTiming {
            stamp: self.stamp(at),
            session_id: self.session.clone(),
            turn: self.turn,
            request: self.turn + 10,
            state,
            accepting_input: true,
            event,
            first_audio_us: None,
        };
        if self.resumable {
            Input::Progress(VoiceProgress {
                timing,
                audio_generation: self.generation,
                held,
            })
        } else {
            Input::Voice(timing)
        }
    }

    fn voice(&mut self, at: u64, state: Voice, event: VoiceEvent, held: bool) {
        let input = self.voice_input(at, state, event, held);
        self.engine.ingest(at, input).unwrap();
    }

    fn context(&mut self, at: u64, room: Room) {
        let stamp = self.stamp(at);
        self.engine
            .ingest(
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
            )
            .unwrap();
        let stamp = self.stamp(at);
        self.engine
            .ingest(
                at,
                Input::Room {
                    stamp,
                    value: RoomState { state: room },
                },
            )
            .unwrap();
    }

    fn frame(&mut self, stamp: u64, start: u64, end: u64, level: f32) -> Input {
        let playout = VoicePlayout {
            stamp: self.stamp(stamp),
            session_id: self.session.clone(),
            turn: self.turn,
            request: self.turn + 10,
            start_us: start,
            end_us: end,
            level,
        };
        if self.resumable {
            Input::Sound(VoiceSound {
                playout,
                audio_generation: self.generation,
            })
        } else {
            Input::Playout(playout)
        }
    }

    fn receive(
        &mut self,
        arrival: u64,
        stamp: u64,
        start: u64,
        end: u64,
        level: f32,
    ) -> Result<(), Error> {
        let frame = self.frame(stamp, start, end, level);
        self.engine.ingest(arrival, frame)
    }

    fn brightness(&mut self, now: u64) -> f32 {
        self.engine.tick(now).light.unwrap().brightness
    }
}

fn near(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.00001,
        "{actual} != {expected}"
    );
}

fn follows(output: &Output) -> bool {
    output
        .state
        .behaviours
        .iter()
        .any(|name| name == "follow played sound")
}

#[test]
fn a_twenty_ms_piece_published_after_onset_is_heard_when_read_just_after_end() {
    let mut h = Harness::new(WINDOW, 0, false);
    // Source timing from First's report, with a written receiver delay. This
    // test is not a new acoustic or bus measurement from the lamp.
    h.receive(121_700, 101_700, 100_000, 120_000, 0.8).unwrap();
    let out = h.engine.tick(121_700);
    assert!(follows(&out));
    assert!(out.state.warnings.is_empty());
    assert!(!out.state.missing.contains(&MISSING.into()));
    near(out.light.unwrap().brightness, 0.3 + 0.4 * 0.8 * 0.983);
    assert_eq!(h.engine.queued_playout_frames(), 0);
    assert_eq!(h.engine.rejected_inputs(), 0);
}

#[test]
fn default_zero_preserves_end_rejection_and_half_open_playback() {
    let mut h = Harness::new(0, 0, false);
    assert!(h.receive(120_000, 101_700, 100_000, 120_000, 0.8).is_err());
    h.receive(130_000, 130_000, 140_000, 160_000, 1.0).unwrap();
    near(h.brightness(159_999), 0.7);
    let out = h.engine.tick(160_000);
    near(out.light.unwrap().brightness, 0.3);
    assert!(
        out.state
            .missing
            .contains(&"current actual playout level".into())
    );
    assert_eq!(h.engine.queued_playout_frames(), 0);
}

#[test]
fn exact_end_is_observed_but_exact_recent_expiry_is_excluded() {
    let mut h = Harness::new(WINDOW, 0, false);
    h.receive(120_000, 101_700, 100_000, 120_000, 1.0).unwrap();
    near(h.brightness(120_000), 0.7);
    near(h.brightness(170_000), 0.5);
    assert!(h.brightness(219_999) > 0.3);
    let out = h.engine.tick(220_000);
    near(out.light.unwrap().brightness, 0.3);
    assert!(out.state.missing.contains(&MISSING.into()));
    let mut expired = Harness::new(WINDOW, 0, false);
    assert!(
        expired
            .receive(220_000, 220_000, 100_000, 120_000, 1.0)
            .is_err()
    );
}

#[test]
fn receipt_delay_does_not_extend_the_original_end_deadline() {
    let mut h = Harness::new(WINDOW, 0, false);
    h.receive(200_000, 150_000, 100_000, 120_000, 1.0).unwrap();
    near(h.brightness(200_000), 0.38);
    near(h.brightness(220_000), 0.3);
}

#[test]
fn fresh_stamps_cannot_rejuvenate_old_intervals_and_fresh_ends_cannot_rescue_stale_stamps() {
    let mut h = Harness::new(WINDOW, 0, false);
    assert!(h.receive(250_000, 250_000, 100_000, 120_000, 1.0).is_err());
    assert!(h.receive(250_000, 50_000, 230_000, 250_000, 1.0).is_err());
    assert!(!follows(&h.engine.tick(250_000)));
}

#[test]
fn an_interval_received_in_time_becomes_recent_when_the_queue_is_pruned() {
    let mut h = Harness::new(WINDOW, 0, false);
    h.receive(101_700, 101_700, 100_000, 120_000, 0.5).unwrap();
    assert_eq!(h.engine.queued_playout_frames(), 1);
    near(h.brightness(110_000), 0.5);
    near(h.brightness(140_000), 0.46);
    assert_eq!(h.engine.queued_playout_frames(), 0);
    assert_eq!(h.engine.rejected_inputs(), 0);
}

#[test]
fn a_missed_tick_uses_only_the_latest_ended_interval_and_never_catches_up() {
    let mut h = Harness::new(WINDOW, 0, false);
    for (start, level) in [(100_000, 1.0), (120_000, 0.6), (140_000, 0.2)] {
        h.receive(20_000, 20_000, start, start + 20_000, level)
            .unwrap();
    }
    near(h.brightness(170_000), 0.372);
    assert_eq!(h.engine.queued_playout_frames(), 0);
    near(h.brightness(260_000), 0.3);
    near(h.brightness(270_000), 0.3);
}

#[test]
fn late_batched_observations_replace_rather_than_accumulate_levels() {
    let mut h = Harness::new(WINDOW, 0, false);
    h.receive(180_000, 101_700, 100_000, 120_000, 1.0).unwrap();
    h.receive(180_000, 121_700, 120_000, 140_000, 0.5).unwrap();
    h.receive(180_000, 141_700, 140_000, 160_000, 0.1).unwrap();
    near(h.brightness(180_000), 0.332);
    assert_eq!(h.engine.queued_playout_frames(), 0);
}

#[test]
fn a_newer_zero_is_known_silence_and_does_not_fall_back_to_an_older_peak() {
    let mut h = Harness::new(WINDOW, 0, false);
    h.receive(125_000, 101_700, 100_000, 120_000, 1.0).unwrap();
    h.receive(145_000, 121_700, 120_000, 140_000, 0.0).unwrap();
    let out = h.engine.tick(150_000);
    near(out.light.unwrap().brightness, 0.3);
    assert!(!out.state.missing.contains(&MISSING.into()));
    assert!(!follows(&h.engine.tick(239_999)));
    assert!(
        h.engine
            .tick(240_000)
            .state
            .missing
            .contains(&MISSING.into())
    );
}

#[test]
fn a_current_zero_or_level_always_takes_precedence_over_recent_sound() {
    for level in [0.0, 0.25] {
        let mut h = Harness::new(WINDOW, 0, false);
        h.receive(120_000, 101_700, 100_000, 120_000, 1.0).unwrap();
        h.receive(125_000, 125_000, 130_000, 150_000, level)
            .unwrap();
        near(h.brightness(140_000), 0.3 + 0.4 * level);
    }
}

#[test]
fn a_future_interval_neither_refreshes_recent_sound_nor_lights_up_early() {
    let mut h = Harness::new(WINDOW, 0, false);
    h.receive(120_000, 101_700, 100_000, 120_000, 1.0).unwrap();
    h.receive(150_000, 150_000, 300_000, 320_000, 0.5).unwrap();
    near(h.brightness(220_000), 0.3);
    near(h.brightness(299_999), 0.3);
    h.context(300_000, Room::Quiet);
    near(h.brightness(300_000), 0.5);
}

#[test]
fn committed_future_zero_wins_for_body_while_light_follows_recent_sound() {
    let mut h = Harness::new(WINDOW, 100_000, false);
    h.receive(120_000, 101_700, 100_000, 120_000, 1.0).unwrap();
    h.receive(125_000, 125_000, 230_000, 250_000, 0.0).unwrap();
    let out = h.engine.tick(130_000);
    near(out.light.unwrap().brightness, 0.66);
    assert!((out.body.unwrap().offsets.head_pitch + 0.015).abs() < 1e-9);
}

#[test]
fn lag_without_committed_future_uses_current_level_at_now() {
    let mut h = Harness::new(WINDOW, 300_000, false);
    h.receive(101_700, 101_700, 100_000, 120_000, 0.5).unwrap();
    let out = h.engine.tick(110_000);
    near(out.light.unwrap().brightness, 0.5);
    assert!((out.body.unwrap().offsets.head_pitch - 0.005).abs() < 1e-8);
}

#[test]
fn reactive_recent_body_sampling_does_not_advance_animation_by_motor_lag() {
    let mut catalogue = Catalogue::builtin();
    catalogue.timing.body_lag_us = Some(300_000);
    catalogue.timing.recent_playout_us = WINDOW;
    let clip = catalogue
        .behaviours
        .iter_mut()
        .find(|c| c.trigger == Trigger::Playout)
        .unwrap();
    clip.keyframes.truncate(2);
    clip.keyframes[0].offsets = BodyOffsets::ZERO;
    clip.keyframes[1].at_us = 1_000_000;
    clip.keyframes[1].offsets = BodyOffsets {
        head_pitch: 0.04,
        ..BodyOffsets::ZERO
    };
    let mut h = Harness::configured(catalogue);
    h.receive(125_000, 101_700, 100_000, 120_000, 1.0).unwrap();
    let actual = h.engine.tick(130_000).body.unwrap().offsets.head_pitch;
    // Current phase is 30 ms into the clip; a 300 ms motor lag must not turn
    // already-ended evidence into a requested 330 ms animation phase.
    let share = 0.03_f64;
    let expected = -0.015 + 0.04 * share * share * (3.0 - 2.0 * share) * 0.9;
    assert!((actual - expected).abs() < 1e-8, "{actual} != {expected}");
}

#[test]
fn late_history_never_promotes_thinking_or_invents_first_audio() {
    let mut h = Harness::new(WINDOW, 300_000, false);
    h.voice(10_000, Voice::Thinking, VoiceEvent::State, false);
    h.receive(125_000, 101_700, 100_000, 120_000, 1.0).unwrap();
    let out = h.engine.tick(130_000);
    assert_eq!(out.light.unwrap().face, ExpressionFace::Thinking);
    assert!(out.state.missing.contains(&"scheduled first audio".into()));
    assert!(!follows(&h.engine.tick(130_000)));
}

#[test]
fn phase_edges_clear_history_but_keep_committed_future_intervals() {
    let mut h = Harness::new(WINDOW, 0, false);
    h.receive(120_000, 101_700, 100_000, 120_000, 1.0).unwrap();
    h.receive(125_000, 125_000, 180_000, 200_000, 0.5).unwrap();
    h.voice(130_000, Voice::Thinking, VoiceEvent::State, false);
    h.voice(140_000, Voice::Speaking, VoiceEvent::State, false);
    assert_eq!(h.engine.queued_playout_frames(), 1);
    near(h.brightness(140_000), 0.3);
    near(h.brightness(180_000), 0.5);
}

#[test]
fn a_future_interval_admitted_promptly_can_outlive_its_publication_ttl() {
    let mut h = Harness::new(WINDOW, 0, false);
    h.receive(10_000, 10_000, 350_000, 370_000, 0.5).unwrap();
    h.voice(250_000, Voice::Speaking, VoiceEvent::State, false);
    h.context(250_000, Room::Quiet);
    near(h.brightness(350_000), 0.5);
    near(h.brightness(380_000), 0.48);
}

#[test]
fn repeated_ticks_and_voice_heartbeats_never_renew_recent_evidence() {
    let mut h = Harness::new(WINDOW, 0, false);
    h.receive(121_700, 101_700, 100_000, 120_000, 1.0).unwrap();
    for at in [130_000, 150_000, 190_000, 220_000] {
        h.voice(at, Voice::Speaking, VoiceEvent::State, false);
        near(
            h.brightness(at),
            0.3 + 0.4 * (1.0 - (at - 120_000) as f32 / WINDOW as f32),
        );
    }
    assert!(
        h.engine
            .tick(220_000)
            .state
            .missing
            .contains(&MISSING.into())
    );
}

#[test]
fn a_long_stalled_tick_drops_all_past_levels_without_a_delayed_gesture() {
    let mut h = Harness::new(WINDOW, 0, false);
    h.receive(10_000, 10_000, 100_000, 120_000, 1.0).unwrap();
    let out = h.engine.tick(250_000);
    near(out.light.unwrap().brightness, 0.3);
    assert_eq!(h.engine.queued_playout_frames(), 0);
    assert!(!follows(&h.engine.tick(260_000)));
}

#[test]
fn hold_resume_clears_history_and_rejects_delayed_old_generation_or_held_sound() {
    let mut h = Harness::new(WINDOW, 0, true);
    h.receive(120_000, 101_700, 100_000, 120_000, 1.0).unwrap();
    let old = h.frame(130_000, 120_000, 140_000, 1.0);
    h.generation = 2;
    h.voice(130_000, Voice::Listening, VoiceEvent::State, true);
    let during = h.frame(140_000, 140_000, 160_000, 1.0);
    assert_eq!(
        h.engine.tick(140_000).light.unwrap().face,
        ExpressionFace::Listening
    );
    h.voice(170_000, Voice::Speaking, VoiceEvent::State, false);
    assert!(h.engine.ingest(175_000, old).is_err());
    assert!(h.engine.ingest(175_000, during).is_err());
    near(h.brightness(175_000), 0.3);
    h.receive(191_700, 181_700, 180_000, 190_000, 0.5).unwrap();
    assert!(h.brightness(200_000) > 0.3);
}

#[test]
fn a_missed_hold_generation_clears_recent_sound_without_requiring_the_held_heartbeat() {
    let mut h = Harness::new(WINDOW, 0, true);
    h.receive(120_000, 101_700, 100_000, 120_000, 1.0).unwrap();
    assert!(h.brightness(120_000) > 0.3);
    let old = h.frame(130_000, 120_000, 140_000, 1.0);
    h.generation = 2;
    h.voice(130_000, Voice::Speaking, VoiceEvent::State, false);
    near(h.brightness(130_000), 0.3);
    assert!(h.engine.ingest(150_000, old).is_err());
    near(h.brightness(150_000), 0.3);
    h.receive(161_700, 141_700, 140_000, 160_000, 0.5).unwrap();
    near(h.brightness(170_000), 0.48);
}

#[test]
fn stale_joints_stop_body_while_recent_light_remains_independent() {
    let mut h = Harness::new(WINDOW, 0, false);
    h.receive(301_000, 281_700, 280_000, 300_000, 0.5).unwrap();
    let out = h.engine.tick(301_000);
    assert!(out.body.is_none());
    assert!(out.state.missing.contains(&"fresh measured joints".into()));
    near(out.light.unwrap().brightness, 0.498);
    h.context(330_000, Room::Quiet);
    let out = h.engine.tick(330_000);
    assert!(out.body.is_some());
    near(out.light.unwrap().brightness, 0.44);
}

#[test]
fn every_terminal_event_clears_recent_evidence_and_retires_the_turn() {
    for resumable in [false, true] {
        for event in [
            VoiceEvent::Interrupted,
            VoiceEvent::Cancelled,
            VoiceEvent::Finished,
        ] {
            let mut h = Harness::new(WINDOW, 0, resumable);
            h.receive(120_000, 101_700, 100_000, 120_000, 1.0).unwrap();
            h.voice(130_000, Voice::Listening, event, false);
            assert!(!follows(&h.engine.tick(130_000)));
            assert!(h.receive(150_000, 141_700, 140_000, 150_000, 1.0).is_err());
            let revive = h.voice_input(160_000, Voice::Speaking, VoiceEvent::State, false);
            assert!(h.engine.ingest(160_000, revive).is_err());
        }
    }
}

#[test]
fn stale_voice_clears_recent_evidence_and_cannot_be_revived_by_same_turn_sound() {
    let mut h = Harness::new(WINDOW, 0, false);
    h.context(450_000, Room::Quiet);
    h.receive(490_000, 481_700, 480_000, 490_000, 1.0).unwrap();
    assert!(h.engine.tick(500_000).light.is_none());
    assert!(h.receive(510_000, 501_700, 500_000, 510_000, 1.0).is_err());
    let revive = h.voice_input(520_000, Voice::Speaking, VoiceEvent::State, false);
    assert!(h.engine.ingest(520_000, revive).is_err());
}

#[test]
fn new_turn_and_session_clear_levels_and_preserve_the_old_identity_fence() {
    for new_session in [false, true] {
        let mut h = Harness::new(WINDOW, 0, true);
        h.receive(120_000, 101_700, 100_000, 120_000, 1.0).unwrap();
        let old = h.frame(130_000, 120_000, 140_000, 1.0);
        if new_session {
            h.session = "voice-two".into();
            h.sequence = 0;
        } else {
            h.turn += 1;
        }
        h.voice(140_000, Voice::Speaking, VoiceEvent::State, false);
        near(h.brightness(140_000), 0.3);
        assert!(h.engine.ingest(150_000, old).is_err());
        h.receive(171_700, 161_700, 160_000, 170_000, 0.5).unwrap();
        assert!(h.brightness(180_000) > 0.3);
    }
}

#[test]
fn concentration_and_busy_room_still_suppress_recent_modulation() {
    for concentrating in [false, true] {
        let mut h = Harness::new(WINDOW, 0, false);
        h.receive(120_000, 101_700, 100_000, 120_000, 1.0).unwrap();
        if concentrating {
            let stamp = h.stamp(130_000);
            h.engine
                .ingest(
                    130_000,
                    Input::Concentration(Concentration {
                        stamp,
                        concentrating: true,
                    }),
                )
                .unwrap();
        } else {
            h.context(130_000, Room::Busy);
        }
        let out = h.engine.tick(130_000);
        near(out.light.unwrap().brightness, 0.3);
        assert!(!follows(&h.engine.tick(130_000)));
    }
}

#[test]
fn ended_intervals_still_obey_nonoverlap_source_order_and_watermarks() {
    let mut h = Harness::new(WINDOW, 0, false);
    h.receive(150_000, 101_700, 100_000, 120_000, 0.5).unwrap();
    assert!(h.receive(160_000, 151_000, 119_999, 140_000, 1.0).is_err());
    assert!(h.receive(160_000, 100_000, 120_000, 140_000, 1.0).is_err());
    let mut duplicate = h.frame(151_000, 120_000, 140_000, 1.0);
    let Input::Playout(ref mut frame) = duplicate else {
        unreachable!()
    };
    frame.stamp.sequence = 1;
    assert!(h.engine.ingest(160_000, duplicate).is_err());
    h.receive(170_000, 151_000, 120_000, 140_000, 0.25).unwrap();
    near(h.brightness(170_000), 0.37);
    // Pruning the recent slot does not reset the strict audio interval floor.
    h.engine.tick(240_000);
    assert!(h.receive(250_000, 250_000, 139_999, 240_000, 1.0).is_err());
}

#[test]
fn the_future_queue_bound_is_unchanged_and_recent_storage_is_not_a_backlog() {
    let mut h = Harness::new(WINDOW, 0, false);
    h.receive(20_000, 10_000, 1_000, 20_000, 1.0).unwrap();
    for index in 0..MAX_PLAYOUT_FRAMES as u64 {
        h.receive(
            30_000,
            30_000,
            100_000 + index * 2,
            100_001 + index * 2,
            0.1,
        )
        .unwrap();
    }
    assert_eq!(h.engine.queued_playout_frames(), MAX_PLAYOUT_FRAMES);
    assert!(h.receive(30_000, 30_000, 101_000, 101_001, 0.1).is_err());
    near(
        h.brightness(110_000),
        0.3 + 0.4 * 0.1 * (1.0 - 9_745.0 / WINDOW as f32),
    );
    assert_eq!(h.engine.queued_playout_frames(), 0);
}

#[test]
fn backwards_and_near_overflow_clocks_remain_fail_closed() {
    let mut h = Harness::new(WINDOW, 0, false);
    h.receive(120_000, 101_700, 100_000, 120_000, 1.0).unwrap();
    assert!(h.engine.tick(119_999).light.is_none());
    assert!(h.engine.tick(130_000).body.is_none());
    let mut h = Harness::new(WINDOW, 0, false);
    assert!(h.engine.tick(u64::MAX - 1).light.is_none());
}
