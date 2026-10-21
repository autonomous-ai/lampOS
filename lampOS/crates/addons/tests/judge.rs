//! `lamp-addons serve` on a bus of its own: a proposal written as lamp-skills
//! writes it, the lamp's state published as its parts publish it, and what the
//! judge asks of the ring read back.
use lamp_bus::Bus;
use lamp_msgs::{
    AddonAsk, AddonDecision, AddonLight, AddonMood, AddonProposal, ClockStamp, Decision, Light,
    LightRequest, MicrophonePrivacy, MoodNudge, MoodState, Pause, Priority, Room, RoomState, Voice,
    VoiceMute, VoiceState,
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
    judge_with(root, enabled, &[])
}

/// The same, with `more` options.
fn judge_with(root: &Path, enabled: bool, more: &[&str]) -> Running {
    let mut command = Command::new(env!("CARGO_BIN_EXE_lamp-addons"));
    command
        .args(["serve", "--clock-id", "test-boot", "--bus-root"])
        .arg(root)
        .args(more)
        .env_remove("LAMP_ADDONS")
        .stdout(Stdio::null())
        // Its journal, kept in the test's folder for a failure to show
        // (Second on #2602, 22:0x: a failure on a Mac said only `[]`).
        .stderr(std::fs::File::create(root.join("judge.log")).unwrap());
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
    let journal = std::fs::read_to_string(dir.path().join("judge.log")).unwrap_or_default();
    JOURNAL.with(|kept| *kept.borrow_mut() = journal);
    (requests, decisions)
}

