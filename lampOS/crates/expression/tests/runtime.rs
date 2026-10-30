use lamp_bus::{Bus, Header, Kind, Message};
use lamp_expression::{Catalogue, runtime};
use lamp_msgs::{
    ClockStamp, Diagnostic, ExpressionBody, ExpressionLight, ExpressionState, JointState, Light,
    Voice, VoiceEvent, VoiceTiming,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = PathBuf::from(format!(
            "/tmp/ex-{}-{}",
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

fn voice(at_us: u64) -> VoiceTiming {
    VoiceTiming {
        stamp: ClockStamp {
            clock_id: "test".into(),
            at_us,
            sequence: 1,
        },
        session_id: "voice_session".into(),
        turn: 1,
        request: 1,
        state: Voice::Listening,
        accepting_input: true,
        event: VoiceEvent::State,
        first_audio_us: None,
    }
}

/// Publishes the fixture's inputs and returns the wall clock just after:
/// a test steps with it for the engine's freshness checks (#844 review, #912).
/// Transport admission still uses the real wall clock. Finish service setup
/// before publishing inputs that the first step must accept. Loosest limit first:
/// the light (a setting, no age), the voice (500 ms), the joints (300 ms in
/// the engine) last. A stall between the voice and the clock read longer
/// than 500 ms still fails (docs/realtime-tests.md).
fn populate(bus: &Bus, at_us: u64) -> u64 {
    let joints = JointState {
        name: ["base_yaw", "shoulder", "elbow", "head_roll", "head_pitch"]
            .map(str::to_owned)
            .to_vec(),
        position: vec![0.; 5],
        velocity: vec![0.; 5],
        effort: vec![0.; 5],
        commanded: vec![0.; 5],
    };
    bus.publisher_declared(Light::TOPICS[0], &Light::declaration(), "actions")
        .unwrap()
        .publish_json(900_000_000, &Light::new(true, "warm", 2).unwrap())
        .unwrap();
    bus.publisher_declared(VoiceTiming::TOPIC, &VoiceTiming::declaration(), "voice")
        .unwrap()
        .publish_json(5, &voice(at_us))
        .unwrap();
    bus.publisher_declared(JointState::TOPIC, &JointState::declaration(), "body")
        .unwrap()
        .publish_json(7, &joints)
        .unwrap();
    runtime::unix_ms()
}

#[test]
fn shadow_mode_has_diagnostics_but_never_an_actuator_request() {
    let root = Root::new();
    let bus = Bus::new(&root.0).unwrap();
    let wall = populate(&bus, 10_000_000);
    let mut service = runtime::Service::new(&bus, Catalogue::builtin(), false, "test").unwrap();
    let output = service.step(|| 10_000_000, wall);
    assert!(output.body.is_none() && output.light.is_none());
    assert!(!output.state.enabled);
    assert!(bus.latest(ExpressionBody::TOPIC).unwrap().is_none());
    assert!(bus.latest(ExpressionLight::TOPIC).unwrap().is_none());
    let state: ExpressionState = bus
        .latest(ExpressionState::TOPIC)
        .unwrap()
        .unwrap()
        .parse()
        .unwrap();
    assert!(!state.enabled);
    assert!(bus.declaration(Diagnostic::TOPIC).unwrap().is_some());
}

#[test]
fn the_heads_topics_are_read_only_with_head_touch() {
    // #711: off unless `serve --head-touch`.
    use lamp_msgs::{TouchEvent, TouchState};
    let root = Root::new();
    let bus = Bus::new(&root.0).unwrap();
    let service = runtime::Service::new(&bus, Catalogue::builtin(), false, "test").unwrap();
    for topic in [TouchEvent::TOPIC, TouchState::TOPIC] {
        assert!(
            bus.subscribers(topic).unwrap_or_default().is_empty(),
            "{topic}"
        );
    }
    let _service = service.with_head_touch(&bus).unwrap();
    for topic in [TouchEvent::TOPIC, TouchState::TOPIC] {
        assert_eq!(bus.subscribers(topic).unwrap().len(), 1, "{topic}");
    }
}

#[test]
fn enabled_output_uses_a_new_session_and_expires_after_input_loss() {
    let root = Root::new();
    let bus = Bus::new(&root.0).unwrap();
    let mut service = runtime::Service::new(&bus, Catalogue::builtin(), true, "test").unwrap();
    let wall = populate(&bus, 10_000_000);
    let output = service.step(|| 10_000_000, wall);
    let body = output.body.expect("fresh voice and joints");
    body.check().unwrap();
    let light = output.light.expect("fresh light setting");
    light.check().unwrap();
    assert_ne!(body.lease.session_id, "expression");
    assert_eq!(body.lease.session_id, light.lease.session_id);
    assert_eq!(body.lease.clock_id, "test");
    assert!(body.lease.valid_until_us <= 10_200_000);
    let expired = service.step(|| 12_000_000, wall + 2_000);
    assert!(expired.body.is_none() && expired.light.is_none());
    drop(service);
    let mut restarted = runtime::Service::new(&bus, Catalogue::builtin(), true, "test").unwrap();
    // Kept old voice and joints cannot acquire authority after a restart.
    let empty = restarted.step(|| 12_000_000, wall + 2_000);
    assert!(empty.body.is_none() && empty.light.is_none());
    let wall = populate(&bus, 12_100_000);
    let fresh = restarted.step(|| 12_100_000, wall);
    assert_ne!(fresh.body.unwrap().lease.session_id, body.lease.session_id);
}

#[test]
fn two_expression_processes_cannot_own_one_bus() {
    let root = Root::new();
    let bus = Bus::new(&root.0).unwrap();
    let first = runtime::Service::new(&bus, Catalogue::builtin(), false, "test").unwrap();
    assert!(runtime::Service::new(&bus, Catalogue::builtin(), true, "test").is_err());
    drop(first);
    assert!(runtime::Service::new(&bus, Catalogue::builtin(), false, "test").is_ok());
}

#[test]
fn the_voices_remarks_are_read_only_with_the_remark_glance() {
    // #4175: off unless `LAMP_REMARK_GLANCE=on`.
    use lamp_msgs::VoiceRemarked;
    let root = Root::new();
    let bus = Bus::new(&root.0).unwrap();
    let service = runtime::Service::new(&bus, Catalogue::builtin(), false, "test").unwrap();
    assert!(
        bus.subscribers(VoiceRemarked::TOPIC)
            .unwrap_or_default()
            .is_empty()
    );
    let _service = service.with_remark_glance(&bus).unwrap();
    assert_eq!(bus.subscribers(VoiceRemarked::TOPIC).unwrap().len(), 1);
}

#[test]
fn a_remark_is_aged_by_wall_time() {
    let mut item = message(
        "voice/remarked",
        "lamp_msgs/VoiceRemarked",
        serde_json::json!({"request":7}),
    );
    item.header.kind = Kind::Event;
    let input = runtime::input_from_message(&item, 500_000, 1_020, "test", 3).unwrap();
    let lamp_expression::Input::Remarked { stamp, value } = input else {
        panic!("not a remark: {input:?}");
    };
    assert_eq!(stamp.at_us, 480_000);
    assert_eq!(value.request, 7);
}

fn message(topic: &str, kind: &str, payload: serde_json::Value) -> Message {
    Message {
        header: Header {
            topic: topic.into(),
            kind: Kind::Latest,
            enc: "json".into(),
            type_name: Some(kind.into()),
            source: "part".into(),
            boot: "0000000000000001".into(),
            seq: 1,
            at_us: 987_654_321,
            at_unix_ms: 1_000,
            expires_ms: None,
            clock: None,
        },
        payload: serde_json::to_vec(&payload).unwrap(),
        gap: 0,
        publisher_restarted: false,
        expired: false,
    }
}

#[test]
fn older_topics_are_aged_by_wall_time_not_private_publisher_time() {
    let item = message(
        "voice/state",
        "lamp_msgs/VoiceState",
        serde_json::json!({"state":"listening"}),
    );
    let input = runtime::input_from_message(&item, 500_000, 1_020, "test", 8).unwrap();
    assert_eq!(input.stamp().at_us, 480_000);
    assert_eq!(input.stamp().sequence, 8);
    assert_eq!(input.stamp().clock_id, "test");
    let mut future = item.clone();
    future.header.at_unix_ms = 1_100;
    assert!(runtime::input_from_message(&future, 500_000, 1_020, "test", 8).is_err());
    assert!(runtime::input_from_message(&item, 500_000, 4_000, "test", 8).is_err());
    let mut untyped = item;
    untyped.header.type_name = None;
    assert!(runtime::input_from_message(&untyped, 500_000, 1_020, "test", 8).is_err());
}

#[test]
fn typed_timing_keeps_its_shared_stamp_and_bad_inputs_are_counted() {
    let item = message(
        VoiceTiming::TOPIC,
        VoiceTiming::TYPE,
        serde_json::to_value(voice(400_000)).unwrap(),
    );
    assert_eq!(
        runtime::input_from_message(&item, 500_000, 1_020, "test", 8)
            .unwrap()
            .stamp()
            .at_us,
        400_000
    );
    let root = Root::new();
    let bus = Bus::new(&root.0).unwrap();
    let mut service = runtime::Service::new(&bus, Catalogue::builtin(), true, "test").unwrap();
    let mut publisher = bus
        .publisher_declared(VoiceTiming::TOPIC, &VoiceTiming::declaration(), "voice")
        .unwrap();
    publisher
        .publish_json(0, &serde_json::json!({"unexpected":"shape"}))
        .unwrap();
    let output = service.step(|| 1_000_000, runtime::unix_ms());
    assert_eq!(service.counts.refused, 1);
    assert!(output.body.is_none() && output.light.is_none());
}

#[test]
fn boot_identity_cannot_be_empty_or_a_path() {
    assert_eq!(
        runtime::clock_id(Some("test-boot_1")).unwrap(),
        "test-boot_1"
    );
    for id in ["", "two words", "../boot", "new\nboot"] {
        assert!(runtime::clock_id(Some(id)).is_err());
    }
}

#[test]
fn a_saved_light_setting_remains_valid_after_days_without_a_change() {
    let mut item = message(
        "light/command",
        Light::TYPE,
        serde_json::json!({"on":false,"colour":"blue","level":2}),
    );
    item.header.at_unix_ms = 1;
    let input = runtime::input_from_message(&item, 90_000_000_000, 100_000_000, "test", 1).unwrap();
    assert_eq!(input.stamp().at_us, 90_000_000_000);
    let lamp_expression::Input::Light { value, .. } = input else {
        panic!("not a setting");
    };
    assert!(!value.on);
    assert_eq!(value.colour, "blue");
    // An explicit producer expiry still wins over persistence.
    item.header.expires_ms = Some(500);
    assert!(runtime::input_from_message(&item, 90_000_000_000, 100_000_000, "test", 1).is_err());
}

#[test]
fn slow_input_draining_does_not_make_old_joints_fresh() {
    let root = Root::new();
    let bus = Bus::new(&root.0).unwrap();
    let mut service = runtime::Service::new(&bus, Catalogue::builtin(), true, "test").unwrap();
    let wall = populate(&bus, 10_000_000);
    let clock = std::cell::Cell::new(10_000_000_u64);
    let output = service.step(
        || {
            let at = clock.get();
            clock.set(at + 100_000);
            at
        },
        wall,
    );
    // Voice is still under its 500 ms limit, but joints are over their 300 ms
    // limit. Stamping them at receive time would wrongly issue a body command.
    assert!(output.light.is_some());
    assert!(output.body.is_none());
    assert!(
        output
            .state
            .missing
            .iter()
            .any(|reason| reason.contains("joints"))
    );
}

#[test]
fn a_full_output_record_stops_growing_and_counts_loss() {
    let root = Root::new();
    let path = root.0.join("output.jsonl");
    let recorder = runtime::Recorder::new(&path, 0).unwrap();
    let mut engine = lamp_expression::Engine::new(Catalogue::builtin(), false, "test").unwrap();
    recorder.record(0, engine.tick(0));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while recorder.lost.load(Ordering::Relaxed) == 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "record worker did not report limit"
        );
        std::thread::yield_now();
    }
    assert_eq!(fs::metadata(&path).unwrap().len(), 0);
    assert!(runtime::Recorder::new(&path, 1_000).is_err());
}

/// What the planner says of itself for the lamp's health. A note about one
/// turn's gesture is said and is no warning; a fault, a failing bus, or an
/// input a switched-on planner lacks is one. (First run on a lamp, with
/// people in the office: "The lamp has a warning: expression: voice turn
/// cancelled" after a colleague's remark.)
#[test]
fn a_note_about_one_turn_is_not_a_warning_and_a_fault_or_a_lacking_input_is() {
    use lamp_msgs::Level;
    let state = |enabled: bool, missing: &[&str], warnings: &[&str]| ExpressionState {
        stamp: ClockStamp {
            clock_id: "test".into(),
            at_us: 1,
            sequence: 1,
        },
        enabled,
        turn: Some(1),
        request: Some(1),
        behaviours: Vec::new(),
        missing: missing.iter().map(|text| (*text).to_owned()).collect(),
        warnings: warnings.iter().map(|text| (*text).to_owned()).collect(),
        body: enabled,
        light: enabled,
        asleep: None,
    };
    let well = runtime::Counts::default();
    let note = "short audio notice; return shortened, arrival unverified";
    // Watching only: well, whatever it lacks, with the note in the message.
    assert_eq!(
        runtime::standing(&well, None, &state(false, &["measured body lag"], &[note])),
        (
            Level::Ok,
            format!("shadow mode; no actuator requests; noted: {note}")
        )
    );
    assert_eq!(
        runtime::standing(&well, None, &state(false, &[], &[])),
        (Level::Ok, "shadow mode; no actuator requests".to_owned())
    );
    // Switched on: an input it lacks is a warning, a note is not.
    assert_eq!(
        runtime::standing(&well, None, &state(true, &["fresh room state"], &[note])),
        (Level::Warn, "waiting for fresh room state".to_owned())
    );
    let (level, message) = runtime::standing(&well, None, &state(true, &[], &[note]));
    assert_eq!(level, Level::Ok);
    assert!(message.ends_with(&format!("noted: {note}")), "{message}");
    // What a turn waits for in its ordinary course is no warning: most
    // turns in an office get no answer and never schedule a first sound.
    let (level, message) =
        runtime::standing(&well, None, &state(true, &["scheduled first audio"], &[]));
    assert_eq!(level, Level::Ok);
    assert_eq!(
        message,
        "planning; this turn waits for scheduled first audio"
    );
    // With an input really missing beside it, still a warning.
    assert_eq!(
        runtime::standing(
            &well,
            None,
            &state(true, &["scheduled first audio", "measured body lag"], &[])
        )
        .0,
        Level::Warn
    );
    // A fault of the planner's own is a warning in either mode, and so is
    // a bus that fails.
    for enabled in [false, true] {
        assert_eq!(
            runtime::standing(
                &well,
                Some("clock moved backwards"),
                &state(enabled, &[], &[])
            ),
            (Level::Warn, "clock moved backwards".to_owned())
        );
    }
    let failing = runtime::Counts {
        input_errors: 1,
        ..runtime::Counts::default()
    };
    assert_eq!(
        runtime::standing(&failing, None, &state(false, &[], &[])).0,
        Level::Warn
    );
}

/// A lamp listening with nobody speaking to a turn it can keep has no
/// fresh person activity, and is well: a healthy Lamp 2's row in the app
/// read "expression: waiting for fresh person activity" as a warning most
/// of the time. A part a working lamp always has (joints, room, voice
/// timing, light setting, body lag) still warns when it is missing.
#[test]
fn listening_without_person_activity_is_ordinary_and_a_missing_part_still_warns() {
    use lamp_msgs::Level;
    let state = |missing: &[&str]| ExpressionState {
        stamp: ClockStamp {
            clock_id: "test".into(),
            at_us: 1,
            sequence: 1,
        },
        enabled: true,
        turn: Some(1),
        request: Some(1),
        behaviours: Vec::new(),
        missing: missing.iter().map(|text| (*text).to_owned()).collect(),
        warnings: Vec::new(),
        body: true,
        light: true,
        asleep: None,
    };
    let well = runtime::Counts::default();
    assert_eq!(
        runtime::standing(&well, None, &state(&["fresh person activity"])),
        (
            Level::Ok,
            "planning; this turn waits for fresh person activity".to_owned()
        )
    );
    for part in [
        "measured body lag",
        "light setting",
        "fresh voice timing",
        "fresh measured joints",
        "fresh room state",
    ] {
        assert_eq!(
            runtime::standing(&well, None, &state(&[part])),
            (Level::Warn, format!("waiting for {part}")),
            "{part}"
        );
        assert_eq!(
            runtime::standing(&well, None, &state(&["fresh person activity", part])),
            (
                Level::Warn,
                format!("waiting for fresh person activity, {part}")
            ),
            "{part}"
        );
    }
}

mod sleep_name {
    use super::*;
    use lamp_bus::Publisher;
    use lamp_expression::{Input, Output};
    use lamp_msgs::{Faces, NameHeard, PersonActivity, Room, RoomState, VoiceProgress};
    use std::{
        process::{Child, Command, Stdio},
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
        time::{Duration, Instant},
    };

    const TICK_US: u64 = 200_000;
    const SECOND_US: u64 = 1_000_000;
    const START_US: u64 = 1_000 * SECOND_US;

    fn activity(at_us: u64, speaking: bool) -> PersonActivity {
        PersonActivity {
            stamp: ClockStamp {
                clock_id: "test".into(),
                at_us,
                sequence: at_us + 1,
            },
            session_id: "voice_session".into(),
            turn: 1,
            request: 1,
            speaking,
        }
    }

    #[test]
    fn name_adaptation_keeps_publication_delay_and_ago_ms() {
        let value = NameHeard {
            score: 0.99,
            ago_ms: 700,
        };
        let mut item = message(
            NameHeard::TOPIC,
            NameHeard::TYPE,
            serde_json::to_value(value).unwrap(),
        );
        item.header.kind = Kind::Event;
        let Input::Name { stamp, value: got } =
            runtime::input_from_message(&item, 10_000_000, 1_250, "test", 8).unwrap()
        else {
            panic!("name must retain its event variant");
        };
        // Receipt is 250 ms after publication; the sound began a further
        // 700 ms earlier. The publisher's private header.at_us is irrelevant.
        assert_eq!(stamp.at_us, 9_750_000);
        assert_eq!(stamp.clock_id, "test");
        assert_eq!(stamp.sequence, 8);
        assert_eq!(got, value);
        assert_eq!(stamp.at_us - u64::from(got.ago_ms) * 1_000, 9_050_000);
    }

    #[test]
    fn activity_adaptation_keeps_the_source_clock_and_owner() {
        let mut value = activity(9_900_000, true);
        value.stamp.clock_id = "another-boot".into();
        value.stamp.sequence = 42;
        let item = message(
            PersonActivity::TOPIC,
            PersonActivity::TYPE,
            serde_json::to_value(&value).unwrap(),
        );
        let Input::Activity(got) =
            runtime::input_from_message(&item, 10_000_000, 1_250, "test", 8).unwrap()
        else {
            panic!("activity must retain its typed variant");
        };
        // The adapter must leave a foreign clock visible for Engine to reject,
        // not replace it with this service's clock or its wall-age conversion.
        assert_eq!(got, value);
    }

    /// #1670, through the service and its bus: with Mood's state saying the
    /// lamp's one breath is fully in, the head holds at the top of a breath,
    /// where the plain clock's breath goes on rising and falling.
    #[test]
    fn the_service_breathes_the_head_with_moods_breath_from_the_bus() {
        const STEP_US: u64 = 250_000;
        let (mut plain, mut mood) = (SleepingService::new(), SleepingService::with_mood(0.5));
        // In, past the layer's blend and the breath's catch-up.
        for _ in 0..30 {
            plain.step_by(STEP_US);
            mood.step_by(STEP_US);
        }
        let (mut low, mut high) = (f64::MAX, f64::MIN);
        for _ in 0..30 {
            let held = mood.pitch_by(STEP_US);
            assert!((held - -0.03).abs() < 1e-3, "{held}");
            let pitch = plain.pitch_by(STEP_US);
            low = low.min(pitch);
            high = high.max(pitch);
        }
        assert!(high - low > 0.01, "{low} {high}");
        // #1749: Mood at its most lively, through the same bus: the same
        // breath 1.3 times as large (energy 1), held at the top.
        let mut lively = SleepingService::with_mood_energy(0.5, 0.7);
        for _ in 0..30 {
            lively.step_by(STEP_US);
        }
        for _ in 0..30 {
            let held = lively.pitch_by(STEP_US);
            assert!((held - -0.039).abs() < 1e-3, "{held}");
        }
    }

    struct SleepingService {
        service: runtime::Service,
        voice: Publisher,
        joints: Publisher,
        faces: Publisher,
        room: Publisher,
        name: Publisher,
        activity: Publisher,
        /// With `serve --head-touch`: the head's requests and `voice/privacy`.
        head: Option<(Publisher, Publisher)>,
        /// With the lamp's one breath read (#1670): Mood's state, said each
        /// step at this phase and this energy.
        mood: Option<(Publisher, f32, f32)>,
        now_us: u64,
        /// The bus's wall clock, in ms since 1970: what every publish here
        /// is stamped with, and what the service is given. It moves only
        /// with `now_us`, so a host that stalls a publish's file work (its
        /// topic's `latest`) ages no input (#1386).
        wall_ms: Arc<AtomicU64>,
        state: Voice,
        _root: Root,
    }

    impl SleepingService {
        fn new() -> Self {
            Self::with(|service| service)
        }

        fn with(change: fn(runtime::Service) -> runtime::Service) -> Self {
            Self::with_head(change, false)
        }

        /// As [`Self::with`], and with `serve --head-touch` when `head`.
        fn with_head(change: fn(runtime::Service) -> runtime::Service, head: bool) -> Self {
            Self::build(change, head, None)
        }

        /// A service that reads the lamp's one breath, and Mood saying it at
        /// `phase` each step (#1670).
        fn with_mood(phase: f32) -> Self {
            Self::with_mood_energy(phase, 0.3)
        }

        /// As [`Self::with_mood`], with Mood's energy at `energy`, which
        /// sets both the breath's pace and the moves' liveliness.
        fn with_mood_energy(phase: f32, energy: f32) -> Self {
            Self::build(|service| service, false, Some((phase, energy)))
        }

        fn build(
            change: fn(runtime::Service) -> runtime::Service,
            head: bool,
            mood: Option<(f32, f32)>,
        ) -> Self {
            let root = Root::new();
            // The bus reads the fixture's clocks, not the host's (#1386,
            // `Bus::clocks`): a wall that starts at the host's and moves
            // only with each step, and as the monotonic clock that same
            // wall in microseconds.
            let wall_ms = Arc::new(AtomicU64::new(runtime::unix_ms()));
            let bus = Bus::new(&root.0).unwrap().clocks(
                {
                    let wall_ms = wall_ms.clone();
                    move || wall_ms.load(Ordering::Relaxed)
                },
                {
                    let wall_ms = wall_ms.clone();
                    move || wall_ms.load(Ordering::Relaxed) * 1_000
                },
            );
            let mut catalogue =
                Catalogue::from_json(include_str!("../alive-behaviours.json")).unwrap();
            let sleep = catalogue.alive.as_mut().unwrap().sleep.as_mut().unwrap();
            sleep.nobody_us = 60 * SECOND_US;
            sleep.name_requires_speech = true;
            catalogue.check().unwrap();
            // Select a night-time offset without changing the bus's wall.
            let offset = 23 * 60 + 30
                - i32::from(runtime::minute_of_day(wall_ms.load(Ordering::Relaxed), 0));
            let mut service = runtime::Service::new(&bus, catalogue, true, "test")
                .unwrap()
                .with_local_time(offset);
            if head {
                service = service.with_head_touch(&bus).unwrap();
            }
            if mood.is_some() {
                service = service.with_mood_breath(&bus);
            }
            let service = change(service);
            bus.publisher_declared(Light::TOPICS[0], &Light::declaration(), "actions")
                .unwrap()
                .publish_json(987_654_321, &Light::new(true, "warm", 1).unwrap())
                .unwrap();
            Self {
                service,
                voice: bus
                    .publisher_declared(
                        VoiceProgress::TOPIC,
                        &VoiceProgress::declaration(),
                        "voice",
                    )
                    .unwrap(),
                joints: bus
                    .publisher_declared(JointState::TOPIC, &JointState::declaration(), "body")
                    .unwrap(),
                faces: bus
                    .publisher_declared(Faces::TOPIC, &Faces::declaration(), "eyes")
                    .unwrap(),
                room: bus
                    .publisher_declared(RoomState::TOPIC, &RoomState::declaration(), "ears")
                    .unwrap(),
                name: bus
                    .publisher_declared(NameHeard::TOPIC, &NameHeard::declaration(), "name")
                    .unwrap(),
                activity: bus
                    .publisher_declared(
                        PersonActivity::TOPIC,
                        &PersonActivity::declaration(),
                        "voice",
                    )
                    .unwrap(),
                head: head.then(|| {
                    (
                        bus.publisher_declared(
                            lamp_msgs::TouchEvent::TOPIC,
                            &lamp_msgs::TouchEvent::declaration(),
                            "controls",
                        )
                        .unwrap(),
                        bus.publisher_declared(
                            lamp_msgs::MicrophonePrivacy::TOPIC,
                            &lamp_msgs::MicrophonePrivacy::declaration(),
                            "voice",
                        )
                        .unwrap(),
                    )
                }),
                mood: mood.map(|(phase, energy)| {
                    (
                        bus.publisher_declared(
                            lamp_msgs::MoodState::TOPIC,
                            &lamp_msgs::MoodState::declaration(),
                            "mood",
                        )
                        .unwrap(),
                        phase,
                        energy,
                    )
                }),
                now_us: START_US,
                wall_ms,
                state: Voice::Resting,
                _root: root,
            }
        }

        /// `voice/privacy` with the switch closed or open, read at the next
        /// look.
        fn switch(&mut self, closed: bool) {
            let at = self.now_us;
            let (_, privacy) = self.head.as_mut().expect("with the head");
            let lease = lamp_msgs::MicrophonePrivacy {
                stamp: ClockStamp {
                    clock_id: "test".into(),
                    at_us: at,
                    sequence: at,
                },
                session_id: "0123456789abcdef".into(),
                controller_boot: "0123456789abcdef0123456789abcdef".into(),
                microphone_generation: 1,
                observed_us: at - 10_000,
                expires_at_us: at + 30_000,
                permitted: false,
                physical_muted: Some(closed),
                no_slide: false,
            };
            lease.check().unwrap();
            privacy.publish_json(987_654_321, &lease).unwrap();
        }

        /// [`Self::switch`], then a tap on the head, both read at the next
        /// step.
        fn tap_with_switch(&mut self, closed: bool) {
            self.switch(closed);
            self.tap();
        }

        fn tap(&mut self) {
            let at = self.now_us;
            let (taps, _) = self.head.as_mut().expect("with the head");
            let stamp = ClockStamp {
                clock_id: "test".into(),
                at_us: at,
                sequence: at,
            };
            let tap = lamp_msgs::TouchEvent {
                stamp,
                session_id: "controls-one".into(),
                reader_session_id: "gpio".into(),
                enabled: true,
                source: lamp_msgs::TouchSource::Head,
                name: "head".into(),
                contact_id: at,
                gesture: lamp_msgs::TouchGesture::Tap,
                when_speaking: lamp_msgs::TouchIntent::Attend,
                when_quiet: lamp_msgs::TouchIntent::Attend,
                when_asleep: Some(lamp_msgs::TouchIntent::Wake),
                began_us: at - 300_000,
                observed_us: at,
                released_us: Some(at - 150_000),
                active_mask: 0,
                active_names: vec![],
                position: None,
                toward_pad: None,
                distance: 0.0,
                speed: 0.0,
            };
            tap.check().unwrap();
            assert_eq!(taps.publish_json(987_654_321, &tap).unwrap().delivered, 1);
        }

        fn step(&mut self) -> Output {
            self.step_by(TICK_US)
        }

        fn step_by(&mut self, step_us: u64) -> Output {
            self.now_us += step_us;
            // The bus's wall moves with the step, and not while the inputs
            // below are published: each is stamped with `wall`, the one the
            // service is given, however long the host takes over the
            // topics' files. Before, a host stall here aged the first
            // inputs past their limits (#1386), or placed this step's room
            // before the last one's (#1196).
            let wall = self.wall_ms.fetch_add(step_us / 1_000, Ordering::Relaxed) + step_us / 1_000;
            let mut timing = voice(self.now_us);
            timing.stamp.sequence = self.now_us + 1;
            timing.state = self.state;
            // Loosest limit first, strictest last (#912): the room (2 s),
            // the voice and the faces (500 ms), the joints (300 ms).
            self.room
                .publish_json(987_654_321, &RoomState { state: Room::Quiet })
                .unwrap();
            self.voice
                .publish_json(
                    987_654_321,
                    &VoiceProgress {
                        timing,
                        audio_generation: 1,
                        held: false,
                    },
                )
                .unwrap();
            self.faces
                .publish_json(987_654_321, &Faces { faces: vec![] })
                .unwrap();
            self.joints
                .publish_json(
                    987_654_321,
                    &JointState {
                        name: ["base_yaw", "shoulder", "elbow", "head_roll", "head_pitch"]
                            .map(str::to_owned)
                            .to_vec(),
                        position: vec![0.; 5],
                        velocity: vec![],
                        effort: vec![],
                        commanded: vec![],
                    },
                )
                .unwrap();
            if let Some((mood, phase, energy)) = self.mood.as_mut() {
                let state = lamp_msgs::MoodState {
                    stamp: ClockStamp {
                        clock_id: "test".into(),
                        at_us: self.now_us,
                        sequence: self.now_us,
                    },
                    session_id: "mood-session".into(),
                    awake: 0.7,
                    energy: *energy,
                    warmth: 0.6,
                    asleep: false,
                    attention: None,
                    familiarity: lamp_msgs::Familiarity::Unknown,
                    familiar: None,
                    felt: None,
                    breath: Some(lamp_msgs::MoodBreath {
                        phase: *phase,
                        pace: lamp_msgs::mood_pace(0.7, *energy, false),
                    }),
                };
                mood.publish_json(987_654_321, &state).unwrap();
            }
            let now_us = self.now_us;
            let output = self.service.step(|| now_us, wall);
            assert_eq!(self.service.counts.refused, 0);
            assert_eq!(self.service.counts.input_errors, 0);
            output
        }

        /// The head's pitch at a step that admits a new Mood state.
        fn pitch_by(&mut self, step_us: u64) -> f64 {
            self.step_by(step_us)
                .body
                .map_or(0.0, |body| body.offsets.head_pitch)
        }

        fn sleep(&mut self) {
            // Only the injected clock advances: no real minute is spent here.
            for _ in 0..350 {
                if has(&self.step(), "asleep") {
                    self.state = Voice::Listening;
                    assert!(has(&self.step(), "asleep"));
                    return;
                }
            }
            panic!("service never reached sleep within 70 logical seconds");
        }

        fn sleep_after_absence(&mut self) {
            // The first fresh empty look establishes nobody_since. Only then
            // cross the catalogue's minute of absence, publishing fresh bus
            // inputs again rather than ageing the retained first look.
            assert!(!has(&self.step(), "asleep"));
            let mut output = self.step_by(60 * SECOND_US);
            for _ in 0..50 {
                if has(&output, "asleep") {
                    self.state = Voice::Listening;
                    assert!(has(&self.step(), "asleep"));
                    return;
                }
                output = self.step();
            }
            panic!("service never reported sleep after the absence threshold");
        }

        fn name(&mut self) {
            let sent = self
                .name
                .publish_json(
                    987_654_321,
                    &NameHeard {
                        score: 0.99,
                        ago_ms: 400,
                    },
                )
                .unwrap();
            assert_eq!(sent.delivered, 1);
            assert_eq!(sent.dropped, 0);
        }

        fn speech(&mut self, speaking: bool) {
            // The next step receives a 250 ms old typed observation, still
            // within the unchanged 300 ms activity bound. Its envelope is new.
            self.activity
                .publish_json(987_654_321, &activity(self.now_us - 50_000, speaking))
                .unwrap();
        }
    }

    fn has(output: &Output, behaviour: &str) -> bool {
        output.state.behaviours.iter().any(|name| name == behaviour)
    }

    /// #711 (First, 17 Oct 10:18): through the bus, a tap on the head is
    /// weighed with `voice/privacy` as it is at that tap. Closed, no look
    /// up; open, the look up as before.
    #[test]
    fn a_tap_while_the_privacy_switch_is_closed_gets_no_look_up() {
        for closed in [false, true] {
            let mut lamp = SleepingService::with_head(|service| service, true);
            for _ in 0..5 {
                lamp.step();
            }
            lamp.tap_with_switch(closed);
            let looked = (0..5).any(|_| has(&lamp.step(), "looking up at a tap"));
            assert_eq!(looked, !closed, "closed: {closed}");
        }
    }

    #[test]
    fn a_privacy_read_error_refuses_a_tap_until_a_valid_open_lease() {
        use lamp_msgs::MicrophonePrivacy;

        let mut lamp = SleepingService::with_head(|service| service, true);
        let bus = Bus::new(&lamp._root.0).unwrap();
        for _ in 0..5 {
            lamp.step();
        }
        let switch = |lamp: &mut SleepingService, closed: bool| {
            let at = lamp.now_us;
            let (_, publisher) = lamp.head.as_mut().unwrap();
            let lease = MicrophonePrivacy {
                stamp: ClockStamp {
                    clock_id: "test".into(),
                    at_us: at,
                    sequence: publisher.totals().published + 1,
                },
                session_id: publisher.boot().to_owned(),
                controller_boot: "0123456789abcdef0123456789abcdef".into(),
                microphone_generation: 1,
                observed_us: at,
                expires_at_us: at + 50_000,
                permitted: !closed,
                physical_muted: Some(closed),
                no_slide: false,
            };
            lease.check().unwrap();
            publisher.publish_json(at, &lease).unwrap();
            let message = bus.latest(MicrophonePrivacy::TOPIC).unwrap().unwrap();
            assert_eq!(message.header.boot, lease.session_id);
            assert_eq!(message.header.seq, lease.stamp.sequence);
            assert_eq!(message.header.at_us, at);
            assert_eq!(message.parse::<MicrophonePrivacy>().unwrap(), lease);
        };

        switch(&mut lamp, true);
        lamp.tap();
        assert!(!has(&lamp.step(), "looking up at a tap"));

        // The retained envelope cannot be read. This is an actual Bus error,
        // distinct from no publisher or a parseable envelope with a bad payload.
        fs::write(lamp._root.0.join("voice/privacy/latest"), b"broken\n").unwrap();
        assert_eq!(
            bus.latest(MicrophonePrivacy::TOPIC).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        lamp.tap();
        for _ in 0..5 {
            assert!(
                !has(&lamp.step(), "looking up at a tap"),
                "a privacy read error must not reopen the tap's look-up"
            );
        }

        switch(&mut lamp, false);
        lamp.tap();
        assert!(
            (0..5).any(|_| has(&lamp.step(), "looking up at a tap")),
            "a new tap works after a valid open lease replaces the unreadable envelope"
        );

        // No new tap: an unreadable envelope during that accepted look must
        // end it with the same bounded ease-out as a valid closed lease.
        fs::write(lamp._root.0.join("voice/privacy/latest"), b"broken\n").unwrap();
        assert_eq!(
            bus.latest(MicrophonePrivacy::TOPIC).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        let steps = (0..20)
            .position(|_| !has(&lamp.step(), "looking up at a tap"))
            .expect("the unreadable privacy envelope ends the active look");
        // The valid-close control below has this same bound: half a second
        // to ease out, well before the uninterrupted look's two seconds.
        assert!(
            steps <= 4,
            "privacy read error left the look up for {steps} steps"
        );
        switch(&mut lamp, false);
        for _ in 0..5 {
            assert!(
                !has(&lamp.step(), "looking up at a tap"),
                "reopening privacy without a new tap must not revive the cancelled look"
            );
        }
    }

    /// The reviewer and the seventh on #1282: the switch closed while it
    /// looks up ends the look, since the switch is read at each step of a
    /// look; it is down well before the look's own 2 s.
    #[test]
    fn the_switch_closed_mid_look_ends_it() {
        let mut lamp = SleepingService::with_head(|service| service, true);
        for _ in 0..5 {
            lamp.step();
        }
        lamp.tap_with_switch(false);
        assert!(has(&lamp.step(), "looking up at a tap"));
        lamp.switch(true);
        // Eased out over half a second (`LOOK_UP_OUT_US`), three 200 ms
        // steps; left alone, the look would hold to its 2 s.
        let steps = (0..20)
            .position(|_| !has(&lamp.step(), "looking up at a tap"))
            .expect("the look ended");
        assert!(steps <= 4, "{steps}");
    }

    #[test]
    fn sleeping_name_requires_validated_speech_through_the_local_bus() {
        let mut lamp = SleepingService::new();
        lamp.sleep();
        lamp.name();
        assert!(has(&lamp.step(), "asleep"));
        lamp.speech(false);
        assert!(has(&lamp.step(), "asleep"));
        lamp.speech(true);
        let output = lamp.step();
        assert!(has(&output, "waking"));
        assert!(!has(&output, "asleep"));
    }

    /// The reviewer's case on #884: the clock was set at the first look and
    /// is found not set at the next. The time of day given at the first is
    /// not carried on for its freshness: no night, so no sleep.
    #[test]
    fn a_clock_found_not_set_forgets_the_time_of_day_it_had() {
        static LOOKS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        fn set_only_first() -> Option<bool> {
            Some(LOOKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0)
        }
        let mut lamp = SleepingService::with(|service| service.with_clock_check(set_only_first));
        for _ in 0..350 {
            assert!(!has(&lamp.step(), "asleep"));
        }
        assert!(
            LOOKS.load(std::sync::atomic::Ordering::Relaxed) >= 2,
            "looked again"
        );
    }

    #[test]
    fn a_clock_not_set_gives_no_night_and_a_set_one_does() {
        // The same night-time hour: asleep with a set clock, never without.
        SleepingService::with(|service| service.with_clock_check(|| Some(true))).sleep();
        let unset = [
            SleepingService::with(|service| service.with_clock_check(|| Some(false))),
            SleepingService::with(|service| service.with_clock_check(|| None)),
        ];
        for mut lamp in unset {
            for _ in 0..350 {
                assert!(!has(&lamp.step(), "asleep"));
            }
        }
    }

    struct JournalChild(Child);

    impl Drop for JournalChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn an_expired_sleeping_name_is_journaled_once_by_the_service() {
        const CHILD: &str = "LAMP_EXPRESSION_SLEEP_NAME_JOURNAL_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let mut lamp = SleepingService::new();
            lamp.sleep_after_absence();
            lamp.name();
            // Beyond the two-second source window and 300 ms delivery grace,
            // then several further ticks: one rejection must not repeat.
            for _ in 0..25 {
                assert!(has(&lamp.step(), "asleep"));
            }
            return;
        }
        // Capture the real Service stderr in a separate test process so the
        // test harness and parallel cases keep their own descriptors intact.
        let root = Root::new();
        let journal = root.0.join("journal.txt");
        let mut child = JournalChild(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "sleep_name::an_expired_sleeping_name_is_journaled_once_by_the_service",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(CHILD, "1")
                .stdout(Stdio::null())
                .stderr(Stdio::from(fs::File::create(&journal).unwrap()))
                .spawn()
                .unwrap(),
        );
        // A hang guard only: the test checks what the child journaled. Its
        // run is about 0.2 s alone, but every step's four publishes rewrite
        // their topics' `latest` files, and beside eight processes' file
        // churn on a Mac it took 57 to 82 s, journaling as it should (#1386).
        let deadline = Instant::now() + Duration::from_secs(120);
        let status = loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "journal fixture host watchdog");
            std::thread::sleep(Duration::from_millis(5));
        };
        let lines = fs::read_to_string(&journal).unwrap();
        assert!(status.success(), "journal fixture failed: {lines}");
        assert_eq!(
            lines.lines().collect::<Vec<_>>(),
            [
                "lamp-expression: ignored sleeping name (1): no validated person speech within +/-2 s"
            ]
        );
    }
}

