//! `lamp-addons serve` on a bus of its own: a proposal written as lamp-skills
//! writes it, the lamp's state published as its parts publish it, and what the
//! judge asks of the ring read back.
use lamp_bus::Bus;
use lamp_msgs::{
    AddonDecision, AddonLight, AddonProposal, ClockStamp, Light, LightRequest, MicrophonePrivacy,
    Pause, Priority, Room, RoomState, Voice, VoiceMute, VoiceState,
};
use std::{
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn judge(root: &Path, enabled: bool) -> Running {
    let mut command = Command::new(env!("CARGO_BIN_EXE_lamp-addons"));
    command
        .args(["serve", "--clock-id", "test-boot", "--bus-root"])
        .arg(root)
        .env_remove("LAMP_ADDONS")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if enabled {
        command.arg("--enable");
    }
    Running(command.spawn().unwrap())
}

fn glow(proposal: u64) -> AddonProposal {
    AddonProposal {
        proposal,
        addon: "evening-glow".into(),
        version: "1.0.0".into(),
        act: "light".into(),
        why: "evening".into(),
        sure: 0.6,
        holds_ms: 30_000,
        light: Some(AddonLight {
            colour: "warm".into(),
            level: 2,
            pattern: None,
        }),
        gesture: None,
        mood: None,
        note: None,
    }
}

/// A calm lamp at rest, freshly said. `product`: as a product lamp says it,
/// with no `lamp/pause` (only `lamp-keeper` says one) and no `voice/mute`
/// (only the head's controls say one).
fn calm(bus: &Bus, persons_light: bool, product: bool) {
    let mut privacy = bus
        .publisher_declared(
            MicrophonePrivacy::TOPIC,
            &MicrophonePrivacy::declaration(),
            "voice",
        )
        .unwrap();
    let now = lamp_ipc::monotonic_us();
    privacy
        .publish_json(
            now,
            &MicrophonePrivacy {
                stamp: ClockStamp {
                    clock_id: "test-boot".into(),
                    at_us: now,
                    sequence: 1,
                },
                session_id: "0123456789abcdef".into(),
                controller_boot: "0123456789abcdef0123456789abcdef".into(),
                microphone_generation: 1,
                observed_us: now,
                expires_at_us: now + 50_000,
                permitted: true,
                physical_muted: Some(false),
                no_slide: false,
            },
        )
        .unwrap();
    if !product {
        bus.publisher_declared(Pause::TOPIC, &Pause::declaration(), "keeper")
            .unwrap()
            .publish_json(now, &Pause { paused: false })
            .unwrap();
        bus.publisher_declared(VoiceMute::TOPIC, &VoiceMute::declaration(), "voice")
            .unwrap()
            .publish_json(
                now,
                &VoiceMute {
                    muted: false,
                    reason: None,
                },
            )
            .unwrap();
    }
    bus.publisher_declared(VoiceState::TOPIC, &VoiceState::declaration(), "voice")
        .unwrap()
        .publish_json(
            now,
            &VoiceState {
                state: Voice::Resting,
            },
        )
        .unwrap();
    bus.publisher_declared(RoomState::TOPIC, &RoomState::declaration(), "voice")
        .unwrap()
        .publish_json(now, &RoomState { state: Room::Quiet })
        .unwrap();
    if persons_light {
        let blue = Light::new(true, "blue", 3).unwrap();
        bus.publisher_declared("light/command", &Light::declaration(), "actions")
            .unwrap()
            .publish_json(now, &blue)
            .unwrap();
        bus.publisher_declared("light/state", &Light::declaration(), "ring")
            .unwrap()
            .publish_json(now, &blue)
            .unwrap();
    }
}

/// What the ring was asked after one proposal, with the judge on or off.
fn asked(enabled: bool, persons_light: bool) -> Vec<LightRequest> {
    judged(enabled, persons_light).0
}

/// And what the judge decided.
fn judged(enabled: bool, persons_light: bool) -> (Vec<LightRequest>, Vec<AddonDecision>) {
    run(enabled, persons_light, false, "skills")
}

/// One proposal written by `source`, on a lamp said in full or as a
/// product lamp says it.
fn run(
    enabled: bool,
    persons_light: bool,
    product: bool,
    source: &str,
) -> (Vec<LightRequest>, Vec<AddonDecision>) {
    let dir = tempfile::Builder::new()
        .prefix("ad")
        .tempdir_in("/tmp")
        .unwrap();
    let bus = Bus::new(dir.path()).unwrap();
    let mut ring = bus.subscribe(LightRequest::TOPIC, "ring").unwrap();
    let mut said = bus.subscribe(AddonDecision::TOPIC, "test").unwrap();
    let _judge = judge(dir.path(), enabled);
    // It subscribes to addon/proposal once it runs; off, it never does.
    let deadline = Instant::now() + Duration::from_secs(if enabled { 10 } else { 2 });
    while Instant::now() < deadline && bus.subscribers(AddonProposal::TOPIC).unwrap().is_empty() {
        std::thread::sleep(Duration::from_millis(50));
    }
    if product {
        // Long enough that a sleep mute never said is known never said.
        std::thread::sleep(Duration::from_millis(3_200));
    }
    calm(&bus, persons_light, product);
    std::thread::sleep(Duration::from_millis(300));
    let mut skills = bus
        .publisher_declared(AddonProposal::TOPIC, &AddonProposal::declaration(), source)
        .unwrap();
    skills
        .publish_json(lamp_ipc::monotonic_us(), &glow(lamp_ipc::monotonic_us()))
        .unwrap();
    let mut requests = Vec::new();
    while let Some(message) = ring.recv(Duration::from_millis(1_500)).unwrap() {
        requests.push(message.parse().unwrap());
    }
    let mut decisions = Vec::new();
    while let Some(message) = said.recv(Duration::from_millis(300)).unwrap() {
        decisions.push(message.parse().unwrap());
    }
    (requests, decisions)
}

#[test]
fn a_calm_lamp_keeps_an_add_ons_light_and_asks_it_of_the_ring_at_idle() {
    let requests = asked(true, false);
    let [request] = requests.as_slice() else {
        panic!("{requests:?}");
    };
    assert_eq!(request.claim.source, "addon:evening-glow");
    assert_eq!(request.claim.priority, Priority::Idle);
    assert_eq!(request.claim.clock_id, "test-boot");
    assert_eq!(request.light, Light::new(true, "warm", 2).unwrap());
}

#[test]
fn the_persons_own_light_wins_and_nothing_is_asked() {
    let (requests, decisions) = judged(true, true);
    assert!(requests.is_empty());
    let [decision] = decisions.as_slice() else {
        panic!("{decisions:?}");
    };
    assert_eq!(
        (decision.choice.as_str(), decision.by.as_str()),
        ("dropped", "rule")
    );
    assert!(decision.why.contains("the person's own"), "{decision:?}");
    assert_eq!(decision.check(), Ok(()));
}

#[test]
fn a_kept_light_is_said_kept_on_addon_decision() {
    let (_, decisions) = judged(true, false);
    let [decision] = decisions.as_slice() else {
        panic!("{decisions:?}");
    };
    assert_eq!(
        (decision.choice.as_str(), decision.addon.as_str()),
        ("kept", "evening-glow")
    );
    assert_eq!(decision.check(), Ok(()));
}

/// A fresh product lamp: nobody has paused it, and its voice has no sleep
/// mute to say. The reviewer-lead's P2 on #2553.
#[test]
fn a_product_lamp_with_no_pause_or_mute_said_keeps_a_light() {
    let (requests, decisions) = run(true, false, true, "skills");
    assert_eq!(requests.len(), 1, "{decisions:?}");
    assert_eq!(decisions[0].choice, "kept", "{decisions:?}");
}

/// Only lamp-skills writes proposals; any other writer's is not judged.
#[test]
fn a_proposal_not_from_lamp_skills_is_not_judged() {
    let (requests, decisions) = run(true, false, false, "voice");
    assert!(requests.is_empty() && decisions.is_empty(), "{decisions:?}");
}

#[test]
fn without_the_switch_nothing_runs_and_nothing_is_asked() {
    assert!(asked(false, false).is_empty());
}
