//! Real local Unix-bus transport with injected planner and wall clocks.
//! The times below are authored and do not measure device scheduling or DACs.
use lamp_bus::{Bus, Header, Kind, Message};
use lamp_expression::{Catalogue, Engine, Input, replay::Record, runtime};
use lamp_msgs::{
    ClockStamp, ExpressionBody, ExpressionLight, JointState, Light, RoomState, Voice, VoiceEvent,
    VoicePlayout, VoiceTiming,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const CLOCK: &str = "recent-playout-runtime";
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = PathBuf::from(format!(
            "/tmp/ex-recent-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn catalogue(recent: bool) -> Catalogue {
    let mut catalogue =
        Catalogue::from_json(include_str!("../../playout-behaviours.json")).unwrap();
    if !recent {
        catalogue.timing.recent_playout_us = 0;
    }
    catalogue
}

fn stamp(at_us: u64) -> ClockStamp {
    ClockStamp {
        clock_id: CLOCK.into(),
        at_us,
        sequence: at_us + 1,
    }
}

fn voice(at_us: u64) -> VoiceTiming {
    VoiceTiming {
        stamp: stamp(at_us),
        session_id: "fixture-voice".into(),
        turn: 1,
        request: 1,
        state: Voice::Speaking,
        accepting_input: true,
        event: VoiceEvent::State,
        first_audio_us: None,
    }
}

fn frame(start_us: u64, level: f32) -> VoicePlayout {
    VoicePlayout {
        stamp: stamp(start_us + 1_700),
        session_id: "fixture-voice".into(),
        turn: 1,
        request: 1,
        start_us,
        end_us: start_us + 20_000,
        level,
    }
}

fn message(frame: &VoicePlayout) -> Message {
    Message {
        header: Header {
            topic: VoicePlayout::TOPIC.into(),
            kind: Kind::Event,
            enc: "json".into(),
            type_name: Some(VoicePlayout::TYPE.into()),
            source: "voice".into(),
            boot: "0123456789abcdef".into(),
            seq: 9,
            at_us: 987_654_321,
            at_unix_ms: 1_000,
            expires_ms: Some(200),
            clock: None,
        },
        payload: serde_json::to_vec(frame).unwrap(),
        gap: 0,
        publisher_restarted: false,
        expired: false,
    }
}

fn populate(bus: &Bus, at_us: u64) {
    for line in include_str!("../../fixtures/recent-playout.jsonl")
        .lines()
        .take(3)
    {
        let record: Record = serde_json::from_str(line).unwrap();
        match record.input {
            Input::Joints { value, .. } => {
                bus.publisher_declared(JointState::TOPIC, &JointState::declaration(), "body")
                    .unwrap()
                    .publish_json(7, &value)
                    .unwrap();
            }
            Input::Light { value, .. } => {
                bus.publisher_declared(Light::TOPICS[0], &Light::declaration(), "actions")
                    .unwrap()
                    .publish_json(900_000_000, &value)
                    .unwrap();
            }
            Input::Room { value, .. } => {
                bus.publisher_declared(RoomState::TOPIC, &RoomState::declaration(), "eyes")
                    .unwrap()
                    .publish_json(0, &value)
                    .unwrap();
            }
            _ => panic!("fixture context must remain joints, light and room"),
        }
    }
    bus.publisher_declared(VoiceTiming::TOPIC, &VoiceTiming::declaration(), "voice")
        .unwrap()
        .publish_json(2, &voice(at_us))
        .unwrap();
}

#[test]
fn adaptation_keeps_actual_payload_stamp_and_existing_header_expiry_grace() {
    let frame = frame(100_000, 1.0);
    let item = message(&frame);
    // The bus's existing 50 ms wall-clock grace is unchanged: its exact
    // boundary is accepted, one millisecond beyond is rejected.
    let adapted = runtime::input_from_message(&item, 121_700, 1_250, CLOCK, 8_888).unwrap();
    assert_eq!(adapted.stamp(), &frame.stamp);
    let Input::Playout(adapted) = adapted else {
        panic!("typed playout must retain its variant");
    };
    assert_eq!(adapted, frame);
    let mut engine = Engine::new(catalogue(true), true, CLOCK).unwrap();
    engine
        .ingest(100_000, Input::Voice(voice(100_000)))
        .unwrap();
    engine.ingest(121_700, Input::Playout(adapted)).unwrap();
    assert_eq!(engine.queued_playout_frames(), 0);
    assert_eq!(
        runtime::input_from_message(&item, 121_700, 1_251, CLOCK, 8_888)
            .unwrap_err()
            .to_string(),
        "expired voice/playout"
    );
    let mut already_expired = item;
    already_expired.expired = true;
    assert_eq!(
        runtime::input_from_message(&already_expired, 121_700, 1_001, CLOCK, 8_888)
            .unwrap_err()
            .to_string(),
        "expired voice/playout"
    );
}

#[test]
fn a_fresh_header_cannot_refresh_the_typed_observation_age() {
    let mut frame = frame(0, 1.0);
    frame.stamp = stamp(0);
    frame.end_us = 200_000;
    let adapted = runtime::input_from_message(&message(&frame), 200_000, 1_001, CLOCK, 4).unwrap();
    let mut engine = Engine::new(catalogue(true), true, CLOCK).unwrap();
    engine
        .ingest(200_000, Input::Voice(voice(200_000)))
        .unwrap();
    assert_eq!(
        engine.ingest(200_000, adapted).unwrap_err().to_string(),
        "input observation is future or stale at arrival"
    );
    assert_eq!(engine.queued_playout_frames(), 0);
}

#[test]
fn real_bus_tracks_successive_ended_levels_and_shadow_never_publishes_actuators() {
    for recent in [false, true] {
        for enabled in [false, true] {
            let root = Root::new();
            let bus = Bus::new(&root.0).unwrap();
            let mut service =
                runtime::Service::new(&bus, catalogue(recent), enabled, CLOCK).unwrap();
            populate(&bus, 100_000);
            let mut publisher = bus
                .publisher_declared(VoicePlayout::TOPIC, &VoicePlayout::declaration(), "voice")
                .unwrap();
            for (index, level) in [1.0, 0.5, 0.0].into_iter().enumerate() {
                let frame = frame(100_000 + index as u64 * 20_000, level);
                let at_us = frame.end_us + 20_000;
                assert_eq!(publisher.publish_json(42, &frame).unwrap().delivered, 1);
                let output = service.step(|| at_us, runtime::unix_ms());
                assert_eq!(service.counts.input_errors, 0);
                assert_eq!(service.counts.output_errors, 0);
                assert_eq!(service.counts.dropped, 0);
                if enabled {
                    let expected = if recent { 0.3 + 0.4 * level * 0.8 } else { 0.3 };
                    assert!((output.light.unwrap().brightness - expected).abs() < 0.000_002);
                    assert!(bus.latest(ExpressionLight::TOPIC).unwrap().is_some());
                } else {
                    assert!(output.light.is_none() && output.body.is_none());
                    assert!(bus.latest(ExpressionLight::TOPIC).unwrap().is_none());
                    assert!(bus.latest(ExpressionBody::TOPIC).unwrap().is_none());
                }
            }
            assert_eq!(service.counts.accepted, if recent { 7 } else { 4 });
            assert_eq!(service.counts.refused, if recent { 0 } else { 3 });
        }
    }
}

#[test]
fn real_bus_expired_header_is_refused_even_when_the_recent_window_would_accept_payload() {
    let root = Root::new();
    let bus = Bus::new(&root.0).unwrap();
    let mut service = runtime::Service::new(&bus, catalogue(true), true, CLOCK).unwrap();
    populate(&bus, 100_000);
    service.step(|| 100_000, runtime::unix_ms());
    assert_eq!(service.counts.accepted, 4);
    let frame = frame(100_000, 1.0);
    bus.publisher_declared(VoicePlayout::TOPIC, &VoicePlayout::declaration(), "voice")
        .unwrap()
        .publish_json(42, &frame)
        .unwrap();
    // Inject wall age beyond 200 ms + the existing 50 ms bus grace. The
    // monotonic payload itself is only 38.3 ms old and ended 20 ms ago.
    let output = service.step(|| 140_000, runtime::unix_ms() + 251);
    assert_eq!(service.counts.accepted, 4);
    assert_eq!(service.counts.refused, 1);
    assert_eq!(service.counts.input_errors, 0);
    assert!((output.light.unwrap().brightness - 0.3).abs() < 0.000_002);
    assert!(
        !output
            .state
            .behaviours
            .iter()
            .any(|name| name == "follow played sound")
    );
}
