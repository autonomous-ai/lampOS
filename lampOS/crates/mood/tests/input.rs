use lamp_bus::{Declaration, Header, Kind, Message};
use lamp_mood::{
    engine::{Engine, Event, EventKind, FaceTarget, Reject},
    input::{
        INPUTS, InputAdapter, InputError, MAX_PAYLOAD_BYTES, MAX_REMINDER_IDENTITIES,
        MAX_RETIRED_PUBLISHERS,
    },
};
use lamp_msgs::{
    AirFeel, AirState, ClockStamp, ControlGesture, ControlIntent, ControlSource, ControlsState,
    Face, Faces, Gesture, HoldTier, NameHeard, ScheduleDue, ScheduleNotice, Speech, SuddenSound,
    Voice, VoiceState,
};

const NOW: u64 = 10_000_000;
const WALL: u64 = 1_791_974_400_000;
const BOOT: &str = "1111111111111111";
const OTHER: &str = "2222222222222222";
const CLOCK: &str = "the-current-os-boot";

fn adapter() -> InputAdapter {
    InputAdapter::new(CLOCK).unwrap()
}

fn message(index: usize, value: &impl serde::Serialize) -> Message {
    let spec = INPUTS[index];
    Message {
        header: Header {
            topic: spec.topic.into(),
            kind: spec.kind,
            enc: "json".into(),
            type_name: Some(spec.type_name.into()),
            source: spec.source.into(),
            boot: BOOT.into(),
            seq: 1,
            // This deliberately differs from the shared machine clock.
            at_us: 7,
            at_unix_ms: WALL,
            expires_ms: None,
        },
        payload: serde_json::to_vec(value).unwrap(),
        gap: 0,
        publisher_restarted: false,
        expired: false,
    }
}

fn stamp() -> ClockStamp {
    ClockStamp {
        clock_id: CLOCK.into(),
        at_us: NOW,
        sequence: 1,
    }
}

fn gesture(kind: bool) -> ControlGesture {
    ControlGesture {
        stamp: stamp(),
        session_id: BOOT.into(),
        source: if kind {
            ControlSource::Headpad
        } else {
            ControlSource::Button
        },
        name: if kind { "head" } else { "button" }.into(),
        gesture: if kind { Gesture::Pet } else { Gesture::Tap },
        intent: if kind {
            ControlIntent::Pet
        } else {
            ControlIntent::Stop
        },
        duration_us: 100_000,
        released_us: NOW - 20_000,
        enabled: true,
    }
}

fn controls() -> ControlsState {
    ControlsState {
        stamp: stamp(),
        session_id: BOOT.into(),
        enabled: true,
        ready: true,
        fault: None,
        buttons: Vec::new(),
        head_active: false,
        base_ready: false,
        base_held_us: None,
        base_tier: HoldTier::None,
        latest_stop: None,
        latest_mute: None,
        event_sequence: 0,
        rejected_inputs: 0,
        privacy: None,
    }
}

fn face(id: &str) -> Face {
    Face {
        speaking: None,
        id: id.into(),
        xmin: 0.2,
        ymin: 0.2,
        xmax: 0.6,
        ymax: 0.6,
        confidence: 0.9,
        yaw: None,
        facing: None,
    }
}

fn air(co2: Option<u16>) -> AirState {
    AirState {
        pm1_0: None,
        pm2_5: None,
        pm4_0: None,
        pm10: None,
        humidity: None,
        temperature: None,
        co2_ppm: co2,
        feel: AirFeel::of_co2(co2),
    }
}

fn due(id: u64) -> ScheduleDue {
    ScheduleDue {
        stamp: stamp(),
        store_id: "a".repeat(32),
        revision: id,
        enabled: true,
        ready: true,
        pending: 1,
        waiting_for_clock: 0,
        oldest: Some(ScheduleNotice {
            id,
            delivery: 1,
            due_unix_ms: (WALL - 1_000) as i64,
            late_by_ms: Some(1_000),
            late: false,
        }),
    }
}

fn name() -> NameHeard {
    NameHeard {
        score: 0.95,
        ago_ms: 100,
    }
}

fn newer(mut message: Message, sequence: u64, elapsed_ms: u64) -> Message {
    message.header.seq = sequence;
    message.header.at_unix_ms += elapsed_ms;
    message
}

