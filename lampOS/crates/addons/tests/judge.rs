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

/// Waits until the judge listens to everything it reads, at most 30 s.
/// `policy/decisions` is the last topic it subscribes to, after
/// `addon/proposal` and the lamp's own topics: a lamp said calm before then
/// could be missed, and a say dropped as "talking" is never asked (the first
/// builder on #2602, 22 Oct 04:50, a run of #2737).
fn listening(bus: &Bus, root: &Path) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while bus.subscribers(Decision::TOPIC).unwrap().is_empty() {
        assert!(
            Instant::now() < deadline,
            "the judge never listened; its journal:\n{}",
            std::fs::read_to_string(root.join("judge.log")).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The same final subscription, with the child checked while waiting and
/// the completion fixture's deadline shared with its later decision wait.
fn wait_for_inputs(bus: &Bus, root: &Path, judge: &mut Running, deadline: Instant) {
    while bus.subscribers(Decision::TOPIC).unwrap().is_empty() {
        assert!(
            judge.0.try_wait().unwrap().is_none(),
            "the judge ended before listening"
        );
        assert!(
            Instant::now() < deadline,
            "the judge never listened; its journal:\n{}",
            std::fs::read_to_string(root.join("judge.log")).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
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

/// The voice's privacy said every 20 ms from one publisher, as the voice
/// says it, until dropped: one said once can be passed over by a judge on a
/// loaded runner, since a reading claims 50 ms at most
/// (`MICROPHONE_PRIVACY_FRESH_US`) and the bus skips one past it.
struct Privacy {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    saying: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Privacy {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(saying) = self.saying.take() {
            let _ = saying.join();
        }
    }
}

/// A calm lamp at rest, freshly said, its privacy said for as long as the
/// returned [`Privacy`] is kept. `product`: as a product lamp says it,
/// with no `lamp/pause` (only `lamp-keeper` says one) and no `voice/mute`
/// (only the head's controls say one).
#[must_use = "privacy is said only while the guard is kept"]
fn calm(bus: &Bus, persons_light: bool, product: bool) -> Privacy {
    let mut privacy = bus
        .publisher_declared(
            MicrophonePrivacy::TOPIC,
            &MicrophonePrivacy::declaration(),
            "voice",
        )
        .unwrap();
    let mut say = move |sequence| {
        let now = lamp_ipc::monotonic_us();
        privacy.publish_json(
            now,
            &MicrophonePrivacy {
                stamp: ClockStamp {
                    clock_id: "test-boot".into(),
                    at_us: now,
                    sequence,
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
    };
    // The light and mood fixtures start the judge after calm(). Its first
    // privacy value must already be retained, even if the renewal thread
    // has not run yet. Keep that same publisher for every renewal.
    say(1).unwrap();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stopped = stop.clone();
    let saying = std::thread::spawn(move || {
        let mut sequence = 1;
        while !stopped.load(std::sync::atomic::Ordering::Relaxed) {
            sequence += 1;
            let _ = say(sequence);
            std::thread::sleep(Duration::from_millis(20));
        }
    });
    let now = lamp_ipc::monotonic_us();
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
                    muted_by: None,
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
    Privacy {
        stop,
        saying: Some(saying),
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
    // The judge reads these retained topics before its first proposal.
    // Privacy's first value is already there; its guard keeps it renewed.
    let _privacy = calm(&bus, persons_light, product);
    let mut judge = if product {
        judge_with(dir.path(), enabled, &["--mute-grace-ms", "0"])
    } else {
        judge(dir.path(), enabled)
    };
    // Wait for every input subscription. Off, it says so and ends
    // without subscribing: its end waited for, not 2 s of no subscriber
    // (#1806).
    let deadline = Instant::now() + Duration::from_secs(30);
    if enabled {
        wait_for_inputs(&bus, dir.path(), &mut judge, deadline);
    } else {
        while judge.0.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "off, it did not end");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(bus.subscribers(AddonProposal::TOPIC).unwrap().is_empty());
    }
    let mut skills = bus
        .publisher_declared(AddonProposal::TOPIC, &AddonProposal::declaration(), source)
        .unwrap();
    skills
        .publish_json(lamp_ipc::monotonic_us(), &glow(lamp_ipc::monotonic_us()))
        .unwrap();
    // The judge says its decision on a proposal after the light it asks for
    // it, so once the decision is read the light is there too: waited for,
    // not 1.5 s of quiet from the ring (#1806). A proposal it does not judge
    // says nothing, so one of lamp-skills' under another name follows it,
    // is judged after it, and ends the wait. Off, it has ended already.
    if enabled && source != "skills" {
        bus.publisher_declared(
            AddonProposal::TOPIC,
            &AddonProposal::declaration(),
            "skills",
        )
        .unwrap()
        .publish_json(
            lamp_ipc::monotonic_us(),
            &AddonProposal {
                addon: LAST.into(),
                ..glow(lamp_ipc::monotonic_us())
            },
        )
        .unwrap();
    }
    let mut decisions: Vec<AddonDecision> = Vec::new();
    while enabled && decisions.is_empty() {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "no decision");
        if let Some(message) = said.recv(left).unwrap() {
            decisions.push(message.parse().unwrap());
        }
    }
    // The light precedes its decision, so take this snapshot at the first
    // decision, before the duplicate-decision observation below. A light
    // asked after its receipt must not be accepted during that window.
    let mut requests: Vec<LightRequest> = Vec::new();
    while let Some(message) = ring.recv(Duration::ZERO).unwrap() {
        requests.push(message.parse().unwrap());
    }
    // And 100 ms of quiet after it, so a second decision on the same
    // proposal, a little late, is still seen.
    while let Some(message) = said
        .recv(Duration::from_millis(if enabled { 100 } else { 0 }))
        .unwrap()
    {
        decisions.push(message.parse().unwrap());
    }
    let journal = std::fs::read_to_string(dir.path().join("judge.log")).unwrap_or_default();
    JOURNAL.with(|kept| *kept.borrow_mut() = journal);
    assert!(
        ring.recv(Duration::ZERO).unwrap().is_none(),
        "a light was asked after its decision"
    );
    requests.retain(|request| request.claim.source != format!("addon:{LAST}"));
    decisions.retain(|decision| decision.addon != LAST);
    (requests, decisions)
}

/// The add-on of the proposal that ends the wait after one not judged.
const LAST: &str = "the-last-glow";

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
        ("kept", "evening-glow"),
        "{decision:?}"
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
    let _privacy = calm(&bus, false, false);
    let mut judge = judge(dir.path(), true);
    let deadline = Instant::now() + Duration::from_secs(30);
    wait_for_inputs(&bus, dir.path(), &mut judge, deadline);
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
    listening(&bus, dir.path());
    let _privacy = calm(&bus, false, false);
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
    let _privacy = calm(&bus, true, false);
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
        felt: None,
        breath: None,
    };
    bus.publisher_declared(MoodState::TOPIC, &MoodState::declaration(), "mood")
        .unwrap()
        .publish_json(lamp_ipc::monotonic_us(), &asleep)
        .unwrap();
    let mut judge = judge(dir.path(), true);
    wait_for_inputs(
        &bus,
        dir.path(),
        &mut judge,
        Instant::now() + Duration::from_secs(30),
    );
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
        .stderr(std::fs::File::create(dir.path().join("judge.log")).unwrap());
    let _judge = Running(command.spawn().unwrap());
    listening(&bus, dir.path());
    let _privacy = calm(&bus, false, false);
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

#[test]
fn the_mute_grace_override_is_described_and_bounded_for_tests() {
    let binary = env!("CARGO_BIN_EXE_lamp-addons");
    for args in [vec!["--help"], vec!["serve", "--help"]] {
        let output = Command::new(binary).args(args).output().unwrap();
        assert!(output.status.success());
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.contains("--mute-grace-ms"), "{help}");
        assert!(help.contains("Tests only"), "{help}");
        assert!(help.contains("default 3000 ms"), "{help}");
    }
    for ms in ["0", "3000"] {
        let output = Command::new(binary)
            .args(["serve", "--mute-grace-ms", ms])
            .env_remove("LAMP_ADDONS")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
    for args in [
        vec!["--mute-grace-ms"],
        vec!["--mute-grace-ms", "-1"],
        vec!["--mute-grace-ms", "3001"],
        vec!["--mute-grace-ms", "not-a-number"],
        vec!["--mute-grace-ms", "0", "--mute-grace-ms", "1"],
    ] {
        let output = Command::new(binary)
            .arg("serve")
            .args(&args)
            .env_remove("LAMP_ADDONS")
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?}: {output:?}");
    }
}
