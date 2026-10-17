use lamp_expression::{Catalogue, Engine, Input, LEASE_US, MAX_PLAYOUT_FRAMES, Output, Trigger};
use lamp_msgs::{
    BodyOffsets, ClockStamp, Concentration, ExpressionFace, Face, Faces, JointState, Light,
    NameHeard, PersonActivity, Room, RoomState, Speech, Voice, VoiceEvent, VoicePlayout,
    VoiceState, VoiceTiming,
};

const CLOCK: &str = "test-boot";

fn stamp(at_us: u64, sequence: u64) -> ClockStamp {
    ClockStamp {
        clock_id: CLOCK.into(),
        at_us,
        sequence,
    }
}

fn voice(at_us: u64, sequence: u64, state: Voice, event: VoiceEvent) -> VoiceTiming {
    VoiceTiming {
        stamp: stamp(at_us, sequence),
        session_id: "voice-one".into(),
        turn: 1,
        request: 7,
        state,
        accepting_input: true,
        event,
        first_audio_us: None,
    }
}

fn joints(at_us: u64, sequence: u64) -> Input {
    Input::Joints {
        stamp: stamp(at_us, sequence),
        value: JointState {
            name: ["base_yaw", "shoulder", "elbow", "head_roll", "head_pitch"]
                .map(str::to_owned)
                .to_vec(),
            position: vec![0.5, 0.4, -0.3, -0.1, 0.6],
            commanded: vec![],
            velocity: vec![],
            effort: vec![],
        },
    }
}

fn activity(at_us: u64, sequence: u64, speaking: bool) -> Input {
    Input::Activity(PersonActivity {
        stamp: stamp(at_us, sequence),
        session_id: "voice-one".into(),
        turn: 1,
        request: 7,
        speaking,
    })
}

fn playout(at_us: u64, sequence: u64, start_us: u64, end_us: u64, level: f32) -> Input {
    Input::Playout(VoicePlayout {
        stamp: stamp(at_us, sequence),
        session_id: "voice-one".into(),
        turn: 1,
        request: 7,
        start_us,
        end_us,
        level,
    })
}

fn send(engine: &mut Engine, input: Input) {
    engine.ingest(input.stamp().at_us, input).unwrap();
}

fn configured(catalogue: Catalogue, enabled: bool) -> Engine {
    let mut engine = Engine::new(catalogue, enabled, CLOCK).unwrap();
    for input in [
        Input::Voice(voice(0, 1, Voice::Listening, VoiceEvent::State)),
        joints(0, 1),
        Input::Room {
            stamp: stamp(0, 1),
            value: RoomState { state: Room::Quiet },
        },
        Input::Light {
            stamp: stamp(0, 1),
            value: Light::new(true, "blue", 2).unwrap(),
        },
        Input::Concentration(Concentration {
            stamp: stamp(0, 1),
            concentrating: false,
        }),
    ] {
        send(&mut engine, input);
    }
    engine
}

fn engine() -> Engine {
    configured(Catalogue::builtin(), true)
}

fn heartbeat(engine: &mut Engine, at_us: u64, state: Voice) {
    send(
        engine,
        Input::Voice(voice(at_us, at_us + 2, state, VoiceEvent::State)),
    );
    send(engine, joints(at_us, at_us + 2));
}

fn has(output: &Output, name: &str) -> bool {
    output.state.behaviours.iter().any(|v| v == name)
}

fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-8, "{a} != {b}");
}

#[test]
fn shadow_runs_the_same_clips_but_emits_no_actuator_commands() {
    let mut shadow = configured(Catalogue::builtin(), false);
    heartbeat(&mut shadow, 20_000, Voice::Thinking);
    let planned = shadow.tick(200_000);
    assert!(has(&planned, "look away and back"));
    assert!(planned.body.is_none() && planned.light.is_none());
    assert!(!planned.state.enabled && !planned.state.body && !planned.state.light);
    assert_eq!(planned.state.check(), Ok(()));
}

#[test]
fn words_a_name_and_legacy_state_cannot_establish_timing_or_readiness() {
    let mut engine = Engine::new(Catalogue::builtin(), true, CLOCK).unwrap();
    let speech = Speech {
        text: "yes, Lamp".into(),
        finished: true,
        locale: None,
        request: Some(7),
    };
    for input in [
        Input::VoiceState {
            stamp: stamp(0, 1),
            value: VoiceState {
                state: Voice::Listening,
            },
        },
        Input::Heard {
            stamp: stamp(0, 1),
            value: speech.clone(),
        },
        Input::Said {
            stamp: stamp(0, 1),
            value: speech,
        },
        Input::Name {
            stamp: stamp(0, 1),
            value: NameHeard {
                score: 0.99,
                ago_ms: 0,
            },
        },
        joints(0, 1),
    ] {
        send(&mut engine, input);
    }
    let output = engine.tick(0);
    assert!(output.body.is_none() && output.light.is_none());
    assert!(output.state.missing.contains(&"fresh voice timing".into()));
    assert!(output.state.behaviours.is_empty());
}