/// The keeper's end of `lamp/pause`.
fn keeper(bus: &Bus) -> lamp_bus::Publisher {
    bus.publisher_declared(
        lamp_msgs::Pause::TOPIC,
        &lamp_msgs::Pause::declaration(),
        "keeper",
    )
    .unwrap()
}

#[test]
fn paused_by_the_keeper_nothing_is_asked_of_the_body_or_the_light_until_resumed() {
    let root = Root::new();
    let bus = Bus::new(&root.0).unwrap();
    let wall = populate(&bus, 10_000_000);
    let mut keeper = keeper(&bus);
    keeper
        .publish_json(1, &lamp_msgs::Pause { paused: true })
        .unwrap();
    let mut service = runtime::Service::new(&bus, Catalogue::builtin(), true, "test").unwrap();
    let paused = service.step(|| 10_000_000, wall);
    assert!(paused.body.is_none() && paused.light.is_none());
    assert!(bus.latest(ExpressionBody::TOPIC).unwrap().is_none());
    assert!(bus.latest(ExpressionLight::TOPIC).unwrap().is_none());
    keeper
        .publish_json(2, &lamp_msgs::Pause { paused: false })
        .unwrap();
    // The pause is looked at once a second, as the body looks at it.
    std::thread::sleep(std::time::Duration::from_millis(1_100));
    let wall = populate(&bus, 10_100_000);
    let resumed = service.step(|| 10_100_000, wall);
    assert!(resumed.body.is_some() && resumed.light.is_some());
    assert!(bus.latest(ExpressionBody::TOPIC).unwrap().is_some());
    assert!(bus.latest(ExpressionLight::TOPIC).unwrap().is_some());
}

