//! Add-ons (`docs/addons.md`): a skill of kind `addon` loads only as one that
//! proposes, and the program writes its proposals on `addon/proposal` with
//! `LAMP_ADDONS=on`, and nothing without it.
use lamp_bus::Bus;
use lamp_msgs::{AddonProposal, SkillEvent, VoiceState};
use lamp_skills::{SkillKind, load};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
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
propose = ["note"]

[trigger]
topic = "voice/state"
field = "state"
equals = "resting"

[[do]]
propose = { act = "note", why = "quiet", sure = 0.5, holds = "10s", note = "The room is quiet." }
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
    addon_with(&root.join("skills"), "quiet-note", RESTING);
    let bus = Bus::new(root.join("bus")).unwrap();
    let mut events = bus.subscribe(SkillEvent::TOPIC, "test").unwrap();
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
        .env_remove(lamp_skills::ADDONS_ENVIRONMENT)
        .stderr(Stdio::null());
    if on {
        command.env(lamp_skills::ADDONS_ENVIRONMENT, "on");
    }
    let _running = Running(command.spawn().unwrap());
    // Loaded at its start, and subscribed to the voice once it is: with the
    // switch, waited for; without it, given two seconds to show it is not.
    let wait = Duration::from_secs(if on { 10 } else { 2 });
    let deadline = Instant::now() + wait;
    let mut subscribed = false;
    while Instant::now() < deadline {
        if !bus.subscribers(VoiceState::TOPIC).unwrap().is_empty() {
            subscribed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut voice = bus
        .publisher_declared(VoiceState::TOPIC, &VoiceState::declaration(), "voice")
        .unwrap();
    for _ in 0..3 {
        voice
            .publish_json(1, &serde_json::json!({ "state": "resting" }))
            .unwrap();
        std::thread::sleep(Duration::from_millis(300));
    }
    let mut written = Vec::new();
    while let Some(message) = proposals.recv(Duration::from_millis(500)).unwrap() {
        written.push(message.parse::<AddonProposal>().unwrap());
    }
    let mut seen = Vec::new();
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
    assert_eq!(proposal.addon, "quiet-note");
    assert_eq!(proposal.version, "1.2.0");
    assert_eq!(proposal.act, "note");
    assert_eq!(proposal.note.as_deref(), Some("The room is quiet."));
    assert_eq!(proposal.check(), Ok(()));
    let said: Vec<&str> = events.iter().map(|event| event.why.as_str()).collect();
    assert!(
        said.iter().any(|why| why.starts_with("proposed note as")),
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