#[test]
fn stale_joints_release_body_and_stale_voice_releases_both() {
    let mut engine = engine();
    let first = engine.tick(0);
    assert!(first.body.is_some() && first.light.is_some());
    let stale_joints = engine.tick(300_000);
    assert!(stale_joints.body.is_none() && stale_joints.light.is_some());
    assert!(
        stale_joints
            .state
            .missing
            .contains(&"fresh measured joints".into())
    );
    let stale_voice = engine.tick(500_000);
    assert!(stale_voice.body.is_none() && stale_voice.light.is_none());
    assert!(
        stale_voice
            .state
            .missing
            .contains(&"fresh voice timing".into())
    );
    assert!(!first.body.unwrap().lease.valid_at(CLOCK, LEASE_US));
}

#[test]
fn thinking_returns_its_requested_pose_before_scheduled_first_audio() {
    let mut engine = engine();
    heartbeat(&mut engine, 10_000, Voice::Thinking);
    heartbeat(&mut engine, 260_000, Voice::Thinking);
    near(engine.tick(260_000).body.unwrap().offsets.base_yaw, 0.065);
    let mut scheduled = voice(270_000, 270_002, Voice::Thinking, VoiceEvent::Scheduled);
    scheduled.first_audio_us = Some(800_000);
    send(&mut engine, Input::Voice(scheduled));
    heartbeat(&mut engine, 500_000, Voice::Thinking);
    let midway = engine.tick(635_000).body.unwrap().offsets.base_yaw;
    assert!(midway > 0.0 && midway < 0.065);
    heartbeat(&mut engine, 650_000, Voice::Thinking);
    let before_sound = engine.tick(760_000);
    near(before_sound.body.as_ref().unwrap().offsets.base_yaw, 0.0);
    assert!(!has(&before_sound, "look away and back"));
    assert!(
        before_sound
            .state
            .missing
            .contains(&"measured body lag".into())
    );
}

#[test]
fn short_notice_shrinks_the_remaining_look_and_reports_that_arrival_is_unverified() {
    let mut engine = engine();
    heartbeat(&mut engine, 10_000, Voice::Thinking);
    heartbeat(&mut engine, 260_000, Voice::Thinking);
    assert!(engine.tick(260_000).body.unwrap().offsets.base_yaw > 0.0);
    let mut scheduled = voice(270_000, 270_002, Voice::Thinking, VoiceEvent::Scheduled);
    scheduled.first_audio_us = Some(290_000);
    send(&mut engine, Input::Voice(scheduled));
    let output = engine.tick(270_000);
    near(output.body.unwrap().offsets.base_yaw, 0.0);
    assert!(
        output
            .state
            .warnings
            .iter()
            .any(|v| v.contains("short audio notice"))
    );
}

#[test]
fn an_expired_schedule_at_arrival_is_not_a_new_future_cue() {
    let mut engine = engine();
    let mut scheduled = voice(10_000, 2, Voice::Thinking, VoiceEvent::Scheduled);
    scheduled.first_audio_us = Some(20_000);
    assert!(engine.ingest(20_000, Input::Voice(scheduled)).is_err());
    assert!(!has(&engine.tick(20_000), "look away and back"));
}

#[test]
fn a_pause_requires_actual_speech_then_quiet_and_resumption_cancels_the_nod() {
    let mut engine = engine();
    send(&mut engine, activity(0, 1, false));
    assert!(!has(&engine.tick(120_000), "nod in a pause"));
    send(&mut engine, activity(130_000, 2, true));
    send(&mut engine, activity(140_000, 3, false));
    heartbeat(&mut engine, 250_000, Voice::Listening);
    assert!(!has(&engine.tick(259_999), "nod in a pause"));
    assert!(has(&engine.tick(260_000), "nod in a pause"));
    send(&mut engine, activity(300_000, 4, false));
    let stroke = engine.tick(410_000);
    near(stroke.body.unwrap().offsets.head_pitch, -0.025 + 0.035);
    send(&mut engine, activity(420_000, 5, true));
    let resumed = engine.tick(420_000);
    assert!(!has(&resumed, "nod in a pause"));
    near(resumed.body.unwrap().offsets.head_pitch, -0.025);
}