#[test]
fn without_a_pause_said_expression_asks_exactly_as_before() {
    // Nothing kept on `lamp/pause`, and a kept "not paused": the same
    // requests, but for the lease each new session names.
    let asked = |said: Option<bool>| {
        let root = Root::new();
        let bus = Bus::new(&root.0).unwrap();
        if let Some(paused) = said {
            keeper(&bus)
                .publish_json(1, &lamp_msgs::Pause { paused })
                .unwrap();
        }
        let mut service = runtime::Service::new(&bus, Catalogue::builtin(), true, "test").unwrap();
        let wall = populate(&bus, 10_000_000);
        let output = service.step(|| 10_000_000, wall);
        let mut body = serde_json::to_value(output.body.expect("a body request")).unwrap();
        let mut light = serde_json::to_value(output.light.expect("a light request")).unwrap();
        body["lease"].take();
        light["lease"].take();
        (body, light)
    };
    assert_eq!(asked(None), asked(Some(false)));
}

/// #830, through the service: the same fresh "offline" on `lamp/online`
/// leaves a service without `--offline-face` drawing listening as ready, and
/// makes the opted-in service draw it as unavailable.
#[test]
fn only_a_service_with_the_offline_face_draws_listening_unavailable_when_offline() {
    let face = |offline_face: bool| {
        let root = Root::new();
        let bus = Bus::new(&root.0).unwrap();
        let mut service = runtime::Service::new(&bus, Catalogue::builtin(), true, "test").unwrap();
        if offline_face {
            service = service.with_offline_face(&bus);
        }
        bus.publisher_declared(
            lamp_msgs::Online::TOPIC,
            &lamp_msgs::Online::declaration(),
            "health",
        )
        .unwrap()
        .publish_json(
            1,
            &lamp_msgs::Online {
                online: Some(false),
                from: "voice".into(),
                down_for_s: Some(12),
            },
        )
        .unwrap();
        let wall = populate(&bus, 10_000_000);
        service
            .step(|| 10_000_000, wall)
            .light
            .expect("a light request")
            .face
    };
    assert_eq!(face(false), lamp_msgs::ExpressionFace::Listening);
    assert_eq!(face(true), lamp_msgs::ExpressionFace::Unavailable);
}