#[test]
fn every_input_requires_the_exact_current_declaration() {
    for spec in INPUTS {
        let mut declared = Declaration::new(spec.type_name, spec.version, spec.kind);
        assert!(spec.accepts_declaration(&declared));
        declared.version += 1;
        assert!(!spec.accepts_declaration(&declared));
        declared.version = spec.version;
        declared.kind = match spec.kind {
            Kind::Event => Kind::Latest,
            Kind::Latest => Kind::Event,
        };
        assert!(!spec.accepts_declaration(&declared));
        declared.kind = spec.kind;
        declared.type_name = "lamp_msgs/AFutureType".into();
        assert!(!spec.accepts_declaration(&declared));
    }
}

#[test]
fn malformed_envelopes_are_rejected_before_payload_parsing() {
    let good = message(7, &name());
    let mutations: [fn(&mut Message); 13] = [
        |m| m.header.source = "someone_else".into(),
        |m| m.header.kind = Kind::Latest,
        |m| m.header.enc = "none".into(),
        |m| m.header.type_name = None,
        |m| m.header.type_name = Some("lamp_msgs/Speech".into()),
        |m| m.header.boot = String::new(),
        |m| m.header.boot = "G".repeat(16),
        |m| m.header.boot = "a".repeat(17),
        |m| m.header.seq = 0,
        |m| m.header.at_unix_ms = 0,
        |m| m.header.expires_ms = Some(0),
        |m| m.payload = vec![b' '; MAX_PAYLOAD_BYTES + 1],
        |m| m.payload = vec![0xff; MAX_PAYLOAD_BYTES + 1],
    ];
    for change in mutations {
        let mut bad = good.clone();
        change(&mut bad);
        assert_eq!(
            adapter().ingest(&bad, NOW, WALL),
            Err(InputError::InvalidHeader)
        );
    }
    let mut bad = good;
    bad.header.topic = "voice/said".into();
    assert_eq!(
        adapter().ingest(&bad, NOW, WALL),
        Err(InputError::UnknownTopic)
    );
}

#[test]
fn legacy_timestamps_use_strict_wall_age_not_private_at_us() {
    let mut heard = message(7, &name());
    heard.header.at_us = u64::MAX;
    heard.header.at_unix_ms -= 200;
    let events = adapter().ingest(&heard, NOW, WALL).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].observed_us, NOW - 300_000);
    assert_eq!(events[0].valid_until_us, NOW + 1_200_000);
    assert_eq!(events[0].kind, EventKind::Name);

    heard.header.at_unix_ms = WALL + 1;
    assert_eq!(adapter().ingest(&heard, NOW, WALL), Err(InputError::Future));
    heard.header.at_unix_ms = WALL - 1_400;
    assert_eq!(adapter().ingest(&heard, NOW, WALL), Err(InputError::Stale));
    heard.header.at_unix_ms = WALL;
    heard.expired = true;
    assert_eq!(adapter().ingest(&heard, NOW, WALL), Err(InputError::Stale));
}

#[test]
fn explicit_expiry_shortens_but_never_extends_the_authored_cap() {
    let mut state = message(6, &air(Some(900)));
    state.header.expires_ms = Some(u64::MAX);
    let events = adapter().ingest(&state, NOW, WALL).unwrap();
    assert_eq!(events[0].valid_until_us, NOW + 15_000_000);
    state.header.expires_ms = Some(10);
    assert_eq!(
        adapter().ingest(&state, NOW + 10_000, WALL + 10),
        Err(InputError::Stale)
    );
    let events = adapter().ingest(&state, NOW + 9_000, WALL + 9).unwrap();
    assert_eq!(events[0].valid_until_us, NOW + 10_000);
}

#[test]
fn header_order_and_publisher_restarts_are_fenced_without_history_eviction() {
    let mut input = adapter();
    let first = message(7, &name());
    input.ingest(&first, NOW, WALL).unwrap();
    assert_eq!(input.ingest(&first, NOW, WALL), Err(InputError::OutOfOrder));
    let next = newer(first.clone(), 2, 1);
    input.ingest(&next, NOW + 1_000, WALL + 1).unwrap();
    let mut unknown_old = newer(first.clone(), 1, 0);
    unknown_old.header.boot = OTHER.into();
    assert_eq!(
        input.ingest(&unknown_old, NOW + 1_000, WALL + 1),
        Err(InputError::OutOfOrder)
    );

    for restart in 1..=MAX_RETIRED_PUBLISHERS {
        let mut message = newer(first.clone(), 1, restart as u64 + 1);
        message.header.boot = format!("{restart:016x}");
        input
            .ingest(
                &message,
                NOW + (restart as u64 + 1) * 1_000,
                WALL + restart as u64 + 1,
            )
            .unwrap();
    }
    let mut retired = newer(first.clone(), 100, 100);
    assert_eq!(
        input.ingest(&retired, NOW + 100_000, WALL + 100),
        Err(InputError::RetiredPublisher)
    );
    retired.header.boot = "ffffffffffffffff".into();
    assert_eq!(
        input.ingest(&retired, NOW + 100_000, WALL + 100),
        Err(InputError::PublisherFenceFull)
    );
    retired.header.boot = format!("{:016x}", MAX_RETIRED_PUBLISHERS);
    assert_eq!(
        input.ingest(&retired, NOW + 100_000, WALL + 100),
        Err(InputError::PublisherFenceFull)
    );
}