#[test]
fn pause_heartbeats_do_not_repeat_a_nod_or_bypass_its_cooldown() {
    let mut engine = engine();
    send(&mut engine, activity(0, 1, true));
    send(&mut engine, activity(20_000, 2, false));
    assert!(has(&engine.tick(140_000), "nod in a pause"));
    for at in (200_000..=4_000_000).step_by(100_000) {
        heartbeat(&mut engine, at, Voice::Listening);
        send(&mut engine, activity(at, at + 2, false));
        if at > 460_000 {
            assert!(!has(&engine.tick(at), "nod in a pause"));
        }
    }
    send(&mut engine, activity(4_010_000, 4_010_002, true));
    send(&mut engine, activity(4_020_000, 4_020_002, false));
    heartbeat(&mut engine, 4_140_000, Voice::Listening);
    assert!(has(&engine.tick(4_140_000), "nod in a pause"));
    send(&mut engine, activity(4_150_000, 4_150_002, true));
    send(&mut engine, activity(4_160_000, 4_160_002, false));
    assert!(!has(&engine.tick(4_280_000), "nod in a pause"));
}

#[test]
fn stale_activity_and_stalled_ticks_never_deliver_a_late_nod() {
    let mut engine = engine();
    send(&mut engine, activity(0, 1, true));
    send(&mut engine, activity(10_000, 2, false));
    heartbeat(&mut engine, 300_000, Voice::Listening);
    send(&mut engine, activity(300_000, 3, false));
    assert!(!has(&engine.tick(400_000), "nod in a pause"));
    heartbeat(&mut engine, 600_000, Voice::Listening);
    assert!(!has(&engine.tick(600_000), "nod in a pause"));
}

#[test]
fn actual_playout_is_half_open_and_a_cloud_state_never_fabricates_levels() {
    let mut engine = engine();
    heartbeat(&mut engine, 10_000, Voice::Speaking);
    send(&mut engine, playout(20_000, 1, 100_000, 200_000, 1.0));
    let before = engine.tick(99_999);
    assert!(!has(&before, "follow played sound"));
    let started = engine.tick(100_000);
    assert!(has(&started, "follow played sound"));
    assert!(started.light.unwrap().brightness > before.light.unwrap().brightness);
    let ended = engine.tick(200_000);
    assert!(!has(&ended, "follow played sound"));
    near(
        f64::from(ended.light.unwrap().brightness),
        f64::from(0.3f32),
    );
    assert!(
        ended
            .state
            .missing
            .contains(&"current actual playout level".into())
    );
    assert_eq!(engine.queued_playout_frames(), 0);
}

#[test]
fn measured_lag_advances_only_the_body_and_only_on_committed_playout() {
    let mut catalogue = Catalogue::builtin();
    catalogue.timing.body_lag_us = Some(100_000);
    let mut engine = configured(catalogue, true);
    heartbeat(&mut engine, 10_000, Voice::Speaking);
    send(&mut engine, playout(20_000, 1, 200_000, 300_000, 1.0));
    let before = engine.tick(100_000);
    assert!(has(&before, "follow played sound"));
    assert!(before.body.unwrap().offsets.head_pitch > -0.015);
    near(
        f64::from(before.light.unwrap().brightness),
        f64::from(0.3f32),
    );
    let sounding = engine.tick(200_000);
    assert!(sounding.light.unwrap().brightness > 0.3);
    assert!(!sounding.state.missing.contains(&"measured body lag".into()));
}

#[test]
fn interrupted_light_changes_first_and_body_blend_finishes_in_350ms_despite_heartbeats() {
    let mut engine = engine();
    heartbeat(&mut engine, 10_000, Voice::Thinking);
    heartbeat(&mut engine, 260_000, Voice::Thinking);
    let before = engine.tick(260_000).body.unwrap().offsets;
    send(
        &mut engine,
        Input::Voice(voice(
            270_000,
            270_002,
            Voice::Listening,
            VoiceEvent::Interrupted,
        )),
    );
    let first = engine.tick(270_000);
    assert_eq!(first.light.unwrap().face, ExpressionFace::Listening);
    assert_eq!(first.body.unwrap().offsets, before);
    send(
        &mut engine,
        Input::Voice(voice(
            370_000,
            370_002,
            Voice::Listening,
            VoiceEvent::Interrupted,
        )),
    );
    send(&mut engine, joints(370_000, 370_002));
    let middle = engine.tick(445_000).body.unwrap().offsets;
    near(middle.base_yaw, before.base_yaw / 2.0);
    send(
        &mut engine,
        Input::Voice(voice(
            570_000,
            570_002,
            Voice::Listening,
            VoiceEvent::Interrupted,
        )),
    );
    send(&mut engine, joints(570_000, 570_002));
    let complete = engine.tick(620_000).body.unwrap().offsets;
    assert_eq!(complete, Catalogue::builtin().postures.listening);
}