/// #711: with `--head-touch`, a request from the head is weighed with the
/// last `voice/privacy` said: closed, unknown or invalid counts as closed
/// however late (no sender's trouble reads as open, the reviewer on #1282
/// and #1301); only a valid open lease, or nothing ever said, is open.
#[test]
fn a_closed_switch_reads_from_the_last_privacy_lease_said() {
    let root = Root::new();
    let bus = Bus::new(&root.0).unwrap();
    let latest = || bus.latest(lamp_msgs::MicrophonePrivacy::TOPIC).unwrap();
    assert!(!runtime::privacy_muted_in(latest()));
    let mut out = bus
        .publisher_declared(
            lamp_msgs::MicrophonePrivacy::TOPIC,
            &lamp_msgs::MicrophonePrivacy::declaration(),
            "voice",
        )
        .unwrap();
    let lease = |sequence, physical_muted: Option<bool>| lamp_msgs::MicrophonePrivacy {
        stamp: ClockStamp {
            clock_id: "boot".into(),
            at_us: 1_000_000,
            sequence,
        },
        session_id: "0123456789abcdef".into(),
        controller_boot: "0123456789abcdef0123456789abcdef".into(),
        microphone_generation: 1,
        observed_us: if physical_muted.is_some() { 990_000 } else { 0 },
        expires_at_us: 1_030_000,
        permitted: false,
        physical_muted,
        no_slide: false,
    };
    for (sequence, physical_muted, closed) in [
        (1, Some(true), true),
        (2, Some(false), false),
        (3, None, true),
    ] {
        let said = lease(sequence, physical_muted);
        said.check().unwrap();
        out.publish_json(sequence, &said).unwrap();
        assert_eq!(
            runtime::privacy_muted_in(latest()),
            closed,
            "{physical_muted:?}"
        );
    }
    // Closed, then past the lease's 50 ms and the bus's 50 ms of grace:
    // still closed, until an open one replaces it.
    out.publish_json(4, &lease(4, Some(true))).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(150));
    assert!(latest().unwrap().expired);
    assert!(runtime::privacy_muted_in(latest()));
    out.publish_json(5, &lease(5, Some(false))).unwrap();
    assert!(!runtime::privacy_muted_in(latest()));
    // A lease that fails its check, said after an open one: closed, the
    // safe side (the reviewer on #1301).
    let mut bad = lease(6, Some(false));
    bad.microphone_generation = 0;
    assert!(bad.check().is_err());
    out.publish_json(6, &bad).unwrap();
    assert!(runtime::privacy_muted_in(latest()));
    // A lamp with no slide (`no-slide`): its valid lease is open, muted by
    // software or not, since it has no switch to close, as a lamp without
    // the route. Its unknown lease, with the Controls word stale, is closed.
    for (sequence, permitted) in [(7, true), (8, false)] {
        let mut no_slide = lease(sequence, None);
        no_slide.no_slide = true;
        no_slide.observed_us = 990_000;
        no_slide.permitted = permitted;
        no_slide.check().unwrap();
        out.publish_json(sequence, &no_slide).unwrap();
        assert!(!runtime::privacy_muted_in(latest()), "{permitted}");
    }
    out.publish_json(9, &lease(9, None)).unwrap();
    assert!(runtime::privacy_muted_in(latest()));
}