#[test]
fn invalid_replacements_do_not_retire_the_healthy_publisher() {
    let mut input = adapter();
    let first = message(7, &name());
    input.ingest(&first, NOW, WALL).unwrap();
    let mut stale = name();
    stale.ago_ms = 1_500;
    let mut replacement = newer(message(7, &stale), 1, 1);
    replacement.header.boot = OTHER.into();
    assert_eq!(
        input.ingest(&replacement, NOW + 1_000, WALL + 1),
        Err(InputError::Stale)
    );
    input
        .ingest(&newer(first, 2, 2), NOW + 2_000, WALL + 2)
        .unwrap();

    let mut input = adapter();
    input.ingest(&message(3, &controls()), NOW, WALL).unwrap();
    let mut invalid_owner = controls();
    invalid_owner.stamp.at_us += 1_000;
    let mut replacement = newer(message(3, &invalid_owner), 1, 1);
    replacement.header.boot = OTHER.into();
    assert_eq!(
        input.ingest(&replacement, NOW + 1_000, WALL + 1),
        Err(InputError::InvalidPayload)
    );
    let mut healthy = controls();
    healthy.stamp.at_us += 2_000;
    healthy.stamp.sequence = 2;
    input
        .ingest(&newer(message(3, &healthy), 2, 2), NOW + 2_000, WALL + 2)
        .unwrap();
}

#[test]
fn every_topic_has_an_independent_bounded_publisher_fence() {
    let mut input = adapter();
    input.ingest(&message(7, &name()), NOW, WALL).unwrap();
    input
        .ingest(
            &message(
                0,
                &VoiceState {
                    state: Voice::Thinking,
                },
            ),
            NOW,
            WALL,
        )
        .unwrap();
    input.ingest(&message(6, &air(None)), NOW, WALL).unwrap();
    assert_eq!(input.counts().accepted, 3);
}

#[test]
fn clock_regression_fails_closed_without_recovering_on_later_messages() {
    let mut input = adapter();
    let first = message(7, &name());
    input.ingest(&first, NOW, WALL).unwrap();
    assert_eq!(
        input.ingest(&newer(first.clone(), 2, 1), NOW - 1, WALL + 1),
        Err(InputError::ClockReset)
    );
    assert_eq!(
        input.ingest(&newer(first, 3, 2), NOW + 2_000, WALL + 2),
        Err(InputError::ClockReset)
    );
    assert!(InputAdapter::new("").is_err());
    assert!(InputAdapter::new("x".repeat(129)).is_err());
}

#[test]
fn speech_is_only_activity_and_no_words_or_request_are_retained() {
    let mut input = adapter();
    let private = Speech {
        text: "PRIVATE PERSON NEVER RETAIN THESE WORDS".into(),
        finished: false,
        locale: Some("en_US".into()),
        request: Some(4_242_424_242),
    };
    let events = input.ingest(&message(1, &private), NOW, WALL).unwrap();
    assert_eq!(events[0].kind, EventKind::Heard);
    let retained = format!("{input:?} {events:?}");
    assert!(!retained.contains("PRIVATE"));
    assert!(!retained.contains("4242424242"));
    assert!(!retained.contains("en_US"));
    let empty = Speech {
        text: " \n\t".into(),
        ..private
    };
    assert!(
        adapter()
            .ingest(&message(1, &empty), NOW, WALL)
            .unwrap()
            .is_empty()
    );
    let mut malformed = message(1, &empty);
    malformed.payload = br#"{"text":42,"finished":true}"#.to_vec();
    assert_eq!(
        adapter().ingest(&malformed, NOW, WALL),
        Err(InputError::InvalidPayload)
    );
}