thread_local! {
    /// The judge's journal in this test's last run, for a failure to show.
    static JOURNAL: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// What the judge journaled in this thread's last run.
fn journal() -> String {
    JOURNAL.with(|kept| kept.borrow().clone())
}

#[test]
fn a_calm_lamp_keeps_an_add_ons_light_and_asks_it_of_the_ring_at_idle() {
    let (requests, decisions) = judged(true, false);
    let [request] = requests.as_slice() else {
        panic!(
            "asked {requests:?}; decided {decisions:?}; the judge's journal:\n{}",
            journal()
        );
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

/// A kept mood is said to Mood on `mood/nudge`, as the add-on asked it,
/// on the lamp's clock (#2530).
#[test]
fn a_kept_mood_is_said_to_lamp_mood() {
    let dir = tempfile::Builder::new()
        .prefix("ad")
        .tempdir_in("/tmp")
        .unwrap();
    let bus = Bus::new(dir.path()).unwrap();
    let mut mood = bus.subscribe(MoodNudge::TOPIC, "mood").unwrap();
    let _judge = judge(dir.path(), true);
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && bus.subscribers(AddonProposal::TOPIC).unwrap().is_empty() {
        std::thread::sleep(Duration::from_millis(50));
    }
    calm(&bus, false, false);
    std::thread::sleep(Duration::from_millis(300));
    let lift = AddonProposal {
        act: "mood".into(),
        why: "music".into(),
        light: None,
        mood: Some(AddonMood {
            energy: Some(0.1),
            warmth: Some(0.05),
            for_ms: 600_000,
        }),
        ..glow(lamp_ipc::monotonic_us())
    };
    bus.publisher_declared(
        AddonProposal::TOPIC,
        &AddonProposal::declaration(),
        "skills",
    )
    .unwrap()
    .publish_json(lamp_ipc::monotonic_us(), &lift)
    .unwrap();
    let message = mood.recv(Duration::from_millis(1_500)).unwrap().unwrap();
    assert_eq!(message.header.source, "addons");
    let nudge: MoodNudge = message.parse().unwrap();
    assert_eq!(nudge.check(), Ok(()));
    assert_eq!(nudge.stamp.clock_id, "test-boot");
    assert_eq!(
        (nudge.addon.as_str(), nudge.energy, nudge.warmth),
        ("evening-glow", 0.1, 0.05)
    );
    assert_eq!(nudge.until_us - nudge.stamp.at_us, 600_000_000);
}

/// A `nod` proposed on a calm lamp, the judge's move quiet `quiet_ms`
/// (`None`: the product's minute): what the body's mood socket was sent, and
/// what the judge decided.
fn moved(quiet_ms: Option<u64>) -> (Option<serde_json::Value>, Vec<AddonDecision>) {
    let dir = tempfile::Builder::new()
        .prefix("ad")
        .tempdir_in("/tmp")
        .unwrap();
    let bus = Bus::new(dir.path()).unwrap();
    let mut said = bus.subscribe(AddonDecision::TOPIC, "test").unwrap();
    let socket = dir.path().join("mood.sock");
    let body = std::os::unix::net::UnixDatagram::bind(&socket).unwrap();
    body.set_read_timeout(Some(Duration::from_millis(1_500)))
        .unwrap();
    let quiet = quiet_ms.map(|ms| ms.to_string());
    let mut more = vec!["--body-socket", socket.to_str().unwrap()];
    if let Some(quiet) = &quiet {
        more.extend(["--move-quiet-ms", quiet.as_str()]);
    }
    let _judge = judge_with(dir.path(), true, &more);
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && bus.subscribers(AddonProposal::TOPIC).unwrap().is_empty() {
        std::thread::sleep(Duration::from_millis(50));
    }
    calm(&bus, false, false);
    std::thread::sleep(Duration::from_millis(300));
    let nod = AddonProposal {
        act: "move".into(),
        why: "music".into(),
        light: None,
        gesture: Some("nod".into()),
        ..glow(lamp_ipc::monotonic_us())
    };
    bus.publisher_declared(
        AddonProposal::TOPIC,
        &AddonProposal::declaration(),
        "skills",
    )
    .unwrap()
    .publish_json(lamp_ipc::monotonic_us(), &nod)
    .unwrap();
    let mut datagram = [0u8; 512];
    let asked = body
        .recv(&mut datagram)
        .ok()
        .map(|read| serde_json::from_slice(&datagram[..read]).unwrap());
    let mut decisions = Vec::new();
    while let Some(message) = said.recv(Duration::from_millis(300)).unwrap() {
        decisions.push(message.parse().unwrap());
    }
    let journal = std::fs::read_to_string(dir.path().join("judge.log")).unwrap_or_default();
    JOURNAL.with(|kept| *kept.borrow_mut() = journal);
    (asked, decisions)
}

/// The first builder on #2530 (22 Oct 04:25): a kept move is asked of the
/// body on its mood socket as one gesture, at idle, from the add-on.
#[test]
fn a_kept_move_is_asked_of_the_body_as_one_gesture_at_idle() {
    let (asked, decisions) = moved(Some(0));
    assert_eq!(
        asked,
        Some(
            serde_json::json!({"gesture": "nod", "source": "addon:evening-glow", "priority": "idle"})
        ),
        "{decisions:?}; the judge's journal:\n{}",
        journal()
    );
    let [decision] = decisions.as_slice() else {
        panic!("{decisions:?}");
    };
    assert_eq!(
        (
            decision.act.as_str(),
            decision.choice.as_str(),
            decision.by.as_str()
        ),
        ("move", "kept", "rule")
    );
    assert_eq!(decision.why, "asked of the body at idle: nod");
}

/// Within a minute of the voice speaking, whoever is near, a move is
/// dropped and the body is sent nothing: after talk it reads as an answer.
#[test]
fn a_move_just_after_talk_is_dropped_and_the_body_sent_nothing() {
    let (asked, decisions) = moved(None);
    assert_eq!(asked, None, "{decisions:?}");
    let [decision] = decisions.as_slice() else {
        panic!("{decisions:?}");
    };
    assert_eq!(decision.choice, "dropped");
    assert_eq!(
        decision.why,
        "the lamp spoke or was spoken to in the last minute"
    );
}

/// The first builder on #2530 (20:1x): once the lamp has slept since the
/// person chose their light, an add-on may lay a brief glow over it, and the
/// ring then shows theirs again by itself.
#[test]
fn over_a_persons_light_that_rests_a_glow_is_asked_briefly() {
    let dir = tempfile::Builder::new()
        .prefix("ad")
        .tempdir_in("/tmp")
        .unwrap();
    let bus = Bus::new(dir.path()).unwrap();
    let mut ring = bus.subscribe(LightRequest::TOPIC, "ring").unwrap();
    let mut said = bus.subscribe(AddonDecision::TOPIC, "test").unwrap();
    let _judge = judge(dir.path(), true);
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && bus.subscribers(AddonProposal::TOPIC).unwrap().is_empty() {
        std::thread::sleep(Duration::from_millis(50));
    }
    calm(&bus, true, false);
    let asleep = MoodState {
        stamp: ClockStamp {
            clock_id: "test-boot".into(),
            at_us: lamp_ipc::monotonic_us(),
            sequence: 1,
        },
        session_id: "s".into(),
        awake: 0.0,
        energy: 0.3,
        warmth: 0.6,
        asleep: true,
        attention: None,
        familiarity: lamp_msgs::Familiarity::Unknown,
        familiar: None,
        breath: None,
    };
    bus.publisher_declared(MoodState::TOPIC, &MoodState::declaration(), "mood")
        .unwrap()
        .publish_json(lamp_ipc::monotonic_us(), &asleep)
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    bus.publisher_declared(
        AddonProposal::TOPIC,
        &AddonProposal::declaration(),
        "skills",
    )
    .unwrap()
    .publish_json(lamp_ipc::monotonic_us(), &glow(lamp_ipc::monotonic_us()))
    .unwrap();
    let decision: AddonDecision = said
        .recv(Duration::from_secs(5))
        .unwrap()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(decision.choice, "kept", "{decision:?}");
    assert!(
        decision.why.contains("the person's own light, which rests"),
        "{decision:?}"
    );
    let request: LightRequest = ring.recv(Duration::ZERO).unwrap().unwrap().parse().unwrap();
    assert_eq!(request.claim.priority, Priority::Idle);
    let holds_us = request.claim.until_us - lamp_ipc::monotonic_us();
    assert!(holds_us <= 5_000_000, "{holds_us}");
}

/// What the policy answers a say with, in the stand-in below.
#[derive(Clone, Copy)]
enum Answer {
    SayNow,
    Wait,
    Shadow,
    None,
}

/// One say through the judge (#2530 part 2), with a stand-in policy that
/// answers its ask: the ask it saw, and the judge's decision.
fn say_through(answer: Answer) -> (AddonAsk, AddonDecision) {
    let dir = tempfile::Builder::new()
        .prefix("ad")
        .tempdir_in("/tmp")
        .unwrap();
    let bus = Bus::new(dir.path()).unwrap();
    let mut asks = bus.subscribe(AddonAsk::TOPIC, "policy").unwrap();
    let mut said = bus.subscribe(AddonDecision::TOPIC, "test").unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_lamp-addons"));
    command
        .args([
            "serve",
            "--enable",
            "--say-quiet-ms",
            "200",
            "--clock-id",
            "test-boot",
            "--bus-root",
        ])
        .arg(dir.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let _judge = Running(command.spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && bus.subscribers(AddonProposal::TOPIC).unwrap().is_empty() {
        std::thread::sleep(Duration::from_millis(50));
    }
    calm(&bus, false, false);
    // The voice has rested past the test's quiet, each reading still fresh
    // (said once: a second publisher would be a restarted voice).
    std::thread::sleep(Duration::from_millis(400));
    let proposal = lamp_ipc::monotonic_us();
    let say = AddonProposal {
        act: "say".into(),
        why: "music".into(),
        light: None,
        note: Some("Music is playing.".into()),
        ..glow(proposal)
    };
    bus.publisher_declared(
        AddonProposal::TOPIC,
        &AddonProposal::declaration(),
        "skills",
    )
    .unwrap()
    .publish_json(lamp_ipc::monotonic_us(), &say)
    .unwrap();
    let ask: AddonAsk = asks
        .recv(Duration::from_secs(5))
        .unwrap()
        .expect("asked")
        .parse()
        .unwrap();
    let (choice, shadow) = match answer {
        Answer::SayNow => (Some("say_now"), false),
        Answer::Wait => (Some("wait"), false),
        Answer::Shadow => (Some("say_now"), true),
        Answer::None => (None, false),
    };
    if let Some(choice) = choice {
        bus.publisher_declared(Decision::TOPIC, &Decision::declaration(), "policy")
            .unwrap()
            .publish_json(
                lamp_ipc::monotonic_us(),
                &Decision {
                    about: AddonAsk::ABOUT.into(),
                    choice: choice.into(),
                    probability: 0.8,
                    answers: Default::default(),
                    by: "jev".into(),
                    took_ms: 300,
                    shadow,
                    request: None,
                    task: None,
                    arrival: None,
                    ask: None,
                    proposal: Some(ask.proposal),
                },
            )
            .unwrap();
    }
    let decision: AddonDecision = said
        .recv(Duration::from_secs(6))
        .unwrap()
        .expect("decided")
        .parse()
        .unwrap();
    (ask, decision)
}

/// Part 2: a say past the rules is put to the policy with no words, and
/// only the policy's acting `say_now` keeps it; no answer drops it.
#[test]
fn a_say_is_asked_of_the_policy_and_kept_or_dropped_by_its_answer() {
    let (ask, kept) = say_through(Answer::SayNow);
    assert_eq!(
        (ask.addon.as_str(), ask.why.as_str()),
        ("evening-glow", "music")
    );
    assert!(serde_json::to_value(&ask).unwrap().get("note").is_none());
    assert_eq!(
        (kept.choice.as_str(), kept.by.as_str()),
        ("kept", "jev"),
        "{kept:?}"
    );
    assert_eq!(kept.proposal, ask.proposal);
    for (answer, by, why) in [
        (Answer::Wait, "jev", "the policy said to wait"),
        (Answer::Shadow, "jev", "the policy's answer is shadow"),
        (Answer::None, "rule", "the policy did not answer"),
    ] {
        let (_, dropped) = say_through(answer);
        assert_eq!(
            (dropped.choice.as_str(), dropped.by.as_str()),
            ("dropped", by),
            "{dropped:?}"
        );
        assert!(dropped.why.starts_with(why), "{dropped:?}");
    }
}
