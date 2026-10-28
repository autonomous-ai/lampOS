//! Add-ons (`docs/addons.md`): a skill of kind `addon` loads only as one that
//! proposes, and the program writes its proposals on `addon/proposal` with
//! `LAMP_ADDONS=on`, and nothing without it.
use lamp_bus::Bus;
use lamp_msgs::{AddonProposal, SkillEvent, SkillStage, VoiceState};
use lamp_skills::{SkillKind, load};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command},
    time::{Duration, Instant},
};

/// An add-on folder named `name` in `dir`, with this `skill.toml`, as is.
fn addon_with(dir: &Path, name: &str, toml: &str) -> PathBuf {
    let skill = dir.join(name);
    fs::create_dir_all(&skill).unwrap();
    fs::write(
        skill.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: A partner's add-on.\n---\n"),
    )
    .unwrap();
    fs::write(skill.join("skill.toml"), toml).unwrap();
    skill
}

/// `docs/addons.md`'s emotion example: music lifts the lamp's mood.
const MUSIC: &str = r#"
kind = "addon"
version = "1.0.0"

[permissions]
topics = ["ears/classes", "voice/state"]
propose = ["mood"]

[trigger]
topic = "ears/classes"
field = "scores.music"
above = 0.7
for = "20s"
rearm = "ends"
cooldown = "15m"

[[while]]
topic = "voice/state"
field = "state"
equals = "resting"

[[do]]
propose = { act = "mood", why = "music", sure = 0.7, holds = "30s", mood = { energy = 0.1, for = "10m" } }
"#;

/// `docs/addons.md`'s light example: a warm evening breath.
const EVENING: &str = r#"
kind = "addon"
version = "1.0.0"
most_a_day = 1

[permissions]
topics = ["room/presence", "clock/local", "light/state"]
propose = ["light"]

[trigger]
name = "evening"
topic = "room/presence"
field = "someone"
equals = true
for = "2m"
during = { from = "19:00", to = "22:00" }
once_per_day = true

[[do]]
propose = { act = "light", why = "evening", sure = 0.6, holds = "30s", light = { colour = "warm", level = 2, pattern = { kind = "breathe", period = "8s" } } }
"#;

#[test]
fn the_documents_examples_load_as_add_ons_that_propose() {
    let dir = tempfile::tempdir().unwrap();
    let music = load(&addon_with(dir.path(), "happy-with-music", MUSIC)).unwrap();
    assert_eq!(music.kind, SkillKind::Addon);
    assert!(music.policy.is_empty(), "an add-on asks nobody here");
    assert_eq!(
        music.permissions.said(),
        "reads ears/classes, voice/state; proposes mood"
    );
    let proposals = music.proposals();
    let [proposal] = proposals.as_slice() else {
        panic!("{proposals:?}");
    };
    assert_eq!(proposal.addon, "happy-with-music");
    assert_eq!(proposal.version, "1.0.0");
    assert_eq!(proposal.holds_ms, 30_000);
    assert_eq!(proposal.mood.as_ref().unwrap().for_ms, 600_000);
    let evening = load(&addon_with(dir.path(), "evening-glow", EVENING)).unwrap();
    let proposals = evening.proposals();
    let [proposal] = proposals.as_slice() else {
        panic!("{proposals:?}");
    };
    assert_eq!(proposal.light.as_ref().unwrap().colour, "warm");
    assert_eq!(
        proposal.light.as_ref().unwrap().pattern.unwrap().period_ms,
        8_000
    );
}

