use lamp_bus::{Bus, Header, Kind, Message};
use lamp_expression::{Catalogue, Engine, Input, Output, runtime};
use lamp_msgs::{
    ClockStamp, Concentration, ExpressionBody, ExpressionRestOp as Op,
    ExpressionRestPhase as Phase, ExpressionRestRequest as Request, ExpressionRestState as Receipt,
    ExpressionState, JointState, MuteReason, NameHeard, Pause, Voice, VoiceEvent, VoiceMute,
    VoiceProgress, VoiceTiming,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

const START: u64 = 10_000_000;
const CLOCK: &str = "rest-test";
const TARGET: &str = "expression-one";

fn catalogue() -> Catalogue {
    Catalogue::from_json(include_str!("../alive-behaviours.json")).unwrap()
}

fn stamp(at_us: u64, sequence: u64) -> ClockStamp {
    ClockStamp {
        clock_id: CLOCK.into(),
        at_us,
        sequence,
    }
}

fn voice(at_us: u64, sequence: u64) -> VoiceTiming {
    VoiceTiming {
        stamp: stamp(at_us, sequence),
        session_id: "voice-one".into(),
        turn: 1,
        request: 1,
        state: Voice::Resting,
        accepting_input: false,
        event: VoiceEvent::State,
        first_audio_us: None,
    }
}

fn joints() -> JointState {
    JointState {
        name: ["base_yaw", "shoulder", "elbow", "head_roll", "head_pitch"]
            .map(str::to_owned)
            .to_vec(),
        position: vec![0.; 5],
        commanded: vec![],
        velocity: vec![],
        effort: vec![],
    }
}

fn progress(timing: VoiceTiming) -> VoiceProgress {
    VoiceProgress {
        timing,
        audio_generation: 1,
        held: false,
    }
}

fn voice_input(timing: VoiceTiming) -> Input {
    Input::Progress(progress(timing))
}

fn request(at_us: u64, sequence: u64, episode: u64, op: Op) -> Request {
    Request {
        stamp: stamp(at_us, sequence),
        skill: "sample-routine".into(),
        skill_session_id: "publisher-one".into(),
        expression_session_id: TARGET.into(),
        episode,
        op,
        valid_until_us: at_us + 3_000_000,
    }
}

struct Fixture {
    engine: Engine,
    now: u64,
    sequence: u64,
    request_sequence: u64,
}

impl Fixture {
    fn new(enabled: bool) -> Self {
        let engine = Engine::new(catalogue(), enabled, CLOCK)
            .unwrap()
            .with_session_id(TARGET)
            .unwrap()
            .with_rest_requests();
        let mut fixture = Self {
            engine,
            now: START,
            sequence: 0,
            request_sequence: 0,
        };
        fixture.heartbeat();
        fixture
    }

    fn heartbeat(&mut self) {
        self.sequence += 1;
        self.engine
            .ingest(self.now, voice_input(voice(self.now, self.sequence)))
            .unwrap();
        self.engine
            .ingest(
                self.now,
                Input::Joints {
                    stamp: stamp(self.now, self.sequence),
                    value: joints(),
                },
            )
            .unwrap();
    }

    fn advance(&mut self, by: u64) -> Output {
        assert_eq!(by % 100_000, 0);
        for _ in 0..by / 100_000 {
            self.now += 100_000;
            self.heartbeat();
            self.engine.tick(self.now);
        }
        self.engine.tick(self.now)
    }

    fn offer(&mut self, episode: u64, op: Op) -> Result<(), lamp_expression::Error> {
        self.request_sequence += 1;
        self.engine.ingest_rest(
            self.now,
            request(self.now, self.request_sequence, episode, op),
        )
    }

    fn receipt(&mut self) -> Receipt {
        let receipt = self.engine.rest_state(self.now).unwrap();
        receipt.check().unwrap();
        receipt
    }
}

#[test]
fn sustained_rest_renews_without_restarting_and_one_wake_finishes() {
    let mut fixture = Fixture::new(true);
    fixture.offer(1, Op::Rest).unwrap();
    assert_eq!(fixture.receipt().phase, Phase::Resting);
    for _ in 0..5 {
        fixture.advance(1_000_000);
        fixture.offer(1, Op::Rest).unwrap();
    }
    let output = fixture.engine.tick(fixture.now);
    assert!(output.state.behaviours.iter().any(|name| name == "asleep"));
    fixture.offer(1, Op::Wake).unwrap();
    assert_eq!(fixture.receipt().phase, Phase::Waking);
    for _ in 0..2 {
        fixture.advance(1_000_000);
        fixture.offer(1, Op::Wake).unwrap();
    }
    fixture.advance(700_000);
    assert_eq!(fixture.receipt().phase, Phase::Completed);
    assert!(fixture.offer(1, Op::Wake).is_err());
    assert!(fixture.offer(1, Op::Rest).is_err());
    assert_eq!(fixture.receipt().phase, Phase::Completed);
}

#[test]
fn expiry_is_exact_and_renewals_cannot_revive_the_episode() {
    let mut fixture = Fixture::new(true);
    fixture.offer(1, Op::Rest).unwrap();
    fixture.advance(2_900_000);
    assert_eq!(fixture.receipt().phase, Phase::Resting);
    fixture.advance(100_000);
    assert_eq!(fixture.receipt().phase, Phase::Cancelled);
    assert!(fixture.offer(1, Op::Rest).is_err());
    assert!(fixture.offer(1, Op::Wake).is_err());
}

#[test]
fn original_age_is_not_rejuvenated_and_invalid_observations_are_spent() {
    for offset in [500_000_i64, -1] {
        let mut fixture = Fixture::new(true);
        let at = fixture.now.checked_add_signed(-offset).unwrap();
        assert!(
            fixture
                .engine
                .ingest_rest(fixture.now, request(at, 1, 1, Op::Rest))
                .is_err()
        );
        assert_eq!(fixture.receipt().phase, Phase::Refused);
        assert!(
            fixture
                .engine
                .ingest_rest(fixture.now, request(fixture.now, 2, 1, Op::Rest))
                .is_err()
        );
    }
    let mut fixture = Fixture::new(true);
    fixture
        .engine
        .ingest_rest(fixture.now, request(fixture.now - 499_999, 1, 1, Op::Rest))
        .unwrap();
    assert_eq!(fixture.receipt().phase, Phase::Resting);
}

#[test]
fn shadow_pause_and_cancel_before_start_fence_the_episode() {
    let mut shadow = Fixture::new(false);
    assert!(shadow.offer(1, Op::Rest).is_err());
    assert_eq!(shadow.receipt().phase, Phase::Refused);
    let output = shadow.engine.tick(shadow.now);
    assert!(output.body.is_none() && output.light.is_none());

    let mut fixture = Fixture::new(true);
    fixture.engine.set_rest_paused(fixture.now, true);
    assert!(fixture.offer(1, Op::Rest).is_err());
    fixture.engine.set_rest_paused(fixture.now, false);
    assert!(fixture.offer(1, Op::Rest).is_err());
    fixture.offer(2, Op::Cancel).unwrap();
    assert!(fixture.offer(2, Op::Rest).is_err());
    fixture.offer(3, Op::Rest).unwrap();
    fixture.engine.set_rest_paused(fixture.now, true);
    assert_eq!(fixture.receipt().phase, Phase::Cancelled);
    fixture.engine.set_rest_paused(fixture.now, false);
    fixture.engine = fixture.engine.with_rest_requests();
    assert!(fixture.offer(3, Op::Rest).is_err());
}

#[test]
fn privacy_concentration_and_cancelled_voice_cancel_without_resumption() {
    for guard in 0..3 {
        let mut fixture = Fixture::new(true);
        fixture.offer(1, Op::Rest).unwrap();
        match guard {
            0 => fixture.engine.set_privacy_muted(true),
            1 => fixture
                .engine
                .ingest(
                    fixture.now,
                    Input::Concentration(Concentration {
                        stamp: stamp(fixture.now, 1),
                        concentrating: true,
                    }),
                )
                .unwrap(),
            _ => {
                let mut unavailable = voice(fixture.now, 2);
                unavailable.event = VoiceEvent::Cancelled;
                fixture
                    .engine
                    .ingest(fixture.now, voice_input(unavailable))
                    .unwrap();
            }
        }
        assert_eq!(fixture.receipt().phase, Phase::Cancelled, "guard {guard}");
        fixture.engine.set_privacy_muted(false);
        fixture.sequence += 1;
        fixture.heartbeat();
        assert!(fixture.offer(1, Op::Rest).is_err());
    }
}

#[test]
fn a_new_voice_owner_or_stale_measured_state_spends_rest() {
    for change in 0..5 {
        let mut fixture = Fixture::new(true);
        fixture.offer(1, Op::Rest).unwrap();
        let mut changed = voice(fixture.now, 2);
        match change {
            0 => {
                changed.session_id = "voice-two".into();
                fixture.now += 1;
                changed.stamp.at_us = fixture.now;
            }
            1 => {
                changed.turn += 1;
                changed.request += 1;
            }
            2 => changed.state = Voice::Listening,
            3 => {
                fixture.now += 300_000;
                changed.stamp.at_us = fixture.now;
            }
            _ => {
                fixture.now += 500_000;
                changed.stamp.at_us = fixture.now;
            }
        }
        fixture
            .engine
            .ingest(fixture.now, voice_input(changed))
            .unwrap();
        assert_eq!(fixture.receipt().phase, Phase::Cancelled, "change {change}");
        assert!(fixture.offer(1, Op::Rest).is_err());
    }
}

#[test]
fn a_name_or_conversation_wake_survives_routine_cancellation() {
    for conversation in [false, true] {
        let mut fixture = Fixture::new(true);
        fixture.offer(1, Op::Rest).unwrap();
        fixture.advance(1_000_000);
        if conversation {
            let mut talking = voice(fixture.now, fixture.sequence + 1);
            talking.state = Voice::Thinking;
            fixture
                .engine
                .ingest(fixture.now, voice_input(talking))
                .unwrap();
        } else {
            fixture
                .engine
                .ingest(
                    fixture.now,
                    Input::Name {
                        stamp: stamp(fixture.now, 1),
                        value: NameHeard {
                            score: 0.99,
                            ago_ms: 0,
                        },
                    },
                )
                .unwrap();
        }
        assert_eq!(fixture.receipt().phase, Phase::Cancelled);
        let output = fixture.engine.tick(fixture.now);
        assert!(output.state.behaviours.iter().any(|name| name == "waking"));
        assert!(fixture.offer(1, Op::Wake).is_err());
    }
}

#[test]
fn manual_sleep_is_neither_woken_nor_released_by_a_routine() {
    let mut fixture = Fixture::new(true);
    fixture.offer(1, Op::Rest).unwrap();
    fixture
        .engine
        .ingest(
            fixture.now,
            Input::VoiceMute {
                stamp: stamp(fixture.now, 1),
                value: VoiceMute {
                    muted: true,
                    reason: Some(MuteReason::Sleep),
                },
            },
        )
        .unwrap();
    assert_eq!(fixture.receipt().phase, Phase::Cancelled);
    assert!(fixture.offer(1, Op::Wake).is_err());
    fixture.offer(2, Op::Cancel).unwrap();
    assert!(fixture.offer(3, Op::Rest).is_err());
    let output = fixture.advance(4_000_000);
    assert!(output.state.behaviours.iter().any(|name| name == "asleep"));
    assert!(!output.state.behaviours.iter().any(|name| name == "waking"));
}

#[test]
fn publisher_restart_retires_its_old_identity_and_does_not_steal_manual_or_other_rest() {
    let mut fixture = Fixture::new(true);
    fixture.offer(1, Op::Rest).unwrap();
    fixture.advance(100_000);
    let mut restarted = request(fixture.now, 1, 1, Op::Cancel);
    restarted.skill_session_id = "publisher-two".into();
    restarted.skill = "a-different-skill".into();
    fixture.engine.ingest_rest(fixture.now, restarted).unwrap();
    assert_eq!(fixture.receipt().phase, Phase::Cancelled);
    assert!(fixture.offer(2, Op::Rest).is_err());

    let mut fixture = Fixture::new(true);
    fixture.offer(1, Op::Rest).unwrap();
    let mut other = request(fixture.now, 1, 1, Op::Rest);
    other.skill = "another-routine".into();
    assert!(
        fixture
            .engine
            .ingest_rest(fixture.now, other.clone())
            .is_err()
    );
    fixture.offer(1, Op::Rest).unwrap();
    assert_eq!(fixture.receipt().phase, Phase::Resting);
    other.stamp.sequence += 1;
    other.episode += 1;
    other.op = Op::Cancel;
    fixture.engine.ingest_rest(fixture.now, other).unwrap();
    fixture.offer(1, Op::Rest).unwrap();
    assert_eq!(fixture.receipt().phase, Phase::Resting);
}

#[test]
fn duplicate_order_and_wake_to_rest_regression_cannot_restart_a_clip() {
    let mut fixture = Fixture::new(true);
    fixture.offer(1, Op::Rest).unwrap();
    assert!(
        fixture
            .engine
            .ingest_rest(fixture.now, request(fixture.now, 1, 1, Op::Rest))
            .is_err()
    );
    fixture.advance(100_000);
    fixture.offer(1, Op::Wake).unwrap();
    assert!(fixture.offer(1, Op::Rest).is_err());
    assert_eq!(fixture.receipt().phase, Phase::Refused);
    assert!(fixture.offer(1, Op::Wake).is_err());
    let output = fixture.engine.tick(fixture.now);
    assert!(!output.state.behaviours.iter().any(|name| name == "waking"));
}

#[test]
fn bounded_history_keeps_old_tombstones_when_new_sources_are_refused() {
    let mut fixture = Fixture::new(true);
    for source in 0..32 {
        let mut cancel = request(fixture.now, 1, 1, Op::Cancel);
        cancel.skill = format!("routine-{source}");
        fixture.engine.ingest_rest(fixture.now, cancel).unwrap();
    }
    assert!(fixture.offer(1, Op::Rest).is_err());
    let mut old = request(fixture.now, 2, 1, Op::Rest);
    old.skill = "routine-0".into();
    assert!(fixture.engine.ingest_rest(fixture.now, old).is_err());
}

#[test]
fn wrong_boot_or_target_and_default_off_fail_quiet() {
    let mut fixture = Fixture::new(true);
    for wrong_clock in [false, true] {
        let mut wrong = request(fixture.now, 1, 1, Op::Rest);
        if wrong_clock {
            wrong.stamp.clock_id = "different-boot".into();
        } else {
            wrong.expression_session_id = "old-expression".into();
        }
        assert!(fixture.engine.ingest_rest(fixture.now, wrong).is_err());
        assert_eq!(fixture.receipt().phase, Phase::Idle);
    }
    let mut off = Engine::new(catalogue(), true, CLOCK)
        .unwrap()
        .with_session_id(TARGET)
        .unwrap();
    assert!(
        off.ingest_rest(START, request(START, 1, 1, Op::Rest))
            .is_err()
    );
    assert!(off.rest_state(START).is_none());
    let mut cold = off.with_rest_requests();
    assert!(
        cold.ingest_rest(START, request(START, 1, 1, Op::Rest))
            .is_err()
    );
    assert_eq!(cold.rest_state(START).unwrap().phase, Phase::Refused);
    let output = cold.tick(START);
    assert!(output.body.is_none() && output.light.is_none());
    assert!(runtime::rest_requests_on(Some("on")));
    for value in [
        None,
        Some("off"),
        Some("true"),
        Some("1"),
        Some("ON"),
        Some(" on"),
    ] {
        assert!(!runtime::rest_requests_on(value));
    }
}

#[test]
fn a_backwards_clock_cancels_rest_and_cannot_restart_it() {
    let mut fixture = Fixture::new(true);
    fixture.offer(1, Op::Rest).unwrap();
    let output = fixture.engine.tick(fixture.now - 1);
    assert!(output.body.is_none() && output.light.is_none());
    assert_eq!(fixture.receipt().phase, Phase::Cancelled);
    assert!(fixture.offer(1, Op::Rest).is_err());
}

fn message() -> Message {
    Message {
        header: Header {
            topic: Request::TOPIC.into(),
            kind: Kind::Event,
            enc: "json".into(),
            type_name: Some(Request::TYPE.into()),
            source: "skills".into(),
            boot: "publisher-one".into(),
            seq: 1,
            at_us: 999_999_999,
            at_unix_ms: 1_000,
            expires_ms: Some(500),
        },
        payload: serde_json::to_vec(&request(START, 1, 1, Op::Rest)).unwrap(),
        gap: 0,
        publisher_restarted: false,
        expired: false,
    }
}

#[test]
fn transport_binds_the_skill_publisher_sequence_and_original_monotonic_stamp() {
    let message = message();
    let parsed = runtime::rest_from_message(&message, 1_000).unwrap();
    assert_eq!(parsed.stamp.at_us, START);
    for changed in 0..10 {
        let mut wrong = message.clone();
        match changed {
            0 => wrong.header.source = "different".into(),
            1 => wrong.header.boot = "publisher-two".into(),
            2 => wrong.header.seq += 1,
            3 => wrong.header.kind = Kind::Latest,
            4 => wrong.header.type_name = None,
            5 => wrong.header.topic = "body/target".into(),
            6 => wrong.header.at_unix_ms = 1_051,
            7 => wrong.expired = true,
            8 => wrong.payload.resize(4097, b' '),
            _ => wrong.header.enc = "none".into(),
        }
        assert!(
            runtime::rest_from_message(&wrong, 1_000).is_err(),
            "change {changed}"
        );
    }
    assert!(runtime::rest_from_message(&message, 1_551).is_err());
}

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = PathBuf::from(format!(
            "/tmp/rest-{}-{}",
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

fn publish_context(bus: &Bus, now: u64) {
    bus.publisher_declared(VoiceProgress::TOPIC, &VoiceProgress::declaration(), "voice")
        .unwrap()
        .publish_json(1, &progress(voice(now, 1)))
        .unwrap();
    bus.publisher_declared(JointState::TOPIC, &JointState::declaration(), "body")
        .unwrap()
        .publish_json(2, &joints())
        .unwrap();
}

#[test]
fn live_bus_request_uses_the_existing_writer_and_publishes_only_a_planning_receipt() {
    let root = Root::new();
    let bus = Bus::new(&root.0).unwrap().rescan_every(Duration::ZERO);
    let mut service = runtime::Service::new(&bus, catalogue(), true, CLOCK)
        .unwrap()
        .with_rest_requests(&bus)
        .unwrap();
    publish_context(&bus, START);
    service.step(|| START, runtime::unix_ms());
    let state = bus.latest(ExpressionState::TOPIC).unwrap().unwrap();
    let idle: Receipt = bus
        .latest(Receipt::TOPIC)
        .unwrap()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(idle.phase, Phase::Idle);
    assert_eq!(idle.expression_session_id, state.header.boot);
    let mut publisher = bus
        .publisher_declared(Request::TOPIC, &Request::declaration(), "skills")
        .unwrap();
    let mut rest = request(START, 1, 1, Op::Rest);
    rest.skill_session_id = publisher.boot().to_owned();
    rest.expression_session_id = state.header.boot;
    publisher.publish_json(99, &rest).unwrap();
    let output = service.step(|| START + 200_000, runtime::unix_ms());
    assert!(output.body.is_some());
    let receipt: Receipt = bus
        .latest(Receipt::TOPIC)
        .unwrap()
        .unwrap()
        .parse()
        .unwrap();
    receipt.check().unwrap();
    assert_eq!(receipt.phase, Phase::Resting);
    assert_eq!(receipt.request, Some(rest));
    assert_eq!(bus.publishers(ExpressionBody::TOPIC).unwrap().len(), 1);
    assert!(bus.latest(VoiceMute::TOPIC).unwrap().is_none());
}

#[test]
fn live_pause_is_checked_before_a_request_and_shadow_never_claims_rest() {
    for (enabled, paused) in [(true, true), (false, false)] {
        let root = Root::new();
        let bus = Bus::new(&root.0).unwrap();
        bus.publisher_declared(Pause::TOPIC, &Pause::declaration(), "keeper")
            .unwrap()
            .publish_json(1, &Pause { paused })
            .unwrap();
        let mut service = runtime::Service::new(&bus, catalogue(), enabled, CLOCK)
            .unwrap()
            .with_rest_requests(&bus)
            .unwrap();
        publish_context(&bus, START);
        service.step(|| START, runtime::unix_ms());
        let state = bus.latest(ExpressionState::TOPIC).unwrap().unwrap();
        let mut publisher = bus
            .publisher_declared(Request::TOPIC, &Request::declaration(), "skills")
            .unwrap();
        let mut rest = request(START, 1, 1, Op::Rest);
        rest.skill_session_id = publisher.boot().to_owned();
        rest.expression_session_id = state.header.boot;
        publisher.publish_json(1, &rest).unwrap();
        let output = service.step(|| START + 200_000, runtime::unix_ms());
        assert!(output.body.is_none() && output.light.is_none());
        let receipt: Receipt = bus
            .latest(Receipt::TOPIC)
            .unwrap()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(receipt.phase, Phase::Refused);
    }
}

#[test]
fn default_service_declares_no_new_rest_transport() {
    let root = Root::new();
    let bus = Bus::new(&root.0).unwrap();
    let mut service = runtime::Service::new(&bus, catalogue(), true, CLOCK).unwrap();
    service.step(|| START, runtime::unix_ms());
    assert!(
        bus.subscribers(Request::TOPIC)
            .unwrap_or_default()
            .is_empty()
    );
    assert!(bus.latest(Receipt::TOPIC).unwrap().is_none());
}
