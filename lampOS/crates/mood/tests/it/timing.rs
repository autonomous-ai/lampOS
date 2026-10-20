//! Manual host measurements, not lamp or acoustic latency qualification.
use lamp_bus::Bus;
use lamp_mood::{
    engine::{Engine, Event, EventKind},
    runtime::{Clock, SystemClock},
};
use lamp_msgs::{
    AttentionTarget, ClockStamp, ControlGesture, ControlIntent, ControlSource, ControlsState,
    Gesture, HoldTier, MoodState, NameHeard,
};
use std::{
    hint::black_box,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

fn report(name: &str, unit: &str, samples: &[u128]) -> u128 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let percentile = |n: usize| sorted[(sorted.len() * n).div_ceil(100).saturating_sub(1)];
    println!(
        "{}",
        serde_json::json!({"case":name,"unit":unit,
        "count":samples.len(),"first":samples[0],"p50":percentile(50),
        "p95":percentile(95),"p99":percentile(99),"max":sorted.last(),"samples":samples})
    );
    percentile(95)
}

#[test]
#[ignore = "manual release-mode host timing; run alone and preserve raw output"]
fn reducer_first_use_repeated_actions_and_long_gap() {
    let mut fresh = Vec::new();
    let mut repeated = Vec::new();
    let mut long_gap = Vec::new();
    let mut engine = Engine::new("timing-host".into(), 1, Some(540)).unwrap();
    for n in 1..=2000u64 {
        let now = n * 1000 + 1;
        let event = Event {
            observed_us: now,
            valid_until_us: now + 1_000_000,
            kind: if n % 2 == 0 {
                EventKind::Name
            } else {
                EventKind::Touch { kind: false }
            },
        };
        let started = Instant::now();
        let mut first = Engine::new("timing-host".into(), now - 1, Some(540)).unwrap();
        first.apply(now, event.clone()).unwrap();
        black_box(first.snapshot(now, 1, "1234567890abcdef").unwrap());
        fresh.push(started.elapsed().as_nanos());

        let started = Instant::now();
        engine.apply(now, event).unwrap();
        black_box(engine.snapshot(now, n, "1234567890abcdef").unwrap());
        repeated.push(started.elapsed().as_nanos());

        let started = Instant::now();
        first.advance(now + 86_400_000_000, Some(540)).unwrap();
        black_box(
            first
                .snapshot(now + 86_400_000_000, 2, "1234567890abcdef")
                .unwrap(),
        );
        long_gap.push(started.elapsed().as_nanos());
    }
    for (name, values) in [
        ("fresh_engine_apply_snapshot", fresh),
        ("existing_engine_apply_snapshot", repeated),
        ("24h_gap_advance_snapshot", long_gap),
    ] {
        assert!(
            report(name, "nanoseconds", &values) < 1_000_000,
            "host reducer p95 exceeded the 1ms target"
        );
    }
}

struct Owner(Child);
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "manual local-process bus timing; run alone with Unix sockets allowed"]
fn authored_bus_cues_to_observed_state_first_and_repeated() {
    let root = tempfile::Builder::new()
        .prefix("mot")
        .tempdir_in("/tmp")
        .unwrap();
    let bus = Bus::new(root.path()).unwrap();
    let clock = SystemClock::new("UTC", Some("mood-timing-host")).unwrap();
    let mut states = bus
        .subscribe_typed(MoodState::TOPIC, "timing", MoodState::TYPE)
        .unwrap();
    let mut names = bus
        .publisher_declared(NameHeard::TOPIC, &NameHeard::declaration(), "name")
        .unwrap();
    let mut controls = bus
        .publisher_declared(
            ControlsState::TOPIC,
            &ControlsState::declaration(),
            "controls",
        )
        .unwrap();
    let startup = Instant::now();
    let _owner = Owner(
        Command::new(env!("CARGO_BIN_EXE_lamp-mood"))
            .env_clear()
            .args([
                "serve",
                "--enable",
                "--zone",
                "UTC",
                "--clock-id",
                "mood-timing-host",
                "--run-for-ms",
                "30000",
            ])
            .arg("--bus-root")
            .arg(root.path())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let initial = loop {
        assert!(
            startup.elapsed() < Duration::from_secs(5),
            "no initial state"
        );
        if let Some(message) = states.recv(Duration::from_millis(20)).unwrap() {
            let state: MoodState = message.parse().unwrap();
            state.check().unwrap();
            assert_eq!(state.stamp.clock_id, clock.clock_id());
            assert_eq!(state.session_id, message.header.boot);
            break state;
        }
    };
    let mut last_sequence = initial.stamp.sequence;
    println!(
        "{}",
        serde_json::json!({"case":"process_start_to_first_observed_state",
        "microseconds":startup.elapsed().as_micros(),"boundary":"before spawn to first parsed bus message"})
    );
    let mut times = Vec::new();
    let mut touch_sequence = 0u64;
    for n in 0..101 {
        let now = clock.sample().unwrap().now_us;
        let touching = n % 2 == 1;
        let started = Instant::now();
        if touching {
            touch_sequence += 1;
            let stamp = ClockStamp {
                clock_id: clock.clock_id().into(),
                at_us: now,
                sequence: touch_sequence,
            };
            let gesture = ControlGesture {
                stamp: stamp.clone(),
                session_id: controls.boot().into(),
                source: ControlSource::Base,
                name: "authored-base".into(),
                gesture: Gesture::Tap,
                intent: ControlIntent::Stop,
                duration_us: 1_000,
                released_us: now,
                enabled: true,
            };
            let state = ControlsState {
                stamp,
                session_id: controls.boot().into(),
                enabled: true,
                ready: true,
                fault: None,
                buttons: Vec::new(),
                head_active: false,
                base_ready: false,
                base_held_us: None,
                base_tier: HoldTier::None,
                latest_stop: Some(gesture),
                latest_mute: None,
                event_sequence: touch_sequence,
                rejected_inputs: 0,
                privacy: None,
                no_privacy_slide: false,
            };
            controls.publish_json(now, &state).unwrap();
        } else {
            names
                .publish_json(
                    now,
                    &NameHeard {
                        score: 1.0,
                        ago_ms: 0,
                    },
                )
                .unwrap();
        }
        loop {
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "cue {n} produced no matching state"
            );
            if let Some(message) = states.recv(Duration::from_millis(10)).unwrap() {
                let state: MoodState = message.parse().unwrap();
                state.check().unwrap();
                assert_eq!(state.stamp.clock_id, clock.clock_id());
                assert_eq!(state.session_id, initial.session_id);
                assert_eq!(state.session_id, message.header.boot);
                let wanted = if touching {
                    AttentionTarget::Touch
                } else {
                    AttentionTarget::Sound
                };
                if state.stamp.sequence > last_sequence
                    && state.stamp.at_us >= now
                    && state.attention == Some(wanted)
                {
                    last_sequence = state.stamp.sequence;
                    times.push(started.elapsed().as_micros());
                    break;
                }
            }
        }
    }
    println!(
        "boundary: before authored cue publication to subscriber receipt of matching Mood state; includes bus serialization, file I/O and receiver scheduling; no sensor/provider/actuator"
    );
    let p95 = report(
        "cue_publish_to_matching_state_received",
        "microseconds",
        &times,
    );
    assert!(
        p95 < 50_000,
        "host metadata proxy p95 exceeded the 50ms target"
    );
}