/// On the lamp the person who cuts in is heard at once as a turn of their
/// own, so the interruption and the next turn's first state arrive in the
/// same moment. The turn to them goes on into their turn; a voice that
/// has started again begins with nothing of it.
#[test]
fn the_turn_that_cut_the_lamp_off_does_not_end_the_yield_it_caused() {
    let mut engine = engine();
    heartbeat(&mut engine, 10_000, Voice::Thinking);
    heartbeat(&mut engine, 260_000, Voice::Thinking);
    let before = engine.tick(260_000).body.unwrap().offsets;
    assert!(before.base_yaw.abs() > 0.01, "the glance had begun");
    send(
        &mut engine,
        Input::Voice(voice(
            270_000,
            270_002,
            Voice::Listening,
            VoiceEvent::Interrupted,
        )),
    );
    let mut theirs = voice(270_000, 270_003, Voice::Listening, VoiceEvent::State);
    theirs.turn += 1;
    theirs.request += 1;
    send(&mut engine, Input::Voice(theirs.clone()));
    send(&mut engine, joints(270_000, 270_003));
    let first = engine.tick(270_000);
    assert!(has(&first, "yield when interrupted"), "{:?}", first.state);
    assert_eq!(first.body.unwrap().offsets, before);
    theirs.stamp = stamp(440_000, 440_000);
    send(&mut engine, Input::Voice(theirs.clone()));
    send(&mut engine, joints(440_000, 440_000));
    let middle = engine.tick(445_000).body.unwrap().offsets;
    near(middle.base_yaw, before.base_yaw / 2.0);
    theirs.stamp = stamp(600_000, 600_000);
    send(&mut engine, Input::Voice(theirs.clone()));
    send(&mut engine, joints(600_000, 600_000));
    let complete = engine.tick(620_000);
    assert_eq!(
        complete.body.unwrap().offsets,
        Catalogue::builtin().postures.listening
    );
    // That was its last step: the next one is their turn and nothing else.
    assert!(!has(&engine.tick(640_000), "yield when interrupted"));
    // The same from a voice that has started again: nothing is carried.
    let mut engine = self::engine();
    heartbeat(&mut engine, 10_000, Voice::Thinking);
    heartbeat(&mut engine, 260_000, Voice::Thinking);
    engine.tick(260_000);
    send(
        &mut engine,
        Input::Voice(voice(
            270_000,
            270_002,
            Voice::Listening,
            VoiceEvent::Interrupted,
        )),
    );
    let mut restarted = voice(280_000, 1, Voice::Listening, VoiceEvent::State);
    restarted.session_id = "voice-two".into();
    send(&mut engine, Input::Voice(restarted));
    send(&mut engine, joints(280_000, 280_003));
    assert!(!has(&engine.tick(280_000), "yield when interrupted"));
}

#[test]
fn interruption_must_not_show_listening_until_input_can_be_retained() {
    let mut engine = engine();
    let mut interrupted = voice(10_000, 2, Voice::Listening, VoiceEvent::Interrupted);
    interrupted.accepting_input = false;
    send(&mut engine, Input::Voice(interrupted));
    assert_eq!(
        engine.tick(10_000).light.unwrap().face,
        ExpressionFace::Unavailable
    );
    assert!(engine.ingest(20_000, activity(20_000, 1, true)).is_err());
}

#[test]
fn cancellation_fences_old_audio_old_turns_and_state_heartbeats() {
    let mut engine = engine();
    heartbeat(&mut engine, 10_000, Voice::Thinking);
    send(&mut engine, playout(20_000, 1, 100_000, 200_000, 1.0));
    send(
        &mut engine,
        Input::Voice(voice(30_000, 30_002, Voice::Resting, VoiceEvent::Cancelled)),
    );
    assert_eq!(engine.queued_playout_frames(), 0);
    assert!(
        engine
            .ingest(40_000, playout(40_000, 2, 100_000, 200_000, 1.0))
            .is_err()
    );
    assert!(
        engine
            .ingest(
                50_000,
                Input::Voice(voice(50_000, 50_002, Voice::Speaking, VoiceEvent::State))
            )
            .is_err()
    );
    assert!(engine.tick(60_000).body.is_none());
    let mut next = voice(70_000, 70_002, Voice::Listening, VoiceEvent::State);
    next.turn = 2;
    next.request = 8;
    send(&mut engine, Input::Voice(next));
    assert!(engine.tick(70_000).body.is_some());
    assert!(
        engine
            .ingest(
                80_000,
                Input::Voice(voice(80_000, 80_002, Voice::Speaking, VoiceEvent::State))
            )
            .is_err()
    );
}