/// What an add-on may not be, each refused with why (`docs/addons.md`).
#[test]
fn the_loader_refuses_an_add_on_anything_but_proposing() {
    let dir = tempfile::tempdir().unwrap();
    let cases: &[(&str, &str, &str)] = &[
        ("version = \"1.0.0\"\n", "", "says its version"),
        (
            "propose = [\"mood\"]",
            "propose = [\"mood\"]\nspeak = true",
            "speak in [permissions]",
        ),
        (
            "propose = [\"mood\"]",
            "propose = [\"mood\"]\nlight = true",
            "light in [permissions]",
        ),
        (
            "propose = [\"mood\"]",
            "propose = [\"mood\"]\nbody = true",
            "body in [permissions]",
        ),
        (
            "propose = [\"mood\"]",
            "propose = [\"mood\"]\nactions = [\"light_off\"]",
            "actions in [permissions]",
        ),
        (
            "propose = [\"mood\"]",
            "propose = [\"mood\"]\nknown_faces = true",
            "known_faces in [permissions]",
        ),
        (
            "propose = [\"mood\"]",
            "propose = [\"mood\"]\nlook = true",
            "look in [permissions]",
        ),
        (
            "propose = [\"mood\"]",
            "propose = [\"mood\"]\ncamera = true",
            "camera in [permissions]",
        ),
        ("propose = [\"mood\"]\n", "", "lists what it may propose"),
        (
            "topics = [\"ears/classes\", \"voice/state\"]",
            "topics = [\"ears/classes\", \"voice/state\", \"voice/heard\"]",
            "reading voice/heard",
        ),
        (
            "topics = [\"ears/classes\", \"voice/state\"]",
            "topics = [\"ears/classes\", \"voice/state\", \"humans/faces/tracked\"]",
            "reading humans/faces/tracked",
        ),
        (
            "[[do]]\n",
            "[ask]\npolicy = \"unprompted\"\n\n[[do]]\n",
            "[ask]",
        ),
        (
            "version = \"1.0.0\"\n",
            "version = \"1.0.0\"\ntool = \"lift_mood\"\n",
            "a tool",
        ),
        (
            "[[do]]\npropose",
            "[[do]]\nsay = \"Music!\"\n\n[[do]]\npropose",
            "a step that is not a proposal",
        ),
        (
            "[[do]]\npropose",
            "[[do]]\nlight = { colour = \"warm\" }\n\n[[do]]\npropose",
            "a step that is not a proposal",
        ),
        // The act is checked against [permissions] propose where it leaves.
        (
            "propose = [\"mood\"]",
            "propose = [\"light\"]",
            "propose mood needs \"mood\"",
        ),
        (
            "propose = [\"mood\"]",
            "propose = [\"dance\"]",
            "is not one of",
        ),
        ("sure = 0.7", "sure = 2.0", "sure is 0 to 1"),
        ("holds = \"30s\"", "holds = \"1m\"", "1 s to 30 s"),
        (
            "why = \"music\"",
            "why = \"because I said so\"",
            "why is one of",
        ),
        ("energy = 0.1", "energy = 0.5", "at most 0.1"),
        // The say grant (#2530 part 2): declared with propose naming say,
        // "quoted" only.
        (
            "propose = [\"mood\"]",
            "propose = [\"mood\", \"say\"]",
            "declares say = \"quoted\"",
        ),
        (
            "propose = [\"mood\"]",
            "propose = [\"mood\"]\nsay = \"quoted\"",
            "goes with propose naming \"say\"",
        ),
        (
            "propose = [\"mood\"]",
            "propose = [\"mood\", \"say\"]\nsay = \"spoken\"",
            "say is \"quoted\" only",
        ),
        // A note is refused at load until notes are typed (#2530).
        (
            "propose = [\"mood\"]",
            "propose = [\"mood\", \"note\"]",
            "\"note\" is refused for now",
        ),
        (
            "mood = { energy = 0.1, for = \"10m\" }",
            "mood = { energy = 0.1, for = \"10m\" }, note = \"Hi.\"",
            "the part its act names",
        ),
    ];
    for (n, (from, to, why)) in cases.iter().enumerate() {
        assert!(MUSIC.contains(from), "{from}");
        let name = format!("refused-{n}");
        let error = load(&addon_with(dir.path(), &name, &MUSIC.replacen(from, to, 1))).unwrap_err();
        assert!(error.contains(why), "{from:?} -> {to:?}: {error}");
    }
    // With no trigger it never starts itself, and a tool is refused.
    let lone = MUSIC.split("[trigger]").next().unwrap().to_owned()
        + "\n[[do]]\npropose = { act = \"mood\", why = \"music\", sure = 0.7, holds = \"30s\", mood = { energy = 0.1, for = \"10m\" } }\n";
    let error = load(&addon_with(dir.path(), "lone", &lone)).unwrap_err();
    assert!(error.contains("by its triggers alone"), "{error}");
    let gesture = EVENING
        .replace("propose = [\"light\"]", "propose = [\"move\"]")
        .replace(
            "light = { colour = \"warm\", level = 2, pattern = { kind = \"breathe\", period = \"8s\" } }",
            "gesture = \"backflip\"",
        )
        .replace("act = \"light\"", "act = \"move\"");
    let error = load(&addon_with(dir.path(), "flips", &gesture)).unwrap_err();
    assert!(error.contains("is not one of the body's"), "{error}");
    // One of the body's gestures, but not an add-on's (the first builder on
    // #2530, 22 Oct 04:25): refused; nod, curious and stretching load.
    let scan = gesture.replace("\"backflip\"", "\"scanning\"");
    let error = load(&addon_with(dir.path(), "scans", &scan)).unwrap_err();
    assert!(
        error.contains("a move is nod, curious or stretching"),
        "{error}"
    );
    for allowed in lamp_msgs::ADDON_GESTURES {
        let moves = gesture.replace("\"backflip\"", &format!("\"{allowed}\""));
        load(&addon_with(dir.path(), &format!("moves-{allowed}"), &moves))
            .unwrap_or_else(|error| panic!("{allowed}: {error}"));
    }
}