#[test]
fn listening_and_resting_are_not_evidence_of_company() {
    for (state, company) in [
        (Voice::Resting, false),
        (Voice::Listening, false),
        (Voice::Thinking, true),
        (Voice::Speaking, true),
    ] {
        let events = adapter()
            .ingest(&message(0, &VoiceState { state }), NOW, WALL)
            .unwrap();
        assert_eq!(events[0].kind, EventKind::Voice(company));
    }
}

#[test]
fn face_choice_is_stable_scoped_to_publisher_and_cleared_on_empty() {
    let mut input = adapter();
    let first = message(
        2,
        &Faces {
            faces: vec![face("z"), face("m")],
        },
    );
    let chosen = input.ingest(&first, NOW, WALL).unwrap();
    assert_eq!(
        chosen[0].kind,
        EventKind::Faces(Some(FaceTarget {
            publisher_boot: BOOT.into(),
            id: "m".into(),
        }))
    );
    let second = newer(
        message(
            2,
            &Faces {
                faces: vec![face("a"), face("m")],
            },
        ),
        2,
        1,
    );
    let chosen = input.ingest(&second, NOW + 1_000, WALL + 1).unwrap();
    assert!(matches!(&chosen[0].kind, EventKind::Faces(Some(face)) if face.id == "m"));
    let mut restarted = newer(
        message(
            2,
            &Faces {
                faces: vec![face("a"), face("m")],
            },
        ),
        1,
        2,
    );
    restarted.header.boot = OTHER.into();
    let chosen = input.ingest(&restarted, NOW + 2_000, WALL + 2).unwrap();
    assert_eq!(
        chosen[0].kind,
        EventKind::Faces(Some(FaceTarget {
            publisher_boot: OTHER.into(),
            id: "a".into(),
        }))
    );
    let mut empty = newer(message(2, &Faces { faces: Vec::new() }), 2, 3);
    empty.header.boot = OTHER.into();
    assert_eq!(
        input.ingest(&empty, NOW + 3_000, WALL + 3).unwrap()[0].kind,
        EventKind::Faces(None)
    );
    assert!(!format!("{input:?}").contains("FaceTarget"));
}

#[test]
fn idle_expiry_forgets_the_current_face_and_stale_input_does_not_preserve_it() {
    let mut input = adapter();
    let first = message(
        2,
        &Faces {
            faces: vec![face("ephemeral-person")],
        },
    );
    input.ingest(&first, NOW, WALL).unwrap();
    input.expire(NOW + 2_000_000);
    assert!(!format!("{input:?}").contains("ephemeral-person"));
    let mut input = adapter();
    input.ingest(&first, NOW, WALL).unwrap();
    assert_eq!(
        input.ingest(&newer(first, 2, 0), NOW + 2_000_000, WALL + 2_000),
        Err(InputError::Stale)
    );
    assert!(!format!("{input:?}").contains("ephemeral-person"));
}

#[test]
fn faces_reject_duplicate_long_invalid_or_unbounded_lists() {
    for faces in [
        vec![face("a"); 17],
        vec![face("a"), face("a")],
        vec![face("")],
        vec![face(&"x".repeat(65))],
        vec![Face {
            confidence: 1.1,
            ..face("a")
        }],
        vec![Face {
            xmin: 0.8,
            ..face("a")
        }],
    ] {
        assert_eq!(
            adapter().ingest(&message(2, &Faces { faces }), NOW, WALL),
            Err(InputError::InvalidPayload)
        );
    }
}

#[test]
fn gestures_need_a_fresh_ready_enabled_matching_controls_owner() {
    let mut input = adapter();
    let mut event = message(4, &gesture(false));
    event.header.boot = OTHER.into();
    assert_eq!(
        input.ingest(&event, NOW, WALL),
        Err(InputError::NoControlsState)
    );
    let mut input = adapter();
    input.ingest(&message(3, &controls()), NOW, WALL).unwrap();
    let events = input.ingest(&event, NOW, WALL).unwrap();
    assert_eq!(events[0].kind, EventKind::Touch { kind: false });
    assert_eq!(events[0].observed_us, NOW - 20_000);

    let mut stale = adapter();
    stale.ingest(&message(3, &controls()), NOW, WALL).unwrap();
    assert_eq!(
        stale.ingest(&event, NOW + 300_000, WALL + 300),
        Err(InputError::NoControlsState)
    );
    let mut shadow_state = controls();
    shadow_state.enabled = false;
    let mut shadow = adapter();
    shadow
        .ingest(&message(3, &shadow_state), NOW, WALL)
        .unwrap();
    assert_eq!(
        shadow.ingest(&event, NOW, WALL),
        Err(InputError::NoControlsState)
    );

    let mut wrong_session = gesture(false);
    wrong_session.session_id = OTHER.into();
    let mut input = adapter();
    input.ingest(&message(3, &controls()), NOW, WALL).unwrap();
    assert_eq!(
        input.ingest(&message(4, &wrong_session), NOW, WALL),
        Err(InputError::NoControlsState)
    );
}