/// #830: `--offline-face` reads `lamp/online`: only a fresh, valid "offline"
/// counts; online, unknown or nothing published does not.
#[test]
fn offline_face_reads_only_a_fresh_offline_from_lamp_online() {
    let root = Root::new();
    let bus = Bus::new(&root.0).unwrap();
    assert!(!runtime::offline_in(
        bus.latest(lamp_msgs::Online::TOPIC).unwrap()
    ));
    let mut out = bus
        .publisher_declared(
            lamp_msgs::Online::TOPIC,
            &lamp_msgs::Online::declaration(),
            "health",
        )
        .unwrap();
    for (online, from, down, offline) in [
        (Some(false), "voice", Some(12), true),
        (Some(true), "voice", None, false),
        (None, "", None, false),
    ] {
        out.publish_json(
            1,
            &lamp_msgs::Online {
                online,
                from: from.into(),
                down_for_s: down,
            },
        )
        .unwrap();
        assert_eq!(
            runtime::offline_in(bus.latest(lamp_msgs::Online::TOPIC).unwrap()),
            offline,
            "{online:?}"
        );
    }
}

/// #1670: the service takes the lamp's one breath and its liveliness from
/// Mood's state only: fresh, valid and from `mood`; else neither.
#[test]
fn mood_in_reads_only_a_fresh_valid_state_from_mood() {
    use lamp_msgs::{Familiarity, MOOD_STATE_TTL_US, MoodBreath, MoodState};
    let root = Root::new();
    let bus = Bus::new(&root.0).unwrap();
    let latest = || bus.latest(MoodState::TOPIC).unwrap();
    assert_eq!(runtime::mood_in(latest().as_ref(), 5_000_000), (None, None));
    let state = |energy: f32, sequence: u64| MoodState {
        stamp: ClockStamp {
            clock_id: "boot".into(),
            at_us: 5_000_000,
            sequence,
        },
        session_id: "s".into(),
        awake: 0.85,
        energy,
        warmth: 0.6,
        asleep: false,
        attention: None,
        familiarity: Familiarity::Unknown,
        familiar: None,
        felt: None,
        breath: Some(MoodBreath {
            phase: 0.5,
            pace: lamp_msgs::mood_pace(0.85, energy, false),
        }),
    };
    let mut mood = bus
        .publisher_declared(MoodState::TOPIC, &MoodState::declaration(), "mood")
        .unwrap();
    mood.publish_json(1, &state(0.70, 1)).unwrap();
    // At its stamp: its phase, and the most lively.
    assert_eq!(
        runtime::mood_in(latest().as_ref(), 5_000_000),
        (Some(0.5), Some(0.5))
    );
    // Before its stamp or past its 2.5 s: nothing.
    assert_eq!(runtime::mood_in(latest().as_ref(), 4_999_999), (None, None));
    assert_eq!(
        runtime::mood_in(latest().as_ref(), 5_000_000 + MOOD_STATE_TTL_US),
        (None, None)
    );
    // Calm: the catalogue's own style.
    mood.publish_json(2, &state(0.30, 2)).unwrap();
    assert_eq!(
        runtime::mood_in(latest().as_ref(), 5_000_000),
        (Some(0.5), Some(0.0))
    );
    // From anyone but Mood: nothing.
    bus.publisher_declared(MoodState::TOPIC, &MoodState::declaration(), "replay")
        .unwrap()
        .publish_json(3, &state(0.70, 3))
        .unwrap();
    assert_eq!(runtime::mood_in(latest().as_ref(), 5_000_000), (None, None));
}