#[test]
fn session_restart_clears_queue_and_rejects_even_a_newly_stamped_retired_session() {
    let mut engine = engine();
    heartbeat(&mut engine, 10_000, Voice::Speaking);
    send(&mut engine, playout(20_000, 1, 100_000, 200_000, 1.0));
    let mut restarted = voice(30_000, 1, Voice::Listening, VoiceEvent::State);
    restarted.session_id = "voice-two".into();
    send(&mut engine, Input::Voice(restarted));
    assert_eq!(engine.queued_playout_frames(), 0);
    assert!(
        engine
            .ingest(40_000, playout(40_000, 2, 100_000, 200_000, 1.0))
            .is_err()
    );
    assert!(
        engine
            .ingest(
                50_000,
                Input::Voice(voice(50_000, 99, Voice::Speaking, VoiceEvent::State))
            )
            .is_err()
    );
    let out = engine.tick(50_000);
    assert_eq!(out.body.unwrap().lease.voice_session_id, "voice-two");
}

#[test]
fn wrong_clock_rejects_only_the_input_but_clock_regression_latches_safe_stop() {
    let mut engine = engine();
    let mut wrong = voice(10_000, 2, Voice::Thinking, VoiceEvent::State);
    wrong.stamp.clock_id = "other-boot".into();
    assert!(engine.ingest(10_000, Input::Voice(wrong)).is_err());
    assert!(engine.tick(20_000).body.is_some());
    let backwards = engine.tick(19_999);
    assert!(backwards.body.is_none() && backwards.light.is_none());
    assert!(
        backwards
            .state
            .warnings
            .iter()
            .any(|v| v.contains("clock moved backwards"))
    );
    assert!(engine.tick(30_000).body.is_none());
    assert!(
        engine
            .ingest(
                40_000,
                Input::Voice(voice(40_000, 3, Voice::Listening, VoiceEvent::State))
            )
            .is_err()
    );
}

#[test]
fn duplicate_sequence_backdated_input_and_future_stamp_do_not_change_the_pose() {
    let mut engine = engine();
    heartbeat(&mut engine, 10_000, Voice::Thinking);
    let old = voice(9_999, 99_000, Voice::Speaking, VoiceEvent::State);
    assert!(engine.ingest(20_000, Input::Voice(old)).is_err());
    let duplicate = voice(20_000, 10_002, Voice::Speaking, VoiceEvent::State);
    assert!(engine.ingest(20_000, Input::Voice(duplicate)).is_err());
    let future = voice(30_001, 99_001, Voice::Speaking, VoiceEvent::State);
    assert!(engine.ingest(30_000, Input::Voice(future)).is_err());
    assert!(has(&engine.tick(40_000), "look away and back"));
    assert_eq!(engine.rejected_inputs(), 3);
}

#[test]
fn future_playout_queue_is_bounded_and_expired_frames_cannot_reappear() {
    let mut engine = engine();
    heartbeat(&mut engine, 10_000, Voice::Speaking);
    for index in 0..MAX_PLAYOUT_FRAMES as u64 {
        send(
            &mut engine,
            playout(
                10_000,
                index + 1,
                100_000 + index * 2,
                100_001 + index * 2,
                0.2,
            ),
        );
    }
    assert_eq!(engine.queued_playout_frames(), MAX_PLAYOUT_FRAMES);
    assert!(
        engine
            .ingest(10_000, playout(10_000, 200, 101_000, 102_000, 0.2))
            .is_err()
    );
    engine.tick(200_000);
    assert_eq!(engine.queued_playout_frames(), 0);
    assert!(
        engine
            .ingest(200_000, playout(199_999, 201, 100_000, 100_020, 0.2))
            .is_err()
    );
    assert!(!has(&engine.tick(200_000), "follow played sound"));
}