#[test]
fn controls_state_owner_and_shared_clock_must_match() {
    let mut state = controls();
    state.session_id = OTHER.into();
    assert_eq!(
        adapter().ingest(&message(3, &state), NOW, WALL),
        Err(InputError::InvalidPayload)
    );
    state = controls();
    state.stamp.clock_id = "previous-os-boot".into();
    assert_eq!(
        adapter().ingest(&message(3, &state), NOW, WALL),
        Err(InputError::WrongClock)
    );
    state = controls();
    state.stamp.at_us = NOW + 1;
    assert_eq!(
        adapter().ingest(&message(3, &state), NOW, WALL),
        Err(InputError::Future)
    );
    state.stamp.at_us = NOW - 300_000;
    assert_eq!(
        adapter().ingest(&message(3, &state), NOW, WALL),
        Err(InputError::Stale)
    );
    state = controls();
    state.stamp.sequence = 0;
    assert_eq!(
        adapter().ingest(&message(3, &state), NOW, WALL),
        Err(InputError::InvalidPayload)
    );
}

#[test]
fn shared_stamp_order_is_checked_independently_of_the_wire_sequence() {
    let mut input = adapter();
    input.ingest(&message(3, &controls()), NOW, WALL).unwrap();
    let mut repeated_stamp = controls();
    repeated_stamp.stamp.at_us += 1_000;
    assert_eq!(
        input.ingest(
            &newer(message(3, &repeated_stamp), 2, 1),
            NOW + 1_000,
            WALL + 1,
        ),
        Err(InputError::OutOfOrder)
    );
    repeated_stamp.stamp.sequence = 2;
    repeated_stamp.stamp.at_us = NOW - 1;
    assert_eq!(
        input.ingest(
            &newer(message(3, &repeated_stamp), 3, 2),
            NOW + 2_000,
            WALL + 2,
        ),
        Err(InputError::OutOfOrder)
    );
    repeated_stamp.stamp.at_us = NOW + 3_000;
    input
        .ingest(
            &newer(message(3, &repeated_stamp), 4, 3),
            NOW + 3_000,
            WALL + 3,
        )
        .unwrap();
}

#[test]
fn bounded_expiry_arithmetic_cannot_panic_near_the_clock_limit() {
    let now = u64::MAX - 100_000;
    let mut value = name();
    value.ago_ms = 0;
    let mut hearing = message(7, &value);
    hearing.header.expires_ms = Some(1);
    let events = adapter().ingest(&hearing, now, WALL).unwrap();
    assert_eq!(events[0].valid_until_us, now + 1_000);
    hearing.header.expires_ms = None;
    assert_eq!(
        adapter().ingest(&hearing, now, WALL),
        Err(InputError::InvalidHeader)
    );
    value.ago_ms = u32::MAX;
    let hearing = message(7, &value);
    assert_eq!(
        adapter().ingest(&hearing, NOW, WALL),
        Err(InputError::Stale)
    );
}