/// #2530 part 2: an add-on that proposes `say` loads with its say grant,
/// which no other kind may declare.
#[test]
fn a_say_needs_the_quoted_grant_and_only_an_add_on_has_it() {
    let dir = tempfile::tempdir().unwrap();
    let saying = MUSIC.replace(
        "propose = [\"mood\"]",
        "propose = [\"mood\", \"say\"]\nsay = \"quoted\"",
    );
    load(&addon_with(dir.path(), "says", &saying)).unwrap();
    let installable = MUSIC
        .replace("kind = \"addon\"", "kind = \"installable\"")
        .replace("propose = [\"mood\"]", "say = \"quoted\"");
    let error = load(&addon_with(dir.path(), "not-addon", &installable)).unwrap_err();
    assert!(error.contains("an add-on's say grant"), "{error}");
}

/// A skill that is not an add-on proposes nothing.
#[test]
fn only_an_add_on_proposes() {
    let dir = tempfile::tempdir().unwrap();
    let installable = MUSIC.replace("kind = \"addon\"", "kind = \"installable\"");
    let error = load(&addon_with(dir.path(), "not-addon", &installable)).unwrap_err();
    assert!(
        error.contains("[permissions] propose is an add-on's"),
        "{error}"
    );
    let step_only = installable
        .replace("propose = [\"mood\"]\n", "")
        .replace("[[while]]", "[ask]\npolicy = \"unprompted\"\n\n[[while]]");
    let error = load(&addon_with(dir.path(), "steps", &step_only)).unwrap_err();
    assert!(error.contains("propose is an add-on's step"), "{error}");
}

struct Running(Child);

impl Running {
    fn waiting(&mut self, deadline: Instant, root: &Path, what: &str) {
        let status = self.0.try_wait().unwrap();
        assert!(
            status.is_none(),
            "lamp-skills exited {status:?} while waiting for {what}; stderr:\n{}",
            fs::read_to_string(root.join("stderr")).unwrap()
        );
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}; stderr:\n{}",
            fs::read_to_string(root.join("stderr")).unwrap()
        );
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A trigger on the voice resting, which an add-on may read.
const RESTING: &str = r#"
kind = "addon"
version = "1.2.0"

[permissions]
topics = ["voice/state"]
propose = ["say"]
say = "quoted"

[trigger]
topic = "voice/state"
field = "state"
equals = "resting"

[[do]]
propose = { act = "say", why = "quiet", sure = 0.5, holds = "10s", note = "The room is quiet." }
"#;