#[test]
fn explicit_concentration_holds_actual_pose_and_remains_a_setting_across_silence() {
    let mut engine = engine();
    heartbeat(&mut engine, 10_000, Voice::Speaking);
    send(&mut engine, playout(20_000, 1, 20_000, 120_000, 1.0));
    assert!(has(&engine.tick(20_000), "follow played sound"));
    send(
        &mut engine,
        Input::Concentration(Concentration {
            stamp: stamp(30_000, 2),
            concentrating: true,
        }),
    );
    let still = engine.tick(30_000);
    assert!(has(&still, "stay still for work"));
    assert!(!has(&still, "follow played sound"));
    let body = still.body.unwrap();
    assert!(body.hold && body.gaze.is_none());
    assert_eq!(body.offsets, BodyOffsets::ZERO);
    near(
        f64::from(still.light.unwrap().brightness),
        f64::from(0.3f32),
    );
    let mut next = voice(6_000_000, 2, Voice::Listening, VoiceEvent::State);
    next.session_id = "voice-two".into();
    send(&mut engine, Input::Voice(next));
    send(&mut engine, joints(6_000_000, 6_000_002));
    assert!(engine.tick(6_000_000).body.unwrap().hold);
    // The setting publisher may restart its sequence, but only a newer
    // monotonic observation can release the stillness setting.
    send(
        &mut engine,
        Input::Concentration(Concentration {
            stamp: stamp(6_000_010, 1),
            concentrating: false,
        }),
    );
    assert!(!engine.tick(6_000_010).body.unwrap().hold);
}

#[test]
fn persistent_settings_can_be_read_long_after_they_were_published() {
    let mut engine = Engine::new(Catalogue::builtin(), true, CLOCK).unwrap();
    engine
        .ingest(
            10_000_000,
            Input::Concentration(Concentration {
                stamp: stamp(0, 99),
                concentrating: true,
            }),
        )
        .unwrap();
    engine
        .ingest(
            10_000_000,
            Input::Light {
                stamp: stamp(0, 99),
                value: Light::new(false, "red", 1).unwrap(),
            },
        )
        .unwrap();
    send(
        &mut engine,
        Input::Voice(voice(10_000_000, 1, Voice::Listening, VoiceEvent::State)),
    );
    send(&mut engine, joints(10_000_000, 1));
    let out = engine.tick(10_000_000);
    assert!(out.body.unwrap().hold);
    assert_eq!(out.light.unwrap().brightness, 0.0);
    assert!(
        engine
            .ingest(
                10_000_000,
                Input::Concentration(Concentration {
                    stamp: stamp(0, 98),
                    concentrating: false
                })
            )
            .is_err()
    );
}

#[test]
fn busy_room_restrains_unsolicited_motion_but_never_infers_concentration() {
    let mut engine = engine();
    send(
        &mut engine,
        Input::Room {
            stamp: stamp(10_000, 2),
            value: RoomState { state: Room::Busy },
        },
    );
    send(&mut engine, activity(20_000, 1, true));
    send(&mut engine, activity(30_000, 2, false));
    let out = engine.tick(150_000);
    assert!(!has(&out, "nod in a pause") && !has(&out, "stay still for work"));
    assert!(!out.body.unwrap().hold);
}

#[test]
fn largest_fresh_face_is_a_normalized_target_without_invented_geometry() {
    let mut engine = engine();
    let face = |id: &str, min: f32, max: f32| Face {
        speaking: None,
        id: id.into(),
        xmin: min,
        ymin: 0.1,
        xmax: max,
        ymax: 0.7,
        confidence: 0.9,
        yaw: None,
        facing: None,
    };
    send(
        &mut engine,
        Input::Faces {
            stamp: stamp(10_000, 1),
            value: Faces {
                faces: vec![face("small", 0.3, 0.4), face("large", 0.5, 0.9)],
            },
        },
    );
    let body = engine.tick(10_000).body.unwrap();
    let gaze = body.gaze.unwrap();
    assert_eq!(gaze.face_id, "large");
    assert!((gaze.x - 0.4).abs() < 1e-6 && (gaze.y + 0.2).abs() < 1e-6);
    assert_eq!(body.offsets, Catalogue::builtin().postures.listening);
    heartbeat(&mut engine, 400_000, Voice::Listening);
    assert!(engine.tick(510_000).body.unwrap().gaze.is_none());
}

#[test]
fn layers_add_then_clamp_and_joint_feedback_never_accumulates_an_offset() {
    let mut catalogue = Catalogue::builtin();
    let away = catalogue
        .behaviours
        .iter_mut()
        .find(|b| b.trigger == Trigger::Thinking)
        .unwrap();
    away.keyframes.last_mut().unwrap().offsets.base_yaw = 0.3;
    let mut another = away.clone();
    another.name = "another authored look".into();
    catalogue.behaviours.push(another);
    let mut engine = configured(catalogue, true);
    heartbeat(&mut engine, 10_000, Voice::Thinking);
    heartbeat(&mut engine, 260_000, Voice::Thinking);
    let first = engine.tick(260_000);
    near(first.body.unwrap().offsets.base_yaw, 0.1);
    let mut moved = joints(280_000, 280_002);
    if let Input::Joints { value, .. } = &mut moved {
        value.position.iter_mut().for_each(|v| *v += 0.5);
    }
    send(&mut engine, moved);
    near(engine.tick(280_000).body.unwrap().offsets.base_yaw, 0.1);
}