#[test]
fn stop_is_deduplicated_across_retained_state_events_and_invalid_state() {
    let mut state = controls();
    state.latest_stop = Some(gesture(false));
    state.event_sequence = 1;
    let mut input = adapter();
    assert_eq!(
        input.ingest(&message(3, &state), NOW, WALL).unwrap().len(),
        1
    );
    let mut event = message(4, &gesture(false));
    event.header.boot = OTHER.into();
    assert!(input.ingest(&event, NOW, WALL).unwrap().is_empty());
    let mut malformed = newer(message(3, &state), 2, 1);
    malformed.payload = b"{}".to_vec();
    assert_eq!(
        input.ingest(&malformed, NOW + 1_000, WALL + 1),
        Err(InputError::InvalidPayload)
    );
    state.stamp.at_us += 2_000;
    state.stamp.sequence = 3;
    let renewed = newer(message(3, &state), 3, 2);
    assert!(
        input
            .ingest(&renewed, NOW + 2_000, WALL + 2)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_late_retained_stop_or_old_pet_release_cannot_wake() {
    let mut stop = gesture(false);
    stop.released_us = NOW - 500_000;
    let mut state = controls();
    state.latest_stop = Some(stop);
    state.event_sequence = 1;
    assert!(
        adapter()
            .ingest(&message(3, &state), NOW, WALL)
            .unwrap()
            .is_empty()
    );

    let mut input = adapter();
    input.ingest(&message(3, &controls()), NOW, WALL).unwrap();
    let mut pet = gesture(true);
    pet.released_us = NOW - 2_000_000;
    assert_eq!(
        input.ingest(&message(4, &pet), NOW, WALL),
        Err(InputError::Stale)
    );
}

#[test]
fn a_normal_delayed_pet_keeps_its_release_time_and_publication_expiry() {
    let mut input = adapter();
    input.ingest(&message(3, &controls()), NOW, WALL).unwrap();
    let mut pet = gesture(true);
    // Faithful to HeadRecognizer::due: 200 ms to close the session, then
    // 1,200 ms to decide Pet, before this fresh publication reaches Mood.
    pet.released_us = NOW - 1_400_000;
    let events = input.ingest(&message(4, &pet), NOW, WALL).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, EventKind::Touch { kind: true });
    assert_eq!(events[0].observed_us, pet.released_us);
    assert_eq!(events[0].valid_until_us, NOW + 500_000);

    // A republished event retains its controls identity even if the event
    // transport publisher restarts. The Pet watermark still prevents replay.
    let mut duplicate = newer(message(4, &pet), 1, 1);
    duplicate.header.boot = OTHER.into();
    assert!(
        input
            .ingest(&duplicate, NOW + 1_000, WALL + 1)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn pet_acquisition_and_publication_have_separate_strict_expiry_bounds() {
    let mut pet = gesture(true);
    pet.released_us = NOW - 1_999_999;
    let mut input = adapter();
    input.ingest(&message(3, &controls()), NOW, WALL).unwrap();
    let events = input.ingest(&message(4, &pet), NOW, WALL).unwrap();
    assert_eq!(events[0].observed_us, pet.released_us);
    assert_eq!(events[0].valid_until_us, NOW + 1);

    pet.released_us = NOW - 2_000_000;
    let mut input = adapter();
    input.ingest(&message(3, &controls()), NOW, WALL).unwrap();
    assert_eq!(
        input.ingest(&message(4, &pet), NOW, WALL),
        Err(InputError::Stale)
    );

    pet.released_us = NOW - 1_900_000;
    pet.stamp.at_us = NOW - 500_000;
    let mut input = adapter();
    input.ingest(&message(3, &controls()), NOW, WALL).unwrap();
    assert_eq!(
        input.ingest(&message(4, &pet), NOW, WALL),
        Err(InputError::Stale)
    );
    pet.stamp.at_us = NOW;
    let mut stale_wire = message(4, &pet);
    stale_wire.header.at_unix_ms = WALL - 500;
    assert_eq!(input.ingest(&stale_wire, NOW, WALL), Err(InputError::Stale));
}

#[test]
fn delayed_pet_cannot_cross_sleep_using_its_new_publication_time() {
    let mut input = adapter();
    input.ingest(&message(3, &controls()), NOW, WALL).unwrap();
    let mut pet = gesture(true);
    pet.released_us = NOW - 1_400_000;
    let event = input
        .ingest(&message(4, &pet), NOW, WALL)
        .unwrap()
        .remove(0);
    for (slept_us, can_wake) in [(NOW - 1_500_000, true), (NOW - 1_000_000, false)] {
        let mut engine = Engine::new(CLOCK.into(), NOW - 2_000_000, Some(720)).unwrap();
        engine
            .apply(
                slept_us,
                Event {
                    observed_us: slept_us,
                    valid_until_us: slept_us + 500_000,
                    kind: EventKind::Sleep,
                },
            )
            .unwrap();
        let result = engine.apply(NOW, event.clone());
        if can_wake {
            assert!(result.is_ok());
        } else {
            assert_eq!(result, Err(Reject::BeforeSleep));
        }
        engine.advance(NOW, Some(720)).unwrap();
        assert_eq!(engine.snapshot(NOW, 1, BOOT).unwrap().asleep, !can_wake);
    }
}

#[test]
fn pet_has_its_own_watermark_and_shadow_and_mute_do_not_become_touch() {
    let mut state = controls();
    state.event_sequence = 8;
    let mut input = adapter();
    input.ingest(&message(3, &state), NOW, WALL).unwrap();
    let mut pet = gesture(true);
    pet.stamp.sequence = 8;
    assert_eq!(
        input.ingest(&message(4, &pet), NOW, WALL).unwrap()[0].kind,
        EventKind::Touch { kind: true }
    );
    assert_eq!(
        input.ingest(&newer(message(4, &pet), 2, 1), NOW + 1_000, WALL + 1),
        Err(InputError::OutOfOrder)
    );

    for shadow in [false, true] {
        let mut input = adapter();
        input.ingest(&message(3, &controls()), NOW, WALL).unwrap();
        let mut value = gesture(false);
        if shadow {
            value.enabled = false;
        } else {
            value.gesture = Gesture::LongPress;
            value.intent = ControlIntent::SoftwareMute;
            value.duration_us = 2_000_000;
        }
        assert!(
            input
                .ingest(&message(4, &value), NOW, WALL)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn controls_restart_changes_session_and_rejects_the_previous_owner() {
    let mut input = adapter();
    input.ingest(&message(3, &controls()), NOW, WALL).unwrap();
    let mut restarted = controls();
    restarted.session_id = OTHER.into();
    restarted.stamp.at_us += 1_000;
    let mut state = newer(message(3, &restarted), 1, 1);
    state.header.boot = OTHER.into();
    input.ingest(&state, NOW + 1_000, WALL + 1).unwrap();
    assert_eq!(
        input.ingest(&message(4, &gesture(false)), NOW + 1_000, WALL + 1),
        Err(InputError::NoControlsState)
    );
    let mut retired = newer(message(3, &controls()), 2, 2);
    retired.header.at_unix_ms = WALL + 2;
    assert_eq!(
        input.ingest(&retired, NOW + 2_000, WALL + 2),
        Err(InputError::RetiredPublisher)
    );
}

#[test]
fn names_must_meet_the_existing_score_threshold_and_onset_age() {
    for (score, expected) in [(0.89, 0), (0.9, 1), (1.0, 1)] {
        let value = NameHeard { score, ago_ms: 100 };
        assert_eq!(
            adapter()
                .ingest(&message(7, &value), NOW, WALL)
                .unwrap()
                .len(),
            expected
        );
    }
    let value = NameHeard {
        score: 1.01,
        ago_ms: 100,
    };
    assert_eq!(
        adapter().ingest(&message(7, &value), NOW, WALL),
        Err(InputError::InvalidPayload)
    );
    let value = NameHeard {
        score: 0.99,
        ago_ms: 1_500,
    };
    assert_eq!(
        adapter().ingest(&message(7, &value), NOW, WALL),
        Err(InputError::Stale)
    );
}

#[test]
fn sudden_input_is_recent_short_energy_metadata_without_a_direction() {
    let mut value = SuddenSound {
        peak_db: -20.0,
        floor_db: -50.0,
        loud_ms: 50,
        short: true,
        ago_ms: 300,
    };
    let events = adapter().ingest(&message(5, &value), NOW, WALL).unwrap();
    assert_eq!(events[0].kind, EventKind::SuddenSound);
    assert_eq!(events[0].observed_us, NOW - 300_000);
    value.short = false;
    value.loud_ms = 250;
    assert!(
        adapter()
            .ingest(&message(5, &value), NOW, WALL)
            .unwrap()
            .is_empty()
    );
    value.short = true;
    assert_eq!(
        adapter().ingest(&message(5, &value), NOW, WALL),
        Err(InputError::InvalidPayload)
    );
    value.loud_ms = 50;
    value.ago_ms = 1_000;
    assert_eq!(
        adapter().ingest(&message(5, &value), NOW, WALL),
        Err(InputError::Stale)
    );
}

#[test]
fn air_is_a_finite_optional_reading_with_a_fifteen_second_lease() {
    for co2 in [None, Some(450), Some(1_500), Some(3_000)] {
        let events = adapter().ingest(&message(6, &air(co2)), NOW, WALL).unwrap();
        assert_eq!(events[0].kind, EventKind::Air(co2));
        assert_eq!(events[0].valid_until_us, NOW + 15_000_000);
    }
    let mut bad = air(Some(3_000));
    bad.feel = AirFeel::Fresh;
    assert_eq!(
        adapter().ingest(&message(6, &bad), NOW, WALL),
        Err(InputError::InvalidPayload)
    );
}

#[test]
fn reminder_identity_survives_heartbeat_and_publisher_restart() {
    let mut input = adapter();
    let first = due(1);
    assert_eq!(
        input.ingest(&message(8, &first), NOW, WALL).unwrap()[0].kind,
        EventKind::UserReminder
    );
    let mut renewed = first.clone();
    renewed.stamp.sequence = 2;
    renewed.stamp.at_us += 1_000;
    assert!(
        input
            .ingest(&newer(message(8, &renewed), 2, 1), NOW + 1_000, WALL + 1)
            .unwrap()
            .is_empty()
    );
    renewed.stamp.sequence = 1;
    renewed.stamp.at_us += 1_000;
    let mut restarted = newer(message(8, &renewed), 1, 2);
    restarted.header.boot = OTHER.into();
    assert!(
        input
            .ingest(&restarted, NOW + 2_000, WALL + 2)
            .unwrap()
            .is_empty()
    );
    renewed.stamp.sequence = 2;
    renewed.stamp.at_us += 1_000;
    renewed.oldest.as_mut().unwrap().delivery = 2;
    let mut next_occurrence = newer(message(8, &renewed), 2, 3);
    next_occurrence.header.boot = OTHER.into();
    assert_eq!(
        input
            .ingest(&next_occurrence, NOW + 3_000, WALL + 3)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn shadow_unready_foreign_or_invalid_reminders_cannot_wake() {
    for (enabled, ready) in [(false, true), (true, false), (false, false)] {
        let mut value = due(1);
        value.enabled = enabled;
        value.ready = ready;
        value.oldest = None;
        assert!(
            adapter()
                .ingest(&message(8, &value), NOW, WALL)
                .unwrap()
                .is_empty()
        );
    }
    let mut value = due(1);
    value.enabled = false;
    assert_eq!(
        adapter().ingest(&message(8, &value), NOW, WALL),
        Err(InputError::InvalidPayload)
    );
    value = due(1);
    value.stamp.clock_id = "another-boot".into();
    assert_eq!(
        adapter().ingest(&message(8, &value), NOW, WALL),
        Err(InputError::WrongClock)
    );
}

#[test]
fn fresh_schedule_authority_survives_a_wall_clock_step_back_once() {
    let mut input = adapter();
    let mut value = due(1);
    // A queued elapsed reminder can remain due after wall time steps back.
    // Its owner validates that clock; Mood does not reschedule the occurrence.
    value.oldest.as_mut().unwrap().due_unix_ms = WALL as i64 + 60_000;
    let events = input.ingest(&message(8, &value), NOW, WALL).unwrap();
    assert_eq!(events[0].kind, EventKind::UserReminder);
    value.stamp.sequence = 2;
    value.stamp.at_us += 1_000;
    assert!(
        input
            .ingest(&newer(message(8, &value), 2, 1), NOW + 1_000, WALL + 1,)
            .unwrap()
            .is_empty()
    );
    value.stamp.at_us = NOW - 3_000_000;
    assert_eq!(
        adapter().ingest(&message(8, &value), NOW, WALL),
        Err(InputError::Stale)
    );
}

#[test]
fn reminder_flood_cannot_expand_history_or_repeat_an_old_delivery() {
    let mut input = adapter();
    for identity in 1..=MAX_REMINDER_IDENTITIES + 20 {
        let elapsed = identity as u64;
        let mut value = due(elapsed);
        value.stamp.sequence = elapsed;
        value.stamp.at_us += elapsed * 1_000;
        let message = newer(message(8, &value), elapsed, elapsed);
        let output = input.ingest(&message, NOW + elapsed * 1_000, WALL + elapsed);
        if identity <= MAX_REMINDER_IDENTITIES {
            assert_eq!(output.unwrap().len(), 1);
        } else {
            assert_eq!(output, Err(InputError::ReminderHistoryFull));
        }
    }
    let mut old = due(1);
    old.stamp.sequence = 1_000;
    old.stamp.at_us += 1_000_000;
    assert!(
        input
            .ingest(
                &newer(message(8, &old), 1_000, 1_000),
                NOW + 1_000_000,
                WALL + 1_000
            )
            .unwrap()
            .is_empty()
    );
    assert_eq!(input.counts().accepted, MAX_REMINDER_IDENTITIES as u64);
    assert_eq!(input.counts().rejected, 20);
}