/// The program with one add-on, the switch `on` or not: what it wrote on
/// `addon/proposal` after the voice said it rests three times, and its
/// skill events.
fn the_program(on: bool) -> (Vec<AddonProposal>, Vec<SkillEvent>, bool) {
    let dir = tempfile::Builder::new()
        .prefix("ad")
        .tempdir_in("/tmp")
        .unwrap();
    let root = dir.path();
    let addon = "quiet-say";
    addon_with(&root.join("skills"), addon, RESTING);
    let bus = Bus::new(root.join("bus")).unwrap();
    let mut events = bus.subscribe(SkillEvent::TOPIC, "test").unwrap();
    let mut proposals = bus.subscribe(AddonProposal::TOPIC, "judge").unwrap();
    let clock = (!on).then(|| super::skills::Clock::at(root));
    let mut command = Command::new(env!("CARGO_BIN_EXE_lamp-skills"));
    for (flag, path) in [
        ("--dir", root.join("skills")),
        ("--bus-root", root.join("bus")),
        ("--gaze-socket", root.join("gaze.sock")),
        ("--mood-socket", root.join("mood.sock")),
        ("--actions-socket", root.join("actions.sock")),
        ("--say-socket", root.join("say.sock")),
    ] {
        command.arg(flag).arg(path);
    }
    command
        .env_remove("LAMP_SKILLS_ASK")
        .env_remove(lamp_skills::ADDONS_ENVIRONMENT)
        .stderr(std::fs::File::create(root.join("stderr")).unwrap());
    if on {
        command.env(lamp_skills::ADDONS_ENVIRONMENT, "on");
    }
    if let Some(clock) = &clock {
        command.args(clock.args());
    }
    let mut running = Running(command.spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(10);
    let subscribed = if let Some(clock) = &clock {
        while !std::fs::read_to_string(root.join("stderr"))
            .unwrap()
            .contains("lamp-skills: loaded ")
        {
            running.waiting(deadline, root, "the loaded announcement");
            std::thread::sleep(Duration::from_millis(10));
        }
        // The loaded line precedes subscription setup. A skip requested
        // after that line is taken after setup, so its acknowledgement
        // makes this absence check independent of a startup sleep (#1806).
        running.waiting(deadline, root, "the startup acknowledgement");
        clock.skip(Duration::ZERO);
        !bus.subscribers(VoiceState::TOPIC).unwrap().is_empty()
    } else {
        loop {
            running.waiting(deadline, root, "the voice/state subscription");
            if !bus.subscribers(VoiceState::TOPIC).unwrap().is_empty() {
                break true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    let mut voice = bus
        .publisher_declared(VoiceState::TOPIC, &VoiceState::declaration(), "voice")
        .unwrap();
    let mut seen = Vec::new();
    for publication in 1..=3 {
        voice
            .publish_json(1, &serde_json::json!({ "state": "resting" }))
            .unwrap();
        if let Some(clock) = &clock {
            // No add-on is loaded, so there can be no completion event.
            // A zero skip takes in any input already sent without moving
            // the clock ahead; the absence drains below still run.
            clock.skip(Duration::ZERO);
        } else {
            // Each proposal or limit refusal completes before the next
            // trigger. Latest-state publications could otherwise coalesce,
            // and a clock acknowledgement alone does not clear a busy run.
            let deadline = Instant::now() + Duration::from_secs(10);
            let what = format!("{addon} completion after publication {publication}");
            loop {
                running.waiting(deadline, root, &what);
                let wait = deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(100));
                if let Some(message) = events.recv(wait).unwrap() {
                    let event = message.parse::<SkillEvent>().unwrap();
                    let done = event.skill == addon && event.stage == SkillStage::Done;
                    seen.push(event);
                    if done {
                        break;
                    }
                }
            }
        }
    }
    let mut written = Vec::new();
    while let Some(message) = proposals.recv(Duration::from_millis(500)).unwrap() {
        written.push(message.parse::<AddonProposal>().unwrap());
    }
    while let Some(message) = events.recv(Duration::from_millis(100)).unwrap() {
        seen.push(message.parse::<SkillEvent>().unwrap());
    }
    (written, seen, subscribed)
}

/// With `LAMP_ADDONS=on`, the add-on's proposal is written once, numbered
/// and stamped with its folder's name and version; the next ones within
/// 10 s are held back by the limits, and said so.
#[test]
fn with_the_switch_the_program_writes_one_proposal_stamped_with_the_add_on() {
    let (written, events, subscribed) = the_program(true);
    assert!(subscribed, "the add-on was never loaded");
    assert_eq!(written.len(), 1, "{written:?}");
    let proposal = &written[0];
    assert!(proposal.proposal > 0);
    assert_eq!(proposal.addon, "quiet-say");
    assert_eq!(proposal.version, "1.2.0");
    assert_eq!(proposal.act, "say");
    assert_eq!(proposal.note.as_deref(), Some("The room is quiet."));
    assert_eq!(proposal.check(), Ok(()));
    let said: Vec<&str> = events.iter().map(|event| event.why.as_str()).collect();
    assert!(
        said.iter().any(|why| why.starts_with("proposed say as")),
        "{said:?}"
    );
    assert!(
        said.iter()
            .any(|why| why.contains("one proposal every 10 s")),
        "{said:?}"
    );
}

/// Without the switch, the add-on is not loaded and nothing is written.
#[test]
fn without_the_switch_no_add_on_is_loaded_and_nothing_is_written() {
    let (written, events, subscribed) = the_program(false);
    assert!(!subscribed, "an add-on was loaded without the switch");
    assert!(written.is_empty(), "{written:?}");
    assert!(events.is_empty(), "{events:?}");
}

/// An add-on that proposes a say whenever the voice rests (#2530 part 2).
const SAYS: &str = r#"
kind = "addon"
version = "1.0.0"

[permissions]
topics = ["voice/state"]
propose = ["say"]
say = "quoted"

[trigger]
topic = "voice/state"
field = "state"
equals = "resting"

[[do]]
propose = { act = "say", why = "music", sure = 0.7, holds = "10s", note = "Music is playing." }
"#;

/// The program with [`SAYS`] and a stand-in say service: the judge's
/// decision on its say is `choice` by `by`; what the voice was asked, if
/// anything, within 2 s.
fn a_say_decided(choice: &str, by: &str) -> Option<serde_json::Value> {
    a_say_decided_told(choice, by, "{\"outcome\":\"spoken\"}").0
}

/// [`a_say_decided`], the say service answering `outcome`: also the
/// program's journal.
fn a_say_decided_told(
    choice: &str,
    by: &str,
    outcome: &'static str,
) -> (Option<serde_json::Value>, String) {
    use std::io::{Read, Write};
    let dir = tempfile::Builder::new()
        .prefix("ad")
        .tempdir_in("/tmp")
        .unwrap();
    let root = dir.path();
    addon_with(&root.join("skills"), "music-says", SAYS);
    let say = std::os::unix::net::UnixListener::bind(root.join("say.sock")).unwrap();
    say.set_nonblocking(true).unwrap();
    let bus = Bus::new(root.join("bus")).unwrap();
    let mut proposals = bus.subscribe(AddonProposal::TOPIC, "judge").unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_lamp-skills"));
    for (flag, path) in [
        ("--dir", root.join("skills")),
        ("--bus-root", root.join("bus")),
        ("--gaze-socket", root.join("gaze.sock")),
        ("--mood-socket", root.join("mood.sock")),
        ("--actions-socket", root.join("actions.sock")),
        ("--say-socket", root.join("say.sock")),
    ] {
        command.arg(flag).arg(path);
    }
    command
        .env_remove("LAMP_SKILLS_ASK")
        .env(lamp_skills::ADDONS_ENVIRONMENT, "on")
        .stderr(std::fs::File::create(root.join("stderr")).unwrap());
    let mut running = Running(command.spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && bus.subscribers(VoiceState::TOPIC).unwrap().is_empty() {
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut voice = bus
        .publisher_declared(VoiceState::TOPIC, &VoiceState::declaration(), "voice")
        .unwrap();
    voice
        .publish_json(1, &serde_json::json!({ "state": "resting" }))
        .unwrap();
    let proposal: AddonProposal = proposals
        .recv(Duration::from_secs(5))
        .unwrap()
        .expect("proposed")
        .parse()
        .unwrap();
    bus.publisher_declared(
        lamp_msgs::AddonDecision::TOPIC,
        &lamp_msgs::AddonDecision::declaration(),
        "addons",
    )
    .unwrap()
    .publish_json(
        1,
        &lamp_msgs::AddonDecision {
            proposal: proposal.proposal,
            addon: proposal.addon.clone(),
            act: "say".into(),
            choice: choice.into(),
            by: by.into(),
            why: "test".into(),
        },
    )
    .unwrap();
    let request_deadline = Instant::now() + Duration::from_secs(2);
    let completion_deadline = request_deadline + Duration::from_millis(200);
    let pause = |deadline: Instant| {
        std::thread::sleep(
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(5)),
        );
    };
    // Own the listener here, including when no request should arrive. A
    // detached accept thread would keep it alive after either negative case.
    let asked = loop {
        let absence_observed = Instant::now() >= completion_deadline;
        match say.accept() {
            Ok((mut stream, _)) => {
                assert!(Instant::now() < request_deadline, "late say request");
                stream.set_nonblocking(true).unwrap();
                let mut line = Vec::new();
                let mut buffer = [0; 1_024];
                let asked = loop {
                    running.waiting(request_deadline, root, "a complete say request");
                    match stream.read(&mut buffer) {
                        Ok(0) => panic!("say request ended before its newline"),
                        Ok(count) => {
                            line.extend_from_slice(&buffer[..count]);
                            assert!(line.len() <= 65_536, "say request exceeded 64 KiB");
                            if let Some(end) = line.iter().position(|byte| *byte == b'\n') {
                                let asked =
                                    serde_json::from_slice::<serde_json::Value>(&line[..end])
                                        .unwrap();
                                assert!(
                                    Instant::now() < request_deadline,
                                    "late complete say request"
                                );
                                break asked;
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            pause(request_deadline);
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(error) => panic!("read say request: {error}"),
                    }
                };
                let reply = format!("{outcome}\n");
                let mut sent = 0;
                while sent < reply.len() {
                    running.waiting(completion_deadline, root, "writing the say outcome");
                    match stream.write(&reply.as_bytes()[sent..]) {
                        Ok(0) => panic!("say outcome stopped being writable"),
                        Ok(count) => sent += count,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            pause(completion_deadline);
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(error) => panic!("write say outcome: {error}"),
                    }
                }
                break Some(asked);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                let status = running.0.try_wait().unwrap();
                assert!(status.is_none(), "lamp-skills exited {status:?}");
                // Keep the full former 2 s plus 200 ms observation envelope
                // for silence. Any request after 2 s fails above, not as None.
                if absence_observed {
                    break None;
                }
                pause(completion_deadline);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
                assert!(
                    !absence_observed,
                    "say listener was interrupted after its observation deadline"
                );
            }
            Err(error) => panic!("accept say request: {error}"),
        }
    };
    if asked.is_some() {
        let prefix = format!(
            "lamp-skills: {} say #{}, quoted: ",
            proposal.addon, proposal.proposal
        );
        // The say worker can finish before Main drains its queued Done.
        // Observe both writers before closing the child for the privacy check.
        let main_done = format!(
            "lamp-skills: {} Done: proposed say as {} on {}\n",
            proposal.addon,
            proposal.proposal,
            AddonProposal::TOPIC
        );
        loop {
            let journal = fs::read_to_string(root.join("stderr")).unwrap();
            if journal
                .split_inclusive('\n')
                .any(|line| line.ends_with('\n') && line.starts_with(&prefix))
                && journal.split_inclusive('\n').any(|line| line == main_done)
            {
                assert!(
                    Instant::now() < completion_deadline,
                    "late complete say outcome: {journal}"
                );
                break;
            }
            running.waiting(
                completion_deadline,
                root,
                "the quoted say's complete outcome",
            );
            pause(completion_deadline);
        }
    }
    // Include all output through child closure in the privacy assertion.
    // The listener is local too; neither success nor panic leaves a worker.
    drop(running);
    let journal = std::fs::read_to_string(root.join("stderr")).unwrap();
    assert!(
        !journal.contains("Music is playing."),
        "no words journaled: {journal}"
    );
    (asked, journal)
}

/// #2530 part 2: only a say the policy kept (`by` `jev`) is asked of the
/// voice, and only quoted: `{skill, quoted}`, never a note it would follow.
#[test]
fn only_a_say_kept_by_the_policy_is_asked_of_the_voice_quoted() {
    let asked = a_say_decided("kept", "jev").expect("asked of the voice");
    assert_eq!(asked["skill"], "music-says");
    assert_eq!(asked["quoted"], "Music is playing.");
    assert!(
        asked.get("note").is_none() && asked.get("text").is_none(),
        "{asked}"
    );
    assert_eq!(a_say_decided("dropped", "jev"), None);
    assert_eq!(a_say_decided("kept", "rule"), None);
}

/// Four add-ons' says, then a fifth's (on `listening`), with the four
/// dropped by the judge first or not decided at all, then the four's
/// trigger again: what the fifth did, in the journal.
fn a_fifth_say(drop_the_four: bool) -> String {
    use std::io::Read;

    #[derive(Debug)]
    struct Completion {
        skill: String,
        why: String,
    }

    let dir = tempfile::Builder::new()
        .prefix("ad")
        .tempdir_in("/tmp")
        .unwrap();
    let root = dir.path();
    for name in ["says-a", "says-b", "says-c", "says-d"] {
        addon_with(&root.join("skills"), name, SAYS);
    }
    addon_with(
        &root.join("skills"),
        "says-e",
        &SAYS.replace("equals = \"resting\"", "equals = \"listening\""),
    );
    let bus = Bus::new(root.join("bus")).unwrap();
    let mut proposals = bus.subscribe(AddonProposal::TOPIC, "judge").unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_lamp-skills"));
    for (flag, path) in [
        ("--dir", root.join("skills")),
        ("--bus-root", root.join("bus")),
        ("--gaze-socket", root.join("gaze.sock")),
        ("--mood-socket", root.join("mood.sock")),
        ("--actions-socket", root.join("actions.sock")),
        ("--say-socket", root.join("say.sock")),
    ] {
        command.arg(flag).arg(path);
    }
    command
        .env_remove("LAMP_SKILLS_ASK")
        .env(lamp_skills::ADDONS_ENVIRONMENT, "on")
        .stderr(std::fs::File::create(root.join("stderr")).unwrap());
    let mut running = Running(command.spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline
        && (bus.subscribers(VoiceState::TOPIC).unwrap().is_empty()
            || bus
                .subscribers(lamp_msgs::AddonDecision::TOPIC)
                .unwrap()
                .is_empty())
    {
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut voice = bus
        .publisher_declared(VoiceState::TOPIC, &VoiceState::declaration(), "voice")
        .unwrap();
    // The probe budget is shorter than the unchanged ten-second proposal
    // limit and starts before any of the first four can be admitted.
    let completion_deadline = Instant::now() + Duration::from_secs(8);
    // This cursor belongs to all phases. A completed phase consumes its
    // journal lines before the next voice publication can start another.
    let mut journal_seen = Vec::<u8>::new();
    let mut journal_cursor = 0;
    let mut completed = |names: &[&str], phase: &str| {
        let mut seen = Vec::<Completion>::new();
        while seen.len() < names.len() {
            let left = completion_deadline.saturating_duration_since(Instant::now());
            let status = running.0.try_wait().unwrap();
            assert!(
                !left.is_zero() && status.is_none(),
                "waiting for {phase}: child {status:?}, completions {seen:?}; journal:\n{}",
                String::from_utf8_lossy(&journal_seen)
            );
            let mut journal = Vec::new();
            std::fs::File::open(root.join("stderr"))
                .unwrap()
                .take(1_048_577)
                .read_to_end(&mut journal)
                .unwrap();
            assert!(
                journal.len() <= 1_048_576,
                "the fixture journal exceeded 1 MiB"
            );
            assert!(
                journal.starts_with(&journal_seen),
                "the fixture journal stopped being append-only during {phase}"
            );
            journal_seen = journal;
            while let Some(end) = journal_seen[journal_cursor..]
                .iter()
                .position(|byte| *byte == b'\n')
            {
                let end = journal_cursor + end + 1;
                let line = std::str::from_utf8(&journal_seen[journal_cursor..end - 1])
                    .expect("complete fixture journal lines are UTF-8");
                journal_cursor = end;
                if let Some((skill, why)) = line
                    .strip_prefix("lamp-skills: ")
                    .and_then(|line| line.split_once(" Done: "))
                    && names.contains(&skill)
                    && !seen.iter().any(|old| old.skill == skill)
                {
                    seen.push(Completion {
                        skill: skill.to_owned(),
                        why: why.to_owned(),
                    });
                }
            }
            if seen.len() < names.len() {
                std::thread::sleep(
                    completion_deadline
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(10)),
                );
            }
        }
        assert!(
            Instant::now() < completion_deadline,
            "the completion budget expired during {phase}; journal:\n{}",
            String::from_utf8_lossy(&journal_seen)
        );
        seen
    };
    voice
        .publish_json(1, &serde_json::json!({ "state": "resting" }))
        .unwrap();
    let mut four = Vec::new();
    while four.len() < 4 {
        let message = proposals
            .recv(Duration::from_secs(5))
            .unwrap()
            .expect("four proposed");
        four.push(message.parse::<AddonProposal>().unwrap());
    }
    let names: Vec<&str> = four
        .iter()
        .map(|proposal| proposal.addon.as_str())
        .collect();
    completed(&names, "the first four proposals");
    if drop_the_four {
        let mut judge = bus
            .publisher_declared(
                lamp_msgs::AddonDecision::TOPIC,
                &lamp_msgs::AddonDecision::declaration(),
                "addons",
            )
            .unwrap();
        for proposal in &four {
            judge
                .publish_json(
                    1,
                    &lamp_msgs::AddonDecision {
                        proposal: proposal.proposal,
                        addon: proposal.addon.clone(),
                        act: "say".into(),
                        choice: "dropped".into(),
                        by: "rule".into(),
                        why: "the room is busy, or that is not known".into(),
                    },
                )
                .unwrap();
        }
        // The decision reader has its own thread. A main-loop clock
        // acknowledgement cannot prove that it refunded all four says.
        // Each probe finishes before the next, so no trigger coalesces or
        // runs while its predecessor is busy. Only the real drop refusal
        // proves that every decision was consumed before asking the fifth.
        loop {
            assert!(
                Instant::now() < completion_deadline,
                "the four drop decisions were not consumed; journal:\n{}",
                std::fs::read_to_string(root.join("stderr")).unwrap()
            );
            voice
                .publish_json(1, &serde_json::json!({ "state": "resting" }))
                .unwrap();
            if completed(&names, "the four drop refusals")
                .iter()
                .all(|event| event.why == "not proposed: 300 s after the judge dropped its say")
            {
                break;
            }
        }
    }
    voice
        .publish_json(2, &serde_json::json!({ "state": "listening" }))
        .unwrap();
    let fifth = proposals.recv(Duration::from_secs(3)).unwrap();
    completed(&["says-e"], "the fifth proposal or its refusal");
    voice
        .publish_json(3, &serde_json::json!({ "state": "resting" }))
        .unwrap();
    // Only new complete lines from this final trigger can finish this phase.
    completed(&names, "the final four refusals");
    let journal = std::fs::read_to_string(root.join("stderr")).unwrap();
    match fifth {
        Some(message) => format!(
            "proposed by {}\n{journal}",
            message.parse::<AddonProposal>().unwrap().addon
        ),
        None => journal,
    }
}

/// lamp-75c9, 22 Oct 07:25: four says dropped in a talking room spent the
/// hour, and the quiet after found nothing left. A say the judge drops is
/// no longer counted toward the four; four not yet decided still are.
#[test]
fn says_the_judge_dropped_do_not_spend_the_hour() {
    let journal = a_fifth_say(true);
    assert!(journal.starts_with("proposed by says-e"), "{journal}");
    // A drop holds that add-on five minutes (the first builder on #2530).
    assert!(
        journal.contains("says-a Done: not proposed: 300 s after the judge dropped its say"),
        "{journal}"
    );
    let journal = a_fifth_say(false);
    assert!(
        journal.contains("says-e Done: not proposed: 4 say proposals an hour"),
        "{journal}"
    );
}

/// The model may leave a quoted line unsaid: the journal says `said
/// nothing`, with the add-on and the proposal, never the words.
#[test]
fn a_quoted_line_left_unsaid_is_journaled_as_said_nothing() {
    let (asked, journal) = a_say_decided_told(
        "kept",
        "jev",
        "{\"outcome\":\"dropped\",\"why\":\"no_audio_answer\"}",
    );
    assert!(asked.is_some());
    let line = journal
        .lines()
        .find(|line| line.contains("music-says say #"))
        .unwrap_or_else(|| panic!("{journal}"));
    assert!(line.ends_with(", quoted: said nothing"), "{line}");
}