#[test]
fn malformed_catalogues_and_nonfinite_measurements_are_refused() {
    let mut catalogue = Catalogue::builtin();
    catalogue.limits.base_yaw = f64::NAN;
    assert!(catalogue.check().is_err());
    let mut value = serde_json::to_value(Catalogue::builtin()).unwrap();
    value["typo"] = true.into();
    assert!(Catalogue::from_json(&value.to_string()).is_err());
    let mut catalogue = Catalogue::builtin();
    catalogue.behaviours[1].cooldown_us = 1;
    assert!(catalogue.check().is_err());
    let mut catalogue = Catalogue::builtin();
    catalogue.behaviours[1].keyframes[1].at_us = 0;
    assert!(catalogue.check().is_err());
    let mut catalogue = Catalogue::builtin();
    catalogue.behaviours[4].keyframes[1].offsets.base_yaw = 0.1;
    assert!(catalogue.check().is_err());
    let mut invalid = joints(0, 1);
    if let Input::Joints { value, .. } = &mut invalid {
        value.position[0] = f64::INFINITY;
    }
    assert!(invalid.check().is_err());
    if let Input::Joints { value, .. } = &mut invalid {
        value.position[0] = 0.0;
        value.name[0] = "head_pitch".into();
    }
    assert!(invalid.check().is_err());
    assert!(playout(0, 1, 0, 20_000, f32::NAN).check().is_err());
}

#[test]
fn extreme_clock_values_fail_closed_without_panicking_or_wrapping_a_lease() {
    let mut engine = engine();
    let out = engine.tick(u64::MAX);
    assert!(out.body.is_none() && out.light.is_none());
    assert!(out.state.warnings.iter().any(|v| v.contains("lease")));
}

#[test]
fn generated_messages_always_pass_their_final_boundary_checks() {
    let mut engine = engine().with_session_id("planner-one");
    assert!(
        engine.is_err(),
        "a session cannot change after ingesting input"
    );
    engine = Ok(self::engine());
    let mut engine = engine.unwrap();
    for at in (0..=400_000).step_by(20_000) {
        if at != 0 {
            heartbeat(&mut engine, at, Voice::Listening);
        }
        let out = engine.tick(at);
        out.state.check().unwrap();
        let body = out.body.unwrap();
        body.check().unwrap();
        assert!(body.lease.valid_at(CLOCK, at));
        assert!(!body.lease.valid_at("other", at));
        assert!(body.lease.valid_until_us - body.lease.issued_us <= LEASE_US);
        out.light.unwrap().check().unwrap();
    }
}

#[test]
fn cancelled_is_sticky_through_finished_acknowledgement_and_state_heartbeat() {
    let mut engine = engine();
    heartbeat(&mut engine, 10_000, Voice::Speaking);
    send(
        &mut engine,
        Input::Voice(voice(20_000, 20_002, Voice::Resting, VoiceEvent::Cancelled)),
    );
    send(
        &mut engine,
        Input::Voice(voice(30_000, 30_002, Voice::Resting, VoiceEvent::Finished)),
    );
    send(
        &mut engine,
        Input::Voice(voice(40_000, 40_002, Voice::Listening, VoiceEvent::State)),
    );
    let out = engine.tick(40_000);
    assert!(out.body.is_none() && out.light.is_none());
    assert!(
        engine
            .ingest(50_000, playout(50_000, 1, 50_000, 70_000, 1.0))
            .is_err()
    );
}

#[test]
fn repeated_interruption_through_state_heartbeat_preserves_the_original_blend() {
    let mut engine = engine();
    heartbeat(&mut engine, 10_000, Voice::Thinking);
    heartbeat(&mut engine, 260_000, Voice::Thinking);
    engine.tick(260_000);
    send(
        &mut engine,
        Input::Voice(voice(
            270_000,
            270_002,
            Voice::Listening,
            VoiceEvent::Interrupted,
        )),
    );
    engine.tick(270_000);
    heartbeat(&mut engine, 400_000, Voice::Listening);
    engine.tick(400_000);
    send(
        &mut engine,
        Input::Voice(voice(
            500_000,
            500_002,
            Voice::Listening,
            VoiceEvent::Interrupted,
        )),
    );
    send(&mut engine, joints(500_000, 500_002));
    assert_eq!(
        engine.tick(620_000).body.unwrap().offsets,
        Catalogue::builtin().postures.listening
    );
}