/// The journal says the liveliness the idle layer takes from Mood, first and
/// on a change, and nothing with no fresh state: Mood off writes nothing new
/// (the first builder on #1722, 03:54).
#[test]
fn the_liveliness_taken_is_said_and_nothing_with_mood_off() {
    let mut said = lamp_msgs::MoodTaken::default();
    for at in [0, 60_000_000, 600_000_000] {
        assert_eq!(runtime::liveliness_line(&mut said, None, at), None);
    }
    assert_eq!(
        runtime::liveliness_line(&mut said, Some(-0.31), 1).as_deref(),
        Some("lamp-expression: Mood's liveliness -0.31")
    );
    assert_eq!(
        runtime::liveliness_line(&mut said, Some(-0.3), 120_000_000),
        None
    );
    assert!(runtime::liveliness_line(&mut said, Some(0.5), 120_000_000).is_some());
    // A change 59 s after the last line waits for the minute.
    assert_eq!(
        runtime::liveliness_line(&mut said, Some(-0.3), 179_000_000),
        None
    );
    // Mood gone, then back at the same liveliness: said again after a minute.
    assert_eq!(runtime::liveliness_line(&mut said, None, 200_000_000), None);
    assert!(runtime::liveliness_line(&mut said, Some(0.5), 200_000_001).is_some());
}