#[test]
fn a_later_false_heartbeat_cannot_turn_expired_speech_into_a_pause() {
    let mut engine = engine();
    send(&mut engine, activity(0, 1, true));
    for at in (100_000..=2_000_000).step_by(100_000) {
        heartbeat(&mut engine, at, Voice::Listening);
    }
    send(&mut engine, activity(2_000_000, 2, false));
    assert!(!has(&engine.tick(2_120_000), "nod in a pause"));
    send(&mut engine, activity(2_130_000, 3, true));
    send(&mut engine, activity(2_140_000, 4, false));
    assert!(has(&engine.tick(2_260_000), "nod in a pause"));
}

#[test]
fn a_stale_voice_cannot_revive_queued_audio_by_renewing_the_same_turn() {
    let mut engine = engine();
    heartbeat(&mut engine, 10_000, Voice::Speaking);
    send(&mut engine, playout(20_000, 1, 700_000, 800_000, 1.0));
    assert!(engine.tick(510_000).body.is_none());
    assert_eq!(engine.queued_playout_frames(), 0);
    assert!(
        engine
            .ingest(
                520_000,
                Input::Voice(voice(520_000, 520_002, Voice::Speaking, VoiceEvent::State))
            )
            .is_err()
    );
    assert!(
        engine
            .ingest(530_000, playout(530_000, 2, 700_000, 800_000, 1.0))
            .is_err()
    );
}

/// A lamp restarts its voice at every update and after a fault: lamp-4ace
/// passed 32 sessions in a day (17 Oct), and the fence that then refused every
/// new session left Expression stopped until it was restarted. The newest
/// retired sessions stay refused; the oldest are forgotten.
#[test]
fn many_voice_restarts_keep_expression_running_and_a_recent_session_retired() {
    let mut engine = engine();
    let sessions = lamp_expression::MAX_VOICE_SESSIONS as u64 + 8;
    for index in 1..=sessions {
        let mut next = voice(index, 1, Voice::Listening, VoiceEvent::State);
        next.session_id = format!("voice-{index}");
        send(&mut engine, Input::Voice(next));
    }
    let out = engine.tick(sessions + 1);
    assert!(engine.fault().is_none(), "{:?}", engine.fault());
    assert!(
        !out.state
            .warnings
            .iter()
            .any(|v| v.contains("session fence"))
    );
    let mut stale = voice(sessions + 2, 1, Voice::Listening, VoiceEvent::State);
    stale.session_id = format!("voice-{}", sessions - 1);
    assert!(engine.ingest(sessions + 2, Input::Voice(stale)).is_err());
    let mut next = voice(sessions + 3, 1, Voice::Listening, VoiceEvent::State);
    next.session_id = "voice-after-many".into();
    send(&mut engine, Input::Voice(next));
    assert!(engine.fault().is_none());
    // The first session is forgotten by now, but a late or replayed message of
    // its own, with its old stamp, is still refused by the order mark.
    let mut replayed = voice(1, 1, Voice::Listening, VoiceEvent::State);
    replayed.session_id = "voice-1".into();
    assert!(engine.ingest(sessions + 4, Input::Voice(replayed)).is_err());
    assert!(engine.fault().is_none());
}

#[test]
fn catalogue_compatible_extra_clips_need_no_engine_changes() {
    let mut catalogue = Catalogue::builtin();
    let mut quiet_nod = catalogue
        .behaviours
        .iter()
        .find(|b| b.trigger == Trigger::Pause)
        .unwrap()
        .clone();
    quiet_nod.name = "small extra nod".into();
    quiet_nod.weight = 0.2;
    catalogue.behaviours.push(quiet_nod);
    let json = serde_json::to_string(&catalogue).unwrap();
    let mut engine = configured(Catalogue::from_json(&json).unwrap(), true);
    send(&mut engine, activity(0, 1, true));
    send(&mut engine, activity(10_000, 2, false));
    let out = engine.tick(130_000);
    assert!(has(&out, "nod in a pause") && has(&out, "small extra nod"));
}

/// #830: while the lamp cannot reach the internet, listening is not drawn
/// as ready; online again, it is, at once.
#[test]
fn offline_listening_is_not_drawn_as_ready() {
    let mut engine = engine();
    heartbeat(&mut engine, 10_000, Voice::Thinking);
    let listen = |engine: &mut Engine, at: u64| {
        send(
            engine,
            Input::Voice(voice(at, at + 2, Voice::Listening, VoiceEvent::Interrupted)),
        );
        engine.tick(at).light.unwrap().face
    };
    engine.set_offline(true);
    assert_eq!(listen(&mut engine, 270_000), ExpressionFace::Unavailable);
    engine.set_offline(false);
    assert_eq!(listen(&mut engine, 370_000), ExpressionFace::Listening);
}
